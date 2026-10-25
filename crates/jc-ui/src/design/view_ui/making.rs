//! 「クリックで作る」流れ: 手順ごとのクリック・描きかけの見本・Enter と Esc・OK を押せない理由。

use super::*;

impl Workspace {
    pub(crate) fn gesture_events(&mut self, ev: &view3d::ViewEvents, rect: Rect) {
        // 辺を選ぶ手順: クリックした辺(面も受ける手順は面も)を足す・外す。足したら次の手順へ進む
        // (Ctrl を押していれば進まずに続けて足す。Enter で次へ)
        let edges_step = self.cur.gesture.as_ref().and_then(|g| {
            g.current()
                .filter(|h| matches!(h.gesture, jc_core::Gesture::Edges))
                .map(|h| h.faces)
        });
        if let Some(faces) = edges_step {
            let mut added = false;
            // ダブルクリックの 2 回目は捨てる(つながる辺は、作るフィレット・面取りが入れる)
            if let Some(p) = ev.click.filter(|_| ev.double_click.is_none()) {
                let hit = if faces {
                    self.hit_at(
                        rect,
                        p,
                        PickFilter {
                            vertex: false,
                            ..PickFilter::default()
                        },
                    )
                } else {
                    self.edge_hit(rect, p).map(Hit::Edge)
                };
                match hit {
                    Some(Hit::Edge(h)) if self.cur.picked_edges.contains(&h) => {
                        self.cur.picked_edges.retain(|e| *e != h)
                    }
                    Some(Hit::Edge(h)) => {
                        self.cur.picked_edges.push(h);
                        added = true;
                    }
                    Some(Hit::Face(f)) => {
                        added = !self.picked_faces().contains(&f);
                        self.pick_with(Some(f), false, true);
                    }
                    _ => {}
                }
            }
            if let Some(g) = &mut self.cur.gesture {
                g.live = (!self.cur.picked_edges.is_empty())
                    .then(|| serde_json::to_value(&self.cur.picked_edges).unwrap_or_default());
            }
            if added && !ev.add && self.click_advances() {
                self.gesture_enter();
            }
            return;
        }
        // 面を選ぶ手順: クリックした平らな面を選んだ面にして、次の手順へ(コマンドが先、面が後)
        // (曲面も選べる手順は、どの面でもよい。ねじを付ける円筒の面など)
        let face_step = self.cur.gesture.as_ref().and_then(|g| {
            g.current().and_then(|h| match h.gesture {
                jc_core::Gesture::Face => Some((false, h.vertices)),
                jc_core::Gesture::AnyFace => Some((true, h.vertices)),
                _ => None,
            })
        });
        if let Some((any, vertices)) = face_step {
            // 頂点も受ける手順(「対象まで」): 頂点の近くを押したら頂点を選ぶ
            if vertices
                && let Some(p) = ev.click
                && let Some(v) = self.vertex_hit(rect, p)
            {
                self.cur.picked = None;
                self.cur.picked_vertices = vec![v.clone()];
                self.gesture_commit_step(
                    serde_json::json!({"vertex": v.vertex, "feature": v.feature.0}),
                );
                return;
            }
            if let Some(p) = ev.click {
                let mut hit = view3d::pick_at(&self.cur.scene, &self.cur.camera, rect, p);
                // 直している機能の計算が失敗している間は前の形を出していて、押した面にはその機能の
                // 番号が付くが、その機能に形は無い。面を持つ、計算できた前の機能の面として読む
                if let Some(h) = &mut hit
                    && !matches!(
                        self.output(h.feature, "body"),
                        Some(jc_core::Value::Shape(_))
                    )
                    && let Some(owner) = self.face_owner(&h.face)
                {
                    h.feature = owner;
                }
                let before = (
                    self.cur.picked.take(),
                    std::mem::take(&mut self.cur.also_picked),
                );
                self.cur.picked = hit.clone();
                match hit {
                    Some(h) if any || self.context().picked_plane().is_some() => {
                        self.gesture_commit_step(serde_json::Value::String(h.face));
                    }
                    _ => {
                        (self.cur.picked, self.cur.also_picked) = before;
                        self.win.message = Some(match (any, vertices) {
                            (true, _) => "面をクリックしてください(何もない所は選べません)".into(),
                            (false, true) => {
                                "平らな面か頂点をクリックしてください(曲面や何もない所は選べません)"
                                    .into()
                            }
                            (false, false) => {
                                "平らな面をクリックしてください(曲面や何もない所は選べません)"
                                    .into()
                            }
                        });
                    }
                }
            }
            return;
        }
        // 候補を選ぶ手順で、ボディの平らな面も受け付けるもの(スケッチを描く平面): 平らな面をクリック
        // したらその面を選んだ面にして決める。面がなければ候補(原点の平面など)から選ぶ
        let face_choice = self.cur.gesture.as_ref().is_some_and(|g| {
            g.current()
                .is_some_and(|h| h.faces && matches!(h.gesture, jc_core::Gesture::Choice))
        });
        if face_choice
            && let Some(p) = ev.click
            && let Some(hit) = view3d::pick_at(&self.cur.scene, &self.cur.camera, rect, p)
        {
            let before = (
                self.cur.picked.take(),
                std::mem::take(&mut self.cur.also_picked),
            );
            self.cur.picked = Some(hit);
            if self.context().picked_plane().is_some() {
                self.gesture_commit_step(serde_json::Value::String("face".into()));
                return;
            }
            (self.cur.picked, self.cur.also_picked) = before;
            // 曲面: 後ろに候補(原点の平面)があればそれを選ぶ。なければ理由を出す
            let behind = self
                .cur
                .gesture
                .as_ref()
                .and_then(|g| g.current())
                .and_then(|h| gesture::choice_at(h, &self.cur.camera, rect, p));
            if behind.is_none() {
                self.win.message =
                    Some("平らな面をクリックしてください(曲面には描けません)".into());
                return;
            }
        }
        // 候補を足す・外す手順(押し出す領域): クリックした候補を足す・外す(Enter で次へ)
        if let Some(g) = &mut self.cur.gesture
            && let Some(h) = g.handles.get(g.step)
            && matches!(h.gesture, jc_core::Gesture::Regions)
        {
            let mut advance = false;
            let mut last = false;
            let mut restart_on_face = None;
            if let Some(p) = ev.click {
                match gesture::choice_at(h, &self.cur.camera, rect, p) {
                    Some(k) => {
                        let before = g.picks.len();
                        gesture::toggle_pick(h, &mut g.picks, k);
                        // 足したら次の手順へ進む(Ctrl を押していれば進まずに続けて足す。
                        // 次の欄へ送り、複数は Ctrl で選ぶ)
                        advance = g.picks.len() > before && !ev.add;
                        last = g.step + 1 >= g.handles.len();
                    }
                    None if h.faces => restart_on_face = Some(p),
                    None => {
                        self.win.message = Some("領域の中をクリックしてください".into());
                    }
                }
            }
            if let Some(p) = restart_on_face {
                // 領域の外: ボディの平らな面なら、その面を選んで手順を始めからやり直す
                // (押し出しは面から高さを決める手順になる)。Ctrl なら面を足して待つ
                match view3d::pick_at(&self.cur.scene, &self.cur.camera, rect, p) {
                    Some(hit) if self.is_planar(&hit) => {
                        self.pick_with(Some(hit), ev.add, false);
                        if !ev.add
                            && let Some(action) = self.cur.gesture.take().map(|g| g.action)
                        {
                            self.begin_pick(&action);
                        }
                    }
                    _ => {
                        self.win.message =
                            Some("領域の中か、ボディの平らな面をクリックしてください".into());
                    }
                }
                return;
            }
            if advance && (!last || self.click_advances()) {
                self.gesture_enter();
            }
            return;
        }
        // 面も選べる手順: マウスの下が平らな面なら、後ろの候補ではなくその面を示す
        let over_face = face_choice
            && ev
                .hover
                .and_then(|p| view3d::pick_at(&self.cur.scene, &self.cur.camera, rect, p))
                .is_some_and(|hit| self.is_planar(&hit));
        let Some(g) = &mut self.cur.gesture else {
            return;
        };
        if let (Some(h), Some(p)) = (g.handles.get(g.step), ev.hover) {
            g.live = if over_face {
                Some(serde_json::Value::String("face".into()))
            } else {
                gesture::value_at(h, &self.cur.camera, rect, p)
            };
        }
        // 面の上の点を決める手順: 選んだ面の外(何もない所・別の面)のクリックは受け付けない。
        // 平面に写すと面の外の点にも穴ができ、形が変わらないまま確定してしまうため
        // (吸い付く点は面の縁にあるので、吸い付いていれば受け付ける)
        if let (Some(p), Some(h), Some(face)) = (ev.click, g.handles.get(g.step), &self.cur.picked)
            && matches!(h.gesture, jc_core::Gesture::Point { .. })
            && g.typed.is_empty()
        {
            let snapped = g.live.as_ref().is_some_and(|v| {
                h.snap_to
                    .iter()
                    .any(|q| (0..3).all(|i| v[i].as_f64() == Some(q[i])))
            });
            let on_face = view3d::pick_at(&self.cur.scene, &self.cur.camera, rect, p)
                .is_some_and(|hit| hit == *face);
            if !snapped && !on_face {
                let name = self.face_label(&face.face);
                self.win.message = Some(format!("選んだ面({name})の中をクリックしてください"));
                return;
            }
        }
        let Some(g) = &mut self.cur.gesture else {
            return;
        };
        if ev.click.is_some() {
            let ready = g.ready();
            let v = match (g.handles.get(g.step), &g.typed) {
                (Some(h), t) if !t.is_empty() => gesture::typed_value(h, t).or(g.live.clone()),
                _ => g.live.clone(),
            };
            match v {
                Some(v) => self.gesture_commit_step(v),
                None if ready => {
                    self.gesture_finish();
                }
                None => {}
            }
        }
    }

