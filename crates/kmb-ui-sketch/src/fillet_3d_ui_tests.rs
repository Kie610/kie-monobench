//! 確かめること: 3D の編集中に 2D のフィレットを呼べず、名前の操作は失敗を返す。
use crate::{Sketch3dEditor, SketchEditor, Tool, Tool3};
use egui_kittest::Harness;
use kmb_core::{Document, FeatureEntry, Op, Transaction};
use kmb_ui::{CommandId, Workspace};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "kmb-fillet-3d-{}-{}",
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
            began.elapsed() < Duration::from_secs(20),
            "MCP の答えが届きません"
        );
    }
    let reply = worker.join().unwrap().expect("MCP の要求に答える");
    h.run_steps(8);
    reply
}

fn fixture(three: bool, dir: &Path) -> Harness<'static, Workspace> {
    let (type_id, data) = if three {
        let mut s = kmb_sketch_3d::Sketch3d::default();
        let a = s.add_point([0., 0., 0.]);
        let b = s.add_point([20., 0., 0.]);
        let c = s.add_point([20., 20., 0.]);
        s.add_line(a, b);
        s.add_line(b, c);
        (kmb_sketch_3d::SKETCH3D, serde_json::to_value(s).unwrap())
    } else {
        let mut s = kmb_sketch::Sketch::default();
        let a = s.add_point(0., 0.);
        let b = s.add_point(20., 0.);
        let c = s.add_point(20., 20.);
        s.add_line(a, b);
        s.add_line(b, c);
        (kmb_sketch::SKETCH, serde_json::to_value(s).unwrap())
    };
    let mut doc = Document::new();
    let id = doc.alloc_feature_id();
    doc.apply(Transaction::new(
        "試験の図",
        vec![Op::InsertFeature {
            index: None,
            entry: FeatureEntry {
                id,
                type_id: type_id.into(),
                name: "フィレットの見本".into(),
                data,
                suppressed: false,
                pin_bodies: false,
            },
        }],
    ))
    .unwrap();
    let mut ws = Workspace::new(doc, None, crate::tests::reg())
        .with_editor(SketchEditor::new())
        .with_editor(Sketch3dEditor::new())
        .with_data_dir(dir.to_owned());
    ws.layout_mut().ai_live = true;
    ws.layout_mut().appearance.language = "ja".into();
    ws.select(id);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400., 900.))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run_steps(8);
    assert!(h.state().live_port().is_some());
    assert_eq!(
        call(&mut h, dir, "ui_key", json!({"keys":"Return"}))["ok"],
        true
    );
    assert!(h.state().is_editing(id));
    h
}

#[test]
fn a_3d_sketch_does_not_expose_a_silent_2d_fillet_entry() {
    let dir = Scratch::new();
    let mut h = fixture(true, &dir.0);
    let before = h.state().document().to_json();
    assert!(
        !h.state()
            .commands()
            .iter()
            .any(|c| c.id == CommandId::EditorTool("tool:Fillet".into()))
    );
    assert!(
        h.state()
            .key_of(&CommandId::EditorTool("tool:Fillet".into()))
            .is_empty()
    );
    assert!(
        kmb_ui::keys::current()
            .chords_of("sketch_fillet")
            .is_empty()
    );
    assert_eq!(
        call(
            &mut h,
            &dir.0,
            "ui_expect",
            json!({"target":"フィレット","state":"missing"})
        )["ok"],
        true
    );
    for (tool, args) in [
        ("ui_click", json!({"target":"フィレット"})),
        ("command", json!({"line":"click フィレット"})),
        ("script", json!({"text":"click フィレット"})),
    ] {
        let reply = call(&mut h, &dir.0, tool, args);
        assert_eq!(reply["ok"], false, "{tool}: {reply}");
        assert!(reply.to_string().contains("フィレット"), "{tool}: {reply}");
        assert_eq!(h.state().document().to_json(), before);
        assert_eq!(
            h.state_mut().editor_mut::<Sketch3dEditor>().unwrap().tool,
            Tool3::Select
        );
    }
    h.state_mut().live_disconnect();
}

#[test]
fn a_2d_sketch_can_start_a_fillet_from_the_named_button() {
    let dir = Scratch::new();
    let mut h = fixture(false, &dir.0);
    let reply = call(&mut h, &dir.0, "ui_click", json!({"target":"フィレット"}));
    assert_eq!(reply["ok"], true, "{reply}");
    assert_eq!(
        h.state_mut()
            .editor_mut::<SketchEditor>()
            .unwrap()
            .editor
            .tool,
        Tool::Fillet
    );
    h.state_mut().live_disconnect();
}
