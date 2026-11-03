//! 選ぶ道具: 押して選ぶ・引いて動かす・囲んで選ぶ・長押しと Alt+クリックの重なりの一覧・
//! ダブルクリックでつながりを選ぶ。

use super::*;

/// 選ぶ道具の、このコマの入力を扱う。
pub(super) fn act(
    ui: &mut egui::Ui,
    state: &mut ViewState,
    editor: &mut Editor,
    input: Input<'_>,
    out: &mut Acted,
) {
    let Input {
        sketch,
        sol,
        base_sketch,
        base_sol,
        aug,
        values,
        refs,
        response,
        t,
        tol,
        cursor,
        ctrl,
        primary_drag,
        long_id,
        long_state,
        long_fired,
        ..
    } = input;
    // 札だけを選んでいる時は、矢印で位置を微調整する。値の欄の入力は奪わない。
    if input.free_keys
        && editor.edit_dim.is_none()
        && editor.placing.is_none()
        && let Some((id, rect)) = editor.picked_constraint.and_then(|id| {
            state
                .labels
                .iter()
                .find(|(c, _)| *c == id)
                .map(|(_, r)| (id, *r))
        })
        && let Some(delta) = ui.input_mut(|i| {
            if !i.modifiers.is_none() {
                return None;
            }
            [
                (egui::Key::ArrowLeft, Vec2::new(-5.0, 0.0)),
                (egui::Key::ArrowRight, Vec2::new(5.0, 0.0)),
                (egui::Key::ArrowUp, Vec2::new(0.0, -5.0)),
                (egui::Key::ArrowDown, Vec2::new(0.0, 5.0)),
            ]
            .into_iter()
            .find_map(|(key, d)| i.consume_key(egui::Modifiers::NONE, key).then_some(d))
        })
    {
        let mut s = base_sketch.clone();
        s.adopt_solution(base_sol);
        s.place_label(id, t.to_world(rect.center() + delta), base_sol);
        out.result = Some(s);
        ui.ctx().request_repaint();
        return;
    }
    // 長押し: 押した所に重なっている点・線・領域の一覧を出す(離した時のクリックは捨てる)
    if response.is_pointer_button_down_on()
        && primary_drag
        && long_state == 0
        && let Some((origin, held)) = ui.input(|i| {
            let p = &i.pointer;
            let still = p
                .press_origin()
                .zip(p.latest_pos())
                .is_some_and(|(a, b)| (a - b).length() <= 4.0);
            p.press_origin()
                .filter(|_| still)
                .zip(p.press_start_time().map(|t| i.time - t))
        })
    {
        if held >= 0.5 {
            // 重なりが 2 つ以上の時だけ一覧を出す(1 つなら離した時のクリックで選ぶ)
            let list = others_at(base_sketch, base_sol, t.to_world(origin), tol);
            let shown = list.len() > 1;
            ui.ctx()
                .data_mut(|m| m.insert_temp(long_id, if shown { 2u8 } else { 1 }));
            if shown {
                editor.others = Some((origin, list));
                editor.others_hover = None;
            }
        } else {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
    // 掴むものは、動かし始めた位置でなく押した位置で決める(速く動かしても掴める)。
    // 何もない所から引いた時は囲んで選ぶ
    // 長押しで一覧を出した後は、押したままでは掴まない(egui は長く押すと引き始めとみなす)。
    // 動かしたら一覧を閉じて、いつもどおり掴む・囲む
    let moved = ui.input(|i| {
        i.pointer
            .press_origin()
            .zip(i.pointer.latest_pos())
            .is_some_and(|(a, b)| (a - b).length() > 4.0)
    });
    if moved && editor.others.is_some() && response.dragged() {
        editor.others = None;
        editor.others_hover = None;
    }
    let late = long_fired
        && moved
        && response.dragged()
        && editor.drag.is_none()
        && editor.window.is_none();
    if (response.drag_started() || late)
        && primary_drag
        && (!long_fired || moved)
        && let Some(p) = ui.input(|i| i.pointer.press_origin())
    {
        let w = t.to_world(p);
        let on_mark = crate::marks::at(&state.marks, p).is_some();
        // 寸法の値の札を掴んだら、札を動かす(寸法線を出す側と位置が変わる)
        let grabbed = state.labels.iter().find(|(_, r)| r.contains(p));
        let label = grabbed.map(|(id, _)| *id);
        editor.dim_grab = grabbed.map_or(Vec2::ZERO, |(_, r)| r.center() - p);
        match crate::drag::grab(base_sketch, base_sol, &editor.selection, w, tol) {
            _ if label.is_some() => {
                editor.dim_drag = label;
                editor.picked_constraint = label;
                editor.clear_selection();
            }
            Some(g) if !on_mark => editor.drag = Some(g),
            _ => {
                editor.window = Some(p);
                editor.lasso = vec![p];
            }
        }
    }
    let at = response
        .interact_pointer_pos()
        .or_else(|| ui.input(|i| i.pointer.latest_pos()));
    if editor.window.is_some()
        && editor.select_mode != crate::display::SelectMode::Window
        && let Some(p) = at
        && editor.lasso.last().is_none_or(|q| (*q - p).length() >= 2.0)
    {
        editor.lasso.push(p);
    }
    if let (Some(g), Some(p), Some(values)) = (editor.drag.as_mut(), at, values)
        && g.blocked.is_none()
    {
        let w = t.to_world(p);
        let mut start = base_sketch.clone();
        start.adopt_solution(base_sol);
        let mut moved = g.moved(&start, w);
        // 前のフレームの解から続けて解く(押した時の形から一度に解くと、長穴の線が
        // 交差するなど、拘束は満たすが崩れた形へ跳ぶ)
        let last = g.last.as_ref().map(|(_, s)| s);
        if let Ok(s) = kmb_sketch::solve_dragging(&start, &moved, values, &g.held, last)
            && !matches!(s.status, SolveStatus::Inconsistent { .. })
        {
            g.stuck = g.distance(w) > tol && crate::drag::unchanged(base_sol, &s);
            moved.adopt_solution(&s);
            g.last = Some((moved, s));
        }
        // 点を別の点・線の上へ引いたら、離すと付く拘束を札で見せる(Ctrl で付けない。K21)
        g.drop = match (g.point, g.last.as_ref()) {
            (Some(pt), Some((m, s))) if !ctrl && !g.stuck => {
                drop_target(m, s, refs, pt, w, tol).map(|(_, _, label)| label)
            }
            _ => None,
        };
    }
    if let Some((_, s)) = editor.drag.as_ref().and_then(|g| g.last.as_ref()) {
        out.preview = Some(s.clone());
    }
    if response.drag_stopped()
        && let (Some(id), Some(p)) = (editor.dim_drag.take(), at)
    {
        let mut s = base_sketch.clone();
        s.adopt_solution(base_sol);
        if s.constraints.iter().any(|c| c.id == id) {
            s.place_label(id, t.to_world(p + editor.dim_grab), base_sol);
            out.result = Some(s);
        }
    }
    if response.drag_stopped() {
        // 引いたものを選ぶ(すぐ寸法や拘束を付けられる)
        if let Some(g) = editor.drag.take() {
            editor.selection = g.select;
            // パターンの写しは、並びの拘束でソルバが元に付いて行かせる
            if let Some((m, s)) = g.last.filter(|_| !g.stuck) {
                // 点を別の点・線の上で離したら、一致・線上・円周上を付ける(K21)
                let dropped = match (g.point, at) {
                    (Some(pt), Some(p)) if !ctrl => {
                        drop_target(&m, &s, refs, pt, t.to_world(p), tol)
                    }
                    _ => None,
                };
                out.result = Some(match dropped {
                    Some((aug, k, _)) => match attach_dropped(aug, k, values) {
                        Ok(made) => made,
                        Err(why) => {
                            editor.notice = Some(why.into());
                            m
                        }
                    },
                    None => m,
                });
            }
        }
        if let (Some(a), Some(b)) = (editor.window.take(), at) {
            let add = ui.input(|i| i.modifiers.shift || i.modifiers.command);
            let crossing = b.x < a.x && editor.select_mode == crate::display::SelectMode::Window;
            // 3D の視点では、画面の四角の隅を平面へ写した四角形で囲む
            let win = if t.space.is_some() {
                let r = Rect::from_two_pos(a, b);
                crate::drag::Window::quad(
                    [
                        r.left_top(),
                        r.right_top(),
                        r.right_bottom(),
                        r.left_bottom(),
                    ]
                    .map(|p| t.to_world(p)),
                )
            } else {
                crate::drag::Window::new(t.to_world(a), t.to_world(b))
            };
            if !add {
                editor.clear_selection();
            }
            let mut path: Vec<[f64; 2]> = editor.lasso.drain(..).map(|p| t.to_world(p)).collect();
            path.push(t.to_world(b));
            let picked = match editor.select_mode {
                crate::display::SelectMode::Window => {
                    crate::drag::window_pick(base_sketch, base_sol, win, crossing)
                }
                crate::display::SelectMode::Lasso => {
                    crate::display::lasso_pick(base_sketch, base_sol, &path)
                }
                crate::display::SelectMode::Paint => {
                    crate::display::paint_pick(base_sketch, base_sol, &path, tol)
                }
            };
            let picked: Vec<EntityId> = picked
                .into_iter()
                .filter(|id| crate::display::selectable(editor, base_sketch, *id))
                .collect();
            editor.selection.extend(picked);
            if !add {
                editor.regions.clear();
            }
            if crossing && editor.filter.profiles {
                for k in
                    crate::drag::window_regions(crate::annot::regions(base_sketch, base_sol), win)
                {
                    if !editor.regions.contains(&k) {
                        editor.regions.push(k);
                    }
                }
            }
        }
    }
    // Alt+クリックでも重なりの一覧を出す(長押しのほかの入口)
    let alt = ui.input(|i| i.modifiers.alt);
    if response.clicked()
        && !long_fired
        && alt
        && let Some(p) = response.interact_pointer_pos()
    {
        let list = others_at(base_sketch, base_sol, t.to_world(p), tol);
        if !list.is_empty() {
            editor.others = Some((p, list));
            editor.others_hover = None;
            // 開いたクリックで閉じないよう、このフレームは開いたばかりと伝える
            ui.ctx()
                .data_mut(|m| m.insert_temp(long_id.with("alt"), true));
        }
    } else if response.clicked()
        && editor.edit_dim.is_some()
        && response
            .interact_pointer_pos()
            .is_some_and(|p| state.labels.iter().any(|(_, r)| r.contains(p)))
    {
        // 寸法の値を直している間の札のクリックは、名前を式に入れる(選びは変えない)
    } else if response.clicked()
        && !long_fired
        && editor.filter.dims
        && let Some(id) = response.interact_pointer_pos().and_then(|p| {
            state
                .labels
                .iter()
                .find(|(_, r)| r.contains(p))
                .map(|(id, _)| *id)
        })
    {
        // 寸法の値の札: その寸法を選ぶ(Delete で消す。値はダブルクリックで直す。網羅版 F6)
        editor.clear_selection();
        editor.regions.clear();
        editor.picked_constraint = Some(id);
    } else if response.clicked()
        && !long_fired
        && editor.filter.constraints
        && let Some(id) = response
            .interact_pointer_pos()
            .and_then(|p| crate::marks::at(&state.marks, p))
    {
        // 拘束の印: その拘束を選ぶ(もう一度で外す)。Delete か右クリックで外せる
        editor.clear_selection();
        editor.regions.clear();
        editor.picked_constraint = (editor.picked_constraint != Some(id)).then_some(id);
    } else if response.clicked()
        && !long_fired
        && let Some(w) = cursor
    {
        // Shift は足すだけ、Ctrl は足し外し
        let (shift, toggle) = ui.input(|i| (i.modifiers.shift, i.modifiers.command));
        let add = shift || toggle;
        editor.picked_constraint = None;
        if !add {
            editor.regions.clear();
        }
        // 前の操作の知らせ(オフセットできなかった等)は次の選びで消す
        editor.notice = None;
        // 選べる物(絞り込み・出していない物を除く)のうち、いちばん近いもの。点が先
        let hit = sketch
            .hits(sol, w[0], w[1], tol)
            .into_iter()
            .find(|h| crate::display::selectable(editor, sketch, h.entity));
        match hit {
            Some(h) => {
                if !add {
                    editor.clear_selection();
                }
                if aug.kind(h.entity).is_some() {
                    // ボディの辺や角は選ぶだけでは写さない(網羅版 G10)。寸法や拘束に
                    // 使った時に写す(パネルの操作は lib.rs、道具は prune が残す)
                    if !(toggle && editor.selection.remove(&h.entity)) {
                        editor.select(h.entity);
                    }
                } else if !(toggle && editor.selection.remove(&h.entity)) {
                    editor.select(h.entity);
                }
            }
            None => {
                // 線の無い所: 閉じた領域の中なら、その領域を選ぶ(Shift で足す、Ctrl で足し外し)
                if !add {
                    editor.clear_selection();
                }
                let areas = crate::annot::regions(base_sketch, base_sol);
                if editor.filter.profiles
                    && let Some(key) = crate::annot::region_at(&areas, w)
                {
                    match editor.regions.iter().position(|k| k == key) {
                        Some(i) if toggle => {
                            editor.regions.remove(i);
                        }
                        Some(_) => {}
                        None => editor.regions.push(key.to_string()),
                    }
                }
            }
        }
    }
    // ダブルクリック: 端でつながった線分・円弧を全部選ぶ(寸法の値の札の上は値を直す)
    // (直前にも押していると egui は三度押しと数えるので、それも同じに扱う)
    if (response.double_clicked() || response.triple_clicked())
        && !alt
        && let Some(p) = response.interact_pointer_pos()
        && !state.labels.iter().any(|(_, r)| r.contains(p))
        && let Some(chain) = {
            let w = t.to_world(p);
            base_sketch
                .hits(base_sol, w[0], w[1], tol)
                .into_iter()
                .map(|h| base_sketch.chain_of(h.entity))
                .find(|c| !c.is_empty())
        }
    {
        editor.selection.extend(chain);
    }
}
