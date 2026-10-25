//! 中央: 見出し(パンくず)・探す欄・絞り込み・帯(元に戻す・自動保存から戻す・選んだ数・ごみ箱の確かめ)・
//! フォルダとデザインのカード。
//!
//! カードは押すと選び(Ctrl で足し引き、Shift で範囲)、二度押すと開く(フォルダは中へ)。デザインの
//! カードは引いて、左の木の行かフォルダのカードへ離すと移す。

use crate::left::flat;
use crate::{
    Drag, Filter, Item, Library, Place, Tidy, file_name, heading, key_button, paint_fit, plate,
    steps,
};
use kmb_session::library::{Period, Summary, ago_label};
use kmb_session::projects::ModelInfo;
use kmb_ui::theme;
use kmb_ui::{HomeHost, HomeRequest, RecentAction};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// 絞り込みの行に出す値(今の場所にある物だけ)。
#[derive(Default)]
pub(crate) struct Values {
    genres: BTreeSet<String>,
    groups: BTreeSet<String>,
    tags: BTreeSet<String>,
}

impl Values {
    fn add(&mut self, i: &ModelInfo) {
        if !i.genre.is_empty() {
            self.genres.insert(i.genre.clone());
        }
        if !i.project.is_empty() {
            self.groups.insert(i.project.clone());
        }
        self.tags.extend(i.tags.iter().cloned());
    }
}

impl Library {
    /// 今の場所に並べる物(探す欄と絞り込みと変更日で絞った物)と、絞り込みの行の値。
    fn items(&mut self, ctx: &egui::Context, host: &HomeHost<'_>) -> (Vec<Item>, Values) {
        let (place, flt, period) = (self.place.clone(), self.filter(), self.period);
        self.items_in(ctx, host, &place, &flt, period)
    }

    /// 場所 `place` に並べる物(`flt` と変更日 `period` で絞った物)と、絞り込みの行の値。
    /// ホームの中央とデータパネルが使う。
    pub(crate) fn items_in(
        &mut self,
        ctx: &egui::Context,
        host: &HomeHost<'_>,
        place: &Place,
        flt: &Filter,
        period: Period,
    ) -> (Vec<Item>, Values) {
        let now = SystemTime::now();
        let filtering = !flt.is_empty();
        let mut values = Values::default();
        let mut out = Vec::new();
        let pinned = |p: &Path| host.recent.iter().any(|r| r.pinned && r.path == p);
        match place.clone() {
            Place::Samples | Place::Tidy(Tidy::Similar) => {}
            Place::Tidy(Tidy::Missing) => {
                for r in host.recent.iter().filter(|r| r.missing) {
                    if !flt.passes(&file_name(&r.path), None) {
                        continue;
                    }
                    out.push(Item {
                        path: r.path.clone(),
                        folder: None,
                        pinned: r.pinned,
                        missing: true,
                    });
                }
            }
            Place::Tidy(Tidy::Unclassified) | Place::Saved(_) => {
                // 全部のプロジェクトから(保存した絞り込みは最近のデザインも)
                let saved = match place {
                    Place::Saved(name) => host.filters.iter().find(|f| f.name == *name).cloned(),
                    _ => None,
                };
                let (entries, _) = self.all_entries(ctx, host);
                let mut seen = std::collections::HashSet::new();
                // パス・名前・分類・変更日・見つからないか
                type Cand = (PathBuf, String, Option<ModelInfo>, Option<SystemTime>, bool);
                let mut cand: Vec<Cand> = entries
                    .into_iter()
                    .map(|e| (e.path, e.name, Some(e.info), e.modified, false))
                    .collect();
                if saved.is_some() {
                    for r in &host.recent {
                        let s = self.summaries.get(ctx, &r.path).cloned();
                        cand.push((
                            r.path.clone(),
                            file_name(&r.path),
                            s.as_ref().map(|s| s.info.clone()),
                            s.and_then(|s| s.modified),
                            r.missing,
                        ));
                    }
                }
                for (path, name, info, modified, missing) in cand {
                    if missing || !seen.insert(path.clone()) {
                        continue;
                    }
                    if let Some(i) = &info {
                        if *place == Place::Tidy(Tidy::Unclassified)
                            && !kmb_session::tidy::unclassified(i)
                        {
                            continue;
                        }
                        values.add(i);
                    }
                    if let Some(f) = &saved
                        && !f.passes_at(&name, info.as_ref(), modified, now)
                    {
                        continue;
                    }
                    if period != Period::All && !period.contains(modified, now) {
                        continue;
                    }
                    if !flt.passes(&name, info.as_ref()) {
                        continue;
                    }
                    out.push(Item {
                        pinned: pinned(&path),
                        path,
                        folder: None,
                        missing: false,
                    });
                }
            }
            Place::Recent | Place::Pinned => {
                for r in &host.recent {
                    if *place == Place::Pinned && !r.pinned {
                        continue;
                    }
                    let s = self.summaries.get(ctx, &r.path).cloned();
                    if let Some(s) = &s {
                        values.add(&s.info);
                    }
                    if period != Period::All
                        && !period.contains(s.as_ref().and_then(|s| s.modified), now)
                    {
                        continue;
                    }
                    if !flt.passes(&file_name(&r.path), s.as_ref().map(|s| &s.info)) {
                        continue;
                    }
                    out.push(Item {
                        path: r.path.clone(),
                        folder: None,
                        pinned: r.pinned,
                        missing: r.missing,
                    });
                }
            }
            Place::Folder { project, folder } => {
                let Some(t) = self.trees.get(ctx, &project) else {
                    return (out, values);
                };
                for m in t.root.all_models() {
                    if let Some(i) = t.infos.get(&m.path) {
                        values.add(i);
                    }
                }
                // 探す・絞る間は、プロジェクト全体から出す
                let (folders, models) = if filtering {
                    (Vec::new(), t.root.all_models())
                } else {
                    match t.root.find(&folder) {
                        Some(f) => (f.folders.iter().collect(), f.models.iter().collect()),
                        None => (Vec::new(), Vec::new()),
                    }
                };
                for f in folders {
                    out.push(Item {
                        path: f.path.clone(),
                        folder: Some(f.model_count()),
                        pinned: false,
                        missing: false,
                    });
                }
                for m in models {
                    if period != Period::All && !period.contains(m.modified, now) {
                        continue;
                    }
                    if !flt.passes(&m.name, t.infos.get(&m.path)) {
                        continue;
                    }
                    out.push(Item {
                        path: m.path.clone(),
                        folder: None,
                        pinned: pinned(&m.path),
                        missing: false,
                    });
                }
            }
        }
        (out, values)
    }

