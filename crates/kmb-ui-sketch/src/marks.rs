//! 図の上の拘束の印(水平・平行・一致など、値のない拘束)。自動で付いたものも含めて常に出し、
//! 隠れた拘束を作らない(試作 S2 の「拘束の印: 常に出す」)。印は要素ごとに並べ、
//! 指を置くと関わる要素を光らせ、クリックで選んで Delete で外せる。オフセット・ポリゴン・
//! パターンは、関わる要素がいくつあっても 1 つの印にする。

use crate::view::Transform;
use egui::{Color32, Pos2, Rect, Stroke, Vec2};
use kmb_sketch::{
    ConstraintId, ConstraintKind as K, Entity, EntityId, OffsetPart, PatternDef, Sketch, Solution,
};
use std::collections::BTreeMap;

/// 印 1 つの大きさ(px)。
const SIZE: f32 = 16.0;

/// 描いた印(次のコマのクリックとホバーに使う)。
#[derive(Clone, Debug)]
pub(crate) struct Mark {
    pub id: ConstraintId,
    pub rect: Rect,
}

/// 拘束の印のアイコン(リボンの拘束と同じ絵)。値のある拘束(寸法)は `None`。
pub(crate) fn icon(sketch: &Sketch, k: &K) -> Option<&'static str> {
    Some(match k {
        K::Offset { .. } => "skoffset",
        K::Polygon { .. } => "polycons",
        K::Pattern { pattern } => match sketch.pattern(*pattern).map(|p| &p.def) {
            Some(PatternDef::Rectangular(_)) => "linpat",
            _ => "circpat",
        },
        K::Horizontal { .. } | K::HorizontalPoints { .. } => "horiz",
        K::Vertical { .. } | K::VerticalPoints { .. } => "vert",
        K::Coincident { .. } => "coinc",
        K::Parallel { .. } => "parallel",
        K::Perpendicular { .. } => "perp",
        K::Equal { .. } => "equal",
        K::Fix { .. } => "fix",
        K::PointOnLine { .. } => "online",
        K::PointOnCircle { .. } => "oncircle",
        K::Midpoint { .. } => "midpoint",
        K::Tangent { .. } | K::TangentCircles { .. } => "tangent",
        K::Concentric { .. } => "concentric",
        K::Collinear { .. } => "collinear",
        K::Symmetric { .. } | K::SymmetricPairs { .. } => "symmetric",
        _ => return None,
    })
}

/// 拘束が関わる要素(印を並べる先は、点の拘束なら点、それ以外は線や円)。
pub(crate) fn members(sketch: &Sketch, k: &K) -> Vec<EntityId> {
    match k {
        K::Offset { parts } => {
            let mut v: Vec<EntityId> = parts.iter().flat_map(OffsetPart::made).collect();
            v.sort();
            v.dedup();
            v
        }
        K::Polygon { sides, .. } => sides.clone(),
        K::Pattern { pattern } => sketch
            .pattern(*pattern)
            .map(|p| {
                let mut v = p.def.entities().to_vec();
                v.extend(p.copies.iter().flat_map(|c| c.entities.iter().copied()));
                v.retain(|e| sketch.entity(*e).is_some_and(Entity::is_curve));
                v
            })
            .unwrap_or_default(),
        K::Horizontal { line } | K::Vertical { line } => vec![*line],
        K::Coincident { a, b } | K::HorizontalPoints { a, b } | K::VerticalPoints { a, b } => {
            vec![*a, *b]
        }
        K::Fix { point } => vec![*point],
        K::Parallel { a, b }
        | K::Perpendicular { a, b }
        | K::Equal { a, b }
        | K::TangentCircles { a, b }
        | K::Concentric { a, b }
        | K::Collinear { a, b } => vec![*a, *b],
        K::PointOnLine { point, line } | K::Midpoint { point, line } => vec![*point, *line],
        K::PointOnCircle { point, circle } => vec![*point, *circle],
        K::Tangent { line, circle } => vec![*line, *circle],
        K::Symmetric { a, b, line } => vec![*a, *b, *line],
        K::SymmetricPairs {
            a1,
            b1,
            a2,
            b2,
            line,
        } => vec![*a1, *b1, *a2, *b2, *line],
        _ => vec![],
    }
}

/// 印を並べる先(1 つの拘束に 1 つか 2 つ)。点に関わる拘束は点に 1 つ、2 本の線の拘束は両方に出す。
/// まとまりの拘束は代表の 1 つ(オフセットは写した最初の線、ポリゴンは最初の辺、パターンは元)。
fn hosts(sketch: &Sketch, k: &K) -> Vec<EntityId> {
    match k {
        K::Offset { parts } => parts
            .iter()
            .find_map(|p| match p {
                OffsetPart::Parallel { to, .. }
                | OffsetPart::Distance { to, .. }
                | OffsetPart::Radius { to, .. } => Some(*to),
                _ => None,
            })
            .or_else(|| parts.first().and_then(|p| p.made().first().copied()))
            .into_iter()
            .collect(),
        K::Polygon { sides, .. } => sides.first().copied().into_iter().collect(),
        K::Pattern { .. } => members(sketch, k).into_iter().take(1).collect(),
        K::Coincident { a, .. } => vec![*a],
        K::Fix { point }
        | K::PointOnLine { point, .. }
        | K::PointOnCircle { point, .. }
        | K::Midpoint { point, .. } => vec![*point],
        K::Symmetric { line, .. } | K::SymmetricPairs { line, .. } => vec![*line],
        _ => members(sketch, k),
    }
}

