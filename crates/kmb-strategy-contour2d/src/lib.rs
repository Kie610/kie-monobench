//! 2D 輪郭加工(`cam.contour2d@1`)。輪郭に沿って、上から底まで段ごとに削る。
//!
//! 削る輪郭は「削る輪郭」(`chains`)で選ぶ。平らな面の外周と穴の縁、スケッチの閉じた線と
//! 開いた線を、いくつでも並べられる。選んでいなければ、足す時に選んだ面(`face`)の外周、
//! それもなければ部品の外形(いちばん大きい下向きの面の外周)を使う。
//! 外側を削る時は時計回り、内側を削る時は反時計回りに回る(主軸が時計回りの時のダウンカット)。
//! 削る向きをアップカットにすると逆に回る。穴の縁を先に、外周を後に削る(切り抜いた部品が動かないように)。
//!
//! 入り方は「進入方法」で選ぶ(自動・らせん・斜め・真下・下穴・ストックの外から)。入る位置を置くと、
//! 輪郭のそこに最も近い所から入る。入りと抜けの円弧の半径があれば、削り落とす側で下りて、
//! 円弧で壁に沿い始め、1 周したら円弧で離れる。
//! 横の荒取りの数を 2 以上にすると、壁から離れた所から壁へ、何周かに分けて削る。
//! 仕上げ代があれば、粗削りはこの分を壁に残し、最後に底の深さで仕上げる(周回の数・送り・
//! もう 1 周・重ねを選べる)。
//! 切り抜く時は、タブ(部品を残しておく橋。四角か三角)を等間隔か、手で置いた所に置ける。タブの上では底の面から
//! タブの高さまでを削らずに残す。タブを作らない時は、両面テープなどで材料を固定する。
//!
//! 残り削り(`rest_tool` に前の太い工具の番号)では、その工具が届かなかった内側の隅や細い溝だけを、
//! 閉じた輪郭ごとに削る(2D ポケットの残り削りと同じ見積もり。[`kmb_cam_setup::rest_plan`])。
//! 削り残しは壁から前の工具の半径ほどの所までしかないので、今の工具の輪を壁から外へ何周か作り、
//! 工具の円が削り残しにかかる部分だけをたどる。開いた線と「線の上」は削らない。
//! 残り削りでは、タブ・仕上げ・横の荒取り・入りと抜けの円弧を使わない。

use kmb_cam_geom::{Area, Loop, P2, offset_loop_outward};
use kmb_cam_setup::{
    CutDir, EntryKind, EntryOpts, EntryPlan, FinishOpts, FinishPlan, Lap, LeadOpts, LeadShape,
    Leads, OpBase, Planner, Prepared, RestSize, StrategyInput, StrategyOutput, approach,
    direction_field, entry_fields, expr_refs, finish_fields, finish_offsets, free_on_left,
    helix_moves, helix_start, invalid, lap_start, lead_field, lead_shape_fields, leads, length,
    lift_over_tabs_shaped, loop_moves, output, plan_error, prepare, ramp_angle, ramp_moves,
    rest_levels, rest_plan, rest_tool_or_size, retract, sketch_chains, start_after, start_at,
    start_mid_line, tab_centers, zigzag_moves,
};
/// タブの付く経路の出力(加工の操作が読むので kmb-cam-setup に置く)。
pub use kmb_cam_setup::{TAB_PATHS, TabPath, TabPaths};

mod cut;

use cut::{Cut, Notes};
use kmb_cam_tool::Tool;
use kmb_core::{EvalCtx, EvalError, Feature, FeatureId, Field, InputRef, Outputs, Registry};
use kmb_toolpath::{Move, Role, Toolpath};
use serde::{Deserialize, Serialize};
use std::f64::consts::TAU;

pub const CONTOUR: &str = "cam.contour2d@1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    /// 輪郭の外(面の縁なら面の外。穴の縁なら穴の中)
    #[default]
    Outside,
    /// 輪郭の内(面の縁なら面の上)
    Inside,
    /// 線の上(工具の中心が線をなぞる)
    On,
}

/// タブの形。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabShape {
    #[default]
    Rect,
    /// 三角(中心で高さいっぱい、縁で底まで下がる)。外す時に折りやすい
    Triangle,
}

impl TabShape {
    fn is_rect(&self) -> bool {
        *self == TabShape::Rect
    }
}

