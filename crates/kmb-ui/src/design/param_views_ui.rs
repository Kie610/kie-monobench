//! パラメータの窓の表(U50): 上のタブ(全部・作った表・「+」)、行の右クリック(表に入れる・外す)、
//! 機能の値(押し出しの距離など)の欄。表は文書に持つ([`kmb_core::ParamView`])ので取り消せ、
//! 保存される。表は同じパラメータを見る窓で、どの表で直しても元のパラメータ・寸法・機能の値が変わる。
//! 行を選ぶ計算は CLI・MCP と同じ [`kmb_session::params`]。

use crate::theme::RightMenu as _;
use crate::{FieldEdit, Workspace, complete, reason_label, star_button};
use kmb_core::{FeatureId, Param, ParamKey, ParamStatus, ParamView};
use kmb_session::{param_io, params};
use std::collections::HashSet;

/// 「お気に入り」のタブ(U52)の番号。表の番号と重ならない値にし、表がまだ無くても開ける。
pub(crate) const FAVORITES_TAB: u64 = u64::MAX;

/// 行の右クリックで選んだこと。
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RowAct {
    /// 表に入れる(表の番号)
    AddTo(u64),
    /// 新しい表を作って入れる
    NewView,
    /// 今の表から外す
    Hide,
    /// お気に入りに入れる(真)・外す(偽)
    Favorite(bool),
    /// 今の表の中で 1 つ上(真)・下(偽)へ
    Move(bool),
}

/// 行の右クリックのメニューを作る材料(作った表の番号・名前・出ている行と、今の表、お気に入りの行)。
pub(crate) struct RowMenu {
    pub views: Vec<(u64, String, HashSet<ParamKey>)>,
    pub tab: u64,
    pub favorites: HashSet<ParamKey>,
}

impl RowMenu {
    /// 行 `key` の右クリックの中身。選んだことを返す。
    pub(crate) fn show(&self, ui: &mut egui::Ui, key: &ParamKey) -> Option<RowAct> {
        let mut act = None;
        crate::theme::menu_sub(ui, "table", "表に入れる", |ui| {
            for (id, name, rows) in &self.views {
                let has = rows.contains(key);
                let r = crate::theme::menu_row(ui, "", name, "", !has);
                let r = if has {
                    r.on_disabled_hover_text("この表にもう入っています")
                } else {
                    r.on_hover_text("この行を表に入れる(写しは作らない。どの表で直しても同じ値)")
                };
                if r.clicked() {
                    act = Some(RowAct::AddTo(*id));
                    ui.close();
                }
            }
            if !self.views.is_empty() {
                ui.separator();
            }
            if crate::theme::menu_row(ui, "plus", "新しい表…", "", true)
                .on_hover_text("新しい表を作り、この行を入れる")
                .clicked()
            {
                act = Some(RowAct::NewView);
                ui.close();
            }
        });
        let fav = self.favorites.contains(key);
        let (text, tip) = if fav {
            (
                "お気に入りから外す",
                "「お気に入り」の表に出さない(パラメータは消さない)",
            )
        } else {
            (
                "お気に入りに入れる",
                "「お気に入り」の表に入れる(行の ☆ と同じ)",
            )
        };
        if crate::theme::menu_row(ui, "star", text, "", true)
            .on_hover_text(tip)
            .clicked()
        {
            act = Some(RowAct::Favorite(!fav));
            ui.close();
        }
        if self.tab != 0 {
            ui.separator();
            if crate::theme::menu_row(ui, "up", "上へ", "", true)
                .on_hover_text("この表の中で 1 つ上へ(同じ種類の行の中で)")
                .clicked()
            {
                act = Some(RowAct::Move(true));
                ui.close();
            }
            if crate::theme::menu_row(ui, "down", "下へ", "", true)
                .on_hover_text("この表の中で 1 つ下へ(同じ種類の行の中で)")
                .clicked()
            {
                act = Some(RowAct::Move(false));
                ui.close();
            }
        }
        if self.tab != 0
            && self.tab != FAVORITES_TAB
            && crate::theme::menu_row(ui, "eyeoff", "この表から外す", "", true)
                .on_hover_text("この表に出さない(パラメータは消さない。「全部」の表には残る)")
                .clicked()
        {
            act = Some(RowAct::Hide);
            ui.close();
        }
        act
    }
}

