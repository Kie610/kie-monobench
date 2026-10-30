//! 確かめること: 解析解の節点値から塗る表面の補間誤差と、一定の場の保存。

use super::*;

fn scale() -> Scale {
    Scale {
        lo: 0.0,
        span: 2.0,
        levels: 40,
        reverse: false,
        cap: None,
        base: 0,
    }
}

fn triangle() -> Tets {
    Tets {
        points: NODE_RS.map(|[r, s]| [r as f32, s as f32, 0.0]).to_vec(),
        faces: vec![[0, 1, 2, 3, 4, 5]],
        tets: Vec::new(),
        face_ids: vec![0],
    }
}

#[test]
fn quadratic_field_follows_every_painted_position() {
    let t = triangle();
    let f = |x: f64, y: f64| x * x + 2.0 * y * y + x * y;
    let v = Values::exact(
        t.points
            .iter()
            .map(|p| f(p[0] as f64, p[1] as f64) as f32)
            .collect(),
    );
    let d = Values::exact(
        t.points
            .iter()
            .flat_map(|p| [0.0, 0.0, (f(p[0] as f64, p[1] as f64) / 16.0) as f32])
            .collect(),
    );
    let mut b = Builder::default();
    tets_mesh(
        &mut b,
        &t,
        Some((&d, 1.0)),
        Some((Src::Plain(&v, 1), &scale())),
        None,
        0,
    );
    assert!(b.mesh.triangles.len() > 4);
    let error = b
        .mesh
        .positions
        .iter()
        .map(|p| (p[2] as f64 - f(p[0] as f64, p[1] as f64) / 16.0).abs())
        .fold(0.0, f64::max);
    eprintln!("2 次式の変位の最大誤差 {error:.12}");
    // 描く網の座標は f32。補間の f64 の精度は別に確かめる。
    assert!(error < 1e-7, "2 次式の変位の最大誤差 {error}");
}

fn annulus() -> (Tets, Values, Scale) {
    let mut t = Tets {
        points: Vec::new(),
        tets: Vec::new(),
        faces: Vec::new(),
        face_ids: Vec::new(),
    };
    let (nr, nt) = (8, 48);
    for j in 0..nr {
        for i in 0..nt {
            let p = |r: f64, a: f64| [r * a.cos(), r * a.sin(), 3.0];
            let (ra, rb) = (
                5.0 + 5.0 * j as f64 / nr as f64,
                5.0 + 5.0 * (j + 1) as f64 / nr as f64,
            );
            let (a, b) = (
                std::f64::consts::TAU * i as f64 / nt as f64,
                std::f64::consts::TAU * (i + 1) as f64 / nt as f64,
            );
            let quad = [p(ra, a), p(rb, a), p(rb, b), p(ra, b)];
            for indices in [[0, 1, 2], [0, 2, 3]] {
                let corners = indices.map(|k| quad[k]);
                let mut nodes = Vec::from(corners);
                for [u, v] in [[0, 1], [1, 2], [2, 0]] {
                    // 平らな端面の内部の中点は直線の辺の真ん中。
                    nodes.push([0, 1, 2].map(|k| (corners[u][k] + corners[v][k]) * 0.5));
                }
                let base = t.points.len() as u32;
                t.faces.push(std::array::from_fn(|k| base + k as u32));
                t.face_ids.push(0);
                t.points
                    .extend(nodes.into_iter().map(|p| p.map(|v| v as f32)));
            }
        }
    }
    // 内圧 1、内径の半径 5、外径の半径 10 の閉じた厚肉円筒の端面。
    let (a, b) = (1.0 / 3.0, 100.0 / 3.0);
    let stress = Values::exact(
        t.points
            .iter()
            .flat_map(|p| {
                let (x, y) = (p[0] as f64, p[1] as f64);
                let r2 = x * x + y * y;
                [
                    a + b * (y * y - x * x) / (r2 * r2),
                    a + b * (x * x - y * y) / (r2 * r2),
                    a,
                    -2.0 * b * x * y / (r2 * r2),
                    0.0,
                    0.0,
                ]
                .map(|v| v as f32)
            })
            .collect(),
    );
    let lo = 3.0f64.sqrt() * b / 100.0;
    let hi = 3.0f64.sqrt() * b / 25.0;
    let s = Scale {
        lo: lo as f32,
        span: (hi - lo) as f32,
        ..scale()
    };
    (t, stress, s)
}

#[test]
fn pressure_cylinder_band_borders_are_concentric() {
    let (t, v, s) = annulus();
    let mut b = Builder::default();
    tets_mesh(
        &mut b,
        &t,
        None,
        Some((Src::Tensor(&v, Derive::VonMises), &s)),
        None,
        0,
    );
    let mut points = std::collections::BTreeMap::<[i64; 2], ([f64; 2], u32, u32)>::new();
    for (tri, face) in b.mesh.triangles.iter().zip(&b.mesh.triangle_faces) {
        for i in tri {
            let p = b.mesh.positions[*i as usize];
            let key = [p[0], p[1]].map(|x| (x as f64 * 1e5).round() as i64);
            let e = points
                .entry(key)
                .or_insert(([p[0] as f64, p[1] as f64], *face, *face));
            e.1 = e.1.min(*face);
            e.2 = e.2.max(*face);
        }
    }
    let radius = |value: f64| (3.0f64.sqrt() * (100.0 / 3.0) / value).sqrt();
    let step = s.span as f64 / s.levels as f64;
    let mut worst = 0.0f64;
    let mut count = 0;
    for (p, lo, hi) in points.values() {
        if lo == hi {
            continue;
        }
        let value = s.lo as f64 + step * (*hi as f64);
        let width = radius(value - step * 0.5) - radius(value + step * 0.5);
        worst = worst.max((p[0].hypot(p[1]) - radius(value)).abs() / width);
        count += 1;
    }
    assert!(count > 1000, "帯の境の点 {count}");
    eprintln!(
        "円筒の帯の境: 点 {count}、最大半径誤差 / 帯幅 {worst:.9}、三角形 {}",
        b.mesh.triangles.len()
    );
    // 4 分割の線形補間では 0.139411207 段。半分以下かつ 0.1 段以下。
    assert!(
        worst < 0.139411207 * 0.5 && worst < 0.1,
        "帯の境の波 {worst:.9} 段"
    );
}

