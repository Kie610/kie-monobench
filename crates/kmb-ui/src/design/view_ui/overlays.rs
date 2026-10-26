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
                let mut parts = Vec::new();
                if !self.cur.picked_vertices.is_empty() {
                    parts.push(format!("{} 個の頂点", self.cur.picked_vertices.len()));
                }
                if !self.cur.picked_edges.is_empty() {
                    parts.push(format!("{} 本の辺", self.cur.picked_edges.len()));
                }
                if picked.is_some() {
                    parts.push(format!("{} 面", self.cur.also_picked.len() + 1));
                }
                Some((
                    "選択".to_string(),
                    format!(
                        "{}を選んでいます。Shift・Ctrl+クリックで足す・外す",
                        parts.join("・")
                    ),
                    false,
                ))
            }
            (None, Some(p)) => {
                let n = self.cur.also_picked.len() + 1;
                let text = if n > 1 {
                    format!(
                        "{n} 面を選んでいます(最後: {})。Shift・Ctrl+クリックで足す・外す",
                        self.face_label(&p.face)
                    )
                } else {
                    format!(
                        "選んだ面: {}(Shift・Ctrl+クリックで足す)",
                        self.face_label(&p.face)
                    )
                };
                Some((format!("{n} 面"), text, false))
            }
            (None, None) => {
                // 加工は削る途中の形、解析は結果の色分け
                let head = if self.win.layout.space == crate::layout::Space::Simulation {
                    "結果"
                } else {
                    "途中の形"
                };
                shape.map(|s| (head.to_string(), s, false))
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
                    let name = if focus {
                        "全画面表示を終了"
                    } else {
                        "全画面表示"
                    };
                    let fr = egui::Frame::new()
                        .fill(l.pal.panel)
                        .stroke(egui::Stroke::new(1.0, l.pal.edge))
                        .corner_radius(crate::theme::corner(5.0))
                        .show(ui, |ui| theme::icon_button(ui, "fit", name, true, focus))
                        .inner;
                    if fr.on_hover_text(name).clicked() {
                        view = Some(9);
                    }
                    ui.add_space(4.0);
                    // 上・前・右は下の軸の玉で選ぶ。等角だけボタンで残す
                    // (もう一度押すと反対側から見る。その間は名前も「等角(後)」に変わる)
                    let b = view3d::VIEW_BUTTONS[3];
                    let item = match self.cur.camera.view_side(&b) {
                        Some(true) => (b.back, true, true),
                        side => (b.label, side.is_some(), true),
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
                    for (icon, text, mode) in [
                        ("orbit", "オービット", LeftDrag::Orbit),
                        ("pan", "画面移動", LeftDrag::Pan),
                        ("zoom", "ズーム", LeftDrag::Zoom),
                    ] {
                        let on = self.win.left_drag == mode;
                        let c = if on { l.ws() } else { l.pal.ink2 };
                        let r = theme::icon_text_button_fit(ui, icon, text, c, 11.5, icon_only)
                            .on_hover_text(if mode == LeftDrag::Orbit {
                                format!(
                                    "押している間、左ドラッグで{text}(もう一度押すか Esc で戻す)。今は{}。右クリックで替える",
                                    if self.win.layout.free_orbit {
                                        "フリーオービット"
                                    } else {
                                        "拘束オービット"
                                    }
                                )
                            } else {
                                format!("押している間、左ドラッグで{text}(もう一度押すか Esc で戻す)")
                            });
                        if r.clicked() {
                            act = Some(if on { LeftDrag::Select } else { mode });
                        }
                        // オービットの右クリックで回し方を選ぶ
                        if mode == LeftDrag::Orbit {
                            r.right_menu(|ui| {
                                ui.radio_value(
                                    &mut self.win.layout.free_orbit,
                                    false,
                                    "拘束オービット(上下を保つ)",
                                );
                                ui.radio_value(&mut self.win.layout.free_orbit, true, "フリーオービット");
                            });
                        }
                    }
                    if theme::icon_text_button_fit(
                        ui,
                        "fit",
                        "全体表示",
                        l.pal.ink2,
                        11.5,
                        icon_only,
                    )
                    .on_hover_text("全体を収め、回す中心を部品の中心にする(Home)")
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
                    let r = theme::icon_text_button_fit(ui, "lookat", "正面", c, 11.5, icon_only);
                    let r = match &why {
                        None => r.on_hover_text("選んだ面・スケッチ・原点の平面を真正面から見て、形全体を収める(Look At)"),
                        Some(w) => r.on_hover_text(format!("選んだ面・スケッチ・原点の平面を真正面から見る。{w}")),
                    };
                    if r.clicked() {
                        self.look_at_selected();
                        ui.ctx().request_repaint();
                    }
                    let r = theme::icon_text_button_fit(
                        ui,
                        "display",
                        "表示 ▾",
                        l.pal.ink2,
                        11.5,
                        icon_only,
                    )
                    .on_hover_text(
                        "ブラウザ・タイムライン・編集パネル、表示の形と投影、格子と原点",
                    );
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
                ui.label(
                    egui::RichText::new("パネル")
                        .weak()
                        .size(crate::theme::fs(11.0)),
                );
                ui.checkbox(&mut self.win.layout.data_open, "データパネル")
                    .on_hover_text("最近・プロジェクトのデザインの一覧(上の帯の左端のボタンと同じ)");
                ui.checkbox(&mut self.win.layout.browser_open, "ブラウザ");
                // レンダリングと解析では下の帯を出さない(dock_ui の Pane::in_space)
                let here = crate::dock_ui::Pane::Timeline.in_space(self.win.layout.space);
                ui.add_enabled(
                    here,
                    egui::Checkbox::new(&mut self.win.layout.timeline_open, "タイムライン(加工は段の帯)"),
                )
                .on_disabled_hover_text("レンダリングと解析では出しません(設計へ戻すと出ます)");
                ui.checkbox(&mut self.win.layout.panel_always, "選んだだけでも編集パネルを出す")
                    .on_hover_text("切: 編集パネルは作る・直す・パラメータの間だけ出す。入: 機能を選ぶと値を見せる");
                ui.checkbox(&mut self.win.layout.commands_open, "コマンド")
                    .on_hover_text("コマンドを打つ欄。画面の部品を名前で押す・打つ、スクリプトを流す(help で一覧)");
                if ui
                    .add_enabled(
                        !self.dock_is_default(),
                        egui::Button::new("並びを既定に戻す"),
                    )
                    .on_hover_text(
                        "部品のタブを引いて付け替えた並びを、初めの形に戻す(隠した部品も出す)",
                    )
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
                .on_hover_text("ブラウザを出す");
            resp.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "ブラウザを出す")
            });
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
            for (k, ch) in GRIP_TEXT.chars().enumerate() {
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
        let n = GRIP_TEXT.chars().count() as f32;
        egui::vec2(
            20.0_f32.max(crate::theme::fs(11.0) + 8.0),
            66.0_f32.max(Self::grip_step() * n + 8.0),
        )
    }

    /// 表示の形(陰影と辺・陰影と隠れ線・陰影だけ・線だけ)と投影(平行・透視・自動)、格子と原点を選ぶ項目。
    /// 下の帯の「表示」、何もない所の右クリック、設定の窓で使う。
    pub(crate) fn view_style_items(&mut self, ui: &mut egui::Ui) {
        ui.label(
            egui::RichText::new("表示スタイル")
                .weak()
                .size(crate::theme::fs(11.0)),
        );
        for s in crate::Shading::ALL {
            ui.radio_value(&mut self.win.layout.shading, s, s.label());
        }
        ui.separator();
        ui.label(
            egui::RichText::new("投影")
                .weak()
                .size(crate::theme::fs(11.0)),
        );
        for p in crate::Projection::ALL {
            ui.radio_value(&mut self.win.layout.projection, p, p.label())
                .on_hover_text(p.hint());
        }
        ui.separator();
        ui.label(
            egui::RichText::new("目安")
                .weak()
                .size(crate::theme::fs(11.0)),
        );
        ui.checkbox(&mut self.win.layout.grid, "グリッド")
            .on_hover_text("XY 平面の格子。間隔は拡大に合わせて変わる(左下に今の間隔)");
        ui.checkbox(&mut self.win.layout.origin, "原点と軸")
            .on_hover_text("原点と X(赤)・Y(緑)・Z(青)の軸。空の文書では基準面も");
        ui.checkbox(&mut self.win.layout.see_through, "全部透かす")
            .on_hover_text(
                "本体の奥にあるスケッチと作業平面も普通の線で見せる。切ると、隠れた所は薄い点線",
            );
        ui.separator();
        ui.label(
            egui::RichText::new("効果")
                .weak()
                .size(crate::theme::fs(11.0)),
        );
        self.effects_items(ui);
    }
}
