//! 方向の格子を替える(`regrid`)と、サンプリング周波数を替える(`resample`)。
//!
//! 格子の替え方は 3 つ。
//! - 最近傍(`nearest`): 一番近い方向の IR をそのまま使う。値を混ぜない。
//! - 球面の三角形(`triangle`): 行き先の方向を囲む 3 つの方向の IR と遅れを、球面の三角形の重心の
//!   重みで混ぜる。囲む 3 点は近い 8 点の組み合わせから、行き先を含み一番小さい三角形を選ぶ。近い 8 点が
//!   1 つの輪に並んで囲めない時(方位が細かく仰角のまばらな格子)は、行き先のまわりを 8 つの向きに分けて
//!   向きごとの一番近い点を足し、選び直す。
//! - 輪の双線形(`bilinear`): 元の格子を仰角ごとの輪に分け、行き先の上下の輪で方位の左右の隣 2 点を
//!   方位の比で混ぜ、上下の輪を仰角の比で混ぜる。一番上(下)の輪より外は、極の方向が無ければ極を輪の
//!   平均で埋めて、輪と極の間を仰角の比で混ぜる。`pole` を `ring_mean` にすると、極の方向があっても
//!   使わず、極を一番上(下)の輪の平均で埋める。
//!
//! 混ぜる替え方は、混ぜる前に立ち上がりを外してある(輪の枠)ことを求める。時間差を含んだまま混ぜると、
//! ずれた 2 つの音が重なって櫛の形に欠けるため。
//!
//! 再標本化は Kaiser の窓をかけた sinc の補間(帯域は低い方のナイキストの `cutoff` 倍まで)。

use serde::{Deserialize, Serialize};

use crate::{Dir, Frame, Grid, HrirSet, HrtfError};

/// 行き先の方向の格子。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Target {
    /// 仰角が Gauss–Legendre の点 `el` 個、方位が等間隔 `az` 個。
    GaussLegendre { el: usize, az: usize },
    /// 輪の格子(`crate::grid::Rings`)。
    Rings { step: f64 },
    /// 別の集まりと同じ方向の並び。
    Like { set: String },
}

/// 格子の替え方。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    Nearest,
    #[default]
    Triangle,
    /// 輪の双線形。仰角ごとに方位が 3 つ以上並ぶ格子だけに使う。
    Bilinear,
}

/// 輪の双線形での極の値の取り方。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pole {
    /// 極の方向があればその値、無ければ一番上(下)の輪の平均。
    #[default]
    Measured,
    /// 極の方向があっても使わず、一番上(下)の輪の平均にする。
    RingMean,
}

/// Gauss–Legendre の格子の方向(方位、仰角。度)。仰角は下から、輪の中は方位の順。
pub fn gauss_legendre(n_el: usize, n_az: usize) -> Result<Vec<(f64, f64)>, HrtfError> {
    if n_el == 0 || n_az == 0 || n_el > 2000 || n_az > 4000 {
        return Err(HrtfError::input(format!(
            "Gauss–Legendre の格子の数(仰角 {n_el}・方位 {n_az})は仰角 1〜2000・方位 1〜4000 にする"
        )));
    }
    let mut out = Vec::with_capacity(n_el * n_az);
    for x in legendre_roots(n_el) {
        let el = x.asin().to_degrees();
        for j in 0..n_az {
            out.push((j as f64 * 360.0 / n_az as f64, el));
        }
    }
    Ok(out)
}

/// n 次の Legendre 多項式の根(小さい順)。Newton 法で求める。
fn legendre_roots(n: usize) -> Vec<f64> {
    let mut xs: Vec<f64> = (0..n)
        .map(|i| {
            let mut x = -(std::f64::consts::PI * (i as f64 + 0.75) / (n as f64 + 0.5)).cos();
            for _ in 0..100 {
                let (p, dp) = legendre(n, x);
                let dx = p / dp;
                x -= dx;
                if dx.abs() < 1e-15 {
                    break;
                }
            }
            x
        })
        .collect();
    xs.sort_by(f64::total_cmp);
    xs
}

/// P_n(x) と P_n'(x)。
fn legendre(n: usize, x: f64) -> (f64, f64) {
    let (mut p0, mut p1) = (1.0, x);
    if n == 0 {
        return (1.0, 0.0);
    }
    for k in 2..=n {
        let k = k as f64;
        let p2 = ((2.0 * k - 1.0) * x * p1 - (k - 1.0) * p0) / k;
        p0 = p1;
        p1 = p2;
    }
    let dp = n as f64 * (x * p1 - p0) / (x * x - 1.0);
    (p1, dp)
}

