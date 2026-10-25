//! シミュレーションの動きの単位の再生の帯(ビューの下)、今の様子(ビューの右上)、色の凡例。
//!
//! 再生の中身(削った形、工具の位置、様子の行)は CAM のモジュールが [`kmb_core::Playback`] で出し、
//! 画面は帯を描いて時刻の操作を実行するだけ。再生している間は、コマごとに時刻を進める。

use crate::{Workspace, theme};
use egui::{Rect, Sense, Stroke, Vec2};
use kmb_core::{LineKind, Playback};

/// 再生の速さの選び(機械の時間の何倍か)。
pub(crate) const SPEEDS: [f64; 6] = [1.0, 5.0, 10.0, 50.0, 200.0, 1000.0];

/// 秒を「分:秒.1 桁」にする。
fn clock(s: f64) -> String {
    let m = (s / 60.0).floor();
    format!("{}:{:04.1}", m as u64, s - m * 60.0)
}

impl Workspace {
    /// 今の再生(段の帯の再生を始めている時だけ)。
    pub(crate) fn playback(&self) -> Option<Playback> {
        self.stage_band()?.playback
    }

    /// 操作を実行し、空の知らせは出さない(時刻を動かす操作はコマごとに走るので)。
    pub(crate) fn run_quiet(&mut self, id: &str) {
        let r = self.run_through(id, self.cur.selected, None, &serde_json::Value::Null);
        match r {
            Ok(ran)
                if matches!(&ran.handled, kmb_session::Handled::Other(o)
                    if matches!(o.as_ref(), kmb_core::Outcome::Message(m) if m.is_empty())) => {}
            Ok(ran) => {
                self.take_ran(ran, None);
            }
            Err(e) => self.win.fail(e),
        }
    }

    /// 再生している間、前のコマからの時間に速さを掛けて時刻を進める(ビューを描く前に呼ぶ)。
    pub(crate) fn play_tick(&mut self, ctx: &egui::Context) {
        let Some(last) = self.cur.playing else {
            return;
        };
        let Some(pb) = self.playback() else {
            self.cur.playing = None;
            return;
        };
        let now = ctx.input(|i| i.time);
        // 1 コマで進める時間は 0.1 秒までにする(重いコマの後に大きく飛ばないように)
        let dt = (now - last).clamp(0.0, 0.1);
        let mut t = pb.at + dt * self.win.play_speed;
        if t >= pb.total {
            t = pb.total;
            self.cur.playing = None;
        } else {
            self.cur.playing = Some(now);
            ctx.request_repaint();
        }
        self.run_quiet(&format!("{}{t}", pb.seek));
    }

    /// 再生している時は、隠すと決めた描き方の線を除く。
    pub(crate) fn play_filter_lines(&self, lines: &mut Vec<(Vec<[f64; 3]>, LineKind)>) {
        if !self.cur.hidden_lines.is_empty() && self.playback().is_some() {
            lines.retain(|(_, k)| !self.cur.hidden_lines.contains(&k.ink));
        }
    }

