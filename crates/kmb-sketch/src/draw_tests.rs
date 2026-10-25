use crate::*;
use std::collections::HashMap;
use std::f64::consts::{FRAC_PI_2, TAU};

/// 寸法の式を数として読んで解く(`12 mm` → 12、`30 deg` → rad)。
fn solve_plain(s: &Sketch) -> Solution {
    let values: HashMap<ConstraintId, f64> = s
        .constraints
        .iter()
        .filter_map(|c| {
            let v = c.kind.value()?;
            let n: f64 = v.split_whitespace().next()?.parse().ok()?;
            Some((c.id, if c.kind.is_angle() { n.to_radians() } else { n }))
        })
        .collect();
    solve(s, &values).expect("解ける")
}

fn near(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6
}

fn ends(s: &Sketch, sol: &Solution, line: EntityId) -> ([f64; 2], [f64; 2]) {
    match s.entity(line) {
        Some(Entity::Line { a, b, .. }) => (sol.points[a], sol.points[b]),
        _ => panic!("線分ではない"),
    }
}

fn length(s: &Sketch, sol: &Solution, line: EntityId) -> f64 {
    let (p, q) = ends(s, sol, line);
    (q[0] - p[0]).hypot(q[1] - p[1])
}

fn regions(s: &Sketch, sol: &Solution) -> usize {
    profile_of(s, sol, kmb_core::Plane::XY).0.regions.len()
}

#[test]
fn a_three_point_rectangle_is_tilted_and_stays_square() {
    let mut s = Sketch::default();
    let l = s
        .add_three_point_rectangle((None, [0.0, 0.0]), (None, [8.0, 6.0]), [0.0, 10.0])
        .unwrap();
    let sol = solve_plain(&s);
    assert_eq!(sol.dof, 5, "位置 2、向き 1、幅と高さ 2");
    assert!(matches!(sol.status, SolveStatus::UnderConstrained { .. }));
    assert!((length(&s, &sol, l[0]) - 10.0).abs() < 1e-6);
    // 3 点目から 1 辺目までの距離が高さ(10 × 8 / 10 = 8)
    assert!((length(&s, &sol, l[1]) - 8.0).abs() < 1e-6);
    assert_eq!(regions(&s, &sol), 1);
    // 一直線の 3 点では描かない
    let mut t = Sketch::default();
    assert!(
        t.add_three_point_rectangle((None, [0.0, 0.0]), (None, [5.0, 0.0]), [9.0, 0.0])
            .is_none()
    );
}

#[test]
fn circles_from_two_and_three_points() {
    let mut s = Sketch::default();
    let p = s.add_point(-3.0, 4.0);
    let c = s
        .add_circle_two_points((Some(p), [-3.0, 4.0]), (None, [3.0, -4.0]))
        .unwrap();
    let sol = solve_plain(&s);
    assert!((sol.radii[&c] - 5.0).abs() < 1e-6);
    // 吸着した点は円周の上に拘束する
    assert!(s.constraints.iter().any(|k| k.kind
        == ConstraintKind::PointOnCircle {
            point: p,
            circle: c
        }));

    let mut t = Sketch::default();
    let c = t
        .add_circle_three_points((None, [5.0, 0.0]), (None, [0.0, 5.0]), (None, [-5.0, 0.0]))
        .unwrap();
    let sol = solve_plain(&t);
    assert!((sol.radii[&c] - 5.0).abs() < 1e-6);
    let Some(Entity::Circle { center, .. }) = t.entity(c) else {
        panic!()
    };
    assert!(near(sol.points[center], [0.0, 0.0]));
    assert!(
        t.add_circle_three_points((None, [0.0, 0.0]), (None, [1.0, 1.0]), (None, [2.0, 2.0]))
            .is_none()
    );
}

