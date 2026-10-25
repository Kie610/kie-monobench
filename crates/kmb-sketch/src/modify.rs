//! スケッチを直す操作: 延長と破断([`Sketch::extend`]・[`Sketch::break_at`])、面取りの 3 種
//! ([`Sketch::chamfer_corner_with`])、写して動かす([`Sketch::copy_transformed`])・つながりを
//! 選んで動かす([`Sketch::move_with`])、ブレンド曲線([`Sketch::add_blend`])。どれも保存されている
//! 座標(先に [`Sketch::adopt_solution`] で解いた位置を写しておく)を元に要素と拘束を足す。

use crate::curve::{Shape, conic_part, ellipse_angle, ellipse_axes};
use crate::edit::{refs, uses};
use crate::freeform::{Plan, plan, spline_part};
use crate::geom::{Curve, EPS, add, angle_of, cross, dot, intersect, len, scale, sub};
use crate::model::{ConstraintId, ConstraintKind as K, Entity, EntityId, Sketch};
use crate::solver::Solution;
use std::collections::BTreeSet;
use std::f64::consts::{PI, TAU};

type P2 = [f64; 2];

/// 面取りの種類と値。値は数(mm、rad)と、寸法に書く式(`"3 mm"`、`"30 deg"`、パラメータなど)。
#[derive(Debug, Clone, PartialEq)]
pub enum Chamfer {
    /// 両方の線分で同じ距離(既存の [`Sketch::chamfer_corner`] と同じ形)
    Equal { d: f64, value: String },
    /// 1 本目の線分で `d1`、2 本目で `d2`
    TwoDistances {
        d1: f64,
        d2: f64,
        value1: String,
        value2: String,
    },
    /// 1 本目の線分で距離 `d`、面取りの線が 1 本目の線分となす角 `angle`(角の内側で測る)
    DistanceAngle {
        d: f64,
        angle: f64,
        value: String,
        angle_value: String,
    },
}

/// 動かし方・写し方。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Transform {
    /// ずらす(mm)
    Translate([f64; 2]),
    /// 点 `center` のまわりに `angle`(rad、反時計回り)回す
    Rotate { center: [f64; 2], angle: f64 },
    /// 点 `center` を中心に `k` 倍にする
    Scale { center: [f64; 2], k: f64 },
}

impl Transform {
    /// 点を写す。
    pub fn apply(&self, p: [f64; 2]) -> [f64; 2] {
        match *self {
            Transform::Translate(d) => add(p, d),
            Transform::Rotate { center, angle } => crate::arrange::rotate_about(center, angle)(p),
            Transform::Scale { center, k } => add(center, scale(sub(p, center), k)),
        }
    }
}

/// 動かす時の、選んでいない要素とのつながりの扱い。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Links {
    /// つながりを残す。端を共有する図形は伸び縮みし、一致の拘束で
    /// つながる選んでいない点も一緒に動かす(解き直しで引き戻されない)。それ以外の
    /// 選んでいない要素への拘束(寸法など)は残すので、解き直すとそれに引かれることがある
    /// ([`Moved::holding`])
    #[default]
    Follow,
    /// つながりを外す。選んでいない図形と共有する端の点は分け、選んでいない要素へつながる
    /// 拘束(一致・寸法など)は消す([`Moved::dropped`])
    Detach,
}

/// [`Sketch::move_with`] の結果。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Moved {
    /// 動かした要素(選んだ要素と、その点)
    pub entities: Vec<EntityId>,
    /// 一致の拘束でつながっていて一緒に動かした、選んでいない点([`Links::Follow`])
    pub followers: Vec<EntityId>,
    /// 消した拘束([`Links::Detach`])
    pub dropped: Vec<ConstraintId>,
    /// 選んでいない要素や、伸び縮みした図形にかかっていて、解き直すと形を引き戻すかもしれない
    /// 拘束([`Links::Follow`]。画面で知らせる用)
    pub holding: Vec<ConstraintId>,
}

/// ブレンド曲線のつなぎ方。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Continuity {
    /// 接する(向きがそろう)
    G1,
    /// 曲率もそろう
    G2,
}

/// 延長の行き先。
struct Reach {
    /// 延ばす端の点
    end: EntityId,
    /// 始点の側を延ばすか
    at_start: bool,
    /// 当たった所
    to: P2,
    /// 当たった曲線
    by: EntityId,
    /// 延びる部分の折れ線
    path: Vec<P2>,
}

