//! 表の窓(部品表・干渉など)。出す列と並びを「列」で選び、選び方は表の種類ごとに画面の配置へ覚える
//! (名前と見た目は独自)。行に立体のある表(干渉)は、
//! 3D のビューにその立体を赤く透かして重ね、行を押すとその行を強く見せる。

use crate::{Workspace, view3d};
use kmb_core::{Outcome, Table};

/// 開いている表の窓。
pub(crate) struct TableView {
    pub title: String,
    pub table: Table,
    /// 文書が変わった(次に描く時に、表を作った操作で作り直す)
    pub stale: bool,
    /// 列の設定を開いているか
    pub columns_open: bool,
    /// 押して選んだ行
    pub row: Option<usize>,
    /// 行の立体を全部見せるか(偽なら選んだ行だけ)
    pub show_all: bool,
}

/// 表の切り替えの値を、表を作った操作の引数にする(`change` は今切り替えた物)。
fn option_args(table: &Table, change: Option<(&str, bool)>) -> serde_json::Value {
    let mut m = serde_json::Map::new();
    for o in &table.options {
        let on = match change {
            Some((id, on)) if id == o.id => on,
            _ => o.on,
        };
        m.insert(o.id.clone(), on.into());
    }
    serde_json::Value::Object(m)
}

/// 出す列(表の列の番号を出す順に)。選び方を覚えていなければ既定の列。
pub(crate) fn shown_columns(table: &Table, saved: Option<&Vec<String>>) -> Vec<usize> {
    match saved {
        Some(ids) => ids
            .iter()
            .filter_map(|id| table.columns.iter().position(|c| &c.id == id))
            .collect(),
        None => (0..table.columns.len())
            .filter(|&i| table.columns[i].shown)
            .collect(),
    }
}

/// 列 `i` を出すか隠す。出す時は、既定の並びで後ろにある出している列の前に入れる。
pub(crate) fn toggle_column(shown: &mut Vec<usize>, i: usize) {
    if let Some(k) = shown.iter().position(|&c| c == i) {
        shown.remove(k);
    } else {
        let at = shown.iter().position(|&c| c > i).unwrap_or(shown.len());
        shown.insert(at, i);
    }
}

/// 出している列 `i` を `step`(-1 で前、1 で後ろ)へ 1 つ動かす。端なら動かさない。
pub(crate) fn move_column(shown: &mut [usize], i: usize, step: isize) {
    let Some(k) = shown.iter().position(|&c| c == i) else {
        return;
    };
    let j = k as isize + step;
    if j >= 0 && (j as usize) < shown.len() {
        shown.swap(k, j as usize);
    }
}

impl Workspace {
    /// 文書が変わった: 開いている表を次に描く時に作り直す。
    pub(crate) fn table_stale(&mut self) {
        if let Some(t) = &mut self.cur.table_view {
            t.stale = true;
        }
    }

    /// 表の窓を開く(同じ窓を置き換える。列の設定の開け閉めは残し、同じ表なら選んだ行と
    /// 「すべて表示」も残す)。
    pub(crate) fn open_table(&mut self, title: String, table: Table) {
        let old = self.cur.table_view.as_ref();
        let columns_open = old.is_some_and(|t| t.columns_open);
        let same = old.filter(|t| t.table.key == table.key);
        let row = same.and_then(|t| t.row).filter(|&r| r < table.rows.len());
        let show_all = same.is_none_or(|t| t.show_all);
        self.cur.table_view = Some(TableView {
            title,
            table,
            stale: false,
            columns_open,
            row,
            show_all,
        });
    }

    /// 開いている表で選んだ行。
    pub fn table_row(&self) -> Option<usize> {
        self.cur.table_view.as_ref().and_then(|t| t.row)
    }

    /// 開いている表の行を選ぶ(`None` で外す)。
    pub fn table_select(&mut self, row: Option<usize>) {
        if let Some(t) = &mut self.cur.table_view {
            t.row = row.filter(|&r| r < t.table.rows.len());
        }
    }

    /// 開いている表の立体を全部見せるか(偽なら選んだ行だけ)。
    pub fn table_show_all(&mut self, on: bool) {
        if let Some(t) = &mut self.cur.table_view {
            t.show_all = on;
        }
    }

