//! 密閉箱とバスレフ箱の集中定数の計算。値は SI、振幅と風速の結果はピークで返す。
mod circuit;
mod driver;
mod response;

pub use circuit::{Enclosure, Excitation, Port, PortEnds, Radiation, Sealed, Vented, port_length};
pub use driver::{DerivedDriver, Driver};
pub use response::{
    Analysis, FrequencyGrid, FrequencyRange, Peak, Response, Sample, Summary, analyze, sample,
};

pub const AIR_DENSITY: f64 = 1.204;
pub const SOUND_SPEED: f64 = 343.2;
pub(crate) const REFERENCE_PRESSURE: f64 = 20e-6;

/// 計算の名乗り。文書の機能や操作は登録しない。
pub fn module() -> kmb_core::Module {
    kmb_core::Module::new("kmb-acoustics", |_| {})
}

fn positive(name: &str, value: f64) -> Result<(), kmb_core::Problem> {
    if value.is_finite() && value > 0.0 {
        Ok(())
    } else {
        Err(format!("{name} は正の有限な数にしてください(入力 {value})").into())
    }
}

#[cfg(test)]
mod tests;
