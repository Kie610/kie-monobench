//! 描く道具のうち、ほかの線に接する・続く物([`crate::draw_core`])の試験。どれも「作る → 解く → 位置・大きさ・状態」。

use crate::*;
use std::collections::{BTreeSet, HashMap};
use std::f64::consts::TAU;

/// 寸法の式を数として読む(`12 mm` → 12、`30 deg` → rad)。
fn plain_values(s: &Sketch) -> HashMap<ConstraintId, f64> {
    s.constraints
        .iter()
        .filter(|c| !c.reference)
        .filter_map(|c| {
            let v = c.kind.value()?;
            let n: f64 = v.split_whitespace().next()?.parse().ok()?;
            Some((c.id, if c.kind.is_angle() { n.to_radians() } else { n }))
        })
        .collect()
}

fn solve_plain(s: &Sketch) -> Solution {
    solve(s, &plain_values(s)).expect("解ける")
}

fn d(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn near(a: [f64; 2], b: [f64; 2]) -> bool {
    d(a, b) < 1e-6
}

/// 余分な拘束も矛盾もなく、式を満たしている。
fn clean(sol: &Solution) {
    assert!(
        matches!(
            sol.status,
            SolveStatus::WellConstrained | SolveStatus::UnderConstrained { .. }
        ),
        "{:?}",
        sol.status
    );
    assert!(sol.max_residual < 1e-7, "{}", sol.max_residual);
}

fn arc_parts(s: &Sketch, id: EntityId) -> (EntityId, EntityId, EntityId) {
    match s.entity(id) {
        Some(Entity::Arc {
            center, start, end, ..
        }) => (*center, *start, *end),
        _ => panic!("{id} は円弧ではない"),
    }
}

fn line_parts(s: &Sketch, id: EntityId) -> (EntityId, EntityId) {
    match s.entity(id) {
        Some(Entity::Line { a, b, .. }) => (*a, *b),
        _ => panic!("{id} は線分ではない"),
    }
}

/// 円・円弧の (中心, 半径)。
fn round(s: &Sketch, sol: &Solution, id: EntityId) -> ([f64; 2], f64) {
    match s.entity(id) {
        Some(Entity::Circle { center, .. }) => (sol.points[center], sol.radii[&id]),
        Some(Entity::Arc { center, start, .. }) => {
            let c = sol.points[center];
            (c, d(c, sol.points[start]))
        }
        _ => panic!("{id} は円ではない"),
    }
}

/// 点 `p` から直線 `line` までの符号付きの距離(左が正)。
fn side(s: &Sketch, sol: &Solution, line: EntityId, p: [f64; 2]) -> f64 {
    let (a, b) = line_parts(s, line);
    let (a, b) = (sol.points[&a], sol.points[&b]);
    ((b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])) / d(a, b)
}

/// 円弧の開き(度。始点から反時計回り)。
fn sweep_deg(s: &Sketch, sol: &Solution, arc: EntityId) -> f64 {
    let (c, a, b) = arc_parts(s, arc);
    let (c, a, b) = (sol.points[&c], sol.points[&a], sol.points[&b]);
    ((b[1] - c[1]).atan2(b[0] - c[0]) - (a[1] - c[1]).atan2(a[0] - c[0]))
        .rem_euclid(TAU)
        .to_degrees()
}

/// 円弧 `arc` の端 `at` から、円弧の外へ出る向き(単位)。終点なら反時計回りの接線、
/// 始点なら逆向き。円弧の中へ入る向きはこの逆。
fn leaving(s: &Sketch, sol: &Solution, arc: EntityId, at: EntityId) -> [f64; 2] {
    let (c, st, en) = arc_parts(s, arc);
    let (c, p) = (sol.points[&c], sol.points[&at]);
    let r = d(c, p);
    let v = [(p[0] - c[0]) / r, (p[1] - c[1]) / r];
    if at == en {
        [-v[1], v[0]]
    } else {
        assert_eq!(at, st);
        [v[1], -v[0]]
    }
}

fn line(s: &mut Sketch, a: [f64; 2], b: [f64; 2]) -> EntityId {
    let p = s.add_point(a[0], a[1]);
    let q = s.add_point(b[0], b[1]);
    s.add_line(p, q)
}

// ---- 1. 接線円弧 ----

