//! 右: 選んだ物の概要。デザイン 1 つ(分類をその場で直す・名前を変える・版・大きさ)、
//! フォルダ(中の数・大きさ・最近変えたデザイン・ジャンルの内訳・よく付いているタグ)、
//! 複数(数・場所・大きさの合計・共通の分類・一緒に動く物)、何も選んでいない時は今の場所。

use crate::{
    InfoDraft, Item, Library, Place, file_name, heading, key_button, paint_fit, plate, rows,
};
use jc_session::library::{bytes_label, date_label, folder_summary};
use jc_session::projects::ModelInfo;
use jc_ui::theme;
use jc_ui::{HomeHost, HomeRequest};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

impl Library {
    /// 右: 選んだ物の概要か、今の場所の概要。
    pub(crate) fn right(&mut self, ui: &mut egui::Ui, host: &mut HomeHost<'_>) {
        ui.add_space(16.0);
        if let (Place::Tidy(crate::Tidy::Similar), Some(k), []) =
            (&self.place, self.group, self.selected.as_slice())
        {
            self.group_panel(ui, host, k);
            return;
        }
        match self.selected.clone().as_slice() {
            [] => match self.place.clone() {
                Place::Folder { project, folder } => self.folder_panel(ui, host, &project, &folder),
                _ => self.place_summary(ui, host),
            },
            [p] if p.is_dir() => match Self::project_of(host, p) {
                Some(project) => self.folder_panel(ui, host, &project, p),
                None => self.place_summary(ui, host),
            },
            [p] => self.design_panel(ui, host, p),
            many => self.multi_panel(ui, host, many),
        }
    }

