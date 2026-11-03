//! 確かめること: 分割の縁と薄い壁の面を名前で指し、寸法を変えても同じ所に付いて来る。

use crate::tests::{TestKernel, assert_ok, registry};
use crate::*;
use kmb_core::{Document, FeatureId, FeatureStatus, Op, ShapeHandle, Transaction, Value};
use kmb_samples::sample::{DocBuilder, SketchBuilder};
use kmb_sketch::{FaceOrigin, SketchPlane};
use serde_json::json;

fn insert(doc: &mut Document, kind: &str, data: impl serde::Serialize) -> FeatureId {
    doc.apply_edits(
        "機能",
        vec![kmb_core::Edit::Insert {
            type_id: kind.into(),
            name: "参照の試験".into(),
            data: serde_json::to_value(data).unwrap(),
        }],
    )
    .unwrap()[0]
}

fn body(report: &kmb_core::RecomputeReport, id: FeatureId) -> ShapeHandle {
    match report.feature(id) {
        Some(FeatureStatus::Ok { outputs, .. }) => match outputs["body"] {
            Value::Shape(h) => h,
            ref other => panic!("ボディがない: {other:?}"),
        },
        other => panic!("機能を作れない: {other:?}"),
    }
}

fn parameter(doc: &mut Document, name: &str, expr: &str) {
    let id = doc.data().param_by_name(name).unwrap().id;
    doc.apply(Transaction::new(
        "寸法",
        vec![Op::SetParamExpr {
            id,
            expr: expr.into(),
        }],
    ))
    .unwrap();
}

#[test]
fn a_split_lid_rim_keeps_its_named_edge_when_the_plate_is_resized() {
    let k = TestKernel::new();
    if k.is_truck() {
        kmb_contract::skip!("truck は平面での分割を作れない");
    }
    let reg = registry();
    for (kind, factor) in [(FILLET, 1.0 - std::f64::consts::PI / 4.0), (CHAMFER, 0.5)] {
        let mut b = DocBuilder::new();
        b.param("width", "40 mm");
        let mut s = SketchBuilder::xy();
        let origin = s.fixed_point(0.0, 0.0);
        s.rectangle(origin, "width", "20 mm", [0.0, 0.0, 40.0, 20.0]);
        let sketch = b.sketch("板の輪郭", s);
        let plate = b.feature(
            EXTRUDE,
            "板",
            Extrude {
                profile: Some(sketch),
                distance: "10 mm".into(),
                ..Default::default()
            },
        );
        let split = b.feature(
            kmb_cad_modify::SPLIT,
            "分割",
            json!({"body": plate.0, "normal": [0,0,1]}),
        );
        let rest = b.feature(kmb_cad_modify::SPLIT_PART, "残り", json!({"of": split.0}));
        let mut doc = b.finish("分割の縁");
        let report = doc.recompute(&reg, &*k);
        assert_ok(&report);
        let edge = k
            .edges(body(&report, split))
            .unwrap()
            .into_iter()
            .find(|e| {
                [e.a, e.b]
                    .iter()
                    .all(|p| p[1].abs() < 1e-6 && (p[2] - 5.0).abs() < 1e-6)
            })
            .expect("分割した蓋の手前の縁");
        let selected = json!({"kind":"edge", "feature":split.0, "faces":edge.faces});
        let data = if kind == FILLET {
            json!({"target":split.0,"method":"blend","rows":[{"edges":[selected],"radius":"1 mm"}]})
        } else {
            json!({"target":split.0,"method":"blend","rows":[{"edges":[selected],"distance":"1 mm"}]})
        };
        let rounded = insert(&mut doc, kind, data);
        for width in [40.0, 60.0] {
            parameter(&mut doc, "width", &format!("{width} mm"));
            let report = doc.recompute(&reg, &*k);
            assert_ok(&report);
            let h = body(&report, rounded);
            let volume = k.volume(h).unwrap();
            let expected = width * 20.0 * 5.0 - width * factor;
            assert!(
                (volume - expected).abs() < 0.05,
                "{kind} 幅{width}: {volume} / {expected}"
            );
            assert!((k.volume(body(&report, rest)).unwrap() - width * 20.0 * 5.0).abs() < 1e-5);
            let found = k
                .edges(body(&report, split))
                .unwrap()
                .into_iter()
                .find(|e| e.faces == edge.faces)
                .expect("同じ面の名前の縁");
            assert!(
                [found.a, found.b]
                    .iter()
                    .all(|p| p[1].abs() < 1e-6 && (p[2] - 5.0).abs() < 1e-6)
            );
            let rounded_edges = k.edges(h).unwrap();
            let long_edge_at = |y: f64, z: f64| {
                rounded_edges.iter().any(|e| {
                    (e.a[0] - e.b[0]).abs() > width - 1e-5
                        && [e.a, e.b]
                            .iter()
                            .all(|p| (p[1] - y).abs() < 1e-5 && (p[2] - z).abs() < 1e-5)
                })
            };
            // 手前の縁だけが半径・距離の分だけずれ、奥の縁は元の位置を保つ。
            assert!(
                !long_edge_at(0.0, 5.0),
                "{kind} 幅{width}: 手前の元の縁が残る"
            );
            let both_ends_at = |y: f64, z: f64| {
                [0.0, width].into_iter().all(|x| {
                    rounded_edges.iter().flat_map(|e| [e.a, e.b]).any(|p| {
                        (p[0] - x).abs() < 1e-5
                            && (p[1] - y).abs() < 1e-5
                            && (p[2] - z).abs() < 1e-5
                    })
                })
            };
            // 接線でつながる縁は角の辺一覧に出ないので、両端の点で位置を確かめる。
            assert!(
                both_ends_at(0.0, 6.0) && both_ends_at(1.0, 5.0),
                "{kind} 幅{width}: 丸めた縁が手前にない: {rounded_edges:?}"
            );
            assert!(
                long_edge_at(20.0, 5.0),
                "{kind} 幅{width}: 奥の縁が移っている"
            );
            let (lo, hi) = k.bounds(h).unwrap();
            assert!(lo[2] > 4.999 && hi[2] < 10.001, "{lo:?} {hi:?}");
        }
    }
}