#[test]
fn a_tangent_arc_continues_from_the_end_of_an_arc() {
    for (from_end, end, turns_left) in [
        // 終点 (0, 10) から -x へ出て、上へ(右へ曲がる。外へふくらむ)
        (true, [-8.0, 14.0], false),
        // 終点から下へ(左へ曲がる。内へ巻く)
        (true, [-6.0, 4.0], true),
        // 始点 (10, 0) から -y へ出て、+x の側へ(左へ曲がる)
        (false, [14.0, -8.0], true),
    ] {
        let mut s = Sketch::default();
        let c = s.add_point(0.0, 0.0);
        let p = s.add_point(10.0, 0.0);
        let q = s.add_point(0.0, 10.0);
        let base = s.add_arc(c, p, q);
        let at = if from_end { q } else { p };
        let arc = s
            .add_tangent_arc_from(base, at, (None, end))
            .expect("描ける");
        assert!(
            s.constraints
                .iter()
                .any(|k| k.kind == ConstraintKind::TangentCircles { a: base, b: arc })
        );
        let sol = solve_plain(&s);
        clean(&sol);
        // 元の円弧 5、新しい円弧は終点の 2 だけ増える(接する拘束は余分でない)
        assert_eq!(sol.status, SolveStatus::UnderConstrained { dof: 7 });
        // 継ぎ目で向きがそろう: 元の円弧から出る向き = 新しい円弧へ入る向き
        let u = leaving(&s, &sol, base, at);
        let w = leaving(&s, &sol, arc, at);
        assert!(near(u, [-w[0], -w[1]]), "{u:?} {w:?}");
        let (_, st, en) = arc_parts(&s, arc);
        assert_eq!(st == at, turns_left, "左へ曲がるなら at が始点");
        let other = if st == at { en } else { st };
        assert!(near(sol.points[&other], end), "終わりは指した所");

        // 元の円弧の半径を変えても、継ぎ目で接したまま
        let mut t = s.clone();
        t.add_constraint(ConstraintKind::Fix { point: c });
        t.add_constraint(ConstraintKind::Radius {
            circle: base,
            value: "12 mm".into(),
        });
        let sol = solve_plain(&t);
        clean(&sol);
        assert!((round(&t, &sol, base).1 - 12.0).abs() < 1e-6);
        let u = leaving(&t, &sol, base, at);
        let w = leaving(&t, &sol, arc, at);
        assert!(near(u, [-w[0], -w[1]]), "{u:?} {w:?}");
    }
}

#[test]
fn a_tangent_arc_preview_matches_the_arc_and_lines_use_the_old_tool() {
    let mut s = Sketch::default();
    let c = s.add_point(0.0, 0.0);
    let p = s.add_point(10.0, 0.0);
    let q = s.add_point(0.0, 10.0);
    let base = s.add_arc(c, p, q);
    let sol = solve_plain(&s);
    let pts = s
        .tangent_arc_preview(&sol, base, q, [-8.0, 14.0])
        .expect("見本");
    assert!(pts.len() >= 9);
    assert!(near(pts[0], [0.0, 10.0]));
    assert!(near(*pts.last().unwrap(), [-8.0, 14.0]));
    let mut t = s.clone();
    let arc = t
        .add_tangent_arc_from(base, q, (None, [-8.0, 14.0]))
        .unwrap();
    let (cc, r) = round(&t, &solve_plain(&t), arc);
    assert!(pts.iter().all(|x| (d(*x, cc) - r).abs() < 1e-9));
    // 向きの直線の上の終わりでは描かない。端でない点(中心)からも描かない。何も変えない
    let before = s.clone();
    assert!(
        s.add_tangent_arc_from(base, q, (None, [-5.0, 10.0]))
            .is_none()
    );
    assert!(s.tangent_arc_preview(&sol, base, q, [-5.0, 10.0]).is_none());
    assert!(
        s.add_tangent_arc_from(base, c, (None, [3.0, 3.0]))
            .is_none()
    );
    assert_eq!(s, before);

    // 線分は今までの接線円弧と同じ
    let mut u = Sketch::default();
    let l = line(&mut u, [0.0, 0.0], [10.0, 0.0]);
    let (_, b) = line_parts(&u, l);
    let mut v = u.clone();
    let x = u.add_tangent_arc_from(l, b, (None, [10.0, 10.0])).unwrap();
    let y = v.add_tangent_arc(l, b, (None, [10.0, 10.0])).unwrap();
    assert_eq!((x, &u), (y, &v));
    let sol = solve_plain(&u);
    let (cc, r) = round(&u, &sol, x);
    assert!(near(cc, [10.0, 5.0]) && (r - 5.0).abs() < 1e-9);
    let pts = u
        .tangent_arc_preview(&sol, l, b, [10.0, 10.0])
        .expect("見本");
    assert!(pts.iter().all(|p| (d(*p, cc) - 5.0).abs() < 1e-9));
}

