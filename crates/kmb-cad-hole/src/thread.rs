//! ねじ(`solid.thread@1`): 円筒の面にねじの属性を付けるか、溝を引いて山の形を作る。
//! 面の径から近い呼び(M6×1 など)を提案し、呼び径とピッチを見せる。
//!
//! 円筒の向き・径・長さは、平らな面と直角に交わる円の辺([`kmb_core::Kernel::arc_edges`])から
//! 求める。外ねじ(ボス)か内ねじ(穴)かは、円の辺の内か外に平らな面があるかと、辺が出っ張りか
//! へこみかで決める(ボスの上の縁は内・出っ張り、根もとは外・へこみ。穴の口は外・出っ張り、
//! 止まり穴の底は内・へこみ)。

use kmb_cad::*;
use kmb_core::kernel::{self as k, P3};
use kmb_core::{
    Action, Context, EvalCtx, EvalError, Extension, Feature, FeatureId, Field, Gesture, Handle,
    InputRef, Outcome, Outputs, Place, Registry, ShapeHandle, Value,
};
use serde::{Deserialize, Serialize};

use super::is_default;

pub const THREAD: &str = "solid.thread@1";
/// 円筒の面をクリックしてねじを付ける操作。
pub const THREAD_ON_FACE: &str = "cad.thread";

/// メートルねじの呼び径と、並目・細目のピッチ(mm)。ISO 261・ISO 262 の第 1 選択と、よく使う
/// 第 2 選択(M14・M18・M22・M27・M33)。
pub const METRIC: &[(f64, f64, &[f64])] = &[
    (1.6, 0.35, &[0.2]),
    (2.0, 0.4, &[0.25]),
    (2.5, 0.45, &[0.35]),
    (3.0, 0.5, &[0.35]),
    (3.5, 0.6, &[0.35]),
    (4.0, 0.7, &[0.5]),
    (5.0, 0.8, &[0.5]),
    (6.0, 1.0, &[0.75]),
    (8.0, 1.25, &[1.0, 0.75]),
    (10.0, 1.5, &[1.25, 1.0]),
    (12.0, 1.75, &[1.5, 1.25]),
    (14.0, 2.0, &[1.5]),
    (16.0, 2.0, &[1.5]),
    (18.0, 2.5, &[2.0, 1.5]),
    (20.0, 2.5, &[2.0, 1.5]),
    (22.0, 2.5, &[2.0, 1.5]),
    (24.0, 3.0, &[2.0]),
    (27.0, 3.0, &[2.0]),
    (30.0, 3.5, &[2.0]),
    (33.0, 3.5, &[2.0]),
    (36.0, 4.0, &[3.0]),
];

/// ねじの呼び(例: M6×1)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    /// 呼び径(外径)
    pub major: f64,
    pub pitch: f64,
    /// 並目か
    pub coarse: bool,
}

