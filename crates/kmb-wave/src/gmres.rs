//! 再始動つきの GMRES(右から対角で前処理する)。残りは前処理の無い本当の残りと同じ量になる。

use num_complex::Complex64 as C;

/// 反復の結果。
pub(crate) struct Gmres {
    pub x: Vec<C>,
    pub iterations: usize,
    /// 右辺の大きさに対する残りの比
    pub residual: f64,
    /// 中止されたか
    pub cancelled: bool,
}

/// A x = b を解く。`matvec(v, out)` は out = A v、`diag_inv` は対角の逆数(前処理)。
/// `stop()` が真を返したら、その時の x で止める。
pub(crate) fn solve(
    b: &[C],
    diag_inv: &[C],
    matvec: &dyn Fn(&[C], &mut [C]),
    restart: usize,
    max_iterations: usize,
    tolerance: f64,
    stop: &dyn Fn() -> bool,
) -> Gmres {
    let n = b.len();
    let mut x = vec![C::new(0.0, 0.0); n];
    let bnorm = norm(b);
    if bnorm == 0.0 {
        return Gmres {
            x,
            iterations: 0,
            residual: 0.0,
            cancelled: false,
        };
    }
    let m = restart.max(1).min(n.max(1));
    let mut iterations = 0;
    let mut residual;
    let mut work = vec![C::new(0.0, 0.0); n];
    let mut tmp = vec![C::new(0.0, 0.0); n];
    loop {
        // r = b − A x
        matvec(&x, &mut work);
        let r: Vec<C> = b.iter().zip(&work).map(|(b, a)| b - a).collect();
        let beta = norm(&r);
        residual = beta / bnorm;
        if residual <= tolerance || iterations >= max_iterations {
            break;
        }
        if stop() {
            return Gmres {
                x,
                iterations,
                residual,
                cancelled: true,
            };
        }
        let mut v: Vec<Vec<C>> = vec![r.iter().map(|z| z / beta).collect()];
        // h は列ごと(長さ j+2)
        let mut h: Vec<Vec<C>> = vec![];
        let mut cs: Vec<f64> = vec![];
        let mut sn: Vec<C> = vec![];
        let mut g = vec![C::new(0.0, 0.0); m + 1];
        g[0] = C::new(beta, 0.0);
        let mut used = 0;
        for j in 0..m {
            // w = A M⁻¹ v_j
            for (t, (vj, d)) in tmp.iter_mut().zip(v[j].iter().zip(diag_inv)) {
                *t = vj * d;
            }
            matvec(&tmp, &mut work);
            let mut w = work.clone();
            let mut col = vec![C::new(0.0, 0.0); j + 2];
            // 修正グラム・シュミット
            for (i, vi) in v.iter().enumerate() {
                let hij = dotc(vi, &w);
                col[i] = hij;
                for (wk, vk) in w.iter_mut().zip(vi) {
                    *wk -= hij * vk;
                }
            }
            let hn = norm(&w);
            col[j + 1] = C::new(hn, 0.0);
            // これまでの回転をかける
            for i in 0..j {
                let (a, b) = (col[i], col[i + 1]);
                col[i] = cs[i] * a + sn[i] * b;
                col[i + 1] = -sn[i].conj() * a + cs[i] * b;
            }
            let (c, s) = givens(col[j], col[j + 1]);
            col[j] = c * col[j] + s * col[j + 1];
            col[j + 1] = C::new(0.0, 0.0);
            g[j + 1] = -s.conj() * g[j];
            g[j] *= c;
            cs.push(c);
            sn.push(s);
            h.push(col);
            used = j + 1;
            iterations += 1;
            residual = g[j + 1].norm() / bnorm;
            if residual <= tolerance || iterations >= max_iterations || hn == 0.0 || stop() {
                break;
            }
            v.push(w.iter().map(|z| z / hn).collect());
        }
        // 上三角の H y = g を後ろから解き、x += M⁻¹ V y
        let mut y = vec![C::new(0.0, 0.0); used];
        for i in (0..used).rev() {
            let mut s = g[i];
            for (k, yk) in y.iter().enumerate().skip(i + 1) {
                s -= h[k][i] * yk;
            }
            y[i] = s / h[i][i];
        }
        for (i, yi) in y.iter().enumerate() {
            for ((xk, vk), d) in x.iter_mut().zip(&v[i]).zip(diag_inv) {
                *xk += yi * vk * d;
            }
        }
        if stop() {
            matvec(&x, &mut work);
            let r: f64 = b
                .iter()
                .zip(&work)
                .map(|(b, a)| (b - a).norm_sqr())
                .sum::<f64>()
                .sqrt();
            return Gmres {
                x,
                iterations,
                residual: r / bnorm,
                cancelled: true,
            };
        }
    }
    Gmres {
        x,
        iterations,
        residual,
        cancelled: false,
    }
}

/// (a, b) の b を 0 にする回転 [c, s; −s̄, c]。
fn givens(a: C, b: C) -> (f64, C) {
    let d = (a.norm_sqr() + b.norm_sqr()).sqrt();
    if d == 0.0 {
        return (1.0, C::new(0.0, 0.0));
    }
    if a.norm() == 0.0 {
        return (0.0, b.conj() / b.norm());
    }
    let c = a.norm() / d;
    let s = a / a.norm() * b.conj() / d;
    (c, s)
}

fn dotc(a: &[C], b: &[C]) -> C {
    a.iter().zip(b).map(|(a, b)| a.conj() * b).sum()
}

fn norm(a: &[C]) -> f64 {
    a.iter().map(|z| z.norm_sqr()).sum::<f64>().sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 確かめること: 対角が強い複素の行列で、再始動をはさんでも答えに収まる。
    #[test]
    fn solves_a_small_complex_system_across_restarts() {
        let n = 30;
        let a: Vec<C> = (0..n * n)
            .map(|k| {
                let (i, j) = (k / n, k % n);
                if i == j {
                    C::new(4.0 + i as f64 * 0.1, 1.0)
                } else {
                    C::new(
                        ((i * 7 + j * 3) % 5) as f64 * 0.05,
                        ((i + 2 * j) % 3) as f64 * -0.04,
                    )
                }
            })
            .collect();
        let want: Vec<C> = (0..n)
            .map(|i| C::new(i as f64, 1.0 - i as f64 * 0.5))
            .collect();
        let mv = |v: &[C], out: &mut [C]| {
            for i in 0..n {
                out[i] = (0..n).map(|j| a[i * n + j] * v[j]).sum();
            }
        };
        let mut b = vec![C::new(0.0, 0.0); n];
        mv(&want, &mut b);
        let d: Vec<C> = (0..n).map(|i| 1.0 / a[i * n + i]).collect();
        let out = solve(&b, &d, &mv, 4, 500, 1e-10, &|| false);
        assert!(out.residual <= 1e-10);
        for (x, w) in out.x.iter().zip(&want) {
            assert!((x - w).norm() < 1e-8);
        }
    }
}
