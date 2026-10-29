//! 確かめること: 分解の対象を部品に絞り、追加済みの段の後の外形から離す。
use super::*;
use kmb_core::kernel::P3;
use kmb_core::{
    Context, Document, Edit, Extensions, FeatureEntry, FeatureId, FeatureResult, FeatureStatus,
    Kernel, KernelError, Op, Outcome, RecomputeReport, Registry, ShapeHandle, Transaction, Value,
};
use kmb_interchange::animation::{self as anim, StepKind, Target};
use serde_json::json;
use std::sync::Arc;

struct Boxes(Vec<(P3, P3)>);
impl Kernel for Boxes {
    fn name(&self) -> &str {
        "bounds-test"
    }
    fn bounds(&self, h: ShapeHandle) -> Result<(P3, P3), KernelError> {
        self.0
            .get(h.0 as usize - 1)
            .copied()
            .ok_or(KernelError::UnknownShape(h))
    }
}
struct Fixture {
    doc: Document,
    report: RecomputeReport,
    reg: Registry,
    kernel: Arc<dyn Kernel>,
    lid: u64,
    knob: u64,
}
impl Fixture {
    fn new() -> Self {
        let mut doc = Document::new();
        let mut report = RecomputeReport::default();
        for name in ["箱", "蓋", "つまみ"] {
            let id = doc.alloc_feature_id();
            doc.apply(Transaction::new(
                "形",
                vec![Op::InsertFeature {
                    index: None,
                    entry: FeatureEntry {
                        id,
                        type_id: "test.box@1".into(),
                        name: name.into(),
                        data: json!({}),
                        suppressed: false,
                        pin_bodies: false,
                    },
                }],
            ))
            .unwrap();
            report.features.push(FeatureResult {
                id,
                name: name.into(),
                status: FeatureStatus::Ok {
                    outputs: [("body".into(), Value::Shape(ShapeHandle(id.0)))].into(),
                    reused: false,
                },
            });
        }
        let lid = doc.alloc_group_id();
        let knob = doc.alloc_group_id();
        doc.apply(Transaction::new(
            "部品",
            vec![Op::SetComponents {
                components: vec![
                    Component {
                        id: lid,
                        name: "蓋の部品".into(),
                        parent: None,
                        bodies: vec![FeatureId(2)],
                    },
                    Component {
                        id: knob,
                        name: "つまみの部品".into(),
                        parent: Some(lid),
                        bodies: vec![FeatureId(3)],
                    },
                ],
            }],
        ))
        .unwrap();
        let kernel: Arc<dyn Kernel> = Arc::new(Boxes(vec![
            ([0., 0., 0.], [30., 20., 10.]),
            ([0., 0., 10.], [30., 20., 12.]),
            ([13., 8., 12.], [17., 12., 16.]),
        ]));
        Self {
            doc,
            report,
            reg: Registry::new(),
            kernel,
            lid,
            knob,
        }
    }
    fn change(
        &self,
        action: &str,
        args: serde_json::Value,
    ) -> Result<(String, Vec<Edit>), kmb_core::Problem> {
        let cx = Context {
            args: &args,
            ..Context::new(self.doc.data(), &self.report, &self.reg, &self.kernel)
        };
        let outcome = Extensions(vec![Arc::new(AsmCommands)]).run(action, &cx)?;
        let Outcome::Change { label, edits } = outcome else {
            panic!("段を足す変更")
        };
        Ok((label, edits))
    }
    fn run(&mut self, action: &str, args: serde_json::Value) {
        let (label, edits) = self.change(action, args).unwrap();
        self.doc.apply_edits(&label, edits).unwrap();
    }
}

#[test]
fn scoped_auto_explode_uses_only_the_selected_components_children() {
    let mut f = Fixture::new();
    f.run(crate::anim::AUTO_EXPLODE_ONE, json!({"component":f.lid}));
    let steps = anim::steps(f.doc.data());
    let StepKind::ExplodeMany { moves } = &steps[0].kind else {
        panic!("同時の段")
    };
    assert_eq!(moves.len(), 2);
    assert_eq!(moves[0].targets, vec![Target::Component { id: f.knob }]);
    assert_eq!(moves[0].offset, [0., 0., 4.]);
    assert_eq!(moves[1].targets, vec![Target::Body { root: FeatureId(2) }]);
    assert_eq!(moves[1].offset, [0., 0., -2.]);
    assert!(
        moves
            .iter()
            .flat_map(|m| &m.targets)
            .flat_map(|t| t.roots(f.doc.data()))
            .all(|r| r != FeatureId(1))
    );
}

