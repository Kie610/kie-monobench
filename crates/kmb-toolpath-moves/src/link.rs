//! 経路のつなぎ(削り終えた所から次に削り始める所への、削らない動き)を短くする。
//!
//! 加工の戦略は、つなぎで安全高さまで上げて早送りし、退避高さまで下げてから切削の送りで
//! 下ろす(全退避)。[`relink`] は、経路を先頭から順に削ったとして、削った所と部品の形を
//! 見ながら、つなぎを次のどれか 1 つに置き換える。どれにするかは、つなぎの横の長さと、
//! 通り道にある材料と部品の高さから 1 回の計算で決める(つなぎ方の候補を順に試すことはしない。
//! 機械の運動学のモデルも持たない。docs/避ける形.md)。
//!
//! - 下げたまま / 少し持ち上げる: 横の長さが「下げたまま移る距離」以下で、通り道に削って
//!   いないストックの上面がない時。高さは、始めと終わりの高さ、通り道の材料と部品の高さの
//!   いちばん高い所に「持ち上げる高さ」を足したもの。切削の送りで横に動く。
//! - 低い退避: それ以外で、通り道の材料のいちばん高い所に安全高さの余裕(ストックの上面から
//!   安全高さまでと同じ)を足した高さが、安全高さより低い時。早送りの高さは退避高さ
//!   (ストックの上面 + 1 mm)より下げないので、早送りがストックと部品に当たることはない。
//! - 全退避: 元のまま。
//!
//! 安全の考え方:
//! - 上がる所(始めの真上)と下りる所(終わりの真上)は、元の経路が通った所の一部で、送りも
//!   元と同じ。新しく通るのは、横に動く 1 本の線分だけ。
//! - 下げたまま移る時に、シャンクかホルダーが材料か部品に当たるなら、そのつなぎは下げたまま
//!   移らない(当たらない高さへ持ち上げ直して逃がすことはしない)。
//! - 削った形は 2 つの格子で持つ。`ctr` はシミュレーションと同じく、セルの中心に工具が
//!   かかれば削れたとする(下げたまま移る時の材料。セルの大きさほどの誤差で材料に触れても、
//!   切削の送りなので少し削るだけ)。部品は網の三角形で調べ、食い込まないことを保証する。
//! - `ub` は、セル全体が工具の中に入った時だけ削れたとする(材料の高さの上限)。低い退避の
//!   高さを決めるのに使う。
//! - 削った所は、この加工の中で削った分だけを数える(前の加工で削った分は見ない)。材料が
//!   ストックの箱より多いことはないので、安全側になる。

use kmb_cam_geom::seg_dist;
use kmb_cam_tool::Tool;
use kmb_toolpath::{Link, LinkInfo, LinkKind, Move, P3, Toolpath};

/// つなぎの設定。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinkOptions {
    /// 下げたまま(切削の送りで)移る、横の長さの上限(mm)。0 ならしない
    pub stay_down: f64,
    /// 下げたまま移る時に、通り道の材料と部品の上へ空ける高さ(mm)
    pub lift: f64,
    /// 全退避の高さを、通り道の材料に合わせて下げる
    pub fit_retract: bool,
}

/// 下げたまま移る距離の既定値(mm)。
pub const STAY_DOWN: f64 = 15.0;
/// 持ち上げる高さの既定値(mm)。
pub const LIFT: f64 = 0.2;

impl Default for LinkOptions {
    fn default() -> Self {
        LinkOptions {
            stay_down: STAY_DOWN,
            lift: LIFT,
            fit_retract: true,
        }
    }
}

/// 経路の周りの物(座標はどれも経路と同じ作業座標系)。
pub struct Scene<'a> {
    pub stock_min: P3,
    pub stock_max: P3,
    /// 部品の三角形
    pub triangles: &'a [[[f64; 3]; 3]],
    /// 固定具の避ける範囲の箱
    pub keep_out: &'a [(P3, P3)],
}

/// つなぎを置き換えた数。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub stay_down: usize,
    pub lift: usize,
    pub low: usize,
    pub full: usize,
}

