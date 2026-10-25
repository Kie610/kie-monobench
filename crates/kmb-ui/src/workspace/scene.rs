//! 3D のビューに出す物: 体の表示と非表示、スケッチ・光源の線、絵、辺、面の名前、描いた画像。

use crate::*;

impl Workspace {
    pub fn visible_bodies(&self) -> Vec<(FeatureId, kmb_core::ShapeHandle)> {
        kmb_core::visible_bodies(
            self.cur.work.doc().data(),
            &self.win.registry,
            self.cur.work.report(),
        )
    }

    pub fn picked(&self) -> Option<&view3d::Pick> {
        self.cur.picked.as_ref()
    }

    /// 機能 `id` を 3D のビューで隠しているか(行の目のほか、ボディやまとまりの目で隠した物も)。
    pub fn is_hidden(&self, id: FeatureId) -> bool {
        self.cur.hidden_now.contains(&id)
    }

    /// 機能 `id` の行の目を閉じているか(まとまりの目で隠れていても、行の目は別に覚える)。
    pub(crate) fn is_own_hidden(&self, id: FeatureId) -> bool {
        self.cur.hidden.contains(&id)
    }

    /// 表示しているボディの(最後の機能、最初に作った機能)。
    pub fn body_roots(&self) -> Vec<(FeatureId, FeatureId)> {
        let data = self.cur.work.doc().data();
        let list: Vec<kmb_core::FeatureEntry> = self
            .visible_bodies()
            .iter()
            .filter_map(|(id, _)| data.feature(*id).cloned())
            .collect();
        self.body_numbers(&list)
            .into_iter()
            .map(|(_, root, f)| (f.id, root))
            .collect()
    }

    /// ボディ(最初に作った機能 `root`、今の最後の機能 `last`)の目を閉じているか。
    pub(crate) fn is_body_hidden(&self, root: FeatureId, last: FeatureId) -> bool {
        self.cur.hidden_bodies.contains(&root) || self.cur.hidden.contains(&last)
    }

    /// ボディを 3D のビューで隠す・出す。最初に作った機能で覚えるので、隠したボディに後から
    /// 機能を足しても(最後の機能が替わっても)隠したまま。
    pub fn set_body_hidden(&mut self, root: FeatureId, hide: bool) {
        if hide {
            self.cur.hidden_bodies.insert(root);
        } else {
            self.cur.hidden_bodies.remove(&root);
            // 前の作りで最後の機能に付けた目も外す
            let lasts: Vec<FeatureId> = self
                .body_roots()
                .into_iter()
                .filter(|(_, r)| *r == root)
                .map(|(l, _)| l)
                .collect();
            for l in lasts {
                self.cur.hidden.remove(&l);
            }
        }
        self.refresh_hidden();
    }

    /// ブラウザのまとまり(スケッチ・ボディ)を、まとめて隠しているか。
    pub(crate) fn is_folder_hidden(&self, kind: browser_ui::Kind) -> bool {
        self.cur.hidden_folders.contains(&kind)
    }

    /// ブラウザのまとまり(スケッチ・ボディ)をまとめて隠す・出す。中の行の目は変えない。
    pub fn set_folder_hidden(&mut self, kind: browser_ui::Kind, hide: bool) {
        if hide {
            self.cur.hidden_folders.insert(kind);
        } else {
            self.cur.hidden_folders.remove(&kind);
        }
        self.refresh_hidden();
    }

    /// 原点の平面をいつも 3D のビューに出しているか(ブラウザの「原点」の目)。
    pub fn is_origin_shown(&self) -> bool {
        self.cur.origin_shown
    }

    pub fn set_origin_shown(&mut self, shown: bool) {
        self.cur.origin_shown = shown;
    }

    /// 機能 `id` を 3D のビューで隠す・出す(見えなくなるだけで、計算と加工には使う)。
    /// 隠したボディの面と辺は選びから外す。
    pub fn set_hidden(&mut self, id: FeatureId, hide: bool) {
        self.cur.auto_hidden.remove(&id);
        let mut set = self.cur.hidden.clone();
        if hide {
            set.insert(id);
        } else {
            set.remove(&id);
        }
        self.set_all_hidden(set);
    }

