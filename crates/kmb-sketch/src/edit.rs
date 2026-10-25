//! スケッチの編集操作。画面の操作から呼ぶ。どれも新しい [`Sketch`] の値を作るだけで、
//! 文書への反映(取り消しの記録)は呼び出し側がトランザクションで行う。

use crate::model::{Constraint, ConstraintId, ConstraintKind, Entity, EntityId, Sketch};
use crate::solver::Solution;
use std::collections::BTreeSet;

/// 当たり判定の結果。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    pub entity: EntityId,
    /// 判定の基準点からの距離(mm)
    pub distance: f64,
}

impl Sketch {
    pub fn entity(&self, id: EntityId) -> Option<&Entity> {
        self.entities.iter().find(|e| e.id() == id)
    }

    /// 次の要素の番号。消した要素の番号は使い回さない(拘束や機能の参照が黙って別の要素に
    /// 付き替わらない)。控えの無い古いスケッチは今ある最大+1 から数える。
    pub(crate) fn next_entity_id(&mut self) -> EntityId {
        let max = self.entities.iter().map(|e| e.id().0).max().unwrap_or(0);
        let id = self.next_ids.entity.max(max + 1);
        self.next_ids.entity = id + 1;
        EntityId(id)
    }

    /// 要素を新しい番号で足す(`e` の番号は付け直す)。足した番号を返す。
    pub(crate) fn add_entity(&mut self, e: &Entity) -> EntityId {
        let id = self.next_entity_id();
        self.entities.push(e.with_points(id, &|p| p));
        id
    }

    /// 次の拘束の番号。要素と同じく使い回さない(寸法の名前や式の参照が付き替わらない)。
    fn next_constraint_id(&mut self) -> ConstraintId {
        let max = self.constraints.iter().map(|c| c.id.0).max().unwrap_or(0);
        let id = self.next_ids.constraint.max(max + 1);
        self.next_ids.constraint = id + 1;
        ConstraintId(id)
    }

    /// `before` にあって今は無い寸法(値を持つ拘束)の名前(名前の無い物は番号)。トリム・分割で
    /// 外れた寸法を知らせる(黙って消さない)。
    pub fn dims_dropped_since(&self, before: &Sketch) -> Vec<String> {
        before
            .constraints
            .iter()
            .filter(|c| c.kind.value().is_some())
            .filter(|c| !self.constraints.iter().any(|d| d.id == c.id))
            .map(|c| match before.dim_name(c.id) {
                Some(n) => n.to_string(),
                None => format!("寸法 {}", c.id.0),
            })
            .collect()
    }

    /// 解いた位置を、保存する座標(次に解く時の初期値)へ写す。
    /// 編集の前に呼ぶと、描かれている形が編集で跳ねない。
    pub fn adopt_solution(&mut self, sol: &Solution) {
        // 古い絶対座標の札は、今の(書き換える前の)位置について相対の座標へ移す
        self.anchor_labels();
        for e in &mut self.entities {
            match e {
                Entity::Point { id, x, y } => {
                    if let Some(p) = sol.points.get(id) {
                        (*x, *y) = (p[0], p[1]);
                    }
                }
                Entity::Circle { id, radius, .. } => {
                    if let Some(r) = sol.radii.get(id) {
                        *radius = *r;
                    }
                }
                _ => {}
            }
        }
    }

    pub fn add_point(&mut self, x: f64, y: f64) -> EntityId {
        let id = self.next_entity_id();
        self.entities.push(Entity::Point { id, x, y });
        id
    }

    pub fn add_line(&mut self, a: EntityId, b: EntityId) -> EntityId {
        let id = self.next_entity_id();
        self.entities.push(Entity::Line {
            id,
            a,
            b,
            construction: false,
        });
        id
    }

    pub fn add_circle(&mut self, center: EntityId, radius: f64) -> EntityId {
        let id = self.next_entity_id();
        self.entities.push(Entity::Circle {
            id,
            center,
            radius,
            construction: false,
        });
        id
    }

    pub fn add_arc(&mut self, center: EntityId, start: EntityId, end: EntityId) -> EntityId {
        let id = self.next_entity_id();
        self.entities.push(Entity::Arc {
            id,
            center,
            start,
            end,
            construction: false,
        });
        id
    }

    pub fn add_constraint(&mut self, kind: ConstraintKind) -> ConstraintId {
        let id = self.next_constraint_id();
        self.constraints.push(Constraint::new(id, kind));
        id
    }

