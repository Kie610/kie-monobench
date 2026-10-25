//! 版の履歴の窓(候補 35)。保存のたびに文書の横の `.jc/history` に残る版(kmb-session の
//! `history`)を、縮小図・日時・説明・節目の名前つきで並べる。版は別のタブに写しとして開くか、
//! 「最新として戻す」で今の文書にして保存する(戻したことも新しい版になる)。
//! 節目の名前(マイルストーン)を付けられ、「節目だけ」で絞れる。
//! 分岐と合流は作らない(docs/避ける形.md)。

use crate::Workspace;
use kmb_session::history::{self, Version};
use std::path::{Path, PathBuf};

/// 版の履歴の窓の状態。
#[derive(Default)]
pub(crate) struct VersionsPage {
    pub open: bool,
    /// 読んだ一覧と、その文書
    pub list: Option<(PathBuf, Vec<Version>)>,
    /// 節目の名前を書いている版と名前
    pub naming: Option<(String, String)>,
    /// 節目だけ出す
    pub milestones_only: bool,
    /// 名前の欄にキーボードを移す(書き始めた時に 1 回)
    pub focus_name: bool,
    /// 見比べている版(候補 37)
    pub compare: Option<crate::compare_ui::Comparison>,
}

/// 縮小図の絵の控え(画面の絵と、読んだ時のファイルの更新時刻)。
#[derive(Default)]
pub(crate) struct Thumbs {
    by_key: std::collections::HashMap<
        PathBuf,
        (Option<std::time::SystemTime>, Option<egui::TextureHandle>),
    >,
    /// 裏で確かめている文書(更新時刻と、変わっていれば縮小図を読む)
    loading: std::collections::HashMap<PathBuf, std::sync::mpsc::Receiver<Loaded>>,
    /// 最後に更新時刻を確かめに行った時
    checked: std::collections::HashMap<PathBuf, std::time::Instant>,
}

/// 裏で文書を確かめた結果。
enum Loaded {
    /// 更新時刻が控えと同じ
    Same,
    /// 変わった(新しい更新時刻と、縮小図の絵の大きさと画素)
    New(Option<std::time::SystemTime>, Option<(u32, u32, Vec<u8>)>),
}

/// 文書の更新時刻を確かめ直す間(開始画面は最近のファイルを毎フレーム描くので、毎回はディスクを
/// 読まない。ネットワークのドライブで画面が止まらないよう、読むのは裏のスレッド)。
const RECHECK: std::time::Duration = std::time::Duration::from_secs(3);

/// 経った時間を短く(「3 分前」)。
pub(crate) fn ago_secs(t: u64) -> String {
    let now = crate::help_ui::now();
    let m = now.saturating_sub(t) / 60;
    match m {
        0 => "たった今".into(),
        1..=59 => format!("{m} 分前"),
        60..=1439 => format!("{} 時間前", m / 60),
        _ => format!("{} 日前", m / 1440),
    }
}

fn texture(ctx: &egui::Context, name: String, png: &[u8]) -> Option<egui::TextureHandle> {
    let (w, h, rgb) = history::decode_thumbnail(png)?;
    let img = egui::ColorImage::from_rgb([w as usize, h as usize], &rgb);
    Some(ctx.load_texture(name, img, egui::TextureOptions::LINEAR))
}

