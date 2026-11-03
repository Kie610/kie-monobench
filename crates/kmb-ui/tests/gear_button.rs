//! 確かめること: 設計の「作成 ▾」の「歯車」を押し、平らな面 → 中心の順にクリックすると、歯車の輪郭と
//! ボディが 1 回の取り消し単位でできる。足した後の編集パネルは輪郭の欄(種類・歯数…)を出し、歯数を
//! 打って OK で決めると形が作り直される。「歯車の輪郭」は輪郭だけを足し、Esc なら何も足さない。
//! 操作は MCP の ui_* の道具だけで行う。

use egui_kittest::Harness;
use kmb_cad_extrude::{EXTRUDE, Extrude};
use kmb_samples::sample::{DocBuilder, SketchBuilder};
use kmb_sketch::SketchPlane;
use kmb_ui::Workspace;
use kmb_ui_sketch::SketchEditor;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "kmb-gear-button-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn call(h: &mut Harness<'static, Workspace>, dir: &Path, tool: &str, args: Value) -> Value {
    call_within(h, dir, tool, args, 30)
}

/// `call` の時間の上限を `secs` 秒にしたもの。
fn call_within(
    h: &mut Harness<'static, Workspace>,
    dir: &Path,
    tool: &str,
    args: Value,
    secs: u64,
) -> Value {
    let dir = dir.to_owned();
    let tool = tool.to_string();
    let worker = std::thread::spawn(move || kmb_session::live::call(&dir, &tool, args));
    let began = Instant::now();
    while !worker.is_finished() {
        h.step();
        std::thread::sleep(Duration::from_millis(2));
        assert!(
            began.elapsed() < Duration::from_secs(secs),
            "MCP の答えが届きません"
        );
    }
    let reply = worker.join().unwrap().expect("MCP の要求に答える");
    assert_eq!(reply["ok"], true, "{reply}");
    while h.state().is_busy() {
        h.step();
        std::thread::sleep(Duration::from_millis(2));
        assert!(
            began.elapsed() < Duration::from_secs(secs),
            "再計算が終わりません"
        );
    }
    h.run_steps(8);
    reply
}

/// 40 × 40 × 5 mm の板 1 枚の文書。板の上の面の名前も返す。
fn fixture(dir: &Path) -> (Harness<'static, Workspace>, String) {
    let mut builder = DocBuilder::new();
    let mut sketch = SketchBuilder::new(SketchPlane::Xy);
    let origin = sketch.fixed_point(-20.0, -20.0);
    sketch.rectangle(origin, "40 mm", "40 mm", [-20.0, -20.0, 20.0, 20.0]);
    let profile = builder.feature(kmb_sketch::SKETCH, "板のスケッチ", sketch.build());
    let plate = builder.feature(
        EXTRUDE,
        "板",
        Extrude {
            profile: Some(profile),
            distance: "5 mm".into(),
            ..Default::default()
        },
    );
    let mut doc = builder.finish("歯車の見本");
    doc.clear_history();
    let mut ws = Workspace::new(doc, None, kmb_bundle::modules())
        .with_editor(SketchEditor::new())
        .with_data_dir(dir.to_owned());
    ws.layout_mut().ai_live = true;
    ws.layout_mut().appearance.language = "ja".into();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1600.0, 1000.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run_steps(8);
    assert!(h.state().live_port().is_some());
    assert_eq!(h.state().report().error_count(), 0);
    (h, format!("f{}:end", plate.0))
}

fn types(h: &Harness<'static, Workspace>) -> Vec<String> {
    h.state()
        .document()
        .data()
        .features
        .iter()
        .map(|f| f.type_id.clone())
        .collect()
}

/// 「作成 ▾」の `button` を押し、面 → 中心の順にクリックする。
///
/// 歯車のボディは辺が多く、できた直後に画面が辺を数え直す(手元の debug で十数秒)。そのため
/// ボディを作る最後のクリックだけ時間の上限を広げる。輪郭だけならすぐ終わる。
fn place_gear(h: &mut Harness<'static, Workspace>, dir: &Path, button: &str, face: &str) {
    call(h, dir, "ui_click", json!({"target": "作成 ▾"}));
    call(h, dir, "ui_click", json!({"target": button}));
    call(h, dir, "ui_pick", json!({"face": face}));
    call_within(h, dir, "ui_pick", json!({"face": face}), 150);
}

#[test]
fn the_gear_button_places_an_outline_and_a_body_edited_in_the_panel() {
    let dir = Scratch::new();
    let (mut h, face) = fixture(&dir.0);
    let before = types(&h);
    place_gear(&mut h, &dir.0, "歯車", &face);
    let after = types(&h);
    assert_eq!(
        after[before.len()..],
        [kmb_cad_gear::GEAR, kmb_cad_gear::GEAR_BODY],
        "輪郭とボディを足す"
    );
    let gear = h.state().document().data().features[before.len()].id;
    assert!(h.state().is_editing(gear), "編集パネルは輪郭の欄を出す");
    call(
        &mut h,
        &dir.0,
        "ui_expect",
        json!({"target": "種類", "state": "exists"}),
    );
    call_within(
        &mut h,
        &dir.0,
        "ui_type",
        json!({"target": "歯数", "text": "16", "commit": "tab"}),
        150,
    );
    call(
        &mut h,
        &dir.0,
        "ui_click",
        json!({"target": "button:OKEnter"}),
    );
    assert!(!h.state().is_editing(gear), "OK で決める");
    assert_eq!(h.state().report().error_count(), 0);
    let data = h.state().document().data();
    let g = &data.feature(gear).unwrap().data;
    assert_eq!(g["teeth"], "16");
    assert_eq!(g["plane"]["kind"], "face", "{g}");
    // 取り消しは、欄の直し → 輪郭とボディを足したことの順に 1 回ずつ戻る(足した 2 つは一緒に消える)
    call(&mut h, &dir.0, "ui_key", json!({"keys": "Ctrl+Z"}));
    assert_eq!(types(&h), after);
    call(&mut h, &dir.0, "ui_key", json!({"keys": "Ctrl+Z"}));
    assert_eq!(types(&h), before);
    h.state_mut().live_disconnect();
}

#[test]
fn escape_while_placing_a_gear_outline_adds_nothing() {
    // ボディを作らない「歯車の輪郭」で確かめる(同じ手順で、重いボディの計算を待たない)
    let dir = Scratch::new();
    let (mut h, face) = fixture(&dir.0);
    let before = types(&h);
    place_gear(&mut h, &dir.0, "歯車の輪郭", &face);
    assert_eq!(
        types(&h)[before.len()..],
        [kmb_cad_gear::GEAR],
        "輪郭だけを足す"
    );
    call(&mut h, &dir.0, "ui_key", json!({"keys": "Escape"}));
    assert_eq!(types(&h), before, "作っている途中を Esc でやめると元に戻る");
    h.state_mut().live_disconnect();
}
