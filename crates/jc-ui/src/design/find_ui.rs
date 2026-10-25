//! 形から機能を探す(「タイムライン内を検索」「ブラウザ内を検索」)と、ボディの選択表示(これだけを表示)。
//!
//! - 探すと、面・辺を作った機能をタイムラインで、面のボディをブラウザで、しばらく光らせて見える所まで
//!   送る。選びは変えない(右クリックした面を選んだまま、どの機能で作ったかを見る)。
//! - 選択表示は、行の目・ボディの目を変えずに上から隠す。やめると前の見え方に戻る。

use crate::select_ui::Hit;
use crate::{Found, Workspace, browser_ui, theme};
use jc_core::FeatureId;
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
            jc_core::face_feature(face)
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

    /// 機能 `id` をタイムラインで光らせ、見える所まで送る。
    pub(crate) fn find_in_timeline(&mut self, id: FeatureId) {
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

    /// 機能 `feature` のボディを、ブラウザで光らせ、見える所まで送る(畳んでいれば開く)。
    pub(crate) fn find_in_browser(&mut self, feature: FeatureId) {
        let root = self.body_root(feature);
        self.cur.found = Some((Found::Browser(root), Instant::now()));
        self.cur.found_scroll = Some(Found::Browser(root));
        self.win.layout.browser_open = true;
        self.browser.query.clear();
        self.browser.kind = browser_ui::Kind::All;
        self.browser.open_folder = true;
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
            && ui
                .button("タイムライン内を検索")
                .on_hover_text("この面・辺を作った機能を、タイムラインで光らせる")
                .clicked()
        {
            self.find_in_timeline(id);
            done = true;
        }
        // 面を真正面から見る。選びは変えない
        if let Some(Hit::Face(f)) = &hit
            && ui
                .button("正面から見る")
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
            if ui
                .button("ブラウザ内を検索")
                .on_hover_text("この形のボディを、ブラウザで光らせる")
                .clicked()
            {
                self.find_in_browser(b);
                done = true;
            }
            let root = self.body_root(b);
            let roots = self.bodies_to_isolate(root);
            if self.cur.isolated != roots
                && ui
                    .button("選択表示")
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
            && ui
                .button("選択表示の解除")
                .on_hover_text("ほかのボディも前と同じように出す")
                .clicked()
        {
            self.end_isolation();
            done = true;
        }
        if self.any_body_hidden()
            && ui
                .button("すべてのボディを表示")
                .on_hover_text("選択表示をやめ、隠したボディを全部出す")
                .clicked()
        {
            self.show_all_bodies();
            done = true;
        }
        done
    }
}
