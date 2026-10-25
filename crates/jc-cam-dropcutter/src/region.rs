//! 格子の節点の印(真偽)から輪と領域を作る道具。3D の加工の範囲(シルエット、選んだ面)、
//! 3D の荒削りの段ごとの削る所、取残しの所などに使う。
//!
//! - [`trace`] は、印の境目の輪(マーチングスクエア)。印の付いた側を進む向きの左に見る
//! - [`areas_from_loops`] は、向きの付いた輪を、外周と内周の領域([`Area`])に組む
//! - [`Region`] は、領域に点が入るかを速く調べ、格子の印にする
//! - [`dilate`] は、印を半径だけ広げる(ユークリッド距離の変換)

use jc_cam_geom::{Area, Loop, P2, point_in_polygon, polygon_area as signed_area, seg_dist};
use std::collections::HashMap;

/// 格子の形(節点 (i, j) の位置は `origin + (i, j) × cell`)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    pub origin: [f64; 2],
    pub cell: f64,
    pub nx: usize,
    pub ny: usize,
}

impl Grid {
    pub fn at(&self, i: usize, j: usize) -> P2 {
        [
            self.origin[0] + i as f64 * self.cell,
            self.origin[1] + j as f64 * self.cell,
        ]
    }
}

/// 印 `inside(i, j)` の境目の輪。印の付いた側を進む向きの左に見るので、印の付いた所の外周は
/// 反時計回り、その中の印のない所の周りは時計回りになる。`center(i, j)` は、セル (i, j) が
/// 鞍点(対角の 2 隅だけに印)の時に真ん中に印があるとみなすか。`crossing(a, b)` は印の付いた
/// 節点 `a` と付いていない節点 `b` の間の境目の点。周の長さが `min_length` より短い輪は捨てる。
pub(crate) fn trace(
    nx: usize,
    ny: usize,
    inside: &dyn Fn(usize, usize) -> bool,
    center: &dyn Fn(usize, usize) -> bool,
    crossing: &mut dyn FnMut((usize, usize), (usize, usize)) -> P2,
    min_length: f64,
) -> Vec<Vec<P2>> {
    if nx < 2 || ny < 2 {
        return Vec::new();
    }
    let blocked = |i: usize, j: usize| !inside(i, j);
    // 辺の番号: 横の辺 (i, j)-(i+1, j) は 2(j nx + i)、縦の辺 (i, j)-(i, j+1) は +1
    let horizontal = |i: usize, j: usize| 2 * (j * nx + i);
    let vertical = |i: usize, j: usize| 2 * (j * nx + i) + 1;
    let mut next: HashMap<usize, usize> = Default::default();
    let mut points: HashMap<usize, P2> = Default::default();
    for j in 0..ny - 1 {
        for i in 0..nx - 1 {
            // 反時計回りに 4 隅と 4 辺(辺 k は隅 k から隅 k+1 へ)
            let corners = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)];
            let out = corners.map(|(a, b)| blocked(a, b));
            if out.iter().all(|&b| b) || out.iter().all(|&b| !b) {
                continue;
            }
            let edges = [
                horizontal(i, j),
                vertical(i + 1, j),
                horizontal(i, j + 1),
                vertical(i, j),
            ];
            // 印あり → 印なし は入り、その逆は出
            let mut starts = Vec::new();
            let mut ends = Vec::new();
            for k in 0..4 {
                let (a, b) = (out[k], out[(k + 1) % 4]);
                if a != b {
                    let (ca, cb) = (corners[k], corners[(k + 1) % 4]);
                    let (fa, fb) = if a { (cb, ca) } else { (ca, cb) };
                    points.entry(edges[k]).or_insert_with(|| crossing(fa, fb));
                    if b {
                        starts.push(k);
                    } else {
                        ends.push(k);
                    }
                }
            }
            if starts.len() == 1 {
                next.insert(edges[starts[0]], edges[ends[0]]);
            } else {
                // 鞍点: 真ん中に印がなければ、印のある隅を 1 つずつ切り取る
                let center_blocked = !center(i, j);
                for &s in &starts {
                    let e = if center_blocked {
                        (s + 3) % 4
                    } else {
                        (s + 1) % 4
                    };
                    next.insert(edges[s], edges[e]);
                }
            }
        }
    }
    let mut loops = Vec::new();
    // 始める辺の順を決めて、同じ入力なら同じ輪の順になるようにする
    let mut keys: Vec<usize> = next.keys().copied().collect();
    keys.sort_unstable();
    for first in keys {
        if !next.contains_key(&first) {
            continue;
        }
        let mut lp = Vec::new();
        let mut e = first;
        while let Some(n) = next.remove(&e) {
            lp.push(points[&e]);
            e = n;
        }
        let length: f64 = (0..lp.len())
            .map(|k| {
                let (a, b) = (lp[k], lp[(k + 1) % lp.len()]);
                (a[0] - b[0]).hypot(a[1] - b[1])
            })
            .sum();
        if e == first && length >= min_length {
            loops.push(crate::simplify_loop(&lp));
        }
    }
    loops
}

