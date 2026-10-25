//! 前の加工で削った所の空振りを省く(加工の欄「削った所の空振りを省く」)。
//!
//! セットアップの加工を順に、ストックの高さの格子で削っていき、各加工の経路のうち工具が材料に
//! 触れない切削(空振り)を、上げて移る動きに置き換える。裏返す前のセットアップで削った形も、
//! シミュレーションと同じ引き継ぎ方([`crate::carried_stock`])で始めの形にする。
//!
//! 安全の考え方:
//! - 格子は材料の高さの上限を持つ。セル全体が工具の中に入った時だけ、そのセルを削れたとする
//!   (セルの中でいちばん工具の縁に近い所の刃の高さまで)。前の加工が実際より多く削ったと
//!   見なすことはない。
//! - 触れたかは、工具の半径と高さに [`MARGIN`] の余裕を足して調べる。
//! - 省く区間の始めと終わりは、どちらも工具が材料に触れない点。高さの格子なので、その真上は
//!   空いている。上げる高さは通り道の材料の上限 + 安全高さの余裕(退避高さより下げない)で、
//!   固定具の避ける範囲が越えられなければ省かない。下りるのは早送りで退避高さまで、その下は
//!   切削の送り(元の経路と同じ決まり)。
//! - 前の加工を外して書き出す時と並べ替える時は、元の経路から省き直す([`SetupPaths::only`]・
//!   [`SetupPaths::by_tool`])。
//!
//! 取り代を場所ごとに変えることはしない(docs/避ける形.md)。

use crate::SetupPaths;
use jc_cam_tool::Tool;
use jc_sim::ZMap;
use jc_toolpath::{Link, LinkKind, Move, P3, Role, Toolpath};
use std::sync::Arc;

/// 材料に触れたとみなす余裕(mm)。機械と格子の誤差の分だけ工具を太く低く見る。
pub const MARGIN: f64 = 0.1;
/// 省く空振りの長さの下限(mm)。これより短ければ、上げ下げの方が遅いので残す。
pub const MIN_RUN: f64 = 5.0;
/// 時間の見積もりに使う早送りの速さ(mm/min)。機械の値を知らないので遅めに見る。
const RAPID: f64 = 3000.0;

/// 格子の大きさ(mm)。ストックの長い辺を 800 に分け、0.2〜1 mm に収める。
pub fn cell_for(s: &SetupPaths) -> f64 {
    let i = &s.info;
    let size = (i.stock_max[0] - i.stock_min[0]).max(i.stock_max[1] - i.stock_min[1]);
    (size / 800.0).clamp(0.2, 1.0)
}

/// セル `(i, j)` の中心と、中心からセルの角までの距離。
fn half_diag(map: &ZMap) -> f64 {
    map.cell * std::f64::consts::FRAC_1_SQRT_2
}

/// 工具の先端 `tip` で、セル全体が工具の中に入ったセルを削る(高さの上限を保つ)。
fn stamp_at(map: &mut ZMap, tool: &Tool, tip: P3, bottom: f64) {
    let r = tool.radius();
    let hd = half_diag(map);
    if r < hd {
        return;
    }
    // セルの中心が軸から r - hd より遠ければ、角が工具の外に出る(根を取る前に 2 乗で比べる)
    let inside = (r - hd) * (r - hd);
    let reach = (r / map.cell).ceil() as isize + 1;
    let ci = ((tip[0] - map.origin[0]) / map.cell - 0.5).round() as isize;
    let cj = ((tip[1] - map.origin[1]) / map.cell - 0.5).round() as isize;
    for j in (cj - reach).max(0)..=(cj + reach).min(map.ny as isize - 1) {
        let cy = map.origin[1] + (j as f64 + 0.5) * map.cell - tip[1];
        for i in (ci - reach).max(0)..=(ci + reach).min(map.nx as isize - 1) {
            let cx = map.origin[0] + (i as f64 + 0.5) * map.cell - tip[0];
            let d2 = cx * cx + cy * cy;
            if d2 > inside {
                continue;
            }
            let far = d2.sqrt() + hd;
            if far > r {
                continue;
            }
            let Some(h) = tool.height_at(far) else {
                continue;
            };
            let k = j as usize * map.nx + i as usize;
            let zt = (tip[2] + h).max(bottom);
            if zt < map.z[k] {
                map.z[k] = zt;
            }
        }
    }
}

