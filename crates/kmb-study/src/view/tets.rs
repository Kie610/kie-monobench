//! 四面体の網の結果の塗り方: 表面を色の帯で塗り、断面で切った所を塗る。辺の中点で曲げた
//! 2 次の三角形を小三角形に分け、陰の法線を面ごとに滑らかにする。

use super::*;

/// 塗る値の出どころ: 点の値(成分の数)か、応力の 6 成分から求める量。
#[derive(Debug, Clone, Copy)]
pub(super) enum Src<'a> {
    Plain(&'a Values, usize),
    Tensor(&'a Values, Derive),
}

impl Src<'_> {
    /// 点 `i` の、補間する値(スカラーは値を 1 つ目に、応力は 6 成分)。
    fn at(&self, i: usize) -> [f64; 6] {
        match self {
            Src::Plain(v, c) => [v.magnitude(i, *c) as f64, 0.0, 0.0, 0.0, 0.0, 0.0],
            Src::Tensor(v, _) => v.vec6(i),
        }
    }

    /// 補間した値から、塗る値。
    fn value(&self, x: &[f64; 6]) -> f32 {
        match self {
            Src::Plain(..) => x[0] as f32,
            Src::Tensor(_, d) => d.of(x) as f32,
        }
    }
}

/// 四面体の網の点(元の位置、変形の分、表面と断面の補間する値、陰の法線)。
#[derive(Debug, Clone, Copy)]
struct Vtx {
    p: [f64; 3],
    d: [f64; 3],
    s: [f64; 6],
    c: [f64; 6],
    /// 陰の法線(変形した形で。長さが無ければ三角形の向きで平らに塗る)
    n: [f64; 3],
}

impl Vtx {
    fn lerp(a: Vtx, b: Vtx, f: f64) -> Vtx {
        let m = |x: f64, y: f64| x + (y - x) * f;
        Vtx {
            p: [0, 1, 2].map(|k| m(a.p[k], b.p[k])),
            d: [0, 1, 2].map(|k| m(a.d[k], b.d[k])),
            s: std::array::from_fn(|k| m(a.s[k], b.s[k])),
            c: std::array::from_fn(|k| m(a.c[k], b.c[k])),
            n: [0, 1, 2].map(|k| m(a.n[k], b.n[k])),
        }
    }

    fn at(&self) -> [f64; 3] {
        [0, 1, 2].map(|k| self.p[k] + self.d[k])
    }
}

/// 多角形の `p[axis] <= at` の側を残す。
fn keep_below(poly: &[Vtx], axis: usize, at: f64) -> Vec<Vtx> {
    let mut out = Vec::with_capacity(poly.len() + 2);
    for k in 0..poly.len() {
        let (a, b) = (poly[k], poly[(k + 1) % poly.len()]);
        let (da, db) = (a.p[axis] - at, b.p[axis] - at);
        if da <= 0.0 {
            out.push(a);
        }
        if (da <= 0.0) != (db <= 0.0) {
            out.push(Vtx::lerp(a, b, da / (da - db)));
        }
    }
    out
}

/// 多角形を扇に分けて、段の座標 `band`(無ければ灰色)で塗って足す。
fn paint_polygon(
    b: &mut Builder,
    poly: &[Vtx],
    band: impl Fn(&Vtx) -> f64,
    scale: Option<&Scale>,
    gray: u32,
) {
    for k in 1..poly.len().saturating_sub(1) {
        let v = [poly[0], poly[k], poly[k + 1]];
        match scale {
            Some(s) if v.iter().all(|x| band(x).is_finite()) => paint_tri(b, v, &band, s, SPLIT),
            _ => b.tri_n(v.map(|x| x.at()), v.map(|x| x.n), gray),
        }
    }
}

/// 三角形を細かく分ける深さの上限と、分ける目安(段の幅に対するずれ)。
const SPLIT: u32 = 4;
const SPLIT_TOL: f64 = 0.15;

/// 三角形を塗る。辺の中点や重心で、補間した値から求めた段が頂点の段の平均とずれる時は 4 つに
/// 分ける(ミーゼス応力は成分の線形でないので、応力の向きが変わる所で V 字に折れる)。
fn paint_tri(b: &mut Builder, v: [Vtx; 3], band: &impl Fn(&Vtx) -> f64, s: &Scale, depth: u32) {
    let t = v.map(|x| band(&x));
    if depth > 0 {
        let m = [
            Vtx::lerp(v[0], v[1], 0.5),
            Vtx::lerp(v[1], v[2], 0.5),
            Vtx::lerp(v[2], v[0], 0.5),
        ];
        let c = Vtx::lerp(m[0], v[2], 1.0 / 3.0);
        let off = [
            band(&m[0]) - (t[0] + t[1]) / 2.0,
            band(&m[1]) - (t[1] + t[2]) / 2.0,
            band(&m[2]) - (t[2] + t[0]) / 2.0,
            band(&c) - (t[0] + t[1] + t[2]) / 3.0,
        ];
        if off.iter().any(|d| d.abs() > SPLIT_TOL) {
            for sub in [
                [v[0], m[0], m[2]],
                [m[0], v[1], m[1]],
                [m[2], m[1], v[2]],
                [m[0], m[1], m[2]],
            ] {
                paint_tri(b, sub, band, s, depth - 1);
            }
            return;
        }
    }
    b.banded_n(v.map(|x| x.at()), v.map(|x| x.n), t, s);
}

