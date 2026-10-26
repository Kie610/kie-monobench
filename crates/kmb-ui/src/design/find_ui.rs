//! 形から機能を探す(「タイムライン内を検索」「ブラウザ内を検索」)と、ボディの選択表示(これだけを表示)。
//!
//! - 探すと、面・辺を作った機能をタイムラインで、面のボディをブラウザで、しばらく光らせて見える所まで
//!   送る。選びは変えない(右クリックした面を選んだまま、どの機能で作ったかを見る)。
//! - 選択表示は、行の目・ボディの目を変えずに上から隠す。やめると前の見え方に戻る。

use crate::select_ui::Hit;
use crate::{Found, Workspace, browser_ui, theme};
use kmb_core::FeatureId;
use std::time::{Duration, Instant};

/// 探した物を光らせる長さ。終わりの方で薄くする
const GLOW: Duration = Duration::from_millis(2500);
const FADE: f32 = 0.8;

impl Workspace {
    /// 面・辺を作った機能。辺は両側の面のうち後で作った方(後の機能が辺の形を決めたので)。
    /// 名前から作った機能が分からない面(読み込んだ STEP)は、その面を持つボディの機能。
    pub(crate) fn creator_of(&self, hit: &Hit) -> Option<FeatureId> {
        let data = self.cur.work.doc().data();
        let of_face = |feature: FeatureId, face: &str| {
            kmb_core::face_feature(face)
                .filter(|id| data.feature(*id).is_some())
                .unwrap_or(feature)
        };
        match hit {
            Hit::Face(p) => Some(of_face(p.feature, &p.face)),
            Hit::Edge(e) => e
                .faces
                .iter()
                .map(|f| of_face(e.feature, f))
                .max_by_key(|id| data.index_of(*id)),
            Hit::Vertex(_) => None,
        }
    }

    /// 機能 `id` をタイムラインで光らせ、見える所まで送る(畳んだグループの中なら開く。10-26 b1)。
    pub(crate) fn find_in_timeline(&mut self, id: FeatureId) {
        let data = self.cur.work.doc().data();
        if let Some(at) = data.index_of(id) {
            let within = |f: FeatureId, ok: fn(usize, usize) -> bool| {
                data.index_of(f).is_some_and(|i| ok(i, at))
            };
            let around: Vec<u64> = data
                .groups
                .iter()
                .filter(|g| within(g.first, |i, at| i <= at) && within(g.last, |i, at| at <= i))
                .map(|g| g.id)
                .collect();
            for g in around {
                self.cur.collapsed.remove(&g);
            }
        }
        self.cur.found = Some((Found::Timeline(id), Instant::now()));
        self.cur.found_scroll = Some(Found::Timeline(id));
        let name = self
            .cur
            .work
            .doc()
            .data()
            .feature(id)
            .map_or(String::new(), |f| f.name.clone());
        self.win.message = Some(format!(
            "「{name}」で作った形です(タイムラインで光っている機能)"
        ));
    }

    /// 機能 `feature` のボディを、ブラウザで光らせ、見える所まで送る。畳んでいれば、文書と
    /// ボディのまとまり、ボディの入っている部品と親の部品を開く(10-26 b1)。
    pub(crate) fn find_in_browser(&mut self, feature: FeatureId) {
        let root = self.body_root(feature);
        self.glow_in_browser(root);
        self.win.layout.browser_open = true;
        self.browser.query.clear();
        self.browser.kind = browser_ui::Kind::All;
        // 探して開いた枝は、選びを外しても閉じない
        for k in self.branches_to(root) {
            self.cur.tree_auto.remove(&k);
            self.cur.tree_open.insert(k, true);
        }
    }

    /// ボディ `root` の行をブラウザで光らせ、次に描く時に見える所まで送る。
    fn glow_in_browser(&mut self, root: FeatureId) {
        self.cur.found = Some((Found::Browser(root), Instant::now()));
        self.cur.found_scroll = Some(Found::Browser(root));
    }

