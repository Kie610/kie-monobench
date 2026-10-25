//! スイープとロフトの曲面の網(M7)。形の計算はここで行い、SMLib には出来た NURBS の曲面を渡して
//! 縫い合わせだけを任せる(ffi::patch_solid)。
//!
//! SMLib のロフト(SmApiCreateSkinPrimitive)は断面ごとの輪郭を 1 本の曲線につないでから張るので、
//! 断面の点どうしの対応(どの角とどの角をつなぐか)を選べず、四角と円のような区間の数の違う断面や、
//! 大きくねじれた断面で形が崩れる。そこで、断面を周の長さの割合でそろえ、全部の断面の角の位置で
//! 区間を分けて、区間ごとに 1 枚の曲面を張る。隣の区間の曲面は同じ境目の点を同じ u の値で通るので、
//! 隙間なく縫える。
//!
//! 曲線は v の向きに 2 次の有理 B スプライン(直線と円弧を正確に表せる)、ロフトの向き(u)は断面を通る
//! 3 次(断面が 2 つなら 1 次)の補間。スイープの直線の区間は 1 次、円弧の区間は回転面を正確に作る。

use kmb_core::kernel::{
    self as k, Matrix4, P3, PlanarPath, Plane, Profile, ProfileEdge, ProfileLoop, Segment,
};
use std::f64::consts::{FRAC_PI_2, TAU};

/// 同次座標の点(w·x, w·y, w·z, w)。
type H = [f64; 4];

fn h(p: P3, w: f64) -> H {
    [p[0] * w, p[1] * w, p[2] * w, w]
}

fn euclid(q: H) -> P3 {
    [q[0] / q[3], q[1] / q[3], q[2] / q[3]]
}

fn dist(a: P3, b: P3) -> f64 {
    let d = k::sub(a, b);
    k::dot(d, d).sqrt()
}

/// 空間の線の 1 区間(直線か円弧。円弧は中心 `c`、半径 `r`、平面の向き `e1`・`e2` で、角 `phi0` から
/// `delta` だけ回る。負なら逆回り)。
#[derive(Debug, Clone)]
enum Geo {
    Line {
        a: P3,
        b: P3,
    },
    Arc {
        c: P3,
        r: f64,
        e1: P3,
        e2: P3,
        phi0: f64,
        delta: f64,
    },
}

impl Geo {
    fn at(&self, t: f64) -> P3 {
        match *self {
            Geo::Line { a, b } => k::add(a, k::scale(k::sub(b, a), t)),
            Geo::Arc {
                c,
                r,
                e1,
                e2,
                phi0,
                delta,
            } => {
                let p = phi0 + delta * t;
                k::add(
                    c,
                    k::add(k::scale(e1, r * p.cos()), k::scale(e2, r * p.sin())),
                )
            }
        }
    }

    /// 向き(長さ 1)。
    fn tangent(&self, t: f64) -> P3 {
        match *self {
            Geo::Line { a, b } => k::normalize(k::sub(b, a)),
            Geo::Arc {
                e1,
                e2,
                phi0,
                delta,
                ..
            } => {
                let p = phi0 + delta * t;
                let s = delta.signum();
                k::normalize(k::add(
                    k::scale(e1, -p.sin() * s),
                    k::scale(e2, p.cos() * s),
                ))
            }
        }
    }

    fn length(&self) -> f64 {
        match *self {
            Geo::Line { a, b } => dist(a, b),
            Geo::Arc { r, delta, .. } => r * delta.abs(),
        }
    }

    fn reversed(&self) -> Geo {
        match *self {
            Geo::Line { a, b } => Geo::Line { a: b, b: a },
            Geo::Arc {
                c,
                r,
                e1,
                e2,
                phi0,
                delta,
            } => Geo::Arc {
                c,
                r,
                e1,
                e2,
                phi0: phi0 + delta,
                delta: -delta,
            },
        }
    }

    /// `t0`〜`t1` の所を、`spans` 個の 2 次の区間に分けた曲線(同次座標の制御点、2·spans + 1 個)。
    fn curve(&self, t0: f64, t1: f64, spans: usize) -> Vec<H> {
        let n = spans.max(1);
        match *self {
            Geo::Line { .. } => (0..=2 * n)
                .map(|i| h(self.at(t0 + (t1 - t0) * i as f64 / (2 * n) as f64), 1.0))
                .collect(),
            Geo::Arc {
                c,
                r,
                e1,
                e2,
                phi0,
                delta,
            } => {
                let p0 = phi0 + delta * t0;
                let step = delta * (t1 - t0) / n as f64;
                let w = (step / 2.0).cos();
                let on = |p: f64, rr: f64| {
                    k::add(
                        c,
                        k::add(k::scale(e1, rr * p.cos()), k::scale(e2, rr * p.sin())),
                    )
                };
                let mut out = Vec::with_capacity(2 * n + 1);
                for i in 0..n {
                    let a = p0 + step * i as f64;
                    out.push(h(on(a, r), 1.0));
                    out.push(h(on(a + step / 2.0, r / w), w));
                }
                out.push(h(on(p0 + step * n as f64, r), 1.0));
                out
            }
        }
    }

    /// `t0`〜`t1` を 2 次の区間で表すのに要る数(円弧は 90 度ごとに 1 つ)。
    fn spans(&self, t0: f64, t1: f64) -> usize {
        match *self {
            Geo::Line { .. } => 1,
            Geo::Arc { delta, .. } => ((delta * (t1 - t0)).abs() / FRAC_PI_2 - 1e-9)
                .ceil()
                .max(1.0) as usize,
        }
    }
}

/// 名前つきの区間。
#[derive(Debug, Clone)]
struct Edge3 {
    geo: Geo,
    name: String,
}

fn edge3(plane: &Plane, e: &ProfileEdge) -> Edge3 {
    let ex = k::normalize(plane.x_axis);
    let ey = k::normalize(plane.y_axis);
    let geo = match e.segment {
        Segment::Line { a, b } => Geo::Line {
            a: plane.to_world(a),
            b: plane.to_world(b),
        },
        Segment::Arc {
            center,
            radius,
            start,
            end,
        } => Geo::Arc {
            c: plane.to_world(center),
            r: radius,
            e1: ex,
            e2: ey,
            phi0: start,
            delta: k::arc_sweep(start, end),
        },
        Segment::Circle { center, radius } => Geo::Arc {
            c: plane.to_world(center),
            r: radius,
            e1: ex,
            e2: ey,
            phi0: 0.0,
            delta: TAU,
        },
    };
    Edge3 {
        geo: if e.reversed { geo.reversed() } else { geo },
        name: e.name.clone(),
    }
}