fn arc_points(c: P2, r: f64, a0: f64, a1: f64) -> Vec<P2> {
    let n = (((a1 - a0).abs() / TAU) * 96.0).ceil().max(2.0) as usize;
    (0..=n)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f64 / n as f64;
            [c[0] + r * a.cos(), c[1] + r * a.sin()]
        })
        .collect()
}

fn shape_points(s: &Shape, a: f64, b: f64) -> Vec<P2> {
    let n = 96;
    (0..=n)
        .map(|i| s.at(a + (b - a) * i as f64 / n as f64))
        .collect()
}

impl Sketch {
    /// 曲線 `target` とほかの曲線の交点と、それを作った曲線(楕円自身の軸の補助線は除く)。
    fn crossings(&self, target: EntityId, curve: &Curve) -> Vec<(P2, EntityId)> {
        let mut cuts = vec![];
        for e in &self.entities {
            let id = e.id();
            if id == target || self.own_axis(target, id) {
                continue;
            }
            if let Some(c) = self.curve(id) {
                cuts.extend(intersect(curve, &c).into_iter().map(|p| (p, id)));
            }
        }
        cuts
    }

    /// スケッチの大きさの目安(点を囲む箱の対角と、円の直径)。
    fn extent(&self) -> f64 {
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        let mut r: f64 = 0.0;
        for e in &self.entities {
            match e {
                Entity::Point { x, y, .. } => {
                    lo = [lo[0].min(*x), lo[1].min(*y)];
                    hi = [hi[0].max(*x), hi[1].max(*y)];
                }
                Entity::Circle { radius, .. } => r = r.max(*radius),
                _ => {}
            }
        }
        if lo[0] > hi[0] {
            return 1.0;
        }
        len(sub(hi, lo)) + 2.0 * r + 1.0
    }

    /// 延長の行き先(保存されている位置で)。延ばせなければ `None`。
    fn reach(&self, target: EntityId, at: P2) -> Option<Reach> {
        match self.entity(target)?.clone() {
            Entity::Line { a, b, .. } => {
                let (pa, pb) = (self.pos(a)?, self.pos(b)?);
                let d = sub(pb, pa);
                let l = len(d);
                if l < EPS {
                    return None;
                }
                let t = dot(sub(at, pa), d) / (l * l);
                let at_start = t < 0.5;
                let (from, u) = if at_start {
                    (pa, scale(d, -1.0 / l))
                } else {
                    (pb, scale(d, 1.0 / l))
                };
                let ray = Curve::Seg(from, add(from, scale(u, 4.0 * self.extent() + 4.0 * l)));
                let (to, by) = self
                    .crossings(target, &ray)
                    .into_iter()
                    .map(|(p, by)| (len(sub(p, from)), p, by))
                    .filter(|(dist, ..)| *dist > 1e-7)
                    .min_by(|x, y| x.0.total_cmp(&y.0))
                    .map(|(_, p, by)| (p, by))?;
                Some(Reach {
                    end: if at_start { a } else { b },
                    at_start,
                    to,
                    by,
                    path: vec![from, to],
                })
            }
            Entity::Arc { start, end, .. } => {
                let Some(Curve::Arc(c, r, a0, span)) = self.curve(target) else {
                    return None;
                };
                if span >= TAU - EPS {
                    return None;
                }
                let u = (angle_of(c, at) - a0).rem_euclid(TAU);
                let at_start = if u <= span {
                    u < span / 2.0
                } else {
                    TAU - u < u - span
                };
                let full = Curve::Arc(c, r, 0.0, TAU);
                let a1 = a0 + span;
                let (delta, to, by) = self
                    .crossings(target, &full)
                    .into_iter()
                    .map(|(p, by)| {
                        let ang = angle_of(c, p);
                        let delta = if at_start {
                            (a0 - ang).rem_euclid(TAU)
                        } else {
                            (ang - a1).rem_euclid(TAU)
                        };
                        (delta, p, by)
                    })
                    .filter(|(d, ..)| *d > 1e-9 && *d < TAU - span - 1e-9)
                    .min_by(|x, y| x.0.total_cmp(&y.0))?;
                let path = if at_start {
                    arc_points(c, r, a0, a0 - delta)
                } else {
                    arc_points(c, r, a1, a1 + delta)
                };
                Some(Reach {
                    end: if at_start { start } else { end },
                    at_start,
                    to,
                    by,
                    path,
                })
            }
            Entity::Ellipse {
                center,
                major,
                minor,
                ends: Some((s, e)),
                ..
            } => {
                let (c, ma, mi) = (self.pos(center)?, self.pos(major)?, self.pos(minor)?);
                let (u, v) = ellipse_axes(c, ma, mi)?;
                let t0 = ellipse_angle(c, u, v, self.pos(s)?);
                let mut span = (ellipse_angle(c, u, v, self.pos(e)?) - t0).rem_euclid(TAU);
                if span < 1e-12 {
                    return None;
                }
                if span >= TAU - EPS {
                    span = TAU;
                }
                let ua = (ellipse_angle(c, u, v, at) - t0).rem_euclid(TAU);
                let at_start = if ua <= span {
                    ua < span / 2.0
                } else {
                    TAU - ua < ua - span
                };
                let whole = Shape::ellipse(c, ma, mi, None)?;
                let full = Curve::Free(Box::new(whole.clone()));
                let t1 = t0 + span;
                let (delta, to, by) = self
                    .crossings(target, &full)
                    .into_iter()
                    .map(|(p, by)| {
                        let ang = ellipse_angle(c, u, v, p);
                        let delta = if at_start {
                            (t0 - ang).rem_euclid(TAU)
                        } else {
                            (ang - t1).rem_euclid(TAU)
                        };
                        (delta, p, by)
                    })
                    .filter(|(d, ..)| *d > 1e-9 && *d < TAU - span - 1e-9)
                    .min_by(|x, y| x.0.total_cmp(&y.0))?;
                let path = if at_start {
                    shape_points(&whole, t0, t0 - delta)
                } else {
                    shape_points(&whole, t1, t1 + delta)
                };
                Some(Reach {
                    end: if at_start { s } else { e },
                    at_start,
                    to,
                    by,
                    path,
                })
            }
            _ => None,
        }
    }