    /// 再生の帯・今の様子・凡例を、ビュー `rect` の上に描く。
    pub(crate) fn playback_ui(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        legend: &[(kmb_core::ShapeInk, String)],
    ) {
        let l = self.win.look;
        let pb = self.playback();
        // 凡例(色を塗り分けた時)。左の案内の下に
        if !legend.is_empty() {
            let row = 17.0;
            let r = Rect::from_min_size(
                rect.min + egui::vec2(12.0, 80.0),
                egui::vec2(190.0, legend.len() as f32 * row + 10.0),
            );
            let p = ui.painter_at(rect);
            p.rect(
                r,
                4.0,
                l.pal.panel,
                Stroke::new(1.0, l.pal.edge),
                egui::StrokeKind::Inside,
            );
            for (k, (c, name)) in legend.iter().enumerate() {
                let y = r.top() + 5.0 + row * k as f32 + row / 2.0;
                let sw = Rect::from_center_size(egui::pos2(r.left() + 14.0, y), Vec2::splat(10.0));
                p.rect_filled(sw, theme::corner(2.0), l.pal.inks.shape(*c));
                p.text(
                    egui::pos2(r.left() + 26.0, y),
                    egui::Align2::LEFT_CENTER,
                    name,
                    crate::theme::prop(11.0),
                    l.pal.ink2,
                );
            }
        }
        let Some(pb) = pb else {
            return;
        };
        // 右上: 今の様子
        let row = 17.0;
        let ir = Rect::from_min_size(
            egui::pos2(rect.right() - 262.0, rect.top() + 44.0),
            egui::vec2(250.0, pb.info.len() as f32 * row + 10.0),
        );
        {
            let p = ui.painter_at(rect);
            p.rect(
                ir,
                4.0,
                l.pal.panel,
                Stroke::new(1.0, l.pal.edge),
                egui::StrokeKind::Inside,
            );
            for (k, (name, value)) in pb.info.iter().enumerate() {
                let y = ir.top() + 5.0 + row * k as f32 + row / 2.0;
                p.text(
                    egui::pos2(ir.left() + 8.0, y),
                    egui::Align2::LEFT_CENTER,
                    name,
                    crate::theme::prop(10.5),
                    l.pal.ink3,
                );
                p.text(
                    egui::pos2(ir.left() + 74.0, y),
                    egui::Align2::LEFT_CENTER,
                    value,
                    crate::theme::prop(11.0),
                    l.pal.ink,
                );
            }
        }
        // 下: 再生の帯(ビューの下の帯のすぐ上)
        let w = (rect.width() - 24.0).min(780.0);
        let h = 62.0;
        let br = Rect::from_center_size(
            egui::pos2(rect.center().x, rect.bottom() - 12.0 - 32.0 - 8.0 - h / 2.0),
            egui::vec2(w, h),
        );
        let mut run: Option<String> = None;
        let mut toggle_play = false;
        ui.scope_builder(egui::UiBuilder::new().max_rect(br), |ui| {
            egui::Frame::new()
                .fill(l.pal.panel)
                .stroke(Stroke::new(1.0, l.pal.edge))
                .corner_radius(crate::theme::corner(6.0))
                .inner_margin(egui::Margin::symmetric(6, 3))
                .show(ui, |ui| {
                    ui.set_width(w - 12.0);
                    ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);
                    ui.horizontal(|ui| {
                        // 前の操作・次の操作の時刻
                        let prev_op = pb
                            .marks
                            .iter()
                            .rev()
                            .map(|m| m.0)
                            .find(|t| *t < pb.at - 1e-6)
                            .unwrap_or(0.0);
                        let next_op = pb
                            .marks
                            .iter()
                            .map(|m| m.0)
                            .find(|t| *t > pb.at + 1e-6)
                            .unwrap_or(pb.total);
                        let playing = self.cur.playing.is_some();
                        let buttons: [(&str, &str, Option<String>); 7] = [
                            ("first", "先頭", Some(format!("{}0", pb.seek))),
                            ("prev", "前の操作", Some(format!("{}{prev_op}", pb.seek))),
                            ("back", "前の動き", Some(pb.prev_move.clone())),
                            (
                                if playing { "pause" } else { "play" },
                                if playing { "一時停止" } else { "再生" },
                                None,
                            ),
                            ("fwd", "次の動き", Some(pb.next_move.clone())),
                            ("next", "次の操作", Some(format!("{}{next_op}", pb.seek))),
                            ("last", "末尾", Some(format!("{}{}", pb.seek, pb.total))),
                        ];
                        for (icon, name, action) in buttons {
                            let on = action.is_none() && playing;
                            if theme::icon_button(ui, icon, name, true, on)
                                .on_hover_text(name)
                                .clicked()
                            {
                                match action {
                                    Some(a) => run = Some(a),
                                    None => toggle_play = true,
                                }
                            }
                        }
                        ui.add_space(6.0);
                        // 速さ
                        let label = format!("×{}", self.win.play_speed);
                        egui::ComboBox::from_id_salt("play_speed")
                            .width(64.0)
                            .selected_text(label)
                            .show_ui(ui, |ui| {
                                for s in SPEEDS {
                                    ui.selectable_value(
                                        &mut self.win.play_speed,
                                        s,
                                        format!("×{s}"),
                                    );
                                }
                            })
                            .response
                            .on_hover_text("再生の速さ(機械の時間の何倍か)");
                        ui.add_space(6.0);
                        // ストックの色
                        let items: Vec<(&str, bool, bool)> = pb
                            .colorings
                            .iter()
                            .map(|(n, _, on)| (n.as_str(), *on, true))
                            .collect();
                        if let Some(k) = theme::segmented(ui, &items, l.pal.panel) {
                            run = Some(pb.colorings[k].1.clone());
                        }
                        ui.add_space(6.0);
                        // 線の表示
                        for (name, ink) in &pb.line_toggles {
                            let (name, ink) = (name.as_str(), *ink);
                            let shown = !self.cur.hidden_lines.contains(&ink);
                            let c = if shown { l.ws() } else { l.pal.ink3 };
                            let r = ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new(name)
                                            .size(crate::theme::fs(11.0))
                                            .color(c),
                                    )
                                    .selected(shown)
                                    .min_size(egui::vec2(0.0, 22.0)),
                                )
                                .on_hover_text(format!("{name}の経路を出す・隠す"));
                            if r.clicked() {
                                if shown {
                                    self.cur.hidden_lines.push(ink);
                                } else {
                                    self.cur.hidden_lines.retain(|k| *k != ink);
                                }
                            }
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if theme::icon_button(ui, "x", "再生をやめる", true, false)
                                .on_hover_text("再生をやめて、部品の形に戻す")
                                .clicked()
                            {
                                run = Some(pb.stop.clone());
                            }
                        });
                    });
                    ui.horizontal(|ui| {
                        let txt = format!("{} / {}", clock(pb.at), clock(pb.total));
                        ui.add_sized(
                            egui::vec2(104.0, 20.0),
                            egui::Label::new(
                                egui::RichText::new(txt)
                                    .size(crate::theme::fs(11.5))
                                    .monospace(),
                            ),
                        );
                        if let Some(t) = self.time_slider(ui, &pb) {
                            run = Some(format!("{}{t}", pb.seek));
                        }
                    });
                });
        });
        if toggle_play {
            self.cur.playing = match self.cur.playing {
                Some(_) => None,
                None => {
                    // 末尾にいれば先頭から
                    if pb.at >= pb.total - 1e-6 {
                        self.run_quiet(&format!("{}0", pb.seek));
                    }
                    Some(ui.input(|i| i.time))
                }
            };
            ui.ctx().request_repaint();
        }
        if let Some(a) = run {
            self.run_quiet(&a);
        }
    }

    /// 時間の軸。クリックかドラッグで時刻を返す。操作の境目に目盛りを打つ。
    fn time_slider(&mut self, ui: &mut egui::Ui, pb: &Playback) -> Option<f64> {
        let l = self.win.look;
        let w = ui.available_width() - 4.0;
        let (rect, r) = ui.allocate_exact_size(egui::vec2(w, 20.0), Sense::click_and_drag());
        r.widget_info(|| egui::WidgetInfo::slider(true, pb.at, "時間の軸"));
        // 両端は丸の半径だけ空ける(先頭と末尾で丸が左の時刻の文字や枠に掛からないように)
        let rect = rect.shrink2(egui::vec2(8.0, 0.0));
        let x_of = |t: f64| rect.left() + (t / pb.total.max(1e-9)) as f32 * rect.width();
        let p = ui.painter();
        let track = Rect::from_min_max(
            egui::pos2(rect.left(), rect.center().y - 3.0),
            egui::pos2(rect.right(), rect.center().y + 3.0),
        );
        p.rect_filled(track, crate::theme::corner(3.0), l.pal.chrome2);
        p.rect_filled(
            Rect::from_min_max(track.min, egui::pos2(x_of(pb.at), track.max.y)),
            3.0,
            l.ws_soft(),
        );
        // 操作の境目
        let mut hover_name = None;
        for (k, (t, name)) in pb.marks.iter().enumerate() {
            if !t.is_finite() {
                continue;
            }
            let x = x_of(*t);
            p.vline(
                x,
                (rect.top() + 2.0)..=(rect.bottom() - 2.0),
                Stroke::new(1.0, l.pal.ink3),
            );
            let end = pb.marks.get(k + 1).map_or(pb.total, |m| m.0);
            if let Some(h) = r.hover_pos()
                && h.x >= x
                && h.x < x_of(end)
            {
                hover_name = Some(format!("{} {}", k + 1, name));
            }
        }
        p.circle_filled(egui::pos2(x_of(pb.at), rect.center().y), 6.0, l.ws());
        if let Some(n) = hover_name {
            r.clone().on_hover_text(n);
        }
        if (r.clicked() || r.dragged())
            && let Some(pos) = r.interact_pointer_pos()
        {
            let f = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64;
            return Some(f * pb.total);
        }
        None
    }
}
