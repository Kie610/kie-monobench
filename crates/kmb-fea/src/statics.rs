//! 線形静解析(応力とたわみ)。単位は mm・N・MPa(N/mm²)。

use crate::amg::rigid_modes;
use crate::boundary::Prepared;
use crate::geom::{P3, cross, dot, norm, scale, sub};
use crate::solve::{apply_fixed, assemble, solve};
use crate::tet10::{self, NODE_XI, von_mises};
use crate::tetmesh::TetMesh;
use std::collections::HashMap;

/// 荷重の向き。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Dir {
    /// 面を押す向き(面の外向きの法線の逆)
    Normal,
    /// 決まった向き(単位ベクトル)
    Fixed(P3),
}

/// 荷重の種類。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LoadKind {
    /// 面全体にかかる力の合計(N)
    Force(f64),
    /// 圧力(MPa)。面を押す向き
    Pressure(f64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Load {
    pub faces: Vec<String>,
    pub kind: LoadKind,
    pub dir: Dir,
}

/// 線形静解析の条件。
#[derive(Debug, Clone, PartialEq)]
pub struct StaticInput {
    /// 縦弾性係数(MPa)
    pub e: f64,
    pub nu: f64,
    pub fixed: Vec<String>,
    pub loads: Vec<Load>,
}

/// 線形静解析の結果。
#[derive(Debug, Clone)]
pub struct StaticOutput {
    /// 節点ごとの変位(mm)
    pub u: Vec<P3>,
    /// 節点ごとのミーゼス応力(節点の応力から、MPa)
    pub node_vm: Vec<f64>,
    /// 節点ごとの応力(Voigt の順 xx, yy, zz, xy, yz, zx。角はまわりの要素の平均、辺の中点は両端の角の
    /// 平均に、両端の勾配から求めた曲がりを足した物)
    pub node_stress: Vec<[f64; 6]>,
    /// 要素ごとの最大のミーゼス応力(節点の値の最大、MPa)
    pub elem_vm: Vec<f64>,
    pub iterations: usize,
}

impl StaticOutput {
    pub fn max_vm(&self) -> f64 {
        self.node_vm.iter().copied().fold(0.0, f64::max)
    }
    pub fn max_disp(&self) -> f64 {
        self.u
            .iter()
            .map(|u| (u[0] * u[0] + u[1] * u[1] + u[2] * u[2]).sqrt())
            .fold(0.0, f64::max)
    }
}

/// 荷重を節点の力にする(節点ごとに 3 つ)。力の合計は網の面積で割って面に一様に配る。
pub fn load_vector(p: &Prepared, loads: &[Load]) -> Result<Vec<f64>, String> {
    let mut f = vec![0.0; p.mesh.points.len() * 3];
    for l in loads {
        let faces = p.faces_of(&l.faces)?;
        let per = match l.kind {
            LoadKind::Force(v) => v / p.area(&faces).max(1e-30),
            LoadKind::Pressure(pr) => pr,
        };
        match (l.kind, l.dir) {
            (LoadKind::Pressure(_), _) | (_, Dir::Normal) => {
                p.add_traction(&faces, |n| scale(n, -per), &mut f)
            }
            (_, Dir::Fixed(v)) => p.add_traction(&faces, |_| scale(v, per), &mut f),
        }
    }
    Ok(f)
}

/// 固定する節点の未知数(値は 0)。
pub fn fixed_dofs(p: &Prepared, faces: &[String]) -> Result<Vec<(usize, f64)>, String> {
    if faces.is_empty() {
        return Err("固定する面を選んでください".into());
    }
    let fs = p.faces_of(faces)?;
    Ok(p.nodes_of(&fs)
        .into_iter()
        .flat_map(|n| (0..3).map(move |k| (n as usize * 3 + k, 0.0)))
        .collect())
}

/// 要素の変位(30 個)。
pub fn element_u(p: &Prepared, u: &[f64], e: usize) -> [f64; 30] {
    let mut ue = [0.0; 30];
    for (a, n) in p.mesh.tets[e].iter().enumerate() {
        for k in 0..3 {
            ue[a * 3 + k] = u[*n as usize * 3 + k];
        }
    }
    ue
}

/// 要素の角の応力から、節点の応力とミーゼス応力・要素の最大のミーゼス応力。角の節点は、
/// まわりの要素の角の値の平均。辺の中点は両端の角の平均に、辺に沿う
/// 曲がりを足す(`mid_bend`)。辺の中点まで要素ごとの値で平均すると、要素の形の不揃いがそのまま
/// 値の上下になるため。曲面の上の辺の中点は、両端の角の値を中点の向きへ回してから平均する
/// (`surface_mid_turns`)。
pub fn stresses(
    p: &Prepared,
    nu: f64,
    u: &[f64],
    e_of: impl Fn(usize) -> f64,
) -> (Vec<[f64; 6]>, Vec<f64>, Vec<f64>) {
    let m = &p.mesh;
    let n = m.points.len();
    let mut sum = vec![[0.0f64; 6]; n];
    let mut cnt = vec![0u32; n];
    let mut elem_vm = vec![0.0; m.tets.len()];
    // 角ごとの応力の勾配(まわりの要素の勾配を体積で重みを付けて足す)と重み
    let mut grad = vec![[[0.0f64; 3]; 6]; m.corners];
    let mut weight = vec![0.0f64; m.corners];
    for (e, evm) in elem_vm.iter_mut().enumerate() {
        let x = m.tet_points(e);
        let ue = element_u(p, u, e);
        let mut corner = [[0.0; 6]; 4];
        for (a, xi) in NODE_XI.iter().enumerate() {
            let s = tet10::stress_at(&x, e_of(e), nu, &ue, *xi);
            *evm = f64::max(*evm, von_mises(&s));
            if a < 4 {
                let v = m.tets[e][a] as usize;
                for k in 0..6 {
                    sum[v][k] += s[k];
                }
                cnt[v] += 1;
                corner[a] = s;
            }
        }
        if let Some((g, vol)) = corner_gradient(&x, &corner) {
            for a in 0..4 {
                let v = m.tets[e][a] as usize;
                for k in 0..6 {
                    for d in 0..3 {
                        grad[v][k][d] += vol * g[k][d];
                    }
                }
                weight[v] += vol;
            }
        }
    }
    for (g, w) in grad.iter_mut().zip(&weight) {
        if *w > 0.0 {
            *g = g.map(|r| r.map(|v| v / w));
        }
    }
    let mut node_stress: Vec<[f64; 6]> = sum
        .iter()
        .zip(&cnt)
        .map(|(s, c)| s.map(|v| v / (*c).max(1) as f64))
        .collect();
    let mut bend = vec![[0.0; 6]; n];
    for t in &m.tets {
        for (k, [a, b]) in tet10::EDGES.iter().enumerate() {
            let (a, b, mid) = (t[*a] as usize, t[*b] as usize, t[4 + k] as usize);
            let (sa, sb) = (node_stress[a], node_stress[b]);
            bend[mid] = mid_bend(
                [m.points[a], m.points[b], m.points[mid]],
                &grad[a],
                &grad[b],
            );
            node_stress[mid] = std::array::from_fn(|i| (sa[i] + sb[i]) / 2.0 + bend[mid][i]);
        }
    }
    // 曲面の上の辺の中点は、両端の角の値を中点の向きへ回してから平均する
    for (mid, a, b, ra, rb) in surface_mid_turns(m) {
        let sa = turn(&node_stress[a as usize], &ra);
        let sb = turn(&node_stress[b as usize], &rb);
        let bd = bend[mid as usize];
        node_stress[mid as usize] = std::array::from_fn(|i| (sa[i] + sb[i]) / 2.0 + bd[i]);
    }
    let node_vm = node_stress.iter().map(|s| von_mises(s)).collect();
    (node_stress, node_vm, elem_vm)
}

/// 要素の角の 4 つの応力を 1 次とみた時の勾配(成分ごと、[成分][軸])と体積。潰れた要素は None。
fn corner_gradient(x: &[P3; 10], s: &[[f64; 6]; 4]) -> Option<([[f64; 3]; 6], f64)> {
    let rows = [1, 2, 3].map(|k| sub(x[k], x[0]));
    let mm = nalgebra::Matrix3::from_fn(|i, j| rows[i][j]);
    let vol = mm.determinant().abs() / 6.0;
    let inv = mm.try_inverse().filter(|_| vol > 0.0)?;
    let g = std::array::from_fn(|c| {
        let d = nalgebra::Vector3::new(s[1][c] - s[0][c], s[2][c] - s[0][c], s[3][c] - s[0][c]);
        let v = inv * d;
        [v[0], v[1], v[2]]
    });
    Some((g, vol))
}

/// 辺の中点の値に足す曲がり: 両端の値の平均からの、3 次の Hermite の中点の式
/// f(½) = (f₀ + f₁)/2 + (f′₀ − f′₁)/8 のずれ(勾配は角の値 `grad`)と、中点がまっすぐの位置から
/// 外れた分(曲面へ乗せた中点)の勾配による寄り。両端の平均だけでは、応力が辺に沿って曲がる所
/// (厚肉の円筒の 1/r² など)で中点の値が辺の向きによって上下し、色の帯の境が要素の大きさで
/// 波打った(半径の向きの辺で +、周の向きで −、段の幅の 0.5 倍)。
fn mid_bend(x: [P3; 3], ga: &[[f64; 3]; 6], gb: &[[f64; 3]; 6]) -> [f64; 6] {
    let [xa, xb, xm] = x;
    let ed = sub(xb, xa);
    let off = sub(
        xm,
        scale([xa[0] + xb[0], xa[1] + xb[1], xa[2] + xb[2]], 0.5),
    );
    std::array::from_fn(|c| {
        let dg = sub(ga[c], gb[c]);
        let gm = scale(
            [
                ga[c][0] + gb[c][0],
                ga[c][1] + gb[c][1],
                ga[c][2] + gb[c][2],
            ],
            0.5,
        );
        dot(dg, ed) / 8.0 + dot(gm, off)
    })
}

pub(crate) type M3 = [[f64; 3]; 3];

/// 単位ベクトル u を v へ回す、いちばん小さい回転(Rodrigues)。向きがほぼ逆なら回さない。
fn rotation(u: P3, v: P3) -> M3 {
    let k = cross(u, v);
    let c = dot(u, v);
    let mut r = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    if c <= -0.5 {
        return r;
    }
    let kx = [[0.0, -k[2], k[1]], [k[2], 0.0, -k[0]], [-k[1], k[0], 0.0]];
    for i in 0..3 {
        for j in 0..3 {
            let k2: f64 = (0..3).map(|l| kx[i][l] * kx[l][j]).sum();
            r[i][j] += kx[i][j] + k2 / (1.0 + c);
        }
    }
    r
}

/// 応力(Voigt の順)を R σ Rᵀ に回す。
pub(crate) fn turn(s: &[f64; 6], r: &M3) -> [f64; 6] {
    let sm = [[s[0], s[3], s[5]], [s[3], s[1], s[4]], [s[5], s[4], s[2]]];
    let mut rs = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            rs[i][j] = (0..3).map(|k| r[i][k] * sm[k][j]).sum();
        }
    }
    let o = |i: usize, j: usize| -> f64 { (0..3).map(|k| rs[i][k] * r[j][k]).sum() };
    [o(0, 0), o(1, 1), o(2, 2), o(0, 1), o(1, 2), o(0, 2)]
}

