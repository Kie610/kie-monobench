//! 疎な行列の分解。拘束のヤコビ行列は 1 行に 0 でない値が 2〜6 個しかないので、行列全体を
//! 密な SVD にかけず(変数の数の 3 乗で増え、変数 800 のパターンで 1 回 1 秒かかった)、
//! 行を 1 本ずつギブンス回転で上三角の行列 R に入れていく(行ごとの QR 分解)。
//!
//! 1 つの分解から次を求める。
//! - 階数と、前の行の張る空間に入っていて広げない行(余分な式)。行を入れる順に調べるので、
//!   式を順に足して空間が広がらないものを余分とする前のやり方と同じ判定になる
//! - 最小ノルムの最小二乗の解(ガウス・ニュートン法の 1 歩)
//! - 変数が式で決まっているか(変数の単位ベクトルが行の空間にあるか)
//! - ベクトルの行の空間への写し(引く時の 1 歩で、拘束の式を変えない向きを求める)
//!
//! 行は後から 1 本ずつ足せる([`Factor::add`])。拘束を足す前の確かめと自動拘束は、行を足して
//! 空間が広がるかを見る。
//! 列は、使う行の少ない変数から順に並べて R の埋まりを抑える(パターンの写しの変数は 1 本の式
//! だけが使い、元の変数は写しの数だけの式が使う。元を先に並べると R の行が全部の写しに広がる)。

/// 疎な行。(列の番号, 値) を列の番号の昇順に持つ。
pub type Row = Vec<(usize, f64)>;

/// 分解で 0 と見なす値(回転で打ち消した後に残る丸めの値。行の大きさに対して)
const DROP: f64 = 1e-15;
/// 変数が決まっているとみなす、単位ベクトルの行の空間からの残りの 2 乗の上限
/// (1 - 写した長さの 2 乗で求めるので、2 乗で 1e-16 より細かくは分からない)
const UNIT_REST2: f64 = 1e-12;
/// 行を入れた後の残りがこの倍数 × 0 と見なす大きさ以下なら、距離を求め直して確かめる
const SUSPECT: f64 = 1e3;

/// 疎な行列の行ごとの QR 分解。
#[derive(Clone)]
pub struct Factor {
    n: usize,
    /// 0 と見なす大きさ
    tol: f64,
    /// 列 → 並べた位置
    pos: Vec<usize>,
    /// 並べた位置 → 列
    col: Vec<usize>,
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

/// 行を 1 本ずつ上三角の `r` に入れる。広げたら、入れた残りの行の大きさを返す(R に行の無い
/// 位置に来た先頭の値が `tol` 以下なら 0 と見なす。どこにも入らなければ広げない)。
/// `z` は右辺の値(行ごと)。`log` があれば、書き換えた R の行の前の値を積む(戻す時に使う)。
fn insert(
    r: &mut [Option<Row>],
    z: &mut [f64],
    mut w: Row,
    mut zw: f64,
    tol: f64,
    mut log: Option<&mut Vec<(usize, Option<Row>, f64)>>,
) -> Option<f64> {
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
                if let Some(log) = log.as_deref_mut() {
                    log.push((p, Some(a), za));
                }
                w = nb;
            }
            // R に行の無い位置で先頭がごく小さいのは、前の行の空間に入る行の丸めの残り
            // (前の行の空間に入る行は、回転で先頭がいつも R に行の有る位置に来る)。0 と見なして
            // 外し、残りを後ろの行で打ち消し続ける(止めると、後ろの行で消える値まで残りに数える)
            None if b.abs() <= tol => {
                w.remove(0);
            }
            None => {
                if let Some(log) = log.as_deref_mut() {
                    log.push((p, None, z[p]));
                }
                let rest = norm(&w);
                z[p] = zw;
                r[p] = Some(w);
                return Some(rest);
            }
        }
    }
    None
}

impl Factor {
    /// 行 `rows`(列の数 `n`)を順に入れて分解する。`rhs` は右辺(行ごと。最小二乗の解に使う)。
    /// 0 と見なす大きさは、行列の大きさ(行と列の長さの最大)の 1e-8 倍(1 未満なら 1e-8)。
    pub fn new(rows: &[Row], n: usize, rhs: Option<&[f64]>) -> Self {
        let mut a = Self::empty(rows, n);
        for (i, row) in rows.iter().enumerate() {
            a.insert_row(row, rhs.map_or(0.0, |f| f[i]), a.tol);
        }
        a
    }

    /// 行を 1 本も入れていない分解。列の並べ方と 0 と見なす大きさは `rows` から [`Self::new`] と
    /// 同じに決める(行は [`Self::add`] で足す)。
    pub fn empty(rows: &[Row], n: usize) -> Self {
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
        Self {
            n,
            tol,
            pos,
            col: order,
            r: vec![None; n],
            z: vec![0.0; n],
            grew: Vec::with_capacity(rows.len()),
            rank: 0,
            s: None,
        }
    }

