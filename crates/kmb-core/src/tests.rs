//! コアの単体テスト。機能はテスト用に、ここで小さなものを定義する。

use crate::*;
use proptest::prelude::*;
use serde_json::json;
use std::cell::Cell;

/// `{"expr": "..."}` を評価して `value` に出す
struct ValueFeature(String);
impl Feature for ValueFeature {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        expr_inputs(&self.0)
    }
    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        EVALS.with(|c| c.set(c.get() + 1));
        Ok([("value".to_string(), Value::Scalar(ctx.expr(&self.0)?))].into())
    }
}
fn value_factory(d: &serde_json::Value) -> Result<Box<dyn Feature>, EvalError> {
    let e = d["expr"]
        .as_str()
        .ok_or(EvalError::InvalidData("expr がありません".into()))?;
    Ok(Box::new(ValueFeature(e.to_string())))
}

/// `{"source": id}` の `value` を 2 倍にする
struct DoubleFeature(FeatureId);
impl Feature for DoubleFeature {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        Ok(vec![InputRef::Output {
            feature: self.0,
            slot: "value".into(),
        }])
    }
    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let Value::Scalar(q) = ctx.output(self.0, "value")? else {
            return Err(EvalError::Other("数値ではありません".into()));
        };
        Ok([(
            "value".to_string(),
            Value::Scalar(Quantity::new(q.value * 2.0, q.dim)),
        )]
        .into())
    }
}
fn double_factory(d: &serde_json::Value) -> Result<Box<dyn Feature>, EvalError> {
    let id = d["source"]
        .as_u64()
        .ok_or(EvalError::InvalidData("source がありません".into()))?;
    Ok(Box::new(DoubleFeature(FeatureId(id))))
}

/// 宣言せずにパラメータを読む(契約違反)
struct SneakyFeature;
impl Feature for SneakyFeature {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        Ok(vec![])
    }
    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        ctx.param("width")?;
        Ok(Outputs::new())
    }
}

struct PanicFeature;
impl Feature for PanicFeature {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        Ok(vec![])
    }
    fn evaluate(&self, _: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        panic!("カーネルの不具合の代わり")
    }
}

thread_local! {
    static EVALS: Cell<usize> = const { Cell::new(0) };
}

fn registry() -> Registry {
    let mut r = Registry::new();
    r.register("test.value@1", value_factory);
    r.register("test.double@1", double_factory);
    r.register("test.sneaky@1", |_| Ok(Box::new(SneakyFeature)));
    r.register("test.panic@1", |_| Ok(Box::new(PanicFeature)));
    r
}

fn add_param(doc: &mut Document, name: &str, expr: &str) -> ParamId {
    let id = doc.alloc_param_id();
    let param = Param::new(id, name, expr);
    doc.apply(Transaction::new(
        "パラメータ追加",
        vec![Op::AddParam { index: None, param }],
    ))
    .unwrap();
    id
}

fn add_feature(doc: &mut Document, type_id: &str, data: serde_json::Value) -> FeatureId {
    let id = doc.alloc_feature_id();
    let entry = FeatureEntry {
        id,
        type_id: type_id.into(),
        name: type_id.into(),
        data,
        suppressed: false,
        pin_bodies: false,
    };
    doc.apply(Transaction::new(
        "機能追加",
        vec![Op::InsertFeature { index: None, entry }],
    ))
    .unwrap();
    id
}

fn scalar(r: &RecomputeReport, id: FeatureId) -> Quantity {
    match r.feature(id) {
        Some(FeatureStatus::Ok { outputs, .. }) => match outputs["value"] {
            Value::Scalar(q) => q,
            _ => panic!("数値ではありません"),
        },
        other => panic!("{id} が Ok ではありません: {other:?}"),
    }
}

#[test]
fn parameter_change_propagates_downstream() {
    let reg = registry();
    let mut doc = Document::new();
    let w = add_param(&mut doc, "width", "40 mm");
    add_param(&mut doc, "half", "width / 2");
    let a = add_feature(&mut doc, "test.value@1", json!({"expr": "half + 1 mm"}));
    let b = add_feature(&mut doc, "test.double@1", json!({"source": a.0}));
    let r = doc.recompute(&reg, &NullKernel);
    assert_eq!(r.error_count(), 0);
    assert_eq!(scalar(&r, b), Quantity::mm(42.0));

    doc.apply(Transaction::new(
        "幅を変更",
        vec![Op::SetParamExpr {
            id: w,
            expr: "60 mm".into(),
        }],
    ))
    .unwrap();
    let r = doc.recompute(&reg, &NullKernel);
    assert_eq!(scalar(&r, b), Quantity::mm(62.0));
}

#[test]
fn unchanged_features_are_reused_from_cache() {
    let reg = registry();
    let mut doc = Document::new();
    add_param(&mut doc, "a", "1");
    let b = add_param(&mut doc, "b", "2");
    add_feature(&mut doc, "test.value@1", json!({"expr": "a"}));
    add_feature(&mut doc, "test.value@1", json!({"expr": "b"}));
    let r = doc.recompute(&reg, &NullKernel);
    assert_eq!((r.evaluated, r.reused), (2, 0));
    doc.apply(Transaction::new(
        "b",
        vec![Op::SetParamExpr {
            id: b,
            expr: "3".into(),
        }],
    ))
    .unwrap();
    let r = doc.recompute(&reg, &NullKernel);
    assert_eq!((r.evaluated, r.reused), (1, 1));
}

/// 評価すると [`STOP`] を立てる(計算の途中で新しい変更が来た時の代わり)
struct StopFeature;
impl Feature for StopFeature {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        Ok(vec![])
    }
    fn evaluate(&self, _: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        STOP.store(true, std::sync::atomic::Ordering::Relaxed);
        Ok(Outputs::new())
    }
}
static STOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[test]
fn a_cancelled_recompute_stops_before_the_next_feature_and_keeps_what_it_did() {
    let mut reg = registry();
    reg.register("test.stop@1", |_| Ok(Box::new(StopFeature)));
    let mut doc = Document::new();
    add_feature(&mut doc, "test.value@1", json!({"expr": "1"}));
    add_feature(&mut doc, "test.stop@1", json!({}));
    let b = add_feature(&mut doc, "test.value@1", json!({"expr": "2"}));
    let c = add_feature(&mut doc, "test.double@1", json!({"source": b.0}));
    let mut cache = doc.take_cache();
    EVALS.with(|e| e.set(0));
    let r = recompute_cancellable(doc.data(), &reg, &NullKernel, &mut cache, &STOP);
    assert!(r.is_none(), "印が立ったら残りは計算しない");
    assert_eq!(EVALS.with(Cell::get), 1, "止める前の 1 つだけ評価した");

    // やめるまでに評価した結果は、次の再計算で使い回す
    STOP.store(false, std::sync::atomic::Ordering::Relaxed);
    let never = std::sync::atomic::AtomicBool::new(false);
    let r = recompute_cancellable(doc.data(), &reg, &NullKernel, &mut cache, &never).unwrap();
    assert_eq!((r.evaluated, r.reused), (2, 2));
    assert_eq!(scalar(&r, c), Quantity::new(4.0, Dim::NONE));
}

#[test]
fn undoing_a_change_reuses_the_previous_result() {
    let reg = registry();
    let mut doc = Document::new();
    let a = add_param(&mut doc, "a", "1");
    let v = add_feature(&mut doc, "test.value@1", json!({"expr": "a"}));
    add_feature(&mut doc, "test.double@1", json!({"source": v.0}));
    doc.recompute(&reg, &NullKernel);
    let set = |doc: &mut Document, e: &str| {
        doc.apply(Transaction::new(
            "a",
            vec![Op::SetParamExpr {
                id: a,
                expr: e.into(),
            }],
        ))
        .unwrap();
    };
    set(&mut doc, "2");
    let r = doc.recompute(&reg, &NullKernel);
    assert_eq!((r.evaluated, r.reused), (2, 0));
    // 取り消し・やり直しは、前と今の結果のどちらも覚えているので計算し直さない
    doc.undo();
    let r = doc.recompute(&reg, &NullKernel);
    assert_eq!((r.evaluated, r.reused), (0, 2));
    doc.redo();
    let r = doc.recompute(&reg, &NullKernel);
    assert_eq!((r.evaluated, r.reused), (0, 2));
    // 覚えるのは 2 つまで(3 つ目の値に変えると、一番古い 1 は忘れる)
    set(&mut doc, "3");
    doc.recompute(&reg, &NullKernel);
    set(&mut doc, "1");
    let r = doc.recompute(&reg, &NullKernel);
    assert_eq!((r.evaluated, r.reused), (2, 0));
}

#[test]
fn param_cycles_and_unknown_names_are_errors() {
    let reg = registry();
    let mut doc = Document::new();
    add_param(&mut doc, "a", "b + 1");
    add_param(&mut doc, "b", "a + 1");
    add_param(&mut doc, "c", "c");
    add_param(&mut doc, "d", "nothing");
    add_param(&mut doc, "e", "2");
    let r = doc.recompute(&reg, &NullKernel);
    for name in ["a", "b", "c", "d"] {
        assert!(
            matches!(r.param(name), Some(ParamStatus::Error { .. })),
            "{name}"
        );
    }
    assert!(matches!(r.param("e"), Some(ParamStatus::Ok { .. })));
}

#[test]
fn failures_block_downstream_but_not_the_rest() {
    let reg = registry();
    let mut doc = Document::new();
    add_param(&mut doc, "bad", "1 mm + 1 deg");
    let a = add_feature(&mut doc, "test.value@1", json!({"expr": "bad"}));
    let b = add_feature(&mut doc, "test.double@1", json!({"source": a.0}));
    let c = add_feature(&mut doc, "test.value@1", json!({"expr": "5"}));
    let p = add_feature(&mut doc, "test.panic@1", json!({}));
    let s = add_feature(&mut doc, "test.sneaky@1", json!({}));
    let u = add_feature(&mut doc, "test.unknown@1", json!({}));
    let r = doc.recompute(&reg, &NullKernel);
    // パラメータが原因なら上流の機能はなく、機能が原因ならその機能を指す
    assert!(matches!(
        r.feature(a),
        Some(FeatureStatus::Blocked { feature: None, .. })
    ));
    assert!(matches!(
        r.feature(b),
        Some(FeatureStatus::Blocked { feature: Some(f), .. }) if *f == a
    ));
    assert_eq!(scalar(&r, c), Quantity::scalar(5.0));
    assert!(
        matches!(r.feature(p), Some(FeatureStatus::Error { message }) if message.contains("panic"))
    );
    assert!(
        matches!(r.feature(s), Some(FeatureStatus::Error { message }) if message.contains("宣言"))
    );
    assert!(matches!(r.feature(u), Some(FeatureStatus::Error { .. })));
}