// ---- 2. 中点線分 ----

#[test]
fn a_midpoint_line_is_symmetric_about_its_midpoint() {
    let mut s = Sketch::default();
    let m = s.add_point(5.0, 5.0);
    s.add_constraint(ConstraintKind::Fix { point: m });
    let l = s
        .add_midpoint_line((Some(m), [5.0, 5.0]), (None, [8.0, 9.0]))
        .unwrap();
    assert!(
        s.constraints
            .iter()
            .any(|k| k.kind == ConstraintKind::Midpoint { point: m, line: l })
    );
    let sol = solve_plain(&s);
    clean(&sol);
    assert_eq!(sol.status, SolveStatus::UnderConstrained { dof: 2 });
    let (a, b) = line_parts(&s, l);
    assert!(near(sol.points[&a], [2.0, 1.0]));
    assert!(near(sol.points[&b], [8.0, 9.0]));
    // 長さを変えても、中点は真ん中のまま
    s.add_constraint(ConstraintKind::Length {
        line: l,
        value: "20 mm".into(),
    });
    let sol = solve_plain(&s);
    clean(&sol);
    let (pa, pb) = (sol.points[&a], sol.points[&b]);
    assert!((d(pa, pb) - 20.0).abs() < 1e-6);
    assert!(near(
        [(pa[0] + pb[0]) / 2.0, (pa[1] + pb[1]) / 2.0],
        [5.0, 5.0]
    ));

    // 中点が新しい点でもよい。中点と端が重なれば描かない
    let mut t = Sketch::default();
    let l = t
        .add_midpoint_line((None, [0.0, 0.0]), (None, [3.0, 4.0]))
        .unwrap();
    let sol = solve_plain(&t);
    assert_eq!(sol.status, SolveStatus::UnderConstrained { dof: 4 });
    let (a, _) = line_parts(&t, l);
    assert!(near(sol.points[&a], [-3.0, -4.0]));
    let before = t.clone();
    assert!(
        t.add_midpoint_line((None, [1.0, 1.0]), (None, [1.0, 1.0]))
            .is_none()
    );
    assert_eq!(t, before);
}

// ---- 3. 2 つに接する円 ----

/// 円 `circle` が `other`(線分・円・円弧)に接しているか。
fn touches(s: &Sketch, sol: &Solution, other: EntityId, circle: EntityId) -> bool {
    let (c, r) = round(s, sol, circle);
    match s.entity(other) {
        Some(Entity::Line { .. }) => (side(s, sol, other, c).abs() - r).abs() < 1e-6,
        _ => {
            let (c2, r2) = round(s, sol, other);
            let dd = d(c, c2);
            (dd - (r + r2)).abs() < 1e-6 || (dd - (r - r2).abs()).abs() < 1e-6
        }
    }
}

