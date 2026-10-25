//! 引いて動かした後の「位置を残す/元に戻す」: ビューの上の帯と、決めずに別のコマンドを始めた時に
//! 聞く窓。

use super::Held;
use crate::Workspace;
use egui::Rect;

impl Workspace {
    /// 残すか戻すかを決めていない変更の後に、ほかの変更(取り消し・直す)をしていたら、残したとみなす。
    pub(super) fn moved_check(&mut self) {
        if let Some(m) = &self.cur.moved
            && self.cur.work.doc().undo_depth() != m.top
        {
            if self.cur.work.doc().undo_depth() < m.top {
                log::info!("引いて動かした位置を決めずに取り消したので、帯を消す");
            } else {
                log::info!("引いて動かした位置を残した(ほかの変更をした)");
            }
            self.cur.moved = None;
        }
    }

    /// 引いて動かした位置を決めていないか。
    pub fn moved_pending(&mut self) -> bool {
        self.moved_check();
        self.cur.moved.is_some()
    }

    /// 引いて動かした位置を残す(`keep`)か、引く前に戻す。
    pub fn settle_moved(&mut self, keep: bool) {
        self.moved_check();
        let Some(m) = self.cur.moved.take() else {
            return;
        };
        if keep {
            log::info!("引いて動かした位置を残す({})", m.label);
            return;
        }
        let n = self.cur.work.rollback_to(m.depth);
        log::info!("引いて動かした位置を戻す({}、{n} 手)", m.label);
        self.supersede_job();
        self.after_change(Ok(()));
    }

    /// コマンドを始める前に呼ぶ。引いて動かした位置を決めていなければ、コマンド `held` を止めて
    /// 残すか戻すかを窓で聞き、真を返す(答えた後にそのコマンドを始める)。
    pub(crate) fn hold_for_moved(&mut self, held: Held) -> bool {
        if !self.moved_pending() {
            return false;
        }
        log::info!("引いて動かした位置を決めていないので、{held:?} の前に聞く");
        self.cur.moved_ask = Some(held);
        true
    }

    /// 聞いている窓に答える(`None` はやめる: コマンドを始めず、位置も決めない)。
    pub fn answer_moved(&mut self, keep: Option<bool>) {
        let Some(held) = self.cur.moved_ask.take() else {
            return;
        };
        let Some(keep) = keep else {
            log::info!("{held:?} をやめた(位置は決めていないまま)");
            return;
        };
        self.settle_moved(keep);
        match held {
            Held::Action(id) => {
                self.start_action(&id);
            }
            Held::ActionWith(id, args) => {
                self.start_action_with(&id, args);
            }
            Held::Add(t) => {
                self.add_described(&t);
            }
            Held::Edit(id) => self.open_edit(id),
        }
    }

    /// ビューの上の「位置を残す/元に戻す」の帯と、コマンドの前に聞く窓。
    pub(crate) fn moved_ui(&mut self, ui: &egui::Ui, rect: Rect) {
        if !self.moved_pending() {
            self.cur.moved_ask = None;
            return;
        }
        let l = self.win.look;
        let label = self
            .cur
            .moved
            .as_ref()
            .map(|m| m.label.clone())
            .unwrap_or_default();
        let mut choice: Option<bool> = None;
        // 選択表示中の帯の下に出す(重ならない)
        let top = rect.top()
            + if !self.cur.isolated.is_empty() {
                44.0
            } else {
                8.0
            };
        egui::Area::new(egui::Id::new("moved_banner"))
            .fixed_pos(egui::pos2(rect.center().x - 170.0, top))
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::new()
                    .fill(crate::theme::mix(l.pal.panel, l.ws(), 0.12))
                    .stroke(egui::Stroke::new(1.0, l.ws()))
                    .corner_radius(crate::theme::corner(6.0))
                    .inner_margin(egui::Margin::symmetric(10, 4))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!("位置: {label}")).color(l.pal.ink),
                            );
                            if ui
                                .button("位置を残す")
                                .on_hover_text("引いて動かした位置のまま決める(タイムラインに残る)")
                                .clicked()
                            {
                                choice = Some(true);
                            }
                            if ui
                                .button("元に戻す")
                                .on_hover_text("引く前の位置に戻す")
                                .clicked()
                            {
                                choice = Some(false);
                            }
                        });
                    });
            });
        if let Some(keep) = choice {
            self.cur.moved_ask = None;
            self.settle_moved(keep);
            return;
        }
        if self.cur.moved_ask.is_none() {
            return;
        }
        let mut answer: Option<Option<bool>> = None;
        egui::Window::new("動かした位置")
            .id(egui::Id::new("moved_ask"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                ui.label("部品を引いて動かした位置を、まだ決めていません。");
                ui.label("動かした位置を残しますか、引く前の位置に戻しますか?");
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("元に戻す").clicked() {
                        answer = Some(Some(false));
                    }
                    if ui.button("位置を残す").clicked() {
                        answer = Some(Some(true));
                    }
                    if ui
                        .button("キャンセル")
                        .on_hover_text("コマンドを始めない")
                        .clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        answer = Some(None);
                    }
                });
            });
        if let Some(a) = answer {
            self.answer_moved(a);
        }
    }
}
