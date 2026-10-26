//! ブラウザの部品の木。部品は文書の `components` にあり、ボディを
//! まとめて名前と外観を付け、入れ子にする。部品の行の下に中のボディと子の部品を出し、ボディの
//! まとまり(「ボディ」)には部品に入っていないボディだけを出す。
//! 部品を作る・移す・解く・名前と外観を変えるのは、どれも 1 回の取り消し単位。
//! 別ファイルからリンクで組み込んだ部品は、行にリンクの印と様子(元が変わった・固定・元が無い)を
//! 出し、右クリックに最新にする・固定・リンクを切る・元を開くを並べる。
//!
//! 部品のアクティブ化: 行に乗ると目の左に丸が出て、押すとその部品が
//! アクティブになる(文書の行の丸で直下へ戻る)。アクティブな間に作ったボディと、直下に作った部品は、
//! 計算の後にその部品へ入れる(作った変更と同じ取り消し単位)。ほかの部品のボディは淡く描く。
//! アクティブはタブごとの画面の状態で、文書には入れない。
//! 部品の木の並べ替え: 部品の行を引いて、ほかの部品の行の真ん中へ落とすと中へ、上か下の端へ落とすと
//! その前か後ろへ(同じ親の並び)。ボディの行は部品の行へ落とすと中へ、文書の行か「ボディ」へ落とすと
//! 部品から外す。部品の複製は、中のボディを元の形を追う写し(役割 `kmb_interchange::role::kind::COPY_BODY` の機能)にして、
//! 同じ並びの新しい部品にまとめ、横へずらした位置(役割 `kmb_interchange::role::kind::COMPONENT_POSITION` の機能)に置く。

use crate::Workspace;
use crate::browser_ui::folder_row_radio;
use crate::theme::RightMenu as _;
use kmb_core::{Component, FeatureEntry, FeatureId, LinkInfo, LinkStatus};
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

/// 部品の木のドラッグで動かしている物。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TreeDrag {
    Component(u64),
    /// ボディ(最初に作った機能)
    Body(FeatureId),
}

/// 部品の木で落とす所。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TreeDrop {
    /// 部品の中(`None` は文書の直下)
    Into(Option<u64>),
    /// 同じ親の部品の並びで、部品の前
    Before(u64),
    /// 同じ親の部品の並びで、部品の後ろ
    After(u64),
}

impl TreeDrop {
    fn place(self) -> kmb_session::components::Place {
        use kmb_session::components::Place;
        match self {
            TreeDrop::Into(to) => Place::Into(to),
            TreeDrop::Before(c) => Place::Before(c),
            TreeDrop::After(c) => Place::After(c),
        }
    }
}

fn drag_key(ctx: &egui::Context) -> egui::Id {
    crate::doc_id(ctx, "component_drag")
}

/// リンクの様子を調べ直す間(元のファイルの更新時刻を見るだけなら軽い)。
const LINK_CHECK: Duration = Duration::from_secs(2);

/// 部品の行の横に出す、リンクの様子の短い言葉。
fn link_meta(l: &LinkInfo) -> &'static str {
    match l.status {
        LinkStatus::Latest => "リンク",
        LinkStatus::Stale => "リンク ⚠ 元が変わった",
        LinkStatus::Pinned => "リンク(固定)",
        LinkStatus::Missing => "リンク(元が無い)",
        LinkStatus::Broken => "写し",
    }
}

