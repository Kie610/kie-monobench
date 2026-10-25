use crate::curve::{Shape, segment_point_distance};
use crate::*;
use std::collections::HashMap;
use std::f64::consts::PI;

/// 寸法の式(`12.5 mm` の形)を数にして解く。
fn solve_mm(s: &Sketch) -> Solution {
    let values: HashMap<ConstraintId, f64> = s
        .constraints
        .iter()
        .filter(|c| !c.reference)
        .filter_map(|c| {
            let v = c.kind.value()?;
            Some((c.id, v.trim_end_matches("mm").trim().parse().ok()?))
        })
        .collect();
    solve(s, &values).expect("解ける")
}

fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// 楕円(中心 (0, 0)、長軸の端 (30, 0)、短半径 12)と、その長軸・短軸の補助線。
fn ellipse() -> (Sketch, EntityId, EntityId, EntityId, EntityId) {
    let mut s = Sketch::default();
    let id = s
        .add_ellipse((None, [0.0, 0.0]), (None, [30.0, 0.0]), [4.0, 12.0])
        .unwrap();
    let Some(Entity::Ellipse {
        center,
        major,
        minor,
        ..
    }) = s.entity(id).cloned()
    else {
        panic!("楕円")
    };
    assert_eq!(s.pos(minor), Some([0.0, 12.0]), "短軸の端は長軸に直角");
    (s, id, center, major, minor)
}

/// 曲線の上の点が、区間の並び(近似)から許容差の中にあるか。逆に、区間の上の点が曲線の近くに
/// あるかも測る。測った最大の隔たりを返す。
fn approximation_error(shape: &Shape, segs: &[jc_core::Segment]) -> f64 {
    let (lo, hi) = shape.domain();
    let mut worst: f64 = 0.0;
    for i in 0..=4000 {
        let p = shape.at(lo + (hi - lo) * i as f64 / 4000.0);
        let d = segs
            .iter()
            .map(|s| segment_point_distance(s, p))
            .fold(f64::INFINITY, f64::min);
        worst = worst.max(d);
    }
    for s in segs {
        for p in s.polyline(256) {
            worst = worst.max(shape.closest(p).1);
        }
    }
    worst
}

#[test]
fn an_ellipse_is_fully_constrained_by_its_center_axis_and_two_radii() {
    let (mut s, id, center, major, minor) = ellipse();
    // 中心・長軸の端・短軸の端の 6 つから、軸が直角の 1 つを引いて 5 の自由度
    let sol = solve_mm(&s);
    assert_eq!(sol.status, SolveStatus::UnderConstrained { dof: 5 });
    let axis = EntityId(id.0 + 1);
    assert!(
        matches!(s.entity(axis), Some(Entity::Line { a, b, construction: true, .. }) if *a == center && *b == major)
    );
    s.add_constraint(ConstraintKind::Fix { point: center });
    s.add_constraint(ConstraintKind::Horizontal { line: axis });
    // 楕円を 1 つ選んだ寸法は長半径
    let k = dimension_for(&s, &sol, &[id]).unwrap();
    assert_eq!(
        k,
        ConstraintKind::Distance {
            a: center,
            b: major,
            value: String::new()
        }
    );
    s.add_constraint(ConstraintKind::Distance {
        a: center,
        b: major,
        value: "40 mm".into(),
    });
    s.add_constraint(ConstraintKind::Distance {
        a: center,
        b: minor,
        value: "15 mm".into(),
    });
    let sol = solve_mm(&s);
    assert_eq!(sol.status, SolveStatus::WellConstrained, "{:?}", sol.status);
    assert!(dist(sol.points[&major], [40.0, 0.0]) < 1e-6);
    assert!(dist(sol.points[&minor], [0.0, 15.0]) < 1e-6);
    assert!(sol.fixed.contains(&id));
}

