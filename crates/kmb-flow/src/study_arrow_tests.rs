//! 確かめること: 矢印の間隔は表示だけを変え、解く仕事の鍵を変えない。
use crate::study::{FlowData, SECTION, VOLUME, WING};
use kmb_core::*;
use kmb_study::{PLAN, Plan};
use serde_json::json;

struct Body;
impl Feature for Body {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        Ok(Vec::new())
    }
    fn evaluate(&self, _: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        Ok([("body".into(), Value::Shape(ShapeHandle(1)))].into())
    }
}
struct Shape;
impl Kernel for Shape {
    fn name(&self) -> &str {
        "test"
    }
    fn mesh(&self, _: ShapeHandle, _: f64) -> Result<Mesh, KernelError> {
        Ok(Mesh {
            positions: vec![
                [-1., -1., -1.],
                [1., -1., -1.],
                [1., 1., -1.],
                [-1., 1., -1.],
                [-1., -1., 1.],
                [1., -1., 1.],
                [1., 1., 1.],
                [-1., 1., 1.],
            ],
            triangles: vec![
                [0, 2, 1],
                [0, 3, 2],
                [4, 5, 6],
                [4, 6, 7],
                [0, 1, 5],
                [0, 5, 4],
                [1, 2, 6],
                [1, 6, 5],
                [2, 3, 7],
                [2, 7, 6],
                [3, 0, 4],
                [3, 4, 7],
            ],
            ..Mesh::default()
        })
    }
}
fn plan(kind: &str, spacing: &str) -> Result<Plan, String> {
    let mut reg = Registry::new();
    crate::register(&mut reg);
    reg.provide(std::sync::Arc::new(kmb_study::Prepared::default()));
    reg.register("test.arrow-body@1", |_| Ok(Box::new(Body)));
    let mut doc = Document::new();
    let mut study = FeatureId(0);
    for (kind, data) in [
        ("test.arrow-body@1", json!({})),
        (kind, json!({"bodies":[1],"arrows":spacing})),
    ] {
        let id = doc.alloc_feature_id();
        study = id;
        doc.apply(Transaction::new(
            "試験",
            vec![Op::InsertFeature {
                index: None,
                entry: FeatureEntry {
                    id,
                    type_id: kind.into(),
                    name: kind.into(),
                    data,
                    suppressed: false,
                    pin_bodies: false,
                },
            }],
        ))
        .unwrap();
    }
    let report = doc.recompute(&reg, &Shape);
    match report.feature(study).unwrap() {
        FeatureStatus::Ok { outputs, .. } => Ok(Plan::from_value(&outputs[PLAN]).unwrap()),
        other => Err(format!("{other:?}")),
    }
}
#[test]
fn arrow_spacing_is_a_length_and_does_not_change_the_solve_key() {
    for kind in [SECTION, VOLUME] {
        let a = plan(kind, "").unwrap();
        let b = plan(kind, "2 * 3 mm").unwrap();
        assert_eq!(a.key, b.key);
        assert_eq!(a.arrows, None);
        assert_eq!(b.arrows, Some(6.0));
        for invalid in ["0", "-1 mm", "1 deg"] {
            assert!(plan(kind, invalid).is_err(), "{kind}: {invalid}");
        }
    }
}
#[test]
fn arrow_spacing_is_optional_in_old_data_and_only_appears_for_grid_flow() {
    let data: FlowData = serde_json::from_value(json!({})).unwrap();
    assert!(data.arrows.is_empty());
    let old: Plan = serde_json::from_value(json!({"key":7})).unwrap();
    assert_eq!(old.arrows, None);
    let json = serde_json::to_value(old).unwrap();
    assert!(json.get("arrows").is_none());
    let mut reg = Registry::new();
    crate::register(&mut reg);
    for kind in [SECTION, VOLUME, WING] {
        let d = reg.descriptor(kind).unwrap();
        assert_eq!(d.fields.iter().any(|f| f.key == "arrows"), kind != WING);
    }
}
