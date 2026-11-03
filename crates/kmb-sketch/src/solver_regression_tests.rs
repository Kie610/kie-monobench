//! 確かめること: 矛盾の説明、遠くへ動く投影、接点を指定する拘束、距離ゼロの自由度。
use super::*;
use crate::{ProjectionSource, RefItem, RefShape, SketchCommands, refresh_projections_with_values};
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
    let commands = SketchCommands::default();
    let message = outputs
        .values()
        .filter_map(|v| commands.describe(v))
        .collect::<Vec<_>>()
        .join("、");
    assert!(
        message.contains("長さ") && message.contains("件"),
        "{message}"
    );
    assert!(
        message.contains("5×5") && message.contains("25"),
        "{message}"
    );
    // MCP の状態の JSON にも、CLI で読む出力と同じ理由を渡す。
    let registry = crate::tests::registry();
    let kernel: Arc<dyn kmb_core::Kernel> = Arc::new(NullKernel);
    let context = kmb_core::Context::new(doc.data(), &report, &registry, &kernel);
    let status = commands.status(&context);
    let diagnoses = status
        .iter()
        .find(|(key, _)| key == "sketch_diagnostics")
        .expect("MCP から矛盾の理由を読める");
    assert_eq!(diagnoses.1[0]["feature"], serde_json::json!(id.0));
    let reason = inconsistency_reason(&s, &solution).unwrap();
    assert_eq!(diagnoses.1[0]["reason"], reason);
    assert!(message.contains(&reason));
    // 初回の寸法名の補完だけを分けて比べ、診断が保存へ混ざらないことを確かめる。
    let first = doc.data().feature(id).unwrap().data.clone();
    let mut without_names = first.clone();
    let names = without_names
        .as_object_mut()
        .unwrap()
        .remove("dim_names")
        .unwrap();
    assert_eq!(names.as_object().unwrap().len(), 3);
    let mut original = serde_json::to_value(&s).unwrap();
    original.as_object_mut().unwrap().remove("dim_names");
    assert_eq!(without_names, original);
    assert!(first.get("diagnosis").is_none() && first.get("reason").is_none());
    doc.recompute(&crate::tests::registry(), &NullKernel);
    assert_eq!(doc.data().feature(id).unwrap().data, first);
}

