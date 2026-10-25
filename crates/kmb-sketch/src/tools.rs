//! 作図の道具: 矩形、長穴、多角形、ミラー、トリム、オフセット、角の丸め。どれも保存されている座標
//! (先に [`Sketch::adopt_solution`] で解いた位置を写しておく)を元に要素と拘束を足す。

use crate::edit::{refs, uses};
use crate::geom::{Curve, EPS, add, angle_of, cross, dot, intersect, len, scale, sub};
use crate::model::{ConstraintId, ConstraintKind as K, Entity, EntityId, OffsetPart, Sketch};
use std::collections::BTreeMap;
use std::f64::consts::TAU;

/// 位置。既存の点に吸着していれば、その点を使う。
pub type Spot = (Option<EntityId>, [f64; 2]);

impl Sketch {
    pub(crate) fn pos(&self, id: EntityId) -> Option<[f64; 2]> {
        match self.entity(id)? {
            Entity::Point { x, y, .. } => Some([*x, *y]),
            _ => None,
        }
    }

    pub(crate) fn spot(&mut self, s: Spot) -> EntityId {
        s.0.unwrap_or_else(|| self.add_point(s.1[0], s.1[1]))
    }

    pub(crate) fn curve(&self, id: EntityId) -> Option<Curve> {
        Some(match self.entity(id)? {
            Entity::Point { .. } => return None,
            Entity::Line { a, b, .. } => Curve::Seg(self.pos(*a)?, self.pos(*b)?),
            Entity::Circle { center, radius, .. } => {
                Curve::Arc(self.pos(*center)?, *radius, 0.0, TAU)
            }
            Entity::Arc {
                center, start, end, ..
            } => {
                let (c, s, e) = (self.pos(*center)?, self.pos(*start)?, self.pos(*end)?);
                let a0 = angle_of(c, s);
                let mut span = (angle_of(c, e) - a0).rem_euclid(TAU);
                if span < EPS {
                    span = TAU;
                }
                Curve::Arc(c, len(sub(s, c)), a0, span)
            }
            Entity::Ellipse { .. } | Entity::Spline { .. } | Entity::Conic { .. } => {
                Curve::Free(Box::new(self.shape(id)?))
            }
        })
    }

    pub(crate) fn set_construction(&mut self, target: EntityId, on: bool) {
        for e in &mut self.entities {
            if e.id() == target
                && let Some(c) = e.construction_mut()
            {
                *c = on;
            }
        }
    }

    pub(crate) fn is_construction(&self, id: EntityId) -> bool {
        self.entity(id).is_some_and(Entity::is_construction)
    }

    /// 対角の 2 隅から矩形を描く。上下の辺に水平、左右の辺に垂直の拘束を付ける。
    /// 幅か高さが 0 なら何もしない。足した 4 本の線分を返す。
    pub fn add_rectangle(&mut self, a: Spot, b: Spot) -> Option<[EntityId; 4]> {
        let (p, q) = (a.1, b.1);
        if (q[0] - p[0]).abs() < 1e-6 || (q[1] - p[1]).abs() < 1e-6 {
            return None;
        }
        let p0 = self.spot(a);
        let p1 = self.add_point(q[0], p[1]);
        let p2 = self.spot(b);
        let p3 = self.add_point(p[0], q[1]);
        let l = [
            self.add_line(p0, p1),
            self.add_line(p1, p2),
            self.add_line(p2, p3),
            self.add_line(p3, p0),
        ];
        self.add_constraint(K::Horizontal { line: l[0] });
        self.add_constraint(K::Horizontal { line: l[2] });
        self.add_constraint(K::Vertical { line: l[1] });
        self.add_constraint(K::Vertical { line: l[3] });
        Some(l)
    }

    /// 中心と 1 つの隅から矩形を描く。中心は補助線の対角線の中点に拘束する。
    pub fn add_center_rectangle(&mut self, center: Spot, corner: Spot) -> Option<[EntityId; 4]> {
        let (c, q) = (center.1, corner.1);
        let p = [2.0 * c[0] - q[0], 2.0 * c[1] - q[1]];
        let l = self.add_rectangle((None, p), corner)?;
        let ends = |s: &Sketch, line: EntityId| match s.entity(line) {
            Some(Entity::Line { a, .. }) => *a,
            _ => unreachable!("足したばかりの線分"),
        };
        let (p0, p2) = (ends(self, l[0]), ends(self, l[2]));
        let diag = self.add_line(p0, p2);
        self.set_construction(diag, true);
        let m = self.spot(center);
        self.add_constraint(K::Midpoint {
            point: m,
            line: diag,
        });
        Some(l)
    }

