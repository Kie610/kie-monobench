//! 入り方(材料へ下りる動き)、リード(壁に沿い始める・離れる動き)、仕上げの周回。加工に共通。
//!
//! 入り方は [`EntryKind`] で選ぶ。らせんは G2・G3 の円弧(Z が変わる円弧)で出し、
//! 下りきった高さでもう 1 周して底を平らにする。斜めは輪の上を下りながら回り、
//! 開いた線では行って戻りながら下りる。どの入り方も、下りる送りは「ランプの送り」
//! (空なら切削の送り)、真下と下穴は下降の送りにする。
//!
//! リードは円弧(角度は 0 より大きく 180° まで)と、その前後の直線で作る。入りと抜けは
//! 同じ形にする。工具の中心が入ってよい側だけを通り、入らなければ半径を半分ずつ小さくする。

use crate::{Cutter, invalid, length, loop_length, seg_length};
use jc_cam_geom::{Loop, P2, Seg, Vertex};
use jc_core::{Dim, EvalCtx, EvalError, Field};
use jc_toolpath::{ArcDir, Move, P3};
use serde::{Deserialize, Serialize};
use std::f64::consts::{FRAC_PI_2, PI, TAU};

/// 入り方。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    /// 加工ごとの既定(ポケットは入る位置があればらせん、なければ輪の上を斜め。
    /// 輪郭は斜めに入る角度があれば輪郭の上を斜め、なければ真下。負荷一定はらせん)
    #[default]
    Auto,
    /// らせんで下りる
    Helix,
    /// 輪(輪郭)の上を下りながら回る
    Ramp,
    /// 真下に下りる(底で削れる工具だけ)
    Plunge,
    /// 先に開けた下穴の上で真下に下りる(入る位置が要る)
    Predrill,
    /// ストックの外で下りて、横から入る
    Outside,
}

impl EntryKind {
    pub fn is_auto(&self) -> bool {
        *self == EntryKind::Auto
    }

    pub fn label(&self) -> &'static str {
        match self {
            EntryKind::Auto => "自動",
            EntryKind::Helix => "らせん",
            EntryKind::Ramp => "斜め(輪に沿って)",
            EntryKind::Plunge => "真下",
            EntryKind::Predrill => "下穴",
            EntryKind::Outside => "ストックの外から",
        }
    }

    fn value(&self) -> &'static str {
        match self {
            EntryKind::Auto => "auto",
            EntryKind::Helix => "helix",
            EntryKind::Ramp => "ramp",
            EntryKind::Plunge => "plunge",
            EntryKind::Predrill => "predrill",
            EntryKind::Outside => "outside",
        }
    }
}

/// 入り方の設定(加工のデータに平らに入る)。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct EntryOpts {
    #[serde(default, skip_serializing_if = "EntryKind::is_auto")]
    pub entry_kind: EntryKind,
    /// らせんの直径(工具の中心が回る円)。空なら工具径の半分
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub helix_diameter: Option<String>,
    /// らせんと斜めで下りる送り(mm/min)。空なら切削の送り
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ramp_feed: Option<String>,
}

/// 入り方の決まった値。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EntryPlan {
    pub kind: EntryKind,
    /// 斜めとらせんの角度(ラジアン)
    pub angle: f64,
    /// らせんの半径(工具の中心の円)
    pub helix_r: f64,
    /// らせんと斜めの送り
    pub ramp_feed: f64,
    /// 真下と下穴の送り
    pub plunge_feed: f64,
}

/// らせんと斜めの角度を決めない時(度)。
pub const DEFAULT_ENTRY_DEG: f64 = 2.0;

impl EntryOpts {
    pub fn exprs(&self) -> impl Iterator<Item = &String> {
        [&self.helix_diameter, &self.ramp_feed]
            .into_iter()
            .flatten()
    }

    pub fn exprs_mut(&mut self) -> impl Iterator<Item = &mut String> {
        [&mut self.helix_diameter, &mut self.ramp_feed]
            .into_iter()
            .flatten()
    }

    /// 値を読む。`angle` は加工の「ランプ角度」(空なら [`DEFAULT_ENTRY_DEG`])。
    pub fn read(
        &self,
        ctx: &EvalCtx<'_>,
        p: &impl Cutter,
        angle: Option<f64>,
    ) -> Result<EntryPlan, EvalError> {
        let helix_r = match &self.helix_diameter {
            Some(s) => {
                let d = length(ctx, s, "らせんの直径")?;
                if !(d > 0.0 && d.is_finite()) {
                    return Err(invalid("らせんの直径は 0 より大きくしてください"));
                }
                d / 2.0
            }
            None => 0.5 * p.radius(),
        };
        let ramp_feed = match &self.ramp_feed {
            Some(s) => feed_value(ctx, s, "ランプの送り")?,
            None => p.feed(),
        };
        Ok(EntryPlan {
            kind: self.entry_kind,
            angle: angle.unwrap_or(DEFAULT_ENTRY_DEG.to_radians()),
            helix_r,
            ramp_feed,
            plunge_feed: p.plunge(),
        })
    }
}

