//! 3D 粗取りと平らな所の仕上げの経路の中身(機能の評価から切り離した計算)。
//!
//! 段の高さ z ごとに、工具の先端が z の時に部品を削らない(残す厚さを残す)工具の中心の領域を
//! [`DropField`] の等高線で求め、ストックの外形の箱(と加工の範囲)との共通部分を、送り幅ずつ
//! 内側へずらした輪でたどる(昔からある等高線ごとのオフセット荒削り)。工具が材料に掛かる幅を
//! 一定に保つような制御や、トロコイド、中心軸を使う渦巻きは入れない。
//!
//! 輪から輪へは、まっすぐ動いても部品を削らず、動く所がこの段で工具の中心が通った所から
//! 送り幅以内(= 工具が掛かるのは送り幅まで。削った所の中)なら下げたまま移る。そうでなければ
//! 上げて、次の輪へ次のどれかで入る。
//! 1. 外側が開いている領域(ストックの縁に届く領域)のいちばん外の輪: ストックの外で下りて横から入る
//! 2. この段で削った所の上: 真下に下りる
//! 3. 入る位置(`entry`)がその領域の中: そこで下りて(螺旋が入れば螺旋で)、輪の始まりへ動く
//! 4. どれでもない: 1 つ上の段の高さから、輪に沿って斜めに下りる(輪が小さければらせん)
//!
//! 取残し(`rest`)は、前の太い工具が段ごとに届いた所(前の工具の中心が下りられる所を前の工具の
//! 半径だけ広げた所)と今の工具が届く所の差だけを削る。前の工具は部品の全体を削ったとみなす。

use jc_cam_dropcutter::region::{Grid, distance2};
use jc_cam_dropcutter::{
    Cutter, DropField, Heights, Region, Resolved, areas_from_loops, dilate, simplify_closed,
};
use jc_cam_geom::{Area, Loop, P2, Vertex, offset_area};
use jc_cam_setup::{EntryKind, Planner, approach, loop_moves, ramp_moves, retract, start_near};
use jc_cam_tool::Tool;
use jc_toolpath::{Move, Toolpath};
use std::collections::HashMap;

/// 経路の計算の設定(式を数にしたもの)。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Settings {
    /// 軸方向(上向き)に残す厚さ
    pub leave: f64,
    /// 径方向に残す厚さ・加工の範囲
    pub limits: Resolved,
    /// 部品の高さを調べる格子の幅
    pub cell: f64,
    /// 段の深さ
    pub stepdown: f64,
    /// 送り幅(mm)
    pub stepover: f64,
    /// 輪に沿って下りる角度(ラジアン)
    pub ramp: f64,
    /// 入る位置
    pub entry: Option<P2>,
    /// 取残し: 前の太い工具
    pub rest: Option<Tool>,
    /// 削る底(工具の先端の高さ)
    pub bottom: f64,
    /// 領域の輪を 1 本の渦巻きにつなぐ
    pub spiral: bool,
    /// 内側の輪の角を丸める半径(0 なら丸めない)
    pub smoothing: f64,
    /// 閉じた所へ下りる方法
    pub entry_kind: EntryKind,
    /// らせんの半径(空なら工具の半径 × [`HELIX`])
    pub helix: Option<f64>,
}

/// 計算の結果の数(試験と警告に使う)。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    pub levels: usize,
    pub loops: usize,
    /// 上げて入り直した数
    pub entries: usize,
    /// ストックの外で下りた数
    pub outside: usize,
    /// 入る位置で下りた数
    pub at_entry: usize,
    /// らせんで、領域のいちばん広い所で下りた数
    pub at_widest: usize,
    pub warnings: Vec<String>,
}

/// 輪に沿って下りる角度の既定(度)。
pub const DEFAULT_RAMP_DEG: f64 = 3.0;

/// 下げたまま移る、つなぎの長さの上限(工具の直径の倍数)。
const STAY_DOWN: f64 = 10.0;

/// 入る位置での螺旋の半径(工具の半径に対する比)。
const HELIX: f64 = 0.5;

/// ストックの外で下りる時、工具の縁をストックから離す距離(mm)。
const OUTSIDE: f64 = 1.0;

/// 点の近さを調べる入れ物(一辺 `size` の升目に分ける)。
struct Buckets {
    size: f64,
    map: HashMap<(i64, i64), Vec<P2>>,
}

impl Buckets {
    fn new(size: f64) -> Buckets {
        Buckets {
            size: size.max(1e-3),
            map: HashMap::new(),
        }
    }

    fn key(&self, p: P2) -> (i64, i64) {
        (
            (p[0] / self.size).floor() as i64,
            (p[1] / self.size).floor() as i64,
        )
    }

    fn add(&mut self, p: P2) {
        let k = self.key(p);
        self.map.entry(k).or_default().push(p);
    }