/// 表面の辺の中点ごとに、両端の角の応力を中点の向きへ回す回転(中点・角 a・角 b・a の回転・b の
/// 回転)。曲面では、向きの違う 2 つの応力をそのまま平均すると、面に沿う応力に面に垂直な応力が
/// 混ざる(厚肉の円筒の内の面で周の応力が 1% 小さく出た)。1 つの面の中の辺は面の法線の向きの差
/// だけ、面の境の辺は辺の向きの差だけ回す。平らな面とまっすぐな辺では回さない。
pub(crate) fn surface_mid_turns(m: &TetMesh) -> Vec<(u32, u32, u32, M3, M3)> {
    const SUBS: [[usize; 3]; 4] = [[0, 3, 5], [3, 1, 4], [5, 4, 2], [3, 4, 5]];
    const SIDES: [(usize, usize, usize); 3] = [(0, 1, 3), (1, 2, 4), (2, 0, 5)];
    // 節点と面ごとの法線(辺の中点で 4 つに分けた三角形の面積の重み)
    let mut nrm: HashMap<(u32, u32), P3> = HashMap::new();
    // 辺の中点 → 両端と、乗っている面
    let mut mids: HashMap<u32, (u32, u32, Vec<u32>)> = HashMap::new();
    for f in &m.faces {
        let x = f.nodes.map(|v| m.points[v as usize]);
        for tri in SUBS {
            let n = cross(sub(x[tri[1]], x[tri[0]]), sub(x[tri[2]], x[tri[0]]));
            for i in tri {
                let e = nrm.entry((f.nodes[i], f.face)).or_insert([0.0; 3]);
                for a in 0..3 {
                    e[a] += n[a];
                }
            }
        }
        for (a, b, md) in SIDES {
            let e = mids
                .entry(f.nodes[md])
                .or_insert((f.nodes[a], f.nodes[b], Vec::new()));
            if !e.2.contains(&f.face) {
                e.2.push(f.face);
            }
        }
    }
    let unit = |v: P3| -> Option<P3> {
        let l = norm(v);
        (l > 0.0).then(|| scale(v, 1.0 / l))
    };
    // 回す角がこれより大きい所(鋭く曲がった所)は回さない
    let max_cos = 40f64.to_radians().cos();
    let mut out = Vec::new();
    let mut keys: Vec<&u32> = mids.keys().collect();
    keys.sort_unstable();
    for &mid in keys {
        let (a, b, faces) = &mids[&mid];
        let pair = match faces.as_slice() {
            [f] => {
                let n = |v: u32| nrm.get(&(v, *f)).copied().and_then(unit);
                let (Some(na), Some(nb), Some(nm)) = (n(*a), n(*b), n(mid)) else {
                    continue;
                };
                (na, nm, nb, nm)
            }
            [_, _] => {
                let p = |v: u32| m.points[v as usize];
                let (Some(u1), Some(u2)) = (unit(sub(p(mid), p(*a))), unit(sub(p(*b), p(mid))))
                else {
                    continue;
                };
                (u1, u2, u2, u1)
            }
            _ => continue,
        };
        if dot(pair.0, pair.1) < max_cos || dot(pair.2, pair.3) < max_cos {
            continue;
        }
        out.push((
            mid,
            *a,
            *b,
            rotation(pair.0, pair.1),
            rotation(pair.2, pair.3),
        ));
    }
    out
}

