//! ファイルの扱い: ホーム(開始画面)の状態、新規・開く・保存・名前を付けて保存、未保存の確認、自動保存。
//!
//! OS のファイルダイアログは画面のアプリ(kmb-app)が [`Dialogs`] として渡す。
//! この crate はダイアログのライブラリに依存しない(テストでは決まった答えを返す)。

use crate::Workspace;
use kmb_core::Document;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// 未保存の変更があるときの答え。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unsaved {
    /// 保存してから続ける
    Save,
    /// 変更を捨てて続ける
    Discard,
    /// やめる
    Cancel,
}

/// ファイルの選択と確認のダイアログ(`Send` の訳は `FeatureEditor` と同じ)。
pub trait Dialogs: Send {
    /// 開く文書を選ぶ。
    fn open_document(&self) -> Option<PathBuf>;
    /// 保存先を選ぶ。`name` は勧めるファイル名。
    fn save_document(&self, name: &str) -> Option<PathBuf>;
    /// 読み込むファイルを選ぶ。`extensions` は点なしの拡張子(例: `step`)。
    fn open_file(&self, title: &str, filter: &str, extensions: &[&str]) -> Option<PathBuf>;
    /// 書き出す先を選ぶ。`name` は勧めるファイル名。
    fn save_file(
        &self,
        title: &str,
        filter: &str,
        extensions: &[&str],
        name: &str,
    ) -> Option<PathBuf>;
    /// 未保存の変更をどうするか。`name` は文書の名前。
    fn unsaved(&self, name: &str) -> Unsaved;
    /// フォルダを選ぶ(プロジェクトを足す時)。選べない画面では何も選ばない。
    fn pick_folder(&self, _title: &str) -> Option<PathBuf> {
        None
    }
    /// 窓を出せない理由(出せるなら `None`)。選ばずに閉じたのか、窓が出なかったのかを
    /// 分けて知らせるため(Z24: Linux で zenity が無くても「選ばれなかった」と出ていた)
    fn unavailable(&self) -> Option<String> {
        None
    }
}

/// ダイアログを出さない(何も選ばない、未保存の確認ではやめる)。
pub struct NoDialogs;

impl Dialogs for NoDialogs {
    fn open_document(&self) -> Option<PathBuf> {
        None
    }
    fn save_document(&self, _: &str) -> Option<PathBuf> {
        None
    }
    fn open_file(&self, _: &str, _: &str, _: &[&str]) -> Option<PathBuf> {
        None
    }
    fn save_file(&self, _: &str, _: &str, _: &[&str], _: &str) -> Option<PathBuf> {
        None
    }
    fn unsaved(&self, _: &str) -> Unsaved {
        Unsaved::Cancel
    }
}

/// 最近使ったファイルのうち見つからない物と、裏で調べている途中の結果の受け口。
#[derive(Default)]
pub(crate) struct RecentMissing {
    pub set: std::collections::HashSet<PathBuf>,
    pub pending: Option<std::sync::mpsc::Receiver<std::collections::HashSet<PathBuf>>>,
}

/// 開いている途中の文書(U8)。計算が終わるまで前の文書を持っておき、「キャンセル」で戻す。
pub(crate) struct Opening {
    /// 開いているファイルの名前
    pub name: String,
    pub since: std::time::Instant,
    pub back: Back,
}

/// 開くのをやめた時の戻し方。
pub(crate) enum Back {
    /// 前の文書に戻す(文書を開く時)
    Document {
        doc: Box<Document>,
        path: Option<PathBuf>,
        modified: bool,
        saved: Box<kmb_core::DocumentData>,
        report: Box<kmb_core::RecomputeReport>,
    },
    /// 読み込んだ機能を取り消す(STEP を読み込む時)
    Undo,
}

/// 開く窓を出すまでの時間(すぐ終わる時はちらつかせない)。
const OPENING_DELAY: std::time::Duration = std::time::Duration::from_millis(400);

