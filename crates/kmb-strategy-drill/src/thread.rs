//! ねじ切り(`cam.thread@1`)。ねじ切りミルを、選んだ穴かボス(縦の円筒)の壁に沿うらせんで回し、
//! めねじ(穴)かおねじ(ボス)を切る。
//!
//! - 穴: 下穴(めねじの内径)の壁から、歯の先がねじの外径に届くまで、横の周に分けて広げる。
//!   ボス: ボスの壁(おねじの外径)から、歯の先がおねじの谷の径に届くまで、外から寄せる。
//!   山の深さはメートルねじ(ISO 68-1)の基準の形で決める(めねじ 0.541 P、おねじ 0.613 P)
//! - 1 周でピッチ(多条ねじはピッチ × 条数)だけ上がるか下がるらせん。右ねじで主軸が時計回りの時、
//!   ダウンカットはめねじが下から上(反時計回り)、おねじが上から下(時計回り)。左ねじとアップカットは逆
//! - 2 段以上の歯の工具は、歯の並ぶ長さの分だけ周を減らす(工具の歯の間隔とねじのピッチが同じ時だけ使える)
//! - 入りと抜け: 穴は中心から半円で壁に接して入り、半円で中心へ戻る。ボスは外から接線で入り、接線で抜ける
//!
//! 高さの地図のシミュレーションは、歯の横の空き(ねじ山の溝)を表せないので、首の柱だけを削る形とみなす
//! (`kmb_cam_tool::Tool::radius_at`)。首が壁に当たるかは、ここで調べて誤りにする。
//!
//! 削る円筒は 3D のビューで選んだ物だけ(選ばない時にすべての穴を探して切ることはしない。
//! docs/避ける形.md)。

use crate::Start;
use crate::cycle::down_to;
use crate::heights;
use crate::order::{self, Order};
use crate::{is_default, zero};
use kmb_cam_geom::Bore;
use kmb_cam_setup::{
    CutDir, OpBase, Planner, StrategyInput, StrategyOutput, approach, direction_field, expr_refs,
    invalid, length, opt_length, plan_error, prepare, retract,
};
use kmb_cam_tool::ToolKind;
use kmb_core::{EvalCtx, EvalError, Feature, Field, InputRef, Outputs, Ref, RefKind, Registry};
use kmb_toolpath::{ArcDir, Move, Toolpath};
use serde::{Deserialize, Serialize};

pub const THREAD: &str = "cam.thread@1";

/// めねじの山の深さ(半径の差)÷ ピッチ(ISO 68-1 の 5H/8、H = 0.866 P)。
pub const INTERNAL_DEPTH: f64 = 0.541_266;
/// おねじの山の深さ ÷ ピッチ(ISO 898 の谷の径 d3 = d − 1.226 869 P の半分)。
pub const EXTERNAL_DEPTH: f64 = 0.613_434;

/// ねじの向き。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Hand {
    /// 右ねじ(時計回りに回すと締まる。ふつうのねじ)
    #[default]
    Right,
    /// 左ねじ
    Left,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Thread {
    #[serde(flatten)]
    pub base: OpBase,
    /// ねじを切る円筒(穴かボスの壁の面)。3D のビューで選ぶ
    #[serde(default, with = "kmb_core::one_or_many")]
    pub faces: Vec<Ref>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub hand: Hand,
    /// ピッチ(1 周で進む山の間隔。インチねじは 1 / 山数)
    #[serde(default = "pitch")]
    pub pitch: String,
    /// ねじの外径(呼び径)。空なら、穴は下穴の径 + 2 × 0.541 P、ボスはボスの径
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub major: Option<String>,
    /// ねじを切る長さ(円筒の上から)。空なら円筒の全長
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<String>,
    /// 横の周の数(山の深さを等しく分ける)
    #[serde(default = "one")]
    pub passes: u32,
    /// 最後の周をもう 1 回まわる(刃のたわみで残った分を取る)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub spring: bool,
    /// 径(半径)に残す厚さ。正ならめねじは小さめ、おねじは大きめに仕上がる(はめ合いの調整)
    #[serde(default = "zero")]
    pub leave: String,
    /// 条数(多条ねじ)。1 周で ピッチ × 条数 進む
    #[serde(default = "one")]
    pub starts: u32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub order: Order,
    #[serde(default, skip_serializing_if = "CutDir::is_climb")]
    pub direction: CutDir,
}

