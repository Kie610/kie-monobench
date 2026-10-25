//! 3D の投影(`cam.project3d@1`)。スケッチなどの線を工具の軸の向き(真上)から部品へ落とし、
//! 線の上の点ごとに工具を部品に触れるまで下ろした高さをなぞる。曲面の上への文字や模様の彫り、
//! 決めた線に沿った仕上げに使う(昔からある、線をドロップカッターで面へ落とすやり方)。
//!
//! - 工具の中心が線の真上を通る(左右の補正はしない)。線の平面は機械に置いた向きで水平
//! - 残し量を負にすると、面より下を彫る(彫った溝は形にないので、シミュレーションはその深さ
//!   までの食い込みを許す)
//! - 段に分けると、ストックの上面から工具の切り込みずつ下げ、各段では面と段の高い方をなぞる
//! - つながる線は持ち上げずに続け、閉じた線は同じ向き、開いた線は段ごとに行って戻る
//! - 加工の範囲・傾斜の範囲の外の点は削らない(短い切れ目は面をなぞってつなぐ)

use crate::{BRIDGE, Pt, cut_passes, split};
use crate::{resolution, zero};
use kmb_cam_dropcutter::limits::{boundary_fields, leave_radial_field, slope_fields, slope_span};
use kmb_cam_dropcutter::{Cutter, Heights, Limits, Resolved};
use kmb_cam_setup::{
    Chain, OpBase, Planner, Prepared, StrategyInput, StrategyOutput, expr_refs, invalid, length,
    output, plan_error, prepare,
};
use kmb_cam_tool::ToolKind;
use kmb_core::{EvalCtx, EvalError, Feature, FeatureId, Field, InputRef, Outputs, Registry};
use kmb_toolpath::P3;
use serde::{Deserialize, Serialize};

pub const PROJECT3D: &str = "cam.project3d@1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    #[serde(flatten)]
    pub base: OpBase,
    /// 落とす線を出す機能(スケッチなど)
    pub curves: FeatureId,
    /// 部品の面の上に残す厚さ(軸方向。負なら面より下を彫る)
    #[serde(default = "zero")]
    pub leave: String,
    /// 径方向に残す厚さ・加工の範囲・傾斜の範囲
    #[serde(flatten)]
    pub limits: Limits,
    /// 部品の高さを調べる格子の幅(線の上の点の間隔も同じ)
    #[serde(default = "resolution")]
    pub resolution: String,
    /// 段に分けて上から削るか
    #[serde(default)]
    pub levels: bool,
}

impl Project {
    pub fn new(base: OpBase, curves: FeatureId) -> Project {
        Project {
            base,
            curves,
            leave: zero(),
            limits: Limits::default(),
            resolution: resolution(),
            levels: false,
        }
    }
}

/// 経路の計算の設定(式を数にしたもの)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjOpts {
    pub leave: f64,
    pub cell: f64,
    /// これより下はなぞらない
    pub floor: f64,
    pub levels: bool,
    pub limits: Resolved,
}

