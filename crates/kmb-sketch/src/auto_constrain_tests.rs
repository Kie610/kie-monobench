//! 自動拘束(K18)の試験。

use crate::{
    AutoGroup, AutoOptions, ConstraintKind as K, EntityId, Sketch, SolveStatus, apply_suggestions,
    auto_constrain, origin_ref, solve,
};
use std::collections::HashMap;

/// 読み込んだ DXF のような、拘束の無い長方形(左下 (0, 0)、40 × 20。角は別の点が重なる)と、
/// 写した原点。
fn loose_rect() -> (Sketch, EntityId) {
    let mut s = Sketch::default();
    let origin = s.add_projection(&origin_ref(&kmb_core::Plane::XY));
    let c = [[0.0, 0.0], [40.0, 0.0], [40.0, 20.0], [0.0, 20.0]];
    for i in 0..4 {
        let (p, q) = (c[i], c[(i + 1) % 4]);
        let a = s.add_point(p[0], p[1]);
        let b = s.add_point(q[0], q[1]);
        s.add_line(a, b);
    }
    (s, origin)
}

fn run(s: &Sketch, opts: &AutoOptions) -> crate::AutoResult {
    let values = crate::dimension_values(s, &|_| None).unwrap();
    let sol = solve(s, &values).unwrap();
    auto_constrain(s, &sol, &values, opts).unwrap()
}

fn solved_status(s: &Sketch) -> SolveStatus {
    let values = crate::dimension_values(s, &|_| None).unwrap();
    solve(s, &values).unwrap().status
}

// 確かめること:
// - 拘束の無い長方形が、寸法も付けると完全拘束になる(一致 4・水平垂直 4・寸法 2 と位置 2)
// - 付けた物はどれも余分にならず、等しいは出さない
#[test]
fn a_loose_rectangle_becomes_fully_constrained() {
    let (s, origin) = loose_rect();
    let r = run(
        &s,
        &AutoOptions {
            datum: Some(origin),
            ..Default::default()
        },
    );
    assert_eq!(r.dof_before, 16, "線 4 本の端 8 点");
    assert_eq!(r.dof_after, 0, "{:#?}", r.suggestions);
    let count = |g: AutoGroup| r.suggestions.iter().filter(|x| x.group == g).count();
    assert_eq!(count(AutoGroup::Coincident), 5, "角 4 つと、左下の角と原点");
    assert_eq!(count(AutoGroup::HorizontalVertical), 4);
    assert_eq!(count(AutoGroup::Size), 2, "縦と横の長さ 1 本ずつ");
    // 左下の角は原点と一致するので、位置の寸法は要らない
    assert_eq!(count(AutoGroup::Position), 0);
    assert_eq!(
        count(AutoGroup::ParallelPerpendicular),
        0,
        "水平垂直で決まる"
    );
    assert!(
        r.suggestions
            .iter()
            .all(|x| !matches!(x.kind, K::Equal { .. })),
        "等しいは推測しない"
    );
    let sol = solve(&s, &HashMap::new()).unwrap();
    let t = apply_suggestions(&s, &sol, &r.suggestions);
    assert_eq!(solved_status(&t), SolveStatus::WellConstrained);
    // もう一度流しても、足す物は無い
    let again = run(
        &t,
        &AutoOptions {
            datum: Some(origin),
            ..Default::default()
        },
    );
    assert!(again.suggestions.is_empty(), "{:?}", again.suggestions);
}

#[test]
fn without_dimensions_only_constraints_are_suggested() {
    let (s, _) = loose_rect();
    let r = run(
        &s,
        &AutoOptions {
            dimensions: false,
            ..Default::default()
        },
    );
    assert!(r.suggestions.iter().all(|x| x.kind.value().is_none()));
    assert_eq!(r.dof_after, 2, "左下の角は原点と一致し、幅と高さが残る");
}

#[test]
fn the_rectangle_away_from_the_origin_gets_position_dimensions() {
    let mut s = Sketch::default();
    let origin = s.add_projection(&origin_ref(&kmb_core::Plane::XY));
    let c = [[5.0, 7.0], [25.0, 7.0], [25.0, 17.0], [5.0, 17.0]];
    for i in 0..4 {
        let (p, q) = (c[i], c[(i + 1) % 4]);
        let a = s.add_point(p[0], p[1]);
        let b = s.add_point(q[0], q[1]);
        s.add_line(a, b);
    }
    let r = run(
        &s,
        &AutoOptions {
            datum: Some(origin),
            ..Default::default()
        },
    );
    assert_eq!(r.dof_after, 0);
    let pos: Vec<&K> = r
        .suggestions
        .iter()
        .filter(|x| x.group == AutoGroup::Position)
        .map(|x| &x.kind)
        .collect();
    assert_eq!(pos.len(), 2, "{pos:?}");
    assert!(matches!(pos[0], K::HorizontalDistance { value, .. } if value == "5 mm"));
    assert!(matches!(pos[1], K::VerticalDistance { value, .. } if value == "7 mm"));
}

