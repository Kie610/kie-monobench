//! ねじ(`solid.thread@1`): 円筒の面にねじの属性を付ける。形のねじ山は作らない。
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

/// ねじの族。管用は平行ねじだけを扱う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Metric,
    Unified,
    Pipe,
}

/// ねじの呼び。寸法はすべて mm。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    pub family: Family,
    /// 呼び径(ねじ山の外径)
    pub major: f64,
    pub pitch: f64,
    /// メートルの並目、またはユニファイの UNC か。
    pub coarse: bool,
    nominal: &'static str,
    tap: f64,
    /// 管用の平行内ねじ Rp か。
    pipe_internal: bool,
}

/// ASME B1.1 の呼び径(in)、UNC・UNF の山数と、切削タップ用の代表下穴(in)。
/// 下穴はねじ山の基本の谷の径とは別の、切削のための値。
struct Unified {
    nominal: &'static str,
    inch: f64,
    tpi: [u16; 2],
    tap: [f64; 2],
}

#[rustfmt::skip]
const UNIFIED: &[Unified] = &[
    Unified { nominal: "#4", inch: 0.112, tpi: [40, 48], tap: [0.0890, 0.0935] },
    Unified { nominal: "#5", inch: 0.125, tpi: [40, 44], tap: [0.1015, 0.1040] },
    Unified { nominal: "#6", inch: 0.138, tpi: [32, 40], tap: [0.1065, 0.1130] },
    Unified { nominal: "#8", inch: 0.164, tpi: [32, 36], tap: [0.1360, 0.1360] },
    Unified { nominal: "#10", inch: 0.190, tpi: [24, 32], tap: [0.1495, 0.1590] },
    Unified { nominal: "#12", inch: 0.216, tpi: [24, 28], tap: [0.1770, 0.1820] },
    Unified { nominal: "1/4", inch: 0.25, tpi: [20, 28], tap: [0.2010, 0.2130] },
    Unified { nominal: "5/16", inch: 0.3125, tpi: [18, 24], tap: [0.2570, 0.2720] },
    Unified { nominal: "3/8", inch: 0.375, tpi: [16, 24], tap: [0.3125, 0.3320] },
    Unified { nominal: "7/16", inch: 0.4375, tpi: [14, 20], tap: [0.3680, 25.0 / 64.0] },
    Unified { nominal: "1/2", inch: 0.5, tpi: [13, 20], tap: [27.0 / 64.0, 29.0 / 64.0] },
    Unified { nominal: "9/16", inch: 0.5625, tpi: [12, 18], tap: [31.0 / 64.0, 33.0 / 64.0] },
    Unified { nominal: "5/8", inch: 0.625, tpi: [11, 18], tap: [17.0 / 32.0, 37.0 / 64.0] },
    Unified { nominal: "3/4", inch: 0.75, tpi: [10, 16], tap: [21.0 / 32.0, 11.0 / 16.0] },
    Unified { nominal: "7/8", inch: 0.875, tpi: [9, 14], tap: [49.0 / 64.0, 13.0 / 16.0] },
    Unified { nominal: "1", inch: 1.0, tpi: [8, 12], tap: [7.0 / 8.0, 15.0 / 16.0] },
    Unified { nominal: "1-1/8", inch: 1.125, tpi: [7, 12], tap: [63.0 / 64.0, 1.0 + 3.0 / 64.0] },
    Unified { nominal: "1-1/4", inch: 1.25, tpi: [7, 12], tap: [1.0 + 7.0 / 64.0, 1.0 + 11.0 / 64.0] },
    Unified { nominal: "1-3/8", inch: 1.375, tpi: [6, 12], tap: [1.0 + 7.0 / 32.0, 1.0 + 19.0 / 64.0] },
    Unified { nominal: "1-1/2", inch: 1.5, tpi: [6, 12], tap: [1.0 + 11.0 / 32.0, 1.0 + 27.0 / 64.0] },
];

/// ISO 228-1・ISO 7-1 の管用平行ねじの基本寸法。呼びの数は外径(in)ではない。
struct Pipe {
    nominal: &'static str,
    major: f64,
    tpi: u16,
    tap: f64,
}

#[rustfmt::skip]
const PIPE: &[Pipe] = &[
    Pipe { nominal: "1/8", major: 9.728, tpi: 28, tap: 8.8 },
    Pipe { nominal: "1/4", major: 13.157, tpi: 19, tap: 11.8 },
    Pipe { nominal: "3/8", major: 16.662, tpi: 19, tap: 15.25 },
    Pipe { nominal: "1/2", major: 20.955, tpi: 14, tap: 19.0 },
    Pipe { nominal: "3/4", major: 26.441, tpi: 14, tap: 24.5 },
    Pipe { nominal: "1", major: 33.249, tpi: 11, tap: 30.75 },
    Pipe { nominal: "1 1/4", major: 41.910, tpi: 11, tap: 39.5 },
];

