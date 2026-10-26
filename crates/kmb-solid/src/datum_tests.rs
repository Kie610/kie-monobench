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

fn extrude(
    profile: FeatureId,
    distance: &str,
    direction: Direction,
    op: Operation,
    target: Option<FeatureId>,
) -> Extrude {
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
    let plate = b.feature(
        EXTRUDE,
        "板",
        extrude(sk, "t", Direction::Normal, Operation::New, None),
    );
    let mut s = SketchBuilder::new(SketchPlane::face(plate, format!("f{}:end", plate.0)));
    rect(&mut s, 10.0, 10.0, 20.0, 10.0);
    let sk = b.feature(kmb_sketch::SKETCH, "ポケットの輪郭", s.build());
    let pocket = b.feature(
        EXTRUDE,
        "ポケット",
        extrude(sk, "d", Direction::Reverse, Operation::Cut, Some(plate)),
    );
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
    doc.recompute(&registry(), k)
        .feature(id)
        .cloned()
        .expect("機能の結果")
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
            pts.plane
                .to_world(*pts.points.values().next().expect("点が 1 つ"))
        }
        other => panic!("{other:?}"),
    }
}

fn set_param(doc: &mut Document, name: &str, expr: &str) {
    let id = doc
        .data()
        .params
        .iter()
        .find(|p| p.name == name)
        .expect("パラメータ")
        .id;
    doc.apply(Transaction::new(
        name,
        vec![kmb_core::Op::SetParamExpr {
            id,
            expr: expr.into(),
        }],
    ))
    .unwrap();
}

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() < 1e-6)
}

#[test]
fn a_sketch_on_a_datum_plane_follows_the_face_it_floats_over() {
    let k = SmlibKernel::new();
    let mut p = plate();
    let top = Base::Face {
        feature: p.plate,
        face: format!("f{}:end", p.plate.0),
    };
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
        .find(|f| {
            f.planar
                .is_some_and(|q| q.normal[2] > 0.9 && (q.point[2] - 6.0).abs() < 1e-6)
        })
        .unwrap_or_else(|| {
            panic!(
                "ポケットの底: {:?}",
                faces.iter().map(|f| &f.name).collect::<Vec<_>>()
            )
        })
        .name
        .clone();
    let base = Base::Face {
        feature: p.pocket,
        face: floor.clone(),
    };
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
    assert!(matches!(
        status(&mut doc, &k, sk),
        FeatureStatus::Blocked { .. } | FeatureStatus::Error { .. }
    ));
    assert!(matches!(
        status(&mut doc, &k, p.pocket),
        FeatureStatus::Ok { .. }
    ));
}

#[test]
fn a_negative_distance_keeps_the_sketch_x_direction() {
    let k = SmlibKernel::new();
    for face in ["end", "start"] {
        let mut p = plate();
        let base = Base::Face {
            feature: p.plate,
            face: format!("f{}:{face}", p.plate.0),
        };
        let up =
            p.b.feature(DATUM_PLANE, "浮かせる", offset(base.clone(), "3 mm"));
        let down = p.b.feature(DATUM_PLANE, "沈める", offset(base, "-3 mm"));
        let on_up = sketch_on(&mut p.b, up);
        let on_down = sketch_on(&mut p.b, down);
        let mut doc = p.b.finish("板");
        let (a, b) = (datum(&mut doc, &k, up), datum(&mut doc, &k, down));
        assert!(
            near(a.x_axis, b.x_axis) && near(a.y_axis, b.y_axis),
            "{face}: {a:?} {b:?}"
        );
        // 原点は法線に沿って 6 mm 離れ、スケッチの (5, 0) の点は同じ向きにずれる
        let n = a.normal();
        let gap = kmb_core::kernel::sub(a.origin, b.origin);
        assert!(
            near(gap, kmb_core::kernel::scale(n, 6.0)),
            "{face}: {gap:?}"
        );
        let (pu, pd) = (point(&mut doc, &k, on_up), point(&mut doc, &k, on_down));
        assert!(
            near(kmb_core::kernel::sub(pu, pd), gap),
            "{face}: {pu:?} {pd:?}"
        );
    }
}