fn num(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

impl Size {
    /// 保存する名前(例: `M6x1`)。
    pub fn name(&self) -> String {
        format!("M{}x{}", num(self.major), num(self.pitch))
    }

    /// 見せる名前(例: `M6×1`)。
    pub fn label(&self) -> String {
        format!("M{}×{}", num(self.major), num(self.pitch))
    }

    /// 内ねじの下穴の径。並目で穴の表([`crate::THREADS`])にある呼びは表の下穴(穴の機能のネジ穴と
    /// 同じ径)、ほかは呼び径 − ピッチ。
    pub fn tap_drill(&self) -> f64 {
        match crate::THREADS.iter().find(|t| {
            self.coarse
                && (t.major - self.major).abs() < 1e-9
                && (t.pitch - self.pitch).abs() < 1e-9
        }) {
            Some(t) => t.tap_drill,
            None => self.major - self.pitch,
        }
    }
}

/// 表のすべての呼び(径の小さい順、同じ径なら並目が先)。
pub fn sizes() -> Vec<Size> {
    METRIC
        .iter()
        .flat_map(|(d, c, fine)| {
            std::iter::once(Size {
                major: *d,
                pitch: *c,
                coarse: true,
            })
            .chain(fine.iter().map(|p| Size {
                major: *d,
                pitch: *p,
                coarse: false,
            }))
        })
        .collect()
}

/// 名前(`M6`、`M6x1`、`M6×0.75`、大小文字・空白は問わない)から呼びを探す。ピッチを省くと並目。
pub fn parse_size(name: &str) -> Option<Size> {
    let s: String = name
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_uppercase()
        .replace('×', "X");
    let s = s.strip_prefix('M')?;
    let (d, p) = match s.split_once('X') {
        Some((d, p)) => (d.parse::<f64>().ok()?, Some(p.parse::<f64>().ok()?)),
        None => (s.parse::<f64>().ok()?, None),
    };
    sizes().into_iter().find(|z| {
        (z.major - d).abs() < 1e-9
            && match p {
                Some(p) => (z.pitch - p).abs() < 1e-9,
                None => z.coarse,
            }
    })
}

/// 表に無い呼びの誤り。径が表にあればピッチ、無ければ近い呼びと細目の書き方を示す。
pub fn unknown_size(name: &str) -> String {
    let head = format!("ねじの呼び「{name}」は表にありません");
    let s: String = name
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_uppercase()
        .replace('×', "X");
    let d = s
        .strip_prefix('M')
        .and_then(|r| r.split('X').next())
        .and_then(|d| d.parse::<f64>().ok());
    let Some(d) = d else {
        return format!("{head}。M と呼び径で書き、細目はピッチを付けます(例: M6、M6x0.75)");
    };
    let pitches = |(major, coarse, fine): &(f64, f64, &[f64])| {
        let fine: Vec<String> = fine.iter().map(|p| num(*p)).collect();
        format!(
            "M{} のピッチは {}(並目)・{}(細目)",
            num(*major),
            num(*coarse),
            fine.join("・")
        )
    };
    if let Some(row) = METRIC.iter().find(|r| (r.0 - d).abs() < 1e-9) {
        return format!("{head}。{}", pitches(row));
    }
    let below = METRIC.iter().rev().find(|r| r.0 < d);
    let above = METRIC.iter().find(|r| r.0 > d);
    let near: Vec<String> = below
        .into_iter()
        .chain(above)
        .map(|(m, c, fine)| {
            let fine: Vec<String> = fine.iter().map(|p| num(*p)).collect();
            format!("M{}(並目 {}・細目 {})", num(*m), num(*c), fine.join("・"))
        })
        .collect();
    let example = below
        .or(above)
        .map(|(m, _, fine)| format!("M{}x{}", num(*m), num(fine[0])))
        .unwrap_or_default();
    format!(
        "{head}。近い呼びは {}。細目はピッチを付けて書きます(例: {example})",
        near.join(" と ")
    )
}

/// 面の径 `diameter` に近い順の呼び(同じ近さなら並目)。おねじもめねじも呼び径で比べる
/// (穴もねじの呼び径の概形で作ることが多いので、6 mm の穴は M6)。
pub fn suggest(diameter: f64) -> Vec<Size> {
    let mut v = sizes();
    v.sort_by(|a, b| {
        let da = (a.major - diameter).abs();
        let db = (b.major - diameter).abs();
        da.total_cmp(&db).then(b.coarse.cmp(&a.coarse))
    });
    v
}

/// 円筒 `c` に近い順の呼び。おねじもめねじも呼び径で比べる(穴もねじの呼び径の概形で作ることが
/// 多いので、6 mm の穴は M6)。ただし、めねじの径が穴の表のネジ穴の下穴とちょうど
/// 同じなら、その呼びを先にする(JC のネジ穴で開けた M6 の 5 mm は M5 でなく M6)。
pub fn suggest_for(c: &Cylinder) -> Vec<Size> {
    let mut v = suggest(c.diameter);
    if c.internal
        && let Some(i) = v.iter().position(|s| {
            s.coarse
                && crate::THREADS.iter().any(|t| {
                    (t.major - s.major).abs() < 1e-9 && (t.tap_drill - c.diameter).abs() < 1e-3
                })
        })
    {
        let s = v.remove(i);
        v.insert(0, s);
    }
    v
}

/// 呼びの径と面の径の違いを許す幅(これを越えると注意を出す)。
fn tolerance(s: &Size) -> f64 {
    0.05 + 0.25 * s.pitch
}

/// 円筒 `c` の径が呼び `size` に合わない時の注意(合えば無し)。めねじは下穴の径か呼び径の
/// どちらかに合えばよく、合わなければ下穴の径を添える(例: M6 なら下穴は 5 mm)。
fn mismatch(c: &Cylinder, size: &Size, near: &Size) -> Option<String> {
    let tol = tolerance(size);
    let major = c.diameter - size.major;
    if !c.internal {
        return (major.abs() > tol).then(|| {
            format!(
                "面の径 {} mm は {} の呼び径 {} mm と {} mm 違います(面の径に近いのは {})",
                num(c.diameter),
                size.label(),
                num(size.major),
                num(major.abs()),
                near.label()
            )
        });
    }
    let tap = size.tap_drill();
    if (c.diameter - tap).abs() <= tol || major.abs() <= tol {
        return None;
    }
    Some(format!(
        "穴の径 {} mm は {} に合いません({} なら下穴は {} mm、呼び径は {} mm。穴の径に近いのは {})",
        num(c.diameter),
        size.label(),
        size.label(),
        num(tap),
        num(size.major),
        near.label()
    ))
}

/// ねじを付ける円筒(面の径・軸・長さ・内か外か)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cylinder {
    pub diameter: f64,
    /// 軸の上の点(円の中心の 1 つ)
    pub center: P3,
    pub axis: P3,
    /// 軸の向きの長さ(両端の円の中心の間。片端しか分からなければ 0)
    pub length: f64,
    /// 内ねじ(穴)か
    pub internal: bool,
}

