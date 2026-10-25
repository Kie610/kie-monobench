//! 3D の加工に共通の欄: 径方向に残す厚さ(H6)、加工の範囲(H4)、傾斜の範囲(H5)。
//!
//! 走査線・等高線・ペンシル・3D 粗取りの機能のデータに `#[serde(flatten)]` で入れる。欄が
//! どれも空なら、径方向の残す厚さは 0、範囲は部品の外形の箱、傾斜は限らない(これらの欄の無い
//! 文書も同じ経路になる)。
//!
//! - 径方向に残す厚さ: 工具をその厚さだけ太らせた形([`crate::Cutter`])を下ろす。軸方向(上向き)の
//!   残す厚さは先端の高さに足す。両方を同じにすると、面のどの向きにも同じ厚さが残る。
//! - 加工の範囲: シルエット(部品を上から見た外形)か、選んだ面を上から見た外形。工具の中心が
//!   その中・工具が丸ごと中(半径だけ内側)・工具が触れる所まで(半径だけ外側)を選び、さらに
//!   追加のずらし(正なら外へ)を足す。
//! - 傾斜の範囲: 工具を下ろした高さの面の傾き(ボールなら当たる所の部品の面の傾き)が、開始の
//!   角度から終了の角度までの所だけを削る。

use crate::region::{Region, areas_from_loops, mask_loops};
use crate::{DropField, Heights};
use kmb_cam_geom::{Area, MachiningModel, offset_area};
use kmb_core::{Dim, EvalCtx, EvalError, Field};
use kmb_toolpath_moves::length;
use serde::{Deserialize, Serialize};

/// 加工の範囲の決め方。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Boundary {
    /// 限らない(部品の外形の箱 + 工具の半径。前からの動き)
    #[default]
    None,
    /// 部品を上から見た外形
    Silhouette,
    /// 選んだ面を上から見た外形
    Faces,
}

impl Boundary {
    pub fn is_none(&self) -> bool {
        *self == Boundary::None
    }
}

/// 範囲に対する工具の置き方。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Contain {
    /// 工具の中心が範囲の中
    #[default]
    Center,
    /// 工具が丸ごと範囲の中(半径だけ内側)
    Inside,
    /// 工具が範囲に触れる所まで(半径だけ外側)
    Outside,
}

impl Contain {
    pub fn is_center(&self) -> bool {
        *self == Contain::Center
    }
}

/// 3D の加工に共通の欄(機能のデータに平らに入れる)。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Limits {
    /// 径方向(面から横の向き)に残す厚さ。空なら 0(残す厚さは上向きだけ。前からの文書と同じ)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leave_radial: Option<String>,
    /// 加工の範囲
    #[serde(default, skip_serializing_if = "Boundary::is_none")]
    pub boundary: Boundary,
    /// 範囲を決める面の名前(`boundary` が `faces` の時)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub faces: Vec<String>,
    /// 範囲に対する工具の置き方
    #[serde(default, skip_serializing_if = "Contain::is_center")]
    pub contain: Contain,
    /// 範囲の追加のずらし(正なら外へ広げる)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundary_offset: Option<String>,
    /// 傾斜の開始角度(これよりなだらかな所は削らない)。空なら 0°
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slope_from: Option<String>,
    /// 傾斜の終了角度(これより急な所は削らない)。空なら 90°
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slope_to: Option<String>,
}

/// 欄を数にしたもの。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Resolved {
    /// 径方向に残す厚さ(mm)
    pub leave_radial: f64,
    /// 工具の中心が動いてよい所。`None` なら限らない
    pub region: Option<Region>,
    /// 削る傾斜の範囲(ラジアン、下限と上限)。`None` なら限らない
    pub slope: Option<(f64, f64)>,
}

impl Resolved {
    /// 点 `p` に工具の中心を置いてよいか(範囲)。
    pub fn inside(&self, p: [f64; 2]) -> bool {
        self.region.as_ref().is_none_or(|r| r.contains(p))
    }

    /// 傾き `a`(ラジアン)の所を削るか。
    pub fn slope_ok(&self, a: f64) -> bool {
        self.slope
            .is_none_or(|(lo, hi)| a >= lo - 1e-9 && a <= hi + 1e-9)
    }

