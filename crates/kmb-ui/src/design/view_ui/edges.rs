//! 辺を拾う・選ぶ: マウスの下の辺、見えている辺、つながった辺の輪、焦点のモード、強く出す面。

use super::*;

impl Workspace {
    /// 画面上の点 `pos` のいちばん近くの、見えている辺(GPU の有無によらず CPU で拾う)。
    pub(crate) fn edge_hit(&mut self, rect: Rect, pos: Pos2) -> Option<kmb_core::EdgePick> {
        self.scene_edges();
        let see_through = !self.win.layout.shading.faces();
        view3d::edge_at(
            &self.cur.scene,
            &self.cur.camera,
            rect,
            pos,
            &self.cur.edge_cache.1,
            see_through,
        )
    }

    /// 面を明るくしない時か(辺を選ぶ手順の途中、面を選べない時、前のフレームでマウスの下が
    /// 頂点か辺だった時)。
    pub(super) fn no_face_hover(&self) -> bool {
        match &self.cur.gesture {
            Some(g) => g.current().is_some_and(|h| {
                matches!(h.gesture, kmb_core::Gesture::Edges)
                    && (!h.faces
                        || matches!(self.cur.hover_kind, Some(RefKind::Vertex | RefKind::Edge)))
            }),
            None if self.active_point_pick().is_some() => true,
            None if self.active_pick_field().is_some() => {
                matches!(self.cur.hover_kind, Some(RefKind::Vertex | RefKind::Edge))
            }
            None => {
                let f = self.win.pick_filter;
                (!f.face_on() && !f.body)
                    || matches!(self.cur.hover_kind, Some(RefKind::Vertex | RefKind::Edge))
            }
        }
    }

    /// マウスの下の物(明るくする物)。手順の途中は辺を選ぶ手順の時だけ辺、選ぶ欄が受けている間は
    /// 欄の受ける頂点・辺、それ以外は選べる種類の物。
    pub(super) fn hover_hit(&mut self, rect: Rect, pos: Pos2) -> Option<Hit> {
        if let Some(g) = &self.cur.gesture {
            return match g.current() {
                // 面も受ける辺の手順(フィレット・面取り)は面も明るくする
                Some(h) if matches!(h.gesture, kmb_core::Gesture::Edges) && h.faces => self.hit_at(
                    rect,
                    pos,
                    PickFilter {
                        vertex: false,
                        ..PickFilter::default()
                    },
                ),
                Some(h) if matches!(h.gesture, kmb_core::Gesture::Edges) => {
                    self.edge_hit(rect, pos).map(Hit::Edge)
                }
                _ => None,
            };
        }
        if self.active_point_pick().is_some() {
            return self.vertex_hit(rect, pos).map(Hit::Vertex);
        }
        if let Some((_, field)) = self.active_pick_field() {
            let kmb_core::FieldKind::Pick { accept, .. } = &field.kind else {
                return None;
            };
            let ff = self.effective_pick_filter();
            let f = PickFilter {
                vertex: ff.vertex_on() && accept.contains(&RefKind::Vertex),
                edge: ff.edge_on() && accept.contains(&RefKind::Edge),
                face: false,
                body: false,
            };
            return self.hit_at(rect, pos, f);
        }
        if self.win.pick_filter.body {
            return None;
        }
        self.hit_at(rect, pos, self.win.pick_filter)
    }

    /// 選んだ辺と、マウスの近くの辺を太く描く。
    pub(super) fn draw_edges(&mut self, ui: &egui::Ui, rect: Rect, hover: Option<&Hit>) {
        let hover = match hover {
            Some(Hit::Edge(e)) => Some(e.clone()),
            _ => None,
        };
        if self.cur.picked_edges.is_empty() && hover.is_none() {
            return;
        }
        self.scene_edges();
        let painter = ui.painter_at(rect);
        let color = self.win.look.ws();
        let soft = crate::theme::mix(self.win.look.pal.panel, color, 0.6);
        let picked = self.shown_edges();
        let lines = picked
            .iter()
            .map(|e| (e, 3.0, color))
            .chain(hover.iter().map(|e| (e, 2.5, soft)));
        for (e, width, c) in lines {
            for (a, b) in view3d::edge_ends(&self.cur.edge_cache.1, e) {
                let f32s = |p: [f64; 3]| p.map(|v| v as f32);
                if let Some((p, q)) = self.cur.camera.segment(f32s(a), f32s(b), rect) {
                    painter.line_segment([p, q], egui::Stroke::new(width, c));
                }
            }
        }
    }