    /// 曲線 `target`(線分・円弧・楕円弧)の、`at` に近い端を、延ばした先で次に当たる曲線まで
    /// 延ばす(スケッチの延長)。延ばした端は新しい点にして、当たった曲線の上に拘束する。
    /// 前の端の点は、ほかに使われていなければ消す(その点への拘束も落ちる)。線分の長さの寸法と
    /// 長さの等しい拘束は、長さが変わるので外す。当たる物がない(円・スプライン・円錐曲線も)
    /// なら何もせず偽を返す。
    pub fn extend(&mut self, target: EntityId, at: [f64; 2]) -> bool {
        let Some(r) = self.reach(target, at) else {
            return false;
        };
        let m = self.add_point(r.to[0], r.to[1]);
        self.attach(m, r.by);
        let swap = |p: &mut EntityId| {
            if *p == r.end {
                *p = m;
            }
        };
        for e in &mut self.entities {
            if e.id() != target {
                continue;
            }
            match e {
                Entity::Line { a, b, .. } => {
                    if r.at_start {
                        swap(a)
                    } else {
                        swap(b)
                    }
                }
                Entity::Arc { start, end, .. } => {
                    if r.at_start {
                        swap(start)
                    } else {
                        swap(end)
                    }
                }
                Entity::Ellipse {
                    ends: Some((s, t)), ..
                } => {
                    if r.at_start {
                        swap(s)
                    } else {
                        swap(t)
                    }
                }
                _ => {}
            }
        }
        let is_line = matches!(self.entity(target), Some(Entity::Line { .. }));
        // 前の端でつながっていた曲線との、端でそろえる拘束は意味がなくなる
        let joined: BTreeSet<EntityId> = self
            .entities
            .iter()
            .filter(|e| e.id() != target && e.ends().is_some_and(|(a, b)| a == r.end || b == r.end))
            .map(Entity::id)
            .collect();
        self.constraints.retain(|c| match c.kind {
            K::Length { line, .. } => !(is_line && line == target),
            K::Equal { a, b } => !(is_line && (a == target || b == target)),
            K::TangentCurve { a, b } | K::Curvature { a, b } => {
                !((a == target && joined.contains(&b)) || (b == target && joined.contains(&a)))
            }
            _ => true,
        });
        let used = self.entities.iter().any(|e| uses(e).contains(&r.end));
        if !used {
            self.remove_entities(&[r.end]);
        }
        true
    }