#[test]
fn an_ellipse_is_one_closed_region_within_the_arc_tolerance() {
    let (s, id, ..) = ellipse();
    let sol = solve_mm(&s);
    let (profile, unused) = profile_of(&s, &sol, jc_core::Plane::XY);
    assert_eq!(profile.regions.len(), 1, "{unused:?}");
    assert!(unused.is_empty(), "{unused:?}");
    let area = profile.regions[0].outer.signed_area();
    assert!((area - PI * 30.0 * 12.0).abs() < 0.5, "{area}");
    assert_eq!(faces_of(&s, &sol, jc_core::Plane::XY).faces.len(), 1);
    // 近似は線分と円弧だけで、許容差の中
    let segs: Vec<jc_core::Segment> = entity_segments(&s, &sol, false)
        .into_iter()
        .filter(|(e, _)| *e == id)
        .map(|(_, g)| g)
        .collect();
    assert!(segs.len() >= 4);
    let shape = s.solved_shape(&sol, id).unwrap();
    let err = approximation_error(&shape, &segs);
    assert!(err <= ARC_TOLERANCE + 1e-9, "{err}");
    // 補助線(軸)は輪郭に入らない
    assert_eq!(
        curves_of(&s, &sol, jc_core::Plane::XY).segments.len(),
        segs.len()
    );
}

#[test]
fn a_closed_spline_is_one_closed_region_within_the_arc_tolerance() {
    let mut s = Sketch::default();
    let pts: Vec<Spot> = [
        [0.0, 0.0],
        [20.0, -4.0],
        [35.0, 8.0],
        [22.0, 25.0],
        [5.0, 18.0],
    ]
    .iter()
    .map(|p| (None, *p))
    .collect();
    let id = s.add_spline(&pts, false, true).unwrap();
    let sol = solve_mm(&s);
    let (profile, unused) = profile_of(&s, &sol, jc_core::Plane::XY);
    assert_eq!(profile.regions.len(), 1, "{unused:?}");
    let segs: Vec<jc_core::Segment> = entity_segments(&s, &sol, false)
        .into_iter()
        .map(|(_, g)| g)
        .collect();
    let shape = s.solved_shape(&sol, id).unwrap();
    let err = approximation_error(&shape, &segs);
    assert!(err <= ARC_TOLERANCE + 1e-9, "{err}");
    // 制御点の閉じたスプラインも領域になる
    let mut c = Sketch::default();
    c.add_spline(&pts, true, true).unwrap();
    let sol = solve_mm(&c);
    assert_eq!(profile_of(&c, &sol, jc_core::Plane::XY).0.regions.len(), 1);
}

#[test]
fn an_open_spline_closed_by_a_line_is_a_region() {
    let mut s = Sketch::default();
    let a = s.add_point(0.0, 0.0);
    let b = s.add_point(40.0, 0.0);
    s.add_line(a, b);
    let pts = [
        (Some(b), [40.0, 0.0]),
        (None, [30.0, 15.0]),
        (None, [10.0, 12.0]),
        (Some(a), [0.0, 0.0]),
    ];
    s.add_spline(&pts, false, false).unwrap();
    let sol = solve_mm(&s);
    let (profile, unused) = profile_of(&s, &sol, jc_core::Plane::XY);
    assert_eq!(profile.regions.len(), 1, "{unused:?}");
    assert!(unused.is_empty());
}

/// 切った後の曲線の点が、元の曲線の上にあるか。
fn on_original(original: &Shape, s: &Sketch, piece: EntityId) -> bool {
    let shape = s.shape(piece).unwrap();
    let (lo, hi) = shape.domain();
    (0..=200).all(|i| {
        let p = shape.at(lo + (hi - lo) * i as f64 / 200.0);
        original.closest(p).1 < 1e-7
    })
}

