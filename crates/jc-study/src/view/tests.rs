use super::*;
use crate::result::{Frame, Part, Quantity};
use jc_core::OverlayShape;

fn plate(values: Vec<f32>) -> SimResult {
    SimResult {
        kind: "static".into(),
        parts: vec![Part {
            name: "表面".into(),
            geom: Geometry::Surface {
                points: vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]],
                tris: vec![[0, 1, 2]],
            },
        }],
        quantities: vec![Quantity::scalar("応力", "MPa", 0)],
        frames: vec![Frame {
            label: String::new(),
            time: None,
            values: vec![Values::exact(values)],
        }],
        ..SimResult::default()
    }
}

#[test]
fn a_triangle_is_cut_at_the_band_borders() {
    let r = plate(vec![0.0, 10.0, 0.0]);
    let look = Look {
        surface: Some(0),
        smooth: false,
        ..Look::default()
    };
    let s = draw(&r, &look, 1);
    // 0〜10 を 10 帯: 帯ごとに 1 片以上
    let mut faces: Vec<u32> = s.mesh.triangle_faces.clone();
    faces.sort();
    faces.dedup();
    assert_eq!(faces.len(), BANDS);
    // 切った小片の面積の和は元の三角形の面積
    let area: f64 = s
        .mesh
        .triangles
        .iter()
        .map(|t| {
            let p = t.map(|i| s.mesh.positions[i as usize].map(f64::from));
            let u = [p[1][0] - p[0][0], p[1][1] - p[0][1]];
            let w = [p[2][0] - p[0][0], p[2][1] - p[0][1]];
            0.5 * (u[0] * w[1] - u[1] * w[0]).abs()
        })
        .sum();
    assert!((area - 50.0).abs() < 1e-6, "{area}");
    assert_eq!(s.legend.len(), BANDS);
    assert_eq!(s.label, "応力(MPa)");
}

#[test]
fn a_safety_factor_above_the_cap_is_all_safe() {
    let mut r = plate(vec![40.0, 41.0, 42.0]);
    r.quantities[0] = Quantity {
        low_is_bad: true,
        cap: Some(10.0),
        ..Quantity::scalar("安全率", "", 0)
    };
    let look = Look {
        surface: Some(0),
        smooth: false,
        ..Look::default()
    };
    let s = draw(&r, &look, 1);
    // 安全な色(青)の帯 1 つに入り、凡例の上は上限の帯の「以上」
    let mut faces = s.mesh.triangle_faces.clone();
    faces.dedup();
    assert_eq!(faces.len(), 1);
    let Some(ShapeInk::Rgb(c)) = s.face_colors[faces[0] as usize] else {
        panic!("色がない")
    };
    assert!(c[2] > c[0], "{c:?}");
    assert_eq!(s.label, "安全率");
    assert!(s.legend[0].1.ends_with("以上"), "{:?}", s.legend[0]);
}

#[test]
fn colors_run_from_blue_to_red() {
    let lo = ramp(0.0);
    let hi = ramp(1.0);
    assert!(lo[2] > lo[0] && hi[0] > hi[2]);
}

#[test]
fn a_flat_triangle_stays_one_piece() {
    let r = plate(vec![3.0, 3.0, 3.0]);
    let look = Look {
        surface: Some(0),
        ..Look::default()
    };
    let s = draw(&r, &look, 1);
    assert_eq!(s.mesh.triangles.len(), 1);
}

/// 2D の格子と円柱(z に長い 8 角柱)を切る。
fn channel() -> SimResult {
    let n = [40, 20, 1];
    let h = 1.0;
    let mut solid = vec![false; n[0] * n[1]];
    let (cx, cy, rad) = (15.0, 10.0, 3.0);
    let mut speed = Vec::new();
    for j in 0..n[1] {
        for i in 0..n[0] {
            let (x, y) = (i as f64 + 0.5, j as f64 + 0.5);
            let inside = (x - cx).powi(2) + (y - cy).powi(2) < rad * rad;
            solid[i + n[0] * j] = inside;
            speed.extend([1.0, 0.0, 0.0]);
        }
    }
    let mut points = Vec::new();
    let mut tris = Vec::new();
    for k in 0..8 {
        let t = k as f64 / 8.0 * std::f64::consts::TAU;
        let (x, y) = (cx + rad * t.cos(), cy + rad * t.sin());
        points.push([x as f32, y as f32, -5.0]);
        points.push([x as f32, y as f32, 5.0]);
    }
    for k in 0..8u32 {
        let (a, b) = (2 * k, 2 * ((k + 1) % 8));
        tris.push([a, b, b + 1]);
        tris.push([a, b + 1, a + 1]);
    }
    SimResult {
        kind: "flow2d".into(),
        parts: vec![
            Part {
                name: "物体".into(),
                geom: Geometry::Surface { points, tris },
            },
            Part {
                name: "流れ".into(),
                geom: Geometry::Grid(Grid {
                    n,
                    origin: [0.0, 0.0, 0.0],
                    h,
                    axes: [0, 1, 2],
                    signs: [1.0; 3],
                    solid,
                    body: Some(0),
                }),
            },
        ],
        quantities: vec![Quantity::vector("速さ", "m/s", 1, Role::Velocity)],
        frames: vec![Frame {
            label: String::new(),
            time: Some(0.1),
            values: vec![Values::packed(&speed)],
        }],
        ..SimResult::default()
    }
}

