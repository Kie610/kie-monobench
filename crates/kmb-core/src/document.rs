use crate::recompute::{self, Cache, RecomputeReport};
use crate::transaction::{self, Op, Transaction, TxError};
use crate::{FeatureId, Kernel, LengthUnit, ParamId, Registry, ShapeHandle};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::mpsc::{Receiver, Sender, channel};

/// 保存形式の版。読み込めない版のファイルは拒否する。
/// 版 2(2026-10-14): タイムラインのグループ(`groups`)を足した。
/// 版 3(2026-10-19): パラメータの表(`param_views`)を足した。
/// 版 4(2026-10-23): モジュールごとの区画(`sections`)を足し、モーション スタディ(`motion_studies`)と
/// 接触セット(`contacts`)を区画 `asm.motion`・`asm.contacts` へ移した。
/// 版 5(2026-10-24): 構成表・材料・外観・アニメーションの段を、それぞれのモジュールの区画へ移した。
pub const FORMAT_VERSION: u32 = 6;

/// 文書のファイルの拡張子(点を付けない)。保存・開く・リンク・一覧はこれを使い、字で書かない
/// (点検: 文書の拡張子)。製品名(kieMonoBench)から `kmb`。中身は今までどおり JSON。
pub const DOCUMENT_EXTENSION: &str = "kmb";

/// 文書として開く拡張子。拡張子を替えた時は、前の物もここに残して古い文書を開けるようにする
/// (`json` は改名の前の拡張子)。
pub const DOCUMENT_EXTENSIONS: &[&str] = &[DOCUMENT_EXTENSION, "json"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Param {
    pub id: ParamId,
    pub name: String,
    pub expr: String,
    /// パラメータの表のコメントの列(何のための値か)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
    /// 単位。数だけの式に補い、式の次元がそれに合うかを確かめる
    #[serde(default, skip_serializing_if = "ParamUnit::is_any")]
    pub unit: ParamUnit,
}

impl Param {
    /// 単位もコメントもないパラメータ。
    pub fn new(id: ParamId, name: impl Into<String>, expr: impl Into<String>) -> Self {
        Param {
            id,
            name: name.into(),
            expr: expr.into(),
            comment: String::new(),
            unit: ParamUnit::Any,
        }
    }

    /// 計算する式: 数だけの式に単位を補う(`10` → `10 mm`)。
    pub fn completed_expr(&self) -> String {
        match self.unit.symbol() {
            Some(u) if self.expr.trim().parse::<f64>().is_ok() => {
                format!("{} {u}", self.expr.trim())
            }
            _ => self.expr.clone(),
        }
    }
}

/// パラメータの単位(長さか角度か、単位なしか)。表の列で選ぶ。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamUnit {
    /// 決めていない(単位の列ができる前のパラメータ。式のままの次元を使う)
    #[default]
    Any,
    /// 長さ(数だけなら mm)
    Length,
    /// 角度(数だけなら deg)
    Angle,
    /// 単位なしの数(個数・比など)
    Unitless,
}

impl ParamUnit {
    pub const ALL: [ParamUnit; 4] = [
        ParamUnit::Length,
        ParamUnit::Angle,
        ParamUnit::Unitless,
        ParamUnit::Any,
    ];

    pub fn is_any(&self) -> bool {
        *self == ParamUnit::Any
    }

    /// 数だけの式に補う単位。
    pub fn symbol(self) -> Option<&'static str> {
        match self {
            ParamUnit::Length => Some("mm"),
            ParamUnit::Angle => Some("deg"),
            _ => None,
        }
    }

    /// 表に出す名前。
    pub fn label(self) -> &'static str {
        match self {
            ParamUnit::Any => "未指定",
            ParamUnit::Length => "mm",
            ParamUnit::Angle => "deg",
            ParamUnit::Unitless => "単位なし",
        }
    }

    /// 式の値の次元が、この単位に合うか。合わなければ理由。
    pub fn check(self, dim: kmb_expr::Dim) -> Result<(), String> {
        let (want, what) = match self {
            ParamUnit::Any => return Ok(()),
            ParamUnit::Length => (kmb_expr::Dim::LENGTH, "長さ(mm)"),
            ParamUnit::Angle => (kmb_expr::Dim::ANGLE, "角度(deg)"),
            ParamUnit::Unitless => (kmb_expr::Dim::NONE, "単位なし"),
        };
        if dim == want {
            Ok(())
        } else {
            let got = match dim {
                kmb_expr::Dim::NONE => "単位のない数".to_string(),
                kmb_expr::Dim::LENGTH => "長さ".to_string(),
                kmb_expr::Dim::ANGLE => "角度".to_string(),
                d => d.to_string(),
            };
            Err(format!(
                "単位が合いません: {what}のパラメータに{got}の式です"
            ))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureEntry {
    pub id: FeatureId,
    /// 例: `"cad.extrude@1"`。`@` の後ろは機能ごとのデータの版
    pub type_id: String,
    pub name: String,
    pub data: serde_json::Value,
    #[serde(default)]
    pub suppressed: bool,
    /// 真なら、読むボディをデータの機能の番号の形のまま読む(保存の形の版 5 までの読み方)。
    /// 偽(新しく足す機能)なら、その手前でのそのボディの最新の形を読む([`crate::BodyTable`])
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pin_bodies: bool,
}

/// タイムラインのグループ。`first` から `last` までの続いた機能をまとめる。
/// グループは入れ子にできるが、互いに一部だけ重なることはない。計算には使わない(見せ方だけ)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Group {
    /// 機能・パラメータと同じ通し番号から割り当てる
    pub id: u64,
    pub name: String,
    pub first: FeatureId,
    pub last: FeatureId,
}

/// パラメータの表の 1 行が指す物。名前を変えても指す先が変わらないように、ID と鍵で指す。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ParamKey {
    /// 利用者のパラメータ
    User { id: ParamId },
    /// 機能の寸法(モデルのパラメータ。`key` は [`crate::ModelParam::key`]。スケッチなら拘束の ID)
    Model { feature: FeatureId, key: String },
    /// 機能の値(記述子の式の欄。`key` は欄の名前。行の並びの欄は `並び.行番号.欄`)
    Field { feature: FeatureId, key: String },
}

