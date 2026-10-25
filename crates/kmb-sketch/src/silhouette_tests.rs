//! 交差の参照(面の名前つき)、シルエット、面の選び(Kie のレビュー K12・K13)。

use crate::{ProjectionSource, RefItem, RefShape, face_under, section_refs, silhouette_refs};
use kmb_core::{FeatureId, Mesh, Plane};

const BODY: FeatureId = FeatureId(7);

/// 網に面を 1 つ足す(四角形を 2 つの三角形に)。
fn quad(m: &mut Mesh, name: &str, p: [[f64; 3]; 4]) {
    let face = m.face_names.len() as u32;
    m.face_names.push(name.into());
    let base = m.positions.len() as u32;
    for q in p {
        m.positions.push(q.map(|v| v as f32));
        m.normals.push([0.0, 0.0, 1.0]);
    }
    m.triangles.push([base, base + 1, base + 2]);
    m.triangles.push([base, base + 2, base + 3]);
    m.triangle_faces.extend([face, face]);
}

/// 箱(x 0..w、y 0..h、z 0..t)。面の名前は bottom・top・x0・x1・y0・y1。
fn boxed(w: f64, h: f64, t: f64) -> Mesh {
    let mut m = Mesh::default();
    quad(
        &mut m,
        "bottom",
        [[0.0, 0.0, 0.0], [0.0, h, 0.0], [w, h, 0.0], [w, 0.0, 0.0]],
    );
    quad(
        &mut m,
        "top",
        [[0.0, 0.0, t], [w, 0.0, t], [w, h, t], [0.0, h, t]],
    );
    quad(
        &mut m,
        "y0",
        [[0.0, 0.0, 0.0], [w, 0.0, 0.0], [w, 0.0, t], [0.0, 0.0, t]],
    );
    quad(
        &mut m,
        "y1",
        [[0.0, h, 0.0], [0.0, h, t], [w, h, t], [w, h, 0.0]],
    );
    quad(
        &mut m,
        "x0",
        [[0.0, 0.0, 0.0], [0.0, 0.0, t], [0.0, h, t], [0.0, h, 0.0]],
    );
    quad(
        &mut m,
        "x1",
        [[w, 0.0, 0.0], [w, h, 0.0], [w, h, t], [w, 0.0, t]],
    );
    m
}

/// x 軸に沿って寝た円柱(x 0..l、半径 r、中心は y = 0, z = r)。側面と両端の面。
fn lying_cylinder(l: f64, r: f64) -> Mesh {
    let mut m = Mesh::default();
    let n = 64;
    let at = |x: f64, k: usize| {
        let t = std::f64::consts::TAU * k as f64 / n as f64;
        [x, r * t.cos(), r + r * t.sin()]
    };
    for k in 0..n {
        quad(
            &mut m,
            "side",
            [at(0.0, k), at(0.0, k + 1), at(l, k + 1), at(l, k)],
        );
    }
    m
}

fn mid(z: f64) -> Plane {
    Plane {
        origin: [0.0, 0.0, z],
        ..Plane::XY
    }
}

#[test]
fn a_section_names_the_face_each_line_was_cut_from() {
    let refs = section_refs(&boxed(60.0, 40.0, 10.0), &mid(3.0), BODY);
    assert_eq!(refs.len(), 4, "{refs:?}");
    let mut faces: Vec<&str> = refs
        .iter()
        .map(|r| match &r.source {
            ProjectionSource::Section {
                feature,
                face,
                index,
            } => {
                assert_eq!((*feature, *index), (BODY, 0));
                face.as_str()
            }
            other => panic!("{other:?}"),
        })
        .collect();
    faces.sort();
    assert_eq!(faces, ["x0", "x1", "y0", "y1"]);
    // 面 x1 の線は x = 60
    let x1 = refs
        .iter()
        .find(|r| matches!(&r.source, ProjectionSource::Section { face, .. } if face == "x1"))
        .unwrap();
    let RefShape::Line(a, b) = x1.shape else {
        panic!("{:?}", x1.shape)
    };
    assert!((a[0] - 60.0).abs() < 1e-6 && (b[0] - 60.0).abs() < 1e-6);
    // 同じ形を切り直すと同じ元になる(計算の時に取り直せる)
    let wider = section_refs(&boxed(80.0, 40.0, 10.0), &mid(3.0), BODY);
    let moved = wider.iter().find(|r| r.source.same(&x1.source)).unwrap();
    let RefShape::Line(a, _) = moved.shape else {
        panic!()
    };
    assert!((a[0] - 80.0).abs() < 1e-6);
}

#[test]
fn the_face_under_a_point_is_the_one_nearest_the_viewer() {
    let m = boxed(60.0, 40.0, 10.0);
    let (name, h) = face_under(&m, &Plane::XY, [30.0, 20.0]).unwrap();
    assert_eq!(name, "top");
    assert!((h - 10.0).abs() < 1e-6);
    assert!(face_under(&m, &Plane::XY, [70.0, 20.0]).is_none(), "箱の外");
}

#[test]
fn a_silhouette_reuses_the_body_edges_on_the_outline() {
    let m = boxed(60.0, 40.0, 10.0);
    // 底の 4 辺(body_refs が写す辺に当たる)と、外形に乗らない線(面の真ん中)
    let edge = |a: [f64; 2], b: [f64; 2], f: &str| RefItem {
        source: ProjectionSource::Edge {
            feature: BODY,
            faces: ["bottom".into(), f.into()],
        },
        shape: RefShape::Line(a, b),
    };
    let body = vec![
        edge([0.0, 0.0], [60.0, 0.0], "y0"),
        edge([60.0, 0.0], [60.0, 40.0], "x1"),
        edge([60.0, 40.0], [0.0, 40.0], "y1"),
        edge([0.0, 40.0], [0.0, 0.0], "x0"),
        edge([10.0, 20.0], [50.0, 20.0], "inner"),
    ];
    let out = silhouette_refs(&m, &Plane::XY, BODY, &body);
    assert_eq!(out.len(), 4, "{out:?}");
    assert!(out.iter().all(|r| body[..4].contains(r)), "外形の 4 辺だけ");
}

#[test]
fn a_silhouette_without_edges_is_fitted_from_the_mesh() {
    // 寝た円柱を上から見ると 20 × 10 の長方形(元の辺は無い)
    let out = silhouette_refs(&lying_cylinder(20.0, 5.0), &Plane::XY, BODY, &[]);
    assert_eq!(out.len(), 4, "{out:?}");
    for (i, r) in out.iter().enumerate() {
        assert_eq!(
            r.source,
            ProjectionSource::Silhouette {
                feature: BODY,
                index: i as u32
            }
        );
        let RefShape::Line(a, b) = r.shape else {
            panic!("{:?}", r.shape)
        };
        for p in [a, b] {
            assert!(
                (p[0].abs() < 1e-6 || (p[0] - 20.0).abs() < 1e-6)
                    && (p[1].abs() - 5.0).abs() < 1e-6,
                "{p:?}"
            );
        }
    }
}
