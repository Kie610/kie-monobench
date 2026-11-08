//! 輪郭の辺が近づけている元の曲線(楕円・スプライン・円錐曲線を有理 B スプラインにしたもの)と、
//! 同じ曲線から続く辺を 1 本の曲線にまとめる手順。
//!
//! 輪郭の辺([`Segment`])は線分と円弧だけで、加工の経路・画面・truck はその並びを使う。曲線を
//! そのまま扱えるカーネルは、辺に添えた [`EdgeCurve`] を [`ProfileLoop::pieces`] で 1 本にまとめ、
//! 側面を曲線ごとに 1 枚の面にする。

use super::*;

/// 次数 `degree`・制御点 `n` 個の B スプラインの節点 `knots` で、`knots[k] <= t < knots[k + 1]`
/// となる区間 `k`(範囲の外は端の区間。長さ 0 の区間は飛ばす)。
pub fn knot_span(knots: &[f64], degree: usize, n: usize, t: f64) -> usize {
    let mut last = degree;
    for k in degree..n {
        if knots[k] < knots[k + 1] {
            if t < knots[k + 1] {
                return k;
            }
            last = k;
        }
    }
    last
}

/// 平面の上の有理 B スプライン。節点の数は制御点の数 + 次数 + 1。重みが空なら全部 1。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Nurbs2 {
    pub degree: usize,
    pub knots: Vec<f64>,
    pub ctrl: Vec<P2>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub weights: Vec<f64>,
}

impl Nurbs2 {
    /// 数がそろい、節点が減らず、重みが正か。
    pub fn valid(&self) -> bool {
        let (p, n) = (self.degree, self.ctrl.len());
        p >= 1
            && n > p
            && self.knots.len() == n + p + 1
            && self.knots.windows(2).all(|w| w[0] <= w[1])
            && self.knots[n] > self.knots[p]
            && (self.weights.is_empty()
                || self.weights.len() == n && self.weights.iter().all(|w| *w > 0.0))
            && self.knots.iter().all(|k| k.is_finite())
            && self
                .ctrl
                .iter()
                .all(|c| c[0].is_finite() && c[1].is_finite())
    }

    /// パラメータの範囲。
    pub fn domain(&self) -> (f64, f64) {
        (self.knots[self.degree], self.knots[self.ctrl.len()])
    }

    fn weight(&self, i: usize) -> f64 {
        self.weights.get(i).copied().unwrap_or(1.0)
    }

    /// 同次座標(x·w, y·w, w)の制御点。
    fn homogeneous(&self) -> Vec<[f64; 3]> {
        (0..self.ctrl.len())
            .map(|i| {
                let w = self.weight(i);
                [self.ctrl[i][0] * w, self.ctrl[i][1] * w, w]
            })
            .collect()
    }

    fn span(&self, t: f64) -> usize {
        knot_span(&self.knots, self.degree, self.ctrl.len(), t)
    }

    /// 点 `t` の位置(de Boor の手順を同次座標で)。
    pub fn at(&self, t: f64) -> P2 {
        let p = self.degree;
        let k = self.span(t);
        let u = &self.knots;
        let h = self.homogeneous();
        let mut d: Vec<[f64; 3]> = (0..=p).map(|j| h[k - p + j]).collect();
        for r in 1..=p {
            for j in (r..=p).rev() {
                let i = k - p + j;
                let den = u[i + p + 1 - r] - u[i];
                let a = if den > 0.0 { (t - u[i]) / den } else { 0.0 };
                let prev = d[j - 1];
                for (c, v) in d[j].iter_mut().enumerate() {
                    *v = (1.0 - a) * prev[c] + a * *v;
                }
            }
        }
        [d[p][0] / d[p][2], d[p][1] / d[p][2]]
    }

    /// 節点 `t` を 1 つ入れる(形は変わらない。Boehm の手順を同次座標で)。
    fn insert(&mut self, t: f64) {
        let p = self.degree;
        let k = self.span(t);
        let h = self.homogeneous();
        let u = self.knots.clone();
        let mut q: Vec<[f64; 3]> = Vec::with_capacity(h.len() + 1);
        for i in 0..=h.len() {
            if i + p <= k {
                q.push(h[i]);
            } else if i > k {
                q.push(h[i - 1]);
            } else {
                let den = u[i + p] - u[i];
                let a = if den > 0.0 { (t - u[i]) / den } else { 0.0 };
                q.push([
                    (1.0 - a) * h[i - 1][0] + a * h[i][0],
                    (1.0 - a) * h[i - 1][1] + a * h[i][1],
                    (1.0 - a) * h[i - 1][2] + a * h[i][2],
                ]);
            }
        }
        self.knots.insert(k + 1, t);
        let rational = !self.weights.is_empty();
        self.ctrl = q.iter().map(|c| [c[0] / c[2], c[1] / c[2]]).collect();
        self.weights = if rational {
            q.iter().map(|c| c[2]).collect()
        } else {
            Vec::new()
        };
    }

