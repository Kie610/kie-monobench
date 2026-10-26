//! パラメータの窓(ユーザーのパラメータとモデルのパラメータの表)と、その欄の部品。

use crate::theme::RightMenu as _;
use crate::*;

impl Workspace {
    pub(crate) fn params_ui(&mut self, ui: &mut egui::Ui) {
        // 表のタブ(全部・作った表。U50)。作った表では、その表の行だけを出す
        self.param_tabs_ui(ui);
        // 作った表とお気に入りでは、その表の行を表の並び(手で並べた順)で出す
        let order = self.param_view_rows();
        let shown: Option<HashSet<kmb_core::ParamKey>> =
            order.as_ref().map(|o| o.iter().cloned().collect());
        let rank = |k: &kmb_core::ParamKey| {
            order
                .as_ref()
                .and_then(|o| o.iter().position(|x| x == k))
                .unwrap_or(usize::MAX)
        };
        let menu = self.row_menu();
        let all_users = self.param_users_map();
        let mut row_act: Option<(kmb_core::ParamKey, param_views_ui::RowAct)> = None;
        // 検索と並べ方と、消した物を出すか(画面の記憶に置く)
        let sid = crate::doc_id(ui.ctx(), "params_view");
        let (mut query, mut by_name, mut show_removed): (String, bool, bool) =
            ui.data(|d| d.get_temp(sid)).unwrap_or_default();
        let mut params = self.cur.work.doc().data().params.clone();
        if let Some(s) = &shown {
            params.retain(|p| s.contains(&kmb_core::ParamKey::User { id: p.id }));
            params.sort_by_key(|p| rank(&kmb_core::ParamKey::User { id: p.id }));
        }
        let mut removed = self.removed_rows();
        if params.len() > 3 || !query.is_empty() || !removed.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut query)
                        .hint_text("名前・式で探す")
                        .desired_width(140.0),
                );
                ui.checkbox(&mut by_name, "名前順");
                if !removed.is_empty() {
                    ui.checkbox(
                        &mut show_removed,
                        format!("消した物も出す({})", removed.len()),
                    )
                    .on_hover_text("消したパラメータを薄く出し、「戻す」で戻せるようにする");
                }
            });
        }
        ui.data_mut(|d| d.insert_temp(sid, (query.clone(), by_name, show_removed)));
        let q = query.trim().to_lowercase();
        if !q.is_empty() {
            params.retain(|p| {
                p.name.to_lowercase().contains(&q) || p.expr.to_lowercase().contains(&q)
            });
            removed.retain(|p| {
                p.name.to_lowercase().contains(&q) || p.expr.to_lowercase().contains(&q)
            });
        }
        if by_name {
            params.sort_by(|a, b| a.name.cmp(&b.name));
        }
        let mut commit_expr = None;
        let mut commit_name = None;
        let mut commit_info = None;
        let mut remove = None;
        let err = ui.visuals().error_fg_color;
        // 通らなかった入力の理由(欄の横に出す。打ち直すか通るまで残す)
        // 鍵の元は先に作る(doc_id は記憶を読むので、data_mut の中で呼ぶと止まる)
        let failed = crate::doc_id(ui.ctx(), "param_failed");
        let failed_id = move |id: ParamId| failed.with(id);
        // 幅の狭いパネルでも読めるよう、1 つのパラメータを 2 行にする(1 行目: 名前・式・値・消す、
        // 2 行目: 単位・コメント・使っている所)。欄は大きさを決めて置くので、行どうしの列がそろう。
        // 通らない理由と、広げた使っている所は、その下にパネルの幅で折り返して出す
        let h = ui.spacing().interact_size.y;
        let (w_star, w_name, w_value) = (18.0, 72.0, 64.0);
        // 式の欄はパネルの残りの幅にする(☆ の列を足しても狭いパネルで「削除」が切れないように)
        let w_remove = 44.0;
        let w_expr = (ui.available_width()
            - (w_star + w_name + w_value + w_remove + 4.0 * ui.spacing().item_spacing.x))
            .clamp(64.0, 240.0);
        // 決めた幅の中に左寄せで置く
        let cell = |ui: &mut egui::Ui, w: f32, l: egui::Label| {
            ui.allocate_ui_with_layout(
                egui::vec2(w, h),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_width(w);
                    ui.add(l)
                },
            )
            .inner
        };
        ui.horizontal(|ui| {
            cell(
                ui,
                w_star,
                egui::Label::new(egui::RichText::new("☆").strong()),
            )
            .on_hover_text("お気に入り。☆ を押すと「お気に入り」の表に入る");
            cell(
                ui,
                w_name,
                egui::Label::new(egui::RichText::new("名前").strong()),
            )
            .on_hover_text("2 行目は単位");
            cell(
                ui,
                w_expr,
                egui::Label::new(egui::RichText::new("式").strong()),
            )
            .on_hover_text("2 行目はコメント");
            cell(
                ui,
                w_value,
                egui::Label::new(egui::RichText::new("値").strong()),
            )
            .on_hover_text("2 行目は使っている所");
        });
        for (i, p) in params.iter().enumerate() {
            let fill = if i % 2 == 0 {
                ui.visuals().faint_bg_color
            } else {
                egui::Color32::TRANSPARENT
            };
            // 見えていない行は描かずに前のコマの高さだけ空ける(R15)
            let row = crate::doc_id(ui.ctx(), ("param_row", p.id));
            if skip_row(ui, row) {
                continue;
            }
            let shown_rect = egui::Frame::new().fill(fill).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let key = kmb_core::ParamKey::User { id: p.id };
                    if let Some(on) = star_button(ui, w_star, h, menu.favorites.contains(&key)) {
                        row_act = Some((key.clone(), param_views_ui::RowAct::Favorite(on)));
                    }
                    let name = FieldEdit::text(self.cur.name_edits.entry(p.id), &p.name);
                    // 欄の id は場所でなくパラメータで決める(下の理由や一覧の出入りで入力中の欄が変わらない)
                    let r = ui.add_sized(
                        [w_name, h],
                        egui::TextEdit::singleline(name).id(crate::doc_id(ui.ctx(), ("param_name", p.id))),
                    );
                    if r.lost_focus() && *name != p.name {
                        commit_name = Some((p.id, name.clone()));
                    }
                    r.right_menu(|ui| {
                        if let Some(a) = menu.show(ui, &key) {
                            row_act = Some((key.clone(), a));
                        }
                    });
                    let expr = FieldEdit::text(self.cur.expr_edits.entry(p.id), &p.expr);
                    // 打っている名前の候補を欄の下に出す(U51)
                    let c = complete::begin(ui, crate::doc_id(ui.ctx(), ("param_expr", p.id)));
                    let r = ui.add_sized([w_expr, h], c.filter(egui::TextEdit::singleline(expr)));
                    c.end(ui, &r, expr, &[]);
                    if r.changed() {
                        ui.data_mut(|d| d.remove::<String>(failed_id(p.id)));
                    }
                    if r.lost_focus() && *expr != p.expr {
                        commit_expr = Some((p.id, expr.clone()));
                    }
                    let value = match self.cur.work.report().params.iter().find(|r| r.id == p.id) {
                        Some(r) => match &r.status {
                            ParamStatus::Ok { value } => egui::RichText::new(self.shown(*value)),
                            ParamStatus::Error { .. } => egui::RichText::new("誤り").color(err),
                        },
                        None => egui::RichText::new(""),
                    };
                    cell(ui, w_value, egui::Label::new(value).truncate());
                    // 使っている式があれば消せない
                    let users = all_users.get(&p.name).cloned().unwrap_or_default();
                    let r = ui.add_enabled(users.is_empty(), egui::Button::new("削除").small());
                    let r = if users.is_empty() {
                        r.on_hover_text("このパラメータを消す(取り消せる)")
                    } else {
                        r.on_disabled_hover_text(format!(
                            "{}が使っているので消せません",
                            users.join("、")
                        ))
                    };
                    if r.clicked() {
                        remove = Some(p.id);
                    }
                });
                // 2 行目: 単位(数だけの式に補い、式の次元を確かめる)・
                // コメント・使っている所(U49・U53)
                let users = all_users.get(&p.name).cloned().unwrap_or_default();
                let open = ui
                    .horizontal(|ui| {
                        ui.add_space(w_star + ui.spacing().item_spacing.x);
                        let mut unit = p.unit;
                        ui.allocate_ui_with_layout(
                            egui::vec2(w_name, h),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.set_min_width(w_name);
                                egui::ComboBox::from_id_salt(("param_unit", p.id))
                                    .width(w_name - 8.0)
                                    .selected_text(self.param_unit_label(unit))
                                    .show_ui(ui, |ui| {
                                        for u in kmb_core::ParamUnit::ALL {
                                            ui.selectable_value(&mut unit, u, self.param_unit_label(u));
                                        }
                                    })
                                    .response
                                    .on_hover_text("単位: 数だけを打った時に補い、式の値がこの単位に合わなければ誤りにする(「未指定」は式のまま)");
                            },
                        );
                        if unit != p.unit {
                            commit_info = Some((p.id, p.comment.clone(), unit));
                        }
                        let key = crate::doc_id(ui.ctx(), ("param_comment", p.id));
                        let mut comment = ui
                            .data(|d| d.get_temp::<FieldEdit>(key))
                            .map_or_else(|| p.comment.clone(), |e| e.text_on(&p.comment));
                        let r = ui.add_sized(
                            [w_expr, h],
                            egui::TextEdit::singleline(&mut comment)
                                .id(crate::doc_id(ui.ctx(), ("param_comment_edit", p.id)))
                                .hint_text("コメント"),
                        );
                        if r.lost_focus() && comment != p.comment {
                            commit_info = Some((p.id, comment.clone(), p.unit));
                        }
                        ui.data_mut(|d| d.insert_temp(key, FieldEdit::new(&p.comment, comment)));
                        users_cell(ui, crate::doc_id(ui.ctx(), ("param_users", p.id)), &users)
                    })
                    .inner;
                // 通らない式の理由は、吹き出しにせず欄の下に出す(U48)
                let failed: Option<String> = ui.data(|d| d.get_temp(failed_id(p.id)));
                let status = self.cur.work.report().params.iter().find(|r| r.id == p.id).map(|r| &r.status);
                match (failed, status) {
                    (Some(why), _) => reason_label(ui, err, &format!("通りません: {why}")),
                    (None, Some(ParamStatus::Error { message })) => reason_label(ui, err, message),
                    _ => {}
                }
                if open {
                    users_list(ui, &users);
                }
            });
            ui.data_mut(|d| d.insert_temp(row, shown_rect.response.rect));
        }
        let cols = Cols {
            h,
            star: w_star,
            name: w_name,
            expr: w_expr,
            value: w_value,
        };
        self.new_param_row(ui, &cols);
        // 消したパラメータ(「消した物も出す」の時だけ、薄い色で。N2)
        let mut restore = None;
        if show_removed {
            for p in &removed {
                ui.horizontal(|ui| {
                    ui.add_space(w_star + ui.spacing().item_spacing.x);
                    let faint = |t: &str| egui::RichText::new(t).color(self.win.look.pal.ink3);
                    cell(ui, w_name, egui::Label::new(faint(&p.name)).truncate())
                        .on_hover_text("消したパラメータ");
                    cell(ui, w_expr, egui::Label::new(faint(&p.expr)).truncate());
                    cell(ui, w_value, egui::Label::new(faint("消した")));
                    let taken = self.cur.work.doc().data().param_by_name(&p.name).is_some()
                        || self.cur.work.report().model_params.iter().any(|m| m.name == p.name);
                    let r = ui.add_enabled(!taken, egui::Button::new("戻す").small());
                    let r = if taken {
                        r.on_disabled_hover_text(format!(
                            "同じ名前 {} が使われているので戻せません(今ある方の名前を変えると戻せます)",
                            p.name
                        ))
                    } else {
                        r.on_hover_text("このパラメータを戻す(取り消せる)")
                    };
                    if r.clicked() {
                        restore = Some(p.id);
                    }
                });
            }
        }
        if let Some(id) = restore {
            self.restore_param(id);
        }
        // 失敗した時は入力をそのまま残し、理由を欄の横に出す
        if let Some((id, expr)) = commit_expr {
            // 長さのパラメータに数だけを打てば、文書の単位を付ける(候補 19)
            let length = self
                .cur
                .work
                .doc()
                .data()
                .params
                .iter()
                .any(|p| p.id == id && p.unit == kmb_core::ParamUnit::Length);
            let expr = if length {
                self.in_doc_unit(&expr)
            } else {
                expr
            };
            if !self.set_param_expr(id, &expr) {
                // 式を読めなければ読み取りの理由だけ(「式 `…` の誤り:」の繰り返しを省く)
                let why = kmb_core::parse_expr(&expr)
                    .err()
                    .map(|e| e.to_string())
                    .or_else(|| self.win.message.clone())
                    .unwrap_or_else(|| "式を読めません".into());
                ui.data_mut(|d| d.insert_temp(failed_id(id), why));
            } else {
                ui.data_mut(|d| d.remove::<String>(failed_id(id)));
            }
        }
        if let Some((id, comment, unit)) = commit_info {
            log::info!("パラメータの説明を変える: {id:?} 単位 {unit:?}");
            self.apply(edit::set_param_info(
                self.cur.work.doc().data(),
                id,
                comment.trim(),
                unit,
            ));
        }
        if let Some((id, name)) = commit_name {
            self.rename_param(id, &name);
            self.clear_edits();
        }
        if let Some(id) = remove {
            self.remove_param(id);
        }
        if let Some(a) = self.model_params_ui(ui, &q, order.as_deref(), &menu) {
            row_act = Some(a);
        }
        if let Some(a) = self.feature_values_ui(ui, &q, order.as_deref()) {
            row_act = Some(a);
        }
        if let Some((key, act)) = row_act {
            self.row_act(key, act);
        }
        if shown.as_ref().is_some_and(|s| s.is_empty()) {
            ui.label(
                egui::RichText::new(
                    if self.cur.param_tab == param_views_ui::FAVORITES_TAB {
                        "お気に入りはまだありません。「全部」の表で行の ☆ を押すと、ここに集まります"
                    } else {
                        "この表にはまだ行がありません。「全部」の表で行の名前を右クリックし、「表に入れる」で入れる"
                    },
                )
                .small()
                .color(self.win.look.pal.ink3),
            );
        }
        // 構成表は文書に 1 つなので、「全部」の表の時だけ出す
        if self.cur.param_tab == 0 {
            self.config_ui(ui);
        }
    }
}

