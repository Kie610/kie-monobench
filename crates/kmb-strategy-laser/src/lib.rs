//! レーザー(`cam.laser@1`)。スケッチの線をなぞって焼く「線」と、閉じた形の中を平行な線で埋めて
//! 焼く「塗り」。機械に依らない汎用の加工で、入切と出力の命令は機械の値とポストが決める。
//!
//! - 工具はレーザー(工具の種類「レーザー」)。工具の切削条件の回転数の欄が出力(%)、送りが速さ
//! - 焼く高さは線の平面(材料の表面)+ 焦点のずらし。上下には動かない
//! - 塗りは、閉じた線の内側(偶奇の規則。文字の穴は焼かない)を、角度 `angle` の平行線で
//!   `interval` おきに埋める。往復すると速い。行の両端の外へ `overscan` だけ出力 0 の送りで
//!   はみ出し、加減速の間に焼かない(焼きむらを防ぐ。古くからある一般的な方式)
//! - 出力は速さで変えない(速さに比例させるのは機械のファームの役目。Grbl の M4 など)
//! - 削らないので、シミュレーションと空振りの省きは、この経路で材料を変えない
//! - 切り抜き(線で焼く時): 切り口の幅の半分だけ、残す側から離れる向きへ閉じた線をずらす
//!   (偶奇の規則で、外周は外へ、穴は内へ)。進入・退出(直線か 1/4 円)は捨てる側に付け、焼き始めの
//!   太い跡を形の外へ出す。重ね切りで閉じた線の始めを越えて焼き、貫通は焼き始めで止まったまま焼く。
//!   どれもレーザー加工で広く使われる一般的な方式(CNC の工具径補正と同じく古くからある)
//! - ガルバノの機械では塗りのはみ出しを 0 にする(鏡は加減速の遅れが小さく、焼き始めと終わりの
//!   遅れは制御盤の値で直す)

use kmb_cam_geom::{Loop, P2, offset_loop_outward, point_in_polygon};
use kmb_cam_setup::{Chain, chains, read_lines};
use kmb_cam_setup::{
    OpBase, Planner, StrategyInput, StrategyOutput, expr_refs, invalid, length, opt_length, output,
    plan_error, prepare,
};
use kmb_core::{
    Descriptor, EvalCtx, EvalError, Feature, FeatureId, Field, InputRef, Outputs, Registry,
    category,
};
use kmb_toolpath::{Dwell, Move, P3, Role, Span, Toolpath};
use serde::{Deserialize, Serialize};

pub const LASER: &str = "cam.laser@1";

/// 焼き方。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaserMode {
    /// 線の上をなぞる
    #[default]
    Line,
    /// 閉じた形の中を埋める
    Fill,
    /// 中を埋めてから縁をなぞる
    FillAndLine,
}

/// 切り口の補正の側(閉じた線だけ。偶奇の規則で内と外を決める)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KerfSide {
    /// 線の中心を焼く
    #[default]
    None,
    /// 線の内側(外周の内、穴の外)を部品として残す。外周は外へ、穴は内へずらす
    Outside,
    /// 線の内側を抜いて捨てる(板に穴をあける)。外周は内へずらす
    Inside,
}

/// 進入・退出の形。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lead {
    #[default]
    None,
    /// 線に直角な直線
    Line,
    /// 線に接する円弧(既定は 1/4 円。掃引角と、円弧の前後の直線を足せる)
    Arc,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Laser {
    #[serde(flatten)]
    pub base: OpBase,
    /// 焼く線・形を出す機能(スケッチなど)
    pub curves: FeatureId,
    #[serde(default)]
    pub mode: LaserMode,
    /// くり返す回数
    #[serde(default = "one")]
    pub passes: u32,
    /// 焦点のずらし(材料の表面から上が正)
    #[serde(default = "zero")]
    pub focus: String,
    /// 塗りの線の間隔
    #[serde(default = "interval")]
    pub interval: String,
    /// 塗りの線の角度(X から反時計回り)
    #[serde(default = "zero_deg")]
    pub angle: String,
    /// 往復で埋める(行ごとに向きを替える)
    #[serde(default = "yes")]
    pub both_ways: bool,
    /// 塗りを直角の向きにもう一度焼く
    #[serde(default)]
    pub cross: bool,
    /// 行の外へのはみ出し(出力 0 で通る長さ)。空なら機械の加速度と速さから決める
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overscan: Option<String>,
    /// 切り口の補正(線で焼く時の閉じた線)
    #[serde(default, skip_serializing_if = "is_default")]
    pub kerf_side: KerfSide,
    /// 切り口の幅。空なら工具の径(光の点の径)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kerf: Option<String>,
    /// 進入(閉じた線の焼き始めの前に、捨てる側から入る)
    #[serde(default, skip_serializing_if = "is_default")]
    pub lead_in: Lead,
    /// 進入の長さ(円弧なら半径)。空なら 1 mm
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead_in_length: Option<String>,
    /// 進入の円弧の掃引角。空なら 90 度
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead_in_sweep: Option<String>,
    /// 進入の円弧の前の直線(円弧の始めに接する)。空なら 0
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead_in_line: Option<String>,
    /// 退出(閉じた線を焼き終えた後に、捨てる側へ出る)
    #[serde(default, skip_serializing_if = "is_default")]
    pub lead_out: Lead,
    /// 退出の長さ(円弧なら半径)。空なら 0.5 mm
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead_out_length: Option<String>,
    /// 退出の円弧の掃引角。空なら 90 度
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead_out_sweep: Option<String>,
    /// 退出の円弧の後の直線(円弧の終わりに接する)。空なら 0
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead_out_line: Option<String>,
    /// 重ね切り(閉じた線の焼き始めを越えて焼く長さ。最後の回だけ)。空なら 0
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overcut: Option<String>,
    /// 貫通の時間(秒)。焼き始めの所で止まったまま焼く。0 なら止まらない
    #[serde(default, skip_serializing_if = "is_zero_f")]
    pub pierce_time: f64,
    /// 貫通の高さ(焼く高さから上)。空なら焼く高さのまま貫通する
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pierce_height: Option<String>,
    /// 貫通クリアランス(焼き始めの穴を、進入の始めからさらに捨てる側へ離す長さ。進入の始めの
    /// 直線に足す。進入がなしでも、この長さの直線で入る)。空なら 0
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pierce_clearance: Option<String>,
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}
/// 進入・退出の長さを決めない時(mm)
const LEAD_IN: f64 = 1.0;
const LEAD_OUT: f64 = 0.5;
fn is_zero_f(v: &f64) -> bool {
    *v == 0.0
}