impl Thumbs {
    /// 文書 `doc` のいちばん新しい縮小図(版の履歴から。無いか読んでいる途中なら `None`)。
    /// 更新時刻の確かめと縮小図の読み込みは裏のスレッドで行い、文書のファイルが変わった時だけ
    /// 読み直す(読み終えたら画面を描き直す)。
    pub fn latest(&mut self, ctx: &egui::Context, doc: &Path) -> Option<egui::TextureHandle> {
        let cached = self.by_key.get(doc).cloned();
        if let Some(rx) = self.loading.get(doc) {
            match rx.try_recv() {
                Err(std::sync::mpsc::TryRecvError::Empty) => return cached.and_then(|c| c.1),
                Ok(Loaded::New(mtime, img)) => {
                    let name = format!("thumb:{}", doc.display());
                    let h = img.map(|(w, h, rgb)| {
                        let img = egui::ColorImage::from_rgb([w as usize, h as usize], &rgb);
                        ctx.load_texture(name, img, egui::TextureOptions::LINEAR)
                    });
                    self.by_key.insert(doc.to_path_buf(), (mtime, h.clone()));
                    self.loading.remove(doc);
                    return h;
                }
                Ok(Loaded::Same) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.loading.remove(doc);
                }
            }
        }
        if self.checked.get(doc).is_some_and(|t| t.elapsed() < RECHECK) {
            return cached.and_then(|c| c.1);
        }
        self.checked
            .insert(doc.to_path_buf(), std::time::Instant::now());
        let (tx, rx) = std::sync::mpsc::channel();
        let known = cached.as_ref().map(|c| c.0);
        let (path, wake) = (doc.to_path_buf(), ctx.clone());
        let spawned = std::thread::Builder::new()
            .name("縮小図".into())
            .spawn(move || {
                let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                let r = if known == Some(mtime) {
                    Loaded::Same
                } else {
                    let img = history::latest_thumbnail(&path)
                        .and_then(|(png, _)| history::decode_thumbnail(&png));
                    Loaded::New(mtime, img)
                };
                if tx.send(r).is_ok() {
                    wake.request_repaint();
                }
            });
        if spawned.is_ok() {
            self.loading.insert(doc.to_path_buf(), rx);
        }
        cached.and_then(|c| c.1)
    }

    /// 版 `id` の縮小図(中身が変わらないので 1 回だけ読む)。
    fn version(
        &mut self,
        ctx: &egui::Context,
        doc: &Path,
        id: &str,
    ) -> Option<egui::TextureHandle> {
        let key = history::dir(doc).join(id);
        if let Some((_, h)) = self.by_key.get(&key) {
            return h.clone();
        }
        let h = history::thumbnail(doc, id)
            .and_then(|png| texture(ctx, format!("thumb:{}", key.display()), &png));
        self.by_key.insert(key, (None, h.clone()));
        h
    }
}

