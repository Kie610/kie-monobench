//! 次元に依らない残差の行の解き方。行は変数の並びから 1 つの数を返す閉包で、微分は数値で取る
//! (中心差分)。最小ノルムのガウス・ニュートン法で解き、ヤコビ行列の階数から自由度と余分の
//! 拘束を求める。3D スケッチ(jc-sketch-3d)が使う。2D のスケッチは式の形を持つ [`crate::system`]。
//!
//! 解き方を作り直す時は、ここに新しい解き方を足し、同じ行で前の解き方と比べる(policy.md)。

use crate::ConstraintId;
use nalgebra::{DMatrix, DVector};

/// 残差の式(変数の値から 1 つの数)。
pub type Row = Box<dyn Fn(&[f64]) -> f64>;

/// 行と、その行を出した拘束(`None` は内側の式。例: 面を平らに保つ式)。
pub type Tagged = (Option<ConstraintId>, Row);

/// ヤコビ行列(数値微分)。
pub struct Jacobian(DMatrix<f64>);

impl Jacobian {
    /// 行の数。
    pub fn rows(&self) -> usize {
        self.0.nrows()
    }

    /// 変数の数。
    pub fn columns(&self) -> usize {
        self.0.ncols()
    }
}

fn vector(rows: &[Tagged], x: &[f64]) -> DVector<f64> {
    DVector::from_iterator(rows.len(), rows.iter().map(|(_, r)| r(x)))
}

/// 各行の残差。
pub fn residuals(rows: &[Tagged], x: &[f64]) -> Vec<f64> {
    rows.iter().map(|(_, r)| r(x)).collect()
}

/// 残差の絶対値の最大(行が無ければ 0)。
pub fn max_residual(rows: &[Tagged], x: &[f64]) -> f64 {
    rows.iter().map(|(_, r)| r(x).abs()).fold(0.0, f64::max)
}

/// 数値微分のヤコビ行列(中心差分)。`free` が偽の変数の列は 0 のままにする(その変数は動かさない)。
pub fn jacobian(rows: &[Tagged], x: &[f64], free: Option<&[bool]>) -> Jacobian {
    let (m, n) = (rows.len(), x.len());
    let mut j = DMatrix::zeros(m, n);
    let mut xp = x.to_vec();
    for k in 0..n {
        if free.is_some_and(|f| !f[k]) {
            continue;
        }
        let h = 1e-7 * x[k].abs().max(1.0);
        xp[k] = x[k] + h;
        let fp = residuals(rows, &xp);
        xp[k] = x[k] - h;
        let fm = residuals(rows, &xp);
        xp[k] = x[k];
        for i in 0..m {
            j[(i, k)] = (fp[i] - fm[i]) / (2.0 * h);
        }
    }
    Jacobian(j)
}

/// 式の残差を 0 にする(擬似逆行列の一番小さい動き、残差が減るまで歩幅を半分に)。`free` が偽の
/// 変数は動かさない。決まっていない所は始めの位置からできるだけ動かさない。解いた値と回数を返す。
pub fn least_change(rows: &[Tagged], mut x: Vec<f64>, free: Option<&[bool]>) -> (Vec<f64>, usize) {
    let mut iterations = 0;
    for _ in 0..100 {
        let r = vector(rows, &x);
        let cost = r.norm_squared();
        if cost < 1e-24 {
            break;
        }
        iterations += 1;
        let Jacobian(j) = jacobian(rows, &x, free);
        let svd = j.svd(true, true);
        let Ok(step) = svd.solve(&r, 1e-10) else {
            break;
        };
        let mut t = 1.0;
        let mut better = false;
        for _ in 0..30 {
            let trial: Vec<f64> = x.iter().zip(step.iter()).map(|(a, d)| a - t * d).collect();
            if vector(rows, &trial).norm_squared() < cost {
                x = trial;
                better = true;
                break;
            }
            t *= 0.5;
        }
        if !better {
            break;
        }
    }
    (x, iterations)
}

