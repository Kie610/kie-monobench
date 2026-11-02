//! 自由曲線の要素の形(曲線の計算そのものは kmb-sketch-solver の `curve`)。

use crate::geom::{add, dot, len, scale, sub};
use crate::model::{Entity, EntityId, Sketch};
use crate::solver::Solution;
pub(crate) use kmb_sketch_solver::curve::*;

/// 要素の形。点の位置は `pos`、円の半径は `radius` で引く。点や、形にならない要素は `None`。
pub(crate) fn entity_shape(
    e: &Entity,
    pos: &dyn Fn(EntityId) -> Option<P2>,
    radius: &dyn Fn(EntityId) -> Option<f64>,
) -> Option<Shape> {
    match e {
        Entity::Point { .. } => None,
        Entity::Line { a, b, .. } => Some(Shape::Line(pos(*a)?, pos(*b)?)),
        Entity::Circle { id, center, .. } => Shape::circle(pos(*center)?, radius(*id)?),
        Entity::Arc {
            center, start, end, ..
        } => Shape::arc(pos(*center)?, pos(*start)?, pos(*end)?),
        Entity::Ellipse {
            center,
            major,
            minor,
            ends,
            ..
        } => {
            let ends = match ends {
                Some((s, t)) => Some((pos(*s)?, pos(*t)?)),
                None => None,
            };
            Shape::ellipse(pos(*center)?, pos(*major)?, pos(*minor)?, ends)
        }
        Entity::Spline {
            points,
            control,
            closed,
            degree,
            knots,
            ..
        } => {
            let p: Option<Vec<P2>> = points.iter().map(|q| pos(*q)).collect();
            Shape::spline(&p?, *control, *closed, *degree, knots.as_deref())
        }
        Entity::Conic {
            start,
            end,
            apex,
            rho,
            ..
        } => Shape::conic(pos(*start)?, pos(*end)?, pos(*apex)?, *rho),
    }
}

/// 楕円・スプライン・円錐曲線か(線分・円・円弧でない曲線)。
pub(crate) fn is_free(e: &Entity) -> bool {
    matches!(
        e,
        Entity::Ellipse { .. } | Entity::Spline { .. } | Entity::Conic { .. }
    )
}

impl Sketch {
    /// 保存されている座標での形。
    pub(crate) fn shape(&self, id: EntityId) -> Option<Shape> {
        let e = self.entity(id)?;
        entity_shape(e, &|p| self.pos(p), &|c| match self.entity(c) {
            Some(Entity::Circle { radius, .. }) => Some(*radius),
            _ => None,
        })
    }

    /// 解いた位置での形。
    pub(crate) fn solved_shape(&self, sol: &Solution, id: EntityId) -> Option<Shape> {
        let e = self.entity(id)?;
        entity_shape(e, &|p| sol.points.get(&p).copied(), &|c| {
            sol.radii.get(&c).copied()
        })
    }

    /// 曲線 `id` を、解いた位置 `sol` で折れ線にする(画面に描く用。弦と曲線の隔たりが `tol`
    /// 以下。両端を含み、閉じた曲線は最後の点が最初の点と同じ)。曲線でなければ `None`。
    pub fn curve_polyline(&self, sol: &Solution, id: EntityId, tol: f64) -> Option<Vec<P2>> {
        let s = self.solved_shape(sol, id)?;
        Some(
            s.polyline(tol.max(1e-6))
                .into_iter()
                .map(|(_, p)| p)
                .collect(),
        )
    }

    /// 曲線 `id` の、点 `p` に最も近い所(点、その向きの単位ベクトル、距離)。解いた位置で測る。
    /// 吸着や、接する拘束の向きの予告に使う。曲線でなければ `None`。
    pub fn curve_closest(&self, sol: &Solution, id: EntityId, p: P2) -> Option<(P2, P2, f64)> {
        let s = self.solved_shape(sol, id)?;
        Some(closest_on_shape(&s, p))
    }

    /// 自由曲線と線分・円・円弧の交点。解いた形で求め、離れた外形の箱は調べない。
    /// 曲線どうしの交点は対象にしない。
    pub fn free_curve_crossings(&self, sol: &Solution) -> Vec<(P2, EntityId, EntityId)> {
        use crate::geom::Curve;
        let shapes: Vec<_> = self
            .entities
            .iter()
            .filter_map(|e| {
                let shape = self.solved_shape(sol, e.id())?;
                let bounds = shape_bounds(&shape)?;
                let curve = match shape {
                    Shape::Line(a, b) => Curve::Seg(a, b),
                    Shape::Arc {
                        c, r, a0, sweep, ..
                    } => Curve::Arc(c, r, a0, sweep),
                    other => Curve::Free(Box::new(other)),
                };
                Some((e.id(), is_free(e), bounds, curve))
            })
            .collect();
        let mut out = Vec::new();
        for (i, (a, free_a, ba, ca)) in shapes.iter().enumerate() {
            for (b, free_b, bb, cb) in &shapes[i + 1..] {
                if free_a == free_b
                    || (0..2).any(|k| ba[1][k] + 1e-8 < bb[0][k] || bb[1][k] + 1e-8 < ba[0][k])
                {
                    continue;
                }
                for p in crate::geom::intersect(ca, cb) {
                    out.push((p, *a, *b));
                }
            }
        }
        out
    }