/// 削る輪郭の 1 つ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ChainRef {
    /// 平らな面の縁。`ring` は 0 が外周、1 からが穴の縁。空なら外周と穴の縁の全部
    Face {
        face: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ring: Option<usize>,
    },
    /// スケッチの線のつながり。`chain` は何番目のつながりか。空なら全部
    Sketch {
        sketch: FeatureId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        chain: Option<usize>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Contour {
    #[serde(flatten)]
    pub base: OpBase,
    /// 外周を使う平らな面の名前。空なら部品の外形(`chains` があれば使わない)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<String>,
    /// 削る輪郭(面の縁・スケッチの線)。空なら `face` か部品の外形
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chains: Vec<ChainRef>,
    #[serde(default)]
    pub side: Side,
    /// 底よりさらに下げる深さ
    #[serde(default = "zero")]
    pub extra_depth: String,
    /// スケッチの線の平面から測る深さ。空なら部品の底まで
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<String>,
    /// 壁に残す厚さ
    #[serde(default = "zero")]
    pub leave: String,
    /// 斜めに入る角度(例: `3 deg`)。輪郭の上を下りながら回って入る。空なら真下に入る
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ramp_angle: Option<String>,
    /// 入る位置(作業座標系の XY)。輪郭のここに最も近い所から入る。空なら自動
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<[f64; 2]>,
    #[serde(flatten)]
    pub entry_opts: EntryOpts,
    /// 横の荒取りの数。2 以上なら、壁から離れた所から壁へ何周かに分けて削る
    #[serde(default, skip_serializing_if = "is_le_one")]
    pub rough_passes: u32,
    /// 横の荒取りの間隔。空なら工具径の半分
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rough_step: Option<String>,
    /// タブ(切り抜く部品を残しておく橋)の数。0 なら作らない。輪郭に等間隔に置く
    #[serde(default, skip_serializing_if = "is_zero")]
    pub tabs: u32,
    /// タブの幅(部品の側で測る)
    #[serde(default = "four_mm")]
    pub tab_width: String,
    /// タブの高さ(底の面から)
    #[serde(default = "one_half_mm")]
    pub tab_height: String,
    #[serde(default, skip_serializing_if = "TabShape::is_rect")]
    pub tab_shape: TabShape,
    /// 手で置いたタブの位置(作業座標系の XY。工具の中心の経路の上)。あれば数の欄より先に使う
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tab_points: Vec<[f64; 2]>,
    /// 仕上げ代。あれば、粗削りはこの分を壁に残し、最後に底の深さで仕上げる
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish: Option<String>,
    #[serde(flatten)]
    pub finish_opts: FinishOpts,
    /// 削る向き(既定はダウンカット)
    #[serde(default, skip_serializing_if = "CutDir::is_climb")]
    pub direction: CutDir,
    /// 入りと抜けの円弧の半径(工具の中心の円弧)。空なら壁の上で下りる
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead: Option<String>,
    #[serde(flatten)]
    pub lead_opts: LeadOpts,
    /// 残り削り: 前に同じ輪郭を削った太い工具の番号。その工具が届かなかった隅だけを削る
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rest_tool: Option<u32>,
    /// 残り削りの前の工具を直径とコーナー半径で打つ時(番号が空の時に使う)
    #[serde(flatten)]
    pub rest_size: RestSize,
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}

fn is_le_one(v: &u32) -> bool {
    *v <= 1
}

fn four_mm() -> String {
    "4 mm".into()
}

fn one_half_mm() -> String {
    "1.5 mm".into()
}

fn zero() -> String {
    "0 mm".into()
}

/// 輪郭で選べる入り方。
const ENTRY_KINDS: [EntryKind; 6] = [
    EntryKind::Auto,
    EntryKind::Ramp,
    EntryKind::Helix,
    EntryKind::Plunge,
    EntryKind::Predrill,
    EntryKind::Outside,
];

/// 削る形の 1 つ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum Shape {
    /// 閉じた輪(反時計回り)。`tool_out` なら工具は輪の外
    Closed { l: Loop, tool_out: bool },
    /// 開いた線(工具の中心がなぞる)
    Open(Vec<P2>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Target {
    shape: Shape,
    /// 線の上をなぞるか(閉じた輪で、削る側が「線の上」)
    on: bool,
    /// 底(さらに下げる深さを足す前)
    floor: f64,
    /// 底へさらに下げる深さを足すか(部品の底まで切り抜く時。部品の面が底なら足さない)
    extra: bool,
}

/// 下り方。
#[derive(Debug, Clone, Copy)]
enum Down {
    /// 輪郭の上を下りながら回る
    Ramp,
    /// 真下(送り)
    Straight(f64),
    /// らせん(中心、半径、始まりの角)
    Helix { c: P2, r: f64, a0: f64 },
}

/// 入り方の決まった形。`d` で下りて、`t`(輪郭の始まりか、入りの円弧の始まり)へ進む。
#[derive(Debug, Clone, Copy)]
struct Way {
    d: P2,
    t: P2,
    down: Down,
}

fn dist(a: P2, b: P2) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// 輪郭の始まりでの、入ってよい側を向く法線(長さ 1)。
fn free_normal(l: &Loop, free_left: bool) -> Option<P2> {
    let s = l.segments();
    let t = match *s.first()? {
        kmb_cam_geom::Seg::Line { a, b } => [b[0] - a[0], b[1] - a[1]],
        kmb_cam_geom::Seg::Arc { a, center, ccw, .. } => {
            let u = [a[0] - center[0], a[1] - center[1]];
            if ccw { [-u[1], u[0]] } else { [u[1], -u[0]] }
        }
    };
    let n = t[0].hypot(t[1]);
    if n < 1e-12 {
        return None;
    }
    let t = [t[0] / n, t[1] / n];
    Some(if free_left {
        [-t[1], t[0]]
    } else {
        [t[1], -t[0]]
    })
}

/// 中心 `c`、半径 `r` の円(の上と中)がすべて入ってよい側にあるか。
fn circle_fits(c: P2, r: f64, fits: &impl Fn(P2) -> bool) -> bool {
    fits(c)
        && (0..16).all(|k| {
            let a = TAU * k as f64 / 16.0;
            // 輪郭に接する所は境目になるので、少しだけ内へ寄せて調べる
            let q = [c[0] + 0.98 * r * a.cos(), c[1] + 0.98 * r * a.sin()];
            fits(q)
        })
}

/// 線分の上がすべて入ってよい側にあるか。
fn segment_fits(a: P2, b: P2, fits: &impl Fn(P2) -> bool) -> bool {
    let n = ((dist(a, b) / 0.5).ceil() as usize).clamp(1, 400);
    (1..n).all(|k| {
        let t = k as f64 / n as f64;
        fits([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t])
    })
}

/// 点 `p` から向き `n` に進んで、箱 `(min, max)` の外へ出るまでの距離。
fn exit_distance(p: P2, n: P2, min: P2, max: P2) -> f64 {
    let mut t = f64::INFINITY;
    for i in 0..2 {
        if n[i] > 1e-9 {
            t = t.min((max[i] - p[i]) / n[i]);
        } else if n[i] < -1e-9 {
            t = t.min((min[i] - p[i]) / n[i]);
        }
    }
    t.max(0.0)
}

impl Contour {
    /// タブを作るか(数か、手で置いた位置がある)。
    fn has_tabs(&self) -> bool {
        self.tabs > 0 || !self.tab_points.is_empty()
    }

    /// 削る形の列。穴の縁(工具が輪の内)を先に、外周を後に並べる。
    fn targets(
        &self,
        ctx: &EvalCtx<'_>,
        p: &Prepared,
        skipped: &mut Vec<String>,
        nested_outlines: Option<Vec<(Loop, f64)>>,
    ) -> Result<Vec<Target>, EvalError> {
        let on = self.side == Side::On;
        let closed = |l: &Loop, tool_out: bool| {
            let l = if l.signed_area() < 0.0 {
                l.reversed()
            } else {
                l.clone()
            };
            Shape::Closed { l, tool_out }
        };
        let mut out = Vec::new();
        if self.chains.is_empty() {
            let outlines = match &self.face {
                Some(name) => {
                    let faces = p.model.flats_named(name);
                    if faces.is_empty() {
                        return Err(invalid(format!(
                            "面 {name} が見つかりません(参照が切れたか、水平な平面ではありません)"
                        )));
                    }
                    faces
                        .into_iter()
                        .map(|f| (f.area.outer.clone(), f.z))
                        .collect::<Vec<_>>()
                }
                None => match nested_outlines {
                    Some(outlines) => outlines,
                    None => vec![(
                        p.model.outline().ok_or_else(|| {
                            invalid("部品の外形(下向きの平らな面)が見つかりません")
                        })?,
                        p.model.min[2],
                    )],
                },
            };
            for (outline, floor) in outlines {
                out.push(Target {
                    shape: closed(&outline, self.side != Side::Inside),
                    on,
                    floor,
                    extra: true,
                });
            }
            return Ok(out);
        }
        let depth = match &self.depth {
            Some(d) => Some(length(ctx, d, "線からの深さ")?),
            None => None,
        };
        if depth.is_some_and(|d| d <= 0.0) {
            return Err(invalid("線からの深さは 0 より大きくしてください"));
        }
        for c in &self.chains {
            match c {
                ChainRef::Face { face, ring } => {
                    let faces = p.model.flats_named(face);
                    if faces.is_empty() {
                        return Err(invalid(format!(
                            "面 {face} が見つかりません(参照が切れたか、水平な平面ではありません)"
                        )));
                    }
                    for f in faces {
                        let rings: Vec<usize> = match ring {
                            Some(k) => vec![*k],
                            None => (0..=f.area.holes.len()).collect(),
                        };
                        for k in rings {
                            let l = if k == 0 {
                                &f.area.outer
                            } else {
                                f.area.holes.get(k - 1).ok_or_else(|| {
                                    invalid(format!("面 {face} に {k} 番目の穴の縁がありません"))
                                })?
                            };
                            // 「外側」は面から離れる側(外周なら輪の外、穴なら穴の中)
                            let away_out = k == 0;
                            let tool_out = match self.side {
                                Side::Inside => !away_out,
                                _ => away_out,
                            };
                            let floor = if self.side == Side::Inside {
                                f.z
                            } else {
                                match floor_beyond(p, l, away_out, f.z) {
                                    Some(z) => z,
                                    None => {
                                        // 一つだけ選んだ縁なら知らせる(全部の時はボスの根元などを黙って除く)
                                        if ring.is_some() {
                                            skipped.push(format!(
                                            "面 {face} の {} は、外側が面より高い(ボスや壁の根元)ので削りません\
                                             (削る側を「内側」にすると、面の上を縁に沿って削ります)",
                                            if k == 0 { "外周".to_string() } else { format!("{k} 番目の穴の縁") }
                                        ));
                                        }
                                        continue;
                                    }
                                }
                            };
                            out.push(Target {
                                shape: closed(l, tool_out),
                                on,
                                floor,
                                extra: floor <= p.model.min[2] + 1e-6,
                            });
                        }
                    }
                }
                ChainRef::Sketch { sketch, chain } => {
                    let (z, all) = sketch_chains(ctx, p, *sketch)?;
                    let picked: Vec<_> = match chain {
                        Some(k) => vec![all.get(*k).cloned().ok_or_else(|| {
                            invalid(format!(
                                "スケッチに {} 番目の線のつながりがありません",
                                k + 1
                            ))
                        })?],
                        None => all,
                    };
                    let floor = depth.map_or(p.model.min[2], |d| z - d);
                    for ch in picked {
                        let shape = match ch.to_loop() {
                            Some(l) => closed(&l, self.side != Side::Inside),
                            None => Shape::Open(ch.pts.clone()),
                        };
                        out.push(Target {
                            shape,
                            on,
                            floor,
                            extra: depth.is_none(),
                        });
                    }
                }
            }
        }
        // 工具が輪の内にあるもの(穴の縁)を先に。切り抜いた部品が動く前に穴を削る
        out.sort_by_key(|t| matches!(t.shape, Shape::Closed { tool_out: true, .. }));
        Ok(out)
    }
}

impl Contour {
    /// 残り削り: 前の工具 `n` が届かなかった隅だけを、閉じた輪郭ごとに今の工具で削る。
    /// 3 つ組は (ストックの上面、さらに下げる深さ、壁の残し量)。
    fn rest(
        &self,
        mut p: Planner,
        big: &Tool,
        targets: &[Target],
        (top, extra, leave): (f64, f64, f64),
        ramp: Option<f64>,
    ) -> Result<StrategyOutput, EvalError> {
        let mut tp = p.toolpath();
        let (mut any, mut lines, mut no_depth, mut too_thick) = (false, false, false, None);
        for t in targets {
            let (l, tool_out) = match &t.shape {
                Shape::Closed { l, tool_out } if !t.on => (l, *tool_out),
                _ => {
                    lines = true;
                    continue;
                }
            };
            let bottom = p.bottom(if t.extra { t.floor - extra } else { t.floor });
            if bottom >= top - 1e-9 {
                no_depth = true;
                continue;
            }
            p.check_depth(top, bottom)?;
            // 工具の中心が入ってよい側。外側なら、輪郭を穴にした大きな箱(箱の縁は使わない)
            let area = if tool_out {
                let m = 4.0 * (big.radius() + p.tool.radius()) + leave + 10.0;
                let (lo, hi) = l.bounds();
                let (a, b) = ([lo[0] - m, lo[1] - m], [hi[0] + m, hi[1] + m]);
                Area {
                    outer: Loop::from_points(&[a, [b[0], a[1]], b, [a[0], b[1]]]),
                    holes: vec![l.clone()],
                }
            } else {
                Area {
                    outer: l.clone(),
                    holes: vec![],
                }
            }
            .normalized();
            let levels = rest_plan(
                &area,
                leave,
                big,
                &p.tool,
                p.cutting.stepover,
                top,
                &p.levels(top, bottom),
                !tool_out,
            );
            if levels.iter().all(|l| l.1.is_empty()) {
                continue;
            }
            any = true;
            // 深い段で今の工具が太くなり、もう届かない所は削れない
            if p.tool.is_tapered()
                && let Some(deep) = levels.iter().find(|l| l.1.is_empty())
            {
                too_thick = Some(too_thick.map_or(top - deep.0, |d: f64| d.min(top - deep.0)));
            }
            let levels: Vec<_> = levels.into_iter().filter(|l| !l.1.is_empty()).collect();
            rest_levels(&mut tp, &p, &levels, ramp);
        }
        if !any {
            return Err(invalid(if no_depth && !lines {
                "削る深さがありません".to_string()
            } else {
                format!(
                    "工具 {} の削り残しはありません(残り削りは要りません)",
                    big.name
                )
            }));
        }
        if let Some(d) = too_thick {
            p.warnings.push(format!(
                "工具 {} は深さ {d:.2} mm から先が太くなり、前の工具の削り残しに届きません\
                 (そこから下は削りません)",
                p.tool.name
            ));
        }
        if lines {
            p.warnings.push(
                "残り削りは閉じた輪郭の外側か内側だけを削ります(開いた線と線の上は削りません)"
                    .into(),
            );
        }
        if no_depth {
            p.warnings
                .push("削る深さがない輪郭があるので、そこは削りません".into());
        }
        if self.has_tabs() {
            p.warnings.push(
                "残り削りはタブの上で持ち上げません(タブのそばに隅があると、そこも底まで削ります)"
                    .into(),
            );
        }
        if self.finish.is_some() || self.rough_passes > 1 || self.lead.is_some() {
            p.warnings
                .push("残り削りでは、仕上げ代・横の荒取り・入りと抜けの円弧を使いません".into());
        }
        Ok(p.output(tp))
    }
}

/// 面の縁 `l` の、面から離れる側(`away_out` なら輪の外)のすぐ先の高さで決める底。
/// 何もなければ(抜けている)部品の底、面より低ければその高さ、同じならその面の高さ。
/// 面より高ければ(ボスの根元や壁の縁)、そちらは削れないので `None`。
fn floor_beyond(p: &Prepared, l: &Loop, away_out: bool, face_z: f64) -> Option<f64> {
    let probe = offset_loop_outward(l, if away_out { 0.1 } else { -0.1 });
    let mut best: Option<f64> = None;
    for q in probe.iter().flat_map(|r| r.flatten(0.5)) {
        if let Some(z) = p.model.top_at(q) {
            best = Some(best.map_or(z, |b: f64| b.max(z)));
        }
    }
    match best {
        None => Some(p.model.min[2]),
        Some(z) if z < face_z - 1e-6 => Some(z),
        Some(z) if z <= face_z + 1e-6 => Some(face_z),
        Some(_) => None,
    }
}

/// ホストが解いた輪郭の値(`plan` の入力の `params`)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContourPlan {
    /// 欄の値(削る側・タブ・横の荒取りの数など、式でない物を読む)
    pub c: Contour,
    pub entry_plan: EntryPlan,
    pub ramp: Option<f64>,
    pub extra: f64,
    pub leave: f64,
    /// タブの幅と高さ
    pub tab: (f64, f64),
    pub finish: f64,
    pub lead: Option<LeadShape>,
    pub fin: FinishPlan,
    pub rough_step: f64,
    /// 削る形(ホストが面の縁とスケッチの線から作る)
    targets: Vec<Target>,
    /// 残り削りの前の工具
    pub rest: Option<Tool>,
}

/// 戦略の純関数: 解いた入力だけから経路を作る(文書とカーネルに触らない)。
pub fn plan(input: &StrategyInput) -> Result<StrategyOutput, String> {
    run(input).map_err(plan_error)
}

fn run(input: &StrategyInput) -> Result<StrategyOutput, EvalError> {
    let mut p = Planner::new(input);
    let f: ContourPlan = input.params().map_err(invalid)?;
    let ContourPlan {
        c,
        entry_plan: plan,
        ramp,
        extra,
        leave,
        tab: (tab_width, tab_height),
        finish,
        lead,
        fin,
        rough_step,
        targets,
        rest,
    } = f;
    let top = p.top(p.info.stock_max[2]);
    if let Some(big) = rest {
        return c.rest(p, &big, &targets, (top, extra, leave), ramp);
    }
    let mut tp = p.toolpath();
    let mut notes = Notes::default();
    // 入る位置は、それに最も近い輪郭だけで使う
    let entry_target = c.entry.and_then(|e| {
        targets
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let d = match &t.shape {
                    Shape::Closed { l, .. } => {
                        let s = start_at(l, e);
                        dist([s.verts[0].x, s.verts[0].y], e)
                    }
                    Shape::Open(pts) => dist(pts[0], e).min(dist(pts[pts.len() - 1], e)),
                };
                (i, d)
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    });
    let mut cut_any = false;
    let mut through_no_tabs = false;
    let tab_paths = std::cell::RefCell::new(Vec::new());
    for (i, t) in targets.iter().enumerate() {
        let bottom = p.bottom(if t.extra { t.floor - extra } else { t.floor });
        if bottom >= top - 1e-9 {
            notes.no_depth = true;
            continue;
        }
        p.check_depth(top, bottom)?;
        if !c.has_tabs() && bottom <= p.info.stock_min[2] + 1e-6 {
            through_no_tabs = true;
        }
        let entry = if Some(i) == entry_target {
            c.entry
        } else {
            None
        };
        let ctxs = Cut {
            c: &c,
            p: &p,
            plan: &plan,
            lead,
            top,
            floor: t.floor,
            bottom,
            entry,
            leave,
            finish,
            rough_step,
            tab: (tab_width, tab_height),
            fin,
            tab_paths: &tab_paths,
        };
        cut_any |= match &t.shape {
            Shape::Closed { l, tool_out } => {
                ctxs.closed(&mut tp, l, *tool_out && !t.on, t.on, &mut notes)
            }
            Shape::Open(pts) => {
                ctxs.open(&mut tp, pts, &mut notes);
                true
            }
        };
    }
    if !cut_any {
        return Err(invalid(if notes.no_depth {
            "削る深さがありません"
        } else {
            "工具が輪郭の内側に入りません"
        }));
    }
    if through_no_tabs {
        p.warnings.push(
            "ストックの底まで切り抜くのにタブがありません。部品が外れて飛ぶことがあるので、\
             タブを足すか、両面テープなどで固定してください"
                .into(),
        );
    }
    notes.report(&mut p.warnings, c.entry_opts.entry_kind);
    // タブを手で置く手順で見せる経路(タブの付く、最後の周の工具の中心)
    let paths = TabPaths {
        paths: tab_paths.into_inner(),
    };
    let mut out = p.output(tp);
    if !paths.paths.is_empty() {
        out.extra
            .insert(TAB_PATHS.into(), kmb_core::Payload::to_value(&paths));
    }
    Ok(out)
}

impl Feature for Contour {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = self.base.inputs()?;
        for c in &self.chains {
            if let ChainRef::Sketch { sketch, .. } = c {
                v.push(output(*sketch, "curves"));
            }
        }
        v.extend(expr_refs(
            [
                &self.extra_depth,
                &self.leave,
                &self.tab_width,
                &self.tab_height,
            ]
            .into_iter()
            .chain(&self.ramp_angle)
            .chain(&self.finish)
            .chain(&self.lead)
            .chain(&self.depth)
            .chain(&self.rough_step)
            .chain(self.entry_opts.exprs())
            .chain(self.finish_opts.exprs())
            .chain(self.lead_opts.exprs()),
        )?);
        Ok(v)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let mut p = prepare(ctx, &self.base)?;
        let ramp = ramp_angle(ctx, &self.ramp_angle)?;
        let entry_plan = self.entry_opts.read(ctx, &p, ramp)?;
        let extra = length(ctx, &self.extra_depth, "さらに下げる深さ")?;
        let leave = length(ctx, &self.leave, "残し量")?;
        let tab_width = length(ctx, &self.tab_width, "タブの幅")?;
        let tab_height = length(ctx, &self.tab_height, "タブの高さ")?;
        let finish = match &self.finish {
            Some(f) => length(ctx, f, "仕上げ代")?,
            None => 0.0,
        };
        let lead = self.lead_opts.read(ctx, &p, &self.lead)?;
        let fin = self.finish_opts.read(ctx, &p)?;
        let rough_step = match &self.rough_step {
            Some(s) => length(ctx, s, "粗取りパスの間隔")?,
            None => p.r(),
        };
        if finish < 0.0 {
            return Err(invalid("仕上げ代は 0 以上にしてください"));
        }
        if self.has_tabs() && (tab_width <= 0.0 || tab_height <= 0.0) {
            return Err(invalid("タブの幅と高さは 0 より大きくしてください"));
        }
        if self.rough_passes > 1 && !(rough_step > 0.0 && rough_step <= 2.0 * p.r() + 1e-9) {
            return Err(invalid(
                "横の荒取りの間隔は 0 より大きく、工具径以下にしてください",
            ));
        }
        if self.rough_passes > 50 {
            return Err(invalid("横の荒取りの数は 50 以下にしてください"));
        }
        let mut skipped = Vec::new();
        let nested_outlines = if self.face.is_none()
            && self.chains.is_empty()
            && p.info
                .sections
                .as_ref()
                .is_some_and(|s| s.kind == kmb_cam_setup::sections::SectionKind::Sheets)
        {
            Some(p.body_outlines(ctx)?)
        } else {
            None
        };
        let targets = self.targets(ctx, &p, &mut skipped, nested_outlines)?;
        p.warnings.extend(skipped);
        let rest = rest_tool_or_size(ctx, &p, self.base.tools, self.rest_tool, &self.rest_size)?;
        let params = ContourPlan {
            c: self.clone(),
            entry_plan,
            ramp,
            extra,
            leave,
            tab: (tab_width, tab_height),
            finish,
            lead,
            fin,
            rough_step,
            targets,
            rest,
        };
        let input = p.input(ctx.feature_id(), params, Default::default());
        let out = plan(&input);
        let mut outputs = p.finish_plan(self.base.setup, out)?;
        kmb_cam_setup::sections::add_entry(&mut outputs, &p.info, entry_plan);
        Ok(outputs)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        let mut c = self.clone();
        Ok(c.base
            .rename(
                [
                    &mut c.extra_depth,
                    &mut c.leave,
                    &mut c.tab_width,
                    &mut c.tab_height,
                ]
                .into_iter()
                .chain(c.ramp_angle.as_mut())
                .chain(c.finish.as_mut())
                .chain(c.lead.as_mut())
                .chain(c.depth.as_mut())
                .chain(c.rough_step.as_mut())
                .chain(c.entry_opts.exprs_mut())
                .chain(c.finish_opts.exprs_mut())
                .chain(c.lead_opts.exprs_mut())
                .chain(c.rest_size.exprs_mut()),
                old,
                new,
            )?
            .then(|| serde_json::to_value(&c).expect("JSON にできる")))
    }
}

