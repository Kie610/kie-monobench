//! CAM のセットアップと、加工の機能に共通する部品。
//!
//! セットアップ(`cam.setup@1`)は、加工するボディ、ストック(材料の箱)、作業座標系(WCS)の原点、
//! 安全高さを決める。出力の `setup` スロットに [`SetupInfo`] を入れる。
//!
//! 加工の機能(フェイス、輪郭、ポケット、穴)は [`OpBase`] でセットアップと工具を指し、
//! [`prepare`] で加工用の幾何と工具をそろえ、[`finish`] で経路を出力にする。
//! 座標はすべて WCS で、Z は上向き(当面は上から加工するセットアップだけを扱う)。

use kmb_cam_geom::{Area, Loop, MachiningModel, P2, offset_area};
use kmb_cam_tool::{Cutting, Tool, ToolLibrary};
use kmb_core::{
    Descriptor, Dim, EvalCtx, EvalError, Feature, FeatureId, Field, InputRef, Mesh, Outputs,
    Registry, ShapeHandle, Value, add_warnings, category, expr_inputs,
};
use kmb_toolpath::{Move, P3, TOOLPATH_TYPE, Toolpath};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

mod chains;
mod fixture;
mod floor;
mod manual;
pub mod nest;
mod op;
mod pattern;
pub mod placement;
mod plan;
use plan::{depth_problem, empty_toolpath, keep_bottom, keep_top, range_problem, reachable_bottom};
mod rest;
pub mod sections;
mod strategy;
pub mod testing;
pub use chains::{
    Chain, chains, chains_of, edge_lines, edge_points, lines_of, read_lines, sketch_chains,
};
pub use fixture::{FIXTURE_GAP, FIXTURE_MARGIN, Fixture, avoid, passes_over, place_clamp};
pub use floor::{
    Floor, FloorPick, floor_area, floor_fields, floor_inputs, floors, last_step_field,
    leave_fields, leaves, min_radius_field, opt_length, profile_areas, round_corners,
    unreachable_area, wall_top,
};
pub use kmb_toolpath_moves::clip::{ClipOpts, clip_to_section};
pub use kmb_toolpath_moves::{
    DEFAULT_ENTRY_DEG, ENTRY_AREA, EntryArea, EntryKind, EntryOpts, EntryPlan, FinishOpts,
    FinishPlan, Lap, LeadOpts, LeadShape, Leads, add_entry_area, entry_area_of, entry_fields,
    feed_value, finish_fields, finish_offsets, helix_moves, helix_start, lap_start,
    lead_shape_fields, leads, start_after, start_at, walk_moves, zigzag_moves,
};
pub use kmb_toolpath_moves::{
    LeadArcs, approach, free_on_left, lead_arcs, length, lift_over_tabs, lift_over_tabs_shaped,
    loop_length, loop_moves, ramp_moves, retract, start_mid_line, start_near, tab_centers,
};
pub use manual::{MANUAL_NC, ManualKind, ManualNc, ManualOutput, manual_of};
pub use op::{
    AirCuts, CutDir, CuttingOutput, HeightFrom, HeightsOutput, OpBase, OpHeights, Prepared,
    RetractHeight, SimGrid, cutting_of, direction_field, finish, heights_of, lead_field, prepare,
    ramp_angle, split_levels, toolpath_of,
};
pub use pattern::{MAX_COPIES, MirrorAxis, PATTERN, Pattern, PatternKind};
pub use placement::{Mat3, Up};
pub use plan::{
    EntryRegion, HeightRule, MachineView, Planner, SetupView, Sources, StrategyInput,
    StrategyOutput, machine_stepdown, plan_error,
};
pub use rest::{RestSize, is_rest_data, rest_levels, rest_plan, rest_tool, rest_tool_or_size};
pub use strategy::{
    Adopt, CUTS_FREE, FaceMenu, Make, Needs, Picks, ROUGH_GRID, Strategy, StrategyModule, fitted,
    split, to_data,
};

pub const SETUP: &str = "cam.setup@1";
pub const SETUP_TYPE: &str = "cam.setup@1";

/// 加工用の網の許容差の既定値(mm)。セットアップごとに変えられる。
pub const MESH_TOLERANCE: f64 = 0.01;
/// 網の許容差に使える範囲(mm)。
const TOLERANCE_RANGE: (f64, f64) = (0.001, 0.5);

fn mesh_tolerance() -> f64 {
    MESH_TOLERANCE
}

/// ストックの決め方。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StockMode {
    /// 部品の外形に余白を足す
    #[default]
    Margin,
    /// 決まった大きさの材料(幅・奥行・高さ)に部品を置く
    Fixed,
    /// 別のボディ(前に削った板、鋳物のような素材の形)をストックにする。箱はそのボディの外形
    Body,
}

/// ストック(削る前の材料の箱)。長さはすべて式。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stock {
    #[serde(default)]
    pub mode: StockMode,
    /// 材料の名前(例: 「アルミ」)。工具の切削条件のプリセットを選ぶ。空なら工具の既定の条件
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub material: String,
    /// 横の余白(4 面共通)。`Margin` の時
    #[serde(default = "two_mm")]
    pub margin_side: String,
    /// 面ごとの横の余白。空なら `margin_side`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub margin_left: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub margin_right: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub margin_front: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub margin_back: Option<String>,
    /// 部品の上面からストックの上面まで。`Fixed` の時も、部品をこの深さに置く
    #[serde(default = "one_mm")]
    pub margin_top: String,
    #[serde(default = "zero_mm")]
    pub margin_bottom: String,
    /// ストックの幅・奥行・高さを、この長さの倍数に切り上げる(`Margin` の時)。空なら丸めない
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round_to: Option<String>,
    /// 決まった大きさ(X、Y、Z)。`Fixed` の時
    #[serde(default = "hundred_mm")]
    pub width: String,
    #[serde(default = "hundred_mm")]
    pub depth: String,
    #[serde(default = "twenty_mm")]
    pub height: String,
    /// 部品を XY のどこに寄せるか。`Fixed` の時
    #[serde(default = "center")]
    pub align: Origin,
    /// ストックにするボディを出す機能。`Body` の時
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<FeatureId>,
}

impl Default for Stock {
    fn default() -> Self {
        Stock {
            mode: StockMode::Margin,
            material: String::new(),
            margin_side: two_mm(),
            margin_left: None,
            margin_right: None,
            margin_front: None,
            margin_back: None,
            margin_top: one_mm(),
            margin_bottom: zero_mm(),
            round_to: None,
            width: hundred_mm(),
            depth: hundred_mm(),
            height: twenty_mm(),
            align: Origin::Center,
            body: None,
        }
    }
}

fn hundred_mm() -> String {
    "100 mm".into()
}

fn center() -> Origin {
    Origin::Center
}

fn zero_mm() -> String {
    "0 mm".into()
}
fn one_mm() -> String {
    "1 mm".into()
}
fn two_mm() -> String {
    "2 mm".into()
}
fn five_mm() -> String {
    "5 mm".into()
}
fn twenty_mm() -> String {
    "20 mm".into()
}

/// WCS の原点をストックの XY のどこに置くか。高さは [`OriginZ`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// 手前左の角(X、Y が最小の角)
    #[default]
    FrontLeft,
    FrontRight,
    BackLeft,
    BackRight,
    Center,
}

/// WCS の原点の高さ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OriginZ {
    /// ストックの上面(工具の先を材料の上に合わせる)
    #[default]
    Top,
    /// ストックの下面(機械のテーブルに合わせる)
    Bottom,
}

/// 裏返して加工する時の向き。部品の外形の箱の中心を通る軸のまわりに 180° 回す。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Flip {
    /// 裏返さない(表から削る)
    #[default]
    None,
    /// X 軸(左右の向き)のまわりに回す: 手前と奥、上と下が入れ替わる
    AroundX,
    /// Y 軸(前後の向き)のまわりに回す: 左と右、上と下が入れ替わる
    AroundY,
}

