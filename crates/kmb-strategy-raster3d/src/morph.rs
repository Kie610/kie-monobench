//! 3D のモーフィング スパイラル(`cam.morph3d@1`)。加工の範囲の形に沿って外から内へ縮んでいく
//! 1 本の渦巻きを平面の上に作り、点ごとに工具を部品へ下ろしてなぞる。なだらかな面の仕上げで、
//! 持ち上げずに範囲の全体を回る(急な面はスキャロップや等高線の方が向く)。
//!
//! 平面の渦巻きの作り方:
//!
//! - 範囲の中の点の、範囲の境目からの距離 d を格子で求め、送り幅ほどの長さで拡散させてならす
//!   (f)。f の等高線は、境目の近くでは境目と同じ形、内へ行くほど角が丸くなる
//! - 段は、前の輪と次の輪の間のどの点も、どちらかの輪から送り幅の半分以内に入る最大の段を選ぶ
//!   (取り残しが出ない。細い所・鋭い角の近くでは輪が詰まる)
//! - 隣り合う段の輪を、弧長の割合で混ぜながら 1 周ずつ移って、1 本の渦巻きにする。輪が分かれる
//!   所は、分かれた輪ごとに渦巻きを続ける
//!
//! 中心軸(ボロノイ図)は使わない。工具の負荷を見て線を変えることもしない。範囲を決めない時は、
//! 部品の外形(工具の半径だけ外まで)の長方形を範囲にする。

use crate::{BRIDGE, Pt, cut_passes, split};
use crate::{resolution, zero};
use kmb_cam_dropcutter::limits::{boundary_fields, leave_radial_field, slope_fields, slope_span};
use kmb_cam_dropcutter::region::{distance2, level_loops};
use kmb_cam_dropcutter::{Cutter, Grid, Heights, Limits, Resolved};
use kmb_cam_setup::{
    OpBase, Planner, Prepared, StrategyInput, StrategyOutput, expr_refs, invalid, length,
    plan_error, prepare,
};
use kmb_cam_tool::ToolKind;
use kmb_core::{EvalCtx, EvalError, Feature, Field, InputRef, Outputs, Registry};
use kmb_toolpath::P3;
use serde::{Deserialize, Serialize};

pub const MORPH3D: &str = "cam.morph3d@1";

type P2 = [f64; 2];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Morph {
    #[serde(flatten)]
    pub base: OpBase,
    /// 軸方向に残す厚さ
    #[serde(default = "zero")]
    pub leave: String,
    /// 径方向に残す厚さ・加工の範囲・傾き
    #[serde(flatten)]
    pub limits: Limits,
    /// 部品の高さを調べる格子の幅(線の点の間隔も同じ)
    #[serde(default = "resolution")]
    pub resolution: String,
    /// 輪の間隔の上限。空なら工具の送り幅
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    /// 部品のいちばん高い所から、どこまで下をなぞるか(省くとストックの底まで)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<String>,
    /// 内から外へたどる(既定は外から内へ)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub outward: bool,
    /// 上から見て時計回りに回る(既定は反時計回り)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub clockwise: bool,
}

impl Morph {
    pub fn new(base: OpBase) -> Morph {
        Morph {
            base,
            leave: zero(),
            limits: Limits::default(),
            resolution: resolution(),
            step: None,
            depth: None,
            outward: false,
            clockwise: false,
        }
    }
}

/// 経路の計算の設定(式を数にしたもの)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MorphOpts {
    pub leave: f64,
    pub cell: f64,
    /// これより下はなぞらない
    pub floor: f64,
    pub step: f64,
    pub outward: bool,
    pub clockwise: bool,
    pub limits: Resolved,
}

pub(crate) use kmb_cam_geom::polygon_area as signed_area;
use kmb_cam_geom::ring::{at, cumulative};
pub(crate) use kmb_cam_geom::ring::{dist, nearest, rotate};

/// 範囲の格子の上の、ならした距離の場と、その等高線を選ぶ道具。
struct Field2 {
    grid: Grid,
    inside: Vec<bool>,
    /// 境目からの距離
    d: Vec<f64>,
    /// `d` をならした物
    f: Vec<f64>,
}