#[test]
fn a_circle_tangent_to_two_lines_sits_in_the_corner_near_the_cursor() {
    let mut s = Sketch::default();
    let x = line(&mut s, [0.0, 0.0], [20.0, 0.0]);
    let y = line(&mut s, [0.0, 0.0], [0.0, 20.0]);
    let sol0 = solve_plain(&s);
    let w = [3.0, 1.0];
    let pre = s.circle_tangent_two_preview(&sol0, x, y, w).unwrap();
    let c = s.add_circle_tangent_two(x, y, w).unwrap();
    let sol = solve_plain(&s);
    clean(&sol);
    // 元の 2 本の 8 と円の大きさの 1(接する拘束は余分にならない)
    assert_eq!(sol.status, SolveStatus::UnderConstrained { dof: 9 });
    let (cc, r) = round(&s, &sol, c);
    // 角 (0,0) の 2 本に接し (3,1) を通る円のうち小さい方: r² - 8r + 10 = 0
    let want = 4.0 - 6f64.sqrt();
    assert!((r - want).abs() < 1e-6, "{r}");
    assert!(near(cc, [want, want]));
    assert!(near(pre.0, cc) && (pre.1 - r).abs() < 1e-9);
    assert!(touches(&s, &sol, x, c) && touches(&s, &sol, y, c));

    // 交わる 2 本の別の角(左上)をカーソルで選ぶ
    let mut t = Sketch::default();
    let x = line(&mut t, [-20.0, 0.0], [20.0, 0.0]);
    let y = line(&mut t, [0.0, -20.0], [0.0, 20.0]);
    let c = t.add_circle_tangent_two(x, y, [-2.0, 2.5]).unwrap();
    let sol = solve_plain(&t);
    clean(&sol);
    let (cc, r) = round(&t, &sol, c);
    assert!(cc[0] < 0.0 && cc[1] > 0.0);
    assert!((d(cc, [-2.0, 2.5]) - r).abs() < 1e-6, "カーソルを通る");

    // 平行な 2 本: 間の半分の半径で、カーソルに近い所
    let mut u = Sketch::default();
    let a = line(&mut u, [0.0, 0.0], [20.0, 0.0]);
    let b = line(&mut u, [0.0, 10.0], [20.0, 10.0]);
    let c = u.add_circle_tangent_two(a, b, [7.0, 3.0]).unwrap();
    let sol = solve_plain(&u);
    clean(&sol);
    let (cc, r) = round(&u, &sol, c);
    assert!(near(cc, [7.0, 5.0]) && (r - 5.0).abs() < 1e-6);
    // 間の外をカーソルで指しても、同じ線分を 2 回選んでも描かない
    let before = u.clone();
    assert!(u.add_circle_tangent_two(a, b, [7.0, 13.0]).is_none());
    assert!(u.add_circle_tangent_two(a, a, [7.0, 3.0]).is_none());
    assert_eq!(u, before);
}

#[test]
fn a_circle_tangent_to_lines_circles_and_arcs_passes_the_cursor() {
    // 線分と円(円の外)
    let mut s = Sketch::default();
    let l = line(&mut s, [-20.0, 0.0], [30.0, 0.0]);
    let o = s.add_point(10.0, 10.0);
    let big = s.add_circle(o, 4.0);
    let w = [13.0, 2.0];
    let c = s.add_circle_tangent_two(l, big, w).unwrap();
    let sol = solve_plain(&s);
    clean(&sol);
    let (cc, r) = round(&s, &sol, c);
    assert!(touches(&s, &sol, l, c) && touches(&s, &sol, big, c));
    assert!((d(cc, w) - r).abs() < 1e-6, "カーソルを通る");
    assert!(cc[1] > 0.0 && (d(cc, [10.0, 10.0]) - (r + 4.0)).abs() < 1e-6);

    // 円の中と弦: 円の内側で、弦の上の側
    let mut t = Sketch::default();
    let o = t.add_point(0.0, 0.0);
    let big = t.add_circle(o, 10.0);
    let l = line(&mut t, [-20.0, -5.0], [20.0, -5.0]);
    let w = [1.0, 0.0];
    let c = t.add_circle_tangent_two(big, l, w).unwrap();
    let sol = solve_plain(&t);
    clean(&sol);
    let (cc, r) = round(&t, &sol, c);
    assert!(
        (d(cc, [0.0, 0.0]) - (10.0 - r)).abs() < 1e-6,
        "内から接する"
    );
    assert!((cc[1] + 5.0 - r).abs() < 1e-6 && (d(cc, w) - r).abs() < 1e-6);

    // 円弧と円(外どうし)。円弧は円として接する
    let mut u = Sketch::default();
    let c1 = u.add_point(0.0, 0.0);
    let p = u.add_point(5.0, 0.0);
    let q = u.add_point(0.0, 5.0);
    let arc = u.add_arc(c1, p, q);
    let c2 = u.add_point(16.0, 0.0);
    let small = u.add_circle(c2, 3.0);
    let w = [8.0, 3.0];
    let sol0 = solve_plain(&u);
    let pre = u.circle_tangent_two_preview(&sol0, arc, small, w).unwrap();
    let c = u.add_circle_tangent_two(arc, small, w).unwrap();
    let sol = solve_plain(&u);
    clean(&sol);
    let (cc, r) = round(&u, &sol, c);
    assert!(near(pre.0, cc) && (pre.1 - r).abs() < 1e-9);
    assert!((d(cc, [0.0, 0.0]) - (5.0 + r)).abs() < 1e-6);
    assert!((d(cc, [16.0, 0.0]) - (3.0 + r)).abs() < 1e-6);
    assert!((d(cc, w) - r).abs() < 1e-6);
    // 元の円の大きさを変えても接したまま(拘束で付いている)
    u.add_constraint(ConstraintKind::Fix { point: c1 });
    u.add_constraint(ConstraintKind::Fix { point: c2 });
    u.add_constraint(ConstraintKind::Radius {
        circle: small,
        value: "4 mm".into(),
    });
    let sol = solve_plain(&u);
    clean(&sol);
    assert!((round(&u, &sol, small).1 - 4.0).abs() < 1e-6);
    assert!(touches(&u, &sol, arc, c) && touches(&u, &sol, small, c));
}