    /// 節点 `t` の重なりの数。
    fn multiplicity(&self, t: f64) -> usize {
        self.knots
            .iter()
            .filter(|k| (**k - t).abs() < 1e-14)
            .count()
    }

    /// `a`〜`b`(`a < b`、範囲の中)の部分。両端は節点を次数 + 1 回重ねて止める(clamped)。
    pub fn sub(&self, a: f64, b: f64) -> Option<Nurbs2> {
        let (lo, hi) = self.domain();
        let span = hi - lo;
        if a.partial_cmp(&b) != Some(std::cmp::Ordering::Less)
            || a < lo - 1e-12 * span
            || b > hi + 1e-12 * span
        {
            return None;
        }
        let (a, b) = (a.max(lo), b.min(hi));
        let mut c = self.clone();
        let p = c.degree;
        for t in [a, b] {
            // 範囲の端でも、止まっていない(unclamped)節点の並びを止めるため p 回まで入れる
            while c.multiplicity(t) < p {
                c.insert(t);
            }
        }
        // 節点 t が k 番目で終わる p 重なら曲線の点 C(t) は制御点 k - p、f 番目から始まる p 重なら f - 1。
        // a の最後の添字から b の最初の添字までの節点と、その間の制御点が部分の曲線になる
        let ka = c.knots.iter().rposition(|k| (*k - a).abs() < 1e-14)?;
        let fb = c.knots.iter().position(|k| (*k - b).abs() < 1e-14)?;
        if ka < p || fb == 0 || fb <= ka {
            return None;
        }
        let (i0, i1) = (ka - p, fb - 1);
        let mut knots = vec![a; p + 1];
        knots.extend_from_slice(&c.knots[ka + 1..fb]);
        knots.extend(std::iter::repeat_n(b, p + 1));
        let ctrl = c.ctrl[i0..=i1].to_vec();
        let weights = if c.weights.is_empty() {
            Vec::new()
        } else {
            c.weights[i0..=i1].to_vec()
        };
        let out = Nurbs2 {
            degree: p,
            knots,
            ctrl,
            weights,
        };
        out.valid().then_some(out)
    }
}

/// 輪郭の辺が近づけている元の曲線の一部。`t0` は辺の区間の始点([`Segment::start_point`])、
/// `t1` は終点の、曲線のパラメータ(辺の `reversed` には関係しない)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EdgeCurve {
    /// 元の曲線の名前(スケッチの要素の名前。例 `e5`)。続く辺をまとめた面の名前に使う。
    pub name: String,
    pub curve: Nurbs2,
    /// 閉じた曲線か(パラメータの範囲の両端が同じ点)。
    #[serde(default)]
    pub closed: bool,
    pub t0: f64,
    pub t1: f64,
}

/// ループの並びの 1 つ。[`ProfileLoop::pieces`] が作る。
#[derive(Debug, Clone, PartialEq)]
pub enum LoopPiece {
    /// そのままの辺(ループの辺の添字)
    Edge(usize),
    /// 同じ元の曲線から続く辺(添字、たどる順)を 1 本にした曲線
    Curve {
        name: String,
        curve: Nurbs2,
        edges: Vec<usize>,
    },
}

/// まとめた曲線の端が、ループの頂点から離れてよい距離(mm)。これより離れたら辺のまま使う。
const CURVE_GAP: f64 = 1e-7;

impl ProfileLoop {
    /// 辺を、同じ元の曲線([`ProfileEdge::curve`])から続く所を 1 本の曲線にまとめた並びにする。
    /// 曲線の上で続いていない所や、まとめた曲線の端がループの頂点に合わない所は辺のまま残す
    /// (隙間のある輪郭をカーネルに渡さない)。閉じた曲線の継ぎ目をまたぐ所は継ぎ目で 2 本に分ける。
    pub fn pieces(&self) -> Vec<LoopPiece> {
        let n = self.edges.len();
        let same = |i: usize, j: usize| match (&self.edges[i].curve, &self.edges[j].curve) {
            (Some(a), Some(b)) => a.name == b.name && a.curve == b.curve,
            _ => false,
        };
        // 曲線の続きの途中から始まらないよう、前の辺と別の曲線になる所から数える
        let start = (0..n).find(|&i| !same((i + n - 1) % n, i));
        let Some(start) = start else {
            // 全部が同じ曲線(閉じた楕円・スプライン 1 本の輪郭)
            return self.run_pieces(&(0..n).collect::<Vec<_>>());
        };
        let mut out = Vec::new();
        let mut run: Vec<usize> = Vec::new();
        for i in (start..start + n).map(|i| i % n) {
            if let Some(&last) = run.last()
                && !same(last, i)
            {
                out.extend(self.run_pieces(&run));
                run.clear();
            }
            run.push(i);
        }
        out.extend(self.run_pieces(&run));
        out
    }