/// パラメータの表(データベースのビューのように、同じパラメータを見る窓。写しは持たない)。
/// 出す行は、手で入れた行(`members`)と、決まりで入る行(`body` に関わる物)を足し、外した行
/// (`hidden`)を除いた物。表の中で直すと、元のパラメータ・寸法・機能の値を直す。
/// 指す先が消えた行は、表に出さないだけで残す(取り消しで戻った時にまた出る)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParamView {
    /// 機能・パラメータと同じ通し番号から割り当てる
    pub id: u64,
    pub name: String,
    /// 手で入れた行(並べた順)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<ParamKey>,
    /// このボディ(最初に作った機能)に関わる行を決まりで入れる
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<FeatureId>,
    /// この表から外した行(決まりで入る行も外す)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden: Vec<ParamKey>,
    /// お気に入りの表(U52。文書に 1 つまで)。タブは「全部」の次に「お気に入り」として出し、
    /// 名前の変更と削除はさせない。行の ☆ で入れる・外す
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub favorites: bool,
}

/// 設計の属性(部品名・部品番号など)。3MF の Title・Designer・Description・Copyright・
/// LicenseTerms、STEP の頭の FILE_NAME・FILE_DESCRIPTION に倣う。
/// 計算には使わない。G-code の頭の注などに流す。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Properties {
    /// 部品名(3MF の Title、STEP の FILE_NAME の name)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// 部品番号
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub part_number: String,
    /// 改訂
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub revision: String,
    /// 作者(3MF の Designer、STEP の author)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub designer: String,
    /// 所属(STEP の organization)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub organization: String,
    /// 説明(3MF の Description、STEP の FILE_DESCRIPTION)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// 材料(文字。加工のストックの材料とは別)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub material: String,
    /// 著作権(3MF の Copyright)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub copyright: String,
    /// 使用条件(3MF の LicenseTerms)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub license: String,
    /// 作品(フォルダとは別に、同じ物を作るためのデザインをまとめる札。モデル管理の分類。
    /// 保存の鍵は前の呼び名「プロジェクト」の `project` のまま)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub project: String,
    /// ジャンル(治具・家具・小物など。モデル管理の分類)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub genre: String,
    /// タグ(自由に付ける分類。並べた順、同じ物は 1 つ)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// 自分で足す項目(名前と値。並べた順)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom: Vec<(String, String)>,
    /// 文書の長さの単位(画面に出す・数だけ打った時の単位。計算と保存は mm のまま。候補 19)
    #[serde(default, skip_serializing_if = "LengthUnit::is_mm")]
    pub units: LengthUnit,
}

impl Properties {
    /// 決まった項目の(鍵, 画面の名前)。並べる順。
    pub const FIELDS: [(&'static str, &'static str); 11] = [
        ("title", "部品名"),
        ("project", "作品"),
        ("genre", "ジャンル"),
        ("part_number", "部品番号"),
        ("revision", "改訂"),
        ("designer", "作者"),
        ("organization", "所属"),
        ("description", "説明"),
        ("material", "材料"),
        ("copyright", "著作権"),
        ("license", "使用条件"),
    ];

    pub fn is_empty(&self) -> bool {
        *self == Properties::default()
    }

    /// 決まった項目の値(知らない鍵は `None`)。
    pub fn get(&self, key: &str) -> Option<&str> {
        Some(match key {
            "title" => &self.title,
            "part_number" => &self.part_number,
            "revision" => &self.revision,
            "designer" => &self.designer,
            "organization" => &self.organization,
            "description" => &self.description,
            "material" => &self.material,
            "copyright" => &self.copyright,
            "license" => &self.license,
            "project" => &self.project,
            "genre" => &self.genre,
            "units" => self.units.name(),
            _ => return None,
        })
    }

    /// 決まった項目の値への参照(知らない鍵は `None`)。
    pub fn get_mut(&mut self, key: &str) -> Option<&mut String> {
        Some(match key {
            "title" => &mut self.title,
            "part_number" => &mut self.part_number,
            "revision" => &mut self.revision,
            "designer" => &mut self.designer,
            "organization" => &mut self.organization,
            "description" => &mut self.description,
            "material" => &mut self.material,
            "copyright" => &mut self.copyright,
            "license" => &mut self.license,
            "project" => &mut self.project,
            "genre" => &mut self.genre,
            _ => return None,
        })
    }

    /// 鍵(決まった項目の鍵か、自分で足した項目の名前)で値を入れる。空の値は消す。
    /// 鍵 `tags` はタグ([`Properties::set_tags_text`] と同じく「,」で区切る)。
    pub fn set(&mut self, key: &str, value: &str) {
        if key == "tags" {
            self.set_tags_text(value);
            return;
        }
        // 単位は名前で(空なら mm に戻す。読めない名前は変えない。確かめは呼ぶ側で)
        if key == "units" {
            if value.trim().is_empty() {
                self.units = LengthUnit::Mm;
            } else if let Some(u) = LengthUnit::parse(value) {
                self.units = u;
            }
            return;
        }
        let value = value.trim().to_string();
        if let Some(v) = self.get_mut(key) {
            *v = value;
            return;
        }
        match self.custom.iter().position(|(k, _)| k == key) {
            Some(i) if value.is_empty() => {
                self.custom.remove(i);
            }
            Some(i) => self.custom[i].1 = value,
            None if !value.is_empty() && !key.trim().is_empty() => {
                self.custom.push((key.trim().to_string(), value))
            }
            None => {}
        }
    }

    /// タグを文字から入れる(「,」「、」で区切る。前後の空白を落とし、空と重なりは捨てる)。
    pub fn set_tags_text(&mut self, text: &str) {
        self.tags.clear();
        for t in text.split([',', '、', '，']) {
            let t = t.trim();
            if !t.is_empty() && !self.tags.iter().any(|x| x == t) {
                self.tags.push(t.to_string());
            }
        }
    }

    /// タグを 1 つの文字に(「, 」で区切る)。
    pub fn tags_text(&self) -> String {
        self.tags.join(", ")
    }

    /// G-code の頭の注に入れる行(部品名・部品番号・改訂・材料。空の項目は出さない)。
    pub fn gcode_lines(&self) -> Vec<String> {
        [
            ("部品名", &self.title),
            ("部品番号", &self.part_number),
            ("改訂", &self.revision),
            ("材料", &self.material),
        ]
        .iter()
        .filter(|(_, v)| !v.trim().is_empty())
        .map(|(k, v)| format!("{k} {}", v.trim()))
        .collect()
    }
}

