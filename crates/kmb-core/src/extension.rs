//! 機能のモジュールが画面・CLI・MCP に出す操作(拡張点)。
//!
//! 追加のボタン、右クリックのメニュー、コマンド検索、書き出し、シミュレーション、
//! 3D のビューへの重ね描きは、各モジュールが [`Extension`] として出す。
//! 画面は一覧を描いて実行するだけで、機能の中身を知らない。

use crate::{
    Arg, DocumentData, FeatureId, FeatureStatus, Kernel, PlanarFace, RecomputeReport, Registry,
    ShapeHandle, Value,
};
use serde::Serialize;
use std::path::Path;
use std::sync::Arc;

mod action;
mod handle;
mod job;
mod outcome;
mod overlay;

pub use action::*;
pub use handle::*;
pub use job::*;
pub use outcome::*;
pub use overlay::*;

/// 3D のビューで選んだ面(機能の出力 `body` の面の名前)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FacePick {
    pub feature: FeatureId,
    pub face: String,
}

/// 3D のビューで選んだ辺(機能の出力 `body` の、両側の面の名前の組で表す)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct EdgePick {
    pub feature: FeatureId,
    pub faces: [String; 2],
}

/// 操作を調べる・実行する時の文脈(文書の状態と、利用者が選んでいるもの)。
#[derive(Clone, Copy)]
pub struct Context<'a> {
    pub data: &'a DocumentData,
    pub report: &'a RecomputeReport,
    pub registry: &'a Registry,
    pub kernel: &'a Arc<dyn Kernel>,
    /// タイムラインで選んでいる機能(右クリックしたもの)
    pub selected: Option<FeatureId>,
    /// 3D のビューで選んでいる面
    pub picked: Option<&'a FacePick>,
    /// 一緒に選んでいる面(Shift・Ctrl+クリックで足したもの。`picked` は含めない)
    pub also_picked: &'a [FacePick],
    /// 3D のビューで選んでいる辺
    pub edges: &'a [EdgePick],
    /// 3D のビューで選んでいる頂点
    pub vertices: &'a [crate::VertexPick],
    /// 一緒に選んでいる機能(Ctrl・Shift で足したもの。`selected` は含めない)
    pub also_selected: &'a [FeatureId],
    /// 選んでいる機能を、3D のビューで選んだボディとして扱うか(選ぶ種類が「ボディ」の時)。
    /// 偽なら、機能を選んでいても先に選んだボディにはしない(作った直後の機能は選ばれているため)
    pub bodies_picked: bool,
    /// 操作がファイルを求める時([`Input::OpenFile`])、利用者が選んだファイル
    pub file: Option<&'a Path>,
    /// 開いている文書のファイル(保存していなければ `None`)。リンクで組み込んだ部品の元を、
    /// 文書の隣から探したり、輪になる組み込みを断ったりするのに使う
    pub doc_path: Option<&'a Path>,
    /// CLI・MCP から渡す細かい指定(例: シミュレーションの格子の大きさ `cell`)。
    /// 画面からは `Null`(どの操作も既定で動く)
    pub args: &'a serde_json::Value,
    /// 開いている文書の番号(画面のタブごとに違う。CLI・MCP など文書が 1 つの所は 0)。
    /// 文書の外に結果を持つモジュール(解析の研究の結果)が、タブをまたいで混ぜないのに使う
    pub doc: u64,
}

static NO_ARGS: serde_json::Value = serde_json::Value::Null;

impl<'a> Context<'a> {
    /// 選びも指定もない文脈。
    pub fn new(
        data: &'a DocumentData,
        report: &'a RecomputeReport,
        registry: &'a Registry,
        kernel: &'a Arc<dyn Kernel>,
    ) -> Self {
        Context {
            data,
            report,
            registry,
            kernel,
            selected: None,
            picked: None,
            also_picked: &[],
            edges: &[],
            vertices: &[],
            also_selected: &[],
            bodies_picked: false,
            file: None,
            doc_path: None,
            args: &NO_ARGS,
            doc: 0,
        }
    }

