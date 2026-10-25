//! 外のファイル(DXF・SVG)から読んだ形と、それをスケッチへ入れる操作。
//!
//! 読み込み(kmb-io-dxf の `from_dxf`・kmb-io-svg の `from_svg`)は [`Imported`] を返すだけで、
//! スケッチは変えない。画面で置く位置と倍率を決めてから [`Sketch::insert_shapes`] で入れる。
//! 今のスケッチに無い曲線(ベジエ・楕円・B スプライン)は、入れる時に [`approximate`] で
//! 線分と円弧の並びにする。

use crate::geom::{add, cross, dist, dot, len, perp, scale, sub};
use crate::model::{Entity, EntityId, Sketch};
use kmb_core::kernel::{P2, arc_sweep};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

/// 端を同じ点にまとめる距離(mm)
pub const WELD_TOL: f64 = 1e-6;
/// ベジエ・楕円・B スプラインを線分と円弧にする時の許容差(mm)
pub const APPROX_TOL: f64 = 0.005;

/// 読んだ形。座標は mm、y は上向き(スケッチと同じ)。角度とパラメータはラジアン。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Shape {
    Line {
        a: P2,
        b: P2,
    },
    /// 中心のまわりに `start` から `end` へ反時計回りの円弧(`start == end` なら 1 周)
    Arc {
        center: P2,
        radius: f64,
        start: f64,
        end: f64,
    },
    Circle {
        center: P2,
        radius: f64,
    },
    Point {
        p: P2,
    },
    /// 3 次のベジエ曲線(始点・制御点 2 つ・終点)
    Bezier {
        p: [P2; 4],
    },
    /// 楕円(の弧)。点は `center + major·cos t + minor·sin t`。`minor` は `major` を
    /// 反時計回りに 90° 回して `ratio` 倍したもの。`t` は `start` から `end` へ増える向き
    /// (`start == end` か差が 2π なら 1 周)。
    Ellipse {
        center: P2,
        major: P2,
        ratio: f64,
        start: f64,
        end: f64,
    },
    /// B スプライン(DXF の SPLINE をそのまま)。`knots` は `control.len() + degree + 1` 個。
    /// `weights` は有理のときだけ制御点と同じ数、空なら全部 1。
    BSpline {
        degree: usize,
        control: Vec<P2>,
        knots: Vec<f64>,
        weights: Vec<f64>,
    },
}

/// ファイルから読んだ結果。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Imported {
    pub shapes: Vec<Shape>,
    /// 読めなかった要素の種類と数(DXF の要素名、SVG の要素名)。種類の名前の順。
    pub skipped: Vec<(String, usize)>,
    /// 形の全部を囲む箱(左下, 右上)。形が無ければ `None`。
    pub bounds: Option<(P2, P2)>,
}

impl Imported {
    /// 形と読み飛ばした数から作る(囲む箱を計算する)。
    pub fn new(shapes: Vec<Shape>, skipped: HashMap<String, usize>) -> Imported {
        let mut skipped: Vec<(String, usize)> = skipped.into_iter().collect();
        skipped.sort();
        let bounds = shapes_bounds(&shapes);
        Imported {
            shapes,
            skipped,
            bounds,
        }
    }
}

/// スケッチへ入れる時の置き方。読んだ座標 `p` は `offset + R(rotation)·(scale·p)` に置く。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Place {
    /// ずれ(mm)
    pub offset: P2,
    /// 倍率(1 で原寸)
    pub scale: f64,
    /// 原点まわりの回転(ラジアン、反時計回り)
    pub rotation: f64,
}

impl Default for Place {
    fn default() -> Self {
        Place {
            offset: [0.0, 0.0],
            scale: 1.0,
            rotation: 0.0,
        }
    }
}

impl Place {
    pub fn affine(&self) -> Affine {
        let (s, c) = self.rotation.sin_cos();
        let k = self.scale;
        Affine([k * c, k * s, -k * s, k * c, self.offset[0], self.offset[1]])
    }
}

/// 2 次元のアフィン変換。SVG と同じ並び `[a, b, c, d, e, f]` で
/// `x' = a·x + c·y + e`、`y' = b·x + d·y + f`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine(pub [f64; 6]);