// ---- 4. 3 つに接する円 ----

#[test]
fn a_circle_tangent_to_three_lines_is_the_same_as_before() {
    let mut s = Sketch::default();
    let a = s.add_point(0.0, 0.0);
    let b = s.add_point(6.0, 0.0);
    let c = s.add_point(0.0, 8.0);
    let l = [s.add_line(a, b), s.add_line(b, c), s.add_line(c, a)];
    let mut t = s.clone();
    let x = s.add_circle_tangent_three(l, [1.5, 1.5]).unwrap();
    let y = t.add_circle_tangent_to_lines(l, [1.5, 1.5]).unwrap();
    assert_eq!((x, &s), (y, &t));
    let sol = solve_plain(&s);
    assert!((sol.radii[&x] - 2.0).abs() < 1e-6);
}

/// 中心を固定し半径を決めた円。
fn fixed_circle(s: &mut Sketch, c: [f64; 2], r: f64) -> EntityId {
    let p = s.add_point(c[0], c[1]);
    s.add_constraint(ConstraintKind::Fix { point: p });
    let circle = s.add_circle(p, r);
    s.add_constraint(ConstraintKind::Radius {
        circle,
        value: format!("{r} mm"),
    });
    circle
}

/// 両端を固定した線分。
fn fixed_line(s: &mut Sketch, a: [f64; 2], b: [f64; 2]) -> EntityId {
    let l = line(s, a, b);
    let (p, q) = line_parts(s, l);
    s.add_constraint(ConstraintKind::Fix { point: p });
    s.add_constraint(ConstraintKind::Fix { point: q });
    l
}

#[test]
fn a_circle_tangent_to_three_circles_fills_the_gap_and_is_fully_constrained() {
    let h = 20.0 * 3f64.sqrt() / 2.0;
    let mut s = Sketch::default();
    let ids = [
        fixed_circle(&mut s, [0.0, 0.0], 8.0),
        fixed_circle(&mut s, [20.0, 0.0], 8.0),
        fixed_circle(&mut s, [10.0, h], 8.0),
    ];
    let g = [10.0, h / 3.0];
    let sol0 = solve_plain(&s);
    let pre = s.circle_tangent_three_preview(&sol0, ids, g).unwrap();
    let c = s.add_circle_tangent_three(ids, g).unwrap();
    let sol = solve_plain(&s);
    // 3 つが決まっていれば円も決まる(接する 3 つの拘束は余分でない)
    assert_eq!(sol.status, SolveStatus::WellConstrained);
    let (cc, r) = round(&s, &sol, c);
    assert!(near(cc, g));
    assert!((r - (20.0 / 3f64.sqrt() - 8.0)).abs() < 1e-6, "{r}");
    assert!(near(pre.0, cc) && (pre.1 - r).abs() < 1e-9);
    for id in ids {
        assert!(touches(&s, &sol, id, c));
    }
    // 同じ要素を 2 回は選べない
    let before = s.clone();
    assert!(
        s.add_circle_tangent_three([ids[0], ids[0], ids[1]], g)
            .is_none()
    );
    assert_eq!(s, before);
}