#[test]
fn the_section_skips_the_body_and_follows_its_outline() {
    let r = channel();
    let look = Look {
        section: Some((0, 2, 0.0)),
        ..Look::default()
    };
    let s = draw(&r, &look, 1);
    let gray = s.face_colors.len() as u32 - 1;
    // 断面の三角形は円柱の中に入らない(本当の外形 = 8 角形の内側の円 r cos(22.5°) より内に点が無い)
    let inner = 3.0 * (std::f64::consts::PI / 8.0).cos() - 1e-6;
    let mut section_area = 0.0;
    for (t, f) in s.mesh.triangles.iter().zip(&s.mesh.triangle_faces) {
        if *f == gray {
            continue;
        }
        let p = t.map(|i| s.mesh.positions[i as usize].map(f64::from));
        let c = [
            (p[0][0] + p[1][0] + p[2][0]) / 3.0,
            (p[0][1] + p[1][1] + p[2][1]) / 3.0,
        ];
        assert!(
            ((c[0] - 15.0).powi(2) + (c[1] - 10.0).powi(2)).sqrt() > inner * 0.95,
            "物体の中に断面: {c:?}"
        );
        let u = [p[1][0] - p[0][0], p[1][1] - p[0][1]];
        let w = [p[2][0] - p[0][0], p[2][1] - p[0][1]];
        section_area += 0.5 * (u[0] * w[1] - u[1] * w[0]).abs();
    }
    // 塗った面積 ≒ 箱(セルの中心の範囲 39×19)− 8 角形
    let oct = 0.5 * 8.0 * 9.0 * (std::f64::consts::TAU / 8.0).sin();
    let want = 39.0 * 19.0 - oct;
    assert!(
        (section_area - want).abs() < 0.01 * want,
        "{section_area} / {want}"
    );
    // 物体(灰色)は描く
    assert!(s.mesh.triangle_faces.contains(&gray));
}

/// 1 辺 1 の立方体を 6 つの 2 次の四面体に分けた網(値は z)。
fn cube() -> SimResult {
    let c: Vec<[f32; 3]> = (0..8)
        .map(|i| [(i & 1) as f32, ((i >> 1) & 1) as f32, ((i >> 2) & 1) as f32])
        .collect();
    let mut points = c.clone();
    let mut mid = std::collections::HashMap::new();
    let mut m = |a: u32, b: u32, points: &mut Vec<[f32; 3]>| -> u32 {
        *mid.entry((a.min(b), a.max(b))).or_insert_with(|| {
            let (p, q) = (points[a as usize], points[b as usize]);
            points.push([0, 1, 2].map(|k| 0.5 * (p[k] + q[k])));
            points.len() as u32 - 1
        })
    };
    // 対角 0–7 のまわりの 6 つ(Kuhn の分け方)
    let corners: [[u32; 4]; 6] = [
        [0, 1, 3, 7],
        [0, 1, 5, 7],
        [0, 2, 3, 7],
        [0, 2, 6, 7],
        [0, 4, 5, 7],
        [0, 4, 6, 7],
    ];
    let mut tets = Vec::new();
    for t in corners {
        let mut e = |a: usize, b: usize, p: &mut Vec<[f32; 3]>| m(t[a], t[b], p);
        let n = [
            e(0, 1, &mut points),
            e(1, 2, &mut points),
            e(0, 2, &mut points),
            e(0, 3, &mut points),
            e(1, 3, &mut points),
            e(2, 3, &mut points),
        ];
        tets.push([t[0], t[1], t[2], t[3], n[0], n[1], n[2], n[3], n[4], n[5]]);
    }
    // 外の面: 立方体の 6 面を 2 つずつ(向きは問わない試験なので、面積だけ見る)
    let mut faces = Vec::new();
    for f in [
        [0, 1, 3, 2],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 3, 7, 6],
        [0, 2, 6, 4],
        [1, 3, 7, 5],
    ] {
        for tri in [[f[0], f[1], f[2]], [f[0], f[2], f[3]]] {
            let n = [
                m(tri[0], tri[1], &mut points),
                m(tri[1], tri[2], &mut points),
                m(tri[2], tri[0], &mut points),
            ];
            faces.push([tri[0], tri[1], tri[2], n[0], n[1], n[2]]);
        }
    }
    let z: Vec<f32> = points.iter().map(|p| p[2]).collect();
    SimResult {
        kind: "static".into(),
        parts: vec![Part {
            name: "ボディ".into(),
            geom: Geometry::Tets(Tets {
                points,
                tets,
                faces,
                face_ids: Vec::new(),
            }),
        }],
        quantities: vec![Quantity::scalar("高さ", "mm", 0)],
        frames: vec![Frame {
            label: String::new(),
            time: None,
            values: vec![Values::exact(z)],
        }],
        ..SimResult::default()
    }
}