fn pitch() -> String {
    "1 mm".into()
}

fn one() -> u32 {
    1
}

impl Thread {
    /// 右ねじ、ピッチ 1 mm、横の周 1 回のねじ切り(残りの項目を `..Thread::new(base)` で埋めるのに使う)。
    pub fn new(base: OpBase) -> Thread {
        Thread {
            base,
            faces: vec![],
            hand: Hand::Right,
            pitch: pitch(),
            major: None,
            length: None,
            passes: 1,
            spring: false,
            leave: zero(),
            starts: 1,
            order: Order::Nearest,
            direction: CutDir::Climb,
        }
    }

    fn exprs_mut(&mut self) -> impl Iterator<Item = &mut String> {
        [&mut self.pitch, &mut self.leave]
            .into_iter()
            .chain([&mut self.major, &mut self.length].into_iter().flatten())
    }
}

/// 1 つの円筒のねじの切り方(セットアップの座標)。
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub center: [f64; 2],
    pub hole: bool,
    /// 工具の中心が回る円の半径(削る順。最後が仕上げ)
    pub radii: Vec<f64>,
    /// 先端の通る高さの下と上(らせんの始めと終わりのどちらかの端)
    pub lo: f64,
    pub hi: f64,
    /// 反時計回りか
    pub ccw: bool,
    /// 上へ進むか
    pub up: bool,
    /// 1 周で進む高さ(ピッチ × 条数)
    pub lead: f64,
    /// 条の数と、条の間の高さ(ピッチ)
    pub starts: u32,
    pub pitch: f64,
    /// 送り高さ
    pub r: f64,
}

/// らせんの周の数。先端が `lo` から始まり、歯の並ぶ長さ `span` を足して `top` まで届く数(1 以上)。
pub fn turns(lo: f64, top: f64, span: f64, lead: f64) -> usize {
    (((top - lo - span) / lead) - 1e-9).ceil().max(1.0) as usize
}

/// 中心の周の半径の列。`from` は歯の先が壁に触れる半径、`to` は仕上げの半径。山の深さを等しく分ける。
pub fn pass_radii(from: f64, to: f64, passes: u32, spring: bool) -> Vec<f64> {
    let n = passes.max(1);
    let mut v: Vec<f64> = (1..=n)
        .map(|k| from + (to - from) * k as f64 / n as f64)
        .collect();
    if spring {
        v.push(to);
    }
    v
}

