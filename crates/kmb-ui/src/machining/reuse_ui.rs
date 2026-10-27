//! 加工の操作の使い回し(ブラウザの右クリック・キー・ドラッグ): 複製(Ctrl+D)、切り取り・コピー・
//! 貼り付け(Ctrl+X / Ctrl+C / Ctrl+V)、ブラウザのドラッグでの並べ替え、テンプレートから作成。
//! どれが加工かは記述子の区分(`cam.operation`・`cam.setup`)で、どのセットアップに属すかは記述子の
//! 親の欄(`parent`)で決める(画面は加工の中身を知らない)。
//! 写したものは画面の中に持つ。OS のクリップボードには短い文を置く(Ctrl+V はクリップボードに文が
//! ある時だけ貼り付けの合図として届くため。ほかの文をコピーした後の Ctrl+V では貼らない)。

use crate::{Space, Workspace};
use kmb_core::{FeatureEntry, FeatureId, InputRef, category};
use kmb_session::edit;

/// 写した加工。
#[derive(Clone)]
pub(crate) struct OpClip {
    pub entries: Vec<FeatureEntry>,
    /// OS のクリップボードに置いた文(Ctrl+V で届いた文がこれなら貼る)
    pub text: String,
}

/// 名前を打たせてから実行する操作([`kmb_core::Input::Name`])。
#[derive(Clone)]
pub(crate) struct NameAsk {
    pub action: String,
    pub title: String,
    pub label: String,
    pub text: String,
    pub selection: Vec<FeatureId>,
}

/// ブラウザで加工の行をドラッグしている印。
#[derive(Clone, Copy)]
struct BrowserDrag(FeatureId);

/// ブラウザで落とす所。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropAt {
    /// 加工の前
    Before(FeatureId),
    /// 加工の後ろ
    After(FeatureId),
    /// セットアップの加工の最後
    Into(FeatureId),
}

impl Workspace {
    fn category_of(&self, id: FeatureId) -> Option<String> {
        let f = self.cur.work.doc().data().feature(id)?;
        Some(self.win.registry.descriptor(&f.type_id)?.category)
    }

    /// 加工の操作か。
    pub(crate) fn is_operation(&self, id: FeatureId) -> bool {
        self.category_of(id).as_deref() == Some(category::OPERATION)
    }

    /// セットアップか。
    pub(crate) fn is_setup(&self, id: FeatureId) -> bool {
        self.category_of(id).as_deref() == Some(category::SETUP)
    }

    /// 親を指す欄の名前(加工なら `setup`)。
    fn parent_key(&self, type_id: &str) -> Option<String> {
        self.win.registry.descriptor(type_id)?.parent
    }

    /// 加工の属するセットアップ。
    fn parent_of(&self, id: FeatureId) -> Option<FeatureId> {
        let f = self.cur.work.doc().data().feature(id)?;
        let key = self.parent_key(&f.type_id)?;
        f.data.get(&key)?.as_u64().map(FeatureId)
    }

    /// セットアップ `setup` の加工(タイムラインの順)。
    fn operations_of(&self, setup: FeatureId) -> Vec<FeatureId> {
        self.cur
            .work
            .doc()
            .data()
            .features
            .iter()
            .map(|f| f.id)
            .filter(|id| self.is_operation(*id) && self.parent_of(*id) == Some(setup))
            .collect()
    }

    /// 選んだ物の加工(セットアップは中の加工すべて。タイムラインの順、重なりなし)。
    pub(crate) fn operations_in(&self, ids: &[FeatureId]) -> Vec<FeatureId> {
        let mut out: Vec<FeatureId> = Vec::new();
        for f in &self.cur.work.doc().data().features {
            let pick = ids.contains(&f.id) && self.is_operation(f.id)
                || self
                    .parent_of(f.id)
                    .is_some_and(|s| ids.contains(&s) && self.is_operation(f.id));
            if pick && !out.contains(&f.id) {
                out.push(f.id);
            }
        }
        out
    }

    /// 文書の機能の名前(写しの名前を重ならないようにするため)。
    pub(crate) fn feature_names(&self) -> Vec<String> {
        self.cur
            .work
            .doc()
            .data()
            .features
            .iter()
            .map(|f| f.name.clone())
            .collect()
    }

