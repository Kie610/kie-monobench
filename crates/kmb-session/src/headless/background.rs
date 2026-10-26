//! 裏の再計算(画面が使う)。文書の写しとキャッシュを別のスレッドへ渡して計算し、終わったら
//! 結果とキャッシュを受け取る。計算の途中で文書が変わったら、終わった後でもう一度計算する。
//!
//! 画面と CLI・MCP が同じ [`Headless`] を通るよう、ここに置く。始めるのと受け取るのは呼ぶ側が
//! 決める(画面は毎フレーム [`Headless::poll_recompute`] を呼び、要れば [`Headless::start_recompute`]
//! を呼ぶ)。

use super::Headless;
use kmb_core::{Cache, Document, FeatureId, Kernel, Modules, RecomputeReport, ShapeHandle};
use std::any::Any;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

/// 計算が終わった後に、同じスレッドで続けて行う仕事(画面の表示の網づくりなど)。見えている
/// ボディを受け取り、呼ぶ側だけが知る物を返す([`Recomputed::extra`])。
pub type After =
    Box<dyn FnOnce(&dyn Kernel, &[(FeatureId, ShapeHandle)]) -> Option<Box<dyn Any + Send>> + Send>;

/// 終わった再計算。
pub struct Recomputed {
    /// 前の計算の結果(新しい結果は [`Headless::report`])。前の形を出し続けるかの判断に使う
    pub previous: RecomputeReport,
    /// 見えているボディ
    pub bodies: Vec<(FeatureId, ShapeHandle)>,
    /// [`After`] が返した物
    pub extra: Option<Box<dyn Any + Send>>,
}

/// [`Headless::poll_recompute`] の結果。
pub enum Polled {
    /// 走っていない(計算が要るかは [`Headless::needs_recompute`])
    Idle,
    /// まだ走っている
    Running,
    /// 終わった
    Done(Recomputed),
    /// やめた計算か、入れ替える前の文書の計算だったので、結果は使わない。もう一度計算が要る
    Dropped,
    /// 計算のスレッドが途中で止まった。キャッシュは失われたので、次は初めから計算する
    Lost,
}

/// 裏で再計算する時の様子。
#[derive(Default)]
pub(super) struct Background {
    running: Option<Running>,
    /// 計算が要る(文書が変わった・計算をやめた)
    dirty: bool,
}

/// 走っている計算。
struct Running {
    rx: Receiver<Finished>,
    /// 走っている間に文書が変わった(終わった後でもう一度計算する)
    stale: bool,
    cancel: Arc<AtomicBool>,
    done: Arc<AtomicUsize>,
    total: usize,
    /// 入れ替える前の文書の計算(結果を使わない)
    foreign: bool,
}

/// 計算のスレッドから返る物。
struct Finished {
    cache: Cache,
    /// やめた時は `None`(途中の結果は出さない)
    report: Option<RecomputeReport>,
    bodies: Vec<(FeatureId, ShapeHandle)>,
    extra: Option<Box<dyn Any + Send>>,
}

impl Background {
    /// 文書が変わった。
    pub(super) fn mark(&mut self) {
        match &mut self.running {
            Some(r) => r.stale = true,
            None => self.dirty = true,
        }
    }

    /// 走っている計算の結果を使わない(文書を入れ替えた)。
    pub(super) fn forget_running(&mut self) {
        if let Some(r) = &mut self.running {
            r.foreign = true;
            r.cancel.store(true, Ordering::Relaxed);
        }
    }
}

use super::Hold;

impl Headless {
    /// 裏で再計算する文書。ここではまだ計算しない([`Self::start_recompute`] で始める)。
    pub fn in_background(modules: Modules, doc: Document, path: Option<PathBuf>) -> Self {
        Headless {
            doc,
            path,
            modules,
            report: RecomputeReport::default(),
            keep: vec![],
            background: Some(Background {
                running: None,
                dirty: true,
            }),
            active: Default::default(),
            recomputes: 0,
            hold: Hold::Off,
            creating: None,
            just_hidden: Default::default(),
        }
    }

    /// 裏で再計算するか。
    pub fn is_background(&self) -> bool {
        self.background.is_some()
    }

    /// 計算が要るのに走っていない(呼ぶ側が [`Self::start_recompute`] を呼ぶ時)。
    pub fn needs_recompute(&self) -> bool {
        self.background
            .as_ref()
            .is_some_and(|bg| bg.dirty && bg.running.is_none())
    }

    /// 裏の再計算が走っているか。
    pub fn is_recomputing(&self) -> bool {
        self.background
            .as_ref()
            .is_some_and(|bg| bg.running.is_some())
    }

    /// 走っている計算が、やめる印([`Self::supersede_recompute`])を立てられているか。
    pub fn is_superseded(&self) -> bool {
        self.background
            .as_ref()
            .and_then(|bg| bg.running.as_ref())
            .is_some_and(|r| r.stale && r.cancel.load(Ordering::Relaxed))
    }

