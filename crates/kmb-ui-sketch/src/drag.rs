//! 選ぶ道具のドラッグ。押した所の線・円・点を掴んで動かす(拘束を保って解き直す)か、
//! 何もない所から囲んで選ぶ。形はドラッグで作らない(既存の要素を動かす・囲むだけ)。

use kmb_sketch::{ConstraintKind, Entity, EntityId, Sketch, Solution};
use std::collections::BTreeSet;

/// 写した要素を掴んだ時の知らせ。
pub(crate) const PROJECTED: &str = "既存の形から写した要素は、元の形で決まるので動かせません";
/// 固定の拘束が付いた点だけを掴んだ時の知らせ。
pub(crate) const FIXED: &str = "固定の拘束が付いているので動かせません(拘束を外すと動かせます)";
/// 引いても拘束で動かなかった時の知らせ。
pub(crate) const STUCK: &str =
    "寸法と拘束で決まっているので動かせません(寸法の値を直すか、拘束を外す)";

/// 掴んだものの動かし方。位置は押した時のもの。
#[derive(Clone, Debug)]
pub(crate) enum Motion {
    /// 点を同じだけ動かす(点・線分・選んだもの全部)
    Translate(Vec<(EntityId, [f64; 2])>),
    /// 円周を引いて半径を変える(中心は動かさない)
    CircleRadius { circle: EntityId, center: [f64; 2] },
    /// 円弧を引いて半径を変える(中心は動かさず、両端を中心から離す・近づける)
    ArcRadius {
        center: [f64; 2],
        ends: Vec<(EntityId, [f64; 2])>,
    },
}

/// 掴んでいるもの。
pub(crate) struct Grab {
    /// 押した位置(mm)
    origin: [f64; 2],
    motion: Motion,
    /// 解く時に、なるべく動かさない要素(引いている要素)
    pub held: BTreeSet<EntityId>,
    /// 引き終えたら選ぶもの
    pub select: BTreeSet<EntityId>,
    /// 動かせない理由(掴んだ時に分かったもの)
    pub blocked: Option<&'static str>,
    /// 引いても動かなかった(拘束で決まっている)
    pub stuck: bool,
    /// 最後にうまく解けた形とその解(離した時に決める)
    pub last: Option<(Sketch, Solution)>,
    /// 点を 1 つだけ掴んだ時の点(別の点・線の上で離すと一致・線上を付ける。K21)
    pub point: Option<EntityId>,
    /// 今離すと付く拘束の名前(カーソルの脇に出す)
    pub drop: Option<&'static str>,
}

/// 要素が使う点(点はそれ自身)。
fn points_of(sketch: &Sketch, id: EntityId) -> Vec<EntityId> {
    match sketch.entity(id) {
        Some(Entity::Point { .. }) => vec![id],
        Some(e) => e.points(),
        None => vec![],
    }
}

fn has_fix(sketch: &Sketch, p: EntityId) -> bool {
    sketch
        .constraints
        .iter()
        .any(|c| matches!(c.kind, ConstraintKind::Fix { point } if point == p))
}

/// 要素が寸法と拘束で完全に決まっている(掴んでも動かせない)。
/// 引くたびに解き直してから「動かない」と分かるのでなく、掴んだ時に分かる。解けていない形では
/// 決まっているかが当てにならないので、真にしない。
fn determined(sol: &Solution, ids: impl IntoIterator<Item = EntityId>) -> bool {
    !matches!(sol.status, kmb_sketch::SolveStatus::Inconsistent { .. })
        && ids.into_iter().all(|id| sol.fixed.contains(&id))
}

fn is_projected(sketch: &Sketch, id: EntityId) -> bool {
    sketch.is_projected(id) || sketch.projected_parts().0.contains(&id)
}

