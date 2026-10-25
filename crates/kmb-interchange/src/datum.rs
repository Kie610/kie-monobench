//! 作業平面・作業軸・作業点。それぞれの機能が出力 [`DATUM_OUTPUT`] に入れ、スケッチは置く平面、
//! 回転・パターン・移動は軸、作業平面・作業軸の作り方は点として読む。平面の X・Y の向きは作業平面を
//! 作る時に決めておき、読む側はそのまま使う(浮かせる距離が負でも、読む側で向きを決め直さない)。

use kmb_core::kernel::P3;
use kmb_core::{Context, EvalCtx, EvalError, FeatureId, HandleChoice, Payload, Plane};
use serde::{Deserialize, Serialize};

/// 作業平面・作業軸・作業点を入れる出力の名前。
pub const DATUM_OUTPUT: &str = "datum";

/// 作業平面の形。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DatumPlane {
    /// 原点と X・Y の向き(世界座標)
    pub plane: Plane,
    /// 画面に描く四角(平面の座標の最小・最大。mm)
    pub rect: [[f64; 2]; 2],
}

impl Payload for DatumPlane {
    const TYPE: &'static str = "datum.plane@1";
}

/// 機能 `feature` の作業平面を読む(この機能の入力に `feature` の [`DATUM_OUTPUT`] を入れておく)。
/// 作業平面を作れなかった時はその誤りを返す(別の平面に置き換えない)。
pub fn datum_plane(ctx: &EvalCtx<'_>, feature: FeatureId) -> Result<Plane, EvalError> {
    DatumPlane::from_value(ctx.output(feature, DATUM_OUTPUT)?)
        .map(|d| d.plane)
        .ok_or_else(|| {
            EvalError::InvalidData(format!("機能 {} は作業平面ではありません", feature.0))
        })
}

impl DatumPlane {
    /// 画面に描く四角の角(世界座標、順に回る 4 点)。
    pub fn corners(&self) -> [[f64; 3]; 4] {
        let [lo, hi] = self.rect;
        [lo, [hi[0], lo[1]], hi, [lo[0], hi[1]]].map(|uv| self.plane.to_world(uv))
    }
}

/// 文書の作業平面(計算できた物だけ。機能の番号と名前と形)。作業平面の出力を持つ機能を全部見る
/// (機能の種類の名前は見ない)。
pub fn datum_planes(cx: &Context<'_>) -> Vec<(FeatureId, String, DatumPlane)> {
    cx.data
        .features
        .iter()
        .filter(|f| !f.suppressed)
        .filter_map(|f| {
            let d = DatumPlane::from_value(cx.output(f.id, DATUM_OUTPUT)?)?;
            Some((f.id, f.name.clone(), d))
        })
        .collect()
}

/// 平面を選ぶ手順の候補にする作業平面(値は機能の番号。四角の内側をクリックしても選べる)。
pub fn datum_choices(cx: &Context<'_>) -> Vec<HandleChoice> {
    datum_planes(cx)
        .into_iter()
        .map(|(id, name, d)| {
            let c = d.corners();
            HandleChoice {
                value: serde_json::json!(id.0),
                label: name,
                outline: vec![vec![c[0], c[1], c[2], c[3], c[0]]],
                ..Default::default()
            }
        })
        .collect()
}

/// 選んでいる機能が作業平面ならその番号。
pub fn selected_datum(cx: &Context<'_>) -> Option<FeatureId> {
    let id = cx.selected?;
    cx.output(id, DATUM_OUTPUT)
        .and_then(DatumPlane::from_value)
        .map(|_| id)
}

/// 作業軸の形。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DatumAxis {
    /// 通る点(世界座標)
    pub at: P3,
    /// 向き(長さ 1)
    pub dir: P3,
    /// 画面に描く線の範囲(`at` からの向きに沿った距離の最小・最大。mm)
    pub span: [f64; 2],
}

impl Payload for DatumAxis {
    const TYPE: &'static str = "datum.axis@1";
}

impl DatumAxis {
    /// 画面に描く線の両端(世界座標)。
    pub fn ends(&self) -> [P3; 2] {
        self.span
            .map(|t| [0, 1, 2].map(|i| self.at[i] + self.dir[i] * t))
    }
}

/// 作業点の形。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DatumPoint {
    /// 位置(世界座標)
    pub at: P3,
}

impl Payload for DatumPoint {
    const TYPE: &'static str = "datum.point@1";
}

/// 機能 `feature` の作業軸(通る点と長さ 1 の向き)を読む。作れなかった時はその誤りを返す。
pub fn datum_axis(ctx: &EvalCtx<'_>, feature: FeatureId) -> Result<(P3, P3), EvalError> {
    DatumAxis::from_value(ctx.output(feature, DATUM_OUTPUT)?)
        .map(|d| (d.at, d.dir))
        .ok_or_else(|| EvalError::InvalidData(format!("機能 {} は作業軸ではありません", feature.0)))
}

/// 機能 `feature` の作業点の位置を読む。作れなかった時はその誤りを返す。
pub fn datum_point(ctx: &EvalCtx<'_>, feature: FeatureId) -> Result<P3, EvalError> {
    DatumPoint::from_value(ctx.output(feature, DATUM_OUTPUT)?)
        .map(|d| d.at)
        .ok_or_else(|| EvalError::InvalidData(format!("機能 {} は作業点ではありません", feature.0)))
}

/// 文書の作業軸か作業点(計算できた物だけ。機能の番号と名前と形)。`before` があれば、その機能より
/// 前の物だけ(後ろの物を指すと計算の順が回る)。
fn datums<T: Payload>(cx: &Context<'_>, before: Option<FeatureId>) -> Vec<(FeatureId, String, T)> {
    let end = before
        .and_then(|b| cx.data.features.iter().position(|f| f.id == b))
        .unwrap_or(cx.data.features.len());
    cx.data.features[..end]
        .iter()
        .filter(|f| !f.suppressed)
        .filter_map(|f| {
            Some((
                f.id,
                f.name.clone(),
                T::from_value(cx.output(f.id, DATUM_OUTPUT)?)?,
            ))
        })
        .collect()
}

/// 文書の作業軸([`datums`])。
pub fn datum_axes(
    cx: &Context<'_>,
    before: Option<FeatureId>,
) -> Vec<(FeatureId, String, DatumAxis)> {
    datums(cx, before)
}

/// 文書の作業点([`datums`])。
pub fn datum_points(
    cx: &Context<'_>,
    before: Option<FeatureId>,
) -> Vec<(FeatureId, String, DatumPoint)> {
    datums(cx, before)
}

/// 軸を選ぶ手順の候補にする作業軸(値は `{"datum": 機能の番号}`。線の近くをクリックして選ぶ)。
pub fn datum_axis_choices(cx: &Context<'_>, before: Option<FeatureId>) -> Vec<HandleChoice> {
    datum_axes(cx, before)
        .into_iter()
        .map(|(id, name, d)| HandleChoice {
            value: serde_json::json!({"datum": id.0}),
            label: name,
            outline: vec![d.ends().to_vec()],
            ..Default::default()
        })
        .collect()
}