fn one() -> u32 {
    1
}
fn zero() -> String {
    "0 mm".into()
}
fn zero_deg() -> String {
    "0 deg".into()
}
fn interval() -> String {
    "0.1 mm".into()
}
fn yes() -> bool {
    true
}

impl Laser {
    /// 線をなぞって 1 回焼くレーザー(残りの項目を `..Laser::new(base, curves)` で埋めるのに使う)。
    pub fn new(base: OpBase, curves: FeatureId) -> Laser {
        Laser {
            base,
            curves,
            mode: LaserMode::Line,
            passes: 1,
            focus: zero(),
            interval: interval(),
            angle: zero_deg(),
            both_ways: true,
            cross: false,
            overscan: None,
            kerf_side: KerfSide::None,
            kerf: None,
            lead_in: Lead::None,
            lead_in_length: None,
            lead_in_sweep: None,
            lead_in_line: None,
            lead_out: Lead::None,
            lead_out_length: None,
            lead_out_sweep: None,
            lead_out_line: None,
            overcut: None,
            pierce_time: 0.0,
            pierce_height: None,
            pierce_clearance: None,
        }
    }
}

/// はみ出しを決めない時の長さ(機械の加速度が分からない時。mm)。
const OVERSCAN: f64 = 2.0;

/// 速さ `speed`(mm/min)から加速度 `accel`(mm/s²)で止まるまでの長さ(加速度が分からなければ
/// [`OVERSCAN`])。0.5〜10 mm に収める。
pub fn auto_overscan(speed: f64, accel: f64) -> f64 {
    if accel <= 0.0 {
        return OVERSCAN;
    }
    let v = speed / 60.0;
    (v * v / (2.0 * accel)).clamp(0.5, 10.0)
}

/// 閉じた線(偶奇の規則)の内側を、角度 `angle`(ラジアン)の平行線で `interval` おきに切った区間。
/// 行ごとに `[始め, 終わり]` の区間の並び(左から右。回した座標の X の順)を返す。
pub fn hatch(loops: &[Vec<P2>], interval: f64, angle: f64) -> Vec<Vec<[P2; 2]>> {
    let (c, s) = (angle.cos(), angle.sin());
    // 線が X に沿うよう、-angle だけ回した座標で考える
    let to = |p: P2| [p[0] * c + p[1] * s, -p[0] * s + p[1] * c];
    let back = |q: P2| [q[0] * c - q[1] * s, q[0] * s + q[1] * c];
    let rings: Vec<Vec<P2>> = loops
        .iter()
        .map(|l| l.iter().map(|p| to(*p)).collect())
        .collect();
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for q in rings.iter().flatten() {
        lo = lo.min(q[1]);
        hi = hi.max(q[1]);
    }
    let mut rows = Vec::new();
    if lo >= hi || hi.is_nan() || interval <= 0.0 {
        return rows;
    }
    // 行ごとに幅 `interval` の帯を焼くと考え、帯の数を高さに収まる数にする。端の行が形の縁に
    // 重ならないよう、余りを両側に分ける
    let n = ((hi - lo) / interval - 1e-9).ceil().max(1.0) as usize;
    let start = lo + (hi - lo - (n - 1) as f64 * interval) / 2.0;
    for k in 0..n {
        let y = start + k as f64 * interval;
        let mut xs: Vec<f64> = Vec::new();
        for r in &rings {
            for i in 0..r.len() {
                let (a, b) = (r[i], r[(i + 1) % r.len()]);
                // 下端を含み上端を含まない(頂点を 2 回数えない)
                if (a[1] <= y) != (b[1] <= y) {
                    xs.push(a[0] + (y - a[1]) / (b[1] - a[1]) * (b[0] - a[0]));
                }
            }
        }
        xs.sort_by(f64::total_cmp);
        let segs: Vec<[P2; 2]> = xs
            .as_chunks::<2>()
            .0
            .iter()
            .filter(|p| p[1] - p[0] > 1e-9)
            .map(|p| [back([p[0], y]), back([p[1], y])])
            .collect();
        if !segs.is_empty() {
            rows.push(segs);
        }
    }
    rows
}

/// 行の並び `rows` を焼く動き(焦点の高さ `z`、速さ `speed`)を `tp` に足す。行の両端の外へ
/// `over` だけ出力 0 の送り(役目は「つなぎ」)ではみ出す。行の間も出力 0 の送りで移る。
fn burn_rows(tp: &mut Toolpath, rows: &[Vec<[P2; 2]>], z: f64, speed: f64, over: f64, both: bool) {
    for (k, row) in rows.iter().enumerate() {
        let forward = !both || k % 2 == 0;
        let segs: Vec<[P2; 2]> = if forward {
            row.clone()
        } else {
            row.iter().rev().map(|s| [s[1], s[0]]).collect()
        };
        let (first, last) = (segs[0][0], segs[segs.len() - 1][1]);
        let dir = unit(first, last);
        let before = [first[0] - dir[0] * over, first[1] - dir[1] * over];
        let after = [last[0] + dir[0] * over, last[1] + dir[1] * over];
        go(tp, before, z, speed);
        if over > 0.0 {
            link(tp, [first[0], first[1], z], speed);
        }
        for (n, s) in segs.iter().enumerate() {
            if n > 0 {
                // 行の中のすき間(文字の穴など)は、出力 0 で同じ速さのまま越える
                link(tp, [s[0][0], s[0][1], z], speed);
            }
            tp.moves.push(Move::Linear {
                to: [s[1][0], s[1][1], z],
                feed: speed,
            });
        }
        if over > 0.0 {
            link(tp, [after[0], after[1], z], speed);
        }
    }
}

fn unit(a: P2, b: P2) -> P2 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l = dx.hypot(dy);
    if l < 1e-12 {
        [1.0, 0.0]
    } else {
        [dx / l, dy / l]
    }
}

/// 出力 0 の送り(役目は「つなぎ」)で `to` へ動く。早送りにしないのは、焦点の高さは材料の
/// 表面のすぐ上で、早送りの決まり(退避高さより下へ早送りで下りない)に合わせるため。
fn link(tp: &mut Toolpath, to: P3, speed: f64) {
    let i = tp.moves.len();
    tp.moves.push(Move::Linear { to, feed: speed });
    tp.spans.push(Span {
        start: i,
        end: i + 1,
        role: Role::Link,
    });
}

