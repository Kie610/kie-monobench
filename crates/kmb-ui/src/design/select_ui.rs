//! 3D のビューで選ぶ物(頂点・辺・面・ボディ)と、編集パネルの選ぶ欄([`kmb_core::FieldKind::Pick`])。
//!
//! - 頂点・辺・面は混ぜて選べる。マウスの近くに頂点があれば頂点、無ければ辺、無ければ面を拾う
//!   (既定で全部を選べる)。ボディを選ぶ間は、ボディだけを選ぶ。
//! - 選ぶ欄は、欄を押して「受ける状態」にすると、3D のビューのクリックがその欄に入る。1 つだけ受ける
//!   欄が埋まると次の欄へ進む。受けている間は、その機能の手前の形をビューに出す(編集の
//!   間は時間を戻す。作った機能自身や、使われて消えたボディも選べるように)。
//! - マウスを乗せた時は物を明るくするだけで、結果の形はクリックで欄に入れてから出す(docs/避ける形.md)。

use crate::measure::{self, Geo, Kind};
use crate::{PickFilter, Workspace, view3d};
use egui::{Pos2, Rect};
use kmb_core::{EdgePick, FeatureId, FieldKind, Ref, RefKind, VertexPick};

/// 頂点を拾う、画面の上の距離(点)。辺([`view3d::EDGE_PX`])より少し広く、頂点を先に拾う
pub const VERTEX_PX: f32 = 9.0;

/// マウスの下(クリックした所)の物。
#[derive(Debug, Clone, PartialEq)]
pub enum Hit {
    Vertex(VertexPick),
    Edge(EdgePick),
    Face(view3d::Pick),
}

impl Hit {
    pub fn kind(&self) -> RefKind {
        match self {
            Hit::Vertex(_) => RefKind::Vertex,
            Hit::Edge(_) => RefKind::Edge,
            Hit::Face(_) => RefKind::Face,
        }
    }

    /// 当たった物を出した機能。
    pub fn feature(&self) -> FeatureId {
        match self {
            Hit::Vertex(v) => v.feature,
            Hit::Edge(e) => e.feature,
            Hit::Face(p) => p.feature,
        }
    }
}

/// 選ぶ欄の状態(画面の状態で、文書には入れない)。
#[derive(Debug, Default)]
pub struct PickFieldState {
    /// 3D のビューのクリックを入れる欄(機能、欄の `key`)
    pub active: std::cell::RefCell<Option<(FeatureId, String)>>,
    /// 直前の候補で埋めた欄(最初のクリックで、足すのでなく選び直す)
    pub auto: std::cell::RefCell<Vec<(FeatureId, String)>>,
    /// 欄の一覧でマウスを乗せている物(3D のビューで明るくする)
    pub hover: std::cell::RefCell<Option<Ref>>,
    /// 受けている欄で替えた選ぶ種類(欄、種類)。欄が替わると使わない(欄の初めの種類に戻る)。
    /// 普段の選ぶ種類とは別に持つので、欄の間に替えても、欄を出た後の普段の選択には残らない
    pub filter: std::cell::RefCell<Option<((FeatureId, String), PickFilter)>>,
    /// 3D のビューの頂点のクリックで埋める点の欄(機能、欄の `key`)。点から点の移動の「元の点」など
    pub point: std::cell::RefCell<Option<(FeatureId, String)>>,
}

/// 頂点のクリックで埋められる欄か(長さの単位の 3 つの数で、点の座標を持つ欄)。
pub(crate) fn is_point_field(f: &kmb_core::Field) -> bool {
    matches!(f.kind, FieldKind::Vector)
        && f.unit == Some(kmb_core::FieldUnit::Length)
        && (f.key.ends_with("origin") || f.key.ends_with("point"))
}

/// 欄が受ける種類から、選べる種類(頂点・辺・面・ボディのどれを選べるか)。ボディだけを受ける欄は
/// 面・辺のクリックもそのボディにするので、ボディだけにする。
fn field_mask(accept: &[RefKind]) -> PickFilter {
    let face = accept
        .iter()
        .any(|k| matches!(k, RefKind::Face | RefKind::PlanarFace | RefKind::Feature));
    PickFilter {
        vertex: accept.contains(&RefKind::Vertex),
        edge: accept.contains(&RefKind::Edge),
        face,
        body: accept.contains(&RefKind::Body),
    }
}

/// 欄の初めの選ぶ種類: 受ける頂点・辺・面の全部。面もボディも受ける欄(シェルの「面/ボディ」)は面から
/// (普段の左上で「ボディ」にしていても、欄では面を選べる)。ボディだけを受ける欄はボディ。
fn field_default(accept: &[RefKind]) -> PickFilter {
    let m = field_mask(accept);
    PickFilter {
        body: m.body && !(m.vertex || m.edge || m.face),
        ..m
    }
}

impl Workspace {
    /// 受けている欄の(今の選ぶ種類、選べる種類)。欄が受けていなければ `None`。
    pub(crate) fn field_filter(&self) -> Option<(PickFilter, PickFilter)> {
        let (entry, field) = self.active_pick_field()?;
        let FieldKind::Pick { accept, .. } = &field.kind else {
            return None;
        };
        let key = (entry.id, field.key.clone());
        let now = match &*self.cur.pick_field.filter.borrow() {
            Some((k, f)) if *k == key => *f,
            _ => field_default(accept),
        };
        Some((now, field_mask(accept)))
    }

    /// 今の選ぶ種類(欄が受けていればその欄の、なければ普段の)。
    pub(crate) fn effective_pick_filter(&self) -> PickFilter {
        self.field_filter().map_or(self.win.pick_filter, |(f, _)| f)
    }

