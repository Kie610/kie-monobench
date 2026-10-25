//! 拘束の道具・推測の拘束・つながりの選択・オフセットなどを、人と同じマウスとキーの操作で確かめる。

use super::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kmb_core::Document;
use kmb_sketch::{ConstraintKind as K, Entity};
use kmb_ui::Workspace;

fn empty_sketch() -> Harness<'static, Workspace> {
    let ws = Workspace::new(Document::new(), None, kmb_bundle::modules())
        .with_editor(SketchEditor::new());
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        // 1 コマを人の画面と同じ長さに(既定の 0.25 秒では二度押しにならない)
        .with_step_dt(1.0 / 60.0)
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h.state_mut().run_action(kmb_sketch::ADD_SKETCH);
    h.run();
    h
}

fn ed<'a>(h: &'a mut Harness<'static, Workspace>) -> &'a mut view::Editor {
    &mut h
        .state_mut()
        .editor_mut::<SketchEditor>()
        .expect("組み込んである")
        .editor
}

fn tool(h: &mut Harness<'static, Workspace>, t: Tool) {
    h.state_mut()
        .editor_mut::<SketchEditor>()
        .unwrap()
        .set_tool(t);
    h.run();
}

fn sketch(h: &Harness<'static, Workspace>) -> kmb_sketch::Sketch {
    let ws = h.state();
    let id = ws.selected().unwrap();
    serde_json::from_value(ws.document().data().feature(id).unwrap().data.clone()).unwrap()
}

fn has(h: &Harness<'static, Workspace>, f: impl Fn(&K) -> bool) -> bool {
    sketch(h).constraints.iter().any(|c| f(&c.kind))
}

fn press(h: &mut Harness<'static, Workspace>, p: [f64; 2], double: bool) {
    let pos = crate::tests::screen(h, p);
    h.hover_at(pos);
    h.run();
    for _ in 0..if double { 2 } else { 1 } {
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
            h.step();
        }
    }
    h.run();
}

fn click(h: &mut Harness<'static, Workspace>, p: [f64; 2]) {
    press(h, p, false);
}

