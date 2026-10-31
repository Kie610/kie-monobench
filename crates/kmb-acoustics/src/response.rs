use crate::{
    AIR_DENSITY, DerivedDriver, Driver, Enclosure, Excitation, REFERENCE_PRESSURE, SOUND_SPEED,
    circuit::Complex as C, positive,
};
use kmb_core::{Problem, ProblemKind};
use serde::{Deserialize, Serialize};
use std::f64::consts::{PI, SQRT_2, TAU};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FrequencyGrid {
    pub low_hz: f64,
    pub high_hz: f64,
    pub points_per_octave: u32,
}
impl Default for FrequencyGrid {
    fn default() -> Self {
        Self {
            low_hz: 10.0,
            high_hz: 2000.0,
            points_per_octave: 48,
        }
    }
}
impl FrequencyGrid {
    fn values(&self) -> Result<Vec<f64>, Problem> {
        positive("下限の周波数", self.low_hz)?;
        positive("上限の周波数", self.high_hz)?;
        if self.low_hz < 5.0 || self.high_hz > 20_000.0 || self.high_hz <= self.low_hz {
            return Err("周波数は 5 Hz〜20 kHz の中で、下限より上限を大きくしてください".into());
        }
        if !(1..=1536).contains(&self.points_per_octave) {
            return Err("1 オクターブの点の数は 1〜1536 にしてください".into());
        }
        Ok(log_grid(self.low_hz, self.high_hz, self.points_per_octave))
    }
}
fn log_grid(low: f64, high: f64, resolution: u32) -> Vec<f64> {
    let count = ((high / low).log2() * f64::from(resolution)).ceil() as usize;
    (0..=count)
        .map(|i| (low * 2.0_f64.powf(i as f64 / f64::from(resolution))).min(high))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub freq: Vec<f64>,
    pub spl_db: Vec<f64>,
    pub impedance_ohm: Vec<f64>,
    pub impedance_phase_deg: Vec<f64>,
    pub group_delay_ms: Vec<f64>,
    pub excursion_mm: Vec<f64>,
    pub port_velocity: Option<Vec<f64>>,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FrequencyRange {
    pub low_hz: f64,
    pub high_hz: f64,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Peak {
    pub value: f64,
    pub frequency_hz: f64,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub fc_hz: Option<f64>,
    pub qtc: Option<f64>,
    pub fb_hz: Option<f64>,
    pub port_length_m: Option<f64>,
    pub port_resonance_hz: Option<f64>,
    pub f3_hz: Option<f64>,
    pub maximum_excursion_mm: Option<Peak>,
    pub xmax_exceeded: Option<Vec<FrequencyRange>>,
    pub maximum_port_velocity: Option<Peak>,
    pub port_above_10: Option<Vec<FrequencyRange>>,
    pub port_above_17: Option<Vec<FrequencyRange>>,
    pub allowed_input_w: Option<f64>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Analysis {
    pub response: Response,
    pub summary: Summary,
    pub notes: Vec<String>,
}

/// 1 つの周波数の結果。音圧の実部・虚部は Pa、変位はピーク mm、風速はピーク m/s。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub pressure_re: f64,
    pub pressure_im: f64,
    pub spl_db: f64,
    pub impedance_ohm: f64,
    pub impedance_phase_deg: f64,
    pub excursion_mm: f64,
    pub port_velocity: Option<f64>,
}
impl Sample {
    fn phase(&self) -> f64 {
        self.pressure_im.atan2(self.pressure_re)
    }
}

struct Model<'a> {
    driver: &'a Driver,
    derived: DerivedDriver,
    excitation: &'a Excitation,
    cab: f64,
    rab: f64,
    port: Option<PortCircuit>,
    pressure_scale: f64,
    summary: Summary,
}
struct PortCircuit {
    area: f64,
    map: f64,
    rap: f64,
    ral: f64,
}
impl<'a> Model<'a> {
    fn new(
        driver: &'a Driver,
        enclosure: &Enclosure,
        excitation: &'a Excitation,
    ) -> Result<Self, Problem> {
        let derived = driver.derive()?;
        let volume = enclosure.volume()?;
        let cab = volume / (AIR_DENSITY * SOUND_SPEED.powi(2));
        positive("箱の音響コンプライアンス", cab)?;
        let mut summary = Summary {
            allowed_input_w: driver.pe,
            ..Default::default()
        };
        let (rab, port) = match enclosure {
            Enclosure::Sealed(b) => {
                positive("吸音の QA", b.qa)?;
                let factor = (1.0 + driver.vas / volume).sqrt();
                let fc = driver.fs * factor;
                summary.fc_hz = Some(fc);
                summary.qtc = Some(derived.qts * factor);
                (1.0 / (TAU * fc * cab * b.qa), None)
            }
            Enclosure::Vented(b) => {
                for (name, value) in [
                    ("漏れの QL", b.ql),
                    ("吸音の QA", b.qa),
                    ("ポートの QP", b.qp),
                ] {
                    positive(name, value)?;
                }
                let length = b.port.effective_length()?;
                let area = b.port.area_m2 * f64::from(b.port.count);
                let map = AIR_DENSITY * length / area;
                let omega = 1.0 / (map * cab).sqrt();
                positive("ポートの共振の角周波数", omega)?;
                summary.fb_hz = Some(omega / TAU);
                summary.port_length_m = Some(b.port.length_m);
                summary.port_resonance_hz = Some(SOUND_SPEED / (2.0 * length));
                (
                    1.0 / (omega * cab * b.qa),
                    Some(PortCircuit {
                        area,
                        map,
                        rap: 1.0 / (omega * cab * b.qp),
                        ral: b.ql / (omega * cab),
                    }),
                )
            }
        };
        let pressure_scale = excitation.scale()?;
        positive("箱の吸音の抵抗", rab)?;
        Ok(Self {
            driver,
            derived,
            excitation,
            cab,
            rab,
            port,
            pressure_scale,
            summary,
        })
    }

    fn at(&self, frequency: f64, le: f64) -> Result<Sample, Problem> {
        positive("計算する周波数", frequency)?;
        let w = TAU * frequency;
        let one = C::real(1.0);
        let compliance = C::real(self.rab) + C::imag(-1.0 / (w * self.cab));
        let zab = if let Some(p) = &self.port {
            one / (one / compliance
                + C::real(1.0 / p.ral)
                + one / (C::real(p.rap) + C::imag(w * p.map)))
        } else {
            compliance
        };
        let d = &self.derived;
        let zm = C::real(d.rms)
            + C::imag(w * d.mms - 1.0 / (w * d.cms))
            + C::real(self.driver.sd.powi(2)) * zab;
        let electric = C::real(self.driver.re) + C::imag(w * le);
        let total = zm + C::real(d.bl.powi(2)) / electric;
        let speed = (C::real(d.bl * self.excitation.voltage_rms) / electric) / total;
        let ud = C::real(self.driver.sd) * speed;
        let impedance = electric + C::real(d.bl.powi(2)) / zm;
        let (radiated, port_velocity) = if let Some(p) = &self.port {
            let pressure = C::real(-1.0) * zab * ud;
            let up = pressure / (C::real(p.rap) + C::imag(w * p.map));
            let ul = pressure / C::real(p.ral);
            (ud + up + ul, Some(SQRT_2 * up.norm() / p.area))
        } else {
            (ud, None)
        };
        // 伝播による位相を加えず、箱とドライバー自体の群遅延を調べる。
        let pressure = C::imag(w * self.pressure_scale) * radiated;
        let result = Sample {
            pressure_re: pressure.re,
            pressure_im: pressure.im,
            spl_db: 20.0 * (pressure.norm() / REFERENCE_PRESSURE).log10(),
            impedance_ohm: impedance.norm(),
            impedance_phase_deg: impedance.phase().to_degrees(),
            excursion_mm: SQRT_2 * speed.norm() / w * 1000.0,
            port_velocity,
        };
        if [
            result.pressure_re,
            result.pressure_im,
            result.spl_db,
            result.impedance_ohm,
            result.impedance_phase_deg,
            result.excursion_mm,
        ]
        .iter()
        .any(|v| !v.is_finite())
            || result.port_velocity.is_some_and(|v| !v.is_finite())
        {
            return Err(Problem::new(
                ProblemKind::Input,
                "値の桁が大きすぎるか小さすぎるため、有限な計算結果を作れません。単位と値を確かめてください",
            ));
        }
        Ok(result)
    }

    fn f3(&self, grid: &FrequencyGrid) -> Result<Option<f64>, Problem> {
        let reference = 10.0
            * self
                .driver
                .fs
                .max(self.summary.fc_hz.unwrap_or(0.0))
                .max(self.summary.fb_hz.unwrap_or(0.0));
        let threshold = self.at(reference, 0.0)?.spl_db - 3.0;
        if reference <= grid.low_hz {
            return Ok(None);
        }
        let frequencies = log_grid(grid.low_hz, reference, grid.points_per_octave);
        let mut previous: Option<(f64, f64)> = None;
        for f in frequencies.into_iter().rev() {
            let spl = self.at(f, self.driver.le)?.spl_db;
            if let Some((higher, above)) = previous
                && above >= threshold
                && spl < threshold
            {
                let t = (threshold - spl) / (above - spl);
                return Ok(Some((f.ln() + t * (higher.ln() - f.ln())).exp()));
            }
            previous = Some((f, spl));
        }
        Ok(None)
    }
}

/// 低周波の検証にも使える 1 点の計算。表示用の 5 Hz の下限は設けない。
pub fn sample(
    driver: &Driver,
    enclosure: &Enclosure,
    excitation: &Excitation,
    frequency: f64,
) -> Result<Sample, Problem> {
    Model::new(driver, enclosure, excitation)?.at(frequency, driver.le)
}

fn peak(frequencies: &[f64], values: &[f64]) -> Option<Peak> {
    values
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.total_cmp(b))
        .map(|(i, v)| Peak {
            value: *v,
            frequency_hz: frequencies[i],
        })
}