    /// 中央。
    pub(crate) fn middle(&mut self, ui: &mut egui::Ui, host: &mut HomeHost<'_>) {
        let l = theme::look(ui.ctx());
        ui.add_space(16.0);
        if self.place == Place::Samples {
            self.samples_page(ui, host);
            return;
        }
        if self.place == Place::Tidy(Tidy::Similar) {
            self.similar_page(ui, host);
            return;
        }
        if let Place::Saved(name) = &self.place
            && !host.filters.iter().any(|f| f.name == *name)
        {
            self.go(Place::Recent);
        }
        let (items, values) = self.items(ui.ctx(), host);
        // 見えなくなった物は選びから外す
        self.selected.retain(|p| items.iter().any(|i| &i.path == p));
        self.header(
            ui,
            host,
            items.iter().filter(|i| i.folder.is_none()).count(),
        );
        ui.add_space(8.0);
        self.filter_row(ui, &values);
        ui.add_space(8.0);
        self.save_filter_ui(ui, host);
        self.undo_band(ui, host);
        if matches!(self.place, Place::Recent | Place::Pinned) {
            self.recoveries(ui, host);
        }
        if self.place == Place::Tidy(Tidy::Missing) {
            self.missing_band(ui, host);
        }
        self.trash_band(ui, host);
        self.folder_trash_band(ui, host);
        self.selection_band(ui, host);
        if self.place == Place::Recent && host.recent.is_empty() {
            ui.label(
                egui::RichText::new("まだありません。左の「新規デザイン」か見本から始めます。")
                    .color(l.pal.ink3),
            );
            steps(ui);
            return;
        }
        if let Place::Folder { project, .. } = &self.place
            && self.trees.get(ui.ctx(), &project.clone()).is_none()
        {
            ui.label(egui::RichText::new("読んでいます…").color(l.pal.ink3));
            return;
        }
        if items.is_empty() {
            ui.label(
                egui::RichText::new(if self.filtering() || self.period != Period::All {
                    "当てはまるデザインはありません。"
                } else if matches!(self.place, Place::Folder { .. }) {
                    "このフォルダにデザインはありません。左の「新しいフォルダの名前」でフォルダを作るか、デザインを引いて入れます。"
                } else if self.place == Place::Tidy(Tidy::Missing) {
                    "見つからないファイルはありません。"
                } else if self.place == Place::Tidy(Tidy::Unclassified) {
                    "分類のないデザインはありません。"
                } else {
                    "まだありません。"
                })
                .color(l.pal.ink3),
            );
            return;
        }
        if self.list_view {
            self.list_table(ui, host, &items);
        } else {
            self.cards(ui, host, &items);
        }
    }