/// 空間の閉じた輪郭。`starts[i]` は区間 i の始まりの、周の長さの割合(0 から)。
#[derive(Debug, Clone)]
struct Loop3 {
    edges: Vec<Edge3>,
    starts: Vec<f64>,
    total: f64,
    /// 区間 i の始まりが角(向きが折れる)か
    corner: Vec<bool>,
    normal: P3,
}

impl Loop3 {
    fn new(edges: Vec<Edge3>, normal: P3) -> Result<Loop3, String> {
        let lens: Vec<f64> = edges.iter().map(|e| e.geo.length()).collect();
        let total: f64 = lens.iter().sum();
        if total <= 1e-9 {
            return Err("輪郭の長さが 0 です".into());
        }
        let mut starts = Vec::with_capacity(edges.len());
        let mut acc = 0.0;
        for l in &lens {
            starts.push(acc / total);
            acc += l;
        }
        let n = edges.len();
        let corner = (0..n)
            .map(|i| {
                let prev = &edges[(i + n - 1) % n].geo;
                k::dot(prev.tangent(1.0), edges[i].geo.tangent(0.0)) < 1.0 - 1e-6
            })
            .collect();
        Ok(Loop3 {
            edges,
            starts,
            total,
            corner,
            normal,
        })
    }

    fn from_profile(plane: &Plane, l: &ProfileLoop) -> Result<Loop3, String> {
        let n = k::scale(plane.normal(), l.signed_area().signum());
        Loop3::new(l.edges.iter().map(|e| edge3(plane, e)).collect(), n)
    }

    fn reversed(&self) -> Loop3 {
        let edges = self
            .edges
            .iter()
            .rev()
            .map(|e| Edge3 {
                geo: e.geo.reversed(),
                name: e.name.clone(),
            })
            .collect();
        Loop3::new(edges, k::scale(self.normal, -1.0)).expect("長さは同じ")
    }

    fn len_of(&self, i: usize) -> f64 {
        let next = if i + 1 < self.starts.len() {
            self.starts[i + 1]
        } else {
            1.0
        };
        next - self.starts[i]
    }

    /// 割合 `s` の所の区間と、区間の中の位置(0〜1)。
    fn locate(&self, s: f64) -> (usize, f64) {
        let s = s.rem_euclid(1.0);
        let i = self
            .starts
            .iter()
            .rposition(|&a| a <= s + 1e-15)
            .unwrap_or_default();
        (i, ((s - self.starts[i]) / self.len_of(i)).clamp(0.0, 1.0))
    }

    fn at(&self, s: f64) -> P3 {
        let (i, t) = self.locate(s);
        self.edges[i].geo.at(t)
    }

    /// 点 `p` にいちばん近い所の割合。
    fn nearest(&self, p: P3) -> (f64, f64) {
        let n = 2000;
        let mut best = (0.0, f64::INFINITY);
        for i in 0..n {
            let s = i as f64 / n as f64;
            let d = dist(self.at(s), p);
            if d < best.1 {
                best = (s, d);
            }
        }
        // 前後の刻みの間を詰める
        let (mut lo, mut hi) = (best.0 - 1.0 / n as f64, best.0 + 1.0 / n as f64);
        for _ in 0..60 {
            let a = lo + (hi - lo) / 3.0;
            let b = hi - (hi - lo) / 3.0;
            if dist(self.at(a), p) < dist(self.at(b), p) {
                hi = b;
            } else {
                lo = a;
            }
        }
        let s = ((lo + hi) / 2.0).rem_euclid(1.0);
        (s, dist(self.at(s), p))
    }

    fn centroid(&self) -> P3 {
        let n = 256;
        let mut c = [0.0; 3];
        for i in 0..n {
            c = k::add(c, self.at(i as f64 / n as f64));
        }
        k::scale(c, 1.0 / n as f64)
    }

    fn has_corner(&self) -> bool {
        self.corner.iter().any(|c| *c)
    }
}

/// 端を含む、2 次の区間 `spans` 個のノット(区間の継ぎ目は 2 重)。
fn arc_knots(spans: usize) -> Vec<f64> {
    let mut v = vec![0.0; 3];
    for i in 1..spans {
        let t = i as f64 / spans as f64;
        v.push(t);
        v.push(t);
    }
    v.extend([1.0; 3]);
    v
}

/// 曲面 1 枚(制御点は u ごとに v の並び)。
pub struct Surface {
    pub degree_u: usize,
    pub knots_u: Vec<f64>,
    pub knots_v: Vec<f64>,
    pub rows: Vec<Vec<H>>,
    /// 1: u の最小を始まりの端の面に、2: 最大を終わりの端の面に
    pub caps: i32,
    pub name: String,
}

impl Surface {
    pub fn patch(&self, tag: i64) -> crate::ffi::Patch {
        crate::ffi::Patch {
            degree_u: self.degree_u as u32,
            degree_v: 2,
            knots_u: self.knots_u.clone(),
            knots_v: self.knots_v.clone(),
            points: self
                .rows
                .iter()
                .flatten()
                .map(|q| {
                    let p = euclid(*q);
                    [p[0], p[1], p[2], q[3]]
                })
                .collect(),
            tag,
            caps: self.caps,
        }
    }
}

// ---- B スプラインの補間 ----