    /// 範囲か傾斜で、削る所を限るか。
    pub fn limits_anything(&self) -> bool {
        self.region.is_some() || self.slope.is_some()
    }

    /// 点 `p` を削るか(範囲と、`field` で測った傾き)。
    pub fn keep(&self, field: &DropField, p: [f64; 2]) -> bool {
        self.inside(p)
            && (self.slope.is_none()
                || self.slope_ok(field.slope_at(p, slope_span(field.heights.cell))))
    }
}

/// 傾きを測る前後の節点の数(0.2 mm ほど。格子が粗ければ 1 節点)。
pub fn slope_span(cell: f64) -> usize {
    ((0.2 / cell).round() as usize).clamp(1, 8)
}

fn invalid(m: impl Into<String>) -> EvalError {
    EvalError::InvalidData(m.into())
}

/// 角度の式を読む(ラジアン)。
fn angle(ctx: &EvalCtx<'_>, src: &str, what: &str) -> Result<f64, EvalError> {
    let q = ctx.expr(src)?;
    if q.dim != Dim::ANGLE {
        return Err(invalid(format!(
            "{what} `{src}` は角度の単位(deg)が必要です(値: {q})"
        )));
    }
    Ok(q.value)
}

impl Limits {
    /// 式の欄(依存を集めるのに使う)。
    pub fn exprs(&self) -> impl Iterator<Item = &String> {
        [
            &self.leave_radial,
            &self.boundary_offset,
            &self.slope_from,
            &self.slope_to,
        ]
        .into_iter()
        .flatten()
    }

    /// 式の欄(パラメータの名前を書き換えるのに使う)。
    pub fn exprs_mut(&mut self) -> impl Iterator<Item = &mut String> {
        [
            &mut self.leave_radial,
            &mut self.boundary_offset,
            &mut self.slope_from,
            &mut self.slope_to,
        ]
        .into_iter()
        .flatten()
    }

    /// 欄を数にする。`cell` は範囲を求める格子の幅。
    pub fn resolve(
        &self,
        ctx: &EvalCtx<'_>,
        p: &(impl kmb_toolpath_moves::Cutter + AsRef<MachiningModel>),
        cell: f64,
    ) -> Result<Resolved, EvalError> {
        let leave_radial = match &self.leave_radial {
            Some(s) => length(ctx, s, "径方向の残し量")?,
            None => 0.0,
        };
        if leave_radial < 0.0 {
            return Err(invalid("径方向に残す厚さは 0 以上にしてください"));
        }
        let offset = match &self.boundary_offset {
            Some(s) => length(ctx, s, "範囲のオフセット")?,
            None => 0.0,
        };
        let from = match &self.slope_from {
            Some(s) => Some(angle(ctx, s, "傾斜の開始角度")?),
            None => None,
        };
        let to = match &self.slope_to {
            Some(s) => Some(angle(ctx, s, "傾斜の終了角度")?),
            None => None,
        };
        let slope = slope_range(from, to)?;
        let region = region_of(p, self.boundary, &self.faces, self.contain, offset, cell)?;
        Ok(Resolved {
            leave_radial,
            region,
            slope,
        })
    }
}

/// 傾斜の範囲(ラジアン)。どちらも空なら `None`。
pub fn slope_range(from: Option<f64>, to: Option<f64>) -> Result<Option<(f64, f64)>, EvalError> {
    if from.is_none() && to.is_none() {
        return Ok(None);
    }
    let right = std::f64::consts::FRAC_PI_2;
    let (lo, hi) = (from.unwrap_or(0.0), to.unwrap_or(right));
    if !(0.0..=right + 1e-9).contains(&lo) || !(0.0..=right + 1e-9).contains(&hi) {
        return Err(invalid("傾斜の角度は 0〜90 deg にしてください"));
    }
    if lo >= hi {
        return Err(invalid("傾斜の開始角度は終了角度より小さくしてください"));
    }
    Ok(Some((lo, hi)))
}

