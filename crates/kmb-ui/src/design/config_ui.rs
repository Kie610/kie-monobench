//! 構成表の欄。「パラメータ」の欄の下に、行(構成)× 列(パラメータの式・
//! 機能の抑制)の表を出す。左の丸で行を当てる(パラメータと抑制を書き換えて計算し直す。1 回の
//! 取り消しで戻る)。表の中身と当て方は [`kmb_interchange::configuration`](文書の区画)、CLI・MCP は同じ表を
//! 操作 `cad.config.*` で直す。

use crate::{FieldEdit, Workspace, reason_label};
use kmb_core::{FeatureId, ParamId, Transaction};
use kmb_interchange::configuration::{self, ConfigCell, ConfigColumn, ConfigRow, Configurations};

/// 表で押した物(描き終えてから文書を変える)。
enum Act {
    AddColumn(ConfigColumn),
    RemoveColumn(usize),
    AddRow,
    RemoveRow(String),
    Rename(String, String),
    Set(String, usize, ConfigCell),
    Activate(String),
    WriteBack(String),
}

impl Workspace {
    /// 構成表の欄(パラメータの欄の下)。
    pub(crate) fn config_ui(&mut self, ui: &mut egui::Ui) {
        let data = self.cur.work.doc().data();
        let c = configuration::of(data);
        let labels: Vec<String> = c
            .columns
            .iter()
            .map(|col| Configurations::column_label(data, col))
            .collect();
        let params: Vec<(ParamId, String)> = data
            .params
            .iter()
            .filter(|p| !c.columns.contains(&ConfigColumn::Param { param: p.id }))
            .map(|p| (p.id, p.name.clone()))
            .collect();
        let features: Vec<(FeatureId, String)> = data
            .features
            .iter()
            .filter(|f| {
                !c.columns
                    .contains(&ConfigColumn::Suppress { feature: f.id })
            })
            .map(|f| (f.id, f.name.clone()))
            .collect();
        let changed = c.active.as_ref().filter(|a| !c.matches(data, a)).cloned();
        let ink3 = self.win.look.pal.ink3;
        let warn = self.win.look.pal.warn;
        let mut act = None;
        ui.add_space(6.0);
        ui.separator();
        egui::CollapsingHeader::new(egui::RichText::new("構成表").strong())
            .id_salt("config_table")
            .default_open(!c.rows.is_empty())
            .show(ui, |ui| {
                if c.columns.is_empty() && c.rows.is_empty() {
                    ui.label(
                        egui::RichText::new(
                            "寸法違いの部品を 1 つの文書で持てます。「列を追加 ▾」で構成ごとに変えるパラメータか機能の抑制を足し、「構成を追加」で行を足します",
                        )
                        .small()
                        .color(ink3),
                    );
                } else {
                    egui::ScrollArea::horizontal()
                        .id_salt("config_scroll")
                        .show(ui, |ui| {
                            egui::Grid::new("config_grid").striped(true).show(ui, |ui| {
                                ui.label("");
                                ui.label(egui::RichText::new("構成").strong());
                                for (i, l) in labels.iter().enumerate() {
                                    let r = ui
                                        .label(egui::RichText::new(l).strong())
                                        .on_hover_text("右クリックで列を外す");
                                    r.context_menu(|ui| {
                                        if ui
                                            .button("列を外す")
                                            .on_hover_text("構成表から列を外す(パラメータや機能は消さない)")
                                            .clicked()
                                        {
                                            act = Some(Act::RemoveColumn(i));
                                            ui.close();
                                        }
                                    });
                                }
                                ui.end_row();
                                for (i, row) in c.rows.iter().enumerate() {
                                    if let Some(a) = row_ui(ui, i, row, &c, &labels) {
                                        act = Some(a);
                                    }
                                    ui.end_row();
                                }
                            });
                        });
                }
                // 当てた後にパラメータを直したか、当てた行の値を直した時は、どちらに合わせるか選べるようにする
                if let Some(a) = &changed {
                    ui.horizontal_wrapped(|ui| {
                        reason_label(ui, warn, &format!("今の値が構成「{a}」と違います"));
                        if ui
                            .button("当て直す")
                            .on_hover_text(format!("構成「{a}」の値をもう一度当てる"))
                            .clicked()
                        {
                            act = Some(Act::Activate(a.clone()));
                        }
                        if ui
                            .button(format!("「{a}」に書き込む"))
                            .on_hover_text("今のパラメータの式と抑制を、当てた構成の行に書き込む")
                            .clicked()
                        {
                            act = Some(Act::WriteBack(a.clone()));
                        }
                    });
                }
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .button("構成を追加")
                        .on_hover_text("今の値で構成表に行を足す")
                        .clicked()
                    {
                        act = Some(Act::AddRow);
                    }
                    ui.menu_button("列を追加 ▾", |ui| {
                        egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                            for (id, name) in &params {
                                if ui.button(name).clicked() {
                                    act = Some(Act::AddColumn(ConfigColumn::Param { param: *id }));
                                    ui.close();
                                }
                            }
                            if !params.is_empty() && !features.is_empty() {
                                ui.separator();
                            }
                            for (id, name) in &features {
                                if ui.button(format!("{name} の抑制")).clicked() {
                                    act = Some(Act::AddColumn(ConfigColumn::Suppress {
                                        feature: *id,
                                    }));
                                    ui.close();
                                }
                            }
                        });
                    })
                    .response
                    .on_hover_text("構成ごとに変えるパラメータか、抑制を切り替える機能を列に足す");
                });
            });
        if let Some(a) = act {
            self.config_act(a);
        }
    }

    fn config_act(&mut self, a: Act) {
        let data = self.cur.work.doc().data();
        let mut c = configuration::of(data);
        let (label, ops) = match a {
            Act::AddColumn(col) => {
                let label = format!(
                    "構成表に列「{}」を足す",
                    Configurations::column_label(data, &col)
                );
                if let Err(e) = c.add_column(data, col) {
                    self.win.fail(e);
                    return;
                }
                (label, None)
            }
            Act::RemoveColumn(i) => {
                let label = format!(
                    "構成表の列「{}」を外す",
                    Configurations::column_label(data, &c.columns[i])
                );
                if let Err(e) = c.remove_column(i) {
                    self.win.fail(e);
                    return;
                }
                (label, None)
            }
            Act::AddRow => {
                let name = c.free_name();
                let row = c.current_row(data, &name, None);
                c.rows.push(row);
                (format!("構成「{name}」を足す"), None)
            }
            Act::RemoveRow(name) => {
                c.rows.retain(|r| r.name != name);
                if c.active.as_deref() == Some(name.as_str()) {
                    c.active = None;
                }
                (format!("構成「{name}」を消す"), None)
            }
            Act::Rename(old, new) => {
                let new = new.trim().to_string();
                if new.is_empty() || c.row(&new).is_some() {
                    self.win.message = Some(format!(
                        "構成の名前を「{new}」にできません(空か、同じ名前の構成があります)"
                    ));
                    return;
                }
                if let Some(r) = c.rows.iter_mut().find(|r| r.name == old) {
                    r.name = new.clone();
                }
                if c.active.as_deref() == Some(old.as_str()) {
                    c.active = Some(new.clone());
                }
                (format!("構成「{old}」の名前を「{new}」にする"), None)
            }
            Act::Set(row, col, cell) => {
                // 長さのパラメータに数だけを打てば、文書の単位を付ける(パラメータの欄と同じ)
                let cell = match (&c.columns[col], cell) {
                    (ConfigColumn::Param { param }, ConfigCell::Expr(e)) => {
                        let length = data
                            .params
                            .iter()
                            .any(|p| p.id == *param && p.unit == kmb_core::ParamUnit::Length);
                        ConfigCell::Expr(if length {
                            self.in_doc_unit(e.trim())
                        } else {
                            e.trim().to_string()
                        })
                    }
                    (_, cell) => cell,
                };
                let label = format!(
                    "構成「{row}」の「{}」を直す",
                    Configurations::column_label(data, &c.columns[col])
                );
                if let Some(r) = c.rows.iter_mut().find(|r| r.name == row) {
                    r.cells[col] = cell;
                }
                (label, None)
            }
            Act::Activate(row) => match c.activate_ops(data, &row) {
                Ok(ops) => (format!("構成「{row}」を当てる"), Some(ops)),
                Err(e) => {
                    self.win.fail(e);
                    return;
                }
            },
            Act::WriteBack(row) => {
                if let Some(i) = c.rows.iter().position(|r| r.name == row) {
                    let next = c.current_row(data, &row, Some(&c.rows[i]));
                    c.rows[i] = next;
                }
                (format!("構成「{row}」に今の値を書き込む"), None)
            }
        };
        log::info!("{label}");
        let tx = match ops {
            Some(ops) => Transaction::new(label, ops),
            None => match kmb_session::edit::set_section(data, &label, &c) {
                Ok(tx) => tx,
                Err(e) => {
                    self.win.fail(e);
                    return;
                }
            },
        };
        self.apply(tx);
    }
}

