//! 自由曲線の計算: 楕円(弧)・B スプライン・円錐曲線(有理 2 次ベジエ)の点と微分、
//! 最も近い点、節点挿入による分割、線分と円弧の並び(バイアーク)への近似。
//!
//! どの曲線もパラメータ `t` で表す([`Shape::domain`] の範囲)。楕円は離心角(中心のまわりに
//! 反時計回り)、フィット点のスプラインは弦長を 0〜1 にしたもの、円錐曲線は 0〜1。

use kmb_core::Segment;
use std::f64::consts::{FRAC_PI_2, TAU};

pub use crate::vec2::P2;
use crate::vec2::{add, cross, dist, dot, len, lerp, scale, sub};

/// 近似の許容差(mm)。CNC の円弧補間に渡しても形が変わらない程度。
pub const ARC_TOLERANCE: f64 = 0.002;

/// B スプライン(制御点・節点・次数)。節点の数は制御点の数 + 次数 + 1。
#[derive(Debug, Clone, PartialEq)]
pub struct BSpline {
    pub degree: usize,
    pub ctrl: Vec<P2>,
    pub knots: Vec<f64>,
}

impl BSpline {
    /// 形として正しいか(数がそろい、節点が減らない)。
    fn valid(&self) -> bool {
        let (p, n) = (self.degree, self.ctrl.len());
        n > p
            && self.knots.len() == n + p + 1
            && self.knots.windows(2).all(|w| w[0] <= w[1])
            && self.knots[n] > self.knots[p]
            && self.knots.iter().all(|k| k.is_finite())
            && self
                .ctrl
                .iter()
                .all(|c| c[0].is_finite() && c[1].is_finite())
    }

    pub fn domain(&self) -> (f64, f64) {
        (self.knots[self.degree], self.knots[self.ctrl.len()])
    }

    /// `knots[k] <= t < knots[k + 1]` の区間 `k`(範囲の外は端の区間)。
    fn span(&self, t: f64) -> usize {
        let (p, n) = (self.degree, self.ctrl.len());
        let u = &self.knots;
        let mut last = p;
        for k in p..n {
            if u[k] < u[k + 1] {
                if t < u[k + 1] {
                    return k;
                }
                last = k;
            }
        }
        last
    }

    /// 区間 `k` での基底関数とその微分(`nd` 階まで)。The NURBS Book の A2.3 による。
    fn ders_basis(&self, k: usize, t: f64, nd: usize) -> Vec<Vec<f64>> {
        let p = self.degree;
        let u = &self.knots;
        let mut out = vec![vec![0.0; p + 1]; nd + 1];
        let nd = nd.min(p);
        let mut ndu = vec![vec![0.0; p + 1]; p + 1];
        let mut left = vec![0.0; p + 1];
        let mut right = vec![0.0; p + 1];
        ndu[0][0] = 1.0;
        for j in 1..=p {
            left[j] = t - u[k + 1 - j];
            right[j] = u[k + j] - t;
            let mut saved = 0.0;
            for r in 0..j {
                ndu[j][r] = right[r + 1] + left[j - r];
                let temp = ndu[r][j - 1] / ndu[j][r];
                ndu[r][j] = saved + right[r + 1] * temp;
                saved = left[j - r] * temp;
            }
            ndu[j][j] = saved;
        }
        for j in 0..=p {
            out[0][j] = ndu[j][p];
        }
        let mut a = [vec![0.0; p + 1], vec![0.0; p + 1]];
        for r in 0..=p {
            let (mut s1, mut s2) = (0, 1);
            a[0][0] = 1.0;
            for kk in 1..=nd {
                let mut d = 0.0;
                let rk = r as isize - kk as isize;
                let pk = p - kk;
                if rk >= 0 {
                    a[s2][0] = a[s1][0] / ndu[pk + 1][rk as usize];
                    d = a[s2][0] * ndu[rk as usize][pk];
                }
                let j1: usize = if rk >= -1 { 1 } else { (-rk) as usize };
                let j2: usize = if r as isize - 1 <= pk as isize {
                    kk - 1
                } else {
                    p - r
                };
                let mut j = j1;
                while j <= j2 {
                    let idx = (rk + j as isize) as usize;
                    a[s2][j] = (a[s1][j] - a[s1][j - 1]) / ndu[pk + 1][idx];
                    d += a[s2][j] * ndu[idx][pk];
                    j += 1;
                }
                if r <= pk {
                    a[s2][kk] = -a[s1][kk - 1] / ndu[pk + 1][r];
                    d += a[s2][kk] * ndu[r][pk];
                }
                out[kk][r] = d;
                std::mem::swap(&mut s1, &mut s2);
            }
        }
        let mut f = p as f64;
        for (kk, row) in out.iter_mut().enumerate().take(nd + 1).skip(1) {
            for v in row.iter_mut() {
                *v *= f;
            }
            f *= (p - kk) as f64;
        }
        out
    }

