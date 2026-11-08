//! 確かめること: 画面への要求で複数の式をため、適用と取り消しを1回にまとめる。
use crate::*;
use egui_kittest::{Harness, kittest::Queryable};
use serde_json::json;

struct Scratch(std::path::PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture(language: &str, width: f32, scale: f32) -> (Harness<'static, Workspace>, Scratch) {
    let mut doc = kmb_core::Document::new();
    for (name, expr) in [("width", "10 mm"), ("height", "20 mm"), ("depth", "30 mm")] {
        let tx = kmb_session::edit::add_param(&mut doc, name, expr);
        doc.apply(tx).unwrap();
    }
    let dir = Scratch(std::env::temp_dir().join(format!(
            "param-batch-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    let mut ws = Workspace::new(doc, None, kmb_bundle::modules()).with_data_dir(dir.0.clone());
    ws.win.show_params = true;
    ws.layout_mut().ai_live = true;
    ws.layout_mut().appearance.language = language.into();
    ws.layout_mut().appearance.text_scale = scale;
    ws.layout_mut().appearance.icon_scale = scale;
    let mut h = Harness::builder()
        .with_size(egui::vec2(width, 900.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    (h, dir)
}

fn finish(mut h: Harness<'static, Workspace>, dir: Scratch) {
    h.state_mut().live_disconnect();
    drop(h);
    drop(dir);
}

#[test]
fn mcp_ui_tools_stage_three_expressions_then_apply_and_undo_them_together() {
    let (mut h, dir) = fixture("ja", 1400.0, 1.0);
    let before = h.state().document().to_json();
    let history = h.state().cur.work.mark();
    app(&mut h, &dir.0, "ui_click", json!({"target":"まとめて適用"}));
    for (name, new) in [
        ("width", "40 mm"),
        ("height", "width / 2"),
        ("depth", "height + 5 mm"),
    ] {
        app(
            &mut h,
            &dir.0,
            "ui_type",
            json!({"target":format!("{name} の式"),"text":new}),
        );
    }
    assert_eq!(
        h.state().document().to_json(),
        before,
        "適用までは文書を変えない"
    );
    app(&mut h, &dir.0, "ui_click", json!({"target":"適用"}));
    assert_eq!(h.state().cur.work.mark(), history + 1);
    for (name, expected) in [
        ("width", "40 mm"),
        ("height", "width / 2"),
        ("depth", "height + 5 mm"),
    ] {
        assert_eq!(
            h.state()
                .document()
                .data()
                .param_by_name(name)
                .unwrap()
                .expr,
            expected
        );
    }
    app(&mut h, &dir.0, "ui_key", json!({"keys":"Ctrl+Z"}));
    assert_eq!(h.state().document().to_json(), before);
    finish(h, dir);
}

#[test]
fn a_bad_expression_keeps_every_edit_pending_and_discard_changes_no_document() {
    let (mut h, dir) = fixture("ja", 1400.0, 1.0);
    let before = h.state().document().to_json();
    let history = h.state().cur.work.mark();
    app(&mut h, &dir.0, "ui_click", json!({"target":"まとめて適用"}));
    app(
        &mut h,
        &dir.0,
        "ui_type",
        json!({"target":"width の式","text":"("}),
    );
    app(
        &mut h,
        &dir.0,
        "ui_type",
        json!({"target":"height の式","text":"60 mm"}),
    );
    app(&mut h, &dir.0, "ui_click", json!({"target":"適用"}));
    assert_eq!(h.state().document().to_json(), before);
    assert_eq!(h.state().cur.work.mark(), history);
    let key = kmb_core::ParamKey::User {
        id: h.state().cur.work.param_id("width").unwrap(),
    };
    assert!(h.state().cur.param_batch.errors.contains_key(&key));
    assert_eq!(h.state().cur.param_batch.inputs.len(), 2);
    assert!(h.query_by_label("式を直してから適用してください").is_some());
    app(&mut h, &dir.0, "ui_click", json!({"target":"やめる"}));
    assert!(h.state().cur.param_batch.inputs.is_empty());
    assert!(h.state().cur.param_batch.errors.is_empty());
    assert_eq!(h.state().document().to_json(), before);
    finish(h, dir);
}

#[test]
fn immediate_edits_remain_the_default_and_turning_batch_mode_off_discards_pending_edits() {
    let (mut h, dir) = fixture("ja", 1400.0, 1.0);
    assert!(!h.state().cur.param_batch.enabled);
    app(
        &mut h,
        &dir.0,
        "ui_type",
        json!({"target":"width の式","text":"45 mm"}),
    );
    assert_eq!(
        h.state()
            .document()
            .data()
            .param_by_name("width")
            .unwrap()
            .expr,
        "45 mm"
    );
    let before = h.state().document().to_json();
    app(&mut h, &dir.0, "ui_click", json!({"target":"まとめて適用"}));
    app(
        &mut h,
        &dir.0,
        "ui_type",
        json!({"target":"width の式","text":"70 mm"}),
    );
    app(&mut h, &dir.0, "ui_click", json!({"target":"まとめて適用"}));
    assert!(!h.state().cur.param_batch.enabled);
    assert!(h.state().cur.param_batch.inputs.is_empty());
    assert_eq!(h.state().document().to_json(), before);
    finish(h, dir);
}

#[test]
fn batch_controls_use_english_and_remain_readable_at_large_text_scale() {
    for (width, scale) in [(1280.0, 1.0), (1920.0, 1.0), (1280.0, 2.5), (1920.0, 2.5)] {
        let (mut h, dir) = fixture("en", width, scale);
        app(&mut h, &dir.0, "ui_click", json!({"target":"Batch edits"}));
        app(
            &mut h,
            &dir.0,
            "ui_type",
            json!({"target":"Expression for width","text":"40 mm"}),
        );
        h.get_by_label("Batch edits").scroll_to_me();
        h.run_steps(20);
        for label in ["Batch edits", "Apply", "Discard"] {
            assert!(h.output().shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text()==label && s.clip_rect.contains_rect(egui::Rect::from_min_size(t.pos,t.galley.size())))), "欄の文字が切れない: {label} {width} {scale}");
        }
        app(&mut h, &dir.0, "ui_click", json!({"target":"Discard"}));
        assert!(h.state().cur.param_batch.inputs.is_empty());
        finish(h, dir);
    }
}

fn app(
    h: &mut Harness<'static, Workspace>,
    dir: &std::path::Path,
    tool: &str,
    args: serde_json::Value,
) {
    let (dir, tool) = (dir.to_owned(), tool.to_owned());
    let request = std::thread::spawn(move || kmb_session::live::call(&dir, &tool, args));
    let start = std::time::Instant::now();
    while !request.is_finished() {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(2));
        assert!(start.elapsed().as_secs() < 20, "画面の要求が時間切れ");
    }
    let result = request.join().unwrap().unwrap();
    assert_eq!(result["ok"], true, "{result}");
    h.run_steps(12);
}