#[test]
fn repeated_auto_explode_measures_the_ended_nested_steps() {
    let mut f = Fixture::new();
    f.run(crate::anim::AUTO_EXPLODE_ALL, json!({}));
    let before = anim::steps(f.doc.data());
    f.run(crate::anim::AUTO_EXPLODE_ONE, json!({"time":0.0}));
    let steps = anim::steps(f.doc.data());
    let StepKind::ExplodeMany { moves } = &steps.last().unwrap().kind else {
        panic!("同時の段")
    };
    let moved_lid = moves
        .iter()
        .find(|m| m.targets == vec![Target::Component { id: f.lid }])
        .unwrap();
    assert_eq!(
        moved_lid.offset,
        [0., 0., 12.],
        "前の内部分解で蓋の部品の高さは6から12になる"
    );
    let end = anim::poses(
        f.doc.data(),
        &f.reg,
        &f.report,
        &steps,
        anim::duration(&steps),
    );
    assert_eq!(
        kmb_core::kernel::transform_point(&end[&FeatureId(1)], [0., 0., 0.]),
        [0., 0., -20.]
    );
    assert_eq!(
        kmb_core::kernel::transform_point(&end[&FeatureId(2)], [0., 0., 10.]),
        [0., 0., 26.]
    );
    assert_eq!(
        kmb_core::kernel::transform_point(&end[&FeatureId(3)], [0., 0., 12.]),
        [0., 0., 34.]
    );
    f.doc.undo();
    assert_eq!(anim::steps(f.doc.data()), before);
}

#[test]
fn scoped_auto_explode_checks_the_direct_children_without_descending_a_single_child() {
    let mut f = Fixture::new();
    let top = f.doc.alloc_group_id();
    let mut components = f.doc.data().components.clone();
    components[0].parent = Some(top);
    components[0].bodies.push(FeatureId(1));
    components.push(Component {
        id: top,
        name: "外側".into(),
        parent: None,
        bodies: vec![],
    });
    f.doc
        .apply(Transaction::new(
            "部品",
            vec![Op::SetComponents { components }],
        ))
        .unwrap();
    for action in [crate::anim::AUTO_EXPLODE_ONE, crate::anim::AUTO_EXPLODE_ALL] {
        let error = f.change(action, json!({"component":top})).unwrap_err();
        assert_eq!(error.kind, kmb_core::ProblemKind::Input);
        assert!(
            error
                .message
                .contains("分けて動かせる物が 2 つ以上ありません")
        );
        assert!(
            f.change(action, json!({})).is_ok(),
            "省くと1子の中の分岐まで下りる"
        );
    }
}

#[test]
fn scoped_auto_explode_preserves_defaults_expressions_and_typed_rejections() {
    let f = Fixture::new();
    let original = f.doc.to_json();
    for action in [crate::anim::AUTO_EXPLODE_ONE, crate::anim::AUTO_EXPLODE_ALL] {
        let (label, edits) = f.change(action, json!({"component":f.lid})).unwrap();
        let (same_label, same_edits) = f
            .change(
                action,
                json!({"component":f.lid as f64, "sequential":false,"trail":true,"scale":"2 / 2"}),
            )
            .unwrap();
        assert_eq!((label, edits), (same_label, same_edits));
        for args in [
            json!({"component":-1}),
            json!({"component":2.5}),
            json!({"component":999}),
            json!({"component":true}),
            json!({"component":"4"}),
            json!({"component":f.knob}),
            json!({"component":1}),
            json!({"components":f.lid}),
            json!({"scale":0}),
        ] {
            assert_eq!(
                f.change(action, args.clone()).unwrap_err().kind,
                kmb_core::ProblemKind::Input,
                "{args}"
            );
        }
        assert_eq!(f.doc.to_json(), original, "確かめでは文書を変えない");
    }
}

