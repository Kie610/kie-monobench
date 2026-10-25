//! 2D ポケット加工(`cam.pocket2d@1`)。平らな底の面の上を、ストックの上面から底まで削る。
//!
//! 底の面の外周が壁になる(外周の外に部品の壁がない開いた面なら、ストックの縁まで削る)。底の面の内周のうち、内側に部品が底より高く残る所(島)は避け、
//! 低くなる所(穴や、さらに深いポケット)は避けずに通る。
//! 底の面は複数選べ、スケッチの閉じた輪郭(平面の高さか、平面から決めた深さが底)も削る。
//! 領域は 1 つずつ、上から底まで削ってから次へ移る。
//! 最小の切削半径があれば、粗削りの輪の内側の角をその半径以上に丸める(角の材料は仕上げに残す)。
//! 各段では内側の輪から外側の輪へ等間隔のオフセットをたどる(外周は反時計回り、
//! 島の周りは時計回りで、主軸が時計回りの時のダウンカット。削る向きをアップカットにすると逆に回る)。
//! 輪から輪へは、工具が壁に食い込まない時だけ下げたまま移り、それ以外は上げて移る。
//! 下げる時は、斜めに入る角度があればその輪の上を下りながら回って入り(輪が小さければらせん)、
//! なければ下向きの送りで真下に入る。
//!
//! 残り削り(`rest_tool` に前の太い工具の番号)では、その工具が届かなかった隅や細い溝だけを削る。
//! 細い工具の輪のうち、工具の円が削り残しにかかる部分だけを開いた線としてたどる。
//! 今の工具は平・V・テーパーのどれでもよい。V とテーパーは深さで太さが変わるので、段ごとに
//! ストックの上面での工具の半径 r(d)(d は上面から先端まで)だけ壁から離す。前の工具が V や
//! テーパーなら、その段で前の工具の中心が届いた所を、前の工具の先の半径だけ広げた所を削れたとみなす
//! (先より上の太い所で削れた分は数えない。削り残しを多めに見るので、空を削ることはあっても
//! 削り忘れはない)。線の入る所は前の工具が削った所(材料の外)を選ぶので、下りる時に材料へ突っ込まない。
//! 線が輪をひと回りする(入る所に材料の外がない)時は、斜めに入る角度があれば輪の上を下りながら入る。
//! 残り削りの線は往復でつなぐので、削る向きは揃えない。
//!
//! 仕上げ(壁に沿う周)は、底の深さで仕上げ代を外から決めた回数に分けて取り、望めば最後の周を
//! もう 1 周する。入りと抜けの円弧の半径があれば、壁から離れたポケットの中で下りて
//! 円弧で壁に沿い始め、1 周したら円弧で離れる。

use jc_cam_geom::{Area, Loop, P2, offset_area, seg_dist};
use jc_cam_setup::{
    CutDir, EntryKind, EntryOpts, EntryPlan, EntryRegion, FinishOpts, FinishPlan, Floor, FloorPick,
    Lap, LeadOpts, LeadShape, OpBase, Planner, RestSize, StrategyInput, StrategyOutput, approach,
    direction_field, entry_fields, expr_refs, finish_fields, floor_fields, floor_inputs, floors,
    free_on_left, helix_moves, helix_start, invalid, lap_start, last_step_field, lead_field,
    lead_shape_fields, leads, leave_fields, leaves, loop_moves, min_radius_field, opt_length,
    plan_error, prepare, ramp_angle, ramp_moves, rest_levels, rest_plan, rest_tool_or_size,
    retract, round_corners, start_after, start_mid_line, start_near, unreachable_area, wall_top,
};
use jc_cam_tool::Tool;
use jc_core::{EvalCtx, EvalError, Feature, InputRef, Outputs, Registry};
use jc_core::{FeatureId, Field};
use jc_toolpath::{Move, Role, Toolpath};
use serde::{Deserialize, Serialize};

