//! 加工の段の帯(試作の S4 の下の帯): セットアップごとに、素材と削る順の操作の札を並べる。
//! 札を押すとその操作を選び(ブラウザ・編集パネルと同じ選び)、CAM のモジュールが出す段
//! ([`kmb_core::StageBand`]、素材 → 各操作の後の形)のセットアップなら、その段の形を 3D の
//! ビューに出す。もう一度押すと部品の形に戻す。帯の中身と段の形はモジュールが決め、画面は
//! 並べて実行するだけ。右端には、モジュールが [`kmb_core::Place::Timeline`] に置いた操作
//! (G-code を見る・書き出すなど)を出す。

use crate::{Workspace, icons, theme};
use egui::{Rect, Sense, Stroke, Vec2};
use kmb_core::{FeatureEntry, FeatureId, Place, StageBand, category};

/// 段の帯の段に当たる札(名前、説明、いま出している段か、それより後か、押した時の操作)。
struct StageChip {
    label: String,
    hint: String,
    on: bool,
    later: bool,
    action: Option<String>,
}

/// 札を押した時にすること。
struct Press {
    select: FeatureId,
    /// 段の帯の操作(段の形を出す、部品に戻す)
    stage: Option<String>,
}

impl Workspace {
    /// 今の段の帯(加工のモジュールが出す。なければ `None`)。
    pub fn stage_band(&self) -> Option<StageBand> {
        self.win.extensions.stages(&self.context())
    }

    /// 削る順の操作の一覧(セットアップごと)。親のセットアップは記述子の `parent` の欄で決める。
    fn steps(&self) -> Vec<(FeatureEntry, Vec<FeatureEntry>)> {
        let data = self.cur.work.doc().data();
        let parent_of = |f: &FeatureEntry| -> Option<FeatureId> {
            let d = self.win.registry.descriptor(&f.type_id)?;
            f.data.get(&d.parent?)?.as_u64().map(FeatureId)
        };
        let is = |f: &FeatureEntry, c: &str| {
            self.win
                .registry
                .descriptor(&f.type_id)
                .is_some_and(|d| d.category == c)
        };
        data.features
            .iter()
            .filter(|f| is(f, category::SETUP))
            .map(|s| {
                let ops = data
                    .features
                    .iter()
                    .filter(|f| is(f, category::OPERATION) && parent_of(f) == Some(s.id))
                    .cloned()
                    .collect();
                (s.clone(), ops)
            })
            .collect()
    }

