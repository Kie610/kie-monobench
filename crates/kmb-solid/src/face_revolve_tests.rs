//! ボディの平らな面を、スケッチと同じ輪郭として回す。
use crate::tests::{TestKernel, registry};
use crate::{Axis, EXTRUDE, Extrude, REVOLVE, Revolve};
use kmb_cad_revolve::{RevolveCommands, Source};
use kmb_core::{Context, Document, Extension, FeatureId, FeatureStatus, Kernel, Ref, Value};
use kmb_samples::sample::{DocBuilder, SketchBuilder};
use kmb_sketch::SketchPlane;
use serde_json::json;

fn face(f: FeatureId, end: &str) -> Ref {
    Ref::Face {
        feature: f,
        face: format!("f{}:{end}", f.0),
    }
}

fn mass(doc: &mut Document, k: &dyn Kernel, id: FeatureId) -> f64 {
    let report = doc.recompute(&registry(), k);
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(id) else {
        panic!("{:?}", report.feature(id));
    };
    let Some(Value::Shape(h)) = outputs.get("body") else {
        panic!("no body");
    };
    k.mass(*h).unwrap().volume
}

fn disk(hole: bool) -> (DocBuilder, FeatureId) {
    let mut b = DocBuilder::new();
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let c = s.fixed_point(0.0, 5.0);
    s.circle(c, "4 mm", 2.0);
    if hole {
        s.circle(c, "2 mm", 1.0);
    }
    let sk = b.feature(kmb_sketch::SKETCH, "円板の輪郭", s.build());
    let f = b.feature(
        EXTRUDE,
        "円板",
        Extrude {
            profile: Some(sk),
            distance: "3 mm".into(),
            ..Default::default()
        },
    );
    (b, f)
}

#[test]
fn revolving_a_disk_face_keeps_the_hole_and_the_saved_reference() {
    let k = TestKernel::new();
    for hole in [false, true] {
        let (mut b, f) = disk(hole);
        let mut line = SketchBuilder::new(SketchPlane::face(f, format!("f{}:end", f.0)));
        let a = line.fixed_point(0.0, 0.0);
        let end = line.fixed_point(10.0, 0.0);
        line.line(a, end);
        let axis = b.feature(kmb_sketch::SKETCH, "上面の軸", line.build());
        let r = b.feature(
            REVOLVE,
            "面を回す",
            Revolve {
                source: Source::Faces,
                faces: vec![face(f, "end")],
                axis: Axis::Points {
                    sketch: Some(axis),
                    a: Some(kmb_core::PointId(a.0)),
                    b: Some(kmb_core::PointId(end.0)),
                },
                ..Default::default()
            },
        );
        let mut doc = b.finish("面の回転");
        let expected = 2.0 * std::f64::consts::PI.powi(2) * 5.0 * if hole { 3.0 } else { 4.0 };
        let actual = mass(&mut doc, &*k, r);
        assert!(
            (actual - expected).abs() / expected < 2e-3,
            "hole={hole}, volume={actual}, expected={expected}"
        );
        let reg = registry();
        let report = doc.recompute(&reg, &*k);
        let ids: Vec<_> = kmb_core::visible_bodies(doc.data(), &reg, &report)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        assert_eq!(ids, vec![f, r]);
        let mut loaded = Document::from_json(&doc.to_json()).unwrap();
        assert!((mass(&mut loaded, &*k, r) - expected).abs() / expected < 2e-3);
    }
}

#[test]
fn revolving_a_face_rejects_an_axis_outside_its_plane() {
    let k = TestKernel::new();
    let (mut b, f) = disk(false);
    let r = b.feature(
        REVOLVE,
        "面を回す",
        Revolve {
            source: Source::Faces,
            faces: vec![face(f, "start")],
            axis: Axis::OriginZ,
            ..Default::default()
        },
    );
    let mut doc = b.finish("平面の外の軸");
    let report = doc.recompute(&registry(), &*k);
    let status = format!("{:?}", report.feature(r));
    assert!(
        status.contains("輪郭の平面に沿う軸を選んでください"),
        "{status}"
    );
}

