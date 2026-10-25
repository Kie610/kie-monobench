use super::*;
use jc_core::{Document, FeatureEntry, FeatureStatus, NullKernel, Op, Param, Transaction};
use serde_json::json;

fn pt(id: u32, x: f64, y: f64) -> Entity {
    Entity::Point {
        id: EntityId(id),
        x,
        y,
    }
}
fn line(id: u32, a: u32, b: u32) -> Entity {
    Entity::Line {
        id: EntityId(id),
        a: EntityId(a),
        b: EntityId(b),
        construction: false,
    }
}
fn c(id: u32, kind: ConstraintKind) -> Constraint {
    Constraint::new(ConstraintId(id), kind)
}
pub(crate) fn e(id: u32) -> EntityId {
    EntityId(id)
}

/// 原点に固定した長方形。点 1〜4、辺 11〜14。幅と高さは式。
pub(crate) fn rectangle(width: &str, height: &str) -> Sketch {
    use ConstraintKind as K;
    Sketch {
        plane: Default::default(),
        offset: None,
        projections: vec![],
        centerlines: vec![],
        patterns: vec![],
        dim_names: Default::default(),
        next_ids: Default::default(),
        entities: vec![
            pt(1, 0.0, 0.0),
            pt(2, 9.0, 1.0),
            pt(3, 11.0, 6.0),
            pt(4, -1.0, 4.0),
            line(11, 1, 2),
            line(12, 2, 3),
            line(13, 3, 4),
            line(14, 4, 1),
        ],
        constraints: vec![
            c(1, K::Fix { point: e(1) }),
            c(2, K::Horizontal { line: e(11) }),
            c(3, K::Horizontal { line: e(13) }),
            c(4, K::Vertical { line: e(12) }),
            c(5, K::Vertical { line: e(14) }),
            c(
                6,
                K::Length {
                    line: e(11),
                    value: width.into(),
                },
            ),
            c(
                7,
                K::Length {
                    line: e(12),
                    value: height.into(),
                },
            ),
        ],
    }
}

fn values(s: &Sketch, v: &[(u32, f64)]) -> HashMap<ConstraintId, f64> {
    let _ = s;
    v.iter().map(|(i, x)| (ConstraintId(*i), *x)).collect()
}

pub(crate) fn close(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6
}

#[test]
fn rectangle_is_well_constrained() {
    let s = rectangle("w", "h");
    let sol = solve(&s, &values(&s, &[(6, 40.0), (7, 25.0)])).unwrap();
    assert_eq!(sol.status, SolveStatus::WellConstrained);
    assert_eq!(sol.dof, 0);
    assert!(close(sol.points[&e(2)], [40.0, 0.0]));
    assert!(close(sol.points[&e(3)], [40.0, 25.0]));
    assert!(close(sol.points[&e(4)], [0.0, 25.0]));
    for id in [1, 2, 3, 4, 11, 12, 13, 14] {
        assert!(sol.fixed.contains(&e(id)), "E{id} が決まっていません");
    }
}

#[test]
fn missing_dimension_leaves_freedom() {
    let mut s = rectangle("w", "h");
    s.constraints.retain(|c| c.id != ConstraintId(7));
    let sol = solve(&s, &values(&s, &[(6, 40.0)])).unwrap();
    assert_eq!(sol.status, SolveStatus::UnderConstrained { dof: 1 });
    // 下の辺は決まっているが、上の辺は高さが決まらない
    assert!(sol.fixed.contains(&e(11)));
    assert!(!sol.fixed.contains(&e(13)));
    assert!(!sol.fixed.contains(&e(3)));
}

#[test]
fn redundant_and_conflicting_dimensions() {
    use ConstraintKind as K;
    let mut s = rectangle("w", "h");
    s.constraints.push(c(
        8,
        K::Length {
            line: e(14),
            value: "h".into(),
        },
    ));
    let sol = solve(&s, &values(&s, &[(6, 40.0), (7, 25.0), (8, 25.0)])).unwrap();
    assert_eq!(
        sol.status,
        SolveStatus::OverConstrained {
            redundant: vec![ConstraintId(8)]
        }
    );

    let sol = solve(&s, &values(&s, &[(6, 40.0), (7, 25.0), (8, 30.0)])).unwrap();
    match sol.status {
        SolveStatus::Inconsistent { conflicting } => {
            assert!(conflicting.contains(&ConstraintId(8)))
        }
        other => panic!("矛盾として検出されません: {other:?}"),
    }
}

#[test]
fn circle_tangent_and_arc() {
    use ConstraintKind as K;
    let s = Sketch {
        plane: Default::default(),
        offset: None,
        projections: vec![],
        centerlines: vec![],
        patterns: vec![],
        dim_names: Default::default(),
        next_ids: Default::default(),
        entities: vec![
            pt(1, 0.0, 0.0),
            pt(2, 50.0, 0.0),
            line(10, 1, 2),
            pt(3, 20.0, 12.0),
            Entity::Circle {
                id: e(20),
                center: e(3),
                radius: 3.0,
                construction: false,
            },
            pt(4, 40.0, 30.0),
            pt(5, 48.0, 30.0),
            pt(6, 40.0, 38.0),
            Entity::Arc {
                id: e(30),
                center: e(4),
                start: e(5),
                end: e(6),
                construction: false,
            },
        ],
        constraints: vec![
            c(1, K::Fix { point: e(1) }),
            c(2, K::Fix { point: e(2) }),
            c(
                3,
                K::Tangent {
                    line: e(10),
                    circle: e(20),
                },
            ),
            c(
                4,
                K::Radius {
                    circle: e(20),
                    value: "r".into(),
                },
            ),
            c(
                5,
                K::HorizontalDistance {
                    a: e(1),
                    b: e(3),
                    value: "x".into(),
                },
            ),
            c(6, K::Equal { a: e(20), b: e(30) }),
        ],
    };
    let sol = solve(&s, &values(&s, &[(4, 10.0), (5, 20.0)])).unwrap();
    assert!(
        close(sol.points[&e(3)], [20.0, 10.0]),
        "{:?}",
        sol.points[&e(3)]
    );
    assert!((sol.radii[&e(20)] - 10.0).abs() < 1e-6);
    let p = |id| sol.points[&e(id)];
    let r = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]);
    assert!((r(p(5), p(4)) - 10.0).abs() < 1e-6);
    assert!((r(p(6), p(4)) - 10.0).abs() < 1e-6);
    assert!(matches!(sol.status, SolveStatus::UnderConstrained { .. }));
}

