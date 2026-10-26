//! 図に描く印・格子・軸と、線の色。

use super::*;

/// 決めた中心の印(塗った丸と十字)。
pub(crate) fn center_mark(painter: &egui::Painter, p: Pos2, color: Color32, back: Color32) {
    let stroke = Stroke::new(1.2, color);
    painter.line_segment([p - Vec2::X * 9.0, p + Vec2::X * 9.0], stroke);
    painter.line_segment([p - Vec2::Y * 9.0, p + Vec2::Y * 9.0], stroke);
    painter.circle(p, 3.5, color, Stroke::new(1.5, back));
}

pub(crate) fn draw_axes(painter: &egui::Painter, rect: Rect, t: &Transform, color: Color32) {
    let stroke = Stroke::new(1.0, color);
    // 3D の視点: スケッチの平面の上の、見えている範囲の X・Y の線
    if t.space.is_some() {
        if let Some((lo, hi)) = space_extent(t, rect) {
            for (a, b) in [([lo[0], 0.0], [hi[0], 0.0]), ([0.0, lo[1]], [0.0, hi[1]])] {
                if let Some((a, b)) = t.segment(a, b) {
                    painter.line_segment([a, b], stroke);
                }
            }
        }
        return;
    }
    let o = t.to_screen([0.0, 0.0]);
    if rect.y_range().contains(o.y) {
        painter.line_segment(
            [Pos2::new(rect.left(), o.y), Pos2::new(rect.right(), o.y)],
            stroke,
        );
    }
    if rect.x_range().contains(o.x) {
        painter.line_segment(
            [Pos2::new(o.x, rect.top()), Pos2::new(o.x, rect.bottom())],
            stroke,
        );
    }
}

/// 欄に出す式(`12.5 mm`、`30 deg`)を、図の寸法と同じ桁数と単位で書く。数で始まらなければそのまま。
/// 長さは書いた単位(`0.5 in`)で読み、文書の単位で出す。
pub(crate) fn input_shown(
    fmt: &kmb_sketch::DimFormat,
    kind: &ConstraintKind,
    text: &str,
) -> String {
    if !kind.is_angle()
        && let Some(mm) = super::length_mm(text)
    {
        return fmt.length(mm);
    }
    let num: String = text
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E'))
        .collect();
    match num.parse::<f64>() {
        Ok(v) if kind.is_angle() => fmt.angle(v.to_radians()),
        Ok(v) => fmt.length(v),
        Err(_) => text.to_string(),
    }
}

/// 格子の線が尽きないか(拡大率が 0 や無限の時。ビューがまだ大きさを持たない時など)。
fn too_many_lines(lo: [f64; 2], hi: [f64; 2], step: f64, rect: Rect, t: &Transform) -> bool {
    let lines = (0..2).map(|a| (hi[a] - lo[a]) / step).fold(0.0, f64::max);
    let many = !lines.is_finite() || lines > 4000.0;
    if many {
        log::debug!(
            "格子を描かない(線の数 {lines}、ビュー {rect:?}、拡大率 {})",
            t.scale
        );
    }
    many
}

/// 方眼(1・5・10 の刻みから、画面で 12 px 以上になる最小の幅。5 目ごとに濃く)。
/// 格子。`base` があれば、その刻みの 1・2・5・10… 倍で、画面で 12 px 以上になる最小の間隔
/// (吸い付く刻みは `base` のまま。線の間隔だけ拡大率で変える)。
pub(crate) fn draw_grid(
    painter: &egui::Painter,
    rect: Rect,
    t: &Transform,
    color: Color32,
    base: Option<f64>,
) {
    let unit = base.filter(|b| *b > 0.0).unwrap_or(1.0);
    let step = [
        1.0f64, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0,
    ]
    .into_iter()
    .map(|k| k * unit)
    // 3D の視点では斜めに縮んで詰まって見えるので、間隔を広げる
    .find(|s| *s as f32 * t.scale >= if t.space.is_some() { 28.0 } else { 12.0 })
    .unwrap_or(1000.0 * unit);
    let faint = color.gamma_multiply(0.55);
    // 3D の視点: スケッチの平面の上に、見えている範囲だけ線分で引く
    if t.space.is_some() {
        let Some((lo, hi)) = space_extent(t, rect) else {
            return;
        };
        if too_many_lines(lo, hi, step, rect, t) {
            return;
        }
        for axis in 0..2 {
            let mut k = (lo[axis] / step).floor() as i64;
            while (k as f64) * step <= hi[axis] {
                let v = k as f64 * step;
                let c = if k % 5 == 0 {
                    color.gamma_multiply(0.7)
                } else {
                    faint.gamma_multiply(0.5)
                };
                let (a, b) = if axis == 0 {
                    ([v, lo[1]], [v, hi[1]])
                } else {
                    ([lo[0], v], [hi[0], v])
                };
                if let Some((a, b)) = t.segment(a, b) {
                    painter.line_segment([a, b], Stroke::new(1.0, c));
                }
                k += 1;
            }
        }
        return;
    }
    let lo = t.to_world(rect.left_bottom());
    let hi = t.to_world(rect.right_top());
    if too_many_lines(lo, hi, step, rect, t) {
        return;
    }
    for axis in 0..2 {
        let mut k = (lo[axis] / step).floor() as i64;
        while (k as f64) * step <= hi[axis] {
            let v = k as f64 * step;
            let c = if k % 5 == 0 { color } else { faint };
            let stroke = Stroke::new(1.0, c);
            if axis == 0 {
                let x = t.to_screen([v, 0.0]).x;
                painter.vline(x, rect.y_range(), stroke);
            } else {
                let y = t.to_screen([0.0, v]).y;
                painter.hline(rect.x_range(), y, stroke);
            }
            k += 1;
        }
    }
}