/// 縮小図を枠に収まる大きさで真ん中に描く(大きさ 0 の絵は描かない)。ホームとモデル管理の
/// 縮小図も同じ描き方にするので公開する。
pub fn paint_fit(p: &egui::Painter, rect: egui::Rect, tex: &egui::TextureHandle) {
    let size = tex.size_vec2();
    if size.x <= 0.0 || size.y <= 0.0 {
        return;
    }
    let s = (rect.width() / size.x).min(rect.height() / size.y);
    let r = egui::Rect::from_center_size(rect.center(), size * s);
    p.image(
        tex.id(),
        r,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
}

impl Workspace {
    /// 今の文書の縮小図と形の数(保存の時に版に残す。見えているボディを等角から)。
    pub(crate) fn snapshot(&self) -> history::Snapshot {
        let tol = self.win.layout.render.mesh.tolerance();
        let meshes: Vec<_> = self
            .visible_bodies()
            .into_iter()
            .filter_map(|(f, h)| Some((f, self.cur.work.kernel().mesh(h, tol).ok()?)))
            .collect();
        let b: Vec<_> = meshes.iter().map(|(f, m)| (*f, m)).collect();
        history::snapshot_of(&b)
    }

    /// 版の履歴の窓を開く・閉じる。
    pub fn toggle_versions(&mut self) {
        self.cur.versions.open = !self.cur.versions.open;
        self.cur.versions.list = None;
    }

    /// 今の文書の版の一覧(新しい順。保存していなければ空)。
    pub fn versions(&self) -> Vec<Version> {
        self.cur.work.path().map(history::list).unwrap_or_default()
    }

    /// 版 `id` を別のタブに写しとして開く(元のファイルとはつながない。保存すると名前を聞く)。
    pub fn open_version(&mut self, id: &str) -> bool {
        let Some(path) = self.cur.work.path().map(std::path::Path::to_path_buf) else {
            return false;
        };
        let Some(v) = history::list(&path).into_iter().find(|v| v.id == id) else {
            return false;
        };
        match history::read(&path, id) {
            Ok(doc) => {
                let stem = path
                    .file_stem()
                    .map_or(String::new(), |s| s.to_string_lossy().to_string());
                self.make_room();
                self.replace_document(doc, None);
                self.cur.doc_label = Some(format!("{stem}(版 {})", v.number));
                self.win.message = Some(format!(
                    "{stem} の版 {} を別のタブで開きました(元のファイルとは別。保存すると名前を聞きます)",
                    v.number
                ));
                true
            }
            Err(e) => {
                self.win.fail(e);
                false
            }
        }
    }

    /// 版 `id` を最新として戻し、保存する(戻したことも新しい版として残る)。
    /// 未保存の変更があれば先に聞く。
    pub fn restore_version(&mut self, id: &str) -> bool {
        let Some(path) = self.cur.work.path().map(std::path::Path::to_path_buf) else {
            return false;
        };
        let Some(v) = history::list(&path).into_iter().find(|v| v.id == id) else {
            return false;
        };
        if !self.confirm_discard() {
            return false;
        }
        match history::read(&path, id) {
            Ok(doc) => {
                self.replace_document(doc, Some(path.clone()));
                self.wait();
                self.save_noted(&format!("版 {} に戻した", v.number));
                self.win.message = Some(format!(
                    "版 {} を最新として戻しました(戻す前の形も版の履歴に残っています)",
                    v.number
                ));
                self.cur.versions.list = None;
                true
            }
            Err(e) => {
                self.win.fail(e);
                false
            }
        }
    }

    /// 版 `id` に節目の名前を付ける(空なら外す)。
    pub fn name_version(&mut self, id: &str, name: &str) -> bool {
        let Some(path) = self.cur.work.path().map(std::path::Path::to_path_buf) else {
            return false;
        };
        match history::set_milestone(&path, id, name) {
            Ok(()) => {
                self.cur.versions.list = None;
                true
            }
            Err(e) => {
                self.win.fail(e);
                false
            }
        }
    }

    /// 版の履歴の窓。
    pub(crate) fn versions_window(&mut self, ctx: &egui::Context) {
        if !self.cur.versions.open {
            return;
        }
        let path = self.cur.work.path().map(std::path::Path::to_path_buf);
        if self.cur.versions.list.as_ref().map(|(p, _)| p) != path.as_ref()
            && let Some(p) = &path
        {
            self.cur.versions.list = Some((p.clone(), history::list(p)));
        }
        let mut open = true;
        let mut act: Option<(u8, String)> = None;
        let name = self.document_name();
        // 版ごとのレンダリング ギャラリーの絵の数(#17)
        let mut renders: std::collections::HashMap<u32, usize> = Default::default();
        for r in self.gallery() {
            if let Some(n) = r.version_number {
                *renders.entry(n).or_default() += 1;
            }
        }
        egui::Window::new("版の履歴")
            .id(egui::Id::new("versions_window"))
            .open(&mut open)
            .default_size([520.0, 480.0])
            .default_pos([270.0, 150.0])
            .show(ctx, |ui| {
                let Some(p) = path.clone() else {
                    ui.label("まだ保存していない文書です。保存すると、そのたびに版が残ります。");
                    return;
                };
                ui.horizontal(|ui| {
                    ui.strong(&name);
                    ui.weak("保存のたびに、文書の横の .jc フォルダに残ります");
                });
                ui.checkbox(&mut self.cur.versions.milestones_only, "節目だけ")
                    .on_hover_text("名前を付けた版(マイルストーン)だけを出す");
                ui.separator();
                let list = self
                    .cur.versions
                    .list
                    .as_ref()
                    .map(|(_, l)| l.clone())
                    .unwrap_or_default();
                if list.is_empty() {
                    ui.weak("(まだ版がありません。次に保存すると残ります)");
                }
                let latest = list.first().map(|v| (v.number, v.id.clone()));
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for v in &list {
                        if self.cur.versions.milestones_only && v.milestone.is_empty() {
                            continue;
                        }
                        ui.horizontal(|ui| {
                            let (rect, _) = ui.allocate_exact_size(
                                egui::vec2(96.0, 72.0),
                                egui::Sense::hover(),
                            );
                            let l = crate::theme::look(ui.ctx());
                            ui.painter()
                                .rect_filled(rect, crate::theme::corner(4.0), l.pal.canvas);
                            if let Some(t) = self.thumbs.version(ui.ctx(), &p, &v.id) {
                                paint_fit(ui.painter(), rect, &t);
                            }
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    ui.strong(format!("版 {}", v.number));
                                    let same = latest.as_ref().is_some_and(|(_, id)| *id == v.id);
                                    if latest.as_ref().is_some_and(|(n, _)| *n == v.number) {
                                        ui.weak("(今の版)");
                                    } else if same {
                                        ui.weak("(今と同じ中身)");
                                    }
                                    if self.cur.versions.compare.as_ref().is_some_and(|c| c.number == v.number) {
                                        ui.label(
                                            egui::RichText::new("見比べ中")
                                                .color(crate::theme::look(ui.ctx()).ws()),
                                        );
                                    }
                                    if !v.milestone.is_empty() {
                                        ui.label(
                                            egui::RichText::new(format!("◆ {}", v.milestone))
                                                .color(l.ws()),
                                        );
                                    }
                                });
                                ui.label(format!(
                                    "{}・{}・機能 {} 個",
                                    ago_secs(v.saved_at),
                                    v.note,
                                    v.features
                                ))
                                .on_hover_text(format!(
                                    "UTC {}",
                                    kmb_session::logging::utc(
                                        std::time::UNIX_EPOCH
                                            + std::time::Duration::from_secs(v.saved_at)
                                    )
                                    .get(..16)
                                    .unwrap_or_default()
                                    .replace('T', " ")
                                ));
                                ui.horizontal(|ui| {
                                    if ui
                                        .small_button("開く")
                                        .on_hover_text("この版を別のタブに写しとして開く(元のファイルは変えない)")
                                        .clicked()
                                    {
                                        act = Some((0, v.id.clone()));
                                    }
                                    // 今と同じ中身なら戻しても変わらない
                                    let is_latest = latest.as_ref().is_some_and(|(_, id)| *id == v.id);
                                    if ui
                                        .add_enabled(!is_latest, egui::Button::new("最新として戻す").small())
                                        .on_hover_text("この版を今の文書にして保存する。戻す前の形も版として残る")
                                        .clicked()
                                    {
                                        act = Some((1, v.id.clone()));
                                    }
                                    if ui
                                        .small_button("今と比べる")
                                        .on_hover_text("この版と今の文書の違いを表と重ねた形で見る")
                                        .clicked()
                                    {
                                        act = Some((4, v.id.clone()));
                                    }
                                    if let Some(n) = renders.get(&v.number)
                                        && ui
                                            .small_button(format!("絵 {n} 枚"))
                                            .on_hover_text("この版から描いたレンダリングの絵を、レンダリング ギャラリーに出す")
                                            .clicked()
                                    {
                                        act = Some((5, v.number.to_string()));
                                    }
                                    let naming = self
                                        .cur.versions
                                        .naming
                                        .as_ref()
                                        .is_some_and(|(id, _)| *id == v.id);
                                    if !naming
                                        && ui
                                            .small_button(if v.milestone.is_empty() {
                                                "節目にする…"
                                            } else {
                                                "名前を変える…"
                                            })
                                            .on_hover_text("この版に名前を付けて節目(マイルストーン)にする")
                                            .clicked()
                                    {
                                        self.cur.versions.naming =
                                            Some((v.id.clone(), v.milestone.clone()));
                                        self.cur.versions.focus_name = true;
                                    }
                                });
                                if let Some((id, text)) = &mut self.cur.versions.naming
                                    && *id == v.id
                                {
                                    ui.horizontal(|ui| {
                                        let r = ui.add(
                                            egui::TextEdit::singleline(text)
                                                .hint_text("例: 初めて削れた形")
                                                .desired_width(200.0),
                                        );
                                        if std::mem::take(&mut self.cur.versions.focus_name) {
                                            r.request_focus();
                                        }
                                        let enter = r.lost_focus()
                                            && ui.input(|i| i.key_pressed(egui::Key::Enter));
                                        if ui.small_button("OK").clicked() || enter {
                                            act = Some((2, id.clone()));
                                        }
                                        if ui.small_button("キャンセル").clicked() {
                                            act = Some((3, String::new()));
                                        }
                                    });
                                }
                            });
                        });
                        ui.separator();
                    }
                });
            });
        if !open {
            self.cur.versions.open = false;
            self.cur.versions.naming = None;
        }
        match act {
            Some((0, id)) => {
                self.open_version(&id);
            }
            Some((1, id)) => {
                self.restore_version(&id);
            }
            Some((2, id)) => {
                let name = self
                    .cur
                    .versions
                    .naming
                    .take()
                    .map(|(_, n)| n)
                    .unwrap_or_default();
                self.name_version(&id, &name);
            }
            Some((3, _)) => self.cur.versions.naming = None,
            Some((4, id)) => {
                self.compare_version(&id);
            }
            Some((5, number)) => self.show_gallery_of_version(number.parse().ok()),
            _ => {}
        }
    }
}