/// 経路 `tp` のつなぎを置き換え、`tp.links` に記録する。
pub fn relink(tp: &mut Toolpath, scene: &Scene<'_>, opt: &LinkOptions) -> Counts {
    let started = std::time::Instant::now();
    let tool = tp.tool.clone();
    let mut grid = Grid::new(scene, tool.radius());
    let part = Part::new(scene.triangles);
    let top = scene.stock_max[2];
    // ストックの上面から安全高さまでの余裕
    let safe = (tp.safe_z - top).max(0.0);
    let moves = std::mem::take(&mut tp.moves);
    let n = moves.len();
    let mut out: Vec<Move> = Vec::with_capacity(n);
    let mut air: Vec<bool> = Vec::with_capacity(n);
    let mut links: Vec<Link> = Vec::new();
    let mut counts = Counts::default();
    // 元の動きごとの、新しい列での始めの番号(入り・リード・つなぎの印を付け直す)
    let mut starts: Vec<usize> = vec![0; n + 1];
    let mut cur: Option<P3> = None;
    let mut feed: Option<f64> = None;
    let step = grid.cell / 2.0;
    // 送りの動きを足して削る。削ったら偽(空でない)
    let mut push = |out: &mut Vec<Move>,
                    air: &mut Vec<bool>,
                    grid: &mut Grid,
                    cur: &mut Option<P3>,
                    m: Move| {
        let removed = match (m, *cur) {
            (Move::Rapid { .. }, _) | (_, None) => 0.0,
            (_, Some(from)) => m
                .points(from, step)
                .into_iter()
                .map(|q| grid.stamp(&tool, q))
                .sum(),
        };
        air.push(removed <= 1e-6 * grid.cell * grid.cell);
        out.push(m);
        *cur = Some(m.to());
    };
    let mut i = 0;
    while i < n {
        let m = moves[i];
        let (Move::Rapid { .. }, Some(a), Some(f)) = (m, cur, feed) else {
            if let Move::Linear { feed: f, .. } | Move::Arc { feed: f, .. } = m {
                feed = Some(f);
            }
            starts[i] = out.len();
            push(&mut out, &mut air, &mut grid, &mut cur, m);
            i += 1;
            continue;
        };
        // 早送りの続き(つなぎ)と、その後の真下への送り(下りる所)
        let mut j = i;
        while j < n && matches!(moves[j], Move::Rapid { .. }) {
            j += 1;
        }
        if j == n {
            // 最後の退避
            for (d, &m) in moves[i..].iter().enumerate() {
                starts[i + d] = out.len();
                push(&mut out, &mut air, &mut grid, &mut cur, m);
            }
            break;
        }
        let run_end = moves[j - 1].to();
        let mut k = j;
        let mut b = run_end;
        while k < n {
            match moves[k] {
                Move::Linear { to, .. } if same_xy(to, b) && to[2] < b[2] => {
                    b = to;
                    k += 1;
                }
                _ => break,
            }
        }
        let down = &moves[j..k];
        let plan = plan(
            &grid,
            &part,
            scene,
            &tool,
            opt,
            (a, b, run_end),
            (top, safe),
            tp,
        );
        let start = out.len();
        // つなぎの早送りは新しいつなぎの始めへ、下りる送りは新しい下りの始めへ
        starts[i..k].fill(start);
        let (kind, z) = match plan {
            Plan::Feed { z, stay } => {
                if z > a[2] + 1e-9 {
                    push(
                        &mut out,
                        &mut air,
                        &mut grid,
                        &mut cur,
                        Move::Rapid {
                            to: [a[0], a[1], z],
                        },
                    );
                }
                push(
                    &mut out,
                    &mut air,
                    &mut grid,
                    &mut cur,
                    Move::Linear {
                        to: [b[0], b[1], z],
                        feed: f,
                    },
                );
                starts[j..k].fill(out.len());
                descend(&mut out, &mut air, &mut grid, &mut cur, &mut push, down, z);
                if stay {
                    counts.stay_down += 1;
                    (LinkKind::StayDown, z)
                } else {
                    counts.lift += 1;
                    (LinkKind::Lift, z)
                }
            }
            Plan::Low { z } => {
                if z > a[2] + 1e-9 {
                    push(
                        &mut out,
                        &mut air,
                        &mut grid,
                        &mut cur,
                        Move::Rapid {
                            to: [a[0], a[1], z],
                        },
                    );
                }
                push(
                    &mut out,
                    &mut air,
                    &mut grid,
                    &mut cur,
                    Move::Rapid {
                        to: [b[0], b[1], z],
                    },
                );
                if run_end[2] < z - 1e-9 {
                    // 早送りで下りるのは退避高さまで。早送りの続きが退避高さより下で終わる時
                    // (削った穴の中で上げただけの所など)は、その先を送りで下りる
                    let r = tp.retract_z.max(run_end[2]);
                    if r < z - 1e-9 {
                        push(
                            &mut out,
                            &mut air,
                            &mut grid,
                            &mut cur,
                            Move::Rapid {
                                to: [b[0], b[1], r],
                            },
                        );
                    }
                    if run_end[2] < r - 1e-9 {
                        push(
                            &mut out,
                            &mut air,
                            &mut grid,
                            &mut cur,
                            Move::Linear {
                                to: [b[0], b[1], run_end[2]],
                                feed: f,
                            },
                        );
                    }
                }
                starts[j..k].fill(out.len());
                descend(&mut out, &mut air, &mut grid, &mut cur, &mut push, down, z);
                counts.low += 1;
                (LinkKind::Low, z)
            }
            Plan::Keep => {
                let high = moves[i..j]
                    .iter()
                    .map(|m| m.to()[2])
                    .fold(f64::MIN, f64::max);
                for (d, &m) in moves[i..k].iter().enumerate() {
                    starts[i + d] = out.len();
                    push(&mut out, &mut air, &mut grid, &mut cur, m);
                }
                if high >= tp.safe_z - 1e-9 {
                    counts.full += 1;
                    (LinkKind::Full, high)
                } else {
                    counts.low += 1;
                    (LinkKind::Low, high)
                }
            }
        };
        links.push(Link {
            start,
            end: out.len(),
            kind,
            z,
        });
        i = k;
    }
    starts[n] = out.len();
    tp.moves = out;
    tp.remap_spans(&starts);
    tp.links = LinkInfo {
        links,
        air: ranges(&air),
    };
    log::debug!(
        "つなぎ({}): 下げたまま {}、持ち上げ {}、低い退避 {}、全退避 {}({} ms、格子 {} × {}、{} mm)",
        tp.name,
        counts.stay_down,
        counts.lift,
        counts.low,
        counts.full,
        started.elapsed().as_millis(),
        grid.nx,
        grid.ny,
        grid.cell
    );
    counts
}