    /// 曲線 `id` の端(`at_end` が偽なら始点、真なら終点)での、たどる向きの単位ベクトルと
    /// 符号つきの曲率(左へ曲がると正、1/mm)。閉じた曲線や点は `None`。
    pub fn curve_end(&self, sol: &Solution, id: EntityId, at_end: bool) -> Option<(P2, f64)> {
        let s = self.solved_shape(sol, id)?;
        if s.closed() {
            return None;
        }
        let (lo, hi) = s.domain();
        let t = if at_end { hi } else { lo };
        Some((s.tangent(t), s.curvature(t)))
    }
}

/// 曲線を囲む箱。スプラインと有理曲線は制御点の凸包に入り、楕円は軸の成分から囲む。
pub(crate) fn shape_bounds(shape: &Shape) -> Option<[P2; 2]> {
    let points = match shape {
        Shape::Line(a, b) => vec![*a, *b],
        Shape::Arc { c, r, .. } => vec![[c[0] - r, c[1] - r], [c[0] + r, c[1] + r]],
        Shape::Ellipse { c, u, v, .. } => {
            let extent = [u[0].hypot(v[0]), u[1].hypot(v[1])];
            vec![sub(*c, extent), add(*c, extent)]
        }
        Shape::Spline { s, .. } => s.ctrl.clone(),
        Shape::Conic { p, .. } => p.to_vec(),
    };
    let mut bounds = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];
    for p in points {
        if p.iter().any(|v| !v.is_finite()) {
            return None;
        }
        for (i, value) in p.into_iter().enumerate() {
            bounds[0][i] = bounds[0][i].min(value);
            bounds[1][i] = bounds[1][i].max(value);
        }
    }
    bounds
        .iter()
        .flatten()
        .all(|v| v.is_finite())
        .then_some(bounds)
}

/// 折れ線で得たパラメータを、曲線の式の上の距離で詰める。
/// 接線と点への向きが直角になる所を探し、距離が増える一歩は小さくする。
pub(crate) fn closest_on_shape(shape: &Shape, p: P2) -> (P2, P2, f64) {
    let (lo, hi) = shape.domain();
    let (mut t, _) = shape.closest(p);
    let distance = |t| len(sub(shape.at(t), p));
    let mut best = distance(t);
    for _ in 0..24 {
        let [at, first, second] = shape.ders(t);
        let toward = sub(at, p);
        let derivative = dot(first, first) + dot(toward, second);
        if derivative.abs() < 1e-300 {
            break;
        }
        let mut step = (-dot(toward, first) / derivative).clamp(-(hi - lo) / 4.0, (hi - lo) / 4.0);
        let mut next = t;
        for _ in 0..16 {
            let trial = if shape.closed() {
                lo + (t + step - lo).rem_euclid(hi - lo)
            } else {
                (t + step).clamp(lo, hi)
            };
            if distance(trial) <= best {
                next = trial;
                break;
            }
            step *= 0.5;
        }
        if next == t {
            break;
        }
        t = next;
        best = distance(t);
    }
    if !shape.closed() {
        for end in [lo, hi] {
            if distance(end) < best {
                t = end;
                best = distance(t);
            }
        }
    }
    (shape.at(t), shape.tangent(t), best)
}

/// 短軸の端の位置: 中心 `center` と長軸の端 `major` の楕円で、点 `p` を通る大きさの短軸
/// (長軸から `p` までの距離が短半径。`p` の側)。楕円の道具の 3 点目に使う。
pub fn ellipse_minor_point(center: P2, major: P2, p: P2) -> Option<P2> {
    let u = sub(major, center);
    let a = len(u);
    if a < 1e-9 {
        return None;
    }
    let u = scale(u, 1.0 / a);
    let n = [-u[1], u[0]];
    let b = dot(sub(p, center), n);
    (b.abs() > 1e-9).then(|| add(center, scale(n, b)))
}

/// 描いている途中の楕円の見本の折れ線(中心、長軸の端、短軸を決める点)。描けなければ空。
pub fn ellipse_preview(center: P2, major: P2, minor_point: P2, tol: f64) -> Vec<P2> {
    ellipse_minor_point(center, major, minor_point)
        .and_then(|m| Shape::ellipse(center, major, m, None))
        .map(|s| preview(&s, tol))
        .unwrap_or_default()
}

/// 描いている途中のスプラインの見本の折れ線(`control` なら制御点、そうでなければ通る点)。
pub fn spline_preview(points: &[P2], control: bool, closed: bool, tol: f64) -> Vec<P2> {
    spline_preview_degree(points, control, closed, 3, tol)
}

/// [`spline_preview`] の、制御点のスプラインの次数を選べるもの。
pub fn spline_preview_degree(
    points: &[P2],
    control: bool,
    closed: bool,
    degree: u8,
    tol: f64,
) -> Vec<P2> {
    Shape::spline(points, control, closed, degree, None)
        .map(|s| preview(&s, tol))
        .unwrap_or_default()
}

/// 描いている途中の円錐曲線の見本の折れ線。
pub fn conic_preview(start: P2, end: P2, apex: P2, rho: f64, tol: f64) -> Vec<P2> {
    Shape::conic(start, end, apex, rho)
        .map(|s| preview(&s, tol))
        .unwrap_or_default()
}

fn preview(s: &Shape, tol: f64) -> Vec<P2> {
    s.polyline(tol.max(1e-6))
        .into_iter()
        .map(|(_, p)| p)
        .collect()
}
