//! 加工の戦略の名乗り。各戦略の crate が `module()` で、登録する種類と、画面に出す追加の操作
//! (ボタンの名前・まとまり・アイコン・説明)、足す時の初めの値の作り方、入る位置を置けるか、
//! シミュレーションで荒削りの格子に数えるか、残り削りに使える工具を、データで渡す。
//! 加工の操作(jc-cam-commands)は、この一覧を見て操作を並べるだけで、戦略の名前を知らない。

use crate::OpBase;
use jc_cam_tool::ToolKind;
use jc_core::{Extension, FeatureId, Module, Registry};
use std::sync::Arc;

/// 「シミュレーションの精度」が自動の時に、粗い格子で削る種類に付ける印([`Registry::mark`])。
pub const ROUGH_GRID: &str = "cam.rough_grid";

/// 部品を素材から切り抜く種類に付ける印([`Registry::mark`])。工具の順に並べ替える時、この加工を
/// 元より前へ動かさない(後の加工の時に部品が外れているおそれ)。
pub const CUTS_FREE: &str = "cam.cuts_free";

/// 足す時に要る選び。満たさなければ操作は灰色になり、理由を出す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Needs {
    /// 要らない(セットアップと工具ライブラリだけ)
    Nothing,
    /// 3D のビューで選んだ平らな底の面か、タイムラインで選んだ閉じた輪郭のスケッチ
    FloorOrProfile,
    /// なぞる線(出力 `curves` を持つスケッチか文字)
    Curves,
    /// 閉じた輪郭(出力 `profile` を持つスケッチ)
    Profile,
}

impl Needs {
    /// 満たさない時の理由。
    pub fn why(self) -> &'static str {
        match self {
            Needs::Nothing => "",
            Needs::FloorOrProfile => {
                "3D のビューで平らな底の面を選ぶか、タイムラインで閉じた輪郭のスケッチを選んでから使う"
            }
            Needs::Curves => "なぞる線(スケッチか文字)が要る",
            Needs::Profile => "閉じた輪郭のあるスケッチが要る",
        }
    }
}

/// 3D のビューで面を右クリックした時のメニューに出すか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaceMenu {
    No,
    /// どの面でも
    AnyFace,
    /// 平らでない面(穴の壁)だけ
    Wall,
}

/// 足す時に、今の文書と選びから読むもの。加工の操作が今の文脈で答える。
pub trait Picks {
    /// 足す先のセットアップと工具ライブラリ、工具は 1 番
    fn base(&self) -> OpBase;
    /// ライブラリの、種類 `kind` の工具
    fn tool_of_kind(&self, kind: ToolKind) -> Option<u32>;
    /// ライブラリの、いちばん細い平の工具
    fn smallest_flat(&self) -> Option<u32>;
    /// 3D のビューで選んだ平らな面
    fn planar_face(&self) -> Option<String>;
    /// 3D のビューで選んだ平らでない面(穴やボスの壁)
    fn wall_face(&self) -> Option<String>;
    /// ポケットの底の面と、まとめて削る面。スケッチを選んでいれば空
    fn floor_faces(&self) -> Result<(String, Vec<String>), String>;
    /// タイムラインで選んだ、閉じた輪郭のスケッチ
    fn selected_profile(&self) -> Option<FeatureId>;
    /// スケッチ `id` からポケットを作る時の深さ(空なら平面の高さが底)
    fn sketch_depth(&self, id: FeatureId) -> Option<String>;
    /// 出力 `slot`(`curves`・`profile`)を持つ元(選んでいるもの、なければ最後のもの)
    fn source(&self, slot: &str) -> Option<FeatureId>;
    /// 足す先のセットアップのデータ
    fn setup_data(&self) -> Option<serde_json::Value>;
    /// 3D のビューで選んだ平らでない面(穴やボスの壁)の全部(Shift・Ctrl+クリックで足した物も)。
    /// 加工の選ぶ欄に入れる形
    fn wall_refs(&self) -> Vec<jc_core::Ref> {
        vec![]
    }
    /// 3D のビューで選んだ辺(加工の選ぶ欄に入れる形)
    fn edge_refs(&self) -> Vec<jc_core::Ref> {
        vec![]
    }

    /// 種類 `kind` の工具(なければ 1 番)で始める。
    fn with_tool(&self, kind: ToolKind) -> OpBase {
        OpBase {
            tool: self.tool_of_kind(kind).unwrap_or(1),
            ..self.base()
        }
    }
}

