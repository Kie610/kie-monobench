//! 点の参照。位置は再計算した出力から読み、番号や名前が消えた時は止める(近くの別の点に付け替えない)。
//! ボディの点は面の名前(生成履歴による名前付け)で指すので、寸法を変えても同じ角・円・辺を追う。
use crate::{body_of, kernel_err, output};
use kmb_core::kernel::{self as k, P3};
use kmb_core::{
    Context, EvalCtx, EvalError, FeatureId, HandleChoice, InputRef, Payload, PointId, Points,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// 作業点、スケッチの点、ボディの頂点・円の辺の中心・まっすぐな辺の中点。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PointRef {
    Datum {
        #[serde(rename = "datum")]
        feature: FeatureId,
    },
    Sketch {
        #[serde(rename = "sketch")]
        feature: FeatureId,
        point: PointId,
    },
    Vertex {
        feature: FeatureId,
        vertex: String,
    },
    /// ボディの円の辺(両側の面の名前)の中心
    Center {
        feature: FeatureId,
        faces: [String; 2],
    },
    /// ボディのまっすぐな辺(両側の面の名前)の中点
    Midpoint {
        feature: FeatureId,
        faces: [String; 2],
    },
}

impl PointRef {
    pub fn input(&self) -> InputRef {
        match self {
            Self::Datum { feature } => output(*feature, kmb_interchange::DATUM_OUTPUT),
            Self::Sketch { feature, .. } => output(*feature, "points"),
            Self::Vertex { feature, .. }
            | Self::Center { feature, .. }
            | Self::Midpoint { feature, .. } => output(*feature, "body"),
        }
    }

    pub fn resolve(&self, ctx: &EvalCtx<'_>) -> Result<P3, EvalError> {
        match self {
            Self::Datum { feature } => kmb_interchange::datum_point(ctx, *feature),
            Self::Sketch { feature, point } => {
                let points =
                    Points::from_value(ctx.output(*feature, "points")?).ok_or_else(|| {
                        EvalError::InvalidData(
                            "端の点のスケッチがありません。点を選び直してください".into(),
                        )
                    })?;
                points
                    .points
                    .get(point)
                    .map(|p| points.plane.to_world(*p))
                    .ok_or_else(|| {
                        EvalError::InvalidData(format!(
                            "スケッチの点 {} がありません。端の点を選び直してください",
                            point.0
                        ))
                    })
            }
            Self::Vertex { feature, vertex } => {
                let body = body_of(ctx, *feature)?;
                kmb_core::vertices(ctx.kernel(), body)
                    .into_iter()
                    .find(|v| v.name == *vertex)
                    .map(|v| v.at)
                    .ok_or_else(|| {
                        EvalError::InvalidData(format!(
                            "端の頂点 {vertex} がありません。点を選び直してください"
                        ))
                    })
            }
            Self::Center { feature, faces } => {
                let body = body_of(ctx, *feature)?;
                let arcs = ctx.kernel().arc_edges(body).map_err(kernel_err)?;
                one_point(
                    arcs.iter()
                        .filter(|e| same(&e.faces, faces))
                        .filter_map(|e| e.arc.map(|a| a.center)),
                )
                .map_err(|n| missing("円の辺", faces, n))
            }
            Self::Midpoint { feature, faces } => {
                let body = body_of(ctx, *feature)?;
                let edges = ctx.kernel().edges(body).map_err(kernel_err)?;
                one_point(edges.iter().filter(|e| same(&e.faces, faces)).map(mid))
                    .map_err(|n| missing("まっすぐな辺", faces, n))
            }
        }
    }

    /// 機能 `feature` が読む物か(辺や角の持ち主のボディ、スケッチ、作業点)。
    pub fn feature(&self) -> FeatureId {
        match self {
            Self::Datum { feature }
            | Self::Sketch { feature, .. }
            | Self::Vertex { feature, .. }
            | Self::Center { feature, .. }
            | Self::Midpoint { feature, .. } => *feature,
        }
    }

    /// ボディの点(角・円の中心・辺の中点)なら、その持ち主のボディの機能。
    pub fn body(&self) -> Option<FeatureId> {
        match self {
            Self::Vertex { feature, .. }
            | Self::Center { feature, .. }
            | Self::Midpoint { feature, .. } => Some(*feature),
            Self::Datum { .. } | Self::Sketch { .. } => None,
        }
    }
}