#[test]
fn references_to_later_features_are_rejected() {
    let reg = registry();
    let mut doc = Document::new();
    let a = add_feature(&mut doc, "test.value@1", json!({"expr": "1"}));
    let b = add_feature(&mut doc, "test.double@1", json!({"source": a.0}));
    doc.apply(Transaction::new(
        "移動",
        vec![Op::MoveFeature { id: b, to: 0 }],
    ))
    .unwrap();
    let r = doc.recompute(&reg, &NullKernel);
    assert!(
        matches!(r.feature(b), Some(FeatureStatus::Error { message }) if message.contains("後ろ"))
    );
}

#[test]
fn rollback_and_suppression() {
    let reg = registry();
    let mut doc = Document::new();
    let a = add_feature(&mut doc, "test.value@1", json!({"expr": "1"}));
    let b = add_feature(&mut doc, "test.double@1", json!({"source": a.0}));
    doc.recompute(&reg, &NullKernel);
    doc.apply(Transaction::new(
        "ロールバック",
        vec![Op::SetRollback { rollback: Some(1) }],
    ))
    .unwrap();
    // ロールバック中に追加した機能は、マーカーの位置に入って有効になる
    let c = add_feature(&mut doc, "test.value@1", json!({"expr": "3"}));
    assert_eq!(doc.data().rollback, Some(2));
    assert_eq!(doc.data().features[1].id, c);
    let r = doc.recompute(&reg, &NullKernel);
    assert!(matches!(r.feature(b), Some(FeatureStatus::RolledBack)));
    assert_eq!(scalar(&r, c), Quantity::scalar(3.0));
    // 棒の後ろの機能の結果は残るので、取り消して棒を戻すと計算し直さない
    doc.undo();
    doc.undo();
    let r = doc.recompute(&reg, &NullKernel);
    assert_eq!((r.evaluated, r.reused), (0, 2));
    doc.redo();
    doc.redo();
    let r = doc.recompute(&reg, &NullKernel);
    assert_eq!(scalar(&r, c), Quantity::scalar(3.0));

    doc.apply(Transaction::new(
        "戻す",
        vec![Op::SetRollback { rollback: None }],
    ))
    .unwrap();
    doc.apply(Transaction::new(
        "抑制",
        vec![Op::SetSuppressed {
            id: a,
            suppressed: true,
        }],
    ))
    .unwrap();
    let r = doc.recompute(&reg, &NullKernel);
    assert!(matches!(r.feature(a), Some(FeatureStatus::Suppressed)));
    assert!(matches!(r.feature(b), Some(FeatureStatus::Blocked { .. })));
}

#[test]
fn invalid_transactions_change_nothing() {
    let mut doc = Document::new();
    add_param(&mut doc, "width", "1 mm");
    let before = doc.data().clone();
    let id = doc.alloc_param_id();
    let ok = Op::AddParam {
        index: None,
        param: Param::new(id, "ok", "1"),
    };
    for bad in [
        Op::AddParam {
            index: None,
            param: Param::new(ParamId(999), "x", "1"),
        },
        Op::SetParamExpr {
            id: ParamId(1),
            expr: "1 +".into(),
        },
        Op::RenameParam {
            id: ParamId(1),
            name: "ok".into(),
        },
        Op::RenameParam {
            id: ParamId(1),
            name: "mm".into(),
        },
        Op::RemoveFeature { id: FeatureId(1) },
        Op::SetRollback { rollback: Some(5) },
    ] {
        let tx = Transaction::new("失敗", vec![ok.clone(), bad.clone()]);
        assert!(doc.apply(tx).is_err(), "{bad:?}");
        let mut expected = before.clone();
        expected.next_id = doc.data().next_id;
        assert_eq!(doc.data(), &expected);
    }
}

#[test]
fn events_are_delivered() {
    let reg = registry();
    let mut doc = Document::new();
    let rx = doc.subscribe();
    add_param(&mut doc, "a", "1");
    doc.undo();
    doc.redo();
    doc.recompute(&reg, &NullKernel);
    let events: Vec<_> = rx.try_iter().collect();
    assert!(matches!(events[0], DocEvent::Changed { .. }));
    assert!(matches!(events[1], DocEvent::Undone { .. }));
    assert!(matches!(events[2], DocEvent::Redone { .. }));
    assert_eq!(events[3], DocEvent::Recomputed { errors: 0 });
}

#[test]
fn load_rejects_bad_files() {
    assert!(matches!(Document::from_json("{"), Err(LoadError::Json(_))));
    let data = DocumentData {
        format_version: 99,
        ..DocumentData::default()
    };
    let s = serde_json::to_string(&data).unwrap();
    assert!(matches!(
        Document::from_json(&s),
        Err(LoadError::UnsupportedVersion(99))
    ));
    let e = Document::from_json(&s).err().unwrap().to_string();
    assert!(e.contains("新しい版のアプリ"), "{e}");
    let mut data = DocumentData::default();
    data.params.push(Param::new(ParamId(1), "a", "1"));
    let s = serde_json::to_string(&data).unwrap(); // next_id = 1 なのに ID 1 を使っている
    assert!(matches!(
        Document::from_json(&s),
        Err(LoadError::Invalid(_))
    ));
    // 単位の名前と同じパラメータ(単位を足す前の文書)は、黙って単位として読まず、名前を替える
    // よう言って止める
    let mut data = DocumentData {
        next_id: 2,
        ..DocumentData::default()
    };
    data.params.push(Param::new(ParamId(1), "ft", "3"));
    let e = Document::from_json(&serde_json::to_string(&data).unwrap())
        .err()
        .unwrap()
        .to_string();
    assert!(
        e.contains("ft") && e.contains("単位の名前") && e.contains("替えて"),
        "{e}"
    );
}

// ---- 性質のテスト ----

/// 乱数で選んだ操作。ID は文書の中の既存のものから選ぶ。
#[derive(Debug, Clone)]
enum Action {
    AddParam(u8, String),
    SetExpr(u8, String),
    Rename(u8, u8),
    RemoveParam(u8),
    AddValue(Option<u8>, String),
    AddDouble(u8),
    Remove(u8),
    Move(u8, u8),
    Suppress(u8, bool),
    Rollback(Option<u8>),
    Group(u8, u8),
    Ungroup(u8),
    Undo,
    Redo,
}

fn expr_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("1".to_string()),
        Just("2 mm".to_string()),
        Just("p0 + 1".to_string()),
        Just("p1 * 2".to_string()),
        Just("p2 / 0".to_string()),
        Just("1 +".to_string()),
    ]
}

fn action_strategy() -> impl Strategy<Value = Action> {
    prop_oneof![
        (any::<u8>(), expr_strategy()).prop_map(|(n, e)| Action::AddParam(n, e)),
        (any::<u8>(), expr_strategy()).prop_map(|(n, e)| Action::SetExpr(n, e)),
        (any::<u8>(), any::<u8>()).prop_map(|(a, b)| Action::Rename(a, b)),
        any::<u8>().prop_map(Action::RemoveParam),
        (proptest::option::of(any::<u8>()), expr_strategy())
            .prop_map(|(i, e)| Action::AddValue(i, e)),
        any::<u8>().prop_map(Action::AddDouble),
        any::<u8>().prop_map(Action::Remove),
        (any::<u8>(), any::<u8>()).prop_map(|(a, b)| Action::Move(a, b)),
        (any::<u8>(), any::<bool>()).prop_map(|(a, b)| Action::Suppress(a, b)),
        proptest::option::of(any::<u8>()).prop_map(Action::Rollback),
        (any::<u8>(), any::<u8>()).prop_map(|(a, b)| Action::Group(a, b)),
        any::<u8>().prop_map(Action::Ungroup),
        Just(Action::Undo),
        Just(Action::Redo),
    ]
}

fn pick<T>(v: &[T], n: u8) -> Option<&T> {
    if v.is_empty() {
        None
    } else {
        v.get(n as usize % v.len())
    }
}

/// 操作を 1 つ試す。不正な操作は誤りになるだけで、文書は変わらない。
fn perform(doc: &mut Document, a: &Action) {
    let d = doc.data().clone();
    let op = match a {
        Action::Undo => {
            doc.undo();
            return;
        }
        Action::Redo => {
            doc.redo();
            return;
        }
        Action::AddParam(n, e) => {
            let id = doc.alloc_param_id();
            Op::AddParam {
                index: None,
                param: Param::new(id, format!("p{}", n % 4), e.clone()),
            }
        }
        Action::SetExpr(n, e) => match pick(&d.params, *n) {
            Some(p) => Op::SetParamExpr {
                id: p.id,
                expr: e.clone(),
            },
            None => return,
        },
        Action::Rename(n, m) => match pick(&d.params, *n) {
            Some(p) => Op::RenameParam {
                id: p.id,
                name: format!("p{}", m % 4),
            },
            None => return,
        },
        Action::RemoveParam(n) => match pick(&d.params, *n) {
            Some(p) => Op::RemoveParam { id: p.id },
            None => return,
        },
        Action::AddValue(i, e) => {
            let id = doc.alloc_feature_id();
            let entry = FeatureEntry {
                id,
                type_id: "test.value@1".into(),
                name: "v".into(),
                data: json!({"expr": e}),
                suppressed: false,
                pin_bodies: false,
            };
            Op::InsertFeature {
                index: i.map(|i| i as usize % (d.features.len() + 1)),
                entry,
            }
        }
        Action::AddDouble(n) => {
            let src = pick(&d.features, *n).map_or(0, |f| f.id.0);
            let id = doc.alloc_feature_id();
            let entry = FeatureEntry {
                id,
                type_id: "test.double@1".into(),
                name: "d".into(),
                data: json!({"source": src}),
                suppressed: false,
                pin_bodies: false,
            };
            Op::InsertFeature { index: None, entry }
        }
        Action::Remove(n) => match pick(&d.features, *n) {
            Some(f) => Op::RemoveFeature { id: f.id },
            None => return,
        },
        Action::Move(n, m) => match pick(&d.features, *n) {
            Some(f) => Op::MoveFeature {
                id: f.id,
                to: *m as usize % d.features.len(),
            },
            None => return,
        },
        Action::Suppress(n, s) => match pick(&d.features, *n) {
            Some(f) => Op::SetSuppressed {
                id: f.id,
                suppressed: *s,
            },
            None => return,
        },
        Action::Rollback(n) => Op::SetRollback {
            rollback: n.map(|n| n as usize % (d.features.len() + 1)),
        },
        Action::Group(a, b) => {
            let (Some(x), Some(y)) = (pick(&d.features, *a), pick(&d.features, *b)) else {
                return;
            };
            let (i, j) = (d.index_of(x.id).unwrap(), d.index_of(y.id).unwrap());
            let (first, last) = if i <= j { (x.id, y.id) } else { (y.id, x.id) };
            let id = doc.alloc_group_id();
            let mut groups = d.groups.clone();
            groups.push(Group {
                id,
                name: "g".into(),
                first,
                last,
            });
            Op::SetGroups { groups }
        }
        Action::Ungroup(n) => {
            let mut groups = d.groups.clone();
            if groups.is_empty() {
                return;
            }
            groups.remove(*n as usize % groups.len());
            Op::SetGroups { groups }
        }
    };
    let _ = doc.apply(Transaction::new("乱数", vec![op]));
}

