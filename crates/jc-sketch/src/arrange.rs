//! 選んだ要素を並べる(直線・円形)・動かす・回す・拡大縮小する。どれも保存されている座標
//! (先に [`Sketch::adopt_solution`] で解いた位置を写しておく)を書き換える。

use crate::edit::{refs, uses};
use crate::geom::{EPS, add, scale, sub};
use crate::model::{Constraint, ConstraintKind as K, Entity, EntityId, Sketch};
use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::{FRAC_PI_2, PI, TAU};

pub(crate) fn rotate_about(c: [f64; 2], angle: f64) -> impl Fn([f64; 2]) -> [f64; 2] {
    let (s, co) = angle.sin_cos();
    move |p| {
        let d = sub(p, c);
        add(c, [d[0] * co - d[1] * s, d[0] * s + d[1] * co])
    }
}

/// 角度が `step` の倍数に近いか(倍数の何番目かを返す)。
fn multiple_of(angle: f64, step: f64) -> Option<i64> {
    let k = (angle / step).round();
    ((angle - k * step).abs() < 1e-9).then_some(k as i64)
}

/// 数だけの長さの寸法(`12.5 mm`)の値。式やパラメータなら `None`。
fn plain_mm(value: &str) -> Option<f64> {
    value.trim().strip_suffix("mm")?.trim().parse().ok()
}

