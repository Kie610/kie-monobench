//! 辺をまたぐ面のつながり: 辺の両側の面が、位置・接線(法線の向き)・曲率でそろうか。
//!
//! 辺に沿った点ごとに、両側の面の点の隔たり(mm)、法線の角度の差(deg)、辺を横切る向きの
//! 法曲率の差(1/mm)を求め、許しと比べて区分を決める。辺全体の区分は、いちばん悪い点の区分。

use kmb_core::kernel::P3;
use serde::{Deserialize, Serialize};

use crate::vec::{cross, dot, norm, scale, sub, unit};

/// 面の 1 点の評価: 位置、媒介変数 (u, v) による 1 階・2 階の偏微分、立体の外へ向く単位の法線。
/// 法線の向きは面の裏表を決めるためだけに使う(大きさと細かい向きは偏微分から求め直す)。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SurfaceJet {
    pub p: P3,
    pub su: P3,
    pub sv: P3,
    pub suu: P3,
    pub suv: P3,
    pub svv: P3,
    pub outward: P3,
}

impl SurfaceJet {
    /// 偏微分から求めた単位の法線を、`outward` と同じ側に向けた物。偏微分が平行なら `None`。
    pub fn normal(&self) -> Option<P3> {
        let n = unit(cross(self.su, self.sv))?;
        Some(if dot(n, self.outward) < 0.0 {
            scale(n, -1.0)
        } else {
            n
        })
    }
}

/// 面の点 `j` での、接平面の向き `dir` の法曲率(1/mm)。面が外向きの法線の側へ曲がる(へこむ)
/// 時を正、法線と反対へ曲がる(出っ張る)時を負にする。`dir` は接平面に垂直な成分を無視し、
/// 向きの正負によらない。求められない(偏微分が平行・向きが法線と同じ)なら `None`。
pub fn normal_curvature(j: &SurfaceJet, dir: P3) -> Option<f64> {
    let n = j.normal()?;
    let (e, f, g) = (dot(j.su, j.su), dot(j.su, j.sv), dot(j.sv, j.sv));
    let det = e * g - f * f;
    if det.abs() < 1e-18 {
        return None;
    }
    // dir ≈ a·su + b·sv を最小二乗で解く(接平面に垂直な成分は消える)
    let (p, q) = (dot(dir, j.su), dot(dir, j.sv));
    let a = (g * p - f * q) / det;
    let b = (e * q - f * p) / det;
    let first = e * a * a + 2.0 * f * a * b + g * b * b;
    if first < 1e-18 {
        return None;
    }
    let (l, m, nn) = (dot(j.suu, n), dot(j.suv, n), dot(j.svv, n));
    Some((l * a * a + 2.0 * m * a * b + nn * b * b) / first)
}

/// 辺の 1 点での評価: 辺の上の点と接線、両側の面それぞれでその点にいちばん近い点の評価。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EdgeSample {
    pub p: P3,
    pub tangent: P3,
    pub faces: [SurfaceJet; 2],
}

/// つながりの区分。悪い順に並べ、比べると悪い方が小さい。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Continuity {
    /// 面の評価が壊れていて求められない
    Unknown,
    /// 両側の面が離れている(隙間)
    Gap,
    /// 位置だけそろう(角がある)
    Position,
    /// 接線までそろう(なめらかだが曲率が跳ぶ)
    Tangent,
    /// 曲率までそろう
    Curvature,
}

/// 区分を決める許し。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Tolerances {
    /// 両側の面の点の隔たり(mm)。これ以下なら位置がそろう
    pub gap: f64,
    /// 法線の角度の差(deg)。これ以下なら接線がそろう
    pub angle_deg: f64,
    /// 法曲率の差の、大きい方の曲率に対する割合。これ以下なら曲率がそろう
    pub curvature_ratio: f64,
    /// 法曲率の差(1/mm)がこれ以下なら、割合によらず曲率がそろうとみなす(ほぼ平らな面どうしで
    /// 小さな数の割合に振り回されないため)
    pub curvature_floor: f64,
}

impl Default for Tolerances {
    /// 位置 0.001 mm、接線 0.5°、曲率 5 %(差が 1e-4/mm 以下ならそろう。半径 10 m の差に当たる)。
    fn default() -> Self {
        Tolerances {
            gap: 1e-3,
            angle_deg: 0.5,
            curvature_ratio: 0.05,
            curvature_floor: 1e-4,
        }
    }
}