    /// 開いている表の切り替え `id` を `on` にして、表を作った操作で作り直す。
    pub fn table_option(&mut self, id: &str, on: bool) {
        let Some(t) = &self.cur.table_view else {
            return;
        };
        let (key, args) = (t.table.key.clone(), option_args(&t.table, Some((id, on))));
        log::info!("表「{}」: {id} を {on} にして作り直す", t.title);
        self.rerun_table(&key, &args);
    }

    /// 表を作った操作 `key` を引数 `args` で動かし、表を置き換える。作れなければ前の表のまま。
    fn rerun_table(&mut self, key: &str, args: &serde_json::Value) {
        let r = self
            .run_through(key, self.cur.selected, None, args)
            .map(|ran| ran.handled);
        match r {
            Ok(kmb_session::Handled::Other(o)) if matches!(o.as_ref(), Outcome::Table { .. }) => {
                if let Outcome::Table { title, table } = *o {
                    self.open_table(title, table);
                }
            }
            Ok(_) => {
                if let Some(t) = &mut self.cur.table_view {
                    t.stale = false;
                }
            }
            Err(e) => {
                log::warn!("表を作り直せない: {e}");
                self.win.fail(e);
                if let Some(t) = &mut self.cur.table_view {
                    t.stale = false;
                }
            }
        }
    }

    /// 開いている表の行の立体を、3D のビューに透かして重ねる物(選んだ行は濃く)。
    pub(crate) fn table_ghosts(&self) -> Vec<view3d::Ghost> {
        let Some(t) = &self.cur.table_view else {
            return Vec::new();
        };
        let inks = &self.win.look.pal.inks;
        let ink = inks.ghost_cut;
        let tint = |a: u8| egui::Color32::from_rgba_unmultiplied(ink.r(), ink.g(), ink.b(), a);
        t.table
            .marks
            .iter()
            .enumerate()
            .filter(|(i, m)| (t.show_all || t.row == Some(*i)) && !m.tris.is_empty())
            .map(|(i, m)| {
                let on = t.row == Some(i);
                view3d::Ghost {
                    tris: m.tris.clone(),
                    edges: m.edges.clone(),
                    color: tint(if on { 200 } else { view3d::GHOST_CUT_ALPHA }),
                    edge_color: if on { self.win.look.pal.err } else { ink },
                    layer: view3d::GhostLayer::Over,
                }
            })
            .collect()
    }

    /// 開いている表の窓の題と中身。
    pub fn table_view(&self) -> Option<(&str, &Table)> {
        self.cur
            .table_view
            .as_ref()
            .map(|t| (t.title.as_str(), &t.table))
    }

    /// 開いている表で出す列の番号(出す順)。
    pub fn table_shown(&self) -> Vec<usize> {
        self.cur.table_view.as_ref().map_or_else(Vec::new, |t| {
            shown_columns(&t.table, self.win.layout.table_columns.get(&t.table.key))
        })
    }

    /// 開いている表の列を出す・隠す・動かす(`step` が 0 なら出す・隠すを入れ替える)。
    /// 最後の 1 列は隠さない。
    pub fn table_column(&mut self, id: &str, step: isize) {
        let Some(t) = &self.cur.table_view else {
            return;
        };
        let Some(i) = t.table.columns.iter().position(|c| c.id == id) else {
            return;
        };
        let mut shown = self.table_shown();
        if step == 0 {
            if shown == [i] {
                return;
            }
            toggle_column(&mut shown, i);
        } else {
            move_column(&mut shown, i, step);
        }
        let ids = shown
            .iter()
            .map(|&c| t.table.columns[c].id.clone())
            .collect();
        let key = t.table.key.clone();
        self.win.layout.table_columns.insert(key, ids);
    }

    /// 開いている表の列を既定に戻す。
    pub fn table_columns_reset(&mut self) {
        if let Some(t) = &self.cur.table_view {
            let key = t.table.key.clone();
            self.win.layout.table_columns.remove(&key);
        }
    }

    /// 表を作った操作で作り直す(窓を開いている間に文書が変わった時。切り替えの値はそのまま)。
    /// 作れなければ前の表のまま。
    fn refresh_table(&mut self) {
        let Some((key, args)) = self
            .cur
            .table_view
            .as_ref()
            .filter(|t| t.stale)
            .map(|t| (t.table.key.clone(), option_args(&t.table, None)))
        else {
            return;
        };
        self.rerun_table(&key, &args);
    }