    /// 拘束を外す。パターンの並びの拘束なら、パターンの定義も消す(写しは普通の要素として残る)。
    pub fn remove_constraint(&mut self, id: ConstraintId) {
        if let Some(ConstraintKind::Pattern { pattern }) = self
            .constraints
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.kind.clone())
        {
            self.remove_pattern(pattern, true);
        }
        self.constraints.retain(|c| c.id != id);
    }

    pub fn set_point(&mut self, point: EntityId, x: f64, y: f64) {
        for e in &mut self.entities {
            if let Entity::Point { id, x: px, y: py } = e
                && *id == point
            {
                (*px, *py) = (x, y);
            }
        }
    }

    pub fn toggle_construction(&mut self, target: EntityId) {
        for e in &mut self.entities {
            if e.id() == target
                && let Some(c) = e.construction_mut()
            {
                *c = !*c;
            }
        }
        // 補助線でなくなった中心線は普通の線
        self.centerlines.retain(|c| *c != target);
    }

    /// 要素を消す。消した点を使う図形と、消した要素を参照する拘束も消す。
    /// 消した図形だけが使っていた点も消す(ほかの図形と共有している点は残す)。
    pub fn remove_entities(&mut self, ids: &[EntityId]) {
        let mut gone: BTreeSet<EntityId> = ids.iter().copied().collect();
        for e in &self.entities {
            if uses(e).iter().any(|u| gone.contains(u)) {
                gone.insert(e.id());
            }
        }
        let remaining: BTreeSet<EntityId> = self
            .entities
            .iter()
            .filter(|e| !gone.contains(&e.id()))
            .flat_map(uses)
            .collect();
        let freed: Vec<EntityId> = self
            .entities
            .iter()
            .filter(|e| gone.contains(&e.id()))
            .flat_map(uses)
            .filter(|p| !remaining.contains(p))
            .collect();
        gone.extend(freed);
        self.patterns_forget(&gone);
        self.entities.retain(|e| !gone.contains(&e.id()));
        // まとまりの拘束(オフセット)は、消えた要素の関係だけ外す
        for c in &mut self.constraints {
            if let ConstraintKind::Offset { parts } = &mut c.kind {
                parts.retain(|p| !p.refs().iter().any(|r| gone.contains(r)));
            }
        }
        self.constraints.retain(|c| {
            !refs(&c.kind).iter().any(|r| gone.contains(r))
                && !matches!(&c.kind, ConstraintKind::Offset { parts } if parts.is_empty())
        });
        self.projections.retain(|p| !gone.contains(&p.entity));
        self.centerlines.retain(|c| !gone.contains(c));
    }

    /// 基準点(mm)に最も近い点。`tol` より遠ければ `None`。
    pub fn nearest_point(&self, sol: &Solution, x: f64, y: f64, tol: f64) -> Option<Hit> {
        sol.points
            .iter()
            .map(|(id, p)| Hit {
                entity: *id,
                distance: (p[0] - x).hypot(p[1] - y),
            })
            .filter(|h| h.distance <= tol)
            .min_by(|a, b| a.distance.total_cmp(&b.distance))
    }

    /// 基準点に最も近い要素(点を優先し、次に線分・円・円弧)。
    pub fn hit_test(&self, sol: &Solution, x: f64, y: f64, tol: f64) -> Option<Hit> {
        if let Some(h) = self.nearest_point(sol, x, y, tol) {
            return Some(h);
        }
        self.curve_hits(sol, x, y, tol).into_iter().next()
    }

    /// 基準点から `tol` 以内の要素の全部(点が先、それぞれ近い順)。重なったものから選び分けるのに使う。
    pub fn hits(&self, sol: &Solution, x: f64, y: f64, tol: f64) -> Vec<Hit> {
        let mut points: Vec<Hit> = sol
            .points
            .iter()
            .map(|(id, p)| Hit {
                entity: *id,
                distance: (p[0] - x).hypot(p[1] - y),
            })
            .filter(|h| h.distance <= tol)
            .collect();
        points.sort_by(|a, b| a.distance.total_cmp(&b.distance));
        points.extend(self.curve_hits(sol, x, y, tol));
        points
    }

    /// 基準点から `tol` 以内の線分・円・円弧(近い順)。
    fn curve_hits(&self, sol: &Solution, x: f64, y: f64, tol: f64) -> Vec<Hit> {
        let mut out = Vec::new();
        for e in &self.entities {
            let d = match e {
                Entity::Line { a, b, .. } => match (sol.points.get(a), sol.points.get(b)) {
                    (Some(p), Some(q)) => segment_distance(*p, *q, [x, y]),
                    _ => continue,
                },
                Entity::Circle { id, center, .. } => {
                    match (sol.points.get(center), sol.radii.get(id)) {
                        (Some(c), Some(r)) => ((c[0] - x).hypot(c[1] - y) - r).abs(),
                        _ => continue,
                    }
                }
                Entity::Arc {
                    center, start, end, ..
                } => match (
                    sol.points.get(center),
                    sol.points.get(start),
                    sol.points.get(end),
                ) {
                    (Some(c), Some(s), Some(e)) => arc_distance(*c, *s, *e, [x, y]),
                    _ => continue,
                },
                Entity::Point { .. } => continue,
                Entity::Ellipse { .. } | Entity::Spline { .. } | Entity::Conic { .. } => {
                    match self.solved_shape(sol, e.id()) {
                        Some(s) => s.closest([x, y]).1,
                        None => continue,
                    }
                }
            };
            if d <= tol {
                out.push(Hit {
                    entity: e.id(),
                    distance: d,
                });
            }
        }
        out.sort_by(|a, b| a.distance.total_cmp(&b.distance));
        out
    }
}