/// 線 `pts` を、点の間がほぼ `ds` 以下になるよう細かくする。
fn densify(pts: &[[f64; 2]], ds: f64) -> Vec<[f64; 2]> {
    let mut out = Vec::with_capacity(pts.len());
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let n = (((b[0] - a[0]).hypot(b[1] - a[1]) / ds).ceil() as usize).max(1);
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

/// 線ごとの、段ごとの工具の先端が通る点の続き(作業座標系)。
pub fn project_passes(
    p: &Planner,
    chains: &[Chain],
    o: &ProjOpts,
) -> Result<Vec<Vec<P3>>, EvalError> {
    let cutter = Cutter::new(p.tool.clone(), o.limits.leave_radial);
    let cell = o.cell;
    let lines: Vec<(Vec<[f64; 2]>, bool)> = chains
        .iter()
        .map(|c| (densify(&c.pts, cell), c.closed))
        .filter(|(l, _)| l.len() >= 2)
        .collect();
    let all = lines.iter().flat_map(|(l, _)| l.iter());
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for q in all {
        for k in 0..2 {
            lo[k] = lo[k].min(q[k]);
            hi[k] = hi[k].max(q[k]);
        }
    }
    if lines.is_empty() {
        return Err(invalid("落とす線がありません"));
    }
    let pad = cutter.radius() + 2.0 * cell;
    let h = Heights::new(
        [lo[0] - pad, lo[1] - pad],
        [hi[0] + pad, hi[1] + pad],
        cell,
        o.floor,
        &p.model.triangles,
    );
    let lim = &o.limits;
    let d = slope_span(cell) as f64 * cell;
    let point = |q: [f64; 2]| {
        let z = h.drop_at(&cutter, q).max(o.floor);
        let mut keep = lim.inside(q);
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
    // 点ごとの下ろした高さは互いに関係しないので、線ごとにスレッドに分ける
    let found = kmb_cam_dropcutter::par_map(&lines, |(l, _)| {
        l.iter().map(|&q| point(q)).collect::<Vec<_>>()
    });
    let bridge = BRIDGE * p.tool.diameter;
    // 線ごとの、削る点の続き(閉じた線が範囲で切れなければ閉じたまま)
    let pieces: Vec<(Vec<Vec<P3>>, bool)> = found
        .iter()
        .zip(&lines)
        .map(|(pts, (_, closed))| {
            let s: Vec<Vec<P3>> = split(pts, bridge)
                .into_iter()
                .filter(|s| s.len() >= 2)
                .collect();
            let whole = s.len() == 1 && s[0].len() == pts.len();
            (s, *closed && whole)
        })
        .collect();
    let top = p.info.stock_max[2];
    let lowest = pieces
        .iter()
        .flat_map(|(s, _)| s.iter().flatten())
        .map(|q| q[2])
        .fold(f64::INFINITY, f64::min);
    if !lowest.is_finite() || lowest >= top - 1e-6 {
        return Err(invalid(if !lowest.is_finite() {
            "削る所がありません(線が加工の範囲か傾斜の範囲に入りません)"
        } else {
            "削る所がありません(線の下の部品がストックの上面より上)"
        }));
    }
    p.check_depth(top, lowest)?;
    let levels = if o.levels {
        p.levels(top, lowest)
    } else {
        vec![f64::NEG_INFINITY]
    };
    // 線ごとに段を下げていく(線から線へ移る回数を減らす)
    let mut passes = Vec::new();
    for (segs, closed) in &pieces {
        for seg in segs {
            let mut forward = true;
            for &z in &levels {
                let mut pass: Vec<P3> = seg.iter().map(|q| [q[0], q[1], q[2].max(z)]).collect();
                if !forward {
                    pass.reverse();
                }
                // 開いた線は、段ごとに行って戻る(持ち上げずに次の段へ)
                if !closed {
                    forward = !forward;
                }
                passes.push(kmb_cam_dropcutter::simplify(&pass));
            }
        }
    }
    Ok(passes)
}

impl Project {
    fn opts(&self, ctx: &EvalCtx<'_>, p: &mut Prepared) -> Result<ProjOpts, EvalError> {
        let leave = length(ctx, &self.leave, "残し量")?;
        let cell = length(ctx, &self.resolution, "格子の幅")?;
        if !(0.01..=0.5).contains(&cell) {
            return Err(invalid("格子の幅は 0.01〜0.5 mm にしてください"));
        }
        // 線が部品の外へ出ると工具はストックの底まで下りる。刃長より深い所は刃長まで(注意は
        // 実際にそこまで下りた時だけ出す。evaluate を見る)
        let top = p.info.stock_max[2];
        let floor = p.info.stock_min[2].max(top - p.tool.flute_length);
        let limits = self.limits.resolve(ctx, p, cell.max(0.05))?;
        Ok(ProjOpts {
            leave,
            cell,
            floor,
            levels: self.levels,
            limits,
        })
    }
}

/// ホストが解いた投影の値(`plan` の入力の `params`)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjPlan {
    pub o: ProjOpts,
    /// なぞるスケッチの線(作業座標系)
    pub chains: Vec<Chain>,
}

/// 戦略の純関数: 解いた入力だけから経路を作る。
pub fn plan(input: &StrategyInput) -> Result<StrategyOutput, String> {
    let mut p = Planner::new(input);
    let ProjPlan { o, chains } = input.params()?;
    let passes = project_passes(&p, &chains, &o).map_err(plan_error)?;
    let lowest = passes
        .iter()
        .flatten()
        .map(|q| q[2] - o.leave)
        .fold(f64::INFINITY, f64::min);
    if o.floor > p.info.stock_min[2] + 1e-9 && lowest <= o.floor + 1e-6 {
        // 刃長で止めた所がある(その下は残る)
        p.reachable_bottom(p.info.stock_max[2], p.info.stock_min[2]);
    }
    let mut tp = p.toolpath();
    // 面より下を彫る分は形にないので、シミュレーションはその深さまでの食い込みを許す
    if o.leave < 0.0 {
        tp.edge_break = -o.leave + 0.05;
    }
    cut_passes(&p, &mut tp, passes);
    Ok(p.output(tp))
}

impl Feature for Project {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = self.base.inputs()?;
        v.push(output(self.curves, "curves"));
        v.extend(expr_refs(
            [&self.leave, &self.resolution]
                .into_iter()
                .chain(self.limits.exprs()),
        )?);
        Ok(v)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let mut p = prepare(ctx, &self.base)?;
        if p.tool.kind == ToolKind::Drill {
            return Err(invalid(
                "投影の加工にはエンドミル(平・ボール・V ビット)を使ってください",
            ));
        }
        let o = self.opts(ctx, &mut p)?;
        let (_, chains) = kmb_cam_setup::sketch_chains(ctx, &p, self.curves)?;
        let input = p.input(ctx.feature_id(), ProjPlan { o, chains }, Default::default());
        let out = plan(&input);
        p.finish_plan(self.base.setup, out)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        let mut c = self.clone();
        let mut fields = vec![&mut c.leave, &mut c.resolution];
        fields.extend(c.limits.exprs_mut());
        Ok(c.base
            .rename(fields, old, new)?
            .then(|| serde_json::to_value(&c).expect("JSON にできる")))
    }
}

