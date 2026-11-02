//! 確かめること: 動いている画面の道具のボタンを台本で2回押すと、描きかけを捨てて選ぶ。

use crate::tests::*;
use egui_kittest::{Harness, kittest::Queryable};
use kmb_core::Document;
use kmb_ui::Workspace;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn call(h: &mut Harness<'static, Workspace>, dir: &Path, tool: &str, args: Value) -> Value {
    let dir = dir.to_owned();
    let tool = tool.to_owned();
    let worker = std::thread::spawn(move || kmb_session::live::call(&dir, &tool, args));
    let began = Instant::now();
    while !worker.is_finished() {
        h.step();
        std::thread::sleep(Duration::from_millis(2));
        assert!(
            began.elapsed() < Duration::from_secs(30),
            "画面の答えの時間切れ"
        );
    }
    let reply = worker.join().unwrap().expect("画面から答えが届く");
    assert_eq!(reply["ok"], true, "{reply}");
    while h.state().is_busy() {
        h.step();
        std::thread::sleep(Duration::from_millis(2));
        assert!(
            began.elapsed() < Duration::from_secs(30),
            "再計算の時間切れ"
        );
    }
    h.run_steps(8);
    reply
}

#[test]
fn live_active_tool_button_returns_to_selection_through_ui_tools() {
    let dir = Scratch(std::env::temp_dir().join(format!(
            "kmb-tool-button-live-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    std::fs::create_dir_all(&dir.0).unwrap();
    let mut ws = ws_of(Document::new(), None, reg()).with_data_dir(dir.0.clone());
    ws.layout_mut().ai_live = true;
    ws.layout_mut().appearance.language = "ja".into();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h.state_mut().run_action(kmb_sketch::ADD_SKETCH);
    h.run();
    assert_eq!(editor(h.state_mut()).editor.tool, crate::Tool::Select);
    let before = current_sketch(h.state());
    call(&mut h, &dir.0, "ui_click", json!({"target":"線分"}));
    assert_eq!(editor(h.state_mut()).editor.tool, crate::Tool::Line);
    call(&mut h, &dir.0, "ui_click", json!({"target":"スケッチの図"}));
    assert!(editor(h.state_mut()).editor.pending.is_some());
    call(&mut h, &dir.0, "ui_click", json!({"target":"線分"}));
    assert_eq!(editor(h.state_mut()).editor.tool, crate::Tool::Select);
    assert!(editor(h.state_mut()).editor.pending.is_none());
    assert_eq!(current_sketch(h.state()), before);
    call(&mut h, &dir.0, "ui_click", json!({"target":"線分"}));
    call(&mut h, &dir.0, "ui_click", json!({"target":"長方形"}));
    assert_eq!(editor(h.state_mut()).editor.tool, crate::Tool::Rectangle);
    call(&mut h, &dir.0, "ui_click", json!({"target":"長方形"}));
    assert_eq!(editor(h.state_mut()).editor.tool, crate::Tool::Select);
    h.get_by_label("線分");
}
