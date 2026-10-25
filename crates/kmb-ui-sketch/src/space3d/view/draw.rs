//! 3D スケッチのビューの描画(作図面・面・線・点・印・寸法・描きかけ)。

use super::*;

/// 作図面・面・線・点・印・寸法・描きかけを描く。描いた寸法の値の札の場所を返す。
#[allow(clippy::too_many_arguments)]
pub(super) fn draw(
    ed: &Sketch3dEditor,
    ui: &egui::Ui,
    sk: &Sketch3d,
    sol: &Solution3d,
    marks: &[Label],
    dim_shapes: &[Dim],
    hover: Option<&Hit>,
    hover_pos: Option<Pos2>,
    look: &kmb_ui::theme::Look,
    entry: &FeatureEntry,
) -> Vec<(ConstraintId, Rect)> {
    let rect = ed.rect;
    // ボディから取り込んだ物と求めた線の色
    let linked_ink = look.pal.inks.sketch_linked;
    let axis_ink = look.pal.inks.axis;
    let pr = ed.proj();
    let painter = ui.painter_at(rect);
    let pal = &look.pal;
    // 図の中の色は作業空間の色のまま(画面の差し色はスケッチ中に水色になる)
    let accent = look.pal.ws(look.space);
    let free_ink = view3d::sketch_ink(pal.dark);
    let fixed_ink = pal.ink;
    // 作図面: 描いている平面の四角と目盛り
    let half = plane_half(&ed.cam, rect);
    let dp = ed.draw_plane(sk, sol);
    let center = dp.origin;
    let quad_of = |f: &DrawPlane, c: P3, h: f64| -> Vec<Pos2> {
        [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .iter()
            .map(|(a, b)| pr.at(f.at(c, a * h, b * h)))
            .collect()
    };
    let quad = quad_of(&dp, center, half);
    let plane_col = kmb_ui::theme::mix(pal.canvas, accent, 0.5);
    painter.add(egui::Shape::convex_polygon(
        quad.clone(),
        plane_col.gamma_multiply(0.08),
        Stroke::new(1.0, plane_col.gamma_multiply(0.6)),
    ));
    let step = view3d::grid_step(ed.cam.zoom, 22.0) as f64;
    let lines = (half / step).floor() as i64;
    if lines <= 60 {
        for k in -lines..=lines {
            for (axis, other) in [(dp.u, dp.v), (dp.v, dp.u)] {
                // 座標の平面では座標のきりのよい所に、作図面では作図面の中心から刻む
                let c = geom::dot(center, axis);
                let base = (c / step).round() * step + k as f64 * step;
                let off = base - c;
                if off.abs() > half {
                    continue;
                }
                let a = geom::add(
                    center,
                    geom::add(geom::scale(axis, off), geom::scale(other, -half)),
                );
                let b = geom::add(a, geom::scale(other, 2.0 * half));
                if let Some((a, b)) = pr.seg(a, b) {
                    painter.line_segment([a, b], Stroke::new(0.5, plane_col.gamma_multiply(0.3)));
                }
            }
        }
    }
    let title = match dp.work.and_then(|id| sk.work_plane(id)) {
        Some(w) => format!("{}({})", w.name, w.def.label()),
        None => {
            let n = ed.plane.normal();
            format!(
                "作図面 {}({} = {:.2} mm)",
                ed.plane.label(),
                n.label().to_lowercase(),
                center[n.index()]
            )
        }
    };
    painter.text(
        quad[2] + (quad[0] - quad[2]) * 0.03,
        egui::Align2::LEFT_BOTTOM,
        title,
        kmb_ui::theme::prop(11.0),
        plane_col,
    );
    // ほかの作図面(枠だけ)と、作図面の名前の札
    let other_col = kmb_ui::theme::mix(pal.canvas, pal.ink2, 0.6);
    for w in &sk.planes {
        if dp.work == Some(w.id) {
            continue;
        }
        let Some(f) = work_frame(sk, sol, w.id) else {
            continue;
        };
        let sel = ed.selection.contains(&Sel::Plane(w.id));
        let col = if sel { accent } else { other_col };
        let q = quad_of(&f, f.origin, 0.6 * half);
        painter.add(egui::Shape::convex_polygon(
            q,
            col.gamma_multiply(if sel { 0.12 } else { 0.04 }),
            Stroke::new(1.0, col.gamma_multiply(0.8)),
        ));
    }
    for (id, at, name) in plane_tags(ed, sk, sol) {
        let r = tag_rect(ui, at, &name);
        let on = ed.selection.contains(&Sel::Plane(id)) || hover == Some(&Hit::Plane(id));
        let (fg, bg) = if on {
            (Color32::WHITE, accent)
        } else {
            (pal.ink, pal.paper.gamma_multiply(0.85))
        };
        painter.rect_filled(r, kmb_ui::theme::corner(3.0), bg);
        painter.text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            &name,
            kmb_ui::theme::prop(11.0),
            fg,
        );
    }
    // 原点からの軸(向きが分かるように)
    for (k, axis) in [Axis::X, Axis::Y, Axis::Z].into_iter().enumerate() {
        let tip = geom::scale(axis.unit(), half);
        if let Some((a, b)) = pr.seg([0.0; 3], tip) {
            painter.line_segment([a, b], Stroke::new(1.2, axis_ink[k].gamma_multiply(0.7)));
            painter.text(
                b,
                egui::Align2::LEFT_BOTTOM,
                axis.label(),
                kmb_ui::theme::prop(11.0),
                axis_ink[k],
            );
        }
    }
    let selected = |s: &Sel| ed.selection.contains(s);
    let picked_refs: Vec<EntityId> = ed
        .picked_constraint
        .and_then(|c| sk.constraint(c))
        .map(|c| c.kind.refs())
        .unwrap_or_default();
    // 面(薄く塗る。選んだ面とマウスの下の面は濃く)
    for f in &sol.faces {
        let pts: Vec<Pos2> = f
            .points
            .iter()
            .filter_map(|p| sol.points.get(p))
            .map(|q| pr.at(*q))
            .collect();
        if pts.len() < 3 {
            continue;
        }
        let sel = selected(&Sel::Face(f.lines.clone()));
        let hov = hover == Some(&Hit::Face(f.lines.clone()));
        let (col, a) = if sel {
            (accent, 0.35)
        } else if hov {
            (accent, 0.2)
        } else {
            (free_ink, 0.07)
        };
        let c = pr.at(f.center);
        let mut m = egui::epaint::Mesh::default();
        let fill = col.gamma_multiply(a);
        m.colored_vertex(c, fill);
        for p in &pts {
            m.colored_vertex(*p, fill);
        }
        let k = pts.len() as u32;
        for i in 0..k {
            m.add_triangle(0, 1 + i, 1 + (i + 1) % k);
        }
        painter.add(egui::Shape::mesh(m));
    }
    // 線分と円
    let ink_of = |id: EntityId, pts: &[EntityId]| {
        let free = pts.iter().any(|p| sol.free.contains(p)) || sol.free.contains(&id);
        if selected(&Sel::Ent(id)) || picked_refs.contains(&id) {
            accent
        } else if sk.is_included(id) {
            linked_ink
        } else if free {
            free_ink
        } else {
            fixed_ink
        }
    };
    for e in &sk.entities {
        let (pts, id, cons) = match e {
            Entity3d::Line {
                id,
                a,
                b,
                construction,
            } => {
                let (Some(a), Some(b)) = (sol.points.get(a), sol.points.get(b)) else {
                    continue;
                };
                (vec![*a, *b], *id, *construction)
            }
            Entity3d::Circle {
                id,
                center,
                normal,
                construction,
                ..
            } => {
                let (Some(c), Some(r)) = (sol.points.get(center), sol.radii.get(id)) else {
                    continue;
                };
                (
                    circle_points(*c, *r, *sol.normals.get(id).unwrap_or(normal)),
                    *id,
                    *construction,
                )
            }
            Entity3d::Point { .. } => continue,
        };
        let col = ink_of(id, &e.points());
        let w = if hover == Some(&Hit::Line(id)) || hover == Some(&Hit::Circle(id)) {
            3.0
        } else {
            1.8
        };
        for win in pts.windows(2) {
            let Some((a, b)) = pr.seg(win[0], win[1]) else {
                continue;
            };
            if cons {
                painter.add(egui::Shape::dashed_line(
                    &[a, b],
                    Stroke::new(w * 0.7, col.gamma_multiply(0.8)),
                    5.0,
                    4.0,
                ));
            } else {
                painter.line_segment([a, b], Stroke::new(w, col));
            }
        }
    }
    // ボディから求めた線
    for (id, lines) in &sol.derived {
        let on = selected(&Sel::Derived(*id));
        let col = if on { accent } else { linked_ink };
        let w = if on || hover == Some(&Hit::Derived(*id)) {
            3.0
        } else {
            1.8
        };
        for l in lines {
            if let [p] = l[..] {
                painter.circle_filled(pr.at(p), w + 1.5, col);
                continue;
            }
            let pts: Vec<Pos2> = l.iter().map(|q| pr.at(*q)).collect();
            painter.add(egui::Shape::line(pts, Stroke::new(w, col)));
        }
    }
    // 投影・交差の道具: 今の作図面へ写した下絵(薄く)と、押すと写す線(太く)。道具の間だけ描く
    // (2D の「投影」と同じ。まとめてレビュー R1)。隠したボディは下絵にも入らない
    if let Some(c) = ed
        .on_plane
        .as_ref()
        .filter(|_| matches!(ed.tool, Tool3::Project | Tool3::Intersect))
    {
        let faint = Stroke::new(1.0, linked_ink.gamma_multiply(0.45));
        let under: Vec<&Vec<P3>> = match ed.tool {
            Tool3::Project => c.edges.iter().flatten().flatten().collect(),
            _ => c.pieces.iter().flatten().map(|p| &p.3).collect(),
        };
        let strong = Stroke::new(3.0, linked_ink);
        for (l, st) in under
            .into_iter()
            .map(|l| (l, faint))
            .chain(c.hover.iter().map(|l| (l, strong)))
        {
            if let [p] = l[..] {
                painter.circle_filled(pr.at(p), st.width + 1.0, st.color);
            } else {
                let pts: Vec<Pos2> = l.iter().map(|q| pr.at(*q)).collect();
                painter.add(egui::Shape::line(pts, st));
            }
        }
        // 面・辺・点を写す時は、元のボディの角・辺も示す
        if let Some(Hit::Proj { body, items }) = hover
            && let Some(g) = ed.geoms.get(*body)
        {
            let st = Stroke::new(2.0, linked_ink.gamma_multiply(0.8));
            for (_, pts) in items.iter().filter_map(|i| g.named.get(*i)) {
                let sp: Vec<Pos2> = pts.iter().map(|q| world_at(&ed.cam, rect, *q)).collect();
                if let [p] = sp[..] {
                    painter.circle_stroke(p, 6.0, st);
                } else {
                    painter.add(egui::Shape::line(sp, st));
                }
            }
        }
    }
    // 取り込む道具: マウスの下を押すと取り込むボディの角・辺・円
    let lit: &[usize] = match hover {
        Some(Hit::Body(i)) => std::slice::from_ref(i),
        Some(Hit::Bodies(v)) => v,
        _ => &[],
    };
    for it in lit.iter().filter_map(|i| ed.body_items.get(*i)) {
        let pts = item_screen(&ed.cam, rect, &it.shape);
        let st = Stroke::new(3.0, linked_ink);
        if let [p] = pts[..] {
            painter.circle_stroke(p, 6.0, st);
            painter.circle_filled(p, 3.5, linked_ink);
        } else {
            painter.add(egui::Shape::line(pts, st));
        }
    }
    // 軸の向きの差の寸法: 測る軸の線の先から相手の点まで、補助の点線
    let dim_col = kmb_ui::theme::mix(pal.ink2, accent, 0.3);
    for c in &sk.constraints {
        if let Kind3d::AxisDistance { a, b, axis, .. } = &c.kind
            && let (Some(a), Some(b)) = (sol.points.get(a), sol.points.get(b))
        {
            let mut q = *a;
            q[axis.index()] = b[axis.index()];
            if let Some((q, b)) = pr.seg(q, *b) {
                painter.add(egui::Shape::dashed_line(
                    &[q, b],
                    Stroke::new(1.0, dim_col),
                    4.0,
                    3.0,
                ));
            }
        }
    }
    // 点
    let inc_points = sk.included_parts().0;
    for e in &sk.entities {
        let Entity3d::Point { id, .. } = e else {
            continue;
        };
        let Some(q) = sol.points.get(id) else {
            continue;
        };
        let at = pr.at(*q);
        let col = if selected(&Sel::Ent(*id)) || picked_refs.contains(id) {
            accent
        } else if inc_points.contains(id) {
            linked_ink
        } else if sol.free.contains(id) {
            free_ink
        } else {
            fixed_ink
        };
        let r = if hover == Some(&Hit::Point(*id)) {
            5.0
        } else {
            3.2
        };
        painter.circle_filled(at, r, col);
    }
    // 拘束の印
    for l in marks {
        let on = ed.picked_constraint == Some(l.id) || hover == Some(&Hit::Constraint(l.id));
        let r = label_rect(ui, l);
        let bg = if on {
            accent
        } else {
            kmb_ui::theme::mix(pal.ink2, pal.inks.sketch_constraint, 0.6)
        };
        painter.rect_filled(r, kmb_ui::theme::corner(3.0), bg);
        painter.text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            &l.text,
            kmb_ui::theme::prop(11.0),
            Color32::WHITE,
        );
    }
    // 寸法: 2D と同じく、形から離した寸法線と補助の線・矢印、地の箱なしの値(測るだけの寸法は点線)
    let away = {
        let pts: Vec<Pos2> = sol.points.values().map(|p| pr.at(*p)).collect();
        if pts.is_empty() {
            rect.center()
        } else {
            let sum = pts.iter().fold(Vec2::ZERO, |a, p| a + p.to_vec2());
            (sum / pts.len() as f32).to_pos2()
        }
    };
    let taken = std::cell::RefCell::new(Vec::new());
    let mut rects = Vec::new();
    for d in dim_shapes {
        let on = ed.picked_constraint == Some(d.id) || hover == Some(&Hit::Constraint(d.id));
        let (line, text) = if on {
            (accent, accent)
        } else if d.reference {
            (pal.ink3, pal.ink3)
        } else {
            (pal.ink2, pal.ink)
        };
        let look = crate::annot::DimLook {
            line,
            text,
            dashed: d.reference,
            away,
            toward: d.toward,
            taken: &taken,
        };
        if let Some(r) = crate::annot::screen_dimension(&painter, &look, &d.shape, d.text.clone()) {
            rects.push((d.id, r));
        }
    }
    // 描きかけと吸い付き
    let guess = hover_pos
        .filter(|_| {
            !matches!(
                ed.tool,
                Tool3::Select | Tool3::Include | Tool3::Project | Tool3::Intersect
            )
        })
        .and_then(|pos| match ed.start {
            Some(_) if ed.tool == Tool3::Line => ed.line_end(pos, sk, sol, &dp),
            _ => ed.snap(pos, sk, sol, &dp).map(|a| (a, None)),
        });
    if let (Some(pos), Some((at, dir))) = (hover_pos, guess) {
        if at.id.is_some() || at.origin {
            painter.circle_stroke(pr.at(at.p), 7.0, Stroke::new(1.5, accent));
        }
        let guide = Stroke::new(1.4, accent.gamma_multiply(0.8));
        if let Some(start) = ed.start {
            match ed.tool {
                Tool3::Line => {
                    // 軸のガイド: 始めの点を通る軸の線を、軸の色の点線で画面の端まで
                    if let Some(Dir::Axis(axis)) = dir {
                        let far = 4.0 * half;
                        let (a, b) = (
                            geom::add(start.p, geom::scale(axis.unit(), -far)),
                            geom::add(start.p, geom::scale(axis.unit(), far)),
                        );
                        if let Some((a, b)) = pr.seg(a, b) {
                            painter.add(egui::Shape::dashed_line(
                                &[a, b],
                                Stroke::new(1.2, axis_ink[axis.index()]),
                                6.0,
                                4.0,
                            ));
                        }
                    }
                    if let Some((a, b)) = pr.seg(start.p, at.p) {
                        painter.line_segment([a, b], guide);
                    }
                    let named = match dir {
                        Some(Dir::Plane(k)) => Some((
                            dp.dir_label(k),
                            match dp.axes {
                                Some((a, b)) => axis_ink[if k == 0 { a } else { b }.index()],
                                None => accent,
                            },
                        )),
                        Some(Dir::Axis(axis)) => {
                            Some((format!("{} に沿う", axis.label()), axis_ink[axis.index()]))
                        }
                        None => None,
                    };
                    if let Some((text, col)) = named {
                        // 向きの名前は、下に出す位置の数と重ならないよう、終点の右上に
                        painter.text(
                            pr.at(at.p) + Vec2::new(12.0, -6.0),
                            egui::Align2::LEFT_BOTTOM,
                            text,
                            kmb_ui::theme::prop(11.0),
                            col,
                        );
                    }
                    let len = geom::len(geom::sub(at.p, start.p));
                    let text = if ed.typed.is_empty() {
                        ed.fmt.length(len)
                    } else {
                        format!("{}▏", ed.typed)
                    };
                    painter.text(
                        pr.at(geom::scale(geom::add(start.p, at.p), 0.5)) + Vec2::new(0.0, -14.0),
                        egui::Align2::CENTER_BOTTOM,
                        text,
                        kmb_ui::theme::prop(12.0),
                        pal.ink,
                    );
                }
                Tool3::Rectangle => {
                    if let Some([p1, p2, p3, p4]) = rect_corners(&dp, start.p, at.p) {
                        for (a, b) in [(p1, p2), (p2, p3), (p3, p4), (p4, p1)] {
                            if let Some((a, b)) = pr.seg(a, b) {
                                painter.line_segment([a, b], guide);
                            }
                        }
                    }
                }
                Tool3::Circle => {
                    if let Some(rim) = pr.on_plane(pos, start.p, dp.n) {
                        let r = geom::len(geom::sub(rim, start.p));
                        let pts: Vec<Pos2> = circle_points(start.p, r, dp.n)
                            .into_iter()
                            .map(|q| pr.at(q))
                            .collect();
                        painter.add(egui::Shape::line(pts, guide));
                    }
                }
                _ => {}
            }
        }
        // 位置(スケッチの座標)
        painter.text(
            pos + Vec2::new(16.0, 14.0),
            egui::Align2::LEFT_TOP,
            format!("{:.2}, {:.2}, {:.2}", at.p[0], at.p[1], at.p[2]),
            kmb_ui::theme::prop(10.5),
            pal.ink2,
        );
    }
    // 左上の題: 名前・状態・道具
    let title = format!(
        "{} ・ 3D スケッチ ・ {} ・ {}{}",
        entry.name,
        status_text(sol),
        ed.tool.label(),
        if ed.construction { "(補助線)" } else { "" }
    );
    painter.text(
        rect.left_top() + Vec2::new(10.0, 8.0),
        egui::Align2::LEFT_TOP,
        title,
        kmb_ui::theme::prop(13.0),
        pal.ink,
    );
    if let Some(n) = &ed.note {
        painter.text(
            rect.left_top() + Vec2::new(10.0, 28.0),
            egui::Align2::LEFT_TOP,
            n,
            kmb_ui::theme::prop(12.0),
            pal.inks.sketch_note,
        );
    }
    rects
}
