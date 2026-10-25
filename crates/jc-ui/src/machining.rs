//! 加工の作業空間: 段の帯、使い回し、シミュレーションの再生。
//! 中のモジュールは crate の直下でも同じ名前で引ける(lib.rs の `use`)。

pub(crate) mod play_ui;
pub(crate) mod reuse_ui;
pub(crate) mod steps_ui;

#[cfg(test)]
mod machining_tests;
#[cfg(test)]
mod reuse_tests;