#[test]
fn the_other_ways_make_the_plane_or_say_why_not() {
    let k = SmlibKernel::new();
    let mut p = plate();
    let face = |n: &str| Base::Face {
        feature: p.plate,
        face: format!("f{}:{n}", p.plate.0),
    };
    let mid = p.b.feature(
        DATUM_PLANE,
        "中間",
        DatumPlaneFeature {
            method: Method::Midplane,
            base: Some(face("end")),
            other: Some(face("start")),
            ..Default::default()
        },
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
        DatumPlaneFeature {
            method: Method::ThreePoints,
            points: vec![
                at([0.0, 0.0, 1.0]),
                at([1.0, 0.0, 1.0]),
                at([0.0, 1.0, 1.0]),
            ],
            ..Default::default()
        },
    );
    let line = p.b.feature(
        DATUM_PLANE,
        "一直線",
        DatumPlaneFeature {
            method: Method::ThreePoints,
            points: vec![at([0.0; 3]), at([1.0, 0.0, 0.0]), at([2.0, 0.0, 0.0])],
            ..Default::default()
        },
    );
    let mut doc = p.b.finish("板");
    assert!((datum(&mut doc, &k, mid).origin[2] - 5.0).abs() < 1e-6);
    let t = datum(&mut doc, &k, tilted);
    assert!(
        near(t.x_axis, [1.0, 0.0, 0.0]) && near(t.normal(), [0.0, -1.0, 0.0]),
        "{t:?}"
    );
    let p3 = datum(&mut doc, &k, three);
    assert!(
        near(p3.origin, [0.0, 0.0, 1.0]) && near(p3.normal(), [0.0, 0.0, 1.0]),
        "{p3:?}"
    );
    assert!(
        matches!(status(&mut doc, &k, line), FeatureStatus::Error { message } if message.contains("一直線"))
    );
}

fn output_of<T: Payload>(doc: &mut Document, k: &dyn Kernel, id: FeatureId) -> T {
    match status(doc, k, id) {
        FeatureStatus::Ok { outputs, .. } => {
            T::from_value(&outputs[kmb_interchange::DATUM_OUTPUT]).expect("作業軸か作業点")
        }
        other => panic!("{other:?}"),
    }
}

fn body_handle(doc: &mut Document, k: &dyn Kernel, id: FeatureId) -> kmb_core::ShapeHandle {
    match status(doc, k, id) {
        FeatureStatus::Ok { outputs, .. } => match outputs["body"] {
            kmb_core::Value::Shape(h) => h,
            _ => panic!("ボディ"),
        },
        other => panic!("{other:?}"),
    }
}

/// 板の上面と、ある側面の境目の辺(両側の面の名前)。
fn top_edge(doc: &mut Document, k: &dyn Kernel, plate: FeatureId) -> [String; 2] {
    let body = body_handle(doc, k, plate);
    let top = format!("f{}:end", plate.0);
    k.edges(body)
        .unwrap()
        .into_iter()
        .find(|e| e.faces.contains(&top))
        .expect("上面の辺")
        .faces
}