/// 要素の脇の、印を並べ始める位置と並べる向き(画面)。
fn anchor(sketch: &Sketch, sol: &Solution, t: &Transform, id: EntityId) -> Option<(Pos2, Vec2)> {
    let pt = |p: &EntityId| sol.points.get(p).map(|q| t.to_screen(*q));
    match sketch.entity(id)? {
        Entity::Point { .. } => Some((pt(&id)? + Vec2::new(12.0, -12.0), Vec2::X)),
        Entity::Line { a, b, .. } => {
            let (p, q) = (pt(a)?, pt(b)?);
            let d = (q - p).normalized();
            let d = if d.x < -1e-3 || (d.x.abs() <= 1e-3 && d.y > 0.0) {
                -d
            } else {
                d
            };
            // 線の上側(画面で上、縦の線は右)へ離す
            let n = Vec2::new(d.y, -d.x);
            let n = if n.y > 0.0 || (n.y.abs() < 1e-3 && n.x < 0.0) {
                -n
            } else {
                n
            };
            Some((p + (q - p) * 0.5 + n * 13.0, d))
        }
        Entity::Circle { center, .. } => {
            let c = pt(center)?;
            let r = *sol.radii.get(&id)? as f32 * t.scale;
            let a = std::f32::consts::FRAC_PI_4;
            Some((c + Vec2::new(a.cos(), -a.sin()) * (r + 12.0), Vec2::X))
        }
        Entity::Arc {
            center, start, end, ..
        } => {
            let (c, s, e) = (
                sol.points.get(center)?,
                sol.points.get(start)?,
                sol.points.get(end)?,
            );
            let pts = crate::view::arc_points(*c, *s, *e);
            let m = t.to_screen(pts[pts.len() / 2]);
            let cs = t.to_screen(*c);
            let out = (m - cs).normalized();
            Some((m + out * 13.0, Vec2::new(-out.y, out.x)))
        }
        // 楕円・スプライン・円錐曲線: 折れ線の真ん中の脇
        Entity::Ellipse { .. } | Entity::Spline { .. } | Entity::Conic { .. } => {
            let pts = sketch.curve_polyline(sol, id, kmb_sketch::ARC_TOLERANCE * 10.0)?;
            let i = pts.len() / 2;
            let (a, b) = (
                t.to_screen(pts[i.saturating_sub(1)]),
                t.to_screen(pts[(i + 1).min(pts.len() - 1)]),
            );
            let d = (b - a).normalized();
            let n = Vec2::new(d.y, -d.x);
            Some((t.to_screen(pts[i]) + n * 13.0, d))
        }
    }
}

/// 拘束の印を描き、描いた場所を返す。`hot` は光らせる印(指を置いた・選んだ)。
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw(
    painter: &egui::Painter,
    t: &Transform,
    sketch: &Sketch,
    sol: &Solution,
    hover: Option<ConstraintId>,
    picked: Option<ConstraintId>,
    ink: Color32,
    back: Color32,
    pick: Color32,
    avoid: &[Rect],
) -> Vec<Mark> {
    // 要素ごとに印を集め、脇に一列に並べる(同じ線の水平と平行などが重ならない)
    let mut per: BTreeMap<EntityId, Vec<(ConstraintId, &'static str)>> = BTreeMap::new();
    for c in &sketch.constraints {
        let Some(name) = icon(sketch, &c.kind) else {
            continue;
        };
        for h in hosts(sketch, &c.kind) {
            per.entry(h).or_default().push((c.id, name));
        }
    }
    let mut out = Vec::new();
    for (host, list) in per {
        let Some((start, dir)) = anchor(sketch, sol, t, host) else {
            continue;
        };
        let n = list.len() as f32;
        // 並びの真ん中を始まりの位置に合わせる。寸法の札や先に置いた印と重なるなら、並びの向きへずらす
        let row = |shift: f32| -> Vec<Rect> {
            (0..list.len())
                .map(|k| {
                    let at = start + dir * ((k as f32 - (n - 1.0) / 2.0 + shift) * (SIZE + 2.0));
                    Rect::from_center_size(at, Vec2::splat(SIZE))
                })
                .collect()
        };
        let clear = |rs: &[Rect]| {
            rs.iter().all(|r| {
                let r = r.expand(1.0);
                !avoid.iter().any(|a| a.intersects(r))
                    && !out.iter().any(|m: &Mark| m.rect.intersects(r))
            })
        };
        let rects = [0.0, 1.0, -1.0, 2.0, -2.0, 3.0, -3.0, 4.0, -4.0]
            .iter()
            .map(|s| row(*s * (n.max(1.0) * 0.5 + 0.5)))
            .find(|rs| clear(rs))
            .unwrap_or_else(|| row(0.0));
        for ((id, name), rect) in list.into_iter().zip(rects) {
            let hot = hover == Some(id) || picked == Some(id);
            let (fill, edge, color) = if picked == Some(id) {
                (pick.gamma_multiply(0.18), pick, pick)
            } else if hot {
                (back, pick, pick)
            } else {
                (back, ink.gamma_multiply(0.55), ink)
            };
            painter.rect(
                rect,
                3.0,
                fill,
                Stroke::new(1.0, edge),
                egui::StrokeKind::Inside,
            );
            kmb_ui::theme::paint_icon(painter, rect.shrink(1.5), name, color);
            out.push(Mark { id, rect });
        }
    }
    out
}

/// 画面の点 `p` の下の印(後に描いたものから)。
pub(crate) fn at(marks: &[Mark], p: Pos2) -> Option<ConstraintId> {
    marks
        .iter()
        .rev()
        .find(|m| m.rect.contains(p))
        .map(|m| m.id)
}
