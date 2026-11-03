//! 確かめること: 面の候補と、辺を軸にする候補を同じ中点で選び分ける。

use crate::snap::{SnapPick, choices};
use kmb_core::kernel::{EdgeInfo, FaceInfo, KernelError, PlanarFace, Seam};
use kmb_core::{
    Context, DocumentData, FacePick, FeatureId, FeatureResult, FeatureStatus, Kernel,
    RecomputeReport, Registry, ShapeHandle, Value,
};
use std::sync::Arc;

struct Rectangle;

impl Kernel for Rectangle {
    fn name(&self) -> &str {
        "snap-test"
    }
    fn faces(&self, _: ShapeHandle) -> Result<Vec<FaceInfo>, KernelError> {
        Ok(vec![FaceInfo {
            name: "top".into(),
            planar: Some(PlanarFace {
                point: [0.; 3],
                normal: [0., 0., 2.],
            }),
            area: 1200.,
        }])
    }
    fn edges(&self, _: ShapeHandle) -> Result<Vec<EdgeInfo>, KernelError> {
        let points = [[0., 0., 0.], [40., 0., 0.], [40., 30., 0.], [0., 30., 0.]];
        Ok((0..4)
            .map(|i| EdgeInfo {
                faces: ["top".into(), format!("side{i}")],
                a: points[i],
                b: points[(i + 1) % 4],
                convex: true,
                seam: Seam::Corner,
                arc: None,
                curve: vec![],
            })
            .collect())
    }
}

fn choices_using(kernel: Arc<dyn Kernel>) -> Vec<SnapPick> {
    let data = DocumentData::default();
    let mut report = RecomputeReport::default();
    report.features.push(FeatureResult {
        id: FeatureId(1),
        name: "面".into(),
        status: FeatureStatus::Ok {
            outputs: [("body".into(), Value::Shape(ShapeHandle(1)))].into(),
            reused: false,
        },
    });
    let registry = Registry::default();
    let cx = Context::new(&data, &report, &registry, &kernel);
    choices(
        &cx,
        &FacePick {
            feature: FeatureId(1),
            face: "top".into(),
        },
    )
    .into_iter()
    .map(|c| serde_json::from_value(c.value).unwrap())
    .collect()
}

fn rectangle_choices() -> Vec<SnapPick> {
    choices_using(Arc::new(Rectangle))
}

#[test]
fn straight_edge_midpoints_offer_the_edge_as_the_joint_axis() {
    let picks = rectangle_choices();
    let axes: Vec<_> = picks
        .iter()
        .filter(|p| p.label == "辺の向きを軸に(中点)")
        .collect();
    assert_eq!(axes.len(), 4, "4 本の辺を、それぞれ軸にできる");
    for (pick, (point, direction)) in axes.iter().zip([
        ([20., 0., 0.], [1., 0., 0.]),
        ([40., 15., 0.], [0., 1., 0.]),
        ([20., 30., 0.], [-1., 0., 0.]),
        ([0., 15., 0.], [0., -1., 0.]),
    ]) {
        assert_eq!(pick.frame.origin, point);
        assert_eq!(pick.frame.z, direction);
        assert_eq!(pick.frame.x, [0., 0., 1.]);
        assert!(picks.iter().any(|p| p.label == "辺の中点"
            && p.frame.origin == point
            && p.frame.z == [0., 0., 1.]));
    }
}