    /// `entry` のデータの親の欄を `parent` にしたもの。
    fn with_parent(&self, entry: &FeatureEntry, parent: FeatureId) -> serde_json::Value {
        let mut data = entry.data.clone();
        if let (Some(key), Some(m)) = (self.parent_key(&entry.type_id), data.as_object_mut()) {
            m.insert(key, serde_json::json!(parent.0));
        }
        data
    }

    /// `entry`(データは `data`)が使う機能のうち、いちばん後ろのタイムラインの位置。
    fn last_used_index(&self, type_id: &str, data: &serde_json::Value) -> Option<usize> {
        let feature = self.win.registry.instantiate(type_id, data)?.ok()?;
        let d = self.cur.work.doc().data();
        feature
            .inputs()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|r| match r {
                InputRef::Output { feature, .. } => Some(feature),
                InputRef::Param { .. } => None,
            })
            .filter_map(|f| d.index_of(f))
            .max()
    }

    /// 加工とセットアップを複製する(右クリックの「複製」、Ctrl+D)。写しは元のすぐ後ろに置き、
    /// 名前に「(2)」などを付ける。セットアップは中の加工も一緒に写す。写したものを選び、その番号を返す。
    pub fn duplicate_features(&mut self, ids: &[FeatureId]) -> Vec<FeatureId> {
        let ids: Vec<FeatureId> = ids
            .iter()
            .copied()
            .filter(|id| self.is_setup(*id) || self.is_operation(*id))
            .collect();
        // 一緒に選んだセットアップの中の加工は、セットアップと一緒に写す
        let ids: Vec<FeatureId> = ids
            .iter()
            .copied()
            .filter(|id| !self.parent_of(*id).is_some_and(|s| ids.contains(&s)))
            .collect();
        if ids.is_empty() {
            self.win.message = Some("複製できるのは加工とセットアップです".into());
            return Vec::new();
        }
        let data = self.cur.work.doc().data().clone();
        let mut order: Vec<FeatureId> = data.features.iter().map(|f| f.id).collect();
        let mut names = self.feature_names();
        let mut ops = kmb_session::edit::Changes::new();
        let mut made = Vec::new();
        let mut insert = |ws: &mut Workspace,
                          order: &mut Vec<FeatureId>,
                          after: FeatureId,
                          entry: &FeatureEntry,
                          data: serde_json::Value|
         -> FeatureId {
            let id = ws.cur.work.alloc_feature_id();
            let name = jc_name(&entry.name, &names);
            names.push(name.clone());
            let at = order
                .iter()
                .position(|x| *x == after)
                .map_or(order.len(), |i| i + 1);
            order.insert(at, id);
            ops.insert_feature(
                Some(at),
                FeatureEntry {
                    id,
                    type_id: entry.type_id.clone(),
                    name,
                    data,
                    suppressed: entry.suppressed,
                    pin_bodies: false,
                },
            );
            id
        };
        for id in &ids {
            let Some(entry) = data.feature(*id).cloned() else {
                continue;
            };
            if self.is_setup(*id) {
                // セットアップの写しは、元のセットアップの加工の後ろに置き、その下に加工を写す
                let children = self.operations_of(*id);
                let tail = children.last().copied().unwrap_or(*id);
                let s = insert(self, &mut order, tail, &entry, entry.data.clone());
                made.push(s);
                let mut after = s;
                for c in children {
                    let Some(ce) = data.feature(c).cloned() else {
                        continue;
                    };
                    let d = self.with_parent(&ce, s);
                    after = insert(self, &mut order, after, &ce, d);
                    made.push(after);
                }
            } else {
                made.push(insert(self, &mut order, *id, &entry, entry.data.clone()));
            }
        }
        let label = format!("{}を複製", edit::names(&data, &ids));
        if !self.apply(ops.into_transaction(label)) {
            return Vec::new();
        }
        log::info!("複製した: {} 個", made.len());
        self.select_many(&made);
        made
    }

    /// 幾つかの機能を選ぶ(最後のものを主に選ぶ)。
    fn select_many(&mut self, ids: &[FeatureId]) {
        if let Some((last, rest)) = ids.split_last() {
            self.select(*last);
            self.cur.also = rest.to_vec();
        }
    }

    /// 選んだ加工(セットアップは中の加工すべて)を写す。写した数を返す。`ctx` があれば OS の
    /// クリップボードにも短い文を置く(Ctrl+V を届かせるため)。
    pub fn copy_operations(&mut self, ctx: Option<&egui::Context>, ids: &[FeatureId]) -> usize {
        let ops = self.operations_in(ids);
        if ops.is_empty() {
            self.win.message = Some("写せるのは加工とセットアップの中の加工です".into());
            return 0;
        }
        let data = self.cur.work.doc().data();
        let entries: Vec<FeatureEntry> = ops
            .iter()
            .filter_map(|id| data.feature(*id).cloned())
            .collect();
        let text = format!(
            "kieMonoBench の加工 {} 個({})",
            entries.len(),
            entries
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>()
                .join("、")
        );
        if let Some(ctx) = ctx {
            ctx.copy_text(text.clone());
        }
        let n = entries.len();
        log::info!("加工を写した: {n} 個");
        self.cur.op_clip = Some(OpClip { entries, text });
        self.win.message = Some(format!(
            "加工 {n} 個を写しました。貼る先のセットアップか加工を選んで Ctrl+V(右クリックの「貼り付け」)"
        ));
        n
    }

    /// 選んだ加工を写してから消す(Ctrl+X)。
    pub fn cut_operations(&mut self, ctx: Option<&egui::Context>, ids: &[FeatureId]) -> usize {
        let n = self.copy_operations(ctx, ids);
        if n > 0 {
            let ops: Vec<FeatureId> = self
                .cur
                .op_clip
                .as_ref()
                .map(|c| c.entries.iter().map(|e| e.id).collect())
                .unwrap_or_default();
            self.ask_remove_features(&ops);
            self.win.message = Some(format!(
                "加工 {n} 個を切り取りました。貼る先のセットアップか加工を選んで Ctrl+V"
            ));
        }
        n
    }

    /// 写したものがあるか。
    pub fn has_op_clip(&self) -> bool {
        self.cur.op_clip.is_some()
    }

    /// 写した加工を貼る。`target` がセットアップならその加工の最後に、加工ならその後ろに置き、
    /// そのセットアップの加工にする(裏返しの 2 つ目のセットアップへ同じ加工を写す時など)。
    /// 貼ったものを選び、その番号を返す。
    pub fn paste_operations(&mut self, target: Option<FeatureId>) -> Vec<FeatureId> {
        let Some(clip) = self.cur.op_clip.clone() else {
            self.win.message = Some("先に加工をコピー(Ctrl+C)してください".into());
            return Vec::new();
        };
        let Some(target) = target else {
            self.win.message = Some("貼る先のセットアップか加工を選んでください".into());
            return Vec::new();
        };
        let (setup, mut after) = if self.is_setup(target) {
            let tail = self.operations_of(target).last().copied().unwrap_or(target);
            (target, tail)
        } else if self.is_operation(target)
            && let Some(s) = self.parent_of(target)
        {
            (s, target)
        } else {
            self.win.message = Some("貼る先のセットアップか加工を選んでください".into());
            return Vec::new();
        };
        let data = self.cur.work.doc().data().clone();
        let mut order: Vec<FeatureId> = data.features.iter().map(|f| f.id).collect();
        let mut names = self.feature_names();
        let mut ops = kmb_session::edit::Changes::new();
        let mut made = Vec::new();
        for e in &clip.entries {
            let d = self.with_parent(e, setup);
            let id = self.cur.work.alloc_feature_id();
            let name = jc_name(&e.name, &names);
            names.push(name.clone());
            let mut at = order
                .iter()
                .position(|x| *x == after)
                .map_or(order.len(), |i| i + 1);
            // 使う機能(スケッチなど)より前には置けないので、その後ろへ
            if let Some(u) = self.last_used_index(&e.type_id, &d) {
                let u = order
                    .iter()
                    .position(|x| Some(*x) == data.features.get(u).map(|f| f.id));
                if let Some(u) = u {
                    at = at.max(u + 1);
                }
            }
            order.insert(at, id);
            ops.insert_feature(
                Some(at),
                FeatureEntry {
                    id,
                    type_id: e.type_id.clone(),
                    name,
                    data: d,
                    suppressed: e.suppressed,
                    pin_bodies: false,
                },
            );
            made.push(id);
            after = id;
        }
        let setup_name = data
            .feature(setup)
            .map_or(String::new(), |f| f.name.clone());
        let label = format!("加工 {} 個を{setup_name}に貼り付け", made.len());
        if !self.apply(ops.into_transaction(label)) {
            return Vec::new();
        }
        log::info!("貼り付けた: {} 個を {setup_name} へ", made.len());
        self.select_many(&made);
        made
    }

    /// ブラウザで加工 `id` を落とした所 `at` へ動かす(別のセットアップの所なら、そのセットアップの
    /// 加工にする)。動かせない時は理由を出して `false`。
    pub fn drop_operation(&mut self, id: FeatureId, at: DropAt) -> bool {
        let data = self.cur.work.doc().data().clone();
        let (Some(entry), Some(from)) = (data.feature(id).cloned(), data.index_of(id)) else {
            return false;
        };
        let (parent, slot) = match at {
            DropAt::Before(y) | DropAt::After(y) => {
                let (Some(p), Some(i)) = (self.parent_of(y), data.index_of(y)) else {
                    return false;
                };
                (
                    p,
                    if matches!(at, DropAt::After(_)) {
                        i + 1
                    } else {
                        i
                    },
                )
            }
            DropAt::Into(s) => {
                let tail = self
                    .operations_of(s)
                    .into_iter()
                    .rfind(|o| *o != id)
                    .unwrap_or(s);
                (
                    s,
                    data.index_of(tail).map_or(data.features.len(), |i| i + 1),
                )
            }
        };
        let to = if from < slot { slot - 1 } else { slot };
        let moved_setup = self.parent_of(id) != Some(parent);
        if to == from && !moved_setup {
            return false;
        }
        let mut ops = kmb_session::edit::Changes::new();
        let mut check = data.clone();
        if moved_setup {
            let d = self.with_parent(&entry, parent);
            if let Some(f) = check.features.iter_mut().find(|f| f.id == id) {
                f.data = d.clone();
            }
            ops.set_feature_data(id, d);
        }
        if let Some(why) = edit::move_conflict(&check, &self.win.registry, id, to) {
            self.win.fail(why);
            return false;
        }
        if to != from {
            ops.move_feature(id, to);
        }
        let label = if moved_setup {
            let s = data
                .feature(parent)
                .map_or(String::new(), |f| f.name.clone());
            format!("{}を{s}へ動かす", entry.name)
        } else {
            format!("{}を動かす", entry.name)
        };
        log::info!("{label}");
        self.apply(ops.into_transaction(label))
    }

    /// 右クリックのメニューの、加工とセットアップの使い回しの項目。押したら真。
    pub(crate) fn reuse_menu(&mut self, ui: &mut egui::Ui, id: FeatureId) -> bool {
        if !(self.is_operation(id) || self.is_setup(id)) {
            return false;
        }
        let mut done = false;
        let sel = self.selection();
        let ids = if sel.contains(&id) { sel } else { vec![id] };
        if crate::theme::menu_row(ui, "duplicate", "複製", "Ctrl+D", true)
            .on_hover_text("すぐ後ろに同じ値の写しを作る(セットアップなら中の加工も)")
            .clicked()
        {
            self.duplicate_features(&ids);
            done = true;
        }
        if crate::theme::menu_row(ui, "cut", "切り取り", "Ctrl+X", true)
            .on_hover_text("加工を写してから消す。別のセットアップに貼ると移る")
            .clicked()
        {
            self.cut_operations(Some(ui.ctx()), &ids);
            done = true;
        }
        if crate::theme::menu_row(ui, "copy", "コピー", "Ctrl+C", true)
            .on_hover_text("加工を写す(セットアップなら中の加工すべて)")
            .clicked()
        {
            self.copy_operations(Some(ui.ctx()), &ids);
            done = true;
        }
        let what = if self.is_setup(id) {
            "このセットアップの最後に貼る"
        } else {
            "この加工の後ろに、同じセットアップの加工として貼る"
        };
        let r = crate::theme::menu_row(ui, "paste", "貼り付け", "Ctrl+V", self.has_op_clip());
        let r = if self.has_op_clip() {
            r.on_hover_text(what)
        } else {
            r.on_disabled_hover_text("先に加工をコピー(Ctrl+C)する")
        };
        if r.clicked() {
            self.paste_operations(Some(id));
            done = true;
        }
        // テンプレート(加工のモジュールの操作。役割の印で探し、まとめて 1 つの一覧に出す)
        let templates: Vec<_> = self
            .actions_for(id)
            .into_iter()
            .filter(|a| a.role == Some(kmb_interchange::role::action::USE_TEMPLATE))
            .map(|a| (a.role_name.clone().unwrap_or_else(|| a.label.clone()), a))
            .collect();
        crate::theme::menu_sub(ui, "lib", "テンプレートから作成", |ui| {
            if templates.is_empty() {
                ui.weak("保存したテンプレートはまだありません。加工の右クリックの「テンプレートとして保存」で作る");
            }
            for (name, a) in &templates {
                let r = crate::theme::menu_row(ui, &a.icon, name, "", a.disabled.is_none())
                    .on_hover_text(&a.hint);
                if r.clicked() {
                    if self.cur.selected != Some(id) {
                        self.select(id);
                    }
                    self.start_action(&a.id);
                    done = true;
                }
            }
        });
        ui.separator();
        done
    }

    /// 加工の使い回しのキー(文字の欄を打っていない時だけ。加工の作業空間で): Ctrl+D 複製、
    /// Ctrl+C コピー、Ctrl+X 切り取り、Ctrl+V 貼り付け。F2 の名前の変更は両方の作業空間で。
    pub(crate) fn reuse_keys(&mut self, ctx: &egui::Context) {
        use egui::{Key, KeyboardShortcut, Modifiers};
        if ctx.input(|i| i.key_pressed(Key::F2) && i.modifiers.is_none())
            && let [id] = self.selection()[..]
        {
            self.start_rename_feature(id);
        }
        if self.win.layout.space != Space::Machining {
            return;
        }
        let dup = KeyboardShortcut::new(Modifiers::COMMAND, Key::D);
        if ctx.input_mut(|i| i.consume_shortcut(&dup)) {
            let sel = self.selection();
            if !sel.is_empty() {
                self.duplicate_features(&sel);
            }
        }
        let events: Vec<egui::Event> = ctx.input(|i| {
            i.events
                .iter()
                .filter(|e| {
                    matches!(
                        e,
                        egui::Event::Copy | egui::Event::Cut | egui::Event::Paste(_)
                    )
                })
                .cloned()
                .collect()
        });
        for e in events {
            let sel = self.selection();
            match e {
                egui::Event::Copy if !sel.is_empty() => {
                    self.copy_operations(Some(ctx), &sel);
                }
                egui::Event::Cut if !sel.is_empty() => {
                    self.cut_operations(Some(ctx), &sel);
                }
                egui::Event::Paste(text)
                    if self.cur.op_clip.as_ref().is_some_and(|c| c.text == text) =>
                {
                    self.paste_operations(self.cur.selected);
                }
                _ => {}
            }
        }
    }

    /// 名前を打たせる操作を始める([`kmb_core::Input::Name`])。
    pub(crate) fn ask_name(&mut self, action: &str, title: &str, label: &str, default: &str) {
        self.cur.name_ask = Some(NameAsk {
            action: action.into(),
            title: title.into(),
            label: label.into(),
            text: default.into(),
            selection: self.selection(),
        });
    }

    /// 名前を打たせる窓。Enter・OK で、打った名前と選んでいた機能で操作を実行する。Esc でやめる。
    pub(crate) fn name_ask_ui(&mut self, ctx: &egui::Context) {
        let Some(mut ask) = self.cur.name_ask.clone() else {
            return;
        };
        let mut open = true;
        let mut ok = false;
        let mut cancel = false;
        egui::Window::new(&ask.title)
            .id(egui::Id::new("name_ask"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(&ask.label);
                let r = ui.add(egui::TextEdit::singleline(&mut ask.text).desired_width(260.0));
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
            self.cur.name_ask = None;
        } else if ok {
            self.cur.name_ask = None;
            self.run_named(&ask);
        } else {
            self.cur.name_ask = Some(ask);
        }
    }

    /// 名前を決めた操作を実行する(指定 `name` と、選んでいた機能 `selection`)。
    pub(crate) fn run_named(&mut self, ask: &NameAsk) -> Option<FeatureId> {
        let sel: Vec<u64> = ask.selection.iter().map(|f| f.0).collect();
        self.run_action_args(
            &ask.action,
            serde_json::json!({"name": ask.text.trim(), "selection": sel}),
        )
    }

    /// 加工の行のドラッグを始める(ブラウザの行から呼ぶ)。
    pub(crate) fn browser_drag_row(&mut self, ui: &egui::Ui, id: FeatureId, r: &egui::Response) {
        if self.is_setup(id) || self.is_operation(id) {
            self.browser.drop_rows.push((id, r.rect));
        }
        if self.is_operation(id) && r.drag_started() {
            let key = crate::doc_id(ui.ctx(), "browser_drag");
            ui.ctx().data_mut(|d| d.insert_temp(key, BrowserDrag(id)));
        }
    }

    /// ドラッグしている加工の落とす所を示し、離したらそこへ動かす(ブラウザの木を描いた後に呼ぶ)。
    pub(crate) fn finish_browser_drag(&mut self, ui: &egui::Ui) {
        let rows = std::mem::take(&mut self.browser.drop_rows);
        let key = crate::doc_id(ui.ctx(), "browser_drag");
        let Some(BrowserDrag(id)) = ui.ctx().data(|d| d.get_temp::<BrowserDrag>(key)) else {
            return;
        };
        let pos = ui.input(|i| i.pointer.latest_pos());
        let target = pos.and_then(|p| {
            let (t, rect) = rows.iter().find(|(_, r)| r.contains(p))?;
            if *t == id {
                return None;
            }
            Some(if self.is_setup(*t) {
                (DropAt::Into(*t), *rect)
            } else if p.y < rect.center().y {
                (DropAt::Before(*t), *rect)
            } else {
                (DropAt::After(*t), *rect)
            })
        });
        let l = self.win.look;
        let p = ui.painter();
        // 落とす所の印: 加工の間なら線、セットアップなら枠
        match target {
            Some((DropAt::Into(_), r)) => {
                p.rect_stroke(
                    r.shrink(1.0),
                    3.0,
                    egui::Stroke::new(2.0, l.pal.cam),
                    egui::StrokeKind::Inside,
                );
            }
            Some((DropAt::Before(_), r)) => {
                p.hline(
                    r.x_range(),
                    r.top() + 1.0,
                    egui::Stroke::new(2.0, l.pal.cam),
                );
            }
            Some((DropAt::After(_), r)) => {
                p.hline(
                    r.x_range(),
                    r.bottom() - 1.0,
                    egui::Stroke::new(2.0, l.pal.cam),
                );
            }
            None => {}
        }
        if let Some(p) = pos {
            ui.painter().text(
                p + egui::vec2(12.0, -8.0),
                egui::Align2::LEFT_BOTTOM,
                self.cur
                    .work
                    .doc()
                    .data()
                    .feature(id)
                    .map_or("", |f| f.name.as_str()),
                crate::theme::prop(12.0),
                ui.visuals().strong_text_color(),
            );
        }
        if ui.input(|i| i.pointer.any_down()) {
            ui.ctx().request_repaint();
            return;
        }
        ui.ctx().data_mut(|d| d.remove::<BrowserDrag>(key));
        if let Some((at, _)) = target {
            self.drop_operation(id, at);
        }
    }
}

/// 文書の中で使っていない名前(決まりは [`kmb_core::unique_name`])。`taken` は使っている名前。
pub(crate) fn jc_name(name: &str, taken: &[String]) -> String {
    kmb_core::unique_name(name, |t| taken.iter().any(|x| x == t))
}
