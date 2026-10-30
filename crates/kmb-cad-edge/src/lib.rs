//! 辺のフィレットと面取り。カーネルの丸めは曲がった辺も扱う。
//! 柱で削る方法は、平面どうしの直線と、平面に垂直な円筒の円の辺を扱う。
//!
//! 両側の幅が違う面取りは、指定幅を保つ柱で削る方法を使う。
//! それ以外の曲がった辺では一定幅を保証できないため、種類を替えるよう知らせる。
//! 柱のブーリアンは接する面で失敗しやすいので、[`OFFSETS`] の量だけ外へずらす。
//! へこんだ辺では、足元の穴やポケットを埋めないよう [`shadow`] で柱を切る。
//! 大きさが面に収まらない時は最大値を知らせ、`fit` が真なら値を下げて警告を出す。

use kmb_cad::*;
use kmb_core::kernel::{self as k, P3, Plane, Profile, ProfileEdge, ProfileLoop, Region, Segment};
use kmb_core::{
    BooleanOp, EdgeInfo, EvalCtx, EvalError, FaceInfo, Feature, FeatureId, Field, InputRef,
    Outputs, Ref, RefKind, Registry, Retarget, Value,
};
use serde::{Deserialize, Serialize};

mod arc;
mod commands;
mod corner;
mod limits;
mod shadow;
mod shell;
pub use commands::{EdgeCommands, register};
use corner::{ball_corner, corners, patch_corner};
use limits::{fit_value, limits};
use shadow::{OUTLINE_TOL, reach, run_out, shadow};
pub use shell::{SHELL, Shell, ShellDirection};

pub const FILLET: &str = "solid.fillet@1";
pub const CHAMFER: &str = "solid.chamfer@1";
/// 選んだ面のまわりの辺を丸める操作。
pub const FILLET_FACE: &str = "cad.fillet";
/// 選んだ面のまわりの辺を面取りする操作。
pub const CHAMFER_FACE: &str = "cad.chamfer";

/// 丸めの円を外へずらす量の候補(mm)。ブーリアンに失敗したら次の値で試す。
const OFFSETS: [f64; 3] = [0.001, 0.003, 0.01];
/// 柱を辺の両端より先へ延ばす長さ(mm)
const EXTEND: f64 = 1.0;
/// 角の外側(へこんだ辺では中身の側)へ柱をはみ出させる長さ(mm)
const OUTSIDE: f64 = 0.1;
/// へこんだ辺が外の面で終わるとき、柱をその面より手前で止める量(mm)。
/// 柱の端が外の面と同じ平面になるとブーリアンが失敗するため。
const INSET: f64 = 0.001;
/// 球の角の球を、丸めの円筒より大きくする量(mm)。ちょうど接するとブーリアンが失敗するため
const BALL_GROW: f64 = 0.005;
/// 三角の面の角の平面を、面取りの境の点から頂点の側へずらす量(mm)
const PATCH_INSET: f64 = 0.002;
/// 辺の先が曲面で下がっていくかを調べる点を、角の内側へ入れる量(mm)。網のずれ(`OUTLINE_TOL`)と同じ程度
const RUN_OUT_PROBE: f64 = 0.002;
/// 収まる最大の値から引いておく量(mm)。面の幅いっぱいに丸めると面が消え、ブーリアンが失敗するため
const FIT_MARGIN: f64 = 0.005;
/// 面の幅を測る、辺に沿った位置(辺の長さに対する割合)
const SAMPLES: [f64; 9] = [0.02, 0.1, 0.2, 0.35, 0.5, 0.65, 0.8, 0.9, 0.98];

/// 辺の選び方。`edges` は両側の面の名前の組、`faces` はその面のまわりの出っ張った辺すべて
/// (出っ張った辺がなければへこんだ辺すべて)。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EdgeSelection {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<[String; 2]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub faces: Vec<String>,
    /// `faces` のまわりの円の辺(穴の縁、円柱の上面の縁など)も入れる。
    /// 項目の無い前の文書は、形を変えないよう入れない(画面から新しく足すものは入れる)
    #[serde(default, skip_serializing_if = "is_false")]
    pub arcs: bool,
}

impl EdgeSelection {
    /// `chain` が真なら、選んだ辺から接してつながる辺もたどって入れる(面のまわりの辺はそのまま)。
    fn resolve(&self, all: &[EdgeInfo], chain: bool) -> Result<Vec<EdgeInfo>, EvalError> {
        let mut out: Vec<EdgeInfo> = Vec::new();
        let mut push = |e: &EdgeInfo| {
            if !out.contains(e) {
                out.push(e.clone());
            }
        };
        for [a, b] in &self.edges {
            let found: Vec<&EdgeInfo> = all
                .iter()
                .filter(|e| {
                    (&e.faces[0] == a && &e.faces[1] == b) || (&e.faces[0] == b && &e.faces[1] == a)
                })
                .collect();
            if found.is_empty() {
                return Err(EvalError::InvalidData(format!(
                    "面 {a} と {b} の間の辺が見つかりません(参照が切れたか、平面と円筒の境目でない曲がった辺です)"
                )));
            }
            if chain {
                let seeds: Vec<usize> = found
                    .iter()
                    .filter_map(|f| all.iter().position(|e| std::ptr::eq(e, *f)))
                    .collect();
                for i in kmb_cad::tangent_chains(all, &seeds, kmb_cad::TANGENT_DEG) {
                    push(&all[i]);
                }
            } else {
                for e in found {
                    push(e);
                }
            }
        }
        for f in &self.faces {
            // 出っ張った辺を選ぶ。なければ(ポケットの底など)へこんだ辺を選ぶ
            let of = |convex: bool| -> Vec<&EdgeInfo> {
                all.iter()
                    .filter(|e| e.convex == convex && e.faces.contains(f))
                    .filter(|e| self.arcs || e.arc.is_none())
                    .collect()
            };
            let mut around = of(true);
            if around.is_empty() {
                around = of(false);
            }
            if around.is_empty() {
                return Err(EvalError::InvalidData(format!(
                    "面 {f} のまわりに丸められる辺がありません(参照が切れていないか確かめてください)"
                )));
            }
            for e in around {
                push(e);
            }
        }
        if out.is_empty() {
            return Err(EvalError::InvalidData("辺を選んでください".into()));
        }
        Ok(out)
    }
}

/// フィレットの 1 行: 選んだ辺と面、その半径。
/// 例: `{"edges": [{"kind": "face", "feature": 7, "face": "f2:end"}], "radius": "2 mm"}`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilletRow {
    /// 辺(両側の面の組)と面(そのまわりの辺すべて)
    #[serde(
        default,
        with = "kmb_core::one_or_many",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub edges: Vec<Ref>,
    pub radius: String,
    /// 面のまわりの円の辺を入れない(項目の無かった前の文書の 1 組を、形を変えずに読むため)
    #[serde(default, skip_serializing_if = "is_false")]
    pub straight_only: bool,
    /// 半径の種類。弦の長さでは `radius` が弦の長さ、途中で変わる
    /// 半径では `radius` が始めの半径
    #[serde(default, skip_serializing_if = "RadiusKind::is_constant")]
    pub kind: RadiusKind,
    /// 途中で変わる半径の、終わりの半径(空なら始めと同じ)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_radius: Option<String>,
    /// 途中で変わる半径の、途中の点の半径。「位置 = 半径」を `;` で区切る(位置は始めの端を 0、
    /// 終わりの端を 1 とした割合。例: `0.5 = 3 mm; 0.75 = radius * 2`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mid_radii: Option<String>,
    /// 断面のつなぎ方
    #[serde(default, skip_serializing_if = "Continuity::is_g1")]
    pub continuity: Continuity,
}