/// 半径 1.7・高さ 3 の円柱の横の面(面 0、外向き)と上の面(面 1)の 2 次の三角形。辺の中点は
/// 形の上(横の面の辺と上の面の縁の中点は円柱へ乗せる)。見本の部品の直径 3.4 mm の穴に、要素の大きさ 1 mm
/// ほどの網を張った物と同じくらいの粗さ。横の面の対角は互い違い(網のつなぎ方が不揃い)。
fn cylinder() -> Tets {
    let (r, h, nt, nz) = (1.7f32, 3.0f32, 10usize, 3usize);
    let mut points: Vec<[f32; 3]> = Vec::new();
    let at = |a: f32, z: f32| [r * a.cos(), r * a.sin(), z];
    let ring = |i: usize, j: usize| (j * nt + i % nt) as u32;
    for j in 0..=nz {
        for i in 0..nt {
            // 段ごとに半分ずらして、斜めの辺を作る
            let a = (i as f32 + 0.5 * (j % 2) as f32) / nt as f32 * std::f32::consts::TAU;
            points.push(at(a, h * j as f32 / nz as f32));
        }
    }
    let center = points.len() as u32;
    points.push([0.0, 0.0, h]);
    let mut mid = std::collections::HashMap::new();
    let mut m = |a: u32, b: u32, points: &mut Vec<[f32; 3]>| -> u32 {
        *mid.entry((a.min(b), a.max(b))).or_insert_with(|| {
            let (p, q) = (points[a as usize], points[b as usize]);
            let mut c = [0, 1, 2].map(|k| 0.5 * (p[k] + q[k]));
            // 円の上の 2 点を結ぶ辺(横の面の辺と、上の面の縁)の中点は円柱へ乗せる
            if a != center && b != center {
                let l = (c[0] * c[0] + c[1] * c[1]).sqrt();
                c[0] *= r / l;
                c[1] *= r / l;
            }
            points.push(c);
            points.len() as u32 - 1
        })
    };
    let mut faces = Vec::new();
    let mut face_ids = Vec::new();
    let mut push = |tri: [u32; 3], id: u32, points: &mut Vec<[f32; 3]>| {
        let n = [
            m(tri[0], tri[1], points),
            m(tri[1], tri[2], points),
            m(tri[2], tri[0], points),
        ];
        faces.push([tri[0], tri[1], tri[2], n[0], n[1], n[2]]);
        face_ids.push(id);
    };
    for j in 0..nz {
        for i in 0..nt {
            let (a, b) = (ring(i, j), ring(i + 1, j));
            let (c, d) = (ring(i, j + 1), ring(i + 1, j + 1));
            // 外から見て反時計回り
            if j % 2 == 0 {
                push([a, b, c], 0, &mut points);
                push([b, d, c], 0, &mut points);
            } else {
                push([a, b, d], 0, &mut points);
                push([a, d, c], 0, &mut points);
            }
        }
    }
    for i in 0..nt {
        push([ring(i, nz), ring(i + 1, nz), center], 1, &mut points);
    }
    Tets {
        points,
        tets: Vec::new(),
        faces,
        face_ids,
    }
}

/// 描いた網の点ごとの(位置, 陰の法線)。
fn shaded(b: &Builder) -> Vec<([f32; 3], [f32; 3])> {
    b.mesh
        .positions
        .iter()
        .zip(&b.mesh.normals)
        .map(|(p, n)| (*p, *n))
        .collect()
}

fn deg(a: [f32; 3], b: [f32; 3]) -> f64 {
    let d = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2]) as f64;
    d.clamp(-1.0, 1.0).acos().to_degrees()
}