/// `xy` の焦点の高さ `z` へ移る。近ければその高さのまま出力 0 の送りで、遠ければ安全高さを
/// 通り、退避高さから出力 0 の送りで下りる。
fn go(tp: &mut Toolpath, xy: P2, z: f64, speed: f64) {
    /// この長さまでは焦点の高さのまま移る(塗りの次の行など)
    const NEAR: f64 = 5.0;
    let near = tp
        .moves
        .last()
        .map(|m| m.to())
        .is_some_and(|c| (c[2] - z).abs() < 1e-9 && (c[0] - xy[0]).hypot(c[1] - xy[1]) <= NEAR);
    if !near {
        kmb_cam_setup::approach(tp, xy);
    }
    let c = tp.moves.last().map(|m| m.to()).expect("近づいた後");
    if (c[0] - xy[0]).hypot(c[1] - xy[1]) > 1e-9 || (c[2] - z).abs() > 1e-9 {
        if c[2] < z {
            tp.moves.push(Move::Rapid {
                to: [c[0], c[1], z],
            });
        }
        link(tp, [xy[0], xy[1], z], speed);
    }
}

/// 塗りの値(`LaserPlan::fill`)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fill {
    pub interval: f64,
    /// 線の角度(ラジアン)
    pub angle: f64,
    /// はみ出し。空なら機械の加速度から決める(ガルバノなら 0)
    pub overscan: Option<f64>,
    pub cross: bool,
    pub both_ways: bool,
}

/// ホストが解いたレーザーの値(`plan` の入力の `params`)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LaserPlan {
    pub lines: Vec<Vec<P2>>,
    /// 焼く高さ(線の平面と焦点のずらし)
    pub z: f64,
    pub passes: u32,
    /// 塗る(塗らなければ空)
    pub fill: Option<Fill>,
    /// 線で焼く(焼かなければ空)
    cut: Option<Cut>,
}

/// 戦略の純関数: 解いた入力だけから経路を作る(文書とカーネルに触らない)。
pub fn plan(input: &StrategyInput) -> Result<StrategyOutput, String> {
    run(input).map_err(plan_error)
}

fn run(input: &StrategyInput) -> Result<StrategyOutput, EvalError> {
    let p = Planner::new(input);
    let LaserPlan {
        lines,
        z,
        passes,
        fill,
        cut,
    } = input.params().map_err(invalid)?;
    let speed = p.cutting.feed;
    let mut tp = p.toolpath();
    let all = chains(lines);
    if let Some(f) = &fill {
        let m = &p.info.machine;
        let over = match f.overscan {
            Some(o) => o,
            None if m.galvo => 0.0,
            None => auto_overscan(speed, m.acceleration),
        };
        let loops: Vec<Vec<P2>> = all
            .iter()
            .filter(|c| c.closed)
            .map(|c| c.pts[..c.pts.len() - 1].to_vec())
            .collect();
        if loops.is_empty() {
            return Err(invalid(
                "塗る形がありません(閉じた線を描いてください。開いた線は「線」で焼く)",
            ));
        }
        let mut angles = vec![f.angle];
        if f.cross {
            angles.push(f.angle + std::f64::consts::FRAC_PI_2);
        }
        for _ in 0..passes {
            for a in &angles {
                let rows = hatch(&loops, f.interval, *a);
                burn_rows(&mut tp, &rows, z, speed, over, f.both_ways);
            }
        }
    }
    if let Some(cut) = &cut {
        let all = cut.prepare(all)?;
        trace(&mut tp, &all, z, speed, passes, cut);
    }
    if tp.moves.is_empty() {
        return Err(invalid("焼く所がありません"));
    }
    kmb_cam_setup::retract(&mut tp);
    Ok(p.output(tp))
}

impl Feature for Laser {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = self.base.inputs()?;
        v.push(output(self.curves, "curves"));
        v.extend(expr_refs([&self.focus, &self.interval, &self.angle])?);
        v.extend(expr_refs(
            [
                &self.overscan,
                &self.kerf,
                &self.lead_in_length,
                &self.lead_in_sweep,
                &self.lead_in_line,
                &self.lead_out_length,
                &self.lead_out_sweep,
                &self.lead_out_line,
                &self.overcut,
                &self.pierce_height,
                &self.pierce_clearance,
            ]
            .into_iter()
            .flatten(),
        )?);
        Ok(v)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let mut p = prepare(ctx, &self.base)?;
        if p.tool.kind != kmb_cam_tool::ToolKind::Laser {
            return Err(invalid(format!(
                "工具 T{} {} はレーザーではありません。工具ライブラリに種類「レーザー」の工具を足して選んでください",
                p.tool.number, p.tool.name
            )));
        }
        let power = p.cutting.rpm;
        if !(0.0..=100.0).contains(&power) {
            return Err(invalid(format!(
                "出力は 0〜100 % にしてください(今は {power})。レーザーの工具では、切削条件の回転数の欄が出力(%)"
            )));
        }
        if p.cutting.feed <= 0.0 {
            return Err(invalid("速さは 0 より大きくしてください"));
        }
        if !(1..=50).contains(&self.passes) {
            return Err(invalid("回数は 1〜50 にしてください"));
        }
        let focus = length(ctx, &self.focus, "焦点のずらし")?;
        let (lines, top) = read_lines(ctx, &p, self.curves)?;
        let fill = matches!(self.mode, LaserMode::Fill | LaserMode::FillAndLine);
        let fill = if fill {
            let interval = length(ctx, &self.interval, "線の間隔")?;
            if !(0.01..=10.0).contains(&interval) {
                return Err(invalid("線の間隔は 0.01〜10 mm にしてください"));
            }
            let angle = ctx.expr(&self.angle)?;
            if angle.dim != kmb_core::Dim::ANGLE {
                return Err(invalid(format!(
                    "線の角度 `{}` は角度の単位(deg)が必要です",
                    self.angle
                )));
            }
            let overscan = opt_length(ctx, &self.overscan, "はみ出し")?;
            if overscan.is_some_and(|o| o < 0.0) {
                return Err(invalid("はみ出しは 0 以上にしてください"));
            }
            Some(Fill {
                interval,
                angle: angle.value,
                overscan,
                cross: self.cross,
                both_ways: self.both_ways,
            })
        } else {
            None
        };
        let cut = if fill.is_none() || self.mode == LaserMode::FillAndLine {
            Some(self.cut(ctx, p.tool.diameter)?)
        } else {
            None
        };
        let params = LaserPlan {
            lines,
            z: top + focus,
            passes: self.passes,
            fill,
            cut,
        };
        let input = p.input(ctx.feature_id(), params, Default::default());
        let out = plan(&input);
        p.finish_plan(self.base.setup, out)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        let mut c = self.clone();
        let mut exprs: Vec<&mut String> = vec![&mut c.focus, &mut c.interval, &mut c.angle];
        exprs.extend(
            [
                &mut c.overscan,
                &mut c.kerf,
                &mut c.lead_in_length,
                &mut c.lead_in_sweep,
                &mut c.lead_in_line,
                &mut c.lead_out_length,
                &mut c.lead_out_sweep,
                &mut c.lead_out_line,
                &mut c.overcut,
                &mut c.pierce_height,
                &mut c.pierce_clearance,
            ]
            .into_iter()
            .flatten(),
        );
        Ok(c.base
            .rename(exprs, old, new)?
            .then(|| serde_json::to_value(&c).expect("JSON にできる")))
    }
}

