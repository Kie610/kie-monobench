//! 特異点の候補: 固定した面の縁と、入隅(へこんだ鋭い辺)。そこでは理論上の応力が無限に
//! 上がり、網を細かくするほど値が上がり続ける。値は消さず、印を付けて、離れた所の値を並べる。
//! 印の近くに別の形(印の辺に接しない穴の内側・丸めなどの面)があれば、その形の方が近い節点は
//! 近くに入れない。そこの応力集中は網を細かくすると落ち着く本物の値で、除くと強さを甘く見るため。

use crate::boundary::Prepared;
use crate::geom::{P3, cross, dot, norm, sub};
use std::collections::{HashMap, HashSet};

/// 入隅とみなす、隣り合う面の法線の角(度)。曲面の小さな折れ目(穴の内側など)は含めない。
const REENTRANT_DEG: f64 = 30.0;

#[derive(Debug, Clone, Default)]
pub struct Singular {
    /// 印を付ける辺(節点の対。辺の中点で 2 つに分けてある)
    pub edges: Vec<[u32; 2]>,
    /// 節点ごとの、印の辺から要素 `layers` 層の内で、別の形の鋭い辺より印の辺に近いか
    pub near: Vec<bool>,
    /// 近くとみなす要素の層の数
    pub layers: usize,
    /// 固定した面の縁の辺の数と、入隅の辺の数
    pub fixed: usize,
    pub reentrant: usize,
}

fn unit(v: P3) -> P3 {
    let l = norm(v).max(1e-300);
    [v[0] / l, v[1] / l, v[2] / l]
}

/// `fixed` は固定した表面の三角形の番号。近くは印の節点から四面体を `layers` 回たどった範囲
/// (距離で決めない。網は曲面や薄い所で全体の間隔より細かくなるので、距離では要素の数が合わない)。
pub fn find(p: &Prepared, fixed: &[usize], layers: usize) -> Singular {
    let m = &p.mesh;
    let fixed: HashSet<usize> = fixed.iter().copied().collect();
    // 角の辺 → (表面の三角形, 辺の中点, 向かいの角)
    let mut edges: HashMap<[u32; 2], Vec<(usize, u32, u32)>> = HashMap::new();
    for (i, f) in m.faces.iter().enumerate() {
        let n = f.nodes;
        for (a, b, mid, opp) in [
            (n[0], n[1], n[3], n[2]),
            (n[1], n[2], n[4], n[0]),
            (n[2], n[0], n[5], n[1]),
        ] {
            edges
                .entry(if a < b { [a, b] } else { [b, a] })
                .or_default()
                .push((i, mid, opp));
        }
    }
    let normal = |i: usize| {
        let x = m.face_points(i);
        unit(cross(sub(x[1], x[0]), sub(x[2], x[0])))
    };
    let cos_limit = REENTRANT_DEG.to_radians().cos();
    let mut out = Singular {
        layers,
        ..Singular::default()
    };
    let mut keys: Vec<&[u32; 2]> = edges.keys().collect();
    keys.sort_unstable();
    for k in keys {
        let v = &edges[k];
        let [(f1, mid, _), (f2, _, opp2)] = v.as_slice() else {
            continue;
        };
        let is_fixed = fixed.contains(f1) != fixed.contains(f2);
        let is_sharp =
            m.faces[*f1].face != m.faces[*f2].face && dot(normal(*f1), normal(*f2)) < cos_limit;
        let is_reentrant = !is_fixed && is_sharp && {
            let a = m.points[k[0] as usize];
            dot(normal(*f1), sub(m.points[*opp2 as usize], a)) > 0.0
        };
        if is_fixed || is_reentrant {
            out.fixed += is_fixed as usize;
            out.reentrant += is_reentrant as usize;
            out.edges.push([k[0], *mid]);
            out.edges.push([*mid, k[1]]);
        }
    }
    // 別の形: 印の辺に接しない面(穴の内側・丸め・印から離れた所の面)。箱の側面のように印の辺に
    // 接する面は印の側。鋭い辺の鎖で分けると、丸めや、外形の縁につながる溝・段まで印の側に
    // 入ってしまう
    let on_mark: HashSet<u32> = out.edges.iter().flatten().copied().collect();
    let by_mark: HashSet<u32> = m
        .faces
        .iter()
        .filter(|f| f.nodes.iter().any(|n| on_mark.contains(n)))
        .map(|f| f.face)
        .collect();
    let mut other = HashSet::new();
    for f in m.faces.iter().filter(|f| !by_mark.contains(&f.face)) {
        other.extend(f.nodes);
    }
    let mark_nodes: Vec<u32> = out.edges.iter().flatten().copied().collect();
    let other: Vec<u32> = other.into_iter().collect();
    // 印の節点から四面体を層ごとにたどる。距離は、たどった四面体のいちばん長い辺 × 層の数で抑える
    let mut ring = vec![false; m.points.len()];
    for n in &mark_nodes {
        ring[*n as usize] = true;
    }
    let mut longest: f64 = 0.0;
    for _ in 0..layers {
        let before = ring.clone();
        for t in &m.tets {
            if t.iter().any(|n| before[*n as usize]) {
                for n in t {
                    ring[*n as usize] = true;
                }
                for (i, j) in [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)] {
                    let l = norm(sub(m.points[t[i] as usize], m.points[t[j] as usize]));
                    longest = longest.max(l);
                }
            }
        }
    }
    let reach = longest * layers as f64;
    let to_mark = within(&m.points, &mark_nodes, reach);
    let to_other = within(&m.points, &other, reach);
    out.near = (0..m.points.len())
        .map(|i| ring[i] && to_mark[i] < to_other[i])
        .collect();
    out
}

