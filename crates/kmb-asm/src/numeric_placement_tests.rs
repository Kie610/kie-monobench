//! 確かめること: 数と式による部品の配置、部品の中心、リンクとジョイントの参照。

use super::*;
use kmb_core::{
    Descriptor, Document, EvalCtx, EvalError, Feature, FeatureStatus, InputRef, Modules, Outputs,
    RecomputeReport, Value, category,
};
use serde::Deserialize;
use std::path::Path;

const BOX: &str = "test.placement_box@1";

#[derive(Deserialize)]
struct BoxShape {
    at: [f64; 3],
    size: [f64; 3],
}

impl Feature for BoxShape {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        Ok(Vec::new())
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let [w, d, h] = self.size;
        let points = [[0.0, 0.0], [w, 0.0], [w, d], [0.0, d]];
        let profile = k::Profile {
            plane: k::Plane {
                origin: self.at,
                ..k::Plane::XY
            },
            regions: vec![k::Region {
                outer: k::ProfileLoop {
                    edges: (0..4)
                        .map(|i| k::ProfileEdge {
                            name: format!("e{i}"),
                            segment: k::Segment::Line {
                                a: points[i],
                                b: points[(i + 1) % 4],
                            },
                            reversed: false,
                        })
                        .collect(),
                },
                holes: Vec::new(),
            }],
        };
        let shape = ctx
            .kernel()
            .extrude(&profile, 0.0, h, "box")
            .map_err(|e| EvalError::Kernel(e.to_string()))?;
        Ok(Outputs::from([("body".into(), Value::Shape(shape))]))
    }
}

struct Bench {
    doc: Document,
    modules: Modules,
    report: RecomputeReport,
}

impl Bench {
    fn new() -> Self {
        let mut all = kmb_bundle::all();
        all.push(kmb_core::Module::new("placement-test", |r| {
            r.register_described(Descriptor::new(BOX, category::SOLID, "試験の箱"), |v| {
                let b: BoxShape = serde_json::from_value(v.clone())
                    .map_err(|e| EvalError::InvalidData(e.to_string()))?;
                Ok(Box::new(b))
            });
        }));
        let modules = Modules::assemble(all, kmb_bundle::kernel_of(kmb_bundle::KernelKind::Smlib));
        assert_eq!(
            modules.kernel.name(),
            "smlib",
            "実形状は指定したカーネルで調べる"
        );
        let mut b = Self {
            doc: Document::new(),
            modules,
            report: Default::default(),
        };
        b.recompute();
        b
    }

    fn recompute(&mut self) {
        self.report = self
            .doc
            .recompute(&self.modules.registry, self.modules.kernel.as_ref());
        assert!(
            self.report
                .features
                .iter()
                .all(|f| matches!(f.status, FeatureStatus::Ok { .. })),
            "{:?}",
            self.report
        );
    }

