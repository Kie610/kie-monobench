//! タブ付きツールバー: 作業空間の切替の箱、タブ、まとまりの帯(大きなボタン・2 段の小さなボタン・▾ の一覧)、選択のまとまり。

use super::*;
use crate::theme::RightMenu as _;

impl Workspace {
    /// 今の作業空間のツールバーのタブ。
    pub fn ribbon_tabs(&self) -> Vec<ribbon::Tab> {
        let space = self.win.layout.space;
        let mut items: Vec<(String, Item)> = Vec::new();
        // 測る道具は、設計でも加工でも使う(加工では部品の寸法を確かめる)
        let check = match space {
            Space::Design | Space::Simulation => "検査",
            Space::Machining => "確かめる",
            Space::Render | Space::Animation => "",
        };
        let measure = (
            check.to_string(),
            Item {
                key: "u:measure".into(),
                label: self.win.t("toolbar-measure"),
                icon: "measure".into(),
                hint: self.win.t("toolbar-measure-tip"),
                disabled: None,
                front: true,
                formerly: Vec::new(),
            },
        );
        let section = (
            check.to_string(),
            Item {
                key: "u:section".into(),
                label: self.win.t("toolbar-section"),
                icon: "section".into(),
                hint: self.win.t("toolbar-section-tip"),
                disabled: None,
                // 加工では ▾ の一覧だけ(Kie 2026-10-21 の画板)
                front: space != Space::Machining,
                formerly: Vec::new(),
            },
        );
        let press_pull = (
            "修正".to_string(),
            Item {
                key: crate::press_pull_ui::PRESS_PULL.into(),
                label: self.win.t("toolbar-press-pull"),
                icon: "presspull".into(),
                hint: self.win.t("toolbar-press-pull-tip"),
                disabled: None,
                front: true,
                formerly: Vec::new(),
            },
        );
        // 設計の修正はプレス/プルから、検査は計測・断面解析から並べる(Kie 2026-10-21 の画板)。
        // 加工は操作の後ろ。レンダリングとアニメーションでは測る道具と断面を出さない
        let (first, last) = match space {
            Space::Design => (vec![press_pull, measure, section], vec![]),
            Space::Simulation | Space::Machining => (vec![], vec![measure, section]),
            Space::Render | Space::Animation => (vec![], vec![]),
        };
        items.extend(first);
        for a in self
            .actions()
            .into_iter()
            .filter(|a| a.group == space.action_group())
            // 置き場所が選んだ物のパネルや右クリックだけの操作(工具の順番に足すなど)は出さない
            .filter(|a| a.places.is_empty() || a.is_at(Place::Toolbar) || a.is_at(Place::Menu))
        {
            let front = a.is_at(Place::Toolbar);
            items.push((
                a.menu.clone(),
                Item {
                    key: format!("a:{}", a.id),
                    label: a.label,
                    icon: a.icon,
                    hint: a.hint,
                    disabled: a.disabled,
                    front,
                    formerly: a.replaces.iter().map(|t| format!("f:{t}")).collect(),
                },
            ));
        }
        items.extend(last);
        match space {
            Space::Render => {
                items.push((
                    "設定".into(),
                    Item {
                        key: "u:scene".into(),
                        label: self.win.t("toolbar-scene"),
                        icon: "gear".into(),
                        hint: self.win.t("toolbar-scene-tip"),
                        disabled: None,
                        front: true,
                        formerly: Vec::new(),
                    },
                ));
                items.push((
                    "設定".into(),
                    Item {
                        key: "u:appearance".into(),
                        label: self.win.t("toolbar-appearance"),
                        icon: "swatch".into(),
                        hint: self.win.t("toolbar-appearance-tip"),
                        disabled: None,
                        front: true,
                        formerly: Vec::new(),
                    },
                ));
                items.push((
                    "キャンバス内レンダリング".into(),
                    Item {
                        key: "u:canvas_render".into(),
                        label: self.win.t("toolbar-canvas-render"),
                        icon: "render".into(),
                        hint: self.win.t("toolbar-canvas-render-tip"),
                        disabled: None,
                        front: true,
                        formerly: Vec::new(),
                    },
                ));
            }
            Space::Design => {
                let action_labels: Vec<String> =
                    items.iter().map(|(_, i)| i.label.clone()).collect();
                for (type_id, label, why) in self.addable(kmb_core::category::SOLID) {
                    let (menu, mut item) = self.feature_item(&type_id, &label, why);
                    // 同じ名前の操作(クリックで作る流れ)があれば、欄で指定して足す方だと分かる名前にする
                    if action_labels.contains(&item.label) {
                        item.label = self
                            .win
                            .t_with("toolbar-feature-by-fields", &[("name", item.label.into())]);
                    }
                    items.push((menu, item));
                }
                items.push((
                    "修正".into(),
                    Item {
                        key: "u:params".into(),
                        label: self.win.t("toolbar-params"),
                        icon: "param".into(),
                        hint: self.win.t("toolbar-params-tip"),
                        disabled: None,
                        front: true,
                        formerly: Vec::new(),
                    },
                ));
            }
            Space::Animation => {}
            Space::Simulation => {
                // 研究は欄で条件を付ける機能(固定・荷重の面は 3D のビューでクリックして足す)
                for (type_id, label, why) in self.addable(kmb_core::category::STUDY) {
                    items.push(self.feature_item(&type_id, &label, why));
                }
            }
            Space::Machining => {
                items.push((
                    "管理".into(),
                    Item {
                        key: "u:tools".into(),
                        label: self.win.t("toolbar-tools"),
                        icon: "tools".into(),
                        hint: self.win.t("toolbar-tools-tip"),
                        disabled: None,
                        front: true,
                        formerly: Vec::new(),
                    },
                ));
                items.push((
                    "管理".into(),
                    Item {
                        key: "u:machines".into(),
                        // 機械は 1 台だけなので、一覧(ライブラリ)ではなく設定と呼ぶ(Z7)
                        label: self.win.t("toolbar-machine"),
                        icon: "machine".into(),
                        hint: self.win.t("toolbar-machine-tip"),
                        disabled: None,
                        front: true,
                        formerly: Vec::new(),
                    },
                ));
            }
        }
        ribbon::tabs(space, items, &self.win.layout)
    }

