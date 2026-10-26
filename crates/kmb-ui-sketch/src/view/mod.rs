//! スケッチのビューと描画ツール。ホイール押しで移動、ホイールで拡大縮小、右ドラッグで 3D の視点へ回す。
//! 1 コマは `sketch_view` が順に進める: 視点(ここ)→ キー(`keys.rs`)→ 今の道具の操作(`act.rs`、
//! 道具ごとの子は `act/`)→ 描画(`draw.rs`、描きかけの見本と寸法は `draw/`)。

use egui::{Color32, Pos2, Rect, Sense, Stroke, Vec2};
use kmb_sketch::{ConstraintKind, Entity, EntityId, Sketch, Solution, SolveStatus};
use kmb_ui::theme::RightMenu as _;
use kmb_ui::view3d;
use std::collections::{BTreeSet, HashMap};

mod act;
mod draw;
mod editor;
mod inline;
mod keys;
mod measure;
mod paint;
mod project;
mod shapes;
mod state;
mod tool;

use act::{Acted, Input, act};
#[cfg(test)]
pub(crate) use draw::corner_preview;
use draw::{Frame, draw};
pub(crate) use editor::*;
pub(crate) use inline::*;
use keys::{Keys, keys};
pub(crate) use measure::*;
pub(crate) use paint::*;
pub(crate) use project::*;
pub(crate) use shapes::*;
pub(crate) use state::*;
pub use tool::Tool;
pub(crate) use tool::*;

/// 戻す物が無い時の Esc を、この秒数のうちに 2 回押したらスケッチを終える。
const ESC_TWICE: f64 = 0.4;