impl Field2 {
    /// `inside` の境目からの距離を、長さ `smooth` ほど拡散でならす。
    fn new(grid: Grid, inside: Vec<bool>, smooth: f64) -> Field2 {
        let (nx, ny) = (grid.nx, grid.ny);
        let outside: Vec<bool> = inside.iter().map(|&m| !m).collect();
        let d: Vec<f64> = distance2(&outside, nx, ny)
            .into_iter()
            .zip(&inside)
            .map(|(d2, &m)| if m { d2.sqrt() * grid.cell } else { 0.0 })
            .collect();
        let mut f = d.clone();
        // 拡散の長さ ≒ √回数 × 格子の幅
        let it = ((smooth / grid.cell).powi(2).round() as usize).min(400);
        let mut g = f.clone();
        for _ in 0..it {
            for j in 0..ny {
                for i in 0..nx {
                    let k = j * nx + i;
                    if !inside[k] {
                        continue;
                    }
                    let v = |a: usize, b: usize| f[b * nx + a];
                    let (l, r) = (v(i.saturating_sub(1), j), v((i + 1).min(nx - 1), j));
                    let (d, u) = (v(i, j.saturating_sub(1)), v(i, (j + 1).min(ny - 1)));
                    g[k] = 0.25 * (l + r + d + u);
                }
            }
            std::mem::swap(&mut f, &mut g);
        }
        Field2 { grid, inside, d, f }
    }

    fn max(&self) -> f64 {
        self.f.iter().copied().fold(0.0, f64::max)
    }

    /// 高さ `l` の等高線(f ≥ l の所の外周が反時計回り)。ならした場は頂上の近くで平らになり、
    /// 輪がそこで急に小さくなるので、小さな輪も捨てない(捨てると頂上の近くを埋める輪が無くなり、
    /// 段が決まらなくなる)
    fn rings(&self, l: f64) -> Vec<Vec<P2>> {
        level_loops(&self.grid, &self.f, l, 0.0)
    }

    /// 輪の上の点から半径 `h` の内の節点に印を付ける。
    fn stamp(&self, rings: &[Vec<P2>], h: f64, out: &mut [bool]) {
        let g = &self.grid;
        let r = (h / g.cell).ceil() as isize;
        let h2 = (h / g.cell).powi(2);
        for ring in rings {
            let n = ring.len();
            for k in 0..n {
                let (a, b) = (ring[k], ring[(k + 1) % n]);
                let m = ((dist(a, b) / (0.5 * g.cell)).ceil() as usize).max(1);
                for s in 0..m {
                    let t = s as f64 / m as f64;
                    let q = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
                    let (fi, fj) = ((q[0] - g.origin[0]) / g.cell, (q[1] - g.origin[1]) / g.cell);
                    let (ci, cj) = (fi.round() as isize, fj.round() as isize);
                    for dj in -r..=r {
                        for di in -r..=r {
                            let (i, j) = (ci + di, cj + dj);
                            if i < 0 || j < 0 || i >= g.nx as isize || j >= g.ny as isize {
                                continue;
                            }
                            let (x, y) = (i as f64 - fi, j as f64 - fj);
                            if x * x + y * y <= h2 {
                                out[j as usize * g.nx + i as usize] = true;
                            }
                        }
                    }
                }
            }
        }
    }
}

