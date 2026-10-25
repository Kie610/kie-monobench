//! 3D のビュー。ボディの三角形を平行投影か透視投影で画面に写し、深度バッファで手前の面を残して塗る。
//! 面の境目の線も同じ深度で比べるので、隠れた線は手前の面に隠れる。

use crate::{Inks, Shading};
use egui::{Color32, Pos2, Rect, Sense, Vec2};
use kmb_core::{FeatureId, Kernel, Mesh, ShapeHandle};
use std::collections::HashSet;

mod camera;
mod hit;
mod look;
#[cfg(test)]
mod pick_wait_tests;
mod raster;
mod scene;
#[cfg(test)]
mod turn_tests;

pub use camera::*;
pub use hit::*;
pub use look::*;
pub use raster::*;
pub use scene::*;

/// 選んだ面。
pub type Pick = kmb_core::FacePick;

/// 設定の回し方(拘束オービットかフリーオービット)で回す。
fn orbit(cam: &mut Camera, controls: Controls, d: Vec2) {
    if controls.free_orbit {
        cam.orbit_free(d);
    } else {
        cam.orbit(d);
    }
}

/// 3D のビューを描き、視点の操作を受け付ける。選びに関わる出来事は [`ViewEvents`] で返す。
///
/// マウス: 右ドラッグ=回す、ホイール押しドラッグ=動かす(設定で回すと入れ替え、Shift で逆)、
/// Ctrl+ホイール押しドラッグとホイール=拡大(ホイールはカーソルの位置へ寄る)、Alt+左ドラッグ=回す、
/// 左ドラッグ=範囲で選ぶ、左の長押し=重なった候補、右クリック=メニュー、ホイールのダブルクリック=回す中心を置く。
#[allow(clippy::too_many_arguments)]
pub fn view3d(
    ui: &mut egui::Ui,
    cam: &mut Camera,
    scene: &Scene,
    picked: &[Pick],
    overlay: &kmb_core::Overlay,
    ghosts: &[Ghost],
    controls: Controls,
    colors: &Colors,
    shading: Shading,
    guides: Guides,
) -> (ViewEvents, egui::Response, Rect) {
    let mut ev = ViewEvents::default();
    let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    if !cam.fitted {
        if scene.is_empty() {
            let pts = overlay.lines.iter().flat_map(|(l, _)| l);
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for p in pts {
                for i in 0..3 {
                    lo[i] = lo[i].min(p[i] as f32);
                    hi[i] = hi[i].max(p[i] as f32);
                }
            }
            if lo[0] <= hi[0] {
                cam.fit_box(lo, hi, rect);
            }
        } else {
            cam.fit(scene, rect);
        }
    }
    let (mods, pointer) = ui.input(|i| (i.modifiers, i.pointer.clone()));
    // 回す量は押した時の向きからの全部の量で決める(押した所へ戻すと元の向き。10-20 u9)
    if resp.drag_started() {
        cam.start_drag();
    }
    let d = resp.drag_delta();
    let left = resp.dragged_by(egui::PointerButton::Primary);
    let left_mode = if mods.alt {
        LeftDrag::Orbit
    } else {
        controls.left
    };
    if left && left_mode == LeftDrag::Pan {
        cam.pan_by(d);
    } else if left && left_mode == LeftDrag::Zoom {
        cam.zoom_at(
            (-d.y * 0.01).exp(),
            pointer.press_origin().unwrap_or(rect.center()),
            rect,
        );
    } else if resp.dragged_by(egui::PointerButton::Secondary)
        || (left && left_mode == LeftDrag::Orbit)
    {
        orbit(cam, controls, d);
        ev.orbiting = true;
    } else if resp.dragged_by(egui::PointerButton::Middle) {
        if mods.command {
            cam.zoom_at(
                (-d.y * 0.01).exp(),
                resp.interact_pointer_pos().unwrap_or(rect.center()),
                rect,
            );
        } else if controls.middle_orbits != mods.shift {
            orbit(cam, controls, d);
            ev.orbiting = true;
        } else {
            cam.pan_by(d);
        }
    }
    if !resp.dragged() {
        cam.end_drag();
    }
    // 範囲は押した所と今の所で持つ(向きで選び方が替わる)
    let box_id = resp.id.with("box");
    if left && left_mode == LeftDrag::Select && !controls.no_box {
        if let Some(start) = pointer.press_origin() {
            let now = pointer.latest_pos().unwrap_or(start);
            ui.ctx().data_mut(|m| m.insert_temp(box_id, (start, now)));
        }
    } else if let Some((a, b)) = ui.ctx().data(|m| m.get_temp::<(Pos2, Pos2)>(box_id)) {
        ui.ctx().data_mut(|m| m.remove::<(Pos2, Pos2)>(box_id));
        ev.boxed = Some(Rect::from_two_pos(a, b));
        ev.crossing = b.x < a.x;
    }
    if resp.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            let at = resp.hover_pos().unwrap_or(rect.center());
            cam.zoom_at((scroll * 0.002).exp(), at, rect);
        }
    }
    // 長押し: 押してから 0.5 秒、動かしていなければ 1 回だけ知らせる(離した時のクリックは捨てる)
    let long_id = resp.id.with("long");
    if resp.is_pointer_button_down_on() && pointer.button_down(egui::PointerButton::Primary) {
        let held = pointer.press_start_time().map(|t| ui.input(|i| i.time) - t);
        let still = pointer
            .press_origin()
            .zip(pointer.latest_pos())
            .is_some_and(|(a, b)| (a - b).length() <= 4.0);
        let fired = ui
            .ctx()
            .data(|m| m.get_temp::<bool>(long_id))
            .unwrap_or(false);
        if still && !fired {
            if held.is_some_and(|h| h >= 0.5) {
                ev.long_press = pointer.press_origin();
                ui.ctx().data_mut(|m| m.insert_temp(long_id, true));
            } else {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
    }
    let long_fired = ui
        .ctx()
        .data(|m| m.get_temp::<bool>(long_id))
        .unwrap_or(false);
    if !pointer.button_down(egui::PointerButton::Primary) && long_fired && !resp.clicked() {
        ui.ctx().data_mut(|m| m.remove::<bool>(long_id));
    }
    if resp.clicked_by(egui::PointerButton::Primary) && !mods.alt {
        if long_fired {
            ui.ctx().data_mut(|m| m.remove::<bool>(long_id));
        } else {
            ev.click = resp.interact_pointer_pos();
            ev.add = mods.command;
            if resp.double_clicked_by(egui::PointerButton::Primary) {
                ev.double_click = ev.click;
            }
        }
    }
    if resp.clicked_by(egui::PointerButton::Secondary) {
        ev.context = resp.interact_pointer_pos();
    }
    if resp.double_clicked_by(egui::PointerButton::Middle)
        && let Some(p) = resp.interact_pointer_pos()
        && let Some((_, at)) = picks_at(scene, cam, rect, p).into_iter().next()
    {
        cam.set_pivot(at);
    }
    ev.hover = resp.hover_pos();
    let hover = ev
        .hover
        .filter(|_| !controls.no_face_hover)
        .and_then(|p| pick_at(scene, cam, rect, p));

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, crate::corner(0.0), colors.background);
    // スケッチの線だけがある時は、形を描かずに線を重ねる
    let empty = scene.is_empty() && overlay.lines.is_empty();
    if empty && !guides.any() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "表示するボディがありません",
            crate::prop(14.0),
            ui.visuals().weak_text_color(),
        );
        return (ev, resp, rect);
    }
    if !scene.is_empty() || guides.any() {
        // 加工の途中の形を出す時は、部品の代わりにそれを描く(選びは部品のまま)
        let shape_scene: Option<std::sync::Arc<Scene>> = overlay.shape.as_ref().map(|sh| {
            let sid = resp.id.with("shape");
            // テーマを替えたら塗り直す
            match ui
                .ctx()
                .data(|d| d.get_temp::<(u64, Inks, std::sync::Arc<Scene>)>(sid))
            {
                Some((k, i, sc)) if k == sh.key && i == colors.inks => sc,
                _ => {
                    let mut sc = Scene::default();
                    sc.set(&[], vec![(FeatureId(u64::MAX), sh.mesh.clone())]);
                    sc.set_face_colors(&sh.face_colors, &colors.inks);
                    let sc = std::sync::Arc::new(sc);
                    ui.ctx()
                        .data_mut(|d| d.insert_temp(sid, (sh.key, colors.inks, sc.clone())));
                    sc
                }
            }
        });
        let gpu = crate::gpu3d::get(ui.ctx());
        // 描画の設定の「描く細かさ」。CPU で描く時は、回す・動かす間だけ半分にする(止まると描き直す)
        let settings = crate::render::current();
        let moving = resp.dragged() || cam.turning();
        let lighter = gpu.is_none() && settings.dynamic && moving;
        let ppp = ui.ctx().pixels_per_point() * settings.scale * if lighter { 0.5 } else { 1.0 };
        let size = [
            (rect.width() * ppp).round().max(1.0) as usize,
            (rect.height() * ppp).round().max(1.0) as usize,
        ];
        let key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            for v in [
                cam.yaw, cam.pitch, cam.roll, cam.zoom, cam.pan.x, cam.pan.y, ppp,
            ] {
                v.to_bits().hash(&mut h);
            }
            cam.target.map(f32::to_bits).hash(&mut h);
            (cam.perspective, shading, guides).hash(&mut h);
            size.hash(&mut h);
            scene.generation.hash(&mut h);
            // ボディの外観の色と、ボディ・面の材質(網を作り直さずに替わる。材質を鍵に入れないと、
            // 色の同じ外観で粗さだけを直した時に絵が替わらない)
            let look_key = |l: &kmb_render::Look| {
                (
                    l.color,
                    l.finish.metallic.to_bits(),
                    l.finish.roughness.to_bits(),
                    // デカール: 絵と置き方(置き方は面の平面の枠に表れる)
                    l.decal.as_ref().map(|d| {
                        (
                            d.image.key,
                            d.place.opacity.to_bits(),
                            d.frame
                                .map(|f| [f.origin, f.u, f.v].map(|v| v.map(f32::to_bits))),
                        )
                    }),
                )
            };
            scene.bodies.iter().for_each(|b| {
                b.tint.map(|c| c.to_array()).hash(&mut h);
                b.look.as_ref().map(look_key).hash(&mut h);
                b.face_looks
                    .iter()
                    .for_each(|l| l.as_ref().map(look_key).hash(&mut h));
            });
            scene.faded().hash(&mut h);
            scene.lamps.iter().for_each(|l| {
                [l.pos, l.dir, l.power]
                    .map(|v| v.map(f32::to_bits))
                    .hash(&mut h);
                [l.cos_outer, l.cos_inner, l.radius]
                    .map(f32::to_bits)
                    .hash(&mut h);
            });
            scene
                .glow()
                .map(|(s, c, k)| (s, c.to_array(), k))
                .hash(&mut h);
            picked
                .iter()
                .map(|p| (p.feature, p.face.clone()))
                .for_each(|k| k.hash(&mut h));
            hover
                .as_ref()
                .map(|p| (p.feature, p.face.clone()))
                .hash(&mut h);
            [
                colors.background,
                colors.background_low,
                colors.body,
                colors.picked,
                colors.edge,
                colors.line,
            ]
            .map(|c| c.to_array())
            .hash(&mut h);
            scene.effects.hash(&mut h);
            overlay.shape.as_ref().map(|s| s.key).hash(&mut h);
            ghosts.iter().map(Ghost::key).for_each(|k| k.hash(&mut h));
            if let Some(sec) = scene.section {
                sec.hash_into(&mut h);
            }
            (
                settings.samples,
                settings.hidden_alpha.to_bits(),
                settings.edge_px.to_bits(),
            )
                .hash(&mut h);
            h.finish()
        };
        let id = resp.id.with("render");
        let started = std::time::Instant::now();
        let texture = match &gpu {
            // GPU: 毎回描き直しても軽い(網は形が変わった時だけ送る)
            Some(g) => {
                let frame = ui.ctx().cumulative_frame_nr();
                let gid = id.with("gpu");
                let last: Option<(u64, egui::TextureId)> = ui.ctx().data(|d| d.get_temp(gid));
                match last {
                    Some((k, t)) if k == key => t,
                    _ => {
                        let t = match &shape_scene {
                            Some(sc) => {
                                let c = Colors {
                                    body: colors.shape,
                                    ..*colors
                                };
                                g.render(
                                    id,
                                    frame,
                                    sc,
                                    cam,
                                    size,
                                    ppp,
                                    &[],
                                    None,
                                    &c,
                                    shading,
                                    guides,
                                    &[],
                                )
                            }
                            None => g.render(
                                id,
                                frame,
                                scene,
                                cam,
                                size,
                                ppp,
                                picked,
                                hover.as_ref(),
                                colors,
                                shading,
                                guides,
                                ghosts,
                            ),
                        };
                        ui.ctx().data_mut(|d| d.insert_temp(gid, (key, t)));
                        t
                    }
                }
            }
            None => {
                let cached: Option<(u64, egui::TextureHandle)> = ui.ctx().data(|d| d.get_temp(id));
                let texture = match cached {
                    Some((k, t)) if k == key => t,
                    other => {
                        let image = match &shape_scene {
                            Some(sc) => {
                                let c = Colors {
                                    body: colors.shape,
                                    ..*colors
                                };
                                render_guided(
                                    sc,
                                    cam,
                                    size,
                                    ppp,
                                    &[],
                                    None,
                                    &c,
                                    shading,
                                    guides,
                                    &[],
                                )
                            }
                            None => render_guided(
                                scene,
                                cam,
                                size,
                                ppp,
                                picked,
                                hover.as_ref(),
                                colors,
                                shading,
                                guides,
                                ghosts,
                            ),
                        };
                        let t = match other {
                            Some((_, mut t)) => {
                                t.set(image, egui::TextureOptions::LINEAR);
                                t
                            }
                            None => {
                                ui.ctx()
                                    .load_texture("view3d", image, egui::TextureOptions::LINEAR)
                            }
                        };
                        ui.ctx().data_mut(|d| d.insert_temp(id, (key, t.clone())));
                        t
                    }
                };
                texture.id()
            }
        };
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        crate::perf::add(crate::perf::Part::Draw, ms);
        log::trace!(
            "3D のビュー({}): {ms:.2} ms",
            if gpu.is_some() { "GPU" } else { "CPU" },
        );
        painter.image(
            texture,
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
    }
    // 下絵(キャンバス)は形の上に半透明で重ね、ほかの線はその上に描く
    for pic in &overlay.pictures {
        painter.add(picture_shape(ui.ctx(), pic, |p| {
            cam.project(p.map(|v| v as f32), rect).0
        }));
    }
    // ストック、作業範囲、原点、経路は形の上に重ねて描く(奥行きは見ない)
    let edges = |lo: [f32; 3], hi: [f32; 3]| {
        let c = |i: usize| {
            [
                if i & 1 == 0 { lo[0] } else { hi[0] },
                if i & 2 == 0 { lo[1] } else { hi[1] },
                if i & 4 == 0 { lo[2] } else { hi[2] },
            ]
        };
        let mut out = Vec::new();
        for i in 0..8usize {
            for bit in [1, 2, 4] {
                if i & bit == 0 {
                    out.push((c(i), c(i | bit)));
                }
            }
        }
        out
    };
    let f32s = |p: [f64; 3]| p.map(|v| v as f32);
    let boxes = |k: kmb_core::BoxInk| {
        overlay
            .boxes
            .iter()
            .filter(move |b| b.2.ink == k)
            .map(move |b| (f32s(b.0), f32s(b.1)))
    };
    for (lo, hi) in boxes(kmb_interchange::ink::boxes::LIMIT) {
        let color = colors.envelope;
        for (a, b) in edges(lo, hi) {
            let Some((a, b)) = cam.segment(a, b, rect) else {
                continue;
            };
            painter.add(egui::Shape::dashed_line(
                &[a, b],
                egui::Stroke::new(1.0, color),
                6.0,
                4.0,
            ));
        }
    }
    let stock = boxes(kmb_interchange::ink::boxes::WORK).next();
    if let Some((lo, hi)) = stock {
        let color = colors.stock;
        for (a, b) in edges(lo, hi) {
            if let Some((a, b)) = cam.segment(a, b, rect) {
                painter.line_segment([a, b], egui::Stroke::new(1.5, color));
            }
        }
    }
    if let (Some(o), Some((lo, hi))) = (overlay.points.first().map(|p| f32s(*p)), stock) {
        // 原点の軸。長さはストックの大きさに合わせる
        let len = ((hi[0] - lo[0]).max(hi[1] - lo[1]) * 0.2).max(2.0);
        let p0 = cam.project(o, rect).0;
        for (k, color) in colors.inks.origin_axis.into_iter().enumerate() {
            let mut tip = o;
            match &overlay.axes {
                Some(axes) => {
                    for (t, d) in tip.iter_mut().zip(axes[k]) {
                        *t += d as f32 * len;
                    }
                }
                None => tip[k] += len,
            }
            if let Some((a, b)) = cam.segment(o, tip, rect) {
                painter.line_segment([a, b], egui::Stroke::new(2.5, color));
            }
        }
        painter.circle_filled(p0, 3.5, colors.edge);
    }
    for (pts, kind) in &overlay.lines {
        // 1 点だけの線は点として描く(スケッチの単独の点)
        if let [p] = pts.as_slice() {
            let p = f32s(*p);
            if let Some((at, _)) = cam.segment(p, p, rect) {
                let color = match kind.ink {
                    kmb_interchange::ink::line::CONSTRUCTION => colors.sketch,
                    _ => colors.edge,
                };
                painter.circle_filled(at, 2.5, color);
            }
            continue;
        }
        for line in screen_lines(cam, rect, pts) {
            draw_overlay_line(&painter, line, kind.ink, colors);
        }
    }
    // シミュレーションの印: 食い込みは赤、削り残しは紫(残った高さが大きいほど大きく)
    for (p, kind, amount) in &overlay.marks {
        let (color, r) = match *kind {
            kmb_interchange::ink::mark::GOUGE => (colors.inks.gouge, 4.0),
            // 削り残し(知らない印もこの色で描く)
            _ => (
                with_alpha(colors.inks.leftover, 210),
                1.5 + amount.min(2.0) as f32 * 1.5,
            ),
        };
        painter.circle_filled(cam.project(f32s(*p), rect).0, r, color);
    }
    // 加工の高さの面: 半透明の四角と、最初の角に高さの札。札は面の色の地に読みやすい字の色で、
    // 全部の面の後に描き、重ならないよう下へずらす
    let mut tags: Vec<(Rect, std::sync::Arc<egui::Galley>, Color32)> = Vec::new();
    for pl in &overlay.planes {
        let color = colors.inks.height(pl.ink);
        let pts: Vec<Pos2> = pl
            .corners
            .iter()
            .map(|p| cam.project(f32s(*p), rect).0)
            .collect();
        painter.add(egui::Shape::convex_polygon(
            pts.clone(),
            color.gamma_multiply(0.16),
            egui::Stroke::new(1.0, color),
        ));
        let [r8, g8, b8, _] = color.to_array();
        let light = 0.299 * r8 as f32 + 0.587 * g8 as f32 + 0.114 * b8 as f32 > 150.0;
        let ink = if light {
            Color32::BLACK
        } else {
            Color32::WHITE
        };
        let galley = painter.layout_no_wrap(pl.label.clone(), crate::prop(11.5), ink);
        let pad = Vec2::new(4.0, 1.0);
        let size = galley.size() + 2.0 * pad;
        let mut r = Rect::from_min_size(pts[0] + Vec2::new(6.0, -4.0 - size.y), size);
        for _ in 0..overlay.planes.len() {
            match tags.iter().find(|t| t.0.intersects(r)) {
                Some(t) => r = r.translate(Vec2::new(0.0, t.0.bottom() + 2.0 - r.top())),
                None => break,
            }
        }
        tags.push((r, galley, color));
    }
    for (r, galley, color) in tags {
        painter.rect_filled(r, 3.0, color);
        painter.galley(r.min + Vec2::new(4.0, 1.0), galley, Color32::PLACEHOLDER);
    }
    // 空の文書では原点の平面を灰色の半透明の四角と名前で見せる(どこが原点で、どの向きが何の平面か)
    if empty && guides.axes && !controls.no_guide_planes {
        origin_planes(&painter, cam, rect, colors.machine);
    }
    if guides.axes {
        let o = cam.project([0.0; 3], rect).0;
        if rect.contains(o) {
            painter.circle_filled(o, 3.0, colors.edge);
        }
    }
    // 途中の形の名前は、ビューの上の案内(view_ui の ctxhint)に出す
    let (right, up, _) = cam.basis();
    // 軸の印
    let o = rect.left_bottom() + Vec2::new(42.0, -50.0);
    let mark = colors.inks.axis_mark;
    for (axis, color, label) in [
        ([1.0, 0.0, 0.0], mark[0], "X"),
        ([0.0, 1.0, 0.0], mark[1], "Y"),
        ([0.0, 0.0, 1.0], mark[2], "Z"),
    ] {
        let tip = o + Vec2::new(dot(axis, right), -dot(axis, up)) * 20.0;
        painter.line_segment([o, tip], egui::Stroke::new(2.0, color));
        painter.text(
            tip,
            egui::Align2::CENTER_CENTER,
            label,
            crate::prop(11.0),
            color,
        );
    }
    if guides.grid {
        painter.text(
            rect.left_bottom() + Vec2::new(12.0, -8.0),
            egui::Align2::LEFT_BOTTOM,
            grid_label(cam.zoom),
            crate::prop(10.5),
            ui.visuals().weak_text_color(),
        );
    }
    // 回している間だけ、回す中心の印を出す
    if ev.orbiting {
        let c = cam.project(cam.target, rect).0;
        let k = colors.picked;
        painter.circle_stroke(c, 6.0, egui::Stroke::new(1.5, k));
        painter.line_segment(
            [c - Vec2::X * 10.0, c + Vec2::X * 10.0],
            egui::Stroke::new(1.0, k),
        );
        painter.line_segment(
            [c - Vec2::Y * 10.0, c + Vec2::Y * 10.0],
            egui::Stroke::new(1.0, k),
        );
    }
    if let Some((a, b)) = ui.ctx().data(|m| m.get_temp::<(Pos2, Pos2)>(box_id)) {
        draw_select_box(&painter, a, b, colors);
    }
    (ev, resp, rect)
}