/// フィレットの半径の種類。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RadiusKind {
    #[default]
    Constant,
    /// 丸めの両端(両側の面に接する所)の間の距離を、半径の欄の値にそろえる
    Chord,
    /// 辺の始めから終わりへ半径を変える(途中の点も置ける)。始めは辺の端のうち X の小さい方
    /// (同じなら Y、Z の小さい方)
    Variable,
}

impl RadiusKind {
    fn is_constant(&self) -> bool {
        *self == RadiusKind::Constant
    }
}

/// フィレットの断面のつなぎ方。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Continuity {
    /// 円の断面。面と接線までつながる
    #[default]
    G1,
    /// 曲率までなめらかにつながる断面
    G2,
}

impl Continuity {
    fn is_g1(&self) -> bool {
        *self == Continuity::G1
    }
}

/// 途中の点の半径の欄(`位置 = 半径; …`)を読む(式はまだ計算しない)。
fn mid_points(src: &str) -> Result<Vec<(f64, String)>, EvalError> {
    let mut out = Vec::new();
    for part in src.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let bad = || {
            EvalError::InvalidData(format!(
                "途中の半径 `{part}` は「位置 = 半径」で書いてください(位置は 0 より大きく 1 より小さい割合。例: 0.5 = 3 mm)"
            ))
        };
        let (at, r) = part.split_once('=').ok_or_else(bad)?;
        let at: f64 = at.trim().parse().map_err(|_| bad())?;
        if !(at > 0.0 && at < 1.0) || r.trim().is_empty() {
            return Err(bad());
        }
        out.push((at, r.trim().to_string()));
    }
    if out.len() > MAX_MID_POINTS {
        return Err(EvalError::InvalidData(format!(
            "途中の半径は {MAX_MID_POINTS} 個までです"
        )));
    }
    Ok(out)
}

/// 途中の点の数の上限。
const MAX_MID_POINTS: usize = 20;

/// 面取りの 1 行: 選んだ辺と面、その距離。種類([`ChamferKind`])が 2 つの距離なら `distance2`、
/// 距離と角度なら `angle` も使う。距離 1 は、辺の両側の面のうち上を向いた面(同じなら +Y、+X の
/// 向きの面)に取る。円の辺では平らな面の側。`flip` で反対の面にする。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChamferRow {
    #[serde(
        default,
        with = "kmb_core::one_or_many",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub edges: Vec<Ref>,
    pub distance: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance2: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub flip: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub straight_only: bool,
}

/// 面取りの種類(スケッチの面取りともそろえる)。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChamferKind {
    /// 両側の面から同じ距離
    #[default]
    Equal,
    /// 距離 1 の面から `distance`、もう片方の面から `distance2`
    TwoDistances,
    /// 距離 1 の面から `distance`、その面と面取りの面の角が `angle`
    DistanceAngle,
}

impl ChamferKind {
    fn is_equal(&self) -> bool {
        *self == ChamferKind::Equal
    }
}

/// 辺を丸める。行ごとに違う半径を持てる。
/// 例: `{"target": 7, "rows": [{"edges": {"kind": "face", "feature": 7, "face": "f2:end"}, "radius": "2 mm"}]}`。
/// 前の 1 組だけの形 `{"target": 7, "faces": ["f2:end"], "radius": "2 mm"}` も読む(1 行として)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "OldOrRows<FilletRow>")]
pub struct Fillet {
    pub target: FeatureId,
    pub rows: Vec<FilletRow>,
    /// 半径が収まらない辺だけ、収まる最大の半径に下げる
    #[serde(default, skip_serializing_if = "is_false")]
    pub fit: bool,
    /// 選んだ 3 本の辺が集まる角の形(`Miter` か `Ball`)
    #[serde(default, skip_serializing_if = "Corner::is_miter")]
    pub corner: Corner,
    /// 選んだ辺から接してつながる辺も入れる。計算のたびに選んだ辺から
    /// たどり直す。項目の無い前の文書は、形を変えないよう入れない(画面から新しく作るものは入れる)
    #[serde(default, skip_serializing_if = "is_false")]
    pub chain: bool,
    /// 丸め方。項目の無い前の文書は、形を変えないよう `Cut`(画面から新しく作るものは `Blend`)
    #[serde(default, skip_serializing_if = "Method::is_cut")]
    pub method: Method,
    /// 曲率(G2)の断面のふくらみ(空なら 1)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<f64>,
}

/// 辺を面取りする(既定は両側の面から同じ距離。`kind` で 2 つの距離・距離と角度)。行ごとに違う距離を持てる。
/// 例: `{"target": 7, "kind": "distance_angle", "rows": [{"edges": …, "distance": "2 mm", "angle": "30 deg"}]}`。
/// 前の形 `{"target": 7, "edges": [["f2:end", "f2:side:e1"]], "distance": "1 mm"}` も読む。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "OldOrRows<ChamferRow>")]
pub struct Chamfer {
    pub target: FeatureId,
    #[serde(default, skip_serializing_if = "ChamferKind::is_equal")]
    pub kind: ChamferKind,
    pub rows: Vec<ChamferRow>,
    /// 距離が収まらない辺だけ、収まる最大の距離に下げる
    #[serde(default, skip_serializing_if = "is_false")]
    pub fit: bool,
    /// 選んだ 3 本の辺が集まる角の形(`Miter` か `Patch`)
    #[serde(default, skip_serializing_if = "Corner::is_miter")]
    pub corner: Corner,
    /// 選んだ辺から接してつながる辺も入れる([`Fillet::chain`] と同じ)
    #[serde(default, skip_serializing_if = "is_false")]
    pub chain: bool,
    /// 丸め方([`Fillet::method`] と同じ。カーネルの丸めでは、断面が直線になる)
    #[serde(default, skip_serializing_if = "Method::is_cut")]
    pub method: Method,
}

/// 行(フィレットと面取りで共通の扱い)。
pub trait EdgeRow {
    fn refs(&self) -> &[Ref];
    fn value(&self) -> &str;
    fn value_mut(&mut self) -> &mut String;
    fn straight_only(&self) -> bool;
    fn new(edges: Vec<Ref>, value: String, straight_only: bool) -> Self;
    /// 前の形の値の項目名
    const VALUE: &'static str;
}

impl EdgeRow for FilletRow {
    fn refs(&self) -> &[Ref] {
        &self.edges
    }
    fn value(&self) -> &str {
        &self.radius
    }
    fn value_mut(&mut self) -> &mut String {
        &mut self.radius
    }
    fn straight_only(&self) -> bool {
        self.straight_only
    }
    fn new(edges: Vec<Ref>, radius: String, straight_only: bool) -> Self {
        FilletRow {
            edges,
            radius,
            straight_only,
            kind: RadiusKind::Constant,
            end_radius: None,
            mid_radii: None,
            continuity: Continuity::G1,
        }
    }
    const VALUE: &'static str = "radius";
}

impl EdgeRow for ChamferRow {
    fn refs(&self) -> &[Ref] {
        &self.edges
    }
    fn value(&self) -> &str {
        &self.distance
    }
    fn value_mut(&mut self) -> &mut String {
        &mut self.distance
    }
    fn straight_only(&self) -> bool {
        self.straight_only
    }
    fn new(edges: Vec<Ref>, distance: String, straight_only: bool) -> Self {
        ChamferRow {
            edges,
            distance,
            distance2: None,
            angle: None,
            flip: false,
            straight_only,
        }
    }
    const VALUE: &'static str = "distance";
}

