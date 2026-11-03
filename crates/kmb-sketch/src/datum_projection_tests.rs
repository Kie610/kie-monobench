//! 確かめること: 型で受けた作業軸の投影、元を動かした時の追従、保存と取り消し。
use super::*;
use kmb_core::{
    Context, Document, Edit, Extensions, FeatureId, FeatureStatus, Kernel, NullKernel, Op, Outcome,
    Transaction,
};
use kmb_interchange::DatumAxis;
use serde_json::json;
use std::sync::Arc;

const PROJECT: &str = "cad.sketch.project_axis";

struct Source(DatumAxis);
impl Feature for Source {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        Ok(vec![])
    }
    fn evaluate(&self, _: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        Ok([(kmb_interchange::DATUM_OUTPUT.into(), self.0.to_value())].into())
    }
}

fn registry() -> Registry {
    let mut r = crate::tests::registry();
    r.register("test.axis@1", |data| {
        Ok(Box::new(Source(
            serde_json::from_value(data.clone())
                .map_err(|e| EvalError::InvalidData(e.to_string()))?,
        )))
    });
    r
}

fn fixture(dir: [f64; 3]) -> (Document, FeatureId, FeatureId) {
    let mut doc = Document::new();
    let ids = doc
        .apply_edits(
            "見本",
            vec![
                Edit::Insert {
                    type_id: "test.axis@1".into(),
                    name: "基準の軸".into(),
                    data: serde_json::to_value(DatumAxis {
                        at: [2.0, 3.0, 4.0],
                        dir,
                        span: [-5.0, 5.0],
                    })
                    .unwrap(),
                },
                Edit::Insert {
                    type_id: SKETCH.into(),
                    name: "投影先".into(),
                    data: serde_json::to_value(Sketch::default()).unwrap(),
                },
            ],
        )
        .unwrap();
    (doc, ids[0], ids[1])
}

fn project(doc: &mut Document, axis: FeatureId, sketch: FeatureId) {
    let registry = registry();
    let kernel: Arc<dyn Kernel> = Arc::new(NullKernel);
    let report = doc.recompute(&registry, kernel.as_ref());
    let extensions = Extensions(module().extensions);
    let args = json!({"feature":sketch.0,"axis":axis.0});
    let cx = Context {
        args: &args,
        ..Context::new(doc.data(), &report, &registry, &kernel)
    };
    let result = extensions
        .run(PROJECT, &cx)
        .expect("画面・CLI・MCPが同じ宣言から作業軸を投影できる");
    let Outcome::Change { label, edits } = result else {
        panic!("投影は取り消せる変更")
    };
    doc.apply_edits(&label, edits).unwrap();
}

fn points(doc: &mut Document, id: FeatureId) -> Vec<[f64; 2]> {
    let report = doc.recompute(&registry(), &NullKernel);
    let FeatureStatus::Ok { outputs, .. } = report.feature(id).unwrap() else {
        panic!("投影を計算する: {:?}", report.feature(id))
    };
    let sol = solution_of(&outputs["sketch"]).unwrap();
    let saved: Sketch =
        serde_json::from_value(doc.data().feature(id).unwrap().data.clone()).unwrap();
    assert_eq!(saved.projections.len(), 1);
    assert_eq!(
        serde_json::to_value(&saved.projections[0].source).unwrap()["from"],
        "datum_axis"
    );
    let mut points = match saved.entity(saved.projections[0].entity).unwrap() {
        Entity::Point { id, .. } => vec![sol.points[id]],
        Entity::Line { a, b, .. } => vec![sol.points[a], sol.points[b]],
        other => panic!("線か点: {other:?}"),
    };
    points.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    points
}

#[test]
fn an_axis_projects_to_a_line_or_point_and_follows_its_source() {
    for (dir, expected) in [
        ([1.0, 0.0, 0.0], vec![[-3.0, 3.0], [7.0, 3.0]]),
        ([0.0, 0.0, 1.0], vec![[2.0, 3.0]]),
    ] {
        let (mut doc, axis, sketch) = fixture(dir);
        let before = doc.data().feature(sketch).unwrap().data.clone();
        project(&mut doc, axis, sketch);
        assert_eq!(points(&mut doc, sketch), expected);
        let reopened = Document::from_json(&doc.to_json()).unwrap();
        assert_eq!(reopened.data(), doc.data());
        let mut data = doc.data().feature(axis).unwrap().data.clone();
        data["at"][0] = json!(12.0);
        doc.apply(Transaction::new(
            "軸を動かす",
            vec![Op::SetFeatureData { id: axis, data }],
        ))
        .unwrap();
        assert_eq!(
            points(&mut doc, sketch),
            expected
                .iter()
                .map(|p| [p[0] + 10.0, p[1]])
                .collect::<Vec<_>>()
        );
        doc.undo();
        assert_eq!(points(&mut doc, sketch), expected);
        doc.undo();
        assert_eq!(doc.data().feature(sketch).unwrap().data, before);
    }
}

