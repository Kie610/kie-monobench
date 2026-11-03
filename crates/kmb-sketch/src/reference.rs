//! ボディの角・辺・円の中心と原点を、スケッチの平面へ写す(投影)。
//!
//! 画面はこれで既存の形を下に描き、吸い付く先と寸法の相手にする。計算の時は
//! [`refresh_projections`] が、スケッチに写した要素の位置を元の形から取り直す。

use crate::model::{Entity, ProjectionSource, Sketch};
use kmb_core::{FeatureId, Kernel, Plane, ShapeHandle};

type P2 = [f64; 2];
type P3 = [f64; 3];
use kmb_core::kernel::{dot, sub};
use std::collections::{BTreeMap, BTreeSet};

/// スケッチの平面へ写した、参照できる形の 1 つ。
#[derive(Debug, Clone, PartialEq)]
pub struct RefItem {
    pub source: ProjectionSource,
    pub shape: RefShape,
}

/// 写した形(スケッチの平面の座標、mm)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RefShape {
    Point(P2),
    Line(P2, P2),
    /// 円、または `arc` があれば (始点, 終点) の反時計回りの円弧
    Circle {
        center: P2,
        radius: f64,
        arc: Option<(P2, P2)>,
    },
}

/// 作業軸を平面へ正射影する。表示範囲の両端を使い、垂直な軸は通る点になる。
pub fn datum_axis_ref(
    axis: kmb_interchange::DatumAxis,
    feature: FeatureId,
    plane: &Plane,
) -> Result<RefItem, String> {
    if !axis
        .at
        .into_iter()
        .chain(axis.dir)
        .chain(axis.span)
        .all(f64::is_finite)
        || dot(axis.dir, axis.dir) < 1e-20
        || axis.span[1] <= axis.span[0]
    {
        return Err(format!(
            "作業軸 {} の位置・向き・表示範囲が不正です。軸を直してから投影してください",
            feature.0
        ));
    }
    let [a, b] = axis.ends().map(|p| plane.to_local(p));
    if !a.into_iter().chain(b).all(f64::is_finite) {
        return Err(format!("作業軸 {} の投影を計算できません", feature.0));
    }
    let shape = if (a[0] - b[0]).hypot(a[1] - b[1]) <= 1e-9 {
        RefShape::Point(plane.to_local(axis.at))
    } else {
        RefShape::Line(a, b)
    };
    Ok(RefItem {
        source: ProjectionSource::DatumAxis { feature },
        shape,
    })
}

/// 原点の参照(スケッチの原点。XY・YZ・ZX と古い面のスケッチでは世界の原点を写した点、新しい
/// 面のスケッチでは面の角。[`crate::FaceOrigin`])。
pub fn origin_ref(plane: &Plane) -> RefItem {
    RefItem {
        source: ProjectionSource::Origin,
        shape: RefShape::Point(plane.to_local(plane.origin)),
    }
}

/// 位置を数えるための丸め(0.1 µm)
fn key(p: P3) -> (i64, i64, i64) {
    let r = |v: f64| (v * 1e4).round() as i64;
    (r(p[0]), r(p[1]), r(p[2]))
}

fn key2(p: P2) -> (i64, i64) {
    let r = |v: f64| (v * 1e4).round() as i64;
    (r(p[0]), r(p[1]))
}

/// ボディ `shape`(機能 `feature` の `body`)の角・辺・円の中心を、平面 `plane` へ写す。
/// まっすぐな辺は写した長さが 0 でないもの、円の辺は軸が平面の法線に平行なものだけ
/// (斜めの円は楕円になるので写さない)。同じ位置に写る角は、平面に近い方を残す。
pub fn body_refs(
    kernel: &dyn Kernel,
    feature: FeatureId,
    shape: ShapeHandle,
    plane: &Plane,
) -> Vec<RefItem> {
    refs_of(kernel, feature, shape, plane, true)
}

/// [`body_refs`] の辺と円の中心を、同じ位置に写る物を 1 つにまとめずに全部(面の縁を写す時に、
/// 平面から遠い面の辺も要る)。角は含めない。
pub fn body_edge_refs(
    kernel: &dyn Kernel,
    feature: FeatureId,
    shape: ShapeHandle,
    plane: &Plane,
) -> Vec<RefItem> {
    refs_of(kernel, feature, shape, plane, false)
        .into_iter()
        .filter(|r| !matches!(r.source, ProjectionSource::Vertex { .. }))
        .collect()
}

