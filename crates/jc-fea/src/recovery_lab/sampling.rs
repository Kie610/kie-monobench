//! 調べ物の道具: 網の読み書き、点を含む要素を探す升目、要素の中の値の補間、絵の色と書き出し、
//! ギザギザの測り方。

use super::*;

pub(super) fn read_mesh(path: &str) -> jc_core::Mesh {
    let s = std::fs::read_to_string(path).unwrap();
    let mut it = s.lines();
    let h: Vec<usize> = it
        .next()
        .unwrap()
        .split_whitespace()
        .map(|x| x.parse().unwrap())
        .collect();
    let mut m = jc_core::Mesh::default();
    for _ in 0..h[0] {
        let v: Vec<f32> = it
            .next()
            .unwrap()
            .split_whitespace()
            .map(|x| x.parse().unwrap())
            .collect();
        m.positions.push([v[0], v[1], v[2]]);
    }
    for _ in 0..h[1] {
        let v: Vec<u32> = it
            .next()
            .unwrap()
            .split_whitespace()
            .map(|x| x.parse().unwrap())
            .collect();
        m.triangles.push([v[0], v[1], v[2]]);
        m.triangle_faces.push(v[3]);
    }
    for _ in 0..h[2] {
        m.face_names.push(it.next().unwrap().to_string());
    }
    m
}

pub(super) struct Solved {
    pub(super) p: Prepared,
    pub(super) u: Vec<f64>,
}

pub(super) fn solve_plate(mesh: &jc_core::Mesh, h: f64) -> Solved {
    let (e, nu) = (69_000.0, 0.33);
    let p = Prepared::new(mesh, h, &|| false).unwrap();
    let m = &p.mesh;
    eprintln!("網 h {h}: 要素 {}・節点 {}", m.tets.len(), m.points.len());
    let mut k = assemble(m, 3, |el| Some(tet10::stiffness(&m.tet_points(el), e, nu)));
    let mut f = load_vector(
        &p,
        &[Load {
            faces: names(&["f8:end"]),
            kind: LoadKind::Force(100.0),
            dir: Dir::Fixed([0.0, 0.0, -1.0]),
        }],
    )
    .unwrap();
    let fixed = fixed_dofs(&p, &names(&["f8:side:e6"])).unwrap();
    apply_fixed(&mut k, &mut f, &fixed);
    let modes = rigid_modes(&m.points, &fixed_mask(k.rows, &fixed));
    let mut u = vec![0.0; f.len()];
    solve(k, &f, &mut u, 3, modes, 1e-10).unwrap();
    Solved { p, u }
}

/// 要素を探す箱の格子。
pub(super) struct Locator {
    pub(super) lo: P3,
    pub(super) cell: f64,
    pub(super) n: [usize; 3],
    pub(super) bins: Vec<Vec<u32>>,
}

impl Locator {
    pub(super) fn new(m: &crate::tetmesh::TetMesh) -> Self {
        let mut lo = [f64::MAX; 3];
        let mut hi = [f64::MIN; 3];
        for p in &m.points {
            for a in 0..3 {
                lo[a] = lo[a].min(p[a]);
                hi[a] = hi[a].max(p[a]);
            }
        }
        let cell = m.stats.h.max(0.3);
        let n = [0, 1, 2].map(|a| ((hi[a] - lo[a]) / cell).ceil() as usize + 1);
        let mut bins = vec![Vec::new(); n[0] * n[1] * n[2]];
        for (e, t) in m.tets.iter().enumerate() {
            let (mut l, mut u) = ([f64::MAX; 3], [f64::MIN; 3]);
            for v in t {
                let q = m.points[*v as usize];
                for a in 0..3 {
                    l[a] = l[a].min(q[a]);
                    u[a] = u[a].max(q[a]);
                }
            }
            let i0 = [0, 1, 2].map(|a| ((l[a] - lo[a]) / cell).floor() as usize);
            let i1 = [0, 1, 2].map(|a| (((u[a] - lo[a]) / cell).floor() as usize).min(n[a] - 1));
            for i in i0[0]..=i1[0] {
                for j in i0[1]..=i1[1] {
                    for k in i0[2]..=i1[2] {
                        bins[(i * n[1] + j) * n[2] + k].push(e as u32);
                    }
                }
            }
        }
        Locator { lo, cell, n, bins }
    }
    pub(super) fn candidates(&self, q: P3) -> &[u32] {
        let i = [0, 1, 2].map(|a| ((q[a] - self.lo[a]) / self.cell).floor());
        if (0..3).any(|a| i[a] < 0.0 || i[a] as usize >= self.n[a]) {
            return &[];
        }
        let i = i.map(|x| x as usize);
        &self.bins[(i[0] * self.n[1] + i[1]) * self.n[2] + i[2]]
    }
}

