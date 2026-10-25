//! スロット: スケッチの線(補助線を除く)を溝の中心にして、決めた幅と深さの溝を削る。
//!
//! 幅が工具の径より広ければ、中心の線を削ってから、左右へ送り幅ずつずらした線を内から外へ
//! 削って広げる。段ごとの終わりと最後は溝の中心の線の端へ戻ってから上げるので、上げる時に
//! 壁を擦らない。溝は形にないので、シミュレーションは深さまでの食い込みを許す。

use crate::{Chain, P2, TraceSide, chains, read_lines, shifted};
use kmb_cam_setup::{
    OpBase, Planner, StrategyInput, StrategyOutput, approach, expr_refs, invalid, length, output,
    plan_error, prepare, retract,
};
use kmb_core::{EvalCtx, EvalError, Feature, FeatureId, Field, InputRef, Outputs, Registry};
use kmb_toolpath::Move;
use serde::{Deserialize, Serialize};

pub const SLOT: &str = "cam.slot@1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Slot {
    #[serde(flatten)]
    pub base: OpBase,
    /// 溝の中心の線を出す機能(出力 `curves`)
    pub curves: FeatureId,
    /// 溝の幅。空なら工具の径(中心の線を 1 回なぞる)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<String>,
    /// 溝の深さ(線の平面から)
    #[serde(default = "depth")]
    pub depth: String,
}

fn depth() -> String {
    "2 mm".into()
}

impl Slot {
    /// 工具の径の幅で深さ 2 mm の溝(残りの項目を `..Slot::new(base, curves)` で埋めるのに使う)。
    pub fn new(base: OpBase, curves: FeatureId) -> Slot {
        Slot {
            base,
            curves,
            width: None,
            depth: depth(),
        }
    }
}

/// 中心の線からずらす量の並び(中心、左右へ内から外へ)。`half` は工具の中心が動ける片側の幅、
/// `step` は 1 回に広げる幅。最後は必ず `half` ちょうどにする。
fn offsets(half: f64, step: f64) -> Vec<f64> {
    let mut v = vec![0.0];
    if half <= 1e-6 {
        return v;
    }
    let n = (half / step.max(1e-3)).ceil().max(1.0) as usize;
    for k in 1..=n {
        let d = half * k as f64 / n as f64;
        v.push(d);
        v.push(-d);
    }
    v
}

fn near_end(pts: &[P2], at: P2) -> bool {
    let d = |q: P2| (q[0] - at[0]).hypot(q[1] - at[1]);
    d(pts[pts.len() - 1]) < d(pts[0])
}

/// 1 本の線の溝を、高さ `levels` ごとに削る動き。始めは `center` の始点の上(退避高さ)にいる。
/// `passes` は中心の線と、ずらした線(中心の線と同じ向き)。終わりは中心の線の端(溝の底)。
fn cut_slot(
    moves: &mut Vec<Move>,
    center: &Chain,
    passes: &[Chain],
    levels: &[f64],
    (feed, plunge): (f64, f64),
) {
    let mut at: P2 = center.pts[0];
    for &z in levels {
        // 中心の線の近い端で下げる(前の段の終わりは中心の線の端)
        let line: Vec<P2> = if center.closed || !near_end(&center.pts, at) {
            center.pts.clone()
        } else {
            center.pts.iter().rev().copied().collect()
        };
        moves.push(Move::Linear {
            to: [line[0][0], line[0][1], z],
            feed: plunge,
        });
        let cut = |pts: &[P2], moves: &mut Vec<Move>| {
            moves.extend(pts.iter().map(|q| Move::Linear {
                to: [q[0], q[1], z],
                feed,
            }));
        };
        cut(&line[1..], moves);
        at = line[line.len() - 1];
        for pass in passes {
            // 今いる所に近い端から始める(閉じた線は始点から)
            let pts: Vec<P2> = if !pass.closed && near_end(&pass.pts, at) {
                pass.pts.iter().rev().copied().collect()
            } else {
                pass.pts.clone()
            };
            cut(&pts, moves);
            at = pts[pts.len() - 1];
        }
        // 溝の中心の線の近い端へ戻る(壁から離れる)
        let end = if center.closed || !near_end(&center.pts, at) {
            center.pts[0]
        } else {
            center.pts[center.pts.len() - 1]
        };
        if (end[0] - at[0]).hypot(end[1] - at[1]) > 1e-9 {
            moves.push(Move::Linear {
                to: [end[0], end[1], z],
                feed,
            });
        }
        at = end;
    }
}

/// ホストが解いた値(`plan` の入力の `params`)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlotPlan {
    pub depth: f64,
    pub width: f64,
    pub lines: Vec<Vec<P2>>,
    pub top: f64,
}

/// 戦略の純関数: 解いた入力だけから経路を作る(文書とカーネルに触らない)。
pub fn plan(input: &StrategyInput) -> Result<StrategyOutput, String> {
    run(input).map_err(plan_error)
}