pub const POCKET: &str = "cam.pocket2d@1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pocket {
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
    /// 斜めに入る角度(例: `3 deg`)。輪の上を下りながら回って入る。空なら真下に入る
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ramp_angle: Option<String>,
    /// 壁の仕上げ代。あれば、粗削りはこの分を壁に残し、最後に底の深さで壁に沿って仕上げる
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish: Option<String>,
    /// 残り削り: 前に同じポケットを削った太い工具の番号。その工具が届かなかった所だけを削る
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rest_tool: Option<u32>,
    /// 残り削りの前の工具を直径とコーナー半径で打つ時(番号が空の時に使う)
    #[serde(flatten)]
    pub rest_size: RestSize,
    /// 入る位置(セットアップの座標の XY)。あればそこで下りて(斜めに入る角度があれば螺旋で)から
    /// 輪の始まりへ動く。空なら輪の始まりで入る。残り削りでは使わない
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<[f64; 2]>,
    /// 削る向き(既定はダウンカット)
    #[serde(default, skip_serializing_if = "CutDir::is_climb")]
    pub direction: CutDir,
    /// 仕上げの周の入りと抜けの円弧の半径(工具の中心の円弧)。空なら壁の上で下りる
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead: Option<String>,
    /// 最小の切削半径。粗削りの輪の内側の角を、この半径以上に丸める
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_radius: Option<String>,
    /// 仕上げの周の横の間隔。空なら仕上げ代を周の数で等しく分ける
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_stepover: Option<String>,
    /// 入り方(らせん・斜め・真下・下穴)
    #[serde(flatten)]
    pub entry_opts: EntryOpts,
    /// リードの角度と直線と送り
    #[serde(flatten)]
    pub lead_opts: LeadOpts,
    /// 仕上げの周回の数・送り・重ね
    #[serde(flatten)]
    pub finish_opts: FinishOpts,
}

/// ポケットで選べる入り方。
const ENTRY_KINDS: &[EntryKind] = &[
    EntryKind::Auto,
    EntryKind::Helix,
    EntryKind::Ramp,
    EntryKind::Plunge,
    EntryKind::Predrill,
];

impl Pocket {
    /// 底の面 `face` を削るポケット(ほかの欄は初めの値)。
    pub fn on_face(base: OpBase, face: impl Into<String>) -> Pocket {
        Pocket {
            base,
            face: face.into(),
            faces: Vec::new(),
            profile: None,
            depth: None,
            leave: zero(),
            leave_floor: None,
            ramp_angle: None,
            finish: None,
            rest_tool: None,
            rest_size: Default::default(),
            entry: None,
            direction: Default::default(),
            lead: None,
            min_radius: None,
            finish_stepover: None,
            entry_opts: Default::default(),
            lead_opts: Default::default(),
            finish_opts: Default::default(),
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
                &mut self.ramp_angle,
                &mut self.finish,
                &mut self.lead,
                &mut self.depth,
                &mut self.min_radius,
                &mut self.finish_stepover,
            ]
            .into_iter()
            .flatten()
            .chain(self.entry_opts.exprs_mut())
            .chain(self.lead_opts.exprs_mut())
            .chain(self.finish_opts.exprs_mut())
            .chain(self.rest_size.exprs_mut()),
        )
    }
}

fn zero() -> String {
    "0 mm".into()
}

/// 領域を内側へ寄せていった輪の列。外側の輪から順に並ぶ。
/// 送り幅が工具の半径より大きい時、輪の内側に削り残しが出ないよう、送り幅だけ寄せると
/// 消えてしまう所(領域全体、または島と外周の間の細い所などの一部)には、半径より少し
/// 内側に輪を足す。
fn rings(area: &Area, first: f64, step: f64, r: f64) -> Vec<Area> {
    let mut out = Vec::new();
    let mut cur = offset_area(area, first);
    let mut guard = 0;
    while !cur.is_empty() && guard < 10_000 {
        let mut next = Vec::new();
        for a in &cur {
            let mut n = offset_area(a, step);
            if step > 0.95 * r {
                let gaps = fill_gaps(a, &n, r);
                if !gaps.is_empty() {
                    log::debug!("送り幅の輪から届かない所に小さな輪を {} 個足す", gaps.len());
                }
                n.extend(gaps);
            }
            next.extend(n);
        }
        out.extend(cur);
        cur = next;
        guard += 1;
    }
    out
}