/// 最近使ったファイルの手入れ(U15)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecentAction {
    /// ピン留め(真)・ピン留めを外す(偽)
    Pin(bool),
    /// 一覧から外す
    Remove,
    /// ピン留めしていないものを全部外す
    ClearUnpinned,
    /// ファイルのあるフォルダを開く
    ShowInFolder,
}

/// 見本の文書(開始画面に出す)。
pub type Sample = (String, fn() -> Document);

impl Workspace {
    /// ファイルのダイアログを渡す。
    pub fn with_dialogs(mut self, d: Box<dyn Dialogs>) -> Self {
        self.win.dialogs = d;
        self
    }

    /// ユーザーのデータフォルダ(最近使ったファイル、自動保存の写し)を使う。
    pub fn with_data_dir(mut self, dir: PathBuf) -> Self {
        self.home.recent = kmb_session::Recent::load(&dir);
        self.check_recent();
        self.home.projects = kmb_session::projects::Projects::load(&dir);
        self.home.filters = kmb_session::filters::Saved::load(&dir);
        if !self.win.safe_mode {
            self.home.recoveries = kmb_session::recoveries(&dir);
        }
        self.cur.autosave = Some(kmb_session::Autosave::new(&dir));
        for e in &mut self.cur.editors {
            e.set_data_dir(Some(&dir));
        }
        // 加工のテンプレートなど、モジュールが PC 全体のファイルを置く所
        self.win.extensions.set_data_dir(Some(&dir));
        if !self.win.safe_mode {
            let problems = kmb_session::load_settings(&self.win.extensions, &dir);
            if !problems.is_empty() {
                self.win.fail(problems.join("\n"));
            }
            self.win.layout = crate::Layout::load(&dir);
            // 利用者のテーマも利用者の設定なので、安全モードでは読まない(同梱のテーマだけ)
            self.win.themes = crate::themes::Themes::load(Some(&dir));
        }
        self.win.layout_saved = self.win.layout.clone();
        self.win.data_dir = Some(dir);
        self
    }

    /// 安全モードにする([`Self::with_data_dir`] より前に呼ぶ)。利用者の設定と画面の配置を読まず、
    /// 変えても書かない(次の普通の起動で元に戻る)。自動保存の復旧も出さない(写しは残る)。
    pub fn with_safe_mode(mut self) -> Self {
        self.win.safe_mode = true;
        self
    }

    /// 今使っている描画の API と装置を、設定の窓の「描画」に出す。
    pub fn set_renderer_note(&mut self, note: String) {
        self.win.renderer_note = Some(note);
    }

    pub fn is_safe_mode(&self) -> bool {
        self.win.safe_mode
    }

    /// 起動の時に知らせること(壊れた設定を退けた、安全モードで始めた、など)を出す。
    pub fn with_notice(mut self, notes: &[String]) -> Self {
        if !notes.is_empty() {
            let mut all: Vec<String> = notes.to_vec();
            all.extend(self.win.message.take());
            self.win.message = Some(all.join("\n"));
        }
        self
    }

    /// 開始画面に出す見本。
    pub fn with_samples(mut self, s: Vec<Sample>) -> Self {
        self.home.samples = s;
        self
    }

    /// 開始画面を出す。
    pub fn show_start(mut self) -> Self {
        self.home.start = true;
        // 起動の時の無題の文書にも、新しい文書の既定の値を入れる(候補 19)
        if self.is_blank() && self.win.layout.new_doc != crate::layout::NewDoc::default() {
            self.replace_document(self.blank_document(), None);
        }
        self
    }

    /// 新しい文書(設定の「新しい文書」の既定の値が入る。候補 19)。
    pub(crate) fn blank_document(&self) -> Document {
        Document::with_properties(self.win.layout.new_doc.properties())
    }

    pub fn is_start_screen(&self) -> bool {
        self.home.start
    }