#[test]
fn angle_parallel_perpendicular_midpoint() {
    use ConstraintKind as K;
    let s = Sketch {
        plane: Default::default(),
        offset: None,
        projections: vec![],
        centerlines: vec![],
        patterns: vec![],
        dim_names: Default::default(),
        next_ids: Default::default(),
        entities: vec![
            pt(1, 0.0, 0.0),
            pt(2, 10.0, 1.0),
            pt(3, 0.0, 0.0),
            pt(4, 6.0, 8.0),
            line(10, 1, 2),
            line(11, 3, 4),
            pt(5, 0.0, 5.0),
            pt(6, 10.0, 6.0),
            line(12, 5, 6),
            pt(7, 3.0, 3.0),
            pt(8, 2.0, 9.0),
            line(13, 7, 8),
            pt(9, 1.0, 1.0),
        ],
        constraints: vec![
            c(1, K::Fix { point: e(1) }),
            c(2, K::Coincident { a: e(1), b: e(3) }),
            c(3, K::Horizontal { line: e(10) }),
            c(
                4,
                K::Angle {
                    a: e(10),
                    b: e(11),
                    value: "a".into(),
                },
            ),
            c(5, K::Parallel { a: e(12), b: e(10) }),
            c(6, K::Perpendicular { a: e(13), b: e(10) }),
            c(
                7,
                K::Midpoint {
                    point: e(9),
                    line: e(10),
                },
            ),
            c(
                8,
                K::Length {
                    line: e(10),
                    value: "l".into(),
                },
            ),
            c(
                9,
                K::PointOnLine {
                    point: e(7),
                    line: e(12),
                },
            ),
        ],
    };
    let sol = solve(&s, &values(&s, &[(4, 60f64.to_radians()), (8, 10.0)])).unwrap();
    let p = |id| sol.points[&e(id)];
    let ang = (p(4)[1] - p(3)[1]).atan2(p(4)[0] - p(3)[0]).to_degrees();
    assert!((ang - 60.0).abs() < 1e-5, "{ang}");
    assert!((p(5)[1] - p(6)[1]).abs() < 1e-6);
    assert!((p(7)[0] - p(8)[0]).abs() < 1e-6);
    assert!(close(p(9), [5.0, 0.0]));
    assert!((p(7)[1] - p(5)[1]).abs() < 1e-6);
}

#[test]
fn bad_references_are_errors() {
    use ConstraintKind as K;
    let mut s = rectangle("w", "h");
    s.constraints.push(c(9, K::Horizontal { line: e(1) }));
    assert!(matches!(
        solve(&s, &values(&s, &[(6, 1.0), (7, 1.0)])),
        Err(SketchError::BadReference { .. })
    ));
    let mut s = rectangle("w", "h");
    s.entities.push(line(15, 1, 99));
    assert!(solve(&s, &values(&s, &[(6, 1.0), (7, 1.0)])).is_err());
    let s = rectangle("w", "h");
    assert!(matches!(
        solve(&s, &values(&s, &[(6, 1.0)])),
        Err(SketchError::MissingValue(_))
    ));
}

// ---- タイムラインに載せた時の振る舞い ----

pub(crate) fn registry() -> Registry {
    let mut r = Registry::new();
    register(&mut r);
    r
}

pub(crate) fn doc_with_rectangle() -> (Document, jc_core::FeatureId) {
    let mut doc = Document::new();
    let (pw, ph, f) = (
        doc.alloc_param_id(),
        doc.alloc_param_id(),
        doc.alloc_feature_id(),
    );
    let data = serde_json::to_value(rectangle("width", "width / 2")).unwrap();
    doc.apply(Transaction::new(
        "準備",
        vec![
            Op::AddParam {
                index: None,
                param: Param::new(pw, "width", "40 mm"),
            },
            Op::AddParam {
                index: None,
                param: Param::new(ph, "unused", "1"),
            },
            Op::InsertFeature {
                index: None,
                entry: FeatureEntry {
                    id: f,
                    type_id: SKETCH.into(),
                    name: "スケッチ1".into(),
                    data,
                    suppressed: false,
                },
            },
        ],
    ))
    .unwrap();
    (doc, f)
}

pub(crate) fn solved(doc: &mut Document, f: jc_core::FeatureId) -> Solution {
    match doc.recompute(&registry(), &NullKernel).feature(f) {
        Some(FeatureStatus::Ok { outputs, .. }) => solution_of(&outputs["sketch"]).unwrap(),
        other => panic!("スケッチが解けません: {other:?}"),
    }
}

#[test]
fn parameter_change_updates_sketch() {
    let (mut doc, f) = doc_with_rectangle();
    assert!(close(solved(&mut doc, f).points[&e(3)], [40.0, 20.0]));
    let id = doc.data().param_by_name("width").unwrap().id;
    doc.apply(Transaction::new(
        "幅",
        vec![Op::SetParamExpr {
            id,
            expr: "6 cm".into(),
        }],
    ))
    .unwrap();
    let sol = solved(&mut doc, f);
    assert!(close(sol.points[&e(3)], [60.0, 30.0]));
    assert_eq!(sol.status, SolveStatus::WellConstrained);
}

#[test]
fn renaming_a_parameter_rewrites_dimensions() {
    let (mut doc, f) = doc_with_rectangle();
    let id = doc.data().param_by_name("width").unwrap().id;
    doc.rename_param(&registry(), id, "w").unwrap();
    let s: Sketch = serde_json::from_value(doc.data().feature(f).unwrap().data.clone()).unwrap();
    assert_eq!(s.constraints[5].kind.value(), Some("w"));
    assert_eq!(s.constraints[6].kind.value(), Some("w / 2"));
    assert!(close(solved(&mut doc, f).points[&e(3)], [40.0, 20.0]));
    // 名前の変更と式の書き換えは、1 回の取り消しで戻る
    doc.undo();
    assert!(doc.data().param_by_name("width").is_some());
    let s: Sketch = serde_json::from_value(doc.data().feature(f).unwrap().data.clone()).unwrap();
    assert_eq!(s.constraints[5].kind.value(), Some("width"));
}

#[test]
fn wrong_units_are_reported() {
    let (mut doc, f) = doc_with_rectangle();
    let id = doc.data().param_by_name("width").unwrap().id;
    doc.apply(Transaction::new(
        "幅",
        vec![Op::SetParamExpr {
            id,
            // 単位の無い値は mm として読むので、角度で誤りにする
            expr: "40 deg".into(),
        }],
    ))
    .unwrap();
    let r = doc.recompute(&registry(), &NullKernel);
    assert!(
        matches!(r.feature(f), Some(FeatureStatus::Error { message }) if message.contains("長さ"))
    );
}

#[test]
fn sketch_contract() {
    jc_contract::check_feature(
        &registry(),
        &jc_contract::Case {
            type_id: SKETCH,
            params: &[("width", "40 mm")],
            upstream: &[],
            data: serde_json::to_value(rectangle("width", "10 mm")).unwrap(),
            bad_data: &[
                json!({"entities": 1}),
                json!({"entities": [{"type": "line", "id": 1, "a": 2, "b": 3}]}),
                json!({"plane": "xy"}),
            ],
        },
    );
}

proptest::proptest! {
    /// 長方形は、どんな正の寸法でも完全拘束で解ける
    #[test]
    fn rectangle_solves_for_any_size(w in 0.5f64..500.0, h in 0.5f64..500.0) {
        let s = rectangle("w", "h");
        let sol = solve(&s, &values(&s, &[(6, w), (7, h)])).unwrap();
        proptest::prop_assert_eq!(sol.status, SolveStatus::WellConstrained);
        proptest::prop_assert!(close(sol.points[&e(3)], [w, h]));
    }
}