    /// ボディ `root` の行を見せるのに開く枝の鍵(文書と、ボディのまとまりか、入っている部品と親の部品)。
    fn branches_to(&self, root: FeatureId) -> Vec<String> {
        let mut keys = vec![browser_ui::tree_key("doc")];
        let data = self.cur.work.doc().data();
        let mut at = data.component_of(root);
        if at.is_none() {
            keys.push(browser_ui::tree_key("ボディ"));
        }
        // 親をたどる(壊れた文書の輪で回り続けない)
        for _ in 0..=data.components.len() {
            let Some(c) = at else { break };
            keys.push(browser_ui::tree_key(("component", c.id)));
            at = c.parent.and_then(|p| data.component(p));
        }
        keys
    }

    /// 選んでいるボディ(最初に作った機能)。3D のビューで選んだ面・辺・頂点のボディと、選んでいる機能の
    /// ボディ。指しているだけの物は入れない。
    pub(crate) fn selected_bodies(&self) -> Vec<FeatureId> {
        self.bodies_picked(true)
    }

    /// 3D のビューで選んだボディ(ボディを選ぶ時は選んでいる機能も)。
    pub(crate) fn bodies_picked_in_view(&self) -> Vec<FeatureId> {
        self.bodies_picked(self.win.pick_filter.body)
    }

    /// 面・辺・頂点の選びのボディと、`with_selection` なら選んでいる機能のボディ。
    fn bodies_picked(&self, with_selection: bool) -> Vec<FeatureId> {
        let c = &self.cur;
        let selection = if with_selection {
            self.selection()
        } else {
            Vec::new()
        };
        let mut features: Vec<FeatureId> = c
            .picked
            .iter()
            .chain(&c.also_picked)
            .map(|p| p.feature)
            .chain(c.picked_edges.iter().map(|e| e.feature))
            .chain(c.picked_vertices.iter().map(|v| v.feature))
            .chain(selection)
            .collect();
        features.sort();
        features.dedup();
        let roots = self.body_roots();
        let mut out: Vec<FeatureId> = Vec::new();
        for r in features.into_iter().map(|f| self.body_root(f)) {
            if roots.iter().any(|(_, x)| *x == r) && !out.contains(&r) {
                out.push(r);
            }
        }
        out
    }

    /// 3D のビューで新しく選んだボディの行を、ブラウザで見せる(10-25 u3)。閉じていた親の枝を
    /// ひとりでに開き、行を見える所まで送って光らせる(開いた時だけ)。`before` は選ぶ前に 3D のビューで選んでいたボディ
    /// ([`Self::bodies_picked_in_view`])。
    /// ひとりでに開いた枝は、選びを外すと閉じる([`Self::settle_auto_open`])。
    pub(crate) fn reveal_in_browser(&mut self, before: &[FeatureId]) {
        use crate::Space;
        if !matches!(
            self.win.layout.space,
            Space::Design | Space::Render | Space::Animation
        ) {
            return;
        }
        let now = self.bodies_picked_in_view();
        let Some(&root) = now.iter().find(|r| !before.contains(r)) else {
            return;
        };
        // 押して閉じた枝だけを開く(覚えの無い枝は既定で開いている)
        let mut opened = false;
        for k in self.branches_to(root) {
            if self.cur.tree_open.get(&k) == Some(&false) {
                self.cur.tree_open.insert(k.clone(), true);
                self.cur.tree_auto.insert(k);
                opened = true;
            }
        }
        // 畳んだ所から出した行だけ光らせる(見えていた行は選びの印で分かる。押すたびに光らせて
        // 描き直し続けない)。見える所へはいつも送る
        if opened {
            self.glow_in_browser(root);
        } else {
            self.cur.found_scroll = Some(Found::Browser(root));
        }
    }

