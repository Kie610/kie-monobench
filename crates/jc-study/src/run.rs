//! 研究を「実行」で解くしくみ。
//!
//! 研究の機能は、再計算では解かない。評価([`jc_core::Feature::evaluate`])は条件を読み、解く仕事
//! ([`Task`])を作って [`prepare`] に預け、出力 `plan` に仕事の鍵([`Plan::key`])だけを出す(速い)。
//! 「実行」を押すと、画面の拡張([`crate::StudyCommands`])が鍵で仕事を取り出し、別のスレッドで解く
//! ([`Runner`])。条件や形が変わると鍵が変わるので、前の結果は「古い」になる(消さずに出す)。
//! 計算中は進み具合と段を [`Control`] で知らせ、中止は [`Control::cancelled`] を解き方が見て止める。

use crate::result::SimResult;
use jc_core::{Payload, Series};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// 研究の出力の名前(評価が出す仕事の鍵)。
pub const PLAN: &str = "plan";

/// 評価が出す、解く仕事の札。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    /// 仕事の鍵(解く条件と形から作る。同じなら同じ結果になる)
    pub key: u64,
    /// 計算の段の名前(進み具合の行)
    #[serde(default)]
    pub steps: Vec<String>,
    /// 見せ方の初めの断面(世界の軸と位置 mm)。解く条件ではないので鍵に入れない
    /// (3D の流れの「断面」の欄。直しても解き直さない)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<(usize, f64)>,
    /// 中止しても途中までの結果を残すか(時刻を進める流れ)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub keeps_partial: bool,
}

impl Payload for Plan {
    const TYPE: &'static str = "study.plan@1";
}

/// 解く仕事。
pub trait Task: Send + Sync {
    /// 解く(重い)。中止されたら、途中までの結果([`SimResult::partial`])か誤りを返す。
    fn solve(&self, c: &Control) -> Result<SimResult, String>;
}

/// 評価が作った解く仕事の預かり所(鍵ごと)。古い物から捨てる。組み立てごとに 1 つで、jc-study の
/// モジュールが共有の値([`jc_core::Registry::provide`])として置く。解析の機能(jc-fea・jc-flow)は
/// 評価の中で [`prepare`] で預け、研究の実行は [`prepared`] で受け取る。鍵は条件から作るので、
/// 文書やタブをまたいでも混ざらない。
#[derive(Default)]
pub struct Prepared {
    inner: Mutex<Store>,
}

#[derive(Default)]
struct Store {
    order: Vec<u64>,
    tasks: HashMap<u64, Arc<dyn Task>>,
}

/// 預ける仕事の数の上限(形の網を持つので多くは持たない)。
const KEEP: usize = 48;

impl Prepared {
    fn put(&self, key: u64, task: Arc<dyn Task>) {
        let mut s = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        s.order.retain(|k| *k != key);
        s.order.push(key);
        s.tasks.insert(key, task);
        while s.order.len() > KEEP {
            let old = s.order.remove(0);
            s.tasks.remove(&old);
        }
    }

    fn get(&self, key: u64) -> Option<Arc<dyn Task>> {
        let s = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        s.tasks.get(&key).cloned()
    }
}

/// 評価が作った仕事を預ける。預かり所が組み立てに無ければ(jc-study のモジュールを並べていない)
/// ログに残し、実行の時に「解く準備が見つかりません」になる。
pub fn prepare(ctx: &jc_core::EvalCtx<'_>, key: u64, task: Arc<dyn Task>) {
    match ctx.service::<Prepared>() {
        Some(p) => p.put(key, task),
        None => {
            log::warn!("解析の仕事の預かり所がありません(jc-study のモジュールを並べてください)")
        }
    }
}

/// 預けた仕事。
pub fn prepared(registry: &jc_core::Registry, key: u64) -> Option<Arc<dyn Task>> {
    registry.service::<Prepared>()?.get(key)
}

/// 解き方へ渡す、進み具合の知らせと中止の印。
#[derive(Default)]
pub struct Control {
    cancel: AtomicBool,
    state: Mutex<Progress>,
}

/// 計算中の様子。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Progress {
    /// 今の段の番号([`Plan::steps`])
    pub step: usize,
    /// 全体の進み(0〜1)
    pub fraction: f32,
    /// 今の段の様子(「0.21 / 0.50 s」)
    pub note: String,
    /// 推移のグラフ
    pub series: Option<Series>,
}

impl Control {
    pub fn new() -> Self {
        Self::default()
    }

    /// 中止を頼まれたか。
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    fn with(&self, f: impl FnOnce(&mut Progress)) {
        if let Ok(mut p) = self.state.lock() {
            f(&mut p);
        }
    }

    /// 段 `i` に入った。
    pub fn step(&self, i: usize) {
        self.with(|p| {
            p.step = i;
            p.note.clear();
        });
    }