impl Workspace {
    /// パラメータを足す行。表の最後の行として、名前・式の列に揃えて置く(10-20 u19)。
    fn new_param_row(&mut self, ui: &mut egui::Ui, w: &Cols) {
        let failed_new = crate::doc_id(ui.ctx(), "param_new_failed");
        ui.horizontal(|ui| {
            ui.add_space(w.star + ui.spacing().item_spacing.x);
            ui.add_sized(
                [w.name, w.h],
                egui::TextEdit::singleline(&mut self.cur.new_param.0).hint_text("名前"),
            );
            let c = complete::begin(ui, crate::doc_id(ui.ctx(), "param_new_expr"));
            let r = ui.add_sized(
                [w.expr, w.h],
                c.filter(egui::TextEdit::singleline(&mut self.cur.new_param.1).hint_text("式")),
            );
            let inserted = c.end(ui, &r, &mut self.cur.new_param.1, &[]);
            // 単位は値の列に置く(足した後は行の 2 行目の単位になる)
            ui.allocate_ui_with_layout(
                egui::vec2(w.value, w.h),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_width(w.value);
                    egui::ComboBox::from_id_salt("param_new_unit")
                        .width(w.value - 8.0)
                        .selected_text(self.param_unit_label(self.cur.new_param_unit))
                        .show_ui(ui, |ui| {
                            for u in kmb_core::ParamUnit::ALL {
                                let label = self.param_unit_label(u);
                                ui.selectable_value(&mut self.cur.new_param_unit, u, label);
                            }
                        });
                },
            );
            // 式の欄の Enter でも足す(候補を選ぶ Enter は除く)
            let enter = r.lost_focus()
                && !inserted
                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                && !self.cur.new_param.0.trim().is_empty();
            if ui.add(egui::Button::new("追加").small()).clicked() || enter {
                let (name, expr) = std::mem::take(&mut self.cur.new_param);
                let unit = self.cur.new_param_unit;
                let expr = if unit == kmb_core::ParamUnit::Length {
                    self.in_doc_unit(&expr)
                } else {
                    expr
                };
                if self.add_param_with(&name, &expr, unit) {
                    ui.data_mut(|d| d.remove::<String>(failed_new));
                    // 作った表を開いている時は、足したパラメータをその表にも入れる
                    // (お気に入りを開いている時は ☆ を付ける)
                    if let (true, Some(p)) = (
                        self.cur.param_tab != 0,
                        self.cur.work.doc().data().param_by_name(name.trim()),
                    ) {
                        let key = kmb_core::ParamKey::User { id: p.id };
                        let tab = self.cur.param_tab;
                        let act = if tab == param_views_ui::FAVORITES_TAB {
                            param_views_ui::RowAct::Favorite(true)
                        } else {
                            param_views_ui::RowAct::AddTo(tab)
                        };
                        self.row_act(key, act);
                    }
                } else {
                    let why = self
                        .win
                        .message
                        .clone()
                        .unwrap_or_else(|| "追加できません".into());
                    ui.data_mut(|d| d.insert_temp(failed_new, why));
                    self.cur.new_param = (name, expr);
                }
            }
        });
        if let Some(why) = ui.data(|d| d.get_temp::<String>(failed_new)) {
            reason_label(
                ui,
                ui.visuals().error_fg_color,
                &format!("追加できません: {why}"),
            );
        }
    }

    /// パラメータの一覧の「寸法」の欄: スケッチの寸法の名前
    /// (`d1` など)と式と値。名前と式はここで直せる。ほかの式から名前で参照できる。
    /// 名前ごとの「使っている所」。文書が変わった時(計算の番号か取り消しの深さが変わった時)だけ作り直す。
    fn param_users_map(&mut self) -> crate::doc_state::ParamUsers {
        let key = (
            self.cur.work.report().stamp(),
            self.cur.work.doc().undo_depth(),
        );
        if let Some((k, m)) = &self.cur.param_users
            && *k == key
        {
            return m.clone();
        }
        let m = std::sync::Arc::new(edit::all_name_users(
            self.cur.work.doc().data(),
            &self.win.registry,
        ));
        self.cur.param_users = Some((key, m.clone()));
        m
    }

    fn model_params_ui(
        &mut self,
        ui: &mut egui::Ui,
        query: &str,
        order: Option<&[kmb_core::ParamKey]>,
        menu: &param_views_ui::RowMenu,
    ) -> Option<(kmb_core::ParamKey, param_views_ui::RowAct)> {
        let all_users = self.param_users_map();
        let key_of = |p: &kmb_core::ModelParamResult| kmb_core::ParamKey::Model {
            feature: p.feature,
            key: if p.key.is_empty() {
                p.name.clone()
            } else {
                p.key.clone()
            },
        };
        let mut rows: Vec<kmb_core::ModelParamResult> = self.cur.work.report().model_params.clone();
        if let Some(o) = order {
            rows.retain(|p| o.contains(&key_of(p)));
            rows.sort_by_key(|p| o.iter().position(|k| *k == key_of(p)));
        }
        if rows.is_empty() {
            return None;
        }
        let mut act = None;
        let feature_name = |id: FeatureId| {
            self.cur
                .work
                .doc()
                .data()
                .feature(id)
                .map_or_else(|| id.to_string(), |f| f.name.clone())
        };
        if !query.is_empty() {
            rows.retain(|p| {
                p.name.to_lowercase().contains(query)
                    || p.expr.to_lowercase().contains(query)
                    || feature_name(p.feature).to_lowercase().contains(query)
            });
        }
        ui.add_space(6.0);
        ui.strong("モデルパラメータ")
            .on_hover_text("スケッチの寸法に付く名前(d1 など)。寸法・機能の欄・パラメータの式に名前を打つと、その寸法の値を使える");
        let mut rename = None;
        let mut set = None;
        // 幅の狭いパネルでも式が読めるよう、場所(機能の名前)は見出しの行に出し、
        // 寸法の種類は名前に乗せた時に出す
        let err = ui.visuals().error_fg_color;
        let users_of = |name: &str| all_users.get(name).cloned().unwrap_or_default();
        // 式の欄はパネルの残りの幅にする(☆・名前・値・使う所の列を引く。狭いパネルで使う所が切れない)
        let w_expr = (ui.available_width()
            - (18.0 + 52.0 + 60.0 + 52.0)
            - 4.0 * ui.spacing().item_spacing.x)
            .clamp(70.0, 240.0);
        egui::Grid::new("model_params")
            .num_columns(5)
            .striped(true)
            .show(ui, |ui| {
                ui.strong("☆")
                    .on_hover_text("お気に入り。☆ を押すと「お気に入り」の表に入る");
                ui.strong("名前");
                ui.strong("式");
                ui.strong("値");
                ui.strong("使う所");
                ui.end_row();
                let mut last = None;
                for p in &rows {
                    if last != Some(p.feature) {
                        last = Some(p.feature);
                        ui.label("");
                        ui.label(egui::RichText::new(feature_name(p.feature)).weak().small());
                        ui.end_row();
                    }
                    let k = key_of(p);
                    let h = ui.spacing().interact_size.y;
                    let key = crate::doc_id(ui.ctx(), ("model_param", p.feature, p.name.as_str()));
                    // 見えていない行は入力欄を置かず、同じ大きさの空きにする(R15)
                    let row = key.with("row");
                    if skip_grid_row(ui, row, h) {
                        ui.allocate_space(egui::vec2(18.0, h));
                        ui.allocate_space(egui::vec2(52.0, h));
                        ui.allocate_space(egui::vec2(w_expr, h));
                        // 値と使う所は軽いので同じに置く(列の幅を見えている時と揃える)
                        match &p.status {
                            ParamStatus::Ok { value } => {
                                ui.label(self.shown(*value));
                            }
                            ParamStatus::Error { message } => reason_label(ui, err, message),
                        }
                        users_cell(
                            ui,
                            crate::doc_id(
                                ui.ctx(),
                                ("model_param_users", p.feature, p.name.as_str()),
                            ),
                            &users_of(&p.name),
                        );
                        ui.end_row();
                        continue;
                    }
                    let top = ui.cursor().min;
                    if let Some(on) = star_button(ui, 18.0, h, menu.favorites.contains(&k)) {
                        act = Some((k.clone(), param_views_ui::RowAct::Favorite(on)));
                    }
                    let place = format!("{} の{}", feature_name(p.feature), p.label);
                    let mut name = ui
                        .data(|d| d.get_temp::<FieldEdit>(key.with("name")))
                        .map_or_else(|| p.name.clone(), |e| e.text_on(&p.name));
                    // 表の欄は前のコマの幅までしか広がらないので、大きさを決めて置く
                    let r = ui
                        .add_sized(
                            [52.0, h],
                            egui::TextEdit::singleline(&mut name).id(key.with("name_edit")),
                        )
                        .on_hover_text(&place);
                    if r.lost_focus() && name != p.name {
                        rename = Some((p.name.clone(), name.clone()));
                    }
                    r.right_menu(|ui| {
                        if let Some(a) = menu.show(ui, &k) {
                            act = Some((k.clone(), a));
                        }
                    });
                    ui.data_mut(|d| d.insert_temp(key.with("name"), FieldEdit::new(&p.name, name)));
                    let mut expr = ui
                        .data(|d| d.get_temp::<FieldEdit>(key.with("expr")))
                        .map_or_else(|| p.expr.clone(), |e| e.text_on(&p.expr));
                    let c = complete::begin(ui, key.with("expr_edit"));
                    let r = ui
                        .add_sized([w_expr, h], c.filter(egui::TextEdit::singleline(&mut expr)))
                        .on_hover_text(&place);
                    c.end(ui, &r, &mut expr, &[]);
                    if r.lost_focus() && expr != p.expr {
                        set = Some((p.name.clone(), expr.clone()));
                    }
                    ui.data_mut(|d| d.insert_temp(key.with("expr"), FieldEdit::new(&p.expr, expr)));
                    match &p.status {
                        ParamStatus::Ok { value } => {
                            ui.label(self.shown(*value));
                        }
                        ParamStatus::Error { message } => reason_label(ui, err, message),
                    }
                    let users = users_of(&p.name);
                    let open = users_cell(
                        ui,
                        crate::doc_id(ui.ctx(), ("model_param_users", p.feature, p.name.as_str())),
                        &users,
                    );
                    let rect =
                        egui::Rect::from_two_pos(top, egui::pos2(ui.max_rect().right(), top.y + h));
                    ui.data_mut(|d| d.insert_temp(row, rect));
                    ui.end_row();
                    if open {
                        ui.label("");
                        ui.label("");
                        users_list(ui, &users);
                        ui.label("");
                        ui.label("");
                        ui.end_row();
                    }
                }
            });
        // 直したら欄の入力を捨てて、文書の今の値を出し直す(失敗した時は理由をツールバーに)
        let forget = |ui: &egui::Ui, name: &str| {
            for p in &rows {
                if p.name == name {
                    let key = crate::doc_id(ui.ctx(), ("model_param", p.feature, p.name.as_str()));
                    ui.data_mut(|d| {
                        d.remove::<FieldEdit>(key.with("name"));
                        d.remove::<FieldEdit>(key.with("expr"));
                    });
                }
            }
        };
        if let Some((old, new)) = rename {
            log::info!("寸法の名前を変える: {old} → {new}");
            if self.rename_model_param(&old, new.trim()) {
                forget(ui, &old);
            }
        }
        if let Some((name, expr)) = set {
            log::info!("寸法の式を変える: {name} = {expr}");
            // 数だけを打てば寸法の単位を補う(角度は deg、長さは文書の単位)
            let angle = rows.iter().any(|p| {
                p.name == name
                    && matches!(p.status, ParamStatus::Ok { value } if value.dim == kmb_core::Dim::ANGLE)
            });
            let unit = if angle {
                "0 deg".to_string()
            } else {
                format!("0 {}", self.length_unit().name())
            };
            if self.set_model_param(&name, &with_unit_of(expr.trim(), &unit)) {
                forget(ui, &name);
            }
        }
        act
    }
}

