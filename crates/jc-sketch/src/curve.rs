//! 自由曲線の要素の形(曲線の計算そのものは jc-sketch-solver の `curve`)。

use crate::geom::{add, dot, len, scale, sub};
use crate::model::{Entity, EntityId, Sketch};
use crate::solver::Solution;
pub(crate) use jc_sketch_solver::curve::*;

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
        let (t, d) = s.closest(p);
        Some((s.at(t), s.tangent(t), d))
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
