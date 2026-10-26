//! 整理: 左の「整理」と「保存した絞り込み」の行、同じ・似ているデザインの頁と右の組の違い、
//! 見つからないファイルの帯、中央の一覧(表)、フォルダそのものの操作(名前・移す・削除・ここに新規デザイン)。
//!
//! 判定は kmb-session の [`kmb_session::tidy`](画面は呼んで出すだけ)。どれも候補を並べるだけで、
//! 移す・消すは利用者が押した時に [`HomeRequest`] で頼む。

use kmb_ui::theme::RightMenu as _;
use crate::middle::band;
use crate::{
    Drag, GroupsKey, Item, Library, Place, Sort, Tidy, file_name, heading, key_button, place_row,
};
use kmb_session::library::{Period, ago_label, bytes_label};
use kmb_session::tidy::{self, Entry, Group, Kind};
use kmb_ui::theme;
use kmb_ui::{HomeHost, HomeRequest};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

impl Library {
    /// 全部のプロジェクトのデザインの値(読み終えた物だけ)と、読み終えたプロジェクトの数。
    pub(crate) fn all_entries(
        &mut self,
        ctx: &egui::Context,
        host: &HomeHost<'_>,
    ) -> (Vec<Entry>, usize) {
        let mut out = Vec::new();
        let mut loaded = 0;
        for p in &host.projects {
            if let Some(t) = self.trees.get(ctx, p) {
                loaded += 1;
                out.extend(t.entries.iter().cloned());
            }
        }
        (out, loaded)
    }

    /// 同じ・似ているデザインの組(控えて使う。仕分けやプロジェクトの読み終わりで作り直す)。
    pub(crate) fn tidy_groups(&mut self, ctx: &egui::Context, host: &HomeHost<'_>) -> Vec<Group> {
        let (entries, loaded) = self.all_entries(ctx, host);
        let key: GroupsKey = (host.changed, loaded, entries.len());
        if let Some((k, g)) = &self.groups
            && *k == key
        {
            return g.clone();
        }
        let g = tidy::groups(&entries);
        self.groups = Some((key, g.clone()));
        g
    }

    /// 左の「整理」と「保存した絞り込み」。
    pub(crate) fn left_tidy(&mut self, ui: &mut egui::Ui, host: &mut HomeHost<'_>) {
        heading(ui, "整理");
        let groups = self.tidy_groups(ui.ctx(), host).len();
        let (entries, _) = self.all_entries(ui.ctx(), host);
        let unclassified = entries
            .iter()
            .filter(|e| tidy::unclassified(&e.info))
            .count();
        let missing = host.recent.iter().filter(|r| r.missing).count();
        for (t, n) in [
            (Tidy::Similar, groups),
            (Tidy::Unclassified, unclassified),
            (Tidy::Missing, missing),
        ] {
            let place = Place::Tidy(t);
            let hint = match t {
                Tidy::Similar => {
                    "中身が同じ・名前が似ている・形が近いデザインの組(候補だけで、自動では動かしません)"
                }
                Tidy::Unclassified => "ジャンル・作品・タグのどれも付いていないデザイン",
                Tidy::Missing => "最近使ったデザインのうち、ファイルが見つからない物",
            };
            if place_row(ui, t.icon(), t.label(), Some(n), self.place == place)
                .on_hover_text(hint)
                .clicked()
            {
                self.go(place);
            }
        }
        heading(ui, "保存した絞り込み");
        let l = theme::look(ui.ctx());
        if host.filters.is_empty() {
            ui.label(
                egui::RichText::new(
                    "中央で絞り込み、「絞り込みを保存…」で名前を付けると、ここに並びます",
                )
                .size(theme::fs(11.0))
                .color(l.pal.ink3),
            );
        }
        for f in host.filters.clone() {
            let place = Place::Saved(f.name.clone());
            let r = place_row(ui, "search", &f.name, None, self.place == place)
                .on_hover_text(f.describe());
            r.right_menu(|ui| {
                if ui.button("削除").clicked() {
                    host.request(HomeRequest::RemoveFilter(f.name.clone()));
                    if self.place == place {
                        self.go(Place::Recent);
                    }
                    ui.close();
                }
            });
            if r.clicked() {
                self.go(place);
            }
        }
    }

