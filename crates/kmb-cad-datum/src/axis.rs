//! 作業軸(`cad.datum_axis@1`)。2 点を通る、ボディの辺、円筒の面の軸、2 つの平面の交わり、の 4 つの
//! 作り方を 1 つの機能で持ち、出力 [`DATUM_OUTPUT`] に軸の形([`DatumAxis`])を出す。回転・円形の
//! パターン・移動の軸に選べる(`kmb_cad::AxisRef::Datum`)。
use crate::{BASES, Base, Point, need, point_inputs};
use kmb_cad::*;
use kmb_core::kernel::{self as k, P3};
use kmb_core::{
    EvalCtx, EvalError, Feature, FeatureId, Field, InputRef, Outputs, Payload, Registry,
};
use kmb_interchange::{DATUM_OUTPUT, DatumAxis};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const DATUM_AXIS: &str = "cad.datum_axis@1";

/// 作業軸の作り方。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AxisMethod {
    /// 2 点 `points` を通る(1 点目から 2 点目の向き)
    #[default]
    TwoPoints,
    /// ボディの辺 `edge`(まっすぐな辺はその直線、円の辺はその円の軸)
    Edge,
    /// 円筒の面 `face` の軸
    Cylinder,
    /// 2 つの平面 `base` と `other` の交わり
    TwoPlanes,
}

pub(crate) const AXIS_METHODS: &[(&str, &str)] = &[
    ("two_points", "2 点を通る軸"),
    ("edge", "辺に沿う軸"),
    ("cylinder", "円筒の軸"),
    ("two_planes", "2 平面の交わり"),
];

/// ボディの辺(両側の面の名前)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdgeRef {
    pub feature: FeatureId,
    pub faces: [String; 2],
}

/// ボディの面。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FaceRef {
    pub feature: FeatureId,
    pub face: String,
}

/// 作業軸。例: `{"method": "edge", "edge": {"feature": 2, "faces": ["f2:side:e0", "f2:end"]}}`。
/// 作り方で使わない欄は残っていても読まない(作業平面と同じ)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct DatumAxisFeature {
    #[serde(default)]
    pub method: AxisMethod,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge: Option<EdgeRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<FaceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<Base>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub other: Option<Base>,
}

/// 点 `pts` を軸(`at` を通る向き `dir`)に写した範囲を、少し広げた物。
fn span_of(at: P3, dir: P3, pts: &[P3]) -> [f64; 2] {
    let ts: Vec<f64> = pts.iter().map(|p| k::dot(k::sub(*p, at), dir)).collect();
    let (lo, hi) = ts
        .iter()
        .fold((0.0f64, 0.0f64), |(lo, hi), t| (lo.min(*t), hi.max(*t)));
    let pad = ((hi - lo) * 0.1).max(5.0);
    [lo - pad, hi + pad]
}

