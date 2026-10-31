//! 設計の作業空間: 左のブラウザ、下のタイムライン、右の編集パネル、中央の 3D のビュー、パラメータの窓、選ぶ・測る・断面・プレス/プル。
//! 中のモジュールは crate の直下でも同じ名前で引ける(lib.rs の `use`)。

pub(crate) mod actions_ui;
pub(crate) mod browser_ui;
pub(crate) mod config_ui;
pub(crate) mod doc_ui;
pub(crate) mod feature_clip_ui;
pub(crate) mod feature_ui;
pub(crate) mod find_ui;
pub(crate) mod measure_ui;
pub(crate) mod panel_ui;
pub(crate) mod param_views_ui;
pub(crate) mod params_ui;
pub mod picture_ui;
pub(crate) mod press_pull_ui;
pub(crate) mod review_ui;
pub(crate) mod section_ui;
pub(crate) mod select_ui;
pub(crate) mod timeline_ui;
pub(crate) mod value_box_ui;
pub(crate) mod view_ui;

#[cfg(test)]
mod browser_tests;
#[cfg(test)]
mod canvas_tests;
#[cfg(test)]
mod config_tests;
#[cfg(test)]
mod extrude_tests;
#[cfg(test)]
mod feature_tests;
#[cfg(test)]
mod find_tests;
#[cfg(test)]
mod hole_tests;
#[cfg(test)]
mod material_tests;
#[cfg(test)]
mod modify_tests;
#[cfg(test)]
mod param_batch_tests;
#[cfg(test)]
mod param_table_tests;
#[cfg(test)]
mod param_views_tests;
#[cfg(test)]
mod pick_tests;
#[cfg(test)]
mod press_pull_tests;
#[cfg(test)]
mod revolve_tests;
#[cfg(test)]
mod select_tests;
#[cfg(test)]
mod timeline_tests;
#[cfg(test)]
mod view_tests;