/// 押した所 `w` の要素を掴む。何もなければ `None`(囲んで選ぶ)。
/// 選んだものの 1 つを押した時は、選んだもの全部を同じだけ動かす。
pub(crate) fn grab(
    sketch: &Sketch,
    sol: &Solution,
    selection: &BTreeSet<EntityId>,
    w: [f64; 2],
    tol: f64,
) -> Option<Grab> {
    let hit = sketch.hit_test(sol, w[0], w[1], tol)?.entity;
    let pos = |p: EntityId| sol.points.get(&p).copied().unwrap_or([0.0, 0.0]);
    let mut g = Grab {
        origin: w,
        motion: Motion::Translate(vec![]),
        held: BTreeSet::new(),
        select: BTreeSet::from([hit]),
        blocked: None,
        stuck: false,
        last: None,
        point: None,
        drop: None,
    };
    let group = selection.contains(&hit) && selection.len() > 1;
    let single_curve = match sketch.entity(hit) {
        Some(Entity::Circle { center, .. }) if !group => Some(Motion::CircleRadius {
            circle: hit,
            center: pos(*center),
        }),
        Some(Entity::Arc {
            center, start, end, ..
        }) if !group => Some(Motion::ArcRadius {
            center: pos(*center),
            ends: vec![(*start, pos(*start)), (*end, pos(*end))],
        }),
        _ => None,
    };
    if let Some(m) = single_curve {
        if is_projected(sketch, hit) {
            g.blocked = Some(PROJECTED);
        } else if match &m {
            Motion::CircleRadius { circle, .. } => determined(sol, [*circle]),
            Motion::ArcRadius { ends, .. } => determined(sol, ends.iter().map(|(p, _)| *p)),
            Motion::Translate(_) => false,
        } {
            g.blocked = Some(STUCK);
        }
        g.held = points_of(sketch, hit).into_iter().collect();
        g.held.insert(hit);
        g.motion = m;
        return Some(g);
    }
    let ids: Vec<EntityId> = if group {
        g.select = selection.clone();
        selection.iter().copied().collect()
    } else {
        vec![hit]
    };
    let mut pts: Vec<EntityId> = ids.iter().flat_map(|id| points_of(sketch, *id)).collect();
    pts.sort();
    pts.dedup();
    let movable: Vec<EntityId> = pts
        .iter()
        .copied()
        .filter(|p| !is_projected(sketch, *p) && !has_fix(sketch, *p))
        .collect();
    if movable.is_empty() {
        g.blocked = Some(if pts.iter().any(|p| is_projected(sketch, *p)) {
            PROJECTED
        } else {
            FIXED
        });
    } else if determined(sol, movable.iter().copied()) {
        g.blocked = Some(STUCK);
    }
    // 選んだ円は半径を保つ(中心だけ動かす)
    g.held = movable.iter().copied().collect();
    g.held.extend(
        ids.iter()
            .filter(|id| matches!(sketch.entity(**id), Some(Entity::Circle { .. }))),
    );
    if !group && matches!(sketch.entity(hit), Some(Entity::Point { .. })) && movable == [hit] {
        g.point = Some(hit);
    }
    g.motion = Motion::Translate(movable.into_iter().map(|p| (p, pos(p))).collect());
    Some(g)
}

