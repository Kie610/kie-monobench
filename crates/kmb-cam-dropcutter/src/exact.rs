//! 三角形の網へ工具を直接下ろす(格子を使わない)。
//!
//! 三角形ごとに、工具が面の内側・稜線(両端の頂点を含む)に触れる高さを求め、そのいちばん
//! 高い所を取る(Hwang & Chang 1998 などと同じ考え方)。工具の形は中心からの距離について
//! 下に凸(ボール・平・ブルノーズ・V・テーパー・テーパーボールと、それを球で太らせた形)
//! なので、面の上でも稜線の上でも「部品の高さ − 刃の高さ」は山が 1 つになる。山の頂上を
//! 黄金分割で探す。
//!
//! 格子([`crate::Heights`])はセルの 4 隅のいちばん高い所をセル全体の高さとみなすので、
//! 縦の壁の際で最大でセル 1 つ分外へずれる。こちらのずれは網の許し(三角形にした誤差)だけ。

use crate::{Profile, Shape};
use kmb_cam_geom::z_on_triangle;

type P2 = [f64; 2];
type P3 = [f64; 3];

/// 黄金分割で山の頂上を探す回数(区間は 0.618^40 ≈ 4e-9 倍に縮む)
const STEPS: usize = 40;

struct Tri {
    v: [P3; 3],
    zmax: f64,
    lo: P2,
    hi: P2,
}

/// 三角形の網と、xy の升目ごとの三角形の一覧。
pub struct Mesh {
    tris: Vec<Tri>,
    origin: P2,
    bin: f64,
    nx: usize,
    ny: usize,
    bins: Vec<Vec<u32>>,
}

impl Mesh {
    pub fn new(tris: &[[P3; 3]]) -> Mesh {
        let tris: Vec<Tri> = tris
            .iter()
            .map(|t| {
                let f =
                    |k: usize, g: fn(f64, f64) -> f64, s: f64| t.iter().map(|v| v[k]).fold(s, g);
                Tri {
                    v: *t,
                    zmax: f(2, f64::max, f64::NEG_INFINITY),
                    lo: [f(0, f64::min, f64::INFINITY), f(1, f64::min, f64::INFINITY)],
                    hi: [
                        f(0, f64::max, f64::NEG_INFINITY),
                        f(1, f64::max, f64::NEG_INFINITY),
                    ],
                }
            })
            .collect();
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for t in &tris {
            for k in 0..2 {
                lo[k] = lo[k].min(t.lo[k]);
                hi[k] = hi[k].max(t.hi[k]);
            }
        }
        if tris.is_empty() {
            (lo, hi) = ([0.0; 2], [0.0; 2]);
        }
        // 升目は 1 升に三角形が数個入る程度の大きさにする
        let area = ((hi[0] - lo[0]) * (hi[1] - lo[1])).max(1e-6);
        let bin = (area / tris.len().max(1) as f64 * 4.0)
            .sqrt()
            .clamp(0.25, 10.0);
        let nx = ((hi[0] - lo[0]) / bin).floor() as usize + 1;
        let ny = ((hi[1] - lo[1]) / bin).floor() as usize + 1;
        let mut bins = vec![Vec::new(); nx * ny];
        let cell =
            |v: f64, o: f64, n: usize| (((v - o) / bin).floor().max(0.0) as usize).min(n - 1);
        for (k, t) in tris.iter().enumerate() {
            for j in cell(t.lo[1], lo[1], ny)..=cell(t.hi[1], lo[1], ny) {
                for i in cell(t.lo[0], lo[0], nx)..=cell(t.hi[0], lo[0], nx) {
                    bins[j * nx + i].push(k as u32);
                }
            }
        }
        // 高い三角形から当てると、先端が早く上がって後の三角形を飛ばせる
        for b in &mut bins {
            b.sort_by(|x, y| tris[*y as usize].zmax.total_cmp(&tris[*x as usize].zmax));
        }
        Mesh {
            tris,
            origin: lo,
            bin,
            nx,
            ny,
            bins,
        }
    }

