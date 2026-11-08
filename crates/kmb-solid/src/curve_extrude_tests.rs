//! 楕円・スプラインの輪郭を押し出すと、側面が曲線ごとに 1 枚の面になり、上の縁を 1 本の辺として
//! 丸められる(SMLib)。truck は今までどおり線分と円弧の並びで作る。

use super::tests::{TestKernel, assert_ok, close, registry};
use super::*;
use kmb_core::Document;
use kmb_samples::sample::DocBuilder;
use kmb_sketch::{Sketch, SketchPlane};

/// XY 平面に、`draw` で描いたスケッチを 10 mm 押し出し、上の縁を `fillet` の半径で丸める文書。
fn extruded(draw: impl FnOnce(&mut Sketch) -> u32, fillet: Option<&str>) -> (Document, u64, u32) {
    let mut s = Sketch {
        plane: SketchPlane::Xy,
        ..Default::default()
    };
    let curve = draw(&mut s);
    let mut b = DocBuilder::new();
    let sk = b.feature(kmb_sketch::SKETCH, "曲線", s);
    let body = b.feature(
        EXTRUDE,
        "柱",
        Extrude {
            profile: Some(sk),
            distance: "10 mm".into(),
            ..Default::default()
        },
    );
    if let Some(r) = fillet {
        // 曲がった辺はカーネルの丸めで丸める(新しく作るフィレットの既定)
        b.feature(
            FILLET,
            "上の縁",
            Fillet {
                method: kmb_cad_edge::Method::Blend,
                ..Fillet::one(
                    body,
                    EdgeSelection {
                        edges: vec![[
                            format!("f{}:end", body.0),
                            format!("f{}:side:e{curve}", body.0),
                        ]],
                        faces: vec![],
                        arcs: false,
                    },
                    r,
                    false,
                    Default::default(),
                )
            },
        );
    }
    (b.finish("曲線の柱"), body.0, curve)
}

/// 見えるボディ 1 つの面の名前。
fn face_names(doc: &mut Document, k: &TestKernel) -> (Vec<String>, kmb_core::ShapeHandle) {
    let reg = registry();
    let report = doc.recompute(&reg, &**k);
    assert_ok(&report);
    let bodies = visible_bodies(doc.data(), &reg, &report);
    assert_eq!(bodies.len(), 1, "{bodies:?}");
    let names = k
        .faces(bodies[0].1)
        .unwrap()
        .into_iter()
        .map(|f| f.name)
        .collect();
    (names, bodies[0].1)
}

fn ellipse(s: &mut Sketch) -> u32 {
    s.add_ellipse((None, [0.0, 0.0]), (None, [30.0, 0.0]), [4.0, 12.0])
        .unwrap()
        .0
}

/// フィット点のスプライン(0,0)→(10,8)→(20,0) と、両端を結ぶ線分。
fn spline_and_line(s: &mut Sketch) -> u32 {
    let id = s
        .add_spline(
            &[(None, [0.0, 0.0]), (None, [10.0, 8.0]), (None, [20.0, 0.0])],
            false,
            false,
        )
        .unwrap();
    let Some(kmb_sketch::Entity::Spline { points, .. }) = s.entity(id).cloned() else {
        panic!("スプライン")
    };
    s.add_line(points[2], points[0]);
    id.0
}

#[test]
fn an_extruded_ellipse_has_one_side_face_and_its_top_edge_rounds() {
    let k = TestKernel::new();
    let (mut doc, body, curve) = extruded(ellipse, None);
    let (names, h) = face_names(&mut doc, &k);
    let want = std::f64::consts::PI * 30.0 * 12.0 * 10.0;
    // truck は曲面を網で近づけて体積を測るので、楕円柱より 0.1% ほどずれる
    let tol = if k.is_truck() { 2e-3 } else { 1e-4 };
    let v = k.mass(h).unwrap().volume;
    assert!(close(v, want, tol), "{v} {want}");
    if k.is_truck() {
        kmb_contract::skip!("truck は曲線のまま押し出さず、線分と円弧の並びで作る");
    }
    let mut sorted = names.clone();
    sorted.sort();
    let mut want_names = vec![
        format!("f{body}:end"),
        format!("f{body}:side:e{curve}"),
        format!("f{body}:start"),
    ];
    want_names.sort();
    assert_eq!(sorted, want_names);
    // 上の縁は 1 本の辺: 上面と側面の組で丸められる
    let (mut doc, ..) = extruded(ellipse, Some("1 mm"));
    let (_, rounded) = face_names(&mut doc, &k);
    let v = k.mass(rounded).unwrap().volume;
    assert!(v < want - 1.0 && v > want - 200.0, "{v} {want}");
}

#[test]
fn an_extruded_spline_closed_by_a_line_has_one_curved_side_face() {
    let k = TestKernel::new();
    if k.is_truck() {
        kmb_contract::skip!("truck は曲線のまま押し出さず、線分と円弧の並びで作る");
    }
    let (mut doc, body, curve) = extruded(spline_and_line, Some("1 mm"));
    let (names, _) = face_names(&mut doc, &k);
    let side = format!("f{body}:side:e{curve}");
    assert!(names.iter().any(|n| n.starts_with(&side)), "{names:?}");
    // 曲線の側面は 1 枚(丸めの面と、線分の側面・上下の面を除く)
    let curved: Vec<_> = names
        .iter()
        .filter(|n| n.starts_with(&format!("{side}.")))
        .collect();
    assert!(curved.is_empty(), "{names:?}");
}