fn refs_of(
    kernel: &dyn Kernel,
    feature: FeatureId,
    shape: ShapeHandle,
    plane: &Plane,
    merge: bool,
) -> Vec<RefItem> {
    let mut edges = kernel.edges(shape).unwrap_or_default();
    edges.extend(kernel.arc_edges(shape).unwrap_or_default());
    let n = plane.normal();
    let height = |p: P3| dot(sub(p, plane.origin), n).abs();
    let full = |e: &kmb_core::EdgeInfo| {
        e.arc
            .is_some_and(|a| (a.sweep.abs() - std::f64::consts::TAU).abs() < 1e-6)
    };
    // 角: 辺の端の位置ごとに、そこへ来る辺の面を集める(一周の円の継ぎ目は角にしない)
    let mut corners: BTreeMap<(i64, i64, i64), (P3, BTreeSet<String>)> = BTreeMap::new();
    for e in edges.iter().filter(|e| !full(e)) {
        for p in [e.a, e.b] {
            let c = corners.entry(key(p)).or_insert((p, BTreeSet::new()));
            c.1.extend(e.faces.iter().cloned());
        }
    }
    let mut out = Vec::new();
    let mut seen: BTreeMap<(i64, i64), (f64, usize)> = BTreeMap::new();
    for (p, faces) in corners.into_values() {
        let q = plane.to_local(p);
        let h = height(p);
        let item = RefItem {
            source: ProjectionSource::Vertex {
                feature,
                faces: faces.into_iter().collect(),
            },
            shape: RefShape::Point(q),
        };
        match seen.get(&key2(q)) {
            Some(&(h0, i)) if h < h0 - 1e-9 => {
                out[i] = item;
                seen.insert(key2(q), (h, i));
            }
            Some(_) => {}
            None => {
                seen.insert(key2(q), (h, out.len()));
                out.push(item);
            }
        }
    }
    // 写した線(両端の位置)ごとに、平面からの高さと out の何番目か
    type Ends = ((i64, i64), (i64, i64));
    let mut lines: BTreeMap<Ends, (f64, usize)> = BTreeMap::new();
    for e in &edges {
        let faces = e.faces.clone();
        match e.arc {
            None => {
                let (a, b) = (plane.to_local(e.a), plane.to_local(e.b));
                if (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-6 {
                    continue;
                }
                // 上下の面の同じ形の辺は同じ線に写るので、平面に近い方を残す
                let k = if key2(a) <= key2(b) {
                    (key2(a), key2(b))
                } else {
                    (key2(b), key2(a))
                };
                let h = height(e.a).max(height(e.b));
                let item = RefItem {
                    source: ProjectionSource::Edge { feature, faces },
                    shape: RefShape::Line(a, b),
                };
                match lines.get(&k).filter(|_| merge) {
                    Some(&(h0, i)) if h < h0 - 1e-9 => {
                        out[i] = item;
                        lines.insert(k, (h, i));
                    }
                    Some(_) => {}
                    None => {
                        lines.insert(k, (h, out.len()));
                        out.push(item);
                    }
                }
            }
            Some(arc) => {
                let along = dot(arc.axis, n);
                if along.abs() < 1.0 - 1e-6 {
                    continue;
                }
                let center = plane.to_local(arc.center);
                let ends = if full(e) {
                    None
                } else {
                    // 軸が法線と同じ向きで正に回るなら a から b へ反時計回り
                    let (a, b) = (plane.to_local(e.a), plane.to_local(e.b));
                    Some(if (along > 0.0) == (arc.sweep > 0.0) {
                        (a, b)
                    } else {
                        (b, a)
                    })
                };
                out.push(RefItem {
                    source: ProjectionSource::Edge {
                        feature,
                        faces: faces.clone(),
                    },
                    shape: RefShape::Circle {
                        center,
                        radius: arc.radius,
                        arc: ends,
                    },
                });
                out.push(RefItem {
                    source: ProjectionSource::Center { feature, faces },
                    shape: RefShape::Point(center),
                });
            }
        }
    }
    out
}

/// 写した要素の位置を、元の形から取り直す。`refs` は元の形を写したもの
/// ([`origin_ref`] と [`body_refs`])。元が見つからない要素(辺が消えた時など)は、
/// 前の位置のまま残し、その要素を返す(画面で「参照が切れた」と知らせる)。
pub fn refresh_projections(sketch: &mut Sketch, refs: &[RefItem]) -> Vec<crate::EntityId> {
    let mut lost = Vec::new();
    for pr in sketch.projections.clone() {
        let Some(item) = refs.iter().find(|r| r.source.same(&pr.source)) else {
            lost.push(pr.entity);
            continue;
        };
        let Some(entity) = sketch.entity(pr.entity).cloned() else {
            continue;
        };
        match (entity, item.shape) {
            (Entity::Point { id, .. }, RefShape::Point(p)) => sketch.set_point(id, p[0], p[1]),
            (Entity::Line { a, b, .. }, RefShape::Line(p, q)) => {
                // 辺の向きが変わっても端が入れ替わらないよう、今の位置に近い向きで当てる
                let (pa, pb) = (point_of(sketch, a), point_of(sketch, b));
                let d = |u: P2, v: P2| (u[0] - v[0]).hypot(u[1] - v[1]);
                let (p, q) = if d(pa, p) + d(pb, q) <= d(pa, q) + d(pb, p) {
                    (p, q)
                } else {
                    (q, p)
                };
                sketch.set_point(a, p[0], p[1]);
                sketch.set_point(b, q[0], q[1]);
            }
            (
                Entity::Circle { id, center, .. },
                RefShape::Circle {
                    center: c, radius, ..
                },
            ) => {
                sketch.set_point(center, c[0], c[1]);
                for e in &mut sketch.entities {
                    if let Entity::Circle {
                        id: i, radius: r, ..
                    } = e
                        && *i == id
                    {
                        *r = radius;
                    }
                }
            }
            (
                Entity::Arc {
                    center, start, end, ..
                },
                RefShape::Circle {
                    center: c,
                    arc: Some((s, e)),
                    ..
                },
            ) => {
                sketch.set_point(center, c[0], c[1]);
                sketch.set_point(start, s[0], s[1]);
                sketch.set_point(end, e[0], e[1]);
            }
            _ => lost.push(pr.entity),
        }
    }
    lost
}

/// 投影の固定目標と寸法を替え、更新前の形から近い解へ付いて行く。
/// 形を保って解けなければ理由を返し、渡されたスケッチを変更しない。
pub fn refresh_projections_with_values(
    sketch: &mut Sketch,
    refs: &[RefItem],
    values: &std::collections::HashMap<crate::ConstraintId, f64>,
) -> Result<Vec<crate::EntityId>, String> {
    let before = sketch.clone();
    let lost = refresh_projections(sketch, refs);
    if let Err(reason) = crate::solver::follow_projection_targets(&before, sketch, values) {
        *sketch = before;
        return Err(reason);
    }
    Ok(lost)
}

fn point_of(sketch: &Sketch, id: crate::EntityId) -> P2 {
    match sketch.entity(id) {
        Some(Entity::Point { x, y, .. }) => [*x, *y],
        _ => [0.0, 0.0],
    }
}

impl Sketch {
    /// 参照 `item` をスケッチに写す(補助線として足し、投影として記録する)。
    /// 同じ元を写した要素が既にあれば、それを返す。点は、写した位置に既にある写した点を使い回す。
    pub fn add_projection(&mut self, item: &RefItem) -> crate::EntityId {
        self.add_projection_as(item, true)
    }

    /// [`Self::add_projection`] の補助線にするかを選べる形(交差の線は輪郭に使うので補助線にしない)。
    /// 断面の線の端は、同じボディの断面の線とだけ点を分け合う(角の投影とは、ボディの形が
    /// 変わると位置がずれるため)。
    pub fn add_projection_as(&mut self, item: &RefItem, construction: bool) -> crate::EntityId {
        if let Some(p) = self
            .projections
            .iter()
            .find(|p| p.source.same(&item.source))
        {
            return p.entity;
        }
        let section = |src: &ProjectionSource| matches!(src, ProjectionSource::Section { .. });
        let mine = section(&item.source);
        let point = |s: &mut Sketch, p: P2| -> crate::EntityId {
            let (projected, _) = s.projected_parts();
            // 断面は断面とだけ点を共有する。独立して動く作業軸は、別の元の点を使い回さない。
            let fits = |s: &Sketch, id: crate::EntityId| {
                s.projections.iter().all(|pr| {
                    let uses = pr.entity == id
                        || s.entity(pr.entity)
                            .is_some_and(|e| e.points().contains(&id));
                    !uses
                        || (section(&pr.source) == mine
                            && ((pr.source.datum_axis().is_none()
                                && item.source.datum_axis().is_none())
                                || pr.source.same(&item.source)))
                })
            };
            let near = s.entities.iter().find_map(|e| match e {
                Entity::Point { id, x, y }
                    if projected.contains(id)
                        && (x - p[0]).hypot(y - p[1]) < 1e-6
                        && fits(s, *id) =>
                {
                    Some(*id)
                }
                _ => None,
            });
            near.unwrap_or_else(|| s.add_point(p[0], p[1]))
        };
        let id = match item.shape {
            RefShape::Point(p) => point(self, p),
            RefShape::Line(a, b) => {
                let (pa, pb) = (point(self, a), point(self, b));
                let l = self.add_line(pa, pb);
                self.set_construction(l, construction);
                l
            }
            RefShape::Circle {
                center,
                radius,
                arc,
            } => {
                let c = point(self, center);
                let e = match arc {
                    None => self.add_circle(c, radius),
                    Some((s, e)) => {
                        let (ps, pe) = (point(self, s), point(self, e));
                        self.add_arc(c, ps, pe)
                    }
                };
                self.set_construction(e, construction);
                e
            }
        };
        self.projections.push(crate::Projection {
            entity: id,
            source: item.source.clone(),
        });
        id
    }
}

/// 別のスケッチから写せる要素の出力の型の名前(K8)。
pub const ITEMS_TYPE: &str = "sketch.items@1";

/// スケッチの要素(線分・円・円弧・単独の点)を、そのスケッチの平面の座標で並べた物(`items`
/// スロット)。後のスケッチが、寸法や拘束の相手に写すのに使う(K8)。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SketchItems {
    pub plane: Plane,
    pub items: Vec<SketchItem>,
}

