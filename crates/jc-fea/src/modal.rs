//! 固有振動(共振する周波数と揺れの形)。K φ = ω² M φ の小さい方からいくつかを、
//! LOBPCG(局所最適のブロック前処理つき共役勾配法、Knyazev 2001)で求める。前処理は
//! 静解析と同じ代数的マルチグリッドの V サイクル。質量は整合した質量行列(2 次の四面体で
//! 節点に集めると角の質量が負になるため)。単位は mm・N・t(トン)・s で、密度は g/cm³ で受け取る。

use crate::amg::{Amg, rigid_modes};
use crate::boundary::Prepared;
use crate::solve::{apply_fixed, assemble, blocks};
use crate::sparse::dot;
use crate::statics::{fixed_dofs, fixed_mask};
use crate::tet10;
use nalgebra::{DMatrix, SymmetricEigen};

type P3 = [f64; 3];

#[derive(Debug, Clone, PartialEq)]
pub struct ModalInput {
    pub e: f64,
    pub nu: f64,
    /// 密度(g/cm³)
    pub density: f64,
    pub fixed: Vec<String>,
    /// 求める数
    pub count: usize,
}

#[derive(Debug, Clone)]
pub struct ModalOutput {
    /// 固有振動数(Hz、小さい順)
    pub freqs: Vec<f64>,
    /// 揺れの形(節点ごと。いちばん大きく動く所を 1 にそろえる)
    pub shapes: Vec<Vec<P3>>,
    pub iterations: usize,
}

/// 小さな対称の行列の固有値を小さい順に(固有ベクトルは列)。
fn eig_sorted(a: DMatrix<f64>) -> (Vec<f64>, DMatrix<f64>) {
    let n = a.nrows();
    let e = SymmetricEigen::new(a);
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&i, &j| e.eigenvalues[i].total_cmp(&e.eigenvalues[j]));
    let vals = order.iter().map(|&i| e.eigenvalues[i]).collect();
    let mut vecs = DMatrix::zeros(n, n);
    for (c, &i) in order.iter().enumerate() {
        vecs.set_column(c, &e.eigenvectors.column(i));
    }
    (vals, vecs)
}

/// 列の並び S について、K と M の部分空間での小さい方から `m` 個の固有値と、組み合わせの係数。
fn rayleigh_ritz(
    s: &[Vec<f64>],
    ks: &[Vec<f64>],
    ms: &[Vec<f64>],
    m: usize,
) -> Option<(Vec<f64>, DMatrix<f64>)> {
    let q = s.len();
    let mut g = DMatrix::zeros(q, q);
    let mut h = DMatrix::zeros(q, q);
    for i in 0..q {
        for j in i..q {
            let gv = dot(&s[i], &ms[j]);
            let hv = dot(&s[i], &ks[j]);
            g[(i, j)] = gv;
            g[(j, i)] = gv;
            h[(i, j)] = hv;
            h[(j, i)] = hv;
        }
    }
    // 列の大きさをそろえてから、M で直交にする(小さすぎる向きは捨てる)
    let d: Vec<f64> = (0..q)
        .map(|i| {
            let v = g[(i, i)];
            if v > 0.0 { 1.0 / v.sqrt() } else { 0.0 }
        })
        .collect();
    for i in 0..q {
        for j in 0..q {
            g[(i, j)] *= d[i] * d[j];
            h[(i, j)] *= d[i] * d[j];
        }
    }
    let (gv, gvec) = eig_sorted(g);
    let gmax = gv.iter().copied().fold(0.0, f64::max);
    let keep: Vec<usize> = (0..q).filter(|&i| gv[i] > 1e-10 * gmax).collect();
    if keep.len() < m {
        return None;
    }
    let mut z = DMatrix::zeros(q, keep.len());
    for (c, &i) in keep.iter().enumerate() {
        z.set_column(c, &(gvec.column(i) / gv[i].sqrt()));
    }
    let a = z.transpose() * &h * &z;
    let (vals, vecs) = eig_sorted((&a + a.transpose()) * 0.5);
    let mut c = &z * vecs.columns(0, m);
    for i in 0..q {
        for j in 0..m {
            c[(i, j)] *= d[i];
        }
    }
    Some((vals[..m].to_vec(), c))
}

fn combine(s: &[Vec<f64>], c: &DMatrix<f64>, rows: std::ops::Range<usize>) -> Vec<Vec<f64>> {
    let n = s[0].len();
    (0..c.ncols())
        .map(|j| {
            let mut v = vec![0.0; n];
            for i in rows.clone() {
                let w = c[(i, j)];
                if w != 0.0 {
                    for (o, x) in v.iter_mut().zip(&s[i]) {
                        *o += w * x;
                    }
                }
            }
            v
        })
        .collect()
}