/// 文書の比較用。`next_id` は取り消しても戻らないので除く。
fn content(d: &DocumentData) -> (Vec<Param>, Vec<FeatureEntry>, Option<usize>, Vec<Group>) {
    (
        d.params.clone(),
        d.features.clone(),
        d.rollback,
        d.groups.clone(),
    )
}

proptest! {
    #[test]
    fn undo_all_restores_and_redo_all_replays(actions in proptest::collection::vec(action_strategy(), 0..40)) {
        let mut doc = Document::new();
        let initial = content(doc.data());
        for a in &actions {
            perform(&mut doc, a);
        }
        // やり直せる変更が残っていれば先にやり直し、履歴の最後の状態にする
        while doc.redo() {}
        let last = content(doc.data());
        while doc.undo() {}
        prop_assert_eq!(content(doc.data()), initial);
        while doc.redo() {}
        prop_assert_eq!(content(doc.data()), last);
    }

    #[test]
    fn groups_stay_valid_through_any_edit(actions in proptest::collection::vec(action_strategy(), 0..40)) {
        let mut doc = Document::new();
        for a in &actions {
            perform(&mut doc, a);
            // どの変更の後も、読み込みの検査を通る(両端があり、一部だけ重ならない)
            prop_assert!(crate::transaction::validate(doc.data()).is_ok(), "{:?}", doc.data().groups);
        }
    }

    #[test]
    fn save_load_and_recompute_are_deterministic(actions in proptest::collection::vec(action_strategy(), 0..40)) {
        let reg = registry();
        let mut doc = Document::new();
        for a in &actions {
            perform(&mut doc, a);
        }
        let json = doc.to_json();
        let mut loaded = Document::from_json(&json).expect("保存した文書は必ず読み込める");
        prop_assert_eq!(loaded.data(), doc.data());
        prop_assert_eq!(loaded.to_json(), json);

        let strip = |mut r: RecomputeReport| {
            r.evaluated = 0;
            r.reused = 0;
            for f in &mut r.features {
                if let FeatureStatus::Ok { reused, .. } = &mut f.status {
                    *reused = false;
                }
            }
            r
        };
        let r1 = strip(doc.recompute(&reg, &NullKernel));
        let r2 = strip(doc.recompute(&reg, &NullKernel));
        let r3 = strip(loaded.recompute(&reg, &NullKernel));
        prop_assert_eq!(&r1, &r2);
        prop_assert_eq!(&r1, &r3);
    }
}

#[test]
fn shape_values_serialize() {
    let v = Value::Shape(ShapeHandle(7));
    let j = serde_json::to_value(&v).unwrap();
    assert_eq!(j, serde_json::json!({"kind": "shape", "handle": 7}));
    assert_eq!(serde_json::from_value::<Value>(j).unwrap(), v);
}

#[test]
fn descriptors_give_categories_and_read_and_write_nested_fields() {
    let mut r = Registry::new();
    r.register_described(
        Descriptor::new("t.a@1", category::SOLID, "A").field(Field::expr("stock.margin", "余白")),
        |_| Err(EvalError::Other("x".into())),
    );
    r.register("t.b@1", |_| Err(EvalError::Other("x".into())));
    assert_eq!(r.category("t.a@1"), Some(category::SOLID));
    assert_eq!(r.category("t.b@1"), Some(category::OTHER));
    assert_eq!(r.category("t.c@1"), None);
    assert_eq!(
        r.in_category(category::SOLID).collect::<Vec<_>>(),
        ["t.a@1"]
    );
    let d = r.descriptor("t.a@1").unwrap();
    let f = &d.fields[0];
    let mut data = serde_json::json!({});
    f.set(&mut data, Some(serde_json::json!("2 mm")));
    assert_eq!(data, serde_json::json!({"stock": {"margin": "2 mm"}}));
    assert_eq!(f.get(&data), Some(&serde_json::json!("2 mm")));
    f.set(&mut data, None);
    assert_eq!(data, serde_json::json!({"stock": {}}));
    let j = serde_json::to_value(&d).unwrap();
    assert_eq!(j["fields"][0]["kind"], "expr");
}

#[test]
fn fields_apply_only_when_their_condition_holds() {
    let only = Field::expr("w", "幅").only("mode", "fixed", "margin");
    let unless = Field::opt_output("t", "対象", "body").unless("op", "new");
    assert!(!only.applies(&json!({})));
    assert!(only.applies(&json!({"mode": "fixed"})));
    assert!(!unless.applies(&json!({})));
    assert!(unless.applies(&json!({"op": "join"})));
    let text = Field::text("m", "材料");
    assert!(matches!(text.kind, FieldKind::Text));
}

#[test]
fn a_template_is_filled_with_the_latest_candidates() {
    let d = Descriptor::new("t@1", category::SOLID, "試し")
        .field(Field::output("target", "対象", "body"))
        .field(Field::output("tool", "道具", "body"))
        .field(Field::opt_output("extra", "おまけ", "body"))
        .field(Field::output("sketch", "スケッチ", "sketch"))
        .template(serde_json::json!({"n": 1}));
    let cands = |slot: &str| match slot {
        "body" => vec![3, 5, 8],
        "sketch" => vec![2],
        _ => vec![],
    };
    let v = d.instantiate(&cands).unwrap();
    assert_eq!(
        v,
        serde_json::json!({"n": 1, "target": 5, "tool": 8, "sketch": 2})
    );
    let none = |_: &str| vec![];
    assert_eq!(d.instantiate(&none).unwrap_err(), "スケッチ");
    let one = |slot: &str| if slot == "body" { vec![3] } else { vec![2] };
    assert_eq!(d.instantiate(&one).unwrap_err(), "対象");
}

#[test]
fn edits_from_an_action_are_one_undo_step_with_new_ids() {
    let mut doc = Document::new();
    let a = add_feature(&mut doc, "test.value@1", json!({"expr": "1 mm"}));
    let added = doc
        .apply_edits(
            "操作",
            vec![
                Edit::Insert {
                    type_id: "test.value@1".into(),
                    name: "一つ目".into(),
                    data: json!({"expr": "2 mm"}),
                },
                Edit::SetData {
                    id: a,
                    data: json!({"expr": "3 mm"}),
                },
                Edit::Insert {
                    type_id: "test.value@1".into(),
                    name: "二つ目".into(),
                    data: json!({"expr": "4 mm"}),
                },
            ],
        )
        .unwrap();
    assert_eq!(added.len(), 2);
    assert!(added[0] != added[1] && added.iter().all(|id| *id != a));
    assert_eq!(doc.data().features.len(), 3);
    assert_eq!(doc.data().feature(a).unwrap().data["expr"], "3 mm");
    assert!(doc.undo());
    assert_eq!(doc.data().features.len(), 1);
    assert_eq!(doc.data().feature(a).unwrap().data["expr"], "1 mm");
}

#[test]
fn an_unknown_action_is_an_error_and_retargeting_sets_the_face() {
    let r = registry();
    let mut doc = Document::new();
    add_feature(&mut doc, "test.value@1", json!({"expr": "1 mm"}));
    let report = doc.recompute(&r, &NullKernel);
    let kernel: std::sync::Arc<dyn Kernel> = std::sync::Arc::new(NullKernel);
    let cx = Context::new(doc.data(), &report, &r, &kernel);
    assert!(Extensions::default().run("no.such", &cx).is_err());
    assert_eq!(cx.candidates("value").len(), 1);

    let mut d = json!({"plane": {"kind": "xy"}});
    let plane = Retarget::Plane {
        key: "plane".into(),
    };
    assert!(!plane.applies(&d), "面でない平面は付け替えない");
    d["plane"] = json!({"kind": "face", "feature": 1, "face": "f1:end"});
    assert!(plane.applies(&d));
    plane.apply(&mut d, 7, "f7:side");
    assert_eq!(
        d["plane"],
        json!({"kind": "face", "feature": 7, "face": "f7:side"})
    );
    // 面の上のスケッチの原点の決め方は残す
    let corner = json!({"at": "vertex", "faces": ["f7:side", "f7:start"]});
    d["plane"]["origin"] = corner.clone();
    plane.apply(&mut d, 8, "f8:end");
    assert_eq!(
        d["plane"],
        json!({"kind": "face", "feature": 8, "face": "f8:end", "origin": corner})
    );
    let mut e = json!({"faces": ["a"], "edges": [["a", "b"]]});
    Retarget::Faces {
        key: "faces".into(),
        clear: vec!["edges".into()],
    }
    .apply(&mut e, 1, "c");
    assert_eq!(e, json!({"faces": ["c"]}));
}

// 元は 2 本:
// - 値の包みは往復し、型の違うものは読まない。警告は以前と同じ形で入る
// - 警告は出力に足され、読み戻せる(空なら足さない)
#[test]
fn payloads_and_warnings_round_trip_and_other_types_are_refused() {
    use crate::{Payload, PointId, Points};
    let p = Points {
        plane: Plane::XY,
        points: [(PointId(3), [1.0, 2.0])].into(),
        lone: vec![PointId(3)],
    };
    let v = p.to_value();
    assert_eq!(Points::from_value(&v), Some(p));
    assert!(
        crate::Profile::from_value(&v).is_none(),
        "型の名前が違えば読まない"
    );
    assert!(Points::from_value(&Value::Scalar(crate::Quantity::scalar(1.0))).is_none());
    // 警告は以前と同じ形(文字列の配列)で入る
    let mut out = Outputs::new();
    crate::add_warnings(&mut out, vec!["a".into()]);
    crate::add_warnings(&mut out, vec!["b".into()]);
    let Some(Value::Custom { data, .. }) = out.get(crate::WARNINGS) else {
        unreachable!()
    };
    assert_eq!(data, &serde_json::json!(["a", "b"]));

    let mut out = Outputs::new();
    add_warnings(&mut out, vec![]);
    assert!(out.is_empty());
    add_warnings(&mut out, vec!["一つ目".into()]);
    add_warnings(&mut out, vec!["二つ目".into()]);
    assert_eq!(warnings_of(&out), vec!["一つ目", "二つ目"]);
}

