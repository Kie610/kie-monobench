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
                label: "計測".into(),
                icon: "measure".into(),
                hint: "M。点・辺・面をクリックして、距離・角度・径・面積を測る".into(),
                disabled: None,
                front: true,
                formerly: Vec::new(),
            },
        );
        let section = (
            check.to_string(),
            Item {
                key: "u:section".into(),
                label: "断面解析".into(),
                icon: "section".into(),
                hint: "平面で切って中を見る(形は変えない)。平らな面を選んでから押すと、その面に平行に切る"
                    .into(),
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
                label: "プレス/プル".into(),
                icon: "presspull".into(),
                hint: "選んだ物で決める: スケッチの形・平らな面は押し出し、辺はフィレット、フィレット・面取り・穴の面はその機能を直す".into(),
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
                        label: "シーンの設定".into(),
                        icon: "gear".into(),
                        hint: "環境の光(スタジオ・屋外)と明るさ・回転、背景(環境・単色・透明)、グラウンド面、カメラの焦点距離・露出・被写界深度、縦横比、絵と動画の大きさと長さ。設定の「レンダリング」".into(),
                        disabled: None,
                        front: true,
                        formerly: Vec::new(),
                    },
                ));
                items.push((
                    "設定".into(),
                    Item {
                        key: "u:appearance".into(),
                        label: "外観".into(),
                        icon: "swatch".into(),
                        hint: "A。ボディ・部品・面に外観(色・金属・粗さ)を付ける窓を開く。ライブラリを分類と名前で探し、このデザイン内の外観を直す".into(),
                        disabled: None,
                        front: true,
                        formerly: Vec::new(),
                    },
                ));
                items.push((
                    "キャンバス内レンダリング".into(),
                    Item {
                        key: "u:canvas_render".into(),
                        label: "キャンバス内レンダリング".into(),
                        icon: "render".into(),
                        hint: "今のビューを光線追跡で描き、止まっている間に少しずつきれいにする(影と映り込み)。もう一度押すと止める".into(),
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
                    let d = self.win.registry.descriptor(&type_id);
                    // 同じ名前の操作(クリックで作る流れ)があれば、欄で指定して足す方だと分かる名前にする
                    let label = if action_labels.contains(&label) {
                        format!("{label}(欄で指定)")
                    } else {
                        label
                    };
                    items.push((
                        d.as_ref().map(|d| d.menu.clone()).unwrap_or_default(),
                        Item {
                            key: format!("f:{type_id}"),
                            label,
                            icon: d.as_ref().map(|d| d.icon.clone()).unwrap_or_default(),
                            hint: d.as_ref().map(|d| d.hint.clone()).unwrap_or_default(),
                            disabled: why,
                            front: d.is_some_and(|d| d.pinned),
                            formerly: Vec::new(),
                        },
                    ));
                }
                items.push((
                    "修正".into(),
                    Item {
                        key: "u:params".into(),
                        label: "パラメータ".into(),
                        icon: "param".into(),
                        hint: "Ctrl+P。右の編集パネルにパラメータの一覧を出す".into(),
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
                    let d = self.win.registry.descriptor(&type_id);
                    items.push((
                        d.as_ref().map(|d| d.menu.clone()).unwrap_or_default(),
                        Item {
                            key: format!("f:{type_id}"),
                            label,
                            icon: d.as_ref().map(|d| d.icon.clone()).unwrap_or_default(),
                            hint: d.as_ref().map(|d| d.hint.clone()).unwrap_or_default(),
                            disabled: why,
                            front: d.is_some_and(|d| d.pinned),
                            formerly: Vec::new(),
                        },
                    ));
                }
            }
            Space::Machining => {
                items.push((
                    "管理".into(),
                    Item {
                        key: "u:tools".into(),
                        label: "工具ライブラリ".into(),
                        icon: "tools".into(),
                        hint: "工具の一覧を選ぶ(なければ足す)".into(),
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
                        label: "機械の設定".into(),
                        icon: "machine".into(),
                        hint: "機械の値(作業範囲、回転数、コレット、ベッド)。設定の「機械」。複数の機械を持つマシンライブラリはまだ無い".into(),
                        disabled: None,
                        front: true,
                        formerly: Vec::new(),
                    },
                ));
            }
        }
        ribbon::tabs(space, items, &self.win.layout)
    }

    /// ツールバーの項目のボタン。押したら実行して真を返す。
    pub(crate) fn ribbon_item_button(&mut self, ui: &mut egui::Ui, it: &Item) -> bool {
        let r = ui.add_enabled(it.disabled.is_none(), egui::Button::new(it.label.as_str()));
        let r = match &it.disabled {
            Some(why) => r.on_disabled_hover_text(why),
            None => match (self.key_of_item(&it.key), it.hint.is_empty()) {
                (Some(k), true) => r.on_hover_text(format!("キー: {k}")),
                (Some(k), false) => r.on_hover_text(format!("{}\nキー: {k}", it.hint)),
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
                    self.win.layout.panel_open = true;
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
            None => self.win.message = Some("この作業空間では使えないキーです".into()),
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
                            tab_button(ui, "スケッチ", true, true);
                        }
                        for t in &tabs {
                            let r = tab_button(ui, &t.name, self.win.tab == t.name, false);
                            if r.clicked() {
                                self.win.tab = t.name.clone();
                            }
                            r.right_menu(|ui| {
                                if crate::theme::menu_row(ui, "chevl", "左へ", "", true).clicked()
                                {
                                    ribbon::move_tab(
                                        self.win.layout.space,
                                        &tabs,
                                        &t.name,
                                        -1,
                                        &mut self.win.layout,
                                    );
                                    ui.close();
                                }
                                if crate::theme::menu_row(ui, "chevr", "右へ", "", true).clicked()
                                {
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
                                if crate::theme::menu_row(
                                    ui,
                                    "undo",
                                    "タブの順を既定に戻す",
                                    "",
                                    true,
                                )
                                .clicked()
                                {
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
            space.label().to_string(),
            crate::theme::prop(16.0),
            l.ws(),
        );
        let head =
            ui.painter()
                .layout_no_wrap("作業空間".into(), crate::theme::prop(10.5), l.pal.ink2);
        let widest = Space::ALL
            .iter()
            .map(|s| {
                ui.painter()
                    .layout_no_wrap(s.label().to_string(), crate::theme::prop(16.0), l.ws())
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
        let label = format!("作業空間: {}", space.label());
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
        let r = r.on_hover_text("作業空間: 設計(形を作る)と加工(経路を作る)");
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
                    let g = ui.painter().layout_no_wrap(
                        s.label().to_string(),
                        crate::theme::prop(16.0),
                        c,
                    );
                    let (row, rr) = ui.allocate_exact_size(
                        egui::vec2(row_w, g.size().y + 20.0),
                        egui::Sense::click(),
                    );
                    rr.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::SelectableLabel,
                            true,
                            s == space,
                            s.label(),
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
        let widths: Vec<ribbon::Widths> = tab
            .groups
            .iter()
            .zip(&fronts)
            .map(|(g, f)| {
                let on = f.iter().position(|i| Some(&i.key) == active.as_ref());
                group_widths(ui, &g.name, f.len(), on, labels)
            })
            .collect();
        let select_w = 196.0 * theme::band();
        let fold = ribbon::fit(&widths, select_w, more_width(ui), ui.available_width());
        let tall = labels;
        for ((g, front), f) in tab.groups.iter().zip(&fronts).zip(&fold) {
            let shown: Vec<&Item> = match f {
                ribbon::Fold::Hidden => continue,
                ribbon::Fold::Compact(n) => front
                    .iter()
                    .enumerate()
                    .filter(|(k, i)| *k < *n || Some(&i.key) == active.as_ref())
                    .map(|(_, i)| i)
                    .collect(),
                _ => front.iter().collect(),
            };
            let compact = !labels || *f != ribbon::Fold::Full;
            let n = shown.len().max(1) as f32;
            let w = (n * tile_width(compact) + (n - 1.0) * 2.0 + 8.0).max(name_width(ui, &g.name));
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(w, self.band_height()), egui::Sense::hover());
            let mut col = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(rect.shrink2(egui::vec2(4.0, 0.0)))
                    .layout(egui::Layout::top_down(egui::Align::Center)),
            );
            col.spacing_mut().item_spacing = egui::vec2(2.0, 0.0);
            col.add_space(3.0);
            col.horizontal(|ui| {
                for it in &shown {
                    let on = Some(&it.key) == active.as_ref();
                    self.command(ui, it, &tab.name, g, on, compact, tall);
                }
            });
            // まとまりの名前と ▾ の一覧(ピンで前に出す)
            let lit = g.items.iter().any(|i| Some(&i.key) == active.as_ref());
            let r = group_label(ui, rect, ("glabel", &tab.name, &g.name), &g.name, lit);
            egui::Popup::menu(&r).show(|ui| {
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
                            if pinned {
                                "前から外す"
                            } else {
                                "前に出す"
                            },
                            true,
                            pinned,
                        )
                        .on_hover_text(if pinned {
                            "前から外す"
                        } else {
                            "前に出す"
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
                        let r =
                            theme::menu_item(ui, &it.icon, &it.label, it.disabled.is_none(), on);
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
                if ui.button("前の並びを既定に戻す").clicked() {
                    ribbon::edit_pins(space, &tab.name, g, &mut self.win.layout, PinEdit::Reset);
                }
            });
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
                        let icon = g.items.first().map_or("", |i| i.icon.as_str());
                        let lit = g.items.iter().any(|i| Some(&i.key) == active.as_ref());
                        let row = sub_menu_row(ui, icon, &g.name, lit);
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
        let tile = icon_tile(
            &mut col,
            "chevr",
            MORE,
            true,
            lit,
            self.win.layout.appearance.tool_labels,
        )
        .on_hover_text(
            "帯に入りきらないまとまり。窓を広げるか、ボタンの名前を出さない設定にすると帯に並ぶ",
        );
        let label = group_label(ui, rect, "more_group", MORE, lit);
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
            3.0 + icon_tile_size(false).y + theme::fs(19.0)
        }
    }

    /// どのタブにも右端に出す「選択」のまとまり: 選べる種類(頂点・辺・面・ボディ)を常に見せ、
    /// 押して替える(中身はビューの左上・Ctrl+右クリックと同じ)。
    /// 選ぶ欄が受けている間は、その欄が受ける種類を見せる。
    fn select_group(&mut self, ui: &mut egui::Ui) {
        let l = theme::look(ui.ctx());
        let items = self.pick_filter_items();
        let field = self.active_pick_field().map(|(_, f)| f.label);
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
        // 名前を出さない低い帯には、今の選び方の説明は入れない(▾ の説明に出す)
        if labels {
            col.add_space(3.0);
            let now = match &field {
                Some(label) => format!("欄「{label}」が受ける物"),
                None if self.win.pick_filter.body => "面を押すとボディごと選ぶ".to_string(),
                None => "頂点・辺・面を混ぜて選ぶ".to_string(),
            };
            col.label(
                egui::RichText::new(now)
                    .size(crate::theme::fs(10.5))
                    .color(l.pal.ink3),
            );
        }
        let r = group_label(ui, rect, "select_group", "選択", false)
            .on_hover_text("選べる種類。ボディを入れると、面を押した時にそのボディごと選ぶ");
        egui::Popup::menu(&r).show(|ui| {
            for (k, (label, on, enabled)) in self.pick_filter_items().into_iter().enumerate() {
                let mut v = on;
                let text = if k == 3 {
                    "ボディを選ぶ(面を押すとボディごと)".to_string()
                } else {
                    format!("{label}を選ぶ")
                };
                if ui
                    .add_enabled(enabled, egui::Checkbox::new(&mut v, text))
                    .clicked()
                {
                    self.toggle_pick_kind(k);
                }
            }
            ui.separator();
            if ui.button("すべて選べるようにする").clicked() {
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
        r.right_menu(|ui| {
            let mut e = None;
            if crate::theme::menu_row(ui, "chevl", "左へ", "", true).clicked() {
                e = Some(PinEdit::Move(it.key.clone(), -1));
            }
            if crate::theme::menu_row(ui, "chevr", "右へ", "", true).clicked() {
                e = Some(PinEdit::Move(it.key.clone(), 1));
            }
            ui.separator();
            if crate::theme::menu_row(ui, "pin", "前から外す", "", true).clicked() {
                e = Some(PinEdit::Unpin(it.key.clone()));
            }
            if crate::theme::menu_row(ui, "undo", "このまとまりを既定に戻す", "", true).clicked()
            {
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

/// まとまりの出し方ごとの幅。`n` は前に出すボタンの数、`on` は使っている道具の前の並びの位置
/// (ボタンを減らしても、使っている道具は出す)、`labels` はボタンに名前を出す設定。
pub(super) fn group_widths(
    ui: &egui::Ui,
    name: &str,
    n: usize,
    on: Option<usize>,
    labels: bool,
) -> ribbon::Widths {
    let w = |n: usize, compact: bool| {
        let n = n.max(1) as f32;
        (n * tile_width(compact) + (n - 1.0) * 2.0 + 8.0).max(name_width(ui, name))
    };
    ribbon::Widths {
        full: w(n, !labels),
        compact: (1..=n.max(1))
            .map(|k| w(k + usize::from(on.is_some_and(|p| p >= k)), true))
            .collect(),
    }
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
