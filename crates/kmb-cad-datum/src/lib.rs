//! 作業平面(`cad.datum_plane@1`)・作業軸(`cad.datum_axis@1`)・作業点(`cad.datum_point@1`)。
//! 作業平面は面・平面から浮かせる、軸のまわりに傾ける、2 つの平らな面の真ん中、3 点を通る、の 4 つの
//! 作り方を 1 つの機能で持ち、出力 [`DATUM_OUTPUT`] に平面の形([`DatumPlane`])を出す。スケッチは
//! この平面の上に描ける(`SketchPlane::Datum`)。作業軸([`axis`])は回転・パターン・移動の軸に、
//! 作業点([`point`])は作業平面の 3 点と作業軸の 2 点に使える。
//!
//! 元の面・辺・角は名前で指すので、寸法を変えても付いて来る。元が消えたら別の面に付け替えず、
//! この機能を理由つきの誤りにする(その上のスケッチも誤りになる)。X・Y の向きは元の平面のまま
//! (浮かせる距離が負でも入れ替えない)。
pub mod axis;
mod commands;
pub mod point;

use kmb_cad::*;
use kmb_core::kernel::{self as k, P3, Plane};
use kmb_core::{
    EvalCtx, EvalError, Feature, FeatureId, Field, InputRef, Outputs, Payload, Registry,
};
use kmb_interchange::{DATUM_OUTPUT, DatumPlane};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub use axis::{AxisMethod, DATUM_AXIS, DatumAxisFeature};
pub use commands::{ADD_DATUM_AXIS, ADD_DATUM_PLANE, ADD_DATUM_POINT, DatumCommands};
pub use point::{DATUM_POINT, DatumPointFeature};

pub const DATUM_PLANE: &str = "cad.datum_plane@1";

/// 作業平面の作り方。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    /// 面・平面から `distance` だけ浮かせる
    #[default]
    Offset,
    /// 面・平面を、それに沿う軸 `axis` のまわりに `angle` だけ傾ける
    Angle,
    /// 2 つの平行な平らな面(`base` と `other`)の真ん中
    Midplane,
    /// 3 点 `points` を通る(1 点目が原点、1 点目から 2 点目の向きが X)
    ThreePoints,
}

const METHODS: &[(&str, &str)] = &[
    ("offset", "オフセット平面"),
    ("angle", "傾けた平面"),
    ("midplane", "中間平面"),
    ("three_points", "3 点を通る平面"),
];

/// 元の平面。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Base {
    Xy,
    Yz,
    Zx,
    /// 機能 `feature` のボディの平らな面 `face`
    Face {
        feature: FeatureId,
        face: String,
    },
    /// 別の作業平面
    Datum {
        feature: FeatureId,
    },
}

pub(crate) const BASES: &[(&str, &str)] = &[
    ("xy", "原点の XY 平面"),
    ("yz", "原点の YZ 平面"),
    ("zx", "原点の ZX 平面"),
    ("face", "平らな面"),
    ("datum", "作業平面"),
];

/// 傾ける軸。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Axis {
    X,
    Y,
    Z,
    /// ボディのまっすぐな辺(両側の面の名前)
    Edge {
        feature: FeatureId,
        faces: [String; 2],
    },
}

/// 点(作業平面の 3 点・作業軸の 2 点・作業点)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Point {
    /// ボディの角(頂点の名前。[`kmb_core::vertices`])
    Vertex { feature: FeatureId, vertex: String },
    /// 座標(mm)
    At { at: P3 },
    /// ボディの円の辺(両側の面の名前)の中心
    Center {
        feature: FeatureId,
        faces: [String; 2],
    },
    /// 作業点
    Datum { feature: FeatureId },
}

impl Point {
    fn input(&self) -> Option<InputRef> {
        match self {
            Point::Vertex { feature, .. } | Point::Center { feature, .. } => {
                Some(output(*feature, "body"))
            }
            Point::Datum { feature } => Some(output(*feature, DATUM_OUTPUT)),
            Point::At { .. } => None,
        }
    }