    /// 対角の 2 隅から、角を半径 `r` で丸めた矩形を描く。線 4 本と円弧 4 つの 1 つの閉じた輪郭で、円弧は線に接する。半径の寸法は
    /// 1 つ目の円弧にだけ式 `value` で付け、ほかの 3 つは 1 つ目と等しくする(自由度は位置 2・
    /// 大きさ 2)。半径は 0 より大きく、短い辺の半分より小さいこと(半分ちょうどは長穴になるので
    /// 断る)。作れなければスケッチを変えずに `None`。(線 4 本(下・右・上・左)、円弧 4 つ)を返す。
    pub fn add_rounded_rectangle(
        &mut self,
        a: Spot,
        b: Spot,
        r: f64,
        value: &str,
    ) -> Option<([EntityId; 4], [EntityId; 4])> {
        let (p, q) = (a.1, b.1);
        let short = (q[0] - p[0]).abs().min((q[1] - p[1]).abs());
        if !(r > EPS && r < short / 2.0 - 1e-6) {
            return None;
        }
        let before = self.clone();
        // 隅は丸めて消えるので、吸い付いた点には付けない
        let l = self.add_rectangle((None, p), (None, q))?;
        let corner = |s: &Sketch, line: EntityId| match s.entity(line) {
            Some(Entity::Line { a, .. }) => *a,
            _ => unreachable!("足したばかりの線分"),
        };
        let corners = l.map(|line| corner(self, line));
        let mut arcs = [EntityId(0); 4];
        for (k, c) in corners.into_iter().enumerate() {
            let Some(arc) = self.fillet_corner(c, r, value) else {
                *self = before;
                return None;
            };
            arcs[k] = arc;
        }
        // 半径の寸法は 1 つ目だけ。ほかは 1 つ目と等しい
        let extra: Vec<EntityId> = arcs[1..].to_vec();
        self.constraints
            .retain(|c| !matches!(&c.kind, K::Radius { circle, .. } if extra.contains(circle)));
        for arc in extra {
            self.add_constraint(K::Equal { a: arcs[0], b: arc });
        }
        Some((l, arcs))
    }

    /// 角を丸めた矩形の幅・高さの寸法(向かい合う辺の距離。丸めると線が短くなるので線の長さにしない)。
    /// `lines` は [`Self::add_rounded_rectangle`] の線(下・右・上・左)。
    pub fn rounded_rectangle_size(
        &mut self,
        lines: [EntityId; 4],
        w: Option<&str>,
        h: Option<&str>,
    ) {
        let start = |s: &Sketch, line: EntityId| match s.entity(line) {
            Some(Entity::Line { a, .. }) => Some(*a),
            _ => None,
        };
        let (Some(bottom), Some(right), Some(top), Some(left)) = (
            start(self, lines[0]),
            start(self, lines[1]),
            start(self, lines[2]),
            start(self, lines[3]),
        ) else {
            return;
        };
        if let Some(v) = w {
            self.add_constraint(K::HorizontalDistance {
                a: left,
                b: right,
                value: v.to_string(),
            });
        }
        if let Some(v) = h {
            self.add_constraint(K::VerticalDistance {
                a: bottom,
                b: top,
                value: v.to_string(),
            });
        }
    }

