//! 代数的マルチグリッド(平滑化した集約、Vaněk・Mandel・Brezina 1996)。共役勾配法の前処理に使う。
//!
//! 四面体の網は格子のように 2 個ずつまとめられないので、行列だけから粗い段を作る:
//!
//! 1. 節点どうしの強いつながり(行列の節点の塊の大きさが、対角の塊に比べて大きい所)を調べる。
//! 2. 強くつながった節点を集めて塊(集約)にする。
//! 3. 塊ごとに「力のかからない動き」(弾性は 3 つの平行移動と 3 つの回転、熱は一様な温度)を
//!    直交にして、粗い段の未知数にする(仮の補間)。
//! 4. 仮の補間を Jacobi で 1 回ならし(P = (I − ω D⁻¹A) P₀)、粗い行列を Pᵀ A P で作る。
//!
//! ならしは Chebyshev の多項式(D⁻¹A の大きい固有値の見積もりから。前と後で同じなので
//! V サイクルが対称になり、共役勾配法の前処理に使える)。一番粗い段は密な行列のコレスキー分解。
//! 解き方は網の大きさに関わらずいつもこの前処理つきの共役勾配法で、直接法と反復法を自動で
//! 切り替えない(docs/避ける形.md)。

use crate::sparse::{Csr, dot};
use nalgebra::{DMatrix, DVector};

/// 一番粗い段の未知数の目安(これ以下なら密な行列で解く)。
const COARSE: usize = 500;
/// 密な行列で解ける未知数の上限。
const DENSE_MAX: usize = 6000;
/// 強いつながりのしきい値(Vaněk らの ε = 0.08、段ごとに半分)。
const THETA: f64 = 0.08;
/// Chebyshev のならしの次数。
const DEGREE: usize = 2;

struct Level {
    a: Csr,
    dinv: Vec<f64>,
    /// D⁻¹A のいちばん大きい固有値の見積もり
    lmax: f64,
    /// 一段粗い段からの補間と、その転置(最後の段には無い)
    p: Option<(Csr, Csr)>,
}

/// 代数的マルチグリッドの段の並び。
pub struct Amg {
    levels: Vec<Level>,
    coarse: nalgebra::Cholesky<f64, nalgebra::Dyn>,
}

/// D⁻¹A のいちばん大きい固有値(べき乗法 15 回。下から近づくので 1.1 倍にする)。
fn spectral_radius(a: &Csr, dinv: &[f64]) -> f64 {
    let n = a.rows;
    let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut v: Vec<f64> = (0..n)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64 + 0.5
        })
        .collect();
    let mut av = vec![0.0; n];
    let mut rho = 1.0;
    for _ in 0..15 {
        let nv = dot(&v, &v).sqrt();
        if nv == 0.0 {
            break;
        }
        v.iter_mut().for_each(|x| *x /= nv);
        a.mul(&v, &mut av);
        for i in 0..n {
            av[i] *= dinv[i];
        }
        rho = dot(&v, &av);
        std::mem::swap(&mut v, &mut av);
    }
    1.1 * rho.max(1e-12)
}

/// 節点の塊(`blocks[i]..blocks[i + 1]` が節点 i の未知数)どうしの強いつながり。
fn strength(a: &Csr, blocks: &[usize], theta: f64) -> Vec<Vec<u32>> {
    let n = blocks.len() - 1;
    let mut node_of = vec![0u32; a.rows];
    for i in 0..n {
        node_of[blocks[i]..blocks[i + 1]].fill(i as u32);
    }
    // 節点の塊の大きさ(フロベニウスのノルムの 2 乗)
    let mut acc = vec![0.0f64; n];
    let mut seen = vec![u32::MAX; n];
    let mut touched: Vec<u32> = Vec::new();
    let mut rows: Vec<Vec<(u32, f64)>> = Vec::with_capacity(n);
    let mut diag = vec![0.0f64; n];
    for i in 0..n {
        touched.clear();
        for r in blocks[i]..blocks[i + 1] {
            for k in a.ptr[r]..a.ptr[r + 1] {
                let j = node_of[a.idx[k] as usize];
                if seen[j as usize] != i as u32 {
                    seen[j as usize] = i as u32;
                    acc[j as usize] = 0.0;
                    touched.push(j);
                }
                acc[j as usize] += a.val[k] * a.val[k];
            }
        }
        let mut row = Vec::with_capacity(touched.len());
        for &j in &touched {
            if j as usize == i {
                diag[i] = acc[i];
            } else if acc[j as usize] > 0.0 {
                row.push((j, acc[j as usize]));
            }
        }
        rows.push(row);
    }
    rows.iter()
        .enumerate()
        .map(|(i, row)| {
            let mut s: Vec<u32> = row
                .iter()
                .filter(|(j, v)| *v > theta * theta * (diag[i] * diag[*j as usize]).sqrt())
                .map(|x| x.0)
                .collect();
            s.sort_unstable();
            s
        })
        .collect()
}

