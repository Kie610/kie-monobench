//! 穴あけ(`cam.drill@1`)。部品の縦の穴を探し、工具に合わせて削る。
//!
//! - ドリル: 径が穴と同じ(差 0.05 mm 以内)なら、まっすぐ下げる。サイクルは 1 回で下げる・つつき・
//!   切りくず切断から選べ、既定(工具で決める)はつつき。固定サイクルは使わず、G0 と G1 に展開する
//!   (Carvera の対応表に固定サイクルの記載がないため)
//! - エンドミル: 穴より細ければ、らせん(ヘリカル)で削る。中心に芯が残る太さの穴は、
//!   半径を内側から外側へ広げながら何周かに分ける
//!
//! - V ビット: もみつけ(スポット)。穴の縁より 0.2 mm 外まで円すいに下げ、ドリルの位置決めと縁の面取りにする
//!   (穴が刃の径より太ければ、刃の径いっぱいまで)。皿の穴では、皿の径まで面取りする
//!
//! 削る穴の選び方:
//! - 何も選ばなければ、上から削れる丸い穴をすべて自動で拾う(`diameter` でその径の穴だけに絞れる)
//! - 3D のビューで選んだ穴(`faces`)。「同じ径の穴も」で、選んだ穴と同じ径の穴をすべて足す
//! - 穴の機能(`holes`)の穴。穴の径や種類を直すと、経路も作り直される(タイムラインの依存)
//! - スケッチの点(`points`)。単独の点と円の中心に、穴の無い所でもあける(下穴・もみつけ)
//!
//! 穴とボスを壁に沿う円やらせんで削る円形(`cam.circular@1`)は [`Circular`]、ねじ切り(`cam.thread@1`)は
//! [`Thread`]、裏返しの位置合わせのピン穴(`cam.pin_holes@1`)は [`PinHoles`]。穴の機能から穴あけをまとめて
//! 作る操作は [`HoleOps`]。
//!
//! ポケットの底の穴は、ストックの上面からではなく穴の上から削り始める(部品の上面の穴は、
//! 上面がまだ削られていないかもしれないので、ストックの上面から)。貫通穴は `breakthrough` だけ深く削る。

mod circular;
mod cycle;
mod hole_ops;
pub mod holes;
pub mod order;
mod thread;

pub use circular::{CIRCULAR, CircleKind, CirclePath, Circular};
pub use cycle::{CHIP_LIFT, Cycle};
pub use hole_ops::{AUTO_HOLES, HOLE_OPS, HOLE_TOOLS, HoleOps, PICK_HOLE, SAME_DIAMETER};
pub use order::Order;
pub use thread::{Hand, THREAD, Thread};

use cycle::{Heights, Plunge};
use kmb_cam_geom::Bore;
use kmb_cam_setup::{
    OpBase, Planner, Prepared, StrategyInput, StrategyOutput, approach, expr_refs, invalid, length,
    plan_error, prepare, retract,
};
use kmb_cam_tool::{ToolKind, ToolLibrary};
use kmb_core::Field;
use kmb_core::kernel::{Curves, Segment};
use kmb_core::{
    EvalCtx, EvalError, Feature, FeatureId, InputRef, Outputs, Payload, Points, Registry,
};
use kmb_interchange::HolePart;
use kmb_toolpath::{Move, Toolpath};
use serde::{Deserialize, Serialize};

pub const DRILL: &str = "cam.drill@1";
pub const PIN_HOLES: &str = "cam.pin_holes@1";

pub(crate) fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

pub(crate) fn zero() -> String {
    "0 mm".into()
}

/// 削り始める高さ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Start {
    /// 穴の上(ポケットの底の穴なら底から)。部品の上面の穴はストックの上面から
    #[default]
    Hole,
    /// いつもストックの上面から
    Stock,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Drill {
    #[serde(flatten)]
    pub base: OpBase,
    /// この径の穴だけを削る(差 0.05 mm 以内)。空ならすべての穴
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diameter: Option<String>,
    /// 貫通穴で、底よりさらに下げる深さ
    #[serde(default = "breakthrough")]
    pub breakthrough: String,
    /// 3D のビューで選んだ穴(穴の壁の面の名前)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub faces: Vec<String>,
    /// 選んだ穴と同じ径の穴もすべて削る
    #[serde(default, skip_serializing_if = "is_default")]
    pub same_diameter: bool,
    /// 穴の機能(`solid.hole@1`)。その穴をすべて削る
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holes: Vec<FeatureId>,
    /// 穴の機能のどの部分を削るか
    #[serde(default, skip_serializing_if = "is_default")]
    pub part: HolePart,
    /// 点のスケッチ。単独の点と円の中心にあける
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub points: Option<FeatureId>,
    /// 点にあける深さ(点の所の部品の上から。V ビットは先端の深さ)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub cycle: Cycle,
    /// 1 回に下げる深さ。空なら工具の切り込み
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peck: Option<String>,
    /// 底で止まる秒数
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dwell: Option<f64>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub start: Start,
    #[serde(default, skip_serializing_if = "is_default")]
    pub order: Order,
    /// 穴と穴の間を、穴の上の低い高さ(送り高さ + この余裕)で移る。空なら毎回安全高さへ上げる
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stay_low: Option<String>,
}