    /// 引数 `key` の値(なければ `None`)。操作は引数をこれか `arg_*` で読む(読んだ鍵を記録し、
    /// [`Extensions::run`] が宣言 [`Action::args`] に無い鍵を読んだ操作を見つける)。
    pub fn arg(&self, key: &str) -> Option<&'a serde_json::Value> {
        crate::args::note_read(key);
        self.args.get(key)
    }

    /// 指定 `key` の数(なければ `None`。数でなければ誤り)。
    pub fn arg_f64(&self, key: &str) -> Result<Option<f64>, String> {
        match self.arg(key) {
            None | Some(serde_json::Value::Null) => Ok(None),
            Some(v) => v
                .as_f64()
                .map(Some)
                .ok_or_else(|| format!("{key} は数で指定します")),
        }
    }

    /// 指定 `key` の文字列。
    pub fn arg_str(&self, key: &str) -> Option<&'a str> {
        self.arg(key)?.as_str()
    }

    /// 計算できた機能の出力。
    pub fn output(&self, id: FeatureId, slot: &str) -> Option<&'a Value> {
        match self.report.feature(id) {
            Some(FeatureStatus::Ok { outputs, .. }) => outputs.get(slot),
            _ => None,
        }
    }

    /// 最後に残ったボディ(表示しているもの)。
    pub fn visible_bodies(&self) -> Vec<(FeatureId, ShapeHandle)> {
        crate::visible_bodies(self.data, self.registry, self.report)
    }

    /// 機能 `id` が読む機能。読むボディは今のそのボディの最後の機能([`crate::reads_now`])。
    pub fn reads_now(&self, id: FeatureId) -> Vec<FeatureId> {
        crate::reads_now(self.data, self.registry, self.report, id)
    }

    /// 書き出すボディ(見えているボディから、引数 `exclude` の機能の番号のボディを除く。画面は目を
    /// 閉じて隠したボディを渡す)。再計算に誤りがある時と、書くボディが無い時は理由を返す。
    /// 書き出しの操作(STL・STEP・3MF・視点の線)が共に使う。
    pub fn export_bodies(&self) -> Result<Vec<ShapeHandle>, String> {
        let n = self.report.error_count();
        if n > 0 {
            return Err(format!("再計算で誤りが {n} 件あります"));
        }
        let exclude: Vec<u64> = self
            .arg("exclude")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_u64()).collect())
            .unwrap_or_default();
        let all = self.visible_bodies();
        let b: Vec<_> = all
            .iter()
            .filter(|(id, _)| !exclude.contains(&id.0))
            .map(|(_, h)| *h)
            .collect();
        if b.is_empty() {
            return Err(if all.is_empty() {
                "ボディがありません".into()
            } else {
                "見えているボディがありません(隠したボディは書き出しません)".into()
            });
        }
        Ok(b)
    }

    /// 最後までの機能のうち、出力 `slot` を持つもの(古い順)。ボディと、並べる機能の形は、
    /// 今見えているもの(後ろの機能に使われていないもの)だけにする。途中の機能を並べると、
    /// そこから枝分かれした別のボディになるため。
    pub fn candidates(&self, slot: &str) -> Vec<u64> {
        if slot == "body" || slot == "recipe" {
            return self
                .visible_bodies()
                .iter()
                .filter(|b| self.output(b.0, slot).is_some())
                .map(|b| b.0.0)
                .collect();
        }
        self.data
            .features
            .iter()
            .filter(|f| self.output(f.id, slot).is_some())
            .map(|f| f.id.0)
            .collect()
    }

    /// 選んでいる面が平らなら、その平面。
    pub fn picked_plane(&self) -> Option<PlanarFace> {
        let p = self.picked?;
        let Some(Value::Shape(h)) = self.output(p.feature, "body") else {
            return None;
        };
        self.kernel
            .faces(*h)
            .ok()?
            .into_iter()
            .find(|f| f.name == p.face)?
            .planar
    }

    /// 選んでいる面の全部(一緒に選んだ面が先、最後に選んだ面 `picked` が後)。
    pub fn picked_faces(&self) -> Vec<&'a FacePick> {
        self.also_picked.iter().chain(self.picked).collect()
    }

    /// 3D のビューで選んでいる物の全部(面、辺、頂点、ボディの順)。選ぶ欄へ振り分けるのに使う。
    pub fn refs(&self) -> Vec<crate::Ref> {
        let mut out: Vec<crate::Ref> = self
            .picked_faces()
            .into_iter()
            .map(|p| crate::Ref::from(p.clone()))
            .collect();
        out.extend(self.edges.iter().cloned().map(crate::Ref::from));
        out.extend(self.vertices.iter().cloned().map(crate::Ref::from));
        if self.bodies_picked {
            out.extend(
                self.also_selected
                    .iter()
                    .copied()
                    .chain(self.selected)
                    .filter(|f| self.output(*f, "body").is_some())
                    .map(crate::Ref::Body),
            );
        }
        out
    }

    /// 面 `r` が平らか(面でなければ偽)。
    pub fn is_planar(&self, r: &crate::Ref) -> bool {
        let crate::Ref::Face { feature, face } = r else {
            return false;
        };
        let Some(Value::Shape(h)) = self.output(*feature, "body") else {
            return false;
        };
        self.kernel
            .faces(*h)
            .ok()
            .and_then(|l| l.into_iter().find(|f| f.name == *face))
            .is_some_and(|f| f.planar.is_some())
    }

    /// 種類 `type_id` の機能の数。
    pub fn count_of(&self, type_id: &str) -> usize {
        self.data
            .features
            .iter()
            .filter(|f| f.type_id == type_id)
            .count()
    }

    /// 種類が `pred` に合う最後の機能。
    pub fn last_where(&self, pred: impl Fn(&str) -> bool) -> Option<FeatureId> {
        self.data
            .features
            .iter()
            .rev()
            .find(|f| pred(&f.type_id))
            .map(|f| f.id)
    }

    /// 選んでいる機能の種類。
    pub fn selected_type(&self) -> Option<&'a str> {
        Some(self.data.feature(self.selected?)?.type_id.as_str())
    }
}

