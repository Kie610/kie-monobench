//! 2D スケッチのビューの描画。操作の段(`sketch_view`)が決めた物を受け取り、格子・下絵・
//! スケッチの線と点・拘束の印・寸法・描きかけの見本・案内の札を描く。寸法の札の上で値を打つ欄も
//! ここで出すので、置いた寸法で直したスケッチを返す。

use super::*;

mod notes;
mod preview;

/// 操作の段で決まった、1 コマの描画に要る物(`sketch_view` が操作を済ませてから渡す)。
pub(super) struct Frame<'a> {
    pub(super) title: &'a str,
    pub(super) underlay: &'a Segments,
    pub(super) pictures: &'a [jc_core::OverlayPicture],
    pub(super) refs: &'a [jc_sketch::RefItem],
    pub(super) sections: &'a [Section],
    pub(super) origins: &'a [([f64; 2], jc_sketch::FaceOrigin)],
    pub(super) plane: Option<jc_core::Plane>,
    pub(super) sketch: &'a Sketch,
    pub(super) sol: &'a Solution,
    pub(super) values: Option<&'a DimValues>,
    pub(super) rect: Rect,
    pub(super) response: egui::Response,
    pub(super) t: Transform,
    pub(super) tol: f64,
    pub(super) cursor: Option<[f64; 2]>,
    pub(super) snapped: Option<crate::infer::Snapped>,
    pub(super) line_rel: Option<crate::infer::LineRel>,
    pub(super) preview: Option<Solution>,
    pub(super) hover: Option<Hover>,
    pub(super) hovered: Option<EntityId>,
    pub(super) busy: bool,
    pub(super) hud_on: bool,
    pub(super) lit: Vec<EntityId>,
    pub(super) mark_hover: Option<jc_sketch::ConstraintId>,
    pub(super) to_project: Vec<jc_sketch::RefItem>,
    pub(super) origin_hover: Option<[f64; 2]>,
    pub(super) long_id: egui::Id,
    pub(super) long_fired: bool,
    pub(super) result: Option<Sketch>,
}

