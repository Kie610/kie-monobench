//! 確かめること: 表の複数の式の全件検査、1回の再計算と取り消し、同じ機能の値の蓄積。
use super::*;

fn plain() -> Headless {
    let mut h = Headless::new(kmb_core::Modules::assemble(
        vec![crate::module()],
        std::sync::Arc::new(kmb_core::NullKernel),
    ));
    h.add_param("width", "10 mm").unwrap();
    h.add_param("height", "20 mm").unwrap();
    h
}

fn user(h: &Headless, name: &str, expr: &str) -> Change {
    Change {
        row: ParamKey::User {
            id: h.param_id(name).unwrap(),
        },
        expr: expr.into(),
    }
}

#[test]
fn an_invalid_expression_or_cycle_keeps_all_values_and_history() {
    for expr in ["(", "unknown + 1 mm", "width", "1 / 0"] {
        let mut h = plain();
        let before = h.doc().to_json();
        let (runs, history) = (h.recomputes(), h.mark());
        let changes = [user(&h, "height", "50 mm"), user(&h, "width", expr)];
        assert!(!h.check_param_batch(&changes).is_empty());
        let error = h.set_param_batch(&changes).unwrap_err();
        assert_eq!(error.kind, ProblemKind::Input);
        assert_eq!(h.doc().to_json(), before);
        assert_eq!((h.recomputes(), h.mark()), (runs, history));
    }
}

#[test]
fn forward_dependencies_use_all_proposed_expressions_and_disallow_new_downstream_errors() {
    let mut h = plain();
    h.set_param_batch(&[user(&h, "height", "width * 3"), user(&h, "width", "15 mm")])
        .unwrap();
    assert!(
        matches!(&h.report().params.iter().find(|p| p.name=="height").unwrap().status, ParamStatus::Ok { value } if value.value == 45.0)
    );
    h.set_param("height", "1 mm / (width / 1 mm - 10)").unwrap();
    let before = h.doc().to_json();
    assert!(h.set_param_batch(&[user(&h, "width", "10 mm")]).is_err());
    assert_eq!(h.doc().to_json(), before);
}

#[test]
fn text_templates_and_document_length_units_use_the_common_parameter_evaluation() {
    let mut h = plain();
    h.set_properties(&[("units".into(), "in".into())]).unwrap();
    h.set_param_info(
        "width",
        None,
        &crate::edit::ParamInfo {
            unit: Some(ParamUnit::Length),
            comment: None,
        },
    )
    .unwrap();
    h.add_param_info(
        "note",
        "width={width}",
        &crate::edit::ParamInfo {
            unit: Some(ParamUnit::Text),
            comment: None,
        },
    )
    .unwrap();
    h.set_param_batch(&[user(&h, "width", "2"), user(&h, "note", "size={width}")])
        .unwrap();
    assert_eq!(h.doc().data().param_by_name("width").unwrap().expr, "2 in");
    assert!(
        matches!(&h.report().params.iter().find(|p| p.name=="note").unwrap().status, ParamStatus::Text { value } if value.contains('2'))
    );
    let before = h.doc().to_json();
    assert!(h.set_param_batch(&[user(&h, "width", "30 deg")]).is_err());
    assert_eq!(h.doc().to_json(), before);
}

#[test]
fn duplicate_and_missing_rows_and_unknown_arguments_are_rejected() {
    let mut h = plain();
    let before = h.doc().to_json();
    let change = user(&h, "width", "30 mm");
    assert!(
        h.set_param_batch(&[change.clone(), change.clone()])
            .is_err()
    );
    assert!(
        h.set_param_batch(&[Change {
            row: ParamKey::User {
                id: kmb_core::ParamId(u64::MAX)
            },
            expr: "20 mm".into()
        }])
        .is_err()
    );
    for args in [
        serde_json::json!({"changes":[change.clone()],"extra":true}),
        serde_json::json!({"changes":[{"row":{"kind":"user","id":1,"extra":true},"expr":"20 mm"}]}),
    ] {
        assert_eq!(
            h.run(APPLY, None, None, &args).unwrap_err().kind,
            ProblemKind::Input
        );
    }
    let runs = h.recomputes();
    h.set_param_batch(&[]).unwrap();
    h.set_param_batch(&[user(&h, "width", "10 mm")]).unwrap();
    assert_eq!(h.recomputes(), runs);
    assert_eq!(h.doc().to_json(), before);
}

struct Counted {
    data: serde_json::Value,
}

