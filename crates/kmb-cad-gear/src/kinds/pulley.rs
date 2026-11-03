//! ベルト車の軸断面。平ベルト車は中高の円弧、V ベルト車は規格寸法の溝を持つ。

use super::{Outline, Params};
use crate::tooth::{P2, arc, edge};
use kmb_core::{ProfileEdge, Segment};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum VSection {
    #[default]
    A,
    B,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VDimensions {
    pub pitch_width: f64,
    pub above: f64,
    pub below: f64,
    pub angle: f64,
}

impl VSection {
    pub fn dimensions(self, diameter: f64) -> VDimensions {
        let (pitch_width, above, below, angle) = match self {
            Self::A => (
                11.0,
                2.75,
                8.7,
                if diameter <= 118.0 {
                    34.0f64
                } else if diameter <= 190.0 {
                    36.0
                } else {
                    38.0
                },
            ),
            Self::B => (
                14.0,
                3.5,
                10.8,
                if diameter <= 190.0 {
                    34.0f64
                } else if diameter <= 315.0 {
                    36.0
                } else {
                    38.0
                },
            ),
        };
        VDimensions {
            pitch_width,
            above,
            below,
            angle: angle.to_radians(),
        }
    }
}

/// 断面の X は半径、Y は軸方向の幅。上書きの深さは溝の上端から底まで。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pulley {
    pub diameter: f64,
    pub width: f64,
    pub bore: f64,
    pub crown: f64,
    pub grooves: u32,
    pub section: VSection,
    pub angle: Option<f64>,
    pub top_width: Option<f64>,
    pub depth: Option<f64>,
}

impl Default for Pulley {
    fn default() -> Self {
        Self {
            diameter: 100.0,
            width: 30.0,
            bore: 0.0,
            crown: 0.0,
            grooves: 1,
            section: VSection::A,
            angle: None,
            top_width: None,
            depth: None,
        }
    }
}

pub(super) fn line(name: &str, a: P2, b: P2) -> ProfileEdge {
    edge(name, Segment::Line { a, b }, a)
}

pub(super) fn validate_section(
    diameter: f64,
    width: f64,
    bore: f64,
    minimum_radius: f64,
) -> Result<(), String> {
    if [diameter, width, bore, minimum_radius]
        .iter()
        .any(|v| !v.is_finite())
        || diameter <= 0.0
        || width <= 0.0
        || bore < 0.0
        || minimum_radius <= 0.0
    {
        return Err(
            "ピッチ円の径と幅は有限の 0 より大きい長さ、軸の穴の径は 0 以上にしてください".into(),
        );
    }
    if bore / 2.0 >= minimum_radius {
        return Err(format!(
            "軸の穴の径は溝の底の径 {:.3} mm より小さくしてください",
            2.0 * minimum_radius
        ));
    }
    Ok(())
}

pub fn outline(p: &Params, v_groove: bool) -> Result<Outline, String> {
    let p = p.pulley.unwrap_or_default();
    let r = p.diameter / 2.0;
    let b = p.bore / 2.0;
    if v_groove {
        let d = p.section.dimensions(p.diameter);
        let angle = p.angle.unwrap_or(d.angle);
        if !angle.is_finite()
            || angle <= 0.0
            || angle >= std::f64::consts::FRAC_PI_2
            || p.grooves == 0
        {
            return Err(
                "V 溝の角は 0 deg より大きく 90 deg より小さく、溝の数は 1 以上にしてください"
                    .into(),
            );
        }
        let tan = (angle / 2.0).tan();
        let top = p.top_width.unwrap_or(d.pitch_width + 2.0 * d.above * tan);
        let depth = p.depth.unwrap_or(d.above + d.below);
        let bottom = top - 2.0 * depth * tan;
        let outer = r + d.above;
        let floor = outer - depth;
        validate_section(p.diameter, p.width, p.bore, floor)?;
        if [top, depth, bottom, outer].iter().any(|v| !v.is_finite())
            || top <= 0.0
            || depth <= d.above
            || bottom <= 0.0
        {
            return Err(format!(
                "溝の幅と深さでは V 溝を作れません。深さは {:.3} mm より大きく {:.3} mm より小さくしてください",
                d.above,
                top / (2.0 * tan)
            ));
        }
        let spacing = p.width / p.grooves as f64;
        if spacing <= top {
            return Err(format!(
                "溝が重なります。幅を {:.3} mm より大きくするか、溝の数か幅を小さくしてください",
                top * p.grooves as f64
            ));
        }
        let mut edges = vec![line("end_start", [b, 0.0], [outer, 0.0])];
        let mut at = [outer, 0.0];
        for i in 0..p.grooves {
            let center = (i as f64 + 0.5) * spacing;
            let points = [
                [outer, center - top / 2.0],
                [floor, center - bottom / 2.0],
                [floor, center + bottom / 2.0],
                [outer, center + top / 2.0],
            ];
            for (part, to) in ["land", "left", "floor", "right"].into_iter().zip(points) {
                edges.push(line(&format!("g{i}:{part}"), at, to));
                at = to;
            }
        }
        for (name, to) in [
            ("land_end", [outer, p.width]),
            ("end_finish", [b, p.width]),
            ("bore", [b, 0.0]),
        ] {
            edges.push(line(name, at, to));
            at = to;
        }
        Ok(Outline {
            edges,
            reference: vec![
                ("pitch", [0.0, 0.0], r),
                ("tip", [0.0, 0.0], outer),
                ("root", [0.0, 0.0], floor),
            ],
            warnings: Vec::new(),
            axial: true,
        })
    } else {
        validate_section(p.diameter, p.width, p.bore, r)?;
        if !p.crown.is_finite()
            || p.crown < 0.0
            || p.crown > p.width / 2.0
            || !(r + p.crown).is_finite()
        {
            return Err("中高の高さは有限の 0 以上、幅の半分以下にしてください".into());
        }
        let mut edges = vec![line("end_start", [b, 0.0], [r, 0.0])];
        if p.crown == 0.0 {
            edges.push(line("crown", [r, 0.0], [r, p.width]));
        } else {
            let radius = p.width * p.width / (8.0 * p.crown) + p.crown / 2.0;
            let center = [r + p.crown - radius, p.width / 2.0];
            if !radius.is_finite() || !center[0].is_finite() {
                return Err("中高の円弧を作れない寸法です".into());
            }
            let crown = arc("crown", center, radius, [r, 0.0], [r, p.width], true);
            // 大きな円の差し引きで端点を失う寸法は、開いた断面にせず入力で止める。
            let tolerance = 1e-8 * r.max(p.width).max(1.0);
            for (actual, expected) in [
                (crown.from_point(), [r, 0.0]),
                (crown.to_point(), [r, p.width]),
            ] {
                let distance = (actual[0] - expected[0]).hypot(actual[1] - expected[1]);
                if !distance.is_finite() || distance > tolerance {
                    return Err("中高の円弧の端点を保てない寸法です".into());
                }
            }
            edges.push(crown);
        }
        edges.push(line("end_finish", [r, p.width], [b, p.width]));
        edges.push(line("bore", [b, p.width], [b, 0.0]));
        Ok(Outline {
            edges,
            reference: vec![
                ("pitch", [0.0, 0.0], r),
                ("tip", [0.0, 0.0], r + p.crown),
                ("root", [0.0, 0.0], r),
            ],
            warnings: Vec::new(),
            axial: true,
        })
    }
}

#[cfg(test)]
#[path = "pulley_tests.rs"]
mod tests;