/// 確かめること: 辺を軸に合わせた実形が、辺のまわりに回り、辺の方向に滑る。
#[test]
fn edge_axis_candidates_drive_a_real_hinge_and_slider() {
    use kmb_core::{Document, FeatureEntry, Op, Outcome, Transaction};
    use serde_json::json;
    let modules = kmb_bundle::modules();
    assert_eq!(
        modules.kernel.name(),
        "smlib",
        "実形の試験は SMLib の組み立てで流す"
    );
    let mut doc = Document::new();
    let mut roots = Vec::new();
    for (x, width, depth, height) in [(0., 40., 30., 5.), (100., 4., 4., 10.)] {
        let sk = doc.alloc_feature_id();
        let body = doc.alloc_feature_id();
        let points = [[x, 0.], [x + width, 0.], [x + width, depth], [x, depth]];
        let mut entities = Vec::new();
        for (i, p) in points.iter().enumerate() {
            entities.push(json!({"type":"point","id":i+1,"x":p[0],"y":p[1]}));
        }
        for i in 0..4 {
            entities.push(json!({"type":"line","id":i+5,"a":i+1,"b":(i+1)%4+1}));
        }
        doc.apply(Transaction::new(
            "箱を用意",
            vec![
                Op::InsertFeature {
                    index: None,
                    entry: FeatureEntry {
                        id: sk,
                        type_id: "cad.sketch@1".into(),
                        name: "輪郭".into(),
                        data: json!({"entities":entities}),
                        suppressed: false,
                        pin_bodies: false,
                    },
                },
                Op::InsertFeature {
                    index: None,
                    entry: FeatureEntry {
                        id: body,
                        type_id: "solid.extrude@1".into(),
                        name: "箱".into(),
                        data: json!({"profile":sk.0,"distance":format!("{height} mm")}),
                        suppressed: false,
                        pin_bodies: false,
                    },
                },
            ],
        ))
        .unwrap();
        roots.push(body);
    }
    let mut report = doc.recompute(&modules.registry, modules.kernel.as_ref());
    assert_eq!(report.error_count(), 0, "{report:?}");
    let snap_on = |feature, normal, report: &RecomputeReport| {
        let h = match report.feature(feature).unwrap() {
            FeatureStatus::Ok { outputs, .. } => match outputs["body"] {
                Value::Shape(h) => h,
                _ => panic!("形"),
            },
            _ => panic!("計算できる"),
        };
        let face = modules
            .kernel
            .faces(h)
            .unwrap()
            .into_iter()
            .find(|f| f.planar.is_some_and(|p| p.normal == normal))
            .unwrap()
            .name;
        let cx = Context::new(doc.data(), report, &modules.registry, &modules.kernel);
        choices(&cx, &FacePick { feature, face })
            .into_iter()
            .map(|c| serde_json::from_value::<SnapPick>(c.value).unwrap())
            .find(|p| p.label == "辺の向きを軸に(中点)" && p.frame.z[0].abs() > 0.999)
            .unwrap()
    };
    let from = snap_on(roots[1], [0., 0., -1.], &report);
    let to = snap_on(roots[0], [0., 0., 1.], &report);
    assert_eq!(to.frame.origin[0], 20.);
    assert_eq!(to.frame.origin[2], 5.);
    assert!(to.frame.origin[1] == 0. || to.frame.origin[1] == 30.);
    let args = json!({"moving":"f","from":from,"base":"f","to":to});
    let cx = Context {
        args: &args,
        ..Context::new(doc.data(), &report, &modules.registry, &modules.kernel)
    };
    let Outcome::Change { label, edits } =
        modules.extensions.run(crate::NEW_SNAP_JOINT, &cx).unwrap()
    else {
        panic!("ジョイントを足す")
    };
    let joint = doc.apply_edits(&label, edits).unwrap()[0];
    report = doc.recompute(&modules.registry, modules.kernel.as_ref());
    assert_eq!(report.error_count(), 0, "{report:?}");
    let moving_shape = |doc: &Document, report: &RecomputeReport| {
        kmb_core::visible_bodies(doc.data(), &modules.registry, report)
            .into_iter()
            .find(|(id, _)| kmb_core::body_root(doc.data(), &modules.registry, *id) == roots[1])
            .unwrap()
            .1
    };
    let h0 = moving_shape(&doc, &report);
    let initial = modules.kernel.bounds(h0).unwrap();
    let volume = modules.kernel.volume(h0).unwrap();
    let mut data = doc.data().feature(joint).unwrap().data.clone();
    data["kind"] = json!("revolute");
    data["angle"] = json!("90 deg");
    doc.apply_edits("回す", vec![kmb_core::Edit::SetData { id: joint, data }])
        .unwrap();
    report = doc.recompute(&modules.registry, modules.kernel.as_ref());
    assert_eq!(report.error_count(), 0);
    let h = moving_shape(&doc, &report);
    let rotated = modules.kernel.bounds(h).unwrap();
    let span = |b: ([f64; 3], [f64; 3])| [0, 1, 2].map(|i| b.1[i] - b.0[i]);
    let a = span(initial);
    let b = span(rotated);
    for (got, want) in b.into_iter().zip([a[0], a[2], a[1]]) {
        assert!((got - want).abs() < 1e-6, "{initial:?} {rotated:?}");
    }
    let center = |b: ([f64; 3], [f64; 3])| [0, 1, 2].map(|i| (b.0[i] + b.1[i]) / 2.);
    let c = center(initial);
    let o = to.frame.origin;
    let sign = to.frame.z[0];
    let want = [
        c[0],
        o[1] - sign * (c[2] - o[2]),
        o[2] + sign * (c[1] - o[1]),
    ];
    for (got, want) in center(rotated).into_iter().zip(want) {
        assert!((got - want).abs() < 1e-6);
    }
    assert!((modules.kernel.volume(h).unwrap() - volume).abs() < 1e-6);
    let mut data = doc.data().feature(joint).unwrap().data.clone();
    data["kind"] = json!("slider");
    data["angle"] = json!("0 deg");
    data["distance"] = json!("7 mm");
    doc.apply_edits(
        "滑らせる",
        vec![kmb_core::Edit::SetData { id: joint, data }],
    )
    .unwrap();
    report = doc.recompute(&modules.registry, modules.kernel.as_ref());
    assert_eq!(report.error_count(), 0);
    let h = moving_shape(&doc, &report);
    let slid = modules.kernel.bounds(h).unwrap();
    for i in 0..3 {
        assert!((slid.0[i] - initial.0[i] - to.frame.z[i] * 7.).abs() < 1e-6);
        assert!((slid.1[i] - initial.1[i] - to.frame.z[i] * 7.).abs() < 1e-6);
    }
    assert!((modules.kernel.volume(h).unwrap() - volume).abs() < 1e-6);
}