type Push<'p> = dyn FnMut(&mut Vec<Move>, &mut Vec<bool>, &mut Grid, &mut Option<P3>, Move) + 'p;

/// 元の下りる動き `down`(真下への送り)のうち、高さ `z` より下の所を足す。
fn descend(
    out: &mut Vec<Move>,
    air: &mut Vec<bool>,
    grid: &mut Grid,
    cur: &mut Option<P3>,
    push: &mut Push<'_>,
    down: &[Move],
    z: f64,
) {
    let mut at = z;
    for &m in down {
        if m.to()[2] < at - 1e-9 {
            push(out, air, grid, cur, m);
            at = m.to()[2];
        }
    }
}

fn same_xy(a: P3, b: P3) -> bool {
    (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-6
}

/// 真の続きを `[始め, 終わり)` の範囲にする。
fn ranges(flags: &[bool]) -> Vec<[usize; 2]> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, &f) in flags.iter().enumerate() {
        match (f, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push([s, i]);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push([s, flags.len()]);
    }
    out
}

enum Plan {
    /// 切削の送りで高さ `z` を横に動く。`stay` は始めと終わりの高さのまま(材料の上へ持ち上げない)
    Feed { z: f64, stay: bool },
    /// 高さ `z`(退避高さ以上、安全高さ未満)で早送りする
    Low { z: f64 },
    /// 元のまま
    Keep,
}

