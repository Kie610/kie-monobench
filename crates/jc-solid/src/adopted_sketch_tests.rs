//! 他のソフトのスケッチで起きた不具合が、JC では起きないことを確かめる(採用分の点検で「試験だけ」と
//! 判定した物のうち、形のカーネルが要る物)。面に置いたスケッチの投影と、スケッチを変えた後の
//! フィレットの辺。

use super::*;
use crate::tests::{EdgeBench, TestKernel, assert_ok, final_mass, rect_sketch, registry};
use jc_core::{Document, FeatureId, FeatureStatus, Kernel, Op, Value};
use jc_samples::sample::{DocBuilder, SketchBuilder, reference_part};
use jc_sketch::{Projection, ProjectionSource, SketchPlane};
use serde_json::json;

/// 文書の最後に機能を足す。
fn push(doc: &mut Document, type_id: &str, data: serde_json::Value) -> FeatureId {
    doc.apply_edits(
        type_id,
        vec![jc_core::Edit::Insert {
            type_id: type_id.into(),
            name: type_id.into(),
            data,
        }],
    )
    .unwrap()[0]
}

/// 外向きの法線が `n` の平らな面の名前。
fn face(k: &dyn Kernel, h: jc_core::ShapeHandle, n: [f64; 3]) -> String {
    k.faces(h)
        .unwrap()
        .into_iter()
        .find(|f| {
            f.planar
                .is_some_and(|p| jc_core::kernel::dot(p.normal, n) > 0.999)
        })
        .unwrap_or_else(|| panic!("法線 {n:?} の面が無い"))
        .name
}

/// 機能 `id` の押し出しの距離を変える。
fn set_distance(doc: &mut Document, id: FeatureId, distance: &str) {
    let mut data = doc.data().feature(id).unwrap().data.clone();
    data["distance"] = json!(distance);
    doc.apply(jc_core::Transaction::new(
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
        jc_sketch::SKETCH,
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
    let p = jc_sketch::profile_value(&outputs["profile"]).unwrap();
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
        doc.apply(jc_core::Transaction::new(
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
