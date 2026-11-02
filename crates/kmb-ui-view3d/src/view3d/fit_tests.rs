//! 全体表示で収める箱の試験。

use super::*;

/// 1 辺 `a` の正方形の板(三角形 2 つ)。
fn square(a: f32) -> Mesh {
    Mesh {
        positions: vec![[0.0, 0.0, 0.0], [a, 0.0, 0.0], [a, a, 0.0], [0.0, a, 0.0]],
        normals: vec![[0.0, 0.0, 1.0]; 4],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        triangle_faces: vec![0, 0],
        face_names: vec!["上".into()],
        ..Default::default()
    }
}

/// 解析の結果の断面のように、部品の代わりに描く形がボディより大きい時は、その形まで収める。
#[test]
fn fit_includes_the_shape_drawn_instead_of_the_bodies() {
    let mut sc = Scene::default();
    sc.set(&[], vec![(FeatureId(1), square(2.0))]);
    let mut overlay = kmb_core::Overlay::default();
    assert_eq!(fit_bounds(&sc, &overlay).unwrap().1, [2.0, 2.0, 0.0]);
    overlay.shape = Some(std::sync::Arc::new(kmb_core::OverlayShape {
        mesh: square(30.0),
        ..Default::default()
    }));
    let (lo, hi) = fit_bounds(&sc, &overlay).unwrap();
    assert_eq!((lo, hi), ([0.0; 3], [30.0, 30.0, 0.0]));
}

/// ボディも形も無ければ重ねる線(スケッチだけ)、何も無ければ合わせない。
#[test]
fn fit_falls_back_to_lines_and_then_to_nothing() {
    let sc = Scene::default();
    let mut overlay = kmb_core::Overlay::default();
    assert!(fit_bounds(&sc, &overlay).is_none());
    overlay.lines.push((
        vec![[-1.0, 0.0, 0.0], [4.0, 5.0, 0.0]],
        kmb_core::LineKind::new(kmb_core::LineInk("main"), ""),
    ));
    assert_eq!(
        fit_bounds(&sc, &overlay).unwrap(),
        ([-1.0, 0.0, 0.0], [4.0, 5.0, 0.0])
    );
}