/// 機能のモジュールが出す操作一式。
pub trait Extension: Send + Sync {
    /// 今の文脈で出す操作(使えないものは理由つき)。
    fn actions(&self, cx: &Context<'_>) -> Vec<Action>;

    /// 操作 `id` を実行する。自分の操作でなければ `None`。
    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, String>>;

    /// 一覧([`Extension::actions`])に出さない自分の操作 `id`(画面の部品が中から呼ぶ物。部品を
    /// 引いて動かす、など)の引数の宣言。`None` の操作は引数を受け取らない(段の帯の段のように、値を
    /// ID の後ろに付ける物はそのままでよい)。
    fn unlisted(&self, _id: &str) -> Option<Vec<Arg>> {
        None
    }

    /// 選んでいる機能について 3D のビューに重ねるもの。
    fn overlay(&self, _cx: &Context<'_>) -> Overlay {
        Overlay::default()
    }

    /// 加工の段の帯(なければ `None`)。画面は帯を描き、押された段の操作を実行するだけ。
    fn stages(&self, _cx: &Context<'_>) -> Option<StageBand> {
        None
    }

    /// 操作 `id` を 3D のビューで値を決めながら行う時の手順(今の選びと、決めた値 `cx.args` で)。
    /// 手順のない操作なら `None`。全部の値を決めたら、画面は同じ `args` で [`Extension::run`] を呼ぶ。
    fn handles(&self, _id: &str, _cx: &Context<'_>) -> Option<Vec<Handle>> {
        None
    }

    /// 機能 `id` を「実行」で走らせる重い計算の様子(解析の研究)。ブラウザの札とパネルの頭に出す。
    /// そういう機能でなければ `None`。
    fn run_state(&self, _cx: &Context<'_>, _id: FeatureId) -> Option<RunState> {
        None
    }

    /// 今 3D のビューに出している結果のコマ(時刻・モード)の帯。なければ `None`。
    fn frames(&self, _cx: &Context<'_>) -> Option<FrameBar> {
        None
    }

    /// 機能 `id` の注意(評価の警告とは別に、機械の検査などで分かること)。
    fn notes(&self, _cx: &Context<'_>, _id: FeatureId) -> Vec<String> {
        Vec::new()
    }

    /// 機能 `id` の設定欄に出す要約(例: 経路の長さと時間)。
    fn summary(&self, _cx: &Context<'_>, _id: FeatureId) -> Option<String> {
        None
    }

    /// コアの知らない出力の値(`Value::Custom`)の短い説明(例: 「スケッチ(完全拘束)」)。
    fn describe(&self, _v: &Value) -> Option<String> {
        None
    }

    /// PC 全体の設定(なければ空)。
    fn settings(&self) -> Vec<Settings> {
        Vec::new()
    }

    /// 設定 `id` を `value` に変える。知らない `id` なら `None`。
    /// 値が正しくなければ、何も変えずに理由を返す。
    fn set_settings(&self, _id: &str, _value: &serde_json::Value) -> Option<Result<(), String>> {
        None
    }

    /// 文書ごとに持つ設定(レンダリングのシーンなど)の、文書 `data` での値。文書ごとでない設定と、
    /// 文書にまだ書いていない時は `None`(PC 全体の値 = 前回使った値を使う)。
    fn document_settings(&self, _id: &str, _data: &DocumentData) -> Option<serde_json::Value> {
        None
    }

    /// 設定 `id` を文書ごとに持つなら、文書 `data` に `value` を書く変更(多くは
    /// [`DocumentData::set_section`])。文書ごとでない設定は `None`。値が正しくなければ理由を返す。
    fn document_settings_edit(
        &self,
        _id: &str,
        _value: &serde_json::Value,
        _data: &DocumentData,
    ) -> Option<Result<Edit, String>> {
        None
    }

    /// CLI・MCP の状態の JSON に足す項目(名前と値。例: セットアップの一覧)。
    fn status(&self, _cx: &Context<'_>) -> Vec<(String, serde_json::Value)> {
        Vec::new()
    }

    /// 外のファイルとつながった機能(リンクで組み込んだ部品)の今の様子。元のファイルを読むので、
    /// 画面は時々(数秒ごと)だけ呼ぶ。
    fn links(&self, _cx: &Context<'_>) -> Vec<LinkInfo> {
        Vec::new()
    }

    /// 3D のビューで面 `at` を左ドラッグした時に動かせる物(組立のジョイントの自由度)。動かせなければ
    /// `None`(画面は範囲で選ぶ)。画面は引いた量を欄 [`Drag::field`] の値にして機能を直す。
    fn drag(&self, _cx: &Context<'_>, _at: &FacePick) -> Option<Drag> {
        None
    }

    /// 引いている [`Drag`](回転かスライダ)を、欄の値 `from` から `to` へ動かす時に、ぶつかって
    /// 止まる値([`crate::contact`])。止めなければ `None`(画面は `to` へ動かす)。
    fn drag_limit(&self, _cx: &Context<'_>, _drag: &Drag, _from: f64, _to: f64) -> Option<f64> {
        None
    }

    /// 利用者のデータフォルダを渡す(PC 全体のファイル、例: 加工のテンプレートを置く所)。
    /// データフォルダのない時(テストなど)は `None` のまま。
    fn set_data_dir(&self, _dir: Option<&Path>) {}

    /// 辺 `seeds`(`edges` の番号)から、一緒に選ぶつながった辺の番号(`seeds` を含む)。画面が辺の
    /// ダブルクリックで使う。たどり方は CAD のモジュールが持つ。持たなければ `None`
    fn edge_chain(&self, _edges: &[crate::EdgeInfo], _seeds: &[usize]) -> Option<Vec<usize>> {
        None
    }
}

/// 外のファイルとつながった機能の様子([`Extension::links`])。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LinkInfo {
    /// つながっている機能
    pub feature: FeatureId,
    /// 一緒に入ったボディ(最初の機能)。ブラウザは、これを持つ部品に印を付ける
    pub bodies: Vec<FeatureId>,
    /// 元のファイル(見つからなければ `None`)
    pub source: Option<std::path::PathBuf>,
    pub status: LinkStatus,
    /// 短い説明(「元のファイルが変わりました」など)
    pub note: String,
    /// この機能についての操作(最新にする・固定・リンクを切る)。画面は部品の右クリックに並べ、
    /// [`Extension::run`] を、選んだ機能 `feature` と元のファイル `source` で呼ぶ
    pub actions: Vec<String>,
}

