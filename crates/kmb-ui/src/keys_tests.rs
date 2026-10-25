//! 1 文字のキー、カーソルの所のコマンド検索、正面から見るを、
//! 人の操作(キーとクリック)で通して試す。

use crate::Workspace;
use crate::commands_ui::CommandId;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kmb_cad_extrude::{EXTRUDE, Extrude};
use kmb_core::FeatureId;
use kmb_samples::sample::{DocBuilder, SketchBuilder};
use kmb_sketch::SketchPlane;

/// 30×20×10 の板を開いた画面と、板の押し出しの番号。
fn plate() -> (Harness<'static, Workspace>, FeatureId) {
    let mut b = DocBuilder::new();
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let o = s.fixed_point(0.0, 0.0);
    s.rectangle(o, "30 mm", "20 mm", [0.0, 0.0, 30.0, 20.0]);
    let sk = b.feature(kmb_sketch::SKETCH, "板のスケッチ", s.build());
    let ex = b.feature(
        EXTRUDE,
        "板",
        Extrude {
            profile: Some(sk),
            distance: "10 mm".into(),
            ..Default::default()
        },
    );
    let mut ws = Workspace::new(b.finish("板"), None, kmb_bundle::modules());
    ws.wait();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h.state_mut().clear_selection();
    h.run();
    (h, ex)
}

/// 表示している板の面のうち、外向きの法線が `n` の面。
fn face_toward(ws: &Workspace, n: [f32; 3]) -> kmb_core::FacePick {
    let b = &ws.cur.scene.bodies[0];
    let face = b
        .mesh
        .face_names
        .iter()
        .find(|f| {
            crate::view3d::face_normal(&ws.cur.scene, f)
                .is_some_and(|m| (0..3).all(|i| (m[i] - n[i]).abs() < 1e-3))
        })
        .expect("その向きの面がある")
        .clone();
    kmb_core::FacePick {
        feature: b.feature,
        face,
    }
}

#[test]
fn i_measures_m_moves_and_s_opens_the_search_at_the_cursor() {
    let (mut h, _) = plate();
    // I は計測。もう一度押すと閉じる
    h.key_press(egui::Key::I);
    h.run();
    assert!(h.state().cur.measure.is_some(), "I で測る");
    h.key_press(egui::Key::I);
    h.run();
    assert!(h.state().cur.measure.is_none(), "I をもう一度で閉じる");
    // M は移動/コピーを始める(計測ではない。Kie 2026-10-22)。Esc でやめる
    h.key_press(egui::Key::M);
    h.run();
    assert!(h.state().cur.measure.is_none(), "M では測らない");
    let id = h.state().creating().expect("M で移動/コピーを始める");
    let f = h.state().document().data().feature(id).unwrap();
    assert_eq!(f.type_id, kmb_cad_modify::MOVE);
    h.key_press(egui::Key::Escape);
    h.run();
    // S はカーソルの所に検索の窓を出す(Ctrl+K は画面の上の真ん中)
    let at = egui::pos2(600.0, 380.0);
    h.input_mut().events.push(egui::Event::PointerMoved(at));
    h.run();
    h.key_press(egui::Key::S);
    h.run();
    assert!(h.state().is_command_palette_open());
    assert_eq!(h.state().search.at, Some(at));
    // 欄がカーソルの近くに出る(3D のビューの真ん中なので、ほかの欄は近くにない)
    let near = h
        .get_all_by_role(egui::accesskit::Role::TextInput)
        .map(|n| n.rect())
        .any(|r| (r.left() - at.x).abs() < 60.0 && (r.top() - at.y).abs() < 60.0);
    assert!(near, "欄がカーソルの近くに出る");
    // 探した行の右に、ツールバーの場所とキーを出す
    h.state_mut().search.text = Some("おしだし".into());
    h.run();
    let ws = h.state();
    let c = ws.find_commands("おしだし");
    assert_eq!(
        c[0].id,
        CommandId::Action(kmb_cad_extrude::EXTRUDE_SKETCH.into())
    );
    assert_eq!(
        ws.place_of(&c[0].id, &ws.ribbon_places()),
        "ソリッド › 作成"
    );
    assert_eq!(ws.key_of(&c[0].id), "E");
    assert_eq!(ws.key_of(&CommandId::Measure), "I");
    assert!(h.get_all_by_label_contains("ソリッド › 作成   E").count() == 1);
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(!h.state().is_command_palette_open(), "Esc で閉じる");
    // 別の所でもう一度開くと、その所に出る(前に開いた所を覚えない)
    let at = egui::pos2(420.0, 250.0);
    h.input_mut().events.push(egui::Event::PointerMoved(at));
    h.run();
    h.key_press(egui::Key::S);
    h.run();
    let near = h
        .get_all_by_role(egui::accesskit::Role::TextInput)
        .map(|n| n.rect())
        .any(|r| (r.left() - at.x).abs() < 60.0 && (r.top() - at.y).abs() < 60.0);
    assert!(near, "2 度目も欄がカーソルの近くに出る");
    h.key_press(egui::Key::Escape);
    h.run();
    h.key_press(egui::Key::S);
    h.run();
    h.key_press(egui::Key::S);
    h.run();
    assert!(
        h.state().is_command_palette_open(),
        "欄に打っている間の S は文字になる(閉じない)"
    );
}

