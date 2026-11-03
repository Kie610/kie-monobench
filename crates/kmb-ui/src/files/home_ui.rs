//! ホームのタブ(開始画面)。上の帯の左端の閉じられないタブで、文書を開いたまま行き来できる。
//!
//! 中身(場所・デザインのカード・概要)は拡張の crate が [`HomePage`] で描く(アプリは
//! kmb-ui-library の `Library` を渡す)。拡張は描くことと自分の状態だけを持ち、文書を開く・
//! 最近の一覧を変えるなどは [`HomeRequest`] で頼む。ここが受けて行う。
//! 拡張を渡されていない時(画面の試験など)は、新規デザインと開くだけの簡単な頁を出す。
//!
//! デザインを移す・名前を変える・ごみ箱へ移す・分類を直すのも頼みで受け、kmb-session の
//! `organize` で行う(CLI・MCP と同じ関数)。行ったことは戻せるように積み、ホームの帯の
//! 「元に戻す」と、ホームを出している間の Ctrl+Z で戻す。開いているタブと最近の一覧のパスも付け替える。
//!
//! 設計の画面の左のデータパネル([`crate::dock_ui::Pane::Data`])も、同じ拡張が細い形で描く
//! ([`HomePage::panel`])。頼みの受け口は同じ。データパネルから 3D のビューへデザイン([`DesignDrag`])を
//! 引いて離すと、リンクで挿入する操作(役割の印 [`kmb_interchange::role::action::INSERT_LINK`])をそのファイルで行う。

