//! 3D スケッチの中央のビュー。視点の操作とボディの絵は画面(kmb-ui)の 3D のビューを使い、
//! その上にスケッチの線・点・面・拘束の印・寸法と、作図面を重ねる。

use super::ops::{DrawPlane, Sel};
use super::{
    ProjectPick, Sketch3dEditor, Start, Tool3, sketch_of, status_text, values_of, work_frame,
};
use crate::annot::ScreenDim;
use egui::{Color32, Pos2, Rect, Stroke, Vec2};
use kmb_core::{FeatureEntry, Plane};
use kmb_sketch::{ConstraintId, EntityId, ProjectionSource};
use kmb_sketch_3d::body::{
    BodyItem, BodyShape, body_geom, body_items, mesh_tris_named, piece, project_body,
    section_pieces,
};
use kmb_sketch_3d::{
    Axis, AxisPlane, DerivedDef, Entity3d, FaceRef, Kind3d, P3, PlaneRef, ProjectWhat, Sketch3d,
    Solution3d, basis, geom, plane_at, solve3d_dragging, to_local, to_world,
};
use kmb_ui::EditorHost;
use kmb_ui::view3d::{self, Camera};
use std::collections::HashMap;

mod dims;
mod draw;
mod hit;

pub(crate) use dims::*;
use draw::*;
pub(crate) use hit::*;

/// 点を引いている途中(引く点と、引く平面)。
pub(crate) struct Drag {
    point: EntityId,
    origin: P3,
    normal: P3,
    /// 引いている間の解(離すと文書へ入れる)
    preview: Option<Solution3d>,
}

/// 寸法の値の札を引いて動かしている所(視線に垂直な平面の上で動かす)。
pub(crate) struct LabelDrag {
    id: ConstraintId,
    anchor: P3,
    normal: P3,
    /// 掴んだ所から札の真ん中までのずれ(画面。引き始めで札が跳ねないように)
    grab: Vec2,
    /// 今のずれ(基準の点から)
    offset: Option<P3>,
}

/// 点に吸い付く距離(画素)。
const SNAP_PX: f32 = 9.0;
/// 線分を引く時、画面で軸の線に吸い付く距離(画素)。
const AXIS_PX: f32 = 8.0;

/// 引いている線分の向き(推した向き)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Dir {
    /// 作図面の上の向き(0 が `u`、1 が `v`)
    Plane(usize),
    /// 座標の軸の向き(軸のガイド。作図面から出る向きにも引ける)
    Axis(Axis),
}
/// 線を拾う距離(画素)。
const LINE_PX: f32 = 6.0;

/// スケッチの座標と画面の位置の行き来。
pub(crate) struct Proj<'a> {
    pub cam: &'a Camera,
    pub rect: Rect,
    pub frame: Plane,
    /// 寸法の見せ方(スケッチの設定)
    pub fmt: kmb_sketch::DimFormat,
}

impl Proj<'_> {
    pub fn at(&self, p: P3) -> Pos2 {
        let w = to_world(&self.frame, p);
        self.cam.project(w.map(|v| v as f32), self.rect).0
    }

    fn seg(&self, a: P3, b: P3) -> Option<(Pos2, Pos2)> {
        let (a, b) = (to_world(&self.frame, a), to_world(&self.frame, b));
        self.cam
            .segment(a.map(|v| v as f32), b.map(|v| v as f32), self.rect)
    }

    /// 画面の位置を通る視線(スケッチの座標の始点と向き)。
    fn ray(&self, pos: Pos2) -> (P3, P3) {
        let (o, d) = self.cam.ray(pos, self.rect);
        let o = o.map(|v| v as f64);
        let e = [o[0] + d[0] as f64, o[1] + d[1] as f64, o[2] + d[2] as f64];
        let (ol, el) = (to_local(&self.frame, o), to_local(&self.frame, e));
        (ol, geom::sub(el, ol))
    }

    /// 視線と、点 `origin` を通り法線 `n` の平面の交わり(平面を真横から見ていれば `None`)。
    fn on_plane(&self, pos: Pos2, origin: P3, n: P3) -> Option<P3> {
        let (o, d) = self.ray(pos);
        let dn = geom::dot(d, n);
        if dn.abs() < 1e-4 {
            return None;
        }
        let t = geom::dot(geom::sub(origin, o), n) / dn;
        Some(geom::add(o, geom::scale(d, t)))
    }
}

/// 点と線分の画面の上の距離。
fn seg_dist(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    (a + ab * t - p).length()
}

/// 点が多角形の中か(偶奇の数え方)。
fn inside(p: Pos2, poly: &[Pos2]) -> bool {
    let mut odd = false;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            odd = !odd;
        }
    }
    odd
}

/// 作図面に描いた点を作図面に乗せる(座標の平面の作図面では何もしない)。
fn on_work(s: &mut Sketch3d, point: EntityId, dp: &DrawPlane) {
    if let Some(id) = dp.work {
        s.add_constraint(Kind3d::PointOnPlane {
            point,
            plane: PlaneRef::Work { id },
        });
    }
}

/// 作図面の上の矩形の 4 隅(対角の `a` と `c`。`c` は作図面へ寄せる)。つぶれていれば `None`。
fn rect_corners(dp: &DrawPlane, a: P3, c: P3) -> Option<[P3; 4]> {
    let d = geom::sub(c, a);
    let (du, dv) = (geom::dot(d, dp.u), geom::dot(d, dp.v));
    if du.abs() < 1e-6 || dv.abs() < 1e-6 {
        return None;
    }
    Some([a, dp.at(a, du, 0.0), dp.at(a, du, dv), dp.at(a, 0.0, dv)])
}

/// 作図面の四角の大きさ(画面の大きさに合わせた半分の幅)。
fn plane_half(cam: &Camera, rect: Rect) -> f64 {
    2.0 * view3d::guide_size(cam, rect) as f64
}