pub fn register(reg: &mut Registry) {
    reg.register_described(
        kmb_cam_setup::operation(
            CONTOUR,
            "輪郭",
            "選んだ輪郭(面の外周と穴の縁、スケッチの線)に沿って削る。選ばなければ部品の外形",
            [
                vec![
                    Field::choice(
                        "side",
                        "削る側",
                        &[
                            ("outside", "外側(穴の縁なら穴の中)"),
                            ("inside", "内側(面の上)"),
                            ("on", "線の上"),
                        ],
                    )
                    .help(
                        "外側: 輪郭の外を削る。面の縁では面から離れる側(外周なら外、穴なら穴の中)。\
                         内側: 輪郭の内(面の上)を削る。線の上: 工具の中心が線をなぞる(溝を彫る)",
                    ),
                    Field::expr("extra_depth", "さらに下げる深さ").length().help(
                        "部品の底まで切り抜く時に、底よりさらに下げる深さ。底が部品の面(ポケットの底など)なら使わない",
                    ),
                    Field::opt_expr("depth", "線からの深さ").length().help(
                        "スケッチの線を削る時の深さ(線の平面から)。空なら部品の底まで。\
                         面の縁は、縁の先が抜けていれば部品の底、低い面があればその高さまで",
                    ),
                    Field::expr("leave", "壁の残し量").length(),
                    Field::opt_expr("ramp_angle", "ランプ角度").angle()
                        .help("例: 3 deg。斜めとらせんの傾き。入り方が自動の時、空なら真下に入る"),
                ],
                entry_fields(&ENTRY_KINDS),
                vec![
                    Field::integer("rough_passes", "粗取りパスの数").unitless()
                        .help("2 以上なら、壁から離れた所から壁へ、何周かに分けて削る。0 か 1 なら壁に沿って 1 周"),
                    Field::opt_expr("rough_step", "粗取りパスの間隔").length()
                        .help("例: 2 mm。空なら工具径の半分。工具径より大きくできない"),
                    Field::integer("tabs", "タブの数").unitless()
                        .help("切り抜く部品を残しておく橋。0 なら作らない。輪郭に等間隔に置く(手で置いたタブがあればそちらを使う)"),
                    Field::info("tab_points", "手で置いたタブ").help(
                        "3D のビューで「タブを置く」から、経路の上をクリックで足す・外す。空なら数の欄で等間隔",
                    ),
                    Field::expr("tab_width", "タブの幅").length(),
                    Field::expr("tab_height", "タブの高さ").length().help("底の面から"),
                    Field::choice(
                        "tab_shape",
                        "タブの形",
                        &[("rect", "四角"), ("triangle", "三角")],
                    )
                    .help("三角は中心で高さいっぱい、縁で底まで下がる。外す時に折りやすい"),
                    Field::opt_expr("finish", "仕上げ代").length().help(
                        "例: 0.2 mm。粗削りでこの分を残し、最後に底の深さで仕上げる。空なら仕上げない",
                    ),
                ],
                finish_fields(),
                vec![
                    Field::opt_integer("rest_tool", "取残し加工: 前の工具の番号").unitless().help(
                        "前に同じ輪郭を削った太い工具。その工具が届かなかった内側の隅や細い溝だけを、\
                         今の工具で削る。今の工具は平・ブルノーズ・V・テーパー。\
                         タブ・仕上げ・横の荒取り・入りと抜けの円弧は使わない",
                    ),
                ],
                RestSize::fields().to_vec(),
                vec![
                    direction_field(),
                    lead_field(),
                ],
                lead_shape_fields(),
                kmb_cam_setup::height_fields().to_vec(),
            ]
            .concat(),
        )
        .icon("contour")
        .retarget(kmb_core::Retarget::Face {
            key: "face".into(),
        }),
        |d| Ok(Box::new(kmb_cam_setup::parse::<Contour>(d)?)),
    );
}