/// 範囲 `inside`(`grid` の節点の印)を、間隔 `step` 以下の輪で埋めて、渦巻きの線にする(外から内へ。
/// 上から見て反時計回り、`clockwise` なら時計回り)。輪が分かれる所では、分かれた輪ごとに別の線になる。
pub fn morph_xy(grid: Grid, inside: Vec<bool>, step: f64, clockwise: bool) -> Vec<Vec<P2>> {
    let cell = grid.cell;
    let field = Field2::new(grid, inside, step);
    let fmax = field.max();
    if fmax <= cell {
        return Vec::new();
    }
    // 節点の印の丸めの分だけ、半分の間隔を詰める
    let h = (0.5 * step - 0.5 * cell).max(cell);
    let n = field.f.len();
    // 前の輪までで送り幅の半分以内に入った節点(範囲の外は埋める必要がない)
    let mut covered: Vec<bool> = field.inside.iter().map(|&m| !m).collect();
    let mut levels: Vec<Vec<Vec<P2>>> = Vec::new();
    // いちばん外の輪は境目のすぐ内側(工具の中心が範囲の縁を回る)。ならした場の等高線は角を
    // 丸めて隅を取り残すので、ならす前の距離で取る
    let mut prev = 0.0;
    let first = level_loops(&field.grid, &field.d, 0.5 * cell, 2.0 * cell);
    field.stamp(&first, h, &mut covered);
    levels.push(first);
    let ok = |l: f64, prev: f64, covered: &[bool]| -> bool {
        let mut c = covered.to_vec();
        if l < fmax {
            field.stamp(&field.rings(l), h, &mut c);
        }
        (0..n).all(|k| c[k] || field.f[k] < prev || (l < fmax && field.f[k] > l))
    };
    let mut guess = step;
    while !ok(fmax, prev, &covered) {
        // 前の段の幅から始めて、通る間は広げ、通らなくなったら二分で詰める
        let (mut a, mut b) = (prev, fmax);
        let mut l = (prev + guess).min(fmax);
        while ok(l, prev, &covered) && l < fmax {
            a = l;
            l = (prev + (l - prev) * 1.5).min(fmax);
        }
        if l < b {
            b = l;
        }
        for _ in 0..8 {
            let c = 0.5 * (a + b);
            if ok(c, prev, &covered) {
                a = c;
            } else {
                b = c;
            }
        }
        if a <= prev + 1e-9 {
            a = 0.5 * (prev + b);
        }
        guess = (a - prev).max(cell * 0.1);
        let rings = field.rings(a);
        field.stamp(&rings, h, &mut covered);
        levels.push(rings);
        prev = a;
    }
    chain_levels(&levels)
        .into_iter()
        .map(|mut c| {
            // 範囲の外周の輪は反時計回り、穴の周りの輪は時計回りに並んでいる。回る向きをそろえる
            for r in &mut c {
                if (signed_area(r) > 0.0) == clockwise {
                    r.reverse();
                }
            }
            spiral(&c)
        })
        .collect()
}

/// 段ごとの輪を、外から内へ移る列に分ける。
fn chain_levels(levels: &[Vec<Vec<P2>>]) -> Vec<Vec<Vec<P2>>> {
    // 段 k の輪 i から段 k+1 のどの輪へ移るか(向きが同じで、いちばん近い物。
    // 一つの輪へ移れるのは一つだけ)
    let mut used: Vec<Vec<bool>> = levels.iter().map(|l| vec![false; l.len()]).collect();
    let mut out = Vec::new();
    for k in 0..levels.len() {
        for i in 0..levels[k].len() {
            if used[k][i] {
                continue;
            }
            used[k][i] = true;
            let mut chain = vec![levels[k][i].clone()];
            let mut kk = k;
            while kk + 1 < levels.len() {
                let last = chain.last().expect("輪がある");
                let sign = signed_area(last).signum();
                let c = centroid(last);
                let best = levels[kk + 1]
                    .iter()
                    .enumerate()
                    .filter(|(j, r)| !used[kk + 1][*j] && signed_area(r).signum() == sign)
                    .filter(|(_, r)| inside_ring(last, r[0]) == (sign > 0.0))
                    .min_by(|a, b| dist(centroid(a.1), c).total_cmp(&dist(centroid(b.1), c)));
                let Some((j, _)) = best else {
                    break;
                };
                used[kk + 1][j] = true;
                chain.push(levels[kk + 1][j].clone());
                kk += 1;
            }
            out.push(chain);
        }
    }
    out
}

fn centroid(pts: &[P2]) -> P2 {
    let n = pts.len() as f64;
    let s = pts
        .iter()
        .fold([0.0, 0.0], |a, q| [a[0] + q[0], a[1] + q[1]]);
    [s[0] / n, s[1] / n]
}

