//! スケッチを編集している間のツールバー(編集器の道具をまとまりに並べる)と、スケッチの終わり方。

use super::*;
use ribbon_band::{
    group_label, group_widths, more_width, name_width, small_grid, small_width, sub_menu_row,
    tile_hint, tile_width, tool_tile,
};

impl Workspace {
    /// スケッチを直している間の帯: 編集器が出す道具のまとまりと、そのすぐ右の「スケッチを終了」。
    /// 使っている道具を光らせる。前に出す道具は設計の帯と同じく ▾ の一覧のピンで選び、右クリックで
    /// 並べ替える。
    pub(super) fn sketch_band(&mut self, ui: &mut egui::Ui) {
        let l = theme::look(ui.ctx());
        let Some(entry) = self
            .cur
            .selected
            .and_then(|id| self.cur.work.doc().data().feature(id).cloned())
        else {
            return;
        };
        let groups = self.view_tool_groups();
        self.search.view_tools = groups
            .iter()
            .flat_map(|g| g.all().into_iter().map(move |t| (g, t)))
            .map(|(g, t)| {
                let name = g.name.clone();
                (
                    t.id.clone(),
                    t.label.clone(),
                    t.hint.clone(),
                    t.enabled,
                    name,
                )
            })
            .collect();
        let space = Space::Design;
        // 道具がまとまりを移った(ミラーと並べるを「修正」から「作成」へ)時のピン
        let rgs: Vec<ribbon::Group> = groups.iter().map(sketch_group).collect();
        ribbon::migrate_pins(space, SKETCH_TAB, &rgs, &mut self.win.layout);
        let mut used: Option<String> = None;
        let labels = self.win.layout.appearance.tool_labels;
        let fronts: Vec<Vec<crate::EditorTool>> = groups
            .iter()
            .map(|g| {
                let rg = sketch_group(g);
                let all = g.all();
                let mut front: Vec<crate::EditorTool> =
                    ribbon::front(space, SKETCH_TAB, &rg, &self.win.layout)
                        .into_iter()
                        .filter_map(|it| all.iter().find(|t| t.id == it.key).map(|t| (*t).clone()))
                        .collect();
                // 使っている道具が前にない(▾ の一覧から選んだ)時は、終わるまで後ろに出して光らせる
                // (小さなボタンの道具は、そこで光る)
                if let Some(t) = all.iter().find(|t| t.on)
                    && !front.iter().any(|f| f.id == t.id)
                    && !g.small.iter().any(|f| f.id == t.id)
                {
                    front.push((*t).clone());
                }
                front
            })
            .collect();
        // 小さなボタンには、前に出した道具を出さない(前の大きなボタン 1 つだけにする)
        let smalls: Vec<Vec<crate::EditorTool>> = groups
            .iter()
            .zip(&fronts)
            .map(|(g, front)| small_tools(&g.small, front))
            .collect();
        let widths: Vec<ribbon::Widths> = groups
            .iter()
            .zip(&fronts)
            .zip(&smalls)
            .map(|((g, f), small)| {
                let mut w = group_widths(ui, &g.name, f.len(), f.iter().position(|t| t.on), labels);
                // 小さなボタンは、前の道具を全部出す時だけ並べる(帯に入らなければ先に畳む)
                let extra = small_width(small.len(), labels);
                w.full += extra;
                for (k, c) in w.compact.iter_mut().enumerate() {
                    if k + 1 >= f.len() {
                        *c += extra;
                    }
                }
                w
            })
            .collect();
        // 右の「スケッチを終了」(前の 14 のすき間と、ボタンの字と余白)は必ず帯に出す
        let finish = 14.0
            + ui.painter()
                .layout_no_wrap(
                    crate::panel_ui::SKETCH_FINISH.into(),
                    theme::prop(12.5),
                    l.pal.ink,
                )
                .size()
                .x
            + 2.0 * ui.spacing().button_padding.x
            + 8.0;
        let fold = ribbon::fit(&widths, finish, more_width(ui), ui.available_width());
        for (((g, front), f), g_small) in groups.iter().zip(&fronts).zip(&fold).zip(&smalls) {
            let shown: Vec<&crate::EditorTool> = match f {
                ribbon::Fold::Hidden => continue,
                ribbon::Fold::Compact(n) => front
                    .iter()
                    .enumerate()
                    .filter(|(k, t)| *k < *n || t.on)
                    .map(|(_, t)| t)
                    .collect(),
                _ => front.iter().collect(),
            };
            let compact = !labels || *f != ribbon::Fold::Full;
            let small: &[crate::EditorTool] = match f {
                ribbon::Fold::Compact(n) if *n < front.len() => &[],
                _ => g_small,
            };
            let rg = sketch_group(g);
            let all = g.all();
            let n = shown.len().max(1) as f32;
            let w = (n * tile_width(compact) + (n - 1.0) * 2.0 + 8.0).max(name_width(ui, &g.name))
                + small_width(small.len(), labels);
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(w, self.band_height()), egui::Sense::hover());
            let mut col = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(rect.shrink2(egui::vec2(4.0, 0.0)))
                    .layout(egui::Layout::top_down(egui::Align::Center)),
            );
            col.spacing_mut().item_spacing = egui::vec2(2.0, 0.0);
            col.add_space(3.0);
            col.horizontal(|ui| {
                for t in &shown {
                    // 親のある道具は、同じ親の道具が前に 1 つだけなら親の名前で出す(「2 点指定の長方形」は
                    // 「長方形」。種類で見分ける)。説明には道具の名前を出す
                    let label = match &t.sub {
                        Some(p)
                            if shown.iter().filter(|f| f.sub.as_ref() == Some(p)).count() == 1 =>
                        {
                            p.as_str()
                        }
                        _ => t.label.as_str(),
                    };
                    let r = tool_tile(ui, &t.icon, label, t.enabled, t.on, compact, labels);
                    let r = if label == t.label {
                        r.on_hover_text(tile_hint(label, &t.hint, compact))
                    } else {
                        r.on_hover_text(format!("{}: {}", t.label, t.hint))
                    };
                    if r.clicked() {
                        used = Some(t.id.clone());
                    }
                    r.context_menu(|ui| {
                        let mut e = None;
                        if ui.button("左へ").clicked() {
                            e = Some(PinEdit::Move(t.id.clone(), -1));
                        }
                        if ui.button("右へ").clicked() {
                            e = Some(PinEdit::Move(t.id.clone(), 1));
                        }
                        if ui.button("前から外す").clicked() {
                            e = Some(PinEdit::Unpin(t.id.clone()));
                        }
                        if ui.button("このまとまりを既定に戻す").clicked() {
                            e = Some(PinEdit::Reset);
                        }
                        if let Some(e) = e {
                            ribbon::edit_pins(space, SKETCH_TAB, &rg, &mut self.win.layout, e);
                            ui.close();
                        }
                    });
                }
                if let Some(id) = small_grid(ui, small, labels) {
                    used = Some(id);
                }
            });
            let lit = all.iter().any(|t| t.on);
            let r = group_label(ui, rect, ("sk_glabel", &g.name), &g.name, lit);
            egui::Popup::menu(&r).show(|ui| {
                let pinned: Vec<String> = ribbon::front(space, SKETCH_TAB, &rg, &self.win.layout)
                    .iter()
                    .map(|i| i.key.clone())
                    .collect();
                // 道具が多い(描くは 28)ので、画面の下に届く時は巻物にする(窓が低くても全部選べる)
                let max_h = (ui.ctx().content_rect().bottom() - ui.next_widget_position().y - 60.0)
                    .max(160.0);
                // 親のある道具は親の行にまとめ、指を置くと子の一覧を横に出す
                egui::ScrollArea::vertical()
                    .max_height(max_h)
                    .show(ui, |ui| {
                        let menu = g.menu();
                        let mut i = 0;
                        while i < menu.len() {
                            let t = menu[i];
                            if t.sep && i > 0 {
                                ui.separator();
                            }
                            let Some(parent) = &t.sub else {
                                if self.tool_menu_row(ui, t, &rg, &pinned) {
                                    used = Some(t.id.clone());
                                    ui.close();
                                }
                                i += 1;
                                continue;
                            };
                            let kids: Vec<&crate::EditorTool> = menu[i..]
                                .iter()
                                .take_while(|k| k.sub.as_ref() == Some(parent))
                                .copied()
                                .collect();
                            i += kids.len();
                            let lit = kids.iter().any(|k| k.on);
                            let icon = kids.iter().find(|k| k.on).unwrap_or(&kids[0]).icon.clone();
                            let row = ui
                                .horizontal(|ui| {
                                    // ピンの欄(30×26)だけ空けて、子のない行と名前の位置と高さをそろえる
                                    ui.allocate_exact_size(
                                        egui::vec2(30.0, 26.0),
                                        egui::Sense::hover(),
                                    );
                                    let r = theme::menu_item(ui, &icon, parent, true, lit);
                                    crate::icons::paint(
                                        ui.painter(),
                                        egui::Rect::from_center_size(
                                            egui::pos2(r.rect.right() - 10.0, r.rect.center().y),
                                            egui::vec2(12.0, 12.0),
                                        ),
                                        "chevr",
                                        l.pal.ink2,
                                        1.6,
                                    );
                                    r
                                })
                                .inner;
                            let row = row.on_hover_text(
                                kids.iter()
                                    .map(|k| k.label.as_str())
                                    .collect::<Vec<_>>()
                                    .join("・"),
                            );
                            egui::containers::menu::SubMenu::new().show(ui, &row, |ui| {
                                for (j, k) in kids.iter().enumerate() {
                                    if k.sep && j > 0 {
                                        ui.separator();
                                    }
                                    if self.tool_menu_row(ui, k, &rg, &pinned) {
                                        used = Some(k.id.clone());
                                        ui.close();
                                    }
                                }
                            });
                        }
                    });
                ui.separator();
                if ui.button("前の並びを既定に戻す").clicked() {
                    ribbon::edit_pins(space, SKETCH_TAB, &rg, &mut self.win.layout, PinEdit::Reset);
                }
            });
            ui.painter().vline(
                rect.right() - 0.5,
                rect.y_range(),
                egui::Stroke::new(1.0, l.pal.edge),
            );
        }
        // 入りきらなかったまとまりは、右端の「その他」から選ぶ
        let hidden: Vec<&crate::ToolGroup> = groups
            .iter()
            .zip(&fold)
            .filter(|(_, f)| **f == ribbon::Fold::Hidden)
            .map(|(g, _)| g)
            .collect();
        if !hidden.is_empty() {
            let lit = hidden.iter().any(|g| g.all().iter().any(|t| t.on));
            let (tile, label) = self.more_group(ui, lit);
            for r in [tile, label] {
                egui::Popup::menu(&r).show(|ui| {
                    for g in &hidden {
                        let menu = g.menu();
                        let icon = menu.first().map_or("", |t| t.icon.as_str());
                        let lit = menu.iter().any(|t| t.on);
                        let row = sub_menu_row(ui, icon, &g.name, lit);
                        egui::containers::menu::SubMenu::new().show(ui, &row, |ui| {
                            for t in &menu {
                                if theme::menu_item(ui, &t.icon, &t.label, t.enabled, t.on)
                                    .on_hover_text(&t.hint)
                                    .clicked()
                                {
                                    used = Some(t.id.clone());
                                    ui.close();
                                }
                            }
                        });
                    }
                });
            }
        }
        if groups.is_empty() {
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.add_space(22.0);
                ui.label(
                    egui::RichText::new(format!("{} を直しています", entry.name)).color(l.pal.ink2),
                );
            });
        }
        // 「スケッチを終了」は道具のまとまりのすぐ右に置く(
        // 窓の右端に離して置くと、広い画面で見落とす。レビュー K15)
        ui.add_space(14.0);
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width().max(0.0), self.band_height()),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                let r = theme::primary(ui, crate::panel_ui::SKETCH_FINISH, "", true)
                    .on_hover_text("スケッチを決めて 3D のビューに戻る(スケッチは選んだまま)");
                if r.clicked() {
                    self.finish_sketch();
                }
            },
        );
        if let Some(id) = used {
            self.use_editor_tool(&id);
        }
    }

    /// スケッチの帯の ▾ の一覧の 1 行(ピンと道具)。道具が押されたら真。
    fn tool_menu_row(
        &mut self,
        ui: &mut egui::Ui,
        t: &crate::EditorTool,
        rg: &ribbon::Group,
        pinned: &[String],
    ) -> bool {
        ui.horizontal(|ui| {
            let on_front = pinned.contains(&t.id);
            let what = if on_front {
                "前から外す"
            } else {
                "前に出す"
            };
            let pr = theme::icon_button(ui, "pin", what, true, on_front).on_hover_text(what);
            if pr.clicked() {
                let e = if on_front {
                    PinEdit::Unpin(t.id.clone())
                } else {
                    PinEdit::Pin(t.id.clone())
                };
                ribbon::edit_pins(Space::Design, SKETCH_TAB, rg, &mut self.win.layout, e);
            }
            theme::menu_item(ui, &t.icon, &t.label, t.enabled, t.on)
                .on_hover_text(&t.hint)
                .clicked()
        })
        .inner
    }

    /// 中央のビューを持つ編集器の帯の道具を押す(帯とコマンド検索から)。
    pub(crate) fn use_editor_tool(&mut self, id: &str) {
        let Some(type_id) = self.view_editor_type() else {
            return;
        };
        let Some(entry) = self
            .cur
            .selected
            .and_then(|id| self.cur.work.doc().data().feature(id).cloned())
        else {
            return;
        };
        let changed = self.with_editors(|eds, host| {
            eds.iter_mut()
                .find(|ed| ed.handles(&type_id) && ed.takes_view())
                .map(|ed| (ed.use_tool(id, host, &entry), ed.edit_label()))
        });
        if let Some((Some(data), label)) = changed {
            let mut tx =
                jc_session::edit::set_feature_data(self.cur.work.doc().data(), entry.id, data);
            tx.label = format!("{}({})", label, entry.name);
            self.apply(tx);
        }
    }

    /// スケッチを終了(3D のビューへ戻る)。作っている途中なら決める。
    /// スケッチは選んだまま残すので、続けて「押し出し」を押せる。
    pub fn finish_sketch(&mut self) {
        self.finish_edit();
    }
}