/// 切り口の補正で円弧を折れ線にする誤差(mm)
const TOL: f64 = 0.001;
/// 進入・退出の 1/4 円を分ける数
const ARC_STEPS: usize = 8;

/// 線で焼く時の切り抜きの値。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Cut {
    side: KerfSide,
    /// 切り口の幅
    kerf: f64,
    lead_in: LeadShape,
    lead_out: LeadShape,
    overcut: f64,
    /// 貫通の時間(秒)
    pierce: f64,
    /// 貫通の高さ(焼く高さから上)
    pierce_up: f64,
    /// 貫通クリアランス(進入の始めの直線に足す長さ)
    clearance: f64,
}

/// 進入・退出の形と大きさ。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
struct LeadShape {
    kind: Lead,
    /// 直線の長さ、円弧なら半径
    len: f64,
    /// 円弧の掃引角(ラジアン)
    sweep: f64,
    /// 円弧の外側の直線(進入は円弧の前、退出は後。円弧に接する)
    line: f64,
}

impl LeadShape {
    const NONE: LeadShape = LeadShape {
        kind: Lead::None,
        len: 0.0,
        sweep: std::f64::consts::FRAC_PI_2,
        line: 0.0,
    };

    /// 進入の始めの直線を貫通クリアランス `c` だけ延ばした形(直線は長さに、円弧は前の直線に足す。
    /// 進入がなしなら長さ `c` の直線)。
    fn with_clearance(self, c: f64) -> LeadShape {
        if c <= 1e-9 {
            return self;
        }
        match self.kind {
            Lead::None => LeadShape {
                kind: Lead::Line,
                len: c,
                ..self
            },
            Lead::Line => LeadShape {
                len: self.len + c,
                ..self
            },
            Lead::Arc => LeadShape {
                line: self.line + c,
                ..self
            },
        }
    }
}

/// 進入・退出の欄(`exprs` は長さ・掃引角・直線の式、`what` は「進入」か「退出」)。
fn lead_of(
    ctx: &mut EvalCtx<'_>,
    kind: Lead,
    (len, sweep, line): (&Option<String>, &Option<String>, &Option<String>),
    default: f64,
    what: &str,
) -> Result<LeadShape, EvalError> {
    if kind == Lead::None {
        return Ok(LeadShape::NONE);
    }
    let label = format!("{what}の長さ");
    let l = opt_length(ctx, len, &label)?.unwrap_or(default);
    if !(l > 0.0 && l <= 50.0) {
        return Err(invalid(format!(
            "{label}は 0 より大きく 50 mm 以下にしてください"
        )));
    }
    let mut shape = LeadShape {
        kind,
        len: l,
        ..LeadShape::NONE
    };
    if kind == Lead::Arc {
        if let Some(src) = sweep {
            let a = ctx.expr(src)?;
            if a.dim != kmb_core::Dim::ANGLE {
                return Err(invalid(format!(
                    "{what}の掃引角 `{src}` は角度の単位(deg)が必要です"
                )));
            }
            if !(a.value > 1e-9 && a.value <= std::f64::consts::PI + 1e-9) {
                return Err(invalid(format!(
                    "{what}の掃引角は 0 度より大きく 180 度以下にしてください"
                )));
            }
            shape.sweep = a.value.min(std::f64::consts::PI);
        }
        let label = format!("{what}の直線");
        shape.line = opt_length(ctx, line, &label)?.unwrap_or(0.0);
        if shape.line > 50.0 {
            return Err(invalid(format!("{label}は 0〜50 mm にしてください")));
        }
    }
    Ok(shape)
}

impl Laser {
    fn cut(&self, ctx: &mut EvalCtx<'_>, diameter: f64) -> Result<Cut, EvalError> {
        let kerf = opt_length(ctx, &self.kerf, "切り口の幅")?.unwrap_or(diameter);
        if !(0.0..=10.0).contains(&kerf) {
            return Err(invalid("切り口の幅は 0〜10 mm にしてください"));
        }
        let overcut = opt_length(ctx, &self.overcut, "重ね切り")?.unwrap_or(0.0);
        if !(0.0..=50.0).contains(&overcut) {
            return Err(invalid("重ね切りは 0〜50 mm にしてください"));
        }
        if !(0.0..=60.0).contains(&self.pierce_time) {
            return Err(invalid("貫通の時間は 0〜60 秒にしてください"));
        }
        let pierce_up = opt_length(ctx, &self.pierce_height, "貫通の高さ")?.unwrap_or(0.0);
        if !(0.0..=50.0).contains(&pierce_up) {
            return Err(invalid("貫通の高さは 0〜50 mm にしてください"));
        }
        let clearance = opt_length(ctx, &self.pierce_clearance, "貫通クリアランス")?.unwrap_or(0.0);
        if !(0.0..=50.0).contains(&clearance) {
            return Err(invalid("貫通クリアランスは 0〜50 mm にしてください"));
        }
        Ok(Cut {
            side: self.kerf_side,
            kerf,
            lead_in: lead_of(
                ctx,
                self.lead_in,
                (
                    &self.lead_in_length,
                    &self.lead_in_sweep,
                    &self.lead_in_line,
                ),
                LEAD_IN,
                "進入",
            )?,
            lead_out: lead_of(
                ctx,
                self.lead_out,
                (
                    &self.lead_out_length,
                    &self.lead_out_sweep,
                    &self.lead_out_line,
                ),
                LEAD_OUT,
                "退出",
            )?,
            overcut,
            pierce: self.pierce_time,
            pierce_up,
            clearance,
        })
    }
}