pub(crate) fn inside_ring(ring: &[P2], p: P2) -> bool {
    let mut inside = false;
    let n = ring.len();
    for k in 0..n {
        let (a, b) = (ring[k], ring[(k + 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0])
        {
            inside = !inside;
        }
    }
    inside
}

/// 輪の列を、1 周ずつ次の輪へ混ぜながら移る 1 本の線にする。混ぜた周は輪の上を始めの点でしか
/// 通らないので、最初の輪(範囲の縁)と最後の輪は、それぞれ一周そのままなぞる。
fn spiral(chain: &[Vec<P2>]) -> Vec<P2> {
    let mut cur = chain[0].clone();
    let mut out: Vec<P2> = cur.clone();
    for next in &chain[1..] {
        let b = rotate(next, nearest(next, cur[0]));
        let (ca, cb) = (cumulative(&cur), cumulative(&b));
        let per = ca[cur.len()].max(cb[b.len()]);
        let ds = per / (cur.len().max(b.len()) as f64);
        let m = ((per / ds.max(1e-6)).ceil() as usize).max(8);
        for s in 0..m {
            let t = s as f64 / m as f64;
            let (p, q) = (at(&cur, &ca, t), at(&b, &cb, t));
            out.push([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]);
        }
        cur = b;
    }
    out.extend(cur.iter().copied());
    out.push(cur[0]);
    out
}

/// 工具の先端が通る点の続き(作業座標系)。
pub fn morph_passes(p: &Planner, o: &MorphOpts) -> Result<Vec<Vec<P3>>, EvalError> {
    let cutter = Cutter::new(p.tool.clone(), o.limits.leave_radial);
    let (grid, inside, lo, hi) = area(p, o)?;
    let mut lines = morph_xy(grid, inside, o.step, o.clockwise);
    if lines.is_empty() {
        return Err(invalid(
            "削る所がありません(加工の範囲が送り幅より狭いです)",
        ));
    }
    if o.outward {
        for l in &mut lines {
            l.reverse();
        }
    }
    let h = heights(p, o, &cutter, lo, hi);
    follow(p, o, &cutter, &h, &lines)
}

/// 平面の線を作る範囲: 送り幅の 1/6 ほどの格子(線の滑らかさに足りて、速い)と、その節点の
/// 範囲の印、範囲の外形の箱。範囲を決めない時は、部品の外形(工具の半径だけ外まで)。
pub(crate) fn area(p: &Planner, o: &MorphOpts) -> Result<(Grid, Vec<bool>, P2, P2), EvalError> {
    let r = p.r();
    let (m0, m1) = (p.model.min, p.model.max);
    let (mut lo, mut hi) = ([m0[0] - r, m0[1] - r], [m1[0] + r, m1[1] + r]);
    if let Some(reg) = o.limits.region.as_ref().filter(|g| !g.is_empty()) {
        let (a, b) = reg.bounds();
        lo = [lo[0].max(a[0]), lo[1].max(a[1])];
        hi = [hi[0].min(b[0]), hi[1].min(b[1])];
    }
    if lo[0] >= hi[0] || lo[1] >= hi[1] {
        return Err(invalid(
            "削る所がありません(加工の範囲が部品から外れています)",
        ));
    }
    let g = (o.step / 6.0).clamp(o.cell, 1.0);
    let grid = Grid {
        origin: [lo[0] - g, lo[1] - g],
        cell: g,
        nx: ((hi[0] - lo[0]) / g).ceil() as usize + 3,
        ny: ((hi[1] - lo[1]) / g).ceil() as usize + 3,
    };
    let inside: Vec<bool> = {
        let boxed = (0..grid.ny)
            .flat_map(|j| (0..grid.nx).map(move |i| (i, j)))
            .map(|(i, j)| {
                let q = grid.at(i, j);
                (lo[0]..=hi[0]).contains(&q[0]) && (lo[1]..=hi[1]).contains(&q[1])
            });
        match o.limits.region.as_ref() {
            Some(reg) => reg
                .mask(&grid)
                .into_iter()
                .zip(boxed)
                .map(|(x, y)| x && y)
                .collect(),
            None => boxed.collect(),
        }
    };
    Ok((grid, inside, lo, hi))
}

/// 範囲 `lo`〜`hi` に工具を下ろすための、部品の網の高さ。
pub(crate) fn heights(p: &Planner, o: &MorphOpts, cutter: &Cutter, lo: P2, hi: P2) -> Heights {
    let pad = cutter.radius() + 2.0 * o.cell;
    Heights::new(
        [lo[0] - pad, lo[1] - pad],
        [hi[0] + pad, hi[1] + pad],
        o.cell,
        o.floor,
        &p.model.triangles,
    )
}

/// 平面の線を細かく分け、点ごとに工具を下ろしてなぞる点の続きにする(範囲・傾斜の外は抜く)。
pub(crate) fn follow(
    p: &Planner,
    o: &MorphOpts,
    cutter: &Cutter,
    h: &Heights,
    lines: &[Vec<P2>],
) -> Result<Vec<Vec<P3>>, EvalError> {
    let cell = o.cell;
    let lim = &o.limits;
    let d = slope_span(cell) as f64 * cell;
    let point = |q: P2| {
        let z = h.exact_at(cutter, q).max(o.floor);
        let mut keep = lim.inside(q);
        if keep && lim.slope.is_some() {
            let f = |x: f64, y: f64| h.exact_at(cutter, [x, y]);
            let gx = (f(q[0] + d, q[1]) - f(q[0] - d, q[1])) / (2.0 * d);
            let gy = (f(q[0], q[1] + d) - f(q[0], q[1] - d)) / (2.0 * d);
            keep = lim.slope_ok(gx.hypot(gy).atan());
        }
        Pt {
            q: [q[0], q[1], z + o.leave],
            keep,
        }
    };
    let dense: Vec<Vec<P2>> = lines.iter().map(|l| densify(l, cell)).collect();
    let found =
        kmb_cam_dropcutter::par_map(&dense, |l| l.iter().map(|&q| point(q)).collect::<Vec<_>>());
    let pieces: Vec<Vec<P3>> = found
        .iter()
        .flat_map(|pts| split(pts, BRIDGE * p.tool.diameter))
        .filter(|s| s.len() >= 2)
        .map(|s| kmb_cam_dropcutter::simplify(&s))
        .collect();
    let top = p.info.stock_max[2];
    let lowest = pieces
        .iter()
        .flatten()
        .map(|q| q[2])
        .fold(f64::INFINITY, f64::min);
    if pieces.is_empty() || lowest >= top - 1e-6 {
        return Err(invalid(if pieces.is_empty() {
            "削る所がありません(加工の範囲・傾斜の範囲に入る所がありません)"
        } else {
            "削る所がありません(部品がストックの上面より上)"
        }));
    }
    p.check_depth(top, lowest)?;
    Ok(pieces)
}

/// 線 `pts` を、点の間がほぼ `ds` 以下になるよう細かくする。
pub(crate) fn densify(pts: &[P2], ds: f64) -> Vec<P2> {
    let mut out = Vec::with_capacity(pts.len());
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let n = ((dist(a, b) / ds).ceil() as usize).max(1);
        for k in 0..n {
            let t = k as f64 / n as f64;
            out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
        }
    }
    if let Some(&l) = pts.last() {
        out.push(l);
    }
    out
}

