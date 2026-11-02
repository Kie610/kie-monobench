//! 描いている途中の吸い付きと推測。
//!
//! - 吸い付き: 曲線の上、交点、四分点(交点と四分点は snap.rs が仮の点として足す)、通った点と
//!   縦・横に並ぶ位置(破線の案内を出す)。
//! - 推測で付く拘束: 曲線の上に置いた点には線上・円周上、線分には水平・垂直・平行・直角、
//!   円弧の端から引いた線分には接する。Ctrl を押している間は吸い付きも推測もしない。
//!
//! どれも形を作った後の拘束を足すだけで、解き方(ソルバ)には手を入れない。

use crate::snap::Kind;
use kmb_sketch::{ConstraintKind, Entity, EntityId, Sketch, Solution};

/// 推測で向きをそろえる角度の幅(度)。
const ANGLE_TOL: f64 = 3.0;
/// 並びの案内に使う、通った点の数。
const TRACKED: usize = 4;
/// 平行・直角の相手にする、触れた線分の数。
const RECENT: usize = 2;

/// 描いている間に覚えておくもの(文書には保存しない)。
#[derive(Default)]
pub(crate) struct State {
    /// カーソルが通った点(縦・横の並びの案内に使う)
    pub tracked: Vec<EntityId>,
    /// カーソルが触れた線分(平行・直角の相手にする)
    pub recent: Vec<EntityId>,
    /// 曲線の上に置いた位置と、その曲線(できた点に線上・円周上を付ける)
    pub placed: Vec<([f64; 2], EntityId)>,
    /// 形が同じ間の交点。カーソルを動かすだけでは計算し直さない。
    pub crossings: CrossingCache,
}

impl State {
    pub(crate) fn clear(&mut self) {
        *self = State::default();
    }

    /// カーソルの下の点と線分を覚える。
    pub(crate) fn notice(&mut self, sketch: &Sketch, sol: &Solution, w: [f64; 2], tol: f64) {
        if let Some(h) = sketch.nearest_point(sol, w[0], w[1], tol) {
            self.tracked.retain(|p| *p != h.entity);
            self.tracked.push(h.entity);
            if self.tracked.len() > TRACKED {
                self.tracked.remove(0);
            }
        } else if let Some(h) = sketch.hit_test(sol, w[0], w[1], tol)
            && matches!(sketch.entity(h.entity), Some(Entity::Line { .. }))
        {
            self.recent.retain(|l| *l != h.entity);
            self.recent.push(h.entity);
            if self.recent.len() > RECENT {
                self.recent.remove(0);
            }
        }
    }
}

/// 曲線の形(解けた位置)。
#[derive(Clone, Copy, Debug)]
pub(crate) enum Shape {
    Seg([f64; 2], [f64; 2]),
    /// 中心、半径、始まりの角度、反時計回りの広がり(円なら TAU)
    Arc([f64; 2], f64, f64, f64),
}

const TAU: f64 = std::f64::consts::TAU;

fn angle_of(c: [f64; 2], p: [f64; 2]) -> f64 {
    (p[1] - c[1]).atan2(p[0] - c[0]).rem_euclid(TAU)
}

pub(crate) fn shape(sketch: &Sketch, sol: &Solution, id: EntityId) -> Option<Shape> {
    let pt = |p: &EntityId| sol.points.get(p).copied();
    Some(match sketch.entity(id)? {
        Entity::Line { a, b, .. } => Shape::Seg(pt(a)?, pt(b)?),
        Entity::Circle { center, .. } => Shape::Arc(pt(center)?, *sol.radii.get(&id)?, 0.0, TAU),
        Entity::Arc {
            center, start, end, ..
        } => {
            let (c, s, e) = (pt(center)?, pt(start)?, pt(end)?);
            let a0 = angle_of(c, s);
            let mut span = (angle_of(c, e) - a0).rem_euclid(TAU);
            if span < 1e-9 {
                span = TAU;
            }
            Shape::Arc(c, (s[0] - c[0]).hypot(s[1] - c[1]), a0, span)
        }
        // 自由曲線はスケッチの曲線の式で最近点と交点を求める。
        Entity::Point { .. }
        | Entity::Ellipse { .. }
        | Entity::Spline { .. }
        | Entity::Conic { .. } => return None,
    })
}