    /// 選ぶ種類を初め(頂点・辺・面の全部)に戻す。欄が受けていれば、その欄の初めの種類に戻す。
    pub(crate) fn reset_pick_filter(&mut self) {
        if self.active_pick_field().is_some() {
            *self.cur.pick_field.filter.borrow_mut() = None;
        } else {
            self.set_pick_filter(PickFilter::default());
        }
    }

    /// 選ぶ種類を替える。ボディとそれ以外は混ぜないので、替えた時に合わない選びは外す。
    pub(crate) fn set_pick_filter(&mut self, f: PickFilter) {
        if self.win.pick_filter == f {
            return;
        }
        let was_body = self.win.pick_filter.body;
        self.win.pick_filter = f;
        if f.body && !was_body {
            self.cur.picked = None;
            self.cur.also_picked.clear();
            self.cur.picked_edges.clear();
            self.cur.picked_vertices.clear();
        }
        if !f.face_on() && !f.body {
            self.cur.picked = None;
            self.cur.also_picked.clear();
        }
        if !f.edge_on() {
            self.cur.picked_edges.clear();
        }
        if !f.vertex_on() {
            self.cur.picked_vertices.clear();
        }
    }

    /// 選ぶ種類の切り替えを 1 つ押した(0 頂点、1 辺、2 面、3 ボディ)。ボディを選んでいる間に
    /// 頂点・辺・面を押すと、ボディをやめてそれを選べるようにする。頂点・辺・面が全部切れる押し方はしない。
    pub(crate) fn toggle_pick_kind(&mut self, k: usize) {
        // 欄が受けている間は、その欄の種類だけを替える(受けない種類は押せない)
        if let Some((now, mask)) = self.field_filter() {
            let Some(f) = toggled(now, k)
                .filter(|f| (!f.vertex || mask.vertex) && (!f.edge || mask.edge))
                .filter(|f| (!f.face || mask.face) && (!f.body || mask.body))
                .filter(|f| f.body || f.vertex || f.edge || f.face)
            else {
                return;
            };
            if let Some((entry, field)) = self.active_pick_field() {
                *self.cur.pick_field.filter.borrow_mut() = Some(((entry.id, field.key), f));
            }
            return;
        }
        if let Some(f) = toggled(self.win.pick_filter, k) {
            self.set_pick_filter(f);
        }
    }

    /// 選ぶ種類の切り替え(ビューの左上、帯の「選択」、Ctrl+右クリックのメニューに出す)。
    /// 欄が受けている間は、その欄の種類を出し、受けない種類は押せなくする。
    pub(crate) fn pick_filter_items(&self) -> [(&'static str, bool, bool); 4] {
        let (f, mask) = self.field_filter().unwrap_or((
            self.win.pick_filter,
            PickFilter {
                vertex: true,
                edge: true,
                face: true,
                body: true,
            },
        ));
        [
            ("頂点", f.vertex_on(), mask.vertex),
            ("辺", f.edge_on(), mask.edge),
            ("面", f.face_on(), mask.face),
            ("ボディ", f.body, mask.body),
        ]
    }

    /// 選んでいる頂点。
    pub fn picked_vertices(&self) -> &[VertexPick] {
        &self.cur.picked_vertices
    }

    pub fn set_picked_vertices(&mut self, v: Vec<VertexPick>) {
        self.cur.picked_vertices = v;
    }

    /// 頂点 `v` の位置(表示している形の角から)。
    pub(crate) fn vertex_at(&mut self, v: &VertexPick) -> Option<[f64; 3]> {
        self.scene_edges();
        self.cur
            .vertex_cache
            .iter()
            .find(|(f, _)| *f == v.feature)?
            .1
            .iter()
            .find(|x| x.name == v.vertex)
            .map(|x| x.at)
    }

    /// 画面上の点 `pos` のいちばん近くの、見えている頂点。
    pub(crate) fn vertex_hit(&mut self, rect: Rect, pos: Pos2) -> Option<VertexPick> {
        self.scene_edges();
        let see_through = !self.win.layout.shading.faces();
        let front = if see_through {
            None
        } else {
            view3d::picks_at(&self.cur.scene, &self.cur.camera, rect, pos)
                .first()
                .map(|(_, at)| self.cur.camera.project(*at, rect).1)
        };
        let (lo, hi) = self.cur.scene.bounds();
        let size = (0..3).map(|k| hi[k] - lo[k]).fold(0.0f32, f32::max);
        let eps = 0.01 * size.max(1.0);
        let mut best: Option<(f32, VertexPick)> = None;
        for (feature, list) in &self.cur.vertex_cache {
            if !self.cur.scene.bodies.iter().any(|b| b.feature == *feature) {
                continue;
            }
            for v in list {
                let (p, depth) = self.cur.camera.project(v.at.map(|c| c as f32), rect);
                let d = p.distance(pos);
                let hidden = front.is_some_and(|f| depth < f - eps);
                if d < VERTEX_PX && !hidden && best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                    best = Some((
                        d,
                        VertexPick {
                            feature: *feature,
                            vertex: v.name.clone(),
                        },
                    ));
                }
            }
        }
        best.map(|b| b.1)
    }

    /// 画面上の点 `pos` の物。選べる種類のうち、頂点 → 辺 → 面の順に拾う。
    pub(crate) fn hit_at(&mut self, rect: Rect, pos: Pos2, f: PickFilter) -> Option<Hit> {
        if f.vertex_on()
            && let Some(v) = self.vertex_hit(rect, pos)
        {
            return Some(Hit::Vertex(v));
        }
        if f.edge_on()
            && let Some(e) = self.edge_hit(rect, pos)
        {
            return Some(Hit::Edge(e));
        }
        if f.face_on() {
            return view3d::pick_at(&self.cur.scene, &self.cur.camera, rect, pos).map(Hit::Face);
        }
        None
    }