struct RepeatedRectangle;

impl Kernel for RepeatedRectangle {
    fn name(&self) -> &str {
        "snap-test"
    }
    fn faces(&self, h: ShapeHandle) -> Result<Vec<FaceInfo>, KernelError> {
        Rectangle.faces(h)
    }
    fn edges(&self, h: ShapeHandle) -> Result<Vec<EdgeInfo>, KernelError> {
        let mut edges = Rectangle.edges(h)?;
        edges.push(edges[0].clone());
        let mut reversed = edges[0].clone();
        std::mem::swap(&mut reversed.a, &mut reversed.b);
        edges.push(reversed);
        Ok(edges)
    }
}

/// 確かめること: 同じ中点と同じ軸は一つ、逆向きの軸は別に残り、面の候補の順を保つ。
#[test]
fn coincident_candidates_keep_different_axes_and_preserve_the_face_choices() {
    let ordinary = rectangle_choices();
    let repeated = choices_using(Arc::new(RepeatedRectangle));
    let face_choices = |picks: Vec<SnapPick>| {
        picks
            .into_iter()
            .filter(|p| p.label != "辺の向きを軸に(中点)")
            .collect::<Vec<_>>()
    };
    assert_eq!(
        face_choices(ordinary.clone()),
        face_choices(repeated.clone())
    );
    assert_eq!(face_choices(ordinary).len(), 9);
    let axes: Vec<_> = repeated
        .iter()
        .filter(|p| p.label == "辺の向きを軸に(中点)" && p.frame.origin == [20., 0., 0.])
        .collect();
    assert_eq!(axes.len(), 2);
    assert_eq!(axes[0].frame.z, [1., 0., 0.]);
    assert_eq!(axes[1].frame.z, [-1., 0., 0.]);
}

/// 確かめること: 閉じた円筒は公開の直線辺の対象ではなく、円の縁を持つ。
#[test]
fn a_closed_cylinder_has_no_straight_boundary_edges() {
    use kmb_core::kernel::{Plane, Profile, ProfileEdge, ProfileLoop, Region, Segment};
    let kernel = kmb_bundle::kernel();
    assert_eq!(kernel.name(), "smlib");
    let circle = Profile {
        plane: Plane::XY,
        regions: vec![Region {
            outer: ProfileLoop {
                edges: vec![ProfileEdge {
                    name: "rim".into(),
                    reversed: false,
                    segment: Segment::Circle {
                        center: [0., 0.],
                        radius: 5.,
                    },
                }],
            },
            holes: vec![],
        }],
    };
    let h = kernel.extrude(&circle, 0., 10., "cylinder").unwrap();
    assert!(kernel.edges(h).unwrap().is_empty());
    assert_eq!(kernel.arc_edges(h).unwrap().len(), 2);
}
