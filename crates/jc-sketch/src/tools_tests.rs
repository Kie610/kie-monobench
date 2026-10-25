use crate::*;
use std::collections::HashMap;

/// 寸法の式をすべて `v` mm とみなして解く。
fn solve_all(s: &Sketch, v: f64) -> Solution {
    let values: HashMap<ConstraintId, f64> = s
        .constraints
        .iter()
        .filter(|c| c.kind.value().is_some())
        .map(|c| (c.id, v))
        .collect();
    solve(s, &values).expect("解ける")
}

fn near(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6
}

fn lines(s: &Sketch) -> Vec<(EntityId, EntityId, EntityId)> {
    s.entities
        .iter()
        .filter_map(|e| match e {
            Entity::Line { id, a, b, .. } => Some((*id, *a, *b)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_rectangle_is_four_lines_kept_square() {
    let mut s = Sketch::default();
    let l = s
        .add_rectangle((None, [0.0, 0.0]), (None, [20.0, 10.0]))
        .unwrap();
    assert_eq!(lines(&s).len(), 4);
    let sol = solve_all(&s, 0.0);
    assert_eq!(sol.dof, 4, "位置 2 と幅・高さの 2");
    let (p, _) = profile_of(&s, &sol, jc_core::Plane::XY);
    assert_eq!(p.regions.len(), 1);
    assert_eq!(l.len(), 4);
    assert!(
        s.add_rectangle((None, [0.0, 0.0]), (None, [5.0, 0.0]))
            .is_none()
    );

    let mut c = Sketch::default();
    c.add_center_rectangle((None, [10.0, 5.0]), (None, [14.0, 8.0]))
        .unwrap();
    let sol = solve_all(&c, 0.0);
    assert_eq!(sol.dof, 4, "中心 2 と幅・高さの 2");
    let xs: Vec<f64> = sol.points.values().map(|p| p[0]).collect();
    assert!(xs.iter().any(|x| (x - 6.0).abs() < 1e-6), "{xs:?}");
}

#[test]
fn mirrored_elements_follow_the_original() {
    use ConstraintKind as K;
    let mut s = Sketch::default();
    let a = s.add_point(0.0, -5.0);
    let b = s.add_point(0.0, 5.0);
    let axis = s.add_line(a, b);
    let p = s.add_point(1.0, 1.0);
    let q = s.add_point(3.0, 2.0);
    let l = s.add_line(p, q);
    let on = s.add_point(0.0, 0.0);
    let circle = s.add_circle(q, 1.0);
    let line2 = s.add_line(on, p);
    for x in [a, b, p, q, on] {
        s.add_constraint(K::Fix { point: x });
    }
    s.add_constraint(K::Radius {
        circle,
        value: "1 mm".into(),
    });
    let new = s.mirror(&[l, circle, line2, axis], axis);
    // 点 2 つ(軸の上の点は共有)、線 2 本、円 1 つ
    assert_eq!(new.len(), 5, "{new:?}");
    // 元の点を動かすと、写しも付いてくる
    s.set_point(q, 4.0, 3.0);
    s.constraints.retain(|c| c.kind != K::Fix { point: q });
    s.add_constraint(K::Fix { point: q });
    let sol = solve_all(&s, 1.0);
    assert!(
        sol.points.values().any(|v| near(*v, [-4.0, 3.0])),
        "{:?}",
        sol.points
    );
    assert!(sol.points.values().any(|v| near(*v, [-1.0, 1.0])));
    let r: Vec<f64> = sol.radii.values().copied().collect();
    assert!((r[0] - r[1]).abs() < 1e-9);
    assert_eq!(sol.status, SolveStatus::WellConstrained, "{:?}", sol.status);
}

#[test]
fn trimming_cuts_at_the_nearest_crossings() {
    // X 字の 2 本の線。1 本目の左下の部分を消す
    let mut s = Sketch::default();
    let (p, q) = (s.add_point(0.0, 0.0), s.add_point(10.0, 10.0));
    let (r, t) = (s.add_point(0.0, 10.0), s.add_point(10.0, 0.0));
    let first = s.add_line(p, q);
    let second = s.add_line(r, t);
    s.add_constraint(ConstraintKind::Horizontal { line: second });
    assert!(s.trim(first, [2.0, 2.0]));
    let ls = lines(&s);
    assert_eq!(ls.len(), 2);
    let kept = ls.iter().find(|l| l.0 != second).unwrap();
    assert_eq!(kept.2, q, "右上の端は残る");
    let sol = solve_all(&s, 0.0);
    assert!(near(sol.points[&kept.1], [5.0, 5.0]));
    assert!(!sol.points.contains_key(&p), "使われなくなった点は消える");

    // 2 本目は 1 本目の残りの端で切れ、押した右下の側だけが消える
    let kept = *kept;
    assert!(s.trim(second, [9.0, 1.0]));
    let ls = lines(&s);
    assert_eq!(ls.len(), 2);
    let rest = ls.iter().find(|l| l.0 != kept.0).unwrap();
    assert_eq!(rest.1, r, "左上の端は残る");
    let sol = solve_all(&s, 0.0);
    let (start, end) = (sol.points[&rest.1], sol.points[&rest.2]);
    assert!(
        near(end, sol.points[&kept.1]),
        "切った端は 1 本目の端と重なる"
    );
    assert!((start[1] - end[1]).abs() < 1e-9, "水平のまま");
    assert!(end[0] > start[0]);

    // 交点のない線はまるごと消える
    let (a, b) = (s.add_point(20.0, 0.0), s.add_point(30.0, 0.0));
    let lone = s.add_line(a, b);
    assert!(s.trim(lone, [25.0, 0.0]));
    assert_eq!(lines(&s).len(), 2);
    assert!(lines(&s).iter().all(|l| l.0 != lone));

    // 円を直径の線で切ると、押した側の反対の半円が残る
    let mut s = Sketch::default();
    let c = s.add_point(0.0, 0.0);
    let circle = s.add_circle(c, 5.0);
    s.add_constraint(ConstraintKind::Radius {
        circle,
        value: "5 mm".into(),
    });
    let (a, b) = (s.add_point(-10.0, 0.0), s.add_point(10.0, 0.0));
    s.add_line(a, b);
    assert!(s.trim(circle, [0.0, 5.0]));
    let arc = s
        .entities
        .iter()
        .find_map(|e| match e {
            Entity::Arc { id, start, end, .. } => Some((*id, *start, *end)),
            _ => None,
        })
        .expect("円弧になる");
    assert!(
        s.constraints.iter().any(|k| k.kind
            == ConstraintKind::Radius {
                circle: arc.0,
                value: "5 mm".into()
            }),
        "半径の寸法は円弧へ移る"
    );
    let sol = solve_all(&s, 5.0);
    assert!(
        near(sol.points[&arc.1], [-5.0, 0.0]),
        "{:?}",
        sol.points[&arc.1]
    );
    assert!(near(sol.points[&arc.2], [5.0, 0.0]));
}

#[test]
fn an_offset_square_keeps_its_distance() {
    let mut s = Sketch::default();
    let l = s
        .add_rectangle((None, [0.0, 0.0]), (None, [10.0, 10.0]))
        .unwrap();
    let first = lines(&s)[0].1;
    s.add_constraint(ConstraintKind::Fix { point: first });
    for line in [l[0], l[1]] {
        s.add_constraint(ConstraintKind::Length {
            line,
            value: "10 mm".into(),
        });
    }
    let new = s.offset(&l, 2.0, "2 mm", true);
    assert_eq!(new.len(), 4);
    let mut values: HashMap<ConstraintId, f64> = HashMap::new();
    for c in &s.constraints {
        match &c.kind {
            ConstraintKind::Length { .. } => values.insert(c.id, 10.0),
            ConstraintKind::PointLineDistance { .. } => values.insert(c.id, 2.0),
            _ => None,
        };
    }
    let sol = solve(&s, &values).unwrap();
    assert!(
        sol.points.values().any(|p| near(*p, [-2.0, -2.0])),
        "{:?}",
        sol.points
    );
    assert!(sol.points.values().any(|p| near(*p, [12.0, 12.0])));
    assert_eq!(sol.status, SolveStatus::WellConstrained, "{:?}", sol.status);
    // 内側へ
    let mut s2 = Sketch::default();
    let l = s2
        .add_rectangle((None, [0.0, 0.0]), (None, [10.0, 10.0]))
        .unwrap();
    s2.offset(&l, 2.0, "2 mm", false);
    let pts: Vec<[f64; 2]> = s2
        .entities
        .iter()
        .filter_map(|e| match e {
            Entity::Point { x, y, .. } => Some([*x, *y]),
            _ => None,
        })
        .collect();
    assert!(pts.iter().any(|p| near(*p, [2.0, 2.0])), "{pts:?}");
}

#[test]
fn lone_points_are_the_ones_not_used_by_shapes() {
    let mut s = Sketch::default();
    let o = s.add_point(0.0, 0.0);
    s.add_constraint(ConstraintKind::Fix { point: o });
    let h = s.add_point(5.0, 5.0);
    let (a, b) = (s.add_point(0.0, 10.0), s.add_point(10.0, 10.0));
    s.add_line(a, b);
    let touching = s.add_point(10.0, 10.0);
    s.add_constraint(ConstraintKind::Coincident { a: b, b: touching });
    assert_eq!(s.lone_points(), vec![h]);
}

#[test]
fn a_tangent_arc_continues_a_line_smoothly() {
    let mut s = Sketch::default();
    let (p, q) = (s.add_point(0.0, 0.0), s.add_point(10.0, 0.0));
    let l = s.add_line(p, q);
    // 右へ進んで左(上)へ曲がり、(10, 10) で半円を描く
    let arc = s.add_tangent_arc(l, q, (None, [10.0, 10.0])).unwrap();
    let Some(Entity::Arc { center, start, .. }) = s.entity(arc).cloned() else {
        panic!()
    };
    assert_eq!(start, q, "左へ曲がるので線の端から反時計回り");
    let sol = solve_all(&s, 0.0);
    assert!(
        near(sol.points[&center], [10.0, 5.0]),
        "{:?}",
        sol.points[&center]
    );
    // 右へ曲がる時は終点から反時計回り
    let arc2 = s.add_tangent_arc(l, q, (None, [10.0, -4.0])).unwrap();
    let Some(Entity::Arc { start, end, .. }) = s.entity(arc2).cloned() else {
        panic!()
    };
    assert_eq!(end, q);
    assert_ne!(start, q);
    // 線の延長上には描けない
    assert!(s.add_tangent_arc(l, q, (None, [20.0, 0.0])).is_none());
}

#[test]
fn a_corner_is_rounded_with_a_tangent_arc() {
    let mut s = Sketch::default();
    s.add_rectangle((None, [0.0, 0.0]), (None, [20.0, 10.0]))
        .unwrap();
    // 右下の隅
    let corner = s
        .entities
        .iter()
        .find_map(|e| match e {
            Entity::Point { id, x, y } if (*x - 20.0).abs() < 1e-9 && y.abs() < 1e-9 => Some(*id),
            _ => None,
        })
        .unwrap();
    let points = s.entities.len();
    let arc = s.fillet_corner(corner, 3.0, "3 mm").expect("丸められる");
    assert!(s.entity(corner).is_none(), "使われなくなった隅の点は消す");
    // 接点 2 つと中心を足し、隅を 1 つ消し、円弧を 1 つ足した
    assert_eq!(s.entities.len(), points + 3);
    let sol = solve_all(&s, 3.0);
    let Some(Entity::Arc {
        center, start, end, ..
    }) = s.entity(arc).cloned()
    else {
        panic!()
    };
    assert!(
        near(sol.points[&center], [17.0, 3.0]),
        "{:?}",
        sol.points[&center]
    );
    let ends = [sol.points[&start], sol.points[&end]];
    assert!(
        ends.iter().any(|p| near(*p, [17.0, 0.0])) && ends.iter().any(|p| near(*p, [20.0, 3.0]))
    );
    let (p, bad) = profile_of(&s, &sol, jc_core::Plane::XY);
    assert!(bad.is_empty(), "{bad:?}");
    assert_eq!(p.regions.len(), 1);
    assert_eq!(p.regions[0].outer.edges.len(), 5);
    // 半径が辺より大きければ丸めない
    let mut t = Sketch::default();
    t.add_rectangle((None, [0.0, 0.0]), (None, [4.0, 4.0]))
        .unwrap();
    let c = t.entities.iter().find_map(|e| match e {
        Entity::Point { id, .. } => Some(*id),
        _ => None,
    });
    assert!(t.fillet_corner(c.unwrap(), 5.0, "5 mm").is_none());
}

#[test]
fn a_slot_is_two_tangent_arcs_and_two_lines() {
    let mut s = Sketch::default();
    let [top, _, left, right] = s
        .add_slot((None, [0.0, 0.0]), (None, [20.0, 0.0]), 3.0)
        .expect("描ける");
    let r = s.add_constraint(ConstraintKind::Radius {
        circle: left,
        value: "4 mm".into(),
    });
    let _ = (r, top, right);
    // 半径を 4 mm にすると、両端とも 4 mm で幅 8 mm の長穴になる
    let mut values: HashMap<ConstraintId, f64> = HashMap::new();
    values.insert(r, 4.0);
    let sol = solve(&s, &values).expect("解ける");
    let Some(Entity::Arc { center, start, .. }) = s.entity(right).cloned() else {
        panic!()
    };
    let (c, p) = (sol.points[&center], sol.points[&start]);
    assert!(((p[0] - c[0]).hypot(p[1] - c[1]) - 4.0).abs() < 1e-6);
    let (p, bad) = profile_of(&s, &sol, jc_core::Plane::XY);
    assert!(bad.is_empty(), "{bad:?}");
    assert_eq!(p.regions.len(), 1);
    assert_eq!(p.regions[0].outer.edges.len(), 4);
    assert!(
        s.add_slot((None, [0.0, 0.0]), (None, [0.0, 0.0]), 3.0)
            .is_none()
    );
}

#[test]
fn a_regular_polygon_keeps_its_shape() {
    let mut s = Sketch::default();
    let sides = s
        .add_polygon((None, [0.0, 0.0]), (None, [10.0, 0.0]), 6)
        .expect("描ける");
    assert_eq!(sides.len(), 6);
    let d = s.add_constraint(ConstraintKind::Length {
        line: sides[0],
        value: "5 mm".into(),
    });
    let mut values: HashMap<ConstraintId, f64> = HashMap::new();
    values.insert(d, 5.0);
    let sol = solve(&s, &values).expect("解ける");
    // 正六角形の辺は外接円の半径に等しい
    for l in &sides {
        let Some(Entity::Line { a, b, .. }) = s.entity(*l).cloned() else {
            panic!()
        };
        let (p, q) = (sol.points[&a], sol.points[&b]);
        assert!(((p[0] - q[0]).hypot(p[1] - q[1]) - 5.0).abs() < 1e-6);
    }
    let (p, bad) = profile_of(&s, &sol, jc_core::Plane::XY);
    assert!(bad.is_empty(), "{bad:?}");
    assert_eq!(p.regions.len(), 1);
    assert_eq!(p.regions[0].outer.edges.len(), 6);
    assert!(
        s.add_polygon((None, [0.0, 0.0]), (None, [1.0, 0.0]), 2)
            .is_none()
    );
}

#[test]
fn trimming_keeps_lone_points_that_were_not_on_the_line() {
    // 点だけで置いた印(穴の位置など)は、関係のない線を切っても消えない
    let mut s = Sketch::default();
    let mark = s.add_point(30.0, 30.0);
    let (p, q) = (s.add_point(0.0, 0.0), s.add_point(10.0, 10.0));
    let (r, t) = (s.add_point(0.0, 10.0), s.add_point(10.0, 0.0));
    let first = s.add_line(p, q);
    s.add_line(r, t);
    assert!(s.trim(first, [2.0, 2.0]));
    assert!(s.entity(mark).is_some(), "線に関係のない点は残る");
    assert!(s.entity(p).is_none(), "切った線だけが使っていた端は消える");
}

fn edge(feature: u64, a: &str, b: &str) -> ProjectionSource {
    ProjectionSource::Edge {
        feature: jc_core::FeatureId(feature),
        faces: [a.into(), b.into()],
    }
}

#[test]
fn projected_points_stay_on_the_body_and_are_not_lone() {
    let mut s = Sketch::default();
    let line = s.add_projection(&RefItem {
        source: edge(1, "top", "side"),
        shape: RefShape::Line([0.0, 0.0], [60.0, 0.0]),
    });
    let Some(Entity::Line {
        a, b, construction, ..
    }) = s.entity(line).cloned()
    else {
        panic!("線になる");
    };
    assert!(construction, "写した辺は補助線");
    assert!(s.lone_points().is_empty(), "写した点は孤立した点に数えない");
    // 写した点は動かない(自由度に数えない)。そこから 10 mm の点を置く
    let p = s.add_point(5.0, 8.0);
    s.add_constraint(ConstraintKind::PointLineDistance {
        point: p,
        line,
        value: "10 mm".into(),
    });
    let sol = solve_all(&s, 10.0);
    assert!(near(sol.points[&a], [0.0, 0.0]));
    assert!(near(sol.points[&b], [60.0, 0.0]));
    assert_eq!(sol.dof, 1, "点の横の位置だけが残る");
    assert!((sol.points[&p][1] - 10.0).abs() < 1e-6);
    // 写した点を固定するのは余分
    let values: HashMap<ConstraintId, f64> = s.constraints.iter().map(|c| (c.id, 10.0)).collect();
    let k = ConstraintKind::Fix { point: a };
    assert!(redundant_with(&s, &values, &k).is_some());
    // 同じ元を写すと同じ要素を返す(面の順は問わない)
    let again = s.add_projection(&RefItem {
        source: edge(1, "side", "top"),
        shape: RefShape::Line([0.0, 0.0], [60.0, 0.0]),
    });
    assert_eq!(again, line);
}

#[test]
fn refreshing_follows_the_body_without_swapping_ends() {
    let mut s = Sketch::default();
    let line = s.add_projection(&RefItem {
        source: edge(1, "top", "side"),
        shape: RefShape::Line([0.0, 0.0], [60.0, 0.0]),
    });
    let Some(Entity::Line { a, b, .. }) = s.entity(line).cloned() else {
        panic!()
    };
    // 板の幅が 80 になり、辺の向きが逆に返ってきても端は入れ替わらない
    let refs = vec![RefItem {
        source: edge(1, "side", "top"),
        shape: RefShape::Line([80.0, 0.0], [0.0, 0.0]),
    }];
    assert!(refresh_projections(&mut s, &refs).is_empty());
    let at = |id| match s.entity(id) {
        Some(Entity::Point { x, y, .. }) => [*x, *y],
        _ => panic!(),
    };
    assert!(near(at(a), [0.0, 0.0]));
    assert!(near(at(b), [80.0, 0.0]));
    // 元の辺が無くなったら知らせる
    assert_eq!(refresh_projections(&mut s, &[]), vec![line]);
}

#[test]
fn closing_a_square_with_a_vertical_is_reported_as_already_decided() {
    // 上下の辺を横・同じ長さ、右の辺を縦にした後、閉じる左の辺の縦は余分になる
    use ConstraintKind as K;
    let mut s = Sketch::default();
    let p: Vec<EntityId> = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]
        .iter()
        .map(|q| s.add_point(q[0], q[1]))
        .collect();
    let l0 = s.add_line(p[0], p[1]);
    let l1 = s.add_line(p[1], p[2]);
    let l2 = s.add_line(p[2], p[3]);
    let l3 = s.add_line(p[3], p[0]);
    s.add_constraint(K::Horizontal { line: l0 });
    s.add_constraint(K::Vertical { line: l1 });
    s.add_constraint(K::Horizontal { line: l2 });
    s.add_constraint(K::Equal { a: l0, b: l2 });
    let by = redundant_with(&s, &HashMap::new(), &K::Vertical { line: l3 });
    assert!(by.is_some_and(|b| !b.is_empty()), "3 本で既に決まる");
    // 同じ長さが無ければ、左の辺の縦は余分ではない
    s.constraints.pop();
    assert!(redundant_with(&s, &HashMap::new(), &K::Vertical { line: l3 }).is_none());
    // 斜めの線の縦は余分ではない
    let mut t = Sketch::default();
    let (a, b) = (t.add_point(0.0, 0.0), t.add_point(3.0, 9.0));
    let l = t.add_line(a, b);
    assert!(redundant_with(&t, &HashMap::new(), &K::Vertical { line: l }).is_none());
}

#[test]
fn a_rounded_rectangle_is_one_loop_with_one_radius_and_equal_corners() {
    let mut s = Sketch::default();
    let (l, arcs) = s
        .add_rounded_rectangle((None, [0.0, 0.0]), (None, [30.0, 20.0]), 4.0, "4 mm")
        .unwrap();
    assert_eq!(lines(&s).len(), 4);
    let radii = s
        .constraints
        .iter()
        .filter(|c| matches!(c.kind, ConstraintKind::Radius { .. }))
        .count();
    let equal = s
        .constraints
        .iter()
        .filter(|c| matches!(c.kind, ConstraintKind::Equal { a, .. } if a == arcs[0]))
        .count();
    assert_eq!(
        (radii, equal),
        (1, 3),
        "半径の寸法は 1 つ、残り 3 つは等しい"
    );
    let sol = solve_all(&s, 4.0);
    assert_eq!(sol.dof, 4, "位置 2 と大きさ 2");
    let (p, _) = profile_of(&s, &sol, jc_core::Plane::XY);
    assert_eq!(p.regions.len(), 1, "閉じた 1 つの輪郭");
    for a in arcs {
        let Some(Entity::Arc { center, start, .. }) = s.entity(a) else {
            panic!("円弧");
        };
        let (c, p) = (sol.points[center], sol.points[start]);
        assert!(((p[0] - c[0]).hypot(p[1] - c[1]) - 4.0).abs() < 1e-6);
    }
    // 幅・高さは向かい合う辺の距離
    s.rounded_rectangle_size(l, Some("40 mm"), Some("25 mm"));
    let values: HashMap<ConstraintId, f64> = s
        .constraints
        .iter()
        .filter_map(|c| {
            let v = c.kind.value()?;
            Some((c.id, v.trim_end_matches(" mm").parse().ok()?))
        })
        .collect();
    let sol = solve(&s, &values).unwrap();
    assert_eq!(sol.dof, 2, "位置だけ");
    let xs: Vec<f64> = sol.points.values().map(|p| p[0]).collect();
    let ys: Vec<f64> = sol.points.values().map(|p| p[1]).collect();
    let span = |v: &[f64]| {
        v.iter().copied().fold(f64::MIN, f64::max) - v.iter().copied().fold(f64::MAX, f64::min)
    };
    assert!((span(&xs) - 40.0).abs() < 1e-6, "{xs:?}");
    assert!((span(&ys) - 25.0).abs() < 1e-6, "{ys:?}");
    // 半径は短い辺の半分より小さいこと(半分ちょうども断る)
    let mut t = Sketch::default();
    assert!(
        t.add_rounded_rectangle((None, [0.0, 0.0]), (None, [30.0, 20.0]), 10.0, "10 mm")
            .is_none()
    );
    assert!(
        t.add_rounded_rectangle((None, [0.0, 0.0]), (None, [30.0, 20.0]), 0.0, "0 mm")
            .is_none()
    );
    assert!(t.entities.is_empty(), "断ったら何も描かない");
}

#[test]
fn a_rounded_corner_still_follows_its_dimension() {
    let mut s = Sketch::default();
    s.add_rectangle((None, [0.0, 0.0]), (None, [20.0, 10.0]))
        .unwrap();
    let at = |s: &Sketch, q: [f64; 2]| {
        s.entities
            .iter()
            .find_map(|e| match e {
                Entity::Point { id, x, y } if near([*x, *y], q) => Some(*id),
                _ => None,
            })
            .unwrap()
    };
    let (origin, corner) = (at(&s, [0.0, 0.0]), at(&s, [20.0, 0.0]));
    s.add_constraint(ConstraintKind::Fix { point: origin });
    let width = s.add_constraint(ConstraintKind::HorizontalDistance {
        a: origin,
        b: corner,
        value: "30 mm".into(),
    });
    let arc = s.fillet_corner(corner, 3.0, "3 mm").expect("丸められる");
    assert!(s.entity(corner).is_some(), "式が指す角の点は残す");
    let values: HashMap<ConstraintId, f64> = s
        .constraints
        .iter()
        .filter_map(|c| {
            c.kind
                .value()
                .map(|_| (c.id, if c.id == width { 30.0 } else { 3.0 }))
        })
        .collect();
    let sol = solve(&s, &values).expect("解ける");
    let Some(Entity::Arc { center, .. }) = s.entity(arc).cloned() else {
        panic!()
    };
    // 角が式どおり 30 mm へ動くと、丸めた輪郭も一緒に動く
    assert!(
        near(sol.points[&corner], [30.0, 0.0]),
        "{:?}",
        sol.points[&corner]
    );
    assert!(
        near(sol.points[&center], [27.0, 3.0]),
        "{:?}",
        sol.points[&center]
    );
    // 仮の角の点は穴の位置や 3D の点にしない
    assert!(!s.lone_points().contains(&corner));
}

#[test]
fn an_offset_that_flips_an_arc_is_refused_with_a_reason() {
    // 上の辺の中ほどに低いこぶ(円弧)のある長方形
    let mut s = Sketch::default();
    let p = |s: &mut Sketch, x, y| s.add_point(x, y);
    let (p1, p2, p3) = (
        p(&mut s, 0.0, 0.0),
        p(&mut s, 40.0, 0.0),
        p(&mut s, 40.0, 20.0),
    );
    let (p4, p5, p6) = (
        p(&mut s, 22.0, 20.0),
        p(&mut s, 18.0, 20.0),
        p(&mut s, 0.0, 20.0),
    );
    let c = p(&mut s, 20.0, 18.5);
    let mut all = vec![s.add_line(p1, p2), s.add_line(p2, p3), s.add_line(p3, p4)];
    all.push(s.add_arc(c, p4, p5));
    all.extend([s.add_line(p5, p6), s.add_line(p6, p1)]);
    // 少し内へなら作れる
    let (made, why) = s
        .clone()
        .offset_corners_why(&all, 1.0, "1 mm", false, false);
    assert_eq!((made.len(), why), (6, None));
    // こぶの半径より深く内へずらすと、円弧の両端が回り込んで下へ膨らむ(裏返る)ので断る
    let mut t = s.clone();
    let (made, why) = t.offset_corners_why(&all, 2.2, "2.2 mm", false, false);
    assert!(made.is_empty());
    assert_eq!(why, Some(crate::offset::ARC_FLIPS));
    assert_eq!(t.entities.len(), s.entities.len(), "断った時は何も足さない");
}
