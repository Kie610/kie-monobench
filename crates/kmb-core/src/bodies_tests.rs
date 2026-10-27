//! ボディの ID と、機能が読むボディを「その手前の最新の形」に解く読み方の試験。

use crate::*;
use serde_json::json;

/// 形の番号 `n` のボディを出す箱。
struct Block(u64);

impl Feature for Block {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        Ok(vec![])
    }

    fn evaluate(&self, _: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        Ok([("body".to_string(), Value::Shape(ShapeHandle(self.0)))].into())
    }
}

fn body(feature: FeatureId) -> InputRef {
    InputRef::Output {
        feature,
        slot: "body".into(),
    }
}

/// ボディ `target` を変える機能(丸めなど)。読んだ形の番号の後ろに `k` を足した番号の形を出し、
/// どの形を読んだかが番号で分かる。`tool` があれば、それを足し込む(結合)。
struct Step {
    target: FeatureId,
    tool: Option<FeatureId>,
    k: u64,
}

impl Feature for Step {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        Ok([Some(self.target), self.tool]
            .into_iter()
            .flatten()
            .map(body)
            .collect())
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let Value::Shape(h) = ctx.output(self.target, "body")? else {
            return Err(EvalError::Other("形ではありません".into()));
        };
        Ok([(
            "body".to_string(),
            Value::Shape(ShapeHandle(h.0 * 10 + self.k)),
        )]
        .into())
    }

    fn pass_through(&self) -> Vec<(String, InputRef)> {
        vec![("body".into(), body(self.target))]
    }
}

/// ボディを読むだけで変えない機能(作業平面・加工のセットアップなど)。読んだ形の番号を出す。
struct Look(FeatureId);

impl Feature for Look {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        Ok(vec![body(self.0)])
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let Value::Shape(h) = ctx.output(self.0, "body")? else {
            return Err(EvalError::Other("形ではありません".into()));
        };
        Ok([(
            "value".to_string(),
            Value::Scalar(Quantity::scalar(h.0 as f64)),
        )]
        .into())
    }
}

fn id(d: &serde_json::Value, key: &str) -> FeatureId {
    FeatureId(d[key].as_u64().unwrap())
}

fn registry() -> Registry {
    let mut r = Registry::new();
    r.register("block@1", |d| Ok(Box::new(Block(d["n"].as_u64().unwrap()))));
    r.register("step@1", |d| {
        Ok(Box::new(Step {
            target: id(d, "target"),
            tool: d.get("tool").map(|_| id(d, "tool")),
            k: d["k"].as_u64().unwrap(),
        }))
    });
    r.register("look@1", |d| Ok(Box::new(Look(id(d, "of")))));
    r.mark("look@1", CONCURRENT);
    r
}

fn insert(
    doc: &mut Document,
    index: Option<usize>,
    type_id: &str,
    data: serde_json::Value,
) -> FeatureId {
    let id = doc.alloc_feature_id();
    let entry = FeatureEntry {
        id,
        type_id: type_id.into(),
        name: format!("{type_id} {}", id.0),
        data,
        suppressed: false,
        pin_bodies: false,
    };
    doc.apply(Transaction::new(
        "機能を足す",
        vec![Op::InsertFeature { index, entry }],
    ))
    .unwrap();
    id
}

fn shape(r: &RecomputeReport, id: FeatureId) -> u64 {
    match r.feature(id) {
        Some(FeatureStatus::Ok { outputs, .. }) => match outputs.get("body") {
            Some(Value::Shape(h)) => h.0,
            _ => panic!("{id} にボディがありません"),
        },
        other => panic!("{id} が Ok ではありません: {other:?}"),
    }
}

fn number(r: &RecomputeReport, id: FeatureId) -> u64 {
    match r.feature(id) {
        Some(FeatureStatus::Ok { outputs, .. }) => match outputs.get("value") {
            Some(Value::Scalar(q)) => q.value as u64,
            _ => panic!("{id} に数がありません"),
        },
        other => panic!("{id} が Ok ではありません: {other:?}"),
    }
}

fn visible(doc: &Document, reg: &Registry, r: &RecomputeReport) -> Vec<FeatureId> {
    visible_bodies(doc.data(), reg, r)
        .into_iter()
        .map(|(f, _)| f)
        .collect()
}

fn warnings(r: &RecomputeReport, id: FeatureId) -> Vec<String> {
    match r.feature(id) {
        Some(FeatureStatus::Ok { outputs, .. }) => warnings_of(outputs),
        _ => vec![],
    }
}