    /// 表の窓。毎フレーム呼ぶ。
    pub(crate) fn table_window(&mut self, ctx: &egui::Context) {
        self.refresh_table();
        let Some(view) = &self.cur.table_view else {
            return;
        };
        let l = crate::theme::look(ctx);
        let table = &view.table;
        let shown = shown_columns(table, self.win.layout.table_columns.get(&table.key));
        let mut columns_open = view.columns_open;
        let mut show_all = view.show_all;
        let mut row = view.row;
        let mut option: Option<(String, bool)> = None;
        // 行に立体を持つ種類の表か(行が 0 でも窓の置き場所と切り替えを変えない)
        let marked = !table.marks_label.is_empty();
        let mut open = true;
        let mut export = false;
        let mut change: Option<(String, isize)> = None;
        let mut reset = false;
        // 立体を重ねる表は、重ねた所を隠さないよう 3D のビューの左下に小さめに出す
        let (size, pivot, pos) = if marked {
            (
                egui::vec2(620.0, 260.0),
                egui::Align2::LEFT_BOTTOM,
                egui::pos2(262.0, ctx.content_rect().bottom() - 110.0),
            )
        } else {
            (
                egui::vec2(900.0, 420.0),
                egui::Align2::CENTER_CENTER,
                ctx.content_rect().center(),
            )
        };
        egui::Window::new(view.title.as_str())
            .id(egui::Id::new(("table_view", table.key.as_str())))
            .collapsible(false)
            .resizable(true)
            .default_size(size)
            .pivot(pivot)
            .default_pos(pos)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // 枠のあるボタンにする(枠の無い切り替えは、閉じている時にただの字に見えた)
                    if ui
                        .add(egui::Button::new("列…").selected(columns_open))
                        .on_hover_text("出す列と並びを選ぶ")
                        .clicked()
                    {
                        columns_open = !columns_open;
                    }
                    if table.export.is_some()
                        && ui
                            .button("書き出す…")
                            .on_hover_text("CSV(表計算ソフトで開ける)に書き出す")
                            .clicked()
                    {
                        export = true;
                    }
                    ui.weak(format!("{} 行", table.rows.len()));
                    for o in &table.options {
                        let mut v = o.on;
                        if ui
                            .checkbox(&mut v, o.label.as_str())
                            .on_hover_text(&o.hint)
                            .changed()
                        {
                            option = Some((o.id.clone(), v));
                        }
                    }
                    if marked {
                        ui.checkbox(&mut show_all, table.marks_label.as_str())
                            .on_hover_text("切ると、押した行の所だけを 3D に出す");
                    }
                });
                ui.separator();
                if columns_open {
                    egui::Panel::right("table_columns")
                        .resizable(false)
                        .default_size(190.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.strong("列");
                                if ui
                                    .small_button("既定に戻す")
                                    .on_hover_text("出す列と並びを最初の形にする")
                                    .clicked()
                                {
                                    reset = true;
                                }
                            });
                            // 出している列を出す順に、その後に隠した列を元の順に
                            let hidden = (0..table.columns.len()).filter(|i| !shown.contains(i));
                            let order: Vec<usize> = shown.iter().copied().chain(hidden).collect();
                            egui::ScrollArea::vertical()
                                .id_salt("table_columns_list")
                                .auto_shrink(false)
                                .show(ui, |ui| {
                                    for (k, &i) in order.iter().enumerate() {
                                        let c = &table.columns[i];
                                        let on = k < shown.len();
                                        ui.horizontal(|ui| {
                                            let mut v = on;
                                            let last = on && shown.len() == 1;
                                            let r = ui.add_enabled(
                                                !last,
                                                egui::Checkbox::new(&mut v, c.label.as_str()),
                                            );
                                            if r.changed() {
                                                change = Some((c.id.clone(), 0));
                                            }
                                            if on {
                                                ui.with_layout(
                                                    egui::Layout::right_to_left(
                                                        egui::Align::Center,
                                                    ),
                                                    |ui| {
                                                        let down = ui.add_enabled(
                                                            k + 1 < shown.len(),
                                                            egui::Button::new("▼").small(),
                                                        );
                                                        if down.on_hover_text("後ろへ").clicked()
                                                        {
                                                            change = Some((c.id.clone(), 1));
                                                        }
                                                        let up = ui.add_enabled(
                                                            k > 0,
                                                            egui::Button::new("▲").small(),
                                                        );
                                                        if up.on_hover_text("前へ").clicked() {
                                                            change = Some((c.id.clone(), -1));
                                                        }
                                                    },
                                                );
                                            }
                                        });
                                    }
                                });
                        });
                }
                if !table.note.is_empty() {
                    egui::Panel::bottom("table_note")
                        .show_separator_line(false)
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(&table.note)
                                    .size(crate::theme::fs(10.5))
                                    .color(l.pal.ink3),
                            );
                        });
                }
                egui::ScrollArea::both()
                    .id_salt("table_rows")
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        if table.rows.is_empty() {
                            ui.weak(if table.empty.is_empty() {
                                "行がありません"
                            } else {
                                table.empty.as_str()
                            });
                            return;
                        }
                        if let Some(r) = grid(ui, table, &shown, l.pal.ink3, row) {
                            // もう一度押すと外す
                            row = (row != Some(r)).then_some(r);
                        }
                    });
            });
        let export = export.then(|| table.export.clone()).flatten();
        if let Some(t) = &mut self.cur.table_view {
            t.columns_open = columns_open;
            t.show_all = show_all;
            t.row = row;
        }
        if let Some((id, on)) = option {
            self.table_option(&id, on);
        }
        if let Some((id, step)) = change {
            self.table_column(&id, step);
        }
        if reset {
            self.table_columns_reset();
        }
        if !open {
            self.cur.table_view = None;
        }
        if let Some(id) = export {
            self.run_action(&id);
        }
    }
}

