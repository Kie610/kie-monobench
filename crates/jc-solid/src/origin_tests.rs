//! 面の上のスケッチの原点(まとめてレビューの R2): 新しいスケッチは面の左下の角、古いスケッチは
//! 世界の原点を写した点のまま、寸法を変えても同じ角に付いて行く。

use super::*;
use jc_core::{Document, FeatureId, FeatureStatus, Kernel, Payload, Points, Registry, Value};
use jc_kernel_smlib::SmlibKernel;
use jc_samples::sample::{DocBuilder, SketchBuilder};
use jc_sketch::{Corner, FaceOrigin, SketchPlane};

fn registry() -> Registry {
    let mut r = Registry::new();
    jc_sketch::register(&mut r);
    register(&mut r);
    r
}

/// 板: 左下 (x0, 5) から幅 30・奥行き 20 の長方形を 5 mm 押し出す(x0 はパラメータ)。上面に
/// `plane` のスケッチを置き、その原点に固定の点を 1 つ打つ。
fn plate(origin: Option<FaceOrigin>) -> (Document, FeatureId, FeatureId) {
    let mut b = DocBuilder::new();
    b.param("x0", "10 mm");
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let o = s.fixed_point(0.0, 0.0);
    let c = s.point_from(o, "x0", "5 mm", [10.0, 5.0]);
    s.rectangle(c, "30 mm", "20 mm", [10.0, 5.0, 30.0, 20.0]);
    let sk = b.feature(jc_sketch::SKETCH, "板の形", s.build());
    let plate = b.feature(
        EXTRUDE,
        "板",
        Extrude {
            profile: Some(sk),
            regions: Vec::new(),
            distance: "5 mm".into(),
            direction: Direction::Normal,
            distance2: None,
            extent: Default::default(),
            to_face: None,
            to_offset: None,
            operation: Operation::New,
            target: None,
            ..Default::default()
        },
    );
    let top = format!("f{}:end", plate.0);
    let mut top_sketch = SketchBuilder::new(SketchPlane::face(plate, top.clone()));
    top_sketch.fixed_point(0.0, 0.0);
    let mut data = top_sketch.build();
    if let Some(origin) = origin {
        data.plane = SketchPlane::Face {
            feature: plate,
            face: top,
            origin,
        };
    }
    let on_top = b.feature(jc_sketch::SKETCH, "上面", data);
    (b.finish("板"), plate, on_top)
}

fn outputs(doc: &mut Document, k: &dyn Kernel, id: FeatureId) -> jc_core::Outputs {
    let report = doc.recompute(&registry(), k);
    match report.feature(id) {
        Some(FeatureStatus::Ok { outputs, .. }) => outputs.clone(),
        other => panic!("{other:?}"),
    }
}

/// スケッチの原点に打った点の世界の位置。
fn origin_point(doc: &mut Document, k: &dyn Kernel, id: FeatureId) -> [f64; 3] {
    let out = outputs(doc, k, id);
    let pts = Points::from_value(&out["points"]).expect("点");
    let p = pts.points.values().next().expect("点が 1 つ");
    pts.plane.to_world(*p)
}

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() < 1e-6)
}

fn body(doc: &mut Document, k: &dyn Kernel, plate: FeatureId) -> jc_core::ShapeHandle {
    match &outputs(doc, k, plate)["body"] {
        Value::Shape(h) => *h,
        v => panic!("{v:?}"),
    }
}

#[test]
fn a_new_sketch_on_a_face_starts_at_its_lower_left_corner_and_follows_it() {
    let k = SmlibKernel::new();
    let (mut doc, plate, _) = plate(None);
    let h = body(&mut doc, &k, plate);
    let face = format!("f{}:end", plate.0);
    let origin = jc_sketch::default_origin(&k, h, &face);
    assert!(matches!(origin, FaceOrigin::Vertex { .. }), "{origin:?}");
    let (mut doc, plate, on_top) = self::plate(Some(origin));
    assert!(near(origin_point(&mut doc, &k, on_top), [10.0, 5.0, 5.0]));
    // 板を動かす(x0 を 10 → 4 mm)と、原点も同じ角に付いて行く
    let id = doc.data().params[0].id;
    doc.apply(jc_core::Transaction::new(
        "x0",
        vec![jc_core::Op::SetParamExpr {
            id,
            expr: "4 mm".into(),
        }],
    ))
    .unwrap();
    assert!(near(origin_point(&mut doc, &k, on_top), [4.0, 5.0, 5.0]));
    let _ = plate;
}