/// しきい値をまたぐ所は対数の周波数で線形に補間する。
fn ranges(frequencies: &[f64], values: &[f64], limit: f64) -> Vec<FrequencyRange> {
    let mut result = Vec::new();
    let mut start = (values[0] > limit).then_some(frequencies[0]);
    for i in 1..values.len() {
        let (a, b) = (values[i - 1], values[i]);
        if (a > limit) == (b > limit) {
            continue;
        }
        let t = (limit - a) / (b - a);
        let crossing =
            (frequencies[i - 1].ln() + t * (frequencies[i].ln() - frequencies[i - 1].ln())).exp();
        if b > limit {
            start = Some(crossing);
        } else if let Some(low_hz) = start.take() {
            result.push(FrequencyRange {
                low_hz,
                high_hz: crossing,
            });
        }
    }
    if let Some(low_hz) = start {
        result.push(FrequencyRange {
            low_hz,
            high_hz: frequencies[frequencies.len() - 1],
        });
    }
    result
}

fn group_delay(frequencies: &[f64], samples: &[Sample]) -> Vec<f64> {
    let mut phase = vec![samples[0].phase()];
    for (i, sample) in samples.iter().enumerate().skip(1) {
        let mut delta = sample.phase() - samples[i - 1].phase();
        while delta > PI {
            delta -= TAU;
        }
        while delta < -PI {
            delta += TAU;
        }
        phase.push(phase[i - 1] + delta);
    }
    let n = phase.len();
    (0..n)
        .map(|i| {
            let derivative = if i == 0 {
                (phase[1] - phase[0]) / (TAU * (frequencies[1] - frequencies[0]))
            } else if i == n - 1 {
                (phase[i] - phase[i - 1]) / (TAU * (frequencies[i] - frequencies[i - 1]))
            } else {
                let h0 = TAU * (frequencies[i] - frequencies[i - 1]);
                let h1 = TAU * (frequencies[i + 1] - frequencies[i]);
                (h1 * (phase[i] - phase[i - 1]) / h0 + h0 * (phase[i + 1] - phase[i]) / h1)
                    / (h0 + h1)
            };
            -1000.0 * derivative
        })
        .collect()
}