    /// 2 つの中心 `a`、`b` と幅の半分 `w` から長穴を描く。両端の円弧と 2 本の線分は接し、
    /// 両端の半径は等しい。中心を結ぶ補助線を引く。足した要素を
    /// (線分 2 本、円弧 2 本)の順で返す。中心が重なるか幅が 0 なら `None`。
    pub fn add_slot(&mut self, a: Spot, b: Spot, w: f64) -> Option<[EntityId; 4]> {
        let (c1, c2) = (a.1, b.1);
        let d = sub(c2, c1);
        let l = len(d);
        if l < 1e-6 || w < 1e-6 {
            return None;
        }
        let n = [-d[1] / l * w, d[0] / l * w];
        let p1 = self.spot(a);
        let p2 = self.spot(b);
        let q = [add(c1, n), add(c2, n), sub(c2, n), sub(c1, n)];
        let q: Vec<EntityId> = q.iter().map(|q| self.add_point(q[0], q[1])).collect();
        let top = self.add_line(q[0], q[1]);
        let bottom = self.add_line(q[2], q[3]);
        // 円弧は中心のまわりに反時計回り(右端は -n から +n、左端は +n から -n)
        let right = self.add_arc(p2, q[2], q[1]);
        let left = self.add_arc(p1, q[0], q[3]);
        let axis = self.add_line(p1, p2);
        self.set_construction(axis, true);
        for line in [top, bottom] {
            for circle in [left, right] {
                self.add_constraint(K::Tangent { line, circle });
            }
        }
        self.add_constraint(K::Equal { a: left, b: right });
        Some([top, bottom, left, right])
    }

    /// 中心 `center` と 1 つの頂点 `corner` から、`n` 角形を描く。辺の長さを等しくし、
    /// 頂点は補助線の外接円の上に置く。足した辺を返す。`n` が 3 未満か大きさが 0 なら `None`。
    pub fn add_polygon(&mut self, center: Spot, corner: Spot, n: usize) -> Option<Vec<EntityId>> {
        let (c, q) = (center.1, corner.1);
        let r = len(sub(q, c));
        if n < 3 || r < 1e-6 {
            return None;
        }
        let a0 = angle_of(c, q);
        let cp = self.spot(center);
        let circle = self.add_circle(cp, r);
        self.set_construction(circle, true);
        let mut pts = vec![self.spot(corner)];
        for k in 1..n {
            let t = a0 + TAU * k as f64 / n as f64;
            pts.push(self.add_point(c[0] + r * t.cos(), c[1] + r * t.sin()));
        }
        let sides: Vec<EntityId> = (0..n)
            .map(|k| self.add_line(pts[k], pts[(k + 1) % n]))
            .collect();
        self.add_constraint(K::Polygon {
            circle,
            sides: sides.clone(),
        });
        Some(sides)
    }

    /// ポリゴンの拘束 `id` の辺の数を `n`(3〜64)にする。最初の辺とその両端は残し(付けた拘束や
    /// 寸法も残る)、ほかの辺を作り直す。頂点は今の外接円の上に、最初の辺と同じ回り方で並べる。
    /// 拘束の番号は変わらない。直せなければ偽。
    pub fn set_polygon_sides(&mut self, id: ConstraintId, n: usize) -> bool {
        let Some(at) = self.constraints.iter().position(|c| c.id == id) else {
            return false;
        };
        let K::Polygon { circle, sides } = self.constraints[at].kind.clone() else {
            return false;
        };
        if !(3..=64).contains(&n) || n == sides.len() {
            return false;
        }
        let (Some(Entity::Circle { center, radius, .. }), Some(Entity::Line { a: v0, b: v1, .. })) =
            (self.entity(circle).cloned(), self.entity(sides[0]).cloned())
        else {
            return false;
        };
        let (Some(c), Some(p0), Some(p1)) = (self.pos(center), self.pos(v0), self.pos(v1)) else {
            return false;
        };
        let turn = if cross(sub(p0, c), sub(p1, c)) >= 0.0 {
            1.0
        } else {
            -1.0
        };
        let a0 = angle_of(c, p0);
        let at_k = |k: usize| {
            let t = a0 + turn * TAU * k as f64 / n as f64;
            [c[0] + radius * t.cos(), c[1] + radius * t.sin()]
        };
        let mut kept = self.constraints.remove(at);
        self.remove_entities(&sides[1..]);
        let q = at_k(1);
        self.set_point(v1, q[0], q[1]);
        let mut pts = vec![v0, v1];
        for k in 2..n {
            let q = at_k(k);
            pts.push(self.add_point(q[0], q[1]));
        }
        let mut new_sides = vec![sides[0]];
        for k in 1..n {
            new_sides.push(self.add_line(pts[k], pts[(k + 1) % n]));
        }
        kept.kind = K::Polygon {
            circle,
            sides: new_sides,
        };
        let at = at.min(self.constraints.len());
        self.constraints.insert(at, kept);
        true
    }