/// スケッチの帯のピンの鍵に使うタブの名前(設計の帯のタブと重ならない)。
const SKETCH_TAB: &str = "スケッチ";

/// 編集器の道具のまとまりを、ピンを扱うツールバーのまとまりにする(前に出すのは `tools`)。
fn sketch_group(g: &crate::ToolGroup) -> ribbon::Group {
    let item = |t: &crate::EditorTool, front: bool| Item {
        key: t.id.clone(),
        label: t.label.clone(),
        icon: t.icon.clone(),
        hint: t.hint.clone(),
        disabled: (!t.enabled).then(|| "今は使えません".to_string()),
        front,
        formerly: Vec::new(),
    };
    ribbon::Group {
        name: g.name.clone(),
        items: g
            .tools
            .iter()
            .map(|t| item(t, true))
            .chain(g.small.iter().chain(&g.more).map(|t| item(t, false)))
            .collect(),
    }
}

/// 2 段の小さなボタンに出す道具。前に出した道具(`front`)は除く。
fn small_tools(small: &[crate::EditorTool], front: &[crate::EditorTool]) -> Vec<crate::EditorTool> {
    small
        .iter()
        .filter(|t| !front.iter().any(|f| f.id == t.id))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::EditorTool;

    #[test]
    fn tool_brought_to_front_leaves_the_small_buttons() {
        let t = |id: &str| EditorTool::new(id, id, "", "");
        let small = [t("接線"), t("直交"), t("固定")];
        let front = [t("一致"), t("直交")];
        let ids: Vec<String> = super::small_tools(&small, &front)
            .into_iter()
            .map(|t| t.id)
            .collect();
        assert_eq!(ids, ["接線", "固定"]);
    }
}