    /// 裏の再計算の進み具合(通った機能の数、全部の数)。走っていなければ `None`。
    pub fn recompute_progress(&self) -> Option<(usize, usize)> {
        let r = self.background.as_ref()?.running.as_ref()?;
        Some((r.done.load(Ordering::Relaxed).min(r.total), r.total))
    }

    /// 裏で再計算を始める。走っていれば何もしない(終わった後で、要ればもう一度計算する)。
    /// `wake` は終わった時に別のスレッドから呼ぶ(画面を描き直させる)。
    pub fn start_recompute(&mut self, after: After, wake: Option<Box<dyn FnOnce() + Send>>) {
        let Some(bg) = &mut self.background else {
            return;
        };
        if bg.running.is_some() {
            return;
        }
        bg.dirty = false;
        // 寸法の名前(d1 など)をそろえてから写す(同期の計算は Document::recompute の中でそろえる)
        self.doc.settle_names(&self.modules.registry);
        let data = self.doc.data().clone();
        let mut cache = self.doc.take_cache();
        cache.hold(self.keep.iter().copied());
        let (registry, kernel) = (self.modules.registry.clone(), self.modules.kernel.clone());
        let (tx, rx) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let done = Arc::new(AtomicUsize::new(0));
        let passed = done.clone();
        let total = data.features.len();
        std::thread::spawn(move || {
            let report = kmb_core::recompute_watched(
                &data,
                &registry,
                kernel.as_ref(),
                &mut cache,
                &stop,
                &passed,
            )
            // 最後の機能の間にやめる印が立った時も、結果は出さない(続けて行う仕事をせずに次の
            // 計算へ移る。結果はキャッシュに残るので、次の計算で使い回せる)
            .filter(|_| !stop.load(Ordering::Relaxed));
            let bodies = report
                .as_ref()
                .map(|r| kmb_core::visible_bodies(&data, &registry, r))
                .unwrap_or_default();
            let extra = report
                .is_some()
                .then(|| after(kernel.as_ref(), &bodies))
                .flatten();
            let _ = tx.send(Finished {
                cache,
                report,
                bodies,
                extra,
            });
            if let Some(wake) = wake {
                wake();
            }
        });
        bg.running = Some(Running {
            rx,
            stale: false,
            cancel,
            done,
            total,
            foreign: false,
        });
    }

