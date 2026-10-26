//! 疎な行列の分解。拘束のヤコビ行列は 1 行に 0 でない値が 2〜6 個しかないので、行列全体を
//! 密な SVD にかけず(変数の数の 3 乗で増え、変数 800 のパターンで 1 回 1 秒かかった)、
//! 行を 1 本ずつギブンス回転で上三角の行列 R に入れていく(行ごとの QR 分解)。
//!
//! 1 つの分解から次を求める。
//! - 階数と、前の行の張る空間に入っていて広げない行(余分な式)。行を入れる順に調べるので、
//!   式を順に足して空間が広がらないものを余分とする前のやり方と同じ判定になる
//! - 最小ノルムの最小二乗の解(ガウス・ニュートン法の 1 歩)
//! - 変数が式で決まっているか(変数の単位ベクトルが行の空間にあるか)
//!
//! 列は、使う行の少ない変数から順に並べて R の埋まりを抑える(パターンの写しの変数は 1 本の式
//! だけが使い、元の変数は写しの数だけの式が使う。元を先に並べると R の行が全部の写しに広がる)。

/// 疎な行。(列の番号, 値) を列の番号の昇順に持つ。
pub type Row = Vec<(usize, f64)>;

/// 分解で 0 と見なす値(回転で打ち消した後に残る丸めの値。行の大きさに対して)
const DROP: f64 = 1e-15;
/// 変数が決まっているとみなす、単位ベクトルの行の空間からの残りの 2 乗の上限
/// (1 - 写した長さの 2 乗で求めるので、2 乗で 1e-16 より細かくは分からない)
const UNIT_REST2: f64 = 1e-12;

/// 疎な行列の行ごとの QR 分解。
pub struct Factor {
    n: usize,
    /// 列 → 並べた位置
    pos: Vec<usize>,
    /// 並べた位置ごとの R の行(その位置が先頭の行)。中の列の番号は並べた位置
    r: Vec<Option<Row>>,
    /// R の行ごとの、回転を右辺にも当てた値
    z: Vec<f64>,
    /// 入れた行ごとに、空間を広げたか
    grew: Vec<bool>,
    rank: usize,
    /// R Rᵀ のコレスキー分解(R の転置の QR 分解の上三角)。初めに使う時に作る
    s: Option<Vec<Option<Row>>>,
}

/// 2 つの疎な行の回転: (c a + s b, -s a + c b)。`skip` の列は b から消す(打ち消した所)。
fn rotate(a: &Row, b: &Row, c: f64, s: f64, skip: usize, eps: f64) -> (Row, Row) {
    let mut na = Vec::with_capacity(a.len() + b.len());
    let mut nb = Vec::with_capacity(a.len() + b.len());
    let (mut i, mut k) = (0, 0);
    while i < a.len() || k < b.len() {
        let (col, va, vb) = match (a.get(i), b.get(k)) {
            (Some(&(ca, va)), Some(&(cb, vb))) if ca == cb => {
                i += 1;
                k += 1;
                (ca, va, vb)
            }
            (Some(&(ca, va)), Some(&(cb, _))) if ca < cb => {
                i += 1;
                (ca, va, 0.0)
            }
            (Some(&(ca, va)), None) => {
                i += 1;
                (ca, va, 0.0)
            }
            (_, Some(&(cb, vb))) => {
                k += 1;
                (cb, 0.0, vb)
            }
            (None, None) => unreachable!("どちらかが残る"),
        };
        let x = c * va + s * vb;
        if x.abs() > eps {
            na.push((col, x));
        }
        let y = -s * va + c * vb;
        if col != skip && y.abs() > eps {
            nb.push((col, y));
        }
    }
    (na, nb)
}

fn norm(row: &Row) -> f64 {
    row.iter().map(|(_, v)| v * v).sum::<f64>().sqrt()
}