/// 数を書く(小数 4 桁まで、末尾の 0 は省く)。
fn num_text(v: f64) -> String {
    let s = format!("{v:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// 拘束の参照先を `m` で付け替える。参照先が `m` に無ければ `None`。
pub(crate) fn remap(kind: &K, m: &BTreeMap<EntityId, EntityId>) -> Option<K> {
    let g = |id: &EntityId| m.get(id).copied();
    Some(match kind {
        K::Coincident { a, b } => K::Coincident { a: g(a)?, b: g(b)? },
        K::Fix { point } => K::Fix { point: g(point)? },
        K::Horizontal { line } => K::Horizontal { line: g(line)? },
        K::Vertical { line } => K::Vertical { line: g(line)? },
        K::Parallel { a, b } => K::Parallel { a: g(a)?, b: g(b)? },
        K::Perpendicular { a, b } => K::Perpendicular { a: g(a)?, b: g(b)? },
        K::Equal { a, b } => K::Equal { a: g(a)?, b: g(b)? },
        K::PointOnLine { point, line } => K::PointOnLine {
            point: g(point)?,
            line: g(line)?,
        },
        K::PointOnCircle { point, circle } => K::PointOnCircle {
            point: g(point)?,
            circle: g(circle)?,
        },
        K::Tangent { line, circle } => K::Tangent {
            line: g(line)?,
            circle: g(circle)?,
        },
        K::TangentCircles { a, b } => K::TangentCircles { a: g(a)?, b: g(b)? },
        K::Concentric { a, b } => K::Concentric { a: g(a)?, b: g(b)? },
        K::Collinear { a, b } => K::Collinear { a: g(a)?, b: g(b)? },
        K::Midpoint { point, line } => K::Midpoint {
            point: g(point)?,
            line: g(line)?,
        },
        K::Symmetric { a, b, line } => K::Symmetric {
            a: g(a)?,
            b: g(b)?,
            line: g(line)?,
        },
        K::Distance { a, b, value } => K::Distance {
            a: g(a)?,
            b: g(b)?,
            value: value.clone(),
        },
        K::PointLineDistance { point, line, value } => K::PointLineDistance {
            point: g(point)?,
            line: g(line)?,
            value: value.clone(),
        },
        K::HorizontalDistance { a, b, value } => K::HorizontalDistance {
            a: g(a)?,
            b: g(b)?,
            value: value.clone(),
        },
        K::VerticalDistance { a, b, value } => K::VerticalDistance {
            a: g(a)?,
            b: g(b)?,
            value: value.clone(),
        },
        K::Length { line, value } => K::Length {
            line: g(line)?,
            value: value.clone(),
        },
        K::Radius { circle, value } => K::Radius {
            circle: g(circle)?,
            value: value.clone(),
        },
        K::Diameter { circle, value } => K::Diameter {
            circle: g(circle)?,
            value: value.clone(),
        },
        K::Angle { a, b, value } => K::Angle {
            a: g(a)?,
            b: g(b)?,
            value: value.clone(),
        },
        K::PointOnCurve { point, curve } => K::PointOnCurve {
            point: g(point)?,
            curve: g(curve)?,
        },
        K::TangentCurve { a, b } => K::TangentCurve { a: g(a)?, b: g(b)? },
        K::Curvature { a, b } => K::Curvature { a: g(a)?, b: g(b)? },
        K::CircleDistance {
            circle,
            other,
            far,
            value,
        } => K::CircleDistance {
            circle: g(circle)?,
            other: g(other)?,
            far: *far,
            value: value.clone(),
        },
        K::LinearDiameter { point, line, value } => K::LinearDiameter {
            point: g(point)?,
            line: g(line)?,
            value: value.clone(),
        },
        K::Offset { parts } => K::Offset {
            parts: parts
                .iter()
                .map(|p| p.remap(&|id| g(&id)))
                .collect::<Option<Vec<_>>>()?,
        },
        K::Polygon { circle, sides } => K::Polygon {
            circle: g(circle)?,
            sides: sides.iter().map(g).collect::<Option<Vec<_>>>()?,
        },
        // パターンは要素を参照しない(写して作った物はパターンでない)
        K::Pattern { .. } => return None,
    })
}

impl Sketch {
    /// 番号が `entity_after`・`constraint_after` より大きい要素と拘束を、続きの番号に詰める
    /// (描く間だけ仮に足した要素が番号を使い、間が空いた時に)。変えた要素の番号の対応を返す。
    /// 次に配る番号は詰めた後の続きにする。`entity_after`・`constraint_after` には、元の
    /// スケッチが配った番号の最後([`Sketch::last_ids`])を渡す(消した番号へ詰めない)。
    pub fn renumber_after(
        &mut self,
        entity_after: u32,
        constraint_after: u32,
    ) -> BTreeMap<EntityId, EntityId> {
        let mut ids: Vec<EntityId> = self.entities.iter().map(|e| e.id()).collect();
        ids.sort();
        let mut next = entity_after;
        let mut changed = BTreeMap::new();
        let m: BTreeMap<EntityId, EntityId> = ids
            .into_iter()
            .map(|id| {
                if id.0 <= entity_after {
                    return (id, id);
                }
                next += 1;
                if id.0 != next {
                    changed.insert(id, EntityId(next));
                }
                (id, EntityId(next))
            })
            .collect();
        let g = |id: &mut EntityId| *id = m[id];
        for e in &mut self.entities {
            *e = e.with_points(m[&e.id()], &|p| m[&p]);
        }
        let mut cs: Vec<u32> = self.constraints.iter().map(|c| c.id.0).collect();
        cs.sort();
        let cm: BTreeMap<u32, u32> = cs
            .into_iter()
            .filter(|c| *c > constraint_after)
            .zip(constraint_after + 1..)
            .collect();
        for c in &mut self.constraints {
            if let Some(kind) = remap(&c.kind, &m) {
                c.kind = kind;
            }
            if let Some(n) = cm.get(&c.id.0) {
                c.id.0 = *n;
            }
        }
        for p in &mut self.projections {
            g(&mut p.entity);
        }
        for c in &mut self.centerlines {
            g(c);
        }
        self.patterns_renumber(&m, &cm);
        self.next_ids = crate::NextIds {
            entity: next + 1,
            constraint: constraint_after + cm.len() as u32 + 1,
        };
        changed
    }

    /// 配った要素と拘束の番号の最後(消した物も含む。控えの無い古いスケッチは今ある最大)。
    pub fn last_ids(&self) -> (u32, u32) {
        let e = self.entities.iter().map(|e| e.id().0).max().unwrap_or(0);
        let c = self.constraints.iter().map(|c| c.id.0).max().unwrap_or(0);
        (
            e.max(self.next_ids.entity.saturating_sub(1)),
            c.max(self.next_ids.constraint.saturating_sub(1)),
        )
    }

    /// 選んだ要素が使う点(選んだ点そのものも含む)。
    pub(crate) fn points_of(&self, ids: &[EntityId]) -> BTreeSet<EntityId> {
        let mut out = BTreeSet::new();
        for id in ids {
            match self.entity(*id) {
                Some(Entity::Point { .. }) => {
                    out.insert(*id);
                }
                Some(e) => out.extend(uses(e)),
                None => {}
            }
        }
        out
    }

    /// 選んだ要素(図形と点。図形の点も含む)の集まり。
    pub(crate) fn closure(&self, ids: &[EntityId]) -> BTreeSet<EntityId> {
        let mut set = self.points_of(ids);
        set.extend(ids.iter().copied().filter(|id| self.entity(*id).is_some()));
        set
    }

    /// 選んだ要素を、点を `f` で写した位置へ 1 組写す。選んだ要素の中だけを参照する拘束も写す
    /// (固定は除く)。`turn` は写しの回った角度で、0 でなければ向きに頼る拘束(水平・垂直)は
    /// 元の線との角度に替え、水平・垂直の距離は外す。写した要素(図形と点)を返す。
    pub(crate) fn copy_mapped(
        &mut self,
        ids: &[EntityId],
        f: &dyn Fn([f64; 2]) -> [f64; 2],
        turn: f64,
    ) -> Vec<EntityId> {
        let set = self.closure(ids);
        let mut m: BTreeMap<EntityId, EntityId> = BTreeMap::new();
        let mut out = vec![];
        for p in self.points_of(ids) {
            if let Some(pos) = self.pos(p) {
                let q = f(pos);
                let n = self.add_point(q[0], q[1]);
                m.insert(p, n);
                out.push(n);
            }
        }
        for id in ids {
            let Some(e) = self.entity(*id).cloned() else {
                continue;
            };
            let constr = self.is_construction(*id);
            let new = match e {
                Entity::Point { .. } => continue,
                Entity::Line { a, b, .. } => self.add_line(m[&a], m[&b]),
                Entity::Circle { center, radius, .. } => self.add_circle(m[&center], radius),
                Entity::Arc {
                    center, start, end, ..
                } => self.add_arc(m[&center], m[&start], m[&end]),
                e @ (Entity::Ellipse { .. } | Entity::Spline { .. } | Entity::Conic { .. }) => {
                    self.add_entity(&e.with_points(EntityId(0), &|p| m[&p]))
                }
            };
            self.set_construction(new, constr);
            m.insert(*id, new);
            out.push(new);
        }
        let turned = multiple_of(turn, TAU).is_none();
        let deg = turn.rem_euclid(TAU).to_degrees();
        let copied: Vec<Constraint> = self
            .constraints
            .iter()
            .filter(|c| !c.reference && !matches!(c.kind, K::Fix { .. }))
            .filter(|c| refs(&c.kind).iter().all(|r| set.contains(r)))
            .cloned()
            .collect();
        for c in copied {
            let kind = match (&c.kind, turned) {
                (K::Horizontal { line } | K::Vertical { line }, true) => K::Angle {
                    a: *line,
                    b: m[line],
                    value: format!("{} deg", num_text(deg)),
                },
                (K::HorizontalDistance { .. } | K::VerticalDistance { .. }, true) => continue,
                (k, _) => match remap(k, &m) {
                    Some(k) => k,
                    None => continue,
                },
            };
            self.add_constraint(kind);
        }
        out
    }

    /// 選んだ要素を、`step` ずつずらして全部で `count` 個に並べる(元を含む)。
    /// 写しは形を決める拘束(寸法の式も)を元と共有するので、パラメータを変えると大きさも追う。
    /// 写した要素を返す。
    pub fn pattern_linear(
        &mut self,
        ids: &[EntityId],
        step: [f64; 2],
        count: usize,
    ) -> Vec<EntityId> {
        if count < 2 || step[0].hypot(step[1]) < 1e-6 {
            return vec![];
        }
        let mut out = vec![];
        for k in 1..count {
            let d = scale(step, k as f64);
            out.extend(self.copy_mapped(ids, &|p| add(p, d), 0.0));
        }
        out
    }

    /// 選んだ要素を、点 `center` のまわりに全部で `count` 個に並べる(元を含む)。
    /// `total` は並べる角度の範囲(rad)。一周なら等分、それより狭ければ両端を含めて等分する。
    pub fn pattern_circular(
        &mut self,
        ids: &[EntityId],
        center: [f64; 2],
        count: usize,
        total: f64,
    ) -> Vec<EntityId> {
        if count < 2 || total.abs() < 1e-6 {
            return vec![];
        }
        let step = if (total.abs() - TAU).abs() < 1e-9 || total.abs() > TAU {
            total / count as f64
        } else {
            total / (count - 1) as f64
        };
        let mut out = vec![];
        for k in 1..count {
            let a = step * k as f64;
            out.extend(self.copy_mapped(ids, &rotate_about(center, a), a));
        }
        out
    }

    fn map_points(&mut self, pts: &BTreeSet<EntityId>, f: &dyn Fn([f64; 2]) -> [f64; 2]) {
        for e in &mut self.entities {
            if let Entity::Point { id, x, y } = e
                && pts.contains(id)
            {
                [*x, *y] = f([*x, *y]);
            }
        }
    }

    /// 選んだ要素を `d` だけ動かす(保存されている座標を書き換える)。選んでいない要素への
    /// 拘束はそのまま残るので、解き直すとそれに引かれることがある。
    pub fn move_entities(&mut self, ids: &[EntityId], d: [f64; 2]) {
        let pts = self.points_of(ids);
        self.map_points(&pts, &|p| add(p, d));
    }

    /// 選んだ要素を、点 `center` のまわりに `angle`(rad、反時計回り)だけ回す。
    /// 選んだ線に付いた水平・垂直と、選んだ点どうしの水平・垂直の距離は、90° の倍数なら
    /// 入れ替え、それ以外の角度なら外す(付けたままだと解き直しで元に戻るため)。外した数を返す。
    pub fn rotate_entities(&mut self, ids: &[EntityId], center: [f64; 2], angle: f64) -> usize {
        let pts = self.points_of(ids);
        self.map_points(&pts, &rotate_about(center, angle));
        let set = self.closure(ids);
        let inside = |k: &K| refs(k).iter().all(|r| set.contains(r));
        let quarter = multiple_of(angle, FRAC_PI_2);
        let swap = quarter.is_some_and(|k| k.rem_euclid(2) == 1);
        let keep = multiple_of(angle, PI).is_some();
        let before = self.constraints.len();
        let mut kept = Vec::with_capacity(before);
        for mut c in std::mem::take(&mut self.constraints) {
            let directional = matches!(
                c.kind,
                K::Horizontal { .. }
                    | K::Vertical { .. }
                    | K::HorizontalDistance { .. }
                    | K::VerticalDistance { .. }
            );
            if directional && inside(&c.kind) && !keep {
                if !swap {
                    continue;
                }
                c.kind = match c.kind {
                    K::Horizontal { line } => K::Vertical { line },
                    K::Vertical { line } => K::Horizontal { line },
                    K::HorizontalDistance { a, b, value } => K::VerticalDistance { a, b, value },
                    K::VerticalDistance { a, b, value } => K::HorizontalDistance { a, b, value },
                    k => k,
                };
            }
            kept.push(c);
        }
        self.constraints = kept;
        before - self.constraints.len()
    }

    /// 選んだ要素を、点 `center` を中心に `k` 倍にする。円の半径と、選んだ要素の中だけを測る
    /// 数だけの長さの寸法(`12 mm` など)も `k` 倍にする。式やパラメータの寸法は変えないので、
    /// 解き直すとその大きさに戻る。`k` が正でなければ何もしない。
    pub fn scale_entities(&mut self, ids: &[EntityId], center: [f64; 2], k: f64) {
        if k <= EPS || !k.is_finite() {
            return;
        }
        let pts = self.points_of(ids);
        self.map_points(&pts, &|p| add(center, scale(sub(p, center), k)));
        let set = self.closure(ids);
        for e in &mut self.entities {
            if let Entity::Circle { id, radius, .. } = e
                && set.contains(id)
            {
                *radius *= k;
            }
        }
        for c in &mut self.constraints {
            if c.kind.is_angle() || !refs(&c.kind).iter().all(|r| set.contains(r)) {
                continue;
            }
            if let Some(v) = c.kind.value_mut()
                && let Some(x) = plain_mm(v)
            {
                *v = format!("{} mm", num_text(x * k));
            }
        }
    }
}