pub fn solve_modal(p: &Prepared, input: &ModalInput) -> Result<ModalOutput, String> {
    let model = &p.mesh;
    let mut k = assemble(model, 3, |e| {
        Some(tet10::stiffness(&model.tet_points(e), input.e, input.nu))
    });
    let fixed = fixed_dofs(p, &input.fixed)?;
    let mut dummy = vec![0.0; k.rows];
    apply_fixed(&mut k, &mut dummy, &fixed);
    let n = k.rows;
    let rho = input.density * 1e-9;
    let mut mass = assemble(model, 3, |e| {
        let m10 = tet10::mass(&model.tet_points(e));
        let mut me = vec![0.0; 900];
        for a in 0..10 {
            for b in 0..10 {
                for c in 0..3 {
                    me[(3 * a + c) * 30 + 3 * b + c] = rho * m10[a * 10 + b];
                }
            }
        }
        Some(me)
    });
    let is_fixed = fixed_mask(n, &fixed);
    for r in 0..n {
        for kk in mass.ptr[r]..mass.ptr[r + 1] {
            if is_fixed[r] || is_fixed[mass.idx[kk] as usize] {
                mass.val[kk] = 0.0;
            }
        }
    }
    let free: Vec<f64> = is_fixed
        .iter()
        .map(|f| if *f { 0.0 } else { 1.0 })
        .collect();
    let modes = rigid_modes(&model.points, &is_fixed);
    let mg = Amg::new(k, blocks(model.points.len(), 3), modes)?;
    let k = mg.matrix();
    let want = input.count.clamp(1, 20);
    let m = (want + 3).min(n.saturating_sub(fixed.len()).max(1));
    let kmul = |x: &[f64]| {
        let mut y = vec![0.0; n];
        k.mul(x, &mut y);
        y
    };
    let mmul = |x: &[f64]| {
        let mut y = vec![0.0; n];
        mass.mul(x, &mut y);
        y
    };
    let mask = |v: &mut Vec<f64>| v.iter_mut().zip(&free).for_each(|(a, f)| *a *= f);
    // 決まった種から作る乱数の初めの列(毎回同じ結果にする)
    let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
    let mut x: Vec<Vec<f64>> = (0..m)
        .map(|_| {
            let mut v: Vec<f64> = (0..n)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    (seed >> 11) as f64 / (1u64 << 53) as f64 - 0.5
                })
                .collect();
            mask(&mut v);
            v
        })
        .collect();
    let mut p_dir: Vec<Vec<f64>> = Vec::new();
    let (mut theta, c) = {
        let ks: Vec<_> = x.iter().map(|v| kmul(v)).collect();
        let ms: Vec<_> = x.iter().map(|v| mmul(v)).collect();
        rayleigh_ritz(&x, &ks, &ms, m).ok_or("揺れの形の初めの値が作れませんでした")?
    };
    x = combine(&x, &c, 0..m);
    let tol = 1e-4;
    let mut iterations = 0;
    let mut calm = 0;
    for it in 0..300 {
        iterations = it + 1;
        let kx: Vec<_> = x.iter().map(|v| kmul(v)).collect();
        let mx: Vec<_> = x.iter().map(|v| mmul(v)).collect();
        let mut done = true;
        let mut w = Vec::with_capacity(m);
        for j in 0..m {
            let mut r: Vec<f64> = kx[j]
                .iter()
                .zip(&mx[j])
                .map(|(a, b)| a - theta[j] * b)
                .collect();
            mask(&mut r);
            let rn = dot(&r, &r).sqrt();
            let scale = theta[j].abs().max(1e-30) * dot(&mx[j], &mx[j]).sqrt();
            if j < want && rn > tol * scale {
                done = false;
            }
            let mut z = vec![0.0; n];
            mg.apply(&r, &mut z);
            mask(&mut z);
            w.push(z);
        }
        if done || calm >= 2 {
            break;
        }
        // W と P から X の向きを除く(M の内積で 2 回)。収束が進むと W は X とほぼ同じ向きになり、
        // 除かないと違いの所が丸めに埋もれて止まる(Hetmaniuk・Lehoucq 2006 の基底の選び方)
        let mut extra: Vec<Vec<f64>> = w.into_iter().chain(p_dir.iter().cloned()).collect();
        for v in &mut extra {
            for _ in 0..2 {
                for (xi, mxi) in x.iter().zip(&mx) {
                    let c = dot(mxi, v);
                    for (a, b) in v.iter_mut().zip(xi) {
                        *a -= c * b;
                    }
                }
            }
            let nrm = dot(v, &mmul(v)).sqrt();
            if nrm > 0.0 {
                v.iter_mut().for_each(|a| *a /= nrm);
            }
        }
        let mut s = x.clone();
        s.extend(extra);
        let ks: Vec<_> = s.iter().map(|v| kmul(v)).collect();
        let ms: Vec<_> = s.iter().map(|v| mmul(v)).collect();
        let Some((th, c)) = rayleigh_ritz(&s, &ks, &ms, m) else {
            // 向きがそろってしまった時は、前の向きを捨ててやり直す
            p_dir.clear();
            continue;
        };
        // 残差は網の細かい揺れに敏感で小さくなりにくいので、固有値が落ち着いたら止める
        let settled = th[..want]
            .iter()
            .zip(&theta[..want])
            .all(|(a, b)| (a - b).abs() <= 1e-7 * a.abs());
        calm = if settled { calm + 1 } else { 0 };
        theta = th;
        p_dir = combine(&s, &c, m..s.len());
        x = combine(&s, &c, 0..s.len());
    }
    let freqs: Vec<f64> = theta[..want]
        .iter()
        .map(|t| t.max(0.0).sqrt() / std::f64::consts::TAU)
        .collect();
    let shapes = x[..want]
        .iter()
        .map(|v| {
            let s: Vec<P3> = v.as_chunks::<3>().0.to_vec();
            let mx = s
                .iter()
                .map(|u| (u[0] * u[0] + u[1] * u[1] + u[2] * u[2]).sqrt())
                .fold(0.0, f64::max);
            s.into_iter()
                .map(|u| if mx > 0.0 { u.map(|c| c / mx) } else { u })
                .collect()
        })
        .collect();
    log::info!("固有振動: {iterations} 回で {freqs:?} Hz");
    Ok(ModalOutput {
        freqs,
        shapes,
        iterations,
    })
}