/// 送りの式(mm/min。単位なしの数か、長さ/分として読む)。0 より大きい。
pub fn feed_value(ctx: &EvalCtx<'_>, src: &str, what: &str) -> Result<f64, EvalError> {
    let q = ctx.expr(src)?;
    if !(q.dim == Dim::NONE || q.dim == Dim::LENGTH) {
        return Err(invalid(format!(
            "{what} `{src}` の単位が合いません(値: {q})"
        )));
    }
    if !(q.value > 0.0 && q.value.is_finite()) {
        return Err(invalid(format!("{what}は 0 より大きくしてください")));
    }
    Ok(q.value)
}

/// 入り方の欄。`kinds` はその加工で選べる入り方(自動を含める)。
pub fn entry_fields(kinds: &[EntryKind]) -> Vec<Field> {
    let choices: Vec<(&str, &str)> = kinds.iter().map(|k| (k.value(), k.label())).collect();
    vec![
        Field::choice("entry_kind", "進入方法", &choices).help(
            "材料へ下りる動き。らせん: 入る位置のまわりを回りながら下りる。斜め: 輪の上を下りながら回る。\
             真下: そのまま下りる(底で削れる工具だけ)。下穴: 先に開けた穴の上で下りる(入る位置を下穴の中心に置く)。\
             ストックの外から: 材料の外で下りて横から入る",
        ),
        Field::opt_expr("helix_diameter", "らせんの直径").length()
            .help("工具の中心が回る円の直径。例: 3 mm。空なら工具径の半分。入らなければ小さくし、それでも入らなければ真下に下りる")
            .only("entry_kind", "helix", "auto"),
        Field::opt_expr("ramp_feed", "ランプ送り(mm/min)").length()
            .help("らせんと斜めで下りる時の送り。空なら切削の送り"),
    ]
}

/// リードの形の設定(加工のデータに平らに入る)。半径は各加工の `lead`。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct LeadOpts {
    /// 円弧の角度。空なら 90°
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead_angle: Option<String>,
    /// 円弧の前(抜けでは後)の直線の長さ。空なら直線なし
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead_line: Option<String>,
    /// 入りと抜けの送り(mm/min)。空なら切削の送り
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead_feed: Option<String>,
}

/// リードの決まった形。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LeadShape {
    pub radius: f64,
    /// 円弧の角度(ラジアン)
    pub sweep: f64,
    pub line: f64,
    pub feed: f64,
}

impl LeadOpts {
    pub fn exprs(&self) -> impl Iterator<Item = &String> {
        [&self.lead_angle, &self.lead_line, &self.lead_feed]
            .into_iter()
            .flatten()
    }

    pub fn exprs_mut(&mut self) -> impl Iterator<Item = &mut String> {
        [
            &mut self.lead_angle,
            &mut self.lead_line,
            &mut self.lead_feed,
        ]
        .into_iter()
        .flatten()
    }

    /// 半径 `radius`(空ならリードなし)と合わせて読む。
    pub fn read(
        &self,
        ctx: &EvalCtx<'_>,
        p: &impl Cutter,
        radius: &Option<String>,
    ) -> Result<Option<LeadShape>, EvalError> {
        let Some(r) = radius else {
            return Ok(None);
        };
        let radius = length(ctx, r, "リードの円弧の半径")?;
        if radius <= 0.0 {
            return Err(invalid("入りと抜けの円弧の半径は 0 より大きくしてください"));
        }
        let sweep = match &self.lead_angle {
            Some(s) => {
                let q = ctx.expr(s)?;
                if q.dim != Dim::ANGLE {
                    return Err(invalid(format!(
                        "リードの円弧の角度 `{s}` は角度の単位(deg)が必要です(値: {q})"
                    )));
                }
                if !(q.value > 1e-6 && q.value <= PI + 1e-9) {
                    return Err(invalid(
                        "リードの円弧の角度は 0 より大きく 180 deg 以下にしてください",
                    ));
                }
                q.value
            }
            None => FRAC_PI_2,
        };
        let line = match &self.lead_line {
            Some(s) => length(ctx, s, "リードの直線の長さ")?,
            None => 0.0,
        };
        if line < 0.0 {
            return Err(invalid("リードの直線の長さは 0 以上にしてください"));
        }
        let feed = match &self.lead_feed {
            Some(s) => feed_value(ctx, s, "入りと抜けの送り")?,
            None => p.feed(),
        };
        Ok(Some(LeadShape {
            radius,
            sweep,
            line,
            feed,
        }))
    }
}