/// ボディ `body` の面 `face` の円筒。平らな面と直角に交わる円筒でなければ理由を返す。
pub fn cylinder_of(
    kernel: &dyn kmb_core::Kernel,
    body: ShapeHandle,
    face: &str,
) -> Result<Cylinder, String> {
    let edges: Vec<_> = kernel
        .arc_edges(body)
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|e| e.faces[1] == face)
        .filter_map(|e| Some((e.arc?, e.convex)))
        .collect();
    let arcs: Vec<_> = edges.iter().map(|e| e.0).collect();
    let Some((first, convex)) = edges.first().copied() else {
        return Err(
            "円筒の面をクリックしてください(平らな面と直角に交わる円筒の面だけ使えます)".into(),
        );
    };
    let axis = k::normalize(first.axis);
    let along = |c: P3| k::dot(k::sub(c, first.center), axis);
    let (lo, hi) = arcs.iter().fold((0.0f64, 0.0f64), |(lo, hi), a| {
        let t = along(a.center);
        (lo.min(t), hi.max(t))
    });
    Ok(Cylinder {
        diameter: first.radius * 2.0,
        center: first.center,
        axis,
        length: hi - lo,
        internal: first.inside != convex,
    })
}

/// 3D のビューにねじ山の陰影を付ける印。ねじは開いた端(穴の口・ボスの先。出っ張りの円の辺)から
/// `length`(省くと面の全長)だけ続く。両端とも開いていれば(貫通の穴)軸の向きの手前の端から。
/// 両端の分からない面(円の辺が片側だけ)は印を出さない。
pub fn mark_of(
    kernel: &dyn kmb_core::Kernel,
    body: ShapeHandle,
    face: &str,
    c: &Cylinder,
    pitch: f64,
    length: Option<f64>,
) -> Option<kmb_interchange::ThreadMark> {
    let ends: Vec<(f64, bool)> = kernel
        .arc_edges(body)
        .ok()?
        .into_iter()
        .filter(|e| e.faces[1] == face)
        .filter_map(|e| Some((k::dot(k::sub(e.arc?.center, c.center), c.axis), e.convex)))
        .collect();
    let lo = ends.iter().map(|e| e.0).fold(f64::INFINITY, f64::min);
    let hi = ends.iter().map(|e| e.0).fold(f64::NEG_INFINITY, f64::max);
    let full = hi - lo;
    if full <= 1e-9 || full.is_nan() {
        return None;
    }
    let open = |t: f64| {
        ends.iter()
            .any(|&(u, convex)| convex && (u - t).abs() < 1e-6)
    };
    let from_hi = open(hi) && !open(lo);
    let (t, axis) = if from_hi {
        (hi, k::scale(c.axis, -1.0))
    } else {
        (lo, c.axis)
    };
    Some(kmb_interchange::ThreadMark {
        start: k::add(c.center, k::scale(c.axis, t)),
        axis,
        length: length.unwrap_or(full).min(full),
        radius: c.diameter / 2.0,
        pitch,
        internal: c.internal,
    })
}

/// ねじの巻く向き。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Hand {
    #[default]
    Right,
    Left,
}

/// 属性だけを残すか、溝を切って山の形を作るか。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Form {
    #[default]
    Cosmetic,
    Modeled,
}

/// 奥の端で溝を逃がすか、深さを保ったまま止めるか。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum End {
    #[default]
    RunOut,
    Abrupt,
}

/// 両方のねじの機能で同じ欄を使う。
pub(crate) fn form_fields() -> Vec<Field> {
    vec![
        Field::choice(
            "form",
            "ねじ山",
            &[("cosmetic", "属性のみ"), ("modeled", "形を作る")],
        ),
        Field::opt_expr("clearance", "すき間")
            .length()
            .only("form", "modeled", "cosmetic")
            .help("径で指定するすき間。空なら 0 mm"),
        Field::choice(
            "end",
            "奥の端",
            &[("run_out", "切り上げ"), ("abrupt", "切りっぱなし")],
        )
        .only("form", "modeled", "cosmetic"),
        Field::bool("flip", "反対の端から始める").only("form", "modeled", "cosmetic"),
    ]
}

pub(crate) fn form_args() -> Vec<kmb_core::Arg> {
    vec![
        kmb_core::Arg::choice(
            "form",
            "ねじ山",
            &[("cosmetic", "属性のみ"), ("modeled", "形を作る")],
        ),
        kmb_core::Arg::length("clearance", "ねじ山のすき間(径、mm)"),
        kmb_core::Arg::choice(
            "end",
            "ねじの奥の端",
            &[("run_out", "切り上げ"), ("abrupt", "切りっぱなし")],
        ),
        kmb_core::Arg::flag("flip", "反対の端からねじを始める"),
    ]
}