/// リンクの様子。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkStatus {
    /// 元と同じ
    Latest,
    /// 元が変わった(最新にできる)
    Stale,
    /// 元が変わったが、この版に固定している
    Pinned,
    /// 元のファイルが見つからない・読めない(写しのまま使う)
    Missing,
    /// リンクを切った写し
    Broken,
}

/// 引いて動かす物の動きの 1 つ(ジョイントの欄 1 つ分。[`Drag::dofs`])。
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
pub struct Dof {
    /// 欄の名前(`angle`・`distance` など)
    pub field: String,
    /// 真なら向き `axis` に動く(値は mm)、偽なら点 `origin` を通る軸 `axis` のまわりに回る(値は
    /// ラジアン。右ねじの向きが正)
    pub slide: bool,
    pub origin: crate::kernel::P3,
    /// 長さ 1
    pub axis: crate::kernel::P3,
}

impl Dof {
    /// 値 `v` だけ動かす行列。
    pub fn matrix(&self, v: f64) -> crate::kernel::Matrix4 {
        use crate::kernel as k;
        if self.slide {
            k::translation(k::scale(self.axis, v))
        } else {
            k::rotation(self.origin, self.axis, v)
        }
    }
}

/// 3D のビューで引いて動かせる物([`Extension::drag`])。軸と点は今の位置(世界座標)。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Drag {
    /// 直す機能(ジョイント)。[`DragMotion::Free`] の時は引いた面のボディ
    pub feature: FeatureId,
    /// 直す欄(記述子のスライダの付いた式の欄。範囲と単位はそのスライダのもの)。
    /// [`DragMotion::Free`] の時は、動かした量で実行する操作
    pub field: String,
    pub motion: DragMotion,
    /// 短い説明(「ジョイント1 を回す」など)
    pub label: String,
    /// [`DragMotion::Follow`] の時の、ジョイントの動き全部(点に掛ける順、値が全部 0 の時の軸)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dofs: Vec<Dof>,
    /// [`DragMotion::Follow`] の時に、引いても変えない欄(`dofs` の欄の名前)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub still: Vec<String>,
}