/// 円弧(中心 `c`、`s` から `e` へ反時計回り)までの距離。円弧の無い向きでは近い端までの距離
/// (円の続きの所を押しても円弧を選ばない)。
fn arc_distance(c: [f64; 2], s: [f64; 2], e: [f64; 2], p: [f64; 2]) -> f64 {
    use std::f64::consts::TAU;
    let r = (s[0] - c[0]).hypot(s[1] - c[1]);
    let ang = |q: [f64; 2]| (q[1] - c[1]).atan2(q[0] - c[0]);
    let sweep = (ang(e) - ang(s)).rem_euclid(TAU);
    if (ang(p) - ang(s)).rem_euclid(TAU) <= sweep {
        ((c[0] - p[0]).hypot(c[1] - p[1]) - r).abs()
    } else {
        (s[0] - p[0])
            .hypot(s[1] - p[1])
            .min((e[0] - p[0]).hypot(e[1] - p[1]))
    }
}

/// 図形が使う点
pub(crate) fn uses(e: &Entity) -> Vec<EntityId> {
    e.points()
}

/// 拘束が参照する要素
pub fn refs(kind: &ConstraintKind) -> Vec<EntityId> {
    use ConstraintKind as K;
    match kind {
        K::Coincident { a, b }
        | K::Parallel { a, b }
        | K::Perpendicular { a, b }
        | K::Equal { a, b }
        | K::TangentCircles { a, b }
        | K::Concentric { a, b }
        | K::Collinear { a, b }
        | K::TangentCurve { a, b }
        | K::Curvature { a, b }
        | K::Distance { a, b, .. }
        | K::HorizontalPoints { a, b }
        | K::VerticalPoints { a, b }
        | K::HorizontalDistance { a, b, .. }
        | K::VerticalDistance { a, b, .. }
        | K::Angle { a, b, .. } => vec![*a, *b],
        K::Fix { point } => vec![*point],
        K::Symmetric { a, b, line } => vec![*a, *b, *line],
        K::Horizontal { line } | K::Vertical { line } | K::Length { line, .. } => vec![*line],
        K::PointOnLine { point, line }
        | K::Midpoint { point, line }
        | K::PointLineDistance { point, line, .. } => vec![*point, *line],
        K::PointOnCircle { point, circle } => vec![*point, *circle],
        K::PointOnCurve { point, curve } => vec![*point, *curve],
        K::Tangent { line, circle } => vec![*line, *circle],
        K::Radius { circle, .. } | K::Diameter { circle, .. } => vec![*circle],
        K::CircleDistance { circle, other, .. } => vec![*circle, *other],
        K::LinearDiameter { point, line, .. } => vec![*point, *line],
        K::Offset { parts } => {
            let mut v: Vec<EntityId> = parts.iter().flat_map(|p| p.refs()).collect();
            v.sort();
            v.dedup();
            v
        }
        K::Polygon { circle, sides } => std::iter::once(*circle)
            .chain(sides.iter().copied())
            .collect(),
        // 写しと元は、パターンの定義が持つ([`Sketch::patterns_forget`])
        K::Pattern { .. } => vec![],
    }
}

fn segment_distance(p: [f64; 2], q: [f64; 2], x: [f64; 2]) -> f64 {
    let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((x[0] - p[0]) * dx + (x[1] - p[1]) * dy) / len2).clamp(0.0, 1.0)
    };
    (p[0] + t * dx - x[0]).hypot(p[1] + t * dy - x[1])
}