impl Workspace {
    /// リンクで組み込んだ部品の様子を、前に調べてから [`LINK_CHECK`] たったら調べ直す。元が新しく
    /// 変わったリンクがあれば下の帯で知らせる。
    pub(crate) fn refresh_links(&mut self, ctx: &egui::Context) {
        // egui は描くたびに次の描画の頼みを置き直すので、毎回頼む(頼まないと、キーを押した後などに
        // 調べ直しが止まる)
        if !self.cur.links.is_empty() || self.cur.links_at.is_none() {
            let left = self
                .cur
                .links_at
                .map_or(Duration::ZERO, |t| LINK_CHECK.saturating_sub(t.elapsed()));
            ctx.request_repaint_after(left);
        }
        if self.cur.links_at.is_some_and(|t| t.elapsed() < LINK_CHECK) {
            return;
        }
        let now = self.win.extensions.links(&self.context());
        for l in now.iter().filter(|l| l.status == LinkStatus::Stale) {
            let was = self
                .cur
                .links
                .iter()
                .any(|o| o.feature == l.feature && o.status == LinkStatus::Stale);
            if !was {
                let name = self.link_name(l);
                self.win.message = Some(format!(
                    "「{name}」の元のファイルが変わりました。ブラウザの部品を右クリックして「最新にする」で差し替えます"
                ));
            }
        }
        for l in now.iter().filter(|l| l.status == LinkStatus::Missing) {
            let was = self
                .cur
                .links
                .iter()
                .any(|o| o.feature == l.feature && o.status == LinkStatus::Missing);
            if !was {
                let name = self.link_name(l);
                self.win.message = Some(format!(
                    "「{name}」の元のファイルが見つかりません(写しのまま使います)。ブラウザの部品を右クリックして「元のファイルを選び直す…」でつなぎ直せます"
                ));
            }
        }
        if !now.is_empty() {
            ctx.request_repaint_after(LINK_CHECK);
        }
        self.cur.links = now;
        self.cur.links_at = Some(Instant::now());
    }

    /// 部品 `c` が、リンクで組み込んだ部品ならその様子。
    pub(crate) fn link_of_component(&self, c: &Component) -> Option<LinkInfo> {
        self.cur
            .links
            .iter()
            .find(|l| l.bodies.iter().any(|b| c.bodies.contains(b)))
            .cloned()
    }

    /// リンクの名前(入っている部品の名前、無ければ機能の名前)。
    fn link_name(&self, l: &LinkInfo) -> String {
        let data = self.cur.work.doc().data();
        l.bodies
            .first()
            .and_then(|b| data.component_of(*b))
            .map(|c| c.name.clone())
            .or_else(|| data.feature(l.feature).map(|f| f.name.clone()))
            .unwrap_or_default()
    }

    /// リンクの操作 `id`(最新にする・選び直す・固定・リンクを切る)を、元のファイルを渡して行う。
    /// ファイルを求める操作(選び直す)は窓で選ばせる。変えた後は、した事(元に見つからないボディが
    /// あれば、その名前も)を下の帯に出す。作っている途中の扱い(編集パネル・Esc で戻す)にはしない(すぐ決まる変更)。
    pub(crate) fn run_link_action(&mut self, id: &str, l: &LinkInfo) {
        log::info!("リンク {} の操作 {id}", l.feature.0);
        let input = self
            .actions_for(l.feature)
            .into_iter()
            .find(|a| a.id == id)
            .map(|a| a.input);
        let source = match input {
            Some(kmb_core::Input::OpenFile {
                title,
                filter,
                extensions,
            }) => {
                let exts: Vec<&str> = extensions.iter().map(String::as_str).collect();
                let Some(f) = self.win.dialogs.open_file(&title, &filter, &exts) else {
                    self.win.message =
                        Some(format!("{title}: ファイルが選ばれなかったので、やめました"));
                    return;
                };
                Some(f)
            }
            _ => l.source.clone(),
        };
        let r = self.run_through(
            id,
            Some(l.feature),
            source.as_deref(),
            &serde_json::Value::Null,
        );
        match r.map(|ran| ran.handled) {
            Ok(kmb_session::Handled::Changed(_)) => {
                self.after_change(Ok(()));
                self.refresh_body_colors();
                // した事を下の帯に出す(変更の名前に、元に見つからないボディが並ぶ)
                self.win.message = self
                    .cur
                    .work
                    .doc()
                    .undo_labels()
                    .last()
                    .map(|s| s.to_string());
                // すぐ調べ直す(元が見つからない印を消す)
                self.cur.links_at = None;
            }
            Ok(kmb_session::Handled::Other(o)) => {
                self.take_outcome(*o, None);
            }
            Err(e) => self.win.fail(e),
        }
    }

    /// 部品の木を `list` に置き換える(取り消せる)。
    pub fn set_components(&mut self, label: &str, list: Vec<Component>) -> bool {
        if list == self.cur.work.doc().data().components {
            return false;
        }
        log::info!("部品の木を変える: {label}");
        let done = self.apply(kmb_session::edit::set_components(label, list));
        self.refresh_body_colors();
        done
    }