impl Flip {
    pub fn is_none(&self) -> bool {
        *self == Flip::None
    }

    /// 回し方の行列。
    pub fn matrix(self) -> Mat3 {
        match self {
            Flip::None => placement::IDENTITY,
            Flip::AroundX => Up::MinusZ.matrix(),
            Flip::AroundY => [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]],
        }
    }

    /// 点 `p` を、中心 `c` を通る軸のまわりに回す(2 回回すと元に戻る)。
    pub fn point(self, p: P3, c: P3) -> P3 {
        match self {
            Flip::None => p,
            Flip::AroundX => [p[0], 2.0 * c[1] - p[1], 2.0 * c[2] - p[2]],
            Flip::AroundY => [2.0 * c[0] - p[0], p[1], 2.0 * c[2] - p[2]],
        }
    }

    fn dir(self, n: [f32; 3]) -> [f32; 3] {
        match self {
            Flip::None => n,
            Flip::AroundX => [n[0], -n[1], -n[2]],
            Flip::AroundY => [-n[0], n[1], -n[2]],
        }
    }

    /// 網を回す(面の名前と三角形の並びはそのまま)。
    pub fn mesh(self, m: &Mesh, c: P3) -> Mesh {
        if self.is_none() {
            return m.clone();
        }
        let pt = |p: &[f32; 3]| {
            let q = self.point([p[0] as f64, p[1] as f64, p[2] as f64], c);
            [q[0] as f32, q[1] as f32, q[2] as f32]
        };
        Mesh {
            positions: m.positions.iter().map(pt).collect(),
            normals: m.normals.iter().map(|n| self.dir(*n)).collect(),
            triangles: m.triangles.clone(),
            triangle_faces: m.triangle_faces.clone(),
            face_names: m.face_names.clone(),
            edges: m.edges.iter().map(|e| e.iter().map(pt).collect()).collect(),
        }
    }
}

/// セットアップの機能のデータ。例: `{"body": 7}`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Setup {
    /// 加工するボディを出す機能
    pub body: FeatureId,
    /// 使う機械(マシンライブラリの ID。`kmb_post::machines`)。この欄が無い古い文書は
    /// [`LEGACY_MACHINE`](同じ値は書かない)
    #[serde(default = "legacy_machine", skip_serializing_if = "is_legacy_machine")]
    pub machine: String,
    /// セットアップを足した時に写した機械の値(設定で直した値を含む)。経路の計算・ポスト・検査が
    /// この値を使うので、結果は文書だけで決まる(後で設定を直しても変わらない)。ID が `machine` と違う
    /// (欄で機械を替えた)か無い(古い文書)ならマシンライブラリの値
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine_values: Option<kmb_post::MachineProfile>,
    #[serde(default)]
    pub stock: Stock,
    #[serde(default)]
    pub origin: Origin,
    #[serde(default)]
    pub origin_z: OriginZ,
    /// 裏返して加工する(2 つ目のセットアップ)
    #[serde(default, skip_serializing_if = "Flip::is_none")]
    pub flip: Flip,
    /// ストックの上面から、横に早送りする高さまで
    #[serde(default = "five_mm")]
    pub safe_height: String,
    /// 工具交換と最後に戻る高さ(ストックの上面から)
    #[serde(default = "twenty_mm")]
    pub clearance: String,
    /// 作業座標系の番号(1 = G54)
    #[serde(default = "one")]
    pub wcs: u32,
    /// 加工とシミュレーションで部品を三角形に分ける細かさ(長さ)。空なら [`MESH_TOLERANCE`]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<String>,
    /// 固定具(クランプ)。経路はこれを避ける
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fixtures: Vec<Fixture>,
    /// 一緒に削るほかのボディ(1 枚の板から複数の部品を削る時など)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub more_bodies: Vec<FeatureId>,
    /// 上(工具の側)へ向ける部品の面。上面以外を選ぶと、裏返し(`flip`)の代わりにこれで置く
    #[serde(default, skip_serializing_if = "Up::is_default")]
    pub up: Up,
    /// 上から見て部品を回す角度
    #[serde(default, skip_serializing_if = "Turn::is_none")]
    pub turn: Turn,
    /// 3D のビューで選んだ原点。あれば `origin`・`origin_z` の代わりに使う
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_at: Option<OriginAt>,
    /// 選んだ原点の説明(画面に出すだけ。`origin_at` と一緒に書き換える)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub origin_note: String,
    /// プログラム番号(G-code の先頭の O 番号)。空なら出さない
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<u32>,
    /// プログラムの先頭に入れるコメント
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
    /// ベッドの上の置き場所: L 字の基準の角から、ストックの手前左の角までの X・Y。空なら 0(角に当てる)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bed_x: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bed_y: Option<String>,
    /// プローブで表面の高さを測って、彫る深さを表面に沿わせる
    #[serde(default, skip_serializing_if = "Leveling::is_none")]
    pub leveling: Leveling,
    /// 複数の部品を板へ配置する。無い文書は通常のセットアップ。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nest: Option<nest::Nest>,
}

fn one() -> u32 {
    1
}

/// 機械の欄が無い古い文書の機械(欄を足す前は、この機械だけだった。文書の互換の値で、
/// 機械の値は組み立てが入れたマシンライブラリから読む)。
pub const LEGACY_MACHINE: &str = "carvera-air";

fn legacy_machine() -> String {
    LEGACY_MACHINE.into()
}

fn is_legacy_machine(id: &str) -> bool {
    id == LEGACY_MACHINE
}

/// 高さの補正(プローブで材料の表面を格子状に測る)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Leveling {
    #[serde(default)]
    pub mode: LevelingMode,
    /// 格子の点の数(X・Y)
    #[serde(default = "five")]
    pub nx: u32,
    #[serde(default = "five")]
    pub ny: u32,
    /// 点の間を動く高さ(材料の表面から)
    #[serde(default = "two_mm")]
    pub height: String,
    /// 測る範囲を、削る範囲から内へ狭める長さ(プローブが縁から落ちないよう)
    #[serde(default = "one_mm")]
    pub inset: String,
}

fn five() -> u32 {
    5
}

impl Default for Leveling {
    fn default() -> Self {
        Leveling {
            mode: LevelingMode::None,
            nx: 5,
            ny: 5,
            height: two_mm(),
            inset: one_mm(),
        }
    }
}

impl Leveling {
    fn is_none(&self) -> bool {
        *self == Leveling::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LevelingMode {
    /// 補正しない
    #[default]
    None,
    /// G-code の先頭で、機械の命令で測る(Carvera の G32 など)
    Gcode,
    /// 機械のコントローラーの側で測る(G-code には出さず、段取り表に書く)
    Controller,
}

/// セットアップの高さの補正(長さを解いた値)。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LevelingInfo {
    pub mode: LevelingMode,
    pub nx: u32,
    pub ny: u32,
    pub height: f64,
    pub inset: f64,
}

/// 上から見て部品を回す角度(反時計回り)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Turn {
    #[default]
    None,
    Ccw90,
    Half,
    Cw90,
}

impl Turn {
    pub fn is_none(&self) -> bool {
        *self == Turn::None
    }

    pub fn degrees(self) -> f64 {
        match self {
            Turn::None => 0.0,
            Turn::Ccw90 => 90.0,
            Turn::Half => 180.0,
            Turn::Cw90 => 270.0,
        }
    }
}

/// 3D のビューで選んだ原点。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OriginAt {
    /// ストックの箱の点。軸ごとに 0 = 最小の側、0.5 = 中、1 = 最大の側(機械に置いた向きで)
    Stock { at: [f64; 3] },
    /// 部品の外形の箱の点(機械に置いた向きで、軸ごとの割合)
    Model { at: [f64; 3] },
    /// 部品の上の点(部品を作った時の座標)
    Point { at: [f64; 3] },
}

