//! 3D の走査線加工(`cam.raster3d@1`)。工具を X(か Y、または `angle` の角度)の向きに
//! 往復させ、各点で工具を部品に触れるまで下ろした高さ(ドロップカッター)をなぞる。
//! 曲面や斜面、フィレット、面取りの仕上げに使う。
//!
//! 角度を指定した時は、部品を逆向きに回した座標で X の向きに往復し、点を元の座標へ戻す。
//! 行は、部品の外形より工具の半径だけ外の長方形(作業座標系)に収める。
//!
//! 下ろした高さは `kmb-cam-dropcutter` で求める(安全側に倒した格子)。
//! `stepdown` ごとの段に分けて上から下ろす(`levels: false` なら最後の段だけ)。
//!
//! 3D に共通の欄([`Limits`]): 径方向に残す厚さは工具を太らせて下ろし、加工の範囲の外と傾斜の
//! 範囲の外の点は削らない(短い切れ目は面の上をなぞってつなぎ、長い切れ目は上げて移る)。
//! 向き(`direction`)を一方向にすると、どの行も同じ向きに削り(切上げ・切下げ)、行の終わりで
//! 上げて次の行の始まりへ移る。
//!
//! 同じ下ろし方で、渦巻き(`cam.spiral3d@1`)と放射(`cam.radial3d@1`)の線もなぞる
//! ([`Circular`])。スケッチの線を部品へ落としてなぞる投影(`cam.project3d@1`、[`Project`])も同じ
//! 下ろし方を使う。範囲の形に沿って縮む 1 本の渦巻きをなぞるモーフィング スパイラル
//! (`cam.morph3d@1`、[`Morph`])も同じ。範囲の縁から面に沿って一定の間隔でずらした輪を
//! なぞるスキャロップ(`cam.scallop3d@1`、[`Scallop`])は、モーフィング スパイラルと欄を共にする。

use kmb_cam_dropcutter::limits::{boundary_fields, leave_radial_field, slope_fields, slope_span};
use kmb_cam_dropcutter::{Cutter, Heights, Limits, Resolved};
use kmb_cam_setup::{
    OpBase, Planner, Prepared, StrategyInput, StrategyOutput, approach, expr_refs, invalid, length,
    plan_error, prepare,
};
use kmb_cam_tool::ToolKind;
use kmb_core::{Dim, Field};
use kmb_core::{EvalCtx, EvalError, Feature, InputRef, Outputs, Registry};
use kmb_toolpath::{Move, P3};
use serde::{Deserialize, Serialize};

pub const RASTER3D: &str = "cam.raster3d@1";

mod circular;
mod morph;
mod project;
mod scallop;
pub use circular::{
    CircOpts, Circular, Pattern, RADIAL3D, SPIRAL3D, circular_passes, radial_xy, spiral_xy,
};
pub use morph::{MORPH3D, Morph, MorphOpts, morph_passes, morph_xy};
pub use project::{PROJECT3D, ProjOpts, Project, project_passes};
pub use scallop::{SCALLOP3D, Scallop, scallop_passes, scallop_xy};

/// 往復の向き。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Along {
    #[default]
    X,
    Y,
}

/// 行を削る向き。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RasterDir {
    /// 往復(行ごとに向きを変える)
    #[default]
    Both,
    /// 一方向のダウンカット(切下げ。次の行の側の材料を、進む向きの右に見る。主軸が時計回りの時)
    Climb,
    /// 一方向のアップカット(切上げ)
    Conventional,
}

