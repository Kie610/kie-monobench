//! 確かめること: 矛盾の説明、遠くへ動く投影、接点を指定する拘束、距離ゼロの自由度。
use super::*;
use crate::{ProjectionSource, RefItem, RefShape, SketchCommands, refresh_projections};
use kmb_core::{Document, Extension, FeatureEntry, FeatureStatus, NullKernel, Op, Transaction};

#[test]
fn an_inconsistent_pattern_explains_constraint_kinds_counts_and_pattern_size() {
    let (mut s, _) = super::speed_tests::pattern(5, true);
    let line = super::speed_tests::lines_of(&s)[0];
    s.add_constraint(ConstraintKind::Length {
        line,
        value: "12 mm".into(),
    });
    let mut doc = Document::new();
    let id = doc.alloc_feature_id();
    doc.apply(Transaction::new(
        "スケッチ",
        vec![Op::InsertFeature {
            index: None,
            entry: FeatureEntry {
                id,
                type_id: crate::SKETCH.into(),
                name: "パターン".into(),
                data: serde_json::to_value(&s).unwrap(),
                suppressed: false,
                pin_bodies: false,
            },
        }],
    ))
    .unwrap();
    let report = doc.recompute(&crate::tests::registry(), &NullKernel);
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(id) else {
        panic!("スケッチの解を出す");
    };
    let solution = crate::solution_of(&outputs["sketch"]).unwrap();
    assert!(matches!(solution.status, SolveStatus::Inconsistent { .. }));
    let message = SketchCommands::default()
        .describe(&outputs["sketch"])
        .unwrap();
    assert!(
        message.contains("長さ") && message.contains("件"),
        "{message}"
    );
    assert!(
        message.contains("5×5") && message.contains("25"),
        "{message}"
    );
    // 求め直した出力だけを変え、保存する機能のデータは変えない。
    assert_eq!(
        doc.data().feature(id).unwrap().data,
        serde_json::to_value(&s).unwrap()
    );
}

#[test]
fn a_sketch_follows_a_projected_edge_that_moves_far_away() {
    let mut s = Sketch::default();
    let source = ProjectionSource::Edge {
        feature: kmb_core::FeatureId(1),
        faces: ["top".into(), "side".into()],
    };
    let line = s.add_projection(&RefItem {
        source: source.clone(),
        shape: RefShape::Line([0.0, 0.0], [20.0, 0.0]),
    });
    let Some(Entity::Line { a, b, .. }) = s.entity(line).cloned() else {
        panic!("投影の線");
    };
    let p = s.add_point(0.0, 10.0);
    let q = s.add_point(20.0, 10.0);
    let left = s.add_line(a, p);
    let top = s.add_line(p, q);
    let right = s.add_line(b, q);
    s.add_constraint(ConstraintKind::Vertical { line: left });
    s.add_constraint(ConstraintKind::Vertical { line: right });
    s.add_constraint(ConstraintKind::Horizontal { line: top });
    let height = s.add_constraint(ConstraintKind::Length {
        line: left,
        value: "10 mm".into(),
    });
    let values = [(height, 10.0)].into();
    let first = solve(&s, &values).unwrap();
    assert_eq!(first.status, SolveStatus::WellConstrained);
    s.adopt_solution(&first);
    for offset in [[1200.0, 800.0], [-2400.0, -1600.0]] {
        let refs = [RefItem {
            source: source.clone(),
            shape: RefShape::Line(offset, [offset[0] + 20.0, offset[1]]),
        }];
        assert!(refresh_projections(&mut s, &refs).is_empty());
        let solution = solve(&s, &values).unwrap();
        assert_eq!(
            solution.status,
            SolveStatus::WellConstrained,
            "{solution:?}"
        );
        for (id, expected) in [
            (p, [offset[0], offset[1] + 10.0]),
            (q, [offset[0] + 20.0, offset[1] + 10.0]),
        ] {
            assert!((solution.points[&id][0] - expected[0]).abs() < 1e-6);
            assert!(
                (solution.points[&id][1] - expected[1]).abs() < 1e-6,
                "{solution:?}"
            );
        }
        s.adopt_solution(&solution);
    }
}

#[test]
fn a_tangent_contact_stays_at_a_point_fixed_by_other_constraints() {
    for contact_kind in ["curve", "midpoint", "coincident"] {
        let mut s = Sketch::default();
        let ellipse = s
            .add_ellipse((None, [0.0, 0.0]), (None, [30.0, 0.0]), [0.0, 12.0])
            .unwrap();
        let Some(Entity::Ellipse {
            center,
            major,
            minor,
            ..
        }) = s.entity(ellipse).cloned()
        else {
            panic!("楕円");
        };
        for point in [center, major, minor] {
            s.add_constraint(ConstraintKind::Fix { point });
        }
        let contact = s.add_point(0.0, 12.0);
        s.add_constraint(ConstraintKind::Fix { point: contact });
        s.add_constraint(ConstraintKind::PointOnCurve {
            point: contact,
            curve: ellipse,
        });
        let a = s.add_point(
            if contact_kind == "coincident" {
                0.0
            } else {
                -30.0
            },
            12.0,
        );
        let b = s.add_point(30.0, 11.8);
        let line = s.add_line(a, b);
        match contact_kind {
            "midpoint" => {
                s.add_constraint(ConstraintKind::Fix { point: a });
                s.add_constraint(ConstraintKind::Midpoint {
                    point: contact,
                    line,
                });
            }
            "coincident" => {
                s.add_constraint(ConstraintKind::Coincident { a, b: contact });
            }
            _ => {
                s.add_constraint(ConstraintKind::Fix { point: a });
                s.add_constraint(ConstraintKind::PointOnCurve {
                    point: contact,
                    curve: line,
                });
            }
        }
        s.add_constraint(ConstraintKind::TangentCurve {
            a: line,
            b: ellipse,
        });
        let solution = solve(&s, &Default::default()).unwrap();
        assert!(
            !matches!(solution.status, SolveStatus::Inconsistent { .. }),
            "{contact_kind}: {solution:?}"
        );
        assert!(solution.max_residual < 1e-7, "{contact_kind}: {solution:?}");
        assert!((solution.points[&a][1] - 12.0).abs() < 1e-6);
        assert!((solution.points[&b][1] - 12.0).abs() < 1e-6);
    }
}

#[test]
fn a_zero_distance_fixes_both_coordinates_without_a_coincident_constraint() {
    for initial in [[0.0, 0.0], [3.0, 4.0]] {
        let mut s = Sketch::default();
        let a = s.add_point(0.0, 0.0);
        let b = s.add_point(initial[0], initial[1]);
        s.add_constraint(ConstraintKind::Fix { point: a });
        let id = s.add_constraint(ConstraintKind::Distance {
            a,
            b,
            value: "0 mm".into(),
        });
        let solution = solve(&s, &[(id, 0.0)].into()).unwrap();
        assert_eq!(
            solution.status,
            SolveStatus::WellConstrained,
            "{solution:?}"
        );
        assert_eq!(solution.dof, 0);
        assert!(solution.fixed.contains(&b));
        assert!(solution.points[&b][0].hypot(solution.points[&b][1]) < 1e-7);
        let json = serde_json::to_string(&s).unwrap();
        let back: Sketch = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
        assert!(matches!(
            back.constraints.last().unwrap().kind,
            ConstraintKind::Distance { .. }
        ));
    }
}