#[test]
fn groups_follow_removed_and_moved_ends_and_refuse_partial_overlaps() {
    let mut doc = Document::new();
    let ids: Vec<FeatureId> = (0..4)
        .map(|i| add_feature(&mut doc, "test.value@1", json!({"expr": format!("{i}")})))
        .collect();
    let gid = doc.alloc_group_id();
    let g = |first, last| Group {
        id: gid,
        name: "土台".into(),
        first,
        last,
    };
    doc.apply(Transaction::new(
        "まとめる",
        vec![Op::SetGroups {
            groups: vec![g(ids[0], ids[2])],
        }],
    ))
    .unwrap();
    assert_eq!(doc.data().groups_of(ids[1]).len(), 1);
    assert!(doc.data().groups_of(ids[3]).is_empty());
    // 先頭を消すと、先頭が隣へ寄る。取り消すと戻る
    doc.apply(Transaction::new(
        "消す",
        vec![Op::RemoveFeature { id: ids[0] }],
    ))
    .unwrap();
    assert_eq!(doc.data().groups, vec![g(ids[1], ids[2])]);
    doc.undo();
    assert_eq!(doc.data().groups, vec![g(ids[0], ids[2])]);
    // 末尾を後ろへ動かすと、末尾が手前の中身へ寄る
    doc.apply(Transaction::new(
        "動かす",
        vec![Op::MoveFeature { id: ids[2], to: 3 }],
    ))
    .unwrap();
    assert_eq!(doc.data().groups, vec![g(ids[0], ids[1])]);
    doc.undo();
    // 一部だけ重なるグループは作れない
    let other = doc.alloc_group_id();
    let r = doc.apply(Transaction::new(
        "まとめる",
        vec![Op::SetGroups {
            groups: vec![
                g(ids[0], ids[2]),
                Group {
                    id: other,
                    name: "穴".into(),
                    first: ids[1],
                    last: ids[3],
                },
            ],
        }],
    ));
    assert!(matches!(r, Err(TxError::BadGroup(_))), "{r:?}");
    // 版 1 の文書(グループなし)も読める。保存すると今の版
    let v1 = r#"{"format_version": 1, "next_id": 1, "params": [], "features": []}"#;
    let d = Document::from_json(v1).unwrap();
    assert!(d.data().groups.is_empty());
    assert!(
        d.to_json()
            .contains(&format!("\"format_version\": {FORMAT_VERSION}"))
    );
}

#[test]
fn dedupe_orders_by_position() {
    let names = crate::kernel::dedupe_names(
        vec!["a".into(), "b".into(), "a".into()],
        &[[5.0, 0.0, 0.0], [0.0; 3], [1.0, 0.0, 0.0]],
    );
    assert_eq!(names, vec!["a#1", "b", "a#0"]);
}

/// 1 µm より近い座標の揺れで番号が入れ替わらない。y が 0 の切れ端が #0。
#[test]
fn dedupe_ignores_jitter_below_a_micrometre() {
    for (x0, x1) in [(0.000_499_9, 0.000_500_1), (0.000_500_1, 0.000_499_9)] {
        let names = crate::kernel::dedupe_names(
            vec!["a".into(), "a".into()],
            &[[x0, 50.0, 0.0], [x1, 0.0, 0.0]],
        );
        assert_eq!(names, vec!["a#1", "a#0"], "x {x0} と {x1}");
    }
    // 離れていれば今までどおり x の順
    let names = crate::kernel::dedupe_names(
        vec!["a".into(), "a".into()],
        &[[0.0, 50.0, 0.0], [0.01, 0.0, 0.0]],
    );
    assert_eq!(names, vec!["a#0", "a#1"]);
}

#[test]
fn a_body_ref_is_written_as_the_bare_feature_number() {
    let body = Ref::Body(FeatureId(5));
    assert_eq!(serde_json::to_value(&body).unwrap(), json!(5));
    let face = Ref::Face {
        feature: FeatureId(7),
        face: "f2:end".into(),
    };
    let v = serde_json::to_value(&face).unwrap();
    assert_eq!(v, json!({"kind": "face", "feature": 7, "face": "f2:end"}));
    for r in [body, face] {
        let back: Ref = serde_json::from_value(serde_json::to_value(&r).unwrap()).unwrap();
        assert_eq!(back, r);
    }
    // 種類つきのボディも読める
    let b: Ref = serde_json::from_value(json!({"kind": "body", "feature": 3})).unwrap();
    assert_eq!(b, Ref::Body(FeatureId(3)));
}

#[test]
fn a_pick_field_keeps_one_item_as_a_value_and_more_as_a_list() {
    let f = Field::pick("tool", "道具", &[RefKind::Body], 1, None);
    let mut d = json!({"tool": 5});
    assert_eq!(f.refs(&d), vec![Ref::Body(FeatureId(5))]);
    f.set_refs(&mut d, &[Ref::Body(FeatureId(5)), Ref::Body(FeatureId(8))]);
    assert_eq!(d, json!({"tool": [5, 8]}));
    f.set_refs(&mut d, &[Ref::Body(FeatureId(8))]);
    assert_eq!(d, json!({"tool": 8}));
    f.set_refs(&mut d, &[]);
    assert_eq!(d, json!({}));
    let one = Field::pick("target", "対象", &[RefKind::Body], 1, Some(1));
    assert_eq!(one.room(&json!({})), 1);
    assert_eq!(one.room(&json!({"target": 2})), 0);
}

fn picking_descriptor() -> Descriptor {
    Descriptor::new("t@1", category::SOLID, "試し")
        .field(Field::pick("target", "対象", &[RefKind::Body], 1, Some(1)))
        .field(Field::pick("tool", "道具", &[RefKind::Body], 1, None))
        .field(Field::pick(
            "faces",
            "面",
            &[RefKind::PlanarFace, RefKind::Edge],
            0,
            None,
        ))
        .template(json!({"operation": "cut"}))
}

#[test]
fn preselected_things_go_to_the_first_field_that_takes_them() {
    let d = picking_descriptor();
    let face = |f: u64, n: &str| Ref::Face {
        feature: FeatureId(f),
        face: n.into(),
    };
    let planar = |r: &Ref| matches!(r, Ref::Face { face, .. } if face != "f9:curved");
    let mut data = json!({});
    let refs = vec![
        Ref::Body(FeatureId(2)),
        Ref::Body(FeatureId(4)),
        Ref::Body(FeatureId(6)),
        face(4, "f4:end"),
        face(9, "f9:curved"),
        Ref::Vertex {
            feature: FeatureId(2),
            vertex: "a|b|c".into(),
        },
    ];
    let left = d.distribute(&mut data, &refs, &planar);
    // 対象は 1 つで埋まり、残りのボディは道具へ。平らな面は面の欄へ。曲面はそのボディとして
    // 道具へ。頂点のボディ 2 はもう対象に入っているので、どこにも入らない
    assert_eq!(data["target"], json!(2));
    assert_eq!(
        data["faces"],
        json!({"kind": "face", "feature": 4, "face": "f4:end"})
    );
    assert_eq!(data["tool"], json!([4, 6, 9]));
    assert_eq!(left, vec![refs[5].clone()]);
}

#[test]
fn things_no_field_takes_are_returned() {
    let d = Descriptor::new("t@1", category::SOLID, "試し").field(Field::pick(
        "target",
        "対象",
        &[RefKind::Body],
        1,
        Some(1),
    ));
    let mut data = json!({});
    let refs = vec![Ref::Body(FeatureId(2)), Ref::Body(FeatureId(3))];
    let left = d.distribute(&mut data, &refs, &|_| false);
    assert_eq!(data, json!({"target": 2}));
    assert_eq!(left, vec![Ref::Body(FeatureId(3))]);
}

#[test]
fn empty_body_fields_are_filled_with_the_latest_unused_bodies() {
    let d = picking_descriptor();
    let cands = |slot: &str| {
        if slot == "body" {
            vec![3, 5, 8]
        } else {
            vec![]
        }
    };
    // 何も選んでいない: 前と同じく、対象は 2 つ目に新しい物、道具はいちばん新しい物
    let (v, left, auto) = d.instantiate_with(&cands, &[], &|_| false).unwrap();
    assert_eq!(v, json!({"operation": "cut", "target": 5, "tool": 8}));
    assert!(left.is_empty());
    assert_eq!(auto, vec!["target", "tool"]);
    // 対象だけ先に選んだ: 道具は残りのいちばん新しい物
    let (v, _, auto) = d
        .instantiate_with(&cands, &[Ref::Body(FeatureId(8))], &|_| false)
        .unwrap();
    assert_eq!(v, json!({"operation": "cut", "target": 8, "tool": 5}));
    assert_eq!(auto, vec!["tool"]);
    let one = |slot: &str| if slot == "body" { vec![3] } else { vec![] };
    assert_eq!(
        d.instantiate_with(&one, &[], &|_| false).unwrap_err(),
        "対象"
    );
}

/// 面も受ける欄(シェルの「面/ボディ」)は面を選ぶのが主なので、ボディで埋めずに空で始める。
#[test]
fn face_or_body_fields_start_empty() {
    let d = Descriptor::new("t.shell@1", category::SOLID, "シェル").field(Field::pick(
        "faces",
        "面/ボディ",
        &[RefKind::Face, RefKind::Body],
        1,
        None,
    ));
    let cands = |slot: &str| if slot == "body" { vec![3] } else { vec![] };
    let (v, _, auto) = d.instantiate_with(&cands, &[], &|_| false).unwrap();
    assert!(
        v.get("faces")
            .is_none_or(|f| f.is_null() || f == &json!([])),
        "{v}"
    );
    assert!(auto.is_empty());
}

#[test]
fn vertices_are_named_by_the_faces_around_them() {
    let e = |a: [f64; 3], b: [f64; 3], f: [&str; 2]| EdgeInfo {
        curve: Vec::new(),
        faces: [f[0].into(), f[1].into()],
        a,
        b,
        convex: true,
        seam: Seam::Corner,
        arc: None,
    };
    // 上の面の四隅だけ(側面 4 枚と上面)。並べる順は名前の数を数として比べる(e2 < e10)
    let edges = vec![
        e([0.0, 0.0, 1.0], [1.0, 0.0, 1.0], ["f1:end", "f1:side:e1"]),
        e([1.0, 0.0, 1.0], [1.0, 1.0, 1.0], ["f1:end", "f1:side:e2"]),
        e([1.0, 1.0, 1.0], [0.0, 1.0, 1.0], ["f1:end", "f1:side:e3"]),
        e([0.0, 1.0, 1.0], [0.0, 0.0, 1.0], ["f1:end", "f1:side:e10"]),
        e(
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            ["f1:side:e10", "f1:side:e1"],
        ),
    ];
    let v = vertices_of_edges(&edges);
    assert_eq!(v.len(), 5);
    let names: Vec<&str> = v.iter().map(|x| x.name.as_str()).collect();
    assert!(
        names.contains(&"f1:end|f1:side:e1|f1:side:e10"),
        "{names:?}"
    );
    assert!(names.contains(&"f1:end|f1:side:e1|f1:side:e2"), "{names:?}");
    // 下の端は面が 2 つだけ
    assert!(names.contains(&"f1:side:e1|f1:side:e10"), "{names:?}");
    // 同じ面の組の角が 2 つあれば番号で分ける
    let twin = vec![
        e([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], ["a", "b"]),
        e([5.0, 0.0, 0.0], [6.0, 0.0, 0.0], ["a", "b"]),
    ];
    let names: Vec<String> = vertices_of_edges(&twin)
        .into_iter()
        .map(|x| x.name)
        .collect();
    assert_eq!(names, vec!["a|b", "a|b~1", "a|b~2", "a|b~3"]);
}