/// 固定した物の色。
pub(crate) fn fix_ink(l: &kmb_ui::theme::Look) -> Color32 {
    l.pal.ok
}

/// 拘束が指す要素(線と点。線の拘束は線の点も)。
pub(crate) fn members(sketch: &Sketch, ids: &[kmb_sketch::ConstraintId]) -> BTreeSet<EntityId> {
    let mut out = BTreeSet::new();
    for c in sketch.constraints.iter().filter(|c| ids.contains(&c.id)) {
        for r in kmb_sketch::refs(&c.kind) {
            out.insert(r);
        }
    }
    out
}

/// 固定した点と、使う点がみな固定した線(緑で描く)。
pub(crate) fn pinned(sketch: &Sketch) -> BTreeSet<EntityId> {
    let fixed: BTreeSet<EntityId> = sketch
        .constraints
        .iter()
        .filter_map(|c| match c.kind {
            ConstraintKind::Fix { point } => Some(point),
            _ => None,
        })
        .collect();
    let mut out = fixed.clone();
    for e in &sketch.entities {
        // 円は中心を固定しても半径が動くので除く
        let pts = e.points();
        if !matches!(e, Entity::Circle { .. })
            && !pts.is_empty()
            && pts.iter().all(|p| fixed.contains(p))
        {
            out.insert(e.id());
        }
    }
    out
}

/// 選んだ要素の色。未拘束の青・完全拘束の黒・写した形の紫・補助線の灰のどれとも見分けられる橙。
pub(crate) fn pick_ink(l: &kmb_ui::theme::Look) -> Color32 {
    l.pal.inks.sketch_pick
}

/// まだ動ける要素の色(試作の「動ける」の青)。
pub(crate) fn free_ink(l: &kmb_ui::theme::Look) -> Color32 {
    l.pal.inks.sketch_free
}

/// マウスの下にある要素の形(印を描く)。
#[derive(Clone, Copy)]
pub(crate) enum HoverShape {
    Point([f64; 2]),
    Line([f64; 2], [f64; 2]),
    Circle([f64; 2], f64),
}

pub(crate) struct Hover {
    pub(crate) shape: HoverShape,
    /// 仮の吸い付く先なら、その種類
    pub(crate) kind: Option<crate::snap::Kind>,
}

pub(crate) fn hover_shape(
    sketch: &Sketch,
    sol: &Solution,
    id: EntityId,
    kind: Option<crate::snap::Kind>,
) -> Hover {
    let pt = |p: &EntityId| sol.points.get(p).copied().unwrap_or([0.0, 0.0]);
    let shape = match sketch.entity(id) {
        Some(Entity::Line { a, b, .. }) => HoverShape::Line(pt(a), pt(b)),
        Some(Entity::Circle { center, .. }) => {
            HoverShape::Circle(pt(center), sol.radii.get(&id).copied().unwrap_or(0.0))
        }
        Some(Entity::Arc { center, start, .. }) => {
            let (c, s) = (pt(center), pt(start));
            HoverShape::Circle(c, (s[0] - c[0]).hypot(s[1] - c[1]))
        }
        _ => HoverShape::Point(pt(&id)),
    };
    Hover { shape, kind }
}

/// 写した要素(既存の形の参照)の色。紫に寄せる。
pub(crate) fn ref_ink(l: &kmb_ui::theme::Look) -> Color32 {
    l.pal.inks.sketch_ref
}

/// カーソルの脇の小さな札(吸い付いた先や、推測した拘束の名前)。
pub(crate) fn badge(painter: &egui::Painter, at: Pos2, text: &str, ink: Color32, fill: Color32) {
    let g = painter.layout_no_wrap(text.to_string(), kmb_ui::theme::prop(11.5), ink);
    let r = Rect::from_min_size(at, g.size() + Vec2::new(10.0, 4.0));
    painter.rect(
        r,
        3.0,
        fill,
        Stroke::new(1.0, ink),
        egui::StrokeKind::Inside,
    );
    painter.galley(r.min + Vec2::new(5.0, 2.0), g, ink);
}

/// 3D の視点で、スケッチの平面の見えている範囲(平面の座標の最小と最大)。ビューの隅と辺の
/// 真ん中を通る視線が平面と交わる所を囲む。地平線の近くで遠くまで延びないよう、回す中心から
/// ビューの大きさの 2 倍までに留める。平面を真横から見ていれば `None`。
pub(crate) fn space_extent(t: &Transform, rect: Rect) -> Option<([f64; 2], [f64; 2])> {
    let s = t.space.as_ref()?;
    let (_, _, eye) = s.cam.basis_pub();
    let n = s.plane.normal();
    let facing: f64 = (0..3).map(|i| eye[i] as f64 * n[i]).sum();
    if facing.abs() < 0.02 {
        return None;
    }
    let c = t.to_world(rect.center() + s.cam.pan);
    let reach = 2.0 * (rect.width().max(rect.height()) / t.scale.max(1e-6)) as f64;
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for fx in [0.0, 0.5, 1.0] {
        for fy in [0.0, 0.5, 1.0] {
            let p = rect.min + egui::vec2(rect.width() * fx, rect.height() * fy);
            let w = t.to_world(p);
            for i in 0..2 {
                let v = w[i].clamp(c[i] - reach, c[i] + reach);
                lo[i] = lo[i].min(v);
                hi[i] = hi[i].max(v);
            }
        }
    }
    Some((lo, hi))
}