#[test]
fn revolving_a_face_around_its_straight_edge_follows_dimensions() {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    b.param("length", "10 mm");
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let o = s.fixed_point(0.0, 0.0);
    s.rectangle(o, "length", "4 mm", [0.0, 0.0, 10.0, 4.0]);
    let sk = b.feature(kmb_sketch::SKETCH, "長方形", s.build());
    let f = b.feature(
        EXTRUDE,
        "板",
        Extrude {
            profile: Some(sk),
            distance: "3 mm".into(),
            ..Default::default()
        },
    );
    let mut doc = b.finish("板");
    let reg = registry();
    let report = doc.recompute(&reg, &*k);
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(f) else {
        panic!();
    };
    let Some(Value::Shape(h)) = outputs.get("body") else {
        panic!();
    };
    let e = k
        .edges(*h)
        .unwrap()
        .into_iter()
        .find(|e| {
            e.a[1].abs() < 1e-6 && e.b[1].abs() < 1e-6 && e.a[2].abs() < 1e-6 && e.b[2].abs() < 1e-6
        })
        .unwrap();
    let r = doc.alloc_feature_id();
    doc.apply(kmb_core::Transaction::new(
        "回転",
        vec![kmb_core::Op::InsertFeature {
            index: None,
            entry: kmb_core::FeatureEntry {
                id: r,
                type_id: REVOLVE.into(),
                name: "面を回す".into(),
                data: serde_json::to_value(Revolve {
                    source: Source::Faces,
                    faces: vec![face(f, "start")],
                    axis: Axis::Edge {
                        feature: f,
                        faces: e.faces.clone(),
                    },
                    ..Default::default()
                })
                .unwrap(),
                suppressed: false,
                pin_bodies: false,
            },
        }],
    ))
    .unwrap();
    for length in [10.0, 15.0] {
        let id = doc.data().param_by_name("length").unwrap().id;
        doc.apply(kmb_core::Transaction::new(
            "長さ",
            vec![kmb_core::Op::SetParamExpr {
                id,
                expr: format!("{length} mm"),
            }],
        ))
        .unwrap();
        let actual = mass(&mut doc, &*k, r);
        let expected = std::f64::consts::PI * 16.0 * length;
        assert!(
            (actual - expected).abs() / expected < 2e-3,
            "length={length}, volume={actual}, expected={expected}"
        );
    }
    // 同じ軸を含む別の平面の2面も、それぞれ回して合わせる。
    let data = serde_json::to_value(Revolve {
        source: Source::Faces,
        faces: e
            .faces
            .iter()
            .map(|name| Ref::Face {
                feature: f,
                face: name.clone(),
            })
            .collect(),
        axis: Axis::OriginX,
        ..Default::default()
    })
    .unwrap();
    doc.apply(kmb_core::Transaction::new(
        "2面",
        vec![kmb_core::Op::SetFeatureData { id: r, data }],
    ))
    .unwrap();
    let expected = std::f64::consts::PI * 16.0 * 15.0;
    assert!((mass(&mut doc, &*k, r) - expected).abs() / expected < 2e-3);
}

#[test]
fn the_revolve_action_declares_faces_and_uses_the_same_axis_gesture() {
    let k: std::sync::Arc<dyn Kernel> = std::sync::Arc::new(kmb_kernel_smlib::SmlibKernel::new());
    let (b, f) = disk(false);
    let mut doc = b.finish("円板");
    let reg = registry();
    let report = doc.recompute(&reg, k.as_ref());
    let args = json!({"faces":face(f, "start"),"axis":"x", "angle":360.0});
    let cx = Context {
        args: &args,
        ..Context::new(doc.data(), &report, &reg, &k)
    };
    let action = RevolveCommands
        .actions(&cx)
        .into_iter()
        .find(|a| a.id == kmb_cad_revolve::REVOLVE_SKETCH)
        .unwrap();
    assert!(action.args.iter().any(|a| a.key == "faces"));
    let handles = RevolveCommands.handles(&action.id, &cx).unwrap();
    assert_eq!(
        handles.iter().map(|h| h.key.as_str()).collect::<Vec<_>>(),
        vec!["axis", "angle"]
    );
    let Some(Ok(kmb_core::Outcome::Change { edits, .. })) = RevolveCommands.run(&action.id, &cx)
    else {
        panic!();
    };
    let kmb_core::Edit::Insert { data, .. } = &edits[0] else {
        panic!();
    };
    let r: Revolve = serde_json::from_value(data.clone()).unwrap();
    assert_eq!(r.source, Source::Faces);
    assert_eq!(r.profile, None);
}