/// 箱 → 穴(箱を読む) → セットアップ(箱を読む)の文書に、箱の直後へ丸めを差し込む。
fn inserted(pin_hole: bool) -> (Document, FeatureId, FeatureId, FeatureId, FeatureId) {
    let mut doc = Document::new();
    let block = insert(&mut doc, None, "block@1", json!({"n": 1}));
    let hole = insert(&mut doc, None, "step@1", json!({"target": block.0, "k": 2}));
    let setup = insert(&mut doc, None, "look@1", json!({"of": hole.0}));
    if pin_hole {
        doc.apply(Transaction::new(
            "前の版の読み方",
            vec![Op::SetPinBodies {
                id: hole,
                pin: true,
            }],
        ))
        .unwrap();
    }
    let fillet = insert(
        &mut doc,
        Some(1),
        "step@1",
        json!({"target": block.0, "k": 3}),
    );
    (doc, block, fillet, hole, setup)
}

#[test]
fn a_feature_inserted_before_a_reader_is_read_by_it_and_the_body_does_not_split() {
    let reg = registry();
    let (mut doc, _, fillet, hole, setup) = inserted(false);
    let r = doc.recompute(&reg, &kernel::NullKernel);
    assert_eq!(shape(&r, fillet), 13);
    // 穴は差し込んだ丸めの後の形を読み、セットアップは穴の後の形を読む
    assert_eq!(shape(&r, hole), 132);
    assert_eq!(number(&r, setup), 132);
    assert_eq!(visible(&doc, &reg, &r), vec![hole]);
    assert!(warnings(&r, hole).is_empty());
    let table = &r.bodies;
    let b = table.body_of(hole).unwrap();
    assert_eq!(b, BodyId::new(FeatureId(1)));
    assert_eq!(table.body_of(fillet), Some(b));
    assert_eq!(table.tip_before(b, hole), Some((fillet, "body".into())));
    assert_eq!(table.tip(b), Some((hole, "body".into())));
    assert_eq!(body_heir(doc.data(), &reg, &r, fillet), hole);
    assert_eq!(body_root(doc.data(), &reg, hole), FeatureId(1));

    // 取り消すと丸めが無くなり、穴は箱を読む
    assert!(doc.undo());
    let r = doc.recompute(&reg, &kernel::NullKernel);
    assert_eq!(shape(&r, hole), 12);
    assert_eq!(number(&r, setup), 12);
}

#[test]
fn a_pinned_reader_keeps_the_old_reading_with_a_warning_until_it_reads_the_latest() {
    let reg = registry();
    // 前の版の読み方: 穴は箱をそのまま読み、ボディが 2 つに見える(今までの動き)
    let (mut doc, _, fillet, hole, setup) = inserted(true);
    let r = doc.recompute(&reg, &kernel::NullKernel);
    assert_eq!(shape(&r, hole), 12);
    assert_eq!(number(&r, setup), 12);
    assert_eq!(visible(&doc, &reg, &r), vec![fillet, hole]);
    let w = warnings(&r, hole);
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].contains("step@1 4"), "{w:?}");
    assert!(warnings(&r, fillet).is_empty());
    // 差し込み済みなので、印は外れない
    assert_eq!(doc.release_pins(&reg), 0);

    // 「最新の形を読む」で付いて来る(1 回の取り消し単位)
    doc.apply(Transaction::new(
        "最新の形を読む",
        vec![Op::SetPinBodies {
            id: hole,
            pin: false,
        }],
    ))
    .unwrap();
    let r = doc.recompute(&reg, &kernel::NullKernel);
    assert_eq!(shape(&r, hole), 132);
    assert_eq!(visible(&doc, &reg, &r), vec![hole]);
    assert!(warnings(&r, hole).is_empty());
    assert!(doc.undo());
    let r = doc.recompute(&reg, &kernel::NullKernel);
    assert_eq!(shape(&r, hole), 12);
}

#[test]
fn a_reader_of_a_merged_tool_reads_the_body_it_went_into() {
    let reg = registry();
    let mut doc = Document::new();
    let a = insert(&mut doc, None, "block@1", json!({"n": 1}));
    let b = insert(&mut doc, None, "block@1", json!({"n": 5}));
    let join = insert(
        &mut doc,
        None,
        "step@1",
        json!({"target": a.0, "tool": b.0, "k": 7}),
    );
    let look = insert(&mut doc, None, "look@1", json!({"of": b.0}));
    let r = doc.recompute(&reg, &kernel::NullKernel);
    // 結合は道具を足される前の形で読み、後ろの機能は足された先のボディを読む
    assert_eq!(shape(&r, join), 17);
    assert_eq!(number(&r, look), 17);
    assert_eq!(visible(&doc, &reg, &r), vec![join]);
    let line = r.bodies.line(BodyId::new(b)).unwrap();
    assert_eq!(line.merged, Some((join, BodyId::new(a))));
}

