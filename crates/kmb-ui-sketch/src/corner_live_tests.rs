//! 確かめること: 動いている画面への台本で角を囲み、値を一度打って丸め、取り消せる。

use crate::tests::*;
use egui_kittest::{Harness, kittest::Queryable};
use kmb_core::Document;
use kmb_sketch::Entity;
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
fn live_corner_box_fillet_and_undo_use_ui_tools() {
    let dir = Scratch(std::env::temp_dir().join(format!(
            "kmb-corner-live-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    std::fs::create_dir_all(&dir.0).unwrap();
    let mut ws = ws_of(Document::new(), None, reg()).with_data_dir(dir.0.clone());
    ws.layout_mut().ai_live = true;
    ws.layout_mut().commands_open = true;
    ws.layout_mut().appearance.language = "ja".into();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h.state_mut().run_action(kmb_sketch::ADD_SKETCH);
    h.run();
    editor(h.state_mut()).set_tool(crate::Tool::Rectangle);
    h.run();
    let rect = editor(h.state_mut()).view.seen.as_ref().unwrap().1;
    let t = editor(h.state_mut()).view.last.clone().unwrap();
    let a = t.to_world(rect.left_top() + egui::vec2(45.0, 45.0));
    let b = t.to_world(rect.left_top() + egui::vec2(225.0, 185.0));
    click_at(&mut h, a);
    click_at(&mut h, b);
    press_key(&mut h, egui::Key::Escape);
    let before = current_sketch(h.state());
    assert_eq!(count_of(&before, is_line), 4);
    // 台本は名前のある図の中央から左上へ引き、角4つと線4本を囲む。
    let dx = rect.left() + 10.0 - rect.center().x;
    let dy = rect.top() + 10.0 - rect.center().y;
    call(
        &mut h,
        &dir.0,
        "ui_type",
        json!({"target":"コマンド", "text":format!("drag \"スケッチの図\" {dx} {dy}"), "commit":"enter"}),
    );
    assert_eq!(editor(h.state_mut()).editor.selection.len(), 8);
    call(&mut h, &dir.0, "ui_click", json!({"target":"フィレット"}));
    call(
        &mut h,
        &dir.0,
        "ui_type",
        json!({"target":"寸法の値", "text":"2", "commit":"enter"}),
    );
    assert_eq!(
        count_of(&current_sketch(h.state()), |e| matches!(
            e,
            Entity::Arc { .. }
        )),
        4
    );
    call(&mut h, &dir.0, "ui_click", json!({"target":"元に戻す"}));
    assert_eq!(current_sketch(h.state()), before);
    h.get_by_label("フィレット");
}