    /// 点と 1 階・2 階の微分。
    pub fn ders(&self, t: f64) -> [P2; 3] {
        let p = self.degree;
        let k = self.span(t);
        let b = self.ders_basis(k, t, 2);
        let mut out = [[0.0; 2]; 3];
        for (d, row) in b.iter().enumerate() {
            for (j, w) in row.iter().enumerate() {
                let c = self.ctrl[k - p + j];
                out[d][0] += w * c[0];
                out[d][1] += w * c[1];
            }
        }
        out
    }

    /// 節点 `t` を 1 つ足す(形は変わらない。Boehm の方法)。
    fn insert(&mut self, t: f64) {
        let p = self.degree;
        let k = self.span(t);
        let u = &self.knots;
        let mut q = Vec::with_capacity(self.ctrl.len() + 1);
        for i in 0..=self.ctrl.len() {
            let c = if i + p <= k {
                self.ctrl[i]
            } else if i > k {
                self.ctrl[i - 1]
            } else {
                let den = u[i + p] - u[i];
                let a = if den.abs() < 1e-300 {
                    0.0
                } else {
                    (t - u[i]) / den
                };
                lerp(self.ctrl[i - 1], self.ctrl[i], a)
            };
            q.push(c);
        }
        self.ctrl = q;
        self.knots.insert(k + 1, t);
    }

    /// 節点 `t` の重なりを `m` 以上にする(近い節点があればそれに寄せる)。寄せた値を返す。
    fn raise(&mut self, t: f64, m: usize) -> f64 {
        let (lo, hi) = self.domain();
        let tol = 1e-12 * (hi - lo).abs().max(1.0);
        let t = self
            .knots
            .iter()
            .copied()
            .find(|k| (k - t).abs() <= tol)
            .unwrap_or(t);
        let have = self.knots.iter().filter(|k| **k == t).count();
        for _ in have..m {
            self.insert(t);
        }
        t
    }

    /// `t0`〜`t1` の部分を、両端の clamped な B スプラインにする(形は変わらない)。
    pub fn extract(&self, t0: f64, t1: f64) -> Option<BSpline> {
        let p = self.degree;
        let (lo, hi) = self.domain();
        let (t0, t1) = (t0.clamp(lo, hi), t1.clamp(lo, hi));
        if t1 - t0 <= 1e-12 * (hi - lo).max(1.0) {
            return None;
        }
        let mut s = self.clone();
        let t0 = s.raise(t0, p);
        let t1 = s.raise(t1, p);
        let last0 = s.knots.iter().rposition(|k| *k == t0)?;
        let first1 = s.knots.iter().position(|k| *k == t1)?;
        if last0 < p || first1 == 0 {
            return None;
        }
        let ctrl = s.ctrl[last0 - p..first1].to_vec();
        let mut knots = vec![t0; p + 1];
        knots.extend(s.knots[last0 + 1..first1].iter().copied());
        knots.extend(std::iter::repeat_n(t1, p + 1));
        let out = BSpline {
            degree: p,
            ctrl,
            knots,
        };
        out.valid().then_some(out)
    }

    /// 終点で始点につながる 2 本(どちらも clamped、同じ次数)を 1 本にする。
    pub fn join(a: &BSpline, b: &BSpline) -> BSpline {
        let p = a.degree;
        let (_, ea) = a.domain();
        let (sb, _) = b.domain();
        let shift = ea - sb;
        let mut ctrl = a.ctrl.clone();
        ctrl.extend(b.ctrl[1..].iter().copied());
        let mut knots = a.knots[..a.knots.len() - 1].to_vec();
        knots.extend(b.knots[p + 1..].iter().map(|k| k + shift));
        BSpline {
            degree: p,
            ctrl,
            knots,
        }
    }

    /// 区間の切れ目(範囲の中の異なる節点、両端を含む)。
    fn breaks(&self) -> Vec<f64> {
        let (lo, hi) = self.domain();
        let mut v: Vec<f64> = self
            .knots
            .iter()
            .copied()
            .filter(|k| *k >= lo && *k <= hi)
            .collect();
        v.dedup();
        v
    }
}

/// 制御点 `n` 個、次数 `p` の等間隔の clamped な節点(0〜1)。
pub fn clamped_knots(n: usize, p: usize) -> Vec<f64> {
    let inner = n - p;
    let mut k = vec![0.0; p + 1];
    k.extend((1..inner).map(|i| i as f64 / inner as f64));
    k.extend(std::iter::repeat_n(1.0, p + 1));
    k
}

/// 閉じた B スプライン(制御点 `m` 個を周期につなぐ)。節点は等間隔で、範囲は 0〜1。
fn periodic_uniform(ctrl: &[P2], p: usize) -> Option<BSpline> {
    let m = ctrl.len();
    if m < 2 {
        return None;
    }
    let mut c = ctrl.to_vec();
    c.extend((0..p).map(|i| ctrl[i % m]));
    let knots = (0..m + 2 * p + 1)
        .map(|j| (j as f64 - p as f64) / m as f64)
        .collect();
    let s = BSpline {
        degree: p,
        ctrl: c,
        knots,
    };
    s.valid().then_some(s)
}