    /// 頂点・辺・面のどれかを選ぶ。Shift で足す、Ctrl で足す・外す。修飾キーなしなら選びを
    /// その 1 つにする(ほかの種類の選びも外す)。何もない所なら選びを外す。
    pub(crate) fn pick_hit(&mut self, hit: Option<Hit>, add: bool, remove: bool) {
        // 面・辺・頂点を選んだら、タイムラインとブラウザの機能の選びを外す(Delete などは
        // 見えている選びにだけ効く。開いた直後は最後の機能が選ばれたままで、面を選んで Delete を押すと
        // 機能が消えていた。Kie 2026-10-22)。作る途中・直す途中・手順の途中は、選びが欄に入るので外さない
        if hit.is_some()
            && self.cur.creating.is_none()
            && self.cur.editing.is_none()
            && self.cur.gesture.is_none()
            && !self.selection().is_empty()
        {
            self.clear_selection();
        }
        if !add && !remove {
            if !matches!(hit, Some(Hit::Vertex(_))) {
                self.cur.picked_vertices.clear();
            }
            if !matches!(hit, Some(Hit::Edge(_))) {
                self.cur.picked_edges.clear();
            }
            if !matches!(hit, Some(Hit::Face(_))) {
                self.cur.picked = None;
                self.cur.also_picked.clear();
            }
        }
        match hit {
            Some(Hit::Face(p)) => self.pick_with(Some(p), add, remove),
            Some(Hit::Edge(e)) => self.pick_edge_with(Some(e), add, remove),
            Some(Hit::Vertex(v)) => {
                let has = self.cur.picked_vertices.contains(&v);
                match (add, remove) {
                    (_, true) if has => self.cur.picked_vertices.retain(|x| *x != v),
                    (true, _) | (_, true) => {
                        if !has {
                            self.cur.picked_vertices.push(v);
                        }
                    }
                    _ => self.cur.picked_vertices = vec![v],
                }
            }
            None if !add && !remove => {
                self.cur.picked = None;
                self.cur.also_picked.clear();
            }
            None => {}
        }
    }

    /// 選んだ頂点と、マウスの近くの頂点を点で描く。
    pub(crate) fn draw_vertices(&mut self, ui: &egui::Ui, rect: Rect, hover: Option<&Hit>) {
        let hover = match hover {
            Some(Hit::Vertex(v)) => Some(v.clone()),
            _ => None,
        };
        if self.cur.picked_vertices.is_empty() && hover.is_none() {
            return;
        }
        let painter = ui.painter_at(rect);
        let color = self.win.look.ws();
        let soft = crate::theme::mix(self.win.look.pal.panel, color, 0.6);
        let list: Vec<(VertexPick, f32, egui::Color32)> = self
            .cur
            .picked_vertices
            .iter()
            .map(|v| (v.clone(), 4.5, color))
            .chain(hover.map(|v| (v, 4.0, soft)))
            .collect();
        for (v, r, c) in list {
            if let Some(at) = self.vertex_at(&v) {
                let (p, _) = self.cur.camera.project(at.map(|x| x as f32), rect);
                painter.circle(p, r, c, egui::Stroke::new(1.0, self.win.look.pal.panel));
            }
        }
    }

    /// 選んでいる物の形(下の帯の数値に使う)。
    fn geo_of(&mut self, r: &Ref) -> Option<Geo> {
        match r {
            Ref::Vertex { feature, vertex } => {
                let at = self.vertex_at(&VertexPick {
                    feature: *feature,
                    vertex: vertex.clone(),
                })?;
                Some(Geo {
                    kind: Kind::Point(at),
                    points: vec![at],
                    segs: vec![],
                    tris: vec![],
                })
            }
            Ref::Edge { feature, faces } => {
                self.scene_edges();
                let list: Vec<&kmb_core::EdgeInfo> = self
                    .cur
                    .edge_cache
                    .1
                    .iter()
                    .filter(|(f, _)| f == feature)
                    .flat_map(|(_, l)| l.iter().filter(|e| e.faces == *faces))
                    .collect();
                let segs: Vec<[[f64; 3]; 2]> = list
                    .iter()
                    .flat_map(|e| {
                        view3d::edge_polyline(e)
                            .windows(2)
                            .map(|w| [w[0], w[1]])
                            .collect::<Vec<_>>()
                    })
                    .collect();
                let kind = match list.as_slice() {
                    [e] => match e.arc {
                        None => Kind::Line { a: e.a, b: e.b },
                        Some(a) => Kind::Circle(measure::Circle {
                            center: a.center,
                            normal: a.axis,
                            radius: a.radius,
                            closed: (a.sweep.abs() - std::f64::consts::TAU).abs() < 1e-6,
                            sweep: a.sweep,
                        }),
                    },
                    [] => return None,
                    _ => Kind::Curve {
                        length: segs.iter().map(|s| measure::dist(s[0], s[1])).sum(),
                    },
                };
                Some(Geo {
                    kind,
                    points: vec![],
                    segs,
                    tris: vec![],
                })
            }
            Ref::Face { feature, face } => measure::resolve(
                &self.cur.scene,
                &measure::Target::Face(view3d::Pick {
                    feature: *feature,
                    face: face.clone(),
                }),
            ),
            Ref::Body(_) => None,
        }
    }