    /// 部品 `id` の外観を替える(取り消せる。`None` なら外し、中のボディは自分か材料の色に戻る)。
    pub fn set_component_appearance(&mut self, label: &str, id: u64, name: Option<&str>) -> bool {
        let data = self.cur.work.doc().data();
        let cur = kmb_look::appearance::of(data).components.get(&id).cloned();
        if cur.as_deref() == name.map(str::trim).filter(|n| !n.is_empty()) {
            return false;
        }
        log::info!("部品の外観を変える: {label}");
        let done = match kmb_session::edit::set_component_appearance(data, label, id, name) {
            Ok(tx) => self.apply(tx),
            Err(e) => {
                self.win.fail(e);
                false
            }
        };
        self.refresh_body_colors();
        done
    }

    /// 新しい部品を作る操作([`kmb_interchange::role::action::NEW_COMPONENT`]。CLI・MCP と同じ道で、
    /// 名前の既定と親の決め方も同じ)を引数 `args` で流す。作った部品の番号。
    pub(crate) fn new_component(&mut self, args: serde_json::Value) -> Option<u64> {
        let Some(id) = self.action_for_role(&kmb_interchange::role::action::NEW_COMPONENT) else {
            self.win.fail("部品を作る操作がありません");
            return None;
        };
        let ids = |ws: &Self| -> Vec<u64> {
            ws.cur
                .work
                .doc()
                .data()
                .components
                .iter()
                .map(|c| c.id)
                .collect()
        };
        let before = ids(self);
        self.run_action_args(&id, args);
        self.refresh_body_colors();
        ids(self).into_iter().find(|c| !before.contains(c))
    }

    /// 部品の名前を変える(取り消せる)。空なら変えない。
    pub fn rename_component(&mut self, id: u64, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        let mut list = self.cur.work.doc().data().components.clone();
        let Some(c) = list.iter_mut().find(|c| c.id == id) else {
            return false;
        };
        c.name = name.to_string();
        self.set_components(&format!("部品の名前を「{name}」に変更"), list)
    }

    /// M キー: アクティブでない部品のボディだけを選んでいれば、部品ごと値で動かす操作を始める
    /// (中の形だけを動かさない)。始めた・理由を出したら真。偽なら移動/コピーを始める。
    pub(crate) fn move_part_by_key(&mut self) -> bool {
        let shown = self.body_roots();
        let root_of = |f: FeatureId| shown.iter().find(|(b, _)| *b == f).map(|(_, r)| *r);
        let picked: Vec<FeatureId> = self
            .selection()
            .into_iter()
            .chain(self.picked_faces().iter().map(|p| p.feature))
            .collect();
        let Some(roots) = picked
            .iter()
            .map(|f| root_of(*f))
            .collect::<Option<Vec<_>>>()
        else {
            return false;
        };
        let data = self.cur.work.doc().data();
        let Some(c) = roots
            .first()
            .and_then(|r| data.component_of(*r))
            .map(|c| c.id)
        else {
            return false;
        };
        // アクティブにした部品(その中の部品も)の中では、ボディを動かす
        if self
            .active_component()
            .is_some_and(|a| data.component_tree(c).contains(&a))
        {
            return false;
        }
        let role = kmb_interchange::role::action::MOVE_COMPONENT;
        let Some(a) = self.actions().into_iter().find(|a| a.role == Some(role)) else {
            return false;
        };
        match a.disabled {
            Some(why) => self.win.fail(format!("{}: {why}", a.label)),
            None => {
                self.run_action(&a.id);
            }
        }
        true
    }

    /// 選んでいるボディ(最初に作った機能)。ボディの行を選んでいる時だけ。
    fn selected_body_roots(&self, fallback: FeatureId) -> Vec<FeatureId> {
        let bodies: Vec<FeatureId> = self.visible_bodies().iter().map(|b| b.0).collect();
        let data = self.cur.work.doc().data();
        let mut roots: Vec<FeatureId> = self
            .selection()
            .into_iter()
            .filter(|id| bodies.contains(id))
            .map(|id| kmb_core::body_root(data, &self.win.registry, id))
            .collect();
        if !roots.contains(&fallback) {
            roots = vec![fallback];
        }
        roots.dedup();
        roots
    }

