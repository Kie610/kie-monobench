//! 負荷制御粗取り(`cam.adaptive2d@1`)。平らな底の面の上を、工具が材料に掛かる幅を
//! ほぼ一定に保ちながら、ストックの上面から底まで削る。
//!
//! 等間隔のオフセットをたどるポケットは、隅や輪の切り替わりで工具が材料に深く掛かる
//! (全溝に近くなる)。ここでは削った所を格子で持ち、1 歩ごとに新たに削る面積が
//! 「横の切り込み × 歩幅」に合う向きへ進む([`plan`])。そのぶん 1 段を深くできる。
//!
//! 各段では、らせんで入り、計画の線を順にたどる(線から線へは、削った所の中を通れれば
//! 工具を下げたまま移る)。最後に壁(外周と島)に沿って 1 周し、壁の際の薄い皮を削る。
//! 計画は平面の上で 1 度だけ行い、どの段も同じ線をたどる。
//! 底の面は複数選べ、スケッチの閉じた輪郭も削る。底の高さが同じ領域は 1 つの計画にまとめる。
//! 最小の切削半径があれば、工具の中心が入る所の内側の角を丸め、線が角へ食い込まないようにする。

pub mod plan;

use jc_cam_geom::{Area, Loop, P2, Vertex, offset_area};
use jc_cam_setup::{
    EntryKind, EntryOpts, EntryPlan, EntryRegion, Floor, FloorPick, OpBase, Planner, RestSize,
    StrategyInput, StrategyOutput, approach, entry_fields, expr_refs, floor_fields, floor_inputs,
    floors, invalid, last_step_field, leave_fields, leaves, length, loop_moves, min_radius_field,
    opt_length, plan_error, prepare, ramp_angle, ramp_moves, retract, round_corners, start_near,
    unreachable_area,
};
use jc_cam_tool::ToolLibrary;
use jc_core::{EvalCtx, EvalError, Feature, InputRef, Outputs, Registry};
use jc_core::{FeatureId, Field};
use jc_toolpath::{Move, Role, Toolpath};
use plan::{Params, Plan, Step};
use serde::{Deserialize, Serialize};

pub const ADAPTIVE: &str = "cam.adaptive2d@1";

/// 横の切り込みを決めない時の、工具径に対する比
pub const DEFAULT_LOAD: f64 = 0.15;
/// らせんで入る角度を決めない時の角度(度)
pub const DEFAULT_RAMP_DEG: f64 = 2.0;

/// 負荷制御粗取りの 1 段の深さ。溝(工具径いっぱい)で削る時の深さ `slot` から、
/// 削る断面(深さ × 横の切り込み)が同じになるよう深くする。刃長の 8 割と、
/// 工具径の 2 倍を上限にする(上限の値は一般的な目安で、推測 A)。溝の深さより浅くはしない。
pub fn adaptive_stepdown(slot: f64, diameter: f64, load: f64, flute_length: f64) -> f64 {
    (slot * diameter / load)
        .min(0.8 * flute_length)
        .min(2.0 * diameter)
        .max(slot)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Adaptive {
    #[serde(flatten)]
    pub base: OpBase,
    /// 底の面の名前(スケッチの輪郭だけを削る時は空)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub face: String,
    /// ほかの底の面。1 つの操作でまとめて削る
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub faces: Vec<String>,
    /// 閉じた輪郭を出すスケッチ。輪郭の内側も削る
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<FeatureId>,
    /// スケッチの平面から底までの深さ。空ならスケッチの平面が底
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<String>,
    /// 壁(径方向)に残す厚さ。`leave_floor` が空なら底にも同じ厚さを残す
    #[serde(default = "zero")]
    pub leave: String,
    /// 底(軸方向)に残す厚さ。空なら `leave` と同じ
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leave_floor: Option<String>,
    /// 横の切り込み(工具が材料に掛かる幅。例 `0.5 mm`)。空なら工具径の 15 %
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load: Option<String>,
    /// らせんで入る角度(例: `2 deg`)。空なら 2 deg
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ramp_angle: Option<String>,
    /// 残り削り: 前に同じ面を削った太い工具の番号。その工具が届かなかった所だけを削る
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rest_tool: Option<u32>,
    /// 残り削りの前の工具を直径とコーナー半径で打つ時(番号が空の時に使う)
    #[serde(flatten)]
    pub rest_size: RestSize,
    /// 最小の切削半径。工具の中心が入る所の内側の角を、この半径以上に丸める
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_radius: Option<String>,
    /// 入る位置(セットアップの座標の XY)。最初はそこに最も近い、入れる所から入る
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<[f64; 2]>,
    /// 入り方(らせん・真下・下穴・ストックの外から)
    #[serde(flatten)]
    pub entry_opts: EntryOpts,
}

/// 負荷制御粗取りで選べる入り方。
const ENTRY_KINDS: &[EntryKind] = &[
    EntryKind::Auto,
    EntryKind::Helix,
    EntryKind::Plunge,
    EntryKind::Predrill,
    EntryKind::Outside,
];

fn zero() -> String {
    "0 mm".into()
}

impl Adaptive {
    /// 底の面 `face` を削る負荷制御粗取り(ほかの欄は初めの値)。
    pub fn on_face(base: OpBase, face: impl Into<String>) -> Adaptive {
        Adaptive {
            base,
            face: face.into(),
            faces: Vec::new(),
            profile: None,
            depth: None,
            leave: zero(),
            leave_floor: None,
            load: None,
            ramp_angle: None,
            rest_tool: None,
            rest_size: Default::default(),
            min_radius: None,
            entry: None,
            entry_opts: Default::default(),
        }
    }

    /// 削る領域の選び方。
    pub fn pick(&self) -> FloorPick<'_> {
        FloorPick {
            face: &self.face,
            faces: &self.faces,
            profile: self.profile,
            depth: &self.depth,
        }
    }

    fn exprs_mut(&mut self) -> impl Iterator<Item = &mut String> {
        std::iter::once(&mut self.leave).chain(
            [
                &mut self.leave_floor,
                &mut self.load,
                &mut self.ramp_angle,
                &mut self.depth,
                &mut self.min_radius,
            ]
            .into_iter()
            .flatten()
            .chain(self.entry_opts.exprs_mut())
            .chain(self.rest_size.exprs_mut()),
        )
    }
}