impl Workspace {
    /// 今の表(「全部」なら `None`。お気に入りの表がまだ無ければ空の表)。
    fn current_view(&self) -> Option<ParamView> {
        match self.cur.param_tab {
            0 => None,
            FAVORITES_TAB => Some(
                self.cur
                    .work
                    .doc()
                    .data()
                    .favorites_view()
                    .cloned()
                    .unwrap_or(ParamView {
                        id: FAVORITES_TAB,
                        name: params::FAVORITES.into(),
                        members: Vec::new(),
                        body: None,
                        hidden: Vec::new(),
                        favorites: true,
                    }),
            ),
            id => self.cur.work.doc().data().param_view(id).cloned(),
        }
    }

    /// 今の表の行を表の並びで(「全部」なら `None`)。表が消えていれば「全部」に戻す。
    pub(crate) fn param_view_rows(&mut self) -> Option<Vec<ParamKey>> {
        let Some(v) = self.current_view() else {
            self.cur.param_tab = 0;
            return None;
        };
        Some(params::view_keys(
            self.cur.work.doc().data(),
            &self.win.registry,
            self.cur.work.report(),
            &v,
        ))
    }

    /// 消したパラメータのうち、今の表に出す物(「全部」なら全部、表ならその表に入れていた物)。
    /// 新しく消した物を先に。
    pub(crate) fn removed_rows(&self) -> Vec<Param> {
        let view = self.current_view();
        self.cur
            .work
            .doc()
            .data()
            .removed_params
            .iter()
            .rev()
            .filter(|p| {
                view.as_ref()
                    .is_none_or(|v| v.members.contains(&ParamKey::User { id: p.id }))
            })
            .cloned()
            .collect()
    }

    /// 消したパラメータを戻す(取り消せる)。同じ名前が使われていれば理由を出す。
    pub(crate) fn restore_param(&mut self, id: kmb_core::ParamId) -> bool {
        let model: Vec<String> = self
            .cur
            .work
            .report()
            .model_params
            .iter()
            .map(|p| p.name.clone())
            .collect();
        log::info!("消したパラメータを戻す: {id:?}");
        match kmb_session::edit::restore_param(self.cur.work.doc().data(), &model, id) {
            Ok(tx) => self.apply(tx),
            Err(e) => {
                self.win.fail(e);
                false
            }
        }
    }

    /// 式の欄の名前の候補(U51): 利用者のパラメータと寸法の名前、今の値。
    pub(crate) fn expr_candidates(&self) -> Vec<complete::Candidate> {
        let value = |s: Option<&ParamStatus>| match s {
            Some(ParamStatus::Ok { value }) => self.shown(*value),
            _ => String::new(),
        };
        let mut out: Vec<complete::Candidate> = self
            .cur
            .work
            .doc()
            .data()
            .params
            .iter()
            .map(|p| {
                let st = self.cur.work.report().params.iter().find(|r| r.id == p.id);
                complete::Candidate::new(&p.name, value(st.map(|r| &r.status)))
            })
            .collect();
        out.extend(self.cur.work.report().model_params.iter().map(|p| {
            complete::Candidate::new(&p.name, format!("寸法 {}", value(Some(&p.status))))
        }));
        out
    }