    /// ひとりでに開いた枝のうち、もう選んでいるボディを見せるのに要らない物を閉じて戻す(毎回描く前に)。
    /// その間に自分で閉じた枝は、ひとりでに開いた物として覚えるのをやめる(開き直したら、自分で開いた物)。
    pub(crate) fn settle_auto_open(&mut self) {
        if self.cur.tree_auto.is_empty() {
            return;
        }
        let open = &self.cur.tree_open;
        self.cur.tree_auto.retain(|k| open.get(k) == Some(&true));
        let keep: std::collections::BTreeSet<String> = self
            .selected_bodies()
            .into_iter()
            .flat_map(|r| self.branches_to(r))
            .collect();
        let back: Vec<String> = self
            .cur
            .tree_auto
            .iter()
            .filter(|k| !keep.contains(*k))
            .cloned()
            .collect();
        for k in back {
            log::debug!("ひとりでに開いた枝 {k} を閉じて戻す");
            self.cur.tree_auto.remove(&k);
            self.cur.tree_open.insert(k, false);
        }
    }

    /// 探した物 `which` を今光らせる強さ(0〜1)。光らせていなければ `None`。
    pub(crate) fn found_glow(&self, ctx: &egui::Context, which: Found) -> Option<f32> {
        let (f, at) = self.cur.found?;
        if f != which {
            return None;
        }
        let t = at.elapsed();
        if t >= GLOW {
            return None;
        }
        ctx.request_repaint();
        let left = (GLOW - t).as_secs_f32();
        Some((left / FADE).min(1.0))
    }

    /// ボディ(最初に作った機能 `roots`)だけを 3D のビューに出す(選択表示)。
    pub fn isolate_bodies(&mut self, mut roots: Vec<FeatureId>) {
        roots.dedup();
        self.cur.isolated = roots;
        self.refresh_hidden();
    }

    /// 選択表示をやめる(目の状態はそのまま)。
    pub fn end_isolation(&mut self) {
        self.cur.isolated.clear();
        self.refresh_hidden();
    }

    /// 選択表示しているボディ(最初に作った機能)。
    pub fn isolated_bodies(&self) -> &[FeatureId] {
        &self.cur.isolated
    }

    /// 選択表示に使うボディ: `root` と、一緒に選んでいるボディ・面のボディ(作った順)。
    pub(crate) fn bodies_to_isolate(&self, root: FeatureId) -> Vec<FeatureId> {
        let roots = self.body_roots();
        let mut out: Vec<FeatureId> = self
            .selection()
            .into_iter()
            .chain(self.picked_faces().into_iter().map(|p| p.feature))
            .map(|id| self.body_root(id))
            .filter(|r| roots.iter().any(|(_, x)| x == r))
            .chain([root])
            .collect();
        let data = self.cur.work.doc().data();
        out.sort_by_key(|id| data.index_of(*id).unwrap_or(usize::MAX));
        out.dedup();
        out
    }

    /// すべてのボディを出す(選択表示をやめ、ボディの目とボディのまとまりの目を開ける)。
    pub fn show_all_bodies(&mut self) {
        self.cur.isolated.clear();
        self.cur.hidden_bodies.clear();
        self.cur.hidden_folders.remove(&browser_ui::Kind::Body);
        let lasts: Vec<FeatureId> = self.body_roots().into_iter().map(|(l, _)| l).collect();
        for l in lasts {
            self.cur.hidden.remove(&l);
        }
        self.refresh_hidden();
    }

    /// 隠しているボディがあるか(「すべてのボディを表示」を使えるか)。
    pub(crate) fn any_body_hidden(&self) -> bool {
        !self.cur.isolated.is_empty()
            || self.cur.hidden_folders.contains(&browser_ui::Kind::Body)
            || self
                .body_roots()
                .iter()
                .any(|(last, root)| self.is_body_hidden(*root, *last))
    }