/// 加工の範囲(工具の中心が動いてよい所)。`Boundary::None` なら `None`。
pub fn region_of(
    p: &(impl kmb_toolpath_moves::Cutter + AsRef<MachiningModel>),
    boundary: Boundary,
    faces: &[String],
    contain: Contain,
    offset: f64,
    cell: f64,
) -> Result<Option<Region>, EvalError> {
    let tris = match boundary {
        Boundary::None => return Ok(None),
        Boundary::Silhouette => p.as_ref().triangles.clone(),
        Boundary::Faces => {
            if faces.is_empty() {
                return Err(invalid(
                    "範囲の面を選んでいません(面を選んでから加工を足すと、その面が範囲になります)",
                ));
            }
            let t = p.as_ref().triangles_of(faces);
            if t.is_empty() {
                return Err(invalid(
                    "範囲の面が見つかりません(参照が切れたかもしれません)",
                ));
            }
            t
        }
    };
    let areas = silhouette(&tris, cell);
    let d = match contain {
        Contain::Center => 0.0,
        Contain::Inside => p.radius(),
        Contain::Outside => -p.radius(),
    } - offset;
    let areas: Vec<Area> = if d.abs() < 1e-9 {
        areas
    } else {
        areas.iter().flat_map(|a| offset_area(a, d)).collect()
    };
    if areas.is_empty() {
        return Err(invalid(
            "加工の範囲が空です(上から見て広さのない面か、工具が範囲より太い)",
        ));
    }
    Ok(Some(Region::new(&areas)))
}

/// 三角形を上から見た外形(格子の幅 `cell` で調べる)。
pub fn silhouette(tris: &[[[f64; 3]; 3]], cell: f64) -> Vec<Area> {
    if tris.is_empty() {
        return Vec::new();
    }
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for v in tris.iter().flatten() {
        for k in 0..3 {
            min[k] = min[k].min(v[k]);
            max[k] = max[k].max(v[k]);
        }
    }
    let pad = 2.0 * cell;
    let floor = min[2] - 1.0;
    let h = Heights::new(
        [min[0] - pad, min[1] - pad],
        [max[0] + pad, max[1] + pad],
        cell,
        floor,
        tris,
    );
    let g = h.grid();
    let mask: Vec<bool> = (0..g.nx * g.ny)
        .map(|k| h.node(k % g.nx, k / g.nx) > floor + 0.5)
        .collect();
    areas_from_loops(mask_loops(&g, &mask, 2.0 * cell))
}

/// 径方向に残す厚さの欄。
pub fn leave_radial_field() -> Field {
    Field::opt_expr("leave_radial", "径方向の残し量").length().help(
        "面から横の向きに残す厚さ(工具をこの厚さだけ太らせて下ろす)。空なら 0 で、残す厚さは上向きだけに足す。\
         上の「残し量」(軸方向)と同じにすると、急な壁にも面のどの向きにも同じ厚さが残る",
    )
}

/// 加工の範囲の欄。
pub fn boundary_fields() -> Vec<Field> {
    vec![
        Field::choice(
            "boundary",
            "加工の範囲",
            &[
                ("none", "なし(部品の外形の箱)"),
                ("silhouette", "シルエット(上から見た部品の外形)"),
                ("faces", "選んだ面"),
            ],
        )
        .help("一部だけを削る時に限る。選んだ面は、面を選んでから加工を足すとその面になる"),
        Field::info("faces", "範囲の面").only("boundary", "faces", "none"),
        Field::choice(
            "contain",
            "範囲と工具",
            &[
                ("center", "工具の中心が範囲の中"),
                ("inside", "工具が丸ごと範囲の中"),
                ("outside", "工具が範囲に触れる所まで"),
            ],
        )
        .unless("boundary", "none"),
        Field::opt_expr("boundary_offset", "範囲のオフセット")
            .length()
            .help("例: 1 mm。正なら外へ広げ、負なら内へ狭める")
            .unless("boundary", "none"),
    ]
}

/// 傾斜の範囲の欄。
pub fn slope_fields() -> Vec<Field> {
    vec![
        Field::opt_expr("slope_from", "傾斜の開始角度")
            .angle()
            .help("例: 30 deg。これよりなだらかな所は削らない(0° が水平)。空なら 0°"),
        Field::opt_expr("slope_to", "傾斜の終了角度")
            .angle()
            .help("例: 60 deg。これより急な所は削らない(90° が垂直)。空なら 90°"),
    ]
}
