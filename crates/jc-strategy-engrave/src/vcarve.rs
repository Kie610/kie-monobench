//! V 彫り(`cam.vcarve@1`)。閉じた領域(スケッチの輪郭)を V ビットで彫る。
//!
//! 縁から距離 `d` の所を深さ `d / tan(刃先角 / 2)` でなぞると、V ビットの円すいがちょうど縁に届く。
//! そこで、領域を内側へ等間隔に寄せた輪を、外側から順に、その距離に合う深さでなぞる。
//! 尖った角は寄せた輪の角が二等分線をたどるので、角まで細く鋭く彫れる。
//! 深さが「最大の深さ」に届いた所(太い所)は、その深さのまま同じ間隔で削る
//! (底には、間隔の半分を tan(刃先角 / 2) で割った高さの筋が残る。0.1 mm、90° なら 0.05 mm)。
//! 彫った溝は形にないので、シミュレーションは深さまでの食い込みを許す。

use jc_cam_geom::{Area, P2, fit_arcs, offset_area};
use jc_cam_setup::{
    OpBase, Planner, StrategyInput, StrategyOutput, approach, expr_refs, invalid, length,
    loop_moves, output, plan_error, prepare, retract, start_near,
};
use jc_cam_tool::ToolKind;
use jc_core::{
    EvalCtx, EvalError, Feature, FeatureId, Field, InputRef, Outputs, Profile, Registry,
};
use jc_toolpath::Move;
use serde::{Deserialize, Serialize};

pub const VCARVE: &str = "cam.vcarve@1";

/// 輪郭の折れ線を円弧にまとめる時の許容差(mm)
const ARC_TOL: f64 = 0.001;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VCarve {
    #[serde(flatten)]
    pub base: OpBase,
    /// 彫る領域を出す機能(閉じた輪郭のあるスケッチ)
    pub profile: FeatureId,
    /// これより深くは彫らない(太い所は、この深さで平らに削る)
    #[serde(default = "max_depth")]
    pub max_depth: String,
    /// 寄せた輪の間隔。細かいほど滑らかで、時間がかかる
    #[serde(default = "spacing")]
    pub spacing: String,
}

fn max_depth() -> String {
    "2 mm".into()
}

fn spacing() -> String {
    "0.1 mm".into()
}

/// ホストが解いた値(`plan` の入力の `params`)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VCarvePlan {
    pub half: f64,
    pub max: f64,
    pub step: f64,
    pub areas: Vec<Area>,
    pub top: f64,
}

/// 戦略の純関数: 解いた入力だけから経路を作る(文書とカーネルに触らない)。
pub fn plan(input: &StrategyInput) -> Result<StrategyOutput, String> {
    run(input).map_err(plan_error)
}

fn run(input: &StrategyInput) -> Result<StrategyOutput, EvalError> {
    let p = Planner::new(input);
    let VCarvePlan {
        half,
        max,
        step,
        areas,
        top,
    } = input.params().map_err(invalid)?;
    let mut tp = p.toolpath();
    let mut deepest: f64 = 0.0;
    for area in &areas {
        let mut d = step;
        // 工具が下りている所と、その深さで工具が縁を越えずに動ける領域(前の輪の領域)
        let mut here: Option<P2> = None;
        let mut prev: Vec<Area> = Vec::new();
        loop {
            let rings = offset_area(area, d);
            if rings.is_empty() {
                break;
            }
            let depth = (d / half).min(max);
            deepest = deepest.max(depth);
            let z = top - depth;
            let mut moved_here = false;
            for a in &rings {
                for l in std::iter::once(&a.outer).chain(&a.holes) {
                    let l = here.map_or_else(|| l.clone(), |h| start_near(l, h));
                    let start = [l.verts[0].x, l.verts[0].y];
                    // 次の輪の始まりが近く、間が今の深さで削ってよい領域の中なら、上げずに横へ移る
                    // (輪ごとに安全高さへ上げて真下に下りると、無駄な上げ下げが増える)
                    let region = if moved_here { &rings } else { &prev };
                    let cur_z = tp.moves.last().map(|m| m.to()[2]);
                    match (here, cur_z) {
                        (Some(h), Some(cz)) if stays_inside(region, h, start, step) => {
                            tp.moves.push(Move::Linear {
                                to: [start[0], start[1], cz],
                                feed: p.cutting.feed,
                            });
                        }
                        _ => approach(&mut tp, start),
                    }
                    tp.moves.push(Move::Linear {
                        to: [start[0], start[1], z],
                        feed: p.cutting.plunge,
                    });
                    tp.moves.extend(loop_moves(&l, z, p.cutting.feed));
                    here = Some(start);
                    moved_here = true;
                }
            }
            prev = rings;
            // 平らな所も同じ間隔(V ビットの先は尖っているので、間隔の半分 / tan の筋が残る)
            d += step;
        }
        retract(&mut tp);
    }
    p.check_depth(top, top - deepest)?;
    tp.edge_break = deepest + 0.05;
    Ok(p.output(tp))
}

