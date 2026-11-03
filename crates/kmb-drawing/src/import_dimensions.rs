//! スケッチの寸法を、見えている名前付きの辺の端・中心へ一回だけ対応付ける。

use crate::dimension::{self as dim, ViewGeometry};
use crate::*;
use kmb_core::kernel::{P3, dot};
use kmb_core::{FeatureId, Plane};
use kmb_interchange::drawing::{LineKey, ViewLine};
use kmb_sketch::{ConstraintKind as K, Entity, EntityId, Sketch, Solution};
use std::collections::BTreeMap;

const TOL: f64 = 1e-5;

struct Candidate {
    reference: DimRef,
    world: P3,
    paper: [f64; 2],
    radius: Option<f64>,
}
fn visible_point(line: &ViewLine, p: [f64; 2], tolerance: f64) -> bool {
    if let kmb_interchange::drawing::Path::Arc {
        c,
        r,
        start_deg,
        sweep_deg,
    } = &line.path
    {
        let q = dim::sub(p, *c);
        if (dim::norm(q) - r).abs() > tolerance {
            return false;
        }
        let angle = q[1].atan2(q[0]).to_degrees();
        let delta = if *sweep_deg >= 0.0 {
            (angle - start_deg).rem_euclid(360.0)
        } else {
            (start_deg - angle).rem_euclid(360.0)
        };
        return sweep_deg.abs() >= 360.0 - 1e-6 || delta <= sweep_deg.abs() + 1e-6;
    }
    let points = crate::geometry::path_points(&line.path, 0.001);
    points.windows(2).any(|w| {
        let d = dim::sub(w[1], w[0]);
        let l = dim::dot2(d, d);
        let t = if l > 0.0 {
            (dim::dot2(dim::sub(p, w[0]), d) / l).clamp(0.0, 1.0)
        } else {
            0.0
        };
        dim::norm(dim::sub(p, dim::add(w[0], dim::scale(d, t)))) <= tolerance
    })
}
fn candidates(view: &str, space: &ViewGeometry) -> Vec<Candidate> {
    let mut out = Vec::new();
    for (key, edge) in &space.edges {
        let lines=space.lines.iter().filter(|line|line.visible && matches!(&line.key,LineKey::Edge {key:other} if space.canonical_key(other)==*key)).collect::<Vec<_>>();
        if lines.is_empty() {
            continue;
        }
        for (at, world) in [(RefAt::A, edge.a), (RefAt::B, edge.b)] {
            let paper = space.point(world).0;
            if lines
                .iter()
                .any(|l| visible_point(l, paper, TOL * space.scale))
            {
                out.push(Candidate {
                    reference: DimRef {
                        view: view.into(),
                        key: key.clone(),
                        at,
                    },
                    world,
                    paper,
                    radius: None,
                });
            }
        }
        if let Some(a) = edge.arc {
            out.push(Candidate {
                reference: DimRef {
                    view: view.into(),
                    key: key.clone(),
                    at: RefAt::Center,
                },
                world: a.center,
                paper: space.point(a.center).0,
                radius: Some(a.radius),
            });
        }
    }
    out
}
fn same_world(a: P3, b: P3) -> bool {
    a.into_iter()
        .zip(b)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f64>()
        <= TOL * TOL
}
fn pick_point(
    point: [f64; 2],
    radius: Option<f64>,
    candidates: &[Candidate],
    scale: f64,
    world: P3,
) -> Result<DimRef, String> {
    let mut matches = candidates
        .iter()
        .filter(|c| {
            dim::norm(dim::sub(c.paper, point)) <= TOL * scale
                && radius.is_none_or(|r| c.radius.is_some_and(|q| (r - q).abs() <= TOL))
        })
        .collect::<Vec<_>>();
    // 投影が同じでも、元の位置と一致する物があればその参照だけが対応する。
    if matches.iter().any(|c| same_world(c.world, world)) {
        matches.retain(|c| same_world(c.world, world));
    }
    let first = matches
        .first()
        .ok_or("見えている辺の端か中心に重なる参照がありません")?;
    if matches.iter().any(|c| {
        c.reference.key.body != first.reference.key.body || !same_world(c.world, first.world)
    }) {
        return Err("投影で重なる参照が複数あり、決められません。ビューを分けてください".into());
    }
    Ok(first.reference.clone())
}
fn point(sol: &Solution, id: EntityId) -> Result<[f64; 2], String> {
    sol.points
        .get(&id)
        .copied()
        .ok_or_else(|| format!("スケッチの点 {} がありません", id.0))
}
fn line(sketch: &Sketch, sol: &Solution, id: EntityId) -> Result<[[f64; 2]; 2], String> {
    match sketch.entities.iter().find(|e| e.id() == id) {
        Some(Entity::Line { a, b, .. }) => Ok([point(sol, *a)?, point(sol, *b)?]),
        _ => Err("寸法の元の線分がありません".into()),
    }
}
fn line_ref(view: &str, space: &ViewGeometry, world: [P3; 2]) -> Result<DimRef, String> {
    let points = world.map(|p| space.point(p).0);
    let mut matches = Vec::new();
    for (key, e) in &space.edges {
        if e.arc.is_some() || !e.curve.is_empty() {
            continue;
        }
        let (a, b) = (space.point(e.a).0, space.point(e.b).0);
        let close = |a, b| dim::norm(dim::sub(a, b)) <= TOL * space.scale;
        if !((close(a, points[0]) && close(b, points[1]))
            || (close(a, points[1]) && close(b, points[0])))
        {
            continue;
        }
        let visible = |point| {
            space.lines.iter().any(|l| {
            l.visible
                && matches!(&l.key,LineKey::Edge {key:other} if space.canonical_key(other)==*key)
                && visible_point(l, point, TOL * space.scale)
        })
        };
        if !visible(a) || !visible(b) {
            continue;
        }
        matches.push((key, e));
    }
    let exact = |e: &kmb_core::kernel::EdgeInfo| {
        same_world(e.a, world[0]) && same_world(e.b, world[1])
            || same_world(e.a, world[1]) && same_world(e.b, world[0])
    };
    if matches.iter().any(|(_, e)| exact(e)) {
        matches.retain(|(_, e)| exact(e));
    }
    let (key, edge) = matches
        .first()
        .ok_or("スケッチの線の両端が、見えている辺の両端に重なりません")?;
    if matches.iter().any(|(k, e)| {
        k.body != key.body
            || !(same_world(e.a, edge.a) && same_world(e.b, edge.b)
                || same_world(e.a, edge.b) && same_world(e.b, edge.a))
    }) {
        return Err("角度の線の参照が重なり、決められません".into());
    }
    Ok(DimRef {
        view: view.into(),
        key: (*key).clone(),
        at: RefAt::Line,
    })
}

