use crate::{AIR_DENSITY, SOUND_SPEED, positive};
use kmb_core::Problem;
use serde::{Deserialize, Serialize};
use std::f64::consts::TAU;

/// ドライバーの入力。抵抗 Ω、インダクタンス H、周波数 Hz、容積 m³、面積 m²。
/// Q の欠けは Qts ともう一方の Q から求められる。任意の振幅は m、質量 kg、入力 W。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Driver {
    pub re: f64,
    pub le: f64,
    pub fs: f64,
    pub qms: Option<f64>,
    pub qes: Option<f64>,
    pub qts: Option<f64>,
    pub vas: f64,
    pub sd: f64,
    pub xmax: Option<f64>,
    pub bl: Option<f64>,
    pub mms: Option<f64>,
    pub pe: Option<f64>,
}

/// 検査済みの Q と機械の値。Cms は m/N、Mms は kg、Rms は N·s/m、Bl は T·m。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DerivedDriver {
    pub qms: f64,
    pub qes: f64,
    pub qts: f64,
    pub cms: f64,
    pub mms: f64,
    pub rms: f64,
    pub bl: f64,
}

fn agrees(name: &str, input: f64, calculated: f64, tolerance: f64) -> Result<(), Problem> {
    if (input - calculated).abs() <= tolerance * calculated.abs() {
        Ok(())
    } else {
        Err(format!("{name} の入力値 {input} と計算値 {calculated} が許容差 {}% を超えて違います。値の組を確かめてください", tolerance * 100.0).into())
    }
}

impl Driver {
    pub fn derive(&self) -> Result<DerivedDriver, Problem> {
        for (name, value) in [
            ("Re", self.re),
            ("fs", self.fs),
            ("Vas", self.vas),
            ("Sd", self.sd),
        ] {
            positive(name, value)?;
        }
        if !self.le.is_finite() || self.le < 0.0 {
            return Err("Le は 0 以上の有限な数にしてください".into());
        }
        for (name, value) in [
            ("Qms", self.qms),
            ("Qes", self.qes),
            ("Qts", self.qts),
            ("Xmax", self.xmax),
            ("Bl", self.bl),
            ("Mms", self.mms),
            ("Pe", self.pe),
        ] {
            if let Some(value) = value {
                positive(name, value)?;
            }
        }
        let (qms, qes) = match (self.qms, self.qes, self.qts) {
            (Some(m), Some(e), _) => (m, e),
            (Some(m), None, Some(t)) if m > t => (m, t * m / (m - t)),
            (None, Some(e), Some(t)) if e > t => (t * e / (e - t), e),
            _ => {
                return Err(
                    "Qms と Qes、または Qts とそれより大きい Qms か Qes の組が要ります".into(),
                );
            }
        };
        let qts = 1.0 / (1.0 / qms + 1.0 / qes);
        if let Some(input) = self.qts {
            agrees("Qts", input, qts, 0.02)?;
        }
        let cms = self.vas / (AIR_DENSITY * SOUND_SPEED.powi(2) * self.sd.powi(2));
        let omega = TAU * self.fs;
        let mms = 1.0 / (omega.powi(2) * cms);
        let rms = omega * mms / qms;
        let bl = (omega * mms * self.re / qes).sqrt();
        for (name, value) in [
            ("Qms", qms),
            ("Qes", qes),
            ("Qts", qts),
            ("Cms", cms),
            ("Mms", mms),
            ("Rms", rms),
            ("Bl", bl),
        ] {
            positive(name, value)?;
        }
        if let Some(input) = self.mms {
            agrees("Mms", input, mms, 0.05)?;
        }
        if let Some(input) = self.bl {
            agrees("Bl", input, bl, 0.05)?;
        }
        Ok(DerivedDriver {
            qms,
            qes,
            qts,
            cms,
            mms,
            rms,
            bl,
        })
    }
}
