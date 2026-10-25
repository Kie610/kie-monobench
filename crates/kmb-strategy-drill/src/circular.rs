//! 円形(`cam.circular@1`)。縦の円筒の穴とボス(外側の円)を、円筒に沿う円かららせんで削る。
//!
//! - 穴: 工具の中心が穴の壁から工具の半径(+ 壁に残す厚さ)だけ内側の円を回る。周の数を決めなければ、
//!   中心に芯が残らないよう内側の円から外側へ何周かに分ける。終わりは壁から離れて中心で上げる
//! - ボス: 工具の中心がボスの壁から工具の半径だけ外側の円を回る。周の数を決めると、外側の円から
//!   内側へ送り幅ずつ寄せる(ボスのまわりの材料を何周かで取る)。周りの広い所は先にポケットなどで削る
//! - 削り方: らせん(1 周で切り込みだけ下がり、底で 1 周)か、段ごとの円(1 周のらせんで次の段へ
//!   下り、その段で 1 周)
//!
//! 選び方: 3D のビューで選んだ円筒(同じ径もまとめられる)か、選ばなければ径の範囲に入るすべての
//! 円筒。円筒の一部だけの面(角の丸みや溝の端)は、ひと回りが形にないので使わない。
//! 削る向きは主軸が時計回りの時のダウンカットが既定(穴は反時計回り、ボスは時計回り)。

use crate::cycle::{Heights, down_to};
use crate::order::{self, Order};
use crate::{Start, heights, holes};
use crate::{is_default, zero};
use kmb_cam_geom::{Bore, MachiningModel};
use kmb_cam_setup::{
    CutDir, OpBase, Planner, StrategyInput, StrategyOutput, approach, direction_field, expr_refs,
    invalid, length, opt_length, plan_error, prepare, retract,
};
use kmb_core::{EvalCtx, EvalError, Feature, Field, InputRef, Outputs, Registry};
use kmb_toolpath::{ArcDir, Move, Toolpath};
use serde::{Deserialize, Serialize};

pub const CIRCULAR: &str = "cam.circular@1";

/// 削る円筒の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CircleKind {
    /// 穴とボスの両方
    #[default]
    Both,
    /// 穴だけ
    Holes,
    /// ボス(外側の円)だけ
    Bosses,
}

/// 下り方。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CirclePath {
    /// 1 周で切り込みだけ下がるらせんで底まで下り、底で 1 周する
    #[default]
    Helix,
    /// 段ごとに、1 周のらせんで次の段へ下り、その段で 1 周する
    Circles,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Circular {
    #[serde(flatten)]
    pub base: OpBase,
    #[serde(default, skip_serializing_if = "is_default")]
    pub kind: CircleKind,
    /// 3D のビューで選んだ円筒(壁の面の名前)。空なら径の範囲に入るすべての円筒
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub faces: Vec<String>,
    /// 選んだ円筒と同じ径の円筒(同じ種類)もすべて削る
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub same_diameter: bool,
    /// この径以上の円筒だけ(空なら下限なし)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_diameter: Option<String>,
    /// この径以下の円筒だけ(空なら上限なし)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_diameter: Option<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub path: CirclePath,
    /// 壁(径方向)に残す厚さ
    #[serde(default = "zero")]
    pub leave: String,
    /// 底(軸方向)に残す厚さ。空なら 0(貫通穴は使わない)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leave_floor: Option<String>,
    /// 横の周の数。空なら、穴は中心まで、ボスは 1 周
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passes: Option<u32>,
    /// 貫通穴で、底よりさらに下げる深さ
    #[serde(default = "breakthrough")]
    pub breakthrough: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub start: Start,
    #[serde(default, skip_serializing_if = "is_default")]
    pub order: Order,
    #[serde(default, skip_serializing_if = "CutDir::is_climb")]
    pub direction: CutDir,
}

fn breakthrough() -> String {
    "0.3 mm".into()
}