pub(crate) fn sketch_view(
    ui: &mut egui::Ui,
    state: &mut ViewState,
    editor: &mut Editor,
    sketch: &Sketch,
    sol: &Solution,
    values: Option<&DimValues>,
    backdrop: Backdrop<'_>,
) -> Option<Sketch> {
    let Backdrop {
        title,
        underlay,
        pictures,
        refs,
        sections,
        bodies,
        origins,
        plane,
        scene,
        projection,
    } = backdrop;
    // 平面の分からないスケッチは 3D の視点で見られない
    if plane.is_none() {
        state.space = None;
    }
    // 「正面から見る」(N1c): 3D の視点ならスケッチの平面を真正面から見る向きへ回し、回し終えたら
    // 平らな図に戻す。回す間に形全体が収まる所へも移す。回し終えた時に平らな図の「すべて表示」と同じ見え方になるよう、表示の基準を今の
    // 範囲に取り直し、回す中心と倍率をそれに合わせる
    if std::mem::take(&mut editor.look_front) {
        match (state.space.as_mut(), plane) {
            (Some(cam), Some(plane)) => {
                let (min, max) = bounds(sol, underlay, refs);
                let size = (max - min).max(Vec2::splat(20.0));
                let center = min + size / 2.0;
                let rect = ui.available_rect_before_wrap();
                let fit = (rect.width() / size.x).min(rect.height() / size.y) * 0.7;
                let target = plane.to_world([center.x as f64, center.y as f64]);
                cam.look_along_to(
                    plane.normal().map(|v| v as f32),
                    target.map(|v| v as f32),
                    fit,
                );
                state.frame = Some((center, size));
                state.to_front = true;
            }
            // もう正面から見ている: 全体が収まるよう合わせるだけ
            _ => state.fit(),
        }
        log::info!("スケッチ: 正面から見る");
    }
    // 3D の視点(N1b): 視点の操作(右ドラッグで回す・ホイール押しで動かす・ホイールで拡大)と
    // ボディの絵は 3D のビューのものを使い、その上にスケッチを重ねる
    let (rect, mut response) = match (state.space.as_mut(), plane) {
        (Some(cam), Some(plane)) => {
            if std::mem::take(&mut state.refit) {
                let (lo, hi) = space_bounds(sol, &plane, scene);
                cam.fit_box(lo, hi, ui.available_rect_before_wrap());
            }
            cam.perspective = projection.perspective_for(cam);
            if cam.step_turn(ui.input(|i| i.time)) {
                ui.ctx().request_repaint();
            }
            // ボディを選んだ・指した色は作業空間の色のまま(画面の差し色はスケッチ中に水色になる)
            let colors = view3d::Colors::from_look(&kmb_ui::theme::Look {
                sketch: false,
                ..kmb_ui::theme::look(ui.ctx())
            });
            // ボディの面は選べないので、マウスの下の面を色付けしない
            let controls = view3d::Controls {
                no_face_hover: true,
                no_guide_planes: true,
                no_box: true,
                ..Default::default()
            };
            let (_, resp, rect) = view3d::view3d(
                ui,
                cam,
                scene,
                &[],
                &kmb_core::Overlay::default(),
                &[],
                controls,
                &colors,
                kmb_ui::Shading::ShadedEdges,
                view3d::Guides {
                    grid: false,
                    axes: true,
                },
            );
            (rect, resp)
        }
        _ => {
            let (rect, response) =
                ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
            // ホイール押しで動かす(右ドラッグは 3D の視点へ回す。下で)
            if response.dragged_by(egui::PointerButton::Middle) {
                state.pan += response.drag_delta();
            }
            if response.hovered() {
                let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                if scroll != 0.0 {
                    // カーソルの下の位置を動かさずに拡大縮小する
                    let old = state.zoom;
                    state.zoom = (state.zoom * (1.0 + scroll * 0.002)).clamp(0.02, 100.0);
                    if let Some(p) = response.hover_pos() {
                        let k = state.zoom / old;
                        let d = p - rect.center();
                        state.pan = d - (d - state.pan) * k;
                    }
                }
            }
            // ホイールの二度押し: 全体が収まる表示に戻す(3D の視点では 3D のビューと同じく回す中心を決める)
            if response.double_clicked_by(egui::PointerButton::Middle) {
                state.fit();
            }
            (rect, response)
        }
    };
    let primary_drag = ui.input(|i| i.pointer.primary_down());
    let (center, size) = *state.frame.get_or_insert_with(|| {
        let (min, max) = bounds(sol, underlay, refs);
        let size = (max - min).max(Vec2::splat(20.0));
        (min + size / 2.0, size)
    });
    let fit = (rect.width() / size.x).min(rect.height() / size.y) * 0.7;
    // 正面へ回し終えた: 平らな図へ戻す。回す中心が画面の同じ所に残り、大きさも変わらないようにする
    if state.to_front
        && let (Some(cam), Some(plane)) = (state.space, plane)
        && !cam.turning()
    {
        let c = plane.to_local(cam.target.map(|v| v as f64));
        state.zoom = cam.zoom / fit;
        state.pan = cam.pan
            - Vec2::new(
                (c[0] as f32 - center.x) * cam.zoom,
                -(c[1] as f32 - center.y) * cam.zoom,
            );
        state.space = None;
        state.to_front = false;
    }
    let mut t = match (state.space, plane) {
        (Some(cam), Some(plane)) => Transform::spatial(cam, rect, plane),
        _ => Transform::flat(center, rect.center() + state.pan, fit * state.zoom),
    };
    // 平らな図で右ドラッグ: 今の見え方のまま 3D の視点へ移り、そのまま回す(N1b)
    if t.space.is_none()
        && response.dragged_by(egui::PointerButton::Secondary)
        && let Some(plane) = plane
    {
        let mut cam = space_camera(&t, rect, plane);
        cam.perspective = projection.perspective_for(&cam);
        cam.orbit(response.drag_delta());
        cam.perspective = projection.perspective_for(&cam);
        state.space = Some(cam);
        state.to_front = false;
        t = Transform::spatial(cam, rect, plane);
        log::info!("スケッチ: 3D の視点で見る");
    }
    editor.in_space = state.space.is_some();
    state.seen = plane.map(|p| match &t.space {
        Some(s) => (s.cam, rect),
        None => (space_camera(&t, rect, p), rect),
    });
    state.last = Some(t.clone());
    let tol = 8.0 / t.scale as f64;
    slop_click(ui, editor, &mut response);
    let cursor = response.hover_pos().map(|p| t.to_world(p));
    editor.line_base = (editor.tool == Tool::Line)
        .then_some(editor.pending)
        .flatten()
        .and_then(|start| {
            let p = start.point?;
            let line = line_ending_at(sketch, p)?;
            let Some(Entity::Line { a, b, .. }) = sketch.entity(line) else {
                return None;
            };
            let other = if *a == p { *b } else { *a };
            let (q, o) = (sol.points.get(&p)?, sol.points.get(&other)?);
            Some((o[1] - q[1]).atan2(o[0] - q[0]))
        });

    // 付いた寸法の値の札をダブルクリック: その場で値を直す
    if response.double_clicked()
        && let Some(p) = response.interact_pointer_pos()
        && let Some(id) = state
            .labels
            .iter()
            .find(|(_, r)| r.contains(p))
            .map(|(id, _)| *id)
        && let Some(value) = sketch
            .constraints
            .iter()
            .find(|c| c.id == id)
            .and_then(|c| c.kind.value())
    {
        editor.edit_dim = Some((id, value.to_string()));
        editor.edit_dim_fresh = true;
        editor.dim_error = None;
    }

    // 長押しで一覧を出したか(押している間と、離したフレームまで覚える)
    // (1: 長押しを調べた、2: 一覧を出したので離した時のクリックを捨てる)
    let long_id = response.id.with("long_press");
    let long_state = ui.ctx().data(|m| m.get_temp::<u8>(long_id)).unwrap_or(0);
    let long_fired = long_state == 2;
    if long_state > 0 && !ui.input(|i| i.pointer.primary_down()) && !response.clicked() {
        ui.ctx().data_mut(|m| m.remove::<u8>(long_id));
    }

    // 吸い付く先(原点・中点・既存の形の角と辺)を仮に足したスケッチで道具を動かし、
    // 使った仮の要素だけを残す(snap.rs)
    let (base_sketch, base_sol) = (sketch, sol);
    // Ctrl を押している間は、新しい吸い付き(曲線の上・交点・並び)と推測の拘束を止める
    let ctrl = ui.input(|i| i.modifiers.command);
    let shift = ui.input(|i| i.modifiers.shift);
    let mut wants = snap_wants(editor.tool);
    wants.crossings &= !ctrl;
    let aug = crate::snap::augment(sketch, sol, refs, wants);
    let (sketch, sol) = (&aug.sketch, &aug.sol);
    // 描く道具: カーソルを曲線の上や、通った点と並ぶ位置へ寄せる。線分は向きも推測する(infer.rs)
    let (mut cursor, mut snapped, mut line_rel) =
        crate::infer::steer(editor, sketch, sol, cursor, tol, wants.midpoints && !ctrl);
    // 刻みに吸い付く(描く道具。既にある点・曲線の上・並びの案内に寄せる時はそちらを先にする)。
    // 刻みは利用者が決めた値のまま、拡大しても変えない(docs/避ける形.md)。Ctrl で止める
    if editor.display.snap_grid
        && draws(editor.tool)
        && !ctrl
        && let Some(w) = cursor
        && snapped
            .as_ref()
            .is_none_or(|sn| sn.on.is_none() && sn.guides.is_empty())
        && sketch.nearest_point(sol, w[0], w[1], tol).is_none()
    {
        let g = crate::display::to_grid(w, editor.display.snap_step.max(1e-6));
        cursor = Some(g);
        if let Some(sn) = snapped.as_mut() {
            sn.pos = g;
            sn.label = None;
        }
        line_rel = None;
    }
    if response.clicked()
        && let Some(sn) = &snapped
        && let Some(on) = sn.on
    {
        editor.infer.placed.push((sn.pos, on));
    }
    // 形を作って道具が終わると覚えたものは消えるので、ここで写しておく
    let placed = editor.infer.placed.clone();

    // ---- 操作 ----
    let Keys {
        free_keys,
        hud_on,
        mut enter,
        raw_enter,
        enter_empty,
        x_toggle,
    } = keys(ui, state, editor, base_sketch, base_sol, &response, &t, tol);
    // 右クリックの「OK」: 打った値があれば、その値で形を作ってから道具を終える
    let ok = std::mem::take(&mut editor.ok);
    let confirm = std::mem::take(&mut editor.confirm);
    if ok || confirm {
        enter = true;
    }
    let cursor = if ok {
        editor.menu_at.or(cursor)
    } else if confirm {
        editor.last_cursor.or(cursor)
    } else {
        cursor
    };
    if let Some(w) = cursor {
        editor.last_cursor = Some(w);
    }
    // 形を作ったら道具を終えるか見るため、操作の前の道具を覚える
    let used = editor.tool;
    // 円弧の終点を決める間、始点からカーソルが回った向きを覚える(時計回り・反時計回りを決める)
    if editor.tool == Tool::Arc
        && let (Some(c), Some(_), Some(w), Some((last, sweep))) =
            (editor.pending, editor.pending2, cursor, editor.arc_turn)
    {
        let a = (w[1] - c.pos[1]).atan2(w[0] - c.pos[0]);
        let mut d = a - last;
        while d > std::f64::consts::PI {
            d -= std::f64::consts::TAU;
        }
        while d < -std::f64::consts::PI {
            d += std::f64::consts::TAU;
        }
        editor.arc_turn = Some((a, sweep + d));
    }
    // 前に作れなかった知らせは、次のクリックで消す(また作れなければ出し直す)
    if response.clicked() && editor.tool != Tool::Select {
        editor.notice = None;
    }
    let Acted {
        mut result,
        preview,
        keep,
        to_project,
        origin_hover,
    } = act(
        ui,
        state,
        editor,
        Input {
            sketch,
            sol,
            base_sketch,
            base_sol,
            aug: &aug,
            values,
            refs,
            sections,
            bodies,
            origins,
            response: &response,
            t: &t,
            tol,
            cursor,
            ctrl,
            shift,
            enter,
            enter_empty,
            raw_enter,
            free_keys,
            primary_drag,
            long_id,
            long_state,
            long_fired,
        },
    );

    // 描く道具は形を作った後も続ける(Kie 2026-10-15)。選んだ要素を並べる・動かす
    // 道具だけは 1 回で終えて選ぶ道具へ戻る
    if result.is_some() && finishes_on_make(used) && editor.tool == used {
        editor.finish(used);
    }
    if ok && editor.tool == used && used != Tool::Select {
        editor.finish(used);
    }
    // 補助線で描く(右クリックか X で切り替え): 描く道具で足した線を補助線にする
    if editor.draw_construction
        && draws(used)
        && let Some(s) = result.as_mut()
    {
        let new: Vec<EntityId> = s
            .entities
            .iter()
            .map(|e| e.id())
            .filter(|id| sketch.entity(*id).is_none())
            .collect();
        for id in new {
            // 道具が補助線で作った線(多角形の外接円など)はそのまま
            let plain = matches!(
                s.entity(id),
                Some(
                    Entity::Line {
                        construction: false,
                        ..
                    } | Entity::Circle {
                        construction: false,
                        ..
                    } | Entity::Arc {
                        construction: false,
                        ..
                    }
                )
            );
            if plain {
                s.toggle_construction(id);
            }
        }
    }
    // 中心線で描く(パレットの線種): 描く道具で足した線分を中心線にする(K4)
    if editor.draw_centerline
        && draws(used)
        && let Some(s) = result.as_mut()
    {
        let new: Vec<EntityId> = s
            .entities
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    Entity::Line {
                        construction: false,
                        ..
                    }
                )
            })
            .map(|e| e.id())
            .filter(|id| sketch.entity(*id).is_none())
            .collect();
        for id in new {
            s.toggle_centerline(id);
        }
    }
    if used == Tool::Line
        && editor.tool == Tool::Line
        && editor.pending.is_some()
        && (response.double_clicked() || enter_empty)
    {
        // つながりを区切る(線の道具は続く。Esc で道具を終える)
        editor.cancel();
        editor.last_tool = Some(Tool::Line);
    }
    // 曲線の上に置いた位置にできた点へ、線上・円周上の拘束を足す(仮の点を除く前に)
    if let Some(s) = result.as_mut()
        && !ctrl
    {
        crate::infer::attach_placed(sketch, s, &placed, values);
    }
    // 使い終えた置き場所は忘れる(描きかけの点の分だけ残す)
    let waiting: Vec<[f64; 2]> = [editor.pending, editor.pending2, editor.pending3]
        .into_iter()
        .flatten()
        .map(|p| p.pos)
        .collect();
    editor.infer.placed.retain(|(p, _)| {
        waiting
            .iter()
            .any(|q| (q[0] - p[0]).hypot(q[1] - p[1]) < 1e-9)
    });
    // 形が変わる時は、選んでいた仮の辺や角(写していない物)を選びから外す(番号が変わるため)
    if result.is_some() {
        editor
            .selection
            .retain(|id| aug.kind(*id).is_none() || keep.contains(id));
    }
    let mut result = match result {
        Some(s) => Some(aug.prune(s, &keep)),
        None if !keep.is_empty() => Some(aug.materialize(&keep)),
        None => None,
    }
    .map(|(s, moved)| {
        editor.renumber(&moved);
        s
    });
    // 投影リンクを外して写した(投影・交差): 元とのつながりを切り、動かないよう固定する
    let unlinked = match editor.tool {
        Tool::Project => !editor.project_link,
        Tool::Intersect => !editor.intersect_link,
        _ => false,
    };
    if unlinked && let Some(s) = result.as_mut() {
        let fresh: Vec<EntityId> = s
            .projections
            .iter()
            .map(|p| p.entity)
            .filter(|id| !base_sketch.is_projected(*id))
            .collect();
        s.projections.retain(|p| !fresh.contains(&p.entity));
        *s = crate::menu::set_fixed(s, &fresh, true);
    }
    // 右クリックのメニュー(開いた時に覚えた様子で項目を出す)。図は画面の要素の操作で
    // 名前で探せるようにする(台本の rclick "スケッチの図")
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "スケッチの図"));
    let mut ask = None;
    response.right_menu(|ui| {
        if let Some(cx) = &editor.menu {
            ask = crate::menu::sketch_menu(ui, cx, base_sketch);
        }
    });
    if x_toggle && result.is_none() {
        result =
            crate::constraints_ui::selection_action(editor, base_sketch, base_sol, "construction");
    }
    if let Some(a) = ask
        && result.is_none()
    {
        result = crate::menu::apply(ui.ctx(), state, editor, base_sketch, base_sol, values, a);
    }
    // Ctrl+C / Ctrl+X / Ctrl+V(ほかの欄に入力している時と、描いている途中は除く)。貼ると
    // 写した時と同じ位置に重ね、移動の道具を始める
    if result.is_none() && free_keys && editor.pending.is_none() && editor.typed.is_empty() {
        let ctx = ui.ctx().clone();
        for ask in crate::clipboard::asks(&ctx) {
            match ask {
                crate::clipboard::Ask::Copy => {
                    crate::clipboard::copy(&ctx, editor, base_sketch, base_sol);
                }
                crate::clipboard::Ask::Cut => {
                    result = crate::clipboard::cut(&ctx, editor, base_sketch, base_sol);
                }
                crate::clipboard::Ask::Paste(text) => {
                    if crate::clipboard::held(&ctx).is_some_and(|h| h.text == text) {
                        result = crate::clipboard::paste(&ctx, editor, base_sketch, base_sol);
                    }
                }
            }
            if result.is_some() {
                break;
            }
        }
    }
    // マウスの下の吸い付く先(描く道具)と、選べる既存の辺(選ぶ・寸法の道具)
    let hover = cursor.and_then(|w| {
        let h = if matches!(
            editor.tool,
            Tool::Select | Tool::Dimension | Tool::Constraint(_)
        ) {
            sketch.hit_test(sol, w[0], w[1], tol)
        } else if !matches!(
            editor.tool,
            Tool::Trim | Tool::Extend | Tool::Break | Tool::MirrorAxis
        ) {
            sketch.nearest_point(sol, w[0], w[1], tol)
        } else {
            None
        }?;
        Some(hover_shape(sketch, sol, h.entity, aug.kind(h.entity)))
    });
    // 選ぶ・寸法の道具で、押せば選ばれる自分の要素(太く光らせて先に見せる)。引いている間は出さない
    let busy = editor.drag.is_some() || editor.window.is_some() || editor.dim_drag.is_some();
    let hovered = match &editor.others_hover {
        Some(Other::Entity(id)) => Some(*id),
        Some(Other::Region(_)) => None,
        None => cursor
            .filter(|_| {
                (matches!(editor.tool, Tool::Select | Tool::Dimension | Tool::Measure)
                    || editor.tool == Tool::Offset && editor.offset.chain.is_empty())
                    && !busy
            })
            .and_then(|w| base_sketch.hit_test(base_sol, w[0], w[1], tol))
            .map(|h| h.entity)
            .or_else(|| {
                // 拘束の道具: 今の選びに足せる要素だけを光らせる
                let Tool::Constraint(name) = editor.tool else {
                    return None;
                };
                let w = cursor?;
                let h = base_sketch.hit_test(base_sol, w[0], w[1], tol)?.entity;
                let sel: Vec<EntityId> = editor.selection.iter().copied().collect();
                crate::constraint_tool::accepts(name, base_sketch, &sel, h).then_some(h)
            }),
    };
    // 指を置いた拘束の印と、選んだ拘束: 関わる要素を光らせる
    let mark_hover = response
        .hover_pos()
        .filter(|_| editor.tool == Tool::Select && !busy)
        .and_then(|p| crate::marks::at(&state.marks, p));
    let lit: Vec<EntityId> = [mark_hover, editor.picked_constraint]
        .into_iter()
        .flatten()
        .filter_map(|id| base_sketch.constraints.iter().find(|c| c.id == id))
        .flat_map(|c| crate::marks::members(base_sketch, &c.kind))
        .collect();
    let hovered = if mark_hover.is_some() { None } else { hovered };
    if editor.tool == Tool::Select {
        let icon = match &editor.drag {
            Some(g) if g.blocked.is_some() || g.stuck => egui::CursorIcon::NotAllowed,
            Some(_) => egui::CursorIcon::Grabbing,
            None if editor.window.is_some() => egui::CursorIcon::Crosshair,
            None if mark_hover.is_some() => egui::CursorIcon::PointingHand,
            None if hovered.is_some() => egui::CursorIcon::Grab,
            None => egui::CursorIcon::Default,
        };
        if icon != egui::CursorIcon::Default {
            ui.ctx().set_cursor_icon(icon);
        }
    }
    if result.is_some() {
        editor.regions.clear();
    }
    // パターンの欄を引いている間は、その値で作り直した見本を文書の代わりに描く
    let ghost = editor.pattern_ghost.clone();
    let (sketch, sol, preview) = match &ghost {
        Some((_, s, so)) => (s, so, None),
        None => (base_sketch, base_sol, preview),
    };

    draw(
        ui,
        state,
        editor,
        Frame {
            title,
            underlay,
            pictures,
            refs,
            sections,
            origins,
            plane,
            scene,
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
        },
    )
}