    /// 延長の道具で `target` の `at` に近い端を押した時に、延びる部分(折れ線、スケッチの座標)。
    /// [`Sketch::extend`] と同じ決め方で、解けた位置 `sol` で求める。延ばせなければ `None`。
    pub fn extend_preview(
        &self,
        sol: &Solution,
        target: EntityId,
        at: [f64; 2],
    ) -> Option<Vec<[f64; 2]>> {
        let mut s = self.clone();
        s.adopt_solution(sol);
        s.reach(target, at).map(|r| r.path)
    }

    /// 破断で分ける所(保存されている位置で)。分けられなければ `None`。
    fn break_plan(&self, target: EntityId, at: P2) -> Option<(Shape, Plan)> {
        let curve = self.curve(target)?;
        let shape = self.shape(target)?;
        let cuts = self.crossings(target, &curve);
        match plan(&shape, &cuts, at) {
            Plan::Whole => None,
            p => Some((shape, p)),
        }
    }

    /// 曲線 `target` の `t0`〜`t1` を、端の点を `p`・`q` にした 1 本にする(破断の片)。
    fn piece(
        &mut self,
        e: &Entity,
        shape: &Shape,
        t0: f64,
        t1: f64,
        p: EntityId,
        q: EntityId,
    ) -> Option<EntityId> {
        let constr = e.is_construction();
        let id = match (e, shape) {
            (Entity::Line { .. }, _) => self.add_line(p, q),
            (Entity::Arc { center, .. } | Entity::Circle { center, .. }, _) => {
                self.add_arc(*center, p, q)
            }
            (
                Entity::Ellipse {
                    center,
                    major,
                    minor,
                    ..
                },
                _,
            ) => self.ellipse_arc(*center, *major, *minor, p, q, constr),
            (Entity::Spline { .. }, Shape::Spline { s, closed }) => {
                let part = spline_part(s, *closed, t0, t1)?;
                self.add_control_spline(&part, p, q, constr)
            }
            (Entity::Conic { .. }, Shape::Conic { p: cp, w }) => {
                let (_, apex, _, rho) = conic_part(*cp, *w, t0, t1)?;
                let a = self.add_point(apex[0], apex[1]);
                self.add_entity(&Entity::Conic {
                    id: EntityId(0),
                    start: p,
                    end: q,
                    apex: a,
                    rho,
                    construction: constr,
                })
            }
            _ => return None,
        };
        self.set_construction(id, constr);
        Some(id)
    }

    /// 曲線 `target` を、`at` の両隣の交点で切り分ける(スケッチの破断。消す所はない)。
    /// 開いた曲線は 2 つか 3 つ、閉じた曲線(円・楕円・閉じたスプライン)は 2 つに分かれる。
    /// 切った所の点は隣の片どうしで共有し、交わった曲線の上に拘束する。片はもとの線の上に保つ
    /// (線分は後ろの片の端をはじめの片の直線の上に、円弧・楕円弧は中心と軸を共有、スプライン・
    /// 円錐曲線は継ぎ目で曲率をそろえる)。向きや大きさの拘束は、はじめの片(戻り値の先頭)へ
    /// 付け替える。分けた片を返す(交点が足りなければ何もせず空)。
    pub fn break_at(&mut self, target: EntityId, at: [f64; 2]) -> Vec<EntityId> {
        let Some(e) = self.entity(target).cloned() else {
            return vec![];
        };
        let Some((shape, plan)) = self.break_plan(target, at) else {
            return vec![];
        };
        let mut pieces = vec![];
        match plan {
            Plan::Open { lo, hi } => {
                let Some((s, t)) = e.ends() else {
                    return vec![];
                };
                let (dlo, dhi) = shape.domain();
                let mut marks = vec![(dlo, s)];
                for c in [lo, hi].into_iter().flatten() {
                    let m = self.cut_point(&c);
                    marks.push((c.0, m));
                }
                marks.push((dhi, t));
                for w in marks.windows(2) {
                    if let Some(p) = self.piece(&e, &shape, w[0].0, w[1].0, w[0].1, w[1].1) {
                        pieces.push(p);
                    }
                }
            }
            Plan::Closed { keep_from, keep_to } => {
                let (f, t) = (self.cut_point(&keep_from), self.cut_point(&keep_to));
                // 押した所を通る片(keep_to から keep_from)が先、残りが後
                for (t0, t1, p, q) in [
                    (keep_to.0, keep_from.0, t, f),
                    (keep_from.0, keep_to.0, f, t),
                ] {
                    if let Some(id) = self.piece(&e, &shape, t0, t1, p, q) {
                        pieces.push(id);
                    }
                }
            }
            Plan::Whole => return vec![],
        }
        if pieces.len() < 2 {
            // 片を作れなかった: 足した物を戻す
            let gone: Vec<EntityId> = pieces;
            self.remove_entities(&gone);
            return vec![];
        }
        match e {
            Entity::Line { .. } => {
                let first = pieces[0];
                let own: Vec<EntityId> = self.entity(first).map(uses).unwrap_or_default();
                let mut rest: Vec<EntityId> = pieces[1..]
                    .iter()
                    .filter_map(|p| self.entity(*p).map(uses))
                    .flatten()
                    .filter(|p| !own.contains(p))
                    .collect();
                rest.sort();
                rest.dedup();
                for point in rest {
                    self.add_constraint(K::PointOnLine { point, line: first });
                }
            }
            Entity::Spline { .. } | Entity::Conic { .. } => {
                let n = pieces.len();
                let pairs = if shape.closed() { n } else { n - 1 };
                for k in 0..pairs {
                    self.add_constraint(K::Curvature {
                        a: pieces[k],
                        b: pieces[(k + 1) % n],
                    });
                }
            }
            _ => {}
        }
        self.retarget_pieces(target, &pieces);
        self.replace_curve(target, pieces[0]);
        pieces
    }

