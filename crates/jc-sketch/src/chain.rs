//! つながった曲線を拾う([`Sketch::chain_of`])と、トリムで消える区間の予告([`Sketch::trim_preview`])。

use crate::geom::{Curve, angle_of, dot, intersect, sub};
use crate::model::{ConstraintKind as K, Entity, EntityId, Sketch};
use crate::solver::Solution;
use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::TAU;

impl Sketch {
    /// 曲線 `id` と、端でつながった線分・円弧の全部(ダブルクリックで選ぶ連なり)。端の点を
    /// 共有するか、一致の拘束で結ばれていればつながっているとみなす。円はそれだけ。
    /// 曲線でなければ空を返す。
    pub fn chain_of(&self, id: EntityId) -> Vec<EntityId> {
        // 閉じた曲線(円・楕円・閉じたスプライン)は端がないので、それだけ
        let ends = |e: &Entity| {
            e.is_curve()
                .then(|| e.ends().map_or(vec![], |(a, b)| vec![a, b]))
        };
        if self.entity(id).and_then(ends).is_none() {
            return vec![];
        }
        // 一致の拘束で結ばれた点を、同じ点とみなす(代表の点へ寄せる)
        let mut same: BTreeMap<EntityId, EntityId> = BTreeMap::new();
        fn root(m: &BTreeMap<EntityId, EntityId>, mut p: EntityId) -> EntityId {
            while let Some(q) = m.get(&p) {
                if *q == p {
                    break;
                }
                p = *q;
            }
            p
        }
        for c in &self.constraints {
            if let K::Coincident { a, b } = c.kind {
                let (ra, rb) = (root(&same, a), root(&same, b));
                if ra != rb {
                    same.insert(ra.max(rb), ra.min(rb));
                }
            }
        }
        let curves: Vec<(EntityId, Vec<EntityId>)> = self
            .entities
            .iter()
            .filter_map(|e| {
                Some((
                    e.id(),
                    ends(e)?.into_iter().map(|p| root(&same, p)).collect(),
                ))
            })
            .collect();
        let mut out: BTreeSet<EntityId> = BTreeSet::from([id]);
        let mut todo = vec![id];
        while let Some(cur) = todo.pop() {
            let Some((_, pts)) = curves.iter().find(|(c, _)| *c == cur) else {
                continue;
            };
            let pts = pts.clone();
            for (other, ps) in &curves {
                if !out.contains(other) && ps.iter().any(|p| pts.contains(p)) {
                    out.insert(*other);
                    todo.push(*other);
                }
            }
        }
        out.into_iter().collect()
    }