    /// 欄で足す機能の種類 `type_id` の項目と、入れるまとまりの名前。名前と説明は今の言語にする
    /// (分野 `feature`、ID は [`kmb_i18n::feature_id`])。`why` は使えない理由。
    fn feature_item(&self, type_id: &str, label: &str, why: Option<String>) -> (String, Item) {
        let d = self.win.registry.descriptor(type_id);
        let id = kmb_i18n::feature_id(type_id);
        let hint = d.as_ref().map(|d| d.hint.as_str()).unwrap_or_default();
        let item = Item {
            key: format!("f:{type_id}"),
            label: self.win.t_or(Some(&id), label),
            icon: d.as_ref().map(|d| d.icon.clone()).unwrap_or_default(),
            hint: if hint.is_empty() {
                String::new()
            } else {
                self.win.t_or(Some(&format!("{id}-tip")), hint)
            },
            disabled: why,
            front: d.as_ref().is_some_and(|d| d.pinned),
            formerly: Vec::new(),
        };
        (d.map(|d| d.menu).unwrap_or_default(), item)
    }

    /// ツールバーの項目のボタン。押したら実行して真を返す。
    pub(crate) fn ribbon_item_button(&mut self, ui: &mut egui::Ui, it: &Item) -> bool {
        let r = ui.add_enabled(it.disabled.is_none(), egui::Button::new(it.label.as_str()));
        let r = match &it.disabled {
            Some(why) => r.on_disabled_hover_text(why),
            None => match (self.key_of_item(&it.key), it.hint.is_empty()) {
                (Some(k), true) => {
                    r.on_hover_text(self.win.t_with("toolbar-key", &[("key", k.into())]))
                }
                (Some(k), false) => r.on_hover_text(format!(
                    "{}\n{}",
                    it.hint,
                    self.win.t_with("toolbar-key", &[("key", k.into())])
                )),
                (None, false) => r.on_hover_text(&it.hint),
                (None, true) => r,
            },
        };
        if r.clicked() {
            self.run_item(&it.key);
            return true;
        }
        false
    }

    /// 今使っているツールバーの項目(クリックで作っている途中・作った機能の値を決めている途中・測る)。
    /// ツールバーで光らせる。
    pub(crate) fn active_item(&self) -> Option<String> {
        if let Some(g) = &self.cur.gesture {
            return Some(format!("a:{}", g.action));
        }
        if self.cur.creating.is_some() {
            return self.win.last_item.clone();
        }
        if self.cur.measure.is_some() {
            return Some("u:measure".into());
        }
        if self.is_sectioning() {
            return Some("u:section".into());
        }
        if self.cur.canvas_render.is_some() {
            return Some("u:canvas_render".into());
        }
        None
    }

    /// ツールバーの項目を実行する(`key` は「a:操作」「f:種類」「u:窓」)。
    pub fn run_item(&mut self, key: &str) {
        if let (Some(ctx), Some(name)) = (&self.win.repaint, self.keycast_item_name(key)) {
            crate::keycast::command_name(ctx, name, false);
        }
        if key.starts_with("a:") || key.starts_with("f:") {
            self.win.last_item = Some(key.to_string());
        }
        if let Some(id) = key.strip_prefix("a:") {
            self.start_action(id);
        } else if let Some(t) = key.strip_prefix("f:") {
            self.add_described(t);
        } else {
            match key {
                "u:params" => {
                    self.win.show_params = true;
                }
                "u:machines" => self.open_machine_settings(),
                "u:measure" => self.toggle_measure(),
                "u:section" => self.toggle_section(),
                crate::press_pull_ui::PRESS_PULL => self.press_pull(),
                "u:tools" => self.open_tools(),
                "u:scene" => self.open_render_settings(),
                "u:appearance" => self.open_looks(),
                "u:canvas_render" => self.toggle_canvas_render(),
                _ => {}
            }
        }
    }

    /// ツールバーの項目 `key` の名前と、今は使えない理由。
    pub(crate) fn item_of(&self, key: &str) -> Option<Item> {
        self.ribbon_tabs()
            .into_iter()
            .flat_map(|t| t.groups)
            .flat_map(|g| g.items)
            .find(|i| i.key == key)
    }

    /// 1 文字のキーで、ツールバーの項目を始める(使えないときは理由を出す)。
    pub(crate) fn run_shortcut(&mut self, key: &str) {
        match self.item_of(key) {
            Some(it) => match it.disabled {
                Some(why) => self.win.fail(format!("{}: {why}", it.label)),
                None => self.run_item(key),
            },
            None => self.win.message = Some(self.win.t("toolbar-key-not-here")),
        }
    }

    /// 工具ライブラリを選ぶ(なければ足す)。
    fn open_tools(&mut self) {
        let tools = self.cur.work.doc().data().features.iter().rev().find(|f| {
            self.win
                .registry
                .descriptor(&f.type_id)
                .is_some_and(|d| d.category == kmb_core::category::TOOLS)
        });
        match tools.map(|f| f.id) {
            Some(id) => self.open_edit(id),
            None => {
                let t = self
                    .win
                    .registry
                    .descriptors()
                    .into_iter()
                    .find(|d| d.category == kmb_core::category::TOOLS && d.template.is_some())
                    .map(|d| d.type_id);
                match t {
                    Some(t) => {
                        self.add_described(&t);
                    }
                    None => self.win.fail("工具ライブラリの種類がありません"),
                }
            }
        }
    }