#[test]
fn a_missing_or_changed_axis_stops_without_retargeting() {
    for remove in [false, true] {
        let (mut doc, axis, sketch) = fixture([1.0, 0.0, 0.0]);
        project(&mut doc, axis, sketch);
        let op = if remove {
            Op::RemoveFeature { id: axis }
        } else {
            let mut data = doc.data().feature(axis).unwrap().data.clone();
            data["dir"] = json!([0.0, 0.0, 1.0]);
            Op::SetFeatureData { id: axis, data }
        };
        doc.apply(Transaction::new("元を変更", vec![op])).unwrap();
        let report = doc.recompute(&registry(), &NullKernel);
        assert!(!matches!(
            report.feature(sketch),
            Some(FeatureStatus::Ok { .. })
        ));
        doc.undo();
        assert_eq!(points(&mut doc, sketch), vec![[-3.0, 3.0], [7.0, 3.0]]);
    }
}

#[test]
fn unknown_projection_data_survives_document_round_trip() {
    let (mut doc, _, sketch) = fixture([1.0, 0.0, 0.0]);
    let mut data = doc.data().feature(sketch).unwrap().data.clone();
    data["projections"] = json!([{"entity":4,"from":"future_projection","extra":{"key":17}}]);
    doc.apply(Transaction::new(
        "未知の投影",
        vec![Op::SetFeatureData {
            id: sketch,
            data: data.clone(),
        }],
    ))
    .unwrap();
    let mut reopened = Document::from_json(&doc.to_json()).unwrap();
    assert_eq!(reopened.data().feature(sketch).unwrap().data, data);
    let report = reopened.recompute(&registry(), &NullKernel);
    assert!(!matches!(
        report.feature(sketch),
        Some(FeatureStatus::Ok { .. })
    ));
    assert_eq!(
        Document::from_json(&reopened.to_json())
            .unwrap()
            .data()
            .feature(sketch)
            .unwrap()
            .data,
        data
    );
}

#[test]
fn invalid_axes_and_undeclared_arguments_are_rejected() {
    for axis in [
        DatumAxis {
            at: [0.0; 3],
            dir: [0.0; 3],
            span: [-5.0, 5.0],
        },
        DatumAxis {
            at: [0.0; 3],
            dir: [1.0, 0.0, 0.0],
            span: [5.0, -5.0],
        },
    ] {
        assert!(datum_axis_ref(axis, FeatureId(1), &Plane::XY).is_err());
    }
    let (mut doc, axis, sketch) = fixture([1.0, 0.0, 0.0]);
    let registry = registry();
    let kernel: Arc<dyn Kernel> = Arc::new(NullKernel);
    let report = doc.recompute(&registry, kernel.as_ref());
    let args = json!({"feature":sketch.0,"axis":axis.0,"unknown":true});
    let cx = Context {
        args: &args,
        ..Context::new(doc.data(), &report, &registry, &kernel)
    };
    let err = Extensions(module().extensions)
        .run(PROJECT, &cx)
        .unwrap_err();
    assert_eq!(err.kind, kmb_core::ProblemKind::Input);
    assert!(err.message.contains("unknown"));
}

#[test]
fn an_unlinked_axis_projection_is_fixed_and_no_longer_reads_the_axis() {
    let (mut doc, axis, sketch) = fixture([1.0, 0.0, 0.0]);
    let registry = registry();
    let kernel: Arc<dyn Kernel> = Arc::new(NullKernel);
    let report = doc.recompute(&registry, kernel.as_ref());
    let args = json!({"feature":sketch.0,"axis":axis.0,"link":false});
    let cx = Context {
        args: &args,
        ..Context::new(doc.data(), &report, &registry, &kernel)
    };
    let Outcome::Change { label, edits } =
        Extensions(module().extensions).run(PROJECT, &cx).unwrap()
    else {
        panic!("取り消せる変更");
    };
    doc.apply_edits(&label, edits).unwrap();
    doc.apply(Transaction::new(
        "元を削除",
        vec![Op::RemoveFeature { id: axis }],
    ))
    .unwrap();
    let report = doc.recompute(&registry, kernel.as_ref());
    assert!(matches!(
        report.feature(sketch),
        Some(FeatureStatus::Ok { .. })
    ));
    let saved: Sketch =
        serde_json::from_value(doc.data().feature(sketch).unwrap().data.clone()).unwrap();
    assert!(saved.projections.is_empty());
    assert_eq!(
        saved
            .constraints
            .iter()
            .filter(|c| matches!(c.kind, ConstraintKind::Fix { .. }))
            .count(),
        2
    );
}

#[test]
fn a_normal_axis_does_not_move_an_overlapping_origin_reference() {
    let mut sketch = Sketch::default();
    let origin = origin_ref(&Plane::XY);
    let origin_point = sketch.add_projection(&origin);
    let axis = DatumAxis {
        at: [0.0, 0.0, 4.0],
        dir: [0.0, 0.0, 1.0],
        span: [-5.0, 5.0],
    };
    let axis_point =
        sketch.add_projection(&datum_axis_ref(axis, FeatureId(1), &Plane::XY).unwrap());
    assert_ne!(origin_point, axis_point, "独立して動く参照は点を共有しない");
    let moved = DatumAxis {
        at: [5.0, 0.0, 4.0],
        ..axis
    };
    assert!(
        refresh_projections(
            &mut sketch,
            &[
                origin,
                datum_axis_ref(moved, FeatureId(1), &Plane::XY).unwrap()
            ]
        )
        .is_empty()
    );
    let sol = solve(&sketch, &Default::default()).unwrap();
    assert_eq!(sol.points[&origin_point], [0.0, 0.0]);
    assert_eq!(sol.points[&axis_point], [5.0, 0.0]);
}