/// 印 `mask`(`grid` の節点の並び)の境目の輪。境目の点は節点の間の真ん中。向きは [`trace`] と同じ
/// (印の付いた所の外周が反時計回り)。
pub fn mask_loops(grid: &Grid, mask: &[bool], min_length: f64) -> Vec<Vec<P2>> {
    let nx = grid.nx;
    let inside = |i: usize, j: usize| mask[j * nx + i];
    let center = |_: usize, _: usize| false;
    let mut crossing = |a: (usize, usize), b: (usize, usize)| {
        let (pa, pb) = (grid.at(a.0, a.1), grid.at(b.0, b.1));
        [(pa[0] + pb[0]) / 2.0, (pa[1] + pb[1]) / 2.0]
    };
    trace(
        grid.nx,
        grid.ny,
        &inside,
        &center,
        &mut crossing,
        min_length,
    )
}

/// 節点の値 `values`(`grid` の並び)の、高さ `level` の等高線の輪。境目の点は節点の間を値で直線に
/// 補う。向きは [`trace`] と同じ(`level` 以上の所の外周が反時計回り)。
pub fn level_loops(grid: &Grid, values: &[f64], level: f64, min_length: f64) -> Vec<Vec<P2>> {
    let nx = grid.nx;
    let v = |i: usize, j: usize| values[j * nx + i];
    let inside = |i: usize, j: usize| v(i, j) >= level;
    let center =
        |i: usize, j: usize| (v(i, j) + v(i + 1, j) + v(i + 1, j + 1) + v(i, j + 1)) / 4.0 >= level;
    let mut crossing = |a: (usize, usize), b: (usize, usize)| {
        let (pa, pb) = (grid.at(a.0, a.1), grid.at(b.0, b.1));
        let (fa, fb) = (v(a.0, a.1), v(b.0, b.1));
        let t = if (fa - fb).abs() < 1e-12 {
            0.5
        } else {
            ((fa - level) / (fa - fb)).clamp(0.0, 1.0)
        };
        [pa[0] + (pb[0] - pa[0]) * t, pa[1] + (pb[1] - pa[1]) * t]
    };
    trace(
        grid.nx,
        grid.ny,
        &inside,
        &center,
        &mut crossing,
        min_length,
    )
}

/// 閉じた点の列を、どの点も元の線から `tol` 以内に収まるよう間引く(ダグラス・ポーカー)。
pub fn simplify_closed(pts: &[P2], tol: f64) -> Vec<P2> {
    let n = pts.len();
    if n < 4 || tol <= 0.0 {
        return pts.to_vec();
    }
    // 始点と、始点からいちばん遠い点で 2 つに分ける
    let far = (1..n)
        .max_by(|&a, &b| dist(pts[0], pts[a]).total_cmp(&dist(pts[0], pts[b])))
        .unwrap_or(n / 2);
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[far] = true;
    let ring: Vec<P2> = pts
        .iter()
        .chain(std::iter::once(&pts[0]))
        .copied()
        .collect();
    dp(&ring, 0, far, tol, &mut keep);
    dp(&ring, far, n, tol, &mut keep);
    let out: Vec<P2> = (0..n).filter(|&k| keep[k]).map(|k| pts[k]).collect();
    if out.len() < 3 { pts.to_vec() } else { out }
}

fn dist(a: P2, b: P2) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn dp(ring: &[P2], a: usize, b: usize, tol: f64, keep: &mut [bool]) {
    if b <= a + 1 {
        return;
    }
    let (pa, pb) = (ring[a], ring[b]);
    let (mut worst, mut at) = (-1.0, a);
    for (k, &q) in ring.iter().enumerate().take(b).skip(a + 1) {
        let d = seg_dist(q, pa, pb);
        if d > worst {
            worst = d;
            at = k;
        }
    }
    if worst > tol {
        keep[at % keep.len()] = true;
        dp(ring, a, at, tol, keep);
        dp(ring, at, b, tol, keep);
    }
}