/// リードの形の欄(半径の欄 `lead` の後に並べる)。
pub fn lead_shape_fields() -> Vec<Field> {
    vec![
        Field::opt_expr("lead_angle", "リードの円弧の角度")
            .angle()
            .help("例: 45 deg。空なら 90 deg(4 分の 1 周)。小さいほど壁から離れる量が小さい"),
        Field::opt_expr("lead_line", "リードの直線の長さ")
            .length()
            .help("例: 1 mm。円弧の手前(抜けでは後)に足す直線。空なら直線なし"),
        Field::opt_expr("lead_feed", "リードの送り(mm/min)")
            .length()
            .help("空なら切削の送り"),
    ]
}

/// 仕上げの周回の設定(加工のデータに平らに入る)。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct FinishOpts {
    /// 仕上げの周回の数(仕上げ代を等分する)。0 か 1 なら 1 周
    #[serde(default, skip_serializing_if = "is_le_one")]
    pub finish_passes: u32,
    /// 仕上げの送り(mm/min)。空なら切削の送り
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_feed: Option<String>,
    /// 最後の仕上げを同じ所でもう 1 周する(工具のたわみで残った分を削る)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub spring_pass: bool,
    /// 仕上げの周回の終わりで、始めの所を越えて重ねる長さ。空なら重ねない
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlap: Option<String>,
}

fn is_le_one(v: &u32) -> bool {
    *v <= 1
}

/// 仕上げの周回の決まった値。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FinishPlan {
    pub passes: u32,
    pub feed: f64,
    pub spring: bool,
    pub overlap: f64,
}

impl FinishOpts {
    pub fn exprs(&self) -> impl Iterator<Item = &String> {
        [&self.finish_feed, &self.overlap].into_iter().flatten()
    }

    pub fn exprs_mut(&mut self) -> impl Iterator<Item = &mut String> {
        [&mut self.finish_feed, &mut self.overlap]
            .into_iter()
            .flatten()
    }

    pub fn read(&self, ctx: &EvalCtx<'_>, p: &impl Cutter) -> Result<FinishPlan, EvalError> {
        let feed = match &self.finish_feed {
            Some(s) => feed_value(ctx, s, "仕上げの送り")?,
            None => p.feed(),
        };
        let overlap = match &self.overlap {
            Some(s) => length(ctx, s, "仕上げの重ね")?,
            None => 0.0,
        };
        if overlap < 0.0 {
            return Err(invalid("仕上げの重ねは 0 以上にしてください"));
        }
        if self.finish_passes > 50 {
            return Err(invalid("仕上げの周回は 50 以下にしてください"));
        }
        Ok(FinishPlan {
            passes: self.finish_passes.max(1),
            feed,
            spring: self.spring_pass,
            overlap,
        })
    }
}

/// 仕上げの周回の欄(仕上げ代の欄の後に並べる)。
pub fn finish_fields() -> Vec<Field> {
    vec![
        Field::integer("finish_passes", "仕上げパスの数")
            .unitless()
            .help("仕上げ代を等分して、外から順に回る。0 か 1 なら 1 周"),
        Field::opt_expr("finish_feed", "仕上げ送り(mm/min)")
            .length()
            .help("空なら切削の送り"),
        Field::bool("spring_pass", "最後の仕上げをもう 1 周")
            .help("同じ所をもう 1 周して、工具のたわみで残った分を削る"),
        Field::opt_expr("overlap", "仕上げの重ね").length().help(
            "例: 1 mm。仕上げの周回の終わりで、始めの所を越えて重ねてから離れる(継ぎ目の跡を消す)",
        ),
    ]
}

/// 中心 `c`、半径 `r` の円の上で、角 `a` の点(高さ `z`)。
fn on_circle(c: P2, r: f64, a: f64, z: f64) -> P3 {
    [c[0] + r * a.cos(), c[1] + r * a.sin(), z]
}

/// らせんの始まりの点(角 `a0`、高さ `z`)。工具をここへ動かしてから [`helix_moves`] を足す。
pub fn helix_start(c: P2, r: f64, a0: f64, z: f64) -> P3 {
    on_circle(c, r, a0, z)
}

/// 中心 `c`、半径 `r` のらせんで、高さ `from` から `to` まで傾き `angle` で下り、`to` で
/// もう 1 周して底を平らにする。工具は [`helix_start`] にいるとする。半周ごとの円弧(G2・G3)で出す。
/// 反時計回り(`ccw`)は、内側を削る時のダウンカット。終わりは始まりの真下。
pub fn helix_moves(
    c: P2,
    r: f64,
    a0: f64,
    (from, to): (f64, f64),
    angle: f64,
    feed: f64,
    ccw: bool,
) -> Vec<Move> {
    let drop = from - to;
    if r < 1e-6 || drop <= 1e-9 {
        return vec![];
    }
    let pitch = TAU * r * angle.tan();
    let halves = 2 * ((drop / pitch.max(1e-9)) - 1e-9).ceil().max(1.0) as usize;
    let (dir, s) = if ccw {
        (ArcDir::Ccw, 1.0)
    } else {
        (ArcDir::Cw, -1.0)
    };
    let mut out = Vec::with_capacity(halves + 2);
    for k in 1..=halves + 2 {
        let z = if k <= halves {
            from - drop * k as f64 / halves as f64
        } else {
            to
        };
        out.push(Move::Arc {
            to: on_circle(c, r, a0 + s * PI * k as f64, z),
            center: c,
            dir,
            feed,
        });
    }
    out
}

