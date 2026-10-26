//! 右クリックのメニュー。選んだものに使える操作だけを出す。
//! タイムラインの機能と、3D のビュー(右クリックした面)に付ける。

use crate::Workspace;
use kmb_core::{FeatureId, Place};

/// 名前を変えている物。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Renaming {
    Feature(FeatureId),
    /// ボディ(最初に作った機能で指す)
    Body(FeatureId),
    /// 部品(番号)
    Component(u64),
}
use kmb_session::edit;

impl Workspace {
    /// 記述子に初めのデータがある種類(画面から追加できるもの)と、今は追加できない理由。
    /// 区分 `category` のものだけ。手で作る専用のボタンがある種類は除く。
    pub fn addable(&self, category: &str) -> Vec<(String, String, Option<String>)> {
        let key = (
            category.to_string(),
            self.cur.work.report().stamp(),
            self.cur.work.doc().undo_depth(),
        );
        let mut memo = self.cur.addable.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((k, v)) = memo.as_ref()
            && *k == key
        {
            return v.as_ref().clone();
        }
        let v = std::sync::Arc::new(self.addable_now(category));
        *memo = Some((key, v.clone()));
        v.as_ref().clone()
    }

    /// [`Workspace::addable`] を控えを使わずに求める。
    fn addable_now(&self, category: &str) -> Vec<(String, String, Option<String>)> {
        let mut list: Vec<_> = self
            .win
            .registry
            .descriptors()
            .into_iter()
            .filter(|d| d.category == category && d.template.is_some())
            .collect();
        list.sort_by_key(|d| d.order);
        list.into_iter()
            .map(|d| {
                let why = d
                    .instantiate_with(&|slot| self.candidates(slot), &[], &|_| false)
                    .err()
                    .map(|label| format!("先に「{label}」に使える機能が要る"));
                (d.type_id, d.label, why)
            })
            .collect()
    }

    /// 最後までの機能のうち、出力 `slot` を持つもの(古い順)。
    fn candidates(&self, slot: &str) -> Vec<u64> {
        self.context().candidates(slot)
    }

    /// 記述子の初めのデータで機能を足す。3D のビューで先に選んでいた物は選ぶ欄へ振り分け、
    /// 残りの参照は直前の候補で埋める。最初の選ぶ欄を、クリックを受ける状態にする。
    pub fn add_described(&mut self, type_id: &str) -> Option<FeatureId> {
        if self.hold_for_moved(crate::joint_drag_ui::Held::Add(type_id.into())) {
            return None;
        }
        self.commit_creation();
        self.cur.editing = None;
        let d = self.win.registry.descriptor(type_id)?;
        let made = {
            let cx = self.context();
            let refs = cx.refs();
            d.instantiate_with(&|slot| self.candidates(slot), &refs, &|r| cx.is_planar(r))
                .map(|(mut data, left, auto)| {
                    d.follow(&mut data, &|slot| self.candidates(slot), &|id| {
                        self.reads(id)
                    });
                    (data, left, auto)
                })
        };
        let (data, left, auto) = match made {
            Ok(v) => v,
            Err(label) => {
                self.win.message = Some(format!(
                    "{} を足すには、先に「{label}」に使える機能が要ります",
                    d.label
                ));
                return None;
            }
        };
        let n = self.count_of(type_id) + 1;
        let mut outcome = kmb_core::Outcome::insert(
            &format!("{}を追加", d.label),
            type_id,
            format!("{}{n}", d.label),
            data,
        );
        // 続けて足す種類(ボディの分割の残りの側など)も同じ取り消し単位で足す。直すのは先の方
        if let (Some((ctype, cdata)), kmb_core::Outcome::Change { edits, .. }) =
            (d.companion.clone(), &mut outcome)
        {
            let label = self
                .win
                .registry
                .descriptor(&ctype)
                .map_or_else(|| ctype.clone(), |c| c.label);
            edits.push(kmb_core::Edit::Insert {
                name: format!("{label}{}", self.count_of(&ctype) + 1),
                type_id: ctype,
                data: cdata,
            });
        }
        let id = self.take_outcome(outcome, None)?;
        self.enter_first_pick_field(id, &auto);
        if !left.is_empty() {
            // 黙って捨てない
            self.win.message = Some(format!(
                "先に選んでいた {} 個は「{}」のどの欄にも入らないので使いませんでした",
                left.len(),
                d.label
            ));
        }
        Some(id)
    }

    /// 機能 `id` が読む機能(読む順。読み込めない機能なら空)。ボディは今のそのボディの最後の機能
    /// にする(スケッチの後で同じボディに足した機能があれば、それを対象にする)。
    fn reads(&self, id: u64) -> Vec<u64> {
        kmb_core::reads_now(
            self.cur.work.doc().data(),
            &self.win.registry,
            self.cur.work.report(),
            FeatureId(id),
        )
        .into_iter()
        .map(|f| f.0)
        .collect()
    }

    fn index_of(&self, id: FeatureId) -> Option<usize> {
        self.cur.work.doc().data().index_of(id)
    }

    /// 機能を消す(取り消せる)。使っていた機能は理由つきで止まる。
    pub fn remove_feature(&mut self, id: FeatureId) -> bool {
        self.remove_features(&[id])
    }