use crate::Workspace;
use crate::files_ui::RecentAction;
use kmb_session::organize::{self, Done};
use kmb_session::projects::ModelInfo;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// ホームの中身を描く拡張。
/// `Send` は、作業場を別の窓から描く時に動かすため([`crate::FeatureEditor`] と同じ)。
pub trait HomePage: Send {
    /// ホームを描く。してほしいことは `host` に積む(描き終えてから行う)。
    fn ui(&mut self, ui: &mut egui::Ui, host: &mut HomeHost<'_>);

    /// 設計の画面の左のデータパネル(ホームと同じ部品の細い形)を描く。
    fn panel(&mut self, ui: &mut egui::Ui, host: &mut HomeHost<'_>) {
        let _ = host;
        ui.label("データパネルの中身はありません");
    }

    /// 描画 `pass`(egui の `cumulative_pass_nr`)の時点で、裏のスレッドでファイルを読み直している
    /// 途中か(読み終えると出している一覧が変わる)。画面の台本は、これが終わるまで次の操作と確かめを
    /// 待つ(人も一覧が出直してから次を押す)。
    fn loading(&self, pass: u64) -> bool {
        let _ = pass;
        false
    }
}

/// 引いているデザイン(ホームとデータパネルの中で引く物。フォルダへ離すと移し、3D のビューへ
/// 離すとリンクで挿入する)。
#[derive(Debug, Clone, PartialEq)]
pub struct DesignDrag(pub Vec<PathBuf>);

/// 最近使ったデザイン 1 つ。
#[derive(Debug, Clone, PartialEq)]
pub struct RecentEntry {
    pub path: PathBuf,
    pub pinned: bool,
    /// ファイルが見つからない
    pub missing: bool,
}

/// 自動保存から戻せる文書 1 つ。
#[derive(Debug, Clone, PartialEq)]
pub struct RecoveryEntry {
    /// 元の文書の名前(無題なら「無題」)
    pub name: String,
    pub saved_at: SystemTime,
}

/// ホームから頼むこと。
#[derive(Debug, Clone, PartialEq)]
pub enum HomeRequest {
    /// 新規デザイン
    New,
    /// 開く…(ファイルの窓)
    OpenDialog,
    /// このデザインを開く(開いていればそのタブへ)
    Open(PathBuf),
    /// 見本 `i` を開く
    Sample(usize),
    /// 自動保存の写し `i` を戻す・捨てる
    Recover(usize),
    DiscardRecovery(usize),
    /// 最近の一覧の手入れ(ピン留め・一覧から外す・フォルダを開く・一覧を消す)
    Recent(PathBuf, RecentAction),
    /// プロジェクトを追加(フォルダを選ぶ)
    AddProject,
    /// このデザインを開いて版の履歴を出す
    ShowVersions(PathBuf),
    /// プロジェクトを一覧から外す(フォルダは消さない)
    RemoveProject(PathBuf),
    /// フォルダそのものを OS のファイルの窓で開く
    ShowFolder(PathBuf),
    /// デザインをフォルダへ移す(版の履歴と移した記録も)
    Move {
        designs: Vec<PathBuf>,
        to: PathBuf,
    },
    /// デザインの名前を変える(拡張子を除いた名前)
    Rename {
        design: PathBuf,
        name: String,
    },
    /// デザインを PC のごみ箱へ移す(版の履歴も)
    Trash(Vec<PathBuf>),
    /// フォルダ `parent` の中に新しいフォルダ `name` を作る
    NewFolder {
        parent: PathBuf,
        name: String,
    },
    /// デザインの分類を開かずに直す(新しい版「分類を変更」)
    SetInfo(PathBuf, ModelInfo),
    /// 直前の仕分け(移す・名前・ごみ箱・新しいフォルダ・分類)を戻す
    Undo,
    /// このデザインを今の文書へリンクで挿入する(データパネル)
    Insert(PathBuf),
    /// ホームのタブへ移る(データパネルの「ホームで開く」)
    ShowHome,
    /// フォルダ `.0` に新規デザインを作って開く(「ここに新規デザイン」)
    NewDesignIn(PathBuf),
    /// フォルダそのものをフォルダ `to` の中へ移す(中の物も)
    MoveFolder {
        folder: PathBuf,
        to: PathBuf,
    },
    /// フォルダの名前を変える
    RenameFolder {
        folder: PathBuf,
        name: String,
    },
    /// フォルダを PC のごみ箱へ移す(中の物も)
    TrashFolder(PathBuf),
    /// 絞り込みに名前を付けて保存する(同じ名前は置き換える)
    SaveFilter(kmb_session::filters::Filter),
    /// 保存した絞り込みを消す(名前)
    RemoveFilter(String),
    /// 見つからない最近のデザイン `from` を、移した先 `to` に付け替える
    Relink {
        from: PathBuf,
        to: PathBuf,
    },
    /// データパネルを隠す
    ClosePanel,
}

/// 戻せる仕分け 1 回分。
pub(crate) struct FileUndo {
    /// 帯に出す説明(例: 「2 件を「部品」へ移しました」)
    pub label: String,
    pub done: Vec<Done>,
    /// 分類を直した時の前の分類
    pub info: Option<(PathBuf, ModelInfo)>,
    /// 前の最近の一覧(戻す時にそのまま戻す)
    pub recent: kmb_session::Recent,
}

/// ホームを描く時に拡張へ渡す物(読むための今の値と、頼みの受け口)。
pub struct HomeHost<'a> {
    pub recent: Vec<RecentEntry>,
    pub recoveries: Vec<RecoveryEntry>,
    /// 見本の名前(`HomeRequest::Sample` の番号の順)
    pub samples: Vec<String>,
    /// 登録したプロジェクト(PC のフォルダ)
    pub projects: Vec<PathBuf>,
    /// ホームを出した最初のフレームか(開いている間に変わった物を拡張が読み直す合図)
    pub shown: bool,
    /// 仕分けでファイルが変わるたびに 1 増える(拡張がフォルダを読み直す合図)
    pub changed: u64,
    /// 開いているタブのデザイン(分類はタブで直す・ごみ箱へは移さない)
    pub open: Vec<PathBuf>,
    /// 戻せる直前の仕分けの説明(無ければ `None`)
    pub undo: Option<String>,
    /// 直前に追加したプロジェクト(拡張がそこを開く。1 フレームだけ)
    pub added: Option<PathBuf>,
    /// 今のタブの文書のパス(データパネルで、自分自身を挿入させない。保存していなければ `None`)
    pub current: Option<PathBuf>,
    /// 名前を付けて保存した絞り込み(左に並べる順)
    pub filters: Vec<kmb_session::filters::Filter>,
    thumbs: &'a mut crate::versions_ui::Thumbs,
    requests: Vec<HomeRequest>,
}

