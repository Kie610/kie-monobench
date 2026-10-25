//! 2D スケッチのビューの、描いている途中の見本(線・円などの形、寸法の欄、角度の弧)と、
//! カーソルの下の吸い付く先の札。

use super::*;

/// 描いている途中の見本と、吸い付く先の札を描く。
#[allow(clippy::too_many_arguments)]
pub(super) fn previews(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    painter: &egui::Painter,
    l: &kmb_ui::theme::Look,
    t: &Transform,
    sketch: &Sketch,
    sol: &Solution,
    cursor: Option<[f64; 2]>,
    hover: &Option<Hover>,
    hud_on: bool,
    selected_color: Color32,
) {
    // 描画中の線・円の見本と、寸法の欄(固定した値に合わせた位置で描く)
    if let (Some(start), Some(cur)) = (editor.pending, cursor) {
        let stroke = Stroke::new(1.0, selected_color);
        let guide = Stroke::new(1.0, selected_color.gamma_multiply(0.6));
        let w = aimed(editor, cur);
        let s = t.to_screen(start.pos);
        let ws = t.to_screen(w);
        // 欄の札を置く位置と、そこに出す今の値(固定していない欄はカーソルで測った値)
        let mut spots: Vec<(Pos2, Option<f64>)> = Vec::new();
        let mid_out = |p: Pos2, q: Pos2, k: f32| {
            let d = q - p;
            let n = Vec2::new(-d.y, d.x).normalized();
            let n = if n.y > 0.0 { -n } else { n };
            p + d * 0.5 + n * k
        };
        let dist = |p: [f64; 2], q: [f64; 2]| (q[0] - p[0]).hypot(q[1] - p[1]);
        match editor.tool {
            Tool::Line if editor.arc_drag => {
                // 引いている接線円弧の見本
                if let Some(b) = editor.line_base
                    && let Some(pts) = tangent_arc_points(start.pos, b + std::f64::consts::PI, cur)
                {
                    let pts: Vec<Pos2> = pts.iter().map(|p| t.to_screen(*p)).collect();
                    painter.add(egui::Shape::line(pts, stroke));
                }
            }
            Tool::Line => {
                painter.line_segment([s, ws], stroke);
                let len = dist(start.pos, w);
                let dir = (w[1] - start.pos[1]).atan2(w[0] - start.pos[0]);
                // 角度は X 軸の向きから(つなげて描く時は前の線分から。網羅版 C2): 基準の点線と、
                // 回った分の弧
                let base = editor.line_base.unwrap_or(0.0);
                let mut turn = (dir - base).rem_euclid(std::f64::consts::TAU);
                let deg = if editor.line_base.is_some() {
                    // 前の線分との角(0〜180°)。回る向きはカーソルの側
                    if turn > std::f64::consts::PI {
                        turn -= std::f64::consts::TAU;
                    }
                    turn.abs().to_degrees()
                } else {
                    turn.to_degrees()
                };
                spots.push((mid_out(s, ws, 18.0), Some(len)));
                let rr = 34.0;
                let bv = Vec2::new(base.cos() as f32, -base.sin() as f32);
                painter.add(egui::Shape::dashed_line(
                    &[s, s + bv * (rr + 18.0)],
                    guide,
                    4.0,
                    3.0,
                ));
                let steps = 24;
                let pts: Vec<Pos2> = (0..=steps)
                    .map(|k| {
                        let a = (base + turn * k as f64 / steps as f64) as f32;
                        s + Vec2::new(a.cos(), -a.sin()) * rr
                    })
                    .collect();
                painter.add(egui::Shape::line(pts, guide));
                let half = (base + turn / 2.0) as f32;
                spots.push((
                    s + Vec2::new(half.cos(), -half.sin()) * (rr + 26.0),
                    Some(deg),
                ));
            }
            Tool::Circle => {
                t.circle(painter, start.pos, dist(start.pos, w), stroke);
                painter.add(egui::Shape::dashed_line(&[s, ws], guide, 4.0, 3.0));
                spots.push((mid_out(s, ws, 16.0), Some(2.0 * dist(start.pos, w))));
            }
            Tool::TwoPointCircle => {
                // 円は more_tools の見本が描く。打つ欄だけを直径の脇に出す
                painter.add(egui::Shape::dashed_line(&[s, ws], guide, 4.0, 3.0));
                spots.push((mid_out(s, ws, 16.0), Some(dist(start.pos, w))));
            }
            Tool::Rectangle | Tool::CenterRectangle | Tool::RoundedRectangle => {
                let (p, q) = if editor.tool != Tool::CenterRectangle {
                    (start.pos, w)
                } else {
                    ([2.0 * start.pos[0] - w[0], 2.0 * start.pos[1] - w[1]], w)
                };
                let r = (editor.tool == Tool::RoundedRectangle)
                    .then(|| rounded_radius(editor, p, q))
                    .filter(|r| {
                        *r > 0.0 && *r < (q[0] - p[0]).abs().min((q[1] - p[1]).abs()) / 2.0
                    });
                let pts: Vec<Pos2> = match r {
                    Some(r) => rounded_outline(p, q, r),
                    None => vec![p, [q[0], p[1]], q, [p[0], q[1]], p],
                }
                .into_iter()
                .map(|c| t.to_screen(c))
                .collect();
                painter.add(egui::Shape::line(pts, stroke));
                let (lo, hi) = (
                    t.to_screen([p[0].min(q[0]), p[1].min(q[1])]),
                    t.to_screen([p[0].max(q[0]), p[1].max(q[1])]),
                );
                // 幅は下の辺の下、高さは右の辺の右
                spots.push((
                    Pos2::new((lo.x + hi.x) / 2.0, lo.y + 18.0),
                    Some((q[0] - p[0]).abs()),
                ));
                spots.push((
                    Pos2::new(hi.x + 30.0, (lo.y + hi.y) / 2.0),
                    Some((q[1] - p[1]).abs()),
                ));
                // 角の半径は 1 つ目の隅の外(カーソルから遠い側)
                if editor.tool == Tool::RoundedRectangle {
                    let out = (t.to_screen(p) - t.to_screen(q)).normalized() * 36.0;
                    spots.push((t.to_screen(p) + out, Some(rounded_radius(editor, p, q))));
                }
            }
            Tool::Slot => {
                let b = editor.pending2.map_or(w, |p| p.pos);
                painter.add(egui::Shape::dashed_line(
                    &[s, t.to_screen(b)],
                    guide,
                    4.0,
                    3.0,
                ));
                match editor.pending2 {
                    None => {
                        painter.line_segment([s, ws], stroke);
                        spots.push((mid_out(s, ws, 18.0), Some(dist(start.pos, w))));
                    }
                    Some(_) => {
                        let h = slot_half(start.pos, b, w);
                        let d = [b[0] - start.pos[0], b[1] - start.pos[1]];
                        let l = d[0].hypot(d[1]).max(1e-9);
                        let n = [-d[1] / l * h, d[0] / l * h];
                        for k in [1.0, -1.0] {
                            let p = [start.pos[0] + k * n[0], start.pos[1] + k * n[1]];
                            let q = [b[0] + k * n[0], b[1] + k * n[1]];
                            painter.line_segment([t.to_screen(p), t.to_screen(q)], stroke);
                        }
                        for (c, o) in [(start.pos, b), (b, start.pos)] {
                            let cap = slot_cap(c, n, [c[0] - o[0], c[1] - o[1]]);
                            let v: Vec<Pos2> = cap.iter().map(|p| t.to_screen(*p)).collect();
                            painter.add(egui::Shape::line(v, stroke));
                        }
                        let mid = [(start.pos[0] + b[0]) / 2.0, (start.pos[1] + b[1]) / 2.0];
                        let side = [mid[0] + n[0], mid[1] + n[1]];
                        spots.push((
                            mid_out(t.to_screen(mid), t.to_screen(side), 0.0)
                                + Vec2::new(26.0, 0.0),
                            Some(2.0 * h),
                        ));
                    }
                }
            }
            Tool::Polygon => {
                let n = polygon_sides(editor);
                let c = start.pos;
                let r = dist(c, w);
                let a0 = (w[1] - c[1]).atan2(w[0] - c[0]);
                let pts: Vec<Pos2> = (0..=n)
                    .map(|k| {
                        let a = a0 + std::f64::consts::TAU * k as f64 / n as f64;
                        t.to_screen([c[0] + r * a.cos(), c[1] + r * a.sin()])
                    })
                    .collect();
                painter.add(egui::Shape::line(pts, stroke));
                t.circle(painter, c, r, guide);
                painter.add(egui::Shape::dashed_line(&[s, ws], guide, 4.0, 3.0));
                spots.push((s + Vec2::new(0.0, 26.0), Some(n as f64)));
                spots.push((mid_out(s, ws, 16.0), Some(r)));
            }
            Tool::Arc => match editor.pending2 {
                None => {
                    painter.add(egui::Shape::dashed_line(&[s, ws], guide, 4.0, 3.0));
                    spots.push((mid_out(s, ws, 16.0), Some(dist(start.pos, w))));
                }
                Some(st) => {
                    let r = dist(start.pos, st.pos);
                    let a = (w[1] - start.pos[1]).atan2(w[0] - start.pos[0]);
                    let e = [start.pos[0] + r * a.cos(), start.pos[1] + r * a.sin()];
                    let ccw = arc_ccw(editor);
                    let arc = if ccw {
                        arc_points(start.pos, st.pos, e)
                    } else {
                        arc_points(start.pos, e, st.pos)
                    };
                    // 回った角度と、弧の真ん中(札を置く)
                    let a0 = (st.pos[1] - start.pos[1]).atan2(st.pos[0] - start.pos[0]);
                    let mut sweep = if ccw { a - a0 } else { a0 - a };
                    sweep = sweep.rem_euclid(std::f64::consts::TAU);
                    let am = if ccw {
                        a0 + sweep / 2.0
                    } else {
                        a0 - sweep / 2.0
                    };
                    let pts = arc.into_iter().map(|p| t.to_screen(p)).collect();
                    painter.add(egui::Shape::line(pts, stroke));
                    for q in [st.pos, e] {
                        painter.add(egui::Shape::dashed_line(
                            &[s, t.to_screen(q)],
                            guide,
                            4.0,
                            3.0,
                        ));
                    }
                    let m = [start.pos[0] + r * am.cos(), start.pos[1] + r * am.sin()];
                    let dir = (t.to_screen(m) - s).normalized();
                    spots.push((t.to_screen(m) + dir * 22.0, Some(sweep.to_degrees())));
                }
            },
            Tool::TangentArc => {
                // 端の向きに接して続く円弧の見本
                if let Some(at) = start.point
                    && let Some(curve) = curve_ending_at(sketch, at)
                    && let Some(pts) = sketch.tangent_arc_preview(sol, curve, at, cur)
                {
                    let pts: Vec<Pos2> = pts.iter().map(|p| t.to_screen(*p)).collect();
                    painter.add(egui::Shape::line(pts, stroke));
                }
            }
            Tool::Offset => {
                if let Some((at, live)) =
                    crate::offset_tool::preview(painter, t, editor, sketch, sol, cur, stroke)
                {
                    spots.push((at + Vec2::new(0.0, -16.0), Some(live)));
                }
            }
            _ => {}
        }
        // 円・円弧・多角形・中心指定の長方形は、決めた中心を印で見せ続ける
        if matches!(
            editor.tool,
            Tool::Circle | Tool::Arc | Tool::Polygon | Tool::CenterRectangle
        ) {
            center_mark(painter, s, selected_color, l.pal.panel);
        }
        if hud_on {
            let mut last: Option<Rect> = None;
            for (k, (f, (at, live))) in editor.hud.fields.iter().zip(spots).enumerate() {
                last = Some(crate::hud::draw_field(
                    painter,
                    at,
                    f,
                    live,
                    k == editor.hud.active,
                    l,
                    selected_color,
                ));
            }
            // 値を打ったら、欄の脇に確定の ✓ を出す(Enter と同じ)
            if let Some(r) = last.filter(|_| editor.hud.pinned()) {
                let pressed = egui::Area::new(egui::Id::new("sketch_hud_ok"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(r.right_top() + Vec2::new(6.0, -1.0))
                    .show(ui.ctx(), |ui| {
                        ui.add(
                            egui::Button::new(egui::RichText::new("✓").color(l.pal.paper))
                                .fill(selected_color)
                                .corner_radius(kmb_ui::theme::corner(10.0))
                                .min_size(Vec2::splat(20.0)),
                        )
                        .on_hover_text("この値で作る(Enter)")
                        .clicked()
                    })
                    .inner;
                if pressed {
                    editor.confirm = true;
                    ui.ctx().request_repaint();
                }
            }
        }
    }
    if let Some(w) = cursor
        && crate::more_tools::handles(editor.tool)
    {
        let stroke = Stroke::new(1.0, selected_color);
        crate::more_tools::preview(painter, t, editor, sketch, sol, w, stroke);
    }
    if let Some(h) = &hover {
        let stroke = Stroke::new(1.5, selected_color);
        let at = match h.shape {
            HoverShape::Point(p) => {
                // 選ぶ道具では、決まった点の上に印を出さない(線を選ぶ邪魔になる)
                if h.kind.is_some() || !matches!(editor.tool, Tool::Select | Tool::Dimension) {
                    painter.circle_stroke(t.to_screen(p), 7.0, stroke);
                }
                p
            }
            HoverShape::Line(a, b) => {
                if h.kind.is_some() {
                    painter.line_segment(
                        [t.to_screen(a), t.to_screen(b)],
                        Stroke::new(3.0, selected_color),
                    );
                }
                [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]
            }
            HoverShape::Circle(c, r) => {
                if h.kind.is_some() {
                    t.circle(painter, c, r, Stroke::new(3.0, selected_color));
                }
                [c[0] + r, c[1]]
            }
        };
        // 何に吸い付くか(原点・中点・角・辺・中心)を札で出す
        if let Some(k) = h.kind {
            painter.text(
                t.to_screen(at) + Vec2::new(10.0, -10.0),
                egui::Align2::LEFT_BOTTOM,
                k.label(),
                kmb_ui::theme::prop(12.0),
                selected_color,
            );
        }
    }
}