    /// 同じ曲線の辺の続き `run` を曲線にする(できなければ辺のまま)。
    fn run_pieces(&self, run: &[usize]) -> Vec<LoopPiece> {
        let edges = || run.iter().map(|&i| LoopPiece::Edge(i)).collect();
        let Some(c) = run.first().and_then(|&i| self.edges[i].curve.as_ref()) else {
            return edges();
        };
        if !c.curve.valid() {
            return edges();
        }
        let (lo, hi) = c.curve.domain();
        let period = hi - lo;
        let tol = 1e-9 * period.max(1.0);
        // たどる順の、曲線のパラメータの区間。逆向きの辺は終点から始点へ
        let ranges: Vec<(f64, f64)> = run
            .iter()
            .map(|&i| {
                let e = &self.edges[i];
                let c = e.curve.as_ref().expect("同じ曲線");
                if e.reversed {
                    (c.t1, c.t0)
                } else {
                    (c.t0, c.t1)
                }
            })
            .collect();
        let forward =
            ranges[0].1 > ranges[0].0 || c.closed && ranges[0].1 < ranges[0].0 - period / 2.0;
        // 区間をつないで、たどる順に増える(または減る)1 本の範囲にする
        let unwrap = |t: f64, near: f64| {
            if !c.closed {
                return t;
            }
            let k = ((near - t) / period).round();
            t + k * period
        };
        let mut a = ranges[0].0;
        let mut b = a;
        for &(s, e) in &ranges {
            let s = unwrap(s, b);
            if (s - b).abs() > 1e-6 * period.max(1.0) {
                return edges();
            }
            let mut e = unwrap(e, s);
            if c.closed && (e - s).abs() < tol {
                // 1 本で一周する辺
                e = if forward { s + period } else { s - period };
            }
            if (e > s) != forward {
                return edges();
            }
            b = e;
        }
        if !forward {
            std::mem::swap(&mut a, &mut b);
        }
        // ループの全部が閉じた曲線 1 本を一周する(楕円・閉じたスプラインだけの輪郭)。頂点は無いので、
        // 曲線の範囲の端を継ぎ目にした 1 本にする
        if c.closed
            && run.len() == self.edges.len()
            && (b - a - period).abs() < 1e-6 * period.max(1.0)
        {
            return match c.curve.sub(lo, hi) {
                Some(curve) => vec![LoopPiece::Curve {
                    name: c.name.clone(),
                    curve,
                    edges: run.to_vec(),
                }],
                None => edges(),
            };
        }
        // 範囲を曲線の範囲の中の 1 本か、継ぎ目で分けた 2 本にする
        let mut parts: Vec<(f64, f64)> = Vec::new();
        if c.closed {
            if b - a > period + tol {
                return edges();
            }
            let shift = ((a - lo) / period).floor() * period;
            let (a, b) = (a - shift, b - shift);
            if b <= hi + tol {
                parts.push((a, b.min(hi)));
            } else {
                parts.push((a, hi));
                parts.push((lo, b - period));
            }
        } else {
            parts.push((a, b));
        }
        if parts.len() > 1 && (parts[0].1 - parts[0].0 < tol || parts[1].1 - parts[1].0 < tol) {
            parts.retain(|(s, e)| e - s >= tol);
        }
        let curves: Option<Vec<Nurbs2>> = parts.iter().map(|(s, e)| c.curve.sub(*s, *e)).collect();
        let Some(mut curves) = curves else {
            return edges();
        };
        // 端がループの頂点に合うか(継ぎ目で分けた時は、継ぎ目の点は曲線どうしで合う)
        let from = self.edges[run[0]].from_point();
        let to = self.edges[*run.last().expect("1 つはある")].to_point();
        let (first, last) = if forward {
            (curves[0].at(curves[0].domain().0), {
                let l = &curves[curves.len() - 1];
                l.at(l.domain().1)
            })
        } else {
            (
                {
                    let l = &curves[curves.len() - 1];
                    l.at(l.domain().1)
                },
                curves[0].at(curves[0].domain().0),
            )
        };
        let gap = |p: P2, q: P2| (p[0] - q[0]).hypot(p[1] - q[1]);
        if gap(first, from) > CURVE_GAP || gap(last, to) > CURVE_GAP {
            return edges();
        }
        if curves.len() == 1 {
            return vec![LoopPiece::Curve {
                name: c.name.clone(),
                curve: curves.pop().expect("1 本"),
                edges: run.to_vec(),
            }];
        }
        // 継ぎ目で分けた 2 本は、継ぎ目の側の辺の分け方を知らないので辺の全部を両方に持たせる
        curves
            .into_iter()
            .map(|curve| LoopPiece::Curve {
                name: c.name.clone(),
                curve,
                edges: run.to_vec(),
            })
            .collect()
    }
}