    /// 3D のビューで選んでいる頂点・辺・面(ボディは含めない)。
    pub(crate) fn picked_refs(&self) -> Vec<Ref> {
        let mut v: Vec<Ref> = self.picked_faces().into_iter().map(Ref::from).collect();
        v.extend(self.cur.picked_edges.iter().cloned().map(Ref::from));
        v.extend(self.cur.picked_vertices.iter().cloned().map(Ref::from));
        v
    }

    /// 下の帯に出す、選んでいる物の数値(「頂点 | X: … Y: … Z: … mm」、
    /// 2 つなら「1 頂点 1 面 | 最小距離: … mm」。詳しく測るのは測る道具)。
    pub(crate) fn selection_info(&mut self) -> Option<String> {
        let refs = self.picked_refs();
        // 種類ごとの数(「2 辺 1 面」)
        let counts = |refs: &[Ref]| {
            [RefKind::Vertex, RefKind::Edge, RefKind::Face]
                .iter()
                .filter_map(|k| {
                    let n = refs.iter().filter(|r| r.kind() == *k).count();
                    (n > 0).then(|| format!("{n} {}", k.label()))
                })
                .collect::<Vec<_>>()
                .join(" ")
        };
        let num = |v: f64| if v.abs() < 5e-4 { 0.0 } else { v };
        match refs.as_slice() {
            [] => None,
            [r] => {
                let g = self.geo_of(r)?;
                let text = match &g.kind {
                    Kind::Point(p) => {
                        let u = self.length_unit();
                        let v = |i: usize| num(u.from_mm(p[i]));
                        format!("X: {:.3} Y: {:.3} Z: {:.3} {}", v(0), v(1), v(2), u.name())
                    }
                    Kind::Line { a, b } => format!("長さ: {}", measure::mm(measure::dist(*a, *b))),
                    Kind::Curve { length } => format!("長さ: {}", measure::mm(*length)),
                    Kind::Circle(c) => format!("半径: {}", measure::mm(c.radius)),
                    Kind::Cylinder(c) => format!("半径: {}", measure::mm(c.radius)),
                    Kind::Plane { area, .. } | Kind::Surface { area } => {
                        format!("面積: {}", self.length_unit().power(*area, 2, 3))
                    }
                };
                Some(format!("{} | {text}", r.kind().label()))
            }
            [a, b] => {
                let (ga, gb) = (self.geo_of(a)?, self.geo_of(b)?);
                let (d, _, _) = measure::closest(&ga, &gb)?;
                Some(format!("{} | 最小距離: {}", counts(&refs), measure::mm(d)))
            }
            many => Some(counts(many)),
        }
    }

    // ---- 選ぶ欄 ----

    /// 機能 `id` の欄 `key` が、3D のビューのクリックを受けているか。
    pub(crate) fn is_pick_field(&self, id: FeatureId, key: &str) -> bool {
        self.cur
            .pick_field
            .active
            .borrow()
            .as_ref()
            .is_some_and(|(f, k)| *f == id && k == key)
    }

    /// 欄を押した: 受ける状態にする(もう受けていればやめる)。
    pub(crate) fn toggle_pick_field(&self, id: FeatureId, key: &str) {
        *self.cur.pick_field.filter.borrow_mut() = None;
        *self.cur.pick_field.point.borrow_mut() = None;
        let mut a = self.cur.pick_field.active.borrow_mut();
        if a.as_ref().is_some_and(|(f, k)| *f == id && k == key) {
            *a = None;
        } else {
            *a = Some((id, key.to_string()));
        }
    }

    /// クリックを受けている欄(直している機能の、今使う欄だけ)。機能、欄。
    pub(crate) fn active_pick_field(&self) -> Option<(kmb_core::FeatureEntry, kmb_core::Field)> {
        let (id, key) = self.cur.pick_field.active.borrow().clone()?;
        if !self.is_editing(id) {
            // 直すのを終えた機能の欄は忘れる(次に直す時に、押さないうちから受けないように)
            *self.cur.pick_field.active.borrow_mut() = None;
            return None;
        }
        let mut entry = self.cur.work.doc().data().feature(id)?.clone();
        let desc = self.win.registry.descriptor(&entry.type_id)?;
        // 前の形のデータは今の形にして渡す(行の並びの行の欄が読めるように)
        entry.data = desc.upgraded(&entry.data);
        let field = desc
            .field_at(&key)
            .filter(|f| matches!(f.kind, FieldKind::Pick { .. }))?;
        field.applies(&entry.data).then_some((entry, field))
    }

    /// 機能 `id` の点の欄 `key` が、3D のビューの頂点のクリックを受けているか。
    pub(crate) fn is_point_pick(&self, id: FeatureId, key: &str) -> bool {
        self.cur
            .pick_field
            .point
            .borrow()
            .as_ref()
            .is_some_and(|(f, k)| *f == id && k == key)
    }

    /// 点の欄の「選ぶ」を押した: 頂点のクリックを受ける状態にする(もう受けていればやめる)。
    /// 選ぶ欄とは同時に受けない(クリックの行き先を 1 つにする)
    pub(crate) fn toggle_point_pick(&self, id: FeatureId, key: &str) {
        *self.cur.pick_field.active.borrow_mut() = None;
        *self.cur.pick_field.filter.borrow_mut() = None;
        let mut p = self.cur.pick_field.point.borrow_mut();
        if p.as_ref().is_some_and(|(f, k)| *f == id && k == key) {
            *p = None;
        } else {
            *p = Some((id, key.to_string()));
        }
    }