    /// ボディの右クリックの「部品」の項目(新しい部品にする・部品へ移す・部品から外す)。
    pub(crate) fn body_component_menu(&mut self, ui: &mut egui::Ui, root: FeatureId) -> bool {
        let roots = self.selected_body_roots(root);
        let mut done = false;
        let now = self.cur.work.doc().data().component_of(root).map(|c| c.id);
        if crate::theme::menu_row(ui, "component", "新しい部品にする", "", true)
            .on_hover_text(
                "選んでいるボディをまとめて部品にする(ブラウザの部品の木に出る。取り消せる)。\
                 ボディがみな同じ部品に入っていればその中に作る",
            )
            .clicked()
        {
            let ids: Vec<u64> = roots.iter().map(|r| r.0).collect();
            self.new_component(serde_json::json!({ "selection": ids }));
            done = true;
        }
        let comps: Vec<(u64, String, usize)> = self.component_list();
        if !comps.is_empty() {
            crate::theme::menu_sub(ui, "move", "部品へ移す", |ui| {
                for (id, name, depth) in &comps {
                    let label = format!("{}{name}", "  ".repeat(*depth));
                    if ui.selectable_label(now == Some(*id), label).clicked() {
                        let list = self
                            .cur
                            .work
                            .doc()
                            .data()
                            .components_moving(&roots, Some(*id));
                        self.set_components(&format!("ボディを部品「{name}」へ移す"), list);
                        done = true;
                        ui.close();
                    }
                }
            });
        }
        if now.is_some()
            && crate::theme::menu_row(ui, "x", "部品から外す", "", true)
                .on_hover_text("ボディを部品から出して、文書の直下のボディにする")
                .clicked()
        {
            let list = self.cur.work.doc().data().components_moving(&roots, None);
            self.set_components("ボディを部品から外す", list);
            done = true;
        }
        done
    }

    /// 部品の一覧(番号、名前、深さ)。木の順。
    fn component_list(&self) -> Vec<(u64, String, usize)> {
        fn walk(
            data: &kmb_core::DocumentData,
            parent: Option<u64>,
            depth: usize,
            out: &mut Vec<(u64, String, usize)>,
        ) {
            for c in data.child_components(parent) {
                if out.iter().any(|o| o.0 == c.id) {
                    continue;
                }
                out.push((c.id, c.name.clone(), depth));
                walk(data, Some(c.id), depth + 1, out);
            }
        }
        let mut out = Vec::new();
        walk(self.cur.work.doc().data(), None, 0, &mut out);
        out
    }

    /// ブラウザの部品の木(文書の直下の部品から)。`numbered` はボディの(番号、最初の機能、最後の機能)。
    pub(crate) fn component_tree_ui(
        &mut self,
        ui: &mut egui::Ui,
        numbered: &[(String, FeatureId, FeatureEntry)],
    ) {
        let top: Vec<u64> = self
            .cur
            .work
            .doc()
            .data()
            .child_components(None)
            .iter()
            .map(|c| c.id)
            .collect();
        for id in top {
            self.component_row(ui, id, 1, numbered, 0);
        }
    }

