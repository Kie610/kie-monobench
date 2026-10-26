//! 2D スケッチを 3D の視点で見る(N1b)と、正面から見るに戻す(N1c)。

use crate::Tool;
use crate::tests::{click_px, current_sketch, editor, empty_sketch, screen, solved};
use crate::view::{Transform, space_camera};
use egui_kittest::Harness;
use kmb_ui::Workspace;

/// 右ボタンで画面の位置から位置へ引く(3D の視点へ回す)。
fn right_drag(h: &mut Harness<'static, Workspace>, a: egui::Pos2, b: egui::Pos2) {
    h.hover_at(a);
    h.run();
    h.event(egui::Event::PointerButton {
        pos: a,
        button: egui::PointerButton::Secondary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
    for k in 1..=10 {
        h.event(egui::Event::PointerMoved(a + (b - a) * (k as f32 / 10.0)));
        h.run();
    }
    h.event(egui::Event::PointerButton {
        pos: b,
        button: egui::PointerButton::Secondary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
}

fn last(h: &Harness<'static, Workspace>) -> Transform {
    h.state()
        .editor::<crate::SketchEditor>()
        .expect("組み込んである")
        .view
        .last
        .clone()
        .expect("描いた後")
}

#[test]
fn the_plane_point_under_the_cursor_comes_back_in_a_turned_perspective_view() {
    let rect = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(900.0, 600.0));
    let plane = kmb_core::Plane::ZX.offset(3.0);
    let flat = Transform::flat(egui::pos2(5.0, -2.0), rect.center(), 6.0);
    let mut cam = space_camera(&flat, rect, plane);
    // 正面から見た 3D の視点は、平らな図と同じ所に同じ大きさで写す
    for p in [[0.0, 0.0], [12.5, -4.0], [-30.0, 18.0]] {
        let t = Transform::spatial(cam, rect, plane);
        let (a, b) = (flat.to_screen(p), t.to_screen(p));
        assert!(
            (a - b).length() < 0.05,
            "正面の 3D の視点 {b:?} と平らな図 {a:?}"
        );
    }
    cam.orbit(egui::vec2(70.0, -45.0));
    cam.perspective = true;
    let t = Transform::spatial(cam, rect, plane);
    for p in [[0.0, 0.0], [12.5, -4.0], [-30.0, 18.0]] {
        let w = t.to_world(t.to_screen(p));
        assert!(
            (w[0] - p[0]).abs() < 1e-3 && (w[1] - p[1]).abs() < 1e-3,
            "視線と平面の交わる所 {w:?} が元の点 {p:?} に戻らない"
        );
    }
}

#[test]
fn right_drag_turns_to_3d_where_clicks_still_draw_on_the_plane_and_look_front_returns() {
    let mut h = empty_sketch();
    let before = screen(&h, [10.0, 5.0]);
    let c = last(&h).center_screen;
    right_drag(&mut h, c, c + egui::vec2(90.0, 50.0));
    assert!(
        editor(h.state_mut()).view.space.is_some(),
        "右ドラッグで 3D の視点にならない"
    );
    assert!(
        editor(h.state_mut()).editor.in_space,
        "パレットの「正面から見る」が押せない"
    );
    // 回したままクリックで線を描く: 置いた点は視線と平面の交わる所
    editor(h.state_mut()).set_tool(Tool::Line);
    h.run();
    let (a, b) = (c + egui::vec2(-60.0, 20.0), c + egui::vec2(80.0, -30.0));
    let t = last(&h);
    assert!(t.space.is_some());
    click_px(&mut h, a);
    click_px(&mut h, b);
    let s = current_sketch(h.state());
    let sol = solved(h.state());
    let mut pts: Vec<[f64; 2]> = sol.points.values().copied().collect();
    pts.sort_by(|p, q| p[0].total_cmp(&q[0]));
    assert_eq!(pts.len(), 2, "線の両端が置かれていない: {s:?}");
    for (p, at) in pts.iter().zip([a, b]) {
        let back = t.to_screen(*p);
        assert!(
            (back - at).length() < 1.0,
            "置いた点 {p:?} がクリックした所 {at:?} に写らない({back:?})"
        );
    }
    // 正面から見る: 正面へ回し終えたら平らな図に戻り、回す前と同じ所に見える
    editor(h.state_mut()).set_tool(Tool::Select);
    editor(h.state_mut()).editor.look_front = true;
    for _ in 0..40 {
        h.step();
        if editor(h.state_mut()).view.space.is_none() {
            break;
        }
    }
    h.run();
    assert!(
        editor(h.state_mut()).view.space.is_none(),
        "正面から見るで平らな図に戻らない"
    );
    assert!(!editor(h.state_mut()).editor.in_space);
    // 戻った平らな図は、描いた形全体が収まる「すべて表示」と同じ。
    // 回し終えた所で跳ねないよう、回す間に回す中心と倍率も移している
    let v = &editor(h.state_mut()).view;
    assert!(
        (v.zoom - 1.0).abs() < 1e-3 && v.pan.length() < 0.5,
        "形全体が収まる表示になっていない: 倍率 {}、ずらし {:?}",
        v.zoom,
        v.pan
    );
    // 右クリックの「すべて表示」と同じ所に写る(回す前の見え方は残さない)
    let shown: Vec<egui::Pos2> = pts.iter().map(|p| screen(&h, *p)).collect();
    editor(h.state_mut()).view.fit();
    h.run();
    for (p, at) in pts.iter().zip(&shown) {
        let fit = screen(&h, *p);
        assert!(
            (fit - *at).length() < 0.5,
            "{p:?} が「すべて表示」の {fit:?} でなく {at:?} に写る(回す前は {before:?})"
        );
    }
}

#[test]
fn look_front_in_the_flat_view_fits_the_whole_sketch() {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Line);
    h.run();
    let c = last(&h).center_screen;
    click_px(&mut h, c + egui::vec2(-40.0, 10.0));
    click_px(&mut h, c + egui::vec2(60.0, -20.0));
    editor(h.state_mut()).set_tool(Tool::Select);
    // 拡大して横へずらし、形を画面の外へ出す
    {
        let v = &mut editor(h.state_mut()).view;
        v.zoom = 6.0;
        v.pan = egui::vec2(700.0, 400.0);
    }
    h.run();
    editor(h.state_mut()).editor.look_front = true;
    h.run();
    let v = &editor(h.state_mut()).view;
    assert!(v.space.is_none(), "平らな図のまま合わせる");
    assert!(
        (v.zoom - 1.0).abs() < 1e-6 && v.pan.length() < 1e-3,
        "平らな図で正面から見るを押しても全体が収まらない: 倍率 {}、ずらし {:?}",
        v.zoom,
        v.pan
    );
}

#[test]
fn fit_in_3d_keeps_the_turned_view() {
    let mut h = empty_sketch();
    let c = last(&h).center_screen;
    right_drag(&mut h, c, c + egui::vec2(-80.0, 60.0));
    let yaw = editor(h.state_mut()).view.space.expect("3D の視点").yaw;
    editor(h.state_mut()).view.fit();
    h.run();
    let cam = editor(h.state_mut())
        .view
        .space
        .expect("すべて表示でも 3D の視点のまま");
    assert!((cam.yaw - yaw).abs() < 1e-6, "すべて表示で向きが変わった");
}

/// 3D のビューの今の向き(ヨー・ピッチ・傾き、度)。名前を付けた視点に残して読む。
fn view_angles(h: &mut Harness<'static, Workspace>) -> [f64; 3] {
    let id = h.state_mut().save_named_view("今").expect("残せる");
    let v = h.state().document().data().named_view(id).unwrap().clone();
    [v.yaw, v.pitch, v.roll]
}

/// スケッチで最後に見ていた向き(ヨー・ピッチ・傾き、度)。
fn sketch_angles(h: &Harness<'static, Workspace>) -> [f64; 3] {
    let (cam, rect) = h
        .state()
        .editor::<crate::SketchEditor>()
        .unwrap()
        .view
        .seen
        .expect("描いた後");
    let (_, yaw, pitch, _) = cam.snapshot(rect);
    [yaw, pitch, cam.tilt_deg()].map(f64::from)
}

fn same_angles(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter()
        .zip(&b)
        .all(|(x, y)| ((x - y + 180.0).rem_euclid(360.0) - 180.0).abs() < 0.5)
}

/// スケッチを終えても、スケッチで見ていた向きのまま 3D のビューへ戻る(10-23 u1)。平らな図で
/// 終えればスケッチの平面を正面から見た向き、右ドラッグで回した後ならその向き。
#[test]
fn finishing_a_sketch_keeps_the_view_it_was_seen_from() {
    // 平らな図で終える: 3D のビューは入る前の向き(等角)でなく、平面を正面から見る向きになる
    let mut h = empty_sketch();
    let seen = sketch_angles(&h);
    h.state_mut().finish_sketch();
    h.run();
    let after = view_angles(&mut h);
    assert!(
        same_angles(after, seen),
        "平らな図の向き {seen:?} で戻る: {after:?}"
    );
    assert!(
        (after[1].abs() - 90.0).abs() < 0.5,
        "XY 平面を真上から: {after:?}"
    );

    // 右ドラッグで回してから終える: その向きのまま
    let mut h = empty_sketch();
    let c = last(&h).center_screen;
    right_drag(&mut h, c, c + egui::vec2(90.0, 50.0));
    let seen = sketch_angles(&h);
    h.state_mut().finish_sketch();
    h.run();
    let after = view_angles(&mut h);
    assert!(
        same_angles(after, seen),
        "回した向き {seen:?} で戻る: {after:?}"
    );
}

/// 平らな図(正面の図)でも、前にできたボディを線の下に描く(表示と非表示の決まり 6)。前は 3D の視点の
/// 時だけボディを用意し、平らな図では置いた面の輪郭だけが出た。
#[test]
fn the_flat_view_draws_the_bodies_before_the_sketch() {
    let part = kmb_samples::reference_part();
    let mut ws = crate::tests::ws_of(part.doc, None, crate::tests::reg());
    ws.open_edit(part.top_sketch);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h.run();
    let ed = editor(h.state_mut());
    assert!(ed.view.space.is_none(), "平らな図で開いていない");
    assert!(
        ed.scene.key().iter().any(|(f, _)| *f == part.plate),
        "平らな図で板のボディを描かない: {:?}",
        ed.scene.key()
    );
}
