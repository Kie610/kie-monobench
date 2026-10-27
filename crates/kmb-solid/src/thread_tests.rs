//! 確かめること: ねじの溝だけを引き、大きな軸・細かいピッチ・長いねじと穴が閉じた形になる。
use crate::tests::{TestKernel, assert_ok, registry};
use crate::*;
use kmb_core::{Document, FeatureId, FeatureStatus, ShapeHandle, Value};
use kmb_samples::sample::{DocBuilder, SketchBuilder};
use serde_json::json;
use std::f64::consts::PI;

fn shape(report: &kmb_core::RecomputeReport, id: FeatureId) -> ShapeHandle {
    match report.feature(id) {
        Some(FeatureStatus::Ok { outputs, .. }) => match outputs["body"] {
            Value::Shape(h) => h,
            _ => panic!("ボディ"),
        },
        other => panic!("{other:?}"),
    }
}

fn cylinder(diameter: f64, height: f64) -> (Document, FeatureId) {
    let mut builder = DocBuilder::new();
    builder.param("diameter", &format!("{diameter} mm"));
    builder.param("height", &format!("{height} mm"));
    let mut sketch = SketchBuilder::xy();
    let center = sketch.fixed_point(0.0, 0.0);
    sketch.circle(center, "diameter", diameter / 2.0);
    let profile = builder.sketch("軸の輪郭", sketch);
    let body = builder.feature(
        EXTRUDE,
        "軸",
        Extrude {
            profile: Some(profile),
            distance: "height".into(),
            ..Default::default()
        },
    );
    (builder.finish("ねじの軸"), body)
}

fn insert_thread(
    doc: &mut Document,
    kernel: &TestKernel,
    target: FeatureId,
    size: &str,
    extra: serde_json::Value,
) -> FeatureId {
    let report = doc.recompute(&registry(), &**kernel);
    assert_ok(&report);
    let face = kernel
        .arc_edges(shape(&report, target))
        .unwrap()
        .into_iter()
        .find_map(|e| e.arc.map(|_| e.faces[1].clone()))
        .unwrap();
    let mut data = json!({"target":target.0,"face":face,"size":size,"form":"modeled"});
    data.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    doc.apply_edits(
        "ねじ",
        vec![kmb_core::Edit::Insert {
            type_id: THREAD.into(),
            name: "ねじ".into(),
            data,
        }],
    )
    .unwrap()[0]
}

fn closed(kernel: &TestKernel, shape: ShapeHandle) {
    if let TestKernel::Smlib(k) = kernel {
        assert_eq!(k.check(shape).unwrap(), (true, 1, 0));
    }
}

fn outer_removed(diameter: f64, major: f64, pitch: f64, height: f64, clearance: f64) -> f64 {
    let r = diameter / 2.0;
    let root = major / 2.0 - 5.0 * 3f64.sqrt() * pitch / 16.0 - clearance / 2.0;
    let integral = pitch / 8.0 * (r * r - root * root)
        + 2.0 / 3f64.sqrt()
            * ((r.powi(3) - root.powi(3)) / 3.0 - root * (r * r - root * root) / 2.0);
    2.0 * PI * height / pitch * integral
}

#[test]
fn modeled_threads_keep_large_fine_long_and_print_clearance_shafts_closed() {
    let kernel = TestKernel::new();
    if kernel.is_truck() {
        kmb_contract::skip!("truck はらせんの掃引に対応していない");
    }
    for (diameter, major, pitch, height, clearance, size) in [
        (8.0, 8.0, 1.25, 20.0, 0.0, "M8"),
        (36.0, 36.0, 4.0, 40.0, 0.0, "M36"),
        (8.0, 8.0, 0.75, 80.0, 0.0, "M8x0.75"),
        (7.8, 8.0, 1.25, 20.0, 0.2, "M8"),
    ] {
        let (mut doc, target) = cylinder(diameter, height);
        let id = insert_thread(
            &mut doc,
            &kernel,
            target,
            size,
            json!({"clearance":format!("{clearance} mm")}),
        );
        let report = doc.recompute(&registry(), &*kernel);
        assert_ok(&report);
        let h = shape(&report, id);
        closed(&kernel, h);
        let volume = kernel.volume(h).unwrap();
        let expected = PI * (diameter / 2.0).powi(2) * height
            - outer_removed(diameter, major, pitch, height, clearance);
        assert!(
            (volume - expected).abs() < expected * 0.0002,
            "{size}: {volume} / {expected}"
        );
        let mesh = kernel.mesh(h, 0.03).unwrap();
        assert!(
            mesh.positions
                .iter()
                .all(|p| (p[0] as f64).hypot(p[1] as f64) <= diameter / 2.0 + 0.005)
        );
        if height / pitch > 100.0 {
            assert!(format!("{:?}", report.feature(id)).contains("再計算に時間"));
        }
    }
}