    pub fn path(&self) -> Option<&Path> {
        self.cur.work.path()
    }

    pub fn recent_files(&self) -> Vec<PathBuf> {
        self.home.recent.existing()
    }

    /// 最近使ったファイルの一覧(留めたものが先)と、見つからないかどうか。
    pub fn recent_entries(&self) -> Vec<(PathBuf, bool)> {
        self.home
            .recent
            .list()
            .into_iter()
            .map(|p| {
                let missing = self.home.recent_missing.set.contains(&p);
                (p, missing)
            })
            .collect()
    }

    /// 最近使ったファイルがあるかを、裏のスレッドで調べ直す(つながっていないネットワークの
    /// ドライブで開始画面を止めない)。結果は [`Self::poll_recent`] で受ける。
    pub(crate) fn check_recent(&mut self) {
        let list = self.home.recent.list();
        let (tx, rx) = std::sync::mpsc::channel();
        let wake = self.win.repaint.clone();
        let spawned = std::thread::Builder::new()
            .name("最近のファイル".into())
            .spawn(move || {
                let missing = list.into_iter().filter(|p| !p.exists()).collect();
                if tx.send(missing).is_ok()
                    && let Some(c) = wake
                {
                    c.request_repaint();
                }
            });
        match spawned {
            Ok(_) => self.home.recent_missing.pending = Some(rx),
            Err(e) => log::warn!("最近のファイルを調べるスレッドを作れません: {e}"),
        }
    }