/// 固定した未知数の印。
pub fn fixed_mask(n: usize, fixed: &[(usize, f64)]) -> Vec<bool> {
    let mut m = vec![false; n];
    for (d, _) in fixed {
        m[*d] = true;
    }
    m
}

/// 線形静解析を解く。
pub fn solve_static(p: &Prepared, input: &StaticInput) -> Result<StaticOutput, String> {
    if input.loads.is_empty() {
        return Err("荷重を付けてください(面と力か圧力)".into());
    }
    let m = &p.mesh;
    let mut a = assemble(m, 3, |e| {
        Some(tet10::stiffness(&m.tet_points(e), input.e, input.nu))
    });
    let mut b = load_vector(p, &input.loads)?;
    let fixed = fixed_dofs(p, &input.fixed)?;
    apply_fixed(&mut a, &mut b, &fixed);
    let modes = rigid_modes(&m.points, &fixed_mask(a.rows, &fixed));
    let mut x = vec![0.0; b.len()];
    let (rep, _) = solve(a, &b, &mut x, 3, modes, 1e-8)?;
    let (node_stress, node_vm, elem_vm) = stresses(p, input.nu, &x, |_| input.e);
    let u = x.as_chunks::<3>().0.to_vec();
    Ok(StaticOutput {
        u,
        node_vm,
        node_stress,
        elem_vm,
        iterations: rep.iterations,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 円筒の周と半径の応力を、z 軸のまわりの回転で別の角度へ回すと、その角度の応力になる。
    #[test]
    fn a_turned_stress_follows_the_cylinder() {
        let (sr, st, sz) = (-10.0, 16.0, 2.0);
        let at = |t: f64| -> [f64; 6] {
            let (c, s) = (t.cos(), t.sin());
            [
                sr * c * c + st * s * s,
                sr * s * s + st * c * c,
                sz,
                (sr - st) * c * s,
                0.0,
                0.0,
            ]
        };
        let (t0, t1) = (0.3_f64, 0.5_f64);
        let r = rotation([t0.cos(), t0.sin(), 0.0], [t1.cos(), t1.sin(), 0.0]);
        let got = turn(&at(t0), &r);
        let want = at(t1);
        for i in 0..6 {
            assert!((got[i] - want[i]).abs() < 1e-9, "{got:?} と {want:?}");
        }
    }
}