impl Shape {
    fn contains(&self, p: [f64; 2]) -> bool {
        match *self {
            Shape::Seg(a, b) => {
                let d = [b[0] - a[0], b[1] - a[1]];
                let t = ((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1])
                    / (d[0] * d[0] + d[1] * d[1]).max(1e-12);
                (-1e-9..=1.0 + 1e-9).contains(&t)
            }
            Shape::Arc(c, _, a0, span) => {
                span >= TAU - 1e-9 || (angle_of(c, p) - a0).rem_euclid(TAU) <= span + 1e-9
            }
        }
    }

    /// `w` に最も近い曲線の上の点(円弧の外なら近い端)。
    pub(crate) fn nearest(&self, w: [f64; 2]) -> [f64; 2] {
        match *self {
            Shape::Seg(a, b) => {
                let d = [b[0] - a[0], b[1] - a[1]];
                let t = (((w[0] - a[0]) * d[0] + (w[1] - a[1]) * d[1])
                    / (d[0] * d[0] + d[1] * d[1]).max(1e-12))
                .clamp(0.0, 1.0);
                [a[0] + d[0] * t, a[1] + d[1] * t]
            }
            Shape::Arc(c, r, a0, span) => {
                let a = angle_of(c, w);
                let a = if self.contains(w) {
                    a
                } else {
                    // 近い方の端
                    let to0 = (a0 - a).rem_euclid(TAU).min((a - a0).rem_euclid(TAU));
                    let a1 = a0 + span;
                    let to1 = (a1 - a).rem_euclid(TAU).min((a - a1).rem_euclid(TAU));
                    if to0 <= to1 { a0 } else { a1 }
                };
                [c[0] + r * a.cos(), c[1] + r * a.sin()]
            }
        }
    }
}

/// 2 つの曲線の交点。
fn intersect(a: &Shape, b: &Shape) -> Vec<[f64; 2]> {
    let sub = |p: [f64; 2], q: [f64; 2]| [p[0] - q[0], p[1] - q[1]];
    let cross = |p: [f64; 2], q: [f64; 2]| p[0] * q[1] - p[1] * q[0];
    let dot = |p: [f64; 2], q: [f64; 2]| p[0] * q[0] + p[1] * q[1];
    let raw = match (*a, *b) {
        (Shape::Seg(p, q), Shape::Seg(r, s)) => {
            let (d, e) = (sub(q, p), sub(s, r));
            let den = cross(d, e);
            if den.abs() < 1e-12 {
                vec![]
            } else {
                let t = cross(sub(r, p), e) / den;
                vec![[p[0] + d[0] * t, p[1] + d[1] * t]]
            }
        }
        (Shape::Seg(p, q), Shape::Arc(c, r, ..)) | (Shape::Arc(c, r, ..), Shape::Seg(p, q)) => {
            let d = sub(q, p);
            let f = sub(p, c);
            let (aa, bb, cc) = (dot(d, d), 2.0 * dot(f, d), dot(f, f) - r * r);
            let disc = bb * bb - 4.0 * aa * cc;
            if aa < 1e-12 || disc < 0.0 {
                vec![]
            } else {
                let sq = disc.sqrt();
                [(-bb - sq) / (2.0 * aa), (-bb + sq) / (2.0 * aa)]
                    .into_iter()
                    .map(|t| [p[0] + d[0] * t, p[1] + d[1] * t])
                    .collect()
            }
        }
        (Shape::Arc(c1, r1, ..), Shape::Arc(c2, r2, ..)) => {
            let d = sub(c2, c1);
            let dl = d[0].hypot(d[1]);
            if dl < 1e-9 || dl > r1 + r2 || dl < (r1 - r2).abs() {
                vec![]
            } else {
                let x = (dl * dl + r1 * r1 - r2 * r2) / (2.0 * dl);
                let h = (r1 * r1 - x * x).max(0.0).sqrt();
                let u = [d[0] / dl, d[1] / dl];
                let m = [c1[0] + u[0] * x, c1[1] + u[1] * x];
                let n = [-u[1], u[0]];
                vec![
                    [m[0] + n[0] * h, m[1] + n[1] * h],
                    [m[0] - n[0] * h, m[1] - n[1] * h],
                ]
            }
        }
    };
    raw.into_iter()
        .filter(|p| a.contains(*p) && b.contains(*p))
        .collect()
}