    /// 辺 `hit` を選ぶ。Shift で足す、Ctrl で足す・外す。何もない所なら選びを外す。
    pub(crate) fn pick_edge_with(
        &mut self,
        hit: Option<kmb_core::EdgePick>,
        add: bool,
        remove: bool,
    ) {
        match (hit, add, remove) {
            (Some(h), _, true) if !self.cur.picked_edges.contains(&h) => {
                self.cur.picked_edges.push(h)
            }
            (Some(h), _, true) => self.cur.picked_edges.retain(|e| *e != h),
            (Some(h), true, _) => {
                if !self.cur.picked_edges.contains(&h) {
                    self.cur.picked_edges.push(h);
                }
            }
            (Some(h), false, false) => self.cur.picked_edges = vec![h],
            (None, false, false) => self.cur.picked_edges.clear(),
            _ => {}
        }
    }

    /// 辺 `seed` から接してつながる辺の輪(ダブルクリック)。1 回目のクリックで `seed` はもう
    /// 選び直されている。修飾なしは輪だけを選び、Shift は輪を足す。Ctrl は 1 回目で `seed` を
    /// 足したなら輪を足し、外したなら輪を外す。
    pub(crate) fn pick_edge_chain(&mut self, seed: kmb_core::EdgePick, add: bool, toggle: bool) {
        let chain = self.edge_chain(&seed);
        if !add && !toggle {
            self.cur.picked = None;
            self.cur.also_picked.clear();
            self.cur.picked_vertices.clear();
            self.cur.picked_edges = chain;
            return;
        }
        if toggle && !self.cur.picked_edges.contains(&seed) {
            self.cur.picked_edges.retain(|e| !chain.contains(e));
            return;
        }
        for e in chain {
            if !self.cur.picked_edges.contains(&e) {
                self.cur.picked_edges.push(e);
            }
        }
    }

    /// 辺 `seed` から接してつながる辺(`seed` を先頭に。形の似た辺は足さない)。
    /// 選んだ辺として描く辺。辺の手順がつながる辺も入れるなら、それも入れる(Enter で入る物を
    /// クリックの後すぐ見せる)。
    pub(crate) fn shown_edges(&mut self) -> Vec<kmb_core::EdgePick> {
        let chained = self.cur.gesture.as_ref().is_some_and(|g| {
            g.current()
                .is_some_and(|h| h.chain && matches!(h.gesture, kmb_core::Gesture::Edges))
        });
        let mut all = self.cur.picked_edges.clone();
        if chained {
            for e in self.cur.picked_edges.clone() {
                for c in self.edge_chain(&e) {
                    if !all.contains(&c) {
                        all.push(c);
                    }
                }
            }
        }
        all
    }

    pub(crate) fn edge_chain(&mut self, seed: &kmb_core::EdgePick) -> Vec<kmb_core::EdgePick> {
        self.scene_edges();
        let Some((_, list)) = self
            .cur
            .edge_cache
            .1
            .iter()
            .find(|(f, _)| *f == seed.feature)
        else {
            return vec![seed.clone()];
        };
        let seeds: Vec<usize> = (0..list.len())
            .filter(|&i| list[i].faces == seed.faces)
            .collect();
        let mut out = vec![seed.clone()];
        for i in self.win.extensions.edge_chain(list, &seeds) {
            let e = kmb_core::EdgePick {
                feature: seed.feature,
                faces: list[i].faces.clone(),
            };
            if !out.contains(&e) {
                out.push(e);
            }
        }
        out
    }

    /// 全画面(ブラウザ・タイムライン・編集パネルを隠す。もう一度で戻す)。
    pub(super) fn toggle_focus_mode(&mut self) {
        match self.win.focus_mode.take() {
            Some((b, t, p)) => {
                // 入る前の並びへ戻す(隠した部品を元の場所へ)
                self.remember_dock_for_focus(false);
                self.win.layout.browser_open = b;
                self.win.layout.timeline_open = t;
                self.win.layout.panel_always = p;
            }
            None => {
                self.remember_dock_for_focus(true);
                self.win.focus_mode = Some((
                    self.win.layout.browser_open,
                    self.win.layout.timeline_open,
                    self.win.layout.panel_always,
                ));
                self.win.layout.browser_open = false;
                self.win.layout.timeline_open = false;
                self.win.layout.panel_always = false;
            }
        }
    }

    /// 3D のビューで色を変える面(選んだ面と、ボディを選ぶ時は選んだ機能のボディの全部の面)。
    pub(super) fn highlighted_faces(&self) -> Vec<Pick> {
        if self.active_pick_field().is_some() {
            return self.pick_field_faces();
        }
        let mut v = self.picked_faces();
        if self.win.pick_filter.body {
            for id in self.selection() {
                if let Some(b) = self.cur.scene.bodies.iter().find(|b| b.feature == id) {
                    for f in &b.mesh.face_names {
                        v.push(Pick {
                            feature: id,
                            face: f.clone(),
                        });
                    }
                }
            }
        }
        v
    }
}