#[test]
fn a_datum_axis_on_an_edge_follows_the_size_and_turns_a_body() {
    use kmb_cad_datum::{AxisMethod, DATUM_AXIS, DatumAxisFeature, axis::EdgeRef};
    let k = SmlibKernel::new();
    let mut p = plate();
    let mut probe = plate().b.finish("確かめ");
    let faces = top_edge(&mut probe, &k, p.plate);
    let edge = p.b.feature(
        DATUM_AXIS,
        "作業軸1",
        DatumAxisFeature {
            method: AxisMethod::Edge,
            edge: Some(EdgeRef {
                feature: p.plate,
                faces,
            }),
            ..Default::default()
        },
    );
    // 原点の YZ 平面と ZX 平面の交わりは Z 軸。それで板を 90° 回す
    let z = p.b.feature(
        DATUM_AXIS,
        "作業軸2",
        DatumAxisFeature {
            method: AxisMethod::TwoPlanes,
            base: Some(Base::Yz),
            other: Some(Base::Zx),
            ..Default::default()
        },
    );
    let parallel = p.b.feature(
        DATUM_AXIS,
        "平行",
        DatumAxisFeature {
            method: AxisMethod::TwoPlanes,
            base: Some(Base::Xy),
            other: Some(Base::Xy),
            ..Default::default()
        },
    );
    let turn = p.b.feature(
        kmb_cad_modify::MOVE,
        "回す",
        serde_json::json!({"body": p.pocket, "mode": "rotate", "axis_ref": {"kind": "datum", "feature": z.0}, "angle": "90 deg"}),
    );
    let mut doc = p.b.finish("板");
    let a: kmb_interchange::DatumAxis = output_of(&mut doc, &k, edge);
    assert!((a.at[2] - 10.0).abs() < 1e-6, "{a:?}");
    let zaxis: kmb_interchange::DatumAxis = output_of(&mut doc, &k, z);
    assert!(
        near(zaxis.at, [0.0; 3]) && (zaxis.dir[2].abs() - 1.0).abs() < 1e-9,
        "{zaxis:?}"
    );
    assert!(
        matches!(status(&mut doc, &k, parallel), FeatureStatus::Error { message } if message.contains("平行"))
    );
    // 板(x 0〜40、y 0〜30)を Z のまわりに 90° 回すと x -30〜0、y 0〜40(向きは軸の向きの右ねじ)
    let (lo, hi) = k.bounds(body_handle(&mut doc, &k, turn)).unwrap();
    let s = zaxis.dir[2].signum();
    let (want_lo, want_hi) = if s > 0.0 {
        ([-30.0, 0.0], [0.0, 40.0])
    } else {
        ([0.0, -40.0], [30.0, 0.0])
    };
    assert!(
        (0..2).all(|i| (lo[i] - want_lo[i]).abs() < 1e-3 && (hi[i] - want_hi[i]).abs() < 1e-3),
        "{lo:?} {hi:?}"
    );
    // 板を厚くすると、辺の作業軸も上がる
    set_param(&mut doc, "t", "12 mm");
    let a2: kmb_interchange::DatumAxis = output_of(&mut doc, &k, edge);
    assert!((a2.at[2] - 12.0).abs() < 1e-6, "{a2:?}");
}

#[test]
fn datum_points_make_axes_and_planes_and_follow_the_corner() {
    use kmb_cad_datum::{AxisMethod, DATUM_AXIS, DATUM_POINT, DatumAxisFeature, DatumPointFeature};
    let k = SmlibKernel::new();
    let mut p = plate();
    // 上面の角(z = t)を 1 つ取る
    let mut probe = plate().b.finish("確かめ");
    let body = body_handle(&mut probe, &k, p.plate);
    let corner = kmb_core::vertices(&k, body)
        .into_iter()
        .find(|v| (v.at[2] - 10.0).abs() < 1e-6 && v.at[0].abs() < 1e-6 && v.at[1].abs() < 1e-6)
        .expect("上面の角 (0, 0, 10)");
    let on_corner = p.b.feature(
        DATUM_POINT,
        "作業点1",
        DatumPointFeature {
            point: Point::Vertex {
                feature: p.plate,
                vertex: corner.name.clone(),
            },
        },
    );
    let at = p.b.feature(
        DATUM_POINT,
        "作業点2",
        DatumPointFeature {
            point: Point::At {
                at: [10.0, 0.0, 0.0],
            },
        },
    );
    let third = p.b.feature(
        DATUM_POINT,
        "作業点3",
        DatumPointFeature {
            point: Point::At {
                at: [0.0, 10.0, 0.0],
            },
        },
    );
    let axis = p.b.feature(
        DATUM_AXIS,
        "2 点",
        DatumAxisFeature {
            method: AxisMethod::TwoPoints,
            points: vec![
                Point::Datum { feature: at },
                Point::Datum { feature: on_corner },
            ],
            ..Default::default()
        },
    );
    let same = p.b.feature(
        DATUM_AXIS,
        "重なる 2 点",
        DatumAxisFeature {
            method: AxisMethod::TwoPoints,
            points: vec![Point::Datum { feature: at }, Point::Datum { feature: at }],
            ..Default::default()
        },
    );
    let plane = p.b.feature(
        DATUM_PLANE,
        "3 点",
        DatumPlaneFeature {
            method: Method::ThreePoints,
            points: vec![
                Point::Datum { feature: on_corner },
                Point::Datum { feature: at },
                Point::Datum { feature: third },
            ],
            ..Default::default()
        },
    );
    let mut doc = p.b.finish("板");
    let c: kmb_interchange::DatumPoint = output_of(&mut doc, &k, on_corner);
    assert!(near(c.at, [0.0, 0.0, 10.0]), "{c:?}");
    let a: kmb_interchange::DatumAxis = output_of(&mut doc, &k, axis);
    assert!(near(a.at, [10.0, 0.0, 0.0]), "{a:?}");
    let len = 200.0f64.sqrt();
    assert!(near(a.dir, [-10.0 / len, 0.0, 10.0 / len]), "{a:?}");
    assert!(
        matches!(status(&mut doc, &k, same), FeatureStatus::Error { message } if message.contains("重なって"))
    );
    assert!(near(datum(&mut doc, &k, plane).origin, [0.0, 0.0, 10.0]));
    // 板を厚くすると、角の作業点と、それを使う作業軸・作業平面が付いて来る
    set_param(&mut doc, "t", "12 mm");
    let c2: kmb_interchange::DatumPoint = output_of(&mut doc, &k, on_corner);
    assert!(near(c2.at, [0.0, 0.0, 12.0]), "{c2:?}");
    assert!(near(datum(&mut doc, &k, plane).origin, [0.0, 0.0, 12.0]));
}

