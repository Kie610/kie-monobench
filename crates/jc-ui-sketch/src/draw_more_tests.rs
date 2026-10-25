//! 描く道具の残り(網羅版 C3・C5・C7・C8・C9)を、人の操作(クリック、キー)で確かめる。

use crate::Tool;
use crate::tests::*;
use egui_kittest::Harness;
use jc_sketch::{ConstraintKind, Entity};
use jc_ui::Workspace;

fn kinds(h: &Harness<'static, Workspace>, f: impl Fn(&ConstraintKind) -> bool) -> usize {
    current_sketch(h.state())
        .constraints
        .iter()
        .filter(|c| f(&c.kind))
        .count()
}

fn arcs(h: &Harness<'static, Workspace>) -> usize {
    count_of(&current_sketch(h.state()), |e| {
        matches!(e, Entity::Arc { .. })
    })
}

fn circles(h: &Harness<'static, Workspace>) -> usize {
    count_of(&current_sketch(h.state()), |e| {
        matches!(e, Entity::Circle { .. })
    })
}

fn stop(h: &mut Harness<'static, Workspace>) {
    press_key(h, egui::Key::Escape);
    press_key(h, egui::Key::Escape);
}

fn draw_line(h: &mut Harness<'static, Workspace>, a: [f64; 2], b: [f64; 2]) {
    editor(h.state_mut()).set_tool(Tool::Line);
    h.run();
    click_at(h, a);
    click_at(h, b);
    stop(h);
}

fn draw_circle(h: &mut Harness<'static, Workspace>, c: [f64; 2], r: f64) {
    editor(h.state_mut()).set_tool(Tool::Circle);
    h.run();
    click_at(h, c);
    click_at(h, [c[0] + r, c[1]]);
    stop(h);
}

#[test]
fn tangent_arc_continues_from_the_end_of_an_arc() {
    let mut h = empty_sketch();
    // 中心、始まり、終わりで円弧(右から上へ反時計回り)
    editor(h.state_mut()).set_tool(Tool::Arc);
    h.run();
    click_at(&mut h, [1.19, 11.15]);
    click_at(&mut h, [4.32, 11.15]);
    click_at(&mut h, [1.19, 14.28]);
    stop(&mut h);
    assert_eq!(arcs(&h), 1);
    // 円弧の端(上)から接線円弧を左下へ
    editor(h.state_mut()).set_tool(Tool::TangentArc);
    h.run();
    click_at(&mut h, [1.19, 14.28]);
    click_at(&mut h, [-2.98, 14.8]);
    stop(&mut h);
    assert_eq!(arcs(&h), 2);
    assert_eq!(
        kinds(&h, |k| matches!(k, ConstraintKind::TangentCircles { .. })),
        1
    );
    // 解いても崩れない(継ぎ目で接したまま): 2 つの中心と継ぎ目が一直線に並ぶ
    let sol = solved(h.state());
    let at = |id| sol.points[&id];
    let arcs: Vec<_> = current_sketch(h.state())
        .entities
        .iter()
        .filter_map(|e| match e {
            Entity::Arc {
                center, start, end, ..
            } => Some((at(*center), [at(*start), at(*end)])),
            _ => None,
        })
        .collect();
    let (c1, ends1) = arcs[0];
    let (c2, ends2) = arcs[1];
    let d = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]);
    let joint = ends1
        .iter()
        .find(|p| ends2.iter().any(|q| d(**p, *q) < 1e-6))
        .copied()
        .expect("2 つの円弧の端が重なる");
    let (u, v) = (
        [c1[0] - joint[0], c1[1] - joint[1]],
        [c2[0] - joint[0], c2[1] - joint[1]],
    );
    let sin = (u[0] * v[1] - u[1] * v[0]) / (d(c1, joint) * d(c2, joint));
    assert!(sin.abs() < 1e-6, "継ぎ目で折れている: {sin}");
    // それぞれの円弧の端は、自分の中心から同じ距離
    for (c, [a, b]) in &arcs {
        assert!((d(*c, *a) - d(*c, *b)).abs() < 1e-6);
    }
}

#[test]
fn midpoint_line_is_drawn_from_its_middle() {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::MidpointLine);
    h.run();
    click_at(&mut h, [1.19, 11.15]);
    click_at(&mut h, [5.36, 13.23]);
    stop(&mut h);
    let s = current_sketch(h.state());
    assert_eq!(count_of(&s, is_line), 1);
    assert_eq!(
        kinds(&h, |k| matches!(k, ConstraintKind::Midpoint { .. })),
        1
    );
    // 中点は最初にクリックした所(両端の真ん中)
    let sol = solved(h.state());
    let (a, b) = s
        .entities
        .iter()
        .find_map(|e| match e {
            Entity::Line { a, b, .. } => Some((sol.points[a], sol.points[b])),
            _ => None,
        })
        .unwrap();
    let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
    let m = s
        .constraints
        .iter()
        .find_map(|c| match c.kind {
            ConstraintKind::Midpoint { point, .. } => Some(sol.points[&point]),
            _ => None,
        })
        .unwrap();
    assert!((mid[0] - m[0]).abs() < 1e-6 && (mid[1] - m[1]).abs() < 1e-6);
}