    /// ツールバー(試作の `.toolbar`): 左に作業空間の箱、右にタブとまとまり。
    pub(crate) fn ribbon_ui(&mut self, ui: &mut egui::Ui) {
        let l = theme::look(ui.ctx());
        let full = ui.max_rect();
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            self.space_box(ui, full.height());
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                let width = ui.available_width();
                // スケッチを直している間は、スケッチのタブだけにする(道具はビューの上の行)
                let sketch = self.view_editor_type();
                let tabs = if sketch.is_some() {
                    Vec::new()
                } else {
                    self.ribbon_tabs()
                };
                if sketch.is_none()
                    && !tabs.is_empty()
                    && !tabs.iter().any(|t| t.name == self.win.tab)
                {
                    self.win.tab = tabs[0].name.clone();
                }
                // タブの行
                ui.allocate_ui_with_layout(
                    egui::vec2(width, theme::fs(30.0)),
                    egui::Layout::left_to_right(egui::Align::Max),
                    |ui| {
                        ui.add_space(8.0);
                        ui.spacing_mut().item_spacing.x = 2.0;
                        if sketch.is_some() {
                            tab_button(ui, &self.win.t("toolbar-tab-sketch"), true, true);
                        }
                        for t in &tabs {
                            let shown = self.win.t_or(ribbon::tab_id(&t.name), &t.name);
                            let r = tab_button(ui, &shown, self.win.tab == t.name, false);
                            if r.clicked() {
                                self.win.tab = t.name.clone();
                            }
                            let (left, right, reset) = (
                                self.win.t("toolbar-move-left"),
                                self.win.t("toolbar-move-right"),
                                self.win.t("toolbar-tab-order-reset"),
                            );
                            r.right_menu(|ui| {
                                if crate::theme::menu_row(ui, "chevl", &left, "", true).clicked() {
                                    ribbon::move_tab(
                                        self.win.layout.space,
                                        &tabs,
                                        &t.name,
                                        -1,
                                        &mut self.win.layout,
                                    );
                                    ui.close();
                                }
                                if crate::theme::menu_row(ui, "chevr", &right, "", true).clicked() {
                                    ribbon::move_tab(
                                        self.win.layout.space,
                                        &tabs,
                                        &t.name,
                                        1,
                                        &mut self.win.layout,
                                    );
                                    ui.close();
                                }
                                ui.separator();
                                if crate::theme::menu_row(ui, "undo", &reset, "", true).clicked() {
                                    self.win.layout.tab_order.clear();
                                    ui.close();
                                }
                            });
                        }
                    },
                );
                // まとまりの帯
                let (band, _) = ui.allocate_exact_size(
                    egui::vec2(width, self.band_height()),
                    egui::Sense::hover(),
                );
                ui.painter()
                    .rect_filled(band, crate::theme::corner(0.0), l.pal.panel);
                ui.painter().hline(
                    band.x_range(),
                    band.top() + 0.5,
                    egui::Stroke::new(1.0, l.pal.edge),
                );
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(band.shrink2(egui::vec2(4.0, 0.0)))
                        .layout(egui::Layout::left_to_right(egui::Align::Min)),
                );
                if sketch.is_some() {
                    self.sketch_band(&mut child);
                } else if let Some(tab) = tabs.iter().find(|t| t.name == self.win.tab).cloned() {
                    self.groups_band(&mut child, &tab);
                    self.select_group(&mut child);
                }
            });
        });
    }

    /// 作業空間の箱(押すと設計と加工を選ぶ)。
    fn space_box(&mut self, ui: &mut egui::Ui, height: f32) {
        // 作業空間の箱は、スケッチを直している間も作業空間の色(差し色の水色にしない)
        let l = theme::Look {
            sketch: false,
            ..theme::look(ui.ctx())
        };
        let space = self.win.layout.space;
        // 字を大きくしても「作業空間」と作業空間の名前が箱に収まる幅にする(1 倍なら 116)。
        // 作業空間を替えても右のタブとツールバーがずれないよう、一番長い名前(アニメーション)で決める
        let name = ui.painter().layout_no_wrap(
            self.win.space_name(space),
            crate::theme::prop(16.0),
            l.ws(),
        );
        let head = ui.painter().layout_no_wrap(
            self.win.t("toolbar-space"),
            crate::theme::prop(10.5),
            l.pal.ink2,
        );
        let widest = Space::ALL
            .iter()
            .map(|s| {
                ui.painter()
                    .layout_no_wrap(self.win.space_name(*s), crate::theme::prop(16.0), l.ws())
                    .size()
                    .x
            })
            .fold(0.0_f32, f32::max);
        let inner = head
            .size()
            .x
            .max(widest + 12.0 + crate::theme::icon_px(12.0));
        let (outer, _) = ui.allocate_exact_size(
            egui::vec2((inner + 38.0).max(116.0), height),
            egui::Sense::hover(),
        );
        let rect = outer.shrink(8.0);
        let r = ui.interact(rect, ui.id().with("space_box"), egui::Sense::click());
        let label = self.win.t_with(
            "toolbar-space-now",
            &[("name", self.win.space_name(space).into())],
        );
        r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
        let p = ui.painter();
        let fill = if r.hovered() {
            theme::mix(l.ws_soft(), l.ws(), 0.12)
        } else {
            l.ws_soft()
        };
        p.rect(
            rect,
            6.0,
            fill,
            egui::Stroke::new(1.0, l.ws()),
            egui::StrokeKind::Inside,
        );
        let x = rect.left() + 11.0;
        // 2 行(上に「作業空間」、下に名前)の間は字の大きさに合わせてあける
        let gap = (head.size().y + name.size().y) / 2.0 + 2.5;
        let hy = rect.center().y - gap * 0.5;
        p.galley(egui::pos2(x, hy - head.size().y / 2.0), head, l.pal.ink2);
        let g = name;
        let ty = hy + gap;
        let w = g.size().x;
        p.galley(egui::pos2(x, ty - g.size().y / 2.0), g.clone(), l.ws());
        // 太字の代わりに少しずらして重ねる
        p.galley(egui::pos2(x + 0.6, ty - g.size().y / 2.0), g, l.ws());
        crate::icons::paint_exact(
            p,
            egui::Rect::from_center_size(
                egui::pos2(x + w + 6.0 + crate::theme::icon_px(6.0), ty + 1.0),
                egui::Vec2::splat(crate::theme::icon_px(12.0)),
            ),
            "chev",
            l.ws(),
            1.6,
        );
        let r = r.on_hover_text(self.win.t("toolbar-space-tip"));
        // 一覧は箱と同じ幅にし、行の字は箱の名前と同じ大きさにする(字の倍率にもついていく)
        let pad = 4.0;
        let frame = egui::Frame::popup(ui.style()).inner_margin(pad);
        let row_w = (rect.width() - 2.0 * (pad + frame.stroke.width)).max(widest + 14.0);
        let list_w = row_w + 2.0 * (pad + frame.stroke.width);
        egui::Popup::menu(&r)
            .frame(frame)
            .gap(4.0)
            .width(list_w)
            .show(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                let bg = ui.visuals().window_fill;
                for s in Space::ALL {
                    let c = l.pal.ws(s);
                    let shown = self.win.space_name(s);
                    let g = ui
                        .painter()
                        .layout_no_wrap(shown.clone(), crate::theme::prop(16.0), c);
                    let (row, rr) = ui.allocate_exact_size(
                        egui::vec2(row_w, g.size().y + 20.0),
                        egui::Sense::click(),
                    );
                    rr.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::SelectableLabel,
                            true,
                            s == space,
                            &shown,
                        )
                    });
                    let p = ui.painter();
                    let fill = match (s == space, rr.hovered()) {
                        (true, _) => Some(theme::mix(bg, c, 0.22)),
                        (false, true) => Some(theme::mix(bg, c, 0.12)),
                        _ => None,
                    };
                    if let Some(f) = fill {
                        p.rect_filled(row, 4.0, f);
                    }
                    let pos = egui::pos2(row.left() + 7.0, row.center().y - g.size().y / 2.0);
                    p.galley(pos, g.clone(), c);
                    p.galley(pos + egui::vec2(0.6, 0.0), g, c);
                    if rr.clicked() {
                        self.switch_space(s);
                        ui.close();
                    }
                }
            });
    }

    /// タブのまとまり(大きなボタンの列と、下にまとまりの名前 ▾)。帯に入りきらなければ、右の
    /// まとまりから畳む([`ribbon::fit`])。畳んだまとまりの道具も ▾ の一覧と「その他」から全部押せる。
    fn groups_band(&mut self, ui: &mut egui::Ui, tab: &ribbon::Tab) {
        let l = theme::look(ui.ctx());
        let space = self.win.layout.space;
        let active = self.active_item();
        let labels = self.win.layout.appearance.tool_labels;
        let fronts: Vec<Vec<Item>> = tab
            .groups
            .iter()
            .map(|g| {
                let mut front: Vec<Item> = ribbon::front(space, &tab.name, g, &self.win.layout)
                    .into_iter()
                    .cloned()
                    .collect();
                // 使っている道具が前にない(▾ の一覧から始めた)時は、終わるまで後ろに出して光らせる
                if let Some(it) = g.items.iter().find(|i| Some(&i.key) == active.as_ref())
                    && !front.iter().any(|f| f.key == it.key)
                {
                    front.push(it.clone());
                }
                front
            })
            .collect();
        // まとまりの見せる名前(名前そのものはピンの鍵なので訳さずに持つ)
        let names: Vec<String> = tab
            .groups
            .iter()
            .map(|g| self.win.t_or(ribbon::group_id(&g.name), &g.name))
            .collect();
        let widths: Vec<ribbon::Widths> = names
            .iter()
            .zip(&fronts)
            .map(|(name, f)| group_widths(ui, name, f.len(), labels))
            .collect();
        let select_w = 196.0 * theme::band();
        let fold = ribbon::fit(&widths, select_w, more_width(ui), ui.available_width());
        let tall = labels;
        let (unpin, pin, reset) = (
            self.win.t("toolbar-unpin"),
            self.win.t("toolbar-pin"),
            self.win.t("toolbar-pins-reset"),
        );
        for (((g, name), front), f) in tab.groups.iter().zip(&names).zip(&fronts).zip(&fold) {
            let shown: Vec<&Item> = match f {
                ribbon::Fold::Hidden => continue,
                ribbon::Fold::Collapsed => Vec::new(),
                _ => front.iter().collect(),
            };
            let compact = !labels || *f != ribbon::Fold::Full;
            let n = shown.len().max(1) as f32;
            let w = (n * tile_width(compact) + (n - 1.0) * 2.0 + 8.0).max(name_width(ui, name));
            let lit = g.items.iter().any(|i| Some(&i.key) == active.as_ref());
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(w, self.band_height()), egui::Sense::hover());
            let mut col = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(rect.shrink2(egui::vec2(4.0, 0.0)))
                    .layout(egui::Layout::top_down(egui::Align::Center)),
            );
            col.spacing_mut().item_spacing = egui::vec2(2.0, 0.0);
            col.add_space(3.0);
            // 畳んだまとまりは、まとまりのボタン 1 つ(押すと名前の ▾ と同じ一覧)
            let mut opens = Vec::new();
            col.horizontal(|ui| {
                if *f == ribbon::Fold::Collapsed {
                    let icon = g
                        .items
                        .iter()
                        .find(|i| Some(&i.key) == active.as_ref())
                        .or(front.first())
                        .or(g.items.first())
                        .map_or("", |i| i.icon.as_str());
                    opens.push(group_tile(ui, &self.win, icon, name, lit, tall));
                }
                for it in &shown {
                    let on = Some(&it.key) == active.as_ref();
                    self.command(ui, it, &tab.name, g, on, compact, tall);
                }
            });
            // まとまりの名前と ▾ の一覧(ピンで前に出す)
            opens.push(group_label(
                ui,
                rect,
                ("glabel", &tab.name, &g.name),
                name,
                lit,
            ));
            for r in &opens {
                egui::Popup::menu(r).show(|ui| {
                    let front: Vec<String> = ribbon::front(space, &tab.name, g, &self.win.layout)
                        .iter()
                        .map(|i| i.key.clone())
                        .collect();
                    for it in &g.items {
                        ui.horizontal(|ui| {
                            let pinned = front.contains(&it.key);
                            let pr = theme::icon_button(
                                ui,
                                "pin",
                                if pinned { &unpin } else { &pin },
                                true,
                                pinned,
                            )
                            .on_hover_text(if pinned {
                                &unpin
                            } else {
                                &pin
                            });
                            if pr.clicked() {
                                let e = if pinned {
                                    PinEdit::Unpin(it.key.clone())
                                } else {
                                    PinEdit::Pin(it.key.clone())
                                };
                                ribbon::edit_pins(space, &tab.name, g, &mut self.win.layout, e);
                            }
                            let on = Some(&it.key) == active.as_ref();
                            let r = theme::menu_item(
                                ui,
                                &it.icon,
                                &it.label,
                                it.disabled.is_none(),
                                on,
                            );
                            let r = match &it.disabled {
                                Some(why) => r.on_hover_text(format!("{}: {why}", it.label)),
                                None if !it.hint.is_empty() => r.on_hover_text(&it.hint),
                                None => r,
                            };
                            if r.clicked() {
                                self.run_item(&it.key);
                                ui.close();
                            }
                        });
                    }
                    ui.separator();
                    if ui.button(&reset).clicked() {
                        ribbon::edit_pins(
                            space,
                            &tab.name,
                            g,
                            &mut self.win.layout,
                            PinEdit::Reset,
                        );
                    }
                });
            }
            // まとまりの区切り
            ui.painter().vline(
                rect.right() - 0.5,
                rect.y_range(),
                egui::Stroke::new(1.0, l.pal.edge),
            );
        }
        // 入りきらなかったまとまりは、右端の「その他」から選ぶ
        let hidden: Vec<&ribbon::Group> = tab
            .groups
            .iter()
            .zip(&fold)
            .filter(|(_, f)| **f == ribbon::Fold::Hidden)
            .map(|(g, _)| g)
            .collect();
        if !hidden.is_empty() {
            let lit = hidden
                .iter()
                .any(|g| g.items.iter().any(|i| Some(&i.key) == active.as_ref()));
            let (tile, label) = self.more_group(ui, lit);
            for r in [tile, label] {
                egui::Popup::menu(&r).show(|ui| {
                    for g in &hidden {
                        let name = self.win.t_or(ribbon::group_id(&g.name), &g.name);
                        let icon = g.items.first().map_or("", |i| i.icon.as_str());
                        let lit = g.items.iter().any(|i| Some(&i.key) == active.as_ref());
                        let row = sub_menu_row(ui, icon, &name, lit);
                        egui::containers::menu::SubMenu::new().show(ui, &row, |ui| {
                            for it in &g.items {
                                let on = Some(&it.key) == active.as_ref();
                                let r = theme::menu_item(
                                    ui,
                                    &it.icon,
                                    &it.label,
                                    it.disabled.is_none(),
                                    on,
                                );
                                let r = match &it.disabled {
                                    Some(why) => r.on_hover_text(format!("{}: {why}", it.label)),
                                    None if !it.hint.is_empty() => r.on_hover_text(&it.hint),
                                    None => r,
                                };
                                if r.clicked() {
                                    self.run_item(&it.key);
                                    ui.close();
                                }
                            }
                        });
                    }
                });
            }
        }
    }

    /// 右端の「その他」のまとまり(アイコンだけのボタンと、下の名前 ▾)。押す所の応答を返す
    /// (どちらを押しても同じ一覧を出す)。`lit` は中の道具を使っている時。
    pub(super) fn more_group(
        &mut self,
        ui: &mut egui::Ui,
        lit: bool,
    ) -> (egui::Response, egui::Response) {
        let l = theme::look(ui.ctx());
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(more_width(ui), self.band_height()),
            egui::Sense::hover(),
        );
        let mut col = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(egui::vec2(4.0, 0.0)))
                .layout(egui::Layout::top_down(egui::Align::Center)),
        );
        col.add_space(3.0);
        let more = self.win.t("toolbar-group-more");
        let tile = icon_tile(
            &mut col,
            "chevr",
            &more,
            true,
            lit,
            self.win.layout.appearance.tool_labels,
        )
        .on_hover_text(self.win.t("toolbar-group-more-tip"));
        let label = group_label(ui, rect, "more_group", &more, lit);
        ui.painter().vline(
            rect.right() - 0.5,
            rect.y_range(),
            egui::Stroke::new(1.0, l.pal.edge),
        );
        (tile, label)
    }

    /// ツールバーのまとまりの帯の高さ(名前を出さない設定なら、アイコンだけのボタンの高さにする)。
    pub(crate) fn band_height(&self) -> f32 {
        if self.win.layout.appearance.tool_labels {
            theme::band_height()
        } else {
            let status = if self.picking_body_field() {
                theme::fs(20.0)
            } else {
                0.0
            };
            3.0 + icon_tile_size(false).y + theme::fs(19.0) + status
        }
    }

    /// ボディを受ける欄がクリックを受け付けている状態。普段の選ぶ種類とは別に扱う。
    fn picking_body_field(&self) -> bool {
        self.field_filter().is_some_and(|(now, _)| now.body)
    }

    /// どのタブにも右端に出す「選択」のまとまり: 選べる種類(頂点・辺・面・ボディ)を常に見せ、
    /// 押して替える(中身はビューの左上・Ctrl+右クリックと同じ)。
    /// 選ぶ欄が受けている間は、その欄が受ける種類を見せる。
    fn select_group(&mut self, ui: &mut egui::Ui) {
        let l = theme::look(ui.ctx());
        const KINDS: [&str; 4] = [
            "toolbar-pick-vertex",
            "toolbar-pick-edge",
            "toolbar-pick-face",
            "toolbar-pick-body",
        ];
        let names: Vec<String> = KINDS.iter().map(|id| self.win.t(id)).collect();
        let items: Vec<(&str, bool, bool)> = self
            .pick_filter_items()
            .iter()
            .zip(&names)
            .map(|((_, on, enabled), name)| (name.as_str(), *on, *enabled))
            .collect();
        let field = self.active_pick_field().map(|(entry, f)| {
            crate::field_text::translate(&self.win.words(), &entry.type_id, &f, "").label
        });
        let body_field = self.picking_body_field();
        let labels = self.win.layout.appearance.tool_labels;
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(196.0 * theme::band(), self.band_height()),
            egui::Sense::hover(),
        );
        let mut col = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(egui::vec2(8.0, 0.0)))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        col.add_space(if labels { 9.0 } else { 8.0 });
        if let Some(k) = theme::segmented(&mut col, &items, l.pal.panel) {
            self.toggle_pick_kind(k);
        }
        // 欄で部品を選ぶ状態は、アイコンだけの帯でも名前と終了のボタンを残す。
        if labels || body_field {
            col.add_space(3.0);
            let now = match &field {
                Some(label) if body_field => self
                    .win
                    .t_with("toolbar-pick-now-part", &[("name", label.as_str().into())]),
                Some(label) => self
                    .win
                    .t_with("toolbar-pick-field", &[("name", label.as_str().into())]),
                None if self.win.pick_filter.body => self.win.t("toolbar-pick-now-body"),
                None => self.win.t("toolbar-pick-now-mixed"),
            };
            if body_field {
                let finish = self.win.t("toolbar-pick-stop-part");
                col.horizontal(|ui| {
                    let button_width = theme::fs(22.0);
                    let width = (ui.available_width() - button_width - ui.spacing().item_spacing.x)
                        .max(1.0);
                    ui.add_sized(
                        egui::vec2(width, ui.spacing().interact_size.y),
                        egui::Label::new(
                            egui::RichText::new(&now)
                                .size(theme::fs(10.5))
                                .color(l.pal.ink2),
                        )
                        .truncate(),
                    )
                    .on_hover_text(&now);
                    let r = ui.small_button("×").on_hover_text(&finish);
                    r.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &finish)
                    });
                    if r.clicked() {
                        self.leave_pick_field();
                    }
                });
            } else {
                col.label(
                    egui::RichText::new(now)
                        .size(theme::fs(10.5))
                        .color(l.pal.ink3),
                );
            }
        }
        let title = self.win.t("toolbar-group-select");
        let r = group_label(ui, rect, "select_group", &title, false)
            .on_hover_text(self.win.t("toolbar-group-select-tip"));
        let texts: Vec<String> = KINDS
            .iter()
            .map(|id| self.win.t(&format!("{id}-check")))
            .collect();
        let all = self.win.t("toolbar-pick-all");
        egui::Popup::menu(&r).show(|ui| {
            for (k, (_, on, enabled)) in self.pick_filter_items().into_iter().enumerate() {
                let mut v = on;
                let text = texts[k].clone();
                if ui
                    .add_enabled(enabled, egui::Checkbox::new(&mut v, text))
                    .clicked()
                {
                    self.toggle_pick_kind(k);
                }
            }
            ui.separator();
            if ui.button(&all).clicked() {
                self.reset_pick_filter();
                ui.close();
            }
        });
        ui.painter().vline(
            rect.right() - 0.5,
            rect.y_range(),
            egui::Stroke::new(1.0, l.pal.edge),
        );
    }

    /// まとまりの大きなボタン 1 つ(右クリックで並びを変える)。`compact` はアイコンだけ、
    /// `tall` は名前つきのボタンと同じ高さにする。
    #[allow(clippy::too_many_arguments)]
    fn command(
        &mut self,
        ui: &mut egui::Ui,
        it: &Item,
        tab: &str,
        g: &ribbon::Group,
        on: bool,
        compact: bool,
        tall: bool,
    ) {
        let space = self.win.layout.space;
        let r = tool_tile(
            ui,
            &it.icon,
            &it.label,
            it.disabled.is_none(),
            on,
            compact,
            tall,
        );
        let hint = tile_hint(&it.label, &it.hint, compact);
        let r = match &it.disabled {
            Some(why) => r.on_hover_text(format!("{}: {why}", it.label)),
            None if !hint.is_empty() => r.on_hover_text(hint),
            None => r,
        };
        if r.clicked() {
            self.run_item(&it.key);
        }
        let texts = [
            "toolbar-move-left",
            "toolbar-move-right",
            "toolbar-unpin",
            "toolbar-group-reset",
        ]
        .map(|id| self.win.t(id));
        r.right_menu(|ui| {
            let mut e = None;
            if crate::theme::menu_row(ui, "chevl", &texts[0], "", true).clicked() {
                e = Some(PinEdit::Move(it.key.clone(), -1));
            }
            if crate::theme::menu_row(ui, "chevr", &texts[1], "", true).clicked() {
                e = Some(PinEdit::Move(it.key.clone(), 1));
            }
            ui.separator();
            if crate::theme::menu_row(ui, "pin", &texts[2], "", true).clicked() {
                e = Some(PinEdit::Unpin(it.key.clone()));
            }
            if crate::theme::menu_row(ui, "undo", &texts[3], "", true).clicked() {
                e = Some(PinEdit::Reset);
            }
            if let Some(e) = e {
                ribbon::edit_pins(space, tab, g, &mut self.win.layout, e);
                ui.close();
            }
        });
    }

    /// 中央のビューを持つ編集器(スケッチ)で直している間の、帯の道具のまとまり(帯に並べる順)。
    /// 直していなければ空。
    pub fn view_tool_groups(&mut self) -> Vec<crate::ToolGroup> {
        let Some(type_id) = self.view_editor_type() else {
            return Vec::new();
        };
        let Some(entry) = self
            .cur
            .selected
            .and_then(|id| self.cur.work.doc().data().feature(id).cloned())
        else {
            return Vec::new();
        };
        let mut groups = self.with_editors(|eds, host| {
            eds.iter()
                .find(|ed| ed.handles(&type_id) && ed.takes_view())
                .map(|ed| ed.tools(host, &entry))
                .unwrap_or_default()
        });
        ribbon::sort_known(&mut groups, |g| g.name.as_str(), ribbon::SKETCH_ORDER);
        groups
    }
}

