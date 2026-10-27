//! 確かめること: パターンの式も寸法と同じ評価と名前の付け替えを通る。
use crate::*;
use serde_json::json;

fn circular(count: &str) -> Sketch {
    let mut s = Sketch::default();
    let a = s.add_point(10.0, 0.0);
    s.add_circular_pattern(CircularPattern {
        entities: vec![a],
        center: PatternCenter::At([0.0, 0.0]),
        count: 3,
        spacing: AngularSpacing::Total,
        angle: std::f64::consts::TAU,
        suppressed: vec![],
    })
    .unwrap();
    let mut data = serde_json::to_value(s).unwrap();
    data["patterns"][0]["expressions"] = json!({"kind":"circular","count":count,"angle":"360 deg"});
    serde_json::from_value(data).unwrap()
}

#[test]
fn pattern_counts_reject_invalid_expressions_instead_of_using_cached_numbers() {
    for src in ["1.5", "0", "361", "missing", "2 mm"] {
        assert!(
            dimension_values(&circular(src), &|_| None).is_err(),
            "{src}"
        );
    }
}

#[test]
fn renaming_a_parameter_also_changes_pattern_expressions() {
    let s = circular("holes + 1");
    let renamed = s.renamed("holes", "count").unwrap().expect("式を直す");
    let data = serde_json::to_value(renamed).unwrap();
    assert_eq!(data["patterns"][0]["expressions"]["count"], "count + 1");
}

fn rectangular(count: &str) -> Sketch {
    let mut s = Sketch::default();
    let a = s.add_point(0.0, 0.0);
    s.add_constraint(ConstraintKind::Fix { point: a });
    s.add_rectangular_pattern(RectangularPattern {
        entities: vec![a],
        suppressed: vec![],
        directions: [
            PatternDirection {
                axis: PatternAxis::Angle(0.0),
                count: 3,
                distance: 10.0,
                distance_type: DistanceType::Spacing,
                symmetric: false,
            },
            PatternDirection {
                axis: PatternAxis::Angle(std::f64::consts::FRAC_PI_2),
                count: 1,
                distance: 10.0,
                distance_type: DistanceType::Spacing,
                symmetric: false,
            },
        ],
    })
    .unwrap();
    let mut data = serde_json::to_value(s).unwrap();
    data["patterns"][0]["expressions"]["directions"][0]["count"] = json!(count);
    serde_json::from_value(data).unwrap()
}

fn resolved(s: &Sketch, lookup: &dyn Fn(&str) -> Option<kmb_core::Quantity>) -> (Sketch, Solution) {
    let values = dimension_values(s, lookup).unwrap();
    let s = s.evaluated_patterns(&values).unwrap();
    let sol = solve(&s, &values).unwrap();
    (s, sol)
}

#[test]
fn expressions_change_both_pattern_counts_and_keep_remaining_copy_ids() {
    for s in [circular("holes"), rectangular("holes")] {
        let first = s.patterns[0].copies[0].entities.clone();
        let (more, _) = resolved(&s, &|n| {
            (n == "holes").then(|| kmb_core::Quantity::scalar(5.0))
        });
        assert_eq!(more.patterns[0].copies.len(), 4);
        assert_eq!(more.patterns[0].copies[0].entities, first);
        let (less, _) = resolved(&more, &|n| {
            (n == "holes").then(|| kmb_core::Quantity::scalar(2.0))
        });
        assert_eq!(less.patterns[0].copies.len(), 1);
        assert_eq!(less.patterns[0].copies[0].entities, first);
        assert_eq!(less.entities.len(), 2);
    }
}

#[test]
fn angular_distance_and_direction_expressions_use_dimension_units() {
    let mut s = circular("3");
    s.add_constraint(ConstraintKind::Fix { point: EntityId(1) });
    s.patterns[0].expressions = Some(PatternExpressions::Circular {
        count: "3".into(),
        angle: "turn / 2".into(),
    });
    let (s, sol) = resolved(&s, &|n| {
        (n == "turn").then(|| kmb_core::Quantity::deg(360.0))
    });
    let p = s.patterns[0].copies.last().unwrap().entities[0];
    assert!(crate::tests::close(sol.points[&p], [-10.0, 0.0]));
    let mut s = rectangular("3");
    if let Some(PatternExpressions::Rectangular { directions }) = &mut s.patterns[0].expressions {
        directions[0].distance = "pitch * 2".into();
        directions[0].angle = Some("45 + 45".into());
    }
    let (s, sol) = resolved(&s, &|n| (n == "pitch").then(|| kmb_core::Quantity::mm(3.0)));
    let p = s.patterns[0].copies.last().unwrap().entities[0];
    assert!(crate::tests::close(sol.points[&p], [0.0, 12.0]));
}