fn breakthrough() -> String {
    "0.3 mm".into()
}

impl Drill {
    /// 部品の穴をすべて自動で拾う穴あけ(残りの項目を `..Drill::new(base)` で埋めるのに使う)。
    pub fn new(base: OpBase) -> Drill {
        Drill {
            base,
            diameter: None,
            breakthrough: breakthrough(),
            faces: vec![],
            same_diameter: false,
            holes: vec![],
            part: HolePart::Wall,
            points: None,
            depth: None,
            cycle: Cycle::Auto,
            peck: None,
            dwell: None,
            start: Start::Hole,
            order: Order::Nearest,
            stay_low: None,
        }
    }

    fn exprs(&self) -> Vec<&String> {
        let mut e = vec![&self.breakthrough];
        e.extend(
            [&self.diameter, &self.depth, &self.peck, &self.stay_low]
                .into_iter()
                .flatten(),
        );
        e
    }
}

/// 削る穴 1 つ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Target {
    bore: Bore,
    /// 選んだ順
    rank: usize,
    /// スケッチの点(穴の無い所)
    point: bool,
    /// 皿の径と角度
    countersink: Option<(f64, f64)>,
}

/// 穴 1 つの削り方。
#[derive(Debug, Clone, PartialEq)]
enum Cut {
    Plunge {
        bottom: f64,
    },
    Helical {
        radii: Vec<f64>,
        bottom: f64,
    },
    /// V ビット: 先端の深さと、回る円の半径
    Spot {
        tip: f64,
        ring: f64,
    },
}

fn same_place(a: &Bore, b: &Bore) -> bool {
    (a.center[0] - b.center[0]).hypot(a.center[1] - b.center[1]) < 1e-3
        && (a.radius - b.radius).abs() < 1e-3
        && (a.z_max - b.z_max).abs() < 1e-3
}

/// 削り始める高さと送り高さ。
fn heights(p: &Planner, hole_top: f64, start: Start) -> Heights {
    let stock_top = p.info.stock_max[2];
    let feed_h = (p.info.retract_z - stock_top).max(0.0);
    let top = p.top(match start {
        Start::Hole if hole_top < p.model.max[2] - 1e-3 => hole_top,
        _ => stock_top,
    });
    Heights {
        top,
        r: (top + feed_h).min(p.info.retract_z),
    }
}

/// 世界座標の点を、セットアップの座標にする(裏返しは 180° 回すので、同じ回し方で戻る)。
fn to_setup(p: &Prepared, w: [f64; 3]) -> [f64; 3] {
    let i = &p.info;
    let q = i.flip.point(w, i.flip_center);
    [q[0] - i.origin[0], q[1] - i.origin[1], q[2] - i.origin[2]]
}

/// 穴 `a` の上から穴 `b` の上へ、低い高さで移れるならその高さ。間の部品(まだ削っていないかも
/// しれない部品の上面はストックの上面とみなす)と固定具の高い所を越える。退避高さより低く
/// ならなければ `None`(いつもどおり安全高さで移る)。
fn low_height(
    p: &Planner,
    a: (&Bore, [f64; 2], f64),
    b: (&Bore, [f64; 2], f64),
    margin: f64,
) -> Option<f64> {
    let stock_top = p.info.stock_max[2];
    let feed_h = (p.info.retract_z - stock_top).max(0.0);
    let (from, to) = (a.1, b.1);
    let len = (to[0] - from[0]).hypot(to[1] - from[1]);
    let dir = if len > 1e-9 {
        [(to[0] - from[0]) / len, (to[1] - from[1]) / len]
    } else {
        [1.0, 0.0]
    };
    let side = [-dir[1], dir[0]];
    let tr = p.tool.diameter.max(p.tool.shank()) / 2.0;
    // 穴の中と、穴の上で上下する工具の真下(そこは送り高さより低いとわかっている)は見ない
    let inside =
        |s: [f64; 2], h: &Bore| (s[0] - h.center[0]).hypot(s[1] - h.center[1]) < h.radius.max(tr);
    let mut top = f64::NEG_INFINITY;
    let n = (len / 0.5).ceil().max(1.0) as usize;
    for i in 0..=n {
        let t = i as f64 / n as f64;
        let q = [
            from[0] + (to[0] - from[0]) * t,
            from[1] + (to[1] - from[1]) * t,
        ];
        for k in [-1.0, -0.5, 0.0, 0.5, 1.0] {
            let s = [q[0] + side[0] * tr * k, q[1] + side[1] * tr * k];
            if inside(s, a.0) || inside(s, b.0) {
                continue;
            }
            let z = match p.model.top_at(s) {
                Some(z) if z < p.model.max[2] - 1e-3 => z,
                _ => stock_top,
            };
            top = top.max(z);
        }
    }
    let lo = [from[0].min(to[0]) - tr, from[1].min(to[1]) - tr];
    let hi = [from[0].max(to[0]) + tr, from[1].max(to[1]) + tr];
    for (fl, fh) in p.info.fixtures.iter().chain(&p.info.keep_out) {
        if fl[0] < hi[0] && fh[0] > lo[0] && fl[1] < hi[1] && fh[1] > lo[1] {
            top = top.max(fh[2]);
        }
    }
    let z = (top + feed_h + margin).max(a.2).max(b.2);
    (z < p.info.retract_z - 1e-9).then_some(z)
}