    /// 世界座標の位置。元が消えたら別の物に付け替えず誤りにする。
    pub(crate) fn resolve(&self, ctx: &EvalCtx<'_>) -> Result<P3, EvalError> {
        match self {
            Point::At { at } => Ok(*at),
            Point::Vertex { feature, vertex } => {
                let body = body_of(ctx, *feature)?;
                kmb_core::vertices(ctx.kernel(), body)
                    .into_iter()
                    .find(|v| &v.name == vertex)
                    .map(|v| v.at)
                    .ok_or_else(|| {
                        EvalError::InvalidData(format!(
                            "点にした角 {vertex} が見つかりません(寸法を変えて角が無くなりました)。点を選び直してください"
                        ))
                    })
            }
            Point::Center { feature, faces } => {
                let body = body_of(ctx, *feature)?;
                let same = |f: &[String; 2]| f == faces || (f[0] == faces[1] && f[1] == faces[0]);
                ctx.kernel()
                    .arc_edges(body)
                    .map_err(kernel_err)?
                    .into_iter()
                    .find(|e| same(&e.faces))
                    .and_then(|e| e.arc)
                    .map(|a| a.center)
                    .ok_or_else(|| {
                        EvalError::InvalidData(format!(
                            "点にした円の辺(面 {} の境目)が見つかりません。点を選び直してください",
                            faces.join("・")
                        ))
                    })
            }
            Point::Datum { feature } => kmb_interchange::datum_point(ctx, *feature),
        }
    }
}

/// 点の入力を `add` に足す(同じ物は 1 回)。
pub(crate) fn point_inputs(points: &[Point], v: &mut Vec<InputRef>) {
    for i in points.iter().filter_map(Point::input) {
        if !v.contains(&i) {
            v.push(i);
        }
    }
}

/// 作業平面。例: `{"method": "offset", "base": {"kind": "face", "feature": 2, "face": "f2:end"},
/// "distance": "10 mm"}`。作り方で使わない欄は残っていても読まない(作り方を切り替えても前の値を
/// 消さない)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatumPlaneFeature {
    #[serde(default)]
    pub method: Method,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<Base>,
    /// 浮かせる距離(負なら元の平面の裏の側)
    #[serde(default = "zero_mm")]
    pub distance: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axis: Option<Axis>,
    #[serde(default = "zero_deg")]
    pub angle: String,
    /// 中間平面のもう 1 つの面
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub other: Option<Base>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<Point>,
}

fn zero_mm() -> String {
    "0 mm".into()
}

fn zero_deg() -> String {
    "0 deg".into()
}

impl Default for DatumPlaneFeature {
    fn default() -> Self {
        DatumPlaneFeature {
            method: Method::Offset,
            base: None,
            distance: zero_mm(),
            axis: None,
            angle: zero_deg(),
            other: None,
            points: Vec::new(),
        }
    }
}

impl Exprs for DatumPlaneFeature {
    fn exprs_mut(&mut self) -> Vec<&mut String> {
        // 使わない作り方の式も、パラメータの名前の変更に付いて行く(作り方を戻した時に切れない)
        vec![&mut self.distance, &mut self.angle]
    }
}

pub(crate) fn need<T>(v: &Option<T>, what: &str) -> Result<T, EvalError>
where
    T: Clone,
{
    v.clone()
        .ok_or_else(|| EvalError::InvalidData(format!("{what}をまだ選んでいません")))
}

impl Base {
    pub(crate) fn input(&self) -> Option<InputRef> {
        match self {
            Base::Face { feature, .. } => Some(output(*feature, "body")),
            Base::Datum { feature } => Some(output(*feature, DATUM_OUTPUT)),
            _ => None,
        }
    }