/// 反時計回りの円(始点は中心の右)。
fn circle(c: P2, r: f64) -> Loop {
    Loop {
        verts: vec![
            Vertex {
                x: c[0] + r,
                y: c[1],
                bulge: 1.0,
            },
            Vertex {
                x: c[0] - r,
                y: c[1],
                bulge: 1.0,
            },
        ],
    }
}

/// 工具の今の位置。
fn here(tp: &Toolpath) -> [f64; 3] {
    tp.moves
        .last()
        .map(|m| m.to())
        .unwrap_or([0.0, 0.0, tp.safe_z])
}

/// 前の段の高さ `z_prev` まで送りで下げる(削れている所。印を付ける)。下げ始める高さを返す。
fn down_to_prev(tp: &mut Toolpath, p: &Planner, xy: P2, z_prev: f64) -> f64 {
    if z_prev < tp.retract_z {
        let s = tp.moves.len();
        tp.moves.push(Move::Linear {
            to: [xy[0], xy[1], z_prev],
            feed: p.cutting.feed,
        });
        tp.mark(s, Role::Link);
    }
    z_prev.min(tp.retract_z)
}

/// 削った所の上へ移って `z` まで下げる(前の段の高さ `z_prev` までは送りで速めに)。
fn drop_to(tp: &mut Toolpath, p: &Planner, plan: &EntryPlan, xy: P2, z: f64, z_prev: f64) {
    approach(tp, xy);
    down_to_prev(tp, p, xy, z_prev);
    let s = tp.moves.len();
    tp.moves.push(Move::Linear {
        to: [xy[0], xy[1], z],
        feed: plan.plunge_feed,
    });
    tp.mark(s, Role::Entry);
}

/// 高さ `z` で、`q` へ送りで動く(今の位置と同じなら何もしない)。
fn feed_to(tp: &mut Toolpath, q: P2, z: f64, feed: f64) {
    let c = here(tp);
    if (c[0] - q[0]).hypot(c[1] - q[1]) > 1e-9 || (c[2] - z).abs() > 1e-9 {
        tp.moves.push(Move::Linear {
            to: [q[0], q[1], z],
            feed,
        });
    }
}

