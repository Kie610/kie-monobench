//! 輪郭の端の手当て。どの線ともつながらない端(押し出せない理由)と、位置は重なるのに一致・線上の
//! 拘束で結ばれていない端を数え、「端をつなぐ」で拘束を付ける。
//!
//! 近い端どうしは 1 対 1 に組む(近い順に組み、組んだ端は使わない)。代表の点を選んで他の点の
//! 関係を捨てる作りにはしない(docs/避ける形.md)。

use crate::model::{ConstraintKind, Entity, EntityId, Sketch};
use crate::profile::open_ends_of;
use crate::solver::Solution;
use std::collections::BTreeMap;

/// 位置が同じとみなす距離(mm)。
const SAME: f64 = 1e-6;

/// 端の手当ての数え。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LooseEnds {
    /// どの線ともつながらない端の点
    pub open: Vec<EntityId>,
    /// 位置だけで接する端に付ける拘束(一致・線上・円周上)
    pub untied: Vec<ConstraintKind>,
}

/// 補助線でない曲線の端(点、その曲線)。
fn curve_ends(sketch: &Sketch) -> Vec<(EntityId, EntityId)> {
    sketch
        .entities
        .iter()
        .filter(|e| !e.is_construction())
        .filter_map(|e| e.ends().map(|(a, b)| [(a, e.id()), (b, e.id())]))
        .flatten()
        .collect()
}

/// 一致の拘束でつながった点の組の代表(同じ組なら同じ値)。
pub(crate) fn tied_groups(sketch: &Sketch) -> BTreeMap<EntityId, EntityId> {
    let mut root: BTreeMap<EntityId, EntityId> = BTreeMap::new();
    fn find(root: &BTreeMap<EntityId, EntityId>, mut p: EntityId) -> EntityId {
        while let Some(q) = root.get(&p).copied().filter(|q| *q != p) {
            p = q;
        }
        p
    }
    for c in &sketch.constraints {
        if let ConstraintKind::Coincident { a, b } = c.kind {
            let (ra, rb) = (find(&root, a), find(&root, b));
            if ra != rb {
                root.insert(ra.max(rb), ra.min(rb));
            }
        }
    }
    let ids: Vec<EntityId> = root.keys().copied().collect();
    ids.into_iter().map(|p| (p, find(&root, p))).collect()
}

