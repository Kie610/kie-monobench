//! データパネル: 設計の画面の左に出し入れする、ホームと同じ部品の細い形(設計.md 3 節「設計の画面の
//! 左のデータパネル」)。
//!
//! 上にパンくず(「すべての場所」› プロジェクト › フォルダ)と探す欄・ホームで開く・閉じる。中は場所の
//! 一覧(最近・ピン留め・プロジェクト)か、フォルダの行とデザインの行。デザインの行を押すとその場で
//! 広がり、版の最近 3 つと「開く」「現在のデザインに挿入」が出る。二度押すと開く。行を 3D のビューへ
//! 引いて離すとリンクで挿入し、フォルダの行へ離すと移す。右クリックはホームと同じ仕分けの操作。
//!
//! 場所・広げた行・名前を変える途中などはホームと別に持つ(ホームを開いても、パネルの場所は変わらない)。
//! デザインの概要とフォルダの木の控えはホームと同じ物を使う。

use crate::middle::drop_target;
use crate::{
    Drag, Filter, Item, Library, Place, file_name, key_button, paint_fit, place_row, plate,
};
use jc_session::library::{Period, Summary, ago_label, date_label};
use jc_ui::theme;
use jc_ui::{HomeHost, HomeRequest, RecentAction};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// データパネルの状態。
#[derive(Default)]
pub(crate) struct Panel {
    /// 開いている場所(`None` は場所の一覧)
    place: Option<Place>,
    /// 探す欄の文字
    query: String,
    /// 広げているデザインの行
    open_row: Option<PathBuf>,
    /// 名前を変えている途中(デザインと新しい名前)
    renaming: Option<(PathBuf, String)>,
    rename_focus: bool,
    /// ごみ箱へ移すか確かめている物
    confirm_trash: Option<Vec<PathBuf>>,
    /// 新しいフォルダの名前の欄
    new_folder: String,
}

impl Panel {
    /// 仕分けでファイルが変わった(途中の操作は捨てる。広げた行は消えていれば閉じる)。
    pub(crate) fn changed(&mut self) {
        self.renaming = None;
        self.confirm_trash = None;
        if self.open_row.as_ref().is_some_and(|p| !p.exists()) {
            self.open_row = None;
        }
    }

    fn go(&mut self, place: Option<Place>) {
        self.place = place;
        self.open_row = None;
        self.renaming = None;
        self.confirm_trash = None;
    }
}