    /// 破断の道具で `target` の `at` を押した時に、切り離される片(押した所を通る片。折れ線、
    /// スケッチの座標)。[`Sketch::break_at`] と同じ決め方で、解けた位置 `sol` で求める。
    /// 分けられなければ `None`。
    pub fn break_preview(
        &self,
        sol: &Solution,
        target: EntityId,
        at: [f64; 2],
    ) -> Option<Vec<[f64; 2]>> {
        let mut s = self.clone();
        s.adopt_solution(sol);
        let (shape, plan) = s.break_plan(target, at)?;
        let (lo, hi) = shape.domain();
        let (a, b) = match plan {
            Plan::Whole => return None,
            Plan::Open { lo: l, hi: h } => (l.map_or(lo, |c| c.0), h.map_or(hi, |c| c.0)),
            Plan::Closed { keep_from, keep_to } => {
                let mut b = keep_from.0;
                if b <= keep_to.0 {
                    b += hi - lo;
                }
                (keep_to.0, b)
            }
        };
        Some(shape_points(&shape, a, b))
    }

    /// 2 本の線分が点 `corner` で作る角を落とす(スケッチの面取り。距離が等しい・距離 2 つ・
    /// 距離と角度)。`first` は 1 本目の線分(距離 1 と角度を測る方。角の線分でなければ見つけた順)。
    /// 線分の端を落とした所へ移して、その間に線分を引く。角の点は残し、補助線で両端とつなぎ、
    /// 両方の線分を延ばした直線の上に拘束する。寸法は補助線の長さ(と、面取りの線と 1 本目の
    /// 補助線の角度)で付けるので、値を後から直せる。足した面取りの線分と寸法の拘束を返す。
    /// 落とせない(角に線分が 2 本でない、まっすぐ、線分が距離に対して短い、角度が合わない)
    /// なら何もせず `None`。
    pub fn chamfer_corner_with(
        &mut self,
        corner: EntityId,
        first: Option<EntityId>,
        chamfer: &Chamfer,
    ) -> Option<(EntityId, Vec<ConstraintId>)> {
        let mut lines: Vec<(EntityId, EntityId)> = self
            .entities
            .iter()
            .filter_map(|e| match e {
                Entity::Line {
                    id,
                    a,
                    b,
                    construction: false,
                } if *a == corner => Some((*id, *b)),
                Entity::Line {
                    id,
                    a,
                    b,
                    construction: false,
                } if *b == corner => Some((*id, *a)),
                _ => None,
            })
            .collect();
        if lines.len() != 2 {
            return None;
        }
        if first == Some(lines[1].0) {
            lines.swap(0, 1);
        }
        let [(l1, o1), (l2, o2)] = [lines[0], lines[1]];
        let c = self.pos(corner)?;
        let (d1, d2) = (sub(self.pos(o1)?, c), sub(self.pos(o2)?, c));
        let (n1, n2) = (len(d1), len(d2));
        if n1 < EPS || n2 < EPS || cross(d1, d2).abs() / (n1 * n2) < 1e-3 {
            return None;
        }
        let alpha = (dot(d1, d2) / (n1 * n2)).clamp(-1.0, 1.0).acos();
        let (da, db) = match chamfer {
            Chamfer::Equal { d, .. } => (*d, *d),
            Chamfer::TwoDistances { d1, d2, .. } => (*d1, *d2),
            Chamfer::DistanceAngle { d, angle, .. } => {
                if !(*angle > 1e-6 && alpha + angle < PI - 1e-6) {
                    return None;
                }
                (*d, d * angle.sin() / (alpha + angle).sin())
            }
        };
        if !(da > EPS && db > EPS) || n1 <= da + EPS || n2 <= db + EPS {
            return None;
        }
        let (p1, p2) = (add(c, scale(d1, da / n1)), add(c, scale(d2, db / n2)));
        let t1 = self.add_point(p1[0], p1[1]);
        let t2 = self.add_point(p2[0], p2[1]);
        for (line, to) in [(l1, t1), (l2, t2)] {
            for e in &mut self.entities {
                if let Entity::Line { id, a, b, .. } = e
                    && *id == line
                {
                    if *a == corner {
                        *a = to;
                    } else {
                        *b = to;
                    }
                }
            }
        }
        let cut = self.add_line(t1, t2);
        let c1 = self.add_line(corner, t1);
        let c2 = self.add_line(corner, t2);
        self.set_construction(c1, true);
        self.set_construction(c2, true);
        self.add_constraint(K::PointOnLine {
            point: corner,
            line: l1,
        });
        self.add_constraint(K::PointOnLine {
            point: corner,
            line: l2,
        });
        let mut dims = vec![];
        match chamfer {
            Chamfer::Equal { value, .. } => {
                dims.push(self.add_constraint(K::Length {
                    line: c1,
                    value: value.clone(),
                }));
                self.add_constraint(K::Equal { a: c1, b: c2 });
            }
            Chamfer::TwoDistances { value1, value2, .. } => {
                dims.push(self.add_constraint(K::Length {
                    line: c1,
                    value: value1.clone(),
                }));
                dims.push(self.add_constraint(K::Length {
                    line: c2,
                    value: value2.clone(),
                }));
            }
            Chamfer::DistanceAngle {
                value, angle_value, ..
            } => {
                dims.push(self.add_constraint(K::Length {
                    line: c1,
                    value: value.clone(),
                }));
                // 角度の寸法は線分 a から b へ反時計回りに測る(0〜π)。角の内側の角が値になる向きを選ぶ
                let (a, b) = if cross(sub(c, p1), sub(p2, p1)) > 0.0 {
                    (c1, cut)
                } else {
                    (cut, c1)
                };
                dims.push(self.add_constraint(K::Angle {
                    a,
                    b,
                    value: angle_value.clone(),
                }));
            }
        }
        Some((cut, dims))
    }

