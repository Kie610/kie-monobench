//! ローラーチェーンの鎖車。座と側面を規格の中間寸法の円弧でつなぐ。

use super::{Outline, Params};
use crate::tooth::{P2, arc, mirror, polar, rotate};
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

/// 鎖の呼び。手入力の寸法は呼びが `Manual` の時だけ使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ChainSize {
    #[serde(rename = "25")]
    Size25,
    #[serde(rename = "35")]
    Size35,
    #[default]
    #[serde(rename = "40")]
    Size40,
    #[serde(rename = "50")]
    Size50,
    #[serde(rename = "60")]
    Size60,
    #[serde(rename = "80")]
    Size80,
    #[serde(rename = "manual")]
    Manual,
}

impl ChainSize {
    pub const CHOICES: [(&'static str, &'static str); 7] = [
        ("40", "40"),
        ("25", "25"),
        ("35", "35 (ブシュ)"),
        ("50", "50"),
        ("60", "60"),
        ("80", "80"),
        ("manual", "手で打つ"),
    ];

    pub fn dimensions(self) -> Option<Chain> {
        let (pitch, roller, inner_width) = match self {
            Self::Size25 => (6.35, 3.30, 3.18),
            Self::Size35 => (9.525, 5.08, 4.77),
            Self::Size40 => (12.70, 7.92, 7.95),
            Self::Size50 => (15.875, 10.16, 9.53),
            Self::Size60 => (19.05, 11.91, 12.70),
            Self::Size80 => (25.40, 15.88, 15.88),
            Self::Manual => return None,
        };
        Some(Chain {
            pitch,
            roller,
            inner_width,
        })
    }
}

/// 鎖の寸法(mm)。呼びの表の値も手入力も同じ形の計算へ渡す。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Chain {
    pub pitch: f64,
    pub roller: f64,
    pub inner_width: f64,
}

impl Chain {
    pub fn validate(self) -> Result<Self, String> {
        if [self.pitch, self.roller, self.inner_width]
            .iter()
            .any(|v| !v.is_finite() || *v <= 0.0)
        {
            return Err(
                "鎖のピッチ・ローラーの径・鎖の内幅は有限の 0 より大きい長さにしてください".into(),
            );
        }
        if self.roller >= self.pitch {
            return Err("ローラーの径は鎖のピッチより小さくしてください".into());
        }
        Ok(self)
    }

    pub fn default_width(self) -> f64 {
        self.inner_width * if self.pitch <= 12.7 { 0.93 } else { 0.95 }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Dimensions {
    pub pitch_radius: f64,
    pub tip_radius: f64,
    /// 規格の呼び寸法。座にはローラーの逃げを含むので最深部はこれより内側になる。
    pub root_radius: f64,
    pub seat_radius: f64,
    pub seat_angle: f64,
    pub flank_radius: f64,
}

pub fn dimensions(chain: Chain, teeth: u32) -> Result<Dimensions, String> {
    let c = chain.validate()?;
    if teeth < 3 {
        return Err("鎖車の歯数は 3 以上にしてください".into());
    }
    let z = teeth as f64;
    let d = c.pitch / (PI / z).sin();
    let da_min = d + c.pitch * (1.0 - 1.6 / z) - c.roller;
    let da_max = d + 1.25 * c.pitch - c.roller;
    let out = Dimensions {
        pitch_radius: d / 2.0,
        tip_radius: (da_min + da_max) / 4.0,
        root_radius: (d - c.roller) / 2.0,
        seat_radius: 0.505 * c.roller + 0.0345 * c.roller.cbrt(),
        seat_angle: (130.0 - 90.0 / z).to_radians(),
        flank_radius: 0.004 * c.roller * (z * z + 180.0) + 0.06 * c.roller * (z + 2.0),
    };
    if [
        out.pitch_radius,
        out.tip_radius,
        out.root_radius,
        out.seat_radius,
        out.flank_radius,
    ]
    .iter()
    .any(|v| !v.is_finite() || *v <= 0.0)
        || out.seat_radius >= out.pitch_radius
    {
        return Err("鎖車の寸法が作れる範囲を超えます。鎖の寸法と歯数を見直してください".into());
    }
    Ok(out)
}

/// 歯先円と側面の円の交点のうち、隣の歯溝までの範囲にある点。
fn tip_point(center: P2, radius: f64, tip: f64, half_pitch: f64) -> Result<P2, String> {
    let distance = center[0].hypot(center[1]);
    let along = (tip * tip - radius * radius + distance * distance) / (2.0 * distance);
    let height2 = tip * tip - along * along;
    if !along.is_finite() || !height2.is_finite() || height2 <= 0.0 {
        return Err(
            "座と側面の円弧を歯先円までつなげません。鎖の寸法と歯数を見直してください".into(),
        );
    }
    let unit = [center[0] / distance, center[1] / distance];
    let height = height2.sqrt();
    [-1.0, 1.0]
        .into_iter()
        .map(|s| {
            [
                along * unit[0] - s * height * unit[1],
                along * unit[1] + s * height * unit[0],
            ]
        })
        .find(|p| {
            let angle = p[1].atan2(p[0]);
            angle > 0.0 && angle < half_pitch
        })
        .ok_or_else(|| "歯先の円弧が隣の歯に重なります。鎖の寸法と歯数を見直してください".into())
}

pub fn outline(p: &Params) -> Result<Outline, String> {
    let chain = p
        .chain
        .unwrap_or_else(|| ChainSize::default().dimensions().expect("呼びの寸法"));
    let d = dimensions(chain, p.teeth)?;
    let half_pitch = PI / p.teeth as f64;
    let step = 2.0 * half_pitch;
    let half_seat = d.seat_angle / 2.0;
    let seat_center = [d.pitch_radius, 0.0];
    let side = [-half_seat.cos(), half_seat.sin()];
    let join = [
        seat_center[0] + d.seat_radius * side[0],
        d.seat_radius * side[1],
    ];
    // 外接する 2 円なので、接点では座と側面の進む向きが一致する。
    let flank_center = [
        seat_center[0] + (d.seat_radius + d.flank_radius) * side[0],
        (d.seat_radius + d.flank_radius) * side[1],
    ];
    let tip = tip_point(flank_center, d.flank_radius, d.tip_radius, half_pitch)?;
    let seat = arc(
        "",
        seat_center,
        d.seat_radius,
        [join[0], -join[1]],
        join,
        false,
    );
    let flank = arc("", flank_center, d.flank_radius, join, tip, true);
    let next_tip = polar(d.tip_radius, step - tip[1].atan2(tip[0]));
    let top = arc("", [0.0, 0.0], d.tip_radius, tip, next_tip, true);
    let back = rotate(&mirror(&flank, ""), step);
    let mut edges = Vec::new();
    for i in 0..p.teeth {
        let base = step * i as f64 - half_pitch;
        for (name, e) in [
            (format!("t{}:root", (i + p.teeth - 1) % p.teeth), &seat),
            (format!("t{i}:r0"), &flank),
            (format!("t{i}:tip"), &top),
            (format!("t{i}:l0"), &back),
        ] {
            let mut placed = rotate(e, base);
            placed.name = name;
            edges.push(placed);
        }
    }
    Ok(Outline {
        edges,
        reference: vec![
            ("pitch", [0.0, 0.0], d.pitch_radius),
            ("tip", [0.0, 0.0], d.tip_radius),
            ("root", [0.0, 0.0], d.root_radius),
        ],
        warnings: Vec::new(),
        reference_lines: Vec::new(),
    })
}

#[cfg(test)]
#[path = "sprocket_tests.rs"]
mod tests;
