//! 3D の渦巻き(`cam.spiral3d@1`)と放射(`cam.radial3d@1`)の仕上げ。
//!
//! どちらも昔からある、平面の上の決まった形の線を部品の上へ下ろしてなぞる仕上げ。
//!
//! - 渦巻き: 中心のまわりのアルキメデスの渦巻き(1 周ごとに半径が送り幅ずつ減る)を、外から
//!   内へたどる。同心の輪を外から内へ等間隔にたどるのと同じ間隔になる
//! - 放射: 中心から等しい角度ごとの放射の線を、外へ・内へと交互にたどる。線の間隔は、外の半径で
//!   送り幅になる角度
//!
//! 線の点ごとに工具(径方向に残す厚さの分だけ太らせたもの)を下ろした高さに軸方向の残す厚さを
//! 足す。部品の外形(工具の半径だけ外まで)と加工の範囲・傾きの外は削らない(短い所は面をなぞって
//! つなぐ)。工具の負荷を見て線を変えることはしない。

use crate::{BRIDGE, Pt, cut_passes, split};
use crate::{resolution, zero};
use jc_cam_dropcutter::limits::{boundary_fields, leave_radial_field, slope_fields, slope_span};
use jc_cam_dropcutter::{Cutter, Heights, Limits, Resolved};
use jc_cam_setup::{
    OpBase, Planner, Prepared, StrategyInput, StrategyOutput, expr_refs, invalid, length,
    plan_error, prepare,
};
use jc_cam_tool::ToolKind;
use jc_core::{EvalCtx, EvalError, Feature, Field, InputRef, Outputs, Registry};
use jc_toolpath::P3;
use serde::{Deserialize, Serialize};
use std::f64::consts::TAU;

pub const SPIRAL3D: &str = "cam.spiral3d@1";
pub const RADIAL3D: &str = "cam.radial3d@1";

/// 線の形。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Pattern {
    #[default]
    Spiral,
    Radial,
}

/// 渦巻きか放射の仕上げ(どちらも同じ欄。形は機能の種類で決まる)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Circular {
    #[serde(flatten)]
    pub base: OpBase,
    /// 線の形(機能の種類から決まるので、文書には書かない)
    #[serde(skip)]
    pub pattern: Pattern,
    /// 軸方向に残す厚さ
    #[serde(default = "zero")]
    pub leave: String,
    /// 径方向に残す厚さ・加工の範囲・傾き
    #[serde(flatten)]
    pub limits: Limits,
    /// 部品の高さを調べる格子の幅(線の点の間隔も同じ)
    #[serde(default = "resolution")]
    pub resolution: String,
    /// 中心(セットアップの座標の XY)。空なら加工の範囲か部品の外形の中心
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub center: Option<[f64; 2]>,
    /// これより中心に近い所は削らない。空なら 0
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inner: Option<String>,
    /// これより中心から遠い所は削らない。空なら部品の外形の角まで
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outer: Option<String>,
    /// 線の間隔(渦巻きの 1 周ごとの間隔、放射の外の半径での間隔)。空なら工具の送り幅
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    /// 部品のいちばん高い所から、どこまで下をなぞるか(省くとストックの底まで)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<String>,
}

impl Circular {
    pub fn new(base: OpBase, pattern: Pattern) -> Circular {
        Circular {
            base,
            pattern,
            leave: zero(),
            limits: Limits::default(),
            resolution: resolution(),
            center: None,
            inner: None,
            outer: None,
            step: None,
            depth: None,
        }
    }

    pub fn type_id(&self) -> &'static str {
        match self.pattern {
            Pattern::Spiral => SPIRAL3D,
            Pattern::Radial => RADIAL3D,
        }
    }
}

/// 経路の計算の設定(式を数にしたもの)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircOpts {
    pub pattern: Pattern,
    pub leave: f64,
    pub cell: f64,
    /// これより下はなぞらない
    pub floor: f64,
    pub center: Option<[f64; 2]>,
    pub inner: f64,
    /// 空なら部品の外形の角まで
    pub outer: Option<f64>,
    pub step: f64,
    pub limits: Resolved,
}

/// 渦巻きの線(外から内へ)。点の間隔はほぼ `ds`。
pub fn spiral_xy(c: [f64; 2], inner: f64, outer: f64, pitch: f64, ds: f64) -> Vec<[f64; 2]> {
    let mut out = Vec::new();
    let (mut r, mut a) = (outer, 0.0f64);
    let k = pitch / TAU;
    while r > inner {
        out.push([c[0] + r * a.cos(), c[1] + r * a.sin()]);
        let da = (ds / r.max(ds)).min(0.2);
        a += da;
        r -= k * da;
    }
    out.push([c[0] + inner * a.cos(), c[1] + inner * a.sin()]);
    out
}

