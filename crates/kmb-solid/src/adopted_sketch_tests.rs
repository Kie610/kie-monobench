//! 他のソフトのスケッチで起きた不具合が、JC では起きないことを確かめる(採用分の点検で「試験だけ」と
//! 判定した物のうち、形のカーネルが要る物)。面に置いたスケッチの投影と、スケッチを変えた後の
//! フィレットの辺、元の形を大きく変えた時に写した縁・外形とオフセットが付いて行くこと。

use super::*;
use crate::tests::{EdgeBench, TestKernel, assert_ok, close, final_mass, rect_sketch, registry};
use kmb_core::{Document, FeatureId, FeatureStatus, Kernel, Op, Value};
use kmb_samples::sample::{DocBuilder, SketchBuilder, reference_part};
use kmb_sketch::{Projection, ProjectionSource, SketchPlane};
use serde_json::json;

/// 文書の最後に機能を足す。
fn push(doc: &mut Document, type_id: &str, data: serde_json::Value) -> FeatureId {
    doc.apply_edits(
        type_id,
        vec![kmb_core::Edit::Insert {
            type_id: type_id.into(),
            name: type_id.into(),
            data,
        }],
    )
    .unwrap()[0]
}

/// 外向きの法線が `n` の平らな面の名前。
fn face(k: &dyn Kernel, h: kmb_core::ShapeHandle, n: [f64; 3]) -> String {
    k.faces(h)
        .unwrap()
        .into_iter()
        .find(|f| {
            f.planar
                .is_some_and(|p| kmb_core::kernel::dot(p.normal, n) > 0.999)
        })
        .unwrap_or_else(|| panic!("法線 {n:?} の面が無い"))
        .name
}

/// 機能 `id` の押し出しの距離を変える。
fn set_distance(doc: &mut Document, id: FeatureId, distance: &str) {
    let mut data = doc.data().feature(id).unwrap().data.clone();
    data["distance"] = json!(distance);
    doc.apply(kmb_core::Transaction::new(
        "距離",
        vec![Op::SetFeatureData { id, data }],
    ))
    .unwrap();
}

/// 10-22 t8: 板の上の面に置き、板の辺を写したスケッチから押し出す。後ろの押し出しの距離を変えても
/// 計算し直すのはその押し出しだけで、スケッチ(と写した辺)は前の結果を使い回す。板の高さを変えた時
/// だけスケッチを計算し直し、写した辺も新しい上の面に付いて行く(他のソフトでは、面のスケッチの後の
/// 機能のたびに本体を作り直して写し直し、大きな部品ほど待たされた)。
#[test]
fn a_sketch_on_a_face_is_not_projected_again_for_later_features() {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    let s = rect_sketch(&mut b, SketchPlane::Xy, 0.0, 0.0, 30.0, 20.0);
    let plate = b.feature(
        EXTRUDE,
        "板",
        Extrude {
            profile: Some(s),
            distance: "10 mm".into(),
            ..Default::default()
        },
    );
    let mut doc = b.finish("面のスケッチ");
    let report = doc.recompute(&registry(), &*k);
    assert_ok(&report);
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(plate) else {
        panic!()
    };
    let Value::Shape(h) = outputs["body"] else {
        panic!()
    };
    let (top, front) = (
        face(&*k, h, [0.0, 0.0, 1.0]),
        face(&*k, h, [0.0, -1.0, 0.0]),
    );

    // 上の面に 10×10 の長方形と、手前の辺を写した線
    let mut sk = SketchBuilder::new(SketchPlane::face(plate, top.clone()));
    let o = sk.fixed_point(0.0, 0.0);
    let c = sk.point_from(o, "10 mm", "5 mm", [10.0, 5.0]);
    sk.rectangle(c, "10 mm", "10 mm", [10.0, 5.0, 10.0, 10.0]);
    let a = sk.point(0.0, 0.0);
    let e = sk.point(30.0, 0.0);
    let edge = sk.line(a, e);
    let mut sketch = sk.build();
    sketch.projections.push(Projection {
        entity: edge,
        source: ProjectionSource::Edge {
            feature: plate,
            faces: [top, front],
        },
    });
    let sketch = push(
        &mut doc,
        kmb_sketch::SKETCH,
        serde_json::to_value(sketch).unwrap(),
    );
    let boss = push(
        &mut doc,
        EXTRUDE,
        serde_json::to_value(Extrude {
            profile: Some(sketch),
            distance: "5 mm".into(),
            operation: Operation::Join,
            target: Some(plate),
            ..Default::default()
        })
        .unwrap(),
    );
    let first = final_mass(&mut doc, &k).volume;
    assert!(
        (first - (30.0 * 20.0 * 10.0 + 10.0 * 10.0 * 5.0)).abs() < 1e-3,
        "{first}"
    );

    // 後ろの押し出しだけを変えると、計算し直すのはそれだけ
    set_distance(&mut doc, boss, "8 mm");
    let report = doc.recompute(&registry(), &*k);
    assert_ok(&report);
    assert_eq!(report.evaluated, 1, "スケッチまで計算し直した: {report:?}");
    let v = final_mass(&mut doc, &k).volume;
    assert!(
        (v - (30.0 * 20.0 * 10.0 + 10.0 * 10.0 * 8.0)).abs() < 1e-3,
        "{v}"
    );

    // 板の高さを変えた時は、スケッチも写した辺も新しい上の面へ
    set_distance(&mut doc, plate, "12 mm");
    let report = doc.recompute(&registry(), &*k);
    assert_ok(&report);
    assert!(report.evaluated >= 3, "{report:?}");
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(sketch) else {
        panic!()
    };
    let p = kmb_sketch::profile_value(&outputs["profile"]).unwrap();
    assert!((p.plane.origin[2] - 12.0).abs() < 1e-9, "{:?}", p.plane);
    let v = final_mass(&mut doc, &k).volume;
    assert!(
        (v - (30.0 * 20.0 * 12.0 + 10.0 * 10.0 * 8.0)).abs() < 1e-3,
        "{v}"
    );
}