    /// 工具の中心を `p` に置いて下ろした時の先端の高さ(部品に触れなければ `floor`)。
    pub fn drop_at<P: Profile + ?Sized>(&self, tool: &P, p: P2, floor: f64) -> f64 {
        let r = tool.radius();
        let mut z = floor;
        if self.tris.is_empty() {
            return z;
        }
        let range = |v: f64, axis: usize, n: usize| {
            let o = self.origin[axis];
            let a = ((v - r - o) / self.bin).floor();
            let b = ((v + r - o) / self.bin).floor();
            if b < 0.0 || a > (n - 1) as f64 {
                return 0..0;
            }
            (a.max(0.0) as usize)..((b as usize).min(n - 1) + 1)
        };
        let (cols, rows) = (range(p[0], 0, self.nx), range(p[1], 1, self.ny));
        let mut seen: Vec<u32> = Vec::new();
        for j in rows {
            for i in cols.clone() {
                for &k in &self.bins[j * self.nx + i] {
                    let t = &self.tris[k as usize];
                    // 一覧は高い順なので、ここから先の三角形は先端を上げない
                    if t.zmax <= z {
                        break;
                    }
                    let dx = (t.lo[0] - p[0]).max(p[0] - t.hi[0]).max(0.0);
                    let dy = (t.lo[1] - p[1]).max(p[1] - t.hi[1]).max(0.0);
                    // 刃は外ほど高いので、いちばん近い所で当たっても先端を上げない三角形は飛ばす
                    match blade(tool, dx.hypot(dy)) {
                        Some(k) if t.zmax - k > z => {}
                        _ => continue,
                    }
                    // 升目をまたぐ三角形は 1 回だけ当てる
                    if seen.contains(&k) {
                        continue;
                    }
                    seen.push(k);
                    z = z.max(touch(tool, &t.v, p, z));
                }
            }
        }
        z
    }
}

/// 刃の高さ。刃の外は当たらない。ちょうど半径の所は丸めの誤差で外れないよう、少し外まで
/// 刃があるとみなす。
fn blade<P: Profile + ?Sized>(tool: &P, d: f64) -> Option<f64> {
    (d <= tool.radius() + 1e-9)
        .then(|| tool.height_at(d.min(tool.radius())))
        .flatten()
}

/// `f` の [lo, hi] での最大(`f` は山が 1 つ)。両端も比べる。
fn peak(lo: f64, hi: f64, f: impl Fn(f64) -> f64) -> f64 {
    let g = (5f64.sqrt() - 1.0) / 2.0;
    let (mut a, mut b) = (lo, hi);
    let (mut c, mut d) = (b - g * (b - a), a + g * (b - a));
    let (mut fc, mut fd) = (f(c), f(d));
    for _ in 0..STEPS {
        if fc >= fd {
            b = d;
            (d, fd) = (c, fc);
            c = b - g * (b - a);
            fc = f(c);
        } else {
            a = c;
            (c, fc) = (d, fd);
            d = a + g * (b - a);
            fd = f(d);
        }
    }
    fc.max(fd).max(f(lo)).max(f(hi))
}

/// 三角形 `t` に、中心 `p` の工具が触れる先端の高さ(触れなければ負の無限大)。
/// `below` 以下にしかならないと分かれば、数で探さずに負の無限大を返す。
fn touch<P: Profile + ?Sized>(tool: &P, t: &[P3; 3], p: P2, below: f64) -> f64 {
    match tool.shape() {
        Shape::Ball(r) => touch_ball(r, t, p),
        Shape::Flat(r) => touch_flat(r, t, p),
        // 刃の高さは負にならないので、同じ半径の平らな先が触れる高さを超えない
        Shape::Other if touch_flat(tool.radius(), t, p) <= below => f64::NEG_INFINITY,
        Shape::Other => touch_any(tool, t, p),
    }
}