fn unit(az: f64, el: f64) -> [f64; 3] {
    let (a, e) = (az.to_radians(), el.to_radians());
    [e.cos() * a.cos(), e.cos() * a.sin(), e.sin()]
}

fn dist2(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum()
}

fn cross(a: &[f64; 3], b: &[f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// 単位の球の上の点の k-d 木(近い点を探すだけの小さな物)。
struct Tree {
    pts: Vec<[f64; 3]>,
    idx: Vec<usize>,
}

impl Tree {
    fn new(pts: Vec<[f64; 3]>) -> Tree {
        let mut idx: Vec<usize> = (0..pts.len()).collect();
        Self::build(&pts, &mut idx, 0);
        Tree { pts, idx }
    }

    fn build(pts: &[[f64; 3]], idx: &mut [usize], depth: usize) {
        if idx.len() <= 1 {
            return;
        }
        let axis = depth % 3;
        let mid = idx.len() / 2;
        idx.select_nth_unstable_by(mid, |&a, &b| pts[a][axis].total_cmp(&pts[b][axis]));
        let (lo, hi) = idx.split_at_mut(mid);
        Self::build(pts, lo, depth + 1);
        Self::build(pts, &mut hi[1..], depth + 1);
    }

    /// `q` に近い順の k 個(番号と距離の 2 乗)。
    fn nearest(&self, q: &[f64; 3], k: usize) -> Vec<(usize, f64)> {
        let mut best: Vec<(usize, f64)> = Vec::with_capacity(k + 1);
        self.search(&self.idx, q, k, 0, &mut best);
        best
    }

    /// `q` のまわりを接平面で `n` 個の向きに分け、向きごとに一番近い点(距離の 2 乗が `max_d2` 未満)。
    fn sectors(&self, q: &[f64; 3], n: usize, max_d2: f64) -> Vec<(usize, f64)> {
        let e1 = {
            // 極の近くでは z 軸との外積が小さいので x 軸を使う。
            let a = if q[2].abs() < 0.9 {
                [0.0, 0.0, 1.0]
            } else {
                [1.0, 0.0, 0.0]
            };
            let c = cross(&a, q);
            let l = dist2(&c, &[0.0; 3]).sqrt();
            c.map(|x| x / l)
        };
        let e2 = cross(q, &e1);
        let mut best: Vec<Option<(usize, f64)>> = vec![None; n];
        self.search_sectors(&self.idx, q, (&e1, &e2), max_d2, 0, &mut best);
        best.into_iter().flatten().collect()
    }

    fn search_sectors(
        &self,
        idx: &[usize],
        q: &[f64; 3],
        basis: (&[f64; 3], &[f64; 3]),
        max_d2: f64,
        depth: usize,
        best: &mut [Option<(usize, f64)>],
    ) {
        if idx.is_empty() {
            return;
        }
        let mid = idx.len() / 2;
        let i = idx[mid];
        let p = &self.pts[i];
        let d = dist2(p, q);
        if d > 1e-18 && d < max_d2 {
            let x: f64 = (0..3).map(|k| p[k] * basis.0[k]).sum();
            let y: f64 = (0..3).map(|k| p[k] * basis.1[k]).sum();
            let n = best.len();
            let a = y.atan2(x) + std::f64::consts::PI;
            let s = ((a / std::f64::consts::TAU * n as f64) as usize).min(n - 1);
            if best[s].is_none_or(|b| d < b.1) {
                best[s] = Some((i, d));
            }
        }
        // 一番遠い向きの答え(まだ無い向きは `max_d2`)より平面が遠ければ、向こう側は見ない。
        let worst = best
            .iter()
            .map(|b| b.map_or(max_d2, |b| b.1))
            .fold(0.0, f64::max);
        let axis = depth % 3;
        let diff = q[axis] - p[axis];
        let (near, far) = if diff < 0.0 {
            (&idx[..mid], &idx[mid + 1..])
        } else {
            (&idx[mid + 1..], &idx[..mid])
        };
        self.search_sectors(near, q, basis, max_d2, depth + 1, best);
        if diff * diff < worst {
            self.search_sectors(far, q, basis, max_d2, depth + 1, best);
        }
    }

    fn search(
        &self,
        idx: &[usize],
        q: &[f64; 3],
        k: usize,
        depth: usize,
        best: &mut Vec<(usize, f64)>,
    ) {
        if idx.is_empty() {
            return;
        }
        let mid = idx.len() / 2;
        let i = idx[mid];
        let d = dist2(&self.pts[i], q);
        if best.len() < k || d < best[best.len() - 1].1 {
            let at = best.partition_point(|b| b.1 <= d);
            best.insert(at, (i, d));
            best.truncate(k);
        }
        let axis = depth % 3;
        let diff = q[axis] - self.pts[i][axis];
        let (near, far) = if diff < 0.0 {
            (&idx[..mid], &idx[mid + 1..])
        } else {
            (&idx[mid + 1..], &idx[..mid])
        };
        self.search(near, q, k, depth + 1, best);
        if best.len() < k || diff * diff < best[best.len() - 1].1 {
            self.search(far, q, k, depth + 1, best);
        }
    }
}

/// 3×3 の連立(列が a・b・c)を解く。行列式が小さすぎれば `None`。
fn solve3(a: &[f64; 3], b: &[f64; 3], c: &[f64; 3], t: &[f64; 3]) -> Option<[f64; 3]> {
    let det = |x: &[f64; 3], y: &[f64; 3], z: &[f64; 3]| {
        x[0] * (y[1] * z[2] - y[2] * z[1]) - y[0] * (x[1] * z[2] - x[2] * z[1])
            + z[0] * (x[1] * y[2] - x[2] * y[1])
    };
    let d = det(a, b, c);
    if d.abs() < 1e-14 {
        return None;
    }
    Some([det(t, b, c) / d, det(a, t, c) / d, det(a, b, t) / d])
}

/// `cand` の点から `q` を含む一番小さい三角形(番号と重心の重み)。
fn smallest_triangle(tree: &Tree, cand: &[usize], q: &[f64; 3]) -> Option<([usize; 3], [f64; 3])> {
    let mut best: Option<(f64, [usize; 3], [f64; 3])> = None;
    for i in 0..cand.len() {
        for j in i + 1..cand.len() {
            for k in j + 1..cand.len() {
                let ids = [cand[i], cand[j], cand[k]];
                let p = ids.map(|n| tree.pts[n]);
                let Some(b) = solve3(&p[0], &p[1], &p[2], q) else {
                    continue;
                };
                if b.iter().any(|&x| x < -1e-9) {
                    continue;
                }
                let size = dist2(&p[0], &p[1])
                    .max(dist2(&p[1], &p[2]))
                    .max(dist2(&p[0], &p[2]));
                if best.as_ref().is_none_or(|bb| size < bb.0 - 1e-15) {
                    best = Some((size, ids, b));
                }
            }
        }
    }
    best.map(|(_, ids, b)| (ids, b))
}

/// 行き先の方向ごとの(元の方向の番号, 重み)の並びと、囲む三角形が見つからず最近傍にした数。
fn weights(from: &[Dir], to: &[Dir], method: Method) -> (Weights, usize) {
    let tree = Tree::new(from.iter().map(|d| unit(d.az, d.el)).collect());
    let mut fallback = 0;
    let w = to
        .iter()
        .map(|d| {
            let q = unit(d.az, d.el);
            let near = tree.nearest(&q, if method == Method::Nearest { 1 } else { 8 });
            if method == Method::Nearest || near[0].1 < 1e-18 || near.len() < 3 {
                return vec![(near[0].0, 1.0)];
            }
            let mut cand: Vec<usize> = near.iter().map(|b| b.0).collect();
            let mut found = smallest_triangle(&tree, &cand, &q);
            if found.is_none() {
                // 近い点が 1 つの輪に並ぶ時。向きごとの一番近い点(90° より内)を足して選び直す。
                for (n, _) in tree.sectors(&q, 8, 2.0) {
                    if !cand.contains(&n) {
                        cand.push(n);
                    }
                }
                found = smallest_triangle(&tree, &cand, &q);
            }
            match found {
                Some((ids, b)) => {
                    let s: f64 = b.iter().sum();
                    ids.iter()
                        .zip(b)
                        .map(|(&n, x)| (n, x.max(0.0) / s))
                        .collect()
                }
                None => {
                    fallback += 1;
                    vec![(near[0].0, 1.0)]
                }
            }
        })
        .collect();
    (w, fallback)
}

/// 仰角の輪 1 本。方位(0 以上 360 未満)の順の(方位, 元の番号)。
struct Ring {
    el: f64,
    pts: Vec<(f64, usize)>,
}

impl Ring {
    fn is_pole(&self) -> bool {
        self.el.abs() > 90.0 - 1e-6
    }

    /// 方位 `az` を左右の隣 2 点の比で混ぜる重み。
    fn at(&self, az: f64, scale: f64, out: &mut Vec<(usize, f64)>) {
        let n = self.pts.len();
        if n == 1 || self.is_pole() {
            // 極は方位を持たないので、輪の平均にする(極の点が 1 つなら、その点)。
            for &(_, i) in &self.pts {
                out.push((i, scale / n as f64));
            }
            return;
        }
        let az = az.rem_euclid(360.0);
        let j = self.pts.partition_point(|p| p.0 <= az);
        let (lo, hi) = (self.pts[(j + n - 1) % n], self.pts[j % n]);
        let gap = (hi.0 - lo.0).rem_euclid(360.0);
        let t = if gap > 0.0 {
            (az - lo.0).rem_euclid(360.0) / gap
        } else {
            0.0
        };
        if t < 1.0 {
            out.push((lo.1, scale * (1.0 - t)));
        }
        if t > 0.0 {
            out.push((hi.1, scale * t));
        }
    }

    /// 輪の平均(極を埋める値)の重み。
    fn mean(&self, scale: f64, out: &mut Vec<(usize, f64)>) {
        for &(_, i) in &self.pts {
            out.push((i, scale / self.pts.len() as f64));
        }
    }
}

/// 元の方向を仰角ごとの輪に分ける。極でない輪に方位が 3 つ未満なら輪の格子でないとして誤り。
fn rings_of(from: &[Dir]) -> Result<Vec<Ring>, HrtfError> {
    let mut order: Vec<usize> = (0..from.len()).collect();
    order.sort_by(|&a, &b| from[a].el.total_cmp(&from[b].el));
    let mut rings: Vec<Ring> = Vec::new();
    for i in order {
        let el = from[i].el;
        match rings.last_mut() {
            Some(r) if (el - r.el).abs() < 1e-6 => r.pts.push((from[i].az.rem_euclid(360.0), i)),
            _ => rings.push(Ring {
                el,
                pts: vec![(from[i].az.rem_euclid(360.0), i)],
            }),
        }
    }
    for r in &mut rings {
        r.pts.sort_by(|a, b| a.0.total_cmp(&b.0));
        if !r.is_pole() && r.pts.len() < 3 {
            return Err(HrtfError::input(format!(
                "元の格子が輪になっていない(仰角 {}° の方向が {} 個しかない)。輪の双線形は仰角ごとに方位が 3 つ以上並ぶ格子に使う(ほかは method を triangle に)",
                r.el,
                r.pts.len()
            )));
        }
    }
    Ok(rings)
}

/// 行き先の方向ごとの(元の方向の番号, 重み)の並び。
type Weights = Vec<Vec<(usize, f64)>>;

/// 輪の双線形の重みと、輪の平均で埋めた極の数。
fn bilinear_weights(from: &[Dir], to: &[Dir], pole: Pole) -> Result<(Weights, usize), HrtfError> {
    let mut rings = rings_of(from)?;
    if pole == Pole::RingMean {
        rings.retain(|r| !r.is_pole());
        if rings.is_empty() {
            return Err(HrtfError::input(
                "極のほかに輪が無いので、極を輪の平均で埋められない",
            ));
        }
    }
    let (first, last) = (&rings[0], &rings[rings.len() - 1]);
    let filled = usize::from(!first.is_pole()) + usize::from(!last.is_pole());
    let w = to
        .iter()
        .map(|d| {
            let mut out = Vec::new();
            let el = d.el.clamp(-90.0, 90.0);
            if el >= last.el {
                // 一番上の輪から極まで。極の方向が無ければ輪の平均で埋める。
                let t = if last.is_pole() {
                    0.0
                } else {
                    (el - last.el) / (90.0 - last.el)
                };
                if t < 1.0 {
                    last.at(d.az, 1.0 - t, &mut out);
                }
                if t > 0.0 {
                    last.mean(t, &mut out);
                }
            } else if el <= first.el {
                let t = if first.is_pole() {
                    0.0
                } else {
                    (first.el - el) / (first.el + 90.0)
                };
                if t < 1.0 {
                    first.at(d.az, 1.0 - t, &mut out);
                }
                if t > 0.0 {
                    first.mean(t, &mut out);
                }
            } else {
                let k = rings.partition_point(|r| r.el <= el);
                let (lo, hi) = (&rings[k - 1], &rings[k]);
                let t = (el - lo.el) / (hi.el - lo.el);
                if t < 1.0 {
                    lo.at(d.az, 1.0 - t, &mut out);
                }
                if t > 0.0 {
                    hi.at(d.az, t, &mut out);
                }
            }
            out
        })
        .collect();
    Ok((w, filled))
}

/// 方向の並び `from` から `to` へ混ぜる重みと、記録に書く数(三角形は最近傍にした方向、双線形は
/// 輪の平均で埋めた極)。`pole` は双線形の時だけ効く。
pub(crate) fn mix_weights(
    from: &[Dir],
    to: &[Dir],
    method: Method,
    pole: Pole,
) -> Result<(Weights, usize), HrtfError> {
    if pole != Pole::Measured && method != Method::Bilinear {
        return Err(HrtfError::input(
            "極の取り方(pole)は輪の双線形(method を bilinear)の時だけ選べる",
        ));
    }
    match method {
        Method::Bilinear => bilinear_weights(from, to, pole),
        _ => Ok(weights(from, to, method)),
    }
}

/// 方向の並び `to` に替えた集まり。
pub fn regrid(
    s: &HrirSet,
    to: &[Dir],
    grid: Grid,
    method: Method,
    pole: Pole,
) -> Result<HrirSet, HrtfError> {
    s.check()?;
    if to.is_empty() {
        return Err(HrtfError::input("行き先の方向が 1 つも無い"));
    }
    if method != Method::Nearest && s.frame != Frame::Circular {
        return Err(HrtfError::input(
            "三角形や双線形で混ぜる前に立ち上がりを外す(先に onset の段を流す。値を混ぜない時は method を nearest に)",
        ));
    }
    let (w, fallback) = mix_weights(&s.dirs, to, method, pole)?;
    let taps = s.taps();
    let mut out = s.clone();
    out.dirs = Vec::with_capacity(to.len());
    out.ir = Vec::with_capacity(to.len());
    out.delay = Vec::with_capacity(to.len());
    let mut onset = s.onset.as_ref().map(|_| Vec::with_capacity(to.len()));
    for (d, wl) in to.iter().zip(&w) {
        out.dirs.push(Dir {
            az: d.az,
            el: d.el,
            dist: s.dirs[wl[0].0].dist,
        });
        let mut pair = [vec![0.0; taps], vec![0.0; taps]];
        let mut dl = [0.0; 2];
        let mut on = [0.0; 2];
        for &(n, x) in wl {
            for ear in 0..2 {
                for (o, v) in pair[ear].iter_mut().zip(&s.ir[n][ear]) {
                    *o += x * v;
                }
                dl[ear] += x * s.delay[n][ear];
                if let Some(so) = &s.onset {
                    on[ear] += x * so[n][ear];
                }
            }
        }
        out.ir.push(pair);
        out.delay.push(dl);
        if let Some(o) = onset.as_mut() {
            o.push(on);
        }
    }
    out.onset = onset;
    out.grid = grid;
    let how = match method {
        Method::Nearest => "最近傍".to_string(),
        Method::Triangle if fallback > 0 => {
            format!("球面の三角形(囲む三角形が無い {fallback} 方向は最近傍)")
        }
        Method::Triangle => "球面の三角形".to_string(),
        Method::Bilinear if fallback > 0 => {
            let measured = if pole == Pole::RingMean {
                "。測った極の方向は使わない"
            } else {
                ""
            };
            format!("輪の双線形(極 {fallback} つを輪の平均で埋めた{measured})")
        }
        Method::Bilinear => "輪の双線形".to_string(),
    };
    out.history.push(format!(
        "regrid: 方向 {} 個を {} 個に替えた({how})",
        s.dirs.len(),
        to.len()
    ));
    Ok(out)
}

/// 0 次の第 1 種変形 Bessel 関数(級数)。
fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term) = (1.0, 1.0);
    let q = x * x / 4.0;
    for k in 1..200 {
        term *= q / (k * k) as f64;
        sum += term;
        if term < sum * 1e-17 {
            break;
        }
    }
    sum
}