/// 引いた時の動き。
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum DragMotion {
    /// 点 `origin` を通る向き `axis` のまわりに回す(値は度。右ねじの向きが正)
    Rotate { origin: [f64; 3], axis: [f64; 3] },
    /// 向き `axis` に動かす(値は mm。`axis` の向きが正)
    Slide { axis: [f64; 3] },
    /// 画面に平行に自由に動かす(ジョイントの無い部品)。画面は、動かした量(世界座標の mm の
    /// `[x, y, z]`)を引数 `offset` に、[`Drag::feature`] を引数 `body` にして、操作 [`Drag::field`] を
    /// 実行する(コマごとに押す前へ戻してから)
    Free,
    /// ジョイントの動き全部([`Drag::dofs`]。[`Drag::still`] の欄は除く)で、押した点がカーソルの
    /// 視線にいちばん近づくよう動かす(kmb-interchange の `animation::follow`)。画面は欄ごとの値を直す。
    /// [`Drag::field`] は代表の欄(札の単位に使う)
    Follow,
}

/// 複数のモジュールの操作をまとめたもの。
#[derive(Clone, Default)]
pub struct Extensions(pub Vec<Arc<dyn Extension>>);

impl Extensions {
    /// 辺 `seeds` から一緒に選ぶつながった辺([`Extension::edge_chain`])。どのモジュールも持たなければ
    /// `seeds` だけ。
    pub fn edge_chain(&self, edges: &[crate::EdgeInfo], seeds: &[usize]) -> Vec<usize> {
        self.0
            .iter()
            .find_map(|e| e.edge_chain(edges, seeds))
            .unwrap_or_else(|| seeds.to_vec())
    }