/// 上を向いた単位の法線と、面の高さ(縦の面は `None`)。
fn plane(t: &[P3; 3]) -> Option<(P3, impl Fn(P2) -> f64 + '_)> {
    let e1 = [t[1][0] - t[0][0], t[1][1] - t[0][1], t[1][2] - t[0][2]];
    let e2 = [t[2][0] - t[0][0], t[2][1] - t[0][1], t[2][2] - t[0][2]];
    let mut n = [
        e1[1] * e2[2] - e1[2] * e2[1],
        e1[2] * e2[0] - e1[0] * e2[2],
        e1[0] * e2[1] - e1[1] * e2[0],
    ];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if len < 1e-15 || n[2].abs() <= 1e-9 * len {
        return None;
    }
    let sign = n[2].signum() / len;
    n = [n[0] * sign, n[1] * sign, n[2] * sign];
    Some((n, move |q: P2| {
        t[0][2] - (n[0] * (q[0] - t[0][0]) + n[1] * (q[1] - t[0][1])) / n[2]
    }))
}

/// 稜線 a→b のうち、中心 `p` から水平に `r` 以内の範囲 [l0, l1](0〜1)と、
/// 水平の距離の 2 乗の式の係数(|w + e l|² = ee l² + 2 bq l + ww)。
fn reach(a: P3, b: P3, p: P2, r: f64) -> Option<(f64, f64, f64, f64)> {
    let e = [b[0] - a[0], b[1] - a[1]];
    let w = [a[0] - p[0], a[1] - p[1]];
    let ee = e[0] * e[0] + e[1] * e[1];
    let (bq, ww) = (w[0] * e[0] + w[1] * e[1], w[0] * w[0] + w[1] * w[1]);
    let disc = bq * bq - ee * (ww - r * r);
    if ee < 1e-24 || disc < 0.0 {
        return None;
    }
    let root = disc.sqrt();
    let (l0, l1) = (((-bq - root) / ee).max(0.0), ((-bq + root) / ee).min(1.0));
    (l0 <= l1).then_some((l0, l1, ee, bq))
}

/// 球の先(半径 r)。面は接点が内側の時だけ、稜線は球と線分の距離が r になる高さ。
fn touch_ball(r: f64, t: &[P3; 3], p: P2) -> f64 {
    let mut z = f64::NEG_INFINITY;
    if let Some((n, f)) = plane(t) {
        // 中心は接点から法線の向きに r
        let q = [p[0] - r * n[0], p[1] - r * n[1]];
        if z_on_triangle(t, q).is_some() {
            z = f(q) + r * n[2] - r;
        }
    }
    for k in 0..3 {
        let (a, b) = (t[k], t[(k + 1) % 3]);
        let at = |l: f64| {
            let d2 = (a[0] + (b[0] - a[0]) * l - p[0]).powi(2)
                + (a[1] + (b[1] - a[1]) * l - p[1]).powi(2);
            a[2] + (b[2] - a[2]) * l + (r * r - d2).max(0.0).sqrt() - r
        };
        let Some((l0, l1, ee, bq)) = reach(a, b, p, r) else {
            // 縦の稜線か、届かない稜線。頂点だけ見る
            for v in [a, b] {
                let d2 = (v[0] - p[0]).powi(2) + (v[1] - p[1]).powi(2);
                if d2 <= r * r {
                    z = z.max(v[2] + (r * r - d2).sqrt() - r);
                }
            }
            continue;
        };
        // 山の頂上: ee l + bq = ez √(r² − d²) を解く(K = ee (r² − 線までの距離²))
        let ez = b[2] - a[2];
        let k = (bq * bq - ee * ((a[0] - p[0]).powi(2) + (a[1] - p[1]).powi(2) - r * r)).max(0.0);
        let m = ez * (k / (ee + ez * ez)).sqrt();
        let l = ((m - bq) / ee).clamp(l0, l1);
        z = z.max(at(l));
    }
    z
}

