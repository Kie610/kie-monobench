//! トレース(`cam.trace@1`)。選んだモデルの辺とスケッチの線の上を、線の高さに沿って工具でなぞる。
//!
//! 彫刻(`cam.engrave@1`)は水平なスケッチの線を決めた深さでなぞるのに対し、トレースは線そのものの
//! 高さをたどる(斜めの辺や、立った面に描いたスケッチも、その高さの変わり方のまま)。深さは
//! 「軸方向のオフセット」で線を上下にずらして決める(負で深く)。
//! 段に分けると、線の高さから軸方向のオフセットまでを工具の切り込みごとに下げる。
//! V ビットで部品の縁の辺をなぞれば、縁の面取り(バリ取り)になる。
//!
//! つながった線は持ち上げずに続けて削り、離れた線は近い物から回る。開いた線は段ごとに行き来する。
//! 左右の補正は、水平な線だけ(工具の中心を刃の半径だけ横へずらす。閉じた線は外側か内側、
//! 開いた線は描いた向きの左か右)。

use crate::is_default;
use crate::{CHORD, Chain, TraceSide, shifted};
use jc_cam_setup::{
    OpBase, Planner, Prepared, StrategyInput, StrategyOutput, approach, invalid, length, output,
    plan_error, prepare, retract,
};
use jc_cam_tool::ToolKind;
use jc_core::{
    Curves, EvalCtx, EvalError, Feature, FeatureId, Field, InputRef, Outputs, Ref, RefKind,
    Registry,
};
use jc_toolpath::Move;
use serde::{Deserialize, Serialize};

pub const TRACE: &str = "cam.trace@1";

type P3 = [f64; 3];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Trace {
    #[serde(flatten)]
    pub base: OpBase,
    /// なぞるモデルの辺(3D のビューで選ぶ)
    #[serde(default, with = "jc_core::one_or_many")]
    pub edges: Vec<Ref>,
    /// なぞる線を出す機能(スケッチなど)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curves: Option<FeatureId>,
    /// 線を上下にずらす量(負で線より下を削る)
    #[serde(default = "offset")]
    pub axial_offset: String,
    /// 左右の補正(既定は線の上。水平な線だけ)
    #[serde(default, skip_serializing_if = "is_default")]
    pub side: TraceSide,
    /// 線の高さから軸方向のオフセットまでを、工具の切り込みごとの段に分ける
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub multiple_depths: bool,
}

fn offset() -> String {
    "-0.3 mm".into()
}

impl Trace {
    /// 線の 0.3 mm 下を 1 回でなぞるトレース(残りの項目を `..Trace::new(base)` で埋めるのに使う)。
    pub fn new(base: OpBase) -> Trace {
        Trace {
            base,
            edges: vec![],
            curves: None,
            axial_offset: offset(),
            side: TraceSide::Center,
            multiple_depths: false,
        }
    }
}

/// 3D の線を、つながる物を 1 本にまとめた列にする(今いる所に近い端から順に)。
pub fn chains3(mut lines: Vec<Vec<P3>>) -> Vec<(Vec<P3>, bool)> {
    let d = |a: P3, b: P3| {
        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
    };
    let near = |a: P3, b: P3| d(a, b) < 1e-4;
    let mut out = Vec::new();
    let mut at: P3 = [0.0, 0.0, f64::INFINITY];
    while !lines.is_empty() {
        let dist = |p: P3| {
            if at[2].is_finite() {
                d(p, at)
            } else {
                p[0].hypot(p[1])
            }
        };
        let (i, rev) = lines
            .iter()
            .enumerate()
            .flat_map(|(i, l)| [(i, false, l[0]), (i, true, l[l.len() - 1])])
            .min_by(|a, b| dist(a.2).total_cmp(&dist(b.2)))
            .map(|(i, r, _)| (i, r))
            .expect("空でない");
        let mut chain = lines.swap_remove(i);
        if rev {
            chain.reverse();
        }
        loop {
            let end = chain[chain.len() - 1];
            let Some((j, rev)) = lines.iter().enumerate().find_map(|(j, l)| {
                if near(l[0], end) {
                    Some((j, false))
                } else if near(l[l.len() - 1], end) {
                    Some((j, true))
                } else {
                    None
                }
            }) else {
                break;
            };
            let mut next = lines.swap_remove(j);
            if rev {
                next.reverse();
            }
            chain.extend(next.into_iter().skip(1));
        }
        let closed = chain.len() > 2 && near(chain[0], chain[chain.len() - 1]);
        at = chain[chain.len() - 1];
        out.push((chain, closed));
    }
    out
}

