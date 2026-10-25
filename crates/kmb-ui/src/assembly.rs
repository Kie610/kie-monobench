//! 組立: 部品の木、ジョイントを引く・試しに動かす、モーション スタディ、表の窓(部品表・干渉)。
//! 中のモジュールは crate の直下でも同じ名前で引ける(lib.rs の `use`)。

pub(crate) mod components_ui;
pub(crate) mod joint_drag_ui;
pub(crate) mod joint_preview_ui;
pub(crate) mod motion_ui;
pub(crate) mod table_ui;

#[cfg(test)]
mod components_tests;
#[cfg(test)]
mod joint_range_tests;
