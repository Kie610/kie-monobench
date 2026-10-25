//! 穴の情報。穴の機能が出力 [`HOLE_OUTPUT`] に入れ、加工が穴あけの並びと寸法を決めるのに読む。
//!
//! 穴の面の名前の決まりもここに置く。穴の機能は、道具の面に点の番号の名前(まっすぐな穴は
//! `p<点の番号>`、ザグリは `p<点の番号>_counterbore`)を付ける([`edge_name`])。ボディの面の名前は
//! `f<機能の番号>:side:<名前>` になる(truck は円を 2 面に分けて `.a`・`.b`、面が分かれると
//! `#0` … が付く)。加工はこの名前で、部品の縦の穴のうちどれがその穴の機能のものかを見分ける
//! ([`hole_face`])。

use kmb_core::{FeatureId, Payload};
use serde::{Deserialize, Serialize};

/// 穴の情報を入れる出力の名前。
pub const HOLE_OUTPUT: &str = "hole";

/// 穴の口の形。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoleKind {
    #[default]
    Simple,
    Counterbore,
    Countersink,
}

/// ネジとの関係。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tap {
    #[default]
    Simple,
    Clearance,
    Tapped,
}

/// 穴の機能 1 つの情報(寸法は mm とラジアン)。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct HoleInfo {
    #[serde(default)]
    pub kind: HoleKind,
    #[serde(default)]
    pub tap: Tap,
    /// ネジのサイズ(穴の機能で省いた時は空。読む側が既定の M3 にする)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    /// 穴の径
    pub diameter: f64,
    /// 穴の数(点の数)
    pub count: usize,
    /// ザグリの径と深さ(ザグリの穴だけ)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counterbore: Option<(f64, f64)>,
    /// 皿の径と角度(頂角。皿の穴だけ)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub countersink: Option<(f64, f64)>,
}

impl Payload for HoleInfo {
    const TYPE: &'static str = "hole.info@1";
}

/// 穴のどの部分を削るか。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HolePart {
    /// 穴そのもの(V ビットならもみつけ)
    #[default]
    Wall,
    /// ザグリ(六角穴付きボルトの頭が沈む段)
    Counterbore,
    /// 皿の口(V ビットで皿の径まで面取りする)
    Countersink,
}

impl HolePart {
    pub fn label(self) -> &'static str {
        match self {
            HolePart::Wall => "穴",
            HolePart::Counterbore => "ザグリ",
            HolePart::Countersink => "皿の面取り",
        }
    }
}

/// 穴の道具の、点 `point` の部品 `piece`(`wall`・`counterbore` …)の面の名前。
/// まっすぐな穴(`wall`)は `p<点の番号>`、ほかは `p<点の番号>_<部品>`。
pub fn edge_name(point: u32, piece: &str) -> String {
    if piece == "wall" {
        format!("p{point}")
    } else {
        format!("p{point}_{piece}")
    }
}

/// 名前が、穴の点の面の名前(`p<点の番号>`)か。
pub fn is_point_name(name: &str) -> bool {
    name.strip_prefix('p')
        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

/// 穴の機能 `hole` が作ったボディの面の名前の頭(`f<番号>:side:p`)。
pub fn face_prefix(hole: FeatureId) -> String {
    format!("f{}:side:p", hole.0)
}

/// 面の名前 `face` が穴の機能 `hole` の面なら、その点の番号と部分。
pub fn hole_face(face: &str, hole: FeatureId) -> Option<(u32, HolePart)> {
    let rest = face.strip_prefix(&format!("f{}:side:", hole.0))?;
    let name = rest.split(['.', '#']).next()?;
    let (num, part) = match name.strip_suffix("_counterbore") {
        Some(n) => (n, HolePart::Counterbore),
        None => (name, HolePart::Wall),
    };
    let n: u32 = num.strip_prefix('p')?.parse().ok()?;
    Some((n, part))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hole_faces_are_matched_by_the_feature_and_the_point() {
        let h = FeatureId(12);
        assert_eq!(hole_face("f12:side:p5", h), Some((5, HolePart::Wall)));
        assert_eq!(hole_face("f12:side:p5.a", h), Some((5, HolePart::Wall)));
        assert_eq!(hole_face("f12:side:p7#1", h), Some((7, HolePart::Wall)));
        assert_eq!(
            hole_face("f12:side:p5_counterbore.b", h),
            Some((5, HolePart::Counterbore))
        );
        assert_eq!(hole_face("f13:side:p5", h), None);
        assert_eq!(hole_face("f12:side:e5", h), None);
        assert_eq!(hole_face("f12:side:countersink", h), None);
        assert_eq!(hole_face("f12:end", h), None);
        assert!(format!("f12:side:{}", edge_name(5, "wall")).starts_with(&face_prefix(h)));
        assert_eq!(
            hole_face(&format!("f12:side:{}", edge_name(5, "counterbore")), h),
            Some((5, HolePart::Counterbore))
        );
    }
}
