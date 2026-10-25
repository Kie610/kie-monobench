//! 閉じた線分の輪から面を見つける。輪の点がみな 1 つの平面に乗り、輪の中を横切る
//! 別の線分(弦)がないものを面とする(立方体なら 6 つの四角)。補助線は使わない。
//! 一致の拘束でつないだ点は同じ点とみなす。

use crate::geom::{centroid, cross, dot, len, newell, sub, unit};
use crate::model::{Entity3d, FaceRef, Kind3d, P3, Sketch3d};
use jc_sketch::EntityId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// 見つけた面。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Face3d {
    /// 輪の線分(小さい順。拘束から面を指す時の名前)
    pub lines: FaceRef,
    /// 輪の点(輪をたどる順)
    pub points: Vec<EntityId>,
    /// 単位の法線(輪をたどる向きで右ねじ)
    pub normal: P3,
    /// 重心
    pub center: P3,
}

/// 輪の長さの上限(これより長い輪は面にしない。探す手間を抑える)。
const MAX_LOOP: usize = 16;

/// 一致でつないだ点を代表の点にまとめる表。
pub(crate) fn merged_points(sketch: &Sketch3d) -> HashMap<EntityId, EntityId> {
    let mut parent: HashMap<EntityId, EntityId> = HashMap::new();
    fn find(parent: &mut HashMap<EntityId, EntityId>, x: EntityId) -> EntityId {
        let p = *parent.get(&x).unwrap_or(&x);
        if p == x {
            return x;
        }
        let r = find(parent, p);
        parent.insert(x, r);
        r
    }
    for c in &sketch.constraints {
        if let Kind3d::Coincident { a, b } = c.kind {
            let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
            if ra != rb {
                let (lo, hi) = if ra < rb { (ra, rb) } else { (rb, ra) };
                parent.insert(hi, lo);
            }
        }
    }
    let keys: Vec<EntityId> = parent.keys().copied().collect();
    let mut out = HashMap::new();
    for k in keys {
        let r = find(&mut parent, k);
        out.insert(k, r);
    }
    out
}

/// 面を見つける(`pos` は点の位置)。
pub fn find_faces(sketch: &Sketch3d, pos: &BTreeMap<EntityId, P3>) -> Vec<Face3d> {
    let merged = merged_points(sketch);
    let rep = |p: EntityId| *merged.get(&p).unwrap_or(&p);
    // 線分の両端(代表の点)
    let lines: Vec<(EntityId, EntityId, EntityId)> = sketch
        .entities
        .iter()
        .filter_map(|e| match e {
            Entity3d::Line {
                id,
                a,
                b,
                construction: false,
            } => Some((*id, rep(*a), rep(*b))),
            _ => None,
        })
        .filter(|(_, a, b)| a != b)
        .collect();
    let mut adj: BTreeMap<EntityId, Vec<(EntityId, EntityId)>> = BTreeMap::new();
    for (l, a, b) in &lines {
        adj.entry(*a).or_default().push((*l, *b));
        adj.entry(*b).or_default().push((*l, *a));
    }
    let at = |p: EntityId| pos.get(&p).copied();
    // 大きさの目安(平面に乗っているかの許容差に使う)
    let size = pos
        .values()
        .map(|p| p.iter().fold(0.0f64, |m, x| m.max(x.abs())))
        .fold(1.0f64, f64::max);
    // 描いた点はマウスの位置から来るので、少しの誤差は同じ平面とみなす(大きさの 1 万分の 1)
    let tol = 1e-5 * size;
    let mut seen: BTreeSet<Vec<EntityId>> = BTreeSet::new();
    let mut out = Vec::new();
    for &(start, a, b) in &lines {
        // 輪の中で一番小さい番号の線分から始める輪だけを数える(同じ輪を何度も見ない)
        let mut path_pts = vec![a, b];
        let mut path_lines = vec![start];
        let mut found = Vec::new();
        search(
            &adj,
            &at,
            tol,
            start,
            &mut path_pts,
            &mut path_lines,
            &mut found,
        );
        for (pts, ls) in found {
            let key = {
                let mut k = ls.clone();
                k.sort();
                k
            };
            if !seen.insert(key.clone()) {
                continue;
            }
            let Some(xyz) = pts.iter().map(|p| at(*p)).collect::<Option<Vec<P3>>>() else {
                continue;
            };
            let n = newell(&xyz);
            if len(n) < tol {
                continue;
            }
            let normal = unit(n);
            let center = centroid(&xyz);
            // 弦(輪の 2 点をつなぐ、輪に入っていない線分)があれば、小さい面に分かれるので除く
            let in_loop: BTreeSet<EntityId> = pts.iter().copied().collect();
            let chord = lines
                .iter()
                .any(|(l, p, q)| !key.contains(l) && in_loop.contains(p) && in_loop.contains(q));
            if chord {
                continue;
            }
            out.push(Face3d {
                lines: FaceRef::new(key),
                points: pts,
                normal,
                center,
            });
        }
    }
    out
}

/// 深さ優先で、平面に乗ったまま始めの点へ戻る輪を探す。
fn search(
    adj: &BTreeMap<EntityId, Vec<(EntityId, EntityId)>>,
    at: &dyn Fn(EntityId) -> Option<P3>,
    tol: f64,
    start: EntityId,
    pts: &mut Vec<EntityId>,
    lines: &mut Vec<EntityId>,
    found: &mut Vec<(Vec<EntityId>, Vec<EntityId>)>,
) {
    if lines.len() >= MAX_LOOP {
        return;
    }
    let last = *pts.last().expect("2 点から始める");
    let plane = plane_of(pts, at, tol);
    for &(l, next) in adj.get(&last).into_iter().flatten() {
        if l <= start || lines.contains(&l) {
            continue;
        }
        if next == pts[0] {
            if pts.len() >= 3 && plane.is_some() {
                found.push((pts.clone(), {
                    let mut v = lines.clone();
                    v.push(l);
                    v
                }));
            }
            continue;
        }
        if pts.contains(&next) {
            continue;
        }
        // 平面が決まっていれば、乗らない点へは進まない
        if let (Some((o, n)), Some(q)) = (plane, at(next))
            && dot(sub(q, o), n).abs() > tol.max(1e-9) * 10.0
        {
            continue;
        }
        pts.push(next);
        lines.push(l);
        search(adj, at, tol, start, pts, lines, found);
        pts.pop();
        lines.pop();
    }
}

/// 点の並びの平面(並びに一直線でない 3 点があれば、点と単位の法線)。
fn plane_of(pts: &[EntityId], at: &dyn Fn(EntityId) -> Option<P3>, tol: f64) -> Option<(P3, P3)> {
    let xyz: Vec<P3> = pts.iter().filter_map(|p| at(*p)).collect();
    let o = *xyz.first()?;
    for i in 1..xyz.len() {
        for j in i + 1..xyz.len() {
            let n = cross(sub(xyz[i], o), sub(xyz[j], o));
            if len(n) > tol * 1e3 {
                return Some((o, unit(n)));
            }
        }
    }
    None
}