/// 引いた点 `point` を `w` で離した時に付ける拘束と、その名前。別の点の上なら一致、線分の上なら線上、円・円弧の上なら円周上。引いた点と、
/// 一致でつながった点、その点を使う線(自分の線の上や、自分の線の反対の端)は相手にしない。
pub(crate) fn drop_on(
    sketch: &Sketch,
    sol: &Solution,
    point: EntityId,
    w: [f64; 2],
    tol: f64,
) -> Option<(ConstraintKind, &'static str)> {
    // 一致でつながった点(引いた点と一緒に動く)
    let mut tied = BTreeSet::from([point]);
    loop {
        let more: Vec<EntityId> = sketch
            .constraints
            .iter()
            .filter_map(|c| match c.kind {
                ConstraintKind::Coincident { a, b } if tied.contains(&a) != tied.contains(&b) => {
                    Some(if tied.contains(&a) { b } else { a })
                }
                _ => None,
            })
            .collect();
        if more.is_empty() {
            break;
        }
        tied.extend(more);
    }
    let own: Vec<&Entity> = sketch
        .entities
        .iter()
        .filter(|e| !matches!(e, Entity::Point { .. }))
        .filter(|e| e.points().iter().any(|p| tied.contains(p)))
        .collect();
    let mut skip = tied.clone();
    for e in &own {
        skip.insert(e.id());
        skip.extend(e.points());
    }
    let near = sketch
        .hits(sol, w[0], w[1], tol)
        .into_iter()
        .find(|h| !skip.contains(&h.entity))?;
    match sketch.entity(near.entity) {
        Some(Entity::Point { .. }) => Some((
            ConstraintKind::Coincident {
                a: point,
                b: near.entity,
            },
            "一致",
        )),
        Some(Entity::Line { .. }) => Some((
            ConstraintKind::PointOnLine {
                point,
                line: near.entity,
            },
            "線上",
        )),
        Some(Entity::Circle { .. } | Entity::Arc { .. }) => Some((
            ConstraintKind::PointOnCircle {
                point,
                circle: near.entity,
            },
            "円周上",
        )),
        _ => None,
    }
}

impl Grab {
    /// カーソル `w` へ引いた位置に書き換えたスケッチ(解く前。`sketch` は解いた位置を写したもの)。
    pub(crate) fn moved(&self, sketch: &Sketch, w: [f64; 2]) -> Sketch {
        let mut s = sketch.clone();
        let d = [w[0] - self.origin[0], w[1] - self.origin[1]];
        match &self.motion {
            Motion::Translate(pts) => {
                for (id, p) in pts {
                    s.set_point(*id, p[0] + d[0], p[1] + d[1]);
                }
            }
            Motion::CircleRadius { circle, center } => {
                let r = (w[0] - center[0]).hypot(w[1] - center[1]).max(1e-3);
                for e in &mut s.entities {
                    if let Entity::Circle { id, radius, .. } = e
                        && id == circle
                    {
                        *radius = r;
                    }
                }
            }
            Motion::ArcRadius { center, ends } => {
                let r = (w[0] - center[0]).hypot(w[1] - center[1]).max(1e-3);
                for (id, p) in ends {
                    let (dx, dy) = (p[0] - center[0], p[1] - center[1]);
                    let l = dx.hypot(dy).max(1e-9);
                    s.set_point(*id, center[0] + dx / l * r, center[1] + dy / l * r);
                }
            }
        }
        s
    }

    /// 押した所からの距離(mm)。
    pub(crate) fn distance(&self, w: [f64; 2]) -> f64 {
        (w[0] - self.origin[0]).hypot(w[1] - self.origin[1])
    }
}

/// 2 つの解で、点も半径も動いていないか。
pub(crate) fn unchanged(a: &Solution, b: &Solution) -> bool {
    let near = |p: &[f64; 2], q: &[f64; 2]| (p[0] - q[0]).hypot(p[1] - q[1]) < 1e-6;
    a.points
        .iter()
        .all(|(id, p)| b.points.get(id).is_none_or(|q| near(p, q)))
        && a.radii
            .iter()
            .all(|(id, r)| b.radii.get(id).is_none_or(|s| (r - s).abs() < 1e-6))
}

/// 囲んだ範囲(mm)。平らな図では軸に沿った四角、3D の視点では画面の四角を平面へ写した四角形。
#[derive(Clone, Copy)]
pub(crate) struct Window {
    min: [f64; 2],
    max: [f64; 2],
    /// 3D の視点で囲んだ時の 4 隅(画面の四角の隅を平面へ写した順)
    quad: Option<[[f64; 2]; 4]>,
}