/// 1 次の四面体の重心座標。
pub(super) fn bary4(p: [P3; 4], q: P3) -> [f64; 4] {
    let d = |a: P3, b: P3| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let m = nalgebra::Matrix3::from_columns(&[
        nalgebra::Vector3::from(d(p[1], p[0])),
        nalgebra::Vector3::from(d(p[2], p[0])),
        nalgebra::Vector3::from(d(p[3], p[0])),
    ]);
    let r = m
        .try_inverse()
        .map(|mi| mi * nalgebra::Vector3::from(d(q, p[0])));
    match r {
        Some(r) => [1.0 - r[0] - r[1] - r[2], r[0], r[1], r[2]],
        None => [-1.0; 4],
    }
}

/// 点 q の値: 表示と同じく、要素を 8 つの 1 次の四面体に分けて節点の値を線形に補間。
pub(super) fn sample_linear(
    m: &crate::tetmesh::TetMesh,
    loc: &Locator,
    vals: &[f64],
    q: P3,
) -> Option<f64> {
    let sub = |t: [u32; 10]| -> [[u32; 4]; 8] {
        [
            [t[0], t[4], t[6], t[7]],
            [t[4], t[1], t[5], t[8]],
            [t[6], t[5], t[2], t[9]],
            [t[7], t[8], t[9], t[3]],
            [t[6], t[8], t[4], t[5]],
            [t[6], t[8], t[5], t[9]],
            [t[6], t[8], t[9], t[7]],
            [t[6], t[8], t[7], t[4]],
        ]
    };
    for e in loc.candidates(q) {
        for s in sub(m.tets[*e as usize]) {
            let b = bary4(s.map(|v| m.points[v as usize]), q);
            if b.iter().all(|x| *x >= -1e-9) {
                return Some((0..4).map(|k| b[k] * vals[s[k] as usize]).sum());
            }
        }
    }
    None
}

/// 点 q の応力の成分を線形に補間してからミーゼス(cols は成分ごとの節点の値)。
pub(super) fn sample_tensor_vm(
    m: &crate::tetmesh::TetMesh,
    loc: &Locator,
    cols: &[Vec<f64>],
    q: P3,
) -> Option<f64> {
    let mut c = [0.0; 6];
    for k in 0..6 {
        c[k] = sample_linear(m, loc, &cols[k], q)?;
    }
    Some(von_mises(&c))
}

/// 点 q の参照の解の応力(要素の中の値、角の重心座標から自然座標へ)。
pub(super) fn sample_exact(s: &Solved, loc: &Locator, q: P3, e_mod: f64, nu: f64) -> Option<S6> {
    let m = &s.p.mesh;
    let mut best: Option<(f64, usize, [f64; 4])> = None;
    for e in loc.candidates(q) {
        let e = *e as usize;
        let b = bary4([0, 1, 2, 3].map(|k| m.points[m.tets[e][k] as usize]), q);
        let worst = b.iter().copied().fold(f64::MAX, f64::min);
        if best.is_none_or(|x| worst > x.0) {
            best = Some((worst, e, b));
        }
    }
    let (w, e, b) = best?;
    if w < -0.05 {
        return None;
    }
    let xi = [b[1].max(0.0), b[2].max(0.0), b[3].max(0.0)];
    let x = m.tet_points(e);
    let ue = element_u(&s.p, &s.u, e);
    Some(tet10::stress_at(&x, e_mod, nu, &ue, xi))
}

pub(super) fn write_png_ppm(path: &str, w: usize, h: usize, px: &[[u8; 3]]) {
    let mut f = std::fs::File::create(path).unwrap();
    write!(f, "P6\n{w} {h}\n255\n").unwrap();
    for p in px {
        f.write_all(p).unwrap();
    }
}