/// 名前を付けた視点(U20)。画面の真ん中の点・向き・見える高さを持ち、ビューの大きさに依らない。
/// 向きは Z が上の回し方(ヨーとピッチ。画面のカメラと同じ)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedView {
    /// 機能・パラメータと同じ通し番号から割り当てる
    pub id: u64,
    pub name: String,
    /// 画面の真ん中に来る点(世界座標、mm)
    pub target: [f64; 3],
    /// 水平の回り(度。X 軸から反時計回り)
    pub yaw: f64,
    /// 見下ろす角度(度。-90〜90)
    pub pitch: f64,
    /// 画面の上での傾き(度。視線のまわり。上を Z に保って回すなら 0)
    #[serde(default)]
    pub roll: f64,
    /// 画面の縦に見える高さ(mm)
    pub height: f64,
    /// 透視投影で見るか(偽なら平行投影)
    #[serde(default)]
    pub perspective: bool,
}

/// 部品。ボディをまとめて名前と色を付け、入れ子にする。ボディは
/// 最初に作った機能(`body_names` と同じ鍵)で指し、1 つのボディは 1 つの部品にだけ入る。
/// 部品に入っていないボディは文書そのもの(一番上の部品)のボディ。形の計算には使わない
/// (ジョイントや部品表が、どのボディが一緒に動くか・何個あるかを知るのに使う)。
/// 指すボディが消えても部品は残す(取り消しで戻った時にまた入る)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Component {
    /// 機能・パラメータと同じ通し番号から割り当てる
    pub id: u64,
    pub name: String,
    /// 親の部品(`None` なら文書の直下)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<u64>,
    /// 入っているボディ(最初に作った機能。並べた順)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bodies: Vec<FeatureId>,
}

/// 保存される文書の中身。形状などの派生データは含まない。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentData {
    pub format_version: u32,
    /// 次に割り当てる ID。ID は再利用しない
    pub next_id: u64,
    pub params: Vec<Param>,
    pub features: Vec<FeatureEntry>,
    /// `Some(n)` なら先頭から n 個の機能だけを有効にする(ロールバック)
    #[serde(default)]
    pub rollback: Option<usize>,
    /// タイムラインのグループ(外側のものが先)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<Group>,
    /// 利用者が付けたボディの名前。鍵はボディを最初に作った機能(後から機能を足しても変わらない)。
    /// 無いボディは画面が「ボディN」と出す
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub body_names: std::collections::BTreeMap<FeatureId, String>,
    /// パラメータの表(「全部」の表は持たない。いつも先頭に出す決まりの表)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_views: Vec<ParamView>,
    /// 設計の属性(U13。無い文書は空)
    #[serde(default, skip_serializing_if = "Properties::is_empty")]
    pub properties: Properties,
    /// 名前を付けた視点(U20。ブラウザの「名前の付いたビュー」に並べた順)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub named_views: Vec<NamedView>,
    /// 消したパラメータ(消した順)。パラメータの表の「消した物も出す」で薄く出し、
    /// 「戻す」で戻せる。番号は使い回さないので、戻すと前と同じ番号になる
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed_params: Vec<Param>,
    /// 部品の木(親が先とは限らない)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<Component>,
    /// モジュールごとの区画(鍵 → 版と中身。[`crate::section`])。知らない区画も読んだまま書き戻す
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub sections: std::collections::BTreeMap<String, crate::section::Section>,
}

impl Default for DocumentData {
    fn default() -> Self {
        DocumentData {
            format_version: FORMAT_VERSION,
            next_id: 1,
            params: Vec::new(),
            features: Vec::new(),
            rollback: None,
            groups: Vec::new(),
            body_names: Default::default(),
            param_views: Vec::new(),
            properties: Properties::default(),
            named_views: Vec::new(),
            removed_params: Vec::new(),
            components: Vec::new(),
            sections: Default::default(),
        }
    }
}

impl DocumentData {
    /// 有効な機能の数。
    pub fn active_len(&self) -> usize {
        self.rollback
            .map_or(self.features.len(), |n| n.min(self.features.len()))
    }

    pub fn param_by_name(&self, name: &str) -> Option<&Param> {
        self.params.iter().find(|p| p.name == name)
    }

    pub fn feature(&self, id: FeatureId) -> Option<&FeatureEntry> {
        self.features.iter().find(|f| f.id == id)
    }

    pub fn index_of(&self, id: FeatureId) -> Option<usize> {
        self.features.iter().position(|f| f.id == id)
    }

    pub fn named_view(&self, id: u64) -> Option<&NamedView> {
        self.named_views.iter().find(|v| v.id == id)
    }

    pub fn param_view(&self, id: u64) -> Option<&ParamView> {
        self.param_views.iter().find(|v| v.id == id)
    }

    /// お気に入りの表(まだ ☆ を付けていなければ `None`)。
    pub fn favorites_view(&self) -> Option<&ParamView> {
        self.param_views.iter().find(|v| v.favorites)
    }

    /// 消したパラメータ `id`。
    pub fn removed_param(&self, id: ParamId) -> Option<&Param> {
        self.removed_params.iter().find(|p| p.id == id)
    }

    pub fn component(&self, id: u64) -> Option<&Component> {
        self.components.iter().find(|c| c.id == id)
    }

    /// ボディ(最初に作った機能 `root`)が入っている部品。
    pub fn component_of(&self, root: FeatureId) -> Option<&Component> {
        self.components.iter().find(|c| c.bodies.contains(&root))
    }

    /// 部品 `id` の子の部品(並べた順)。`None` なら文書の直下の部品。
    pub fn child_components(&self, id: Option<u64>) -> Vec<&Component> {
        self.components.iter().filter(|c| c.parent == id).collect()
    }

    /// 部品 `id` と、その中の部品すべて(`id` が先、深さ優先)。
    pub fn component_tree(&self, id: u64) -> Vec<u64> {
        let mut out = vec![id];
        let mut i = 0;
        while i < out.len() {
            let cur = out[i];
            for c in &self.components {
                if c.parent == Some(cur) && !out.contains(&c.id) {
                    out.push(c.id);
                }
            }
            i += 1;
        }
        out
    }

    /// 部品 `id` と中の部品に入っているボディすべて(最初に作った機能)。
    pub fn component_bodies(&self, id: u64) -> Vec<FeatureId> {
        self.component_tree(id)
            .into_iter()
            .filter_map(|c| self.component(c))
            .flat_map(|c| c.bodies.iter().copied())
            .collect()
    }