    /// 後の機能が使い始めたスケッチを 3D のビューで隠し、使われなくなった(取り消した)ものは
    /// 自動で隠した時だけ出し直す。利用者が目で替えたものはそのままにする。
    pub(crate) fn hide_used_sketches(&mut self) {
        let data = self.cur.work.doc().data();
        let sketch = |id: FeatureId| {
            data.feature(id).is_some_and(|f| {
                self.win
                    .registry
                    .descriptor(&f.type_id)
                    .is_some_and(|d| d.category == kmb_core::category::SKETCH)
            })
        };
        let mut used = HashSet::new();
        for f in data.features.iter().filter(|f| !f.suppressed) {
            let Some(Ok(feature)) = self.win.registry.instantiate(&f.type_id, &f.data) else {
                continue;
            };
            for i in feature.inputs().unwrap_or_default() {
                if let kmb_core::InputRef::Output { feature, .. } = i
                    && feature != f.id
                    && sketch(feature)
                {
                    used.insert(feature);
                }
            }
        }
        let mut hidden = self.cur.hidden.clone();
        for id in used.difference(&self.cur.used_sketches) {
            if hidden.insert(*id) {
                self.cur.auto_hidden.insert(*id);
            }
        }
        for id in self.cur.used_sketches.difference(&used) {
            if self.cur.auto_hidden.remove(id) {
                hidden.remove(id);
            }
        }
        self.cur.used_sketches = used;
        if hidden != self.cur.hidden {
            self.set_all_hidden(hidden);
        }
    }

    /// 3D のビューに出すスケッチの線(隠していない、止めていないスケッチの線)。
    pub(crate) fn sketch_lines(&self) -> Vec<(Vec<[f64; 3]>, kmb_core::LineKind)> {
        let mut out = Vec::new();
        for f in &self.cur.work.doc().data().features {
            let is_sketch = self
                .win
                .registry
                .descriptor(&f.type_id)
                .is_some_and(|d| d.category == kmb_core::category::SKETCH);
            if !is_sketch || f.suppressed || self.is_hidden(f.id) {
                continue;
            }
            // 3D スケッチは世界座標の折れ線(補助線は 3D のビューに出さない。2D と同じ)
            if let Some(w) = self
                .output(f.id, "lines")
                .and_then(<kmb_core::Wires as kmb_core::Payload>::from_value)
            {
                out.extend(
                    w.0.into_iter()
                        .filter(|w| !w.construction)
                        .map(|w| (w.points, SKETCH_LINE)),
                );
                continue;
            }
            let Some(c) = self
                .output(f.id, "curves")
                .and_then(kmb_core::Curves::from_value)
            else {
                continue;
            };
            for seg in &c.segments {
                let mut pts: Vec<[f64; 3]> = seg
                    .polyline(seg.steps_per_turn(0.01))
                    .into_iter()
                    .map(|q| c.plane.to_world(q))
                    .collect();
                pts.push(c.plane.to_world(seg.end_point()));
                out.push((pts, SKETCH_LINE));
            }
            // 線や円に使われていない単独の点(穴の位置など)も、スケッチを閉じた後に見えるよう
            // 1 点の線として出す(3D のビューは点として描く)
            if let Some(p) = self
                .output(f.id, "points")
                .and_then(<kmb_core::Points as kmb_core::Payload>::from_value)
            {
                out.extend(
                    p.lone
                        .iter()
                        .filter_map(|id| p.points.get(id))
                        .map(|q| (vec![p.plane.to_world(*q)], SKETCH_LINE)),
                );
            }
        }
        out
    }

    /// 平面のスケッチ `id` の平面(3D スケッチ・スケッチでない機能・計算できなかった時は `None`)。
    pub(crate) fn sketch_plane(&self, id: FeatureId) -> Option<kmb_core::Plane> {
        let f = self.cur.work.doc().data().feature(id)?;
        let d = self.win.registry.descriptor(&f.type_id)?;
        if d.category != kmb_core::category::SKETCH {
            return None;
        }
        // 線の無いスケッチも点の出力に平面を持つ
        self.output(id, "curves")
            .and_then(kmb_core::Curves::from_value)
            .map(|c| c.plane)
            .or_else(|| {
                self.output(id, "points")
                    .and_then(<kmb_core::Points as kmb_core::Payload>::from_value)
                    .map(|p| p.plane)
            })
    }

