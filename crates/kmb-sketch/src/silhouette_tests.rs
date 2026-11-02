//! 確かめること: 交差の参照の面の名前と、曲線を含むシルエットの個数・閉じ方。

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

fn cap(mesh: &mut Mesh, name: &str, center: [f64; 3], rim: &[[f64; 3]]) {
    let face = mesh.face_names.len() as u32;
    mesh.face_names.push(name.into());
    let base = mesh.positions.len() as u32;
    mesh.positions.push(center.map(|v| v as f32));
    mesh.positions
        .extend(rim.iter().map(|p| p.map(|v| v as f32)));
    mesh.normals.resize(mesh.positions.len(), [0.0, 0.0, 1.0]);
    for i in 0..rim.len() {
        mesh.triangles.push([
            base,
            base + 1 + i as u32,
            base + 1 + ((i + 1) % rim.len()) as u32,
        ]);
        mesh.triangle_faces.push(face);
    }
}

/// x 軸に沿う、両端の半径を指定した閉じた円柱・円すいの網。
fn tapered_cylinder(length: f64, first: f64, last: f64) -> Mesh {
    let mut mesh = Mesh::default();
    let rim = |x: f64, radius: f64| -> Vec<_> {
        (0..128)
            .map(|i| {
                let a = std::f64::consts::TAU * i as f64 / 128.0;
                [x, radius * a.cos(), first + radius * a.sin()]
            })
            .collect()
    };
    let a = rim(0.0, first);
    let b = rim(length, last);
    for i in 0..a.len() {
        let j = (i + 1) % a.len();
        quad(&mut mesh, "wall", [a[i], a[j], b[j], b[i]]);
    }
    cap(&mut mesh, "start", [0.0, 0.0, first], &a);
    if last > 0.0 {
        cap(&mut mesh, "end", [length, 0.0, first], &b);
    }
    mesh
}

/// 高さ方向の4辺を半径rでフィレットした箱。rが平面の縦の幅の半分なら長円になる。
fn rounded_box(width: f64, height: f64, r: f64) -> Mesh {
    let mut rim = Vec::new();
    for (center, start) in [
        ([width - r, r], -std::f64::consts::FRAC_PI_2),
        ([width - r, height - r], 0.0),
        ([r, height - r], std::f64::consts::FRAC_PI_2),
        ([r, r], std::f64::consts::PI),
    ] {
        for i in 0..=32 {
            let a = start + std::f64::consts::FRAC_PI_2 * i as f64 / 32.0;
            rim.push([center[0] + r * a.cos(), center[1] + r * a.sin(), 0.0]);
        }
    }
    let top: Vec<_> = rim.iter().map(|p| [p[0], p[1], 8.0]).collect();
    let mut mesh = Mesh::default();
    for i in 0..rim.len() {
        let j = (i + 1) % rim.len();
        quad(&mut mesh, "wall", [rim[i], rim[j], top[j], top[i]]);
    }
    cap(&mut mesh, "bottom", [width / 2.0, height / 2.0, 0.0], &rim);
    cap(&mut mesh, "top", [width / 2.0, height / 2.0, 8.0], &top);
    mesh
}

fn closed_outline(mesh: &Mesh, plane: Plane, area: f64) -> Vec<RefItem> {
    let out = silhouette_refs(mesh, &plane, BODY, &[]);
    assert_outline_closes(&out, plane, area);
    out
}

fn assert_outline_closes(out: &[RefItem], plane: Plane, area: f64) {
    assert!(!out.is_empty());
    let mut sketch = crate::Sketch::default();
    for item in out {
        sketch.add_projection_as(item, false);
    }
    let solution = crate::solve(&sketch, &Default::default()).unwrap();
    let (profile, unused) = crate::profile_of(&sketch, &solution, plane);
    assert_eq!(profile.regions.len(), 1, "{out:?}");
    assert!(profile.regions[0].holes.is_empty());
    assert!(unused.is_empty(), "{unused:?}");
    assert!(crate::open_ends_of(&sketch, &solution).is_empty());
    let measured = profile.regions[0].outer.signed_area();
    assert!(
        (measured - area).abs() <= area * 0.005,
        "{measured} != {area}"
    );
}

fn shape_counts(out: &[RefItem]) -> (usize, usize, usize) {
    let mut counts = (0, 0, 0);
    for item in out {
        match item.shape {
            RefShape::Line(..) => counts.0 += 1,
            RefShape::Circle { arc: Some(_), .. } => counts.1 += 1,
            RefShape::Circle { arc: None, .. } => counts.2 += 1,
            RefShape::Point(_) => panic!("外形に点だけの参照が入りました"),
        }
    }
    counts
}

#[test]
fn a_cylinders_side_and_end_silhouettes_close_as_a_rectangle_and_a_circle() {
    let mesh = tapered_cylinder(20.0, 5.0, 5.0);
    let side = closed_outline(&mesh, Plane::XY, 200.0);
    assert_eq!(shape_counts(&side), (4, 0, 0));
    let end = closed_outline(&mesh, Plane::YZ, std::f64::consts::PI * 25.0);
    assert_eq!(shape_counts(&end), (0, 0, 1));
}