/// 1 段分の経路を足す。
fn level(
    tp: &mut Toolpath,
    p: &Planner,
    entry: &EntryPlan,
    plan: &Plan,
    walls: &[Loop],
    (z, z_prev): (f64, f64),
) {
    let feed = p.cutting.feed;
    let mut down = false;
    for st in &plan.steps {
        match st {
            Step::Entry { center, radius } => {
                if *radius > 0.0 {
                    let c = circle(*center, *radius);
                    let start = [center[0] + radius, center[1]];
                    approach(tp, start);
                    let from = down_to_prev(tp, p, start, z_prev);
                    let s = tp.moves.len();
                    tp.moves.extend(ramp_moves(
                        &c,
                        from,
                        z,
                        entry.angle,
                        entry.ramp_feed,
                        entry.plunge_feed,
                    ));
                    // 下りきった高さでもう 1 周して、底を平らにする
                    tp.moves.extend(loop_moves(&c, z, entry.ramp_feed));
                    tp.mark(s, Role::Entry);
                } else {
                    drop_to(tp, p, entry, *center, z, z_prev);
                }
                down = true;
            }
            Step::Cut { link, points } => {
                match link {
                    Some(l) if down => {
                        let s = tp.moves.len();
                        for q in l {
                            feed_to(tp, *q, z, feed);
                        }
                        tp.mark(s, Role::Link);
                    }
                    _ => drop_to(tp, p, entry, points[0], z, z_prev),
                }
                for q in points {
                    feed_to(tp, *q, z, feed);
                }
                down = true;
            }
        }
    }
    // 壁(外周は反時計回り、島は時計回り = ダウンカット)に沿って 1 周し、際の皮を削る
    for l in walls {
        let c = here(tp);
        let l = start_near(l, [c[0], c[1]]);
        let start = [l.verts[0].x, l.verts[0].y];
        drop_to(tp, p, entry, start, z, z_prev);
        tp.moves.extend(loop_moves(&l, z, feed));
    }
    retract(tp);
}

/// 入り方の選び方と、計画が実際に入った所が違う時の知らせ。
fn entry_warnings(p: &mut Planner, plan: &Plan, kind: EntryKind, entry: Option<P2>) {
    let first_entry = plan.steps.iter().find_map(|s| match s {
        Step::Entry { center, .. } => Some(*center),
        _ => None,
    });
    let entered_outside = !matches!(plan.steps.first(), Some(Step::Entry { .. }));
    if kind == EntryKind::Outside && !entered_outside {
        p.warnings
            .push("ストックの外から入れる所がない(閉じたポケット)ので、らせんで入ります".into());
    }
    if kind == EntryKind::Predrill && entry.is_none() {
        p.warnings.push(
            "下穴から入るには、入る位置(下穴の中心)を置いてください。今は広い所で真下に下ります"
                .into(),
        );
    }
    if let (Some(e), Some(c)) = (entry, first_entry)
        && (c[0] - e[0]).hypot(c[1] - e[1]) > 0.5
        && !entered_outside
    {
        p.warnings.push(format!(
            "入る位置({:.1}, {:.1})には入れないので、いちばん近い({:.1}, {:.1})から入ります",
            e[0], e[1], c[0], c[1]
        ));
    }
}

/// ホストが解いた負荷制御粗取りの値(`plan` の入力の `params`)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdaptivePlan {
    pub leave: f64,
    pub leave_floor: f64,
    /// 横の切り込み(mm)
    pub load: f64,
    pub min_radius: f64,
    /// 1 段の深さを機械の条件の表から決める(加工の欄で決めていない)
    pub auto_stepdown: bool,
    pub entry_plan: EntryPlan,
    /// らせんの径を決めている
    pub helix_set: bool,
    pub floors: Vec<Floor>,
    /// 残り削りの前の工具
    pub rest: Option<jc_cam_tool::Tool>,
    pub entry: Option<P2>,
}

/// 戦略の純関数: 解いた入力だけから経路を作る(文書とカーネルに触らない)。
pub fn plan(input: &StrategyInput) -> Result<StrategyOutput, String> {
    run(input).map_err(plan_error)
}