#[test]
fn scoped_auto_explode_all_remains_inside_the_component_and_undoes_as_one_change() {
    for sequential in [false, true] {
        let mut f = Fixture::new();
        let before = f.doc.to_json();
        f.run(
            crate::anim::AUTO_EXPLODE_ALL,
            json!({"component":f.lid,"sequential":sequential,"trail":false,"scale":"2 / 1"}),
        );
        let steps = anim::steps(f.doc.data());
        assert_eq!(steps.len(), if sequential { 2 } else { 1 });
        assert!(steps.iter().all(|s| !s.trail));
        let end = anim::poses(
            f.doc.data(),
            &f.reg,
            &f.report,
            &steps,
            anim::duration(&steps),
        );
        assert!(!end.contains_key(&FeatureId(1)));
        assert_eq!(
            kmb_core::kernel::transform_point(&end[&FeatureId(2)], [0., 0., 10.]),
            [0., 0., 6.]
        );
        assert_eq!(
            kmb_core::kernel::transform_point(&end[&FeatureId(3)], [0., 0., 12.]),
            [0., 0., 20.]
        );
        f.doc.undo();
        assert_eq!(f.doc.to_json(), before);
    }
}

#[test]
fn auto_explode_uses_all_eight_corners_after_a_rotation_and_translation() {
    use kmb_interchange::animation::{Drive, Step};
    let mut f = Fixture::new();
    f.kernel = Arc::new(Boxes(vec![
        ([40., 0., 0.], [50., 2., 1.]),
        ([0., 0., 0.], [10., 2., 1.]),
        ([-20., 0., 0.], [-10., 2., 1.]),
    ]));
    let joint = f.doc.alloc_feature_id();
    f.doc
        .apply(Transaction::new(
            "軸",
            vec![Op::InsertFeature {
                index: None,
                entry: FeatureEntry {
                    id: joint,
                    type_id: "test.drive@1".into(),
                    name: "回転".into(),
                    data: json!({}),
                    suppressed: false,
                    pin_bodies: false,
                },
            }],
        ))
        .unwrap();
    let drive = Drive {
        slide: false,
        origin: [0.; 3],
        axis: [0., 0., 1.],
        value: 0.,
        min: None,
        max: None,
        bodies: vec![FeatureId(2)],
        dofs: vec![],
        lead: None,
    };
    f.report.features.push(FeatureResult {
        id: joint,
        name: "回転".into(),
        status: FeatureStatus::Ok {
            outputs: [(anim::DRIVE_SLOT.into(), drive.to_value())].into(),
            reused: false,
        },
    });
    let steps = vec![
        Step::new(
            StepKind::Drive {
                joint,
                from: 0.,
                to: 45.,
            },
            1.,
        ),
        Step::new(
            StepKind::Explode {
                targets: vec![Target::Body { root: FeatureId(2) }],
                offset: [2., -3., 4.],
            },
            1.,
        ),
    ];
    f.doc
        .apply_edits("前段", vec![anim::set_steps(f.doc.data(), steps).unwrap()])
        .unwrap();
    for (scale, expected) in [("1", 8.49), ("2 / 1", 16.97)] {
        let mut results = Vec::new();
        for cursor in [0., 1000.] {
            let (_, edits) = f
                .change(
                    crate::anim::AUTO_EXPLODE_ONE,
                    json!({"component":f.lid,"scale":scale,"time":cursor}),
                )
                .unwrap();
            let mut copy = Document::from_json(&f.doc.to_json()).unwrap();
            copy.apply_edits("次", edits).unwrap();
            let all = anim::steps(copy.data());
            let StepKind::ExplodeMany { moves } = &all.last().unwrap().kind else {
                panic!("同時の段")
            };
            let m = moves
                .iter()
                .find(|m| m.targets == vec![Target::Body { root: FeatureId(2) }])
                .unwrap();
            assert_eq!(
                m.offset,
                [expected, 0., 0.],
                "45度の箱のX幅は12/√2。対角2点だけなら幅を落とす"
            );
            results.push(all);
        }
        assert_eq!(
            results[0], results[1],
            "カーソルの時刻ではなく全段の後から決める"
        );
    }
}