#[test]
fn a_cylinder_gives_its_axis_and_its_circle_gives_a_center() {
    use kmb_cad_datum::{
        AxisMethod, DATUM_AXIS, DATUM_POINT, DatumAxisFeature, DatumPointFeature, axis::FaceRef,
    };
    let k = SmlibKernel::new();
    let mut b = DocBuilder::new();
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let c = s.fixed_point(5.0, 7.0);
    s.circle(c, "6 mm", 3.0);
    let sk = b.feature(kmb_sketch::SKETCH, "円", s.build());
    let rod = b.feature(
        EXTRUDE,
        "棒",
        extrude(sk, "20 mm", Direction::Normal, Operation::New, None),
    );
    let mut probe = {
        let mut b2 = DocBuilder::new();
        let mut s = SketchBuilder::new(SketchPlane::Xy);
        let c = s.fixed_point(5.0, 7.0);
        s.circle(c, "6 mm", 3.0);
        let sk = b2.feature(kmb_sketch::SKETCH, "円", s.build());
        b2.feature(
            EXTRUDE,
            "棒",
            extrude(sk, "20 mm", Direction::Normal, Operation::New, None),
        );
        b2.finish("確かめ")
    };
    let body = body_handle(&mut probe, &k, rod);
    let arc = k
        .arc_edges(body)
        .unwrap()
        .into_iter()
        .next()
        .expect("円の辺");
    let axis = b.feature(
        DATUM_AXIS,
        "円筒の軸",
        DatumAxisFeature {
            method: AxisMethod::Cylinder,
            face: Some(FaceRef {
                feature: rod,
                face: arc.faces[1].clone(),
            }),
            ..Default::default()
        },
    );
    let center = b.feature(
        DATUM_POINT,
        "中心",
        DatumPointFeature {
            point: Point::Center {
                feature: rod,
                faces: arc.faces.clone(),
            },
        },
    );
    let mut doc = b.finish("棒");
    let a: kmb_interchange::DatumAxis = output_of(&mut doc, &k, axis);
    assert!(
        (a.at[0] - 5.0).abs() < 1e-6
            && (a.at[1] - 7.0).abs() < 1e-6
            && (a.dir[2].abs() - 1.0).abs() < 1e-9,
        "{a:?}"
    );
    let p: kmb_interchange::DatumPoint = output_of(&mut doc, &k, center);
    assert!(
        (p.at[0] - 5.0).abs() < 1e-6 && (p.at[1] - 7.0).abs() < 1e-6,
        "{p:?}"
    );
}