impl kmb_core::Feature for Counted {
    fn inputs(&self) -> Result<Vec<kmb_core::InputRef>, kmb_core::EvalError> {
        Ok(vec![])
    }
    fn evaluate(
        &self,
        _: &mut kmb_core::EvalCtx<'_>,
    ) -> Result<kmb_core::Outputs, kmb_core::EvalError> {
        for key in ["a", "b", "c"] {
            let value = kmb_core::eval_str(self.data[key].as_str().unwrap(), &|_| None)?;
            ParamUnit::Length
                .check(value.dim)
                .map_err(kmb_core::EvalError::InvalidData)?;
        }
        Ok(Default::default())
    }
    fn model_params(&self) -> Vec<kmb_core::ModelParam> {
        ["a", "b"]
            .into_iter()
            .map(|key| kmb_core::ModelParam {
                key: key.into(),
                name: format!("d{key}"),
                expr: self.data[key].as_str().unwrap().into(),
                label: key.into(),
            })
            .collect()
    }
    fn set_model_param(&self, name: &str, expr: &str) -> Option<serde_json::Value> {
        let key = name.strip_prefix('d').filter(|k| ["a", "b"].contains(k))?;
        let mut data = self.data.clone();
        data[key] = expr.into();
        Some(data)
    }
}

#[test]
fn dimensions_and_a_field_of_the_same_feature_accumulate_and_evaluate_geometry_once() {
    use std::sync::Arc;
    let module = kmb_core::Module::new("test", |registry| {
        registry.register_described(
            kmb_core::Descriptor::new("test.counted@1", kmb_core::category::SOLID, "見本")
                .field(kmb_core::Field::expr("c", "長さ").length()),
            |data| Ok(Box::new(Counted { data: data.clone() })),
        );
    });
    let modules = kmb_core::Modules::assemble(
        vec![crate::module(), module],
        Arc::new(kmb_core::NullKernel),
    );
    let mut h = Headless::new(modules);
    let feature = h
        .apply_edits(
            "見本",
            vec![Edit::Insert {
                type_id: "test.counted@1".into(),
                name: "見本".into(),
                data: serde_json::json!({"a":"1 mm","b":"2 mm","c":"3 mm"}),
            }],
        )
        .unwrap()[0];
    let before = h.doc().to_json();
    let count = h.recomputes();
    let changes = [
        Change {
            row: ParamKey::Model {
                feature,
                key: "a".into(),
            },
            expr: "5 mm".into(),
        },
        Change {
            row: ParamKey::Model {
                feature,
                key: "b".into(),
            },
            expr: "6 mm".into(),
        },
        Change {
            row: ParamKey::Field {
                feature,
                key: "c".into(),
            },
            expr: "7 mm".into(),
        },
    ];
    assert!(h.check_param_batch(&changes).is_empty());
    assert_eq!(h.recomputes(), count, "検査では形を計算しない");
    h.set_param_batch(&changes).unwrap();
    assert_eq!(h.recomputes(), count + 1);
    let data = &h.doc().data().feature(feature).unwrap().data;
    assert_eq!(data, &serde_json::json!({"a":"5 mm","b":"6 mm","c":"7 mm"}));
    h.undo().unwrap();
    assert_eq!(h.doc().to_json(), before);
    let mut wrong = changes.to_vec();
    wrong[2].expr = "30 deg".into();
    assert!(h.set_param_batch(&wrong).is_err());
    assert_eq!(h.doc().to_json(), before);
    h.set_model_param("da", "30 deg").unwrap();
    assert!(matches!(
        h.report().feature(feature),
        Some(kmb_core::FeatureStatus::Error { .. })
    ));
    h.set_param_batch(&[Change {
        row: ParamKey::Model {
            feature,
            key: "a".into(),
        },
        expr: "8 mm".into(),
    }])
    .unwrap();
    assert!(matches!(
        h.report().feature(feature),
        Some(kmb_core::FeatureStatus::Ok { .. })
    ));
}

#[test]
fn three_expressions_are_recomputed_once_and_one_undo_returns_them_all() {
    let mut h = Headless::new(kmb_core::Modules::assemble(
        vec![crate::module()],
        std::sync::Arc::new(kmb_core::NullKernel),
    ));
    for (name, expr) in [("width", "10 mm"), ("height", "20 mm"), ("depth", "30 mm")] {
        h.add_param(name, expr).unwrap();
    }
    let before = h.doc().to_json();
    let runs = h.recomputes();
    let changes: Vec<_> = [
        ("width", "40 mm"),
        ("height", "width / 2"),
        ("depth", "height + 5 mm"),
    ]
    .into_iter()
    .map(|(name, expr)| Change {
        row: ParamKey::User {
            id: h.param_id(name).unwrap(),
        },
        expr: expr.into(),
    })
    .collect();
    h.set_param_batch(&changes).unwrap();
    assert_eq!(h.recomputes(), runs + 1);
    for c in &changes {
        let ParamKey::User { id } = c.row else {
            unreachable!()
        };
        assert_eq!(
            h.doc()
                .data()
                .params
                .iter()
                .find(|p| p.id == id)
                .unwrap()
                .expr,
            c.expr
        );
    }
    h.undo().unwrap();
    assert_eq!(h.doc().to_json(), before);
}