    /// 押し出しを直している間、押し出す輪郭を青く塗る。
    /// 面 `face` を持つ形を出した、計算できた機能(タイムラインの後ろから探す)。
    pub(super) fn face_owner(&self, face: &str) -> Option<jc_core::FeatureId> {
        self.cur
            .work
            .doc()
            .data()
            .features
            .iter()
            .rev()
            .find_map(|f| {
                let Some(jc_core::Value::Shape(h)) = self.output(f.id, "body") else {
                    return None;
                };
                self.cur
                    .work
                    .kernel()
                    .faces(*h)
                    .is_ok_and(|fs| fs.iter().any(|x| x.name == face))
                    .then_some(f.id)
            })
    }

    /// 直している機能に、使っている原点の平面の欄([`jc_core::Field::origin_planes`])があれば
    /// `Some`(中は欄が指す原点の平面の番号。原点の平面でなければ `None`)。
    pub(crate) fn origin_plane_field(&self) -> Option<Option<usize>> {
        let e = self.cur.work.doc().data().feature(self.cur.editing?)?;
        let d = self.win.registry.descriptor(&e.type_id)?;
        let f = d
            .fields
            .iter()
            .find(|f| f.origin_planes && f.applies(&e.data))?;
        Some(f.origin_plane_of(&e.data))
    }