/// 読む時の形: 今の行の並びか、前の 1 組だけの形(`edges`・`faces`・`arcs` と値)。
#[derive(Deserialize)]
struct OldOrRows<R> {
    target: FeatureId,
    #[serde(default = "Option::default")]
    rows: Option<Vec<R>>,
    #[serde(flatten)]
    select: EdgeSelection,
    #[serde(default, alias = "distance")]
    radius: Option<String>,
    #[serde(default)]
    fit: bool,
    #[serde(default)]
    corner: Corner,
    #[serde(default)]
    chain: bool,
    #[serde(default)]
    method: Method,
    /// 面取りの種類(フィレットには無い)
    #[serde(default)]
    kind: ChamferKind,
    /// 接線ウェイト(面取りには無い)
    #[serde(default)]
    weight: Option<f64>,
}

impl<R: EdgeRow> OldOrRows<R> {
    fn rows(self) -> (FeatureId, Vec<R>, bool, Corner, bool, Method) {
        let rows = match self.rows {
            Some(rows) => rows,
            None => {
                let (refs, straight) = row_refs(self.target, self.select);
                vec![R::new(refs, self.radius.unwrap_or_default(), straight)]
            }
        };
        (
            self.target,
            rows,
            self.fit,
            self.corner,
            self.chain,
            self.method,
        )
    }
}

impl From<OldOrRows<FilletRow>> for Fillet {
    fn from(o: OldOrRows<FilletRow>) -> Self {
        let weight = o.weight;
        let (target, rows, fit, corner, chain, method) = o.rows();
        Fillet {
            target,
            rows,
            fit,
            corner,
            chain,
            method,
            weight,
        }
    }
}

impl From<OldOrRows<ChamferRow>> for Chamfer {
    fn from(o: OldOrRows<ChamferRow>) -> Self {
        let kind = o.kind;
        let (target, rows, fit, corner, chain, method) = o.rows();
        Chamfer {
            target,
            kind,
            rows,
            fit,
            corner,
            chain,
            method,
        }
    }
}

/// 辺の選び方 1 組を、1 行の参照にする(前の形と同じ選び方)。
fn row_refs(target: FeatureId, select: EdgeSelection) -> (Vec<Ref>, bool) {
    let mut refs: Vec<Ref> = select
        .edges
        .into_iter()
        .map(|faces| Ref::Edge {
            feature: target,
            faces,
        })
        .collect();
    refs.extend(select.faces.into_iter().map(|face| Ref::Face {
        feature: target,
        face,
    }));
    (refs, !select.arcs)
}

impl Fillet {
    /// 1 行だけのフィレット(辺の選び方 1 組と半径)。
    pub fn one(
        target: FeatureId,
        select: EdgeSelection,
        radius: impl Into<String>,
        fit: bool,
        corner: Corner,
    ) -> Self {
        let (refs, straight) = row_refs(target, select);
        Fillet {
            target,
            rows: vec![FilletRow::new(refs, radius.into(), straight)],
            fit,
            corner,
            chain: false,
            method: Method::Cut,
            weight: None,
        }
    }
}

impl Chamfer {
    /// 1 行だけの面取り(辺の選び方 1 組と距離)。
    pub fn one(
        target: FeatureId,
        select: EdgeSelection,
        distance: impl Into<String>,
        fit: bool,
        corner: Corner,
    ) -> Self {
        let (refs, straight) = row_refs(target, select);
        Chamfer {
            target,
            kind: ChamferKind::Equal,
            rows: vec![ChamferRow::new(refs, distance.into(), straight)],
            fit,
            corner,
            chain: false,
            method: Method::Cut,
        }
    }
}

/// 行の参照を、辺の選び方にする(`target` のボディの面の名前で探す)。
fn row_selection(row: &impl EdgeRow) -> Result<EdgeSelection, EvalError> {
    let mut sel = EdgeSelection {
        edges: vec![],
        faces: vec![],
        arcs: !row.straight_only(),
    };
    for r in row.refs() {
        match r {
            Ref::Edge { faces, .. } => sel.edges.push(faces.clone()),
            Ref::Face { face, .. } => sel.faces.push(face.clone()),
            Ref::Vertex { .. } | Ref::Body(_) => {
                return Err(EvalError::InvalidData(
                    "フィレット・面取りには辺か面を選んでください(頂点やボディは使えません)".into(),
                ));
            }
        }
    }
    Ok(sel)
}

/// 前の 1 組だけの形のデータを、行の並びの形にする(画面の欄が読めるように)。
fn upgrade<T: Serialize + for<'de> Deserialize<'de>>(data: &mut serde_json::Value) {
    if data.get("rows").is_some() {
        return;
    }
    if let Ok(v) = serde_json::from_value::<T>(data.clone())
        && let Ok(new) = serde_json::to_value(v)
    {
        *data = new;
    }
}

/// 選んだ 3 本の出っ張った辺が 1 つの頂点に集まる角(箱の角など)の形。
/// 2 本だけが集まる角や、へこんだ辺の角は、いつも `Miter` の形になる。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Corner {
    /// 辺ごとの丸め(面取り)をそのまま延ばして交える。角に折れ目が集まる(前からの形)
    #[default]
    Miter,
    /// 3 つの面に接する球で角をなめらかにつなぐ(フィレットだけ)
    Ball,
    /// 角に平らな三角の面を置き、面取りの面を角の手前で終える(面取りだけ)
    Patch,
}

impl Corner {
    fn is_miter(&self) -> bool {
        *self == Corner::Miter
    }
}

/// 画面の選択肢(保存する値, 表示名)。最初が、項目がない時の形。
const FILLET_CORNERS: &[(&str, &str)] = &[("miter", "交わり"), ("ball", "ローリングボール")];
const RADIUS_KINDS: &[(&str, &str)] = &[
    ("constant", "一定"),
    ("chord", "弦の長さ"),
    ("variable", "徐変"),
];
const CONTINUITIES: &[(&str, &str)] = &[("g1", "接線(G1)"), ("g2", "曲率(G2)")];
const CHAMFER_CORNERS: &[(&str, &str)] = &[("miter", "交わり"), ("patch", "三角の面")];
const CHAMFER_KINDS: &[(&str, &str)] = &[
    ("equal", "等距離"),
    ("two_distances", "2 つの距離"),
    ("distance_angle", "距離と角度"),
];

/// 丸め方。前の文書は柱で削る形(`Cut`)のまま読み、新しく作るものはカーネルの丸め(`Blend`)にする。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Method {
    /// 角を削り取る柱(へこんだ辺では埋める柱)を足し引きする(前からの形。truck のカーネルもこれ)。
    /// 辺は平面どうしと、平面と垂直な円筒の境目だけ
    #[default]
    Cut,
    /// カーネルの丸め(SMLib の SmApiFilletEdgesPerEdge)。両側の面にちょうど接し、どの辺でも使える。
    /// 辺の集まる角はカーネルがなめらかにつなぐ。できないカーネルでは `Cut` で作る
    Blend,
}