    /// 線分 `a`-`b` の上に `spacing` ごとに点を足す。
    fn add_segment(&mut self, a: P2, b: P2, spacing: f64) {
        let n = (((b[0] - a[0]).hypot(b[1] - a[1]) / spacing).ceil() as usize).max(1);
        for k in 0..=n {
            let t = k as f64 / n as f64;
            self.add([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
        }
    }

    /// `q` から `d` 以内に点があるか(`d` は升目の大きさ以下)。
    fn near(&self, q: P2, d: f64) -> bool {
        let (ki, kj) = self.key(q);
        let d2 = d * d;
        for dj in -1..=1 {
            for di in -1..=1 {
                if let Some(v) = self.map.get(&(ki + di, kj + dj))
                    && v.iter().any(|p| {
                        let (x, y) = (p[0] - q[0], p[1] - q[1]);
                        x * x + y * y <= d2
                    })
                {
                    return true;
                }
            }
        }
        false
    }
}

/// 領域を内側へ寄せていった輪と、外から数えた番号。送り幅が工具の半径より大きい時は、
/// 送り幅だけ寄せると消える所に小さな輪を足す(ポケットの輪と同じ考え)。`round` が正なら、
/// 2 つ目から内の輪の角を半径 `round` で丸める(`step + round` 寄せて `round` 戻す)。角を
/// 丸めて前の輪から工具が届かなくなる所にも、小さな輪を足す。
pub fn rings(area: &Area, first: f64, step: f64, r: f64, round: f64) -> Vec<(usize, Area)> {
    let mut out = Vec::new();
    let mut cur = offset_area(area, first);
    let mut k = 0;
    while !cur.is_empty() && k < 10_000 {
        let mut next = Vec::new();
        for a in &cur {
            let mut n = offset_area(a, step);
            if round > 0.0 {
                n = rounded(a, step, round, n.len()).unwrap_or(n);
            }
            if step > 0.95 * r || round > 0.0 {
                n.extend(fill_gaps(a, &n, r));
            }
            next.extend(n);
        }
        out.extend(cur.into_iter().map(|a| (k, a)));
        cur = next;
        k += 1;
    }
    out
}

/// `a` を `step` 寄せた輪の角を、半径 `round` で丸めた輪(`step + round` 寄せて `round` 戻す)。
/// 丸めると、半径の 2 倍より狭い所は角だけでなく丸ごと消える(小さなポケットの真ん中の輪が
/// 消え、小さな円の輪だけが残っていた)。寄せただけの輪の数 `want` と同じ数の輪が残るまで、
/// 半径を半分ずつ小さくする。`round` の 1/20 でも残らなければ `None`(丸めない)。
fn rounded(a: &Area, step: f64, round: f64, want: usize) -> Option<Vec<Area>> {
    let mut rho = round;
    while rho > 0.05 * round.max(1e-9) && rho > 1e-3 {
        let n: Vec<Area> = offset_area(a, step + rho)
            .iter()
            .flat_map(|b| offset_area(b, -rho))
            .collect();
        if n.len() == want {
            return Some(n);
        }
        rho *= 0.5;
    }
    None
}

/// 輪 `a` と、その内側の次の輪 `next` のどちらからも工具が届かない所に足す、小さな円の輪。
/// `a` の中を半径の 1/5 の間隔で調べ、届かない点ごとに半径の 1/5 の円を足す。
fn fill_gaps(a: &Area, next: &[Area], r: f64) -> Vec<Area> {
    let spacing = 0.1 * r;
    let mut near = Buckets::new(r);
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for x in std::iter::once(a).chain(next) {
        for l in std::iter::once(&x.outer).chain(&x.holes) {
            let pts = l.flatten(1e-3);
            for k in 0..pts.len() {
                near.add_segment(pts[k], pts[(k + 1) % pts.len()], spacing);
                for i in 0..2 {
                    lo[i] = lo[i].min(pts[k][i]);
                    hi[i] = hi[i].max(pts[k][i]);
                }
            }
        }
    }
    let inside = Region::new(std::slice::from_ref(a));
    let inner = Region::new(next);
    let h = 0.2 * r;
    let rho = 0.2 * r;
    let mut centers: Vec<P2> = Vec::new();
    let (nx, ny) = (
        ((hi[0] - lo[0]) / h) as usize,
        ((hi[1] - lo[1]) / h) as usize,
    );
    for j in 0..=ny {
        for i in 0..=nx {
            let q = [lo[0] + i as f64 * h, lo[1] + j as f64 * h];
            if !inside.contains(q)
                || inner.contains(q)
                || centers
                    .iter()
                    .any(|c| (c[0] - q[0]).hypot(c[1] - q[1]) <= 0.75 * r)
                || near.near(q, r - 0.02 - spacing)
            {
                continue;
            }
            centers.push(q);
        }
    }
    centers
        .into_iter()
        .map(|c| Area {
            outer: Loop {
                verts: vec![
                    Vertex {
                        x: c[0] + rho,
                        y: c[1],
                        bulge: 1.0,
                    },
                    Vertex {
                        x: c[0] - rho,
                        y: c[1],
                        bulge: 1.0,
                    },
                ],
            },
            holes: vec![],
        })
        .collect()
}

/// 領域 `area` の中で、縁からいちばん遠い所(らせんで下りる所)。格子 `cell` より細かくは探さない。
fn widest(area: &Area, cell: f64) -> Option<P2> {
    let edge: Vec<P2> = std::iter::once(&area.outer)
        .chain(&area.holes)
        .flat_map(|l| l.flatten(0.5 * cell))
        .collect();
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for q in &edge {
        for i in 0..2 {
            lo[i] = lo[i].min(q[i]);
            hi[i] = hi[i].max(q[i]);
        }
    }
    let room = |q: P2| {
        edge.iter()
            .map(|e| (e[0] - q[0]).hypot(e[1] - q[1]))
            .fold(f64::INFINITY, f64::min)
    };
    // 粗く 40 × 40 ほどで探してから、いちばん良い所のまわりを細かく探す
    let mut h = ((hi[0] - lo[0]).max(hi[1] - lo[1]) / 40.0).max(cell);
    let mut best: Option<(f64, P2)> = None;
    let look = |a: P2, b: P2, h: f64, best: &mut Option<(f64, P2)>| {
        let (nx, ny) = (((b[0] - a[0]) / h) as usize, ((b[1] - a[1]) / h) as usize);
        for j in 0..=ny {
            for i in 0..=nx {
                let q = [a[0] + i as f64 * h, a[1] + j as f64 * h];
                if !area.contains(q) {
                    continue;
                }
                let d = room(q);
                if best.is_none_or(|b| d > b.0) {
                    *best = Some((d, q));
                }
            }
        }
    };
    look(lo, hi, h, &mut best);
    while h > cell {
        let Some((_, c)) = best else { break };
        let a = [c[0] - h, c[1] - h];
        let b = [c[0] + h, c[1] + h];
        h = (0.25 * h).max(cell);
        look(a, b, h, &mut best);
    }
    best.map(|b| b.1)
}

/// 閉じた輪の点(始めの点を終わりにも置く)。
fn closed(l: &Loop) -> Vec<P2> {
    let mut p = l.flatten(0.01);
    if let Some(&f) = p.first() {
        p.push(f);
    }
    p
}

/// 閉じた点の列の、始めからの長さ。
fn lengths(pts: &[P2]) -> Vec<f64> {
    let mut c = vec![0.0];
    for w in pts.windows(2) {
        let last = *c.last().expect("始めがある");
        c.push(last + (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]));
    }
    c
}

/// 閉じた点の列の、長さの割合 `t`(0〜1)の所。
fn at(pts: &[P2], len: &[f64], t: f64) -> P2 {
    let total = *len.last().expect("点がある");
    let want = t.clamp(0.0, 1.0) * total;
    let k = len.partition_point(|&x| x < want).clamp(1, pts.len() - 1);
    let (a, b) = (pts[k - 1], pts[k]);
    let seg = len[k] - len[k - 1];
    let u = if seg > 1e-12 {
        (want - len[k - 1]) / seg
    } else {
        0.0
    };
    [a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u]
}

/// 輪 `first` から `rest` の輪へ、1 周ずつ長さの割合で混ぜながら移る渦巻きの点(高さは同じ)。
/// 混ぜた周は輪の上を始めの点でしか通らないので、最初と最後の輪は一周そのままなぞる。
/// 混ぜる点は `ds` 以下の間隔で取る(輪の頂点の数だけで取ると、角にしか頂点の無い四角の輪
/// では 1 周が数点の折れ線になる)。
fn spiral(first: &Loop, rest: &[Loop], ds: f64) -> Vec<P2> {
    let mut cur = closed(first);
    let mut out = cur.clone();
    for next in rest {
        let start = cur[0];
        let b = start_near(next, start);
        let b = closed(&b);
        let (ca, cb) = (lengths(&cur), lengths(&b));
        let longest = ca
            .last()
            .copied()
            .unwrap_or(0.0)
            .max(cb.last().copied().unwrap_or(0.0));
        let m = cur
            .len()
            .max(b.len())
            .max(8)
            .max((longest / ds).ceil() as usize);
        for k in 1..=m {
            let t = k as f64 / m as f64;
            let (p, q) = (at(&cur, &ca, t), at(&b, &cb, t));
            out.push([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]);
        }
        cur = b;
    }
    out.extend(cur.iter().skip(1).copied());
    out
}

/// 段ごとに削る輪の 1 つ。
struct Item {
    l: Loop,
    /// 並べる順の鍵(小さい方から、上げて入り直す時に選ぶ)
    key: i64,
    /// 外側が開いている領域のいちばん外の輪(ストックの外から入れる)
    open_outer: bool,
    /// 属する領域の番号(段の中での)
    comp: usize,
    /// 渦巻きにつなぐ続きの輪(`l` の次から順に)。空なら `l` だけを 1 周する
    chain: Vec<Loop>,
}

/// 計算の途中の状態。
struct State<'a> {
    p: &'a Planner,
    s: &'a Settings,
    field: DropField,
    tp: Toolpath,
    /// 工具を下げて削り終えた所(上げてあれば None)
    here: Option<[f64; 3]>,
    /// 外側が開いているとみなす箱(工具の中心の、ストックの外形の箱を広げたもの)
    bx: (P2, P2),
    report: Report,
}