    /// 行の右クリックの材料。
    pub(crate) fn row_menu(&self) -> RowMenu {
        let data = self.cur.work.doc().data();
        RowMenu {
            views: data
                .param_views
                .iter()
                .filter(|v| !v.favorites)
                .map(|v| {
                    let rows =
                        params::view_keys(data, &self.win.registry, self.cur.work.report(), v);
                    (v.id, v.name.clone(), rows.into_iter().collect())
                })
                .collect(),
            tab: self.cur.param_tab,
            favorites: data
                .favorites_view()
                .map(|v| v.members.iter().cloned().collect())
                .unwrap_or_default(),
        }
    }

    /// 次の表の名前(「表1」「表2」…。使っていない番号)。
    fn next_view_name(&self) -> String {
        (1..)
            .map(|n| format!("表{n}"))
            .find(|n| {
                !self
                    .cur
                    .work
                    .doc()
                    .data()
                    .param_views
                    .iter()
                    .any(|v| &v.name == n)
            })
            .expect("どこかで空く")
    }

    /// 新しい表を作って開く(`body` があればそのボディに関わる行を決まりで入れる)。
    pub(crate) fn new_param_view(
        &mut self,
        name: Option<String>,
        body: Option<FeatureId>,
        rows: Vec<ParamKey>,
    ) {
        let name = name.unwrap_or_else(|| self.next_view_name());
        log::info!(
            "パラメータの表を作る: {name}(ボディ {body:?}、行 {})",
            rows.len()
        );
        match self
            .cur
            .work
            .prepare(|d| params::add_view(d, &name, body, rows))
        {
            Ok((tx, id)) => {
                if self.apply(tx) {
                    self.cur.param_tab = id;
                    self.win.show_params = true;
                }
            }
            Err(e) => self.win.fail(e),
        }
    }

    /// ボディの右クリックの「このボディのパラメータの表を作る」。
    pub fn make_body_param_view(&mut self, root: FeatureId) {
        let name = params::body_view_name(
            self.cur.work.doc().data(),
            &self.win.registry,
            self.cur.work.report(),
            root,
        );
        self.new_param_view(Some(name), Some(root), Vec::new());
    }

    /// 行の右クリックで選んだことを行う。
    pub(crate) fn row_act(&mut self, key: ParamKey, act: RowAct) {
        log::info!("パラメータの表の行: {key:?} {act:?}");
        let tx = match act {
            RowAct::AddTo(id) => params::add_to_view(self.cur.work.doc().data(), id, &[key]),
            RowAct::NewView => {
                self.new_param_view(None, None, vec![key]);
                return;
            }
            RowAct::Hide => params::remove_from_view(
                self.cur.work.doc().data(),
                &self.win.registry,
                self.cur.work.report(),
                self.cur.param_tab,
                &[key],
            ),
            RowAct::Favorite(on) => {
                Ok(self.cur.work.prepare(|d| params::set_favorite(d, &key, on)))
            }
            RowAct::Move(up) => {
                let Some(id) = self.current_view().map(|v| v.id) else {
                    return;
                };
                match params::move_in_view(
                    self.cur.work.doc().data(),
                    &self.win.registry,
                    self.cur.work.report(),
                    id,
                    &key,
                    up,
                ) {
                    Ok(Some(tx)) => Ok(tx),
                    // 端の行: 何もしない
                    Ok(None) => return,
                    Err(e) => Err(e),
                }
            }
        };
        match tx {
            Ok(tx) => {
                self.apply(tx);
            }
            Err(e) => self.win.fail(e),
        }
    }