/// 放射の線(外へ・内へと交互)。各線の点の間隔はほぼ `ds`。
pub fn radial_xy(c: [f64; 2], inner: f64, outer: f64, step: f64, ds: f64) -> Vec<Vec<[f64; 2]>> {
    let n = ((TAU * outer / step).ceil() as usize).max(3);
    let m = (((outer - inner) / ds).ceil() as usize).max(1);
    (0..n)
        .map(|k| {
            let a = TAU * k as f64 / n as f64;
            let (ca, sa) = (a.cos(), a.sin());
            let mut v: Vec<[f64; 2]> = (0..=m)
                .map(|t| {
                    let r = inner + (outer - inner) * t as f64 / m as f64;
                    [c[0] + r * ca, c[1] + r * sa]
                })
                .collect();
            if k % 2 == 1 {
                v.reverse();
            }
            v
        })
        .collect()
}

/// 工具の先端が通る点の続き(作業座標系)。
pub fn circular_passes(p: &Planner, o: &CircOpts) -> Result<Vec<Vec<P3>>, EvalError> {
    let cutter = Cutter::new(p.tool.clone(), o.limits.leave_radial);
    let r = p.r();
    let (m0, m1) = (p.model.min, p.model.max);
    // 工具の中心が動ける範囲(部品の外形より工具の半径だけ外まで)
    let lo = [m0[0] - r, m0[1] - r];
    let hi = [m1[0] + r, m1[1] + r];
    let center = o.center.unwrap_or_else(|| match o.limits.region.as_ref() {
        Some(reg) if !reg.is_empty() => {
            let (a, b) = reg.bounds();
            [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]
        }
        _ => [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0],
    });
    let outer = o.outer.unwrap_or_else(|| {
        [lo, [hi[0], lo[1]], hi, [lo[0], hi[1]]]
            .iter()
            .map(|q| (q[0] - center[0]).hypot(q[1] - center[1]))
            .fold(0.0, f64::max)
    });
    if outer <= o.inner + 1e-6 {
        return Err(invalid("外の半径は内の半径より大きくしてください"));
    }
    let cell = o.cell;
    let pad = cutter.radius() + 2.0 * cell;
    let glo = [lo[0].max(center[0] - outer), lo[1].max(center[1] - outer)];
    let ghi = [hi[0].min(center[0] + outer), hi[1].min(center[1] + outer)];
    if glo[0] >= ghi[0] || glo[1] >= ghi[1] {
        return Err(invalid(
            "削る所がありません(中心と半径が部品から外れています)",
        ));
    }
    let h = Heights::new(
        [glo[0] - pad, glo[1] - pad],
        [ghi[0] + pad, ghi[1] + pad],
        cell,
        o.floor,
        &p.model.triangles,
    );
    let lines: Vec<Vec<[f64; 2]>> = match o.pattern {
        Pattern::Spiral => vec![spiral_xy(center, o.inner, outer, o.step, cell)],
        Pattern::Radial => radial_xy(center, o.inner, outer, o.step, cell),
    };
    let lim = &o.limits;
    let d = slope_span(cell) as f64 * cell;
    let in_box = |q: [f64; 2]| {
        (lo[0] - 1e-9..=hi[0] + 1e-9).contains(&q[0])
            && (lo[1] - 1e-9..=hi[1] + 1e-9).contains(&q[1])
    };
    let point = |q: [f64; 2]| {
        let z = h.drop_at(&cutter, q).max(o.floor);
        let mut keep = in_box(q) && lim.inside(q);
        if keep && lim.slope.is_some() {
            let f = |x: f64, y: f64| h.drop_at(&cutter, [x, y]);
            let gx = (f(q[0] + d, q[1]) - f(q[0] - d, q[1])) / (2.0 * d);
            let gy = (f(q[0], q[1] + d) - f(q[0], q[1] - d)) / (2.0 * d);
            keep = lim.slope_ok(gx.hypot(gy).atan());
        }
        Pt {
            q: [q[0], q[1], z + o.leave],
            keep,
        }
    };
    // 点ごとの下ろした高さは互いに関係しないので、まとめてスレッドに分ける
    let flat: Vec<[f64; 2]> = lines.iter().flatten().copied().collect();
    let chunks: Vec<&[[f64; 2]]> = flat.chunks(512).collect();
    let mut found =
        jc_cam_dropcutter::par_map(&chunks, |c| c.iter().map(|&q| point(q)).collect::<Vec<_>>())
            .into_iter()
            .flatten();
    // 放射の線は、外の端か内の端どうしを、面をなぞってつなぐ(一続きにする)
    let mut surface: Vec<Pt> = Vec::new();
    for line in &lines {
        let pts: Vec<Pt> = found.by_ref().take(line.len()).collect();
        if let (Some(a), Some(b)) = (surface.last(), pts.first()) {
            let (a, b) = ([a.q[0], a.q[1]], [b.q[0], b.q[1]]);
            let m = ((b[0] - a[0]).hypot(b[1] - a[1]) / cell).ceil() as usize;
            for k in 1..m {
                let t = k as f64 / m as f64;
                surface.push(point([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]));
            }
        }
        surface.extend(pts);
    }
    let pieces: Vec<Vec<P3>> = split(&surface, BRIDGE * p.tool.diameter)
        .into_iter()
        .filter(|s| s.len() >= 2)
        .map(|s| jc_cam_dropcutter::simplify(&s))
        .collect();
    let top = p.info.stock_max[2];
    let lowest = pieces
        .iter()
        .flatten()
        .map(|q| q[2])
        .fold(f64::INFINITY, f64::min);
    if pieces.is_empty() || lowest >= top - 1e-6 {
        return Err(invalid(if pieces.is_empty() {
            "削る所がありません(半径・加工の範囲・傾斜の範囲に入る所がありません)"
        } else {
            "削る所がありません(部品がストックの上面より上)"
        }));
    }
    p.check_depth(top, lowest)?;
    Ok(pieces)
}