/// 10-24 t10: 基準部品の板の縦の角を半径 2 で丸めた後に、板の幅(スケッチの寸法の式)を 60 から 70 に
/// 変えても、フィレットは選んだ辺を見失わず、同じだけ丸める(辺を形の名前で覚えるので、他のソフト
/// のように「選んだ辺が見つからない」にならない)。
#[test]
fn a_fillet_keeps_its_edge_after_the_sketch_is_resized() {
    let p = reference_part().plate.0;
    let corner = json!({"kind": "edge", "feature": 0, "faces": [format!("f{p}:side:e5"), format!("f{p}:side:e6")]});
    let rounded = 2.0 * 2.0 * (1.0 - std::f64::consts::PI / 4.0) * 10.0;
    let mut bench = EdgeBench::reference();
    let before = bench.before;
    let after = bench.after(
        FILLET,
        json!({"rows": [{"edges": [corner], "radius": "2 mm"}]}),
    );
    assert!((before - after - rounded).abs() < 0.5, "{before} - {after}");

    let widen = |doc: &mut Document| {
        let width = doc.data().param_by_name("width").unwrap().id;
        doc.apply(kmb_core::Transaction::new(
            "幅",
            vec![Op::SetParamExpr {
                id: width,
                expr: "70 mm".into(),
            }],
        ))
        .unwrap();
    };
    widen(&mut bench.doc);
    let after = final_mass(&mut bench.doc, &bench.k).volume;
    let mut plain = reference_part().doc;
    widen(&mut plain);
    let before = final_mass(&mut plain, &TestKernel::new()).volume;
    assert!((before - after - rounded).abs() < 0.5, "{before} - {after}");
}

/// 10-31 t5: 板の手前の面に置いたスケッチで、面の縁(4 本の辺)を写して内へ 2 mm オフセットし、
/// その輪郭を手前へ押し出す。板の厚み(押し出しの距離)を 10 から 60、さらに 5 へ大きく変えても、
/// 写した縁は新しい面に付いて行き、オフセットした輪郭も同じ 2 mm の内側に作り直されて、後の押し出しが
/// 失敗しない(他のソフトでは、元の形を大きく変えると写した輪郭が付いて来ず、オフセットが失敗した)。
#[test]
fn a_projected_face_outline_and_its_offset_follow_a_large_thickness_change() {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    let s = rect_sketch(&mut b, SketchPlane::Xy, 0.0, 0.0, 30.0, 20.0);
    let plate = b.feature(
        EXTRUDE,
        "板",
        Extrude {
            profile: Some(s),
            distance: "10 mm".into(),
            ..Default::default()
        },
    );
    let mut doc = b.finish("面の縁のオフセット");
    let report = doc.recompute(&registry(), &*k);
    assert_ok(&report);
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(plate) else {
        panic!()
    };
    let Value::Shape(h) = outputs["body"] else {
        panic!()
    };
    let front = face(&*k, h, [0.0, -1.0, 0.0]);

    // 画面と同じに、新しい面のスケッチは面の左下の角を原点にし、面の縁の辺を写す
    let origin = kmb_sketch::default_origin(&*k, h, &front);
    let (plane, _) = kmb_sketch::face_plane(&*k, h, &front, &origin).unwrap();
    let rim: Vec<_> = kmb_sketch::body_edge_refs(&*k, plate, h, &plane)
        .into_iter()
        .filter(
            |r| matches!(&r.source, ProjectionSource::Edge { faces, .. } if faces.contains(&front)),
        )
        .collect();
    assert_eq!(rim.len(), 4, "{rim:?}");
    let mut sketch = SketchBuilder::new(SketchPlane::Face {
        feature: plate,
        face: front.clone(),
        origin,
    })
    .build();
    let edges: Vec<_> = rim.iter().map(|r| sketch.add_projection(r)).collect();
    let made = sketch.offset(&edges, 2.0, "2 mm", false);
    assert_eq!(made.len(), 4, "{:?}", sketch.entities);
    let sketch = push(
        &mut doc,
        kmb_sketch::SKETCH,
        serde_json::to_value(sketch).unwrap(),
    );
    push(
        &mut doc,
        EXTRUDE,
        serde_json::to_value(Extrude {
            profile: Some(sketch),
            distance: "5 mm".into(),
            operation: Operation::Join,
            target: Some(plate),
            ..Default::default()
        })
        .unwrap(),
    );
    let volume = |t: f64| 30.0 * 20.0 * t + (30.0 - 4.0) * (t - 4.0) * 5.0;
    let v = final_mass(&mut doc, &k).volume;
    assert!((v - volume(10.0)).abs() < 1e-3, "{v}");

    for t in [60.0, 5.0] {
        set_distance(&mut doc, plate, &format!("{t} mm"));
        let report = doc.recompute(&registry(), &*k);
        assert_ok(&report);
        let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(sketch) else {
            panic!("{report:?}")
        };
        let warnings = kmb_core::warnings_of(outputs);
        assert!(warnings.is_empty(), "厚み {t}: {warnings:?}");
        let v = final_mass(&mut doc, &k).volume;
        assert!(
            (v - volume(t)).abs() < 1e-3,
            "厚み {t}: {v} != {}",
            volume(t)
        );
    }
}