    /// 窓の上のタブ(全部・作った表・「+」)と、選んだ表の説明。
    pub(crate) fn param_tabs_ui(&mut self, ui: &mut egui::Ui) {
        let views: Vec<ParamView> = self
            .cur
            .work
            .doc()
            .data()
            .param_views
            .iter()
            .filter(|v| !v.favorites)
            .cloned()
            .collect();
        if self.cur.param_tab != 0
            && self.cur.param_tab != FAVORITES_TAB
            && !views.iter().any(|v| v.id == self.cur.param_tab)
        {
            self.cur.param_tab = 0;
        }
        let rename_id = crate::doc_id(ui.ctx(), "param_view_rename");
        let mut renaming: Option<(u64, String)> = ui.data(|d| d.get_temp(rename_id));
        let mut remove = None;
        let mut create = false;
        // 書き出す(形)か読み込む(None)
        let mut io: Option<Option<param_io::Format>> = None;
        ui.horizontal_wrapped(|ui| {
            if ui
                .selectable_label(self.cur.param_tab == 0, "全部")
                .on_hover_text("文書のパラメータ・寸法・機能の値をすべて出す")
                .clicked()
            {
                self.cur.param_tab = 0;
            }
            if ui
                .selectable_label(self.cur.param_tab == FAVORITES_TAB, "☆ お気に入り")
                .on_hover_text("行の ☆ を押したパラメータ・寸法・機能の値。行の右クリックの「上へ」「下へ」で並べ替えられる")
                .clicked()
            {
                self.cur.param_tab = FAVORITES_TAB;
            }
            for v in &views {
                let tip = match v.body {
                    Some(b) => format!(
                        "{}に関わる物と、手で入れた行。右クリックで名前の変更・削除",
                        self.view_body_name(b)
                    ),
                    None => "手で入れた行。右クリックで名前の変更・削除".to_string(),
                };
                let r = ui
                    .selectable_label(self.cur.param_tab == v.id, &v.name)
                    .on_hover_text(tip);
                if r.clicked() {
                    self.cur.param_tab = v.id;
                }
                r.right_menu(|ui| {
                    if crate::theme::menu_row(ui, "rename", "名前変更", "", true).clicked() {
                        renaming = Some((v.id, v.name.clone()));
                        ui.close();
                    }
                    ui.separator();
                    if crate::theme::menu_row(ui, "trash", "表を削除", "", true)
                        .on_hover_text("表だけを消す(中のパラメータは消さない。取り消せる)")
                        .clicked()
                    {
                        remove = Some(v.id);
                        ui.close();
                    }
                });
            }
            if ui
                .small_button("+")
                .on_hover_text("新しい表を作る(行の右クリックの「表に入れる」で行を入れる)")
                .clicked()
            {
                create = true;
            }
            ui.menu_button("出し入れ", |ui| {
                if ui
                    .button("CSV に書き出す…")
                    .on_hover_text("今の表の行を CSV にする(表計算で開ける)")
                    .clicked()
                {
                    io = Some(Some(param_io::Format::Csv));
                    ui.close();
                }
                if ui
                    .button("JSON に書き出す…")
                    .on_hover_text("今の表の行を JSON にする")
                    .clicked()
                {
                    io = Some(Some(param_io::Format::Json));
                    ui.close();
                }
                ui.separator();
                if ui
                    .button("読み込む…")
                    .on_hover_text(
                        "書き出した CSV・JSON を読み込む。同じ名前のパラメータは式・単位・コメントを上書きし、\
                         無ければ足す。寸法と機能の値は、この文書にある物だけ直す(取り消せる)",
                    )
                    .clicked()
                {
                    io = Some(None);
                    ui.close();
                }
            })
            .response
            .on_hover_text("パラメータの表を CSV・JSON に書き出す・読み込む");
        });
        if let Some((id, mut text)) = renaming.take() {
            let mut done = None;
            ui.horizontal(|ui| {
                let r = ui.add(
                    egui::TextEdit::singleline(&mut text)
                        .id(egui::Id::new("param_view_rename_edit"))
                        .desired_width(140.0),
                );
                ui.ctx()
                    .accesskit_node_builder(r.id, |b| b.set_label("表示の名前"));
                // 機能・グループ・視点の名前と同じ決まり方: Enter・欄の外を押すと決まり、Esc でやめる。
                // 注目は打ち始めの 1 こまだけ移す(毎こま移すと欄の外を押しても抜けられない)
                let started = rename_id.with("started");
                let first = ui.ctx().data_mut(|d| {
                    let first = d.get_temp::<u64>(started) != Some(id);
                    d.insert_temp(started, id);
                    first
                });
                if first {
                    r.request_focus();
                }
                let ok = ui.small_button("OK");
                let cancel = ui.small_button("キャンセル");
                let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
                if cancel.clicked() || esc {
                    done = Some(false);
                } else if ok.clicked() {
                    done = Some(true);
                } else if !first
                    && (r.lost_focus() || !r.has_focus())
                    && !ok.hovered()
                    && !cancel.hovered()
                {
                    // OK・キャンセルを押している途中は、離した時の押した方で決める
                    done = Some(true);
                }
                if done.is_some() {
                    ui.ctx().data_mut(|d| d.remove::<u64>(started));
                }
            });
            match done {
                Some(true) => {
                    match params::rename_view(self.cur.work.doc().data(), id, &text) {
                        Ok(tx) => {
                            self.apply(tx);
                        }
                        Err(e) => self.win.fail(e),
                    }
                    ui.data_mut(|d| {
                        d.remove::<(u64, String)>(rename_id);
                    });
                }
                Some(false) => ui.data_mut(|d| {
                    d.remove::<(u64, String)>(rename_id);
                }),
                None => ui.data_mut(|d| {
                    d.insert_temp(rename_id, (id, text));
                }),
            }
        }
        if let Some(id) = remove {
            match params::remove_view(self.cur.work.doc().data(), id) {
                Ok(tx) => {
                    if self.apply(tx) {
                        self.cur.param_tab = 0;
                    }
                }
                Err(e) => self.win.fail(e),
            }
        }
        if create {
            self.new_param_view(None, None, Vec::new());
        }
        match io {
            Some(Some(f)) => self.export_param_table(f),
            Some(None) => self.import_param_table(),
            None => {}
        }
        if let Some(v) = self
            .cur
            .work
            .doc()
            .data()
            .param_view(self.cur.param_tab)
            .cloned()
        {
            let what = match v.body {
                Some(b) => format!("{}に関わる物", self.view_body_name(b)),
                None => "手で入れた行".to_string(),
            };
            ui.label(
                egui::RichText::new(format!("{what}(行の右クリックで表に入れる・外す)"))
                    .small()
                    .color(self.win.look.pal.ink3),
            );
        }
    }

