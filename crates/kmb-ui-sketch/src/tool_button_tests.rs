//! 確かめること: 道具の同じボタンで抜け、選びを保ち、直接の道具指定は続ける。

use crate::tests::{click_at, current_sketch, editor, empty_sketch};
use crate::Tool;
use egui_kittest::kittest::Queryable;

#[test]
fn active_tool_button_returns_to_selection_and_discards_the_pending_line() {
    let mut h = empty_sketch();
    let before = current_sketch(h.state());
    h.get_by_label("線分").click();
    h.run();
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::Line);
    click_at(&mut h, [1.19, 18.97]);
    assert!(editor(h.state_mut()).editor.pending.is_some());
    h.get_by_label("線分").click();
    h.run();
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::Select);
    assert!(editor(h.state_mut()).editor.pending.is_none());
    assert_eq!(current_sketch(h.state()), before);
}

#[test]
fn active_tool_button_keeps_the_selected_entities() {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Point);
    h.run();
    click_at(&mut h, [1.19, 18.97]);
    let point = current_sketch(h.state()).entities[0].id();
    editor(h.state_mut()).editor.selection.insert(point);
    let selected = editor(h.state_mut()).editor.selection.clone();
    h.get_by_label("線分").click();
    h.run();
    h.get_by_label("線分").click();
    h.run();
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::Select);
    assert_eq!(editor(h.state_mut()).editor.selection, selected);
}

#[test]
fn a_different_tool_button_switches_and_direct_selection_continues_the_same_tool() {
    let mut h = empty_sketch();
    h.get_by_label("線分").click();
    h.run();
    h.get_by_label("長方形").click();
    h.run();
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::Rectangle);
    editor(h.state_mut()).set_tool(Tool::Rectangle);
    h.run();
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::Rectangle);
}