    /// 機能をまとめて消す(1 回の取り消し単位)。
    pub fn remove_features(&mut self, ids: &[FeatureId]) -> bool {
        // 直している途中・作っている途中の機能を消す時は、先に直すのを終える(消えた機能を直し続けない。
        // 作っている途中の物は決めてから消すので、取り消すと作った形で戻る。10-22 a4)
        if self.cur.editing.is_some_and(|e| ids.contains(&e))
            || self.cur.creating.is_some_and(|(c, _)| ids.contains(&c))
        {
            self.finish_edit();
        }
        if self.cur.selected.is_some_and(|s| ids.contains(&s)) {
            self.clear_selection();
        }
        self.cur.also.retain(|a| !ids.contains(a));
        let tx = edit::remove_features(self.cur.work.doc().data(), ids);
        self.apply(tx)
    }

    /// 機能を、それを使う機能と中身がなくなる部品と一緒に消す(1 回の取り消し単位)。
    pub fn remove_features_with_users(&mut self, ids: &[FeatureId]) -> bool {
        let (tx, all) = self.cur.work.remove_with_users_tx(ids);
        if self.cur.editing.is_some_and(|e| all.contains(&e))
            || self.cur.creating.is_some_and(|(c, _)| all.contains(&c))
        {
            self.finish_edit();
        }
        if self.cur.selected.is_some_and(|s| all.contains(&s)) {
            self.clear_selection();
        }
        self.cur.also.retain(|a| !all.contains(a));
        self.apply(tx)
    }

    /// 画面から機能を消す(右クリックの「削除」、Delete キー)。ほかの機能が使っているなら、
    /// すぐには消さず、使えなくなる機能を示して確かめる(黙って消すと、後で使っていた機能が赤くなる)。
    pub fn ask_remove_features(&mut self, ids: &[FeatureId]) -> bool {
        let users = edit::dependents(self.cur.work.doc().data(), &self.win.registry, ids);
        // 部品の中身が全部なくなる時も確かめる(空の部品の箱を黙って残さない。10-24 a1)
        let emptied = !self.cur.work.parts_emptied_by(ids, false).is_empty();
        if users.is_empty() && !emptied {
            // 確かめずに消す代わりに、戻し方を知らせる
            let names = edit::names(self.cur.work.doc().data(), ids);
            let groups: Vec<String> = edit::groups_emptied(self.cur.work.doc().data(), ids)
                .into_iter()
                .map(|g| format!("グループ「{}」", g.name))
                .collect();
            let done = self.remove_features(ids);
            if done {
                let also = if groups.is_empty() {
                    String::new()
                } else {
                    format!("。{}もなくなりました", groups.join(""))
                };
                self.win.message = Some(format!("{names}を消しました{also}(Ctrl+Z で戻せます)"));
            }
            return done;
        }
        self.cur.remove_ask = Some((ids.to_vec(), users));
        false
    }