fn key(h: &mut Harness<'static, Workspace>, k: egui::Key) {
    for pressed in [true, false] {
        h.event(egui::Event::Key {
            key: k,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        h.run();
    }
}

/// 線分を 1 本ずつ描く(描いたら Esc で区切る)。
fn line(h: &mut Harness<'static, Workspace>, a: [f64; 2], b: [f64; 2]) {
    tool(h, Tool::Line);
    click(h, a);
    click(h, b);
    key(h, egui::Key::Escape);
}

#[test]
fn a_constraint_tool_is_picked_first_and_stays_for_the_next_pair() {
    let mut h = empty_sketch();
    line(&mut h, [-4.03, 16.36], [6.4, 18.97]);
    line(&mut h, [-4.03, 5.93], [7.45, 10.1]);
    line(&mut h, [16.83, 16.36], [22.04, 21.57]);
    line(&mut h, [16.83, 5.93], [24.65, 6.98]);
    assert!(ed(&mut h).selection.is_empty());
    // 何も選ばずに帯の「平行」を押すと、拘束の道具になる(断らない)
    h.get_by_label("平行").click();
    h.run();
    assert_eq!(ed(&mut h).tool, Tool::Constraint("平行"));
    click(&mut h, [1.19, 17.66]);
    assert_eq!(ed(&mut h).selection.len(), 1);
    click(&mut h, [1.71, 8.02]);
    assert!(has(&h, |k| matches!(k, K::Parallel { .. })));
    // 道具は続き、次の組にも付けられる
    assert_eq!(ed(&mut h).tool, Tool::Constraint("平行"));
    assert!(ed(&mut h).selection.is_empty());
    click(&mut h, [19.44, 18.97]);
    click(&mut h, [20.74, 6.45]);
    let n = sketch(&h)
        .constraints
        .iter()
        .filter(|c| matches!(c.kind, K::Parallel { .. }))
        .count();
    assert_eq!(n, 2);
    // 点は平行に使えないので選ばず、知らせる
    click(&mut h, [-4.03, 16.36]);
    assert!(ed(&mut h).selection.is_empty());
    assert!(ed(&mut h).notice.is_some());
    // Esc で選ぶ道具へ
    key(&mut h, egui::Key::Escape);
    assert_eq!(ed(&mut h).tool, Tool::Select);
}

#[test]
fn coincident_from_the_tool_puts_a_point_on_a_line() {
    let mut h = empty_sketch();
    line(&mut h, [-4.03, 16.36], [6.4, 16.36]);
    tool(&mut h, Tool::Point);
    click(&mut h, [11.62, 11.15]);
    tool(&mut h, Tool::Select);
    h.get_by_label("一致").click();
    h.run();
    click(&mut h, [11.62, 11.15]);
    click(&mut h, [-1.42, 16.36]);
    assert!(has(&h, |k| matches!(k, K::PointOnLine { .. })));
}

#[test]
fn a_line_drawn_square_to_the_previous_one_gets_a_right_angle() {
    let mut h = empty_sketch();
    tool(&mut h, Tool::Line);
    click(&mut h, [-4.03, 16.36]);
    click(&mut h, [6.4, 18.97]);
    // 前の線に直角な向き(画面で (50, 200))から 2° ほどずらして引く
    click(&mut h, [9.37, 8.59]);
    key(&mut h, egui::Key::Escape);
    assert!(has(&h, |k| matches!(k, K::Perpendicular { .. })));
    assert!(!has(&h, |k| matches!(
        k,
        K::Horizontal { .. } | K::Vertical { .. }
    )));
}

#[test]
fn a_point_dropped_on_a_line_stays_on_it_and_one_on_a_point_coincides() {
    let mut h = empty_sketch();
    line(&mut h, [-4.03, 16.36], [6.4, 16.36]);
    tool(&mut h, Tool::Point);
    // 線の上(中点ではない所)
    click(&mut h, [-1.31, 16.26]);
    assert!(has(&h, |k| matches!(k, K::PointOnLine { .. })));
    // 線の端の上: 重ねるだけでなく一致させる
    tool(&mut h, Tool::Point);
    click(&mut h, [6.4, 16.36]);
    assert!(has(&h, |k| matches!(k, K::Coincident { .. })));
}

#[test]
fn a_double_click_selects_the_whole_connected_outline() {
    let mut h = empty_sketch();
    tool(&mut h, Tool::Rectangle);
    click(&mut h, [1.19, 16.36]);
    click(&mut h, [11.62, 8.54]);
    line(&mut h, [22.04, 16.36], [27.26, 16.36]);
    tool(&mut h, Tool::Select);
    press(&mut h, [6.4, 16.36], true);
    let s = sketch(&h);
    let lines: Vec<_> = ed(&mut h)
        .selection
        .iter()
        .filter(|id| matches!(s.entity(**id), Some(Entity::Line { .. })))
        .copied()
        .collect();
    assert_eq!(lines.len(), 4, "長方形の 4 辺だけ(離れた線は選ばない)");
}

#[test]
fn the_dimension_value_does_not_linger_in_the_panel() {
    let mut h = empty_sketch();
    tool(&mut h, Tool::Rectangle);
    click(&mut h, [1.19, 16.36]);
    click(&mut h, [11.62, 8.54]);
    key(&mut h, egui::Key::D);
    click(&mut h, [6.4, 16.36]);
    // 置く前に測った値がパネルの欄に入る
    assert!(!ed(&mut h).dim_text.is_empty());
    // 道具を変えると空に戻る(残ると次の角の丸めやオフセットの値に使われる)
    key(&mut h, egui::Key::Escape);
    key(&mut h, egui::Key::Escape);
    assert!(ed(&mut h).dim_text.is_empty(), "{:?}", ed(&mut h).dim_text);
}

#[test]
fn offsetting_one_side_of_a_slot_offsets_the_whole_slot_with_its_arcs() {
    let mut h = empty_sketch();
    tool(&mut h, Tool::Slot);
    click(&mut h, [-1.42, 11.15]);
    click(&mut h, [9.01, 11.15]);
    click(&mut h, [9.01, 9.06]);
    let before = sketch(&h);
    let arcs = |s: &kmb_sketch::Sketch| {
        s.entities
            .iter()
            .filter(|e| matches!(e, Entity::Arc { .. }))
            .count()
    };
    assert_eq!(arcs(&before), 2);
    tool(&mut h, Tool::Select);
    // 上の辺を 1 本だけ選んで、パネルの「外へオフセット」
    click(&mut h, [3.8, 13.23]);
    assert_eq!(ed(&mut h).selection.len(), 1);
    h.get_by_label("外へオフセット").click();
    h.run();
    let after = sketch(&h);
    assert_eq!(arcs(&after), 4, "円弧も写る");
    // 写しの線と円弧も接する(1 つのオフセットの拘束の中の関係)
    assert!(
        after.constraints.iter().any(|c| matches!(
            &c.kind,
            K::Offset { parts } if parts.iter().any(|p| matches!(p, kmb_sketch::OffsetPart::Tangent { .. }))
        )),
        "写しの線と円弧も接する"
    );
}
