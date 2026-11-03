use super::*;
use crate::result::{Frame, Part, Quantity, Values};
use crate::run::{Control, Task};
use kmb_core::{
    Descriptor, Document, Extensions, FeatureEntry, NullKernel, Op, RecomputeReport, Registry,
    Transaction,
};
use serde_json::json;

struct ResultTask(SimResult);
impl Task for ResultTask {
    fn solve(&self, _: &Control) -> Result<SimResult, String> {
        Ok(self.0.clone())
    }
}

fn result() -> SimResult {
    SimResult {
        kind: "modal".into(),
        parts: vec![Part {
            name: "形".into(),
            geom: Geometry::Surface {
                points: vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]],
                tris: vec![[0, 1, 2]],
            },
        }],
        quantities: vec![Quantity::vector("揺れ", "相対", 0, Role::Deform)],
        frames: (0..2)
            .map(|i| Frame {
                label: format!("形{i}"),
                time: None,
                values: vec![Values::exact(vec![
                    0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0,
                ])],
            })
            .collect(),
        ..SimResult::default()
    }
}

#[test]
fn phase_seek_checks_the_result_address_and_declares_its_coefficient() {
    let mut doc = Document::default();
    let fid = doc.alloc_feature_id();
    doc.apply(Transaction::new(
        "研究",
        vec![Op::InsertFeature {
            index: None,
            entry: FeatureEntry {
                id: fid,
                type_id: "study.test@1".into(),
                name: "試験".into(),
                data: json!({}),
                suppressed: false,
                pin_bodies: false,
            },
        }],
    ))
    .unwrap();
    let mut reg = Registry::new();
    reg.register_described(
        Descriptor::new("study.test@1", kmb_core::category::STUDY, "試験"),
        |_| Err(kmb_core::EvalError::Other("試験では評価しません".into())),
    );
    let report = RecomputeReport::default();
    let kernel: Arc<dyn kmb_core::Kernel> = Arc::new(NullKernel);
    let cx = Context {
        selected: Some(fid),
        doc: 9,
        ..Context::new(doc.data(), &report, &reg, &kernel)
    };
    let commands = Arc::new(StudyCommands::default());
    commands
        .runner
        .solve_now((9, fid.0), 42, &ResultTask(result()))
        .unwrap();
    let ext = Extensions(vec![commands.clone()]);
    let bar = commands.frames(&cx).unwrap();
    assert!(bar.playable);
    assert_eq!(bar.fps, 30.0);
    let id = format!("{}0", bar.seek);
    assert_eq!(commands.unlisted(&id).unwrap()[0].key, "phase");
    let run = |id: &str, args: &serde_json::Value| ext.run(id, &Context { args, ..cx });
    run(&id, &json!({"phase": -0.5})).unwrap();
    let r = commands.result(&cx, fid).unwrap().0;
    assert_eq!(commands.look(&cx, fid, &r, false).phase, -0.5);
    commands.shape(&cx, fid).unwrap();
    let cache_key = || commands.phase_drawn.lock().unwrap().as_ref().unwrap().key;
    let first_key = cache_key();
    assert!(
        commands
            .phase_drawn
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .mesh
            .is_some()
    );
    run(&id, &json!({"phase": -0.25})).unwrap();
    commands.shape(&cx, fid).unwrap();
    assert_eq!(cache_key(), first_key);
    assert_eq!(commands.frames(&cx).unwrap().at, 0);
    assert!(run(&id, &json!({"phase": 1.1})).is_err());
    assert!(run(&id, &json!({"other": 0})).is_err());
    run(&id, &json!({})).unwrap();
    assert_eq!(commands.look(&cx, fid, &r, false).phase, 1.0);
    run(&id, &json!({"phase": 0.0})).unwrap();
    run(&format!("{VIEW_FRAME}0"), &json!({})).unwrap();
    assert_eq!(commands.look(&cx, fid, &r, false).phase, 1.0);
    run(&format!("{VIEW_FRAME}1"), &json!({})).unwrap();
    commands.shape(&cx, fid).unwrap();
    let mode_key = cache_key();
    assert_ne!(mode_key, first_key);
    run(VIEW_SMOOTH, &json!({})).unwrap();
    commands.shape(&cx, fid).unwrap();
    assert_ne!(cache_key(), mode_key);
    commands
        .runner
        .solve_now((9, fid.0), 43, &ResultTask(result()))
        .unwrap();
    assert_ne!(commands.frames(&cx).unwrap().seek, bar.seek);
    assert!(
        run(&id, &json!({"phase": 0.0}))
            .unwrap_err()
            .to_string()
            .contains("結果が変わりました")
    );
    let mut changed = result();
    changed.summary = "新しい結果".into();
    let before_stamp = commands.frames(&cx).unwrap().seek;
    commands
        .runner
        .solve_now((9, fid.0), 43, &ResultTask(changed))
        .unwrap();
    assert_ne!(commands.frames(&cx).unwrap().seek, before_stamp);
    let previous_key = cache_key();
    commands.shape(&cx, fid).unwrap();
    assert_ne!(cache_key(), previous_key);
    let mut timed = result();
    timed.kind = "thermal".into();
    for (i, frame) in timed.frames.iter_mut().enumerate() {
        frame.time = Some(i as f64);
    }
    commands
        .runner
        .solve_now((9, fid.0), 44, &ResultTask(timed))
        .unwrap();
    let bar = commands.frames(&cx).unwrap();
    assert!(bar.playable);
    assert_eq!(bar.fps, 12.0);
    assert_eq!(bar.seek, VIEW_FRAME);
    commands.shape(&cx, fid).unwrap();
    assert!(commands.phase_drawn.lock().unwrap().is_none());
}