/// 開いた線 `pts` の上を、高さ `from` から `to` まで傾き `angle` で行って戻りながら下り、
/// 下りきったら始めの点へ `to` の高さで戻る(下りた跡の斜面を削る)。工具は `pts[0]` の `from` にいるとする。
/// 線がほとんど長さを持たなければ真下に下げる(`plunge` の送り)。
pub fn zigzag_moves(
    pts: &[P2],
    (from, to): (f64, f64),
    angle: f64,
    feed: f64,
    plunge: f64,
) -> Vec<Move> {
    let drop = from - to;
    if drop <= 1e-9 || pts.is_empty() {
        return vec![];
    }
    let len: f64 = pts
        .windows(2)
        .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
        .sum();
    if len < 1e-3 {
        return vec![Move::Linear {
            to: [pts[0][0], pts[0][1], to],
            feed: plunge,
        }];
    }
    let slope = angle.tan();
    let mut out = Vec::new();
    let mut z = from;
    // 行って戻る点の並び(下りきるまで)
    let mut path: Vec<P2> = Vec::new();
    let mut forward = true;
    let mut cur = pts[0];
    'outer: loop {
        let seq: Vec<P2> = if forward {
            pts[1..].to_vec()
        } else {
            pts[..pts.len() - 1].iter().rev().copied().collect()
        };
        for q in seq {
            let run = (q[0] - cur[0]).hypot(q[1] - cur[1]);
            let dz = run * slope;
            if z - dz <= to + 1e-9 {
                // この区間の途中で下りきる
                let t = if dz > 1e-12 { (z - to) / dz } else { 1.0 };
                let m = [cur[0] + (q[0] - cur[0]) * t, cur[1] + (q[1] - cur[1]) * t];
                out.push(Move::Linear {
                    to: [m[0], m[1], to],
                    feed,
                });
                path.push(cur);
                cur = m;
                break 'outer;
            }
            z -= dz;
            out.push(Move::Linear {
                to: [q[0], q[1], z],
                feed,
            });
            path.push(cur);
            cur = q;
        }
        forward = !forward;
    }
    // 始めの点へ、来た道を戻る
    for q in path.into_iter().rev() {
        if (q[0] - cur[0]).hypot(q[1] - cur[1]) > 1e-9 {
            out.push(Move::Linear {
                to: [q[0], q[1], to],
                feed,
            });
            cur = q;
        }
    }
    if (pts[0][0] - cur[0]).hypot(pts[0][1] - cur[1]) > 1e-9 {
        out.push(Move::Linear {
            to: [pts[0][0], pts[0][1], to],
            feed,
        });
    }
    out
}

/// 区間の上で、始点から `t`(0〜1、長さの比)の点。
fn seg_point(s: &Seg, t: f64) -> P2 {
    match *s {
        Seg::Line { a, b } => [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t],
        Seg::Arc { a, b, center, ccw } => {
            let r = (a[0] - center[0]).hypot(a[1] - center[1]);
            let sweep = jc_cam_geom::sweep_of(a, b, center, ccw);
            let a0 = (a[1] - center[1]).atan2(a[0] - center[0]);
            let th = a0 + sweep * t;
            [center[0] + r * th.cos(), center[1] + r * th.sin()]
        }
    }
}