/// パラメータの表の列の幅(足す行を表の行に揃えるため)。
struct Cols {
    h: f32,
    star: f32,
    name: f32,
    expr: f32,
    value: f32,
}

/// 前のコマに描いた行 `row` の場所が今見えず、入力中の欄も中に無ければ、描かずにその高さだけ空けて
/// 真を返す(大きな設計でパラメータの窓を軽くする。R15)。初めての行と、見える行は描く。
fn skip_row(ui: &mut egui::Ui, row: egui::Id) -> bool {
    let Some(last) = ui.data(|d| d.get_temp::<egui::Rect>(row)) else {
        return false;
    };
    let here = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(last.width(), last.height()));
    if ui.is_rect_visible(here) || has_focus_in(ui, last) {
        return false;
    }
    // 幅も前と同じだけ取る(巻く欄は中身の幅に縮むので、取らないと列の幅が変わる)
    ui.allocate_space(last.size());
    true
}

/// 表(`egui::Grid`)の行 `row` が今見えず、入力中の欄も中に無いか(高さ `h` の 1 行として見る)。
fn skip_grid_row(ui: &egui::Ui, row: egui::Id, h: f32) -> bool {
    let Some(last) = ui.data(|d| d.get_temp::<egui::Rect>(row)) else {
        return false;
    };
    let here = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(last.width(), h));
    !ui.is_rect_visible(here) && !has_focus_in(ui, last)
}

