//! 窓全体・PC 全体の画面の状態(構造のレビュー 段 2)。タブを替えても残る物(設定・画面の配置・
//! 最近使ったファイル・知らせ・開いているタブの並び)を置く。文書ごとの物は [`crate::doc_state::DocState`]、
//! 部品ごとの物(ドック・コマンド検索・設定の窓・ブラウザ・3D のビューのメニュー)と、開始画面・モデル管理の
//! 物([`crate::home::HomeState`])は各部品の struct に置く。

use crate::*;

/// 知らせの強さ。文の言い回しではなく、知らせを出す所が決める。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Tone {
    #[default]
    Info,
    Warn,
    Error,
}

/// 今の言語の文言(窓の状態を持たない、部品を描く関数のため。作るのは [`WinState::words`])。
#[derive(Clone)]
pub(crate) struct Words {
    texts: Arc<kmb_i18n::Catalog>,
    lang: kmb_i18n::Lang,
}

impl Words {
    pub(crate) fn find(&self, id: &str) -> Option<String> {
        self.texts.find(self.lang, id)
    }

    fn key() -> egui::Id {
        egui::Id::new("kmb_words")
    }

    /// egui の記憶に置く(毎フレーム、見た目と一緒に)。
    pub(crate) fn share(self, ctx: &egui::Context) {
        ctx.data_mut(|d| d.insert_temp(Words::key(), self));
    }

    /// egui の記憶に置いた文言。まだ無ければ日本語(試験で画面の部品だけを描く時)。
    pub(crate) fn of(ctx: &egui::Context) -> Words {
        ctx.data_mut(|d| {
            d.get_temp_mut_or_insert_with(Words::key(), || Words {
                texts: Arc::new(kmb_i18n::Catalog::with_os(kmb_i18n::Lang::Ja)),
                lang: kmb_i18n::Lang::Ja,
            })
            .clone()
        })
    }

    /// 文言 `id` の文。
    pub(crate) fn t(&self, id: &str) -> String {
        self.texts.text(self.lang, id)
    }

    /// 差し込み口のある文言。
    pub(crate) fn t_with(&self, id: &str, args: &[(&str, kmb_i18n::Arg)]) -> String {
        self.texts.text_with(self.lang, id, args)
    }
}

/// 窓全体・PC 全体の画面の状態。
pub(crate) struct WinState {
    pub(crate) registry: Arc<Registry>,
    pub(crate) message: Option<String>,
    /// 今の知らせの強さ(知らせを出す所が [`WinState::fail`]・[`WinState::warn`] で決める。履歴に残すと普通に戻る)
    pub(crate) message_tone: Tone,
    /// モジュールの操作(追加のボタン、右クリック、書き出し、重ね描き)
    pub(crate) extensions: Extensions,
    /// 再計算を別のスレッドで行うか(画面では真、テストでは偽)
    pub(crate) background: bool,
    /// 再計算が終わった時に画面を描き直させる
    pub(crate) repaint: Option<egui::Context>,
    /// ファイルのダイアログ(画面のアプリが渡す)
    pub(crate) dialogs: Box<dyn Dialogs>,
    /// ユーザーのデータフォルダ(なければ最近使ったファイルと自動保存を使わない)
    pub(crate) data_dir: Option<PathBuf>,
    /// 安全モード(利用者の設定と画面の配置を読まず、書きもしない。自動保存の復旧も出さない)
    pub(crate) safe_mode: bool,
    /// 今使っている描画の API と装置(設定の窓に出す。起動の時にアプリが決める)
    pub(crate) renderer_note: Option<String>,
    /// 3D マウスの読み手(画面のアプリが渡す。無ければ使わない)
    pub(crate) mouse3d: Option<kmb_ui_mouse3d::Mouse3d>,
    /// 録画向けの操作の表示。文書には残さない。
    pub(crate) keycast: crate::keycast::Keycast,
    /// 最後に描いた 3D のビューの枠(名前付きの視点を取り出す・戻すのに使う)
    pub(crate) view_rect: egui::Rect,
    /// `view_rect` を描いた時に並びに置いていた部品(出し入れで 3D のビューの片側が動いた時に、
    /// 形をその場に留める。まだ描いていなければ `None`)
    pub(crate) view_panes: Option<Vec<crate::dock_ui::Pane>>,
    /// 落ちた時の知らせ(開始画面と「ヘルプ」に出す)
    pub(crate) crash_page: help_ui::CrashPage,
    /// これまでの知らせ(誤りや完了の文。最大 100 件)
    pub(crate) history: Vec<String>,
    /// 知らせの時刻(UNIX 時刻の秒、`history` と同じ並び)
    pub(crate) history_at: Vec<u64>,
    /// 知らせの強さ(`history` と同じ並び。知らせの一覧の印の色)
    pub(crate) history_tone: Vec<Tone>,
    /// 左ドラッグの働き(ビューの下の帯で替える)
    pub(crate) left_drag: view3d::LeftDrag,
    /// 全画面にする前の、ブラウザ・タイムライン・編集パネルの開閉
    pub(crate) focus_mode: Option<(bool, bool, bool)>,
    /// 再生の速さ(機械の時間の何倍か)
    pub(crate) play_speed: f64,
    /// 今の色と作業空間(毎フレーム決める)
    pub(crate) look: theme::Look,
    /// 選べるテーマ(同梱と、データフォルダの `themes/` から読んだもの。U38)
    pub(crate) themes: themes::Themes,
    /// 画面の文言の訳(全言語。「自動」は起動の時の OS の言語。試験では日本語にそろえる)
    pub(crate) texts: Arc<kmb_i18n::Catalog>,
    /// 入れてある字の書体の設定(変わったら入れ直す。`None` はまだ見ていない)
    pub(crate) font_applied: Option<Option<String>>,
    /// 知らせの一覧を開いて見た時の数(鐘の数は、これより後ろの知らせ)
    pub(crate) notices_seen: usize,
    /// 取り消しの履歴の窓を開いているか
    pub(crate) undo_history_open: bool,
    /// 操作の手引きの窓(F1)
    pub(crate) guide_open: bool,
    /// 最後にツールバー・キーから始めたコマンド(右クリックの「繰り返す」に使う)
    pub(crate) last_item: Option<String>,
    /// キーの割り当て(設定の付け替えから作る)と、作った元の付け替え
    pub(crate) keymap: std::sync::Arc<keys::KeyMap>,
    pub(crate) keymap_src: Option<std::collections::BTreeMap<String, Vec<String>>>,
    /// 画面の配置(PC ごとに覚える)と、最後に書いた配置
    pub(crate) layout: Layout,
    pub(crate) layout_saved: Layout,
    /// デカールの読んだ絵の控え(文書が変わるたびに材質を作り直すので、毎回読み直さない)
    pub(crate) decal_images: kmb_render::decal::DecalImages,
    /// ツールバーで開いているタブ
    pub(crate) tab: String,
    /// 画面の速さ(設定の「画面の速さを出す」で 3D のビューに出す)
    pub(crate) perf: perf::Meter,
    /// 3D のビューで選ぶもの
    pub(crate) pick_filter: PickFilter,
    /// 右の編集パネルにパラメータを出しているか
    pub(crate) show_params: bool,
    /// タイムラインを縦の一覧で描いているか(縦長の所に置いた時。描くたびに決める)
    pub(crate) timeline_vertical: bool,
    /// アプリが持つ共有の形(外の窓が本体と別の時に中身を描く。`into_shared`)
    pub(crate) shared: Option<std::sync::Weak<std::sync::Mutex<Workspace>>>,
    /// 窓へ伝えた、標準の帯の有り無し
    pub(crate) decorations_sent: Option<bool>,
    /// キャンバス内レンダリングの止める目安(回す数。0 は止めない。止めて始め直しても覚えておく)
    pub(crate) canvas_target: u32,
    /// 開いている文書のタブ(候補 20)。今のタブの中身は作業場そのもの(tabs_ui)
    pub(crate) doc_tabs: Vec<tabs_ui::DocTab>,
    /// 今のタブの位置
    pub(crate) doc_tab: usize,
    /// タブの通し番号の最後
    pub(crate) tab_seq: u64,
    /// タブごとに形のカーネルを作る関数(無ければ分け合う)
    pub(crate) kernel_factory: Option<tabs_ui::KernelFactory>,
}