#[test]
fn cone_rims_are_one_closed_circle_in_the_axial_silhouette() {
    for radius in [0.0, 2.0] {
        let mesh = tapered_cylinder(20.0, 5.0, radius);
        let out = closed_outline(&mesh, Plane::YZ, std::f64::consts::PI * 25.0);
        assert_eq!(shape_counts(&out), (0, 0, 1));
    }
}

#[test]
fn a_filleted_box_silhouette_has_four_lines_and_four_arcs() {
    let out = closed_outline(
        &rounded_box(30.0, 20.0, 3.0),
        Plane::XY,
        600.0 - (4.0 - std::f64::consts::PI) * 9.0,
    );
    assert_eq!(shape_counts(&out), (4, 4, 0), "{out:?}");
    assert_rounded_boundary(&out, 30.0, 20.0, 3.0);
}

#[test]
fn a_capsule_silhouette_has_two_lines_and_two_semicircles() {
    let out = closed_outline(
        &rounded_box(30.0, 10.0, 5.0),
        Plane::XY,
        200.0 + std::f64::consts::PI * 25.0,
    );
    assert_eq!(shape_counts(&out), (2, 2, 0), "{out:?}");
    assert_rounded_boundary(&out, 30.0, 10.0, 5.0);
}

#[test]
fn a_plain_box_without_edge_references_keeps_four_closed_lines() {
    let out = closed_outline(&boxed(30.0, 20.0, 8.0), Plane::XY, 600.0);
    assert_eq!(shape_counts(&out), (4, 0, 0));
}

fn assert_rounded_boundary(out: &[RefItem], width: f64, height: f64, radius: f64) {
    for item in out {
        for p in crate::reference_samples(&item.shape, 64) {
            let x = (p[0] - width / 2.0).abs() - (width / 2.0 - radius);
            let y = (p[1] - height / 2.0).abs() - (height / 2.0 - radius);
            let distance = x.max(0.0).hypot(y.max(0.0)) + x.max(y).min(0.0) - radius;
            assert!(
                distance.abs() <= crate::SECTION_TOL + 1e-5,
                "{p:?}: {distance}"
            );
        }
    }
}

#[test]
fn fitted_curves_keep_saved_straight_references_on_the_same_sides() {
    // 保存した写しの4辺と、左下の丸めを細かい線分として持つデータ。
    let saved = [
        (0, [0.0, 17.0], [0.01444581151008606, 2.7059485912323]),
        (7, [3.0, 0.0], [27.294052124023438, 0.01444581151008606]),
        (14, [30.0, 3.0], [29.985553741455078, 17.294052124023438]),
        (21, [27.0, 20.0], [2.7059485912323, 19.985553741455078]),
        (
            3,
            [0.5055911540985107, 1.3332892656326294],
            [0.9853231310844421, 0.7771466374397278],
        ),
    ];
    let mut sketch = crate::Sketch::default();
    let mut refs = Vec::new();
    let mut entities = Vec::new();
    for (index, a, b) in saved {
        let item = RefItem {
            source: ProjectionSource::Silhouette {
                feature: BODY,
                index,
            },
            shape: RefShape::Line(a, b),
        };
        entities.push(sketch.add_projection_as(&item, false));
        refs.push(item);
    }
    let curved = entities[4];
    let before: Vec<_> = sketch
        .entity(curved)
        .unwrap()
        .points()
        .into_iter()
        .map(|id| (id, sketch.entity(id).unwrap().clone()))
        .collect();
    let out = silhouette_refs(&rounded_box(30.0, 20.0, 3.0), &Plane::XY, BODY, &[]);
    let lost = crate::refresh_projections(&mut sketch, &out);
    assert_eq!(lost, vec![curved], "丸めの線分だけを選び直す");
    for old in &refs[..4] {
        let now = out.iter().find(|r| r.source.same(&old.source)).unwrap();
        assert!(matches!(now.shape, RefShape::Line(..)));
        for p in crate::reference_samples(&old.shape, 16) {
            assert!(now.shape.distance(p) <= crate::SECTION_TOL);
        }
    }
    for (id, point) in before {
        assert_eq!(sketch.entity(id), Some(&point));
    }
}

#[test]
fn a_curved_silhouette_keeps_the_original_straight_body_edges() {
    let body: Vec<_> = [
        ([3.0, 0.0], [27.0, 0.0]),
        ([30.0, 3.0], [30.0, 17.0]),
        ([27.0, 20.0], [3.0, 20.0]),
        ([0.0, 17.0], [0.0, 3.0]),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (a, b))| RefItem {
        source: ProjectionSource::Edge {
            feature: BODY,
            faces: ["cap".into(), format!("side{i}")],
        },
        shape: RefShape::Line(a, b),
    })
    .collect();
    let out = silhouette_refs(&rounded_box(30.0, 20.0, 3.0), &Plane::XY, BODY, &body);
    assert_eq!(shape_counts(&out), (4, 4, 0));
    for edge in &body {
        assert!(out.contains(edge));
    }
    assert_outline_closes(&out, Plane::XY, 600.0 - (4.0 - std::f64::consts::PI) * 9.0);
}
