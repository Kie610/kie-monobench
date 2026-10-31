//! 自由な要素の読み取り専用の欄と、図へ渡す強調。
use crate::view::DimValues;
use kmb_sketch::{EntityId, FreedomReport, Sketch, Solution, SolveStatus};

#[derive(Default)]
pub(crate) struct State {
    pub open: bool,
    pub hovered: Option<EntityId>,
    pub picked: Option<EntityId>,
    cache: Option<Cache>,
}

struct Cache {
    sketch: Sketch,
    solution: Solution,
    values: DimValues,
    report: Result<FreedomReport, String>,
}

impl State {
    pub fn highlights(&self) -> impl Iterator<Item = EntityId> + '_ {
        [self.hovered, self.picked]
            .into_iter()
            .flatten()
            .filter(|_| self.open)
    }

    pub fn lights(&self, id: EntityId) -> bool {
        self.highlights().any(|lit| lit == id)
    }

    fn refresh(&mut self, sketch: &Sketch, sol: &Solution, values: &DimValues) {
        if self
            .cache
            .as_ref()
            .is_none_or(|c| c.sketch != *sketch || c.solution != *sol || c.values != *values)
        {
            let report = kmb_sketch::freedom_report(sketch, sol, values).map_err(|e| e.to_string());
            self.cache = Some(Cache {
                sketch: sketch.clone(),
                solution: sol.clone(),
                values: values.clone(),
                report,
            });
        }
    }
}

pub(crate) fn panel(
    ui: &mut egui::Ui,
    state: &mut State,
    sketch: &Sketch,
    sol: &Solution,
    values: Option<&DimValues>,
) {
    ui.scope(|ui| {
        if ui.clip_rect().height() < kmb_ui::theme::fs(80.0) {
            // 狭い表示範囲では、見出しと少なくとも 1 行を読めるように余白だけを縮める。
            ui.spacing_mut().interact_size.y = 0.0;
            ui.spacing_mut().button_padding.y = kmb_ui::theme::fs(1.0);
        }
        contents(ui, state, sketch, sol, values);
    });
}

fn contents(
    ui: &mut egui::Ui,
    state: &mut State,
    sketch: &Sketch,
    sol: &Solution,
    values: Option<&DimValues>,
) {
    let previous = state.hovered;
    state.hovered = None;
    let Some(values) = values.filter(|_| !matches!(sol.status, SolveStatus::Inconsistent { .. }))
    else {
        state.picked = None;
        return;
    };
    let field =
        kmb_ui::translated_field(ui.ctx(), kmb_sketch::SKETCH, &kmb_sketch::freedom_field());
    let panel_bottom = ui.clip_rect().bottom();
    let response = egui::CollapsingHeader::new(format!("{} {}", field.label, sol.dof))
        .id_salt(kmb_ui::doc_id(ui.ctx(), "sketch-freedom-list"))
        .open(Some(state.open))
        .show(ui, |ui| {
            if !state.open {
                return;
            }
            state.refresh(sketch, sol, values);
            let report = match &state.cache.as_ref().unwrap().report {
                Ok(report) => report,
                Err(why) => {
                    state.picked = None;
                    ui.colored_label(kmb_ui::theme::look(ui.ctx()).pal.err, why);
                    return;
                }
            };
            if state
                .picked
                .is_some_and(|id| !report.entities.contains_key(&id))
            {
                state.picked = None;
            }
            let height = (panel_bottom - ui.next_widget_position().y)
                .max(0.0)
                .min(kmb_ui::theme::fs(160.0));
            // 開閉のアニメーションの途中でも、少なくとも 1 行の内容の高さを保つ。
            let row_height =
                ui.text_style_height(&egui::TextStyle::Body) + 2.0 * ui.spacing().button_padding.y;
            egui::ScrollArea::vertical()
                .min_scrolled_height(row_height)
                .max_height(height.max(row_height))
                .show(ui, |ui| {
                    for (id, dof) in &report.entities {
                        let row = ui.selectable_label(
                            state.picked == Some(*id),
                            egui::RichText::new(format!("{id}   {dof}")).monospace(),
                        );
                        if row.hovered() {
                            state.hovered = Some(*id);
                        }
                        if row.clicked() {
                            state.picked = if state.picked == Some(*id) {
                                None
                            } else {
                                Some(*id)
                            };
                            ui.ctx().request_repaint();
                        }
                    }
                });
        });
    response
        .header_response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &field.label));
    if response.header_response.clicked() {
        state.open = !state.open;
        if state.open
            && ui.clip_rect().bottom() - response.header_response.rect.bottom()
                < kmb_ui::theme::fs(60.0)
        {
            ui.scroll_to_rect(response.header_response.rect, Some(egui::Align::Min));
        }
        ui.ctx().request_repaint();
    }
    response.header_response.on_hover_text(field.help);
    if previous != state.hovered {
        ui.ctx().request_repaint();
    }
}
