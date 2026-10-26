//! 外観の窓。
//! 上に適用先(ボディ/コンポーネント・面)、次に「このデザイン内」(使っている外観と直した外観)、
//! 下に「ライブラリ」(探す欄・すべて/お気に入り・分類ごとの一覧)。3D のビューで付ける物を選んで
//! から見本を押すと付く(見本を引いて落とす代わり)。見本の右クリックで、直す・複製・使っている所を
//! 選ぶ・お気に入り・割り当てを外して消す。直すのは小さな窓(名前・色・金属・粗さ)。
//! 文書を変えるのは全部外観の操作([`kmb_interchange::role::action::APPEARANCE`]。CLI・MCP と同じ道)。窓は画面を止めない。

use crate::Workspace;
use crate::layout::Space;
use crate::theme;
use crate::theme::RightMenu as _;
use kmb_core::FeatureId;
use kmb_look::material::{CATEGORIES, LIBRARY};
use serde_json::{Value, json};

/// 窓の様子(文書ごと)。
#[derive(Debug, Default)]
pub(crate) struct LooksPage {
    pub open: bool,
    /// 適用先が面か(偽ならボディ/コンポーネント)
    pub faces: bool,
    /// ライブラリを探す言葉
    pub query: String,
    /// ライブラリでお気に入りだけを出すか
    pub favorites: bool,
    /// 直している外観(小さな窓)
    pub editing: Option<Editing>,
}

/// 直している途中の値(OK で外観の操作に渡す)。
#[derive(Debug, Clone)]
pub(crate) struct Editing {
    /// 直し始めた時の名前
    pub name: String,
    pub new_name: String,
    pub color: [u8; 3],
    pub metallic: f32,
    pub roughness: f32,
}

/// 見本の右クリックで選んだこと。
enum SwatchCmd {
    Apply(String),
    Edit(String),
    Duplicate(String),
    SelectUsers(String),
    Favorite(String, bool),
    Delete(String),
}

impl Workspace {
    /// 外観の窓を開く(ツールバーの「外観」・A のキー・ボディの右クリック)。
    pub fn open_looks(&mut self) {
        if !matches!(self.win.layout.space, Space::Design | Space::Render) {
            self.switch_space(Space::Design);
        }
        self.cur.looks.open = true;
        log::info!("外観の窓を開く");
    }

    /// 外観の操作を引数 `args` で流す。文書が変わったら真。
    pub(crate) fn looks_run(&mut self, args: Value) -> bool {
        let Some(id) = self.action_for_role(&kmb_interchange::role::action::APPEARANCE) else {
            self.win.fail("外観の操作がありません");
            return false;
        };
        let before = self.cur.work.doc().undo_labels().len();
        self.run_action_args(&id, args);
        let changed = self.cur.work.doc().undo_labels().len() != before;
        if changed {
            self.refresh_body_colors();
        }
        changed
    }

    /// 選んでいる物に外観 `name` を付ける(`None` なら外す)。
    pub(crate) fn looks_apply(&mut self, name: Option<&str>) {
        let target = if self.cur.looks.faces { "face" } else { "body" };
        let args = match name {
            Some(n) => json!({"apply": n, "target": target}),
            None => json!({"remove": true, "target": target}),
        };
        self.looks_run(args);
    }

    /// 選んでいる物の数の説明(窓の上に出す)。
    fn looks_target_note(&self) -> String {
        let faces = usize::from(self.cur.picked.is_some()) + self.cur.also_picked.len();
        if self.cur.looks.faces {
            return if faces == 0 {
                "3D のビューで面を選んでから見本を押す".into()
            } else {
                format!("選んでいる面 {faces} 個に付ける")
            };
        }
        let mut roots: Vec<FeatureId> = self
            .cur
            .picked
            .iter()
            .chain(self.cur.also_picked.iter())
            .map(|p| p.feature)
            .chain(self.cur.selected)
            .chain(self.cur.also.iter().copied())
            .filter_map(|id| {
                self.body_roots()
                    .into_iter()
                    .find(|(last, root)| *last == id || *root == id)
                    .map(|b| b.1)
            })
            .collect();
        roots.sort();
        roots.dedup();
        if roots.is_empty() {
            "3D のビューかブラウザでボディを選んでから見本を押す".into()
        } else {
            format!("選んでいるボディ {} 個に付ける", roots.len())
        }
    }