impl Drill {
    /// 削る穴を集める。
    fn targets(
        &self,
        ctx: &EvalCtx<'_>,
        p: &Prepared,
        diameter: Option<f64>,
    ) -> Result<Vec<Target>, EvalError> {
        let all: Vec<&Bore> = p.model.bores.iter().filter(|b| b.hole).collect();
        let fits = |b: &Bore| diameter.is_none_or(|d| (b.diameter() - d).abs() <= holes::SAME);
        let mut out: Vec<Target> = Vec::new();
        let push = |out: &mut Vec<Target>, t: Target| {
            if !out.iter().any(|o| same_place(&o.bore, &t.bore)) {
                out.push(t);
            }
        };
        let hole = |b: &Bore, rank: usize, countersink| Target {
            bore: b.clone(),
            rank,
            point: false,
            countersink,
        };
        if self.part == HolePart::Countersink && self.holes.is_empty() {
            return Err(invalid(
                "皿の面取りは、穴の機能から作った穴あけで使います(皿の径と角度を穴の機能から読むため)",
            ));
        }
        if self.faces.is_empty() && self.holes.is_empty() && self.points.is_none() {
            for b in all.iter().filter(|b| fits(b)) {
                push(&mut out, hole(b, 0, None));
            }
            return Ok(out);
        }
        for (k, f) in self.faces.iter().enumerate() {
            let b = all
                .iter()
                .find(|b| b.faces.contains(f))
                .ok_or_else(|| {
                    invalid(format!(
                        "選んだ面 {f} が穴の中に見つかりません(形が変わったか、穴ではない面です。穴を選び直してください)"
                    ))
                })?;
            push(&mut out, hole(b, k, None));
        }
        if self.same_diameter && !self.faces.is_empty() {
            let picked: Vec<f64> = out.iter().map(|t| t.bore.diameter()).collect();
            for b in &all {
                if picked
                    .iter()
                    .any(|d| (b.diameter() - d).abs() <= holes::SAME)
                {
                    push(&mut out, hole(b, self.faces.len(), None));
                }
            }
        }
        let mut rank = self.faces.len() + 1;
        for &fid in &self.holes {
            let section = ctx
                .output(fid, kmb_interchange::HOLE_OUTPUT)
                .ok()
                .and_then(kmb_interchange::HoleInfo::from_value)
                .ok_or_else(|| invalid(format!("{fid} は穴の機能ではありません")))?;
            let countersink =
                match self.part {
                    HolePart::Countersink => Some(section.countersink.ok_or_else(|| {
                        invalid(format!("穴の機能 {fid} は皿の穴ではありません"))
                    })?),
                    _ => None,
                };
            let mut found: Vec<(u32, &Bore)> = all
                .iter()
                .filter_map(|b| holes::bore_of(b, fid, self.part).map(|n| (n, *b)))
                .collect();
            if found.is_empty() {
                return Err(invalid(format!(
                    "穴の機能 {fid} の{}が、このセットアップの形にありません(穴の種類が変わったか、上から削れない穴です)",
                    self.part.label()
                )));
            }
            found.sort_by_key(|(n, _)| *n);
            for (n, b) in found {
                push(&mut out, hole(b, rank + n as usize, countersink));
            }
            rank += 1 << 20;
        }
        if diameter.is_some() {
            out.retain(|t| fits(&t.bore));
        }
        if let Some(sk) = self.points {
            out.extend(self.sketch_points(ctx, p, sk, rank)?);
        }
        Ok(out)
    }

    /// スケッチの単独の点と円の中心に、深さ `depth` の穴を置く。
    fn sketch_points(
        &self,
        ctx: &EvalCtx<'_>,
        p: &Prepared,
        sk: FeatureId,
        rank: usize,
    ) -> Result<Vec<Target>, EvalError> {
        let depth = match &self.depth {
            Some(d) => length(ctx, d, "点の深さ")?,
            None => {
                return Err(invalid(
                    "スケッチの点にあける深さ(点の深さ)を入れてください",
                ));
            }
        };
        if depth <= 0.0 {
            return Err(invalid("点の深さは 0 より大きくしてください"));
        }
        let pts = Points::from_value(ctx.output(sk, "points")?)
            .ok_or_else(|| invalid(format!("{sk} はスケッチではありません")))?;
        let mut local: Vec<[f64; 2]> = pts
            .lone
            .iter()
            .filter_map(|id| pts.points.get(id).copied())
            .collect();
        let mut plane = pts.plane;
        if let Some(c) = Curves::from_value(ctx.output(sk, "curves")?) {
            plane = c.plane;
            for s in &c.segments {
                if let Segment::Circle { center, .. } = s {
                    local.push(*center);
                }
            }
        }
        if local.is_empty() {
            return Err(invalid("スケッチに、穴をあける単独の点か円がありません"));
        }
        let mut out: Vec<Target> = Vec::new();
        for (k, l) in local.into_iter().enumerate() {
            let w = to_setup(p, plane.to_world(l));
            let c = [w[0], w[1]];
            let top = p.model.top_at(c).unwrap_or(p.info.stock_max[2]);
            let b = Bore {
                center: c,
                radius: p.r(),
                z_min: top - depth,
                z_max: top,
                hole: true,
                faces: vec![],
            };
            if out.iter().any(|t| same_place(&t.bore, &b)) {
                continue;
            }
            out.push(Target {
                bore: b,
                rank: rank + k,
                point: true,
                countersink: None,
            });
        }
        Ok(out)
    }