    pub(super) fn draw_editing_profile(&self, ui: &egui::Ui, rect: Rect) {
        // 直している途中に面を選ぶ手順(「対象まで」の面を選ぶ)の間も、輪郭と矢印は出したままにする
        let face_step = self
            .cur
            .gesture
            .as_ref()
            .and_then(|g| g.current())
            .is_some_and(|h| {
                matches!(
                    h.gesture,
                    jc_core::Gesture::Face | jc_core::Gesture::AnyFace
                )
            });
        if self.cur.gesture.is_some() && !face_step {
            return;
        }
        let Some(t) = self
            .cur
            .editing
            .and_then(|id| self.output(id, jc_core::TOOL_PREVIEW))
            .and_then(<jc_core::ToolPreview as jc_core::Payload>::from_value)
        else {
            return;
        };
        if t.profile.is_empty() {
            return;
        }
        let screen: Vec<Vec<Pos2>> = t
            .profile
            .iter()
            .map(|l| {
                l.iter()
                    .map(|p| self.cur.camera.project(p.map(|v| v as f32), rect).0)
                    .collect()
            })
            .collect();
        let painter = ui.painter_at(rect);
        gesture::fill(&painter, rect, &screen, profile_fill(&self.win.look));
        for l in screen {
            painter.add(egui::Shape::line(
                l,
                egui::Stroke::new(1.5, self.win.look.pal.inks.profile),
            ));
        }
        for [a, b] in t.arrows {
            self.draw_extrude_arrow(&painter, rect, a, b);
        }
    }