fn run(input: &StrategyInput) -> Result<StrategyOutput, EvalError> {
    let mut p = Planner::new(input);
    let AdaptivePlan {
        leave,
        leave_floor,
        load,
        min_radius,
        auto_stepdown,
        entry_plan,
        helix_set,
        floors,
        rest: big,
        entry,
    } = input.params().map_err(invalid)?;
    if !(load > 0.0 && load <= p.r() + 1e-9) {
        return Err(invalid(format!(
            "横の切り込みは 0 より大きく、工具の半径 {:.2} mm 以下にしてください",
            p.r()
        )));
    }
    // 1 段の深さを決めていなければ、機械の条件の表(表のない機械や知らない材料は工具の条件)の
    // 溝の深さから、横の切り込みに合わせて深くする
    if auto_stepdown {
        let slot = p.info.machine.slot_stepdown.unwrap_or(p.cutting.stepdown);
        p.cutting.stepdown = adaptive_stepdown(slot, p.tool.diameter, load, p.tool.flute_length);
    }
    let top = p.top(p.info.stock_max[2]);
    for f in &floors {
        let bottom = p.bottom(f.z + leave_floor);
        if bottom >= top {
            return Err(invalid("削る深さがありません"));
        }
        p.check_depth(top, bottom)?;
    }
    if let Some(big) = &big
        && big.radius() <= p.r() + 1e-6
    {
        return Err(invalid(format!(
            "残り削りの前の工具 {} は、今の工具 {} より太くしてください",
            big.name, p.tool.name
        )));
    }
    let r = p.r() + leave;
    let (smin, smax) = (
        [p.info.stock_min[0], p.info.stock_min[1]],
        [p.info.stock_max[0], p.info.stock_max[1]],
    );
    // 底の高さが同じ領域は、1 つの計画でまとめて削る
    let mut groups: Vec<(f64, Vec<&Floor>)> = Vec::new();
    for f in &floors {
        match groups.iter_mut().find(|g| (g.0 - f.z).abs() < 1e-6) {
            Some(g) => g.1.push(f),
            None => groups.push((f.z, vec![f])),
        }
    }
    let mut tp = p.toolpath();
    // スケッチの輪郭は部品の形にない所を削るので、平面からの深さまでの食い込みは許す
    tp.edge_break = floors
        .iter()
        .filter_map(|f| f.sketch_top.map(|t| t - f.z + 0.05))
        .fold(0.0, f64::max);
    let (mut rest, mut left, mut straight, mut planned) = (0.0, 0.0, false, false);
    let helix = !matches!(entry_plan.kind, EntryKind::Plunge | EntryKind::Predrill);
    let mut region: Vec<Area> = Vec::new();
    for (z, group) in &groups {
        let areas: Vec<Area> = group.iter().map(|f| f.area.clone()).collect();
        let walls: Vec<Area> = round_corners(
            areas.iter().flat_map(|a| offset_area(a, r)).collect(),
            min_radius,
        );
        if walls.is_empty() {
            return Err(invalid("工具がポケットに入りません(工具が太すぎます)"));
        }
        let allowed = round_corners(
            areas
                .iter()
                .flat_map(|a| offset_area(a, r + 1e-3))
                .collect(),
            min_radius,
        );
        // 残り削り: 太い工具の中心が届いた所を、その半径だけ広げた領域は削れている
        let cleared: Vec<Area> = match &big {
            Some(big) => areas
                .iter()
                .flat_map(|a| offset_area(a, big.radius() + leave))
                .flat_map(|a| offset_area(&a, -big.radius()))
                .collect(),
            None => Vec::new(),
        };
        let plan = plan::plan_rest(&areas, &cleared, &allowed, {
            let kind = entry_plan.kind;
            let mut prm = Params::new(p.r(), load, 0.0, Some((smin, smax)));
            prm.entry = entry;
            prm.helix = if helix_set { entry_plan.helix_r } else { 0.0 };
            prm.plunge = matches!(kind, EntryKind::Plunge | EntryKind::Predrill);
            // 置いた入る位置や、らせん・真下・下穴を選んだ時は、外から入らない
            prm.outside = match kind {
                EntryKind::Outside => true,
                EntryKind::Auto => entry.is_none(),
                _ => false,
            };
            prm
        })
        .map_err(invalid)?;
        if big.is_some() && plan.steps.is_empty() {
            continue;
        }
        // 入り方の知らせは、最初に削る領域で 1 度だけ
        if !planned {
            entry_warnings(&mut p, &plan, entry_plan.kind, entry);
        }
        planned = true;
        // 入る位置を置く手順で塗る所: らせんなら、らせんが縮まずに入る所
        if helix {
            region.extend(
                areas
                    .iter()
                    .flat_map(|a| offset_area(a, r + entry_plan.helix_r)),
            );
        } else {
            region.extend(allowed.iter().cloned());
        }
        rest += areas
            .iter()
            .map(|a| unreachable_area(a, p.r() + min_radius, leave))
            .sum::<f64>();
        left += plan.left;
        straight |= plan
            .steps
            .iter()
            .any(|s| matches!(s, Step::Entry { radius, .. } if *radius <= 0.0));
        // ストックをまるごと囲む輪(開いた面の外周)は空を切るだけなので回らない
        let wall_loops: Vec<Loop> = walls
            .iter()
            .flat_map(|a| std::iter::once(a.outer.clone()).chain(a.holes.iter().cloned()))
            .filter(|l| {
                let (lo, hi) = l.bounds();
                !(lo[0] < smin[0] && lo[1] < smin[1] && hi[0] > smax[0] && hi[1] > smax[1])
            })
            .collect();
        let mut prev = top;
        for z in p.levels(top, p.bottom(z + leave_floor)) {
            level(&mut tp, &p, &entry_plan, &plan, &wall_loops, (z, prev));
            prev = z;
        }
    }
    if !planned {
        return Err(invalid(
            "前の工具の削り残しで、この工具が届く所はありません(残り削りは要りません)",
        ));
    }
    if rest > 0.05 {
        let why = if min_radius > 1e-6 {
            format!(
                "内側の角の半径が、工具の半径 {:.2} mm と最小の切削半径 {min_radius:.2} mm の和より小さい",
                p.r()
            )
        } else {
            format!("内側の角の半径が工具の半径 {:.2} mm より小さい", p.r())
        };
        p.warnings.push(format!(
            "工具 {} が届かない隅に、上から見て約 {rest:.2} mm² が削り残ります({why})。\
             細い工具で残りを削ってください",
            p.tool.name,
        ));
    }
    if left > 0.5 {
        p.warnings.push(format!(
            "工具が届く所のうち、上から見て約 {left:.1} mm² を削り残しました(負荷一定の線が入れなかった所)。\
             ポケットで削り直すか、横の切り込みを変えてください",
        ));
    }
    if straight {
        p.warnings.push(
            "らせんで入る広さがない所に、真下に入ります(工具が底で材料を削れる形か確かめてください)"
                .into(),
        );
    }
    let mut out = p.output(tp);
    out.entry_area = Some(EntryRegion {
        z: top,
        areas: region,
        helix,
    });
    Ok(out)
}