/// 点 q を含む要素と自然座標(角の重心座標から)。
pub(super) fn locate(
    m: &crate::tetmesh::TetMesh,
    loc: &Locator,
    q: P3,
) -> Option<(usize, [f64; 3])> {
    let mut best: Option<(f64, usize, [f64; 4])> = None;
    for e in loc.candidates(q) {
        let e = *e as usize;
        let b = bary4([0, 1, 2, 3].map(|k| m.points[m.tets[e][k] as usize]), q);
        let worst = b.iter().copied().fold(f64::MAX, f64::min);
        if best.is_none_or(|x| worst > x.0) {
            best = Some((worst, e, b));
        }
    }
    let (w, e, b) = best?;
    (w > -0.02).then_some((e, [b[1].max(0.0), b[2].max(0.0), b[3].max(0.0)]))
}

/// 箱(0..l)の構造的な網: 立方体を主対角線で 6 つの四面体に分ける(Freudenthal)。
/// `jitter`: 0 = そのまま、1 = 中の点だけ揺らす、2 = 面の上の点も面に沿って揺らす、
/// 3 = 辺の上の点も辺に沿って揺らす。揺れは h の 0.25 倍。
pub(super) fn structured_box(l: [f64; 3], h: f64, jitter: u32) -> crate::tetmesh::TetMesh {
    use crate::tetmesh::{BFace, TetMesh};
    let n = [0, 1, 2].map(|a| (l[a] / h).round() as usize);
    let d = [0, 1, 2].map(|a| l[a] / n[a] as f64);
    let id = |i: usize, j: usize, k: usize| ((i * (n[1] + 1) + j) * (n[2] + 1) + k) as u32;
    let mut points = Vec::new();
    let mut seed: u64 = 12345;
    let mut rnd = move || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((seed >> 33) as f64 / (1u64 << 31) as f64) - 0.5
    };
    for i in 0..=n[0] {
        for j in 0..=n[1] {
            for k in 0..=n[2] {
                let idx = [i, j, k];
                let on: Vec<bool> = (0..3).map(|a| idx[a] == 0 || idx[a] == n[a]).collect();
                let nb = on.iter().filter(|x| **x).count();
                let mut p = [i as f64 * d[0], j as f64 * d[1], k as f64 * d[2]];
                let move_ok = match nb {
                    0 => jitter >= 1,
                    1 => jitter >= 2,
                    2 => jitter >= 3,
                    _ => false,
                };
                if move_ok {
                    for a in 0..3 {
                        if !on[a] {
                            p[a] += 0.5 * rnd() * d[a];
                        }
                    }
                }
                points.push(p);
            }
        }
    }
    let corners = points.len();
    let perms = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    let mut mids: HashMap<(u32, u32), u32> = HashMap::new();
    let mut tets = Vec::new();
    for i in 0..n[0] {
        for j in 0..n[1] {
            for k in 0..n[2] {
                for pm in perms {
                    let mut c = [i, j, k];
                    let mut v = vec![id(c[0], c[1], c[2])];
                    for a in pm {
                        c[a] += 1;
                        v.push(id(c[0], c[1], c[2]));
                    }
                    let mut t4 = [v[0], v[1], v[2], v[3]];
                    let pt = |x: u32| points[x as usize];
                    let vol = crate::geom::dot(
                        crate::geom::sub(pt(t4[1]), pt(t4[0])),
                        crate::geom::cross(
                            crate::geom::sub(pt(t4[2]), pt(t4[0])),
                            crate::geom::sub(pt(t4[3]), pt(t4[0])),
                        ),
                    );
                    if vol < 0.0 {
                        t4.swap(1, 2);
                    }
                    let mut t = [0u32; 10];
                    t[..4].copy_from_slice(&t4);
                    for (e, ed) in EDGES.iter().enumerate() {
                        let (a, b) = (t4[ed[0]].min(t4[ed[1]]), t4[ed[0]].max(t4[ed[1]]));
                        let m = *mids.entry((a, b)).or_insert_with(|| {
                            let (pa, pb) = (points[a as usize], points[b as usize]);
                            points.push([0, 1, 2].map(|q| 0.5 * (pa[q] + pb[q])));
                            (points.len() - 1) as u32
                        });
                        t[4 + e] = m;
                    }
                    tets.push(t);
                }
            }
        }
    }
    // 外の面
    let mut count: HashMap<[u32; 3], (u32, [u32; 6], u32)> = HashMap::new();
    for (e, t) in tets.iter().enumerate() {
        for (c, mid) in tet10::FACES {
            let nodes = [c[0], c[1], c[2], mid[0], mid[1], mid[2]].map(|q| t[q]);
            let mut key = [nodes[0], nodes[1], nodes[2]];
            key.sort_unstable();
            count
                .entry(key)
                .and_modify(|x| x.0 += 1)
                .or_insert((1, nodes, e as u32));
        }
    }
    let mut faces = Vec::new();
    for (_, (c, nodes, e)) in count {
        if c != 1 {
            continue;
        }
        let ps = [0, 1, 2].map(|q| points[nodes[q] as usize]);
        let face = (0..3)
            .find_map(|a| {
                if ps.iter().all(|p| p[a].abs() < 1e-9) {
                    Some(2 * a as u32)
                } else if ps.iter().all(|p| (p[a] - l[a]).abs() < 1e-9) {
                    Some(2 * a as u32 + 1)
                } else {
                    None
                }
            })
            .unwrap_or(u32::MAX);
        faces.push(BFace {
            nodes,
            elem: e,
            face,
        });
    }
    faces.sort_by_key(|f| f.nodes);
    let mut tm = TetMesh {
        points,
        corners,
        tets,
        faces,
        face_names: ["x0", "x1", "y0", "y1", "z0", "z1"]
            .map(String::from)
            .to_vec(),
        ..Default::default()
    };
    tm.stats.h = h;
    tm
}