    /// 合う工具の候補(誤りの文に添える)。
    fn tool_hint(&self, ctx: &EvalCtx<'_>, diameter: f64) -> String {
        let Some(lib) = ctx
            .output(self.base.tools, "tools")
            .ok()
            .and_then(ToolLibrary::from_value)
        else {
            return String::new();
        };
        let c = holes::candidates(&lib, diameter, self.part, false);
        if c.is_empty() {
            let need = holes::Need {
                kind: ToolKind::Drill,
                diameter,
                angle: 118.0,
            };
            return format!(
                "。工具ライブラリに合う工具がありません(要る工具: {})",
                need.text()
            );
        }
        let names: Vec<String> = c
            .iter()
            .map(|t| format!("T{} {}", t.number, t.name))
            .collect();
        format!("。使える工具: {}", names.join("、"))
    }
}

/// V ビットで縁につける面取りの幅(もみつけ)。
const RIM: f64 = 0.2;

/// ホストが解いた値(`plan` の入力の `params`)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrillPlan {
    pub extra: f64,
    pub margin: Option<f64>,
    pub plunge: Plunge,
    pub targets: Vec<Target>,
    pub hints: Vec<String>,
    pub start: Start,
    pub part: HolePart,
    pub cycle: Cycle,
    pub order: Order,
}

/// 戦略の純関数: 解いた入力だけから経路を作る(文書とカーネルに触らない)。
pub fn plan(input: &StrategyInput) -> Result<StrategyOutput, String> {
    run(input).map_err(plan_error)
}