/// 円筒の両端。出っ張りの端を優先し、両端が自由なら軸の正の側から始める。
fn modeled_ends(
    kernel: &dyn kmb_core::Kernel,
    body: ShapeHandle,
    face: &str,
    c: &Cylinder,
    flip: bool,
) -> Result<(P3, P3, f64, bool), EvalError> {
    let mut ends: Vec<_> = kernel
        .arc_edges(body)
        .map_err(kernel_err)?
        .into_iter()
        .filter(|e| e.faces[1] == face)
        .filter_map(|e| Some((e.arc?.center, e.convex)))
        .collect();
    if ends.len() < 2 {
        // 面取りと円筒の境は平らな面を持たない。既知の軸と径に一致する閉じた円の辺だけを補う。
        for edge in kernel.curve_edges(body).map_err(kernel_err)? {
            if !edge.faces.iter().any(|name| name == face)
                || edge.curve.len() < 4
                || k::dot(k::sub(edge.a, edge.b), k::sub(edge.a, edge.b)) > 1e-12
            {
                continue;
            }
            let t = k::dot(k::sub(edge.a, c.center), c.axis);
            let center = k::add(c.center, k::scale(c.axis, t));
            let circular = edge.curve.iter().all(|p| {
                let d = k::sub(*p, center);
                k::dot(d, c.axis).abs() <= 1e-6
                    && (k::dot(d, d).sqrt() - c.diameter / 2.0).abs() <= 1e-6
            });
            if circular {
                ends.push((center, edge.convex));
            }
        }
    }
    ends.sort_by(|a, b| k::dot(a.0, c.axis).total_cmp(&k::dot(b.0, c.axis)));
    ends.dedup_by(|a, b| k::dot(k::sub(a.0, b.0), c.axis).abs() < 1e-6);
    let (Some(lo), Some(hi)) = (ends.first(), ends.last()) else {
        return Err(EvalError::InvalidData(
            "ねじの両端の円を持つ円筒の面を選んでください".into(),
        ));
    };
    let full = k::dot(k::sub(hi.0, lo.0), c.axis);
    if full <= 1e-6 {
        return Err(EvalError::InvalidData(
            "ねじの両端が分かりません。両端の円を持つ円筒の面を選んでください".into(),
        ));
    }
    if !lo.1 && !hi.1 {
        return Err(EvalError::InvalidData(
            "ねじの両端がへこんでいます。開いた端のある円筒の面を選んでください".into(),
        ));
    }
    let from_hi = (hi.1 || !lo.1) != flip;
    Ok(if from_hi {
        (hi.0, k::scale(c.axis, -1.0), full, lo.1)
    } else {
        (lo.0, c.axis, full, hi.1)
    })
}

/// 溝の台形と、壁の径に対する作れる範囲。
fn groove(
    c: &Cylinder,
    size: Size,
    clearance: f64,
) -> Result<([[f64; 2]; 4], f64, f64), EvalError> {
    let bad = |s: String| EvalError::InvalidData(s);
    if !clearance.is_finite() || clearance < 0.0 {
        return Err(bad("すき間は 0 mm 以上にしてください".into()));
    }
    let p = size.pitch;
    let h = 3f64.sqrt() / 2.0 * p;
    let radius = c.diameter / 2.0;
    let root = if c.internal {
        size.major / 2.0 + clearance / 2.0
    } else {
        size.major / 2.0 - 5.0 * h / 8.0 - clearance / 2.0
    };
    let rim = radius + if c.internal { -0.05 * p } else { 0.05 * p };
    let half_root = if c.internal { p / 16.0 } else { p / 8.0 };
    let depth = if c.internal { root - rim } else { rim - root };
    let half = half_root + depth / 3f64.sqrt();
    if half * 2.0 > 0.98 * p {
        return Err(bad(if c.internal {
            format!(
                "下穴 {} mm が小さすぎます。下穴を大きくしてください",
                num(c.diameter)
            )
        } else {
            format!(
                "軸の径 {} mm が呼び {} に対して太すぎて山の先が無くなります。{} mm 以下にしてください",
                num(c.diameter),
                size.label(),
                num(size.major - clearance)
            )
        }));
    }
    if root <= 1e-6
        || if c.internal {
            radius >= root - 0.1 * p
        } else {
            radius <= root + 0.1 * p
        }
    {
        return Err(bad(if c.internal {
            "下穴が大きすぎて山ができません。下穴の径を小さくしてください".into()
        } else {
            "軸が細すぎて山ができません。軸の径を大きくしてください".into()
        }));
    }
    Ok((
        [
            [root, -half_root],
            [rim, -half],
            [rim, half],
            [root, half_root],
        ],
        half,
        depth + 0.05 * p,
    ))
}

