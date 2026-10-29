//! 3D のビューのクリック・範囲・長押し・右クリックを選びに直す。

use super::*;

/// 重なった面の一覧の幅(論理の点。字の大きさの設定で伸びる)。
const CANDIDATE_WIDTH: f32 = 280.0;

impl Workspace {
    /// 3D のビューの右クリック: メニューが使う、押した所の物と重なった面を覚え、押した面を選ぶ。
    pub(crate) fn context_pick(&mut self, rect: Rect, p: Pos2) {
        // 右クリックした所の物(「タイムライン内を検索」)と、重なった面(「ほかを選ぶ」)
        let all = view3d::picks_at(&self.cur.scene, &self.cur.camera, rect, p);
        let f = if self.win.pick_filter.body {
            PickFilter::FACE
        } else {
            self.win.pick_filter
        };
        self.cur.context_hit = self.hit_at(rect, p, f);
        self.view_menu.context_others = (p, all.iter().map(|h| h.0.clone()).collect());
        // 選んでいる辺・頂点の上の右クリックは選びを変えない(複数の辺を選んで右クリックすると、
        // 下の面が選びに入り、メニューがその面に効いていた。10-25 u7)
        let on_picked = match &self.cur.context_hit {
            Some(Hit::Edge(e)) => self.cur.picked_edges.contains(e),
            Some(Hit::Vertex(v)) => self.cur.picked_vertices.contains(v),
            _ => false,
        };
        // 右クリックした面も選ぶ(メニューはその面に使える操作)
        let hit = all.into_iter().next();
        if let Some((pick, at)) = hit {
            self.cur.selected_component = None;
            if !on_picked && !self.picked_faces().contains(&pick) {
                self.cur.picked = Some(pick);
                self.cur.also_picked.clear();
            }
            self.cur.context_point = Some(at);
        } else {
            self.cur.context_point = None;
        }
    }

    pub(super) fn pick_events(&mut self, ui: &egui::Ui, ev: &view3d::ViewEvents, rect: Rect) {
        let m = ui.input(|i| i.modifiers);
        self.pick_events_with(ev, rect, m);
    }

    pub(crate) fn pick_events_with(
        &mut self,
        ev: &view3d::ViewEvents,
        rect: Rect,
        m: egui::Modifiers,
    ) {
        if ev.click.is_none() && ev.boxed.is_none() {
            self.pick_events_now(ev, rect, m);
            return;
        }
        // 新しく選んだボディの行をブラウザで見せる(10-25 u3)
        let before = self.bodies_picked_in_view();
        self.pick_events_now(ev, rect, m);
        self.reveal_in_browser(&before);
    }