/// 1 つの円筒を削る動き。始めは最初の周の入る所の上(安全高さか送り高さ)。終わりは送り高さ。
pub fn cut(tp: &mut Toolpath, pl: &Plan, feed: f64, plunge: f64) {
    let c = pl.center;
    let dir = if pl.ccw { ArcDir::Ccw } else { ArcDir::Cw };
    let last = |tp: &Toolpath| tp.moves.last().map_or([c[0], c[1], tp.safe_z], |m| m.to());
    let line = |tp: &mut Toolpath, to: [f64; 3], f: f64| {
        if kmb_toolpath::dist(last(tp), to) > 1e-9 {
            tp.moves.push(Move::Linear { to, feed: f });
        }
    };
    let span = pl.hi - pl.lo;
    let (z0, z1) = if pl.up {
        (pl.lo, pl.hi)
    } else {
        (pl.hi, pl.lo)
    };
    let n = ((span / pl.lead).round() as usize).max(1);
    // 進む向きの接線(反時計回りなら角度の増える向き)
    let sign = if pl.ccw { 1.0 } else { -1.0 };
    for k in 0..pl.starts {
        // 条ごとに、らせんの始めの角度を 1 / 条数 周ずつ回す(同じ高さの範囲で、1 ピッチずらしたのと同じ)
        let a = std::f64::consts::TAU * k as f64 / pl.starts as f64;
        let (cos, sin) = (a.cos(), a.sin());
        let tangent = [-sin * sign, cos * sign];
        for &rc in &pl.radii {
            let s = [c[0] + rc * cos, c[1] + rc * sin];
            let m = [c[0] + rc / 2.0 * cos, c[1] + rc / 2.0 * sin];
            if pl.hole {
                // 中心で始めの高さへ下り、半円で壁に接して入る
                down_to(tp, c, pl.r, plunge);
                line(tp, [c[0], c[1], pl.r], feed);
                line(tp, [c[0], c[1], z0], plunge);
                tp.moves.push(Move::Arc {
                    to: [s[0], s[1], z0],
                    center: m,
                    dir,
                    feed,
                });
            } else {
                // 外の接線の上から下り、接線で入る(進む向きの後ろから)
                let o = boss_lead(s, tangent, -rc.max(1.0));
                down_to(tp, o, pl.r, plunge);
                line(tp, [o[0], o[1], pl.r], feed);
                line(tp, [o[0], o[1], z0], plunge);
                line(tp, [s[0], s[1], z0], feed);
            }
            for i in 1..=n {
                tp.moves.push(Move::Arc {
                    to: [s[0], s[1], z0 + (z1 - z0) * i as f64 / n as f64],
                    center: c,
                    dir,
                    feed,
                });
            }
            if pl.hole {
                // 半円で中心へ戻り、真上へ抜ける
                tp.moves.push(Move::Arc {
                    to: [c[0], c[1], z1],
                    center: m,
                    dir,
                    feed,
                });
                tp.moves.push(Move::Rapid {
                    to: [c[0], c[1], pl.r],
                });
            } else {
                // 接線で離れ、真上へ抜ける
                let o = boss_lead(s, tangent, rc.max(1.0));
                line(tp, [o[0], o[1], z1], feed);
                tp.moves.push(Move::Rapid {
                    to: [o[0], o[1], pl.r],
                });
            }
        }
    }
}

/// ボスの周の始めの点 `s` から、接線 `t` の向きに `d` 進んだ点(負なら後ろ)。
fn boss_lead(s: [f64; 2], t: [f64; 2], d: f64) -> [f64; 2] {
    [s[0] + t[0] * d, s[1] + t[1] * d]
}

/// ボスの周の、入る所(1 条目の最初の周の、接線の後ろ)。
pub fn boss_entry(pl: &Plan) -> [f64; 2] {
    let rc = pl.radii[0];
    let sign = if pl.ccw { 1.0 } else { -1.0 };
    boss_lead([pl.center[0] + rc, pl.center[1]], [0.0, sign], -rc.max(1.0))
}