impl Cut {
    /// 補正・進入・退出・重ね切りのどれかを使うか(使わなければ描いた通りに焼く)。
    fn active(&self) -> bool {
        self.side != KerfSide::None || self.leads() || self.overcut > 0.0
    }

    fn leads(&self) -> bool {
        self.lead_in.kind != Lead::None || self.lead_out.kind != Lead::None || self.clearance > 0.0
    }

    /// 閉じた線を切り口の幅の半分だけ残す側から離し、捨てる側が進む向きの左になるよう向きを
    /// そろえる。進入・退出を付けるなら、焼き始めを最も長い辺の中ほどにする(角を避ける)。
    /// 開いた線はそのまま。
    fn prepare(&self, all: Vec<Chain>) -> Result<Vec<Chain>, EvalError> {
        if !self.active() {
            return Ok(all);
        }
        let rings: Vec<Vec<P2>> = all
            .iter()
            .filter(|c| c.closed)
            .map(|c| c.pts[..c.pts.len() - 1].to_vec())
            .collect();
        let mut out = Vec::new();
        let mut k = 0;
        for c in all {
            if !c.closed {
                out.push(c);
                continue;
            }
            let (i, ring) = (k, &rings[k]);
            k += 1;
            // 偶奇の規則: いくつの線に囲まれているか
            let depth = rings
                .iter()
                .enumerate()
                .filter(|(j, r)| *j != i && point_in_polygon(r, ring[0]))
                .count();
            // この線の内側を残すか(補正なしでも、進入は外周の外・穴の内に付ける)
            let keep_inside = (depth % 2 == 0) != (self.side == KerfSide::Inside);
            let mut loops = vec![ring.clone()];
            if self.side != KerfSide::None && self.kerf > 0.0 {
                let d = self.kerf / 2.0;
                let d = if keep_inside { d } else { -d };
                loops = offset_loop_outward(&Loop::from_points(ring), d)
                    .iter()
                    .map(|l| l.flatten(TOL))
                    .filter(|p| p.len() >= 3)
                    .collect();
                if loops.is_empty() {
                    return Err(invalid(format!(
                        "切り口の補正で消える形があります(切り口の幅 {} mm より細い形)",
                        self.kerf
                    )));
                }
            }
            for mut l in loops {
                // 内側を残すなら捨てる側は外で、外が左になるのは時計回り
                let ccw = Loop::from_points(&l).signed_area() > 0.0;
                if ccw == keep_inside {
                    l.reverse();
                }
                if self.leads() {
                    l = start_mid_longest(l);
                }
                l.push(l[0]);
                out.push(Chain {
                    pts: l,
                    closed: true,
                    reversed: false,
                });
            }
        }
        Ok(out)
    }
}

