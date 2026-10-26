//! 文書に持つ見せ方の情報: 設計の属性(U13)、名前を付けた視点(U20)、読み込んだ STEP を
//! 立てるかの問い(U12)。どれも取り消せる変更として文書へ入れる。

use crate::theme::RightMenu as _;
use crate::Workspace;
use kmb_core::{FeatureId, NamedView, Properties};
use kmb_session::edit;

/// 読み込んだ STEP を立てるかの答え(U12)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepUp {
    /// X 軸のまわりに 90 度回して立てる(1 回の取り消しで戻る)
    StandUp,
    /// そのままにする
    Keep,
    /// そのままにし、次から聞かない(設定で戻せる)
    NeverAsk,
}

/// 読み込んだ STEP の機能の「立てる」欄(kmb-cad-import-step の `ImportStep::stand_up`)。
const STAND_UP_KEY: &str = "stand_up";
/// 書いた CAD が Z を上にして書いた印(同じ機能の `ImportStep::upright`)。立っているなら聞かない
const UPRIGHT_KEY: &str = "upright";

impl Workspace {
    // ---- 設計の属性(U13) ----

    /// 設計の属性を置き換える(取り消せる)。
    pub fn set_properties(&mut self, p: Properties) -> bool {
        if self.cur.work.doc().data().properties == p {
            return true;
        }
        self.apply(edit::set_properties(p))
    }

    /// 設計の属性の窓を開く(今の値から直し始める)。
    pub fn open_properties(&mut self) {
        self.cur.props_draft = Some(self.cur.work.doc().data().properties.clone());
        self.cur.props_tags = self.cur.work.doc().data().properties.tags_text();
    }