    /// 頂点のクリックを受けている点の欄(直している機能の、今使う欄だけ)。機能、欄。
    pub(crate) fn active_point_pick(&self) -> Option<(kmb_core::FeatureEntry, kmb_core::Field)> {
        let (id, key) = self.cur.pick_field.point.borrow().clone()?;
        if !self.is_editing(id) {
            *self.cur.pick_field.point.borrow_mut() = None;
            return None;
        }
        let entry = self.cur.work.doc().data().feature(id)?.clone();
        let desc = self.win.registry.descriptor(&entry.type_id)?;
        let field = desc
            .field_at(&key)
            .filter(|f| is_point_field(f) && f.applies(&entry.data))?;
        Some((entry, field))
    }

    /// 点の欄が受けている間の 3D のビューのクリック。いちばん近い頂点の座標を入れ、後ろの空いた
    /// 点の欄へ進む(点から点の移動は「元の点」→「先の点」の順に頂点を押せば済む)。
    /// 頂点に当たらなければ何も変えず、理由を出す
    pub(crate) fn point_pick_click(&mut self, rect: Rect, pos: Pos2) {
        let Some((entry, field)) = self.active_point_pick() else {
            return;
        };
        let Some(at) = self.vertex_hit(rect, pos).and_then(|v| self.vertex_at(&v)) else {
            self.win.message = Some(format!(
                "「{}」には頂点(ボディの角)をクリックしてください",
                field.label
            ));
            return;
        };
        let mut data = entry.data.clone();
        field.set(&mut data, Some(serde_json::Value::from(at.to_vec())));
        if data != entry.data {
            self.set_panel_data(&entry, data.clone());
        }
        let next = self.win.registry.descriptor(&entry.type_id).and_then(|d| {
            let i = d.fields.iter().position(|f| f.key == field.key)?;
            d.fields[i + 1..]
                .iter()
                .find(|f| is_point_field(f) && f.applies(&data) && f.get(&data).is_none())
                .map(|f| f.key.clone())
        });
        *self.cur.pick_field.point.borrow_mut() = next.map(|k| (entry.id, k));
    }

    /// 行の並びの行の欄が受けているのをやめる(行を消して番号がずれる時)。
    pub(crate) fn leave_rows_pick(&self) {
        *self.cur.pick_field.filter.borrow_mut() = None;
        *self.cur.pick_field.active.borrow_mut() = None;
        *self.cur.pick_field.hover.borrow_mut() = None;
    }

    /// 選ぶ欄を受ける状態をやめる(Esc の 1 段)。やめたら真。
    pub(crate) fn leave_pick_field(&mut self) -> bool {
        let had = self.active_pick_field().is_some() || self.active_point_pick().is_some();
        *self.cur.pick_field.point.borrow_mut() = None;
        *self.cur.pick_field.filter.borrow_mut() = None;
        *self.cur.pick_field.active.borrow_mut() = None;
        *self.cur.pick_field.hover.borrow_mut() = None;
        had
    }

    /// 作り始めた機能の、最初の選ぶ欄を受ける状態にする。`auto` は直前の候補で埋めた欄。
    pub(crate) fn enter_first_pick_field(&mut self, id: FeatureId, auto: &[String]) {
        let Some(entry) = self.cur.work.doc().data().feature(id).cloned() else {
            return;
        };
        let Some(d) = self.win.registry.descriptor(&entry.type_id) else {
            return;
        };
        let picks: Vec<&kmb_core::Field> = d
            .fields
            .iter()
            .filter(|f| matches!(f.kind, FieldKind::Pick { .. }) && f.applies(&entry.data))
            .collect();
        // 先に選んだ物で埋まった欄は飛ばし、まだ選んでいない要る欄(候補で仮に埋めた欄も)から
        // 始める(先にボディを選んでから結合を押すと、ターゲットに入りツールの欄が受ける)
        let open = picks.iter().find(|f| {
            matches!(f.kind, FieldKind::Pick { min, .. } if min > 0)
                && (f.refs(&entry.data).is_empty() || auto.contains(&f.key))
        });
        let first = open.or(picks.first());
        *self.cur.pick_field.active.borrow_mut() = first.map(|f| (id, f.key.clone()));
        *self.cur.pick_field.auto.borrow_mut() = auto.iter().map(|k| (id, k.clone())).collect();
    }

    /// 欄の受ける種類で、クリックした所の物を参照にする。ボディを受ける欄は、面・辺・頂点の
    /// ボディにする。受けない物なら `None`。クリックした物のボディ(その時の最後の機能)も返す。
    fn ref_for_field(
        &mut self,
        rect: Rect,
        pos: Pos2,
        accept: &[RefKind],
    ) -> Option<(Ref, FeatureId)> {
        // 欄の選ぶ種類(普段の左上の種類は見ない。普段「ボディ」にしていても欄では面を選べる)
        let ff = self
            .field_filter()
            .map_or_else(|| field_default(accept), |(f, _)| f);
        let filter = PickFilter {
            vertex: ff.vertex_on(),
            edge: ff.edge_on(),
            face: ff.face_on() || ff.body,
            body: false,
        };
        let hit = self.hit_at(rect, pos, filter)?;
        let r = match hit {
            Hit::Vertex(v) => Ref::from(v),
            Hit::Edge(e) => Ref::from(e),
            Hit::Face(p) => Ref::from(p),
        };
        let body = r.feature();
        if ff.body && accept.contains(&RefKind::Body) {
            // 欄で「ボディ」を選んでいれば、面も受ける欄(シェルの「面/ボディ」)でもボディにする
            return Some((Ref::Body(body), body));
        }
        let cx = self.context();
        if r.accepted_by(accept, &|r| cx.is_planar(r)) {
            return Some((r, body));
        }
        if accept.contains(&RefKind::Feature) {
            // 機能を受ける欄は、クリックした面を作った機能(写しの面なら元の機能)
            let creator = match &r {
                Ref::Face { face, .. } => kmb_core::face_feature(face),
                _ => None,
            };
            return Some((Ref::Body(creator.unwrap_or(body)), body));
        }
        accept
            .contains(&RefKind::Body)
            .then_some((Ref::Body(body), body))
    }