impl OriginAt {
    /// 利用者に見せる説明。
    pub fn describe(&self) -> String {
        let axis = |v: f64, lo: &str, hi: &str| -> String {
            if v < 0.25 {
                lo.into()
            } else if v > 0.75 {
                hi.into()
            } else {
                "中".into()
            }
        };
        let frac = |a: [f64; 3]| {
            format!(
                "{}・{}・{}",
                axis(a[0], "左", "右"),
                axis(a[1], "手前", "奥"),
                axis(a[2], "下", "上")
            )
        };
        match self {
            OriginAt::Stock { at } => format!("ストックの箱の点({})", frac(*at)),
            OriginAt::Model { at } => format!("部品の外形の箱の点({})", frac(*at)),
            OriginAt::Point { at } => {
                format!("部品の点({:.3}, {:.3}, {:.3})", at[0], at[1], at[2])
            }
        }
    }
}

/// セットアップの出力。座標は WCS。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetupInfo {
    pub body: ShapeHandle,
    /// WCS の原点の世界座標
    pub origin: P3,
    pub stock_min: P3,
    pub stock_max: P3,
    pub safe_z: f64,
    /// 下へ早送りしてよい高さ(ストックの上面 + 1 mm)
    pub retract_z: f64,
    pub clearance_z: f64,
    pub wcs: u32,
    /// ストックの材料(空なら指定なし)
    #[serde(default)]
    pub material: String,
    /// 裏返しの向きと、回す軸の通る点(部品の外形の箱の中心、世界座標)
    #[serde(default, skip_serializing_if = "Flip::is_none")]
    pub flip: Flip,
    #[serde(default)]
    pub flip_center: P3,
    /// 部品の網の許容差(mm)
    #[serde(default = "mesh_tolerance")]
    pub tolerance: f64,
    /// 経路が避ける範囲の箱(WCS、固定具の外形に [`FIXTURE_MARGIN`] を足したもの)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keep_out: Vec<(P3, P3)>,
    /// 固定具そのものの箱(WCS、画面に描く)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fixtures: Vec<(P3, P3)>,
    /// 部品を置く回転(外形の箱の中心 `flip_center` のまわり)。無ければ `flip` の回し方
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rot: Option<Mat3>,
    /// 一緒に削るほかのボディ
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub more_bodies: Vec<ShapeHandle>,
    /// 部品(すべてのボディ)の外形の箱(WCS)
    #[serde(default)]
    pub part_min: P3,
    #[serde(default)]
    pub part_max: P3,
    /// ストックにしたボディ(ストックの決め方が「ボディから」の時)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stock_body: Option<ShapeHandle>,
    /// プログラム番号とコメント
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<u32>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
    /// L 字の基準の角から、ストックの手前左の角までの X・Y(mm)
    #[serde(default)]
    pub bed_offset: [f64; 2],
    /// 使う機械(マシンライブラリの ID)
    #[serde(default = "legacy_machine")]
    pub machine: String,
    /// 使う機械の値(セットアップが文書に写して持つ値。無ければライブラリの値)
    #[serde(default)]
    pub machine_values: kmb_post::MachineProfile,
    /// 高さの補正(しないなら `None`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leveling: Option<LevelingInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sections: Option<sections::Sections>,
}

impl SetupInfo {
    /// 使う機械の値(セットアップが文書に写して持つ値。経路の計算・ポスト・検査が同じ値を使う)。
    /// 知らない機械はセットアップの評価で誤りにしている。
    pub fn machine_profile(&self) -> &kmb_post::MachineProfile {
        &self.machine_values
    }

    /// L 字の基準の角(ストックの手前左の下の角から、ベッドの上の置き場所だけ戻した所。WCS)。
    /// ストックはベッドの上に置くので、高さはベッドの上面。
    pub fn fence_corner(&self) -> P3 {
        [
            self.stock_min[0] - self.bed_offset[0],
            self.stock_min[1] - self.bed_offset[1],
            self.stock_min[2],
        ]
    }

    /// 部品を置く回転の行列。
    pub fn matrix(&self) -> Mat3 {
        self.rot.unwrap_or_else(|| self.flip.matrix())
    }

    /// 部品の(置く前の)世界座標の点を、機械に置いた向きの世界座標にする。
    pub fn place(&self, p: P3) -> P3 {
        placement::rotate_about(&self.matrix(), p, self.flip_center)
    }

    /// [`SetupInfo::place`] の逆。
    pub fn unplace(&self, q: P3) -> P3 {
        placement::rotate_about(&placement::transpose(&self.matrix()), q, self.flip_center)
    }

    /// 部品の世界座標の点を、セットアップの座標(WCS)にする。
    pub fn to_setup(&self, p: P3) -> P3 {
        let q = self.place(p);
        [
            q[0] - self.origin[0],
            q[1] - self.origin[1],
            q[2] - self.origin[2],
        ]
    }

    /// WCS の座標を、部品の(置く前の)世界座標にする。3D のビューに重ねる時に使う。
    pub fn to_world(&self, p: P3) -> P3 {
        let o = self.origin;
        self.unplace([p[0] + o[0], p[1] + o[1], p[2] + o[2]])
    }

    /// 部品の網を、機械に置いた向きにする。
    pub fn placed_mesh(&self, m: &Mesh) -> Mesh {
        placement::rotate_mesh(&self.matrix(), m, self.flip_center)
    }

    /// 機械に置いた向きの網を、部品の(置く前の)世界座標へ戻す(3D のビューに重ねる時に使う)。
    pub fn placed_mesh_inverse(&self, m: &Mesh) -> Mesh {
        placement::rotate_mesh(&placement::transpose(&self.matrix()), m, self.flip_center)
    }

    /// 上から削った形(縦の柱)が、置く向きを変えても縦の柱のままか(上下だけが入れ替わる置き方)。
    /// 前のセットアップの削り跡を引き継げるのはこの時だけ。
    pub fn keeps_vertical(&self) -> bool {
        self.matrix()[2][2].abs() > 0.5
    }

    /// 加工するボディすべての網(置く前の世界座標)。
    pub fn part_mesh(&self, kernel: &dyn kmb_core::Kernel) -> Result<Mesh, EvalError> {
        part_mesh(kernel, self.body, &self.more_bodies, self.tolerance)
    }

    pub fn to_value(&self) -> Value {
        kmb_core::Payload::to_value(self)
    }

    pub fn from_value(v: &Value) -> Option<SetupInfo> {
        <SetupInfo as kmb_core::Payload>::from_value(v)
    }
}

impl kmb_core::Payload for SetupInfo {
    const TYPE: &'static str = SETUP_TYPE;
}

/// 加工の経路の出力(`toolpath`)。どのセットアップの経路かを添える。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolpathOutput {
    pub setup: FeatureId,
    pub path: Toolpath,
}

impl kmb_core::Payload for ToolpathOutput {
    const TYPE: &'static str = TOOLPATH_TYPE;
}

/// 加工の出力の名前: タブの付く経路。
pub const TAB_PATHS: &str = "tab_paths";

/// タブの付く経路(工具の中心。作業座標系)。`z` は底の面の高さ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TabPath {
    pub z: f64,
    /// 閉じた折れ線(最後の点は最初と同じ)
    pub pts: Vec<P2>,
}

/// 加工の出力 [`TAB_PATHS`] の中身。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct TabPaths {
    pub paths: Vec<TabPath>,
}

impl kmb_core::Payload for TabPaths {
    const TYPE: &'static str = "cam.tab_paths@1";
}

pub fn parse<T: DeserializeOwned>(d: &serde_json::Value) -> Result<T, EvalError> {
    serde_json::from_value(d.clone()).map_err(|e| EvalError::InvalidData(e.to_string()))
}

