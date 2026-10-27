//! 機能の注意・エラーを確かめる窓。タイムラインやブラウザの右クリック「エラーを確かめる」
//! で開き、ビューの右下に知らせの全文と直し方の手がかり、
//! 編集・文を写す・原因の機能を選ぶボタンを出す。

use crate::Workspace;
use crate::timeline_ui::{Health, paint_health};
use egui::{Rect, Vec2};
use kmb_core::FeatureId;

/// 確かめる窓の状態。
pub struct Review {
    feature: FeatureId,
    /// 「写した」と出す時刻(秒)
    copied: Option<f64>,
}

impl Workspace {
    /// 確かめる窓を開く(測る道具とは場所が重なるので、測る道具は閉じる)。
    pub(crate) fn open_review(&mut self, id: FeatureId) {
        self.cur.measure = None;
        self.cur.review = Some(Review {
            feature: id,
            copied: None,
        });
        if self.cur.selected != Some(id) {
            self.select(id);
        }
    }

    /// 写すための文(機能の名前、種類、知らせ)。
    pub(crate) fn review_text(&self, id: FeatureId) -> String {
        let name = self
            .cur
            .work
            .doc()
            .data()
            .feature(id)
            .map_or(String::new(), |f| f.name.clone());
        let h = self.health(id);
        let w = self.win.words();
        let mut s = format!("{name}({}): {}", h.title(&w), h.message(&w));
        if let Some(raw) = self.raw_text(id) {
            s += "\n";
            s += &self.win.t_with("review-raw-line", &[("raw", raw.into())]);
        }
        s
    }

    /// ビューの右下の窓。
    pub(crate) fn review_panel(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let Some(rv) = &self.cur.review else { return };
        let id = rv.feature;
        let Some(f) = self.cur.work.doc().data().feature(id).cloned() else {
            self.cur.review = None;
            return;
        };
        let health = self.health(id);
        let raw = self.raw_text(id);
        let own = self
            .win
            .registry
            .descriptor(&f.type_id)
            .map(|d| d.fixes)
            .unwrap_or_default();
        let fixes = crate::plain_error::fixes(&own, &health);
        let copied = rv.copied.is_some_and(|t| ui.input(|i| i.time) - t < 2.0);
        let upstream = match &health {
            Health::Blocked { feature, .. } => *feature,
            _ => None,
        };
        let upstream_name = upstream
            .and_then(|u| self.cur.work.doc().data().feature(u))
            .map(|u| u.name.clone());
        let l = self.win.look;
        let words = self.win.words();
        let (mut close, mut edit, mut copy, mut pick_up) = (false, false, false, false);
        let screen = ui.ctx().content_rect();
        let offset = egui::vec2(
            rect.right() - screen.right() - 10.0,
            rect.bottom() - screen.bottom() - 56.0,
        );
        let accent = if health.is_problem() {
            health.color(&l)
        } else {
            l.pal.ok
        };
        egui::Area::new(egui::Id::new("review_panel"))
            .anchor(egui::Align2::RIGHT_BOTTOM, offset)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::new()
                    .fill(l.pal.panel)
                    .stroke(egui::Stroke::new(1.0, accent))
                    .corner_radius(crate::theme::corner(6.0))
                    .inner_margin(egui::Margin::same(10))
                    .show(ui, |ui| {
                        ui.set_width(320.0);
                        ui.horizontal(|ui| {
                            let (r, _) =
                                ui.allocate_exact_size(Vec2::splat(14.0), egui::Sense::hover());
                            paint_health(ui.painter(), r.center(), &health, &l, false);
                            let title = if health.is_problem() {
                                health.title(&words)
                            } else {
                                words.t("review-ok")
                            };
                            ui.label(egui::RichText::new(title).strong().color(accent));
                            ui.label(egui::RichText::new(&f.name).strong());
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .small_button(words.t("review-close"))
                                        .on_hover_text(words.t("review-close-tip"))
                                        .clicked()
                                    {
                                        close = true;
                                    }
                                },
                            );
                        });
                        ui.add_space(4.0);
                        if health.is_problem() {
                            ui.label(
                                egui::RichText::new(crate::plain_error::headline(&health))
                                    .strong()
                                    .color(l.pal.ink),
                            );
                            // 全文(選んで写せる)
                            egui::ScrollArea::vertical()
                                .max_height(160.0)
                                .show(ui, |ui| {
                                    ui.add(
                                        egui::Label::new(health.message(&words))
                                            .wrap()
                                            .selectable(true),
                                    );
                                });
                            if let Some(n) = &upstream_name {
                                ui.label(
                                    egui::RichText::new(words.t_with(
                                        "review-blocked-by",
                                        &[("name", n.as_str().into())],
                                    ))
                                    .color(l.pal.ink2),
                                );
                            }
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(words.t("review-fixes"))
                                    .size(crate::theme::fs(11.0))
                                    .color(l.pal.ink2),
                            );
                            for fix in &fixes {
                                ui.label(
                                    egui::RichText::new(format!("・{fix}"))
                                        .size(crate::theme::fs(11.0))
                                        .color(l.pal.ink2),
                                );
                            }
                            if let Some(raw) = &raw {
                                // 内部の言葉を残した元の文(不具合の報告に使う)
                                egui::CollapsingHeader::new(
                                    egui::RichText::new(words.t("review-raw"))
                                        .size(crate::theme::fs(11.0))
                                        .color(l.pal.ink3),
                                )
                                .id_salt(("review_raw", id))
                                .show(ui, |ui| {
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(raw)
                                                .size(crate::theme::fs(11.0))
                                                .color(l.pal.ink3),
                                        )
                                        .wrap()
                                        .selectable(true),
                                    );
                                });
                            }
                        } else {
                            ui.label(
                                egui::RichText::new(words.t("review-no-problem")).color(l.pal.ink2),
                            );
                        }
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(
                                    !self.is_editing(id),
                                    egui::Button::new(words.t("review-edit")),
                                )
                                .clicked()
                            {
                                edit = true;
                            }
                            if ui
                                .add_enabled(
                                    health.is_problem(),
                                    egui::Button::new(words.t("review-copy")),
                                )
                                .on_hover_text(words.t("review-copy-tip"))
                                .clicked()
                            {
                                copy = true;
                            }
                            if upstream.is_some()
                                && ui.button(words.t("review-pick-cause")).clicked()
                            {
                                pick_up = true;
                            }
                            if copied {
                                ui.label(
                                    egui::RichText::new(words.t("review-copied")).color(l.ws()),
                                );
                            }
                        });
                    });
            });
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) && !ui.ctx().egui_wants_keyboard_input() {
            close = true;
        }
        if copy {
            ui.ctx().copy_text(self.review_text(id));
            if let Some(rv) = &mut self.cur.review {
                rv.copied = Some(ui.input(|i| i.time));
            }
        }
        if edit {
            self.open_edit(id);
        }
        if pick_up && let Some(u) = upstream {
            self.select(u);
            self.cur.review = Some(Review {
                feature: u,
                copied: None,
            });
        }
        if close {
            self.cur.review = None;
        }
    }
}