/// 1 コマ分を描く。寸法の札の上の欄で値を決めたら、直したスケッチを返す(無ければ `frame.result`)。
pub(super) fn draw(
    ui: &mut egui::Ui,
    state: &mut ViewState,
    editor: &mut Editor,
    frame: Frame<'_>,
) -> Option<Sketch> {
    let Frame {
        title,
        underlay,
        pictures,
        refs,
        sections,
        origins,
        plane,
        sketch,
        sol,
        values,
        rect,
        response,
        t,
        tol,
        cursor,
        snapped,
        line_rel,
        preview,
        hover,
        hovered,
        busy,
        hud_on,
        lit,
        mark_hover,
        to_project,
        origin_hover,
        long_id,
        long_fired,
        result,
    } = frame;
    let mut result = result;
    let sol = preview.as_ref().unwrap_or(sol);
    let painter = ui.painter_at(rect);
    let l = jc_ui::theme::look(ui.ctx());
    // 3D の視点では、下にボディを描いた 3D のビューの上に重ねる
    if t.space.is_none() {
        painter.rect_filled(rect, jc_ui::theme::corner(0.0), l.pal.canvas);
    }
    if editor.display.grid {
        // 刻みに吸い付く時は、その刻み(とその倍)で線を引く
        let base = editor.display.snap_grid.then_some(editor.display.snap_step);
        draw_grid(&painter, rect, &t, l.pal.grid, base);
    }
    draw_axes(
        &painter,
        rect,
        &t,
        jc_ui::theme::mix(l.pal.grid, l.pal.ink3, 0.5),
    );
    // 同じ平面の下絵(キャンバス)。格子と軸の上、面の輪郭とスケッチの線の下
    if let Some(pl) = plane {
        for pic in pictures {
            painter.add(jc_ui::view3d::picture_shape(ui.ctx(), pic, |w| {
                t.to_screen(pl.to_local(w))
            }));
        }
    }
    // スケッチを置いた面の輪郭(スケッチの線より薄く、格子より濃く)
    let face = Stroke::new(1.5, jc_ui::theme::mix(l.pal.grid, l.pal.ink2, 0.55));
    for [a, b] in underlay {
        painter.line_segment([t.to_screen(*a), t.to_screen(*b)], face);
    }
    // スライス・交差の道具: 前にできたボディの断面。交差の道具では、押すと写す線を太く
    let accent = jc_ui::theme::mix(l.pal.ink2, l.pal.inks.sketch_section, 0.9);
    if editor.display.slice || editor.tool == Tool::Intersect {
        let stroke = Stroke::new(1.5, accent);
        for sec in sections {
            for [a, b] in &sec.segs {
                painter.line_segment([t.to_screen(*a), t.to_screen(*b)], stroke);
            }
        }
    }
    // 既存の形の辺(吸い付く先と寸法の相手)。投影・交差の道具の間だけ描き、ほかの道具では
    // 描かずに吸い付きだけ残す(まとめてレビュー R1: 道具を使っていないのにボディの線が出ていた)。
    // 「投影」を出さない時は描かない
    let show_refs =
        editor.display.projected && matches!(editor.tool, Tool::Project | Tool::Intersect);
    for r in refs.iter().filter(|_| show_refs) {
        if !matches!(r.shape, jc_sketch::RefShape::Point(_)) {
            draw_ref(&painter, &t, &r.shape, face);
        }
    }
    // 投影・交差の道具: 押すと写す物(面の縁・シルエット・断面の線)を太く
    for r in &to_project {
        draw_ref(&painter, &t, &r.shape, Stroke::new(3.5, accent));
    }
    // 原点を選ぶ道具: 原点にできる面の角を丸で、カーソルの所を塗って
    if editor.tool == Tool::Origin {
        for (at, _) in origins {
            let p = t.to_screen(*at);
            if origin_hover == Some(*at) {
                painter.circle_filled(p, 7.0, accent);
            } else {
                painter.circle_stroke(p, 5.0, Stroke::new(2.0, accent));
            }
        }
    }

    // 閉じた形(押し出しなどに使える領域)を薄く塗る。選ぶ道具ではカーソルの下を濃く
    // 線や点の上では、その線や点を光らせ、領域は濃くしない(何が選ばれるかを 1 つに絞って見せる)
    let hover_world = cursor.filter(|_| editor.tool == Tool::Select && !busy && hovered.is_none());
    let pick_color = pick_ink(&l);
    let areas = crate::annot::regions(sketch, sol);
    let under = match &editor.others_hover {
        Some(Other::Region(k)) => Some(k.as_str()),
        Some(Other::Entity(_)) => None,
        None => hover_world.and_then(|w| crate::annot::region_at(&areas, w)),
    };
    for a in &areas {
        // 「領域の塗り」を切った時は、選んだ領域だけ塗る
        if !editor.display.profiles && !editor.regions.contains(&a.key) {
            continue;
        }
        // 選んだ領域は選んだ色で塗る(クリックか、右から左へ囲んで選ぶ)
        let color = if editor.regions.contains(&a.key) {
            pick_color.gamma_multiply(if under == Some(a.key.as_str()) {
                0.4
            } else {
                0.3
            })
        } else {
            let strong = under == Some(a.key.as_str());
            l.pal
                .ws(l.space)
                .gamma_multiply(if strong { 0.22 } else { 0.09 })
        };
        crate::annot::fill(&painter, rect, &t, &a.loops, color);
    }

    let fixed_color = l.pal.ink;
    let free_color = free_ink(&l);
    // 描いている途中の見本と案内の色(選んだ色とは分ける)。図の中の色は作業空間の色のまま
    // (画面の差し色はスケッチ中に水色になるが、それだと「動ける」の青と紛れる)
    let selected_color = l.pal.ws(l.space);
    let construction = l.pal.ink3;
    let reference = ref_ink(&l);
    let (projected_points, _) = sketch.projected_parts();
    // 拘束の状態の色: 矛盾した拘束の相手は赤、余分な拘束の相手は黄、固定した物は緑
    let (bad, extra) = match &sol.status {
        SolveStatus::Inconsistent { conflicting } => {
            (members(sketch, conflicting), BTreeSet::new())
        }
        SolveStatus::OverConstrained { redundant } => (BTreeSet::new(), members(sketch, redundant)),
        _ => (BTreeSet::new(), BTreeSet::new()),
    };
    let pinned = pinned(sketch);
    let (bad_color, extra_color, pinned_color) = (l.pal.err, l.pal.warn, fix_ink(&l));
    let color_of = |id: EntityId, is_construction: bool| {
        if editor.selection.contains(&id) {
            pick_color
        } else if bad.contains(&id) {
            bad_color
        } else if extra.contains(&id) {
            extra_color
        } else if sketch.is_projected(id) || projected_points.contains(&id) {
            reference
        } else if is_construction {
            construction
        } else if pinned.contains(&id) {
            pinned_color
        } else if sol.fixed.contains(&id) {
            fixed_color
        } else {
            free_color
        }
    };
    // 選んだものは太く選んだ色で、カーソルの下のものは周りを淡く光らせる(押す前に分かる)
    let halo = |id: EntityId| {
        let (sel, hov) = (
            editor.selection.contains(&id),
            hovered == Some(id) || lit.contains(&id),
        );
        match (sel, hov) {
            (true, true) => Some(Stroke::new(11.0, pick_color.gamma_multiply(0.55))),
            (false, true) => Some(Stroke::new(11.0, pick_color.gamma_multiply(0.45))),
            (true, false) => Some(Stroke::new(8.0, pick_color.gamma_multiply(0.25))),
            (false, false) => None,
        }
    };
    let width = |id: EntityId, is_construction: bool| {
        if editor.selection.contains(&id) || hovered == Some(id) || lit.contains(&id) {
            3.0
        } else if is_construction {
            1.0
        } else {
            2.0
        }
    };
    let point = |id: &EntityId| sol.points.get(id).map(|p| t.to_screen(*p));

    for e in &sketch.entities {
        let id = e.id();
        let path: Vec<Pos2> = match e {
            Entity::Line { a, b, .. } => match (point(a), point(b)) {
                (Some(p), Some(q)) => vec![p, q],
                _ => continue,
            },
            Entity::Circle { center, .. } => match (sol.points.get(center), sol.radii.get(&id)) {
                (Some(c), Some(r)) => t.circle_path(*c, *r),
                _ => continue,
            },
            Entity::Arc {
                center, start, end, ..
            } => match (
                sol.points.get(center),
                sol.points.get(start),
                sol.points.get(end),
            ) {
                (Some(c), Some(s), Some(e)) => arc_points(*c, *s, *e)
                    .into_iter()
                    .map(|p| t.to_screen(p))
                    .collect(),
                _ => continue,
            },
            Entity::Point { .. } => continue,
            Entity::Ellipse { .. } | Entity::Spline { .. } | Entity::Conic { .. } => {
                // 画面で 0.3 px ほどの隔たりで折れ線にする
                let tol = (0.3 / t.scale as f64).max(jc_sketch::ARC_TOLERANCE * 0.1);
                match sketch.curve_polyline(sol, id, tol) {
                    Some(pts) => pts.into_iter().map(|p| t.to_screen(p)).collect(),
                    None => continue,
                }
            }
        };
        let is_construction = e.is_construction();
        // 出さない物(切った補助線・写した形)は、選んでいる時だけ描く
        if editor.display.hides(sketch, id) && !editor.selection.contains(&id) {
            continue;
        }
        if let Some(h) = halo(id) {
            painter.add(egui::Shape::line(path.clone(), h));
        }
        let stroke = Stroke::new(width(id, is_construction), color_of(id, is_construction));
        if sketch.is_centerline(id) && !editor.selection.contains(&id) {
            // 中心線は一点鎖線
            painter.extend(egui::Shape::dashed_line_with_offset(
                &path,
                stroke,
                &[14.0, 2.0],
                &[3.0, 3.0],
                0.0,
            ));
        } else if is_construction && !editor.selection.contains(&id) {
            painter.add(egui::Shape::dashed_line(&path, stroke, 6.0, 4.0));
        } else {
            painter.add(egui::Shape::line(path, stroke));
        }
    }
    // 円と円弧の中心は十字の印でも示す
    let centers: BTreeSet<EntityId> = sketch
        .entities
        .iter()
        .filter_map(|e| match e {
            Entity::Circle { center, .. } | Entity::Arc { center, .. } => Some(*center),
            _ => None,
        })
        .collect();
    for (id, p) in &sol.points {
        // 「点」を切った時は、選んだ点とカーソルの下の点だけ描く
        if editor.display.hides(sketch, *id)
            && !editor.selection.contains(id)
            && hovered != Some(*id)
        {
            continue;
        }
        let c = t.to_screen(*p);
        if let Some(h) = halo(*id) {
            painter.circle_filled(c, 8.0, h.color);
        }
        let r = if editor.selection.contains(id) {
            4.5
        } else {
            3.5
        };
        painter.circle_filled(c, r, color_of(*id, false));
        if centers.contains(id) {
            let k = Stroke::new(1.0, color_of(*id, false));
            painter.line_segment([c - Vec2::new(7.0, 0.0), c + Vec2::new(7.0, 0.0)], k);
            painter.line_segment([c - Vec2::new(0.0, 7.0), c + Vec2::new(0.0, 7.0)], k);
        }
    }
    // 閉じていない輪郭の端を赤い輪で示す(押し出せない理由がひと目で分かる)。
    // 「領域の塗り」と一緒に出し入れする
    if editor.display.profiles {
        let ring = Stroke::new(2.0, l.pal.err);
        for p in jc_sketch::open_ends_of(sketch, sol) {
            painter.circle_stroke(t.to_screen(p), 7.0, ring);
        }
    }

    // 自由形状で囲んでいる形と、なぞっている道
    if editor.window.is_some() && editor.select_mode != crate::display::SelectMode::Window {
        let mut path = editor.lasso.clone();
        if let Some(p) = ui.input(|i| i.pointer.latest_pos()) {
            path.push(p);
        }
        if editor.select_mode == crate::display::SelectMode::Lasso {
            if path.len() >= 3 {
                let mut closed = path.clone();
                closed.push(path[0]);
                painter.add(egui::Shape::line(closed, Stroke::new(1.5, selected_color)));
            }
        } else {
            painter.add(egui::Shape::line(
                path.clone(),
                Stroke::new(10.0, pick_color.gamma_multiply(0.3)),
            ));
        }
        if let Some(p) = path.last() {
            painter.text(
                *p + Vec2::new(10.0, 10.0),
                egui::Align2::LEFT_TOP,
                if editor.select_mode == crate::display::SelectMode::Lasso {
                    "中に入ったものを選ぶ"
                } else {
                    "なぞったものを選ぶ"
                },
                jc_ui::theme::prop(12.0),
                l.pal.ink2,
            );
        }
    }
    // 囲んでいる範囲。右へ囲む(中に入ったものを選ぶ)は実線、左へ囲む(触れたものを選ぶ)は点線で、塗りの色も変える
    if let (Some(a), Some(b)) = (editor.window, ui.input(|i| i.pointer.latest_pos()))
        && editor.select_mode == crate::display::SelectMode::Window
    {
        let crossing = b.x < a.x;
        let r = Rect::from_two_pos(a, b);
        let (fill, edge) = if crossing {
            (pick_color.gamma_multiply(0.12), pick_color)
        } else {
            (selected_color.gamma_multiply(0.12), selected_color)
        };
        painter.rect_filled(r, jc_ui::theme::corner(0.0), fill);
        let stroke = Stroke::new(1.5, edge);
        let c = [
            r.left_top(),
            r.right_top(),
            r.right_bottom(),
            r.left_bottom(),
            r.left_top(),
        ];
        if crossing {
            painter.add(egui::Shape::dashed_line(&c, stroke, 6.0, 4.0));
        } else {
            painter.add(egui::Shape::line(c.to_vec(), stroke));
        }
        painter.text(
            r.right_bottom() + Vec2::new(8.0, 8.0),
            egui::Align2::LEFT_TOP,
            if crossing {
                "触れたものを選ぶ"
            } else {
                "中に入ったものを選ぶ"
            },
            jc_ui::theme::prop(12.0),
            edge,
        );
    }
    // 掴んだものが動かせない時は、理由をカーソルの横に出す
    if let (Some(g), Some(p)) = (&editor.drag, ui.input(|i| i.pointer.latest_pos()))
        && let Some(why) = g.blocked.or(g.stuck.then_some(crate::drag::STUCK))
    {
        let galley = painter.layout_no_wrap(why.to_string(), jc_ui::theme::prop(12.0), l.pal.err);
        let r = Rect::from_min_size(
            p + Vec2::new(16.0, 12.0),
            galley.size() + Vec2::new(12.0, 6.0),
        );
        painter.rect(
            r,
            4.0,
            l.pal.panel,
            Stroke::new(1.0, l.pal.err),
            egui::StrokeKind::Inside,
        );
        painter.galley(r.min + Vec2::new(6.0, 3.0), galley, l.pal.err);
    }

    // 角の丸め・面取りで打っている値
    if let Some(w) = cursor
        && matches!(editor.tool, Tool::Fillet | Tool::Chamfer)
    {
        let what = if editor.tool == Tool::Fillet {
            "半径"
        } else {
            "距離"
        };
        let shown = if editor.typed.is_empty() {
            if editor.dim_text.trim().is_empty() {
                default_corner()
            } else {
                editor.dim_text.clone()
            }
        } else {
            format!("{}▏", editor.typed)
        };
        painter.text(
            t.to_screen(w) + Vec2::new(14.0, 14.0),
            egui::Align2::LEFT_TOP,
            format!("{what}: {shown}"),
            jc_ui::theme::prop(14.0),
            selected_color,
        );
        // 角の点を指している間、今の半径・距離で作る角の形を見せる(打つたびに変わる)
        if let Some(h) = sketch.nearest_point(sol, w[0], w[1], tol) {
            let stroke = Stroke::new(2.0, selected_color);
            for pts in corner_preview(editor, sketch, sol, h.entity) {
                let pts = pts.iter().map(|p| t.to_screen(*p)).collect();
                painter.add(egui::Shape::line(pts, stroke));
            }
        }
    }
    // 打ち込み中の寸法
    if let Some(w) = cursor
        && (editor.pending.is_some() || editor.tool == Tool::Point)
        && !editor.typed.is_empty()
    {
        let what = match editor.tool {
            Tool::Line => "長さ",
            Tool::Circle => "直径",
            Tool::Slot => "幅",
            Tool::Polygon => "辺の数",
            t => crate::more_tools::typed_label(t).unwrap_or("幅, 高さ"),
        };
        painter.text(
            t.to_screen(w) + Vec2::new(14.0, 14.0),
            egui::Align2::LEFT_TOP,
            format!("{what}: {}▏", editor.typed),
            jc_ui::theme::prop(14.0),
            selected_color,
        );
    }
    preview::previews(
        ui,
        editor,
        &painter,
        &l,
        &t,
        sketch,
        sol,
        cursor,
        &hover,
        hud_on,
        selected_color,
    );

    // 延長・破断: カーソルの下の曲線で、延びる所・切り離す区間を前もって示す
    if matches!(editor.tool, Tool::Extend | Tool::Break)
        && let Some(w) = cursor
        && let Some(h) = sketch
            .hits(sol, w[0], w[1], tol)
            .into_iter()
            .find(|h| !matches!(sketch.entity(h.entity), Some(Entity::Point { .. }) | None))
        && let Some(span) = if editor.tool == Tool::Extend {
            sketch.extend_preview(sol, h.entity, w)
        } else {
            sketch.break_preview(sol, h.entity, w)
        }
    {
        let pts: Vec<Pos2> = span.iter().map(|p| t.to_screen(*p)).collect();
        painter.add(egui::Shape::line(
            pts.clone(),
            Stroke::new(9.0, selected_color.gamma_multiply(0.25)),
        ));
        painter.add(egui::Shape::dashed_line(
            &pts,
            Stroke::new(2.5, selected_color),
            6.0,
            4.0,
        ));
        painter.text(
            t.to_screen(w) + Vec2::new(12.0, -12.0),
            egui::Align2::LEFT_BOTTOM,
            if editor.tool == Tool::Extend {
                "ここまで延ばす"
            } else {
                "ここで切り離す"
            },
            jc_ui::theme::prop(12.0),
            selected_color,
        );
    }
    // トリム: なぞっている道
    if editor.trim_path.len() > 1 {
        let pts: Vec<Pos2> = editor.trim_path.iter().map(|p| t.to_screen(*p)).collect();
        painter.add(egui::Shape::line(pts, Stroke::new(2.0, l.pal.err)));
    }
    // トリム: カーソルの下の曲線で、消える区間を前もって示す
    if editor.tool == Tool::Trim
        && editor.trim_path.is_empty()
        && let Some(w) = cursor
        && let Some(h) = sketch
            .hits(sol, w[0], w[1], tol)
            .into_iter()
            .find(|h| !matches!(sketch.entity(h.entity), Some(Entity::Point { .. }) | None))
        && let Some(span) = sketch.trim_preview(sol, h.entity, w)
    {
        let pts: Vec<Pos2> = span.iter().map(|p| t.to_screen(*p)).collect();
        painter.add(egui::Shape::line(
            pts.clone(),
            Stroke::new(9.0, l.pal.err.gamma_multiply(0.25)),
        ));
        painter.add(egui::Shape::dashed_line(
            &pts,
            Stroke::new(2.5, l.pal.err),
            6.0,
            4.0,
        ));
        painter.text(
            t.to_screen(w) + Vec2::new(12.0, -12.0),
            egui::Align2::LEFT_BOTTOM,
            "ここを消す",
            jc_ui::theme::prop(12.0),
            l.pal.err,
        );
    }
    // 描く道具の吸い付き(曲線の上・並び)と、推測した向き
    if let Some(sn) = &snapped
        && let Some(w) = cursor
    {
        let ink = selected_color;
        for (a, b) in &sn.guides {
            painter.add(egui::Shape::dashed_line(
                &[t.to_screen(*a), t.to_screen(*b)],
                Stroke::new(1.0, ink),
                5.0,
                4.0,
            ));
        }
        if sn.on.is_some() || !sn.guides.is_empty() || line_rel.is_some() {
            let c = t.to_screen(w);
            painter.circle_stroke(c, 5.0, Stroke::new(1.5, ink));
        }
        if let Some(label) = sn.label {
            badge(
                &painter,
                t.to_screen(w) + Vec2::new(12.0, 10.0),
                label,
                ink,
                l.pal.panel,
            );
        }
    }
    // 引いている点を離すと付く拘束(一致・線上・円周上。K21)
    if let (Some(label), Some(p)) = (
        editor.drag.as_ref().and_then(|g| g.drop),
        ui.input(|i| i.pointer.latest_pos()),
    ) {
        painter.circle_stroke(p, 5.0, Stroke::new(1.5, selected_color));
        badge(
            &painter,
            p + Vec2::new(12.0, 10.0),
            label,
            selected_color,
            l.pal.panel,
        );
    }
    if let Some(s) = notes::notes(
        ui, state, editor, &painter, &l, &t, sketch, sol, values, cursor, rect, &areas, mark_hover,
        pick_color, long_id, long_fired,
    ) {
        result = Some(s);
    }
    // 左上: スケッチの名前と解けた様子、その下に今の道具の案内(試作の .ctxhint)
    let mut y = rect.top() + 12.0;
    let mut banners = vec![
        (title.to_string(), l.pal.panel, l.pal.ink2),
        (
            // 今の道具の名前を頭に出す(道具は形を作った後も続くので、何をしているかを見せる)
            format!(
                "{}  ・  {}",
                crate::tool_label(editor.tool),
                if editor.choosing {
                    "使う要素をクリックで選ぶ(もう一度押すと外す)。選び終えたら Enter".to_string()
                } else if let Tool::Constraint(name) = editor.tool {
                    crate::constraint_tool::hint(name, editor)
                } else {
                    match editor.tool {
                        Tool::Polygon | Tool::OuterPolygon | Tool::EdgePolygon
                            if editor.pending.is_none() =>
                        {
                            format!(
                                "中心をクリック(辺の数 {}。数字キーで変える)",
                                editor.polygon_n.clamp(3, 64)
                            )
                        }
                        t => t.hint(editor.step()).to_string(),
                    }
                }
            ),
            l.pal.ws_soft(l.space),
            l.pal.ws(l.space),
        ),
    ];
    if let Some(m) = editor
        .measured
        .as_ref()
        .filter(|_| editor.tool == Tool::Measure)
    {
        banners.push((m.clone(), l.pal.panel, l.pal.ink));
    }
    if let Some(n) = &editor.notice {
        banners.push((n.clone(), l.pal.panel, l.pal.err));
    }
    for (text, fill, ink) in banners {
        let g = painter.layout_no_wrap(text, jc_ui::theme::prop(11.5), ink);
        let r = Rect::from_min_size(
            Pos2::new(rect.left() + 12.0, y),
            g.size() + Vec2::new(16.0, 8.0),
        );
        painter.rect(
            r,
            5.0,
            fill,
            Stroke::new(1.0, l.pal.edge),
            egui::StrokeKind::Inside,
        );
        painter.galley(r.min + Vec2::new(8.0, 4.0), g, ink);
        y = r.bottom() + 6.0;
    }
    if let Some(w) = cursor {
        painter.text(
            rect.right_bottom() + Vec2::new(-10.0, -10.0),
            egui::Align2::RIGHT_BOTTOM,
            {
                // 文書の単位で(候補 19)
                let u = jc_core::display_unit();
                format!(
                    "x {:.2}  y {:.2} {}",
                    u.from_mm(w[0]),
                    u.from_mm(w[1]),
                    u.name()
                )
            },
            jc_ui::theme::mono(11.0),
            l.pal.ink3,
        );
    }
    // 右下: 選んだ物の数と長さの合計
    if let Some(info) = selection_info(sketch, sol, editor) {
        painter.text(
            rect.right_bottom() + Vec2::new(-10.0, -28.0),
            egui::Align2::RIGHT_BOTTOM,
            info,
            jc_ui::theme::prop(12.0),
            l.pal.ink2,
        );
    }
    // カーソルの脇: 次にすることの短い案内(左上の帯の長い案内と同じ中身を短く)
    if let (Some(p), Some(text)) = (
        response.hover_pos(),
        prompt(editor).filter(|_| editor.drag.is_none() && editor.placing.is_none()),
    ) {
        let g = painter.layout_no_wrap(text.to_string(), jc_ui::theme::prop(11.5), l.pal.ink2);
        let r = Rect::from_min_size(p + Vec2::new(16.0, 20.0), g.size() + Vec2::new(10.0, 4.0));
        painter.rect_filled(
            r,
            jc_ui::theme::corner(3.0),
            l.pal.panel.gamma_multiply(0.85),
        );
        painter.galley(r.min + Vec2::new(5.0, 2.0), g, l.pal.ink2);
    }
    result
}

