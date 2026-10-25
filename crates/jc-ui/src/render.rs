//! レンダリングの作業空間: 写実の描画、ギャラリー、外観。
//! 中のモジュールは crate の直下でも同じ名前で引ける(lib.rs の `use`)。

pub(crate) mod gallery_ui;
pub(crate) mod looks_ui;
pub(crate) mod render_ui;

#[cfg(test)]
mod looks_tests;
#[cfg(test)]
mod render_tests;