/// 点列の弦長のパラメータ(0〜1)。`closed` なら最後の点から最初の点への弦も足す(長さ n + 1)。
fn chord_params(q: &[P2], closed: bool) -> Option<Vec<f64>> {
    let n = q.len();
    let mut u = vec![0.0];
    let segs = if closed { n } else { n - 1 };
    for i in 0..segs {
        let d = dist(q[i], q[(i + 1) % n]);
        u.push(u[i] + d);
    }
    let total = *u.last()?;
    if total < 1e-9 {
        return None;
    }
    // 重なった点は、弦長 0 で割れないよう少しだけ離す
    let min = 1e-6 * total;
    for i in 1..u.len() {
        if u[i] - u[i - 1] < min {
            u[i] = u[i - 1] + min;
        }
    }
    let total = *u.last()?;
    Some(u.iter().map(|x| x / total).collect())
}

/// 連立 1 次方程式 `a x = b`(右辺は x と y の 2 列)を解く。
fn solve_dense(a: nalgebra::DMatrix<f64>, b: nalgebra::DMatrix<f64>) -> Option<Vec<P2>> {
    let x = a.lu().solve(&b)?;
    let out: Vec<P2> = (0..x.nrows()).map(|i| [x[(i, 0)], x[(i, 1)]]).collect();
    out.iter()
        .all(|p| p[0].is_finite() && p[1].is_finite())
        .then_some(out)
}

/// 点 `q` を通る 3 次の B スプライン(弦長のパラメータ)。端は not-a-knot の条件(端から 2 つ目の
/// 点で 3 階微分も続く)。自然条件(端で 2 階微分が 0)だと端の曲率が必ず 0 になり、円弧と
/// 曲率をそろえる拘束(G2)が満たせないため。点が 3 つなら 2 次、2 つなら線分になる。
pub fn interpolate_open(q: &[P2]) -> Option<BSpline> {
    let m = q.len();
    if m < 2 {
        return None;
    }
    let p = (m - 1).min(3);
    let u = chord_params(q, false)?;
    let mut knots = vec![0.0; p + 1];
    if p == 3 {
        knots.extend(u[2..m - 2].iter().copied());
    }
    knots.extend(std::iter::repeat_n(1.0, p + 1));
    let mut s = BSpline {
        degree: p,
        ctrl: vec![[0.0; 2]; m],
        knots,
    };
    let mut a = nalgebra::DMatrix::zeros(m, m);
    let mut b = nalgebra::DMatrix::zeros(m, 2);
    for (i, t) in u.iter().enumerate() {
        let k = s.span(*t);
        let d = s.ders_basis(k, *t, 0);
        for j in 0..=p {
            a[(i, k - p + j)] += d[0][j];
        }
        b[(i, 0)] = q[i][0];
        b[(i, 1)] = q[i][1];
    }
    s.ctrl = solve_dense(a, b)?;
    s.valid().then_some(s)
}

/// 点 `q` を通る閉じた 3 次の B スプライン(弦長のパラメータ、周期)。
pub fn interpolate_closed(q: &[P2]) -> Option<BSpline> {
    let n = q.len();
    if n < 3 {
        return None;
    }
    let p = 3;
    let u = chord_params(q, true)?;
    // 周期に延ばした節点: t_j = U(j - p)、U(i) = u[i mod n] + (i div n)
    let big_u = |i: isize| {
        let n = n as isize;
        let r = i.rem_euclid(n);
        let k = i.div_euclid(n);
        u[r as usize] + k as f64
    };
    let knots: Vec<f64> = (0..n + 2 * p + 1)
        .map(|j| big_u(j as isize - p as isize))
        .collect();
    let mut s = BSpline {
        degree: p,
        ctrl: vec![[0.0; 2]; n + p],
        knots,
    };
    let mut a = nalgebra::DMatrix::zeros(n, n);
    let mut b = nalgebra::DMatrix::zeros(n, 2);
    for i in 0..n {
        let t = u[i];
        let k = s.span(t);
        let d = s.ders_basis(k, t, 0);
        for j in 0..=p {
            a[(i, (k - p + j) % n)] += d[0][j];
        }
        b[(i, 0)] = q[i][0];
        b[(i, 1)] = q[i][1];
    }
    let c = solve_dense(a, b)?;
    s.ctrl = (0..n + p).map(|i| c[i % n]).collect();
    s.valid().then_some(s)
}

/// 形だけの曲線。
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Line(P2, P2),
    /// 中心、半径、始まりの角度、反時計回りの開き(円は TAU で閉じる)
    Arc {
        c: P2,
        r: f64,
        a0: f64,
        sweep: f64,
        closed: bool,
    },
    /// 中心、長軸の向きと長さ `u`、短軸 `v`(`u` を反時計回りに 90° 回した向き)、
    /// 離心角の始まりと開き
    Ellipse {
        c: P2,
        u: P2,
        v: P2,
        t0: f64,
        sweep: f64,
        closed: bool,
    },
    Spline {
        s: BSpline,
        closed: bool,
    },
    /// 有理 2 次ベジエ(始点・頂点・終点と、頂点の重み)
    Conic {
        p: [P2; 3],
        w: f64,
    },
}

