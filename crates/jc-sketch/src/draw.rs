//! 作図の道具(基本の矩形・長穴・多角形・ミラー・トリムは tools.rs): 3 点指定の長方形、2 点・3 点・3 本の線に接する円、3 点の円弧、外接の多角形、
//! 全長・中心からの長穴、円弧に沿う長穴、角の面取り。どれも保存されている座標
//! (先に [`Sketch::adopt_solution`] で解いた位置を写しておく)を元に要素と拘束を足す。

use crate::geom::{EPS, add, angle_of, cross, dot, len, scale, sub};
use crate::model::{ConstraintKind as K, Entity, EntityId, Sketch};
use crate::tools::Spot;
use std::f64::consts::{PI, TAU};

/// 3 点を通る円の中心。3 点が一直線に近ければ `None`。
pub fn circumcenter(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> Option<[f64; 2]> {
    let (ab, ac) = (sub(b, a), sub(c, a));
    let (bl, cl) = (dot(ab, ab), dot(ac, ac));
    let d = 2.0 * cross(ab, ac);
    if d.abs() < 1e-9 * (bl + cl).max(EPS) {
        return None;
    }
    let u = [(ac[1] * bl - ab[1] * cl) / d, (ab[0] * cl - ac[0] * bl) / d];
    Some(add(a, u))
}

/// 3 本の直線(線分を延ばしたもの)に接する円のうち、中心が `hint` に最も近いもの(中心, 半径)。
/// 内接円と 3 つの傍接円が候補(平行な線があれば減る)。描けなければ `None`。
pub fn circle_tangent_to_lines(
    lines: [([f64; 2], [f64; 2]); 3],
    hint: [f64; 2],
) -> Option<([f64; 2], f64)> {
    // 直線 i: n_i · x = d_i(n_i は単位法線)。中心 c と半径 r は n_i · c - s_i r = d_i を満たす
    let mut rows = [[0.0; 3]; 3];
    let mut rhs = [0.0; 3];
    for (i, (a, b)) in lines.iter().enumerate() {
        let d = sub(*b, *a);
        let l = len(d);
        if l < EPS {
            return None;
        }
        let n = [-d[1] / l, d[0] / l];
        rows[i] = [n[0], n[1], 0.0];
        rhs[i] = dot(n, *a);
    }
    let det3 = |m: [[f64; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let mut best: Option<([f64; 2], f64)> = None;
    for signs in 0..8u32 {
        let mut m = rows;
        for (i, row) in m.iter_mut().enumerate() {
            row[2] = if signs & (1 << i) == 0 { -1.0 } else { 1.0 };
        }
        let det = det3(m);
        if det.abs() < 1e-12 {
            continue;
        }
        // クラメルの公式
        let solve = |col: usize| {
            let mut k = m;
            for (i, row) in k.iter_mut().enumerate() {
                row[col] = rhs[i];
            }
            det3(k) / det
        };
        let (c, r) = ([solve(0), solve(1)], solve(2));
        if r > 1e-6 && best.is_none_or(|(bc, _)| len(sub(c, hint)) < len(sub(bc, hint)) - 1e-9) {
            best = Some((c, r));
        }
    }
    best
}

impl Sketch {
    /// 円・円弧の上の点にする(吸着した既存の点だけ)。
    fn on_round(&mut self, s: Spot, round: EntityId) {
        if let Some(p) = s.0 {
            self.add_constraint(K::PointOnCircle {
                point: p,
                circle: round,
            });
        }
    }

    /// 1 辺の両端 `a`、`b` と、向かいの辺の位置 `c` から、傾いた矩形を描く。
    /// 隣の辺は直角、向かいの辺は平行にする。足した 4 本の線分を(a-b、b 側、向かい、a 側)の順で返す。
    pub fn add_three_point_rectangle(
        &mut self,
        a: Spot,
        b: Spot,
        c: [f64; 2],
    ) -> Option<[EntityId; 4]> {
        let (p, q) = (a.1, b.1);
        let d = sub(q, p);
        let l = len(d);
        if l < 1e-6 {
            return None;
        }
        let n = [-d[1] / l, d[0] / l];
        let h = dot(sub(c, p), n);
        if h.abs() < 1e-6 {
            return None;
        }
        let off = scale(n, h);
        let p0 = self.spot(a);
        let p1 = self.spot(b);
        if p0 == p1 {
            return None;
        }
        let (r, s) = (add(q, off), add(p, off));
        let p2 = self.add_point(r[0], r[1]);
        let p3 = self.add_point(s[0], s[1]);
        let l = [
            self.add_line(p0, p1),
            self.add_line(p1, p2),
            self.add_line(p2, p3),
            self.add_line(p3, p0),
        ];
        self.add_constraint(K::Perpendicular { a: l[0], b: l[1] });
        self.add_constraint(K::Parallel { a: l[0], b: l[2] });
        self.add_constraint(K::Parallel { a: l[1], b: l[3] });
        Some(l)
    }

    /// 直径の両端 `a`、`b` から円を描く(2 点の円)。吸着した点は円周の上に拘束する。
    pub fn add_circle_two_points(&mut self, a: Spot, b: Spot) -> Option<EntityId> {
        let d = len(sub(b.1, a.1));
        if d < 1e-6 {
            return None;
        }
        let m = scale(add(a.1, b.1), 0.5);
        let c = self.add_point(m[0], m[1]);
        let circle = self.add_circle(c, d / 2.0);
        self.on_round(a, circle);
        self.on_round(b, circle);
        Some(circle)
    }

    /// 円周の上の 3 点から円を描く(3 点の円)。吸着した点は円周の上に拘束する。
    pub fn add_circle_three_points(&mut self, a: Spot, b: Spot, c: Spot) -> Option<EntityId> {
        let m = circumcenter(a.1, b.1, c.1)?;
        let r = len(sub(a.1, m));
        let cp = self.add_point(m[0], m[1]);
        let circle = self.add_circle(cp, r);
        for s in [a, b, c] {
            self.on_round(s, circle);
        }
        Some(circle)
    }

    /// 3 本の線分(を延ばした直線)に接する円を描く。候補のうち中心が `hint` に近いもの。
    /// 3 本それぞれに接する拘束を付ける。描けない(線分でない、平行で候補がない)なら `None`。
    pub fn add_circle_tangent_to_lines(
        &mut self,
        lines: [EntityId; 3],
        hint: [f64; 2],
    ) -> Option<EntityId> {
        let seg = |s: &Sketch, id: EntityId| match s.entity(id)? {
            Entity::Line { a, b, .. } => Some((s.pos(*a)?, s.pos(*b)?)),
            _ => None,
        };
        let segs = [
            seg(self, lines[0])?,
            seg(self, lines[1])?,
            seg(self, lines[2])?,
        ];
        let (c, r) = circle_tangent_to_lines(segs, hint)?;
        let cp = self.add_point(c[0], c[1]);
        let circle = self.add_circle(cp, r);
        for line in lines {
            self.add_constraint(K::Tangent { line, circle });
        }
        Some(circle)
    }

    /// 始点 `start`、終点 `end` と、途中の 1 点 `via` を通る円弧を描く(3 点の円弧)。
    /// 一直線に並ぶなら `None`。途中の点が既にある点に吸い付いていれば、その点を円弧の上に
    /// 拘束する(後で動かしても通ったまま。t5。3 点の円と同じ)。
    pub fn add_arc_three_points(&mut self, start: Spot, end: Spot, via: Spot) -> Option<EntityId> {
        let (s, e, via_spot, via) = (start.1, end.1, via, via.1);
        if len(sub(e, s)) < 1e-6 {
            return None;
        }
        let c = circumcenter(s, e, via)?;
        let a0 = angle_of(c, s);
        let ccw = (angle_of(c, via) - a0).rem_euclid(TAU) < (angle_of(c, e) - a0).rem_euclid(TAU);
        let sp = self.spot(start);
        let ep = self.spot(end);
        if sp == ep {
            return None;
        }
        let cp = self.add_point(c[0], c[1]);
        // 円弧は反時計回りに持つので、途中の点が時計回りの側なら始点と終点を入れ替える
        let arc = if ccw {
            self.add_arc(cp, sp, ep)
        } else {
            self.add_arc(cp, ep, sp)
        };
        if via_spot.0.is_some_and(|p| p != sp && p != ep) {
            self.on_round(via_spot, arc);
        }
        Some(arc)
    }

    /// 中心 `center` と 1 辺の中点 `mid` から、正 `n` 角形を描く(外接の多角形。辺が補助線の
    /// 円に接する)。頂点は外の補助線の円の上に置いて辺の長さを等しくし、最初の辺を内の補助線の
    /// 円に接させる(内の円の直径が二面幅)。足した辺を返す。
    pub fn add_circumscribed_polygon(
        &mut self,
        center: Spot,
        mid: [f64; 2],
        n: usize,
    ) -> Option<Vec<EntityId>> {
        let c = center.1;
        let apothem = len(sub(mid, c));
        if n < 3 || apothem < 1e-6 {
            return None;
        }
        let half = PI / n as f64;
        let big = apothem / half.cos();
        let a0 = angle_of(c, mid) - half;
        let cp = self.spot(center);
        let inner = self.add_circle(cp, apothem);
        self.set_construction(inner, true);
        let outer = self.add_circle(cp, big);
        self.set_construction(outer, true);
        let pts: Vec<EntityId> = (0..n)
            .map(|k| {
                let t = a0 + TAU * k as f64 / n as f64;
                self.add_point(c[0] + big * t.cos(), c[1] + big * t.sin())
            })
            .collect();
        let sides: Vec<EntityId> = (0..n)
            .map(|k| self.add_line(pts[k], pts[(k + 1) % n]))
            .collect();
        self.add_constraint(K::Polygon {
            circle: outer,
            sides: sides.clone(),
        });
        self.add_constraint(K::Tangent {
            line: sides[0],
            circle: inner,
        });
        Some(sides)
    }

    /// 両端の先 `a`、`b`(長穴の全長)と幅の半分 `w` から長穴を描く。全長が幅より短ければ `None`。
    /// 足した要素は [`Sketch::add_slot`] と同じ。
    pub fn add_overall_slot(&mut self, a: Spot, b: Spot, w: f64) -> Option<[EntityId; 4]> {
        let d = sub(b.1, a.1);
        let l = len(d);
        if l <= 2.0 * w + 1e-6 {
            return None;
        }
        let u = scale(d, w / l);
        self.add_slot((None, add(a.1, u)), (None, sub(b.1, u)), w)
    }

    /// 長穴の中心 `center`、片方の端の中心 `end` と幅の半分 `w` から長穴を描く。
    /// 中心は、両端の中心を結ぶ補助線の中点に拘束する。
    pub fn add_center_slot(&mut self, center: Spot, end: Spot, w: f64) -> Option<[EntityId; 4]> {
        let other = sub(scale(center.1, 2.0), end.1);
        let made = self.add_slot((None, other), end, w)?;
        // add_slot は最後に両端の中心を結ぶ補助線を足す
        let axis = self.entities.last()?.id();
        let m = self.spot(center);
        self.add_constraint(K::Midpoint {
            point: m,
            line: axis,
        });
        Some(made)
    }

    /// 円弧に沿う長穴: 中心 `center`、中心線の始まり `start`、終わりの向き `end`(反時計回り)、
    /// 幅の半分 `w`。外と内の円弧と、両端の半円を足す。
    /// 足した要素を(外の円弧、内の円弧、始まりの端、終わりの端)の順で返す。
    pub fn add_arc_slot(
        &mut self,
        center: Spot,
        start: [f64; 2],
        end: [f64; 2],
        w: f64,
    ) -> Option<[EntityId; 4]> {
        self.arc_slot_spots(center, (None, start), (None, end), w)
    }

    /// [`Sketch::add_arc_slot`] の中身。端の円弧の中心に既存の点(`start`・`end` の吸着した点)を
    /// 使える。`end` の点を使う時は、中心線の円の上にあること(3 点円弧の長穴)。
    pub(crate) fn arc_slot_spots(
        &mut self,
        center: Spot,
        start: Spot,
        end: Spot,
        w: f64,
    ) -> Option<[EntityId; 4]> {
        let c = center.1;
        let r = len(sub(start.1, c));
        let a0 = angle_of(c, start.1);
        let span = (angle_of(c, end.1) - a0).rem_euclid(TAU);
        if w < 1e-6 || r <= w + 1e-6 || !(1e-3..TAU - 1e-3).contains(&span) {
            return None;
        }
        if start.0.is_some() && start.0 == end.0 {
            return None;
        }
        let at = |rad: f64, ang: f64| [c[0] + rad * ang.cos(), c[1] + rad * ang.sin()];
        let a1 = a0 + span;
        let cp = self.spot(center);
        let pt = |s: &mut Sketch, p: [f64; 2]| s.add_point(p[0], p[1]);
        let (os, oe) = (pt(self, at(r + w, a0)), pt(self, at(r + w, a1)));
        let (is, ie) = (pt(self, at(r - w, a0)), pt(self, at(r - w, a1)));
        let s0 = start.0.unwrap_or_else(|| pt(self, at(r, a0)));
        let s1 = end.0.unwrap_or_else(|| pt(self, at(r, a1)));
        let outer = self.add_arc(cp, os, oe);
        let inner = self.add_arc(cp, is, ie);
        // 端は長穴の外へふくらむ: 始まりは内から外へ、終わりは外から内へ反時計回り
        let cap0 = self.add_arc(s0, is, os);
        let cap1 = self.add_arc(s1, oe, ie);
        // 4 つの継ぎ目で接する。端の半径が等しいことと中心線の半径は、これで決まる
        // (中心線の補助の円弧や、端の半径が等しい拘束を足すと余分になる)
        for (cap, arc) in [(cap0, outer), (cap0, inner), (cap1, outer), (cap1, inner)] {
            self.add_constraint(K::TangentCircles { a: cap, b: arc });
        }
        Some([outer, inner, cap0, cap1])
    }

    /// 2 本の線分が点 `corner` で作る角を、角から距離 `d` の所で落とす(スケッチの面取り)。
    /// 線分の端を落とした所へ移して、その間に線分を引く。角の点は残し、補助線で両端とつなぐ。
    /// 角の点は両方の線分を延ばした直線の上に拘束し、補助線の長さを寸法 `value` で等しく決める。
    /// 落とせない(角に線分が 2 本でない、まっすぐ、線分が距離に対して短い)なら `None`。
    pub fn chamfer_corner(&mut self, corner: EntityId, d: f64, value: &str) -> Option<EntityId> {
        if d <= EPS {
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
        if n1 <= d + EPS || n2 <= d + EPS {
            return None;
        }
        if cross(d1, d2).abs() / (n1 * n2) < 1e-3 {
            return None;
        }
        let (p1, p2) = (add(c, scale(d1, d / n1)), add(c, scale(d2, d / n2)));
        let t1 = self.add_point(p1[0], p1[1]);
        let t2 = self.add_point(p2[0], p2[1]);
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
        let cut = self.add_line(t1, t2);
        let c1 = self.add_line(corner, t1);
        let c2 = self.add_line(corner, t2);
        self.set_construction(c1, true);
        self.set_construction(c2, true);
        self.add_constraint(K::PointOnLine {
            point: corner,
            line: *l1,
        });
        self.add_constraint(K::PointOnLine {
            point: corner,
            line: *l2,
        });
        self.add_constraint(K::Length {
            line: c1,
            value: value.to_string(),
        });
        self.add_constraint(K::Equal { a: c1, b: c2 });
        Some(cut)
    }
}

#[cfg(test)]
#[path = "draw_tests.rs"]
mod tests;
