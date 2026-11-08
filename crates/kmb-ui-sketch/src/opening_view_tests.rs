//! 確かめること: 開く時の視点、描く・選ぶ・寸法、手動の正面、設定の切替と開閉。

use super::*;
use crate::tests::{click_at, current_sketch, editor, empty_sketch};
use egui_kittest::kittest::Queryable;
use kmb_sketch::Entity;
use kmb_ui::FeatureEditor;

fn set_front(ws: &mut kmb_ui::Workspace, front: bool) {
    assert!(ws.change_settings(
        opening_settings::ID,
        &serde_json::json!({"look_front_on_open":front})
    ));
}

#[test]
fn camera_is_copied_whole_when_front_is_off_and_defaults_remain_front() {
    let mut editor = SketchEditor::new();
    let mut camera = kmb_ui::view3d::Camera::default();
    camera.set_view(-47.0, 32.0);
    camera.zoom = 7.5;
    camera.pan = egui::vec2(33.0, -19.0);
    camera.target = [11.0, 22.0, 33.0];
    camera.roll = 0.2;
    camera.perspective = true;
    camera.fov_deg = 43.0;
    editor.open_view(camera);
    assert!(editor.view.space.is_none(), "既定は正面の平面ビュー");
    editor
        .change_settings(
            opening_settings::ID,
            &serde_json::json!({"look_front_on_open":false}),
            false,
        )
        .unwrap()
        .unwrap();
    editor.open_view(camera);
    let copied = editor.view.space.expect("現在の3Dの視点");
    assert_eq!(
        format!("{copied:?}"),
        format!("{camera:?}"),
        "非公開のfit/移動状態も含めて引き継ぐ"
    );
    assert!(!editor.view.refit, "開いただけでは全体表示し直さない");
    assert!(!editor.view.to_front);
    editor.reset();
    assert!(
        !editor.opening_settings.front(),
        "文書の編集状態を消してもPC設定は保つ"
    );
    assert_eq!(
        editor.fresh().settings(&|id| id.into())[0].value,
        serde_json::json!({"look_front_on_open":false})
    );
}