impl Morph {
    pub(crate) fn opts(&self, ctx: &EvalCtx<'_>, p: &mut Prepared) -> Result<MorphOpts, EvalError> {
        let leave = length(ctx, &self.leave, "残し量")?;
        let cell = length(ctx, &self.resolution, "格子の幅")?;
        if !(0.01..=0.5).contains(&cell) {
            return Err(invalid("格子の幅は 0.01〜0.5 mm にしてください"));
        }
        let mut floor = p.info.stock_min[2];
        if let Some(d) = &self.depth {
            let d = length(ctx, d, "なぞる深さ")?;
            if d <= 0.0 {
                return Err(invalid("なぞる深さは 0 より大きくしてください"));
            }
            floor = floor.max(p.model.max[2] - d);
        } else {
            floor = p.reachable_bottom(p.info.stock_max[2], floor);
        }
        let step = match &self.step {
            Some(s) => length(ctx, s, "送り幅")?,
            None => p.tool.diameter * p.cutting.stepover,
        };
        if step < 2.0 * cell {
            return Err(invalid("送り幅は格子の幅の 2 倍より広くしてください"));
        }
        let limits = self.limits.resolve(ctx, p, cell.max(0.05))?;
        Ok(MorphOpts {
            leave,
            cell,
            floor,
            step,
            outward: self.outward,
            clockwise: self.clockwise,
            limits,
        })
    }
}

