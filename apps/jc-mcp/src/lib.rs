//! 文書を AI から操作する MCP サーバ。標準入出力で 1 つのクライアントとつなぐ。
//!
//! 道具: status、open、sample、save、set_param、rename_param、set_value(機能の値)、param_views・
//! edit_param_view(パラメータの表)、types、get_feature、
//! add_feature、set_feature、suppress、remove_feature、move_features、paste_features、
//! activate_component・move_component・move_bodies・duplicate_component(部品の木)、named_views・save_named_view・
//! rename_named_view・remove_named_view(名前を付けた視点)、undo、redo、mass、topology、measure、
//! screenshot(絵)、export、gcode、simulate、actions、run_action(モジュールの操作を画面と同じように
//! 使う)、crash_reports、batch(まとめて実行。途中で失敗したら全部戻す)、app(動いている画面へ。
//! 画面の部品を名前で押す・打つ ui_* と、コマンド欄のコマンドを流す command・script も)。
//! ファイルは作業フォルダ(起動した場所か `--root`)と `--allow` で許したフォルダの中だけを
//! 読み書きする(`..` やリンクで外へ出るパスも断る)。

pub mod batch;
pub mod line_cap;
pub mod reference;
pub mod server;
pub mod session;

pub use server::Server;
pub use session::Session;

#[cfg(test)]
mod tests;