#[test]
fn rows_fields_are_reached_by_row_number_and_empty_rows_go_away() {
    use crate::{Descriptor, Field, RefKind};
    let d = Descriptor::new("t@1", "cad", "行").field(Field::rows(
        "rows",
        "エッジと半径",
        vec![
            Field::pick("edges", "辺", &[RefKind::Edge], 1, None),
            Field::expr("radius", "半径"),
        ],
    ));
    let mut data = serde_json::json!({"rows": [{"edges": 3, "radius": "1 mm"}]});
    // 行の欄は「並び.行番号.欄」で、名前は「選択セット N」。見出しは中身も書く(レビュー 2 回目 M7)
    let f = d.field_at("rows.0.radius").unwrap();
    assert_eq!(f.get(&data), Some(&serde_json::json!("1 mm")));
    assert_eq!(d.field_at("rows.1.edges").unwrap().label, "選択セット 2");
    let rows = d.field_at("rows").unwrap();
    let edges = serde_json::json!({"rows": [{"edges": [
        {"kind": "edge", "feature": 2, "faces": ["a", "b"]},
        {"kind": "edge", "feature": 2, "faces": ["b", "c"]},
    ], "radius": "1 mm"}]});
    assert_eq!(rows.row_title(0, &edges), "選択セット 1(辺 2・半径 1 mm)");
    assert_eq!(
        rows.row_title(0, &serde_json::json!({"rows": [{"radius": "2 mm"}]})),
        "選択セット 1(未選択・半径 2 mm)"
    );
    assert!(d.field_at("rows.x.edges").is_none());
    // まだ無い行は、前の行を写し、選ぶ欄を空にして足す
    let pick = d.field_at("rows.1.edges").unwrap();
    d.ensure_row(&mut data, &pick.key);
    assert_eq!(data["rows"][1], serde_json::json!({"radius": "1 mm"}));
    pick.set_refs(&mut data, &[crate::Ref::Body(FeatureId(5))]);
    assert_eq!(data["rows"][1]["edges"], 5);
    assert_eq!(d.pick_fields(&data).len(), 2);
    // 選ぶ欄が空の行は消す。最後の 1 行は残す
    d.field_at("rows.0.edges").unwrap().set_refs(&mut data, &[]);
    assert!(d.prune_rows(&mut data));
    assert_eq!(data["rows"].as_array().unwrap().len(), 1);
    assert_eq!(data["rows"][0]["edges"], 5);
    d.field_at("rows.0.edges").unwrap().set_refs(&mut data, &[]);
    assert!(!d.prune_rows(&mut data));
    assert_eq!(data["rows"].as_array().unwrap().len(), 1);
}

#[test]
fn a_face_name_tells_the_feature_that_made_it() {
    assert_eq!(face_feature("f7:end"), Some(FeatureId(7)));
    assert_eq!(face_feature("f12@2:side:e1"), Some(FeatureId(12)));
    assert_eq!(face_feature("f3@m:start"), Some(FeatureId(3)));
    assert_eq!(face_feature("origin_xy"), None);
    assert_eq!(face_feature("f:end"), None);
}

/// パターンのように、機能(いくつでも)と、足すボディ(空でもよい)を受ける機能。
fn pattern_like() -> Descriptor {
    Descriptor::new("p@1", category::SOLID, "並べる")
        .field(Field::pick("feature", "機能", &[RefKind::Feature], 1, None))
        .field(Field::pick(
            "body",
            "足すボディ",
            &[RefKind::Body],
            0,
            Some(1),
        ))
}

#[test]
fn a_preselected_face_gives_the_feature_that_made_it_and_its_body() {
    let d = pattern_like();
    // 板(3)の上の柱(5)。柱の面は、その後の穴(9)が受け継いだボディの面として選ばれる
    let face = |n: &str| Ref::Face {
        feature: FeatureId(9),
        face: n.into(),
    };
    let mut data = json!({});
    let left = d.distribute(&mut data, &[face("f5:side:e1"), face("f3:end")], &|_| true);
    assert!(left.is_empty(), "{left:?}");
    assert_eq!(data, json!({"feature": [5, 3], "body": 9}));
    // 機能は番号で持つのでボディと同じく受ける
    assert!(Ref::Body(FeatureId(5)).accepted_by(&[RefKind::Feature], &|_| false));
    // 何も選ばずに足す時は、道具の作り方を持つ機能の候補から
    let cands = |slot: &str| if slot == "recipe" { vec![5, 9] } else { vec![] };
    let (v, _, auto) = d.instantiate_with(&cands, &[], &|_| false).unwrap();
    assert_eq!(v, json!({"feature": 9}));
    assert_eq!(auto, vec!["feature"]);
}

/// 読んだボディの一部を使い切らない機能。
#[derive(Debug)]
struct Keeper {
    target: FeatureId,
    tool: FeatureId,
    keep: bool,
}

impl Feature for Keeper {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        Ok([self.target, self.tool]
            .map(|feature| InputRef::Output {
                feature,
                slot: "body".into(),
            })
            .into())
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        Ok([("body".to_string(), ctx.output(self.target, "body")?.clone())].into())
    }

    fn keeps(&self) -> Vec<FeatureId> {
        if self.keep { vec![self.tool] } else { vec![] }
    }
}

/// 形状の番号だけを出す箱(形はカーネルに作らせない)。
#[derive(Debug)]
struct FakeBox(u64);

impl Feature for FakeBox {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        Ok(vec![])
    }

    fn evaluate(&self, _: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        Ok([("body".to_string(), Value::Shape(ShapeHandle(self.0)))].into())
    }
}

#[test]
fn a_kept_tool_stays_visible() {
    for keep in [false, true] {
        let mut reg = Registry::new();
        reg.register("box@1", |d| Ok(Box::new(FakeBox(d["n"].as_u64().unwrap()))));
        reg.register("keep@1", |d| {
            Ok(Box::new(Keeper {
                target: FeatureId(d["target"].as_u64().unwrap()),
                tool: FeatureId(d["tool"].as_u64().unwrap()),
                keep: d["keep"].as_bool().unwrap(),
            }))
        });
        let mut doc = Document::new();
        let a = add_feature(&mut doc, "box@1", json!({"n": 1}));
        let t = add_feature(&mut doc, "box@1", json!({"n": 2}));
        let c = add_feature(
            &mut doc,
            "keep@1",
            json!({"target": a.0, "tool": t.0, "keep": keep}),
        );
        let report = doc.recompute(&reg, &kernel::NullKernel);
        let ids: Vec<FeatureId> = visible_bodies(doc.data(), &reg, &report)
            .into_iter()
            .map(|(f, _)| f)
            .collect();
        if keep {
            assert_eq!(ids, vec![t, c]);
            assert_eq!(body_heir(doc.data(), &reg, &report, t), t);
        } else {
            assert_eq!(ids, vec![c]);
            assert_eq!(body_heir(doc.data(), &reg, &report, t), c);
        }
    }
}