/// 点 `p` に最も近い区間の上の点の、長さの比 `t` と距離。
fn seg_project(s: &Seg, p: P2) -> (f64, f64) {
    match *s {
        Seg::Line { a, b } => {
            let d = [b[0] - a[0], b[1] - a[1]];
            let l2 = d[0] * d[0] + d[1] * d[1];
            let t = if l2 > 0.0 {
                (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let q = seg_point(s, t);
            (t, (q[0] - p[0]).hypot(q[1] - p[1]))
        }
        Seg::Arc { a, b, center, ccw } => {
            let sweep = jc_cam_geom::sweep_of(a, b, center, ccw);
            let a0 = (a[1] - center[1]).atan2(a[0] - center[0]);
            let ap = (p[1] - center[1]).atan2(p[0] - center[0]);
            let mut rel = ap - a0;
            if sweep >= 0.0 {
                rel = rel.rem_euclid(TAU);
            } else {
                rel = -(-rel).rem_euclid(TAU);
            }
            let t = if sweep.abs() > 1e-12 {
                rel / sweep
            } else {
                0.0
            };
            let mut best = (0.0, f64::INFINITY);
            for t in [t, 0.0, 1.0] {
                if (0.0..=1.0).contains(&t) {
                    let q = seg_point(s, t);
                    let d = (q[0] - p[0]).hypot(q[1] - p[1]);
                    if d < best.1 {
                        best = (t, d);
                    }
                }
            }
            best
        }
    }
}

/// 輪郭の区間 `k` の長さの比 `t` の所で輪郭を割り、そこから始まる輪郭にする。
fn split_start(l: &Loop, k: usize, t: f64) -> Loop {
    let n = l.verts.len();
    let segs = l.segments();
    let s = segs[k];
    if t <= 1e-9 {
        return Loop {
            verts: (0..n).map(|i| l.verts[(k + i) % n]).collect(),
        };
    }
    if t >= 1.0 - 1e-9 {
        return Loop {
            verts: (0..n).map(|i| l.verts[(k + 1 + i) % n]).collect(),
        };
    }
    let q = seg_point(&s, t);
    let (b1, b2) = match s {
        Seg::Line { .. } => (0.0, 0.0),
        Seg::Arc { .. } => {
            let total = 4.0 * l.verts[k].bulge.atan();
            ((total * t / 4.0).tan(), (total * (1.0 - t) / 4.0).tan())
        }
    };
    let mut verts = Vec::with_capacity(n + 1);
    verts.push(Vertex {
        x: q[0],
        y: q[1],
        bulge: b2,
    });
    for i in 1..n {
        verts.push(l.verts[(k + i) % n]);
    }
    let mut last = l.verts[k];
    last.bulge = b1;
    verts.push(last);
    Loop { verts }
}

/// 輪郭の始まりを、点 `p` に最も近い輪郭の上の点にする。
pub fn start_at(l: &Loop, p: P2) -> Loop {
    let segs = l.segments();
    let Some((k, t)) = segs
        .iter()
        .enumerate()
        .map(|(k, s)| (k, seg_project(s, p)))
        .min_by(|a, b| a.1.1.total_cmp(&b.1.1))
        .map(|(k, (t, _))| (k, t))
    else {
        return l.clone();
    };
    split_start(l, k, t)
}

/// 輪郭の始まりを、始まりから長さ `d` だけ進んだ点にする(1 周より長ければ回る)。
pub fn start_after(l: &Loop, d: f64) -> Loop {
    let per = loop_length(l);
    if per < 1e-9 {
        return l.clone();
    }
    let mut d = d.rem_euclid(per);
    for (k, s) in l.segments().iter().enumerate() {
        let len = seg_length(s);
        if d <= len {
            return split_start(l, k, if len > 0.0 { d / len } else { 0.0 });
        }
        d -= len;
    }
    l.clone()
}

/// 輪郭の始まりから長さ `d` まで、高さ `z` で進む動き(1 周より長ければ 1 周で止める)。
pub fn walk_moves(l: &Loop, d: f64, z: f64, feed: f64) -> Vec<Move> {
    let mut left = d.min(loop_length(l));
    let mut out = Vec::new();
    for s in l.segments() {
        if left <= 1e-9 {
            break;
        }
        let len = seg_length(&s);
        let t = if len > 0.0 {
            (left / len).min(1.0)
        } else {
            1.0
        };
        let q = seg_point(&s, t);
        out.push(match s {
            Seg::Line { .. } => Move::Linear {
                to: [q[0], q[1], z],
                feed,
            },
            Seg::Arc { center, ccw, .. } => Move::Arc {
                to: [q[0], q[1], z],
                center,
                dir: if ccw { ArcDir::Ccw } else { ArcDir::Cw },
                feed,
            },
        });
        left -= len;
    }
    out
}

/// 輪郭の始まりの入りと、終わりの抜け(円弧と直線)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Leads {
    /// 円弧の半径(入らなければ小さくした値)
    pub radius: f64,
    /// 入りの始まり(ここで下りる)
    pub start: P2,
    /// 抜けの終わり
    pub end: P2,
    arc_in_start: P2,
    arc_out_end: P2,
    center_in: P2,
    center_out: P2,
    /// 輪郭の始まり(= 終わり)
    on: P2,
    dir: ArcDir,
}

impl Leads {
    /// 入りの動き(`start` から直線で円弧の始まりへ、円弧で輪郭の始まりへ。高さ `z`)。
    pub fn lead_in(&self, z: f64, feed: f64) -> Vec<Move> {
        let mut out = Vec::new();
        if (self.arc_in_start[0] - self.start[0]).hypot(self.arc_in_start[1] - self.start[1]) > 1e-9
        {
            out.push(Move::Linear {
                to: [self.arc_in_start[0], self.arc_in_start[1], z],
                feed,
            });
        }
        out.push(Move::Arc {
            to: [self.on[0], self.on[1], z],
            center: self.center_in,
            dir: self.dir,
            feed,
        });
        out
    }

    /// 抜けの動き(輪郭の終わりから円弧で離れ、直線で `end` へ。高さ `z`)。
    pub fn lead_out(&self, z: f64, feed: f64) -> Vec<Move> {
        let mut out = vec![Move::Arc {
            to: [self.arc_out_end[0], self.arc_out_end[1], z],
            center: self.center_out,
            dir: self.dir,
            feed,
        }];
        if (self.end[0] - self.arc_out_end[0]).hypot(self.end[1] - self.arc_out_end[1]) > 1e-9 {
            out.push(Move::Linear {
                to: [self.end[0], self.end[1], z],
                feed,
            });
        }
        out
    }
}

/// 区間の始まり(`at_end` なら終わり)での進む向き(長さ 1)。
pub(crate) fn tangent(s: &Seg, at_end: bool) -> Option<P2> {
    let t = match *s {
        Seg::Line { a, b } => [b[0] - a[0], b[1] - a[1]],
        Seg::Arc { a, b, center, ccw } => {
            let q = if at_end { b } else { a };
            let u = [q[0] - center[0], q[1] - center[1]];
            if ccw { [-u[1], u[0]] } else { [u[1], -u[0]] }
        }
    };
    let n = t[0].hypot(t[1]);
    (n > 1e-12).then(|| [t[0] / n, t[1] / n])
}

/// 輪郭 `l`(始点から 1 周)の入りと抜け。`free_left` は進む向きの左が入ってよい側か。
/// 円弧と直線の上の点がすべて `fits` を満たす半径を、`shape.radius` から半分ずつ 3 回まで小さくして
/// 探す(直線の長さも同じ比で縮める)。どれも入らなければ `None`。
pub fn leads(
    l: &Loop,
    shape: (f64, f64, f64),
    free_left: bool,
    fits: impl Fn(P2) -> bool,
) -> Option<Leads> {
    let (radius, sweep, line) = shape;
    let segs = l.segments();
    let t0 = tangent(segs.first()?, false)?;
    let t1 = tangent(segs.last()?, true)?;
    let on = [l.verts[0].x, l.verts[0].y];
    // 入ってよい側を向く法線
    let side = |t: P2| {
        if free_left {
            [-t[1], t[0]]
        } else {
            [t[1], -t[0]]
        }
    };
    let (n0, n1) = (side(t0), side(t1));
    let dir = if free_left { ArcDir::Ccw } else { ArcDir::Cw };
    let mut r = radius;
    let mut ln = line;
    for _ in 0..4 {
        let c_in = [on[0] + n0[0] * r, on[1] + n0[1] * r];
        let c_out = [on[0] + n1[0] * r, on[1] + n1[1] * r];
        // 円の上の点: 角 θ で、θ = 0 が輪郭の上、負が入りの側、正が抜けの側
        let at = |c: P2, n: P2, t: P2, th: f64| {
            [
                c[0] + r * (-n[0] * th.cos() + t[0] * th.sin()),
                c[1] + r * (-n[1] * th.cos() + t[1] * th.sin()),
            ]
        };
        // 円弧の端での進む向き
        let dir_at = |n: P2, t: P2, th: f64| {
            [
                n[0] * th.sin() + t[0] * th.cos(),
                n[1] * th.sin() + t[1] * th.cos(),
            ]
        };
        let a_in = at(c_in, n0, t0, -sweep);
        let a_out = at(c_out, n1, t1, sweep);
        let d_in = dir_at(n0, t0, -sweep);
        let d_out = dir_at(n1, t1, sweep);
        let s_in = [a_in[0] - d_in[0] * ln, a_in[1] - d_in[1] * ln];
        let e_out = [a_out[0] + d_out[0] * ln, a_out[1] + d_out[1] * ln];
        let ok = (1..=8).all(|k| {
            let th = sweep * k as f64 / 8.0;
            let u = k as f64 / 8.0;
            fits(at(c_in, n0, t0, -th))
                && fits(at(c_out, n1, t1, th))
                && fits([a_in[0] - d_in[0] * ln * u, a_in[1] - d_in[1] * ln * u])
                && fits([a_out[0] + d_out[0] * ln * u, a_out[1] + d_out[1] * ln * u])
        });
        if ok {
            return Some(Leads {
                radius: r,
                start: s_in,
                end: e_out,
                arc_in_start: a_in,
                arc_out_end: a_out,
                center_in: c_in,
                center_out: c_out,
                on,
                dir,
            });
        }
        r /= 2.0;
        ln /= 2.0;
    }
    None
}

/// 輪郭 `l` を高さ `z` で回る仕上げの 1 周(リードと重ねを含む)。工具は `down_at` の上へ来て、
/// 高さ `z` まで下りているとする(下りる所は [`lap_start`])。`leads` は `l` の入りと抜け、
/// `out_leads` は重ねた後の抜け(重ねがなければ `leads` と同じ)。
pub struct Lap<'a> {
    pub l: &'a Loop,
    pub z: f64,
    pub feed: f64,
    pub lead_feed: f64,
    pub leads: Option<Leads>,
    pub out_leads: Option<Leads>,
    pub overlap: f64,
}

