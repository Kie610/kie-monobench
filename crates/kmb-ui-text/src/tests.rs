//! 文字の編集器の試験。人の操作(クリックとキー)でフォントを選び、字を打ち足す。

use super::*;
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kmb_core::{Document, FeatureStatus, Profile};
use kmb_ui::Workspace;

fn fonts() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../kmb-text/tests/fonts")
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
    let (tx, id) = kmb_session::edit::insert_feature(
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
    let mut ws = Workspace::new(doc, None, kmb_bundle::modules())
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

#[path = "../../kmb-text/src/test_fonts.rs"]
mod gothic_test_fonts;

fn script(h: &mut Harness<'static, Workspace>, text: &str) {
    let batch = h.state_mut().run_script(text).expect("読める台本");
    let started = std::time::Instant::now();
    loop {
        h.step();
        if let Some(result) = h.state_mut().script_result(batch) {
            assert!(result.ok, "{}", result.report());
            return;
        }
        assert!(
            started.elapsed().as_secs() < 30,
            "台本が終わらない: {:?}",
            h.state().command_log()
        );
        if h.state().is_busy() {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

#[test]
fn newly_placed_text_shows_its_gothic_font_and_can_return_to_single_lines() {
    let fonts = gothic_test_fonts::GothicFonts::new(&fonts().join("kmb-test-jp.ttf"));
    let modules = kmb_bundle::modules();
    let data = fonts.path().join("data");
    std::fs::create_dir_all(data.join("fonts")).unwrap();
    std::fs::copy(
        fonts.path().join("gothic.ttf"),
        data.join("fonts/gothic.ttf"),
    )
    .unwrap();
    let ws = Workspace::new(kmb_bundle::Sample::Part.make()(), None, modules)
        .with_editor(TextEditor::new().with_font_dir(fonts.path()))
        .with_data_dir(data);
    let mut h = harness(ws);
    kmb_ui::install_fonts(&h.ctx, None);
    runs(&mut h);
    script(
        &mut h,
        r#"
        pick f12:end
        click "作成 ▾"
        click "面に文字"
        wait
        until "combobox:フォント" exists
    "#,
    );
    let id = h
        .state()
        .document()
        .data()
        .features
        .iter()
        .find(|f| f.type_id == TEXT)
        .unwrap()
        .id;
    let t = text_of(h.state(), id);
    capture_panel(&mut h, "new-default");
    let f = t.outline.as_ref().expect("新しい文字はゴシック体");
    let mut index = FontIndex::default();
    index.add_dir(fonts.path());
    assert_eq!(f.family, index.gothic().unwrap().family);
    script(
        &mut h,
        &format!("expect \"フォント\" = \"{}\"", f.label_or_family()),
    );
    script(
        &mut h,
        r#"
        type "文字" "自作の名入れ"
        wait
        expect "~字「" missing
    "#,
    );
    let t = text_of(h.state(), id);
    assert!(t.outline.as_ref().unwrap().missing(&t.text).is_empty());
    let Some(FeatureStatus::Ok { outputs, .. }) = h.state().report().feature(id) else {
        panic!("文字の輪郭ができる")
    };
    assert!(
        !Profile::from_value(&outputs["profile"])
            .unwrap()
            .regions
            .is_empty()
    );
    capture_panel(&mut h, "japanese");
    script(
        &mut h,
        r#"
        click "combobox:フォント"
        click "1 本線(飾りなし)"
        wait
    "#,
    );
    assert!(text_of(h.state(), id).outline.is_none());
}

fn capture_panel(h: &mut Harness<'_, Workspace>, name: &str) {
    let Some(dir) = std::env::var_os("JC_SCREENS_OUT") else {
        return;
    };
    h.step();
    let output = h.output().clone();
    let primitives = h.ctx.tessellate(output.shapes, output.pixels_per_point);
    let atlas = h.ctx.fonts(|f| f.image());
    let field = h.get_by_role_and_label(Role::ComboBox, "フォント").rect();
    let label = label_of(&text_of(h.state(), h.state().selected().unwrap()));
    let meshes: Vec<_> = primitives.into_iter().filter_map(|p| {
        let egui::epaint::Primitive::Mesh(m) = p.primitive else { return None };
        (m.texture_id == egui::TextureId::default()).then(|| serde_json::json!({
            "clip": [p.clip_rect.min.x, p.clip_rect.min.y, p.clip_rect.max.x, p.clip_rect.max.y],
            "vertices": m.vertices.iter().map(|v| (v.pos.x, v.pos.y, v.uv.x, v.uv.y, v.color.to_array())).collect::<Vec<_>>(),
            "indices": m.indices,
        }))
    }).collect();
    let path = std::path::Path::new(&dir).join(format!("{name}.json"));
    std::fs::create_dir_all(&dir).unwrap();
    kmb_session::write_output(&path, &serde_json::to_vec(&serde_json::json!({
        "size": [1400, 1000], "ppp": output.pixels_per_point,
        "font_label": label, "font_rect": [field.min.x, field.min.y, field.max.x, field.max.y],
        "atlas_size": atlas.size, "atlas": atlas.pixels.iter().map(|c| c.to_array()).collect::<Vec<_>>(), "meshes": meshes,
    })).unwrap()).unwrap();
}