#[test]
fn trimming_an_ellipse_leaves_an_elliptical_arc_on_the_same_ellipse() {
    let (mut s, id, ..) = ellipse();
    let original = s.shape(id).unwrap();
    let (p, q) = (s.add_point(-50.0, 0.0), s.add_point(50.0, 0.0));
    let line = s.add_line(p, q);
    // 予告は上半分
    let sol = solve_mm(&s);
    let span = s.trim_preview(&sol, id, [0.0, 12.0]).unwrap();
    assert!(span.iter().all(|p| p[1] >= -1e-6), "{span:?}");
    assert!(s.trim(id, [0.0, 12.0]));
    let arcs: Vec<EntityId> = s
        .entities
        .iter()
        .filter(|e| matches!(e, Entity::Ellipse { ends: Some(_), .. }))
        .map(|e| e.id())
        .collect();
    assert_eq!(arcs.len(), 1);
    assert!(on_original(&original, &s, arcs[0]));
    let shape = s.shape(arcs[0]).unwrap();
    assert!(
        shape.at(shape.domain().0 + 0.5)[1] < 0.0,
        "下半分が残る {shape:?} {:?}",
        s.entities
    );
    // 端は線の上に拘束される。解いても形は変わらない
    let on_line = s
        .constraints
        .iter()
        .filter(|c| matches!(c.kind, ConstraintKind::PointOnLine { line: l, .. } if l == line))
        .count();
    assert_eq!(on_line, 2);
    let sol = solve_mm(&s);
    assert!(sol.max_residual < 1e-7, "{sol:?}");
    assert!(on_original(&original, &s, arcs[0]));
    // 楕円弧と線で閉じた領域になる(線の外の部分は行き止まりで外れる)
    assert_eq!(profile_of(&s, &sol, jc_core::Plane::XY).0.regions.len(), 1);
    // 楕円弧をもう一度切ると、2 つの楕円弧になる
    let (r, t) = (s.add_point(0.0, -50.0), s.add_point(0.0, 0.0));
    s.add_line(r, t);
    assert!(s.trim(arcs[0], [-29.0, -2.0]));
    let n = s
        .entities
        .iter()
        .filter(|e| matches!(e, Entity::Ellipse { ends: Some(_), .. }))
        .count();
    assert_eq!(n, 1, "左下だけ消える");
}

#[test]
fn trimming_a_spline_between_two_lines_leaves_two_control_splines_of_the_same_shape() {
    let mut s = Sketch::default();
    let pts: Vec<Spot> = [
        [0.0, 0.0],
        [10.0, 8.0],
        [20.0, -2.0],
        [30.0, 6.0],
        [40.0, 0.0],
    ]
    .iter()
    .map(|p| (None, *p))
    .collect();
    let id = s.add_spline(&pts, false, false).unwrap();
    let Some(Entity::Spline { points, .. }) = s.entity(id).cloned() else {
        panic!()
    };
    let original = s.shape(id).unwrap();
    for x in [15.0, 25.0] {
        let (p, q) = (s.add_point(x, -20.0), s.add_point(x, 20.0));
        s.add_line(p, q);
    }
    assert!(s.trim(id, [20.0, -2.0]));
    let pieces: Vec<Entity> = s
        .entities
        .iter()
        .filter(|e| matches!(e, Entity::Spline { .. }))
        .cloned()
        .collect();
    assert_eq!(pieces.len(), 2);
    for e in &pieces {
        let Entity::Spline { control, knots, .. } = e else {
            panic!()
        };
        assert!(*control && knots.is_some(), "制御点のスプラインになる");
        assert!(on_original(&original, &s, e.id()));
    }
    // 外側の端は元の点のまま(ほかの線とつながったまま)、切った端は点になって線の上にある
    let ends: Vec<(EntityId, EntityId)> = pieces.iter().filter_map(Entity::ends).collect();
    assert_eq!(ends[0].0, points[0]);
    assert_eq!(ends[1].1, points[4]);
    for p in [ends[0].1, ends[1].0] {
        assert!(
            s.constraints
                .iter()
                .any(|c| matches!(c.kind, ConstraintKind::PointOnLine { point, .. } if point == p))
        );
    }
    let x = s.pos(ends[0].1).unwrap()[0];
    assert!((x - 15.0).abs() < 1e-7, "{x}");
    let sol = solve_mm(&s);
    assert!(sol.max_residual < 1e-7);
    for e in &pieces {
        let shape = s.solved_shape(&sol, e.id()).unwrap();
        let (lo, hi) = shape.domain();
        for i in 0..=50 {
            let p = shape.at(lo + (hi - lo) * i as f64 / 50.0);
            assert!(original.closest(p).1 < 1e-6, "解いても形は変わらない");
        }
    }
}

