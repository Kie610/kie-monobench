//! 3D スケッチ(kmb-sketch-3d)の線を回転・円形のパターンの軸に使う: 線のまわりに回る、軸を選ぶ手順の
//! 候補に出る、線が無くなったら別の線に替えず止まる。

use super::tests::{TestKernel, assert_ok, close, rect_sketch};
use super::*;
use kmb_core::{Context, Document, FeatureId, FeatureStatus, Kernel, Registry};
use kmb_samples::sample::DocBuilder;
use kmb_sketch::SketchPlane;
use serde_json::json;

fn registry() -> Registry {
    let mut r = super::tests::registry();
    kmb_sketch_3d::register(&mut r);
    r
}

/// 点 `a` から `b` への線 1 本の 3D スケッチ(線の番号は 3)。
fn line3d(b: &mut DocBuilder, a: [f64; 3], to: [f64; 3]) -> FeatureId {
    b.feature(
        kmb_sketch_3d::SKETCH3D,
        "3D スケッチ",
        json!({"entities": [
            {"type": "point", "id": 1, "p": a},
            {"type": "point", "id": 2, "p": to},
            {"type": "line", "id": 3, "a": 1, "b": 2},
        ]}),
    )
}

fn mass(doc: &mut Document, k: &TestKernel) -> kmb_core::MassProps {
    let reg = registry();
    let report = doc.recompute(&reg, &**k);
    assert_ok(&report);
    let bodies = visible_bodies(doc.data(), &reg, &report);
    assert_eq!(bodies.len(), 1, "{bodies:?}");
    k.mass(bodies[0].1).unwrap()
}

#[test]
fn a_3d_sketch_line_turns_a_revolve_and_a_circular_pattern() {
    let k = TestKernel::new();
    // XY 平面の長方形(x 10〜15、y 0〜8)を、Y 軸に重なる 3D スケッチの線で一周回す
    let mut b = DocBuilder::new();
    let s = rect_sketch(&mut b, SketchPlane::Xy, 10.0, 0.0, 5.0, 8.0);
    let l = line3d(&mut b, [0.0; 3], [0.0, 10.0, 0.0]);
    b.feature(
        REVOLVE,
        "輪",
        json!({"profile": s.0, "axis": {"kind": "segment", "sketch": l.0, "line": 3}}),
    );
    let m = mass(&mut b.finish("3D スケッチの線で回す"), &k);
    let want = std::f64::consts::PI * (15.0f64.powi(2) - 10.0f64.powi(2)) * 8.0;
    assert!(close(m.volume, want, 1e-3), "{} {want}", m.volume);
    // 箱(x 10〜14)を、Z に立てた 3D スケッチの線のまわりに 4 つ並べる
    let mut b = DocBuilder::new();
    let s = rect_sketch(&mut b, SketchPlane::Xy, 10.0, -2.0, 4.0, 4.0);
    let bl = b.feature(
        EXTRUDE,
        "箱",
        Extrude {
            profile: Some(s),
            distance: "3 mm".into(),
            ..Default::default()
        },
    );
    let l = line3d(&mut b, [0.0; 3], [0.0, 0.0, 5.0]);
    b.feature(
        CIRCULAR_PATTERN,
        "輪",
        json!({"kind": "bodies", "bodies": [bl.0], "axis_ref": {"kind": "segment", "sketch": l.0, "line": 3}, "count": "4"}),
    );
    let mut doc = b.finish("3D スケッチの線で並べる");
    let m = mass(&mut doc, &k);
    assert!(close(m.volume, 4.0 * 48.0, 1e-6), "{}", m.volume);
    assert!(close(m.min[0], -14.0, 1e-6), "{:?}", m.min);
}

#[test]
fn a_3d_sketch_line_is_offered_as_an_axis_and_a_lost_line_stops() {
    let k: std::sync::Arc<dyn Kernel> = std::sync::Arc::new(kmb_kernel_smlib::SmlibKernel::new());
    let mut b = DocBuilder::new();
    let s = rect_sketch(&mut b, SketchPlane::Xy, 10.0, -2.0, 4.0, 4.0);
    let bl = b.feature(
        EXTRUDE,
        "箱",
        Extrude {
            profile: Some(s),
            distance: "3 mm".into(),
            ..Default::default()
        },
    );
    let l = line3d(&mut b, [0.0; 3], [0.0, 0.0, 5.0]);
    let p = b.feature(
        CIRCULAR_PATTERN,
        "輪",
        json!({"kind": "bodies", "bodies": [bl.0], "axis_ref": {"kind": "segment", "sketch": l.0, "line": 9}, "count": "4"}),
    );
    let mut doc = b.finish("無い線");
    let reg = registry();
    let report = doc.recompute(&reg, &*k);
    // 線 9 は無いので、別の線に替えず止まる
    assert!(
        matches!(report.feature(p), Some(FeatureStatus::Error { message }) if message.contains("線 9")),
        "{:?}",
        report.feature(p)
    );
    let cx = Context::new(doc.data(), &report, &reg, &k);
    let want = json!({"sketch": l.0, "segment": 3});
    let shown = kmb_cad::axis::axis_choices(&cx, p, None, None);
    assert!(shown.iter().any(|c| c.value == want), "{shown:?}");
    assert_eq!(
        kmb_cad::AxisRef::from_choice(&want, &cx),
        Ok(kmb_cad::AxisRef::Segment { sketch: l, line: 3 })
    );
    assert!(
        kmb_cad::AxisRef::from_choice(&json!({"sketch": l.0, "segment": 9}), &cx)
            .is_err_and(|e| e.contains("線 9"))
    );
    // 3D スケッチより前の機能の候補には出ない
    let shown = kmb_cad::axis::axis_choices(&cx, l, None, None);
    assert!(shown.iter().all(|c| c.value != want), "{shown:?}");
}