/// 曲面(円柱の横の面)の陰は、小三角形の境で跳ばず、面の向き(半径の向き)から外れない。
/// 平らな面の陰は面の向きのまま、面の境の縁(90°)は分けたまま。小三角形ごとの平らな
/// 法線で塗ると、同じ点で隣の三角形との差が 18° ほどあり、網目が透けて見える。
#[test]
fn curved_faces_are_shaded_smoothly_and_edges_stay_sharp() {
    let t = cylinder();
    let mut b = Builder::default();
    tets_mesh(&mut b, &t, None, None, None, 0);
    let pts = shaded(&b);
    let key = |p: [f32; 3]| p.map(|x| (x * 1e4).round() as i64);
    let mut by_pos: std::collections::HashMap<[i64; 3], Vec<[f32; 3]>> =
        std::collections::HashMap::new();
    let (mut worst_dev, mut worst_jump) = (0.0f64, 0.0f64);
    for (p, n) in &pts {
        let rr = (p[0] * p[0] + p[1] * p[1]).sqrt();
        let top = (p[2] - 3.0).abs() < 1e-5;
        let on_wall = (rr - 1.7).abs() < 1e-4 && n[2].abs() < 0.5;
        if on_wall {
            let radial = [p[0] / rr, p[1] / rr, 0.0];
            worst_dev = worst_dev.max(deg(*n, radial));
            if !top {
                by_pos.entry(key(*p)).or_default().push(*n);
            }
        } else {
            // 上の面(縁の点も上の面の三角形の物は上向きのまま)
            assert!(top, "横でも上でもない点 {p:?}");
            assert!(deg(*n, [0.0, 0.0, 1.0]) < 1e-3, "上の面の法線 {n:?}");
        }
    }
    for ns in by_pos.values() {
        for a in ns {
            for c in ns {
                worst_jump = worst_jump.max(deg(*a, *c));
            }
        }
    }
    eprintln!("横の面: 半径の向きからのずれ {worst_dev:.2}°・同じ点での差 {worst_jump:.2}°");
    assert!(worst_jump < 0.5, "同じ点での陰の法線の差 {worst_jump}°");
    assert!(worst_dev < 3.0, "半径の向きからのずれ {worst_dev}°");
    // 縁の点: 横の面の三角形は横向き、上の面の三角形は上向き(縁を丸めない)
    let rim_side = pts
        .iter()
        .filter(|(p, n)| (p[2] - 3.0).abs() < 1e-5 && n[2].abs() < 0.1)
        .count();
    assert!(rim_side > 0, "縁で横の面の陰が上の面と混ざった");
}

/// 陰の法線は変形した形で求める(元の形の法線のままだと、傾いた面が傾いて見えない)。
#[test]
fn the_shading_follows_the_deformed_shape() {
    let mut t = cylinder();
    // 上の面だけ使う(z = 3 の円板)。変位は x に比例して上へ(傾き 0.1 × 倍率 2)
    let keep: Vec<usize> = (0..t.faces.len()).filter(|i| t.face_ids[*i] == 1).collect();
    t.faces = keep.iter().map(|i| t.faces[*i]).collect();
    t.face_ids = vec![1; t.faces.len()];
    let u: Vec<f32> = t
        .points
        .iter()
        .flat_map(|p| [0.0, 0.0, 0.1 * p[0]])
        .collect();
    let u = Values::exact(u);
    let mut b = Builder::default();
    tets_mesh(&mut b, &t, Some((&u, 2.0)), None, None, 0);
    let want = {
        let l = (0.2f32 * 0.2 + 1.0).sqrt();
        [-0.2 / l, 0.0, 1.0 / l]
    };
    for (_, n) in shaded(&b) {
        assert!(deg(n, want) < 1e-3, "{n:?} と {want:?}");
    }
}