/// サンプリング周波数を `fs` に替えた集まり。遅れと立ち上がりも同じ比で伸び縮みする。
pub fn resample(s: &HrirSet, fs: f64, cutoff: f64) -> Result<HrirSet, HrtfError> {
    s.check()?;
    if !(fs.is_finite() && fs > 0.0) {
        return Err(HrtfError::input(format!(
            "サンプリング周波数 {fs} Hz が正の数でない"
        )));
    }
    if !(cutoff > 0.0 && cutoff <= 1.0) {
        return Err(HrtfError::input(format!(
            "cutoff {cutoff} は 0 より大きく 1 以下にする"
        )));
    }
    if s.frame != Frame::Linear {
        return Err(HrtfError::input(
            "輪の枠の IR は再標本化できない(onset の段より前に resample を置く)",
        ));
    }
    let r = fs / s.fs;
    let mut out = s.clone();
    if (r - 1.0).abs() < 1e-12 {
        out.history.push(format!("resample: {fs} Hz のまま"));
        return Ok(out);
    }
    // 入力の sample を単位にした窓つき sinc。帯域は低い方のナイキストの cutoff 倍。
    let c = r.min(1.0) * cutoff;
    let half = 24.0 / c;
    let beta = 8.6;
    let i0b = bessel_i0(beta);
    let kernel = |tau: f64| -> f64 {
        if tau.abs() >= half {
            return 0.0;
        }
        let x = std::f64::consts::PI * c * tau;
        let sinc = if x.abs() < 1e-12 { 1.0 } else { x.sin() / x };
        let u = tau / half;
        c * sinc * bessel_i0(beta * (1.0 - u * u).sqrt()) / i0b
    };
    let len = (s.taps() as f64 * r).ceil() as usize;
    for (pair, src) in out.ir.iter_mut().zip(&s.ir) {
        for ear in 0..2 {
            let x = &src[ear];
            pair[ear] = (0..len)
                .map(|n| {
                    let t = n as f64 / r;
                    let lo = (t - half).ceil().max(0.0) as usize;
                    let hi = ((t + half).floor() as usize).min(x.len() - 1);
                    (lo..=hi).map(|k| x[k] * kernel(t - k as f64)).sum()
                })
                .collect();
        }
    }
    for d in out.delay.iter_mut().flatten() {
        *d *= r;
    }
    if let Some(o) = out.onset.as_mut() {
        for v in o.iter_mut().flatten() {
            *v *= r;
        }
    }
    out.fs = fs;
    out.history
        .push(format!("resample: {} Hz を {fs} Hz にした", s.fs));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 確かめること: Gauss–Legendre の根は Legendre 多項式を 0 にし、仰角 89 × 方位 180 の格子は
    /// 16020 方向で上下に対称。
    #[test]
    fn gauss_legendre_grid() {
        for x in legendre_roots(7) {
            assert!(legendre(7, x).0.abs() < 1e-12);
        }
        let g = gauss_legendre(89, 180).unwrap();
        assert_eq!(g.len(), 16020);
        assert!(g[44 * 180].1.abs() < 1e-12, "中の輪は仰角 0");
        assert!((g[0].1 + g[g.len() - 1].1).abs() < 1e-9);
        assert!(gauss_legendre(0, 10).is_err());
    }

    /// 確かめること: k-d 木の近い点は全部を調べた答えと同じ。
    #[test]
    fn tree_matches_brute_force() {
        let pts: Vec<[f64; 3]> = (0..500)
            .map(|i| unit((i * 137 % 360) as f64 + 0.3, ((i * 53 % 179) as f64) - 89.0))
            .collect();
        let tree = Tree::new(pts.clone());
        for i in 0..50 {
            let q = unit(i as f64 * 7.1, (i as f64 * 3.3) % 180.0 - 90.0);
            let got: Vec<usize> = tree.nearest(&q, 4).iter().map(|b| b.0).collect();
            let mut all: Vec<(usize, f64)> = pts
                .iter()
                .enumerate()
                .map(|(n, p)| (n, dist2(p, &q)))
                .collect();
            all.sort_by(|a, b| a.1.total_cmp(&b.1));
            let want: Vec<usize> = all[..4].iter().map(|b| b.0).collect();
            assert_eq!(got, want);
        }
    }

    /// 確かめること: 三角形の重みは足して 1、元の方向の上では元の値 1 つ、線形の値は正しく補える。
    #[test]
    fn triangle_weights() {
        let from: Vec<Dir> = gauss_legendre(9, 18)
            .unwrap()
            .into_iter()
            .map(|(az, el)| Dir { az, el, dist: 1.0 })
            .collect();
        let to: Vec<Dir> = (0..40)
            .map(|i| Dir {
                az: i as f64 * 9.7,
                el: (i as f64 * 4.1) % 160.0 - 80.0,
                dist: 1.0,
            })
            .chain([from[20]])
            .collect();
        let (w, fallback) = weights(&from, &to, Method::Triangle);
        assert_eq!(fallback, 0);
        for (t, wl) in to.iter().zip(&w) {
            let s: f64 = wl.iter().map(|x| x.1).sum();
            assert!((s - 1.0).abs() < 1e-12);
            // 重心の重みで混ぜた単位の方向は、行き先の方向を向く。
            let mut v = [0.0; 3];
            for &(n, x) in wl {
                let p = unit(from[n].az, from[n].el);
                for i in 0..3 {
                    v[i] += x * p[i];
                }
            }
            let q = unit(t.az, t.el);
            let norm = dist2(&v, &[0.0; 3]).sqrt();
            let cos: f64 = (0..3).map(|i| v[i] * q[i]).sum::<f64>() / norm;
            assert!(cos > 1.0 - 1e-9);
        }
        assert_eq!(w[40], vec![(20, 1.0)]);
    }

    /// 方位 1° の輪が仰角のまばらな 5 本(−45°・−15°・0°・15°・45°)だけ並び、極の方向の無い格子。
    fn sparse_rings() -> Vec<Dir> {
        [-45.0, -15.0, 0.0, 15.0, 45.0]
            .into_iter()
            .flat_map(|el| {
                (0..360).map(move |a| Dir {
                    az: a as f64,
                    el,
                    dist: 1.0,
                })
            })
            .collect()
    }

    /// 確かめること: 仰角のまばらな格子でも、輪の間の方向は上下の輪にまたがる三角形で囲まれ(最近傍に
    /// 落ちない)、混ぜた方向は行き先を向く。
    #[test]
    fn triangle_on_sparse_rings() {
        let from = sparse_rings();
        let to: Vec<Dir> = crate::grid::Rings::new(2.0)
            .unwrap()
            .dirs()
            .into_iter()
            .filter(|&(_, el)| el.abs() <= 45.0)
            .map(|(az, el)| Dir { az, el, dist: 1.0 })
            .collect();
        let (w, fallback) = weights(&from, &to, Method::Triangle);
        assert_eq!(fallback, 0);
        for (t, wl) in to.iter().zip(&w) {
            let mut v = [0.0; 3];
            for &(n, x) in wl {
                let p = unit(from[n].az, from[n].el);
                for i in 0..3 {
                    v[i] += x * p[i];
                }
                // 囲む点は行き先の上下の輪(と同じ輪)のもの。
                let gap = match t.el.abs() {
                    e if e <= 15.0 => 15.0,
                    _ => 30.0,
                };
                assert!(
                    (from[n].el - t.el).abs() <= gap + 1e-9,
                    "{t:?} {:?}",
                    from[n]
                );
            }
            let q = unit(t.az, t.el);
            let norm = dist2(&v, &[0.0; 3]).sqrt();
            let cos: f64 = (0..3).map(|i| v[i] * q[i]).sum::<f64>() / norm;
            assert!(cos > 1.0 - 1e-9, "{t:?}");
        }
    }

    /// 確かめること: 輪の双線形は、仰角に線形で輪の上の隣どうしの間で方位に線形な値をそのまま補い、
    /// 輪の上の方向では元の値 1 つになる。一番上の輪より上は極を輪の平均で埋めて仰角の比で混ぜ、極では
    /// 輪の平均。輪になっていない格子は誤り。手順の JSON の `bilinear` で選べる。
    #[test]
    fn bilinear_on_sparse_rings() {
        let from = sparse_rings();
        let f = |az: f64, el: f64| 3.0 * el + az;
        let mean_top = (0..360).map(|a| f(a as f64, 45.0)).sum::<f64>() / 360.0;
        let to = [
            (10.25, 7.5),
            (100.0, -30.0),
            (358.5, 0.0),
            (20.0, 15.0),
            (200.5, 67.5),
            (5.0, 90.0),
            (77.0, -90.0),
        ]
        .map(|(az, el)| Dir { az, el, dist: 1.0 });
        let (w, filled) = bilinear_weights(&from, &to, Pole::Measured).unwrap();
        assert_eq!(filled, 2);
        let val = |wl: &[(usize, f64)]| -> f64 {
            wl.iter().map(|&(n, x)| x * f(from[n].az, from[n].el)).sum()
        };
        for wl in &w {
            let s: f64 = wl.iter().map(|x| x.1).sum();
            assert!((s - 1.0).abs() < 1e-12);
        }
        for i in 0..4 {
            assert!(
                (val(&w[i]) - f(to[i].az, to[i].el)).abs() < 1e-9,
                "{:?}",
                to[i]
            );
        }
        assert_eq!(w[3], vec![(4 * 360 - 360 + 20, 1.0)]);
        let half = 0.5 * f(200.5, 45.0) + 0.5 * mean_top;
        assert!((val(&w[4]) - half).abs() < 1e-9);
        assert!((val(&w[5]) - mean_top).abs() < 1e-9);
        assert_eq!(w[6].len(), 360);
        let e = bilinear_weights(
            &[(0.0, 0.0), (90.0, 0.0), (45.0, 10.0)].map(|(az, el)| Dir { az, el, dist: 1.0 }),
            &to,
            Pole::Measured,
        )
        .unwrap_err();
        assert!(e.message.contains("輪になっていない"), "{}", e.message);
        let m: Method = serde_json::from_str(r#""bilinear""#).unwrap();
        assert_eq!(m, Method::Bilinear);
    }

    /// 確かめること: 天頂の方向がある格子では、既定は天頂の値を使い、`ring_mean` では天頂を使わず
    /// 一番上の輪の平均で埋める。`ring_mean` は双線形の時だけ選べ、手順の JSON の名前で選べる。
    #[test]
    fn bilinear_pole_choice() {
        let mut from = sparse_rings();
        from.push(Dir {
            az: 0.0,
            el: 90.0,
            dist: 1.0,
        });
        let zenith = from.len() - 1;
        let to = [(30.0, 90.0), (30.0, 67.5)].map(|(az, el)| Dir { az, el, dist: 1.0 });
        let (w, filled) = bilinear_weights(&from, &to, Pole::Measured).unwrap();
        assert_eq!(filled, 1, "下の極だけ埋める");
        assert_eq!(w[0], vec![(zenith, 1.0)]);
        assert!(w[1].contains(&(zenith, 0.5)));
        let (w, filled) = bilinear_weights(&from, &to, Pole::RingMean).unwrap();
        assert_eq!(filled, 2);
        assert_eq!(w[0].len(), 360);
        assert!(w.iter().flatten().all(|&(n, _)| n != zenith));
        assert!(
            w[0].iter()
                .all(|&(n, x)| from[n].el == 45.0 && (x - 1.0 / 360.0).abs() < 1e-15)
        );
        let e = mix_weights(&from, &to, Method::Triangle, Pole::RingMean).unwrap_err();
        assert!(e.message.contains("bilinear"), "{}", e.message);
        let p: Pole = serde_json::from_str(r#""ring_mean""#).unwrap();
        assert_eq!(p, Pole::RingMean);
    }

    /// 確かめること: 帯域の中の正弦波は 44.1 → 48 kHz で同じ周波数の正弦波になり、遅れは比で伸びる。
    #[test]
    fn resample_keeps_a_sine() {
        let f = 1000.0;
        let n = 2048;
        let wave: Vec<f64> = (0..n)
            .map(|k| (2.0 * std::f64::consts::PI * f * k as f64 / 44100.0).sin())
            .collect();
        let mut s = HrirSet {
            fs: 44100.0,
            dirs: vec![Dir {
                az: 0.0,
                el: 0.0,
                dist: 1.0,
            }],
            ir: vec![[wave.clone(), wave]],
            delay: vec![[0.0, 0.0]],
            onset: None,
            frame: Frame::Linear,
            grid: Grid::Any,
            meta: Default::default(),
            history: vec![],
        };
        s.delay[0] = [10.0, 20.0];
        let r = resample(&s, 48000.0, 0.95).unwrap();
        assert_eq!(r.taps(), (n as f64 * 48000.0 / 44100.0).ceil() as usize);
        assert!((r.delay[0][0] - 10.0 * 48000.0 / 44100.0).abs() < 1e-12);
        // 端の窓の影響を除いた中ほどで比べる。
        for k in 200..1800 {
            let want = (2.0 * std::f64::consts::PI * f * k as f64 / 48000.0).sin();
            assert!((r.ir[0][0][k] - want).abs() < 1e-3, "{k}");
        }
        assert!(resample(&s, -1.0, 0.95).is_err());
    }
}