/// 円筒の壁に共通の道具を掃引し、ボディから 1 回だけ引く。
#[allow(clippy::too_many_arguments)]
pub(crate) fn cut_modeled(
    kernel: &dyn kmb_core::Kernel,
    body: ShapeHandle,
    face: &str,
    size: Size,
    length: Option<f64>,
    clearance: f64,
    end: End,
    flip: bool,
    hand: Hand,
    tag: &str,
) -> Result<(ShapeHandle, Vec<String>), EvalError> {
    let c = cylinder_of(kernel, body, face).map_err(EvalError::InvalidData)?;
    let (start, axis, full, through) = modeled_ends(kernel, body, face, &c, flip)?;
    let (vertices, half, run_radial) = groove(&c, size, clearance)?;
    let p = size.pitch;
    let run_length = if end == End::RunOut { p / 4.0 } else { 0.0 };
    let maximum = if through {
        full
    } else {
        full - run_length - half
    };
    let length = length.unwrap_or(maximum);
    if !length.is_finite() || length <= 0.0 || (!through && length > maximum + 1e-6) {
        return Err(EvalError::InvalidData(format!(
            "ねじの長さは最大 {} mm です。0 より大きく、この長さ以下にしてください",
            num(maximum.max(0.0))
        )));
    }
    let escape = through && length >= full - 1e-6;
    if length > full + 1e-6 {
        return Err(EvalError::InvalidData(format!(
            "ねじの長さは最大 {} mm です。この長さ以下にしてください",
            num(full)
        )));
    }
    let distance = if escape { full + p } else { length };
    let turns = (distance + p) / p;
    let radial = k::normalize(k::cross(
        axis,
        if axis[2].abs() < 0.9 {
            [0.0, 0.0, 1.0]
        } else {
            [0.0, 1.0, 0.0]
        },
    ));
    let origin = k::add(start, k::scale(axis, -p));
    let profile = k::Profile {
        plane: k::Plane {
            origin,
            x_axis: radial,
            y_axis: axis,
        },
        regions: vec![k::Region {
            outer: k::ProfileLoop {
                edges: (0..4)
                    .map(|i| k::ProfileEdge {
                        name: format!("e{i}"),
                        segment: k::Segment::Line {
                            a: vertices[i],
                            b: vertices[(i + 1) % 4],
                        },
                        reversed: false,
                    })
                    .collect(),
            },
            holes: vec![],
        }],
    };
    let helix = k::Helix {
        origin,
        axis,
        left: hand == Hand::Left,
        spans: vec![k::HelixSpan { turns, pitch: p }],
        blend: 0.0,
        run_out: (end == End::RunOut && !escape).then_some(k::HelixRun {
            turns: 0.25,
            radial: if c.internal { -run_radial } else { run_radial },
        }),
    };
    let tool = kernel
        .helical_sweep(&profile, &helix, tag)
        .map_err(kernel_err)?;
    let result = kernel
        .boolean(k::BooleanOp::Subtract, body, tool)
        .map_err(kernel_err)?;
    let count = turns + helix.run_out.map_or(0.0, |r| r.turns);
    let warnings = if count > 100.0 {
        vec![format!(
            "ねじ山が {} 巻きあり、再計算に時間がかかります",
            num(count)
        )]
    } else {
        vec![]
    };
    Ok((result, warnings))
}

/// 円筒の面にねじの属性を付けるか、山の形を作る。
/// 例: `{"target": 3, "face": "f3:side", "size": "M6x1"}`。`size` を省くと面の径に近い呼び。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Thread {
    pub target: FeatureId,
    pub face: String,
    /// 呼び(例: `M6x1`、`M6` は並目)。空なら面の径に近い呼び
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub size: String,
    /// ねじの長さ。省くと面の全長
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub hand: Hand,
    #[serde(default, skip_serializing_if = "is_default")]
    pub form: Form,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clearance: Option<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub end: End,
    #[serde(default, skip_serializing_if = "is_default")]
    pub flip: bool,
}

impl Exprs for Thread {
    fn exprs_mut(&mut self) -> Vec<&mut String> {
        self.length
            .iter_mut()
            .chain(self.clearance.iter_mut())
            .collect()
    }
}

/// ねじの呼びを決め、注意の文を返す。
fn resolve(t: &Thread, c: &Cylinder) -> Result<(Size, Vec<String>), String> {
    if t.form == Form::Modeled
        && ["G", "R", "NPT", "BSP"]
            .iter()
            .any(|prefix| t.size.trim().to_ascii_uppercase().starts_with(prefix))
    {
        return Err(
            "管用ねじとテーパねじの山の形はまだ作れません。属性のみを使ってください".into(),
        );
    }
    let near = suggest_for(c);
    let size = if t.size.trim().is_empty() {
        near[0]
    } else {
        parse_size(&t.size).ok_or_else(|| {
            format!(
                "{}(面の径に近いのは {})",
                unknown_size(&t.size),
                near[0].label()
            )
        })?
    };
    Ok((size, mismatch(c, &size, &near[0]).into_iter().collect()))
}

impl Feature for Thread {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut v = vec![output(self.target, "body")];
        v.extend(expr_refs(self)?);
        Ok(v)
    }

    fn pass_through(&self) -> Vec<(String, InputRef)> {
        vec![("body".into(), output(self.target, "body"))]
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let body = body_of(ctx, self.target)?;
        let c = cylinder_of(ctx.kernel(), body, &self.face).map_err(EvalError::InvalidData)?;
        let (size, mut warn) = resolve(self, &c).map_err(EvalError::InvalidData)?;
        let mut thread_length = None;
        if let Some(src) = &self.length {
            let l = length(ctx, src, "ねじの長さ")?;
            thread_length = Some(l);
            if l <= 0.0 {
                return Err(EvalError::InvalidData(
                    "ねじの長さは 0 より大きくしてください".into(),
                ));
            }
            if c.length > 0.0 && l > c.length + 1e-6 {
                warn.push(format!(
                    "ねじの長さ {} mm が面の長さ {} mm より長くなっています",
                    num(l),
                    num(c.length)
                ));
            }
        }
        let near = suggest_for(&c);
        let choices: Vec<kmb_core::Choice> = near
            .iter()
            .enumerate()
            .map(|(i, s)| kmb_core::Choice {
                value: s.name(),
                label: format!(
                    "{}(呼び径 {} mm・ピッチ {} mm・{}){}",
                    s.label(),
                    num(s.major),
                    num(s.pitch),
                    if s.coarse { "並目" } else { "細目" },
                    if i == 0 && c.internal {
                        " ← 穴の径に近い"
                    } else if i == 0 {
                        " ← 面の径に近い"
                    } else {
                        ""
                    }
                ),
            })
            .collect();
        let result = if self.form == Form::Modeled {
            let clearance = self
                .clearance
                .as_deref()
                .map(|s| length(ctx, s, "すき間"))
                .transpose()?
                .unwrap_or(0.0);
            let (result, warnings) = cut_modeled(
                ctx.kernel(),
                body,
                &self.face,
                size,
                thread_length,
                clearance,
                self.end,
                self.flip,
                self.hand,
                &tag(ctx),
            )?;
            warn.extend(warnings);
            result
        } else {
            body
        };
        let mut out: Outputs = [
            ("body".to_string(), Value::Shape(result)),
            ("sizes".to_string(), kmb_core::choices_value(choices)),
        ]
        .into();
        kmb_core::add_warnings(&mut out, warn);
        if self.form == Form::Cosmetic
            && let Some(mark) = mark_of(
                ctx.kernel(),
                body,
                &self.face,
                &c,
                size.pitch,
                thread_length,
            )
        {
            out.insert(
                kmb_interchange::THREAD_MARKS_OUTPUT.into(),
                kmb_core::Payload::to_value(&kmb_interchange::ThreadMarks { marks: vec![mark] }),
            );
        }
        Ok(out)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        rename(self, old, new)
    }
}