    /// 押し出す向きの矢印: 輪郭の重心の点から端までの線と、端の矢じり(
    /// 両側・対称の押し出しは 1 本ずつ描くので、それぞれの向きと長さが分かる)。
    pub(super) fn draw_extrude_arrow(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        a: [f64; 3],
        b: [f64; 3],
    ) {
        let ink = self.win.look.pal.ink;
        let rim = self.win.look.pal.canvas;
        let p = self.cur.camera.project(a.map(|v| v as f32), rect).0;
        let q = self.cur.camera.project(b.map(|v| v as f32), rect).0;
        painter.circle_filled(p, 3.0, self.win.look.pal.inks.profile);
        let d = q - p;
        let len = d.length();
        if len < 1.0 {
            // 真上から見ていて線が点になる時は、端に丸だけ
            painter.circle(q, 5.0, ink, egui::Stroke::new(1.0, rim));
            return;
        }
        let u = d / len;
        let head = 14.0_f32.min(len * 0.6).max(8.0);
        let base = q - u * head;
        let side = egui::vec2(-u.y, u.x) * head * 0.5;
        painter.line_segment([p, base], egui::Stroke::new(1.5, ink));
        painter.add(egui::Shape::convex_polygon(
            vec![q, base + side, base - side],
            ink,
            egui::Stroke::new(1.0, rim),
        ));
    }