/// つなぎ `a` → `b`(`run_end` は元の早送りの終わり)の動き方を決める。
#[allow(clippy::too_many_arguments)]
fn plan(
    grid: &Grid,
    part: &Part,
    scene: &Scene<'_>,
    tool: &Tool,
    opt: &LinkOptions,
    (a, b, run_end): (P3, P3, P3),
    (top, safe): (f64, f64),
    tp: &Toolpath,
) -> Plan {
    let (a2, b2) = ([a[0], a[1]], [b[0], b[1]]);
    let len = (b2[0] - a2[0]).hypot(b2[1] - a2[1]);
    let r = tool.radius();
    let ends = a[2].max(b[2]);
    // 横へ動かず、退避高さより下で止まる早送り(つつきの戻りや、削った穴の中で上げるだけの
    // 動き)は、つなぎではないのでそのまま
    if len < 1e-9 && run_end[2] < tp.retract_z - 1e-9 {
        return Plan::Keep;
    }
    if opt.stay_down > 0.0 && len <= opt.stay_down + 1e-9 {
        // 刃の届く所: 削った形(工具の形を見る)、部品、固定具
        let need = grid
            .need_ctr(tool, a2, b2, r)
            .max(part.max_near(a2, b2, r - 1e-4))
            .max(fixture_need(scene.keep_out, a2, b2, r));
        // 削っていないストックの上面を通るなら下げたまま移らない(V の先のように細い工具でも、
        // 刃の届く範囲に削っていない所があれば上面の上を擦ることになる)
        let uncut = grid.need_flat(&grid.ctr, a2, b2, r) >= top - 1e-3;
        if !uncut && need < top - 1e-3 {
            let z = ends.max(need) + opt.lift;
            // シャンクとホルダー(刃の上の太い所)が材料か部品に当たるなら、下げたまま移らない。
            // 当たらない高さへ持ち上げ直すことはしない(当たりを見つけて経路を逃がす形は
            // docs/避ける形.md の決まりで避ける)
            let rs = tool.shank().max(tool.diameter) / 2.0;
            let holder = tool.stickout.zip(tool.holder_diameter);
            let hits = std::iter::once((rs, tool.flute_length))
                .chain(holder.map(|(s, d)| (d / 2.0, s)))
                .any(|(rad, above)| {
                    z + above < top
                        && grid
                            .need_flat(&grid.ctr, a2, b2, rad)
                            .max(part.max_near(a2, b2, rad - 1e-4))
                            .max(fixture_need(scene.keep_out, a2, b2, rad))
                            > z + above
                });
            if !hits && z < tp.retract_z - 1e-9 && z < run_end[2] {
                return Plan::Feed {
                    z,
                    stay: need <= ends + 1e-6,
                };
            }
        }
    }
    if !opt.fit_retract {
        return Plan::Keep;
    }
    // 早送りの高さは退避高さより下げない(ストックの上面より上なので、材料にも部品にも
    // 当たらない)。固定具はシャンクとホルダーの太さも見る
    let wide = [
        r,
        tool.shank() / 2.0,
        tool.holder_diameter.unwrap_or(0.0) / 2.0,
    ]
    .into_iter()
    .fold(0.0, f64::max);
    let h = grid
        .need_ub(a2, b2, r)
        .max(fixture_need(scene.keep_out, a2, b2, wide));
    let z = (h + safe)
        .max(tp.retract_z)
        .max(a[2])
        .max(b[2])
        .max(fixture_need(scene.keep_out, a2, b2, wide) + 1.0);
    if z < tp.safe_z - 1e-6 {
        Plan::Low { z }
    } else {
        Plan::Keep
    }
}

