use crate::{AIR_DENSITY, SOUND_SPEED, positive};
use kmb_core::Problem;
use serde::{Deserialize, Serialize};
use std::{
    f64::consts::{PI, TAU},
    ops::{Add, Div, Mul, Sub},
};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PortEnds {
    #[default]
    BaffleAndOpen,
    BothFlanged,
    BothOpen,
}
impl PortEnds {
    fn correction(self) -> f64 {
        match self {
            Self::BaffleAndOpen => 1.46,
            Self::BothFlanged => 1.7,
            Self::BothOpen => 1.22,
        }
    }
}

/// ポートの本数、1 本の断面 m²、物理長 m、端の形。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Port {
    pub count: u32,
    pub area_m2: f64,
    pub length_m: f64,
    #[serde(default)]
    pub ends: PortEnds,
}
impl Port {
    pub fn effective_length(&self) -> Result<f64, Problem> {
        check_port(self.count, self.area_m2)?;
        positive("ポートの物理長", self.length_m)?;
        let length = self.length_m + self.ends.correction() * (self.area_m2 / PI).sqrt();
        positive("ポートの有効長", length)?;
        Ok(length)
    }
}
fn check_port(count: u32, area: f64) -> Result<(), Problem> {
    if count == 0 {
        return Err("ポートは 1 本以上にしてください".into());
    }
    positive("ポート 1 本の断面", area)
}

/// 実効容積 m³ と狙いの fb Hz から物理長 m を求める。
pub fn port_length(
    volume_m3: f64,
    area_m2: f64,
    count: u32,
    fb_hz: f64,
    ends: PortEnds,
) -> Result<f64, Problem> {
    positive("箱の実効容積", volume_m3)?;
    positive("fb", fb_hz)?;
    check_port(count, area_m2)?;
    let length = SOUND_SPEED.powi(2) * area_m2 * f64::from(count)
        / ((TAU * fb_hz).powi(2) * volume_m3)
        - ends.correction() * (area_m2 / PI).sqrt();
    if !length.is_finite() || length <= 0.0 {
        return Err("ポートの断面が大きすぎて、この容積と fb では管が作れません。断面を小さくするか本数を減らしてください".into());
    }
    Ok(length)
}

/// 容積 m³、吸音材による容積の増分の比、吸音の Q。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Sealed {
    pub volume_m3: f64,
    pub absorption: f64,
    pub qa: f64,
}
impl Default for Sealed {
    fn default() -> Self {
        Self {
            volume_m3: 0.02,
            absorption: 0.0,
            qa: 100.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Vented {
    pub volume_m3: f64,
    pub absorption: f64,
    pub port: Port,
    pub ql: f64,
    pub qa: f64,
    pub qp: f64,
}
impl Default for Vented {
    fn default() -> Self {
        Self {
            volume_m3: 0.02,
            absorption: 0.0,
            port: Port {
                count: 1,
                area_m2: PI * 0.025_f64.powi(2),
                length_m: 0.14656896800984692,
                ends: PortEnds::default(),
            },
            ql: 7.0,
            qa: 100.0,
            qp: 100.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "values", rename_all = "snake_case")]
pub enum Enclosure {
    Sealed(Sealed),
    Vented(Vented),
}
impl Enclosure {
    pub(crate) fn volume(&self) -> Result<f64, Problem> {
        let (v, a) = match self {
            Self::Sealed(b) => (b.volume_m3, b.absorption),
            Self::Vented(b) => (b.volume_m3, b.absorption),
        };
        positive("箱の容積", v)?;
        if !a.is_finite() || a < 0.0 {
            return Err("吸音材の補正は 0 以上の有限な比にしてください".into());
        }
        let effective = v * (1.0 + a);
        positive("箱の実効容積", effective)?;
        Ok(effective)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Radiation {
    #[default]
    HalfSpace,
    FullSpace,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Excitation {
    pub voltage_rms: f64,
    pub distance_m: f64,
    pub radiation: Radiation,
}
impl Default for Excitation {
    fn default() -> Self {
        Self {
            voltage_rms: 2.83,
            distance_m: 1.0,
            radiation: Radiation::HalfSpace,
        }
    }
}
impl Excitation {
    pub(crate) fn scale(&self) -> Result<f64, Problem> {
        positive("入力の実効値の電圧", self.voltage_rms)?;
        positive("音圧を求める距離", self.distance_m)?;
        let angle = match self.radiation {
            Radiation::HalfSpace => TAU,
            Radiation::FullSpace => 2.0 * TAU,
        };
        Ok(AIR_DENSITY / (angle * self.distance_m))
    }
}

/// 回路だけで使う複素数。時間の向きは exp(jωt)。
#[derive(Clone, Copy, Debug)]
pub(crate) struct Complex {
    pub re: f64,
    pub im: f64,
}
impl Complex {
    pub fn real(re: f64) -> Self {
        Self { re, im: 0.0 }
    }
    pub fn imag(im: f64) -> Self {
        Self { re: 0.0, im }
    }
    pub fn norm(self) -> f64 {
        self.re.hypot(self.im)
    }
    pub fn phase(self) -> f64 {
        self.im.atan2(self.re)
    }
}
impl Add for Complex {
    type Output = Self;
    fn add(self, b: Self) -> Self {
        Self {
            re: self.re + b.re,
            im: self.im + b.im,
        }
    }
}
impl Sub for Complex {
    type Output = Self;
    fn sub(self, b: Self) -> Self {
        Self {
            re: self.re - b.re,
            im: self.im - b.im,
        }
    }
}
impl Mul for Complex {
    type Output = Self;
    fn mul(self, b: Self) -> Self {
        Self {
            re: self.re * b.re - self.im * b.im,
            im: self.re * b.im + self.im * b.re,
        }
    }
}
impl Div for Complex {
    type Output = Self;
    fn div(self, b: Self) -> Self {
        let d = b.re * b.re + b.im * b.im;
        Self {
            re: (self.re * b.re + self.im * b.im) / d,
            im: (self.im * b.re - self.re * b.im) / d,
        }
    }
}
