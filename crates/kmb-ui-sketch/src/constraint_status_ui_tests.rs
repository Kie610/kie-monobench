//! 行の拘束状態は閉じても残り、編集の帯は普通の MCP 操作で開いた間だけ出る。
use crate::tests::{reg, ws_of};
use egui_kittest::{Harness, kittest::Queryable};
use kmb_core::{Document, Edit};
use kmb_sketch::{ConstraintKind, Sketch};
use kmb_ui::Workspace;
use serde_json::json;

struct Scratch(std::path::PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_participating_busy_summary_clears_success_without_changing_the_legacy_fallback() {
    // 本物の成功を確定してから裏の再計算を始める。時計や計算時間には依存しない。
    let mut sketch = Sketch::default();
    let a = sketch.add_point(0.0, 0.0);
    let b = sketch.add_point(10.0, 0.0);
    let line = sketch.add_line(a, b);
    sketch.add_constraint(ConstraintKind::Fix { point: a });
    sketch.add_constraint(ConstraintKind::Horizontal { line });
    sketch.add_constraint(ConstraintKind::Length {
        line,
        value: "10 mm".into(),
    });
    let mut doc = Document::new();
    let id = doc
        .apply_edits(
            "見本",
            vec![Edit::Insert {
                type_id: kmb_sketch::SKETCH.into(),
                name: "Busy fixture".into(),
                data: serde_json::to_value(&sketch).unwrap(),
            }],
        )
        .unwrap()[0];
    let modules = || {
        kmb_core::Modules::assemble(
            vec![kmb_sketch::module()],
            std::sync::Arc::new(kmb_core::NullKernel),
        )
    };
    let mut participating = Workspace::open(
        Document::from_json(&doc.to_json()).unwrap(),
        None,
        true,
        modules(),
    )
    .with_editor(crate::SketchEditor::new());
    let mut legacy = Workspace::open(doc, None, true, modules());
    participating.layout_mut().appearance.language = "en".into();
    legacy.layout_mut().appearance.language = "en".into();
    participating.wait();
    legacy.wait();
    assert_eq!(
        participating.summary(id).as_deref(),
        Some("Fully constrained")
    );
    assert_eq!(legacy.summary(id).as_deref(), Some("完全拘束"));
    // 新しい入力を送った直後は、完了を取り込むまで前の成功した report が残る。
    sketch.add_constraint(ConstraintKind::Length {
        line,
        value: "10 mm".into(),
    });
    let data = serde_json::to_value(sketch).unwrap();
    assert!(participating.set_feature_data(id, data.clone()));
    assert!(legacy.set_feature_data(id, data));
    assert!(participating.is_busy());
    assert!(legacy.is_busy());
    let participating_summary = participating.summary(id);
    let legacy_summary = legacy.summary(id);
    participating.wait();
    legacy.wait();
    assert_eq!(
        legacy_summary.as_deref(),
        Some("完全拘束"),
        "非参加の既存の要約は変えない"
    );
    assert_eq!(
        participating_summary, None,
        "再計算中は古い成功の文に戻さない"
    );
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
        assert!(start.elapsed().as_secs() < 15, "画面への要求が時間切れ");
    }
    let reply = request.join().unwrap().unwrap();
    assert_eq!(reply["ok"], true, "{reply}");
    h.run_steps(8);
}

#[test]
fn mcp_open_and_close_show_the_current_constraint_state_without_editing_the_document() {
    for language in ["ja", "en"] {
        for (dimensions, mark, ja, en) in [
            (0, "○", "拘束不足 自由度 1", "Underconstrained · 1 DOF"),
            (1, "✔", "完全拘束", "Fully constrained"),
            (2, "⚠", "過剰な拘束", "Overconstrained"),
        ] {
            let mut sketch = Sketch::default();
            let a = sketch.add_point(0.0, 0.0);
            let b = sketch.add_point(10.0, 0.0);
            let line = sketch.add_line(a, b);
            sketch.add_constraint(ConstraintKind::Fix { point: a });
            sketch.add_constraint(ConstraintKind::Horizontal { line });
            for _ in 0..dimensions {
                sketch.add_constraint(ConstraintKind::Length {
                    line,
                    value: "10 mm".into(),
                });
            }
            let mut doc = Document::new();
            let id = doc
                .apply_edits(
                    "見本",
                    vec![Edit::Insert {
                        type_id: kmb_sketch::SKETCH.into(),
                        name: "Status fixture".into(),
                        data: serde_json::to_value(sketch).unwrap(),
                    }],
                )
                .unwrap()[0];
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let dir = Scratch(
                std::env::temp_dir()
                    .join(format!("constraint-status-{}-{stamp}", std::process::id())),
            );
            let mut ws = ws_of(doc, None, reg()).with_data_dir(dir.0.clone());
            ws.layout_mut().ai_live = true;
            ws.layout_mut().appearance.language = language.into();
            let mut h = Harness::builder()
                .with_size(egui::vec2(1400.0, 800.0))
                .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
            h.run();
            assert!(h.state().live_port().is_some());
            let before = h.state().document().to_json();
            let state = if language == "ja" { ja } else { en };
            let band = format!("{mark} Status fixture · {state}");
            assert_eq!(h.state().summary(id).as_deref(), Some(state));
            assert!(
                h.query_by_label(&band).is_none(),
                "閉じた行に編集の帯は出ない"
            );
            app(
                &mut h,
                &dir.0,
                "ui_click",
                json!({"target":"button:Status fixture#1","button":"double"}),
            );
            app(
                &mut h,
                &dir.0,
                "ui_expect",
                json!({"target":format!("label:{band}"),"state":"exists"}),
            );
            app(
                &mut h,
                &dir.0,
                "ui_click",
                json!({"target":"button:✓  スケッチを終了#1"}),
            );
            assert!(
                h.query_by_label(&band).is_none(),
                "閉じると編集の帯が消える"
            );
            assert_eq!(
                h.state().summary(id).as_deref(),
                Some(state),
                "閉じた行も現在の状態を読める"
            );
            assert_eq!(h.state().document().to_json(), before);
            h.state_mut().live_disconnect();
            drop(h);
        }
    }
}