#[test]
fn editing_operations() {
    let mut s = Sketch::default();
    let a = s.add_point(0.0, 0.0);
    let b = s.add_point(10.0, 0.0);
    let c = s.add_point(10.0, 10.0);
    let l1 = s.add_line(a, b);
    let l2 = s.add_line(b, c);
    s.add_constraint(ConstraintKind::Horizontal { line: l1 });
    s.add_constraint(ConstraintKind::Length {
        line: l2,
        value: "5 mm".into(),
    });
    let sol = solve(&s, &values(&s, &[(2, 5.0)])).unwrap();
    let (pa, pb) = (sol.points[&a], sol.points[&b]);
    let mid = [(pa[0] + pb[0]) / 2.0, (pa[1] + pb[1]) / 2.0];
    assert_eq!(
        s.hit_test(&sol, mid[0], mid[1] + 0.3, 1.0)
            .map(|h| h.entity),
        Some(l1)
    );
    assert_eq!(
        s.hit_test(&sol, pb[0] + 0.2, pb[1], 1.0).map(|h| h.entity),
        Some(b)
    );
    s.adopt_solution(&sol);
    // 線分 2 を消すと、共有していない端点 c と、その長さの拘束も消える
    s.remove_entities(&[l2]);
    assert!(s.entity(c).is_none());
    assert!(s.entity(b).is_some());
    assert_eq!(s.constraints.len(), 1);
    // 点 a を消すと、それを使う線分 1 と水平拘束も消える
    s.remove_entities(&[a]);
    assert!(s.entity(l1).is_none());
    assert!(s.constraints.is_empty());
}

#[test]
fn profile_finds_outer_loop_hole_and_open_lines() {
    let mut s = rectangle("40 mm", "20 mm");
    // 穴の円と、途切れた線を足す
    s.entities.push(pt(20, 10.0, 10.0));
    s.entities.push(Entity::Circle {
        id: e(21),
        center: e(20),
        radius: 3.0,
        construction: false,
    });
    s.entities.push(pt(30, 50.0, 0.0));
    s.entities.push(pt(31, 60.0, 0.0));
    s.entities.push(line(32, 30, 31));
    // 補助線は輪郭に入らない
    s.entities.push(Entity::Line {
        id: e(33),
        a: e(1),
        b: e(3),
        construction: true,
    });
    let sol = solve(&s, &values(&s, &[(6, 40.0), (7, 20.0)])).unwrap();
    let (p, rejected) = profile_of(&s, &sol, Plane::XY);
    assert_eq!(rejected, vec!["e32".to_string()]);
    assert_eq!(p.regions.len(), 1);
    let r = &p.regions[0];
    assert_eq!(r.outer.edges.len(), 4);
    assert!(
        (r.outer.signed_area() - 800.0).abs() < 1e-6,
        "{}",
        r.outer.signed_area()
    );
    assert_eq!(r.holes.len(), 1);
    assert!(r.holes[0].signed_area() < 0.0);
    assert_eq!(r.holes[0].edges[0].name, "e21");
}

#[test]
fn open_ends_are_the_line_ends_that_touch_nothing() {
    // 閉じていない輪郭の端だけを印にする
    let mut s = rectangle("40 mm", "20 mm");
    let sol = solve(&s, &values(&s, &[(6, 40.0), (7, 20.0)])).unwrap();
    assert!(
        open_ends_of(&s, &sol).is_empty(),
        "閉じた長方形には端がない"
    );
    // 途切れた線(両端)、上の辺の途中から立てた線(先だけ)、補助線(数えない)
    s.entities.push(pt(30, 50.0, 0.0));
    s.entities.push(pt(31, 60.0, 0.0));
    s.entities.push(line(32, 30, 31));
    s.entities.push(pt(40, 20.0, 20.0));
    s.entities.push(pt(41, 20.0, 30.0));
    s.entities.push(line(42, 40, 41));
    s.entities.push(Entity::Line {
        id: e(43),
        a: e(1),
        b: e(31),
        construction: true,
    });
    let sol = solve(&s, &values(&s, &[(6, 40.0), (7, 20.0)])).unwrap();
    let mut ends = open_ends_of(&s, &sol);
    ends.sort_by(|a, b| a[0].total_cmp(&b[0]));
    assert_eq!(ends.len(), 3, "{ends:?}");
    assert!(close(ends[0], [20.0, 30.0]), "{ends:?}");
    assert!(close(ends[1], [50.0, 0.0]), "{ends:?}");
    assert!(close(ends[2], [60.0, 0.0]), "{ends:?}");
}

#[test]
fn profile_of_reversed_drawing_is_counter_clockwise() {
    // 時計回りに描いた三角形も、外周は反時計回りになる
    let s = Sketch {
        plane: Default::default(),
        offset: None,
        projections: vec![],
        centerlines: vec![],
        patterns: vec![],
        dim_names: Default::default(),
        next_ids: Default::default(),
        entities: vec![
            pt(1, 0.0, 0.0),
            pt(2, 0.0, 10.0),
            pt(3, 10.0, 0.0),
            line(4, 1, 2),
            line(5, 3, 2),
            line(6, 3, 1),
        ],
        constraints: vec![],
    };
    let sol = solve(&s, &HashMap::new()).unwrap();
    let (p, rejected) = profile_of(&s, &sol, Plane::XY);
    assert!(rejected.is_empty());
    let l = &p.regions[0].outer;
    assert!((l.signed_area() - 50.0).abs() < 1e-6);
    for i in 0..3 {
        let a = l.edges[i].to_point();
        let b = l.edges[(i + 1) % 3].from_point();
        assert!(close(a, b));
    }
}

#[test]
fn sketch_outputs_profile_slot() {
    let mut doc = Document::new();
    let id = doc.alloc_feature_id();
    let tx = Transaction::new(
        "スケッチ",
        vec![Op::InsertFeature {
            entry: FeatureEntry {
                id,
                type_id: SKETCH.into(),
                name: "s".into(),
                data: serde_json::to_value(rectangle("30 mm", "10 mm")).unwrap(),
                suppressed: false,
            },
            index: None,
        }],
    );
    doc.apply(tx).unwrap();
    let mut reg = Registry::new();
    register(&mut reg);
    let report = doc.recompute(&reg, &NullKernel);
    let FeatureStatus::Ok { outputs, .. } = report.feature(id).unwrap() else {
        panic!("{:?}", report.feature(id));
    };
    let p = profile_value(&outputs["profile"]).unwrap();
    assert_eq!(p.plane, Plane::XY);
    assert!((p.regions[0].outer.signed_area() - 300.0).abs() < 1e-6);
}

#[test]
fn a_dimension_already_determined_is_previewed_as_a_reference() {
    use ConstraintKind as K;
    let s = rectangle("w", "h");
    let v = values(&s, &[(6, 40.0), (7, 25.0)]);
    let sol = solve(&s, &v).unwrap();
    // 左の辺の長さは、右の辺の高さと水平・垂直で既に決まっている
    let kind = dimension_for(&s, &sol, &[e(14)]).unwrap();
    assert!(matches!(kind, K::Length { line, .. } if line == e(14)));
    let p = preview_dimension(&s, &v, &kind).unwrap();
    assert!(p.reference);
    assert_eq!(p.text, "25 mm");
    assert!(p.determined_by.contains(&ConstraintId(7)), "{p:?}");
    let note = p.note(&s).unwrap();
    assert!(note.contains("C7(長さ h)"), "{note}");

    // 置くと参照寸法になり、形は変わらず、測った値が解に載る
    let mut placed = s.clone();
    let (id, _) = placed.add_dimension(kind, &v).unwrap();
    let c = placed.constraints.iter().find(|c| c.id == id).unwrap();
    assert!(c.reference);
    let sol2 = solve(&placed, &v).unwrap();
    assert_eq!(sol2.status, SolveStatus::WellConstrained);
    assert!((sol2.measured[&id] - 25.0).abs() < 1e-9);

    // 参照寸法は JSON に印が付き、読み戻せる
    let j = serde_json::to_value(&placed).unwrap();
    assert_eq!(j["constraints"][7]["reference"], json!(true));
    assert!(j["constraints"][0].get("reference").is_none());
    let back: Sketch = serde_json::from_value(j).unwrap();
    assert_eq!(back, placed);

    // 形を決める寸法へは戻せない(既に決まっている)
    let err = placed.set_driving(id, true, &v).unwrap_err();
    assert!(err.contains("既に決まっています"), "{err}");
}