pub fn output(feature: FeatureId, slot: &str) -> InputRef {
    InputRef::Output {
        feature,
        slot: slot.into(),
    }
}

/// 式の依存をまとめる(重なりは除く)。
pub fn expr_refs<'a>(
    exprs: impl IntoIterator<Item = &'a String>,
) -> Result<Vec<InputRef>, EvalError> {
    let mut out = Vec::new();
    for e in exprs {
        for i in expr_inputs(e)? {
            if !out.contains(&i) {
                out.push(i);
            }
        }
    }
    Ok(out)
}

/// 式の中のパラメータ名を書き換える。変わったら真。
pub fn rename_all<'a>(
    exprs: impl IntoIterator<Item = &'a mut String>,
    old: &str,
    new: &str,
) -> Result<bool, EvalError> {
    let mut changed = false;
    for e in exprs {
        let r = kmb_core::rename_reference(e, old, new)?;
        if r != *e {
            *e = r;
            changed = true;
        }
    }
    Ok(changed)
}

pub fn invalid(m: impl Into<String>) -> EvalError {
    EvalError::InvalidData(m.into())
}

impl Setup {
    /// ボディ `body` を既定の値で削るセットアップ。機械は古い文書と同じ [`LEGACY_MACHINE`]
    /// (組み立てが入れたマシンライブラリに依らず同じ文書になる)。画面の「セットアップ」は
    /// [`Setup::set_machine`] で設定の機械に替える。
    pub fn new(body: FeatureId) -> Self {
        Setup {
            body,
            stock: Default::default(),
            origin: Default::default(),
            origin_z: Default::default(),
            flip: Default::default(),
            safe_height: five_mm(),
            clearance: twenty_mm(),
            wcs: 1,
            tolerance: None,
            fixtures: vec![],
            more_bodies: vec![],
            up: Up::PlusZ,
            turn: Turn::None,
            origin_at: None,
            origin_note: String::new(),
            program: None,
            comment: String::new(),
            bed_x: None,
            bed_y: None,
            leveling: Leveling::default(),
            nest: None,
            machine: LEGACY_MACHINE.into(),
            machine_values: None,
        }
    }

    /// 機械を選び、値を文書に写す。
    pub fn set_machine(&mut self, m: kmb_post::MachineProfile) {
        self.machine = m.id.clone();
        self.machine_values = Some(m);
    }

    /// 使う機械の値: 写した値(ID が欄と同じ時)、無ければマシンライブラリの値。
    pub fn machine_values(
        &self,
        lib: &kmb_post::machines::MachineLibrary,
    ) -> Result<kmb_post::MachineProfile, String> {
        match &self.machine_values {
            Some(m) if m.id == self.machine => Ok(m.clone()),
            _ => lib
                .get(&self.machine)
                .ok_or_else(|| format!("機械「{}」はマシンライブラリにありません", self.machine)),
        }
    }

    /// 部品を置く回転。上へ向ける面を選んでいなければ裏返しの回し方。
    pub fn matrix(&self) -> Mat3 {
        let base = if self.up.is_default() {
            self.flip.matrix()
        } else {
            self.up.matrix()
        };
        placement::mul(&placement::turn_matrix(self.turn.degrees()), &base)
    }

    fn exprs_mut(&mut self) -> Vec<&mut String> {
        let st = &mut self.stock;
        let mut v = vec![
            &mut st.margin_side,
            &mut st.margin_top,
            &mut st.margin_bottom,
            &mut st.width,
            &mut st.depth,
            &mut st.height,
            &mut self.safe_height,
            &mut self.clearance,
            &mut self.leveling.height,
            &mut self.leveling.inset,
        ];
        v.extend(
            [
                &mut st.margin_left,
                &mut st.margin_right,
                &mut st.margin_front,
                &mut st.margin_back,
                &mut st.round_to,
                &mut self.tolerance,
                &mut self.bed_x,
                &mut self.bed_y,
            ]
            .into_iter()
            .flatten(),
        );
        if let Some(n) = &mut self.nest {
            v.extend([&mut n.edge, &mut n.spacing, &mut n.grid]);
        }
        v
    }

    /// 部品の外形 `min`〜`max` から、ストックの箱(世界座標)を求める。
    fn stock_box(&self, ctx: &EvalCtx<'_>, min: P3, max: P3) -> Result<(P3, P3), EvalError> {
        let st = &self.stock;
        let len = |src: &str, what: &str| -> Result<f64, EvalError> {
            let v = length(ctx, src, what)?;
            if v < 0.0 || !v.is_finite() {
                return Err(invalid(format!("{what}は 0 以上にしてください")));
            }
            Ok(v)
        };
        let top = len(&st.margin_top, "上の余白")?;
        match st.mode {
            StockMode::Margin => {
                let side = len(&st.margin_side, "横の余白")?;
                let each = |o: &Option<String>, what: &str| match o {
                    Some(src) => len(src, what),
                    None => Ok(side),
                };
                let left = each(&st.margin_left, "左の余白")?;
                let right = each(&st.margin_right, "右の余白")?;
                let front = each(&st.margin_front, "手前の余白")?;
                let back = each(&st.margin_back, "奥の余白")?;
                let bottom = len(&st.margin_bottom, "下の余白")?;
                let mut smin = [min[0] - left, min[1] - front, min[2] - bottom];
                let mut smax = [max[0] + right, max[1] + back, max[2] + top];
                if let Some(src) = &st.round_to {
                    let step = len(src, "丸めの幅")?;
                    if step > 0.0 {
                        // 大きさを切り上げ、増えた分は左右(上下は下側)に等しく分ける
                        for i in 0..3 {
                            let size = smax[i] - smin[i];
                            let grow = (size / step - 1e-9).ceil() * step - size;
                            if i == 2 {
                                smin[i] -= grow;
                            } else {
                                smin[i] -= grow / 2.0;
                                smax[i] += grow / 2.0;
                            }
                        }
                    }
                }
                Ok((smin, smax))
            }
            // ボディの箱は呼ぶ側で求める
            StockMode::Body => Ok((min, max)),
            StockMode::Fixed => {
                let w = len(&st.width, "ストックの幅")?;
                let d = len(&st.depth, "ストックの奥行")?;
                let h = len(&st.height, "ストックの高さ")?;
                let part = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
                let tol = 1e-6;
                if w + tol < part[0] || d + tol < part[1] || h + tol < part[2] + top {
                    return Err(invalid(format!(
                        "部品({:.1} × {:.1} × {:.1} mm、上の余白 {top:.1} mm)が \
                         ストック({w:.1} × {d:.1} × {h:.1} mm)に入りません",
                        part[0], part[1], part[2]
                    )));
                }
                let (fx, fy) = match st.align {
                    Origin::FrontLeft => (0.0, 0.0),
                    Origin::FrontRight => (1.0, 0.0),
                    Origin::BackLeft => (0.0, 1.0),
                    Origin::BackRight => (1.0, 1.0),
                    Origin::Center => (0.5, 0.5),
                };
                // 寄せた側では部品とストックの面が揃う
                let x0 = min[0] - (w - part[0]) * fx;
                let y0 = min[1] - (d - part[1]) * fy;
                let ztop = max[2] + top;
                Ok(([x0, y0, ztop - h], [x0 + w, y0 + d, ztop]))
            }
        }
    }
}