/// 工具の先端が `tip` の時に、材料に触れるか(余裕 [`MARGIN`] を足して)。
fn touches(map: &ZMap, tool: &Tool, tip: P3) -> bool {
    let r = tool.radius();
    let hd = half_diag(map);
    let rad = r + MARGIN + hd;
    let reach = (rad / map.cell).ceil() as isize + 1;
    let ci = ((tip[0] - map.origin[0]) / map.cell - 0.5).round() as isize;
    let cj = ((tip[1] - map.origin[1]) / map.cell - 0.5).round() as isize;
    let h_edge = tool.height_at(r).unwrap_or(0.0);
    // セルの中心が軸から rad より遠ければ、セルのどこも余裕の内に入らない(根を取る前に 2 乗で比べる)
    let rad2 = rad * rad;
    for j in (cj - reach).max(0)..=(cj + reach).min(map.ny as isize - 1) {
        let cy = map.origin[1] + (j as f64 + 0.5) * map.cell - tip[1];
        for i in (ci - reach).max(0)..=(ci + reach).min(map.nx as isize - 1) {
            let cx = map.origin[0] + (i as f64 + 0.5) * map.cell - tip[0];
            let d2 = cx * cx + cy * cy;
            if d2 > rad2 {
                continue;
            }
            let near = (d2.sqrt() - hd).max(0.0);
            if near > r + MARGIN {
                continue;
            }
            // セルの中でいちばん工具の軸に近い所の刃の高さ(余裕の分だけ内へ・下へ)
            let d = (near - MARGIN).max(0.0);
            let h = tool.height_at(d.min(r)).unwrap_or(h_edge);
            if map.z[j as usize * map.nx + i as usize] > tip[2] + h - MARGIN {
                return true;
            }
        }
    }
    false
}

/// 線分 `a`–`b` から `rad` より近いセルの、材料の高さの上限のいちばん高い所。
fn max_near(map: &ZMap, a: [f64; 2], b: [f64; 2], rad: f64) -> f64 {
    let rad = rad + half_diag(map);
    let lo = [a[0].min(b[0]) - rad, a[1].min(b[1]) - rad];
    let hi = [a[0].max(b[0]) + rad, a[1].max(b[1]) + rad];
    let ix = |x: f64, o: f64, n: usize| (((x - o) / map.cell).floor().max(0.0) as usize).min(n - 1);
    let (i0, i1) = (
        ix(lo[0], map.origin[0], map.nx),
        ix(hi[0], map.origin[0], map.nx),
    );
    let (j0, j1) = (
        ix(lo[1], map.origin[1], map.ny),
        ix(hi[1], map.origin[1], map.ny),
    );
    let mut top = f64::NEG_INFINITY;
    for j in j0..=j1 {
        for i in i0..=i1 {
            let c = map.center(i, j);
            if seg_dist(c, a, b) <= rad {
                top = top.max(map.z[j * map.nx + i]);
            }
        }
    }
    top
}

fn seg_dist(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (p[0] - a[0] - dx * t).hypot(p[1] - a[1] - dy * t)
}

fn dist(a: P3, b: P3) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// 調べる点の間隔。
fn step_of(map: &ZMap, tool: &Tool) -> f64 {
    (map.cell / 2.0).min(tool.radius() / 4.0).max(0.02)
}

/// 経路 `tp` で格子を削る(切削の動きだけ)。
pub fn stamp_path(map: &mut ZMap, tp: &Toolpath, bottom: f64) {
    let step = step_of(map, &tp.tool);
    let mut cur: Option<P3> = None;
    for m in &tp.moves {
        if let (Some(from), Move::Linear { .. } | Move::Arc { .. }) = (cur, m) {
            for q in m.points(from, step) {
                stamp_at(map, &tp.tool, q, bottom);
            }
        }
        cur = Some(m.to());
    }
}

