//! 面の上のスケッチの原点(まとめてレビューの R2)を、人の操作の順で確かめる: 面を選んで
//! 「面にスケッチ」を押すと面の左下の角が原点になり、パネルの「原点」で替えても描いた形は動かない。

use crate::tests::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kmb_core::{FeatureStatus, Payload};
use kmb_sketch::{FaceOrigin, SketchPlane};
use kmb_ui::Workspace;

/// 基準部品の板(x 0〜60、y 0〜40、厚さ 10)の上面を選び、「面にスケッチ」を押した作業場。
fn sketch_on_plate_top() -> Harness<'static, Workspace> {
    let part = kmb_samples::reference_part();
    let plate = part.plate;
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .build_ui_state(
            |ui, ws: &mut Workspace| ws.ui(ui),
            ws_of(part.doc, None, reg()),
        );
    h.run();
    let ws = h.state_mut();
    ws.set_picked(Some(kmb_ui::Pick {
        feature: plate,
        face: format!("f{}:end", plate.0),
    }));
    ws.run_action(kmb_sketch::SKETCH_ON_FACE);
    h.run();
    h.run();
    h
}

/// 開いているスケッチの平面(計算の結果)。
fn plane_now(ws: &Workspace) -> kmb_core::Plane {
    let id = ws.selected().unwrap();
    match ws.report().feature(id) {
        Some(FeatureStatus::Ok { outputs, .. }) => {
            kmb_core::Points::from_value(&outputs["points"])
                .unwrap()
                .plane
        }
        other => panic!("{other:?}"),
    }
}

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() < 1e-3)
}

#[test]
fn a_new_sketch_on_a_face_puts_its_origin_at_the_lower_left_corner() {
    let h = sketch_on_plate_top();
    let ws = h.state();
    let s = current_sketch(ws);
    assert!(
        matches!(
            &s.plane,
            SketchPlane::Face {
                origin: FaceOrigin::Vertex { .. },
                ..
            }
        ),
        "{:?}",
        s.plane
    );
    let p = plane_now(ws);
    assert!(near(p.origin, [0.0, 0.0, 10.0]), "{:?}", p.origin);
    assert!(
        h.query_by_value("左下の角").is_some(),
        "パネルの原点の欄に出る"
    );
    // 欄の一覧から右下の角を選ぶと、そこが原点になる
    let mut h = h;
    h.get_by_value("左下の角").click();
    h.run();
    h.get_by_label("右下の角").click();
    h.run();
    h.run();
    let p = plane_now(h.state());
    assert!(near(p.origin, [60.0, 0.0, 10.0]), "{:?}", p.origin);
    // 世界の原点を写した点(古いスケッチと同じ)にも戻せる
    h.get_by_value("右下の角").click();
    h.run();
    h.get_by_label("世界の原点を写した点").click();
    h.run();
    h.run();
    assert!(matches!(
        &current_sketch(h.state()).plane,
        SketchPlane::Face {
            origin: FaceOrigin::World,
            ..
        }
    ));
    assert!(near(plane_now(h.state()).origin, [0.0, 0.0, 10.0]));
}

#[test]
fn clicking_another_corner_moves_the_origin_but_not_the_drawing() {
    let mut h = sketch_on_plate_top();
    editor(h.state_mut()).set_tool(crate::Tool::Point);
    h.run();
    click_at(&mut h, [20.0, 10.0]);
    let world = |h: &Harness<'static, Workspace>| {
        let s = current_sketch(h.state());
        // 面の縁を写した点(K7)は除く
        let (projected, _) = s.projected_parts();
        let p = s
            .entities
            .iter()
            .find_map(|e| match e {
                kmb_sketch::Entity::Point { id, x, y } if !projected.contains(id) => Some([*x, *y]),
                _ => None,
            })
            .expect("打った点");
        plane_now(h.state()).to_world(p)
    };
    let before = world(&h);
    let on_screen = screen(&h, [20.0, 10.0]);
    assert!(near(before, [20.0, 10.0, 10.0]), "{before:?}");
    // パネルの「角をクリック…」を押し、図の中で右上の角(60, 40)をクリックする
    h.get_by_label("角をクリック…").click();
    h.run();
    assert_eq!(editor(h.state_mut()).editor.tool, crate::Tool::Origin);
    click_at(&mut h, [60.0, 40.0]);
    h.run();
    let p = plane_now(h.state());
    assert!(near(p.origin, [60.0, 40.0, 10.0]), "{:?}", p.origin);
    assert!(near(world(&h), before), "描いた点は動かない");
    // 画面の上でも跳ねない(点の座標は (20 - 60, 10 - 40) になる)
    let d = screen(&h, [-40.0, -30.0]) - on_screen;
    assert!(d.length() < 1.0, "{d:?}");
    assert_eq!(editor(h.state_mut()).editor.tool, crate::Tool::Select);
    assert!(h.query_by_value("右上の角").is_some());
    // 取り消すと左下に戻る
    h.state_mut().undo();
    h.run();
    assert!(near(plane_now(h.state()).origin, [0.0, 0.0, 10.0]));
}