/// 固定具の避ける範囲の箱のうち、線分 `a`–`b` から `rad` より近いもののいちばん高い所。
fn fixture_need(boxes: &[(P3, P3)], a: [f64; 2], b: [f64; 2], rad: f64) -> f64 {
    boxes
        .iter()
        .filter(|(lo, hi)| seg_rect_dist(a, b, [lo[0], lo[1]], [hi[0], hi[1]]) < rad)
        .map(|(_, hi)| hi[2])
        .fold(f64::NEG_INFINITY, f64::max)
}

/// 線分と長方形(軸に沿う)の距離。
fn seg_rect_dist(a: [f64; 2], b: [f64; 2], lo: [f64; 2], hi: [f64; 2]) -> f64 {
    let inside = |p: [f64; 2]| p[0] >= lo[0] && p[0] <= hi[0] && p[1] >= lo[1] && p[1] <= hi[1];
    if inside(a) || inside(b) {
        return 0.0;
    }
    let c = [lo, [hi[0], lo[1]], hi, [lo[0], hi[1]]];
    let mut d = f64::INFINITY;
    for k in 0..4 {
        let (p, q) = (c[k], c[(k + 1) % 4]);
        if segments_cross(a, b, p, q) {
            return 0.0;
        }
        d = d
            .min(seg_dist(p, a, b))
            .min(seg_dist(a, p, q))
            .min(seg_dist(b, p, q));
    }
    d
}

fn segments_cross(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    let cross = |o: [f64; 2], p: [f64; 2], q: [f64; 2]| {
        (p[0] - o[0]) * (q[1] - o[1]) - (p[1] - o[1]) * (q[0] - o[0])
    };
    let (d1, d2) = (cross(a, b, c), cross(a, b, d));
    let (d3, d4) = (cross(c, d, a), cross(c, d, b));
    d1 * d2 < 0.0 && d3 * d4 < 0.0
}

/// 上から見た高さの格子 2 つ(`ctr` と `ub`、モジュールの説明を参照)。
struct Grid {
    origin: [f64; 2],
    cell: f64,
    nx: usize,
    ny: usize,
    ctr: Vec<f64>,
    ub: Vec<f64>,
    /// 格子の外の高さ(機械のテーブル = ストックの底)
    bed: f64,
}

/// 格子のセルの数の上限。
const MAX_CELLS: f64 = 2.0e6;

impl Grid {
    fn new(scene: &Scene<'_>, r: f64) -> Grid {
        let (smin, smax) = (scene.stock_min, scene.stock_max);
        let margin = r + 2.0;
        let origin = [smin[0] - margin, smin[1] - margin];
        let w = smax[0] - smin[0] + 2.0 * margin;
        let h = smax[1] - smin[1] + 2.0 * margin;
        let mut cell = (r / 4.0).clamp(0.05, 0.5);
        if w * h / (cell * cell) > MAX_CELLS {
            cell = (w * h / MAX_CELLS).sqrt();
        }
        let nx = (w / cell).ceil().max(1.0) as usize;
        let ny = (h / cell).ceil().max(1.0) as usize;
        let (top, bed) = (smax[2], smin[2]);
        let mut ctr = vec![bed; nx * ny];
        let mut ub = vec![bed; nx * ny];
        for j in 0..ny {
            let y0 = origin[1] + j as f64 * cell;
            for i in 0..nx {
                let x0 = origin[0] + i as f64 * cell;
                let k = j * nx + i;
                let (cx, cy) = (x0 + cell / 2.0, y0 + cell / 2.0);
                if cx >= smin[0] && cx <= smax[0] && cy >= smin[1] && cy <= smax[1] {
                    ctr[k] = top;
                }
                // セルの一部でもストックに掛かれば上限は上面
                if x0 + cell > smin[0] && x0 < smax[0] && y0 + cell > smin[1] && y0 < smax[1] {
                    ub[k] = top;
                }
            }
        }
        Grid {
            origin,
            cell,
            nx,
            ny,
            ctr,
            ub,
            bed,
        }
    }