    /// ボディの名前(最初に作った機能 `root` で指す)。
    fn view_body_name(&self, root: FeatureId) -> String {
        params::body_view_name(
            self.cur.work.doc().data(),
            &self.win.registry,
            self.cur.work.report(),
            root,
        )
        .trim_end_matches(" のパラメータ")
        .to_string()
    }

    /// 機能の値(押し出しの距離など)の欄。機能の名前の見出しの下に、欄の名前・式・値を並べ、
    /// 式はその場で直せる(機能の欄を直すのと同じ。取り消せる)。`shown` は今の表の行。
    pub(crate) fn feature_values_ui(
        &mut self,
        ui: &mut egui::Ui,
        query: &str,
        order: Option<&[ParamKey]>,
    ) -> Option<(ParamKey, RowAct)> {
        let feature_name = |id: FeatureId| {
            self.cur
                .work
                .doc()
                .data()
                .feature(id)
                .map_or_else(|| id.to_string(), |f| f.name.clone())
        };
        let mut rows = params::feature_values(
            self.cur.work.doc().data(),
            &self.win.registry,
            self.cur.work.report(),
        );
        let key_of = |v: &params::FeatureValue| ParamKey::Field {
            feature: v.feature,
            key: v.key.clone(),
        };
        if let Some(o) = order {
            rows.retain(|v| o.contains(&key_of(v)));
            rows.sort_by_key(|v| o.iter().position(|k| *k == key_of(v)));
        }
        if !query.is_empty() {
            rows.retain(|v| {
                v.label.to_lowercase().contains(query)
                    || v.expr.to_lowercase().contains(query)
                    || feature_name(v.feature).to_lowercase().contains(query)
            });
        }
        if rows.is_empty() {
            return None;
        }
        ui.add_space(6.0);
        ui.strong("フィーチャの値").on_hover_text(
            "押し出しの距離・フィレットの半径など、機能の欄の式。ここで直すと機能の欄も変わる(取り消せる)",
        );
        let menu = self.row_menu();
        let mut act = None;
        let mut set = None;
        let err = ui.visuals().error_fg_color;
        egui::Grid::new("feature_values")
            .num_columns(4)
            .striped(true)
            .show(ui, |ui| {
                ui.strong("☆")
                    .on_hover_text("お気に入り。☆ を押すと「お気に入り」の表に入る");
                ui.strong("欄");
                ui.strong("式");
                ui.strong("値");
                ui.end_row();
                let mut last = None;
                for v in &rows {
                    if last != Some(v.feature) {
                        last = Some(v.feature);
                        ui.label("");
                        ui.label(egui::RichText::new(feature_name(v.feature)).weak().small());
                        ui.end_row();
                    }
                    let key = key_of(v);
                    let h = ui.spacing().interact_size.y;
                    if let Some(on) = star_button(ui, 18.0, h, menu.favorites.contains(&key)) {
                        act = Some((key.clone(), RowAct::Favorite(on)));
                    }
                    let r = ui
                        .add(egui::Label::new(&v.label).sense(egui::Sense::click()))
                        .on_hover_text(format!("{} の{}", feature_name(v.feature), v.label));
                    r.right_menu(|ui| {
                        if let Some(a) = menu.show(ui, &key) {
                            act = Some((key.clone(), a));
                        }
                    });
                    let id = crate::doc_id(ui.ctx(), ("feature_value", v.feature, v.key.as_str()));
                    let mut expr = ui
                        .data(|d| d.get_temp::<FieldEdit>(id))
                        .map_or_else(|| v.expr.clone(), |e| e.text_on(&v.expr));
                    let c = complete::begin(ui, id.with("edit"));
                    let r =
                        ui.add_sized([130.0, h], c.filter(egui::TextEdit::singleline(&mut expr)));
                    c.end(ui, &r, &mut expr, &[]);
                    if r.lost_focus() && expr != v.expr {
                        set = Some((v.feature, v.key.clone(), expr.clone()));
                    }
                    ui.data_mut(|d| d.insert_temp(id, FieldEdit::new(&v.expr, expr)));
                    match &v.status {
                        ParamStatus::Ok { value } => {
                            ui.label(self.shown(*value));
                        }
                        ParamStatus::Text { value } => {
                            ui.label(value);
                        }
                        ParamStatus::Error { message } => reason_label(ui, err, message),
                    }
                    ui.end_row();
                }
            });
        if let Some((feature, key, expr)) = set {
            log::info!("機能の値を変える: {feature} {key} = {expr}");
            match params::set_feature_value(
                self.cur.work.doc().data(),
                &self.win.registry,
                self.cur.work.report(),
                feature,
                &key,
                &expr,
            ) {
                Ok(tx) => {
                    if self.apply(tx) {
                        let id = crate::doc_id(ui.ctx(), ("feature_value", feature, key.as_str()));
                        ui.data_mut(|d| d.remove::<FieldEdit>(id));
                    }
                }
                Err(e) => self.win.fail(e),
            }
        }
        act
    }
}

