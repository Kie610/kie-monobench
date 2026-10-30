//! 四面体の網の結果の塗り方: 表面を色の帯で塗り、断面で切った所を塗る。辺の中点で曲げた
//! 2 次の三角形の中の座標で細分化し、陰の法線を面ごとに滑らかにする。

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
        self.value64(x) as f32
    }

    fn value64(&self, x: &[f64; 6]) -> f64 {
        match self {
            Src::Plain(..) => x[0],
            Src::Tensor(_, d) => d.of(x),
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

/// 6 節点の同じ形関数で、位置・変位・成分・陰の法線を求める。
fn quadratic(v: &[Vtx; 6], [r, s]: [f64; 2]) -> Vtx {
    let l = 1.0 - r - s;
    let w = [
        l * (2.0 * l - 1.0),
        r * (2.0 * r - 1.0),
        s * (2.0 * s - 1.0),
        4.0 * l * r,
        4.0 * r * s,
        4.0 * s * l,
    ];
    let sum = |f: &dyn Fn(&Vtx) -> f64| (0..6).map(|i| w[i] * f(&v[i])).sum();
    Vtx {
        p: std::array::from_fn(|k| sum(&|x| x.p[k])),
        d: std::array::from_fn(|k| sum(&|x| x.d[k])),
        s: std::array::from_fn(|k| sum(&|x| x.s[k])),
        c: std::array::from_fn(|k| sum(&|x| x.c[k])),
        n: std::array::from_fn(|k| sum(&|x| x.n[k])),
    }
}

fn mix_rs(a: [f64; 2], b: [f64; 2], f: f64) -> [f64; 2] {
    [0, 1].map(|k| a[k] + (b[k] - a[k]) * f)
}

/// 切る点も元の面の座標で求める。位置や成分を、分けた三角形の頂点から補間しない。
fn clip_surface(
    poly: &[[f64; 2]],
    value: &impl Fn([f64; 2]) -> f64,
    level: f64,
    above: bool,
) -> Vec<[f64; 2]> {
    let tagged: Vec<_> = poly.iter().map(|q| (*q, value(*q))).collect();
    clip_tagged(&tagged, Some(value), level, above)
        .into_iter()
        .map(|v| v.0)
        .collect()
}

/// 小片の段の境は直線で分け、その段の値を保つ。位置と陰は切った面の座標から求める。
fn clip_tagged(
    poly: &[([f64; 2], f64)],
    value: Option<&dyn Fn([f64; 2]) -> f64>,
    level: f64,
    above: bool,
) -> Vec<([f64; 2], f64)> {
    let inside = |v: f64| if above { v >= level } else { v <= level };
    let mut out = Vec::with_capacity(poly.len() + 2);
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        let ia = inside(a.1);
        if ia {
            out.push(a);
        }
        if ia != inside(b.1) {
            let f = match value {
                Some(value) => {
                    let (mut lo, mut hi) = (0.0, 1.0);
                    for _ in 0..36 {
                        let mid = (lo + hi) * 0.5;
                        if inside(value(mix_rs(a.0, b.0, mid))) == ia {
                            lo = mid;
                        } else {
                            hi = mid;
                        }
                    }
                    (lo + hi) * 0.5
                }
                None => (level - a.1) / (b.1 - a.1),
            };
            out.push((mix_rs(a.0, b.0, f), level));
        }
    }
    out
}

fn cross_rs(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

struct Surface<'a> {
    nodes: [Vtx; 6],
    paint: Option<(Src<'a>, &'a Scale)>,
    plane: Option<(usize, f64)>,
    gray: u32,
}

impl Surface<'_> {
    fn at(&self, rs: [f64; 2]) -> Vtx {
        quadratic(&self.nodes, rs)
    }

    fn band(&self, rs: [f64; 2]) -> f64 {
        self.paint.map_or(f64::NAN, |(v, s)| {
            let value = v.value64(&self.at(rs).s);
            let value = s.cap.map_or(value, |c| value.min(c as f64));
            // 境を探す間に f32 へ丸めると、等値の台ができて隣の帯が重なる。
            ((value - s.lo as f64) / s.span as f64).clamp(0.0, 1.0) * s.levels as f64
        })
    }

    fn polygon(&self, b: &mut Builder, rs: &[[f64; 2]], face: u32) {
        if rs.len() < 3 {
            return;
        }
        let a = self.at(rs[0]);
        for i in 1..rs.len() - 1 {
            let v = [a, self.at(rs[i]), self.at(rs[i + 1])];
            b.tri_n(v.map(|x| x.at()), v.map(|x| x.n), face);
        }
    }

    fn leaf(&self, b: &mut Builder, rs: [[f64; 2]; 3]) {
        let poly = match self.plane {
            Some((axis, at)) => clip_surface(&rs, &|q| self.at(q).p[axis], at, false),
            None => rs.to_vec(),
        };
        let Some((_, s)) = self.paint else {
            self.polygon(b, &poly, self.gray);
            return;
        };
        if poly.is_empty() {
            return;
        }
        let t: Vec<f64> = poly.iter().map(|q| self.band(*q)).collect();
        if t.iter().any(|v| !v.is_finite()) {
            self.polygon(b, &poly, self.gray);
            return;
        }
        let band = |t: f64| (t.floor().max(0.0) as usize).min(s.levels - 1);
        let first = band(t.iter().copied().fold(f64::INFINITY, f64::min));
        let last = band(t.iter().copied().fold(f64::NEG_INFINITY, f64::max));
        let tagged: Vec<_> = poly.iter().copied().zip(t).collect();
        let pieces = |exact: bool| {
            (first..=last)
                .map(|k| {
                    let value = |q| self.band(q);
                    let value = exact.then_some(&value as &dyn Fn([f64; 2]) -> f64);
                    let mut part = tagged.clone();
                    if k > first {
                        part = clip_tagged(&part, value, k as f64, true);
                    }
                    if k < last {
                        part = clip_tagged(&part, value, (k + 1) as f64, false);
                    }
                    part.into_iter().map(|v| v.0).collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        };
        let mut parts = pieces(true);
        // 曲がった量が小片の中で折り返す時は、根の選び方で帯の多角形が重なり得る。
        // 面積と凸性を保てない小片だけ直線の境へ戻し、穴や重なりを作らない。
        let area = |p: &[[f64; 2]]| -> f64 {
            (1..p.len().saturating_sub(1))
                .map(|i| cross_rs(p[0], p[i], p[i + 1]).abs() * 0.5)
                .sum()
        };
        let convex = |p: &[[f64; 2]]| {
            (0..p.len())
                .all(|i| cross_rs(p[i], p[(i + 1) % p.len()], p[(i + 2) % p.len()]) >= -1e-12)
        };
        let expected = area(&poly);
        let actual: f64 = parts.iter().map(|p| area(p)).sum();
        if (actual - expected).abs() > 1e-10 * expected.max(1e-10)
            || parts.iter().any(|p| !convex(p))
        {
            parts = pieces(false);
        }
        for (k, part) in (first..=last).zip(parts) {
            self.polygon(b, &part, s.face(k));
        }
    }

    /// 元の面の座標だけを 4 分し、試す点の値を毎回 6 節点から求める。
    fn draw(
        &self,
        b: &mut Builder,
        rs: [[f64; 2]; 3],
        depth: u32,
        curved: bool,
        limit: usize,
    ) -> bool {
        if depth > 0 {
            let m = [
                mix_rs(rs[0], rs[1], 0.5),
                mix_rs(rs[1], rs[2], 0.5),
                mix_rs(rs[2], rs[0], 0.5),
            ];
            let c = mix_rs(m[0], rs[2], 1.0 / 3.0);
            let t = rs.map(|q| self.band(q));
            let off = [
                self.band(m[0]) - (t[0] + t[1]) * 0.5,
                self.band(m[1]) - (t[1] + t[2]) * 0.5,
                self.band(m[2]) - (t[2] + t[0]) * 0.5,
                self.band(c) - (t[0] + t[1] + t[2]) / 3.0,
            ];
            if curved || off.iter().any(|d| d.abs() > SPLIT_TOL) {
                for sub in [
                    [rs[0], m[0], m[2]],
                    [m[0], rs[1], m[1]],
                    [m[2], m[1], rs[2]],
                    [m[0], m[1], m[2]],
                ] {
                    if !self.draw(b, sub, depth - 1, false, limit) {
                        return false;
                    }
                }
                return true;
            }
        }
        self.leaf(b, rs);
        b.mesh.triangles.len() <= limit
    }
}

/// 追加の細分化で描く表面の上限。元の面だけで超える時は穴を空けず、全体を描いて知らせる。
const SURFACE_LIMIT: usize = 2_000_000;

fn draw_surfaces(b: &mut Builder, surfaces: &[Surface<'_>], limit: usize) -> u32 {
    let (points, triangles) = (b.mesh.positions.len(), b.mesh.triangles.len());
    for depth in (0..=SPLIT).rev() {
        let mut complete = true;
        for surface in surfaces {
            // 曲がった幾何では、少なくとも辺の中点を通して形と陰を残す。
            let curved = [[0, 1, 3], [1, 2, 4], [2, 0, 5]].iter().any(|[a, z, m]| {
                let p = Vtx::lerp(surface.nodes[*a], surface.nodes[*z], 0.5).at();
                let q = surface.nodes[*m].at();
                (0..3).any(|k| (p[k] - q[k]).abs() > 1e-10)
            });
            let ok = surface.draw(
                b,
                [NODE_RS[0], NODE_RS[1], NODE_RS[2]],
                depth.max(u32::from(curved)),
                curved,
                if depth == 0 { usize::MAX } else { limit },
            );
            if !ok {
                complete = false;
                break;
            }
        }
        if complete {
            if b.mesh.triangles.len() > limit {
                log::warn!(
                    "解析の表面は元の面だけで三角形の上限を超えます: {} / {limit}",
                    b.mesh.triangles.len()
                );
            }
            return depth;
        }
        // 全部の面を同じ上限で描き直す。中断した面以降を欠いた網は使わない。
        b.mesh.positions.truncate(points);
        b.mesh.normals.truncate(points);
        b.mesh.triangles.truncate(triangles);
        b.mesh.triangle_faces.truncate(triangles);
        log::warn!(
            "解析の表面の三角形が上限 {limit} を超えるので、分ける深さを {} に下げます",
            depth - 1
        );
    }
    unreachable!()
}

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

/// 2 次の四面体の網: 表面を 2 次の形関数で塗り、断面があれば `p[axis] > at` の側を取り除いて
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
    let normals = surface_normals(t, |i| vtx(i).at());
    let surfaces: Vec<Surface<'_>> = t
        .faces
        .iter()
        .zip(&normals)
        .map(|(f, n6)| {
            let mut nodes = f.map(vtx);
            for (v, n) in nodes.iter_mut().zip(n6) {
                v.n = *n;
            }
            Surface {
                nodes,
                paint,
                plane,
                gray,
            }
        })
        .collect();
    draw_surfaces(b, &surfaces, SURFACE_LIMIT);
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

#[cfg(test)]
mod tests;