impl Circular {
    /// すべての穴とボスを、らせんで削る円形(残りの項目を `..Circular::new(base)` で埋めるのに使う)。
    pub fn new(base: OpBase) -> Circular {
        Circular {
            base,
            kind: CircleKind::Both,
            faces: vec![],
            same_diameter: false,
            min_diameter: None,
            max_diameter: None,
            path: CirclePath::Helix,
            leave: zero(),
            leave_floor: None,
            passes: None,
            breakthrough: breakthrough(),
            start: Start::Hole,
            order: Order::Nearest,
            direction: CutDir::Climb,
        }
    }

    fn exprs_mut(&mut self) -> impl Iterator<Item = &mut String> {
        [&mut self.leave, &mut self.breakthrough].into_iter().chain(
            [
                &mut self.min_diameter,
                &mut self.max_diameter,
                &mut self.leave_floor,
            ]
            .into_iter()
            .flatten(),
        )
    }

    fn wants(&self, b: &Bore) -> bool {
        match self.kind {
            CircleKind::Both => true,
            CircleKind::Holes => b.hole,
            CircleKind::Bosses => !b.hole,
        }
    }
}

/// 円筒がひと回り形にあるか(角の丸みや溝の端のような、円筒の一部だけの面でないか)。
/// 壁のすぐ外とすぐ内の高さで見る: 穴なら外は壁の上端まで材料、内は空き。ボスなら逆。
pub fn full_circle(model: &MachiningModel, b: &Bore) -> bool {
    let top = b.z_max - 0.01;
    (0..24).all(|k| {
        let t = k as f64 / 24.0 * std::f64::consts::TAU;
        let at = |r: f64| [b.center[0] + r * t.cos(), b.center[1] + r * t.sin()];
        let solid = |q| model.top_at(q).is_some_and(|z| z >= top);
        let (inside, outside) = (at(b.radius - 0.05), at(b.radius + 0.05));
        if b.hole {
            solid(outside) && !solid(inside)
        } else {
            solid(inside) && !solid(outside)
        }
    })
}

/// 工具の中心が回る円の半径の列(削る順)。`wall` は壁に接する円の半径、`step` は周の間。
/// 穴は内側から外側へ(`passes` が空なら中心に芯が残らないまで)、ボスは外側から内側へ。
pub fn ring_radii(hole: bool, wall: f64, tool_r: f64, step: f64, passes: Option<u32>) -> Vec<f64> {
    match (hole, passes) {
        (true, None) => crate::cycle::helical_radii(wall, tool_r, step),
        (true, Some(n)) => (0..n.max(1))
            .rev()
            .map(|k| wall - step * k as f64)
            .filter(|r| *r > 1e-3)
            .collect(),
        (false, n) => (0..n.unwrap_or(1).max(1))
            .rev()
            .map(|k| wall + step * k as f64)
            .collect(),
    }
}

/// 1 つの円筒を削る動き。始めは最初の円の始点の上(安全高さか送り高さ)にいる。終わりは送り高さ。
fn cut_rings(
    tp: &mut Toolpath,
    p: &Planner,
    c: [f64; 2],
    rings: &[f64],
    (h, bottom): (Heights, f64),
    (path, dir, hole): (CirclePath, ArcDir, bool),
) {
    let (feed, plunge) = (p.cutting.feed, p.cutting.plunge);
    let step = p.cutting.stepdown;
    let last = |tp: &Toolpath| tp.moves.last().map_or([c[0], c[1], tp.safe_z], |m| m.to());
    let line = |tp: &mut Toolpath, to: [f64; 3], f: f64| {
        if kmb_toolpath::dist(last(tp), to) > 1e-9 {
            tp.moves.push(Move::Linear { to, feed: f });
        }
    };
    let turn = |tp: &mut Toolpath, s: [f64; 2], z: f64| {
        tp.moves.push(Move::Arc {
            to: [s[0], s[1], z],
            center: c,
            dir,
            feed,
        });
    };
    for (k, r) in rings.iter().enumerate() {
        let s = [c[0] + r, c[1]];
        if k == 0 {
            down_to(tp, s, h.r, plunge);
        } else {
            // 前の円で削った所の上を通って次の円へ
            line(tp, [s[0], s[1], h.r], feed);
        }
        line(tp, [s[0], s[1], h.top], plunge);
        match path {
            CirclePath::Helix => {
                let turns = ((h.top - bottom) / step - 1e-9).ceil().max(1.0) as usize;
                for i in 1..=turns {
                    turn(tp, s, h.top - (h.top - bottom) * i as f64 / turns as f64);
                }
                turn(tp, s, bottom);
            }
            CirclePath::Circles => {
                for z in kmb_cam_setup::split_levels(h.top, bottom, step, p.last_step) {
                    turn(tp, s, z);
                    turn(tp, s, z);
                }
            }
        }
        if hole {
            // 壁から離れて真上へ抜ける
            line(tp, [c[0], c[1], bottom], feed);
            tp.moves.push(Move::Rapid {
                to: [c[0], c[1], h.r],
            });
        } else {
            tp.moves.push(Move::Rapid {
                to: [s[0], s[1], h.r],
            });
        }
    }
}