/// 工具の中心が `a` から `b` へ、先端を `zc`(太らせた工具の先端の高さ)に保って動いても
/// 部品を削らないか。
fn clear(field: &DropField, a: P2, b: P2, zc: f64) -> bool {
    let len = (b[0] - a[0]).hypot(b[1] - a[1]);
    let n = (len / (field.heights.cell * 0.5)).ceil().max(1.0) as usize;
    (0..=n).all(|k| {
        let t = k as f64 / n as f64;
        let q = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        field.drop_at(q) <= zc + 1e-6
    })
}

/// 線分の上のどの点も、この段で工具の中心が通った所から `d` 以内か(= 削った所の中)。
fn inside_cut(done: &Buckets, a: P2, b: P2, d: f64) -> bool {
    let len = (b[0] - a[0]).hypot(b[1] - a[1]);
    let n = (len / (0.5 * d)).ceil().max(1.0) as usize;
    (0..=n).all(|k| {
        let t = k as f64 / n as f64;
        done.near([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t], d)
    })
}

/// 輪の上で `q` にいちばん近い点と、その点がある辺の番号。直線の辺は辺の上まで、弧の辺は
/// 端の頂点だけを見る(頂点だけで探すと、角にしか頂点の無い四角の輪では隣の輪から遠い角を
/// 選んでしまい、下げたまま移れずに上げて入り直すことになる)。
fn closest(l: &Loop, q: P2) -> (usize, P2) {
    let n = l.verts.len();
    let d = |a: P2| (a[0] - q[0]).hypot(a[1] - q[1]);
    let mut best = (
        0,
        [
            l.verts.first().map_or(0.0, |v| v.x),
            l.verts.first().map_or(0.0, |v| v.y),
        ],
    );
    for i in 0..n {
        let (a, b) = (l.verts[i], l.verts[(i + 1) % n]);
        let (a, bulge, b) = ([a.x, a.y], a.bulge, [b.x, b.y]);
        let c = if bulge.abs() > 1e-12 {
            a
        } else {
            let (ex, ey) = (b[0] - a[0], b[1] - a[1]);
            let len2 = ex * ex + ey * ey;
            let t = if len2 > 0.0 {
                (((q[0] - a[0]) * ex + (q[1] - a[1]) * ey) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            [a[0] + ex * t, a[1] + ey * t]
        };
        if d(c) < d(best.1) {
            best = (i, c);
        }
    }
    best
}

/// 輪の上で `q` にいちばん近い点。
fn nearest(l: &Loop, q: P2) -> P2 {
    closest(l, q).1
}

/// `q` にいちばん近い点から始めた輪(直線の辺の途中なら、そこに頂点を足す)。
fn start_at(l: &Loop, q: P2) -> Loop {
    let n = l.verts.len();
    if n == 0 {
        return l.clone();
    }
    let (i, c) = closest(l, q);
    let at = |k: usize| (l.verts[k].x - c[0]).hypot(l.verts[k].y - c[1]) < 1e-9;
    let k = if at(i) {
        i
    } else if at((i + 1) % n) {
        (i + 1) % n
    } else {
        let mut verts = vec![Vertex {
            x: c[0],
            y: c[1],
            bulge: 0.0,
        }];
        verts.extend((1..=n).map(|m| l.verts[(i + m) % n]));
        return Loop { verts };
    };
    Loop {
        verts: (0..n).map(|m| l.verts[(k + m) % n]).collect(),
    }
}

/// 箱の辺の上にある、輪のいちばん長い直線の区間の中ほどから始めた輪と、そこへ横から入る
/// ストックの外の点。
fn box_entry(l: &Loop, bx: (P2, P2), tol: f64, outside: (P2, P2)) -> Option<(Loop, P2)> {
    let n = l.verts.len();
    let side = |p: P2| -> Option<usize> {
        let d = [
            (p[0] - bx.0[0]).abs(),
            (bx.1[0] - p[0]).abs(),
            (p[1] - bx.0[1]).abs(),
            (bx.1[1] - p[1]).abs(),
        ];
        (0..4)
            .filter(|&k| d[k] <= tol)
            .min_by(|&a, &b| d[a].total_cmp(&d[b]))
    };
    let mut best: Option<(f64, usize, usize)> = None;
    for k in 0..n {
        let (a, b) = (l.verts[k], l.verts[(k + 1) % n]);
        if a.bulge.abs() > 1e-12 {
            continue;
        }
        let (pa, pb) = ([a.x, a.y], [b.x, b.y]);
        if let (Some(sa), Some(sb)) = (side(pa), side(pb))
            && sa == sb
        {
            let len = (pb[0] - pa[0]).hypot(pb[1] - pa[1]);
            if best.is_none_or(|(l0, _, _)| len > l0) {
                best = Some((len, k, sa));
            }
        }
    }
    let (_, k, s) = best?;
    let (a, b) = (l.verts[k], l.verts[(k + 1) % n]);
    let mid = [(a.x + b.x) / 2.0, (a.y + b.y) / 2.0];
    let e = match s {
        0 => [outside.0[0], mid[1]],
        1 => [outside.1[0], mid[1]],
        2 => [mid[0], outside.0[1]],
        _ => [mid[0], outside.1[1]],
    };
    let start = Vertex {
        x: mid[0],
        y: mid[1],
        bulge: 0.0,
    };
    let lp = Loop {
        verts: std::iter::once(start)
            .chain((1..=n).map(|i| l.verts[(k + i) % n]))
            .collect(),
    };
    Some((lp, e))
}

/// 外周が箱の辺に触れるか(外側が開いている領域か)。
fn touches_box(l: &Loop, bx: (P2, P2), tol: f64) -> bool {
    l.verts.iter().any(|v| {
        (v.x - bx.0[0]).abs() <= tol
            || (bx.1[0] - v.x).abs() <= tol
            || (v.y - bx.0[1]).abs() <= tol
            || (bx.1[1] - v.y).abs() <= tol
    })
}

/// 入る位置 `e` で、高さ `from` から `to` まで下りる動き。`helix` があれば半径 `helix` の螺旋で。
fn plunge_at(
    tp: &mut Toolpath,
    p: &Planner,
    e: P2,
    (from, to): (f64, f64),
    helix: Option<(f64, f64)>,
) {
    let Some((a, rh)) = helix else {
        tp.moves.push(Move::Linear {
            to: [e[0], e[1], to],
            feed: p.cutting.plunge,
        });
        return;
    };
    let drop = from - to;
    let pitch = std::f64::consts::TAU * rh * a.tan();
    let laps = ((drop / pitch.max(1e-6)) - 1e-9).ceil().max(1.0) as usize;
    const N: usize = 24;
    let at = |k: usize, z: f64| {
        let t = k as f64 / N as f64 * std::f64::consts::TAU;
        [e[0] + rh * t.cos(), e[1] + rh * t.sin(), z]
    };
    tp.moves.push(Move::Linear {
        to: at(0, from),
        feed: p.cutting.feed,
    });
    let total = laps * N;
    for k in 1..=total {
        tp.moves.push(Move::Linear {
            to: at(k, from - drop * k as f64 / total as f64),
            feed: p.cutting.feed,
        });
    }
    // 底で 1 周して平らにし、中心へ戻る
    for k in 1..=N {
        tp.moves.push(Move::Linear {
            to: at(k, to),
            feed: p.cutting.feed,
        });
    }
    tp.moves.push(Move::Linear {
        to: [e[0], e[1], to],
        feed: p.cutting.feed,
    });
}

impl State<'_> {
    fn grid(&self) -> Grid {
        self.field.heights.grid()
    }

    /// 1 つの段を削る。`allowed` は工具の中心が動いてよい節点の印、`z_prev` は 1 つ上の段
    /// (そこまでは削れている)、`open` が偽なら外側が開いていても箱の外からは入らない。
    fn level(&mut self, z: f64, z_prev: f64, allowed: &[bool], open: bool) {
        let s = self.s;
        let cell = s.cell;
        let zc = z - s.leave;
        let r = self.p.r();
        let tol = 0.25 * cell;
        let loops: Vec<Vec<P2>> = self
            .field
            .free_loops(zc, allowed, 4.0 * cell)
            .into_iter()
            .map(|l| simplify_closed(&l, tol))
            .collect();
        let areas = areas_from_loops(loops);
        if areas.is_empty() {
            return;
        }
        self.report.levels += 1;
        let first = tol + 0.01;
        let mut items: Vec<Item> = Vec::new();
        for (c, comp) in areas.iter().enumerate() {
            let is_open = open && touches_box(&comp.outer, self.bx, 1.5 * cell + first);
            let all = rings(comp, first, s.stepover, r, s.smoothing);
            // 渦巻きにつなげるのは、どの番号の輪も 1 つで穴の無い領域(輪が分かれない)
            let simple = all.len() >= 2
                && all
                    .iter()
                    .enumerate()
                    .all(|(n, (k, a))| *k == n && a.holes.is_empty());
            if s.spiral && simple {
                // 開いた領域は外から内へ、閉じた領域は内から外へ
                let mut chain: Vec<Loop> = all.into_iter().map(|(_, a)| a.outer).collect();
                let deepest = chain.len() as i64 - 1;
                if !is_open {
                    chain.reverse();
                }
                let l = chain.remove(0);
                items.push(Item {
                    l,
                    key: if is_open { 0 } else { -deepest },
                    open_outer: is_open,
                    comp: c,
                    chain,
                });
                continue;
            }
            for (k, ring) in all {
                // 開いた領域は外から内へ、閉じた領域は内から外へ
                let key = if is_open { k as i64 } else { -(k as i64) };
                items.push(Item {
                    l: ring.outer,
                    key,
                    open_outer: is_open && k == 0,
                    comp: c,
                    chain: Vec::new(),
                });
                items.extend(ring.holes.into_iter().map(|h| Item {
                    l: h,
                    key,
                    open_outer: false,
                    comp: c,
                    chain: Vec::new(),
                }));
            }
        }
        let reach = STAY_DOWN * self.p.tool.diameter;
        let near = s.stepover * 1.05 + 0.5 * cell;
        let mut done = Buckets::new(near);
        let mut entered = vec![false; areas.len()];
        // 前の段の終わりからは上げて入り直す
        self.here = None;
        if let Some(h) = self.tp.moves.last().map(|m| m.to())
            && h[2] < self.tp.safe_z
        {
            self.here = Some(h);
        }
        let mut down_here = false;
        while !items.is_empty() {
            let h = self.here.map(|h| [h[0], h[1]]);
            // 下げたまま移れる輪のうち、いちばん近いもの
            let stay = if down_here {
                let h = h.expect("下げている");
                items
                    .iter()
                    .enumerate()
                    .filter_map(|(n, it)| {
                        let q = nearest(&it.l, h);
                        let d = (q[0] - h[0]).hypot(q[1] - h[1]);
                        (d <= reach
                            && inside_cut(&done, h, q, near)
                            && clear(&self.field, h, q, zc))
                        .then_some((n, q, d))
                    })
                    .min_by(|a, b| a.2.total_cmp(&b.2))
            } else {
                None
            };
            let (l, chain, key, comp) = if let Some((n, q, _)) = stay {
                let it = items.swap_remove(n);
                let (chain, key, comp) = (it.chain.clone(), it.key, it.comp);
                let l = start_at(&it.l, q);
                let st = [l.verts[0].x, l.verts[0].y];
                if (st[0] - h.unwrap()[0]).hypot(st[1] - h.unwrap()[1]) > 1e-9 {
                    self.tp.moves.push(Move::Linear {
                        to: [st[0], st[1], z],
                        feed: self.p.cutting.feed,
                    });
                }
                (l, chain, key, comp)
            } else {
                // 上げて入り直す: 鍵がいちばん小さく、近い輪
                let from = h.unwrap_or([self.p.model.min[0], self.p.model.min[1]]);
                let n = (0..items.len())
                    .min_by(|&a, &b| {
                        let (ia, ib) = (&items[a], &items[b]);
                        ia.key.cmp(&ib.key).then_with(|| {
                            let (qa, qb) = (nearest(&ia.l, from), nearest(&ib.l, from));
                            (qa[0] - from[0])
                                .hypot(qa[1] - from[1])
                                .total_cmp(&(qb[0] - from[0]).hypot(qb[1] - from[1]))
                        })
                    })
                    .expect("輪がある");
                let it = items.swap_remove(n);
                let first_in_comp = !entered[it.comp];
                entered[it.comp] = true;
                let (chain, key, comp) = (it.chain.clone(), it.key, it.comp);
                let l = self.enter(it, (z, z_prev), &done, &areas, first_in_comp);
                (l, chain, key, comp)
            };
            let path = (!chain.is_empty())
                .then(|| spiral(&l, &chain, 2.0 * cell))
                .filter(|p| {
                    // 輪の間を混ぜた所が部品を削らず、領域から出ないこと(出るなら輪のまま)
                    p.windows(2).all(|w| clear(&self.field, w[0], w[1], zc))
                        && p.iter().all(|&q| areas[comp].contains(q))
                });
            match path {
                Some(path) => {
                    for q in &path[1..] {
                        self.tp.moves.push(Move::Linear {
                            to: [q[0], q[1], z],
                            feed: self.p.cutting.feed,
                        });
                    }
                    for w in path.windows(2) {
                        done.add_segment(w[0], w[1], 0.25 * near);
                    }
                    self.report.loops += chain.len() + 1;
                    let e = *path.last().expect("渦巻きは空でない");
                    self.here = Some([e[0], e[1], z]);
                }
                None => {
                    self.tp.moves.extend(loop_moves(&l, z, self.p.cutting.feed));
                    let pts = l.flatten(0.01);
                    for k in 0..pts.len() {
                        done.add_segment(pts[k], pts[(k + 1) % pts.len()], 0.25 * near);
                    }
                    self.report.loops += 1;
                    self.here = Some([l.verts[0].x, l.verts[0].y, z]);
                    // 渦巻きにできなければ、続きの輪はふつうの輪として後で削る
                    items.extend(chain.into_iter().enumerate().map(|(n, l)| Item {
                        l,
                        key: key + n as i64 + 1,
                        open_outer: false,
                        comp,
                        chain: Vec::new(),
                    }));
                }
            }
            down_here = true;
        }
    }

    /// 上げて輪 `it` に入る。輪の始まりに工具を置いた状態(高さ z)にして、その輪を返す。
    fn enter(
        &mut self,
        it: Item,
        (z, z_prev): (f64, f64),
        done: &Buckets,
        areas: &[Area],
        first_in_comp: bool,
    ) -> Loop {
        let p = self.p;
        let s = self.s;
        let zc = z - s.leave;
        let r = p.r();
        let (feed, plunge) = (p.cutting.feed, p.cutting.plunge);
        self.report.entries += 1;
        let from_here = self.here.map(|h| [h[0], h[1]]).unwrap_or([0.0, 0.0]);
        // 1. 開いた領域のいちばん外の輪: ストックの外で下りて横から入る
        if it.open_outer {
            let (smin, smax) = (p.info.stock_min, p.info.stock_max);
            let out = (
                [smin[0] - r - OUTSIDE, smin[1] - r - OUTSIDE],
                [smax[0] + r + OUTSIDE, smax[1] + r + OUTSIDE],
            );
            if let Some((l, e)) = box_entry(&it.l, self.bx, 1.5 * s.cell + 0.5, out) {
                let st = [l.verts[0].x, l.verts[0].y];
                if clear(&self.field, e, st, zc) {
                    approach(&mut self.tp, e);
                    self.tp.moves.push(Move::Linear {
                        to: [e[0], e[1], z],
                        feed: plunge,
                    });
                    self.tp.moves.push(Move::Linear {
                        to: [st[0], st[1], z],
                        feed,
                    });
                    self.report.outside += 1;
                    return l;
                }
            }
        }
        let l = start_at(&it.l, from_here);
        let st = [l.verts[0].x, l.verts[0].y];
        // 2. この段で削った所の上なら、真下に下りる
        if done.near(st, 0.25 * s.stepover) {
            approach(&mut self.tp, st);
            self.tp.moves.push(Move::Linear {
                to: [st[0], st[1], z],
                feed: plunge,
            });
            return l;
        }
        let from = z_prev.min(self.tp.retract_z).max(z);
        // 3. 入る位置がこの領域の中なら、そこで下りる。らせんを選んでいて入る位置が無ければ、
        //    領域のいちばん広い所で下りる
        let area = &areas[it.comp];
        let at_entry = s
            .entry
            .filter(|&e| s.entry_kind != EntryKind::Ramp && area.contains(e));
        let widest = || {
            (s.entry_kind == EntryKind::Helix)
                .then(|| widest(area, s.cell))
                .flatten()
        };
        if first_in_comp
            && let Some((e, auto)) = at_entry
                .map(|e| (e, false))
                .or_else(|| widest().map(|e| (e, true)))
        {
            let l = start_near(&it.l, e);
            let st = [l.verts[0].x, l.verts[0].y];
            if clear(&self.field, e, st, zc) {
                let fits_r = |rh: f64| {
                    (0..24).all(|k| {
                        let t = k as f64 / 24.0 * std::f64::consts::TAU;
                        let q = [e[0] + rh * t.cos(), e[1] + rh * t.sin()];
                        area.contains(q) && clear(&self.field, e, q, zc)
                    })
                };
                // 入らなければ、半分ずつ小さくする(工具の半径の 1/10 まで)
                let mut rh = s.helix.unwrap_or(HELIX * r);
                while !fits_r(rh) && rh > 0.1 * r {
                    rh *= 0.5;
                }
                let fits = fits_r(rh);
                // 自動で選んだ所でらせんが入らなければ、真下には下りず輪に沿って斜めに下りる
                if auto && !fits {
                    return self.ramp_in(it, (z, z_prev), from_here);
                }
                approach(&mut self.tp, e);
                if from < self.tp.retract_z {
                    self.tp.moves.push(Move::Linear {
                        to: [e[0], e[1], from],
                        feed,
                    });
                }
                plunge_at(&mut self.tp, p, e, (from, z), fits.then_some((s.ramp, rh)));
                self.tp.moves.push(Move::Linear {
                    to: [st[0], st[1], z],
                    feed,
                });
                if auto {
                    self.report.at_widest += 1;
                } else {
                    self.report.at_entry += 1;
                }
                return l;
            }
        }
        self.ramp_in(it, (z, z_prev), from_here)
    }

    /// 4. 1 つ上の段の高さから、輪に沿って斜めに下りる。
    fn ramp_in(&mut self, it: Item, (z, z_prev): (f64, f64), from_here: P2) -> Loop {
        let (s, feed, plunge) = (self.s, self.p.cutting.feed, self.p.cutting.plunge);
        let l = start_near(&it.l, from_here);
        let st = [l.verts[0].x, l.verts[0].y];
        let from = z_prev.min(self.tp.retract_z).max(z);
        approach(&mut self.tp, st);
        if from < self.tp.retract_z {
            self.tp.moves.push(Move::Linear {
                to: [st[0], st[1], from],
                feed,
            });
        }
        self.tp
            .moves
            .extend(ramp_moves(&l, from, z, s.ramp, feed, plunge));
        l
    }
}

/// 格子と、太らせた工具を下ろした高さ。ストックの外形の箱 + 工具の半径の外まで覆う。
fn field_for(p: &Planner, s: &Settings, tool: &Tool) -> DropField {
    let (smin, smax) = (p.info.stock_min, p.info.stock_max);
    let (m0, m1) = (p.model.min, p.model.max);
    let pad = tool.radius() + s.limits.leave_radial + 3.0 * s.cell;
    let lo = [smin[0].min(m0[0]) - pad, smin[1].min(m0[1]) - pad];
    let hi = [smax[0].max(m1[0]) + pad, smax[1].max(m1[1]) + pad];
    let heights = Heights::new(
        lo,
        hi,
        s.cell,
        smin[2].min(s.bottom) - 1.0,
        &p.model.triangles,
    );
    DropField::new(heights, Cutter::new(tool.clone(), s.limits.leave_radial))
}

fn new_state<'a>(p: &'a Planner, s: &'a Settings) -> State<'a> {
    let field = field_for(p, s, &p.tool);
    // 外側が開いているとみなす箱: 工具の縁がストックの縁に少し掛かる所(部品とストックの縁の間が
    // 工具の半径より狭くても、そこを削れるように、箱は工具の半径近くまで外へ広げる)
    let g = p.r() - (0.25 * s.stepover).min(0.5);
    let (smin, smax) = (p.info.stock_min, p.info.stock_max);
    let bx = ([smin[0] - g, smin[1] - g], [smax[0] + g, smax[1] + g]);
    State {
        p,
        s,
        field,
        tp: Toolpath {
            name: String::new(),
            tool: p.tool.clone(),
            rpm: p.cutting.rpm,
            safe_z: p.info.safe_z,
            retract_z: p.info.retract_z,
            edge_break: 0.0,
            spans: vec![],
            moves: Vec::new(),
            dwells: vec![],
            links: Default::default(),
        },
        here: None,
        bx,
        report: Report::default(),
    }
}

/// 節点ごとの、箱と加工の範囲の中かの印。
fn box_mask(st: &State<'_>) -> Vec<bool> {
    let g = st.grid();
    let (lo, hi) = st.bx;
    let region = st.s.limits.region.as_ref().map(|r| r.mask(&g));
    (0..g.nx * g.ny)
        .map(|k| {
            let q = g.at(k % g.nx, k / g.nx);
            q[0] >= lo[0] - 1e-9
                && q[0] <= hi[0] + 1e-9
                && q[1] >= lo[1] - 1e-9
                && q[1] <= hi[1] + 1e-9
                && region.as_ref().is_none_or(|m| m[k])
        })
        .collect()
}

/// 段の高さ(上から順)。ストックの上面から底まで段の深さで等分し、上を向いた平らな面の
/// 高さ(+ 残す厚さ)にも段を足す(平らな所に段の深さまでの厚さが残らないように)。
pub fn levels(p: &Planner, s: &Settings) -> Vec<f64> {
    let top = p.top(p.info.stock_max[2]);
    let n = (((top - s.bottom) / s.stepdown) - 1e-9).ceil().max(1.0) as usize;
    let mut zs: Vec<f64> = (1..=n)
        .map(|k| top - (top - s.bottom) * k as f64 / n as f64)
        .collect();
    for f in p.model.flats.iter().filter(|f| f.up) {
        let z = f.z + s.leave;
        if z > s.bottom + 0.05 && z < top - 0.05 && !zs.iter().any(|&w| (w - z).abs() < 0.05) {
            zs.push(z);
        }
    }
    zs.sort_by(|a, b| b.total_cmp(a));
    zs
}

/// 3D 粗取りの経路(名前は空。呼ぶ側が付ける)。
pub fn rough(p: &Planner, s: &Settings) -> (Toolpath, Report) {
    let mut st = new_state(p, s);
    let bmask = box_mask(&st);
    let g = st.grid();
    let prev = s.rest.as_ref().map(|t| field_for(p, s, t));
    let mut z_prev = p.top(p.info.stock_max[2]);
    for z in levels(p, s) {
        let zc = z - s.leave;
        let allowed = match &prev {
            None => bmask.clone(),
            Some(pf) => rest_mask(&st.field, pf, &bmask, &g, zc),
        };
        st.level(z, z_prev, &allowed, true);
        z_prev = z;
    }
    retract(&mut st.tp);
    (st.tp, st.report)
}

/// 取残しで工具の中心が動く所の印: 今の工具が届く所のうち前の工具が届かなかった所に、
/// 今の工具が掛かる中心の所。
fn rest_mask(cur: &DropField, prev: &DropField, bmask: &[bool], g: &Grid, zc: f64) -> Vec<bool> {
    let (nx, ny) = (g.nx, g.ny);
    let free = |f: &DropField| -> Vec<bool> {
        (0..nx * ny)
            .map(|k| bmask[k] && f.node(k % nx, k / nx) <= zc + 1e-6)
            .collect()
    };
    let (fc, fp) = (free(cur), free(prev));
    let rc = cur.tool.radius() / g.cell;
    let rp = prev.tool.radius() / g.cell;
    let cut_cur = dilate(&fc, nx, ny, rc);
    let cut_prev = dilate(&fp, nx, ny, rp);
    // 前の工具が削った所から 2 節点より離れた所だけを削り残しとみなす(格子の誤差の細い帯を除く)
    let d_prev = distance2(&cut_prev, nx, ny);
    let left: Vec<bool> = (0..nx * ny)
        .map(|k| cut_cur[k] && d_prev[k] > 4.0)
        .collect();
    let touch = dilate(&left, nx, ny, rc + 1.0);
    (0..nx * ny).map(|k| fc[k] && touch[k]).collect()
}

/// 平らな所の仕上げの経路。上を向いた平らな面ごとに、その高さ(+ 残す厚さ)で面の中を輪で削る。
/// `faces` が空でなければその名前の面だけ。
pub fn flats(p: &Planner, s: &Settings, faces: &[String]) -> (Toolpath, Report) {
    let mut st = new_state(p, s);
    let g = st.grid();
    let region = s.limits.region.as_ref().map(|r| r.mask(&g));
    let top = p.top(p.info.stock_max[2]);
    let mut heights: Vec<f64> = Vec::new();
    let chosen: Vec<_> = p
        .model
        .flats
        .iter()
        .filter(|f| f.up && f.z >= s.bottom - 1e-6 && f.z + s.leave < top - 1e-6)
        .filter(|f| faces.is_empty() || faces.contains(&f.name))
        .collect();
    for f in &chosen {
        if !heights.iter().any(|&h| (h - f.z).abs() < 1e-3) {
            heights.push(f.z);
        }
    }
    heights.sort_by(|a, b| b.total_cmp(a));
    for zf in heights {
        let areas: Vec<Area> = chosen
            .iter()
            .filter(|f| (f.z - zf).abs() < 1e-3)
            .map(|f| f.area.clone())
            .collect();
        let mask = Region::new(&areas).mask(&g);
        let allowed: Vec<bool> = (0..g.nx * g.ny)
            .map(|k| mask[k] && region.as_ref().is_none_or(|m| m[k]))
            .collect();
        let z = zf + s.leave;
        st.level(z, (z + p.cutting.stepdown).min(top), &allowed, false);
    }
    retract(&mut st.tp);
    (st.tp, st.report)
}