#[test]
fn a_param_unit_completes_bare_numbers_and_checks_the_dimension() {
    // U49: 単位の列。数だけの式に単位を補い、式の次元が合わなければ誤り。コメントも持つ
    let reg = registry();
    let mut doc = Document::new();
    let w = add_param(&mut doc, "w", "10");
    let status = |doc: &mut Document| {
        let r = doc.recompute(&reg, &NullKernel);
        r.params.iter().find(|p| p.id == w).unwrap().status.clone()
    };
    // 単位を決めないうちは、数だけなら単位のない数(前のままの扱い)
    assert_eq!(
        status(&mut doc),
        ParamStatus::Ok {
            value: Quantity {
                value: 10.0,
                dim: Dim::NONE
            }
        }
    );
    doc.apply(Transaction::new(
        "単位",
        vec![Op::SetParamInfo {
            id: w,
            comment: "板の幅".into(),
            unit: ParamUnit::Length,
        }],
    ))
    .unwrap();
    assert_eq!(
        status(&mut doc),
        ParamStatus::Ok {
            value: Quantity::mm(10.0)
        }
    );
    assert_eq!(doc.data().params[0].comment, "板の幅");
    // 角度にすると 10 は 10 deg。長さの式は誤り
    doc.apply(Transaction::new(
        "単位",
        vec![Op::SetParamInfo {
            id: w,
            comment: "板の幅".into(),
            unit: ParamUnit::Angle,
        }],
    ))
    .unwrap();
    doc.apply(Transaction::new(
        "式",
        vec![Op::SetParamExpr {
            id: w,
            expr: "3 mm".into(),
        }],
    ))
    .unwrap();
    match status(&mut doc) {
        ParamStatus::Error { message } => {
            assert!(
                message.contains("角度(deg)のパラメータに長さの式"),
                "{message}"
            )
        }
        s => panic!("{s:?}"),
    }
    // 取り消すと単位とコメントも戻る
    assert!(doc.undo());
    assert!(doc.undo());
    assert!(doc.undo());
    assert_eq!(doc.data().params[0].unit, ParamUnit::Any);
    assert!(doc.data().params[0].comment.is_empty());
    // 列ができる前のファイル(comment・unit なし)も読め、書くときも増やさない
    let json = serde_json::to_string(&doc.data().params[0]).unwrap();
    assert!(
        !json.contains("unit") && !json.contains("comment"),
        "{json}"
    );
    let old: Param = serde_json::from_str(r#"{"id":1,"name":"a","expr":"1"}"#).unwrap();
    assert_eq!(old.unit, ParamUnit::Any);
}

#[test]
fn sibling_paths_add_the_tail_to_the_chosen_name() {
    // M55: ボディごとの書き出しの名前
    let p = sibling_path(std::path::Path::new("/x/部品.stl"), "ボディ1.stl");
    assert_eq!(p, std::path::PathBuf::from("/x/部品-ボディ1.stl"));
    let p = sibling_path(std::path::Path::new("/x/a.step"), "ふた/上.step");
    assert_eq!(p, std::path::PathBuf::from("/x/a-ふた_上.step"));
}

#[test]
fn a_batch_is_rolled_back_to_its_start_or_squashed_into_one_undo() {
    let mut doc = Document::new();
    let a = add_param(&mut doc, "a", "1");
    let set = |doc: &mut Document, e: &str| {
        doc.apply(Transaction::new(
            format!("a={e}"),
            vec![Op::SetParamExpr {
                id: a,
                expr: e.into(),
            }],
        ))
        .unwrap();
    };
    let expr = |doc: &Document| doc.data().params[0].expr.clone();
    // 途中で失敗した: 始めの位置まで戻し、戻した分はやり直せない
    let start = doc.undo_depth();
    set(&mut doc, "2");
    set(&mut doc, "3");
    assert_eq!(doc.rollback_to(start), 2);
    assert_eq!(expr(&doc), "1");
    assert_eq!(doc.undo_depth(), start);
    assert!(!doc.can_redo());
    // 何も変えずに失敗した時は何もしない
    assert_eq!(doc.rollback_to(start), 0);
    // 通った: 1 つの取り消し単位にまとめ、1 回で戻せてやり直せる
    set(&mut doc, "4");
    set(&mut doc, "5");
    set(&mut doc, "6");
    assert_eq!(doc.squash_since(start, "まとめて 3 手"), 3);
    assert_eq!(doc.undo_labels().last(), Some(&"まとめて 3 手"));
    assert_eq!(doc.undo_depth(), start + 1);
    assert!(doc.undo());
    assert_eq!(expr(&doc), "1");
    assert!(doc.redo());
    assert_eq!(expr(&doc), "6");
    // 1 手だけなら元の名前のまま
    let start = doc.undo_depth();
    set(&mut doc, "7");
    assert_eq!(doc.squash_since(start, "まとめて"), 1);
    assert_eq!(doc.undo_labels().last(), Some(&"a=7"));
}

#[test]
fn param_views_are_saved_in_version_3_and_older_files_still_open() {
    // U50 段 B: 表は文書に持つ(版 3)。版 2 の文書(グループ・ボディの名前・コメントつき)も読める
    let v2 = r#"{
      "format_version": 2,
      "next_id": 5,
      "params": [{"id": 1, "name": "w", "expr": "10 mm", "comment": "幅"}],
      "features": [
        {"id": 2, "type_id": "test.value@1", "name": "V", "data": {"expr": "w"}},
        {"id": 3, "type_id": "test.value@1", "name": "U", "data": {"expr": "w * 2"}}
      ],
      "groups": [{"id": 4, "name": "G", "first": 2, "last": 3}],
      "body_names": {"2": "板"}
    }"#;
    let mut doc = Document::from_json(v2).unwrap();
    assert_eq!(doc.data().format_version, FORMAT_VERSION);
    assert!(doc.data().param_views.is_empty());
    assert_eq!(doc.data().params[0].comment, "幅");
    assert_eq!(doc.data().groups.len(), 1);
    // 表のない文書は、表の項目を書かない
    assert!(!doc.to_json().contains("param_views"));
    let id = doc.alloc_view_id();
    let view = ParamView {
        id,
        name: "外形".into(),
        members: vec![
            ParamKey::User { id: ParamId(1) },
            ParamKey::Field {
                feature: FeatureId(3),
                key: "expr".into(),
            },
        ],
        body: Some(FeatureId(2)),
        hidden: vec![ParamKey::Model {
            feature: FeatureId(2),
            key: "C1".into(),
        }],
        favorites: false,
    };
    doc.apply(Transaction::new(
        "表",
        vec![Op::SetParamViews {
            views: vec![view.clone()],
        }],
    ))
    .unwrap();
    let saved = doc.to_json();
    let now = format!("\"format_version\": {FORMAT_VERSION}");
    assert!(saved.contains(&now), "{saved}");
    let back = Document::from_json(&saved).unwrap();
    assert_eq!(back.data().param_views, vec![view]);
    // 取り消すと表はなくなる
    assert!(doc.undo());
    assert!(doc.data().param_views.is_empty());
    // 番号が重なる・割り当て前・名前が空の表は断る
    for bad in [
        ParamView {
            id: 2,
            name: "x".into(),
            members: vec![],
            body: None,
            hidden: vec![],
            favorites: false,
        },
        ParamView {
            id: 99,
            name: "x".into(),
            members: vec![],
            body: None,
            hidden: vec![],
            favorites: false,
        },
        ParamView {
            id,
            name: " ".into(),
            members: vec![],
            body: None,
            hidden: vec![],
            favorites: false,
        },
    ] {
        let r = doc.apply(Transaction::new(
            "表",
            vec![Op::SetParamViews { views: vec![bad] }],
        ));
        assert!(r.is_err(), "{r:?}");
    }
    // 新しい版の文書は読まない
    let next = FORMAT_VERSION + 1;
    let newer = saved.replace(&now, &format!("\"format_version\": {next}"));
    assert!(matches!(
        Document::from_json(&newer),
        Err(LoadError::UnsupportedVersion(v)) if v == next
    ));
}

// 元は 2 本:
// - 属性と名前つきの視点を版 3 に足し、前の版 3 と版 2 の文書も開ける
// - 消したパラメータとお気に入りの表を読み、番号の重なりを断る
#[test]
fn fields_added_to_version_3_load_and_older_files_still_open() {
    // U13・U20: 版 3 のまま、無ければ空の項目として足した。前の版 3 と版 2 の文書も開ける
    for old in [
        r#"{"format_version": 3, "next_id": 2, "params": [], "features": [
            {"id": 1, "type_id": "test.value@1", "name": "V", "data": {"expr": "1"}}]}"#,
        r#"{"format_version": 2, "next_id": 2, "params": [], "features": [
            {"id": 1, "type_id": "test.value@1", "name": "V", "data": {"expr": "1"}}]}"#,
    ] {
        let doc = Document::from_json(old).unwrap();
        assert!(doc.data().properties.is_empty());
        assert!(doc.data().named_views.is_empty());
        // 空の間は項目を書かない(前のアプリでも開ける)
        let saved = doc.to_json();
        assert!(!saved.contains("properties") && !saved.contains("named_views"));
    }
    let mut doc = Document::from_json(
        r#"{"format_version": 3, "next_id": 2, "params": [], "features": [
            {"id": 1, "type_id": "test.value@1", "name": "V", "data": {"expr": "1"}}]}"#,
    )
    .unwrap();
    let mut props = Properties::default();
    props.set("title", "ブラケット");
    props.set("part_number", " JC-001 ");
    props.set("検査", "済み");
    assert_eq!(props.part_number, "JC-001", "前後の空白は落とす");
    assert_eq!(props.custom, vec![("検査".to_string(), "済み".to_string())]);
    let id = doc.alloc_view_id();
    let view = NamedView {
        id,
        name: "正面の穴".into(),
        target: [10.0, 0.0, 5.0],
        yaw: -90.0,
        pitch: 0.0,
        roll: 0.0,
        height: 80.0,
        perspective: false,
    };
    doc.apply(Transaction::new(
        "属性と視点",
        vec![
            Op::SetProperties {
                properties: Box::new(props.clone()),
            },
            Op::SetNamedViews {
                views: vec![view.clone()],
            },
        ],
    ))
    .unwrap();
    let saved = doc.to_json();
    let now = format!("\"format_version\": {FORMAT_VERSION}");
    assert!(saved.contains(&now), "{saved}");
    let back = Document::from_json(&saved).unwrap();
    assert_eq!(back.data().properties, props);
    assert_eq!(back.data().named_views, vec![view.clone()]);
    // 1 回の取り消しで両方戻る
    assert!(doc.undo());
    assert!(doc.data().properties.is_empty() && doc.data().named_views.is_empty());
    // 自分で足した項目は空にすると消える
    props.set("検査", "");
    assert!(props.custom.is_empty());
    // 番号が重なる・名前が空・高さが 0・ピッチが範囲外の視点は断る
    for bad in [
        NamedView {
            id: 1,
            ..view.clone()
        },
        NamedView {
            name: " ".into(),
            ..view.clone()
        },
        NamedView {
            height: 0.0,
            ..view.clone()
        },
        NamedView {
            pitch: 120.0,
            ..view.clone()
        },
    ] {
        let r = doc.apply(Transaction::new(
            "視点",
            vec![Op::SetNamedViews { views: vec![bad] }],
        ));
        assert!(r.is_err(), "{r:?}");
    }

    // 版 3 のまま足した欄: 消したパラメータ(N2)とお気に入りの表(U52)。無い文書も、ある文書も開ける
    let plain = r#"{"format_version": 3, "next_id": 2,
        "params": [{"id": 1, "name": "w", "expr": "10 mm"}], "features": []}"#;
    let doc = Document::from_json(plain).unwrap();
    assert!(doc.data().removed_params.is_empty());
    assert!(!doc.to_json().contains("removed_params"));
    let with = r#"{"format_version": 3, "next_id": 4,
        "params": [{"id": 1, "name": "w", "expr": "10 mm"}], "features": [],
        "param_views": [{"id": 3, "name": "お気に入り", "members": [{"kind": "user", "id": 1}], "favorites": true}],
        "removed_params": [{"id": 2, "name": "w", "expr": "5 mm"}]}"#;
    let doc = Document::from_json(with).unwrap();
    assert_eq!(doc.data().removed_param(ParamId(2)).unwrap().expr, "5 mm");
    assert!(doc.data().favorites_view().is_some());
    let back = Document::from_json(&doc.to_json()).unwrap();
    assert_eq!(back.data(), doc.data());
    // 消した物の番号が今のパラメータと重なる文書は断る
    let bad = with.replace(r#""id": 2, "name": "w""#, r#""id": 1, "name": "w""#);
    assert!(Document::from_json(&bad).is_err());
}

/// 評価に 50 ms かかり、計算したスレッドを覚える([`CONCURRENT`] の試験用)
struct SlowFeature(String);
impl Feature for SlowFeature {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        expr_inputs(&self.0)
    }
    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        std::thread::sleep(std::time::Duration::from_millis(50));
        SLOW_THREADS
            .lock()
            .unwrap()
            .push(std::thread::current().id());
        Ok([("value".to_string(), Value::Scalar(ctx.expr(&self.0)?))].into())
    }
}

static SLOW_THREADS: std::sync::Mutex<Vec<std::thread::ThreadId>> =
    std::sync::Mutex::new(Vec::new());

fn concurrent_registry() -> Registry {
    let mut r = registry();
    r.register("test.slow@1", |d| {
        Ok(Box::new(SlowFeature(
            d["expr"].as_str().unwrap_or("0").to_string(),
        )))
    });
    for t in ["test.slow@1", "test.double@1", "test.panic@1"] {
        r.mark(t, CONCURRENT);
    }
    r
}