    /// 見出し(場所の名前かパンくず・件数・一覧を消す・探す欄)。
    fn header(&mut self, ui: &mut egui::Ui, host: &mut HomeHost<'_>, count: usize) {
        let l = theme::look(ui.ctx());
        ui.horizontal(|ui| {
            match self.place.clone() {
                Place::Folder { project, folder } => {
                    // パンくず: プロジェクト › フォルダ › …(押すとそこへ)
                    let mut parts: Vec<PathBuf> = folder
                        .ancestors()
                        .take_while(|p| p.starts_with(&project))
                        .map(Path::to_path_buf)
                        .collect();
                    parts.reverse();
                    let last = parts.len().saturating_sub(1);
                    for (k, p) in parts.into_iter().enumerate() {
                        if k > 0 {
                            ui.label(egui::RichText::new("›").color(l.pal.ink3));
                        }
                        let text = egui::RichText::new(file_name(&p)).size(theme::fs(16.0));
                        if k == last {
                            ui.label(text.strong().color(l.pal.ink));
                        } else if ui
                            .add(egui::Button::new(text.color(l.pal.ink2)).frame(false))
                            .clicked()
                        {
                            self.go(Place::Folder {
                                project: project.clone(),
                                folder: p,
                            });
                        }
                    }
                }
                place => {
                    ui.label(
                        egui::RichText::new(place.label())
                            .size(theme::fs(16.0))
                            .strong()
                            .color(l.pal.ink),
                    );
                }
            }
            ui.label(
                egui::RichText::new(format!("{count} 件"))
                    .size(theme::fs(11.5))
                    .color(l.pal.ink3),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // 格子と一覧の切り替え
                for (list, text, name) in
                    [(true, "一覧", "一覧で表示"), (false, "格子", "格子で表示")]
                {
                    let r = theme::chip_toggle(ui, self.list_view == list, text);
                    ui.ctx().accesskit_node_builder(r.id, |b| b.set_label(name));
                    if r.on_hover_text(name).clicked() {
                        self.list_view = list;
                    }
                }
                ui.add_space(10.0);
                let hint = if matches!(self.place, Place::Folder { .. }) {
                    "プロジェクトの中を名前・作品・タグで探す"
                } else {
                    "名前・作品・タグで探す"
                };
                let r = ui
                    .add(
                        egui::TextEdit::singleline(&mut self.query)
                            .hint_text(hint)
                            .desired_width(220.0),
                    )
                    .on_hover_text("空白で区切ると、全部を含む物だけ出す");
                // 画面の台本・MCP が名前で探せるように
                ui.ctx()
                    .accesskit_node_builder(r.id, |b| b.set_label("名前・作品・タグで探す"));
                ui.add_space(6.0);
                if self.place == Place::Recent
                    && host.recent.iter().any(|r| !r.pinned)
                    && ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("一覧を消す")
                                    .size(theme::fs(11.0))
                                    .color(l.pal.ink2),
                            )
                            .frame(false),
                        )
                        .on_hover_text(
                            "ピン留めしていないデザインを一覧から外す(ファイルは消さない)",
                        )
                        .clicked()
                {
                    host.request(HomeRequest::Recent(
                        PathBuf::new(),
                        RecentAction::ClearUnpinned,
                    ));
                }
            });
        });
    }

    /// 変更日のチップと、ジャンル・作品・タグの絞り込み。
    fn filter_row(&mut self, ui: &mut egui::Ui, v: &Values) {
        let l = theme::look(ui.ctx());
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new("変更日")
                    .size(theme::fs(11.0))
                    .color(l.pal.ink2),
            );
            for p in Period::ALL {
                if theme::chip_toggle(ui, self.period == p, p.label()).clicked() {
                    self.period = p;
                }
            }
            let combo =
                |ui: &mut egui::Ui,
                 id: &str,
                 label: &str,
                 vals: &BTreeSet<String>,
                 cur: &mut Option<String>| {
                    if vals.is_empty() && cur.is_none() {
                        return;
                    }
                    ui.add_space(6.0);
                    egui::ComboBox::from_id_salt(id)
                        .selected_text(cur.clone().map_or_else(
                            || format!("{label}: すべて"),
                            |c| format!("{label}: {c}"),
                        ))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(cur, None, "すべて");
                            for x in vals {
                                ui.selectable_value(cur, Some(x.clone()), x);
                            }
                        });
                };
            combo(ui, "home_genre", "ジャンル", &v.genres, &mut self.genre);
            combo(ui, "home_group", "作品", &v.groups, &mut self.work);
            for t in v
                .tags
                .iter()
                .chain(self.tags.clone().iter().filter(|t| !v.tags.contains(*t)))
            {
                let on = self.tags.contains(t);
                if theme::chip_toggle(ui, on, &format!("#{t}"))
                    .on_hover_text("このタグを持つデザインだけ出す(押し直すと外す)")
                    .clicked()
                {
                    if on {
                        self.tags.retain(|x| x != t);
                    } else {
                        self.tags.push(t.clone());
                    }
                }
            }
            if (self.filtering() || self.period != Period::All)
                && self.filter_name.is_none()
                && ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("絞り込みを保存…")
                                .size(theme::fs(11.0))
                                .color(l.ws()),
                        )
                        .frame(false),
                    )
                    .on_hover_text("今の絞り込みに名前を付けて、左の「保存した絞り込み」に置きます")
                    .clicked()
            {
                self.filter_name = Some(String::new());
                self.filter_focus = true;
            }
            if (self.filtering() || self.period != Period::All)
                && ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("絞り込みを外す")
                                .size(theme::fs(11.0))
                                .color(l.pal.ink2),
                        )
                        .frame(false),
                    )
                    .clicked()
            {
                self.query.clear();
                self.genre = None;
                self.work = None;
                self.tags.clear();
                self.period = Period::All;
            }
        });
    }

    /// 直前の仕分けを戻す帯(閉じるまで出す)。
    pub(crate) fn undo_band(&mut self, ui: &mut egui::Ui, host: &mut HomeHost<'_>) {
        let Some(label) = host.undo.clone() else {
            return;
        };
        if self.undo_closed == Some(host.changed) {
            return;
        }
        let l = theme::look(ui.ctx());
        band(ui, l.ws(), |ui| {
            ui.label(egui::RichText::new(label).color(l.pal.ink));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(egui::Button::new("×").frame(false))
                    .on_hover_text("帯を閉じる(Ctrl+Z ではまだ戻せます)")
                    .clicked()
                {
                    self.undo_closed = Some(host.changed);
                }
                if key_button(ui, false, "元に戻す", "Ctrl+Z", true).clicked() {
                    host.request(HomeRequest::Undo);
                }
            });
        });
    }

    /// ごみ箱へ移すかを確かめる帯(消す物の一覧)。
    pub(crate) fn trash_band(&mut self, ui: &mut egui::Ui, host: &mut HomeHost<'_>) {
        let Some(list) = self.confirm_trash.clone() else {
            return;
        };
        let l = theme::look(ui.ctx());
        band(ui, l.pal.warn, |ui| {
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "次の {} 件を PC のごみ箱へ移します(版の履歴も一緒に)。ごみ箱から戻せます。",
                        list.len()
                    ))
                    .strong()
                    .color(l.pal.ink),
                );
                let names: Vec<String> = list.iter().map(|p| file_name(p)).collect();
                ui.label(
                    egui::RichText::new(names.join("、"))
                        .size(theme::fs(11.5))
                        .color(l.pal.ink2),
                );
                ui.horizontal(|ui| {
                    let go = egui::Button::new(
                        egui::RichText::new("ごみ箱へ移す").color(l.pal.chrome),
                    )
                    .fill(l.pal.warn)
                    .stroke(egui::Stroke::new(1.0, l.pal.warn));
                    if ui.add(go).clicked() {
                        host.request(HomeRequest::Trash(list.clone()));
                        self.confirm_trash = None;
                        self.selected.clear();
                    }
                    if theme::secondary(ui, "キャンセル", "", true).clicked() {
                        self.confirm_trash = None;
                    }
                });
            });
        });
    }

    /// 選んだ数の帯(2 件以上を選んでいる時)。
    fn selection_band(&mut self, ui: &mut egui::Ui, host: &mut HomeHost<'_>) {
        let designs: Vec<PathBuf> = self
            .selected
            .iter()
            .filter(|p| !p.is_dir())
            .cloned()
            .collect();
        if self.selected.len() < 2 {
            return;
        }
        let l = theme::look(ui.ctx());
        band(ui, l.ws(), |ui| {
            ui.label(
                egui::RichText::new(format!("{} 件を選択中", self.selected.len()))
                    .strong()
                    .color(l.pal.ink),
            );
            ui.label(
                egui::RichText::new("左のフォルダへ引いて移します")
                    .size(theme::fs(11.5))
                    .color(l.pal.ink2),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if key_button(ui, false, "選択を解除", "Esc", true).clicked() {
                    self.select_none();
                }
                if theme::secondary(ui, "削除…", "", !designs.is_empty()).clicked() {
                    self.confirm_trash = Some(designs.clone());
                }
                let r = theme::secondary(ui, "移す", "", !designs.is_empty());
                egui::Popup::menu(&r).show(|ui| self.move_menu(ui, host, &designs));
            });
        });
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.select_none();
        }
    }

    /// 「移す」の行き先の一覧(読んであるプロジェクトのフォルダの木)。
    pub(crate) fn move_menu(
        &mut self,
        ui: &mut egui::Ui,
        host: &mut HomeHost<'_>,
        designs: &[PathBuf],
    ) {
        self.destinations(
            ui,
            host,
            |path| designs.iter().all(|d| d.parent() == Some(path)),
            "今ここにあります",
            |to| HomeRequest::Move {
                designs: designs.to_vec(),
                to,
            },
        );
    }

    /// 移し先のフォルダの一覧(プロジェクトごとに中のフォルダを字下げで)。`bad` の所は押せない。
    pub(crate) fn destinations(
        &mut self,
        ui: &mut egui::Ui,
        host: &mut HomeHost<'_>,
        bad: impl Fn(&std::path::Path) -> bool,
        why: &str,
        request: impl Fn(PathBuf) -> HomeRequest,
    ) {
        let l = theme::look(ui.ctx());
        if host.projects.is_empty() {
            ui.label(
                egui::RichText::new("プロジェクトがありません(左の「プロジェクトを追加」)")
                    .color(l.pal.ink3),
            );
            return;
        }
        for p in host.projects.clone() {
            let Some(t) = self.trees.get(ui.ctx(), &p) else {
                ui.label(
                    egui::RichText::new(format!("{}(読んでいます…)", file_name(&p)))
                        .color(l.pal.ink3),
                );
                continue;
            };
            let mut rows = Vec::new();
            flat(&t.root, 0, &mut rows);
            for (path, name, depth) in rows {
                let text = format!("{}{name}", "  ".repeat(depth));
                if ui
                    .add_enabled(!bad(&path), egui::Button::new(text))
                    .on_disabled_hover_text(why)
                    .clicked()
                {
                    host.request(request(path));
                    ui.close();
                }
            }
        }
    }

    /// カードの格子。
    fn cards(&mut self, ui: &mut egui::Ui, host: &mut HomeHost<'_>, items: &[Item]) {
        let cw = 188.0 * theme::band().clamp(1.0, 1.6);
        let gap = 12.0;
        let per_row = ((ui.available_width() + gap) / (cw + gap)).floor().max(1.0) as usize;
        let order: Vec<PathBuf> = items.iter().map(|i| i.path.clone()).collect();
        let in_folder = matches!(self.place, Place::Folder { .. });
        let mut open = None;
        egui::ScrollArea::vertical()
            .id_salt("home_cards")
            .auto_shrink(false)
            .show(ui, |ui| {
                for chunk in items.chunks(per_row) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        for it in chunk {
                            let on = self.selected.contains(&it.path);
                            let r = match it.folder {
                                Some(n) => {
                                    let r = folder_card(ui, it, n, cw, on);
                                    if let Some(designs) = drop_target(ui, &r, &it.path, n) {
                                        host.request(HomeRequest::Move {
                                            designs,
                                            to: it.path.clone(),
                                        });
                                    }
                                    r
                                }
                                None => {
                                    let thumb = (!it.missing)
                                        .then(|| host.thumbnail(ui.ctx(), &it.path))
                                        .flatten();
                                    let s = self.summaries.get(ui.ctx(), &it.path).cloned();
                                    let r =
                                        card(ui, it, s.as_ref(), cw, on, thumb.as_ref(), in_folder);
                                    if !it.missing {
                                        // 選んでいる物をつかめば選んでいる全部、ほかはその 1 つ
                                        let what = if on {
                                            self.selected
                                                .iter()
                                                .filter(|p| !p.is_dir())
                                                .cloned()
                                                .collect()
                                        } else {
                                            vec![it.path.clone()]
                                        };
                                        r.dnd_set_drag_payload(Drag(what));
                                    }
                                    r
                                }
                            };
                            if r.clicked() {
                                let m = ui.input(|i| i.modifiers);
                                self.click(&it.path, it.folder.is_some(), &order, m);
                            }
                            if r.double_clicked() && !it.missing {
                                open = Some(it.clone());
                            }
                            r.context_menu(|ui| self.item_menu(ui, host, it));
                        }
                    });
                    ui.add_space(gap);
                }
            });
        if let Some(it) = open {
            self.open_item(host, it);
        }
    }

    /// 二度押した物を開く(フォルダは中へ、デザインはタブで)。
    pub(crate) fn open_item(&mut self, host: &mut HomeHost<'_>, it: Item) {
        match (it.folder, &self.place) {
            (Some(_), Place::Folder { project, .. }) => {
                let project = project.clone();
                self.go(Place::Folder {
                    project,
                    folder: it.path,
                });
            }
            _ => host.request(HomeRequest::Open(it.path)),
        }
    }

    /// カードを押した時の選び方(Ctrl で足し引き、Shift で起点からの範囲。フォルダは 1 つだけ)。
    pub(crate) fn click(
        &mut self,
        path: &Path,
        folder: bool,
        order: &[PathBuf],
        m: egui::Modifiers,
    ) {
        self.renaming = None;
        let range = self
            .anchor
            .as_ref()
            .and_then(|a| order.iter().position(|p| p == a))
            .zip(order.iter().position(|p| p == path));
        if folder || !(m.command || m.shift) {
            self.selected = vec![path.to_path_buf()];
            self.anchor = Some(path.to_path_buf());
        } else if let (true, Some((a, b))) = (m.shift, range) {
            let (a, b) = (a.min(b), a.max(b));
            self.selected = order[a..=b]
                .iter()
                .filter(|p| !p.is_dir())
                .cloned()
                .collect();
        } else if let Some(k) = self.selected.iter().position(|p| p == path) {
            self.selected.remove(k);
        } else {
            self.selected.retain(|p| !p.is_dir());
            self.selected.push(path.to_path_buf());
            self.anchor = Some(path.to_path_buf());
        }
    }

    /// カードの右クリックと、右の概要の「…」の中身。
    pub(crate) fn item_menu(&mut self, ui: &mut egui::Ui, host: &mut HomeHost<'_>, it: &Item) {
        if it.folder.is_some() {
            if ui.button("開く").clicked() {
                if let Place::Folder { project, .. } = self.place.clone() {
                    self.go(Place::Folder {
                        project,
                        folder: it.path.clone(),
                    });
                }
                ui.close();
            }
            if let Some(project) = Self::project_of(host, &it.path) {
                self.folder_menu(ui, host, &project, &it.path);
            }
            return;
        }
        if it.missing
            && let Some(to) = kmb_session::organize::follow_moves(&it.path)
            && ui
                .button("移した先に付け替える")
                .on_hover_text(format!("最近の一覧のパスを {} に直します", to.display()))
                .clicked()
        {
            host.request(HomeRequest::Relink {
                from: it.path.clone(),
                to,
            });
            ui.close();
        }
        // 選んでいる物の上なら選んでいる全部に、ほかはその 1 つに
        let many: Vec<PathBuf> = if self.selected.contains(&it.path) && self.selected.len() > 1 {
            self.selected
                .iter()
                .filter(|p| !p.is_dir())
                .cloned()
                .collect()
        } else {
            vec![it.path.clone()]
        };
        let ok = !it.missing;
        if ui.add_enabled(ok, egui::Button::new("開く")).clicked() {
            host.request(HomeRequest::Open(it.path.clone()));
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
            .add_enabled(ok && many.len() == 1, egui::Button::new("名前を変える…"))
            .clicked()
        {
            self.selected = vec![it.path.clone()];
            self.renaming = Some((it.path.clone(), file_name(&it.path)));
            self.rename_focus = true;
            ui.close();
        }
        ui.add_enabled_ui(ok, |ui| {
            ui.menu_button(
                if many.len() > 1 {
                    format!("{} 件を移す", many.len())
                } else {
                    "移す".into()
                },
                |ui| self.move_menu(ui, host, &many),
            );
        });
        if ui
            .add_enabled(
                ok,
                egui::Button::new(if many.len() > 1 {
                    format!("{} 件を削除…", many.len())
                } else {
                    "削除…".into()
                }),
            )
            .on_hover_text("PC のごみ箱へ移します(版の履歴も)")
            .clicked()
        {
            self.confirm_trash = Some(many.clone());
            ui.close();
        }
        if matches!(
            self.place,
            Place::Recent | Place::Pinned | Place::Tidy(Tidy::Missing)
        ) && ui
            .button("一覧から外す")
            .on_hover_text("ファイルは消さない")
            .clicked()
        {
            host.request(HomeRequest::Recent(it.path.clone(), RecentAction::Remove));
            ui.close();
        }
    }

    /// 自動保存から戻せる文書(黄色の枠の帯。戻す・捨てる)。
    fn recoveries(&mut self, ui: &mut egui::Ui, host: &mut HomeHost<'_>) {
        let l = theme::look(ui.ctx());
        let mut act = None;
        for (i, r) in host.recoveries.iter().enumerate() {
            band(ui, l.pal.warn, |ui| {
                ui.label(
                    egui::RichText::new(format!("{}(保存していない変更)", r.name))
                        .strong()
                        .color(l.pal.ink),
                );
                ui.label(
                    egui::RichText::new(format!(
                        "{}に自動保存",
                        ago_label(r.saved_at, SystemTime::now())
                    ))
                    .size(theme::fs(11.5))
                    .color(l.pal.ink2),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::secondary(ui, "捨てる", "", true).clicked() {
                        act = Some(HomeRequest::DiscardRecovery(i));
                    }
                    let back = egui::Button::new(egui::RichText::new("戻す").color(l.pal.chrome))
                        .fill(l.pal.warn)
                        .stroke(egui::Stroke::new(1.0, l.pal.warn))
                        .min_size(egui::vec2(58.0, 26.0));
                    if ui.add(back).clicked() {
                        act = Some(HomeRequest::Recover(i));
                    }
                });
            });
        }
        if let Some(a) = act {
            host.request(a);
        }
    }

    /// 見本とはじめての手順の頁。
    fn samples_page(&mut self, ui: &mut egui::Ui, host: &mut HomeHost<'_>) {
        let l = theme::look(ui.ctx());
        ui.label(
            egui::RichText::new(Place::Samples.label())
                .size(theme::fs(16.0))
                .strong()
                .color(l.pal.ink),
        );
        heading(ui, "見本");
        ui.horizontal_wrapped(|ui| {
            for (i, name) in host.samples.clone().iter().enumerate() {
                if crate::big_sample(ui, name).clicked() {
                    host.request(HomeRequest::Sample(i));
                }
            }
        });
        steps(ui);
    }
}

