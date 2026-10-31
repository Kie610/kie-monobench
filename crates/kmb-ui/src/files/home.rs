//! 開始画面(ホーム)とモデル管理の状態。窓全体の状態の 1 区画で、タブを替えても残る。

use crate::*;

/// 開始画面(ホーム)とモデル管理の状態。
pub(crate) struct HomeState {
    /// 開始画面を出しているか
    pub(crate) start: bool,
    /// 自動保存の写しから戻せる文書
    pub(crate) recoveries: Vec<kmb_session::Recovery>,
    /// 全部捨てると確かめた写しのパス。対象が変われば確認をやり直す。
    pub(crate) discard_recoveries: Option<Vec<std::path::PathBuf>>,
    /// 登録したプロジェクト(PC のフォルダ)
    pub(crate) projects: kmb_session::projects::Projects,
    /// 最近使ったファイル
    pub(crate) recent: kmb_session::Recent,
    /// 最近使ったファイルのうち見つからないもの(一覧を変えた時と開始画面を出した時に調べる。
    /// 毎フレーム調べると、つながっていないネットワークのドライブで止まるため)
    pub(crate) recent_missing: crate::files_ui::RecentMissing,
    /// 開始画面の見本
    pub(crate) samples: Vec<Sample>,
    /// ホームの中身を描く拡張(アプリは kmb-ui-library。無ければ簡単な頁)
    pub(crate) page: Option<Box<dyn crate::home_ui::HomePage>>,
    /// 前のフレームでホームを描いたか(出し直した最初のフレームを拡張に知らせる)
    pub(crate) drawn: bool,
    /// 戻せる仕分け(古い順)
    pub(crate) undo: Vec<crate::home_ui::FileUndo>,
    /// 仕分けでファイルが変わった回数(拡張がフォルダを読み直す合図)
    pub(crate) changed: u64,
    /// 直前に追加したプロジェクト(次のフレームで拡張へ渡す)
    pub(crate) added: Option<std::path::PathBuf>,
    /// データパネルを最後に描いた描画の番号(出し直した最初の描画を拡張に知らせる)
    pub(crate) panel_pass: u64,
    /// 名前を付けて保存した絞り込み(ホームの左の「保存した絞り込み」)
    pub(crate) filters: kmb_session::filters::Saved,
}
