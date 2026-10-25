//! 解析の結果の見せ方の画面: 時刻のコマの帯(ビューの下)、色の凡例(左上。いくつも重ねる)、
//! 解き方の札(右上)、設定欄の頭の実行の欄(様子・実行のボタン・進み具合・段・推移のグラフ・中止)。
//!
//! 中身(コマの名前、凡例、様子)は解析のモジュールが [`kmb_core::FrameBar`]・[`kmb_core::RunState`]・
//! [`kmb_core::OverlayShape`] で出し、画面は描いて操作を実行するだけ。

use crate::{Workspace, theme};
use egui::{Color32, Rect, Sense, Stroke, Vec2};
use kmb_core::{FrameBar, RunState, RunTone, Series, ShapeInk, StepMark};

/// 凡例 1 つ(見出しと、上が高い値の色と文字)。
pub(crate) type Legend = (String, Vec<(ShapeInk, String)>);

impl Workspace {
    /// 結果のコマを再生している間、時刻に合わせてコマを進める(ビューを描く前に呼ぶ)。
    pub(crate) fn frames_tick(&mut self, ctx: &egui::Context) {
        let Some((t0, f0)) = self.cur.frames_playing else {
            return;
        };
        let Some(bar) = self.frame_bar().filter(|b| b.playable) else {
            self.cur.frames_playing = None;
            return;
        };
        let now = ctx.input(|i| i.time);
        let last = bar.labels.len().saturating_sub(1);
        let f = f0 + ((now - t0) * bar.fps).floor() as usize;
        let f = if f >= last {
            self.cur.frames_playing = None;
            last
        } else {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(0.5 / bar.fps));
            f
        };
        if f != bar.at {
            self.run_quiet(&format!("{}{f}", bar.seek));
        }
    }

    /// 時刻のコマの帯を、ビュー `rect` の下に描く。
    pub(crate) fn frames_ui(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let Some(bar) = self.frame_bar() else {
            self.cur.frames_playing = None;
            return;
        };
        let l = self.win.look;
        let n = bar.labels.len();
        let last = n.saturating_sub(1);
        let w = (rect.width() - 24.0).min(720.0);
        let h = 62.0;
        let br = Rect::from_center_size(
            egui::pos2(rect.center().x, rect.bottom() - 12.0 - 32.0 - 8.0 - h / 2.0),
            egui::vec2(w, h),
        );
        let mut seek: Option<usize> = None;
        let mut toggle = false;
        let playing = self.cur.frames_playing.is_some();
        ui.scope_builder(egui::UiBuilder::new().max_rect(br), |ui| {
            egui::Frame::new()
                .fill(l.pal.panel)
                .stroke(Stroke::new(1.0, l.pal.edge))
                .corner_radius(theme::corner(6.0))
                .inner_margin(egui::Margin::symmetric(6, 3))
                .show(ui, |ui| {
                    ui.set_width(w - 12.0);
                    ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);
                    ui.horizontal(|ui| {
                        let what = if bar.playable { "時刻" } else { "コマ" };
                        let buttons: [(&str, String, Option<usize>); 5] = [
                            ("first", "最初のコマ".into(), Some(0)),
                            ("back", "前のコマ".into(), Some(bar.at.saturating_sub(1))),
                            (
                                if playing { "pause" } else { "play" },
                                if playing { "一時停止" } else { "再生" }.into(),
                                None,
                            ),
                            ("fwd", "次のコマ".into(), Some((bar.at + 1).min(last))),
                            ("last", "最後のコマ".into(), Some(last)),
                        ];
                        for (icon, name, to) in buttons {
                            let enabled = to.is_some() || bar.playable;
                            let on = to.is_none() && playing;
                            if theme::icon_button(ui, icon, &name, enabled, on)
                                .on_hover_text(name)
                                .clicked()
                            {
                                match to {
                                    Some(f) => seek = Some(f),
                                    None => toggle = true,
                                }
                            }
                        }
                        ui.add_space(8.0);
                        let now = bar.labels.get(bar.at).cloned().unwrap_or_default();
                        let text = if bar.playable {
                            format!("{now} / {}", bar.total)
                        } else {
                            now
                        };
                        ui.label(
                            egui::RichText::new(text)
                                .size(theme::fs(12.0))
                                .monospace()
                                .color(l.pal.ink),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                egui::RichText::new(format!("{what}のコマ {}/{n}", bar.at + 1))
                                    .size(theme::fs(11.0))
                                    .color(l.pal.ink3),
                            );
                        });
                    });
                    if let Some(f) = self.frame_slider(ui, &bar) {
                        seek = Some(f);
                    }
                });
        });
        if toggle {
            self.cur.frames_playing = match self.cur.frames_playing {
                Some(_) => None,
                None => {
                    // 最後にいれば最初から
                    let from = if bar.at >= last { 0 } else { bar.at };
                    if from != bar.at {
                        self.run_quiet(&format!("{}{from}", bar.seek));
                    }
                    Some((ui.input(|i| i.time), from))
                }
            };
            ui.ctx().request_repaint();
        }
        if let Some(f) = seek {
            self.cur.frames_playing = None;
            if f != bar.at {
                self.run_quiet(&format!("{}{f}", bar.seek));
            }
        }
    }

    /// コマの軸。クリックかドラッグでコマを返す。コマごとに目盛りを打つ。
    fn frame_slider(&self, ui: &mut egui::Ui, bar: &FrameBar) -> Option<usize> {
        let l = self.win.look;
        let n = bar.labels.len().max(1);
        let w = ui.available_width() - 4.0;
        let (rect, r) = ui.allocate_exact_size(egui::vec2(w, 20.0), Sense::click_and_drag());
        r.widget_info(|| egui::WidgetInfo::slider(true, bar.at as f64, "コマの軸"));
        let rect = rect.shrink2(egui::vec2(8.0, 0.0));
        let span = (n - 1).max(1) as f32;
        let x_of = |f: usize| rect.left() + f as f32 / span * rect.width();
        let p = ui.painter();
        let track = Rect::from_min_max(
            egui::pos2(rect.left(), rect.center().y - 3.0),
            egui::pos2(rect.right(), rect.center().y + 3.0),
        );
        p.rect_filled(track, theme::corner(3.0), l.pal.chrome2);
        p.rect_filled(
            Rect::from_min_max(track.min, egui::pos2(x_of(bar.at), track.max.y)),
            3.0,
            l.ws_soft(),
        );
        // コマの目盛り(多ければ間引く)
        let every = (n / 50).max(1);
        for f in (0..n).step_by(every) {
            p.vline(
                x_of(f),
                (rect.center().y + 4.0)..=(rect.center().y + 7.0),
                Stroke::new(1.0, l.pal.ink3),
            );
        }
        p.circle_filled(egui::pos2(x_of(bar.at), rect.center().y), 6.0, l.ws());
        let at_pos =
            |x: f32| (((x - rect.left()) / rect.width()).clamp(0.0, 1.0) * span).round() as usize;
        if let Some(h) = r.hover_pos() {
            let f = at_pos(h.x);
            if let Some(name) = bar.labels.get(f) {
                r.clone().on_hover_text(format!("コマ {}: {name}", f + 1));
            }
        }
        if (r.clicked() || r.dragged())
            && let Some(pos) = r.interact_pointer_pos()
        {
            return Some(at_pos(pos.x).min(n - 1));
        }
        None
    }
}

