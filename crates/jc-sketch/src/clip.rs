//! スケッチの要素のコピー・切り取り・貼り付け。写すのは選んだ点・線分・円・円弧(線の点も
//! 一緒に)。写した要素の中だけで閉じる拘束と寸法は一緒に写し、写していない要素へつながる
//! もの・固定は落とす。写した形(投影)は、元の形とのつながりを落として普通の線として写す。

use crate::arrange::remap;
use crate::model::{Constraint, ConstraintKind as K, Entity, EntityId, Sketch};
use std::collections::{BTreeMap, BTreeSet};

/// 写した要素(貼り付けに使う)。番号は元のスケッチのもので、貼る時に付け直す。
/// 座標は写した時の解いた位置。
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct Clip {
    pub entities: Vec<Entity>,
    pub constraints: Vec<Constraint>,
}

impl Clip {
    /// 写した図形(線分・円・円弧)と単独の点の数(線の点は数えない)。
    pub fn len(&self) -> usize {
        let used: BTreeSet<EntityId> = self.entities.iter().flat_map(crate::edit::uses).collect();
        self.entities
            .iter()
            .filter(|e| !matches!(e, Entity::Point { id, .. } if used.contains(id)))
            .count()
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// 写した形を囲む箱の中心(貼る位置の基準)。
    pub fn center(&self) -> [f64; 2] {
        let pos: BTreeMap<EntityId, [f64; 2]> = self
            .entities
            .iter()
            .filter_map(|e| match e {
                Entity::Point { id, x, y } => Some((*id, [*x, *y])),
                _ => None,
            })
            .collect();
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        let mut grow = |p: [f64; 2], r: f64| {
            for i in 0..2 {
                lo[i] = lo[i].min(p[i] - r);
                hi[i] = hi[i].max(p[i] + r);
            }
        };
        for p in pos.values() {
            grow(*p, 0.0);
        }
        for e in &self.entities {
            if let Entity::Circle { center, radius, .. } = e
                && let Some(c) = pos.get(center)
            {
                grow(*c, *radius);
            }
        }
        if lo[0] > hi[0] {
            return [0.0, 0.0];
        }
        [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0]
    }
}

impl Sketch {
    /// 選んだ要素を写す(先に [`Sketch::adopt_solution`] で解いた位置を写しておく)。
    /// 何も写せなければ `None`。
    pub fn copy_entities(&self, ids: &[EntityId]) -> Option<Clip> {
        let mut set: BTreeSet<EntityId> = BTreeSet::new();
        for id in ids {
            let Some(e) = self.entity(*id) else { continue };
            set.insert(*id);
            set.extend(crate::edit::uses(e));
        }
        if set.is_empty() {
            return None;
        }
        // 投影は写さない(写した線は元の形につながらない普通の線になる)
        let entities: Vec<Entity> = self
            .entities
            .iter()
            .filter(|e| set.contains(&e.id()))
            .cloned()
            .collect();
        let constraints = self
            .constraints
            .iter()
            .filter(|c| !matches!(c.kind, K::Fix { .. }))
            .filter(|c| crate::edit::refs(&c.kind).iter().all(|r| set.contains(r)))
            .cloned()
            .collect();
        Some(Clip {
            entities,
            constraints,
        })
    }