/// 吸い付く点 1 つ: 位置と種類、乗っている曲線。
pub(crate) type Crossing = (([f64; 2], Kind), Vec<EntityId>);

/// 解いた形で決まる吸い付く先。座標・半径・曲線の定義が変わった時に作り直す。
#[derive(Default)]
pub(crate) struct CrossingCache {
    ready: bool,
    entities: Vec<Entity>,
    points: std::collections::BTreeMap<EntityId, [f64; 2]>,
    radii: std::collections::BTreeMap<EntityId, f64>,
    found: Vec<Crossing>,
}

impl CrossingCache {
    pub(crate) fn get(&mut self, sketch: &Sketch, sol: &Solution) -> &[Crossing] {
        if !self.ready
            || self.entities != sketch.entities
            || self.points != sol.points
            || self.radii != sol.radii
        {
            self.found = crossings(sketch, sol);
            self.entities.clone_from(&sketch.entities);
            self.points.clone_from(&sol.points);
            self.radii.clone_from(&sol.radii);
            self.ready = true;
        }
        &self.found
    }
}

/// 曲線どうしの交点と、円・円弧の四分点。
pub(crate) fn crossings(sketch: &Sketch, sol: &Solution) -> Vec<Crossing> {
    let curves: Vec<(EntityId, Shape)> = sketch
        .entities
        .iter()
        .filter_map(|e| Some((e.id(), shape(sketch, sol, e.id())?)))
        .collect();
    let mut out: Vec<Crossing> = vec![];
    let mut push = |p: [f64; 2], k: Kind, on: Vec<EntityId>| {
        if !out
            .iter()
            .any(|((q, _), _)| (q[0] - p[0]).hypot(q[1] - p[1]) < 1e-6)
        {
            out.push(((p, k), on));
        }
    };
    for (i, (a, sa)) in curves.iter().enumerate() {
        for (b, sb) in &curves[i + 1..] {
            for p in intersect(sa, sb) {
                push(p, Kind::Intersection, vec![*a, *b]);
            }
        }
    }
    for (p, a, b) in sketch.free_curve_crossings(sol) {
        push(p, Kind::Intersection, vec![a, b]);
    }
    for (id, s) in &curves {
        if let Shape::Arc(c, r, ..) = s {
            for k in 0..4 {
                let a = k as f64 * TAU / 4.0;
                let p = [c[0] + r * a.cos(), c[1] + r * a.sin()];
                if s.contains(p) {
                    push(p, Kind::Quadrant, vec![*id]);
                }
            }
        }
    }
    out
}

/// カーソルを寄せた結果。
#[derive(Clone, Debug, Default)]
pub(crate) struct Snapped {
    pub pos: [f64; 2],
    /// 曲線の上に寄せた(作った点に線上・円周上を付ける)
    pub on: Option<EntityId>,
    /// カーソルの脇に出す札
    pub label: Option<&'static str>,
    /// 並びの案内の破線(点から寄せた位置へ)
    pub guides: Vec<([f64; 2], [f64; 2])>,
}