/// 3D の経路は細かい直線なので、円弧のあてはめを初めから入れる。
pub fn fitted(b: OpBase) -> OpBase {
    OpBase {
        fit: Some("0.005 mm".into()),
        ..b
    }
}

/// 足す時の初めのデータを JSON にする。
pub fn to_data(v: impl serde::Serialize) -> Result<serde_json::Value, String> {
    Ok(serde_json::to_value(v).expect("JSON にできる"))
}

/// 足す時の初めのデータを作る関数。
pub type Make = fn(&dyn Picks) -> Result<serde_json::Value, String>;

/// 同じ領域を削る別の種類の加工のデータを、この種類のデータにする関数(工具の順番で、元の種類が
/// 使えない工具を足す時)。
pub type Adopt = fn(&serde_json::Value) -> Result<serde_json::Value, String>;

/// 1 つの加工の戦略の名乗り。
#[derive(Clone, Copy)]
pub struct Strategy {
    /// 機能の種類(例: `cam.face@1`)
    pub type_id: &'static str,
    /// 足す操作の ID(例: `cam.add.face`)
    pub action: &'static str,
    /// 画面の名前(加工の名前の頭にも使う)
    pub label: &'static str,
    /// ツールバーのまとまり(`2D`・`3D`・`穴`)
    pub menu: &'static str,
    pub icon: &'static str,
    /// ツールバーの前に初めから出すか(残りはまとまりの一覧から選ぶ)
    pub front: bool,
    /// 検索に使う別名
    pub aliases: &'static str,
    pub hint: &'static str,
    pub needs: Needs,
    pub face_menu: FaceMenu,
    /// 入る位置を置ける時、その働きの説明
    pub entry: Option<&'static str>,
    /// 「シミュレーションの精度」が自動の時に粗い格子で削るか(荒削りの加工)
    pub rough_grid: bool,
    /// 細い工具で残りを削る加工を足せる時、使える工具の種類
    pub rest: &'static [ToolKind],
    /// 既定とテンプレートに使えるか
    pub reuse: bool,
    /// ツールバーと一覧での並び(小さい順)
    pub order: u16,
    pub make: Make,
    /// 3D のビューでタブを置けるか(加工の出力 [`crate::TAB_PATHS`] を出す時)
    pub tabs: bool,
    /// 3D のビューで削る輪郭(辺のつながり)を選べるか
    pub pick_chains: bool,
    /// この戦略の crate が持つ操作。加工の操作(jc-cam-commands)が自分の操作と同じ所に並べる。
    /// 1 つの crate で 1 つの戦略にだけ付ける
    pub commands: Option<fn() -> Arc<dyn Extension>>,
    /// 底の面を 3D のビューで足し外しできるか(データの `face`・`faces`・`profile` で領域を持つ)
    pub floor_faces: bool,
    /// 工具の順番を持てるか(同じ領域の残り削りを、データの `rest_tool` で後ろに並べる)
    pub tool_order: bool,
    /// 部品を切り抜くか(工具の順に並べ替える時に前へ動かさない。印 [`CUTS_FREE`])
    pub cuts_free: bool,
    /// 同じ領域の別の種類の加工のデータを、この種類にする(工具の順番で使う)
    pub adopt: Option<Adopt>,
}

impl Strategy {
    /// 2D のまとまりの、ツールバーの前に出す戦略(説明は「最後のセットアップに足す」)。
    pub fn new(
        type_id: &'static str,
        action: &'static str,
        label: &'static str,
        make: Make,
    ) -> Self {
        Strategy {
            type_id,
            action,
            label,
            menu: "2D",
            icon: "",
            front: true,
            aliases: "",
            hint: "最後のセットアップに足す",
            needs: Needs::Nothing,
            face_menu: FaceMenu::No,
            entry: None,
            rough_grid: false,
            rest: &[],
            reuse: true,
            order: 0,
            make,
            tabs: false,
            pick_chains: false,
            commands: None,
            floor_faces: false,
            tool_order: false,
            cuts_free: false,
            adopt: None,
        }
    }

    /// 底の面を 3D のビューで足し外しできる。
    pub fn floor_faces(self) -> Self {
        Strategy {
            floor_faces: true,
            ..self
        }
    }

    /// 工具の順番を持てる。`adopt` があれば、同じ領域の別の種類の加工からも足せる。
    pub fn tool_order(self, adopt: Option<Adopt>) -> Self {
        Strategy {
            tool_order: true,
            adopt,
            ..self
        }
    }

    /// 部品を切り抜く。
    pub fn cuts_free(self) -> Self {
        Strategy {
            cuts_free: true,
            ..self
        }
    }

    pub fn order(self, order: u16) -> Self {
        Strategy { order, ..self }
    }

