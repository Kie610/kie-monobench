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
        for (p, on) in crate::infer::crossings(sketch, &sol) {
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
        last,
    }
}

fn sol_point(sol: &Solution, id: EntityId) -> Option<[f64; 2]> {
    sol.points.get(&id).copied()
}

impl Augmented {
    pub(crate) fn kind(&self, id: EntityId) -> Option<Kind> {
        self.virtuals.get(&id).copied()
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
}
