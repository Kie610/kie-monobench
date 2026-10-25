//! 作業平面(kmb-cad-datum)とその上のスケッチ: 寸法を変えると付いて来る、元の面が消えたら
//! 別の面に付け替えず止まる、浮かせる距離が負でもスケッチの X の向きが入れ替わらない。

use super::*;
use kmb_cad_datum::{Base, DATUM_PLANE, DatumPlaneFeature, Method, Point};
use kmb_core::{
    Document, FeatureId, FeatureStatus, Kernel, Payload, Plane, Points, Registry, Transaction,
};
use kmb_kernel_smlib::SmlibKernel;
use kmb_samples::sample::{DocBuilder, SketchBuilder};
use kmb_sketch::SketchPlane;

fn registry() -> Registry {
    let mut r = Registry::new();
    kmb_sketch::register(&mut r);
    register(&mut r);
    kmb_cad_datum::register(&mut r);
    r
}

fn extrude(profile: FeatureId, distance: &str, direction: Direction, op: Operation, target: Option<FeatureId>) -> Extrude {
    Extrude {
        profile: Some(profile),
        distance: distance.into(),
        direction,
        operation: op,
        target,
        ..Default::default()
    }
}

/// 左下を固定した長方形。
fn rect(s: &mut SketchBuilder, x: f64, y: f64, w: f64, h: f64) {
    let c = s.fixed_point(x, y);
    s.rectangle(c, &format!("{w} mm"), &format!("{h} mm"), [x, y, w, h]);
}

/// 厚み `t` の板(40×30)と、上面から深さ `d` のポケット(20×10)。
struct Plate {
    b: DocBuilder,
    plate: FeatureId,
    pocket: FeatureId,
}

fn plate() -> Plate {
    let mut b = DocBuilder::new();
    b.param("t", "10 mm");
    b.param("d", "4 mm");
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    rect(&mut s, 0.0, 0.0, 40.0, 30.0);
    let sk = b.feature(kmb_sketch::SKETCH, "板の形", s.build());
    let plate = b.feature(EXTRUDE, "板", extrude(sk, "t", Direction::Normal, Operation::New, None));
    let mut s = SketchBuilder::new(SketchPlane::face(plate, format!("f{}:end", plate.0)));
    rect(&mut s, 10.0, 10.0, 20.0, 10.0);
    let sk = b.feature(kmb_sketch::SKETCH, "ポケットの輪郭", s.build());
    let pocket = b.feature(EXTRUDE, "ポケット", extrude(sk, "d", Direction::Reverse, Operation::Cut, Some(plate)));
    Plate { b, plate, pocket }
}

fn offset(base: Base, distance: &str) -> DatumPlaneFeature {
    DatumPlaneFeature {
        method: Method::Offset,
        base: Some(base),
        distance: distance.into(),
        ..Default::default()
    }
}

/// 作業平面 `datum` の上のスケッチ(平面の (5, 0) に固定の点を 1 つ)。
fn sketch_on(b: &mut DocBuilder, datum: FeatureId) -> FeatureId {
    let mut s = SketchBuilder::new(SketchPlane::Datum { feature: datum });
    s.fixed_point(5.0, 0.0);
    b.feature(kmb_sketch::SKETCH, "作業平面の上", s.build())
}

fn status(doc: &mut Document, k: &dyn Kernel, id: FeatureId) -> FeatureStatus {
    doc.recompute(&registry(), k).feature(id).cloned().expect("機能の結果")
}

fn datum(doc: &mut Document, k: &dyn Kernel, id: FeatureId) -> Plane {
    match status(doc, k, id) {
        FeatureStatus::Ok { outputs, .. } => {
            kmb_interchange::DatumPlane::from_value(&outputs[kmb_interchange::DATUM_OUTPUT])
                .expect("作業平面")
                .plane
        }
        other => panic!("{other:?}"),
    }
}

/// スケッチの点の世界の位置。
fn point(doc: &mut Document, k: &dyn Kernel, id: FeatureId) -> [f64; 3] {
    match status(doc, k, id) {
        FeatureStatus::Ok { outputs, .. } => {
            let pts = Points::from_value(&outputs["points"]).expect("点");
            pts.plane.to_world(*pts.points.values().next().expect("点が 1 つ"))
        }
        other => panic!("{other:?}"),
    }
}

fn set_param(doc: &mut Document, name: &str, expr: &str) {
    let id = doc.data().params.iter().find(|p| p.name == name).expect("パラメータ").id;
    doc.apply(Transaction::new(name, vec![kmb_core::Op::SetParamExpr { id, expr: expr.into() }]))
        .unwrap();
}

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() < 1e-6)
}

#[test]
fn a_sketch_on_a_datum_plane_follows_the_face_it_floats_over() {
    let k = SmlibKernel::new();
    let mut p = plate();
    let top = Base::Face { feature: p.plate, face: format!("f{}:end", p.plate.0) };
    let d = p.b.feature(DATUM_PLANE, "作業平面1", offset(top, "5 mm"));
    let sk = sketch_on(&mut p.b, d);
    let mut doc = p.b.finish("板");
    let at = point(&mut doc, &k, sk);
    assert!((at[2] - 15.0).abs() < 1e-6, "{at:?}");
    // 板を厚くすると、作業平面もその上のスケッチも上がる
    set_param(&mut doc, "t", "12 mm");
    let at2 = point(&mut doc, &k, sk);
    assert!(near(at2, [at[0], at[1], 17.0]), "{at2:?}");
}