/// 機能の種類を登録する。
pub fn register(reg: &mut Registry) {
    reg.register_described(
        solid(THREAD, "ねじ")
            .icon("thread")
            .hint("円筒の面にねじを付ける。属性のみか、ねじ山の形を作るかを選べる")
            .aliases("ねじ ネジ thread タップ おねじ めねじ")
            .field(Field::output("target", "対象", "body"))
            .field(Field::info("face", "円筒の面"))
            .field(
                Field::text("size", "呼び")
                    .help("空なら面の径に近い呼び。一覧は面の径に近い順(おねじは呼び径、めねじは下穴の径か呼び径で比べる)")
                    .suggest_own("sizes"),
            )
            .field(Field::opt_expr("length", "長さ").length().help("空なら面の全長"))
            .field(Field::choice(
                "hand",
                "方向",
                &[("right", "右ねじ"), ("left", "左ねじ")],
            ))
            .fields(form_fields())
            .retarget(kmb_core::Retarget::Face { key: "face".into() }),
        |d| Ok(Box::new(parse::<Thread>(d)?)),
    );
}

/// ねじの操作。
pub struct ThreadCommands;

/// クリックした円筒の面にねじを付ける(呼びは面の径に近いもの。CLI・MCP は引数 `face` と `feature`、
/// `size` で指定もできる)。
fn thread_on_face(cx: &Context<'_>) -> Result<Outcome, kmb_core::Problem> {
    let face = cx
        .arg("face")
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(|| cx.picked.map(|p| p.face.clone()))
        .ok_or("ねじを付ける円筒の面をクリックしてください")?;
    let feature = cx
        .arg("feature")
        .and_then(|v| v.as_u64())
        .map(FeatureId)
        .or_else(|| cx.picked.map(|p| p.feature))
        .or_else(|| cx.visible_bodies().last().map(|b| b.0))
        .ok_or("ボディがありません")?;
    let Some(Value::Shape(body)) = cx.output(feature, "body") else {
        return Err(format!("{feature} はボディではありません").into());
    };
    let c = cylinder_of(cx.kernel.as_ref(), *body, &face)?;
    let mut data = Thread {
        target: feature,
        face,
        size: String::new(),
        length: None,
        hand: Hand::Right,
        form: Form::Cosmetic,
        clearance: None,
        end: End::RunOut,
        flip: false,
    };
    data.size = match cx.arg("size").and_then(|v| v.as_str()) {
        Some(s) => parse_size(s).ok_or_else(|| unknown_size(s))?.name(),
        None => match hole_size(cx, &data.face) {
            Some(s) => s.name(),
            None => suggest_for(&c)[0].name(),
        },
    };
    if let Some(v) = cx.arg("form") {
        data.form = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
    }
    if let Some(v) = cx.arg("end") {
        data.end = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
    }
    data.clearance = cx.arg_f64("clearance")?.map(|c| format!("{c} mm"));
    data.flip = cx.arg("flip").and_then(|v| v.as_bool()).unwrap_or(false);
    Ok(Outcome::insert(
        "ねじを付ける",
        THREAD,
        format!("ねじ{}", cx.count_of(THREAD) + 1),
        serde_json::to_value(data).map_err(|e| e.to_string())?,
    ))
}

/// 面 `face` を作った機能が JC のネジ穴(穴の機能でネジ穴を選んだ物)なら、そのネジの呼び(並目)。
fn hole_size(cx: &Context<'_>, face: &str) -> Option<Size> {
    let f = cx.data.feature(kmb_core::face_feature(face)?)?;
    if f.type_id != crate::HOLE {
        return None;
    }
    let h: crate::Hole = serde_json::from_value(f.data.clone()).ok()?;
    if h.tap != crate::Tap::Tapped {
        return None;
    }
    parse_size(h.size.as_deref().unwrap_or(crate::DEFAULT_SIZE))
}