/// 輪 `a`(工具の中心が回る線)と、その内側の次の輪 `next` のどちらからも工具が届かない所に
/// 足す、小さな円の輪。送り幅が半径より大きいと、島と外周の間の細い所のように、次の輪が
/// その所だけ消えて、真ん中に柱のような削り残しが出る。`a` の中を半径の 1/5 の間隔で調べ、
/// 届かない点ごとに半径の 1/5 の円を足す(円の工具は半径の 1.2 倍まで届く)。
fn fill_gaps(a: &Area, next: &[Area], r: f64) -> Vec<Area> {
    let mut paths: Vec<Vec<P2>> = Vec::new();
    for x in std::iter::once(a).chain(next) {
        paths.push(x.outer.flatten(1e-3));
        paths.extend(x.holes.iter().map(|h| h.flatten(1e-3)));
    }
    let near_path = |q: P2| {
        paths
            .iter()
            .any(|p| (0..p.len()).any(|k| seg_dist(q, p[k], p[(k + 1) % p.len()]) <= r - 0.02))
    };
    let outer = &paths[0];
    let lo = outer
        .iter()
        .fold([f64::INFINITY; 2], |m, p| [m[0].min(p[0]), m[1].min(p[1])]);
    let hi = outer.iter().fold([f64::NEG_INFINITY; 2], |m, p| {
        [m[0].max(p[0]), m[1].max(p[1])]
    });
    let h = 0.2 * r;
    let rho = 0.2 * r;
    let mut centers: Vec<P2> = Vec::new();
    let (nx, ny) = (
        ((hi[0] - lo[0]) / h) as usize,
        ((hi[1] - lo[1]) / h) as usize,
    );
    for j in 0..=ny {
        for i in 0..=nx {
            let q = [lo[0] + i as f64 * h, lo[1] + j as f64 * h];
            if !a.contains(q)
                || next.iter().any(|b| b.contains(q))
                || centers
                    .iter()
                    .any(|c| (c[0] - q[0]).hypot(c[1] - q[1]) <= 0.75 * r)
                || near_path(q)
            {
                continue;
            }
            centers.push(q);
        }
    }
    centers
        .into_iter()
        .map(|c| Area {
            outer: Loop {
                verts: vec![
                    jc_cam_geom::Vertex {
                        x: c[0] + rho,
                        y: c[1],
                        bulge: 1.0,
                    },
                    jc_cam_geom::Vertex {
                        x: c[0] - rho,
                        y: c[1],
                        bulge: 1.0,
                    },
                ],
            },
            holes: vec![],
        })
        .collect()
}

/// 線分の上の点が、工具の中心が入ってよい領域の中にあるか。
fn segment_inside(allowed: &[Area], a: P2, b: P2) -> bool {
    let n = (((b[0] - a[0]).hypot(b[1] - a[1]) / 0.2).ceil() as usize).max(1);
    (0..=n).all(|i| {
        let t = i as f64 / n as f64;
        let q = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        allowed.iter().any(|ar| ar.contains(q))
    })
}

/// 最初に下りる所と下り方。
#[derive(Debug, Clone, Copy, PartialEq)]
enum First {
    /// 中心 `e`、半径の らせん
    Helix(P2, f64),
    /// `e` で真下に
    Plunge(P2),
}

/// 高さ `from` から `to` まで、最初に下りる所で下りる(印を付ける)。
fn first_down(tp: &mut Toolpath, plan: &EntryPlan, first: First, (from, to): (f64, f64)) {
    let s = tp.moves.len();
    match first {
        First::Plunge(e) => tp.moves.push(Move::Linear {
            to: [e[0], e[1], to],
            feed: plan.plunge_feed,
        }),
        First::Helix(e, rh) => {
            tp.moves.push(Move::Linear {
                to: helix_start(e, rh, 0.0, from),
                feed: plan.ramp_feed,
            });
            tp.moves.extend(helix_moves(
                e,
                rh,
                0.0,
                (from, to),
                plan.angle,
                plan.ramp_feed,
                true,
            ));
        }
    }
    tp.mark(s, Role::Entry);
}

/// 前の段の高さ `z_prev` まで送りで下げる(削れている所。印を付ける)。下げ始める高さを返す。
fn down_to_prev(tp: &mut Toolpath, p: &Planner, at: P2, z_prev: f64) -> f64 {
    if z_prev < tp.retract_z {
        let s = tp.moves.len();
        tp.moves.push(Move::Linear {
            to: [at[0], at[1], z_prev],
            feed: p.cutting.feed,
        });
        tp.mark(s, Role::Link);
    }
    z_prev.min(tp.retract_z)
}