/// 作った作図面の名前の札(作図面、札の位置、名前)。描いている作図面の札は四角の隅に。
pub(crate) fn plane_tags(
    ed: &Sketch3dEditor,
    sk: &Sketch3d,
    sol: &Solution3d,
) -> Vec<(u32, Pos2, String)> {
    let pr = ed.proj();
    let half = plane_half(&ed.cam, ed.rect);
    sk.planes
        .iter()
        .filter_map(|w| {
            let f = if ed.work == Some(w.id) {
                ed.draw_plane(sk, sol)
            } else {
                work_frame(sk, sol, w.id)?
            };
            // 描いている作図面の札は、題(向かいの隅)と重ならない隅に
            let k = if ed.work == Some(w.id) { -1.0 } else { 0.6 };
            let at = pr.at(f.at(f.origin, k * half, k * half));
            Some((w.id, at + Vec2::new(4.0, -4.0), w.name.clone()))
        })
        .collect()
}

fn tag_rect(ui: &egui::Ui, at: Pos2, text: &str) -> Rect {
    let g = ui
        .painter()
        .layout_no_wrap(text.to_string(), kmb_ui::theme::prop(11.0), Color32::WHITE);
    Rect::from_min_size(
        at - Vec2::new(0.0, g.size().y),
        g.size() + Vec2::new(8.0, 4.0),
    )
}

/// 2 点を結ぶ線分が軸に近ければ(4° 以内)、その軸(既にある点どうしをつないだ時。点は動かさず
/// 拘束で沿わせる)。
fn near_axis(a: P3, b: P3) -> Option<Axis> {
    let d = geom::sub(b, a);
    let l = geom::len(d);
    if l < 1e-9 {
        return None;
    }
    let tol = 4f64.to_radians().cos();
    [Axis::X, Axis::Y, Axis::Z]
        .into_iter()
        .find(|ax| d[ax.index()].abs() / l > tol)
}