    /// 機能 `before` より前にある、道具の作り方を持つ機能(パターン・ミラーに使える機能)。
    pub(crate) fn recipe_features_before(&self, before: FeatureId) -> Vec<FeatureId> {
        self.cur
            .work
            .doc()
            .data()
            .features
            .iter()
            .take_while(|f| f.id != before)
            .filter(|f| self.output(f.id, "recipe").is_some())
            .map(|f| f.id)
            .collect()
    }

    /// 機能の並びを、タイムラインの順にする(パターンは最後の機能の後のボディに足すため)。
    pub(crate) fn in_timeline_order(&self, refs: &mut [Ref]) {
        let at = |r: &Ref| {
            self.cur
                .work
                .doc()
                .data()
                .features
                .iter()
                .position(|f| f.id == r.feature())
        };
        refs.sort_by_key(|r| at(r));
    }

    /// 選ぶ欄が受けている間の 3D のビューのクリック。1 つだけ受ける欄は入れ替えて次の欄へ進む。
    /// いくつでも受ける欄は足す・外し(もう一度押すと外す)、足した時に空いた要る欄があればそこへ進む
    /// (Kie 2026-10-21。次の欄へ送り、続けて足す時は
    /// Ctrl(`add`)を押す。空いた欄が無ければ留まって足し続ける)。
    pub(crate) fn pick_field_click(&mut self, rect: Rect, pos: Pos2, add: bool) {
        let Some((_, field)) = self.active_pick_field() else {
            return;
        };
        let FieldKind::Pick { accept, .. } = &field.kind else {
            return;
        };
        let Some((r, hit_body)) = self.ref_for_field(rect, pos, accept) else {
            self.win.message = Some(format!(
                "「{}」は{}を選ぶ欄です",
                field.label,
                kinds_text(accept)
            ));
            return;
        };
        self.pick_field_put(r, hit_body, add);
    }

    /// ブラウザのボディの行を、選ぶ欄が受けている間に押した(`root` はボディを最初に作った機能)。
    /// 作っている途中の機能を決めて終えず、ボディを受ける欄ならそのボディを入れる。受けていれば
    /// `true`(行を選ぶことはしない)。ブラウザは最後までの形のボディを並べるので、直している機能の
    /// 手前のそのボディ(最後の機能が違う)に読み替える。
    pub(crate) fn pick_field_body_row(&mut self, root: FeatureId, add: bool) -> bool {
        let Some((entry, field)) = self.active_pick_field() else {
            return false;
        };
        let FieldKind::Pick { accept, .. } = &field.kind else {
            return false;
        };
        if !accept.contains(&RefKind::Body) {
            self.win.message = Some(format!(
                "「{}」は{}を選ぶ欄です。3D のビューで選んでください",
                field.label,
                kinds_text(accept)
            ));
            return true;
        }
        let data = self.cur.work.doc().data();
        let reg = &self.win.registry;
        let last =
            kmb_core::visible_bodies_before(data, reg, self.cur.work.report(), Some(entry.id))
                .into_iter()
                .map(|(id, _)| id)
                .find(|id| kmb_core::body_root(data, reg, *id) == root);
        match last {
            Some(last) => self.pick_field_put(Ref::Body(last), last, add),
            None => {
                self.win.message = Some(format!(
                    "このボディは「{}」より後にできるので「{}」に使えません",
                    entry.name, field.label
                ));
            }
        }
        true
    }

