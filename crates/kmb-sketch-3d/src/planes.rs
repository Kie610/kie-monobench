//! 平面(座標の平面・面・作図面)を点の位置から求める。解く時は変数の並びから、測る時と
//! 描く時は解いた位置から、同じ式で求める。

use crate::Solution3d;
use crate::geom::{add, centroid, cross, dot, len, newell, scale, sub, unit};
use crate::model::{FaceRef, P3, PlaneDef, PlaneRef, Sketch3d};
use kmb_sketch::EntityId;
use std::collections::HashMap;

/// 平面の式(変数の値から、平面の上の点と単位の法線)。
#[derive(Clone, Debug)]
pub(super) enum PlaneExpr {
    Fixed(P3, P3),
    Points([usize; 3]),
    /// 面の輪の点
    Loop(Vec<usize>),
    /// 線分(両端)を通り、相手の平面から傾けた平面
    Tilted {
        a: usize,
        b: usize,
        base: Box<PlaneExpr>,
        angle: f64,
        flip: bool,
    },
}

impl PlaneExpr {
    pub fn eval(&self, x: &[f64]) -> (P3, P3) {
        let get = |i: usize| [x[i], x[i + 1], x[i + 2]];
        match self {
            PlaneExpr::Fixed(o, n) => (*o, *n),
            PlaneExpr::Points([a, b, c]) => {
                let (pa, pb, pc) = (get(*a), get(*b), get(*c));
                let n = unit(cross(sub(pb, pa), sub(pc, pa)));
                (centroid(&[pa, pb, pc]), n)
            }
            PlaneExpr::Loop(idx) => {
                let xyz: Vec<P3> = idx.iter().map(|i| get(*i)).collect();
                (centroid(&xyz), unit(newell(&xyz)))
            }
            PlaneExpr::Tilted {
                a,
                b,
                base,
                angle,
                flip,
            } => {
                let (pa, pb) = (get(*a), get(*b));
                let d = unit(sub(pb, pa));
                let m = base.eval(x).1;
                // 相手の法線のうち線分に垂直な成分(線分が相手の平面に乗っていれば法線そのもの)
                let mut u = sub(m, scale(d, dot(m, d)));
                if len(u) < 1e-9 {
                    u = crate::basis(d).0;
                }
                let u = unit(u);
                let w = cross(d, u);
                let s = if *flip { -1.0 } else { 1.0 };
                let n = add(scale(u, angle.cos()), scale(w, s * angle.sin()));
                (scale(add(pa, pb), 0.5), n)
            }
        }
    }
}

/// 面の輪の点の順(線分の番号から、つながりをたどる)。
pub(super) fn loop_points(sketch: &Sketch3d, f: &FaceRef) -> Option<Vec<EntityId>> {
    let merged = crate::faces::merged_points(sketch);
    let rep = |p: EntityId| *merged.get(&p).unwrap_or(&p);
    let ends: Vec<(EntityId, EntityId)> =
        f.0.iter()
            .map(|l| sketch.line_ends(*l).map(|(a, b)| (rep(a), rep(b))))
            .collect::<Option<_>>()?;
    let (first, mut cur) = *ends.first()?;
    let mut pts = vec![first];
    let mut used = vec![false; ends.len()];
    used[0] = true;
    while cur != first {
        pts.push(cur);
        let k = (0..ends.len()).find(|&k| !used[k] && (ends[k].0 == cur || ends[k].1 == cur))?;
        used[k] = true;
        cur = if ends[k].0 == cur {
            ends[k].1
        } else {
            ends[k].0
        };
    }
    (pts.len() >= 3 && used.iter().all(|u| *u)).then_some(pts)
}

/// 平面の式を作る。`idx` は点の変数の位置(点でなければ `None`)。作れなければ `None`。
pub(super) fn plane_expr(
    sk: &Sketch3d,
    r: &PlaneRef,
    idx: &dyn Fn(EntityId) -> Option<usize>,
) -> Option<PlaneExpr> {
    expr(sk, r, idx, 0)
}

fn expr(
    sk: &Sketch3d,
    r: &PlaneRef,
    idx: &dyn Fn(EntityId) -> Option<usize>,
    depth: usize,
) -> Option<PlaneExpr> {
    // 作図面が自分を相手に傾けている時に止める
    if depth > 16 {
        return None;
    }
    Some(match r {
        PlaneRef::Axis { plane } => PlaneExpr::Fixed([0.0; 3], plane.normal().unit()),
        PlaneRef::Face { lines } => PlaneExpr::Loop(
            loop_points(sk, lines)?
                .iter()
                .map(|p| idx(*p))
                .collect::<Option<_>>()?,
        ),
        PlaneRef::Work { id } => match &sk.work_plane(*id)?.def {
            PlaneDef::Offset { base, offset } => {
                let n = base.normal().unit();
                PlaneExpr::Fixed(scale(n, *offset), n)
            }
            PlaneDef::ThreePoints { a, b, c } => PlaneExpr::Points([idx(*a)?, idx(*b)?, idx(*c)?]),
            PlaneDef::Tilted {
                line,
                base,
                angle,
                flip,
            } => {
                let (a, b) = sk.line_ends(*line)?;
                PlaneExpr::Tilted {
                    a: idx(a)?,
                    b: idx(b)?,
                    base: Box::new(expr(sk, base, idx, depth + 1)?),
                    angle: *angle,
                    flip: *flip,
                }
            }
            PlaneDef::Face { lines } => expr(
                sk,
                &PlaneRef::Face {
                    lines: lines.clone(),
                },
                idx,
                depth + 1,
            )?,
        },
    })
}

/// 平面が使う面(解く時に平らに保つ面)。
pub(super) fn faces_of(sk: &Sketch3d, r: &PlaneRef) -> Vec<FaceRef> {
    let mut out = Vec::new();
    let mut todo = vec![r.clone()];
    let mut seen = Vec::new();
    while let Some(r) = todo.pop() {
        match r {
            PlaneRef::Axis { .. } => {}
            PlaneRef::Face { lines } => out.push(lines),
            PlaneRef::Work { id } => {
                if seen.contains(&id) {
                    continue;
                }
                seen.push(id);
                match sk.work_plane(id).map(|w| &w.def) {
                    Some(PlaneDef::Face { lines }) => out.push(lines.clone()),
                    Some(PlaneDef::Tilted { base, .. }) => todo.push(base.clone()),
                    _ => {}
                }
            }
        }
    }
    out
}

/// 解いた位置での平面(平面の上の点と単位の法線)。求められなければ `None`。
pub fn plane_at(sk: &Sketch3d, sol: &Solution3d, r: &PlaneRef) -> Option<(P3, P3)> {
    let mut x = Vec::with_capacity(sol.points.len() * 3);
    let mut pos = HashMap::new();
    for (id, p) in &sol.points {
        pos.insert(*id, x.len());
        x.extend_from_slice(p);
    }
    Some(plane_expr(sk, r, &|id| pos.get(&id).copied())?.eval(&x))
}