fn run(input: &StrategyInput) -> Result<StrategyOutput, EvalError> {
    let mut p = Planner::new(input);
    let DrillPlan {
        extra,
        margin,
        plunge,
        targets,
        hints,
        start,
        part,
        cycle,
        order,
    } = input.params().map_err(invalid)?;
    let mut tp = p.toolpath();
    // 穴ごとの削り方と高さ
    let mut cuts: Vec<(Cut, Heights)> = Vec::new();
    let mut helical_with_cycle = false;
    let tool = p.tool.clone();
    let half = (tool.tip_angle.to_radians() / 2.0).tan();
    for (ti, t) in targets.iter().enumerate() {
        let b = &t.bore;
        if tool.kind == ToolKind::VBit {
            if half.is_nan() || half <= 1e-6 {
                return Err(invalid("V ビットの刃先角を決めてください"));
            }
            if let Some((sd, angle)) = t.countersink {
                // 穴の壁の上端が皿の円すいの下端。皿の上(面)まで円すいを延ばす
                let slope = (angle / 2.0).tan();
                let surface = b.z_max + (sd / 2.0 - b.radius).max(0.0) / slope;
                let reach = p.r().min(sd / 2.0);
                let tip = surface - reach / half;
                p.check_depth(surface, tip)?;
                if (tool.tip_angle - angle.to_degrees()).abs() > 0.5 {
                    p.warnings.push(format!(
                        "V ビットの刃先角 {}° は皿の角度 {:.0}° と違うので、皿の面がずれます",
                        tool.tip_angle,
                        angle.to_degrees()
                    ));
                }
                cuts.push((
                    Cut::Spot {
                        tip,
                        ring: (sd / 2.0 - reach).max(0.0),
                    },
                    heights(&p, surface, start),
                ));
                continue;
            }
            // 穴が工具より太ければ、刃の径いっぱいまでの位置決めだけ
            let tip = if t.point {
                b.z_min
            } else {
                b.z_max - (b.radius + RIM).min(p.r()) / half
            };
            // 穴の縁より上は前の加工で削れている(削れていなければシミュレーションで分かる)
            p.check_depth(b.z_max, tip)?;
            if !t.point {
                // 縁の面取りの幅
                tp.edge_break = tp.edge_break.max(RIM / half + 0.05);
            }
            cuts.push((Cut::Spot { tip, ring: 0.0 }, heights(&p, b.z_max, start)));
            continue;
        }
        if part == HolePart::Countersink {
            return Err(invalid("皿の面取りには V ビットを選んでください"));
        }
        let h = heights(&p, b.z_max, start);
        let through = !t.point && (b.z_min - p.model.min[2]).abs() < 1e-3;
        let mut bottom = p.bottom(if through { b.z_min - extra } else { b.z_min });
        let same = (b.diameter() - tool.diameter).abs() <= holes::SAME;
        let cut = match tool.kind {
            ToolKind::Drill if same => {
                if through {
                    bottom -= tool.tip_length();
                }
                Cut::Plunge { bottom }
            }
            // ブルノーズも平と同じに削る(底の隅に角 R が残る)
            ToolKind::FlatEnd | ToolKind::BullNose if b.radius > p.r() + holes::SAME => {
                helical_with_cycle |= cycle != Cycle::Auto;
                Cut::Helical {
                    radii: cycle::helical_radii(
                        b.radius - p.r(),
                        p.r(),
                        tool.diameter * p.cutting.stepover,
                    ),
                    bottom,
                }
            }
            ToolKind::FlatEnd | ToolKind::BullNose if same => Cut::Plunge { bottom },
            _ => {
                return Err(invalid(format!(
                    "径 {:.2} mm の穴は工具 {} では削れません{}",
                    b.diameter(),
                    tool.name,
                    hints[ti]
                )));
            }
        };
        let bottom = match &cut {
            Cut::Plunge { bottom } | Cut::Helical { bottom, .. } => *bottom,
            Cut::Spot { tip, .. } => *tip,
        };
        p.check_depth(h.top, bottom)?;
        cuts.push((cut, h));
    }
    if helical_with_cycle {
        p.warnings
            .push("らせんで削る穴には、サイクル・ペックの深さ・停留を使いません".into());
    }
    // 並べる順
    let entry = |t: &Target, c: &Cut| match c {
        Cut::Helical { radii, .. } => [t.bore.center[0] + radii[0], t.bore.center[1]],
        _ => t.bore.center,
    };
    let pts: Vec<[f64; 2]> = targets.iter().map(|t| t.bore.center).collect();
    let ranks: Vec<usize> = targets.iter().map(|t| t.rank).collect();
    let start = [p.info.stock_min[0], p.info.stock_min[1]];
    let idx = order::order(&pts, &ranks, order, start);
    let mut prev: Option<usize> = None;
    for &i in &idx {
        let (t, (cut, h)) = (&targets[i], &cuts[i]);
        let e = entry(t, cut);
        let low = match (prev, margin) {
            (Some(j), Some(m)) => low_height(
                &p,
                (&targets[j].bore, targets[j].bore.center, cuts[j].1.r),
                (&t.bore, e, h.r),
                m,
            ),
            _ => None,
        };
        match low {
            Some(z) => {
                let cur = tp.moves.last().map(|m| m.to()).unwrap_or([e[0], e[1], z]);
                if cur[2] < z - 1e-9 {
                    tp.moves.push(Move::Rapid {
                        to: [cur[0], cur[1], z],
                    });
                }
                tp.moves.push(Move::Linear {
                    to: [e[0], e[1], z],
                    feed: p.cutting.feed,
                });
            }
            None => approach(&mut tp, e),
        }
        let c = t.bore.center;
        match cut {
            Cut::Plunge { bottom } => cycle::plunge(&mut tp, c, *h, *bottom, &plunge),
            Cut::Helical { radii, bottom } => {
                cycle::helical(&mut tp, c, radii, *h, *bottom, p.cutting.stepdown, &plunge)
            }
            Cut::Spot { tip, ring } => cycle::spot(&mut tp, c, *h, *tip, *ring, &plunge),
        }
        prev = Some(i);
    }
    retract(&mut tp);
    Ok(p.output(tp))
}

impl Feature for Drill {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = self.base.inputs()?;
        v.extend(expr_refs(self.exprs())?);
        for &h in &self.holes {
            v.push(kmb_cam_setup::output(h, kmb_interchange::HOLE_OUTPUT));
        }
        if let Some(s) = self.points {
            v.push(kmb_cam_setup::output(s, "points"));
            v.push(kmb_cam_setup::output(s, "curves"));
        }
        Ok(v)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let mut p = prepare(ctx, &self.base)?;
        let extra = length(ctx, &self.breakthrough, "貫通の深さ")?;
        let diameter = match &self.diameter {
            Some(d) => Some(length(ctx, d, "穴の径")?),
            None => None,
        };
        let peck = match &self.peck {
            Some(s) => {
                let v = length(ctx, s, "ペックの深さ")?;
                if v <= 0.0 {
                    return Err(invalid("ペックの深さは 0 より大きくしてください"));
                }
                v
            }
            None => p.cutting.stepdown,
        };
        let dwell = self.dwell.unwrap_or(0.0);
        if !(0.0..=60.0).contains(&dwell) {
            return Err(invalid("底での停留は 0〜60 秒にしてください"));
        }
        let margin = match &self.stay_low {
            Some(s) => {
                let v = length(ctx, s, "穴の間を低く移る余裕")?;
                if v < 0.0 {
                    return Err(invalid("穴の間を低く移る余裕は 0 以上にしてください"));
                }
                Some(v)
            }
            None => None,
        };
        let plunge = Plunge {
            cycle: match self.cycle {
                Cycle::Auto => Cycle::Peck,
                c => c,
            },
            peck,
            dwell,
            feed: p.cutting.feed,
            plunge: p.cutting.plunge,
        };
        let targets = self.targets(ctx, &p, diameter)?;
        if targets.is_empty() {
            return Err(invalid("削る穴が見つかりません"));
        }
        let hints = targets
            .iter()
            .map(|t| self.tool_hint(ctx, t.bore.diameter()))
            .collect();
        let params = DrillPlan {
            extra,
            margin,
            plunge,
            targets,
            hints,
            start: self.start,
            part: self.part,
            cycle: self.cycle,
            order: self.order,
        };
        let input = p.input(ctx.feature_id(), params, Default::default());
        let out = plan(&input);
        p.finish_plan(self.base.setup, out)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        let mut c = self.clone();
        let mut e = vec![&mut c.breakthrough];
        e.extend(
            [
                c.diameter.as_mut(),
                c.depth.as_mut(),
                c.peck.as_mut(),
                c.stay_low.as_mut(),
            ]
            .into_iter()
            .flatten(),
        );
        Ok(c.base
            .rename(e, old, new)?
            .then(|| serde_json::to_value(&c).expect("JSON にできる")))
    }
}