    fn component_row(
        &mut self,
        ui: &mut egui::Ui,
        id: u64,
        depth: usize,
        numbered: &[(String, FeatureId, FeatureEntry)],
        guard: usize,
    ) {
        let Some(c) = self.cur.work.doc().data().component(id).cloned() else {
            return;
        };
        if guard > self.cur.work.doc().data().components.len() {
            return;
        }
        let bodies: Vec<&(String, FeatureId, FeatureEntry)> = numbered
            .iter()
            .filter(|(_, r, _)| c.bodies.contains(r))
            .collect();
        let all = self.cur.work.doc().data().component_bodies(id);
        let shown = all.is_empty()
            || numbered
                .iter()
                .filter(|(_, r, _)| all.contains(r))
                .any(|(_, r, f)| !self.is_body_hidden(*r, f.id));
        let link = self.link_of_component(&c);
        let meta = match &link {
            Some(l) => link_meta(l).to_string(),
            None if all.is_empty() => "空".to_string(),
            None => String::new(),
        };
        let active = self.active_component() == Some(id);
        let (open, r, eye, radio) = folder_row_radio(
            ui,
            ("component", id),
            (
                depth,
                match &link {
                    Some(l) if l.status != LinkStatus::Broken => "link",
                    _ => "component",
                },
                &c.name,
                &meta,
            ),
            true,
            None,
            Some(shown),
            Some(active),
        );
        // 3D のビューで選んでいる・指している面のボディが中にあれば印(閉じていても分かる)
        if all.iter().any(|b| self.browser.marked.contains(b)) {
            crate::browser_ui::mark_row(ui, r.rect);
        }
        if radio {
            self.activate_component((!active).then_some(id));
        }
        let r = r.interact(egui::Sense::drag());
        self.tree_drag_source(ui, TreeDrag::Component(id), &r);
        self.browser.component_drop_rows.push((Some(id), r.rect));
        if eye {
            for root in &all {
                self.set_body_hidden(*root, shown);
            }
        }
        let r = match &link {
            Some(l) => {
                let from = l
                    .source
                    .as_ref()
                    .map_or_else(String::new, |p| format!("\n元: {}", p.display()));
                r.on_hover_text(format!("{}{from}", l.note))
            }
            None => r,
        };
        r.right_menu(|ui| {
            if let Some(l) = &link {
                self.link_menu(ui, l);
                ui.separator();
            }
            self.component_menu(ui, &c, shown, &all);
        });
        if !open {
            return;
        }
        for (label, root, f) in bodies {
            let row = FeatureEntry {
                name: label.clone(),
                ..f.clone()
            };
            self.browser_row(ui, &row, depth + 1, "", None, true, Some(*root));
        }
        let kids: Vec<u64> = self
            .cur
            .work
            .doc()
            .data()
            .child_components(Some(id))
            .iter()
            .map(|k| k.id)
            .collect();
        for k in kids {
            self.component_row(ui, k, depth + 1, numbered, guard + 1);
        }
    }

    /// リンクで組み込んだ部品の右クリック: 最新にする・この版に固定・リンクを切る(モジュールの操作)と、
    /// 元を開く(別のタブ)。
    fn link_menu(&mut self, ui: &mut egui::Ui, l: &LinkInfo) {
        let actions = self.actions_for(l.feature);
        for id in &l.actions {
            let Some(a) = actions.iter().find(|a| a.id == *id) else {
                continue;
            };
            if crate::action_row(ui, a, "").clicked() {
                self.run_link_action(id, l);
                ui.close();
            }
        }
        let open = crate::theme::menu_row(ui, "folder", "元を開く", "", l.source.is_some())
            .on_hover_text("元のファイルを別のタブで開く(直して保存し、このタブに戻ると印が出る)")
            .on_disabled_hover_text("元のファイルが見つかりません");
        if open.clicked() {
            if let Some(p) = &l.source {
                self.open_in_tab(p);
            }
            ui.close();
        }
    }

    /// 部品の行の右クリック: 名前の変更、外観、表示、中に部品を作る、部品を解く。
    fn component_menu(&mut self, ui: &mut egui::Ui, c: &Component, shown: bool, all: &[FeatureId]) {
        let active = self.active_component() == Some(c.id);
        let (text, tip) = if active {
            (
                "アクティブをやめる",
                "文書の直下をアクティブに戻す(新しく作るボディは部品に入らない)",
            )
        } else {
            (
                "アクティブにする",
                "新しく作るボディと部品をこの部品に入れ、ほかの部品を淡く出す",
            )
        };
        if crate::theme::menu_row(ui, "component", text, "", true)
            .on_hover_text(tip)
            .clicked()
        {
            self.activate_component((!active).then_some(c.id));
            ui.close();
        }
        ui.separator();
        if crate::theme::menu_row(ui, "rename", "名前変更", "", true).clicked() {
            self.cur.renaming = Some((crate::menus_ui::Renaming::Component(c.id), c.name.clone()));
            ui.close();
        }
        let mut pick = None;
        let own = kmb_look::appearance::of(self.cur.work.doc().data())
            .components
            .get(&c.id)
            .cloned();
        crate::theme::menu_sub(ui, "appearance", "外観", |ui| {
            // 外観を付けていない中のボディを、この色で塗る
            pick = self
                .look_menu(ui, own.as_deref(), "なし(ボディの色)")
                .map(|n| (!n.is_empty()).then_some(n));
        });
        if let Some(a) = pick {
            let label = format!("部品「{}」の外観を変更", c.name);
            self.set_component_appearance(&label, c.id, a.as_deref());
            ui.ctx().request_repaint();
        }
        if !all.is_empty()
            && crate::theme::menu_row(
                ui,
                if shown { "eyeoff" } else { "eye" },
                if shown { "非表示" } else { "表示" },
                &crate::keys::label("toggle_visible"),
                true,
            )
            .on_hover_text("中のボディをまとめて隠す・出す")
            .clicked()
        {
            for root in all {
                self.set_body_hidden(*root, shown);
            }
            ui.close();
        }
        ui.separator();
        if crate::theme::menu_row(ui, "plus", "中に新しい部品", "", true)
            .on_hover_text(
                "この部品の中に空の部品を作る(ボディは右クリックの「部品へ移す」で入れる)",
            )
            .clicked()
        {
            self.new_component(serde_json::json!({ "selection": [], "parent": c.id }));
            ui.close();
        }
        if crate::theme::menu_row(ui, "duplicate", "複製", "", true)
            .on_hover_text(
                "中の部品ごと写して横に置く。写しは元の形を追う(元を直すと写しも変わる)。ジョイントは写さない",
            )
            .clicked()
        {
            self.duplicate_component(c.id);
            ui.close();
        }
        if crate::theme::menu_row(ui, "ungroup", "部品を解く", "", true)
            .on_hover_text("部品をやめる。中のボディと部品は外側へ出す(ボディは消えない)")
            .clicked()
        {
            let list = self.cur.work.doc().data().components_without(c.id);
            self.set_components(&format!("部品「{}」を解く", c.name), list);
            ui.close();
        }
    }
}