    /// 絞り込みを保存する欄(絞り込んでいる時に「絞り込みを保存…」で出す)。
    pub(crate) fn save_filter_ui(&mut self, ui: &mut egui::Ui, host: &mut HomeHost<'_>) {
        let flt = self.filter();
        let Some(name) = self.filter_name.as_mut() else {
            return;
        };
        let mut done = false;
        let mut save = false;
        let l = theme::look(ui.ctx());
        band(ui, l.ws(), |ui| {
            ui.label(
                egui::RichText::new(format!("絞り込み({})に名前を付けて保存", flt.describe()))
                    .color(l.pal.ink),
            );
            let r = ui.add(
                egui::TextEdit::singleline(name)
                    .hint_text("名前")
                    .desired_width(180.0),
            );
            ui.ctx()
                .accesskit_node_builder(r.id, |b| b.set_label("絞り込みの名前"));
            if std::mem::take(&mut self.filter_focus) {
                r.request_focus();
            }
            save |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if key_button(ui, false, "キャンセル", "Esc", true).clicked()
                    || ui.input(|i| i.key_pressed(egui::Key::Escape))
                {
                    done = true;
                }
                save |= key_button(ui, true, "保存", "Enter", !name.trim().is_empty()).clicked();
            });
        });
        if save && !name.trim().is_empty() {
            let f = kmb_session::filters::Filter {
                name: name.trim().to_string(),
                ..flt
            };
            let place = Place::Saved(f.name.clone());
            host.request(HomeRequest::SaveFilter(f));
            done = true;
            // 保存した絞り込みを開く(中の語と絞りは外し、保存した物で絞る)
            self.query.clear();
            self.genre = None;
            self.work = None;
            self.tags.clear();
            self.period = Period::All;
            self.go(place);
        }
        if done {
            self.filter_name = None;
        }
    }

    /// 見つからないファイルの頁の帯(移した先が分かる物をまとめて付け替える)。
    pub(crate) fn missing_band(&mut self, ui: &mut egui::Ui, host: &mut HomeHost<'_>) {
        let found = self.moved_targets(host);
        let l = theme::look(ui.ctx());
        if found.is_empty() {
            return;
        }
        band(ui, l.ws(), |ui| {
            ui.label(
                egui::RichText::new(format!(
                    "{} 件は移した記録から今の場所が分かります",
                    found.len()
                ))
                .color(l.pal.ink),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::secondary(ui, "まとめて付け替える", "", true)
                    .on_hover_text("最近の一覧のパスを、移した先に直します(ファイルは動かしません)")
                    .clicked()
                {
                    for (from, to) in &found {
                        host.request(HomeRequest::Relink {
                            from: from.clone(),
                            to: to.clone(),
                        });
                    }
                }
            });
        });
    }

    /// 見つからない最近のデザインと、移した記録から分かる今の場所(分かる物だけ)。
    pub(crate) fn moved_targets(&self, host: &HomeHost<'_>) -> Vec<(PathBuf, PathBuf)> {
        let paths: Vec<&Path> = host
            .recent
            .iter()
            .filter(|r| r.missing)
            .map(|r| r.path.as_path())
            .collect();
        tidy::missing(paths)
            .into_iter()
            .filter_map(|m| Some((m.path, m.moved_to?)))
            .collect()
    }

    /// 同じ・似ているデザインの頁: 種類ごとに組を並べる。組を押すと右に組の違い。
    pub(crate) fn similar_page(&mut self, ui: &mut egui::Ui, host: &mut HomeHost<'_>) {
        let l = theme::look(ui.ctx());
        let groups = self.tidy_groups(ui.ctx(), host);
        let (_, loaded) = self.all_entries(ui.ctx(), host);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(Tidy::Similar.label())
                    .size(theme::fs(16.0))
                    .strong()
                    .color(l.pal.ink),
            );
            ui.label(
                egui::RichText::new(format!("{} 組", groups.len()))
                    .size(theme::fs(11.5))
                    .color(l.pal.ink3),
            );
        });
        ui.label(
            egui::RichText::new(
                "登録したプロジェクトの中から、候補を並べるだけです。自動では移しも消しもしません。",
            )
            .size(theme::fs(11.5))
            .color(l.pal.ink2),
        );
        ui.add_space(8.0);
        self.undo_band(ui, host);
        self.trash_band(ui, host);
        if host.projects.is_empty() {
            ui.label(
                egui::RichText::new("プロジェクトがありません(左の「プロジェクトを追加」)")
                    .color(l.pal.ink3),
            );
            return;
        }
        if loaded < host.projects.len() {
            ui.label(egui::RichText::new("読んでいます…").color(l.pal.ink3));
        }
        if groups.is_empty() && loaded == host.projects.len() {
            ui.label(egui::RichText::new("同じ・似ているデザインはありません。").color(l.pal.ink3));
            return;
        }
        let mut open = None;
        egui::ScrollArea::vertical()
            .id_salt("home_groups")
            .auto_shrink(false)
            .show(ui, |ui| {
                for kind in Kind::ALL {
                    let of: Vec<(usize, &Group)> = groups
                        .iter()
                        .enumerate()
                        .filter(|(_, g)| g.kind == kind)
                        .collect();
                    if of.is_empty() {
                        continue;
                    }
                    heading(ui, &format!("{}({} 組)", kind.label(), of.len()));
                    for (k, g) in of {
                        if let Some(p) = self.group_row(ui, host, k, g) {
                            open = Some(p);
                        }
                    }
                }
            });
        if let Some(p) = open {
            host.request(HomeRequest::Open(p));
        }
    }

    /// 組 1 つの行(小さなカードを横に並べる)。押すと組を選ぶ。二度押したデザインを返す(開く)。
    fn group_row(
        &mut self,
        ui: &mut egui::Ui,
        host: &mut HomeHost<'_>,
        k: usize,
        g: &Group,
    ) -> Option<PathBuf> {
        let l = theme::look(ui.ctx());
        let on = self.group == Some(k);
        let mut open = None;
        let r = egui::Frame::new()
            .fill(l.pal.panel)
            .stroke(egui::Stroke::new(
                if on { 2.0 } else { 1.0 },
                if on { l.ws() } else { l.pal.edge },
            ))
            .corner_radius(theme::corner(6.0))
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    for (i, p) in g.members.iter().enumerate() {
                        let thumb = host.thumbnail(ui.ctx(), p);
                        let s = self.summaries.get(ui.ctx(), p).cloned();
                        let r = mini_card(ui, p, s.as_ref(), thumb.as_ref(), i == 0);
                        r.dnd_set_drag_payload(Drag(vec![p.clone()]));
                        if r.double_clicked() {
                            open = Some(p.clone());
                        } else if r.clicked() {
                            self.group = Some(k);
                            self.selected.clear();
                        }
                        let it = Item {
                            path: p.clone(),
                            folder: None,
                            pinned: false,
                            missing: false,
                        };
                        r.right_menu(|ui| self.item_menu(ui, host, &it));
                    }
                });
            })
            .response;
        let r = r.interact(egui::Sense::click());
        r.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                true,
                on,
                format!("{}の組 {}", g.kind.label(), k + 1),
            )
        });
        if r.clicked() {
            self.group = Some(k);
            self.selected.clear();
        }
        ui.add_space(6.0);
        open
    }

    /// 右: 選んだ組の違いの表と、版の多い方を残してほかを削除…。
    pub(crate) fn group_panel(&mut self, ui: &mut egui::Ui, host: &mut HomeHost<'_>, k: usize) {
        let l = theme::look(ui.ctx());
        let groups = self.tidy_groups(ui.ctx(), host);
        let Some(g) = groups.get(k) else {
            self.group = None;
            return;
        };
        let (entries, _) = self.all_entries(ui.ctx(), host);
        let of: Vec<&Entry> = g
            .members
            .iter()
            .filter_map(|p| entries.iter().find(|e| &e.path == p))
            .collect();
        ui.label(
            egui::RichText::new("この組の違い")
                .size(theme::fs(15.0))
                .strong()
                .color(l.pal.ink),
        );
        ui.label(
            egui::RichText::new(format!("{}・{} 件", g.kind.label(), of.len()))
                .size(theme::fs(11.5))
                .color(l.pal.ink2),
        );
        ui.add_space(8.0);
        // 右は細いので、デザインごとに縦に並べる(違う所は色を変える)
        let table = tidy::compare(&of);
        for (i, e) in of.iter().enumerate() {
            let mut t = egui::RichText::new(&e.name)
                .size(theme::fs(12.5))
                .strong()
                .color(l.pal.ink);
            if i == 0 {
                t = t.color(l.ws());
            }
            ui.add_space(6.0);
            let r = ui.label(t);
            if i == 0 {
                r.on_hover_text("版が多い(同じなら新しい)ので、残す候補");
            }
            egui::Grid::new(("home_group_grid", i))
                .num_columns(2)
                .spacing(egui::vec2(10.0, 2.0))
                .show(ui, |ui| {
                    for row in &table {
                        let v = &row.values[i];
                        ui.label(
                            egui::RichText::new(row.label)
                                .size(theme::fs(11.0))
                                .color(l.pal.ink2),
                        );
                        let t = egui::RichText::new(if v.is_empty() { "—" } else { v })
                            .size(theme::fs(11.0));
                        ui.label(if row.differs {
                            t.color(l.pal.warn)
                        } else {
                            t.color(l.pal.ink)
                        });
                        ui.end_row();
                    }
                });
        }
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(
                "違う所は色を変えています。形の数は、保存の時に残した物です(古い版だけの物は —)。",
            )
            .size(theme::fs(10.5))
            .color(l.pal.ink3),
        );
        ui.add_space(10.0);
        let rest: Vec<PathBuf> = g
            .members
            .iter()
            .skip(1)
            .filter(|p| !host.open.contains(p))
            .cloned()
            .collect();
        if theme::secondary(ui, "版の多い方を残して、ほかを削除…", "", !rest.is_empty())
            .on_hover_text("消す物の一覧を出して確かめます(PC のごみ箱へ。開いている物は外します)")
            .clicked()
        {
            self.confirm_trash = Some(rest);
        }
    }

    /// 中央の一覧(表): 名前・場所・版・更新日・大きさ・タグ。見出しを押すと並べ替える。
    pub(crate) fn list_table(
        &mut self,
        ui: &mut egui::Ui,
        host: &mut HomeHost<'_>,
        items: &[Item],
    ) {
        let l = theme::look(ui.ctx());
        let now = SystemTime::now();
        // 並べ替え(フォルダは先頭のまま)
        let mut rows: Vec<(Item, Option<kmb_session::library::Summary>)> = items
            .iter()
            .map(|it| {
                let s = (it.folder.is_none())
                    .then(|| self.summaries.get(ui.ctx(), &it.path).cloned())
                    .flatten();
                (it.clone(), s)
            })
            .collect();
        let (sort, rev) = self.sort;
        if sort != Sort::None {
            rows.sort_by(|(a, sa), (b, sb)| {
                a.folder.is_none().cmp(&b.folder.is_none()).then_with(|| {
                    let o = match sort {
                        Sort::None | Sort::Name => file_name(&a.path).cmp(&file_name(&b.path)),
                        Sort::Place => a.path.parent().cmp(&b.path.parent()),
                        Sort::Version => sa
                            .as_ref()
                            .and_then(|s| s.latest())
                            .cmp(&sb.as_ref().and_then(|s| s.latest())),
                        Sort::Modified => sa
                            .as_ref()
                            .and_then(|s| s.modified)
                            .cmp(&sb.as_ref().and_then(|s| s.modified)),
                        Sort::Size => sa
                            .as_ref()
                            .map(|s| s.bytes)
                            .cmp(&sb.as_ref().map(|s| s.bytes)),
                    };
                    if rev { o.reverse() } else { o }
                })
            });
        }
        let order: Vec<PathBuf> = rows.iter().map(|(i, _)| i.path.clone()).collect();
        let cols = [
            (Sort::Name, "名前", 0.30),
            (Sort::Place, "場所", 0.22),
            (Sort::Version, "版", 0.07),
            (Sort::Modified, "更新日", 0.14),
            (Sort::Size, "大きさ", 0.10),
            (Sort::None, "タグ", 0.17),
        ];
        let w = ui.available_width();
        let row_h = 24.0 * theme::band();
        // 見出し
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (s, name, f) in cols {
                let mark = if s != Sort::None && sort == s {
                    if rev { " ▼" } else { " ▲" }
                } else {
                    ""
                };
                let (rect, r) =
                    ui.allocate_exact_size(egui::vec2(w * f, row_h), egui::Sense::click());
                r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
                ui.painter().text(
                    rect.left_center() + egui::vec2(8.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    format!("{name}{mark}"),
                    theme::prop(11.0),
                    l.pal.ink2,
                );
                if s != Sort::None && r.on_hover_text("押すと並べ替え(もう一度で逆)").clicked()
                {
                    self.sort = if sort == s { (s, !rev) } else { (s, false) };
                }
            }
        });
        ui.painter().hline(
            ui.min_rect().x_range(),
            ui.cursor().top(),
            egui::Stroke::new(1.0, l.pal.edge),
        );
        let mut open = None;
        egui::ScrollArea::vertical()
            .id_salt("home_list")
            .auto_shrink(false)
            .show(ui, |ui| {
                for (it, s) in &rows {
                    let on = self.selected.contains(&it.path);
                    let (rect, r) = ui.allocate_exact_size(
                        egui::vec2(w, row_h),
                        if it.folder.is_some() || it.missing {
                            egui::Sense::click()
                        } else {
                            egui::Sense::click_and_drag()
                        },
                    );
                    let name = file_name(&it.path);
                    r.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::SelectableLabel,
                            true,
                            on,
                            &name,
                        )
                    });
                    let p = ui.painter();
                    if on {
                        p.rect_filled(rect, theme::corner(3.0), l.ws_soft());
                    } else if r.hovered() {
                        p.rect_filled(rect, theme::corner(3.0), l.pal.edge.gamma_multiply(0.3));
                    }
                    let ink = if it.missing { l.pal.warn } else { l.pal.ink };
                    let cells: [String; 6] = match it.folder {
                        Some(n) => [
                            name.clone(),
                            String::new(),
                            String::new(),
                            String::new(),
                            format!("{n} 件"),
                            "フォルダ".into(),
                        ],
                        None => [
                            if it.pinned {
                                format!("📌 {name}")
                            } else {
                                name.clone()
                            },
                            it.path
                                .parent()
                                .map(|d| d.display().to_string())
                                .unwrap_or_default(),
                            s.as_ref()
                                .and_then(|s| s.latest())
                                .map(|v| format!("v{v}"))
                                .unwrap_or_default(),
                            if it.missing {
                                "見つかりません".into()
                            } else {
                                s.as_ref()
                                    .and_then(|s| s.modified)
                                    .map(|t| ago_label(t, now))
                                    .unwrap_or_default()
                            },
                            s.as_ref().map(|s| bytes_label(s.bytes)).unwrap_or_default(),
                            s.as_ref()
                                .map(|s| {
                                    s.info
                                        .tags
                                        .iter()
                                        .map(|t| format!("#{t}"))
                                        .collect::<Vec<_>>()
                                        .join(" ")
                                })
                                .unwrap_or_default(),
                        ],
                    };
                    let mut x = rect.left();
                    for (k, (_, _, f)) in cols.iter().enumerate() {
                        let cw = w * f;
                        let cell = egui::Rect::from_min_size(
                            egui::pos2(x, rect.top()),
                            egui::vec2(cw, row_h),
                        );
                        let mut job = egui::text::LayoutJob::simple_singleline(
                            cells[k].clone(),
                            theme::prop(if k == 0 { 12.0 } else { 11.0 }),
                            if k == 0 { ink } else { l.pal.ink2 },
                        );
                        job.wrap.max_width = (cw - 14.0).max(1.0);
                        job.wrap.max_rows = 1;
                        let g = ui.fonts_mut(|f| f.layout_job(job));
                        ui.painter().galley(
                            egui::pos2(cell.left() + 8.0, cell.center().y - g.size().y / 2.0),
                            g,
                            ink,
                        );
                        x += cw;
                    }
                    if it.folder.is_none() && !it.missing {
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
                    if let Some(n) = it.folder
                        && let Some(designs) = crate::middle::drop_target(ui, &r, &it.path, n)
                    {
                        host.request(HomeRequest::Move {
                            designs,
                            to: it.path.clone(),
                        });
                    }
                    if r.clicked() {
                        let m = ui.input(|i| i.modifiers);
                        self.click(&it.path, it.folder.is_some(), &order, m);
                    }
                    if r.double_clicked() && !it.missing {
                        open = Some(it.clone());
                    }
                    let r = r.on_hover_text(it.path.display().to_string());
                    r.right_menu(|ui| self.item_menu(ui, host, it));
                }
            });
        if let Some(it) = open {
            self.open_item(host, it);
        }
    }

    /// フォルダの右クリック(左の木の行とフォルダのカード)。プロジェクトそのものは動かさない。
    pub(crate) fn folder_menu(
        &mut self,
        ui: &mut egui::Ui,
        host: &mut HomeHost<'_>,
        project: &Path,
        path: &Path,
    ) {
        if ui.button("ここに新規デザイン").clicked() {
            host.request(HomeRequest::NewDesignIn(path.to_path_buf()));
            ui.close();
        }
        if ui.button("エクスプローラーで開く").clicked() {
            host.request(HomeRequest::ShowFolder(path.to_path_buf()));
            ui.close();
        }
        if path == project {
            if ui
                .button("一覧から外す")
                .on_hover_text("フォルダとデザインは消しません")
                .clicked()
            {
                host.request(HomeRequest::RemoveProject(path.to_path_buf()));
                ui.close();
            }
            return;
        }
        ui.separator();
        if ui.button("名前を変える…").clicked() {
            self.go(Place::Folder {
                project: project.to_path_buf(),
                folder: path.parent().unwrap_or(project).to_path_buf(),
            });
            self.selected = vec![path.to_path_buf()];
            self.renaming = Some((path.to_path_buf(), file_name(path)));
            self.rename_focus = true;
            ui.close();
        }
        ui.menu_button("移す", |ui| self.folder_move_menu(ui, host, path));
        if ui
            .button("削除…")
            .on_hover_text("フォルダと中の物を PC のごみ箱へ移します")
            .clicked()
        {
            self.confirm_trash_folder = Some(path.to_path_buf());
            ui.close();
        }
    }

    /// フォルダを移す行き先(自分とその中、今の親は選べない)。
    fn folder_move_menu(&mut self, ui: &mut egui::Ui, host: &mut HomeHost<'_>, folder: &Path) {
        self.destinations(
            ui,
            host,
            |path| path.starts_with(folder) || folder.parent() == Some(path),
            "ここへは移せません(今の場所か、自分の中)",
            |to| HomeRequest::MoveFolder {
                folder: folder.to_path_buf(),
                to,
            },
        );
    }

    /// フォルダをごみ箱へ移すかを確かめる帯。
    pub(crate) fn folder_trash_band(&mut self, ui: &mut egui::Ui, host: &mut HomeHost<'_>) {
        let Some(f) = self.confirm_trash_folder.clone() else {
            return;
        };
        let n = self.trees_count(ui.ctx(), host, &f);
        let l = theme::look(ui.ctx());
        band(ui, l.pal.warn, |ui| {
            ui.label(
                egui::RichText::new(format!(
                    "フォルダ「{}」と中のデザイン {n} 件を PC のごみ箱へ移します(版の履歴も一緒に)。ごみ箱から戻せます。",
                    file_name(&f)
                ))
                .strong()
                .color(l.pal.ink),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::secondary(ui, "キャンセル", "", true).clicked() {
                    self.confirm_trash_folder = None;
                }
                let go = egui::Button::new(egui::RichText::new("ごみ箱へ移す").color(l.pal.chrome))
                    .fill(l.pal.warn)
                    .stroke(egui::Stroke::new(1.0, l.pal.warn));
                if ui.add(go).clicked() {
                    host.request(HomeRequest::TrashFolder(f.clone()));
                    self.confirm_trash_folder = None;
                    self.selected.clear();
                }
            });
        });
    }

    /// フォルダ `f` の中のデザインの数(読んである木から。分からなければ 0)。
    fn trees_count(&mut self, ctx: &egui::Context, host: &HomeHost<'_>, f: &Path) -> usize {
        let Some(project) = Self::project_of(host, f) else {
            return 0;
        };
        self.trees
            .get(ctx, &project)
            .and_then(|t| t.root.find(f).map(|x| x.model_count()))
            .unwrap_or(0)
    }
}