/// 節点ごとの、`nodes` までのいちばん近い距離。`reach` より遠ければ無限大(辺の節点の間隔は
/// 要素の大きさほどなので、節点までの距離で足りる)。
fn within(points: &[P3], nodes: &[u32], reach: f64) -> Vec<f64> {
    let cell = reach.max(1e-9);
    let key = |p: P3| p.map(|v| (v / cell).floor() as i64);
    let mut grid: HashMap<[i64; 3], Vec<u32>> = HashMap::new();
    let mut seen = HashSet::new();
    for n in nodes {
        if seen.insert(*n) {
            grid.entry(key(points[*n as usize])).or_default().push(*n);
        }
    }
    points
        .iter()
        .map(|p| {
            let k = key(*p);
            let mut d = f64::INFINITY;
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        for n in grid
                            .get(&[k[0] + dx, k[1] + dy, k[2] + dz])
                            .into_iter()
                            .flatten()
                        {
                            let l = norm(sub(points[*n as usize], *p));
                            if l <= reach {
                                d = d.min(l);
                            }
                        }
                    }
                }
            }
            d
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::statics::{Dir, Load, LoadKind, StaticInput, solve_static};
    use crate::validation_tests::box_mesh;
    use kmb_core::Mesh;

    /// L 字の板(xy の L 字を z 0〜10 に押し出す)。入隅は (10, 10) の縦の辺。
    fn l_plate() -> Mesh {
        let poly: [[f32; 2]; 6] = [
            [0., 0.],
            [20., 0.],
            [20., 10.],
            [10., 10.],
            [10., 20.],
            [0., 20.],
        ];
        let mut m = Mesh::default();
        let push = |m: &mut Mesh, q: &[[f32; 3]], tris: &[[u32; 3]], name: &str| {
            let base = m.positions.len() as u32;
            m.positions.extend_from_slice(q);
            let f = m.face_names.len() as u32;
            for t in tris {
                m.triangles.push(t.map(|i| base + i));
                m.triangle_faces.push(f);
            }
            m.face_names.push(name.into());
        };
        for k in 0..6 {
            let (p, q) = (poly[k], poly[(k + 1) % 6]);
            let quad = [
                [p[0], p[1], 0.],
                [q[0], q[1], 0.],
                [q[0], q[1], 10.],
                [p[0], p[1], 10.],
            ];
            push(&mut m, &quad, &[[0, 1, 2], [0, 2, 3]], &format!("s{k}"));
        }
        let cap = |z: f32| poly.map(|p| [p[0], p[1], z]);
        // 上は +z から見て反時計回り、下は逆
        let top = [[0, 1, 2], [0, 2, 3], [0, 3, 5], [3, 4, 5]];
        push(&mut m, &cap(10.), &top, "top");
        let bottom: Vec<[u32; 3]> = top.iter().map(|t| [t[0], t[2], t[1]]).collect();
        push(&mut m, &cap(0.), &bottom, "bottom");
        m
    }

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_clamped_edges_are_marked_and_the_peak_sits_there() {
        let p = Prepared::new(&box_mesh([100.0, 10.0, 10.0]), 2.5, &|| false).unwrap();
        let input = StaticInput {
            e: 70_000.0,
            nu: 0.3,
            fixed: names(&["x0"]),
            loads: vec![Load {
                faces: names(&["x1"]),
                kind: LoadKind::Force(100.0),
                dir: Dir::Fixed([0.0, 0.0, -1.0]),
            }],
        };
        let out = solve_static(&p, &input).unwrap();
        let fixed = p.faces_of(&input.fixed).unwrap();
        let s = find(&p, &fixed, 2);
        assert!(
            s.fixed >= 4 && s.reentrant == 0,
            "{} {}",
            s.fixed,
            s.reentrant
        );
        for e in &s.edges {
            for n in e {
                assert!(p.mesh.points[*n as usize][0].abs() < 1e-9);
            }
        }
        // 最大の応力は印の近く。離れた所の最大は梁の理論(根元から 5 mm で P (L − x) c / I = 57 MPa)に近い
        let (mut near_max, mut away) = (0.0f64, 0.0f64);
        for (v, n) in out.node_vm.iter().zip(&s.near) {
            if *n {
                near_max = near_max.max(*v);
            } else {
                away = away.max(*v);
            }
        }
        eprintln!("印の近く {near_max:.2} MPa・離れた所 {away:.2} MPa");
        assert!(near_max > away);
        assert!((away - 57.0).abs() / 57.0 < 0.03, "{away}");
    }

    #[test]
    fn the_inner_corner_of_an_l_plate_is_marked() {
        let p = Prepared::new(&l_plate(), 2.0, &|| false).unwrap();
        let s = find(&p, &[], 2);
        assert!(
            s.reentrant >= 3 && s.fixed == 0,
            "{} {}",
            s.reentrant,
            s.fixed
        );
        for e in &s.edges {
            for n in e {
                let x = p.mesh.points[*n as usize];
                assert!(
                    (x[0] - 10.0).abs() < 1e-6 && (x[1] - 10.0).abs() < 1e-6,
                    "{x:?}"
                );
            }
        }
    }

    /// 見本の部品の板の端を固定すると、固定した面から約 2 mm の所に小さな穴(中心 (56, 36)、
    /// 半径 1.7 mm)がある。その縁の応力集中は本物の最大なので、印の近くに入れない(入れると
    /// 「特異点を除いた最大」が穴の値でなく 5.5 MPa ほどに下がり、強さを甘く見る)。
    #[test]
    fn a_hole_next_to_the_clamped_face_is_not_taken_as_singular() {
        let p = Prepared::new(&crate::surfmesh::tests::sample_part(), 2.5, &|| false).unwrap();
        let input = StaticInput {
            e: 69_000.0,
            nu: 0.33,
            fixed: names(&["f8:side:e6"]),
            loads: vec![Load {
                faces: names(&["f8:end"]),
                kind: LoadKind::Force(100.0),
                dir: Dir::Fixed([0.0, 0.0, -1.0]),
            }],
        };
        let out = solve_static(&p, &input).unwrap();
        let fixed = p.faces_of(&input.fixed).unwrap();
        let s = find(&p, &fixed, 2);
        let m = &p.mesh;
        let on_rim = |x: P3| {
            let r = ((x[0] - 56.0).powi(2) + (x[1] - 36.0).powi(2)).sqrt();
            (r - 1.7).abs() < 0.1
        };
        let rim: Vec<usize> = (0..m.points.len())
            .filter(|i| on_rim(m.points[*i]))
            .collect();
        assert!(rim.len() > 20, "穴の縁の節点 {}", rim.len());
        for i in &rim {
            assert!(!s.near[*i], "穴の縁 {:?} が印の近く", m.points[*i]);
        }
        // 固定した面の縁の上の節点は、穴のそばでも印の近く
        for e in &s.edges {
            for n in e {
                assert!(s.near[*n as usize]);
            }
        }
        let (mut all, mut away, mut at) = (0.0f64, 0.0f64, 0);
        for (i, (v, n)) in out.node_vm.iter().zip(&s.near).enumerate() {
            all = all.max(*v);
            if !*n && *v > away {
                away = *v;
                at = i;
            }
        }
        eprintln!(
            "全体の最大 {all:.3} MPa・離れた所の最大 {away:.3} MPa @{:?}",
            m.points[at]
        );
        assert!(
            on_rim(m.points[at]),
            "離れた所の最大が穴の縁にない {:?}",
            m.points[at]
        );
    }

    /// 内角を丸めた L 字の金具(xy の L 字を z 0〜6 に押し出す)。短い脚の端 y = 8 が固定、
    /// 長い脚の端 x = 30 に荷重。丸め(半径 2)は固定した面から 2〜4 mm。
    fn l_bracket() -> Mesh {
        let (t, r, l1, l2, w, arc) = (4.0f32, 2.0f32, 30.0f32, 8.0f32, 6.0f32, 12);
        let mut poly: Vec<[f32; 2]> = vec![[0., 0.], [l1, 0.], [l1, t]];
        for k in 0..=arc {
            let phi = std::f32::consts::FRAC_PI_2 * k as f32 / arc as f32;
            poly.push([t + r - r * phi.sin(), t + r - r * phi.cos()]);
        }
        poly.extend([[t, l2], [0., l2]]);
        let n = poly.len();
        let side = |k: usize| match k {
            1 => "load".to_string(),
            k if (3..3 + arc).contains(&k) => "fillet".to_string(),
            k if k == n - 2 => "clamp".to_string(),
            k => format!("s{k}"),
        };
        let mut m = Mesh::default();
        for k in 0..n {
            let (p, q) = (poly[k], poly[(k + 1) % n]);
            let name = side(k);
            let f = match m.face_names.iter().position(|x| *x == name) {
                Some(i) => i as u32,
                None => {
                    m.face_names.push(name);
                    m.face_names.len() as u32 - 1
                }
            };
            let base = m.positions.len() as u32;
            m.positions.extend_from_slice(&[
                [p[0], p[1], 0.],
                [q[0], q[1], 0.],
                [q[0], q[1], w],
                [p[0], p[1], w],
            ]);
            for tri in [[0, 1, 2], [0, 2, 3]] {
                m.triangles.push(tri.map(|i| base + i));
                m.triangle_faces.push(f);
            }
        }
        for (z, name, up) in [(w, "top", true), (0.0, "bottom", false)] {
            let base = m.positions.len() as u32;
            m.positions.extend(poly.iter().map(|p| [p[0], p[1], z]));
            let f = m.face_names.len() as u32;
            m.face_names.push(name.into());
            for k in 1..n as u32 - 1 {
                m.triangles.push(if up {
                    [base, base + k, base + k + 1]
                } else {
                    [base, base + k + 1, base + k]
                });
                m.triangle_faces.push(f);
            }
        }
        m
    }

    /// 丸めの応力集中は網を細かくすると落ち着く本物の値(h 2.5 で 212 MPa、h 1.25 で 220 MPa)。
    /// 固定した面から 2〜4 mm にあっても「特異点を除いた最大」から外さない。近くを
    /// 2 × 全体の間隔(5 mm)の距離で決めると、丸めの節点がすべて近くに入り、144 MPa と低く出る。
    #[test]
    fn a_fillet_near_the_clamp_stays_in_the_away_maximum() {
        let p = Prepared::new(&l_bracket(), 2.5, &|| false).unwrap();
        let input = StaticInput {
            e: 70_000.0,
            nu: 0.3,
            fixed: names(&["clamp"]),
            loads: vec![Load {
                faces: names(&["load"]),
                kind: LoadKind::Force(100.0),
                dir: Dir::Fixed([0.0, -1.0, 0.0]),
            }],
        };
        let out = solve_static(&p, &input).unwrap();
        let fixed = p.faces_of(&input.fixed).unwrap();
        let s = find(&p, &fixed, 2);
        let m = &p.mesh;
        let fi = m.face_names.iter().position(|x| x == "fillet").unwrap() as u32;
        let mut fillet = 0.0f64;
        let mut away = 0.0f64;
        for f in m.faces.iter().filter(|f| f.face == fi) {
            for n in f.nodes {
                fillet = fillet.max(out.node_vm[n as usize]);
            }
        }
        for (v, n) in out.node_vm.iter().zip(&s.near) {
            if !*n {
                away = away.max(*v);
            }
        }
        eprintln!("丸めの最大 {fillet:.1} MPa・特異点を除いた最大 {away:.1} MPa");
        assert!(
            away >= 0.99 * fillet,
            "丸め {fillet} が除かれた(除いた最大 {away})"
        );
    }
}