impl Window {
    pub(crate) fn new(a: [f64; 2], b: [f64; 2]) -> Self {
        Window {
            min: [a[0].min(b[0]), a[1].min(b[1])],
            max: [a[0].max(b[0]), a[1].max(b[1])],
            quad: None,
        }
    }
    /// 4 隅で囲んだ範囲(3D の視点。N1b)。
    pub(crate) fn quad(c: [[f64; 2]; 4]) -> Self {
        let mut w = Window::new(c[0], c[2]);
        for p in c {
            for (i, v) in p.into_iter().enumerate() {
                w.min[i] = w.min[i].min(v);
                w.max[i] = w.max[i].max(v);
            }
        }
        w.quad = Some(c);
        w
    }
    fn inside(&self, p: [f64; 2]) -> bool {
        match &self.quad {
            Some(q) => crate::display::inside(q, p),
            None => {
                (self.min[0]..=self.max[0]).contains(&p[0])
                    && (self.min[1]..=self.max[1]).contains(&p[1])
            }
        }
    }
    fn corners(&self) -> [[f64; 2]; 4] {
        if let Some(q) = self.quad {
            return q;
        }
        let (a, b) = (self.min, self.max);
        [a, [b[0], a[1]], b, [a[0], b[1]]]
    }
    /// 線分が範囲に触れるか
    fn touches(&self, p: [f64; 2], q: [f64; 2]) -> bool {
        if self.inside(p) || self.inside(q) {
            return true;
        }
        let c = self.corners();
        (0..4).any(|i| segments_cross(p, q, c[i], c[(i + 1) % 4]))
    }
    /// 範囲の中で点 `c` にいちばん近い所までの距離(中なら 0)
    fn nearest(&self, c: [f64; 2]) -> f64 {
        if self.quad.is_none() {
            let near = [
                c[0].clamp(self.min[0], self.max[0]),
                c[1].clamp(self.min[1], self.max[1]),
            ];
            return (near[0] - c[0]).hypot(near[1] - c[1]);
        }
        if self.inside(c) {
            return 0.0;
        }
        let k = self.corners();
        (0..4)
            .map(|i| {
                let (a, b) = (k[i], k[(i + 1) % 4]);
                let ab = [b[0] - a[0], b[1] - a[1]];
                let l2 = (ab[0] * ab[0] + ab[1] * ab[1]).max(1e-18);
                let s = (((c[0] - a[0]) * ab[0] + (c[1] - a[1]) * ab[1]) / l2).clamp(0.0, 1.0);
                (a[0] + ab[0] * s - c[0]).hypot(a[1] + ab[1] * s - c[1])
            })
            .fold(f64::INFINITY, f64::min)
    }
}

fn segments_cross(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    let cross = |o: [f64; 2], p: [f64; 2], q: [f64; 2]| {
        (p[0] - o[0]) * (q[1] - o[1]) - (p[1] - o[1]) * (q[0] - o[0])
    };
    let (d1, d2) = (cross(c, d, a), cross(c, d, b));
    let (d3, d4) = (cross(a, b, c), cross(a, b, d));
    (d1 * d2 <= 0.0) && (d3 * d4 <= 0.0)
}