#[test]
fn face_revolve_inputs_keep_the_existing_sketch_geometry() {
    let k = TestKernel::new();
    for hole in [false, true] {
        let (mut b, f) = disk(hole);
        let r = b.feature(
            REVOLVE,
            "面を回す",
            Revolve {
                source: Source::Faces,
                faces: vec![face(f, "start")],
                axis: Axis::OriginX,
                ..Default::default()
            },
        );
        let mut doc = b.finish("輪郭と面の比較");
        let face_volume = mass(&mut doc, &*k, r);
        let report = doc.recompute(&registry(), &*k);
        let sketch = doc
            .data()
            .features
            .iter()
            .find(|entry| entry.type_id == kmb_sketch::SKETCH)
            .unwrap();
        let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(sketch.id) else {
            panic!();
        };
        let profile = kmb_sketch::profile_value(&outputs["profile"]).unwrap();
        let shape = k
            .revolve(
                &profile,
                [0.0; 3],
                [1.0, 0.0, 0.0],
                std::f64::consts::TAU,
                "control",
            )
            .unwrap();
        let sketch_volume = k.mass(shape).unwrap().volume;
        let expected = 2.0 * std::f64::consts::PI.powi(2) * 5.0 * if hole { 3.0 } else { 4.0 };
        eprintln!(
            "circle hole={hole}, face={face_volume}, sketch={sketch_volume}, expected={expected}"
        );
        assert_face_profile_matches_sketch(&report, &*k, f, r, &profile);
    }
    let mut b = DocBuilder::new();
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let o = s.fixed_point(0.0, 0.0);
    s.rectangle(o, "10 mm", "4 mm", [0.0, 0.0, 10.0, 4.0]);
    let sk = b.feature(kmb_sketch::SKETCH, "長方形", s.build());
    let f = b.feature(
        EXTRUDE,
        "板",
        Extrude {
            profile: Some(sk),
            distance: "3 mm".into(),
            ..Default::default()
        },
    );
    let r = b.feature(
        REVOLVE,
        "面を回す",
        Revolve {
            source: Source::Faces,
            faces: vec![face(f, "start")],
            axis: Axis::OriginX,
            ..Default::default()
        },
    );
    let mut doc = b.finish("長方形と面の比較");
    let face_volume = mass(&mut doc, &*k, r);
    let report = doc.recompute(&registry(), &*k);
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(sk) else {
        panic!();
    };
    let profile = kmb_sketch::profile_value(&outputs["profile"]).unwrap();
    let shape = k
        .revolve(
            &profile,
            [0.0; 3],
            [1.0, 0.0, 0.0],
            std::f64::consts::TAU,
            "control",
        )
        .unwrap();
    let sketch_volume = k.mass(shape).unwrap().volume;
    let expected = std::f64::consts::PI * 16.0 * 10.0;
    eprintln!("rectangle face={face_volume}, sketch={sketch_volume}, expected={expected}");
    assert_face_profile_matches_sketch(&report, &*k, f, r, &profile);
}

