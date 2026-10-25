//! 右クリックの作るメニューと選ぶ種類のメニュー、正面を見る・表示の切り替え・選んだ物へ寄る。

use super::*;

impl Workspace {
    /// Shift+右クリック: 作るもののメニュー(今の作業空間の「作成」とその仲間)。
    pub(super) fn create_menu(&mut self, ui: &mut egui::Ui) {
        let mut done = false;
        let tabs = self.ribbon_tabs();
        for g in tabs.iter().flat_map(|t| &t.groups).filter(|g| {
            matches!(
                g.name.as_str(),
                "作成" | "2D" | "3D" | "穴" | "セットアップ" | "パターン"
            )
        }) {
            ui.weak(&g.name);
            for it in &g.items {
                if self.ribbon_item_button(ui, it) {
                    done = true;
                }
            }
        }
        if done {
            ui.close();
        }
    }

    /// Ctrl+右クリック: 選ぶ種類。
    pub(super) fn filter_menu(&mut self, ui: &mut egui::Ui) {
        for (k, (label, on, enabled)) in self.pick_filter_items().into_iter().enumerate() {
            let mut v = on;
            if ui
                .add_enabled(
                    enabled,
                    egui::Checkbox::new(&mut v, format!("{label}を選ぶ")),
                )
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
    }

    /// F: 選んだもの(面、なければ選んでいる機能のボディ)の中心を回す中心にする。
    /// 正面から見る向き(外向きの法線)。選んだ面、選んだスケッチの平面、ブラウザで選んだ原点の
    /// 平面の順に探す。どれも無ければ理由。
    pub(crate) fn look_at_target(&self) -> Result<[f32; 3], &'static str> {
        if let Some(f) = self.picked_faces().last() {
            return view3d::face_normal(&self.cur.scene, &f.face)
                .ok_or("この面の向きが分かりません");
        }
        if let Some(p) = self
            .selection()
            .into_iter()
            .rev()
            .find_map(|id| self.sketch_plane(id))
        {
            return Ok(p.normal().map(|v| v as f32));
        }
        if let Some(p) = self
            .cur
            .origin_pick
            .and_then(|i| crate::browser_ui::ORIGIN_PLANES.get(i))
        {
            return Ok(p.normal().map(|v| v as f32));
        }
        Err("先に面・スケッチ・原点の平面を選んでください")
    }

    /// 選んだ面・スケッチ・原点の平面を真正面から見る。向きは 0.3 秒かけて替え、その間に形全体が
    /// 収まる所へ移す(スケッチの「正面から見る」と同じ)。もう正面から見ていれば裏から見る。
    pub fn look_at_selected(&mut self) {
        match self.look_at_target() {
            Ok(n) => self.look_along_fit(n, "正面から見る"),
            Err(why) => self.win.fail(format!("正面から見る: {why}")),
        }
    }

    /// 法線 `n` の向きを真正面から見て、ボディとスケッチの線の全体を収める。
    pub(crate) fn look_along_fit(&mut self, n: [f32; 3], what: &str) {
        let (mut lo, mut hi) = if self.cur.scene.is_empty() {
            ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3])
        } else {
            self.cur.scene.bounds()
        };
        for p in self.sketch_lines().iter().flat_map(|(l, _)| l) {
            for i in 0..3 {
                lo[i] = lo[i].min(p[i] as f32);
                hi[i] = hi[i].max(p[i] as f32);
            }
        }
        let cam = &mut self.cur.camera;
        let rect = self.win.view_rect;
        let back = if lo[0] <= hi[0] && rect.is_positive() {
            // 収まる中心と倍率は「すべて表示」と同じ求め方(向きに依らない球で測る)
            let mut fit = *cam;
            fit.fit_box(lo, hi, rect);
            cam.look_along_or_back_to(n, fit.target, fit.zoom)
        } else {
            cam.look_along_or_back(n)
        };
        log::info!("{what}: 法線 {n:?}{}", if back { "(裏から)" } else { "" });
    }
    /// 選んだボディ・機能を隠す・出す。ボディはボディの目、ほかは行の目を替える。
    /// 1 つでも見えている物があれば全部隠し、全部隠れていれば全部出す。
    pub fn toggle_selected_visible(&mut self) {
        let mut ids: Vec<FeatureId> = self.picked_faces().iter().map(|p| p.feature).collect();
        ids.extend(self.selection());
        ids.dedup();
        if ids.is_empty() {
            self.win.message = Some("隠す・出すボディか機能を選んでください".into());
            return;
        }
        let roots = self.body_roots();
        let target: Vec<(FeatureId, Option<FeatureId>)> = ids
            .iter()
            .map(|id| (*id, roots.iter().find(|(l, _)| l == id).map(|(_, r)| *r)))
            .collect();
        let hide = target.iter().any(|(id, root)| match root {
            Some(r) => !self.is_body_hidden(*r, *id),
            None => !self.is_own_hidden(*id),
        });
        for (id, root) in target {
            match root {
                Some(r) => self.set_body_hidden(r, hide),
                None => self.set_hidden(id, hide),
            }
        }
        log::info!(
            "表示の切り替え: {ids:?} を{}",
            if hide { "隠す" } else { "出す" }
        );
    }

    pub(crate) fn focus_selected(&mut self) {
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        let mut any = false;
        let mut grow = |p: [f32; 3]| {
            for i in 0..3 {
                lo[i] = lo[i].min(p[i]);
                hi[i] = hi[i].max(p[i]);
            }
            any = true;
        };
        let faces = self.picked_faces();
        for f in &faces {
            if let Some(c) = view3d::face_center(&self.cur.scene, f) {
                grow(c);
            }
        }
        if faces.is_empty() {
            for id in self.selection() {
                if let Some((a, b)) = view3d::body_bounds(&self.cur.scene, id) {
                    grow(a);
                    grow(b);
                }
            }
        }
        if any {
            self.cur.camera.focus(lo, hi);
        } else {
            self.cur.camera.refit();
        }
    }
}