/// 下りる所(リードがあれば入りの始まり、なければ輪郭の始まり)。
pub fn lap_start(l: &Loop, leads: Option<Leads>) -> P2 {
    leads.map_or([l.verts[0].x, l.verts[0].y], |a| a.start)
}

impl Lap<'_> {
    /// 動きを足し、それぞれの役目の印を付ける。
    pub fn push(&self, tp: &mut jc_toolpath::Toolpath) {
        use jc_toolpath::Role;
        if let Some(a) = self.leads {
            let s = tp.moves.len();
            tp.moves.extend(a.lead_in(self.z, self.lead_feed));
            tp.mark(s, Role::Lead);
        }
        tp.moves
            .extend(crate::loop_moves(self.l, self.z, self.feed));
        if self.overlap > 1e-9 {
            tp.moves
                .extend(walk_moves(self.l, self.overlap, self.z, self.feed));
        }
        if let Some(a) = self.out_leads {
            let s = tp.moves.len();
            tp.moves.extend(a.lead_out(self.z, self.lead_feed));
            tp.mark(s, Role::Lead);
        }
    }
}

/// 仕上げの周回の、壁からの距離(工具の中心が壁から離れる量)の列。外から順(最後が `last`)。
/// 仕上げ代 `finish` を `passes` 回に等分する。`spring` なら最後をもう 1 回。
pub fn finish_offsets(last: f64, finish: f64, passes: u32, spring: bool) -> Vec<f64> {
    let n = passes.max(1);
    let mut v: Vec<f64> = (1..=n)
        .map(|k| last + finish * (n - k) as f64 / n as f64)
        .collect();
    if spring {
        v.push(last);
    }
    v
}