/// アクティブ化・並べ替え・複製。
impl Workspace {
    /// アクティブな部品(`None` は文書の直下)。
    pub fn active_component(&self) -> Option<u64> {
        self.cur.work.active_component()
    }

    /// 部品 `id` をアクティブにする(`None` で文書の直下へ戻す)。無い部品なら直下へ戻す。
    pub fn activate_component(&mut self, id: Option<u64>) {
        let id = id.filter(|c| self.cur.work.doc().data().component(*c).is_some());
        if let Ok(true) = self.cur.work.activate_component(id) {
            self.refresh_faded();
        }
    }

    /// アクティブな部品の外のボディを、3D のビューで淡く描く。
    pub(crate) fn refresh_faded(&mut self) {
        let data = self.cur.work.doc().data();
        let faded: BTreeSet<FeatureId> = match self
            .active_component()
            .filter(|c| data.component(*c).is_some())
        {
            Some(c) => {
                let inside = data.component_bodies(c);
                self.body_roots()
                    .into_iter()
                    .filter(|(_, root)| !inside.contains(root))
                    .map(|(last, _)| last)
                    .collect()
            }
            None => BTreeSet::new(),
        };
        log::debug!("淡く描くボディ {} 個", faded.len());
        self.cur.scene.set_faded(faded);
    }

    /// 計算の後に、アクティブな部品があった間に足した物を、その部品へ入れる(CLI・MCP と同じ
    /// [`kmb_session::Headless::settle_active`])。
    pub(crate) fn settle_active(&mut self) {
        match self.cur.work.settle_active() {
            kmb_session::Settled::Nothing => {}
            kmb_session::Settled::Moved(_) => {
                self.cur.modified = self.differs_from_saved();
                self.refresh_body_colors();
            }
            kmb_session::Settled::Cleared => self.refresh_faded(),
        }
    }

    /// 部品の木の行を引き始めたら覚える。
    fn tree_drag_source(&mut self, ui: &egui::Ui, what: TreeDrag, r: &egui::Response) {
        if r.drag_started() {
            let key = drag_key(ui.ctx());
            ui.ctx().data_mut(|d| d.insert_temp(key, what));
        }
    }

    /// ボディの行を引き始めたら覚える(ブラウザのボディの行から呼ぶ)。
    pub(crate) fn body_drag_source(&mut self, ui: &egui::Ui, root: FeatureId, r: &egui::Response) {
        self.tree_drag_source(ui, TreeDrag::Body(root), r);
    }

    /// 落とせるか(自分の中・自分の前後・今と同じ所へは落とさない)。
    pub(crate) fn tree_drop_ok(&self, what: TreeDrag, at: TreeDrop) -> bool {
        let data = self.cur.work.doc().data();
        match what {
            TreeDrag::Body(root) => match at {
                TreeDrop::Into(to) => data.component_of(root).map(|c| c.id) != to,
                _ => false,
            },
            TreeDrag::Component(k) => {
                kmb_session::components::can_move_component(data, k, at.place()).is_ok()
            }
        }
    }