/// 凡例を左上の `top` から下へ重ねて描く(見出し付き。滑らかな塗りは色の帯、段の塗りと
/// 1 色だけの物は色の四角)。
pub(crate) fn legends_ui(
    ui: &egui::Ui,
    rect: Rect,
    top: f32,
    legends: &[Legend],
    look: &theme::Look,
) {
    let l = look;
    let p = ui.painter_at(rect);
    let mut y = top;
    for (title, items) in legends {
        if items.is_empty() {
            continue;
        }
        let row = 17.0;
        // 1 色だけの物(特異点の候補の印など)は帯にできないので四角で出す
        let banded = items.len() == 1 || items.iter().any(|(_, t)| t.contains('〜'));
        let head = if title.is_empty() { 0.0 } else { 20.0 };
        let body = if banded {
            items.len() as f32 * row
        } else {
            (items.len() as f32 - 1.0).max(1.0) * 22.0 + 14.0
        };
        let r = Rect::from_min_size(
            egui::pos2(rect.left() + 12.0, y),
            egui::vec2(200.0, head + body + 10.0),
        );
        p.rect(
            r,
            4.0,
            l.pal.panel,
            Stroke::new(1.0, l.pal.edge),
            egui::StrokeKind::Inside,
        );
        if !title.is_empty() {
            p.text(
                egui::pos2(r.left() + 8.0, r.top() + 5.0 + 8.0),
                egui::Align2::LEFT_CENTER,
                title,
                theme::prop(11.5),
                l.pal.ink,
            );
        }
        let top = r.top() + 5.0 + head;
        if banded {
            for (k, (c, name)) in items.iter().enumerate() {
                let cy = top + row * k as f32 + row / 2.0;
                let sw = Rect::from_center_size(egui::pos2(r.left() + 14.0, cy), Vec2::splat(10.0));
                p.rect_filled(sw, theme::corner(2.0), l.pal.inks.shape(*c));
                p.text(
                    egui::pos2(r.left() + 26.0, cy),
                    egui::Align2::LEFT_CENTER,
                    name,
                    theme::prop(11.0),
                    l.pal.ink2,
                );
            }
        } else {
            // 滑らかな色の帯と目盛り(上が高い値)
            let gap = 22.0;
            let y0 = top + 7.0;
            let y1 = y0 + gap * (items.len() as f32 - 1.0).max(1.0);
            let x0 = r.left() + 10.0;
            let mut mesh = egui::Mesh::default();
            for (k, (c, _)) in items.iter().enumerate() {
                let cy = y0 + gap * k as f32;
                let col = l.pal.inks.shape(*c);
                let i = mesh.vertices.len() as u32;
                mesh.colored_vertex(egui::pos2(x0, cy), col);
                mesh.colored_vertex(egui::pos2(x0 + 12.0, cy), col);
                if k > 0 {
                    mesh.add_triangle(i - 2, i - 1, i + 1);
                    mesh.add_triangle(i - 2, i + 1, i);
                }
            }
            p.add(egui::Shape::mesh(mesh));
            p.rect_stroke(
                Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x0 + 12.0, y1)),
                0.0,
                Stroke::new(1.0, l.pal.edge),
                egui::StrokeKind::Outside,
            );
            for (k, (_, name)) in items.iter().enumerate() {
                let cy = y0 + gap * k as f32;
                p.hline((x0 + 12.0)..=(x0 + 16.0), cy, Stroke::new(1.0, l.pal.ink3));
                p.text(
                    egui::pos2(x0 + 20.0, cy),
                    egui::Align2::LEFT_CENTER,
                    name,
                    theme::prop(11.0),
                    l.pal.ink2,
                );
            }
        }
        y = r.bottom() + 8.0;
    }
}