/// つつきで削る(ピン穴)。
fn peck(tp: &mut Toolpath, p: &Planner, b: &Bore, top: f64, bottom: f64) {
    approach(tp, b.center);
    let plunge = Plunge {
        cycle: Cycle::Peck,
        peck: p.cutting.stepdown,
        dwell: 0.0,
        feed: p.cutting.feed,
        plunge: p.cutting.plunge,
    };
    let h = Heights {
        top,
        r: tp.retract_z,
    };
    cycle::plunge(tp, b.center, h, bottom, &plunge);
    retract(tp);
}

/// らせんで削る(ピン穴)。
fn helical(tp: &mut Toolpath, p: &Planner, b: &Bore, top: f64, bottom: f64) {
    let radii = cycle::helical_radii(
        b.radius - p.r(),
        p.r(),
        p.tool.diameter * p.cutting.stepover,
    );
    approach(tp, [b.center[0] + radii[0], b.center[1]]);
    let h = Heights {
        top,
        r: tp.retract_z,
    };
    let plunge = Plunge {
        cycle: Cycle::Auto,
        peck: p.cutting.stepdown,
        dwell: 0.0,
        feed: p.cutting.feed,
        plunge: p.cutting.plunge,
    };
    cycle::helical(tp, b.center, &radii, h, bottom, p.cutting.stepdown, &plunge);
    retract(tp);
}

/// ピンを並べる線の向き。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PinLine {
    /// 前後(Y)の向きの線。Y 軸のまわりの裏返しに使う
    #[default]
    AlongY,
    /// 左右(X)の向きの線。X 軸のまわりの裏返しに使う
    AlongX,
}

/// 裏返しの位置合わせのピン穴(`cam.pin_holes@1`)。部品の外形の箱の中心を通る線の上、
/// ストックの両端の余白に 2 つ置き、ストックの下(捨て板)まで削る。裏返しの軸と同じ線の上に
/// あるので、裏返しても同じ位置に来る。表と裏の両方のセットアップに同じ設定で足す。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PinHoles {
    #[serde(flatten)]
    pub base: OpBase,
    /// ピンの径
    #[serde(default = "pin_diameter")]
    pub diameter: String,
    /// ストックの端からピン穴の中心まで
    #[serde(default = "pin_inset")]
    pub inset: String,
    /// ストックの下面より下(捨て板)へ削る深さ
    #[serde(default = "pin_below")]
    pub below: String,
    #[serde(default)]
    pub line: PinLine,
}

fn pin_diameter() -> String {
    "3.175 mm".into()
}

fn pin_inset() -> String {
    "5 mm".into()
}

fn pin_below() -> String {
    "3 mm".into()
}

/// ピン穴の中心(WCS)。部品にかかれば誤り。
pub fn pin_centers(
    p: &Prepared,
    inset: f64,
    radius: f64,
    line: PinLine,
) -> Result<[[f64; 2]; 2], EvalError> {
    let (lo, hi) = (p.info.stock_min, p.info.stock_max);
    let (mlo, mhi) = (p.model.min, p.model.max);
    let c = [(mlo[0] + mhi[0]) / 2.0, (mlo[1] + mhi[1]) / 2.0];
    let k = match line {
        PinLine::AlongY => 1,
        PinLine::AlongX => 0,
    };
    let (a, b) = (lo[k] + inset, hi[k] - inset);
    if a + radius > mlo[k] - 1e-6 || b - radius < mhi[k] + 1e-6 {
        return Err(invalid(format!(
            "ピン穴が部品にかかります(ストックの端から {inset:.1} mm、部品までの余白は {:.1} mm と {:.1} mm)。ストックの余白を広げてください",
            mlo[k] - lo[k],
            hi[k] - mhi[k]
        )));
    }
    Ok(if k == 1 {
        [[c[0], a], [c[0], b]]
    } else {
        [[a, c[1]], [b, c[1]]]
    })
}

/// ホストが解いた値(`plan_pins` の入力の `params`)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PinPlan {
    pub d: f64,
    pub top: f64,
    pub bottom: f64,
    pub same: bool,
    pub centers: Vec<[f64; 2]>,
}

/// 戦略の純関数: 解いた入力だけから経路を作る(文書とカーネルに触らない)。
pub fn plan_pins(input: &StrategyInput) -> Result<StrategyOutput, String> {
    run_pins(input).map_err(plan_error)
}