/// 10-31 t5 の端から見た面: 立てた円柱(直径 20、中心 x=15)を YZ 平面のスケッチから横に見ると、
/// 外形は辺に当たらない線(側面の外形と、端から見た上下の円の面)になる。その外形を写して内へ 2 mm
/// オフセットし、円柱へ向けて押し出す。円柱の高さを 10 から 60、さらに 6 へ大きく変えても、写した
/// 外形とオフセットが新しい高さに付いて行き、後の押し出しが失敗しない。
#[test]
fn a_projected_silhouette_and_its_offset_follow_a_large_height_change() {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let o = s.fixed_point(0.0, 0.0);
    let c = s.point_from(o, "15 mm", "0 mm", [15.0, 0.0]);
    s.circle(c, "20 mm", 10.0);
    let disc = b.feature(kmb_sketch::SKETCH, "円", s.build());
    let pin = b.feature(
        EXTRUDE,
        "円柱",
        Extrude {
            profile: Some(disc),
            distance: "10 mm".into(),
            ..Default::default()
        },
    );
    let mut doc = b.finish("横から見た外形のオフセット");
    let report = doc.recompute(&registry(), &*k);
    assert_ok(&report);
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(pin) else {
        panic!()
    };
    let Value::Shape(h) = outputs["body"] else {
        panic!()
    };
    let plane = SketchPlane::Yz;
    let p = kmb_core::Plane::YZ;
    let body = kmb_sketch::body_refs(&*k, pin, h, &p);
    let mesh = k.mesh(h, kmb_sketch::BODY_MESH_TOL).unwrap();
    let outline: Vec<_> = kmb_sketch::silhouette_refs(&mesh, &p, pin, &body)
        .into_iter()
        .filter(|r| matches!(r.source, ProjectionSource::Silhouette { .. }))
        .collect();
    assert_eq!(outline.len(), 4, "{outline:?}");
    let mut sketch = SketchBuilder::new(plane).build();
    let lines: Vec<_> = outline.iter().map(|r| sketch.add_projection(r)).collect();
    let made = sketch.offset(&lines, 2.0, "2 mm", false);
    assert_eq!(made.len(), 4, "{:?}", sketch.entities);
    let sketch = push(
        &mut doc,
        kmb_sketch::SKETCH,
        serde_json::to_value(sketch).unwrap(),
    );
    push(
        &mut doc,
        EXTRUDE,
        serde_json::to_value(Extrude {
            profile: Some(sketch),
            distance: "30 mm".into(),
            operation: Operation::Join,
            target: Some(pin),
            ..Default::default()
        })
        .unwrap(),
    );
    // 板(x 0〜30・y −8〜8・z 2〜t−2)と円柱の和。重なりは円の |y| ≤ 8 の部分 × 板の高さ
    let lens = 2.0 * (8.0 * 6.0 + 100.0 * 0.8f64.asin());
    let volume = |t: f64| {
        let cyl = std::f64::consts::PI * 100.0 * t;
        cyl + (30.0 * 16.0 - lens) * (t - 4.0)
    };
    let v = final_mass(&mut doc, &k).volume;
    assert!(close(v, volume(10.0), 1e-2), "{v} != {}", volume(10.0));

    for t in [60.0, 6.0] {
        set_distance(&mut doc, pin, &format!("{t} mm"));
        let report = doc.recompute(&registry(), &*k);
        assert_ok(&report);
        let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(sketch) else {
            panic!("{report:?}")
        };
        let warnings = kmb_core::warnings_of(outputs);
        assert!(warnings.is_empty(), "高さ {t}: {warnings:?}");
        let v = final_mass(&mut doc, &k).volume;
        assert!(close(v, volume(t), 1e-2), "高さ {t}: {v} != {}", volume(t));
    }
}