    /// 平面と、画面に描く四角(平面の座標の最小・最大)の目安。
    pub(crate) fn resolve(&self, ctx: &EvalCtx<'_>) -> Result<(Plane, [[f64; 2]; 2]), EvalError> {
        let origin_rect = [[0.0, 0.0], [ORIGIN_SIZE, ORIGIN_SIZE]];
        Ok(match self {
            Base::Xy => (Plane::XY, origin_rect),
            Base::Yz => (Plane::YZ, origin_rect),
            Base::Zx => (Plane::ZX, origin_rect),
            Base::Face { feature, face } => {
                let body = body_of(ctx, *feature)?;
                let p = ctx
                    .kernel()
                    .faces(body)
                    .map_err(kernel_err)?
                    .into_iter()
                    .find(|f| &f.name == face)
                    .ok_or_else(|| {
                        EvalError::InvalidData(format!(
                            "作業平面の元の面 {face} が見つかりません(寸法を変えて面が消えたか分かれました)。元の面を選び直してください"
                        ))
                    })?
                    .planar
                    .ok_or_else(|| {
                        EvalError::InvalidData(format!("作業平面の元の面 {face} は平らではありません"))
                    })?;
                // 面の上のスケッチと同じ決め方(法線から X・Y を決める)
                let plane = Plane::from_point_normal(p.point, p.normal);
                let (lo, hi) = ctx.kernel().bounds(body).map_err(kernel_err)?;
                (plane, rect_of(&plane, &box_corners(lo, hi)))
            }
            Base::Datum { feature } => {
                let d = DatumPlane::from_value(ctx.output(*feature, DATUM_OUTPUT)?).ok_or_else(
                    || {
                        EvalError::InvalidData(format!(
                            "機能 {} は作業平面ではありません",
                            feature.0
                        ))
                    },
                )?;
                (d.plane, d.rect)
            }
        })
    }
}

/// 原点の平面から作った作業平面の四角の一辺(mm)。
const ORIGIN_SIZE: f64 = 50.0;

fn box_corners(lo: P3, hi: P3) -> Vec<P3> {
    (0..8)
        .map(|c| {
            [
                if c & 1 == 0 { lo[0] } else { hi[0] },
                if c & 2 == 0 { lo[1] } else { hi[1] },
                if c & 4 == 0 { lo[2] } else { hi[2] },
            ]
        })
        .collect()
}

/// 点を平面に写した範囲を、一回り(1 割)広げた四角。
fn rect_of(plane: &Plane, pts: &[P3]) -> [[f64; 2]; 2] {
    let mut r = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];
    for p in pts {
        let d = k::sub(*p, plane.origin);
        let uv = [k::dot(d, plane.x_axis), k::dot(d, plane.y_axis)];
        for i in 0..2 {
            r[0][i] = r[0][i].min(uv[i]);
            r[1][i] = r[1][i].max(uv[i]);
        }
    }
    let pad = ((r[1][0] - r[0][0]).max(r[1][1] - r[0][1]) * 0.1).max(2.0);
    [
        [r[0][0] - pad, r[0][1] - pad],
        [r[1][0] + pad, r[1][1] + pad],
    ]
}

/// `p` を、`at` を通る向き `dir`(長さ 1)の軸のまわりに `t` rad 回す(右ねじ)。
fn rotate(p: P3, at: P3, dir: P3, t: f64) -> P3 {
    k::add(at, rotate_vec(k::sub(p, at), dir, t))
}

fn rotate_vec(v: P3, dir: P3, t: f64) -> P3 {
    let (s, c) = t.sin_cos();
    k::add(
        k::add(k::scale(v, c), k::scale(k::cross(dir, v), s)),
        k::scale(dir, k::dot(dir, v) * (1.0 - c)),
    )
}

