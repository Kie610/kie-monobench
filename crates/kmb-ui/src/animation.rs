//! アニメーションの作業空間。
//! 中のモジュールは crate の直下でも同じ名前で引ける(lib.rs の `use`)。

pub(crate) mod animation_ui;

#[cfg(test)]
mod animation_tests;
