//! 軸の選び方の共通部: 原点の X・Y・Z 軸と、スケッチの線と、ボディの辺(回転、円形・直線の
//! パターン、移動が使う)。軸を選ぶ間は原点の軸と見えているスケッチの線とボディの
//! まっすぐな辺・円の辺をクリックで選べる(円の辺はその円の軸。穴やボスの軸のまわりに並べる)。
use crate::output;
use jc_core::kernel::{self as k, P3};
use jc_core::{
    Context, EvalCtx, EvalError, FeatureId, HandleChoice, InputRef, Payload, PointId, Points,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// 軸。`Vector` は機能の数の欄(向きと通る点)で決める。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AxisRef {
    #[default]
    Vector,
    OriginX,
    OriginY,
    OriginZ,
    /// スケッチ `sketch` の 2 点を通る直線。欄で「スケッチの線」を選んだだけの時は、線がまだ無い
    /// (線のクリックで入る)
    Line {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sketch: Option<FeatureId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        a: Option<PointId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        b: Option<PointId>,
    },
    /// 機能 `feature` のボディの、両側の面が `faces` の辺。まっすぐな辺はその直線、円の辺はその円の
    /// 軸(中心を通り円の面に垂直)。辺は面の名前で指すので、形を直しても同じ辺を追う
    Edge {
        feature: FeatureId,
        faces: [String; 2],
    },
}

/// 軸の欄の選択肢(`Vector` の名前は機能ごと)。`edge` は軸を選ぶ手順でボディの辺をクリックした時
/// だけ入るので、欄では隠す(`Field::hide_choice`)。
pub fn axis_ref_choices(vector: &'static str) -> [(&'static str, &'static str); 6] {
    [
        ("vector", vector),
        ("origin_x", "原点の X 軸"),
        ("origin_y", "原点の Y 軸"),
        ("origin_z", "原点の Z 軸"),
        ("line", "スケッチの線(クリックで選ぶ)"),
        ("edge", "ボディの辺"),
    ]
}

impl AxisRef {
    pub fn is_vector(&self) -> bool {
        *self == AxisRef::Vector
    }

    /// 計算に要る入力(スケッチの線ならそのスケッチの点)。
    pub fn inputs(&self) -> Vec<InputRef> {
        match self {
            AxisRef::Line {
                sketch: Some(s), ..
            } => vec![output(*s, "points")],
            AxisRef::Edge { feature, .. } => vec![output(*feature, "body")],
            _ => Vec::new(),
        }
    }

    /// 通る点と向き。`Vector` は `None`(機能の欄で決める)。
    pub fn resolve(&self, ctx: &EvalCtx<'_>) -> Result<Option<(P3, P3)>, EvalError> {
        Ok(Some(match self.clone() {
            AxisRef::Vector => return Ok(None),
            AxisRef::OriginX => origin_axis(0),
            AxisRef::OriginY => origin_axis(1),
            AxisRef::OriginZ => origin_axis(2),
            AxisRef::Line { sketch, a, b } => {
                let (Some(s), Some(a), Some(b)) = (sketch, a, b) else {
                    return Err(EvalError::InvalidData(
                        "軸の線をまだ選んでいません。右クリックの「軸を選ぶ」で、原点の軸かスケッチの線をクリックしてください"
                            .into(),
                    ));
                };
                sketch_line(ctx, s, a, b)?
            }
            AxisRef::Edge { feature, faces } => {
                let jc_core::Value::Shape(shape) = ctx.output(feature, "body")? else {
                    return Err(EvalError::InvalidData(format!(
                        "軸の辺の {feature} はボディではありません"
                    )));
                };
                edge_axis(ctx.kernel(), *shape, &faces).ok_or_else(|| {
                    EvalError::InvalidData(format!(
                        "軸の辺(面 {} と {} の間)が見つかりません。形が変わって辺が無くなったか、まっすぐでも円でもない辺です。軸を選び直してください",
                        faces[0], faces[1]
                    ))
                })?
            }
        }))
    }