/// 戦略の純関数: 解いた入力(`params` は [`MorphOpts`])だけから経路を作る。
pub fn plan(input: &StrategyInput) -> Result<StrategyOutput, String> {
    let p = Planner::new(input);
    let o: MorphOpts = input.params()?;
    let passes = morph_passes(&p, &o).map_err(plan_error)?;
    let mut tp = p.toolpath();
    cut_passes(&p, &mut tp, passes);
    Ok(p.output(tp))
}

impl Feature for Morph {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = self.base.inputs()?;
        v.extend(expr_refs(
            [&self.leave, &self.resolution]
                .into_iter()
                .chain(self.limits.exprs())
                .chain([&self.step, &self.depth].into_iter().flatten()),
        )?);
        Ok(v)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let mut p = prepare(ctx, &self.base)?;
        if p.tool.kind == ToolKind::Drill {
            return Err(invalid(
                "モーフィング スパイラルの加工にはエンドミル(平かボール)を使ってください",
            ));
        }
        let o = self.opts(ctx, &mut p)?;
        let input = p.input(ctx.feature_id(), o, Default::default());
        let out = plan(&input);
        p.finish_plan(self.base.setup, out)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        let mut c = self.clone();
        let mut fields = vec![&mut c.leave, &mut c.resolution];
        fields.extend(c.limits.exprs_mut());
        fields.extend([&mut c.step, &mut c.depth].into_iter().flatten());
        Ok(c.base
            .rename(fields, old, new)?
            .then(|| serde_json::to_value(&c).expect("JSON にできる")))
    }
}

/// モーフィング スパイラルとスキャロップの欄(送り幅の説明だけ違う)。
pub(crate) fn fields(step_help: &str) -> Vec<Field> {
    let mut fields = vec![
        Field::expr("leave", "残し量").length(),
        leave_radial_field(),
        Field::expr("resolution", "格子の幅").length(),
        Field::opt_expr("step", "送り幅").length().help(step_help),
        Field::opt_expr("depth", "なぞる深さ")
            .length()
            .help("空ならストックの底まで(刃長まで)"),
        Field::bool("outward", "内から外へ").help("切れば範囲の縁から中へ回る"),
        Field::bool("clockwise", "時計回り").help("上から見て時計回りに回る"),
    ];
    fields.extend(slope_fields());
    fields.extend(boundary_fields());
    fields
}

