//! ホーム(開始画面)とモデル管理: ファイルを開く・保存、版の履歴と見比べ。
//! 中のモジュールは crate の直下でも同じ名前で引ける(lib.rs の `use`)。

pub(crate) mod compare_ui;
pub(crate) mod files_ui;
pub(crate) mod home;
pub(crate) mod home_ui;
pub(crate) mod versions_ui;

#[cfg(test)]
mod files_tests;