pub(super) struct Imported {
    pub notes: Vec<Annotation>,
    pub skipped: Vec<String>,
}

pub(super) fn from_sketch(
    prefix: &str,
    feature: FeatureId,
    sketch: &Sketch,
    sol: &Solution,
    plane: Plane,
    view: &str,
    space: &ViewGeometry,
) -> Imported {
    let mut result = Imported {
        notes: Vec::new(),
        skipped: Vec::new(),
    };
    if dot(plane.normal(), space.frame.eye).abs() < 1.0 - 1e-6 {
        result.skipped.push(format!(
            "{}: スケッチの面がビューに平行ではありません",
            feature.0
        ));
        return result;
    }
    let candidates = candidates(view, space);
    let paper = |p| space.point(plane.to_world(p)).0;
    let resolve_point = |p, r| pick_point(paper(p), r, &candidates, space.scale, plane.to_world(p));
    for c in &sketch.constraints {
        if c.kind.value().is_none() {
            continue;
        }
        let make = (|| -> Result<Annotation, String> {
            let at = sketch.label_at(c, sol).ok_or(
                "寸法の札の位置が保存されていません。スケッチで寸法の位置を決めてください",
            )?;
            let (kind, refs) = match &c.kind {
                K::Distance { a, b, .. }
                | K::HorizontalDistance { a, b, .. }
                | K::VerticalDistance { a, b, .. } => {
                    let k = match c.kind {
                        K::HorizontalDistance { .. } => DimKind::Horizontal,
                        K::VerticalDistance { .. } => DimKind::Vertical,
                        _ => DimKind::Parallel,
                    };
                    (
                        k,
                        vec![
                            resolve_point(point(sol, *a)?, None)?,
                            resolve_point(point(sol, *b)?, None)?,
                        ],
                    )
                }
                K::Length { line: l, .. } => {
                    let [a, b] = line(sketch, sol, *l)?;
                    (
                        DimKind::Parallel,
                        vec![resolve_point(a, None)?, resolve_point(b, None)?],
                    )
                }
                K::PointLineDistance {
                    point: p, line: l, ..
                } => {
                    let p = point(sol, *p)?;
                    let [a, b] = line(sketch, sol, *l)?;
                    let d = dim::sub(b, a);
                    let den = dim::dot2(d, d);
                    if den < 1e-20 {
                        return Err("長さの無い線分です".into());
                    }
                    let foot = dim::add(a, dim::scale(d, dim::dot2(dim::sub(p, a), d) / den));
                    (
                        DimKind::Parallel,
                        vec![resolve_point(p, None)?, resolve_point(foot, None)?],
                    )
                }
                K::Radius { circle, .. } | K::Diameter { circle, .. } => {
                    let center = match sketch.entities.iter().find(|e| e.id() == *circle) {
                        Some(Entity::Circle { center, .. } | Entity::Arc { center, .. }) => *center,
                        _ => return Err("寸法の元の円がありません".into()),
                    };
                    let radius =
                        kmb_sketch::radius(sketch, sol, *circle).ok_or("円の半径が読めません")?;
                    (
                        if matches!(c.kind, K::Radius { .. }) {
                            DimKind::Radius
                        } else {
                            DimKind::Diameter
                        },
                        vec![resolve_point(point(sol, center)?, Some(radius))?],
                    )
                }
                K::Angle { a, b, .. } => {
                    let a = line(sketch, sol, *a)?.map(|p| plane.to_world(p));
                    let b = line(sketch, sol, *b)?.map(|p| plane.to_world(p));
                    (
                        DimKind::Angle,
                        vec![line_ref(view, space, a)?, line_ref(view, space, b)?],
                    )
                }
                _ => return Err("この寸法は辺の端か中心の参照へ取り込めません".into()),
            };
            let at = paper(at);
            let views = BTreeMap::from([(view.to_string(), space.clone())]);
            let computed = dim::dimension(kind, &refs, at, &views)?.value;
            let expected =
                kmb_sketch::measure(sketch, sol, &c.kind).ok_or("スケッチの寸法を測れません")?;
            let expected = if kind == DimKind::Angle {
                expected.to_degrees()
            } else {
                expected
            };
            if (computed - expected).abs() > TOL * expected.abs().max(1.0) {
                return Err(
                    "このビューで測る値がスケッチの寸法と違います。別のビューを選んでください"
                        .into(),
                );
            }
            Ok(Annotation {
                id: format!("{prefix}-{}-{}", feature.0, c.id.0),
                kind: AnnotationKind::Dimension {
                    dimension: kind,
                    refs,
                    at,
                },
                outline: None,
                rest: Rest::new(),
            })
        })();
        match make {
            Ok(note) => result.notes.push(note),
            Err(e) => result
                .skipped
                .push(format!("{}の寸法{}: {e}", feature.0, c.id.0)),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use kmb_core::kernel::{EdgeInfo, Seam};
    use kmb_interchange::drawing::{EdgeKey, Path};
    #[test]
    fn imports_only_visible_matching_endpoints_and_keeps_the_model_label_position() {
        let sketch:Sketch=serde_json::from_value(serde_json::json!({"entities":[{"type":"point","id":1,"x":0,"y":0},{"type":"point","id":2,"x":20,"y":0},{"type":"line","id":3,"a":1,"b":2}],"constraints":[{"id":1,"type":"length","line":3,"value":"20","at":[10,5]}]})).unwrap();
        let sol = kmb_sketch::solve(
            &sketch,
            &std::collections::HashMap::from([(kmb_sketch::ConstraintId(1), 20.0)]),
        )
        .unwrap();
        let key = EdgeKey {
            body: FeatureId(5),
            faces: ["front".into(), "top".into()],
            index: 0,
        };
        let edge = EdgeInfo {
            a: [0.0, 0.0, 0.0],
            b: [20.0, 0.0, 0.0],
            faces: key.faces.clone(),
            convex: true,
            seam: Seam::Corner,
            arc: None,
            curve: Vec::new(),
        };
        let mut space = ViewGeometry {
            frame: kmb_view_lines::View::named("top").unwrap(),
            at: [50.0, 60.0],
            scale: 0.5,
            edges: BTreeMap::from([(key.clone(), edge)]),
            aliases: BTreeMap::new(),
            bodies: std::collections::BTreeSet::from([FeatureId(5)]),
            lines: vec![ViewLine {
                key: LineKey::Edge { key },
                path: Path::Line {
                    a: [50.0, 60.0],
                    b: [60.0, 60.0],
                },
                visible: true,
            }],
        };
        let imported = from_sketch("d", FeatureId(7), &sketch, &sol, Plane::XY, "v", &space);
        assert!(imported.skipped.is_empty(), "{:?}", imported.skipped);
        assert_eq!(imported.notes.len(), 1);
        assert!(matches!(
            imported.notes[0].kind,
            AnnotationKind::Dimension {
                at: [55.0, 62.5],
                ..
            }
        ));
        space.lines[0].visible = false;
        let hidden = from_sketch("d", FeatureId(7), &sketch, &sol, Plane::XY, "v", &space);
        assert!(hidden.notes.is_empty() && hidden.skipped[0].contains("見えている辺"));
    }
    #[test]
    fn overlapping_projections_require_the_original_position_or_one_unambiguous_reference() {
        let reference = |index| DimRef {
            view: "v".into(),
            key: EdgeKey {
                body: FeatureId(1),
                faces: ["a".into(), "b".into()],
                index,
            },
            at: RefAt::A,
        };
        let candidates = vec![
            Candidate {
                reference: reference(0),
                world: [0.0, 0.0, 0.0],
                paper: [0.0; 2],
                radius: None,
            },
            Candidate {
                reference: reference(1),
                world: [0.0, 0.0, 10.0],
                paper: [0.0; 2],
                radius: None,
            },
        ];
        assert_eq!(
            pick_point([0.0; 2], None, &candidates, 1.0, [0.0; 3])
                .unwrap()
                .key
                .index,
            0
        );
        assert!(
            pick_point([0.0; 2], None, &candidates, 1.0, [0.0, 0.0, 5.0])
                .err()
                .unwrap()
                .contains("複数")
        );
    }
}