/// 行を 1 本ずつ上三角の `r` に入れる。広げたら true(残りの大きさが `tol` 以下なら入れない)。
/// `z` は右辺の値(行ごと)。
fn insert(r: &mut [Option<Row>], z: &mut [f64], mut w: Row, mut zw: f64, tol: f64) -> bool {
    let eps = DROP * norm(&w).max(1.0);
    // R の先頭の値は eps より大きく保つ(回転の後も先頭が消えない)
    w.retain(|e| e.1.abs() > eps);
    while let Some(&(p, b)) = w.first() {
        match r[p].take() {
            Some(a) => {
                let alpha = a[0].1;
                let rho = alpha.hypot(b);
                let (c, s) = (alpha / rho, b / rho);
                let (na, nb) = rotate(&a, &w, c, s, p, eps);
                let za = z[p];
                z[p] = c * za + s * zw;
                zw = -s * za + c * zw;
                r[p] = Some(na);
                w = nb;
            }
            None => {
                if norm(&w) > tol {
                    z[p] = zw;
                    r[p] = Some(w);
                    return true;
                }
                return false;
            }
        }
    }
    false
}

impl Factor {
    /// 行 `rows`(列の数 `n`)を順に入れて分解する。`rhs` は右辺(行ごと。最小二乗の解に使う)。
    /// 0 と見なす大きさは、行列の大きさ(行と列の長さの最大)の 1e-8 倍(1 未満なら 1e-8)。
    pub fn new(rows: &[Row], n: usize, rhs: Option<&[f64]>) -> Self {
        let mut uses = vec![0usize; n];
        let mut col2 = vec![0.0; n];
        let mut big = 0.0_f64;
        for row in rows {
            for &(c, v) in row {
                uses[c] += 1;
                col2[c] += v * v;
            }
            big = big.max(norm(row));
        }
        big = col2.iter().fold(big, |m, v| m.max(v.sqrt()));
        let tol = 1e-8 * big.max(1.0);
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by_key(|&c| (uses[c], c));
        let mut pos = vec![0; n];
        for (p, &c) in order.iter().enumerate() {
            pos[c] = p;
        }
        let mut r = vec![None; n];
        let mut z = vec![0.0; n];
        let mut grew = Vec::with_capacity(rows.len());
        for (i, row) in rows.iter().enumerate() {
            let mut w: Row = row
                .iter()
                .filter(|(_, v)| *v != 0.0)
                .map(|&(c, v)| (pos[c], v))
                .collect();
            w.sort_unstable_by_key(|e| e.0);
            let zw = rhs.map_or(0.0, |f| f[i]);
            grew.push(insert(&mut r, &mut z, w, zw, tol));
        }
        let rank = r.iter().filter(|x| x.is_some()).count();
        Self {
            n,
            pos,
            r,
            z,
            grew,
            rank,
            s: None,
        }
    }

    /// 階数。
    pub fn rank(&self) -> usize {
        self.rank
    }

    /// 入れた行 `i` が、それより前の行の張る空間を広げたか。
    pub fn grew(&self, i: usize) -> bool {
        self.grew[i]
    }

    /// R の列ごとの値(並べた位置ごとに、(R の行, 値))。
    fn columns(&self) -> Vec<Row> {
        let mut cols: Vec<Row> = vec![vec![]; self.n];
        for (k, row) in self.r.iter().enumerate() {
            for &(q, v) in row.iter().flatten() {
                cols[q].push((k, v));
            }
        }
        cols
    }

    /// R Rᵀ = Sᵀ S の上三角 S(R の転置の行ごとの QR 分解)。
    fn s(&mut self) -> &[Option<Row>] {
        if self.s.is_none() {
            let mut s = vec![None; self.n];
            let mut zz = vec![0.0; self.n];
            for col in self.columns() {
                insert(&mut s, &mut zz, col, 0.0, 0.0);
            }
            self.s = Some(s);
        }
        self.s.as_deref().expect("作った")
    }

    /// Sᵀ u = b を解く(`b` は並べた位置ごと。中で書き換える)。
    fn forward(s: &[Option<Row>], b: &mut [f64]) {
        for j in 0..b.len() {
            if b[j] == 0.0 {
                continue;
            }
            let Some(row) = &s[j] else {
                b[j] = 0.0;
                continue;
            };
            let u = b[j] / row[0].1;
            b[j] = u;
            for &(k, v) in &row[1..] {
                b[k] -= v * u;
            }
        }
    }

