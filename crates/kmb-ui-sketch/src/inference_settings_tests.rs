//! 確かめること: 設定画面からの適用と編集器のリセット、タブ複製、安全モードの境界。

use super::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kmb_ui::FeatureEditor;

#[test]
fn setting_checkbox_reaches_the_extension_and_survives_reset_and_fresh() {
    let editor = SketchEditor::new();
    let store = editor.inference_settings.clone();
    let mut ws = kmb_ui::Workspace::new(
        kmb_samples::reference_document(),
        None,
        kmb_bundle::modules(),
    )
    .with_editor(editor);
    ws.open_settings_at("design/sketch.inference");
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, ws: &mut kmb_ui::Workspace| ws.ui(ui), ws);
    harness.run();
    assert!(!store.enabled());
    eprintln!(
        "direct label query selects: {:#?}",
        harness.get_by_label("直近の補助線の交点にスナップ")
    );
    let batch = harness
        .state_mut()
        .run_script("click \"直近の補助線の交点にスナップ\"")
        .unwrap();
    let started = std::time::Instant::now();
    loop {
        harness.step();
        if let Some(result) = harness.state_mut().script_result(batch) {
            eprintln!("{}", result.report());
            assert!(result.ok, "{}", result.report());
            break;
        }
        assert!(started.elapsed().as_secs() < 15, "設定の台本の時間切れ");
    }
    harness.run();
    assert!(store.enabled());
    let editor = harness.state_mut().editor_mut::<SketchEditor>().unwrap();
    editor.reset();
    assert!(editor.inference_settings.enabled());
    let fresh = editor.fresh();
    assert_eq!(
        fresh.settings(&|id| id.into())[0].value,
        serde_json::json!({"guide_intersections": true})
    );
    let reset = harness.state_mut().reset_module_settings();
    assert!(reset.iter().any(|id| id == inference_settings::ID));
    assert!(!store.enabled());
}

#[test]
fn malformed_settings_do_not_block_startup_and_safe_mode_does_not_load_or_save() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir =
        std::env::temp_dir().join(format!("kmb-guide-startup-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("sketch-inference.json");
    kmb_session::write_output(&path, b"{invalid").unwrap();
    let bad = SketchEditor::new();
    let store = bad.inference_settings.clone();
    let _ws = kmb_ui::Workspace::new(
        kmb_samples::reference_document(),
        None,
        kmb_bundle::modules(),
    )
    .with_editor(bad)
    .with_data_dir(dir.clone());
    assert!(!store.enabled());
    assert_eq!(std::fs::read(&path).unwrap(), b"{invalid");
    kmb_session::write_output(&path, br#"{"guide_intersections":true}"#).unwrap();
    let saved = std::fs::read(&path).unwrap();
    let editor = SketchEditor::new();
    let safe_store = editor.inference_settings.clone();
    let mut safe = kmb_ui::Workspace::new(
        kmb_samples::reference_document(),
        None,
        kmb_bundle::modules(),
    )
    .with_safe_mode()
    .with_editor(editor)
    .with_data_dir(dir.clone());
    assert!(
        !safe_store.enabled(),
        "安全モードでは保存された設定を読まない"
    );
    assert!(safe.change_settings(
        inference_settings::ID,
        &serde_json::json!({"guide_intersections": false})
    ));
    assert_eq!(
        std::fs::read(&path).unwrap(),
        saved,
        "安全モードでは元の設定を書き換えない"
    );
    std::fs::remove_file(path).unwrap();
    // 共通設定の読み込みが作る周辺ファイルがあっても、試験自身の専用フォルダだけを片付ける。
    std::fs::remove_dir_all(dir).unwrap();
}