    fn cx(&self) -> Context<'_> {
        Context::new(
            self.doc.data(),
            &self.report,
            &self.modules.registry,
            &self.modules.kernel,
        )
    }

    fn add_box(&mut self, at: [f64; 3], size: [f64; 3]) -> FeatureId {
        let ids = self
            .doc
            .apply_edits(
                "箱",
                vec![Edit::Insert {
                    type_id: BOX.into(),
                    name: "箱".into(),
                    data: json!({"at":at,"size":size}),
                }],
            )
            .unwrap();
        self.recompute();
        ids[0]
    }

    fn component(&mut self, bodies: &[FeatureId]) -> u64 {
        let mut list = self.doc.data().components.clone();
        list.push(Component {
            id: 0,
            name: format!("部品{}", list.len() + 1),
            parent: None,
            bodies: bodies.to_vec(),
        });
        self.doc
            .apply_edits("部品", vec![Edit::SetComponents { components: list }])
            .unwrap();
        self.doc.data().components.last().unwrap().id
    }

    fn run(
        &mut self,
        id: &str,
        args: serde_json::Value,
    ) -> Result<Vec<FeatureId>, kmb_core::Problem> {
        let outcome = self.modules.extensions.run(
            id,
            &Context {
                args: &args,
                ..self.cx()
            },
        )?;
        let Outcome::Change { label, edits } = outcome else {
            panic!("文書を変える操作")
        };
        let ids = self.doc.apply_edits(&label, edits).unwrap();
        self.recompute();
        Ok(ids)
    }

    fn run_file(
        &mut self,
        id: &str,
        args: serde_json::Value,
        selected: Option<FeatureId>,
        file: Option<&Path>,
    ) -> Result<Vec<FeatureId>, kmb_core::Problem> {
        let outcome = self.modules.extensions.run(
            id,
            &Context {
                args: &args,
                selected,
                file,
                ..self.cx()
            },
        )?;
        let Outcome::Change { label, edits } = outcome else {
            panic!("文書を変える操作")
        };
        let ids = self.doc.apply_edits(&label, edits).unwrap();
        self.recompute();
        Ok(ids)
    }

    fn insert(&mut self, kind: &str, data: serde_json::Value) -> FeatureId {
        let ids = self
            .doc
            .apply_edits(
                "機能",
                vec![Edit::Insert {
                    type_id: kind.into(),
                    name: "試験".into(),
                    data,
                }],
            )
            .unwrap();
        self.recompute();
        ids[0]
    }

    fn set(&mut self, id: FeatureId, data: serde_json::Value) {
        self.doc
            .apply_edits("値", vec![Edit::SetData { id, data }])
            .unwrap();
        self.recompute();
    }

    fn transform(&self, id: FeatureId) -> k::Matrix4 {
        crate::joint::transform_of(self.cx().output(id, "transform").unwrap()).unwrap()
    }

    fn bounds(&self, root: FeatureId) -> (k::P3, k::P3) {
        let h = self
            .cx()
            .visible_bodies()
            .into_iter()
            .find(|(id, _)| body_root(self.doc.data(), &self.modules.registry, *id) == root)
            .expect("残っているボディ")
            .1;
        self.modules.kernel.bounds(h).unwrap()
    }
}

fn near(a: k::P3, b: k::P3) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() < 1e-6)
}

#[test]
fn numeric_placement_rotates_a_native_box_about_its_center() {
    let mut b = Bench::new();
    let body = b.add_box([10.0, 20.0, 30.0], [8.0, 4.0, 2.0]);
    b.component(&[body]);
    let before = b.bounds(body);
    b.run(MOVE_PART, json!({"bodies":[body.0],"x":5,"rz":90}))
        .expect("位置と回転を同じ操作へ渡せる");
    let (lo, hi) = b.bounds(body);
    let c = center(before);
    let half = [
        (before.1[1] - before.0[1]) / 2.0,
        (before.1[0] - before.0[0]) / 2.0,
    ];
    assert!(near(center((lo, hi)), [c[0] + 5.0, c[1], c[2]]));
    assert!(
        near(lo, [c[0] + 5.0 - half[0], c[1] - half[1], before.0[2]])
            && near(hi, [c[0] + 5.0 + half[0], c[1] + half[1], before.1[2]]),
        "{lo:?} {hi:?}"
    );
}

fn center((lo, hi): (k::P3, k::P3)) -> k::P3 {
    [0, 1, 2].map(|i| (lo[i] + hi[i]) / 2.0)
}

#[test]
fn expressions_and_the_component_center_follow_parameter_and_shape_changes() {
    let mut b = Bench::new();
    let body = b.add_box([10.0, 20.0, 30.0], [8.0, 4.0, 2.0]);
    b.component(&[body]);
    let shift = b.doc.alloc_param_id();
    let turn = b.doc.alloc_param_id();
    b.doc
        .apply(kmb_core::Transaction::new(
            "寸法",
            vec![
                kmb_core::Op::AddParam {
                    index: None,
                    param: kmb_core::Param::new(shift, "shift", "5 mm"),
                },
                kmb_core::Op::AddParam {
                    index: None,
                    param: kmb_core::Param::new(turn, "turn", "90 deg"),
                },
            ],
        ))
        .unwrap();
    b.recompute();
    let pos = b
        .run(
            MOVE_PART,
            json!({"bodies":[body.0],"x":"shift","rz":"turn","z":"0.5 in"}),
        )
        .unwrap()[0];
    let data = &b.doc.data().feature(pos).unwrap().data;
    assert_eq!(
        (data["x"].as_str(), data["rz"].as_str(), data["z"].as_str()),
        (Some("shift"), Some("turn"), Some("0.5 in"))
    );
    assert!(near(center(b.bounds(body)), [19.0, 22.0, 43.7]));
    b.doc
        .apply_edits(
            "式",
            vec![
                Edit::SetParamExpr {
                    id: shift,
                    expr: "7 mm".into(),
                },
                Edit::SetParamExpr {
                    id: turn,
                    expr: "0 deg".into(),
                },
            ],
        )
        .unwrap();
    b.recompute();
    assert!(near(center(b.bounds(body)), [21.0, 22.0, 43.7]));
    b.set(body, json!({"at":[10,20,30],"size":[12,4,2]}));
    b.doc
        .apply_edits(
            "回転",
            vec![Edit::SetParamExpr {
                id: turn,
                expr: "90 deg".into(),
            }],
        )
        .unwrap();
    b.recompute();
    assert!(near(center(b.bounds(body)), [23.0, 22.0, 43.7]));
    let entry = b.doc.data().feature(pos).unwrap();
    let f = b
        .modules
        .registry
        .instantiate(&entry.type_id, &entry.data)
        .unwrap()
        .unwrap();
    assert!(f.inputs().unwrap().contains(&InputRef::Param {
        name: "turn".into()
    }));
    assert_eq!(
        f.rename_param("turn", "spin").unwrap().unwrap()["rz"],
        "spin"
    );
}

