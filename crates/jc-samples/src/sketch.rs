use jc_core::{Document, FeatureEntry, Op, Param, Transaction};
use jc_sketch::{Constraint, ConstraintId, ConstraintKind as K, Entity, EntityId, Sketch};

/// 動作確認用の文書。幅と高さのパラメータで寸法を決めた、穴あきの板のスケッチ。
pub fn sample_document() -> Document {
    let mut doc = Document::new();
    let ids: Vec<_> = (0..3).map(|_| doc.alloc_param_id()).collect();
    let f = doc.alloc_feature_id();
    let pt = |id, x, y| Entity::Point {
        id: EntityId(id),
        x,
        y,
    };
    let line = |id, a, b| Entity::Line {
        id: EntityId(id),
        a: EntityId(a),
        b: EntityId(b),
        construction: false,
    };
    let e = EntityId;
    let c = |id, kind| Constraint::new(ConstraintId(id), kind);
    let sketch = Sketch {
        plane: Default::default(),
        offset: None,
        entities: vec![
            pt(1, 0.0, 0.0),
            pt(2, 50.0, 2.0),
            pt(3, 52.0, 28.0),
            pt(4, -2.0, 30.0),
            line(11, 1, 2),
            line(12, 2, 3),
            line(13, 3, 4),
            line(14, 4, 1),
            pt(5, 20.0, 12.0),
            Entity::Circle {
                id: e(20),
                center: e(5),
                radius: 4.0,
                construction: false,
            },
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
                    value: "width".into(),
                },
            ),
            c(
                7,
                K::Length {
                    line: e(12),
                    value: "height".into(),
                },
            ),
            c(
                8,
                K::Diameter {
                    circle: e(20),
                    value: "hole".into(),
                },
            ),
            c(
                9,
                K::HorizontalDistance {
                    a: e(1),
                    b: e(5),
                    value: "width / 2".into(),
                },
            ),
            c(
                10,
                K::VerticalDistance {
                    a: e(1),
                    b: e(5),
                    value: "height / 2".into(),
                },
            ),
        ],
        projections: vec![],
        centerlines: vec![],
        patterns: vec![],
        dim_names: Default::default(),
        next_ids: Default::default(),
    };
    let param = |i: usize, name: &str, expr: &str| Op::AddParam {
        index: None,
        param: Param::new(ids[i], name, expr),
    };
    doc.apply(Transaction::new(
        "サンプル",
        vec![
            param(0, "width", "60 mm"),
            param(1, "height", "40 mm"),
            param(2, "hole", "8 mm"),
            Op::InsertFeature {
                index: None,
                entry: FeatureEntry {
                    id: f,
                    type_id: jc_sketch::SKETCH.into(),
                    name: "板の輪郭".into(),
                    data: serde_json::to_value(sketch).expect("JSON にできる"),
                    suppressed: false,
                },
            },
        ],
    ))
    .expect("サンプルの文書は正しい");
    doc.clear_history();
    doc
}

/// 作図の道具の見本(画面の写真に使う)。角の面取りと丸めをした板、並べた穴、外接の六角形、
/// 円弧に沿う長穴、3 点の円弧の縁。寸法は引出し線つきで出る。
pub fn sketch_tools_document() -> Document {
    let mut s = Sketch::default();
    let o = s.add_point(0.0, 0.0);
    s.add_constraint(K::Fix { point: o });
    let l = s
        .add_rectangle((Some(o), [0.0, 0.0]), (None, [100.0, 60.0]))
        .expect("矩形");
    s.add_constraint(K::Length {
        line: l[0],
        value: "100 mm".into(),
    });
    s.add_constraint(K::Length {
        line: l[1],
        value: "60 mm".into(),
    });
    let corner = |s: &Sketch, line: EntityId, first: bool| match s.entity(line) {
        Some(Entity::Line { a, b, .. }) => {
            if first {
                *a
            } else {
                *b
            }
        }
        _ => unreachable!("矩形の辺"),
    };
    let tr = corner(&s, l[1], false);
    s.chamfer_corner(tr, 6.0, "6 mm");
    let tl = corner(&s, l[2], false);
    s.fillet_corner(tl, 6.0, "6 mm");
    let c = s.add_point(15.0, 15.0);
    let hole = s.add_circle(c, 4.0);
    s.add_constraint(K::Diameter {
        circle: hole,
        value: "8 mm".into(),
    });
    s.add_constraint(K::HorizontalDistance {
        a: o,
        b: c,
        value: "15 mm".into(),
    });
    s.add_constraint(K::VerticalDistance {
        a: o,
        b: c,
        value: "15 mm".into(),
    });
    s.pattern_linear(&[hole], [15.0, 0.0], 3);
    s.add_circumscribed_polygon((None, [22.0, 40.0]), [29.0, 40.0], 6);
    if let Some([_, _, cap, _]) =
        s.add_arc_slot((None, [72.0, 18.0]), [88.0, 18.0], [72.0, 34.0], 3.0)
    {
        s.add_constraint(K::Radius {
            circle: cap,
            value: "3 mm".into(),
        });
    }
    let mut doc = Document::new();
    let f = doc.alloc_feature_id();
    doc.apply(Transaction::new(
        "見本",
        vec![Op::InsertFeature {
            index: None,
            entry: FeatureEntry {
                id: f,
                type_id: jc_sketch::SKETCH.into(),
                name: "作図の見本".into(),
                data: serde_json::to_value(s).expect("JSON にできる"),
                suppressed: false,
            },
        }],
    ))
    .expect("見本の文書は正しい");
    doc.clear_history();
    doc
}

