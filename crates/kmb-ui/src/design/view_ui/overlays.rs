//! 3D のビューの上に重ねる部品: 視点のボタン・軸の玉・下の帯(選ぶ・回す・全体表示・正面・表示 ▾)・ブラウザのつまみ・表示スタイルの項目。

use super::*;

/// ビューの上の部品が使った所(上からの下の端の y)。
pub(super) struct Taken {
    /// 左上の案内の下の端(案内が無ければビューの上の端)
    pub left: f32,
    /// 右上の軸の玉の下の端
    pub right: f32,
}

impl Workspace {
    /// ビューの上に重ねる部品(試作の `.ctxhint`・`.viewbtns`・`.navbar`)。
    /// 選ぶ種類はツールバーの「選択」だけ、表示の形と投影は下の帯の「表示 ▾」だけに置く
    /// (ビューの左上・右上に同じ物を重ねて置かない)。
    /// `shape` は加工の途中の形を出している時の名前(段の帯で選んだ段)。
    /// 返すのは左上の案内と右上の軸の玉の下の端(解析の凡例と札をその下に置き、隠さない)。
    pub(super) fn view_overlays(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        shape: Option<String>,
    ) -> Taken {
        use crate::theme;
        let mut taken = Taken {
            left: rect.top(),
            right: rect.top(),
        };
        let l = self.win.look;
        let at = |ui: &mut egui::Ui, r: Rect, f: &mut dyn FnMut(&mut egui::Ui)| {
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(r)
                    .layout(egui::Layout::left_to_right(egui::Align::Min)),
                |ui| f(ui),
            );
        };
        // ブラウザを隠した時の左の端のつまみ(字の倍率で幅が変わる)。左上の案内はその右から置く
        let grip = self.browser_grip_size();
        let left = if self.win.layout.browser_open {
            12.0
        } else {
            grip.x + 8.0
        };
        // 左上: 今の案内(クリックで作る流れ、選んだ面)
        let hint = match (&self.cur.gesture, &self.cur.picked) {
            (Some(g), _) => Some((
                g.label.clone(),
                g.hint()
                    .trim_start_matches(&format!("{}: ", g.label))
                    .to_string(),
                true,
            )),
            (None, _) if self.cur.measure.is_some() => None,
            // 頂点・辺(と面)を混ぜて選んだ時は、種類ごとの数を並べる
            (None, picked)
                if !self.cur.picked_edges.is_empty() || !self.cur.picked_vertices.is_empty() =>
            {
                // 下の帯と同じく頂点・辺・面の順
                let count = |id: &str, n: usize| self.win.t_with(id, &[("count", n.into())]);
                let mut parts = Vec::new();
                if !self.cur.picked_vertices.is_empty() {
                    parts.push(count("view-hint-vertices", self.cur.picked_vertices.len()));
                }
                if !self.cur.picked_edges.is_empty() {
                    parts.push(count("view-hint-edges", self.cur.picked_edges.len()));
                }
                if picked.is_some() {
                    parts.push(count("view-hint-faces", self.cur.also_picked.len() + 1));
                }
                let parts = parts.join(&self.win.t("view-hint-separator"));
                Some((
                    self.win.t("view-hint-select"),
                    self.win
                        .t_with("view-hint-mixed", &[("parts", parts.into())]),
                    false,
                ))
            }
            (None, Some(p)) => {
                let n = self.cur.also_picked.len() + 1;
                let face = self.face_label(&p.face);
                let text = if n > 1 {
                    self.win.t_with(
                        "view-hint-faces-many",
                        &[("count", n.into()), ("face", face.into())],
                    )
                } else {
                    self.win
                        .t_with("view-hint-face-one", &[("face", face.into())])
                };
                let head = self.win.t_with("view-hint-faces", &[("count", n.into())]);
                Some((head, text, false))
            }
            (None, None) => {
                // 加工は削る途中の形、解析は結果の色分け
                let head = self.win.t(
                    if self.win.layout.space == crate::layout::Space::Simulation {
                        "view-hint-result"
                    } else {
                        "view-hint-stage-shape"
                    },
                );
                shape.map(|s| (head, s, false))
            }
        };
        if let Some((head, text, cmd)) = hint {
            let pos = rect.min + egui::vec2(left, 10.0);
            let mut job = egui::text::LayoutJob::default();
            if !head.is_empty() {
                job.append(
                    &head,
                    0.0,
                    egui::TextFormat::simple(crate::theme::prop(11.5), l.ws()),
                );
            }
            job.append(
                &text,
                if head.is_empty() { 0.0 } else { 6.0 },
                egui::TextFormat::simple(
                    crate::theme::prop(11.5),
                    if cmd { l.pal.ink } else { l.pal.ink2 },
                ),
            );
            job.wrap.max_width = rect.width() - left - 28.0;
            let g = ui.fonts_mut(|f| f.layout_job(job));
            let r = Rect::from_min_size(pos, g.size() + egui::vec2(16.0, 6.0));
            taken.left = r.bottom();
            let p = ui.painter_at(rect);
            p.rect(
                r,
                4.0,
                l.pal.panel,
                egui::Stroke::new(1.0, if cmd { l.ws() } else { l.pal.edge }),
                egui::StrokeKind::Inside,
            );
            p.galley(r.min + egui::vec2(8.0, 3.0), g, l.pal.ink);
            // 画面の部品の木にも出す(コマンド欄の expect と MCP の ui_find で今の手順を確かめる)。
            // マウスを乗せるだけの部品で、クリックは下の 3D のビューへ通す
            let name = format!("{head} {text}");
            ui.interact(r, ui.id().with("view-hint"), egui::Sense::hover())
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &name));
        }
        // 右上: 等角と全画面のボタン。その下に視点の軸の玉
        // (押して向きを変える立方体は作らない。docs/避ける形.md)
        let mut view = None;
        // ボタンの行の高さ(icon_button と segmented の高い方。1 倍で 26)
        let row_h = (26.0 * theme::band()).max(theme::fs(24.0)) + 2.0;
        at(
            ui,
            Rect::from_min_max(
                egui::pos2(rect.right() - 230.0 * theme::band(), rect.top() + 10.0),
                egui::pos2(rect.right() - 10.0, rect.top() + 10.0 + row_h),
            ),
            &mut |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    let focus = self.win.focus_mode.is_some();
                    let name = self.win.t(if focus {
                        "view-fullscreen-exit"
                    } else {
                        "view-fullscreen"
                    });
                    let fr = egui::Frame::new()
                        .fill(l.pal.panel)
                        .stroke(egui::Stroke::new(1.0, l.pal.edge))
                        .corner_radius(crate::theme::corner(5.0))
                        .show(ui, |ui| theme::icon_button(ui, "fit", &name, true, focus))
                        .inner;
                    if fr.on_hover_text(name).clicked() {
                        view = Some(9);
                    }
                    ui.add_space(4.0);
                    // 上・前・右は下の軸の玉で選ぶ。等角だけボタンで残す
                    // (もう一度押すと反対側から見る。その間は名前も「等角(後)」に変わる)
                    let b = view3d::VIEW_BUTTONS[3];
                    // 名前は表(view3d の VIEW_BUTTONS)の日本語ではなく今の言語で出す
                    let (iso, iso_back) = (self.win.t("view-iso"), self.win.t("view-iso-back"));
                    let item = match self.cur.camera.view_side(&b) {
                        Some(true) => (iso_back.as_str(), true, true),
                        side => (iso.as_str(), side.is_some(), true),
                    };
                    if theme::segmented(ui, &[item], l.pal.panel).is_some() {
                        view = Some(3);
                    }
                });
            },
        );
        // 軸の玉はボタンの行の下に置く(上からの固定の位置にすると、字とアイコンを大きくした時に
        // 全画面のボタンに重なる)
        let gizmo = egui::pos2(
            rect.right() - 14.0 - view3d::GIZMO_RADIUS,
            rect.top() + 10.0 + row_h + 4.0 + view3d::GIZMO_RADIUS,
        );
        view3d::axis_gizmo(ui, &mut self.cur.camera, gizmo, l.pal.panel);
        taken.right = gizmo.y + view3d::GIZMO_RADIUS;
        match view {
            Some(i @ 0..=3) => {
                self.cur.camera.press_view(&view3d::VIEW_BUTTONS[i]);
                ui.ctx().request_repaint();
            }
            Some(9) => self.toggle_focus_mode(),
            _ => {}
        }
        // 下の真ん中: 左ドラッグの働き、全体、正面から見る、表示
        // 幅は前のフレームで並べた幅(字とアイコンの倍率で変わるので倍率ごとに覚える)。名前つきで
        // ビューに入らなければアイコンだけにする。左下の格子の札に重なるなら右へ寄せ、寄せても
        // 入らなければ札の上へ上げる
        let band_key = (theme::band() * 100.0).round() as i32;
        let full_w = ui.id().with(("view_bar_full", band_key));
        let bar_w = ui.id().with(("view_bar_w", band_key));
        let full = ui
            .data(|d| d.get_temp::<f32>(full_w))
            .unwrap_or(400.0 * theme::band());
        let icon_only = full > rect.width() - 24.0;
        let w = if icon_only {
            ui.data(|d| d.get_temp::<f32>(bar_w)).unwrap_or(full)
        } else {
            full
        };
        let h = 32.0 * theme::band();
        let mut left = rect.center().x - w / 2.0;
        let mut bottom = rect.bottom() - 12.0;
        if self.win.layout.grid {
            let g = ui.painter().layout_no_wrap(
                view3d::grid_label(self.cur.camera.zoom),
                crate::theme::prop(10.5),
                l.pal.ink3,
            );
            let free = rect.left() + 12.0 + g.size().x + 12.0;
            if left < free {
                if free + w <= rect.right() - 12.0 {
                    left = free;
                } else {
                    bottom = rect.bottom() - 8.0 - g.size().y - 6.0;
                }
            }
        }
        let nr = Rect::from_min_size(
            egui::pos2(left.max(rect.left() + 4.0), bottom - h),
            egui::vec2(w.max(1.0), h),
        );
        let bar = egui::Frame::new()
            .fill(l.pal.panel)
            .stroke(egui::Stroke::new(1.0, l.pal.edge))
            .corner_radius(crate::theme::corner(6.0))
            .inner_margin(egui::Margin::same(3));
        let mut act = None;
        let mut display = None;
        let shown = ui.scope_builder(egui::UiBuilder::new().max_rect(nr), |ui| {
            bar.show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                ui.horizontal(|ui| {
                    use view3d::LeftDrag;
                    for (icon, id, mode) in [
                        ("orbit", "view-orbit", LeftDrag::Orbit),
                        ("pan", "view-pan", LeftDrag::Pan),
                        ("zoom", "view-zoom", LeftDrag::Zoom),
                    ] {
                        let on = self.win.left_drag == mode;
                        let c = if on { l.ws() } else { l.pal.ink2 };
                        let tip = if mode == LeftDrag::Orbit {
                            let how = self.win.t(if self.win.layout.free_orbit {
                                "view-orbit-free"
                            } else {
                                "view-orbit-constrained"
                            });
                            self.win.t_with("view-orbit-tip", &[("how", how.into())])
                        } else {
                            self.win.t(&format!("{id}-tip"))
                        };
                        let text = self.win.t(id);
                        let r = theme::icon_text_button_fit(ui, icon, &text, c, 11.5, icon_only)
                            .on_hover_text(tip);
                        if r.clicked() {
                            act = Some(if on { LeftDrag::Select } else { mode });
                        }
                        // オービットの右クリックで回し方を選ぶ
                        if mode == LeftDrag::Orbit {
                            let (constrained, free) = (
                                self.win.t("view-orbit-constrained-item"),
                                self.win.t("view-orbit-free"),
                            );
                            r.right_menu(|ui| {
                                ui.radio_value(&mut self.win.layout.free_orbit, false, constrained);
                                ui.radio_value(&mut self.win.layout.free_orbit, true, free);
                            });
                        }
                    }
                    if theme::icon_text_button_fit(
                        ui,
                        "fit",
                        &self.win.t("view-fit"),
                        l.pal.ink2,
                        11.5,
                        icon_only,
                    )
                    .on_hover_text(self.win.t("view-fit-tip"))
                    .clicked()
                    {
                        self.cur.camera.refit();
                    }
                    // 選んだ面に正対する。面を選んでいなければ理由を出す
                    let why = self.look_at_target().err();
                    let c = if why.is_none() {
                        l.pal.ink2
                    } else {
                        l.pal.ink3
                    };
                    let r = theme::icon_text_button_fit(
                        ui,
                        "lookat",
                        &self.win.t("view-look-at"),
                        c,
                        11.5,
                        icon_only,
                    );
                    let r = match why {
                        None => r.on_hover_text(self.win.t("view-look-at-tip")),
                        Some(w) => r.on_hover_text(
                            self.win.t_with("view-look-at-why", &[("why", w.into())]),
                        ),
                    };
                    if r.clicked() {
                        self.look_at_selected();
                        ui.ctx().request_repaint();
                    }
                    let r = theme::icon_text_button_fit(
                        ui,
                        "display",
                        &self.win.t("view-display"),
                        l.pal.ink2,
                        11.5,
                        icon_only,
                    )
                    .on_hover_text(self.win.t("view-display-tip"));
                    display = Some(r);
                });
            })
            .response
            .rect
            .width()
        });
        if (shown.inner - w).abs() > 0.5 {
            let id = if icon_only { bar_w } else { full_w };
            ui.data_mut(|d| d.insert_temp(id, shown.inner));
            ui.ctx().request_repaint();
        }
        if let Some(m) = act {
            self.win.left_drag = m;
        }
        if let Some(r) = display {
            egui::Popup::menu(&r).show(|ui| {
                let t = |id: &str| self.win.t(id);
                let (data, data_tip, browser, timeline, timeline_off) = (
                    t("view-menu-data-panel"),
                    t("view-menu-data-panel-tip"),
                    t("view-menu-browser"),
                    t("view-menu-timeline"),
                    t("view-menu-timeline-off"),
                );
                let (always, always_tip, commands, commands_tip, reset, reset_tip) = (
                    t("view-menu-panel-always"),
                    t("view-menu-panel-always-tip"),
                    t("view-menu-commands"),
                    t("view-menu-commands-tip"),
                    t("view-menu-reset-dock"),
                    t("view-menu-reset-dock-tip"),
                );
                ui.label(
                    egui::RichText::new(t("view-menu-panels"))
                        .weak()
                        .size(crate::theme::fs(11.0)),
                );
                crate::keycast::command_response(
                    ui.checkbox(&mut self.win.layout.data_open, &data),
                    data,
                )
                .on_hover_text(data_tip);
                crate::keycast::command_response(
                    ui.checkbox(&mut self.win.layout.browser_open, &browser),
                    browser,
                );
                // レンダリングと解析では下の帯を出さない(dock_ui の Pane::in_space)
                let here = crate::dock_ui::Pane::Timeline.in_space(self.win.layout.space);
                crate::keycast::command_response(
                    ui.add_enabled(
                        here,
                        egui::Checkbox::new(&mut self.win.layout.timeline_open, &timeline),
                    ),
                    timeline,
                )
                .on_disabled_hover_text(timeline_off);
                crate::keycast::command_response(
                    ui.checkbox(&mut self.win.layout.panel_always, &always),
                    always,
                )
                .on_hover_text(always_tip);
                crate::keycast::command_response(
                    ui.checkbox(&mut self.win.layout.commands_open, &commands),
                    commands,
                )
                .on_hover_text(commands_tip);
                if crate::keycast::command_response(
                    ui.add_enabled(!self.dock_is_default(), egui::Button::new(&reset)),
                    reset,
                )
                .on_hover_text(reset_tip)
                .clicked()
                {
                    self.reset_dock();
                    ui.close();
                }
                ui.separator();
                self.view_style_items(ui);
            });
        }
        // ブラウザを隠している時は、左の端に戻すつまみ
        if !self.win.layout.browser_open {
            let r = Rect::from_min_size(egui::pos2(rect.left(), rect.top() + 10.0), grip);
            let resp = ui
                .interact(r, ui.id().with("browser_reopen"), egui::Sense::click())
                .on_hover_text(self.win.t("view-grip-tip"));
            let name = self.win.t("view-grip-tip");
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
            let p = ui.painter();
            p.rect(
                r,
                egui::CornerRadius {
                    nw: 0,
                    sw: 0,
                    ne: 5,
                    se: 5,
                },
                l.pal.panel,
                egui::Stroke::new(1.0, l.pal.edge),
                egui::StrokeKind::Inside,
            );
            let step = Self::grip_step();
            for (k, ch) in self.win.t("view-grip").chars().enumerate() {
                p.text(
                    egui::pos2(r.center().x, r.top() + 4.0 + step * (k as f32 + 0.5)),
                    egui::Align2::CENTER_CENTER,
                    ch,
                    crate::theme::prop(11.0),
                    l.pal.ink2,
                );
            }
            if resp.clicked() {
                self.win.layout.browser_open = true;
            }
        }
        taken
    }

    /// ブラウザを隠した時の左の端のつまみの 1 字の送り(字の倍率に合わせる。1 倍で 15)。
    pub(super) fn grip_step() -> f32 {
        15.0 * crate::theme::scale().text.max(1.0)
    }

    /// ブラウザを隠した時の左の端のつまみの大きさ(1 倍で 20×66)。字の数と倍率から決める
    /// (固定の大きさだと、字の倍率 1.4 以上で字が重なってつまみからはみ出す)。
    pub(crate) fn browser_grip_size(&self) -> egui::Vec2 {
        let n = self.win.t("view-grip").chars().count() as f32;
        egui::vec2(
            20.0_f32.max(crate::theme::fs(11.0) + 8.0),
            66.0_f32.max(Self::grip_step() * n + 8.0),
        )
    }

    /// 表示の形(陰影と辺・陰影と隠れ線・陰影だけ・線だけ)と投影(平行・透視・自動)、格子と原点を選ぶ項目。
    /// 下の帯の「表示」、何もない所の右クリック、設定の窓で使う。
    pub(crate) fn view_style_items(&mut self, ui: &mut egui::Ui) {
        let head = |ui: &mut egui::Ui, text: String| {
            ui.label(
                egui::RichText::new(text)
                    .weak()
                    .size(crate::theme::fs(11.0)),
            );
        };
        head(ui, self.win.t("view-style"));
        self.shading_items(ui);
        ui.separator();
        head(ui, self.win.t("view-projection"));
        self.projection_items(ui);
        ui.separator();
        head(ui, self.win.t("view-guides"));
        let t = |id: &str| self.win.t(id);
        let (grid, grid_tip, origin, origin_tip, see, see_tip) = (
            t("view-grid"),
            t("view-grid-tip"),
            t("view-origin"),
            t("view-origin-tip"),
            t("view-see-through"),
            t("view-see-through-tip"),
        );
        crate::keycast::command_response(ui.checkbox(&mut self.win.layout.grid, &grid), grid)
            .on_hover_text(grid_tip);
        crate::keycast::command_response(ui.checkbox(&mut self.win.layout.origin, &origin), origin)
            .on_hover_text(origin_tip);
        crate::keycast::command_response(ui.checkbox(&mut self.win.layout.see_through, &see), see)
            .on_hover_text(see_tip);
        let mut hues = self.cur.work.doc_view().part_hues;
        let label = self.win.t("view-part-hues");
        let tip = self.win.t("view-part-hues-tip");
        if crate::keycast::command_response(ui.checkbox(&mut hues, &label), label)
            .on_hover_text(tip)
            .changed()
        {
            let result = self.run_through(
                kmb_session::doc_view::PART_HUES,
                self.cur.selected,
                None,
                &serde_json::json!({"on":hues}),
            );
            match result {
                Ok(ran) => {
                    self.take_ran(ran, None);
                }
                Err(why) => self.win.fail(why),
            }
        }
        ui.separator();
        head(ui, self.win.t("view-effects"));
        self.effects_items(ui);
    }

    /// 表示の形の選び(今の言語の名前で)。表示のメニューと設定の窓で使う。
    pub(crate) fn shading_items(&mut self, ui: &mut egui::Ui) {
        for s in crate::Shading::ALL {
            let name = self.win.t(shading_id(s));
            crate::keycast::command_response(
                ui.radio_value(&mut self.win.layout.shading, s, &name),
                name,
            );
        }
    }

    /// 投影の選び(今の言語の名前と説明で)。表示のメニューと設定の窓で使う。
    pub(crate) fn projection_items(&mut self, ui: &mut egui::Ui) {
        for p in crate::Projection::ALL {
            let id = projection_id(p);
            let (name, tip) = (self.win.t(id), self.win.t(&format!("{id}-tip")));
            crate::keycast::command_response(
                ui.radio_value(&mut self.win.layout.projection, p, &name),
                name,
            )
            .on_hover_text(tip);
        }
    }
}

/// 表示の形の文言の ID(名前は view3d の `Shading::label` の日本語と同じ)。
fn shading_id(s: crate::Shading) -> &'static str {
    match s {
        crate::Shading::ShadedEdges => "view-shading-shaded-edges",
        crate::Shading::ShadedHidden => "view-shading-shaded-hidden",
        crate::Shading::Shaded => "view-shading-shaded",
        crate::Shading::Wireframe => "view-shading-wireframe",
    }
}

/// 投影の文言の ID(説明は後ろに `-tip`)。
fn projection_id(p: crate::Projection) -> &'static str {
    match p {
        crate::Projection::Parallel => "view-projection-parallel",
        crate::Projection::Perspective => "view-projection-perspective",
        crate::Projection::Auto => "view-projection-auto",
    }
}