#[test]
fn a_single_mode_can_play_but_a_static_frame_cannot() {
    let mut r = result();
    r.frames.truncate(1);
    assert!(has_phase(&r));
    r.kind = "static".into();
    assert!(!has_phase(&r));
    assert!(PhaseSeek::parse("study.view.phase.2.3.4.0").is_some());
    assert!(PhaseSeek::parse("study.view.phase.2.3.4.0.extra").is_none());
    assert!(PhaseSeek::parse("study.view.phase.2.3.-1.0").is_none());
}

#[test]
fn a_new_job_moves_the_phase_cache_even_when_the_result_stamp_stays_equal() {
    let mut doc = Document::new();
    let fid = doc.alloc_feature_id();
    doc.apply(Transaction::new(
        "研究",
        vec![Op::InsertFeature {
            index: None,
            entry: FeatureEntry {
                id: fid,
                type_id: "study.test@1".into(),
                name: "試験".into(),
                data: json!({}),
                suppressed: false,
                pin_bodies: false,
            },
        }],
    ))
    .unwrap();
    let mut reg = Registry::new();
    reg.register_described(
        Descriptor::new("study.test@1", kmb_core::category::STUDY, "試験"),
        |_| Err(kmb_core::EvalError::Other("試験では評価しません".into())),
    );
    let report = RecomputeReport::default();
    let kernel: Arc<dyn kmb_core::Kernel> = Arc::new(NullKernel);
    let cx = Context {
        selected: Some(fid),
        doc: 9,
        ..Context::new(doc.data(), &report, &reg, &kernel)
    };
    let commands = Arc::new(StudyCommands::default());
    let ext = Extensions(vec![commands.clone()]);
    let original = result();
    commands
        .runner
        .solve_now((9, fid.0), 42, &ResultTask(original.clone()))
        .unwrap();
    let seek = format!("{}0", commands.frames(&cx).unwrap().seek);
    let args = json!({"phase": 0.25});
    ext.run(&seek, &Context { args: &args, ..cx }).unwrap();
    let before = commands.shape(&cx, fid).unwrap();
    let mut moved = original.clone();
    let Geometry::Surface { points, .. } = &mut moved.parts[0].geom else {
        panic!("表面")
    };
    for p in points {
        p[0] += 10.0;
    }
    // 内容の指紋だけでは、点数と変位が同じ平行移動を区別できない。
    assert_eq!(moved.stamp(), original.stamp());
    commands
        .runner
        .solve_now((9, fid.0), 43, &ResultTask(moved.clone()))
        .unwrap();
    let seek = format!("{}0", commands.frames(&cx).unwrap().seek);
    ext.run(&seek, &Context { args: &args, ..cx }).unwrap();
    let after = commands.shape(&cx, fid).unwrap();
    assert_ne!(
        after.key, before.key,
        "新しい仕事では同じ位相でも描画の鍵が変わる"
    );
    for (a, b) in after.mesh.positions.iter().zip(&before.mesh.positions) {
        assert_eq!(a[0], b[0] + 10.0);
    }
    let args = json!({"phase": 0.5});
    ext.run(&seek, &Context { args: &args, ..cx }).unwrap();
    let advanced = commands.shape(&cx, fid).unwrap();
    let wanted = view::draw(&moved, &commands.look(&cx, fid, &moved, true), advanced.key);
    assert_eq!(
        advanced.mesh, wanted.mesh,
        "位相を進めても新しい位置の網を使う"
    );
    assert_ne!(advanced.key, after.key);
}
