//! 確かめること: 測定の仕事を待たずに閉じ、中止し、選び替えた後の結果を守る。

use crate::*;
use egui_kittest::Harness;
use kmb_core::{
    EvalCtx, EvalError, Feature, InputRef, Kernel, KernelError, MassProps, Mesh, Modules, Outputs,
    ShapeHandle, Value as Output,
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const BODY: &str = "test.measure_body@1";
struct Body(u64);
impl Feature for Body {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        Ok(vec![])
    }
    fn evaluate(&self, _: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        Ok([("body".into(), Output::Shape(ShapeHandle(self.0)))].into())
    }
}
#[derive(Default)]
struct Gate {
    armed: AtomicBool,
    started: AtomicBool,
    released: Mutex<bool>,
    wake: Condvar,
    mass_one: AtomicUsize,
}
impl Gate {
    fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.wake.notify_all();
    }
}
struct Release(Arc<Gate>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}
struct ReadingKernel {
    gate: Arc<Gate>,
    ui_thread: std::thread::ThreadId,
}
impl Kernel for ReadingKernel {
    fn name(&self) -> &str {
        "measurement fixture"
    }
    fn body_kind(&self, h: ShapeHandle) -> Result<kmb_core::kernel::BodyKind, KernelError> {
        if h.0 == 1
            && std::thread::current().id() != self.ui_thread
            && self.gate.armed.swap(false, Ordering::SeqCst)
        {
            self.gate.started.store(true, Ordering::SeqCst);
            let (_lock, timeout) = self
                .gate
                .wake
                .wait_timeout_while(
                    self.gate.released.lock().unwrap(),
                    Duration::from_secs(5),
                    |done| !*done,
                )
                .unwrap();
            assert!(!timeout.timed_out(), "測定用カーネルの合図の時間切れ");
        }
        Ok(kmb_core::kernel::BodyKind::Solid)
    }
    fn mass(&self, h: ShapeHandle) -> Result<MassProps, KernelError> {
        if h.0 == 1 && std::thread::current().id() != self.ui_thread {
            self.gate.mass_one.fetch_add(1, Ordering::SeqCst);
        }
        let x = (h.0 - 1) as f64 * 4.0;
        let height = h.0 as f64;
        Ok(MassProps {
            volume: 6.0 * height,
            area: 12.0 + 10.0 * height,
            min: [x, 0.0, 0.0],
            max: [x + 2.0, 3.0, height],
            centroid: Some([x + 1.0, 1.5, height / 2.0]),
        })
    }
    fn bounds(&self, h: ShapeHandle) -> Result<([f64; 3], [f64; 3]), KernelError> {
        let p = self.mass(h)?;
        Ok((p.min, p.max))
    }
    fn mesh(&self, h: ShapeHandle, _: f64) -> Result<Mesh, KernelError> {
        let x = (h.0 - 1) as f32 * 4.0;
        let z = h.0 as f32;
        Ok(Mesh {
            positions: vec![
                [x, 0.0, z],
                [x + 2.0, 0.0, z],
                [x + 2.0, 3.0, z],
                [x, 3.0, z],
            ],
            normals: vec![[0.0, 0.0, 1.0]; 4],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            triangle_faces: vec![0, 0],
            face_names: vec![format!("body{}", h.0)],
            edges: vec![],
        })
    }
}
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn call(h: &mut Harness<'static, Workspace>, dir: &Path, tool: &str, args: Value) {
    let (dir, tool) = (dir.to_owned(), tool.to_owned());
    let worker = std::thread::spawn(move || kmb_session::live::call(&dir, &tool, args));
    let began = Instant::now();
    while !worker.is_finished() {
        h.step();
        std::thread::sleep(Duration::from_millis(2));
        assert!(
            began.elapsed() < Duration::from_secs(3),
            "画面の返事の時間切れ"
        );
    }
    let reply = worker.join().unwrap().unwrap();
    assert_eq!(reply["ok"], true, "{reply}");
    h.run_steps(4);
}
fn exercise(mode: &str) {
    let gate = Arc::new(Gate::default());
    let _release = Release(gate.clone());
    let mut reg = kmb_core::Registry::new();
    reg.register_described(kmb_cad::solid(BODY, "測る体"), |d| {
        Ok(Box::new(Body(d["shape"].as_u64().unwrap())))
    });
    let modules = Modules::new(
        reg,
        ReadingKernel {
            gate: gate.clone(),
            ui_thread: std::thread::current().id(),
        },
    )
    .with_extension(kmb_cad::InspectCommands);
    let mut b = kmb_samples::sample::DocBuilder::new();
    b.feature(BODY, "一つ目", json!({"shape":1}));
    b.feature(BODY, "二つ目", json!({"shape":2}));
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir =
        Scratch(std::env::temp_dir().join(format!("kmb-mass-live-{}-{nonce}", std::process::id())));
    std::fs::create_dir_all(&dir.0).unwrap();
    let mut ws =
        Workspace::open(b.finish("測定の仕事"), None, true, modules).with_data_dir(dir.0.clone());
    ws.wait();
    assert_eq!(ws.cur.work.report().error_count(), 0);
    ws.layout_mut().ai_live = true;
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    let data = h.state().document().data().clone();
    let depth = h.state().document().undo_depth();
    call(&mut h, &dir.0, "ui_pick", json!({"face":"body1"}));
    gate.armed.store(true, Ordering::SeqCst);
    call(&mut h, &dir.0, "ui_key", json!({"keys":"I"}));
    let began = Instant::now();
    while !gate.started.load(Ordering::SeqCst) {
        h.step();
        std::thread::sleep(Duration::from_millis(2));
        assert!(
            began.elapsed() < Duration::from_secs(2),
            "測定がHeadlessの背景の仕事へ渡っていません"
        );
    }
    match mode {
        "close" => {
            call(&mut h, &dir.0, "ui_key", json!({"keys":"I"}));
            assert!(!h.state().is_measuring());
        }
        "cancel" => {
            call(&mut h, &dir.0, "ui_click", json!({"target":"計算を中止"}));
            assert!(
                h.state()
                    .cur
                    .measure
                    .as_ref()
                    .unwrap()
                    .report
                    .rows
                    .is_empty()
            );
        }
        "replace" => {
            call(&mut h, &dir.0, "ui_click", json!({"target":"選択をクリア"}));
            call(&mut h, &dir.0, "ui_pick", json!({"face":"body2"}));
        }
        _ => panic!("知らない試験の流れ"),
    }
    gate.release();
    let began = Instant::now();
    while h.state().cur.work.is_recomputing() {
        h.step();
        std::thread::sleep(Duration::from_millis(2));
        assert!(
            began.elapsed() < Duration::from_secs(3),
            "中止後の仕事の時間切れ"
        );
    }
    h.run_steps(12);
    assert_eq!(
        gate.mass_one.load(Ordering::SeqCst),
        0,
        "中止後は次のカーネルの計算へ進まない"
    );
    if mode == "replace" {
        let m = h.state().cur.measure.as_ref().unwrap();
        let volume = &m
            .report
            .rows
            .iter()
            .find(|(k, _)| k == "ボディの体積")
            .expect("選び替えた体の結果")
            .1;
        assert!(volume.starts_with("12"), "{volume}");
    }
    assert_eq!(h.state().document().data(), &data);
    assert_eq!(h.state().document().undo_depth(), depth);
}
#[test]
fn mass_progress_live_close_stops_the_next_call() {
    exercise("close");
}
#[test]
fn mass_progress_live_cancel_clears_the_table() {
    exercise("cancel");
}
#[test]
fn mass_progress_live_replacing_a_face_rejects_the_old_result() {
    exercise("replace");
}