/// 隣の 1 セルまでのいちばん高い所(引き継いだ形の、格子のずれの分を上限に含める)。
pub fn dilate(map: &ZMap) -> ZMap {
    let mut out = map.clone();
    for j in 0..map.ny {
        for i in 0..map.nx {
            let mut m = f64::NEG_INFINITY;
            for jj in j.saturating_sub(1)..=(j + 1).min(map.ny - 1) {
                for ii in i.saturating_sub(1)..=(i + 1).min(map.nx - 1) {
                    m = m.max(map.z[jj * map.nx + ii]);
                }
            }
            out.z[j * map.nx + i] = m;
        }
    }
    out
}

/// 動きを分けた 1 片。
#[derive(Debug, Clone, Copy)]
struct Piece {
    m: Move,
    /// 元の動きの番号
    origin: usize,
    /// 切削で、材料に触れない
    air: bool,
    len: f64,
}

/// 経路を、材料に触れない切削の片と、それ以外の片に分ける。直線の途中の長い空振りは分ける。
fn pieces(tp: &Toolpath, map: &ZMap) -> Vec<Piece> {
    let tool = &tp.tool;
    let step = step_of(map, tool);
    let mut out = Vec::with_capacity(tp.moves.len());
    let mut cur: Option<P3> = None;
    for (k, m) in tp.moves.iter().enumerate() {
        let Some(from) = cur else {
            out.push(Piece {
                m: *m,
                origin: k,
                air: false,
                len: 0.0,
            });
            cur = Some(m.to());
            continue;
        };
        cur = Some(m.to());
        match *m {
            Move::Rapid { to } => out.push(Piece {
                m: *m,
                origin: k,
                air: false,
                len: dist(from, to),
            }),
            Move::Arc { .. } => {
                let pts = m.points(from, step);
                let len = std::iter::once(from)
                    .chain(pts.iter().copied())
                    .collect::<Vec<_>>()
                    .windows(2)
                    .map(|w| dist(w[0], w[1]))
                    .sum();
                let air = !touches(map, tool, from) && pts.iter().all(|q| !touches(map, tool, *q));
                out.push(Piece {
                    m: *m,
                    origin: k,
                    air,
                    len,
                });
            }
            Move::Linear { to, feed } => {
                let pts: Vec<P3> = std::iter::once(from).chain(m.points(from, step)).collect();
                let flags: Vec<bool> = pts.iter().map(|q| !touches(map, tool, *q)).collect();
                let len = dist(from, to);
                if flags.iter().all(|f| *f) || flags.iter().all(|f| !*f) {
                    out.push(Piece {
                        m: *m,
                        origin: k,
                        air: flags[0],
                        len,
                    });
                    continue;
                }
                // 続く空振りのうち、省く長さ以上のものだけを分ける(分けた片の端は触れない点)
                let mut cuts: Vec<(usize, usize)> = Vec::new();
                let mut s = 0;
                while s < flags.len() {
                    if !flags[s] {
                        s += 1;
                        continue;
                    }
                    let mut e = s;
                    while e + 1 < flags.len() && flags[e + 1] {
                        e += 1;
                    }
                    if dist(pts[s], pts[e]) >= MIN_RUN {
                        cuts.push((s, e));
                    }
                    s = e + 1;
                }
                if cuts.is_empty() {
                    out.push(Piece {
                        m: *m,
                        origin: k,
                        air: false,
                        len,
                    });
                    continue;
                }
                let mut at = 0;
                for (s, e) in cuts {
                    if s > at {
                        out.push(Piece {
                            m: Move::Linear { to: pts[s], feed },
                            origin: k,
                            air: false,
                            len: dist(pts[at], pts[s]),
                        });
                    }
                    if e > s {
                        out.push(Piece {
                            m: Move::Linear { to: pts[e], feed },
                            origin: k,
                            air: true,
                            len: dist(pts[s], pts[e]),
                        });
                    }
                    at = e;
                }
                if at + 1 < pts.len() {
                    out.push(Piece {
                        m: Move::Linear { to, feed },
                        origin: k,
                        air: false,
                        len: dist(pts[at], to),
                    });
                }
            }
        }
    }
    out
}

fn feed_of(m: &Move) -> f64 {
    match *m {
        Move::Linear { feed, .. } | Move::Arc { feed, .. } => feed,
        Move::Rapid { .. } => RAPID,
    }
}