#[test]
fn a_datum_plane_stops_instead_of_moving_to_another_face_when_its_face_is_gone() {
    let k = SmlibKernel::new();
    let mut p = plate();
    // ポケットの底の面(上を向いた平らな面のうち、ポケットが作った物)
    let mut probe = plate().b.finish("確かめ");
    let body = match status(&mut probe, &k, p.pocket) {
        FeatureStatus::Ok { outputs, .. } => match outputs["body"] {
            kmb_core::Value::Shape(h) => h,
            _ => panic!("ボディ"),
        },
        other => panic!("{other:?}"),
    };
    let faces = k.faces(body).unwrap();
    let floor = faces
        .iter()
        .find(|f| f.planar.is_some_and(|q| q.normal[2] > 0.9 && (q.point[2] - 6.0).abs() < 1e-6))
        .unwrap_or_else(|| panic!("ポケットの底: {:?}", faces.iter().map(|f| &f.name).collect::<Vec<_>>()))
        .name
        .clone();
    let base = Base::Face { feature: p.pocket, face: floor.clone() };
    let d = p.b.feature(DATUM_PLANE, "作業平面1", offset(base, "1 mm"));
    let sk = sketch_on(&mut p.b, d);
    let mut doc = p.b.finish("板");
    assert!((datum(&mut doc, &k, d).origin[2] - 7.0).abs() < 1e-6);
    // ポケットを貫通させると底の面は無くなる。上面や下面に付け替えず、作業平面を誤りにする
    set_param(&mut doc, "d", "20 mm");
    match status(&mut doc, &k, d) {
        FeatureStatus::Error { message } => assert!(message.contains(&floor), "{message}"),
        other => panic!("止まらずに {other:?}"),
    }
    // その上のスケッチも計算しない(別の平面に描かない)。ポケットとほかの機能は計算を続ける
    assert!(matches!(status(&mut doc, &k, sk), FeatureStatus::Blocked { .. } | FeatureStatus::Error { .. }));
    assert!(matches!(status(&mut doc, &k, p.pocket), FeatureStatus::Ok { .. }));
}

#[test]
fn a_negative_distance_keeps_the_sketch_x_direction() {
    let k = SmlibKernel::new();
    for face in ["end", "start"] {
        let mut p = plate();
        let base = Base::Face { feature: p.plate, face: format!("f{}:{face}", p.plate.0) };
        let up = p.b.feature(DATUM_PLANE, "浮かせる", offset(base.clone(), "3 mm"));
        let down = p.b.feature(DATUM_PLANE, "沈める", offset(base, "-3 mm"));
        let on_up = sketch_on(&mut p.b, up);
        let on_down = sketch_on(&mut p.b, down);
        let mut doc = p.b.finish("板");
        let (a, b) = (datum(&mut doc, &k, up), datum(&mut doc, &k, down));
        assert!(near(a.x_axis, b.x_axis) && near(a.y_axis, b.y_axis), "{face}: {a:?} {b:?}");
        // 原点は法線に沿って 6 mm 離れ、スケッチの (5, 0) の点は同じ向きにずれる
        let n = a.normal();
        let gap = kmb_core::kernel::sub(a.origin, b.origin);
        assert!(near(gap, kmb_core::kernel::scale(n, 6.0)), "{face}: {gap:?}");
        let (pu, pd) = (point(&mut doc, &k, on_up), point(&mut doc, &k, on_down));
        assert!(near(kmb_core::kernel::sub(pu, pd), gap), "{face}: {pu:?} {pd:?}");
    }
}

#[test]
fn the_other_ways_make_the_plane_or_say_why_not() {
    let k = SmlibKernel::new();
    let mut p = plate();
    let face = |n: &str| Base::Face { feature: p.plate, face: format!("f{}:{n}", p.plate.0) };
    let mid = p.b.feature(
        DATUM_PLANE,
        "中間",
        DatumPlaneFeature { method: Method::Midplane, base: Some(face("end")), other: Some(face("start")), ..Default::default() },
    );
    let tilted = p.b.feature(
        DATUM_PLANE,
        "傾ける",
        DatumPlaneFeature {
            method: Method::Angle,
            base: Some(Base::Xy),
            axis: Some(kmb_cad_datum::Axis::X),
            angle: "90 deg".into(),
            ..Default::default()
        },
    );
    let at = |v: [f64; 3]| Point::At { at: v };
    let three = p.b.feature(
        DATUM_PLANE,
        "3 点",
        DatumPlaneFeature { method: Method::ThreePoints, points: vec![at([0.0, 0.0, 1.0]), at([1.0, 0.0, 1.0]), at([0.0, 1.0, 1.0])], ..Default::default() },
    );
    let line = p.b.feature(
        DATUM_PLANE,
        "一直線",
        DatumPlaneFeature { method: Method::ThreePoints, points: vec![at([0.0; 3]), at([1.0, 0.0, 0.0]), at([2.0, 0.0, 0.0])], ..Default::default() },
    );
    let mut doc = p.b.finish("板");
    assert!((datum(&mut doc, &k, mid).origin[2] - 5.0).abs() < 1e-6);
    let t = datum(&mut doc, &k, tilted);
    assert!(near(t.x_axis, [1.0, 0.0, 0.0]) && near(t.normal(), [0.0, -1.0, 0.0]), "{t:?}");
    let p3 = datum(&mut doc, &k, three);
    assert!(near(p3.origin, [0.0, 0.0, 1.0]) && near(p3.normal(), [0.0, 0.0, 1.0]), "{p3:?}");
    assert!(matches!(status(&mut doc, &k, line), FeatureStatus::Error { message } if message.contains("一直線")));
}