    /// 設計の属性の窓(ファイルのメニューとブラウザの文書の右クリックから開く)。
    pub(crate) fn properties_window(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.cur.props_draft.take() else {
            return;
        };
        let mut open = true;
        let mut done = None;
        egui::Window::new("設計の属性")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.weak("部品名・部品番号・改訂・材料は、G-code の頭の注に入る。プロジェクト・ジャンル・タグは、プロジェクトから探す時の分類に使う");
                egui::Grid::new("props_grid")
                    .num_columns(2)
                    .spacing([10.0, 6.0])
                    .show(ui, |ui| {
                        // 文書の長さの単位
                        ui.label("長さの単位");
                        unit_combo(ui, "props_units", &mut draft.units).on_hover_text(
                            "寸法・式の欄・パラメータの値・測る道具をこの単位で出し、数だけ打った長さもこの単位で読む。\n\
                             形と保存は mm のままなので、単位を変えても形は変わらない。G-code はいつも mm",
                        );
                        ui.end_row();
                        for (key, label) in Properties::FIELDS {
                            ui.label(label);
                            if let Some(v) = draft.get_mut(key) {
                                let multi = key == "description";
                                let w = egui::TextEdit::singleline(v).desired_width(260.0);
                                let w = if multi {
                                    egui::TextEdit::multiline(v)
                                        .desired_rows(2)
                                        .desired_width(260.0)
                                } else {
                                    w
                                };
                                ui.add(w);
                            }
                            ui.end_row();
                        }
                        // タグ(モデル管理の分類。プロジェクトから探す時の絞り込みに使う)
                        ui.label("タグ");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.cur.props_tags)
                                .hint_text("「,」で区切る(例: 治具, アルミ)")
                                .desired_width(260.0),
                        )
                        .on_hover_text(
                            "自由に付ける分類。「プロジェクトから探す」でタグ・ジャンル・プロジェクトで絞り込める",
                        );
                        ui.end_row();
                        // 自分で足した項目(名前と値。値を空にすると消える)
                        let mut gone = None;
                        for (i, (k, v)) in draft.custom.iter_mut().enumerate() {
                            ui.add(egui::TextEdit::singleline(k).desired_width(90.0));
                            ui.horizontal(|ui| {
                                ui.add(egui::TextEdit::singleline(v).desired_width(220.0));
                                if ui
                                    .small_button("×")
                                    .on_hover_text("この項目を消す")
                                    .clicked()
                                {
                                    gone = Some(i);
                                }
                            });
                            ui.end_row();
                        }
                        if let Some(i) = gone {
                            draft.custom.remove(i);
                        }
                    });
                if ui
                    .button("項目を足す")
                    .on_hover_text("自分で名前を付けた項目(例: 検査、客先)")
                    .clicked()
                {
                    draft
                        .custom
                        .push((format!("項目{}", draft.custom.len() + 1), String::new()));
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        done = Some(true);
                    }
                    if ui.button("キャンセル").clicked() {
                        done = Some(false);
                    }
                });
            });
        match done {
            Some(true) => {
                // 前後の空白を落とし、名前か値が空の自分の項目は消す
                let mut p = Properties {
                    units: draft.units,
                    ..Default::default()
                };
                for (key, _) in Properties::FIELDS {
                    p.set(key, draft.get(key).unwrap_or(""));
                }
                for (k, v) in &draft.custom {
                    p.set(k, v);
                }
                p.set_tags_text(&self.cur.props_tags);
                self.set_properties(p);
            }
            Some(false) => {}
            None if open => self.cur.props_draft = Some(draft),
            None => {}
        }
    }

    // ---- 名前を付けた視点(U20) ----

    /// 今の視点を名前を付けて保存する(空なら「視点N」)。保存した視点の番号。
    pub fn save_named_view(&mut self, name: &str) -> Option<u64> {
        let (c, yaw, pitch, height) = self.cur.camera.snapshot(self.win.view_rect);
        let view = NamedView {
            id: 0,
            name: name.to_string(),
            target: c.map(f64::from),
            yaw: f64::from(yaw),
            pitch: f64::from(pitch),
            roll: f64::from(self.cur.camera.tilt_deg()),
            height: f64::from(height),
            // 見えている投影(自動なら今の向きで決まった方)
            perspective: self.cur.camera.perspective,
        };
        let (tx, id) = self.cur.work.prepare(|d| edit::save_named_view(d, view));
        self.apply(tx).then_some(id)
    }

    /// 名前を付けた視点へ戻る(向きは滑らかに替える)。文書は変えない。
    pub fn apply_named_view(&mut self, id: u64) -> bool {
        let Some(v) = self.cur.work.doc().data().named_view(id).cloned() else {
            return false;
        };
        // 自動の投影はそのまま(戻った向きで決まる)。平行・透視は視点の投影にする
        if self.win.layout.projection != crate::Projection::Auto {
            self.win.layout.projection = if v.perspective {
                crate::Projection::Perspective
            } else {
                crate::Projection::Parallel
            };
        }
        self.cur.camera.restore(
            v.target.map(|x| x as f32),
            v.yaw as f32,
            v.pitch as f32,
            v.roll as f32,
            v.height as f32,
            self.win.view_rect,
        );
        self.cur.camera.perspective = self.win.layout.projection.perspective_for(&self.cur.camera);
        self.win.message = Some(format!("視点「{}」にしました", v.name));
        true
    }

    /// 名前を付けた視点を今の視点で上書きする。
    pub fn update_named_view(&mut self, id: u64) -> bool {
        let (c, yaw, pitch, height) = self.cur.camera.snapshot(self.win.view_rect);
        let perspective = self.cur.camera.perspective;
        let roll = f64::from(self.cur.camera.tilt_deg());
        let name = self
            .cur
            .work
            .doc()
            .data()
            .named_view(id)
            .map(|v| v.name.clone())
            .unwrap_or_default();
        match edit::change_named_view(
            self.cur.work.doc().data(),
            id,
            &format!("視点「{name}」を今の視点にする"),
            |v| {
                v.target = c.map(f64::from);
                v.yaw = f64::from(yaw);
                v.pitch = f64::from(pitch);
                v.roll = roll;
                v.height = f64::from(height);
                v.perspective = perspective;
            },
        ) {
            Ok(tx) => self.apply(tx),
            Err(e) => {
                self.win.fail(e);
                false
            }
        }
    }

    pub fn rename_named_view(&mut self, id: u64, name: &str) -> bool {
        let name = name.trim().to_string();
        if name.is_empty() {
            self.win.message = Some("視点の名前が空です".into());
            return false;
        }
        match edit::change_named_view(
            self.cur.work.doc().data(),
            id,
            &format!("視点の名前を「{name}」にする"),
            |v| v.name = name.clone(),
        ) {
            Ok(tx) => self.apply(tx),
            Err(e) => {
                self.win.fail(e);
                false
            }
        }
    }

    pub fn remove_named_view(&mut self, id: u64) -> bool {
        match edit::remove_named_view(self.cur.work.doc().data(), id) {
            Ok(tx) => self.apply(tx),
            Err(e) => {
                self.win.fail(e);
                false
            }
        }
    }

    /// ブラウザの「名前の付いたビュー」の中の行。押すとその視点にする。右クリックで名前・上書き・消す。
    pub(crate) fn named_view_rows(&mut self, ui: &mut egui::Ui) {
        let views = self.cur.work.doc().data().named_views.clone();
        if views.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(46.0);
                ui.label(
                    egui::RichText::new("右クリックの「今のビューを保存」で足す")
                        .size(crate::theme::fs(11.0))
                        .color(self.win.look.pal.ink3),
                );
            });
        }
        for v in views {
            if let Some((id, text)) = &mut self.cur.renaming_view
                && *id == v.id
            {
                // 機能・グループの名前と同じ決まり方: Enter・Tab・別の所を押すと決まり、Esc でやめる
                // (注目は打ち始めの 1 こまだけ移す。毎こま移すと欄から抜けられない)
                let started = crate::doc_id(ui.ctx(), "named_view_rename");
                let mut fin = None;
                ui.horizontal(|ui| {
                    ui.add_space(46.0);
                    let r = ui.add(
                        egui::TextEdit::singleline(text)
                            .id_salt((started, v.id))
                            .desired_width(140.0),
                    );
                    ui.ctx()
                        .accesskit_node_builder(r.id, |b| b.set_label("視点の名前"));
                    let first = ui.ctx().data_mut(|d| {
                        let first = d.get_temp::<u64>(started) != Some(v.id);
                        d.insert_temp(started, v.id);
                        first
                    });
                    if first {
                        r.request_focus();
                    } else if r.lost_focus() || !r.has_focus() {
                        fin = Some(!ui.input(|i| i.key_pressed(egui::Key::Escape)));
                    }
                });
                if fin.is_some() {
                    ui.ctx().data_mut(|d| d.remove::<u64>(started));
                }
                match fin {
                    Some(true) => {
                        let name = text.clone();
                        self.cur.renaming_view = None;
                        self.rename_named_view(v.id, &name);
                    }
                    Some(false) => self.cur.renaming_view = None,
                    None => {}
                }
                continue;
            }
            let r = crate::browser_ui::plain_row(ui, 2, "eye", &v.name);
            let r = r.on_hover_text("押すとこの視点にする。右クリックで名前を変える・上書き・消す");
            if r.clicked() {
                self.apply_named_view(v.id);
            }
            r.right_menu(|ui| {
                if ui.button("このビューにする").clicked() {
                    self.apply_named_view(v.id);
                    ui.close();
                }
                if ui.button("今のビューで上書き").clicked() {
                    self.update_named_view(v.id);
                    ui.close();
                }
                if ui.button("名前変更").clicked() {
                    self.cur.renaming_view = Some((v.id, v.name.clone()));
                    ui.close();
                }
                if ui.button("削除").clicked() {
                    self.remove_named_view(v.id);
                    ui.close();
                }
            });
        }
    }

    // ---- 読み込んだ STEP を立てる(U12) ----

    /// 立てるかを聞いている、読み込んだ STEP の機能。
    pub fn step_up_question(&self) -> Option<FeatureId> {
        self.cur.step_up_ask
    }

    /// 読み込んだ機能に立てるかを聞くか(Z を上にして書いた STEP には聞かない)。
    pub(crate) fn needs_step_up(&self, id: FeatureId) -> bool {
        self.cur
            .work
            .doc()
            .data()
            .feature(id)
            .is_some_and(|f| f.data.get(UPRIGHT_KEY).and_then(|v| v.as_bool()) != Some(true))
    }

    /// 立てるかの問いに答える。
    pub fn answer_step_up(&mut self, a: StepUp) {
        let Some(id) = self.cur.step_up_ask.take() else {
            return;
        };
        match a {
            StepUp::StandUp => {
                let Some(f) = self.cur.work.doc().data().feature(id) else {
                    return;
                };
                let mut data = f.data.clone();
                data[STAND_UP_KEY] = serde_json::json!(true);
                let mut tx = edit::set_feature_data(self.cur.work.doc().data(), id, data);
                tx.label = format!("「{}」を立てる", f.name);
                if self.apply(tx) {
                    // 立てた形が見えるよう、次に描く時に全体へ合わせる
                    self.cur.camera.refit();
                    self.win.message = Some(
                        "X 軸のまわりに 90 度回して立てました(取り消しで戻る。機能の欄でも変えられる)"
                            .into(),
                    );
                }
            }
            StepUp::Keep => {}
            StepUp::NeverAsk => {
                self.win.layout.ask_step_up = false;
                self.win.message = Some(
                    "次から聞きません(設定の「ファイル」で戻せる。機能の欄の「Y が上の形を立てる」でいつでも立てられる)"
                        .into(),
                );
            }
        }
    }

    /// 読み込んだ STEP を立てるかの問い(読み込んだすぐ後に 1 回だけ出す)。
    pub(crate) fn step_up_dialog(&mut self, ctx: &egui::Context) {
        if self.cur.step_up_ask.is_none() {
            return;
        }
        let mut answer = None;
        let mut open = true;
        egui::Window::new("読み込んだ形の向き")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 90.0))
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(
                    "他の CAD の多くは Y を上にして作ります。この形が寝ているなら、\n\
                     Z が上になるように立てられます(X 軸のまわりに 90 度。取り消しで戻る)。",
                );
                ui.horizontal(|ui| {
                    if ui.button("立てる").clicked() {
                        answer = Some(StepUp::StandUp);
                    }
                    if ui.button("このまま").clicked() {
                        answer = Some(StepUp::Keep);
                    }
                    if ui
                        .button("次から聞かない")
                        .on_hover_text("設定の「ファイル」で戻せる")
                        .clicked()
                    {
                        answer = Some(StepUp::NeverAsk);
                    }
                });
            });
        if !open {
            answer = Some(StepUp::Keep);
        }
        if let Some(a) = answer {
            self.answer_step_up(a);
        }
    }
}

/// 長さの単位を選ぶ一覧(設計の属性と設定の窓)。
pub(crate) fn unit_combo(
    ui: &mut egui::Ui,
    id: &str,
    unit: &mut kmb_core::LengthUnit,
) -> egui::Response {
    egui::ComboBox::from_id_salt(id)
        .selected_text(unit.label())
        .show_ui(ui, |ui| {
            for u in kmb_core::LengthUnit::ALL {
                ui.selectable_value(unit, u, u.label());
            }
        })
        .response
}