    fn pick_events_now(&mut self, ev: &view3d::ViewEvents, rect: Rect, m: egui::Modifiers) {
        // 点の欄が受けている間は、頂点のクリックをその欄に入れる
        if self.active_point_pick().is_some() {
            if let Some(p) = ev.click.filter(|_| ev.double_click.is_none()) {
                self.point_pick_click(rect, p);
            }
            return;
        }
        // 選ぶ欄が受けている間は、クリックをその欄に入れる(範囲と長押しは使わない)。
        // ダブルクリックの 2 回目は捨てる(足して外すにならないように。つながる辺は欄の機能が入れる)
        if self.active_pick_field().is_some() {
            if let Some(p) = ev.click.filter(|_| ev.double_click.is_none()) {
                self.pick_field_click(rect, p, ev.add || m.command);
            }
            return;
        }
        if ev.click.is_some() || ev.boxed.is_some() {
            self.cur.selected_component = None;
        }
        let f = self.win.pick_filter;
        if !f.body
            && let Some(p) = ev.click
        {
            let hit = self.hit_at(rect, p, f);
            match (hit, ev.double_click.is_some()) {
                // 辺のダブルクリック: 接してつながる辺の輪を選ぶ(Shift で足す、Ctrl で 1 回目と同じく足す・外す)
                (Some(Hit::Edge(e)), true) => self.pick_edge_chain(e, m.shift, m.command),
                (hit, _) => self.pick_hit(hit, m.shift, m.command),
            }
        }
        // 範囲: 面を選べれば面、無ければ辺、無ければ頂点。左から右へ囲むと全部入った物、
        // 右から左へ囲むとかかった物(U30)
        let crossing = ev.crossing;
        if !f.body && !f.face_on() && f.edge_on() {
            if let Some(area) = ev.boxed {
                if !m.shift {
                    self.cur.picked_edges.clear();
                }
                self.scene_edges();
                let inside: Vec<kmb_core::EdgePick> = self
                    .cur
                    .edge_cache
                    .1
                    .iter()
                    .flat_map(|(f, l)| l.iter().map(move |e| (*f, e)))
                    .filter(|(_, e)| {
                        let line: Vec<egui::Pos2> = view3d::edge_polyline(e)
                            .iter()
                            .map(|p| self.cur.camera.project(p.map(|c| c as f32), rect).0)
                            .collect();
                        if crossing {
                            line.windows(2)
                                .any(|w| view3d::segment_touches(area, w[0], w[1]))
                        } else {
                            line.iter().all(|p| area.contains(*p))
                        }
                    })
                    .map(|(feature, e)| kmb_core::EdgePick {
                        feature,
                        faces: e.faces.clone(),
                    })
                    .collect();
                let inside = self.inside_active(inside, |e| e.feature);
                for e in inside {
                    if !self.cur.picked_edges.contains(&e) {
                        self.cur.picked_edges.push(e);
                    }
                }
            }
            return;
        }
        if !f.body && !f.face_on() && !f.edge_on() {
            if let Some(area) = ev.boxed {
                if !m.shift {
                    self.cur.picked_vertices.clear();
                }
                self.scene_edges();
                let inside: Vec<kmb_core::VertexPick> = self
                    .cur
                    .vertex_cache
                    .iter()
                    .flat_map(|(f, l)| l.iter().map(move |v| (*f, v)))
                    .filter(|(_, v)| {
                        area.contains(self.cur.camera.project(v.at.map(|c| c as f32), rect).0)
                    })
                    .map(|(feature, v)| kmb_core::VertexPick {
                        feature,
                        vertex: v.name.clone(),
                    })
                    .collect();
                let inside = self.inside_active(inside, |v| v.feature);
                for v in inside {
                    if !self.cur.picked_vertices.contains(&v) {
                        self.cur.picked_vertices.push(v);
                    }
                }
            }
            return;
        }
        if f.body
            && let Some(p) = ev.click
        {
            let hit = view3d::pick_at(&self.cur.scene, &self.cur.camera, rect, p);
            self.pick_with(hit, m.shift, m.command);
        }
        if let Some(area) = ev.boxed {
            match f.body {
                false => {
                    let faces =
                        view3d::picks_in(&self.cur.scene, &self.cur.camera, rect, area, crossing);
                    let faces = self.inside_active(faces, |p| p.feature);
                    if !m.shift {
                        self.cur.also_picked.clear();
                        self.cur.picked = None;
                        self.cur.picked_edges.clear();
                        self.cur.picked_vertices.clear();
                    }
                    for f in faces {
                        self.pick_with(Some(f), true, false);
                    }
                }
                true => {
                    let bodies =
                        view3d::bodies_in(&self.cur.scene, &self.cur.camera, rect, area, crossing);
                    let bodies = self.inside_active(bodies, |b| *b);
                    for (i, b) in bodies.into_iter().enumerate() {
                        if i == 0 && !m.shift {
                            self.select(b);
                        } else if !self.selection().contains(&b) {
                            self.toggle_select(b);
                        }
                    }
                }
            }
        }
        if let Some(p) = ev.long_press {
            let list: Vec<Pick> = view3d::picks_at(&self.cur.scene, &self.cur.camera, rect, p)
                .into_iter()
                .map(|h| h.0)
                .collect();
            if list.len() > 1 {
                self.view_menu.candidates = Some((p, list));
            }
        }
    }