#[test]
fn a_circle_tangent_to_three_lines_picks_the_one_near_the_click() {
    // 直角三角形 (0,0)-(6,0)-(0,8): 内接円の半径は (6 + 8 - 10) / 2 = 2、中心 (2, 2)
    let mut s = Sketch::default();
    let a = s.add_point(0.0, 0.0);
    let b = s.add_point(6.0, 0.0);
    let c = s.add_point(0.0, 8.0);
    let l = [s.add_line(a, b), s.add_line(b, c), s.add_line(c, a)];
    let circle = s.add_circle_tangent_to_lines(l, [1.5, 1.5]).unwrap();
    let sol = solve_plain(&s);
    assert!((sol.radii[&circle] - 2.0).abs() < 1e-6);
    assert!(matches!(sol.status, SolveStatus::UnderConstrained { .. }));
    // 三角形の外をクリックすれば傍接円(中心は外)
    let mut t = s.clone();
    t.remove_entities(&[circle]);
    let ex = t.add_circle_tangent_to_lines(l, [8.0, 8.0]).unwrap();
    let Some(Entity::Circle { center, radius, .. }) = t.entity(ex).cloned() else {
        panic!()
    };
    let p = t.entity(center).cloned();
    assert!(radius > 2.0);
    assert!(matches!(p, Some(Entity::Point { x, y, .. }) if x + y > 8.0));
}

#[test]
fn a_three_point_arc_goes_through_the_middle_click() {
    for (via, top) in [([0.0, 5.0], true), ([0.0, -5.0], false)] {
        let mut s = Sketch::default();
        let arc = s
            .add_arc_three_points((None, [5.0, 0.0]), (None, [-5.0, 0.0]), (None, via))
            .unwrap();
        let sol = solve_plain(&s);
        let segs = entity_segments(&s, &sol, false);
        let (_, seg) = segs.iter().find(|(id, _)| *id == arc).unwrap();
        let kmb_core::Segment::Arc { center, radius, .. } = seg else {
            panic!("円弧")
        };
        assert!((radius - 5.0).abs() < 1e-6);
        assert!((center[0]).abs() < 1e-6 && center[1].abs() < 1e-6);
        // 上を通るなら (5,0) から反時計回り、下なら (-5,0) から
        let Some(Entity::Arc { start, .. }) = s.entity(arc) else {
            panic!()
        };
        assert_eq!(near(sol.points[start], [5.0, 0.0]), top);
    }
}

#[test]
fn a_circumscribed_hexagon_has_the_given_width_across_flats() {
    let mut s = Sketch::default();
    let sides = s
        .add_circumscribed_polygon((None, [0.0, 0.0]), [5.0, 0.0], 6)
        .unwrap();
    assert_eq!(sides.len(), 6);
    let sol = solve_plain(&s);
    assert_eq!(sol.dof, 4, "中心 2、大きさ 1、向き 1: {:?}", sol.status);
    // 辺の長さは 2 × 5 × tan(30°)
    let want = 10.0 * (std::f64::consts::PI / 6.0).tan();
    for l in &sides {
        assert!((length(&s, &sol, *l) - want).abs() < 1e-6);
    }
    assert_eq!(regions(&s, &sol), 1);
    // 内の補助線の円に直径を付けると、二面幅で決まる(四角形でもひし形に崩れない)
    let mut q = Sketch::default();
    let o = q.add_point(0.0, 0.0);
    q.add_constraint(ConstraintKind::Fix { point: o });
    let sides = q
        .add_circumscribed_polygon((Some(o), [0.0, 0.0]), [4.0, 1.0], 4)
        .unwrap();
    let inner = q
        .entities
        .iter()
        .find_map(|e| match e {
            Entity::Circle { id, .. } => Some(*id),
            _ => None,
        })
        .unwrap();
    q.add_constraint(ConstraintKind::Diameter {
        circle: inner,
        value: "20 mm".into(),
    });
    let sol = solve_plain(&q);
    assert_eq!(sol.dof, 1, "残りは向きだけ");
    for l in &sides {
        assert!((length(&q, &sol, *l) - 20.0).abs() < 1e-6);
    }
}