/// y 5..35 を 0.1 mm おきに取った値 v の、ギザギザ(4 mm の幅の移動平均からの外れ)と全体の振れ。
pub(super) fn jag(d: f64, v: &[f64]) -> String {
    let n = v.len();
    let mean = v.iter().sum::<f64>() / n as f64;
    let (lo, hi) = v
        .iter()
        .fold((f64::MAX, f64::MIN), |(l, u), x| (l.min(*x), u.max(*x)));
    let (rrms, rmax) = jag_numbers(v);
    format!(
        "縁から {d} mm: 平均 {mean:.3} MPa、振れ(y 5〜35){:.1}%、ギザギザ(y 12〜28、4 mm の移動平均からの外れ)RMS {rrms:.2}% 最大 {rmax:.2}%",
        (hi - lo) / mean * 100.0,
    )
}

/// y 12..28 の所(0.1 mm おきの i 70..230)で、前後 20 点(2 mm)の平均からの外れの RMS と最大
/// (平均に対する %)。
pub(super) fn jag_numbers(v: &[f64]) -> (f64, f64) {
    let n = v.len();
    let mean = v.iter().sum::<f64>() / n as f64;
    let mut r = Vec::new();
    for i in 70..230.min(n - 20) {
        let w = &v[i - 20..=i + 20];
        let m = w.iter().sum::<f64>() / w.len() as f64;
        r.push((v[i] - m).abs());
    }
    let rmax = r.iter().copied().fold(0.0, f64::max);
    let rrms = (r.iter().map(|x| x * x).sum::<f64>() / r.len() as f64).sqrt();
    (rrms / mean * 100.0, rmax / mean * 100.0)
}