#[test]
fn constant_tensor_keeps_the_same_band() {
    let t = triangle();
    let v = Values::exact(
        (0..6)
            .flat_map(|_| [0.5, 0.25, 0.125, 0.0, 0.0, 0.0])
            .collect(),
    );
    let mut b = Builder::default();
    tets_mesh(
        &mut b,
        &t,
        None,
        Some((Src::Tensor(&v, Derive::VonMises), &scale())),
        None,
        0,
    );
    assert!(!b.mesh.triangles.is_empty());
    assert!(
        b.mesh
            .triangle_faces
            .iter()
            .all(|f| *f == b.mesh.triangle_faces[0])
    );
    let nodes = NODE_RS.map(|[r, s]| Vtx {
        p: [r, s, 0.0],
        d: [0.0; 3],
        s: [0.5, 0.25, 0.125, 0.0, 0.0, 0.0],
        c: [0.0; 6],
        n: [0.0, 0.0, 1.0],
    });
    let src = Src::Tensor(&v, Derive::VonMises);
    let want = src.value(&nodes[0].s);
    for i in 0..=20 {
        for j in 0..=20 - i {
            assert!(
                (src.value(&quadratic(&nodes, [i as f64 / 20.0, j as f64 / 20.0]).s) - want).abs()
                    < 1e-6
            );
        }
    }
    assert_eq!(b.mesh.triangles.len(), 1, "平らで一定の場は分けない");
}

#[test]
fn quadratic_components_positions_and_normals_are_exact_before_mesh_rounding() {
    let f = |r: f64, s: f64| r * r + 2.0 * s * s + r * s + 0.25 * r - 0.5 * s;
    let nodes = NODE_RS.map(|[r, s]| Vtx {
        p: [r, s, f(r, s)],
        d: [f(r, s), 2.0 * f(r, s), -f(r, s)],
        s: std::array::from_fn(|k| f(r, s) * (k + 1) as f64 + k as f64),
        c: [0.0; 6],
        n: [r, s, f(r, s)],
    });
    let mut points = vec![[0.13, 0.27], [0.0, 0.0], [0.125, 0.875], [0.5, 0.25]];
    points.extend(clip_surface(
        &NODE_RS[..3],
        &|q| quadratic(&nodes, q).s[0],
        0.37,
        true,
    ));
    for [r, s] in points {
        let v = quadratic(&nodes, [r, s]);
        let want = f(r, s);
        for k in 0..6 {
            assert!((v.s[k] - (want * (k + 1) as f64 + k as f64)).abs() < 1e-9);
        }
        for (got, want) in
            v.p.into_iter()
                .zip([r, s, want])
                .chain(v.d.into_iter().zip([want, 2.0 * want, -want]))
                .chain(v.n.into_iter().zip([r, s, want]))
        {
            assert!((got - want).abs() < 1e-9);
        }
    }
}

#[test]
fn reducing_the_surface_depth_keeps_every_face_and_existing_geometry() {
    let v = Values::exact(vec![0.0; 6]);
    let scales: Vec<Scale> = (0..4)
        .map(|i| Scale {
            base: 100 * i,
            ..scale()
        })
        .collect();
    let surfaces: Vec<Surface<'_>> = scales
        .iter()
        .enumerate()
        .map(|(i, s)| Surface {
            nodes: NODE_RS.map(|[r, s]| Vtx {
                p: [r + 2.0 * i as f64, s, 0.0],
                d: [0.0; 3],
                s: [r * r + 2.0 * s * s, 0.0, 0.0, 0.0, 0.0, 0.0],
                c: [0.0; 6],
                n: [0.0, 0.0, 1.0],
            }),
            paint: Some((Src::Plain(&v, 1), s)),
            plane: None,
            gray: 0,
        })
        .collect();
    let mut coarse = Builder::default();
    for s in &surfaces {
        s.draw(
            &mut coarse,
            NODE_RS[..3].try_into().unwrap(),
            0,
            false,
            usize::MAX,
        );
    }
    let limit = coarse.mesh.triangles.len() + 1;
    let mut b = Builder::default();
    b.tri([[0.0, 0.0, -1.0], [1.0, 0.0, -1.0], [0.0, 1.0, -1.0]], 999);
    let depth = draw_surfaces(&mut b, &surfaces, limit);
    assert!(depth < SPLIT, "上限で深さが下がる");
    assert!(b.mesh.triangles.len() <= limit);
    assert_eq!(b.mesh.triangle_faces[0], 999);
    for face in 0..4 {
        let area: f64 = b
            .mesh
            .triangles
            .iter()
            .zip(&b.mesh.triangle_faces)
            .filter(|(_, id)| **id / 100 == face)
            .map(|(tri, _)| {
                let p = tri.map(|i| b.mesh.positions[i as usize]);
                0.5 * ((p[1][0] - p[0][0]) as f64 * (p[2][1] - p[0][1]) as f64
                    - (p[1][1] - p[0][1]) as f64 * (p[2][0] - p[0][0]) as f64)
                    .abs()
            })
            .sum();
        assert!((area - 0.5).abs() < 1e-5, "面 {face} の面積 {area}");
    }
}