#[test]
fn a_pattern_diagnostic_includes_circle_radii_in_its_size() {
    use crate::{DistanceType, PatternAxis, PatternDirection, RectangularPattern};
    let mut s = Sketch::default();
    let center = s.add_point(0.0, 0.0);
    let circle = s.add_circle(center, 5.0);
    let direction = |angle, count| PatternDirection {
        axis: PatternAxis::Angle(angle),
        count,
        distance: 20.0,
        distance_type: DistanceType::Spacing,
        symmetric: false,
    };
    s.add_rectangular_pattern(RectangularPattern {
        entities: vec![circle],
        directions: [direction(0.0, 3), direction(std::f64::consts::FRAC_PI_2, 2)],
        suppressed: vec![],
    })
    .unwrap();
    let mut solution = solve(&s, &Default::default()).unwrap();
    solution.status = SolveStatus::Inconsistent {
        conflicting: vec![],
    };
    let reason = inconsistency_reason(&s, &solution).unwrap();
    assert!(reason.contains("矩形 3×2"), "{reason}");
    assert!(reason.contains("範囲 50.000×30.000 mm"), "{reason}");
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
        assert!(
            refresh_projections_with_values(&mut s, &refs, &values)
                .unwrap()
                .is_empty()
        );
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

#[test]
fn an_explicit_contact_uses_hard_relations_and_a_unique_point() {
    let mut s = Sketch::default();
    let center = s.add_point(0.0, 0.0);
    let circle = s.add_circle(center, 5.0);
    let a = s.add_point(-10.0, 5.0);
    let b = s.add_point(10.0, 5.0);
    let line = s.add_line(a, b);
    let p = s.add_point(0.0, 5.0);
    let q = s.add_point(0.0, 5.0);
    let r = s.add_point(0.0, 5.0);
    s.add_constraint(ConstraintKind::PointOnCurve {
        point: p,
        curve: circle,
    });
    let on_line = s.add_constraint(ConstraintKind::Midpoint { point: r, line });
    // 同じ位置だけでは同じ接点とみなさない。
    s.add_constraint(ConstraintKind::TangentCurve { a: line, b: circle });
    assert!(common_contact(&explicit_contacts(&s), line, circle).is_none());
    s.add_constraint(ConstraintKind::Coincident { a: p, b: q });
    s.add_constraint(ConstraintKind::Coincident { a: q, b: r });
    assert_eq!(
        common_contact(&explicit_contacts(&s), line, circle),
        Some(p)
    );
    // 補助線も、硬い拘束が置いた接点なら同じように扱う。
    if let Some(Entity::Line { construction, .. }) = s.entities.iter_mut().find(|e| e.id() == line)
    {
        *construction = true;
    }
    assert_eq!(
        common_contact(&explicit_contacts(&s), line, circle),
        Some(p)
    );
    // 参照の関係は形を決めないので、接点を結び付ける根拠にしない。
    s.constraints
        .iter_mut()
        .find(|c| c.id == on_line)
        .unwrap()
        .reference = true;
    assert!(common_contact(&explicit_contacts(&s), line, circle).is_none());
    s.constraints
        .iter_mut()
        .find(|c| c.id == on_line)
        .unwrap()
        .reference = false;
    let other = s.add_point(3.0, 4.0);
    s.add_constraint(ConstraintKind::PointOnCurve {
        point: other,
        curve: circle,
    });
    s.add_constraint(ConstraintKind::PointOnCurve {
        point: other,
        curve: line,
    });
    assert!(
        common_contact(&explicit_contacts(&s), line, circle).is_none(),
        "共通の点が2つなら推測しない"
    );
}

#[test]
fn moving_a_projection_does_not_carry_unrelated_or_world_fixed_points() {
    for case in ["translation", "fixed", "different"] {
        let mut s = Sketch::default();
        let source = ProjectionSource::Edge {
            feature: kmb_core::FeatureId(1),
            faces: ["top".into(), "side".into()],
        };
        let line = s.add_projection(&RefItem {
            source: source.clone(),
            shape: RefShape::Line([0.0, 0.0], [20.0, 0.0]),
        });
        let Some(Entity::Line { a, .. }) = s.entity(line).cloned() else {
            panic!()
        };
        let p = s.add_point(0.0, 10.0);
        let side = s.add_line(a, p);
        s.add_constraint(ConstraintKind::Vertical { line: side });
        let length = s.add_constraint(ConstraintKind::Length {
            line: side,
            value: "10 mm".into(),
        });
        let values = [(length, 10.0)].into();
        let unrelated = s.add_point(40.0, 50.0);
        if case == "fixed" {
            s.add_constraint(ConstraintKind::Fix { point: p });
        }
        let before = s.clone();
        let refs = [RefItem {
            source,
            shape: if case == "different" {
                RefShape::Line([0.0, 0.0], [1000.0, 0.0])
            } else {
                RefShape::Line([1200.0, 800.0], [1220.0, 800.0])
            },
        }];
        let result = refresh_projections_with_values(&mut s, &refs, &values);
        if case == "fixed" {
            assert!(result.unwrap_err().contains("満たせない拘束"));
            assert_eq!(s, before, "矛盾する時は入力を変えない");
            continue;
        }
        assert!(result.unwrap().is_empty());
        let at = |id| match s.entity(id).unwrap() {
            Entity::Point { x, y, .. } => [*x, *y],
            _ => unreachable!(),
        };
        assert_eq!(at(unrelated), [40.0, 50.0]);
        let expected = if case == "translation" {
            [1200.0, 810.0]
        } else {
            [0.0, 10.0]
        };
        assert!(
            (at(p)[0] - expected[0]).hypot(at(p)[1] - expected[1]) < 1e-6,
            "{case}"
        );
    }
}

#[test]
fn solution_outputs_keep_their_type_and_saved_sketch_data_does_not_gain_diagnostics() {
    use kmb_core::Payload;
    let s = crate::tests::rectangle("10 mm", "6 mm");
    let values = crate::dimension_values(&s, &|_| None).unwrap();
    let solution = solve(&s, &values).unwrap();
    let output = solution.to_value();
    assert!(
        matches!(&output, kmb_core::Value::Custom { type_key, .. } if type_key == "sketch.solution@1")
    );
    let back = crate::solution_of(&output).unwrap();
    assert_eq!(back.status, solution.status);
    assert_eq!(back.dof, solution.dof);
    for (id, p) in &solution.points {
        assert!((p[0] - back.points[id][0]).hypot(p[1] - back.points[id][1]) < 1e-12);
    }
    let json = serde_json::to_value(&s).unwrap();
    assert!(json.get("reason").is_none());
    assert_eq!(serde_json::from_value::<Sketch>(json).unwrap(), s);
}

#[test]
fn different_projected_endpoint_moves_follow_the_same_branch() {
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
        panic!()
    };
    let p = s.add_point(0.0, 10.0);
    let q = s.add_point(20.0, 10.0);
    let left = s.add_line(a, p);
    let right = s.add_line(b, q);
    let top = s.add_line(p, q);
    s.add_constraint(ConstraintKind::Vertical { line: left });
    s.add_constraint(ConstraintKind::Vertical { line: right });
    s.add_constraint(ConstraintKind::Horizontal { line: top });
    let height = s.add_constraint(ConstraintKind::Length {
        line: left,
        value: "10 mm".into(),
    });
    let values = [(height, 10.0)].into();
    let started = std::time::Instant::now();
    for (pa, pb) in [
        ([1200.0, 800.0], [1240.0, 805.0]),
        ([-2400.0, -1600.0], [-2340.0, -1595.0]),
    ] {
        let refs = [RefItem {
            source: source.clone(),
            shape: RefShape::Line(pa, pb),
        }];
        assert!(
            refresh_projections_with_values(&mut s, &refs, &values)
                .unwrap()
                .is_empty()
        );
        let solution = solve(&s, &values).unwrap();
        assert_eq!(solution.status, SolveStatus::WellConstrained);
        for (id, expected) in [(p, [pa[0], pa[1] + 10.0]), (q, [pb[0], pa[1] + 10.0])] {
            assert!(
                (solution.points[&id][0] - expected[0])
                    .hypot(solution.points[&id][1] - expected[1])
                    < 1e-6
            );
        }
        s.adopt_solution(&solution);
    }
    eprintln!(
        "異なる投影の2回の追従と再計算: {:.3} ms",
        started.elapsed().as_secs_f64() * 1000.0
    );
}

