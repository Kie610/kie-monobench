//! 部品ごとの色は同じ表示操作から切り替え、文書の形と履歴は変えない。

use super::*;
use egui_kittest::Harness;
use kmb_core::Document;
use serde_json::json;

fn document() -> Document {
    let part = kmb_samples::reference_part();
    let mut value: serde_json::Value = serde_json::from_str(&part.doc.to_json()).unwrap();
    let id = value["next_id"].as_u64().unwrap();
    value["components"] = json!([{"id":id,"name":"部品","bodies":[part.plate]}]);
    value["next_id"] = json!(id + 1);
    Document::from_json(&value.to_string()).unwrap()
}

#[test]
fn the_view_menu_and_script_share_colors_without_recomputing_or_changing_the_document() {
    let mut ws = Workspace::new(document(), None, kmb_bundle::modules());
    ws.wait();
    let before = ws.document().to_json();
    let computes = ws.cur.work.recomputes();
    let points = ws.cur.scene.bodies[0].mesh.positions.as_ptr();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1600., 900.))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    let r = crate::cmdline_tests::run(&mut h, "click 表示\nclick 部品ごとに色を変える");
    assert!(r.ok, "{}", r.report());
    assert!(h.state().cur.work.doc_view().part_hues);
    assert!(!h.state().cur.scene.part_hues().is_empty());
    let r = crate::cmdline_tests::run(&mut h, "run view.part_hues on=false");
    assert!(r.ok, "{}", r.report());
    assert!(!h.state().cur.work.doc_view().part_hues);
    assert!(h.state().cur.scene.part_hues().is_empty());
    assert_eq!(h.state().document().to_json(), before);
    assert!(!h.state().is_modified());
    assert!(!h.state().document().can_undo());
    assert_eq!(h.state().cur.work.recomputes(), computes);
    assert_eq!(
        h.state().cur.scene.bodies[0].mesh.positions.as_ptr(),
        points
    );
}

#[test]
fn document_tabs_keep_their_own_component_colors_and_open_cancel_restores_unsaved_colors() {
    let dir = crate::tests::temp_dir("component-colors");
    let path = dir.join("part.json");
    kmb_session::save(&path, &document()).unwrap();
    let mut ws = Workspace::open(document(), None, true, kmb_bundle::modules())
        .with_data_dir(dir.join("data"));
    ws.wait();
    ws.run_through(
        kmb_session::doc_view::PART_HUES,
        None,
        None,
        &json!({"on":true}),
    )
    .unwrap();
    ws.new_tab();
    assert!(!ws.cur.work.doc_view().part_hues);
    ws.switch_tab(0);
    assert!(ws.cur.work.doc_view().part_hues);
    assert!(ws.load_path(&path));
    assert!(!ws.cur.work.doc_view().part_hues);
    assert!(ws.cancel_opening());
    assert!(ws.cur.work.doc_view().part_hues, "未保存の文書の色も戻る");
    ws.wait();
    std::fs::remove_dir_all(dir).unwrap();
}