pub fn register(reg: &mut Registry) {
    let mut fields = vec![
        Field::output("curves", "落とす線", "curves"),
        Field::expr("leave", "残し量")
            .length()
            .help("面の上に残す厚さ。負にすると面より下を彫る(例: -0.3 mm)"),
        leave_radial_field(),
        Field::expr("resolution", "格子の幅").length(),
        Field::bool("levels", "段に分けて上から削る")
            .help("ストックの上面から工具の切り込みずつ下げる。切らなければ面の上を 1 回なぞる"),
    ];
    fields.extend(slope_fields());
    fields.extend(boundary_fields());
    reg.register_described(
        kmb_cam_setup::operation(
            PROJECT3D,
            "投影(3D)",
            "スケッチの線を真上から部品の面へ落としてなぞる。曲面の上の文字や模様、決めた線に沿った仕上げに",
            fields,
        )
        .icon("project3d"),
        |d| Ok(Box::new(kmb_cam_setup::parse::<Project>(d)?)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use kmb_cam_setup::testing::{mesh_of, prepared};
    use kmb_cam_tool::ToolLibrary;

    fn top(x0: f64, y0: f64, x1: f64, y1: f64, z: f64) -> Vec<[P3; 3]> {
        let (a, b, c, d) = ([x0, y0, z], [x1, y0, z], [x1, y1, z], [x0, y1, z]);
        vec![[a, b, c], [a, c, d]]
    }

    /// 床(高さ 0)の上に、x 0〜10・y -5〜5・高さ 6 の箱。ストックの上面は 7。
    fn block() -> Planner {
        let faces = vec![
            ("floor", top(-10.0, -10.0, 20.0, 10.0, 0.0)),
            ("block", top(0.0, -5.0, 10.0, 5.0, 6.0)),
        ];
        let ball = ToolLibrary::starter().tools[3].clone();
        prepared(
            &mesh_of(&faces),
            [-10.0, -10.0, 0.0],
            [20.0, 10.0, 7.0],
            ball,
        )
        .planner()
    }

    fn opts() -> ProjOpts {
        ProjOpts {
            leave: 0.0,
            cell: 0.05,
            floor: 0.0,
            levels: false,
            limits: Resolved::default(),
        }
    }

    fn rect(a: [f64; 2], b: [f64; 2]) -> kmb_cam_dropcutter::Region {
        kmb_cam_dropcutter::Region::new(&kmb_cam_dropcutter::areas_from_loops(vec![vec![
            a,
            [b[0], a[1]],
            b,
            [a[0], b[1]],
        ]]))
    }

    fn line(a: [f64; 2], b: [f64; 2]) -> Chain {
        Chain {
            pts: vec![a, b],
            closed: false,
            reversed: false,
        }
    }

    #[test]
    fn the_tool_center_follows_the_line_on_the_surface() {
        let p = block();
        // 箱を X に横切る線(y = 0)。工具の中心は線の上、高さは下ろした高さ
        let passes = project_passes(&p, &[line([-5.0, 0.0], [15.0, 0.0])], &opts()).unwrap();
        assert_eq!(passes.len(), 1);
        let q = &passes[0];
        assert!(q.iter().all(|v| v[1].abs() < 1e-9), "線の上を通る");
        assert_eq!((q[0][0], q[q.len() - 1][0]), (-5.0, 15.0));
        let z_at = |x: f64| {
            q.iter()
                .min_by(|a, b| (a[0] - x).abs().total_cmp(&(b[0] - x).abs()))
                .unwrap()[2]
        };
        assert!((z_at(5.0) - 6.0).abs() < 1e-6, "箱の上: {}", z_at(5.0));
        assert!(z_at(-5.0).abs() < 1e-6, "床: {}", z_at(-5.0));
        // 箱の縁から工具の半径より離れるまでは、箱に触れて上がっている
        let r = p.r();
        assert!(z_at(-r + 0.2) > 0.5, "縁の近く: {}", z_at(-r + 0.2));
    }

    #[test]
    fn a_negative_leave_cuts_below_the_surface() {
        let p = block();
        let o = ProjOpts {
            leave: -0.3,
            ..opts()
        };
        let passes = project_passes(&p, &[line([2.0, 0.0], [8.0, 0.0])], &o).unwrap();
        assert!(
            passes[0].iter().all(|v| (v[2] - 5.7).abs() < 1e-6),
            "{:?}",
            passes[0]
        );
    }

    #[test]
    fn levels_step_down_from_the_stock_top_and_open_lines_go_back_and_forth() {
        let p = block();
        let o = ProjOpts {
            levels: true,
            ..opts()
        };
        // 床の上の線(高さ 0)。ストックの上面 7 から切り込み 1 mm ずつで 7 段
        let passes = project_passes(&p, &[line([-8.0, -8.0], [-8.0, 8.0])], &o).unwrap();
        assert_eq!(passes.len(), 7);
        for (n, pass) in passes.iter().enumerate() {
            let want = 7.0 - (n + 1) as f64;
            assert!(pass.iter().all(|v| (v[2] - want).abs() < 1e-6), "{n}");
            // 行って戻る: 段の始まりは前の段の終わり
            if n > 0 {
                let (a, b) = (passes[n - 1].last().unwrap(), pass[0]);
                assert_eq!((a[0], a[1]), (b[0], b[1]));
            }
        }
    }

    #[test]
    fn a_closed_line_stays_closed_and_the_boundary_cuts_it() {
        let p = block();
        let sq = Chain {
            pts: vec![
                [2.0, -3.0],
                [8.0, -3.0],
                [8.0, 3.0],
                [2.0, 3.0],
                [2.0, -3.0],
            ],
            closed: true,
            reversed: false,
        };
        let passes = project_passes(&p, std::slice::from_ref(&sq), &opts()).unwrap();
        assert_eq!(passes.len(), 1);
        let q = &passes[0];
        assert_eq!(q[0], q[q.len() - 1], "閉じたまま");
        // 範囲で x < 5 だけにすると、閉じた線は開いた線に切れる
        let o = ProjOpts {
            limits: Resolved {
                region: Some(rect([-20.0, -20.0], [5.0, 20.0])),
                ..Default::default()
            },
            ..opts()
        };
        let cut = project_passes(&p, &[sq], &o).unwrap();
        assert!(cut.iter().flatten().all(|v| v[0] <= 5.0 + 0.1), "{cut:?}");
        assert!(cut.iter().all(|s| s[0] != s[s.len() - 1]));
    }

    #[test]
    fn no_line_over_the_part_is_an_error_not_an_empty_path() {
        let p = block();
        let o = ProjOpts {
            limits: Resolved {
                region: Some(rect([50.0, 50.0], [60.0, 60.0])),
                ..Default::default()
            },
            ..opts()
        };
        let e = project_passes(&p, &[line([2.0, 0.0], [8.0, 0.0])], &o).unwrap_err();
        assert!(e.to_string().contains("削る所がありません"), "{e}");
    }
}