#[test]
fn rotations_apply_x_then_y_then_z_and_world_drag_keeps_them() {
    let mut b = Bench::new();
    let body = b.add_box([10.0, 20.0, 30.0], [8.0, 4.0, 2.0]);
    b.component(&[body]);
    let c = center(b.bounds(body));
    let pos = b
        .run(
            MOVE_PART,
            json!({"bodies":[body.0],"rx":90,"ry":90,"rz":90}),
        )
        .unwrap()[0];
    let m = b.transform(pos);
    // XYZ の各 90 度で、X→-Z、Y→Y、Z→X。
    assert!(near(
        crate::joint::apply_dir(&m, [1.0, 0.0, 0.0]),
        [0.0, 0.0, -1.0]
    ));
    assert!(near(
        crate::joint::apply_dir(&m, [0.0, 1.0, 0.0]),
        [0.0, 1.0, 0.0]
    ));
    assert!(near(
        crate::joint::apply_dir(&m, [0.0, 0.0, 1.0]),
        [1.0, 0.0, 0.0]
    ));
    assert!(near(k::transform_point(&m, c), c));
    let placed = b.bounds(body);
    b.doc = Document::from_json(&b.doc.to_json()).unwrap();
    b.recompute();
    assert_eq!(b.bounds(body), placed);
    assert_eq!(b.transform(pos), m);
    let count = b.doc.data().features.len();
    b.run(DRAG_POSITION, json!({"body":body.0,"offset":[3,4,5]}))
        .unwrap();
    assert_eq!(b.doc.data().features.len(), count);
    let p = &b.doc.data().feature(pos).unwrap().data;
    assert_eq!(
        (&p["rx"], &p["ry"], &p["rz"]),
        (&json!("90 deg"), &json!("90 deg"), &json!("90 deg"))
    );
    assert!(near(center(b.bounds(body)), k::add(c, [3.0, 4.0, 5.0])));
}

#[test]
fn legacy_position_upgrade_and_document_roundtrip_preserve_translation() {
    let mut b = Bench::new();
    let body = b.add_box([10.0, 20.0, 30.0], [8.0, 4.0, 2.0]);
    let part = b.component(&[body]);
    let legacy = json!({"moving":{"kind":"component","id":part},"bodies":[body.0],"x":"3 mm","y":"4 mm","z":"5 mm"});
    let pos = b.insert(POSITION, legacy.clone());
    let before = b.bounds(body);
    assert!(near(center(before), [17.0, 26.0, 36.0]));
    let descriptor = crate::position::descriptor();
    let upgraded = descriptor.upgraded(&legacy);
    assert_eq!(
        (
            upgraded["rx"].as_str(),
            upgraded["ry"].as_str(),
            upgraded["rz"].as_str()
        ),
        (Some("0 deg"), Some("0 deg"), Some("0 deg"))
    );
    assert_eq!(descriptor.type_id, POSITION);
    b.set(pos, upgraded);
    assert_eq!(b.bounds(body), before);
    b.doc = Document::from_json(&b.doc.to_json()).unwrap();
    b.recompute();
    assert_eq!(b.bounds(body), before);
    let mut origin_rotation = b.doc.data().feature(pos).unwrap().data.clone();
    origin_rotation["rz"] = json!("90 deg");
    b.set(pos, origin_rotation);
    assert!(near(center(b.bounds(body)), [-19.0, 18.0, 36.0]));
    assert!(
        !kmb_core::warnings_of(match &b.report.features.last().unwrap().status {
            FeatureStatus::Ok { outputs, .. } => outputs,
            _ => unreachable!(),
        })
        .is_empty()
    );
}