#[test]
fn a_lost_origin_corner_falls_back_to_the_lower_left_with_a_warning() {
    let k = SmlibKernel::new();
    let lost = FaceOrigin::Vertex {
        faces: vec!["f9:none".into(), "f9:gone".into()],
    };
    let (mut doc, _, on_top) = plate(Some(lost));
    assert!(near(origin_point(&mut doc, &k, on_top), [10.0, 5.0, 5.0]));
    let out = outputs(&mut doc, &k, on_top);
    let w = jc_core::warnings_of(&out);
    assert!(
        w.iter().any(|m| m.contains("原点") && m.contains("左下")),
        "{w:?}"
    );
}

/// 古いスケッチ(原点を書かない)を上面に置いた板 1 枚で、読むだけの段を先に、原点を替える段を後に。
/// - 古いスケッチは世界の原点を写した点のまま、保存した形も変わらない
/// - 面の 4 つの角を出し、右上を原点にすると平面の原点がその角になる(向きは変えない)
/// - 原点を左下の角へ替えても、描いた点は世界の中で動かない
#[test]
fn an_old_face_sketch_offers_four_corners_and_moves_its_origin_in_place() {
    let k = SmlibKernel::new();
    let (mut doc, plate, on_top) = plate(None);
    // 古いスケッチは世界の原点を写した点
    let before = origin_point(&mut doc, &k, on_top);
    assert!(near(before, [0.0, 0.0, 5.0]));
    // 保存した形も変わらない(origin を書かない)
    let data = &doc.data().feature(on_top).unwrap().data;
    assert!(data["plane"].get("origin").is_none(), "{data}");

    // 面の 4 つの角
    let h = body(&mut doc, &k, plate);
    let face = format!("f{}:end", plate.0);
    let spots = jc_sketch::face_spots(&k, h, &face);
    assert_eq!(spots.len(), 4, "{spots:?}");
    let (base, _) = jc_sketch::face_plane(&k, h, &face, &FaceOrigin::World).unwrap();
    let at = |c| jc_sketch::corner_spot(&spots, &base, c).unwrap().at;
    assert!(near(at(Corner::LowerLeft), [10.0, 5.0, 5.0]));
    assert!(near(at(Corner::LowerRight), [40.0, 5.0, 5.0]));
    assert!(near(at(Corner::UpperRight), [40.0, 25.0, 5.0]));
    assert!(near(at(Corner::UpperLeft), [10.0, 25.0, 5.0]));
    // 右上を原点にすると、平面の原点がその角になる(向きは変えない)
    let ur = jc_sketch::corner_spot(&spots, &base, Corner::UpperRight).unwrap();
    let (p, note) = jc_sketch::face_plane(&k, h, &face, &ur.origin).unwrap();
    assert!(note.is_none());
    assert!(near(p.origin, [40.0, 25.0, 5.0]));
    assert_eq!((p.x_axis, p.y_axis), (base.x_axis, base.y_axis));

    // 原点を左下の角へ替える
    let ll = jc_sketch::corner_spot(&spots, &base, Corner::LowerLeft).unwrap();
    let entry = doc.data().feature(on_top).unwrap();
    let sketch: jc_sketch::Sketch = serde_json::from_value(entry.data.clone()).unwrap();
    let moved = sketch
        .with_origin(ll.origin.clone(), base.to_local(ll.at))
        .expect("面の上");
    doc.apply(jc_core::Transaction::new(
        "原点",
        vec![jc_core::Op::SetFeatureData {
            id: on_top,
            data: serde_json::to_value(&moved).unwrap(),
        }],
    ))
    .unwrap();
    // 描いた点は世界の中で動かない(原点だけが角へ移る)
    assert!(near(origin_point(&mut doc, &k, on_top), before));
    let out = outputs(&mut doc, &k, on_top);
    let pts = Points::from_value(&out["points"]).unwrap();
    assert!(near(pts.plane.origin, [10.0, 5.0, 5.0]));
}