/// 平らな先(半径 r)。面は登り坂の向きに r の所、稜線は届く範囲の両端の高い方。
fn touch_flat(r: f64, t: &[P3; 3], p: P2) -> f64 {
    let mut z = f64::NEG_INFINITY;
    if let Some((n, f)) = plane(t) {
        let h = n[0].hypot(n[1]);
        let q = if h > 1e-12 {
            [p[0] - r * n[0] / h, p[1] - r * n[1] / h]
        } else {
            p
        };
        if z_on_triangle(t, q).is_some() {
            z = f(q);
        }
    }
    for k in 0..3 {
        let (a, b) = (t[k], t[(k + 1) % 3]);
        match reach(a, b, p, r) {
            Some((l0, l1, _, _)) => {
                z = z.max(a[2] + (b[2] - a[2]) * l0);
                z = z.max(a[2] + (b[2] - a[2]) * l1);
            }
            None => {
                for v in [a, b] {
                    if (v[0] - p[0]).hypot(v[1] - p[1]) <= r + 1e-9 {
                        z = z.max(v[2]);
                    }
                }
            }
        }
    }
    z
}

/// ほかの形: 刃の高さを数で探す。
fn touch_any<P: Profile + ?Sized>(tool: &P, t: &[P3; 3], p: P2) -> f64 {
    let r = tool.radius();
    let mut z = f64::NEG_INFINITY;
    // 面の内側: 中心から登り坂の向きへ距離 s の所で「面の高さ − 刃の高さ」がいちばん高い
    let e1 = [t[1][0] - t[0][0], t[1][1] - t[0][1], t[1][2] - t[0][2]];
    let e2 = [t[2][0] - t[0][0], t[2][1] - t[0][1], t[2][2] - t[0][2]];
    let mut n = [
        e1[1] * e2[2] - e1[2] * e2[1],
        e1[2] * e2[0] - e1[0] * e2[2],
        e1[0] * e2[1] - e1[1] * e2[0],
    ];
    if n[2] < 0.0 {
        n = [-n[0], -n[1], -n[2]];
    }
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if len > 1e-15 && n[2] > 1e-9 * len {
        let plane = |q: P2| t[0][2] - (n[0] * (q[0] - t[0][0]) + n[1] * (q[1] - t[0][1])) / n[2];
        let h = n[0].hypot(n[1]);
        let up = if h > 1e-12 {
            [-n[0] / h, -n[1] / h]
        } else {
            [0.0, 0.0]
        };
        let f = |s: f64| {
            plane([p[0] + up[0] * s, p[1] + up[1] * s]) - blade(tool, s).unwrap_or(f64::INFINITY)
        };
        // 山の頂上を探し、その場所が三角形の内側なら面に触れる
        let (mut a, mut b) = (0.0, if h > 1e-12 { r } else { 0.0 });
        let g = (5f64.sqrt() - 1.0) / 2.0;
        for _ in 0..STEPS {
            let (c, d) = (b - g * (b - a), a + g * (b - a));
            if f(c) >= f(d) {
                b = d;
            } else {
                a = c;
            }
        }
        let s = (a + b) / 2.0;
        let q = [p[0] + up[0] * s, p[1] + up[1] * s];
        if z_on_triangle(t, q).is_some() {
            z = z.max(f(s));
        }
    }
    // 稜線(両端の頂点を含む)
    for k in 0..3 {
        let (a, b) = (t[k], t[(k + 1) % 3]);
        let e = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let w = [a[0] - p[0], a[1] - p[1]];
        let ee = e[0] * e[0] + e[1] * e[1];
        let at = |l: f64| {
            let d = (w[0] + e[0] * l).hypot(w[1] + e[1] * l);
            blade(tool, d).map_or(f64::NEG_INFINITY, |k| a[2] + e[2] * l - k)
        };
        if ee < 1e-24 {
            // 縦の稜線
            z = z.max(at(0.0)).max(at(1.0));
            continue;
        }
        // 刃の届く範囲 |w + e l| <= r を解く
        let (bq, cq) = (w[0] * e[0] + w[1] * e[1], w[0] * w[0] + w[1] * w[1] - r * r);
        let disc = bq * bq - ee * cq;
        if disc < 0.0 {
            continue;
        }
        let root = disc.sqrt();
        let (l0, l1) = (((-bq - root) / ee).max(0.0), ((-bq + root) / ee).min(1.0));
        if l0 > l1 {
            continue;
        }
        z = z.max(peak(l0, l1, at));
    }
    z
}