    /// 決めた値と、マウスの位置での値の形を重ねて描く。
    pub(super) fn draw_gesture(&self, ui: &egui::Ui, rect: Rect) {
        let Some(g) = &self.cur.gesture else { return };
        let painter = ui.painter_at(rect);
        let color = self.win.look.ws();
        let profile_fill = profile_fill(&self.win.look);
        let mut lines = Vec::new();
        // 押し出しの高さを引いている間・回転の角度を決めている間も、輪郭を青く塗っておく
        if let Some(h) = g.current()
            && (h.ops.is_some() && matches!(h.gesture, jc_core::Gesture::Distance { .. })
                || matches!(h.gesture, jc_core::Gesture::Angle { .. }))
            && !h.outline.is_empty()
        {
            let screen: Vec<Vec<Pos2>> = h
                .outline
                .iter()
                .map(|l| {
                    l.iter()
                        .map(|p| self.cur.camera.project(p.map(|v| v as f32), rect).0)
                        .collect()
                })
                .collect();
            gesture::fill(&painter, rect, &screen, profile_fill);
            for l in screen {
                painter.add(egui::Shape::line(
                    l,
                    egui::Stroke::new(1.5, self.win.look.pal.inks.profile),
                ));
            }
        }
        // 候補を足す・外す手順: 選んだ候補を塗って太く、マウスの下の候補を薄く塗る
        if let Some(h) = g.current()
            && matches!(h.gesture, jc_core::Gesture::Regions)
        {
            let soft = crate::theme::mix(self.win.look.pal.panel, color, 0.55);
            let hover = ui
                .input(|i| i.pointer.hover_pos())
                .filter(|p| rect.contains(*p))
                .and_then(|p| gesture::choice_at(h, &self.cur.camera, rect, p));
            for (k, c) in h.choices.iter().enumerate() {
                let screen: Vec<Vec<Pos2>> = c
                    .outline
                    .iter()
                    .map(|l| {
                        l.iter()
                            .map(|p| self.cur.camera.project(p.map(|v| v as f32), rect).0)
                            .collect()
                    })
                    .collect();
                // 選んだ輪郭は青で塗る
                let picked = g.picks.contains(&c.value);
                if picked {
                    gesture::fill(&painter, rect, &screen, profile_fill);
                } else if hover == Some(k) {
                    gesture::fill(&painter, rect, &screen, color.gamma_multiply(0.15));
                }
                let stroke = if picked {
                    egui::Stroke::new(2.0, self.win.look.pal.inks.profile)
                } else {
                    egui::Stroke::new(1.2, soft)
                };
                for l in screen {
                    painter.add(egui::Shape::line(l, stroke));
                }
            }
            if let Some(p) = ui.input(|i| i.pointer.hover_pos()) {
                let text = match hover {
                    Some(k) => h.choices[k].label.clone(),
                    None => format!("{} 個を選んでいます", g.picks.len()),
                };
                painter.text(
                    p + egui::vec2(14.0, -14.0),
                    egui::Align2::LEFT_BOTTOM,
                    text,
                    crate::theme::prop(13.0),
                    color,
                );
            }
        }
        // 候補を選ぶ手順: 候補の線を細く描く(マウスの下の候補は下で太く描く)。
        // 閉じた候補(スケッチを描く原点の平面)は、灰色の半透明の板にして、マウスの下の板を
        // 選ぶ色で塗る(面を選んでいると分かるように)。
        // 色のある候補(原点の軸)は、その色の太い線にする
        if let Some(h) = g.current()
            && matches!(h.gesture, jc_core::Gesture::Choice)
        {
            let soft = crate::theme::mix(self.win.look.pal.panel, color, 0.55);
            let pal = &self.win.look.pal;
            let gray = crate::theme::mix(pal.ink3, pal.ink2, 0.3);
            // 軸を選ぶ間は、原点の平面を目安に出す(選べない)
            if h.show_origin {
                view3d::origin_planes(&painter, &self.cur.camera, rect, gray);
            }
            for c in &h.choices {
                // マウスが候補に乗っていなければ、手順が始めから選んだ候補(回転の自動の軸)を明るく
                let hot = match &g.live {
                    Some(v) => *v == c.value,
                    None => g.picks.contains(&c.value),
                };
                for l in &gesture::outline(c, &self.cur.camera, rect) {
                    let pts: Vec<Pos2> = l
                        .iter()
                        .map(|p| self.cur.camera.project(p.map(|v| v as f32), rect).0)
                        .collect();
                    let closed = pts.len() > 3 && pts[0].distance(pts[pts.len() - 1]) < 0.5;
                    if closed {
                        let (fill, edge) = if hot {
                            (color.gamma_multiply(0.38), egui::Stroke::new(2.0, color))
                        } else {
                            (gray.gamma_multiply(0.22), egui::Stroke::new(1.0, gray))
                        };
                        gesture::fill(&painter, rect, std::slice::from_ref(&pts), fill);
                        painter.add(egui::Shape::line(pts.clone(), edge));
                        // 板の名前(XY 平面など)を板の真ん中に
                        let at = pts[0] + (pts[2] - pts[0]) * 0.5;
                        painter.text(
                            at,
                            egui::Align2::CENTER_CENTER,
                            &c.label,
                            crate::theme::prop(11.5),
                            if hot { color } else { pal.ink2 },
                        );
                    } else if let Some([r, g, b]) = c.color {
                        painter.add(egui::Shape::line(
                            pts,
                            egui::Stroke::new(3.0, egui::Color32::from_rgb(r, g, b)),
                        ));
                    } else {
                        painter.add(egui::Shape::line(pts, egui::Stroke::new(1.2, soft)));
                    }
                }
            }
        }
        // 点を決める手順: 目安の線を描き(入る位置なら、ポケットのうちらせんが入る所を薄く塗る)、
        // 吸い付く点を小さな丸で見せる
        if let Some(h) = g.current()
            && matches!(h.gesture, jc_core::Gesture::Point { .. })
        {
            if !h.outline.is_empty() {
                let screen: Vec<Vec<Pos2>> = h
                    .outline
                    .iter()
                    .map(|l| {
                        l.iter()
                            .map(|p| self.cur.camera.project(p.map(|v| v as f32), rect).0)
                            .collect()
                    })
                    .collect();
                // 置ける所は「良い」の色(緑)で、面を選ぶ色と分ける
                let ok = self.win.look.pal.ok;
                let edge = if h.fill { ok } else { color };
                if h.fill {
                    gesture::fill(&painter, rect, &screen, ok.gamma_multiply(0.3));
                }
                for l in screen {
                    painter.add(egui::Shape::line(l, egui::Stroke::new(1.5, edge)));
                }
            }
            for q in &h.snap_to {
                let p = self.cur.camera.project(q.map(|v| v as f32), rect).0;
                painter.circle_stroke(p, 3.0, egui::Stroke::new(1.2, color));
            }
        }
        for h in &g.handles[..g.step.min(g.handles.len())] {
            if let Some(v) = g.args.get(&h.key) {
                // 選び終えた輪郭は青のまま(押し出す輪郭)
                let c = if matches!(h.gesture, jc_core::Gesture::Regions) {
                    self.win.look.pal.inks.profile
                } else {
                    color
                };
                lines.extend(
                    gesture::preview_in(h, v, &self.cur.camera, rect)
                        .into_iter()
                        .map(|l| (l, c)),
                );
            }
        }
        // 数字を打っていれば、その値で描く(マウスの位置の値より打った値が先)
        let typed = g.current().and_then(|h| gesture::typed_value(h, &g.typed));
        let mut now = Vec::new();
        let mut now_color = color;
        if let (Some(h), Some(v)) = (g.current(), typed.as_ref().or(g.live.as_ref())) {
            now = gesture::preview_in(h, v, &self.cur.camera, rect);
            // 押し出しの道具は色付きの立体で描く(tool_ghosts)。線は、輪郭の重心から
            // 引いた先まで 1 本と、その終わりの矢印だけ(向きと長さが分かる)
            if let (Some(_), jc_core::Gesture::Distance { origin, dir }) = (h.ops, h.gesture) {
                let s = v.as_f64().unwrap_or(0.0);
                now.clear();
                self.draw_extrude_arrow(
                    &painter,
                    rect,
                    origin,
                    [0, 1, 2].map(|i| origin[i] + dir[i] * s),
                );
            }
            // 押し出しの高さなど: 前へ引くと結合、逆へ引くと切り取り。線と値をその操作の色で描き、
            // 値の後ろに操作の名前を添える
            let op = h.op_for(v.as_f64().unwrap_or(0.0));
            if let Some(op) = op {
                now_color = self.win.look.tool_op(op);
            }
            if let Some(p) = ui.input(|i| i.pointer.hover_pos()) {
                let mut text = gesture::describe_for(h, v);
                if let Some(op) = op {
                    text = format!("{text}({})", op.label());
                }
                painter.text(
                    p + egui::vec2(14.0, -14.0),
                    egui::Align2::LEFT_BOTTOM,
                    text,
                    crate::theme::prop(13.0),
                    now_color,
                );
            }
        }
        for (l, c) in lines
            .into_iter()
            .chain(now.into_iter().map(|l| (l, now_color)))
        {
            let pts: Vec<Pos2> = l
                .iter()
                .map(|p| self.cur.camera.project(p.map(|v| v as f32), rect).0)
                .collect();
            painter.add(egui::Shape::line(pts, egui::Stroke::new(2.0, c)));
        }
    }