    /// ボディ `roots` を部品 `to` へ移した部品の木(`None` なら部品から外して文書の直下へ)。
    pub fn components_moving(&self, roots: &[FeatureId], to: Option<u64>) -> Vec<Component> {
        let mut list = self.components.clone();
        for c in &mut list {
            c.bodies.retain(|b| !roots.contains(b));
        }
        if let Some(c) = to.and_then(|t| list.iter_mut().find(|c| c.id == t)) {
            c.bodies.extend(roots.iter().copied());
        }
        list
    }

    /// 部品 `id` を解いた部品の木: 中のボディと子の部品は、親の部品(なければ文書の直下)へ移す。
    pub fn components_without(&self, id: u64) -> Vec<Component> {
        let Some(gone) = self.component(id).cloned() else {
            return self.components.clone();
        };
        let mut list: Vec<Component> = self
            .components
            .iter()
            .filter(|c| c.id != id)
            .cloned()
            .collect();
        for c in &mut list {
            if c.parent == Some(id) {
                c.parent = gone.parent;
            }
        }
        if let Some(p) = gone
            .parent
            .and_then(|p| list.iter_mut().find(|c| c.id == p))
        {
            p.bodies.extend(gone.bodies.iter().copied());
        }
        list
    }

    pub fn group(&self, id: u64) -> Option<&Group> {
        self.groups.iter().find(|g| g.id == id)
    }

    /// グループが占めるタイムラインの範囲(先頭と末尾の位置)。
    pub fn group_range(&self, g: &Group) -> Option<(usize, usize)> {
        Some((self.index_of(g.first)?, self.index_of(g.last)?))
    }

    /// 機能 `id` を含むグループ(外側から順)。
    pub fn groups_of(&self, id: FeatureId) -> Vec<&Group> {
        let Some(i) = self.index_of(id) else {
            return Vec::new();
        };
        let mut v: Vec<(usize, &Group)> = self
            .groups
            .iter()
            .filter_map(|g| {
                let (a, b) = self.group_range(g)?;
                (a <= i && i <= b).then_some((b - a, g))
            })
            .collect();
        // 広いものほど外側
        v.sort_by_key(|(w, _)| std::cmp::Reverse(*w));
        v.into_iter().map(|(_, g)| g).collect()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DocEvent {
    Changed { label: String },
    Undone { label: String },
    Redone { label: String },
    Recomputed { errors: usize },
}

#[derive(Debug)]
pub enum LoadError {
    Json(serde_json::Error),
    UnsupportedVersion(u32),
    Invalid(TxError),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Json(e) => write!(f, "JSON として読めません: {e}"),
            LoadError::UnsupportedVersion(v) if *v > FORMAT_VERSION => write!(
                f,
                "新しい版のアプリで保存された文書です(保存形式の版 {v}、このアプリは {FORMAT_VERSION} まで)。アプリを更新してください"
            ),
            LoadError::UnsupportedVersion(v) => {
                write!(
                    f,
                    "保存形式の版 {v} には対応していません(対応: {FORMAT_VERSION})"
                )
            }
            LoadError::Invalid(e) => write!(f, "文書の内容が不正です: {e}"),
        }
    }
}

impl std::error::Error for LoadError {}

/// 編集中の文書。変更はすべて [`Document::apply`] を通り、取り消せる。
#[derive(Default)]
pub struct Document {
    data: DocumentData,
    undo: Vec<Transaction>,
    redo: Vec<Transaction>,
    subscribers: Vec<Sender<DocEvent>>,
    cache: Cache,
    /// 前に [`Document::settle_names`] で見たモデルのパラメータ(名前 → 式)。消えた寸法を
    /// 参照していた式を、その式で置き換えるのに使う
    known: Option<std::collections::HashMap<String, String>>,
    /// 読んだファイルの上の階にあった、このアプリの知らない項目(新しいアプリが足した物)。
    /// 保存の時に読んだまま書き戻す(少し古いアプリで保存しても消えないように)
    extra: serde_json::Map<String, serde_json::Value>,
}

impl Document {
    pub fn new() -> Self {
        Self::default()
    }

    /// 設計の属性を初めから持った新しい文書(新しい文書の既定の値。取り消しの履歴には入れない。候補 19)。
    pub fn with_properties(properties: Properties) -> Self {
        let mut d = Self::default();
        d.data.properties = properties;
        d
    }

    pub fn data(&self) -> &DocumentData {
        &self.data
    }

    pub fn alloc_param_id(&mut self) -> ParamId {
        ParamId(self.alloc())
    }

    pub fn alloc_feature_id(&mut self) -> FeatureId {
        FeatureId(self.alloc())
    }

    /// グループの番号([`Group::id`])を割り当てる。
    pub fn alloc_group_id(&mut self) -> u64 {
        self.alloc()
    }

    /// パラメータの表の番号([`ParamView::id`])を割り当てる。
    pub fn alloc_view_id(&mut self) -> u64 {
        self.alloc()
    }

    fn alloc(&mut self) -> u64 {
        let id = self.data.next_id;
        self.data.next_id += 1;
        id
    }

    /// 変更を適用する。途中で失敗した場合は何も変えずに誤りを返す。
    pub fn apply(&mut self, tx: Transaction) -> Result<(), TxError> {
        let label = tx.label.clone();
        let tx = transaction::follow_rows(&self.data, tx);
        let inverse = transaction::apply(&mut self.data, tx)
            .inspect_err(|e| log::warn!("変更「{label}」を適用できない: {e}"))?;
        log::info!("変更: {label}");
        self.undo.push(inverse);
        self.redo.clear();
        self.emit(DocEvent::Changed { label });
        Ok(())
    }