/// 経路 `tp` の空振りを省いたもの(省く所がなければ `None`)と、省いた区間の数と長さ。
/// `map` はこの加工の前の材料の高さの上限、`clear` は移る時に材料の上に空ける余裕(加工の送り高さと
/// ストックの上面の差)、
/// `keep_out` は固定具の避ける範囲の箱。
pub fn skip_air(
    tp: &Toolpath,
    map: &ZMap,
    clear: f64,
    keep_out: &[(P3, P3)],
) -> Option<(Toolpath, usize, f64)> {
    // 歯が首より太い工具(ねじ切りミル)は、空振りの所から真上へ上げると切った溝の山に当たる。
    // 格子は材料の高さの上限しか持たず、真上が空いているとは言えないので省かない
    if tp.tool.undercut() {
        return None;
    }
    let ps = pieces(tp, map);
    if !ps.iter().any(|p| p.air) {
        return None;
    }
    let n = tp.moves.len();
    let mut starts: Vec<Option<usize>> = vec![None; n + 1];
    let mut out: Vec<Move> = Vec::with_capacity(ps.len());
    let mut new_links: Vec<Link> = Vec::new();
    let mut new_spans: Vec<(usize, usize)> = Vec::new();
    // 置き換えた元の動きの範囲(そこに掛かっていた前のつなぎの記録は捨てる)
    let mut replaced: Vec<[usize; 2]> = Vec::new();
    let (mut runs, mut skipped) = (0usize, 0.0f64);
    let r = tp.tool.radius();
    let wide = [
        r,
        tp.tool.shank() / 2.0,
        tp.tool.holder_diameter.unwrap_or(0.0) / 2.0,
    ]
    .into_iter()
    .fold(0.0, f64::max);
    let mut cur: P3 = ps.first().map_or([0.0; 3], |p| p.m.to());
    let mut k = 0;
    while k < ps.len() {
        if !ps[k].air {
            starts[ps[k].origin].get_or_insert(out.len());
            out.push(ps[k].m);
            cur = ps[k].m.to();
            k += 1;
            continue;
        }
        // 空振りの間と後の早送り(元のつなぎ)も一緒に置き換える。置き換えの高さは、その早送りの
        // 高さより下げない(退避方法の「完全退避」などを変えない)
        let rapid = |p: &Piece| matches!(p.m, Move::Rapid { .. });
        let mut e = k;
        while e + 1 < ps.len() && (ps[e + 1].air || rapid(&ps[e + 1])) {
            e += 1;
        }
        let run = &ps[k..=e];
        let floor = run
            .iter()
            .filter(|p| rapid(p))
            .map(|p| p.m.to()[2])
            .fold(f64::NEG_INFINITY, f64::max);
        let len: f64 = run.iter().map(|p| p.len).sum();
        let t_run: f64 = run.iter().map(|p| p.len / feed_of(&p.m)).sum();
        let feed = run
            .iter()
            .map(|p| feed_of(&p.m))
            .fold(f64::INFINITY, f64::min);
        let (a, b) = (cur, ps[e].m.to());
        let plan = (len >= MIN_RUN)
            .then(|| plan_link(tp, map, (a, b), floor, clear, keep_out, wide, feed))
            .flatten()
            .filter(|(_, t)| *t < 0.8 * t_run);
        match plan {
            None => {
                for p in run {
                    starts[p.origin].get_or_insert(out.len());
                    out.push(p.m);
                }
            }
            Some((moves, _)) => {
                for p in run {
                    starts[p.origin].get_or_insert(out.len());
                }
                let s = out.len();
                let (kind, z) = link_kind(tp, a, &moves);
                out.extend(moves);
                new_links.push(Link {
                    start: s,
                    end: out.len(),
                    kind,
                    z,
                });
                if kind == LinkKind::StayDown {
                    new_spans.push((s, out.len()));
                }
                replaced.push([run[0].origin, run[run.len() - 1].origin + 1]);
                runs += 1;
                skipped += len;
            }
        }
        cur = b;
        k = e + 1;
    }
    if runs == 0 {
        return None;
    }
    // 元の動きの新しい番号(全部が置き換わった動きは、置き換えの始め)
    let mut idx = vec![0usize; n + 1];
    idx[n] = out.len();
    for i in (0..n).rev() {
        idx[i] = starts[i].unwrap_or(idx[i + 1]);
    }
    let mut t = tp.clone();
    t.moves = out;
    t.links
        .links
        .retain(|l| !replaced.iter().any(|r| l.start < r[1] && r[0] < l.end));
    t.remap_spans(&idx);
    t.links.remap(|i| idx[i.min(n)]);
    t.links.links.retain(|l| l.end > l.start);
    t.links.links.extend(new_links);
    t.links.links.sort_by_key(|l| l.start);
    for (s, e) in new_spans {
        t.spans.push(jc_toolpath::Span {
            start: s,
            end: e,
            role: Role::Link,
        });
    }
    Some((t, runs, skipped))
}

