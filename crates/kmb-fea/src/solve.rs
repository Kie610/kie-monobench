//! 全体の行列の組み立てと、解き方(代数的マルチグリッドを前処理にした共役勾配法)。
//!
//! 網の大きさに関わらず、いつも同じ前処理つきの共役勾配法で解く(直接法と反復法を模型の
//! 大きさで自動で選ぶ形は作らない。docs/避ける形.md)。

use crate::amg::Amg;
use crate::sparse::{Csr, axpy, dot};
use crate::tetmesh::TetMesh;

/// 網の全体の行列の形(節点ごとに `dof` 個の値。同じ要素の節点どうしがつながる)。
pub fn pattern(m: &TetMesh, dof: usize) -> Csr {
    let n = m.points.len();
    let mut elems_of: Vec<Vec<u32>> = vec![Vec::new(); n];
    for (e, t) in m.tets.iter().enumerate() {
        for v in t {
            elems_of[*v as usize].push(e as u32);
        }
    }
    let mut rows = Vec::with_capacity(n * dof);
    let mut near: Vec<u32> = Vec::with_capacity(64);
    for es in &elems_of {
        near.clear();
        for e in es {
            near.extend_from_slice(&m.tets[*e as usize]);
        }
        near.sort_unstable();
        near.dedup();
        let row: Vec<u32> = near
            .iter()
            .flat_map(|&v| (0..dof as u32).map(move |d| v * dof as u32 + d))
            .collect();
        for _ in 0..dof {
            rows.push(row.clone());
        }
    }
    Csr::from_pattern(n * dof, rows)
}

/// 要素ごとの行列 `ke(e)`(10·dof 角、行優先。`None` なら足さない)を組み立てる。
pub fn assemble(m: &TetMesh, dof: usize, ke: impl Fn(usize) -> Option<Vec<f64>>) -> Csr {
    let mut a = pattern(m, dof);
    let n = 10 * dof;
    let mut rows = vec![0usize; n];
    for (e, el) in m.tets.iter().enumerate() {
        let Some(k) = ke(e) else { continue };
        for (a_, node) in el.iter().enumerate() {
            for d in 0..dof {
                rows[a_ * dof + d] = *node as usize * dof + d;
            }
        }
        for r in 0..n {
            let row = rows[r];
            let start = a.ptr[row];
            let cols = &a.idx[start..a.ptr[row + 1]];
            for c in 0..n {
                let v = k[r * n + c];
                if v == 0.0 {
                    continue;
                }
                let pos = cols
                    .binary_search(&(rows[c] as u32))
                    .expect("要素の節点は形にある");
                a.val[start + pos] += v;
            }
        }
    }
    a
}

/// 値を決めた未知数(固定・温度)を入れる。行と列を消して対角だけ残し、右辺を合わせる
/// (対称のまま)。
pub fn apply_fixed(a: &mut Csr, b: &mut [f64], fixed: &[(usize, f64)]) {
    let mut is_fixed = vec![None; a.rows];
    for &(d, v) in fixed {
        is_fixed[d] = Some(v);
    }
    for r in 0..a.rows {
        if is_fixed[r].is_some() {
            continue;
        }
        for k in a.ptr[r]..a.ptr[r + 1] {
            if let Some(v) = is_fixed[a.idx[k] as usize] {
                b[r] -= a.val[k] * v;
                a.val[k] = 0.0;
            }
        }
    }
    for r in 0..a.rows {
        if let Some(v) = is_fixed[r] {
            let mut diag = 0.0;
            for k in a.ptr[r]..a.ptr[r + 1] {
                if a.idx[k] as usize == r {
                    diag = a.val[k];
                } else {
                    a.val[k] = 0.0;
                }
            }
            if diag == 0.0 {
                diag = 1.0;
                a.add(r, r as u32, 1.0);
            }
            b[r] = diag * v;
        }
    }
}

/// 共役勾配法の結果。
#[derive(Debug, Clone, Copy)]
pub struct CgReport {
    pub iterations: usize,
    pub residual: f64,
}

