//! 文字のパラメータ([`ParamUnit::Text`])と、機能が読む差し込みの文字([`EvalCtx::text`])の試験。

use crate::*;
use serde_json::json;
use std::cell::Cell;

thread_local! {
    static EVALS: Cell<usize> = const { Cell::new(0) };
}

/// `{"text": "..."}` の差し込みの文字に値を入れ、出力 `text` に出す
struct LabelFeature(String);
impl Feature for LabelFeature {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        template_inputs(&self.0)
    }
    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        EVALS.with(|c| c.set(c.get() + 1));
        let text = ctx.text(&self.0)?;
        Ok([(
            "text".to_string(),
            Value::Custom {
                type_key: "test.text".into(),
                data: json!(text),
            },
        )]
        .into())
    }
    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        let text = rename_in_template(&self.0, old, new)?;
        Ok((text != self.0).then(|| json!({ "text": text })))
    }
}

/// `{"expr": "..."}` を数の欄として読む
struct NumberFeature(String);
impl Feature for NumberFeature {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        expr_inputs(&self.0)
    }
    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        Ok([("value".to_string(), Value::Scalar(ctx.expr(&self.0)?))].into())
    }
}

fn registry() -> Registry {
    let mut r = Registry::new();
    r.register("test.label@1", |d| {
        Ok(Box::new(LabelFeature(
            d["text"].as_str().unwrap_or_default().into(),
        )))
    });
    r.register("test.number@1", |d| {
        Ok(Box::new(NumberFeature(
            d["expr"].as_str().unwrap_or_default().into(),
        )))
    });
    r
}