/// 囲んでいる範囲を描く(U30。2D スケッチと同じ見分け方)。左から右へ囲む(全部入った物を選ぶ)は
/// 選んだ色の実線、右から左へ囲む(かかった物を選ぶ)はマウスの下の色の破線で、塗りの色も替える。
pub fn draw_select_box(painter: &egui::Painter, a: Pos2, b: Pos2, colors: &Colors) {
    let crossing = b.x < a.x;
    let r = Rect::from_two_pos(a, b);
    let edge = if crossing {
        colors.inks.select_crossing
    } else {
        colors.inks.select_window
    };
    painter.rect_filled(r, 0.0, edge.gamma_multiply(0.10));
    let stroke = egui::Stroke::new(1.5, edge);
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
            "かかった物を選ぶ"
        } else {
            "全部入った物を選ぶ"
        },
        crate::prop(12.0),
        edge,
    );
}

/// 範囲 `area` と三角形 `t`(画面の位置)が重なるか(三角形の点が入る、範囲の角が三角形に入る、
/// 辺どうしが交わる)。
pub fn triangle_touches(area: Rect, t: [Pos2; 3]) -> bool {
    if t.iter().any(|p| area.contains(*p)) {
        return true;
    }
    let corners = [
        area.left_top(),
        area.right_top(),
        area.right_bottom(),
        area.left_bottom(),
    ];
    let side = |a: Pos2, b: Pos2, p: Pos2| (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
    let inside = |p: Pos2| {
        let s = [
            side(t[0], t[1], p),
            side(t[1], t[2], p),
            side(t[2], t[0], p),
        ];
        s.iter().all(|v| *v >= 0.0) || s.iter().all(|v| *v <= 0.0)
    };
    if corners.iter().any(|c| inside(*c)) {
        return true;
    }
    (0..3).any(|i| segment_touches(area, t[i], t[(i + 1) % 3]))
}

/// 範囲 `area` と線分 `a`〜`b`(画面の位置)が重なるか。
pub fn segment_touches(area: Rect, a: Pos2, b: Pos2) -> bool {
    // 範囲で線分を切る(Liang-Barsky)。残りがあれば重なる
    let d = b - a;
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for (p, q) in [
        (-d.x, a.x - area.min.x),
        (d.x, area.max.x - a.x),
        (-d.y, a.y - area.min.y),
        (d.y, area.max.y - a.y),
    ] {
        if p.abs() < 1e-9 {
            if q < 0.0 {
                return false;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
        }
    }
    t0 <= t1
}

/// 重ねる線を 1 本描く(描き方の役割で色と破線を決める)。
fn draw_overlay_line(
    painter: &egui::Painter,
    line: Vec<Pos2>,
    ink: kmb_core::LineInk,
    colors: &Colors,
) {
    match ink {
        kmb_interchange::ink::line::TRAVEL => {
            painter.add(egui::Shape::dashed_line(
                &line,
                egui::Stroke::new(1.0, colors.rapid),
                4.0,
                3.0,
            ));
        }
        kmb_interchange::ink::line::TRAVEL_LOUD => {
            // 全退避は早送りの色で太く、細かい破線にする
            painter.add(egui::Shape::dashed_line(
                &line,
                egui::Stroke::new(2.2, colors.rapid),
                2.5,
                2.0,
            ));
        }
        kmb_interchange::ink::line::FAINT => {
            painter.add(egui::Shape::line(
                line,
                egui::Stroke::new(1.0, colors.guide),
            ));
        }
        kmb_interchange::ink::line::CONSTRUCTION => {
            painter.add(egui::Shape::line(
                line,
                egui::Stroke::new(1.6, colors.sketch),
            ));
        }
        // 入りは少し太く、リードは切削と同じ太さ、つなぎは細い破線(早送りより詰めた破線)
        kmb_interchange::ink::line::APPROACH => {
            painter.add(egui::Shape::line(
                line,
                egui::Stroke::new(1.8, colors.entry),
            ));
        }
        kmb_interchange::ink::line::EASE => {
            painter.add(egui::Shape::line(line, egui::Stroke::new(1.4, colors.lead)));
        }
        kmb_interchange::ink::line::CONNECT => {
            painter.add(egui::Shape::dashed_line(
                &line,
                egui::Stroke::new(1.0, colors.link),
                2.0,
                2.0,
            ));
        }
        kmb_interchange::ink::line::SURROUND => {
            painter.add(egui::Shape::line(
                line,
                egui::Stroke::new(1.0, colors.machine),
            ));
        }
        // 主の線(知らない印もこの線で描く)
        _ => {
            painter.add(egui::Shape::line(line, egui::Stroke::new(1.2, colors.cut)));
        }
    }
}

/// 画面で `THIN` 画素より近い続きの点を間引く(3D の経路は数十万の細かい点になり、そのまま描くと
/// 1 コマに時間がかかってクリックを取りこぼすため)。最後の点は残す。
fn thin(line: Vec<Pos2>) -> Vec<Pos2> {
    const THIN: f32 = 0.75;
    if line.len() < 3 {
        return line;
    }
    let last = line[line.len() - 1];
    let mut out: Vec<Pos2> = Vec::with_capacity(line.len().min(4096));
    for p in line {
        if out
            .last()
            .is_none_or(|q| (p - *q).length_sq() >= THIN * THIN)
        {
            out.push(p);
        }
    }
    if out.last() != Some(&last) {
        out.push(last);
    }
    out
}

/// 折れ線を画面に写す。透視投影で視点に近すぎる所は切り、そこで線を分ける。
fn screen_lines(cam: &Camera, rect: Rect, pts: &[[f64; 3]]) -> Vec<Vec<Pos2>> {
    screen_lines_full(cam, rect, pts)
        .into_iter()
        .map(thin)
        .collect()
}

fn screen_lines_full(cam: &Camera, rect: Rect, pts: &[[f64; 3]]) -> Vec<Vec<Pos2>> {
    let f32s = |p: [f64; 3]| p.map(|v| v as f32);
    if cam.near_limit(rect).is_none() {
        return vec![pts.iter().map(|p| cam.project(f32s(*p), rect).0).collect()];
    }
    let mut out: Vec<Vec<Pos2>> = Vec::new();
    let mut cur: Vec<Pos2> = Vec::new();
    for w in pts.windows(2) {
        match cam.segment(f32s(w[0]), f32s(w[1]), rect) {
            Some((p, q)) => {
                if cur.last() != Some(&p) {
                    if cur.len() > 1 {
                        out.push(std::mem::take(&mut cur));
                    }
                    cur = vec![p];
                }
                cur.push(q);
            }
            None => {
                if cur.len() > 1 {
                    out.push(std::mem::take(&mut cur));
                }
                cur.clear();
            }
        }
    }
    if cur.len() > 1 {
        out.push(cur);
    }
    out
}

/// 下絵の絵を、4 隅の間を格子に割った網に貼った形(透視の時に絵が大きく歪まないように細かく割る)。
/// `to_screen` は世界の座標を画面の位置にする。貼る絵は `key` ごとに覚え、同じ絵を毎フレーム
/// GPU へ送り直さない。スケッチの編集も同じ形で下絵を描く。
pub fn picture_shape(
    ctx: &egui::Context,
    pic: &kmb_core::OverlayPicture,
    to_screen: impl Fn([f64; 3]) -> Pos2,
) -> egui::Shape {
    let id = egui::Id::new(("kmb-picture", pic.key));
    let texture = match ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        Some(t) => t,
        None => {
            let (w, h, px) = &*pic.rgba;
            let image = egui::ColorImage::from_rgba_unmultiplied([*w as usize, *h as usize], px);
            let t = ctx.load_texture(
                format!("picture-{}", pic.key),
                image,
                egui::TextureOptions::LINEAR,
            );
            ctx.data_mut(|d| d.insert_temp(id, t.clone()));
            t
        }
    };
    const N: usize = 12;
    let c = pic.corners;
    let at = |u: f64, v: f64| -> [f64; 3] {
        // 角の並びは左下・右下・右上・左上(絵の下が v = 0)
        let mut out = [0.0; 3];
        for (i, o) in out.iter_mut().enumerate() {
            let bottom = c[0][i] + (c[1][i] - c[0][i]) * u;
            let top = c[3][i] + (c[2][i] - c[3][i]) * u;
            *o = bottom + (top - bottom) * v;
        }
        out
    };
    let tint = Color32::from_white_alpha((pic.opacity.clamp(0.0, 1.0) * 255.0).round() as u8);
    let mut mesh = egui::Mesh::with_texture(texture.id());
    for j in 0..=N {
        for i in 0..=N {
            let (u, v) = (i as f64 / N as f64, j as f64 / N as f64);
            mesh.vertices.push(egui::epaint::Vertex {
                pos: to_screen(at(u, v)),
                uv: Pos2::new(u as f32, 1.0 - v as f32),
                color: tint,
            });
        }
    }
    for j in 0..N {
        for i in 0..N {
            let a = (j * (N + 1) + i) as u32;
            let d = a + (N + 1) as u32;
            mesh.add_triangle(a, a + 1, d + 1);
            mesh.add_triangle(a, d + 1, d);
        }
    }
    egui::Shape::mesh(mesh)
}