fn assert_face_profile_matches_sketch(
    report: &kmb_core::RecomputeReport,
    k: &dyn Kernel,
    f: FeatureId,
    r: FeatureId,
    sketch: &kmb_core::Profile,
) {
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(f) else {
        panic!();
    };
    let Some(Value::Shape(h)) = outputs.get("body") else {
        panic!();
    };
    let face = kmb_cad::face_profile(k, *h, &[format!("f{}:start", f.0)]).unwrap();
    eprintln!("面の輪郭: approximated={}", face.approximated);
    if face.approximated {
        let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(r) else {
            panic!();
        };
        assert!(
            kmb_core::warnings_of(outputs)
                .iter()
                .any(|warning| warning.contains("細かい折れ線で近い形にして回しました"))
        );
    }
    assert_eq!(face.profile.regions.len(), sketch.regions.len());
    for (a, b) in face.profile.regions.iter().zip(&sketch.regions) {
        assert_eq!(a.holes.len(), b.holes.len());
        for (a, b) in std::iter::once(&a.outer)
            .chain(&a.holes)
            .zip(std::iter::once(&b.outer).chain(&b.holes))
        {
            let mut directions = vec![(a, b, face.profile.plane, sketch.plane)];
            if !face.approximated {
                directions.push((b, a, sketch.plane, face.profile.plane));
            }
            for (src, dst, src_plane, dst_plane) in directions {
                // 網で近似した輪は頂点が元の境界に載り、厳密な輪は区間全体が一致する。
                let points: Vec<_> = if face.approximated {
                    src.edges
                        .iter()
                        .flat_map(|e| [e.segment.start_point(), e.segment.end_point()])
                        .collect()
                } else {
                    src.edges.iter().flat_map(|e| e.polyline(128)).collect()
                };
                for p in points {
                    let world = src_plane.to_world(p);
                    let q = dst_plane.to_local(world);
                    let world_again = dst_plane.to_world(q);
                    for (x, y) in world.into_iter().zip(world_again) {
                        assert!((x - y).abs() < 1e-8);
                    }
                    let distance = dst
                        .edges
                        .iter()
                        .map(|e| e.segment.distance(q))
                        .fold(f64::INFINITY, f64::min);
                    assert!(
                        distance < 1e-6,
                        "輪郭の差 {distance}: {world:?}; {face:?}; {sketch:?}"
                    );
                }
            }
        }
    }
}

/// 確かめること: 明示したスケッチをCLI・MCPの共通操作で使い、画面の面選択を混ぜない。
#[test]
fn an_explicit_revolve_profile_ignores_the_screen_face_selection() {
    let k: std::sync::Arc<dyn Kernel> = std::sync::Arc::new(kmb_kernel_smlib::SmlibKernel::new());
    let (b, f) = disk(false);
    let mut doc = b.finish("明示した輪郭");
    let sk = doc
        .data()
        .features
        .iter()
        .find(|f| f.type_id == kmb_sketch::SKETCH)
        .unwrap()
        .id;
    let reg = registry();
    let report = doc.recompute(&reg, k.as_ref());
    let picked = kmb_core::FacePick {
        feature: f,
        face: format!("f{}:start", f.0),
    };
    let args = json!({"profile":sk.0,"axis":"x"});
    let cx = Context {
        args: &args,
        picked: Some(&picked),
        ..Context::new(doc.data(), &report, &reg, &k)
    };
    let Some(Ok(kmb_core::Outcome::Change { edits, .. })) =
        RevolveCommands.run(kmb_cad_revolve::REVOLVE_SKETCH, &cx)
    else {
        panic!("回転を作る")
    };
    let kmb_core::Edit::Insert { data, .. } = &edits[0] else {
        panic!("回転を足す")
    };
    let r: Revolve = serde_json::from_value(data.clone()).unwrap();
    assert_eq!(r.source, Source::Sketch);
    assert_eq!(r.profile, Some(sk));
    assert!(r.faces.is_empty());
}

/// 確かめること: 面と平行でも平面からずれた軸は、理由つきで止まる。
#[test]
fn revolving_a_face_rejects_a_parallel_axis_offset_from_its_plane() {
    let k = TestKernel::new();
    let (mut b, f) = disk(false);
    let r = b.feature(
        REVOLVE,
        "面を回す",
        Revolve {
            source: Source::Faces,
            faces: vec![face(f, "end")],
            axis: Axis::OriginX,
            ..Default::default()
        },
    );
    let mut doc = b.finish("ずれた平行な軸");
    let report = doc.recompute(&registry(), &*k);
    let status = format!("{:?}", report.feature(r));
    assert!(status.contains("輪郭の平面の上にありません"), "{status}");
}

/// 確かめること: 軸の両側にまたがる面は、自己交差した形を作らず止まる。
#[test]
fn revolving_a_face_rejects_a_profile_crossing_the_axis() {
    let k = TestKernel::new();
    let (mut b, f) = disk(false);
    let r = b.feature(
        REVOLVE,
        "面を回す",
        Revolve {
            source: Source::Faces,
            faces: vec![face(f, "start")],
            axis: Axis::OriginY,
            ..Default::default()
        },
    );
    let mut doc = b.finish("軸をまたぐ面");
    let report = doc.recompute(&registry(), &*k);
    let status = format!("{:?}", report.feature(r));
    assert!(status.contains("輪郭が回転軸をまたいでいます"), "{status}");
}