#[test]
fn a_free_dimension_is_driving_and_takes_the_typed_value() {
    let mut s = rectangle("w", "h");
    s.constraints.retain(|c| c.id != ConstraintId(7));
    let v = values(&s, &[(6, 40.0)]);
    let sol = solve(&s, &v).unwrap();
    let mut kind = dimension_for(&s, &sol, &[e(14)]).unwrap();
    let p = preview_dimension(&s, &v, &kind).unwrap();
    assert!(!p.reference && p.note(&s).is_none());
    // 測った値(今の解での左の辺の長さ)が入力欄に出る
    let y4 = sol.points[&e(4)][1];
    assert!((p.measured - y4.abs()).abs() < 1e-9, "{p:?}");
    assert!(p.text.ends_with(" mm"));
    // 打った値で置く
    *kind.value_mut().unwrap() = "12 mm".into();
    let (id, _) = s.add_dimension(kind, &v).unwrap();
    let c = s.constraints.iter().find(|c| c.id == id).unwrap();
    assert!(!c.reference);
    assert_eq!(c.kind.value(), Some("12 mm"));
    let mut v2 = v.clone();
    v2.insert(id, 12.0);
    let sol = solve(&s, &v2).unwrap();
    assert_eq!(sol.status, SolveStatus::WellConstrained);
    assert!(close(sol.points[&e(4)], [0.0, 12.0]));
    // 画面と同じく、解いた位置を保存する座標へ写してから次の編集をする
    s.adopt_solution(&sol);

    // 参照寸法にして高さの寸法を足し、もう一度形を決める寸法へ戻そうとすると断る
    s.set_driving(id, false, &v2).unwrap();
    let mut v3 = values(&s, &[(6, 40.0)]);
    let h = s.add_constraint(ConstraintKind::Length {
        line: e(12),
        value: "20 mm".into(),
    });
    v3.insert(h, 20.0);
    assert!(s.set_driving(id, true, &v3).is_err());
    // 高さの寸法を消せば戻せる。値は今の形で測った値になる
    s.remove_constraint(h);
    s.set_driving(id, true, &v).unwrap();
    let c = s.constraints.iter().find(|c| c.id == id).unwrap();
    assert!(!c.reference);
    assert_eq!(c.kind.value(), Some("12 mm"));
}

#[test]
fn dimension_kinds_follow_the_selection() {
    use ConstraintKind as K;
    let s = rectangle("w", "h");
    let sol = solve(&s, &values(&s, &[(6, 40.0), (7, 25.0)])).unwrap();
    assert!(matches!(
        dimension_for(&s, &sol, &[e(1), e(3)]),
        Some(K::Distance { .. })
    ));
    assert!(matches!(
        dimension_for(&s, &sol, &[e(3), e(11)]),
        Some(K::PointLineDistance { .. })
    ));
    // 平行な 2 本は間の距離、直角な 2 本は角度
    let k = dimension_for(&s, &sol, &[e(11), e(13)]).unwrap();
    assert!(matches!(k, K::PointLineDistance { line, .. } if line == e(11)));
    assert!((measure(&s, &sol, &k).unwrap() - 25.0).abs() < 1e-9);
    let k = dimension_for(&s, &sol, &[e(11), e(12)]).unwrap();
    assert!(matches!(k, K::Angle { .. }));
    let p = preview_dimension(&s, &values(&s, &[(6, 40.0), (7, 25.0)]), &k).unwrap();
    assert_eq!(p.text, "90 deg");
    assert!(p.reference);
    assert_eq!(dimension_for(&s, &sol, &[e(1)]), None);
}

#[test]
fn a_reference_dimension_needs_no_value_and_adds_no_input() {
    let mut s = rectangle("w", "h");
    s.constraints.push(Constraint {
        id: ConstraintId(8),
        kind: ConstraintKind::Length {
            line: e(14),
            value: "unknown_param".into(),
        },
        reference: true,
        at: None,
        label: None,
    });
    let v = dimension_values(&s, &|n| match n {
        "w" => Some(jc_core::Quantity::mm(40.0)),
        "h" => Some(jc_core::Quantity::mm(25.0)),
        _ => None,
    })
    .unwrap();
    assert!(!v.contains_key(&ConstraintId(8)));
    let inputs = jc_core::Feature::inputs(&s).unwrap();
    assert!(
        !format!("{inputs:?}").contains("unknown_param"),
        "{inputs:?}"
    );
}

/// 円(中心 0,0、半径 10)と、その右半分に重ねた長方形(0〜25 × -5〜5)。前方後円墳の形。
fn keyhole() -> Sketch {
    Sketch {
        plane: Default::default(),
        offset: None,
        projections: vec![],
        centerlines: vec![],
        patterns: vec![],
        dim_names: Default::default(),
        next_ids: Default::default(),
        entities: vec![
            pt(1, 0.0, 0.0),
            Entity::Circle {
                id: e(2),
                center: e(1),
                radius: 10.0,
                construction: false,
            },
            pt(3, 0.0, -5.0),
            pt(4, 25.0, -5.0),
            pt(5, 25.0, 5.0),
            pt(6, 0.0, 5.0),
            line(7, 3, 4),
            line(8, 4, 5),
            line(9, 5, 6),
            line(10, 6, 3),
        ],
        constraints: vec![],
    }
}