fn area(s: &OverlayShape, keep: impl Fn([f64; 3]) -> bool) -> f64 {
    s.mesh
        .triangles
        .iter()
        .map(|t| t.map(|i| s.mesh.positions[i as usize].map(f64::from)))
        .filter(|p| keep([0, 1, 2].map(|k| (p[0][k] + p[1][k] + p[2][k]) / 3.0)))
        .map(|p| {
            let u = [0, 1, 2].map(|k| p[1][k] - p[0][k]);
            let w = [0, 1, 2].map(|k| p[2][k] - p[0][k]);
            let c = [
                u[1] * w[2] - u[2] * w[1],
                u[2] * w[0] - u[0] * w[2],
                u[0] * w[1] - u[1] * w[0],
            ];
            0.5 * (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt()
        })
        .sum()
}

#[test]
fn tets_are_painted_on_the_surface_and_cut_by_the_section() {
    let r = cube();
    let look = Look {
        surface: Some(0),
        ..Look::default()
    };
    let s = draw(&r, &look, 1);
    assert!(
        (area(&s, |_| true) - 6.0).abs() < 1e-5,
        "{}",
        area(&s, |_| true)
    );
    // 断面 z = 0.4: 下の面 1 + 横 4 × 0.4 + 切り口 1
    let look = Look {
        surface: Some(0),
        section: Some((0, 2, 0.4)),
        ..Look::default()
    };
    let s = draw(&r, &look, 1);
    let total = area(&s, |_| true);
    assert!((total - (1.0 + 1.6 + 1.0)).abs() < 1e-4, "{total}");
    let cut = area(&s, |c| {
        (c[2] - 0.4).abs() < 1e-4
            && c[0] > 1e-4
            && c[0] < 1.0 - 1e-4
            && c[1] > 1e-4
            && c[1] < 1.0 - 1e-4
    });
    assert!((cut - 1.0).abs() < 1e-4, "切り口 {cut}");
    // 凡例は 1 つ(表面と同じ量)で、断面の位置を見出しに書く
    assert!(s.more_legends.len() <= 1, "{:?}", s.more_legends.len());
    assert!(s.label.contains("断面 Z = 0.400 mm"), "{}", s.label);
}

/// 曲げの応力 σxx = z − 0.37(中立面が節点の間)を、ミーゼス応力で塗る。成分を補間すれば、
/// 塗った帯は正しい値の帯と合う。節点のミーゼス応力を補間すると、中立面の近くで合わない。
#[test]
fn von_mises_follows_the_components_between_the_nodes() {
    let mut r = cube();
    let Geometry::Tets(t) = &r.parts[0].geom else {
        unreachable!()
    };
    let exact = |z: f64| (z - 0.37).abs();
    let tensor: Vec<f32> = t
        .points
        .iter()
        .flat_map(|p| [p[2] - 0.37, 0.0, 0.0, 0.0, 0.0, 0.0])
        .collect();
    let vm: Vec<f32> = t.points.iter().map(|p| exact(p[2] as f64) as f32).collect();
    r.quantities = vec![
        Quantity {
            from: Some((1, Derive::VonMises)),
            ..Quantity::scalar("ミーゼス応力", "MPa", 0)
        },
        Quantity::tensor("応力", "MPa", 0),
    ];
    r.frames[0].values = vec![Values::exact(vm), Values::exact(tensor)];
    let wrong = |r: &SimResult| {
        let look = Look {
            surface: Some(0),
            ..Look::default()
        };
        let s = draw(r, &look, 1);
        let (lo, hi) = r.range(0, Some(0));
        let levels = s.face_colors.len() - 2;
        let band = |v: f64| {
            ((((v - lo as f64) / (hi - lo) as f64).clamp(0.0, 1.0) * levels as f64) as usize)
                .min(levels - 1)
        };
        // 横の面 x = 0 で、塗った帯が重心の正しい値の帯と違う面積
        let mut bad = 0.0;
        for (k, tri) in s.mesh.triangles.iter().enumerate() {
            let p = tri.map(|i| s.mesh.positions[i as usize].map(f64::from));
            if p.iter().any(|q| q[0].abs() > 1e-6) {
                continue;
            }
            let z = (p[0][2] + p[1][2] + p[2][2]) / 3.0;
            if s.mesh.triangle_faces[k] as usize != band(exact(z)) {
                let a = 0.5
                    * ((p[1][1] - p[0][1]) * (p[2][2] - p[0][2])
                        - (p[1][2] - p[0][2]) * (p[2][1] - p[0][1]))
                        .abs();
                bad += a;
            }
        }
        bad
    };
    let good = wrong(&r);
    assert!(good < 0.01, "成分を補間: 違う帯の面積 {good}");
    r.quantities[0].from = None;
    let old = wrong(&r);
    assert!(old > 0.05, "ミーゼス応力を補間: 違う帯の面積 {old}");
}

#[test]
fn streamlines_go_around_the_body() {
    let r = channel();
    let look = Look {
        streamlines: true,
        ..Look::default()
    };
    let Geometry::Grid(g) = &r.parts[1].geom else {
        unreachable!()
    };
    let lines = streamlines(g, &r.frames[0].values[0], None, None);
    assert_eq!(lines.len(), 24);
    assert!(lines.iter().any(|l| l.points.len() > 40));
    let s = draw(&r, &look, 1);
    assert!(!s.more_legends.is_empty() || s.label.starts_with("流線"));
}