    /// 3D のビューでタブを置ける。
    pub fn tabs(self) -> Self {
        Strategy { tabs: true, ..self }
    }

    /// 3D のビューで削る輪郭を選べる。
    pub fn pick_chains(self) -> Self {
        Strategy {
            pick_chains: true,
            ..self
        }
    }

    /// この戦略の crate が持つ操作。
    pub fn commands(self, commands: fn() -> Arc<dyn Extension>) -> Self {
        Strategy {
            commands: Some(commands),
            ..self
        }
    }

    pub fn menu(self, menu: &'static str) -> Self {
        Strategy { menu, ..self }
    }

    pub fn icon(self, icon: &'static str) -> Self {
        Strategy { icon, ..self }
    }

    /// まとまりの一覧からだけ選ぶ。
    pub fn not_front(self) -> Self {
        Strategy {
            front: false,
            ..self
        }
    }

    pub fn aliases(self, aliases: &'static str) -> Self {
        Strategy { aliases, ..self }
    }

    pub fn hint(self, hint: &'static str) -> Self {
        Strategy { hint, ..self }
    }

    pub fn needs(self, needs: Needs) -> Self {
        Strategy { needs, ..self }
    }

    pub fn face_menu(self, face_menu: FaceMenu) -> Self {
        Strategy { face_menu, ..self }
    }

    /// 入る位置を置ける(`hint` はクリックした所の働き)。
    pub fn entry(self, hint: &'static str) -> Self {
        Strategy {
            entry: Some(hint),
            ..self
        }
    }

    pub fn rough_grid(self) -> Self {
        Strategy {
            rough_grid: true,
            ..self
        }
    }

    pub fn rest(self, kinds: &'static [ToolKind]) -> Self {
        Strategy {
            rest: kinds,
            ..self
        }
    }

    /// 既定とテンプレートに使わない。
    pub fn no_reuse(self) -> Self {
        Strategy {
            reuse: false,
            ..self
        }
    }
}

impl std::fmt::Debug for Strategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Strategy")
            .field("type_id", &self.type_id)
            .field("action", &self.action)
            .finish_non_exhaustive()
    }
}

/// 戦略の crate の名乗り: 種類の登録と、その crate の戦略の一覧。
pub struct StrategyModule {
    pub name: &'static str,
    pub register: fn(&mut Registry),
    pub strategies: Vec<Strategy>,
}

impl StrategyModule {
    pub fn new(name: &'static str, register: fn(&mut Registry), strategies: Vec<Strategy>) -> Self {
        StrategyModule {
            name,
            register,
            strategies,
        }
    }
}

/// `before` の後に登録した種類のうち、加工の操作とセットアップに [`jc_core::CONCURRENT`] の印を付ける。
pub(crate) fn concurrent_operations(r: &mut Registry, before: &std::collections::BTreeSet<String>) {
    let new: Vec<String> = r
        .type_ids()
        .filter(|t| !before.contains(*t))
        .map(str::to_string)
        .collect();
    for t in new {
        if r.descriptor(&t).is_some_and(|d| {
            d.category == jc_core::category::OPERATION || d.category == jc_core::category::SETUP
        }) {
            r.mark(&t, jc_core::CONCURRENT);
        }
    }
}

/// 戦略の crate の名乗りを、組み立てに並べるモジュール(種類の登録と、荒削りの印)と、
/// 加工の操作に渡す戦略の一覧に分ける。
pub fn split(all: Vec<StrategyModule>) -> (Vec<Module>, Arc<[Strategy]>) {
    let mut modules = Vec::new();
    let mut strategies = Vec::new();
    for m in all {
        let register = m.register;
        let rough: Vec<&'static str> = m
            .strategies
            .iter()
            .filter(|s| s.rough_grid)
            .map(|s| s.type_id)
            .collect();
        let free: Vec<&'static str> = m
            .strategies
            .iter()
            .filter(|s| s.cuts_free)
            .map(|s| s.type_id)
            .collect();
        modules.push(Module::new(m.name, move |r: &mut Registry| {
            let before: std::collections::BTreeSet<String> =
                r.type_ids().map(str::to_string).collect();
            register(r);
            for t in &rough {
                r.mark(t, ROUGH_GRID);
            }
            for t in &free {
                r.mark(t, CUTS_FREE);
            }
            // 加工の操作は形を作らない(部品の網を読むだけ)ので、互いに依存しなければ同時に計算する
            concurrent_operations(r, &before);
        }));
        strategies.extend(m.strategies);
    }
    (modules, strategies.into())
}
