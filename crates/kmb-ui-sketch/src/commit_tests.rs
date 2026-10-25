//! 値の欄の決め方とドラッグを、人の操作(クリック、キー、引く)で確かめる(Kie のレビュー K1・K6)。
//! 寸法の値の欄は Enter に加えて、欄の外を左クリックしても決まる。右・中ボタンで押しても欄は
//! 残る(Esc でやめるのは tests.rs で確かめる)。長穴の片方の中心を引いても、もう片方の中心は動かない。

use crate::Tool;
use crate::tests::*;
use egui_kittest::Harness;
use kmb_sketch::ConstraintKind;
use kmb_ui::Workspace;

/// 図の空いた所(長方形から離れた所)。
const EMPTY: [f64; 2] = [-5.07, -0.32];

/// 長さの寸法の値(なければ None)。
fn length_value(h: &Harness<'static, Workspace>) -> Option<String> {
    current_sketch(h.state())
        .constraints
        .iter()
        .find(|c| matches!(c.kind, ConstraintKind::Length { .. }))
        .and_then(|c| c.kind.value().map(str::to_string))
}

/// 寸法の道具で上の辺を選び、上へ札を置いて値の欄を出したところ。
fn placing_top_length(h: &mut Harness<'static, Workspace>) {
    press_key(h, egui::Key::D);
    click_at(h, [6.4, 16.36]);
    move_to(h, [6.4, 19.49]);
    h.run();
    quick_click(h, [6.4, 19.49]);
    assert!(
        editor(h.state_mut()).editor.placing.is_some(),
        "値の欄が出る"
    );
}

#[test]
fn clicking_outside_the_value_field_places_the_dimension() {
    let mut h = sketch_with_rectangle();
    placing_top_length(&mut h);
    type_text(&mut h, "30");
    // Enter の代わりに、図の空いた所を左クリック
    quick_click(&mut h, EMPTY);
    h.run();
    assert_eq!(length_value(&h).as_deref(), Some("30 mm"));
    assert!(editor(h.state_mut()).editor.placing.is_none());
    // 寸法の道具は続く
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::Dimension);
    assert_eq!(h.state().report().error_count(), 0);
}

#[test]
fn a_middle_button_press_keeps_the_value_field_open() {
    let mut h = sketch_with_rectangle();
    placing_top_length(&mut h);
    type_text(&mut h, "3");
    // 中ボタンで押して離す(ビューを動かす時の操作): 欄は残り、打ち続けられる
    let pos = screen(&h, EMPTY);
    h.hover_at(pos);
    h.step();
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Middle,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.step();
    }
    h.run();
    assert!(editor(h.state_mut()).editor.placing.is_some(), "欄が残る");
    assert_eq!(length_value(&h), None);
    type_text(&mut h, "5");
    press_enter(&mut h);
    assert_eq!(length_value(&h).as_deref(), Some("35 mm"));
}

#[test]
fn a_value_typed_in_the_panel_is_placed_where_the_view_is_clicked() {
    let mut h = sketch_with_rectangle();
    press_key(&mut h, egui::Key::D);
    click_at(&mut h, [6.4, 16.36]);
    move_to(&mut h, [6.4, 20.01]);
    h.run();
    // 数字から打つと、パネルの「寸法の値」に入る
    type_text(&mut h, "45");
    assert_eq!(editor(h.state_mut()).editor.dim_text, "45");
    // 図を左クリック: 押した所に、打った値で寸法が付く(欄は出さない)
    quick_click(&mut h, [6.4, 20.01]);
    h.run();
    assert_eq!(length_value(&h).as_deref(), Some("45 mm"));
    assert!(editor(h.state_mut()).editor.placing.is_none());
    let s = current_sketch(h.state());
    let c = s
        .constraints
        .iter()
        .find(|c| matches!(c.kind, ConstraintKind::Length { .. }))
        .unwrap();
    let sol = solved(h.state());
    let at = s.label_at(c, &sol).expect("札の位置");
    let tf = editor(h.state_mut()).view.last.clone().unwrap();
    let screen = tf.to_screen(at);
    assert!((screen.y - 230.0).abs() < 3.0, "押した高さに札: {screen:?}");
}