#[test]
fn an_explicit_contact_on_a_parent_curve_keeps_arc_extension_semantics() {
    for ellipse in [false, true] {
        let mut s = Sketch::default();
        let height = if ellipse { 3.0 } else { 5.0 };
        let center = s.add_point(0.0, 0.0);
        let start = s.add_point(5.0, 0.0);
        let end = s.add_point(0.0, height);
        let curve = if ellipse {
            s.add_entity(&Entity::Ellipse {
                id: EntityId(0),
                center,
                major: start,
                minor: end,
                ends: Some((start, end)),
                construction: false,
            })
        } else {
            s.add_arc(center, start, end)
        };
        let contact = s.add_point(0.0, -height);
        let a = s.add_point(-10.0, -height);
        let b = s.add_point(10.0, -height);
        let line = s.add_line(a, b);
        for point in [center, start, end, contact, a, b] {
            s.add_constraint(ConstraintKind::Fix { point });
        }
        s.add_constraint(ConstraintKind::PointOnCurve {
            point: contact,
            curve,
        });
        s.add_constraint(ConstraintKind::Midpoint {
            point: contact,
            line,
        });
        s.add_constraint(ConstraintKind::TangentCurve { a: line, b: curve });
        let solution = solve(&s, &Default::default()).unwrap();
        assert!(
            !matches!(solution.status, SolveStatus::Inconsistent { .. }),
            "ellipse={ellipse}: {solution:?}"
        );
        assert!(
            solution.max_residual < 1e-7,
            "ellipse={ellipse}: {solution:?}"
        );
        assert!((solution.points[&contact][0]).hypot(solution.points[&contact][1] + height) < 1e-6);
    }
}

#[test]
fn an_explicit_contact_on_a_parent_spline_or_conic_keeps_extension_semantics() {
    for conic in [false, true] {
        let mut s = Sketch::default();
        let curve = if conic {
            s.add_conic(
                (None, [-1.0, 1.0]),
                (None, [1.0, 1.0]),
                (None, [0.0, -1.0]),
                0.5,
            )
            .unwrap()
        } else {
            s.add_spline(
                &[(None, [-1.0, 1.0]), (None, [0.0, -1.0]), (None, [1.0, 1.0])],
                true,
                false,
            )
            .unwrap()
        };
        // 親の放物線は y=x²。区間の外の (2,4) で傾き4の線が接する。
        let contact = s.add_point(2.0, 4.0);
        let a = s.add_point(1.0, 0.0);
        let b = s.add_point(3.0, 8.0);
        let line = s.add_line(a, b);
        let points: Vec<_> = s
            .entities
            .iter()
            .filter_map(|e| match e {
                Entity::Point { id, .. } => Some(*id),
                _ => None,
            })
            .collect();
        for point in points {
            s.add_constraint(ConstraintKind::Fix { point });
        }
        s.add_constraint(ConstraintKind::PointOnCurve {
            point: contact,
            curve,
        });
        s.add_constraint(ConstraintKind::Midpoint {
            point: contact,
            line,
        });
        s.add_constraint(ConstraintKind::TangentCurve { a: line, b: curve });
        for reverse in [false, true] {
            let mut ordered = s.clone();
            if reverse {
                ordered.constraints.reverse();
            }
            let solution = solve(&ordered, &Default::default()).unwrap();
            assert!(
                !matches!(solution.status, SolveStatus::Inconsistent { .. }),
                "conic={conic}, reverse={reverse}: {solution:?}"
            );
            assert!(
                solution.max_residual < 1e-7,
                "conic={conic}, reverse={reverse}: {solution:?}"
            );
            assert!(
                (solution.points[&contact][0] - 2.0).hypot(solution.points[&contact][1] - 4.0)
                    < 1e-6
            );
        }
    }
}