/// 表の見出しと行。列の幅は見出しと値のいちばん広い字に合わせ、数の列は右に寄せる
/// (egui の Grid の右寄せは最後の列で隣に重なったので、幅を測って並べる)。行を押せ、
/// 選んだ行 `chosen` を塗る。押した行の番号を返す。
fn grid(
    ui: &mut egui::Ui,
    table: &Table,
    shown: &[usize],
    weak: egui::Color32,
    chosen: Option<usize>,
) -> Option<usize> {
    let text = |row: Option<&Vec<String>>, i: usize| -> egui::RichText {
        match row {
            None => egui::RichText::new(&table.columns[i].label).strong(),
            Some(r) => match r.get(i).map_or("", String::as_str) {
                "" => egui::RichText::new("-").color(weak),
                v => egui::RichText::new(v),
            },
        }
    };
    let measure = |ui: &egui::Ui, t: egui::RichText| -> f32 {
        egui::WidgetText::from(t)
            .into_galley(
                ui,
                Some(egui::TextWrapMode::Extend),
                f32::INFINITY,
                egui::TextStyle::Body,
            )
            .size()
            .x
    };
    let widths: Vec<f32> = shown
        .iter()
        .map(|&i| {
            std::iter::once(None)
                .chain(table.rows.iter().map(Some))
                .map(|r| measure(ui, text(r, i)))
                .fold(0.0, f32::max)
        })
        .collect();
    let gap = 16.0;
    let h = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
    let total = widths.iter().sum::<f32>() + gap * (widths.len() + 1) as f32;
    let stripe = ui.visuals().faint_bg_color;
    let pick = ui.visuals().selection.bg_fill;
    let mut clicked = None;
    for (k, row) in std::iter::once(None)
        .chain(table.rows.iter().map(Some))
        .enumerate()
    {
        let sense = if k == 0 {
            egui::Sense::hover()
        } else {
            egui::Sense::click()
        };
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(total, h), sense);
        if k > 0 && chosen == Some(k - 1) {
            ui.painter().rect_filled(rect, 0.0, pick);
        } else if k > 0 && resp.hovered() {
            ui.painter()
                .rect_filled(rect, 0.0, ui.visuals().widgets.hovered.weak_bg_fill);
        } else if k % 2 == 1 {
            ui.painter().rect_filled(rect, 0.0, stripe);
        }
        if resp.clicked() {
            clicked = Some(k - 1);
        }
        let mut x = rect.left() + gap;
        for (&i, w) in shown.iter().zip(&widths) {
            let cell = egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(*w, h));
            let layout = if table.columns[i].numeric {
                egui::Layout::right_to_left(egui::Align::Center)
            } else {
                egui::Layout::left_to_right(egui::Align::Center)
            };
            // 字は押せないようにし、押した所を行で受ける
            ui.scope_builder(egui::UiBuilder::new().max_rect(cell).layout(layout), |ui| {
                ui.add(egui::Label::new(text(row, i)).selectable(false));
            });
            x += w + gap;
        }
    }
    clicked
}