fn same(f: &[String; 2], faces: &[String; 2]) -> bool {
    f == faces || (f[0] == faces[1] && f[1] == faces[0])
}

fn mid(e: &k::EdgeInfo) -> P3 {
    k::scale(k::add(e.a, e.b), 0.5)
}

/// 同じ点だけが並んでいればその点。無ければ `Err(0)`、別々の点が並べば `Err(数)`(どれか決められない)。
fn one_point(points: impl Iterator<Item = P3>) -> Result<P3, usize> {
    let all: Vec<P3> = points.collect();
    let first = *all.first().ok_or(0usize)?;
    let near = |p: &P3| {
        let d = k::sub(*p, first);
        k::dot(d, d) < 1e-12
    };
    if all.iter().all(near) {
        Ok(first)
    } else {
        Err(all.len())
    }
}

fn missing(what: &str, faces: &[String; 2], n: usize) -> EvalError {
    let at = faces.join("・");
    EvalError::InvalidData(if n == 0 {
        format!(
            "点にした{what}(面 {at} の境目)が見つかりません。形が変わって無くなりました。点を選び直してください"
        )
    } else {
        format!(
            "点にした{what}(面 {at} の境目)が {n} 本あり、どれか決められません。点を選び直してください"
        )
    })
}

/// 機能 `before` より前にある点の候補: 作業点、スケッチの点、見えているボディの角・円の辺の中心・
/// まっすぐな辺の中点。値は [`PointRef`] をそのまま入れる。どれか 1 つに決まらない円や辺(同じ面の組の
/// 境目が別々に 2 本以上ある)は出さない。
pub fn point_choices(cx: &Context<'_>, before: Option<FeatureId>) -> Vec<HandleChoice> {
    let end = before
        .and_then(|id| cx.data.index_of(id))
        .unwrap_or(cx.data.features.len());
    let earlier = |id| cx.data.index_of(id).is_some_and(|i| i < end);
    let mut out = Vec::new();
    let mut add = |point: PointRef, at: P3, label: String| {
        let outline = (0..3)
            .map(|axis| {
                let mut a = at;
                let mut b = at;
                a[axis] -= 0.5;
                b[axis] += 0.5;
                vec![a, b]
            })
            .collect();
        out.push(HandleChoice {
            value: json!(point),
            label,
            outline,
            ..Default::default()
        });
    };
    for (feature, name, point) in kmb_interchange::datum_points(cx, before) {
        add(PointRef::Datum { feature }, point.at, name);
    }
    for feature in cx
        .candidates("points")
        .into_iter()
        .map(FeatureId)
        .filter(|f| earlier(*f))
    {
        if let Some(points) = cx
            .output(feature, "points")
            .and_then(kmb_core::Points::from_value)
        {
            for (point, p) in points.points {
                add(
                    PointRef::Sketch { feature, point },
                    points.plane.to_world(p),
                    format!("スケッチ {feature} の点 {point}"),
                );
            }
        }
    }
    for (feature, body) in kmb_core::visible_bodies_before(cx.data, cx.registry, cx.report, before)
    {
        for vertex in kmb_core::vertices(cx.kernel.as_ref(), body) {
            add(
                PointRef::Vertex {
                    feature,
                    vertex: vertex.name.clone(),
                },
                vertex.at,
                format!("ボディ {feature} の角 {}", vertex.name),
            );
        }
        let arcs = cx.kernel.arc_edges(body).unwrap_or_default();
        let edges = cx.kernel.edges(body).unwrap_or_default();
        let mut seen: Vec<[String; 2]> = Vec::new();
        for (list, center) in [(&arcs, true), (&edges, false)] {
            seen.clear();
            for e in list.iter() {
                if seen.iter().any(|f| same(f, &e.faces)) {
                    continue;
                }
                seen.push(e.faces.clone());
                let row = list.iter().filter(|x| same(&x.faces, &e.faces));
                let at = if center {
                    one_point(row.filter_map(|x| x.arc.map(|a| a.center)))
                } else {
                    one_point(row.map(mid))
                };
                let Ok(at) = at else { continue };
                let faces = e.faces.clone();
                let label = format!(
                    "ボディ {feature} の{}(面 {})",
                    if center {
                        "円の中心"
                    } else {
                        "辺の中点"
                    },
                    faces.join("・")
                );
                let point = if center {
                    PointRef::Center { feature, faces }
                } else {
                    PointRef::Midpoint { feature, faces }
                };
                add(point, at, label);
            }
        }
    }
    out
}