/// 向きの付いた輪(反時計回りが外周、時計回りが内周。[`trace`] の輪のように交わらず入れ子に
/// なっているもの)を領域に組む。内周は、それを含む外周のうちいちばん小さいものに入れる。
pub fn areas_from_loops(loops: Vec<Vec<P2>>) -> Vec<Area> {
    let mut outers: Vec<(f64, Vec<P2>, Vec<Loop>)> = Vec::new();
    let mut holes: Vec<Vec<P2>> = Vec::new();
    for l in loops {
        if l.len() < 3 {
            continue;
        }
        let a = signed_area(&l);
        if a > 0.0 {
            outers.push((a, l, Vec::new()));
        } else if a < 0.0 {
            holes.push(l);
        }
    }
    for h in holes {
        let q = h[0];
        if let Some(o) = outers
            .iter_mut()
            .filter(|o| point_in_polygon(&o.1, q))
            .min_by(|x, y| x.0.total_cmp(&y.0))
        {
            o.2.push(Loop::from_points(&h));
        }
    }
    outers
        .into_iter()
        .map(|(_, o, hs)| Area {
            outer: Loop::from_points(&o),
            holes: hs,
        })
        .collect()
}

/// 点が入るかを速く調べる領域(外周と内周を折れ線にしたもの。偶奇の規則で調べる)。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Region {
    loops: Vec<Vec<P2>>,
    min: P2,
    max: P2,
}

impl Region {
    pub fn new(areas: &[Area]) -> Region {
        let loops: Vec<Vec<P2>> = areas
            .iter()
            .flat_map(|a| std::iter::once(&a.outer).chain(&a.holes))
            .map(|l| l.flatten(1e-3))
            .filter(|l| l.len() >= 3)
            .collect();
        Region::from_loops(loops)
    }

    fn from_loops(loops: Vec<Vec<P2>>) -> Region {
        let mut min = [f64::INFINITY; 2];
        let mut max = [f64::NEG_INFINITY; 2];
        for p in loops.iter().flatten() {
            for k in 0..2 {
                min[k] = min[k].min(p[k]);
                max[k] = max[k].max(p[k]);
            }
        }
        Region { loops, min, max }
    }

    pub fn is_empty(&self) -> bool {
        self.loops.is_empty()
    }

    /// 外接する長方形(左下と右上)。
    pub fn bounds(&self) -> (P2, P2) {
        (self.min, self.max)
    }

    /// 点を `f` で動かした領域(走査線の角度に合わせて回すのに使う)。
    pub fn map(&self, f: impl Fn(P2) -> P2) -> Region {
        Region::from_loops(
            self.loops
                .iter()
                .map(|l| l.iter().map(|&p| f(p)).collect())
                .collect(),
        )
    }

    pub fn contains(&self, p: P2) -> bool {
        if p[0] < self.min[0] || p[0] > self.max[0] || p[1] < self.min[1] || p[1] > self.max[1] {
            return false;
        }
        let mut inside = false;
        for l in &self.loops {
            if point_in_polygon(l, p) {
                inside = !inside;
            }
        }
        inside
    }

    /// 格子の節点ごとの、領域に入るかの印(行ごとに辺との交わりを並べて塗る)。
    pub fn mask(&self, g: &Grid) -> Vec<bool> {
        let mut out = vec![false; g.nx * g.ny];
        let mut xs: Vec<f64> = Vec::new();
        for j in 0..g.ny {
            let y = g.origin[1] + j as f64 * g.cell;
            if y < self.min[1] || y > self.max[1] {
                continue;
            }
            xs.clear();
            for l in &self.loops {
                let n = l.len();
                for k in 0..n {
                    let (a, b) = (l[k], l[(k + 1) % n]);
                    if (a[1] > y) != (b[1] > y) {
                        xs.push(a[0] + (y - a[1]) / (b[1] - a[1]) * (b[0] - a[0]));
                    }
                }
            }
            xs.sort_by(|a, b| a.total_cmp(b));
            for w in xs.chunks(2) {
                let [x0, x1] = [w[0], *w.get(1).unwrap_or(&w[0])];
                let i0 = ((x0 - g.origin[0]) / g.cell).ceil().max(0.0) as usize;
                let i1 = ((x1 - g.origin[0]) / g.cell).floor();
                if i1 < 0.0 {
                    continue;
                }
                for i in i0..=(i1 as usize).min(g.nx - 1) {
                    out[j * g.nx + i] = true;
                }
            }
        }
        out
    }
}

/// 1 次元の距離の 2 乗の変換(フェルゼンツワルブとハッテンロッカー)。`f` は各点の値
/// (印のある所は 0、ない所は大きな値)。
fn edt_1d(f: &[f64], out: &mut [f64], v: &mut [usize], z: &mut [f64]) {
    let n = f.len();
    let mut k = 0usize;
    v[0] = 0;
    z[0] = f64::NEG_INFINITY;
    z[1] = f64::INFINITY;
    let s = |p: usize, q: usize| {
        ((f[q] + (q * q) as f64) - (f[p] + (p * p) as f64)) / (2.0 * (q - p) as f64)
    };
    for q in 1..n {
        let mut x = s(v[k], q);
        while x <= z[k] {
            k -= 1;
            x = s(v[k], q);
        }
        k += 1;
        v[k] = q;
        z[k] = x;
        z[k + 1] = f64::INFINITY;
    }
    k = 0;
    for (q, o) in out.iter_mut().enumerate() {
        while z[k + 1] < q as f64 {
            k += 1;
        }
        let p = v[k];
        let d = q as f64 - p as f64;
        *o = d * d + f[p];
    }
}