/// ねじの寸法(切る高さを除く)を円筒 `b` と工具から決める。`major` は空なら自動。
#[allow(clippy::too_many_arguments)]
pub fn radii_for(
    b: &Bore,
    pitch: f64,
    major: Option<f64>,
    leave: f64,
    tool_r: f64,
    neck_r: f64,
    passes: u32,
    spring: bool,
) -> Result<Vec<f64>, String> {
    let d = b.diameter();
    if b.hole {
        let major = major.unwrap_or(d + 2.0 * INTERNAL_DEPTH * pitch);
        if major <= d + 1e-6 {
            return Err(format!(
                "ねじの外径 {major:.3} mm が穴の径 {d:.3} mm 以下です(穴は下穴の径で描くか、外径を入れてください)"
            ));
        }
        if tool_r >= b.radius - 1e-6 {
            return Err(format!(
                "工具の径 {:.2} mm が穴の径 {d:.2} mm 以上なので、穴に入りません",
                tool_r * 2.0
            ));
        }
        let to = major / 2.0 - tool_r - leave;
        let from = b.radius - tool_r;
        if to <= from + 1e-6 {
            return Err("残す厚さが山の深さ以上なので、削る所がありません".into());
        }
        if to + neck_r > b.radius - 1e-3 {
            return Err(format!(
                "工具の首(径 {:.2} mm)が穴の壁に当たります(首の細い工具にしてください)",
                neck_r * 2.0
            ));
        }
        Ok(pass_radii(from, to, passes, spring))
    } else {
        let major = major.unwrap_or(d);
        let root = major / 2.0 - EXTERNAL_DEPTH * pitch;
        let to = root + tool_r + leave;
        let from = b.radius + tool_r;
        if to >= from - 1e-6 {
            return Err("残す厚さが山の深さ以上なので、削る所がありません".into());
        }
        if to - neck_r < b.radius + 1e-3 {
            return Err(format!(
                "工具の首(径 {:.2} mm)がボスの山に当たります(首の細い工具にしてください)",
                neck_r * 2.0
            ));
        }
        Ok(pass_radii(from, to, passes, spring))
    }
}

/// ホストが解いた値(`plan` の入力の `params`)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThreadPlan {
    pub c: Thread,
    pub pitch: f64,
    pub major: Option<f64>,
    pub len: Option<f64>,
    pub leave: f64,
}

/// 戦略の純関数: 解いた入力だけから経路を作る(文書とカーネルに触らない)。
pub fn plan(input: &StrategyInput) -> Result<StrategyOutput, String> {
    run(input).map_err(plan_error)
}