impl Method {
    fn is_cut(&self) -> bool {
        *self == Method::Cut
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Exprs for Fillet {
    fn exprs_mut(&mut self) -> Vec<&mut String> {
        self.rows
            .iter_mut()
            .flat_map(|r| std::iter::once(&mut r.radius).chain(r.end_radius.as_mut()))
            .collect()
    }
}

impl Exprs for Chamfer {
    fn exprs_mut(&mut self) -> Vec<&mut String> {
        self.rows
            .iter_mut()
            .flat_map(|r| {
                std::iter::once(&mut r.distance)
                    .chain(r.distance2.as_mut())
                    .chain(r.angle.as_mut())
            })
            .collect()
    }
}

/// フィレットの行の半径の決め方(面取りの行は一定)。カーネルの丸めだけが一定以外を作れる。
#[derive(Clone, Debug, Default, PartialEq)]
struct RowRound {
    law: RowLaw,
    g2: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
enum RowLaw {
    #[default]
    Constant,
    /// 行の大きさは弦の長さ(辺ごとに、両側の面の角から半径に直して収まりを調べる)
    Chord,
    /// (位置, 半径) の組。位置は始めの端 0 から終わりの端 1。行の大きさは一番大きい半径
    Variable(Vec<(f64, f64)>),
}

impl RowRound {
    fn is_plain(&self) -> bool {
        self.law == RowLaw::Constant && !self.g2
    }
}

/// 辺の始めの端: X の小さい方(同じなら Y、Z の小さい方。閉じた辺は継ぎ目)。途中で変わる半径の向きを、
/// カーネルの辺の向きによらず決めるため。
fn edge_start(e: &EdgeInfo) -> P3 {
    let key = |p: P3| p.map(|v| (v * 1e6).round());
    if key(e.b) < key(e.a) { e.b } else { e.a }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Shape {
    Round(f64),
    Flat(f64),
    /// 行の 2 つの距離の面取り(距離 1 が `d`、距離 2 が `d2`。辺ごとに [`Shape::Uneven`] にする)
    Two {
        d: f64,
        d2: f64,
        flip: bool,
    },
    /// 行の距離と角度の面取り(距離 1 が `d`、その面と面取りの面の角が `angle` ラジアン)
    Angle {
        d: f64,
        angle: f64,
        flip: bool,
    },
    /// 辺の上の、両側で幅の違う面取り。距離 1 の面の上に `d`、もう片方の面の上に `d·q`。
    /// `b` なら距離 1 は面 B(`faces[1]`)の側
    Uneven {
        d: f64,
        q: f64,
        b: bool,
    },
}

impl Shape {
    /// 大きさ(半径か、距離 1)。
    fn size(self) -> f64 {
        match self {
            Shape::Round(v) | Shape::Flat(v) => v,
            Shape::Two { d, .. } | Shape::Angle { d, .. } | Shape::Uneven { d, .. } => d,
        }
    }

    /// 大きさを `v` にした形(2 つの距離は比を保つ)。
    fn with_size(self, v: f64) -> Shape {
        match self {
            Shape::Round(_) => Shape::Round(v),
            Shape::Flat(_) => Shape::Flat(v),
            Shape::Two { d, d2, flip } => Shape::Two {
                d: v,
                d2: d2 * v / d,
                flip,
            },
            Shape::Angle { angle, flip, .. } => Shape::Angle { d: v, angle, flip },
            Shape::Uneven { q, b, .. } => Shape::Uneven { d: v, q, b },
        }
    }

    fn is_round(self) -> bool {
        matches!(self, Shape::Round(_))
    }

    /// 両側で幅の違う面取りか(カーネルの丸めと角の形は、同じ幅の時だけ使う)。
    fn is_uneven(self) -> bool {
        matches!(
            self,
            Shape::Two { .. } | Shape::Angle { .. } | Shape::Uneven { .. }
        )
    }

    /// 面 A・B の上の幅(丸めは辺から接する所まで)。`cos` は両面の法線の内積。
    fn widths(self, cos: f64) -> (f64, f64) {
        match self {
            Shape::Round(r) => {
                let w = r * ((1.0 - cos) / (1.0 + cos)).sqrt();
                (w, w)
            }
            Shape::Flat(d) => (d, d),
            Shape::Uneven { d, q, b } => {
                if b {
                    (d * q, d)
                } else {
                    (d, d * q)
                }
            }
            // 辺ごとに決める前の形(使わない)
            Shape::Two { d, .. } | Shape::Angle { d, .. } => (d, d),
        }
    }

    /// 辺 `e` の上の形。2 つの距離・距離と角度は、距離 1 を載せる面と幅の比を決める
    /// (`normals` は面 A・B の外向きの法線。円の辺は `None` で、平らな面の側を距離 1 にする)。
    fn on_edge(self, normals: Option<(P3, P3)>) -> Result<Shape, EvalError> {
        let (d, flip) = match self {
            Shape::Two { d, flip, .. } | Shape::Angle { d, flip, .. } => (d, flip),
            _ => return Ok(self),
        };
        // 距離 1 の面: 上を向いた面(同じなら +Y、+X の向き)。円の辺は平らな面(面 A)
        let b = match normals {
            Some((na, nb)) => {
                let key = |n: P3| [n[2], n[1], n[0]];
                let (ka, kb) = (key(na), key(nb));
                let first = (0..3).find(|&i| (ka[i] - kb[i]).abs() > 1e-9);
                first.is_some_and(|i| kb[i] > ka[i])
            }
            None => false,
        } != flip;
        let q = match self {
            Shape::Two { d2, .. } => d2 / d,
            Shape::Angle { angle, .. } => {
                // 辺の中身の側の角 θ(両面の法線の間の角の補角)。距離 1 の点・辺・もう片方の面の点の
                // 三角形で、もう片方の面の上の幅は d·sin(角) / sin(θ + 角)(正弦定理)
                let cos = normals.map_or(0.0, |(na, nb)| k::dot(na, nb));
                let theta = std::f64::consts::PI - cos.clamp(-1.0, 1.0).acos();
                let s = (theta + angle).sin();
                if s < 1e-6 || theta + angle >= std::f64::consts::PI {
                    return Err(EvalError::InvalidData(format!(
                        "角度 {} deg は大きすぎます(この辺では {} deg より小さくしてください)",
                        mm(angle.to_degrees()),
                        mm((std::f64::consts::PI - theta).to_degrees())
                    )));
                }
                angle.sin() / s
            }
            _ => unreachable!("上で返した"),
        };
        Ok(Shape::Uneven { d, q, b })
    }
}

/// 辺に垂直な平面での、辺のまわりの形。辺は原点、面 A の外向きの法線が x 軸。
struct Section {
    plane: Plane,
    length: f64,
    na: [f64; 2],
    nb: [f64; 2],
    convex: bool,
}

fn section(e: &EdgeInfo, na: P3, nb: P3) -> Result<Section, EvalError> {
    let v = k::sub(e.b, e.a);
    let length = k::dot(v, v).sqrt();
    let d = k::scale(v, 1.0 / length);
    let cos = k::dot(na, nb);
    if !(-0.95..=0.995).contains(&cos) {
        return Err(EvalError::InvalidData(format!(
            "面 {} と {} の角度が浅すぎるか鋭すぎます",
            e.faces[0], e.faces[1]
        )));
    }
    let plane = Plane {
        origin: e.a,
        x_axis: na,
        y_axis: k::cross(d, na),
    };
    let nb2 = [k::dot(nb, plane.x_axis), k::dot(nb, plane.y_axis)];
    Ok(Section {
        plane,
        length,
        na: [1.0, 0.0],
        nb: nb2,
        convex: e.convex,
    })
}

fn line(name: &str, a: [f64; 2], b: [f64; 2]) -> ProfileEdge {
    ProfileEdge {
        name: name.into(),
        segment: Segment::Line { a, b },
        reversed: false,
    }
}

fn add2(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] + b[0], a[1] + b[1]]
}

fn scale2(a: [f64; 2], s: f64) -> [f64; 2] {
    [a[0] * s, a[1] * s]
}

/// 角を削り取る柱(へこんだ辺では角を埋める柱)の断面。
fn cutter(c: &Section, shape: Shape, offset: f64, name: &str) -> Profile {
    let (na, nb) = (c.na, c.nb);
    let cos = na[0] * nb[0] + na[1] * nb[1];
    let bis = add2(na, nb);
    let bis_len = (bis[0] * bis[0] + bis[1] * bis[1]).sqrt();
    // 出っ張った辺は角の外側、へこんだ辺は中身の側の点
    let side = if c.convex { 1.0 } else { -1.0 };
    let out = scale2(bis, side * OUTSIDE / bis_len);
    let mut lp = match shape {
        Shape::Round(r) if !c.convex => {
            // 円の中心は空いた側。わずかに面へ寄せて、円が両方の面と交わるようにする
            let center = scale2(bis, (r - offset) / (1.0 + cos));
            let pa = add2(center, scale2(na, -r));
            let pb = add2(center, scale2(nb, -r));
            let sa = (-na[1]).atan2(-na[0]);
            let sb = (-nb[1]).atan2(-nb[0]);
            // 角に近い側の短い弧
            let arc = if k::arc_sweep(sa, sb) < std::f64::consts::PI {
                ProfileEdge {
                    name: name.into(),
                    segment: Segment::Arc {
                        center,
                        radius: r,
                        start: sa,
                        end: sb,
                    },
                    reversed: false,
                }
            } else {
                ProfileEdge {
                    name: name.into(),
                    segment: Segment::Arc {
                        center,
                        radius: r,
                        start: sb,
                        end: sa,
                    },
                    reversed: true,
                }
            };
            ProfileLoop {
                edges: vec![
                    line(&format!("{name}a"), out, pa),
                    arc,
                    line(&format!("{name}b"), pb, out),
                ],
            }
        }
        Shape::Round(r) => {
            let center = scale2(bis, -(r - offset) / (1.0 + cos));
            let pa = add2(center, scale2(na, r));
            let pb = add2(center, scale2(nb, r));
            let ang_b = nb[1].atan2(nb[0]);
            let arc = if ang_b > 0.0 {
                ProfileEdge {
                    name: name.into(),
                    segment: Segment::Arc {
                        center,
                        radius: r,
                        start: 0.0,
                        end: ang_b,
                    },
                    reversed: true,
                }
            } else {
                ProfileEdge {
                    name: name.into(),
                    segment: Segment::Arc {
                        center,
                        radius: r,
                        start: ang_b,
                        end: 0.0,
                    },
                    reversed: false,
                }
            };
            ProfileLoop {
                edges: vec![
                    line(&format!("{name}a"), pa, out),
                    line(&format!("{name}b"), out, pb),
                    arc,
                ],
            }
        }
        Shape::Flat(_) | Shape::Two { .. } | Shape::Angle { .. } | Shape::Uneven { .. } => {
            let (wa, wb) = shape.widths(cos);
            // 面の上を辺から離れる向き(出っ張った辺では相手の面の法線と逆向き、
            // へこんだ辺では同じ向き)
            let inward = |n: [f64; 2], other: [f64; 2]| {
                let w = [-n[1], n[0]];
                if side * (w[0] * other[0] + w[1] * other[1]) < 0.0 {
                    w
                } else {
                    [n[1], -n[0]]
                }
            };
            let pa = scale2(inward(na, nb), wa);
            let pb = scale2(inward(nb, na), wb);
            // 面と交わるよう、面取りの線を両側へ少し延ばす
            let t = 0.05;
            let ea = add2(pa, scale2(add2(pa, scale2(pb, -1.0)), t));
            let eb = add2(pb, scale2(add2(pb, scale2(pa, -1.0)), t));
            ProfileLoop {
                edges: vec![
                    line(&format!("{name}a"), ea, out),
                    line(&format!("{name}b"), out, eb),
                    line(name, eb, ea),
                ],
            }
        }
    };
    if lp.signed_area() < 0.0 {
        lp.reverse();
    }
    Profile {
        plane: c.plane,
        regions: vec![Region {
            outer: lp,
            holes: vec![],
        }],
    }
}

/// 行(辺の選び方と大きさ)ごとに丸める(面取りする)。どの行も同じ種類(丸めか面取り)。
/// 同じ辺が 2 つの行に入っていれば、後の行の大きさにする。
#[allow(clippy::too_many_arguments)]
fn apply(
    ctx: &mut EvalCtx<'_>,
    target: FeatureId,
    rows: &[(EdgeSelection, Shape)],
    rounds: &[RowRound],
    weight: f64,
    fit: bool,
    corner: Corner,
    chain: bool,
    method: Method,
    prefix: &str,
) -> Result<Outputs, EvalError> {
    let Some(&(_, kind)) = rows.first() else {
        return Err(EvalError::InvalidData("辺を選んでください".into()));
    };
    let round_of = |row: usize| rounds.get(row).cloned().unwrap_or_default();
    let special = rounds.iter().any(|r| !r.is_plain());
    if special && method == Method::Cut {
        return Err(EvalError::InvalidData(
            "弦の長さ・途中で変わる半径・曲率(G2)は、カーネルの丸め(新しく作るフィレット)だけで作れます".into(),
        ));
    }
    match (kind.is_round(), corner) {
        (true, Corner::Patch) => {
            return Err(EvalError::InvalidData(
                "フィレットの角は「交わり」か「ローリングボール」です".into(),
            ));
        }
        (false, Corner::Ball) => {
            return Err(EvalError::InvalidData(
                "面取りの角は「交わり」か「三角の面」です".into(),
            ));
        }
        _ => {}
    }
    // 両側で幅の違う面取りは、面の上の幅を保つ柱で削る。
    let uneven = rows.iter().any(|(_, s)| s.is_uneven());
    let method = if uneven { Method::Cut } else { method };
    let body = body_of(ctx, target)?;
    let kernel = ctx.kernel();
    let mut all = kernel.edges(body).map_err(kernel_err)?;
    all.extend(kernel.arc_edges(body).map_err(kernel_err)?);
    // 一定幅で扱えない曲線も列挙し、切れた参照と区別して知らせる。
    if method == Method::Blend || uneven {
        all.extend(kernel.curve_edges(body).map_err(kernel_err)?);
    }
    // 辺ごとの大きさと行(誤りの文で行を示す)
    let mut picked: Vec<(EdgeInfo, Shape, usize)> = Vec::new();
    for (row, (select, shape)) in rows.iter().enumerate() {
        let found = select
            .resolve(&all, chain)
            .map_err(|e| in_row(e, row, rows.len()))?;
        for e in found {
            if shape.is_uneven() && !e.curve.is_empty() {
                return Err(in_row(EvalError::InvalidData(
                    "この曲がった辺には 2 つの距離・距離と角度の面取りを一定の幅で付けられません。等しい距離の面取りを選んでください".into(),
                ), row, rows.len()));
            }
            match picked.iter_mut().find(|p| p.0 == e) {
                Some(p) => (p.1, p.2) = (*shape, row),
                None => picked.push((e, *shape, row)),
            }
        }
    }
    // 円の辺を先に処理する(まっすぐな辺を先に丸めた後だと、円の柱のブーリアンが壊れやすい)
    picked.sort_by_key(|p| p.0.arc.is_none());
    let edges: Vec<EdgeInfo> = picked.iter().map(|p| p.0.clone()).collect();
    let faces = kernel.faces(body).map_err(kernel_err)?;
    // 面の網が取れなければ、収まるかは調べずに進める(従来どおり)
    let mesh = kernel.mesh(body, OUTLINE_TOL).ok();
    // 辺ごとの形(2 つの距離・距離と角度は、辺の両側の面の向きで決める)
    let planar = |name: &str| {
        faces
            .iter()
            .find(|f| f.name == name)
            .and_then(|f| f.planar)
            .map(|p| p.normal)
    };
    let resolved: Vec<Shape> = picked
        .iter()
        .map(|(e, shape, row)| {
            let normals = match (e.arc, planar(&e.faces[0]), planar(&e.faces[1])) {
                (None, Some(na), Some(nb)) => Some((na, nb)),
                _ => None,
            };
            // 弦の長さは、両側の面の法線の間の角 α から半径 c / (2 sin(α/2)) に直す(平面でない面は直角)
            let shape = match (round_of(*row).law, *shape) {
                (RowLaw::Chord, Shape::Round(c)) => {
                    let cos = normals.map_or(0.0, |(na, nb)| k::dot(na, nb));
                    let half = cos.clamp(-1.0, 1.0).acos() / 2.0;
                    if half.sin() < 1e-6 {
                        return Err(in_row(
                            EvalError::InvalidData(format!(
                                "面 {} と {} は平らにつながっているので、弦の長さで丸められません",
                                e.faces[0], e.faces[1]
                            )),
                            *row,
                            rows.len(),
                        ));
                    }
                    Shape::Round(c / (2.0 * half.sin()))
                }
                (_, s) => s,
            };
            shape
                .on_edge(normals)
                .map_err(|err| in_row(err, *row, rows.len()))
        })
        .collect::<Result<_, _>>()?;
    let mut limits = mesh
        .as_ref()
        .map(|m| limits(m, &faces, &edges, &resolved))
        .unwrap_or_else(|| vec![None; edges.len()]);
    // 円の内側の面(ボスの上面など)では、半径は円の半径に収まる
    for (l, e) in limits.iter_mut().zip(&edges) {
        if let Some(arc) = e.arc.filter(|a| a.inside) {
            *l = Some(l.map_or(arc.radius, |v| v.min(arc.radius)));
        }
    }
    let mut warnings = Vec::new();
    let normal = |name: &str| -> Result<P3, EvalError> {
        faces
            .iter()
            .find(|f| f.name == name)
            .and_then(|f| f.planar)
            .map(|p| p.normal)
            .ok_or_else(|| EvalError::InvalidData(format!("面 {name} は平面ではありません")))
    };
    let tag = tag(ctx);
    // 辺ごとの大きさ(収まらない辺は、`fit` なら下げる)
    let mut shapes: Vec<Shape> = Vec::with_capacity(edges.len());
    for (i, e) in edges.iter().enumerate() {
        let (shape, row) = (resolved[i], picked[i].2);
        // 行が 2 つ以上なら、誤りと注意の文の頭に行を書く
        let head = if rows.len() > 1 {
            format!("{}: ", kmb_core::row_name(row))
        } else {
            String::new()
        };
        let shape = match (shape.size(), limits[i]) {
            (v, Some(max)) if v > max + 1e-9 => {
                let what = if shape.is_round() { "半径" } else { "距離" };
                let usable = fit_value(max);
                if !fit || usable <= 0.0 {
                    let hint = if usable > 0.0 {
                        format!(
                            "収まる最大は {} mm です(「収まらないエッジだけ下げる」を使うと、この辺だけ下げます)",
                            mm(usable)
                        )
                    } else {
                        "面の幅が狭すぎて収まりません".into()
                    };
                    return Err(EvalError::InvalidData(format!(
                        "{head}面 {} と {} の間の辺に、{what} {} mm は大きすぎます。{hint}",
                        e.faces[0],
                        e.faces[1],
                        mm(v)
                    )));
                }
                warnings.push(format!(
                    "{head}面 {} と {} の間の辺の{what}を {} mm から {} mm に下げました",
                    e.faces[0],
                    e.faces[1],
                    mm(v),
                    mm(usable)
                ));
                shape.with_size(usable)
            }
            _ => shape,
        };
        shapes.push(shape);
    }
    if method == Method::Blend {
        // 辺ごとの半径の決め方。収まらずに下げた辺は、決め方の値も同じ比で下げる
        let laws: Vec<(k::BlendLaw, f64, bool)> = picked
            .iter()
            .enumerate()
            .map(|(i, (e, shape, row))| {
                let rr = round_of(*row);
                let scale = shapes[i].size() / resolved[i].size();
                let (law, size) = match rr.law {
                    RowLaw::Constant => (k::BlendLaw::Constant, shapes[i].size()),
                    RowLaw::Chord => (k::BlendLaw::Chord, shape.size() * scale),
                    RowLaw::Variable(pts) => (
                        k::BlendLaw::Variable {
                            start: edge_start(e),
                            radii: pts.iter().map(|&(at, r)| (at, r * scale)).collect(),
                        },
                        shapes[i].size(),
                    ),
                };
                (law, size, rr.g2)
            })
            .collect();
        match blend(kernel, body, &edges, &shapes, &laws, weight, &faces, &tag) {
            Ok(h) => {
                let mut out: Outputs = [("body".to_string(), Value::Shape(h))].into();
                kmb_core::add_warnings(&mut out, warnings);
                return Ok(out);
            }
            // できないカーネル(truck)は、柱で削る形で作る(一定の半径だけ)
            Err(kmb_core::KernelError::Unsupported(_)) if special => {
                return Err(EvalError::InvalidData(
                    "このカーネルは辺の丸めができないので、弦の長さ・途中で変わる半径・曲率(G2)は作れません".into(),
                ));
            }
            Err(kmb_core::KernelError::Unsupported(_)) => {}
            Err(err) => {
                return Err(EvalError::Kernel(format!("辺を丸められません: {err}")));
            }
        }
    }
    // 辺の端が、同じ面の上の別の辺とへこんだ角や接する所で出会うなら、延ばしてから切る
    let clips: Vec<[Option<arc::Clip>; 2]> = edges
        .iter()
        .enumerate()
        .map(|(i, e)| arc::clips(&faces, &all, e, shapes[i].size()))
        .collect();
    let rank = stagger_ranks(&edges, &all, &clips);
    let mut cur = body;
    // 辺ごとに実際に使った大きさ(収まらずに下げた値、ずらした分を含む)
    let mut used: Vec<(f64, f64)> = Vec::with_capacity(edges.len());
    for (i, e) in edges.iter().enumerate() {
        let shape = shapes[i];
        let size = shape.size();
        let op = if e.convex {
            BooleanOp::Subtract
        } else {
            BooleanOp::Union
        };
        let clips = &clips[i];
        if e.arc.is_some() {
            let ext = [0, 1].map(|j| {
                if clips[j].is_some() {
                    arc::clip_extension(size)
                } else if e.convex {
                    size.min(EXTEND)
                } else {
                    0.0
                }
            });
            let mut last = None;
            for offset in OFFSETS {
                let stagger = offset * rank[i] as f64;
                let shape = shape.with_size(size + stagger);
                let name = format!("{prefix}{}", i + 1);
                let tool =
                    match arc::arc_tool(kernel, e, shape, offset, &name, ext, clips, size, &tag) {
                        Ok(t) => t,
                        Err(err) => {
                            last = Some(err);
                            continue;
                        }
                    };
                match checked_boolean(kernel, op, cur, tool) {
                    Ok(h) => {
                        last = None;
                        cur = h;
                        used.push((size, shape.size()));
                        break;
                    }
                    Err(err) => last = Some(err),
                }
            }
            if let Some(err) = last {
                return Err(EvalError::Kernel(format!(
                    "面 {} と {} の間の円の辺を処理できません: {err}",
                    e.faces[0], e.faces[1]
                )));
            }
            continue;
        }
        let c = section(e, normal(&e.faces[0])?, normal(&e.faces[1])?)?;
        let d = k::scale(k::sub(e.b, e.a), 1.0 / c.length);
        let (na, nb) = (normal(&e.faces[0])?, normal(&e.faces[1])?);
        // 柱が面に乗る幅(辺から面の上を測る)。丸めは r·tan(φ/2)(φ は両面の法線の間の角)
        let width = {
            let (wa, wb) = shape.widths(k::dot(na, nb));
            wa.max(wb)
        };
        let ends = |p: P3, dir: P3, clip: Option<&arc::Clip>| {
            if clip.is_some() {
                return arc::clip_extension(size);
            }
            let r = reach(&faces, e, p, dir, size);
            match &mesh {
                Some(m) if e.convex => {
                    run_out(m, p, dir, na, nb, width, size).map_or(r, |t| t.max(r))
                }
                _ => r,
            }
        };
        let (start, end) = (
            -ends(e.a, k::scale(d, -1.0), clips[0].as_ref()),
            c.length + ends(e.b, d, clips[1].as_ref()),
        );
        // へこんだ辺は、足元の面に穴やポケットがあれば、その上を埋めない
        let mut shadows = Vec::new();
        if !e.convex {
            for (n, other) in [(na, nb), (nb, na)] {
                if let Some(h) = shadow(kernel, cur, e, n, other, width, start, end, &tag) {
                    shadows.push(h);
                }
            }
        }
        let mut last = None;
        for offset in OFFSETS {
            // 辺ごとに大きさをわずかに変え、隣の辺の柱と同じ点で交わらないようにする
            let stagger = offset * rank[i] as f64;
            let shape = shape.with_size(size + stagger);
            let profile = cutter(&c, shape, offset, &format!("{prefix}{}", i + 1));
            let mut tool = kernel
                .extrude(&profile, start, end, &tag)
                .map_err(kernel_err)?;
            for h in &shadows {
                // 除けなければ、柱のまま埋める
                if let Ok(t) = kernel.boolean(BooleanOp::Subtract, tool, *h) {
                    tool = t;
                }
            }
            for clip in clips.iter().flatten() {
                if let Ok(t) = arc::apply_clip(kernel, tool, clip, size, &tag) {
                    tool = t;
                }
            }
            match checked_boolean(kernel, op, cur, tool) {
                Ok(h) => {
                    last = None;
                    cur = h;
                    used.push((size, shape.size()));
                    break;
                }
                Err(err) => last = Some(err),
            }
        }
        if let Some(err) = last {
            return Err(EvalError::Kernel(format!(
                "面 {} と {} の間の辺を処理できません: {err}",
                e.faces[0], e.faces[1]
            )));
        }
    }
    if corner != Corner::Miter && uneven {
        warnings.push(
            "幅の違う面取り(2 つの距離・距離と角度)では、3 本の辺が集まる角は交わりの形にしました"
                .into(),
        );
    } else if corner != Corner::Miter {
        for v in corners(&edges, &faces) {
            let sizes: Vec<f64> = v.edges.iter().map(|&i| used[i].0).collect();
            if sizes.iter().any(|s| (s - sizes[0]).abs() > 1e-9) {
                warnings.push(format!(
                    "{} の角は、3 本の辺の大きさがそろっていないので交わりの形にしました",
                    mm3(v.at)
                ));
                continue;
            }
            let actual: Vec<f64> = v.edges.iter().map(|&i| used[i].1).collect();
            let done = match corner {
                Corner::Ball => ball_corner(kernel, cur, &v, sizes[0], &actual, &tag),
                _ => patch_corner(kernel, cur, &v, &actual, &tag),
            };
            match done {
                Ok(h) => cur = h,
                Err(err) => warnings.push(format!(
                    "{} の角を作れなかったので、交わりの形のままにしました({err})",
                    mm3(v.at)
                )),
            }
        }
    }
    let mut out: Outputs = [("body".to_string(), Value::Shape(cur))].into();
    kmb_core::add_warnings(&mut out, warnings);
    Ok(out)
}

/// 辺ごとの大きさのずらしの番号。切り口で断面を重ねる辺どうし(まっすぐな辺どうしのへこんだ角)は
/// 同じ番号にする。ずらしが違うと、切り口で 2 本の柱の断面が食い違い、その差の幅の細い面が角に残る。
fn stagger_ranks(
    edges: &[EdgeInfo],
    all: &[EdgeInfo],
    clips: &[[Option<arc::Clip>; 2]],
) -> Vec<usize> {
    let mut rank: Vec<usize> = (0..edges.len()).collect();
    fn root(rank: &mut [usize], i: usize) -> usize {
        let mut r = i;
        while rank[r] != r {
            r = rank[r];
        }
        rank[i] = r;
        r
    }
    for (i, ends) in clips.iter().enumerate() {
        for clip in ends.iter().flatten() {
            let Some(j) = clip
                .with
                .and_then(|w| edges.iter().position(|g| *g == all[w]))
            else {
                continue;
            };
            let (a, b) = (root(&mut rank, i), root(&mut rank, j));
            rank[a.max(b)] = a.min(b);
        }
    }
    (0..edges.len()).map(|i| root(&mut rank, i)).collect()
}

/// カーネルの丸めで、辺 `edges` をそれぞれの大きさ `shapes` で丸める(面取りする)。
/// 面取りの距離は面の上の幅なので、両側の面に接する円の半径 d / tan(φ/2)(φ は両面の法線の
/// 間の角。直角なら d)にして渡す。平面でない面の辺は直角とみなす。
#[allow(clippy::too_many_arguments)]
fn blend(
    kernel: &dyn kmb_core::Kernel,
    body: kmb_core::ShapeHandle,
    edges: &[EdgeInfo],
    shapes: &[Shape],
    laws: &[(k::BlendLaw, f64, bool)],
    weight: f64,
    faces: &[FaceInfo],
    tag: &str,
) -> Result<kmb_core::ShapeHandle, kmb_core::KernelError> {
    let normal = |name: &str| {
        faces
            .iter()
            .find(|f| f.name == name)
            .and_then(|f| f.planar)
            .map(|p| p.normal)
    };
    let blends: Vec<kmb_core::EdgeBlend> = edges
        .iter()
        .zip(shapes)
        .zip(laws)
        .map(|((e, shape), (law, law_size, g2))| {
            let line = e.polyline(8);
            let point = if line.len() == 2 {
                k::scale(k::add(line[0], line[1]), 0.5)
            } else {
                line[line.len() / 2]
            };
            let (size, flat) = match *shape {
                Shape::Round(r) => (r, false),
                // 幅の違う面取りはここへ来ない(柱で削る)。来ても同じ幅にする
                Shape::Flat(d)
                | Shape::Two { d, .. }
                | Shape::Angle { d, .. }
                | Shape::Uneven { d, .. } => {
                    let per = match (normal(&e.faces[0]), normal(&e.faces[1])) {
                        (Some(na), Some(nb)) if e.arc.is_none() => {
                            let cos = k::dot(na, nb);
                            ((1.0 + cos) / (1.0 - cos)).sqrt()
                        }
                        _ => 1.0,
                    };
                    (d * per, true)
                }
            };
            let mut b = kmb_core::EdgeBlend::new(e.faces.clone(), point, size, flat);
            if !flat {
                b.law = law.clone();
                if !law.is_constant() {
                    b.size = *law_size;
                }
                b.g2 = *g2;
                b.weight = weight;
            }
            b
        })
        .collect();
    kernel.blend_edges(body, &blends, tag)
}

/// 行 `row`(全部で `n` 行)の誤りの文に、行を書き足す(1 行だけなら書き足さない)。
fn in_row(e: EvalError, row: usize, n: usize) -> EvalError {
    match e {
        EvalError::InvalidData(m) if n > 1 => {
            EvalError::InvalidData(format!("{}: {m}", kmb_core::row_name(row)))
        }
        e => e,
    }
}

/// 点を短く書く(例: 「(30, 30, 15)」)。
fn mm3(p: P3) -> String {
    format!("({}, {}, {})", mm(p[0]), mm(p[1]), mm(p[2]))
}

/// 長さを短く書く(小数 3 桁まで、末尾の 0 を落とす)。
fn mm(v: f64) -> String {
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn positive(v: f64, what: &str) -> Result<f64, EvalError> {
    if v <= 0.0 {
        return Err(EvalError::InvalidData(format!(
            "{what}は 0 より大きくしてください"
        )));
    }
    Ok(v)
}

/// フィレットの 1 行の形(収まりを調べる大きさ)と、半径の決め方。
fn fillet_row(ctx: &EvalCtx<'_>, row: &FilletRow) -> Result<(Shape, RowRound), EvalError> {
    let what = match row.kind {
        RadiusKind::Chord => "弦の長さ",
        RadiusKind::Variable => "始めの半径",
        RadiusKind::Constant => "半径",
    };
    let r = length(ctx, &row.radius, what).and_then(|r| positive(r, what))?;
    let g2 = row.continuity == Continuity::G2;
    let (shape, law) = match row.kind {
        RadiusKind::Constant => (Shape::Round(r), RowLaw::Constant),
        RadiusKind::Chord => (Shape::Round(r), RowLaw::Chord),
        RadiusKind::Variable => {
            let end = match row.end_radius.as_deref().filter(|s| !s.trim().is_empty()) {
                Some(src) => {
                    length(ctx, src, "終わりの半径").and_then(|v| positive(v, "終わりの半径"))?
                }
                None => r,
            };
            let mut pts = vec![(0.0, r)];
            let mut mids = Vec::new();
            for (at, src) in mid_points(row.mid_radii.as_deref().unwrap_or(""))? {
                let v = length(ctx, &src, "途中の半径").and_then(|v| positive(v, "途中の半径"))?;
                mids.push((at, v));
            }
            mids.sort_by(|a, b| a.0.total_cmp(&b.0));
            if mids.windows(2).any(|w| w[1].0 - w[0].0 < 1e-6) {
                return Err(EvalError::InvalidData(
                    "途中の半径に、同じ位置が 2 つあります".into(),
                ));
            }
            pts.extend(mids);
            pts.push((1.0, end));
            let max = pts.iter().map(|p| p.1).fold(0.0, f64::max);
            (Shape::Round(max), RowLaw::Variable(pts))
        }
    };
    Ok((shape, RowRound { law, g2 }))
}

impl Feature for Fillet {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = vec![output(self.target, "body")];
        v.extend(expr_refs(self)?);
        // 途中の点の半径の式(1 つの欄に並べて書く)
        for row in &self.rows {
            for (_, src) in mid_points(row.mid_radii.as_deref().unwrap_or(""))? {
                for i in kmb_core::expr_inputs(&src)? {
                    if !v.contains(&i) {
                        v.push(i);
                    }
                }
            }
        }
        Ok(v)
    }

    fn pass_through(&self) -> Vec<(String, InputRef)> {
        vec![("body".into(), output(self.target, "body"))]
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let n = self.rows.len();
        let rows = self
            .rows
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let (shape, round) = fillet_row(ctx, row).map_err(|e| in_row(e, i, n))?;
                Ok(((row_selection(row)?, shape), round))
            })
            .collect::<Result<Vec<_>, EvalError>>()?;
        let (rows, rounds): (Vec<_>, Vec<_>) = rows.into_iter().unzip();
        let weight = self.weight.unwrap_or(1.0);
        if weight.is_nan() || weight <= 0.0 {
            return Err(EvalError::InvalidData(
                "接線ウェイトは 0 より大きくしてください".into(),
            ));
        }
        apply(
            ctx,
            self.target,
            &rows,
            &rounds,
            weight,
            self.fit,
            self.corner,
            self.chain,
            self.method,
            "r",
        )
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        let mut c = self.clone();
        for e in c.exprs_mut() {
            *e = kmb_core::rename_reference(e, old, new)?;
        }
        let mut changed = c != *self;
        for row in &mut c.rows {
            let Some(src) = row.mid_radii.clone() else {
                continue;
            };
            let mut parts = Vec::new();
            for (at, e) in mid_points(&src)? {
                parts.push(format!(
                    "{at} = {}",
                    kmb_core::rename_reference(&e, old, new)?
                ));
            }
            let now = parts.join("; ");
            if mid_points(&now)? != mid_points(&src)? {
                row.mid_radii = Some(now);
                changed = true;
            }
        }
        Ok(changed.then(|| serde_json::to_value(&c).expect("機能のデータは JSON にできる")))
    }
}