fn run_pins(input: &StrategyInput) -> Result<StrategyOutput, EvalError> {
    let p = Planner::new(input);
    let PinPlan {
        d,
        top,
        bottom,
        same,
        centers,
    } = input.params().map_err(invalid)?;
    let mut tp = p.toolpath();
    for c in centers {
        let b = Bore {
            center: c,
            radius: d / 2.0,
            z_min: bottom,
            z_max: top,
            hole: true,
            faces: vec![],
        };
        match p.tool.kind {
            ToolKind::Drill | ToolKind::FlatEnd | ToolKind::BullNose if same => {
                peck(&mut tp, &p, &b, top, bottom)
            }
            ToolKind::FlatEnd | ToolKind::BullNose if b.radius > p.r() + 0.05 => {
                helical(&mut tp, &p, &b, top, bottom)
            }
            _ => {
                return Err(invalid(format!(
                    "径 {d:.2} mm のピン穴は工具 {} では削れません",
                    p.tool.name
                )));
            }
        }
    }
    Ok(p.output(tp))
}

impl Feature for PinHoles {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = self.base.inputs()?;
        v.extend(expr_refs([&self.diameter, &self.inset, &self.below])?);
        Ok(v)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let mut p = prepare(ctx, &self.base)?;
        let d = length(ctx, &self.diameter, "ピンの径")?;
        let inset = length(ctx, &self.inset, "端からの距離")?;
        let below = length(ctx, &self.below, "捨て板へ削る深さ")?;
        if d <= 0.0 || below < 0.0 {
            return Err(invalid(
                "ピンの径は 0 より大きく、深さは 0 以上にしてください",
            ));
        }
        let centers = pin_centers(&p, inset, d / 2.0, self.line)?;
        let top = p.info.stock_max[2];
        let same = (d - p.tool.diameter).abs() <= 0.05;
        let mut bottom = p.info.stock_min[2] - below;
        // ドリルの先の円すいの分も下げ、捨て板の深さまで径いっぱいに開ける(穴あけの貫通と同じ)
        if p.tool.kind == ToolKind::Drill && same {
            bottom -= p.tool.tip_length();
        }
        p.check_depth(top, bottom)?;
        let params = PinPlan {
            d,
            top,
            bottom,
            same,
            centers: centers.to_vec(),
        };
        let input = p.input(ctx.feature_id(), params, Default::default());
        let out = plan_pins(&input);
        p.finish_plan(self.base.setup, out)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        let mut c = self.clone();
        let e = vec![&mut c.diameter, &mut c.inset, &mut c.below];
        Ok(c.base
            .rename(e, old, new)?
            .then(|| serde_json::to_value(&c).expect("JSON にできる")))
    }
}

pub fn register(reg: &mut Registry) {
    circular::register(reg);
    thread::register(reg);
    reg.register_described(
        kmb_cam_setup::operation(
            PIN_HOLES,
            "ピン穴",
            "裏返しで位置を合わせるピン穴を、部品の中心を通る線の上、ストックの両端の余白に 2 つ削る。表と裏の両方のセットアップに同じ設定で足す",
            vec![
                Field::expr("diameter", "ピンの径").length(),
                Field::expr("inset", "ストックの端からの距離").length(),
                Field::expr("below", "捨て板へ削る深さ").length(),
                Field::choice(
                    "line",
                    "並べる線",
                    &[
                        ("along_y", "前後の線(Y 軸のまわりに裏返す時)"),
                        ("along_x", "左右の線(X 軸のまわりに裏返す時)"),
                    ],
                ),
            ],
        )
        .icon("pins"),
        |d| Ok(Box::new(kmb_cam_setup::parse::<PinHoles>(d)?)),
    );
    reg.register_described(
        kmb_cam_setup::operation(
            DRILL,
            "穴あけ",
            "上から削れる丸い穴をドリルかヘリカルで削る。3D のビューで穴を選ぶとその穴、選ばなければすべての穴。V ビットを選ぶと、ドリルの前のもみつけ(位置決めと縁の面取り)になる",
            kmb_cam_setup::with_heights(vec![
                Field::opt_expr("diameter", "穴の径").length().help("空ならすべての穴"),
                Field::expr("breakthrough", "貫通でさらに下げる深さ").length(),
                Field::bool("same_diameter", "同じ径の穴も")
                    .help("3D のビューで選んだ穴と同じ径の穴を、すべて削る"),
                Field::choice(
                    "part",
                    "削る所",
                    &[
                        ("wall", "穴"),
                        ("counterbore", "ザグリ"),
                        ("countersink", "皿の面取り(V ビット)"),
                    ],
                )
                .help("穴の機能から作った穴あけで、穴のどこを削るか"),
                Field::opt_output("points", "点のスケッチ", "points")
                    .help("スケッチの単独の点と円の中心に、穴の無い所でもあける(下穴・もみつけ)"),
                Field::opt_expr("depth", "点の深さ").length()
                    .help("スケッチの点にあける深さ(点の所の部品の上から。V ビットは先端の深さ)"),
                Field::choice(
                    "cycle",
                    "サイクル",
                    &[
                        ("auto", "工具で決める"),
                        ("straight", "1 回で下げる"),
                        ("peck", "つつき(毎回上まで戻す)"),
                        ("chip_break", "切りくず切断(少し戻す)"),
                    ],
                )
                .help("工具で決める: ドリルと同じ径の平はつつき、細い平はらせん。らせんにはサイクルを使わない"),
                Field::opt_expr("peck", "ペックの深さ").length()
                    .unless("cycle", "auto")
                    .help("1 回に下げる深さ。空なら工具の切り込み"),
                Field::opt_number("dwell", "停留時間(秒)").plain("秒").unless("cycle", "auto"),
                Field::choice(
                    "start",
                    "自動のトップ高さ",
                    &[("hole", "穴の上"), ("stock", "ストックの上面")],
                )
                .help("穴の上: ポケットの底の穴は底の上から削り始める(部品の上面の穴はストックの上面から)"),
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
                Field::opt_expr("stay_low", "穴の間を低く移る余裕").length().help(
                    "空なら毎回安全高さへ上げる。入れると、穴の上の送り高さ + この値で次の穴へ移る(間の部品や固定具の高い所は越える)",
                ),
            ]),
        )
        .icon("drill"),
        |d| Ok(Box::new(kmb_cam_setup::parse::<Drill>(d)?)),
    );
}

