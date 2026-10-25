//! 拘束の種類の残り(網羅版 E3: 図形の固定、図形どうしの対称、ポリゴン)を、人の操作で確かめる。

use crate::Tool;
use crate::tests::*;
use egui_kittest::Harness;
use jc_sketch::{ConstraintKind, Entity};
use jc_ui::Workspace;

fn solution(h: &Harness<'static, Workspace>) -> jc_sketch::Solution {
    let s = current_sketch(h.state());
    let v = jc_sketch::dimension_values(&s, &|_| None).unwrap();
    jc_sketch::solve(&s, &v).unwrap()
}

fn draw_line(h: &mut Harness<'static, Workspace>, a: [f64; 2], b: [f64; 2]) {
    editor(h.state_mut()).set_tool(Tool::Line);
    h.run();
    click_at(h, a);
    click_at(h, b);
    press_key(h, egui::Key::Escape);
    press_key(h, egui::Key::Escape);
}

#[test]
fn fix_on_a_line_fixes_both_ends() {
    let mut h = sketch_with_rectangle();
    editor(h.state_mut()).set_tool(Tool::Constraint("固定"));
    h.run();
    click_at(&mut h, [6.4, 16.36]);
    let s = current_sketch(h.state());
    let fixes = s
        .constraints
        .iter()
        .filter(|c| matches!(c.kind, ConstraintKind::Fix { .. }))
        .count();
    assert_eq!(fixes, 2, "{:?}", s.constraints);
}

#[test]
fn two_lines_become_symmetric_about_a_third() {
    let mut h = empty_sketch();
    draw_line(&mut h, [-1.42, 16.36], [0.15, 8.54]);
    draw_line(&mut h, [14.74, 15.84], [12.4, 9.06]);
    draw_line(&mut h, [6.4, 18.97], [6.4, 5.93]);
    editor(h.state_mut()).set_tool(Tool::Constraint("対称"));
    h.run();
    click_at(&mut h, [-0.64, 12.45]);
    click_at(&mut h, [13.55, 12.45]);
    click_at(&mut h, [6.4, 17.92]);
    let s = current_sketch(h.state());
    let syms = s
        .constraints
        .iter()
        .filter(|c| matches!(c.kind, ConstraintKind::Symmetric { .. }))
        .count();
    assert_eq!(syms, 2, "{:?}", s.constraints);
    // 解くと 2 本は軸について鏡に写り合う(端の y がそろい、x が軸から同じだけ離れる)
    let sol = solution(&h);
    let lines: Vec<_> = s
        .entities
        .iter()
        .filter_map(|e| match e {
            Entity::Line { a, b, .. } => Some((sol.points[a], sol.points[b])),
            _ => None,
        })
        .collect();
    let axis_x = lines[2].0[0];
    let mut ys1 = [lines[0].0[1], lines[0].1[1]];
    let mut ys2 = [lines[1].0[1], lines[1].1[1]];
    ys1.sort_by(f64::total_cmp);
    ys2.sort_by(f64::total_cmp);
    assert!((ys1[0] - ys2[0]).abs() < 1e-6 && (ys1[1] - ys2[1]).abs() < 1e-6);
    // 1 本目のどちらの端も、軸で折り返すと 2 本目のどちらかの端に重なる
    for p in [lines[0].0, lines[0].1] {
        let mirror = [2.0 * axis_x - p[0], p[1]];
        assert!(
            [lines[1].0, lines[1].1]
                .iter()
                .any(|q| (q[0] - mirror[0]).abs() < 1e-6 && (q[1] - mirror[1]).abs() < 1e-6),
            "軸 {axis_x}: {p:?} の写し {mirror:?} が 2 本目の端にない {:?}",
            lines[1]
        );
    }
}