    /// 全体の進み(0〜1)と今の段の様子。
    pub fn progress(&self, fraction: f32, note: impl Into<String>) {
        let note = note.into();
        self.with(|p| {
            p.fraction = fraction.clamp(0.0, 1.0);
            p.note = note;
        });
    }

    /// 推移のグラフを置き換える。
    pub fn series(&self, s: Series) {
        self.with(|p| p.series = Some(s));
    }

    pub fn snapshot(&self) -> Progress {
        self.state.lock().map(|p| p.clone()).unwrap_or_default()
    }
}

/// 今走っている計算。
struct Running {
    key: u64,
    steps: Vec<String>,
    control: Arc<Control>,
    started: Instant,
    out: Arc<Mutex<Option<Result<SimResult, String>>>>,
}

/// 終わった計算。
#[derive(Clone)]
pub struct Done {
    /// 解いた仕事の鍵
    pub key: u64,
    pub result: Arc<SimResult>,
    /// かかった秒数
    pub secs: f64,
}

/// 研究ごとの計算の記録。
#[derive(Default)]
struct Slot {
    running: Option<Running>,
    /// いちばん新しい結果(古くなっても残す)
    done: Option<Done>,
    /// 最後の実行の誤り(結果は前のまま残す)
    failed: Option<(u64, String)>,
}

/// 研究ごとの様子(画面の札とパネル用)。
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    /// 解いていない
    Idle,
    /// 計算中(経った秒数、段、様子)
    Running {
        key: u64,
        secs: f64,
        steps: Vec<String>,
        progress: Progress,
    },
    /// 結果がある(鍵、秒数)
    Done { key: u64, secs: f64 },
}

/// 実行の管理。研究は (文書の番号、機能の番号) で分ける。
#[derive(Default)]
pub struct Runner {
    slots: Mutex<HashMap<(u64, u64), Slot>>,
}

impl Runner {
    /// 仕事を別のスレッドで解き始める。同じ研究が計算中なら止めてから。
    pub fn start(&self, at: (u64, u64), key: u64, steps: Vec<String>, task: Arc<dyn Task>) {
        let control = Arc::new(Control::new());
        let out = Arc::new(Mutex::new(None));
        {
            let (c, o) = (control.clone(), out.clone());
            std::thread::Builder::new()
                .name("研究の計算".into())
                .spawn(move || {
                    let t = Instant::now();
                    let r =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| task.solve(&c)))
                            .unwrap_or_else(|e| {
                                let msg = e
                                    .downcast_ref::<String>()
                                    .cloned()
                                    .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                                    .unwrap_or_default();
                                Err(format!("計算の途中で止まりました({msg})"))
                            });
                    match &r {
                        Ok(r) => log::info!(
                            "研究の計算が終わった({:.2} 秒、{}{})",
                            t.elapsed().as_secs_f64(),
                            r.summary,
                            if r.partial { "、中止" } else { "" }
                        ),
                        Err(e) => log::warn!("研究の計算が失敗: {e}"),
                    }
                    if let Ok(mut g) = o.lock() {
                        *g = Some(r);
                    }
                })
                .expect("計算のスレッドを作れる");
        }
        if let Ok(mut m) = self.slots.lock() {
            let s = m.entry(at).or_default();
            if let Some(r) = s.running.take() {
                r.control.cancel();
            }
            s.running = Some(Running {
                key,
                steps,
                control,
                started: Instant::now(),
                out,
            });
            s.failed = None;
        }
    }

    /// 仕事をこのスレッドで解く(CLI・MCP・試験。終わるまで返らない)。
    pub fn solve_now(
        &self,
        at: (u64, u64),
        key: u64,
        task: &dyn Task,
    ) -> Result<Arc<SimResult>, String> {
        let t = Instant::now();
        let r = task.solve(&Control::new());
        let mut m = self.slots.lock().map_err(|e| e.to_string())?;
        let s = m.entry(at).or_default();
        match r {
            Ok(r) => {
                let r = Arc::new(r);
                s.done = Some(Done {
                    key,
                    result: r.clone(),
                    secs: t.elapsed().as_secs_f64(),
                });
                s.failed = None;
                Ok(r)
            }
            Err(e) => {
                s.failed = Some((key, e.clone()));
                Err(e)
            }
        }
    }

    /// 計算中なら中止を頼む(止まると途中までのコマが結果になる)。頼んだら真。
    pub fn cancel(&self, at: (u64, u64)) -> bool {
        self.poll();
        let Ok(m) = self.slots.lock() else {
            return false;
        };
        match m.get(&at).and_then(|s| s.running.as_ref()) {
            Some(r) => {
                r.control.cancel();
                true
            }
            None => false,
        }
    }

    /// 終わった計算を結果へ移す。
    pub fn poll(&self) {
        let Ok(mut m) = self.slots.lock() else { return };
        for s in m.values_mut() {
            let finished = s
                .running
                .as_ref()
                .and_then(|r| r.out.lock().ok()?.take().map(|o| (o, r)))
                .map(|(o, r)| {
                    let stopped = r.control.cancelled();
                    (o, r.key, r.started.elapsed().as_secs_f64(), stopped)
                });
            if let Some((out, key, secs, stopped)) = finished {
                s.running = None;
                match out {
                    Ok(r) => {
                        s.done = Some(Done {
                            key,
                            result: Arc::new(r),
                            secs,
                        });
                    }
                    // 中止して途中の結果が無ければ、前の結果のまま(誤りにしない)
                    Err(_) if stopped => {}
                    Err(e) => s.failed = Some((key, e)),
                }
            }
        }
    }

    /// 研究の様子。
    pub fn status(&self, at: (u64, u64)) -> Status {
        self.poll();
        let Ok(m) = self.slots.lock() else {
            return Status::Idle;
        };
        let Some(s) = m.get(&at) else {
            return Status::Idle;
        };
        if let Some(r) = &s.running {
            return Status::Running {
                key: r.key,
                secs: r.started.elapsed().as_secs_f64(),
                steps: r.steps.clone(),
                progress: r.control.snapshot(),
            };
        }
        match &s.done {
            Some(d) => Status::Done {
                key: d.key,
                secs: d.secs,
            },
            None => Status::Idle,
        }
    }

    /// いちばん新しい結果(古くても)。
    pub fn done(&self, at: (u64, u64)) -> Option<Done> {
        self.poll();
        self.slots.lock().ok()?.get(&at)?.done.clone()
    }

    /// 最後の実行の誤り(その鍵と)。
    pub fn failure(&self, at: (u64, u64)) -> Option<(u64, String)> {
        self.slots.lock().ok()?.get(&at)?.failed.clone()
    }

    /// どれかが計算中か(画面は計算中だけ描き直しを続ける)。
    pub fn busy(&self) -> bool {
        self.poll();
        self.slots
            .lock()
            .map(|m| m.values().any(|s| s.running.is_some()))
            .unwrap_or(false)
    }
}