    /// モジュールの順に並べた操作。[`Action::before`] の印のある操作は、印の付いた順に、
    /// 指す操作のすぐ前へ移す。
    pub fn actions(&self, cx: &Context<'_>) -> Vec<Action> {
        let mut all: Vec<Action> = self.0.iter().flat_map(|e| e.actions(cx)).collect();
        let marked: Vec<String> = all
            .iter()
            .filter(|a| a.before.is_some())
            .map(|a| a.id.clone())
            .collect();
        for id in marked {
            let Some(from) = all.iter().position(|a| a.id == id) else {
                continue;
            };
            let anchor = all[from].before.clone().unwrap_or_default();
            if anchor == id || !all.iter().any(|a| a.id == anchor) {
                continue;
            }
            let a = all.remove(from);
            let to = all.iter().position(|x| x.id == anchor).unwrap_or(all.len());
            all.insert(to, a);
        }
        all
    }

    /// 操作 `id` の宣言(今の文脈の一覧から)。
    pub fn action(&self, id: &str, cx: &Context<'_>) -> Option<Action> {
        self.actions(cx).into_iter().find(|a| a.id == id)
    }

    fn run_checked(&self, id: &str, cx: &Context<'_>) -> Result<Outcome, String> {
        // 一覧に出さずに画面の部品が呼ぶ操作(段の帯の段・コマを移す操作など。値は ID の後ろに付ける)は、
        // unlisted で宣言しなければ引数を受け取らない操作として扱う(引数を読めば下で誤りになる)
        let decl = match self.action(id, cx) {
            Some(a) => a.args,
            None => self
                .0
                .iter()
                .find_map(|e| e.unlisted(id))
                .unwrap_or_default(),
        };
        let args = crate::args::check_args(id, &decl, cx.args)?;
        let cx = Context { args: &args, ..*cx };
        let (r, read) = crate::args::recording(|| {
            self.0
                .iter()
                .find_map(|e| e.run(id, &cx))
                .unwrap_or_else(|| Err(format!("操作 {id} はありません")))
        });
        let undeclared: Vec<&str> = read
            .iter()
            .filter(|k| !decl.iter().any(|a| &a.key == *k))
            .map(String::as_str)
            .collect();
        if !undeclared.is_empty() {
            // 作りの誤り(宣言に足し忘れた)。宣言に無い鍵は渡せないので、読んでも既定になるだけ
            let e = format!(
                "操作 {id} が宣言していない引数 {} を読んだ(Action::args に足す)",
                undeclared.join("・")
            );
            log::error!("{e}");
            if cfg!(debug_assertions) {
                return Err(e);
            }
        }
        r
    }

    /// 操作 `id` を探して実行する。どのモジュールも知らなければ誤り。
    ///
    /// 引数は操作の宣言 [`Action::args`] で確かめ(知らない鍵・型の違いは誤り)、単位つきの式を数に
    /// 直し、省いた引数に既定を入れてから渡す。一覧に無い操作は [`Extension::unlisted`] の宣言で確かめる
    /// (宣言が無ければ引数を受け取らない)。
    pub fn run(&self, id: &str, cx: &Context<'_>) -> Result<Outcome, String> {
        let t = std::time::Instant::now();
        let r = self.run_checked(id, cx);
        let ms = t.elapsed().as_secs_f64() * 1e3;
        match r {
            Err(e) => {
                log::warn!("操作 {id} が失敗: {e} ({ms:.1} ms)");
                Err(e)
            }
            // 時間のかかる仕事は、終わった時に時間と結果を書く(別のスレッドで動いてもよい)
            Ok(Outcome::Job(job)) => {
                log::info!("操作 {id} の仕事を始める");
                let id = id.to_string();
                Ok(Outcome::Job(Box::new(move |ctl: &JobControl| {
                    let t = std::time::Instant::now();
                    let report = job(ctl);
                    let level = if report.ok {
                        log::Level::Info
                    } else {
                        log::Level::Warn
                    };
                    log::log!(
                        level,
                        "操作 {id} の仕事が終わった: ok={} {} ({:.1} ms)",
                        report.ok,
                        report.text,
                        t.elapsed().as_secs_f64() * 1e3
                    );
                    report
                })))
            }
            Ok(o) => {
                log::info!("操作 {id}: {} ({ms:.1} ms)", o.brief());
                log::debug!("操作 {id} の結果: {o:?}");
                Ok(o)
            }
        }
    }