#[test]
fn a_line_drawn_the_other_way_is_not_turned_over_by_symmetry() {
    // 10-22 t1: 2 本目を逆向き(下から上)に描いても、対称を付けた途端に片方が裏返って X の
    // 形にならない。端は近い方どうしで組になり、どの端も描いた所から大きく動かない
    let mut h = empty_sketch();
    draw_line(&mut h, [-1.42, 16.36], [0.15, 8.54]);
    draw_line(&mut h, [12.4, 9.06], [14.74, 15.84]);
    draw_line(&mut h, [6.4, 18.97], [6.4, 5.93]);
    editor(h.state_mut()).set_tool(Tool::Constraint("対称"));
    h.run();
    click_at(&mut h, [-0.64, 12.45]);
    click_at(&mut h, [13.55, 12.45]);
    click_at(&mut h, [6.4, 17.92]);
    let s = current_sketch(h.state());
    let syms = s
        .constraints
        .iter()
        .filter(|c| matches!(c.kind, ConstraintKind::Symmetric { .. }))
        .count();
    assert_eq!(syms, 2, "{:?}", s.constraints);
    let sol = solution(&h);
    let lines: Vec<_> = s
        .entities
        .iter()
        .filter_map(|e| match e {
            Entity::Line { a, b, .. } => Some((sol.points[a], sol.points[b])),
            _ => None,
        })
        .collect();
    let drawn = [
        ([-1.42, 16.36], [0.15, 8.54]),
        ([12.4, 9.06], [14.74, 15.84]),
    ];
    for (got, want) in lines.iter().zip(drawn) {
        for (g, w) in [(got.0, want.0), (got.1, want.1)] {
            let d = (g[0] - w[0]).hypot(g[1] - w[1]);
            assert!(
                d < 2.0,
                "描いた所 {w:?} から {d} mm 動いた(裏返り): {lines:?}"
            );
        }
    }
    // 2 本は交わらない(左の線は軸の左、右の線は軸の右のまま)
    let axis_x = lines[2].0[0];
    assert!(lines[0].0[0] < axis_x && lines[0].1[0] < axis_x);
    assert!(lines[1].0[0] > axis_x && lines[1].1[0] > axis_x);
}

#[test]
fn polygon_makes_a_closed_loop_regular() {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Line);
    h.run();
    // ゆがんだ四角を描いて閉じる
    for p in [
        [1.19, 16.36],
        [11.62, 15.32],
        [10.57, 6.98],
        [2.23, 8.54],
        [1.19, 16.36],
    ] {
        click_at(&mut h, p);
    }
    press_key(&mut h, egui::Key::Escape);
    press_key(&mut h, egui::Key::Escape);
    editor(h.state_mut()).set_tool(Tool::Constraint("ポリゴン"));
    h.run();
    click_at(&mut h, [6.4, 15.84]);
    let s = current_sketch(h.state());
    assert!(
        s.entities.iter().any(|e| matches!(
            e,
            Entity::Circle {
                construction: true,
                ..
            }
        )),
        "補助の円"
    );
    let sol = solution(&h);
    let lens: Vec<f64> = s
        .entities
        .iter()
        .filter_map(|e| match e {
            Entity::Line { a, b, .. } => {
                let (p, q) = (sol.points[a], sol.points[b]);
                Some((p[0] - q[0]).hypot(p[1] - q[1]))
            }
            _ => None,
        })
        .collect();
    assert_eq!(lens.len(), 4);
    for l in &lens {
        assert!((l - lens[0]).abs() < 1e-6, "辺が等しい {lens:?}");
    }
    assert_eq!(h.state().report().error_count(), 0);
}

#[test]
fn horizontal_or_vertical_follows_the_direction_of_the_line() {
    // 水平と垂直は 1 つの道具(Kie 2026-10-21): 縦に近い線は垂直、
    // 横に近い線は水平になる
    let mut h = empty_sketch();
    draw_line(&mut h, [-1.42, 16.36], [0.15, 8.54]);
    draw_line(&mut h, [2.0, 3.0], [11.42, 1.14]);
    let kinds = |h: &Harness<'static, Workspace>| -> (usize, usize) {
        let s = current_sketch(h.state());
        let n =
            |f: fn(&ConstraintKind) -> bool| s.constraints.iter().filter(|c| f(&c.kind)).count();
        (
            n(|k| matches!(k, ConstraintKind::Horizontal { .. })),
            n(|k| matches!(k, ConstraintKind::Vertical { .. })),
        )
    };
    assert_eq!(kinds(&h), (0, 0), "斜めの線には自動で付かない");
    editor(h.state_mut()).set_tool(Tool::Constraint("水平/垂直"));
    h.run();
    click_at(&mut h, [-0.64, 12.45]);
    assert_eq!(kinds(&h), (0, 1), "縦に近い線は垂直");
    click_at(&mut h, [6.71, 2.07]);
    assert_eq!(kinds(&h), (1, 1), "横に近い線は水平");
}