/// 枠の付いた帯(中は横に並べる)。
pub(crate) fn band(ui: &mut egui::Ui, edge: egui::Color32, add: impl FnOnce(&mut egui::Ui)) {
    let l = theme::look(ui.ctx());
    egui::Frame::new()
        .fill(l.pal.panel)
        .stroke(egui::Stroke::new(1.0, edge))
        .corner_radius(theme::corner(6.0))
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(add);
        });
    ui.add_space(6.0);
}

/// デザインを引いて来た時の行き先(左の木の行・フォルダのカード)。重なっている間は点線で囲み、
/// 件数を「5 → 7」と出す。離したら、そこに無いデザインを返す(移すのは呼ぶ側)。
pub(crate) fn drop_target(
    ui: &egui::Ui,
    r: &egui::Response,
    folder: &Path,
    count: usize,
) -> Option<Vec<PathBuf>> {
    let moving = |d: &Drag| -> Vec<PathBuf> {
        d.0.iter()
            .filter(|p| p.parent() != Some(folder))
            .cloned()
            .collect()
    };
    if let Some(d) = r.dnd_hover_payload::<Drag>() {
        let n = moving(&d).len();
        let l = theme::look(ui.ctx());
        let p = ui.painter();
        let rect = r.rect.shrink(1.0);
        let c = [
            rect.left_top(),
            rect.right_top(),
            rect.right_bottom(),
            rect.left_bottom(),
            rect.left_top(),
        ];
        p.extend(egui::Shape::dashed_line(
            &c,
            egui::Stroke::new(1.5, l.ws()),
            5.0,
            3.0,
        ));
        if n > 0 {
            p.text(
                rect.right_top() + egui::vec2(-6.0, 4.0),
                egui::Align2::RIGHT_TOP,
                format!("{count} → {}", count + n),
                theme::prop(11.0),
                l.ws(),
            );
        }
    }
    let d = r.dnd_release_payload::<Drag>()?;
    let v = moving(&d);
    (!v.is_empty()).then_some(v)
}