pub(crate) fn find_span(knots: &[f64], degree: usize, count: usize, u: f64) -> usize {
    if u >= knots[count] {
        return count - 1;
    }
    let (mut lo, mut hi) = (degree, count);
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if u < knots[mid] {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    lo
}

pub(crate) fn basis(knots: &[f64], degree: usize, span: usize, u: f64) -> Vec<f64> {
    let mut n = vec![0.0; degree + 1];
    let mut left = vec![0.0; degree + 1];
    let mut right = vec![0.0; degree + 1];
    n[0] = 1.0;
    for j in 1..=degree {
        left[j] = u - knots[span + 1 - j];
        right[j] = knots[span + j] - u;
        let mut saved = 0.0;
        for r in 0..j {
            let d = right[r + 1] + left[j - r];
            let tmp = if d.abs() < 1e-300 { 0.0 } else { n[r] / d };
            n[r] = saved + right[r + 1] * tmp;
            saved = left[j - r] * tmp;
        }
        n[j] = saved;
    }
    n
}

/// 値 `params` で点を通る、次数 `degree` の補間のノットと、点の組から制御点を解く関数。
struct Interp {
    knots: Vec<f64>,
    /// 解いた行列(LU)
    lu: Vec<Vec<f64>>,
    piv: Vec<usize>,
}

impl Interp {
    fn new(params: &[f64], degree: usize) -> Interp {
        let n = params.len();
        let mut knots = vec![0.0; degree + 1];
        for j in 1..n - degree {
            knots.push(params[j..j + degree].iter().sum::<f64>() / degree as f64);
        }
        knots.extend(vec![1.0; degree + 1]);
        let mut a = vec![vec![0.0; n]; n];
        for (i, &u) in params.iter().enumerate() {
            let span = find_span(&knots, degree, n, u);
            for (k, b) in basis(&knots, degree, span, u).into_iter().enumerate() {
                a[i][span - degree + k] = b;
            }
        }
        // 部分ピボットつきの LU 分解
        let mut piv: Vec<usize> = (0..n).collect();
        for c in 0..n {
            let p = (c..n)
                .max_by(|&x, &y| a[x][c].abs().total_cmp(&a[y][c].abs()))
                .unwrap_or(c);
            a.swap(c, p);
            piv.swap(c, p);
            for r in c + 1..n {
                let f = if a[c][c].abs() < 1e-300 {
                    0.0
                } else {
                    a[r][c] / a[c][c]
                };
                a[r][c] = f;
                let (top, low) = a.split_at_mut(r);
                for (x, v) in low[0].iter_mut().enumerate().skip(c + 1) {
                    *v -= f * top[c][x];
                }
            }
        }
        Interp { knots, lu: a, piv }
    }

    fn solve(&self, rhs: &[H]) -> Vec<H> {
        let n = rhs.len();
        let mut y: Vec<H> = self.piv.iter().map(|&p| rhs[p]).collect();
        for r in 0..n {
            for c in 0..r {
                let (f, yc) = (self.lu[r][c], y[c]);
                for (v, w) in y[r].iter_mut().zip(yc) {
                    *v -= f * w;
                }
            }
        }
        for r in (0..n).rev() {
            for c in r + 1..n {
                let (f, yc) = (self.lu[r][c], y[c]);
                for (v, w) in y[r].iter_mut().zip(yc) {
                    *v -= f * w;
                }
            }
            let d = self.lu[r][r];
            for v in &mut y[r] {
                *v /= d;
            }
        }
        y
    }
}

/// 断面の曲線(同じ数の制御点)を、値 `params` で通る曲面の制御点の行。次数は断面の数で決める。
fn skin_rows(sections: &[Vec<H>], params: &[f64]) -> (usize, Vec<f64>, Vec<Vec<H>>) {
    let degree = (sections.len() - 1).min(3);
    let it = Interp::new(params, degree);
    let cols = sections[0].len();
    let mut rows = vec![vec![[0.0; 4]; cols]; sections.len()];
    for c in 0..cols {
        let column: Vec<H> = sections.iter().map(|s| s[c]).collect();
        for (r, q) in it.solve(&column).into_iter().enumerate() {
            rows[r][c] = q;
        }
    }
    (degree, it.knots, rows)
}

/// 曲面の u = `u` の所の断面(同次座標の制御点)。
fn iso(degree: usize, knots: &[f64], rows: &[Vec<H>], u: f64) -> Vec<H> {
    let span = find_span(knots, degree, rows.len(), u);
    let b = basis(knots, degree, span, u);
    (0..rows[0].len())
        .map(|c| {
            let mut q = [0.0; 4];
            for (i, w) in b.iter().enumerate() {
                for d in 0..4 {
                    q[d] += w * rows[span - degree + i][c][d];
                }
            }
            q
        })
        .collect()
}

/// 同次座標の点を、アフィンの変換 `m` で動かす(重みは変えない)。
fn moved(q: H, m: &Matrix4) -> H {
    h(k::transform_point(m, euclid(q)), q[3])
}

fn matmul(a: &Matrix4, b: &Matrix4) -> Matrix4 {
    let mut m = [[0.0; 4]; 4];
    for (r, row) in m.iter_mut().enumerate() {
        for (c, v) in row.iter_mut().enumerate() {
            *v = (0..4).map(|i| a[r][i] * b[i][c]).sum();
        }
    }
    m
}

// ---- ロフト ----

/// 断面をそろえた結果: 断面ごとの、区間の境目の周の割合(始まりを含み、終わりは始まり + 1)。
struct Aligned {
    loops: Vec<Loop3>,
    /// 断面ごとの境目(区間の数 + 1 個、増えていく)
    bounds: Vec<Vec<f64>>,
}

/// 断面 `l` を、`reference`(始まり `ref_start`)にいちばん重なるように回したときの始まりの割合。
/// 角がある断面は角から、角の無い断面(円など)は周を細かく刻んだ所から選ぶ。中心を引いてから
/// 比べるので、大きさや位置の違いには左右されず、ねじれの量に上限は無い。
fn best_start(l: &Loop3, reference: &Loop3, ref_start: f64) -> f64 {
    let m = 96;
    let cr = reference.centroid();
    let cl = l.centroid();
    let refs: Vec<P3> = (0..m)
        .map(|i| k::sub(reference.at(ref_start + (i as f64 + 0.5) / m as f64), cr))
        .collect();
    let candidates: Vec<f64> = if l.has_corner() {
        (0..l.edges.len())
            .filter(|&i| l.corner[i])
            .map(|i| l.starts[i])
            .collect()
    } else {
        (0..720)
            .map(|i| i as f64 / 720.0)
            .chain(l.starts.iter().copied())
            .collect()
    };
    let cost = |t: f64| -> f64 {
        refs.iter()
            .enumerate()
            .map(|(i, r)| {
                let p = k::sub(l.at(t + (i as f64 + 0.5) / m as f64), cl);
                let d = k::sub(p, *r);
                k::dot(d, d)
            })
            .sum()
    };
    candidates
        .into_iter()
        .min_by(|a, b| cost(*a).total_cmp(&cost(*b)))
        .unwrap_or(0.0)
}

/// 境目の割合を、断面の角(区間の始まり)に近ければそこへ寄せる。
fn snap(l: &Loop3, s: f64) -> f64 {
    let base = s.floor();
    for &a in &l.starts {
        for off in [-1.0, 0.0, 1.0] {
            let v = base + off + a;
            if (v - s).abs() < 1e-5 {
                return v;
            }
        }
    }
    s
}

/// 断面をそろえる。`anchors[i]` があれば断面 i の始まりをそこにし(1 本目のガイドレール)、
/// `seconds[i]` があれば、その所どうしも対応させる(2 本目のガイドレール)。
fn align(mut loops: Vec<Loop3>, anchors: Option<&[f64]>, seconds: Option<&[f64]>) -> Aligned {
    // 向きをそろえる(隣の断面と輪郭の向き(法線)が逆なら、たどる向きを逆にする)
    for i in 1..loops.len() {
        if k::dot(loops[i].normal, loops[i - 1].normal) < 0.0 {
            loops[i] = loops[i].reversed();
        }
    }
    let n = loops.len();
    let starts: Vec<f64> = match anchors {
        Some(a) => a.to_vec(),
        None => {
            // 角のある最初の断面を基準にし、そこから前後へ順にそろえる
            let r = loops.iter().position(Loop3::has_corner).unwrap_or(0);
            let mut s = vec![0.0; n];
            s[r] = loops[r]
                .corner
                .iter()
                .position(|c| *c)
                .map_or(0.0, |i| loops[r].starts[i]);
            for i in r + 1..n {
                s[i] = best_start(&loops[i], &loops[i - 1], s[i - 1]);
            }
            for i in (0..r).rev() {
                s[i] = best_start(&loops[i], &loops[i + 1], s[i + 1]);
            }
            s
        }
    };
    // 2 本目のレールの所(始まりからの割合)
    let rel2: Option<Vec<f64>> = seconds.map(|v| {
        v.iter()
            .zip(&starts)
            .map(|(p, s)| (p - s).rem_euclid(1.0))
            .collect()
    });
    let a2 = rel2.as_ref().map(|r| r[0]);
    // 全体の値 g(0〜1)を断面 i の割合へ: 2 本目のレールがあれば、その所で折れる線分で
    let to_loop = |i: usize, g: f64| -> f64 {
        let rel = match (&rel2, a2) {
            (Some(r), Some(a)) if a > 1e-9 && a < 1.0 - 1e-9 => {
                if g <= a {
                    g / a * r[i]
                } else {
                    r[i] + (g - a) / (1.0 - a) * (1.0 - r[i])
                }
            }
            _ => g,
        };
        starts[i] + rel
    };
    let to_global = |i: usize, s: f64| -> f64 {
        let rel = (s - starts[i]).rem_euclid(1.0);
        match (&rel2, a2) {
            (Some(r), Some(a)) if a > 1e-9 && a < 1.0 - 1e-9 => {
                if rel <= r[i] {
                    rel / r[i].max(1e-12) * a
                } else {
                    a + (rel - r[i]) / (1.0 - r[i]).max(1e-12) * (1.0 - a)
                }
            }
            _ => rel,
        }
    };
    let mut gs: Vec<f64> = vec![0.0];
    match a2 {
        Some(a) => gs.push(a),
        // レールが 1 本なら、反対側(周の半分)を動かさない所にするので、そこでも分ける
        None if anchors.is_some() => gs.push(0.5),
        None => {}
    }
    for (i, l) in loops.iter().enumerate() {
        for &a in &l.starts {
            gs.push(to_global(i, a));
        }
    }
    gs.retain(|g| *g < 1.0 - 1e-5);
    gs.sort_by(f64::total_cmp);
    gs.dedup_by(|a, b| (*a - *b).abs() < 1e-5);
    gs.push(1.0);
    let bounds = loops
        .iter()
        .enumerate()
        .map(|(i, l)| gs.iter().map(|g| snap(l, to_loop(i, *g))).collect())
        .collect();
    Aligned { loops, bounds }
}

/// 区間 `b` の、断面 `i` の曲線。区間は断面の 1 つの区間の中に収まる。
fn piece(al: &Aligned, i: usize, b: usize, spans: usize) -> (Vec<H>, &str) {
    let l = &al.loops[i];
    let (s0, s1) = (al.bounds[i][b], al.bounds[i][b + 1]);
    let mid = (s0 + s1) / 2.0;
    let base = mid.floor();
    let (e, _) = l.locate(mid);
    let len = l.len_of(e);
    let t0 = ((s0 - base - l.starts[e]) / len).clamp(0.0, 1.0);
    let t1 = ((s1 - base - l.starts[e]) / len).clamp(0.0, 1.0);
    let geo = &l.edges[e].geo;
    let n = spans.max(geo.spans(t0, t1));
    (geo.curve(t0, t1, n), &l.edges[e].name)
}

fn piece_spans(al: &Aligned, i: usize, b: usize) -> usize {
    let l = &al.loops[i];
    let (s0, s1) = (al.bounds[i][b], al.bounds[i][b + 1]);
    let mid = (s0 + s1) / 2.0;
    let base = mid.floor();
    let (e, _) = l.locate(mid);
    let len = l.len_of(e);
    let t0 = ((s0 - base - l.starts[e]) / len).clamp(0.0, 1.0);
    let t1 = ((s1 - base - l.starts[e]) / len).clamp(0.0, 1.0);
    l.edges[e].geo.spans(t0, t1)
}

/// 経路やレールを、空間の区間の並びにする。
fn path3(p: &PlanarPath) -> Vec<Edge3> {
    p.edges.iter().map(|e| edge3(&p.plane, e)).collect()
}

/// 区間の並び(開いた線)の、長さの割合 `r`(0〜1)の所。
fn path_at(edges: &[Edge3], lens: &[f64], r: f64) -> P3 {
    let total: f64 = lens.iter().sum();
    let mut x = r.clamp(0.0, 1.0) * total;
    for (e, l) in edges.iter().zip(lens) {
        if x <= *l || std::ptr::eq(e, edges.last().expect("区間がある")) {
            return e.geo.at((x / l.max(1e-300)).clamp(0.0, 1.0));
        }
        x -= l;
    }
    edges[0].geo.at(0.0)
}

/// ガイドレール: 断面の平面と交わる所(長さの割合)と、そこでの輪郭の上の点。
struct Rail {
    edges: Vec<Edge3>,
    lens: Vec<f64>,
    /// 断面ごとの、レールの長さの割合
    at: Vec<f64>,
}

impl Rail {
    fn new(p: &PlanarPath, loops: &[Loop3], which: usize) -> Result<Rail, String> {
        let edges = path3(p);
        if edges.is_empty() {
            return Err(format!("ガイドレール {which} に線がありません"));
        }
        let lens: Vec<f64> = edges.iter().map(|e| e.geo.length()).collect();
        let n = 2000;
        let samples: Vec<P3> = (0..=n)
            .map(|i| path_at(&edges, &lens, i as f64 / n as f64))
            .collect();
        let size = loops.iter().map(|l| l.total).fold(0.0, f64::max);
        let tol = (size * 1e-4).max(1e-3);
        let mut at = Vec::with_capacity(loops.len());
        for (i, l) in loops.iter().enumerate() {
            // レールの上で、輪郭にいちばん近い所
            let (mut best, mut bd) = (0usize, f64::INFINITY);
            for (j, q) in samples.iter().enumerate() {
                let d = dist(*q, l.at(l.nearest_coarse(*q)));
                if d < bd {
                    (best, bd) = (j, d);
                }
            }
            let (mut lo, mut hi) = (
                (best as f64 - 1.0) / n as f64,
                (best as f64 + 1.0) / n as f64,
            );
            let gap = |r: f64| {
                let q = path_at(&edges, &lens, r);
                l.nearest(q).1
            };
            for _ in 0..50 {
                let a = lo + (hi - lo) / 3.0;
                let b = hi - (hi - lo) / 3.0;
                if gap(a) < gap(b) {
                    hi = b;
                } else {
                    lo = a;
                }
            }
            let r = ((lo + hi) / 2.0).clamp(0.0, 1.0);
            if gap(r) > tol {
                return Err(format!(
                    "ガイドレール {which} が断面 {} の輪郭に触れていません(レールの線を断面の輪郭の上の点まで描いてください)",
                    i + 1
                ));
            }
            at.push(r);
        }
        let up = at.windows(2).all(|w| w[1] > w[0]);
        let down = at.windows(2).all(|w| w[1] < w[0]);
        if !up && !down {
            return Err(format!(
                "ガイドレール {which} が断面を順に通っていません(最初の断面から最後の断面まで続く線にしてください)"
            ));
        }
        Ok(Rail { edges, lens, at })
    }

    fn point(&self, r: f64) -> P3 {
        path_at(&self.edges, &self.lens, r)
    }
}

impl Loop3 {
    fn nearest_coarse(&self, p: P3) -> f64 {
        let n = 200;
        (0..n)
            .map(|i| i as f64 / n as f64)
            .min_by(|a, b| dist(self.at(*a), p).total_cmp(&dist(self.at(*b), p)))
            .unwrap_or(0.0)
    }
}

/// 2 点 `p1`→`t1`、`p2`→`t2` へ動かす相似の変換(1 点だけなら平行移動)。
fn similarity(p1: P3, t1: P3, second: Option<(P3, P3)>) -> Matrix4 {
    let Some((p2, t2)) = second else {
        return k::translation(k::sub(t1, p1));
    };
    let v = k::sub(p2, p1);
    let w = k::sub(t2, t1);
    let (lv, lw) = (k::dot(v, v).sqrt(), k::dot(w, w).sqrt());
    if lv < 1e-12 || lw < 1e-12 {
        return k::translation(k::sub(t1, p1));
    }
    let s = lw / lv;
    let (vn, wn) = (k::scale(v, 1.0 / lv), k::scale(w, 1.0 / lw));
    let axis = k::cross(vn, wn);
    let sin = k::dot(axis, axis).sqrt();
    let angle = sin.atan2(k::dot(vn, wn));
    let rot = if sin < 1e-12 {
        k::IDENTITY
    } else {
        k::rotation([0.0; 3], axis, angle)
    };
    let mut m = matmul(&rot, &k::translation(k::scale(p1, -1.0)));
    for row in m.iter_mut().take(3) {
        for v in row.iter_mut() {
            *v *= s;
        }
    }
    matmul(&k::translation(t1), &m)
}

/// 点 `fixed` を通り `fixed`→`from` に垂直な平面は動かさず、`from` を `to` へ動かすように、その向きの
/// 距離に比べて動かす変換(レールが 1 本の時。レールの側だけが引っ張られ、反対側は元のまま)。
fn stretch(fixed: P3, from: P3, to: P3) -> Matrix4 {
    let d = k::sub(from, fixed);
    let len2 = k::dot(d, d);
    if len2 < 1e-18 {
        return k::translation(k::sub(to, from));
    }
    let mv = k::sub(to, from);
    // x ↦ x + mv·((x − fixed)·d)/|d|²
    let mut m = k::IDENTITY;
    for r in 0..3 {
        for c in 0..3 {
            m[r][c] += mv[r] * d[c] / len2;
        }
        m[r][3] = -mv[r] * k::dot(fixed, d) / len2;
    }
    m
}

/// ロフトの曲面の網。断面は穴の無い閉じた輪郭 1 つずつ(穴のある断面は、呼ぶ側が外側と穴に分けて
/// 別にロフトし、穴を引く)。
pub fn loft(sections: &[Profile], rails: &[PlanarPath], tag: &str) -> Result<Vec<Surface>, String> {
    if sections.len() < 2 {
        return Err("ロフトには断面が 2 つ以上要ります".into());
    }
    if rails.len() > 2 {
        return Err("ガイドレールは 2 本までです".into());
    }
    let mut loops = Vec::with_capacity(sections.len());
    for (i, p) in sections.iter().enumerate() {
        let [r] = p.regions.as_slice() else {
            return Err(format!(
                "断面 {} には閉じた輪郭を 1 つだけ選んでください(今は {} つ)",
                i + 1,
                p.regions.len()
            ));
        };
        if !r.holes.is_empty() {
            return Err(format!(
                "断面 {} に穴があります。ロフトの断面は穴の無い輪郭にしてください",
                i + 1
            ));
        }
        loops.push(Loop3::from_profile(&p.plane, &r.outer)?);
    }
    // k9: 続きの断面が同じ平面にあると、間をつなげない
    for i in 1..sections.len() {
        let (a, b) = (&sections[i - 1].plane, &sections[i].plane);
        let (na, nb) = (k::normalize(a.normal()), k::normalize(b.normal()));
        if k::dot(na, nb).abs() > 1.0 - 1e-9 && k::dot(k::sub(b.origin, a.origin), na).abs() < 1e-6
        {
            return Err(format!(
                "断面 {i} と断面 {} が同じ平面にあります。続けて並べる断面は別の平面に置いてください(同じ平面の輪郭をつなぐと、厚さの無い形になります)",
                i + 1
            ));
        }
    }
    let rails: Vec<Rail> = rails
        .iter()
        .enumerate()
        .map(|(i, r)| Rail::new(r, &loops, i + 1))
        .collect::<Result<_, _>>()?;
    // レールに触れる所の割合は、輪郭の向きをそろえてから探す
    let al = if rails.is_empty() {
        align(loops, None, None)
    } else {
        let mut ls = loops;
        for i in 1..ls.len() {
            if k::dot(ls[i].normal, ls[i - 1].normal) < 0.0 {
                ls[i] = ls[i].reversed();
            }
        }
        let pick = |r: &Rail| -> Vec<f64> {
            ls.iter()
                .zip(&r.at)
                .map(|(l, a)| l.nearest(r.point(*a)).0)
                .collect()
        };
        let a = pick(&rails[0]);
        let s = rails.get(1).map(pick);
        align(ls, Some(&a), s.as_deref())
    };
    // 断面の中心の間の長さの割合で、u の値を決める
    let cs: Vec<P3> = al.loops.iter().map(Loop3::centroid).collect();
    let mut params = vec![0.0];
    for w in cs.windows(2) {
        let d = dist(w[0], w[1]).max(1e-9);
        params.push(params.last().expect("始まりがある") + d);
    }
    let total = *params.last().expect("値がある");
    for p in &mut params {
        *p /= total;
    }
    let pieces = al.bounds[0].len() - 1;
    let mut out = Vec::with_capacity(pieces);
    for b in 0..pieces {
        let spans = (0..al.loops.len())
            .map(|i| piece_spans(&al, i, b))
            .max()
            .unwrap_or(1);
        let curves: Vec<Vec<H>> = (0..al.loops.len())
            .map(|i| piece(&al, i, b, spans).0)
            .collect();
        let name = piece(&al, 0, b, spans).1.to_string();
        let (degree, knots, rows) = skin_rows(&curves, &params);
        out.push(Surface {
            degree_u: degree,
            knots_u: knots,
            knots_v: arc_knots(spans),
            rows,
            caps: 3,
            name: format!("{tag}:side:{name}"),
        });
    }
    if !rails.is_empty() {
        out = follow_rails(out, &params, &rails, &al)?;
    }
    Ok(out)
}

/// ガイドレールに沿わせる: 断面の間に断面を差し、差した断面をレールの所へ動かしてから張り直す。
fn follow_rails(
    surfaces: Vec<Surface>,
    params: &[f64],
    rails: &[Rail],
    al: &Aligned,
) -> Result<Vec<Surface>, String> {
    const STEPS: usize = 8;
    // 2 本目のレールが触れる所から始まる区間(断面 0 で、その点にいちばん近い境目)。レールが 1 本なら、
    // 反対側(周の半分の所に近い境目)を動かさない所にする
    let nearest_bound = |p: P3| {
        let l = &al.loops[0];
        (0..al.bounds[0].len() - 1).min_by(|x, y| {
            dist(l.at(al.bounds[0][*x]), p).total_cmp(&dist(l.at(al.bounds[0][*y]), p))
        })
    };
    let second_piece = match rails.get(1) {
        Some(r) => nearest_bound(r.point(r.at[0])),
        None => nearest_bound(al.loops[0].at(al.bounds[0][0] + 0.5)),
    };
    let mut us: Vec<f64> = Vec::new();
    for i in 0..params.len() {
        us.push(params[i]);
        if i + 1 < params.len() {
            for s in 1..STEPS {
                us.push(params[i] + (params[i + 1] - params[i]) * s as f64 / STEPS as f64);
            }
        }
    }
    let rail_at = |r: &Rail, u: f64| -> P3 {
        let i = params
            .windows(2)
            .position(|w| u <= w[1] + 1e-12)
            .unwrap_or(params.len() - 2);
        let f = (u - params[i]) / (params[i + 1] - params[i]);
        r.point(r.at[i] + (r.at[i + 1] - r.at[i]) * f)
    };
    // 差す断面ごとの動かし方(全部の区間で同じ)
    let moves: Vec<Matrix4> = us
        .iter()
        .map(|&u| {
            let first = euclid(
                iso(
                    surfaces[0].degree_u,
                    &surfaces[0].knots_u,
                    &surfaces[0].rows,
                    u,
                )[0],
            );
            let point_of = |j: usize| {
                let s = &surfaces[j];
                euclid(iso(s.degree_u, &s.knots_u, &s.rows, u)[0])
            };
            let target = rail_at(&rails[0], u);
            match (rails.get(1), second_piece) {
                (Some(r2), Some(j)) => {
                    similarity(first, target, Some((point_of(j), rail_at(r2, u))))
                }
                (None, Some(j)) => stretch(point_of(j), first, target),
                _ => similarity(first, target, None),
            }
        })
        .collect();
    Ok(surfaces
        .into_iter()
        .map(|s| {
            let sections: Vec<Vec<H>> = us
                .iter()
                .zip(&moves)
                .map(|(&u, m)| {
                    iso(s.degree_u, &s.knots_u, &s.rows, u)
                        .into_iter()
                        .map(|q| moved(q, m))
                        .collect()
                })
                .collect();
            let (degree, knots, rows) = skin_rows(&sections, &us);
            Surface {
                degree_u: degree,
                knots_u: knots,
                rows,
                ..s
            }
        })
        .collect())
}

// ---- スイープ ----

/// 角の二等分の平面へ、向き `d` に沿って写す変換(角の点 `k`、平面の法線 `m`)。
fn miter(kp: P3, d: P3, m: P3) -> Matrix4 {
    // x ↦ x − d·((x − k)·m)/(d·m)
    let f = 1.0 / k::dot(d, m);
    let mut out = k::IDENTITY;
    for r in 0..3 {
        for c in 0..3 {
            out[r][c] -= d[r] * m[c] * f;
        }
        out[r][3] = d[r] * k::dot(kp, m) * f;
    }
    out
}

/// 回転面: 曲線 `curve`(同次座標)を、点 `o` を通る軸 `axis` まわりに `angle` 回す。
fn revolve_rows(curve: &[H], o: P3, axis: P3, angle: f64) -> (Vec<f64>, Vec<Vec<H>>) {
    let a = k::normalize(axis);
    let spans = (angle.abs() / FRAC_PI_2 - 1e-9).ceil().max(1.0) as usize;
    let step = angle / spans as f64;
    let w = (step / 2.0).cos();
    let rot = |p: P3, ang: f64, scale: f64| -> P3 {
        let d = k::sub(p, o);
        let foot = k::add(o, k::scale(a, k::dot(d, a)));
        let rv = k::sub(p, foot);
        let (s, c) = ang.sin_cos();
        let r = k::add(
            k::add(k::scale(rv, c), k::scale(k::cross(a, rv), s)),
            k::scale(a, k::dot(a, rv) * (1.0 - c)),
        );
        k::add(foot, k::scale(r, scale))
    };
    let mut rows = Vec::with_capacity(2 * spans + 1);
    for i in 0..=2 * spans {
        let ang = step * i as f64 / 2.0;
        let mid = i % 2 == 1;
        rows.push(
            curve
                .iter()
                .map(|q| {
                    let p = euclid(*q);
                    if mid {
                        h(rot(p, ang, 1.0 / w), q[3] * w)
                    } else {
                        h(rot(p, ang, 1.0), q[3])
                    }
                })
                .collect(),
        );
    }
    (arc_knots(spans), rows)
}

/// ねじれ・大きさを変えるスイープで、断面を並べる間隔の角度の上限(輪郭の回りと経路の円弧の曲がり)。
const SHAPED_STEP: f64 = TAU / 36.0;

/// 輪郭を点 `p0` を通る軸 `axis` のまわりに `twist × t` 回し、`p0` を中心に `1 + (scale − 1) × t` 倍にする変換。
fn shape_at(p0: P3, axis: P3, twist: f64, scale: f64, t: f64) -> Matrix4 {
    let f = 1.0 + (scale - 1.0) * t;
    let mut m = k::IDENTITY;
    for r in 0..3 {
        m[r][r] = f;
        m[r][3] = p0[r] * (1.0 - f);
    }
    matmul(&k::rotation(p0, axis, twist * t), &m)
}

/// 2 つの変換を割合 `s` で混ぜた変換(点を混ぜるのと同じ)。
fn blend(a: &Matrix4, b: &Matrix4, s: f64) -> Matrix4 {
    let mut m = *a;
    for r in 0..4 {
        for c in 0..4 {
            m[r][c] = a[r][c] * (1.0 - s) + b[r][c] * s;
        }
    }
    m
}

/// スイープの曲面の網。`twist`(ラジアン)と `scale`(終わりの倍率)は経路の長さに比べて一様に効かせる。
pub fn sweep(
    profile: &Profile,
    path: &PlanarPath,
    twist: f64,
    scale: f64,
    tag: &str,
) -> Result<Vec<Surface>, String> {
    if profile.regions.is_empty() {
        return Err("閉じた輪郭がありません".into());
    }
    if !twist.is_finite() || !scale.is_finite() || scale <= 0.0 {
        return Err("ねじれは数、終わりの大きさは 0 より大きい倍率にしてください".into());
    }
    let shaped = twist.abs() > 1e-12 || (scale - 1.0).abs() > 1e-12;
    let mut edges = path3(path);
    if edges.is_empty() {
        return Err("経路に線がありません".into());
    }
    for w in edges.windows(2) {
        if dist(w[0].geo.at(1.0), w[1].geo.at(0.0)) > 1e-6 {
            return Err(
                "経路の線がつながっていません(端と端がつながった線を選んでください)".into(),
            );
        }
    }
    let closed = dist(edges[edges.len() - 1].geo.at(1.0), edges[0].geo.at(0.0)) < 1e-6;
    if closed && shaped {
        return Err(
            "閉じた経路では、ねじれと終わりの大きさは使えません(一周した輪郭が始まりと合わなくなるため)"
                .into(),
        );
    }
    let plane = profile.plane;
    let n = k::normalize(plane.normal());
    let on_plane = |p: P3| k::dot(k::sub(p, plane.origin), n).abs() < 1e-4;
    if closed {
        // 輪郭の平面の上にある頂点から始める
        let i = (0..edges.len())
            .find(|&i| on_plane(edges[i].geo.at(0.0)))
            .ok_or("閉じた経路の頂点の 1 つを、輪郭の平面の上に置いてください")?;
        edges.rotate_left(i);
    } else if !on_plane(edges[0].geo.at(0.0)) {
        if on_plane(edges[edges.len() - 1].geo.at(1.0)) {
            edges = edges
                .iter()
                .rev()
                .map(|e| Edge3 {
                    geo: e.geo.reversed(),
                    name: e.name.clone(),
                })
                .collect();
        } else {
            return Err(
                "経路の端が輪郭の平面の上にありません(経路は輪郭の平面から始めてください)".into(),
            );
        }
    }
    if k::dot(edges[0].geo.tangent(0.0), n).abs() < 1e-3 {
        return Err(
            "経路が輪郭の平面に沿っています(経路は輪郭の平面を横切る向きにしてください)".into(),
        );
    }
    let pn = k::normalize(path.plane.normal());
    let loops: Vec<Loop3> = profile
        .regions
        .iter()
        .flat_map(|r| std::iter::once(&r.outer).chain(&r.holes))
        .map(|l| Loop3::from_profile(&plane, l))
        .collect::<Result<_, _>>()?;
    let count = edges.len();
    // 区間ごとの始まりと終わりの、輪郭の動かし方(回転と平行移動)
    let mut frames: Vec<(Matrix4, Matrix4)> = Vec::with_capacity(count);
    let mut f = k::IDENTITY;
    for (i, e) in edges.iter().enumerate() {
        let end = match e.geo {
            Geo::Line { a, b } => matmul(&k::translation(k::sub(b, a)), &f),
            Geo::Arc {
                c, e1, e2, delta, ..
            } => matmul(&k::rotation(c, k::cross(e1, e2), delta), &f),
        };
        frames.push((f, end));
        f = end;
        let next = if i + 1 < count {
            Some(&edges[i + 1])
        } else if closed {
            Some(&edges[0])
        } else {
            None
        };
        if let Some(nx) = next {
            let (d1, d2) = (e.geo.tangent(1.0), nx.geo.tangent(0.0));
            let turn = k::dot(k::cross(d1, d2), pn).atan2(k::dot(d1, d2));
            if turn.abs() > 1e-9 {
                let lines = matches!(e.geo, Geo::Line { .. }) && matches!(nx.geo, Geo::Line { .. });
                if !lines && turn.abs() > 1e-3 {
                    return Err(format!(
                        "経路の {} と {} が角でつながっています。円弧の所はなめらかにつないでください(角は直線どうしだけ使えます)",
                        e.name, nx.name
                    ));
                }
                if lines {
                    f = matmul(&k::rotation(e.geo.at(1.0), pn, turn), &f);
                }
            }
        }
    }
    // 継ぎ目の断面の変換: 直線どうしの角は二等分の平面へ写す
    let joint = |i: usize| -> Matrix4 {
        // 区間 i の終わり
        let e = &edges[i];
        let nx = if i + 1 < count {
            Some(&edges[i + 1])
        } else if closed {
            Some(&edges[0])
        } else {
            None
        };
        let end = frames[i].1;
        match nx {
            Some(nx) if matches!(e.geo, Geo::Line { .. }) && matches!(nx.geo, Geo::Line { .. }) => {
                let (d1, d2) = (e.geo.tangent(1.0), nx.geo.tangent(0.0));
                if k::dot(d1, d2) > 1.0 - 1e-12 {
                    return end;
                }
                if k::dot(d1, d2) < -1.0 + 1e-6 {
                    return end;
                }
                matmul(
                    &miter(e.geo.at(1.0), d1, k::normalize(k::add(d1, d2))),
                    &end,
                )
            }
            _ => end,
        }
    };
    let ends: Vec<Matrix4> = (0..count).map(joint).collect();
    for (i, e) in edges.iter().enumerate() {
        if let Some(nx) = edges.get(i + 1)
            && k::dot(e.geo.tangent(1.0), nx.geo.tangent(0.0)) < -1.0 + 1e-6
        {
            return Err(format!(
                "経路の {} と {} が折り返しています",
                e.name, nx.name
            ));
        }
    }
    // ねじれ・大きさの中心は経路の始まりの点、回す向きは経路の進む向きの右ねじ
    let p0 = edges[0].geo.at(0.0);
    let axis = k::scale(n, k::dot(edges[0].geo.tangent(0.0), n).signum());
    let lens: Vec<f64> = edges.iter().map(|e| e.geo.length()).collect();
    let total: f64 = lens.iter().sum();
    let mut out = Vec::new();
    let mut walked = 0.0;
    for (i, e) in edges.iter().enumerate() {
        let (t0, t1) = (walked / total, (walked + lens[i]) / total);
        walked += lens[i];
        let start = if i == 0 {
            if closed { ends[count - 1] } else { k::IDENTITY }
        } else {
            ends[i - 1]
        };
        let end = ends[i];
        let mut caps = 0;
        if !closed && i == 0 {
            caps |= 1;
        }
        if !closed && i == count - 1 {
            caps |= 2;
        }
        if shaped {
            out.extend(shaped_sides(
                &loops,
                e,
                [&start, &end],
                |t| shape_at(p0, axis, twist, scale, t0 + (t1 - t0) * t),
                twist * (t1 - t0),
                caps,
                tag,
            )?);
            continue;
        }
        if let Geo::Arc {
            c,
            e1,
            e2,
            delta,
            r,
            ..
        } = e.geo
        {
            // 輪郭が円弧の中心の側へはみ出すと、回した形が自分と重なる
            let p0 = e.geo.at(0.0);
            let out_dir = k::normalize(k::sub(p0, c));
            let axis = k::normalize(k::cross(e1, e2));
            for l in &loops {
                for j in 0..64 {
                    let q = k::transform_point(&start, l.at(j as f64 / 64.0));
                    let d = k::sub(q, c);
                    let radial = k::sub(d, k::scale(axis, k::dot(d, axis)));
                    if k::dot(radial, out_dir) < 1e-6 {
                        return Err(format!(
                            "経路の円弧 {}(半径 {r:.3})より輪郭が大きく、形が自分と重なります。円弧を大きくするか輪郭を小さくしてください",
                            e.name
                        ));
                    }
                }
            }
            for l in &loops {
                for pe in &l.edges {
                    let g = pe.geo.clone();
                    let spans = g.spans(0.0, 1.0);
                    let curve: Vec<H> = g
                        .curve(0.0, 1.0, spans)
                        .into_iter()
                        .map(|q| moved(q, &start))
                        .collect();
                    let (knots_u, rows) = revolve_rows(&curve, c, axis, delta);
                    out.push(Surface {
                        degree_u: 2,
                        knots_u,
                        knots_v: arc_knots(spans),
                        rows,
                        caps,
                        name: format!("{tag}:side:{}:{}", pe.name, e.name),
                    });
                }
            }
            continue;
        }
        for l in &loops {
            for pe in &l.edges {
                let g = &pe.geo;
                let spans = g.spans(0.0, 1.0);
                let curve = g.curve(0.0, 1.0, spans);
                let rows = vec![
                    curve.iter().map(|q| moved(*q, &start)).collect(),
                    curve.iter().map(|q| moved(*q, &end)).collect(),
                ];
                out.push(Surface {
                    degree_u: 1,
                    knots_u: vec![0.0, 0.0, 1.0, 1.0],
                    knots_v: arc_knots(spans),
                    rows,
                    caps,
                    name: format!("{tag}:side:{}:{}", pe.name, e.name),
                });
            }
        }
    }
    Ok(out)
}

/// ねじれ・大きさを変えるスイープの、経路の区間 `e` 1 つ分の側面。断面を区間の中に並べ、その間を
/// なめらかにつなぐ。`ends` は区間の始まりと終わりの輪郭の動かし方、`shape(s)` は区間の割合 `s` の所の
/// ねじれと大きさ、`spin` はこの区間で輪郭の回る角度。
fn shaped_sides(
    loops: &[Loop3],
    e: &Edge3,
    [start, end]: [&Matrix4; 2],
    shape: impl Fn(f64) -> Matrix4,
    spin: f64,
    caps: i32,
    tag: &str,
) -> Result<Vec<Surface>, String> {
    let arc = match e.geo {
        Geo::Arc {
            c, e1, e2, delta, ..
        } => Some((c, k::normalize(k::cross(e1, e2)), delta)),
        Geo::Line { .. } => None,
    };
    let at = |s: f64| -> Matrix4 {
        match arc {
            Some((c, ax, delta)) => {
                matmul(&matmul(&k::rotation(c, ax, delta * s), start), &shape(s))
            }
            None => blend(&matmul(start, &shape(s)), &matmul(end, &shape(s)), s),
        }
    };
    // 輪郭の回る角度 `spin` と円弧の曲がりのうち大きい方で、断面の間隔を決める(少なくとも 4 つ)
    let turn = arc.map_or(0.0, |a| a.2.abs());
    let steps = (spin.abs().max(turn) / SHAPED_STEP).ceil().max(3.0) as usize;
    let mats: Vec<Matrix4> = (0..=steps).map(|j| at(j as f64 / steps as f64)).collect();
    if let Some((c, ax, _)) = arc {
        // 輪郭が円弧の中心の側へはみ出すと、回した形が自分と重なる
        for (j, m) in mats.iter().enumerate() {
            let out_dir = k::normalize(k::sub(e.geo.at(j as f64 / steps as f64), c));
            for l in loops {
                for q in 0..64 {
                    let d = k::sub(k::transform_point(m, l.at(q as f64 / 64.0)), c);
                    let radial = k::sub(d, k::scale(ax, k::dot(d, ax)));
                    if k::dot(radial, out_dir) < 1e-6 {
                        return Err(format!(
                            "経路の円弧 {} より輪郭が大きく、形が自分と重なります。円弧を大きくするか輪郭を小さくしてください",
                            e.name
                        ));
                    }
                }
            }
        }
    }
    let params: Vec<f64> = (0..=steps).map(|j| j as f64 / steps as f64).collect();
    let mut out = Vec::new();
    for l in loops {
        for pe in &l.edges {
            let spans = pe.geo.spans(0.0, 1.0);
            let curve = pe.geo.curve(0.0, 1.0, spans);
            let sections: Vec<Vec<H>> = mats
                .iter()
                .map(|m| curve.iter().map(|q| moved(*q, m)).collect())
                .collect();
            let (degree_u, knots_u, rows) = skin_rows(&sections, &params);
            out.push(Surface {
                degree_u,
                knots_u,
                knots_v: arc_knots(spans),
                rows,
                caps,
                name: format!("{tag}:side:{}:{}", pe.name, e.name),
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(s: &Surface, u: f64, v: f64) -> P3 {
        let c = iso(s.degree_u, &s.knots_u, &s.rows, u);
        let rows: Vec<Vec<H>> = c.iter().map(|q| vec![*q]).collect();
        euclid(iso(2, &s.knots_v, &rows, v)[0])
    }

    #[test]
    fn a_quarter_arc_curve_stays_on_the_circle() {
        let g = Geo::Arc {
            c: [0.0; 3],
            r: 2.0,
            e1: [1.0, 0.0, 0.0],
            e2: [0.0, 1.0, 0.0],
            phi0: 0.0,
            delta: FRAC_PI_2 * 3.0,
        };
        let n = g.spans(0.0, 1.0);
        assert_eq!(n, 3);
        let s = Surface {
            degree_u: 1,
            knots_u: vec![0.0, 0.0, 1.0, 1.0],
            knots_v: arc_knots(n),
            rows: vec![g.curve(0.0, 1.0, n), g.curve(0.0, 1.0, n)],
            caps: 0,
            name: String::new(),
        };
        for i in 0..=20 {
            let p = eval(&s, 0.5, i as f64 / 20.0);
            assert!((k::dot(p, p).sqrt() - 2.0).abs() < 1e-12, "{p:?}");
        }
    }
}
