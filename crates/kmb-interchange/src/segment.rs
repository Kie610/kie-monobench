//! スケッチの線分(世界座標の両端)。3D スケッチが出力 [`SEGMENTS_OUTPUT`] に入れ、回転・パターン・
//! 移動が軸として読む。線は番号で指すので、スケッチを直しても同じ線を追う。

use kmb_core::kernel::{self as k, P3};
use kmb_core::{Context, EvalCtx, EvalError, FeatureId, Payload};
use serde::{Deserialize, Serialize};

/// スケッチの線分を入れる出力の名前。
pub const SEGMENTS_OUTPUT: &str = "segments";

/// スケッチの線分の一覧(補助線も含む)。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Segments(pub Vec<Segment>);

impl Payload for Segments {
    const TYPE: &'static str = "sketch.segments@1";
}

/// スケッチの線分 1 本。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    /// スケッチの中の線の番号
    pub line: u32,
    /// 両端(世界座標。始めの点 → 終わりの点)
    pub ends: [P3; 2],
}

/// 機能 `feature` の線 `line` を通る点と向き(この機能の入力に `feature` の [`SEGMENTS_OUTPUT`] を
/// 入れておく)。線が無いか長さが 0 なら誤り(別の線に替えない)。
pub fn segment_axis(
    ctx: &EvalCtx<'_>,
    feature: FeatureId,
    line: u32,
) -> Result<(P3, P3), EvalError> {
    let all = Segments::from_value(ctx.output(feature, SEGMENTS_OUTPUT)?).ok_or_else(|| {
        EvalError::InvalidData(format!(
            "機能 {} は線を出すスケッチではありません",
            feature.0
        ))
    })?;
    let s = all.0.iter().find(|s| s.line == line).ok_or_else(|| {
        EvalError::InvalidData(format!(
            "スケッチ {} に軸の線 {line} がありません。軸を選び直してください",
            feature.0
        ))
    })?;
    line_of(s).ok_or_else(|| EvalError::InvalidData("軸の線の長さが 0 です".into()))
}

/// 線分の通る点と向き(長さ 0 なら `None`)。
pub fn line_of(s: &Segment) -> Option<(P3, P3)> {
    let d = k::sub(s.ends[1], s.ends[0]);
    (k::dot(d, d) > 1e-18).then_some((s.ends[0], d))
}

/// 画面の計算済みの結果から、機能 `feature` の線分(線を出す機能でないか、まだ計算できていなければ
/// `None`)。
pub fn segments_now(cx: &Context<'_>, feature: FeatureId) -> Option<Segments> {
    Segments::from_value(cx.output(feature, SEGMENTS_OUTPUT)?)
}