/// スケッチなどの線(セットアップの座標。補助線は使わない)。
fn curve_lines(ctx: &EvalCtx<'_>, p: &Prepared, id: FeatureId) -> Result<Vec<Vec<P3>>, EvalError> {
    let curves = Curves::from_value(ctx.output(id, "curves")?)
        .ok_or_else(|| invalid(format!("{id} は線を出しません")))?;
    let mut out = Vec::new();
    for s in &curves.segments {
        let mut pts = s.polyline(s.steps_per_turn(CHORD));
        if matches!(s, jc_core::Segment::Circle { .. }) {
            pts.push(pts[0]);
        } else {
            pts.push(s.end_point());
        }
        let line: Vec<P3> = pts
            .into_iter()
            .map(|q| p.info.to_setup(curves.plane.to_world(q)))
            .collect();
        if line.len() >= 2 {
            out.push(line);
        }
    }
    Ok(out)
}

/// ホストが解いた値(`plan` の入力の `params`)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TracePlan {
    pub off: f64,
    pub lines: Vec<Vec<P3>>,
    pub side: TraceSide,
    pub multiple_depths: bool,
}

/// 戦略の純関数: 解いた入力だけから経路を作る(文書とカーネルに触らない)。
pub fn plan(input: &StrategyInput) -> Result<StrategyOutput, String> {
    run(input).map_err(plan_error)
}

fn run(input: &StrategyInput) -> Result<StrategyOutput, EvalError> {
    let p = Planner::new(input);
    let TracePlan {
        off,
        lines,
        side,
        multiple_depths,
    } = input.params().map_err(invalid)?;
    // 刃より深く下げない(線そのものの高さは形の上なので、下げる量だけを調べる)
    p.check_depth(0.0, off.min(0.0))?;
    // 段: 線の高さ(0)から軸方向のオフセットまで
    let offsets = if multiple_depths && off < 0.0 {
        p.levels(0.0, off)
    } else {
        vec![off]
    };
    let mut tp = p.toolpath();
    // 部品の縁を意図して削るので、線から下げた深さまでの食い込みを許す
    tp.edge_break = (-off).max(0.0) + 0.05;
    let d = p.tool.radius_at((-off).max(0.0)).unwrap_or(p.r());
    let mut all: Vec<(Vec<P3>, bool)> = Vec::new();
    for (pts, closed) in chains3(lines) {
        if side == TraceSide::Center {
            all.push((pts, closed));
            continue;
        }
        let z = pts[0][2];
        if pts.iter().any(|q| (q[2] - z).abs() > 1e-4) {
            return Err(invalid(
                "左右の補正は水平な線だけに使えます(高さの変わる線は「線の上」にしてください)",
            ));
        }
        let c = Chain {
            pts: pts.iter().map(|q| [q[0], q[1]]).collect(),
            closed,
            reversed: false,
        };
        for s in shifted(&c, side, d) {
            all.push((s.pts.iter().map(|q| [q[0], q[1], z]).collect(), s.closed));
        }
    }
    if all.is_empty() {
        return Err(invalid(
            "左右にずらすと線がなくなりました(小さな閉じた線を内側へずらす時は、細い工具にしてください)",
        ));
    }
    for (chain, closed) in all {
        approach(&mut tp, [chain[0][0], chain[0][1]]);
        let mut forward = true;
        for o in &offsets {
            let pts: Vec<P3> = if forward {
                chain.clone()
            } else {
                chain.iter().rev().copied().collect()
            };
            tp.moves.push(Move::Linear {
                to: [pts[0][0], pts[0][1], pts[0][2] + o],
                feed: p.cutting.plunge,
            });
            tp.moves.extend(pts.iter().skip(1).map(|q| Move::Linear {
                to: [q[0], q[1], q[2] + o],
                feed: p.cutting.feed,
            }));
            if !closed {
                forward = !forward;
            }
        }
        retract(&mut tp);
    }
    Ok(p.output(tp))
}