/// ねじの機能 `id` の要約(呼び径・ピッチ・内か外か・長さ)。
fn summary(cx: &Context<'_>, id: FeatureId) -> Option<String> {
    let f = cx.data.feature(id)?;
    if f.type_id != THREAD {
        return None;
    }
    let t: Thread = serde_json::from_value(f.data.clone()).ok()?;
    let Some(Value::Shape(body)) = cx.output(t.target, "body") else {
        return None;
    };
    let c = cylinder_of(cx.kernel.as_ref(), *body, &t.face).ok()?;
    let (s, _) = resolve(&t, &c).ok()?;
    let length = match &t.length {
        Some(l) if !l.trim().is_empty() => format!("長さ {l}"),
        _ if c.length > 0.0 => format!("長さ {} mm(全長)", num(c.length)),
        _ => "全長".into(),
    };
    Some(format!(
        "{}の{}: 呼び径 {} mm・ピッチ {} mm({})・{}・{}。面の径 {} mm{}",
        s.label(),
        if c.internal {
            "めねじ(内)"
        } else {
            "おねじ(外)"
        },
        num(s.major),
        num(s.pitch),
        if s.coarse { "並目" } else { "細目" },
        if t.hand == Hand::Left {
            "左ねじ"
        } else {
            "右ねじ"
        },
        length,
        num(c.diameter),
        if c.internal {
            format!("(下穴の目安 {} mm)", num(s.tap_drill()))
        } else {
            String::new()
        }
    ))
}

impl Extension for ThreadCommands {
    fn actions(&self, cx: &Context<'_>) -> Vec<Action> {
        let none = cx.visible_bodies().is_empty();
        vec![
            Action::new(THREAD_ON_FACE, "ねじ", "cad")
                .args([
                    kmb_core::Arg::text("face", "ねじを付ける円筒の面の名前(省くと選んでいる面)"),
                    kmb_core::Arg::feature("feature", "面を持つ機能の番号"),
                    kmb_core::Arg::text("size", "ねじの呼び(M6 など。省くと面の径に近いもの)"),
                ].into_iter().chain(form_args()))
                .menu("作成")
                .icon("thread")
                .aliases("ねじ ネジ thread タップ おねじ めねじ")
                .hint("押してから円筒の面(ボスの外側か穴の内側)をクリックすると、面の径に近いメートルねじの呼びとピッチが入る。呼び・長さ・向き・ねじ山は欄で直し、OK か Enter で決める")
                // 円筒の面を指定する操作はメニューから始める。
                .at(&[Place::Menu])
                .disabled_if(none.then_some("ボディがありません")),
        ]
    }

    fn run(&self, id: &str, cx: &Context<'_>) -> Option<Result<Outcome, kmb_core::Problem>> {
        (id == THREAD_ON_FACE).then(|| thread_on_face(cx))
    }

    fn handles(&self, id: &str, cx: &Context<'_>) -> Option<Vec<Handle>> {
        // 先に円筒の面を選んでいれば、その面に付ける(面をクリックする手順を飛ばす。穴・フィレットと同じ)
        let picked_cylinder = cx.arg("face").is_none()
            && cx.picked.is_some_and(|p| {
                matches!(cx.output(p.feature, "body"), Some(Value::Shape(b))
                    if cylinder_of(cx.kernel.as_ref(), *b, &p.face).is_ok())
            });
        (id == THREAD_ON_FACE && !picked_cylinder).then(|| {
            vec![Handle::new(
                "face",
                "ねじを付ける円筒の面(ボスの外側か穴の内側)をクリック",
                Gesture::AnyFace,
            )]
        })
    }

