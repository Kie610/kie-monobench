//! 確かめること: 表の不正な式を保存せず、欄の色と理由を残し、既存の不正な行は書き換えない。
use crate::*;
use egui_kittest::Harness;
use kmb_core::{Document, ParamStatus};
use kmb_interchange::configuration::{ConfigCell, ConfigColumn, ConfigRow, Configurations};
use serde_json::json;

struct Scratch(std::path::PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture(expr: &str, cell: Option<&str>) -> (Harness<'static, Workspace>, Scratch) {
    let mut doc = Document::new();
    let tx = kmb_session::edit::add_param(&mut doc, "w", expr);
    doc.apply(tx).unwrap();
    if let Some(cell) = cell {
        let table = Configurations {
            columns: vec![ConfigColumn::Param {
                param: doc.data().params[0].id,
            }],
            rows: vec![ConfigRow {
                name: "row".into(),
                cells: vec![ConfigCell::Expr(cell.into())],
            }],
            ..Default::default()
        };
        let op = doc.data().section_op(&table).unwrap();
        doc.apply(kmb_core::Transaction::new("見本の構成表", vec![op]))
            .unwrap();
    }
    let doc = Document::from_json(&doc.to_json()).unwrap();
    let dir = Scratch(std::env::temp_dir().join(format!(
            "param-validation-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    let mut ws = Workspace::new(doc, None, kmb_bundle::modules()).with_data_dir(dir.0.clone());
    ws.win.show_params = true;
    ws.layout_mut().ai_live = true;
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 1000.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    (h, dir)
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

fn red_input(h: &Harness<'static, Workspace>, text: &str) -> bool {
    let err = h.state().win.look.pal.err;
    h.output().shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()==text && (t.override_text_color==Some(err) || t.galley.job.sections.iter().any(|s|s.format.color==err))))
}

fn finish(mut h: Harness<'static, Workspace>, dir: Scratch) {
    h.state_mut().live_disconnect();
    drop(h);
    drop(dir);
}

#[test]
fn mcp_edits_refuse_unknown_units_and_cycles_without_changing_the_document() {
    let (mut h, dir) = fixture("10 mm", None);
    let before = h.state().document().to_json();
    let mark = h.state().cur.work.mark();
    for expr in ["missing_name + 1 mm", "1 mm + 1 deg", "w + 1 mm"] {
        app(
            &mut h,
            &dir.0,
            "ui_type",
            json!({"target":"label:式#1","text":expr}),
        );
        assert_eq!(h.state().document().to_json(), before);
        assert_eq!(h.state().cur.work.mark(), mark);
        assert!(red_input(&h, expr), "拒否した式を誤りの色で残す: {expr}");
        assert!(h.state().win.message.is_some());
    }
    app(
        &mut h,
        &dir.0,
        "ui_type",
        json!({"target":"label:式#1","text":"20 mm"}),
    );
    assert_eq!(h.state().document().data().params[0].expr, "20 mm");
    assert!(!red_input(&h, "20 mm"));
    finish(h, dir);
}

#[test]
fn mcp_configuration_edits_refuse_bad_cells_and_keep_their_error_color() {
    let (mut h, dir) = fixture("10 mm", Some("10 mm"));
    let before = h.state().document().to_json();
    app(
        &mut h,
        &dir.0,
        "ui_type",
        json!({"target":"row の w","text":"missing_name + 1 mm"}),
    );
    assert_eq!(h.state().document().to_json(), before);
    assert!(red_input(&h, "missing_name + 1 mm"));
    assert!(h.state().win.message.is_some());
    app(
        &mut h,
        &dir.0,
        "ui_type",
        json!({"target":"row の w","text":"20 mm"}),
    );
    assert_eq!(
        kmb_interchange::configuration::of(h.state().document().data()).rows[0].cells[0],
        ConfigCell::Expr("20 mm".into())
    );
    assert!(!red_input(&h, "20 mm"));
    finish(h, dir);
}

#[test]
fn existing_invalid_parameter_and_configuration_rows_stay_unchanged_and_show_errors() {
    for configuration in [false, true] {
        let expr = if configuration {
            "10 mm"
        } else {
            "missing_name + 1 mm"
        };
        let (mut h, dir) = fixture(expr, configuration.then_some("missing_name + 1 mm"));
        let before = h.state().document().to_json();
        h.run_steps(12);
        assert_eq!(h.state().document().to_json(), before);
        assert!(red_input(&h, "missing_name + 1 mm"));
        if configuration {
            assert!(!h.state().cur.work.configuration_issues().is_empty());
        } else {
            assert!(matches!(
                h.state().report().params[0].status,
                ParamStatus::Error { .. }
            ));
        }
        finish(h, dir);
    }
}