impl Feature for Setup {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = vec![output(self.body, "body")];
        v.extend(self.more_bodies.iter().map(|b| output(*b, "body")));
        if let Some(n) = &self.nest {
            for p in &n.parts {
                let r = output(p.body, "body");
                if !v.contains(&r) {
                    v.push(r);
                }
            }
        }
        if self.stock.mode == StockMode::Body
            && let Some(b) = self.stock.body
        {
            v.push(output(b, "body"));
        }
        let mut c = self.clone();
        let exprs: Vec<String> = c.exprs_mut().into_iter().map(|s| s.clone()).collect();
        v.extend(expr_refs(&exprs)?);
        Ok(v)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let shape_of = |ctx: &EvalCtx<'_>, id: FeatureId| -> Result<ShapeHandle, EvalError> {
            match ctx.output(id, "body")? {
                Value::Shape(h) => Ok(*h),
                _ => Err(invalid(format!("{id} はボディではありません"))),
            }
        };
        let body = shape_of(ctx, self.body)?;
        let more = self
            .more_bodies
            .iter()
            .filter(|b| **b != self.body)
            .map(|b| shape_of(ctx, *b))
            .collect::<Result<Vec<_>, _>>()?;
        let mut warnings_pre: Vec<String> = Vec::new();
        let safe = length(ctx, &self.safe_height, "安全高さ")?;
        let clearance = length(ctx, &self.clearance, "退避方法")?;
        if safe < 1.0 {
            return Err(invalid("安全高さは 1 mm 以上にしてください"));
        }
        if clearance < safe {
            return Err(invalid("退避の高さは安全高さ以上にしてください"));
        }
        // 作業座標系は G54〜G59(1〜6)。機械が書けない番号は、欄の段階で知らせる
        if !(1..=6).contains(&self.wcs) {
            return Err(invalid(format!(
                "作業座標系は 1〜6(G54〜G59)にしてください(今は {})",
                self.wcs
            )));
        }
        let lib = ctx
            .service::<kmb_post::machines::MachineLibrary>()
            .ok_or_else(|| invalid("マシンライブラリが組み立てにありません"))?;
        let machine = self.machine_values(&lib).map_err(invalid)?;
        let wcs_warning = (self.wcs > machine.wcs_count).then(|| {
            format!(
                "作業座標系 {}(G{})は、{} では書き出せません(G54{})。ポストのプラグインなら使えます",
                self.wcs,
                53 + self.wcs,
                machine.name,
                if machine.wcs_count <= 1 {
                    " だけ".to_string()
                } else {
                    format!("〜G{}", 53 + machine.wcs_count)
                }
            )
        });
        let leveling = match self.leveling.mode {
            LevelingMode::None => None,
            mode => {
                let l = &self.leveling;
                if !(2..=20).contains(&l.nx) || !(2..=20).contains(&l.ny) {
                    return Err(invalid("高さの補正の格子の点の数は 2〜20 にしてください"));
                }
                let height = length(ctx, &l.height, "測る時に上げる高さ")?;
                let inset = length(ctx, &l.inset, "測る範囲を狭める長さ")?;
                if height <= 0.0 || inset < 0.0 {
                    return Err(invalid(
                        "測る時に上げる高さは 0 より大きく、狭める長さは 0 以上にしてください",
                    ));
                }
                if mode == LevelingMode::Gcode && machine.leveling.is_none() {
                    warnings_pre.push(format!(
                        "{} は G-code で高さを補正する命令を持ちません。「コントローラーで測る」にするか、補正の命令のある機械を選んでください",
                        machine.name
                    ));
                }
                Some(LevelingInfo {
                    mode,
                    nx: l.nx,
                    ny: l.ny,
                    height,
                    inset,
                })
            }
        };
        let tolerance = match &self.tolerance {
            Some(t) if !t.trim().is_empty() => length(ctx, t, "網の許容差")?,
            _ => MESH_TOLERANCE,
        };
        let (lo, hi) = TOLERANCE_RANGE;
        if !(lo..=hi).contains(&tolerance) {
            return Err(invalid(format!(
                "網の許容差は {lo} mm 以上 {hi} mm 以下にしてください"
            )));
        }
        let nested = self
            .nest
            .as_ref()
            .map(|n| nest::prepare(ctx, self, n, tolerance, &machine))
            .transpose()?;
        if let Some(n) = &nested
            && n.bodies.is_empty()
        {
            // 配置の診断は残し、配置されていない元のボディを加工の材料として渡さない。
            let mut out = Outputs::new();
            n.write_outputs(&mut out);
            let mut warnings = n.layout.warnings.clone();
            warnings.push(
                "配置できる部品がありません。板の寸法・余白・部品の向きを確かめてください".into(),
            );
            warnings.extend(wcs_warning);
            warnings.extend(warnings_pre);
            add_warnings(&mut out, warnings);
            return Ok(out);
        }
        let (body, more) = match &nested {
            Some(n) => (n.bodies[0], n.bodies[1..].to_vec()),
            None => (body, more),
        };
        // カーネルの外形の箱は余裕を持たせてあるので、網から正確な箱を求める
        let mesh = part_mesh(ctx.kernel(), body, &more, tolerance)?;
        let raw = MachiningModel::from_mesh(&mesh, [0.0; 3]);
        let center = [0, 1, 2].map(|i| (raw.min[i] + raw.max[i]) / 2.0);
        let rot = self.matrix();
        // 置いた向きの外形の箱(中心のまわりに回すので、中心は変わらない)
        let placed =
            MachiningModel::from_mesh(&placement::rotate_mesh(&rot, &mesh, center), [0.0; 3]);
        let (pmin, pmax) = (placed.min, placed.max);
        let mut warnings = Vec::new();
        let mut stock_body = None;
        let (smin, smax) = if let Some(n) = &nested {
            ([0.0; 3], [n.extent, n.size[1], n.thickness])
        } else if self.stock.mode == StockMode::Body {
            let id = self
                .stock
                .body
                .ok_or_else(|| invalid("ストックにするボディを選んでください"))?;
            let h = shape_of(ctx, id)?;
            if h == body || more.contains(&h) {
                return Err(invalid(
                    "ストックには、削る部品と別のボディを選んでください",
                ));
            }
            let m = ctx
                .kernel()
                .mesh(h, tolerance)
                .map_err(|e| EvalError::Kernel(e.to_string()))?;
            let sm = MachiningModel::from_mesh(&placement::rotate_mesh(&rot, &m, center), [0.0; 3]);
            let inside = (0..3).all(|i| sm.min[i] <= pmin[i] + 1e-3 && sm.max[i] >= pmax[i] - 1e-3);
            if !inside {
                warnings.push(
                    "部品がストックのボディの外形からはみ出しています。はみ出した所は削れません"
                        .to_string(),
                );
            }
            stock_body = Some(h);
            (sm.min, sm.max)
        } else {
            self.stock_box(ctx, pmin, pmax)?
        };
        let origin_max = nested
            .as_ref()
            .map_or(smax, |n| [n.size[0], n.size[1], n.thickness]);
        let origin = match self.origin_at {
            Some(OriginAt::Stock { at }) => {
                [0, 1, 2].map(|i| smin[i] + (origin_max[i] - smin[i]) * at[i])
            }
            Some(OriginAt::Model { at }) => {
                [0, 1, 2].map(|i| pmin[i] + (pmax[i] - pmin[i]) * at[i])
            }
            Some(OriginAt::Point { at }) => placement::rotate_about(&rot, at, center),
            None => {
                let (ox, oy) = match self.origin {
                    Origin::FrontLeft => (smin[0], smin[1]),
                    Origin::FrontRight => (origin_max[0], smin[1]),
                    Origin::BackLeft => (smin[0], origin_max[1]),
                    Origin::BackRight => (origin_max[0], origin_max[1]),
                    Origin::Center => (
                        (smin[0] + origin_max[0]) / 2.0,
                        (smin[1] + origin_max[1]) / 2.0,
                    ),
                };
                let oz = match self.origin_z {
                    OriginZ::Top => smax[2],
                    OriginZ::Bottom => smin[2],
                };
                [ox, oy, oz]
            }
        };
        let oz = origin[2];
        // 高さはどれもストックの上面から測る
        let top = smax[2] - oz;
        let bed = |src: &Option<String>, what: &str| -> Result<f64, EvalError> {
            match src {
                Some(s) if !s.trim().is_empty() => length(ctx, s, what),
                _ => Ok(0.0),
            }
        };
        let bed_offset = [
            bed(&self.bed_x, "ベッドの上の X")?,
            bed(&self.bed_y, "ベッドの上の Y")?,
        ];
        let info = SetupInfo {
            body,
            origin,
            stock_min: sub(smin, origin),
            stock_max: sub(smax, origin),
            safe_z: top + safe,
            retract_z: top + 1.0f64.min(safe),
            clearance_z: top + clearance,
            wcs: self.wcs,
            material: self.stock.material.trim().to_string(),
            flip: self.flip,
            flip_center: center,
            tolerance,
            keep_out: self
                .fixtures
                .iter()
                .map(|f| f.keep_out(smin[2] - oz))
                .collect(),
            fixtures: self.fixtures.iter().map(|f| f.body(smin[2] - oz)).collect(),
            rot: (rot != self.flip.matrix()).then_some(rot),
            more_bodies: more,
            part_min: sub(pmin, origin),
            part_max: sub(pmax, origin),
            stock_body,
            program: self.program,
            comment: self.comment.trim().to_string(),
            bed_offset,
            machine: self.machine.clone(),
            machine_values: machine.clone(),
            leveling,
            sections: nested.as_ref().map(|n| n.sections(origin)),
        };
        let mut out = Outputs::new();
        out.insert("setup".into(), info.to_value());
        if let Some(n) = nested {
            n.write_outputs(&mut out);
            warnings.extend(n.layout.warnings);
        }
        warnings.extend(wcs_warning);
        warnings.extend(warnings_pre);
        add_warnings(&mut out, warnings);
        Ok(out)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        let mut c = self.clone();
        Ok(rename_all(c.exprs_mut(), old, new)?
            .then(|| serde_json::to_value(&c).expect("JSON にできる")))
    }
}