    /// 操作の結果の変更を 1 回の取り消し単位で適用する。足した機能の ID を順に返す。
    /// データの `{"$new": k}` は、この変更で k 番目に足す機能の ID に置き換える。
    pub fn apply_edits(
        &mut self,
        label: &str,
        edits: Vec<crate::Edit>,
    ) -> Result<Vec<FeatureId>, TxError> {
        let mut added = Vec::new();
        // 部品の木を何度も変える時は、前の変更の後の木に重ねる
        let mut components = self.data.components.clone();
        let ops = edits
            .into_iter()
            .flat_map(|e| match e {
                crate::Edit::Insert {
                    type_id,
                    name,
                    mut data,
                } => {
                    let id = self.alloc_feature_id();
                    resolve_new(&mut data, &added, id);
                    added.push(id);
                    vec![Op::InsertFeature {
                        index: None,
                        entry: FeatureEntry {
                            id,
                            type_id,
                            name,
                            data,
                            suppressed: false,
                            pin_bodies: false,
                        },
                    }]
                }
                crate::Edit::SetData { id, data } => vec![Op::SetFeatureData { id, data }],
                crate::Edit::SetSuppressed { id, suppressed } => {
                    vec![Op::SetSuppressed { id, suppressed }]
                }
                crate::Edit::SetPinBodies { id, pin } => vec![Op::SetPinBodies { id, pin }],
                crate::Edit::SetParamExpr { id, expr } => vec![Op::SetParamExpr { id, expr }],
                crate::Edit::SetComponents {
                    components: mut list,
                } => {
                    for c in &mut list {
                        if c.id == 0 {
                            c.id = self.alloc();
                        }
                    }
                    components = list.clone();
                    vec![Op::SetComponents { components: list }]
                }
                crate::Edit::AddBodies {
                    component,
                    name,
                    parent,
                    bodies,
                } => {
                    let mut ops = Vec::new();
                    let mut roots = Vec::new();
                    for (k, body) in bodies {
                        // 範囲の外は、適用の検査で断られる番号(0)にする
                        let root = added.get(k).copied().unwrap_or(FeatureId(0));
                        roots.push(root);
                        ops.push(Op::RenameBody {
                            root,
                            name: Some(body),
                        });
                    }
                    match component.and_then(|c| components.iter_mut().find(|x| x.id == c)) {
                        Some(c) => c.bodies.extend(roots),
                        None => {
                            let id = self.alloc();
                            components.push(crate::Component {
                                id,
                                name,
                                parent,
                                bodies: roots,
                            });
                        }
                    }
                    ops.push(Op::SetComponents {
                        components: components.clone(),
                    });
                    ops
                }
                crate::Edit::SetSection { key, section } => vec![Op::SetSection { key, section }],
                crate::Edit::SetRollback { rollback } => vec![Op::SetRollback { rollback }],
            })
            .collect();
        self.apply(Transaction::new(label, ops))?;
        Ok(added)
    }

    /// パラメータ名を変え、それを参照する式(パラメータと機能)も同じ取り消し単位で書き換える。
    pub fn rename_param(
        &mut self,
        registry: &Registry,
        id: ParamId,
        new_name: &str,
    ) -> Result<(), TxError> {
        let old = self
            .data
            .params
            .iter()
            .find(|p| p.id == id)
            .ok_or(TxError::NoSuchParam(id))?
            .name
            .clone();
        // 寸法の名前(モデルのパラメータ)と重ねない
        if crate::model_params(&self.data, registry)
            .iter()
            .any(|(_, p)| p.name == new_name)
        {
            return Err(TxError::DuplicateName(new_name.into()));
        }
        let mut ops = vec![Op::RenameParam {
            id,
            name: new_name.to_string(),
        }];
        ops.extend(self.rename_ops(registry, &old, new_name)?);
        self.apply(Transaction::new(
            format!("パラメータ名を {new_name} に変更"),
            ops,
        ))
    }

    /// モデルのパラメータ(スケッチの寸法の `d1` など)の名前をそろえる。名前の無い寸法に
    /// 名前を付け、文書の中で重なった名前を付け直す。消えた寸法を参照していた式は、
    /// その寸法の式で置き換える(`d2 = d1 * 2` で d1 を消すと `d2 = (w / 2) * 2`)。
    /// 変えたものは直前の変更と同じ取り消し単位に入れる(取り消すと一緒に戻る)。
    /// 再計算の前に呼ぶ([`Document::recompute`] は自分で呼ぶ)。変えたら true。
    pub fn settle_names(&mut self, registry: &Registry) -> bool {
        use std::collections::HashMap;
        let data = &self.data;
        let mut names = crate::Names::new(data.params.iter().map(|p| p.name.as_str()));
        // 機能ごとの新しいデータ(名前を付けたもの、参照を置き換えたもの)
        let mut changed: Vec<(FeatureId, serde_json::Value)> = Vec::new();
        let mut features: Vec<(FeatureId, Box<dyn crate::Feature>)> = Vec::new();
        for f in &data.features {
            let Some(Ok(feature)) = registry.instantiate(&f.type_id, &f.data) else {
                continue;
            };
            match feature.name_model_params(&mut names) {
                Some(new) => match registry.instantiate(&f.type_id, &new) {
                    Some(Ok(g)) => {
                        changed.push((f.id, new));
                        features.push((f.id, g));
                    }
                    _ => features.push((f.id, feature)),
                },
                None => features.push((f.id, feature)),
            }
        }
        let now: HashMap<String, String> = features
            .iter()
            .flat_map(|(_, f)| f.model_params())
            .map(|p| (p.name, p.expr))
            .collect();
        // 取り消しの後は前の状態に戻っただけなので、置き換えない
        let gone: Vec<(String, String)> = match (&self.known, self.redo.is_empty()) {
            (Some(known), true) => known
                .iter()
                .filter(|(n, _)| !now.contains_key(*n) && data.param_by_name(n).is_none())
                .map(|(n, e)| (n.clone(), e.clone()))
                .collect(),
            _ => Vec::new(),
        };
        let mut params: HashMap<ParamId, String> = HashMap::new();
        if !gone.is_empty() {
            let mut exprs: Vec<(ParamId, String)> =
                data.params.iter().map(|p| (p.id, p.expr.clone())).collect();
            // 置き換えた式がまた消えた名前を含むこともあるので、数回繰り返す
            for _ in 0..8 {
                let mut any = false;
                for (name, expr) in &gone {
                    let with = substitute_text(expr);
                    for (id, e) in &mut exprs {
                        if let Ok(r) = kmb_expr::rename_reference(e, name, &with)
                            && r != *e
                        {
                            *e = r.clone();
                            params.insert(*id, r);
                            any = true;
                        }
                    }
                    for (id, f) in &mut features {
                        let Ok(Some(new)) = f.rename_param(name, &with) else {
                            continue;
                        };
                        let Some(type_id) = data.feature(*id).map(|e| e.type_id.clone()) else {
                            continue;
                        };
                        if let Some(Ok(g)) = registry.instantiate(&type_id, &new) {
                            *f = g;
                            changed.retain(|(c, _)| c != id);
                            changed.push((*id, new));
                            any = true;
                        }
                    }
                }
                if !any {
                    break;
                }
            }
            if !params.is_empty() || !changed.is_empty() {
                let names: Vec<&str> = gone.iter().map(|(n, _)| n.as_str()).collect();
                log::info!(
                    "消えた寸法 {} を参照していた式を、その式で置き換えた",
                    names.join(", ")
                );
            }
        }
        self.known = Some(now);
        if !self.redo.is_empty() || (changed.is_empty() && params.is_empty()) {
            return false;
        }
        let ops: Vec<Op> = changed
            .into_iter()
            .map(|(id, data)| Op::SetFeatureData { id, data })
            .chain(
                params
                    .into_iter()
                    .map(|(id, expr)| Op::SetParamExpr { id, expr }),
            )
            .collect();
        self.amend(ops)
    }