#[test]
fn a_value_typed_in_the_panel_is_dropped_when_the_click_lands_outside_the_view() {
    // パネルに打った後、図でもパネルでもない所(下の帯など)を押してから図を押しても、
    // 前に打った値で寸法を付けない。図の外を押したら、打った値で付ける約束は捨てる
    let mut h = sketch_with_rectangle();
    press_key(&mut h, egui::Key::D);
    click_at(&mut h, [6.4, 16.36]);
    move_to(&mut h, [6.4, 20.01]);
    h.run();
    type_text(&mut h, "45");
    assert_eq!(editor(h.state_mut()).editor.dim_text, "45");
    // 下の帯(図とパネルの外)を押す
    let size = h.ctx.content_rect().size();
    quick_click_px(&mut h, egui::pos2(size.x * 0.5, size.y - 6.0));
    h.run();
    assert!(!editor(h.state_mut()).editor.dim_typed_click);
    // 図を押すと、いつもどおりその場に値の欄を出す(打った値ですぐには付けない)
    quick_click(&mut h, [6.4, 20.01]);
    h.run();
    assert_eq!(length_value(&h), None, "前に打った値ですぐ付けない");
    assert!(
        editor(h.state_mut()).editor.placing.is_some(),
        "値の欄が出る"
    );
}

#[test]
fn editing_a_dimension_is_kept_by_clicking_outside() {
    let mut h = sketch_with_rectangle();
    placing_top_length(&mut h);
    type_text(&mut h, "30");
    press_enter(&mut h);
    press_key(&mut h, egui::Key::Escape);
    press_key(&mut h, egui::Key::Escape);
    h.run();
    let id = current_sketch(h.state())
        .constraints
        .iter()
        .find(|c| matches!(c.kind, ConstraintKind::Length { .. }))
        .unwrap()
        .id;
    let at = editor(h.state_mut())
        .view
        .labels
        .iter()
        .find(|(c, _)| *c == id)
        .map(|(_, r)| r.center())
        .expect("札");
    h.hover_at(at);
    h.run();
    for _ in 0..2 {
        for pressed in [true, false] {
            h.input_mut().events.push(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
    }
    h.step();
    h.run();
    assert!(
        editor(h.state_mut()).editor.edit_dim.is_some(),
        "ダブルクリックで欄が出る"
    );
    type_text(&mut h, "25");
    quick_click(&mut h, EMPTY);
    h.run();
    assert_eq!(length_value(&h).as_deref(), Some("25 mm"));
    assert!(editor(h.state_mut()).editor.edit_dim.is_none());
}

#[test]
fn dragging_one_slot_center_with_the_mouse_leaves_the_other_center() {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Slot);
    h.run();
    click_at(&mut h, [-1.42, 11.15]);
    click_at(&mut h, [9.01, 11.15]);
    click_at(&mut h, [9.01, 9.06]);
    press_key(&mut h, egui::Key::Escape);
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::Select);
    let s = current_sketch(h.state());
    let centers: Vec<kmb_sketch::EntityId> = s
        .entities
        .iter()
        .filter_map(|e| match e {
            kmb_sketch::Entity::Arc { center, .. } => Some(*center),
            _ => None,
        })
        .collect();
    assert_eq!(centers.len(), 2);
    let before = solved(h.state());
    let tf = editor(h.state_mut()).view.last.clone().unwrap();
    // 右の中心を掴んで、右上へ引く
    let (right, left) = if before.points[&centers[0]][0] > before.points[&centers[1]][0] {
        (centers[0], centers[1])
    } else {
        (centers[1], centers[0])
    };
    let at = tf.to_screen(before.points[&right]);
    drag_px(
        &mut h,
        at,
        at + egui::vec2(40.0, -30.0),
        egui::Modifiers::NONE,
    );
    let after = solved(h.state());
    let (p, q) = (before.points[&left], after.points[&left]);
    assert!(
        (p[0] - q[0]).abs() < 1e-9 && (p[1] - q[1]).abs() < 1e-9,
        "掴んでいない中心が動いた: {p:?} → {q:?}"
    );
    let moved = after.points[&right];
    let was = before.points[&right];
    assert!(
        (moved[0] - was[0]).hypot(moved[1] - was[1]) > 1.0,
        "掴んだ中心が動く: {was:?} → {moved:?}"
    );
    assert_eq!(h.state().report().error_count(), 0);
}