#[test]
fn drawing_selection_and_dimensions_work_from_the_preserved_oblique_view() {
    fn trace_click_at(h: &mut egui_kittest::Harness<'static, kmb_ui::Workspace>, p: [f64; 2]) {
        let pos = crate::tests::screen(h, p);
        h.hover_at(pos);
        h.run();
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
            h.run();
            let pointer = h.ctx.input(|i| {
                (
                    i.pointer.latest_pos(),
                    i.pointer.primary_clicked(),
                    i.pointer.primary_down(),
                )
            });
            let ed = editor(h.state_mut());
            let t = ed.view.last.as_ref().unwrap();
            eprintln!(
                "169 pointer event: world={p:?} delivered={pos:?} pressed={pressed} pointer={pointer:?} rect={:?} transform_center={:?} transform_scale={} pending={:?} tool={:?}",
                t.space.map(|space| space.rect),
                t.center_screen,
                t.scale,
                ed.editor.pending,
                ed.editor.tool
            );
        }
    }
    let mut h = empty_sketch();
    let id = h.state().selected().unwrap();
    h.state_mut().finish_sketch();
    h.run();
    set_front(h.state_mut(), false);
    h.state_mut().set_view(-47.0, 32.0);
    h.run();
    h.state_mut().open_edit(id);
    h.run();
    let copied = editor(h.state_mut()).view.space.expect("斜めの視点を保持");
    assert!((copied.yaw.to_degrees() + 47.0).abs() < 0.01);
    assert!((copied.pitch.to_degrees() - 32.0).abs() < 0.01);
    h.key_press(egui::Key::L);
    h.run();
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::Line);
    let (a, b) = ([-12.3, -8.5], [14.6, 9.7]);
    let before_clicks = h.state().document().to_json();
    {
        let ed = editor(h.state_mut());
        let t = ed.view.last.as_ref().unwrap();
        let rect = t.space.map(|space| space.rect);
        eprintln!(
            "169 before clicks: camera={:?} a={a:?} screen_a={:?} b={b:?} screen_b={:?} rect={rect:?} contains_a={:?} contains_b={:?}",
            ed.view.space,
            t.to_screen(a),
            t.to_screen(b),
            rect.map(|r| r.contains(t.to_screen(a))),
            rect.map(|r| r.contains(t.to_screen(b)))
        );
    }
    trace_click_at(&mut h, a);
    {
        let ed = editor(h.state_mut());
        eprintln!(
            "169 after first click: tool={:?} pending={:?} notice={:?}",
            ed.editor.tool,
            ed.editor.pending.as_ref().map(|pick| pick.pos),
            ed.editor.notice
        );
    }
    eprintln!(
        "169 after first click: entities={:?} errors={} document_changed={}",
        current_sketch(h.state()).entities,
        h.state().report().error_count(),
        h.state().document().to_json() != before_clicks
    );
    trace_click_at(&mut h, b);
    {
        let ed = editor(h.state_mut());
        eprintln!(
            "169 after second click: tool={:?} pending={:?} notice={:?}",
            ed.editor.tool,
            ed.editor.pending.as_ref().map(|pick| pick.pos),
            ed.editor.notice
        );
    }
    eprintln!(
        "169 after second click: entities={:?} errors={} document_changed={}",
        current_sketch(h.state()).entities,
        h.state().report().error_count(),
        h.state().document().to_json() != before_clicks
    );
    let sketch = current_sketch(h.state());
    let line = sketch
        .entities
        .iter()
        .find(|e| matches!(e, Entity::Line { .. }))
        .unwrap()
        .id();
    editor(h.state_mut()).set_tool(Tool::Select);
    h.run();
    let middle = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
    click_at(&mut h, middle);
    assert!(editor(h.state_mut()).editor.selection.contains(&line));
    h.key_press(egui::Key::D);
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    let sketch = current_sketch(h.state());
    assert!(sketch.constraints.iter().any(|c| matches!(c.kind,
        kmb_sketch::ConstraintKind::Length { line: target, .. } if target == line)));
    assert_eq!(h.state().report().error_count(), 0);
    h.get_by_label("正面から見る").click();
    for _ in 0..45 {
        h.step();
        if editor(h.state_mut()).view.space.is_none() {
            break;
        }
    }
    h.run();
    assert!(
        editor(h.state_mut()).view.space.is_none(),
        "設定切でも手動で正面へ戻れる"
    );
}

#[test]
fn toggling_the_setting_applies_on_the_next_open_and_reopening_keeps_the_current_view() {
    let mut h = empty_sketch();
    let id = h.state().selected().unwrap();
    h.state_mut().finish_sketch();
    h.run();
    set_front(h.state_mut(), false);
    h.state_mut().set_view(-37.0, 29.0);
    h.run();
    for _ in 0..2 {
        h.state_mut().open_edit(id);
        h.run();
        let camera = editor(h.state_mut()).view.space.unwrap();
        assert!((camera.yaw.to_degrees() + 37.0).abs() < 0.01);
        assert!((camera.pitch.to_degrees() - 29.0).abs() < 0.01);
        set_front(h.state_mut(), true);
        h.run();
        assert!(
            editor(h.state_mut()).view.space.is_some(),
            "設定変更だけで編集中の視点を動かさない"
        );
        set_front(h.state_mut(), false);
        h.state_mut().finish_sketch();
        h.run();
    }
    set_front(h.state_mut(), true);
    h.state_mut().open_edit(id);
    h.run();
    assert!(
        editor(h.state_mut()).view.space.is_none(),
        "次の開く時に正面の設定を使う"
    );
    set_front(h.state_mut(), false);
    h.run();
    assert!(
        editor(h.state_mut()).view.space.is_none(),
        "切替そのもので編集中の図は回さない"
    );
}