/// 組の中の小さなカード(縮小図と名前と版)。残す候補には印。
fn mini_card(
    ui: &mut egui::Ui,
    path: &Path,
    s: Option<&kmb_session::library::Summary>,
    thumb: Option<&egui::TextureHandle>,
    keep: bool,
) -> egui::Response {
    let l = theme::look(ui.ctx());
    let w = 132.0 * theme::band().clamp(1.0, 1.4);
    let th = 78.0;
    let (rect, r) = ui.allocate_exact_size(
        egui::vec2(w, th + theme::fs(40.0)),
        egui::Sense::click_and_drag(),
    );
    let name = file_name(path);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
    let p = ui.painter();
    let edge = if r.hovered() { l.ws() } else { l.pal.edge };
    p.rect(
        rect,
        theme::corner(5.0),
        l.pal.panel,
        egui::Stroke::new(1.0, edge),
        egui::StrokeKind::Inside,
    );
    let pic = egui::Rect::from_min_size(rect.min + egui::vec2(2.0, 2.0), egui::vec2(w - 4.0, th));
    p.rect_filled(pic, theme::corner(4.0), l.pal.canvas);
    match thumb {
        Some(t) => crate::paint_fit(p, pic.shrink(2.0), t),
        None => crate::plate(p, pic, &l),
    }
    let mut job = egui::text::LayoutJob::simple_singleline(name, theme::prop(11.5), l.pal.ink);
    job.wrap.max_width = w - 14.0;
    job.wrap.max_rows = 1;
    let g = ui.fonts_mut(|f| f.layout_job(job));
    let p = ui.painter();
    p.galley(
        egui::pos2(rect.left() + 7.0, pic.bottom() + 4.0),
        g,
        l.pal.ink,
    );
    let sub = format!(
        "{}{}",
        s.and_then(|s| s.latest())
            .map(|v| format!("v{v}  "))
            .unwrap_or_default(),
        path.parent()
            .and_then(Path::file_name)
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    );
    p.text(
        egui::pos2(rect.left() + 7.0, pic.bottom() + theme::fs(28.0)),
        egui::Align2::LEFT_CENTER,
        sub,
        theme::prop(10.5),
        l.pal.ink3,
    );
    if keep {
        let g = p.layout_no_wrap("残す候補".into(), theme::prop(10.0), l.pal.chrome);
        let tag = egui::Rect::from_min_size(pic.left_top() + egui::vec2(5.0, 5.0), g.size())
            .expand2(egui::vec2(5.0, 2.0));
        p.rect_filled(tag, theme::corner(3.0), l.ws());
        p.galley(tag.min + egui::vec2(5.0, 2.0), g, l.pal.chrome);
    }
    r.on_hover_text(format!(
        "{}\n押すと組を選び、二度押すと開きます",
        path.display()
    ))
}