/// 表面の 6 節点の三角形を辺の中点で 4 つに分ける時の、各小三角形の節点(三角形の中の番号)。
/// [`Tets::surface_tris`] と同じ分け方。
const SURFACE_SUBS: [[usize; 3]; 4] = [[0, 3, 5], [3, 1, 4], [5, 4, 2], [3, 4, 5]];

/// 6 節点の三角形の節点の (r, s)(角 a・b・c、辺の中点 ab・bc・ca)。
const NODE_RS: [[f64; 2]; 6] = [
    [0.0, 0.0],
    [1.0, 0.0],
    [0.0, 1.0],
    [0.5, 0.0],
    [0.5, 0.5],
    [0.0, 0.5],
];

/// 陰の法線をつなぐ角の上限(形の面の番号が分からない面どうし)。これより開いた所は縁として
/// 平らに分けたままにする。
const CREASE_DEG: f64 = 45.0;

/// 表面の 6 節点の三角形ごとの、6 つの節点での陰の法線(`at` は点の位置。変形した形を渡す)。
///
/// 各三角形の曲がった面(2 次の形関数)の法線を節点で求め、同じ形の面に属する三角形どうしで
/// 足して(重みはヤコビアンの大きさ)単位にする。形の面の番号が分からない三角形どうしは、
/// 向きの差が [`CREASE_DEG`] 未満の物だけをつなぐ。小三角形ごとの平らな法線で塗ると、
/// 曲面の上で小三角形の境ごとに陰が跳ぶ(見本の部品の直径 3.4 mm の穴で、隣の小三角形との
/// 差が平均 8°・最大 25°、網のつなぎ方がそのまま網目に見える)。
fn surface_normals(t: &Tets, at: impl Fn(u32) -> [f64; 3]) -> Vec<[[f64; 3]; 6]> {
    // 三角形の中の点 (r,s) での、位置の r・s による微分の外積(長さは面積の割合)
    let raw: Vec<[[f64; 3]; 6]> = t
        .faces
        .iter()
        .map(|f| {
            let x = f.map(&at);
            NODE_RS.map(|[r, s]| {
                let l = 1.0 - r - s;
                let dr = [
                    -(4.0 * l - 1.0),
                    4.0 * r - 1.0,
                    0.0,
                    4.0 * (l - r),
                    4.0 * s,
                    -4.0 * s,
                ];
                let ds = [
                    -(4.0 * l - 1.0),
                    0.0,
                    4.0 * s - 1.0,
                    -4.0 * r,
                    4.0 * r,
                    4.0 * (l - s),
                ];
                let mut a = [0.0; 3];
                let mut b = [0.0; 3];
                for k in 0..6 {
                    for c in 0..3 {
                        a[c] += dr[k] * x[k][c];
                        b[c] += ds[k] * x[k][c];
                    }
                }
                cross3(a, b)
            })
        })
        .collect();
    let mut at_node: std::collections::HashMap<u32, Vec<(usize, usize)>> =
        std::collections::HashMap::new();
    for (i, f) in t.faces.iter().enumerate() {
        for (k, v) in f.iter().enumerate() {
            at_node.entry(*v).or_default().push((i, k));
        }
    }
    let label = |i: usize| t.face_ids.get(i).copied().unwrap_or(u32::MAX);
    let crease = CREASE_DEG.to_radians().cos();
    let mut out = vec![[[0.0; 3]; 6]; t.faces.len()];
    for (i, f) in t.faces.iter().enumerate() {
        for k in 0..6 {
            let Some(own) = unit(raw[i][k]) else {
                continue;
            };
            let mut sum = [0.0; 3];
            for &(j, m) in &at_node[&f[k]] {
                let same = match (label(i), label(j)) {
                    (a, b) if a != u32::MAX && b != u32::MAX => a == b,
                    _ => unit(raw[j][m])
                        .is_some_and(|n| own[0] * n[0] + own[1] * n[1] + own[2] * n[2] > crease),
                };
                if same {
                    for c in 0..3 {
                        sum[c] += raw[j][m][c];
                    }
                }
            }
            out[i][k] = unit(sum).unwrap_or(own);
        }
    }
    out
}

