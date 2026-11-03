//! 種類ごとの歯の形。種類の並びはここの 1 か所だけで、種類を足す時は `match` に 1 行と
//! `kinds/` に 1 ファイルを足す。形を作れない種類は「まだ作れない」誤りにする(黙って別の形にしない)。

pub mod rack;
mod spur;

use crate::tooth::P2;
use kmb_core::ProfileEdge;
use kmb_interchange::GearKind;

/// 歯の形を決める値(長さは mm、角度はラジアン、係数はモジュールに対する割合)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Params {
    pub module: f64,
    pub teeth: u32,
    pub pressure_angle: f64,
    pub profile_shift: f64,
    /// 歯先の丸みの半径の係数
    pub tip_round: f64,
    /// 歯底の丸みの半径の係数
    pub root_round: f64,
    /// ラックの全長(空なら歯数 × 円ピッチ)
    pub rack_length: Option<f64>,
    /// ラックの歯底から背までの厚さ(空なら 2 × モジュール)
    pub rack_back: Option<f64>,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            module: 1.0,
            teeth: 20,
            pressure_angle: 20f64.to_radians(),
            profile_shift: 0.0,
            tip_round: 0.0,
            root_round: ROOT_ROUND,
            rack_length: None,
            rack_back: None,
        }
    }
}

/// 歯底の丸みの既定の係数(基準ラックの歯底の丸みの半径 0.38 × モジュール)。
pub const ROOT_ROUND: f64 = 0.38;

/// 歯車の形: 原点を中心に 1 枚目の歯を X 軸の向きに立てた外形の輪(反時計回り)、参照の円
/// (名前と半径)、注意。
#[derive(Debug, Clone, PartialEq)]
pub struct Outline {
    pub edges: Vec<ProfileEdge>,
    pub reference: Vec<(&'static str, P2, f64)>,
    pub warnings: Vec<String>,
    /// 円以外の参照の線(ラックのピッチ線など)
    pub reference_lines: Vec<ProfileEdge>,
}

/// 種類 `kind` の形。
pub fn outline(kind: GearKind, p: &Params) -> Result<Outline, String> {
    match kind {
        GearKind::External => spur::outline(p),
        GearKind::Rack => rack::outline(p),
        other => Err(format!(
            "「{}」の形はまだ作れません(今作れるのは「{}」と「ラック」です)",
            other.label(),
            GearKind::External.label()
        )),
    }
}

/// 種類 `kind` の形を作れるか(画面の種類の一覧で、作れない種類を灰色にする)。
pub fn available(kind: GearKind) -> bool {
    matches!(kind, GearKind::External | GearKind::Rack)
}