/// ホストが解いた値(`plan` の入力の `params`)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CircularPlan {
    pub c: Circular,
    pub leave: f64,
    pub leave_floor: f64,
    pub extra: f64,
    pub lo: Option<f64>,
    pub hi: Option<f64>,
}

/// 戦略の純関数: 解いた入力だけから経路を作る(文書とカーネルに触らない)。
pub fn plan(input: &StrategyInput) -> Result<StrategyOutput, String> {
    run(input).map_err(plan_error)
}

fn run(input: &StrategyInput) -> Result<StrategyOutput, EvalError> {
    let p = Planner::new(input);
    let CircularPlan {
        c,
        leave,
        leave_floor,
        extra,
        lo,
        hi,
    } = input.params().map_err(invalid)?;
    if c.passes == Some(0) {
        return Err(invalid("周の数は 1 以上にしてください"));
    }
    if !matches!(
        p.tool.kind,
        kmb_cam_tool::ToolKind::FlatEnd | kmb_cam_tool::ToolKind::BullNose
    ) {
        return Err(invalid(format!(
            "円形には平かブルノーズのエンドミルを選んでください(工具 {})",
            p.tool.name
        )));
    }
    let in_range = |b: &Bore| {
        lo.is_none_or(|d| b.diameter() >= d - 1e-6) && hi.is_none_or(|d| b.diameter() <= d + 1e-6)
    };
    let all: Vec<&Bore> = p
        .model
        .bores
        .iter()
        .filter(|b| c.wants(b) && full_circle(&p.model, b))
        .collect();
    let mut targets: Vec<(&Bore, usize)> = Vec::new();
    fn push<'a>(t: &mut Vec<(&'a Bore, usize)>, b: &'a Bore, rank: usize) {
        if !t.iter().any(|(o, _)| std::ptr::eq(*o, b)) {
            t.push((b, rank));
        }
    }
    if c.faces.is_empty() {
        for b in all.iter().filter(|b| in_range(b)) {
            push(&mut targets, b, 0);
        }
    } else {
        for (k, f) in c.faces.iter().enumerate() {
            let b = all.iter().find(|b| b.faces.contains(f)).ok_or_else(|| {
                invalid(format!(
                    "選んだ面 {f} が、ひと回りある穴かボスに見つかりません(形が変わったか、円筒ではない面です)"
                ))
            })?;
            push(&mut targets, b, k);
        }
        if c.same_diameter {
            let picked: Vec<(f64, bool)> = targets
                .iter()
                .map(|(b, _)| (b.diameter(), b.hole))
                .collect();
            for b in &all {
                if picked
                    .iter()
                    .any(|(d, h)| *h == b.hole && (b.diameter() - d).abs() <= holes::SAME)
                {
                    push(&mut targets, b, c.faces.len());
                }
            }
        }
        targets.retain(|(b, _)| in_range(b));
    }
    if targets.is_empty() {
        return Err(invalid(
            "削る穴かボスが見つかりません(種類と径の範囲を確かめてください)",
        ));
    }
    let r = p.r();
    let step = p.tool.diameter * p.cutting.stepover;
    let mut plans = Vec::new();
    for &(b, _) in &targets {
        let h = heights(&p, b.z_max, c.start);
        let (rings, bottom) = if b.hole {
            let wall = b.radius - r - leave;
            if wall < 0.02 {
                return Err(invalid(format!(
                    "径 {:.2} mm の穴は工具 {}(径 {:.2} mm)より細いか同じなので、円形では削れません(穴あけを使ってください)",
                    b.diameter(),
                    p.tool.name,
                    p.tool.diameter
                )));
            }
            let through = (b.z_min - p.model.min[2]).abs() < 1e-3;
            let bottom = if through {
                b.z_min - extra
            } else {
                b.z_min + leave_floor
            };
            (ring_radii(true, wall, r, step, c.passes), bottom)
        } else {
            let wall = b.radius + r + leave;
            let rings = ring_radii(false, wall, r, step, c.passes);
            // 工具が回る輪の帯に、ボスの根元より高い部品があれば当たる
            let outer = rings[0] + r;
            let blocked = (0..36).any(|k| {
                let t = k as f64 / 36.0 * std::f64::consts::TAU;
                [
                    b.radius + leave + 0.05,
                    (b.radius + leave + outer) / 2.0,
                    outer - 0.05,
                ]
                .iter()
                .any(|d| {
                    let q = [b.center[0] + d * t.cos(), b.center[1] + d * t.sin()];
                    p.model.top_at(q).is_some_and(|z| z > b.z_min + 1e-3)
                })
            });
            if blocked {
                return Err(invalid(format!(
                    "径 {:.2} mm のボスのまわりに、工具が回る隙間がありません(周の数を減らすか、細い工具にしてください)",
                    b.diameter()
                )));
            }
            (rings, b.z_min + leave_floor)
        };
        let bottom = p.bottom(bottom);
        if bottom >= h.top - 1e-6 {
            return Err(invalid("削る深さがありません"));
        }
        p.check_depth(h.top, bottom)?;
        plans.push((rings, h, bottom));
    }
    let pts: Vec<[f64; 2]> = targets.iter().map(|(b, _)| b.center).collect();
    let ranks: Vec<usize> = targets.iter().map(|(_, k)| *k).collect();
    let start = [p.info.stock_min[0], p.info.stock_min[1]];
    let mut tp = p.toolpath();
    for i in order::order(&pts, &ranks, c.order, start) {
        let (b, (rings, h, bottom)) = (targets[i].0, &plans[i]);
        // ダウンカット: 穴は反時計回り、ボスは時計回り(残す壁が進む向きの右)
        let ccw = b.hole == c.direction.is_climb();
        let dir = if ccw { ArcDir::Ccw } else { ArcDir::Cw };
        approach(&mut tp, [b.center[0] + rings[0], b.center[1]]);
        cut_rings(
            &mut tp,
            &p,
            b.center,
            rings,
            (*h, *bottom),
            (c.path, dir, b.hole),
        );
    }
    retract(&mut tp);
    Ok(p.output(tp))
}