    /// 選ぶ欄へ `r` を入れる(3D のビューのクリックとブラウザの行の共通)。`hit_body` は `r` のボディ。
    fn pick_field_put(&mut self, r: Ref, hit_body: FeatureId, add: bool) {
        let Some((entry, field)) = self.active_pick_field() else {
            return;
        };
        let FieldKind::Pick { accept, max, .. } = &field.kind else {
            return;
        };
        let feature_field = accept.contains(&RefKind::Feature);
        if feature_field && !self.recipe_features_before(entry.id).contains(&r.feature()) {
            let name = self
                .cur
                .work
                .doc()
                .data()
                .feature(r.feature())
                .map_or(r.feature().to_string(), |e| e.name.clone());
            self.win.message = Some(format!(
                "「{name}」は「{}」に使えません。押し出し・回転・穴・パターン・ミラーの面をクリックしてください",
                field.label
            ));
            return;
        }
        // ほかの欄にもう入っている物は入れない(例: ブーリアンの対象を道具にもする)。
        // 機能の欄とボディの欄は、同じ番号でも別の物(機能とボディ)なので比べない
        let desc = self.win.registry.descriptor(&entry.type_id);
        let other = desc.as_ref().and_then(|d| {
            d.pick_fields(&entry.data)
                .into_iter()
                .filter(|f| f.key != field.key && is_feature_field(f) == feature_field)
                .find(|f| f.refs(&entry.data).contains(&r))
                .map(|f| f.label)
        });
        if let Some(other) = other {
            self.win.message = Some(format!(
                "{}はもう「{other}」に入っています",
                self.field_ref_label(accept, &r)
            ));
            return;
        }
        let mut data = entry.data.clone();
        let auto = {
            let mut a = self.cur.pick_field.auto.borrow_mut();
            let had = a.iter().any(|(f, k)| *f == entry.id && *k == field.key);
            a.retain(|(f, k)| !(*f == entry.id && *k == field.key));
            had
        };
        let mut now = if auto { vec![] } else { field.refs(&data) };
        let single = *max == Some(1);
        let before = now.len();
        if accept.contains(&RefKind::Body) && accept.len() > 1 && !feature_field {
            // ボディとそれ以外は混ぜない(シェルはボディを選ぶと面を外し、面を選ぶとボディを外す)
            let body = matches!(r, Ref::Body(_));
            now.retain(|x| matches!(x, Ref::Body(_)) == body);
            if body && now.first().is_some_and(|x| *x != r) {
                now.clear();
            }
        }
        if single {
            now = vec![r];
        } else if let Some(i) = now.iter().position(|x| *x == r) {
            now.remove(i);
        } else if max.is_none_or(|m| now.len() < m) {
            now.push(r);
        }
        if feature_field {
            self.in_timeline_order(&mut now);
            // 足すボディの欄(ボディだけを受け、空でもよい欄)が空なら、クリックした面のボディを
            // 入れる(後の機能が受け継いだボディでも)
            if let Some(b) = desc.iter().flat_map(|d| &d.fields).find(|f| {
                matches!(&f.kind, FieldKind::Pick { accept, min: 0, max: Some(1) }
                    if accept == &[RefKind::Body])
                    && f.applies(&data)
                    && f.refs(&data).is_empty()
            }) {
                b.set_refs(&mut data, &[Ref::Body(hit_body)]);
            }
        }
        if let Some(d) = &desc {
            // 「+」で待っている行なら、前の行の値で行を足してから入れる
            d.ensure_row(&mut data, &field.key);
        }
        field.set_refs(&mut data, &now);
        if let Some(d) = &desc
            && d.prune_rows(&mut data)
        {
            // 空になった行は消える。番号がずれるので、受けるのをやめる
            self.leave_rows_pick();
        }
        if data != entry.data {
            self.set_panel_data(&entry, data.clone());
        }
        // いくつでも受ける欄は、足した時だけ(外した時と Ctrl の時は留まる)
        let added = !single && now.len() > before && !add;
        if single || added {
            // 埋まったら、空いている欄(要る欄のうち後ろ、無ければ前)へ。どれも埋まっていれば次の欄へ
            // (無ければ受けるのをやめる)。後ろへしか進まないと、前に空いた欄が残っていても止まる。
            // いくつでも受ける欄は、空いている要る欄へだけ進む(無ければ留まって足し続ける)
            let next = self.win.registry.descriptor(&entry.type_id).and_then(|d| {
                let picks: Vec<&kmb_core::Field> = d
                    .fields
                    .iter()
                    .filter(|f| matches!(f.kind, FieldKind::Pick { .. }) && f.applies(&data))
                    .collect();
                let at = picks.iter().position(|f| f.key == field.key)?;
                let empty = |f: &&&kmb_core::Field| {
                    matches!(f.kind, FieldKind::Pick { min, .. } if min > 0)
                        && f.refs(&data).is_empty()
                };
                let empty_one = picks[at + 1..]
                    .iter()
                    .find(empty)
                    .or_else(|| picks[..at].iter().find(empty));
                if single {
                    empty_one.or_else(|| picks.get(at + 1))
                } else {
                    empty_one
                }
                .map(|f| f.key.clone())
            });
            if single || next.is_some() {
                *self.cur.pick_field.active.borrow_mut() = next.map(|k| (entry.id, k));
            }
        }
    }