    fn center(&self, i: usize, j: usize) -> [f64; 2] {
        [
            self.origin[0] + (i as f64 + 0.5) * self.cell,
            self.origin[1] + (j as f64 + 0.5) * self.cell,
        ]
    }

    /// 座標の範囲 [a, b] に掛かるセルの添字の範囲。
    fn span(&self, a: f64, b: f64, axis: usize) -> (usize, usize) {
        let n = if axis == 0 { self.nx } else { self.ny };
        let o = self.origin[axis];
        let lo = ((a - o) / self.cell).floor().max(0.0) as usize;
        let hi = (((b - o) / self.cell).floor() + 1.0).clamp(0.0, n as f64) as usize;
        (lo.min(n), hi.max(lo.min(n)))
    }

    /// 工具の先端を `tip` に置いて削る。`ctr` で削った量(体積)を返す。
    fn stamp(&mut self, tool: &Tool, tip: P3) -> f64 {
        let r = tool.radius();
        let (i0, i1) = self.span(tip[0] - r, tip[0] + r, 0);
        let (j0, j1) = self.span(tip[1] - r, tip[1] + r, 1);
        let half = self.cell / 2.0;
        let mut removed = 0.0;
        for j in j0..j1 {
            for i in i0..i1 {
                let c = self.center(i, j);
                let (dx, dy) = ((c[0] - tip[0]).abs(), (c[1] - tip[1]).abs());
                let k = j * self.nx + i;
                if let Some(h) = tool.height_at(dx.hypot(dy) + 1e-6) {
                    let z = tip[2] + h;
                    if z < self.ctr[k] {
                        removed += self.ctr[k] - z;
                        self.ctr[k] = z;
                    }
                }
                // セルの最も遠い隅まで刃の中なら、セル全体がその高さより下
                if let Some(h) = tool.height_at((dx + half).hypot(dy + half)) {
                    let z = tip[2] + h;
                    if z < self.ub[k] {
                        self.ub[k] = z;
                    }
                }
            }
        }
        removed * self.cell * self.cell
    }

    /// 線分 `a`–`b` から `rad` より近いセル(の中心)の `(i, j, 線分までの距離)` を順に渡す。
    fn near(&self, a: [f64; 2], b: [f64; 2], rad: f64, mut f: impl FnMut(usize, usize, f64)) {
        let (i0, i1) = self.span(a[0].min(b[0]) - rad, a[0].max(b[0]) + rad, 0);
        let (j0, j1) = self.span(a[1].min(b[1]) - rad, a[1].max(b[1]) + rad, 1);
        for j in j0..j1 {
            for i in i0..i1 {
                let d = seg_dist(self.center(i, j), a, b);
                if d < rad {
                    f(i, j, d);
                }
            }
        }
    }

    /// 線分の上を工具が高さ z で動く時、`ctr` の材料に刃が触れない最も低い z。
    fn need_ctr(&self, tool: &Tool, a: [f64; 2], b: [f64; 2], r: f64) -> f64 {
        let mut need = self.bed;
        self.near(a, b, r, |i, j, d| {
            let h = tool.height_at(d).unwrap_or(0.0);
            need = need.max(self.ctr[j * self.nx + i] - h);
        });
        need
    }

    /// 平らな円柱(半径 `rad`)が `map` の材料に触れない最も低い高さ。
    fn need_flat(&self, map: &[f64], a: [f64; 2], b: [f64; 2], rad: f64) -> f64 {
        let mut need = self.bed;
        self.near(a, b, rad, |i, j, _| need = need.max(map[j * self.nx + i]));
        need
    }