    /// デザイン 1 つの概要。
    fn design_panel(&mut self, ui: &mut egui::Ui, host: &mut HomeHost<'_>, path: &Path) {
        let l = theme::look(ui.ctx());
        let entry = host.recent.iter().find(|r| r.path == path);
        let missing = entry.map_or(!path.exists(), |e| e.missing);
        let item = Item {
            path: path.to_path_buf(),
            folder: None,
            pinned: entry.is_some_and(|e| e.pinned),
            missing,
        };
        let w = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, w * 0.62), egui::Sense::hover());
        ui.painter()
            .rect_filled(rect, theme::corner(6.0), l.pal.canvas);
        match (!missing).then(|| host.thumbnail(ui.ctx(), path)).flatten() {
            Some(t) => paint_fit(ui.painter(), rect.shrink(4.0), &t),
            None => plate(ui.painter(), rect, &l),
        }
        ui.add_space(8.0);
        if !self.rename_ui(ui, host, path) {
            ui.label(
                egui::RichText::new(file_name(path))
                    .size(theme::fs(15.0))
                    .strong()
                    .color(l.pal.ink),
            );
        }
        ui.label(
            egui::RichText::new(
                path.parent()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
            )
            .size(theme::fs(11.0))
            .color(l.pal.ink3),
        );
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if theme::primary(ui, "開く", "", !missing).clicked() {
                host.request(HomeRequest::Open(path.to_path_buf()));
            }
            let more = theme::secondary(ui, "…", "", true).on_hover_text("ほかの操作");
            egui::Popup::menu(&more).show(|ui| self.item_menu(ui, host, &item));
        });
        if missing {
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(
                    "ファイルが見つかりません。動かしたか消した時は、「…」から一覧から外せます。",
                )
                .size(theme::fs(11.5))
                .color(l.pal.warn),
            );
            return;
        }
        let Some(s) = self.summaries.get(ui.ctx(), path).cloned() else {
            ui.label(egui::RichText::new("読んでいます…").color(l.pal.ink3));
            return;
        };
        heading(ui, "分類");
        if host.open.iter().any(|p| p == path) {
            let info = &s.info;
            let tags = info.tags.join("、");
            rows(
                ui,
                &[
                    ("部品名", info.title.as_str()),
                    ("作品", info.project.as_str()),
                    ("ジャンル", info.genre.as_str()),
                    ("タグ", tags.as_str()),
                    ("材料", info.material.as_str()),
                ],
            );
            ui.label(
                egui::RichText::new("開いているので、そのタブの「設計の属性…」で直します。")
                    .size(theme::fs(11.0))
                    .color(l.pal.ink3),
            );
        } else {
            self.info_editor(ui, host, path, &s.info);
        }
        heading(ui, "版");
        if s.versions.is_empty() {
            ui.label(
                egui::RichText::new("まだ版はありません(保存すると残ります)")
                    .size(theme::fs(11.5))
                    .color(l.pal.ink3),
            );
        }
        for v in s.versions.iter().take(3) {
            let when =
                date_label(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(v.saved_at));
            let note = if v.milestone.is_empty() {
                v.note.clone()
            } else {
                format!("節目「{}」", v.milestone)
            };
            ui.label(
                egui::RichText::new(format!("v{}  {when}  {note}", v.number))
                    .size(theme::fs(11.5))
                    .color(l.pal.ink),
            );
        }
        if !s.versions.is_empty()
            && ui
                .add(egui::Button::new(
                    egui::RichText::new("版の履歴をすべて見る").size(theme::fs(11.5)),
                ))
                .clicked()
        {
            host.request(HomeRequest::ShowVersions(path.to_path_buf()));
        }
        heading(ui, "ファイル");
        let size = bytes_label(s.bytes);
        let total = bytes_label(s.bytes + s.history_bytes);
        let created = s.created.map(date_label).unwrap_or_default();
        let modified = s.modified.map(date_label).unwrap_or_default();
        rows(
            ui,
            &[
                ("大きさ", size.as_str()),
                ("版を含めた大きさ", total.as_str()),
                ("作成日", created.as_str()),
                ("変更日", modified.as_str()),
            ],
        );
    }

    /// 名前を変える欄(名前を変えている途中の時だけ。出したら真)。Enter で変え、Esc でやめる。
    fn rename_ui(&mut self, ui: &mut egui::Ui, host: &mut HomeHost<'_>, path: &Path) -> bool {
        let Some((p, name)) = self.renaming.as_mut() else {
            return false;
        };
        if p != path {
            return false;
        }
        let r = ui.add(
            egui::TextEdit::singleline(name)
                .font(theme::prop(15.0))
                .desired_width(f32::INFINITY),
        );
        ui.ctx()
            .accesskit_node_builder(r.id, |b| b.set_label("新しい名前"));
        if self.rename_focus {
            r.request_focus();
            self.rename_focus = false;
        }
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
        let mut ok = enter;
        let mut cancel = esc;
        ui.horizontal(|ui| {
            ok |= key_button(ui, true, "名前変更", "Enter", true).clicked();
            cancel |= key_button(ui, false, "キャンセル", "Esc", true).clicked();
        });
        if ok {
            let name = name.trim().to_string();
            if !name.is_empty() && name != file_name(path) {
                host.request(if path.is_dir() {
                    HomeRequest::RenameFolder {
                        folder: path.to_path_buf(),
                        name,
                    }
                } else {
                    HomeRequest::Rename {
                        design: path.to_path_buf(),
                        name,
                    }
                });
            }
            self.renaming = None;
        } else if cancel {
            self.renaming = None;
        }
        true
    }

    /// 分類の欄(開かずに直す。欄を離れるか Enter で、新しい版「分類を変更」として残す)。
    fn info_editor(
        &mut self,
        ui: &mut egui::Ui,
        host: &mut HomeHost<'_>,
        path: &Path,
        info: &ModelInfo,
    ) {
        let l = theme::look(ui.ctx());
        let fresh = InfoDraft::of(info);
        // 別のデザインか、読み直した分類が変わっていて手を付けていなければ、今の分類から始める
        let untouched_and_changed = |d: &InfoDraft, base: Option<&ModelInfo>| {
            base.is_some_and(|b| b != info && *d == InfoDraft::of(b))
        };
        let reset = match &self.draft {
            Some((p, d)) => p != path || untouched_and_changed(d, self.draft_base.as_ref()),
            None => true,
        };
        if reset {
            self.draft = Some((path.to_path_buf(), fresh.clone()));
            self.draft_base = Some(info.clone());
        }
        let Some((_, d)) = self.draft.as_mut() else {
            return;
        };
        let mut done = false;
        egui::Grid::new("home_info")
            .num_columns(2)
            .spacing(egui::vec2(10.0, 4.0))
            .show(ui, |ui| {
                for (label, v, hint) in [
                    ("部品名", &mut d.title, ""),
                    ("作品", &mut d.project, "関わるデザインをまとめる名前"),
                    ("ジャンル", &mut d.genre, ""),
                    ("タグ", &mut d.tags, "「,」か「、」で区切る"),
                    ("材料", &mut d.material, ""),
                ] {
                    ui.label(
                        egui::RichText::new(label)
                            .size(theme::fs(11.5))
                            .color(l.pal.ink2),
                    );
                    let r = ui.add(
                        egui::TextEdit::singleline(v)
                            .hint_text(hint)
                            .font(theme::prop(11.5))
                            .desired_width(f32::INFINITY),
                    );
                    ui.ctx()
                        .accesskit_node_builder(r.id, |b| b.set_label(label));
                    done |= r.lost_focus();
                    ui.end_row();
                }
            });
        let d = d.clone();
        if done && d != fresh {
            host.request(HomeRequest::SetInfo(path.to_path_buf(), d.apply(info)));
        }
        ui.label(
            egui::RichText::new(
                "欄を離れるか Enter で保存し、新しい版「分類を変更」として残します。",
            )
            .size(theme::fs(10.5))
            .color(l.pal.ink3),
        );
    }

    /// フォルダ(プロジェクトとその中のフォルダ)の概要。
    fn folder_panel(
        &mut self,
        ui: &mut egui::Ui,
        host: &mut HomeHost<'_>,
        project: &Path,
        folder: &Path,
    ) {
        let l = theme::look(ui.ctx());
        if !self.rename_ui(ui, host, folder) {
            ui.label(
                egui::RichText::new(file_name(folder))
                    .size(theme::fs(15.0))
                    .strong()
                    .color(l.pal.ink),
            );
        }
        ui.label(
            egui::RichText::new(folder.display().to_string())
                .size(theme::fs(11.0))
                .color(l.pal.ink3),
        );
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if theme::primary(ui, "ここに新規デザイン", "", true)
                .on_hover_text("このフォルダに「新規デザイン」のファイルを作って開きます")
                .clicked()
            {
                host.request(HomeRequest::NewDesignIn(folder.to_path_buf()));
            }
            if theme::secondary(ui, "エクスプローラーで開く", "", true).clicked() {
                host.request(HomeRequest::ShowFolder(folder.to_path_buf()));
            }
            if folder != project {
                let r = theme::secondary(ui, "…", "", true);
                ui.ctx()
                    .accesskit_node_builder(r.id, |b| b.set_label("フォルダの操作"));
                egui::Popup::menu(&r).show(|ui| self.folder_menu(ui, host, project, folder));
            }
        });
        let Some(t) = self.trees.get(ui.ctx(), project) else {
            ui.label(egui::RichText::new("読んでいます…").color(l.pal.ink3));
            return;
        };
        let Some(f) = t.root.find(folder) else {
            return;
        };
        let s = folder_summary(f, &t.infos);
        heading(
            ui,
            if folder == project {
                "プロジェクト"
            } else {
                "フォルダ"
            },
        );
        let designs = format!("{} 件", s.designs);
        let folders = format!("{} 個", s.folders);
        let size = bytes_label(s.bytes);
        let modified = s.modified.map(date_label).unwrap_or_default();
        rows(
            ui,
            &[
                ("デザイン", designs.as_str()),
                ("フォルダ", folders.as_str()),
                ("大きさ", size.as_str()),
                ("最後の変更", modified.as_str()),
            ],
        );
        if !s.recent.is_empty() {
            heading(ui, "最近変えたデザイン");
            for p in &s.recent {
                if ui
                    .add(
                        egui::Button::new(egui::RichText::new(file_name(p)).size(theme::fs(11.5)))
                            .frame(false),
                    )
                    .on_hover_text("そのフォルダを開いて選ぶ")
                    .clicked()
                    && let Some(parent) = p.parent()
                {
                    self.go(Place::Folder {
                        project: project.to_path_buf(),
                        folder: parent.to_path_buf(),
                    });
                    self.selected = vec![p.clone()];
                    self.anchor = Some(p.clone());
                }
            }
        }
        if !s.genres.is_empty() {
            heading(ui, "ジャンル");
            let lines: Vec<(String, String)> = s
                .genres
                .iter()
                .map(|(g, n)| (g.clone(), format!("{n} 件")))
                .collect();
            let refs: Vec<(&str, &str)> = lines
                .iter()
                .map(|(a, b)| (a.as_str(), b.as_str()))
                .collect();
            rows(ui, &refs);
        }
        if !s.tags.is_empty() {
            heading(ui, "よく付いているタグ");
            ui.horizontal_wrapped(|ui| {
                for (t, n) in &s.tags {
                    let on = self.tags.contains(t);
                    if theme::chip_toggle(ui, on, &format!("#{t} {n}"))
                        .on_hover_text("このタグで絞り込む")
                        .clicked()
                    {
                        if on {
                            self.tags.retain(|x| x != t);
                        } else {
                            self.tags.push(t.clone());
                        }
                    }
                }
            });
        }
    }

    /// 複数を選んでいる時の概要。
    fn multi_panel(&mut self, ui: &mut egui::Ui, host: &mut HomeHost<'_>, paths: &[PathBuf]) {
        let l = theme::look(ui.ctx());
        ui.label(
            egui::RichText::new(format!("{} 件を選択中", paths.len()))
                .size(theme::fs(15.0))
                .strong()
                .color(l.pal.ink),
        );
        ui.add_space(6.0);
        // 縮小図を 6 つまで
        let w = (ui.available_width() - 8.0) / 3.0;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
            for p in paths.iter().take(6) {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(w, w * 0.7), egui::Sense::hover());
                ui.painter()
                    .rect_filled(rect, theme::corner(4.0), l.pal.canvas);
                match host.thumbnail(ui.ctx(), p) {
                    Some(t) => paint_fit(ui.painter(), rect.shrink(2.0), &t),
                    None => plate(ui.painter(), rect, &l),
                }
            }
        });
        let mut places: Vec<String> = paths
            .iter()
            .filter_map(|p| p.parent().map(file_name))
            .collect();
        places.sort();
        places.dedup();
        let sums: Vec<_> = paths
            .iter()
            .filter_map(|p| self.summaries.get(ui.ctx(), p).cloned())
            .collect();
        let bytes: u64 = sums.iter().map(|s| s.bytes).sum();
        let history: u64 = sums.iter().map(|s| s.history_bytes).sum();
        let common = |f: fn(&ModelInfo) -> String| -> String {
            let mut v = sums.iter().map(|s| f(&s.info));
            match v.next() {
                Some(first) if sums.len() == paths.len() && v.all(|x| x == first) => first,
                _ => String::new(),
            }
        };
        heading(ui, "概要");
        let places = places.join("、");
        let size = bytes_label(bytes);
        let total = bytes_label(bytes + history);
        rows(
            ui,
            &[
                ("今の場所", places.as_str()),
                ("大きさの合計", size.as_str()),
                ("版を含めた大きさ", total.as_str()),
            ],
        );
        heading(ui, "全部に共通の分類");
        let project = common(|i| i.project.clone());
        let genre = common(|i| i.genre.clone());
        let tags = common(|i| i.tags.join("、"));
        let material = common(|i| i.material.clone());
        rows(
            ui,
            &[
                ("作品", project.as_str()),
                ("ジャンル", genre.as_str()),
                ("タグ", tags.as_str()),
                ("材料", material.as_str()),
            ],
        );
        heading(ui, "移すと一緒に動く物");
        ui.label(
            egui::RichText::new(
                "版の履歴(.jc/history)が一緒に動きます。元のフォルダには移した記録を残し、リンクの部品はそれをたどって元を探します。",
            )
            .size(theme::fs(11.5))
            .color(l.pal.ink2),
        );
        ui.add_space(8.0);
        let designs: Vec<PathBuf> = paths.iter().filter(|p| !p.is_dir()).cloned().collect();
        ui.horizontal(|ui| {
            let r = theme::secondary(ui, "移す", "", !designs.is_empty());
            egui::Popup::menu(&r).show(|ui| self.move_menu(ui, host, &designs));
            if theme::secondary(ui, "削除…", "", !designs.is_empty()).clicked() {
                self.confirm_trash = Some(designs.clone());
            }
        });
    }

    /// 何も選んでいない時の右: 今の場所の概要。
    fn place_summary(&mut self, ui: &mut egui::Ui, host: &mut HomeHost<'_>) {
        let l = theme::look(ui.ctx());
        ui.label(
            egui::RichText::new(self.place.label())
                .size(theme::fs(15.0))
                .strong()
                .color(l.pal.ink),
        );
        if self.place == Place::Samples {
            ui.label(
                egui::RichText::new(format!("見本 {} 件", host.samples.len()))
                    .size(theme::fs(11.5))
                    .color(l.pal.ink2),
            );
            return;
        }
        let note = match &self.place {
            Place::Tidy(crate::Tidy::Similar) => Some(
                "組を押すと、ここに組の違いが出ます。候補を並べるだけで、自動では移しも消しもしません。"
                    .to_string(),
            ),
            Place::Tidy(crate::Tidy::Unclassified) => Some(
                "ジャンル・作品・タグのどれも付いていないデザインです。カードを選ぶと、ここで分類を直せます。"
                    .to_string(),
            ),
            Place::Tidy(crate::Tidy::Missing) => Some(
                "最近使ったデザインのうち、ファイルが見つからない物です。移した記録から今の場所が分かる物は付け替えられます。"
                    .to_string(),
            ),
            Place::Saved(name) => host
                .filters
                .iter()
                .find(|f| f.name == *name)
                .map(|f| format!("絞り込み: {}\n登録したプロジェクトと最近使ったデザインから出します。左の行の右クリックで削除できます。", f.describe())),
            _ => None,
        };
        if let Some(n) = note {
            ui.label(
                egui::RichText::new(n)
                    .size(theme::fs(11.5))
                    .color(l.pal.ink2),
            );
            return;
        }
        let pinned = host.recent.iter().filter(|r| r.pinned).count();
        let missing = host.recent.iter().filter(|r| r.missing).count();
        let n = if self.place == Place::Pinned {
            pinned
        } else {
            host.recent.len()
        };
        let all = format!("{n} 件");
        let pinned = format!("{pinned} 件");
        let missing = format!("{missing} 件");
        rows(
            ui,
            &[
                ("デザイン", all.as_str()),
                ("ピン留め", pinned.as_str()),
                ("見つからない", missing.as_str()),
            ],
        );
        ui.add_space(10.0);
        ui.label(
            egui::RichText::new("カードを押すと、ここに概要が出ます。二度押すと開きます。")
                .size(theme::fs(11.5))
                .color(l.pal.ink3),
        );
    }
}