/// 辺の 1 点のつながり。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SampleContinuity {
    pub p: P3,
    pub class: Continuity,
    /// 両側の面の点の隔たり(mm)
    pub gap: f64,
    /// 両側の面の法線の角度の差(deg)
    pub angle_deg: f64,
    /// 辺を横切る向きの、両側の面の法曲率(1/mm)。求められなければ `None`
    pub curvature: Option<[f64; 2]>,
}

impl SampleContinuity {
    /// 両側の法曲率の差の大きさ(1/mm)。
    pub fn curvature_diff(&self) -> Option<f64> {
        self.curvature.map(|[a, b]| (a - b).abs())
    }
}

/// 辺の 1 点のつながりを求める。
pub fn sample_continuity(s: &EdgeSample, tol: &Tolerances) -> SampleContinuity {
    let [fa, fb] = &s.faces;
    let gap = norm(sub(fa.p, fb.p));
    let unknown = SampleContinuity {
        p: s.p,
        class: Continuity::Unknown,
        gap,
        angle_deg: f64::NAN,
        curvature: None,
    };
    let (Some(na), Some(nb)) = (fa.normal(), fb.normal()) else {
        return unknown;
    };
    let angle_deg = dot(na, nb).clamp(-1.0, 1.0).acos().to_degrees();
    // 辺を横切る向き: 辺の接線と平均の法線の両方に垂直。法曲率は向きの正負によらないので、
    // 両側の面で同じ向きを使える。接線がそろう辺では、辺を横切る 1 つの向きの法曲率がそろえば
    // どの向きの法曲率もそろう(辺に沿う向きは共有の曲線なので初めからそろう)ので、この 1 つで足りる
    let across = unit(cross(s.tangent, add_unit(na, nb)));
    let curvature = across.and_then(|c| Some([normal_curvature(fa, c)?, normal_curvature(fb, c)?]));
    let class = if gap > tol.gap {
        Continuity::Gap
    } else if angle_deg > tol.angle_deg {
        Continuity::Position
    } else {
        match curvature {
            Some([a, b]) => {
                let diff = (a - b).abs();
                if diff <= tol.curvature_floor || diff <= tol.curvature_ratio * a.abs().max(b.abs())
                {
                    Continuity::Curvature
                } else {
                    Continuity::Tangent
                }
            }
            None => Continuity::Tangent,
        }
    };
    SampleContinuity {
        p: s.p,
        class,
        gap,
        angle_deg,
        curvature,
    }
}

/// 2 つの単位の法線の平均の向き(足して長さ 1 に)。真逆なら最初の法線を使う。
fn add_unit(a: P3, b: P3) -> P3 {
    unit([a[0] + b[0], a[1] + b[1], a[2] + b[2]]).unwrap_or(a)
}

/// 辺 1 本のつながり。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EdgeContinuity {
    /// 辺全体の区分(いちばん悪い点の区分)。点が無ければ [`Continuity::Unknown`]
    pub class: Continuity,
    pub samples: Vec<SampleContinuity>,
    /// 点のうちの最大の隔たり(mm)
    pub max_gap: f64,
    /// 点のうちの最大の角度の差(deg)
    pub max_angle_deg: f64,
    /// 点のうちの最大の法曲率の差(1/mm)。どの点でも求められなければ `None`
    pub max_curvature_diff: Option<f64>,
}

/// 辺に沿って評価した点の並びから、辺 1 本のつながりを求める。
pub fn edge_continuity(samples: &[EdgeSample], tol: &Tolerances) -> EdgeContinuity {
    let samples: Vec<SampleContinuity> =
        samples.iter().map(|s| sample_continuity(s, tol)).collect();
    let class = samples
        .iter()
        .map(|s| s.class)
        .min()
        .unwrap_or(Continuity::Unknown);
    let max_gap = samples.iter().map(|s| s.gap).fold(0.0, f64::max);
    let max_angle_deg = samples
        .iter()
        .map(|s| s.angle_deg)
        .filter(|a| a.is_finite())
        .fold(0.0, f64::max);
    let max_curvature_diff = samples
        .iter()
        .filter_map(SampleContinuity::curvature_diff)
        .reduce(f64::max);
    EdgeContinuity {
        class,
        samples,
        max_gap,
        max_angle_deg,
        max_curvature_diff,
    }
}