#[test]
fn numeric_documents_upgrade_to_expressions_without_changing_the_shape() {
    for s in [circular("3"), rectangular("3")] {
        let old_sol = solve(&s, &dimension_values(&s, &|_| None).unwrap()).unwrap();
        let mut old = serde_json::to_value(&s).unwrap();
        old["patterns"][0]
            .as_object_mut()
            .unwrap()
            .remove("expressions");
        let upgraded: Sketch = serde_json::from_value(old).unwrap();
        assert!(upgraded.patterns[0].expressions.is_some());
        let (new, sol) = resolved(&upgraded, &|_| None);
        for (id, p) in old_sol.points {
            assert!(crate::tests::close(p, sol.points[&id]));
        }
        let round: Sketch = serde_json::from_str(&serde_json::to_string(&new).unwrap()).unwrap();
        assert_eq!(new, round);
    }
}

#[test]
fn deleting_a_copy_used_by_a_dimension_returns_a_reason() {
    let mut s = rectangular("holes");
    let p = s.patterns[0].copies.last().unwrap().entities[0];
    s.add_constraint(ConstraintKind::Distance {
        a: EntityId(1),
        b: p,
        value: "20 mm".into(),
    });
    let values = dimension_values(&s, &|_| Some(kmb_core::Quantity::scalar(2.0))).unwrap();
    assert!(
        s.evaluated_patterns(&values)
            .unwrap_err()
            .contains("参照を外して")
    );
}

#[test]
fn rectangle_counts_and_lengths_reject_wrong_units_and_out_of_range_values() {
    for src in ["201", "2.1", "0", "-1", "2 deg"] {
        assert!(
            dimension_values(&rectangular(src), &|_| None).is_err(),
            "{src}"
        );
    }
    let mut s = rectangular("2");
    if let Some(PatternExpressions::Rectangular { directions }) = &mut s.patterns[0].expressions {
        directions[0].distance = "20 deg".into();
    }
    assert!(
        dimension_values(&s, &|_| None)
            .unwrap_err()
            .contains("長さ")
    );
}

#[test]
fn document_recompute_tracks_count_parameters_and_renames_them() {
    use kmb_core::{Document, FeatureEntry, FeatureStatus, NullKernel, Op, Param, Transaction};
    for s in [circular("holes"), rectangular("holes")] {
        let mut doc = Document::new();
        let param = doc.alloc_param_id();
        let f = doc.alloc_feature_id();
        doc.apply(Transaction::new(
            "準備",
            vec![
                Op::AddParam {
                    index: None,
                    param: Param::new(param, "holes", "3"),
                },
                Op::InsertFeature {
                    index: None,
                    entry: FeatureEntry {
                        id: f,
                        type_id: SKETCH.into(),
                        name: "並び".into(),
                        data: serde_json::to_value(s).unwrap(),
                        suppressed: false,
                        pin_bodies: false,
                    },
                },
            ],
        ))
        .unwrap();
        let reg = crate::tests::registry();
        let points = |doc: &mut Document| match doc.recompute(&reg, &NullKernel).feature(f) {
            Some(FeatureStatus::Ok { outputs, .. }) => {
                solution_of(&outputs["sketch"]).unwrap().points.len()
            }
            other => panic!("{other:?}"),
        };
        assert_eq!(points(&mut doc), 3);
        doc.apply(Transaction::new(
            "数",
            vec![Op::SetParamExpr {
                id: param,
                expr: "5".into(),
            }],
        ))
        .unwrap();
        assert_eq!(points(&mut doc), 5);
        doc.rename_param(&reg, param, "copies").unwrap();
        assert_eq!(points(&mut doc), 5);
        let data = &doc.data().feature(f).unwrap().data;
        assert!(data.to_string().contains("copies"));
        doc.apply(Transaction::new(
            "式",
            vec![Op::SetParamExpr {
                id: param,
                expr: "1.5".into(),
            }],
        ))
        .unwrap();
        assert!(matches!(
            doc.recompute(&reg, &NullKernel).feature(f),
            Some(FeatureStatus::Error { .. })
        ));
    }
}

#[test]
fn removing_a_direction_line_keeps_the_angle_and_other_expressions() {
    let mut s = rectangular("3");
    let a = s.add_point(5.0, 0.0);
    let b = s.add_point(5.0, 5.0);
    let line = s.add_line(a, b);
    if let PatternDef::Rectangular(r) = &mut s.patterns[0].def {
        r.directions[0].axis = PatternAxis::Line(line);
    }
    if let Some(PatternExpressions::Rectangular { directions }) = &mut s.patterns[0].expressions {
        directions[0].angle = None;
        directions[0].distance = "pitch".into();
    }
    s.remove_entities(&[line]);
    let (s, sol) = resolved(&s, &|n| (n == "pitch").then(|| kmb_core::Quantity::mm(4.0)));
    let p = s.patterns[0].copies.last().unwrap().entities[0];
    assert!(crate::tests::close(sol.points[&p], [0.0, 8.0]));
}