/// ボディ `body` と `more` の網をつなげる(置く前の世界座標)。
pub fn part_mesh(
    kernel: &dyn kmb_core::Kernel,
    body: ShapeHandle,
    more: &[ShapeHandle],
    tolerance: f64,
) -> Result<Mesh, EvalError> {
    let mut v = Vec::with_capacity(1 + more.len());
    for h in std::iter::once(&body).chain(more) {
        v.push(
            kernel
                .mesh(*h, tolerance)
                .map_err(|e| EvalError::Kernel(e.to_string()))?,
        );
    }
    Ok(if v.len() == 1 {
        v.pop().expect("1 つある")
    } else {
        placement::merge_meshes(v)
    })
}

fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// 機械の欄(選びは組み立てが渡したマシンライブラリ)。
fn machine_field(lib: &kmb_post::machines::MachineLibrary) -> Field {
    let choices = lib.choices();
    let choices: Vec<(&str, &str)> = choices
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    Field::choice("machine", "機械", &choices)
}

/// 汎用の機械だけのマシンライブラリで登録する(単体の試験など)。
pub fn register(reg: &mut Registry) {
    register_with(
        reg,
        kmb_post::machines::MachineLibrary::new(kmb_post::machines::MachineLibrary::generic()),
    );
}

/// 組み立てが選んだマシンライブラリで登録する。機械の欄の選びと、写しの無い古い文書の機械の
/// 値がこの一覧を読む(一覧は共有の値として置く)。
fn register_with(reg: &mut Registry, lib: kmb_post::machines::MachineLibrary) {
    reg.register_described(
        Descriptor::new(SETUP, category::SETUP, "セットアップ")
            .icon("setup")
            .hint("削る部品、ストック、原点、安全高さを決める")
            .field(
                machine_field(&lib)
                    .help("作業範囲・回転数・G-code の書き方・付属品(レーザー・プローブ)が機械で決まる。新しいセットアップは設定の「機械」で選んだ機械と、その時の設定の値を文書に持つ(後で設定を直しても、今あるセットアップの経路と G-code は変わらない)。ここで機械を替えると、その機械の初めの値になる"),
            )
            .field(Field::output("body", "部品", "body"))
            .field(Field::choice("nest.method","並べ方",&[("boxes","外形の箱"),("shapes","形どおり")]).only("nest.method","boxes|shapes",""))
            .field(Field::choice("nest.sheet","板の大きさの取り方",&[("stock","ストック"),("machine","機械の作業範囲")]).only("nest.method","boxes|shapes",""))
            .field(Field::expr("nest.edge","縁の余白").length().only("nest.method","boxes|shapes",""))
            .field(Field::expr("nest.spacing","部品の間").length().only("nest.method","boxes|shapes",""))
            .field(Field::expr("nest.grid","格子の目").length().help("形どおりに並べる時に使う。外形の箱に切り替えても値を残す").only("nest.method","boxes|shapes",""))
            .field(Field::number("nest.max_sheets","板の枚数の上限").only("nest.method","boxes|shapes",""))
            .field(Field::choice("nest.output","区画の書き出し",&[("files","板ごとのファイル"),("stops","止めを入れた 1 ファイル")]).only("nest.method","boxes|shapes",""))
            .field(Field::rows("nest.parts","配置する部品",vec![
                Field::info("body","ボディ"),Field::number("count","個数"),
                Field::choice("rotate","回してよい向き",&[("free","自由"),("grain","木目をそろえる"),("fixed","固定")]),
                Field::choice("up","上に向ける面",&[("plus_z","上面"),("minus_z","下面"),("plus_x","右の側面"),("minus_x","左の側面"),("plus_y","奥の側面"),("minus_y","手前の側面")]),
            ]).only("nest.method","boxes|shapes",""))
            .field(
                Field::text("stock.material", "材料")
                    .help("工具の切削条件のプリセットを選ぶ。一覧から選ぶか、名前を打つ")
                    .suggest(&kmb_cam_tool::MATERIALS.iter().map(|m| m.name).collect::<Vec<_>>()),
            )
            .field(Field::choice(
                "stock.mode",
                "ストックの決め方",
                &[
                    ("margin", "部品+余白"),
                    ("fixed", "決まった大きさ"),
                    ("body", "ボディから"),
                ],
            ))
            .field(
                Field::opt_output("stock.body", "ストックにするボディ", "body")
                    .help("前に削った板や、素材の形をしたボディ。箱はそのボディの外形で、シミュレーションはその形から削り始める")
                    .only("stock.mode", "body", "margin"),
            )
            .field(Field::expr("stock.margin_side", "横の余白").length().only(
                "stock.mode",
                "margin",
                "margin",
            ))
            .field(
                Field::opt_expr("stock.margin_left", "左の余白").length()
                    .help("空なら横の余白")
                    .only("stock.mode", "margin", "margin"),
            )
            .field(
                Field::opt_expr("stock.margin_right", "右の余白").length()
                    .help("空なら横の余白")
                    .only("stock.mode", "margin", "margin"),
            )
            .field(
                Field::opt_expr("stock.margin_front", "手前の余白").length()
                    .help("空なら横の余白")
                    .only("stock.mode", "margin", "margin"),
            )
            .field(
                Field::opt_expr("stock.margin_back", "奥の余白").length()
                    .help("空なら横の余白")
                    .only("stock.mode", "margin", "margin"),
            )
            .field(Field::expr("stock.margin_bottom", "下の余白").length().only(
                "stock.mode",
                "margin",
                "margin",
            ))
            .field(
                Field::opt_expr("stock.round_to", "切り上げ").length()
                    .help("幅・奥行・高さをこの長さの倍数に切り上げる(例: 5 mm)")
                    .only("stock.mode", "margin", "margin"),
            )
            .field(Field::expr("stock.width", "幅(X)").length().only("stock.mode", "fixed", "margin"))
            .field(Field::expr("stock.depth", "奥行(Y)").length().only("stock.mode", "fixed", "margin"))
            .field(Field::expr("stock.height", "高さ(Z)").length().only("stock.mode", "fixed", "margin"))
            .field(
                Field::choice(
                    "stock.align",
                    "部品を寄せる所",
                    &[
                        ("center", "中央"),
                        ("front_left", "手前左"),
                        ("front_right", "手前右"),
                        ("back_left", "奥左"),
                        ("back_right", "奥右"),
                    ],
                )
                .only("stock.mode", "fixed", "margin"),
            )
            .field(unless_body_stock(
                Field::expr("stock.margin_top", "上の余白").length()
                    .help("部品の上面からストックの上面まで"),
            ))
            .field(
                Field::choice(
                    "up",
                    "上に向ける面",
                    &[
                        ("plus_z", "上面(作ったままの向き)"),
                        ("minus_z", "下面(裏返す)"),
                        ("plus_x", "右の側面(+X)"),
                        ("minus_x", "左の側面(-X)"),
                        ("plus_y", "奥の側面(+Y)"),
                        ("minus_y", "手前の側面(-Y)"),
                    ],
                )
                .help("部品のどの面を工具の側(上)へ向けて置くか。側面を上にすると、横穴や側面のポケットを上から削れる。上面以外を選ぶと「裏返し」は使わない"),
            )
            .field(
                Field::choice(
                    "turn",
                    "上から見て回す",
                    &[
                        ("none", "回さない"),
                        ("ccw90", "左へ 90°"),
                        ("half", "180°"),
                        ("cw90", "右へ 90°"),
                    ],
                )
                .help("置いた部品を、上から見て回す(長い辺を機械の X に合わせる時など)"),
            )
            .field(
                Field::choice(
                    "flip",
                    "裏返し",
                    &[
                        ("none", "しない(表から削る)"),
                        ("around_x", "X 軸のまわり(手前と奥が入れ替わる)"),
                        ("around_y", "Y 軸のまわり(左と右が入れ替わる)"),
                    ],
                )
                .help("2 つ目のセットアップで裏から削る時に選ぶ。部品の外形の箱の中心を軸にして回す。原点は中央にすると、ストックの寸法の誤差が裏表のずれになりにくい")
                .only("up", "plus_z", "plus_z"),
            )
            .field(
                Field::choice("origin", "原点(XY)", ORIGINS)
                    .help("「原点を 3D で選ぶ」で点を選ぶと、この欄の代わりにその点を使う"),
            )
            .field(Field::choice(
                "origin_z",
                "原点(Z)",
                &[("top", "ストックの上面"), ("bottom", "ストックの下面")],
            ))
            .field(Field::info("origin_note", "3D で選んだ原点"))
            .field(
                Field::expr("safe_height", "安全高さ").length()
                    .help("ストックの上面から。この高さで横に早送りする"),
            )
            .field(Field::expr("clearance", "退避高さ").length().help("工具交換と最後に戻る高さ"))
            .field(Field::integer("wcs", "作業座標系(1 = G54)").unitless().help(
                "1〜6(G54〜G59)。使える番号は機械で決まる(Carvera Air は 1 だけ、汎用は 1〜6)",
            ))
            .field(
                Field::opt_integer("program", "プログラム番号").unitless()
                    .help("G-code の先頭のコメントに「プログラム番号 O1001」のように出す。O 番号の行を書ける機械(汎用の Fanuc 系)では O1001 の行も出す。空なら出さない"),
            )
            .field(
                Field::text("comment", "プログラムのコメント")
                    .help("G-code の先頭にコメントとして出す(段取りの注意など)"),
            )
            .field(
                Field::choice(
                    "leveling.mode",
                    "高さの補正",
                    &[
                        ("none", "しない"),
                        ("gcode", "G-code の先頭で測る"),
                        ("controller", "コントローラーで測る"),
                    ],
                )
                .help("反った板や傾いた材料でも同じ深さで彫れるよう、プローブで表面の高さを格子状に測って補正する。「G-code の先頭で測る」は機械の命令(Carvera Air は G32)を先頭に、解除(M370)を終わりに出す。「コントローラーで測る」は G-code に出さず段取り表に書く(二重に測らないため)。測る範囲は削る範囲"),
            )
            .field(
                // 「高さの補正」を選んだ時は mode だけが入るので、ほかは空なら既定の値と分かる欄にする
                Field::opt_integer("leveling.nx", "格子の点の数 X").unitless()
                    .help("空なら 5。2〜20 で、細かいほど表面に沿うが、測る時間が延びる")
                    .unless("leveling.mode", "none"),
            )
            .field(
                Field::opt_integer("leveling.ny", "格子の点の数 Y").unitless()
                    .help("空なら 5。2〜20")
                    .unless("leveling.mode", "none"),
            )
            .field(
                Field::opt_expr("leveling.height", "測る時に上げる高さ").length()
                    .help("空なら 2 mm。点から点へ動く時の、表面からの高さ")
                    .unless("leveling.mode", "none"),
            )
            .field(
                Field::opt_expr("leveling.inset", "測る範囲を狭める").length()
                    .help("空なら 1 mm。削る範囲から内へ狭める長さ(プローブが材料の縁から落ちないよう)")
                    .unless("leveling.mode", "none"),
            )
            .field(
                Field::opt_expr("bed_x", "ベッドの上の置き場所 X").length()
                    .help("L 字の基準の角から、ストックの手前左の角までの距離。空なら角に当てて置く。ベッド・ネジ穴・作業範囲の線と、固定具が吸い付くネジ穴の位置が動く"),
            )
            .field(Field::opt_expr("bed_y", "ベッドの上の置き場所 Y").length().help("同じく Y の向き"))
            .field(Field::opt_expr("tolerance", "網の許容差").length().help(
                "3D の加工とシミュレーションで、部品の形を三角形に分ける細かさ。空なら 0.01 mm。粗くすると計算は速いが、曲面の経路も粗くなる",
            )),
        |d| Ok(Box::new(parse::<Setup>(d)?)),
    );
    manual::register(reg);
    pattern::register(reg);
    // セットアップと手で書く加工・加工のパターンも形を作らない(同時に計算してよい)
    for t in [SETUP, manual::MANUAL_NC, pattern::PATTERN] {
        reg.mark(t, kmb_core::CONCURRENT);
    }
    reg.provide(std::sync::Arc::new(lib));
}