#[test]
fn trimming_a_closed_spline_leaves_one_open_control_spline() {
    let mut s = Sketch::default();
    let pts: Vec<Spot> = [[0.0, 0.0], [20.0, 0.0], [20.0, 20.0], [0.0, 20.0]]
        .iter()
        .map(|p| (None, *p))
        .collect();
    let id = s.add_spline(&pts, false, true).unwrap();
    let original = s.shape(id).unwrap();
    // 継ぎ目(最初の点)をまたいで切る
    let (p, q) = (s.add_point(5.0, -20.0), s.add_point(5.0, 40.0));
    s.add_line(p, q);
    assert!(s.trim(id, [-3.0, 10.0]));
    let pieces: Vec<EntityId> = s
        .entities
        .iter()
        .filter(|e| matches!(e, Entity::Spline { closed: false, .. }))
        .map(|e| e.id())
        .collect();
    assert_eq!(pieces.len(), 1);
    assert!(on_original(&original, &s, pieces[0]));
    let shape = s.shape(pieces[0]).unwrap();
    assert!(shape.at(0.5)[0] > 5.0, "右側が残る");
}

#[test]
fn trimming_a_conic_leaves_conics_on_the_same_curve() {
    let mut s = Sketch::default();
    let id = s
        .add_conic(
            (None, [0.0, 0.0]),
            (None, [40.0, 0.0]),
            (None, [20.0, 30.0]),
            0.4,
        )
        .unwrap();
    let original = s.shape(id).unwrap();
    for x in [12.0, 28.0] {
        let (p, q) = (s.add_point(x, -5.0), s.add_point(x, 40.0));
        s.add_line(p, q);
    }
    assert!(s.trim(id, [20.0, 14.0]));
    let pieces: Vec<EntityId> = s
        .entities
        .iter()
        .filter(|e| matches!(e, Entity::Conic { .. }))
        .map(|e| e.id())
        .collect();
    assert_eq!(pieces.len(), 2);
    for p in pieces {
        assert!(on_original(&original, &s, p));
    }
}

#[test]
fn a_point_constrained_onto_curves_solves() {
    let mut s = Sketch::default();
    let pts: Vec<Spot> = [[0.0, 0.0], [10.0, 8.0], [20.0, -2.0], [30.0, 6.0]]
        .iter()
        .map(|p| (None, *p))
        .collect();
    let sp = s.add_spline(&pts, false, false).unwrap();
    let Some(Entity::Spline { points, .. }) = s.entity(sp).cloned() else {
        panic!()
    };
    for p in &points {
        s.add_constraint(ConstraintKind::Fix { point: *p });
    }
    let conic = s
        .add_conic(
            (None, [0.0, 30.0]),
            (None, [30.0, 30.0]),
            (None, [15.0, 50.0]),
            0.6,
        )
        .unwrap();
    let ell = s
        .add_ellipse((None, [60.0, 0.0]), (None, [80.0, 0.0]), [60.0, 10.0])
        .unwrap();
    let mut probes = vec![];
    for (curve, at) in [
        (sp, [12.0, 0.0]),
        (conic, [14.0, 45.0]),
        (ell, [60.0, 13.0]),
    ] {
        let p = s.add_point(at[0], at[1]);
        s.add_constraint(ConstraintKind::PointOnCurve { point: p, curve });
        probes.push((p, curve));
    }
    let sol = solve_mm(&s);
    assert!(sol.max_residual < 1e-7, "{:?}", sol.status);
    for (p, curve) in probes {
        let shape = s.solved_shape(&sol, curve).unwrap();
        let d = shape.closest(sol.points[&p]).1;
        assert!(d < 1e-6, "{curve} {d}");
    }
    // 曲線の上の拘束は、点ごとに自由度を 1 つ減らす(点は曲線に沿ってだけ動ける)
    let mut loose = s.clone();
    loose
        .constraints
        .retain(|c| !matches!(c.kind, ConstraintKind::PointOnCurve { .. }));
    let (with, without) = (sol.dof, solve_mm(&loose).dof);
    assert_eq!(without - with, 3, "{without} → {with}");
}