    /// 辺 `side` を持つポリゴンの拘束。
    pub fn polygon_of(&self, side: EntityId) -> Option<ConstraintId> {
        self.constraints
            .iter()
            .find(|c| matches!(&c.kind, K::Polygon { sides, .. } if sides.contains(&side)))
            .map(|c| c.id)
    }

    /// 線分 `line` の端 `at` から、線に接して続く円弧を `end` まで描く。
    /// 円弧には線との接する拘束を付ける。描けない(終点が線の延長上など)なら `None`。
    pub fn add_tangent_arc(&mut self, line: EntityId, at: EntityId, end: Spot) -> Option<EntityId> {
        let Some(Entity::Line { a, b, .. }) = self.entity(line).cloned() else {
            return None;
        };
        let other = if at == b {
            a
        } else if at == a {
            b
        } else {
            return None;
        };
        let (s, o) = (self.pos(at)?, self.pos(other)?);
        let d = sub(s, o);
        let dl = len(d);
        if dl < EPS {
            return None;
        }
        let n = [-d[1] / dl, d[0] / dl];
        let se = sub(s, end.1);
        let k = dot(n, se);
        if k.abs() < 1e-9 || dot(se, se) < 1e-12 {
            return None;
        }
        // 中心は始点の法線の上: |s + t n - e| = |t|
        let t = -dot(se, se) / (2.0 * k);
        let c = add(s, scale(n, t));
        let center = self.add_point(c[0], c[1]);
        let e = self.spot(end);
        // 左へ曲がるなら始点から反時計回り、右なら終点から反時計回り
        let arc = if t > 0.0 {
            self.add_arc(center, at, e)
        } else {
            self.add_arc(center, e, at)
        };
        self.add_constraint(K::Tangent { line, circle: arc });
        Some(arc)
    }