/// 加工の出力の名前: 入る位置を置ける所。
pub const ENTRY_AREA: &str = "entry_area";

/// 入る位置を置ける所(工具の中心で。セットアップの座標)。入る位置を置く手順で、
/// ストックの上面の高さ `z` に塗って見せる。らせんなら、らせんが縮まずに入る所。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntryArea {
    pub z: f64,
    /// 輪(閉じた折れ線。最後の点は最初と同じ)。偶奇で塗る(穴は抜ける)
    pub rings: Vec<Vec<P2>>,
    /// らせんの範囲か(らせんなら true、真下・下穴・斜めなら false)
    pub helix: bool,
}

impl jc_core::Payload for EntryArea {
    const TYPE: &'static str = "cam.entry_area@1";
}

/// 入る位置を置ける所を、加工の出力に足す。
pub fn add_entry_area(
    out: &mut jc_core::Outputs,
    z: f64,
    areas: &[jc_cam_geom::Area],
    helix: bool,
) {
    let rings = areas
        .iter()
        .flat_map(|a| std::iter::once(&a.outer).chain(&a.holes))
        .map(|l| {
            let mut pts = l.flatten(0.05);
            if let Some(p) = pts.first().copied() {
                pts.push(p);
            }
            pts
        })
        .filter(|l| l.len() >= 4)
        .collect();
    let v = EntryArea { z, rings, helix };
    out.insert(ENTRY_AREA.into(), jc_core::Payload::to_value(&v));
}