/// 解き方の札を右上の `top` に描く(「3D・格子 1.6 mm・Re 13,000」)。
pub(crate) fn badge_ui(ui: &egui::Ui, rect: Rect, top: f32, badge: &str, look: &theme::Look) {
    if badge.is_empty() {
        return;
    }
    let p = ui.painter_at(rect);
    let g = p.layout_no_wrap(badge.to_string(), theme::prop(11.0), look.pal.ink2);
    let pad = egui::vec2(8.0, 4.0);
    let r = Rect::from_min_size(
        egui::pos2(
            rect.right() - 12.0 - g.size().x - 2.0 * pad.x,
            top,
        ),
        g.size() + 2.0 * pad,
    );
    p.rect(
        r,
        4.0,
        look.pal.panel,
        Stroke::new(1.0, look.pal.edge),
        egui::StrokeKind::Inside,
    );
    p.galley(r.min + pad, g, look.pal.ink2);
}

/// 設定欄の頭に出す実行の欄。押したボタン(実行・中止)の操作の名前を返す。
pub(crate) fn run_header(ui: &mut egui::Ui, st: &RunState, look: &theme::Look) -> Option<String> {
    let l = look;
    let c = crate::actions_ui::tone_color(st.tone, &l.pal);
    let mut out = None;
    egui::Frame::new()
        .fill(c.gamma_multiply(0.06))
        .stroke(Stroke::new(1.0, c.gamma_multiply(0.45)))
        .corner_radius(theme::corner(6.0))
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                pill(ui, &st.badge, c);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&st.detail)
                            .size(theme::fs(11.0))
                            .color(l.pal.ink2),
                    )
                    .wrap(),
                );
            });
            ui.add_space(4.0);
            if st.tone == RunTone::Busy {
                if let Some(f) = st.fraction {
                    ui.add(
                        egui::ProgressBar::new(f)
                            .desired_height(8.0)
                            .fill(l.pal.sim)
                            .corner_radius(4.0),
                    );
                }
                ui.add_space(2.0);
                for (mark, name) in &st.steps {
                    let (sym, col) = match mark {
                        StepMark::Done => ("✓", l.pal.ok),
                        StepMark::Now => ("●", l.pal.sim),
                        StepMark::Todo => ("○", l.pal.ink3),
                    };
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(sym).color(col).size(theme::fs(11.0)));
                        ui.label(egui::RichText::new(name).size(theme::fs(11.0)).color(
                            if *mark == StepMark::Todo {
                                l.pal.ink3
                            } else {
                                l.pal.ink
                            },
                        ));
                    });
                }
                if let Some(s) = &st.series {
                    series_plot(ui, s, look);
                }
                if let Some((id, label)) = &st.cancel {
                    ui.add_space(4.0);
                    if ui
                        .add(
                            egui::Button::new(label)
                                .min_size(egui::vec2(ui.available_width(), 26.0)),
                        )
                        .on_hover_text("計算をやめる")
                        .clicked()
                    {
                        out = Some(id.clone());
                    }
                }
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(250));
            } else {
                let fill = if st.tone == RunTone::Done {
                    l.pal.chrome2
                } else {
                    l.pal.sim
                };
                let ink = if st.tone == RunTone::Done {
                    l.pal.ink
                } else {
                    Color32::WHITE
                };
                let b = egui::Button::new(
                    egui::RichText::new(format!("▶  {}", st.run_label))
                        .size(theme::fs(13.0))
                        .color(ink),
                )
                .fill(fill)
                .min_size(egui::vec2(ui.available_width(), 30.0));
                let r = ui.add_enabled(st.run_disabled.is_none(), b);
                let r = match &st.run_disabled {
                    Some(why) => r.on_disabled_hover_text(why),
                    None => r.on_hover_text("この研究を解く(研究は自動では解きません)"),
                };
                if r.clicked() {
                    out = Some(st.run.clone());
                }
                if let Some(s) = st.series.as_ref().filter(|s| s.points.len() > 1) {
                    egui::CollapsingHeader::new(&s.name)
                        .id_salt("run_series")
                        .default_open(false)
                        .show(ui, |ui| series_plot(ui, s, look));
                }
            }
        });
    out
}