    /// 外観 `name` を使っているボディと面を選ぶ(「適用するオブジェクトを選択」)。
    fn looks_select_users(&mut self, name: &str) {
        let data = self.cur.work.doc().data();
        let (bodies, comps, faces) = kmb_look::appearance::of(data).users(data, name);
        let roots = self.body_roots();
        let last_of = |root: FeatureId| roots.iter().find(|b| b.1 == root).map(|b| b.0);
        let mut first = true;
        // 部品の外観は、部品の中のボディを選ぶ
        let comp_bodies: Vec<FeatureId> = self
            .cur
            .work
            .doc()
            .data()
            .components
            .iter()
            .filter(|c| comps.contains(&c.id))
            .flat_map(|c| c.bodies.clone())
            .collect();
        for root in bodies.into_iter().chain(comp_bodies) {
            if let Some(last) = last_of(root) {
                self.select_body_row(last, !first);
                first = false;
            }
        }
        let picks: Vec<crate::view3d::Pick> = faces
            .into_iter()
            .filter_map(|(root, face)| {
                Some(crate::view3d::Pick {
                    feature: last_of(root)?,
                    face,
                })
            })
            .collect();
        self.cur.picked = picks.first().cloned();
        self.cur.also_picked = picks.iter().skip(1).cloned().collect();
        if self.cur.picked.is_none() && first {
            self.win
                .fail(format!("外観「{name}」を付けた物は今の形にありません"));
        }
        log::info!("外観「{name}」を使っている所を選ぶ");
    }

    /// お気に入りに足す・外す(PC の設定に残す)。
    pub(crate) fn looks_favorite(&mut self, name: &str, on: bool) {
        let list = &mut self.win.layout.favorite_looks;
        list.retain(|n| n != name);
        if on {
            list.push(name.to_string());
        }
        log::info!(
            "外観「{name}」をお気に入りから{}",
            if on { "足す" } else { "外す" }
        );
    }