    /// 消す前の確かめ(使っている機能があるとき)。直接使う機能と、それを通して使う機能を
    /// 1 行ずつ注意の色で並べ、一緒に消えるグループも出す(消した後で後ろが赤くなって驚かないように)。
    pub(crate) fn remove_ask_ui(&mut self, ctx: &egui::Context) {
        let Some((ids, users)) = self.cur.remove_ask.clone() else {
            return;
        };
        let data = self.cur.work.doc().data().clone();
        let direct = edit::users(&data, &self.win.registry, &ids);
        let (direct, indirect): (Vec<FeatureId>, Vec<FeatureId>) =
            users.iter().partition(|u| direct.contains(u));
        let groups: Vec<String> = edit::groups_emptied(&data, &ids)
            .into_iter()
            .map(|g| g.name.clone())
            .collect();
        // 使っている物も一緒に消すと中身がなくなる部品(これだけ消す時は、空の部品の箱が残る)
        let parts = self.cur.work.parts_emptied_by(&ids, true);
        let name = |id: &FeatureId| {
            data.feature(*id)
                .map_or_else(|| id.to_string(), |f| f.name.clone())
        };
        let warn = self.win.look.pal.warn;
        let mut open = true;
        let mut remove = false;
        let mut with_users = false;
        let mut cancel = false;
        egui::Window::new("機能を消す")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_max_width(420.0);
                let what = if users.is_empty() {
                    "次の部品の中身がなくなります"
                } else {
                    "次の機能が計算できなくなります"
                };
                ui.label(format!(
                    "{}を消すと、{what}。消しますか?",
                    edit::names(&data, &ids)
                ));
                egui::ScrollArea::vertical()
                    .max_height(220.0)
                    .show(ui, |ui| {
                        let list = |ui: &mut egui::Ui, title: &str, rows: &[FeatureId]| {
                            if rows.is_empty() {
                                return;
                            }
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new(title).small().strong());
                            for id in rows {
                                ui.label(
                                    egui::RichText::new(format!("⚠ {}", name(id))).color(warn),
                                );
                            }
                        };
                        list(ui, "直接使っている機能", &direct);
                        list(ui, "それを通して使っている機能", &indirect);
                        if !groups.is_empty() {
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new("一緒になくなるもの").small().strong());
                            for g in &groups {
                                ui.label(format!("グループ「{g}」(中身がなくなるため)"));
                            }
                        }
                        if !parts.is_empty() {
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new("中身がなくなる部品").small().strong(),
                            );
                            for p in &parts {
                                ui.label(format!("部品「{p}」"));
                            }
                        }
                    });
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new("消した後でも取り消し(Ctrl+Z)で戻せます")
                        .small()
                        .weak(),
                );
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui
                        .button("一緒に消す")
                        .on_hover_text(
                            "上に並んだ機能と、中身がなくなる部品も消す(つなぐ相手の無いジョイントや空の部品を残さない)",
                        )
                        .clicked()
                    {
                        with_users = true;
                    }
                    if ui
                        .button("削除")
                        .on_hover_text(
                            "選んだ物だけを消す。使っていた機能は理由つきで止まり、部品の箱は残る",
                        )
                        .clicked()
                    {
                        remove = true;
                    }
                    if ui.button("キャンセル").clicked() {
                        cancel = true;
                    }
                });
            });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            cancel = true;
        }
        if !open || cancel {
            self.cur.remove_ask = None;
        } else if remove {
            self.cur.remove_ask = None;
            self.remove_features(&ids);
        } else if with_users {
            self.cur.remove_ask = None;
            self.remove_features_with_users(&ids);
        }
    }

    /// 機能の名前を変える(取り消せる)。空や前と同じ名前なら何もしない。
    pub fn rename_feature(&mut self, id: FeatureId, name: &str) -> bool {
        let name = name.trim();
        let Some(f) = self.cur.work.doc().data().feature(id) else {
            return false;
        };
        if name.is_empty() || name == f.name {
            return false;
        }
        log::info!("機能の名前を変える: {} → {name}", f.name);
        let tx = edit::rename_feature(self.cur.work.doc().data(), id, name);
        self.apply(tx)
    }

    /// 機能の名前を変える窓を開く(右クリックの「名前変更」)。
    pub fn start_rename_feature(&mut self, id: FeatureId) {
        if let Some(f) = self.cur.work.doc().data().feature(id) {
            self.cur.renaming = Some((Renaming::Feature(id), f.name.clone()));
        }
    }

    /// ボディ(最初に作った機能 `root`)の名前を変える窓を開く。
    pub fn start_rename_body(&mut self, root: FeatureId) {
        let name = self.body_name_of(root);
        self.cur.renaming = Some((Renaming::Body(root), name));
    }

    /// ボディの名前を変える(取り消せる)。空なら「ボディN」に戻す。
    pub fn rename_body(&mut self, root: FeatureId, name: &str) -> bool {
        let name = name.trim();
        let cur = self.cur.work.doc().data().body_names.get(&root).cloned();
        if cur.as_deref() == Some(name) || (cur.is_none() && name == self.body_name_of(root)) {
            return false;
        }
        log::info!("ボディの名前を変える: {} → {name}", self.body_name_of(root));
        let tx = edit::rename_body(root, name);
        self.apply(tx)
    }

    /// ボディ(最初に作った機能 `root`)に物理材料を付ける(取り消せる。M26)。空なら外す。
    pub fn set_body_material(&mut self, root: FeatureId, name: &str) -> bool {
        let data = self.cur.work.doc().data();
        if kmb_look::material::of(data).get(root).unwrap_or("") == name.trim() {
            return false;
        }
        log::info!("ボディの材料を替える: {} → {name}", self.body_name_of(root));
        let done = match edit::set_body_material(self.cur.work.doc().data(), root, name) {
            Ok(tx) => self.apply(tx),
            Err(e) => {
                self.win.fail(e);
                false
            }
        };
        self.refresh_body_colors();
        done
    }

    /// ボディ(最初に作った機能 `root`)の外観を替える(取り消せる)。空なら材料の色に戻す。
    pub fn set_body_appearance(&mut self, root: FeatureId, name: &str) -> bool {
        let data = self.cur.work.doc().data();
        let cur = kmb_look::appearance::of(data).bodies.get(&root).cloned();
        if cur.as_deref().unwrap_or("") == name.trim() {
            return false;
        }
        log::info!("ボディの外観を替える: {} → {name}", self.body_name_of(root));
        let done = match edit::set_body_appearance(self.cur.work.doc().data(), root, name) {
            Ok(tx) => self.apply(tx),
            Err(e) => {
                self.win.fail(e);
                false
            }
        };
        self.refresh_body_colors();
        done
    }

    /// ボディ(最初に作った機能 `root`)を塗る色: 外観、無ければ材料の色、どちらも無いか鋼なら `None`。
    /// `None` のボディは 3D のビューの既定の色(既定の材料の鋼と同じ色。ボディごとに明るさを変えて見分ける)。
    pub fn body_appearance_color(&self, root: FeatureId) -> Option<egui::Color32> {
        BodyLooks::read(self.cur.work.doc().data()).color(root)
    }

    /// 3D のビューのボディの色を、文書の材料と外観に合わせる。
    pub(crate) fn refresh_body_colors(&mut self) {
        let data = self.cur.work.doc().data();
        let body = BodyLooks::read(data);
        let roots = self.body_roots();
        let colors: std::collections::BTreeMap<FeatureId, egui::Color32> = roots
            .iter()
            .filter_map(|&(last, root)| body.color(root).map(|c| (last, c)))
            .collect();
        self.cur.scene.set_body_colors(colors);
        // 材質の質感(金属・粗さ・木目)。外観か材料が付いたボディだけ(鋼は既定と同じに描く)
        let looks: std::collections::BTreeMap<FeatureId, kmb_render::Look> = roots
            .iter()
            .filter(|&&(_, root)| !body.looks_default(root))
            .filter_map(|&(last, root)| {
                // 部品の外観も受け継ぐ(色の決め方と同じ。ボディの外観だけを見ると、部品の外観の
                // ボディは色だけ替わり質感が既定のままになる)
                let a = body.looks.appearance_of(data, root);
                let m = body.materials.get(root);
                (a.is_some() || m.is_some())
                    .then(|| (last, kmb_render::Look::of(&body.looks, a, m)))
            })
            .collect();
        self.cur.scene.set_body_looks(looks);
        // 面の外観とデカール(ボディの最後の機能 → 面の名前 → 材質)
        let decals = kmb_render::decal::decals_of(
            self.decor_outputs(kmb_look::decor::DECAL),
            |b| kmb_core::body_root(data, &self.win.registry, b),
            &self.win.decal_images,
            &kmb_session::canvas::decode,
        );
        let faces: kmb_ui_view3d::FaceLooks = roots
            .iter()
            .filter_map(|&(last, root)| {
                let mut looks: std::collections::BTreeMap<String, kmb_render::Look> = body
                    .looks
                    .faces
                    .get(&root)
                    .into_iter()
                    .flatten()
                    .filter_map(|(face, n)| {
                        let r = body.looks.resolve(n)?;
                        Some((face.clone(), kmb_render::Look::resolved(&r)))
                    })
                    .collect();
                // デカールの面は、面の外観(無ければボディの材質。材料も無ければ既定の材料)に絵を重ねる
                for (face, d) in decals.get(&root).into_iter().flatten() {
                    let base = looks.remove(face).unwrap_or_else(|| {
                        let a = body.looks.appearance_of(data, root);
                        let m = Some(body.materials.material_of(root));
                        kmb_render::Look::of(&body.looks, a, m)
                    });
                    looks.insert(face.clone(), base.with_decal(d.clone()));
                }
                (!looks.is_empty()).then_some((last, looks))
            })
            .collect();
        self.cur.scene.set_face_looks(faces);
        self.cur.scene.set_lamps(kmb_render::lamp::lamps_of(
            self.decor_outputs(kmb_look::decor::LAMP),
        ));
        self.refresh_faded();
    }

    /// ボディの右クリックの「物理材料」「外観」の一覧(見本を落とす代わりに選ぶ)。
    fn body_look_menus(&mut self, ui: &mut egui::Ui, root: FeatureId) -> bool {
        let mut done = false;
        let data = self.cur.work.doc().data();
        let material = kmb_look::material::of(data).get(root).map(str::to_string);
        let look = kmb_look::appearance::of(data).bodies.get(&root).cloned();
        let swatch = |ui: &mut egui::Ui, c: [u8; 3]| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
            ui.painter()
                .rect_filled(r, 2.0, egui::Color32::from_rgb(c[0], c[1], c[2]));
        };
        let mut pick_material = None;
        crate::theme::menu_sub(ui, "material", "物理材料", |ui| {
            for m in kmb_look::material::PHYS_MATERIALS {
                // 材料を付けていないボディは既定の鋼に印を付ける
                let on = material
                    .as_deref()
                    .unwrap_or(kmb_look::material::DEFAULT_MATERIAL)
                    == m.name;
                let r = ui
                    .horizontal(|ui| {
                        swatch(ui, m.color);
                        ui.selectable_label(on, format!("{}  {} g/cm³", m.name, m.density))
                    })
                    .inner
                    .on_hover_text("質量はこの密度で出す(測る道具で面を 1 つ選ぶと出る)。加工のセットアップの材料の既定にもなる");
                if r.clicked() {
                    pick_material = Some(m.name.to_string());
                }
            }
            ui.separator();
            if crate::theme::menu_row(ui, "x", "材料を外す", "", material.is_some()).clicked()
            {
                pick_material = Some(String::new());
            }
        });
        let mut pick_look = None;
        crate::theme::menu_sub(ui, "appearance", "外観", |ui| {
            pick_look = self.look_menu(ui, look.as_deref(), "材料の色(既定)");
        });
        if let Some(m) = pick_material {
            self.set_body_material(root, &m);
            done = true;
        }
        if let Some(a) = pick_look {
            self.set_body_appearance(root, &a);
            done = true;
        }
        if done {
            // 3D のビューはこのメニューより先に描き終えているので、次の絵を描かせて色を替える
            ui.ctx().request_repaint();
        }
        done
    }

    /// ボディ `root` の今の名前(付けていなければ「ボディN」。[`kmb_core::body_labels`])。
    pub fn body_name_of(&self, root: FeatureId) -> String {
        let data = self.cur.work.doc().data();
        kmb_core::body_labels(data, &self.win.registry, &[root])
            .pop()
            .unwrap_or_default()
    }

    /// ブラウザのボディの行の右クリック(今使える物)。
    /// ボディの操作だけを出し、ボディを最後に変えた機能(`last`)の操作は出さない。
    pub(crate) fn body_menu(&mut self, ui: &mut egui::Ui, root: FeatureId, last: FeatureId) {
        let mut done = false;
        let words = self.win.words();
        let t = |id: &str| words.t(id);
        if crate::theme::menu_row(ui, "rename", &t("browser-body-rename"), "", true)
            .on_hover_text(t("browser-body-rename-tip"))
            .clicked()
        {
            self.start_rename_body(root);
            done = true;
        }
        done |= self.body_look_menus(ui, root);
        ui.separator();
        done |= self.body_component_menu(ui, root);
        ui.separator();
        let hidden = self.is_body_hidden(root);
        if crate::theme::menu_row(
            ui,
            if hidden { "eye" } else { "eyeoff" },
            &t(if hidden {
                "browser-body-show"
            } else {
                "browser-body-hide"
            }),
            &crate::keys::label("toggle_visible"),
            true,
        )
        .on_hover_text(t("browser-body-visible-tip"))
        .clicked()
        {
            self.set_body_hidden(root, !hidden);
            done = true;
        }
        let roots = self.bodies_to_isolate(root);
        if self.isolated_bodies() == roots.as_slice() {
            if crate::theme::menu_row(ui, "isolate", &t("browser-body-unisolate"), "", true)
                .on_hover_text(t("browser-body-unisolate-tip"))
                .clicked()
            {
                self.end_isolation();
                done = true;
            }
        } else if crate::theme::menu_row(ui, "isolate", &t("browser-body-isolate"), "", true)
            .on_hover_text(t("browser-body-isolate-tip"))
            .clicked()
        {
            self.isolate_bodies(roots);
            done = true;
        }
        if self.any_body_hidden()
            && crate::theme::menu_row(ui, "eye", &t("browser-body-show-all"), "", true)
                .on_hover_text(t("browser-body-show-all-tip"))
                .clicked()
        {
            self.show_all_bodies();
            done = true;
        }
        ui.separator();
        if crate::theme::menu_row(ui, "param", &t("browser-body-param-view"), "", true)
            .on_hover_text(t("browser-body-param-view-tip"))
            .clicked()
        {
            self.make_body_param_view(root);
            done = true;
        }
        if crate::theme::menu_row(ui, "search", &t("browser-body-find-in-timeline"), "", true)
            .on_hover_text(t("browser-body-find-in-timeline-tip"))
            .clicked()
        {
            self.find_in_timeline(last);
            done = true;
        }
        if done {
            ui.close();
        }
    }

    /// 名前を変える窓(右クリックの「名前変更」)。Enter で決め、Esc でやめる。
    pub(crate) fn rename_feature_ui(&mut self, ctx: &egui::Context) {
        let Some((target, mut name)) = self.cur.renaming.clone() else {
            return;
        };
        let mut open = true;
        let mut ok = false;
        let mut cancel = false;
        egui::Window::new("名前変更")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .open(&mut open)
            .show(ctx, |ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(240.0));
                if !r.has_focus() && !r.lost_focus() {
                    r.request_focus();
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    ok = true;
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        ok = true;
                    }
                    if ui.button("キャンセル").clicked() {
                        cancel = true;
                    }
                });
            });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            cancel = true;
        }
        if !open || cancel {
            self.cur.renaming = None;
        } else if ok {
            self.cur.renaming = None;
            match target {
                Renaming::Feature(id) => self.rename_feature(id, &name),
                Renaming::Body(root) => self.rename_body(root, &name),
                Renaming::Component(id) => self.rename_component(id, &name),
            };
        } else {
            self.cur.renaming = Some((target, name));
        }
    }

    /// 機能をタイムラインで 1 つ上(`-1`)か下(`1`)へ動かす。使う機能より前へは動かさない。
    pub fn move_feature(&mut self, id: FeatureId, delta: i32) -> bool {
        let to = self
            .cur
            .work
            .doc()
            .data()
            .index_of(id)
            .map(|i| i as i64 + delta as i64);
        if let Some(to) = to.filter(|t| *t >= 0)
            && let Some(why) = edit::move_conflict(
                self.cur.work.doc().data(),
                &self.win.registry,
                id,
                to as usize,
            )
        {
            self.win.fail(why);
            return false;
        }
        match edit::move_feature(self.cur.work.doc().data(), id, delta) {
            Some(tx) => self.apply(tx),
            None => false,
        }
    }

    /// ロールバック位置を、機能 `id` の後ろにする(`None` なら最後まで進める)。
    pub fn roll_back_after(&mut self, id: Option<FeatureId>) -> bool {
        match edit::roll_back_after(self.cur.work.doc().data(), id) {
            Ok(tx) => self.apply_superseding(tx),
            Err(e) => {
                self.win.fail(e);
                false
            }
        }
    }

    /// ロールバックの棒を 1 つ前(`-1`)か後ろ(`1`)へ。
    pub fn step_rollback(&mut self, delta: i32) -> bool {
        match edit::step_rollback(self.cur.work.doc().data(), delta) {
            Some(tx) => self.apply_superseding(tx),
            None => false,
        }
    }

    /// タイムラインの機能の右クリックメニュー。
    pub(crate) fn feature_menu(&mut self, ui: &mut egui::Ui, id: FeatureId) {
        let Some(f) = self.cur.work.doc().data().feature(id).cloned() else {
            return;
        };
        let n = self.cur.work.doc().data().features.len();
        let i = self.index_of(id).unwrap_or(0);
        let mut done = false;
        let words = self.win.words();
        let t = |id: &str| words.t(id);
        // 注意・エラーのある機能は、まず確かめる・文を写す
        let health = crate::timeline_ui::Health::of(self.cur.work.report().feature(id));
        if health.is_problem() {
            let review = t(if health.is_red() {
                "timeline-menu-review-error"
            } else {
                "timeline-menu-review-warning"
            });
            if crate::theme::menu_row(ui, "warn", &review, "", true)
                .on_hover_text(t("timeline-menu-review-tip"))
                .clicked()
            {
                self.open_review(id);
                done = true;
            }
            if crate::theme::menu_row(ui, "copy", &t("timeline-menu-copy"), "", true)
                .on_hover_text(t("timeline-menu-copy-tip"))
                .clicked()
            {
                ui.ctx().copy_text(self.review_text(id));
                done = true;
            }
            if let crate::timeline_ui::Health::Blocked {
                feature: Some(up), ..
            } = &health
                && crate::theme::menu_row(ui, "select", &t("timeline-menu-select-cause"), "", true)
                    .on_hover_text(t("timeline-menu-select-cause-tip"))
                    .clicked()
            {
                self.select(*up);
                done = true;
            }
            ui.separator();
        }
        // 直すのは、ここかダブルクリックで開いた時だけ(選んだだけでは直さない)
        if crate::theme::menu_row(
            ui,
            "edit",
            &t("timeline-menu-edit"),
            "",
            !self.is_editing(id),
        )
        .on_hover_text(t("timeline-menu-edit-tip"))
        .clicked()
        {
            self.open_edit(id);
            done = true;
        }
        // 2 つ以上を選んでいれば、共通の欄をまとめて直す(編集パネルは道具の間だけ出るので、入口をここにも置く)
        let sel = self.selection();
        if sel.len() >= 2
            && sel.contains(&id)
            && crate::theme::menu_row(
                ui,
                "copy",
                &self
                    .win
                    .t_with("timeline-menu-edit-many", &[("count", sel.len().into())]),
                "",
                !self.cur.multi_edit && self.can_multi_edit(&sel),
            )
            .on_hover_text(t("timeline-menu-edit-many-tip"))
            .clicked()
        {
            self.cur.multi_edit = true;
            done = true;
        }
        // 押し出し・回転などの元のスケッチ(探さずに一手で入る)
        let sketches: Vec<kmb_core::FeatureEntry> =
            edit::used_features(self.cur.work.doc().data(), &self.win.registry, id)
                .into_iter()
                .filter_map(|u| self.cur.work.doc().data().feature(u).cloned())
                .filter(|u| {
                    self.win
                        .registry
                        .descriptor(&u.type_id)
                        .is_some_and(|d| d.category == kmb_core::category::SKETCH)
                })
                .collect();
        for sk in &sketches {
            let named = |id: &str| words.t_with(id, &[("name", sk.name.as_str().into())]);
            let label = if sketches.len() == 1 {
                t("timeline-menu-edit-sketch")
            } else {
                named("timeline-menu-edit-sketch-of")
            };
            if crate::theme::menu_row(ui, "sketch", &label, "", true)
                .on_hover_text(named("timeline-menu-edit-sketch-tip"))
                .clicked()
            {
                self.open_edit(sk.id);
                done = true;
            }
        }
        if crate::theme::menu_row(ui, "rename", &t("timeline-menu-rename"), "", true)
            .on_hover_text(t("timeline-menu-rename-tip"))
            .clicked()
        {
            self.start_rename_feature(id);
            done = true;
        }
        ui.separator();
        // 加工とセットアップの複製・コピー・貼り付け・テンプレート
        if self.reuse_menu(ui, id) {
            done = true;
        }
        // 設計の機能のコピー・貼り付け(U42)
        if self.feature_clip_menu(ui, id) {
            done = true;
        }
        // モジュールの操作(押し出し、残り削り、書き出し、シミュレーションなど)
        let actions: Vec<_> = self
            .actions_for(id)
            .into_iter()
            .filter(|a| a.is_at(Place::FeatureMenu) && self.belongs_here(a))
            .collect();
        // 同じまとまりで同じアイコンの操作(工具ごとの「T○ を順番に足す」のように、同じ操作を相手ごとに
        // 並べた物)が 3 つ以上なら、横に開く一覧にまとめる(メニューが画面の下へはみ出さないように)
        let mut picked = None;
        let mut shown_groups: Vec<&str> = Vec::new();
        for a in &actions {
            let same: Vec<_> = actions
                .iter()
                .filter(|b| !a.menu.is_empty() && b.menu == a.menu && b.icon == a.icon)
                .collect();
            if same.len() >= 3 {
                if shown_groups.contains(&a.menu.as_str()) {
                    continue;
                }
                shown_groups.push(&a.menu);
                let sub = self.win.t_or(crate::ribbon::group_id(&a.menu), &a.menu);
                if let Some(Some(hit)) = crate::theme::menu_sub(ui, &a.icon, &sub, |ui| {
                    let mut hit = None;
                    for b in &same {
                        let keys = self.key_of(&crate::CommandId::Action(b.id.clone()));
                        if crate::action_row(ui, b, &keys).clicked() {
                            hit = Some(b.id.clone());
                        }
                    }
                    hit
                }) {
                    picked = Some(hit);
                }
            } else if crate::action_row(
                ui,
                a,
                &self.key_of(&crate::CommandId::Action(a.id.clone())),
            )
            .clicked()
            {
                picked = Some(a.id.clone());
            }
        }
        if let Some(action) = picked {
            if self.cur.selected != Some(id) {
                self.select(id);
            }
            // クリックで値を決める手順がある操作(押し出しなど)は、その流れに入る
            self.start_action(&action);
            done = true;
        }
        if !actions.is_empty() {
            ui.separator();
        }
        let label = t(if f.suppressed {
            "timeline-menu-unsuppress"
        } else {
            "timeline-menu-suppress"
        });
        if crate::theme::menu_row(ui, "suppress", &label, "", true).clicked() {
            self.set_suppressed(id, !f.suppressed);
            done = true;
        }
        if crate::theme::menu_row(ui, "back", &t("timeline-menu-roll-back"), "", true)
            .on_hover_text(t("timeline-menu-roll-back-tip"))
            .clicked()
        {
            self.roll_back_after(Some(id));
            done = true;
        }
        if self.cur.work.doc().data().rollback.is_some()
            && crate::theme::menu_row(ui, "last", &t("timeline-menu-roll-forward"), "", true)
                .clicked()
        {
            self.roll_back_after(None);
            done = true;
        }
        // 並べ方(順とグループ)
        ui.separator();
        if crate::theme::menu_row(ui, "up", &t("timeline-menu-move-up"), "", i > 0).clicked() {
            self.move_feature(id, -1);
            done = true;
        }
        if crate::theme::menu_row(ui, "down", &t("timeline-menu-move-down"), "", i + 1 < n)
            .clicked()
        {
            self.move_feature(id, 1);
            done = true;
        }
        let sel = self.selection();
        let ids = if sel.contains(&id) { sel } else { vec![id] };
        if crate::theme::menu_row(
            ui,
            "folder",
            &self
                .win
                .t_with("timeline-menu-group", &[("count", ids.len().into())]),
            &crate::keys::label("group"),
            true,
        )
        .clicked()
        {
            self.group_features(&ids, edit::GroupMode::Exact);
            done = true;
        }
        // 端にいるグループから出す
        let data = self.cur.work.doc().data().clone();
        for g in data.groups_of(id) {
            let Some((a, b)) = data.group_range(g) else {
                continue;
            };
            let i = self.index_of(id).unwrap_or(0);
            if (i == a || i == b)
                && crate::theme::menu_row(
                    ui,
                    "ungroup",
                    &self.win.t_with(
                        "timeline-menu-take-out",
                        &[("name", g.name.as_str().into())],
                    ),
                    "",
                    true,
                )
                .clicked()
            {
                if let Ok(tx) = edit::take_out_of_group(&data, g.id, id) {
                    self.apply(tx);
                }
                done = true;
            }
        }
        ui.separator();
        if crate::theme::menu_row(ui, "trash", &t("timeline-menu-delete"), "Delete", true)
            .on_hover_text(t("timeline-menu-delete-tip"))
            .clicked()
        {
            self.ask_remove_features(&[id]);
            done = true;
        }
        if done {
            ui.close();
        }
    }

    /// 操作を今の作業空間の右クリックとコマンド検索に出すか。加工の操作(ポケットなど)は作業空間
    /// 「加工」、解析の操作(結果の見せ方)は作業空間「解析」でだけ使う。
    pub(crate) fn belongs_here(&self, a: &kmb_core::Action) -> bool {
        use crate::Space;
        let own = |s: Space| a.group == s.action_group();
        let here = self.win.layout.space;
        !(own(Space::Machining) && here != Space::Machining
            || own(Space::Simulation) && here != Space::Simulation)
    }

    /// 右クリックの OK: クリックで作る流れなら Enter と同じ、作っている途中・直している途中なら決める。
    pub fn command_ok(&mut self) {
        if self.cur.gesture.is_some() {
            self.gesture_enter();
        } else {
            self.finish_edit();
        }
    }

    /// 右クリックのキャンセル: クリックで作る流れをやめ、作っている途中なら作る前へ戻し、
    /// 直している途中なら直したところを捨てる(Esc を何度か押したのと同じ)。
    pub fn command_cancel(&mut self) {
        self.cur.gesture = None;
        self.cur.picked_edges.clear();
        self.cancel_edit();
    }

    /// 3D のビューの右クリックメニュー(右クリックした面に使える操作)。
    pub(crate) fn view_menu(&mut self, ui: &mut egui::Ui) {
        let mut done = false;
        // 作っている途中・直している途中は、OK とキャンセルを先に出す
        if self.cur.gesture.is_some() || self.cur.creating.is_some() || self.cur.editing.is_some() {
            let blocked = self.ok_blocked();
            let r = crate::theme::menu_row(ui, "check", "OK", "Enter", blocked.is_none());
            let r = match blocked {
                Some(why) => r.on_disabled_hover_text(why),
                None => r.on_hover_text("Enter。決めた値で作る・直したところを決める"),
            };
            if r.clicked() {
                self.command_ok();
                done = true;
            }
            if crate::theme::menu_row(ui, "x", "キャンセル", "Esc", true)
                .on_hover_text("Esc。作るのをやめる・直したところを捨てる")
                .clicked()
            {
                self.command_cancel();
                done = true;
            }
            ui.separator();
        }
        // 重なった物から選ぶ(長押しと同じ一覧。長押しを知らない人への入口で、使い方も添える)
        let in_command = self.cur.gesture.is_some() || self.cur.creating.is_some();
        if !in_command && self.view_menu.context_others.1.len() > 1 {
            if crate::theme::menu_row(ui, "layers", "ほかを選ぶ…(長押しでも)", "", true)
                .on_hover_text(
                    "右クリックした所に重なっている面の一覧から選ぶ。左ボタンの長押しでも出る",
                )
                .clicked()
            {
                self.view_menu.candidates = Some(self.view_menu.context_others.clone());
                done = true;
            }
            ui.separator();
        }
        // 最後に使ったコマンドをもう一度(押し出しを続けて何度も使うときなど)。コマンドの途中は出さない
        if !in_command
            && let Some(key) = self.win.last_item.clone()
            && let Some(it) = self.item_of(&key)
        {
            let r = crate::theme::menu_row(
                ui,
                "repeat",
                &format!("繰り返す: {}", it.label),
                "",
                it.disabled.is_none(),
            );
            let r = match &it.disabled {
                Some(why) => r.on_disabled_hover_text(why),
                None => r.on_hover_text("最後に使ったコマンドをもう一度始める"),
            };
            if r.clicked() {
                self.run_item(&key);
                done = true;
            }
            ui.separator();
        }
        let actions: Vec<kmb_core::Action> = self
            .actions()
            .into_iter()
            .filter(|a| a.is_at(Place::FaceMenu) && self.belongs_here(a))
            .collect();
        let any = !actions.is_empty();
        for a in actions {
            if crate::action_row(
                ui,
                &a,
                &self.key_of(&crate::CommandId::Action(a.id.clone())),
            )
            .clicked()
            {
                self.start_action(&a.id);
                done = true;
            }
        }
        if any {
            ui.separator();
        }
        if !in_command
            && (self.cur.context_hit.is_some()
                || !self.isolated_bodies().is_empty()
                || self.any_body_hidden())
        {
            if self.find_menu_items(ui) {
                done = true;
            }
            ui.separator();
        }
        let measure = if self.is_measuring() {
            "測るのをやめる"
        } else {
            "測る"
        };
        if crate::theme::menu_row(ui, "measure", measure, &crate::keys::label("measure"), true)
            .on_hover_text("点・辺・面をクリックして、距離・角度・径・面積を測る")
            .clicked()
        {
            self.toggle_measure();
            done = true;
        }
        if let Some(p) = self.cur.context_point
            && crate::theme::menu_row(ui, "orbit", "ここを回す中心にする", "", true)
                .on_hover_text("右クリックした面の点を中心に回す")
                .clicked()
        {
            self.cur.camera.set_pivot(p);
            done = true;
        }
        if crate::theme::menu_row(ui, "fit", "全体表示", "Home", true)
            .on_hover_text("Home。全体を収め、部品の中心で回す")
            .clicked()
        {
            self.cur.camera.refit();
            done = true;
        }
        crate::theme::menu_sub(ui, "display", "表示設定", |ui| {
            self.view_style_items(ui)
        });
        if done {
            ui.close();
        }
    }
}

