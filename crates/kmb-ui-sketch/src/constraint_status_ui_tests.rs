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
                json!({"target":"Status fixture#1","button":"double"}),
            );
            app(
                &mut h,
                &dir.0,
                "ui_expect",
                json!({"target":band,"state":"exists"}),
            );
            app(&mut h, &dir.0, "ui_key", json!({"keys":"Escape"}));
            app(&mut h, &dir.0, "ui_key", json!({"keys":"Escape"}));
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
