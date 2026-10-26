//! コピー・切り取り・貼り付けを、人の操作(囲んで選ぶ、キー、右クリックのメニュー)で確かめる。

use crate::Tool;
use crate::tests::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kmb_sketch::{ConstraintKind, Entity, EntityId, Sketch};
use kmb_ui::Workspace;

fn send(h: &mut Harness<'static, Workspace>, e: egui::Event) {
    h.event(e);
    h.run();
}

/// 写した時に OS のクリップボードへ置いた文(egui_kittest は何フレームか回すので、出力の
/// 命令ではなく、画面の中に持った写しから読む)。
fn copied_text(h: &Harness<'static, Workspace>) -> Option<String> {
    crate::clipboard::held(&h.ctx).map(|c| c.text)
}

fn lines(s: &Sketch) -> Vec<EntityId> {
    s.entities
        .iter()
        .filter_map(|e| match e {
            Entity::Line { id, .. } => Some(*id),
            _ => None,
        })
        .collect()
}

/// 線の点を囲む箱の中心。
fn center_of(s: &Sketch, ids: &[EntityId]) -> [f64; 2] {
    let sol = kmb_sketch::solve(s, &Default::default()).unwrap();
    let pts: Vec<[f64; 2]> = ids
        .iter()
        .filter_map(|id| match s.entity(*id) {
            Some(Entity::Line { a, b, .. }) => Some([sol.points[a], sol.points[b]]),
            _ => None,
        })
        .flatten()
        .collect();
    let f = |i: usize, g: fn(f64, f64) -> f64, s: f64| pts.iter().map(|p| p[i]).fold(s, g);
    [
        (f(0, f64::min, f64::MAX) + f(0, f64::max, f64::MIN)) / 2.0,
        (f(1, f64::min, f64::MAX) + f(1, f64::max, f64::MIN)) / 2.0,
    ]
}

/// 長方形を描き、右へ全体を囲んで選んだ作業場。
fn selected_rectangle() -> Harness<'static, Workspace> {
    let mut h = sketch_with_rectangle();
    drag_on(&mut h, [0.15, 17.4], [12.66, 7.5], egui::Modifiers::NONE);
    assert_eq!(editor(h.state_mut()).editor.selection.len(), 8);
    h
}

#[test]
fn copy_and_paste_puts_a_copy_in_place_and_starts_moving_it() {
    let mut h = selected_rectangle();
    let before = current_sketch(h.state());
    let old = lines(&before);
    assert_eq!(old.len(), 4);
    // Ctrl+C: OS のクリップボードに文を置く(次の Ctrl+V が届くように)
    send(&mut h, egui::Event::Copy);
    let text = copied_text(&h).expect("クリップボードに文を置く");
    assert!(text.contains("線分 4"), "{text}");
    assert_eq!(
        current_sketch(h.state()),
        before,
        "コピーでは形は変わらない"
    );
    // Ctrl+V: 元と同じ位置に重ねて置き、貼ったものを選んで移動の道具を始める
    move_to(&mut h, [27.26, 11.15]);
    h.run();
    send(&mut h, egui::Event::Paste(text));
    let after = current_sketch(h.state());
    let all = lines(&after);
    assert_eq!(all.len(), 8);
    let new: Vec<EntityId> = all.into_iter().filter(|l| !old.contains(l)).collect();
    // 拘束(水平・垂直)も写る
    let hv = |s: &Sketch| {
        s.constraints
            .iter()
            .filter(|c| {
                matches!(
                    c.kind,
                    ConstraintKind::Horizontal { .. } | ConstraintKind::Vertical { .. }
                )
            })
            .count()
    };
    assert_eq!(hv(&after), 2 * hv(&before));
    let (c0, c1) = (center_of(&after, &old), center_of(&after, &new));
    assert!(
        (c1[0] - c0[0]).abs() < 1e-6 && (c1[1] - c0[1]).abs() < 1e-6,
        "同じ位置に重なる: {c0:?} {c1:?}"
    );
    let e = &editor(h.state_mut()).editor;
    assert_eq!(e.tool, Tool::Move);
    assert!(e.pending.is_some(), "基準は写した形の中心");
    assert_eq!(e.selection.len(), 8, "貼った線 4 本と角 4 つを選ぶ");
    assert!(new.iter().all(|l| e.selection.contains(l)));
    // クリックした所へ動く: 写しの中心が、元の長方形の中心(画面の 600, 375)から (1000, 400) へ
    click_at(&mut h, [27.26, 11.15]);
    let moved = current_sketch(h.state());
    let (c0, c1) = (center_of(&moved, &old), center_of(&moved, &new));
    let (dx, dy) = (c1[0] - c0[0], c1[1] - c0[1]);
    assert!(dx > 0.0 && dy < 0.0, "{dx} {dy}");
    assert!((dx / dy + 400.0 / 25.0).abs() < 0.5, "{dx} {dy}");
    // 取り消し 1 回で動かす前(重なった所)、もう 1 回で貼る前に戻る
    h.get_by_label("元に戻す").click();
    h.run();
    assert_eq!(lines(&current_sketch(h.state())).len(), 8);
    h.get_by_label("元に戻す").click();
    h.run();
    assert_eq!(lines(&current_sketch(h.state())).len(), 4);
}