impl Workspace {
    /// ビューの右下の角の印。注意・エラーのある機能があれば
    /// 数を出し、押すと確かめる窓で最初の機能を開く(開いている時は次の機能へ進む)。
    pub(crate) fn problem_badge(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let problems = self.problems();
        if problems.is_empty() {
            return;
        }
        let errors = problems.iter().filter(|(_, h)| h.is_red()).count();
        let warnings = problems.len() - errors;
        let worst = problems[0].1.clone();
        let l = self.win.look;
        let text = self.win.t_with(
            "review-badge",
            &[("errors", errors.into()), ("warnings", warnings.into())],
        );
        let tip = self
            .win
            .t_with("review-badge-tip", &[("badge", text.as_str().into())]);
        let screen = ui.ctx().content_rect();
        let offset = egui::vec2(
            rect.right() - screen.right() - 10.0,
            rect.bottom() - screen.bottom() - 12.0,
        );
        let mut clicked = false;
        egui::Area::new(egui::Id::new("problem_badge"))
            .anchor(egui::Align2::RIGHT_BOTTOM, offset)
            .order(egui::Order::Middle)
            .show(ui.ctx(), |ui| {
                let galley = ui.painter().layout_no_wrap(
                    text.clone(),
                    crate::theme::prop(12.0),
                    worst.color(&l),
                );
                let size = egui::vec2(galley.size().x + 30.0, 22.0);
                let (r, resp) = ui.allocate_exact_size(size, egui::Sense::click());
                let p = ui.painter();
                let hot = resp.hovered();
                p.rect(
                    r,
                    11.0,
                    if hot { l.pal.chrome2 } else { l.pal.panel },
                    egui::Stroke::new(1.0, worst.color(&l)),
                    egui::StrokeKind::Inside,
                );
                paint_health(
                    p,
                    egui::pos2(r.left() + 12.0, r.center().y),
                    &worst,
                    &l,
                    false,
                );
                p.galley(
                    egui::pos2(r.left() + 22.0, r.center().y - galley.size().y / 2.0),
                    galley,
                    worst.color(&l),
                );
                let resp = resp.on_hover_text(tip.as_str());
                if resp.clicked() {
                    clicked = true;
                }
            });
        if clicked {
            let now = self.cur.review.as_ref().map(|r| r.feature);
            let next = now
                .and_then(|n| problems.iter().position(|(id, _)| *id == n))
                .map_or(0, |k| (k + 1) % problems.len());
            self.open_review(problems[next].0);
        }
    }
}