#[test]
fn overlapping_circle_and_rectangle_are_split_into_three_faces() {
    let s = keyhole();
    let sol = solve(&s, &HashMap::new()).unwrap();
    let faces = faces_of(&s, &sol, Plane::XY);
    assert_eq!(
        faces.faces.len(),
        3,
        "{:#?}",
        faces.faces.iter().map(|f| &f.key).collect::<Vec<_>>()
    );
    let pi = std::f64::consts::PI;
    let mut areas: Vec<f64> = faces
        .faces
        .iter()
        .map(|f| f.region.outer.signed_area())
        .collect();
    areas.sort_by(f64::total_cmp);
    // 重なり: 長方形のうち円の中(x≥0, |y|≤5)。円だけ: 円 − 重なり。長方形だけ: 250 − 重なり
    let lens = 2.0 * (0.5 * 5.0 * 75f64.sqrt() + 0.5 * 100.0 * (0.5f64).asin());
    let want = {
        let mut v = vec![lens, 100.0 * pi - lens, 250.0 - lens];
        v.sort_by(f64::total_cmp);
        v
    };
    for (a, w) in areas.iter().zip(&want) {
        assert!((a - w).abs() < 0.6, "{areas:?} {want:?}");
    }
    // 領域の名前はどれも違い、どの領域にも穴はない
    let mut keys: Vec<&String> = faces.faces.iter().map(|f| &f.key).collect();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), 3);
    assert!(faces.faces.iter().all(|f| f.region.holes.is_empty()));
    // 既定の輪郭は全部を合わせた 1 つ(円と長方形の和)
    let (p, unused) = profile_of(&s, &sol, Plane::XY);
    assert!(unused.is_empty(), "{unused:?}");
    assert_eq!(p.regions.len(), 1);
    assert!(p.regions[0].holes.is_empty());
    let union = 100.0 * pi + 250.0 - lens;
    assert!((p.regions[0].outer.signed_area() - union).abs() < 0.6);
    // 選んだ領域だけを合わせる: 重なりと長方形だけの所 = 長方形
    let small: Vec<String> = faces
        .faces
        .iter()
        .filter(|f| f.region.outer.signed_area() < 100.0 * pi - lens - 1.0)
        .map(|f| f.key.clone())
        .collect();
    assert_eq!(small.len(), 2);
    let rect = faces.profile(&small).unwrap();
    assert_eq!(rect.regions.len(), 1);
    assert!((rect.regions[0].outer.signed_area() - 250.0).abs() < 1e-6);
    // 輪郭はつながっている
    let l = &rect.regions[0].outer;
    for i in 0..l.edges.len() {
        let a = l.edges[i].to_point();
        let b = l.edges[(i + 1) % l.edges.len()].from_point();
        assert!(close(a, b), "{a:?} {b:?}");
    }
    assert!(faces.profile(&["e99".into()]).is_err());
    // 円だけの所と重なり = 円。分けた円弧は 1 周の円に戻す(側面が継ぎ目で割れない)
    let circle: Vec<String> = faces
        .faces
        .iter()
        .filter(|f| f.key.starts_with("e2."))
        .filter(|f| !f.key.contains("e8"))
        .map(|f| f.key.clone())
        .collect();
    assert_eq!(circle.len(), 2);
    let p = faces.profile(&circle).unwrap();
    assert_eq!(p.regions.len(), 1);
    let edges = &p.regions[0].outer.edges;
    assert_eq!(edges.len(), 1, "{edges:?}");
    assert!(matches!(edges[0].segment, jc_core::Segment::Circle { .. }));
    assert_eq!(edges[0].name, "e2");
}

#[test]
fn nested_shapes_keep_holes_and_the_inner_disk_is_its_own_face() {
    let mut s = rectangle("40 mm", "20 mm");
    s.entities.push(pt(20, 10.0, 10.0));
    s.entities.push(Entity::Circle {
        id: e(21),
        center: e(20),
        radius: 3.0,
        construction: false,
    });
    let sol = solve(&s, &values(&s, &[(6, 40.0), (7, 20.0)])).unwrap();
    let faces = faces_of(&s, &sol, Plane::XY);
    assert_eq!(faces.faces.len(), 2);
    let ring = faces
        .faces
        .iter()
        .find(|f| f.region.holes.len() == 1)
        .unwrap();
    assert!((ring.region.outer.signed_area() - 800.0).abs() < 1e-6);
    // 輪と円の両方を選ぶと、穴のない長方形
    let keys: Vec<String> = faces.faces.iter().map(|f| f.key.clone()).collect();
    let all = faces.profile(&keys).unwrap();
    assert_eq!(all.regions.len(), 1);
    assert!(all.regions[0].holes.is_empty());
}

#[test]
fn a_line_across_a_rectangle_splits_it_and_dangling_ends_are_dropped() {
    // 長方形(0〜20 × 0〜10)を、上下にはみ出す縦の線 x=8 で 2 つに分ける
    let s = Sketch {
        plane: Default::default(),
        offset: None,
        projections: vec![],
        centerlines: vec![],
        patterns: vec![],
        dim_names: Default::default(),
        next_ids: Default::default(),
        entities: vec![
            pt(1, 0.0, 0.0),
            pt(2, 20.0, 0.0),
            pt(3, 20.0, 10.0),
            pt(4, 0.0, 10.0),
            line(5, 1, 2),
            line(6, 2, 3),
            line(7, 3, 4),
            line(8, 4, 1),
            pt(9, 8.0, -3.0),
            pt(10, 8.0, 13.0),
            line(11, 9, 10),
        ],
        constraints: vec![],
    };
    let sol = solve(&s, &HashMap::new()).unwrap();
    let faces = faces_of(&s, &sol, Plane::XY);
    let mut areas: Vec<f64> = faces
        .faces
        .iter()
        .map(|f| f.region.outer.signed_area())
        .collect();
    areas.sort_by(f64::total_cmp);
    assert_eq!(areas.len(), 2);
    assert!(
        (areas[0] - 80.0).abs() < 1e-6 && (areas[1] - 120.0).abs() < 1e-6,
        "{areas:?}"
    );
    let (p, unused) = profile_of(&s, &sol, Plane::XY);
    assert!(unused.is_empty());
    assert_eq!(p.regions.len(), 1);
    assert!((p.regions[0].outer.signed_area() - 200.0).abs() < 1e-6);
}

#[test]
fn two_overlapping_circles_make_three_faces() {
    let s = Sketch {
        plane: Default::default(),
        offset: None,
        projections: vec![],
        centerlines: vec![],
        patterns: vec![],
        dim_names: Default::default(),
        next_ids: Default::default(),
        entities: vec![
            pt(1, 0.0, 0.0),
            Entity::Circle {
                id: e(2),
                center: e(1),
                radius: 10.0,
                construction: false,
            },
            pt(3, 12.0, 0.0),
            Entity::Circle {
                id: e(4),
                center: e(3),
                radius: 10.0,
                construction: false,
            },
        ],
        constraints: vec![],
    };
    let sol = solve(&s, &HashMap::new()).unwrap();
    let faces = faces_of(&s, &sol, Plane::XY);
    assert_eq!(faces.faces.len(), 3);
    let mut areas: Vec<f64> = faces
        .faces
        .iter()
        .map(|f| f.region.outer.signed_area())
        .collect();
    areas.sort_by(f64::total_cmp);
    let (p, _) = profile_of(&s, &sol, Plane::XY);
    assert_eq!(p.regions.len(), 1);
    // 半径 10、中心の間 12 の重なり(レンズ)は 2r²·acos(d/2r) − (d/2)·√(4r² − d²) = 200·acos(0.6) − 96
    let pi = std::f64::consts::PI;
    let lens = 200.0 * 0.6f64.acos() - 96.0;
    let want = [lens, 100.0 * pi - lens, 100.0 * pi - lens];
    for (a, w) in areas.iter().zip(want) {
        assert!((a - w).abs() < 0.6, "{areas:?} {want:?}");
    }
    // 円は折れ線にして測るので、内接する分だけ少し小さい
    let union = 200.0 * pi - lens;
    let got = p.regions[0].outer.signed_area();
    assert!(got < union && union - got < 1.0, "{got} {union}");
}

/// 引いた結果の比べ方(引いた要素も重みの分だけわずかに戻るので、0.001 mm まで)。
fn near(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3
}

