//! 既存 Sweep による腕の断面。新しいカーネル演算は増やさない。
use super::{Alignment, GearBody, Side, SpokeSection};
use crate::{hub::Parts, tooth};
use kmb_cad::{Operation, Recipe};
use kmb_core::{EvalError, Plane, Profile, ProfileLoop, Region, Segment};
use std::f64::consts::TAU;

fn section(kind: SpokeSection, width: f64, height: f64, plane: Plane) -> Profile {
    let mut edges = Vec::new();
    if kind == SpokeSection::Ellipse {
        for quarter in 0..4 {
            let f = |t: f64| {
                let a = t * TAU;
                let v = [-width * a.sin(), height * a.cos()];
                let n = v[0].hypot(v[1]);
                (
                    [width * a.cos() / 2.0, height * a.sin() / 2.0],
                    [v[0] / n, v[1] / n],
                )
            };
            let lo = quarter as f64 / 4.0;
            let hi = (quarter + 1) as f64 / 4.0;
            let mut from = f(lo).0;
            for (i, s) in tooth::fit_biarcs(&f, lo, hi, tooth::tolerance(width.min(height)))
                .into_iter()
                .enumerate()
            {
                let e = tooth::edge(format!("spoke:ellipse:{quarter}:{i}"), s, from);
                from = e.to_point();
                edges.push(e);
            }
        }
    } else {
        let x = width / 2.0;
        let y = height / 2.0;
        let a = x / 3.0;
        let b = y / 3.0;
        let p = [
            [-a, -y],
            [a, -y],
            [a, -b],
            [x, -b],
            [x, b],
            [a, b],
            [a, y],
            [-a, y],
            [-a, b],
            [-x, b],
            [-x, -b],
            [-a, -b],
        ];
        for i in 0..p.len() {
            edges.push(tooth::edge(
                format!("spoke:cross:{i}"),
                Segment::Line {
                    a: p[i],
                    b: p[(i + 1) % p.len()],
                },
                p[i],
            ));
        }
    }
    Profile {
        plane,
        regions: vec![Region {
            outer: ProfileLoop { edges },
            holes: vec![],
        }],
    }
}