/// 表の 1 行: 当てる丸・名前・列ごとの値。
fn row_ui(
    ui: &mut egui::Ui,
    i: usize,
    row: &ConfigRow,
    c: &Configurations,
    labels: &[String],
) -> Option<Act> {
    let mut act = None;
    let active = c.active.as_deref() == Some(row.name.as_str());
    let r = ui.radio(active, "");
    // 字の無い丸にも名前を残す(ui_* の道具とコマンド欄が名前で探す)
    let name = format!("構成「{}」を当てる", row.name);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::RadioButton, true, &name));
    if r.on_hover_text("この構成を当てる(パラメータの式と抑制を書き換えて計算し直す)")
        .clicked()
        && !active
    {
        act = Some(Act::Activate(row.name.clone()));
    }
    let r = text_cell(
        ui,
        crate::doc_id(ui.ctx(), ("config_row_name", i)),
        &row.name,
        80.0,
        &format!("{} の名前", row.name),
    );
    if let Some(t) = r.0 {
        act = Some(Act::Rename(row.name.clone(), t));
    }
    r.1.on_hover_text("構成の名前。右クリックで消す・今の値を書き込む")
        .context_menu(|ui| {
            if ui
                .button("今の値を書き込む")
                .on_hover_text("今のパラメータの式と抑制を、この行に書き込む")
                .clicked()
            {
                act = Some(Act::WriteBack(row.name.clone()));
                ui.close();
            }
            if ui
                .button("構成を削除")
                .on_hover_text("構成表から行を消す(今の形は変えない。取り消せる)")
                .clicked()
            {
                act = Some(Act::RemoveRow(row.name.clone()));
                ui.close();
            }
        });
    for (j, cell) in row.cells.iter().enumerate() {
        match cell {
            ConfigCell::Expr(e) => {
                let name = format!("{} の {}", row.name, labels[j]);
                let (t, _) = text_cell(
                    ui,
                    crate::doc_id(ui.ctx(), ("config_cell", i, j)),
                    e,
                    90.0,
                    &name,
                );
                if let Some(t) = t {
                    act = Some(Act::Set(row.name.clone(), j, ConfigCell::Expr(t)));
                }
            }
            ConfigCell::Suppressed(s) => {
                let mut on = *s;
                let r = ui.checkbox(&mut on, "抑制");
                let name = format!("{} の {}", row.name, labels[j]);
                r.widget_info(|| {
                    egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, on, &name)
                });
                if r.on_hover_text("この構成で機能を抑制する").changed() {
                    act = Some(Act::Set(row.name.clone(), j, ConfigCell::Suppressed(on)));
                }
            }
        }
    }
    act
}

/// 打っている間の文字を覚える欄。決めた(欄を離れた)時に変わっていれば新しい文字を返す。
/// `name` は台本と読み上げが欄を見つける名前(「構成2 の width」)。
fn text_cell(
    ui: &mut egui::Ui,
    id: egui::Id,
    now: &str,
    width: f32,
    name: &str,
) -> (Option<String>, egui::Response) {
    let mut text = ui
        .data(|d| d.get_temp::<FieldEdit>(id))
        .map_or_else(|| now.to_string(), |e| e.text_on(now));
    let r = ui.add(
        egui::TextEdit::singleline(&mut text)
            .id(id.with("edit"))
            .desired_width(width),
    );
    ui.ctx().accesskit_node_builder(r.id, |b| b.set_label(name));
    let done = (r.lost_focus() && text != now).then(|| text.clone());
    ui.data_mut(|d| d.insert_temp(id, FieldEdit::new(now, text)));
    (done, r)
}
