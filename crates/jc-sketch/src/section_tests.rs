//! 断面(交差・スライス)の確かめ。網は手で作る(箱と円柱)。

use crate::{Shape, section};
use jc_core::{Mesh, Plane};

/// 点と三角形(面の番号つき)から網を作る。
fn mesh(points: Vec<[f64; 3]>, tris: Vec<([u32; 3], u32)>) -> Mesh {
    Mesh {
        positions: points.iter().map(|p| p.map(|x| x as f32)).collect(),
        normals: vec![[0.0, 0.0, 1.0]; points.len()],
        triangles: tris.iter().map(|t| t.0).collect(),
        triangle_faces: tris.iter().map(|t| t.1).collect(),
        face_names: (0..8).map(|i| format!("f{i}")).collect(),
        edges: vec![],
    }
}

/// 0..sx × 0..sy × 0..sz の箱(面ごとに番号)。
fn cuboid(sx: f64, sy: f64, sz: f64) -> Mesh {
    let p = |i: u32| {
        [
            if i & 1 != 0 { sx } else { 0.0 },
            if i & 2 != 0 { sy } else { 0.0 },
            if i & 4 != 0 { sz } else { 0.0 },
        ]
    };
    let quads: [([u32; 4], u32); 6] = [
        ([0, 2, 3, 1], 0),
        ([4, 5, 7, 6], 1),
        ([0, 1, 5, 4], 2),
        ([2, 6, 7, 3], 3),
        ([0, 4, 6, 2], 4),
        ([1, 3, 7, 5], 5),
    ];
    let mut tris = vec![];
    for (q, f) in quads {
        tris.push(([q[0], q[1], q[2]], f));
        tris.push(([q[0], q[2], q[3]], f));
    }
    mesh((0..8).map(p).collect(), tris)
}

/// 中心が原点、半径 `r`、高さ 0..h の円柱(側面は 1 つの面、`n` 角形)。
fn cylinder(r: f64, h: f64, n: u32) -> Mesh {
    let mut pts = vec![];
    for k in 0..n {
        let a = std::f64::consts::TAU * k as f64 / n as f64;
        pts.push([r * a.cos(), r * a.sin(), 0.0]);
        pts.push([r * a.cos(), r * a.sin(), h]);
    }
    pts.push([0.0, 0.0, 0.0]);
    pts.push([0.0, 0.0, h]);
    let (c0, c1) = (2 * n, 2 * n + 1);
    let mut tris = vec![];
    for k in 0..n {
        let (a0, a1) = (2 * k, 2 * k + 1);
        let (b0, b1) = (2 * ((k + 1) % n), 2 * ((k + 1) % n) + 1);
        tris.push(([a0, b0, b1], 0));
        tris.push(([a0, b1, a1], 0));
        tris.push(([c0, b0, a0], 1));
        tris.push(([c1, a1, b1], 2));
    }
    mesh(pts, tris)
}

fn lines(s: &[Shape]) -> usize {
    s.iter().filter(|s| matches!(s, Shape::Line { .. })).count()
}

#[test]
fn a_box_cut_across_gives_its_rectangle() {
    let s = section(&cuboid(20.0, 10.0, 5.0), &Plane::XY.offset(2.5));
    assert_eq!(lines(&s), 4, "{s:?}");
    assert_eq!(s.len(), 4);
    let mut lens: Vec<f64> = s
        .iter()
        .map(|s| match s {
            Shape::Line { a, b } => ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt(),
            _ => 0.0,
        })
        .collect();
    lens.sort_by(f64::total_cmp);
    assert!(
        (lens[0] - 10.0).abs() < 1e-6 && (lens[3] - 20.0).abs() < 1e-6,
        "{lens:?}"
    );
}

#[test]
fn a_cylinder_cut_across_gives_one_circle() {
    let s = section(&cylinder(5.0, 10.0, 128), &Plane::XY.offset(5.0));
    assert_eq!(s.len(), 1, "{s:?}");
    match s[0] {
        Shape::Circle { center, radius } => {
            assert!(center[0].abs() < 1e-3 && center[1].abs() < 1e-3);
            assert!((radius - 5.0).abs() < 0.01, "{radius}");
        }
        _ => panic!("{s:?}"),
    }
}

#[test]
fn a_cylinder_cut_along_its_axis_gives_a_rectangle_even_through_mesh_corners() {
    // 平面がちょうど網の縦の辺を通る(点と辺が平面に乗る)
    let s = section(&cylinder(5.0, 10.0, 64), &Plane::YZ);
    assert_eq!(lines(&s), s.len(), "{s:?}");
    assert_eq!(s.len(), 4, "{s:?}");
}

#[test]
fn a_missed_box_gives_nothing_and_a_touching_face_gives_its_outline() {
    assert!(section(&cuboid(5.0, 5.0, 5.0), &Plane::XY.offset(9.0)).is_empty());
    // 面に乗った平面(底面)では、その面の輪郭
    let s = section(&cuboid(5.0, 5.0, 5.0), &Plane::XY);
    assert_eq!(lines(&s), 4, "{s:?}");
}

#[test]
fn fixing_a_circle_fixes_its_size_too() {
    let mut s = crate::Sketch::default();
    let c = s.add_point(3.0, 4.0);
    let circle = s.add_circle(c, 5.0);
    s.add_constraint(crate::ConstraintKind::Fix { point: circle });
    let sol = crate::solve(&s, &Default::default()).unwrap();
    assert_eq!(sol.status, crate::SolveStatus::WellConstrained);
}