fn add_param(doc: &mut Document, name: &str, expr: &str, unit: ParamUnit) -> ParamId {
    let id = doc.alloc_param_id();
    let param = Param {
        unit,
        ..Param::new(id, name, expr)
    };
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

fn label(r: &RecomputeReport, id: FeatureId) -> String {
    match r.feature(id) {
        Some(FeatureStatus::Ok { outputs, .. }) => match &outputs["text"] {
            Value::Custom { data, .. } => data.as_str().unwrap().to_string(),
            _ => panic!("文字ではありません"),
        },
        other => panic!("{id} が Ok ではありません: {other:?}"),
    }
}

fn text_of(r: &RecomputeReport, name: &str) -> String {
    match r.param(name) {
        Some(ParamStatus::Text { value }) => value.clone(),
        other => panic!("{name}: {other:?}"),
    }
}

fn error_of(r: &RecomputeReport, name: &str) -> String {
    match r.param(name) {
        Some(ParamStatus::Error { message }) => message.clone(),
        other => panic!("{name} が誤りではありません: {other:?}"),
    }
}

fn set_units(doc: &mut Document, units: LengthUnit) {
    let properties = Properties {
        units,
        ..doc.data().properties.clone()
    };
    doc.apply(Transaction::new(
        "単位",
        vec![Op::SetProperties {
            properties: Box::new(properties),
        }],
    ))
    .unwrap();
}

#[test]
fn text_params_join_texts_and_numbers_in_the_document_unit() {
    let reg = registry();
    let mut doc = Document::new();
    add_param(&mut doc, "長さ", "25", ParamUnit::Length);
    add_param(&mut doc, "系列", "M3", ParamUnit::Text);
    add_param(&mut doc, "型番", "{系列}-{長さ}", ParamUnit::Text);
    add_param(&mut doc, "刻印", "L={長さ; 1}・{{型番}}", ParamUnit::Text);
    let r = doc.recompute(&reg, &NullKernel);
    assert_eq!(r.error_count(), 0, "{r:?}");
    assert_eq!(text_of(&r, "型番"), "M3-25 mm");
    assert_eq!(text_of(&r, "刻印"), "L=25.0 mm・{型番}");
    assert_eq!(r.text_values()["型番"], "M3-25 mm");
    assert!(!r.param_values().contains_key("型番"));
    // 文書の単位を替えると、数を入れた文字も替わる
    set_units(&mut doc, LengthUnit::In);
    let r = doc.recompute(&reg, &NullKernel);
    assert_eq!(text_of(&r, "型番"), "M3-0.984 in");
    // 結果の JSON
    let row = r.params.iter().find(|p| p.name == "系列").unwrap();
    assert_eq!(
        serde_json::to_value(row).unwrap(),
        json!({"id": row.id, "name": "系列", "status": "text", "value": "M3"})
    );
}

#[test]
fn text_params_are_not_numbers_and_errors_stay_local() {
    let reg = registry();
    let mut doc = Document::new();
    add_param(&mut doc, "系列", "M3", ParamUnit::Text);
    add_param(&mut doc, "幅", "系列 + 1 mm", ParamUnit::Any);
    add_param(&mut doc, "a", "{b}", ParamUnit::Text);
    add_param(&mut doc, "b", "{a}", ParamUnit::Text);
    add_param(&mut doc, "桁", "{系列; 1}", ParamUnit::Text);
    add_param(&mut doc, "無い", "{c}", ParamUnit::Text);
    let n = add_feature(&mut doc, "test.number@1", json!({"expr": "系列 * 2"}));
    let ok = add_feature(&mut doc, "test.label@1", json!({"text": "{系列}"}));
    let r = doc.recompute(&reg, &NullKernel);
    assert_eq!(
        error_of(&r, "幅"),
        "文字のパラメータ 系列 は計算に使えません"
    );
    assert!(error_of(&r, "a").contains("循環"));
    assert!(error_of(&r, "b").contains("循環"));
    assert!(error_of(&r, "桁").contains("桁は書けません"));
    assert!(error_of(&r, "無い").contains("未定義の名前: c"));
    // 数の欄に文字のパラメータを書いた機能だけが誤り
    assert!(
        matches!(r.feature(n), Some(FeatureStatus::Error { message }) if message.contains("計算に使えません"))
    );
    assert_eq!(label(&r, ok), "M3");
}

#[test]
fn features_read_templates_and_recompute_only_when_their_inputs_change() {
    let reg = registry();
    let mut doc = Document::new();
    let len = add_param(&mut doc, "長さ", "25 mm", ParamUnit::Length);
    let kind = add_param(&mut doc, "系列", "M3", ParamUnit::Text);
    let a = add_feature(&mut doc, "test.label@1", json!({"text": "{系列}-{長さ}"}));
    let only_text = add_feature(&mut doc, "test.label@1", json!({"text": "[{系列}]"}));
    let plain = add_feature(&mut doc, "test.label@1", json!({"text": "字だけ {{}}"}));
    let r = doc.recompute(&reg, &NullKernel);
    assert_eq!(label(&r, a), "M3-25 mm");
    assert_eq!(label(&r, only_text), "[M3]");
    assert_eq!(label(&r, plain), "字だけ {}");
    EVALS.with(|c| c.set(0));
    // 文字を替えると、それを使う機能だけを作り直す
    doc.apply(Transaction::new(
        "系列",
        vec![Op::SetParamExpr {
            id: kind,
            expr: "M4".into(),
        }],
    ))
    .unwrap();
    let r = doc.recompute(&reg, &NullKernel);
    assert_eq!(
        (label(&r, a), label(&r, only_text)),
        ("M4-25 mm".into(), "[M4]".into())
    );
    assert_eq!(EVALS.with(Cell::get), 2);
    // 文書の単位を替えると、差し込みのある機能を作り直す(差し込みの無い物はそのまま)
    EVALS.with(|c| c.set(0));
    set_units(&mut doc, LengthUnit::Cm);
    let r = doc.recompute(&reg, &NullKernel);
    assert_eq!(label(&r, a), "M4-2.5 cm");
    assert_eq!(EVALS.with(Cell::get), 2);
    // 名前を替えると差し込みも替わる(字の所は替えない)
    doc.rename_param(&reg, len, "L").unwrap();
    doc.rename_param(&reg, kind, "S").unwrap();
    let data = doc.data();
    let text = |id: FeatureId| data.feature(id).unwrap().data["text"].clone();
    assert_eq!(text(a), json!("{S}-{L}"));
    let r = doc.recompute(&reg, &NullKernel);
    assert_eq!(label(&r, a), "M4-2.5 cm");
}

#[test]
fn undeclared_reads_are_errors() {
    /// 単位を宣言せずに数を入れる(契約違反)
    struct NoUnits;
    impl Feature for NoUnits {
        fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
            Ok(vec![InputRef::Param {
                name: "長さ".into(),
            }])
        }
        fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
            ctx.text("{長さ}")?;
            Ok(Outputs::new())
        }
    }
    let mut reg = registry();
    reg.register("test.nounits@1", |_| Ok(Box::new(NoUnits)));
    let mut doc = Document::new();
    add_param(&mut doc, "長さ", "25 mm", ParamUnit::Length);
    let f = add_feature(&mut doc, "test.nounits@1", json!({}));
    let r = doc.recompute(&reg, &NullKernel);
    assert!(
        matches!(r.feature(f), Some(FeatureStatus::Error { message }) if message.contains("単位"))
    );
    assert_eq!(
        template_inputs("{a}-{a + b}").unwrap(),
        vec![
            InputRef::Param { name: "a".into() },
            InputRef::Param { name: "b".into() },
            InputRef::Units
        ]
    );
    assert!(template_inputs("字だけ").unwrap().is_empty());
}