    /// 選んだ要素を写して、`xf` で動かした所に足す。
    /// 元は残る。写すのは [`Sketch::copy_entities`] と同じで、写した要素の中だけで閉じる拘束と
    /// 寸法は一緒に写し、写していない要素へつながるもの・固定は落とす。回す時は
    /// [`Sketch::rotate_entities`]、拡大縮小は [`Sketch::scale_entities`] と同じく拘束を直す。
    /// 寸法を置いた位置も一緒に動かす。足した要素(点と図形)を返す。
    pub fn copy_transformed(&mut self, ids: &[EntityId], xf: Transform) -> Vec<EntityId> {
        if let Transform::Scale { k, .. } = xf
            && (k <= EPS || !k.is_finite())
        {
            return vec![];
        }
        let Some(clip) = self.copy_entities(ids) else {
            return vec![];
        };
        let before = self.constraints.iter().map(|c| c.id.0).max().unwrap_or(0);
        let new = match xf {
            Transform::Translate(d) => return self.paste(&clip, d),
            _ => self.paste(&clip, [0.0, 0.0]),
        };
        match xf {
            Transform::Rotate { center, angle } => {
                self.rotate_entities(&new, center, angle);
            }
            Transform::Scale { center, k } => self.scale_entities(&new, center, k),
            Transform::Translate(_) => {}
        }
        for c in &mut self.constraints {
            if c.id.0 > before
                && let Some(p) = c.at
            {
                c.at = Some(xf.apply(p));
            }
        }
        new
    }

