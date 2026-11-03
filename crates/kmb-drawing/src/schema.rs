use kmb_core::{FeatureId, SectionType};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub type Rest = serde_json::Map<String, serde_json::Value>;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Drawings {
    #[serde(default)]
    pub sheets: Vec<Sheet>,
    #[serde(flatten)]
    pub rest: Rest,
}
impl SectionType for Drawings {
    const KEY: &'static str = "draw.sheets";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "図面";
    fn is_empty(&self) -> bool {
        self.sheets.is_empty() && self.rest.is_empty()
    }
    fn check(&self) -> Result<(), String> {
        let mut ids = BTreeSet::new();
        for s in &self.sheets {
            if s.id.is_empty() || !ids.insert(s.id.as_str()) {
                return Err("図面の id が空か重複しています".into());
            }
            let size = s.paper.size();
            if s.paper.colors.values().any(|x| !(1..=255).contains(x)) {
                return Err("画層の色は1〜255の番号で指定してください".into());
            }
            if matches!(s.paper.format, PaperFormat::Custom {size} if !size.iter().all(|x|x.is_finite()&&*x>0.))
            {
                return Err("用紙の大きさは正の有限の数です".into());
            }
            if !size.iter().all(|x| x.is_finite() && *x > 0.)
                || ![s.pens.thick, s.pens.thin, s.pens.extra_thick]
                    .iter()
                    .all(|x| x.is_finite() && *x > 0.)
            {
                return Err("用紙とペンの大きさは正の数です".into());
            }
            for v in &s.views {
                if v.id.is_empty() || !ids.insert(v.id.as_str()) {
                    return Err("ビューの id が空か重複しています".into());
                }
                v.scale.value()?;
                if !v.at.iter().all(|x| x.is_finite()) {
                    return Err("ビューの位置は有限の数です".into());
                }
                if matches!(
                    v.kind,
                    ViewKind::Projected { .. } | ViewKind::Section { .. }
                ) && v.at[1] != 0.
                {
                    return Err("投影・断面ビューの位置は親からの距離一つで指定します".into());
                }
                match &v.kind {
                    ViewKind::Base { rotate, .. } if ![0, 90, 180, 270].contains(rotate) => {
                        return Err("回転は 0・90・180・270 度です".into());
                    }
                    ViewKind::Section { line, letter, .. } => {
                        if !line.iter().flatten().all(|x| x.is_finite())
                            || geometry_distance(line[0], line[1]) < 1e-9
                            || !valid_letter(letter)
                        {
                            return Err("切断線と文字を確認してください".into());
                        }
                    }
                    ViewKind::Detail {
                        center,
                        radius,
                        scale,
                        letter,
                        ..
                    } => {
                        scale.value()?;
                        if !center.iter().all(|x| x.is_finite())
                            || !radius.is_finite()
                            || *radius <= 0.
                            || !valid_letter(letter)
                        {
                            return Err("詳細ビューの円と文字を確認してください".into());
                        }
                    }
                    _ => {}
                }
                let mut chain = BTreeSet::from([v.id.as_str()]);
                let mut p = v.parent();
                while let Some(id) = p {
                    if !chain.insert(id) {
                        return Err("ビューの親が輪になっています".into());
                    }
                    let parent = s
                        .views
                        .iter()
                        .find(|x| x.id == id)
                        .ok_or("親のビューがありません")?;
                    p = parent.parent();
                }
            }
            // 注記の中身は注記のモジュールが扱う。id の契約は区画全体で守る。
            for n in &s.notes {
                if let Some(id) = n.get("id").and_then(|x| x.as_str())
                    && (id.is_empty() || !ids.insert(id))
                {
                    return Err("注記の id が空か重複しています".into());
                }
            }
        }
        Ok(())
    }
}
fn geometry_distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}
fn valid_letter(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_uppercase())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sheet {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub paper: Paper,
    #[serde(default)]
    pub projection: Projection,
    #[serde(default)]
    pub pens: Pens,
    #[serde(default)]
    pub dim_style: serde_json::Value,
    #[serde(default)]
    pub title: serde_json::Value,
    #[serde(default)]
    pub views: Vec<DrawingView>,
    #[serde(default)]
    pub notes: Vec<serde_json::Value>,
    #[serde(flatten)]
    pub rest: Rest,
}
impl Sheet {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            paper: Paper::default(),
            projection: Projection::default(),
            pens: Pens::default(),
            dim_style: Default::default(),
            title: Default::default(),
            views: vec![],
            notes: vec![],
            rest: Rest::new(),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pens {
    pub thick: f64,
    pub thin: f64,
    pub extra_thick: f64,
}
impl Default for Pens {
    fn default() -> Self {
        Self {
            thick: 0.5,
            thin: 0.25,
            extra_thick: 1.0,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Projection {
    #[default]
    ThirdAngle,
    FirstAngle,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Paper {
    #[serde(default)]
    pub format: PaperFormat,
    #[serde(default)]
    pub portrait: bool,
    #[serde(default)]
    pub binding: bool,
    /// 線の役ごとの DXF の画層の色(1〜255)。無い役は既定の色。
    #[serde(default)]
    pub colors: std::collections::BTreeMap<String, i32>,
    #[serde(flatten)]
    pub rest: Rest,
}
impl Default for Paper {
    fn default() -> Self {
        Self {
            format: PaperFormat::A4,
            portrait: false,
            binding: false,
            colors: Default::default(),
            rest: Rest::new(),
        }
    }
}
impl Paper {
    pub fn size(&self) -> [f64; 2] {
        let [w, h] = match self.format {
            PaperFormat::A0 => [1189., 841.],
            PaperFormat::A1 => [841., 594.],
            PaperFormat::A2 => [594., 420.],
            PaperFormat::A3 => [420., 297.],
            PaperFormat::A4 => [297., 210.],
            PaperFormat::Custom { size } => size,
        };
        if self.portrait {
            [w.min(h), w.max(h)]
        } else {
            [w.max(h), w.min(h)]
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "name", rename_all = "snake_case")]
pub enum PaperFormat {
    A0,
    A1,
    A2,
    A3,
    #[default]
    A4,
    Custom {
        size: [f64; 2],
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Ratio {
    pub num: f64,
    pub den: f64,
}
impl Default for Ratio {
    fn default() -> Self {
        Self { num: 1., den: 1. }
    }
}
impl Ratio {
    pub fn value(self) -> Result<f64, String> {
        let r = self.num / self.den;
        if self.num.is_finite()
            && self.den.is_finite()
            && self.num > 0.
            && self.den > 0.
            && r.is_finite()
            && r > 0.
        {
            Ok(r)
        } else {
            Err("尺度は正の有限の比で指定します".into())
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ViewOptions {
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub tangent: bool,
    #[serde(flatten)]
    pub rest: Rest,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrawingView {
    pub id: String,
    #[serde(default)]
    pub label: String,
    pub kind: ViewKind,
    #[serde(default)]
    pub scale: Ratio,
    /// 基準・詳細は自由位置。投影・断面の位置は親からの軸上の距離を at[0] に保持する。
    #[serde(default)]
    pub at: [f64; 2],
    #[serde(default)]
    pub options: ViewOptions,
    #[serde(flatten)]
    pub rest: Rest,
}
impl DrawingView {
    pub fn parent(&self) -> Option<&str> {
        match &self.kind {
            ViewKind::Base { .. } => None,
            ViewKind::Projected { parent, .. }
            | ViewKind::Section { parent, .. }
            | ViewKind::Detail { parent, .. } => Some(parent),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ViewKind {
    Base {
        #[serde(default)]
        source: Source,
        orientation: Orientation,
        #[serde(default)]
        rotate: u16,
    },
    Projected {
        parent: String,
        side: Side,
    },
    Section {
        parent: String,
        line: [[f64; 2]; 2],
        letter: String,
        #[serde(default)]
        uncut: Vec<FeatureId>,
    },
    Detail {
        parent: String,
        center: [f64; 2],
        radius: f64,
        scale: Ratio,
        letter: String,
    },
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    #[default]
    All,
    Component {
        id: u64,
    },
    Bodies {
        bodies: Vec<FeatureId>,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    Front,
    Top,
    Right,
    Left,
    Bottom,
    Back,
    Iso,
}
impl Orientation {
    pub fn name(self) -> &'static str {
        match self {
            Self::Front => "front",
            Self::Top => "top",
            Self::Right => "right",
            Self::Left => "left",
            Self::Bottom => "bottom",
            Self::Back => "back",
            Self::Iso => "iso",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Top,
    Bottom,
    Left,
    Right,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}
impl Side {
    pub fn axis(self) -> [f64; 2] {
        match self {
            Self::Top => [0., 1.],
            Self::Bottom => [0., -1.],
            Self::Left => [-1., 0.],
            Self::Right => [1., 0.],
            Self::TopLeft => [-1., 1.],
            Self::TopRight => [1., 1.],
            Self::BottomLeft => [-1., -1.],
            Self::BottomRight => [1., -1.],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn base(id: &str) -> DrawingView {
        DrawingView {
            id: id.into(),
            label: String::new(),
            kind: ViewKind::Base {
                source: Source::All,
                orientation: Orientation::Front,
                rotate: 0,
            },
            scale: Ratio::default(),
            at: [0.; 2],
            options: ViewOptions::default(),
            rest: Rest::new(),
        }
    }
    #[test]
    fn section_preserves_unknown_fields_and_rejects_broken_parent_graphs() {
        let mut sheet = Sheet::new("s", "図面");
        sheet.views = vec![base("a"), base("b")];
        let mut drawings = Drawings {
            sheets: vec![sheet],
            ..Default::default()
        };
        drawings
            .rest
            .insert("future".into(), serde_json::json!({"x":7}));
        let json = serde_json::to_string(&drawings).unwrap();
        let back: Drawings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, drawings);
        assert!(back.check().is_ok());
        drawings.sheets[0].views[1].id = "a".into();
        assert!(drawings.check().is_err());
        drawings.sheets[0].views[1].id = "b".into();
        drawings.sheets[0].views[0].kind = ViewKind::Projected {
            parent: "missing".into(),
            side: Side::Top,
        };
        assert!(drawings.check().is_err());
        drawings.sheets[0].views[0].kind = ViewKind::Projected {
            parent: "b".into(),
            side: Side::Top,
        };
        drawings.sheets[0].views[1].kind = ViewKind::Projected {
            parent: "a".into(),
            side: Side::Right,
        };
        assert!(drawings.check().is_err());
    }
    #[test]
    fn ratio_and_paper_validate_finite_positive_sizes() {
        for ratio in [
            Ratio { num: 0., den: 1. },
            Ratio { num: 1., den: 0. },
            Ratio {
                num: f64::INFINITY,
                den: 1.,
            },
        ] {
            assert!(ratio.value().is_err());
        }
        assert_eq!(Paper::default().size(), [297., 210.]);
        assert_eq!(
            Paper {
                portrait: true,
                ..Default::default()
            }
            .size(),
            [210., 297.]
        );
    }
}