/// 円柱(半径 10)の側面の上で、底から d の所の値を周(−75°〜75°、0.5° おき)に沿って測り、
/// 滑らかな式(1・cos θ・sin θ・cos 2θ・sin 2θ)に合わせた残りの RMS と最大(平均に対する %)。
/// (点の数、平均、RMS、最大)。
pub(super) fn ring_jag(
    m: &crate::tetmesh::TetMesh,
    loc: &Locator,
    cols: &[Vec<f64>],
    d: f64,
) -> (usize, f64, f64, f64) {
    let (mut th, mut v) = (Vec::new(), Vec::new());
    for i in 0..=300 {
        let t = (-75.0 + i as f64 * 0.5).to_radians();
        // 網の表面のすぐ内(側面は弦の面なので、半径を二分して探す)
        let at = |rr: f64| [rr * t.cos(), rr * t.sin(), d + 1e-6];
        let (mut lo, mut hi) = (9.5, 10.01);
        for _ in 0..30 {
            let mid = 0.5 * (lo + hi);
            if sample_tensor_vm(m, loc, cols, at(mid)).is_some() {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        if let Some(x) = sample_tensor_vm(m, loc, cols, at(lo - 1e-6)) {
            th.push(t);
            v.push(x);
        }
    }
    let basis = |t: f64| [1.0, t.cos(), t.sin(), (2.0 * t).cos(), (2.0 * t).sin()];
    let a = DMatrix::from_fn(v.len(), 5, |r, c| basis(th[r])[c]);
    let b = DVector::from_vec(v.clone());
    let x = (a.transpose() * &a)
        .lu()
        .solve(&(a.transpose() * &b))
        .unwrap();
    let res = &b - &a * x;
    let mean = v.iter().sum::<f64>() / v.len() as f64;
    let rms = (res.iter().map(|r| r * r).sum::<f64>() / v.len() as f64).sqrt();
    let max = res.iter().fold(0.0f64, |s, r| s.max(r.abs()));
    (v.len(), mean, rms / mean * 100.0, max / mean * 100.0)
}

/// 点 q の応力の 6 成分(表示と同じく 8 つの 1 次の四面体で線形に補間)。
pub(super) fn sample_s6(
    m: &crate::tetmesh::TetMesh,
    loc: &Locator,
    vals: &[S6],
    q: P3,
) -> Option<S6> {
    let sub = |t: [u32; 10]| -> [[u32; 4]; 8] {
        [
            [t[0], t[4], t[6], t[7]],
            [t[4], t[1], t[5], t[8]],
            [t[6], t[5], t[2], t[9]],
            [t[7], t[8], t[9], t[3]],
            [t[6], t[8], t[4], t[5]],
            [t[6], t[8], t[5], t[9]],
            [t[6], t[8], t[9], t[7]],
            [t[6], t[8], t[7], t[4]],
        ]
    };
    for e in loc.candidates(q) {
        for s in sub(m.tets[*e as usize]) {
            let b = bary4(s.map(|v| m.points[v as usize]), q);
            if b.iter().all(|x| *x >= -1e-9) {
                let mut o = [0.0; 6];
                for k in 0..4 {
                    for c in 0..6 {
                        o[c] += b[k] * vals[s[k] as usize][c];
                    }
                }
                return Some(o);
            }
        }
    }
    None
}

/// 点 q の応力の 6 成分を、要素の 2 次の形関数で補間(角の重心座標を自然座標とみる)。
pub(super) fn sample_s6_quad(
    m: &crate::tetmesh::TetMesh,
    loc: &Locator,
    vals: &[S6],
    q: P3,
) -> Option<S6> {
    for e in loc.candidates(q) {
        let t = m.tets[*e as usize];
        let b = bary4([0, 1, 2, 3].map(|k| m.points[t[k] as usize]), q);
        if b.iter().all(|x| *x >= -1e-9) {
            let n = tet10::shape([b[1], b[2], b[3]]);
            let mut o = [0.0; 6];
            for a in 0..10 {
                for c in 0..6 {
                    o[c] += n[a] * vals[t[a] as usize][c];
                }
            }
            return Some(o);
        }
    }
    None
}

/// 40 段の色(青 → 赤)。
pub(super) fn band_color(t: f64) -> [u8; 3] {
    let k = (t.clamp(0.0, 0.99999) * 40.0).floor() / 39.0;
    let hue = (1.0 - k) * 240.0;
    let x = 1.0 - ((hue / 60.0) % 2.0 - 1.0).abs();
    let (r, g, b) = match (hue / 60.0) as u32 {
        0 => (1.0, x, 0.0),
        1 => (x, 1.0, 0.0),
        2 => (0.0, 1.0, x),
        3 => (0.0, x, 1.0),
        _ => (x, 0.0, 1.0),
    };
    [r, g, b].map(|v| (v * 255.0) as u8)
}

/// 誤差の色(段の幅で割った値、±0.5 段で青 / 赤に振り切る)。
pub(super) fn err_color(v: f64) -> [u8; 3] {
    let t = (v / 0.5).clamp(-1.0, 1.0);
    if t >= 0.0 {
        [255, (255.0 * (1.0 - t)) as u8, (255.0 * (1.0 - t)) as u8]
    } else {
        [(255.0 * (1.0 + t)) as u8, (255.0 * (1.0 + t)) as u8, 255]
    }
}
