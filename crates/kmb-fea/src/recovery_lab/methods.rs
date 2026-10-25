//! 節点の応力の作り方の候補(平均・体積の重み・角だけ・SPR・L2 の射影・Hermite・曲がりを足す SPR)。

use super::*;

/// 要素の節点の値を数で平均(辺の中点も要素ごとの値で平均する。比べる元)。
pub(super) fn r_avg(p: &Prepared, en: &[[S6; 10]]) -> Vec<S6> {
    let m = &p.mesh;
    let mut sum = vec![[0.0; 6]; m.points.len()];
    let mut c = vec![0.0f64; m.points.len()];
    for (e, t) in m.tets.iter().enumerate() {
        for a in 0..10 {
            let v = t[a] as usize;
            for k in 0..6 {
                sum[v][k] += en[e][a][k];
            }
            c[v] += 1.0;
        }
    }
    sum.iter()
        .zip(&c)
        .map(|(s, c)| s.map(|x| x / c.max(1.0)))
        .collect()
}

/// 体積で重みを付けた平均。
pub(super) fn r_vol(p: &Prepared, en: &[[S6; 10]]) -> Vec<S6> {
    let m = &p.mesh;
    let mut sum = vec![[0.0; 6]; m.points.len()];
    let mut c = vec![0.0f64; m.points.len()];
    for (e, t) in m.tets.iter().enumerate() {
        let w = tet10::volume(&m.tet_points(e));
        for a in 0..10 {
            let v = t[a] as usize;
            for k in 0..6 {
                sum[v][k] += w * en[e][a][k];
            }
            c[v] += w;
        }
    }
    sum.iter()
        .zip(&c)
        .map(|(s, c)| s.map(|x| x / c.max(1e-300)))
        .collect()
}

/// 角だけ平均し、辺の中点は両端の角の平均。
pub(super) fn r_corner(p: &Prepared, en: &[[S6; 10]]) -> Vec<S6> {
    let m = &p.mesh;
    let mut s = r_avg(p, en);
    for t in &m.tets {
        for (k, ed) in EDGES.iter().enumerate() {
            let (a, b) = (t[ed[0]] as usize, t[ed[1]] as usize);
            let v = t[4 + k] as usize;
            for c in 0..6 {
                s[v][c] = 0.5 * (s[a][c] + s[b][c]);
            }
        }
    }
    s
}

/// 積分点の位置と応力。
pub(super) fn gauss_samples(p: &Prepared, e_mod: f64, nu: f64, u: &[f64]) -> Vec<Vec<(P3, S6)>> {
    let m = &p.mesh;
    (0..m.tets.len())
        .map(|e| {
            let x = m.tet_points(e);
            let ue = element_u(p, u, e);
            gauss4()
                .iter()
                .map(|(xi, _)| {
                    let n = tet10::shape(*xi);
                    let mut q = [0.0; 3];
                    for a in 0..10 {
                        for k in 0..3 {
                            q[k] += n[a] * x[a][k];
                        }
                    }
                    (q, tet10::stress_at(&x, e_mod, nu, &ue, *xi))
                })
                .collect()
        })
        .collect()
}

pub(super) fn basis(d: [f64; 3], quad: bool) -> Vec<f64> {
    let mut b = vec![1.0, d[0], d[1], d[2]];
    if quad {
        b.extend([
            d[0] * d[0],
            d[1] * d[1],
            d[2] * d[2],
            d[0] * d[1],
            d[1] * d[2],
            d[2] * d[0],
        ]);
    }
    b
}

/// SPR(Zienkiewicz・Zhu 1992): 角のまわりの要素の積分点の応力に多項式を最小二乗で合わせる。
pub(super) fn r_spr(p: &Prepared, gs: &[Vec<(P3, S6)>], quad: bool) -> Vec<S6> {
    let m = &p.mesh;
    let h = p.h();
    let mut around = vec![Vec::new(); m.corners];
    for (e, t) in m.tets.iter().enumerate() {
        for a in 0..4 {
            around[t[a] as usize].push(e);
        }
    }
    // 角ごとの多項式の係数
    let fits: Vec<Option<(P3, bool, DMatrix<f64>)>> = (0..m.corners)
        .map(|n| {
            let o = m.points[n];
            let pts: Vec<&(P3, S6)> = around[n].iter().flat_map(|e| gs[*e].iter()).collect();
            let q = quad && pts.len() >= 30;
            let nb = if q { 10 } else { 4 };
            if pts.len() < nb {
                return None;
            }
            let mut a = DMatrix::<f64>::zeros(nb, nb);
            let mut b = DMatrix::<f64>::zeros(nb, 6);
            for (x, s) in pts {
                let d = [(x[0] - o[0]) / h, (x[1] - o[1]) / h, (x[2] - o[2]) / h];
                let v = DVector::from_vec(basis(d, q));
                a += &v * v.transpose();
                for k in 0..6 {
                    for i in 0..nb {
                        b[(i, k)] += v[i] * s[k];
                    }
                }
            }
            a.lu().solve(&b).map(|c| (o, q, c))
        })
        .collect();
    let eval = |n: usize, x: P3| -> Option<S6> {
        let (o, q, c) = fits[n].as_ref()?;
        let d = [(x[0] - o[0]) / h, (x[1] - o[1]) / h, (x[2] - o[2]) / h];
        let v = basis(d, *q);
        let mut s = [0.0; 6];
        for k in 0..6 {
            for i in 0..v.len() {
                s[k] += v[i] * c[(i, k)];
            }
        }
        Some(s)
    };
    let mut out = vec![[0.0; 6]; m.points.len()];
    let mut cnt = vec![0.0f64; m.points.len()];
    for t in &m.tets {
        for a in 0..10 {
            let v = t[a] as usize;
            let ends: Vec<usize> = if a < 4 {
                vec![t[a] as usize]
            } else {
                EDGES[a - 4].iter().map(|k| t[*k] as usize).collect()
            };
            for c in ends {
                if let Some(s) = eval(c, m.points[v]) {
                    for k in 0..6 {
                        out[v][k] += s[k];
                    }
                    cnt[v] += 1.0;
                }
            }
        }
    }
    out.iter()
        .zip(&cnt)
        .map(|(s, c)| s.map(|x| x / c.max(1.0)))
        .collect()
}