impl HomeHost<'_> {
    pub fn request(&mut self, r: HomeRequest) {
        self.requests.push(r);
    }

    /// デザインのいちばん新しい縮小図(版の履歴から。裏で読むので、初めは `None`)。
    pub fn thumbnail(
        &mut self,
        ctx: &egui::Context,
        path: &std::path::Path,
    ) -> Option<egui::TextureHandle> {
        self.thumbs.latest(ctx, path)
    }
}

impl Workspace {
    /// ホームの中身を描く拡張を渡す(アプリは kmb-ui-library の `Library`)。
    pub fn with_home(mut self, page: impl HomePage + 'static) -> Self {
        self.home.page = Some(Box::new(page));
        self
    }

    /// ホームのタブへ移る(文書のタブはそのまま)。
    pub fn show_home(&mut self) {
        if !self.home.start {
            self.home.start = true;
            self.home.discard_recoveries = None;
            self.check_recent();
        }
    }

    /// ホームの頁を描く。
    pub(crate) fn home_ui(&mut self, ui: &mut egui::Ui) {
        let shown = !self.home.drawn;
        self.home.drawn = true;
        let has_recovery_bar = self.recovery_bulk_ui(ui);
        let Some(mut page) = self.home.page.take() else {
            self.plain_home(ui);
            return;
        };
        let mut host = self.home_host(shown);
        if has_recovery_bar {
            // 拡張が最大の矩形を使っても、復旧の帯に重ならないよう残りの領域を渡す。
            let mut content = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt("home_below_recoveries")
                    .max_rect(ui.available_rect_before_wrap()),
            );
            page.ui(&mut content, &mut host);
        } else {
            page.ui(ui, &mut host);
        }
        let requests = std::mem::take(&mut host.requests);
        self.home.page = Some(page);
        for r in requests {
            self.home_request(r);
        }
    }

    /// 復旧の一覧の上の一括操作。確認は帯の中に置き、写しの中身は変更しない。
    fn recovery_bulk_ui(&mut self, ui: &mut egui::Ui) -> bool {
        let files: Vec<_> = self
            .home
            .recoveries
            .iter()
            .map(|r| r.file.clone())
            .collect();
        if self
            .home
            .discard_recoveries
            .as_ref()
            .is_some_and(|old| *old != files)
        {
            self.home.discard_recoveries = None;
        }
        if files.len() < 2 {
            self.home.discard_recoveries = None;
            return false;
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.home.discard_recoveries = None;
        }
        let confirming = self.home.discard_recoveries.is_some();
        let label = self.win.t_with(
            if confirming {
                "common-recovery-confirm"
            } else {
                "common-recovery-count"
            },
            &[("count", files.len().to_string().into())],
        );
        let discard = self.win.t("common-recovery-discard-all");
        let cancel = self.win.t("common-cancel");
        let look = crate::theme::look(ui.ctx());
        let mut pressed = false;
        let mut canceled = false;
        egui::Frame::group(ui.style())
            .fill(look.pal.panel)
            .stroke(egui::Stroke::new(
                1.0,
                if confirming {
                    look.pal.warn
                } else {
                    look.pal.edge
                },
            ))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(label);
                    pressed = ui.button(discard).clicked();
                    if confirming {
                        canceled = ui.button(cancel).clicked();
                    }
                });
            });
        if canceled {
            self.home.discard_recoveries = None;
        } else if pressed {
            if confirming {
                self.home.discard_recoveries = None;
                let mut errors = Vec::new();
                for i in (0..self.home.recoveries.len()).rev() {
                    if let Err(problem) = self.discard_recovery_at(i) {
                        errors.push(problem.message);
                    }
                }
                if !errors.is_empty() {
                    self.win.fail(errors.join("\n"));
                }
            } else {
                self.home.discard_recoveries = Some(files);
            }
        }
        true
    }

    /// データパネルの中身を描く(ホームと同じ拡張の細い形)。
    pub(crate) fn data_panel_ui(&mut self, ui: &mut egui::Ui) {
        let pass = ui.ctx().cumulative_pass_nr();
        let shown = self.home.panel_pass + 1 < pass;
        self.home.panel_pass = pass;
        let Some(mut page) = self.home.page.take() else {
            ui.label("データパネルの中身はありません");
            return;
        };
        let mut host = self.home_host(shown);
        page.panel(ui, &mut host);
        let requests = std::mem::take(&mut host.requests);
        self.home.page = Some(page);
        for r in requests {
            self.home_request(r);
        }
    }

    /// 拡張へ渡す今の値と頼みの受け口。
    fn home_host(&mut self, shown: bool) -> HomeHost<'_> {
        let recent = self
            .recent_entries()
            .into_iter()
            .map(|(path, missing)| RecentEntry {
                pinned: self.home.recent.is_pinned(&path),
                path,
                missing,
            })
            .collect();
        let recoveries = self
            .home
            .recoveries
            .iter()
            .map(|r| RecoveryEntry {
                name: r
                    .original
                    .as_ref()
                    .and_then(|p| p.file_stem())
                    .map_or("無題".to_string(), |n| n.to_string_lossy().to_string()),
                saved_at: r.saved_at,
            })
            .collect();
        HomeHost {
            recent,
            recoveries,
            samples: self.home.samples.iter().map(|s| s.0.clone()).collect(),
            projects: self.home.projects.folders.clone(),
            shown,
            changed: self.home.changed,
            open: self.open_paths(),
            undo: self.home.undo.last().map(|u| u.label.clone()),
            added: self.home.added.take(),
            current: self.cur.work.path().map(Path::to_path_buf),
            filters: self.home.filters.filters.clone(),
            thumbs: &mut self.thumbs,
            requests: Vec::new(),
        }
    }

    /// 3D のビューの上に、データパネルから引いて来たデザインを受ける(離すとリンクで挿入)。
    /// 重なっている間はビューを点線で囲み、何が起きるかを出す。
    pub(crate) fn design_drop(&mut self, ui: &egui::Ui, rect: egui::Rect) {
        let Some(d) = egui::DragAndDrop::payload::<DesignDrag>(ui.ctx()) else {
            return;
        };
        let Some(pos) = ui.ctx().pointer_latest_pos() else {
            return;
        };
        if !rect.contains(pos) {
            return;
        }
        let l = crate::theme::look(ui.ctx());
        let p = ui.painter();
        let r = rect.shrink(3.0);
        p.extend(egui::Shape::dashed_line(
            &[
                r.left_top(),
                r.right_top(),
                r.right_bottom(),
                r.left_bottom(),
                r.left_top(),
            ],
            egui::Stroke::new(2.0, l.ws()),
            8.0,
            5.0,
        ));
        let own =
            d.0.iter()
                .any(|p| self.cur.work.path() == Some(p.as_path()));
        let text = if own {
            "開いているデザインそのものは挿入できません".to_string()
        } else if d.0.len() == 1 {
            format!("離すと「{}」をリンクで挿入します", file_name(&d.0[0]))
        } else {
            format!("離すと {} 件をリンクで挿入します", d.0.len())
        };
        let g = p.layout_no_wrap(text, crate::theme::prop(13.0), l.pal.chrome);
        let at = egui::pos2(r.center().x - g.size().x / 2.0, r.top() + 16.0);
        let back = egui::Rect::from_min_size(at, g.size()).expand2(egui::vec2(12.0, 6.0));
        p.rect_filled(
            back,
            crate::theme::corner(5.0),
            if own { l.pal.warn } else { l.ws() },
        );
        p.galley(at, g, l.pal.chrome);
        if ui.input(|i| i.pointer.any_released())
            && let Some(d) = egui::DragAndDrop::take_payload::<DesignDrag>(ui.ctx())
        {
            for path in &d.0 {
                self.insert_design(path);
            }
        }
    }

    /// デザインを今の文書へリンクで挿入する(リンクで挿入の操作を、そのファイルで行う)。
    fn insert_design(&mut self, path: &Path) {
        let Some(id) = self.action_for_role(&kmb_interchange::role::action::INSERT_LINK) else {
            self.win.fail("リンクで挿入する操作がありません");
            return;
        };
        log::info!("データパネルから挿入: {}", path.display());
        self.run_action_with(&id, Some(path.to_path_buf()));
    }

    /// 拡張の頼みを行う。
    pub(crate) fn home_request(&mut self, r: HomeRequest) {
        log::info!("ホームから: {r:?}");
        match r {
            HomeRequest::New => self.new_document(),
            HomeRequest::OpenDialog => self.open_document(),
            HomeRequest::Open(p) => self.open_recent(&p),
            HomeRequest::Sample(i) => self.open_sample(i),
            HomeRequest::Recover(i) => self.recover(i),
            HomeRequest::DiscardRecovery(i) => self.discard_recovery(i),
            HomeRequest::Recent(p, a) => self.recent_action(&p, a),
            HomeRequest::AddProject => self.add_project(),
            HomeRequest::RemoveProject(p) => {
                self.home.projects.remove(&p);
                self.save_projects();
            }
            HomeRequest::ShowFolder(p) => {
                if let Err(e) = kmb_session::open_folder(&p) {
                    self.win.fail(e);
                }
            }
            HomeRequest::ShowVersions(p) => {
                self.open_recent(&p);
                if !self.home.start {
                    self.cur.versions.open = true;
                }
            }
            HomeRequest::Move { designs, to } => self.move_designs(&designs, &to),
            HomeRequest::Rename { design, name } => {
                let before = self.home.recent.clone();
                match organize::rename_design(&design, &name) {
                    Ok(to) => {
                        self.repath(&design, &to);
                        self.done_files(
                            format!("名前を「{}」に変えました", name.trim()),
                            vec![Done::Move { from: design, to }],
                            before,
                        );
                    }
                    Err(e) => self.win.fail(e),
                }
            }
            HomeRequest::Trash(designs) => self.trash_designs(&designs),
            HomeRequest::NewFolder { parent, name } => {
                let before = self.home.recent.clone();
                match organize::new_folder(&parent, &name) {
                    Ok(path) => self.done_files(
                        format!("フォルダ「{}」を作りました", name.trim()),
                        vec![Done::NewFolder { path }],
                        before,
                    ),
                    Err(e) => self.win.fail(e),
                }
            }
            HomeRequest::SetInfo(p, info) => self.set_design_info(&p, &info),
            HomeRequest::Undo => self.undo_files(),
            HomeRequest::Insert(p) => self.insert_design(&p),
            HomeRequest::ShowHome => self.show_home(),
            HomeRequest::NewDesignIn(dir) => {
                let doc = self.blank_document();
                match organize::new_design(&dir, "新規デザイン", &doc) {
                    Ok(p) => {
                        self.home.changed += 1;
                        self.open_recent(&p);
                        self.win.message = Some(format!(
                            "「{}」を作りました(名前はホームで変えられます)",
                            file_name(&p)
                        ));
                    }
                    Err(e) => self.win.fail(e),
                }
            }
            HomeRequest::MoveFolder { folder, to } => {
                let before = self.home.recent.clone();
                match organize::move_folder(&folder, &to, None) {
                    Ok(t) => {
                        self.repath_folder(&folder, &t);
                        let label = format!(
                            "フォルダ「{}」を「{}」へ移しました",
                            file_name(&folder),
                            file_name(&to)
                        );
                        self.done_files(
                            label,
                            vec![Done::MoveFolder {
                                from: folder,
                                to: t,
                            }],
                            before,
                        );
                    }
                    Err(e) => self.win.fail(e),
                }
            }
            HomeRequest::RenameFolder { folder, name } => {
                let before = self.home.recent.clone();
                match organize::rename_folder(&folder, &name) {
                    Ok(t) => {
                        self.repath_folder(&folder, &t);
                        let label = format!("フォルダの名前を「{}」に変えました", name.trim());
                        self.done_files(
                            label,
                            vec![Done::MoveFolder {
                                from: folder,
                                to: t,
                            }],
                            before,
                        );
                    }
                    Err(e) => self.win.fail(e),
                }
            }
            HomeRequest::TrashFolder(folder) => self.trash_folder(&folder),
            HomeRequest::SaveFilter(f) => {
                let name = f.name.trim().to_string();
                match self.home.filters.put(f) {
                    Ok(()) => {
                        self.save_filters();
                        self.win.message = Some(format!("絞り込み「{name}」を保存しました"));
                    }
                    Err(e) => self.win.fail(e),
                }
            }
            HomeRequest::RemoveFilter(name) => {
                self.home.filters.remove(&name);
                self.save_filters();
            }
            HomeRequest::Relink { from, to } => {
                let before = self.home.recent.clone();
                self.home.recent.moved(&from, &to);
                self.save_recent();
                self.home.changed += 1;
                let label = format!("「{}」を移した先に付け替えました", file_name(&to));
                self.win.message = Some(label.clone());
                self.home.undo.push(FileUndo {
                    label,
                    done: Vec::new(),
                    info: None,
                    recent: before,
                });
            }
            HomeRequest::ClosePanel => self.win.layout.data_open = false,
        }
    }

    /// 開いているタブのデザインのパス。
    fn open_paths(&self) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = self
            .win
            .doc_tabs
            .iter()
            .filter_map(|t| {
                t.parked
                    .as_ref()
                    .and_then(|w| w.work.path().map(Path::to_path_buf))
            })
            .collect();
        if let Some(p) = self.cur.work.path() {
            out.push(p.to_path_buf());
        }
        out
    }

    /// プロジェクトを足す(フォルダを選ぶ)。足せたらホームでそこを開く。
    pub(crate) fn add_project(&mut self) {
        let Some(dir) = self.win.dialogs.pick_folder("プロジェクトにするフォルダ")
        else {
            return;
        };
        match self.home.projects.add(&dir) {
            Ok(()) => {
                self.save_projects();
                self.home.added = self.home.projects.folders.last().cloned();
            }
            Err(e) => self.win.fail(e),
        }
    }

    fn save_projects(&mut self) {
        if let Some(d) = &self.win.data_dir
            && let Err(e) = self.home.projects.save(d)
        {
            self.win.fail(e);
        }
    }

    fn save_filters(&mut self) {
        if let Some(d) = &self.win.data_dir
            && let Err(e) = self.home.filters.save(d)
        {
            self.win.fail(e);
        }
    }

    /// フォルダを移した後に、中の物を開いているタブと最近の一覧のパスを付け替える。
    fn repath_folder(&mut self, from: &Path, to: &Path) {
        let moved = |p: PathBuf| match p.strip_prefix(from) {
            Ok(rest) => to.join(rest),
            Err(_) => p,
        };
        for t in &mut self.win.doc_tabs {
            if let Some(w) = &mut t.parked
                && let Some(p) = w.work.path().map(Path::to_path_buf)
            {
                w.work.set_path(Some(moved(p)));
            }
        }
        if let Some(p) = self.cur.work.path().map(Path::to_path_buf) {
            self.cur.work.set_path(Some(moved(p)));
        }
        self.home.recent.moved_folder(from, to);
    }

    /// フォルダを PC のごみ箱へ移す。中に開いているタブの物があれば移さない。
    fn trash_folder(&mut self, folder: &Path) {
        if let Some(p) = self.open_paths().iter().find(|p| p.starts_with(folder)) {
            self.win.message = Some(format!(
                "「{}」は開いているので、閉じてからフォルダを削除してください",
                file_name(p)
            ));
            return;
        }
        let before = self.home.recent.clone();
        match organize::trash_folder(folder) {
            Ok(()) => {
                let inside: Vec<PathBuf> = before
                    .files
                    .iter()
                    .chain(before.pinned.iter())
                    .filter(|p| p.starts_with(folder))
                    .cloned()
                    .collect();
                for p in inside {
                    self.home.recent.remove(&p);
                }
                let label = format!("フォルダ「{}」をごみ箱へ移しました", file_name(folder));
                self.done_files(
                    label,
                    vec![Done::TrashFolder {
                        path: folder.to_path_buf(),
                    }],
                    before,
                );
            }
            Err(e) => self.win.fail(e),
        }
    }

    /// ファイルを移した後に、開いているタブと最近の一覧のパスを付け替える。
    fn repath(&mut self, from: &Path, to: &Path) {
        for t in &mut self.win.doc_tabs {
            if let Some(w) = &mut t.parked
                && w.work.path() == Some(from)
            {
                w.work.set_path(Some(to.to_path_buf()));
            }
        }
        if self.cur.work.path() == Some(from) {
            self.cur.work.set_path(Some(to.to_path_buf()));
        }
        self.home.recent.moved(from, to);
    }

    /// 仕分けを 1 回行ったことを残し、最近の一覧を書き、拡張に読み直させる。
    fn done_files(&mut self, label: String, done: Vec<Done>, recent: kmb_session::Recent) {
        self.home.changed += 1;
        if done.is_empty() {
            return;
        }
        log::info!("仕分け: {label}");
        self.win.message = Some(label.clone());
        self.home.undo.push(FileUndo {
            label,
            done,
            info: None,
            recent,
        });
        self.save_recent();
    }

    fn save_recent(&mut self) {
        self.check_recent();
        if let Some(d) = &self.win.data_dir
            && let Err(e) = self.home.recent.save(d)
        {
            self.win.fail(e);
        }
    }

    /// デザインをフォルダ `to` へ移す。移せない物は飛ばして理由を出す(移せた物は戻せる)。
    fn move_designs(&mut self, designs: &[PathBuf], to: &Path) {
        let before = self.home.recent.clone();
        let mut done = Vec::new();
        let mut errors = Vec::new();
        for d in designs {
            if d.parent() == Some(to) {
                continue;
            }
            match organize::move_design(d, to, None) {
                Ok(t) => {
                    self.repath(d, &t);
                    done.push(Done::Move {
                        from: d.clone(),
                        to: t,
                    });
                }
                Err(e) => errors.push(e),
            }
        }
        let folder = to.file_name().map_or_else(
            || to.display().to_string(),
            |n| n.to_string_lossy().to_string(),
        );
        let label = match done.as_slice() {
            [Done::Move { from, .. }] => {
                format!("「{}」を「{folder}」へ移しました", file_name(from))
            }
            d => format!("{} 件を「{folder}」へ移しました", d.len()),
        };
        self.done_files(label, done, before);
        if !errors.is_empty() {
            self.win.fail(errors.join("\n"));
        }
    }

    /// デザインを PC のごみ箱へ移す。開いているタブの物は移さない。
    fn trash_designs(&mut self, designs: &[PathBuf]) {
        let before = self.home.recent.clone();
        let open = self.open_paths();
        let mut done = Vec::new();
        let mut errors = Vec::new();
        for d in designs {
            if open.contains(d) {
                errors.push(format!(
                    "「{}」は開いているので、閉じてから削除してください",
                    file_name(d)
                ));
                continue;
            }
            match organize::trash_design(d) {
                Ok(()) => {
                    self.home.recent.remove(d);
                    done.push(Done::Trash { path: d.clone() });
                }
                Err(e) => errors.push(e),
            }
        }
        let label = match done.as_slice() {
            [Done::Trash { path }] => format!("「{}」をごみ箱へ移しました", file_name(path)),
            d => format!("{} 件をごみ箱へ移しました", d.len()),
        };
        self.done_files(label, done, before);
        if !errors.is_empty() {
            self.win.fail(errors.join("\n"));
        }
    }

    /// 分類を開かずに直す。開いているタブの物はそのタブで直す(取り消せる変更として)。
    fn set_design_info(&mut self, path: &Path, info: &ModelInfo) {
        if self.open_paths().iter().any(|p| p == path) {
            self.win.message = Some(format!(
                "「{}」は開いているので、そのタブの「設計の属性…」で直してください",
                file_name(path)
            ));
            return;
        }
        let old = kmb_session::projects::model_info(path);
        match organize::set_info(path, info) {
            Ok(true) => {
                self.home.changed += 1;
                let label = format!("「{}」の分類を変えました", file_name(path));
                self.win.message = Some(label.clone());
                self.home.undo.push(FileUndo {
                    label,
                    done: Vec::new(),
                    info: Some((path.to_path_buf(), old)),
                    recent: self.home.recent.clone(),
                });
            }
            Ok(false) => {}
            Err(e) => self.win.fail(e),
        }
    }

    /// 直前の仕分けを戻す(ホームの帯の「元に戻す」と、ホームでの Ctrl+Z)。
    pub(crate) fn undo_files(&mut self) {
        let Some(u) = self.home.undo.pop() else {
            self.win.fail("戻せる仕分けはありません");
            return;
        };
        let mut errors = Vec::new();
        for d in u.done.iter().rev() {
            if let Err(e) = organize::undo(d) {
                errors.push(e);
            } else if let Done::Move { from, to } = d {
                self.repath(to, from);
            } else if let Done::MoveFolder { from, to } = d {
                self.repath_folder(to, from);
            }
        }
        if let Some((p, info)) = &u.info
            && let Err(e) = organize::set_info(p, info)
        {
            errors.push(e);
        }
        self.home.recent = u.recent;
        self.save_recent();
        self.home.changed += 1;
        log::info!("仕分けを戻した: {}", u.label);
        if errors.is_empty() {
            self.win.message = Some(format!("戻しました({})", u.label));
        } else {
            self.win.fail(errors.join("\n"));
        }
    }

    /// 拡張の無い時のホーム(新規デザインと開くだけ)。
    fn plain_home(&mut self, ui: &mut egui::Ui) {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            if ui.button("新規デザイン").clicked() {
                self.new_document();
            }
            if ui.button("開く…").clicked() {
                self.open_document();
            }
        });
    }

    /// 上の帯の左端のホームのタブ(家の絵。閉じられない)。
    pub(crate) fn home_tab(&mut self, ui: &mut egui::Ui, h: f32) {
        let l = crate::theme::look(ui.ctx());
        let on = self.home.start;
        let (rect, r) = ui.allocate_exact_size(egui::vec2(h + 10.0, h), egui::Sense::click());
        r.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, on, "ホーム")
        });
        let fill = if on {
            l.pal.chrome
        } else if r.hovered() {
            l.pal.edge.gamma_multiply(0.35)
        } else {
            l.pal.edge.gamma_multiply(0.15)
        };
        let p = ui.painter();
        p.rect_filled(rect, crate::theme::corner(5.0), fill);
        if on {
            p.hline(
                rect.x_range().shrink(4.0),
                rect.bottom() - 1.0,
                egui::Stroke::new(2.0, l.ws()),
            );
        }
        paint_house(
            p,
            rect.center(),
            crate::theme::icon_px(15.0),
            if on { l.pal.ink } else { l.pal.ink2 },
        );
        if r.on_hover_text("ホーム(最近使ったデザイン・見本・プロジェクト)")
            .clicked()
        {
            self.show_home();
        }
    }
}

