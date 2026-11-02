//! 確かめること: 線をつなげて描く途中の取り消しと、続きのクリック・Esc。

use crate::tests::{click_at, count_of, current_sketch, empty_sketch, is_line, press_key};
use crate::{SketchEditor, Tool};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kmb_sketch::Entity;
use kmb_ui::Workspace;

fn three_lines() -> Harness<'static, Workspace> {
    let mut h = empty_sketch();
    h.get_by_label("線分").click();
    h.run();
    for p in [[6.4, 8.54], [16.83, 8.54], [16.83, 16.36], [27.26, 16.36]] {
        click_at(&mut h, p);
    }
    assert_eq!(count_of(&current_sketch(h.state()), is_line), 3);
    assert_live_points(&h);
    h
}

fn undo(h: &mut Harness<'static, Workspace>) {
    h.get_by_label("元に戻す").click();
    h.run();
}

fn assert_live_points(h: &Harness<'static, Workspace>) {
    let sketch = current_sketch(h.state());
    let is_point = |id| matches!(sketch.entity(id), Some(Entity::Point { .. }));
    let e = &h.state().editor::<SketchEditor>().unwrap().editor;
    for id in [e.pending.and_then(|p| p.point), e.chain_start]
        .into_iter()
        .flatten()
    {
        assert!(is_point(id), "描きかけが消えた点 {id:?} を指している");
    }
    for entity in &sketch.entities {
        if let Entity::Line { a, b, .. } = entity {
            assert!(is_point(*a) && is_point(*b), "線の両端が今の点を指す");
        }
    }
    assert_eq!(h.state().report().error_count(), 0);
}

#[test]
fn undoing_a_line_chain_accepts_a_new_start_and_then_another_line() {
    let mut h = three_lines();
    undo(&mut h);
    assert_eq!(count_of(&current_sketch(h.state()), is_line), 2);
    assert_live_points(&h);
    assert_eq!(
        h.state().editor::<SketchEditor>().unwrap().editor.tool,
        Tool::Line
    );
    click_at(&mut h, [-9.24, 5.93]);
    assert_eq!(count_of(&current_sketch(h.state()), is_line), 2);
    click_at(&mut h, [-4.03, 5.93]);
    assert_eq!(count_of(&current_sketch(h.state()), is_line), 3);
    assert_live_points(&h);
}

#[test]
fn undoing_every_line_clears_the_chain_start_and_allows_drawing_again() {
    let mut h = three_lines();
    for remaining in (0..3).rev() {
        undo(&mut h);
        assert_eq!(count_of(&current_sketch(h.state()), is_line), remaining);
        assert_live_points(&h);
    }
    let e = &h.state().editor::<SketchEditor>().unwrap().editor;
    assert!(e.pending.is_none() && e.chain_start.is_none());
    click_at(&mut h, [1.19, 11.15]);
    click_at(&mut h, [9.01, 16.36]);
    assert_eq!(count_of(&current_sketch(h.state()), is_line), 1);
    assert_live_points(&h);
}

#[test]
fn esc_after_undo_leaves_the_line_tool_without_finishing_the_sketch() {
    let mut h = three_lines();
    let id = h.state().selected().unwrap();
    undo(&mut h);
    click_at(&mut h, [-9.24, 5.93]);
    let before = current_sketch(h.state());
    press_key(&mut h, egui::Key::Escape);
    let e = &h.state().editor::<SketchEditor>().unwrap().editor;
    assert_eq!(e.tool, Tool::Select);
    assert!(e.pending.is_none() && e.chain_start.is_none());
    assert!(h.state().is_editing(id));
    assert_eq!(current_sketch(h.state()), before);
    press_key(&mut h, egui::Key::Escape);
    assert!(
        h.state().is_editing(id),
        "選ぶ状態の最初のEscで編集を終えない"
    );
}