/// まとまりの名前と ▾(帯の下の行)。押すと一覧を出すための応答を返す。`lit` は中の道具を
/// 使っている時(作業空間の色にして下線を引く)。
pub(super) fn group_label(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id: impl std::hash::Hash + std::fmt::Debug,
    name: &str,
    lit: bool,
) -> egui::Response {
    let l = theme::look(ui.ctx());
    let lr = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 4.0, rect.bottom() - theme::fs(19.0)),
        egui::pos2(rect.right() - 4.0, rect.bottom() - 1.0),
    );
    let r = ui.interact(lr, ui.id().with(id), egui::Sense::click());
    let label = format!("{name} ▾");
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&r));
    let accent = l.ws();
    let c = if r.hovered() || open || lit {
        accent
    } else {
        l.pal.ink2
    };
    if r.hovered() || open {
        ui.painter().rect_filled(
            lr,
            crate::theme::corner(3.0),
            theme::mix(l.pal.panel, accent, 0.15),
        );
    }
    let gl = ui
        .painter()
        .layout_no_wrap(name.to_string(), crate::theme::prop(10.5), c);
    let total = gl.size().x + 3.0 + 10.0;
    let x0 = lr.center().x - total / 2.0;
    ui.painter().galley(
        egui::pos2(x0, lr.center().y - gl.size().y / 2.0),
        gl.clone(),
        c,
    );
    if lit {
        ui.painter().line_segment(
            [
                egui::pos2(x0, lr.bottom() - 1.0),
                egui::pos2(x0 + gl.size().x, lr.bottom() - 1.0),
            ],
            egui::Stroke::new(2.0, accent),
        );
    }
    crate::icons::paint(
        ui.painter(),
        egui::Rect::from_center_size(
            egui::pos2(x0 + gl.size().x + 3.0 + 5.0, lr.center().y + 1.0),
            egui::vec2(10.0, 10.0),
        ),
        "chev",
        c,
        1.6,
    );
    r
}