    /// 選択表示の間、3D のビューの上の真ん中に出す札(どのボディか、やめるボタン)。
    pub(crate) fn isolation_banner(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let name = match self.cur.isolated.as_slice() {
            [] => return,
            [one] => format!("「{}」", self.body_name_of(*one)),
            many => format!("{} 個のボディ", many.len()),
        };
        let l = self.win.look;
        let mut end = false;
        egui::Area::new(egui::Id::new("isolation_banner"))
            .fixed_pos(egui::pos2(rect.center().x - 130.0, rect.top() + 8.0))
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::new()
                    .fill(theme::mix(l.pal.panel, l.ws(), 0.12))
                    .stroke(egui::Stroke::new(1.0, l.ws()))
                    .corner_radius(crate::theme::corner(6.0))
                    .inner_margin(egui::Margin::symmetric(10, 4))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!("選択表示中: {name}だけを表示"))
                                    .color(l.pal.ink),
                            );
                            if ui
                                .button("選択表示の解除")
                                .on_hover_text("ほかのボディも前と同じように出す")
                                .clicked()
                            {
                                end = true;
                            }
                        });
                    });
            });
        if end {
            self.end_isolation();
        }
    }

    /// 3D のビューの右クリックの「探す」と「選択表示」の行(右クリックした面・辺に使う)。
    /// 何か押したら真。
    /// 右クリックした面を真正面から見る(下の帯の「正面」と同じ向き替えで、選びは変えない)。
    pub(crate) fn look_at_face(&mut self, f: &crate::view3d::Pick) {
        match crate::view3d::face_normal(&self.cur.scene, &f.face) {
            Some(n) => self.look_along_fit(n, "正面から見る(右クリック)"),
            None => self.win.fail("正面から見る: この面の向きが分かりません"),
        }
    }

    pub(crate) fn find_menu_items(&mut self, ui: &mut egui::Ui) -> bool {
        let mut done = false;
        let hit = self.cur.context_hit.clone();
        if let Some(id) = hit.as_ref().and_then(|h| self.creator_of(h))
            && crate::theme::menu_row(ui, "search", "タイムライン内を検索", "", true)
                .on_hover_text("この面・辺を作った機能を、タイムラインで光らせる")
                .clicked()
        {
            self.find_in_timeline(id);
            done = true;
        }
        // 面を真正面から見る。選びは変えない
        if let Some(Hit::Face(f)) = &hit
            && crate::theme::menu_row(ui, "lookat", "正面から見る", "", true)
                .on_hover_text("この面を真正面から見る。形全体を収める")
                .clicked()
        {
            self.look_at_face(f);
            done = true;
        }
        let body = match &hit {
            Some(Hit::Face(p)) => Some(p.feature),
            Some(Hit::Edge(e)) => Some(e.feature),
            Some(Hit::Vertex(v)) => Some(v.feature),
            None => None,
        };
        if let Some(b) = body {
            if crate::theme::menu_row(ui, "search", "ブラウザ内を検索", "", true)
                .on_hover_text("この形のボディを、ブラウザで光らせる")
                .clicked()
            {
                self.find_in_browser(b);
                done = true;
            }
            let root = self.body_root(b);
            let roots = self.bodies_to_isolate(root);
            if self.cur.isolated != roots
                && crate::theme::menu_row(ui, "isolate", "選択表示", "", true)
                    .on_hover_text(
                        "このボディ(と一緒に選んでいるボディ)だけを出す。「選択表示の解除」で戻す",
                    )
                    .clicked()
            {
                self.isolate_bodies(roots);
                done = true;
            }
        }
        if !self.cur.isolated.is_empty()
            && crate::theme::menu_row(ui, "isolate", "選択表示の解除", "", true)
                .on_hover_text("ほかのボディも前と同じように出す")
                .clicked()
        {
            self.end_isolation();
            done = true;
        }
        if self.any_body_hidden()
            && crate::theme::menu_row(ui, "eye", "すべてのボディを表示", "", true)
                .on_hover_text("選択表示をやめ、隠したボディを全部出す")
                .clicked()
        {
            self.show_all_bodies();
            done = true;
        }
        done
    }
}