/// カーソルの脇に出す、次にすることの短い案内。
fn prompt(editor: &Editor) -> Option<&'static str> {
    // 点の道具で「x, y」を打っている間は、打った値と重ならないよう出さない
    if editor.tool == Tool::Point && !editor.typed.is_empty() {
        return None;
    }
    let step = editor.step();
    Some(match (editor.tool, step) {
        (Tool::Select, _) => return None,
        (Tool::Line, 0) => "始点を指定",
        (Tool::Line, _) => "次の点を指定",
        (Tool::Circle | Tool::Polygon | Tool::OuterPolygon | Tool::CenterRectangle, 0) => {
            "中心を指定"
        }
        (Tool::Circle, _) => "円周の点を指定",
        (Tool::Polygon | Tool::OuterPolygon, _) => "頂点を指定",
        (Tool::CenterRectangle, _) => "隅を指定",
        (Tool::Rectangle | Tool::ThreePointRectangle | Tool::RoundedRectangle, 0) => {
            "1 つ目の隅を指定"
        }
        (Tool::Rectangle | Tool::RoundedRectangle, _) => "対角の隅を指定",
        (Tool::ThreePointRectangle, 1) => "辺のもう一方の端を指定",
        (Tool::ThreePointRectangle, _) => "向かいの辺の位置を指定",
        (Tool::Arc, 0) => "中心を指定",
        (Tool::Arc, 1) => "始点を指定",
        (Tool::Arc, _) => "終点を指定",
        (Tool::ThreePointArc, 0) => "始点を指定",
        (Tool::ThreePointArc, 1) => "終点を指定",
        (Tool::ThreePointArc, _) => "途中の点を指定",
        (Tool::TwoPointCircle, 0) => "直径の一方の端を指定",
        (Tool::TwoPointCircle, _) => "直径のもう一方の端を指定",
        (Tool::ThreePointCircle, _) => "円周の点を指定",
        (Tool::TangentCircle, 0..=2) | (Tool::TwoTangentCircle, 0..=1) => "接する線・円を選択",
        (Tool::TangentCircle | Tool::TwoTangentCircle, _) => "置く側を指定",
        (Tool::Ellipse, 0) => "中心を指定",
        (Tool::Ellipse, 1) => "長軸の端を指定",
        (Tool::Ellipse, _) => "短軸の大きさを指定",
        (Tool::FitSpline | Tool::ControlSpline, 0) => "最初の点を指定",
        (Tool::FitSpline | Tool::ControlSpline, _) => "次の点を指定(Enter で終える)",
        (Tool::Conic, 0) => "始点を指定",
        (Tool::Conic, 1) => "終点を指定",
        (Tool::Conic, _) => "接線の交わる点を指定",
        (Tool::Blend, 0) => "1 つ目の曲線の端を選択",
        (Tool::Blend, _) => "2 つ目の曲線の端を選択",
        (Tool::MidpointLine, 0) => "中点を指定",
        (Tool::MidpointLine, _) => "端を指定",
        (Tool::EdgePolygon, 0) => "辺の始まりを指定",
        (Tool::EdgePolygon, 1) => "辺の終わりを指定",
        (Tool::EdgePolygon, _) => "置く側を指定",
        (Tool::ThreePointArcSlot, 0) => "始点を指定",
        (Tool::ThreePointArcSlot, 1) => "終点を指定",
        (Tool::ThreePointArcSlot, 2) => "途中の点を指定",
        (Tool::ThreePointArcSlot, _) => "幅を指定",
        (Tool::TangentArc, 0) => "線分か円弧の端を指定",
        (Tool::TangentArc, _) => "終点を指定",
        (Tool::Point, _) => "点を指定",
        (Tool::Text, _) => "文字を置く位置を指定",
        (Tool::Slot | Tool::CenterSlot | Tool::OverallSlot | Tool::ArcSlot, 0) => "端の中心を指定",
        (Tool::Slot | Tool::CenterSlot | Tool::OverallSlot, 1) => "もう一方の端を指定",
        (Tool::ArcSlot, 1 | 2) => "円弧の点を指定",
        (Tool::Slot | Tool::CenterSlot | Tool::OverallSlot | Tool::ArcSlot, _) => "幅を指定",
        (Tool::Trim, _) => "消す区間を選択",
        (Tool::Extend, _) => "延ばす端を選択",
        (Tool::Break, _) => "分ける区間を選択",
        (Tool::Fillet | Tool::Chamfer, _) => "角を選択",
        (Tool::MirrorAxis, _) => "対称の線を選択",
        (Tool::Dimension | Tool::Measure, _) => "要素を選択",
        (Tool::Project, _) => "写す角・辺・面を選択",
        (Tool::Intersect, _) => "断面を選択",
        (Tool::Constraint(_), _) => "要素を選択",
        (Tool::Offset, 0) => "ずらす線を選択",
        (Tool::Offset, _) => "距離を指定",
        (Tool::LinearPattern | Tool::Move, 0) => "基準の点を指定",
        (Tool::LinearPattern | Tool::Move, _) => "行き先を指定",
        (Tool::CircularPattern | Tool::Rotate | Tool::Scale, 0) => "中心を指定",
        (Tool::CircularPattern | Tool::Rotate | Tool::Scale, _) => "向きを指定",
        (Tool::Origin, _) => "原点にする角を選択",
    })
}