#[test]
fn a_datum_plane_mirrors_and_splits_a_body_and_follows_its_distance() {
    let k = SmlibKernel::new();
    let mut p = plate();
    p.b.param("mirror_at", "50 mm");
    p.b.param("cut_at", "15 mm");
    let mp =
        p.b.feature(DATUM_PLANE, "鏡の平面", offset(Base::Yz, "mirror_at"));
    let cp =
        p.b.feature(DATUM_PLANE, "割る平面", offset(Base::Yz, "cut_at"));
    let mirror = p.b.feature(
        MIRROR,
        "写し",
        serde_json::json!({"kind": "bodies", "bodies": [p.pocket.0], "plane_ref": {"kind": "datum", "feature": mp.0}}),
    );
    let split = p.b.feature(
        kmb_cad_modify::SPLIT,
        "割る",
        serde_json::json!({"body": p.pocket.0, "plane_ref": {"kind": "datum", "feature": cp.0}}),
    );
    let mut doc = p.b.finish("板");
    // 板(x 0〜40)を x = 50 の作業平面で写すと、写しは x 60〜100
    let (lo, hi) = k.bounds(body_handle(&mut doc, &k, mirror)).unwrap();
    assert!(
        (lo[0] - 0.0).abs() < 1e-3 && (hi[0] - 100.0).abs() < 1e-3,
        "{lo:?} {hi:?}"
    );
    set_param(&mut doc, "mirror_at", "60 mm");
    let (_, hi) = k.bounds(body_handle(&mut doc, &k, mirror)).unwrap();
    assert!((hi[0] - 120.0).abs() < 1e-3, "{hi:?}");

    // 板を x = 15 の作業平面で割る。法線の側(x 15〜40)が元のボディの名前を受け継ぐ
    let (lo, hi) = k.bounds(body_handle(&mut doc, &k, split)).unwrap();
    assert!(
        (lo[0] - 15.0).abs() < 1e-3 && (hi[0] - 40.0).abs() < 1e-3,
        "{lo:?} {hi:?}"
    );
    set_param(&mut doc, "cut_at", "25 mm");
    let (lo, _) = k.bounds(body_handle(&mut doc, &k, split)).unwrap();
    assert!((lo[0] - 25.0).abs() < 1e-3, "{lo:?}");

    // 作業平面が作れない(板の外の面が無い)時は、別の平面に替えず止まる
    set_feature_data(
        &mut doc,
        cp,
        serde_json::to_value(offset(
            Base::Face {
                feature: p.plate,
                face: "無い面".into(),
            },
            "cut_at",
        ))
        .unwrap(),
    );
    assert!(
        matches!(status(&mut doc, &k, split), FeatureStatus::Blocked { feature: Some(f), .. } if f == cp),
        "{:?}",
        status(&mut doc, &k, split)
    );
}

#[test]
fn choosing_a_datum_plane_for_a_mirror_takes_only_one_made_before_it() {
    use kmb_core::{Context, Extension, Outcome};
    let k: std::sync::Arc<dyn Kernel> = std::sync::Arc::new(SmlibKernel::new());
    let mut p = plate();
    let before = p.b.feature(DATUM_PLANE, "前", offset(Base::Yz, "50 mm"));
    let mirror = p.b.feature(
        MIRROR,
        "写し",
        serde_json::json!({"kind": "bodies", "bodies": [p.pocket.0], "plane_ref": {"kind": "origin_yz"}}),
    );
    let after = p.b.feature(DATUM_PLANE, "後", offset(Base::Zx, "50 mm"));
    let mut doc = p.b.finish("板");
    let reg = registry();
    let report = doc.recompute(&reg, &*k);
    let run = |plane: FeatureId| {
        let args = serde_json::json!({"plane": plane.0});
        let cx = Context {
            args: &args,
            selected: Some(mirror),
            ..Context::new(doc.data(), &report, &reg, &k)
        };
        // 手順の候補にも前の作業平面だけが出る
        let shown = kmb_cad::plane::plane_choices(&cx, mirror);
        assert!(shown.iter().any(|c| c.value == serde_json::json!(before.0)));
        assert!(shown.iter().all(|c| c.value != serde_json::json!(after.0)));
        MirrorCommands
            .run(kmb_cad_pattern::MIRROR_PLANE, &cx)
            .expect("ミラーの操作")
    };
    let Ok(Outcome::Change { edits, .. }) = run(before) else {
        panic!("前の作業平面を選べない");
    };
    let kmb_core::Edit::SetData { data, .. } = &edits[0] else {
        panic!("{edits:?}");
    };
    assert_eq!(
        data["plane_ref"],
        serde_json::json!({"kind": "datum", "feature": before.0})
    );
    let Err(e) = run(after) else {
        panic!("後ろの作業平面を選べてしまう");
    };
    assert!(e.to_string().contains("前の作業平面ではありません"), "{e}");
}

fn set_feature_data(doc: &mut Document, id: FeatureId, data: serde_json::Value) {
    doc.apply(Transaction::new(
        "値",
        vec![kmb_core::Op::SetFeatureData { id, data }],
    ))
    .unwrap();
}