#[test]
fn overall_and_center_slots() {
    let mut s = Sketch::default();
    let [top, ..] = s
        .add_overall_slot((None, [0.0, 0.0]), (None, [30.0, 0.0]), 5.0)
        .unwrap();
    let sol = solve_plain(&s);
    assert_eq!(
        sol.status,
        SolveStatus::UnderConstrained { dof: 5 },
        "両端の中心 4 と幅 1(接する所で余分な拘束にならない)"
    );
    // 全長 30、幅 10 なら直線の部分は 20
    assert!((length(&s, &sol, top) - 20.0).abs() < 1e-6);
    assert_eq!(regions(&s, &sol), 1);
    assert!(
        s.clone()
            .add_overall_slot((None, [0.0, 0.0]), (None, [8.0, 0.0]), 5.0)
            .is_none()
    );

    let mut t = Sketch::default();
    let c = t.add_point(10.0, 10.0);
    t.add_constraint(ConstraintKind::Fix { point: c });
    let [top, ..] = t
        .add_center_slot((Some(c), [10.0, 10.0]), (None, [20.0, 10.0]), 3.0)
        .unwrap();
    let sol = solve_plain(&t);
    let (p, q) = ends(&t, &sol, top);
    assert!(((p[0] + q[0]) / 2.0 - 10.0).abs() < 1e-6, "中心が真ん中");
    assert!((length(&t, &sol, top) - 20.0).abs() < 1e-6);
}

#[test]
fn an_arc_slot_is_one_closed_region_with_six_freedoms() {
    let mut s = Sketch::default();
    let [outer, inner, cap0, cap1] = s
        .add_arc_slot((None, [0.0, 0.0]), [20.0, 0.0], [0.0, 20.0], 3.0)
        .unwrap();
    let sol = solve_plain(&s);
    assert_eq!(
        sol.status,
        SolveStatus::UnderConstrained { dof: 6 },
        "中心 2、半径、幅、始まりと終わりの角度"
    );
    assert_eq!(regions(&s, &sol), 1);
    let segs = entity_segments(&s, &sol, false);
    let r = |id: EntityId| match segs.iter().find(|(i, _)| *i == id).unwrap().1 {
        kmb_core::Segment::Arc { radius, .. } => radius,
        _ => panic!(),
    };
    assert!((r(outer) - 23.0).abs() < 1e-6);
    assert!((r(inner) - 17.0).abs() < 1e-6);
    assert!((r(cap0) - 3.0).abs() < 1e-6 && (r(cap1) - 3.0).abs() < 1e-6);
    // 幅の寸法で、両端がそろって太る
    s.add_constraint(ConstraintKind::Radius {
        circle: cap0,
        value: "4 mm".into(),
    });
    let sol = solve_plain(&s);
    assert!(sol.max_residual < 1e-7);
    let segs = entity_segments(&s, &sol, false);
    let r2 = |id: EntityId| match segs.iter().find(|(i, _)| *i == id).unwrap().1 {
        kmb_core::Segment::Arc { radius, .. } => radius,
        _ => panic!(),
    };
    assert!((r2(cap1) - 4.0).abs() < 1e-6);
    assert!((r2(outer) - r2(inner) - 8.0).abs() < 1e-6);
}

#[test]
fn a_chamfer_cuts_the_corner_and_keeps_the_distance() {
    let mut s = Sketch::default();
    let l = s
        .add_rectangle((None, [0.0, 0.0]), (None, [20.0, 10.0]))
        .unwrap();
    let (corner, before) = match s.entity(l[0]) {
        Some(Entity::Line { a, .. }) => (*a, s.clone()),
        _ => panic!(),
    };
    let cut = s.chamfer_corner(corner, 3.0, "3 mm").unwrap();
    let sol = solve_plain(&s);
    assert!(sol.max_residual < 1e-7);
    // 斜めの辺は 3√2、角の点は残って補助線でつながる
    assert!((length(&s, &sol, cut) - 18f64.sqrt()).abs() < 1e-6);
    assert!(near(sol.points[&corner], [0.0, 0.0]));
    assert_eq!(regions(&s, &sol), 1);
    assert!(!s.lone_points().contains(&corner), "穴の位置にならない");
    // 距離の寸法を変えると斜めの辺が追う
    let mut t = s.clone();
    for c in &mut t.constraints {
        if let ConstraintKind::Length { value, .. } = &mut c.kind
            && value == "3 mm"
        {
            *value = "4 mm".into();
        }
    }
    let sol = solve_plain(&t);
    assert!((length(&t, &sol, cut) - 32f64.sqrt()).abs() < 1e-6);
    // 線分が短すぎれば落とさない
    let mut u = before;
    assert!(u.chamfer_corner(corner, 15.0, "15 mm").is_none());
}