/// 強いつながりから塊を作る(節点ごとの塊の番号。どこにも入らない孤立した節点は `u32::MAX`)。
fn aggregate(strong: &[Vec<u32>]) -> (Vec<u32>, usize) {
    let n = strong.len();
    let mut agg = vec![u32::MAX; n];
    let mut count = 0u32;
    // 1. まわりがどこにも入っていない節点を、まわりごと 1 つの塊に
    for i in 0..n {
        if agg[i] != u32::MAX || strong[i].is_empty() {
            continue;
        }
        if strong[i].iter().all(|j| agg[*j as usize] == u32::MAX) {
            agg[i] = count;
            for &j in &strong[i] {
                agg[j as usize] = count;
            }
            count += 1;
        }
    }
    // 2. 残りは、強くつながった塊に入れる(1 の塊だけに。塊が太りすぎないように)
    let first = agg.clone();
    for i in 0..n {
        if agg[i] != u32::MAX {
            continue;
        }
        if let Some(&j) = strong[i].iter().find(|j| first[**j as usize] != u32::MAX) {
            agg[i] = first[j as usize];
        }
    }
    // 3. まだ残る節点どうしで塊を作る
    for i in 0..n {
        if agg[i] != u32::MAX || strong[i].is_empty() {
            continue;
        }
        agg[i] = count;
        for &j in &strong[i] {
            if agg[j as usize] == u32::MAX {
                agg[j as usize] = count;
            }
        }
        count += 1;
    }
    (agg, count as usize)
}

/// 仮の補間: 塊ごとに力のかからない動き `b`(列ごと)を直交にする(修正 Gram–Schmidt。
/// 塊が小さくて向きが足りない時は、その列を捨てる)。返すのは補間、粗い段の節点の塊、
/// 粗い段の力のかからない動き。
fn tentative(
    rows: usize,
    blocks: &[usize],
    agg: &[u32],
    count: usize,
    b: &[Vec<f64>],
) -> (Csr, Vec<usize>, Vec<Vec<f64>>) {
    let k = b.len();
    let mut members: Vec<Vec<usize>> = vec![Vec::new(); count];
    for (i, a) in agg.iter().enumerate() {
        if *a != u32::MAX {
            members[*a as usize].extend(blocks[i]..blocks[i + 1]);
        }
    }
    // 行ごとの (列, 値)
    let mut entries: Vec<Vec<(u32, f64)>> = vec![Vec::new(); rows];
    let mut cblocks = vec![0usize];
    let mut cb: Vec<Vec<f64>> = vec![Vec::new(); k];
    for dofs in &members {
        let start = *cblocks.last().expect("最初は 0");
        let mut q: Vec<Vec<f64>> = Vec::new();
        let mut r = vec![vec![0.0f64; k]; k];
        for j in 0..k {
            let col: Vec<f64> = dofs.iter().map(|d| b[j][*d]).collect();
            let n0 = dot(&col, &col).sqrt();
            let mut v = col;
            for _ in 0..2 {
                for (i, qi) in q.iter().enumerate() {
                    let c = dot(qi, &v);
                    r[i][j] += c;
                    for (x, y) in v.iter_mut().zip(qi) {
                        *x -= c * y;
                    }
                }
            }
            let nv = dot(&v, &v).sqrt();
            if n0 > 0.0 && nv > 1e-8 * n0 {
                let i = q.len();
                r[i][j] = nv;
                q.push(v.into_iter().map(|x| x / nv).collect());
            }
        }
        for (i, qi) in q.iter().enumerate() {
            for (pos, d) in dofs.iter().enumerate() {
                if qi[pos] != 0.0 {
                    entries[*d].push(((start + i) as u32, qi[pos]));
                }
            }
            for (j, col) in cb.iter_mut().enumerate() {
                col.push(r[i][j]);
            }
        }
        cblocks.push(start + q.len());
    }
    let cols = *cblocks.last().expect("最初は 0");
    let pattern: Vec<Vec<u32>> = entries
        .iter()
        .map(|e| e.iter().map(|x| x.0).collect())
        .collect();
    let mut p = Csr::from_pattern(cols, pattern);
    let mut kk = 0;
    for e in &entries {
        for x in e {
            p.val[kk] = x.1;
            kk += 1;
        }
    }
    (p, cblocks, cb)
}