impl DatumAxisFeature {
    /// 作業軸の形。
    pub fn axis(&self, ctx: &EvalCtx<'_>) -> Result<DatumAxis, EvalError> {
        let (at, dir, near) = match self.method {
            AxisMethod::TwoPoints => {
                if self.points.len() != 2 {
                    return Err(EvalError::InvalidData(format!(
                        "2 点を通る軸には点が 2 つ要ります(今は {} つ)",
                        self.points.len()
                    )));
                }
                let a = self.points[0].resolve(ctx)?;
                let b = self.points[1].resolve(ctx)?;
                let d = k::sub(b, a);
                if k::dot(d, d) < 1e-18 {
                    return Err(EvalError::InvalidData(
                        "2 点が重なっているので、軸の向きが決まりません".into(),
                    ));
                }
                (a, d, vec![a, b])
            }
            AxisMethod::Edge => {
                let e = need(&self.edge, "辺")?;
                let body = body_of(ctx, e.feature)?;
                let (at, dir) = kmb_cad::axis::edge_axis(ctx.kernel(), body, &e.faces).ok_or_else(|| {
                    EvalError::InvalidData(format!(
                        "作業軸の辺(面 {} の境目)が見つかりません。形が変わって辺が無くなったか、まっすぐでも円でもない辺です。辺を選び直してください",
                        e.faces.join("・")
                    ))
                })?;
                let mut all = ctx.kernel().edges(body).map_err(kernel_err)?;
                all.extend(ctx.kernel().arc_edges(body).map_err(kernel_err)?);
                let ends = all
                    .iter()
                    .find(|x| {
                        x.faces == e.faces || (x.faces[0] == e.faces[1] && x.faces[1] == e.faces[0])
                    })
                    .map(|x| vec![x.a, x.b])
                    .unwrap_or_default();
                (at, dir, ends)
            }
            AxisMethod::Cylinder => {
                let f = need(&self.face, "円筒の面")?;
                let body = body_of(ctx, f.feature)?;
                // 円筒の面の縁の円の辺から軸を取る(円の辺は平面の面が先、円筒の面が後)
                let arc = ctx
                    .kernel()
                    .arc_edges(body)
                    .map_err(kernel_err)?
                    .into_iter()
                    .filter(|e| e.faces[1] == f.face)
                    .find_map(|e| e.arc)
                    .ok_or_else(|| {
                        EvalError::InvalidData(format!(
                            "円筒の面 {} が見つからないか、縁に円の辺がありません。円筒の面を選び直してください",
                            f.face
                        ))
                    })?;
                let (lo, hi) = ctx.kernel().bounds(body).map_err(kernel_err)?;
                let corners = [lo, hi];
                (arc.center, arc.axis, corners.to_vec())
            }
            AxisMethod::TwoPlanes => {
                let (a, ra) = need(&self.base, "1 つ目の平面")?.resolve(ctx)?;
                let (b, rb) = need(&self.other, "2 つ目の平面")?.resolve(ctx)?;
                let (na, nb) = (a.normal(), b.normal());
                let dir = k::cross(na, nb);
                let s = k::dot(dir, dir);
                if s < 1e-12 {
                    return Err(EvalError::InvalidData(
                        "2 つの平面が平行なので交わりません。交わる 2 つの平面を選んでください"
                            .into(),
                    ));
                }
                // 両方の平面の上にあり、原点に近い点
                let (da, db) = (k::dot(na, a.origin), k::dot(nb, b.origin));
                let at = k::scale(
                    k::add(
                        k::scale(k::cross(nb, dir), da),
                        k::scale(k::cross(dir, na), db),
                    ),
                    1.0 / s,
                );
                let corners = [ra[0], ra[1]]
                    .iter()
                    .map(|uv| a.to_world(*uv))
                    .chain([rb[0], rb[1]].iter().map(|uv| b.to_world(*uv)))
                    .collect();
                (at, dir, corners)
            }
        };
        let dir = k::normalize(dir);
        Ok(DatumAxis {
            at,
            dir,
            span: span_of(at, dir, &near),
        })
    }
}

impl Feature for DatumAxisFeature {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v: Vec<InputRef> = Vec::new();
        match self.method {
            AxisMethod::TwoPoints => point_inputs(&self.points, &mut v),
            AxisMethod::Edge => v.extend(self.edge.iter().map(|e| output(e.feature, "body"))),
            AxisMethod::Cylinder => v.extend(self.face.iter().map(|f| output(f.feature, "body"))),
            AxisMethod::TwoPlanes => {
                for i in self
                    .base
                    .iter()
                    .chain(self.other.iter())
                    .filter_map(Base::input)
                {
                    if !v.contains(&i) {
                        v.push(i);
                    }
                }
            }
        }
        Ok(v)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let d = self.axis(ctx)?;
        log::debug!(
            "作業軸 {}: 通る点 {:?} 向き {:?}",
            ctx.feature_id().0,
            d.at,
            d.dir
        );
        Ok([(DATUM_OUTPUT.to_string(), d.to_value())].into())
    }
}

pub(crate) fn register(reg: &mut Registry) {
    reg.register_described(
        kmb_core::Descriptor::new(DATUM_AXIS, kmb_core::category::SOLID, "作業軸")
            .menu("構築")
            .icon("datumAxis")
            .hint("回転やパターンの軸にする直線を作る(2 点を通る、辺に沿う、円筒の軸、2 平面の交わり)")
            .aliases("さぎょうじく こうちく じく datum axis construction")
            .field(Field::choice("method", "作り方", AXIS_METHODS))
            .field(Field::info("points", "2 点").only("method", "two_points", "two_points"))
            .field(Field::info("edge", "辺").only("method", "edge", "two_points"))
            .field(Field::info("face", "円筒の面").only("method", "cylinder", "two_points"))
            .field(Field::choice("base.kind", "1 つ目の平面", BASES).only("method", "two_planes", "two_points"))
            .field(Field::choice("other.kind", "2 つ目の平面", BASES).only("method", "two_planes", "two_points"))
            .template(json!({"method": "two_planes", "base": {"kind": "xy"}, "other": {"kind": "yz"}})),
        |d| Ok(Box::new(parse::<DatumAxisFeature>(d)?)),
    );
}
