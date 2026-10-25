//! 描く道具の残り(網羅版 C2・C5・C8・C9・B14・C14)を、人の操作(クリック、キー、引く)で確かめる。

use crate::Tool;
use crate::tests::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use jc_sketch::{Entity, EntityId};
use jc_ui::Workspace;

fn line_ends(h: &Harness<'static, Workspace>, id: EntityId) -> ([f64; 2], [f64; 2]) {
    let s = current_sketch(h.state());
    let v = jc_sketch::dimension_values(&s, &|_| None).unwrap();
    let sol = jc_sketch::solve(&s, &v).unwrap();
    match s.entity(id) {
        Some(Entity::Line { a, b, .. }) => (sol.points[a], sol.points[b]),
        e => panic!("線分でない {e:?}"),
    }
}

fn lines(h: &Harness<'static, Workspace>) -> Vec<EntityId> {
    current_sketch(h.state())
        .entities
        .iter()
        .filter(|e| is_line(e))
        .map(|e| e.id())
        .collect()
}

#[test]
fn the_second_line_angle_is_measured_from_the_previous_line() {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Line);
    h.run();
    click_at(&mut h, [1.19, 11.15]);
    click_at(&mut h, [6.4, 11.15]);
    // 上へカーソルを出し、長さ 30 と、Tab で角度 60 を打って Enter
    move_to(&mut h, [6.4, 16.36]);
    h.run();
    type_text(&mut h, "30");
    press_key(&mut h, egui::Key::Tab);
    type_text(&mut h, "60");
    press_enter(&mut h);
    let ls = lines(&h);
    assert_eq!(ls.len(), 2);
    let (p, q) = line_ends(&h, ls[1]);
    let (d0, d1) = (q[0] - p[0], q[1] - p[1]);
    // 前の線分(右向き)との角が 60°: 鋭く折り返して左上へ(X 軸から 60° なら右上になる)
    assert!((d0.hypot(d1) - 30.0).abs() < 1e-6, "長さ {}", d0.hypot(d1));
    assert!(d0 < 0.0 && d1 > 0.0, "左上へ {d0} {d1}");
    assert!((d1.atan2(d0).to_degrees() - 120.0).abs() < 1e-6);
}

#[test]
fn dragging_from_the_line_end_draws_a_tangent_arc_and_the_line_continues() {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Line);
    h.run();
    click_at(&mut h, [1.19, 11.15]);
    click_at(&mut h, [6.4, 11.15]);
    drag_on(&mut h, [6.4, 11.15], [9.53, 14.28], egui::Modifiers::NONE);
    let s = current_sketch(h.state());
    let arc = s
        .entities
        .iter()
        .find(|e| matches!(e, Entity::Arc { .. }))
        .expect("接線円弧");
    assert!(
        s.constraints.iter().any(|c| matches!(
            c.kind,
            jc_sketch::ConstraintKind::Tangent { circle, .. } if circle == arc.id()
        )),
        "線分に接する"
    );
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::Line);
    // 円弧の終わりから線分が続く
    click_at(&mut h, [11.62, 16.36]);
    let s = current_sketch(h.state());
    assert_eq!(count_of(&s, is_line), 2);
    let Some(Entity::Arc { start, end, .. }) = s.entity(arc.id()).cloned() else {
        unreachable!()
    };
    let last = *lines(&h).last().unwrap();
    let Some(Entity::Line { a, .. }) = s.entity(last).cloned() else {
        unreachable!()
    };
    assert!(a == start || a == end, "円弧の端から続く");
    assert_eq!(h.state().report().error_count(), 0);
}

#[test]
fn polygon_sides_typed_before_the_center_are_kept_and_remembered() {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Polygon);
    h.run();
    type_text(&mut h, "8");
    assert_eq!(editor(h.state_mut()).editor.polygon_n, 8);
    click_at(&mut h, [1.19, 11.15]);
    click_at(&mut h, [4.32, 11.15]);
    assert_eq!(count_of(&current_sketch(h.state()), is_line), 8);
    // 次の多角形も 8 角(前の数を覚える)
    click_at(&mut h, [16.83, 11.15]);
    click_at(&mut h, [19.44, 11.15]);
    assert_eq!(count_of(&current_sketch(h.state()), is_line), 16);
}

#[test]
fn move_started_without_a_selection_lets_you_pick_then_enter() {
    let mut h = sketch_with_rectangle();
    editor(h.state_mut()).editor.selection.clear();
    editor(h.state_mut()).set_tool(Tool::Move);
    h.run();
    assert!(editor(h.state_mut()).editor.choosing);
    // 上の辺と右の辺をクリックで選び、右の辺はもう一度押して外す
    click_at(&mut h, [6.4, 16.36]);
    click_at(&mut h, [11.62, 12.19]);
    click_at(&mut h, [11.62, 12.19]);
    assert_eq!(editor(h.state_mut()).editor.selection.len(), 1);
    press_enter(&mut h);
    assert!(!editor(h.state_mut()).editor.choosing);
    let top = *editor(h.state_mut())
        .editor
        .selection
        .iter()
        .next()
        .unwrap();
    let before = line_ends(&h, top);
    click_at(&mut h, [6.4, 21.57]);
    click_at(&mut h, [6.4, 24.18]);
    let after = line_ends(&h, top);
    assert!(
        after.0[1] > before.0[1] + 1.0,
        "上へ動く {before:?} → {after:?}"
    );
}

#[test]
fn mirror_uses_the_centerline_with_enter() {
    let mut h = sketch_with_rectangle();
    // 左の辺を中心線にする
    let s = current_sketch(h.state());
    let sol = solved(h.state());
    let minx = sol.points.values().map(|p| p[0]).fold(f64::MAX, f64::min);
    let left = s
        .entities
        .iter()
        .find_map(|e| match e {
            Entity::Line { id, a, b, .. }
                if (sol.points[a][0] - minx).abs() < 1e-6
                    && (sol.points[b][0] - minx).abs() < 1e-6 =>
            {
                Some(*id)
            }
            _ => None,
        })
        .unwrap();
    editor(h.state_mut()).editor.selection.insert(left);
    h.run();
    h.get_by_label("中心線の切り替え").click();
    h.run();
    editor(h.state_mut()).editor.selection.clear();
    editor(h.state_mut()).set_tool(Tool::MirrorAxis);
    h.run();
    // 右の辺を選び、Enter で選び終え、もう一度 Enter で中心線を対称の線にする
    click_at(&mut h, [11.62, 12.19]);
    press_enter(&mut h);
    let n = lines(&h).len();
    press_enter(&mut h);
    let ls = lines(&h);
    assert_eq!(ls.len(), n + 1, "写しが 1 本できる");
    let (p, q) = line_ends(&h, *ls.last().unwrap());
    assert!(
        p[0] < minx - 1.0 && q[0] < minx - 1.0,
        "中心線の左へ写る {p:?} {q:?}"
    );
}