#[test]
fn concentric_collinear_and_circle_tangent_constraints() {
    use ConstraintKind as K;
    let mut s = Sketch::default();
    let c1 = s.add_point(0.0, 0.0);
    s.add_constraint(K::Fix { point: c1 });
    let a = s.add_circle(c1, 10.0);
    let c2 = s.add_point(1.0, 2.0);
    let b = s.add_circle(c2, 4.0);
    s.add_constraint(K::Concentric { a, b });
    let sol = solve_plain(&s);
    assert!(near(sol.points[&c2], [0.0, 0.0]));

    // 外から接する(初めの位置が離れている)
    let mut t = Sketch::default();
    let p = t.add_point(0.0, 0.0);
    t.add_constraint(K::Fix { point: p });
    let a = t.add_circle(p, 5.0);
    let q = t.add_point(12.0, 0.0);
    let b = t.add_circle(q, 3.0);
    t.add_constraint(K::TangentCircles { a, b });
    let sol = solve_plain(&t);
    let d = sol.points[&q][0].hypot(sol.points[&q][1]);
    assert!((d - (sol.radii[&a] + sol.radii[&b])).abs() < 1e-6);
    // 内から接する(小さい円が中にある)
    let mut u = Sketch::default();
    let p = u.add_point(0.0, 0.0);
    let a = u.add_circle(p, 10.0);
    let q = u.add_point(4.0, 0.0);
    let b = u.add_circle(q, 3.0);
    u.add_constraint(K::TangentCircles { a, b });
    let sol = solve_plain(&u);
    let d = (sol.points[&q][0] - sol.points[&p][0]).hypot(sol.points[&q][1] - sol.points[&p][1]);
    assert!((d - (sol.radii[&a] - sol.radii[&b]).abs()).abs() < 1e-6);

    let mut v = Sketch::default();
    let (p0, p1) = (v.add_point(0.0, 0.0), v.add_point(10.0, 0.0));
    let (p2, p3) = (v.add_point(15.0, 1.0), v.add_point(25.0, 2.0));
    let l1 = v.add_line(p0, p1);
    let l2 = v.add_line(p2, p3);
    v.add_constraint(K::Fix { point: p0 });
    v.add_constraint(K::Horizontal { line: l1 });
    v.add_constraint(K::Collinear { a: l1, b: l2 });
    let sol = solve_plain(&v);
    assert!(sol.points[&p2][1].abs() < 1e-6 && sol.points[&p3][1].abs() < 1e-6);
}

#[test]
fn a_linear_pattern_copies_shape_constraints() {
    let mut s = Sketch::default();
    let l = s
        .add_rectangle((None, [0.0, 0.0]), (None, [4.0, 2.0]))
        .unwrap();
    s.add_constraint(ConstraintKind::Length {
        line: l[0],
        value: "4 mm".into(),
    });
    let ids: Vec<EntityId> = l.to_vec();
    let new = s.pattern_linear(&ids, [10.0, 0.0], 3);
    assert_eq!(
        new.iter()
            .filter(|id| matches!(s.entity(**id), Some(Entity::Line { .. })))
            .count(),
        8
    );
    let sol = solve_plain(&s);
    assert_eq!(regions(&s, &sol), 3);
    // 写しにも長さの寸法(同じ式)が付く
    let lengths = s
        .constraints
        .iter()
        .filter(|c| matches!(c.kind, ConstraintKind::Length { .. }))
        .count();
    assert_eq!(lengths, 3);
    // 単独の点を並べると、穴の位置の点が増える
    let mut p = Sketch::default();
    let a = p.add_point(5.0, 0.0);
    p.pattern_circular(&[a], [0.0, 0.0], 6, TAU);
    assert_eq!(p.lone_points().len(), 6);
    let sol = solve_plain(&p);
    assert!(sol.points.values().any(|q| near(*q, [-5.0, 0.0])));
}