    /// 選ぶ欄が受けている間に明るくする面(欄に入っている物と、一覧でマウスを乗せた物)。
    pub(crate) fn pick_field_faces(&self) -> Vec<view3d::Pick> {
        let Some((entry, field)) = self.active_pick_field() else {
            return vec![];
        };
        let mut refs = field.refs(&entry.data);
        refs.extend(self.cur.pick_field.hover.borrow().clone());
        let feature_field = matches!(&field.kind, FieldKind::Pick { accept, .. }
            if accept.contains(&RefKind::Feature));
        let mut out = Vec::new();
        for r in refs {
            match r {
                Ref::Face { feature, face } => out.push(view3d::Pick { feature, face }),
                // 機能は、その機能が作った面(写しの面も)
                Ref::Body(f) if feature_field => {
                    for b in &self.cur.scene.bodies {
                        out.extend(
                            b.mesh
                                .face_names
                                .iter()
                                .filter(|n| kmb_core::face_feature(n) == Some(f))
                                .map(|n| view3d::Pick {
                                    feature: b.feature,
                                    face: n.clone(),
                                }),
                        );
                    }
                }
                Ref::Body(f) => {
                    if let Some(b) = self.cur.scene.bodies.iter().find(|b| b.feature == f) {
                        out.extend(b.mesh.face_names.iter().map(|n| view3d::Pick {
                            feature: f,
                            face: n.clone(),
                        }));
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// 選ぶ欄が受けている間、マウスの横に出す案内。
    pub(crate) fn draw_pick_field_hint(&self, ui: &egui::Ui, rect: Rect, hover: Option<Pos2>) {
        let (Some((entry, field)), Some(pos)) = (self.active_pick_field(), hover) else {
            return;
        };
        let FieldKind::Pick { accept, max, .. } = &field.kind else {
            return;
        };
        let n = field.refs(&entry.data).len();
        let auto = self
            .cur
            .pick_field
            .auto
            .borrow()
            .contains(&(entry.id, field.key.clone()));
        let text = if n == 0 {
            format!("{}: {}を選ぶ", field.label, kinds_text(accept))
        } else if auto {
            // 候補で仮に埋めた欄は、最初のクリックで選び直す
            format!("{}: 仮に {n} 個。クリックで選び直す", field.label)
        } else if *max == Some(1) {
            format!("{}: クリックで替える", field.label)
        } else {
            format!("{}: {n} 個。クリックで足す・外す", field.label)
        };
        let l = self.win.look;
        let painter = ui.painter_at(rect);
        let g = painter.layout_no_wrap(text, crate::theme::prop(11.5), l.pal.ink);
        let r = Rect::from_min_size(
            pos + egui::vec2(16.0, 14.0),
            g.size() + egui::vec2(10.0, 4.0),
        );
        painter.rect(
            r,
            3.0,
            l.pal.panel,
            egui::Stroke::new(1.0, l.ws()),
            egui::StrokeKind::Inside,
        );
        painter.galley(r.min + egui::vec2(5.0, 2.0), g, l.pal.ink);
    }

    /// 欄の中の 1 つを、欄の受ける種類に合わせて出す名前(機能を受ける欄は機能の名前)。
    pub(crate) fn field_ref_label(&self, accept: &[RefKind], r: &Ref) -> String {
        match r {
            Ref::Body(f) if accept.contains(&RefKind::Feature) => self
                .cur
                .work
                .doc()
                .data()
                .feature(*f)
                .map_or(format!("機能({f})"), |e| e.name.clone()),
            _ => self.ref_label(r),
        }
    }

    /// 欄の中の 1 つを画面に出す名前。
    pub(crate) fn ref_label(&self, r: &Ref) -> String {
        match r {
            Ref::Body(f) => self.body_label(*f),
            Ref::Face { face, .. } => self.face_label(face),
            Ref::Edge { faces, .. } => format!(
                "辺({} と {})",
                self.face_label(&faces[0]),
                self.face_label(&faces[1])
            ),
            Ref::Vertex { vertex, .. } => {
                format!(
                    "頂点({} 面の角)",
                    vertex.split('~').next().unwrap_or("").split('|').count()
                )
            }
        }
    }

    /// ボディ(その時の最後の機能 `f`)の名前。名前を付けていればその名前、無ければ最初に作った機能の名前。
    pub(crate) fn body_label(&self, f: FeatureId) -> String {
        let root = self.body_root(f);
        let data = self.cur.work.doc().data();
        match data.body_names.get(&root) {
            Some(n) => n.clone(),
            None => data
                .feature(root)
                .map_or(format!("ボディ({f})"), |e| format!("ボディ({})", e.name)),
        }
    }

    /// 3D のビューで選んでいる面・辺・頂点と、指している物のボディ(最初に作った機能。ブラウザの印)。
    pub(crate) fn marked_bodies(&self) -> Vec<FeatureId> {
        let c = &self.cur;
        let mut out: Vec<FeatureId> = Vec::new();
        let features = c
            .picked
            .iter()
            .chain(&c.also_picked)
            .map(|p| p.feature)
            .chain(c.picked_edges.iter().map(|e| e.feature))
            .chain(c.picked_vertices.iter().map(|v| v.feature))
            .chain(c.hover_feature);
        for f in features {
            let root = self.body_root(f);
            if !out.contains(&root) {
                out.push(root);
            }
        }
        out
    }

    /// 機能 `id` のボディを最初に作った機能(対象のボディをたどった元)。
    pub(crate) fn body_root(&self, mut id: FeatureId) -> FeatureId {
        let data = self.cur.work.doc().data();
        for _ in 0..data.features.len() {
            let Some(f) = data.feature(id) else { break };
            let Some(Ok(feature)) = self.win.registry.instantiate(&f.type_id, &f.data) else {
                break;
            };
            let up = feature
                .inputs()
                .unwrap_or_default()
                .into_iter()
                .find_map(|i| match i {
                    kmb_core::InputRef::Output { feature, slot } if slot == "body" => Some(feature),
                    _ => None,
                });
            match up {
                Some(u) => id = u,
                None => break,
            }
        }
        id
    }

    /// 3D のビューに出す形を決める: 選ぶ欄が受けている間はその機能の手前、それ以外は最後まで。
    pub(crate) fn sync_scene_stage(&mut self) {
        let want = self.active_pick_field().map(|(e, _)| e.id);
        if want == self.cur.scene_before {
            return;
        }
        self.cur.scene_before = want;
        let bodies = kmb_core::visible_bodies_before(
            self.cur.work.doc().data(),
            &self.win.registry,
            self.cur.work.report(),
            want,
        );
        self.cur
            .scene
            .update(self.cur.work.kernel().as_ref(), &bodies);
        self.refresh_hidden();
    }
}

/// 機能を受ける欄か(値はボディの欄と同じく機能の番号)。
pub(crate) fn is_feature_field(f: &kmb_core::Field) -> bool {
    matches!(&f.kind, FieldKind::Pick { accept, .. } if accept.contains(&RefKind::Feature))
}

/// 受ける種類を並べた文(例: 「ボディ」「辺か面」)。
pub fn kinds_text(accept: &[RefKind]) -> String {
    let v: Vec<&str> = accept.iter().map(|k| k.label()).collect();
    v.join("か")
}

/// 選ぶ種類の切り替えを 1 つ押した後の種類(0 頂点、1 辺、2 面、3 ボディ)。ボディを選んでいる間に
/// 頂点・辺・面を押すと、ボディをやめてそれを選べるようにする。頂点・辺・面が全部切れるなら `None`。
fn toggled(mut f: PickFilter, k: usize) -> Option<PickFilter> {
    if k == 3 {
        f.body = !f.body;
    } else if f.body {
        f.body = false;
        match k {
            0 => f.vertex = true,
            1 => f.edge = true,
            _ => f.face = true,
        }
    } else {
        match k {
            0 => f.vertex = !f.vertex,
            1 => f.edge = !f.edge,
            _ => f.face = !f.face,
        }
        if !(f.vertex || f.edge || f.face) {
            return None;
        }
    }
    Some(f)
}