/// A x = b を前処理つきの共役勾配法で解く(`x` は初めの値。相対残差 `tol` まで)。
pub fn pcg(
    a: &Csr,
    b: &[f64],
    x: &mut [f64],
    precond: &dyn Fn(&[f64], &mut [f64]),
    tol: f64,
    max_iter: usize,
) -> Result<CgReport, String> {
    let n = b.len();
    let bn = dot(b, b).sqrt();
    if bn == 0.0 {
        x.iter_mut().for_each(|v| *v = 0.0);
        return Ok(CgReport {
            iterations: 0,
            residual: 0.0,
        });
    }
    let mut r = vec![0.0; n];
    a.mul(x, &mut r);
    for i in 0..n {
        r[i] = b[i] - r[i];
    }
    let mut z = vec![0.0; n];
    precond(&r, &mut z);
    let mut p = z.clone();
    let mut rz = dot(&r, &z);
    let mut ap = vec![0.0; n];
    for it in 0..max_iter {
        let res = dot(&r, &r).sqrt() / bn;
        if res <= tol {
            return Ok(CgReport {
                iterations: it,
                residual: res,
            });
        }
        a.mul(&p, &mut ap);
        let pap = dot(&p, &ap);
        if pap <= 0.0 || !pap.is_finite() {
            return Err("連立方程式が解けません(支えが足りないか、網がばらばらです)".into());
        }
        let alpha = rz / pap;
        axpy(alpha, &p, x);
        axpy(-alpha, &ap, &mut r);
        precond(&r, &mut z);
        let rz2 = dot(&r, &z);
        let beta = rz2 / rz;
        rz = rz2;
        for i in 0..n {
            p[i] = z[i] + beta * p[i];
        }
    }
    let res = dot(&r, &r).sqrt() / bn;
    if res < tol * 100.0 {
        log::warn!("共役勾配法が {max_iter} 回で収束しきらなかった(相対残差 {res:.2e})");
        return Ok(CgReport {
            iterations: max_iter,
            residual: res,
        });
    }
    Err(format!(
        "連立方程式が {max_iter} 回で収束しませんでした(相対残差 {res:.2e})。固定する面を見直してください"
    ))
}

/// 節点ごとに `dof` 個の未知数の範囲。
pub fn blocks(nodes: usize, dof: usize) -> Vec<usize> {
    (0..=nodes).map(|i| i * dof).collect()
}

/// 前処理を作って A x = b を解く(`x` は初めの値)。
pub fn solve(
    a: Csr,
    b: &[f64],
    x: &mut [f64],
    dof: usize,
    nullspace: Vec<Vec<f64>>,
    tol: f64,
) -> Result<(CgReport, Amg), String> {
    let amg = Amg::new(a, blocks(b.len() / dof, dof), nullspace)?;
    let rep = pcg(amg.matrix(), b, x, &|r, z| amg.apply(r, z), tol, 1000)?;
    log::info!(
        "共役勾配法: {} 回、相対残差 {:.1e}、未知数 {}",
        rep.iterations,
        rep.residual,
        b.len()
    );
    Ok((rep, amg))
}

/// 同じ行列・固定値で右辺だけを変えて解く。前処理と固定値の右辺への寄与を保持する。
pub struct ReusableSolve {
    amg: Amg,
    lift: Vec<f64>,
    fixed: Vec<(usize, f64)>,
}

impl ReusableSolve {
    pub fn new(mut a: Csr, fixed: &[(usize, f64)]) -> Result<Self, String> {
        let mut lift = vec![0.0; a.rows];
        apply_fixed(&mut a, &mut lift, fixed);
        let mut free = vec![1.0; a.rows];
        for &(d, _) in fixed {
            free[d] = 0.0;
        }
        let fixed = fixed.iter().map(|&(d, _)| (d, lift[d])).collect();
        let amg = Amg::new(a, blocks(lift.len(), 1), vec![free])?;
        Ok(Self { amg, lift, fixed })
    }

    pub fn solve(&self, b: &[f64], x: &mut [f64]) -> Result<CgReport, String> {
        let mut rhs: Vec<f64> = b.iter().zip(&self.lift).map(|(b, l)| b + l).collect();
        for &(d, v) in &self.fixed {
            rhs[d] = v;
        }
        pcg(
            self.amg.matrix(),
            &rhs,
            x,
            &|r, z| self.amg.apply(r, z),
            1e-9,
            1000,
        )
    }
}

#[cfg(test)]
mod reusable_tests {
    use super::*;

    /// 確かめること: 右辺を変えても固定温度と固定列の寄与を毎回守る。
    #[test]
    fn reusable_thermal_solve_applies_fixed_values_to_each_rhs() {
        let mut a = Csr::from_pattern(2, vec![vec![0, 1], vec![0, 1]]);
        a.val = vec![2.0, -1.0, -1.0, 2.0];
        let prepared = ReusableSolve::new(a, &[(0, 10.0)]).unwrap();
        let mut x = vec![10.0, 0.0];
        for (rhs, want) in [(0.0, 5.0), (4.0, 7.0), (-2.0, 4.0)] {
            prepared.solve(&[123.0, rhs], &mut x).unwrap();
            assert!((x[0] - 10.0).abs() < 1e-10);
            assert!((x[1] - want).abs() < 1e-10);
        }
    }
}