/// 置き換えたつなぎの種類と横に動く高さ。
fn link_kind(tp: &Toolpath, a: P3, moves: &[Move]) -> (LinkKind, f64) {
    match moves {
        [Move::Linear { to, .. }] => (LinkKind::StayDown, to[2].max(a[2])),
        _ => {
            let z = moves
                .iter()
                .map(|m| m.to()[2])
                .fold(f64::NEG_INFINITY, f64::max);
            if z >= tp.safe_z - 1e-6 {
                (LinkKind::Full, z)
            } else {
                (LinkKind::Low, z)
            }
        }
    }
}

/// 空振りの区間 `a`→`b` の代わりの動きと、その時間の見積もり(分)。区間に早送りがあれば、
/// 横に移る高さは `floor` より下げない。上げる高さが固定具を越えられなければ `None`。
#[allow(clippy::too_many_arguments)]
fn plan_link(
    tp: &Toolpath,
    map: &ZMap,
    (a, b): (P3, P3),
    floor: f64,
    clear: f64,
    keep_out: &[(P3, P3)],
    wide: f64,
    feed: f64,
) -> Option<(Vec<Move>, f64)> {
    let (a2, b2) = ([a[0], a[1]], [b[0], b[1]]);
    let horiz = (b[0] - a[0]).hypot(b[1] - a[1]);
    // 同じ高さで、まっすぐ移っても材料に触れなければ、下げたまま送りで移れる(上げて
    // 早送りで移る方が速ければ、そちら)
    let mut stay: Option<(Vec<Move>, f64)> = None;
    if (a[2] - b[2]).abs() < 1e-9 && floor == f64::NEG_INFINITY {
        let n = ((horiz / step_of(map, &tp.tool)).ceil() as usize).max(1);
        let clear_line = (0..=n).all(|i| {
            let t = i as f64 / n as f64;
            let q = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2]];
            !touches(map, &tp.tool, q)
        });
        let fixtures = keep_out
            .iter()
            .any(|z| z.1[2] + 1.0 > a[2] && near_box(z, a2, b2, wide));
        if clear_line && !fixtures {
            stay = Some((vec![Move::Linear { to: b, feed }], horiz / feed));
        }
    }
    let hop = hop_link(tp, map, (a, b), floor, clear, keep_out, wide, feed);
    match (stay, hop) {
        (Some(s), Some(h)) => Some(if h.1 < s.1 { h } else { s }),
        (s, h) => s.or(h),
    }
}

/// 上げて早送りで移り、退避高さから送りで下りる動きと、その時間の見積もり(分)。
#[allow(clippy::too_many_arguments)]
fn hop_link(
    tp: &Toolpath,
    map: &ZMap,
    (a, b): (P3, P3),
    floor: f64,
    clear: f64,
    keep_out: &[(P3, P3)],
    wide: f64,
    feed: f64,
) -> Option<(Vec<Move>, f64)> {
    let r = tp.tool.radius();
    let (a2, b2) = ([a[0], a[1]], [b[0], b[1]]);
    let horiz = (b[0] - a[0]).hypot(b[1] - a[1]);
    let mut z = (max_near(map, a2, b2, r + MARGIN) + clear)
        .max(tp.retract_z)
        .max(a[2])
        .max(b[2])
        .max(floor);
    for k in keep_out.iter().filter(|k| near_box(k, a2, b2, wide)) {
        z = z.max(k.1[2] + 1.0);
    }
    if z > tp.safe_z + 1e-6 {
        return None;
    }
    if z > tp.safe_z - 1e-6 {
        z = tp.safe_z;
    }
    let mut moves = Vec::new();
    if z > a[2] + 1e-9 {
        moves.push(Move::Rapid {
            to: [a[0], a[1], z],
        });
    }
    moves.push(Move::Rapid {
        to: [b[0], b[1], z],
    });
    // 早送りで下りるのは退避高さまで(元の経路と同じ)。その下は切削の送り
    let down = tp.retract_z.max(b[2]).min(z);
    if down < z - 1e-9 {
        moves.push(Move::Rapid {
            to: [b[0], b[1], down],
        });
    }
    if b[2] < down - 1e-9 {
        moves.push(Move::Linear { to: b, feed });
    }
    let t = ((z - a[2]).max(0.0) + horiz + (z - down)) / RAPID + (down - b[2]).max(0.0) / feed;
    Some((moves, t))
}

