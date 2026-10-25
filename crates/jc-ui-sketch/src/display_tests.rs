//! 表示の切り替え・刻みへの吸い付き・選べる物の絞り込み・選び方(キー 1・2・3)・中心線を、
//! 人の操作(クリック、キー、引く)で確かめる。

use crate::Tool;
use crate::tests::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use jc_sketch::{Entity, EntityId};
use jc_ui::Workspace;

/// 何点かを通って引く(自由形状・なぞるの道)。
fn drag_through(h: &mut Harness<'static, Workspace>, path: &[[f64; 2]]) {
    let path: Vec<egui::Pos2> = path.iter().map(|p| screen(h, *p)).collect();
    let a = path[0];
    h.hover_at(a);
    h.run();
    h.event(egui::Event::PointerButton {
        pos: a,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
    for w in path.windows(2) {
        let (p, q) = (w[0], w[1]);
        for k in 1..=6 {
            h.event(egui::Event::PointerMoved(p + (q - p) * (k as f32 / 6.0)));
            h.run();
        }
    }
    let b = *path.last().unwrap();
    h.event(egui::Event::PointerButton {
        pos: b,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
}

/// 押して離すまでを 1 コマずつのクリック(run は何コマも回して長押しになることがある)。
fn tap(h: &mut Harness<'static, Workspace>, p: [f64; 2]) {
    let pos = screen(h, p);
    h.hover_at(pos);
    h.run();
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.step();
    }
    h.run();
}

fn lines_of(h: &Harness<'static, Workspace>) -> Vec<EntityId> {
    current_sketch(h.state())
        .entities
        .iter()
        .filter(|e| matches!(e, Entity::Line { .. }))
        .map(|e| e.id())
        .collect()
}

fn selection(h: &mut Harness<'static, Workspace>) -> Vec<EntityId> {
    editor(h.state_mut())
        .editor
        .selection
        .iter()
        .copied()
        .collect()
}

#[test]
fn the_display_panel_and_the_old_marks_checkbox_are_shown() {
    let mut h = sketch_with_rectangle();
    h.run();
    // 「スケッチの表示」はパネルの下に閉じてある。開くと切り替えが出る
    assert!(h.query_by_label("領域の塗り").is_none());
    // パレットの下の方にあるので、見える所まで送ってから開く
    h.get_by_label("スケッチの表示").scroll_to_me();
    h.run();
    h.get_by_label("スケッチの表示").click();
    h.run();
    for label in [
        "領域の塗り",
        "点",
        "寸法",
        "投影",
        "補助線",
        "グリッド",
        "スナップ",
    ] {
        assert!(
            h.query_all_by_label(label).next().is_some(),
            "{label} が出る"
        );
    }
    // パネルが長くなり下の方は隠れるので、人がするように転がして出す
    h.get_all_by_label("領域の塗り")
        .next()
        .unwrap()
        .scroll_to_me();
    h.run();
    h.get_all_by_label("領域の塗り").next().unwrap().click();
    h.run();
    assert!(!editor(h.state_mut()).editor.display.profiles);
    // 拘束の印の切り替えは前のとおり効く
    h.get_by_label("拘束を表示").click();
    h.run();
    assert!(!editor(h.state_mut()).editor.show_marks);
}

#[test]
fn grid_snapping_puts_drawn_points_on_the_chosen_step() {
    let mut h = empty_sketch();
    {
        let e = &mut editor(h.state_mut()).editor;
        e.display.snap_grid = true;
        e.display.snap_step = 5.0;
        e.set_tool(Tool::Line);
    }
    h.run();
    click_at(&mut h, [1.35, 16.52]);
    click_at(&mut h, [9.95, 12.66]);
    press_key(&mut h, egui::Key::Escape);
    let s = current_sketch(h.state());
    let pts: Vec<[f64; 2]> = s
        .entities
        .iter()
        .filter_map(|e| match e {
            Entity::Point { x, y, .. } => Some([*x, *y]),
            _ => None,
        })
        .collect();
    assert_eq!(pts.len(), 2, "線の両端");
    for p in pts {
        for v in p {
            assert!(
                ((v / 5.0) - (v / 5.0).round()).abs() < 1e-9,
                "刻み 5 mm に乗る: {p:?}"
            );
        }
    }
}

#[test]
fn turning_points_off_in_the_filter_picks_the_line_at_a_corner() {
    let mut h = sketch_with_rectangle();
    // 角をクリックすると、ふだんは点を選ぶ
    tap(&mut h, [1.19, 16.36]);
    let s = current_sketch(h.state());
    let sel = selection(&mut h);
    assert!(matches!(s.entity(sel[0]), Some(Entity::Point { .. })));
    press_key(&mut h, egui::Key::Escape);
    editor(h.state_mut()).editor.filter.points = false;
    tap(&mut h, [1.19, 16.36]);
    let sel = selection(&mut h);
    assert_eq!(sel.len(), 1);
    assert!(
        matches!(s.entity(sel[0]), Some(Entity::Line { .. })),
        "点を選べない時は線"
    );
}

#[test]
fn key_2_lassos_and_key_3_paints() {
    let mut h = sketch_with_rectangle();
    // 2: 自由形状で長方形の上の辺だけを囲む
    press_key(&mut h, egui::Key::Num2);
    assert_eq!(
        editor(h.state_mut()).editor.select_mode,
        crate::display::SelectMode::Lasso
    );
    drag_through(
        &mut h,
        &[
            [0.15, 17.4],
            [12.66, 17.4],
            [12.66, 15.32],
            [0.15, 15.32],
            [0.2, 17.3],
        ],
    );
    let s = current_sketch(h.state());
    let sel = selection(&mut h);
    let picked_lines: Vec<_> = sel
        .iter()
        .filter(|id| matches!(s.entity(**id), Some(Entity::Line { .. })))
        .collect();
    assert_eq!(picked_lines.len(), 1, "上の辺だけ: {sel:?}");
    // 3: なぞって左右の辺を選ぶ(横切る)
    press_key(&mut h, egui::Key::Escape);
    press_key(&mut h, egui::Key::Num3);
    drag_through(&mut h, &[[-0.38, 12.19], [6.4, 12.45], [13.18, 12.19]]);
    let sel = selection(&mut h);
    let picked_lines: Vec<_> = sel
        .iter()
        .filter(|id| matches!(s.entity(**id), Some(Entity::Line { .. })))
        .collect();
    assert_eq!(picked_lines.len(), 2, "左右の辺: {sel:?}");
    // 1 で窓に戻る
    press_key(&mut h, egui::Key::Num1);
    assert_eq!(
        editor(h.state_mut()).editor.select_mode,
        crate::display::SelectMode::Window
    );
}

#[test]
fn the_menu_turns_a_line_into_a_centerline_and_back() {
    let mut h = sketch_with_rectangle();
    let before = lines_of(&h);
    right_click_at(&mut h, [4.32, 16.36]);
    h.get_by_label("標準/中心線").click();
    h.run();
    let s = current_sketch(h.state());
    assert_eq!(s.centerlines.len(), 1);
    let line = s.centerlines[0];
    assert!(before.contains(&line));
    assert!(s.entity(line).unwrap().is_construction(), "中心線は補助線");
    // もう一度で戻る
    right_click_at(&mut h, [4.32, 16.36]);
    h.get_by_label("標準/中心線").click();
    h.run();
    let s = current_sketch(h.state());
    assert!(s.centerlines.is_empty());
    assert!(!s.entity(line).unwrap().is_construction());
}

#[test]
fn hidden_construction_lines_can_not_be_picked() {
    let mut h = sketch_with_rectangle();
    tap(&mut h, [4.32, 16.36]);
    press_key(&mut h, egui::Key::X);
    press_key(&mut h, egui::Key::Escape);
    let s = current_sketch(h.state());
    assert!(s.entities.iter().any(|e| e.is_construction()));
    editor(h.state_mut()).editor.display.construction = false;
    tap(&mut h, [4.32, 16.36]);
    let sel = selection(&mut h);
    assert!(
        sel.iter()
            .all(|id| !s.entity(*id).unwrap().is_construction()),
        "出していない補助線は選べない: {sel:?}"
    );
}