/// 楕円の点(中心 `c`、長軸の端 `major`、短軸の端 `minor`)から、長軸 `u` と短軸 `v`。
pub fn ellipse_axes(c: P2, major: P2, minor: P2) -> Option<(P2, P2)> {
    let u = sub(major, c);
    let a = len(u);
    let b = dist(minor, c);
    if a < 1e-9 || b < 1e-9 {
        return None;
    }
    Some((u, scale([-u[1], u[0]], b / a)))
}

/// 楕円の上の点 `p` の離心角。
pub fn ellipse_angle(c: P2, u: P2, v: P2, p: P2) -> f64 {
    let d = sub(p, c);
    let x = dot(d, u) / dot(u, u);
    let y = dot(d, v) / dot(v, v);
    y.atan2(x)
}

impl Shape {
    /// 円弧(中心、始点、終点。反時計回り)。
    pub fn arc(c: P2, s: P2, e: P2) -> Option<Shape> {
        let r = dist(s, c);
        if r < 1e-12 {
            return None;
        }
        let a0 = (s[1] - c[1]).atan2(s[0] - c[0]);
        let a1 = (e[1] - c[1]).atan2(e[0] - c[0]);
        let mut sweep = (a1 - a0).rem_euclid(TAU);
        if sweep < 1e-12 {
            sweep = TAU;
        }
        Some(Shape::Arc {
            c,
            r,
            a0,
            sweep,
            closed: false,
        })
    }

    pub fn circle(c: P2, r: f64) -> Option<Shape> {
        (r > 1e-12).then_some(Shape::Arc {
            c,
            r,
            a0: 0.0,
            sweep: TAU,
            closed: true,
        })
    }

    /// 楕円(`ends` があれば始点から終点へ反時計回りの楕円弧)。
    pub fn ellipse(c: P2, major: P2, minor: P2, ends: Option<(P2, P2)>) -> Option<Shape> {
        let (u, v) = ellipse_axes(c, major, minor)?;
        Some(match ends {
            None => Shape::Ellipse {
                c,
                u,
                v,
                t0: 0.0,
                sweep: TAU,
                closed: true,
            },
            Some((s, e)) => {
                let t0 = ellipse_angle(c, u, v, s);
                let mut sweep = (ellipse_angle(c, u, v, e) - t0).rem_euclid(TAU);
                if sweep < 1e-12 {
                    sweep = TAU;
                }
                Shape::Ellipse {
                    c,
                    u,
                    v,
                    t0,
                    sweep,
                    closed: false,
                }
            }
        })
    }

    /// スプライン。`control` なら点は制御点、そうでなければ通る点。
    pub fn spline(
        pts: &[P2],
        control: bool,
        closed: bool,
        degree: u8,
        knots: Option<&[f64]>,
    ) -> Option<Shape> {
        let s = match (control, closed) {
            (false, false) => interpolate_open(pts)?,
            (false, true) => interpolate_closed(pts)?,
            (true, true) => {
                let p = (degree.max(1) as usize)
                    .min(pts.len().saturating_sub(1))
                    .max(1);
                periodic_uniform(pts, p)?
            }
            (true, false) => {
                let n = pts.len();
                if n < 2 {
                    return None;
                }
                let p = (degree.max(1) as usize).min(n - 1);
                let given = knots
                    .filter(|k| k.len() == n + p + 1)
                    .map(|k| BSpline {
                        degree: p,
                        ctrl: pts.to_vec(),
                        knots: k.to_vec(),
                    })
                    .filter(BSpline::valid);
                match given {
                    Some(s) => s,
                    None => {
                        let s = BSpline {
                            degree: p,
                            ctrl: pts.to_vec(),
                            knots: clamped_knots(n, p),
                        };
                        if !s.valid() {
                            return None;
                        }
                        s
                    }
                }
            }
        };
        Some(Shape::Spline { s, closed })
    }

    /// 円錐曲線(始点・終点・頂点と rho)。
    pub fn conic(s: P2, e: P2, apex: P2, rho: f64) -> Option<Shape> {
        if !(rho > 0.0 && rho < 1.0) || dist(s, e) < 1e-12 {
            return None;
        }
        Some(Shape::Conic {
            p: [s, apex, e],
            w: rho / (1.0 - rho),
        })
    }

    pub fn domain(&self) -> (f64, f64) {
        match self {
            Shape::Line(..) | Shape::Conic { .. } => (0.0, 1.0),
            Shape::Arc { a0, sweep, .. } => (*a0, a0 + sweep),
            Shape::Ellipse { t0, sweep, .. } => (*t0, t0 + sweep),
            Shape::Spline { s, .. } => s.domain(),
        }
    }

    pub fn closed(&self) -> bool {
        match self {
            Shape::Line(..) | Shape::Conic { .. } => false,
            Shape::Arc { closed, .. }
            | Shape::Ellipse { closed, .. }
            | Shape::Spline { closed, .. } => *closed,
        }
    }

    /// 閉じた曲線なら `t` を範囲の中へ折り返す。
    pub fn wrap(&self, t: f64) -> f64 {
        if !self.closed() {
            return t;
        }
        let (lo, hi) = self.domain();
        lo + (t - lo).rem_euclid(hi - lo)
    }