    /// 最小ノルムの最小二乗の解: ‖A d - rhs‖ を最小にする d のうち、いちばん短いもの
    /// (分解に渡した `rhs` で。列の順は元の列の順)。
    pub fn min_norm(&mut self) -> Vec<f64> {
        let n = self.n;
        let mut u = self.z.clone();
        let s = self.s().to_vec();
        Self::forward(&s, &mut u);
        // S y = u
        let mut y = vec![0.0; n];
        for k in (0..n).rev() {
            if let Some(row) = &s[k] {
                let mut acc = u[k];
                for &(j, v) in &row[1..] {
                    acc -= v * y[j];
                }
                y[k] = acc / row[0].1;
            }
        }
        // d = Rᵀ y
        let mut d = vec![0.0; n];
        for (k, row) in self.r.iter().enumerate() {
            for &(q, v) in row.iter().flatten() {
                d[q] += v * y[k];
            }
        }
        (0..n).map(|c| d[self.pos[c]]).collect()
    }

    /// 列ごとに、その変数の単位ベクトルが行の空間にあるか(変数が式で決まっているか)。
    pub fn determined(&mut self) -> Vec<bool> {
        let n = self.n;
        let cols = self.columns();
        let s = self.s().to_vec();
        let mut b = vec![0.0; n];
        let by_pos: Vec<bool> = cols
            .iter()
            .map(|col| {
                if col.is_empty() {
                    return false;
                }
                // 単位ベクトルの空間への写しの長さの 2 乗 = ‖S⁻ᵀ R e‖²
                b.iter_mut().for_each(|x| *x = 0.0);
                for &(k, v) in col {
                    b[k] = v;
                }
                Self::forward(&s, &mut b);
                1.0 - b.iter().map(|x| x * x).sum::<f64>() <= UNIT_REST2
            })
            .collect();
        (0..n).map(|c| by_pos[self.pos[c]]).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nalgebra::{DMatrix, DVector};

    fn sparse(m: &DMatrix<f64>) -> Vec<Row> {
        (0..m.nrows())
            .map(|i| {
                (0..m.ncols())
                    .filter(|&c| m[(i, c)] != 0.0)
                    .map(|c| (c, m[(i, c)]))
                    .collect()
            })
            .collect()
    }

    /// 階数・広げた行・決まった変数・最小ノルムの解が、密な SVD で求めたものと同じになる。
    #[test]
    fn agrees_with_dense_svd() {
        // 5 変数。3 行目は 1 行目の 2 倍(余分)、変数 4 は式に出ない(決まらない)、
        // 変数 2 と 3 は差だけが決まる
        let j = DMatrix::from_row_slice(
            4,
            5,
            &[
                1.0, 0.0, 0.0, 0.0, 0.0, //
                0.0, 1.0, -1.0, 0.0, 0.0, //
                2.0, 0.0, 0.0, 0.0, 0.0, //
                0.0, 0.5, 0.0, 1.0, 0.0,
            ],
        );
        let f = DVector::from_vec(vec![1.0, 2.0, 2.0, -1.0]);
        let mut a = Factor::new(&sparse(&j), 5, Some(f.as_slice()));
        assert_eq!(a.rank(), crate::system::rank(&j));
        assert_eq!(
            (0..4).map(|i| a.grew(i)).collect::<Vec<_>>(),
            [true, true, false, true]
        );
        let space = crate::system::RowSpace::of(&j);
        assert_eq!(
            a.determined(),
            (0..5).map(|v| space.has_unit(v, 5)).collect::<Vec<_>>()
        );
        let dense = j.clone().svd(true, true).solve(&f, 1e-12).expect("解ける");
        let d = a.min_norm();
        for (x, y) in d.iter().zip(dense.iter()) {
            assert!((x - y).abs() < 1e-12, "{d:?} と {dense}");
        }
    }
}
