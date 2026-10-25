//! 文字の編集器の試験。人の操作(クリックとキー)でフォントを選び、字を打ち足す。

use super::*;
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use jc_core::{Document, FeatureStatus, Profile};
use jc_ui::Workspace;

fn fonts() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../jc-text/tests/fonts")
}

fn harness(ws: Workspace) -> Harness<'static, Workspace> {
    Harness::builder()
        .with_size(egui::vec2(1400.0, 1000.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws)
}

fn runs(h: &mut Harness<'_, Workspace>) {
    for _ in 0..4 {
        h.run();
    }
}

fn text_of(ws: &Workspace, id: FeatureId) -> Text {
    serde_json::from_value(ws.document().data().feature(id).unwrap().data.clone()).unwrap()
}

#[test]
fn a_pc_font_is_chosen_from_the_list_and_new_letters_are_filled_in() {
    let mut doc = Document::new();
    let (tx, id) = jc_session::edit::insert_feature(
        &mut doc,
        "文字を追加",
        TEXT,
        "名入れ",
        serde_json::to_value(Text {
            text: "自作".into(),
            ..Default::default()
        })
        .unwrap(),
    );
    doc.apply(tx).unwrap();
    let mut ws = Workspace::new(doc, None, jc_bundle::modules())
        .with_editor(TextEditor::new().with_font_dir(&fonts()));
    ws.wait();
    ws.open_edit(id);
    let mut h = harness(ws);
    runs(&mut h);
    // 「フォント」の一覧を開き、名前で絞って試験のフォントを選ぶ
    h.get_by_role_and_label(Role::ComboBox, "フォント").click();
    runs(&mut h);
    h.get_by_role_and_label(Role::TextInput, "名前で絞る")
        .click();
    runs(&mut h);
    h.event(egui::Event::Text("jp".into()));
    runs(&mut h);
    assert!(
        h.query_by_label("JC Test CFF Regular").is_none(),
        "絞った一覧に合わないフォントは出ない"
    );
    h.get_by_label("JC Test JP Regular").click();
    runs(&mut h);
    let t = text_of(h.state(), id);
    let f = t.outline.clone().expect("輪郭のフォントになった");
    assert_eq!(f.family, "JC Test JP");
    assert!(f.missing("自作").is_empty(), "打った字の形が入る");
    // 文字を直すと、足した字の形を PC のフォントから入れる
    let mut more = t.clone();
    more.text = "自作回".into();
    h.state_mut()
        .set_feature_data(id, serde_json::to_value(&more).unwrap());
    runs(&mut h);
    h.state_mut().wait();
    runs(&mut h);
    let t = text_of(h.state(), id);
    assert!(t.outline.as_ref().unwrap().glyph('回').is_some());
    let Some(FeatureStatus::Ok { outputs, .. }) = h.state().report().feature(id) else {
        panic!("{:?}", h.state().report().feature(id))
    };
    let p = Profile::from_value(&outputs["profile"]).unwrap();
    assert!(
        p.regions.len() >= 4,
        "自・作・回の領域: {}",
        p.regions.len()
    );
    // 1 本線に戻せる
    h.get_by_role_and_label(Role::ComboBox, "フォント").click();
    runs(&mut h);
    h.get_by_label("1 本線(飾りあり)").click();
    runs(&mut h);
    let t = text_of(h.state(), id);
    assert!(t.outline.is_none());
    assert_eq!(t.font, Font::Roman);
}