    /// 点と 1 階・2 階の微分。
    pub fn ders(&self, t: f64) -> [P2; 3] {
        match self {
            Shape::Line(a, b) => [lerp(*a, *b, t), sub(*b, *a), [0.0; 2]],
            Shape::Arc { c, r, .. } => {
                let (s, co) = t.sin_cos();
                [
                    [c[0] + r * co, c[1] + r * s],
                    [-r * s, r * co],
                    [-r * co, -r * s],
                ]
            }
            Shape::Ellipse { c, u, v, .. } => {
                let (s, co) = t.sin_cos();
                [
                    add(*c, add(scale(*u, co), scale(*v, s))),
                    add(scale(*u, -s), scale(*v, co)),
                    add(scale(*u, -co), scale(*v, -s)),
                ]
            }
            Shape::Spline { s, .. } => s.ders(self.wrap(t)),
            Shape::Conic { p, w } => {
                let ws = [1.0, *w, 1.0];
                let b = [(1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t];
                let b1 = [-2.0 * (1.0 - t), 2.0 - 4.0 * t, 2.0 * t];
                let b2 = [2.0, -4.0, 2.0];
                let mut n = [[0.0; 2]; 3];
                let mut d = [0.0; 3];
                for i in 0..3 {
                    for (k, bb) in [b, b1, b2].iter().enumerate() {
                        n[k][0] += bb[i] * ws[i] * p[i][0];
                        n[k][1] += bb[i] * ws[i] * p[i][1];
                        d[k] += bb[i] * ws[i];
                    }
                }
                let c = scale(n[0], 1.0 / d[0]);
                let c1 = scale(sub(n[1], scale(c, d[1])), 1.0 / d[0]);
                let c2 = scale(
                    sub(sub(n[2], scale(c1, 2.0 * d[1])), scale(c, d[2])),
                    1.0 / d[0],
                );
                [c, c1, c2]
            }
        }
    }

    pub fn at(&self, t: f64) -> P2 {
        self.ders(t)[0]
    }

    /// 進む向きの単位ベクトル(微分が 0 なら、少し先の点への向き)。
    pub fn tangent(&self, t: f64) -> P2 {
        let d = self.ders(t)[1];
        let l = len(d);
        if l > 1e-12 {
            return scale(d, 1.0 / l);
        }
        let (lo, hi) = self.domain();
        let h = 1e-6 * (hi - lo);
        let (a, b) = if t + h <= hi {
            (self.at(t), self.at(t + h))
        } else {
            (self.at(t - h), self.at(t))
        };
        let d = sub(b, a);
        let l = len(d).max(1e-300);
        scale(d, 1.0 / l)
    }

    /// 符号つきの曲率(左へ曲がると正)。
    pub fn curvature(&self, t: f64) -> f64 {
        let [_, d1, d2] = self.ders(t);
        let l = len(d1);
        if l < 1e-12 {
            return 0.0;
        }
        cross(d1, d2) / (l * l * l)
    }

    /// 近似や交点の探索で区切る切れ目(両端を含む、増える順)。
    fn breaks(&self) -> Vec<f64> {
        let (lo, hi) = self.domain();
        match self {
            Shape::Spline { s, .. } => s.breaks(),
            Shape::Ellipse { .. } | Shape::Arc { .. } => {
                // 長軸・短軸の端(曲率が最も大きく・小さくなる所)で区切る
                let mut v = vec![lo];
                let mut k = (lo / FRAC_PI_2).floor() + 1.0;
                while k * FRAC_PI_2 < hi - 1e-9 {
                    if k * FRAC_PI_2 > lo + 1e-9 {
                        v.push(k * FRAC_PI_2);
                    }
                    k += 1.0;
                }
                v.push(hi);
                v
            }
            Shape::Conic { .. } => vec![0.0, 0.5, 1.0],
            Shape::Line(..) => vec![0.0, 1.0],
        }
    }

    /// 折れ線(弦と曲線の隔たりが `tol` 以下)。パラメータと点の組で、両端を含む。
    pub fn polyline(&self, tol: f64) -> Vec<(f64, P2)> {
        if let Shape::Line(a, b) = self {
            return vec![(0.0, *a), (1.0, *b)];
        }
        let br = self.breaks();
        let mut out = vec![(br[0], self.at(br[0]))];
        for w in br.windows(2) {
            // 1 区間を 4 つに分けてから、弦の隔たりで細かくする
            let n = 4;
            for i in 0..n {
                let t0 = w[0] + (w[1] - w[0]) * i as f64 / n as f64;
                let t1 = w[0] + (w[1] - w[0]) * (i + 1) as f64 / n as f64;
                self.refine(t0, t1, tol, 0, &mut out);
            }
        }
        out
    }

    fn refine(&self, t0: f64, t1: f64, tol: f64, depth: usize, out: &mut Vec<(f64, P2)>) {
        let (a, b) = (self.at(t0), self.at(t1));
        let tm = (t0 + t1) / 2.0;
        let m = self.at(tm);
        let q1 = self.at((3.0 * t0 + t1) / 4.0);
        let q3 = self.at((t0 + 3.0 * t1) / 4.0);
        let dev = [m, q1, q3]
            .iter()
            .map(|p| segment_distance(a, b, *p))
            .fold(0.0, f64::max);
        if dev <= tol || depth >= 16 {
            out.push((t1, b));
        } else {
            self.refine(t0, tm, tol, depth + 1, out);
            self.refine(tm, t1, tol, depth + 1, out);
        }
    }

    /// 点 `p` に最も近い曲線上の点のパラメータと距離。
    pub fn closest(&self, p: P2) -> (f64, f64) {
        let (lo, hi) = self.domain();
        let samples = self.polyline(0.05 * self.size().max(1e-3));
        let mut best = (lo, f64::INFINITY);
        for w in samples.windows(2) {
            let (a, b) = (w[0].1, w[1].1);
            let d = sub(b, a);
            let dd = dot(d, d);
            let k = if dd > 0.0 {
                (dot(sub(p, a), d) / dd).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let q = lerp(a, b, k);
            let dq = dist(p, q);
            if dq < best.1 {
                best = (w[0].0 + (w[1].0 - w[0].0) * k, dq);
            }
        }
        // ニュートン法で (C(t) - p)・C'(t) = 0 へ寄せる
        let mut t = best.0;
        for _ in 0..12 {
            let [c, d1, d2] = self.ders(t);
            let f = dot(sub(c, p), d1);
            let df = dot(d1, d1) + dot(sub(c, p), d2);
            if df.abs() < 1e-300 {
                break;
            }
            let mut tn = t - f / df;
            if self.closed() {
                tn = self.wrap(tn);
            } else {
                tn = tn.clamp(lo, hi);
            }
            if (tn - t).abs() < 1e-15 * (hi - lo).max(1.0) {
                t = tn;
                break;
            }
            t = tn;
        }
        let d = dist(self.at(t), p);
        if d <= best.1 { (t, d) } else { best }
    }

    /// 形の大きさの目安(制御点・端点を囲む箱の対角)。
    pub fn size(&self) -> f64 {
        let pts: Vec<P2> = match self {
            Shape::Line(a, b) => vec![*a, *b],
            Shape::Arc { c, r, .. } => vec![add(*c, [*r, *r]), sub(*c, [*r, *r])],
            Shape::Ellipse { c, u, v, .. } => vec![add(*c, add(*u, *v)), sub(*c, add(*u, *v))],
            Shape::Spline { s, .. } => s.ctrl.clone(),
            Shape::Conic { p, .. } => p.to_vec(),
        };
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for p in pts {
            for i in 0..2 {
                lo[i] = lo[i].min(p[i]);
                hi[i] = hi[i].max(p[i]);
            }
        }
        dist(lo, hi)
    }

    /// 線分・円弧(円)の並びにする。線分・円弧・円はそのまま、自由曲線はバイアークで
    /// 許容差 `tol` の中に近似する(隣の区間と端と向きがそろう。G1)。
    pub fn segments(&self, tol: f64) -> Vec<Segment> {
        match self {
            Shape::Line(a, b) => return vec![Segment::Line { a: *a, b: *b }],
            Shape::Arc {
                c,
                r,
                a0,
                sweep,
                closed,
            } => {
                return vec![if *closed {
                    Segment::Circle {
                        center: *c,
                        radius: *r,
                    }
                } else {
                    Segment::Arc {
                        center: *c,
                        radius: *r,
                        start: *a0,
                        end: a0 + sweep,
                    }
                }];
            }
            _ => {}
        }
        let br = self.breaks();
        let mut out = Vec::new();
        for w in br.windows(2) {
            self.fit_biarc(w[0], w[1], tol, 0, &mut out);
        }
        out
    }

    fn fit_biarc(&self, t0: f64, t1: f64, tol: f64, depth: usize, out: &mut Vec<Segment>) {
        let (p0, p1) = (self.at(t0), self.at(t1));
        if dist(p0, p1) < 1e-9 {
            return;
        }
        let pair = biarc(p0, self.tangent(t0), p1, self.tangent(t1));
        // 曲線の上の点を細かく取り、近似の線との隔たりを測る
        const K: usize = 12;
        let err = match &pair {
            Some(segs) => (1..K)
                .map(|k| {
                    let q = self.at(t0 + (t1 - t0) * k as f64 / K as f64);
                    segs.iter()
                        .map(|s| segment_point_distance(s, q))
                        .fold(f64::INFINITY, f64::min)
                })
                .fold(0.0, f64::max),
            None => f64::INFINITY,
        };
        if err <= tol || depth >= 20 || (t1 - t0).abs() < 1e-12 {
            match pair {
                Some(segs) => out.extend(segs),
                None => out.push(Segment::Line { a: p0, b: p1 }),
            }
            return;
        }
        let tm = (t0 + t1) / 2.0;
        self.fit_biarc(t0, tm, tol, depth + 1, out);
        self.fit_biarc(tm, t1, tol, depth + 1, out);
    }
}

/// 点 `p` から線分 `a`-`b` までの距離。
pub fn segment_distance(a: P2, b: P2, p: P2) -> f64 {
    let d = sub(b, a);
    let dd = dot(d, d);
    let k = if dd > 0.0 {
        (dot(sub(p, a), d) / dd).clamp(0.0, 1.0)
    } else {
        0.0
    };
    dist(lerp(a, b, k), p)
}

/// 点 `p` から区間(線分・円弧・円)までの距離。
pub fn segment_point_distance(s: &Segment, p: P2) -> f64 {
    match s {
        Segment::Line { a, b } => segment_distance(*a, *b, p),
        Segment::Circle { center, radius } => (dist(p, *center) - radius).abs(),
        Segment::Arc {
            center,
            radius,
            start,
            end,
        } => {
            let sweep = kmb_core::kernel::arc_sweep(*start, *end);
            let ang = (p[1] - center[1]).atan2(p[0] - center[0]);
            if (ang - start).rem_euclid(TAU) <= sweep {
                (dist(p, *center) - radius).abs()
            } else {
                dist(p, s.start_point()).min(dist(p, s.end_point()))
            }
        }
    }
}

/// 点 `p` から向き `t` で出て点 `q` に着く円弧(左へ曲がれば反時計回り)。まっすぐなら線分。
fn arc_from(p: P2, t: P2, q: P2) -> Segment {
    let n = [-t[1], t[0]];
    let chord = sub(q, p);
    let cl = len(chord);
    let den = 2.0 * dot(n, chord);
    // 弦からの膨らみが十分に小さければ線分にする(半径が大きすぎる円弧を作らない)
    if den.abs() <= 1e-12 * cl.max(1.0) {
        return Segment::Line { a: p, b: q };
    }
    let s = cl * cl / den;
    let r = s.abs();
    let sagitta = r - (r * r - cl * cl / 4.0).max(0.0).sqrt();
    if sagitta < 1e-7 || r > 1e7 {
        return Segment::Line { a: p, b: q };
    }
    let c = add(p, scale(n, s));
    let ap = (p[1] - c[1]).atan2(p[0] - c[0]);
    let aq = (q[1] - c[1]).atan2(q[0] - c[0]);
    if s > 0.0 {
        Segment::Arc {
            center: c,
            radius: r,
            start: ap,
            end: ap + (aq - ap).rem_euclid(TAU),
        }
    } else {
        Segment::Arc {
            center: c,
            radius: r,
            start: aq,
            end: aq + (ap - aq).rem_euclid(TAU),
        }
    }
}

/// 両端の点と向きから、つなぎ目で向きのそろう 2 つの円弧(バイアーク。両側の接線の長さを
/// 等しくする作り方)。作れなければ `None`。
pub fn biarc(p0: P2, t0: P2, p1: P2, t1: P2) -> Option<Vec<Segment>> {
    let v = sub(p1, p0);
    let vv = dot(v, v);
    if vv < 1e-18 {
        return None;
    }
    // まっすぐ(両端の向きが弦に沿う)なら線分
    let vl = vv.sqrt();
    if cross(t0, v).abs() < 1e-10 * vl && cross(t1, v).abs() < 1e-10 * vl && dot(t0, v) > 0.0 {
        return Some(vec![Segment::Line { a: p0, b: p1 }]);
    }
    let t = add(t0, t1);
    let vt = dot(v, t);
    let den = 2.0 * (1.0 - dot(t0, t1));
    let d = if den.abs() < 1e-12 {
        let k = dot(v, t1);
        if k.abs() < 1e-12 {
            return None;
        }
        vv / (4.0 * k)
    } else {
        (-vt + (vt * vt + den * vv).sqrt()) / den
    };
    if !(d.is_finite() && d > 0.0) {
        return None;
    }
    let q0 = add(p0, scale(t0, d));
    let q1 = sub(p1, scale(t1, d));
    let j = lerp(q0, q1, 0.5);
    let first = arc_from(p0, t0, j);
    // 2 つ目は終点から逆向きに出て、つなぎ目に着く円弧を作り、向きを戻す
    let second = match arc_from(p1, scale(t1, -1.0), j) {
        Segment::Line { a, b } => Segment::Line { a: b, b: a },
        arc => arc,
    };
    Some(vec![first, second])
}

/// 有理 2 次ベジエ(始点・頂点・終点、頂点の重み `w`)の `t0`〜`t1` の部分を、両端の重みが 1 の
/// 形にした(始点, 頂点, 終点, rho)。
pub fn conic_part(p: [P2; 3], w: f64, t0: f64, t1: f64) -> Option<(P2, P2, P2, f64)> {
    let h = [
        [p[0][0], p[0][1], 1.0],
        [w * p[1][0], w * p[1][1], w],
        [p[2][0], p[2][1], 1.0],
    ];
    // 極形式(blossom): B(a, b)
    let blossom = |a: f64, b: f64| {
        let k = [(1.0 - a) * (1.0 - b), (1.0 - a) * b + a * (1.0 - b), a * b];
        let mut out = [0.0; 3];
        for i in 0..3 {
            for (o, hv) in out.iter_mut().zip(h[i]) {
                *o += k[i] * hv;
            }
        }
        out
    };
    let (a, m, b) = (blossom(t0, t0), blossom(t0, t1), blossom(t1, t1));
    if a[2] <= 0.0 || b[2] <= 0.0 || m[2] <= 0.0 {
        return None;
    }
    let pt = |h: [f64; 3]| [h[0] / h[2], h[1] / h[2]];
    let w2 = m[2] / (a[2] * b[2]).sqrt();
    Some((pt(a), pt(m), pt(b), w2 / (1.0 + w2)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: P2, b: P2, tol: f64) -> bool {
        dist(a, b) <= tol
    }

    #[test]
    fn a_fit_spline_passes_its_points_and_its_ends_can_bend() {
        for q in [
            vec![
                [0.0, 0.0],
                [10.0, 5.0],
                [20.0, -3.0],
                [35.0, 4.0],
                [42.0, 9.0],
                [50.0, 0.0],
            ],
            vec![[0.0, 0.0], [10.0, 5.0], [20.0, -3.0], [35.0, 4.0]],
            vec![[0.0, 0.0], [10.0, 5.0], [20.0, -3.0]],
            vec![[0.0, 0.0], [10.0, 5.0]],
        ] {
            let s = Shape::spline(&q, false, false, 3, None).unwrap();
            let u = chord_params(&q, false).unwrap();
            for (p, t) in q.iter().zip(&u) {
                assert!(close(s.at(*t), *p, 1e-9), "{t}");
            }
            if q.len() > 2 {
                // not-a-knot の端は曲がってよい(自然条件なら 0 になる)
                assert!(s.curvature(0.0).abs() > 1e-4, "{q:?}");
            }
        }
    }

    #[test]
    fn a_closed_fit_spline_passes_its_points_and_is_smooth_at_the_seam() {
        let q = [
            [0.0, 0.0],
            [10.0, 0.0],
            [12.0, 8.0],
            [3.0, 10.0],
            [-2.0, 5.0],
        ];
        let s = Shape::spline(&q, false, true, 3, None).unwrap();
        let u = chord_params(&q, true).unwrap();
        for (p, t) in q.iter().zip(&u) {
            assert!(close(s.at(*t), *p, 1e-9), "{t}");
        }
        let (a, b) = (s.ders(0.0), s.ders(1.0 - 1e-12));
        for k in 0..3 {
            assert!(
                close(a[k], b[k], 1e-4 * len(a[k]).max(1.0)),
                "{k} {a:?} {b:?}"
            );
        }
    }

    #[test]
    fn extracting_a_part_keeps_the_shape() {
        let q = [
            [0.0, 0.0],
            [10.0, 5.0],
            [20.0, -3.0],
            [35.0, 4.0],
            [40.0, 10.0],
        ];
        let Shape::Spline { s, .. } = Shape::spline(&q, false, false, 3, None).unwrap() else {
            panic!()
        };
        let part = s.extract(0.3, 0.8).unwrap();
        assert_eq!(part.knots.len(), part.ctrl.len() + 4);
        for i in 0..=20 {
            let t = 0.3 + 0.5 * i as f64 / 20.0;
            assert!(close(part.ders(t)[0], s.ders(t)[0], 1e-9), "{t}");
        }
        assert!(close(part.ctrl[0], s.ders(0.3)[0], 1e-9));
        assert!(close(*part.ctrl.last().unwrap(), s.ders(0.8)[0], 1e-9));
        let (l, r) = (s.extract(0.0, 0.5).unwrap(), s.extract(0.5, 1.0).unwrap());
        let j = BSpline::join(&l, &r);
        for i in 0..=20 {
            let t = i as f64 / 20.0;
            assert!(close(j.ders(t)[0], s.ders(t)[0], 1e-9), "{t}");
        }
    }

    #[test]
    fn biarcs_stay_within_the_tolerance() {
        let e = Shape::ellipse([0.0, 0.0], [30.0, 0.0], [0.0, 12.0], None).unwrap();
        let segs = e.segments(ARC_TOLERANCE);
        assert!(segs.len() >= 4);
        for i in 0..2000 {
            let p = e.at(TAU * i as f64 / 2000.0);
            let d = segs
                .iter()
                .map(|s| segment_point_distance(s, p))
                .fold(f64::INFINITY, f64::min);
            assert!(d <= ARC_TOLERANCE + 1e-9, "{i} {d}");
        }
    }

    #[test]
    fn a_conic_part_is_a_conic() {
        let c = Shape::conic([0.0, 0.0], [20.0, 0.0], [10.0, 15.0], 0.3).unwrap();
        let Shape::Conic { p, w } = c.clone() else {
            panic!()
        };
        let (s, a, e, rho) = conic_part(p, w, 0.2, 0.7).unwrap();
        let part = Shape::conic(s, e, a, rho).unwrap();
        for i in 0..=20 {
            let q = part.at(i as f64 / 20.0);
            assert!(c.closest(q).1 < 1e-9, "{i}");
        }
    }
}