/// 帯に入りきらないまとまりを入れる、右端のまとまりの名前。
const MORE: &str = "その他";

/// アイコンだけのボタンの大きさ。`tall` は名前つきのボタンと同じ高さ(同じ帯に並べる時)。
fn icon_tile_size(tall: bool) -> egui::Vec2 {
    let ic = theme::icon_px(26.0);
    egui::vec2(
        ic + 14.0,
        if tall {
            theme::tile_size().y
        } else {
            ic + 12.0
        },
    )
}

/// ツールバーのボタンの幅(`compact` はアイコンだけ)。
pub(super) fn tile_width(compact: bool) -> f32 {
    if compact {
        icon_tile_size(false).x
    } else {
        theme::tile_size().x
    }
}

/// まとまりの名前と ▾ の幅。ボタンの列がこれより狭ければ名前の幅に広げる(隣の名前と重ならないように)。
pub(super) fn name_width(ui: &egui::Ui, name: &str) -> f32 {
    ui.painter()
        .layout_no_wrap(name.to_string(), theme::prop(10.5), egui::Color32::WHITE)
        .size()
        .x
        + 3.0
        + 10.0
        + 16.0
}

/// まとまりの出し方ごとの幅。`n` は前に出すボタンの数、`labels` はボタンに名前を出す設定。
pub(super) fn group_widths(ui: &egui::Ui, name: &str, n: usize, labels: bool) -> ribbon::Widths {
    let w = |n: usize, compact: bool| {
        let n = n.max(1) as f32;
        (n * tile_width(compact) + (n - 1.0) * 2.0 + 8.0).max(name_width(ui, name))
    };
    ribbon::Widths {
        full: w(n, !labels),
        compact: w(n, true),
        collapsed: w(1, true),
    }
}