    /// 隠していない、止めていない機能の出力 `slot`(タイムラインの順。光源とデカール)。
    pub(crate) fn decor_outputs(&self, slot: &str) -> Vec<&kmb_core::Value> {
        self.cur
            .work
            .doc()
            .data()
            .features
            .iter()
            .filter(|f| !f.suppressed && !self.is_hidden(f.id))
            .filter_map(|f| self.output(f.id, slot))
            .collect()
    }

    /// 3D のビューに重ねる光源の印(点光源は小さな星、スポットライトは向きと円すいの縁)。
    pub(crate) fn lamp_lines(&self) -> Vec<(Vec<[f64; 3]>, kmb_core::LineKind)> {
        use kmb_core::Payload;
        const INK: kmb_core::LineKind =
            kmb_core::LineKind::new(kmb_interchange::ink::line::SURROUND, "光源");
        let (lo, hi) = self.cur.scene.bounds();
        let span = (0..3)
            .map(|k| (hi[k] - lo[k]) as f64)
            .filter(|v| v.is_finite())
            .fold(10.0, f64::max);
        let mut out = Vec::new();
        for v in self.decor_outputs(kmb_look::decor::LAMP) {
            let Some(l) = kmb_look::decor::Lamp::from_value(v) else {
                continue;
            };
            let p = l.position;
            let r = (span * 0.025).max(l.radius);
            let at = |d: [f64; 3], k: f64| [p[0] + d[0] * k, p[1] + d[1] * k, p[2] + d[2] * k];
            // 3 つの向きの短い線と、3 つの平面の小さな輪
            for d in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
                out.push((vec![at(d, -r), at(d, r)], INK));
            }
            let ring = |a: [f64; 3], b: [f64; 3], rad: f64, c: [f64; 3]| -> Vec<[f64; 3]> {
                (0..=24)
                    .map(|i| {
                        let t = i as f64 / 24.0 * std::f64::consts::TAU;
                        let (s, co) = t.sin_cos();
                        [0, 1, 2].map(|k| c[k] + (a[k] * co + b[k] * s) * rad)
                    })
                    .collect()
            };
            out.push((ring([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], r * 0.6, p), INK));
            out.push((ring([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], r * 0.6, p), INK));
            if l.spot {
                // 照らす向きと、円すいの縁(長さは場面の大きさの 1/4)
                let d = l.direction;
                let len = span * 0.25;
                out.push((vec![p, at(d, len * 1.2)], INK));
                let helper = if d[2].abs() < 0.9 {
                    [0.0, 0.0, 1.0]
                } else {
                    [1.0, 0.0, 0.0]
                };
                let cross = |a: [f64; 3], b: [f64; 3]| {
                    [
                        a[1] * b[2] - a[2] * b[1],
                        a[2] * b[0] - a[0] * b[2],
                        a[0] * b[1] - a[1] * b[0],
                    ]
                };
                let norm = |a: [f64; 3]| {
                    let n = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt().max(1e-12);
                    a.map(|v| v / n)
                };
                let e1 = norm(cross(d, helper));
                let e2 = cross(d, e1);
                let half = (l.cone_deg / 2.0).to_radians();
                let rad = len * half.tan();
                let c = at(d, len);
                let rim = ring(e1, e2, rad, c);
                for q in [0, 6, 12, 18] {
                    out.push((vec![p, rim[q]], INK));
                }
                out.push((rim, INK));
            }
        }
        out
    }

    /// 3D のビューに重ねる下絵(隠していない、止めていない機能の絵)。
    pub(crate) fn pictures(&self, ctx: &egui::Context) -> Vec<kmb_core::OverlayPicture> {
        let mut out = Vec::new();
        for f in &self.cur.work.doc().data().features {
            if f.suppressed || self.is_hidden(f.id) {
                continue;
            }
            let Some(v) = self.output(f.id, kmb_core::PICTURE) else {
                continue;
            };
            if let Some(p) = picture_ui::overlay(ctx, v, None) {
                out.push(p);
            }
        }
        out
    }

    /// 隠す機能(行の目)をまとめて替える。
    pub(crate) fn set_all_hidden(&mut self, set: HashSet<FeatureId>) {
        self.cur.hidden = set;
        self.refresh_hidden();
    }

    /// 行の目・ボディの目・まとまりの目から、3D のビューで実際に隠す物を決め直す。
    /// ボディの最後の機能は機能を足すと替わるので、計算し直した後にも呼ぶ。
    pub(crate) fn refresh_hidden(&mut self) {
        // ボディの最後の機能が替わると、色を付けるボディの ID も替わる
        self.refresh_body_colors();
        let mut now = self.cur.hidden.clone();
        let bodies = self.body_roots();
        // 選択表示していたボディが無くなった(消した・取り消した)ら外し、全部無くなればやめる
        self.cur
            .isolated
            .retain(|iso| bodies.iter().any(|(_, r)| r == iso));
        let all_bodies = self.cur.hidden_folders.contains(&browser_ui::Kind::Body);
        let isolated = &self.cur.isolated;
        now.extend(
            bodies
                .iter()
                .filter(|(_, root)| {
                    all_bodies
                        || self.cur.hidden_bodies.contains(root)
                        || (!isolated.is_empty() && !isolated.contains(root))
                })
                .map(|(last, _)| *last),
        );
        if self.cur.hidden_folders.contains(&browser_ui::Kind::Sketch) {
            let data = self.cur.work.doc().data();
            now.extend(data.features.iter().map(|f| f.id).filter(|id| {
                data.feature(*id).is_some_and(|f| {
                    self.win
                        .registry
                        .descriptor(&f.type_id)
                        .is_some_and(|d| d.category == kmb_core::category::SKETCH)
                })
            }));
        }
        self.cur.hidden_now = now;
        self.cur.scene.set_hidden(&self.cur.hidden_now);
        let hidden = &self.cur.hidden_now;
        if self
            .cur
            .picked
            .as_ref()
            .is_some_and(|p| hidden.contains(&p.feature))
        {
            self.cur.picked = None;
        }
        self.cur
            .also_picked
            .retain(|p| !hidden.contains(&p.feature));
        self.cur
            .picked_edges
            .retain(|e| !hidden.contains(&e.feature));
        self.cur
            .picked_vertices
            .retain(|v| !hidden.contains(&v.feature));
        if self.cur.picked.is_none()
            && let Some(p) =
                (!self.cur.also_picked.is_empty()).then(|| self.cur.also_picked.remove(0))
        {
            self.cur.picked = Some(p);
        }
    }

    pub fn set_picked(&mut self, pick: Option<view3d::Pick>) {
        self.cur.picked = pick;
    }

    /// 3D のビューで選んでいる辺。
    pub fn picked_edges(&self) -> &[kmb_core::EdgePick] {
        &self.cur.picked_edges
    }

    pub fn set_picked_edges(&mut self, edges: Vec<kmb_core::EdgePick>) {
        self.cur.picked_edges = edges;
    }

    /// 表示しているボディの辺(まっすぐな辺と円の辺。表示のボディが変わった時だけカーネルに問い合わせる)。
    pub(crate) fn scene_edges(&mut self) -> &[(FeatureId, Vec<kmb_core::EdgeInfo>)] {
        if self.cur.edge_cache.0 != self.cur.scene.key() {
            let key = self.cur.scene.key().to_vec();
            let edges = key
                .iter()
                .map(|(f, h)| {
                    let mut edges = self.cur.work.kernel().edges(*h).unwrap_or_default();
                    edges.extend(self.cur.work.kernel().arc_edges(*h).unwrap_or_default());
                    // 曲がった辺(丸めた面の縁など)も選べる。フィレット・面取りのカーネルの丸めで使う
                    edges.extend(self.cur.work.kernel().curve_edges(*h).unwrap_or_default());
                    (*f, edges)
                })
                .collect::<BodyEdges>();
            self.cur.vertex_cache = edges
                .iter()
                .map(|(f, l)| (*f, kmb_core::vertices_of_edges(l)))
                .collect();
            self.cur.edge_cache = (key, edges);
        }
        &self.cur.edge_cache.1
    }

    pub(crate) fn output(&self, id: FeatureId, slot: &str) -> Option<&Value> {
        match self.cur.work.report().feature(id) {
            Some(FeatureStatus::Ok { outputs, .. }) => outputs.get(slot),
            _ => None,
        }
    }

    /// 3D のビューと同じ絵を、全体表示の視点で描く(画面を開かずに確かめる時に使う)。
    pub fn render_3d(&self, size: [usize; 2]) -> egui::ColorImage {
        let rect =
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size[0] as f32, size[1] as f32));
        let mut cam = self.cur.camera;
        cam.perspective = self.win.layout.projection.perspective_for(&cam);
        cam.fit(&self.cur.scene, rect);
        let colors = view3d::Colors::from_look(&theme::Look::default());
        let mut img = view3d::render(
            &self.cur.scene,
            &cam,
            size,
            1.0,
            &self.picked_faces(),
            None,
            &colors,
            self.win.layout.shading,
        );
        // 選んでいる機能の経路と線を重ねる(3D のビューと同じ色)
        for (pts, kind) in self.overlay().lines {
            let color = match kind.ink {
                kmb_interchange::ink::line::TRAVEL | kmb_interchange::ink::line::TRAVEL_LOUD => {
                    colors.rapid
                }
                kmb_interchange::ink::line::FAINT => colors.guide,
                kmb_interchange::ink::line::APPROACH => colors.entry,
                kmb_interchange::ink::line::EASE => colors.lead,
                kmb_interchange::ink::line::CONNECT => colors.link,
                _ => colors.cut,
            };
            for w in pts.windows(2) {
                let a = cam.project(w[0].map(|v| v as f32), rect).0;
                let b = cam.project(w[1].map(|v| v as f32), rect).0;
                view3d::draw_line(&mut img, a, b, color);
            }
        }
        img
    }

    /// 面の名前(`f8:end` のような内部の名前)を、利用者に見せる名前にする。
    /// 例: 「上の面(押し出し1)」。向きは表示している形の法線、括弧の中は面を作った機能の名前。
    pub fn face_label(&self, face: &str) -> String {
        let side = match view3d::face_normal(&self.cur.scene, face) {
            Some(n) if n[2] > 0.95 => "上の面",
            Some(n) if n[2] < -0.95 => "下の面",
            Some(_) => "横の面",
            None => "面",
        };
        let owner = face
            .strip_prefix('f')
            .and_then(|r| r.split(':').next())
            .and_then(|id| id.parse::<u64>().ok())
            .and_then(|id| self.cur.work.doc().data().feature(FeatureId(id)))
            // 作った機能が名前から分からない面(読み込んだ STEP の面)は、その面を持つボディの名前
            .or_else(|| {
                let b = self
                    .cur
                    .scene
                    .bodies
                    .iter()
                    .find(|b| b.mesh.face_names.iter().any(|n| n == face))?;
                self.cur.work.doc().data().feature(b.feature)
            })
            .map(|f| f.name.clone());
        match owner {
            Some(name) => format!("{side}({name})"),
            None => format!("{side}({face})"),
        }
    }

    /// 選んだ面が平らなら真。
    pub fn picked_face_is_planar(&self) -> bool {
        self.context().picked_plane().is_some()
    }

    /// 面 `pick` が平らか(直前に聞いた面なら覚えた答え)。
    pub(crate) fn is_planar(&mut self, pick: &view3d::Pick) -> bool {
        if let Some((p, yes)) = &self.cur.planar_hover
            && p == pick
        {
            return *yes;
        }
        let cx = kmb_core::Context {
            picked: Some(pick),
            ..self.context()
        };
        let yes = cx.picked_plane().is_some();
        self.cur.planar_hover = Some((pick.clone(), yes));
        yes
    }
}
