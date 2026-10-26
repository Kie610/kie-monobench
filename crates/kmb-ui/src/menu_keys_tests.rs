//! Esc で窓を閉じる・右クリックのメニューの試験(10-25 u5・u7・u8、片付けの残り#L20)。人の操作で通す。

use crate::select_ui::Hit;
use crate::*;
use egui::{Pos2, Rect};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

fn harness(ws: Workspace) -> Harness<'static, Workspace> {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .with_step_dt(0.05)
        .with_max_steps(40)
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h
}

#[test]
fn esc_closes_the_undo_history_and_the_parameters_one_step_at_a_time() {
    let mut ws = Workspace::new(
        kmb_samples::reference_document(),
        None,
        kmb_bundle::modules(),
    );
    ws.wait();
    let first = ws.document().data().features[0].id;
    ws.select(first);
    let mut h = harness(ws);
    // 取り消しの履歴の窓は Esc で閉じ、その Esc では選びを外さない
    h.state_mut().win.undo_history_open = true;
    h.run();
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(
        !h.state().undo_history_open(),
        "取り消しの履歴は Esc で閉じる"
    );
    assert_eq!(
        h.state().selection(),
        [first],
        "窓を閉じた Esc は選びを外さない"
    );
    // 編集パネルのパラメータも Esc で閉じる
    h.state_mut().win.show_params = true;
    h.run();
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(!h.state().win.show_params, "パラメータは Esc で閉じる");
    // 閉じる物が無くなれば、Esc は今までどおり選びを外す
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.state().selection().is_empty());
}

#[test]
fn a_second_right_click_moves_the_menu_instead_of_closing_it() {
    let mut ws = Workspace::new(
        kmb_samples::reference_document(),
        None,
        kmb_bundle::modules(),
    );
    ws.wait();
    let mut h = harness(ws);
    h.get_by_label("ボディ1").click_secondary();
    h.run();
    h.get_by_label("このボディのパラメータの表を作る");
    // 素早くもう一度右クリックしても、メニューは閉じない
    h.get_by_label("ボディ1").click_secondary();
    h.run();
    assert!(
        h.query_by_label("このボディのパラメータの表を作る")
            .is_some(),
        "2 回目の右クリックでメニューが消えた"
    );
    // Esc では今までどおり閉じる
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(
        h.query_by_label("このボディのパラメータの表を作る")
            .is_none()
    );
}

#[test]
fn right_clicking_a_picked_edge_keeps_all_picked_edges() {
    let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0));
    let part = kmb_samples::reference_part();
    let mut ws = Workspace::new(part.doc, None, kmb_bundle::modules());
    ws.cur.camera.fit(&ws.cur.scene, rect);
    let at = |ws: &Workspace, p: [f32; 3]| ws.cur.camera.project(p, rect).0;
    // 60×40×10 の板の上の面の手前の辺と右の辺を Shift で選ぶ
    let front = at(&ws, [30.0, 0.0, 10.0]);
    let right = at(&ws, [60.0, 20.0, 10.0]);
    for (p, add) in [(front, false), (right, true)] {
        let Some(Hit::Edge(e)) = ws.hit_at(rect, p, ws.win.pick_filter) else {
            panic!("辺の上は辺を拾う");
        };
        ws.pick_hit(Some(Hit::Edge(e)), add, false);
    }
    assert_eq!(ws.picked_edges().len(), 2);
    // 選んだ辺の上で右クリックしても、選びはそのまま(下の面を選びに入れない)
    ws.context_pick(rect, right);
    assert_eq!(ws.picked_edges().len(), 2, "辺の選びが減った");
    assert!(ws.picked().is_none(), "下の面が選びに入った");
}

/// 右クリックのメニューの行はアイコンの分だけ字の頭をそろえ、メニューの幅いっぱいに並ぶ。横に開く
/// 一覧も同じ行で出し、行を押せば今までどおり働く(10-19 M17)。
#[test]
fn right_menu_rows_line_up_and_open_their_lists() {
    let mut ws = Workspace::new(
        kmb_samples::reference_document(),
        None,
        kmb_bundle::modules(),
    );
    ws.wait();
    let mut h = harness(ws);
    h.get_by_label("ボディ1").click_secondary();
    h.run();
    let rect = |h: &Harness<'_, Workspace>, l: &str| h.get_by_label(l).rect();
    let short = rect(&h, "名前変更");
    let long = rect(&h, "このボディのパラメータの表を作る");
    assert_eq!(short.left(), long.left(), "行の頭がそろわない");
    assert_eq!(
        short.width(),
        long.width(),
        "行がメニューの幅いっぱいでない"
    );
    // 横に開く一覧(物理材料)は、行に載せると開く
    h.get_by_label("物理材料").hover();
    h.run();
    assert!(
        h.query_all_by_label_contains("g/cm³").next().is_some(),
        "物理材料の一覧が開かない"
    );
    // 行を押すと働き、メニューは閉じる
    h.get_by_label("名前変更").click();
    h.run();
    assert!(
        h.query_by_label("このボディのパラメータの表を作る")
            .is_none()
    );
}