/// 拘束のない長方形(点 1〜4、辺 11〜14、横と縦の拘束だけ)。
fn free_rectangle() -> Sketch {
    let mut s = rectangle("w", "h");
    s.constraints.retain(|c| {
        matches!(
            c.kind,
            ConstraintKind::Horizontal { .. } | ConstraintKind::Vertical { .. }
        )
    });
    for (id, x, y) in [
        (1, 0.0, 0.0),
        (2, 40.0, 0.0),
        (3, 40.0, 20.0),
        (4, 0.0, 20.0),
    ] {
        s.set_point(e(id), x, y);
    }
    s
}

/// 引いた点を書き換え、引いた点を持たせて解く。
fn drag(s: &Sketch, v: &HashMap<ConstraintId, f64>, moves: &[(u32, f64, f64)]) -> Solution {
    let mut m = s.clone();
    for (id, x, y) in moves {
        m.set_point(e(*id), *x, *y);
    }
    let held = moves.iter().map(|(id, ..)| e(*id)).collect();
    solve_dragging(s, &m, v, &held, None).unwrap()
}

#[test]
fn dragging_the_top_edge_moves_it_and_stretches_the_sides() {
    let s = free_rectangle();
    let v = HashMap::new();
    // 上の辺(点 3・4)を 10 上へ: 上の辺はついてきて、下の辺は動かない
    let sol = drag(&s, &v, &[(3, 40.0, 30.0), (4, 0.0, 30.0)]);
    assert!(near(sol.points[&e(3)], [40.0, 30.0]), "{:?}", sol.points);
    assert!(near(sol.points[&e(4)], [0.0, 30.0]));
    assert!(near(sol.points[&e(1)], [0.0, 0.0]));
    assert!(near(sol.points[&e(2)], [40.0, 0.0]));
    // 隅(点 3)を引くと、隣の隅が 1 つの向きにだけついてきて、向かいの隅は動かない
    let sol = drag(&s, &v, &[(3, 50.0, 25.0)]);
    assert!(near(sol.points[&e(3)], [50.0, 25.0]));
    assert!(near(sol.points[&e(2)], [50.0, 0.0]));
    assert!(near(sol.points[&e(4)], [0.0, 25.0]));
    assert!(near(sol.points[&e(1)], [0.0, 0.0]));
}

#[test]
fn an_unsolvable_constraint_elsewhere_does_not_stop_the_drag() {
    // 取り込み前の点検の 3 つ目: 引いた物とつながらない所に解けない拘束(長さ 10 と 20 が両方
    // 付いた線)があると、ドラッグ全体が止まり、上の辺が 1 mm も動かなかった
    let mut s = free_rectangle();
    let a = s.add_point(100.0, 0.0);
    let b = s.add_point(110.0, 0.0);
    let far = s.add_line(a, b);
    let c10 = s.add_constraint(ConstraintKind::Length {
        line: far,
        value: "10 mm".into(),
    });
    let c20 = s.add_constraint(ConstraintKind::Length {
        line: far,
        value: "20 mm".into(),
    });
    let v = HashMap::from([(c10, 10.0), (c20, 20.0)]);
    let sol = drag(&s, &v, &[(3, 40.0, 30.0), (4, 0.0, 30.0)]);
    assert!(near(sol.points[&e(3)], [40.0, 30.0]), "{:?}", sol.points);
    assert!(near(sol.points[&e(4)], [0.0, 30.0]));
    assert!(near(sol.points[&e(1)], [0.0, 0.0]));
}

#[test]
fn dragging_a_dimensioned_edge_moves_the_whole_rectangle() {
    let mut s = free_rectangle();
    let id = s.add_constraint(ConstraintKind::Length {
        line: e(11),
        value: "40 mm".into(),
    });
    let v = HashMap::from([(id, 40.0)]);
    // 幅が決まった長方形の右の辺を右へ 10: 幅を保って長方形ごと動く
    let sol = drag(&s, &v, &[(2, 50.0, 0.0), (3, 50.0, 20.0)]);
    assert!(near(sol.points[&e(2)], [50.0, 0.0]), "{:?}", sol.points);
    assert!(near(sol.points[&e(1)], [10.0, 0.0]));
    assert!(near(sol.points[&e(4)], [10.0, 20.0]));
}

#[test]
fn a_fixed_corner_keeps_the_dragged_edge_on_its_allowed_direction() {
    let mut s = free_rectangle();
    s.add_constraint(ConstraintKind::Fix { point: e(1) });
    let v = HashMap::new();
    // 左下が固定: 上の辺を右上へ引いても、横には動かず上へだけ動く
    let sol = drag(&s, &v, &[(3, 50.0, 30.0), (4, 10.0, 30.0)]);
    assert!(near(sol.points[&e(1)], [0.0, 0.0]));
    assert!((sol.points[&e(4)][0]).abs() < 1e-3, "{:?}", sol.points);
    assert!((sol.points[&e(4)][1] - 30.0).abs() < 1e-3);
}

/// 円弧の開き(度。始点から反時計回り)。
fn sweep_deg(sol: &Solution, center: EntityId, start: EntityId, end: EntityId) -> f64 {
    let (c, p, q) = (sol.points[&center], sol.points[&start], sol.points[&end]);
    ((q[1] - c[1]).atan2(q[0] - c[0]) - (p[1] - c[1]).atan2(p[0] - c[0]))
        .rem_euclid(std::f64::consts::TAU)
        .to_degrees()
}

/// 点 `p` を `path` の位置へ順に引き、フレームごとに前の解から続けて解く(画面のドラッグと同じ)。
fn drag_along(
    s: &Sketch,
    v: &HashMap<ConstraintId, f64>,
    p: EntityId,
    path: impl Iterator<Item = [f64; 2]>,
    mut each: impl FnMut(&Solution),
) -> Solution {
    let mut start = s.clone();
    start.adopt_solution(&solve(s, v).unwrap());
    let held = std::collections::BTreeSet::from([p]);
    let mut last: Option<Solution> = None;
    for [x, y] in path {
        let mut m = start.clone();
        m.set_point(p, x, y);
        let sol = solve_dragging(&start, &m, v, &held, last.as_ref()).unwrap();
        each(&sol);
        last = Some(sol);
    }
    last.unwrap()
}

#[test]
fn dragging_a_slot_end_all_the_way_around_keeps_the_slot() {
    let mut s = Sketch::default();
    let [top, bottom, left, right] = s
        .add_slot((None, [0.0, 0.0]), (None, [30.0, 0.0]), 5.0)
        .unwrap();
    let arc = |s: &Sketch, id| match s.entity(id) {
        Some(Entity::Arc {
            center, start, end, ..
        }) => (*center, *start, *end),
        _ => unreachable!(),
    };
    let ends = |s: &Sketch, id| match s.entity(id) {
        Some(Entity::Line { a, b, .. }) => (*a, *b),
        _ => unreachable!(),
    };
    let (c1, ..) = arc(&s, left);
    let (c2, ..) = arc(&s, right);
    let v = HashMap::new();
    // 右の中心を、左の中心のまわりに 3° ずつ 1 周させる
    let path = (1..=120).map(|k| {
        let t = (k as f64 * 3.0).to_radians();
        [30.0 * t.cos(), 30.0 * t.sin()]
    });
    let check = |sol: &Solution| {
        for id in [left, right] {
            let (c, a, b) = arc(&s, id);
            let sw = sweep_deg(sol, c, a, b);
            assert!((sw - 180.0).abs() < 1e-3, "端の円弧が半円でない: {sw}");
            let (p, q) = (sol.points[&c], sol.points[&a]);
            let r = (q[0] - p[0]).hypot(q[1] - p[1]);
            assert!((r - 5.0).abs() < 0.05, "幅が変わった: 半径 {r}");
        }
        // 2 本の線は両端の中心の同じ側(交差していない)
        for line in [top, bottom] {
            let (a, b) = ends(&s, line);
            let (pa, pb) = (sol.points[&a], sol.points[&b]);
            let side =
                |c: [f64; 2]| (pb[0] - pa[0]) * (c[1] - pa[1]) - (pb[1] - pa[1]) * (c[0] - pa[0]);
            assert!(
                side(sol.points[&c1]) * side(sol.points[&c2]) > 0.0,
                "線が交差した"
            );
        }
    };
    let sol = drag_along(&s, &v, c2, path, check);
    // 1 周して元の所へ戻った
    assert!(near(sol.points[&c2], [30.0, 0.0]), "{:?}", sol.points[&c2]);
}