/// 2 次の四面体の網: 表面を辺の中点で分けて塗り、断面があれば `p[axis] > at` の側を取り除いて
/// 切り口を塗る(要素を 8 つの 1 次の四面体に分けて平面で切る)。
pub(super) fn tets_mesh(
    b: &mut Builder,
    t: &Tets,
    disp: Option<(&Values, f64)>,
    paint: Option<(Src, &Scale)>,
    cut: Option<(usize, f64, Src, &Scale)>,
    gray: u32,
) {
    let mut lo = [f64::MAX; 3];
    let mut hi = [f64::MIN; 3];
    for p in &t.points {
        for a in 0..3 {
            lo[a] = lo[a].min(p[a] as f64);
            hi[a] = hi[a].max(p[a] as f64);
        }
    }
    let size = (0..3).map(|a| hi[a] - lo[a]).fold(1e-9, f64::max);
    let vtx = |i: u32| -> Vtx {
        let i = i as usize;
        let d = disp.map_or([0.0; 3], |(v, k)| v.vec3(i).map(|x| k * x as f64));
        Vtx {
            p: t.points[i].map(f64::from),
            d,
            s: paint.map_or([f64::NAN; 6], |(v, _)| v.at(i)),
            c: cut.map_or([f64::NAN; 6], |(_, _, v, _)| v.at(i)),
            n: [0.0; 3],
        }
    };
    // 辺や頂点にちょうど乗らないよう、平面をわずかにずらす
    let plane = cut.map(|(axis, at, ..)| (axis, at + 1.37e-6 * size));
    let surface_band = |x: &Vtx| paint.map_or(f64::NAN, |(v, s)| s.t(v.value(&x.s)));
    let normals = surface_normals(t, |i| vtx(i).at());
    for (f, n6) in t.faces.iter().zip(&normals) {
        let mut v6 = f.map(vtx);
        for (v, n) in v6.iter_mut().zip(n6) {
            v.n = *n;
        }
        for sub in SURFACE_SUBS {
            let poly = sub.map(|k| v6[k]);
            let poly = match plane {
                Some((axis, at)) => keep_below(&poly, axis, at),
                None => poly.to_vec(),
            };
            paint_polygon(b, &poly, surface_band, paint.map(|p| p.1), gray);
        }
    }
    let Some((axis, at)) = plane else { return };
    let cut_band = |x: &Vtx| cut.map_or(f64::NAN, |(_, _, v, s)| s.t(v.value(&x.c)));
    for e in 0..t.tets.len() {
        let (mut l, mut h) = (f64::MAX, f64::MIN);
        for i in t.tets[e] {
            let x = t.points[i as usize][axis] as f64;
            l = l.min(x);
            h = h.max(x);
        }
        if at < l || at > h {
            continue;
        }
        for sub in t.linear(e) {
            let v = sub.map(vtx);
            let below: Vec<usize> = (0..4).filter(|k| v[*k].p[axis] <= at).collect();
            let above: Vec<usize> = (0..4).filter(|k| v[*k].p[axis] > at).collect();
            let edge = |i: usize, j: usize| {
                let (di, dj) = (v[i].p[axis] - at, v[j].p[axis] - at);
                Vtx::lerp(v[i], v[j], di / (di - dj))
            };
            let mut poly = match (below.len(), above.len()) {
                (1, 3) => above.iter().map(|j| edge(below[0], *j)).collect::<Vec<_>>(),
                (3, 1) => below.iter().map(|i| edge(*i, above[0])).collect(),
                (2, 2) => vec![
                    edge(below[0], above[0]),
                    edge(below[0], above[1]),
                    edge(below[1], above[1]),
                    edge(below[1], above[0]),
                ],
                _ => continue,
            };
            // 切り口は取り除いた側(+axis)を向ける
            let (p0, p1, p2) = (poly[0].p, poly[1].p, poly[2].p);
            let u = [0, 1, 2].map(|k| p1[k] - p0[k]);
            let w = [0, 1, 2].map(|k| p2[k] - p0[k]);
            let n = [
                u[1] * w[2] - u[2] * w[1],
                u[2] * w[0] - u[0] * w[2],
                u[0] * w[1] - u[1] * w[0],
            ];
            if n[axis] < 0.0 {
                poly.reverse();
            }
            paint_polygon(b, &poly, cut_band, cut.map(|c| c.3), gray);
        }
    }
}

/// 変形の倍率: いちばん大きく動く所が、形の大きさの 6% ほどに見えるように(全部のコマで同じ倍率)。
pub(super) fn deform_factor(r: &SimResult, q: usize, points: &[[f32; 3]]) -> f64 {
    let mut maxd = 0.0f32;
    for f in 0..r.frames.len() {
        if let Some(v) = r.values(f, q) {
            for p in 0..points.len() {
                maxd = maxd.max(v.magnitude(p, 3));
            }
        }
    }
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for p in points {
        for a in 0..3 {
            lo[a] = lo[a].min(p[a]);
            hi[a] = hi[a].max(p[a]);
        }
    }
    let size = (0..3)
        .map(|a| ((hi[a] - lo[a]) as f64).powi(2))
        .sum::<f64>()
        .sqrt();
    if maxd > 0.0 {
        nice(0.06 * size / maxd as f64)
    } else {
        0.0
    }
}