impl Chamfer {
    /// 行の面取りの形(種類と、行の距離・角度)。
    fn row_shape(&self, ctx: &EvalCtx<'_>, row: &ChamferRow) -> Result<Shape, EvalError> {
        let first = if self.kind.is_equal() {
            "距離"
        } else {
            "距離 1"
        };
        let d = length(ctx, &row.distance, first).and_then(|d| positive(d, first))?;
        fn given(v: &Option<String>) -> Option<&str> {
            v.as_deref().filter(|s| !s.trim().is_empty())
        }
        Ok(match self.kind {
            ChamferKind::Equal => Shape::Flat(d),
            ChamferKind::TwoDistances => {
                let d2 = match given(&row.distance2) {
                    Some(src) => length(ctx, src, "距離 2").and_then(|v| positive(v, "距離 2"))?,
                    None => d,
                };
                Shape::Two {
                    d,
                    d2,
                    flip: row.flip,
                }
            }
            ChamferKind::DistanceAngle => {
                let a = match given(&row.angle) {
                    Some(src) => angle(ctx, src, "角度")?,
                    None => std::f64::consts::FRAC_PI_4,
                };
                if a <= 0.0 || a >= std::f64::consts::PI {
                    return Err(EvalError::InvalidData(
                        "角度は 0 deg より大きく 180 deg より小さくしてください".into(),
                    ));
                }
                Shape::Angle {
                    d,
                    angle: a,
                    flip: row.flip,
                }
            }
        })
    }
}

impl Feature for Chamfer {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = vec![output(self.target, "body")];
        v.extend(expr_refs(self)?);
        Ok(v)
    }

    fn pass_through(&self) -> Vec<(String, InputRef)> {
        vec![("body".into(), output(self.target, "body"))]
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let n = self.rows.len();
        let rows = self
            .rows
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let shape = self.row_shape(ctx, row).map_err(|e| in_row(e, i, n))?;
                Ok((row_selection(row)?, shape))
            })
            .collect::<Result<Vec<_>, EvalError>>()?;
        apply(
            ctx,
            self.target,
            &rows,
            &[],
            1.0,
            self.fit,
            self.corner,
            self.chain,
            self.method,
            "c",
        )
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        rename(self, old, new)
    }
}

/// この crate の名乗り(登録する種類と操作)。組み立て(kmb-bundle)が 1 回だけ並べる。
pub fn module() -> kmb_core::Module {
    kmb_core::Module::new("kmb-cad-edge", register).with_extension(EdgeCommands)
}