/// 2 次の形関数への L2 の射影(整合質量で解く)。
pub(super) fn r_l2(p: &Prepared, e_mod: f64, nu: f64, u: &[f64]) -> Vec<S6> {
    let m = &p.mesh;
    let a = assemble(m, 1, |e| Some(tet10::mass(&m.tet_points(e))));
    let n = m.points.len();
    let mut rhs = vec![vec![0.0; n]; 6];
    for e in 0..m.tets.len() {
        let x = m.tet_points(e);
        let ue = element_u(p, u, e);
        for (xi, w) in gauss_fine() {
            let nn = tet10::shape(*xi);
            let (_, det) = tet10::jacobian(&x, *xi);
            let s = tet10::stress_at(&x, e_mod, nu, &ue, *xi);
            for k in 0..10 {
                for c in 0..6 {
                    rhs[c][m.tets[e][k] as usize] += w * det * nn[k] * s[c];
                }
            }
        }
    }
    let d = a.diag();
    let mut out = vec![[0.0; 6]; n];
    for c in 0..6 {
        let mut x = vec![0.0; n];
        pcg(
            &a,
            &rhs[c],
            &mut x,
            &|r, z| {
                for i in 0..r.len() {
                    z[i] = r[i] / d[i];
                }
            },
            1e-12,
            5000,
        )
        .unwrap();
        for i in 0..n {
            out[i][c] = x[i];
        }
    }
    out
}

/// 要素ごとの応力の勾配(要素の角の値を 1 次とみて)と体積。g[成分][軸]。
pub(super) fn elem_grads(
    m: &crate::tetmesh::TetMesh,
    en: &[[S6; 10]],
) -> Vec<([[f64; 3]; 6], f64)> {
    (0..m.tets.len())
        .map(|e| {
            let x: [P3; 4] = [0, 1, 2, 3].map(|k| m.points[m.tets[e][k] as usize]);
            let rows = [1, 2, 3].map(|k| [0, 1, 2].map(|a| x[k][a] - x[0][a]));
            let mm = nalgebra::Matrix3::from_fn(|i, j| rows[i][j]);
            let vol = mm.determinant().abs() / 6.0;
            let inv = mm.try_inverse().unwrap_or(nalgebra::Matrix3::zeros());
            let g = std::array::from_fn(|c| {
                let d = nalgebra::Vector3::new(
                    en[e][1][c] - en[e][0][c],
                    en[e][2][c] - en[e][0][c],
                    en[e][3][c] - en[e][0][c],
                );
                let v = inv * d;
                [v[0], v[1], v[2]]
            });
            (g, vol)
        })
        .collect()
}

/// 候補 E: 角は `r_corner` と同じ、辺の中点に両端の勾配の差から曲がりを足す(3 次の Hermite の中点の式
/// f(½) = (f₀+f₁)/2 + (f′₀−f′₁)/8)。勾配は角のまわりの要素の勾配を体積で平均。
pub(super) fn r_hermite(p: &Prepared, en: &[[S6; 10]]) -> Vec<S6> {
    let m = &p.mesh;
    let mut s = r_corner(p, en);
    let eg = elem_grads(m, en);
    let mut g = vec![[[0.0; 3]; 6]; m.corners];
    let mut w = vec![0.0f64; m.corners];
    for (e, t) in m.tets.iter().enumerate() {
        for a in 0..4 {
            let v = t[a] as usize;
            for c in 0..6 {
                for k in 0..3 {
                    g[v][c][k] += eg[e].1 * eg[e].0[c][k];
                }
            }
            w[v] += eg[e].1;
        }
    }
    for v in 0..m.corners {
        for c in 0..6 {
            for k in 0..3 {
                g[v][c][k] /= w[v].max(1e-30);
            }
        }
    }
    for t in &m.tets {
        for (k, [a, b]) in EDGES.iter().enumerate() {
            let (va, vb, vm) = (t[*a] as usize, t[*b] as usize, t[4 + k] as usize);
            let (xa, xb, xm) = (m.points[va], m.points[vb], m.points[vm]);
            let ed = [0, 1, 2].map(|i| xb[i] - xa[i]);
            let off = [0, 1, 2].map(|i| xm[i] - 0.5 * (xa[i] + xb[i]));
            for c in 0..6 {
                let dg: f64 = (0..3).map(|i| (g[va][c][i] - g[vb][c][i]) * ed[i]).sum();
                let go: f64 = (0..3)
                    .map(|i| 0.5 * (g[va][c][i] + g[vb][c][i]) * off[i])
                    .sum();
                s[vm][c] = 0.5 * (s[va][c] + s[vb][c]) + dg / 8.0 + go;
            }
        }
    }
    s
}