#[test]
fn curvature_matches_between_an_arc_and_a_spline_at_their_shared_end() {
    let mut s = Sketch::default();
    let c = s.add_point(0.0, 0.0);
    let a0 = s.add_point(10.0, 0.0);
    let a1 = s.add_point(0.0, 10.0);
    let arc = s.add_arc(c, a0, a1);
    let pts = [
        (Some(a1), [0.0, 10.0]),
        (None, [-8.0, 13.0]),
        (None, [-16.0, 12.0]),
        (None, [-25.0, 16.0]),
    ];
    let sp = s.add_spline(&pts, false, false).unwrap();
    let Some(Entity::Spline { points, .. }) = s.entity(sp).cloned() else {
        panic!()
    };
    for p in [c, a0, a1, points[3]] {
        s.add_constraint(ConstraintKind::Fix { point: p });
    }
    s.add_constraint(ConstraintKind::Curvature { a: arc, b: sp });
    let sol = solve_mm(&s);
    assert!(sol.max_residual < 1e-7, "{:?}", sol.status);
    let (ta, ka) = s.curve_end(&sol, arc, true).unwrap();
    let (tb, kb) = s.curve_end(&sol, sp, false).unwrap();
    assert!((ta[0] * tb[1] - ta[1] * tb[0]).abs() < 1e-6, "向きがそろう");
    assert!(ta[0] * tb[0] + ta[1] * tb[1] > 0.0, "折り返さない");
    assert!(
        (ka - 0.1).abs() < 1e-9 && (kb - ka).abs() < 1e-6,
        "{ka} {kb}"
    );
    // 端を共有しない 2 つの曲線には付けられない
    let mut bad = s.clone();
    let (p, q) = (bad.add_point(50.0, 0.0), bad.add_point(60.0, 0.0));
    let far = bad.add_line(p, q);
    bad.add_constraint(ConstraintKind::Curvature { a: far, b: sp });
    assert!(solve(&bad, &HashMap::new()).is_err());
}

#[test]
fn curvature_between_a_line_and_a_spline_makes_the_spline_start_straight() {
    let mut s = Sketch::default();
    let a = s.add_point(-20.0, 0.0);
    let b = s.add_point(0.0, 0.0);
    let line = s.add_line(a, b);
    let pts = [
        (Some(b), [0.0, 0.0]),
        (None, [10.0, 3.0]),
        (None, [20.0, 10.0]),
        (None, [30.0, 12.0]),
    ];
    let sp = s.add_spline(&pts, false, false).unwrap();
    let Some(Entity::Spline { points, .. }) = s.entity(sp).cloned() else {
        panic!()
    };
    for p in [a, b, points[3]] {
        s.add_constraint(ConstraintKind::Fix { point: p });
    }
    s.add_constraint(ConstraintKind::Curvature { a: line, b: sp });
    let sol = solve_mm(&s);
    assert!(sol.max_residual < 1e-7, "{:?}", sol.status);
    let (t, k) = s.curve_end(&sol, sp, false).unwrap();
    assert!(t[1].abs() < 1e-6 && t[0] > 0.0, "{t:?}");
    assert!(k.abs() < 1e-6, "{k}");
}