    /// 外観の窓。
    pub(crate) fn looks_window(&mut self, ctx: &egui::Context) {
        if !self.cur.looks.open {
            return;
        }
        if !matches!(self.win.layout.space, Space::Design | Space::Render) {
            self.cur.looks.open = false;
            return;
        }
        let mut open = true;
        let mut cmd: Option<SwatchCmd> = None;
        let mut remove = false;
        egui::Window::new("外観")
            .id(egui::Id::new("looks_window"))
            .open(&mut open)
            .default_size([320.0, 560.0])
            .default_pos([ctx.content_rect().right() - 680.0, 180.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let label = ui.label("適用先");
                    let name = |faces: bool| {
                        if faces {
                            "面"
                        } else {
                            "ボディ/コンポーネント"
                        }
                    };
                    egui::ComboBox::from_id_salt("looks_target")
                        .selected_text(name(self.cur.looks.faces))
                        .show_ui(ui, |ui| {
                            for f in [false, true] {
                                ui.selectable_value(&mut self.cur.looks.faces, f, name(f));
                            }
                        })
                        .response
                        .labelled_by(label.id);
                });
                ui.label(
                    egui::RichText::new(self.looks_target_note())
                        .color(theme::look(ui.ctx()).pal.ink2),
                );
                if ui
                    .button("選んだ物の外観を外す")
                    .on_hover_text(
                        "面は面の外観だけを外してボディの外観に戻す。ボディは材料の色に戻す",
                    )
                    .clicked()
                {
                    remove = true;
                }
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.strong("このデザイン内");
                    self.looks_design_ui(ui, &mut cmd);
                    ui.separator();
                    ui.strong("ライブラリ");
                    self.looks_library_ui(ui, &mut cmd);
                });
            });
        if remove {
            self.looks_apply(None);
        }
        match cmd {
            Some(SwatchCmd::Apply(n)) => self.looks_apply(Some(&n)),
            Some(SwatchCmd::Edit(n)) => self.looks_start_edit(&n),
            Some(SwatchCmd::Duplicate(n)) => {
                self.looks_run(json!({"duplicate": n}));
            }
            Some(SwatchCmd::SelectUsers(n)) => self.looks_select_users(&n),
            Some(SwatchCmd::Favorite(n, on)) => self.looks_favorite(&n, on),
            Some(SwatchCmd::Delete(n)) => {
                self.looks_run(json!({"delete": n}));
            }
            None => {}
        }
        self.looks_edit_window(ctx);
        if !open {
            log::info!("外観の窓を閉じる");
            self.cur.looks.open = false;
            self.cur.looks.editing = None;
        }
    }

    /// このデザイン内の外観の見本。
    fn looks_design_ui(&mut self, ui: &mut egui::Ui, cmd: &mut Option<SwatchCmd>) {
        let data = self.cur.work.doc().data();
        let all = kmb_look::appearance::of(data);
        let names = all.design_names(data);
        if names.is_empty() {
            ui.label(
                egui::RichText::new("まだありません(ライブラリの見本を押すと付く)")
                    .color(theme::look(ui.ctx()).pal.ink2),
            );
            return;
        }
        let missing = self.looks_missing_faces();
        for n in names {
            let Some(r) = all.resolve(&n) else {
                // 知らない名前(ほかの版の文書など)。外すことだけできる
                let resp = ui.label(format!("{n}(知らない外観)"));
                resp.right_menu(|ui| {
                    if ui.button("割り当てを外して消す").clicked() {
                        *cmd = Some(SwatchCmd::Delete(n.clone()));
                        ui.close();
                    }
                });
                continue;
            };
            let mut note = Vec::new();
            if r.custom {
                note.push("直した".to_string());
            }
            if let Some(k) = missing.get(&n) {
                note.push(format!("見つからない面 {k}"));
            }
            let fav = self.win.layout.favorite_looks.contains(&n);
            let resp = swatch_row(ui, r.color, &n, &note.join("・"));
            if resp.clicked() {
                *cmd = Some(SwatchCmd::Apply(n.clone()));
            }
            resp.right_menu(|ui| {
                if ui.button("編集…").clicked() {
                    *cmd = Some(SwatchCmd::Edit(n.clone()));
                    ui.close();
                }
                if ui.button("複製").clicked() {
                    *cmd = Some(SwatchCmd::Duplicate(n.clone()));
                    ui.close();
                }
                if ui.button("適用するオブジェクトを選択").clicked() {
                    *cmd = Some(SwatchCmd::SelectUsers(n.clone()));
                    ui.close();
                }
                let label = if fav {
                    "お気に入りから外す"
                } else {
                    "お気に入りに追加"
                };
                if ui.button(label).clicked() {
                    *cmd = Some(SwatchCmd::Favorite(n.clone(), !fav));
                    ui.close();
                }
                ui.separator();
                if ui
                    .button("割り当て解除と削除")
                    .on_hover_text("この外観を付けたボディ・部品・面から外し、このデザインから消す")
                    .clicked()
                {
                    *cmd = Some(SwatchCmd::Delete(n.clone()));
                    ui.close();
                }
            });
        }
    }

    /// ライブラリの見本(探す欄・すべて/お気に入り・分類ごと)。
    fn looks_library_ui(&mut self, ui: &mut egui::Ui, cmd: &mut Option<SwatchCmd>) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.cur.looks.favorites, false, "すべて");
            ui.selectable_value(&mut self.cur.looks.favorites, true, "お気に入り");
        });
        ui.horizontal(|ui| {
            let label = ui.label("探す");
            ui.add(
                egui::TextEdit::singleline(&mut self.cur.looks.query)
                    .hint_text("名前・分類(aluminum のような別名も)")
                    .desired_width(f32::INFINITY),
            )
            .labelled_by(label.id);
        });
        let q = self.cur.looks.query.trim().to_lowercase();
        let favs = self.win.layout.favorite_looks.clone();
        let hits: Vec<&kmb_look::material::LibraryLook> = LIBRARY
            .iter()
            .filter(|l| !self.cur.looks.favorites || favs.iter().any(|f| f == l.name))
            .filter(|l| {
                q.is_empty()
                    || l.name.to_lowercase().contains(&q)
                    || l.category.contains(q.as_str())
                    || kmb_look::material::same_material(l.name, &q)
            })
            .collect();
        if hits.is_empty() {
            ui.label(
                egui::RichText::new(if self.cur.looks.favorites && favs.is_empty() {
                    "お気に入りはまだありません(見本の右クリックで足す)"
                } else {
                    "合う外観はありません"
                })
                .color(theme::look(ui.ctx()).pal.ink2),
            );
            return;
        }
        let open_all = !q.is_empty() || self.cur.looks.favorites;
        for c in CATEGORIES {
            let list: Vec<_> = hits.iter().filter(|l| l.category == *c).collect();
            if list.is_empty() {
                continue;
            }
            egui::CollapsingHeader::new(format!("{c}({})", list.len()))
                .id_salt(("looks_category", *c))
                .open(open_all.then_some(true))
                .default_open(false)
                .show(ui, |ui| {
                    for l in list {
                        let fav = favs.iter().any(|f| f == l.name);
                        let resp = swatch_row(ui, l.color, l.name, if fav { "★" } else { "" });
                        if resp.clicked() {
                            *cmd = Some(SwatchCmd::Apply(l.name.to_string()));
                        }
                        resp.right_menu(|ui| {
                            let label = if fav { "お気に入りから外す" } else { "お気に入りに追加" };
                            if ui.button(label).clicked() {
                                *cmd = Some(SwatchCmd::Favorite(l.name.to_string(), !fav));
                                ui.close();
                            }
                            if ui
                                .button("このデザインに写して編集…")
                                .on_hover_text("色と粗さを直した外観をこのデザインに持つ(同じ名前の外観は全部この値になる)")
                                .clicked()
                            {
                                *cmd = Some(SwatchCmd::Edit(l.name.to_string()));
                                ui.close();
                            }
                            if ui.button("複製").clicked() {
                                *cmd = Some(SwatchCmd::Duplicate(l.name.to_string()));
                                ui.close();
                            }
                        });
                    }
                });
        }
    }

    /// 外観の面のうち、今のボディに無い面の数(外観の名前ごと。3D のビューの網の面で見る)。
    pub(crate) fn looks_missing_faces(&self) -> std::collections::BTreeMap<String, usize> {
        let data = self.cur.work.doc().data();
        let mut out = std::collections::BTreeMap::new();
        let all = kmb_look::appearance::of(data);
        if all.faces.is_empty() {
            return out;
        }
        let roots = self.body_roots();
        for (root, m) in &all.faces {
            let names: Vec<&String> = roots
                .iter()
                .filter(|b| b.1 == *root)
                .filter_map(|b| self.cur.scene.mesh_of(b.0))
                .flat_map(|m| m.face_names.iter())
                .collect();
            for (face, a) in m {
                if !names.contains(&face) {
                    *out.entry(a.clone()).or_default() += 1;
                }
            }
        }
        out
    }

    /// 外観 `name` を直し始める(小さな窓を出す)。
    fn looks_start_edit(&mut self, name: &str) {
        let Some(r) = kmb_look::appearance::of(self.cur.work.doc().data()).resolve(name) else {
            return;
        };
        log::info!("外観「{name}」を直し始める");
        self.cur.looks.editing = Some(Editing {
            name: r.name.clone(),
            new_name: r.name,
            color: r.color,
            metallic: r.finish.metallic,
            roughness: r.finish.roughness,
        });
    }

    /// 外観を直す小さな窓(名前・色・金属・粗さ。OK・Enter で決め、キャンセル・Esc でやめる)。
    fn looks_edit_window(&mut self, ctx: &egui::Context) {
        let Some(mut e) = self.cur.looks.editing.clone() else {
            return;
        };
        let mut ok = false;
        let mut cancel = false;
        egui::Window::new("外観を直す")
            .id(egui::Id::new("looks_edit_window"))
            .collapsible(false)
            .resizable(false)
            .default_pos([ctx.content_rect().right() - 1010.0, 180.0])
            .show(ctx, |ui| {
                egui::Grid::new("looks_edit_grid")
                    .num_columns(2)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        ui.label("名前");
                        ui.add(egui::TextEdit::singleline(&mut e.new_name).desired_width(180.0));
                        ui.end_row();
                        ui.label("色");
                        ui.horizontal(|ui| {
                            ui.color_edit_button_srgb(&mut e.color);
                            for (k, l) in ["R", "G", "B"].iter().enumerate() {
                                ui.label(*l);
                                ui.add(egui::DragValue::new(&mut e.color[k]).range(0..=255));
                            }
                        });
                        ui.end_row();
                        ui.label("金属");
                        ui.add(egui::Slider::new(&mut e.metallic, 0.0..=1.0).fixed_decimals(2))
                            .on_hover_text("1 は金属(映り込みに色が付く)、0 は塗装・樹脂・木");
                        ui.end_row();
                        ui.label("粗さ");
                        ui.add(egui::Slider::new(&mut e.roughness, 0.0..=1.0).fixed_decimals(2))
                            .on_hover_text("0 は鏡のようにつるつる、1 はつやが無い");
                        ui.end_row();
                    });
                ui.horizontal(|ui| {
                    ok = ui.button("OK").clicked();
                    cancel = ui.button("キャンセル").clicked();
                });
            });
        let typing = ctx.memory(|m| m.focused().is_some());
        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) && !typing {
            ok = true;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            cancel = true;
        }
        if cancel {
            log::info!("外観「{}」を直すのをやめる", e.name);
            self.cur.looks.editing = None;
            return;
        }
        if ok {
            let c = e.color;
            let mut args = json!({
                "edit": e.name,
                "color": format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]),
                "metallic": e.metallic,
                "roughness": e.roughness,
            });
            if e.new_name.trim() != e.name {
                args["rename"] = json!(e.new_name.trim());
            }
            self.looks_run(args);
            self.cur.looks.editing = None;
            return;
        }
        self.cur.looks.editing = Some(e);
    }

    /// 右クリックの「外観」の一覧(ボディと部品の右クリック)。選んだ外観の名前か、空なら外す。
    /// 「外観の窓…」を押したら窓を開く。
    pub(crate) fn look_menu(
        &mut self,
        ui: &mut egui::Ui,
        current: Option<&str>,
        default: &str,
    ) -> Option<String> {
        let mut pick = None;
        if ui.selectable_label(current.is_none(), default).clicked() {
            pick = Some(String::new());
        }
        let data = self.cur.work.doc().data();
        let all = kmb_look::appearance::of(data);
        let design = all.design_names(data);
        if !design.is_empty() {
            ui.separator();
            for n in &design {
                let Some(r) = all.resolve(n) else {
                    continue;
                };
                if swatch_label(ui, r.color, n, current == Some(n.as_str())).clicked() {
                    pick = Some(n.clone());
                }
            }
        }
        ui.separator();
        for c in CATEGORIES {
            ui.menu_button(*c, |ui| {
                for l in LIBRARY.iter().filter(|l| l.category == *c) {
                    if swatch_label(ui, l.color, l.name, current == Some(l.name)).clicked() {
                        pick = Some(l.name.to_string());
                    }
                }
            });
        }
        ui.separator();
        if ui.button("外観の窓…").clicked() {
            self.open_looks();
            ui.close();
        }
        pick
    }
}

/// 色の小さな四角と名前の、押せる行(窓の見本)。右に注記を薄く出す。
fn swatch_row(ui: &mut egui::Ui, color: [u8; 3], name: &str, note: &str) -> egui::Response {
    ui.horizontal(|ui| {
        swatch(ui, color);
        let r = ui.selectable_label(false, name);
        if !note.is_empty() {
            ui.label(
                egui::RichText::new(note)
                    .small()
                    .color(theme::look(ui.ctx()).pal.ink2),
            );
        }
        r
    })
    .inner
}

/// 色の四角と名前の、選べる行(右クリックの一覧)。
fn swatch_label(ui: &mut egui::Ui, color: [u8; 3], name: &str, on: bool) -> egui::Response {
    ui.horizontal(|ui| {
        swatch(ui, color);
        ui.selectable_label(on, name)
    })
    .inner
}

/// 色の見本の四角。
fn swatch(ui: &mut egui::Ui, c: [u8; 3]) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(r, 2.0, egui::Color32::from_rgb(c[0], c[1], c[2]));
    ui.painter().rect_stroke(
        r,
        2.0,
        egui::Stroke::new(1.0, theme::look(ui.ctx()).pal.edge),
        egui::StrokeKind::Inside,
    );
}