fn run(input: &StrategyInput) -> Result<StrategyOutput, EvalError> {
    let p = Planner::new(input);
    let ThreadPlan {
        c,
        pitch,
        major,
        len,
        leave,
    } = input.params().map_err(invalid)?;
    let Some(form) = p.tool.thread_form() else {
        return Err(invalid("ねじ切りにはねじ切りミルを選んでください"));
    };
    if c.passes == 0 {
        return Err(invalid("横の周の数は 1 以上にしてください"));
    }
    if c.starts == 0 {
        return Err(invalid("条数は 1 以上にしてください"));
    }
    if form.teeth > 1 {
        let tp_pitch = form.pitch.unwrap_or(pitch);
        if (tp_pitch - pitch).abs() > 1e-4 {
            return Err(invalid(format!(
                "工具 {} の歯の間隔 {tp_pitch} mm がねじのピッチ {pitch} mm と違います(1 山の工具か、同じピッチの工具にしてください)",
                p.tool.name
            )));
        }
        if c.starts > 1 {
            return Err(invalid("多条ねじは 1 山のねじ切りミルで切ってください"));
        }
    }
    if c.faces.is_empty() {
        return Err(invalid(
            "ねじを切る穴かボスの壁を、3D のビューで選んでください(「ねじを切る円筒」の欄)",
        ));
    }
    let all: Vec<&Bore> = p
        .model
        .bores
        .iter()
        .filter(|b| crate::circular::full_circle(&p.model, b))
        .collect();
    let mut targets: Vec<&Bore> = Vec::new();
    for r in &c.faces {
        let Ref::Face { face, .. } = r else {
            return Err(invalid(
                "ねじを切る円筒には、穴かボスの壁の面を選んでください",
            ));
        };
        let found = crate::holes::with_face(&all, face);
        if found.is_empty() {
            return Err(invalid(format!(
                "選んだ面 {face} が、上から見てひと回りある縦の穴かボスに見つかりません(形が変わったか、円筒ではない面です)"
            )));
        }
        for b in found {
            if !targets.iter().any(|t| std::ptr::eq(*t, b)) {
                targets.push(b);
            }
        }
    }
    let tool_r = p.tool.radius();
    let neck_r = p.tool.neck_radius();
    let span = (form.teeth.max(1) - 1) as f64 * pitch;
    let lead = pitch * c.starts as f64;
    let mut plans = Vec::new();
    for b in &targets {
        let radii = radii_for(b, pitch, major, leave, tool_r, neck_r, c.passes, c.spring)
            .map_err(|e| invalid(format!("径 {:.2} mm の円筒: {e}", b.diameter())))?;
        let h = heights(&p, b.z_max, Start::Hole);
        let top = p.top(b.z_max);
        let through = (b.z_min - p.model.min[2]).abs() < 1e-3;
        // 貫通穴とボスの根元が部品の底なら、最後の山まで切れるよう半ピッチ下まで
        let floor = if through {
            b.z_min - pitch / 2.0
        } else {
            b.z_min
        };
        let want = match len {
            Some(l) if l <= 0.0 => {
                return Err(invalid("ねじの長さは 0 より大きくしてください"));
            }
            Some(l) => top - l,
            None => floor,
        };
        if want < floor - 1e-6 {
            return Err(invalid(format!(
                "ねじの長さ {:.2} mm が円筒の深さ {:.2} mm より長くなっています",
                top - want,
                top - b.z_min
            )));
        }
        let lo = p.bottom(want);
        if lo >= top - 1e-6 {
            return Err(invalid("ねじを切る長さがありません"));
        }
        let n = turns(lo, top, span, lead);
        let hi = lo + n as f64 * lead;
        p.check_depth(top, lo)?;
        // ダウンカット: めねじは反時計回り、おねじは時計回り。右ねじは反時計回りで上へ進む
        let ccw = b.hole == c.direction.is_climb();
        let up = ccw == (c.hand == Hand::Right);
        plans.push(Plan {
            center: b.center,
            hole: b.hole,
            radii,
            lo,
            hi,
            ccw,
            up,
            lead,
            starts: c.starts,
            pitch,
            r: h.r,
        });
    }
    // ボスのまわりに、工具が回る隙間があるか(根元より高い部品があれば当たる)
    for (b, pl) in targets.iter().zip(&plans) {
        if b.hole {
            continue;
        }
        let outer = pl.radii[0] + tool_r;
        let blocked = (0..36).any(|k| {
            let t = k as f64 / 36.0 * std::f64::consts::TAU;
            [b.radius + 0.05, (b.radius + outer) / 2.0, outer - 0.05]
                .iter()
                .any(|d| {
                    let q = [b.center[0] + d * t.cos(), b.center[1] + d * t.sin()];
                    p.model.top_at(q).is_some_and(|z| z > pl.lo + 1e-3)
                })
        });
        if blocked {
            return Err(invalid(format!(
                "径 {:.2} mm のボスのまわりに、ねじ切りミルが回る隙間がありません",
                b.diameter()
            )));
        }
    }
    let pts: Vec<[f64; 2]> = targets.iter().map(|b| b.center).collect();
    let ranks: Vec<usize> = (0..targets.len()).collect();
    let start = [p.info.stock_min[0], p.info.stock_min[1]];
    let mut tp = p.toolpath();
    for i in order::order(&pts, &ranks, c.order, start) {
        let pl = &plans[i];
        let first = if pl.hole { pl.center } else { boss_entry(pl) };
        approach(&mut tp, first);
        cut(&mut tp, pl, p.cutting.feed, p.cutting.plunge);
    }
    retract(&mut tp);
    Ok(p.output(tp))
}