/// 入力中の欄(前のコマの場所)が `rect` の中にあるか。見えない所へ巻いても、打っている欄は描き続ける
/// (描かないと egui が入力を外し、打った文字が決まらない)。
fn has_focus_in(ui: &egui::Ui, rect: egui::Rect) -> bool {
    ui.memory(|m| m.focused())
        .and_then(|f| ui.ctx().read_response(f))
        .is_some_and(|r| rect.intersects(r.rect))
}

/// パラメータの一覧の欄に打っている文字と、打ち始めた時の文書の値。文書の値がほかの操作で
/// 変わったら(パラメータの名前を変えて式が書き換わった時など)、打っている文字は捨てて文書の
/// 今の値を出す。失敗した入力は、文書の値が変わらない間はそのまま残す。
#[derive(Clone, Default)]
pub(crate) struct FieldEdit {
    base: String,
    text: String,
}

impl FieldEdit {
    pub(crate) fn new(base: &str, text: String) -> FieldEdit {
        FieldEdit {
            base: base.to_string(),
            text,
        }
    }

    /// 文書の値が `now` の時に欄へ出す文字。
    pub(crate) fn text_on(self, now: &str) -> String {
        if self.base == now {
            self.text
        } else {
            now.to_string()
        }
    }

    /// 表の欄の入力(無いか文書の値が変わっていれば、文書の値 `now` から始める)。
    fn text<'a>(
        e: std::collections::hash_map::Entry<'a, ParamId, FieldEdit>,
        now: &str,
    ) -> &'a mut String {
        let f = e.or_insert_with(|| FieldEdit::new(now, now.to_string()));
        if f.base != now {
            *f = FieldEdit::new(now, now.to_string());
        }
        &mut f.text
    }
}

