//! 画面の小さな直し(面だけ選んだ Delete、ブラウザの長い名前、パラメータの式の Enter、断面の開き直し)を、
//! 人の操作の順で確かめる。

use crate::Workspace;
use crate::select_ui::Hit;
use egui::{Pos2, Rect};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

fn rect() -> Rect {
    Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0))
}

fn harness(ws: Workspace) -> Harness<'static, Workspace> {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h
}

#[test]
fn delete_with_only_a_face_picked_says_how_to_delete() {
    // 面だけを選んで Delete を押すと、黙って何もしないのではなく消し方を出す
    let part = kmb_samples::reference_part();
    let mut ws = Workspace::new(part.doc, None, kmb_bundle::modules());
    ws.cur.camera.fit(&ws.cur.scene, rect());
    // 開いた直後は最後の機能を選んでいるので外す(機能を選んでいれば Delete はそれを消す)
    ws.clear_selection();
    let face = ws.cur.camera.project([50.0, 35.0, 10.0], rect()).0;
    let hit = ws.hit_at(rect(), face, ws.win.pick_filter);
    assert!(matches!(hit, Some(Hit::Face(_))), "{hit:?}");
    ws.pick_hit(hit, false, false);
    let mut h = harness(ws);
    let before = h.state().document().data().features.len();
    h.key_press(egui::Key::Delete);
    h.run();
    let ws = h.state();
    assert_eq!(ws.document().data().features.len(), before, "何も消さない");
    assert!(
        ws.message().is_some_and(|m| m.contains("消せません")),
        "{:?}",
        ws.message()
    );
}

#[test]
fn a_long_name_cut_short_in_the_browser_shows_whole_in_a_tooltip() {
    // ブラウザの行で … で切った名前は、ツールチップで全部を見せる
    let part = kmb_samples::reference_part();
    let mut doc = part.doc;
    let long = "とても長い名前の押し出しで、ブラウザの幅には収まらないので途中で切れる機能の名前";
    doc.apply(kmb_core::Transaction::new(
        "名前",
        vec![kmb_core::Op::RenameFeature {
            id: part.plate_sketch,
            name: long.into(),
        }],
    ))
    .unwrap();
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 700.0))
        .build_ui_state(
            |ui, ws: &mut Workspace| ws.ui(ui),
            Workspace::new(doc, None, kmb_bundle::modules()),
        );
    // ツールチップを待たずに出す
    h.ctx.all_styles_mut(|s| s.interaction.tooltip_delay = 0.0);
    h.run();
    // 名前の行はタイムラインとブラウザの 2 つ。タイムラインのツールチップは計算の時間も出すので、
    // 名前だけのツールチップ(ブラウザの行)が出るかを見る
    let rows = h.query_all_by_label(long).count();
    assert_eq!(rows, 2, "タイムラインとブラウザの行");
    let mut name_only = 0;
    for i in 0..rows {
        h.query_all_by_label(long).nth(i).unwrap().hover();
        for _ in 0..3 {
            h.run();
        }
        name_only += h
            .query_all_by_label_contains("とても長い")
            .filter(|n| {
                let d = format!("{n:?}");
                d.contains("role: Label") && d.contains(&format!("value: \"{long}\""))
            })
            .count();
    }
    assert_eq!(
        name_only, 1,
        "ブラウザの行は切った名前の全部をツールチップに出す"
    );
}

#[test]
fn enter_in_the_new_parameter_expression_adds_it() {
    // 新しいパラメータは、式の欄で Enter を押しても足せる
    // (「追加」ボタンだけでなく)
    let mut ws = Workspace::new(
        kmb_samples::reference_document(),
        None,
        kmb_bundle::modules(),
    );
    ws.win.show_params = true;
    ws.wait();
    let mut h = harness(ws);
    h.state_mut().cur.new_param.0 = "幅2".into();
    let field = crate::doc_id(&h.ctx, "param_new_expr");
    h.ctx.memory_mut(|m| m.request_focus(field));
    h.run();
    h.event(egui::Event::Text("12".into()));
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    let p = h.state().document().data().param_by_name("幅2").cloned();
    assert!(p.is_some_and(|p| p.expr.starts_with("12")), "Enter で足す");
}

#[test]
fn a_section_reopens_where_it_was_left() {
    // 断面を閉じて開き直すと、前に置いた平面・位置・向きから始まる
    // (毎回 ZX 平面の真ん中に戻すと、置き直しが要る)
    let part = kmb_samples::reference_part();
    let mut ws = Workspace::new(part.doc, None, kmb_bundle::modules());
    ws.wait();
    ws.toggle_section();
    let mut s = ws.cur.section.clone().unwrap();
    s.plane = crate::section_ui::SectionPlane::Origin(0);
    s.offset = 3.5;
    s.flip = true;
    ws.cur.section = Some(s.clone());
    ws.toggle_section();
    assert!(!ws.is_sectioning());
    ws.toggle_section();
    assert!(ws.is_sectioning());
    assert_eq!(ws.cur.section.as_ref(), Some(&s), "閉じた時のまま");
}

#[test]
fn the_shape_follows_a_value_while_it_is_still_being_typed() {
    // 10-22 k10: 欄に数を打って手を止めると、Enter や欄の外を押す前に形が付いて来る
    // (決めるのは前と同じく Enter か欄の外。打っている欄からキーボードは外れない)
    let part = kmb_samples::reference_part();
    let boss = part.boss;
    let mut ws = Workspace::new(part.doc, None, kmb_bundle::modules());
    ws.wait();
    ws.open_edit(boss);
    ws.cur.focus_new.set(Some(boss));
    let mut h = harness(ws);
    let before = h
        .state()
        .document()
        .data()
        .feature(boss)
        .unwrap()
        .data
        .clone();
    // 最初の式の欄は全体を選んであるので、打った数で置き換わる
    h.event(egui::Event::Text("7".into()));
    for _ in 0..4 {
        h.step();
    }
    h.run();
    let ws = h.state_mut();
    ws.wait();
    let data = ws.document().data().feature(boss).unwrap().data.clone();
    assert_ne!(data, before, "打っている途中の値で形が付いて来る");
    assert!(data.to_string().contains("\"7 mm\""), "{data}");
    assert!(ws.is_editing(boss), "直している途中のまま");
    assert!(
        h.ctx.memory(|m| m.focused().is_some()),
        "打っている欄から外れない"
    );
}