impl WinState {
    /// 今の表示の言語(設定の「表示の言語」)。
    pub(crate) fn lang(&self) -> kmb_i18n::Lang {
        self.texts
            .resolve(kmb_i18n::Choice::parse(&self.layout.appearance.language))
    }

    /// 文言 `id` の今の言語の文(`kmb-i18n` の `locales/`)。
    pub(crate) fn t(&self, id: &str) -> String {
        self.texts.text(self.lang(), id)
    }

    /// 今の言語の文言があればその文、無ければ `fallback`(訳の無い名前を画面がそのまま出す時。
    /// 日本語では引かずに `fallback` を出す)。
    pub(crate) fn t_or(&self, id: Option<&str>, fallback: &str) -> String {
        match (self.lang(), id) {
            (kmb_i18n::Lang::Ja, _) | (_, None) => fallback.to_string(),
            (lang, Some(id)) => self
                .texts
                .find(lang, id)
                .unwrap_or_else(|| fallback.to_string()),
        }
    }

    /// 操作の名前と説明を今の言語にする(分野 `action`、ID は [`kmb_i18n::action_id`])。日本語と、
    /// 訳の無い操作(後から足したモジュールの操作など)は、モジュールが付けた字のまま。
    pub(crate) fn localize_action(&self, a: &mut kmb_core::Action) {
        let id = kmb_i18n::action_id(&a.id);
        a.label = self.t_or(Some(&id), &a.label);
        if !a.hint.is_empty() {
            a.hint = self.t_or(Some(&format!("{id}-tip")), &a.hint);
        }
    }

    /// 作業空間の名前(ツールバーの左の箱と一覧)。
    pub(crate) fn space_name(&self, s: Space) -> String {
        self.t(match s {
            Space::Design => "toolbar-space-design",
            Space::Machining => "toolbar-space-machining",
            Space::Render => "toolbar-space-render",
            Space::Animation => "toolbar-space-animation",
            Space::Simulation => "toolbar-space-simulation",
        })
    }

    /// 今の言語の文言の手(部品を描く関数に渡す。egui の記憶にも毎フレーム置く [`Words::share`])。
    pub(crate) fn words(&self) -> Words {
        Words {
            texts: self.texts.clone(),
            lang: self.lang(),
        }
    }

    /// 差し込み口のある文言。
    pub(crate) fn t_with(&self, id: &str, args: &[(&str, kmb_i18n::Arg)]) -> String {
        self.texts.text_with(self.lang(), id, args)
    }

    /// 失敗の知らせを出す(知らせの一覧で失敗の色になる)。
    pub(crate) fn fail(&mut self, text: impl Into<String>) {
        self.message = Some(text.into());
        self.message_tone = Tone::Error;
    }

    /// 注意の知らせを出す(知らせの一覧で注意の色になる)。
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn warn(&mut self, text: impl Into<String>) {
        self.message = Some(text.into());
        self.message_tone = Tone::Warn;
    }
}