    /// 選んだ要素を `xf` で動かす。選んでいない要素とのつながりは `links`
    /// で選ぶ([`Links`])。どちらでも、選んだ要素の中で閉じる拘束は残し、回す・拡大縮小する時の
    /// 直し方は [`Sketch::rotate_entities`]・[`Sketch::scale_entities`] と同じ。拡大縮小の `k` が
    /// 正でなければ何もしない。
    pub fn move_with(&mut self, ids: &[EntityId], xf: Transform, links: Links) -> Moved {
        if let Transform::Scale { k, .. } = xf
            && (k <= EPS || !k.is_finite())
        {
            return Moved::default();
        }
        let mut out = Moved::default();
        let mut ids: Vec<EntityId> = ids
            .iter()
            .copied()
            .filter(|id| self.entity(*id).is_some())
            .collect();
        match links {
            Links::Follow => {
                // 一致の拘束でつながる選んでいない点を、つながりをたどって集める
                let mut pts = self.points_of(&ids);
                loop {
                    let mut grew = false;
                    for c in &self.constraints {
                        if let K::Coincident { a, b } = c.kind {
                            for (p, q) in [(a, b), (b, a)] {
                                if pts.contains(&p)
                                    && !pts.contains(&q)
                                    && matches!(self.entity(q), Some(Entity::Point { .. }))
                                {
                                    pts.insert(q);
                                    out.followers.push(q);
                                    grew = true;
                                }
                            }
                        }
                    }
                    if !grew {
                        break;
                    }
                }
                ids.extend(out.followers.iter().copied());
                let set = self.closure(&ids);
                // 動いた点を使う、選んでいない図形(伸び縮みする)
                let stretched: BTreeSet<EntityId> = self
                    .entities
                    .iter()
                    .filter(|e| !set.contains(&e.id()) && uses(e).iter().any(|p| set.contains(p)))
                    .map(Entity::id)
                    .collect();
                out.holding = self
                    .constraints
                    .iter()
                    .filter(|c| !c.reference && !matches!(c.kind, K::Coincident { .. }))
                    .filter(|c| {
                        let r = refs(&c.kind);
                        let inside = r.iter().all(|x| set.contains(x));
                        let touches = r.iter().any(|x| set.contains(x) || stretched.contains(x));
                        touches && !inside
                    })
                    .map(|c| c.id)
                    .collect();
            }
            Links::Detach => {
                let explicit: BTreeSet<EntityId> = ids.iter().copied().collect();
                let curves: BTreeSet<EntityId> = ids
                    .iter()
                    .copied()
                    .filter(|id| self.entity(*id).is_some_and(Entity::is_curve))
                    .collect();
                // 選んでいない図形と共有する端の点は、選んだ図形の側に別の点を作って分ける
                let shared: Vec<EntityId> = self
                    .points_of(&ids)
                    .into_iter()
                    .filter(|p| !explicit.contains(p))
                    .filter(|p| {
                        self.entities
                            .iter()
                            .any(|e| !curves.contains(&e.id()) && uses(e).contains(p))
                    })
                    .collect();
                for p in shared {
                    let Some(pos) = self.pos(p) else { continue };
                    let n = self.add_point(pos[0], pos[1]);
                    for e in &mut self.entities {
                        if curves.contains(&e.id()) && uses(e).contains(&p) {
                            *e = e.with_points(e.id(), &|q| if q == p { n } else { q });
                        }
                    }
                }
                let set = self.closure(&ids);
                let mut kept = vec![];
                for c in std::mem::take(&mut self.constraints) {
                    let r = refs(&c.kind);
                    let inside = r.iter().all(|x| set.contains(x));
                    let touches = r.iter().any(|x| set.contains(x));
                    if touches && !inside {
                        out.dropped.push(c.id);
                    } else {
                        kept.push(c);
                    }
                }
                self.constraints = kept;
            }
        }
        match xf {
            Transform::Translate(d) => self.move_entities(&ids, d),
            Transform::Rotate { center, angle } => {
                self.rotate_entities(&ids, center, angle);
            }
            Transform::Scale { center, k } => self.scale_entities(&ids, center, k),
        }
        // 選んだ要素の中で閉じる寸法の置いた位置も動かす
        let set = self.closure(&ids);
        for c in &mut self.constraints {
            if let Some(p) = c.at
                && refs(&c.kind).iter().all(|x| set.contains(x))
            {
                c.at = Some(xf.apply(p));
            }
        }
        out.entities = set.into_iter().collect();
        out
    }