#[test]
fn transactions_check_templates_by_unit() {
    let mut doc = Document::new();
    let bad = Param {
        unit: ParamUnit::Text,
        ..Param::new(doc.alloc_param_id(), "x", "M{")
    };
    assert!(
        doc.apply(Transaction::new(
            "x",
            vec![Op::AddParam {
                index: None,
                param: bad
            }]
        ))
        .is_err()
    );
    let id = add_param(&mut doc, "系列", "{{M}}3", ParamUnit::Text);
    // 文字のパラメータの式は差し込みの文字として確かめる
    assert!(
        doc.apply(Transaction::new(
            "x",
            vec![Op::SetParamExpr {
                id,
                expr: "M}".into()
            }]
        ))
        .is_err()
    );
    // 数の式でない文字のまま、数の単位には替えられない
    let to_len = |doc: &mut Document| {
        doc.apply(Transaction::new(
            "x",
            vec![Op::SetParamInfo {
                id,
                comment: String::new(),
                unit: ParamUnit::Length,
            }],
        ))
    };
    assert!(to_len(&mut doc).is_err());
    doc.apply(Transaction::new(
        "x",
        vec![Op::SetParamExpr {
            id,
            expr: "3 mm".into(),
        }],
    ))
    .unwrap();
    to_len(&mut doc).unwrap();
}

#[test]
fn version_6_documents_open_and_text_params_save_as_version_7() {
    let reg = registry();
    let v6 = r#"{"format_version": 6, "next_id": 3, "params": [
        {"id": 1, "name": "w", "expr": "10", "unit": "length"}],
        "features": [{"id": 2, "type_id": "test.label@1", "name": "L", "data": {"text": "{w}"}}]}"#;
    let mut doc = Document::from_json(v6).unwrap();
    assert_eq!(doc.data().format_version, 7);
    assert_eq!(FORMAT_VERSION, 7);
    let r = doc.recompute(&reg, &NullKernel);
    assert_eq!(label(&r, FeatureId(2)), "10 mm");
    add_param(&mut doc, "系列", "M3", ParamUnit::Text);
    let saved = doc.to_json();
    assert!(saved.contains("\"unit\": \"text\""), "{saved}");
    let back = Document::from_json(&saved).unwrap();
    assert_eq!(
        back.data().param_by_name("系列").unwrap().unit,
        ParamUnit::Text
    );
    assert_eq!(ParamUnit::Text.label(), "文字");
}