    /// 2 本の線分が点 `corner` で作る角を、半径 `r` の円弧で丸める(スケッチのフィレット)。
    /// 線分の端を接点へ移し、円弧には両方の線分に接する拘束と、半径の寸法 `value` を付ける。
    /// 角の点は、ほかに使われていなければ消す。丸められない(角に線分が 2 本でない、
    /// まっすぐ、線分が半径に対して短い)なら `None`。
    pub fn fillet_corner(&mut self, corner: EntityId, r: f64, value: &str) -> Option<EntityId> {
        if r <= EPS {
            return None;
        }
        let lines: Vec<(EntityId, EntityId)> = self
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
        let [(l1, o1), (l2, o2)] = lines.as_slice() else {
            return None;
        };
        let c = self.pos(corner)?;
        let (d1, d2) = (sub(self.pos(*o1)?, c), sub(self.pos(*o2)?, c));
        let (n1, n2) = (len(d1), len(d2));
        if n1 < EPS || n2 < EPS {
            return None;
        }
        let (u1, u2) = (scale(d1, 1.0 / n1), scale(d2, 1.0 / n2));
        let angle = dot(u1, u2).clamp(-1.0, 1.0).acos();
        if !(1e-3..std::f64::consts::PI - 1e-3).contains(&angle) {
            return None;
        }
        // 角から接点までの長さ
        let t = r / (angle / 2.0).tan();
        if t >= n1 - EPS || t >= n2 - EPS {
            return None;
        }
        let (p1, p2) = (add(c, scale(u1, t)), add(c, scale(u2, t)));
        let bis = add(u1, u2);
        let center = add(c, scale(bis, r / (angle / 2.0).sin() / len(bis)));
        let t1 = self.add_point(p1[0], p1[1]);
        let t2 = self.add_point(p2[0], p2[1]);
        let cp = self.add_point(center[0], center[1]);
        for (line, to) in [(*l1, t1), (*l2, t2)] {
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
        // 円弧は中心のまわりに反時計回り
        let arc = if cross(sub(p1, center), sub(p2, center)) > 0.0 {
            self.add_arc(cp, t1, t2)
        } else {
            self.add_arc(cp, t2, t1)
        };
        self.add_constraint(K::Tangent {
            line: *l1,
            circle: arc,
        });
        self.add_constraint(K::Tangent {
            line: *l2,
            circle: arc,
        });
        self.add_constraint(K::Radius {
            circle: arc,
            value: value.to_string(),
        });
        let used = self.entities.iter().any(|e| uses(e).contains(&corner))
            || self
                .constraints
                .iter()
                .any(|k| refs(&k.kind).contains(&corner));
        if used {
            // 式や寸法が角の点を指していれば、点を 2 本の線の延長の交わりに留め、
            // 角の位置が変わると丸めた輪郭も一緒に動くようにする
            for line in [*l1, *l2] {
                self.add_constraint(K::PointOnLine {
                    point: corner,
                    line,
                });
            }
        } else {
            self.entities.retain(|e| e.id() != corner);
        }
        Some(arc)
    }

    /// 要素 `ids` を線分 `axis` について写す。写した点には対称の拘束を付け、
    /// 軸の上にある点はそのまま使う。写してできた要素を返す。
    pub fn mirror(&mut self, ids: &[EntityId], axis: EntityId) -> Vec<EntityId> {
        let Some(Entity::Line { .. }) = self.entity(axis) else {
            return vec![];
        };
        let Some(Curve::Seg(a, b)) = self.curve(axis) else {
            return vec![];
        };
        let d = sub(b, a);
        let dd = dot(d, d);
        if dd < EPS {
            return vec![];
        }
        let reflect = |p: [f64; 2]| {
            let t = dot(sub(p, a), d) / dd;
            let foot = add(a, scale(d, t));
            sub(scale(foot, 2.0), p)
        };
        let on_axis = |p: [f64; 2]| (cross(d, sub(p, a)) / dd.sqrt()).abs() < 1e-6;
        let mut map: BTreeMap<EntityId, EntityId> = BTreeMap::new();
        let mut out = vec![];
        let mut image = |s: &mut Sketch, p: EntityId, out: &mut Vec<EntityId>| -> EntityId {
            if let Some(m) = map.get(&p) {
                return *m;
            }
            let pos = s.pos(p).unwrap_or([0.0, 0.0]);
            let m = if on_axis(pos) {
                p
            } else {
                let r = reflect(pos);
                let m = s.add_point(r[0], r[1]);
                s.add_constraint(K::Symmetric {
                    a: p,
                    b: m,
                    line: axis,
                });
                out.push(m);
                m
            };
            map.insert(p, m);
            m
        };
        for id in ids {
            if *id == axis {
                continue;
            }
            let Some(e) = self.entity(*id).cloned() else {
                continue;
            };
            let constr = self.is_construction(*id);
            let new = match e {
                Entity::Point { id, .. } => {
                    image(self, id, &mut out);
                    continue;
                }
                Entity::Line { a, b, .. } => {
                    let (ma, mb) = (image(self, a, &mut out), image(self, b, &mut out));
                    self.add_line(ma, mb)
                }
                Entity::Circle { center, radius, .. } => {
                    let mc = image(self, center, &mut out);
                    let c = self.add_circle(mc, radius);
                    self.add_constraint(K::Equal { a: *id, b: c });
                    c
                }
                // 写すと回る向きが逆になるので、始点と終点を入れ替える
                Entity::Arc {
                    center, start, end, ..
                } => {
                    let mc = image(self, center, &mut out);
                    let ms = image(self, end, &mut out);
                    let me = image(self, start, &mut out);
                    self.add_arc(mc, ms, me)
                }
                // 楕円弧も、写すと回る向きが逆になる
                Entity::Ellipse { ends, .. } => {
                    let mut m = BTreeMap::new();
                    for p in e.points() {
                        m.insert(p, image(self, p, &mut out));
                    }
                    let mut n = e.with_points(EntityId(0), &|p| m[&p]);
                    if let Entity::Ellipse { ends: ne, .. } = &mut n
                        && let Some((s, t)) = ends
                    {
                        *ne = Some((m[&t], m[&s]));
                    }
                    self.add_entity(&n)
                }
                Entity::Spline { .. } | Entity::Conic { .. } => {
                    let mut m = BTreeMap::new();
                    for p in e.points() {
                        m.insert(p, image(self, p, &mut out));
                    }
                    self.add_entity(&e.with_points(EntityId(0), &|p| m[&p]))
                }
            };
            self.set_construction(new, constr);
            out.push(new);
        }
        out
    }

    /// 曲線 `target` の、`at` に近い区間(ほかの曲線との交点で区切った 1 区間)を消す。
    /// 交点のない線分と、交点が 2 つ未満の円はまるごと消す。変えたら真を返す。
    pub fn trim(&mut self, target: EntityId, at: [f64; 2]) -> bool {
        let Some(curve) = self.curve(target) else {
            return false;
        };
        // 交点と、それを作った曲線
        let mut cuts: Vec<([f64; 2], EntityId)> = vec![];
        for e in &self.entities {
            let id = e.id();
            if id == target || self.own_axis(target, id) {
                continue;
            }
            if let Some(c) = self.curve(id) {
                cuts.extend(intersect(&curve, &c).into_iter().map(|p| (p, id)));
            }
        }
        match self.entity(target).cloned() {
            Some(Entity::Line { a, b, .. }) => {
                let (pa, pb) = (self.pos(a).unwrap(), self.pos(b).unwrap());
                let d = sub(pb, pa);
                let dd = dot(d, d).max(EPS);
                let param = |p: [f64; 2]| dot(sub(p, pa), d) / dd;
                let tc = param(at).clamp(0.0, 1.0);
                let inner: Vec<(f64, [f64; 2], EntityId)> = cuts
                    .iter()
                    .map(|(p, by)| (param(*p), *p, *by))
                    .filter(|(t, ..)| *t > 1e-6 && *t < 1.0 - 1e-6)
                    .collect();
                let lo = inner
                    .iter()
                    .filter(|c| c.0 < tc)
                    .max_by(|x, y| x.0.total_cmp(&y.0));
                let hi = inner
                    .iter()
                    .filter(|c| c.0 > tc)
                    .min_by(|x, y| x.0.total_cmp(&y.0));
                if lo.is_none() && hi.is_none() {
                    self.remove_entities(&[target]);
                    return true;
                }
                let mut pieces = vec![];
                if let Some(&(_, p, by)) = lo {
                    let m = self.add_point(p[0], p[1]);
                    self.attach(m, by);
                    pieces.push(self.add_line(a, m));
                }
                if let Some(&(_, p, by)) = hi {
                    let m = self.add_point(p[0], p[1]);
                    self.attach(m, by);
                    pieces.push(self.add_line(m, b));
                }
                let constr = self.is_construction(target);
                for p in &pieces {
                    self.set_construction(*p, constr);
                }
                self.retarget_pieces(target, &pieces);
                // 2 つに分かれたら、後ろの片を前の片と一直線に保つ
                if let [first, second] = pieces[..]
                    && let Some(Entity::Line { a: sa, b: sb, .. }) = self.entity(second).cloned()
                {
                    self.add_constraint(K::PointOnLine {
                        point: sa,
                        line: first,
                    });
                    self.add_constraint(K::PointOnLine {
                        point: sb,
                        line: first,
                    });
                }
                self.replace_curve(target, pieces[0]);
                true
            }
            Some(Entity::Circle { center, .. }) => {
                let c = self.pos(center).unwrap();
                let ac = angle_of(c, at);
                let mut angles: Vec<(f64, [f64; 2], EntityId)> = cuts
                    .iter()
                    .map(|(p, by)| ((angle_of(c, *p) - ac).rem_euclid(TAU), *p, *by))
                    .collect();
                angles.sort_by(|x, y| x.0.total_cmp(&y.0));
                angles.dedup_by(|x, y| (x.0 - y.0).abs() < 1e-9);
                if angles.len() < 2 {
                    self.remove_entities(&[target]);
                    return true;
                }
                // 押した所から反時計回りに最初の交点が残す円弧の始点、最後の交点が終点
                let (s, e) = (angles[0], angles[angles.len() - 1]);
                let ms = self.add_point(s.1[0], s.1[1]);
                let me = self.add_point(e.1[0], e.1[1]);
                self.attach(ms, s.2);
                self.attach(me, e.2);
                let arc = self.add_arc(center, ms, me);
                let constr = self.is_construction(target);
                self.set_construction(arc, constr);
                self.retarget_pieces(target, &[arc]);
                self.replace_curve(target, arc);
                true
            }
            Some(Entity::Arc {
                center, start, end, ..
            }) => {
                let Curve::Arc(c, _, a0, span) = curve else {
                    return false;
                };
                let u = |p: [f64; 2]| (angle_of(c, p) - a0).rem_euclid(TAU);
                let uc = u(at).min(span);
                let inner: Vec<(f64, [f64; 2], EntityId)> = cuts
                    .iter()
                    .map(|(p, by)| (u(*p), *p, *by))
                    .filter(|(t, ..)| *t > 1e-6 && *t < span - 1e-6)
                    .collect();
                let lo = inner
                    .iter()
                    .filter(|x| x.0 < uc)
                    .max_by(|x, y| x.0.total_cmp(&y.0));
                let hi = inner
                    .iter()
                    .filter(|x| x.0 > uc)
                    .min_by(|x, y| x.0.total_cmp(&y.0));
                if lo.is_none() && hi.is_none() {
                    self.remove_entities(&[target]);
                    return true;
                }
                let mut pieces = vec![];
                if let Some(&(_, p, by)) = lo {
                    let m = self.add_point(p[0], p[1]);
                    self.attach(m, by);
                    pieces.push(self.add_arc(center, start, m));
                }
                if let Some(&(_, p, by)) = hi {
                    let m = self.add_point(p[0], p[1]);
                    self.attach(m, by);
                    pieces.push(self.add_arc(center, m, end));
                }
                let constr = self.is_construction(target);
                for p in &pieces {
                    self.set_construction(*p, constr);
                }
                if let [first, second] = pieces[..] {
                    self.add_constraint(K::Equal {
                        a: first,
                        b: second,
                    });
                }
                self.retarget_pieces(target, &pieces);
                self.replace_curve(target, pieces[0]);
                true
            }
            Some(Entity::Ellipse { .. } | Entity::Spline { .. } | Entity::Conic { .. }) => {
                self.trim_free(target, at, &cuts)
            }
            _ => false,
        }
    }

    /// `other` が楕円 `target` 自身の軸の補助線か(軸の端はいつも楕円の上にあるので、
    /// トリムの切れ目にしない)。
    pub(crate) fn own_axis(&self, target: EntityId, other: EntityId) -> bool {
        let (Some(Entity::Ellipse { center, .. }), Some(Entity::Line { a, b, .. })) =
            (self.entity(target), self.entity(other))
        else {
            return false;
        };
        self.is_construction(other) && (a == center || b == center)
    }

    /// 交点の点 `p` を、交わった曲線 `by` の上に拘束する。
    pub(crate) fn attach(&mut self, p: EntityId, by: EntityId) {
        match self.entity(by) {
            Some(Entity::Line { .. }) => {
                self.add_constraint(K::PointOnLine { point: p, line: by });
            }
            Some(Entity::Circle { .. } | Entity::Arc { .. }) => {
                self.add_constraint(K::PointOnCircle {
                    point: p,
                    circle: by,
                });
            }
            Some(Entity::Ellipse { .. } | Entity::Spline { .. } | Entity::Conic { .. }) => {
                self.add_constraint(K::PointOnCurve {
                    point: p,
                    curve: by,
                });
            }
            _ => {}
        }
    }

    /// 切った曲線 `old` を指す、曲線の上の点・接する・曲率の拘束を、それが当てはまる片
    /// (点が上にある片、相手と端を共有する片)へ付け替える。当てはまる片がなければ外す。
    pub(crate) fn retarget_pieces(&mut self, old: EntityId, pieces: &[EntityId]) {
        let on_piece = |s: &Sketch, point: EntityId, piece: EntityId| {
            let (Some(p), Some(sh)) = (s.pos(point), s.shape(piece)) else {
                return false;
            };
            sh.closest(p).1 < 1e-6
        };
        let shares_end = |s: &Sketch, piece: EntityId, other: EntityId| {
            let (Some(a), Some(b)) = (
                s.entity(piece).and_then(Entity::ends),
                s.entity(other).and_then(Entity::ends),
            ) else {
                return false;
            };
            [a.0, a.1].iter().any(|p| *p == b.0 || *p == b.1)
        };
        let mut gone = vec![];
        let mut kinds = vec![];
        for c in &self.constraints {
            let kind = match &c.kind {
                K::PointOnCurve { point, curve } if *curve == old => pieces
                    .iter()
                    .find(|q| on_piece(self, *point, **q))
                    .map(|q| K::PointOnCurve {
                        point: *point,
                        curve: *q,
                    }),
                K::TangentCurve { a, b } | K::Curvature { a, b } if *a == old || *b == old => {
                    let other = if *a == old { *b } else { *a };
                    pieces
                        .iter()
                        .find(|q| shares_end(self, **q, other))
                        .or(match &c.kind {
                            // 端でなく触れる接し方は、どの片でもよい
                            K::TangentCurve { .. } => pieces.first(),
                            _ => None,
                        })
                        .map(|q| {
                            let (na, nb) = if *a == old { (*q, *b) } else { (*a, *q) };
                            match c.kind {
                                K::TangentCurve { .. } => K::TangentCurve { a: na, b: nb },
                                _ => K::Curvature { a: na, b: nb },
                            }
                        })
                }
                _ => continue,
            };
            match kind {
                Some(k) => kinds.push((c.id, k)),
                None => gone.push(c.id),
            }
        }
        for (id, k) in kinds {
            if let Some(c) = self.constraints.iter_mut().find(|c| c.id == id) {
                c.kind = k;
            }
        }
        self.constraints.retain(|c| !gone.contains(&c.id));
    }

    /// 曲線 `old` を消し、向きや大きさの拘束を `new` に付け替える。
    /// 長さや中点のように、切って意味の変わる拘束は外す。
    pub(crate) fn replace_curve(&mut self, old: EntityId, new: EntityId) {
        let swap = |x: &mut EntityId| {
            if *x == old {
                *x = new;
            }
        };
        let mut keep = vec![];
        for mut c in std::mem::take(&mut self.constraints) {
            let touches = crate::refs(&c.kind).contains(&old);
            if !touches {
                keep.push(c);
                continue;
            }
            let ok = match &mut c.kind {
                K::Horizontal { line } | K::Vertical { line } => {
                    swap(line);
                    true
                }
                K::Parallel { a, b } | K::Perpendicular { a, b } | K::Angle { a, b, .. } => {
                    swap(a);
                    swap(b);
                    true
                }
                K::PointOnLine { line, .. } | K::PointLineDistance { line, .. } => {
                    swap(line);
                    true
                }
                K::Tangent { line, circle } => {
                    swap(line);
                    swap(circle);
                    true
                }
                K::PointOnCircle { circle, .. }
                | K::Radius { circle, .. }
                | K::Diameter { circle, .. } => {
                    swap(circle);
                    true
                }
                K::Equal { a, b } => {
                    // 円の等しい半径は残す(線分の長さは変わるので外す)
                    let round = matches!(
                        self.entity(new),
                        Some(Entity::Arc { .. } | Entity::Circle { .. })
                    );
                    swap(a);
                    swap(b);
                    round
                }
                K::Offset { parts } => {
                    // オフセットのまとまりは 1 つの拘束なので、外すと同じまとまりの他の線の関係まで
                    // 黙って消える。平行・距離・半径・角の円弧は切った片でも成り立つので付け替え、
                    // 共有する端で接する関係だけ(片が端を持たないことがある)を外す
                    parts.retain(
                        |p| !matches!(p, OffsetPart::Tangent { a, b } if *a == old || *b == old),
                    );
                    for p in parts.iter_mut() {
                        match p {
                            OffsetPart::Parallel { from, to }
                            | OffsetPart::Distance { from, to }
                            | OffsetPart::Radius { from, to } => {
                                swap(from);
                                swap(to);
                            }
                            OffsetPart::Corner { arc } => swap(arc),
                            OffsetPart::Tangent { .. } => {}
                        }
                    }
                    !parts.is_empty()
                }
                _ => false,
            };
            if ok {
                keep.push(c);
            }
        }
        self.constraints = keep;
        // 消す曲線が使っていた点(離れた所の単独の点は、この曲線と関係がないので残す)
        let had: Vec<EntityId> = self.entity(old).map(crate::edit::uses).unwrap_or_default();
        self.entities.retain(|e| e.id() != old);
        // 使われなくなった点は消す
        let used: std::collections::BTreeSet<EntityId> =
            self.entities.iter().flat_map(crate::edit::uses).collect();
        let orphan: Vec<EntityId> = had
            .into_iter()
            .filter(|id| !used.contains(id) && !self.was_lone(*id))
            .collect();
        if !orphan.is_empty() {
            self.remove_entities(&orphan);
        }
    }

    /// 点が、図形の端ではなく単独の点として置かれていたか(拘束の参照があれば残す目安)。
    fn was_lone(&self, id: EntityId) -> bool {
        self.constraints
            .iter()
            .any(|c| matches!(c.kind, K::Fix { point } if point == id))
    }
}

#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;