    /// 変更を直前の変更と同じ取り消し単位に足して適用する(直前の変更が無ければ記録しない)。
    fn amend(&mut self, ops: Vec<Op>) -> bool {
        match transaction::apply(&mut self.data, Transaction::new("寸法の名前", ops)) {
            Ok(inverse) => {
                if let Some(last) = self.undo.last_mut() {
                    // 足した変更を先に戻してから、元の変更を戻す
                    let mut ops = inverse.ops;
                    ops.append(&mut last.ops);
                    last.ops = ops;
                }
                true
            }
            Err(e) => {
                log::warn!("寸法の名前をそろえられない: {e}");
                false
            }
        }
    }

    /// モデルのパラメータ(スケッチの寸法の `d1` など)の名前を変え、それを参照する式も
    /// 同じ取り消し単位で書き換える。
    pub fn rename_model_param(
        &mut self,
        registry: &Registry,
        old: &str,
        new_name: &str,
    ) -> Result<(), TxError> {
        if !kmb_expr::is_valid_name(new_name) {
            return Err(TxError::InvalidName(new_name.into()));
        }
        let model = crate::model_params(&self.data, registry);
        if !model.iter().any(|(_, p)| p.name == old) {
            return Err(TxError::InvalidName(old.into()));
        }
        if self.data.param_by_name(new_name).is_some()
            || model.iter().any(|(_, p)| p.name == new_name)
        {
            return Err(TxError::DuplicateName(new_name.into()));
        }
        let ops = self.rename_ops(registry, old, new_name)?;
        self.apply(Transaction::new(
            format!("寸法の名前を {old} から {new_name} に変更"),
            ops,
        ))?;
        if let Some(known) = &mut self.known
            && let Some(e) = known.remove(old)
        {
            known.insert(new_name.to_string(), e);
        }
        Ok(())
    }

    /// モデルのパラメータ `name` の式を変える(パラメータの一覧から寸法の値を直す)。
    pub fn set_model_param(
        &mut self,
        registry: &Registry,
        name: &str,
        expr: &str,
    ) -> Result<(), TxError> {
        kmb_expr::parse(expr).map_err(|e| TxError::BadExpr {
            expr: expr.into(),
            message: e.to_string(),
        })?;
        let found = self.data.features.iter().find_map(|f| {
            let Some(Ok(feature)) = registry.instantiate(&f.type_id, &f.data) else {
                return None;
            };
            feature.set_model_param(name, expr).map(|d| (f.id, d))
        });
        let Some((id, data)) = found else {
            return Err(TxError::InvalidName(name.into()));
        };
        self.apply(Transaction::new(
            format!("{name} を {expr} に変更"),
            vec![Op::SetFeatureData { id, data }],
        ))
    }

    /// 名前 `old` を `new_name` にする書き換え(パラメータの式と機能)。
    fn rename_ops(
        &self,
        registry: &Registry,
        old: &str,
        new_name: &str,
    ) -> Result<Vec<Op>, TxError> {
        let mut ops = Vec::new();
        for p in &self.data.params {
            let expr = kmb_expr::rename_reference(&p.expr, old, new_name).map_err(|e| {
                TxError::BadExpr {
                    expr: p.expr.clone(),
                    message: e.to_string(),
                }
            })?;
            if expr != p.expr {
                ops.push(Op::SetParamExpr { id: p.id, expr });
            }
        }
        for f in &self.data.features {
            // 組み立てられない機能(未登録・データ不正)は書き換えずに残す
            let Some(Ok(feature)) = registry.instantiate(&f.type_id, &f.data) else {
                continue;
            };
            match feature.rename_param(old, new_name) {
                Ok(Some(data)) => ops.push(Op::SetFeatureData { id: f.id, data }),
                Ok(None) => {}
                Err(e) => {
                    return Err(TxError::BadExpr {
                        expr: f.name.clone(),
                        message: e.to_string(),
                    });
                }
            }
        }
        Ok(ops)
    }