#[test]
fn concurrent_features_match_sequential_results() {
    let mut doc = Document::new();
    add_param(&mut doc, "a", "2 mm");
    let s: Vec<FeatureId> = (1..=4)
        .map(|k| add_feature(&mut doc, "test.slow@1", json!({"expr": format!("a * {k}")})))
        .collect();
    // 待っている機能を読む機能は、待ちが済んでから計算する
    let d = add_feature(&mut doc, "test.double@1", json!({"source": s[3].0}));
    let p = add_feature(&mut doc, "test.panic@1", json!({}));
    // 形を作る種類(印なし)の後ろも同じ
    let v = add_feature(&mut doc, "test.value@1", json!({"expr": "a + 1 mm"}));
    let d2 = add_feature(&mut doc, "test.double@1", json!({"source": v.0}));

    SLOW_THREADS.lock().unwrap().clear();
    let r = doc.recompute(&concurrent_registry(), &NullKernel);
    let threads: std::collections::HashSet<_> = SLOW_THREADS.lock().unwrap().drain(..).collect();
    for (k, id) in s.iter().enumerate() {
        assert_eq!(scalar(&r, *id), Quantity::mm(2.0 * (k + 1) as f64));
    }
    assert_eq!(scalar(&r, d), Quantity::mm(16.0));
    assert_eq!(scalar(&r, d2), Quantity::mm(6.0));
    assert!(matches!(r.feature(p), Some(FeatureStatus::Error { .. })));
    assert_eq!(r.evaluated, 8);
    // 結果は機能の順
    let order: Vec<FeatureId> = r.features.iter().map(|f| f.id).collect();
    let mut want = s.clone();
    want.extend([d, p, v, d2]);
    assert_eq!(order, want);
    if std::thread::available_parallelism().map_or(1, |n| n.get()) > 1 {
        assert!(threads.len() > 1, "別々のスレッドで計算していない");
    }

    // 2 回目は全部キャッシュから
    let r = doc.recompute(&concurrent_registry(), &NullKernel);
    assert_eq!(r.evaluated, 1, "panic の機能だけ計算し直す");
    assert_eq!(scalar(&r, d), Quantity::mm(16.0));
}

#[test]
fn the_document_unit_is_kept_in_the_properties_and_mm_is_not_written() {
    // 候補 19: 文書の長さの単位は設計の属性に持ち、mm(既定)の間は書かない
    let mut p = Properties::default();
    assert_eq!(p.units, LengthUnit::Mm);
    assert_eq!(p.get("units"), Some("mm"));
    p.set("units", "inch");
    assert_eq!(p.units, LengthUnit::In);
    p.set("units", "yd"); // 読めない名前は変えない(ft は 2026-10-22 に読めるようにした)
    assert_eq!(p.units, LengthUnit::In);
    let doc = Document::with_properties(p.clone());
    assert!(
        !doc.can_undo(),
        "新しい文書の既定の値は取り消しの履歴に入れない"
    );
    let saved = doc.to_json();
    assert!(saved.contains("\"units\": \"in\""), "{saved}");
    assert_eq!(Document::from_json(&saved).unwrap().data().properties, p);
    p.set("units", "");
    assert!(p.is_empty());
    assert!(!Document::with_properties(p).to_json().contains("units"));
}

/// 前は知らない上の階の項目を読み捨て、保存し直すと消えた。版 3 のまま項目を足すと(属性・
/// 名前つきの視点・部品の木・アニメーションがそうだった)、少し古いアプリで開いて保存した時に
/// 新しいアプリの項目が黙って消える。知らない項目は読んだまま書き戻す。
#[test]
fn unknown_top_level_fields_survive_a_load_and_save() {
    let newer = r#"{"format_version": 3, "next_id": 2, "params": [], "features": [
        {"id": 1, "type_id": "test.value@1", "name": "V", "data": {"expr": "1", "later": [1, 2]}}],
        "drawings": [{"name": "図面 1", "sheets": 2}], "z_extra": {"a": true}}"#;
    let mut doc = Document::from_json(newer).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&doc.to_json()).unwrap();
    assert_eq!(
        saved["drawings"],
        serde_json::json!([{"name": "図面 1", "sheets": 2}]),
        "{saved}"
    );
    assert_eq!(saved["z_extra"], serde_json::json!({"a": true}));
    // 機能のデータの知らない欄も残る
    assert_eq!(
        saved["features"][0]["data"]["later"],
        serde_json::json!([1, 2])
    );
    // 知っている項目を二重に書かない
    let text = doc.to_json();
    assert_eq!(text.matches("\"features\"").count(), 1, "{text}");
    // 編集してからの保存でも残る
    add_param(&mut doc, "w", "1 mm");
    let again: serde_json::Value = serde_json::from_str(&doc.to_json()).unwrap();
    assert_eq!(again["drawings"], saved["drawings"]);
    // 読み直しても同じ
    let back = Document::from_json(&doc.to_json()).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&back.to_json()).unwrap(),
        again
    );
}

/// 引数を宣言した操作の試験用のモジュール(`test.size` は `size` を宣言して読み、`test.sloppy` は
/// 宣言しない `extra` を読み、`test.hidden` は一覧に出さない)。
struct ArgModule;

impl Extension for ArgModule {
    fn actions(&self, _cx: &Context<'_>) -> Vec<crate::Action> {
        vec![
            crate::Action::new("test.size", "大きさ", "test")
                .arg(crate::Arg::length("size", "大きさ").default(2.0)),
            crate::Action::new("test.sloppy", "読みすぎ", "test"),
        ]
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, Problem>> {
        let msg = match id {
            "test.size" => format!("{:?}", cx.arg_f64("size")),
            "test.sloppy" => format!("{:?}", cx.arg("extra")),
            "test.hidden" => format!("{:?}", cx.arg("at")),
            "test.unknown_hidden" => String::new(),
            _ => return None,
        };
        Some(Ok(Outcome::Message(msg)))
    }

    fn unlisted(&self, id: &str) -> Option<Vec<crate::Arg>> {
        (id == "test.hidden").then(|| vec![crate::Arg::vector("at", "位置")])
    }
}

#[test]
fn actions_check_their_arguments_against_the_declaration() {
    let r = registry();
    let mut doc = Document::new();
    let report = doc.recompute(&r, &NullKernel);
    let kernel: std::sync::Arc<dyn Kernel> = std::sync::Arc::new(NullKernel);
    let ext = Extensions(vec![std::sync::Arc::new(ArgModule)]);
    let run = |id: &str, args: serde_json::Value| {
        let cx = Context {
            args: &args,
            ..Context::new(doc.data(), &report, &r, &kernel)
        };
        ext.run(id, &cx).map(|o| match o {
            Outcome::Message(m) => m,
            _ => unreachable!(),
        })
    };
    // 省くと宣言の既定、単位つきの式は mm に直る
    assert_eq!(run("test.size", json!(null)).unwrap(), "Ok(Some(2.0))");
    assert_eq!(
        run("test.size", json!({"size": "1 in"})).unwrap(),
        "Ok(Some(25.4))"
    );
    // 知らない鍵(打ち間違い)と型の違いは誤り。前は黙って捨てて既定で動いた
    let e = run("test.size", json!({"sise": 3})).unwrap_err();
    assert!(
        e.message.contains("引数 sise はありません") && e.message.contains("size"),
        "{e}"
    );
    assert!(run("test.size", json!({"size": true})).is_err());
    // 宣言しない鍵を読む操作は、作りの誤りとして試験で落ちる(debug の組み立て)
    let e = run("test.sloppy", json!(null)).unwrap_err();
    assert!(e.message.contains("宣言していない引数 extra"), "{e}");
    // 一覧に出さない操作は unlisted の宣言で確かめる。宣言の無い物(段の帯の段など、値を ID に
    // 付ける物)は引数を受け取らない
    assert_eq!(
        run("test.hidden", json!({"at": [1, 2, 3]})).unwrap(),
        "Some(Array [Number(1), Number(2), Number(3)])"
    );
    assert!(run("test.hidden", json!({"other": 1})).is_err());
    assert_eq!(run("test.unknown_hidden", json!(null)).unwrap(), "");
    let e = run("test.unknown_hidden", json!({"x": 1})).unwrap_err();
    assert!(e.message.contains("引数を受け取りません"), "{e}");
    assert_eq!(e.kind, ProblemKind::Input);
    // 知らない操作は入力の誤り、宣言しない鍵を読んだのは作りの誤りなので計算の誤り
    let e = run("no.such", json!(null)).unwrap_err();
    assert!(e.message.contains("ありません") && e.kind == ProblemKind::Input);
    let e = run("test.sloppy", json!(null)).unwrap_err();
    assert_eq!(e.kind, ProblemKind::Compute);
}

/// モジュールの共有の値は型で引き、組み立てた表ごとに別。同じ型を二度置くのは誤り(黙って
/// 片方を捨てない)。
#[test]
fn services_are_found_by_type_and_belong_to_one_registry() {
    struct Library(u32);
    let mut a = Registry::new();
    assert!(a.service::<Library>().is_none());
    a.provide(std::sync::Arc::new(Library(7)));
    assert_eq!(a.service::<Library>().map(|l| l.0), Some(7));
    assert!(a.service::<String>().is_none());
    assert!(Registry::new().service::<Library>().is_none());
    let twice = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        a.provide(std::sync::Arc::new(Library(8)))
    }));
    assert!(twice.is_err());
}

/// 試験用の区画(版 2。版 1 は `text` 1 つ、版 2 は `lines` の並び)。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
struct Notes {
    #[serde(default)]
    lines: Vec<String>,
    #[serde(flatten)]
    rest: serde_json::Map<String, serde_json::Value>,
}

impl SectionType for Notes {
    const KEY: &'static str = "test.notes";
    const VERSION: u32 = 2;
    const LABEL: &'static str = "メモ";

    fn migrate(from: u32, data: &mut serde_json::Value) {
        if from == 1
            && let Some(m) = data.as_object_mut()
            && let Some(t) = m.remove("text")
        {
            m.insert("lines".into(), json!([t]));
        }
    }

    fn check(&self) -> Result<(), String> {
        match self.lines.iter().any(|l| l.is_empty()) {
            true => Err("空の行".into()),
            false => Ok(()),
        }
    }

    fn is_empty(&self) -> bool {
        self.lines.is_empty() && self.rest.is_empty()
    }
}

fn notes(lines: &[&str]) -> Notes {
    Notes {
        lines: lines.iter().map(|l| l.to_string()).collect(),
        ..Default::default()
    }
}

/// 段 3: モジュールの区画は 1 つの変更で書き、取り消しで戻り、保存と読み直しで残る。空にすると外れる。
#[test]
fn a_section_is_set_undone_and_saved_through_one_generic_change() {
    let mut doc = Document::new();
    assert_eq!(doc.data().section::<Notes>(), Ok(Notes::default()));
    let op = doc.data().section_op(&notes(&["a", "b"])).unwrap();
    doc.apply(Transaction::new("メモ", vec![op])).unwrap();
    assert_eq!(doc.data().section::<Notes>(), Ok(notes(&["a", "b"])));
    let saved: serde_json::Value = serde_json::from_str(&doc.to_json()).unwrap();
    assert_eq!(
        saved["sections"]["test.notes"],
        json!({"version": 2, "data": {"lines": ["a", "b"]}})
    );
    let back = Document::from_json(&doc.to_json()).unwrap();
    assert_eq!(back.data().section::<Notes>(), Ok(notes(&["a", "b"])));
    doc.undo();
    assert!(doc.data().sections.is_empty());
    assert!(!doc.to_json().contains("sections"), "空の区画は書かない");
    doc.redo();
    let op = doc.data().section_op(&Notes::default()).unwrap();
    doc.apply(Transaction::new("消す", vec![op])).unwrap();
    assert!(doc.data().sections.is_empty(), "空の値で区画が外れる");
    // 決まりに外れる値と、鍵の決まりに外れる変更は誤り
    assert!(doc.data().section_op(&notes(&[""])).is_err());
    let bad = Op::SetSection {
        key: "Test Notes".into(),
        section: None,
    };
    assert!(doc.apply(Transaction::new("鍵", vec![bad])).is_err());
}