impl Feature for VCarve {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = self.base.inputs()?;
        v.push(output(self.profile, "profile"));
        v.extend(expr_refs([&self.max_depth, &self.spacing])?);
        Ok(v)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let mut p = prepare(ctx, &self.base)?;
        if p.tool.kind != ToolKind::VBit {
            return Err(invalid(format!(
                "V 彫りには V ビットを選んでください(工具 {} は V ビットではありません)",
                p.tool.name
            )));
        }
        let half = (p.tool.tip_angle.to_radians() / 2.0).tan();
        if half.is_nan() || half <= 1e-6 {
            return Err(invalid("V ビットの刃先角を決めてください"));
        }
        let max = length(ctx, &self.max_depth, "最大の深さ")?;
        let step = length(ctx, &self.spacing, "線の間隔")?;
        if max <= 0.0 || step <= 1e-3 {
            return Err(invalid(
                "最大の深さは 0 より大きく、線の間隔は 0.001 mm より大きくしてください",
            ));
        }
        let profile = Profile::from_value(ctx.output(self.profile, "profile")?)
            .ok_or_else(|| invalid(format!("{} は閉じた輪郭を出しません", self.profile)))?;
        if profile.regions.is_empty() {
            return Err(invalid("彫る領域がありません(閉じた輪郭を描いてください)"));
        }
        let info = &p.info;
        let place = |q: [f64; 3]| {
            let w = info.place(q);
            [
                w[0] - info.origin[0],
                w[1] - info.origin[1],
                w[2] - info.origin[2],
            ]
        };
        let mut top: Option<f64> = None;
        let mut areas = Vec::new();
        for r in &profile.regions {
            let mut loops = Vec::new();
            for l in std::iter::once(&r.outer).chain(&r.holes) {
                let mut pts: Vec<P2> = Vec::new();
                for q in l.polyline() {
                    let w = place(profile.plane.to_world(q));
                    match top {
                        None => top = Some(w[2]),
                        Some(z0) if (w[2] - z0).abs() > 1e-4 => {
                            return Err(invalid(
                                "輪郭の平面が、機械に置いた向きで水平ではありません(上か下を向いた面に描いてください)",
                            ));
                        }
                        _ => {}
                    }
                    pts.push([w[0], w[1]]);
                }
                // 円や円弧は真の円弧に戻してから寄せる。細かい直線の円を消える寸前まで寄せると、
                // cavalier_contours の検査が panic する。円弧なら寄せた輪も円弧で、滑らかに出る
                loops.push(fit_arcs(&pts, ARC_TOL));
            }
            let outer = loops.remove(0);
            areas.push(
                Area {
                    outer,
                    holes: loops,
                }
                .normalized(),
            );
        }
        let top = top.expect("輪郭がある");
        let params = VCarvePlan {
            half,
            max,
            step,
            areas,
            top,
        };
        let input = p.input(ctx.feature_id(), params, Default::default());
        let out = plan(&input);
        p.finish_plan(self.base.setup, out)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        let mut c = self.clone();
        Ok(c.base
            .rename([&mut c.max_depth, &mut c.spacing], old, new)?
            .then(|| serde_json::to_value(&c).expect("JSON にできる")))
    }
}

/// `a` から `b` への直線が、近く(輪の間隔の 3 倍以内)で、途中が `region` の中にあるか。
/// 縁から距離 `d` の領域の中なら、深さ `d / tan(刃先角 / 2)` の V ビットは縁を越えない。
fn stays_inside(region: &[Area], a: P2, b: P2, step: f64) -> bool {
    let len = (b[0] - a[0]).hypot(b[1] - a[1]);
    if len > 3.0 * step {
        return false;
    }
    // 端は輪の上(領域の縁)なので、間の点で確かめる
    let n = ((len / (step / 4.0)).ceil() as usize).max(2);
    (1..n).all(|i| {
        let t = i as f64 / n as f64;
        let q = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        region.iter().any(|r| r.contains(q))
    })
}

pub fn register(reg: &mut Registry) {
    reg.register_described(
        jc_cam_setup::operation(
            VCARVE,
            "V 彫り",
            "スケッチの閉じた輪郭の内側を V ビットで彫る。細い所は浅く尖らせ、太い所は最大の深さで平らに削る。看板や文字に",
            vec![
                Field::output("profile", "彫る領域", "profile"),
                Field::expr("max_depth", "最大の深さ").length().help("太い所はこの深さで平らに削る"),
                Field::expr("spacing", "線の間隔").length()
                    .help("縁から内側へ寄せる輪の間隔。細かいほど滑らか(例: 0.1 mm)"),
            ],
        )
        .icon("vcarve"),
        |d| Ok(Box::new(jc_cam_setup::parse::<VCarve>(d)?)),
    );
}
