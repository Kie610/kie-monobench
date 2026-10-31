//! 吸い付く先(原点・線の中点・既存の形の角と辺と円の中心)。
//!
//! 道具に渡す前に、まだスケッチに無い吸い付く先を仮の要素としてスケッチに足しておき
//! ([`augment`])、道具が使った仮の要素だけを文書に残す([`Augmented::prune`])。
//! こうすると道具ごとの処理を変えずに、原点で描いた形は原点に、中点から引いた線は中点に、
//! 板の角から描いた形は板の角について、元の寸法を変えてもついてくる。

use kmb_sketch::{
    ConstraintId, ConstraintKind, Entity, EntityId, ProjectionSource, RefItem, RefShape, Sketch,
    Solution,
};
use std::collections::{BTreeMap, BTreeSet};

/// 仮に足した吸い付く先の種類(画面の札に出す)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Kind {
    Origin,
    Midpoint,
    Corner,
    Center,
    Edge,
    /// 2 つの曲線の交わる所
    Intersection,
    /// 円・円弧の上下左右の点
    Quadrant,
    /// 円の中心を挟んだ、線の始点の反対側
    Opposite,
    /// 前のスケッチの点(K8)
    SketchPoint,
    /// 前のスケッチの線・円(K8)
    SketchCurve,
}

impl Kind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Kind::Origin => "原点",
            Kind::Midpoint => "中点",
            Kind::Corner => "角",
            Kind::Center => "中心",
            Kind::Edge => "辺",
            Kind::Intersection => "交点",
            Kind::Quadrant => "四分点",
            Kind::Opposite => "反対側",
            Kind::SketchPoint => "スケッチの点",
            Kind::SketchCurve => "スケッチの線",
        }
    }
}

/// 何を吸い付く先にするか。
#[derive(Clone, Copy)]
pub(crate) struct Wants {
    /// 原点・角・円の中心
    pub points: bool,
    /// 既存の形の辺(クリックで選んで寸法や拘束の相手にする)
    pub edges: bool,
    /// スケッチの線分の中点
    pub midpoints: bool,
    /// スケッチの曲線どうしの交点と、円・円弧の四分点
    pub crossings: bool,
}

/// 吸い付く先を仮に足したスケッチと解。
pub(crate) struct Augmented {
    pub sketch: Sketch,
    pub sol: Solution,
    /// 仮に足した要素と、その種類
    pub virtuals: BTreeMap<EntityId, Kind>,
    /// 中点・交点・四分点の仮の点と、それに付けた拘束(使わなければ一緒に除く)
    auto: BTreeMap<EntityId, Vec<ConstraintId>>,
    /// 直径の終点の候補と、始点・中心・円。線に使った時だけ拘束を付ける。
    opposites: BTreeMap<EntityId, (EntityId, EntityId, EntityId)>,
    /// 元のスケッチの要素と拘束の番号の最大(残した要素の番号をここから詰める)
    last: (u32, u32),
}

fn uses(e: &Entity) -> Vec<EntityId> {
    e.points()
}

fn kind_of(source: &ProjectionSource) -> Kind {
    match source {
        ProjectionSource::Origin => Kind::Origin,
        ProjectionSource::Vertex { .. } => Kind::Corner,
        ProjectionSource::Center { .. } => Kind::Center,
        ProjectionSource::Edge { .. }
        | ProjectionSource::Section { .. }
        | ProjectionSource::Silhouette { .. } => Kind::Edge,
        ProjectionSource::Sketch { .. } => Kind::SketchPoint,
    }
}

/// 既にある点と同じ位置とみなす距離(mm)。画面の倍率で変えない(倍率を変えても、描いている
/// 途中に選んだ仮の点の番号が変わらないように)
const SAME: f64 = 1e-6;