/// ストックの決め方が「ボディから」でない時だけ使う欄にする。
fn unless_body_stock(mut f: Field) -> Field {
    f.when = Some(kmb_core::When {
        key: "stock.mode".into(),
        value: "body".into(),
        equal: false,
        default: "margin".into(),
        and: None,
    });
    f
}

const ORIGINS: &[(&str, &str)] = &[
    ("front_left", "手前左"),
    ("front_right", "手前右"),
    ("back_left", "奥左"),
    ("back_right", "奥右"),
    ("center", "中央"),
];

/// 加工の機能に共通の設定欄(`OpBase` の項目)。
pub fn op_fields() -> Vec<Field> {
    vec![
        Field::output("setup", "セットアップ", "setup"),
        Field::output("tools", "工具ライブラリ", "tools"),
        Field::integer("tool", "工具").unitless()
            .help("工具ライブラリの工具。一覧から選ぶか、番号を打つ")
            .suggest_from("tools", kmb_cam_tool::TOOL_CHOICES),
        Field::opt_expr("rpm", "主軸回転数(rpm)").plain("rpm").help("空なら工具の既定値"),
        Field::opt_expr("feed", "切削送り(mm/min)").length().help("空なら工具の既定値"),
        Field::opt_expr("plunge", "下降の送り(mm/min)").length().help("空なら工具の既定値"),
        Field::opt_expr("stepdown", "切込み").length().help("空なら工具の既定値"),
        Field::opt_expr("stepover", "切削ピッチ(工具径の比)").ratio().help("空なら工具の既定値"),
        Field::opt_expr("fit", "円弧のあてはめ(許容差)").length().help(
            "例: 0.005 mm。細かい直線を、この離れの中で円弧と長い直線にまとめる(G-code が小さくなり、動きが滑らかになる)。空ならしない",
        ),
        Field::opt_expr("stay_down", "下げたまま移る距離").length().help(
            "削った所の中を通って次の所へ移れる時、この距離までは上げずに切削の送りで移る。空なら 15 mm、0 mm ならいつも上げる",
        ),
        Field::opt_expr("lift", "持ち上げる高さ").length().help(
            "下げたまま移る時に、削った床や部品の上へ空ける高さ。空なら 0.2 mm、0 mm なら床の高さのまま移る",
        ),
        Field::choice(
            "retract",
            "退避方法",
            &[
                ("fit", "最小退避"),
                ("safe", "完全退避"),
            ],
        )
        .help("通り道に合わせる: 通り道の材料のいちばん高い所 + 安全高さの余裕まで上げる(ストックの上面 + 1 mm より下げない)。削った所の上を移る時に上げ下げが短くなる"),
        Field::choice(
            "sim_grid",
            "シミュレーションの精度",
            &[
                ("auto", "自動"),
                ("rough", "粗い"),
                ("fine", "細かい"),
            ],
        )
        .help("シミュレーションでこの加工を削る格子の細かさ。粗いは設定の「シミュレーション」の荒削りの格子、細かいは仕上げの格子の大きさ。自動は、負荷一定・ポケット・フェイス・3D の荒削りを粗い格子、ほかを細かい格子で削る。経路は変わらない"),
    ]
}

