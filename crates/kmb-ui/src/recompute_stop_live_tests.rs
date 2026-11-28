//! 確かめること: 時間のかかる再計算を、下の帯の「中止」と AI の `stop` で止められ、止めた後も文書は
//! 変わらず、「再計算」と AI の `recompute` で計算し直せる(人と同じ入力の台本で)。

use crate::*;
use egui_kittest::Harness;
use kmb_core::{EvalCtx, EvalError, Feature, FeatureStatus, InputRef, Outputs};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const LONG: &str = "test.long_recompute@1";
const QUICK: &str = "test.quick_recompute@1";
const LIMIT: Duration = Duration::from_secs(10);

/// 立っていれば、次に計算する長い機能が止める印を待つ(1 回だけ)。
static ARMED: AtomicBool = AtomicBool::new(false);
/// 長い機能が止める印を待ち始めた。
static STARTED: AtomicBool = AtomicBool::new(false);

/// 止める印を見るまで計算を続ける長い機能(待つのは [`ARMED`] の時だけ)。
struct Long;
impl Feature for Long {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        Ok(vec![])
    }
    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        if !ARMED.swap(false, Ordering::SeqCst) {
            return Ok(Outputs::new());
        }
        STARTED.store(true, Ordering::SeqCst);
        let began = Instant::now();
        while !ctx.stopping() {
            assert!(began.elapsed() < LIMIT, "止める印が届かない");
            std::thread::yield_now();
        }
        Err(EvalError::Other("止めた".into()))
    }
}

struct Quick;
impl Feature for Quick {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        Ok(vec![])
    }
    fn evaluate(&self, _: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        Ok(Outputs::new())
    }
}

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

type Reply = std::thread::JoinHandle<Result<Value, String>>;

/// 画面へ頼みを送り、答えが来るまで画面を回す。
fn ask(h: &mut Harness<'static, Workspace>, dir: &Path, tool: &str, args: Value) -> Value {
    let worker = send(dir, tool, args);
    settle(h, worker)
}

fn send(dir: &Path, tool: &str, args: Value) -> Reply {
    let (dir, tool) = (dir.to_owned(), tool.to_owned());
    std::thread::spawn(move || kmb_session::live::call(&dir, &tool, args))
}

fn settle(h: &mut Harness<'static, Workspace>, worker: Reply) -> Value {
    let began = Instant::now();
    while !worker.is_finished() {
        h.step();
        std::thread::sleep(Duration::from_millis(2));
        assert!(began.elapsed() < LIMIT, "画面の返事の時間切れ");
    }
    let reply = worker.join().unwrap().expect("画面が誤りを返した");
    h.run_steps(4);
    reply
}

/// 長い機能が止める印を待ち始めるまで画面を回す。
fn until_started(h: &mut Harness<'static, Workspace>) {
    let began = Instant::now();
    while !STARTED.swap(false, Ordering::SeqCst) {
        h.step();
        std::thread::sleep(Duration::from_millis(2));
        assert!(began.elapsed() < LIMIT, "長い機能が始まらない");
    }
}

/// 裏の再計算が終わるまで画面を回す。
fn until_idle(h: &mut Harness<'static, Workspace>) {
    let began = Instant::now();
    while h.state().cur.work.is_recomputing() {
        h.step();
        std::thread::sleep(Duration::from_millis(2));
        assert!(began.elapsed() < LIMIT, "止めた再計算が終わらない");
    }
    h.run_steps(4);
}

fn statuses(h: &Harness<'static, Workspace>) -> Vec<&'static str> {
    h.state()
        .cur
        .work
        .report()
        .features
        .iter()
        .map(|f| match f.status {
            FeatureStatus::Ok { .. } => "ok",
            FeatureStatus::Stopped => "stopped",
            _ => "other",
        })
        .collect()
}

#[test]
fn a_long_recompute_is_stopped_from_the_status_bar_and_by_the_ai() {
    let mut modules = crate::tests::reg();
    let reg = Arc::get_mut(&mut modules.registry).expect("作った直後の表");
    reg.register(LONG, |_| Ok(Box::new(Long)));
    reg.register(QUICK, |_| Ok(Box::new(Quick)));
    let mut b = kmb_samples::sample::DocBuilder::new();
    b.feature(QUICK, "前", json!({}));
    b.feature(LONG, "長い計算", json!({}));
    b.feature(QUICK, "後ろ", json!({}));
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = Scratch(
        std::env::temp_dir().join(format!("kmb-recompute-stop-{}-{nonce}", std::process::id())),
    );
    std::fs::create_dir_all(&dir.0).unwrap();
    ARMED.store(true, Ordering::SeqCst);
    let mut ws =
        Workspace::open(b.finish("止める再計算"), None, true, modules).with_data_dir(dir.0.clone());
    ws.layout_mut().ai_live = true;
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    until_started(&mut h);
    let data = h.state().document().data().clone();
    let depth = h.state().document().undo_depth();

    // AI の stop で、開いた時の計算を止める
    let stopped = ask(&mut h, &dir.0, "stop", json!({}));
    assert_eq!(stopped["stopped"], true, "{stopped}");
    until_idle(&mut h);
    assert_eq!(statuses(&h), ["ok", "stopped", "stopped"]);
    assert_eq!(h.state().document().data(), &data, "文書は変えない");
    assert_eq!(h.state().document().undo_depth(), depth);
    let none = ask(&mut h, &dir.0, "stop", json!({}));
    assert_eq!(none["stopped"], false, "走っていなければ止める物が無い");

    // AI の recompute で計算し直させ、その途中を下の帯の「中止」(人と同じクリック)で止める
    ARMED.store(true, Ordering::SeqCst);
    let recompute = send(&dir.0, "recompute", json!({}));
    until_started(&mut h);
    ask(&mut h, &dir.0, "ui_click", json!({"target": "button:中止"}));
    settle(&mut h, recompute);
    until_idle(&mut h);
    assert_eq!(statuses(&h), ["ok", "stopped", "stopped"]);
    assert_eq!(h.state().document().data(), &data);
    assert_eq!(h.state().document().undo_depth(), depth);

    // 下の帯の「再計算」で止めた所から計算し直す
    ask(
        &mut h,
        &dir.0,
        "ui_expect",
        json!({"target": "button:再計算", "state": "enabled"}),
    );
    ask(
        &mut h,
        &dir.0,
        "ui_click",
        json!({"target": "button:再計算"}),
    );
    until_idle(&mut h);
    assert_eq!(statuses(&h), ["ok", "ok", "ok"]);
    ask(
        &mut h,
        &dir.0,
        "ui_expect",
        json!({"target": "button:再計算", "state": "missing"}),
    );
}