    pub(crate) fn steps_ui(&mut self, ui: &mut egui::Ui) {
        let l = theme::look(ui.ctx());
        let steps = self.steps();
        let band = self.stage_band();
        let mut press: Option<Press> = None;
        let order: Vec<FeatureId> = steps
            .iter()
            .flat_map(|(s, ops)| std::iter::once(s.id).chain(ops.iter().map(|o| o.id)))
            .collect();
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            // 前の段・次の段。段の帯があればその段の形を出し(何も出していなければ次は素材)、
            // なければ前後の操作を選ぶ
            for (icon, name, d) in [("back", "前へ", -1i32), ("fwd", "次へ", 1)] {
                let (target, hint) = match &band {
                    Some(b) => {
                        let k = match (b.selected, d) {
                            (Some(k), -1) => k.checked_sub(1),
                            (Some(k), _) => Some(k + 1).filter(|k| *k < b.stages.len()),
                            (None, -1) => None,
                            (None, _) => (!b.stages.is_empty()).then_some(0),
                        };
                        // 段の形と一緒に、その段の操作(素材ならセットアップ)を選ぶ。パネルと経路も
                        // その操作に合わせる(Z21: 形だけ進み、パネルと経路は前の操作のままだった)
                        let a = k.and_then(|k| {
                            let st = b.stages.get(k)?;
                            Some((st.action.clone(), stage_feature(&steps, b, k)))
                        });
                        let h = if d < 0 { "前の段の形を出す" } else { "次の段の形を出す" };
                        (a.map(Ok), h)
                    }
                    None => {
                        let i = self
                            .cur.selected
                            .and_then(|s| order.iter().position(|x| *x == s));
                        let j = match i {
                            Some(i) => (i as i32 + d).clamp(0, order.len() as i32 - 1) as usize,
                            None => 0,
                        };
                        let h = if d < 0 { "前の操作を選ぶ" } else { "次の操作を選ぶ" };
                        (order.get(j).copied().map(Err), h)
                    }
                };
                if theme::icon_button(ui, icon, name, target.is_some(), false)
                    .on_hover_text(hint)
                    .clicked()
                {
                    match target {
                        Some(Ok((a, f))) => {
                            if let Some(f) = f {
                                self.select(f);
                            }
                            self.run_action(&a);
                        }
                        Some(Err(id)) => self.select(id),
                        None => {}
                    }
                }
            }
            // 動きの単位の再生(シミュレーションがまだなら走らせてから)。再生中はやめる
            if let Some(b) = &band
                && let Some(play) = &b.play
            {
                let (icon, name, hint, action) = match &b.playback {
                    Some(pb) => (
                        "x",
                        "再生をやめる",
                        "再生をやめて、部品の形に戻す",
                        pb.stop.clone(),
                    ),
                    None => (
                        "play",
                        "再生",
                        "工具の動きを 1 つずつ再生して、削れていく形と工具を見る",
                        play.clone(),
                    ),
                };
                if theme::icon_button(ui, icon, name, true, b.playback.is_some())
                    .on_hover_text(hint)
                    .clicked()
                {
                    self.cur.playing = None;
                    self.run_action(&action);
                }
            }
            ui.add_space(crate::timeline_ui::STRIP_GAP);
            // 右端の操作の幅をとっておく
            let actions: Vec<kmb_core::Action> = self
                .actions()
                .into_iter()
                .filter(|a| a.is_at(Place::Timeline))
                .collect();
            let clear = band.as_ref().and_then(|b| b.clear.clone());
            let k = theme::band();
            let right_w =
                (actions.len() as f32 * 132.0 + if clear.is_some() { 110.0 } else { 0.0 }) * k;
            let w = (ui.available_width() - right_w - crate::timeline_ui::STRIP_GAP).max(100.0);
            let (rect, _) = ui.allocate_exact_size(
                Vec2::new(w, crate::timeline_ui::strip_height()),
                Sense::hover(),
            );
            crate::timeline_ui::paint_strip(ui, rect, &l);
            // 帯の説明(段の形の出どころ)は右上に小さく
            if let Some(b) = &band {
                let g = ui.painter().layout(
                    b.note.clone(),
                    crate::theme::prop(10.0),
                    l.pal.ink3,
                    rect.width() * 0.55,
                );
                if g.rows.len() == 1 {
                    ui.painter().galley(
                        egui::pos2(rect.right() - g.size().x - 8.0, rect.top() + 3.0),
                        g,
                        l.pal.ink3,
                    );
                }
            }
            let mut strip = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(rect.shrink2(Vec2::new(8.0, 4.0)))
                    .layout(egui::Layout::left_to_right(egui::Align::Max)),
            );
            egui::ScrollArea::horizontal()
                .id_salt("steps_scroll")
                .show(&mut strip, |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    if steps.is_empty() {
                        ui.label(
                            egui::RichText::new(
                                "セットアップはまだありません。上の「セットアップ」から足すと、削る順の操作がここに並びます",
                            )
                            .color(l.pal.ink3),
                        );
                    }
                    for (s, ops) in &steps {
                        let b = band.as_ref().filter(|b| b.title == s.name);
                        if let Some(p) = self.setup_section(ui, s, ops, b) {
                            press = Some(p);
                        }
                    }
                });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                for a in actions.iter() {
                    let pri = a.icon == "export";
                    let r = self.strip_action(ui, a, pri);
                    if r.clicked() {
                        self.start_action(&a.id);
                    }
                }
                if let Some(c) = &clear {
                    let a = kmb_core::Action::new(c, "部品を見る", "cam")
                        .icon("box")
                        .hint("途中の形をやめて、部品の形に戻す");
                    if self.strip_action(ui, &a, false).clicked() {
                        self.run_action(c);
                    }
                }
            });
        });
        if let Some(p) = press {
            self.select(p.select);
            if let Some(a) = p.stage {
                self.run_action(&a);
            }
        }
    }

    /// セットアップ 1 つ分(縦の線と名前、素材の札、操作の札)。`band` はこのセットアップの段の帯。
    fn setup_section(
        &mut self,
        ui: &mut egui::Ui,
        s: &FeatureEntry,
        ops: &[FeatureEntry],
        band: Option<&StageBand>,
    ) -> Option<Press> {
        let l = theme::look(ui.ctx());
        let (bar, _) = ui.allocate_exact_size(
            Vec2::new(14.0, crate::timeline_ui::strip_height() - 8.0),
            Sense::hover(),
        );
        ui.painter()
            .vline(bar.left() + 2.0, bar.y_range(), Stroke::new(2.0, l.ws()));
        let label: String = s.name.chars().take(8).collect();
        ui.painter().text(
            bar.left_top() + Vec2::new(6.0, 2.0),
            egui::Align2::LEFT_TOP,
            label,
            crate::theme::prop(10.5),
            l.ws(),
        );
        ui.add_space(-4.0);
        // 段の番号(素材は 0、操作は削る順に 1 から。名前で帯の段と結ぶ)
        let stage = |k: usize, name: &str| {
            let b = band?;
            let j = if k == 0 {
                0
            } else {
                let head = format!("{k} {name}");
                b.stages
                    .iter()
                    .position(|st| st.label == head || st.label.starts_with(&format!("{head} ")))?
            };
            let st = b.stages.get(j)?;
            let on = b.selected == Some(j);
            // 出している段より後は、まだ削っていない段
            let later = b.selected.is_some_and(|s| j > s);
            let action = if on {
                b.clear.clone()
            } else {
                Some(st.action.clone())
            };
            Some(StageChip {
                label: st.label.clone(),
                hint: st.hint.clone(),
                on,
                later,
                action,
            })
        };
        let mut press = None;
        let st = stage(0, "素材");
        if self.step_chip(ui, s, "stock", "素材", "", st.as_ref()) {
            press = Some(Press {
                select: s.id,
                stage: st.and_then(|x| x.action),
            });
        }
        for (k, o) in ops.iter().enumerate() {
            let icon = self
                .win
                .registry
                .descriptor(&o.type_id)
                .map(|d| d.icon)
                .filter(|i| !i.is_empty())
                .unwrap_or_else(|| "path".into());
            let tool = o
                .data
                .get("tool")
                .and_then(|v| v.as_u64())
                .map_or(String::new(), |t| format!("T{t}"));
            let st = stage(k + 1, &o.name);
            if self.step_chip(ui, o, &icon, &o.name, &tool, st.as_ref()) {
                press = Some(Press {
                    select: o.id,
                    stage: st.and_then(|x| x.action),
                });
            }
        }
        ui.add_space(8.0);
        press
    }

    /// 札 1 枚。`stage` は段の帯の段(名前、いま出している段か、押した時の操作)。押されたら真。
    fn step_chip(
        &mut self,
        ui: &mut egui::Ui,
        f: &FeatureEntry,
        icon: &str,
        name: &str,
        tool: &str,
        stage: Option<&StageChip>,
    ) -> bool {
        let l = theme::look(ui.ctx());
        let font = crate::theme::prop(12.0);
        let g = ui
            .painter()
            .layout_no_wrap(name.to_string(), font.clone(), l.pal.ink);
        let tg = ui
            .painter()
            .layout_no_wrap(tool.to_string(), crate::theme::prop(10.5), l.ws());
        let tw = if tool.is_empty() {
            0.0
        } else {
            tg.size().x + 5.0
        };
        // アイコンと札の高さは倍率に合わせる(1 倍でアイコン 14、高さ 26)。場所を固定のまま
        // アイコンだけ倍率で大きくすると、250 % でアイコンが名前に重なる
        let iw = theme::icon_px(14.0);
        let size = Vec2::new(
            8.0 + iw + 5.0 + g.size().x + tw + 9.0,
            26.0_f32.max(g.size().y + 8.0).max(iw + 12.0),
        );
        let (rect, r) = ui.allocate_exact_size(size, Sense::click());
        let selected = self.selection().contains(&f.id);
        let shown = stage.is_some_and(|s| s.on);
        // 選んだ札が帯の外(右で切れた所)にあれば、見える所まで送る(Z26)。送るのは選びが変わった時だけ
        let seen = crate::doc_id(ui.ctx(), "steps_scrolled_to");
        if selected && ui.ctx().data(|d| d.get_temp::<FeatureId>(seen)) != Some(f.id) {
            ui.ctx().data_mut(|d| d.insert_temp(seen, f.id));
            ui.scroll_to_rect(rect, None);
        }
        let later = stage.is_some_and(|s| s.later);
        // 段の帯の段なら、その名前(「3 ポケット T2」)で読み上げる
        let label = stage.map_or(name.to_string(), |s| s.label.clone());
        r.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                true,
                selected || shown,
                &label,
            )
        });
        let (fill, line) = if shown {
            (l.ws_soft(), l.ws())
        } else if selected {
            (l.pal.panel, l.ws())
        } else if r.hovered() {
            (l.pal.chrome2, l.pal.edge)
        } else {
            (l.pal.panel, l.pal.edge)
        };
        let p = ui.painter();
        p.rect(
            rect,
            4.0,
            fill,
            Stroke::new(1.0, line),
            egui::StrokeKind::Inside,
        );
        let ink = if f.suppressed || later {
            l.pal.ink3
        } else {
            l.pal.ink
        };
        icons::paint_exact(
            p,
            Rect::from_center_size(
                egui::pos2(rect.left() + 8.0 + iw / 2.0, rect.center().y),
                Vec2::splat(iw),
            ),
            icon,
            if f.suppressed { l.pal.ink3 } else { l.pal.ink2 },
            1.6,
        );
        let x = rect.left() + 8.0 + iw + 5.0;
        p.galley_with_override_text_color(
            egui::pos2(x, rect.center().y - g.size().y / 2.0),
            g.clone(),
            ink,
        );
        if !tool.is_empty() {
            p.galley(
                egui::pos2(x + g.size().x + 5.0, rect.center().y - tg.size().y / 2.0),
                tg,
                l.ws(),
            );
        }
        let (_, detail) = self.status_line(f.id);
        let hint = match (stage, detail.is_empty()) {
            (Some(_), true) if shown => "もう一度押すと部品の形に戻す".to_string(),
            (Some(s), true) => format!("{}(押すと 3D のビューに出す)", s.hint),
            (_, false) => detail,
            (None, true) => String::new(),
        };
        let r = if hint.is_empty() {
            r
        } else {
            r.on_hover_text(hint)
        };
        let mut pressed = false;
        if r.clicked() {
            if ui.input(|i| i.modifiers.command) {
                self.toggle_select(f.id);
            } else {
                pressed = true;
            }
        }
        r.context_menu(|ui| self.feature_menu(ui, f.id));
        pressed
    }

    /// 段の帯の右端の操作のボタン(`pri` は作業空間の色の縁取り)。
    fn strip_action(&self, ui: &mut egui::Ui, a: &kmb_core::Action, pri: bool) -> egui::Response {
        let l = theme::look(ui.ctx());
        let enabled = a.disabled.is_none();
        let fg = match (enabled, pri) {
            (false, _) => l.pal.ink3,
            (true, true) => l.ws(),
            (true, false) => l.pal.ink,
        };
        let label = a
            .label
            .trim_end_matches('…')
            .replace("G-code を書き出す", "書き出す");
        let g = ui
            .painter()
            .layout_no_wrap(label.clone(), crate::theme::prop(12.0), fg);
        let iw = theme::icon_px(14.0);
        let size = Vec2::new(
            10.0 + iw + 6.0 + g.size().x + 12.0,
            26.0_f32.max(g.size().y + 8.0).max(iw + 12.0),
        );
        let sense = if enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, r) = ui.allocate_exact_size(size, sense);
        r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &a.label));
        let fill = if pri { l.ws_soft() } else { l.pal.panel };
        let line = if pri && enabled { l.ws() } else { l.pal.edge };
        let p = ui.painter();
        p.rect(
            rect,
            rect.height() / 2.0,
            fill,
            Stroke::new(1.0, line),
            egui::StrokeKind::Inside,
        );
        icons::paint_exact(
            p,
            Rect::from_center_size(
                egui::pos2(rect.left() + 10.0 + iw / 2.0, rect.center().y),
                Vec2::splat(iw),
            ),
            &a.icon,
            fg,
            1.6,
        );
        p.galley(
            egui::pos2(
                rect.left() + 10.0 + iw + 6.0,
                rect.center().y - g.size().y / 2.0,
            ),
            g,
            fg,
        );
        match &a.disabled {
            Some(why) => r.on_hover_text(why),
            None => r.on_hover_text(&a.hint),
        }
    }
}

/// 段の帯 `b` の `k` 番目の段に当たる機能(素材はセットアップ、ほかは札と同じく名前で結んだ操作)。
fn stage_feature(
    steps: &[(FeatureEntry, Vec<FeatureEntry>)],
    b: &StageBand,
    k: usize,
) -> Option<FeatureId> {
    let (s, ops) = steps.iter().find(|(s, _)| s.name == b.title)?;
    if k == 0 {
        return Some(s.id);
    }
    let label = &b.stages.get(k)?.label;
    ops.iter().enumerate().find_map(|(i, o)| {
        let head = format!("{} {}", i + 1, o.name);
        (*label == head || label.starts_with(&format!("{head} "))).then_some(o.id)
    })
}