    /// 行 `row`(右辺 `zw`)を入れる。空間を広げたら true(0 と見なす大きさは `tol`)。
    ///
    /// 回転の後の残りの大きさは、行の空間からの距離そのものではない(前の行の丸めの誤差が
    /// 何倍かになって残る)。残りが `tol` の [`SUSPECT`] 倍以下なら、入れる前の分解に戻して
    /// 行の空間への写しで距離を求め直し、それが `tol` 以下なら広げない。
    fn insert_row(&mut self, row: &Row, zw: f64, tol: f64) -> bool {
        let mut w: Row = row
            .iter()
            .filter(|(_, v)| *v != 0.0)
            .map(|&(c, v)| (self.pos[c], v))
            .collect();
        w.sort_unstable_by_key(|e| e.0);
        let mut log = vec![];
        let grew = match insert(&mut self.r, &mut self.z, w.clone(), zw, tol, Some(&mut log)) {
            Some(rest) if rest <= SUSPECT * tol => {
                for (p, a, za) in log.into_iter().rev() {
                    self.r[p] = a;
                    self.z[p] = za;
                }
                if self.distance(&w) <= tol {
                    false
                } else {
                    insert(&mut self.r, &mut self.z, w, zw, tol, None).is_some()
                }
            }
            got => got.is_some(),
        };
        self.grew.push(grew);
        if grew {
            self.rank += 1;
            self.s = None;
        }
        grew
    }

    /// 行 `w`(中の列の番号は並べた位置)の、R の行の空間からの距離(2 回外して丸めの誤差を抑える)。
    fn distance(&mut self, w: &Row) -> f64 {
        let mut v = vec![0.0; self.n];
        for &(q, x) in w {
            v[q] = x;
        }
        for _ in 0..2 {
            let mut u = vec![0.0; self.n];
            for (k, row) in self.r.iter().enumerate() {
                for &(q, x) in row.iter().flatten() {
                    u[k] += x * v[q];
                }
            }
            let d = self.through_rrt(u);
            v.iter_mut().zip(&d).for_each(|(a, b)| *a -= b);
        }
        v.iter().map(|x| x * x).sum::<f64>().sqrt()
    }

    /// 行を足す。前の行の張る空間を広げたら true(0 と見なす大きさは [`Self::new`] と同じ)。
    pub fn add(&mut self, row: &Row) -> bool {
        self.insert_row(row, 0.0, self.tol)
    }

    /// 行を長さ 1 にして足す。0 と見なす大きさを `rest` にする(長さが 1e-12 未満の行は広げない)。
    pub fn add_unit(&mut self, row: &Row, rest: f64) -> bool {
        let len = norm(row);
        if len < 1e-12 {
            self.grew.push(false);
            return false;
        }
        let unit: Row = row.iter().map(|&(c, v)| (c, v / len)).collect();
        self.insert_row(&unit, 0.0, rest)
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
                insert(&mut s, &mut zz, col, 0.0, 0.0, None);
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
        let z = self.z.clone();
        let d = self.through_rrt(z);
        (0..self.n).map(|c| d[self.pos[c]]).collect()
    }

    /// `v`(元の列の順)の行の空間への直交の写し。
    pub fn project(&mut self, v: &[f64]) -> Vec<f64> {
        // R v を Rᵀ (R Rᵀ)⁻¹ に通す
        let mut u = vec![0.0; self.n];
        for (k, row) in self.r.iter().enumerate() {
            for &(q, x) in row.iter().flatten() {
                u[k] += x * v[self.col[q]];
            }
        }
        let d = self.through_rrt(u);
        (0..self.n).map(|c| d[self.pos[c]]).collect()
    }

    /// Rᵀ (R Rᵀ)⁻¹ u(`u` は R の行ごと、返す値は並べた位置ごと)。
    fn through_rrt(&mut self, mut u: Vec<f64>) -> Vec<f64> {
        let n = self.n;
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
        d
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
        // 変数が決まっている ⇔ 単位ベクトルの行を足しても階数が増えない
        let unit_in_space = |v: usize| {
            let mut m = j.clone().insert_row(4, 0.0);
            m[(4, v)] = 1.0;
            crate::system::rank(&m) == crate::system::rank(&j)
        };
        assert_eq!(
            a.determined(),
            (0..5).map(unit_in_space).collect::<Vec<_>>()
        );
        // 後から行を足しても、まとめて分解したのと同じに広がるかが分かる(長さ 1 にして足すのも)
        let rows = sparse(&j);
        let mut b = Factor::empty(&rows, 5);
        let mut c = b.clone();
        assert_eq!(
            rows.iter().map(|r| b.add(r)).collect::<Vec<_>>(),
            [true, true, false, true]
        );
        assert_eq!(
            rows.iter().map(|r| c.add_unit(r, 1e-6)).collect::<Vec<_>>(),
            [true, true, false, true]
        );
        // 行の空間への写し: 写した残りは行と直交し、写しは行の空間にある(写し直しても同じ)
        let v = [0.3, -1.0, 2.0, 0.5, 4.0];
        let p = b.project(&v);
        let rest = DVector::from_vec(v.iter().zip(&p).map(|(x, y)| x - y).collect());
        assert!((&j * &rest).amax() < 1e-12, "{rest}");
        let again = b.project(&p);
        assert!(p.iter().zip(&again).all(|(x, y)| (x - y).abs() < 1e-12));
        let dense = j.clone().svd(true, true).solve(&f, 1e-12).expect("解ける");
        let d = a.min_norm();
        for (x, y) in d.iter().zip(dense.iter()) {
            assert!((x - y).abs() < 1e-12, "{d:?} と {dense}");
        }
    }
}