/// 引いている間、指の横に件数を出す。
pub(crate) fn paint_drag(ctx: &egui::Context) {
    let Some(d) = egui::DragAndDrop::payload::<Drag>(ctx) else {
        return;
    };
    let Some(pos) = ctx.pointer_latest_pos() else {
        return;
    };
    let l = theme::look(ctx);
    let p = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::new("home_drag"),
    ));
    let text = if d.0.len() == 1 {
        file_name(&d.0[0])
    } else {
        format!("{} 件", d.0.len())
    };
    let g = p.layout_no_wrap(text, theme::prop(12.0), l.pal.chrome);
    let rect = egui::Rect::from_min_size(pos + egui::vec2(14.0, 10.0), g.size())
        .expand2(egui::vec2(8.0, 4.0));
    p.rect_filled(rect, theme::corner(4.0), l.ws());
    p.galley(rect.min + egui::vec2(8.0, 4.0), g, l.pal.chrome);
}

/// フォルダのカード(フォルダの絵・名前・中の件数)。
fn folder_card(ui: &mut egui::Ui, it: &Item, n: usize, w: f32, on: bool) -> egui::Response {
    let l = theme::look(ui.ctx());
    let th = 112.0;
    let (rect, r) =
        ui.allocate_exact_size(egui::vec2(w, th + theme::fs(62.0)), egui::Sense::click());
    let name = file_name(&it.path);
    r.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, on, &name)
    });
    let p = ui.painter();
    let (edge, width) = if on {
        (l.ws(), 2.0)
    } else if r.hovered() {
        (theme::mix(l.pal.edge, l.ws(), 0.6), 1.0)
    } else {
        (l.pal.edge, 1.0)
    };
    p.rect(
        rect,
        theme::corner(6.0),
        l.pal.panel,
        egui::Stroke::new(width, edge),
        egui::StrokeKind::Inside,
    );
    let pic = egui::Rect::from_min_size(
        rect.min + egui::vec2(2.0, 2.0),
        egui::vec2(w - 4.0, th - 2.0),
    );
    p.rect_filled(pic, theme::corner(5.0), l.pal.chrome2);
    let s = 46.0;
    theme::paint_icon(
        p,
        egui::Rect::from_center_size(pic.center(), egui::Vec2::splat(s)),
        "folder",
        l.pal.ink2,
    );
    let x = rect.left() + 10.0;
    let mut job = egui::text::LayoutJob::simple_singleline(name, theme::prop(12.5), l.pal.ink);
    job.wrap.max_width = w - 20.0;
    job.wrap.max_rows = 1;
    let g = ui.fonts_mut(|f| f.layout_job(job));
    let p = ui.painter();
    p.galley(egui::pos2(x, pic.bottom() + 6.0), g, l.pal.ink);
    p.text(
        egui::pos2(x, pic.bottom() + theme::fs(32.0)),
        egui::Align2::LEFT_CENTER,
        format!("フォルダ・{n} 件"),
        theme::prop(11.0),
        l.pal.ink2,
    );
    r.on_hover_text(format!(
        "{}\n押すと選び、二度押すと中を開きます。デザインを引いて離すと、ここへ移します",
        it.path.display()
    ))
}

