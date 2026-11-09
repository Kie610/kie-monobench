//! 自由な形(投げ縄)で囲んで選ぶ。四角の窓と同じ判定の型で、交差して選ぶ・中だけ選ぶを決める。

use crate::tests::reg;
use crate::*;
use egui::{Pos2, Rect, pos2, vec2};
use egui_kittest::Harness;
use view3d::Area;

fn sorted(mut v: Vec<view3d::Pick>) -> Vec<view3d::Pick> {
    v.sort_by(|a, b| (a.feature.0, &a.face).cmp(&(b.feature.0, &b.face)));
    v
}

#[test]
fn a_lasso_in_the_shape_of_a_window_picks_the_same_and_a_cut_corner_picks_less() {
    let part = kmb_samples::reference_part();
    let ws = Workspace::new(part.doc, None, reg());
    let rect = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
    let mut cam = ws.cur.camera;
    cam.fit(&ws.cur.scene, rect);
    let half = Rect::from_min_max(rect.min, pos2(400.0, 600.0));
    let window = Area::from(half);
    let square = Area::lasso(&[
        half.left_top(),
        half.right_top(),
        half.right_bottom(),
        half.left_bottom(),
    ])
    .unwrap();
    let scene = &ws.cur.scene;
    for crossing in [false, true] {
        assert_eq!(
            sorted(view3d::picks_in(scene, &cam, rect, &window, crossing)),
            sorted(view3d::picks_in(scene, &cam, rect, &square, crossing)),
            "同じ四角を多角形で囲んでも同じ物を選ぶ(交差 {crossing})"
        );
        assert_eq!(
            view3d::bodies_in(scene, &cam, rect, &window, crossing),
            view3d::bodies_in(scene, &cam, rect, &square, crossing),
        );
    }
    // 右上の角を切り落とした三角形: かかった面は四角の窓のかかった面に含まれる
    let corner = Area::lasso(&[half.left_top(), half.right_bottom(), half.left_bottom()]).unwrap();
    let wide = view3d::picks_in(scene, &cam, rect, &window, true);
    let cut = view3d::picks_in(scene, &cam, rect, &corner, true);
    assert!(!cut.is_empty());
    assert!(cut.iter().all(|f| wide.contains(f)));
    // 全部入った物も同じく含まれる
    let inner = view3d::picks_in(scene, &cam, rect, &corner, false);
    assert!(inner.iter().all(|f| cut.contains(f)));
}

/// ビューの中を `points` の順になぞる(押して、点を順に通って、最後の点で離す)。
fn trace(h: &mut Harness<'_, Workspace>, points: &[Pos2]) {
    h.drag_at(points[0]);
    h.run();
    for p in &points[1..] {
        h.hover_at(*p);
        h.run();
    }
    h.drop_at(*points.last().unwrap());
    h.run();
}

fn ui_request(h: &mut Harness<'_, Workspace>, tool: &str, args: serde_json::Value) {
    let crate::live_ui::Answer::AfterScript(batch) =
        h.state_mut().live_request(tool, &args).unwrap()
    else {
        panic!("台本で操作する");
    };
    let started = std::time::Instant::now();
    loop {
        h.step();
        if let Some(result) = h.state_mut().script_result(batch) {
            assert!(result.ok, "{}", result.report());
            break;
        }
        assert!(started.elapsed().as_secs() < 60, "{tool} が終わりません");
    }
}

#[test]
fn choosing_freeform_in_the_select_menu_lets_a_traced_path_pick_faces() {
    let part = kmb_samples::reference_part();
    let mut ws = Workspace::new(part.doc, None, reg());
    ws.win.layout.appearance.language = "ja".into();
    let mut h = Harness::builder()
        .with_size(vec2(1400.0, 900.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    // 選択のメニューから「自由な形で囲んで選ぶ」を押す
    let select = h.state().win.t("toolbar-group-select");
    ui_request(
        &mut h,
        "ui_click",
        serde_json::json!({"target": format!("{select} ▾")}),
    );
    let lasso = h.state().win.t("toolbar-enclose-lasso");
    ui_request(&mut h, "ui_click", serde_json::json!({"target": lasso}));
    assert_eq!(h.state().win.enclose, view3d::Enclose::Lasso);
    h.run();
    let rect = h.state().win.view_rect;
    let c = rect.center();
    // 右へ動かしてから回る菱形: 全部入った面だけを選ぶ
    let r = 0.45 * rect.height();
    let diamond = [
        c - vec2(r, 0.0),
        c - vec2(0.0, r),
        c + vec2(r, 0.0),
        c + vec2(0.0, r),
        c - vec2(r * 0.95, 0.0),
    ];
    trace(&mut h, &diamond);
    let cam = h.state().cur.camera;
    let want = view3d::picks_in(
        &h.state().cur.scene,
        &cam,
        rect,
        &Area::lasso(&diamond).unwrap(),
        false,
    );
    assert!(!want.is_empty(), "菱形に全部入る面がある");
    assert_eq!(sorted(h.state().picked_faces()), sorted(want));
    // 左へ動かしてから回る小さな輪: かかった面を選ぶ(四角の窓と同じ覚え方)
    let s = 0.08 * rect.height();
    let ring = [
        c,
        c - vec2(s, 0.0),
        c - vec2(s, s),
        c - vec2(0.0, s),
        c + vec2(1.0, 0.0),
    ];
    trace(&mut h, &ring);
    let cam = h.state().cur.camera;
    let want = view3d::picks_in(
        &h.state().cur.scene,
        &cam,
        rect,
        &Area::lasso(&ring).unwrap(),
        true,
    );
    assert!(!want.is_empty(), "中心の輪にかかる面がある");
    assert_eq!(sorted(h.state().picked_faces()), sorted(want));
    // 四角の窓へ戻せる
    ui_request(
        &mut h,
        "ui_click",
        serde_json::json!({"target": format!("{select} ▾")}),
    );
    let window = h.state().win.t("toolbar-enclose-window");
    ui_request(&mut h, "ui_click", serde_json::json!({"target": window}));
    assert_eq!(h.state().win.enclose, view3d::Enclose::Window);
}