#[test]
fn esc_after_paste_leaves_the_copy_in_place() {
    let mut h = selected_rectangle();
    send(&mut h, egui::Event::Copy);
    let text = copied_text(&h).unwrap();
    send(&mut h, egui::Event::Paste(text));
    press_key(&mut h, egui::Key::Escape);
    press_key(&mut h, egui::Key::Escape);
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::Select);
    assert_eq!(lines(&current_sketch(h.state())).len(), 8);
}

#[test]
fn cut_removes_the_selection_and_paste_brings_it_back() {
    let mut h = selected_rectangle();
    send(&mut h, egui::Event::Cut);
    let text = copied_text(&h).expect("クリップボードに文を置く");
    let s = current_sketch(h.state());
    assert!(lines(&s).is_empty(), "切り取った線は消える");
    assert!(editor(h.state_mut()).editor.selection.is_empty());
    move_to(&mut h, [6.4, 12.45]);
    h.run();
    send(&mut h, egui::Event::Paste(text));
    let s = current_sketch(h.state());
    assert_eq!(lines(&s).len(), 4);
    assert_eq!(s.constraints.len(), 4, "水平 2・垂直 2");
}

#[test]
fn other_text_on_the_clipboard_does_not_paste_the_old_copy() {
    let mut h = selected_rectangle();
    send(&mut h, egui::Event::Copy);
    move_to(&mut h, [27.26, 11.15]);
    h.run();
    // ほかの所で文をコピーした後の Ctrl+V では貼らない
    send(&mut h, egui::Event::Paste("width".into()));
    assert_eq!(lines(&current_sketch(h.state())).len(), 4);
}

#[test]
fn the_right_click_menu_copies_and_pastes_where_it_was_opened() {
    let mut h = selected_rectangle();
    let old = lines(&current_sketch(h.state()));
    // 選んだ線の上で右クリック → コピー
    right_click_at(&mut h, [6.4, 16.36]);
    crate::tests::menu_item(&h, "コピー").click();
    h.run();
    assert!(copied_text(&h).is_some());
    // 空いた所で右クリック → 貼り付け: 同じ位置に置き、移動の道具でクリックした所へ動かす
    right_click_at(&mut h, [27.26, 0.72]);
    crate::tests::menu_item(&h, "貼り付け").click();
    h.run();
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::Move);
    click_at(&mut h, [27.26, 0.72]);
    let s = current_sketch(h.state());
    let new: Vec<EntityId> = lines(&s).into_iter().filter(|l| !old.contains(l)).collect();
    assert_eq!(new.len(), 4);
    let (c0, c1) = (center_of(&s, &old), center_of(&s, &new));
    let (dx, dy) = (c1[0] - c0[0], c1[1] - c0[1]);
    // 画面で (600, 375) から (1000, 600) へ。y は上向き
    assert!((dx / dy + 400.0 / 225.0).abs() < 0.05, "{dx} {dy}");
    // 何も選んでいなければ、切り取りとコピーは出さない
    press_key(&mut h, egui::Key::Escape);
    assert!(editor(h.state_mut()).editor.selection.is_empty());
    right_click_at(&mut h, [-1.42, -3.45]);
    assert!(h.query_by_label("コピー").is_none());
    assert!(h.query_by_label("貼り付け").is_some());
}