    /// トリムの道具で `target` の `at` に近い区間を押した時に、消える区間(折れ線、スケッチの座標)。
    /// [`Sketch::trim`] と同じ決め方で、解けた位置 `sol` で求める。曲線でなければ `None`。
    pub fn trim_preview(
        &self,
        sol: &Solution,
        target: EntityId,
        at: [f64; 2],
    ) -> Option<Vec<[f64; 2]>> {
        let mut s = self.clone();
        s.adopt_solution(sol);
        let curve = s.curve(target)?;
        if let Curve::Free(_) = curve {
            let cuts: Vec<([f64; 2], EntityId)> = s
                .entities
                .iter()
                .filter(|e| e.id() != target && !s.own_axis(target, e.id()))
                .filter_map(|e| Some((e.id(), s.curve(e.id())?)))
                .flat_map(|(id, c)| intersect(&curve, &c).into_iter().map(move |p| (p, id)))
                .collect();
            return s.trim_free_preview(target, at, &cuts);
        }
        let cuts: Vec<[f64; 2]> = s
            .entities
            .iter()
            .filter(|e| e.id() != target)
            .filter_map(|e| s.curve(e.id()))
            .flat_map(|c| intersect(&curve, &c))
            .collect();
        let arc = |c: [f64; 2], r: f64, a0: f64, a1: f64| -> Vec<[f64; 2]> {
            let n = (((a1 - a0).abs() / TAU) * 96.0).ceil().max(2.0) as usize;
            (0..=n)
                .map(|i| {
                    let a = a0 + (a1 - a0) * i as f64 / n as f64;
                    [c[0] + r * a.cos(), c[1] + r * a.sin()]
                })
                .collect()
        };
        Some(match (s.entity(target)?, curve) {
            (Entity::Line { .. }, Curve::Seg(pa, pb)) => {
                let d = sub(pb, pa);
                let dd = dot(d, d).max(1e-12);
                let param = |p: [f64; 2]| dot(sub(p, pa), d) / dd;
                let tc = param(at).clamp(0.0, 1.0);
                let inner: Vec<f64> = cuts
                    .iter()
                    .map(|p| param(*p))
                    .filter(|t| *t > 1e-6 && *t < 1.0 - 1e-6)
                    .collect();
                let lo = inner
                    .iter()
                    .copied()
                    .filter(|t| *t < tc)
                    .fold(0.0, f64::max);
                let hi = inner
                    .iter()
                    .copied()
                    .filter(|t| *t > tc)
                    .fold(1.0, f64::min);
                let p = |t: f64| [pa[0] + d[0] * t, pa[1] + d[1] * t];
                vec![p(lo), p(hi)]
            }
            (Entity::Circle { .. }, Curve::Arc(c, r, ..)) => {
                let ac = angle_of(c, at);
                let mut rel: Vec<f64> = cuts
                    .iter()
                    .map(|p| (angle_of(c, *p) - ac).rem_euclid(TAU))
                    .collect();
                rel.sort_by(f64::total_cmp);
                rel.dedup_by(|x, y| (*x - *y).abs() < 1e-9);
                if rel.len() < 2 {
                    arc(c, r, 0.0, TAU)
                } else {
                    // 押した所の時計回りに最も近い交点から、反時計回りに最も近い交点まで
                    let (first, last) = (rel[0], rel[rel.len() - 1]);
                    arc(c, r, ac + last - TAU, ac + first)
                }
            }
            (Entity::Arc { .. }, Curve::Arc(c, r, a0, span)) => {
                let u = |p: [f64; 2]| (angle_of(c, p) - a0).rem_euclid(TAU);
                let uc = u(at).min(span);
                let inner: Vec<f64> = cuts
                    .iter()
                    .map(|p| u(*p))
                    .filter(|t| *t > 1e-6 && *t < span - 1e-6)
                    .collect();
                let lo = inner
                    .iter()
                    .copied()
                    .filter(|t| *t < uc)
                    .fold(0.0, f64::max);
                let hi = inner
                    .iter()
                    .copied()
                    .filter(|t| *t > uc)
                    .fold(span, f64::min);
                arc(c, r, a0 + lo, a0 + hi)
            }
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn solved(s: &Sketch) -> Solution {
        crate::solve(s, &HashMap::new()).unwrap()
    }

    #[test]
    fn a_double_click_chain_follows_shared_and_coincident_ends() {
        let mut s = Sketch::default();
        let l = s
            .add_rectangle((None, [0.0, 0.0]), (None, [10.0, 10.0]))
            .unwrap();
        // 離れた線分は別の連なり
        let (p, q) = (s.add_point(20.0, 0.0), s.add_point(30.0, 0.0));
        let lone = s.add_line(p, q);
        // 一致の拘束でつないだ線分は同じ連なり
        let (r, t) = (s.add_point(30.0, 0.0), s.add_point(30.0, 10.0));
        let joined = s.add_line(r, t);
        s.add_constraint(K::Coincident { a: q, b: r });
        let mut chain = s.chain_of(l[2]);
        chain.sort();
        let mut want = l.to_vec();
        want.sort();
        assert_eq!(chain, want);
        assert_eq!(s.chain_of(lone), vec![lone, joined]);
        assert!(s.chain_of(p).is_empty(), "点は曲線でない");
    }

    #[test]
    fn the_trim_preview_is_the_span_between_the_nearest_crossings() {
        let mut s = Sketch::default();
        let (a, b) = (s.add_point(0.0, 0.0), s.add_point(30.0, 0.0));
        let target = s.add_line(a, b);
        for x in [10.0, 20.0] {
            let (p, q) = (s.add_point(x, -5.0), s.add_point(x, 5.0));
            s.add_line(p, q);
        }
        let sol = solved(&s);
        let span = s.trim_preview(&sol, target, [15.0, 0.0]).unwrap();
        assert!((span[0][0] - 10.0).abs() < 1e-9 && (span[1][0] - 20.0).abs() < 1e-9);
        let span = s.trim_preview(&sol, target, [3.0, 0.0]).unwrap();
        assert!(span[0][0].abs() < 1e-9 && (span[1][0] - 10.0).abs() < 1e-9);
        // 予告と同じ区間が消える
        let mut cut = s.clone();
        cut.adopt_solution(&sol);
        assert!(cut.trim(target, [15.0, 0.0]));
        let xs: Vec<f64> = cut
            .entities
            .iter()
            .filter_map(|e| match e {
                Entity::Point { x, y, .. } if y.abs() < 1e-9 => Some(*x),
                _ => None,
            })
            .collect();
        assert!(
            xs.iter().any(|x| (x - 10.0).abs() < 1e-9)
                && xs.iter().any(|x| (x - 20.0).abs() < 1e-9)
        );
    }

    #[test]
    fn a_circle_crossed_twice_previews_the_arc_under_the_cursor() {
        let mut s = Sketch::default();
        let c = s.add_point(0.0, 0.0);
        let circle = s.add_circle(c, 10.0);
        let (p, q) = (s.add_point(-20.0, 0.0), s.add_point(20.0, 0.0));
        s.add_line(p, q);
        let sol = solved(&s);
        // 上側を押すと、上の半円が消える
        let span = s.trim_preview(&sol, circle, [0.0, 10.0]).unwrap();
        assert!(span.iter().all(|p| p[1] >= -1e-9), "{span:?}");
        let mid = span[span.len() / 2];
        assert!((mid[1] - 10.0).abs() < 0.1, "{mid:?}");
    }
}
