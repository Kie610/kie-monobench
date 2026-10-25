//! パラメータの表の試験(U50): 機能の値をその場で直す、ボディの右クリックで表を作る、
//! 行の右クリックで表に入れる・外す、タブで表を替える。人の操作(クリックと打鍵)で通す。

use crate::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kmb_core::ParamKey;

/// 縦に長い画面(パラメータの窓の下の機能の値まで、巻かずに見える)。
fn harness(ws: Workspace) -> Harness<'static, Workspace> {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 1700.0))
        .with_step_dt(0.05)
        .with_max_steps(40)
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h
}

/// 押し出しの距離(式が `expr` の機能の値)の機能。
fn extrude_with(ws: &Workspace, expr: &str) -> FeatureId {
    ws.document()
        .data()
        .features
        .iter()
        .find(|f| f.type_id == "solid.extrude@1" && f.data["distance"] == expr)
        .unwrap_or_else(|| panic!("距離 {expr} の押し出しが無い"))
        .id
}

#[test]
fn feature_values_are_edited_and_body_tables_are_made_in_the_parameter_window() {
    // 1 つの画面で順に確かめる:
    // - 機能の値を表でその場で直し、取り消し 1 回で戻る(元 a_feature_value_is_edited_in_the_parameter_table)
    // - ボディの右クリックで表を作り、行を表に入れる・外す、表の名前を変える・消す
    //   (元 a_body_table_is_made_from_the_body_menu_and_rows_are_put_in_and_out)
    let mut ws = Workspace::new(kmb_samples::reference_document(), None, kmb_bundle::modules());
    ws.wait();
    let pocket = extrude_with(&ws, "pocket_depth");
    let plate = extrude_with(&ws, "thickness");
    let mut h = harness(ws);
    // リボンの「パラメータ」で窓を開くと、機能の値の欄に押し出しの距離が出る
    h.query_all_by_label("パラメータ").next().unwrap().click();
    h.run();
    h.get_by_label("フィーチャの値");
    // 名前の欄(パラメータ pocket_depth)の後の、機能の値の欄
    let field = h
        .query_all_by_value("pocket_depth")
        .rfind(|n| n.rect().left() > 900.0)
        .expect("ポケットの距離の欄");
    // 欄をクリックし、全部を選んで打ち直し、Enter で決める
    field.click();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.event(egui::Event::Text("3".into()));
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    h.state_mut().wait();
    // 数だけなら今の単位を補い、取り消し 1 回で戻る
    let d = &h.state().document().data().feature(pocket).unwrap().data;
    assert_eq!(d["distance"], "3 mm");
    h.state_mut().undo();
    let d = &h.state().document().data().feature(pocket).unwrap().data;
    assert_eq!(d["distance"], "pocket_depth");

    // パラメータの窓を閉じてから、ボディの表を作る
    h.state_mut().win.show_params = false;
    h.run();
    // ボディの右クリック →「このボディのパラメータの表を作る」
    h.get_by_label("ボディ1").click_secondary();
    h.run();
    h.get_by_label("このボディのパラメータの表を作る").click();
    h.run();
    let views = h.state().document().data().param_views.clone();
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].name, "ボディ1 のパラメータ");
    let body_view = views[0].id;
    assert_eq!(h.state().cur.param_tab, body_view, "作った表を開く");
    assert!(h.state().win.show_params, "パラメータの窓を開く");
    h.get_by_label("ボディ1に関わる物(行の右クリックで表に入れる・外す)");
    // 表には板の厚みの押し出しの距離が出る
    let shown = h.state_mut().param_view_rows().unwrap();
    assert!(shown.contains(&ParamKey::Field {
        feature: plate,
        key: "distance".into()
    }));
    // 「全部」のタブに戻り、パラメータ hole の名前を右クリック →「表に入れる」→「新しい表…」
    h.get_by_label("全部").click();
    h.run();
    assert_eq!(h.state().cur.param_tab, 0);
    let hole = h
        .state()
        .document()
        .data()
        .param_by_name("hole")
        .unwrap()
        .id;
    h.query_all_by_value("hole")
        .find(|n| n.rect().left() > 900.0)
        .expect("hole の名前の欄")
        .click_secondary();
    h.run();
    h.get_by_label_contains("表に入れる").hover();
    h.run();
    h.get_by_label("新しい表…").click();
    h.run();
    let views = h.state().document().data().param_views.clone();
    assert_eq!(views.len(), 2);
    assert_eq!(views[1].name, "表1");
    assert_eq!(views[1].members, vec![ParamKey::User { id: hole }]);
    assert_eq!(h.state().cur.param_tab, views[1].id);
    // 作った表で行を右クリック →「この表から外す」で空になる(パラメータは残る)
    h.query_all_by_value("hole")
        .find(|n| n.rect().left() > 900.0)
        .unwrap()
        .click_secondary();
    h.run();
    h.get_by_label("この表から外す").click();
    h.run();
    assert!(h.state_mut().param_view_rows().unwrap().is_empty());
    assert!(h.state().document().data().param_by_name("hole").is_some());
    h.get_by_label_contains("この表にはまだ行がありません");
    // タブの右クリック →「名前変更」で、打った名前を Enter で決める
    h.get_by_label("表1").click_secondary();
    h.run();
    h.get_by_label("名前変更").click();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.event(egui::Event::Text("穴の表".into()));
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    assert_eq!(h.state().document().data().param_views[1].name, "穴の表");
    assert!(h.query_by_label("OK").is_none());
    // 欄の外を押しても決まって抜ける(毎こま欄へ注目を戻すと抜けられなかった。台帳の X3)
    h.get_by_label("穴の表").click_secondary();
    h.run();
    h.get_by_label("名前変更").click();
    h.run();
    h.key_press(egui::Key::End);
    h.event(egui::Event::Text("2".into()));
    h.run();
    h.get_by_label_contains("この表にはまだ行がありません")
        .click();
    h.run();
    h.run();
    assert_eq!(h.state().document().data().param_views[1].name, "穴の表2");
    assert!(h.query_by_label("OK").is_none(), "欄から抜ける");
    // キャンセルを押すと、打った名前は入れない
    h.get_by_label("穴の表2").click_secondary();
    h.run();
    h.get_by_label("名前変更").click();
    h.run();
    h.event(egui::Event::Text("x".into()));
    h.run();
    h.get_by_label("キャンセル").click();
    h.run();
    assert_eq!(h.state().document().data().param_views[1].name, "穴の表2");
    assert!(h.query_by_label("OK").is_none());
    // タブの右クリックで表を消すと「全部」に戻り、取り消すと表が戻る
    h.get_by_label("穴の表2").click_secondary();
    h.run();
    h.get_by_label("表を削除").click();
    h.run();
    assert_eq!(h.state().document().data().param_views.len(), 1);
    assert_eq!(h.state().cur.param_tab, 0);
    h.state_mut().undo();
    assert_eq!(h.state().document().data().param_views.len(), 2);
}