#[test]
fn tangent_curves_at_a_shared_end_and_at_a_touch() {
    // 端を共有: 線分とスプライン
    let mut s = Sketch::default();
    let a = s.add_point(-20.0, 0.0);
    let b = s.add_point(0.0, 0.0);
    let line = s.add_line(a, b);
    let pts = [
        (Some(b), [0.0, 0.0]),
        (None, [10.0, 6.0]),
        (None, [20.0, 0.0]),
    ];
    let sp = s.add_spline(&pts, false, false).unwrap();
    let Some(Entity::Spline { points, .. }) = s.entity(sp).cloned() else {
        panic!()
    };
    for p in [a, b, points[2]] {
        s.add_constraint(ConstraintKind::Fix { point: p });
    }
    s.add_constraint(ConstraintKind::TangentCurve { a: line, b: sp });
    let sol = solve_mm(&s);
    assert!(sol.max_residual < 1e-7);
    let (t, _) = s.curve_end(&sol, sp, false).unwrap();
    assert!(t[1].abs() < 1e-6 && t[0] > 0.0, "{t:?}");

    // 触れる: 決まった楕円に、片方の端を固定した線分が接する
    let (mut s, id, center, major, minor) = ellipse();
    s.add_constraint(ConstraintKind::Fix { point: center });
    s.add_constraint(ConstraintKind::Fix { point: major });
    s.add_constraint(ConstraintKind::Fix { point: minor });
    let p = s.add_point(-40.0, 20.0);
    let q = s.add_point(40.0, 16.0);
    let l = s.add_line(p, q);
    s.add_constraint(ConstraintKind::Fix { point: p });
    s.add_constraint(ConstraintKind::TangentCurve { a: l, b: id });
    let sol = solve_mm(&s);
    assert!(sol.max_residual < 1e-7, "{:?}", sol.status);
    let shape = s.solved_shape(&sol, id).unwrap();
    let (pp, qq) = (sol.points[&p], sol.points[&q]);
    let d = [qq[0] - pp[0], qq[1] - pp[1]];
    let n = d[0].hypot(d[1]);
    let side: Vec<f64> = (0..720)
        .map(|i| {
            let e = shape.at(PI * i as f64 / 360.0);
            (d[0] * (e[1] - pp[1]) - d[1] * (e[0] - pp[0])) / n
        })
        .collect();
    let (lo, hi) = side
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| {
            (a.min(*v), b.max(*v))
        });
    // 楕円は線の片側にあり、1 点で触れる
    assert!(hi < 1e-3 || lo > -1e-3, "{lo} {hi}");
    assert!(hi.abs().min(lo.abs()) < 1e-3, "{lo} {hi}");
}

#[test]
fn new_curves_round_trip_through_json_and_old_json_still_loads() {
    let (mut s, id, ..) = ellipse();
    let (p, q) = (s.add_point(-50.0, 0.0), s.add_point(50.0, 0.0));
    s.add_line(p, q);
    s.trim(id, [0.0, 12.0]);
    let pts: Vec<Spot> = [[0.0, 30.0], [10.0, 38.0], [20.0, 28.0], [30.0, 36.0]]
        .iter()
        .map(|p| (None, *p))
        .collect();
    let sp = s.add_spline(&pts, false, false).unwrap();
    let cs = s.add_spline(&pts, true, false).unwrap();
    let conic = s
        .add_conic(
            (None, [0.0, 60.0]),
            (None, [30.0, 60.0]),
            (None, [15.0, 80.0]),
            0.3,
        )
        .unwrap();
    let probe = s.add_point(15.0, 70.0);
    s.add_constraint(ConstraintKind::PointOnCurve {
        point: probe,
        curve: conic,
    });
    s.add_constraint(ConstraintKind::TangentCurve { a: sp, b: cs });
    s.add_constraint(ConstraintKind::Curvature { a: sp, b: cs });
    // JSON の数は最短の 10 進で書くので、切った端の細かな端数をそろえておく
    for e in &mut s.entities {
        if let Entity::Point { x, y, .. } = e {
            *x = (*x * 1e6).round() / 1e6;
            *y = (*y * 1e6).round() / 1e6;
        }
    }
    let json = serde_json::to_string(&s).unwrap();
    let back: Sketch = serde_json::from_str(&json).unwrap();
    assert_eq!(back, s);
    assert!(json.contains("\"type\":\"ellipse\""));
    assert!(json.contains("\"type\":\"spline\""));
    assert!(json.contains("\"type\":\"conic\""));
    assert!(json.contains("\"type\":\"curvature\""));
    // 省いた項目は既定の値になる(3 次、開いた、フィット点)
    let short: Sketch = serde_json::from_str(
        r#"{"entities":[{"type":"point","id":1,"x":0,"y":0},{"type":"point","id":2,"x":10,"y":5},
        {"type":"spline","id":3,"points":[1,2]}]}"#,
    )
    .unwrap();
    assert!(matches!(
        short.entities[2],
        Entity::Spline {
            degree: 3,
            control: false,
            closed: false,
            knots: None,
            construction: false,
            ..
        }
    ));
    // 新しい要素の無い、これまでの JSON
    let old: Sketch = serde_json::from_str(
        r#"{"entities":[{"type":"point","id":1,"x":0,"y":0},{"type":"point","id":2,"x":10,"y":0},
        {"type":"point","id":3,"x":10,"y":10},{"type":"line","id":4,"a":1,"b":2},
        {"type":"arc","id":5,"center":1,"start":2,"end":3},{"type":"circle","id":6,"center":3,"radius":2}],
        "constraints":[{"id":1,"type":"fix","point":1},{"id":2,"type":"horizontal","line":4},
        {"id":3,"type":"radius","circle":6,"value":"2 mm"}]}"#,
    )
    .unwrap();
    assert_eq!(old.entities.len(), 6);
    let sol = solve_mm(&old);
    assert!(sol.max_residual < 1e-7);
}