/// `sketch` に、`refs`(既存の形を写したもの)と線分の中点のうち、まだ無い吸い付く先を仮に足す。
/// 既にある点と同じ位置の吸い付く先は足さない(今ある点に吸い付けばよい)。
pub(crate) fn augment(
    sketch: &Sketch,
    sol: &Solution,
    refs: &[RefItem],
    wants: Wants,
) -> Augmented {
    augment_with_crossings(sketch, sol, refs, wants, None)
}

/// 作り置いた交点があれば使い、仮の点の並びは作り直す時も同じにする。
pub(crate) fn augment_with_crossings(
    sketch: &Sketch,
    sol: &Solution,
    refs: &[RefItem],
    wants: Wants,
    crossings: Option<&[crate::infer::Crossing]>,
) -> Augmented {
    let mut s = sketch.clone();
    s.adopt_solution(sol);
    let mut sol = sol.clone();
    let mut virtuals = BTreeMap::new();
    let mut auto = BTreeMap::new();
    let near_real = |p: [f64; 2]| {
        sketch.entities.iter().any(|e| match e {
            Entity::Point { id, .. } => {
                sol_point(&sol, *id).is_some_and(|q| (q[0] - p[0]).hypot(q[1] - p[1]) < SAME)
            }
            _ => false,
        })
    };
    let near_real: Vec<bool> = refs
        .iter()
        .map(|r| match r.shape {
            RefShape::Point(p) => near_real(p),
            _ => false,
        })
        .collect();
    let projected =
        |s: &Sketch, r: &RefItem| s.projections.iter().any(|p| p.source.same(&r.source));
    let before: BTreeSet<EntityId> = s.entities.iter().map(|e| e.id()).collect();
    // 点(原点・角・中心)を先に足し、辺の端はそれを使い回す
    if wants.points || wants.edges {
        for (r, near) in refs.iter().zip(&near_real) {
            if matches!(r.shape, RefShape::Point(_)) && !near && !projected(sketch, r) {
                let id = s.add_projection(r);
                virtuals.insert(id, kind_of(&r.source));
            }
        }
    }
    if wants.edges {
        for r in refs {
            if !matches!(r.shape, RefShape::Point(_)) && !projected(sketch, r) {
                let id = s.add_projection(r);
                let kind = if r.source.sketch().is_some() {
                    Kind::SketchCurve
                } else {
                    Kind::Edge
                };
                virtuals.insert(id, kind);
            }
        }
    }
    // 辺を足した時に増えた端の点も仮のもの
    for e in &s.entities {
        if !before.contains(&e.id()) {
            virtuals.entry(e.id()).or_insert(Kind::Corner);
        }
    }
    if wants.midpoints {
        let lines: Vec<(EntityId, EntityId, EntityId)> = sketch
            .entities
            .iter()
            .filter_map(|e| match e {
                Entity::Line { id, a, b, .. } => Some((*id, *a, *b)),
                _ => None,
            })
            .collect();
        for (line, a, b) in lines {
            let (Some(p), Some(q)) = (sol_point(&sol, a), sol_point(&sol, b)) else {
                continue;
            };
            let m = [(p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0];
            let taken = s.entities.iter().any(|e| match e {
                Entity::Point { x, y, .. } => (x - m[0]).hypot(y - m[1]) < SAME,
                _ => false,
            });
            if taken {
                continue;
            }
            let id = s.add_point(m[0], m[1]);
            let c = s.add_constraint(ConstraintKind::Midpoint { point: id, line });
            virtuals.insert(id, Kind::Midpoint);
            auto.insert(id, vec![c]);
        }
    }
    if wants.crossings {
        let calculated;
        let crossings = match crossings {
            Some(crossings) => crossings,
            None => {
                calculated = crate::infer::crossings(sketch, &sol);
                &calculated
            }
        };
        for (p, on) in crossings {
            let taken = s.entities.iter().any(|e| match e {
                Entity::Point { x, y, .. } => (x - p.0[0]).hypot(y - p.0[1]) < SAME,
                _ => false,
            });
            if taken {
                continue;
            }
            let id = s.add_point(p.0[0], p.0[1]);
            let cs = on
                .iter()
                .map(|c| s.add_constraint(kmb_sketch::on_curve(sketch, id, *c)))
                .collect();
            virtuals.insert(id, p.1);
            auto.insert(id, cs);
        }
    }
    // 仮の要素の位置を解に入れる(道具は解の位置で当たりを調べる)
    for e in &s.entities {
        if !virtuals.contains_key(&e.id()) {
            continue;
        }
        match e {
            Entity::Point { id, x, y } => {
                sol.points.insert(*id, [*x, *y]);
            }
            Entity::Circle { id, radius, .. } => {
                sol.radii.insert(*id, *radius);
            }
            _ => {}
        }
    }
    let last = sketch.last_ids();
    Augmented {
        sketch: s,
        sol,
        virtuals,
        auto,
        opposites: BTreeMap::new(),
        last,
    }
}

fn sol_point(sol: &Solution, id: EntityId) -> Option<[f64; 2]> {
    sol.points.get(&id).copied()
}

fn point_on_circle(c: &kmb_sketch::Constraint, point: EntityId, circle: EntityId) -> bool {
    matches!(c.kind, ConstraintKind::PointOnCircle { point: p, circle: ring } if p == point && ring == circle)
}

impl Augmented {
    /// 円周上にある始点から、中心を通る反対側を線の終点の候補にする。
    /// 位置だけが円周に近い点は候補にしない。
    pub(crate) fn add_opposites(&mut self, start: EntityId) {
        let Some(p) = sol_point(&self.sol, start) else {
            return;
        };
        let rounds: Vec<(EntityId, EntityId)> = self.sketch.entities.iter().filter_map(|e| {
            let (id, center, own_end) = match e {
                Entity::Circle { id, center, .. } => (*id, *center, false),
                Entity::Arc { id, center, start: a, end: b, .. } => (*id, *center, start == *a || start == *b),
                _ => return None,
            };
            let on = own_end || self.sketch.constraints.iter().any(|c| {
                matches!(c.kind, ConstraintKind::PointOnCircle { point, circle } if point == start && circle == id)
            });
            on.then_some((id, center))
        }).collect();
        for (circle, center) in rounds {
            let Some(c) = sol_point(&self.sol, center) else {
                continue;
            };
            let q = [2.0 * c[0] - p[0], 2.0 * c[1] - p[1]];
            if (q[0] - p[0]).hypot(q[1] - p[1]) < SAME {
                continue;
            }
            // 同じ位置の四分点も直径の候補として使い、点の当たり判定を重ねない。
            let existing = self.sketch.entities.iter().find_map(|e| match e {
                Entity::Point { id, .. }
                    if sol_point(&self.sol, *id)
                        .is_some_and(|r| (q[0] - r[0]).hypot(q[1] - r[1]) < SAME) =>
                {
                    Some(*id)
                }
                _ => None,
            });
            let id = match existing {
                Some(id)
                    if self.virtuals.get(&id) == Some(&Kind::Quadrant)
                        && self.auto.get(&id).is_some_and(|auto| {
                            !auto.is_empty()
                                && auto.iter().all(|cid| {
                                    self.sketch
                                        .constraints
                                        .iter()
                                        .any(|c| c.id == *cid && point_on_circle(c, id, circle))
                                })
                        }) =>
                {
                    id
                }
                // 文書の点とほかの吸い付く先は元の拾い方を保つ。終点の拘束に中点を
                // 重ねると、余分な拘束になる場合があるため、直径の候補にはしない。
                Some(_) => continue,
                None => {
                    let id = self.sketch.add_point(q[0], q[1]);
                    self.sol.points.insert(id, q);
                    id
                }
            };
            self.virtuals.insert(id, Kind::Opposite);
            self.opposites.entry(id).or_insert((start, center, circle));
        }
    }

    /// 始点から反対側の候補へ実際に線を引いた時だけ、直径として拘束する。
    pub(crate) fn attach_diameter(
        &self,
        s: &mut Sketch,
        line: EntityId,
        a: EntityId,
        b: EntityId,
    ) -> bool {
        let Some((_, center, circle)) = self
            .opposites
            .get(&b)
            .copied()
            .filter(|(start, ..)| *start == a)
        else {
            return false;
        };
        s.add_constraint(ConstraintKind::Midpoint {
            point: center,
            line,
        });
        // 始点の円周上拘束と中点で終点も円周上になる。四分点の仮の終点に付いた
        // 同じ円の自動拘束だけを除き、文書にある拘束は変えない。
        if let Some(auto) = self.auto.get(&b) {
            s.constraints
                .retain(|c| !(auto.contains(&c.id) && point_on_circle(c, b, circle)));
        }
        true
    }

    pub(crate) fn kind(&self, id: EntityId) -> Option<Kind> {
        self.opposites
            .contains_key(&id)
            .then_some(Kind::Opposite)
            .or_else(|| self.virtuals.get(&id).copied())
    }

    /// 仮の要素をそのまま残したスケッチ(クリックで選んだ辺や角を文書に写す時)。
    pub(crate) fn materialize(&self, keep: &[EntityId]) -> (Sketch, BTreeMap<EntityId, EntityId>) {
        let mut s = self.sketch.clone();
        s.adopt_solution(&self.sol);
        self.prune(s, keep)
    }

    /// 道具が作ったスケッチから、使われなかった仮の要素を除く。`keep` は使わなくても残すもの。
    /// 仮の要素が使っていた番号を詰め、変えた要素の番号の対応も返す(選びなどを付け替える)。
    pub(crate) fn prune(
        &self,
        mut s: Sketch,
        keep: &[EntityId],
    ) -> (Sketch, BTreeMap<EntityId, EntityId>) {
        let auto: BTreeSet<ConstraintId> = self.auto.values().flatten().copied().collect();
        let mut kept: BTreeSet<EntityId> = keep
            .iter()
            .copied()
            .filter(|id| self.virtuals.contains_key(id))
            .collect();
        for e in s
            .entities
            .iter()
            .filter(|e| !self.virtuals.contains_key(&e.id()))
        {
            kept.extend(
                uses(e)
                    .into_iter()
                    .filter(|u| self.virtuals.contains_key(u)),
            );
        }
        for c in s.constraints.iter().filter(|c| !auto.contains(&c.id)) {
            kept.extend(
                kmb_sketch::refs(&c.kind)
                    .into_iter()
                    .filter(|u| self.virtuals.contains_key(u)),
            );
        }
        // 残す仮の線・円が使う点も残す
        loop {
            let more: Vec<EntityId> = s
                .entities
                .iter()
                .filter(|e| kept.contains(&e.id()))
                .flat_map(uses)
                .filter(|u| self.virtuals.contains_key(u) && !kept.contains(u))
                .collect();
            if more.is_empty() {
                break;
            }
            kept.extend(more);
        }
        let gone: BTreeSet<EntityId> = self
            .virtuals
            .keys()
            .copied()
            .filter(|id| !kept.contains(id))
            .collect();
        s.entities.retain(|e| !gone.contains(&e.id()));
        s.constraints
            .retain(|c| !kmb_sketch::refs(&c.kind).iter().any(|r| gone.contains(r)));
        s.projections.retain(|p| !gone.contains(&p.entity));
        let moved = s.renumber_after(self.last.0, self.last.1);
        (s, moved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const ALL: Wants = Wants {
        points: true,
        edges: true,
        midpoints: true,
        crossings: true,
    };

    fn plate_edge() -> RefItem {
        RefItem {
            source: ProjectionSource::Edge {
                feature: kmb_core::FeatureId(1),
                faces: ["top".into(), "side".into()],
            },
            shape: RefShape::Line([0.0, 0.0], [60.0, 0.0]),
        }
    }

    fn base() -> (Sketch, Solution) {
        let mut s = Sketch::default();
        let (a, b) = (s.add_point(0.0, 20.0), s.add_point(10.0, 20.0));
        s.add_line(a, b);
        let sol = kmb_sketch::solve(&s, &HashMap::new()).unwrap();
        (s, sol)
    }

    #[test]
    fn unused_snap_targets_leave_no_trace() {
        let (s, sol) = base();
        let refs = vec![kmb_sketch::origin_ref(&kmb_core::Plane::XY), plate_edge()];
        let aug = augment(&s, &sol, &refs, ALL);
        let kinds: Vec<Kind> = aug.virtuals.values().copied().collect();
        assert!(kinds.contains(&Kind::Origin));
        assert!(kinds.contains(&Kind::Midpoint));
        assert!(kinds.contains(&Kind::Edge));
        // 何も使わなければ元のまま
        let (out, _) = aug.prune(aug.sketch.clone(), &[]);
        assert_eq!(out.entities.len(), s.entities.len());
        assert_eq!(out.constraints.len(), s.constraints.len());
        assert!(out.projections.is_empty());
    }

    #[test]
    fn a_line_started_at_a_snap_target_keeps_it() {
        let (s, sol) = base();
        let refs = vec![kmb_sketch::origin_ref(&kmb_core::Plane::XY), plate_edge()];
        let aug = augment(&s, &sol, &refs, ALL);
        let origin = aug
            .virtuals
            .iter()
            .find(|(_, k)| **k == Kind::Origin)
            .map(|(id, _)| *id)
            .unwrap();
        let mid = aug
            .virtuals
            .iter()
            .find(|(_, k)| **k == Kind::Midpoint)
            .map(|(id, _)| *id)
            .unwrap();
        // 原点から中点へ線を引いた
        let mut drawn = aug.sketch.clone();
        drawn.add_line(origin, mid);
        let (out, moved) = aug.prune(drawn, &[]);
        let at = |id: EntityId| moved.get(&id).copied().unwrap_or(id);
        let (origin, mid) = (at(origin), at(mid));
        // 仮の要素が使っていた番号は詰める(拘束の番号に間が空かない)
        let n = s.entities.len() as u32;
        assert!(
            out.entities.iter().all(|e| e.id().0 <= n + 3),
            "{:?}",
            out.entities
        );
        assert!(
            out.constraints
                .iter()
                .enumerate()
                .all(|(i, c)| c.id.0 == i as u32 + 1)
        );
        assert_eq!(
            out.projections.len(),
            1,
            "原点だけが写る(板の辺は使っていない)"
        );
        assert!(out.projections[0].source == ProjectionSource::Origin);
        assert!(
            out.constraints
                .iter()
                .any(|c| matches!(c.kind, ConstraintKind::Midpoint { point, .. } if point == mid)),
            "中点の拘束が残る"
        );
        // 原点は動かず、線の端は中点についてくる
        let sol = kmb_sketch::solve(&out, &HashMap::new()).unwrap();
        assert_eq!(sol.points[&origin], [0.0, 0.0]);
        assert!((sol.points[&mid][0] - 5.0).abs() < 1e-9);
    }

    #[test]
    fn a_clicked_edge_is_kept_with_its_ends() {
        let (s, sol) = base();
        let aug = augment(&s, &sol, &[plate_edge()], ALL);
        let edge = aug
            .virtuals
            .iter()
            .find(|(_, k)| **k == Kind::Edge)
            .map(|(id, _)| *id)
            .unwrap();
        let (out, moved) = aug.materialize(&[edge]);
        assert!(
            out.entity(moved.get(&edge).copied().unwrap_or(edge))
                .is_some()
        );
        assert_eq!(out.projections.len(), 1);
        assert_eq!(out.entities.len(), s.entities.len() + 3, "線と両端の点");
    }

    fn diameter_base(on: bool) -> (Sketch, EntityId, EntityId, EntityId) {
        let mut s = Sketch::default();
        let center = s.add_point(0.0, 0.0);
        let circle = s.add_circle(center, 5.0);
        let start = s.add_point(3.0, 4.0);
        s.add_constraint(ConstraintKind::Fix { point: center });
        s.add_constraint(ConstraintKind::Radius {
            circle,
            value: "5 mm".into(),
        });
        if on {
            s.add_constraint(ConstraintKind::PointOnCircle {
                point: start,
                circle,
            });
        }
        (s, center, circle, start)
    }

    fn augmented_diameter(s: &Sketch, start: EntityId) -> Augmented {
        let v = kmb_sketch::dimension_values(s, &|_| None).unwrap();
        let sol = kmb_sketch::solve(s, &v).unwrap();
        let mut aug = augment(s, &sol, &[], ALL);
        aug.add_opposites(start);
        aug
    }

    #[test]
    fn diameter_candidate_requires_a_point_on_the_circle_and_is_not_kept_unused() {
        let (s, _, _, start) = diameter_base(false);
        let aug = augmented_diameter(&s, start);
        assert!(aug.opposites.is_empty(), "位置だけでは直径を推測しない");
        let (s, _, _, start) = diameter_base(true);
        let aug = augmented_diameter(&s, start);
        let (&end, _) = aug.opposites.iter().next().unwrap();
        assert_eq!(aug.kind(end), Some(Kind::Opposite));
        assert_eq!(aug.kind(end).unwrap().label(), "反対側");
        assert_eq!(aug.sol.points[&end], [-3.0, -4.0]);
        let (out, _) = aug.prune(aug.sketch.clone(), &[]);
        assert_eq!(out.entities, s.entities);
        assert_eq!(out.constraints, s.constraints);
        // 候補から離れた所へ線を引いても、直径の拘束は付かない。
        let mut drawn = aug.sketch.clone();
        let other = drawn.add_point(-8.0, 2.0);
        let line = drawn.add_line(start, other);
        assert!(!aug.attach_diameter(&mut drawn, line, start, other));
        let (out, _) = aug.prune(drawn, &[]);
        assert_eq!(out.constraints, s.constraints);
        assert_eq!(out.entities.len(), s.entities.len() + 2);
    }

    #[test]
    fn snapped_diameter_keeps_its_midpoint_after_the_radius_changes() {
        let (s, center, circle, start) = diameter_base(true);
        let aug = augmented_diameter(&s, start);
        let end = aug
            .sketch
            .nearest_point(&aug.sol, -3.05, -4.03, 0.1)
            .unwrap()
            .entity;
        assert!(aug.opposites.contains_key(&end));
        assert!(
            aug.sketch
                .nearest_point(&aug.sol, -3.5, -4.5, 0.1)
                .is_none()
        );
        let mut drawn = aug.sketch.clone();
        let line = drawn.add_line(start, end);
        assert!(aug.attach_diameter(&mut drawn, line, start, end));
        let (mut out, moved) = aug.prune(drawn, &[]);
        let end = moved.get(&end).copied().unwrap_or(end);
        let line = moved.get(&line).copied().unwrap_or(line);
        assert_eq!(out.constraints.iter().filter(|c| matches!(c.kind, ConstraintKind::Midpoint { point, line: l } if point == center && l == line)).count(), 1);
        assert!(!out.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::PointOnCircle { point, circle: c } if point == end && c == circle)));
        assert_eq!(out.constraints.len(), s.constraints.len() + 1);
        for old in &s.constraints {
            assert!(out.constraints.contains(old), "既存拘束を変えない: {old:?}");
        }
        let mut states = Vec::new();
        for r in [5.0, 8.0] {
            let dim = out
                .constraints
                .iter_mut()
                .find(|c| matches!(c.kind, ConstraintKind::Radius { .. }))
                .unwrap();
            *dim.kind.value_mut().unwrap() = format!("{r} mm");
            let v = kmb_sketch::dimension_values(&out, &|_| None).unwrap();
            let sol = kmb_sketch::solve(&out, &v).unwrap();
            let (a, b, c) = (sol.points[&start], sol.points[&end], sol.points[&center]);
            for axis in 0..2 {
                assert!(((a[axis] + b[axis]) / 2.0 - c[axis]).abs() < 1e-6);
            }
            for p in [a, b] {
                assert!(((p[0] - c[0]).hypot(p[1] - c[1]) - r).abs() < 1e-6);
            }
            assert_eq!(sol.dof, 1);
            eprintln!(
                "radius={r} midpoint=ok endpoint_on_circle=ok dof={} status={:?}",
                sol.dof, sol.status
            );
            states.push(sol.status.clone());
            out.adopt_solution(&sol);
        }
        assert!(
            states
                .iter()
                .all(|s| matches!(s, kmb_sketch::SolveStatus::UnderConstrained { dof: 1 })),
            "直径には回転の自由度だけが残る: {states:?}"
        );
    }

    #[test]
    fn an_existing_opposite_point_keeps_its_circle_constraint_without_redundancy() {
        let (mut s, _, circle, start) = diameter_base(true);
        let end = s.add_point(-3.0, -4.0);
        s.add_constraint(ConstraintKind::PointOnCircle { point: end, circle });
        let aug = augmented_diameter(&s, start);
        assert!(!aug.opposites.contains_key(&end));
        assert_ne!(aug.kind(end), Some(Kind::Opposite));
        let mut drawn = aug.sketch.clone();
        let line = drawn.add_line(start, end);
        assert!(!aug.attach_diameter(&mut drawn, line, start, end));
        let (out, _) = aug.prune(drawn, &[]);
        assert_eq!(out.constraints, s.constraints);
        for old in &s.constraints {
            assert!(out.constraints.contains(old), "既存拘束を変えない: {old:?}");
        }
        let values = kmb_sketch::dimension_values(&out, &|_| None).unwrap();
        let sol = kmb_sketch::solve(&out, &values).unwrap();
        assert!(
            !matches!(
                sol.status,
                kmb_sketch::SolveStatus::OverConstrained { .. }
                    | kmb_sketch::SolveStatus::Inconsistent { .. }
            ),
            "既存終点の円周上拘束を残したまま余分な拘束を付けない: {sol:?}"
        );
    }

    #[test]
    fn an_origin_snap_keeps_its_role_without_a_redundant_diameter_constraint() {
        for midpoint in [false, true] {
            let mut s = Sketch::default();
            let center = s.add_point(3.0, 4.0);
            let circle = s.add_circle(center, 5.0);
            let start = s.add_point(6.0, 8.0);
            s.add_constraint(ConstraintKind::Fix { point: center });
            s.add_constraint(ConstraintKind::Radius {
                circle,
                value: "5 mm".into(),
            });
            s.add_constraint(ConstraintKind::PointOnCircle {
                point: start,
                circle,
            });
            if midpoint {
                let a = s.add_point(-1.0, 0.0);
                let b = s.add_point(1.0, 0.0);
                s.add_line(a, b);
                s.add_constraint(ConstraintKind::Fix { point: a });
                s.add_constraint(ConstraintKind::Fix { point: b });
            }
            let values = kmb_sketch::dimension_values(&s, &|_| None).unwrap();
            let sol = kmb_sketch::solve(&s, &values).unwrap();
            let refs = if midpoint {
                vec![]
            } else {
                vec![kmb_sketch::origin_ref(&kmb_core::Plane::XY)]
            };
            let mut aug = augment(&s, &sol, &refs, ALL);
            let kind = if midpoint {
                Kind::Midpoint
            } else {
                Kind::Origin
            };
            let origin = *aug.virtuals.iter().find(|(_, k)| **k == kind).unwrap().0;
            let before_auto = aug.auto.clone();
            let before_virtuals = aug.virtuals.clone();
            aug.add_opposites(start);
            assert_eq!(aug.auto, before_auto);
            assert_eq!(aug.virtuals, before_virtuals);
            assert_eq!(aug.kind(origin), Some(kind));
            let mut drawn = aug.sketch.clone();
            let line = drawn.add_line(start, origin);
            assert!(!aug.attach_diameter(&mut drawn, line, start, origin));
            let (out, _) = aug.prune(drawn, &[]);
            assert_eq!(
                out.constraints.len(),
                s.constraints.len() + usize::from(midpoint)
            );
            assert_eq!(out.projections.len(), usize::from(!midpoint));
            for old in &s.constraints {
                assert!(out.constraints.contains(old));
            }
            let sol = kmb_sketch::solve(&out, &values).unwrap();
            assert!(
                !matches!(
                    sol.status,
                    kmb_sketch::SolveStatus::OverConstrained { .. }
                        | kmb_sketch::SolveStatus::Inconsistent { .. }
                ),
                "原点の固定に直径の中点を重ねて余分にしない: {sol:?}"
            );
        }
    }

    #[test]
    fn a_quadrant_or_arc_start_offers_the_opposite_without_duplicate_circle_constraints() {
        for arc in [false, true] {
            let mut s = Sketch::default();
            let center = s.add_point(10.0, 10.0);
            let circle = if arc {
                let a = s.add_point(15.0, 10.0);
                let b = s.add_point(5.0, 10.0);
                s.add_arc(center, a, b)
            } else {
                s.add_circle(center, 5.0)
            };
            let sol = kmb_sketch::solve(&s, &HashMap::new()).unwrap();
            let mut aug = augment(&s, &sol, &[], ALL);
            let start = aug
                .sketch
                .nearest_point(&aug.sol, 10.0, 15.0, 0.01)
                .unwrap()
                .entity;
            assert_eq!(aug.kind(start), Some(Kind::Quadrant));
            aug.add_opposites(start);
            let end = aug
                .sketch
                .nearest_point(&aug.sol, 10.0, 5.0, 0.01)
                .unwrap()
                .entity;
            assert_eq!(aug.kind(end), Some(Kind::Opposite));
            let mut drawn = aug.sketch.clone();
            let line = drawn.add_line(start, end);
            assert!(aug.attach_diameter(&mut drawn, line, start, end));
            assert!(!drawn.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::PointOnCircle { point, circle: c } if point == end && c == circle)));
            let (out, moved) = aug.prune(drawn, &[]);
            let at = |id| moved.get(&id).copied().unwrap_or(id);
            let (start, end, line) = (at(start), at(end), at(line));
            assert!(out.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::PointOnCircle { point, circle: c } if point == start && c == circle)));
            assert!(out.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::Midpoint { point, line: l } if point == center && l == line)));
            for old in &s.constraints {
                assert!(out.constraints.contains(old), "既存拘束を変えない: {old:?}");
            }
            let sol = kmb_sketch::solve(&out, &HashMap::new()).unwrap();
            assert!(
                !matches!(
                    sol.status,
                    kmb_sketch::SolveStatus::OverConstrained { .. }
                        | kmb_sketch::SolveStatus::Inconsistent { .. }
                ),
                "{sol:?}"
            );
            let (a, b, c) = (sol.points[&start], sol.points[&end], sol.points[&center]);
            for axis in 0..2 {
                assert!(((a[axis] + b[axis]) / 2.0 - c[axis]).abs() < 1e-6);
            }
            let r = kmb_sketch::radius(&out, &sol, circle).unwrap();
            for p in [a, b] {
                assert!(((p[0] - c[0]).hypot(p[1] - c[1]) - r).abs() < 1e-6);
            }
        }
    }
}