/// 畳んだまとまりのボタン(まとまりの道具のアイコン 1 つ)。押すとまとまりの一覧を出す。
/// `win` は名前と説明の文言に使う。
pub(super) fn group_tile(
    ui: &mut egui::Ui,
    win: &crate::window_state::WinState,
    icon: &str,
    name: &str,
    lit: bool,
    tall: bool,
) -> egui::Response {
    // 名前はまとまりの名前と分ける(台本でまとまりの道具と見分ける)
    let r = icon_tile(
        ui,
        icon,
        &win.t_with("toolbar-collapsed", &[("name", name.into())]),
        true,
        lit,
        tall,
    );
    // 右下の ▾ で、押すと道具でなく一覧が出ることを見せる
    let l = theme::look(ui.ctx());
    crate::icons::paint(
        ui.painter(),
        egui::Rect::from_center_size(
            r.rect.right_bottom() + egui::vec2(-7.0, -8.0),
            egui::vec2(9.0, 9.0),
        ),
        "chev",
        if lit { l.ws() } else { l.pal.ink2 },
        1.6,
    );
    r.on_hover_text(win.t_with("toolbar-collapsed-tip", &[("name", name.into())]))
}

/// 右端の「その他」のまとまりの幅。
pub(super) fn more_width(ui: &egui::Ui) -> f32 {
    (tile_width(true) + 8.0).max(name_width(ui, MORE))
}