impl Feature for Trace {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = self.base.inputs()?;
        if let Some(c) = self.curves {
            v.push(output(c, "curves"));
        }
        v.extend(jc_cam_setup::expr_refs([&self.axial_offset])?);
        Ok(v)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let mut p = prepare(ctx, &self.base)?;
        if matches!(
            p.tool.kind,
            ToolKind::Drill | ToolKind::Laser | ToolKind::ThreadMill
        ) {
            return Err(invalid(format!(
                "トレースには、エンドミルか V ビットを選んでください(工具 {} は{})",
                p.tool.name,
                p.tool.kind.label()
            )));
        }
        let off = length(ctx, &self.axial_offset, "軸方向のオフセット")?;
        let mut lines = jc_cam_setup::edge_lines(ctx, &p, &self.edges)?;
        if let Some(c) = self.curves {
            lines.extend(curve_lines(ctx, &p, c)?);
        }
        if lines.is_empty() {
            return Err(invalid(
                "なぞる辺か線がありません(3D のビューで辺を選ぶか、なぞる線にスケッチを選んでください)",
            ));
        }
        let params = TracePlan {
            off,
            lines,
            side: self.side,
            multiple_depths: self.multiple_depths,
        };
        let input = p.input(ctx.feature_id(), params, Default::default());
        let out = plan(&input);
        p.finish_plan(self.base.setup, out)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        let mut c = self.clone();
        Ok(c.base
            .rename([&mut c.axial_offset], old, new)?
            .then(|| serde_json::to_value(&c).expect("JSON にできる")))
    }
}

pub fn register(reg: &mut Registry) {
    reg.register_described(
        jc_cam_setup::operation(
            TRACE,
            "トレース",
            "選んだモデルの辺とスケッチの線の上を、線の高さに沿ってなぞる。V ビットで縁の面取り、エンドミルで線に沿う溝に",
            vec![
                Field::pick("edges", "なぞる辺", &[RefKind::Edge], 0, None)
                    .help("3D のビューで辺を選ぶ(選ぶ種類を「辺」に)。いくつでも。つながった辺は続けて削る"),
                Field::opt_output("curves", "なぞる線", "curves")
                    .help("スケッチか文字の線(補助線を除く)。辺と一緒にも使える"),
                Field::expr("axial_offset", "軸方向のオフセット")
                    .length()
                    .help("線を上下にずらす量。負で線より下を削る(上面に描いたスケッチなら、削る深さを負で入れる)"),
                Field::bool("multiple_depths", "段に分ける")
                    .help("線の高さから軸方向のオフセットまでを、工具の切り込みの深さごとに下げる"),
                Field::choice(
                    "side",
                    "左右の補正",
                    &[
                        ("center", "線の上"),
                        ("left", "外側・左"),
                        ("right", "内側・右"),
                    ],
                )
                .help(
                    "工具の中心を線から刃の半径(下げた深さでの)だけずらす。水平な線だけ。\
                     閉じた線は外側か内側、開いた線は描いた向きの左か右",
                ),
            ],
        )
        .icon("trace"),
        |d| Ok(Box::new(jc_cam_setup::parse::<Trace>(d)?)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touching_3d_lines_are_joined_and_a_ring_is_closed() {
        let c = chains3(vec![
            vec![[0.0, 0.0, 0.0], [10.0, 0.0, -2.0]],
            vec![[10.0, 0.0, -2.0], [10.0, 10.0, -2.0]],
            vec![[30.0, 0.0, 0.0], [40.0, 0.0, 0.0]],
            vec![[10.0, 10.0, -2.0], [0.0, 0.0, 0.0]],
        ]);
        assert_eq!(c.len(), 2);
        assert!(c[0].1, "三角は閉じている");
        assert_eq!(c[0].0.len(), 4);
        assert!(!c[1].1);
        // 同じ平面の位置でも高さが違えばつながない
        let apart = chains3(vec![
            vec![[0.0, 0.0, 0.0], [5.0, 0.0, 0.0]],
            vec![[5.0, 0.0, -1.0], [9.0, 0.0, -1.0]],
        ]);
        assert_eq!(apart.len(), 2);
    }

    #[test]
    fn an_arc_edge_is_split_finely_enough() {
        let e = jc_core::EdgeInfo {
            faces: ["a".into(), "b".into()],
            a: [5.0, 0.0, 0.0],
            b: [5.0, 0.0, 0.0],
            convex: true,
            seam: jc_core::Seam::Corner,
            arc: Some(jc_core::ArcEdge {
                center: [0.0, 0.0, 0.0],
                axis: [0.0, 0.0, 1.0],
                radius: 5.0,
                sweep: std::f64::consts::TAU,
                inside: false,
            }),
            curve: vec![],
        };
        let pts = jc_cam_setup::edge_points(&e);
        assert!(pts.len() > 20);
        // 弦の真ん中と円の差が誤差より小さい
        for w in pts.windows(2) {
            let m = [(w[0][0] + w[1][0]) / 2.0, (w[0][1] + w[1][1]) / 2.0];
            assert!(5.0 - m[0].hypot(m[1]) <= CHORD + 1e-9);
        }
    }
}
