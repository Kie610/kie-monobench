//! 曲線の曲率のくしと、辺をまたぐ面のつながりを求める純関数。
//!
//! 形状カーネルにも文書にも触らない。曲線は点と 1 階・2 階の微分([`CurveJet`])、面は点と
//! 1 階・2 階の偏微分と外向きの法線([`SurfaceJet`])で受け取り、画面に重ねる棘の線と、辺の
//! つながりの区分・数を返す。どこで評価するか(点の並び)は呼ぶ側が決める。くしの棘は曲線に沿って
//! 等しい間隔で並べ、曲率に応じて密にしない(`docs/避ける形.md`)。
//!
//! 長さは mm、曲率は 1/mm、角度は deg。

mod comb;
mod continuity;
mod vec;

pub use comb::{Comb, CombScale, CombSpine, CurveJet, comb, curvature};
pub use continuity::{
    Continuity, EdgeContinuity, EdgeSample, SampleContinuity, SurfaceJet, Tolerances,
    edge_continuity, normal_curvature, sample_continuity,
};

#[cfg(test)]
mod tests;