impl Library {
    /// データパネルを描く。
    pub(crate) fn panel_ui(&mut self, ui: &mut egui::Ui, host: &mut HomeHost<'_>) {
        if let Some(Place::Folder { project, .. }) = &self.panel.place
            && !host.projects.contains(project)
        {
            self.panel.go(None);
        }
        let l = theme::look(ui.ctx());
        let mut c = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("data_panel")
                .max_rect(ui.max_rect().shrink2(egui::vec2(10.0, 0.0)))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        let ui = &mut c;
        ui.add_space(8.0);
        self.panel_header(ui, host);
        if self.panel.place.is_some() {
            ui.add_space(4.0);
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.panel.query)
                    .hint_text("名前・作品・タグで探す")
                    .desired_width(f32::INFINITY),
            );
            ui.ctx()
                .accesskit_node_builder(r.id, |b| b.set_label("データパネルで探す"));
        }
        ui.add_space(4.0);
        ui.painter().hline(
            ui.max_rect().x_range(),
            ui.cursor().top(),
            egui::Stroke::new(1.0, l.pal.edge),
        );
        ui.add_space(4.0);
        self.panel_bands(ui, host);
        egui::ScrollArea::vertical()
            .id_salt("data_panel_scroll")
            .auto_shrink(false)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                match self.panel.place.clone() {
                    None => self.panel_places(ui, host),
                    Some(place) => self.panel_items(ui, host, &place),
                }
            });
    }

    /// 上の行: パンくずと、ホームで開く・閉じる。
    fn panel_header(&mut self, ui: &mut egui::Ui, host: &mut HomeHost<'_>) {
        let l = theme::look(ui.ctx());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            let mut parts: Vec<(String, Option<Place>)> = vec![("すべての場所".into(), None)];
            match self.panel.place.clone() {
                Some(Place::Folder { project, folder }) => {
                    let mut chain: Vec<PathBuf> = folder
                        .ancestors()
                        .take_while(|p| p.starts_with(&project))
                        .map(Path::to_path_buf)
                        .collect();
                    chain.reverse();
                    for p in chain {
                        parts.push((
                            file_name(&p),
                            Some(Place::Folder {
                                project: project.clone(),
                                folder: p,
                            }),
                        ));
                    }
                }
                Some(p) => parts.push((p.label(), Some(p))),
                None => {}
            }
            let last = parts.len() - 1;
            let right = 2.0 * 30.0 * theme::band() + 8.0;
            ui.scope(|ui| {
                ui.set_max_width((ui.available_width() - right).max(40.0));
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    for (k, (name, place)) in parts.into_iter().enumerate() {
                        if k > 0 {
                            ui.label(egui::RichText::new("›").color(l.pal.ink3));
                        }
                        let text = egui::RichText::new(name).size(theme::fs(12.0));
                        if k == last {
                            ui.label(text.strong().color(l.pal.ink));
                        } else if ui
                            .add(egui::Button::new(text.color(l.pal.ink2)).frame(false))
                            .clicked()
                        {
                            self.panel.go(place);
                        }
                    }
                });
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::icon_button(ui, "x", "データパネルを閉じる", true, false)
                    .on_hover_text("データパネルを閉じる(上の帯の左端のボタンでまた出せます)")
                    .clicked()
                {
                    host.request(HomeRequest::ClosePanel);
                }
                if theme::icon_button(ui, "lib", "ホームで開く", true, false)
                    .on_hover_text("この場所をホームで大きく開く(仕分け・分類・概要)")
                    .clicked()
                {
                    let place = self.panel.place.clone().unwrap_or_default();
                    self.go(place);
                    if let Some(p) = self.panel.open_row.clone() {
                        self.selected = vec![p];
                    }
                    host.request(HomeRequest::ShowHome);
                }
            });
        });
    }

    /// 元に戻す・ごみ箱の確かめの帯。
    fn panel_bands(&mut self, ui: &mut egui::Ui, host: &mut HomeHost<'_>) {
        let l = theme::look(ui.ctx());
        if let Some(what) = host.undo.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new(what)
                        .size(theme::fs(11.0))
                        .color(l.pal.ink2),
                );
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("元に戻す").size(theme::fs(11.0)),
                    ))
                    .clicked()
                {
                    host.request(HomeRequest::Undo);
                }
            });
            ui.add_space(4.0);
        }
        if let Some(paths) = self.panel.confirm_trash.clone() {
            let text = match paths.as_slice() {
                [p] => format!("「{}」を PC のごみ箱へ移しますか(版の履歴も)", file_name(p)),
                v => format!("{} 件を PC のごみ箱へ移しますか(版の履歴も)", v.len()),
            };
            egui::Frame::new()
                .stroke(egui::Stroke::new(1.0, l.pal.warn))
                .corner_radius(theme::corner(5.0))
                .inner_margin(egui::Margin::same(6))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(
                        egui::RichText::new(text)
                            .size(theme::fs(11.5))
                            .color(l.pal.ink),
                    );
                    ui.horizontal(|ui| {
                        if key_button(ui, true, "ごみ箱へ移す", "", true).clicked() {
                            host.request(HomeRequest::Trash(paths.clone()));
                            self.panel.confirm_trash = None;
                        }
                        if key_button(ui, false, "キャンセル", "", true).clicked() {
                            self.panel.confirm_trash = None;
                        }
                    });
                });
            ui.add_space(4.0);
        }
    }

    /// 場所の一覧(最近・ピン留め・プロジェクト)。
    fn panel_places(&mut self, ui: &mut egui::Ui, host: &mut HomeHost<'_>) {
        let l = theme::look(ui.ctx());
        let pinned = host.recent.iter().filter(|r| r.pinned).count();
        for (place, icon, n) in [
            (Place::Recent, "time", host.recent.len()),
            (Place::Pinned, "pin", pinned),
        ] {
            if place_row(ui, icon, &place.label(), Some(n), false).clicked() {
                self.panel.go(Some(place));
            }
        }
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new("プロジェクト")
                .size(theme::fs(11.0))
                .color(l.pal.ink2),
        );
        if host.projects.is_empty() {
            ui.label(
                egui::RichText::new("まだありません(ホームの「プロジェクトを追加」)")
                    .size(theme::fs(11.0))
                    .color(l.pal.ink3),
            );
        }
        for p in host.projects.clone() {
            let n = self.trees.get(ui.ctx(), &p).map(|t| t.root.model_count());
            let r = place_row(ui, "lib", &file_name(&p), n, false);
            if let Some(designs) = drop_target(ui, &r, &p, n.unwrap_or(0)) {
                host.request(HomeRequest::Move {
                    designs,
                    to: p.clone(),
                });
            }
            if r.on_hover_text(p.display().to_string()).clicked() {
                self.panel.go(Some(Place::Folder {
                    project: p.clone(),
                    folder: p,
                }));
            }
        }
    }

    /// 場所の中のフォルダの行とデザインの行。
    fn panel_items(&mut self, ui: &mut egui::Ui, host: &mut HomeHost<'_>, place: &Place) {
        let l = theme::look(ui.ctx());
        let flt = Filter::query(&self.panel.query);
        let (items, _) = self.items_in(ui.ctx(), host, place, &flt, Period::All);
        if let Place::Folder { project, .. } = place
            && self.trees.get(ui.ctx(), project).is_none()
        {
            ui.label(egui::RichText::new("読んでいます…").color(l.pal.ink3));
            return;
        }
        if items.is_empty() {
            ui.label(
                egui::RichText::new(if flt.is_empty() {
                    "デザインはありません。"
                } else {
                    "当てはまるデザインはありません。"
                })
                .size(theme::fs(11.5))
                .color(l.pal.ink3),
            );
        }
        let mut open = None;
        for it in &items {
            match it.folder {
                Some(n) => {
                    let r = place_row(ui, "folder", &file_name(&it.path), Some(n), false);
                    if let Some(designs) = drop_target(ui, &r, &it.path, n) {
                        host.request(HomeRequest::Move {
                            designs,
                            to: it.path.clone(),
                        });
                    }
                    if r.clicked()
                        && let Place::Folder { project, .. } = place
                    {
                        self.panel.go(Some(Place::Folder {
                            project: project.clone(),
                            folder: it.path.clone(),
                        }));
                    }
                }
                None => {
                    let expanded = self.panel.open_row.as_ref() == Some(&it.path);
                    let thumb = (!it.missing)
                        .then(|| host.thumbnail(ui.ctx(), &it.path))
                        .flatten();
                    let s = self.summaries.get(ui.ctx(), &it.path).cloned();
                    let is_open = host.open.contains(&it.path);
                    let r = design_row(ui, it, s.as_ref(), thumb.as_ref(), expanded, is_open);
                    if !it.missing {
                        r.dnd_set_drag_payload(Drag(vec![it.path.clone()]));
                    }
                    if r.double_clicked() && !it.missing {
                        open = Some(it.path.clone());
                    } else if r.clicked() {
                        self.panel.open_row = (!expanded).then(|| it.path.clone());
                    }
                    r.context_menu(|ui| self.panel_menu(ui, host, it, place));
                    if self
                        .panel
                        .renaming
                        .as_ref()
                        .is_some_and(|(p, _)| *p == it.path)
                    {
                        self.panel_rename(ui, host);
                    }
                    if expanded {
                        self.panel_expanded(ui, host, it, s.as_ref());
                    }
                }
            }
        }
        if let Some(p) = open {
            host.request(HomeRequest::Open(p));
        }
        if let Place::Folder { project, folder } = place {
            ui.add_space(8.0);
            if theme::secondary(ui, "ここに新規デザイン", "", true)
                .on_hover_text("このフォルダに「新規デザイン」のファイルを作って開きます")
                .clicked()
            {
                host.request(HomeRequest::NewDesignIn(folder.clone()));
            }
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.panel.new_folder)
                    .hint_text("新しいフォルダの名前")
                    .desired_width(f32::INFINITY),
            );
            ui.ctx()
                .accesskit_node_builder(r.id, |b| b.set_label("新しいフォルダの名前"));
            let r = r.on_hover_text(format!(
                "Enter で「{}」の中に作ります(先頭に / を付けるとプロジェクトのいちばん上に)",
                file_name(folder)
            ));
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let name = self.panel.new_folder.trim().to_string();
                let (parent, name) = match name.strip_prefix('/') {
                    Some(n) => (project.clone(), n.trim().to_string()),
                    None => (folder.clone(), name),
                };
                if !name.is_empty() {
                    host.request(HomeRequest::NewFolder { parent, name });
                    self.panel.new_folder.clear();
                }
            }
        }
    }

    /// 広げた行の中: 版の最近 3 つと、開く・現在のデザインに挿入・版の履歴。
    fn panel_expanded(
        &mut self,
        ui: &mut egui::Ui,
        host: &mut HomeHost<'_>,
        it: &Item,
        s: Option<&Summary>,
    ) {
        let l = theme::look(ui.ctx());
        egui::Frame::new()
            .fill(l.pal.chrome2)
            .corner_radius(theme::corner(4.0))
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                match s {
                    _ if it.missing => {
                        ui.label(
                            egui::RichText::new("ファイルが見つかりません")
                                .size(theme::fs(11.0))
                                .color(l.pal.warn),
                        );
                    }
                    None => {
                        ui.label(
                            egui::RichText::new("読んでいます…")
                                .size(theme::fs(11.0))
                                .color(l.pal.ink3),
                        );
                    }
                    Some(s) if s.versions.is_empty() => {
                        ui.label(
                            egui::RichText::new("まだ版はありません")
                                .size(theme::fs(11.0))
                                .color(l.pal.ink3),
                        );
                    }
                    Some(s) => {
                        for v in s.versions.iter().take(3) {
                            let when = date_label(
                                SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(v.saved_at),
                            );
                            ui.label(
                                egui::RichText::new(format!("v{}  {when}", v.number))
                                    .size(theme::fs(11.0))
                                    .color(l.pal.ink2),
                            );
                        }
                    }
                }
                ui.add_space(4.0);
                let own = host.current.as_ref() == Some(&it.path);
                ui.horizontal_wrapped(|ui| {
                    if key_button(ui, true, "開く", "", !it.missing).clicked() {
                        host.request(HomeRequest::Open(it.path.clone()));
                    }
                    if key_button(ui, false, "現在のデザインに挿入", "", !it.missing && !own)
                        .on_hover_text(
                            "今の文書へリンクで挿入します(3D のビューへ引いて離しても同じ)",
                        )
                        .on_disabled_hover_text(if own {
                            "開いているデザインそのものは挿入できません"
                        } else {
                            "ファイルが見つかりません"
                        })
                        .clicked()
                    {
                        host.request(HomeRequest::Insert(it.path.clone()));
                    }
                    if s.is_some_and(|s| !s.versions.is_empty())
                        && key_button(ui, false, "版の履歴", "", true).clicked()
                    {
                        host.request(HomeRequest::ShowVersions(it.path.clone()));
                    }
                });
            });
        ui.add_space(4.0);
    }

    /// 名前を変える欄(行の下)。Enter で変え、Esc でやめる。
    fn panel_rename(&mut self, ui: &mut egui::Ui, host: &mut HomeHost<'_>) {
        let Some((path, name)) = self.panel.renaming.as_mut() else {
            return;
        };
        let r = ui.add(egui::TextEdit::singleline(name).desired_width(f32::INFINITY));
        ui.ctx()
            .accesskit_node_builder(r.id, |b| b.set_label("新しい名前"));
        if std::mem::take(&mut self.panel.rename_focus) {
            r.request_focus();
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.panel.renaming = None;
        } else if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let (design, name) = (path.clone(), name.trim().to_string());
            if !name.is_empty() && name != file_name(&design) {
                host.request(HomeRequest::Rename { design, name });
            }
            self.panel.renaming = None;
        }
    }

    /// デザインの行の右クリック(ホームのカードと同じ仕分けの操作と、現在のデザインに挿入)。
    fn panel_menu(&mut self, ui: &mut egui::Ui, host: &mut HomeHost<'_>, it: &Item, place: &Place) {
        let ok = !it.missing;
        let own = host.current.as_ref() == Some(&it.path);
        if ui.add_enabled(ok, egui::Button::new("開く")).clicked() {
            host.request(HomeRequest::Open(it.path.clone()));
            ui.close();
        }
        if ui
            .add_enabled(ok && !own, egui::Button::new("現在のデザインに挿入"))
            .on_disabled_hover_text("開いているデザインそのものは挿入できません")
            .clicked()
        {
            host.request(HomeRequest::Insert(it.path.clone()));
            ui.close();
        }
        if ui
            .button(if it.pinned {
                "ピン留めを外す"
            } else {
                "ピン留め"
            })
            .clicked()
        {
            host.request(HomeRequest::Recent(
                it.path.clone(),
                RecentAction::Pin(!it.pinned),
            ));
            ui.close();
        }
        if ui
            .add_enabled(ok, egui::Button::new("フォルダを開く"))
            .clicked()
        {
            host.request(HomeRequest::Recent(
                it.path.clone(),
                RecentAction::ShowInFolder,
            ));
            ui.close();
        }
        ui.separator();
        if ui
            .add_enabled(ok, egui::Button::new("名前を変える…"))
            .clicked()
        {
            self.panel.renaming = Some((it.path.clone(), file_name(&it.path)));
            self.panel.rename_focus = true;
            ui.close();
        }
        ui.add_enabled_ui(ok, |ui| {
            ui.menu_button("移す", |ui| {
                self.move_menu(ui, host, std::slice::from_ref(&it.path))
            });
        });
        if ui
            .add_enabled(ok, egui::Button::new("分類を直す…"))
            .on_hover_text("ホームの右の概要で直します")
            .clicked()
        {
            let home = match Self::project_of(host, &it.path) {
                Some(project) => Place::Folder {
                    project,
                    folder: it.path.parent().map(Path::to_path_buf).unwrap_or_default(),
                },
                None => Place::Recent,
            };
            self.go(home);
            self.selected = vec![it.path.clone()];
            host.request(HomeRequest::ShowHome);
            ui.close();
        }
        if ui
            .add_enabled(ok, egui::Button::new("削除…"))
            .on_hover_text("PC のごみ箱へ移します(版の履歴も)")
            .clicked()
        {
            self.panel.confirm_trash = Some(vec![it.path.clone()]);
            ui.close();
        }
        if matches!(place, Place::Recent | Place::Pinned)
            && ui
                .button("一覧から外す")
                .on_hover_text("ファイルは消さない")
                .clicked()
        {
            host.request(HomeRequest::Recent(it.path.clone(), RecentAction::Remove));
            ui.close();
        }
    }
}