/// 寸法の札の見本(長さ・横・縦・角度・直径・半径・参照寸法、置いた札と既定の札)。
/// 札の見た目を画面で確かめるための物。
pub fn sketch_dims_document() -> Document {
    let mut s = Sketch::default();
    let o = s.add_point(0.0, 0.0);
    s.add_constraint(K::Fix { point: o });
    let l = s
        .add_rectangle((Some(o), [0.0, 0.0]), (None, [80.0, 50.0]))
        .expect("矩形");
    s.add_constraint(K::Length {
        line: l[0],
        value: "80 mm".into(),
    });
    let tall = s.add_constraint(K::Length {
        line: l[1],
        value: "50 mm".into(),
    });
    // 既定の札の直径(円の中)と、外へ置いた直径
    let c = s.add_point(25.0, 25.0);
    let hole = s.add_circle(c, 10.0);
    s.add_constraint(K::Diameter {
        circle: hole,
        value: "20 mm".into(),
    });
    let c2 = s.add_point(60.0, 25.0);
    let small = s.add_circle(c2, 6.0);
    let small_d = s.add_constraint(K::Diameter {
        circle: small,
        value: "12 mm".into(),
    });
    s.add_constraint(K::HorizontalDistance {
        a: o,
        b: c,
        value: "25 mm".into(),
    });
    s.add_constraint(K::VerticalDistance {
        a: o,
        b: c,
        value: "25 mm".into(),
    });
    // 斜めの線と角度
    let a = s.add_point(110.0, 0.0);
    let b = s.add_point(160.0, 0.0);
    let t = 35f64.to_radians();
    let e = s.add_point(110.0 + 45.0 * t.cos(), 45.0 * t.sin());
    let base = s.add_line(a, b);
    let slope = s.add_line(a, e);
    s.add_constraint(K::Angle {
        a: base,
        b: slope,
        value: "35 deg".into(),
    });
    s.add_constraint(K::Length {
        line: base,
        value: "50 mm".into(),
    });
    // 円弧と半径
    let ac = s.add_point(150.0, 60.0);
    let a0 = s.add_point(165.0, 60.0);
    let a1 = s.add_point(150.0, 75.0);
    let arc = s.add_arc(ac, a0, a1);
    let r = s.add_constraint(K::Radius {
        circle: arc,
        value: "15 mm".into(),
    });
    // 参照寸法(斜めの線の長さ。測った値を括弧で見せる)
    let driven = s.add_constraint(K::Length {
        line: slope,
        value: "45 mm".into(),
    });
    if let Some(c) = s.constraints.iter_mut().find(|c| c.id == driven) {
        c.reference = true;
    }
    let values = jc_sketch::dimension_values(&s, &|_| None).expect("寸法の値");
    let sol = jc_sketch::solve(&s, &values).expect("解ける");
    s.place_label(small_d, [78.0, 42.0], &sol);
    s.place_label(r, [175.0, 82.0], &sol);
    s.place_label(tall, [94.0, 25.0], &sol);
    let mut doc = Document::new();
    let f = doc.alloc_feature_id();
    doc.apply(Transaction::new(
        "見本",
        vec![Op::InsertFeature {
            index: None,
            entry: FeatureEntry {
                id: f,
                type_id: jc_sketch::SKETCH.into(),
                name: "寸法の見本".into(),
                data: serde_json::to_value(s).expect("JSON にできる"),
                suppressed: false,
            },
        }],
    ))
    .expect("見本の文書は正しい");
    doc.clear_history();
    doc
}

#[cfg(test)]
mod tools_sample_tests {
    #[test]
    fn the_tools_sample_solves_without_conflicts() {
        let doc = super::sketch_tools_document();
        let data = &doc.data().features[0].data;
        let s: jc_sketch::Sketch = serde_json::from_value(data.clone()).unwrap();
        let values = jc_sketch::dimension_values(&s, &|_| None).unwrap();
        let sol = jc_sketch::solve(&s, &values).unwrap();
        assert!(sol.max_residual < 1e-7, "{:?}", sol.status);
        assert!(
            !matches!(
                sol.status,
                jc_sketch::SolveStatus::Inconsistent { .. }
                    | jc_sketch::SolveStatus::OverConstrained { .. }
            ),
            "{:?}",
            sol.status
        );
        let (p, _) = jc_sketch::profile_of(&s, &sol, jc_core::Plane::XY);
        assert_eq!(p.regions.len(), 1, "板 1 つ(穴・六角形・長穴は穴)");
        assert_eq!(p.regions[0].holes.len(), 5);
    }
}