#[test]
fn modeled_thread_run_out_and_recomputed_dimensions_keep_a_closed_body() {
    let kernel = TestKernel::new();
    if kernel.is_truck() {
        kmb_contract::skip!("truck はらせんの掃引に対応していない");
    }
    let (mut doc, target) = cylinder(8.0, 10.0);
    let id = insert_thread(
        &mut doc,
        &kernel,
        target,
        "M8",
        json!({"length":"5 mm","hand":"left"}),
    );
    for (height, flip) in [(10.0, false), (12.0, true)] {
        let mut data = doc.data().feature(id).unwrap().data.clone();
        data["flip"] = json!(flip);
        data["end"] = json!(if flip { "abrupt" } else { "run_out" });
        doc.apply_edits("始まりの端", vec![kmb_core::Edit::SetData { id, data }])
            .unwrap();
        let p = doc.data().param_by_name("height").unwrap().id;
        doc.apply(kmb_core::Transaction::new(
            "長さ",
            vec![kmb_core::Op::SetParamExpr {
                id: p,
                expr: format!("{height} mm"),
            }],
        ))
        .unwrap();
        let report = doc.recompute(&registry(), &*kernel);
        assert_ok(&report);
        let result = shape(&report, id);
        closed(&kernel, result);
        let v = kernel.volume(result).unwrap();
        let original = PI * 16.0 * height;
        assert!(v < original && v > original - outer_removed(8.0, 8.0, 1.25, 7.0, 0.0));
        let mesh = kernel.mesh(result, 0.03).unwrap();
        let groove: Vec<_> = mesh
            .positions
            .iter()
            .filter(|p| {
                p[2] > 0.1 && (p[2] as f64) < height - 0.1 && (p[0] as f64).hypot(p[1] as f64) < 3.8
            })
            .collect();
        assert!(!groove.is_empty());
        assert!(groove.iter().all(|p| if flip {
            (p[2] as f64) < 7.0
        } else {
            p[2] as f64 > height - 7.0
        }));
        let status = report.feature(id).unwrap();
        if let FeatureStatus::Ok { outputs, .. } = status {
            assert!(!outputs.contains_key(kmb_interchange::THREAD_MARKS_OUTPUT));
        }
    }
}

#[test]
fn modeled_threads_cross_a_chamfered_tip_without_cutting_the_shaft_away() {
    let kernel = TestKernel::new();
    if kernel.is_truck() {
        kmb_contract::skip!("truck はらせんの掃引に対応していない");
    }
    let (mut doc, target) = cylinder(8.0, 10.0);
    let chamfer = doc.apply_edits("先端の面取り", vec![kmb_core::Edit::Insert {
        type_id: CHAMFER.into(), name: "先端の面取り".into(),
        data: json!({"target":target.0,"faces":[format!("f{}:end", target.0)],"arcs":true,"distance":"0.4 mm"}),
    }]).unwrap()[0];
    let id = insert_thread(&mut doc, &kernel, chamfer, "M8", json!({}));
    let report = doc.recompute(&registry(), &*kernel);
    assert_ok(&report);
    let result = shape(&report, id);
    closed(&kernel, result);
    let v = kernel.volume(result).unwrap();
    let cylindrical_thread = PI * 16.0 * 10.0 - outer_removed(8.0, 8.0, 1.25, 10.0, 0.0);
    assert!(
        v <= cylindrical_thread + 0.1 && v > cylindrical_thread - 4.0,
        "{v}/{cylindrical_thread}"
    );
}

#[test]
fn modeled_threads_reject_oversize_shafts_and_excessive_lengths() {
    let kernel = TestKernel::new();
    if kernel.is_truck() {
        kmb_contract::skip!("truck は円筒を調べる円の辺を返さない");
    }
    for (diameter, extra, message) in [
        (10.0, json!({}), "太すぎ"),
        (6.0, json!({}), "細すぎ"),
        (8.0, json!({"length":"30 mm"}), "最大"),
    ] {
        let (mut doc, target) = cylinder(diameter, 10.0);
        let id = insert_thread(&mut doc, &kernel, target, "M8", extra);
        let report = doc.recompute(&registry(), &*kernel);
        let text = format!("{:?}", report.feature(id));
        assert!(text.contains(message), "{text}");
    }
}

