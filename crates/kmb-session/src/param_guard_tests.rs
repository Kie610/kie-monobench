//! 確かめること: 名前付きの欄と表の検査の両立、候補と確定の番号の一致、拒否時の番号・履歴・値の保持。

use super::*;
use kmb_core::{
    Descriptor, EvalCtx, EvalError, Feature, FeatureId, Field, InputRef, Module, Modules, Outputs,
    ParamUnit,
};
use serde_json::{Value, json};
use std::sync::Arc;

struct Numbers(Value);
impl Feature for Numbers {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut refs = vec![];
        for key in ["x", "y", "angle"] {
            refs.extend(kmb_core::expr_inputs(self.0[key].as_str().unwrap())?);
        }
        Ok(refs)
    }
    fn evaluate(&self, _: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        Ok(Default::default())
    }
}

fn numbers() -> (Headless, FeatureId) {
    let module = Module::new("test", |registry| {
        registry.register_described(
            Descriptor::new(
                "test.guard_numbers@1",
                kmb_core::category::SOLID,
                "数の見本",
            )
            .field(Field::expr("x", "横").length())
            .field(Field::expr("y", "縦").length())
            .field(Field::expr("angle", "角").angle()),
            |data| Ok(Box::new(Numbers(data.clone()))),
        );
    });
    let mut h = Headless::new(Modules::assemble(
        vec![crate::module(), module],
        Arc::new(kmb_core::NullKernel),
    ));
    let feature = h
        .add_feature(
            "test.guard_numbers@1",
            Some("見本"),
            json!({"x":"1 mm","y":"2 mm","angle":"0 deg"}),
        )
        .unwrap();
    (h, feature)
}

fn values(h: &Headless) -> std::collections::BTreeMap<String, kmb_core::Quantity> {
    h.report()
        .param_values()
        .into_iter()
        .map(|(name, value)| (name.into(), value))
        .collect()
}

#[test]
fn a_named_table_value_is_checked_after_expansion_and_undoes_as_one_change() {
    let (mut h, feature) = numbers();
    let before = h.doc().undo_depth();
    h.set_value(feature, "x", "width=20").unwrap();
    assert_eq!(h.doc().data().param_by_name("width").unwrap().expr, "20 mm");
    assert_eq!(h.doc().data().feature(feature).unwrap().data["x"], "width");
    assert_eq!(values(&h)["width"].value, 20.0);
    assert_eq!(h.doc().undo_depth(), before + 1);
    h.undo().unwrap();
    assert!(h.doc().data().param_by_name("width").is_none());
    assert_eq!(h.doc().data().feature(feature).unwrap().data["x"], "1 mm");
}

#[test]
fn multiple_named_fields_use_the_same_ids_and_references_in_the_preview_and_commit() {
    let (mut h, feature) = numbers();
    let before = h.doc().data().clone();
    let history = h.doc().undo_depth();
    let data = json!({"x":"wide=20","y":"tall=30","angle":"turn=45"});
    let tx = crate::edit::set_feature_data(h.doc().data(), feature, data);
    let mut draft = snapshot(h.doc().data()).unwrap();
    let prepared = crate::params::complete_named_parameter_change(
        &mut draft,
        &h.modules().registry,
        h.report(),
        tx.clone(),
    )
    .unwrap();
    draft.apply(prepared).unwrap();
    h.check_table_transaction(&tx, true).unwrap();
    assert_eq!(h.doc().data(), &before, "候補の検査では番号も変えない");
    assert_eq!(h.doc().undo_depth(), history);
    h.apply(tx).unwrap();
    assert_eq!(
        h.doc().data(),
        draft.data(),
        "候補と確定で番号と参照が一致する"
    );
    assert_eq!(h.doc().data().next_id, before.next_id + 3);
    for (offset, name) in ["wide", "tall", "turn"].into_iter().enumerate() {
        assert_eq!(
            h.doc().data().param_by_name(name).unwrap().id.0,
            before.next_id + offset as u64
        );
    }
    assert_eq!(h.doc().undo_depth(), history + 1);
    h.undo().unwrap();
    assert_eq!(h.doc().data().params, before.params);
    assert_eq!(
        h.doc().data().feature(feature).unwrap().data,
        before.feature(feature).unwrap().data
    );
    h.redo().unwrap();
    assert_eq!(h.doc().data(), draft.data());
}

#[test]
fn a_rejected_named_change_keeps_ids_values_history_and_redo_for_all_failure_stages() {
    use kmb_interchange::configuration::{ConfigCell, ConfigRow};
    let (mut h, feature) = numbers();
    h.add_param_info(
        "base",
        "10 mm",
        &crate::edit::ParamInfo {
            unit: Some(ParamUnit::Length),
            comment: None,
        },
    )
    .unwrap();
    h.set_param("base", "11 mm").unwrap();
    h.undo().unwrap();
    let id = h.param_id("base").unwrap();
    let table = Configurations {
        columns: vec![ConfigColumn::Param { param: id }],
        rows: vec![ConfigRow {
            name: "誤った構成".into(),
            cells: vec![ConfigCell::Expr("unknown + 1 mm".into())],
        }],
        ..Default::default()
    };
    let failures = [
        Op::SetParamExpr {
            id,
            expr: "unknown + 1 mm".into(),
        },
        Op::SetParamExpr {
            id,
            expr: "base".into(),
        },
        Op::SetParamExpr {
            id,
            expr: "30 deg".into(),
        },
        h.doc().data().section_op(&table).unwrap(),
        Op::SetFeatureData {
            id: FeatureId(u64::MAX),
            data: json!({}),
        },
    ];
    let before = h.doc().data().clone();
    let value_before = values(&h);
    let undo: Vec<_> = h
        .doc()
        .undo_labels()
        .into_iter()
        .map(str::to_owned)
        .collect();
    let redo: Vec<_> = h
        .doc()
        .redo_labels()
        .into_iter()
        .map(str::to_owned)
        .collect();
    let recomputes = h.recomputes();
    assert!(!redo.is_empty());
    for failure in failures {
        let mut tx = crate::edit::set_feature_data(
            h.doc().data(),
            feature,
            json!({"x":"candidate=20","y":"2 mm","angle":"0 deg"}),
        );
        tx.ops.push(failure);
        assert!(h.check_param_transaction(&tx).is_err());
        assert!(h.apply(tx).is_err());
        assert_eq!(h.doc().data(), &before, "番号を含む文書を保つ");
        assert_eq!(values(&h), value_before);
        assert_eq!(h.doc().undo_labels(), undo);
        assert_eq!(h.doc().redo_labels(), redo);
        assert_eq!(h.recomputes(), recomputes);
    }
}