#[test]
fn a_circle_tangent_to_a_line_and_two_circles_or_arcs() {
    // 床の線と、床に乗った半径 5 の 2 つの円(中心の間 12)の間に入る円: 36 = 20 r
    for with_arc in [false, true] {
        let mut s = Sketch::default();
        let floor = fixed_line(&mut s, [-20.0, 0.0], [20.0, 0.0]);
        let a = fixed_circle(&mut s, [-6.0, 5.0], 5.0);
        let b = if with_arc {
            // 中心と始点を固定した円弧(終点は円の上で自由)
            let c = s.add_point(6.0, 5.0);
            let p = s.add_point(11.0, 5.0);
            let q = s.add_point(6.0, 10.0);
            s.add_constraint(ConstraintKind::Fix { point: c });
            s.add_constraint(ConstraintKind::Fix { point: p });
            s.add_arc(c, p, q)
        } else {
            fixed_circle(&mut s, [6.0, 5.0], 5.0)
        };
        let ids = [floor, a, b];
        let c = s.add_circle_tangent_three(ids, [0.0, 1.0]).unwrap();
        let sol = solve_plain(&s);
        clean(&sol);
        let (cc, r) = round(&s, &sol, c);
        assert!((r - 1.8).abs() < 1e-6, "{r}");
        assert!(near(cc, [0.0, 1.8]));
        let want = if with_arc {
            SolveStatus::UnderConstrained { dof: 1 }
        } else {
            SolveStatus::WellConstrained
        };
        assert_eq!(sol.status, want, "円弧の終点の 1 だけ残る");
        for id in ids {
            assert!(touches(&s, &sol, id, c));
        }
    }
}

// ---- 5. 1 辺で決める多角形 ----

#[test]
fn an_edge_polygon_is_built_on_the_given_side() {
    let mut s = Sketch::default();
    let a = s.add_point(0.0, 0.0);
    let b = s.add_point(10.0, 0.0);
    s.add_constraint(ConstraintKind::Fix { point: a });
    s.add_constraint(ConstraintKind::Fix { point: b });
    let sides = s
        .add_edge_polygon((Some(a), [0.0, 0.0]), (Some(b), [10.0, 0.0]), 6, [5.0, 5.0])
        .unwrap();
    assert_eq!(sides.len(), 6);
    assert_eq!(line_parts(&s, sides[0]), (a, b), "最初の辺が a-b");
    let sol = solve_plain(&s);
    // 辺を固定すれば決まる(拘束は add_polygon と同じで、余分はない)
    assert_eq!(sol.status, SolveStatus::WellConstrained);
    for l in &sides {
        let (p, q) = line_parts(&s, *l);
        assert!((d(sol.points[&p], sol.points[&q]) - 10.0).abs() < 1e-6);
    }
    let ring = s
        .entities
        .iter()
        .find_map(|e| match e {
            Entity::Circle {
                id,
                construction: true,
                ..
            } => Some(*id),
            _ => None,
        })
        .expect("補助線の外接円");
    let (cc, r) = round(&s, &sol, ring);
    assert!(near(cc, [5.0, 5.0 * 3f64.sqrt()]) && (r - 10.0).abs() < 1e-6);
    assert_eq!(profile_of(&s, &sol, jc_core::Plane::XY).0.regions.len(), 1);

    // 見本は同じ頂点(a から順に、b が 2 つ目)。下の側にも作れる
    let v = edge_polygon_preview([0.0, 0.0], [10.0, 0.0], 6, [5.0, 5.0]);
    assert_eq!(v.len(), 6);
    assert!(near(v[0], [0.0, 0.0]) && near(v[1], [10.0, 0.0]));
    assert!(v.iter().all(|p| p[1] >= -1e-9));
    let w = edge_polygon_preview([0.0, 0.0], [10.0, 0.0], 4, [5.0, -1.0]);
    assert!(near(w[2], [10.0, -10.0]) && near(w[3], [0.0, -10.0]));
    // 新しい点で作れば、大きさと向きと位置が自由(add_polygon と同じ 4)
    let mut t = Sketch::default();
    t.add_edge_polygon((None, [0.0, 0.0]), (None, [3.0, 4.0]), 5, [0.0, 5.0])
        .unwrap();
    let sol = solve_plain(&t);
    assert_eq!(sol.status, SolveStatus::UnderConstrained { dof: 4 });
    // 作れない時は何も変えない
    let before = t.clone();
    assert!(
        t.add_edge_polygon((None, [0.0, 0.0]), (None, [1.0, 0.0]), 2, [0.0, 1.0])
            .is_none()
    );
    assert!(
        t.add_edge_polygon((None, [0.0, 0.0]), (None, [1.0, 0.0]), 5, [3.0, 0.0])
            .is_none()
    );
    assert_eq!(t, before);
    assert!(edge_polygon_preview([0.0, 0.0], [0.0, 0.0], 5, [1.0, 1.0]).is_empty());
}