pub(super) fn recipe(
    body: &GearBody,
    parts: &Parts,
    _width: f64,
    web: f64,
    boss: f64,
    from: f64,
    to: f64,
) -> Result<Recipe, EvalError> {
    let arms = parts.spokes.as_ref().ok_or_else(|| {
        EvalError::InvalidData(
            "楕円と十字の断面には腕のある歯車の輪郭が必要です。円板には長方形を選んでください"
                .into(),
        )
    })?;
    // Many applies Intersect successively; that is not target ∩ union(rim,boss,arms).
    if body.operation == Operation::Intersect {
        return Err(EvalError::InvalidData(
            "楕円と十字の腕の交差には対応していません。新しいボディとして作ってください".into(),
        ));
    }
    let span = |thickness: f64| match body.alignment {
        Alignment::Centered => {
            let middle = (from + to) / 2.0;
            (middle - thickness / 2.0, middle + thickness / 2.0)
        }
        Alignment::Start if body.direction == Side::Reverse => (to - thickness, to),
        Alignment::Start => (from, from + thickness),
    };
    let extrude = |profile, from, to| Recipe::Extrude {
        operation: body.operation,
        profile,
        from,
        to,
        through: false,
        stop: None,
        taper: 0.0,
    };
    let mut recipes = vec![extrude(parts.rim.clone(), from, to)];
    if let Some(profile) = &parts.boss {
        let (a, b) = span(boss);
        recipes.push(extrude(profile.clone(), a, b));
    }
    let (a, b) = span(web);
    let middle = (a + b) / 2.0;
    for original in &arms.paths {
        let mut path = original.clone();
        path.plane = path.plane.offset(middle);
        let first = path
            .edges
            .first()
            .ok_or_else(|| EvalError::InvalidData("腕の中心線がありません".into()))?;
        let angle = first.leave_angle();
        let tangent = [angle.cos(), angle.sin()];
        let normal = path.plane.normal();
        let plane = Plane {
            origin: path.plane.to_world(first.from_point()),
            x_axis: [0, 1, 2]
                .map(|i| -tangent[1] * path.plane.x_axis[i] + tangent[0] * path.plane.y_axis[i]),
            y_axis: normal,
        };
        recipes.push(Recipe::Sweep {
            operation: body.operation,
            profile: section(body.spoke_section, arms.root_width, web, plane),
            path,
            twist: 0.0,
            scale: arms.tip_width / arms.root_width,
        });
    }
    if let Some(pads) = &arms.root_pads {
        recipes.push(extrude(pads.clone(), a, b));
    }
    Ok(Recipe::Many { parts: recipes })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kmb_core::{FeatureId, ProfileEdge};
    fn parts() -> Parts {
        let outer = ProfileLoop {
            edges: vec![ProfileEdge {
                curve: None,
                name: "outside".into(),
                segment: Segment::Circle {
                    center: [0.0; 2],
                    radius: 24.0,
                },
                reversed: false,
            }],
        };
        crate::hub::spokes::layout(
            outer,
            21.5,
            2.0,
            19.5,
            8.0,
            None,
            &crate::hub::spokes::Params {
                count: 5,
                root_width: 2.0,
                tip_width: 1.5,
                angle: 0.3,
                round: 0.5,
            },
        )
        .unwrap()
        .parts
    }
    #[test]
    fn ellipse_and_cross_sections_keep_area_scale_plane_and_all_operation_parts() {
        for kind in [SpokeSection::Ellipse, SpokeSection::Cross] {
            let profile = section(kind, 2.0, 3.0, Plane::XY);
            let expected = if kind == SpokeSection::Ellipse {
                std::f64::consts::PI * 1.5
            } else {
                10.0 / 3.0
            };
            assert!((profile.regions[0].outer.signed_area() - expected).abs() < 0.01);
            for operation in [Operation::New, Operation::Join, Operation::Cut] {
                let body = GearBody {
                    spoke_section: kind,
                    operation,
                    ..GearBody::new(FeatureId(1))
                };
                let Recipe::Many { parts } =
                    recipe(&body, &parts(), 6.0, 3.0, 10.0, 0.0, 6.0).unwrap()
                else {
                    panic!("many")
                };
                assert_eq!(parts.len(), 8, "rim, boss, 5 sweeps, root reinforcement");
                for p in parts {
                    assert_eq!(p.operation(), operation);
                    if let Recipe::Sweep {
                        profile,
                        path,
                        scale,
                        twist,
                        ..
                    } = p
                    {
                        assert_eq!(scale, 0.75);
                        assert_eq!(twist, 0.0);
                        let a = path.plane.to_world(path.edges[0].from_point());
                        assert!(
                            a.into_iter()
                                .zip(profile.plane.origin)
                                .all(|(a, b)| (a - b).abs() < 1e-10)
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn unsupported_compound_intersection_and_non_spoke_sections_are_reasoned_errors() {
        let body = GearBody {
            spoke_section: SpokeSection::Ellipse,
            operation: Operation::Intersect,
            ..GearBody::new(FeatureId(1))
        };
        assert!(
            matches!(recipe(&body,&parts(),6.0,3.0,10.0,0.0,6.0),Err(EvalError::InvalidData(s)) if s.contains("新しいボディ"))
        );
        let mut disk = parts();
        disk.spokes = None;
        let body = GearBody {
            operation: Operation::New,
            ..body
        };
        assert!(
            matches!(recipe(&body,&disk,6.0,3.0,10.0,0.0,6.0),Err(EvalError::InvalidData(s)) if s.contains("腕のある"))
        );
    }
}