/// 区画の版はモジュールが持つ。古い版は読む時に移し、新しい版は読まず上書きもしない。知らない区画と
/// 区画の中の知らない項目は、読んで保存し直しても残る。
#[test]
fn sections_migrate_refuse_newer_versions_and_keep_what_they_do_not_know() {
    let text = r#"{"format_version": 4, "next_id": 1, "params": [], "features": [],
        "sections": {
          "test.notes": {"version": 1, "data": {"text": "古い", "later": 7}},
          "other.module": {"version": 3, "data": {"x": [1, 2]}}
        }}"#;
    let mut doc = Document::from_json(text).unwrap();
    let n = doc.data().section::<Notes>().unwrap();
    assert_eq!(n.lines, ["古い"]);
    assert_eq!(
        n.rest.get("later"),
        Some(&json!(7)),
        "区画の中の知らない項目"
    );
    let mut more = n.clone();
    more.lines.push("新しい".into());
    let op = doc.data().section_op(&more).unwrap();
    doc.apply(Transaction::new("足す", vec![op])).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&doc.to_json()).unwrap();
    assert_eq!(
        saved["sections"]["test.notes"],
        json!({"version": 2, "data": {"lines": ["古い", "新しい"], "later": 7}})
    );
    assert_eq!(
        saved["sections"]["other.module"],
        json!({"version": 3, "data": {"x": [1, 2]}}),
        "知らない区画はそのまま"
    );

    let newer = text.replace(r#""version": 1"#, r#""version": 9"#);
    let doc = Document::from_json(&newer).unwrap();
    assert!(matches!(
        doc.data().section::<Notes>(),
        Err(SectionError::Newer {
            version: 9,
            known: 2,
            ..
        })
    ));
    let e = doc.data().section_op(&notes(&["x"])).unwrap_err();
    assert!(e.contains("新しいアプリ"), "{e}");
    let broken = text.replace(r#""text": "古い""#, r#""lines": 5"#);
    let doc = Document::from_json(&broken).unwrap();
    assert!(matches!(
        doc.data().section::<Notes>(),
        Err(SectionError::Invalid { .. })
    ));
    // 読めない区画は、空として読んだ値で上書きしない
    let e = doc.data().section_op(&notes(&["x"])).unwrap_err();
    assert!(e.contains("読めません"), "{e}");
}

/// 版 3 → 4: 上の階のモーション スタディと接触セットは、組立のモジュールの区画へ移る(区画の版 1)。
#[test]
fn version_3_motion_studies_and_contacts_move_into_sections() {
    let v3 = r#"{"format_version": 3, "next_id": 1, "params": [], "features": [],
        "motion_studies": [{"name": "モーション スタディ1", "keys": []}],
        "contacts": {"enabled": true, "sets": []}}"#;
    let doc = Document::from_json(v3).unwrap();
    assert_eq!(doc.data().format_version, FORMAT_VERSION);
    let saved: serde_json::Value = serde_json::from_str(&doc.to_json()).unwrap();
    assert_eq!(saved.get("motion_studies"), None);
    assert_eq!(saved.get("contacts"), None);
    assert_eq!(
        saved["sections"]["asm.motion"],
        json!({"version": 1, "data": {"studies": [{"name": "モーション スタディ1", "keys": []}]}})
    );
    assert_eq!(
        saved["sections"]["asm.contacts"],
        json!({"version": 1, "data": {"enabled": true, "sets": []}})
    );
    // どちらも無い版 3 の文書は区画を作らない
    let plain = r#"{"format_version": 3, "next_id": 1, "params": [], "features": []}"#;
    assert!(
        Document::from_json(plain)
            .unwrap()
            .data()
            .sections
            .is_empty()
    );
}

/// 版 4 → 5: 上の階の構成表・材料・外観(部品の外観も)・アニメーションの段は、それぞれの
/// モジュールの区画へ移る(区画の版 1)。中身の形は同じで、外観と材料は 1 つの区画にまとまる。
#[test]
fn version_4_tables_move_into_sections() {
    let v4 = r#"{"format_version": 4, "next_id": 9, "params": [], "features": [],
        "body_materials": {"3": "アルミ"},
        "body_appearances": {"3": " 赤", "4": ""},
        "appearances": {"defs": [{"name": "濃紺", "category": "塗装", "color": [16, 32, 48], "roughness": 0.5}],
                        "faces": {"3": {"f3:end": "青"}}},
        "components": [{"id": 5, "name": "蓋", "bodies": [3], "appearance": "メープル"},
                       {"id": 6, "name": "箱"}],
        "animation": [{"kind": {"view": {"view": {"target": [0, 0, 0], "yaw": 0, "pitch": 0, "height": 10, "perspective": false}}}, "seconds": 1}],
        "configurations": {"columns": [], "rows": [{"name": "小", "cells": []}], "active": "小"},
        "sections": {"asm.motion": {"version": 1, "data": {"studies": []}}}}"#;
    let doc = Document::from_json(v4).unwrap();
    assert_eq!(doc.data().format_version, FORMAT_VERSION);
    let saved: serde_json::Value = serde_json::from_str(&doc.to_json()).unwrap();
    for gone in [
        "body_materials",
        "body_appearances",
        "appearances",
        "animation",
        "configurations",
    ] {
        assert_eq!(saved.get(gone), None, "{gone}");
    }
    assert_eq!(saved["components"][0].get("appearance"), None);
    let sec = &saved["sections"];
    assert_eq!(
        sec["cad.materials"],
        json!({"version": 1, "data": {"bodies": {"3": "アルミ"}}})
    );
    assert_eq!(sec["render.appearances"]["version"], json!(1));
    let looks = &sec["render.appearances"]["data"];
    assert_eq!(looks["bodies"], json!({"3": "赤"}));
    assert_eq!(looks["components"], json!({"5": "メープル"}));
    assert_eq!(looks["faces"], json!({"3": {"f3:end": "青"}}));
    assert_eq!(looks["defs"][0]["name"], json!("濃紺"));
    assert_eq!(
        sec["asm.animation"]["data"]["steps"][0]["seconds"],
        json!(1)
    );
    assert_eq!(sec["config.table"]["data"]["active"], json!("小"));
    // 前からあった区画は残る
    assert_eq!(
        sec["asm.motion"],
        json!({"version": 1, "data": {"studies": []}})
    );
    // どれも無い版 4 の文書は区画を作らない
    let plain = r#"{"format_version": 4, "next_id": 1, "params": [], "features": []}"#;
    assert!(
        Document::from_json(plain)
            .unwrap()
            .data()
            .sections
            .is_empty()
    );
}

#[test]
#[should_panic(expected = "二重に名乗られました")]
fn a_section_key_is_declared_once() {
    let mut r = Registry::new();
    r.declare_section::<Notes>();
    r.declare_section::<Notes>();
}

#[test]
fn removing_a_row_keeps_param_views_on_the_same_rows() {
    let src = r#"{
      "format_version": 2,
      "next_id": 5,
      "params": [],
      "features": [
        {"id": 2, "type_id": "test.value@1", "name": "F",
         "data": {"sets": [{"r": "1 mm"}, {"r": "2 mm"}, {"r": "3 mm"}]}}
      ]
    }"#;
    let mut doc = Document::from_json(src).unwrap();
    let key = |row: usize| ParamKey::Field {
        feature: FeatureId(2),
        key: format!("sets.{row}.r"),
    };
    let id = doc.alloc_view_id();
    let view = ParamView {
        id,
        name: "お気に入り".into(),
        members: vec![key(1), key(2)],
        body: None,
        hidden: vec![],
        favorites: true,
    };
    doc.apply(Transaction::new(
        "表",
        vec![Op::SetParamViews { views: vec![view] }],
    ))
    .unwrap();
    // 2 行目を消すと、2 行目を指す鍵は外れ、3 行目を指す鍵は新しい 2 行目を指す
    let removed = serde_json::json!({"sets": [{"r": "1 mm"}, {"r": "3 mm"}]});
    doc.apply(Transaction::new(
        "行を消す",
        vec![Op::SetFeatureData {
            id: FeatureId(2),
            data: removed,
        }],
    ))
    .unwrap();
    assert_eq!(doc.data().param_views[0].members, vec![key(1)]);
    // 取り消すと元の行に戻り、やり直すと同じ付け替えになる
    assert!(doc.undo());
    assert_eq!(doc.data().param_views[0].members, vec![key(1), key(2)]);
    assert!(doc.redo());
    assert_eq!(doc.data().param_views[0].members, vec![key(1)]);
    // 行を足し直すと、後ろの行を指す鍵も後ろへずれる
    let added = serde_json::json!({"sets": [{"r": "0 mm"}, {"r": "1 mm"}, {"r": "3 mm"}]});
    doc.apply(Transaction::new(
        "行を足す",
        vec![Op::SetFeatureData {
            id: FeatureId(2),
            data: added,
        }],
    ))
    .unwrap();
    assert_eq!(doc.data().param_views[0].members, vec![key(2)]);
}

/// 選択肢に原点の平面がある欄は、機能が印を付けなくても原点の平面の欄になる(ビューに平面を出す)
#[test]
fn a_choice_of_origin_planes_is_an_origin_plane_field_by_itself() {
    let planes = Field::choice(
        "plane_ref.kind",
        "平面",
        &[
            ("vector", "点と法線"),
            ("origin_yz", "原点の YZ 平面"),
            ("face", "面"),
        ],
    );
    assert!(planes.origin_planes);
    assert_eq!(
        planes.origin_plane_of(&serde_json::json!({"plane_ref": {"kind": "origin_yz"}})),
        Some(1)
    );
    assert!(!Field::choice("op", "操作", &[("new", "新規"), ("join", "結合")]).origin_planes);
}

/// 確かめること: ▲▼ の刻みは数だけの値を 1 つずつ変え、下限より下げず、式は書き換えない。
#[test]
fn step_changes_plain_numbers_only() {
    let s = Field::expr("count", "個数")
        .unitless()
        .step(1.0, 1.0)
        .step
        .unwrap();
    assert_eq!(s.next("3", true).as_deref(), Some("4"));
    assert_eq!(s.next(" 3 ", false).as_deref(), Some("2"));
    assert_eq!(s.next("1", false), None);
    assert_eq!(s.next("n * 2", true), None);
    assert_eq!(s.next("", true), None);
    let fine = Step { by: 0.1, min: 0.0 };
    assert_eq!(fine.next("0.2", true).as_deref(), Some("0.3"));
}