    /// 囲んだ物 `list` から、作業中(アクティブ)の部品の外の物(淡く描いているボディの物)を除く(10-25 u10)。
    /// 外の物は 1 つずつ押せば選べる。除いた時は知らせる。
    fn inside_active<T>(&mut self, mut list: Vec<T>, feature: impl Fn(&T) -> FeatureId) -> Vec<T> {
        let faded = self.cur.scene.faded();
        if faded.is_empty() {
            return list;
        }
        let n = list.len();
        list.retain(|x| !faded.contains(&feature(x)));
        if list.len() < n {
            log::debug!("囲み: アクティブな部品の外の {} 個を除いた", n - list.len());
            self.win.message = Some(format!(
                "アクティブな部品の外の {} 個は囲みでは選びません(押せば選べます)",
                n - list.len()
            ));
        }
        list
    }

    /// 面 `hit` を選ぶ。Shift で足す、Ctrl で足す・外す。何もない所なら選びを外す。
    pub(crate) fn pick_with(&mut self, hit: Option<Pick>, add: bool, remove: bool) {
        if self.win.pick_filter.body {
            match (hit, add, remove) {
                (Some(h), false, false) => self.select(h.feature),
                (Some(h), true, _) if !self.selection().contains(&h.feature) => {
                    self.toggle_select(h.feature)
                }
                (Some(h), _, true) if self.selection().contains(&h.feature) => {
                    self.toggle_select(h.feature)
                }
                (None, false, false) => self.clear_selection(),
                _ => {}
            }
            return;
        }
        match (hit, add, remove) {
            // Ctrl: 選んでいなければ足し、選んでいれば外す
            (Some(h), _, true) if !self.picked_faces().contains(&h) => {
                if let Some(old) = self.cur.picked.take() {
                    self.cur.also_picked.push(old);
                }
                self.cur.picked = Some(h);
            }
            (Some(h), _, true) => {
                if self.cur.picked.as_ref() == Some(&h) {
                    self.cur.picked = self.cur.also_picked.pop();
                } else {
                    self.cur.also_picked.retain(|p| *p != h);
                }
            }
            (Some(h), true, _) => {
                if !self.picked_faces().contains(&h) {
                    if let Some(old) = self.cur.picked.take() {
                        self.cur.also_picked.push(old);
                    }
                    self.cur.picked = Some(h);
                }
            }
            (Some(h), false, false) => {
                self.cur.also_picked.clear();
                self.cur.origin_pick = None;
                self.cur.picked = Some(h);
            }
            (None, false, false) => {
                self.cur.also_picked.clear();
                self.cur.origin_pick = None;
                self.cur.picked = None;
            }
            _ => {}
        }
    }

    /// 長押しで出す、重なった面の一覧。
    pub(super) fn candidates_popup(&mut self, ui: &egui::Ui) {
        let Some((pos, list)) = self.view_menu.candidates.clone() else {
            return;
        };
        let mut chosen = None;
        let mut close = false;
        let (candidates, cancel) = (self.win.t("view-candidates"), self.win.t("common-cancel"));
        let r = egui::Area::new(egui::Id::new("pick_candidates"))
            .fixed_pos(pos + egui::vec2(8.0, 8.0))
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    // 長い名前は … で切って一覧の幅に収め、指すと全部の名前を出す
                    let max_w = crate::theme::fs(CANDIDATE_WIDTH);
                    ui.set_max_width(max_w);
                    ui.weak(&candidates);
                    for p in &list {
                        let name = self
                            .cur
                            .work
                            .doc()
                            .data()
                            .feature(p.feature)
                            .map_or(String::new(), |f| f.name.clone());
                        let label = format!("{name}: {}", self.face_label(&p.face));
                        let b = ui.add(egui::Button::new(label.as_str()).truncate());
                        let full_w = ui.fonts_mut(|f| {
                            f.layout_no_wrap(
                                label.clone(),
                                egui::TextStyle::Button.resolve(ui.style()),
                                egui::Color32::WHITE,
                            )
                            .size()
                            .x
                        });
                        let b = if full_w + 2.0 * ui.spacing().button_padding.x > max_w {
                            b.on_hover_text(label)
                        } else {
                            b
                        };
                        if b.clicked() {
                            chosen = Some(p.clone());
                        }
                    }
                    if ui.button(&cancel).clicked() {
                        close = true;
                    }
                });
            });
        if r.response.clicked_elsewhere() {
            close = true;
        }
        if let Some(p) = chosen {
            self.pick_with(Some(p), false, false);
            close = true;
        }
        if close {
            self.view_menu.candidates = None;
        }
    }
}