/// この crate の名乗り: 登録する種類と、加工を足す操作(画面の名前・まとまり・初めの値)。
pub fn module() -> kmb_cam_setup::StrategyModule {
    use kmb_cam_setup::{FaceMenu, Strategy, StrategyModule, to_data};
    use kmb_cam_tool::ToolKind;
    StrategyModule::new(
        "kmb-strategy-contour2d",
        register,
        vec![
            Strategy::new(CONTOUR, "cam.add.contour", "輪郭", |p| {
                to_data(Contour {
                    base: p.base(),
                    face: p.planar_face(),
                    side: Side::Outside,
                    extra_depth: "0.3 mm".into(),
                    leave: "0 mm".into(),
                    ramp_angle: Some("3 deg".into()),
                    tabs: 0,
                    tab_width: "4 mm".into(),
                    tab_height: "1.5 mm".into(),
                    finish: None,
                    direction: Default::default(),
                    lead: None,
                    chains: vec![],
                    depth: None,
                    entry: None,
                    entry_opts: Default::default(),
                    rough_passes: 0,
                    rough_step: None,
                    tab_shape: Default::default(),
                    tab_points: Vec::new(),
                    finish_opts: Default::default(),
                    lead_opts: Default::default(),
                    rest_tool: None,
                    rest_size: Default::default(),
                })
            })
            .order(3)
            .icon("contour")
            // 前には出さない(Kie 2026-10-21 の画板)
            .not_front()
            .aliases("りんかく contour profile がいけい")
            .hint("面を選ぶとその外周、選ばなければ部品の外形を削る")
            .face_menu(FaceMenu::AnyFace)
            .entry("輪郭の、クリックした所に最も近い所から入る。らせんならそこを中心に回り、下穴ならそこで下りる")
            .tabs()
            .pick_chains()
            .cuts_free()
            // 同じ種類の操作で続けるので、V・テーパーも使える
            .rest(&[ToolKind::FlatEnd, ToolKind::VBit, ToolKind::TaperEnd]),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_data_reads_the_same() {
        let v = serde_json::json!({
            "setup": 1, "tools": 2, "tool": 1, "side": "inside", "extra_depth": "0 mm", "leave": "0 mm"
        });
        let c: Contour = serde_json::from_value(v).expect("古い形のデータも読める");
        assert!(c.chains.is_empty());
        assert_eq!(c.side, Side::Inside);
        assert_eq!(c.entry_opts.entry_kind, EntryKind::Auto);
    }

    #[test]
    fn chain_refs_are_told_apart_by_their_keys() {
        let v = serde_json::json!([{"face": "f1", "ring": 2}, {"sketch": 7}]);
        let c: Vec<ChainRef> = serde_json::from_value(v).unwrap();
        assert_eq!(
            c[0],
            ChainRef::Face {
                face: "f1".into(),
                ring: Some(2)
            }
        );
        assert_eq!(
            c[1],
            ChainRef::Sketch {
                sketch: FeatureId(7),
                chain: None
            }
        );
    }

    #[test]
    fn the_exit_distance_leaves_the_box() {
        let d = exit_distance([5.0, 5.0], [1.0, 0.0], [0.0, 0.0], [10.0, 10.0]);
        assert!((d - 5.0).abs() < 1e-9);
    }
}