    /// 部品の木に落とす: ボディを部品へ移す・外す、部品を別の部品の中や前後へ移す(取り消せる。
    /// CLI・MCP と同じ組み立て)。
    pub(crate) fn drop_in_tree(&mut self, what: TreeDrag, at: TreeDrop) -> bool {
        let data = self.cur.work.doc().data();
        let tx = match what {
            TreeDrag::Body(root) => {
                let TreeDrop::Into(to) = at else {
                    return false;
                };
                let roots = self.selected_body_roots(root);
                kmb_session::components::move_bodies(data, &self.win.registry, &roots, to)
            }
            TreeDrag::Component(k) => kmb_session::components::move_component(data, k, at.place()),
        };
        log::info!("部品の木に落とす: {what:?} → {at:?}");
        let done = match tx {
            Ok(Some(tx)) => self.apply(tx),
            Ok(None) => false,
            Err(e) => {
                self.win.fail(e);
                false
            }
        };
        self.refresh_body_colors();
        done
    }

    /// 部品の木のドラッグの落とす所を示し、離したらそこへ動かす(ブラウザの設計の木を描いた後に呼ぶ)。
    pub(crate) fn finish_component_drag(&mut self, ui: &egui::Ui) {
        let rows = std::mem::take(&mut self.browser.component_drop_rows);
        let key = drag_key(ui.ctx());
        let Some(what) = ui.ctx().data(|d| d.get_temp::<TreeDrag>(key)) else {
            return;
        };
        let pos = ui.input(|i| i.pointer.latest_pos());
        let target = pos.and_then(|p| {
            let (c, rect) = rows.iter().find(|(_, r)| r.contains(p))?;
            let edge = rect.height() * 0.25;
            let at = match (what, c) {
                (TreeDrag::Component(_), Some(c)) if p.y < rect.top() + edge => {
                    TreeDrop::Before(*c)
                }
                (TreeDrag::Component(_), Some(c)) if p.y > rect.bottom() - edge => {
                    TreeDrop::After(*c)
                }
                _ => TreeDrop::Into(*c),
            };
            self.tree_drop_ok(what, at).then_some((at, *rect))
        });
        let ink = self.win.look.pal.design;
        let p = ui.painter();
        match target {
            Some((TreeDrop::Into(_), r)) => {
                p.rect_stroke(
                    r.shrink(1.0),
                    3.0,
                    egui::Stroke::new(2.0, ink),
                    egui::StrokeKind::Inside,
                );
            }
            Some((TreeDrop::Before(_), r)) => {
                p.hline(r.x_range(), r.top() + 1.0, egui::Stroke::new(2.0, ink));
            }
            Some((TreeDrop::After(_), r)) => {
                p.hline(r.x_range(), r.bottom() - 1.0, egui::Stroke::new(2.0, ink));
            }
            None => {}
        }
        if let Some(at) = pos {
            let data = self.cur.work.doc().data();
            let name = match what {
                TreeDrag::Component(k) => data.component(k).map(|c| c.name.clone()),
                TreeDrag::Body(root) => data.feature(root).map(|_| "ボディ".to_string()),
            }
            .unwrap_or_default();
            p.text(
                at + egui::vec2(12.0, -8.0),
                egui::Align2::LEFT_BOTTOM,
                name,
                crate::theme::prop(12.0),
                ui.visuals().strong_text_color(),
            );
        }
        if ui.input(|i| i.pointer.any_down()) {
            ui.ctx().request_repaint();
            return;
        }
        ui.ctx().data_mut(|d| d.remove::<TreeDrag>(key));
        if let Some((at, _)) = target {
            self.drop_in_tree(what, at);
        }
    }

    /// 部品 `id` を中の部品ごと複製する(1 回の取り消し単位。CLI・MCP と同じ組み立て
    /// [`kmb_session::components::duplicate_component`])。作った部品の番号。
    pub fn duplicate_component(&mut self, id: u64) -> Option<u64> {
        let (tx, top) = match self.cur.work.duplicate_component_tx(id) {
            Ok(v) => v,
            Err(e) => {
                self.win.fail(e);
                return None;
            }
        };
        self.apply(tx).then_some(top)
    }
}