    /// 曲線 `a` の端 `a.1`(点)と、曲線 `b` の端 `b.1` を、スプライン(制御点の 3 次 B スプライン)で
    /// つなぐ(ブレンド曲線)。G1 なら両端で接する([`K::TangentCurve`])、G2 なら曲率もそろう
    /// ([`K::Curvature`]。接する向きも含むので、接する拘束は重ねない)。スプラインの両端は曲線の
    /// 端の点そのもの(共有)。つなげない(端でない、同じ曲線、端が重なる)なら `None`。
    pub fn add_blend(
        &mut self,
        a: (EntityId, EntityId),
        b: (EntityId, EntityId),
        continuity: Continuity,
    ) -> Option<EntityId> {
        if a.0 == b.0 || a.1 == b.1 {
            return None;
        }
        // 端から外へ出る向きと、外へたどる向きの曲率
        let end_of = |s: &Sketch, (curve, point): (EntityId, EntityId)| -> Option<(P2, P2, f64)> {
            let (s0, s1) = s.entity(curve)?.ends()?;
            let sh = s.shape(curve)?;
            let (lo, hi) = sh.domain();
            let (t, sign) = if point == s1 {
                (hi, 1.0)
            } else if point == s0 {
                (lo, -1.0)
            } else {
                return None;
            };
            Some((
                s.pos(point)?,
                scale(sh.tangent(t), sign),
                sign * sh.curvature(t),
            ))
        };
        let (p0, ta, ka) = end_of(self, a)?;
        let (p5, tb_out, kb_out) = end_of(self, b)?;
        // b へ入る向きと、入ってたどる向きの曲率
        let (tb, kb) = (scale(tb_out, -1.0), -kb_out);
        let d = len(sub(p5, p0));
        if d < 1e-6 {
            return None;
        }
        let ctrl: Vec<P2> = match continuity {
            Continuity::G1 => {
                let h = d / 3.0;
                vec![p0, add(p0, scale(ta, h)), sub(p5, scale(tb, h)), p5]
            }
            Continuity::G2 => {
                let h = d / 5.0;
                let (na, nb) = ([-ta[1], ta[0]], [-tb[1], tb[0]]);
                let p1 = add(p0, scale(ta, h));
                let p4 = sub(p5, scale(tb, h));
                let at = |w: f64, v: f64| -> Vec<P2> {
                    vec![
                        p0,
                        p1,
                        add(add(p1, scale(ta, h)), scale(na, w)),
                        add(sub(p4, scale(tb, h)), scale(nb, v)),
                        p4,
                        p5,
                    ]
                };
                // 端の曲率は、2 つ目の内側の点を法線の向きにずらした量について一次なので、2 回測って決める
                let k = |pts: &[P2]| -> Option<(f64, f64)> {
                    let s = Shape::spline(pts, true, false, 3, None)?;
                    let (lo, hi) = s.domain();
                    Some((s.curvature(lo), s.curvature(hi)))
                };
                let (k0s, k0e) = k(&at(0.0, 0.0))?;
                let (k1s, k1e) = k(&at(1.0, 1.0))?;
                let solve = |k0: f64, k1: f64, want: f64| {
                    if (k1 - k0).abs() < 1e-300 {
                        0.0
                    } else {
                        (want - k0) / (k1 - k0)
                    }
                };
                at(solve(k0s, k1s, ka), solve(k0e, k1e, kb))
            }
        };
        let mut pts = vec![a.1];
        for c in &ctrl[1..ctrl.len() - 1] {
            pts.push(self.add_point(c[0], c[1]));
        }
        pts.push(b.1);
        let id = self.add_entity(&Entity::Spline {
            id: EntityId(0),
            points: pts,
            control: true,
            closed: false,
            degree: 3,
            knots: None,
            construction: false,
        });
        match continuity {
            Continuity::G1 => {
                self.add_constraint(K::TangentCurve { a: a.0, b: id });
                self.add_constraint(K::TangentCurve { a: id, b: b.0 });
            }
            Continuity::G2 => {
                self.add_constraint(K::Curvature { a: a.0, b: id });
                self.add_constraint(K::Curvature { a: id, b: b.0 });
            }
        }
        Some(id)
    }
}

#[cfg(test)]
#[path = "modify_tests.rs"]
mod tests;
