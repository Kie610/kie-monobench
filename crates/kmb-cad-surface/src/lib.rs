//! スケッチの領域、3D の線、ボディの辺を縁として面を張る。
use kmb_cad::{output, parse, solid};
use kmb_core::kernel::{self as k, BoundaryCurve, BoundarySegment, P3};
use kmb_core::{
    Action, Arg, Context, EvalCtx, EvalError, Extension, Feature, FeatureId, Field, Gesture,
    Handle, HandleChoice, InputRef, Outcome, Outputs, Payload, Place, Registry, Value, Wires,
};
use kmb_interchange::{SEGMENTS_OUTPUT, Segments};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const SURFACE_PATCH: &str = "solid.surface_patch@1";
pub const PATCH: &str = "cad.surface_patch";

/// 縁の参照。点の座標は保存せず、計算のたびに元の機能から読む。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum BoundaryRef {
    Region(RegionRef),
    Wire(WireRef),
    Edge(EdgeRef),
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegionRef {
    pub profile: FeatureId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireRef {
    pub sketch: FeatureId,
    /// Segments の line(EntityId)。出力の並び順ではなく、線分そのものの ID。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// 作成操作だけが受ける出力順の番号。保存前に線分の永続番号へ解決する。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wire: Option<usize>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub whole: bool,
}
fn is_false(value: &bool) -> bool {
    !*value
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EdgeRef {
    pub edge: kmb_core::Ref,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Source {
    Region,
    Wire,
    Edge,
}

/// 種類を切り替えた時に残る既知の欄は受け、宣言に無い欄は断る。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BoundaryData {
    source: Source,
    profile: Option<FeatureId>,
    key: Option<String>,
    sketch: Option<FeatureId>,
    wire: Option<usize>,
    line: Option<u32>,
    #[serde(default)]
    whole: bool,
    edge: Option<kmb_core::Ref>,
}
impl<'de> Deserialize<'de> for BoundaryRef {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let d = BoundaryData::deserialize(d)?;
        Ok(match d.source {
            Source::Region => Self::Region(RegionRef {
                profile: d
                    .profile
                    .ok_or_else(|| D::Error::custom("領域のスケッチを選んでください"))?,
                key: d.key.filter(|k| !k.is_empty()),
            }),
            Source::Wire => {
                let sketch = d
                    .sketch
                    .ok_or_else(|| D::Error::custom("3D スケッチを選んでください"))?;
                if !d.whole && d.line.is_none() && d.wire.is_none() {
                    return Err(D::Error::custom(
                        "線分の番号を指定するか、線1本のスケッチ全体を選んでください",
                    ));
                }
                if !d.whole && d.line.is_some() && d.wire.is_some() {
                    return Err(D::Error::custom(
                        "線分の番号と出力順の番号は同時に指定できません",
                    ));
                }
                Self::Wire(WireRef {
                    sketch,
                    line: if d.whole { None } else { d.line },
                    wire: if d.whole { None } else { d.wire },
                    whole: d.whole,
                })
            }
            Source::Edge => Self::Edge(EdgeRef {
                edge: d
                    .edge
                    .filter(|r| matches!(r, kmb_core::Ref::Edge { .. }))
                    .ok_or_else(|| D::Error::custom("ボディの辺を選んでください"))?,
            }),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfacePatch {
    pub boundary: Vec<BoundaryRef>,
}

fn invalid(s: impl Into<String>) -> EvalError {
    EvalError::InvalidData(s.into())
}
fn gap(a: P3, b: P3) -> f64 {
    let d = k::sub(a, b);
    k::dot(d, d).sqrt()
}

fn same_segment(wire: &kmb_core::Wire, segment: &kmb_interchange::Segment) -> bool {
    !wire.construction
        && wire.points.len() == 2
        && ((gap(wire.points[0], segment.ends[0]) < 1e-6
            && gap(wire.points[1], segment.ends[1]) < 1e-6)
            || (gap(wire.points[0], segment.ends[1]) < 1e-6
                && gap(wire.points[1], segment.ends[0]) < 1e-6))
}

fn intersects(a: P3, b: P3, c: P3, d: P3) -> bool {
    let u = k::sub(b, a);
    let v = k::sub(d, c);
    let w = k::sub(a, c);
    let aa = k::dot(u, u);
    let bb = k::dot(u, v);
    let cc = k::dot(v, v);
    let dd = k::dot(u, w);
    let ee = k::dot(v, w);
    let det = aa * cc - bb * bb;
    if det > 1e-20 {
        let s = (bb * ee - cc * dd) / det;
        let t = (aa * ee - bb * dd) / det;
        if (0.0..=1.0).contains(&s)
            && (0.0..=1.0).contains(&t)
            && gap(k::add(a, k::scale(u, s)), k::add(c, k::scale(v, t))) <= 1e-8
        {
            return true;
        }
    }
    let on = |p: P3, a: P3, b: P3| {
        let ab = k::sub(b, a);
        let length = k::dot(ab, ab);
        let t = if length > 0.0 {
            (k::dot(k::sub(p, a), ab) / length).clamp(0.0, 1.0)
        } else {
            0.0
        };
        gap(p, k::add(a, k::scale(ab, t))) <= 1e-8
    };
    on(a, c, d) || on(b, c, d) || on(c, a, b) || on(d, a, b)
}

/// 端でたどれる 1 つの輪に並べる。分かれ道・離れた輪を曖昧に選ばない。
fn ordered(
    mut segments: Vec<(BoundarySegment, Vec<P3>)>,
) -> Result<Vec<BoundarySegment>, EvalError> {
    if segments.is_empty() || segments.len() > 32 {
        return Err(invalid("縁は 1〜32 区間にしてください"));
    }
    for (segment, points) in &segments {
        if points.len() < 2 {
            return Err(invalid(format!(
                "縁 {} の点が足りません。2 点以上で描き直してください",
                segment.name
            )));
        }
        if let Some(index) = points.iter().position(|p| p.iter().any(|v| !v.is_finite())) {
            return Err(invalid(format!(
                "縁 {} の点 {} が不正です。有限の座標で描き直してください",
                segment.name,
                index + 1
            )));
        }
        if let Some(index) = points.windows(2).position(|w| gap(w[0], w[1]) <= 1e-9) {
            return Err(invalid(format!(
                "縁 {} の点 {} と点 {} の間に長さ 0 の区間があります。重なった点を離してください",
                segment.name,
                index + 1,
                index + 2
            )));
        }
    }
    let mut out = vec![segments.remove(0)];
    while !segments.is_empty() {
        let end = *out.last().expect("最初の縁").1.last().expect("2 点以上");
        let matches: Vec<_> = segments
            .iter()
            .enumerate()
            .filter_map(|(i, (_, p))| {
                if gap(end, p[0]) <= 1e-4 {
                    Some((i, false))
                } else if gap(end, *p.last().expect("2 点以上")) <= 1e-4 {
                    Some((i, true))
                } else {
                    None
                }
            })
            .collect();
        if matches.len() != 1 {
            return Err(invalid(format!(
                "縁 {} の終わりから 1 つの輪をたどれません。隙間を 0.0001 mm 以内に閉じ、分かれ道や別の輪を外してください",
                out.last().expect("縁").0.name
            )));
        }
        let (i, reverse) = matches[0];
        let (mut segment, mut points) = segments.remove(i);
        if reverse {
            points.reverse();
            match &mut segment.curve {
                BoundaryCurve::Line { a, b } => std::mem::swap(a, b),
                BoundaryCurve::Through(p) => p.reverse(),
                BoundaryCurve::Arc { from, arc } => {
                    *from = arc.point(*from, arc.sweep);
                    arc.sweep = -arc.sweep;
                }
                BoundaryCurve::BodyEdge { .. } => {}
            }
        }
        out.push((segment, points));
    }
    if gap(
        out[0].1[0],
        *out.last().expect("縁").1.last().expect("2 点以上"),
    ) > 1e-4
    {
        return Err(invalid(
            "縁の終わりが始まりにつながっていません。隙間を 0.0001 mm 以内に閉じてください",
        ));
    }
    let pieces: Vec<_> = out
        .iter()
        .flat_map(|(s, p)| p.windows(2).map(move |w| (s.name.as_str(), w[0], w[1])))
        .collect();
    for i in 0..pieces.len() {
        for j in i + 1..pieces.len() {
            if j == i + 1 || (i == 0 && j + 1 == pieces.len()) {
                let (_, a, b) = pieces[i];
                let (_, c, d) = pieces[j];
                let u = k::sub(b, a);
                let v = k::sub(d, c);
                let cross = k::cross(u, v);
                if k::dot(cross, cross) < 1e-20 && k::dot(u, v) < 0.0 {
                    return Err(invalid(format!(
                        "縁 {} と縁 {} が折り返して重なっています。重なる区間を外してください",
                        pieces[i].0, pieces[j].0
                    )));
                }
                continue;
            }
            let (a, p, q) = pieces[i];
            let (b, r, s) = pieces[j];
            if intersects(p, q, r, s) {
                return Err(invalid(format!(
                    "縁 {a} と縁 {b} が交差または重なっています。交わらない輪に描き直してください"
                )));
            }
        }
    }
    Ok(out.into_iter().map(|(s, _)| s).collect())
}

impl Feature for SurfacePatch {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        if self.boundary.is_empty() {
            return Err(invalid("縁を選んでください"));
        }
        let mut inputs = Vec::new();
        for reference in &self.boundary {
            let needed = match reference {
                BoundaryRef::Region(r) => vec![output(
                    r.profile,
                    if r.key.is_some() { "faces" } else { "profile" },
                )],
                BoundaryRef::Wire(r) => {
                    if r.wire.is_some() {
                        return Err(invalid(
                            "出力順の番号は保存した参照に使えません。面を張る操作で線分を選び直してください",
                        ));
                    }
                    if r.whole {
                        vec![output(r.sketch, "lines")]
                    } else {
                        if r.line.is_none() {
                            return Err(invalid("線分の番号を指定してください"));
                        }
                        vec![output(r.sketch, "lines"), output(r.sketch, SEGMENTS_OUTPUT)]
                    }
                }
                BoundaryRef::Edge(r) => vec![output(r.edge.feature(), "body")],
            };
            for input in needed {
                if !inputs.contains(&input) {
                    inputs.push(input);
                }
            }
        }
        Ok(inputs)
    }
    fn keeps(&self) -> Vec<FeatureId> {
        self.boundary
            .iter()
            .filter_map(|r| match r {
                BoundaryRef::Edge(e) => Some(e.edge.feature()),
                _ => None,
            })
            .collect()
    }
    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let tag = kmb_cad::tag(ctx);
        let resolved = self.resolve(ctx.kernel(), |feature, slot| {
            ctx.output(feature, slot).cloned()
        })?;
        let result = match resolved {
            Resolved::Profile(p) => ctx.kernel().planar_sheet(&p, &tag),
            Resolved::Boundary(b) => ctx.kernel().patch_sheet(&b, &tag),
        };
        let h = result.map_err(|e| match e {
            k::KernelError::Failed(s) => invalid(s),
            e => kmb_cad::kernel_err(e),
        })?;
        Ok([("body".into(), Value::Shape(h))].into())
    }
}

enum Resolved {
    Profile(k::Profile),
    Boundary(Vec<BoundarySegment>),
}

impl SurfacePatch {
    fn normalize(&mut self, cx: &Context<'_>) -> Result<(), kmb_core::Problem> {
        for reference in &mut self.boundary {
            let BoundaryRef::Wire(r) = reference else {
                continue;
            };
            let Some(index) = r.wire.take() else {
                continue;
            };
            let wires = cx
                .output(r.sketch, "lines")
                .and_then(Wires::from_value)
                .ok_or("3D スケッチの線がありません。縁を選び直してください")?;
            let wire = wires
                .0
                .get(index)
                .filter(|w| !w.construction)
                .ok_or_else(|| {
                    format!(
                        "3D スケッチ {} の線 {index} がありません。縁を選び直してください",
                        r.sketch
                    )
                })?;
            if wire.points.len() != 2 {
                return Err("曲線は出力順の番号で追跡できません。曲線1本のスケッチを「全体」で選ぶか、スケッチを分けてください".into());
            }
            let all = kmb_interchange::segments_now(cx, r.sketch)
                .ok_or("線分の永続番号がありません。3D スケッチの線分を選んでください")?;
            let matches: Vec<_> = all.0.iter().filter(|s| same_segment(wire, s)).collect();
            if matches.len() != 1 {
                return Err("選んだ線分の番号を一意に決められません。重なった線分を外して選び直してください".into());
            }
            r.line = Some(matches[0].line);
        }
        Ok(())
    }

    fn resolve(
        &self,
        kernel: &dyn kmb_core::Kernel,
        read: impl Fn(FeatureId, &str) -> Result<Value, EvalError>,
    ) -> Result<Resolved, EvalError> {
        self.inputs()?;
        if let Some(BoundaryRef::Region(first)) = self.boundary.first() {
            let mut keys = Vec::new();
            let mut whole = false;
            for reference in &self.boundary {
                let BoundaryRef::Region(r) = reference else {
                    return Err(invalid(
                        "スケッチの領域と線・辺は一緒に使えません。縁を選び直してください",
                    ));
                };
                if r.profile != first.profile {
                    return Err(invalid("領域は同じスケッチから選んでください"));
                }
                if let Some(key) = &r.key {
                    keys.push(key.clone());
                } else {
                    whole = true;
                }
            }
            let profile = if whole {
                kmb_core::Profile::from_value(&read(first.profile, "profile")?)
                    .ok_or_else(|| invalid("閉じた輪郭のあるスケッチを選んでください"))?
            } else {
                kmb_core::ProfileFaces::from_value(&read(first.profile, "faces")?)
                    .ok_or_else(|| invalid("領域を選べるスケッチではありません"))?
                    .profile(&keys)
                    .map_err(invalid)?
            };
            validate_profile(&profile)?;
            Ok(Resolved::Profile(profile))
        } else {
            let mut segments = Vec::new();
            for r in &self.boundary {
                match r {
                    BoundaryRef::Region(_) => {
                        return Err(invalid(
                            "スケッチの領域は 1 つずつ選んでください。領域と線・辺は一緒に使えません",
                        ));
                    }
                    BoundaryRef::Wire(r) => {
                        let wires =
                            Wires::from_value(&read(r.sketch, "lines")?).ok_or_else(|| {
                                invalid("3D スケッチの線がありません。縁を選び直してください")
                            })?;
                        let (name, points) = if r.whole {
                            let main: Vec<_> = wires.0.iter().filter(|w| !w.construction).collect();
                            if main.len() != 1 {
                                return Err(invalid(
                                    "全体で選べるのは線が1本の3D スケッチです。複数の線分は個別に選び、曲線はスケッチを分けてください",
                                ));
                            }
                            (format!("{}:whole", r.sketch), main[0].points.clone())
                        } else {
                            let line = r
                                .line
                                .ok_or_else(|| invalid("線分の番号を選んでください"))?;
                            let all=Segments::from_value(&read(r.sketch,SEGMENTS_OUTPUT)?).ok_or_else(||invalid("線分の永続番号がありません。3D スケッチの線分を選んでください"))?;
                            let segment=all.0.iter().find(|s|s.line==line).ok_or_else(||invalid(format!("3D スケッチ {} の線分 {line} がありません。別の線には替えず、縁を選び直してください",r.sketch)))?;
                            if !wires.0.iter().any(|w| same_segment(w, segment)) {
                                return Err(invalid(format!(
                                    "3D スケッチ {} の線分 {line} は描く線にありません。補助線を外して縁を選び直してください",
                                    r.sketch
                                )));
                            }
                            (format!("{}:line:{line}", r.sketch), segment.ends.to_vec())
                        };
                        if points.len() < 2 {
                            return Err(invalid("縁の線に2点以上必要です。線を描き直してください"));
                        }
                        let curve = if points.len() == 2 {
                            BoundaryCurve::Line {
                                a: points[0],
                                b: points[1],
                            }
                        } else {
                            BoundaryCurve::Through(points.clone())
                        };
                        segments.push((BoundarySegment { name, curve }, points));
                    }
                    BoundaryRef::Edge(r) => {
                        let kmb_core::Ref::Edge { feature, faces } = &r.edge else {
                            return Err(invalid("ボディの辺を選んでください"));
                        };
                        let Value::Shape(shape) = read(*feature, "body")? else {
                            return Err(invalid("選んだ機能にボディがありません"));
                        };
                        let mut edges = kernel.edges(shape).map_err(kmb_cad::kernel_err)?;
                        edges.extend(kernel.arc_edges(shape).map_err(kmb_cad::kernel_err)?);
                        edges.extend(kernel.curve_edges(shape).map_err(kmb_cad::kernel_err)?);
                        let matching: Vec<_> = edges
                            .into_iter()
                            .filter(|e| {
                                e.faces == *faces || e.faces == [faces[1].clone(), faces[0].clone()]
                            })
                            .collect();
                        if matching.len() != 1 {
                            return Err(invalid(format!(
                                "縁 {} の辺が見つからないか、同じ面の組に複数あります。区別できる辺を選び直してください",
                                faces.join("|")
                            )));
                        }
                        let points = matching[0].polyline(128);
                        let point = points[points.len() / 2];
                        segments.push((
                            BoundarySegment {
                                name: faces.join("|"),
                                curve: BoundaryCurve::BodyEdge {
                                    shape,
                                    faces: faces.clone(),
                                    point,
                                },
                            },
                            points,
                        ));
                    }
                }
            }
            let boundary = ordered(segments)?;
            Ok(Resolved::Boundary(boundary))
        }
    }
}

fn validate_profile(profile: &k::Profile) -> Result<(), EvalError> {
    let loops: Vec<_> = profile
        .regions
        .iter()
        .flat_map(|r| std::iter::once(&r.outer).chain(&r.holes))
        .collect();
    if loops.is_empty() || loops.iter().map(|l| l.edges.len()).sum::<usize>() > 32 {
        return Err(invalid(
            "縁は 1〜32 区間にしてください。閉じた輪郭を描き、区間を減らしてください",
        ));
    }
    let mut previous: Vec<(String, Vec<P3>)> = Vec::new();
    for contour in loops {
        let segments: Vec<_> = contour
            .edges
            .iter()
            .map(|edge| {
                let mut points = edge.segment.polyline(128);
                points.push(edge.segment.end_point());
                if edge.reversed {
                    points.reverse();
                }
                let points: Vec<_> = points
                    .into_iter()
                    .map(|p| profile.plane.to_world(p))
                    .collect();
                (
                    BoundarySegment {
                        name: edge.name.clone(),
                        curve: BoundaryCurve::Through(points.clone()),
                    },
                    points,
                )
            })
            .collect();
        ordered(segments.clone())?;
        for (segment, points) in &segments {
            for (name, other) in &previous {
                if points
                    .windows(2)
                    .any(|a| other.windows(2).any(|b| intersects(a[0], a[1], b[0], b[1])))
                {
                    return Err(invalid(format!(
                        "縁 {} と縁 {name} が交差しています。穴と外側の輪郭を離してください",
                        segment.name
                    )));
                }
            }
        }
        previous.extend(segments.into_iter().map(|(s, p)| (s.name, p)));
    }
    Ok(())
}

pub fn register(reg: &mut Registry) {
    reg.register_described(
        solid(SURFACE_PATCH, "面を張る")
            .icon("body")
            .hint("閉じた縁に厚みの無い面を張る")
            .field(
                Field::rows(
                    "boundary",
                    "縁",
                    vec![
                        Field::choice(
                            "source",
                            "種類",
                            &[
                                ("region", "スケッチの領域"),
                                ("wire", "3D スケッチの線"),
                                ("edge", "ボディの辺"),
                            ],
                        ),
                        Field::output("profile", "スケッチ", "profile")
                            .only("@source", "region", "region"),
                        Field::text("key", "領域の名前")
                            .only("@source", "region", "region")
                            .help("空なら輪郭の全部。作る時にクリックした領域の名前を引き継ぐ"),
                        Field::output("sketch", "3D スケッチ", "lines")
                            .only("@source", "wire", "region"),
                        Field::bool("whole","スケッチ全体").only("@source","wire","region").help("線1本のスケッチの出力を丸ごと使う。複数の曲線はスケッチを分ける"),
                        Field::opt_integer("line", "線分のID")
                            .unitless().only("@source", "wire", "region").and_only("@whole","false","false")
                            .help("スケッチ内の線分のID。出力の並び順とは別で、同じIDの線分を追う。曲線は線1本のスケッチ全体を選ぶ"),
                        Field::pick("edge", "辺", &[kmb_core::RefKind::Edge], 1, Some(1))
                            .only("@source", "edge", "region"),
                    ],
                )
                .help("スケッチの領域 1 つか、線・辺で囲む 1 つの輪"),
            ),
        |d| Ok(Box::new(parse::<SurfacePatch>(d)?)),
    );
}

struct Commands;
impl Extension for Commands {
    fn actions(&self, _: &Context<'_>) -> Vec<Action> {
        vec![Action::new(PATCH,"面を張る","cad")
        .arg(Arg::list("boundary","縁の並び。source は region・wire・edge。領域 {profile,key}、3D の線分 {sketch,line}(線分のID)、線1本のスケッチ全体 {sketch,whole:true}、辺 {edge:{kind:edge,feature,faces}}").required())
        .menu("作成").icon("body").hint("閉じた縁をクリックで選び、Enter で厚みの無い面を張る").at(&[Place::Menu])]
    }
    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, kmb_core::Problem>> {
        if id != PATCH {
            return None;
        }
        Some((|| {
            let refs = cx.arg("boundary").ok_or("縁を選んでください")?;
            let mut patch: SurfacePatch =
                serde_json::from_value(json!({"boundary":refs})).map_err(|e| e.to_string())?;
            patch.normalize(cx)?;
            patch
                .resolve(cx.kernel.as_ref(), |feature, slot| {
                    cx.output(feature, slot).cloned().ok_or_else(|| {
                        invalid(format!(
                            "縁の参照先 {feature} の {slot} がありません。縁を選び直してください"
                        ))
                    })
                })
                .map_err(|e| kmb_core::Problem::new(kmb_core::ProblemKind::Input, e.to_string()))?;
            Ok(Outcome::insert(
                "面を張る",
                SURFACE_PATCH,
                format!("面を張る{}", cx.count_of(SURFACE_PATCH) + 1),
                json!(patch),
            ))
        })())
    }
    fn handles(&self, id: &str, cx: &Context<'_>) -> Option<Vec<Handle>> {
        if id != PATCH {
            return None;
        }
        let mut choices = kmb_cad::regions::region_choices(cx, &kmb_cad::regions::candidates(cx));
        for choice in &mut choices {
            choice.group = 0;
            choice.value["source"] = json!("region");
        }
        for f in cx.candidates("lines").into_iter().map(FeatureId) {
            let Some(wires) = cx.output(f, "lines").and_then(Wires::from_value) else {
                continue;
            };
            let name = cx
                .data
                .feature(f)
                .map_or_else(|| f.to_string(), |e| e.name.clone());
            let known = kmb_interchange::segments_now(cx, f).unwrap_or_default();
            for segment in known
                .0
                .iter()
                .filter(|s| wires.0.iter().any(|w| same_segment(w, s)))
            {
                choices.push(HandleChoice {
                    value: json!({"source":"wire","sketch":f,"line":segment.line}),
                    label: format!("{name} の線 {}", segment.line),
                    outline: vec![segment.ends.to_vec()],
                    ..Default::default()
                });
            }
            let main: Vec<_> = wires.0.iter().filter(|w| !w.construction).collect();
            if main
                .iter()
                .any(|w| !known.0.iter().any(|s| same_segment(w, s)))
            {
                choices.push(HandleChoice {
                    value: json!({"source":"wire","sketch":f,"whole":true}),
                    label: format!("{name} の線すべて"),
                    outline: main.iter().map(|w| w.points.clone()).collect(),
                    ..Default::default()
                });
            }
        }
        for (f, h) in cx.visible_bodies() {
            let mut edges = cx.kernel.edges(h).unwrap_or_default();
            edges.extend(cx.kernel.arc_edges(h).unwrap_or_default());
            edges.extend(cx.kernel.curve_edges(h).unwrap_or_default());
            for edge in edges {
                choices.push(HandleChoice{value:json!({"source":"edge","edge":{"kind":"edge","feature":f,"faces":edge.faces}}),label:edge.faces.join(" / "),outline:vec![edge.polyline(64)],..Default::default()});
            }
        }
        Some(vec![Handle {
            choices,
            ..Handle::new(
                "boundary",
                "縁をクリックで選ぶ(Ctrl で続けて選び、Enter で面を張る)",
                Gesture::Regions,
            )
        }])
    }
}
pub fn module() -> kmb_core::Module {
    kmb_core::Module::new("kmb-cad-surface", register).with_extension(Commands)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_boundary_sources_preserve_their_references_and_declare_inputs() {
        for (data, slot) in [
            (json!({"source":"region","profile":2}), "profile"),
            (
                json!({"source":"region","profile":2,"key":"e1+e2"}),
                "faces",
            ),
            (json!({"source":"wire","sketch":2,"line":3}), "lines"),
            (
                json!({"source":"edge","edge":{"kind":"edge","feature":2,"faces":["f1:side:a","f1:end"]}}),
                "body",
            ),
        ] {
            let patch: SurfacePatch = serde_json::from_value(json!({"boundary":[data]})).unwrap();
            let expected = if slot == "lines" {
                vec![
                    output(FeatureId(2), "lines"),
                    output(FeatureId(2), SEGMENTS_OUTPUT),
                ]
            } else {
                vec![output(FeatureId(2), slot)]
            };
            assert_eq!(patch.inputs().unwrap(), expected);
            assert_eq!(
                serde_json::from_value::<SurfacePatch>(json!(patch)).unwrap(),
                patch
            );
        }
    }
    #[test]
    fn one_editable_boundary_field_uses_existing_reference_fields() {
        let mut reg = Registry::new();
        register(&mut reg);
        let d = reg.descriptor(SURFACE_PATCH).unwrap();
        assert_eq!(d.fields.len(), 1);
        assert!(matches!(d.fields[0].kind, kmb_core::FieldKind::Rows { .. }));
        let v = json!({"boundary":[{"source":"wire","sketch":2,"line":3}]});
        assert!(d.field_at("boundary.0.line").unwrap().applies(&v));
    }
    #[test]
    fn creation_action_uses_one_declared_boundary_argument_for_every_source() {
        struct BoundaryKernel;
        let points = vec![
            [0., 0., 0.],
            [10., 0., 0.],
            [10., 10., 0.],
            [0., 10., 0.],
            [0., 0., 0.],
        ];
        impl kmb_core::Kernel for BoundaryKernel {
            fn name(&self) -> &str {
                "boundary-test"
            }
            fn edges(
                &self,
                _: kmb_core::ShapeHandle,
            ) -> Result<Vec<kmb_core::EdgeInfo>, kmb_core::KernelError> {
                Ok(vec![])
            }
            fn curve_edges(
                &self,
                _: kmb_core::ShapeHandle,
            ) -> Result<Vec<kmb_core::EdgeInfo>, kmb_core::KernelError> {
                Ok(vec![kmb_core::EdgeInfo {
                    faces: ["a".into(), "b".into()],
                    a: [0.; 3],
                    b: [0.; 3],
                    curve: vec![
                        [0., 0., 0.],
                        [10., 0., 0.],
                        [10., 10., 0.],
                        [0., 10., 0.],
                        [0., 0., 0.],
                    ],
                    arc: None,
                    convex: false,
                    seam: kmb_core::Seam::Unknown,
                }])
            }
        }
        let modules =
            kmb_core::Modules::assemble(vec![module()], std::sync::Arc::new(BoundaryKernel));
        let profile = k::Profile {
            plane: k::Plane::XY,
            regions: vec![k::Region {
                outer: k::ProfileLoop {
                    edges: (0..4)
                        .map(|i| k::ProfileEdge {
                            name: format!("e{i}"),
                            segment: k::Segment::Line {
                                a: [points[i][0], points[i][1]],
                                b: [points[i + 1][0], points[i + 1][1]],
                            },
                            reversed: false,
                        })
                        .collect(),
                },
                holes: vec![],
            }],
        };
        let wires = Wires(
            (0..4)
                .map(|i| kmb_core::Wire {
                    points: vec![points[i], points[i + 1]],
                    construction: false,
                })
                .collect(),
        );
        let line_ids = Segments(
            (0..4)
                .map(|i| kmb_interchange::Segment {
                    line: i as u32 + 1,
                    ends: [points[i], points[i + 1]],
                })
                .collect(),
        );
        let doc = kmb_core::Document::new();
        let mut report = kmb_core::RecomputeReport::default();
        report.features.push(kmb_core::FeatureResult {
            id: FeatureId(1),
            name: "縁".into(),
            status: kmb_core::FeatureStatus::Ok {
                outputs: [
                    ("profile".into(), profile.to_value()),
                    ("lines".into(), wires.to_value()),
                    (SEGMENTS_OUTPUT.into(), line_ids.to_value()),
                    ("body".into(), Value::Shape(kmb_core::ShapeHandle(1))),
                ]
                .into(),
                reused: false,
            },
        });
        for boundary in [
            vec![json!({"source":"region","profile":1})],
            (1..=4)
                .map(|line| json!({"source":"wire","sketch":1,"line":line}))
                .collect(),
            vec![json!({"source":"edge","edge":{"kind":"edge","feature":1,"faces":["a","b"]}})],
        ] {
            let args = json!({"boundary":boundary});
            let cx = Context {
                args: &args,
                ..Context::new(doc.data(), &report, &modules.registry, &modules.kernel)
            };
            let Outcome::Change { edits, .. } = modules.extensions.run(PATCH, &cx).unwrap() else {
                panic!("機能を足す");
            };
            let kmb_core::Edit::Insert { data, .. } = &edits[0] else {
                panic!("挿入の値");
            };
            assert_eq!(data["boundary"], args["boundary"]);
        }
        for (wire, selection, expected) in [
            (
                kmb_core::Wire {
                    points: points[..2].to_vec(),
                    construction: false,
                },
                json!({"source":"wire","sketch":1,"line":1}),
                "つながっていません",
            ),
            (
                kmb_core::Wire {
                    points: vec![points[0], points[2], points[3], points[1], points[0]],
                    construction: false,
                },
                json!({"source":"wire","sketch":1,"whole":true}),
                "交差",
            ),
        ] {
            let mut invalid_report = report.clone();
            let kmb_core::FeatureStatus::Ok { outputs, .. } =
                &mut invalid_report.features[0].status
            else {
                unreachable!()
            };
            outputs.insert("lines".into(), Wires(vec![wire]).to_value());
            let args = json!({"boundary":[selection]});
            let cx = Context {
                args: &args,
                ..Context::new(
                    doc.data(),
                    &invalid_report,
                    &modules.registry,
                    &modules.kernel,
                )
            };
            let error = modules.extensions.run(PATCH, &cx).unwrap_err();
            assert_eq!(error.kind, kmb_core::ProblemKind::Input);
            assert!(error.message.contains(expected), "{error}");
        }
        let mut curve_report = report.clone();
        let kmb_core::FeatureStatus::Ok { outputs, .. } = &mut curve_report.features[0].status
        else {
            unreachable!()
        };
        outputs.insert(
            "lines".into(),
            Wires(vec![kmb_core::Wire {
                points: points.clone(),
                construction: false,
            }])
            .to_value(),
        );
        let ordinal = json!({"boundary":[{"source":"wire","sketch":1,"wire":0}]});
        let cx = Context {
            args: &ordinal,
            ..Context::new(
                doc.data(),
                &curve_report,
                &modules.registry,
                &modules.kernel,
            )
        };
        assert!(
            modules
                .extensions
                .run(PATCH, &cx)
                .unwrap_err()
                .message
                .contains("曲線は出力順の番号で追跡できません")
        );
        let whole = json!({"boundary":[{"source":"wire","sketch":1,"whole":true}]});
        let cx = Context {
            args: &whole,
            ..Context::new(
                doc.data(),
                &curve_report,
                &modules.registry,
                &modules.kernel,
            )
        };
        assert!(matches!(
            modules.extensions.run(PATCH, &cx),
            Ok(Outcome::Change { .. })
        ));
        let kmb_core::FeatureStatus::Ok { outputs, .. } = &mut curve_report.features[0].status
        else {
            unreachable!()
        };
        outputs.insert(
            "lines".into(),
            Wires(vec![
                kmb_core::Wire {
                    points: points.clone(),
                    construction: false
                };
                2
            ])
            .to_value(),
        );
        let cx = Context {
            args: &whole,
            ..Context::new(
                doc.data(),
                &curve_report,
                &modules.registry,
                &modules.kernel,
            )
        };
        assert!(
            modules
                .extensions
                .run(PATCH, &cx)
                .unwrap_err()
                .message
                .contains("線が1本")
        );
        let args = json!({"boundary":[],"typo":1});
        let cx = Context {
            args: &args,
            ..Context::new(doc.data(), &report, &modules.registry, &modules.kernel)
        };
        assert_eq!(
            modules.extensions.run(PATCH, &cx).unwrap_err().kind,
            kmb_core::ProblemKind::Input
        );
    }
    #[test]
    fn boundary_fields_reject_unknown_keys_and_invalid_wire_numbers() {
        for boundary in [
            json!({"source":"wire","sketch":2,"wire":-1}),
            json!({"source":"region","profile":2,"typo":1}),
            json!({"source":"edge","edge":2}),
        ] {
            assert!(
                serde_json::from_value::<SurfacePatch>(json!({"boundary":[boundary]})).is_err()
            );
        }
        let switched =
            json!({"boundary":[{"source":"wire","profile":2,"key":"unused","sketch":3,"line":5}]});
        assert_eq!(
            serde_json::from_value::<SurfacePatch>(switched)
                .unwrap()
                .inputs()
                .unwrap(),
            [
                output(FeatureId(3), "lines"),
                output(FeatureId(3), SEGMENTS_OUTPUT)
            ]
        );
    }
    fn line(name: &str, a: P3, b: P3) -> (BoundarySegment, Vec<P3>) {
        (
            BoundarySegment {
                name: name.into(),
                curve: BoundaryCurve::Line { a, b },
            },
            vec![a, b],
        )
    }
    #[test]
    fn invalid_boundary_points_name_the_segment_and_location_to_repair() {
        let error = ordered(vec![line("broken", [0.; 3], [0.; 3])])
            .unwrap_err()
            .to_string();
        assert!(error.contains("broken"), "{error}");
        assert!(error.contains("1") && error.contains("2"), "{error}");
        assert!(
            error.contains("長さ 0") && error.contains("離してください"),
            "{error}"
        );
        let error = ordered(vec![line("nonfinite", [f64::NAN, 0., 0.], [1.; 3])])
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("nonfinite") && error.contains("点 1"),
            "{error}"
        );
        assert!(error.contains("有限"), "{error}");
    }

    #[test]
    fn shuffled_reversed_lines_form_one_loop_but_two_loops_and_forks_are_rejected() {
        let p = [[0., 0., 0.], [1., 0., 0.], [1., 1., 1.], [0., 1., 0.]];
        let lines = vec![
            line("a", p[0], p[1]),
            line("c", p[3], p[2]),
            line("b", p[2], p[1]),
            line("d", p[0], p[3]),
        ];
        assert_eq!(
            ordered(lines)
                .unwrap()
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            ["a", "b", "c", "d"]
        );
        let two = vec![
            line("a", p[0], p[1]),
            line("b", p[1], p[0]),
            line("c", p[2], p[3]),
            line("d", p[3], p[2]),
        ];
        assert!(ordered(two).is_err());
        let fork = vec![
            line("a", p[0], p[1]),
            line("b", p[1], p[2]),
            line("c", p[1], p[3]),
        ];
        assert!(ordered(fork).is_err());
    }
}