/// 行の並びの階数を、拘束ごとに足しながら数える。何も増やさない拘束は余分。内側の式(`None`)を
/// 先に、拘束は番号の順に足す。
pub fn redundant(j: &Jacobian, tags: &[Option<ConstraintId>]) -> (usize, Vec<ConstraintId>) {
    let j = &j.0;
    let n = j.ncols();
    let mut basis: Vec<DVector<f64>> = Vec::new();
    let mut redundant = Vec::new();
    let scale = j.amax().max(1e-12);
    let mut order: Vec<usize> = (0..tags.len()).collect();
    order.sort_by_key(|i| (tags[*i].is_some(), tags[*i]));
    let mut i = 0;
    while i < order.len() {
        let tag = tags[order[i]];
        let mut added = false;
        while i < order.len() && tags[order[i]] == tag {
            let mut v = DVector::from_iterator(n, j.row(order[i]).iter().copied());
            for b in &basis {
                let d = v.dot(b);
                v -= b * d;
            }
            let norm = v.norm();
            if norm > 1e-7 * scale {
                basis.push(v / norm);
                added = true;
            }
            i += 1;
        }
        if let Some(t) = tag
            && !added
        {
            redundant.push(t);
        }
    }
    (basis.len(), redundant)
}

/// 零空間に成分のある変数(拘束で決まっていない変数)。
pub fn free_variables(j: &Jacobian) -> Vec<bool> {
    let j = &j.0;
    let n = j.ncols();
    if j.nrows() == 0 {
        return vec![true; n];
    }
    // 行の空間の正規直交の基底を作り、各変数の単位ベクトルが基底で表せるかを見る
    let mut basis: Vec<DVector<f64>> = Vec::new();
    let scale = j.amax().max(1e-12);
    for row in j.row_iter() {
        let mut v = DVector::from_iterator(n, row.iter().copied());
        for b in &basis {
            let d = v.dot(b);
            v -= b * d;
        }
        let norm = v.norm();
        if norm > 1e-7 * scale {
            basis.push(v / norm);
        }
    }
    (0..n)
        .map(|k| basis.iter().map(|b| b[k] * b[k]).sum::<f64>() < 1.0 - 1e-6)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(n: u32) -> Option<ConstraintId> {
        Some(ConstraintId(n))
    }

    /// x0 + x1 = 2 と x0 − x1 = 0 は x = (1, 1) に決まる。
    #[test]
    fn two_lines_meet_at_one_point() {
        let rows: Vec<Tagged> = vec![
            (c(1), Box::new(|x: &[f64]| x[0] + x[1] - 2.0)),
            (c(2), Box::new(|x: &[f64]| x[0] - x[1])),
        ];
        let (x, _) = least_change(&rows, vec![5.0, -3.0], None);
        assert!(
            (x[0] - 1.0).abs() < 1e-9 && (x[1] - 1.0).abs() < 1e-9,
            "{x:?}"
        );
        let j = jacobian(&rows, &x, None);
        assert_eq!((j.rows(), j.columns()), (2, 2));
        assert_eq!(redundant(&j, &[c(1), c(2)]), (2, vec![]));
        assert_eq!(free_variables(&j), [false, false]);
        assert!(max_residual(&rows, &x) < 1e-9);
    }

    /// 決まっていない向きには動かさず(最小ノルム)、同じ式の写しは余分と数える。
    #[test]
    fn the_smallest_move_keeps_free_variables_and_finds_redundant_rows() {
        let rows: Vec<Tagged> = vec![
            (c(1), Box::new(|x: &[f64]| x[0] - 3.0)),
            (c(2), Box::new(|x: &[f64]| 2.0 * x[0] - 6.0)),
        ];
        let (x, _) = least_change(&rows, vec![0.0, 7.0], None);
        assert!((x[0] - 3.0).abs() < 1e-9);
        assert!((x[1] - 7.0).abs() < 1e-12, "{x:?}");
        let j = jacobian(&rows, &x, None);
        assert_eq!(redundant(&j, &[c(1), c(2)]), (1, vec![ConstraintId(2)]));
        assert_eq!(free_variables(&j), [false, true]);
    }

    /// `free` が偽の変数は動かさない(擬似逆行列の丸めの分だけは動く)。
    #[test]
    fn fixed_variables_stay() {
        let rows: Vec<Tagged> = vec![(c(1), Box::new(|x: &[f64]| x[0] + x[1] - 4.0))];
        let (x, _) = least_change(&rows, vec![1.0, 1.0], Some(&[false, true]));
        assert!((x[0] - 1.0).abs() < 1e-12, "{x:?}");
        assert!((x[1] - 3.0).abs() < 1e-9);
    }
}