#[cfg(test)]
mod tests {
    use super::*;
    use kmb_cam_tool::{Tool, ToolKind, ToolLibrary};

    fn ball(r: f64) -> Tool {
        let mut t = ToolLibrary::starter().tools[0].clone();
        t.kind = ToolKind::BallEnd;
        t.diameter = 2.0 * r;
        t
    }

    #[test]
    fn a_ball_on_a_flat_triangle_touches_with_its_tip() {
        let m = Mesh::new(&[[[0.0, 0.0, 2.0], [10.0, 0.0, 2.0], [0.0, 10.0, 2.0]]]);
        let z = m.drop_at(&ball(1.0), [3.0, 3.0], 0.0);
        assert!((z - 2.0).abs() < 1e-9, "{z}");
    }

    #[test]
    fn a_ball_beside_a_vertex_follows_the_sphere() {
        // とがった頂点 (0, 0, 5) の横 d に中心を置くと、先端は 5 + √(r² − d²) − r
        let m = Mesh::new(&[
            [[0.0, 0.0, 5.0], [-3.0, -3.0, 0.0], [3.0, -3.0, 0.0]],
            [[0.0, 0.0, 5.0], [3.0, -3.0, 0.0], [3.0, 3.0, 0.0]],
            [[0.0, 0.0, 5.0], [3.0, 3.0, 0.0], [-3.0, 3.0, 0.0]],
            [[0.0, 0.0, 5.0], [-3.0, 3.0, 0.0], [-3.0, -3.0, 0.0]],
        ]);
        let (r, d) = (2.0, 0.5);
        let z = m.drop_at(&ball(r), [d, 0.0], 0.0);
        let want = 5.0 + (r * r - d * d).sqrt() - r;
        assert!((z - want).abs() < 1e-7, "{z} {want}");
    }

    #[test]
    fn a_ball_on_a_slope_touches_the_face_off_center() {
        // 傾き 30 度の面: 中心の真下の面の高さより r(1/cos − 1) 上で止まる
        let s = 30f64.to_radians().tan();
        let m = Mesh::new(&[
            [
                [-10.0, -10.0, -10.0 * s],
                [10.0, -10.0, 10.0 * s],
                [10.0, 10.0, 10.0 * s],
            ],
            [
                [-10.0, -10.0, -10.0 * s],
                [10.0, 10.0, 10.0 * s],
                [-10.0, 10.0, -10.0 * s],
            ],
        ]);
        let r = 3.0;
        let z = m.drop_at(&ball(r), [1.0, 0.0], -100.0);
        let want = s + r * (1.0 / 30f64.to_radians().cos() - 1.0);
        assert!((z - want).abs() < 1e-7, "{z} {want}");
    }

    #[test]
    fn the_ball_and_flat_formulas_agree_with_the_numeric_search() {
        // 擬似乱数の三角形と点で、式で解く形と数で探す形が同じ高さになる
        let mut seed = 12345u64;
        let mut rnd = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 11) as f64 / (1u64 << 53) as f64 * 8.0 - 4.0
        };
        let mut flat = ball(1.5);
        flat.kind = ToolKind::FlatEnd;
        let (mut hits, mut worst) = (0, 0.0f64);
        for _ in 0..4000 {
            let t = [
                [rnd(), rnd(), rnd()],
                [rnd(), rnd(), rnd()],
                [rnd(), rnd(), rnd()],
            ];
            let p = [rnd() * 0.5, rnd() * 0.5];
            for tool in [ball(1.5), flat.clone()] {
                let (a, b) = (
                    touch(&tool, &t, p, f64::NEG_INFINITY),
                    touch_any(&tool, &t, p),
                );
                if a.is_finite() || b.is_finite() {
                    hits += 1;
                    worst = worst.max((a - b).abs());
                }
            }
        }
        assert!(hits > 2000, "{hits}");
        assert!(worst < 1e-7, "{worst}");
    }
}