    pub fn handles(&self, id: &str, cx: &Context<'_>) -> Option<Vec<Handle>> {
        self.0.iter().find_map(|e| e.handles(id, cx))
    }

    pub fn stages(&self, cx: &Context<'_>) -> Option<StageBand> {
        self.0.iter().find_map(|e| e.stages(cx))
    }

    pub fn overlay(&self, cx: &Context<'_>) -> Overlay {
        let mut o = Overlay::default();
        for e in &self.0 {
            o.extend(e.overlay(cx));
        }
        o
    }

    pub fn notes(&self, cx: &Context<'_>, id: FeatureId) -> Vec<String> {
        self.0.iter().flat_map(|e| e.notes(cx, id)).collect()
    }

    pub fn run_state(&self, cx: &Context<'_>, id: FeatureId) -> Option<RunState> {
        self.0.iter().find_map(|e| e.run_state(cx, id))
    }

    pub fn frames(&self, cx: &Context<'_>) -> Option<FrameBar> {
        self.0.iter().find_map(|e| e.frames(cx))
    }

    pub fn summary(&self, cx: &Context<'_>, id: FeatureId) -> Option<String> {
        self.0.iter().find_map(|e| e.summary(cx, id))
    }

    pub fn describe(&self, v: &Value) -> Option<String> {
        self.0.iter().find_map(|e| e.describe(v))
    }

    pub fn status(&self, cx: &Context<'_>) -> Vec<(String, serde_json::Value)> {
        self.0.iter().flat_map(|e| e.status(cx)).collect()
    }

    /// 面 `at` を引いて動かせる物(最初に答えたモジュールの物)。
    pub fn drag(&self, cx: &Context<'_>, at: &FacePick) -> Option<Drag> {
        self.0.iter().find_map(|e| e.drag(cx, at))
    }

    /// 引いている物が止まる値(最初に答えたモジュールの物。[`Extension::drag_limit`])。
    pub fn drag_limit(&self, cx: &Context<'_>, drag: &Drag, from: f64, to: f64) -> Option<f64> {
        self.0.iter().find_map(|e| e.drag_limit(cx, drag, from, to))
    }

    pub fn links(&self, cx: &Context<'_>) -> Vec<LinkInfo> {
        self.0.iter().flat_map(|e| e.links(cx)).collect()
    }

    pub fn settings(&self) -> Vec<Settings> {
        self.0.iter().flat_map(|e| e.settings()).collect()
    }

    /// 設定の一覧の、文書 `data` での値(文書ごとに持つ設定は、文書に書いてあれば文書の値)。
    pub fn settings_in(&self, data: &DocumentData) -> Vec<Settings> {
        self.0
            .iter()
            .flat_map(|e| {
                e.settings().into_iter().map(move |mut g| {
                    if let Some(v) = e.document_settings(&g.id, data) {
                        g.value = v;
                    }
                    g
                })
            })
            .collect()
    }

    /// 設定 `id` を文書ごとに持つなら、文書 `data` に `value` を書く変更。文書ごとでなければ `None`。
    pub fn document_settings_edit(
        &self,
        id: &str,
        value: &serde_json::Value,
        data: &DocumentData,
    ) -> Option<Result<Edit, String>> {
        self.0
            .iter()
            .find_map(|e| e.document_settings_edit(id, value, data))
    }

    /// 利用者のデータフォルダを、すべてのモジュールに渡す。
    pub fn set_data_dir(&self, dir: Option<&Path>) {
        for e in &self.0 {
            e.set_data_dir(dir);
        }
    }

    /// 設定 `id` を変える。どのモジュールも知らなければ誤り。
    pub fn set_settings(&self, id: &str, value: &serde_json::Value) -> Result<(), String> {
        self.0
            .iter()
            .find_map(|e| e.set_settings(id, value))
            .unwrap_or_else(|| Err(format!("設定 {id} はありません")))
    }
}