// 長穴(線 2 本と半円 2 つ)の端で、接するが付く
#[test]
fn a_slot_gets_tangents_and_no_equal_radius() {
    // 描いた長穴と同じく、つながる端は同じ点
    let mut s = Sketch::default();
    let p = |s: &mut Sketch, x: f64, y: f64| s.add_point(x, y);
    let (a1, b1) = (p(&mut s, 0.0, -5.0), p(&mut s, 20.0, -5.0));
    let (a2, b2) = (p(&mut s, 20.0, 5.0), p(&mut s, 0.0, 5.0));
    s.add_line(a1, b1);
    s.add_line(a2, b2);
    let (c1, c2) = (p(&mut s, 20.0, 0.0), p(&mut s, 0.0, 0.0));
    s.add_arc(c1, b1, a2);
    s.add_arc(c2, b2, a1);
    let r = run(&s, &AutoOptions::default());
    let tangents = r
        .suggestions
        .iter()
        .filter(|x| x.group == AutoGroup::Tangent)
        .count();
    assert_eq!(tangents, 4, "{:#?}", r.suggestions);
    let radii = r
        .suggestions
        .iter()
        .filter(|x| matches!(x.kind, K::Radius { .. }))
        .count();
    assert_eq!(
        radii, 1,
        "2 つ目の半径は接すると平行で決まる: {:#?}",
        r.suggestions
    );
    assert!(
        r.suggestions
            .iter()
            .all(|x| !matches!(x.kind, K::Equal { .. }))
    );
    let sol = solve(&s, &HashMap::new()).unwrap();
    let t = apply_suggestions(&s, &sol, &r.suggestions);
    assert!(matches!(
        solved_status(&t),
        SolveStatus::UnderConstrained { .. }
    ));
}

#[test]
fn a_tilted_line_is_not_made_horizontal_and_order_follows_the_table() {
    let mut s = Sketch::default();
    let (a, b) = (s.add_point(0.0, 0.0), s.add_point(10.0, 1.0));
    s.add_line(a, b);
    let (c, d) = (s.add_point(0.0, 0.0), s.add_point(0.0, 10.0));
    s.add_line(c, d);
    let (e, f) = (s.add_point(30.0, 0.0), s.add_point(30.0, 0.0));
    s.add_circle(e, 4.0);
    s.add_circle(f, 4.0);
    let r = run(&s, &AutoOptions::default());
    assert!(
        r.suggestions
            .iter()
            .all(|x| !matches!(x.kind, K::Horizontal { .. })),
        "傾き 5.7° は水平にしない"
    );
    assert!(
        r.suggestions
            .iter()
            .any(|x| matches!(x.kind, K::Concentric { .. })),
        "中心が重なる円は同心"
    );
    let groups: Vec<AutoGroup> = r.suggestions.iter().map(|x| x.group).collect();
    let mut sorted = groups.clone();
    sorted.sort();
    assert_eq!(groups, sorted, "種類の表の順");
    // 同じ大きさの円でも、直径は 1 つずつ
    let dia = r
        .suggestions
        .iter()
        .filter(|x| matches!(x.kind, K::Diameter { .. }))
        .count();
    assert_eq!(dia, 2);
}

#[test]
fn only_the_chosen_entities_get_constraints() {
    let (s, _) = loose_rect();
    let first = s
        .entities
        .iter()
        .find(|e| matches!(e, crate::Entity::Line { .. }))
        .unwrap()
        .id();
    let r = run(
        &s,
        &AutoOptions {
            dimensions: false,
            only: [first].into(),
            ..Default::default()
        },
    );
    // 選んだ線の水平と、その端の一致 3 つ(隣の線 2 本と原点)、隣の線との直角 2 つ。
    // 選ばなかった線どうしの拘束は作らない
    assert_eq!(r.suggestions.len(), 6, "{:#?}", r.suggestions);
}

/// 長方形 `n` × `n` 個(DXF から読んだような、拘束の無い図)。
fn many_rects(n: usize) -> Sketch {
    let mut s = Sketch::default();
    for i in 0..n {
        for j in 0..n {
            let (x, y) = (i as f64 * 30.0, j as f64 * 20.0);
            let c = [[x, y], [x + 20.0, y], [x + 20.0, y + 10.0], [x, y + 10.0]];
            for k in 0..4 {
                let (p, q) = (c[k], c[(k + 1) % 4]);
                let a = s.add_point(p[0], p[1]);
                let b = s.add_point(q[0], q[1]);
                s.add_line(a, b);
            }
        }
    }
    s
}

#[test]
fn a_drawing_of_a_hundred_lines_is_handled() {
    let s = many_rects(5);
    let t = std::time::Instant::now();
    let r = run(&s, &AutoOptions::default());
    eprintln!("線 100 本: {:?}、候補 {}", t.elapsed(), r.suggestions.len());
    // 位置の基準が無いので、長方形ごとに位置の 2 だけ残る
    assert_eq!(r.dof_after, 2 * 25);
}