pub fn analyze(
    driver: &Driver,
    enclosure: &Enclosure,
    excitation: &Excitation,
    grid: &FrequencyGrid,
) -> Result<Analysis, Problem> {
    let frequencies = grid.values()?;
    let model = Model::new(driver, enclosure, excitation)?;
    let samples = frequencies
        .iter()
        .map(|f| model.at(*f, driver.le))
        .collect::<Result<Vec<_>, _>>()?;
    let mut summary = model.summary.clone();
    summary.f3_hz = model.f3(grid)?;
    let excursion_mm = samples.iter().map(|p| p.excursion_mm).collect::<Vec<_>>();
    summary.maximum_excursion_mm = peak(&frequencies, &excursion_mm);
    summary.xmax_exceeded = driver
        .xmax
        .map(|x| ranges(&frequencies, &excursion_mm, x * 1000.0));
    let port_velocity = model.port.as_ref().map(|_| {
        samples
            .iter()
            .map(|s| s.port_velocity.expect("ポート付き回路の値"))
            .collect::<Vec<_>>()
    });
    let mut notes=vec!["空気は 20 °C、密度 1.204 kg/m³、音速 343.2 m/s としています。".into(),"集中定数による目安です。コーンの分割振動の手前までを扱い、箱の面の振動や室内の反射は入りません。".into()];
    if let Some(values) = &port_velocity {
        summary.maximum_port_velocity = peak(&frequencies, values);
        summary.port_above_10 = Some(ranges(&frequencies, values, 10.0));
        let high = ranges(&frequencies, values, 17.0);
        for range in &high {
            notes.push(format!(
                "ポートの風速が 17 m/s を超えます({:.3}〜{:.3} Hz)。断面と入力を確かめてください。",
                range.low_hz, range.high_hz
            ));
        }
        summary.port_above_17 = Some(high);
    }
    let response = Response {
        group_delay_ms: group_delay(&frequencies, &samples),
        freq: frequencies,
        spl_db: samples.iter().map(|s| s.spl_db).collect(),
        impedance_ohm: samples.iter().map(|s| s.impedance_ohm).collect(),
        impedance_phase_deg: samples.iter().map(|s| s.impedance_phase_deg).collect(),
        excursion_mm,
        port_velocity,
    };
    Ok(Analysis {
        response,
        summary,
        notes,
    })
}
