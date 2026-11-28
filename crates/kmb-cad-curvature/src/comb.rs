//! 曲線の曲率のくし。
//!
//! くしは、曲線の点ごとに曲率の大きさに比例した長さの棘を、曲率の中心と反対の側へ立てた物。
//! 棘の先を結んだ線(包絡線)が波打つ所・跳ぶ所で、曲率の変わり方の悪い所が見える。平面の
//! 曲線は平面の向きを基準に曲率に符号を付けるので、変曲点で棘が曲線の反対の側へ渡る。

use kmb_core::kernel::P3;
use serde::{Deserialize, Serialize};

use crate::vec::{add, cross, dot, norm, scale, unit};

/// 曲線の 1 点の評価: 位置と、媒介変数による 1 階・2 階の微分。媒介変数の取り方によらず
/// 同じ曲率になる。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CurveJet {
    pub p: P3,
    pub d1: P3,
    pub d2: P3,
}

/// 曲率(1/mm、0 以上)と、曲率の中心へ向かう単位の向き(主法線)。曲率がほぼ 0 の所は向きが
/// 決まらないので、向きは 1 階の微分に垂直な任意の向きではなく `None` にする。1 階の微分が
/// ほぼ 0(媒介変数が止まる特異な点)なら、曲率そのものを `None` にする。
pub fn curvature(j: &CurveJet) -> Option<(f64, Option<P3>)> {
    let speed = norm(j.d1);
    if speed < 1e-12 {
        return None;
    }
    let b = cross(j.d1, j.d2);
    let k = norm(b) / speed.powi(3);
    Some((k, unit(cross(b, j.d1))))
}

/// 棘の長さの決め方。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CombScale {
    /// 曲率 1/mm あたりの棘の長さ(mm²)。違う曲線どうしを同じ物差しで比べる時に使う
    Fixed(f64),
    /// いちばん長い棘をこの長さ(mm)にそろえる。曲線ごとに見やすい長さで出す既定の決め方
    Fit(f64),
}

/// くしの棘 1 本。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CombSpine {
    /// 曲線の上の根元
    pub at: P3,
    /// 棘の先
    pub tip: P3,
    /// 曲率(1/mm)。平面の曲線では、平面の向きから見て左へ曲がる所を正、右へ曲がる所を負にする
    pub curvature: f64,
}

/// 曲線 1 本のくし。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Comb {
    pub spines: Vec<CombSpine>,
    /// 棘の先を順に結んだ線(画面はこれを棘と同じ色で描く)
    pub envelope: Vec<P3>,
    /// 使った倍率(曲率 1/mm あたりの棘の長さ mm²)
    pub scale: f64,
    /// 曲率の大きさのいちばん大きい値(1/mm)。札に「最小の半径」として出す
    pub max_curvature: f64,
    /// 微分がほぼ 0 で曲率を求められず、棘を立てなかった点の数
    pub skipped: usize,
}

impl Comb {
    /// いちばん小さい曲率半径(mm)。直線だけなら `None`。
    pub fn min_radius(&self) -> Option<f64> {
        (self.max_curvature > 1e-12).then(|| 1.0 / self.max_curvature)
    }
}

/// 曲線を評価した点の並び `jets`(曲線に沿って順に、等しい間隔で)からくしを作る。
/// `plane` は平面の曲線(スケッチの線)の平面の法線。空間の曲線(ボディの辺)では `None` にし、
/// 棘は主法線の反対へ立てる(曲率に符号を付けない)。
pub fn comb(jets: &[CurveJet], plane: Option<P3>, how: CombScale) -> Comb {
    let plane = plane.and_then(unit);
    // (根元, 棘の向き(曲率の大きさを掛ける前の単位の向き), 符号つきの曲率)
    let mut raw: Vec<(P3, P3, f64)> = Vec::with_capacity(jets.len());
    let mut skipped = 0;
    for j in jets {
        let Some((k, toward_center)) = curvature(j) else {
            skipped += 1;
            continue;
        };
        let t = unit(j.d1).expect("curvature は速さが 0 でない時だけ値を返す");
        match plane {
            Some(n) => {
                // 平面の中で進む向きの左。左へ曲がる(中心が左にある)なら曲率は正
                let left = cross(n, t);
                let signed = match toward_center {
                    Some(c) if dot(c, left) < 0.0 => -k,
                    _ => k,
                };
                // 棘は中心と反対へ立てる: 正の曲率(中心が左)なら右へ
                raw.push((j.p, scale(left, -1.0), signed));
            }
            None => {
                let out = match toward_center {
                    Some(c) => scale(c, -1.0),
                    // 曲率が 0 の所は棘の長さも 0 なので、向きは何でもよい
                    None => [0.0; 3],
                };
                raw.push((j.p, out, k));
            }
        }
    }
    let max_curvature = raw.iter().map(|r| r.2.abs()).fold(0.0, f64::max);
    let s = match how {
        CombScale::Fixed(s) => s,
        CombScale::Fit(len) if max_curvature > 1e-12 => len / max_curvature,
        CombScale::Fit(_) => 0.0,
    };
    let spines: Vec<CombSpine> = raw
        .into_iter()
        .map(|(at, dir, k)| CombSpine {
            at,
            tip: add(at, scale(dir, k * s)),
            curvature: k,
        })
        .collect();
    let envelope = spines.iter().map(|s| s.tip).collect();
    Comb {
        spines,
        envelope,
        scale: s,
        max_curvature,
        skipped,
    }
}