/// スケッチの要素 1 つ(平面の座標、mm)。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SketchItem {
    pub entity: crate::EntityId,
    pub shape: ItemShape,
}

/// 要素の形。円弧は中心のまわりに反時計回りに (始点, 終点)。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ItemShape {
    Point { at: P2 },
    Line { a: P2, b: P2 },
    Circle { center: P2, radius: f64 },
    Arc { center: P2, start: P2, end: P2 },
}

impl kmb_core::Payload for SketchItems {
    const TYPE: &'static str = ITEMS_TYPE;
}

/// 解いた形の、写せる要素(線分・円・円弧と、線や円に使っていない点)。
pub fn sketch_items(sketch: &Sketch, sol: &crate::Solution, plane: Plane) -> SketchItems {
    let at = |id: &crate::EntityId| sol.points.get(id).copied();
    let used: BTreeSet<crate::EntityId> = sketch
        .entities
        .iter()
        .filter(|e| !matches!(e, Entity::Point { .. }))
        .flat_map(|e| e.points())
        .collect();
    let items = sketch
        .entities
        .iter()
        .filter_map(|e| {
            let shape = match e {
                Entity::Point { id, .. } if !used.contains(id) => ItemShape::Point { at: at(id)? },
                Entity::Line { a, b, .. } => ItemShape::Line {
                    a: at(a)?,
                    b: at(b)?,
                },
                Entity::Circle { id, center, .. } => ItemShape::Circle {
                    center: at(center)?,
                    radius: *sol.radii.get(id)?,
                },
                Entity::Arc {
                    center, start, end, ..
                } => ItemShape::Arc {
                    center: at(center)?,
                    start: at(start)?,
                    end: at(end)?,
                },
                _ => return None,
            };
            Some(SketchItem {
                entity: e.id(),
                shape,
            })
        })
        .collect();
    SketchItems { plane, items }
}