// ---- 6. 3 点円弧の長穴 ----

#[test]
fn a_three_point_arc_slot_follows_the_three_clicks() {
    let m = [20.0 / 2f64.sqrt(), 20.0 / 2f64.sqrt()];
    for (a, b) in [([20.0, 0.0], [0.0, 20.0]), ([0.0, 20.0], [20.0, 0.0])] {
        let mut s = Sketch::default();
        let [outer, inner, cap0, cap1] = s
            .add_three_point_arc_slot((None, a), m, (None, b), 3.0)
            .unwrap();
        let sol = solve_plain(&s);
        clean(&sol);
        assert_eq!(
            sol.status,
            SolveStatus::UnderConstrained { dof: 6 },
            "add_arc_slot と同じ"
        );
        let (co, ro) = round(&s, &sol, outer);
        assert!(near(co, [0.0, 0.0]));
        assert!((ro - 23.0).abs() < 1e-6 && (round(&s, &sol, inner).1 - 17.0).abs() < 1e-6);
        // 反時計回りに見て (20,0) が始まり
        let (c0, ..) = arc_parts(&s, cap0);
        let (c1, ..) = arc_parts(&s, cap1);
        assert!(near(sol.points[&c0], [20.0, 0.0]) && near(sol.points[&c1], [0.0, 20.0]));
        assert!((sweep_deg(&s, &sol, outer) - 90.0).abs() < 1e-6);
        assert_eq!(profile_of(&s, &sol, jc_core::Plane::XY).0.regions.len(), 1);
    }
    // 途中の点が反対側なら 270° の長穴。吸着した点を端の中心に使う
    let mut s = Sketch::default();
    let p = s.add_point(20.0, 0.0);
    s.add_constraint(ConstraintKind::Fix { point: p });
    let [outer, _, cap0, cap1] = s
        .add_three_point_arc_slot(
            (Some(p), [20.0, 0.0]),
            [-m[0], -m[1]],
            (None, [0.0, 20.0]),
            2.0,
        )
        .unwrap();
    let sol = solve_plain(&s);
    clean(&sol);
    assert!((sweep_deg(&s, &sol, outer) - 270.0).abs() < 1e-6);
    let (c0, ..) = arc_parts(&s, cap0);
    let (c1, ..) = arc_parts(&s, cap1);
    assert_eq!(c1, p, "終わり(反時計回り)が (20,0) の点");
    assert!(near(sol.points[&c0], [0.0, 20.0]));
    // 一直線の 3 点、幅が中心線の半径より大きい時は描かない
    let before = s.clone();
    assert!(
        s.add_three_point_arc_slot((None, [0.0, 0.0]), [5.0, 0.0], (None, [10.0, 0.0]), 1.0)
            .is_none()
    );
    assert!(
        s.add_three_point_arc_slot((None, [20.0, 0.0]), m, (None, [0.0, 20.0]), 25.0)
            .is_none()
    );
    assert_eq!(s, before);
}

// ---- 7. 長穴の幅を端の円弧の直径で ----

#[test]
fn a_slot_width_is_the_diameter_of_an_end_arc() {
    let mut s = Sketch::default();
    let [top, _, left, right] = s
        .add_slot((None, [0.0, 0.0]), (None, [30.0, 0.0]), 5.0)
        .unwrap();
    s.add_constraint(ConstraintKind::Diameter {
        circle: left,
        value: "12 mm".into(),
    });
    let sol = solve_plain(&s);
    clean(&sol);
    // 両端の中心 4 が残る(直径は円弧にも使え、等しい拘束で両端がそろう)
    assert_eq!(sol.status, SolveStatus::UnderConstrained { dof: 4 });
    assert!((round(&s, &sol, left).1 - 6.0).abs() < 1e-6);
    assert!((round(&s, &sol, right).1 - 6.0).abs() < 1e-6);
    let (a, _) = line_parts(&s, top);
    assert!((sol.points[&a][1] - 6.0).abs() < 1e-6);

    // 円弧に沿う長穴の端でも、直径が幅
    let mut t = Sketch::default();
    let [outer, inner, cap0, _] = t
        .add_arc_slot((None, [0.0, 0.0]), [20.0, 0.0], [0.0, 20.0], 3.0)
        .unwrap();
    t.add_constraint(ConstraintKind::Diameter {
        circle: cap0,
        value: "8 mm".into(),
    });
    let sol = solve_plain(&t);
    clean(&sol);
    assert_eq!(sol.status, SolveStatus::UnderConstrained { dof: 5 });
    assert!((round(&t, &sol, outer).1 - round(&t, &sol, inner).1 - 8.0).abs() < 1e-6);
}