/// 描く道具のカーソルを寄せる。既にある点(仮の点も)の近くなら寄せない(点に吸い付く)。
/// 次に曲線の上、最後に通った点と縦・横に並ぶ位置。
pub(crate) fn snap_cursor(
    sketch: &Sketch,
    sol: &Solution,
    w: [f64; 2],
    tol: f64,
    state: &State,
) -> Snapped {
    let plain = Snapped {
        pos: w,
        ..Snapped::default()
    };
    if sketch.nearest_point(sol, w[0], w[1], tol).is_some() {
        return plain;
    }
    if let Some(h) = sketch.hit_test(sol, w[0], w[1], tol) {
        let found = if let Some(s) = shape(sketch, sol, h.entity) {
            Some((
                s.nearest(w),
                match s {
                    Shape::Seg(..) => "線上",
                    Shape::Arc(..) => "円周上",
                },
            ))
        } else {
            sketch
                .curve_closest(sol, h.entity, w)
                .filter(|(_, _, distance)| *distance <= tol)
                .map(|(point, _, _)| (point, "線上"))
        };
        if let Some((pos, label)) = found {
            return Snapped {
                pos,
                on: Some(h.entity),
                label: Some(label),
                guides: vec![],
            };
        }
    }
    let mut out = plain;
    // 縦に並ぶ(x がそろう)点と、横に並ぶ(y がそろう)点。近い方を 1 つずつ
    let near = |axis: usize| {
        state
            .tracked
            .iter()
            .filter_map(|id| sol.points.get(id).copied())
            .map(|p| ((p[axis] - w[axis]).abs(), p))
            .filter(|(d, _)| *d <= tol)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, p)| p)
    };
    if let Some(p) = near(0) {
        out.pos[0] = p[0];
        out.label = Some("縦に並ぶ");
        out.guides.push((p, out.pos));
    }
    if let Some(p) = near(1) {
        out.pos[1] = p[1];
        out.label = Some(if out.label.is_some() {
            "縦横に並ぶ"
        } else {
            "横に並ぶ"
        });
        out.guides.push((p, out.pos));
    }
    // 案内の始まりは寄せた後の位置へ(縦横の両方に寄せた時に、線が斜めにならないよう)
    for g in &mut out.guides {
        g.1 = out.pos;
    }
    out
}

/// 線分の推測で付ける向きの拘束。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum LineRel {
    Horizontal,
    Vertical,
    Parallel(EntityId),
    Perpendicular(EntityId),
    /// 始点で終わる円弧に接する
    Tangent(EntityId),
}

impl LineRel {
    pub(crate) fn label(self) -> &'static str {
        match self {
            LineRel::Horizontal => "水平",
            LineRel::Vertical => "垂直",
            LineRel::Parallel(_) => "平行",
            LineRel::Perpendicular(_) => "直交",
            LineRel::Tangent(_) => "接線",
        }
    }

    pub(crate) fn constraint(self, line: EntityId) -> ConstraintKind {
        match self {
            LineRel::Horizontal => ConstraintKind::Horizontal { line },
            LineRel::Vertical => ConstraintKind::Vertical { line },
            LineRel::Parallel(a) => ConstraintKind::Parallel { a, b: line },
            LineRel::Perpendicular(a) => ConstraintKind::Perpendicular { a, b: line },
            LineRel::Tangent(arc) => ConstraintKind::Tangent { line, circle: arc },
        }
    }
}

/// 向き(ラジアン)の差を -π..π に。
fn diff(a: f64, b: f64) -> f64 {
    (a - b + std::f64::consts::PI).rem_euclid(TAU) - std::f64::consts::PI
}

/// 線分を `start`(始点の点があればその番号)から `end` へ引く時に推測する向きと、その向きに
/// そろえた終わりの位置(長さは保つ)。接する・水平・垂直・平行・直角の順に調べる。
/// `others` は平行・直角の相手の候補(つなげて描いた前の線分と、触れた線分)。
pub(crate) fn infer_line(
    sketch: &Sketch,
    sol: &Solution,
    start: (Option<EntityId>, [f64; 2]),
    end: [f64; 2],
    others: &[EntityId],
) -> Option<(LineRel, [f64; 2])> {
    let (s, p) = start;
    let d = [end[0] - p[0], end[1] - p[1]];
    let len = d[0].hypot(d[1]);
    if len < 1e-9 {
        return None;
    }
    let dir = d[1].atan2(d[0]);
    let tol = ANGLE_TOL.to_radians();
    let at = |a: f64| [p[0] + len * a.cos(), p[1] + len * a.sin()];
    // 向き `a` か、その逆向きに近ければ、近い方の向き
    let near = |a: f64| -> Option<f64> {
        [a, a + std::f64::consts::PI]
            .into_iter()
            .find(|x| diff(dir, *x).abs() < tol)
    };
    // 円弧の端から引くなら、その円弧に接する向き
    if let Some(s) = s {
        for e in &sketch.entities {
            if let Entity::Arc {
                id,
                center,
                start: a0,
                end: a1,
                ..
            } = e
                && (*a0 == s || *a1 == s)
                && let Some(c) = sol.points.get(center)
            {
                let radial = (p[1] - c[1]).atan2(p[0] - c[0]);
                if let Some(a) = near(radial + std::f64::consts::FRAC_PI_2) {
                    return Some((LineRel::Tangent(*id), at(a)));
                }
            }
        }
    }
    if let Some(a) = near(0.0) {
        return Some((LineRel::Horizontal, at(a)));
    }
    if let Some(a) = near(std::f64::consts::FRAC_PI_2) {
        return Some((LineRel::Vertical, at(a)));
    }
    for o in others.iter().rev() {
        let Some(Entity::Line { a, b, .. }) = sketch.entity(*o) else {
            continue;
        };
        let (Some(q0), Some(q1)) = (sol.points.get(a), sol.points.get(b)) else {
            continue;
        };
        let od = (q1[1] - q0[1]).atan2(q1[0] - q0[0]);
        // 相手が水平・垂直なら、上で水平・垂直として扱った
        if let Some(a) = near(od) {
            return Some((LineRel::Parallel(*o), at(a)));
        }
        if let Some(a) = near(od + std::f64::consts::FRAC_PI_2) {
            return Some((LineRel::Perpendicular(*o), at(a)));
        }
    }
    None
}