/// お気に入りの ☆(U52)。押されたら、付けるか(真)外すか(偽)を返す。
pub(crate) fn star_button(ui: &mut egui::Ui, w: f32, h: f32, on: bool) -> Option<bool> {
    let text = if on { "★" } else { "☆" };
    let r = ui
        .add_sized([w, h], egui::Button::new(text).small().frame(false))
        .on_hover_text(if on {
            "お気に入りから外す"
        } else {
            "お気に入りに入れる(「お気に入り」の表に集まる)"
        });
    r.clicked().then_some(!on)
}

/// 通らない理由を、欄の横に赤い字で出す(長ければ折り返す)。
pub(crate) fn reason_label(ui: &mut egui::Ui, color: egui::Color32, text: &str) {
    ui.add(egui::Label::new(egui::RichText::new(text).color(color).small()).wrap());
}

/// 使っている所の欄: 「使用 N」を押すと、次の行に名前を広げる(U53)。広げているかを返す。
pub(crate) fn users_cell(ui: &mut egui::Ui, id: egui::Id, users: &[String]) -> bool {
    if users.is_empty() {
        ui.add(egui::Label::new(egui::RichText::new("未使用").weak().small()).extend());
        return false;
    }
    let mut open: bool = ui.data(|d| d.get_temp(id)).unwrap_or(false);
    if ui
        .add(
            egui::Button::new(egui::RichText::new(format!("使用 {}", users.len())).small())
                .frame(open)
                .wrap_mode(egui::TextWrapMode::Extend),
        )
        .on_hover_text(format!(
            "使っている所(押すと下に並べる): {}",
            users.join("、")
        ))
        .clicked()
    {
        open = !open;
    }
    ui.data_mut(|d| d.insert_temp(id, open));
    open
}

/// [`users_cell`] で広げた、使っている所の名前(パネルの幅で折り返す)。
pub(crate) fn users_list(ui: &mut egui::Ui, users: &[String]) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(format!("使っている所: {}", users.join("、"))).small(),
        )
        .wrap(),
    );
}