/// 秒を「分:秒」に。
pub fn clock(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Slow;

    impl Task for Slow {
        fn solve(&self, c: &Control) -> Result<SimResult, String> {
            for i in 0..200 {
                if c.cancelled() {
                    return Ok(SimResult {
                        summary: format!("{i} 回で止めた"),
                        partial: true,
                        ..SimResult::default()
                    });
                }
                c.progress(i as f32 / 200.0, format!("{i}"));
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Ok(SimResult::default())
        }
    }

    fn wait_idle(r: &Runner, at: (u64, u64)) {
        let t = Instant::now();
        while matches!(r.status(at), Status::Running { .. }) {
            assert!(t.elapsed().as_secs() < 20, "終わらない");
            std::thread::yield_now();
        }
    }

    #[test]
    fn cancel_keeps_the_partial_result() {
        let r = Runner::default();
        let at = (1, 7);
        r.start(at, 42, vec!["解く".into()], Arc::new(Slow));
        assert!(matches!(r.status(at), Status::Running { key: 42, .. }));
        assert!(r.busy());
        assert!(r.cancel(at));
        wait_idle(&r, at);
        let d = r.done(at).expect("途中までの結果");
        assert!(d.result.partial);
        assert_eq!(d.key, 42);
        assert!(!r.busy());
        // ほかの文書の同じ番号の研究には結果が無い
        assert!(r.done((2, 7)).is_none());
    }

    /// 途中の結果を残さない仕事(強度など)。中止すると誤りを返す。
    struct Stops;

    impl Task for Stops {
        fn solve(&self, c: &Control) -> Result<SimResult, String> {
            while !c.cancelled() {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            Err("中止しました".into())
        }
    }

    #[test]
    fn cancelling_without_a_partial_result_keeps_the_last_result_and_no_failure() {
        let r = Runner::default();
        let at = (1, 8);
        r.solve_now(at, 1, &Slow).unwrap();
        r.start(at, 2, Vec::new(), Arc::new(Stops));
        assert!(r.cancel(at));
        wait_idle(&r, at);
        assert!(r.failure(at).is_none(), "中止は失敗にしない");
        assert_eq!(r.done(at).unwrap().key, 1, "前の結果が残る");
    }

    #[test]
    fn prepared_tasks_are_found_by_key() {
        let p = Prepared::default();
        p.put(9_000_001, Arc::new(Slow));
        assert!(p.get(9_000_001).is_some());
        assert!(p.get(9_000_002).is_none());
        // 組み立てごとに別の預かり所(プロセス全体で 1 つにしない)
        assert!(Prepared::default().get(9_000_001).is_none());
    }
}
