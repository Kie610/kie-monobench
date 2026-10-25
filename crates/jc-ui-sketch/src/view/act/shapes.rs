//! 描く道具(線分・円・点と文字・矩形・円弧・接線円弧・長穴・多角形): 押した点で形を足す。

use super::*;

/// 描く道具(線分・円・点と文字・矩形・円弧・接線円弧・長穴・多角形)の、このコマの入力を扱う。
pub(super) fn act(ui: &mut egui::Ui, editor: &mut Editor, input: Input<'_>, out: &mut Acted) {
    let Input {
        sketch,
        sol,
        aug,
        values,
        response,
        t,
        tol,
        cursor,
        ctrl,
        enter,
        ..
    } = input;
    match editor.tool {
        Tool::Line => {
            // 前の線分の端から押したまま引くと接線円弧になる
            if response.drag_started_by(egui::PointerButton::Primary)
                && editor.line_base.is_some()
                && let (Some(start), Some(p)) =
                    (editor.pending, ui.input(|i| i.pointer.press_origin()))
                && {
                    let w = t.to_world(p);
                    (w[0] - start.pos[0]).hypot(w[1] - start.pos[1]) <= tol
                }
            {
                editor.arc_drag = true;
            }
            if editor.arc_drag && response.drag_stopped() {
                editor.arc_drag = false;
                if let (Some(start), Some(w)) = (editor.pending, cursor)
                    && let Some(at) = start.point
                    && let Some(line) = line_ending_at(sketch, at)
                {
                    let end = input.pick(w);
                    let mut s = sketch.clone();
                    s.adopt_solution(sol);
                    match s.add_tangent_arc(line, at, (end.point, end.pos)) {
                        Some(arc) => {
                            // 円弧の終わりから線分を続ける
                            let tip = match s.entity(arc) {
                                Some(Entity::Arc { start, end, .. }) => {
                                    if *start == at {
                                        *end
                                    } else {
                                        *start
                                    }
                                }
                                _ => at,
                            };
                            editor.pending = Some(Pick {
                                point: Some(tip),
                                pos: end.pos,
                            });
                            out.result = Some(s);
                        }
                        None => {
                            editor.notice = Some(
                                "接線円弧を作れません: 線分の延長の上では円弧になりません".into(),
                            );
                        }
                    }
                }
            }
            // クリックした所か、Enter で固定した寸法の所へ(固定していない値はカーソルで決まる)
            let commit = (response.clicked() || enter) && !editor.arc_drag;
            let target = match editor.pending {
                None if response.clicked() => cursor.map(|p| input.pick(p)),
                Some(start) if commit => {
                    let w = cursor.unwrap_or([start.pos[0] + 1.0, start.pos[1]]);
                    Some(input.aim_pick(editor, w))
                }
                _ => None,
            };
            if let Some(end) = target {
                match editor.pending {
                    None => editor.pending = Some(end),
                    Some(start) => {
                        let mut s = sketch.clone();
                        s.adopt_solution(sol);
                        let a = start
                            .point
                            .unwrap_or_else(|| s.add_point(start.pos[0], start.pos[1]));
                        let b = end
                            .point
                            .unwrap_or_else(|| s.add_point(end.pos[0], end.pos[1]));
                        if a != b {
                            let first = *editor.chain_start.get_or_insert(a);
                            // 平行・直角の相手: つなげて描いた前の線分と、触れた線分
                            let mut others: Vec<EntityId> = start
                                .point
                                .and_then(|p| line_ending_at(sketch, p))
                                .into_iter()
                                .collect();
                            others.extend(editor.infer.recent.iter().copied());
                            let line = s.add_line(a, b);
                            // 描いた向きから、水平・垂直・平行・直角・接するを推測して付ける(Ctrl で止める)。
                            // ただし輪郭を閉じた時など、ほかの拘束で既に決まっていれば付けない
                            if !ctrl {
                                crate::infer::add_line_rel(
                                    &mut s, sol, line, start.pos, end.pos, &others, values,
                                );
                            }
                            if let Some(value) = editor.hud.dim("長さ") {
                                s.add_constraint(ConstraintKind::Length { line, value });
                            }
                            out.result = Some(s);
                            // 続けて描けるよう、終点を次の始点にする。最初の点へ戻ったら輪が閉じたので終える
                            editor.pending = Some(Pick {
                                point: Some(b),
                                pos: end.pos,
                            });
                            editor.hud.reset_values();
                            if b == first {
                                // 輪が閉じた: つながりを区切る(線の道具は続く)
                                editor.cancel();
                                editor.last_tool = Some(Tool::Line);
                            }
                        }
                    }
                }
            }
        }
        Tool::Circle => match editor.pending {
            None if response.clicked() => editor.pending = cursor.map(|p| input.pick(p)),
            Some(center) if response.clicked() || enter => {
                let w = cursor.unwrap_or([center.pos[0] + 1.0, center.pos[1]]);
                let e = aimed(editor, w);
                let r = (e[0] - center.pos[0]).hypot(e[1] - center.pos[1]);
                if r > 1e-6 {
                    let mut s = sketch.clone();
                    s.adopt_solution(sol);
                    let c = center
                        .point
                        .unwrap_or_else(|| s.add_point(center.pos[0], center.pos[1]));
                    let circle = s.add_circle(c, r);
                    if let Some(value) = editor.hud.dim("直径") {
                        s.add_constraint(ConstraintKind::Diameter { circle, value });
                    } else if !editor.hud.pinned()
                        && let Some(p) = input.pick(w).point
                        && p != c
                    {
                        // 円周を既にある点に合わせたら、その点を通る円にする(大きさが自由なまま
                        // 点から離れないように。2 点・3 点の円と同じ「円周上」の拘束)
                        s.add_constraint(ConstraintKind::PointOnCircle { point: p, circle });
                    }
                    out.result = Some(s);
                }
                editor.pending = None;
                editor.hud.clear();
            }
            _ => {}
        },
        Tool::Point if enter && let Some((x, y)) = typed_pair(&editor.typed) => {
            // 「x, y」(式も可)を打って Enter: 原点からの横と縦の寸法を付けた点を置く
            let mut s = sketch.clone();
            let mut sol = sol.clone();
            s.adopt_solution(&sol);
            let origin = jc_sketch::with_origin(&mut s, &mut sol);
            out.keep.push(origin);
            // 負の値は向きにして、寸法は大きさで付ける。式で数にならない時はカーソルの側に置く
            let part = |v: &str, at: f64| match v.strip_prefix('-') {
                Some(rest) => (
                    rest.trim().to_string(),
                    -length_mm(rest).unwrap_or(at.abs()),
                ),
                None => (v.to_string(), length_mm(v).unwrap_or(at)),
            };
            let w = cursor.unwrap_or([1.0, 1.0]);
            let ((vx, px), (vy, py)) = (part(&x, w[0]), part(&y, w[1]));
            let p = s.add_point(px, py);
            s.add_constraint(ConstraintKind::HorizontalDistance {
                a: origin,
                b: p,
                value: vx,
            });
            s.add_constraint(ConstraintKind::VerticalDistance {
                a: origin,
                b: p,
                value: vy,
            });
            editor.typed.clear();
            out.result = Some(s);
        }
        Tool::Point | Tool::Text => {
            if response.clicked()
                && let Some(w) = cursor
            {
                // 文字: 置いた点を基準に、スケッチを終えて文字を作る(lib.rs が画面に頼む)
                editor.wants_text = editor.tool == Tool::Text;
                let mut s = sketch.clone();
                s.adopt_solution(sol);
                match input.pick(w).point.map(|id| (id, aug.kind(id))) {
                    // 中点に置いた点は、中点の拘束ごと残す
                    Some((id, Some(crate::snap::Kind::Midpoint))) => out.keep.push(id),
                    // 原点や角に置いた点は、写した点と一致させる(元の形が動けばついてくる)
                    Some((id, Some(_))) => {
                        let p = sol.points[&id];
                        let new = s.add_point(p[0], p[1]);
                        s.add_constraint(ConstraintKind::Coincident { a: new, b: id });
                    }
                    // 既にある点の上に置いた点は、その点と一致させる(重ねるだけにしない)
                    Some((id, None)) => {
                        let p = sol.points[&id];
                        let new = s.add_point(p[0], p[1]);
                        s.add_constraint(ConstraintKind::Coincident { a: new, b: id });
                    }
                    _ => {
                        s.add_point(w[0], w[1]);
                    }
                }
                out.result = Some(s);
            }
        }
        Tool::Rectangle | Tool::CenterRectangle | Tool::RoundedRectangle => {
            // 幅・高さを固定した時は、カーソルのある側へその大きさで描く
            let target = match editor.pending {
                None if response.clicked() => cursor.map(|p| input.pick(p)),
                Some(first) if response.clicked() || enter => {
                    let w = cursor.unwrap_or([first.pos[0] + 1.0, first.pos[1] + 1.0]);
                    Some(input.aim_pick(editor, w))
                }
                _ => None,
            };
            if let Some(end) = target {
                match editor.pending {
                    None => editor.pending = Some(end),
                    Some(first) => {
                        let mut s = sketch.clone();
                        s.adopt_solution(sol);
                        let (a, b) = ((first.point, first.pos), (end.point, end.pos));
                        if editor.tool == Tool::RoundedRectangle {
                            // 半径は打たなければ短い辺の 1/5。打っても打たなくても寸法にする
                            let r = rounded_radius(editor, first.pos, end.pos);
                            let value = editor
                                .hud
                                .dim("角の半径")
                                .unwrap_or_else(|| format!("{} mm", round_mm(r)));
                            if let Some((l, _)) = s.add_rounded_rectangle(a, b, r, &value) {
                                let (w, h) = (editor.hud.dim("幅"), editor.hud.dim("高さ"));
                                s.rounded_rectangle_size(l, w.as_deref(), h.as_deref());
                                out.result = Some(s);
                            } else {
                                editor.notice = Some(
                                    "角の丸い長方形を作れません: 角の半径は 0 より大きく、短い辺の半分より小さくしてください"
                                        .into(),
                                );
                            }
                        } else {
                            let made = if editor.tool == Tool::Rectangle {
                                s.add_rectangle(a, b)
                            } else {
                                s.add_center_rectangle(a, b)
                            };
                            if let Some(l) = made {
                                // 1 本目は横、2 本目は縦の辺
                                for (line, label) in [(l[0], "幅"), (l[1], "高さ")] {
                                    if let Some(value) = editor.hud.dim(label) {
                                        s.add_constraint(ConstraintKind::Length { line, value });
                                    }
                                }
                                out.result = Some(s);
                            } else {
                                editor.notice = Some(
                                    "矩形を作れません: 幅か高さが 0 です(隅を離して置くか、値を打ってください)"
                                        .into(),
                                );
                            }
                        }
                        editor.pending = None;
                        editor.hud.clear();
                    }
                }
            }
        }
        Tool::Arc => {
            let commit = response.clicked() || enter;
            match (editor.pending, editor.pending2) {
                (None, _) if response.clicked() => editor.pending = cursor.map(|p| input.pick(p)),
                (Some(c), None) if commit => {
                    let w = cursor.unwrap_or([c.pos[0] + 1.0, c.pos[1]]);
                    let st = input.aim_pick(editor, w);
                    editor.pending2 = Some(st);
                    editor.arc_turn =
                        Some(((st.pos[1] - c.pos[1]).atan2(st.pos[0] - c.pos[0]), 0.0));
                }
                (Some(c), Some(st)) if commit => {
                    let w = cursor.unwrap_or(st.pos);
                    let r = (st.pos[0] - c.pos[0]).hypot(st.pos[1] - c.pos[1]);
                    let w = aimed(editor, w);
                    let a = (w[1] - c.pos[1]).atan2(w[0] - c.pos[0]);
                    let e = [c.pos[0] + r * a.cos(), c.pos[1] + r * a.sin()];
                    if r > 1e-6 {
                        let mut s = sketch.clone();
                        s.adopt_solution(sol);
                        let cp = c.point.unwrap_or_else(|| s.add_point(c.pos[0], c.pos[1]));
                        let sp = st
                            .point
                            .unwrap_or_else(|| s.add_point(st.pos[0], st.pos[1]));
                        // 終点は円周の上へ寄せる(角度を固定していなければ、吸着した点はそのまま使う)
                        let snapped = (!editor.hud.pinned())
                            .then(|| input.pick(w).point)
                            .flatten();
                        let ep = snapped.unwrap_or_else(|| s.add_point(e[0], e[1]));
                        if sp != ep {
                            // 円弧は中心のまわりに反時計回りで持つ。時計回りに引いた時は端を入れ替える
                            let arc = if arc_ccw(editor) {
                                s.add_arc(cp, sp, ep)
                            } else {
                                s.add_arc(cp, ep, sp)
                            };
                            if let Some(value) = editor.hud.dim("半径") {
                                s.add_constraint(ConstraintKind::Radius { circle: arc, value });
                            }
                            out.result = Some(s);
                        }
                    }
                    editor.pending = None;
                    editor.pending2 = None;
                    editor.arc_turn = None;
                    editor.hud.clear();
                }
                _ => {}
            }
        }
        Tool::TangentArc => {
            if response.clicked()
                && let Some(w) = cursor
            {
                match editor.pending {
                    None => {
                        // 線分か円弧の端にだけ吸着する(網羅版 C7)
                        let p = input.pick(w);
                        if p.point
                            .is_some_and(|id| curve_ending_at(sketch, id).is_some())
                        {
                            editor.pending = Some(p);
                            editor.notice = None;
                        } else {
                            editor.notice = Some(
                                "接線円弧は線分か円弧の端から始めます。端を押してください".into(),
                            );
                        }
                    }
                    Some(start) => {
                        let at = start.point.expect("端の点");
                        let end = input.pick(w);
                        let mut s = sketch.clone();
                        s.adopt_solution(sol);
                        if let Some(curve) = curve_ending_at(sketch, at)
                            && s.add_tangent_arc_from(curve, at, (end.point, end.pos))
                                .is_some()
                        {
                            out.result = Some(s);
                        } else {
                            editor.notice = Some(
                                "接線円弧を作れません: 終わりが端の向きの直線の上にあると円弧になりません"
                                    .into(),
                            );
                        }
                        editor.pending = None;
                    }
                }
            }
        }
        Tool::Slot => {
            let commit = response.clicked() || enter;
            match (editor.pending, editor.pending2) {
                (None, _) if response.clicked() => editor.pending = cursor.map(|p| input.pick(p)),
                (Some(a), None) if commit => {
                    let w = cursor.unwrap_or([a.pos[0] + 1.0, a.pos[1]]);
                    editor.pending2 = Some(input.aim_pick(editor, w));
                }
                (Some(a), Some(b)) if commit => {
                    let w = aimed(editor, cursor.unwrap_or(b.pos));
                    // 幅の半分は、カーソルから中心を結ぶ線までの距離
                    let half = slot_half(a.pos, b.pos, w);
                    let mut s = sketch.clone();
                    s.adopt_solution(sol);
                    if let Some([_, _, left, _]) =
                        s.add_slot((a.point, a.pos), (b.point, b.pos), half.max(1e-3))
                    {
                        // 中心を結ぶ補助線(最後に足したもの)
                        let axis = s.entities.last().map(Entity::id);
                        if let (Some(line), Some(value)) = (axis, editor.hud.dim("長さ")) {
                            s.add_constraint(ConstraintKind::Length { line, value });
                        }
                        // 幅は端の円弧の直径の寸法
                        if let Some(value) = editor.hud.dim("幅") {
                            s.add_constraint(ConstraintKind::Diameter {
                                circle: left,
                                value,
                            });
                        }
                        out.result = Some(s);
                    }
                    editor.pending = None;
                    editor.pending2 = None;
                    editor.hud.clear();
                }
                _ => {}
            }
        }
        Tool::Polygon => {
            match editor.pending {
                None if response.clicked() => editor.pending = cursor.map(|p| input.pick(p)),
                Some(c) if response.clicked() || enter => {
                    let n = polygon_sides(editor);
                    editor.polygon_n = n;
                    let w = cursor.unwrap_or([c.pos[0] + 1.0, c.pos[1]]);
                    let q = input.aim_pick(editor, w);
                    let mut s = sketch.clone();
                    s.adopt_solution(sol);
                    if s.add_polygon((c.point, c.pos), (q.point, q.pos), n)
                        .is_some()
                    {
                        // 頂点を載せる補助の円(多角形で足したもの)
                        let ring = s.entities.iter().rev().find_map(|e| match e {
                            Entity::Circle {
                                id,
                                construction: true,
                                ..
                            } => Some(*id),
                            _ => None,
                        });
                        if let (Some(circle), Some(value)) = (ring, editor.hud.dim("半径")) {
                            s.add_constraint(ConstraintKind::Radius { circle, value });
                        }
                        out.result = Some(s);
                    }
                    editor.pending = None;
                    editor.hud.clear();
                }
                _ => {}
            }
        }
        _ => {}
    }
}
