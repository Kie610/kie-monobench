//! 確かめること: 別の部品のスケッチの開閉で、次のボディの入れ先を変えない。

use egui_kittest::Harness;
use kmb_cad_extrude::{EXTRUDE, Extrude};
use kmb_core::{Component, Edit, FeatureId};
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
            "kmb-active-sketch-{}-{}",
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
    let dir = dir.to_owned();
    let tool = tool.to_string();
    let worker = std::thread::spawn(move || kmb_session::live::call(&dir, &tool, args));
    let began = Instant::now();
    while !worker.is_finished() {
        h.step();
        std::thread::sleep(Duration::from_millis(2));
        assert!(
            began.elapsed() < Duration::from_secs(30),
            "MCP の答えが届きません"
        );
    }
    let reply = worker.join().unwrap().expect("MCP の要求に答える");
    assert_eq!(reply["ok"], true, "{reply}");
    while h.state().is_busy() {
        h.step();
        std::thread::sleep(Duration::from_millis(2));
        assert!(
            began.elapsed() < Duration::from_secs(30),
            "再計算が終わりません"
        );
    }
    h.run_steps(8);
    reply
}

fn fixture(dir: &Path) -> (Harness<'static, Workspace>, u64, u64, FeatureId) {
    let mut builder = DocBuilder::new();
    let mut bodies = Vec::new();
    let mut sketches = Vec::new();
    for (x, name) in [(0.0, "A"), (40.0, "B")] {
        let mut sketch = SketchBuilder::new(SketchPlane::Xy);
        let origin = sketch.fixed_point(x, 0.0);
        sketch.rectangle(origin, "20 mm", "15 mm", [x, 0.0, x + 20.0, 15.0]);
        let profile = builder.feature(
            kmb_sketch::SKETCH,
            &format!("{name}のスケッチ"),
            sketch.build(),
        );
        sketches.push(profile);
        bodies.push(builder.feature(
            EXTRUDE,
            &format!("{name}のボディ"),
            Extrude {
                profile: Some(profile),
                distance: "3 mm".into(),
                ..Default::default()
            },
        ));
    }
    let mut doc = builder.finish("部品の見本");
    doc.apply_edits(
        "試験の部品",
        vec![Edit::SetComponents {
            components: vec![
                Component {
                    id: 0,
                    name: "部品A".into(),
                    parent: None,
                    bodies: vec![bodies[0]],
                },
                Component {
                    id: 0,
                    name: "部品B".into(),
                    parent: None,
                    bodies: vec![bodies[1]],
                },
            ],
        }],
    )
    .unwrap();
    doc.clear_history();
    let a = doc.data().components[0].id;
    let b = doc.data().components[1].id;
    let mut ws = Workspace::new(doc, None, kmb_bundle::modules())
        .with_editor(SketchEditor::new())
        .with_data_dir(dir.to_owned());
    ws.layout_mut().ai_live = true;
    ws.layout_mut().commands_open = true;
    ws.layout_mut().appearance.language = "ja".into();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1600.0, 1000.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run_steps(8);
    assert!(h.state().live_port().is_some());
    assert_eq!(h.state().report().error_count(), 0);
    (h, a, b, sketches[1])
}

fn check_destination(active_b: bool) {
    let dir = Scratch::new();
    let (mut h, a, b, sketch) = fixture(&dir.0);
    let (active, name) = if active_b {
        (b, "部品B")
    } else {
        (a, "部品A")
    };
    call(
        &mut h,
        &dir.0,
        "ui_click",
        json!({"target":format!("{name} をアクティブにする")}),
    );
    assert_eq!(h.state().active_component(), Some(active));
    call(
        &mut h,
        &dir.0,
        "ui_click",
        json!({"target":"Bのスケッチ#1"}),
    );
    call(&mut h, &dir.0, "ui_key", json!({"keys":"Return"}));
    assert!(h.state().is_editing(sketch));
    assert_eq!(h.state().active_component(), Some(active));
    call(
        &mut h,
        &dir.0,
        "ui_click",
        json!({"target":"button:~スケッチを終了#1"}),
    );
    assert!(!h.state().is_editing(sketch));
    assert_eq!(h.state().active_component(), Some(active));

    let before = h.state().document().data().features.len();
    call(
        &mut h,
        &dir.0,
        "ui_type",
        json!({"target":"コマンド", "text":"run cad.extrude distance=5", "commit":"enter"}),
    );
    assert_eq!(h.state().document().data().features.len(), before + 1);
    let added = h.state().document().data().features.last().unwrap().id;
    assert_eq!(
        h.state().document().data().feature(added).unwrap().type_id,
        EXTRUDE
    );
    if !h.state().is_editing(added) {
        call(&mut h, &dir.0, "ui_key", json!({"keys":"Return"}));
    }
    call(
        &mut h,
        &dir.0,
        "ui_select",
        json!({"target":"演算", "option":"新規ボディ"}),
    );
    call(&mut h, &dir.0, "ui_key", json!({"keys":"Return"}));
    assert_eq!(h.state().report().error_count(), 0);
    assert_eq!(h.state().active_component(), Some(active));
    let data = h.state().document().data();
    assert_eq!(data.features.len(), before + 1);
    assert_eq!(data.feature(added).unwrap().data["operation"], "new");
    assert_eq!(data.component_of(added).map(|c| c.id), Some(active));
    let other = if active_b { a } else { b };
    assert!(!data.component(other).unwrap().bodies.contains(&added));
    h.state_mut().live_disconnect();
}

#[test]
fn editing_another_parts_sketch_keeps_the_active_part_for_the_next_body() {
    check_destination(false);
}

#[test]
fn editing_the_active_parts_sketch_keeps_new_bodies_in_that_part() {
    check_destination(true);
}