#[test]
fn a_sketch_on_a_thin_wall_side_tracks_thickness_and_line_length() {
    let k = TestKernel::new();
    let reg = registry();
    for (thin, factor) in [
        (Thin::Center, 0.5),
        (Thin::Inside, 1.0),
        (Thin::Outside, 0.0),
    ] {
        let mut b = DocBuilder::new();
        b.param("length", "40 mm");
        b.param("wall", "2 mm");
        let mut line = SketchBuilder::xy();
        let a = line.fixed_point(0.0, 0.0);
        let c = line.point_from(a, "length", "0 mm", [40.0, 0.0]);
        line.line(a, c);
        let source = b.sketch("線", line);
        let wall = b.feature(
            EXTRUDE,
            "薄い壁",
            Extrude {
                profile: Some(source),
                thin,
                thickness: Some("wall".into()),
                distance: "10 mm".into(),
                ..Default::default()
            },
        );
        let mut doc = b.finish("薄い壁の面のスケッチ");
        let report = doc.recompute(&reg, &*k);
        assert_ok(&report);
        let h = body(&report, wall);
        let face = k
            .faces(h)
            .unwrap()
            .into_iter()
            .find(|f| f.planar.is_some_and(|p| p.normal[1] > 0.999))
            .expect("壁の+Yの側面")
            .name;
        let (plane, _) = kmb_sketch::face_plane(&*k, h, &face, &FaceOrigin::World).unwrap();
        let at = plane.to_local([12.0, factor * 2.0, 4.0]);
        let mut profile = SketchBuilder::new(SketchPlane::face(wall, face.clone()));
        let origin = profile.fixed_point(at[0], at[1]);
        profile.rectangle(origin, "2 mm", "2 mm", [at[0], at[1], 2.0, 2.0]);
        let sketch = insert(&mut doc, kmb_sketch::SKETCH, profile.build());
        let boss = insert(
            &mut doc,
            EXTRUDE,
            Extrude {
                profile: Some(sketch),
                distance: "3 mm".into(),
                operation: Operation::Join,
                target: Some(wall),
                ..Default::default()
            },
        );
        for (length, thickness) in [(40.0, 2.0), (60.0, 4.0)] {
            parameter(&mut doc, "length", &format!("{length} mm"));
            parameter(&mut doc, "wall", &format!("{thickness} mm"));
            let report = doc.recompute(&reg, &*k);
            assert_ok(&report);
            let FeatureStatus::Ok { outputs, .. } = report.feature(sketch).unwrap() else {
                panic!("面のスケッチ")
            };
            let p = kmb_sketch::profile_value(&outputs["profile"])
                .unwrap()
                .plane;
            assert!(
                (p.origin[1] - factor * thickness).abs() < 1e-6,
                "{thin:?}: {p:?}"
            );
            assert!(p.normal()[1] > 0.999);
            let shape = body(&report, boss);
            let volume = k.volume(shape).unwrap();
            assert!(
                (volume - (length * thickness * 10.0 + 12.0)).abs() < 0.01,
                "{thin:?} {length}/{thickness}: {volume}"
            );
            let mesh = k.mesh(shape, 0.01).unwrap();
            let tip = mesh
                .positions
                .iter()
                .map(|p| f64::from(p[1]))
                .reduce(f64::max)
                .expect("薄い壁と出っ張りの網");
            // 外形の箱には余白があるので、形の端は網の頂点で確かめる。
            assert!(
                (tip - (factor * thickness + 3.0)).abs() < 1e-4,
                "{thin:?} 長さ{length}/厚さ{thickness}: 形の端{tip} 期待{}",
                factor * thickness + 3.0
            );
            let (lo, hi) = k.bounds(shape).unwrap();
            assert!(lo[1] <= tip && tip <= hi[1], "{lo:?} {tip} {hi:?}");
            let data = &doc.data().feature(sketch).unwrap().data;
            let stored: kmb_sketch::Sketch = serde_json::from_value(data.clone()).unwrap();
            assert_eq!(stored.plane, SketchPlane::face(wall, face.clone()));
        }
    }
}