#[test]
fn look_at_turns_to_the_picked_face_and_v_hides_the_picked_body() {
    // 1 つの板の画面で順に確かめる:
    // - 「正面」は選んだ面へ滑らかに向き、形全体の真ん中を回す中心にして全体を収める
    //   (元 look_at_turns_smoothly_to_face_the_picked_face)
    // - V で選んだボディを隠し、もう一度 V で出す(元 v_hides_the_picked_body_and_shows_it_again)
    let (mut h, ex) = plate();
    h.state_mut().cur.camera.set_view(-60.0, 30.0);
    h.run();
    // 面を選ぶ前は理由を出すだけ
    h.get_by_label("正面").click();
    h.run();
    assert!(h.state().cur.camera.is_view(-60.0, 30.0));
    assert!(
        h.state()
            .message()
            .is_some_and(|m| m.contains("面・スケッチ・原点の平面を選んで"))
    );
    // 前の面(外向きが -Y)を選んで「正面」
    let front = face_toward(h.state(), [0.0, -1.0, 0.0]);
    // 回し終えた時の中心は、ボディとスケッチの線の全体の箱の真ん中
    let (mut lo, mut hi) = h.state().cur.scene.bounds();
    for p in h.state().sketch_lines().iter().flat_map(|(l, _)| l) {
        for i in 0..3 {
            lo[i] = lo[i].min(p[i] as f32);
            hi[i] = hi[i].max(p[i] as f32);
        }
    }
    let center = [0, 1, 2].map(|i| (lo[i] + hi[i]) / 2.0);
    h.state_mut().set_picked(Some(front));
    h.run();
    h.get_by_label("正面").click();
    h.step();
    h.step();
    assert!(
        h.state().cur.camera.turning(),
        "すぐには替わらず、滑らかに回る"
    );
    assert!(
        h.state().cur.camera.is_view(-90.0, 0.0),
        "行き先で比べるので、途中でも正面の向きと分かる"
    );
    h.run();
    let cam = h.state().cur.camera;
    assert!(!cam.turning(), "0.3 秒で終わる");
    assert!(
        cam.yaw == (-90f32).to_radians() && cam.pitch == 0.0,
        "行き先ちょうど"
    );
    assert!(
        (0..3).all(|i| (cam.target[i] - center[i]).abs() < 1e-3),
        "形全体の真ん中を回す中心にする: {:?} {center:?}",
        cam.target
    );
    // コマンド検索でも探せる
    let c = h.state().find_commands("look at");
    assert_eq!(c[0].id, CommandId::LookAt);
    assert!(c[0].disabled.is_none());

    // V: 選びを外してから(選んでいなければ理由を出す)
    h.state_mut().clear_selection();
    h.state_mut().set_picked(None);
    h.run();
    h.key_press(egui::Key::V);
    h.run();
    assert!(
        h.state()
            .message()
            .is_some_and(|m| m.contains("選んでください")),
        "選んでいなければ理由"
    );
    let top = face_toward(h.state(), [0.0, 0.0, 1.0]);
    h.state_mut().set_picked(Some(top));
    h.key_press(egui::Key::V);
    h.run();
    assert!(h.state().is_hidden(ex), "V で隠す");
    // ブラウザで選び直して V で出す
    h.state_mut().select(ex);
    h.key_press(egui::Key::V);
    h.run();
    assert!(!h.state().is_hidden(ex), "もう一度 V で出す");
}