/// 丸い札(色付きの文字と淡い地)。
fn pill(ui: &mut egui::Ui, text: &str, c: Color32) {
    let g = ui
        .painter()
        .layout_no_wrap(text.to_string(), theme::prop(11.0), c);
    let pad = egui::vec2(7.0, 2.0);
    let (rect, r) = ui.allocate_exact_size(g.size() + 2.0 * pad, Sense::hover());
    // 読み上げと、画面の部品を名前で探す操作(コマンド欄)から読めるように
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text));
    let p = ui.painter();
    p.rect_filled(rect, 8.0, c.gamma_multiply(0.14));
    p.rect_stroke(
        rect,
        8.0,
        Stroke::new(1.0, c.gamma_multiply(0.6)),
        egui::StrokeKind::Inside,
    );
    p.galley(rect.min + pad, g, c);
}

/// 推移の小さなグラフ(抗力の推移など)。
fn series_plot(ui: &mut egui::Ui, s: &Series, look: &theme::Look) {
    let l = look;
    if s.points.len() < 2 {
        return;
    }
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(&s.name)
            .size(theme::fs(11.0))
            .color(l.pal.ink2),
    );
    let w = ui.available_width();
    let (rect, r) = ui.allocate_exact_size(egui::vec2(w, 70.0), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect(
        rect,
        3.0,
        l.pal.paper,
        Stroke::new(1.0, l.pal.edge),
        egui::StrokeKind::Inside,
    );
    let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for [x, y] in &s.points {
        if !y.is_finite() {
            continue;
        }
        x0 = x0.min(*x);
        x1 = x1.max(*x);
        y0 = y0.min(*y);
        y1 = y1.max(*y);
    }
    // 始めの大きな揺れで潰れないよう、後ろ 3/4 の範囲で縦を決める
    let tail = &s.points[s.points.len() / 4..];
    let (ty0, ty1) = tail
        .iter()
        .filter(|p| p[1].is_finite())
        .fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p[1]), a.1.max(p[1])));
    if ty1 > ty0 {
        let m = 0.25 * (ty1 - ty0).max(1e-9);
        y0 = y0.max(ty0 - 2.0 * m);
        y1 = y1.min(ty1 + 2.0 * m);
    }
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let inner = rect.shrink2(egui::vec2(6.0, 8.0));
    let at = |x: f64, y: f64| {
        egui::pos2(
            inner.left() + ((x - x0) / (x1 - x0)) as f32 * inner.width(),
            inner.bottom() - (((y - y0) / (y1 - y0)).clamp(-0.05, 1.05)) as f32 * inner.height(),
        )
    };
    let pts: Vec<egui::Pos2> = s
        .points
        .iter()
        .filter(|p| p[1].is_finite())
        .map(|p| at(p[0], p[1]))
        .collect();
    p.add(egui::Shape::line(pts, Stroke::new(1.5, l.pal.sim)));
    let last = s.points[s.points.len() - 1];
    p.text(
        rect.right_top() + egui::vec2(-4.0, 3.0),
        egui::Align2::RIGHT_TOP,
        format!("{:.3}", last[1]),
        theme::prop(10.5),
        l.pal.ink2,
    );
    if !s.note.is_empty() {
        r.on_hover_text(format!("{}(横軸: {})", s.note, s.x_name));
    }
}