/// 箱 `k` が、線分 `a`–`b` から `rad` より近いか(上から見て)。
fn near_box(k: &(P3, P3), a: [f64; 2], b: [f64; 2], rad: f64) -> bool {
    let (lo, hi) = (k.0, k.1);
    let n = 16;
    (0..=n).any(|i| {
        let t = i as f64 / n as f64;
        let q = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        let dx = (lo[0] - q[0]).max(q[0] - hi[0]).max(0.0);
        let dy = (lo[1] - q[1]).max(q[1] - hi[1]).max(0.0);
        dx.hypot(dy) <= rad
    }) || seg_dist([(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0], a, b)
        <= rad + (hi[0] - lo[0]).hypot(hi[1] - lo[1]) / 2.0
}

/// セットアップ `s` の加工の空振りを、始めの形 `start`(なければストックの箱)から順に省く。
/// 元の経路は `sources[k].original` に残す。終わりの形(次のセットアップへ引き継ぐ)を返す。
pub fn apply(s: &mut SetupPaths, start: Option<ZMap>) -> ZMap {
    let i = &s.info;
    let cell = cell_for(s);
    let mut map = start.unwrap_or_else(|| ZMap::stock(i.stock_min, i.stock_max, cell));
    s.air_start = Some(Arc::new(map.clone()));
    let i = &s.info;
    let bottom = i.stock_min[2];
    let aligned = s.sources.len() == s.paths.len();
    let started = std::time::Instant::now();
    let (mut total_runs, mut total_len) = (0usize, 0.0f64);
    for k in 0..s.paths.len() {
        // レーザーは削らないので、材料の形を変えず、焼く送りを早送りにもしない
        if s.paths[k].laser_power().is_some() {
            continue;
        }
        if aligned && s.sources[k].skip_air {
            let orig = s.sources[k]
                .original
                .clone()
                .unwrap_or_else(|| Arc::new(s.paths[k].clone()));
            let keep_out = s.info.keep_out.clone();
            let clear = (orig.retract_z - i.stock_max[2]).max(0.5);
            match skip_air(&orig, &map, clear, &keep_out) {
                Some((t, runs, len)) => {
                    log::debug!("{}: 空振り {runs} か所 {len:.1} mm を省く", s.paths[k].name);
                    total_runs += runs;
                    total_len += len;
                    let name = s.paths[k].name.clone();
                    s.paths[k] = Toolpath { name, ..t };
                    s.sources[k].original = Some(orig);
                }
                None => {
                    if let Some(o) = s.sources[k].original.take() {
                        let name = s.paths[k].name.clone();
                        s.paths[k] = Toolpath {
                            name,
                            ..(*o).clone()
                        };
                    }
                }
            }
        }
        stamp_path(&mut map, &s.paths[k], bottom);
    }
    if total_runs > 0 {
        log::info!(
            "{}: 前に削った所の空振り {total_runs} か所 {total_len:.0} mm を省いた({:.0} ms、格子 {cell:.2} mm)",
            s.name,
            started.elapsed().as_secs_f64() * 1000.0
        );
    }
    map
}

/// 元の経路に戻す(省いた経路を作り直す前に)。
pub fn restore(s: &mut SetupPaths) {
    if s.sources.len() != s.paths.len() {
        return;
    }
    for k in 0..s.paths.len() {
        if let Some(o) = s.sources[k].original.take() {
            let name = s.paths[k].name.clone();
            s.paths[k] = Toolpath {
                name,
                ..(*o).clone()
            };
        }
    }
}
