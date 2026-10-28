//! 区画ごとの材料と、共通の経路を切り出す窓。座標は WCS。

use kmb_toolpath::P3;
use serde::{Deserialize, Serialize};

/// 区画 1 の座標で作った、区画によらない経路の出力の印。
pub const SECTION_FIXED: &str = "section_fixed";
pub const SECTION_ENTRY: &str = "section_entry";

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SectionEntry(pub crate::EntryPlan);

impl kmb_core::Payload for SectionEntry {
    const TYPE: &'static str = "cam.section_entry@1";
}

/// 加工で解いた入り方を区画の切り分けへ渡す。区画が無ければ出力は増やさない。
pub fn add_entry(out: &mut kmb_core::Outputs, info: &crate::SetupInfo, entry: crate::EntryPlan) {
    if info.sections.is_some() {
        out.insert(
            SECTION_ENTRY.into(),
            kmb_core::Payload::to_value(&SectionEntry(entry)),
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect2 {
    #[serde(with = "endpoints")]
    pub min: [f64; 2],
    #[serde(with = "endpoints")]
    pub max: [f64; 2],
}

// JSON は無限の数を持たないので、無限の端だけを明示の文字で保存する。
mod endpoints {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    #[derive(Serialize, Deserialize)]
    #[serde(untagged)]
    enum End {
        Number(f64),
        Limit(String),
    }
    pub fn serialize<S: Serializer>(v: &[f64; 2], s: S) -> Result<S::Ok, S::Error> {
        let ends = v.map(|n| {
            if n == f64::INFINITY {
                End::Limit("+inf".into())
            } else if n == f64::NEG_INFINITY {
                End::Limit("-inf".into())
            } else {
                End::Number(n)
            }
        });
        if v.iter().any(|n| n.is_nan()) {
            return Err(serde::ser::Error::custom("窓の端は NaN にできません"));
        }
        ends.serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[f64; 2], D::Error> {
        let ends = <[End; 2]>::deserialize(d)?;
        let mut out = [0.0; 2];
        for (k, e) in ends.into_iter().enumerate() {
            out[k] = match e {
                End::Number(n) if n.is_finite() => n,
                End::Limit(s) if s == "+inf" => f64::INFINITY,
                End::Limit(s) if s == "-inf" => f64::NEG_INFINITY,
                _ => {
                    return Err(serde::de::Error::custom(
                        "窓の端は有限の数か +inf・-inf で指定してください",
                    ));
                }
            };
        }
        Ok(out)
    }
}

impl Rect2 {
    pub fn contains(&self, p: [f64; 2]) -> bool {
        (0..2).all(|k| p[k] >= self.min[k] && p[k] <= self.max[k])
    }

    /// 点の加工は半開区間で受け持ち、境の穴を重ねて削らない。
    pub fn owns(&self, p: [f64; 2]) -> bool {
        (0..2).all(|k| p[k] >= self.min[k] && p[k] < self.max[k])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionKind {
    Sheets,
    Tiles,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionOutput {
    #[default]
    Files,
    Stops,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Section {
    pub index: u32,
    pub label: String,
    pub cut: Rect2,
    pub own: Rect2,
    pub shift: [f64; 2],
    pub stock_min: P3,
    pub stock_max: P3,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sections {
    pub kind: SectionKind,
    #[serde(default)]
    pub output: SectionOutput,
    pub list: Vec<Section>,
}

impl kmb_toolpath_moves::clip::SectionWindow for Section {
    fn cut_min(&self) -> [f64; 2] {
        self.cut.min
    }
    fn cut_max(&self) -> [f64; 2] {
        self.cut.max
    }
    fn owns(&self, point: [f64; 2]) -> bool {
        self.own.owns(point)
    }
    fn shift(&self) -> [f64; 2] {
        self.shift
    }
    fn uncut_entry(&self, point: [f64; 2]) -> bool {
        (0..2).any(|k| (point[k] - self.cut.max[k]).abs() < 1e-7)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn infinite_window_roundtrips_json_and_ownership_is_half_open() {
        let window = Rect2 {
            min: [f64::NEG_INFINITY, -5.0],
            max: [f64::INFINITY, 10.0],
        };
        let json = serde_json::to_string(&window).unwrap();
        assert_eq!(window, serde_json::from_str::<Rect2>(&json).unwrap());
        assert!(window.owns([-10000.0, -5.0]));
        assert!(!window.owns([0.0, 10.0]));
        assert!(window.contains([0.0, 10.0]));
    }
    #[test]
    fn old_setup_and_info_omit_optional_nesting_sections() {
        let setup = crate::Setup::new(kmb_core::FeatureId(1));
        let json = serde_json::to_value(&setup).unwrap();
        assert!(json.get("nest").is_none());
        assert_eq!(setup, serde_json::from_value::<crate::Setup>(json).unwrap());
        let info = crate::testing::prepared(
            &kmb_core::Mesh::default(),
            [0.0; 3],
            [10.0; 3],
            kmb_cam_tool::ToolLibrary::starter().tools[0].clone(),
        )
        .info;
        let json = serde_json::to_value(&info).unwrap();
        assert!(json.get("sections").is_none());
        assert_eq!(
            info,
            serde_json::from_value::<crate::SetupInfo>(json).unwrap()
        );
    }
}