/// 角の丸め・面取りで角の点 `corner` を指している間、今の半径・距離で作った後の角の形
/// (作り直す線と、足す円弧・線の折れ線)。作れなければ空。押した時に作る物と同じ値を使う。
pub(crate) fn corner_preview(
    editor: &Editor,
    sketch: &Sketch,
    sol: &Solution,
    corner: EntityId,
) -> Vec<Vec<[f64; 2]>> {
    let mut s = sketch.clone();
    s.adopt_solution(sol);
    let made = if editor.tool == Tool::Fillet {
        let value = if !editor.typed.trim().is_empty() {
            typed_value(&editor.typed)
        } else if editor.dim_text.trim().is_empty() {
            default_corner()
        } else {
            typed_value(&editor.dim_text)
        };
        let r = typed_length(&value).unwrap_or(2.0);
        s.fillet_corner(corner, r, &value).is_some()
    } else {
        let text = if editor.typed.trim().is_empty() {
            &editor.dim_text
        } else {
            &editor.typed
        };
        s.chamfer_corner_with(corner, None, &crate::more_tools::chamfer_kind(text))
            .is_some()
    };
    if !made {
        return vec![];
    }
    let last = sketch.last_ids().0;
    let pos = |id: &EntityId| match s.entity(*id) {
        Some(Entity::Point { x, y, .. }) => Some([*x, *y]),
        _ => None,
    };
    let new = |id: &EntityId| id.0 > last;
    s.entities
        .iter()
        .filter_map(|e| match e {
            Entity::Line { id, a, b, .. } if new(id) || new(a) || new(b) => {
                Some(vec![pos(a)?, pos(b)?])
            }
            Entity::Arc {
                id,
                center,
                start,
                end,
                ..
            } if new(id) => Some(arc_points(pos(center)?, pos(start)?, pos(end)?)),
            _ => None,
        })
        .collect()
}