/// 加工の出力から、入る位置を置ける所を読む。
pub fn entry_area_of(v: &jc_core::Value) -> Option<EntryArea> {
    jc_core::Payload::from_value(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn end(m: &[Move]) -> P3 {
        m.last().unwrap().to()
    }

    #[test]
    fn the_helix_goes_down_in_half_turn_arcs_and_flattens_the_floor() {
        let c = [5.0, 5.0];
        let a = 2f64.to_radians();
        let s = helix_start(c, 1.0, 0.0, 0.0);
        assert_eq!(s, [6.0, 5.0, 0.0]);
        let m = helix_moves(c, 1.0, 0.0, (0.0, -1.0), a, 300.0, true);
        // 1 周で 2π × tan 2° ≈ 0.219 mm 下がるので 5 周(10 の半周)、底でもう 1 周
        assert_eq!(m.len(), 12);
        assert!(m.iter().all(|x| matches!(
            x,
            Move::Arc {
                dir: ArcDir::Ccw,
                center: [5.0, 5.0],
                feed: 300.0,
                ..
            }
        )));
        let e = end(&m);
        assert!((e[0] - 6.0).abs() < 1e-9 && (e[1] - 5.0).abs() < 1e-9 && e[2] == -1.0);
        // 下りの傾きは角度より急でない
        let mut cur = s;
        for x in &m {
            let to = x.to();
            let run = PI * 1.0;
            assert!(cur[2] - to[2] <= run * a.tan() + 1e-9);
            cur = to;
        }
        // 時計回りも同じ点を通る
        let cw = helix_moves(c, 1.0, 0.0, (0.0, -1.0), a, 300.0, false);
        assert_eq!(cw.len(), 12);
        assert!(matches!(
            cw[0],
            Move::Arc {
                dir: ArcDir::Cw,
                ..
            }
        ));
    }

    #[test]
    fn the_zigzag_goes_back_and_forth_and_returns_to_the_start() {
        let pts = [[0.0, 0.0], [10.0, 0.0]];
        let a = 3f64.to_radians();
        let m = zigzag_moves(&pts, (0.0, -1.0), a, 200.0, 50.0);
        // 10 mm で 0.524 mm 下がるので、1 往復の途中で下りきり、始めの点に戻る
        let e = end(&m);
        assert_eq!(e, [0.0, 0.0, -1.0]);
        let mut cur = [0.0, 0.0, 0.0];
        for x in &m {
            let to = x.to();
            let run = (to[0] - cur[0]).hypot(to[1] - cur[1]);
            assert!(cur[2] - to[2] <= run * a.tan() + 1e-9, "{cur:?} → {to:?}");
            assert!(to[0] >= -1e-9 && to[0] <= 10.0 + 1e-9);
            cur = to;
        }
        assert!(m.iter().any(|x| x.to()[2] == -1.0 && x.to()[0] > 0.0));
        // 長さのない線は真下
        let m = zigzag_moves(&[[1.0, 1.0]], (0.0, -1.0), a, 200.0, 50.0);
        assert!(matches!(m[..], [Move::Linear { feed, .. }] if feed == 50.0));
    }

    #[test]
    fn a_loop_can_start_at_any_point_on_it() {
        let sq = Loop::from_points(&[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]);
        let l = start_at(&sq, [4.0, -3.0]);
        assert_eq!([l.verts[0].x, l.verts[0].y], [4.0, 0.0]);
        assert_eq!(l.verts.len(), 5);
        assert!((loop_length(&l) - 40.0).abs() < 1e-9);
        let l = start_after(&sq, 15.0);
        assert_eq!([l.verts[0].x, l.verts[0].y], [10.0, 5.0]);
        // 円(2 つの半円)の上の点
        let circle = Loop {
            verts: vec![
                Vertex {
                    x: 1.0,
                    y: 0.0,
                    bulge: 1.0,
                },
                Vertex {
                    x: -1.0,
                    y: 0.0,
                    bulge: 1.0,
                },
            ],
        };
        let l = start_at(&circle, [0.0, 5.0]);
        assert!((l.verts[0].x).abs() < 1e-9 && (l.verts[0].y - 1.0).abs() < 1e-9);
        assert!((loop_length(&l) - TAU).abs() < 1e-6);
        assert!((l.signed_area() - PI).abs() < 1e-6);
        // 始まりから 5 mm 進む動き
        let m = walk_moves(&sq, 15.0, -1.0, 100.0);
        assert_eq!(end(&m), [10.0, 5.0, -1.0]);
    }

    #[test]
    fn leads_with_a_small_angle_and_a_line_stay_on_the_free_side() {
        let sq = Loop::from_points(&[[0.0, 0.0], [20.0, 0.0], [20.0, 10.0], [0.0, 10.0]]);
        let l = crate::start_mid_line(&sq);
        let a = leads(&l, (2.0, 45f64.to_radians(), 1.0), true, |q| sq.contains(q)).unwrap();
        assert_eq!(a.radius, 2.0);
        // 円弧の始まりは輪郭から r(1 − cos 45°) だけ内側、直線はそこから進む向きと逆へ 1 mm
        let h = 2.0 * (1.0 - 45f64.to_radians().cos());
        assert!((a.arc_in_start[1] - h).abs() < 1e-9, "{a:?}");
        let d = (a.start[0] - a.arc_in_start[0]).hypot(a.start[1] - a.arc_in_start[1]);
        assert!((d - 1.0).abs() < 1e-9);
        assert!(a.start[1] > a.arc_in_start[1] && a.start[0] < a.arc_in_start[0]);
        let m = a.lead_in(-1.0, 100.0);
        assert_eq!(m.len(), 2);
        assert_eq!(end(&m), [10.0, 0.0, -1.0]);
        let m = a.lead_out(-1.0, 100.0);
        assert_eq!(m.len(), 2);
        assert!((end(&m)[0] - a.end[0]).abs() < 1e-12);
        // 90° で直線なしは、前の 4 分の 1 周と同じ
        let q = leads(&l, (2.0, FRAC_PI_2, 0.0), true, |q| sq.contains(q)).unwrap();
        let old = crate::lead_arcs(&l, 2.0, true, |q| sq.contains(q)).unwrap();
        assert_eq!((q.start, q.end), (old.start, old.end));
    }

    #[test]
    fn finishing_offsets_split_the_allowance() {
        assert_eq!(finish_offsets(3.0, 0.0, 1, false), vec![3.0]);
        assert_eq!(finish_offsets(3.0, 0.4, 2, false), vec![3.2, 3.0]);
        assert_eq!(finish_offsets(3.0, 0.4, 2, true), vec![3.2, 3.0, 3.0]);
    }
}