/// 描く道具のカーソルを寄せる(`on` が偽なら寄せない)。線分の道具で始点を決めた後は、
/// 向き(接する・水平・垂直・平行・直角)も推測してそろえる。寄せた位置と、寄せ方、推測した向きを返す。
pub(crate) fn steer(
    editor: &mut crate::view::Editor,
    sketch: &Sketch,
    sol: &Solution,
    cursor: Option<[f64; 2]>,
    tol: f64,
    on: bool,
) -> (Option<[f64; 2]>, Option<Snapped>, Option<LineRel>) {
    let Some(w) = cursor else {
        return (None, None, None);
    };
    if !on {
        return (Some(w), None, None);
    }
    editor.infer.notice(sketch, sol, w, tol);
    let mut sn = snap_cursor(sketch, sol, w, tol, &editor.infer);
    let mut rel = None;
    if editor.tool == crate::view::Tool::Line
        && let Some(start) = editor.pending
        && sn.on.is_none()
        && sketch.nearest_point(sol, w[0], w[1], tol).is_none()
    {
        let mut others: Vec<EntityId> = start
            .point
            .and_then(|p| crate::view::line_ending_at(sketch, p))
            .into_iter()
            .collect();
        others.extend(editor.infer.recent.iter().copied());
        if let Some((r, e)) = infer_line(sketch, sol, (start.point, start.pos), sn.pos, &others) {
            sn.pos = e;
            sn.guides.clear();
            sn.label = Some(r.label());
            rel = Some(r);
        }
    }
    (Some(sn.pos), Some(sn), rel)
}

/// 道具が作ったスケッチ `made` で、曲線の上に置いた位置にできた新しい点へ、線上・円周上の拘束を
/// 足す。`before` は道具に渡したスケッチ(これに無い点が新しい点)。既にほかの拘束で決まって
/// いれば足さない。
pub(crate) fn attach_placed(
    before: &Sketch,
    made: &mut Sketch,
    placed: &[([f64; 2], EntityId)],
    values: Option<&crate::view::DimValues>,
) {
    let new_points: Vec<(EntityId, [f64; 2])> = made
        .entities
        .iter()
        .filter(|e| before.entity(e.id()).is_none())
        .filter_map(|e| match e {
            Entity::Point { id, x, y } => Some((*id, [*x, *y])),
            _ => None,
        })
        .collect();
    for (pos, curve) in placed {
        let Some((id, _)) = new_points
            .iter()
            .find(|(_, p)| (p[0] - pos[0]).hypot(p[1] - pos[1]) < 1e-9)
        else {
            continue;
        };
        // 自分の端に乗る(曲線が新しくできた点を使う)なら付けない
        if made
            .entity(*curve)
            .is_some_and(|e| jc_sketch_uses(e).contains(id))
        {
            continue;
        }
        let k = kmb_sketch::on_curve(made, *id, *curve);
        let decided = values.is_some_and(|v| kmb_sketch::redundant_with(made, v, &k).is_some());
        if !decided {
            made.add_constraint(k);
        }
    }
}

fn jc_sketch_uses(e: &Entity) -> Vec<EntityId> {
    e.points()
}