/// 囲んで選ぶ要素。`crossing` が偽なら範囲に全部入ったもの(左から右へ囲んだ時)、
/// 真なら範囲に触れたもの(右から左へ囲んだ時)。
pub(crate) fn window_pick(
    sketch: &Sketch,
    sol: &Solution,
    win: Window,
    crossing: bool,
) -> BTreeSet<EntityId> {
    let pos = |p: &EntityId| sol.points.get(p).copied();
    let poly = |pts: &[[f64; 2]]| {
        if crossing {
            pts.windows(2).any(|s| win.touches(s[0], s[1]))
        } else {
            pts.iter().all(|p| win.inside(*p))
        }
    };
    let mut out = BTreeSet::new();
    for e in &sketch.entities {
        let hit = match e {
            Entity::Point { id, .. } => pos(id).is_some_and(|p| win.inside(p)),
            Entity::Line { a, b, .. } => match (pos(a), pos(b)) {
                (Some(p), Some(q)) => poly(&[p, q]),
                _ => false,
            },
            Entity::Circle { id, center, .. } => match (pos(center), sol.radii.get(id)) {
                (Some(c), Some(&r)) => {
                    if crossing {
                        // 円周が範囲に触れる: 範囲の最も近い所は円の中、最も遠い隅は円の外
                        let far = win
                            .corners()
                            .iter()
                            .map(|k| (k[0] - c[0]).hypot(k[1] - c[1]))
                            .fold(0.0, f64::max);
                        win.nearest(c) <= r && far >= r
                    } else if win.quad.is_some() {
                        // 四角形は凸なので、円に外接する八角形の角が全部入れば円も入る
                        (0..8).all(|k| {
                            let a = std::f64::consts::TAU * k as f64 / 8.0;
                            let rr = r / (std::f64::consts::PI / 8.0).cos();
                            win.inside([c[0] + rr * a.cos(), c[1] + rr * a.sin()])
                        })
                    } else {
                        win.inside([c[0] - r, c[1] - r]) && win.inside([c[0] + r, c[1] + r])
                    }
                }
                _ => false,
            },
            Entity::Arc {
                center, start, end, ..
            } => match (pos(center), pos(start), pos(end)) {
                (Some(c), Some(s), Some(t)) => poly(&crate::view::arc_points(c, s, t)),
                _ => false,
            },
            // 楕円・スプライン・円錐曲線は折れ線にして調べる
            _ => sketch
                .curve_polyline(sol, e.id(), kmb_sketch::ARC_TOLERANCE * 10.0)
                .is_some_and(|pts| poly(&pts)),
        };
        if hit {
            out.insert(e.id());
        }
    }
    out
}

/// 右から左へ囲んだ時に一緒に選ぶ閉じた領域の名前(範囲に触れるもの)。
pub(crate) fn window_regions(regions: Vec<crate::annot::Area>, win: Window) -> Vec<String> {
    regions
        .into_iter()
        .filter(|r| {
            let loops = &r.loops;
            let edge = loops
                .iter()
                .any(|l| (0..l.len()).any(|i| win.touches(l[i], l[(i + 1) % l.len()])));
            edge || win
                .corners()
                .iter()
                .any(|k| crate::annot::contains(loops, *k))
        })
        .map(|r| r.key)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// 寸法と拘束で完全に決まった点は、掴んだ時点で「動かせません」にして、
    /// 引くたびに解き直さない。決まっていなければ動かせる。
    #[test]
    fn a_fully_determined_point_is_refused_when_grabbed() {
        let mut s = Sketch::default();
        let a = s.add_point(0.0, 0.0);
        let b = s.add_point(10.0, 0.0);
        let line = s.add_line(a, b);
        s.add_constraint(ConstraintKind::Fix { point: a });
        s.add_constraint(ConstraintKind::Horizontal { line });
        let free = kmb_sketch::solve(&s, &HashMap::new()).unwrap();
        let g = grab(&s, &free, &BTreeSet::new(), [10.0, 0.0], 0.5).expect("端の点");
        assert_eq!(g.blocked, None, "長さが決まっていなければ横へ動かせる");
        let len = s.add_constraint(ConstraintKind::Length {
            line,
            value: "10 mm".into(),
        });
        let sol = kmb_sketch::solve(&s, &HashMap::from([(len, 10.0)])).unwrap();
        assert!(
            sol.fixed.contains(&b),
            "長さも付けると決まる: {:?}",
            sol.fixed
        );
        let g = grab(&s, &sol, &BTreeSet::new(), [10.0, 0.0], 0.5).expect("端の点");
        assert_eq!(g.blocked, Some(STUCK));
        // 線を掴んでも、両端とも決まっているので同じ
        let g = grab(&s, &sol, &BTreeSet::new(), [5.0, 0.0], 0.5).expect("線");
        assert_eq!(g.blocked, Some(STUCK));
    }
}