/// 点 `p` を `path` の位置へ順に引き、フレームごとに前の解から続けて解く(画面のドラッグと同じ)。
fn drag_along(s: &Sketch, p: EntityId, path: &[[f64; 2]], each: &dyn Fn(&Solution)) {
    let v = plain_values(s);
    let mut start = s.clone();
    start.adopt_solution(&solve(s, &v).unwrap());
    let held = BTreeSet::from([p]);
    let mut last: Option<Solution> = None;
    for w in path {
        let mut m = start.clone();
        m.set_point(p, w[0], w[1]);
        let sol = solve_dragging(&start, &m, &v, &held, last.as_ref()).unwrap();
        each(&sol);
        last = Some(sol);
    }
}

#[test]
fn dimensioned_slots_keep_their_shape_when_dragged_around() {
    // 画面と同じく中心線の長さと端の直径を決めた長穴の、上の接点を持って 15° ずつ 1 周回す
    let mut s = Sketch::default();
    let [top, bottom, left, right] = s
        .add_slot((None, [0.0, 0.0]), (None, [30.0, 0.0]), 5.0)
        .unwrap();
    let axis = s.entities.last().unwrap().id();
    s.add_constraint(ConstraintKind::Length {
        line: axis,
        value: "30 mm".into(),
    });
    s.add_constraint(ConstraintKind::Diameter {
        circle: left,
        value: "10 mm".into(),
    });
    let (_, tb) = line_parts(&s, top);
    let r = d([0.0, 0.0], [30.0, 5.0]);
    let a0 = 5f64.atan2(30.0);
    let path: Vec<[f64; 2]> = (1..=24)
        .map(|k| {
            let t = a0 + (k as f64 * 15.0).to_radians();
            [r * t.cos(), r * t.sin()]
        })
        .collect();
    let (c1, ..) = arc_parts(&s, left);
    let (c2, ..) = arc_parts(&s, right);
    drag_along(&s, tb, &path, &|sol| {
        assert!(sol.max_residual < 1e-6);
        for arc in [left, right] {
            assert!((sweep_deg(&s, sol, arc) - 180.0).abs() < 1e-3);
            assert!((round(&s, sol, arc).1 - 5.0).abs() < 1e-6);
        }
        assert!((d(sol.points[&c1], sol.points[&c2]) - 30.0).abs() < 1e-6);
        for l in [top, bottom] {
            let (s1, s2) = (
                side(&s, sol, l, sol.points[&c1]),
                side(&s, sol, l, sol.points[&c2]),
            );
            assert!((s1 - s2).abs() < 1e-6, "線が中心線と平行");
        }
    });

    // 中心を固定した円弧に沿う長穴の、終わりの端の中心を 10° ずつ回す
    let mut t = Sketch::default();
    let [outer, inner, cap0, cap1] = t
        .add_arc_slot((None, [0.0, 0.0]), [20.0, 0.0], [0.0, 20.0], 3.0)
        .unwrap();
    let (cp, ..) = arc_parts(&t, outer);
    t.add_constraint(ConstraintKind::Fix { point: cp });
    let (s1, ..) = arc_parts(&t, cap1);
    let path: Vec<[f64; 2]> = (1..=25)
        .map(|k| {
            let a = (90.0 + k as f64 * 10.0).to_radians();
            [20.0 * a.cos(), 20.0 * a.sin()]
        })
        .collect();
    drag_along(&t, s1, &path, &|sol| {
        assert!(sol.max_residual < 1e-6);
        for cap in [cap0, cap1] {
            assert!((sweep_deg(&t, sol, cap) - 180.0).abs() < 1e-3);
        }
        assert!((sweep_deg(&t, sol, outer) - sweep_deg(&t, sol, inner)).abs() < 1e-3);
        assert!(round(&t, sol, outer).1 > round(&t, sol, inner).1);
    });
}