    /// 写した要素を `d` だけずらして足す。拘束と寸法(置いた位置もずらす)も付け直して足す。
    /// 足した要素(点と図形)を返す。
    pub fn paste(&mut self, clip: &Clip, d: [f64; 2]) -> Vec<EntityId> {
        let mut m: BTreeMap<EntityId, EntityId> = BTreeMap::new();
        let mut out = vec![];
        // 点を先に足す(図形は点を番号で指す)
        for e in &clip.entities {
            if let Entity::Point { id, x, y } = e {
                let n = self.add_point(x + d[0], y + d[1]);
                m.insert(*id, n);
                out.push(n);
            }
        }
        for e in &clip.entities {
            let new = match e {
                Entity::Point { .. } => continue,
                Entity::Line {
                    a, b, construction, ..
                } => {
                    let n = self.add_line(m[a], m[b]);
                    self.set_construction(n, *construction);
                    n
                }
                Entity::Circle {
                    center,
                    radius,
                    construction,
                    ..
                } => {
                    let n = self.add_circle(m[center], *radius);
                    self.set_construction(n, *construction);
                    n
                }
                Entity::Arc {
                    center,
                    start,
                    end,
                    construction,
                    ..
                } => {
                    let n = self.add_arc(m[center], m[start], m[end]);
                    self.set_construction(n, *construction);
                    n
                }
                Entity::Ellipse { .. } | Entity::Spline { .. } | Entity::Conic { .. } => {
                    self.add_entity(&e.with_points(EntityId(0), &|p| m[&p]))
                }
            };
            m.insert(e.id(), new);
            out.push(new);
        }
        for c in &clip.constraints {
            let Some(kind) = remap(&c.kind, &m) else {
                continue;
            };
            let id = self.add_constraint(kind);
            if let Some(n) = self.constraints.iter_mut().find(|n| n.id == id) {
                n.reference = c.reference;
                n.at = c.at.map(|p| [p[0] + d[0], p[1] + d[1]]);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use crate::*;
    use std::collections::HashMap;

    fn values(s: &Sketch) -> HashMap<ConstraintId, f64> {
        s.constraints
            .iter()
            .filter(|c| !c.reference && c.kind.value().is_some())
            .map(|c| (c.id, 20.0))
            .collect()
    }

    /// 長方形(幅 20 の寸法、左下の角を固定)と、外の点への距離の寸法と、写した原点。
    fn rect() -> (Sketch, Vec<EntityId>) {
        let mut s = Sketch::default();
        let lines = s
            .add_rectangle((None, [0.0, 0.0]), (None, [20.0, 10.0]))
            .unwrap();
        let Some(Entity::Line { a, .. }) = s.entity(lines[0]).cloned() else {
            panic!("線分")
        };
        s.add_constraint(ConstraintKind::Length {
            line: lines[0],
            value: "20 mm".into(),
        });
        s.add_constraint(ConstraintKind::Fix { point: a });
        let far = s.add_point(50.0, 0.0);
        s.add_constraint(ConstraintKind::Distance {
            a,
            b: far,
            value: "50 mm".into(),
        });
        (s, lines.to_vec())
    }

    #[test]
    fn copying_keeps_constraints_inside_and_drops_outside_ones() {
        let (s, lines) = rect();
        let clip = s.copy_entities(&lines).unwrap();
        assert_eq!(clip.len(), 4, "線分 4 本(線の点は数えない)");
        assert_eq!(clip.entities.len(), 8, "線分 4 本と角の 4 点");
        let kinds: Vec<&ConstraintKind> = clip.constraints.iter().map(|c| &c.kind).collect();
        assert!(
            kinds
                .iter()
                .any(|k| matches!(k, ConstraintKind::Length { .. }))
        );
        assert!(
            !kinds
                .iter()
                .any(|k| matches!(k, ConstraintKind::Fix { .. }))
        );
        assert!(
            !kinds
                .iter()
                .any(|k| matches!(k, ConstraintKind::Distance { .. }))
        );
        let c = clip.center();
        assert!((c[0] - 10.0).abs() < 1e-9 && (c[1] - 5.0).abs() < 1e-9);
    }

    #[test]
    fn pasting_adds_a_moved_copy_that_solves_like_the_original() {
        let (mut s, lines) = rect();
        let before = (s.entities.len(), s.constraints.len());
        let clip = s.copy_entities(&lines).unwrap();
        let new = s.paste(&clip, [100.0, 40.0]);
        assert_eq!(new.len(), 8);
        assert_eq!(s.entities.len(), before.0 + 8);
        assert_eq!(s.constraints.len(), before.1 + clip.constraints.len());
        let sol = solve(&s, &values(&s)).unwrap();
        let xs: Vec<[f64; 2]> = new
            .iter()
            .filter_map(|id| sol.points.get(id))
            .copied()
            .collect();
        assert_eq!(xs.len(), 4);
        assert!(
            xs.iter()
                .any(|p| (p[0] - 100.0).abs() < 1e-6 && (p[1] - 40.0).abs() < 1e-6)
        );
        assert!(
            xs.iter()
                .any(|p| (p[0] - 120.0).abs() < 1e-6 && (p[1] - 50.0).abs() < 1e-6)
        );
        // 写した長さの寸法は写しの線を指す
        let pasted_len = s
            .constraints
            .iter()
            .filter(
                |c| matches!(c.kind, ConstraintKind::Length { line, .. } if new.contains(&line)),
            )
            .count();
        assert_eq!(pasted_len, 1);
    }

    #[test]
    fn a_projected_line_is_pasted_as_a_plain_line() {
        let mut s = Sketch::default();
        let a = s.add_point(0.0, 0.0);
        let b = s.add_point(10.0, 0.0);
        let l = s.add_line(a, b);
        s.projections.push(Projection {
            entity: l,
            source: ProjectionSource::Origin,
        });
        let clip = s.copy_entities(&[l]).unwrap();
        let new = s.paste(&clip, [0.0, 5.0]);
        assert_eq!(s.projections.len(), 1, "写しは投影にならない");
        assert!(new.iter().all(|id| !s.is_projected(*id)));
    }

    #[test]
    fn a_dimension_keeps_its_place_and_reference_flag() {
        let mut s = Sketch::default();
        let c = s.add_point(0.0, 0.0);
        let circle = s.add_circle(c, 5.0);
        let id = s.add_constraint(ConstraintKind::Diameter {
            circle,
            value: "d1".into(),
        });
        let k = s.constraints.iter_mut().find(|k| k.id == id).unwrap();
        k.at = Some([8.0, 8.0]);
        k.reference = true;
        let clip = s.copy_entities(&[circle]).unwrap();
        s.paste(&clip, [10.0, 0.0]);
        let pasted = s.constraints.last().unwrap();
        assert!(pasted.reference);
        assert_eq!(pasted.at, Some([18.0, 8.0]));
        assert_eq!(pasted.kind.value(), Some("d1"), "式はそのまま");
    }
}