/// 高さの基準を選ぶ欄。`auto` は「自動」の時の高さの説明。
fn height_from_field(key: &str, label: &str, auto: &str) -> Field {
    let auto = format!("自動({auto})");
    let f = Field::choice(
        key,
        &format!("{label}の基準"),
        &[
            ("auto", auto.as_str()),
            ("top", "トップ高さ"),
            ("stock_top", "ストックの上面"),
            ("stock_bottom", "ストックの底面"),
            ("model_top", "部品の上面"),
            ("model_bottom", "部品の底面"),
            ("origin", "原点(Z 0)"),
        ],
    );
    if key == "top_from" || key == "bottom_from" {
        f.hide_choice("top")
    } else {
        f
    }
}

/// 加工の欄の最後にまとめる、空振りと高さの欄(安全・送り)。トップ・ボトムの欄([`height_fields`])は
/// この後に並ぶ。
pub fn link_height_fields() -> Vec<Field> {
    vec![
        Field::choice("air", "削った所の空振り", &[("skip", "省く"), ("keep", "省かない")]).help(
            "前の加工と、前のセットアップ(裏返す前を含む)で削った所を見て、工具が材料に触れない切削を、上げて早送りで移る動きか下げたままの短い送りに替える。触れるか疑わしい所は残す。加工を選んで書き出す・工具ごとに並べ替える時は、その順で省き直す",
        ),
        height_from_field("safe_from", "安全高さ", "セットアップ")
            .help("横に早送りする高さ。自動はセットアップの安全高さ"),
        Field::opt_expr("safe_offset", "安全高さのオフセット").length()
            .help("例: 3 mm。基準の高さに足す(下げるなら負の値)"),
        height_from_field("feed_from", "送り高さ", "トップ高さ + 1 mm")
            .help("早送りで下りて、切削の送りに替える高さ。トップ高さより下にはできない"),
        Field::opt_expr("feed_offset", "送り高さのオフセット").length()
            .help("例: 2 mm。基準の高さに足す(下げるなら負の値)"),
    ]
}

/// 高さの欄の鍵(加工の欄の最後へまとめて並べる)。
const HEIGHT_KEYS: [&str; 4] = ["top_from", "top_offset", "bottom_from", "bottom_offset"];

/// 加工の欄 `fields` の後に、トップ高さとボトム高さの欄を足す。
pub fn with_heights(mut fields: Vec<Field>) -> Vec<Field> {
    fields.extend(height_fields());
    fields
}

/// トップ高さとボトム高さの欄(高さを当てる加工だけが並べる)。
pub fn height_fields() -> [Field; 4] {
    [
        height_from_field("top_from", "トップ高さ", "加工が決める")
            .help("削り始める高さ。自動はふつうストックの上面(穴あけは「自動のトップ高さ」の欄)。前の加工で削った所の途中から始める時に下げる"),
        Field::opt_expr("top_offset", "トップ高さのオフセット").length()
            .help("例: -3 mm。基準の高さに足す(自動にも足せる)"),
        height_from_field("bottom_from", "ボトム高さ", "加工が決める")
            .help("削る底。自動は選んだ面・輪郭・穴の底など"),
        Field::opt_expr("bottom_offset", "ボトム高さのオフセット").length()
            .help("例: 0.2 mm。底を少し浅く止める時は正の値(自動にも足せる)"),
    ]
}

/// 加工の機能の記述子(共通の欄つき)。`fields` はその後に並び、空振りと高さの欄は最後にまとめる
/// (別のタブの代わり。戦略ごとの欄を上に置く)。
pub fn operation(type_id: &str, label: &str, hint: &str, fields: Vec<Field>) -> Descriptor {
    let (heights, rest): (Vec<Field>, Vec<Field>) = fields
        .into_iter()
        .partition(|f| HEIGHT_KEYS.contains(&f.key.as_str()));
    Descriptor::new(type_id, category::OPERATION, label)
        .parent("setup")
        .hint(hint)
        .fields(op_fields())
        .fields(rest)
        .fields(link_height_fields())
        .fields(heights)
}

/// この crate の名乗り(登録する種類)。組み立て(kmb-bundle)が、選んだマシンライブラリを渡して
/// 1 回だけ並べる。
pub fn module(machines: kmb_post::machines::MachineLibrary) -> kmb_core::Module {
    kmb_core::Module::new("kmb-cam-setup", move |reg| {
        register_with(reg, machines.clone())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_level_can_be_a_thin_finishing_step() {
        // 6 mm を 2 mm ずつ: 3 段
        assert_eq!(split_levels(0.0, -6.0, 2.0, None), vec![-2.0, -4.0, -6.0]);
        // 最後を 0.2 mm にすると、5.8 mm を 3 段に等しく分けてから 0.2 mm を削る
        let v = split_levels(0.0, -6.0, 2.0, Some(0.2));
        assert_eq!(v.len(), 4);
        assert!((v[2] + 5.8).abs() < 1e-9 && v[3] == -6.0, "{v:?}");
        assert!(v.windows(2).all(|w| w[0] - w[1] <= 2.0 + 1e-9));
        // 深さが最後の段の厚さ以下なら、等しく分けるだけ
        assert_eq!(split_levels(0.0, -0.1, 2.0, Some(0.2)), vec![-0.1]);
    }

    #[test]
    fn rounding_the_corners_keeps_the_path_away_from_sharp_corners() {
        let sq = Area {
            outer: Loop::from_points(&[[0.0, 0.0], [20.0, 0.0], [20.0, 10.0], [0.0, 10.0]]),
            holes: vec![],
        };
        let r = round_corners(vec![sq.clone()], 2.0);
        assert_eq!(r.len(), 1);
        // 角の頂点は外れ、辺の中ほどは残る
        assert!(!r[0].contains([0.1, 0.1]) && r[0].contains([10.0, 0.1]));
        let lost = sq.area() - r[0].area();
        let want = 4.0 * 4.0 * (1.0 - std::f64::consts::PI / 4.0);
        assert!((lost - want).abs() < 0.05, "{lost} {want}");
        // 丸めると消える細い領域は、そのまま残す
        let thin = Area {
            outer: Loop::from_points(&[[0.0, 0.0], [20.0, 0.0], [20.0, 1.0], [0.0, 1.0]]),
            holes: vec![],
        };
        assert_eq!(round_corners(vec![thin.clone()], 2.0), vec![thin]);
        assert_eq!(round_corners(vec![sq.clone()], 0.0), vec![sq]);
    }
}
