//! 構成表の欄(U55)の試験。パラメータの欄の下の表を、人の操作(クリックとキー)で通す。

use crate::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kmb_samples::reference_document;

fn harness(ws: Workspace) -> Harness<'static, Workspace> {
    Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws)
}

fn runs(h: &mut Harness<'_, Workspace>) {
    for _ in 0..3 {
        h.run();
    }
}

/// 名前が `label` の物を、巻物を送ってからクリックする。
fn click(h: &mut Harness<'_, Workspace>, label: &str) {
    h.get_by_label(label).scroll_to_me();
    runs(h);
    h.get_by_label(label).click();
    runs(h);
}

fn width(h: &Harness<'_, Workspace>) -> String {
    h.state()
        .document()
        .data()
        .param_by_name("width")
        .unwrap()
        .expr
        .clone()
}

#[test]
fn a_table_made_by_clicks_switches_the_part_and_one_undo_returns() {
    let mut ws = Workspace::new(reference_document(), None, kmb_bundle::modules());
    ws.win.show_params = true;
    ws.wait();
    let mut h = harness(ws);
    runs(&mut h);
    // 閉じた見出しを開き、幅を列に足して、構成を 2 つ足す
    click(&mut h, "構成表");
    click(&mut h, "列を追加 ▾");
    h.get_all_by_label("width").last().unwrap().click();
    runs(&mut h);
    click(&mut h, "構成を追加");
    click(&mut h, "構成を追加");
    let c = kmb_interchange::configuration::of(h.state().document().data());
    assert_eq!(c.columns.len(), 1);
    assert_eq!(
        c.rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        ["構成1", "構成2"]
    );
    // 構成2 の幅の欄に 80 mm と打って Enter
    let cell = h.get_by_label("構成2 の width");
    cell.click();
    runs(&mut h);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.event(egui::Event::Text("80 mm".into()));
    h.key_press(egui::Key::Enter);
    runs(&mut h);
    let c = kmb_interchange::configuration::of(h.state().document().data());
    assert_eq!(
        c.row("構成2").unwrap().cells[0],
        kmb_interchange::configuration::ConfigCell::Expr("80 mm".into())
    );
    // 当てるまで形は変えない
    assert_eq!(width(&h), "60 mm");
    click(&mut h, "構成「構成2」を当てる");
    h.state_mut().wait();
    runs(&mut h);
    assert_eq!(width(&h), "80 mm");
    assert_eq!(h.state().cur.work.report().error_count(), 0);
    assert_eq!(
        kmb_interchange::configuration::of(h.state().document().data())
            .active
            .as_deref(),
        Some("構成2")
    );
    // 当てた後に幅を直すと知らせ、「書き込む」で行に入れる
    let id = h
        .state()
        .document()
        .data()
        .param_by_name("width")
        .unwrap()
        .id;
    h.state_mut().set_param_expr(id, "85 mm");
    runs(&mut h);
    assert!(
        h.query_by_label_contains("と違います").is_some(),
        "直したことを知らせる"
    );
    click(&mut h, "「構成2」に書き込む");
    assert_eq!(
        kmb_interchange::configuration::of(h.state().document().data())
            .row("構成2")
            .unwrap()
            .cells[0],
        kmb_interchange::configuration::ConfigCell::Expr("85 mm".into())
    );
    assert!(h.query_by_label_contains("と違います").is_none());
    // 書き込み・幅を直す・当てるを 1 回ずつ取り消すと、当てる前に戻る
    for _ in 0..3 {
        h.state_mut().undo();
    }
    runs(&mut h);
    assert_eq!(width(&h), "60 mm");
    assert_eq!(
        kmb_interchange::configuration::of(h.state().document().data()).active,
        None
    );
}