/// 角ごとの SPR の 2 次の多項式(まわりの要素の積分点に最小二乗で合わせる)。
pub(super) struct SprFits {
    pub(super) h: f64,
    pub(super) fits: Vec<Option<(P3, bool, DMatrix<f64>)>>,
}

impl SprFits {
    pub(super) fn new(p: &Prepared, gs: &[Vec<(P3, S6)>]) -> Self {
        let m = &p.mesh;
        let h = p.h();
        let mut around = vec![Vec::new(); m.corners];
        for (e, t) in m.tets.iter().enumerate() {
            for a in 0..4 {
                around[t[a] as usize].push(e);
            }
        }
        let fits = (0..m.corners)
            .map(|n| {
                let o = m.points[n];
                let pts: Vec<&(P3, S6)> = around[n].iter().flat_map(|e| gs[*e].iter()).collect();
                let q = pts.len() >= 30;
                let nb = if q { 10 } else { 4 };
                if pts.len() < nb {
                    return None;
                }
                let mut a = DMatrix::<f64>::zeros(nb, nb);
                let mut b = DMatrix::<f64>::zeros(nb, 6);
                for (x, s) in pts {
                    let d = [(x[0] - o[0]) / h, (x[1] - o[1]) / h, (x[2] - o[2]) / h];
                    let v = DVector::from_vec(basis(d, q));
                    a += &v * v.transpose();
                    for k in 0..6 {
                        for i in 0..nb {
                            b[(i, k)] += v[i] * s[k];
                        }
                    }
                }
                a.lu().solve(&b).map(|c| (o, q, c))
            })
            .collect();
        SprFits { h, fits }
    }
    pub(super) fn eval(&self, n: usize, x: P3) -> Option<S6> {
        let (o, q, c) = self.fits[n].as_ref()?;
        let h = self.h;
        let d = [(x[0] - o[0]) / h, (x[1] - o[1]) / h, (x[2] - o[2]) / h];
        let v = basis(d, *q);
        let mut s = [0.0; 6];
        for k in 0..6 {
            for i in 0..v.len() {
                s[k] += v[i] * c[(i, k)];
            }
        }
        Some(s)
    }
}

/// 候補 D: 角は `r_corner` と同じ、辺の中点に、両端の SPR の多項式の曲がり
/// (P(中点) − (P(a)+P(b))/2)の平均を足す。
pub(super) fn r_spr_curv(p: &Prepared, en: &[[S6; 10]], gs: &[Vec<(P3, S6)>]) -> Vec<S6> {
    let m = &p.mesh;
    let mut s = r_corner(p, en);
    let f = SprFits::new(p, gs);
    for t in &m.tets {
        for (k, [a, b]) in EDGES.iter().enumerate() {
            let (va, vb, vm) = (t[*a] as usize, t[*b] as usize, t[4 + k] as usize);
            let (xa, xb, xm) = (m.points[va], m.points[vb], m.points[vm]);
            let mut add = [0.0; 6];
            let mut n = 0.0;
            for c in [va, vb] {
                if let (Some(pm), Some(pa), Some(pb)) =
                    (f.eval(c, xm), f.eval(c, xa), f.eval(c, xb))
                {
                    for i in 0..6 {
                        add[i] += pm[i] - 0.5 * (pa[i] + pb[i]);
                    }
                    n += 1.0;
                }
            }
            for i in 0..6 {
                s[vm][i] = 0.5 * (s[va][i] + s[vb][i]) + add[i] / f64::max(n, 1.0);
            }
        }
    }
    s
}

pub(super) fn candidates(p: &Prepared, e: f64, nu: f64, u: &[f64]) -> Vec<(&'static str, Vec<S6>)> {
    let en = elem_nodal(p, e, nu, u);
    let gs = gauss_samples(p, e, nu, u);
    vec![
        ("A 角の平均", r_corner(p, &en)),
        ("B 要素の中点の平均", r_avg(p, &en)),
        ("C SPR 2 次", r_spr(p, &gs, true)),
        ("D 中点に SPR の曲がり", r_spr_curv(p, &en, &gs)),
        ("E 中点に勾配の差", r_hermite(p, &en)),
        ("F L2 の射影", r_l2(p, e, nu, u)),
    ]
}