impl Amg {
    /// 行列 `a`(固定を入れた後。対称・正定値)から段を作る。`blocks` は節点ごとの未知数の
    /// 範囲(長さは節点の数 + 1)、`nullspace` は力のかからない動き(列ごと、長さは未知数の数)。
    pub fn new(a: Csr, blocks: Vec<usize>, nullspace: Vec<Vec<f64>>) -> Result<Self, String> {
        let mut levels = Vec::new();
        let mut a = a;
        let mut blocks = blocks;
        let mut b = nullspace;
        let mut theta = THETA;
        loop {
            let dinv: Vec<f64> = a
                .diag()
                .iter()
                .map(|d| if *d > 0.0 { 1.0 / d } else { 0.0 })
                .collect();
            let lmax = spectral_radius(&a, &dinv);
            if a.rows <= COARSE {
                levels.push(Level {
                    a,
                    dinv,
                    lmax,
                    p: None,
                });
                break;
            }
            let strong = strength(&a, &blocks, theta);
            let (agg, count) = aggregate(&strong);
            let (p0, cblocks, cb) = tentative(a.rows, &blocks, &agg, count, &b);
            if p0.cols == 0 || p0.cols as f64 > 0.7 * a.rows as f64 {
                levels.push(Level {
                    a,
                    dinv,
                    lmax,
                    p: None,
                });
                break;
            }
            // 補間をならす: P = P₀ − ω D⁻¹ A P₀(ω = 4/(3 λmax))
            let ap = a.matmul(&p0);
            let w: Vec<f64> = dinv.iter().map(|d| -4.0 / (3.0 * lmax) * d).collect();
            let p = p0.add_scaled_rows(&ap, &w);
            let pt = p.transpose();
            let ac = pt.matmul(&a.matmul(&p));
            log::debug!(
                "AMG: 段 {} 未知数 {} → {}(塊 {count})",
                levels.len(),
                a.rows,
                ac.rows
            );
            levels.push(Level {
                a,
                dinv,
                lmax,
                p: Some((p, pt)),
            });
            a = ac;
            blocks = cblocks;
            b = cb;
            theta *= 0.5;
        }
        let last = &levels.last().expect("段は 1 つ以上").a;
        let n = last.rows;
        if n > DENSE_MAX {
            return Err(format!(
                "網から粗い段を作れず、一番粗い段の未知数が {n} 個残りました(網がばらばらか、固定が足りないかもしれません)"
            ));
        }
        let mut dense = DMatrix::<f64>::zeros(n, n);
        let mut maxd: f64 = 0.0;
        for r in 0..n {
            for k in last.ptr[r]..last.ptr[r + 1] {
                dense[(r, last.idx[k] as usize)] = last.val[k];
            }
            maxd = maxd.max(dense[(r, r)].abs());
        }
        // 丸めの非対称をならす
        let dense = (&dense + dense.transpose()) * 0.5;
        let coarse = match dense.clone().cholesky() {
            Some(c) => c,
            None => {
                let mut d = dense;
                for r in 0..n {
                    d[(r, r)] += 1e-10 * maxd.max(1e-30);
                }
                d.cholesky()
                    .ok_or("行列が正定値になりません。固定する面が足りないかもしれません")?
            }
        };
        log::info!(
            "代数的マルチグリッド: {} 段、未知数 {} → {}",
            levels.len(),
            levels[0].a.rows,
            n
        );
        Ok(Amg { levels, coarse })
    }

    pub fn matrix(&self) -> &Csr {
        &self.levels[0].a
    }

    /// 前処理: z ≈ A⁻¹ r(V サイクル 1 回)。
    pub fn apply(&self, r: &[f64], z: &mut [f64]) {
        self.vcycle(0, r, z);
    }

    fn vcycle(&self, l: usize, b: &[f64], x: &mut [f64]) {
        let lv = &self.levels[l];
        let Some((p, pt)) = &lv.p else {
            let s = self.coarse.solve(&DVector::from_column_slice(b));
            x.copy_from_slice(s.as_slice());
            return;
        };
        let n = b.len();
        x.iter_mut().for_each(|v| *v = 0.0);
        let mut ax = vec![0.0; n];
        chebyshev(lv, b, x, &mut ax);
        lv.a.mul(x, &mut ax);
        let r: Vec<f64> = b.iter().zip(&ax).map(|(bi, ai)| bi - ai).collect();
        let mut rc = vec![0.0; pt.rows];
        pt.mul(&r, &mut rc);
        let mut ec = vec![0.0; pt.rows];
        self.vcycle(l + 1, &rc, &mut ec);
        let mut e = vec![0.0; n];
        p.mul(&ec, &mut e);
        for (xi, ei) in x.iter_mut().zip(&e) {
            *xi += ei;
        }
        chebyshev(lv, b, x, &mut ax);
    }
}