/// この crate の名乗り: 登録する種類と、加工を足す操作(画面の名前・まとまり・初めの値)。
pub fn module() -> kmb_cam_setup::StrategyModule {
    use kmb_cam_setup::{FaceMenu, Strategy, StrategyModule, to_data};
    use kmb_cam_tool::ToolKind;
    StrategyModule::new(
        "kmb-strategy-drill",
        register,
        vec![
            Strategy::new(DRILL, "cam.add.drill", "穴あけ", |p| {
                to_data(Drill {
                    // 3D のビューで穴の壁(平らでない面)を選んでいれば、その穴だけ
                    faces: p.wall_face().into_iter().collect(),
                    // ドリルがあればそれ、なければいちばん細い平(穴より細ければらせんで削れる)
                    ..Drill::new(OpBase {
                        tool: p
                            .tool_of_kind(ToolKind::Drill)
                            .or_else(|| p.smallest_flat())
                            .unwrap_or(1),
                        ..p.base()
                    })
                })
            })
            .order(40)
            .menu("穴")
            .icon("drill")
            .aliases("あなあけ drill hole どりる")
            .face_menu(FaceMenu::Wall)
            .commands(hole_ops::commands),
            Strategy::new(PIN_HOLES, "cam.add.pin_holes", "ピン穴", |p| {
                // 裏返しの軸に合わせる(X 軸のまわりなら左右の線)
                let around_x = p
                    .setup_data()
                    .and_then(|d| d.get("flip")?.as_str().map(|v| v == "around_x"))
                    .unwrap_or(false);
                to_data(PinHoles {
                    base: p.base(),
                    diameter: "3.175 mm".into(),
                    inset: "5 mm".into(),
                    below: "3 mm".into(),
                    line: if around_x {
                        PinLine::AlongX
                    } else {
                        PinLine::AlongY
                    },
                })
            })
            .order(41)
            .menu("穴")
            .icon("pins")
            // 前には出さない(Kie 2026-10-21 の画板)
            .not_front()
            .aliases("ぴんあな pin dowel うらがえし flip りょうめん")
            .hint("裏返しの位置合わせのピン穴。表と裏の両方のセットアップに足す"),
            Strategy::new(CIRCULAR, "cam.add.circular", "ボア・円形", |p| {
                to_data(Circular {
                    // 3D のビューで穴かボスの壁(平らでない面)を選んでいれば、それだけ
                    faces: p.wall_face().into_iter().collect(),
                    ..Circular::new(OpBase {
                        tool: p.smallest_flat().unwrap_or(1),
                        ..p.base()
                    })
                })
            })
            .order(7)
            // ボア・円形は 2D のまとまり(Kie 2026-10-21 の画板)
            .menu("2D")
            .icon("drill")
            .not_front()
            .aliases("ぼあ bore えんけい circular ぼす boss へりかる helical あな")
            .hint("上から削れる丸い穴とボスを、壁に沿う円かららせんで削る。穴かボスの壁を選ぶとそれ、選ばなければすべて")
            .face_menu(FaceMenu::Wall)
            .no_reuse(),
            Strategy::new(THREAD, "cam.add.thread", "ねじ切り", |p| {
                to_data(Thread {
                    // 3D のビューで選んだ穴かボスの壁(Shift・Ctrl+クリックで足した物も)
                    faces: p.wall_refs(),
                    ..Thread::new(p.with_tool(ToolKind::ThreadMill))
                })
            })
            .order(6)
            .icon("threadmill")
            .not_front()
            .aliases("ねじきり thread ねじ めねじ おねじ たっぷ tap threadmill すれっど")
            .hint("ねじ切りミルで、選んだ穴かボスの壁にねじを切る。工具ライブラリにねじ切りミルが要る")
            .face_menu(FaceMenu::Wall)
            .no_reuse(),
        ],
    )
}