/// 長穴の片方の中心を引く: もう片方の中心は動かず、その円弧の端が回って付いて来る
/// (Kie のレビュー K6「掴んだ点以外は、一緒に動かす必要のある物だけ動く」)。
#[test]
fn dragging_one_slot_center_keeps_the_other_center_still() {
    let mut s = Sketch::default();
    let [top, bottom, left, right] = s
        .add_slot((None, [0.0, 0.0]), (None, [30.0, 0.0]), 5.0)
        .unwrap();
    let center = |id| match s.entity(id) {
        Some(Entity::Arc { center, .. }) => *center,
        _ => unreachable!(),
    };
    let (c1, c2) = (center(left), center(right));
    let v = HashMap::new();
    // 右の中心を斜め上へ、少しずつ引く(画面のドラッグと同じ)
    let path = (1..=20).map(|k| {
        let t = k as f64 / 20.0;
        [30.0 + 4.0 * t, 8.0 * t]
    });
    let sol = drag_along(&s, &v, c2, path, |sol| {
        let p = sol.points[&c1];
        assert!(
            p[0].abs() < 1e-9 && p[1].abs() < 1e-9,
            "掴んでいない中心が動いた: {p:?}"
        );
    });
    assert!(near(sol.points[&c2], [34.0, 8.0]), "{:?}", sol.points[&c2]);
    // 長穴の形(線は両方の円弧に接し、幅はそのまま)は保つ
    assert!(
        matches!(sol.status, SolveStatus::UnderConstrained { .. }),
        "{:?}",
        sol.status
    );
    for line in [top, bottom] {
        let Some(Entity::Line { a, b, .. }) = s.entity(line) else {
            unreachable!()
        };
        let (pa, pb) = (sol.points[a], sol.points[b]);
        for c in [c1, c2] {
            let q = sol.points[&c];
            let d = ((pb[0] - pa[0]) * (q[1] - pa[1]) - (pb[1] - pa[1]) * (q[0] - pa[0])).abs()
                / (pb[0] - pa[0]).hypot(pb[1] - pa[1]);
            assert!((d - 5.0).abs() < 1e-3, "線と中心の距離 {d}");
        }
    }
}

/// 長方形の隅を引く: 向かいの隅はまったく動かない(全体に動きを散らさない)。
#[test]
fn dragging_a_corner_leaves_the_opposite_corner_exactly() {
    let s = free_rectangle();
    let v = HashMap::new();
    let sol = drag_along(
        &s,
        &v,
        e(3),
        (1..=10).map(|k| [40.0 + k as f64, 20.0 + 0.5 * k as f64]),
        |sol| {
            let p = sol.points[&e(1)];
            assert!(p[0].abs() < 1e-12 && p[1].abs() < 1e-12, "{p:?}");
        },
    );
    assert!(near(sol.points[&e(3)], [50.0, 25.0]));
}

#[test]
fn an_arc_end_dragged_over_the_other_end_stops_instead_of_turning_inside_out() {
    let mut s = Sketch::default();
    let a = s.add_point(0.0, 0.0);
    let b = s.add_point(20.0, 0.0);
    let c = s.add_point(10.0, -5.0);
    let arc = s.add_arc(c, b, a);
    let v = HashMap::new();
    let before = sweep_deg(&solve(&s, &v).unwrap(), c, b, a);
    // 終点 a を、始点 b の上を越えて下へ回す: 開きが 0 を越えて 360° 近くへ裏返る手前で止まる
    let path = (1..=60).map(|k| {
        let t = (k as f64 * 5.0).to_radians();
        [12.0 * (1.0 - t.cos()), 12.0 * t.sin()]
    });
    let mut prev = before;
    drag_along(&s, &v, a, path, |sol| {
        let sw = sweep_deg(sol, c, b, a);
        assert!(
            (sw - prev).abs() < 90.0,
            "円弧が裏返った: {prev:.1}° → {sw:.1}°"
        );
        prev = sw;
    });
    let _ = arc;
}

#[test]
fn changing_an_angle_dimension_turns_the_line_instead_of_flipping_it() {
    use ConstraintKind as K;
    for (from, to) in [(30.0_f64, 150.0_f64), (20.0, 170.0), (150.0, 10.0)] {
        let mut s = Sketch::default();
        let o = s.add_point(0.0, 0.0);
        let a = s.add_point(30.0, 0.0);
        let t = from.to_radians();
        let b = s.add_point(30.0 * t.cos(), 30.0 * t.sin());
        let l1 = s.add_line(o, a);
        let l2 = s.add_line(o, b);
        s.add_constraint(K::Fix { point: o });
        s.add_constraint(K::Horizontal { line: l1 });
        let id = s.add_constraint(K::Angle {
            a: l1,
            b: l2,
            value: format!("{from} deg"),
        });
        s.adopt_solution(&solve(&s, &HashMap::from([(id, t)])).unwrap());
        // 値だけを変えて解き直す(保存されている位置は前の値の形)
        let sol = solve(&s, &HashMap::from([(id, to.to_radians())])).unwrap();
        let p = sol.points[&b];
        let got = p[1].atan2(p[0]).to_degrees();
        assert!(
            (got - to).abs() < 1e-6,
            "{from}° → {to}° で線が {got:.1}° を向いた(裏返り)"
        );
    }
}

#[test]
fn a_centerline_is_construction_and_survives_renumbering_and_saving() {
    let mut s = Sketch::default();
    let a = s.add_point(0.0, 0.0);
    let b = s.add_point(0.0, 10.0);
    let axis = s.add_line(a, b);
    let c = s.add_point(5.0, 0.0);
    let d = s.add_point(5.0, 10.0);
    let side = s.add_line(c, d);
    s.toggle_centerline(axis);
    assert!(s.is_centerline(axis));
    assert!(s.entity(axis).unwrap().is_construction(), "中心線は補助線");
    // 点や円は中心線にならない
    s.toggle_centerline(a);
    assert_eq!(s.centerlines, vec![axis]);
    // 保存して読み戻しても中心線のまま
    let back: Sketch = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(back.centerlines, vec![axis]);
    // 前の要素を消して番号を詰めても付いて行く
    let mut t = s.clone();
    t.remove_entities(&[side]);
    let mut u = Sketch::default();
    let p = u.add_point(9.0, 0.0);
    let q = u.add_point(9.0, 1.0);
    let early = u.add_line(p, q);
    let clip = t.copy_entities(&[axis]).unwrap();
    let pasted = u.paste(&clip, [0.0, 0.0]);
    let pasted_line = *pasted.last().unwrap();
    u.toggle_centerline(pasted_line);
    u.remove_entities(&[early]);
    let moved = u.renumber_after(0, 0);
    let now = moved.get(&pasted_line).copied().unwrap_or(pasted_line);
    assert_ne!(now, pasted_line, "番号が詰まる");
    assert_eq!(u.centerlines, vec![now]);
    // 補助線を外すと中心線でもなくなる
    s.toggle_construction(axis);
    assert!(!s.is_centerline(axis));
    // もう一度で戻る
    s.toggle_centerline(axis);
    s.toggle_centerline(axis);
    assert!(!s.is_centerline(axis) && !s.entity(axis).unwrap().is_construction());
    // 消すと一覧からも消える
    s.toggle_centerline(axis);
    s.remove_entities(&[axis]);
    assert!(s.centerlines.is_empty());
}