    /// 押し出す道具を色付きの立体で見せる(結合と新しいボディはボディの色、
    /// 切り取りは赤、交差は黄)。高さを引いている間はマウスの値で、
    /// 押し出しを直している間は評価した道具の形で描く。直している間の結合・新しいボディは、
    /// できた形がそのまま見えているので重ねない。
    pub(crate) fn tool_ghosts(&mut self, colors: &view3d::Colors) -> Vec<view3d::Ghost> {
        let to32 = |p: [f64; 3]| p.map(|v| v as f32);
        let segments = |lines: &[Vec<[f64; 3]>]| -> Vec<[[f32; 3]; 2]> {
            lines
                .iter()
                .flat_map(|l| l.windows(2).map(|w| [to32(w[0]), to32(w[1])]))
                .collect()
        };
        let look = &self.win.look;
        let see_through =
            |c: egui::Color32, a: u8| egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a);
        let ghost = |op: jc_core::ToolOp, tris: Vec<[[f32; 3]; 3]>, edges| match op {
            // 新しいボディはできあがりと同じ灰色の立体、結合は青く透かして見分ける
            jc_core::ToolOp::New => view3d::Ghost {
                tris,
                edges,
                color: colors.body,
                edge_color: colors.edge,
                layer: view3d::GhostLayer::Solid,
            },
            jc_core::ToolOp::Join => view3d::Ghost {
                tris,
                edges,
                color: see_through(look.pal.inks.ghost_join, view3d::GHOST_JOIN_ALPHA),
                edge_color: look.pal.inks.ghost_join_edge,
                layer: view3d::GhostLayer::Over,
            },
            jc_core::ToolOp::Cut => view3d::Ghost {
                tris,
                edges,
                color: see_through(look.pal.inks.ghost_cut, view3d::GHOST_CUT_ALPHA),
                edge_color: look.pal.inks.ghost_cut_edge,
                layer: view3d::GhostLayer::Over,
            },
            jc_core::ToolOp::Intersect => view3d::Ghost {
                tris,
                edges,
                color: see_through(look.pal.inks.ghost_intersect, view3d::GHOST_INTERSECT_ALPHA),
                edge_color: look.pal.inks.ghost_intersect_edge,
                layer: view3d::GhostLayer::Over,
            },
        };
        // 高さを引いている間(押し出しの距離の手順)。道具の形は手順が値 1 の柱で持つので、
        // 輪郭の平面からの距離を値の倍に伸ばす(値が負なら裏返して外向きにそろえる)
        if let Some(g) = &self.cur.gesture
            && let Some(h) = g.current()
            && let jc_core::Gesture::Distance { origin, dir } = h.gesture
            && h.ops.is_some()
        {
            let typed = gesture::typed_value(h, &g.typed);
            let Some(v) = typed.as_ref().or(g.live.as_ref()) else {
                return Vec::new();
            };
            let s = v.as_f64().unwrap_or(0.0);
            let Some(op) = h.op_for(s) else {
                return Vec::new();
            };
            let dd: f64 = dir.iter().map(|d| d * d).sum();
            let tris = match &h.shape {
                Some(shape) if (s * dd.sqrt()).abs() >= 1e-9 => {
                    let m = &shape.mesh;
                    let at = |i: u32| {
                        let p = m.positions[i as usize].map(f64::from);
                        let t = (0..3).map(|k| (p[k] - origin[k]) * dir[k]).sum::<f64>() / dd;
                        to32([0, 1, 2].map(|k| p[k] + t * (s - 1.0) * dir[k]))
                    };
                    m.triangles
                        .iter()
                        .map(|&[a, b, c]| {
                            if s < 0.0 {
                                [at(a), at(c), at(b)]
                            } else {
                                [at(a), at(b), at(c)]
                            }
                        })
                        .collect()
                }
                _ => Vec::new(),
            };
            return vec![ghost(op, tris, segments(&gesture::preview(h, v)))];
        }
        // 押し出しを直している間
        let Some(id) = self.cur.editing else {
            return Vec::new();
        };
        let Some(t) = self
            .output(id, jc_core::TOOL_PREVIEW)
            .and_then(<jc_core::ToolPreview as jc_core::Payload>::from_value)
        else {
            return Vec::new();
        };
        if matches!(t.op, jc_core::ToolOp::New | jc_core::ToolOp::Join) {
            return Vec::new();
        }
        let tris = t.faces.iter().map(|f| f.map(to32)).collect();
        vec![ghost(t.op, tris, segments(&t.lines))]
    }

    /// いくつでも選べる手順(領域・辺)で、クリックで足したらそのまま次へ進むか。後に手順があれば
    /// 進む(回転の領域 → 軸。Kie 2026-10-21)。最後の手順なら、ツールバーから始めた作る操作だけ進み
    /// (フィレットの辺 → 半径の欄)、作ったものを直す操作(領域の選び直し・削る輪郭・タブ)は
    /// クリックで足し外しして Enter で決める。
    pub(super) fn click_advances(&self) -> bool {
        let Some(g) = &self.cur.gesture else {
            return false;
        };
        if g.step + 1 < g.handles.len() {
            return true;
        }
        self.actions()
            .into_iter()
            .find(|a| a.id == g.action)
            .is_some_and(|a| a.places.contains(&jc_core::Place::Toolbar) && !a.keep_edit)
    }

    /// クリックで作る流れの Enter(右クリックの OK も): 全部決めていれば作る。選ぶ手順は選んだもので、
    /// 値の手順は打った数(なければマウスの位置の値)で次へ。省ける手順は値なしで次へ。
    pub(crate) fn gesture_enter(&mut self) {
        let Some(g) = &self.cur.gesture else { return };
        if g.ready() {
            self.gesture_finish();
            return;
        }
        let Some(h) = g.current() else { return };
        // 候補を足す・外す手順: 選んだ候補で次へ。選ばずに Enter は、省ける手順なら値なし
        if matches!(h.gesture, jc_core::Gesture::Regions) {
            if !g.picks.is_empty() {
                let v = serde_json::Value::Array(g.picks.clone());
                self.gesture_commit_step(v);
            } else if h.optional {
                self.gesture_commit_step(serde_json::Value::Null);
            } else {
                self.win.message = Some("領域をクリックで選んでから Enter".into());
            }
            return;
        }
        // 候補を 1 つ選ぶ手順で、始めから選んだ候補がある(回転の自動の軸): 打たずに Enter でそれに
        if matches!(h.gesture, jc_core::Gesture::Choice)
            && g.typed.trim().is_empty()
            && let Some(v) = g.picks.first().cloned()
        {
            self.gesture_commit_step(v);
            return;
        }
        // 辺を選ぶ手順: 選んだ辺で次へ(選んだ辺は手順の値に移し、画面の選びは空にする)
        if matches!(h.gesture, jc_core::Gesture::Edges) {
            // 面も受ける手順は、選んだ面も値に入れる(`{"kind": "face", …}`。辺は今までの形のまま)
            let faces: Vec<jc_core::Ref> = if h.faces {
                self.picked_faces()
                    .into_iter()
                    .map(jc_core::Ref::from)
                    .collect()
            } else {
                vec![]
            };
            if self.cur.picked_edges.is_empty() && faces.is_empty() {
                self.win.message = Some(if h.faces {
                    "辺か面をクリックで選んでから Enter".into()
                } else {
                    "辺をクリックで選んでから Enter".into()
                });
            } else {
                let mut v: Vec<serde_json::Value> = std::mem::take(&mut self.cur.picked_edges)
                    .into_iter()
                    .filter_map(|e| serde_json::to_value(e).ok())
                    .collect();
                v.extend(faces.iter().filter_map(|f| serde_json::to_value(f).ok()));
                if h.faces {
                    self.cur.picked = None;
                    self.cur.also_picked.clear();
                }
                self.gesture_commit_step(serde_json::Value::Array(v));
            }
            return;
        }
        // 省ける手順は、数字を打たずに Enter で値なしのまま次へ(穴の深さなら貫通)
        let v = if h.optional && g.typed.trim().is_empty() {
            Some(serde_json::Value::Null)
        } else {
            gesture::typed_value(h, &g.typed).or(g.live.clone())
        };
        if let Some(v) = v {
            // 最後の手順なら、決めたらそのまま作る。Enter で決めた時は作ったものも決める
            // (値を打って Enter = OK。もう一度 Enter や OK を求めない)
            self.gesture_commit_step(v);
            if self.cur.gesture.is_none() && self.cur.creating.is_some() {
                self.finish_edit();
            }
        }
    }

    /// 右クリックの OK が使えるか(使えなければ理由)。クリックで選ぶ手順は、選ぶまで使えない
    /// (要る物を選ぶまで OK を押せない)。
    pub(crate) fn ok_blocked(&self) -> Option<&'static str> {
        let g = self.cur.gesture.as_ref()?;
        if g.ready() {
            return None;
        }
        let h = g.current()?;
        match h.gesture {
            jc_core::Gesture::Regions if g.picks.is_empty() && !h.optional => {
                Some("領域をクリックで選んでから")
            }
            jc_core::Gesture::Regions => None,
            jc_core::Gesture::Edges
                if self.cur.picked_edges.is_empty()
                    && (!h.faces || self.picked_faces().is_empty()) =>
            {
                Some("辺をクリックで選んでから")
            }
            jc_core::Gesture::Edges => None,
            _ if h.optional => None,
            jc_core::Gesture::Choice if !g.picks.is_empty() => None,
            _ if gesture::typed_value(h, &g.typed).is_some() => None,
            jc_core::Gesture::Choice | jc_core::Gesture::Face | jc_core::Gesture::AnyFace => {
                Some("クリックで選んでから")
            }
            _ => Some("クリックか数字で値を決めてから"),
        }
    }

    /// Esc・Enter・数字(クリックで作る流れ)。
    pub(crate) fn gesture_keys(&mut self, ctx: &egui::Context) {
        if self.cur.gesture.is_none() {
            return;
        }
        let events = ctx.input(|i| i.events.clone());
        // 欄の一覧が開いている時の Esc は一覧を閉じるだけ(Z2)
        let popup = egui::Popup::is_any_open(ctx);
        for e in events {
            match e {
                egui::Event::Key {
                    key: egui::Key::Escape,
                    pressed: true,
                    ..
                } if !popup => {
                    self.cur.gesture = None;
                    self.cur.picked_edges.clear();
                    return;
                }
                egui::Event::Key {
                    key: egui::Key::Enter,
                    pressed: true,
                    ..
                } => {
                    self.gesture_enter();
                    return;
                }
                egui::Event::Key {
                    key: egui::Key::Backspace,
                    pressed: true,
                    ..
                } => {
                    if let Some(g) = &mut self.cur.gesture {
                        g.typed.pop();
                    }
                }
                egui::Event::Text(t) => {
                    // 数字を打てるのは値を決める手順(距離・径・省ける手順)だけ。選ぶ手順では捨てる
                    if let Some(g) = &mut self.cur.gesture
                        && let Some(h) = g.current().filter(|h| {
                            !matches!(h.gesture, jc_core::Gesture::Regions)
                                && gesture::typing_hint(h).is_some()
                        })
                    {
                        let add: String = t.chars().filter(|c| gesture::typable(h, *c)).collect();
                        g.typed.push_str(&add);
                    }
                }
                _ => {}
            }
        }
    }
}