    /// 取り消しとやり直しの履歴を消す。文書を作り終えた直後などに使う。
    pub fn clear_history(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    /// やり直しの履歴だけを消す。作っている途中でやめた変更を、やり直しで戻せないようにする
    /// (作り始めた時点でやり直しの履歴は空なので、消えるのはやめた変更だけ)。
    pub fn clear_redo(&mut self) {
        self.redo.clear();
    }

    /// 取り消せる変更の数。まとめて実行する時の始めの位置に使う([`Document::rollback_to`])。
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    /// 取り消せる変更が `depth` 個になるまで取り消し、取り消した分はやり直せないようにする
    /// (まとめて実行の途中で失敗した時に、始めの形へ戻す)。取り消した数を返す。
    pub fn rollback_to(&mut self, depth: usize) -> usize {
        let mut n = 0;
        while self.undo.len() > depth && self.undo() {
            n += 1;
        }
        if n > 0 {
            self.redo.clear();
        }
        n
    }

    /// `depth` 個より後の変更を 1 つの取り消し単位 `label` にまとめる(まとめて実行が通った時)。
    /// まとめた数を返す。
    pub fn squash_since(&mut self, depth: usize, label: &str) -> usize {
        if self.undo.len() <= depth + 1 {
            return self.undo.len().saturating_sub(depth);
        }
        let tail = self.undo.split_off(depth);
        let n = tail.len();
        // 取り消しは新しい変更から順に戻す
        let ops = tail.into_iter().rev().flat_map(|t| t.ops).collect();
        self.undo.push(Transaction::new(label, ops));
        n
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// 取り消せる変更の名前(古い順)。最後のものが次に取り消される。
    pub fn undo_labels(&self) -> Vec<&str> {
        self.undo.iter().map(|t| t.label.as_str()).collect()
    }

    /// やり直せる変更の名前(次にやり直すものが先頭)。
    pub fn redo_labels(&self) -> Vec<&str> {
        self.redo.iter().rev().map(|t| t.label.as_str()).collect()
    }

    /// 直前の変更を取り消す。取り消すものがなければ false。
    pub fn undo(&mut self) -> bool {
        let Some(tx) = self.undo.pop() else {
            return false;
        };
        let label = tx.label.clone();
        let inverse = transaction::apply(&mut self.data, tx)
            .expect("取り消しの操作は、記録した時点の文書に対して必ず適用できる");
        self.redo.push(inverse);
        log::info!("取り消し: {label}");
        self.emit(DocEvent::Undone { label });
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(tx) = self.redo.pop() else {
            return false;
        };
        let label = tx.label.clone();
        let inverse = transaction::apply(&mut self.data, tx)
            .expect("やり直しの操作は、記録した時点の文書に対して必ず適用できる");
        self.undo.push(inverse);
        log::info!("やり直し: {label}");
        self.emit(DocEvent::Redone { label });
        true
    }

    /// 読むボディを固定する印([`FeatureEntry::pin_bodies`])のうち、要らない物(今の読み方でも同じ形を
    /// 読む機能の物)を外す。結果を変えないので、取り消しの記録にしない(古い版の文書を開いた直後に
    /// 使う。保存すると印が消える)。種類を組み立てられない機能の印は残す。外した数を返す。
    pub fn release_pins(&mut self, registry: &Registry) -> usize {
        let table = crate::BodyTable::build(&self.data, registry);
        let mut n = 0;
        for f in &mut self.data.features {
            if f.pin_bodies
                && !table.stale().contains_key(&f.id)
                && matches!(registry.instantiate(&f.type_id, &f.data), Some(Ok(_)))
            {
                f.pin_bodies = false;
                n += 1;
            }
        }
        if n > 0 {
            log::info!("前の版の読み方の印を外した: {n} 個(読む形は変わらない)");
        }
        n
    }

    /// 変わった入力の下流だけを評価し直す。入力が同じ機能はキャッシュを使う。
    /// 先に寸法の名前をそろえる([`Document::settle_names`])。
    pub fn recompute(&mut self, registry: &Registry, kernel: &dyn Kernel) -> RecomputeReport {
        self.settle_names(registry);
        let report = recompute::recompute_detached(&self.data, registry, kernel, &mut self.cache);
        self.emit(DocEvent::Recomputed {
            errors: report.error_count(),
        });
        report
    }

    /// 別のスレッドで再計算するためにキャッシュを取り出す。戻すまで文書のキャッシュは空になる。
    /// 次の再計算で、結果に無くてもカーネルに残させる形状(画面が出している形。[`Cache::hold`])。
    pub fn hold_shapes(&mut self, shapes: impl IntoIterator<Item = ShapeHandle>) {
        self.cache.hold(shapes);
    }

    pub fn take_cache(&mut self) -> Cache {
        std::mem::take(&mut self.cache)
    }

    /// 別のスレッドで再計算したキャッシュを戻し、結果を知らせる。
    pub fn put_cache(&mut self, cache: Cache, report: &RecomputeReport) {
        self.cache = cache;
        self.emit(DocEvent::Recomputed {
            errors: report.error_count(),
        });
    }

    /// 途中でやめた再計算のキャッシュを戻す(結果は出さないので知らせない)。
    pub fn return_cache(&mut self, cache: Cache) {
        self.cache = cache;
    }

    /// 文書のイベントを受け取る。受け手を捨てると自動で購読をやめる。
    pub fn subscribe(&mut self) -> Receiver<DocEvent> {
        let (tx, rx) = channel();
        self.subscribers.push(tx);
        rx
    }

    fn emit(&mut self, ev: DocEvent) {
        self.subscribers.retain(|s| s.send(ev.clone()).is_ok());
    }

    pub fn to_json(&self) -> String {
        if self.extra.is_empty() {
            return serde_json::to_string_pretty(&self.data)
                .expect("文書のデータは必ず JSON にできる");
        }
        let mut v = serde_json::to_value(&self.data).expect("文書のデータは必ず JSON にできる");
        let map = v.as_object_mut().expect("文書のデータは JSON の表");
        for (k, x) in &self.extra {
            map.entry(k.clone()).or_insert_with(|| x.clone());
        }
        serde_json::to_string_pretty(&v).expect("JSON の値は文字にできる")
    }

    /// JSON から読み込む。中身は編集時と同じ規則で検査する。
    /// 古い保存形式は今の形に直してから読み、新しい形式は読まない。
    pub fn from_json(src: &str) -> Result<Self, LoadError> {
        let mut raw: serde_json::Value = serde_json::from_str(src).map_err(LoadError::Json)?;
        let version = raw
            .get("format_version")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as u32;
        if version == 0 || version > FORMAT_VERSION {
            // 新しい版は読まない(知らない項目を落として保存し直すと、データが消えるため)
            return Err(LoadError::UnsupportedVersion(version));
        }
        // 古い版は、ここで 1 版ずつ今の形に直す
        for v in version..FORMAT_VERSION {
            migrate(v, &mut raw);
        }
        // 知らない上の階の項目は、読んだまま残して書き戻す
        let known = struct_fields::<DocumentData>();
        let extra: serde_json::Map<String, serde_json::Value> = raw
            .as_object()
            .map(|m| {
                m.iter()
                    .filter(|(k, _)| !known.contains(&k.as_str()))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let data: DocumentData = serde_json::from_value(raw).map_err(LoadError::Json)?;
        transaction::validate(&data).map_err(LoadError::Invalid)?;
        Ok(Document {
            data,
            extra,
            ..Self::default()
        })
    }
}

/// serde で読む struct `T` の欄の名前(serde の名前。derive が `deserialize_struct` に渡す一覧を
/// 受け取るだけで、何も読まない)。知らない項目を見分けるのに使う。
fn struct_fields<'de, T: Deserialize<'de>>() -> &'static [&'static str] {
    use serde::de::{Error, Visitor};
    struct Probe<'a>(&'a mut &'static [&'static str]);
    impl<'de> serde::Deserializer<'de> for Probe<'_> {
        type Error = serde::de::value::Error;
        fn deserialize_any<V: Visitor<'de>>(self, _: V) -> Result<V::Value, Self::Error> {
            Err(Error::custom("欄の名前だけを見る"))
        }
        fn deserialize_struct<V: Visitor<'de>>(
            self,
            _: &'static str,
            fields: &'static [&'static str],
            _: V,
        ) -> Result<V::Value, Self::Error> {
            *self.0 = fields;
            Err(Error::custom("欄の名前だけを見る"))
        }
        serde::forward_to_deserialize_any! {
            bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf
            option unit unit_struct newtype_struct seq tuple tuple_struct map enum identifier
            ignored_any
        }
    }
    let mut fields: &'static [&'static str] = &[];
    let _ = T::deserialize(Probe(&mut fields));
    fields
}