/// ツールバーのボタン(`compact` はアイコンだけ、`tall` は名前つきのボタンと同じ高さ)。
#[allow(clippy::too_many_arguments)]
pub(super) fn tool_tile(
    ui: &mut egui::Ui,
    icon: &str,
    label: &str,
    enabled: bool,
    on: bool,
    compact: bool,
    tall: bool,
) -> egui::Response {
    if compact {
        icon_tile(ui, icon, label, enabled, on, tall)
    } else {
        theme::command_tile(ui, icon, label, enabled, false, on)
    }
}

/// 名前を出さないツールバーのボタン(アイコンだけ。名前は読み上げと、指を置いた時の説明に出す)。
/// 使っている道具は [`theme::command_tile`] と同じく、淡い塗りと枠、下の線で囲む。
fn icon_tile(
    ui: &mut egui::Ui,
    icon: &str,
    label: &str,
    enabled: bool,
    on: bool,
    tall: bool,
) -> egui::Response {
    let l = theme::look(ui.ctx());
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, r) = ui.allocate_exact_size(icon_tile_size(tall), sense);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let accent = l.ws();
        if on {
            p.rect(
                rect,
                theme::corner(4.0),
                theme::mix(l.pal.panel, accent, 0.2),
                egui::Stroke::new(1.5, accent),
                egui::StrokeKind::Inside,
            );
            p.line_segment(
                [
                    rect.left_bottom() + egui::vec2(6.0, -2.0),
                    rect.right_bottom() + egui::vec2(-6.0, -2.0),
                ],
                egui::Stroke::new(3.0, accent),
            );
        } else if enabled && r.hovered() {
            p.rect_filled(rect, theme::corner(4.0), l.pal.chrome2);
        }
        crate::icons::paint_exact_inks(
            p,
            egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(theme::icon_px(26.0))),
            icon,
            &theme::tool_inks(&l, enabled, on),
            1.5,
        );
    }
    r
}