impl Affine {
    pub const IDENTITY: Affine = Affine([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    pub fn translate(x: f64, y: f64) -> Affine {
        Affine([1.0, 0.0, 0.0, 1.0, x, y])
    }

    pub fn scale(sx: f64, sy: f64) -> Affine {
        Affine([sx, 0.0, 0.0, sy, 0.0, 0.0])
    }

    pub fn rotate(rad: f64) -> Affine {
        let (s, c) = rad.sin_cos();
        Affine([c, s, -s, c, 0.0, 0.0])
    }

    /// `self ∘ o`(先に `o`、次に `self`)
    pub fn then_after(&self, o: &Affine) -> Affine {
        let [a, b, c, d, e, f] = self.0;
        let [p, q, r, s, t, u] = o.0;
        Affine([
            a * p + c * q,
            b * p + d * q,
            a * r + c * s,
            b * r + d * s,
            a * t + c * u + e,
            b * t + d * u + f,
        ])
    }

    pub fn apply(&self, p: P2) -> P2 {
        let [a, b, c, d, e, f] = self.0;
        [a * p[0] + c * p[1] + e, b * p[0] + d * p[1] + f]
    }

    /// 向きだけ(ずれを除く)
    pub fn vector(&self, v: P2) -> P2 {
        let [a, b, c, d, ..] = self.0;
        [a * v[0] + c * v[1], b * v[0] + d * v[1]]
    }

    pub fn det(&self) -> f64 {
        let [a, b, c, d, ..] = self.0;
        a * d - b * c
    }

    /// 相似変換(回転・一様な倍率・鏡映)なら倍率を返す。
    fn similarity(&self) -> Option<f64> {
        let [a, b, c, d, ..] = self.0;
        let k = (a * a + b * b).sqrt();
        let tol = 1e-9 * k.max((c * c + d * d).sqrt());
        let rot = (a - d).abs() <= tol && (b + c).abs() <= tol;
        let refl = (a + d).abs() <= tol && (b - c).abs() <= tol;
        (k > 0.0 && (rot || refl)).then_some(k)
    }
}

fn polar(c: P2, r: f64, a: f64) -> P2 {
    [c[0] + r * a.cos(), c[1] + r * a.sin()]
}
fn angle(v: P2) -> f64 {
    v[1].atan2(v[0])
}
fn finite(p: P2) -> bool {
    p[0].is_finite() && p[1].is_finite()
}

impl Shape {
    /// 数が全部有限か(壊れた入力を捨てるため)
    pub fn is_finite(&self) -> bool {
        match self {
            Shape::Line { a, b } => finite(*a) && finite(*b),
            Shape::Arc {
                center,
                radius,
                start,
                end,
            } => finite(*center) && radius.is_finite() && start.is_finite() && end.is_finite(),
            Shape::Circle { center, radius } => finite(*center) && radius.is_finite(),
            Shape::Point { p } => finite(*p),
            Shape::Bezier { p } => p.iter().all(|q| finite(*q)),
            Shape::Ellipse {
                center,
                major,
                ratio,
                start,
                end,
            } => {
                finite(*center)
                    && finite(*major)
                    && ratio.is_finite()
                    && start.is_finite()
                    && end.is_finite()
            }
            Shape::BSpline {
                control,
                knots,
                weights,
                ..
            } => {
                control.iter().all(|p| finite(*p))
                    && knots.iter().all(|k| k.is_finite())
                    && weights.iter().all(|w| w.is_finite())
            }
        }
    }
}

/// 共役な半径 `u`・`v`(点は `c + u·cos t + v·sin t`、`t` は `t0` から `t1` へ)を、
/// 長軸と比の [`Shape::Ellipse`] にする。円なら [`Shape::Arc`]・[`Shape::Circle`] にする。
pub fn ellipse_from_conjugate(c: P2, u: P2, v: P2, t0: f64, t1: f64) -> Shape {
    let full = (t1 - t0).abs() >= TAU - 1e-12;
    let (t0, t1) = if t1 < t0 { (t1, t0) } else { (t0, t1) };
    let th = 0.5 * (2.0 * dot(u, v)).atan2(dot(u, u) - dot(v, v));
    let (s, co) = th.sin_cos();
    let major = add(scale(u, co), scale(v, s));
    let minor = add(scale(u, -s), scale(v, co));
    let (lm, ln) = (len(major), len(minor));
    let ratio = if lm > 0.0 { ln / lm } else { 0.0 };
    // パラメータ s = t - th。短軸が時計回りの側なら s を負にして向きを合わせる
    let (start, end) = if cross(major, minor) >= 0.0 {
        (t0 - th, t1 - th)
    } else {
        (-(t1 - th), -(t0 - th))
    };
    let (start, end) = if full {
        (start, start + TAU)
    } else {
        (start, end)
    };
    if lm > 0.0 && (ratio - 1.0).abs() <= 1e-9 {
        if full {
            return Shape::Circle {
                center: c,
                radius: lm,
            };
        }
        let a = angle(major);
        return Shape::Arc {
            center: c,
            radius: lm,
            start: a + start,
            end: a + end,
        };
    }
    Shape::Ellipse {
        center: c,
        major,
        ratio,
        start,
        end,
    }
}

/// 楕円のパラメータの幅(0 より大きく 2π 以下)
fn ellipse_sweep(start: f64, end: f64) -> f64 {
    let d = end - start;
    if d > 0.0 && d <= TAU {
        d
    } else {
        arc_sweep(start, end)
    }
}

/// 形をアフィン変換する。相似でない変換では円と円弧が楕円になる。
pub fn transform_shape(s: &Shape, m: &Affine) -> Shape {
    match s {
        Shape::Line { a, b } => Shape::Line {
            a: m.apply(*a),
            b: m.apply(*b),
        },
        Shape::Point { p } => Shape::Point { p: m.apply(*p) },
        Shape::Bezier { p } => Shape::Bezier {
            p: p.map(|q| m.apply(q)),
        },
        Shape::BSpline {
            degree,
            control,
            knots,
            weights,
        } => Shape::BSpline {
            degree: *degree,
            control: control.iter().map(|p| m.apply(*p)).collect(),
            knots: knots.clone(),
            weights: weights.clone(),
        },
        Shape::Circle { center, radius } => match m.similarity() {
            Some(k) => Shape::Circle {
                center: m.apply(*center),
                radius: radius * k,
            },
            None => ellipse_from_conjugate(
                m.apply(*center),
                m.vector([*radius, 0.0]),
                m.vector([0.0, *radius]),
                0.0,
                TAU,
            ),
        },
        Shape::Arc {
            center,
            radius,
            start,
            end,
        } => {
            let sweep = arc_sweep(*start, *end);
            match m.similarity() {
                Some(k) => {
                    let c = m.apply(*center);
                    // 鏡映なら向きが逆になるので、終点から測る
                    let from = if m.det() >= 0.0 { *start } else { *end };
                    let p = m.apply(polar(*center, *radius, from));
                    let a = angle(sub(p, c));
                    Shape::Arc {
                        center: c,
                        radius: radius * k,
                        start: a,
                        end: a + sweep,
                    }
                }
                None => ellipse_from_conjugate(
                    m.apply(*center),
                    m.vector([*radius, 0.0]),
                    m.vector([0.0, *radius]),
                    *start,
                    start + sweep,
                ),
            }
        }
        Shape::Ellipse {
            center,
            major,
            ratio,
            start,
            end,
        } => {
            let minor = scale(perp(*major), *ratio);
            ellipse_from_conjugate(
                m.apply(*center),
                m.vector(*major),
                m.vector(minor),
                *start,
                start + ellipse_sweep(*start, *end),
            )
        }
    }
}

/// 円弧の端と、軸の向きの端(囲む箱に使う)
fn arc_extremes(center: P2, radius: f64, start: f64, end: f64) -> Vec<P2> {
    let sweep = arc_sweep(start, end);
    let mut pts = vec![polar(center, radius, start), polar(center, radius, end)];
    for k in 0..4 {
        let a = k as f64 * FRAC_PI_2;
        if (a - start).rem_euclid(TAU) <= sweep {
            pts.push(polar(center, radius, a));
        }
    }
    pts
}

/// 形の全部を囲む箱。
pub fn shapes_bounds(shapes: &[Shape]) -> Option<(P2, P2)> {
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    let mut put = |p: P2| {
        if finite(p) {
            for k in 0..2 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
    };
    fn visit(s: &Shape, put: &mut dyn FnMut(P2)) {
        match s {
            Shape::Line { a, b } => {
                put(*a);
                put(*b);
            }
            Shape::Point { p } => put(*p),
            Shape::Circle { center, radius } => {
                put(sub(*center, [*radius, *radius]));
                put(add(*center, [*radius, *radius]));
            }
            Shape::Arc {
                center,
                radius,
                start,
                end,
            } => arc_extremes(*center, *radius, *start, *end)
                .into_iter()
                .for_each(put),
            _ => {
                for a in approximate(s) {
                    visit(&a, put);
                }
            }
        }
    }
    for s in shapes {
        visit(s, &mut put);
    }
    (lo[0] <= hi[0] && lo[1] <= hi[1]).then_some((lo, hi))
}

// ---- 近似 ----

/// 線分・円弧・円・点のどれかを、そのまま返す。ベジエ・楕円・B スプラインは
/// 許容差 [`APPROX_TOL`] で線分と円弧の並び(バイアーク)にする。壊れた形は空。
pub fn approximate(shape: &Shape) -> Vec<Shape> {
    approximate_tol(shape, APPROX_TOL)
}

/// [`approximate`] の許容差(mm)を選べる版。
pub fn approximate_tol(shape: &Shape, tol: f64) -> Vec<Shape> {
    if !shape.is_finite() || tol.is_nan() || tol <= 0.0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    match shape {
        Shape::Line { .. } | Shape::Arc { .. } | Shape::Circle { .. } | Shape::Point { .. } => {
            out.push(shape.clone());
        }
        Shape::Bezier { p } => {
            let p = *p;
            let f = move |t: f64| bezier_at(&p, t);
            fit(&f, 0.0, 1.0, tol, 0, &mut out);
        }
        Shape::Ellipse {
            center,
            major,
            ratio,
            start,
            end,
        } => {
            if (ratio - 1.0).abs() <= 1e-9 || len(*major) == 0.0 {
                // 円か点(点なら下で捨てる)
                let s = transform_shape(shape, &Affine::IDENTITY);
                if !matches!(s, Shape::Ellipse { .. }) {
                    out.push(s);
                }
                return out;
            }
            let (c, m, n) = (*center, *major, scale(perp(*major), *ratio));
            let f = move |t: f64| add(c, add(scale(m, t.cos()), scale(n, t.sin())));
            let t1 = start + ellipse_sweep(*start, *end);
            // 4 分の 1 周ずつに分けてから合わせる
            let mut cuts = vec![*start];
            let mut k = (start / FRAC_PI_2).floor() + 1.0;
            // 1 周は 4 つまで(大きすぎるパラメータで止まらなくならないように)
            while k * FRAC_PI_2 < t1 - 1e-9 && cuts.len() < 5 {
                cuts.push(k * FRAC_PI_2);
                k += 1.0;
            }
            cuts.push(t1);
            for w in cuts.windows(2) {
                fit(&f, w[0], w[1], tol, 0, &mut out);
            }
        }
        Shape::BSpline {
            degree,
            control,
            knots,
            weights,
        } => {
            let Some(spans) = bspline_spans(*degree, control, knots, weights) else {
                return out;
            };
            let f = |t: f64| bspline_at(*degree, control, knots, weights, t);
            for w in spans.windows(2) {
                fit(&f, w[0], w[1], tol, 0, &mut out);
            }
        }
    }
    out
}

fn bezier_at(p: &[P2; 4], t: f64) -> P2 {
    let u = 1.0 - t;
    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    [
        a * p[0][0] + b * p[1][0] + c * p[2][0] + d * p[3][0],
        a * p[0][1] + b * p[1][1] + c * p[2][1] + d * p[3][1],
    ]
}

/// B スプラインが正しければ、節点の区切り(重複を除いた定義域の中の節点)を返す。
fn bspline_spans(
    degree: usize,
    control: &[P2],
    knots: &[f64],
    weights: &[f64],
) -> Option<Vec<f64>> {
    let n = control.len();
    if degree == 0 || degree > 32 || n < degree + 1 || knots.len() != n + degree + 1 {
        return None;
    }
    if !weights.is_empty()
        && (weights.len() != n || weights.iter().any(|w| w.is_nan() || *w <= 0.0))
    {
        return None;
    }
    if knots.windows(2).any(|w| w[1] < w[0]) {
        return None;
    }
    let (lo, hi) = (knots[degree], knots[n]);
    if hi.is_nan() || lo.is_nan() || hi <= lo {
        return None;
    }
    let mut spans: Vec<f64> = knots[degree..=n].to_vec();
    spans.dedup();
    Some(spans)
}

/// de Boor の方法で B スプラインの点を求める(有理なら同次座標で)。
fn bspline_at(degree: usize, control: &[P2], knots: &[f64], weights: &[f64], t: f64) -> P2 {
    let n = control.len();
    let p = degree;
    let t = t.clamp(knots[p], knots[n]);
    // knots[k] <= t < knots[k+1] になる k(p..n-1)
    let mut k = p;
    while k + 1 < n && knots[k + 1] <= t {
        k += 1;
    }
    let w = |i: usize| if weights.is_empty() { 1.0 } else { weights[i] };
    let mut d: Vec<[f64; 3]> = (0..=p)
        .map(|j| {
            let i = j + k - p;
            let wi = w(i);
            [control[i][0] * wi, control[i][1] * wi, wi]
        })
        .collect();
    for r in 1..=p {
        for j in (r..=p).rev() {
            let i = j + k - p;
            let den = knots[i + p + 1 - r] - knots[i];
            let a = if den > 0.0 { (t - knots[i]) / den } else { 0.0 };
            let prev = d[j - 1];
            for (x, y) in d[j].iter_mut().zip(prev) {
                *x = (1.0 - a) * y + a * *x;
            }
        }
    }
    let h = d[p];
    if h[2] != 0.0 {
        [h[0] / h[2], h[1] / h[2]]
    } else {
        [h[0], h[1]]
    }
}

/// 曲線の点から測った、線分までの距離
fn segment_distance(a: P2, b: P2, x: P2) -> f64 {
    let d = sub(b, a);
    let l2 = dot(d, d);
    let t = if l2 > 0.0 {
        (dot(sub(x, a), d) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    dist(add(a, scale(d, t)), x)
}

/// 線分か円弧(`Shape::Line`・`Shape::Arc`)までの距離
fn piece_distance(s: &Shape, x: P2) -> f64 {
    match s {
        Shape::Line { a, b } => segment_distance(*a, *b, x),
        Shape::Arc {
            center,
            radius,
            start,
            end,
        } => {
            let sweep = arc_sweep(*start, *end);
            if (angle(sub(x, *center)) - start).rem_euclid(TAU) <= sweep {
                (dist(x, *center) - radius).abs()
            } else {
                dist(x, polar(*center, *radius, *start)).min(dist(x, polar(*center, *radius, *end)))
            }
        }
        _ => f64::INFINITY,
    }
}

/// 点 `p` で向き `t`(単位)に接し、点 `q` を通る円弧(ほぼまっすぐなら線分)と、その中心角。
fn tangent_arc(p: P2, t: P2, q: P2) -> (Shape, f64) {
    let w = sub(q, p);
    let n = perp(t);
    let den = 2.0 * dot(n, w);
    let ww = dot(w, w);
    // 弦の中央の膨らみ(およそ |n·w| / 4)が 1e-9 mm 未満なら線分
    if den.abs() / 8.0 < 1e-9 {
        return (Shape::Line { a: p, b: q }, 0.0);
    }
    let r = ww / den;
    let c = add(p, scale(n, r));
    let (s, e) = if r > 0.0 {
        (angle(sub(p, c)), angle(sub(q, c)))
    } else {
        (angle(sub(q, c)), angle(sub(p, c)))
    };
    (
        Shape::Arc {
            center: c,
            radius: r.abs(),
            start: s,
            end: e,
        },
        arc_sweep(s, e),
    )
}

fn unit(v: P2) -> Option<P2> {
    let l = len(v);
    (l > 0.0 && l.is_finite()).then(|| scale(v, 1.0 / l))
}

/// 端の接線(差分で求める)
fn tangents(f: &dyn Fn(f64) -> P2, t0: f64, t1: f64) -> (Option<P2>, Option<P2>) {
    let h = (t1 - t0) * 1e-6;
    (unit(sub(f(t0 + h), f(t0))), unit(sub(f(t1), f(t1 - h))))
}

/// 2 つの円弧で、端の位置と接線を合わせる(等しい長さの補助線を使う方法)。
fn biarc(p1: P2, t1: P2, p2: P2, t2: P2) -> Option<[(Shape, f64); 2]> {
    let v = sub(p2, p1);
    let t = add(t1, t2);
    let den = 2.0 * (1.0 - dot(t1, t2));
    let d = if den.abs() < 1e-12 {
        let vt = dot(v, t2);
        if vt.abs() < 1e-12 {
            return None;
        }
        dot(v, v) / (4.0 * vt)
    } else {
        let disc = dot(v, t) * dot(v, t) + den * dot(v, v);
        (-dot(v, t) + disc.max(0.0).sqrt()) / den
    };
    if !d.is_finite() || d <= 0.0 {
        return None;
    }
    let q1 = add(p1, scale(t1, d));
    let q2 = sub(p2, scale(t2, d));
    let pm = scale(add(q1, q2), 0.5);
    let a = tangent_arc(p1, t1, pm);
    // 2 つ目は終点から逆向きにたどって作る
    let b = tangent_arc(p2, scale(t2, -1.0), pm);
    Some([a, b])
}

const SAMPLES: usize = 9;
const MAX_DEPTH: usize = 18;

/// 曲線 `f` の `t0..t1` を、許容差 `tol` の線分と円弧の並びにして `out` に足す。
fn fit(f: &dyn Fn(f64) -> P2, t0: f64, t1: f64, tol: f64, depth: usize, out: &mut Vec<Shape>) {
    let (p0, p1) = (f(t0), f(t1));
    if !finite(p0) || !finite(p1) {
        return;
    }
    let samples: Vec<P2> = (1..SAMPLES)
        .map(|i| f(t0 + (t1 - t0) * i as f64 / SAMPLES as f64))
        .collect();
    let within = |pieces: &[&Shape]| {
        samples.iter().all(|x| {
            pieces
                .iter()
                .map(|s| piece_distance(s, *x))
                .fold(f64::INFINITY, f64::min)
                <= tol
        })
    };
    let chord = Shape::Line { a: p0, b: p1 };
    if within(&[&chord]) {
        if dist(p0, p1) > 0.0 {
            out.push(chord);
        }
        return;
    }
    if depth < MAX_DEPTH {
        if let (Some(ta), Some(tb)) = tangents(f, t0, t1)
            && let Some([(a, sa), (b, sb)]) = biarc(p0, ta, p1, tb)
            && sa <= FRAC_PI_2 + 1e-9
            && sb <= FRAC_PI_2 + 1e-9
            && within(&[&a, &b])
        {
            out.push(a);
            out.push(b);
            return;
        }
        let tm = 0.5 * (t0 + t1);
        fit(f, t0, tm, tol, depth + 1, out);
        fit(f, tm, t1, tol, depth + 1, out);
    } else if dist(p0, p1) > 0.0 {
        out.push(chord);
    }
}

// ---- スケッチへ入れる ----

/// 要素を足していく(番号は最初に一度だけ数える。`add_point` などは毎回全部を数えるので、
/// 大きなファイルでは遅い)。線の端は位置でまとめる(格子に分けて近くだけ探す)。
struct Adder<'s> {
    sketch: &'s mut Sketch,
    cells: HashMap<(i64, i64), Vec<(P2, EntityId)>>,
}

impl Adder<'_> {
    fn id(&mut self) -> EntityId {
        let n = &mut self.sketch.next_ids.entity;
        *n += 1;
        EntityId(*n - 1)
    }

    fn add_point(&mut self, p: P2) -> EntityId {
        let id = self.id();
        self.sketch.entities.push(Entity::Point {
            id,
            x: p[0],
            y: p[1],
        });
        id
    }

    fn cell(p: P2) -> (i64, i64) {
        (
            (p[0] / WELD_TOL).floor() as i64,
            (p[1] / WELD_TOL).floor() as i64,
        )
    }

    fn find(&self, p: P2) -> Option<EntityId> {
        let (cx, cy) = Self::cell(p);
        let mut best: Option<(f64, EntityId)> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                let key = (cx.saturating_add(dx), cy.saturating_add(dy));
                for (q, id) in self.cells.get(&key).into_iter().flatten() {
                    let d = dist(*q, p);
                    if d <= WELD_TOL && best.is_none_or(|b| d < b.0) {
                        best = Some((d, *id));
                    }
                }
            }
        }
        best.map(|b| b.1)
    }

    /// 線の端の点(近くに既にあればそれを使う)
    fn end_point(&mut self, p: P2) -> EntityId {
        if let Some(id) = self.find(p) {
            return id;
        }
        let id = self.add_point(p);
        self.cells.entry(Self::cell(p)).or_default().push((p, id));
        id
    }

    fn add_line(&mut self, a: EntityId, b: EntityId) -> EntityId {
        let id = self.id();
        self.sketch.entities.push(Entity::Line {
            id,
            a,
            b,
            construction: false,
        });
        id
    }

    fn add_circle(&mut self, center: P2, radius: f64) -> EntityId {
        let center = self.add_point(center);
        let id = self.id();
        self.sketch.entities.push(Entity::Circle {
            id,
            center,
            radius,
            construction: false,
        });
        id
    }

    fn add_arc(&mut self, center: P2, start: EntityId, end: EntityId) -> EntityId {
        let center = self.add_point(center);
        let id = self.id();
        self.sketch.entities.push(Entity::Arc {
            id,
            center,
            start,
            end,
            construction: false,
        });
        id
    }
}

impl Sketch {
    /// 読んだ形を `place` に置いて要素として足す。拘束は足さない。
    /// 端が [`WELD_TOL`] 以内で重なる線分・円弧は同じ点を共有する(閉じた輪郭になる)。
    /// ベジエ・楕円・B スプラインは [`approximate`] で線分と円弧にして入れる。
    /// 長さが 0 の線や半径が 0 の円は捨てる。
    /// 戻り値は足した線分・円弧・円と、単独の点(線の端や中心の点は含めない)。
    pub fn insert_shapes(&mut self, shapes: &[Shape], place: Place) -> Vec<EntityId> {
        let m = place.affine();
        // 最初の 1 つで控えを今ある最大+1 以上にそろえ、後は控えから数える
        let first = self.next_entity_id();
        self.next_ids.entity = first.0;
        let mut ad = Adder {
            sketch: self,
            cells: HashMap::new(),
        };
        let mut out = Vec::new();
        for s in shapes {
            for piece in approximate(&transform_shape(s, &m)) {
                match piece {
                    Shape::Line { a, b } => {
                        if dist(a, b) <= WELD_TOL {
                            continue;
                        }
                        let pa = ad.end_point(a);
                        let pb = ad.end_point(b);
                        if pa != pb {
                            out.push(ad.add_line(pa, pb));
                        }
                    }
                    Shape::Arc {
                        center,
                        radius,
                        start,
                        end,
                    } => {
                        if radius.is_nan() || radius <= WELD_TOL {
                            continue;
                        }
                        let sweep = arc_sweep(start, end);
                        if sweep >= TAU - 1e-12 {
                            out.push(ad.add_circle(center, radius));
                            continue;
                        }
                        let (ps, pe) = (polar(center, radius, start), polar(center, radius, end));
                        if dist(ps, pe) <= WELD_TOL {
                            // ごく短い弧は捨てる。ほぼ 1 周なら円にする
                            if sweep > PI {
                                out.push(ad.add_circle(center, radius));
                            }
                            continue;
                        }
                        let a = ad.end_point(ps);
                        let b = ad.end_point(pe);
                        if a != b {
                            out.push(ad.add_arc(center, a, b));
                        }
                    }
                    Shape::Circle { center, radius } => {
                        if radius > WELD_TOL {
                            out.push(ad.add_circle(center, radius));
                        }
                    }
                    Shape::Point { p } => out.push(ad.add_point(p)),
                    _ => {}
                }
            }
        }
        out
    }
}