impl Feature for Thread {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = self.base.inputs()?;
        let mut c = self.clone();
        let exprs: Vec<String> = c.exprs_mut().map(|s| s.clone()).collect();
        v.extend(expr_refs(&exprs)?);
        Ok(v)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let mut p = prepare(ctx, &self.base)?;
        if p.tool.thread_form().is_none() {
            return Err(invalid(format!(
                "ねじ切りにはねじ切りミルを選んでください(工具 {} は{})",
                p.tool.name,
                p.tool.kind.label()
            )));
        }
        debug_assert_eq!(p.tool.kind, ToolKind::ThreadMill);
        let pitch = length(ctx, &self.pitch, "ピッチ")?;
        if pitch <= 0.0 {
            return Err(invalid("ピッチは 0 より大きくしてください"));
        }
        let major = opt_length(ctx, &self.major, "ねじの外径")?;
        let len = opt_length(ctx, &self.length, "ねじの長さ")?;
        let leave = length(ctx, &self.leave, "残し量")?;
        let params = ThreadPlan {
            c: self.clone(),
            pitch,
            major,
            len,
            leave,
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
            THREAD,
            "ねじ切り",
            "ねじ切りミルを穴かボスの壁に沿うらせんで回し、めねじ・おねじを切る。穴は下穴の径で描いておく",
            kmb_cam_setup::with_heights(vec![
                Field::pick("faces", "ねじを切る円筒", &[RefKind::Face], 1, None)
                    .help("3D のビューで穴かボスの壁(円筒の面)を選ぶ。いくつでも"),
                Field::choice("hand", "ねじの向き", &[("right", "右ねじ"), ("left", "左ねじ")]),
                Field::expr("pitch", "ピッチ").length().help(
                    "1 周で進む山の間隔。並目のメートルねじは M3 0.5・M4 0.7・M5 0.8・M6 1・M8 1.25・\
                     M10 1.5・M12 1.75 mm。インチねじは 1 / 山数(in)",
                ),
                Field::opt_expr("major", "ねじの外径").length().help(
                    "呼び径(M6 なら 6 mm)。空なら、穴は下穴の径 + 山の深さ(0.541 P)の 2 倍、ボスはボスの径",
                ),
                Field::opt_expr("length", "ねじの長さ").length().help("円筒の上から。空なら円筒の全長(貫通は半ピッチ下まで)"),
                Field::integer("passes", "横の周の数").unitless().help("山の深さを等しく分けて、何周で削るか"),
                Field::bool("spring", "仕上げでもう 1 周")
                    .help("最後の周をもう 1 回まわり、刃のたわみで残った分を取る"),
                Field::expr("leave", "残し量").length().help(
                    "径(半径)に残す厚さ。正ならめねじは小さめ・おねじは大きめに仕上がる。はめ合いの調整に",
                ),
                Field::integer("starts", "条数").unitless().help("多条ねじの条の数。1 周で ピッチ × 条数 進む"),
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
        .icon("threadmill"),
        |d| Ok(Box::new(kmb_cam_setup::parse::<Thread>(d)?)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bore(hole: bool, d: f64) -> Bore {
        Bore {
            center: [0.0, 0.0],
            radius: d / 2.0,
            z_min: -10.0,
            z_max: 0.0,
            hole,
            faces: vec![],
        }
    }

    #[test]
    fn an_m6_hole_is_cut_from_the_tap_drill_wall_out_to_the_major_diameter() {
        // 下穴 5 mm、刃の径 4.6 mm、首 3 mm。歯の先は 6 mm の円に届く
        let r = radii_for(&bore(true, 5.0), 1.0, Some(6.0), 0.0, 2.3, 1.5, 3, false).unwrap();
        assert_eq!(r.len(), 3);
        assert!((r[2] + 2.3 - 3.0).abs() < 1e-12, "{r:?}");
        assert!(r.windows(2).all(|w| w[0] < w[1]));
        assert!(((r[0] - 0.2) - (3.0 - 2.3 - 0.2) / 3.0).abs() < 1e-12);
        // 外径を空にすると、下穴の径 + 2 × 0.541 P
        let auto = radii_for(&bore(true, 4.917), 1.0, None, 0.0, 2.3, 1.5, 1, true).unwrap();
        assert_eq!(auto.len(), 2, "仕上げでもう 1 周");
        assert!((auto[1] + 2.3 - (4.917 / 2.0 + INTERNAL_DEPTH)).abs() < 1e-9);
        // 首が太いと壁に当たる
        assert!(
            radii_for(&bore(true, 5.0), 1.0, Some(6.0), 0.0, 2.3, 1.85, 1, false)
                .unwrap_err()
                .contains("首")
        );
        // 穴より太い工具は入らない
        assert!(radii_for(&bore(true, 5.0), 1.0, Some(6.0), 0.0, 2.6, 1.0, 1, false).is_err());
    }

    #[test]
    fn a_boss_is_cut_from_its_wall_in_to_the_root_diameter() {
        let r = radii_for(&bore(false, 8.0), 1.25, None, 0.05, 2.0, 1.0, 2, false).unwrap();
        let root = 4.0 - EXTERNAL_DEPTH * 1.25;
        assert!((r[1] - (root + 2.0 + 0.05)).abs() < 1e-12, "{r:?}");
        assert!(r[0] > r[1] && r[0] < 6.0);
    }

    #[test]
    fn the_helix_reaches_the_top_and_more_teeth_take_fewer_turns() {
        assert_eq!(turns(-10.0, 0.0, 0.0, 1.0), 10);
        assert_eq!(turns(-10.0, 0.0, 0.0, 1.5), 7);
        // 4 段の歯(3 ピッチ分の並び)は 7 周で届く
        assert_eq!(turns(-10.0, 0.0, 3.0, 1.0), 7);
        assert_eq!(turns(-1.0, 0.0, 5.0, 1.0), 1);
    }

    #[test]
    fn a_right_hand_inside_thread_climbs_counter_clockwise_and_upward() {
        let pl = Plan {
            center: [10.0, 20.0],
            hole: true,
            radii: vec![0.5, 0.7],
            lo: -8.0,
            hi: 0.0,
            ccw: true,
            up: true,
            lead: 1.0,
            starts: 1,
            pitch: 1.0,
            r: 2.0,
        };
        let mut tp = kmb_toolpath::Toolpath {
            name: "t".into(),
            tool: kmb_cam_tool::ToolLibrary::starter().tools[0].clone(),
            rpm: 10000.0,
            safe_z: 5.0,
            retract_z: 2.0,
            edge_break: 0.0,
            dwells: vec![],
            spans: vec![],
            links: Default::default(),
            moves: vec![Move::Rapid {
                to: [10.0, 20.0, 5.0],
            }],
        };
        cut(&mut tp, &pl, 300.0, 100.0);
        let arcs: Vec<(&[f64; 3], &[f64; 2], &ArcDir)> = tp
            .moves
            .iter()
            .filter_map(|m| match m {
                Move::Arc {
                    to, center, dir, ..
                } => Some((to, center, dir)),
                _ => None,
            })
            .collect();
        // 周ごとに: 入りの半円 1 + らせん 8 周 + 抜けの半円 1
        assert_eq!(arcs.len(), 2 * 10);
        assert!(arcs.iter().all(|a| *a.2 == ArcDir::Ccw));
        let helix: Vec<f64> = arcs
            .iter()
            .filter(|a| *a.1 == [10.0, 20.0])
            .map(|a| a.0[2])
            .collect();
        assert_eq!(helix.len(), 16);
        assert!(
            helix[..8]
                .windows(2)
                .all(|w| (w[1] - w[0] - 1.0).abs() < 1e-12)
        );
        assert!((helix[7] - 0.0).abs() < 1e-12);
        // 終わりは送り高さ
        assert_eq!(tp.moves.last().unwrap().to(), [10.0, 20.0, 2.0]);
        retract(&mut tp);
        assert!(
            kmb_toolpath::check(&tp).is_empty(),
            "{:?}",
            kmb_toolpath::check(&tp)
        );
    }
}