#[test]
fn a_circular_pattern_turns_horizontal_into_an_angle() {
    let mut s = Sketch::default();
    let l = s
        .add_rectangle((None, [10.0, -1.0]), (None, [14.0, 1.0]))
        .unwrap();
    s.pattern_circular(&l, [0.0, 0.0], 4, TAU);
    let sol = solve_plain(&s);
    assert!(sol.max_residual < 1e-7, "{:?}", sol.status);
    assert_eq!(regions(&s, &sol), 4);
    let angles = s
        .constraints
        .iter()
        .filter(|c| matches!(c.kind, ConstraintKind::Angle { .. }))
        .count();
    assert_eq!(angles, 3 * 4, "写し 3 組 × 水平・垂直 4");
}

#[test]
fn move_rotate_and_scale_the_selection() {
    use ConstraintKind as K;
    let mut s = Sketch::default();
    let l = s
        .add_rectangle((None, [0.0, 0.0]), (None, [4.0, 2.0]))
        .unwrap();
    s.add_constraint(K::Length {
        line: l[0],
        value: "4 mm".into(),
    });
    let mut m = s.clone();
    m.move_entities(&l, [5.0, 5.0]);
    let sol = solve_plain(&m);
    assert!(sol.points.values().any(|p| near(*p, [9.0, 7.0])));

    let mut r = s.clone();
    assert_eq!(
        r.rotate_entities(&l, [0.0, 0.0], FRAC_PI_2),
        0,
        "90° は入れ替え"
    );
    let sol = solve_plain(&r);
    assert!(sol.max_residual < 1e-7);
    assert!(sol.points.values().any(|p| near(*p, [-2.0, 4.0])));
    let mut r = s.clone();
    assert_eq!(
        r.rotate_entities(&l, [0.0, 0.0], 0.3),
        4,
        "水平・垂直を外す"
    );
    let sol = solve_plain(&r);
    assert!(sol.max_residual < 1e-7);

    let mut k = s.clone();
    k.scale_entities(&l, [0.0, 0.0], 2.5);
    assert!(
        k.constraints
            .iter()
            .any(|c| c.kind.value() == Some("10 mm"))
    );
    let sol = solve_plain(&k);
    assert!(sol.points.values().any(|p| near(*p, [10.0, 5.0])));
}

#[test]
fn a_filleted_corner_is_not_reported_as_over_constrained() {
    let mut s = Sketch::default();
    let l = s
        .add_rectangle((None, [0.0, 0.0]), (None, [20.0, 10.0]))
        .unwrap();
    let Some(Entity::Line { a: corner, .. }) = s.entity(l[0]).cloned() else {
        panic!()
    };
    s.fillet_corner(corner, 2.0, "2 mm").unwrap();
    let sol = solve_plain(&s);
    // 端を共有して接する所は、半径と線が直角の式で解く(距離の式だと余分に見えていた)
    assert_eq!(sol.status, SolveStatus::UnderConstrained { dof: 4 });
}

/// 3 点の円弧の途中の点が既にある点に吸い付いていれば、その点を円弧の上に
/// 拘束する(後で動かしても通ったまま)。何も無い所をクリックした時は点を残さない。
#[test]
fn a_three_point_arc_keeps_a_snapped_middle_point_on_it() {
    let mut s = Sketch::default();
    let p = s.add_point(0.0, 5.0);
    let arc = s
        .add_arc_three_points(
            (None, [5.0, 0.0]),
            (None, [-5.0, 0.0]),
            (Some(p), [0.0, 5.0]),
        )
        .unwrap();
    assert!(
        s.constraints.iter().any(|c| c.kind
            == ConstraintKind::PointOnCircle {
                point: p,
                circle: arc
            }),
        "吸い付いた途中の点が円弧の上に拘束されていない: {:?}",
        s.constraints
    );
    let mut t = Sketch::default();
    let before = t.entities.len();
    t.add_arc_three_points((None, [5.0, 0.0]), (None, [-5.0, 0.0]), (None, [0.0, 5.0]))
        .unwrap();
    assert!(
        t.constraints
            .iter()
            .all(|c| !matches!(c.kind, ConstraintKind::PointOnCircle { .. }))
    );
    // 中心・始点・終点・円弧の 4 つだけ(途中の点は残さない)
    assert_eq!(t.entities.len(), before + 4);
}