/// 描く道具で、押してから離すまでに手がこれ以内(画面の点)しか動かなければクリックにする。
const CLICK_SLOP: f32 = 10.0;

/// 描く道具では、押してから離すまでに手が少し動いても(egui のクリックの 4 点を超えても)
/// クリックとして点を置く。囲んで選ぶ・なぞって消す・線の端から引く円弧は引く操作のまま。
fn slop_click(ui: &egui::Ui, editor: &Editor, response: &mut egui::Response) {
    let id = response.id.with("press_at");
    if response.drag_started_by(egui::PointerButton::Primary)
        && let Some(p) = ui.input(|i| i.pointer.press_origin())
    {
        ui.ctx().data_mut(|m| m.insert_temp(id, p));
    }
    if !response.drag_stopped_by(egui::PointerButton::Primary) {
        return;
    }
    let from = ui.ctx().data_mut(|m| m.remove_temp::<egui::Pos2>(id));
    if !matches!(editor.tool, Tool::Select | Tool::Trim)
        && !editor.arc_drag
        && let (Some(a), Some(b)) = (from, ui.input(|i| i.pointer.latest_pos()))
        && a.distance(b) <= CLICK_SLOP
    {
        response.flags |= egui::response::Flags::FAKE_PRIMARY_CLICKED;
    }
}