#[test]
fn copying_moving_and_mirroring_carry_the_new_curves() {
    let (mut s, id, ..) = ellipse();
    let pts: Vec<Spot> = [[0.0, 30.0], [10.0, 38.0], [20.0, 28.0]]
        .iter()
        .map(|p| (None, *p))
        .collect();
    let sp = s.add_spline(&pts, false, false).unwrap();
    let before = s.entities.len();
    let copies = s.pattern_linear(&[id, sp], [100.0, 0.0], 2);
    assert!(s.entities.len() > before);
    let moved_ellipse = copies
        .iter()
        .find(|e| matches!(s.entity(**e), Some(Entity::Ellipse { .. })))
        .copied()
        .unwrap();
    let a = s.shape(id).unwrap();
    let b = s.shape(moved_ellipse).unwrap();
    let (pa, pb) = (a.at(1.0), b.at(1.0));
    assert!(dist([pa[0] + 100.0, pa[1]], pb) < 1e-9);
    // コピーと貼り付け
    let clip = s.copy_entities(&[sp]).unwrap();
    let pasted = s.paste(&clip, [0.0, 50.0]);
    assert!(
        pasted
            .iter()
            .any(|e| matches!(s.entity(*e), Some(Entity::Spline { .. })))
    );
    // ミラー: 写した楕円は元の楕円を写した形
    let (p, q) = (s.add_point(-50.0, -100.0), s.add_point(-50.0, 100.0));
    let axis = s.add_line(p, q);
    let out = s.mirror(&[id], axis);
    let m = out
        .iter()
        .find(|e| matches!(s.entity(**e), Some(Entity::Ellipse { .. })))
        .copied()
        .unwrap();
    let mirrored = s.shape(m).unwrap();
    for i in 0..20 {
        let p = a.at(i as f64 * 0.3);
        assert!(mirrored.closest([-100.0 - p[0], p[1]]).1 < 1e-9);
    }
    let sol = solve_mm(&s);
    assert!(sol.max_residual < 1e-7);
    // 消すと、その曲線だけが使っていた点も消える
    let Some(Entity::Spline { points, .. }) = s.entity(sp).cloned() else {
        panic!()
    };
    s.remove_entities(&[sp]);
    assert!(points.iter().all(|p| s.entity(*p).is_none()));
}