/// 1 段分の経路を足す。輪は内側から外側へたどる。`first` は最初に下りる所(なければ輪の上で下りる)。
/// `prefer` は最初の輪を始める所の目安。`ramp` は輪の上を斜めに下りる角度(なければ真下)。
#[allow(clippy::too_many_arguments)]
fn level(
    tp: &mut Toolpath,
    p: &Planner,
    plan: &EntryPlan,
    rings: &[Area],
    allowed: &[Area],
    (z, z_prev): (f64, f64),
    ramp: Option<f64>,
    (first, prefer): (Option<First>, Option<P2>),
) {
    let mut loops: Vec<Loop> = Vec::new();
    for a in rings.iter().rev() {
        loops.extend(a.holes.iter().cloned());
        loops.push(a.outer.clone());
    }
    let mut down = false;
    if let Some(f) = first {
        let e = match f {
            First::Helix(e, _) | First::Plunge(e) => e,
        };
        approach(tp, e);
        let from = down_to_prev(tp, p, e, z_prev);
        first_down(tp, plan, f, (from, z));
        down = true;
    }
    for l in loops {
        let cur = tp
            .moves
            .last()
            .map(|m| m.to())
            .unwrap_or([0.0, 0.0, tp.safe_z]);
        let near = match prefer {
            Some(e) if !down => e,
            _ => [cur[0], cur[1]],
        };
        let l = start_near(&l, near);
        let start = [l.verts[0].x, l.verts[0].y];
        if down && segment_inside(allowed, [cur[0], cur[1]], start) {
            tp.moves.push(Move::Linear {
                to: [start[0], start[1], z],
                feed: p.cutting.feed,
            });
        } else {
            approach(tp, start);
            // 前の段の高さまでは削れているので、そこまでは速めに下げる
            let from = down_to_prev(tp, p, start, z_prev);
            let s = tp.moves.len();
            match ramp {
                Some(a) => {
                    tp.moves
                        .extend(ramp_moves(&l, from, z, a, plan.ramp_feed, plan.plunge_feed))
                }
                None => tp.moves.push(Move::Linear {
                    to: [start[0], start[1], z],
                    feed: plan.plunge_feed,
                }),
            }
            tp.mark(s, Role::Entry);
            down = true;
        }
        tp.moves.extend(loop_moves(&l, z, p.cutting.feed));
    }
    retract(tp);
}

/// 中心 `e` の半径 `rh` の円が、工具の中心が入ってよい所に入るか。
fn helix_fits(allowed: &[Area], e: P2, rh: f64) -> bool {
    (0..24).all(|k| {
        let t = k as f64 / 24.0 * std::f64::consts::TAU;
        let q = [e[0] + rh * t.cos(), e[1] + rh * t.sin()];
        allowed.iter().any(|a| a.contains(q))
    })
}

/// らせんが入る半径(`rh` から半分ずつ 2 回まで小さくする)。
fn fitting_helix(allowed: &[Area], e: P2, rh: f64) -> Option<f64> {
    [rh, rh / 2.0, rh / 4.0]
        .into_iter()
        .find(|&r| helix_fits(allowed, e, r))
}

/// 入る位置を置いていない時のらせんの中心: いちばん内側の輪の中で、輪から最も離れた所の近く
/// (輪の折れ線の重心が中にあればそれ、なければ輪の始まり)。
fn auto_center(rings: &[Area]) -> Option<P2> {
    let a = rings.last()?;
    let pts = a.outer.flatten(1e-2);
    if pts.is_empty() {
        return None;
    }
    let n = pts.len() as f64;
    let c = [
        pts.iter().map(|q| q[0]).sum::<f64>() / n,
        pts.iter().map(|q| q[1]).sum::<f64>() / n,
    ];
    Some(if a.contains(c) { c } else { pts[0] })
}

/// 壁の仕上げの周。`offsets` は各周の壁からの距離(工具の刃の側。外の周から順、最後は壁に残す厚さ)。
struct Finishing {
    offsets: Vec<f64>,
    fin: FinishPlan,
    lead: Option<LeadShape>,
}

impl Finishing {
    /// 仕上げ代 `finish` を `passes` 周で取る。`stepover` が空なら等しく分ける。
    /// 同じ所をもう 1 周するなら、最後の周を繰り返す。
    fn offsets(
        leave: f64,
        finish: f64,
        passes: u32,
        stepover: Option<f64>,
        spring: bool,
    ) -> Vec<f64> {
        let n = passes.max(1);
        let s = stepover.unwrap_or(finish / n as f64);
        let mut v: Vec<f64> = (0..n)
            .map(|i| leave + (s * (n - 1 - i) as f64).min(finish))
            .collect();
        v.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
        if spring {
            v.push(leave);
        }
        v
    }