/// デザインの行(小さな縮小図・名前・版と更新・「開いています」)。広げている行は塗る。
fn design_row(
    ui: &mut egui::Ui,
    it: &Item,
    s: Option<&Summary>,
    thumb: Option<&egui::TextureHandle>,
    expanded: bool,
    is_open: bool,
) -> egui::Response {
    let l = theme::look(ui.ctx());
    let h = theme::fs(44.0).max(40.0);
    let w = ui.available_width();
    let (rect, r) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click_and_drag());
    let name = file_name(&it.path);
    r.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, expanded, &name)
    });
    let p = ui.painter();
    if expanded {
        p.rect_filled(rect, theme::corner(4.0), l.ws_soft());
    } else if r.hovered() {
        p.rect_filled(rect, theme::corner(4.0), l.pal.edge.gamma_multiply(0.35));
    }
    let side = h - 8.0;
    let pic = egui::Rect::from_min_size(rect.min + egui::vec2(4.0, 4.0), egui::Vec2::splat(side));
    p.rect_filled(pic, theme::corner(3.0), l.pal.canvas);
    match thumb {
        Some(t) => paint_fit(p, pic.shrink(1.0), t),
        None => plate(&p.with_clip_rect(pic), pic, &l),
    }
    let x = pic.right() + 8.0;
    let room = (rect.right() - x - 4.0).max(1.0);
    let mut job = egui::text::LayoutJob::simple_singleline(name, theme::prop(12.5), l.pal.ink);
    job.wrap.max_width = room;
    job.wrap.max_rows = 1;
    let g = ui.fonts_mut(|f| f.layout_job(job));
    let p = ui.painter();
    p.galley(egui::pos2(x, rect.top() + 5.0), g, l.pal.ink);
    let line2 = if it.missing {
        "見つかりません".to_string()
    } else {
        let v = s.and_then(Summary::latest).map(|n| format!("v{n}  "));
        let when = s
            .and_then(|s| s.modified)
            .map(|t| ago_label(t, SystemTime::now()));
        format!("{}{}", v.unwrap_or_default(), when.unwrap_or_default())
    };
    let g2 = p.layout_no_wrap(line2, theme::prop(10.5), l.pal.ink2);
    let y2 = rect.bottom() - 5.0 - g2.size().y;
    let w2 = g2.size().x;
    p.galley(
        egui::pos2(x, y2),
        g2,
        if it.missing { l.pal.warn } else { l.pal.ink2 },
    );
    if is_open {
        let g3 = p.layout_no_wrap("開いています".into(), theme::prop(10.0), l.ws());
        let at = egui::pos2(x + w2 + 8.0, y2);
        if at.x + g3.size().x < rect.right() {
            p.galley(at, g3, l.ws());
        }
    }
    if it.pinned {
        let s = theme::icon_px(11.0);
        theme::paint_icon(
            p,
            egui::Rect::from_min_size(
                egui::pos2(rect.right() - s - 6.0, rect.top() + 6.0),
                egui::Vec2::splat(s),
            ),
            "pin",
            l.ws(),
        );
    }
    r.on_hover_text(format!(
        "{}\n押すと広げ、二度押すと開きます。3D のビューへ引いて離すとリンクで挿入、フォルダへ離すと移します",
        it.path.display()
    ))
}