    /// 早送りの線分 `a`–`b` の下の材料の高さの上限(`ub`)。始めの後ろ側と終わりの先の側に
    /// すっかり入るセルは数えない(工具はそこにいたので、そこの材料は工具の外にあり、
    /// 線分の上を動いても近づかない)。
    fn need_ub(&self, a: [f64; 2], b: [f64; 2], r: f64) -> f64 {
        let len = (b[0] - a[0]).hypot(b[1] - a[1]);
        let u = if len > 1e-9 {
            [(b[0] - a[0]) / len, (b[1] - a[1]) / len]
        } else {
            [0.0, 0.0]
        };
        let c = self.cell;
        let mut need = self.bed;
        self.near(a, b, r + c * std::f64::consts::FRAC_1_SQRT_2, |i, j, _| {
            let x0 = self.origin[0] + i as f64 * c;
            let y0 = self.origin[1] + j as f64 * c;
            let corners = [[x0, y0], [x0 + c, y0], [x0, y0 + c], [x0 + c, y0 + c]];
            let along = |p: [f64; 2], o: [f64; 2]| (p[0] - o[0]) * u[0] + (p[1] - o[1]) * u[1];
            if len > 1e-9
                && (corners.iter().all(|&p| along(p, a) <= 0.0)
                    || corners.iter().all(|&p| along(p, b) >= 0.0))
            {
                return;
            }
            need = need.max(self.ub[j * self.nx + i]);
        });
        need
    }
}

/// 三角形と、上から見た外形の箱(最小 x、最小 y、最大 x、最大 y)と、いちばん高い z。
type BoxedTri<'a> = (&'a [[f64; 3]; 3], [f64; 4], f64);

/// 部品の三角形。
struct Part<'a> {
    tris: Vec<BoxedTri<'a>>,
}