impl Sketch3dEditor {
    fn proj(&self) -> Proj<'_> {
        Proj {
            cam: &self.cam,
            rect: self.rect,
            frame: self.frame,
            fmt: self.fmt,
        }
    }

    /// スケッチの点の画面の位置(試験で使う)。
    #[cfg(test)]
    pub(crate) fn screen_of(&self, p: P3) -> Pos2 {
        self.proj().at(p)
    }

    /// 描く時のマウスの位置: 点の上ならその点、原点の近くなら原点、ほかは作図面の上の位置。
    fn snap(&self, pos: Pos2, sk: &Sketch3d, sol: &Solution3d, dp: &DrawPlane) -> Option<Start> {
        let pr = self.proj();
        let mut best: Option<(f32, EntityId, P3)> = None;
        for e in &sk.entities {
            if let Entity3d::Point { id, .. } = e
                && let Some(q) = sol.points.get(id)
            {
                let d = (pr.at(*q) - pos).length();
                if d < SNAP_PX && best.is_none_or(|b| d < b.0) {
                    best = Some((d, *id, *q));
                }
            }
        }
        if let Some((_, id, p)) = best {
            return Some(Start {
                id: Some(id),
                p,
                origin: false,
                off: false,
            });
        }
        // 原点は、作図面が原点を通る時だけ
        let through_origin =
            dp.work.is_none() || geom::dot(geom::sub([0.0; 3], dp.origin), dp.n).abs() < 1e-6;
        if through_origin && (pr.at([0.0; 3]) - pos).length() < SNAP_PX {
            return Some(Start {
                id: None,
                p: [0.0; 3],
                origin: true,
                off: false,
            });
        }
        let p = pr.on_plane(pos, dp.origin, dp.n)?;
        Some(Start {
            id: None,
            p,
            origin: false,
            off: false,
        })
    }

    /// 描く点(既にある点か、新しい点)。作図面に描いた新しい点は作図面に乗せる。
    fn new_point(&self, s: &mut Sketch3d, st: &Start, dp: &DrawPlane) -> EntityId {
        match st.id {
            Some(id) => id,
            None => {
                let id = s.add_point(st.p);
                if st.origin {
                    s.add_constraint(Kind3d::Fix { point: id });
                } else if !st.off {
                    on_work(s, id, dp);
                }
                id
            }
        }
    }

    fn mark_construction(&self, s: &mut Sketch3d, id: EntityId) {
        if let Some(Entity3d::Line { construction, .. } | Entity3d::Circle { construction, .. }) =
            s.entity_mut(id)
        {
            *construction = self.construction;
        }
    }

    /// 線分を足す(軸に沿う時は拘束も)。`len` があれば長さの寸法も付ける。
    fn add_line(
        &self,
        sk: &Sketch3d,
        a: &Start,
        b: &Start,
        dir: Option<Dir>,
        len: Option<String>,
        dp: &DrawPlane,
    ) -> (Sketch3d, EntityId) {
        let mut s = sk.clone();
        let pa = self.new_point(&mut s, a, dp);
        let pb = self.new_point(&mut s, b, dp);
        let l = s.add_line(pa, pb);
        self.mark_construction(&mut s, l);
        let kind = match dir {
            Some(Dir::Plane(k)) => dp.dir_kind(k, l),
            Some(Dir::Axis(axis)) => Some(Kind3d::Along { line: l, axis }),
            None => None,
        };
        if let Some(k) = kind {
            s.add_constraint(k);
        } else if (dp.work.is_none() || (a.id.is_some() && b.id.is_some()))
            && let Some(axis) = near_axis(a.p, b.p)
        {
            // 作図面に乗せた点は、作図面が軸を含まないと軸に沿えないので、座標の平面の時だけ
            s.add_constraint(Kind3d::Along { line: l, axis });
        }
        if let Some(value) = len {
            s.add_constraint(Kind3d::Length { line: l, value });
        }
        (s, pb)
    }

    fn add_rect(&self, sk: &Sketch3d, a: &Start, c: P3, dp: &DrawPlane) -> Option<Sketch3d> {
        let [_, p2, p3, p4] = rect_corners(dp, a.p, c)?;
        let mut s = sk.clone();
        let i1 = self.new_point(&mut s, a, dp);
        let ids: Vec<EntityId> = [p2, p3, p4]
            .iter()
            .map(|p| {
                let id = s.add_point(*p);
                on_work(&mut s, id, dp);
                id
            })
            .collect();
        let corners = [i1, ids[0], ids[1], ids[2]];
        let mut edges = Vec::new();
        for k in 0..4 {
            let l = s.add_line(corners[k], corners[(k + 1) % 4]);
            self.mark_construction(&mut s, l);
            edges.push(l);
            if let Some(kind) = dp.dir_kind(k % 2, l) {
                s.add_constraint(kind);
            }
        }
        if !dp.infers() {
            // 軸のない作図面では、向かい合う辺を平行に、隣の辺を直角に
            s.add_constraint(Kind3d::Perpendicular {
                a: edges[0],
                b: edges[1],
            });
            s.add_constraint(Kind3d::Parallel {
                a: edges[0],
                b: edges[2],
            });
            s.add_constraint(Kind3d::Parallel {
                a: edges[1],
                b: edges[3],
            });
        }
        Some(s)
    }

    fn add_circle(&self, sk: &Sketch3d, c: &Start, rim: P3, dp: &DrawPlane) -> Option<Sketch3d> {
        let r = geom::len(geom::sub(rim, c.p));
        if r < 1e-6 {
            return None;
        }
        let mut s = sk.clone();
        let ci = self.new_point(&mut s, c, dp);
        let id = s.add_circle(ci, r, dp.n);
        self.mark_construction(&mut s, id);
        if let Some(Entity3d::Circle { plane, .. }) = s.entity_mut(id) {
            *plane = dp.work;
        }
        Some(s)
    }

    /// 始めの点 `from` を通る X・Y・Z の軸の線のうち、マウスが画面で近い(8 px 以内)線と、その線の
    /// 上でマウスに近い点。作図面から出る向き(作図面の法線)にも引ける(まとめてレビュー R5)。
    /// 画面でほとんど点に見える(奥を向いた)軸は使わない。
    pub(crate) fn axis_guide(&self, from: P3, pos: Pos2) -> Option<(Axis, P3)> {
        let pr = self.proj();
        let s0 = pr.at(from);
        let rel = pos - s0;
        if rel.length() < 12.0 {
            return None;
        }
        let axes = [Axis::X, Axis::Y, Axis::Z];
        let dirs: Vec<Vec2> = axes
            .iter()
            .map(|a| pr.at(geom::add(from, a.unit())) - s0)
            .collect();
        let longest = dirs.iter().map(|d| d.length()).fold(0.0, f32::max);
        if longest < 1e-6 {
            return None;
        }
        let (o, d) = pr.ray(pos);
        let mut best: Option<(f32, Axis, P3)> = None;
        for (axis, dir) in axes.into_iter().zip(dirs) {
            if dir.length() < 0.25 * longest {
                continue;
            }
            let n = dir.normalized();
            let off = (rel.x * n.y - rel.y * n.x).abs();
            if off > AXIS_PX || best.is_some_and(|b| b.0 <= off) {
                continue;
            }
            // 軸の線と視線のいちばん近い点(軸の線の上)
            let u = axis.unit();
            let w0 = geom::sub(from, o);
            let (b, c) = (geom::dot(u, d), geom::dot(d, d));
            let den = c - b * b;
            if den.abs() < 1e-12 {
                continue;
            }
            let t = (b * geom::dot(d, w0) - c * geom::dot(u, w0)) / den;
            best = Some((off, axis, geom::add(from, geom::scale(u, t))));
        }
        best.map(|(_, a, p)| (a, p))
    }

    /// 線分の道具で、引き始めの点からマウスの所へ引く時の終点と向き。点か原点に吸い付けばそこ、
    /// 軸のガイドに乗ればその軸の上(作図面の外でもよい)、ほかは作図面の上(向きを推す)。
    fn line_end(
        &self,
        pos: Pos2,
        sk: &Sketch3d,
        sol: &Solution3d,
        dp: &DrawPlane,
    ) -> Option<(Start, Option<Dir>)> {
        let start = self.start?;
        let at = self.snap(pos, sk, sol, dp);
        if let Some(a) = at
            && (a.id.is_some() || a.origin)
        {
            return Some((a, None));
        }
        if let Some((axis, p)) = self.axis_guide(start.p, pos) {
            let off = geom::dot(geom::sub(p, dp.origin), dp.n).abs() > 1e-6;
            return Some((
                Start {
                    id: None,
                    p,
                    origin: false,
                    off,
                },
                Some(Dir::Axis(axis)),
            ));
        }
        let mut a = at?;
        let k = dp.infer(start.p, &mut a.p);
        Some((a, k.map(Dir::Plane)))
    }

    /// 描く道具のクリック。文書を変えれば新しいスケッチ。
    fn click_draw(&mut self, pos: Pos2, sk: &Sketch3d, sol: &Solution3d) -> Option<Sketch3d> {
        let dp = self.draw_plane(sk, sol);
        if self.tool == Tool3::Line
            && let Some(start) = self.start
        {
            let (at, dir) = self.line_end(pos, sk, sol, &dp)?;
            if at.id.is_some() && at.id == start.id {
                return None;
            }
            let (s, end) = self.add_line(sk, &start, &at, dir, None, &dp);
            self.anchor = at.p;
            // 既にある点で終えたら、つなぐのを終える(閉じた輪など)
            self.start = at.id.is_none().then_some(Start {
                id: Some(end),
                p: at.p,
                origin: false,
                off: false,
            });
            return Some(s);
        }
        let at = self.snap(pos, sk, sol, &dp)?;
        let Some(start) = self.start else {
            if self.tool == Tool3::Point {
                if at.id.is_some() {
                    return None;
                }
                let mut s = sk.clone();
                self.new_point(&mut s, &at, &dp);
                self.anchor = at.p;
                return Some(s);
            }
            self.anchor = at.p;
            self.start = Some(at);
            return None;
        };
        match self.tool {
            Tool3::Rectangle => {
                let s = self.add_rect(sk, &start, at.p, &dp);
                if s.is_some() {
                    self.start = None;
                }
                s
            }
            Tool3::Circle => {
                let rim = self.proj().on_plane(pos, start.p, dp.n).unwrap_or(at.p);
                let s = self.add_circle(sk, &start, rim, &dp);
                if s.is_some() {
                    self.start = None;
                }
                s
            }
            Tool3::Line
            | Tool3::Point
            | Tool3::Select
            | Tool3::Include
            | Tool3::Project
            | Tool3::Intersect => None,
        }
    }

    /// 線分の長さを打って Enter: マウスの向きへその長さの線分を引き、長さの寸法を付ける。
    pub(crate) fn typed_line(
        &mut self,
        host: &EditorHost<'_>,
        hover: Option<Pos2>,
        sk: &Sketch3d,
        sol: &Solution3d,
    ) -> Option<Sketch3d> {
        let start = self.start?;
        let src = kmb_ui::with_unit_of(self.typed.trim(), "0 mm");
        let len = match host.eval(&src) {
            Ok(q) if q.dim == kmb_core::Dim::LENGTH && q.value > 0.0 => q.value,
            _ => {
                self.note = Some(format!("「{}」は長さになりません", self.typed.trim()));
                self.typed.clear();
                return None;
            }
        };
        let dp = self.draw_plane(sk, sol);
        // 軸のガイドに乗っていれば、その軸の向きへ(作図面の外へも)
        let guide = hover.and_then(|p| self.axis_guide(start.p, p));
        let (dir, k) = match guide {
            Some((axis, p)) => {
                let d = geom::sub(p, start.p);
                let sign = if geom::dot(d, axis.unit()) < 0.0 {
                    -1.0
                } else {
                    1.0
                };
                (geom::scale(axis.unit(), sign), Some(Dir::Axis(axis)))
            }
            None => {
                let dir = hover
                    .and_then(|p| self.proj().on_plane(p, start.p, dp.n))
                    .map(|q| geom::sub(q, start.p))
                    .filter(|d| geom::len(*d) > 1e-9)
                    .unwrap_or(dp.u);
                let mut tip = geom::add(start.p, dir);
                let k = dp.infer(start.p, &mut tip);
                (geom::unit(geom::sub(tip, start.p)), k.map(Dir::Plane))
            }
        };
        let p = geom::add(start.p, geom::scale(dir, len));
        let end = Start {
            id: None,
            p,
            origin: false,
            off: geom::dot(geom::sub(p, dp.origin), dp.n).abs() > 1e-6,
        };
        let (s, id) = self.add_line(sk, &start, &end, k, Some(src), &dp);
        self.anchor = end.p;
        self.start = Some(Start {
            id: Some(id),
            p: end.p,
            origin: false,
            off: false,
        });
        self.typed.clear();
        Some(s)
    }

    /// 選ぶ道具のクリック(Ctrl・Shift で足す・外す)。
    fn click_select(&mut self, hit: Option<Hit>, add: bool, sol: &Solution3d) {
        let sel = match hit {
            Some(Hit::Constraint(c)) => {
                self.picked_constraint = (self.picked_constraint != Some(c)).then_some(c);
                return;
            }
            Some(Hit::Point(id) | Hit::Line(id) | Hit::Circle(id)) => Some(Sel::Ent(id)),
            Some(Hit::Face(f)) => Some(Sel::Face(f)),
            Some(Hit::Plane(id)) => Some(Sel::Plane(id)),
            Some(Hit::Derived(id)) => Some(Sel::Derived(id)),
            Some(Hit::Body(_) | Hit::Bodies(_) | Hit::Proj { .. } | Hit::Piece(_)) | None => None,
        };
        self.picked_constraint = None;
        match (sel, add) {
            (Some(s), true) => {
                if let Some(i) = self.selection.iter().position(|x| *x == s) {
                    self.selection.remove(i);
                } else {
                    self.selection.push(s);
                }
            }
            (Some(s), false) => self.selection = vec![s],
            (None, false) => self.selection.clear(),
            (None, true) => {}
        }
        // 点を選んだら、作図面をその点へ寄せる
        if let Some(Sel::Ent(id)) = self.selection.last()
            && let Some(p) = sol.points.get(id)
        {
            self.anchor = *p;
        }
    }
}