#[test]
fn opening_preference_checkbox_is_on_by_default_and_reaches_the_extension() {
    let editor = SketchEditor::new();
    let store = editor.opening_settings.clone();
    let mut ws = kmb_ui::Workspace::new(kmb_core::Document::new(), None, kmb_bundle::modules())
        .with_editor(editor);
    ws.open_settings_at("design/sketch.opening_view");
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, ws: &mut kmb_ui::Workspace| ws.ui(ui), ws);
    h.run();
    assert!(store.front());
    let batch = h
        .state_mut()
        .run_script("click \"開く時に正面から見る\"")
        .unwrap();
    let started = std::time::Instant::now();
    loop {
        h.step();
        if let Some(result) = h.state_mut().script_result(batch) {
            assert!(result.ok, "{}", result.report());
            break;
        }
        assert!(started.elapsed().as_secs() < 15, "設定の台本の時間切れ");
    }
    h.run();
    assert!(!store.front());
    assert!(
        h.state_mut()
            .reset_module_settings()
            .contains(&opening_settings::ID.into())
    );
    assert!(store.front());
}

#[test]
fn opening_preference_persists_but_safe_mode_keeps_defaults_and_preserves_the_file() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "kmb-opening-settings-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir(&dir).unwrap();
    let editor = SketchEditor::new();
    let mut ws = kmb_ui::Workspace::new(kmb_core::Document::new(), None, kmb_bundle::modules())
        .with_editor(editor)
        .with_data_dir(dir.clone());
    set_front(&mut ws, false);
    let path = dir.join("sketch-opening-view.json");
    let saved = std::fs::read(&path).unwrap();
    let editor = SketchEditor::new();
    let loaded = editor.opening_settings.clone();
    let _normal = kmb_ui::Workspace::new(kmb_core::Document::new(), None, kmb_bundle::modules())
        .with_editor(editor)
        .with_data_dir(dir.clone());
    assert!(!loaded.front());
    let editor = SketchEditor::new();
    let store = editor.opening_settings.clone();
    let mut safe = kmb_ui::Workspace::new(kmb_core::Document::new(), None, kmb_bundle::modules())
        .with_safe_mode()
        .with_editor(editor)
        .with_data_dir(dir.clone());
    assert!(store.front(), "保存済みoffを安全モードでは読まない");
    set_front(&mut safe, true);
    assert_eq!(
        std::fs::read(&path).unwrap(),
        saved,
        "安全モードでは保存しない"
    );
    kmb_session::write_output(&path, b"{invalid").unwrap();
    let editor = SketchEditor::new();
    let bad = editor.opening_settings.clone();
    let _bad = kmb_ui::Workspace::new(kmb_core::Document::new(), None, kmb_bundle::modules())
        .with_editor(editor)
        .with_data_dir(dir.clone());
    assert!(bad.front(), "異常な設定では既定を使い起動を止めない");
    assert_eq!(std::fs::read(&path).unwrap(), b"{invalid");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn direct_edit_switch_and_creation_copy_the_visible_editor_before_reset() {
    let mut h = empty_sketch();
    let first = h.state().selected().unwrap();
    set_front(h.state_mut(), false);
    let second = h.state_mut().run_action(kmb_sketch::ADD_SKETCH).unwrap();
    h.run();
    let center = editor(h.state_mut())
        .view
        .last
        .as_ref()
        .unwrap()
        .center_screen;
    crate::space_view_tests::right_drag(&mut h, center, center + egui::vec2(90.0, 50.0));
    let visible = editor(h.state_mut()).view.seen.unwrap().0;
    // A の専用ビューが描かれた直後、通常の 3D のフレームを挟まず B を開く。
    h.state_mut().open_edit(first);
    let switched = editor(h.state_mut()).view.space.unwrap();
    assert_eq!(format!("{switched:?}"), format!("{visible:?}"));
    h.run();
    let visible = editor(h.state_mut()).view.seen.unwrap().0;
    h.state_mut().finish_sketch();
    h.state_mut().open_edit(second);
    let reopened = editor(h.state_mut()).view.space.unwrap();
    assert_eq!(
        format!("{reopened:?}"),
        format!("{visible:?}"),
        "finish後も3Dフレームを待たずに現在視点を渡す"
    );
    h.run();
    let visible = editor(h.state_mut()).view.seen.unwrap().0;
    h.state_mut().run_action(kmb_sketch::ADD_SKETCH).unwrap();
    let created = editor(h.state_mut()).view.space.unwrap();
    assert_eq!(
        format!("{created:?}"),
        format!("{visible:?}"),
        "新規作成でもreset前に現在の専用ビューから引き継ぐ"
    );
}