impl<'a> Part<'a> {
    fn new(triangles: &'a [[[f64; 3]; 3]]) -> Part<'a> {
        let tris = triangles
            .iter()
            .map(|t| {
                let lo = |k: usize| t.iter().map(|v| v[k]).fold(f64::INFINITY, f64::min);
                let hi = |k: usize| t.iter().map(|v| v[k]).fold(f64::NEG_INFINITY, f64::max);
                (t, [lo(0), lo(1), hi(0), hi(1)], hi(2))
            })
            .collect();
        Part { tris }
    }

    /// 上から見て線分 `a`–`b` から `rad` 以内にある部品のいちばん高い所。なければ負の無限大。
    fn max_near(&self, a: [f64; 2], b: [f64; 2], rad: f64) -> f64 {
        let lo = [a[0].min(b[0]) - rad, a[1].min(b[1]) - rad];
        let hi = [a[0].max(b[0]) + rad, a[1].max(b[1]) + rad];
        let mut best = f64::NEG_INFINITY;
        for (t, bb, top) in &self.tris {
            if *top <= best || bb[2] < lo[0] || bb[0] > hi[0] || bb[3] < lo[1] || bb[1] > hi[1] {
                continue;
            }
            if let Some(z) = tri_max_near(t, a, b, rad) {
                best = best.max(z);
            }
        }
        best
    }
}

/// 三角形 `t` のうち、上から見て線分 `a`–`b` から `rad` 以内にある所のいちばん高い z。
/// その範囲(線分を太らせた形)と三角形の重なりは凸なので、高さの最大は角の点にある:
/// 範囲の中の頂点、辺と範囲の縁の交点、三角形の中にある範囲の縁の点(円弧の上で
/// 傾きの向きの点と、直線の縁の端)。
fn tri_max_near(t: &[[f64; 3]; 3], a: [f64; 2], b: [f64; 2], rad: f64) -> Option<f64> {
    let mut best: Option<f64> = None;
    let mut take = |p: [f64; 2], z: f64| {
        if seg_dist(p, a, b) <= rad + 1e-9 {
            best = Some(best.map_or(z, |w: f64| w.max(z)));
        }
    };
    for v in t {
        take([v[0], v[1]], v[2]);
    }
    let len = (b[0] - a[0]).hypot(b[1] - a[1]);
    let (u, nrm) = if len > 1e-12 {
        let u = [(b[0] - a[0]) / len, (b[1] - a[1]) / len];
        (u, [-u[1], u[0]])
    } else {
        ([1.0, 0.0], [0.0, 1.0])
    };
    // 辺と範囲の縁の交点
    for k in 0..3 {
        let (p, q) = (t[k], t[(k + 1) % 3]);
        let d = [q[0] - p[0], q[1] - p[1]];
        let at = |s: f64| ([p[0] + d[0] * s, p[1] + d[1] * s], p[2] + (q[2] - p[2]) * s);
        let mut ts: Vec<f64> = Vec::new();
        for c in [a, b] {
            // |p + s d - c|² = rad²
            let f = [p[0] - c[0], p[1] - c[1]];
            let qa = d[0] * d[0] + d[1] * d[1];
            if qa < 1e-18 {
                continue;
            }
            let qb = 2.0 * (f[0] * d[0] + f[1] * d[1]);
            let qc = f[0] * f[0] + f[1] * f[1] - rad * rad;
            let disc = qb * qb - 4.0 * qa * qc;
            if disc >= 0.0 {
                let sq = disc.sqrt();
                ts.push((-qb - sq) / (2.0 * qa));
                ts.push((-qb + sq) / (2.0 * qa));
            }
        }
        let nd = nrm[0] * d[0] + nrm[1] * d[1];
        if nd.abs() > 1e-15 {
            let np = nrm[0] * (p[0] - a[0]) + nrm[1] * (p[1] - a[1]);
            for side in [-rad, rad] {
                ts.push((side - np) / nd);
            }
        }
        for s in ts {
            if (0.0..=1.0).contains(&s) {
                let (x, z) = at(s);
                take(x, z);
            }
        }
    }
    // 三角形の中の、範囲の縁の点(三角形が平らでない時は高さが平面の式で決まる)
    if let Some((g, plane)) = plane_of(t) {
        let gl = g[0].hypot(g[1]);
        let mut pts = vec![
            [a[0] + nrm[0] * rad, a[1] + nrm[1] * rad],
            [a[0] - nrm[0] * rad, a[1] - nrm[1] * rad],
            [b[0] + nrm[0] * rad, b[1] + nrm[1] * rad],
            [b[0] - nrm[0] * rad, b[1] - nrm[1] * rad],
            [a[0] - u[0] * rad, a[1] - u[1] * rad],
            [b[0] + u[0] * rad, b[1] + u[1] * rad],
        ];
        if gl > 1e-12 {
            for c in [a, b] {
                pts.push([c[0] + g[0] / gl * rad, c[1] + g[1] / gl * rad]);
            }
        }
        for p in pts {
            if inside_xy(t, p) {
                take(p, plane(p));
            }
        }
    }
    best
}

/// 三角形の平面の傾き(上から見た高さの勾配)と高さの式。上から見て潰れていれば `None`。
#[allow(clippy::type_complexity)]
fn plane_of(t: &[[f64; 3]; 3]) -> Option<([f64; 2], impl Fn([f64; 2]) -> f64 + use<>)> {
    let [p, q, s] = *t;
    let (ux, uy, uz) = (q[0] - p[0], q[1] - p[1], q[2] - p[2]);
    let (vx, vy, vz) = (s[0] - p[0], s[1] - p[1], s[2] - p[2]);
    let det = ux * vy - uy * vx;
    if det.abs() < 1e-12 {
        return None;
    }
    // z = p.z + gx (x - p.x) + gy (y - p.y)
    let gx = (uz * vy - vz * uy) / det;
    let gy = (vz * ux - uz * vx) / det;
    Some(([gx, gy], move |x: [f64; 2]| {
        p[2] + gx * (x[0] - p[0]) + gy * (x[1] - p[1])
    }))
}

fn inside_xy(t: &[[f64; 3]; 3], x: [f64; 2]) -> bool {
    let [a, b, c] = *t;
    let d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
    if d.abs() < 1e-12 {
        return false;
    }
    let l1 = ((b[1] - c[1]) * (x[0] - c[0]) + (c[0] - b[0]) * (x[1] - c[1])) / d;
    let l2 = ((c[1] - a[1]) * (x[0] - c[0]) + (a[0] - c[0]) * (x[1] - c[1])) / d;
    let e = -1e-9;
    l1 >= e && l2 >= e && 1.0 - l1 - l2 >= e
}

#[cfg(test)]
mod tests;