impl Workspace {
    /// 今の表の名前と行(書き出し用)。
    fn table_rows(&self) -> (String, Vec<ParamKey>) {
        let (data, reg, rep) = (
            self.cur.work.doc().data(),
            &self.win.registry,
            self.cur.work.report(),
        );
        match self.current_view() {
            Some(v) => (v.name.clone(), params::view_keys(data, reg, rep, &v)),
            None => ("全部".into(), params::all_keys(data, reg, rep)),
        }
    }

    /// 今の表を CSV・JSON に書き出す(U54)。保存先は窓で選ぶ。
    pub(crate) fn export_param_table(&mut self, format: param_io::Format) {
        let (name, keys) = self.table_rows();
        let rows = param_io::rows(
            self.cur.work.doc().data(),
            &self.win.registry,
            self.cur.work.report(),
            &keys,
        );
        let (ext, filter) = match format {
            param_io::Format::Csv => ("csv", "CSV"),
            param_io::Format::Json => ("json", "JSON"),
        };
        let stem = self
            .cur
            .work
            .path()
            .and_then(|p| p.file_stem())
            .map_or("無題".to_string(), |n| n.to_string_lossy().to_string());
        let title = "パラメータの表を書き出す";
        let Some(path) =
            self.win
                .dialogs
                .save_file(title, filter, &[ext], &format!("{stem}-{name}.{ext}"))
        else {
            self.win.message = Some(match self.win.dialogs.unavailable() {
                Some(why) => format!("{title}: 保存の窓を出せません({why})"),
                None => format!("{title}: 保存先が選ばれなかったので、やめました"),
            });
            return;
        };
        self.write_param_table(&path, &name, rows, format);
    }