/// Chebyshev の多項式でならす(区間は [λmax/30, λmax]。Saad『Iterative Methods』12.3 の形)。
fn chebyshev(lv: &Level, b: &[f64], x: &mut [f64], ax: &mut [f64]) {
    let n = b.len();
    let hi = lv.lmax;
    let lo = hi / 30.0;
    let theta = 0.5 * (hi + lo);
    let delta = 0.5 * (hi - lo);
    let sigma = theta / delta;
    let mut rho = 1.0 / sigma;
    lv.a.mul(x, ax);
    let mut d: Vec<f64> = (0..n)
        .map(|i| lv.dinv[i] * (b[i] - ax[i]) / theta)
        .collect();
    for k in 0..DEGREE {
        for i in 0..n {
            x[i] += d[i];
        }
        if k + 1 == DEGREE {
            break;
        }
        lv.a.mul(x, ax);
        let next = 1.0 / (2.0 * sigma - rho);
        for i in 0..n {
            let r = lv.dinv[i] * (b[i] - ax[i]);
            d[i] = next * rho * d[i] + 2.0 * next / delta * r;
        }
        rho = next;
    }
}

/// 弾性の力のかからない動き(3 つの平行移動と 3 つの回転。節点ごとに x, y, z の 3 つの未知数)。
/// `fixed[d]` の未知数は 0 にする。
pub fn rigid_modes(points: &[[f64; 3]], fixed: &[bool]) -> Vec<Vec<f64>> {
    let n = points.len();
    let mut c = [0.0; 3];
    let mut lo = [f64::MAX; 3];
    let mut hi = [f64::MIN; 3];
    for p in points {
        for a in 0..3 {
            c[a] += p[a] / n.max(1) as f64;
            lo[a] = lo[a].min(p[a]);
            hi[a] = hi[a].max(p[a]);
        }
    }
    let size = (0..3).map(|a| hi[a] - lo[a]).fold(1e-9, f64::max);
    let mut b = vec![vec![0.0; 3 * n]; 6];
    for (i, p) in points.iter().enumerate() {
        let [x, y, z] = [0, 1, 2].map(|a| (p[a] - c[a]) / size);
        for a in 0..3 {
            b[a][3 * i + a] = 1.0;
        }
        // z 軸・x 軸・y 軸のまわりの回転
        b[3][3 * i] = -y;
        b[3][3 * i + 1] = x;
        b[4][3 * i + 1] = -z;
        b[4][3 * i + 2] = y;
        b[5][3 * i] = z;
        b[5][3 * i + 2] = -x;
    }
    for col in &mut b {
        for (v, f) in col.iter_mut().zip(fixed) {
            if *f {
                *v = 0.0;
            }
        }
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solve::pcg;

    /// 1 次元のラプラシアン(両端固定)を AMG の前処理で解くと、少ない回数で収束する。
    #[test]
    fn a_long_laplacian_converges_quickly() {
        let n = 20_000;
        let pattern: Vec<Vec<u32>> = (0..n)
            .map(|i| {
                let mut v = Vec::new();
                if i > 0 {
                    v.push(i as u32 - 1);
                }
                v.push(i as u32);
                if i + 1 < n {
                    v.push(i as u32 + 1);
                }
                v
            })
            .collect();
        let mut a = Csr::from_pattern(n, pattern);
        for i in 0..n {
            a.add(i, i as u32, 2.0);
            if i > 0 {
                a.add(i, i as u32 - 1, -1.0);
            }
            if i + 1 < n {
                a.add(i, i as u32 + 1, -1.0);
            }
        }
        let blocks: Vec<usize> = (0..=n).collect();
        let amg = Amg::new(a.clone(), blocks, vec![vec![1.0; n]]).unwrap();
        let b = vec![1.0; n];
        let mut x = vec![0.0; n];
        let rep = pcg(&a, &b, &mut x, &|r, z| amg.apply(r, z), 1e-8, 200).unwrap();
        assert!(rep.iterations < 40, "{} 回", rep.iterations);
        // 解は x_i = (i+1)(n−i)/2
        let i = n / 2;
        let want = (i + 1) as f64 * (n - i) as f64 / 2.0;
        assert!((x[i] - want).abs() / want < 1e-6);
    }
}