fn num(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

impl Size {
    /// 保存する名前。メートルは `M6x1`、ユニファイは `1/4-20 UNC`、管用は `G1/4`・`Rp1/4`。
    pub fn name(&self) -> String {
        match self.family {
            Family::Metric => format!("M{}x{}", num(self.major), num(self.pitch)),
            Family::Unified => format!(
                "{}-{:.0} {}",
                self.nominal,
                25.4 / self.pitch,
                if self.coarse { "UNC" } else { "UNF" }
            ),
            Family::Pipe => format!(
                "{}{}",
                if self.pipe_internal { "Rp" } else { "G" },
                self.nominal
            ),
        }
    }

    /// 見せる名前。メートルの区切りには × を使う。
    pub fn label(&self) -> String {
        self.name().replace('x', "×")
    }

    fn series_label(&self) -> &'static str {
        match self.family {
            Family::Metric => {
                if self.coarse {
                    "並目"
                } else {
                    "細目"
                }
            }
            Family::Unified => {
                if self.coarse {
                    "UNC"
                } else {
                    "UNF"
                }
            }
            Family::Pipe => "管用平行",
        }
    }

    /// 内ねじの代表下穴。メートル並目の表にある呼びは穴の機能と同じ値。
    pub fn tap_drill(&self) -> f64 {
        if self.family != Family::Metric {
            return self.tap;
        }
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

/// 族ごとに、径の小さい順、同じ径なら並目が先の呼び。
pub fn sizes() -> Vec<Size> {
    let mut out = Vec::new();
    for (major, coarse, fine) in METRIC {
        for (i, pitch) in std::iter::once(coarse).chain(fine.iter()).enumerate() {
            out.push(Size {
                family: Family::Metric,
                major: *major,
                pitch: *pitch,
                coarse: i == 0,
                nominal: "",
                tap: 0.0,
                pipe_internal: false,
            });
        }
    }
    for row in UNIFIED {
        for i in 0..2 {
            out.push(Size {
                family: Family::Unified,
                major: row.inch * 25.4,
                pitch: 25.4 / f64::from(row.tpi[i]),
                coarse: i == 0,
                nominal: row.nominal,
                tap: row.tap[i] * 25.4,
                pipe_internal: false,
            });
        }
    }
    for row in PIPE {
        for pipe_internal in [false, true] {
            out.push(Size {
                family: Family::Pipe,
                major: row.major,
                pitch: 25.4 / f64::from(row.tpi),
                coarse: true,
                nominal: row.nominal,
                tap: row.tap,
                pipe_internal,
            });
        }
    }
    out
}

fn fraction(s: &str) -> Option<f64> {
    let n = match s.split_once('/') {
        Some((a, b)) => a.parse::<f64>().ok()? / b.parse::<f64>().ok()?,
        None => s.parse::<f64>().ok()?,
    };
    (n.is_finite() && n > 0.0).then_some(n)
}

/// 分数と帯分数の呼びを読む。管用では呼びの数字であり、外径への換算には使わない。
fn nominal_inch(s: &str) -> Option<f64> {
    if s.trim().starts_with('-') {
        return None;
    }
    let s = s.trim().replace(['"', '″'], "");
    let s = s
        .strip_suffix("INCH")
        .or_else(|| s.strip_suffix("IN"))
        .unwrap_or(&s)
        .trim();
    if let Some(n) = s.strip_prefix('#') {
        let n = n.trim().parse::<u16>().ok()?;
        return Some(0.060 + 0.013 * f64::from(n));
    }
    let s = s
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace(" /", "/")
        .replace("/ ", "/")
        .replace('-', " ");
    let parts: Vec<_> = s.split_whitespace().collect();
    match &parts[..] {
        [whole, part] if !whole.contains('/') && part.contains('/') => {
            Some(whole.parse::<f64>().ok()? + fraction(part)?)
        }
        [single] => fraction(single),
        _ => None,
    }
}

fn unified_parts(name: &str) -> Option<(f64, u16, Option<bool>)> {
    let s = name.trim();
    let (s, coarse) = match s.strip_suffix("UNC") {
        Some(s) => (s, Some(true)),
        None => match s.strip_suffix("UNF") {
            Some(s) => (s, Some(false)),
            None => (s, None),
        },
    };
    let (nominal, tpi) = s.trim().rsplit_once('-')?;
    Some((nominal_inch(nominal)?, tpi.trim().parse().ok()?, coarse))
}

fn pipe_parts(name: &str) -> Option<(bool, &str)> {
    name.strip_prefix("RP")
        .map(|s| (true, s.trim()))
        .or_else(|| name.strip_prefix('G').map(|s| (false, s.trim())))
}

/// メートル、ユニファイ、管用平行ねじの呼びを読む。族とピッチ・山数の矛盾は受けない。
pub fn parse_size(name: &str) -> Option<Size> {
    let upper = name.trim().to_ascii_uppercase().replace('×', "X");
    if upper.starts_with('M') {
        let s: String = upper.chars().filter(|c| !c.is_whitespace()).collect();
        let s = s.strip_prefix('M')?;
        let (major, pitch) = match s.split_once('X') {
            Some((d, p)) => (d.parse::<f64>().ok()?, Some(p.parse::<f64>().ok()?)),
            None => (s.parse::<f64>().ok()?, None),
        };
        return sizes().into_iter().find(|s| {
            s.family == Family::Metric
                && (s.major - major).abs() < 1e-9
                && pitch.map_or(s.coarse, |p| (s.pitch - p).abs() < 1e-9)
        });
    }
    if let Some((internal, nominal)) = pipe_parts(&upper) {
        let d = nominal_inch(nominal)?;
        let row = PIPE
            .iter()
            .find(|r| (nominal_inch(r.nominal).unwrap_or(0.0) - d).abs() < 1e-9)?;
        return sizes().into_iter().find(|s| {
            s.family == Family::Pipe && s.nominal == row.nominal && s.pipe_internal == internal
        });
    }
    let (major, tpi, coarse) = unified_parts(&upper)?;
    sizes().into_iter().find(|s| {
        s.family == Family::Unified
            && (s.major - major * 25.4).abs() < 1e-9
            && (s.pitch - 25.4 / f64::from(tpi)).abs() < 1e-9
            && coarse.is_none_or(|c| s.coarse == c)
    })
}

/// 表に無い呼びの誤り。族に応じた近い呼びと、正しい書き方を示す。
pub fn unknown_size(name: &str) -> String {
    let head = format!("ねじの呼び「{name}」は表にありません");
    let upper = name.trim().to_ascii_uppercase().replace('×', "X");
    if let Some((internal, nominal)) = pipe_parts(&upper) {
        let prefix = if internal { "Rp" } else { "G" };
        let near = nominal_inch(nominal)
            .map(|d| {
                let below = PIPE
                    .iter()
                    .rev()
                    .find(|r| nominal_inch(r.nominal).unwrap_or(0.0) <= d);
                let above = PIPE
                    .iter()
                    .find(|r| nominal_inch(r.nominal).unwrap_or(0.0) >= d);
                let mut names: Vec<_> = below
                    .into_iter()
                    .chain(above)
                    .map(|r| format!("{prefix}{}", r.nominal))
                    .collect();
                names.dedup();
                names.join(" と ")
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("{prefix}1/8 と {prefix}1/4"));
        return format!(
            "{head}。管用の平行ねじは {prefix}1/4 のように書きます。近い呼びは {near}。テーパねじは扱いません"
        );
    }
    if upper.starts_with('R') {
        return format!(
            "{head}。テーパねじ R・Rc は扱いません。平行ねじ G1/4・Rp1/4 は別の種類です"
        );
    }
    if !upper.starts_with('M')
        && (upper.contains(['#', '/', '-']) || upper.ends_with("UNC") || upper.ends_with("UNF"))
    {
        let near = unified_parts(&upper)
            .map(|(d, _, _)| d)
            .or_else(|| nominal_inch(&upper))
            .map(|d| {
                let rows: Vec<_> =
                    if let Some(row) = UNIFIED.iter().find(|r| (r.inch - d).abs() < 1e-9) {
                        vec![row]
                    } else {
                        UNIFIED
                            .iter()
                            .rev()
                            .find(|r| r.inch < d)
                            .into_iter()
                            .chain(UNIFIED.iter().find(|r| r.inch > d))
                            .collect()
                    };
                rows.into_iter()
                    .map(|r| {
                        format!(
                            "{}-{} UNC・{}-{} UNF",
                            r.nominal, r.tpi[0], r.nominal, r.tpi[1]
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" と ")
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "#4-40 UNC・#4-48 UNF".into());
        return format!(
            "{head}。近い呼びは {near}。並目 UNC・細目 UNF と山数を合わせて書いてください(例: 1/4-20 UNC・1/4-28 UNF)"
        );
    }
    let compact: String = upper.chars().filter(|c| !c.is_whitespace()).collect();
    let d = compact
        .strip_prefix('M')
        .and_then(|r| r.split('X').next())
        .and_then(|d| d.parse::<f64>().ok())
        .filter(|d| d.is_finite() && *d > 0.0);
    let Some(d) = d else {
        return format!(
            "{head}。M と呼び径で書き、細目はピッチを付けます(例: M6、M6x0.75)。ほかの族は 1/4-20 UNC・G1/4・Rp1/4 のように書きます"
        );
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
/// 多いので、6 mm の穴は M6)。ただし、めねじの径が表の下穴とちょうど
/// 同じなら、その呼びを先にする(JC のネジ穴で開けた M6 の 5 mm は M5 でなく M6)。
pub fn suggest_for(c: &Cylinder) -> Vec<Size> {
    let mut v = suggest(c.diameter);
    if c.internal
        && let Some(i) = v.iter().position(|s| {
            (s.family != Family::Metric || s.coarse) && (s.tap_drill() - c.diameter).abs() < 1e-3
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

/// 円筒の面にねじの属性を付ける。形は変えず、対象のボディをそのまま渡す。
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
}

impl Exprs for Thread {
    fn exprs_mut(&mut self) -> Vec<&mut String> {
        self.length.iter_mut().collect()
    }
}

/// ねじの呼びを決め、注意の文を返す。
fn resolve(t: &Thread, c: &Cylinder) -> Result<(Size, Vec<String>), String> {
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
    if size.family == Family::Pipe && size.pipe_internal && !c.internal {
        return Err("Rp は管用の平行内ねじです。穴の円筒の面を選んでください".into());
    }
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
                    s.series_label(),
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
        let mut out: Outputs = [
            ("body".to_string(), Value::Shape(body)),
            ("sizes".to_string(), kmb_core::choices_value(choices)),
        ]
        .into();
        kmb_core::add_warnings(&mut out, warn);
        if let Some(mark) = mark_of(
            ctx.kernel(),
            body,
            &self.face,
            &c,
            size.pitch,
            thread_length,
        ) {
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
            .hint("円筒の面にメートルねじを付ける(形のねじ山は作らず、呼びとピッチを属性として残す)")
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
    };
    data.size = match cx.arg("size").and_then(|v| v.as_str()) {
        Some(s) => parse_size(s).ok_or_else(|| unknown_size(s))?.name(),
        None => match hole_size(cx, &data.face) {
            Some(s) => s.name(),
            None => suggest_for(&c)[0].name(),
        },
    };
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
        s.series_label(),
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
                    kmb_core::Arg::text("size", "ねじの呼び(M6・1/4-20 UNC・G1/4・Rp1/4 など。省くと面の径に近いもの)"),
                ])
                .menu("作成")
                .icon("thread")
                .aliases("ねじ ネジ thread タップ おねじ めねじ")
                .hint("押してから円筒の面(ボスの外側か穴の内側)をクリックすると、面の径に近いメートルねじの呼びとピッチが入る。呼び・長さ・向きは欄で直し、OK か Enter で決める")
                // 作成の一覧に出し、帯の前には出さない。
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
    fn expanded_thread_families_parse_and_keep_their_designations() {
        for (name, major, tpi, canonical) in [
            (" 1/4 - 20 unc ", 6.35, 20.0, "1/4-20 UNC"),
            ("#10-32 unf", 4.826, 32.0, "#10-32 UNF"),
            ("1-1/2-6", 38.1, 6.0, "1-1/2-6 UNC"),
            ("g 1 / 8", 9.728, 28.0, "G1/8"),
            ("Rp 1 1/4", 41.91, 11.0, "Rp1 1/4"),
        ] {
            let size = parse_size(name).unwrap_or_else(|| panic!("{name} が読める"));
            assert!((size.major - major).abs() < 1e-9, "{name}");
            assert!((size.pitch - 25.4 / tpi).abs() < 1e-9, "{name}");
            assert_eq!(size.name(), canonical);
            assert_eq!(parse_size(&size.name()), Some(size));
        }
        assert!(
            parse_size("#10-24 UNF").is_none(),
            "並目と細目を取り違えない"
        );
    }

    #[test]
    fn expanded_thread_spellings_and_invalid_series_are_unambiguous() {
        for (name, canonical) in [
            ("1/4-20", "1/4-20 UNC"),
            (" 1 / 4 in - 28 unf ", "1/4-28 UNF"),
            ("1/4\"-20 UNC", "1/4-20 UNC"),
            ("1/4″-20 unc", "1/4-20 UNC"),
            ("1 1/2-12 UNF", "1-1/2-12 UNF"),
            ("1.5-6", "1-1/2-6 UNC"),
            ("g 1-1/4", "G1 1/4"),
            ("rP 1 / 4", "Rp1/4"),
            (" m 6 × 0.75 ", "M6x0.75"),
        ] {
            assert_eq!(parse_size(name).unwrap().name(), canonical, "{name}");
        }
        for name in [
            "1/4-0",
            "1/4-20 UNF",
            "#10-24 UNF",
            "G0",
            "Rp-1/4",
            "-1/4-20",
            "1/0-20",
            "GNaN",
            "R1/4",
            "Rc1/4",
        ] {
            assert!(parse_size(name).is_none(), "{name}");
        }
    }

    #[test]
    fn expanded_thread_tables_use_basic_dimensions_and_separate_tap_drills() {
        for size in sizes() {
            assert_eq!(parse_size(&size.name()), Some(size));
            assert!(
                size.tap_drill() > 0.0 && size.tap_drill() < size.major,
                "{}",
                size.name()
            );
            if size.family == Family::Unified {
                let minor = size.major - 1.08253175 * size.pitch;
                assert!(size.tap_drill() > minor, "{}", size.name());
            }
        }
        for (nominal, minor) in [("G1/8", 8.566), ("G1/4", 11.445), ("G1 1/4", 38.952)] {
            let size = parse_size(nominal).unwrap();
            assert!((size.major - 1.280654 * size.pitch - minor).abs() < 0.002);
        }
        let inch = parse_size("1/4-20 UNC").unwrap();
        assert_eq!(inch.family, Family::Unified);
        assert!((inch.major - 0.25 * 25.4).abs() < 1e-12);
        assert!((inch.tap_drill() - 0.201 * 25.4).abs() < 1e-12);
        assert_eq!(parse_size("G1/4").unwrap().family, Family::Pipe);
        assert_eq!(parse_size("M6").unwrap().family, Family::Metric);
    }

    #[test]
    fn expanded_thread_unknown_names_point_to_sizes_in_the_same_family() {
        let m = unknown_size("#10-24 UNF");
        assert!(m.contains("#10-24 UNC") && m.contains("#10-32 UNF"), "{m}");
        let m = unknown_size("1/3-20 UNC");
        assert!(m.contains("5/16-18 UNC") && m.contains("3/8-16 UNC"), "{m}");
        let m = unknown_size("G1/3");
        assert!(m.contains("G1/4") && m.contains("G3/8"), "{m}");
        let m = unknown_size("Rp1/3");
        assert!(m.contains("Rp1/4") && m.contains("Rp3/8"), "{m}");
    }

    #[test]
    fn expanded_thread_choices_and_saved_metric_data_keep_their_contract() {
        let field = crate::hole_fields()
            .into_iter()
            .find(|f| f.key == "size")
            .unwrap();
        let Some(kmb_core::Suggest::Values { values }) = field.suggest else {
            panic!()
        };
        assert_eq!(values[0].value, crate::DEFAULT_SIZE);
        for name in [
            "M14",
            "M16",
            "M20",
            "M24",
            "M30",
            "1/4-20 UNC",
            "#10-32 UNF",
            "G1/4",
            "Rp1/4",
        ] {
            assert!(values.iter().any(|v| v.value == name), "{name}");
        }
        let old = serde_json::json!({"target":1,"face":"f1:side","size":"M6x1"});
        let thread: Thread = serde_json::from_value(old.clone()).unwrap();
        assert_eq!(serde_json::to_value(thread).unwrap(), old);
    }

    #[test]
    fn expanded_thread_rp_is_only_an_internal_parallel_thread() {
        let cylinder = Cylinder {
            diameter: 13.157,
            center: [0.0; 3],
            axis: [0.0, 0.0, 1.0],
            length: 10.0,
            internal: false,
        };
        let thread = Thread {
            target: FeatureId(1),
            face: "f1:side".into(),
            size: "Rp1/4".into(),
            length: None,
            hand: Hand::Right,
        };
        assert!(
            resolve(&thread, &cylinder)
                .unwrap_err()
                .contains("穴の円筒")
        );
        assert!(
            resolve(
                &thread,
                &Cylinder {
                    internal: true,
                    ..cylinder
                }
            )
            .is_ok()
        );
        assert!(
            resolve(
                &Thread {
                    size: "G1/4".into(),
                    ..thread
                },
                &cylinder
            )
            .is_ok()
        );
        assert!(unknown_size("MNaN").contains("M6x0.75"));
    }

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
        // 表に無い呼びは、近い呼びと細目の書き方の例まで言う。
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
}
