//! 図面の寸法と注記の定義。位置は用紙の mm、参照する形は辺の名前で持つ。

use crate::Rest;
use kmb_core::LengthUnit;
use kmb_interchange::drawing::EdgeKey;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DimRef {
    pub view: String,
    pub key: EdgeKey,
    pub at: RefAt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefAt {
    A,
    B,
    Midpoint,
    Center,
    Line,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DimKind {
    Horizontal,
    Vertical,
    Parallel,
    Angle,
    Radius,
    Diameter,
    OrdinateX,
    OrdinateY,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Arrow {
    #[default]
    Triangle,
    Dot,
    Slash,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Tolerance {
    #[serde(default)]
    pub plus_minus: Option<f64>,
    #[serde(default)]
    pub upper: Option<f64>,
    #[serde(default)]
    pub lower: Option<f64>,
    #[serde(default)]
    pub fit: String,
    #[serde(flatten)]
    pub rest: Rest,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DimStyle {
    pub height: f64,
    pub arrow: Arrow,
    pub arrow_length: f64,
    pub arrow_angle: f64,
    pub extension_gap: f64,
    pub extension_over: f64,
    pub spacing: f64,
    pub digits: u8,
    pub trim_zeros: bool,
    /// 空なら文書の表示の単位に追従する。
    pub unit: Option<LengthUnit>,
    pub tolerance: Tolerance,
    #[serde(flatten)]
    pub rest: Rest,
}

impl Default for DimStyle {
    fn default() -> Self {
        Self {
            height: 3.5,
            arrow: Arrow::Triangle,
            arrow_length: 3.0,
            arrow_angle: 30.0,
            extension_gap: 1.0,
            extension_over: 2.0,
            spacing: 8.0,
            digits: 2,
            trim_zeros: true,
            unit: None,
            tolerance: Tolerance::default(),
            rest: Rest::new(),
        }
    }
}

impl DimStyle {
    pub fn check(&self) -> Result<(), String> {
        if [self.height, self.arrow_length, self.spacing]
            .iter()
            .any(|x| !x.is_finite() || *x <= 0.0)
            || [self.extension_gap, self.extension_over]
                .iter()
                .any(|x| !x.is_finite() || *x < 0.0)
            || !self.arrow_angle.is_finite()
            || !(0.0..180.0).contains(&self.arrow_angle)
            || self.arrow_angle == 0.0
            || self.digits > 6
        {
            return Err("寸法の文字・矢印・間隔は正の数、すき間と出しすぎは0以上、小数の桁は0〜6にしてください".into());
        }
        let t = &self.tolerance;
        if [t.plus_minus, t.upper, t.lower]
            .into_iter()
            .flatten()
            .any(|x| !x.is_finite())
            || t.plus_minus.is_some_and(|x| x < 0.0)
            || (t.plus_minus.is_some() && (t.upper.is_some() || t.lower.is_some()))
        {
            return Err("公差は有限の値で、±と上下の差はどちらか一方を指定してください".into());
        }
        Ok(())
    }

    pub fn value_text(&self, value: f64, angle: bool, document_unit: LengthUnit) -> String {
        let unit = self.unit.unwrap_or(document_unit);
        let number = |v: f64| {
            let mut s = format!("{:.*}", self.digits as usize, v);
            if self.trim_zeros && s.contains('.') {
                s = s.trim_end_matches('0').trim_end_matches('.').to_string();
            }
            if s.starts_with('-') && s[1..].chars().all(|c| c == '0' || c == '.') {
                s.remove(0);
            }
            s
        };
        let convert = |v| if angle { v } else { unit.from_mm(v) };
        let mut s = number(convert(value));
        if angle {
            s.push('°');
        } else if unit != LengthUnit::Mm {
            s.push_str(&format!(" {}", unit.name()));
        }
        let t = &self.tolerance;
        if let Some(pm) = t.plus_minus {
            s.push_str(&format!(" ±{}", number(convert(pm))));
        }
        if t.upper.is_some() || t.lower.is_some() {
            let signed = |x: f64| {
                format!(
                    "{}{v}",
                    if x >= 0.0 { "+" } else { "" },
                    v = number(convert(x))
                )
            };
            s.push_str(&format!(
                " {}/{}",
                signed(t.upper.unwrap_or(0.0)),
                signed(t.lower.unwrap_or(0.0))
            ));
        }
        s.push_str(&t.fit);
        s
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Annotation {
    pub id: String,
    #[serde(flatten)]
    pub kind: AnnotationKind,
    /// 読む時にフォントのファイルを要らなくする、使う字の輪郭。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline: Option<serde_json::Value>,
    #[serde(flatten)]
    pub rest: Rest,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AnnotationKind {
    Dimension {
        dimension: DimKind,
        refs: Vec<DimRef>,
        at: [f64; 2],
    },
    Note {
        text: String,
        at: [f64; 2],
        #[serde(default)]
        leader: Option<Leader>,
    },
    CenterMark {
        reference: DimRef,
    },
    CenterLine {
        refs: [DimRef; 2],
    },
    Balloon {
        view: String,
        part: kmb_asm::inspect::BomSource,
        tip: [f64; 2],
        at: [f64; 2],
        number: usize,
    },
    PartsTable {
        #[serde(default)]
        at: Option<[f64; 2]>,
        columns: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Leader {
    pub tip: [f64; 2],
    #[serde(default)]
    pub reference: Option<DimRef>,
    #[serde(default)]
    pub arrow: Arrow,
}

pub fn style_of(value: &serde_json::Value) -> Result<DimStyle, String> {
    let style = if value.is_null() {
        DimStyle::default()
    } else {
        serde_json::from_value(value.clone()).map_err(|e| format!("寸法の様式: {e}"))?
    };
    style.check()?;
    Ok(style)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dimension_style_preserves_defaults_units_and_unknown_values() {
        let style = style_of(&serde_json::Value::Null).unwrap();
        assert_eq!(style.value_text(25.4, false, LengthUnit::Mm), "25.4");
        assert_eq!(style.value_text(25.4, false, LengthUnit::In), "1 in");
        let s =
            style_of(&serde_json::json!({"future":{"pen":9},"tolerance":{"fit":" H7","future":7}}))
                .unwrap();
        let back = serde_json::to_value(&s).unwrap();
        assert_eq!(back["future"]["pen"], 9);
        assert_eq!(back["tolerance"]["future"], 7);
        assert!(s.value_text(25.4, false, LengthUnit::Mm).ends_with(" H7"));
        for value in [
            serde_json::json!({"height":0}),
            serde_json::json!({"digits":7}),
            serde_json::json!({"tolerance":{"plus_minus":-1}}),
        ] {
            assert!(style_of(&value).is_err());
        }
    }
}