#[test]
fn a_rotated_base_uses_only_its_own_center_and_rotates_joint_drag_axes() {
    let mut b = Bench::new();
    let base = b.add_box([10.0, 20.0, 30.0], [8.0, 4.0, 2.0]);
    let lid = b.add_box([100.0, 20.0, 30.0], [4.0, 2.0, 2.0]);
    let base_part = b.component(&[base]);
    let lid_part = b.component(&[lid]);
    let j = b.insert(JOINT, json!({"moving":{"kind":"component","id":lid_part},"base":{"kind":"component","id":base_part},"bodies":[lid.0],"origin":[18,22,31],"axis":[1,0,0]}));
    let c = center(b.bounds(base));
    let old_lid = center(b.bounds(lid));
    let pos = b
        .run(MOVE_PART, json!({"bodies":[base.0],"rz":90}))
        .unwrap()[0];
    assert_eq!(
        b.doc.data().feature(pos).unwrap().data["center_bodies"],
        json!([base.0])
    );
    assert_eq!(
        b.doc.data().feature(pos).unwrap().data["bodies"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(near(center(b.bounds(base)), c));
    assert!(near(
        center(b.bounds(lid)),
        [
            c[0] - (old_lid[1] - c[1]),
            c[1] + old_lid[0] - c[0],
            old_lid[2]
        ]
    ));
    let pick = kmb_core::FacePick {
        feature: lid,
        face: "f".into(),
    };
    let drag = b.modules.extensions.drag(&b.cx(), &pick).unwrap();
    let kmb_core::DragMotion::Rotate { origin, axis } = drag.motion else {
        panic!("{drag:?}")
    };
    assert!(
        near(origin, [14.0, 26.0, 31.0]) && near(axis, [0.0, 1.0, 0.0]),
        "{drag:?}"
    );
    assert!(
        b.run(MOVE_PART, json!({"bodies":[lid.0],"x":1}))
            .unwrap_err()
            .message
            .contains("ジョイント")
    );
    let mut data = b.doc.data().feature(j).unwrap().data.clone();
    data["kind"] = json!("slider");
    b.set(j, data);
    let drag = b.modules.extensions.drag(&b.cx(), &pick).unwrap();
    let kmb_core::DragMotion::Slide { axis } = drag.motion else {
        panic!("{drag:?}")
    };
    assert!(near(axis, [0.0, 1.0, 0.0]));
}

#[test]
fn placement_arguments_share_units_and_reject_unknown_or_wrong_values() {
    let mut b = Bench::new();
    let body = b.add_box([0.0; 3], [8.0, 4.0, 2.0]);
    b.component(&[body]);
    for id in [MOVE_PART, crate::link::LINK_INSERT] {
        let a = b.modules.extensions.action(id, &b.cx()).unwrap();
        for (key, unit) in [
            ("x", kmb_core::FieldUnit::Length),
            ("rx", kmb_core::FieldUnit::Angle),
        ] {
            assert_eq!(
                a.args.iter().find(|a| a.key == key).unwrap().unit,
                Some(unit)
            );
        }
    }
    for args in [
        json!({"bodies":[body.0],"unknown":1}),
        json!({"bodies":[body.0],"rx":true}),
        json!({"bodies":[body.0],"x":[]}),
        json!({"bodies":[body.0],"rz":""}),
    ] {
        assert_eq!(
            b.run(MOVE_PART, args).unwrap_err().kind,
            kmb_core::ProblemKind::Input
        );
    }
    b.run(MOVE_PART, json!({"bodies":[body.0]})).unwrap();
    assert!(near(center(b.bounds(body)), [4.0, 2.0, 1.0]));
}

fn scratch(name: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("placement-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn inserted_link_placement_is_optional_and_undoes_with_the_component() {
    let dir = scratch("insert");
    let path = dir.join("source.json");
    let mut source = Bench::new();
    source.add_box([10.0, 20.0, 30.0], [8.0, 4.0, 2.0]);
    source.add_box([30.0, 20.0, 30.0], [4.0, 4.0, 2.0]);
    std::fs::write(&path, source.doc.to_json()).unwrap();
    let mut b = Bench::new();
    for _ in 0..40 {
        b.doc.alloc_group_id();
    }
    let depth = b.doc.undo_depth();
    let ids = b
        .run_file(
            crate::link::LINK_INSERT,
            json!({"x":5,"rz":90}),
            None,
            Some(&path),
        )
        .unwrap();
    assert_eq!(ids.len(), 4);
    let component = b.doc.data().component_of(ids[1]).unwrap().id;
    let p = &b.doc.data().feature(ids[3]).unwrap().data;
    assert_eq!(p["moving"], json!({"kind":"component","id":component}));
    assert_eq!(p["center_bodies"], json!([ids[1].0, ids[2].0]));
    assert!(near(center(b.bounds(ids[1])), [27.0, 14.0, 31.0]));
    assert!(near(center(b.bounds(ids[2])), [27.0, 32.0, 31.0]));
    assert_eq!(b.doc.undo_depth(), depth + 1);
    assert!(b.doc.undo());
    b.recompute();
    assert!(b.doc.data().features.is_empty() && b.doc.data().components.is_empty());
    let ids = b
        .run_file(crate::link::LINK_INSERT, json!({}), None, Some(&path))
        .unwrap();
    assert_eq!(ids.len(), 3);
    assert!(!b.doc.data().features.iter().any(|f| f.type_id == POSITION));
    assert!(near(center(b.bounds(ids[1])), [14.0, 22.0, 31.0]));
    assert!(b.doc.undo());
    b.recompute();
    let explicit_zero = b
        .run_file(crate::link::LINK_INSERT, json!({"x":0}), None, Some(&path))
        .unwrap();
    assert_eq!(explicit_zero.len(), 4, "明示した 0 は配置の指定");
    assert_eq!(b.doc.data().features.last().unwrap().type_id, POSITION);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn two_links_of_one_source_keep_the_joint_on_the_same_component() {
    let dir = scratch("identity");
    let path = dir.join("source.json");
    let other = dir.join("replacement.json");
    let mut source = Bench::new();
    let original = source.add_box([0.0; 3], [8.0, 4.0, 2.0]);
    std::fs::write(&path, source.doc.to_json()).unwrap();
    let mut b = Bench::new();
    let left = b
        .run_file(crate::link::LINK_INSERT, json!({}), None, Some(&path))
        .unwrap();
    let right = b
        .run_file(crate::link::LINK_INSERT, json!({"x":50}), None, Some(&path))
        .unwrap();
    let component = b.doc.data().component_of(right[1]).unwrap().id;
    let j=b.insert(JOINT,json!({"kind":"rigid","moving":{"kind":"component","id":component},"bodies":[right[1].0],"origin":[50,0,0],"axis":[0,0,1]}));
    let joint = b.doc.data().feature(j).unwrap().data.clone();
    b.doc = Document::from_json(&b.doc.to_json()).unwrap();
    b.recompute();
    source.set(original, json!({"at":[0,0,0],"size":[8,4,3]}));
    std::fs::write(&path, source.doc.to_json()).unwrap();
    for link in [left[0], right[0]] {
        b.run_file(crate::link::LINK_UPDATE, json!({}), Some(link), None)
            .unwrap();
        assert_eq!(b.doc.data().feature(j).unwrap().data, joint);
        assert_eq!(b.doc.data().component_of(right[1]).unwrap().id, component);
    }
    let mut replacement = Bench::new();
    for _ in 0..20 {
        replacement.doc.alloc_group_id();
    }
    let replacement_body = replacement.add_box([0.0; 3], [8.0, 4.0, 4.0]);
    assert_ne!(replacement_body, original);
    std::fs::write(&other, replacement.doc.to_json()).unwrap();
    b.run_file(
        crate::link::LINK_RELINK,
        json!({}),
        Some(right[0]),
        Some(&other),
    )
    .unwrap();
    assert_eq!(b.doc.data().feature(j).unwrap().data, joint);
    assert_eq!(b.doc.data().component_of(right[1]).unwrap().id, component);
    assert_eq!(
        b.doc.data().feature(right[1]).unwrap().data["link"],
        json!(right[0].0)
    );
    assert_eq!(
        b.doc.data().feature(right[1]).unwrap().data["part"],
        json!(replacement_body.0)
    );
    assert!(near(center(b.bounds(right[1])), [54.0, 2.0, 2.0]));
    assert!(near(center(b.bounds(left[1])), [4.0, 2.0, 1.5]));
    std::fs::remove_dir_all(dir).unwrap();
}