impl Circular {
    fn opts(&self, ctx: &EvalCtx<'_>, p: &mut Prepared) -> Result<CircOpts, EvalError> {
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
        let inner = match &self.inner {
            Some(s) => length(ctx, s, "内の半径")?,
            None => 0.0,
        };
        if inner < 0.0 {
            return Err(invalid("内の半径は 0 以上にしてください"));
        }
        let outer = match &self.outer {
            Some(s) => Some(length(ctx, s, "外の半径")?),
            None => None,
        };
        let step = match &self.step {
            Some(s) => length(ctx, s, "線の間隔")?,
            None => p.tool.diameter * p.cutting.stepover,
        };
        if step < cell {
            return Err(invalid("線の間隔は格子の幅より広くしてください"));
        }
        let limits = self.limits.resolve(ctx, p, cell.max(0.05))?;
        Ok(CircOpts {
            pattern: self.pattern,
            leave,
            cell,
            floor,
            center: self.center,
            inner,
            outer,
            step,
            limits,
        })
    }
}

/// 戦略の純関数: 解いた入力(`params` は [`CircOpts`])だけから経路を作る。
pub fn plan(input: &StrategyInput) -> Result<StrategyOutput, String> {
    let p = Planner::new(input);
    let o: CircOpts = input.params()?;
    let passes = circular_passes(&p, &o).map_err(plan_error)?;
    let mut tp = p.toolpath();
    cut_passes(&p, &mut tp, passes);
    Ok(p.output(tp))
}

impl Feature for Circular {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = self.base.inputs()?;
        v.extend(expr_refs(
            [&self.leave, &self.resolution]
                .into_iter()
                .chain(self.limits.exprs())
                .chain(
                    [&self.inner, &self.outer, &self.step, &self.depth]
                        .into_iter()
                        .flatten(),
                ),
        )?);
        Ok(v)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let mut p = prepare(ctx, &self.base)?;
        if p.tool.kind == ToolKind::Drill {
            return Err(invalid(
                "渦巻き・放射の加工にはエンドミル(平かボール)を使ってください",
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
        fields.extend(
            [&mut c.inner, &mut c.outer, &mut c.step, &mut c.depth]
                .into_iter()
                .flatten(),
        );
        Ok(c.base
            .rename(fields, old, new)?
            .then(|| serde_json::to_value(&c).expect("JSON にできる")))
    }
}

pub fn register(reg: &mut Registry) {
    let fields = |step_help: &'static str| {
        let mut f = vec![
            Field::expr("leave", "残し量").length(),
            leave_radial_field(),
            Field::expr("resolution", "格子の幅").length(),
            Field::info("center", "中心").help("空なら加工の範囲か部品の外形の中心"),
            Field::opt_expr("inner", "内の半径")
                .length()
                .help("これより中心に近い所は削らない。空なら 0"),
            Field::opt_expr("outer", "外の半径")
                .length()
                .help("これより中心から遠い所は削らない。空なら部品の外形の角まで"),
            Field::opt_expr("step", "線の間隔").length().help(step_help),
            Field::opt_expr("depth", "なぞる深さ")
                .length()
                .help("空ならストックの底まで"),
        ];
        f.extend(slope_fields());
        f.extend(boundary_fields());
        f
    };
    reg.register_described(
        jc_cam_setup::operation(
            SPIRAL3D,
            "渦巻き(3D)",
            "中心のまわりの渦巻きを外から内へなぞり、丸い形や浅い曲面を仕上げる",
            fields("渦巻きの 1 周ごとの間隔。空なら工具の送り幅"),
        )
        .icon("spiral3d"),
        |d| {
            let mut c = jc_cam_setup::parse::<Circular>(d)?;
            c.pattern = Pattern::Spiral;
            Ok(Box::new(c))
        },
    );
    reg.register_described(
        jc_cam_setup::operation(
            RADIAL3D,
            "放射状(3D)",
            "中心から放射の線を等しい角度ごとに外へ・内へとなぞって仕上げる",
            fields("外の半径での線の間隔。空なら工具の送り幅"),
        )
        .icon("radial3d"),
        |d| {
            let mut c = jc_cam_setup::parse::<Circular>(d)?;
            c.pattern = Pattern::Radial;
            Ok(Box::new(c))
        },
    );
}