impl Feature for Adaptive {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = self.base.inputs()?;
        let mut c = self.clone();
        let exprs: Vec<String> = c.exprs_mut().map(|s| s.clone()).collect();
        v.extend(expr_refs(&exprs)?);
        v.extend(floor_inputs(self.pick())?);
        Ok(v)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        // ホストの側: 式・選んだ面とスケッチの輪郭・前の工具を解いて、純関数 `plan` の入力にする
        let mut p = prepare(ctx, &self.base)?;
        let (leave, leave_floor) = leaves(ctx, &self.leave, &self.leave_floor)?;
        let load = match &self.load {
            Some(e) => length(ctx, e, "最適負荷")?,
            None => DEFAULT_LOAD * p.tool.diameter,
        };
        let min_radius = opt_length(ctx, &self.min_radius, "最小切削半径")?.unwrap_or(0.0);
        let ramp = ramp_angle(ctx, &self.ramp_angle)?.unwrap_or(DEFAULT_RAMP_DEG.to_radians());
        let entry_plan = self.entry_opts.read(ctx, &p, Some(ramp))?;
        // 開いた面では、工具がストックの外から入れるよう、工具径と 1 mm だけ外まで広げる
        let floors = floors(ctx, &p, self.pick(), p.tool.diameter + 1.0)?;
        let big = match self.rest_tool {
            Some(n) => Some(ToolLibrary::tool_from(ctx, self.base.tools, n)?),
            None => self.rest_size.tool(ctx)?,
        };
        let params = AdaptivePlan {
            leave,
            leave_floor,
            load,
            min_radius,
            auto_stepdown: self.base.stepdown.is_none(),
            entry_plan,
            helix_set: self.entry_opts.helix_diameter.is_some(),
            floors,
            rest: big,
            entry: self.entry,
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
    let mut fields: Vec<Field> = floor_fields().into();
    fields.extend(leave_fields());
    fields.extend([
        Field::opt_expr("load", "最適負荷").length().help(
            "工具が材料に掛かる幅。例: 0.5 mm。空なら工具径の 15 %。小さくするほど 1 段を深くできる",
        ),
        Field::info("entry", "進入位置").help(
            "3D のビューで「進入位置を置く」から置く。最初はそこに最も近い、らせんが入る所から入る。\
             空なら、開いた面はストックの外から、閉じた面はいちばん広い所から入る",
        ),
    ]);
    fields.extend(entry_fields(ENTRY_KINDS));
    fields.extend([
        Field::opt_expr("ramp_angle", "ランプ角度")
            .angle()
            .help("例: 2 deg。空なら 2 deg"),
        last_step_field(),
        min_radius_field(),
        Field::opt_integer("rest_tool", "取残し加工: 前の工具の番号")
            .unitless()
            .help("前に同じ面を削った太い工具。その工具が届かなかった隅だけを、今の工具で削る"),
    ]);
    fields.extend(RestSize::fields());
    fields.extend(jc_cam_setup::height_fields());
    reg.register_described(
        jc_cam_setup::operation(
            ADAPTIVE,
            "負荷制御粗取り",
            "選んだ底の面の内側を、工具が材料に掛かる幅を一定に保って削る",
            fields,
        )
        .icon("adaptive")
        .retarget(jc_core::Retarget::Face { key: "face".into() }),
        |d| Ok(Box::new(jc_cam_setup::parse::<Adaptive>(d)?)),
    );
}

/// この crate の名乗り: 登録する種類と、加工を足す操作(画面の名前・まとまり・初めの値)。
pub fn module() -> jc_cam_setup::StrategyModule {
    use jc_cam_setup::{FaceMenu, Needs, Strategy, StrategyModule, to_data};
    use jc_cam_tool::ToolKind;
    StrategyModule::new(
        "jc-strategy-adaptive2d",
        register,
        vec![
            Strategy::new(ADAPTIVE, "cam.add.adaptive", "負荷制御粗取り", |p| {
                let (face, faces) = p.floor_faces()?;
                let profile = p.selected_profile().filter(|_| face.is_empty());
                to_data(Adaptive {
                    faces,
                    profile,
                    depth: profile.and_then(|id| p.sketch_depth(id)),
                    ..Adaptive::on_face(p.base(), face)
                })
            })
            .order(0)
            .icon("adaptive")
            .aliases("ふかいってい あらけずり adaptive clearing trochoidal とろこいど こうそく hsm")
            .hint("底の面(複数可)か閉じた輪郭のスケッチを選んでから押す。工具が材料に掛かる幅を一定に保ち、1 段を深くして速く削る")
            .needs(Needs::FloorOrProfile)
            .face_menu(FaceMenu::AnyFace)
            .entry("クリックした所の近くで、らせん(入り方による)で下りてから削り始める")
            .rough_grid()
            .floor_faces()
            .tool_order(None)
            .rest(&[ToolKind::FlatEnd]),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stepdown_follows_the_carvera_air_conditions() {
        use jc_cam_setup::machine_stepdown;
        let carvera = jc_post_carvera_air::profile();
        // 3.175 mm の工具でアルミ: 溝 0.2 mm、横 15 % なら断面が同じになる 1.33 mm
        let slot = machine_stepdown(&carvera, "アルミ", 3.175).unwrap();
        assert_eq!(slot, 0.2);
        let d = adaptive_stepdown(slot, 3.175, 0.15 * 3.175, 12.0);
        assert!((d - 0.2 / 0.15).abs() < 1e-9, "{d}");
        // 木材は工具径の 2 倍、刃が短ければ刃長の 8 割で止まる
        let wood = machine_stepdown(&carvera, "木材", 3.175).unwrap();
        assert_eq!(adaptive_stepdown(wood, 3.175, 0.476, 12.0), 6.35);
        assert!((adaptive_stepdown(wood, 3.175, 0.476, 5.0) - 4.0).abs() < 1e-9);
        // 径の区分と、知らない材料
        assert_eq!(machine_stepdown(&carvera, "MDF", 2.0), Some(1.0));
        assert_eq!(machine_stepdown(&carvera, "真鍮", 1.0), Some(0.05));
        assert_eq!(machine_stepdown(&carvera, "鉄", 6.0), None);
        // 別名や大文字・小文字の違いでも機械の表を使う
        assert_eq!(machine_stepdown(&carvera, "aluminum", 3.175), Some(0.2));
        assert_eq!(machine_stepdown(&carvera, "mdf", 2.0), Some(1.0));
        assert_eq!(machine_stepdown(&carvera, " アルミ ", 3.175), Some(0.2));
        // 表のない機械は工具の条件を使う
        let generic = jc_post::machines::generic_fanuc();
        assert_eq!(machine_stepdown(&generic, "アルミ", 3.175), None);
    }
}