    /// 軸を選ぶ手順で選んだ値([`axis_choices`] の値)から。
    pub fn from_choice(v: &serde_json::Value, cx: &Context<'_>) -> Result<Self, String> {
        if let Some(edge) = v.get("edge") {
            let faces: [String; 2] = serde_json::from_value(edge.clone())
                .map_err(|_| "edge は辺の両側の面の名前 2 つで指定します")?;
            let feature = v
                .get("feature")
                .and_then(|f| f.as_u64())
                .map(FeatureId)
                .ok_or("feature に辺を持つボディの機能を指定します")?;
            return Ok(AxisRef::Edge { feature, faces });
        }
        if let Some(s) = v.as_str() {
            return match s {
                "x" => Ok(AxisRef::OriginX),
                "y" => Ok(AxisRef::OriginY),
                "z" => Ok(AxisRef::OriginZ),
                _ => Err(format!("軸 {s} はありません(x・y・z か、スケッチの線)")),
            };
        }
        let s = v
            .get("sketch")
            .and_then(|s| s.as_u64())
            .map(FeatureId)
            .ok_or("軸は x・y・z か、{\"sketch\": スケッチ, \"line\": 線の番号} で指定します")?;
        let n = v
            .get("line")
            .and_then(|n| n.as_u64())
            .ok_or("line に軸にする線の番号を指定します")?;
        let (_, a, b, _) = sketch_lines(cx, s)
            .into_iter()
            .find(|l| l.0 == n)
            .ok_or_else(|| format!("スケッチ {s} に線 {n} がありません"))?;
        Ok(AxisRef::Line {
            sketch: Some(s),
            a: Some(a),
            b: Some(b),
        })
    }
}

/// ボディ `shape` の、両側の面が `faces` の辺を軸にした時の通る点と向き(まっすぐな辺はその直線、
/// 円の辺は円の中心を通る円の軸)。曲がった辺や見つからない辺は `None`。
pub fn edge_axis(
    kernel: &dyn k::Kernel,
    shape: k::ShapeHandle,
    faces: &[String; 2],
) -> Option<(P3, P3)> {
    let same = |f: &[String; 2]| {
        (f[0] == faces[0] && f[1] == faces[1]) || (f[0] == faces[1] && f[1] == faces[0])
    };
    let mut all = kernel.edges(shape).unwrap_or_default();
    all.extend(kernel.arc_edges(shape).unwrap_or_default());
    let e = all.iter().find(|e| same(&e.faces))?;
    line_of(e)
}

/// 辺を軸にした時の通る点と向き([`edge_axis`])。
fn line_of(e: &jc_core::EdgeInfo) -> Option<(P3, P3)> {
    if let Some(a) = &e.arc {
        return Some((a.center, a.axis));
    }
    let d = k::sub(e.b, e.a);
    (e.curve.is_empty() && k::dot(d, d) > 1e-18).then_some((e.a, d))
}

/// 辺を描く線(円の辺は折れ線。閉じない: 候補は線の近くのクリックで選び、円の内側では選ばない)。
fn edge_outline(e: &jc_core::EdgeInfo) -> Vec<P3> {
    let Some(a) = &e.arc else {
        return vec![e.a, e.b];
    };
    let n = 48;
    let r = k::sub(e.a, a.center);
    let ax = {
        let l = k::dot(a.axis, a.axis).sqrt().max(1e-12);
        a.axis.map(|v| v / l)
    };
    (0..n)
        .map(|i| {
            let t = a.sweep * i as f64 / n as f64;
            let (s, c) = t.sin_cos();
            let cr = k::cross(ax, r);
            let dr = k::dot(ax, r) * (1.0 - c);
            k::add(
                a.center,
                [0, 1, 2].map(|j| r[j] * c + cr[j] * s + ax[j] * dr),
            )
        })
        .collect()
}

/// 原点の X・Y・Z 軸(`i` = 0・1・2)の通る点と向き。
pub fn origin_axis(i: usize) -> (P3, P3) {
    let mut d = [0.0; 3];
    d[i] = 1.0;
    ([0.0; 3], d)
}

/// スケッチ `sketch` の点 `a`・`b` を通る直線(世界座標の通る点と向き)。
pub fn sketch_line(
    ctx: &EvalCtx<'_>,
    sketch: FeatureId,
    a: PointId,
    b: PointId,
) -> Result<(P3, P3), EvalError> {
    let sol = Points::from_value(ctx.output(sketch, "points")?)
        .ok_or_else(|| EvalError::InvalidData(format!("{sketch} はスケッチではありません")))?;
    let (Some(pa), Some(pb)) = (sol.points.get(&a), sol.points.get(&b)) else {
        return Err(EvalError::InvalidData(format!(
            "軸の点 {a} か {b} がスケッチにありません"
        )));
    };
    let (pa, pb) = (sol.plane.to_world(*pa), sol.plane.to_world(*pb));
    let d = k::sub(pb, pa);
    if k::dot(d, d) < 1e-18 {
        return Err(EvalError::InvalidData("軸の線の長さが 0 です".into()));
    }
    Ok((pa, d))
}