#[test]
fn circle_tangent_to_a_line_and_a_circle() {
    let mut h = empty_sketch();
    draw_line(&mut h, [-4.03, 5.93], [11.62, 5.93]);
    draw_circle(&mut h, [4.32, 12.19], 2.09);
    editor(h.state_mut()).set_tool(Tool::TwoTangentCircle);
    h.run();
    click_at(&mut h, [-1.42, 5.93]);
    click_at(&mut h, [6.4, 12.19]);
    // 線分の上、円の左下に置く
    click_at(&mut h, [0.15, 7.5]);
    stop(&mut h);
    assert_eq!(circles(&h), 2);
    assert_eq!(
        kinds(&h, |k| matches!(k, ConstraintKind::Tangent { .. })),
        1
    );
    assert_eq!(
        kinds(&h, |k| matches!(k, ConstraintKind::TangentCircles { .. })),
        1
    );
}

#[test]
fn circle_tangent_to_two_lines_and_a_circle() {
    let mut h = empty_sketch();
    draw_line(&mut h, [-4.03, 5.93], [11.62, 5.93]);
    draw_line(&mut h, [-5.07, 18.97], [-5.07, 4.89]);
    draw_circle(&mut h, [2.23, 11.15], 2.09);
    editor(h.state_mut()).set_tool(Tool::TangentCircle);
    h.run();
    click_at(&mut h, [6.4, 5.93]);
    click_at(&mut h, [-5.07, 16.36]);
    click_at(&mut h, [4.32, 11.15]);
    // 2 本の角と円の間
    click_at(&mut h, [-2.98, 7.5]);
    stop(&mut h);
    assert_eq!(circles(&h), 2);
    assert_eq!(
        kinds(&h, |k| matches!(k, ConstraintKind::Tangent { .. })),
        2
    );
    assert_eq!(
        kinds(&h, |k| matches!(k, ConstraintKind::TangentCircles { .. })),
        1
    );
}

#[test]
fn polygon_from_one_edge_uses_the_typed_count() {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::EdgePolygon);
    h.run();
    // 辺の数は先に打つ
    type_text(&mut h, "5");
    click_at(&mut h, [1.19, 11.15]);
    click_at(&mut h, [4.84, 11.15]);
    click_at(&mut h, [3.01, 14.28]);
    stop(&mut h);
    let s = current_sketch(h.state());
    assert_eq!(count_of(&s, is_line), 5);
    // 辺の等しさと頂点の円は、1 つのポリゴンの拘束(印も 1 つ)
    assert_eq!(kinds(&h, |k| matches!(k, ConstraintKind::Equal { .. })), 0);
    assert_eq!(
        kinds(
            &h,
            |k| matches!(k, ConstraintKind::Polygon { sides, .. } if sides.len() == 5)
        ),
        1
    );
    assert_eq!(editor(h.state_mut()).editor.polygon_n, 5);
}

#[test]
fn three_point_arc_slot_with_typed_width() {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::ThreePointArcSlot);
    h.run();
    click_at(&mut h, [-2.98, 10.1]);
    click_at(&mut h, [5.36, 10.1]);
    click_at(&mut h, [1.19, 13.23]);
    type_text(&mut h, "6");
    press_enter(&mut h);
    stop(&mut h);
    assert_eq!(arcs(&h), 4);
    assert_eq!(
        kinds(
            &h,
            |k| matches!(k, ConstraintKind::Diameter { value, .. } if value == "6 mm")
        ),
        1
    );
}

/// 長穴の見本の端は、できあがりと同じ半円: 幅の両端を結び、向かいの端と逆の側へふくらむ
#[test]
fn slot_preview_ends_are_half_circles_facing_out() {
    // 端の中心 (10, 0)、向かいの端は原点。幅の半分 2
    let cap = crate::view::slot_cap([10.0, 0.0], [0.0, 2.0], [1.0, 0.0]);
    let (first, last) = (cap[0], cap[cap.len() - 1]);
    assert!((first[0] - 10.0).abs() < 1e-9 && (first[1].abs() - 2.0).abs() < 1e-9);
    assert!((last[0] - 10.0).abs() < 1e-9 && (last[1] + first[1]).abs() < 1e-9);
    // どの点も中心から 2、向かいの端の側(x < 10)へは出ない
    for p in &cap {
        assert!(((p[0] - 10.0).hypot(p[1]) - 2.0).abs() < 1e-9);
        assert!(p[0] >= 10.0 - 1e-9, "{p:?}");
    }
    assert!(cap.iter().any(|p| (p[0] - 12.0).abs() < 1e-6));
}

/// 道具の欄に打つ数も、全角の数字・小数点・符号を普通の数として読む。式は数として扱わない
#[test]
fn typed_lengths_accept_full_width_digits() {
    use crate::view::{length_mm, typed_length};
    assert_eq!(typed_length("１２．５ mm"), Some(12.5));
    assert_eq!(length_mm("－３mm"), Some(-3.0));
    assert_eq!(length_mm("1e1 mm"), Some(10.0));
    assert_eq!(length_mm("10-2 mm"), None);
}

/// 角を丸める・落とす道具の既定の値と知らせの長さは、設定の単位で出す
#[test]
fn corner_defaults_follow_the_display_unit() {
    use crate::view::{default_corner, shown_length};
    assert_eq!(default_corner(), "2 mm");
    jc_core::set_display_unit(jc_core::LengthUnit::In);
    assert_eq!(default_corner(), "0.0625 in");
    assert!(shown_length(25.4).ends_with("in"), "{}", shown_length(25.4));
    match crate::more_tools::chamfer_kind("") {
        jc_sketch::Chamfer::Equal { d, value } => {
            assert!((d - 1.5875).abs() < 1e-9, "{d}");
            assert_eq!(value, "0.0625 in");
        }
        other => panic!("{other:?}"),
    }
    jc_core::set_display_unit(jc_core::LengthUnit::Mm);
}