impl DatumPlaneFeature {
    fn axis_line(&self, ctx: &EvalCtx<'_>) -> Result<(P3, P3), EvalError> {
        Ok(match need(&self.axis, "傾ける軸")? {
            Axis::X => ([0.0; 3], [1.0, 0.0, 0.0]),
            Axis::Y => ([0.0; 3], [0.0, 1.0, 0.0]),
            Axis::Z => ([0.0; 3], [0.0, 0.0, 1.0]),
            Axis::Edge { feature, faces } => {
                let body = body_of(ctx, feature)?;
                let e = ctx
                    .kernel()
                    .edges(body)
                    .map_err(kernel_err)?
                    .into_iter()
                    .find(|e| e.arc.is_none() && faces.iter().all(|f| e.faces.contains(f)))
                    .ok_or_else(|| {
                        EvalError::InvalidData(format!(
                            "傾ける軸の辺(面 {} の境目)が見つかりません。軸を選び直してください",
                            faces.join("・")
                        ))
                    })?;
                (e.a, k::normalize(k::sub(e.b, e.a)))
            }
        })
    }

    /// 作業平面と、画面に描く四角。
    pub fn plane(&self, ctx: &EvalCtx<'_>) -> Result<DatumPlane, EvalError> {
        let (plane, rect) = match self.method {
            Method::Offset => {
                let (p, rect) = need(&self.base, "元の平面")?.resolve(ctx)?;
                let d = length(ctx, &self.distance, "浮かせる距離")?;
                let moved = Plane {
                    origin: k::add(p.origin, k::scale(p.normal(), d)),
                    ..p
                };
                (moved, rect)
            }
            Method::Angle => {
                let (p, rect) = need(&self.base, "元の平面")?.resolve(ctx)?;
                let (at, dir) = self.axis_line(ctx)?;
                if k::dot(dir, p.normal()).abs() > 1e-6 {
                    return Err(EvalError::InvalidData(
                        "傾ける軸が元の平面に沿っていません。平面と平行な軸を選んでください".into(),
                    ));
                }
                let t = angle(ctx, &self.angle, "傾ける角度")?;
                let turned = Plane {
                    origin: rotate(p.origin, at, dir, t),
                    x_axis: rotate_vec(p.x_axis, dir, t),
                    y_axis: rotate_vec(p.y_axis, dir, t),
                };
                // 四角は元の四角の角を回した物から取り直す
                let corners: Vec<P3> = [
                    rect[0],
                    [rect[1][0], rect[0][1]],
                    rect[1],
                    [rect[0][0], rect[1][1]],
                ]
                .iter()
                .map(|uv| rotate(p.to_world(*uv), at, dir, t))
                .collect();
                (turned, rect_of(&turned, &corners))
            }
            Method::Midplane => {
                let (a, ra) = need(&self.base, "1 つ目の面")?.resolve(ctx)?;
                let (b, rb) = need(&self.other, "2 つ目の面")?.resolve(ctx)?;
                let (na, nb) = (a.normal(), b.normal());
                if k::dot(k::cross(na, nb), k::cross(na, nb)) > 1e-12 {
                    return Err(EvalError::InvalidData(
                        "中間平面の 2 つの面が平行ではありません。平行な 2 つの面を選んでください"
                            .into(),
                    ));
                }
                let d = k::dot(k::sub(b.origin, a.origin), na);
                let mid = Plane {
                    origin: k::add(a.origin, k::scale(na, d / 2.0)),
                    ..a
                };
                let corners: Vec<P3> = [ra[0], ra[1]]
                    .iter()
                    .map(|uv| a.to_world(*uv))
                    .chain([rb[0], rb[1]].iter().map(|uv| b.to_world(*uv)))
                    .collect();
                (mid, rect_of(&mid, &corners))
            }
            Method::ThreePoints => {
                if self.points.len() != 3 {
                    return Err(EvalError::InvalidData(format!(
                        "3 点を通る平面には点が 3 つ要ります(今は {} つ)",
                        self.points.len()
                    )));
                }
                let p: Vec<P3> = self
                    .points
                    .iter()
                    .map(|q| q.resolve(ctx))
                    .collect::<Result<_, _>>()?;
                let (u, v) = (k::sub(p[1], p[0]), k::sub(p[2], p[0]));
                let n = k::cross(u, v);
                let scale = k::dot(u, u).sqrt() * k::dot(v, v).sqrt();
                if scale < 1e-12 || k::dot(n, n).sqrt() < 1e-9 * scale {
                    return Err(EvalError::InvalidData(
                        "3 点が一直線に並んでいるか重なっているので、平面が決まりません".into(),
                    ));
                }
                let x = k::normalize(u);
                let plane = Plane {
                    origin: p[0],
                    x_axis: x,
                    y_axis: k::cross(k::normalize(n), x),
                };
                (plane, rect_of(&plane, &p))
            }
        };
        Ok(DatumPlane { plane, rect })
    }
}