#[test]
fn removed_ids_are_not_reused() {
    let mut s = Sketch::default();
    let a = s.add_point(0.0, 0.0);
    let b = s.add_point(10.0, 0.0);
    let line = s.add_line(a, b);
    let dim = s.add_constraint(ConstraintKind::Distance {
        a,
        b,
        value: "10".into(),
    });
    // 最後の点と寸法を消して描き直しても、消した番号は配られない(参照が付き替わらない)
    s.remove_entities(&[line, b]);
    s.remove_constraint(dim);
    let c = s.add_point(5.0, 5.0);
    assert!(c.0 > line.0, "消した番号 {} を使い回した: {}", line.0, c.0);
    let fixed = s.add_constraint(ConstraintKind::Fix { point: c });
    assert!(fixed.0 > dim.0);
    // 保存して読み直しても控えが残る
    let back: Sketch = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(back.next_ids, s.next_ids);
    let mut back = back;
    assert!(back.add_point(1.0, 1.0).0 > c.0);
    // 控えの無い古いスケッチは今ある最大+1 から数える
    let mut old: Sketch =
        serde_json::from_str(r#"{"entities":[{"type":"point","id":7,"x":0,"y":0}]}"#).unwrap();
    assert_eq!(old.add_point(1.0, 0.0), EntityId(8));
}

#[test]
fn blank_sketch_is_not_fully_constrained() {
    let s = Sketch::default();
    let sol = solve(&s, &Default::default()).unwrap();
    assert!(sol.is_blank());
    assert_eq!(crate::commands::status_text(&sol), crate::NO_SHAPE);
    let r = rectangle("w", "h");
    let sol = solve(&r, &values(&r, &[(6, 40.0), (7, 25.0)])).unwrap();
    assert!(!sol.is_blank());
    assert_ne!(crate::commands::status_text(&sol), crate::NO_SHAPE);
}

#[test]
fn an_arc_is_hit_only_where_it_is_drawn() {
    // 中心 (0, 0)・半径 10 の四分円(右から上へ)
    let mut s = Sketch::default();
    let c = s.add_point(0.0, 0.0);
    let a = s.add_point(10.0, 0.0);
    let b = s.add_point(0.0, 10.0);
    let arc = s.add_arc(c, a, b);
    let sol = solve(&s, &Default::default()).unwrap();
    let curve_at = |x, y| {
        s.hits(&sol, x, y, 0.5)
            .into_iter()
            .find(|h| h.entity == arc)
    };
    assert!(curve_at(7.07, 7.1).is_some(), "円弧の上");
    // 円の続き(円弧の無い左側)を押しても円弧は選ばれない
    assert!(curve_at(-10.0, 0.1).is_none());
    assert!(curve_at(0.0, -10.0).is_none());
    // 端の近くは、円の外へ少し出ても円弧に当たる
    assert!(curve_at(10.2, -0.2).is_some());
}

// 他のソフトで起きた不具合が JC では起きないことを、症状のまま確かめる(判定「試験だけ」)。

#[test]
fn a_parallelogram_linkage_with_redundant_constraints_can_be_dragged() {
    // 10-24 t7: 平行の拘束が余分に重なったリンク機構(下の辺を固定した平行四辺形)を引いても
    // 固まらず、腕の長さを保って回る
    use ConstraintKind as K;
    let mut s = Sketch::default();
    let p: Vec<EntityId> = [[0.0, 0.0], [40.0, 0.0], [50.0, 20.0], [10.0, 20.0]]
        .iter()
        .map(|[x, y]| s.add_point(*x, *y))
        .collect();
    let l: Vec<EntityId> = (0..4).map(|i| s.add_line(p[i], p[(i + 1) % 4])).collect();
    s.add_constraint(K::Fix { point: p[0] });
    s.add_constraint(K::Fix { point: p[1] });
    let arm = 500f64.sqrt();
    let mut v = HashMap::new();
    for (line, value) in [(l[1], arm), (l[2], 40.0), (l[3], arm)] {
        let c = s.add_constraint(K::Length {
            line,
            value: format!("{value} mm"),
        });
        v.insert(c, value);
    }
    s.add_constraint(K::Parallel { a: l[0], b: l[2] });
    s.add_constraint(K::Parallel { a: l[1], b: l[3] });
    let mut m = s.clone();
    m.set_point(p[2], 55.0, 15.0);
    let held = std::collections::BTreeSet::from([p[2]]);
    let sol = solve_dragging(&s, &m, &v, &held, None).unwrap();
    let d = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]);
    assert!(
        d(sol.points[&p[2]], [50.0, 20.0]) > 1.0,
        "動く: {:?}",
        sol.points
    );
    assert!((d(sol.points[&p[2]], sol.points[&p[1]]) - arm).abs() < 1e-6);
    assert!((d(sol.points[&p[3]], sol.points[&p[0]]) - arm).abs() < 1e-6);
    assert!((d(sol.points[&p[3]], sol.points[&p[2]]) - 40.0).abs() < 1e-6);
}

#[test]
fn a_circle_tangent_to_two_sides_does_not_move_a_free_rectangle() {
    // 10-22 t2(起きない見込みの確かめ): 固定していない長方形の 2 辺に接する円を描いても、
    // 接する位置に置いてから拘束を付けるので、長方形の隅は動かず形が歪まない
    let mut s = free_rectangle();
    let corners: Vec<(EntityId, [f64; 2])> = s
        .entities
        .iter()
        .filter_map(|e| match e {
            Entity::Point { id, x, y } => Some((*id, [*x, *y])),
            _ => None,
        })
        .collect();
    let circle = s
        .add_circle_tangent_two(e(11), e(14), [6.0, 5.0])
        .expect("描ける");
    let sol = solve(&s, &HashMap::new()).unwrap();
    for (id, at) in corners {
        assert!(
            close(sol.points[&id], at),
            "{id:?}: {at:?} → {:?}",
            sol.points[&id]
        );
    }
    let Some(Entity::Circle { center, .. }) = s.entity(circle).cloned() else {
        panic!("円")
    };
    let (c, r) = (sol.points[&center], sol.radii[&circle]);
    // 下の辺(y = 0)と左の辺(x = 0)に接する
    assert!(
        (c[1] - r).abs() < 1e-6 && (c[0] - r).abs() < 1e-6,
        "{c:?} {r}"
    );
}