    /// 領域 `area` の壁(外周と島)を、底 `bottom` の高さで仕上げる。円弧が入らない壁があれば真。
    fn walls(
        &self,
        tp: &mut Toolpath,
        (p, plan): (&Planner, &EntryPlan),
        area: &Area,
        bottom: f64,
        dir: CutDir,
    ) -> bool {
        let fin = &self.fin;
        let mut lead_missing = false;
        for &d in &self.offsets {
            let inside = offset_area(area, p.r() + d - 1e-3);
            let fits = |q: P2| inside.iter().any(|a| a.contains(q));
            for a in offset_area(area, p.r() + d) {
                // 外周は内側が、島の周りは外側がポケットの中
                let walls =
                    std::iter::once((&a.outer, true)).chain(a.holes.iter().map(|h| (h, false)));
                for (l, free_inside) in walls {
                    let l = dir.apply(l.clone());
                    // 円弧で入る時は、角を避けていちばん長い直線の中ほどから回る
                    let l = if self.lead.is_some() {
                        start_mid_line(&l)
                    } else {
                        l
                    };
                    let free_left = free_on_left(&l, free_inside);
                    let shape = self.lead.map(|s| (s.radius, s.sweep, s.line));
                    let arcs = shape.and_then(|sh| {
                        let a = leads(&l, sh, free_left, fits);
                        lead_missing |= a.is_none();
                        a
                    });
                    let out_arcs = if fin.overlap > 1e-9 {
                        shape.and_then(|sh| {
                            leads(&start_after(&l, fin.overlap), sh, free_left, fits)
                        })
                    } else {
                        arcs
                    };
                    let down = lap_start(&l, arcs);
                    approach(tp, down);
                    let s = tp.moves.len();
                    tp.moves.push(Move::Linear {
                        to: [down[0], down[1], bottom],
                        feed: plan.plunge_feed,
                    });
                    tp.mark(s, Role::Entry);
                    Lap {
                        l: &l,
                        z: bottom,
                        feed: fin.feed,
                        lead_feed: self.lead.map_or(fin.feed, |s| s.feed),
                        leads: arcs,
                        out_leads: arcs.and(out_arcs),
                        overlap: fin.overlap,
                    }
                    .push(tp);
                    retract(tp);
                }
            }
        }
        lead_missing
    }
}

/// 領域ごとの、最初に下りる所と最初の輪を始める所の目安。`entry` はこの領域に入る、置いた
/// 入る位置。`first_area` なら、下穴の位置がない知らせを出す(知らせは 1 度だけ)。
fn first_of(
    p: &mut Planner,
    (plan, ramp): (&EntryPlan, Option<f64>),
    entry: Option<P2>,
    (allowed, rs): (&[Area], &[Area]),
    first_area: bool,
) -> (Option<First>, Option<P2>) {
    let helix_at = |p: &mut Planner, e: P2, angle_set: bool| -> Option<First> {
        match fitting_helix(allowed, e, plan.helix_r) {
            Some(rh) => {
                if rh < plan.helix_r - 1e-9 {
                    p.warnings.push(format!(
                        "入る位置のまわりが狭いので、らせんの直径を {:.2} mm に小さくしました",
                        2.0 * rh
                    ));
                }
                Some(First::Helix(e, rh))
            }
            None if angle_set => {
                p.warnings
                    .push("入る位置のまわりが狭く螺旋が入らないので、真下に下ります".into());
                Some(First::Plunge(e))
            }
            None => None,
        }
    };
    match plan.kind {
        EntryKind::Auto | EntryKind::Outside => match entry {
            Some(e) if ramp.is_some() => (helix_at(p, e, true), None),
            Some(e) => (Some(First::Plunge(e)), None),
            None => (None, None),
        },
        EntryKind::Helix => {
            let e = entry.or_else(|| auto_center(rs));
            let f = e.and_then(|e| helix_at(p, e, false));
            if f.is_none() {
                p.warnings.push(
                    "らせんが入る広さがないので、輪の上を斜めに下ります(らせんの直径を小さくしてください)"
                        .into(),
                );
            }
            (f, None)
        }
        EntryKind::Ramp => (None, entry),
        EntryKind::Plunge => (entry.map(First::Plunge), None),
        EntryKind::Predrill => match entry {
            Some(e) => (Some(First::Plunge(e)), None),
            None => {
                if first_area {
                    p.warnings.push(
                        "下穴から入るには、入る位置(下穴の中心)を置いてください。今は輪の始まりで真下に下ります"
                            .into(),
                    );
                }
                (None, None)
            }
        },
    }
}

/// ホストが解いたポケットの値(`plan` の入力の `params`)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PocketPlan {
    pub leave: f64,
    pub leave_floor: f64,
    pub ramp: Option<f64>,
    pub finish: f64,
    pub entry_plan: EntryPlan,
    pub lead: Option<LeadShape>,
    pub fin: FinishPlan,
    pub min_radius: f64,
    pub finish_stepover: Option<f64>,
    /// 削る領域(選んだ底の面とスケッチの輪郭)
    pub floors: Vec<Floor>,
    /// 残り削りの前の工具
    pub rest: Option<Tool>,
    pub entry: Option<[f64; 2]>,
    pub direction: CutDir,
    /// 壁の仕上げの欄(回数・横の間隔・送り・もう 1 周・重ね)のどれかを決めている
    pub finish_fields_set: bool,
}