#[test]
fn a_feature_that_only_reads_a_body_does_not_become_its_latest_shape() {
    let reg = registry();
    let mut doc = Document::new();
    let a = insert(&mut doc, None, "block@1", json!({"n": 1}));
    let datum = insert(&mut doc, None, "look@1", json!({"of": a.0}));
    let hole = insert(&mut doc, None, "step@1", json!({"target": a.0, "k": 2}));
    let r = doc.recompute(&reg, &kernel::NullKernel);
    assert_eq!(number(&r, datum), 1);
    assert_eq!(shape(&r, hole), 12);
    // 抑制した丸めはボディをそのまま渡し、後ろはその形を読む
    let fillet = insert(&mut doc, Some(1), "step@1", json!({"target": a.0, "k": 3}));
    doc.apply(Transaction::new(
        "抑制",
        vec![Op::SetSuppressed {
            id: fillet,
            suppressed: true,
        }],
    ))
    .unwrap();
    let r = doc.recompute(&reg, &kernel::NullKernel);
    assert_eq!(shape(&r, hole), 12);
    assert_eq!(visible(&doc, &reg, &r), vec![hole]);
}

#[test]
fn a_version_5_file_opens_pinned_and_only_needed_pins_stay() {
    let reg = registry();
    // 版 5 の文書: 箱 1 → 丸め 2(箱を読む、後から差し込んだ) → 穴 3(箱を読む)、箱 4 → 穴 5
    let v5 = json!({
        "format_version": 5,
        "next_id": 6,
        "params": [],
        "features": [
            {"id": 1, "type_id": "block@1", "name": "箱", "data": {"n": 1}},
            {"id": 2, "type_id": "step@1", "name": "丸め", "data": {"target": 1, "k": 3}},
            {"id": 3, "type_id": "step@1", "name": "穴", "data": {"target": 1, "k": 2}},
            {"id": 4, "type_id": "block@1", "name": "箱2", "data": {"n": 4}},
            {"id": 5, "type_id": "step@1", "name": "穴2", "data": {"target": 4, "k": 2}}
        ]
    });
    let mut doc = Document::from_json(&v5.to_string()).unwrap();
    assert!(doc.data().features.iter().all(|f| f.pin_bodies));
    // 開いた形は前のアプリと同じ
    let before = doc.recompute(&reg, &kernel::NullKernel);
    let shapes = |r: &RecomputeReport| [2, 3, 5].map(|n| shape(r, FeatureId(n)));
    assert_eq!(shapes(&before), [13, 12, 42]);
    assert_eq!(doc.release_pins(&reg), 4);
    let pinned: Vec<u64> = doc
        .data()
        .features
        .iter()
        .filter(|f| f.pin_bodies)
        .map(|f| f.id.0)
        .collect();
    assert_eq!(pinned, vec![3]);
    let after = doc.recompute(&reg, &kernel::NullKernel);
    assert_eq!(shapes(&after), [13, 12, 42]);
    assert_eq!(
        visible(&doc, &reg, &after),
        vec![FeatureId(2), FeatureId(3), FeatureId(5)]
    );
    assert_eq!(warnings(&after, FeatureId(3)).len(), 1);
    assert!(warnings(&after, FeatureId(5)).is_empty());
    // 外した印は変更の記録にしない
    assert!(!doc.can_undo());

    // 保存すると今の版(印を足したのは版 6)で、残った印だけが書かれ、開き直しても同じ形
    let saved = doc.to_json();
    let raw: serde_json::Value = serde_json::from_str(&saved).unwrap();
    assert_eq!(raw["format_version"], json!(FORMAT_VERSION));
    const { assert!(FORMAT_VERSION >= 6) };
    let written: Vec<bool> = raw["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.get("pin_bodies").is_some())
        .collect();
    assert_eq!(written, vec![false, false, true, false, false]);
    let mut back = Document::from_json(&saved).unwrap();
    assert_eq!(back.release_pins(&reg), 0);
    assert_eq!(
        shapes(&back.recompute(&reg, &kernel::NullKernel)),
        [13, 12, 42]
    );
}

#[test]
fn a_body_id_is_a_number_for_the_body_and_text_for_a_part() {
    let main = BodyId::new(FeatureId(3));
    let part = BodyId {
        root: FeatureId(3),
        part: 2,
    };
    assert_eq!(serde_json::to_value(main).unwrap(), json!(3));
    assert_eq!(serde_json::to_value(part).unwrap(), json!("3.2"));
    assert_eq!(serde_json::from_value::<BodyId>(json!(3)).unwrap(), main);
    assert_eq!(
        serde_json::from_value::<BodyId>(json!("3.2")).unwrap(),
        part
    );
    assert!(serde_json::from_value::<BodyId>(json!("x")).is_err());
    assert_eq!(BodyId::part_of_slot("body"), Some(0));
    assert_eq!(BodyId::part_of_slot("part:4"), Some(4));
    assert_eq!(BodyId::part_of_slot("copy.1"), None);
}