impl RasterDir {
    pub fn is_both(&self) -> bool {
        *self == RasterDir::Both
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Raster3d {
    #[serde(flatten)]
    pub base: OpBase,
    /// 部品の面の上に残す厚さ(軸方向。上向きに測る)
    #[serde(default = "zero")]
    pub leave: String,
    #[serde(default)]
    pub along: Along,
    /// 往復の角度(X 軸から反時計回り、例: `45 deg`)。指定すると `along` より優先
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<String>,
    /// 部品の高さを調べる格子の幅
    #[serde(default = "resolution")]
    pub resolution: String,
    /// 段に分けて上から削るか
    #[serde(default = "yes")]
    pub levels: bool,
    /// 部品のいちばん高い所から、どこまで下をなぞるか(省くとストックの底まで)。
    /// 上の方の曲面や面取りだけを仕上げる時に、外形の外を深く空削りしないために使う
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<String>,
    /// 径方向に残す厚さ・加工の範囲・傾斜の範囲
    #[serde(flatten)]
    pub limits: Limits,
    /// 行を削る向き(往復か一方向か)
    #[serde(default, skip_serializing_if = "RasterDir::is_both")]
    pub direction: RasterDir,
    /// 入る位置(作業座標系の XY)。往復の端のうち、ここに近い方から始める。空なら自動
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<[f64; 2]>,
}

pub(crate) fn zero() -> String {
    "0 mm".into()
}

pub(crate) fn resolution() -> String {
    "0.05 mm".into()
}

fn yes() -> bool {
    true
}

/// 走査線の点。`keep` が偽なら範囲か傾斜の外(削らない)。
#[derive(Debug, Clone, Copy, PartialEq)]
struct Pt {
    q: P3,
    keep: bool,
}

/// 経路の計算の設定(式を数にしたもの)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Opts {
    /// 軸方向に残す厚さ
    pub leave: f64,
    pub cell: f64,
    /// これより下はなぞらない
    pub floor: f64,
    /// 往復の角度(ラジアン)。`None` なら `along`
    pub angle: Option<f64>,
    pub along: Along,
    pub levels: bool,
    pub direction: RasterDir,
    /// 径方向に残す厚さ・範囲・傾斜
    pub limits: Resolved,
    /// 入る位置(作業座標系の XY)。端のうち近い方から始める
    pub entry: Option<[f64; 2]>,
}

/// 範囲の外の点が続く所を、面をなぞってつなぐ長さの上限(工具の直径の倍数)。
const BRIDGE: f64 = 2.0;

/// 行の点を、削る点の続きに分ける。削らない点が短く続くだけの所(両側に削る点がある)は、
/// 面をなぞってつなぐ。
fn split(seg: &[Pt], bridge: f64) -> Vec<Vec<P3>> {
    let n = seg.len();
    let mut use_pt: Vec<bool> = seg.iter().map(|p| p.keep).collect();
    let mut k = 0;
    while k < n {
        if seg[k].keep {
            k += 1;
            continue;
        }
        let s = k;
        while k < n && !seg[k].keep {
            k += 1;
        }
        // seg[s..k] が削らない点
        if s > 0 && k < n {
            let len: f64 = (s - 1..k)
                .map(|m| {
                    let (a, b) = (seg[m].q, seg[m + 1].q);
                    (b[0] - a[0]).hypot(b[1] - a[1])
                })
                .sum();
            if len <= bridge {
                use_pt[s..k].fill(true);
            }
        }
    }
    let mut out = Vec::new();
    let mut cur: Vec<P3> = Vec::new();
    for (p, &u) in seg.iter().zip(&use_pt) {
        if u {
            cur.push(p.q);
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// 節点 (i, j) での下ろした高さの面の傾き(ラジアン)。前後 `s` 節点の差で測る。
fn slope_at(h: &Heights, drop: &dyn Fn(usize, usize) -> f64, i: usize, j: usize, s: usize) -> f64 {
    let d = |a: usize, n: usize, f: &dyn Fn(usize) -> f64| {
        let (lo, hi) = (a.saturating_sub(s), (a + s).min(n - 1));
        if hi == lo {
            0.0
        } else {
            (f(hi) - f(lo)) / ((hi - lo) as f64 * h.cell)
        }
    };
    let gx = d(i, h.nx, &|x| drop(x, j));
    let gy = d(j, h.ny, &|y| drop(i, y));
    gx.hypot(gy).atan()
}

/// 角度 `theta`(ラジアン)の往復の、工具の先端が通る点の続き(作業座標系)。部品を
/// `-theta` 回した座標で X の向きに往復し、長方形 `lo`〜`hi`(作業座標系)の中の節点だけを
/// 通る。往復なら行から行へは真っすぐ移り、移る間も工具を下ろした高さを調べる(続きは 1 つ)。
/// 一方向なら行ごとに分け、どの行も同じ向きにする。
fn angled_surface(
    p: &Planner,
    o: &Opts,
    theta: f64,
    cutter: &Cutter,
    (lo, hi): ([f64; 2], [f64; 2]),
) -> Vec<Vec<Pt>> {
    let (cell, floor, leave) = (o.cell, o.floor, o.leave);
    let (c, s) = (theta.cos(), theta.sin());
    let fwd = |q: [f64; 2]| [c * q[0] + s * q[1], -s * q[0] + c * q[1]];
    let back = |q: [f64; 2]| [c * q[0] - s * q[1], s * q[0] + c * q[1]];
    let tris: Vec<[[f64; 3]; 3]> = p
        .model
        .triangles
        .iter()
        .map(|t| {
            t.map(|v| {
                let w = fwd([v[0], v[1]]);
                [w[0], w[1], v[2]]
            })
        })
        .collect();
    let corners = [lo, [hi[0], lo[1]], hi, [lo[0], hi[1]]].map(fwd);
    let ext = |axis: usize, f: fn(f64, f64) -> f64, init: f64| {
        corners.iter().map(|q| q[axis]).fold(init, f)
    };
    let flo = [
        ext(0, f64::min, f64::INFINITY),
        ext(1, f64::min, f64::INFINITY),
    ];
    let fhi = [
        ext(0, f64::max, f64::NEG_INFINITY),
        ext(1, f64::max, f64::NEG_INFINITY),
    ];
    let pad = cutter.radius() + 2.0 * cell;
    let h = Heights::new(
        [flo[0] - pad, flo[1] - pad],
        [fhi[0] + pad, fhi[1] + pad],
        cell,
        floor,
        &tris,
    );
    let fp = h.footprint(cutter);
    let inside = |q: [f64; 2]| {
        let w = back(q);
        (lo[0] - 1e-9..=hi[0] + 1e-9).contains(&w[0])
            && (lo[1] - 1e-9..=hi[1] + 1e-9).contains(&w[1])
    };
    let (i0, i1) = (h.index(flo[0], 0), h.index(fhi[0], 0));
    let (j0, j1) = (h.index(flo[1], 1), h.index(fhi[1], 1));
    let step = ((p.tool.diameter * p.cutting.stepover / cell).floor() as usize).max(1);
    let mut rows: Vec<usize> = (j0..=j1).step_by(step).collect();
    if rows.last() != Some(&j1) {
        rows.push(j1);
    }
    let lim = &o.limits;
    let span = slope_span(cell);
    let drop = |i: usize, j: usize| h.drop_node(&fp, i, j);
    // 行ごとの下ろした高さは互いに関係しないので、スレッドに分ける
    let found = kmb_cam_dropcutter::par_map(&rows, |&j| {
        (i0..=i1)
            .filter(|&i| inside(h.at(i, j)))
            .map(|i| {
                let q = h.at(i, j);
                let keep = lim.inside(back(q))
                    && (lim.slope.is_none() || lim.slope_ok(slope_at(&h, &drop, i, j, span)));
                Pt {
                    q: [q[0], q[1], drop(i, j) + leave],
                    keep,
                }
            })
            .collect::<Vec<_>>()
    });
    let world = |v: Vec<Pt>| -> Vec<Pt> {
        v.into_iter()
            .map(|p| {
                let w = back([p.q[0], p.q[1]]);
                Pt {
                    q: [w[0], w[1], p.q[2]],
                    keep: p.keep,
                }
            })
            .collect()
    };
    if !o.direction.is_both() {
        // 行は +y の向きに進むので、ダウンカットは -x の向きに削る
        let rev = o.direction == RasterDir::Climb;
        return found
            .into_iter()
            .filter(|r| r.len() >= 2)
            .map(|mut r| {
                if rev {
                    r.reverse();
                }
                world(r)
            })
            .collect();
    }
    let mut surface: Vec<Pt> = Vec::new();
    let mut prev: Option<[f64; 2]> = None;
    let mut n = 0;
    for mut row in found {
        if row.is_empty() {
            continue;
        }
        if n % 2 == 1 {
            row.reverse();
        }
        n += 1;
        let first = [row[0].q[0], row[0].q[1]];
        if let Some(a) = prev {
            let len = (first[0] - a[0]).hypot(first[1] - a[1]);
            let m = (len / cell).ceil() as usize;
            for k in 1..m {
                let t = k as f64 / m as f64;
                let q = [a[0] + (first[0] - a[0]) * t, a[1] + (first[1] - a[1]) * t];
                surface.push(Pt {
                    q: [q[0], q[1], h.drop_at(cutter, q) + leave],
                    keep: lim.inside(back(q)),
                });
            }
        }
        let last = row[row.len() - 1];
        prev = Some([last.q[0], last.q[1]]);
        surface.extend(row);
    }
    vec![world(surface)]
}

/// 往復の角度の式を読む(ラジアン)。
fn angle_of(ctx: &EvalCtx<'_>, src: &str) -> Result<f64, EvalError> {
    let q = ctx.expr(src)?;
    if q.dim != Dim::ANGLE {
        return Err(invalid(format!(
            "往復の角度 `{src}` は角度の単位(deg)が必要です(値: {q})"
        )));
    }
    Ok(q.value)
}

/// 段ごとの、工具の先端が通る点の列(式を数にした後の計算)。
pub fn raster_passes(p: &Planner, o: &Opts) -> Result<Vec<Vec<P3>>, EvalError> {
    let cutter = Cutter::new(p.tool.clone(), o.limits.leave_radial);
    let r = p.r();
    let (m0, m1) = (p.model.min, p.model.max);
    // 工具の中心が動く範囲(部品の外形より工具の半径だけ外まで)
    let lo = [m0[0] - r, m0[1] - r];
    let hi = [m1[0] + r, m1[1] + r];
    let segs = match o.angle {
        Some(theta) => angled_surface(p, o, theta, &cutter, (lo, hi)),
        None => axis_surface(p, o, &cutter, (lo, hi)),
    };
    let bridge = BRIDGE * p.tool.diameter;
    let pieces: Vec<Vec<P3>> = if o.limits.limits_anything() {
        segs.iter()
            .flat_map(|s| split(s, bridge))
            .filter(|s| s.len() >= 2)
            .collect()
    } else {
        segs.into_iter()
            .map(|s| s.into_iter().map(|p| p.q).collect())
            .collect()
    };
    // 入る位置があれば、往復の端のうち近い方から始める
    let mut pieces = pieces;
    if let (Some(e), Some(f), Some(l)) = (
        o.entry,
        pieces.first().and_then(|s| s.first()),
        pieces.last().and_then(|s| s.last()),
    ) {
        let d = |q: &P3| (q[0] - e[0]).hypot(q[1] - e[1]);
        if d(l) < d(f) {
            pieces.reverse();
            for piece in &mut pieces {
                piece.reverse();
            }
        }
    }
    let top = p.info.stock_max[2];
    let lowest = pieces
        .iter()
        .flatten()
        .map(|q| q[2])
        .fold(f64::INFINITY, f64::min)
        .max(o.floor);
    if pieces.is_empty() || lowest >= top - 1e-6 {
        return Err(invalid(if pieces.is_empty() {
            "削る所がありません(加工の範囲か傾斜の範囲に入る所がありません)"
        } else {
            "削る所がありません(部品がストックの上面より上)"
        }));
    }
    p.check_depth(top, lowest)?;
    let levels = if o.levels {
        p.levels(top, lowest)
    } else {
        vec![lowest]
    };
    let mut passes = Vec::new();
    for (n, z) in levels.into_iter().enumerate() {
        let mut level: Vec<Vec<P3>> = pieces
            .iter()
            .map(|piece| piece.iter().map(|q| [q[0], q[1], q[2].max(z)]).collect())
            .collect();
        // 往復なら段ごとに向きを変え、前の段の終わりからそのまま下ろす
        if n % 2 == 1 && o.direction.is_both() {
            level.reverse();
            for piece in &mut level {
                piece.reverse();
            }
        }
        passes.extend(level.iter().map(|piece| kmb_cam_dropcutter::simplify(piece)));
    }
    Ok(passes)
}

impl Raster3d {
    /// 式を数にした設定。
    fn opts(&self, ctx: &EvalCtx<'_>, p: &mut Prepared) -> Result<Opts, EvalError> {
        let leave = length(ctx, &self.leave, "残し量")?;
        let cell = length(ctx, &self.resolution, "格子の幅")?;
        if !(0.005..=0.5).contains(&cell) {
            return Err(invalid("格子の幅は 0.005〜0.5 mm にしてください"));
        }
        let m1 = p.model.max;
        let mut floor = p.info.stock_min[2];
        if let Some(d) = &self.depth {
            let d = length(ctx, d, "なぞる深さ")?;
            if d <= 0.0 {
                return Err(invalid("なぞる深さは 0 より大きくしてください"));
            }
            floor = floor.max(m1[2] - d);
        } else {
            floor = p.reachable_bottom(p.info.stock_max[2], floor);
        }
        let angle = match &self.angle {
            Some(src) => Some(angle_of(ctx, src)?),
            None => None,
        };
        let limits = self.limits.resolve(ctx, p, cell.max(0.05))?;
        Ok(Opts {
            leave,
            cell,
            floor,
            angle,
            along: self.along,
            levels: self.levels,
            direction: self.direction,
            limits,
            entry: self.entry,
        })
    }
}

/// X か Y の向きの往復の、工具の先端が通る点の続き。
fn axis_surface(
    p: &Planner,
    o: &Opts,
    cutter: &Cutter,
    (lo, hi): ([f64; 2], [f64; 2]),
) -> Vec<Vec<Pt>> {
    let (cell, floor, leave) = (o.cell, o.floor, o.leave);
    let pad = cutter.radius() + 2.0 * cell;
    let h = Heights::new(
        [lo[0] - pad, lo[1] - pad],
        [hi[0] + pad, hi[1] + pad],
        cell,
        floor,
        &p.model.triangles,
    );
    let fp = h.footprint(cutter);
    let drop = |i: usize, j: usize| h.drop_node(&fp, i, j);
    let (i0, i1) = (h.index(lo[0], 0), h.index(hi[0], 0));
    let (j0, j1) = (h.index(lo[1], 1), h.index(hi[1], 1));
    let step = ((p.tool.diameter * p.cutting.stepover / cell).floor() as usize).max(1);
    // 往復の行(端の行は必ず入れる)
    let rows = |a: usize, b: usize| {
        let mut v: Vec<usize> = (a..=b).step_by(step).collect();
        if v.last() != Some(&b) {
            v.push(b);
        }
        v
    };
    let (outer, inner) = match o.along {
        Along::X => (rows(j0, j1), (i0, i1)),
        Along::Y => (rows(i0, i1), (j0, j1)),
    };
    let lim = &o.limits;
    let span = slope_span(cell);
    let node = |o2: usize, q: usize| match o.along {
        Along::X => (q, o2),
        Along::Y => (o2, q),
    };
    let point = |o2: usize, q: usize| {
        let (i, j) = node(o2, q);
        let at = h.at(i, j);
        let keep = lim.inside(at)
            && (lim.slope.is_none() || lim.slope_ok(slope_at(&h, &drop, i, j, span)));
        Pt {
            q: [at[0], at[1], drop(i, j) + leave],
            keep,
        }
    };
    // 行ごとの下ろした高さは互いに関係しないので、スレッドに分ける
    let found = kmb_cam_dropcutter::par_map(&outer, |&o2| {
        (inner.0..=inner.1)
            .map(|q| point(o2, q))
            .collect::<Vec<_>>()
    });
    if !o.direction.is_both() {
        // X の行は +Y へ進むので、ダウンカットは -X の向き。Y の行は +X へ進むので +Y の向き
        let rev = (o.direction == RasterDir::Climb) == (o.along == Along::X);
        return found
            .into_iter()
            .map(|mut r| {
                if rev {
                    r.reverse();
                }
                r
            })
            .collect();
    }
    // 往復の行を、行の端の節点を通る移りでつなぐ(移る間も工具を下ろした高さを調べる)
    let mut surface: Vec<Pt> = Vec::new();
    for (n, (&o2, mut row)) in outer.iter().zip(found).enumerate() {
        if n % 2 == 1 {
            row.reverse();
        }
        if n > 0 {
            let side = if n % 2 == 0 { inner.0 } else { inner.1 };
            for m in outer[n - 1] + 1..o2 {
                surface.push(point(m, side));
            }
        }
        surface.extend(row);
    }
    vec![surface]
}

/// 点の続きを順に削る(続きの始まりへは上から下ろす。前の続きの終わりの真下からならそのまま
/// 下ろす)。終わりに工具を上げる。
pub(crate) fn cut_passes(p: &Planner, tp: &mut kmb_toolpath::Toolpath, passes: Vec<Vec<P3>>) {
    let (feed, plunge) = (p.cutting.feed, p.cutting.plunge);
    for pass in passes {
        let Some(first) = pass.first() else {
            continue;
        };
        let here = tp.moves.last().map(|m| m.to());
        match here {
            // 前の段の終わりの真下から始まる時は、そのまま下ろす
            Some(h) if h[2] < tp.safe_z && h[0] == first[0] && h[1] == first[1] => {}
            _ => approach(tp, [first[0], first[1]]),
        }
        tp.moves.push(Move::Linear {
            to: *first,
            feed: plunge,
        });
        for q in &pass[1..] {
            tp.moves.push(Move::Linear { to: *q, feed });
        }
    }
    kmb_cam_setup::retract(tp);
}

/// 戦略の純関数: 解いた入力(`params` は [`Opts`])だけから経路を作る。
pub fn plan(input: &StrategyInput) -> Result<StrategyOutput, String> {
    let p = Planner::new(input);
    let o: Opts = input.params()?;
    let passes = raster_passes(&p, &o).map_err(plan_error)?;
    let mut tp = p.toolpath();
    cut_passes(&p, &mut tp, passes);
    Ok(p.output(tp))
}

impl Feature for Raster3d {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = self.base.inputs()?;
        v.extend(expr_refs([&self.leave, &self.resolution])?);
        for e in [&self.depth, &self.angle].into_iter().flatten() {
            v.extend(expr_refs([e])?);
        }
        v.extend(expr_refs(self.limits.exprs())?);
        Ok(v)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let mut p = prepare(ctx, &self.base)?;
        if p.tool.kind == ToolKind::Drill {
            return Err(invalid(
                "走査線の加工にはエンドミル(平かボール)を使ってください",
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
        for e in [&mut c.depth, &mut c.angle].into_iter().flatten() {
            fields.push(e);
        }
        fields.extend(c.limits.exprs_mut());
        Ok(c.base
            .rename(fields, old, new)?
            .then(|| serde_json::to_value(&c).expect("JSON にできる")))
    }
}

pub fn register(reg: &mut Registry) {
    let mut fields = vec![
        Field::expr("leave", "残し量").length(),
        leave_radial_field(),
        Field::choice("along", "向き", &[("x", "X"), ("y", "Y")]),
        Field::opt_expr("angle", "往復の角度")
            .angle()
            .help("X 軸から反時計回り。例: 45 deg。空なら「向き」に従う"),
        Field::choice(
            "direction",
            "削る向き",
            &[
                ("both", "往復"),
                ("climb", "一方向(ダウンカット)"),
                ("conventional", "一方向(アップカット)"),
            ],
        )
        .help("一方向は、どの行も同じ向きに削り、行の終わりで上げて次の行の始まりへ戻る"),
        Field::expr("resolution", "格子の幅").length(),
        Field::bool("levels", "段に分けて上から削る"),
        Field::opt_expr("depth", "なぞる深さ")
            .length()
            .help("空ならストックの底まで"),
    ];
    fields.extend(slope_fields());
    fields.extend(boundary_fields());
    reg.register_described(
        kmb_cam_setup::operation(
            RASTER3D,
            "走査線(3D)",
            "曲面や斜面を往復でなぞって仕上げる",
            fields,
        )
        .icon("raster"),
        |d| Ok(Box::new(kmb_cam_setup::parse::<Raster3d>(d)?)),
    );
    circular::register(reg);
    morph::register(reg);
    project::register(reg);
    scallop::register(reg);
}

/// この crate の名乗り: 登録する種類と、加工を足す操作(画面の名前・まとまり・初めの値)。
pub fn module() -> kmb_cam_setup::StrategyModule {
    use kmb_cam_setup::{Needs, Strategy, StrategyModule, fitted, to_data};
    use kmb_cam_tool::ToolKind;
    StrategyModule::new(
        "kmb-strategy-raster3d",
        register,
        vec![
            Strategy::new(RASTER3D, "cam.add.raster3d", "走査線(3D)", |p| {
                to_data(Raster3d {
                    base: fitted(p.with_tool(ToolKind::BallEnd)),
                    leave: "0 mm".into(),
                    along: Default::default(),
                    resolution: "0.05 mm".into(),
                    levels: true,
                    depth: None,
                    angle: None,
                    entry: None,
                    direction: Default::default(),
                    limits: Default::default(),
                })
            })
            .order(21)
            .menu("3D")
            .icon("raster")
            .aliases("そうさせん raster parallel 3d")
            .hint("曲面、斜面、面取りを X 方向の往復でなぞる(ボールエンドミルがあればそれを使う)")
            .entry("往復の端のうち、クリックした所に近い方から始める"),
            Strategy::new(SCALLOP3D, "cam.add.scallop3d", "スキャロップ(3D)", |p| {
                to_data(Scallop::new(fitted(p.with_tool(ToolKind::BallEnd))))
            })
            .order(22)
            .menu("3D")
            .icon("scallop3d")
            .aliases("すきゃろっぷ scallop 3d offset とうかんかく めんにそう")
            .hint("加工の範囲の縁から、面に沿って一定の間隔で内へずらした輪で仕上げる。急な所でも削り目がそろう。範囲が無ければ部品の外形(ボールエンドミルがあればそれを使う)")
            .no_reuse(),
            Strategy::new(SPIRAL3D, "cam.add.spiral3d", "渦巻き(3D)", |p| circular(p, Pattern::Spiral))
                .order(27)
                .menu("3D")
                .icon("spiral3d")
                .not_front()
                .aliases("うずまき spiral 3d まるい")
                .hint("部品の中心のまわりの渦巻きを外から内へなぞって仕上げる(ボールエンドミルがあればそれを使う)")
                .no_reuse(),
            Strategy::new(RADIAL3D, "cam.add.radial3d", "放射状(3D)", |p| circular(p, Pattern::Radial))
                .order(28)
                .menu("3D")
                .icon("radial3d")
                .not_front()
                .aliases("ほうしゃ radial 3d まるい")
                .hint("部品の中心から放射の線を外へ・内へとなぞって仕上げる(ボールエンドミルがあればそれを使う)")
                .no_reuse(),
            Strategy::new(MORPH3D, "cam.add.morph3d", "モーフィング スパイラル(3D)", |p| {
                to_data(Morph::new(fitted(p.with_tool(ToolKind::BallEnd))))
            })
            .order(29)
            .menu("3D")
            .icon("morph3d")
            .not_front()
            .aliases("もーふぃんぐ すぱいらる morph morphed spiral 3d うずまき らせん")
            .hint("加工の範囲の形に沿って縮む 1 本の渦巻きで、なだらかな面を持ち上げずに仕上げる。範囲が無ければ部品の外形(ボールエンドミルがあればそれを使う)")
            .no_reuse(),
            Strategy::new(PROJECT3D, "cam.add.project3d", "投影(3D)", |p| {
                to_data(Project::new(
                    fitted(p.with_tool(ToolKind::BallEnd)),
                    p.source("curves").ok_or("落とす線がありません")?,
                ))
            })
            .order(30)
            .menu("3D")
            .icon("project3d")
            .not_front()
            .aliases("とうえい project projection 3d せんをおとす もじ もよう")
            .hint("スケッチの線を真上から部品の面へ落としてなぞる。残し量を負にすると面より下を彫る。スケッチを選ぶとその線、選ばなければ最後のスケッチ(ボールエンドミルがあればそれを使う)")
            .needs(Needs::Curves)
            .no_reuse(),
        ],
    )
}

/// 渦巻きと放射の初めの値。新しく足す時は部品を上から見た外形の中だけ(四角い部品の外の空中を
/// 渦や放射で下りて削らないように)。欄で「限らない」に戻せる
fn circular(p: &dyn kmb_cam_setup::Picks, pattern: Pattern) -> Result<serde_json::Value, String> {
    let mut c = Circular::new(
        kmb_cam_setup::fitted(p.with_tool(ToolKind::BallEnd)),
        pattern,
    );
    c.limits.boundary = kmb_cam_dropcutter::Boundary::Silhouette;
    kmb_cam_setup::to_data(c)
}

#[cfg(test)]
mod tests;