/// スケッチ `sketch` の線(番号、端の点 2 つ、世界座標の端)。補助線も含む。
pub fn sketch_lines(cx: &Context<'_>, sketch: FeatureId) -> Vec<(u64, PointId, PointId, [P3; 2])> {
    let Some(entry) = cx.data.feature(sketch) else {
        return Vec::new();
    };
    let Some(sol) = cx.output(sketch, "points").and_then(Points::from_value) else {
        return Vec::new();
    };
    let Some(entities) = entry.data.get("entities").and_then(|e| e.as_array()) else {
        return Vec::new();
    };
    entities
        .iter()
        .filter(|e| e.get("type").and_then(|t| t.as_str()) == Some("line"))
        .filter_map(|e| {
            let n = e.get("id")?.as_u64()?;
            let a = PointId(e.get("a")?.as_u64()? as u32);
            let b = PointId(e.get("b")?.as_u64()? as u32);
            let (pa, pb) = (sol.points.get(&a)?, sol.points.get(&b)?);
            Some((n, a, b, [sol.plane.to_world(*pa), sol.plane.to_world(*pb)]))
        })
        .collect()
}

/// 軸を選ぶ手順の候補: 原点の X・Y・Z 軸(値 `"x"` など)と、機能 `before` より前の、隠していない
/// スケッチの線(値 `{"sketch": 番号, "line": 線の番号}`。隠したスケッチは画面が外す)と、その時に
/// 見えているボディのまっすぐな辺・円の辺(値 `{"feature": 番号, "edge": [面, 面]}`)。
/// `skip` のスケッチの線は入れない(回転の輪郭のスケッチなど、呼ぶ側が別の値で出すもの)。
/// `normal` があれば、それに平行な軸(輪郭の平面に垂直な軸)は入れない。
pub fn axis_choices(
    cx: &Context<'_>,
    before: FeatureId,
    skip: Option<FeatureId>,
    normal: Option<P3>,
) -> Vec<HandleChoice> {
    let usable = |_: P3, d: P3| {
        normal.is_none_or(|n| {
            let (ln, ld) = (k::dot(n, n).sqrt(), k::dot(d, d).sqrt());
            ln < 1e-12 || ld < 1e-12 || k::dot(n, d).abs() / (ln * ld) < 1.0 - 1e-9
        })
    };
    axis_choices_where(cx, before, skip, usable)
}

/// 直線(点 `p` を通る向き `d`)が平面 `plane` の上にあるか(回転の軸は輪郭と同じ平面に置く)。
pub fn on_plane(plane: &k::Plane, p: P3, d: P3) -> bool {
    let n = plane.normal();
    let (ln, ld) = (k::dot(n, n).sqrt(), k::dot(d, d).sqrt());
    if ln < 1e-12 || ld < 1e-12 {
        return false;
    }
    let off = k::dot(n, k::sub(p, plane.origin)) / ln;
    off.abs() < 1e-6 && k::dot(n, d).abs() / (ln * ld) < 1e-6
}

/// [`axis_choices`] の、候補にする軸を `usable(通る点, 向き)` で決めるもの。
pub fn axis_choices_where(
    cx: &Context<'_>,
    before: FeatureId,
    skip: Option<FeatureId>,
    usable: impl Fn(P3, P3) -> bool,
) -> Vec<HandleChoice> {
    let mut v: Vec<HandleChoice> = (0..3)
        .filter(|i| {
            let (p, d) = origin_axis(*i);
            usable(p, d)
        })
        .map(|i| HandleChoice::origin_axis(i, json!(["x", "y", "z"][i])))
        .collect();
    let end = cx
        .data
        .features
        .iter()
        .position(|f| f.id == before)
        .unwrap_or(cx.data.features.len());
    for f in &cx.data.features[..end] {
        if f.suppressed || Some(f.id) == skip {
            continue;
        }
        for (n, _, _, ends) in sketch_lines(cx, f.id) {
            if !usable(ends[0], k::sub(ends[1], ends[0])) {
                continue;
            }
            v.push(HandleChoice {
                value: json!({"sketch": f.id.0, "line": n}),
                label: format!("{} の線 {n}", f.name),
                outline: vec![ends.to_vec()],
                ..Default::default()
            });
        }
    }
    // ボディの辺
    let until = cx.data.feature(before).map(|f| f.id);
    for (body, shape) in jc_core::visible_bodies_before(cx.data, cx.registry, cx.report, until) {
        let mut edges = cx.kernel.edges(shape).unwrap_or_default();
        edges.extend(cx.kernel.arc_edges(shape).unwrap_or_default());
        for e in edges {
            let Some((p, d)) = line_of(&e) else { continue };
            if !usable(p, d) {
                continue;
            }
            v.push(HandleChoice {
                value: json!({"feature": body.0, "edge": e.faces}),
                label: if e.arc.is_some() {
                    format!("円の辺の軸({} と {})", e.faces[0], e.faces[1])
                } else {
                    format!("辺({} と {})", e.faces[0], e.faces[1])
                },
                outline: vec![edge_outline(&e)],
                ..Default::default()
            });
        }
    }
    v
}