#[test]
fn modeled_thread_reports_unsupported_on_truck_and_never_falls_back() {
    let kernel = TestKernel::new();
    if kernel.is_truck() {
        let profile = kmb_core::kernel::Profile {
            plane: kmb_core::kernel::Plane::ZX,
            regions: vec![],
        };
        let helix = kmb_core::kernel::Helix {
            origin: [0.0; 3],
            axis: [0.0, 0.0, 1.0],
            left: false,
            spans: vec![kmb_core::kernel::HelixSpan {
                turns: 1.0,
                pitch: 1.0,
            }],
            blend: 0.0,
            run_out: None,
        };
        assert!(
            matches!(kernel.helical_sweep(&profile, &helix, "thread"), Err(kmb_core::KernelError::Unsupported(message)) if message == "らせんの掃引")
        );
    } else {
        let (mut doc, target) = cylinder(8.0, 4.0);
        let id = insert_thread(&mut doc, &kernel, target, "M8", json!({}));
        let report = doc.recompute(&registry(), &*kernel);
        assert_ok(&report);
        closed(&kernel, shape(&report, id));
    }
}

#[test]
fn modeled_tapped_holes_cut_internal_crests_and_explain_blind_depth_limits() {
    let kernel = TestKernel::new();
    if kernel.is_truck() {
        kmb_contract::skip!("truck はらせんの掃引に対応していない");
    }
    for size in ["M6", "M36"] {
        let size = thread::parse_size(size).unwrap();
        let mut builder = DocBuilder::new();
        let mut sketch = SketchBuilder::xy();
        let center = sketch.fixed_point(0.0, 0.0);
        let radius = size.major / 2.0 + 10.0;
        sketch.circle(center, &format!("{} mm", radius * 2.0), radius);
        let profile = builder.sketch("円板", sketch);
        let target = builder.feature(
            EXTRUDE,
            "円板",
            Extrude {
                profile: Some(profile),
                distance: "10 mm".into(),
                ..Default::default()
            },
        );
        let mut sketch = SketchBuilder::new(kmb_sketch::SketchPlane::face(
            target,
            format!("f{}:end", target.0),
        ));
        let center = sketch.fixed_point(0.0, 0.0);
        let at = builder.sketch("穴の位置", sketch);
        let id = builder.feature(
            HOLE,
            "ねじ穴",
            Hole {
                points: vec![kmb_core::PointId(center.0)],
                tap: Tap::Tapped,
                size: Some(size.name()),
                form: thread::Form::Modeled,
                ..Hole::new(target, at)
            },
        );
        let mut doc = builder.finish("めねじ");
        let report = doc.recompute(&registry(), &*kernel);
        assert_ok(&report);
        let h = shape(&report, id);
        closed(&kernel, h);
        let root = size.major / 2.0;
        let bore = size.tap_drill() / 2.0;
        let integral = size.pitch / 16.0 * (root * root - bore * bore)
            + 2.0 / 3f64.sqrt()
                * (root * (root * root - bore * bore) / 2.0 - (root.powi(3) - bore.powi(3)) / 3.0);
        let expected =
            PI * (radius * radius - bore * bore) * 10.0 - 2.0 * PI * 10.0 / size.pitch * integral;
        let actual = kernel.volume(h).unwrap();
        assert!(
            (actual - expected).abs() < expected * 0.0002,
            "{}: {actual}/{expected}",
            size.label()
        );
    }
    let mut doc = crate::tests::plate_with_hole(
        &[[15.0, 10.0]],
        |target, sketch, points| Hole {
            points,
            depth: Some("6 mm".into()),
            tap: Tap::Tapped,
            size: Some("M6".into()),
            form: thread::Form::Modeled,
            ..Hole::new(target, sketch)
        },
        false,
    );
    let report = doc.recompute(&registry(), &*kernel);
    let text = format!("{:?}", report.features.last().unwrap().status);
    assert!(
        text.contains("ねじの長さは最大") && text.contains("ねじの機能を別に追加"),
        "{text}"
    );
    let hole = doc.data().features.last().unwrap().id;
    let mut data = doc.data().feature(hole).unwrap().data.clone();
    data["form"] = json!("cosmetic");
    doc.apply_edits("属性のみ", vec![kmb_core::Edit::SetData { id: hole, data }])
        .unwrap();
    let id = insert_thread(&mut doc, &kernel, hole, "M6", json!({"length":"4 mm"}));
    let report = doc.recompute(&registry(), &*kernel);
    assert_ok(&report);
    closed(&kernel, shape(&report, id));
}