/// 画面に出すファイルの名前: 拡張子を除いた名前(今あるフォルダは拡張子に見える所も含めた名前)。
/// ホームとモデル管理(kmb-ui-library)が同じ出し方にするので公開する。
pub fn file_name(p: &Path) -> String {
    let n = if p.is_dir() {
        p.file_name()
    } else {
        p.file_stem()
    };
    n.map_or_else(
        || p.display().to_string(),
        |n| n.to_string_lossy().to_string(),
    )
}

/// 家の絵(ホームのタブ)。屋根と壁と戸。
fn paint_house(p: &egui::Painter, c: egui::Pos2, s: f32, color: egui::Color32) {
    let k = s / 24.0;
    let at = |x: f32, y: f32| c + egui::vec2((x - 12.0) * k, (y - 12.0) * k);
    let stroke = egui::Stroke::new(1.6, color);
    p.add(egui::Shape::line(
        vec![at(3.0, 11.0), at(12.0, 3.5), at(21.0, 11.0)],
        stroke,
    ));
    p.add(egui::Shape::line(
        vec![at(5.5, 9.5), at(5.5, 20.0), at(18.5, 20.0), at(18.5, 9.5)],
        stroke,
    ));
    p.add(egui::Shape::line(
        vec![
            at(10.0, 20.0),
            at(10.0, 14.0),
            at(14.0, 14.0),
            at(14.0, 20.0),
        ],
        stroke,
    ));
}