/// デザインのカード(上に縮小図、下に名前・版と変更日・場所)。
/// 選んでいる物は差し色の枠と左上の印、見つからない物は薄く、ピン留めした物には印。
fn card(
    ui: &mut egui::Ui,
    e: &Item,
    s: Option<&Summary>,
    w: f32,
    on: bool,
    thumb: Option<&egui::TextureHandle>,
    in_folder: bool,
) -> egui::Response {
    let l = theme::look(ui.ctx());
    let th = 112.0;
    let (rect, r) = ui.allocate_exact_size(
        egui::vec2(w, th + theme::fs(62.0)),
        egui::Sense::click_and_drag(),
    );
    let name = file_name(&e.path);
    r.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, on, &name)
    });
    let p = ui.painter();
    let (edge, width) = if on {
        (l.ws(), 2.0)
    } else if r.hovered() {
        (theme::mix(l.pal.edge, l.ws(), 0.6), 1.0)
    } else {
        (l.pal.edge, 1.0)
    };
    p.rect(
        rect,
        theme::corner(6.0),
        l.pal.panel,
        egui::Stroke::new(width, edge),
        egui::StrokeKind::Inside,
    );
    let pic = egui::Rect::from_min_size(
        rect.min + egui::vec2(2.0, 2.0),
        egui::vec2(w - 4.0, th - 2.0),
    );
    p.rect_filled(pic, theme::corner(5.0), l.pal.canvas);
    match thumb {
        Some(t) => paint_fit(p, pic.shrink(2.0), t),
        None => plate(p, pic, &l),
    }
    let x = rect.left() + 10.0;
    let mut job =
        egui::text::LayoutJob::simple_singleline(name.clone(), theme::prop(12.5), l.pal.ink);
    job.wrap.max_width = w - 20.0;
    job.wrap.max_rows = 1;
    let g = ui.fonts_mut(|f| f.layout_job(job));
    let p = ui.painter();
    p.galley(egui::pos2(x, pic.bottom() + 6.0), g, l.pal.ink);
    let line2 = if e.missing {
        "見つかりません".to_string()
    } else {
        let v = s.and_then(Summary::latest).map(|n| format!("v{n}  "));
        let when = s
            .and_then(|s| s.modified)
            .map(|t| ago_label(t, SystemTime::now()));
        format!("{}{}", v.unwrap_or_default(), when.unwrap_or_default())
    };
    p.text(
        egui::pos2(x, pic.bottom() + theme::fs(32.0)),
        egui::Align2::LEFT_CENTER,
        line2,
        theme::prop(11.0),
        if e.missing { l.pal.warn } else { l.pal.ink2 },
    );
    // 3 行目: 最近の頁はフォルダの名前、フォルダの中はジャンルとタグ
    let third = if in_folder {
        s.map(|s| {
            let mut v: Vec<String> = Vec::new();
            if !s.info.genre.is_empty() {
                v.push(s.info.genre.clone());
            }
            v.extend(s.info.tags.iter().map(|t| format!("#{t}")));
            v.join(" ")
        })
        .unwrap_or_default()
    } else {
        e.path
            .parent()
            .and_then(Path::file_name)
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    };
    let color = if in_folder { l.ws() } else { l.pal.ink3 };
    let mut job = egui::text::LayoutJob::simple_singleline(third, theme::prop(10.5), color);
    job.wrap.max_width = w - 20.0;
    job.wrap.max_rows = 1;
    let g = ui.fonts_mut(|f| f.layout_job(job));
    ui.painter()
        .galley(egui::pos2(x, pic.bottom() + theme::fs(42.0)), g, color);
    let p = ui.painter();
    if e.pinned {
        let s = theme::icon_px(13.0);
        let tag = egui::Rect::from_min_size(
            egui::pos2(pic.right() - s - 12.0, pic.top() + 6.0),
            egui::Vec2::splat(s + 6.0),
        );
        p.rect_filled(tag, theme::corner(3.0), l.ws());
        theme::paint_icon(p, tag.shrink(3.0), "pin", l.pal.chrome);
    }
    if on {
        // 選んだ印(左上の丸とチェック)
        let c = pic.left_top() + egui::vec2(14.0, 14.0);
        p.circle_filled(c, 8.0, l.ws());
        p.add(egui::Shape::line(
            vec![
                c + egui::vec2(-3.5, 0.0),
                c + egui::vec2(-1.0, 2.8),
                c + egui::vec2(3.8, -2.8),
            ],
            egui::Stroke::new(1.6, l.pal.chrome),
        ));
    }
    if e.missing {
        p.rect_filled(rect, theme::corner(6.0), l.pal.panel.gamma_multiply(0.55));
    }
    let hint = if e.missing {
        format!(
            "{}\n見つかりません。右クリックで一覧から外せます",
            e.path.display()
        )
    } else {
        format!(
            "{}\n押すと選び(Ctrl で足す・Shift で範囲)、二度押すと開きます。左のフォルダへ引くと移します",
            e.path.display()
        )
    };
    r.on_hover_text(hint)
}