/// ボディの色と質感を決める文書の外観と材料の区画(ボディの数だけ区画を読み直さないよう 1 回読む)。
pub(crate) struct BodyLooks<'a> {
    data: &'a kmb_core::DocumentData,
    pub(crate) looks: kmb_look::appearance::Appearances,
    pub(crate) materials: kmb_look::material::BodyMaterials,
}

impl<'a> BodyLooks<'a> {
    pub(crate) fn read(data: &'a kmb_core::DocumentData) -> Self {
        Self {
            data,
            looks: kmb_look::appearance::of(data),
            materials: kmb_look::material::of(data),
        }
    }

    /// ボディが既定の見た目で描かれるか: 外観も材料も無いか、見た目を決めている物(外観、無ければ
    /// 材料)が既定の材料の鋼。既定は鋼の見た目なので、鋼を選んでも見た目は替えない(色と質感を
    /// 明示した物として描くと、選んだだけで金属の質感に替わり、ボディごとの明るさの見分けも消える)。
    fn looks_default(&self, root: FeatureId) -> bool {
        self.looks
            .appearance_of(self.data, root)
            .or_else(|| self.materials.get(root))
            .is_none_or(|n| {
                kmb_look::material::same_material(n, kmb_look::material::DEFAULT_MATERIAL)
            })
    }

    /// ボディを塗る色(`Workspace::body_appearance_color` と同じ決め方)。
    pub(crate) fn color(&self, root: FeatureId) -> Option<egui::Color32> {
        if self.looks_default(root) {
            return None;
        }
        let rgb = |c: [u8; 3]| egui::Color32::from_rgb(c[0], c[1], c[2]);
        // ボディに外観が無ければ、入っている部品の外観
        if let Some(a) = self
            .looks
            .appearance_of(self.data, root)
            .and_then(|n| self.looks.resolve(n))
        {
            return Some(rgb(a.color));
        }
        self.materials
            .get(root)
            .and_then(kmb_look::material::phys_material)
            .map(|m| rgb(m.color))
    }
}