    /// 表の行を `path` に書く(窓を出さない。試験も使う)。
    pub(crate) fn write_param_table(
        &mut self,
        path: &std::path::Path,
        name: &str,
        rows: Vec<param_io::Row>,
        format: param_io::Format,
    ) {
        let n = rows.len();
        let text = param_io::export(name, rows, format);
        match kmb_session::write_output(path, text.as_bytes()) {
            Ok(()) => {
                log::info!(
                    "パラメータの表「{name}」を書き出した: {}({n} 行)",
                    path.display()
                );
                self.win.message = Some(format!(
                    "パラメータの表「{name}」の {n} 行を {} に書き出しました",
                    path.display()
                ));
            }
            Err(e) => self.win.fail(e.to_string()),
        }
    }

    /// CSV・JSON の表を読み込む(U54。取り消せる)。ファイルは窓で選ぶ。
    pub(crate) fn import_param_table(&mut self) {
        let title = "パラメータの表を読み込む";
        let Some(path) =
            self.win
                .dialogs
                .open_file(title, "パラメータの表(CSV・JSON)", &["csv", "json"])
        else {
            self.win.message = Some(format!("{title}: ファイルが選ばれなかったので、やめました"));
            return;
        };
        self.read_param_table(&path);
    }

    /// `path` の表を読み込む(窓を出さない。試験も使う)。読めたら真。
    pub(crate) fn read_param_table(&mut self, path: &std::path::Path) -> bool {
        let format = path
            .extension()
            .and_then(|e| e.to_str())
            .and_then(param_io::Format::from_extension);
        let Some(format) = format else {
            self.win.message = Some(format!(
                "{} は読めません(拡張子は .csv か .json)",
                path.display()
            ));
            return false;
        };
        let rows = std::fs::read_to_string(path)
            .map_err(|e| format!("{} を読めません: {e}", path.display()))
            .and_then(|t| param_io::parse(&t, format));
        let rows = match rows {
            Ok(r) => r,
            Err(e) => {
                self.win.fail(e);
                return false;
            }
        };
        let report = self.cur.work.report().clone();
        let registry = &self.win.registry;
        let (tx, done) = self
            .cur
            .work
            .prepare(|d| param_io::import(d, registry, &report, &rows));
        log::info!(
            "パラメータの表を読み込む: {}({})",
            path.display(),
            done.summary()
        );
        if let Some(tx) = tx
            && !self.apply(tx)
        {
            return false;
        }
        self.clear_edits();
        self.win.message = Some(format!(
            "パラメータの表を読み込みました: {}",
            done.summary()
        ));
        true
    }
}