#[test]
fn chains_hits_and_offsets_know_the_new_curves() {
    let mut s = Sketch::default();
    let a = s.add_point(-20.0, 0.0);
    let b = s.add_point(0.0, 0.0);
    let line = s.add_line(a, b);
    let pts = [
        (Some(b), [0.0, 0.0]),
        (None, [10.0, 6.0]),
        (None, [20.0, 0.0]),
    ];
    let sp = s.add_spline(&pts, false, false).unwrap();
    let mut chain = s.chain_of(line);
    chain.sort();
    assert_eq!(chain, vec![line, sp]);
    let sol = solve_mm(&s);
    let shape = s.solved_shape(&sol, sp).unwrap();
    let on = shape.at(0.3);
    let hit = s.hit_test(&sol, on[0], on[1] + 0.1, 0.5).unwrap();
    assert_eq!(hit.entity, sp);
    let (p, t, d) = s.curve_closest(&sol, sp, [on[0], on[1] + 0.1]).unwrap();
    assert!(dist(p, on) < 0.1 && d <= 0.1 + 1e-9 && (t[0].hypot(t[1]) - 1.0).abs() < 1e-9);
    // 楕円の外へのオフセットは、2 mm 外を通る閉じたスプライン
    let (mut e, id, ..) = ellipse();
    let made = e.offset(&[id], 2.0, "2 mm", true);
    assert_eq!(made.len(), 1);
    assert!(matches!(
        e.entity(made[0]),
        Some(Entity::Spline { closed: true, .. })
    ));
    let original = e.shape(id).unwrap();
    let off = e.shape(made[0]).unwrap();
    for i in 0..200 {
        let p = off.at(i as f64 / 200.0);
        let d = original.closest(p).1;
        assert!((d - 2.0).abs() < 0.05, "{i} {d}");
    }
}

#[test]
fn dragging_the_minor_axis_end_through_the_center_does_not_turn_the_ellipse_inside_out() {
    let (mut s, _, center, major, minor) = ellipse();
    s.add_constraint(ConstraintKind::Fix { point: center });
    s.add_constraint(ConstraintKind::Fix { point: major });
    let v = HashMap::new();
    let held = std::collections::BTreeSet::from([minor]);
    let mut last: Option<Solution> = None;
    for k in 0..=24 {
        let y = 12.0 - k as f64;
        let mut m = s.clone();
        m.set_point(minor, 0.0, y);
        let sol = solve_dragging(&s, &m, &v, &held, last.as_ref()).unwrap();
        assert!(sol.points[&minor][1] > 0.0, "{k} {:?}", sol.points[&minor]);
        last = Some(sol);
    }
}

#[test]
fn a_long_fit_spline_with_constraints_solves_in_reasonable_time() {
    let mut s = Sketch::default();
    let pts: Vec<Spot> = (0..40)
        .map(|i| {
            let x = i as f64 * 5.0;
            (None, [x, 10.0 * (x / 17.0).sin()])
        })
        .collect();
    let sp = s.add_spline(&pts, false, false).unwrap();
    let Some(Entity::Spline { points, .. }) = s.entity(sp).cloned() else {
        panic!()
    };
    // 始点 (0, 0) で左から入る、半径 40 の円弧(中心 (0, -40)、反時計回り)
    let c = s.add_point(0.0, -40.0);
    let a0 = s.add_point(-40.0, -40.0);
    let arc = s.add_arc(c, points[0], a0);
    for p in [c, a0, points[39]] {
        s.add_constraint(ConstraintKind::Fix { point: p });
    }
    s.add_constraint(ConstraintKind::Curvature { a: arc, b: sp });
    for k in [3.0, 77.0, 151.0] {
        let p = s.add_point(k, 0.0);
        s.add_constraint(ConstraintKind::PointOnCurve {
            point: p,
            curve: sp,
        });
    }
    let t = std::time::Instant::now();
    let sol = solve_mm(&s);
    let took = t.elapsed();
    assert!(sol.max_residual < 1e-7, "{:?}", sol.status);
    assert!(took.as_secs_f64() < 20.0, "{took:?}");
    eprintln!("40 点のスプライン: {took:?} {} 回", sol.iterations);
}