pub fn register(reg: &mut Registry) {
    let fields = fields("輪の間隔の上限。空なら工具の送り幅。細い所・鋭い角の近くでは輪が詰まる");
    reg.register_described(
        kmb_cam_setup::operation(
            MORPH3D,
            "モーフィング スパイラル(3D)",
            "加工の範囲の形に沿って縮んでいく 1 本の渦巻きで、なだらかな面を持ち上げずに仕上げる",
            fields,
        )
        .icon("morph3d"),
        |d| Ok(Box::new(kmb_cam_setup::parse::<Morph>(d)?)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 格子(幅 `cell`)の上の、印 `f(x, y)` の範囲。
    fn shape(w: f64, h: f64, cell: f64, f: impl Fn(f64, f64) -> bool) -> (Grid, Vec<bool>) {
        let grid = Grid {
            origin: [-1.0, -1.0],
            cell,
            nx: ((w + 2.0) / cell) as usize + 1,
            ny: ((h + 2.0) / cell) as usize + 1,
        };
        let m = (0..grid.ny)
            .flat_map(|j| (0..grid.nx).map(move |i| (i, j)))
            .map(|(i, j)| {
                let q = grid.at(i, j);
                f(q[0], q[1])
            })
            .collect();
        (grid, m)
    }

    /// 範囲の中の点から線までの距離の最大(格子の点で測る)。線の点にいちばん近い節点に印を付け、
    /// 印までの距離を距離変換で求める(印の丸めで、格子の対角の半分まで大きく出る)。
    fn worst_gap(lines: &[Vec<P2>], grid: &Grid, inside: &[bool]) -> f64 {
        let mut on = vec![false; grid.nx * grid.ny];
        for q in lines.iter().flat_map(|l| densify(l, 0.5 * grid.cell)) {
            let i = ((q[0] - grid.origin[0]) / grid.cell).round() as usize;
            let j = ((q[1] - grid.origin[1]) / grid.cell).round() as usize;
            if i < grid.nx && j < grid.ny {
                on[j * grid.nx + i] = true;
            }
        }
        distance2(&on, grid.nx, grid.ny)
            .iter()
            .zip(inside)
            .filter(|(_, m)| **m)
            .map(|(d2, _)| d2.sqrt() * grid.cell)
            .fold(0.0, f64::max)
    }

    /// 線の上の、2 mm 先との向きの差の最大(度)。`skip` が真の点は測らない。
    fn sharpest(line: &[P2], skip: impl Fn(P2) -> bool) -> f64 {
        let d = densify(line, 0.1);
        let k = 20;
        (k..d.len().saturating_sub(k))
            .filter(|&i| !skip(d[i]))
            .map(|i| {
                let (a, b, c) = (d[i - k], d[i], d[i + k]);
                let u = (b[1] - a[1]).atan2(b[0] - a[0]);
                let v = (c[1] - b[1]).atan2(c[0] - b[0]);
                ((v - u + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
                    - std::f64::consts::PI)
                    .abs()
                    .to_degrees()
            })
            .fold(0.0, f64::max)
    }

    #[test]
    fn a_rectangle_is_one_spiral_with_no_gap_and_rounded_inner_turns() {
        let (grid, m) = shape(40.0, 24.0, 0.2, |x, y| {
            (0.0..=40.0).contains(&x) && (0.0..=24.0).contains(&y)
        });
        let lines = morph_xy(grid, m.clone(), 2.0, false);
        assert_eq!(lines.len(), 1, "{}", lines.len());
        let gap = worst_gap(&lines, &grid, &m);
        assert!(gap <= 1.0 + 0.15, "最大の隙間 {gap}");
        // 外の輪は長方形の角で曲がるが、内へ行くほど丸い(外から 1/3 より内では 2 mm 先で 60° 未満)。
        // ただし最後の輪は、距離の場の稜線(長方形の中の線分 (12, 12)〜(28, 12))を細長く囲み、
        // その両端で半径 1 mm ほどで折り返す。形から来る物なので、稜線から 4 mm 以内は測らない
        let l = &lines[0];
        let inner = &l[l.len() * 2 / 3..];
        let near_ridge = |q: P2| (q[1] - 12.0).hypot(q[0] - q[0].clamp(12.0, 28.0)) < 4.0;
        let sharp = sharpest(inner, near_ridge);
        assert!(sharp < 60.0, "{sharp}");
        assert!(
            sharpest(&l[..l.len() / 8], |_| false) > 80.0,
            "外の輪は角で曲がる"
        );
        // 反時計回りに回る
        assert!(signed_area(&l[..l.len() / 8]) > 0.0 || signed_area(&l[..]) > 0.0);
    }

    #[test]
    fn an_l_shape_is_covered_including_the_arm_tips() {
        let (grid, m) = shape(50.0, 30.0, 0.2, |x, y| {
            ((0.0..=50.0).contains(&x) && (0.0..=12.0).contains(&y))
                || ((0.0..=15.0).contains(&x) && (0.0..=30.0).contains(&y))
        });
        let lines = morph_xy(grid, m.clone(), 2.0, false);
        let gap = worst_gap(&lines, &grid, &m);
        assert!(gap <= 1.0 + 0.15, "最大の隙間 {gap}");
    }

    #[test]
    fn clockwise_turns_the_other_way_and_starts_on_the_boundary() {
        let (grid, m) = shape(20.0, 20.0, 0.2, |x, y| (x - 10.0).hypot(y - 10.0) <= 10.0);
        let ccw = morph_xy(grid, m.clone(), 2.0, false);
        let cw = morph_xy(grid, m, 2.0, true);
        let turn = |l: &Vec<P2>| signed_area(&l[..l.len() / 3]);
        assert!(turn(&ccw[0]) > 0.0 && turn(&cw[0]) < 0.0);
        // 始まりは範囲の縁(中心から 10 mm)
        let r = (ccw[0][0][0] - 10.0).hypot(ccw[0][0][1] - 10.0);
        assert!((r - 10.0).abs() < 0.3, "{r}");
    }
}