    /// 裏で調べた最近のファイルの有無を受ける(調べ終えていなければ前の結果のまま)。
    pub(crate) fn poll_recent(&mut self) {
        let m = &mut self.home.recent_missing;
        if let Some(rx) = &m.pending {
            match rx.try_recv() {
                Ok(missing) => {
                    m.set = missing;
                    m.pending = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => m.pending = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
    }

    /// 最近使ったファイルの一覧を手入れする(U15)。変えたら保存する。
    pub fn recent_action(&mut self, path: &Path, act: RecentAction) {
        match act {
            RecentAction::Pin(pin) => self.home.recent.set_pinned(path, pin),
            RecentAction::Remove => self.home.recent.remove(path),
            RecentAction::ClearUnpinned => self.home.recent.clear_unpinned(),
            RecentAction::ShowInFolder => {
                if let Err(e) = kmb_session::show_in_folder(path) {
                    self.win.fail(e);
                }
                return;
            }
        }
        self.check_recent();
        if let Some(d) = &self.win.data_dir
            && let Err(e) = self.home.recent.save(d)
        {
            self.win.fail(e);
        }
    }

    /// 最近使ったファイルを開く。見つからなければ理由を出す(一覧からは外さない。外すのは利用者)。
    pub(crate) fn open_recent(&mut self, p: &Path) {
        if !p.exists() {
            self.home.recent_missing.set.insert(p.to_path_buf());
            self.win.message = Some(format!(
                "{} が見つかりません(動かしたか消した)。右クリックで一覧から外せます",
                p.display()
            ));
            return;
        }
        self.open_in_tab(p);
    }

    /// 文書の名前(画面と確認に出す)。
    pub fn document_name(&self) -> String {
        match (
            self.cur.work.path().and_then(|p| p.file_name()),
            &self.cur.doc_label,
        ) {
            (Some(n), _) => n.to_string_lossy().to_string(),
            (None, Some(l)) => l.clone(),
            (None, None) => "無題".to_string(),
        }
    }

    /// 文書を入れ替える(画面の状態は初めに戻す)。
    /// 前の文書を返す。
    pub(crate) fn replace_document(&mut self, doc: Document, path: Option<PathBuf>) -> Document {
        // 開いている途中だった文書には戻らない(開くのは load_path が見張り直す)
        self.cur.opening = None;
        let old = self.cur.work.replace(doc, path);
        self.cur.doc_label = None;
        self.cur.selected = None;
        // 写した加工は前の文書の機能を指すので、別の文書には貼らない(テンプレートを使う)
        self.cur.op_clip = None;
        self.cur.feature_clip = None;
        self.clear_edits();
        for e in &mut self.cur.editors {
            e.reset();
        }
        self.cur.task = None;
        // 別の文書に替えた: 走っている仕事は止める(結果を受け取る所が無い)
        if let Some(c) = self.cur.task_ctl.take() {
            c.cancel();
        }
        self.cur.picked = None;
        self.cur.hidden_bodies.clear();
        self.cur.hidden_folders.clear();
        self.cur.origin_shown = false;
        self.set_all_hidden(Default::default());
        self.cur.used_sketches.clear();
        self.cur.auto_hidden.clear();
        self.cur.data_edit = None;
        self.cur.modified = false;
        // 寸法の名前をそろえてから覚える(名前を付けただけでは変更にしない)
        self.cur.work.settle_names();
        self.cur.saved = self.cur.work.doc().data().clone();
        self.home.start = false;
        if let Some(a) = &mut self.cur.autosave {
            a.clear();
        }
        // 前の文書の計算はもう要らない。次の結果は失敗していても出す(M27 の前の形は前の文書のもの)
        self.supersede_job();
        self.cur.scene_fresh = true;
        self.cur.held.clear();
        self.recompute();
        self.cur.selected = self.last_of_space();
        old
    }

    /// 未保存の変更があれば確かめる。続けてよければ真。
    pub fn confirm_discard(&mut self) -> bool {
        if !self.cur.modified {
            return true;
        }
        match self.win.dialogs.unsaved(&self.document_name()) {
            Unsaved::Save => {
                self.save();
                !self.cur.modified
            }
            Unsaved::Discard => {
                if let Some(a) = &mut self.cur.autosave {
                    a.clear();
                }
                true
            }
            Unsaved::Cancel => false,
        }
    }

    /// 新しい文書を別のタブで開く(今のタブが空ならそこを使う。候補 20)。
    pub fn new_document(&mut self) {
        if !self.make_room() {
            self.replace_document(self.blank_document(), None);
        }
        self.win.message = Some("新しい文書を作りました".into());
    }

    /// ダイアログで選んで開く(別のタブに開き、前の文書は残る)。
    pub fn open_document(&mut self) {
        if let Some(p) = self.win.dialogs.open_document() {
            self.open_in_tab(&p);
        }
    }

    /// ファイルを開く(未保存の確認はしない)。形の計算は裏で行い、終わるまで前の文書を持っておく
    /// (計算の間は進み具合と「キャンセル」を出す。U8)。
    pub fn load_path(&mut self, path: &Path) -> bool {
        match kmb_session::load(path) {
            Ok(doc) => {
                let before = self.cur.work.path().map(std::path::Path::to_path_buf);
                let (modified, saved) = (self.cur.modified, Box::new(self.cur.saved.clone()));
                let report = Box::new(self.cur.work.report().clone());
                let old = self.replace_document(doc, Some(path.to_path_buf()));
                let back = Back::Document {
                    doc: Box::new(old),
                    path: before,
                    modified,
                    saved,
                    report,
                };
                self.watch_opening(path, back);
                self.remember(path);
                self.win.message = Some(format!("{} を開きました", path.display()));
                true
            }
            Err(e) => {
                self.win.fail(e.to_string());
                false
            }
        }
    }

    /// 計算が裏で走っていれば、開いている途中として前の状態を持っておく。
    pub(crate) fn watch_opening(&mut self, path: &Path, back: Back) {
        self.cur.opening = self.is_busy().then(|| Opening {
            name: path.file_name().map_or(path.display().to_string(), |n| {
                n.to_string_lossy().to_string()
            }),
            since: std::time::Instant::now(),
            back,
        });
    }

    /// 開いている途中か(計算が終わるまで)。
    pub fn is_opening(&self) -> bool {
        self.cur.opening.is_some()
    }

    /// 開くのをやめて、前の文書(読み込みなら読み込む前)に戻す(U8)。
    /// 1 つの機能の計算の途中ではやめられないので、その機能が終わってから前の形に戻る。
    pub fn cancel_opening(&mut self) -> bool {
        let Some(o) = self.cur.opening.take() else {
            return false;
        };
        log::info!("{} を開くのをやめる", o.name);
        match o.back {
            Back::Document {
                doc,
                path,
                modified,
                saved,
                report,
            } => {
                // 走っている計算は開いた文書のもの。結果もキャッシュも前の文書には使わない
                self.cur.work.put_back(*doc, path, *report);
                self.cur.modified = modified;
                self.cur.saved = *saved;
                self.table_stale();
                self.cur.selected = None;
                self.cur.also.clear();
                self.clear_edits();
                for e in &mut self.cur.editors {
                    e.reset();
                }
                self.cur.data_edit = None;
                // 出している形は前の文書のもの(開いた文書の形はまだ出していない)
                self.cur.scene_fresh = false;
                self.cur.held.clear();
                self.recompute();
                self.win.message = Some(format!("{} を開くのをやめ、前の文書に戻しました", o.name));
                // 新しいタブに開いていたなら、空のタブは閉じて前のタブへ戻る(候補 20)
                if self.tab_count() > 1 && self.is_blank() {
                    let m = self.win.message.take();
                    self.drop_tab();
                    self.win.message = m;
                }
            }
            Back::Undo => {
                self.undo();
                self.win.message = Some(format!("{} の読み込みをやめました", o.name));
            }
        }
        true
    }

    /// 開いている間の窓(進み具合・経った秒数・やめる)。計算が終わったら閉じる。
    pub(crate) fn opening_window(&mut self, ctx: &egui::Context) {
        let Some(o) = &self.cur.opening else {
            return;
        };
        if !self.is_busy() {
            log::info!(
                "{} を開き終えた({:.1} 秒)",
                o.name,
                o.since.elapsed().as_secs_f64()
            );
            self.cur.opening = None;
            return;
        }
        if o.since.elapsed() < OPENING_DELAY {
            ctx.request_repaint_after(OPENING_DELAY);
            return;
        }
        let (done, total) = self.progress().unwrap_or((0, 0));
        let title = match o.back {
            Back::Document { .. } => format!("{} を開いています", o.name),
            Back::Undo => format!("{} を読み込んでいます", o.name),
        };
        let secs = o.since.elapsed().as_secs();
        let mut cancel = false;
        egui::Window::new("開いています")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_width(320.0);
                ui.label(egui::RichText::new(title).strong());
                let frac = if total > 0 {
                    done as f32 / total as f32
                } else {
                    0.0
                };
                ui.add(
                    egui::ProgressBar::new(frac)
                        .text(format!("機能 {done} / {total} 個 ・ {secs} 秒")),
                );
                ui.weak("終わるまで、前の形を出しています。やめると前の文書に戻ります");
                if ui.button("キャンセル").clicked() {
                    cancel = true;
                }
            });
        if cancel {
            self.cancel_opening();
        }
    }

    /// 見本を開く(無題の文書として)。
    pub fn open_sample(&mut self, i: usize) {
        if let Some((name, f)) = self.home.samples.get(i).cloned() {
            self.make_room();
            self.replace_document(f(), None);
            self.cur.doc_label = Some(name.to_string());
            self.win.message = Some(format!(
                "見本「{name}」を開きました(保存するには名前を付けてください)"
            ));
        }
    }

    /// 自動保存の写しから戻す。元のパスがあればそこに保存する文書として開き、未保存の印を付ける。
    pub fn recover(&mut self, i: usize) {
        let Some(r) = self.home.recoveries.get(i).cloned() else {
            return;
        };
        match kmb_session::restore(&r) {
            Ok(doc) => {
                self.make_room();
                self.replace_document(doc, r.original.clone());
                self.cur.modified = true;
                kmb_session::discard(&r);
                self.home.recoveries.remove(i);
                self.win.message =
                    Some("自動保存の写しから戻しました。確かめてから保存してください".into());
            }
            Err(e) => self.win.fail(format!("写しを読めません: {e}")),
        }
    }

    pub fn discard_recovery(&mut self, i: usize) {
        if i < self.home.recoveries.len() {
            kmb_session::discard(&self.home.recoveries.remove(i));
        }
    }

    fn remember(&mut self, path: &Path) {
        self.home.recent.push(path);
        self.check_recent();
        if let Some(d) = &self.win.data_dir
            && let Err(e) = self.home.recent.save(d)
        {
            self.win.fail(e);
        }
    }

    /// 保存する。保存先がなければ名前を付けて保存する。版の履歴に縮小図つきで残る。
    pub fn save(&mut self) {
        self.save_noted("");
    }

    /// 説明 `note` を付けて保存する(版の履歴の説明。空なら「保存」)。
    pub(crate) fn save_noted(&mut self, note: &str) {
        let Some(path) = self.cur.work.path().map(std::path::Path::to_path_buf) else {
            self.save_as();
            return;
        };
        let snap = self.snapshot();
        match kmb_session::save_with(&path, self.cur.work.doc(), &snap, note) {
            Ok(()) => {
                self.cur.modified = false;
                self.cur.saved = self.cur.work.doc().data().clone();
                if let Some(a) = &mut self.cur.autosave {
                    a.clear();
                }
                self.remember(&path);
                self.cur.versions.list = None;
                self.win.message = Some(format!("{} に保存しました", path.display()));
            }
            Err(e) => self.win.fail(format!("保存できません: {e}")),
        }
    }

    /// 保存先を選んで保存する。
    pub fn save_as(&mut self) {
        let ext = kmb_core::DOCUMENT_EXTENSION;
        let name = self.document_name();
        // 古い拡張子の文書も、今の拡張子で保存する
        let stem = kmb_core::DOCUMENT_EXTENSIONS
            .iter()
            .find_map(|e| name.strip_suffix(e).and_then(|s| s.strip_suffix('.')))
            .unwrap_or(&name);
        let name = format!("{stem}.{ext}");
        let Some(mut p) = self.win.dialogs.save_document(&name) else {
            return;
        };
        if p.extension().is_none() {
            p.set_extension(ext);
        }
        self.cur.work.set_path(Some(p));
        self.save();
    }

    /// 自動保存の時なら写しを書く。毎フレーム呼ぶ。
    /// 間隔は設定の「自動保存」(0 分ならしない)。裏のタブも書く(手前のタブ
    /// だけを書くと、変更してから別のタブへ移った後に落ちた時、その変更が消える)。
    pub(crate) fn autosave_tick(&mut self, now: SystemTime) {
        let minutes = self.win.layout.autosave_minutes;
        if minutes == 0 {
            return;
        }
        let interval = std::time::Duration::from_secs(u64::from(minutes) * 60);
        let mut failed = self.cur.autosave_own(now, interval).err();
        for tab in &mut self.win.doc_tabs {
            if let Some(w) = &mut tab.parked
                && let Err(e) = w.autosave_own(now, interval)
            {
                failed.get_or_insert(e);
            }
        }
        if let Some(e) = failed {
            self.win.fail(format!("自動保存できません: {e}"));
        }
    }

    /// 開始画面の下の帯。
    pub(crate) fn start_status(&mut self, ui: &mut egui::Ui) {
        let l = crate::theme::look(ui.ctx());
        ui.horizontal_centered(|ui| {
            let t = self.win.message.clone().unwrap_or_else(|| {
                "ファイルはこの PC に保存します。ネット接続は要りません。".into()
            });
            let c = if self.win.message.is_some() {
                l.pal.warn
            } else {
                l.pal.ink2
            };
            ui.label(egui::RichText::new(t).size(crate::theme::fs(11.5)).color(c));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(concat!("kieMonoBench ", env!("CARGO_PKG_VERSION")))
                        .size(crate::theme::fs(11.5))
                        .color(l.pal.ink2),
                );
            });
        });
    }

    /// 「ファイル」のメニュー。
    pub(crate) fn file_menu(&mut self, ui: &mut egui::Ui) {
        let r = crate::theme::menu_trigger(ui, "file", "ファイル");
        egui::Popup::menu(&r).show(|ui| {
            if ui.button("新規デザイン   Ctrl+N").clicked() {
                self.new_document();
                ui.close();
            }
            if ui.button("開く…   Ctrl+O").clicked() {
                self.open_document();
                ui.close();
            }
            let recent = self.recent_entries();
            ui.add_enabled_ui(!recent.is_empty(), |ui| {
                ui.menu_button("最近使ったデザイン", |ui| {
                    let mut act = None;
                    for (p, missing) in recent {
                        let pin = if self.home.recent.is_pinned(&p) {
                            "[ピン留め] "
                        } else {
                            ""
                        };
                        let text = if missing {
                            format!("{pin}{}(見つかりません)", p.display())
                        } else {
                            format!("{pin}{}", p.display())
                        };
                        let r = ui.add_enabled(!missing, egui::Button::new(text));
                        if r.clicked() {
                            self.open_recent(&p);
                            ui.close();
                        }
                        if missing {
                            // 見つからないものは押せないので、横に外すボタンを出す
                            r.on_disabled_hover_text(
                                "見つかりません。ホームで右クリックすると一覧から外せます",
                            );
                        }
                    }
                    ui.separator();
                    if ui
                        .button("一覧を消す")
                        .on_hover_text(
                            "ピン留めしていないデザインを一覧から外す(ファイルは消さない)",
                        )
                        .clicked()
                    {
                        act = Some(RecentAction::ClearUnpinned);
                        ui.close();
                    }
                    if let Some(a) = act {
                        self.recent_action(Path::new(""), a);
                    }
                });
            });
            ui.separator();
            if ui.button("保存   Ctrl+S").clicked() {
                self.save();
                ui.close();
            }
            if ui.button("名前を付けて保存…   Ctrl+Shift+S").clicked() {
                self.save_as();
                ui.close();
            }
            if ui
                .add_enabled(
                    self.cur.work.path().is_some(),
                    egui::Button::new("版の履歴…"),
                )
                .on_hover_text(
                    "保存のたびに残る版の一覧。前の版を開く・最新として戻す・節目の名前を付ける",
                )
                .on_disabled_hover_text("保存すると、そのたびに版が残ります")
                .clicked()
            {
                self.cur.versions.open = true;
                self.cur.versions.list = None;
                ui.close();
            }
            if ui
                .button("設計の属性…")
                .on_hover_text(
                    "部品名・部品番号・改訂・作者など。部品名などは G-code の頭の注に入る",
                )
                .clicked()
            {
                self.open_properties();
                ui.close();
            }
            ui.separator();
            // ファイルを読み書きする操作(STEP の読み込み、STL・STEP の書き出しなど)
            let imports: Vec<_> = self
                .actions()
                .into_iter()
                .filter(|a| {
                    a.is_at(kmb_core::Place::FileMenu) || !matches!(a.input, kmb_core::Input::None)
                })
                .collect();
            for a in &imports {
                if crate::action_button(ui, a).clicked() {
                    self.run_action(&a.id);
                    ui.close();
                }
            }
            if !imports.is_empty() {
                ui.separator();
            }
            if ui.button("ホーム").clicked() {
                self.show_home();
                ui.close();
            }
        });
    }
}
