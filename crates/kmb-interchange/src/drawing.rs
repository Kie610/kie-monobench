//! 図面の計算結果。座標と線幅は用紙の mm、原点は左下、上が +y。

use kmb_core::{FeatureId, Problem};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SheetPicture {
    pub sheet: String,
    pub size: [f64; 2],
    pub items: Vec<Item>,
    pub problems: Vec<DrawingProblem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrawingProblem {
    pub target: String,
    pub problem: Problem,
    /// 警告は図を残す。誤りは対象の図を出さない。
    pub warning: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Item {
    Stroke {
        pen: Pen,
        line: LineStyle,
        path: Path,
        owner: String,
    },
    /// 偶奇規則で塗る輪郭。文字と矢印もこの形で渡す。
    Fill {
        loops: Vec<Vec<[f64; 2]>>,
        owner: String,
    },
    Hatch {
        loops: Vec<Vec<[f64; 2]>>,
        angle_deg: f64,
        spacing_mm: f64,
        pen: Pen,
        owner: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pen {
    Thick,
    Thin,
    ExtraThick,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineStyle {
    Solid,
    Dashed,
    Chain,
    DoubleChain,
}
impl LineStyle {
    /// 破線の長さと空きの繰返し(mm)。実線は空。
    pub fn pattern(self, width_mm: f64) -> Vec<f64> {
        let units: &[f64] = match self {
            Self::Solid => &[],
            Self::Dashed => &[12., 3.],
            Self::Chain => &[24., 3., 0.5, 3.],
            Self::DoubleChain => &[24., 3., 0.5, 3., 0.5, 3.],
        };
        units.iter().map(|n| n * width_mm).collect()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Path {
    Line {
        a: [f64; 2],
        b: [f64; 2],
    },
    Arc {
        c: [f64; 2],
        r: f64,
        start_deg: f64,
        sweep_deg: f64,
    },
    Poly {
        points: Vec<[f64; 2]>,
    },
}

/// 稜線の参照。面名の組は辞書順。index は同じ組の位置順で、1 本なら 0。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EdgeKey {
    pub body: FeatureId,
    pub faces: [String; 2],
    pub index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LineKey {
    Edge { key: EdgeKey },
    Silhouette { body: FeatureId, face: String },
}

/// ビュー内の参照用の線。隠れの区切りも含め、座標は用紙の mm。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewLine {
    pub key: LineKey,
    pub path: Path,
    pub visible: bool,
}