/// 2 段の小さなボタンの高さ(大きなボタンの高さの半分。名前を出す帯ではアイコンが 19 px)。
fn small_height(labels: bool) -> f32 {
    let tile = if labels {
        theme::tile_size().y
    } else {
        icon_tile_size(false).y
    };
    (tile - 2.0) / 2.0
}

/// 2 段の小さなボタンの列の幅(前のすき間を含む。`n` は道具の数、無ければ 0)。
pub(super) fn small_width(n: usize, labels: bool) -> f32 {
    if n == 0 {
        return 0.0;
    }
    let cols = n.div_ceil(2) as f32;
    6.0 + cols * (small_height(labels) + 4.0) + (cols - 1.0)
}

/// 道具を 2 段の小さなボタンで並べる(上の段から左から右へ)。押された道具の id を返す。
pub(super) fn small_grid(
    ui: &mut egui::Ui,
    tools: &[crate::EditorTool],
    labels: bool,
) -> Option<String> {
    if tools.is_empty() {
        return None;
    }
    let h = small_height(labels);
    let cols = tools.len().div_ceil(2);
    let mut used = None;
    ui.add_space(6.0 - ui.spacing().item_spacing.x);
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(1.0, 2.0);
        for row in tools.chunks(cols) {
            ui.horizontal(|ui| {
                for t in row {
                    let r = small_tile(ui, &t.icon, &t.label, t.enabled, t.on, h)
                        .on_hover_text(tile_hint(&t.label, &t.hint, true));
                    if r.clicked() {
                        used = Some(t.id.clone());
                    }
                }
            });
        }
    });
    used
}

/// 2 段に並べる小さなボタン(アイコンだけ。名前は読み上げと、指を置いた時の説明に出す)。
/// 使っている道具は大きなボタンと同じく、淡い塗りと作業空間の色の枠で囲む。
fn small_tile(
    ui: &mut egui::Ui,
    icon: &str,
    label: &str,
    enabled: bool,
    on: bool,
    h: f32,
) -> egui::Response {
    let l = theme::look(ui.ctx());
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, r) = ui.allocate_exact_size(egui::vec2(h + 4.0, h), sense);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        if on {
            p.rect(
                rect,
                theme::corner(4.0),
                theme::mix(l.pal.panel, l.ws(), 0.2),
                egui::Stroke::new(1.5, l.ws()),
                egui::StrokeKind::Inside,
            );
        } else if enabled && r.hovered() {
            p.rect_filled(rect, theme::corner(4.0), l.pal.chrome2);
        }
        crate::icons::paint_exact_inks(
            p,
            egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(h - 7.0)),
            icon,
            &theme::tool_inks(&l, enabled, on),
            1.5,
        );
    }
    r
}

/// ボタンに指を置いた時の説明。名前を出していないボタンは名前も出す。
pub(super) fn tile_hint(label: &str, hint: &str, compact: bool) -> String {
    match (compact, hint.is_empty()) {
        (true, true) => label.to_string(),
        (true, false) => format!("{label}\n{hint}"),
        (false, _) => hint.to_string(),
    }
}

/// 横に一覧を出す行(アイコン・名前・▸。「その他」の一覧のまとまり)。
pub(super) fn sub_menu_row(ui: &mut egui::Ui, icon: &str, name: &str, lit: bool) -> egui::Response {
    let l = theme::look(ui.ctx());
    let r = theme::menu_item(ui, icon, name, true, lit);
    crate::icons::paint(
        ui.painter(),
        egui::Rect::from_center_size(
            egui::pos2(r.rect.right() - 10.0, r.rect.center().y),
            egui::vec2(12.0, 12.0),
        ),
        "chevr",
        l.pal.ink2,
        1.6,
    );
    r
}