/// 閉じた点の列(終わりに始めを重ねない)を、最も長い辺の中点から始まるように並べ替える。
fn start_mid_longest(l: Vec<P2>) -> Vec<P2> {
    let n = l.len();
    let len = |i: usize| {
        let (a, b) = (l[i], l[(i + 1) % n]);
        (b[0] - a[0]).hypot(b[1] - a[1])
    };
    let i = (0..n)
        .max_by(|a, b| len(*a).total_cmp(&len(*b)))
        .unwrap_or(0);
    let (a, b) = (l[i], l[(i + 1) % n]);
    let mut out = vec![[(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]];
    out.extend((1..=n).map(|k| l[(i + k) % n]));
    out
}

/// 進む向き `t` の左(捨てる側)。
fn left(t: P2) -> P2 {
    [-t[1], t[0]]
}

fn add(p: P2, v: P2, s: f64) -> P2 {
    [p[0] + v[0] * s, p[1] + v[1] * s]
}

/// 円弧を分ける数(1/4 円で [`ARC_STEPS`])。
fn arc_steps(sweep: f64) -> usize {
    ((sweep / std::f64::consts::FRAC_PI_2 * ARC_STEPS as f64).ceil() as usize).max(1)
}

/// `p` へ向き `t` で入る進入の点(入る所から順に、`p` は含めない)。捨てる側 `n` に付ける。
fn lead_in_points(p: P2, t: P2, n: P2, s: LeadShape) -> Vec<P2> {
    let l = s.len;
    match s.kind {
        Lead::None => vec![],
        Lead::Line => vec![add(p, n, l)],
        Lead::Arc => {
            // 中心 c = p + n·l。p から角 φ だけ戻った点 c - n·l·cos φ - t·l·sin φ から、φ を 0 まで
            // 減らして p に接して入る。円弧の前の直線は、円弧の始めの向き t·cos φ - n·sin φ に沿う
            let c = add(p, n, l);
            let at = |phi: f64| add(add(c, n, -l * phi.cos()), t, -l * phi.sin());
            let steps = arc_steps(s.sweep);
            let mut v = Vec::new();
            if s.line > 1e-9 {
                let dir = [
                    t[0] * s.sweep.cos() - n[0] * s.sweep.sin(),
                    t[1] * s.sweep.cos() - n[1] * s.sweep.sin(),
                ];
                v.push(add(at(s.sweep), dir, -s.line));
            }
            v.extend((0..steps).map(|k| at(s.sweep * (1.0 - k as f64 / steps as f64))));
            v
        }
    }
}

/// `e` を向き `t` で出る退出の点(`e` は含めない)。
fn lead_out_points(e: P2, t: P2, n: P2, s: LeadShape) -> Vec<P2> {
    let l = s.len;
    match s.kind {
        Lead::None => vec![],
        Lead::Line => vec![add(e, n, l)],
        Lead::Arc => {
            // 中心 c = e + n·l。e から角 a だけ進んだ点 c - n·l·cos a + t·l·sin a。円弧の後の直線は、
            // 円弧の終わりの向き t·cos a + n·sin a に沿う
            let c = add(e, n, l);
            let at = |a: f64| add(add(c, n, -l * a.cos()), t, l * a.sin());
            let steps = arc_steps(s.sweep);
            let mut v: Vec<P2> = (1..=steps)
                .map(|k| at(s.sweep * k as f64 / steps as f64))
                .collect();
            if s.line > 1e-9 {
                let dir = [
                    t[0] * s.sweep.cos() + n[0] * s.sweep.sin(),
                    t[1] * s.sweep.cos() + n[1] * s.sweep.sin(),
                ];
                v.push(add(at(s.sweep), dir, s.line));
            }
            v
        }
    }
}

/// 閉じた線 `pts`(終わりに始めを重ねる)を始めから `len` だけたどった点と、終わりの向き。
fn walk(pts: &[P2], len: f64) -> (Vec<P2>, P2) {
    let n = pts.len();
    let mut t = unit(pts[n - 2], pts[n - 1]);
    let mut out = Vec::new();
    let mut rest = len;
    for w in pts.windows(2) {
        if rest <= 1e-9 {
            break;
        }
        let d = (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
        if d < 1e-12 {
            continue;
        }
        t = unit(w[0], w[1]);
        if d >= rest {
            out.push(add(w[0], t, rest));
            break;
        }
        out.push(w[1]);
        rest -= d;
    }
    (out, t)
}

/// 送りの動きを足す。`lead` なら役目を「リード」にする(焼く)。
fn burn(tp: &mut Toolpath, pts: &[P2], z: f64, speed: f64, lead: bool) {
    let i = tp.moves.len();
    tp.moves.extend(pts.iter().map(|q| Move::Linear {
        to: [q[0], q[1], z],
        feed: speed,
    }));
    if lead && tp.moves.len() > i {
        tp.spans.push(Span {
            start: i,
            end: tp.moves.len(),
            role: Role::Lead,
        });
    }
}

/// 線をなぞって焼く(閉じた線は回り続け、開いた線は行って戻る)。閉じた線には進入・重ね切り・
/// 退出を付け、焼き始めで貫通する。
fn trace(tp: &mut Toolpath, all: &[Chain], z: f64, speed: f64, passes: u32, cut: &Cut) {
    for c in all {
        let leads = c.closed && cut.active();
        let p0 = c.pts[0];
        let t0 = unit(p0, c.pts[1]);
        let lead_in = if leads {
            lead_in_points(p0, t0, left(t0), cut.lead_in.with_clearance(cut.clearance))
        } else {
            vec![]
        };
        let entry = lead_in.first().copied().unwrap_or(p0);
        let zp = z + cut.pierce_up;
        go(tp, entry, zp, speed);
        if cut.pierce > 0.0 {
            tp.dwells.push(Dwell {
                at: [entry[0], entry[1], zp],
                seconds: cut.pierce,
            });
        }
        let mut into: Vec<P2> = Vec::new();
        if zp > z + 1e-9 {
            // 貫通の高さから、焼きながら焼く高さへ下りる
            burn(tp, &[entry], z, speed, true);
        }
        into.extend(lead_in.iter().skip(1));
        if !lead_in.is_empty() {
            into.push(p0);
        }
        burn(tp, &into, z, speed, true);
        let mut forward = true;
        for _ in 0..passes {
            let pts: Vec<P2> = if forward {
                c.pts.clone()
            } else {
                c.pts.iter().rev().copied().collect()
            };
            burn(tp, &pts[1..], z, speed, false);
            if !c.closed {
                forward = !forward;
            }
        }
        if leads {
            let (over, t) = walk(&c.pts, cut.overcut);
            burn(tp, &over, z, speed, false);
            let e = over.last().copied().unwrap_or(p0);
            let out = lead_out_points(e, t, left(t), cut.lead_out);
            burn(tp, &out, z, speed, true);
        }
    }
}

pub fn register(reg: &mut Registry) {
    let base = kmb_cam_setup::op_fields();
    let keep = |k: &str| base.iter().find(|f| f.key == k).cloned();
    let mut fields: Vec<Field> = ["setup", "tools", "tool"]
        .into_iter()
        .filter_map(keep)
        .collect();
    fields.extend([
        Field::opt_expr("rpm", "出力(%)").plain("%").help("0〜100。空なら工具の切削条件の値(レーザーの工具では回転数の欄が出力)"),
        Field::opt_expr("feed", "速さ(mm/min)").length().help("焼く時に動く速さ。空なら工具の切削条件の送り"),
        Field::output("curves", "焼く線・形", "curves"),
        Field::choice(
            "mode",
            "焼き方",
            &[
                ("line", "線(なぞる)"),
                ("fill", "塗り(中を埋める)"),
                ("fill_and_line", "塗りと縁"),
            ],
        )
        .help("線: スケッチの線の上をなぞる。塗り: 閉じた線の中を平行な線で埋める(文字の穴は焼かない)。塗りと縁: 埋めてから縁をなぞる"),
        Field::integer("passes", "回数").unitless().help("同じ所をくり返して焼く回数(深く焼く・薄い物を切る時に増やす)"),
        Field::expr("focus", "焦点のずらし").length().help("材料の表面から焼く高さまで(上が正)。0 なら表面に焦点を合わせる。ぼかして太く焼く時は上へずらす"),
        Field::expr("interval", "線の間隔").length()
            .help("塗りの平行な線の間隔。光の点の径くらいが目安(0.1 mm など)")
            .unless("mode", "line"),
        Field::expr("angle", "線の角度").angle()
            .help("塗りの線の向き(X から反時計回り)")
            .unless("mode", "line"),
        Field::bool("both_ways", "往復で埋める")
            .help("行ごとに向きを替えて速く埋める。行の始まりがずれて見える機械では外す")
            .unless("mode", "line"),
        Field::bool("cross", "直角にも焼く")
            .help("同じ形を、線の角度を 90° 替えてもう一度焼く(むらを減らす)")
            .unless("mode", "line"),
        Field::opt_expr("overscan", "はみ出し").length()
            .help("行の両端の外へ、出力 0 のまま同じ速さで通る長さ。加減速の間に焼かないので、縁が濃くならない。空なら機械の加速度と速さから決める(分からなければ 2 mm)")
            .unless("mode", "line"),
        Field::choice(
            "kerf_side",
            "切り口の補正",
            &[
                ("none", "なし(線の中心を焼く)"),
                ("outside", "部品を残す(外へずらす)"),
                ("inside", "穴をあける(内へずらす)"),
            ],
        )
        .help("閉じた線を切り口の幅の半分だけずらし、仕上がりを描いた寸法に合わせる。部品を残す: 外周は外へ、中の穴は内へ。穴をあける: 線の内側を抜いて捨てる。開いた線はずらさない")
        .unless("mode", "fill"),
        Field::opt_expr("kerf", "切り口の幅").length()
            .help("焼いて消える幅。空なら工具の径(光の点の径)。試し切りの外寸と内寸の差の半分くらいが目安")
            .unless("kerf_side", "none"),
        Field::choice(
            "lead_in",
            "進入",
            &[("none", "なし"), ("line", "直線"), ("arc", "円弧")],
        )
        .help("閉じた線を焼き始める前に、捨てる側から入る。焼き始めの太い跡や貫通の穴が形に残らない。焼き始めは最も長い辺の中ほど")
        .unless("mode", "fill"),
        Field::opt_expr("lead_in_length", "進入の長さ").length()
            .help("直線の長さ、円弧なら半径。空なら 1 mm")
            .unless("lead_in", "none"),
        Field::opt_expr("lead_in_sweep", "進入の掃引角").angle()
            .help("円弧の角度(0 より大きく 180 度まで)。空なら 90 度")
            .only("lead_in", "arc", "none"),
        Field::opt_expr("lead_in_line", "進入の直線").length()
            .help("円弧の前に足す直線(円弧の始めに接する)。空なら 0")
            .only("lead_in", "arc", "none"),
        Field::choice(
            "lead_out",
            "退出",
            &[("none", "なし"), ("line", "直線"), ("arc", "円弧")],
        )
        .help("閉じた線を焼き終えた後に、捨てる側へ出る")
        .unless("mode", "fill"),
        Field::opt_expr("lead_out_length", "退出の長さ").length()
            .help("直線の長さ、円弧なら半径。空なら 0.5 mm")
            .unless("lead_out", "none"),
        Field::opt_expr("lead_out_sweep", "退出の掃引角").angle()
            .help("円弧の角度(0 より大きく 180 度まで)。空なら 90 度")
            .only("lead_out", "arc", "none"),
        Field::opt_expr("lead_out_line", "退出の直線").length()
            .help("円弧の後に足す直線(円弧の終わりに接する)。空なら 0")
            .only("lead_out", "arc", "none"),
        Field::opt_expr("overcut", "重ね切り").length()
            .help("閉じた線の焼き始めを越えて焼く長さ(最後の回だけ)。始めと終わりのつなぎ目が切れ残らない。空なら 0")
            .unless("mode", "fill"),
        Field::number("pierce_time", "貫通の時間(秒)").plain("秒")
            .help("焼き始めの所で止まったまま焼く時間。厚い物を切る時に。0 なら止まらない。動く間だけ焼く機械(Carvera Air など)では使えない")
            .unless("mode", "fill"),
        Field::opt_expr("pierce_height", "貫通の高さ").length()
            .help("焼く高さから上へずらして貫通し、焼きながら下りる(焦点をぼかして穴を広げる)。空なら焼く高さのまま")
            .unless("mode", "fill"),
        Field::opt_expr("pierce_clearance", "貫通クリアランス").length()
            .help("焼き始めの穴を、進入の始めからさらに捨てる側へ離す長さ(進入の始めの直線に足す)。進入がなしでも、この長さの直線で入る。空なら 0")
            .unless("mode", "fill"),
    ]);
    fields.extend(
        kmb_cam_setup::link_height_fields()
            .into_iter()
            .filter(|f| f.key.starts_with("safe")),
    );
    reg.register_described(
        Descriptor::new(LASER, category::OPERATION, "レーザー")
            .parent("setup")
            .hint("スケッチの線をレーザーでなぞって焼く・閉じた形の中を埋めて焼く。焼き印・目盛り・薄い物の切り抜きに")
            .fields(fields)
            .icon("engrave"),
        |d| Ok(Box::new(kmb_cam_setup::parse::<Laser>(d)?)),
    );
}

/// この crate の名乗り: 登録する種類と、加工を足す操作(画面の名前・まとまり・初めの値)。
/// レーザーは削らない加工なので、彫刻とは別の crate に置く。
pub fn module() -> kmb_cam_setup::StrategyModule {
    use kmb_cam_setup::{Needs, Strategy, StrategyModule, to_data};
    use kmb_cam_tool::ToolKind;
    StrategyModule::new(
        "kmb-strategy-laser",
        register,
        vec![
            Strategy::new(LASER, "cam.add.laser", "レーザー", |p| {
                to_data(Laser::new(
                    p.with_tool(ToolKind::Laser),
                    p.source("curves").ok_or("焼く線がありません")?,
                ))
            })
            .order(11)
            .icon("engrave")
            .not_front()
            .aliases("れーざー laser やきいん burn せんをやく ぬり fill")
            .hint("スケッチの線をレーザーでなぞる・閉じた形の中を埋めて焼く。工具ライブラリにレーザーの工具が要る。スケッチを選ぶとその線、選ばなければ最後のスケッチ")
            .needs(Needs::Curves),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x0: f64, y0: f64, w: f64) -> Vec<P2> {
        vec![[x0, y0], [x0 + w, y0], [x0 + w, y0 + w], [x0, y0 + w]]
    }

    #[test]
    fn the_hatch_fills_inside_and_skips_holes() {
        let rows = hatch(&[square(0.0, 0.0, 10.0), square(4.0, 4.0, 2.0)], 1.0, 0.0);
        assert_eq!(rows.len(), 10, "{rows:?}");
        // 行は形の中に収まり、余りは両側に分ける(0.5 から 9.5)
        assert!((rows[0][0][0][1] - 0.5).abs() < 1e-9);
        assert!((rows[9][0][0][1] - 9.5).abs() < 1e-9);
        // 穴を通る行は 2 区間
        let mid = &rows[4];
        assert_eq!(mid.len(), 2, "{mid:?}");
        assert!((mid[0][1][0] - 4.0).abs() < 1e-9 && (mid[1][0][0] - 6.0).abs() < 1e-9);
    }

    #[test]
    fn the_hatch_turns_with_the_angle() {
        let rows = hatch(&[square(0.0, 0.0, 10.0)], 1.0, std::f64::consts::FRAC_PI_2);
        assert_eq!(rows.len(), 10);
        // 90° なら線は Y に沿う
        let s = rows[0][0];
        assert!((s[0][0] - s[1][0]).abs() < 1e-9, "{s:?}");
        assert!((s[1][1] - s[0][1]).abs() > 9.0);
    }

    #[test]
    fn rows_go_both_ways_and_overscan_is_a_link_at_the_same_speed() {
        let rows = hatch(&[square(0.0, 0.0, 10.0)], 5.0, 0.0);
        assert_eq!(rows.len(), 2);
        let lib = kmb_cam_tool::ToolLibrary::starter();
        let mut tp = Toolpath {
            name: "焼く".into(),
            tool: lib.tools[0].clone(),
            rpm: 50.0,
            safe_z: 5.0,
            retract_z: 1.0,
            edge_break: 0.0,
            dwells: vec![],
            spans: vec![],
            links: Default::default(),
            moves: vec![],
        };
        burn_rows(&mut tp, &rows, 0.0, 1200.0, 2.0, true);
        let feeds: Vec<P2> = tp
            .moves
            .iter()
            .filter(|m| matches!(m, Move::Linear { .. }))
            .map(|m| [m.to()[0], m.to()[1]])
            .collect();
        // 退避高さから -2 の焦点の高さへ出力 0 で下り、0 から 10 まで焼き、12 へはみ出す。
        // 次の行(近いので焦点の高さのまま出力 0 で移る)は逆向き
        let xs: Vec<f64> = feeds.iter().map(|p| p[0]).collect();
        assert_eq!(xs, [-2.0, 0.0, 10.0, 12.0, 12.0, 10.0, 0.0, -2.0]);
        assert_eq!(feeds[4][1], 7.5);
        // 焼くのは 2 つの行だけ。ほかはどれも出力 0 の送り
        assert_eq!(tp.spans.len(), 6);
        assert!(tp.spans.iter().all(|s| s.role == Role::Link));
        // 焦点の高さでは早送りしない(退避高さより下へ早送りで下りない決まり)
        assert!(
            tp.moves
                .iter()
                .all(|m| !matches!(m, Move::Rapid { to } if to[2] < 1.0))
        );
    }

    #[test]
    fn arc_leads_touch_the_line_from_the_scrap_side() {
        let (p, t, n) = ([0.0, 0.0], [1.0, 0.0], [0.0, 1.0]);
        let arc = |l| LeadShape {
            kind: Lead::Arc,
            len: l,
            ..LeadShape::NONE
        };
        let inn = lead_in_points(p, t, n, arc(1.0));
        // 中心 (0, 1) の 1/4 円。(-1, 1) から入り、最後の点は p の手前で X に沿う
        assert!((inn[0][0] + 1.0).abs() < 1e-9 && (inn[0][1] - 1.0).abs() < 1e-9);
        let last = inn[inn.len() - 1];
        assert!(last[0] < 0.0 && last[1] > 0.0 && last[1] < 0.05, "{last:?}");
        let out = lead_out_points(p, t, n, arc(1.0));
        let end = out[out.len() - 1];
        assert!((end[0] - 1.0).abs() < 1e-9 && (end[1] - 1.0).abs() < 1e-9);
        let line = LeadShape {
            kind: Lead::Line,
            len: 2.0,
            ..LeadShape::NONE
        };
        assert_eq!(lead_in_points(p, t, n, line), vec![[0.0, 2.0]]);
    }

    /// 掃引角と円弧の外の直線: 半円の進入は p の真上の
    /// (0, 2) から入り、その前の直線は円弧の始めに接して -X の側から来る。退出も同じ形で出る。
    /// 貫通クリアランスは進入の始めの直線を延ばし、進入がなしなら p から捨てる側へ直角に入る。
    #[test]
    fn arc_leads_take_a_sweep_and_a_straight_part_and_clearance_pushes_the_pierce_out() {
        let (p, t, n) = ([0.0, 0.0], [1.0, 0.0], [0.0, 1.0]);
        let near = |a: P2, b: P2| (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-9;
        let half = LeadShape {
            kind: Lead::Arc,
            len: 1.0,
            sweep: std::f64::consts::PI,
            line: 3.0,
        };
        let inn = lead_in_points(p, t, n, half);
        // 直線の始め、円弧の始め (0, 2)、…、p の手前
        assert!(near(inn[0], [3.0, 2.0]), "{inn:?}");
        assert!(near(inn[1], [0.0, 2.0]), "{inn:?}");
        assert_eq!(inn.len(), 1 + 2 * ARC_STEPS);
        // 円弧の点はどれも捨てる側(y ≥ 0)で、中心 (0, 1) から半径 1
        assert!(
            inn[1..]
                .iter()
                .all(|q| q[1] >= -1e-9 && ((q[0]).hypot(q[1] - 1.0) - 1.0).abs() < 1e-9)
        );
        let out = lead_out_points(p, t, n, half);
        assert!(
            near(out[out.len() - 2], [0.0, 2.0]) && near(out[out.len() - 1], [-3.0, 2.0]),
            "{out:?}"
        );
        // 掃引角 90 度で直線なしなら前と同じ 1/4 円
        let quarter = LeadShape {
            kind: Lead::Arc,
            len: 1.0,
            ..LeadShape::NONE
        };
        assert!(near(lead_in_points(p, t, n, quarter)[0], [-1.0, 1.0]));
        // 貫通クリアランス
        let line = LeadShape {
            kind: Lead::Line,
            len: 2.0,
            ..LeadShape::NONE
        };
        let inn = |s: LeadShape, c| lead_in_points(p, t, n, s.with_clearance(c));
        assert_eq!(inn(line, 1.5), vec![[0.0, 3.5]]);
        assert_eq!(inn(LeadShape::NONE, 1.5), vec![[0.0, 1.5]]);
        let q = inn(quarter, 2.0);
        assert!(
            near(q[0], [-1.0, 3.0]),
            "1/4 円の始めの向き(-Y)の逆へ延ばす: {q:?}"
        );
    }

    #[test]
    fn the_start_moves_to_the_middle_of_the_longest_edge_and_overcut_walks_on() {
        let l = start_mid_longest(vec![[0.0, 0.0], [1.0, 0.0], [1.0, 4.0], [0.0, 4.0]]);
        // 同じ長さの辺なら後の辺
        assert_eq!(
            l,
            vec![[0.0, 2.0], [0.0, 0.0], [1.0, 0.0], [1.0, 4.0], [0.0, 4.0]]
        );
        let mut closed = l.clone();
        closed.push(l[0]);
        let (pts, t) = walk(&closed, 3.0);
        assert_eq!(pts, vec![[0.0, 0.0], [1.0, 0.0]]);
        assert_eq!(t, [1.0, 0.0]);
    }

    #[test]
    fn the_overscan_follows_the_machine_acceleration() {
        // 1500 mm/min = 25 mm/s、150 mm/s² なら 25²/300 ≈ 2.08 mm
        assert!((auto_overscan(1500.0, 150.0) - 625.0 / 300.0).abs() < 1e-9);
        assert_eq!(auto_overscan(1500.0, 0.0), OVERSCAN);
        assert_eq!(auto_overscan(60000.0, 100.0), 10.0);
    }
}
