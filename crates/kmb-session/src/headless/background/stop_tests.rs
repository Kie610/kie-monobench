//! 確かめること: 裏の再計算を利用者が止めると、計算できた所までの結果を出し、文書は変えず、
//! 「再計算」で止めた所から計算し直す。長い機能も計算の途中で止まる。

use super::*;
use kmb_core::{
    EvalCtx, EvalError, Feature, FeatureStatus, InputRef, Op, Outputs, Quantity, Transaction, Value,
};
use std::time::{Duration, Instant};

/// 計算を始めた印と、止めずに終えてよい印(この試験だけが使う)。
static STARTED: AtomicBool = AtomicBool::new(false);
static RELEASE: AtomicBool = AtomicBool::new(false);
const LIMIT: Duration = Duration::from_secs(10);

/// 止める印を見るか、終えてよい印が立つまで計算を続ける長い機能。
struct Long;
impl Feature for Long {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        Ok(vec![])
    }
    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        STARTED.store(true, Ordering::SeqCst);
        let began = Instant::now();
        while !RELEASE.load(Ordering::SeqCst) {
            if ctx.stopping() {
                return Err(EvalError::Other("止めた".into()));
            }
            assert!(began.elapsed() < LIMIT, "止める印が届かない");
            std::thread::yield_now();
        }
        Ok([(
            "value".to_string(),
            Value::Scalar(Quantity::new(1.0, kmb_core::Dim::NONE)),
        )]
        .into())
    }
}

/// 値を出すだけの機能(長い機能の前と後ろに置く)。
struct Quick;
impl Feature for Quick {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        Ok(vec![])
    }
    fn evaluate(&self, _: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        Ok(Outputs::new())
    }
}

fn add(doc: &mut Document, type_id: &str) -> FeatureId {
    let id = doc.alloc_feature_id();
    doc.apply(Transaction::new(
        "足す",
        vec![Op::InsertFeature {
            index: None,
            entry: kmb_core::FeatureEntry {
                id,
                type_id: type_id.into(),
                name: type_id.into(),
                data: serde_json::json!({}),
                suppressed: false,
                pin_bodies: false,
            },
        }],
    ))
    .unwrap();
    id
}

#[test]
fn stopping_a_background_recompute_keeps_the_document_and_computes_again_on_request() {
    let mut reg = kmb_core::Registry::new();
    reg.register("test.quick@1", |_| Ok(Box::new(Quick)));
    reg.register("test.long@1", |_| Ok(Box::new(Long)));
    let modules = Modules::new(reg, kmb_core::NullKernel);
    let mut doc = Document::new();
    let a = add(&mut doc, "test.quick@1");
    let l = add(&mut doc, "test.long@1");
    let b = add(&mut doc, "test.quick@1");
    let before = doc.to_json();
    let depth = doc.undo_depth();
    let mut h = Headless::in_background(modules, doc, None);
    assert!(!h.stop_recompute(), "走っていなければ止める物が無い");

    h.start_recompute(Box::new(|_, _| None), None);
    let began = Instant::now();
    while !STARTED.load(Ordering::SeqCst) {
        assert!(began.elapsed() < LIMIT, "長い機能が始まらない");
        std::thread::yield_now();
    }
    assert!(h.stop_recompute());
    assert!(h.is_stopping());
    let Polled::Done(_) = h.wait_recompute() else {
        panic!("止めた計算も結果を出す");
    };
    let r = h.report();
    assert!(matches!(r.feature(a), Some(FeatureStatus::Ok { .. })));
    assert_eq!(
        r.feature(l),
        Some(&FeatureStatus::Stopped),
        "長い機能の途中で止まる"
    );
    assert_eq!(r.feature(b), Some(&FeatureStatus::Stopped));
    assert!(!h.needs_recompute(), "止めた後は勝手に計算し直さない");
    assert!(!h.is_stopping());
    assert_eq!(h.doc().to_json(), before, "文書の中身は変えない");
    assert_eq!(h.doc().undo_depth(), depth, "取り消しの履歴にも残さない");

    // 「再計算」: 止めた所から計算し直す
    RELEASE.store(true, Ordering::SeqCst);
    h.recompute();
    assert!(h.needs_recompute());
    h.start_recompute(Box::new(|_, _| None), None);
    assert!(matches!(h.wait_recompute(), Polled::Done(_)));
    assert!(!h.report().stopped());
    assert!(matches!(
        h.report().feature(b),
        Some(FeatureStatus::Ok { .. })
    ));
}

/// 確かめること: CLI の `recompute --stop-after` の口。決めた時間で終わらない計算を、この場の
/// 計算でも止め、止めた機能の状態で返す。
#[test]
fn opening_with_a_time_limit_stops_a_long_recompute() {
    let mut reg = kmb_core::Registry::new();
    reg.register("test.quick@1", |_| Ok(Box::new(Quick)));
    reg.register("test.long_open@1", |_| Ok(Box::new(LongOpen)));
    let modules = Modules::new(reg, kmb_core::NullKernel);
    let mut doc = Document::new();
    add(&mut doc, "test.quick@1");
    let l = add(&mut doc, "test.long_open@1");
    let dir = std::env::temp_dir().join(format!("kmb-open-within-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("long.{}", kmb_core::DOCUMENT_EXTENSION));
    std::fs::write(&path, doc.to_json()).unwrap();
    let h = Headless::open_within(modules, &path, Duration::ZERO).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(h.report().stopped());
    assert_eq!(h.report().feature(l), Some(&FeatureStatus::Stopped));
}

/// 止める印を見るまで計算を続ける(印が届かなければ時間切れで誤り)。
struct LongOpen;
impl Feature for LongOpen {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        Ok(vec![])
    }
    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let began = Instant::now();
        while !ctx.stopping() {
            assert!(began.elapsed() < LIMIT, "止める印が届かない");
            std::thread::yield_now();
        }
        Err(EvalError::Other("止めた".into()))
    }
}