/// ビューを描き、操作を受け付ける。文書を変えれば新しいスケッチ。
pub(crate) fn view(
    ed: &mut Sketch3dEditor,
    ui: &mut egui::Ui,
    host: &mut EditorHost<'_>,
    entry: &FeatureEntry,
) -> Option<Sketch3d> {
    let Some((sk, sol)) = sketch_of(host, entry) else {
        let why = host
            .failure(entry.id)
            .unwrap_or_else(|| "表示できる 3D スケッチではありません".into());
        ui.colored_label(ui.visuals().error_fg_color, why);
        return None;
    };
    if ed.feature != Some(entry.id) {
        kmb_ui::FeatureEditor::reset(ed);
        ed.feature = Some(entry.id);
        ed.fitted = false;
        ed.anchor = [0.0; 3];
        ed.work = None;
        ed.target = None;
    }
    // 消えた作図面には描かない
    if ed.work.is_some_and(|w| sk.work_plane(w).is_none()) {
        ed.work = None;
    }
    if let Some(PlaneRef::Work { id }) = &ed.target
        && sk.work_plane(*id).is_none()
    {
        ed.target = None;
    }
    ed.frame = crate::sketch_plane(host, &sk.plane).unwrap_or(Plane::XY);
    // 3D のビューと同じく、表示のメニューの投影(平行・透視・自動)を効かせる
    ed.cam.perspective = host.projection.perspective_for(&ed.cam);
    // ブラウザの目で隠したボディは描かず、選べない
    let bodies = host.bodies_before(entry.id);
    ed.scene.update(host.kernel.as_ref(), &bodies);
    if bodies != ed.bodies {
        ed.body_items = bodies
            .iter()
            .flat_map(|(f, h)| body_items(host.kernel.as_ref(), *f, *h))
            .collect();
        ed.bodies = bodies;
    }
    // 初めは、スケッチとボディが収まるように合わせる
    let rect = ui.available_rect_before_wrap();
    if !ed.fitted {
        let (mut lo, mut hi) = if ed.scene.is_empty() {
            ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3])
        } else {
            ed.scene.bounds()
        };
        for p in sol.points.values() {
            let w = to_world(&ed.frame, *p).map(|v| v as f32);
            for i in 0..3 {
                lo[i] = lo[i].min(w[i]);
                hi[i] = hi[i].max(w[i]);
            }
        }
        // 何もないか点が 1 つだけなら、原点のまわりの 100 mm の四角
        if lo[0] > hi[0] || (0..3).all(|i| hi[i] - lo[i] < 1.0) {
            (lo, hi) = ([-50.0, -50.0, 0.0], [50.0, 50.0, 0.0]);
        }
        ed.cam.fit_box(lo, hi, rect);
        ed.fitted = true;
    }
    let look = kmb_ui::theme::look(ui.ctx());
    // ボディを選んだ・指した色は作業空間の色のまま(画面の差し色はスケッチ中に水色になる)
    let colors = view3d::Colors::from_look(&kmb_ui::theme::Look {
        sketch: false,
        ..look
    });
    let controls = view3d::Controls {
        no_guide_planes: true,
        ..Default::default()
    };
    let (ev, resp, rect) = view3d::view3d(
        ui,
        &mut ed.cam,
        &ed.scene,
        &[],
        &kmb_core::Overlay::default(),
        &[],
        controls,
        &colors,
        kmb_ui::Shading::ShadedEdges,
        view3d::Guides {
            grid: false,
            axes: true,
        },
    );
    ed.rect = rect;
    let values = values_of(host, &sk);
    let mut out: Option<Sketch3d> = None;

    // 点を引く: 押した所に点があれば、視線に垂直な平面の上で引く
    let pointer = ui.input(|i| i.pointer.clone());
    if ed.tool == Tool3::Select
        && ed.drag.is_none()
        && resp.drag_started_by(egui::PointerButton::Primary)
        && let Some(o) = pointer.press_origin()
    {
        let pr = ed.proj();
        let marks = labels(&sk, &sol, &pr);
        let hit = hit_at(ui, o, &sk, &sol, &pr, &marks, &ed.dim_rects);
        let mut grabbed = None;
        // 寸法の値の札を掴んだら、札を動かす(値は変えない)
        if let Some(Hit::Constraint(id)) = hit
            && let Some((_, r)) = ed.dim_rects.iter().find(|(c, _)| *c == id)
            && let Some(d) = dims(&sk, &sol, &pr).into_iter().find(|d| d.id == id)
        {
            let (_, ray) = pr.ray(o);
            grabbed = Some(LabelDrag {
                id,
                anchor: d.anchor,
                normal: geom::unit(ray),
                grab: r.center() - o,
                offset: None,
            });
        }
        if let Some(Hit::Point(id)) = hit
            && let Some(p) = sol.points.get(&id)
        {
            let (_, d) = pr.ray(o);
            ed.drag = Some(Drag {
                point: id,
                origin: *p,
                normal: geom::unit(d),
                preview: None,
            });
        }
        if let Some(g) = grabbed {
            ed.picked_constraint = Some(g.id);
            ed.label_drag = Some(g);
        }
    }
    let mut was_dragging = false;
    if let Some(drag) = &mut ed.drag {
        was_dragging = true;
        let pr = Proj {
            cam: &ed.cam,
            rect,
            frame: ed.frame,
            fmt: ed.fmt,
        };
        if let Some(pos) = pointer.latest_pos()
            && let Some(target) = pr.on_plane(pos, drag.origin, drag.normal)
            && let Ok(s) = solve3d_dragging(&sk, &values, drag.point, target)
        {
            drag.preview = Some(s);
        }
        if !pointer.primary_down() {
            if let Some(p) = &drag.preview {
                let mut s = sk.clone();
                s.adopt(p);
                out = Some(s);
            }
            ed.drag = None;
        }
    }
    if let Some(drag) = &mut ed.label_drag {
        was_dragging = true;
        let pr = Proj {
            cam: &ed.cam,
            rect,
            frame: ed.frame,
            fmt: ed.fmt,
        };
        if let Some(pos) = pointer.latest_pos()
            && let Some(target) = pr.on_plane(pos + drag.grab, drag.anchor, drag.normal)
        {
            drag.offset = Some(geom::sub(target, drag.anchor));
        }
        if !pointer.primary_down() {
            if let Some(d) = drag.offset {
                let mut s = sk.clone();
                if let Some(c) = s.constraints.iter_mut().find(|c| c.id == drag.id) {
                    c.label = Some(d);
                    out = Some(s);
                }
            }
            ed.label_drag = None;
        }
    }
    let shown = ed
        .drag
        .as_ref()
        .and_then(|d| d.preview.clone())
        .unwrap_or_else(|| sol.clone());
    // 投影・交差の道具の下絵(今の作図面へ写したボディの辺・断面。道具の間だけ描く。まとめてレビュー R1)
    let on_dp = ed.draw_plane(&sk, &shown);
    prepare_on_plane(ed, host.kernel.as_ref(), &on_dp);
    // このコマの視点(編集器を書き換えながら当たりを調べられるよう、写しで)
    let cam = ed.cam;
    let pr = Proj {
        cam: &cam,
        rect,
        frame: ed.frame,
        fmt: ed.fmt,
    };
    // 引いている札は、引いている所に描く
    let dragged_sk = ed.label_drag.as_ref().and_then(|d| {
        let mut s = sk.clone();
        s.constraints.iter_mut().find(|c| c.id == d.id)?.label = Some(d.offset?);
        Some(s)
    });
    let marks = labels(&sk, &shown, &pr);
    let dim_shapes = dims(dragged_sk.as_ref().unwrap_or(&sk), &shown, &pr);
    let hover_pos = ev.hover;
    let tags = plane_tags(ed, &sk, &shown);
    let hit_of = |ed: &Sketch3dEditor, p: Pos2| {
        match ed.tool {
            Tool3::Include => return include_hit(ed, p),
            Tool3::Project => return project_hit(ed, p),
            Tool3::Intersect => return piece_hit(ed, p, &pr),
            _ => {}
        }
        // 求めた線は面より手前(面の中の線を選べるように)
        let h = hit_at(ui, p, &sk, &shown, &pr, &marks, &ed.dim_rects);
        let h = match h {
            None | Some(Hit::Face(_)) => derived_hit(p, &shown, &pr).or(h),
            h => h,
        };
        h.or_else(|| {
            tags.iter()
                .find(|(_, at, name)| tag_rect(ui, *at, name).contains(p))
                .map(|(id, _, _)| Hit::Plane(*id))
        })
    };
    let hover = hover_pos
        .filter(|_| ed.drag.is_none() && ed.label_drag.is_none())
        .and_then(|p| hit_of(ed, p));
    // 投影・交差の道具: マウスの下を押すと写す線
    let lit = hover_lines(ed, host.kernel.as_ref(), &on_dp, hover.as_ref());
    if let Some(c) = ed.on_plane.as_mut() {
        c.hover = lit;
    }
    // 右クリック: 道具の中・何も選んでいない時・選んだ時で項目を分けたメニュー(menu.rs)。選ぶ道具で、
    // 選んでいない物や寸法・拘束の印の上を右クリックしたら、それを選んでから出す
    if let Some(p) = ev.context {
        ed.menu_at = Some(p);
        if ed.tool == Tool3::Select {
            let sel = match hit_of(ed, p) {
                Some(Hit::Constraint(c)) => {
                    ed.selection.clear();
                    ed.picked_constraint = Some(c);
                    None
                }
                Some(Hit::Point(id) | Hit::Line(id) | Hit::Circle(id)) => Some(Sel::Ent(id)),
                Some(Hit::Face(f)) => Some(Sel::Face(f)),
                Some(Hit::Plane(id)) => Some(Sel::Plane(id)),
                Some(Hit::Derived(id)) => Some(Sel::Derived(id)),
                Some(Hit::Body(_) | Hit::Bodies(_) | Hit::Proj { .. } | Hit::Piece(_)) | None => {
                    None
                }
            };
            if let Some(s) = sel
                && !ed.selection.contains(&s)
            {
                ed.selection = vec![s];
                ed.picked_constraint = None;
            }
        }
    }
    let mut ask = None;
    resp.context_menu(|ui| ask = ed.menu_ui(ui, &sk, &shown));

    // キー(ほかの欄に打っている時は使わない)。Tab は egui が欄から欄へ移るのにも使うので、
    // 前のコマの時点で欄に打っていなければ作図面を替え、欄へ移った分は戻す
    if !ed.typing_before && !ed.editing() {
        // 欄が先に Tab を使っていることがあるので、元の出来事から読む
        let tab = ui.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Key {
                    key: egui::Key::Tab,
                    pressed: true,
                    modifiers,
                    ..
                } if !modifiers.command && !modifiers.alt => {
                    Some(if modifiers.shift { 2 } else { 1 })
                }
                _ => None,
            })
        });
        if let Some(step) = tab {
            // 座標の平面の 3 つと、作った作図面を順に
            let axes = [AxisPlane::Xy, AxisPlane::Yz, AxisPlane::Zx];
            let count = 3 + sk.planes.len();
            let i = match ed.work {
                Some(w) => 3 + sk.planes.iter().position(|x| x.id == w).unwrap_or(0),
                None => axes.iter().position(|p| *p == ed.plane).unwrap_or(0),
            };
            let j = (i + if step == 1 { 1 } else { count - 1 }) % count;
            if j < 3 {
                ed.set_plane(axes[j]);
            } else {
                ed.set_work(sk.planes[j - 3].id, &sk, &shown);
            }
            ui.memory_mut(|m| {
                if let Some(id) = m.focused() {
                    m.surrender_focus(id);
                }
            });
        }
    }
    let typing = ui.ctx().egui_wants_keyboard_input();
    ed.typing_before = typing;
    // Esc は 1 段ずつ戻す: 値の欄 → 引いている点 → 打った長さ → 描きかけ → 道具 → 選び
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        let inner = ed.edit.take().is_some()
            || ed.plane_edit.take().is_some()
            || ed.drag.take().is_some()
            || ed.label_drag.take().is_some()
            || !std::mem::take(&mut ed.typed).is_empty()
            || ed.start.take().is_some();
        if was_dragging && ed.drag.is_none() && ed.label_drag.is_none() {
            out = None;
        }
        if !inner {
            if ed.tool != Tool3::Select {
                ed.finish_tool();
            } else {
                ed.selection.clear();
                ed.picked_constraint = None;
            }
        }
    }
    if !typing && !ed.editing() {
        let (keys, mods) = ui.input(|i| {
            let keys: Vec<egui::Event> = i.events.clone();
            (keys, i.modifiers)
        });
        for e in keys {
            match e {
                // Delete・Enter・Backspace は決まった働き。ほかは画面のキーの表(設定で付け替えられる)
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } if modifiers.is_none()
                    && matches!(
                        key,
                        egui::Key::Delete | egui::Key::Enter | egui::Key::Backspace
                    ) =>
                {
                    match key {
                        egui::Key::Delete => {
                            out = ed.delete_selected(&sk).or(out);
                        }
                        egui::Key::Enter if !ed.typed.is_empty() => {
                            out = ed.typed_line(host, hover_pos, &sk, &shown).or(out);
                        }
                        // 何も打たずに Enter: つなぐのを終える・道具を終える
                        egui::Key::Enter => ed.enter(),
                        _ => {
                            ed.typed.pop();
                        }
                    }
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    use kmb_ui::keys::is;
                    if is("sketch_construction", key, modifiers) {
                        out = ed.toggle_construction(&sk).or(out);
                    } else if is("sketch_dimension", key, modifiers) {
                        out = ed.apply_dimension("寸法", host, entry).or(out);
                    } else if !ed.typed.is_empty() {
                        // 長さを打っている間は道具を替えない
                    } else if is("search_here", key, modifiers) {
                        // カーソルの所にコマンド検索(3D のビューと 2D のスケッチと同じ)
                        *host.then = Some((kmb_ui::APP_SEARCH.into(), serde_json::json!({})));
                    } else if let Some(t) = super::DRAW3.iter().find(|d| is(d.3, key, modifiers)) {
                        ed.set_tool(t.0);
                    }
                }
                egui::Event::Text(t)
                    if ed.tool == Tool3::Line
                        && ed.start.is_some()
                        && !mods.command
                        && t.chars()
                            .all(|c| c.is_ascii_digit() || c == '.' || c == '-')
                        && (!ed.typed.is_empty() || t.chars().any(|c| c.is_ascii_digit())) =>
                {
                    ed.typed.push_str(&t);
                }
                _ => {}
            }
        }
    }

    // クリック
    // 押した時の修飾キー(ボタンの出来事にも付いてくる)
    let mods = ui.input(|i| {
        i.events.iter().fold(i.modifiers, |m, e| match e {
            egui::Event::PointerButton { modifiers, .. } => m | *modifiers,
            _ => m,
        })
    });
    // 長押しの一覧はないので、長く押してもクリックとして扱う
    let click = (resp.clicked_by(egui::PointerButton::Primary) && !mods.alt)
        .then(|| resp.interact_pointer_pos())
        .flatten();
    if let Some(pos) = click
        && !was_dragging
        && out.is_none()
    {
        if ed.tool == Tool3::Select {
            ed.click_select(hover.clone(), mods.shift || mods.command, &shown);
        } else if ed.tool == Tool3::Include {
            let picked: Vec<usize> = match &hover {
                Some(Hit::Body(i)) => vec![*i],
                Some(Hit::Bodies(v)) => v.clone(),
                _ => Vec::new(),
            };
            let items: Vec<BodyItem> = picked
                .iter()
                .filter_map(|i| ed.body_items.get(*i).cloned())
                .collect();
            if !items.is_empty() {
                out = ed.include_all(&sk, &items);
            }
        } else if ed.tool == Tool3::Project {
            if let Some(Hit::Proj { body, items }) = &hover
                && let Some(g) = ed.geoms.get(*body)
            {
                let f = g.feature;
                let what = match ed.project_pick {
                    ProjectPick::Parts => ProjectWhat::Parts {
                        sources: items.iter().map(|i| g.named[*i].0.clone()).collect(),
                    },
                    ProjectPick::Silhouette => ProjectWhat::Silhouette,
                    ProjectPick::Edges => ProjectWhat::Edges,
                };
                if ed.on_plane.as_ref().is_some_and(|c| c.hover.is_empty()) {
                    ed.note = Some(
                        "今の作図面へ写せる線がありません(作図面に垂直な辺は点につぶれます)".into(),
                    );
                } else {
                    out = ed.add_onto(&sk, |plane| DerivedDef::Project {
                        plane,
                        what,
                        bodies: vec![f],
                    });
                }
            }
        } else if ed.tool == Tool3::Intersect {
            let picked = match &hover {
                Some(Hit::Piece(k)) => ed
                    .on_plane
                    .as_ref()
                    .and_then(|c| c.pieces.as_ref())
                    .and_then(|p| p.get(*k))
                    .and_then(|(gi, face, i, _)| {
                        Some((ed.geoms.get(*gi)?.feature, piece(face, *i)))
                    }),
                _ => None,
            };
            if let Some((f, p)) = picked {
                let pieces = if ed.intersect_body { vec![] } else { vec![p] };
                out = ed.add_onto(&sk, |plane| DerivedDef::Section {
                    plane,
                    bodies: vec![f],
                    pieces,
                });
            }
        } else if !ed.typed.is_empty() && ed.start.is_some() {
            // 長さを打ったままクリック: Enter と同じく、クリックした向きへ打った長さで引く
            out = ed.typed_line(host, Some(pos), &sk, &shown);
        } else if let Some(s) = ed.click_draw(pos, &sk, &shown) {
            out = Some(s);
        }
    }
    if let Some(a) = ask
        && out.is_none()
    {
        let at = ed.menu_at;
        out = ed.apply_menu(a, host, entry, at);
    }
    // 囲んで選ぶ。左から右へ囲むと全部入った物、右から左へ囲むとかかった物(U30。2D と同じ)
    if let Some(area) = ev.boxed
        && !was_dragging
        && ed.tool == Tool3::Select
    {
        let pr = ed.proj();
        let crossing = ev.crossing;
        // 要素の画面の上の折れ線(点は 1 点、円は 64 分割)
        let screen_line = |e: &Entity3d| -> Vec<Pos2> {
            let at = |id: &EntityId| shown.points.get(id).map(|q| pr.at(*q));
            match e {
                Entity3d::Point { id, .. } => at(id).into_iter().collect(),
                Entity3d::Line { a, b, .. } => [at(a), at(b)].into_iter().flatten().collect(),
                Entity3d::Circle {
                    id, center, normal, ..
                } => {
                    let (Some(c), Some(r)) = (shown.points.get(center), shown.radii.get(id)) else {
                        return Vec::new();
                    };
                    let (u, v) = kmb_sketch_3d::basis(*shown.normals.get(id).unwrap_or(normal));
                    (0..=64)
                        .map(|k| {
                            let (sn, co) = (std::f64::consts::TAU * k as f64 / 64.0).sin_cos();
                            pr.at([0, 1, 2].map(|i| c[i] + u[i] * r * co + v[i] * r * sn))
                        })
                        .collect()
                }
            }
        };
        let picked: Vec<Sel> = sk
            .entities
            .iter()
            .filter(|e| {
                let line = screen_line(e);
                if line.is_empty() {
                    false
                } else if crossing {
                    line.iter().any(|p| area.contains(*p))
                        || line
                            .windows(2)
                            .any(|w| view3d::segment_touches(area, w[0], w[1]))
                } else {
                    line.iter().all(|p| area.contains(*p))
                }
            })
            .map(|e| Sel::Ent(e.id()))
            .collect();
        if !(mods.shift || mods.command) {
            ed.selection.clear();
        }
        for s in picked {
            if !ed.selection.contains(&s) {
                ed.selection.push(s);
            }
        }
    }
    // 作図面の札をダブルクリックで、作図面の値(ずらす距離・傾き)を直す
    if resp.double_clicked()
        && let Some(Hit::Plane(w)) = &hover
        && let Some(t) = sk
            .work_plane(*w)
            .and_then(|x| super::plane_value_text(&ed.fmt, &x.def))
    {
        ed.plane_edit = Some((*w, t));
    }
    // 寸法の値をダブルクリックで直す(カーソルを乗せるだけでは直さない)
    if resp.double_clicked()
        && let Some(Hit::Constraint(c)) = &hover
        && let Some(con) = sk.constraint(*c)
        && !con.reference
        && let Some(v) = con.kind.value()
    {
        ed.edit = Some((*c, v.to_string()));
    }

    ed.dim_rects = draw(
        ed,
        ui,
        &sk,
        &shown,
        &marks,
        &dim_shapes,
        hover.as_ref(),
        hover_pos,
        &look,
        entry,
    );
    // 右上: 全体を表示と斜めのボタン、その下に視点の軸の玉(3D のビューと同じ。
    // 視点の立方体は作らない。docs/避ける形.md)
    let bar = Rect::from_min_size(
        rect.right_top() + Vec2::new(-330.0, 6.0),
        Vec2::new(322.0, 24.0),
    );
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(bar)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
        |ui| {
            if ui.small_button("全体を表示").clicked() {
                ed.fitted = false;
            }
            // 同じボタンをもう一度押すと反対側から見る(3D のビューの等角のボタンと同じ)
            let b = kmb_ui::view3d::VIEW_BUTTONS[3];
            let name = if ed.cam.view_side(&b) == Some(true) {
                "斜め後ろから"
            } else {
                "斜めから"
            };
            if ui.small_button(name).clicked() {
                ed.cam.press_view(&b);
            }
        },
    );
    let gizmo =
        rect.right_top() + Vec2::new(-10.0 - view3d::GIZMO_RADIUS, 36.0 + view3d::GIZMO_RADIUS);
    view3d::axis_gizmo(ui, &mut ed.cam, gizmo, look.pal.panel);

    // 寸法の値を打つ欄(置いた直後と、ダブルクリックした時)
    if let Some((cid, mut text)) = ed.edit.clone() {
        let at = ed
            .dim_rects
            .iter()
            .find(|(c, _)| *c == cid)
            .map(|(_, r)| r.center())
            .or_else(|| {
                dim_shapes
                    .iter()
                    .find(|d| d.id == cid)
                    .map(|d| pr.at(d.anchor))
            })
            .unwrap_or(rect.center());
        let id = kmb_ui::doc_id(ui.ctx(), ("dim3d_edit", cid.0));
        let mut commit = false;
        egui::Area::new(id.with("area"))
            .fixed_pos(at - Vec2::new(50.0, 12.0))
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut text)
                            .id(id)
                            .desired_width(100.0),
                    );
                    // 開いた時は値を全部選んでおく(打てば置き換わる)
                    if !r.has_focus() && !ui.data(|d| d.get_temp::<bool>(id).unwrap_or(false)) {
                        r.request_focus();
                        ui.data_mut(|d| d.insert_temp(id, true));
                        let mut st = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
                        let all = egui::text::CCursorRange::two(
                            egui::text::CCursor::new(0),
                            egui::text::CCursor::new(text.chars().count()),
                        );
                        st.cursor.set_char_range(Some(all));
                        st.store(ui.ctx(), id);
                    }
                    // Enter か欄の外の左クリックで決め、Esc でやめる(2D の欄と同じ)
                    match crate::view::leave_field(ui, &r) {
                        crate::view::Leave::Stay => {}
                        crate::view::Leave::Keep => r.request_focus(),
                        leave => {
                            ui.data_mut(|d| d.remove::<bool>(id));
                            commit = leave == crate::view::Leave::Commit;
                            if !commit {
                                ed.edit = None;
                            }
                        }
                    }
                });
            });
        if ed.edit.is_some() {
            ed.edit = Some((cid, text.clone()));
        }
        if commit {
            ed.edit = None;
            out = ed.set_dimension(host, &sk, cid, &text).or(out);
        }
    }
    // 作図面の値を打つ欄(作った直後。ずらす距離か傾き)
    if let Some((wid, mut text)) = ed.plane_edit.clone() {
        let at = tags
            .iter()
            .find(|t| t.0 == wid)
            .map_or(rect.center(), |t| t.1 + Vec2::new(0.0, 8.0));
        let id = kmb_ui::doc_id(ui.ctx(), ("wplane3d_edit", wid));
        let mut commit = false;
        egui::Area::new(id.with("area"))
            .fixed_pos(at)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let what = match sk.work_plane(wid).map(|w| &w.def) {
                            Some(kmb_sketch_3d::PlaneDef::Tilted { .. }) => "傾き",
                            _ => "ずらす",
                        };
                        ui.label(what);
                        let r = ui.add(
                            egui::TextEdit::singleline(&mut text)
                                .id(id)
                                .desired_width(90.0),
                        );
                        if !r.has_focus() && !ui.data(|d| d.get_temp::<bool>(id).unwrap_or(false)) {
                            r.request_focus();
                            ui.data_mut(|d| d.insert_temp(id, true));
                            let mut st =
                                egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
                            let all = egui::text::CCursorRange::two(
                                egui::text::CCursor::new(0),
                                egui::text::CCursor::new(text.chars().count()),
                            );
                            st.cursor.set_char_range(Some(all));
                            st.store(ui.ctx(), id);
                        }
                        match crate::view::leave_field(ui, &r) {
                            crate::view::Leave::Stay => {}
                            crate::view::Leave::Keep => r.request_focus(),
                            leave => {
                                ui.data_mut(|d| d.remove::<bool>(id));
                                commit = leave == crate::view::Leave::Commit;
                                if !commit {
                                    ed.plane_edit = None;
                                }
                            }
                        }
                    });
                });
            });
        if ed.plane_edit.is_some() {
            ed.plane_edit = Some((wid, text.clone()));
        }
        if commit {
            ed.plane_edit = None;
            out = ed.set_plane_value(host, &sk, wid, &text).or(out);
        }
    }
    out
}