impl Feature for Circular {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = self.base.inputs()?;
        let mut c = self.clone();
        let exprs: Vec<String> = c.exprs_mut().map(|s| s.clone()).collect();
        v.extend(expr_refs(&exprs)?);
        Ok(v)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let mut p = prepare(ctx, &self.base)?;
        let leave = length(ctx, &self.leave, "壁の残し量")?;
        let leave_floor = opt_length(ctx, &self.leave_floor, "底の残し量")?.unwrap_or(0.0);
        let extra = length(ctx, &self.breakthrough, "貫通の深さ")?;
        let lo = opt_length(ctx, &self.min_diameter, "径の下限")?;
        let hi = opt_length(ctx, &self.max_diameter, "径の上限")?;
        let params = CircularPlan {
            c: self.clone(),
            leave,
            leave_floor,
            extra,
            lo,
            hi,
        };
        let input = p.input(ctx.feature_id(), params, Default::default());
        let out = plan(&input);
        p.finish_plan(self.base.setup, out)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        let mut c = self.clone();
        let mut b = c.base.clone();
        Ok(b.rename(c.exprs_mut(), old, new)?.then(|| {
            c.base = b;
            serde_json::to_value(&c).expect("JSON にできる")
        }))
    }
}

pub fn register(reg: &mut Registry) {
    reg.register_described(
        kmb_cam_setup::operation(
            CIRCULAR,
            "円形",
            "縦の円筒の穴とボス(外側の円)を、壁に沿う円やらせんで削る。3D のビューで円筒を選ぶとそれ、選ばなければ径の範囲に入るすべて",
            kmb_cam_setup::with_heights(vec![
                Field::choice(
                    "kind",
                    "削る円筒",
                    &[
                        ("both", "穴とボス"),
                        ("holes", "穴だけ"),
                        ("bosses", "ボス(外側の円)だけ"),
                    ],
                ),
                Field::info("faces", "選んだ円筒")
                    .help("3D のビューで穴かボスの壁を選んでから足す。空なら径の範囲に入るすべて"),
                Field::bool("same_diameter", "同じ径の円筒も")
                    .help("選んだ円筒と同じ径(同じ種類)の円筒を、すべて削る"),
                Field::opt_expr("min_diameter", "径の下限").length().help("この径以上だけ。空なら下限なし"),
                Field::opt_expr("max_diameter", "径の上限").length().help("この径以下だけ。空なら上限なし"),
                Field::choice(
                    "path",
                    "下り方",
                    &[("helix", "らせん"), ("circles", "段ごとの円")],
                )
                .help("らせん: 1 周で切り込みだけ下がる。段ごとの円: 段ごとに 1 周して底を平らにする"),
                Field::expr("leave", "壁の残し量").length(),
                Field::opt_expr("leave_floor", "底の残し量").length().help("空なら 0。貫通穴では使わない"),
                Field::opt_integer("passes", "横の周の数").unitless().help(
                    "壁から送り幅ずつ離れた周を、この数だけ回る。空なら、穴は中心に芯が残らないまで、ボスは 1 周",
                ),
                Field::expr("breakthrough", "貫通でさらに下げる深さ").length(),
                Field::choice(
                    "start",
                    "自動のトップ高さ",
                    &[("hole", "円筒の上"), ("stock", "ストックの上面")],
                )
                .help("円筒の上: 部品のいちばん高い所でない円筒は、その上端から削り始める"),
                Field::choice(
                    "order",
                    "並べる順",
                    &[
                        ("nearest", "近い順"),
                        ("x", "X 方向"),
                        ("y", "Y 方向"),
                        ("selected", "選択順"),
                    ],
                ),
                direction_field(),
            ]),
        )
        .icon("circular"),
        |d| Ok(Box::new(kmb_cam_setup::parse::<Circular>(d)?)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hole_is_cleared_from_the_inside_out_and_a_boss_from_the_outside_in() {
        // 穴: 壁の円 5 mm、工具の半径 1.5、送り幅 1.8 → 芯が残らない内側の円から
        let h = ring_radii(true, 5.0, 1.5, 1.8, None);
        assert!(h.windows(2).all(|w| w[0] < w[1]));
        assert!((h.last().unwrap() - 5.0).abs() < 1e-9);
        assert!(h[0] <= 1.5 * 0.9 + 1.8 + 1e-9);
        // 周の数を決めた穴は、壁から送り幅ずつ
        assert_eq!(
            ring_radii(true, 5.0, 1.5, 1.8, Some(2)),
            vec![5.0 - 1.8, 5.0]
        );
        // ボスは外側から
        assert_eq!(ring_radii(false, 8.0, 3.0, 2.0, None), vec![8.0]);
        assert_eq!(
            ring_radii(false, 8.0, 3.0, 2.0, Some(3)),
            vec![12.0, 10.0, 8.0]
        );
    }
}