    fn summary(&self, cx: &Context<'_>, id: FeatureId) -> Option<String> {
        summary(cx, id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_parse_with_or_without_the_pitch() {
        let m6 = parse_size("M6").unwrap();
        assert_eq!((m6.major, m6.pitch, m6.coarse), (6.0, 1.0, true));
        let fine = parse_size(" m6 × 0.75 ").unwrap();
        assert_eq!((fine.pitch, fine.coarse), (0.75, false));
        assert_eq!(fine.name(), "M6x0.75");
        assert_eq!(parse_size("M8x1").unwrap().label(), "M8×1");
        assert!(parse_size("M7").is_none(), "表にない径");
        assert!(parse_size("M6x2").is_none(), "表にないピッチ");
        assert!(parse_size("6").is_none());
    }

    #[test]
    fn an_unknown_size_names_the_nearest_sizes_and_a_fine_pitch() {
        // 確かめること: 表に無い呼びに、近い呼びと細目の書き方の例を示す。
        let m = unknown_size("M28");
        assert!(m.contains("M27(並目 3・細目 2) と M30"), "{m}");
        assert!(m.contains("M27x2"), "細目の例: {m}");
        // 径は表にあってピッチだけ違う時は、その径のピッチを並べる
        let m = unknown_size("M6x0.8");
        assert!(m.contains("M6 のピッチは 1(並目)・0.75(細目)"), "{m}");
        // 読めない書き方は書き方の例
        assert!(
            unknown_size("6mm").contains("M6"),
            "{}",
            unknown_size("6mm")
        );
    }

    #[test]
    fn the_nearest_size_is_by_the_major_diameter() {
        // ボスの径 5.9 → M6×1(並目が先)
        let s = suggest(5.9);
        assert_eq!(s[0].name(), "M6x1");
        assert_eq!(s[1].name(), "M6x0.75");
        // 6 mm の穴も M6(穴は呼び径の概形で作る)。5 mm なら M5
        assert_eq!(suggest(6.0)[0].name(), "M6x1");
        assert_eq!(suggest(5.0)[0].name(), "M5x0.8");
        assert_eq!(suggest(8.0)[0].name(), "M8x1.25");
    }

    #[test]
    fn a_tapped_hole_suggests_its_size_and_the_tap_drill() {
        // JC のネジ穴の径(表の下穴)は、その呼びを薦める(M6 の 5 mm は M5 でなく M6)
        let hole = |d: f64| Cylinder {
            diameter: d,
            center: [0.0; 3],
            axis: [0.0, 0.0, 1.0],
            length: 10.0,
            internal: true,
        };
        for t in crate::THREADS {
            let s = suggest_for(&hole(t.tap_drill))[0];
            assert_eq!((s.major, s.coarse), (t.major, true), "{}", t.name);
            assert_eq!(s.tap_drill(), t.tap_drill, "{}", t.name);
        }
        // 概形の穴(呼び径で開けた穴)は前と同じく呼び径で
        assert_eq!(suggest_for(&hole(6.0))[0].name(), "M6x1");
        // おねじは呼び径だけで比べる
        let boss = Cylinder {
            internal: false,
            ..hole(5.0)
        };
        assert_eq!(suggest_for(&boss)[0].name(), "M5x0.8");
        // 合わない時は下穴の径を添える
        let t = Thread {
            target: FeatureId(1),
            face: "f1:side".into(),
            size: "M6".into(),
            length: None,
            hand: Hand::Right,
            form: Form::Cosmetic,
            clearance: None,
            end: End::RunOut,
            flip: false,
        };
        let (_, w) = resolve(&t, &hole(5.0)).unwrap();
        assert!(w.is_empty(), "{w:?}");
        let (_, w) = resolve(&t, &hole(4.2)).unwrap();
        assert!(w[0].contains("M6×1 なら下穴は 5 mm"), "{w:?}");
        assert!(w[0].contains("M5×0.8"), "{w:?}");
    }

    #[test]
    fn a_far_size_gives_a_warning_with_the_nearest() {
        let c = Cylinder {
            diameter: 5.2,
            center: [0.0; 3],
            axis: [0.0, 0.0, 1.0],
            length: 10.0,
            internal: false,
        };
        let t = Thread {
            target: FeatureId(1),
            face: "f1:side".into(),
            size: "M6".into(),
            length: None,
            hand: Hand::Right,
            form: Form::Cosmetic,
            clearance: None,
            end: End::RunOut,
            flip: false,
        };
        let (s, w) = resolve(&t, &c).unwrap();
        assert_eq!(s.name(), "M6x1");
        assert!(w[0].contains("M5×0.8"), "{w:?}");
        let near = Thread {
            size: String::new(),
            ..t
        };
        let (s, w) = resolve(&near, &c).unwrap();
        assert_eq!(s.name(), "M5x0.8");
        assert!(w.is_empty(), "{w:?}");
    }

    #[test]
    fn cosmetic_thread_and_hole_defaults_preserve_their_saved_bytes() {
        for bytes in [
            r#"{"target":1,"face":"f1:side"}"#,
            r#"{"target":1,"face":"f1:side","size":"M8x1.25","length":"10 mm","hand":"left"}"#,
        ] {
            let thread: Thread = serde_json::from_str(bytes).unwrap();
            assert_eq!(thread.form, Form::Cosmetic);
            assert_eq!(serde_json::to_string(&thread).unwrap(), bytes);
        }
        let bytes = r#"{"target":1,"sketch":2,"tap":"tapped","size":"M6"}"#;
        let hole: crate::Hole = serde_json::from_str(bytes).unwrap();
        assert_eq!(hole.form, Form::Cosmetic);
        assert_eq!(serde_json::to_string(&hole).unwrap(), bytes);
    }

    #[test]
    fn thread_grooves_reject_missing_crests_and_accept_print_clearance() {
        let c = Cylinder {
            diameter: 7.8,
            center: [0.0; 3],
            axis: [0.0, 0.0, 1.0],
            length: 20.0,
            internal: false,
        };
        let size = parse_size("M8").unwrap();
        let (vertices, half, _) = groove(&c, size, 0.2).unwrap();
        assert!(half * 2.0 < 0.98 * size.pitch);
        assert!(vertices[0][0] < 3.9);
        for (diameter, internal, message) in [
            (10.0, false, "太すぎ"),
            (6.0, false, "細すぎ"),
            (5.0, true, "小さすぎ"),
            (8.0, true, "大きすぎ"),
        ] {
            let error = groove(
                &Cylinder {
                    diameter,
                    internal,
                    ..c
                },
                size,
                0.0,
            )
            .unwrap_err()
            .to_string();
            assert!(error.contains(message), "{error}");
        }
        assert!(
            groove(&c, size, -0.1)
                .unwrap_err()
                .to_string()
                .contains("すき間")
        );
        assert!(groove(&c, size, f64::NAN).is_err());
    }
}