/// 点 `p` から曲線 `curve` の途中(端は除く)までの距離。線分・円・円弧だけ。
pub(crate) fn to_curve(
    sketch: &Sketch,
    sol: &Solution,
    curve: EntityId,
    p: [f64; 2],
) -> Option<f64> {
    let at = |id: &EntityId| sol.points.get(id).copied();
    match sketch.entity(curve)? {
        Entity::Line { a, b, .. } => {
            let (a, b) = (at(a)?, at(b)?);
            let d = [b[0] - a[0], b[1] - a[1]];
            let dd = d[0] * d[0] + d[1] * d[1];
            if dd < SAME {
                return None;
            }
            let t = ((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / dd;
            let inside = SAME / dd.sqrt();
            (inside..=1.0 - inside).contains(&t).then(|| {
                let f = [a[0] + d[0] * t, a[1] + d[1] * t];
                (p[0] - f[0]).hypot(p[1] - f[1])
            })
        }
        Entity::Circle { id, center, .. } => {
            let (c, r) = (at(center)?, *sol.radii.get(id)?);
            Some(((p[0] - c[0]).hypot(p[1] - c[1]) - r).abs())
        }
        Entity::Arc {
            center, start, end, ..
        } => {
            let (c, s, e) = (at(center)?, at(start)?, at(end)?);
            let r = (s[0] - c[0]).hypot(s[1] - c[1]);
            let ang = |q: [f64; 2]| {
                (q[1] - c[1])
                    .atan2(q[0] - c[0])
                    .rem_euclid(std::f64::consts::TAU)
            };
            let (a0, a1, a) = (ang(s), ang(e), ang(p));
            let sweep = (a1 - a0).rem_euclid(std::f64::consts::TAU);
            let rel = (a - a0).rem_euclid(std::f64::consts::TAU);
            (rel > 1e-9 && rel < sweep - 1e-9).then(|| ((p[0] - c[0]).hypot(p[1] - c[1]) - r).abs())
        }
        _ => None,
    }
}

/// 端 `point` が曲線 `curve` の上にあると決める拘束が、もう付いているか。
fn already_on(sketch: &Sketch, point: EntityId, curve: EntityId) -> bool {
    sketch.constraints.iter().any(|c| match c.kind {
        ConstraintKind::PointOnLine { point: p, line } => p == point && line == curve,
        ConstraintKind::PointOnCircle { point: p, circle } => p == point && circle == curve,
        ConstraintKind::Midpoint { point: p, line } => p == point && line == curve,
        _ => false,
    })
}

/// 点を曲線の上に置く拘束(線分なら線上、円・円弧なら円周上)。
pub fn on_curve(sketch: &Sketch, point: EntityId, curve: EntityId) -> ConstraintKind {
    match sketch.entity(curve) {
        Some(Entity::Line { .. }) => ConstraintKind::PointOnLine { point, line: curve },
        _ => ConstraintKind::PointOnCircle {
            point,
            circle: curve,
        },
    }
}

/// 開いた端と、位置だけで接する端を数える。
pub fn loose_ends(sketch: &Sketch, sol: &Solution) -> LooseEnds {
    let ends = curve_ends(sketch);
    let pos = |p: EntityId| sol.points.get(&p).copied();
    let open: Vec<EntityId> = {
        let spots = open_ends_of(sketch, sol);
        let mut out: Vec<EntityId> = ends
            .iter()
            .filter_map(|(p, _)| {
                let q = pos(*p)?;
                spots
                    .iter()
                    .any(|s| (s[0] - q[0]).hypot(s[1] - q[1]) < 1e-6)
                    .then_some(*p)
            })
            .collect();
        out.sort();
        out.dedup();
        out
    };
    let groups = tied_groups(sketch);
    let group = |p: EntityId| groups.get(&p).copied().unwrap_or(p);
    let mut untied = Vec::new();
    let mut seen: Vec<(EntityId, EntityId)> = Vec::new();
    for (i, (p, pc)) in ends.iter().enumerate() {
        let Some(pp) = pos(*p) else { continue };
        // 別の曲線の端と重なる(同じ点でも、一致でつながってもいない)
        for (q, qc) in &ends[i + 1..] {
            let Some(qp) = pos(*q) else { continue };
            let pair = (group(*p).min(group(*q)), group(*p).max(group(*q)));
            if pc == qc
                || group(*p) == group(*q)
                || (pp[0] - qp[0]).hypot(pp[1] - qp[1]) >= SAME
                || seen.contains(&pair)
            {
                continue;
            }
            seen.push(pair);
            untied.push(ConstraintKind::Coincident { a: *p, b: *q });
        }
        // 別の曲線の途中に乗る
        for e in sketch.entities.iter().filter(|e| !e.is_construction()) {
            let c = e.id();
            if c == *pc || e.points().contains(p) || already_on(sketch, *p, c) {
                continue;
            }
            if to_curve(sketch, sol, c, pp).is_some_and(|d| d < SAME) {
                untied.push(on_curve(sketch, *p, c));
            }
        }
    }
    LooseEnds { open, untied }
}

/// 「端をつなぐ」で付ける拘束: 位置だけで接する端の一致・線上と、`gap` mm より近い開いた端どうしの
/// 一致(近い順に 1 対 1 に組む)。
pub fn tie_ends(sketch: &Sketch, sol: &Solution, gap: f64) -> Vec<ConstraintKind> {
    let loose = loose_ends(sketch, sol);
    let mut out = loose.untied;
    let owner: BTreeMap<EntityId, EntityId> = curve_ends(sketch).into_iter().collect();
    let pos = |p: EntityId| sol.points.get(&p).copied();
    let mut pairs: Vec<(f64, EntityId, EntityId)> = Vec::new();
    for (i, p) in loose.open.iter().enumerate() {
        for q in &loose.open[i + 1..] {
            if owner.get(p) == owner.get(q) {
                continue;
            }
            if let (Some(a), Some(b)) = (pos(*p), pos(*q)) {
                let d = (a[0] - b[0]).hypot(a[1] - b[1]);
                if d <= gap {
                    pairs.push((d, *p, *q));
                }
            }
        }
    }
    pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut used: Vec<EntityId> = Vec::new();
    for (_, p, q) in pairs {
        if used.contains(&p) || used.contains(&q) {
            continue;
        }
        used.extend([p, q]);
        out.push(ConstraintKind::Coincident { a: p, b: q });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn solved(s: &Sketch) -> Solution {
        crate::solve(s, &HashMap::new()).unwrap()
    }

    /// 0.05 mm の隙間が 1 か所ある 3 本の線(コの字に閉じる線をもう 1 本)。
    fn gapped() -> Sketch {
        let mut s = Sketch::default();
        let a = s.add_point(0.0, 0.0);
        let b = s.add_point(10.0, 0.0);
        let b2 = s.add_point(10.0, 0.0);
        let c = s.add_point(10.0, 10.0);
        let c2 = s.add_point(10.0, 10.05);
        let d = s.add_point(0.0, 10.0);
        let d2 = s.add_point(0.0, 10.0);
        let a2 = s.add_point(0.0, 0.0);
        s.add_line(a, b);
        s.add_line(b2, c);
        s.add_line(c2, d);
        s.add_line(d2, a2);
        // b と b2 は一致でつながっている。d と d2、a2 と a は位置だけで接する
        s.add_constraint(ConstraintKind::Coincident { a: b, b: b2 });
        s
    }

    // 確かめること:
    // - 開いた端(隙間の両側)と位置だけで接する端が数えられる
    // - 近い端をつなぐと輪が閉じ、数が 0 になって領域が 1 つできる
    #[test]
    fn loose_ends_are_counted_and_tying_closes_the_loop() {
        let mut s = gapped();
        let sol = solved(&s);
        let loose = loose_ends(&s, &sol);
        assert_eq!(loose.open.len(), 2, "隙間の両側");
        assert_eq!(loose.untied.len(), 2, "位置だけで接する 2 か所");
        assert!(
            tie_ends(&s, &sol, 0.01).len() == 2,
            "隙間より短い距離ではつながない"
        );
        for k in tie_ends(&s, &sol, 0.1) {
            s.add_constraint(k);
        }
        let sol = solved(&s);
        let loose = loose_ends(&s, &sol);
        assert!(
            loose.open.is_empty() && loose.untied.is_empty(),
            "{loose:?}"
        );
        let (p, _) = crate::profile_of(&s, &sol, jc_core::Plane::XY);
        assert_eq!(p.regions.len(), 1, "閉じた領域ができる");
    }

    #[test]
    fn three_close_ends_pair_the_nearest_once() {
        let mut s = Sketch::default();
        for (p, q) in [
            ([0.0, 0.0], [-10.0, 0.0]),
            ([0.03, 0.0], [10.0, 0.0]),
            ([0.0, 0.08], [0.0, 10.0]),
        ] {
            let (p, q) = (s.add_point(p[0], p[1]), s.add_point(q[0], q[1]));
            s.add_line(p, q);
        }
        let sol = solved(&s);
        let ties = tie_ends(&s, &sol, 0.1);
        let coincident = ties
            .iter()
            .filter(|k| matches!(k, ConstraintKind::Coincident { .. }))
            .count();
        assert_eq!(
            coincident, 1,
            "1 対 1 に組み、余った端はつながない: {ties:?}"
        );
    }

    #[test]
    fn an_end_resting_on_a_line_gets_point_on_line() {
        let mut s = Sketch::default();
        let (a, b) = (s.add_point(0.0, 0.0), s.add_point(20.0, 0.0));
        s.add_line(a, b);
        let (c, d) = (s.add_point(5.0, 0.0), s.add_point(5.0, 8.0));
        s.add_line(c, d);
        let sol = solved(&s);
        let loose = loose_ends(&s, &sol);
        assert_eq!(
            loose.untied,
            vec![ConstraintKind::PointOnLine {
                point: c,
                line: EntityId(3)
            }]
        );
    }
}