impl Feature for DatumPlaneFeature {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v: Vec<InputRef> = Vec::new();
        let mut add = |i: InputRef| {
            if !v.contains(&i) {
                v.push(i);
            }
        };
        let bases: Vec<&Base> = match self.method {
            Method::Offset | Method::Angle => self.base.iter().collect(),
            Method::Midplane => self.base.iter().chain(self.other.iter()).collect(),
            Method::ThreePoints => Vec::new(),
        };
        for i in bases.into_iter().filter_map(Base::input) {
            add(i);
        }
        if self.method == Method::Angle
            && let Some(Axis::Edge { feature, .. }) = &self.axis
        {
            add(output(*feature, "body"));
        }
        for i in expr_refs(self)? {
            add(i);
        }
        if self.method == Method::ThreePoints {
            point_inputs(&self.points, &mut v);
        }
        Ok(v)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        rename(self, old, new)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let d = self.plane(ctx)?;
        log::debug!(
            "作業平面 {}: 原点 {:?} X {:?} Y {:?}",
            ctx.feature_id().0,
            d.plane.origin,
            d.plane.x_axis,
            d.plane.y_axis
        );
        Ok([(DATUM_OUTPUT.to_string(), d.to_value())].into())
    }
}

/// 機能の種類を登録する。
pub fn register(reg: &mut Registry) {
    reg.register_described(
        kmb_core::Descriptor::new(DATUM_PLANE, kmb_core::category::SOLID, "作業平面")
            .menu("構築")
            .icon("datumPlane")
            .hint("スケッチや機能の基準にする平面を作る(面・平面から浮かせる、軸のまわりに傾ける、2 つの面の真ん中、3 点を通る)")
            .aliases("さぎょうへいめん こうちく へいめん datum plane construction offset plane")
            .field(Field::choice("method", "作り方", METHODS))
            .field(
                Field::choice("base.kind", "元の平面", BASES)
                    .help("浮かせる・傾ける元の平面(中間平面では 1 つ目の面)")
                    .unless("method", "three_points"),
            )
            .field(Field::info("base.face", "元の面").only("base.kind", "face", "xy"))
            .field(
                Field::expr("distance", "距離")
                    .length()
                    .help("元の平面からの距離(負なら裏の側。X・Y の向きは元の平面のまま)")
                    .only("method", "offset", "offset"),
            )
            .field(Field::choice("axis.kind", "軸", &[("x", "原点の X 軸"), ("y", "原点の Y 軸"), ("z", "原点の Z 軸"), ("edge", "まっすぐな辺")]).only("method", "angle", "offset"))
            .field(Field::expr("angle", "角度").angle().only("method", "angle", "offset"))
            .field(Field::info("other.face", "2 つ目の面").only("method", "midplane", "offset"))
            .field(Field::info("points", "3 点").only("method", "three_points", "offset"))
            .retarget(kmb_core::Retarget::Plane { key: "base".into() })
            .template(json!({"method": "offset", "base": {"kind": "xy"}, "distance": "10 mm"})),
        |d| Ok(Box::new(parse::<DatumPlaneFeature>(d)?)),
    );
    axis::register(reg);
    point::register(reg);
}

/// この crate の名乗り(登録する種類と操作)。組み立て(kmb-bundle)が 1 回だけ並べる。
pub fn module() -> kmb_core::Module {
    kmb_core::Module::new("kmb-cad-datum", register).with_extension(DatumCommands)
}

#[cfg(test)]
mod tests;