    /// 走っている計算を、もう要らないものとしてやめさせる(次の機能の前で止まる)。取り消しの
    /// ように、行き先の形だけが要る時に呼ぶ。値を続けて変える時(ドラッグなど)は呼ばない
    /// (途中の形を出し続けるため)。
    pub fn supersede_recompute(&mut self) {
        if let Some(r) = self.background.as_mut().and_then(|bg| bg.running.as_mut()) {
            r.stale = true;
            r.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// 裏の計算が終わっていれば結果を取り込む(待たない)。
    pub fn poll_recompute(&mut self) -> Polled {
        let Some(r) = self.background.as_ref().and_then(|bg| bg.running.as_ref()) else {
            return Polled::Idle;
        };
        match r.rx.try_recv() {
            Ok(f) => self.absorb(f),
            Err(TryRecvError::Empty) => Polled::Running,
            Err(TryRecvError::Disconnected) => self.lost(),
        }
    }

    /// 走っている計算が終わるまで待って取り込む。走っていなければ [`Polled::Idle`]。
    pub fn wait_recompute(&mut self) -> Polled {
        let Some(r) = self.background.as_ref().and_then(|bg| bg.running.as_ref()) else {
            return Polled::Idle;
        };
        match r.rx.recv() {
            Ok(f) => self.absorb(f),
            Err(_) => self.lost(),
        }
    }

    fn lost(&mut self) -> Polled {
        if let Some(bg) = &mut self.background {
            bg.running = None;
            bg.dirty = true;
        }
        Polled::Lost
    }

    fn absorb(&mut self, f: Finished) -> Polled {
        let Some(bg) = &mut self.background else {
            return Polled::Idle;
        };
        let Some(run) = bg.running.take() else {
            return Polled::Idle;
        };
        if run.foreign {
            // 入れ替える前の文書の計算: 結果は使わない。最後まで計算していたら、カーネルは今の
            // 文書の途中の形を捨てたかもしれないので、今の文書のキャッシュも捨てて計算し直す(U8)
            if f.report.is_some() {
                log::info!(
                    "入れ替える前の文書の計算が終わっていたので、今の文書を初めから計算し直す"
                );
                drop(self.doc.take_cache());
            }
            bg.dirty = true;
            return Polled::Dropped;
        }
        let Some(report) = f.report else {
            // やめた計算: 途中まで評価した結果はキャッシュに戻し、今の文書で計算し直す
            log::info!("前の再計算の結果は出さず、今の文書で計算し直す");
            self.doc.return_cache(f.cache);
            bg.dirty = true;
            return Polled::Dropped;
        };
        bg.dirty = run.stale;
        self.doc.put_cache(f.cache, &report);
        let previous = std::mem::replace(&mut self.report, report);
        Polled::Done(Recomputed {
            previous,
            bodies: f.bodies,
            extra: f.extra,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kmb_core::{Op, Param, Transaction};

    fn modules() -> Modules {
        Modules::new(kmb_core::Registry::new(), kmb_core::NullKernel)
    }

    fn with_param(doc: &mut Document, name: &str, expr: &str) {
        let id = doc.alloc_param_id();
        doc.apply(Transaction::new(
            "足す",
            vec![Op::AddParam {
                index: None,
                param: Param::new(id, name, expr),
            }],
        ))
        .unwrap();
    }

    fn copy(doc: &Document) -> Document {
        Document::from_json(&doc.to_json()).unwrap()
    }

    /// 裏の計算が終わるまで待つ(やり直しも含めて)。
    fn settle(h: &mut Headless) -> Vec<Recomputed> {
        let mut done = vec![];
        loop {
            if h.needs_recompute() {
                h.start_recompute(Box::new(|_, _| None), None);
            }
            match h.wait_recompute() {
                Polled::Idle => return done,
                Polled::Done(r) => done.push(r),
                Polled::Running | Polled::Dropped | Polled::Lost => {}
            }
        }
    }

    #[test]
    fn the_background_gives_the_same_report_as_computing_on_the_spot() {
        let mut doc = Document::new();
        with_param(&mut doc, "w", "40 mm");
        let mut now = Headless::with_document(modules(), copy(&doc), None);
        let mut bg = Headless::in_background(modules(), doc, None);
        assert!(bg.needs_recompute(), "作った時は計算が要る");
        assert_eq!(settle(&mut bg).len(), 1);
        assert_eq!(bg.report(), now.report());

        now.set_param("w", "50 mm").unwrap();
        bg.set_param("w", "50 mm").unwrap();
        assert!(bg.needs_recompute(), "変えたら計算が要る");
        let done = settle(&mut bg);
        assert_eq!(bg.report(), now.report());
        assert_ne!(&done[0].previous, bg.report(), "前の結果も返す");
    }

    #[test]
    fn a_change_while_computing_is_computed_again_afterwards() {
        let mut doc = Document::new();
        with_param(&mut doc, "w", "40 mm");
        let mut bg = Headless::in_background(modules(), doc, None);
        bg.start_recompute(Box::new(|_, _| None), None);
        assert!(bg.is_recomputing());
        bg.set_param("w", "60 mm").unwrap();
        assert!(!bg.needs_recompute(), "走っている間は始めない");
        assert!(matches!(bg.wait_recompute(), Polled::Done(_)));
        assert!(
            bg.needs_recompute(),
            "走っている間に変わったので、もう一度計算が要る"
        );
        settle(&mut bg);
        let mut now = Headless::with_document(modules(), copy(bg.doc()), None);
        now.recompute();
        assert_eq!(bg.report(), now.report());
    }

    #[test]
    fn the_result_for_a_replaced_document_is_not_used() {
        let mut doc = Document::new();
        with_param(&mut doc, "w", "40 mm");
        let mut bg = Headless::in_background(modules(), doc, None);
        bg.start_recompute(Box::new(|_, _| None), None);
        let mut other = Document::new();
        with_param(&mut other, "h", "7 mm");
        bg.replace(copy(&other), None);
        assert!(matches!(bg.wait_recompute(), Polled::Dropped));
        settle(&mut bg);
        let now = Headless::with_document(modules(), other, None);
        assert_eq!(bg.report(), now.report());
    }

    #[test]
    fn the_after_work_sees_the_visible_bodies_and_its_result_comes_back() {
        let mut bg = Headless::in_background(modules(), Document::new(), None);
        bg.start_recompute(
            Box::new(|_, bodies| Some(Box::new(bodies.len()) as Box<dyn Any + Send>)),
            None,
        );
        let Polled::Done(r) = bg.wait_recompute() else {
            panic!("終わる");
        };
        assert_eq!(
            r.extra.and_then(|e| e.downcast::<usize>().ok()),
            Some(Box::new(0))
        );
    }

    /// 開くのをやめた時(画面の U8): 入れ替えた文書の計算は使わず、前の文書と前の結果に戻して
    /// 計算し直す。前の文書は入れ替えの時に返る。
    #[test]
    fn a_replaced_document_can_be_put_back_with_its_report() {
        let mut doc = Document::new();
        with_param(&mut doc, "w", "40 mm");
        let mut bg = Headless::in_background(modules(), doc, None);
        settle(&mut bg);
        let before = bg.report().clone();
        let mut other = Document::new();
        with_param(&mut other, "h", "7 mm");
        let old = bg.replace(other, Some("b.json".into()));
        bg.start_recompute(Box::new(|_, _| None), None);
        bg.put_back(old, None, before.clone());
        assert_eq!(bg.report(), &before, "前の結果をすぐ出す");
        assert!(bg.is_recomputing());
        assert!(matches!(bg.wait_recompute(), Polled::Dropped));
        settle(&mut bg);
        assert_eq!(bg.report(), &before);
        assert!(bg.path().is_none());
        assert!(bg.doc().data().param_by_name("w").is_some());
    }
}