/// 線分 `line`(`a` から `b`)に、推測した向きの拘束を足す(既に決まっていれば足さない)。
/// 足した拘束の名前を返す。
pub(crate) fn add_line_rel(
    s: &mut Sketch,
    sol: &Solution,
    line: EntityId,
    from: [f64; 2],
    to: [f64; 2],
    others: &[EntityId],
    values: Option<&crate::view::DimValues>,
) -> Option<&'static str> {
    let a = match s.entity(line) {
        Some(Entity::Line { a, .. }) => *a,
        _ => return None,
    };
    let (rel, _) = infer_line(s, sol, (Some(a), from), to, others)?;
    // 向きは描いた位置で確かめる(寄せていなければ付けない)
    let k = rel.constraint(line);
    let decided = values.is_some_and(|v| kmb_sketch::redundant_with(s, v, &k).is_some());
    if decided {
        return None;
    }
    s.add_constraint(k);
    Some(rel.label())
}

#[cfg(test)]
mod curve_snap_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn solved(s: &Sketch) -> Solution {
        kmb_sketch::solve(s, &HashMap::new()).unwrap()
    }

    #[test]
    fn crossings_and_quadrants_are_found() {
        let mut s = Sketch::default();
        let c = s.add_point(0.0, 0.0);
        s.add_circle(c, 10.0);
        let (p, q) = (s.add_point(-20.0, 5.0), s.add_point(20.0, 5.0));
        s.add_line(p, q);
        let sol = solved(&s);
        let found = crossings(&s, &sol);
        let n = |k: Kind| found.iter().filter(|((_, x), _)| *x == k).count();
        assert_eq!(n(Kind::Intersection), 2);
        assert_eq!(n(Kind::Quadrant), 4);
    }

    #[test]
    fn a_line_near_a_direction_is_squared_to_it() {
        let mut s = Sketch::default();
        let (a, b) = (s.add_point(0.0, 0.0), s.add_point(10.0, 10.0));
        let slanted = s.add_line(a, b);
        let sol = solved(&s);
        // ほぼ水平
        let (r, e) = infer_line(&s, &sol, (None, [0.0, 20.0]), [10.0, 20.3], &[]).unwrap();
        assert_eq!(r, LineRel::Horizontal);
        assert!((e[1] - 20.0).abs() < 1e-9);
        // 斜めの線とほぼ平行・直角(触れた線だけが相手)
        let (r, _) = infer_line(&s, &sol, (None, [0.0, 20.0]), [10.0, 30.4], &[slanted]).unwrap();
        assert_eq!(r, LineRel::Parallel(slanted));
        let (r, _) = infer_line(&s, &sol, (None, [0.0, 20.0]), [10.0, 10.3], &[slanted]).unwrap();
        assert_eq!(r, LineRel::Perpendicular(slanted));
        assert!(infer_line(&s, &sol, (None, [0.0, 20.0]), [10.0, 30.4], &[]).is_none());
    }

    #[test]
    fn a_line_from_the_end_of_an_arc_is_tangent() {
        let mut s = Sketch::default();
        let (c, st, en) = (
            s.add_point(0.0, 0.0),
            s.add_point(10.0, 0.0),
            s.add_point(0.0, 10.0),
        );
        let arc = s.add_arc(c, st, en);
        let sol = solved(&s);
        // (0, 10) から左へ引くと接する
        let (r, _) = infer_line(&s, &sol, (Some(en), [0.0, 10.0]), [-10.0, 10.2], &[]).unwrap();
        assert_eq!(r, LineRel::Tangent(arc));
    }

    #[test]
    fn the_cursor_lines_up_with_a_point_it_passed() {
        let mut s = Sketch::default();
        let p = s.add_point(10.0, 10.0);
        let sol = solved(&s);
        let mut st = State::default();
        st.notice(&s, &sol, [10.0, 10.0], 1.0);
        assert_eq!(st.tracked, vec![p]);
        let snapped = snap_cursor(&s, &sol, [10.4, 30.0], 1.0, &st);
        assert_eq!(snapped.pos, [10.0, 30.0]);
        assert_eq!(snapped.label, Some("縦に並ぶ"));
        assert_eq!(snapped.guides.len(), 1);
    }
}