fn run(input: &StrategyInput) -> Result<StrategyOutput, EvalError> {
    let p = Planner::new(input);
    let SlotPlan {
        depth,
        width,
        lines,
        top,
    } = input.params().map_err(invalid)?;
    let bottom = top - depth;
    p.check_depth(top, bottom)?;
    let mut tp = p.toolpath();
    tp.edge_break = depth + 0.05;
    let half = (width - p.tool.diameter) / 2.0;
    let levels = p.levels(top, bottom);
    let step = p.tool.diameter * p.cutting.stepover;
    for center in chains(lines) {
        let passes: Vec<Chain> = offsets(half, step)
            .into_iter()
            .skip(1)
            .flat_map(|d| {
                let side = if d > 0.0 {
                    TraceSide::Left
                } else {
                    TraceSide::Right
                };
                shifted(&center, side, d.abs())
            })
            .collect();
        approach(&mut tp, center.pts[0]);
        cut_slot(
            &mut tp.moves,
            &center,
            &passes,
            &levels,
            (p.cutting.feed, p.cutting.plunge),
        );
        retract(&mut tp);
    }
    Ok(p.output(tp))
}

impl Feature for Slot {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = self.base.inputs()?;
        v.push(output(self.curves, "curves"));
        v.extend(expr_refs([&self.depth])?);
        v.extend(expr_refs(self.width.iter())?);
        Ok(v)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let mut p = prepare(ctx, &self.base)?;
        let depth = length(ctx, &self.depth, "溝の深さ")?;
        if depth <= 0.0 {
            return Err(invalid("溝の深さは 0 より大きくしてください"));
        }
        let dia = p.tool.diameter;
        let width = match &self.width {
            Some(src) => length(ctx, src, "溝の幅")?,
            None => dia,
        };
        if width < dia - 1e-6 {
            return Err(invalid(format!(
                "溝の幅 {width:.2} mm は工具の径 {dia:.2} mm より狭くできません(細い工具を選んでください)"
            )));
        }
        let (lines, top) = read_lines(ctx, &p, self.curves)?;
        let params = SlotPlan {
            depth,
            width,
            lines,
            top,
        };
        let input = p.input(ctx.feature_id(), params, Default::default());
        let out = plan(&input);
        p.finish_plan(self.base.setup, out)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        let mut c = self.clone();
        let mut e = vec![&mut c.depth];
        e.extend(c.width.as_mut());
        Ok(c.base
            .rename(e, old, new)?
            .then(|| serde_json::to_value(&c).expect("JSON にできる")))
    }
}

pub fn register(reg: &mut Registry) {
    reg.register_described(
        kmb_cam_setup::operation(
            SLOT,
            "スロット",
            "スケッチの線を溝の中心にして、決めた幅と深さの溝を削る。幅が工具より広ければ左右へ広げ、終わりは溝の中心へ戻ってから上げる",
            vec![
                Field::output("curves", "溝の中心の線", "curves"),
                Field::opt_expr("width", "溝の幅").length().help("空なら工具の径(中心の線を 1 回なぞる)"),
                Field::expr("depth", "溝の深さ").length().help("線の平面から"),
            ],
        )
        .icon("engrave"),
        |d| Ok(Box::new(kmb_cam_setup::parse::<Slot>(d)?)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wide_slot_is_widened_from_the_center_out_to_exactly_half() {
        assert_eq!(offsets(0.0, 1.0), vec![0.0]);
        let v = offsets(2.5, 1.0);
        // 中心、±0.833、±1.667、±2.5
        assert_eq!(v.len(), 7);
        assert_eq!(v[0], 0.0);
        assert!((v[5] - 2.5).abs() < 1e-12 && (v[6] + 2.5).abs() < 1e-12);
        assert!(v.windows(2).skip(1).step_by(2).all(|w| w[0] == -w[1]));
    }

    #[test]
    fn each_level_ends_back_on_the_center_line() {
        let center = chains(vec![vec![[0.0, 0.0], [20.0, 0.0]]]).remove(0);
        let passes: Vec<Chain> = [1.0, -1.0]
            .into_iter()
            .flat_map(|d: f64| {
                let side = if d > 0.0 {
                    TraceSide::Left
                } else {
                    TraceSide::Right
                };
                shifted(&center, side, d.abs())
            })
            .collect();
        let mut m = Vec::new();
        cut_slot(&mut m, &center, &passes, &[-1.0, -2.0], (600.0, 200.0));
        // 段ごとの最後は中心の線(y = 0)の端
        for z in [-1.0, -2.0] {
            let last = m.iter().rev().find(|q| q.to()[2] == z).unwrap().to();
            assert!(last[1].abs() < 1e-9, "{last:?}");
            assert!(last[0].abs() < 1e-9 || (last[0] - 20.0).abs() < 1e-9);
        }
        // 2 段目は、1 段目が終わった中心の端で真下へ下げる
        let first_down = m
            .iter()
            .position(|q| {
                matches!(q, Move::Linear { feed, .. } if *feed == 200.0) && q.to()[2] == -2.0
            })
            .unwrap();
        let before = m[first_down - 1].to();
        let after = m[first_down].to();
        assert!(before[0] == after[0] && before[1] == after[1]);
        // 溝の幅いっぱい(y = ±1)を通る
        assert!(m.iter().any(|q| (q.to()[1] - 1.0).abs() < 1e-9));
        assert!(m.iter().any(|q| (q.to()[1] + 1.0).abs() < 1e-9));
    }
}