/// 戦略の純関数: 解いた入力だけから経路を作る(文書とカーネルに触らない)。
pub fn plan(input: &StrategyInput) -> Result<StrategyOutput, String> {
    run(input).map_err(plan_error)
}

fn run(input: &StrategyInput) -> Result<StrategyOutput, EvalError> {
    let mut p = Planner::new(input);
    let f: PocketPlan = input.params().map_err(invalid)?;
    let PocketPlan {
        leave,
        leave_floor,
        ramp,
        finish,
        entry_plan: plan,
        lead,
        fin,
        min_radius,
        finish_stepover,
        floors,
        rest: rest_big,
        entry: entry_at,
        direction,
        finish_fields_set,
    } = f;
    let passes = fin.passes.max(1);
    let top = p.top(p.info.stock_max[2]);
    for f in &floors {
        let bottom = p.bottom(f.z + leave_floor);
        if bottom >= top {
            return Err(invalid("削る深さがありません"));
        }
        p.check_depth(top, bottom)?;
    }
    // スケッチの輪郭は部品の形にない所を削るので、平面からの深さまでの食い込みは許す
    let edge_break = floors
        .iter()
        .filter_map(|f| f.sketch_top.map(|t| t - f.z + 0.05))
        .fold(0.0, f64::max);
    if let Some(big) = rest_big {
        let mut tp = p.toolpath();
        tp.edge_break = edge_break;
        let mut any = false;
        for f in &floors {
            let bottom = p.bottom(f.z + leave_floor);
            // 深さごとの太さは、壁の上端から測る(壁がなければストックの上面)。壁の上端より上には
            // 隅がないので、段も壁の上端から始める
            let wall = wall_top(&p.model, &f.area).map_or(top, |w| w.min(top));
            let levels = rest_plan(
                &f.area,
                leave,
                &big,
                &p.tool,
                p.cutting.stepover,
                wall,
                &p.levels(wall.max(bottom + 1e-6), bottom),
                true,
            );
            if levels.iter().all(|l| l.1.is_empty()) {
                continue;
            }
            any = true;
            // 深い段で今の工具が太くなり、もう届かない所は削れない
            if p.tool.is_tapered()
                && let Some(deep) = levels.iter().find(|l| l.1.is_empty())
            {
                p.warnings.push(format!(
                    "工具 {} は深さ {:.2} mm から先が太くなり、前の工具の削り残しに届きません\
                     (そこから下は削りません)",
                    p.tool.name,
                    wall - deep.0
                ));
            }
            let levels: Vec<_> = levels.into_iter().filter(|l| !l.1.is_empty()).collect();
            rest_levels(&mut tp, &p, &levels, ramp);
        }
        if !any {
            return Err(invalid(format!(
                "工具 {} の削り残しはありません(残り削りは要りません)",
                big.name
            )));
        }
        return Ok(p.output(tp));
    }
    let dir = direction;
    let r = p.r() + leave + finish;
    let mut tp = p.toolpath();
    tp.edge_break = edge_break;
    let mut rest = 0.0;
    let mut entry_used = entry_at.is_none();
    let finishing = Finishing {
        offsets: Finishing::offsets(leave, finish, passes, finish_stepover, fin.spring),
        fin,
        lead,
    };
    // 輪の上で下りる角度(なければ真下)
    let ring_ramp = match plan.kind {
        EntryKind::Plunge | EntryKind::Predrill => None,
        EntryKind::Helix | EntryKind::Ramp => Some(plan.angle),
        EntryKind::Auto | EntryKind::Outside => ramp,
    };
    // 入る位置を置く手順で塗る所: らせんなら、らせんが縮まずに入る所
    let helix = match plan.kind {
        EntryKind::Helix => true,
        EntryKind::Auto | EntryKind::Outside => ramp.is_some(),
        _ => false,
    };
    let mut region: Vec<Area> = Vec::new();
    let mut lead_missing = false;
    for (k, f) in floors.iter().enumerate() {
        let bottom = p.bottom(f.z + leave_floor);
        let allowed = offset_area(&f.area, r - 1e-3);
        let rs = rings(&f.area, r, p.tool.diameter * p.cutting.stepover, p.r());
        if rs.is_empty() {
            return Err(invalid("工具がポケットに入りません(工具が太すぎます)"));
        }
        let rs: Vec<Area> = round_corners(rs, min_radius)
            .into_iter()
            .map(|a| Area {
                outer: dir.apply(a.outer),
                holes: a.holes.into_iter().map(|h| dir.apply(h)).collect(),
            })
            .collect();
        rest += unreachable_area(&f.area, p.r() + min_radius, leave);
        // 置いた入る位置は、工具の中心が入れる領域でだけ使う
        let entry = match entry_at {
            Some(e) if allowed.iter().any(|a| a.contains(e)) => {
                entry_used = true;
                Some(e)
            }
            _ => None,
        };
        let (first, prefer) = first_of(&mut p, (&plan, ramp), entry, (&allowed, &rs), k == 0);
        let mut prev = top;
        for z in p.levels(top, bottom) {
            level(
                &mut tp,
                &p,
                &plan,
                &rs,
                &allowed,
                (z, prev),
                ring_ramp,
                (first, prefer),
            );
            prev = z;
        }
        // 仕上げ: 底の深さで、壁(外周と島)に沿って
        if finish > 1e-9 {
            lead_missing |= finishing.walls(&mut tp, (&p, &plan), &f.area, bottom, dir);
        }
        if helix {
            region.extend(offset_area(&f.area, r + plan.helix_r));
        } else {
            region.extend(allowed);
        }
    }
    if let Some(e) = entry_at
        && !entry_used
    {
        p.warnings.push(format!(
            "入る位置({:.1}, {:.1})は工具の中心が入れる所の外なので、輪の始まりから入ります",
            e[0], e[1]
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
    if finish > 1e-9 {
        if lead_missing {
            p.warnings.push(
                "仕上げの入りと抜けの円弧が入らない壁があるので、そこは壁の上で下ります\
                 (円弧の半径を小さくしてください)"
                    .into(),
            );
        }
        if let Some(s) = finish_stepover
            && s * (passes.max(1) - 1) as f64 > finish + 1e-9
        {
            p.warnings.push(
                "仕上げの横の間隔 × (回数 − 1) が仕上げ代より大きいので、外側の周は仕上げ代の所で回ります"
                    .into(),
            );
        }
    } else {
        if lead.is_some() {
            p.warnings.push(
                "入りと抜けの円弧は仕上げの周で使います(仕上げ代がないので使いません)".into(),
            );
        }
        if finish_fields_set {
            p.warnings.push(
                "壁の仕上げの欄(回数・横の間隔・送り・もう 1 周・重ね)は、壁の仕上げ代がある時に使います"
                    .into(),
            );
        }
    }
    let mut out = p.output(tp);
    out.entry_area = Some(EntryRegion {
        z: top,
        areas: region,
        helix,
    });
    Ok(out)
}

impl Feature for Pocket {
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
        let ramp = ramp_angle(ctx, &self.ramp_angle)?;
        let finish = opt_length(ctx, &self.finish, "仕上げ代")?.unwrap_or(0.0);
        let entry_plan = self.entry_opts.read(ctx, &p, ramp)?;
        let lead = self.lead_opts.read(ctx, &p, &self.lead)?;
        let fin = self.finish_opts.read(ctx, &p)?;
        let min_radius = opt_length(ctx, &self.min_radius, "最小切削半径")?.unwrap_or(0.0);
        let finish_stepover = opt_length(ctx, &self.finish_stepover, "仕上げピッチ")?;
        if finish_stepover.is_some_and(|s| s <= 0.0) {
            return Err(invalid("仕上げの横の間隔は 0 より大きくしてください"));
        }
        let floors = floors(ctx, &p, self.pick(), p.r())?;
        let rest = rest_tool_or_size(ctx, &p, self.base.tools, self.rest_tool, &self.rest_size)?;
        let o = &self.finish_opts;
        let params = PocketPlan {
            leave,
            leave_floor,
            ramp,
            finish,
            entry_plan,
            lead,
            fin,
            min_radius,
            finish_stepover,
            floors,
            rest,
            entry: self.entry,
            direction: self.direction,
            finish_fields_set: o.finish_passes > 1
                || self.finish_stepover.is_some()
                || o.finish_feed.is_some()
                || o.spring_pass
                || o.overlap.is_some(),
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
    fields.push(Field::info("entry", "進入位置").help(
        "3D のビューで「進入位置を置く」から置く(面の上の好きな所をクリックするか、座標を打つ)。\
         空なら自動で決める",
    ));
    fields.extend(entry_fields(ENTRY_KINDS));
    fields.extend([
        Field::opt_expr("ramp_angle", "ランプ角度").angle().help(
            "例: 3 deg。らせんと斜めで下りる傾き。入り方が自動の時は、空なら真下に入る\
             (らせんと斜めでは空なら 2 deg)",
        ),
        last_step_field(),
        min_radius_field(),
        Field::opt_expr("finish", "壁の仕上げ代")
            .length()
            .help("例: 0.2 mm。粗削りでこの分を壁に残し、最後に底の深さで壁に沿って仕上げる"),
    ]);
    fields.extend(finish_fields());
    fields.extend([
        Field::opt_expr("finish_stepover", "仕上げピッチ").length()
            .help("仕上げの周の間隔(例: 0.1 mm)。空なら仕上げ代を周回の数で等しく分ける"),
        Field::opt_integer("rest_tool", "取残し加工: 前の工具の番号").unitless().help(
            "前に同じポケットを削った太い工具。その工具が届かなかった隅や細い溝だけを、今の工具で削る。\
             今の工具は平・ブルノーズ・V・テーパー(V とテーパーは深さごとの太さで壁から離す)",
        ),
    ]);
    fields.extend(RestSize::fields());
    fields.extend([
        direction_field(),
        lead_field().help(
            "例: 2 mm。仕上げの周回で、壁から離れた所で下りて円弧で壁に沿い始め、円弧で離れる\
             (壁に下りた跡が残らない)。仕上げ代がなければ使わない",
        ),
    ]);
    fields.extend(lead_shape_fields());
    fields.extend(jc_cam_setup::height_fields());
    reg.register_described(
        jc_cam_setup::operation(POCKET, "ポケット", "選んだ底の面の内側を削る", fields)
            .icon("pocket")
            .retarget(jc_core::Retarget::Face { key: "face".into() }),
        |d| Ok(Box::new(jc_cam_setup::parse::<Pocket>(d)?)),
    );
}

/// この crate の名乗り: 登録する種類と、加工を足す操作(画面の名前・まとまり・初めの値)。
pub fn module() -> jc_cam_setup::StrategyModule {
    use jc_cam_setup::{FaceMenu, Needs, Strategy, StrategyModule, to_data};
    use jc_cam_tool::ToolKind;
    StrategyModule::new(
        "jc-strategy-pocket2d",
        register,
        vec![
            Strategy::new(POCKET, "cam.add.pocket", "ポケット", |p| {
                let (face, faces) = p.floor_faces()?;
                let profile = p.selected_profile().filter(|_| face.is_empty());
                to_data(Pocket {
                    faces,
                    profile,
                    depth: profile.and_then(|id| p.sketch_depth(id)),
                    ramp_angle: Some("3 deg".into()),
                    ..Pocket::on_face(p.base(), face)
                })
            })
            .order(1)
            .icon("pocket")
            .aliases("ぽけっと pocket")
            .hint("3D のビューで底の面を選んでから押す(Shift・Ctrl+クリックで複数の面をまとめる)。タイムラインで閉じた輪郭のスケッチを選んでもよい")
            .needs(Needs::FloorOrProfile)
            .face_menu(FaceMenu::AnyFace)
            .entry("クリックした所で下りてから削り始める。入り方が自動かつ斜めに入る角度があるか、らせんなら、そこを中心にらせんで下りる")
            .rough_grid()
            .floor_faces()
            .tool_order(Some(adopt))
            // 同じ種類の操作で続けるので、V・テーパーも使える
            .rest(&[ToolKind::FlatEnd, ToolKind::VBit, ToolKind::TaperEnd]),
        ],
    )
}

/// 同じ領域を削る別の加工(負荷制御粗取りなど)のデータを、ポケットのデータにする。引き継ぐのは
/// 領域(底の面・スケッチ・深さ)と残し量・斜めに入る角度・角の最小の半径で、ほかは初めの値。
/// 工具の順番で、元の種類が使えない工具(V・テーパー)を足す時に使う。
fn adopt(d: &serde_json::Value) -> Result<serde_json::Value, String> {
    #[derive(Deserialize)]
    struct Same {
        #[serde(flatten)]
        base: OpBase,
        #[serde(default)]
        face: String,
        #[serde(default)]
        faces: Vec<String>,
        #[serde(default)]
        profile: Option<FeatureId>,
        #[serde(default)]
        depth: Option<String>,
        #[serde(default = "zero")]
        leave: String,
        #[serde(default)]
        leave_floor: Option<String>,
        #[serde(default)]
        ramp_angle: Option<String>,
        #[serde(default)]
        min_radius: Option<String>,
    }
    let a: Same = serde_json::from_value(d.clone()).map_err(|e| e.to_string())?;
    serde_json::to_value(Pocket {
        faces: a.faces,
        profile: a.profile,
        depth: a.depth,
        leave: a.leave,
        leave_floor: a.leave_floor,
        ramp_angle: a.ramp_angle,
        min_radius: a.min_radius,
        ..Pocket::on_face(a.base, a.face)
    })
    .map_err(|e| e.to_string())
}