/// 別のスケッチ `feature` の要素を、平面 `plane` へ写した参照。円と円弧は、平面が平行な時だけ
/// (斜めだと楕円になるので写さない)。
pub fn sketch_refs(items: &SketchItems, feature: FeatureId, plane: &Plane) -> Vec<RefItem> {
    let to = |p: P2| plane.to_local(items.plane.to_world(p));
    let facing = dot(items.plane.normal(), plane.normal());
    let parallel = (facing.abs() - 1.0).abs() < 1e-9;
    items
        .items
        .iter()
        .filter_map(|it| {
            let shape = match it.shape {
                ItemShape::Point { at } => RefShape::Point(to(at)),
                ItemShape::Line { a, b } => {
                    let (a, b) = (to(a), to(b));
                    if (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-9 {
                        return None;
                    }
                    RefShape::Line(a, b)
                }
                ItemShape::Circle { center, radius } if parallel => RefShape::Circle {
                    center: to(center),
                    radius,
                    arc: None,
                },
                ItemShape::Arc { center, start, end } if parallel => {
                    let (s, e) = (to(start), to(end));
                    let c = to(center);
                    RefShape::Circle {
                        center: c,
                        radius: (s[0] - c[0]).hypot(s[1] - c[1]),
                        // 裏から見ると回る向きが逆になる
                        arc: Some(if facing > 0.0 { (s, e) } else { (e, s) }),
                    }
                }
                _ => return None,
            };
            Some(RefItem {
                source: ProjectionSource::Sketch {
                    feature,
                    item: it.entity,
                },
                shape,
            })
        })
        .collect()
}