/// 消えた寸法の名前の代わりに入れる文字(その寸法の式。数と単位だけでなければ括弧で包む)。
fn substitute_text(expr: &str) -> String {
    match kmb_expr::parse(expr) {
        Ok(kmb_expr::Expr::Num(_)) => expr.trim().to_string(),
        _ => format!("({})", expr.trim()),
    }
}

/// `{"$new": k}` を、k 番目に足した機能の ID にする(`self_id` は今足している機能)。
fn resolve_new(v: &mut serde_json::Value, added: &[FeatureId], self_id: FeatureId) {
    match v {
        serde_json::Value::Object(m) => {
            if m.len() == 1
                && let Some(k) = m.get("$new").and_then(serde_json::Value::as_u64)
            {
                let id = added.get(k as usize).copied().unwrap_or(self_id);
                *v = serde_json::json!(id.0);
                return;
            }
            m.values_mut().for_each(|x| resolve_new(x, added, self_id));
        }
        serde_json::Value::Array(a) => a.iter_mut().for_each(|x| resolve_new(x, added, self_id)),
        _ => {}
    }
}

/// 保存形式の版 `from` の JSON を、版 `from + 1` の形に直す。版を上げる時にここへ足す。
/// 版ごとの変換は `if from == 1 { ... }` のように足していく。
/// 1 → 2: グループがないだけなので、版の数だけ変える。
/// 2 → 3: パラメータの表がないだけなので、版の数だけ変える。
/// 3 → 4: 上の階のモーション スタディと接触セットを、組立のモジュールの区画へ移す(区画の版 1。
/// 中身の形は版 3 と同じ)。ここだけは、版 3 の時のモジュールの区画の鍵を知っている(移した後の
/// 鍵はモジュールが名乗る。kmb-interchange の `motion`・`contact`)。
/// 4 → 5: 上の階の構成表・材料・外観・アニメーションの段を、モジュールの区画へ移す
/// ([`migrate_4`])。
/// 5 → 6: 全部の機能に、読むボディをデータの番号の形のまま読む印(`pin_bodies`)を付ける
/// (開いた形を前と同じにする。印の要らない機能は開いた後に [`Document::release_pins`] が外す)。
fn migrate(from: u32, raw: &mut serde_json::Value) {
    if from == 5
        && let Some(list) = raw
            .get_mut("features")
            .and_then(serde_json::Value::as_array_mut)
    {
        for f in list.iter_mut().filter_map(serde_json::Value::as_object_mut) {
            f.insert("pin_bodies".into(), serde_json::Value::Bool(true));
        }
    }
    if from == 4 {
        migrate_4(raw);
    }
    if from == 3
        && let Some(map) = raw.as_object_mut()
    {
        let mut sections = serde_json::Map::new();
        if let Some(v) = map.remove("motion_studies") {
            sections.insert(
                "asm.motion".into(),
                serde_json::json!({"version": 1, "data": {"studies": v}}),
            );
        }
        if let Some(v) = map.remove("contacts") {
            sections.insert(
                "asm.contacts".into(),
                serde_json::json!({"version": 1, "data": v}),
            );
        }
        if !sections.is_empty() {
            map.insert("sections".into(), serde_json::Value::Object(sections));
        }
    }
    raw["format_version"] = serde_json::json!(from + 1);
}

/// 版 4 の上の階の項目を、区画(区画の版 1)へ移す。ここだけは、移した先の区画の鍵と中身の形を
/// 知っている(移した後はモジュールが名乗る)。
/// - `configurations` → `config.table`(中身は同じ形)
/// - `animation` → `asm.animation` の `steps`
/// - `body_materials` → `cad.materials` の `bodies`
/// - `appearances`(`defs`・`faces`)・`body_appearances`・部品の `appearance` →
///   `render.appearances` の `defs`・`faces`・`bodies`・`components`(部品の番号 → 外観の名前)
fn migrate_4(raw: &mut serde_json::Value) {
    use serde_json::{Map, Value};
    let Some(map) = raw.as_object_mut() else {
        return;
    };
    let mut moved = Map::new();
    if let Some(v) = map.remove("configurations") {
        moved.insert("config.table".into(), v);
    }
    if let Some(v) = map.remove("animation") {
        moved.insert("asm.animation".into(), serde_json::json!({ "steps": v }));
    }
    // 名前は区画の決まり(空でなく前後に空白が無い)に合わせる。版 4 は確かめていなかった
    let names = |v: Value| -> Map<String, Value> {
        match v {
            Value::Object(m) => m
                .into_iter()
                .filter_map(|(k, n)| {
                    let n = n.as_str()?.trim();
                    (!n.is_empty()).then(|| (k, Value::String(n.to_string())))
                })
                .collect(),
            _ => Map::new(),
        }
    };
    if let Some(v) = map
        .remove("body_materials")
        .map(names)
        .filter(|m| !m.is_empty())
    {
        moved.insert("cad.materials".into(), serde_json::json!({ "bodies": v }));
    }
    let mut looks = match map.remove("appearances") {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    };
    if let Some(v) = map
        .remove("body_appearances")
        .map(names)
        .filter(|m| !m.is_empty())
    {
        looks.insert("bodies".into(), Value::Object(v));
    }
    let mut by_component = Map::new();
    if let Some(Value::Array(list)) = map.get_mut("components") {
        for c in list.iter_mut().filter_map(Value::as_object_mut) {
            if let (Some(Value::String(a)), Some(id)) = (c.remove("appearance"), c.get("id"))
                && !a.trim().is_empty()
            {
                by_component.insert(id.to_string(), Value::String(a.trim().to_string()));
            }
        }
    }
    if !by_component.is_empty() {
        looks.insert("components".into(), Value::Object(by_component));
    }
    if !looks.is_empty() {
        moved.insert("render.appearances".into(), Value::Object(looks));
    }
    if moved.is_empty() {
        return;
    }
    let sections = map
        .entry("sections")
        .or_insert_with(|| Value::Object(Default::default()));
    if let Some(sections) = sections.as_object_mut() {
        for (key, data) in moved {
            sections.insert(key, serde_json::json!({"version": 1, "data": data}));
        }
    }
}