/// 印のある節点までの距離の 2 乗(節点の数を単位にする)。印が 1 つもなければ大きな値。
pub fn distance2(mask: &[bool], nx: usize, ny: usize) -> Vec<f64> {
    const BIG: f64 = 1e18;
    let mut d: Vec<f64> = mask.iter().map(|&m| if m { 0.0 } else { BIG }).collect();
    let n = nx.max(ny);
    let (mut f, mut out) = (vec![0.0; n], vec![0.0; n]);
    let (mut v, mut z) = (vec![0usize; n], vec![0.0; n + 1]);
    for j in 0..ny {
        f[..nx].copy_from_slice(&d[j * nx..(j + 1) * nx]);
        edt_1d(&f[..nx], &mut out[..nx], &mut v, &mut z);
        d[j * nx..(j + 1) * nx].copy_from_slice(&out[..nx]);
    }
    for i in 0..nx {
        for j in 0..ny {
            f[j] = d[j * nx + i];
        }
        edt_1d(&f[..ny], &mut out[..ny], &mut v, &mut z);
        for j in 0..ny {
            d[j * nx + i] = out[j].min(BIG);
        }
    }
    d
}

/// 印を半径 `radius`(節点の数の単位)だけ広げる。
pub fn dilate(mask: &[bool], nx: usize, ny: usize, radius: f64) -> Vec<bool> {
    let r2 = radius * radius + 1e-9;
    distance2(mask, nx, ny)
        .into_iter()
        .map(|d| d <= r2)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_transform_matches_brute_force() {
        let (nx, ny) = (23, 17);
        let mask: Vec<bool> = (0..nx * ny).map(|k| k % 37 == 5 || k % 53 == 11).collect();
        let d = distance2(&mask, nx, ny);
        for j in 0..ny {
            for i in 0..nx {
                let mut best = f64::INFINITY;
                for jj in 0..ny {
                    for ii in 0..nx {
                        if mask[jj * nx + ii] {
                            let (di, dj) = (i as f64 - ii as f64, j as f64 - jj as f64);
                            best = best.min(di * di + dj * dj);
                        }
                    }
                }
                assert!((d[j * nx + i] - best).abs() < 1e-9, "({i}, {j})");
            }
        }
    }

    #[test]
    fn a_ring_shaped_mask_becomes_one_area_with_a_hole() {
        let g = Grid {
            origin: [0.0, 0.0],
            cell: 0.5,
            nx: 41,
            ny: 41,
        };
        // 中心 (10, 10)、半径 3〜8 の輪
        let mask: Vec<bool> = (0..g.nx * g.ny)
            .map(|k| {
                let p = g.at(k % g.nx, k / g.nx);
                let r = (p[0] - 10.0).hypot(p[1] - 10.0);
                (3.0..=8.0).contains(&r)
            })
            .collect();
        let areas = areas_from_loops(mask_loops(&g, &mask, 1.0));
        assert_eq!(areas.len(), 1);
        assert_eq!(areas[0].holes.len(), 1);
        let want = std::f64::consts::PI * (64.0 - 9.0);
        assert!(
            (areas[0].area() - want).abs() / want < 0.08,
            "{}",
            areas[0].area()
        );
        let reg = Region::new(&areas);
        assert!(reg.contains([15.0, 10.0]) && !reg.contains([10.0, 10.0]));
        let m = reg.mask(&g);
        let same = m.iter().zip(&mask).filter(|(a, b)| a == b).count();
        assert!(same as f64 / mask.len() as f64 > 0.97);
    }

    #[test]
    fn simplify_keeps_points_within_the_tolerance() {
        let pts: Vec<P2> = (0..200)
            .map(|k| {
                let t = k as f64 / 200.0 * std::f64::consts::TAU;
                [10.0 * t.cos(), 10.0 * t.sin()]
            })
            .collect();
        let s = simplify_closed(&pts, 0.05);
        assert!(s.len() < 60, "{}", s.len());
        for p in &pts {
            let d = (0..s.len())
                .map(|k| seg_dist(*p, s[k], s[(k + 1) % s.len()]))
                .fold(f64::INFINITY, f64::min);
            assert!(d <= 0.05 + 1e-9);
        }
    }
}
