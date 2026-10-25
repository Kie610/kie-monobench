//! ジョイントの範囲の試験: 人の操作の順で、ジョイントを直す欄に範囲の外の
//! 値を打つと赤くして受け付けず、範囲の中なら入る。欄の下のスライダを引いて離すと戻る位置に戻る。
//! ▶ で範囲の端から端まで動かして見せ、■ で止めると元の位置に戻る(形と値は変えない)。

use crate::*;
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{Queryable, by};
use jc_cad_extrude::{EXTRUDE, Extrude};
use jc_samples::sample::{DocBuilder, SketchBuilder};
use jc_sketch::SketchPlane;

/// 箱(30×20×10)と、x = 40 の所の蓋(30×20×2)。蓋は奥の上の辺の蝶番で箱につなぎ、-120〜0 度で今 -10 度。
fn hinged() -> (Workspace, FeatureId) {
    let mut b = DocBuilder::new();
    let mut ids = Vec::new();
    for (x, t, name) in [(0.0, "10 mm", "箱"), (40.0, "2 mm", "蓋")] {
        let mut s = SketchBuilder::new(SketchPlane::Xy);
        let o = s.fixed_point(x, 0.0);
        s.rectangle(o, "30 mm", "20 mm", [x, 0.0, x + 30.0, 20.0]);
        let sk = b.feature(jc_sketch::SKETCH, &format!("{name}のスケッチ"), s.build());
        ids.push(b.feature(
            EXTRUDE,
            name,
            Extrude {
                profile: Some(sk),
                distance: t.into(),
                ..Default::default()
            },
        ));
    }
    let (boxed, lid) = (ids[0], ids[1]);
    b.feature(
        "asm.joint@1",
        "ジョイント1",
        serde_json::json!({
            "kind": "revolute",
            "moving": {"kind": "body", "root": lid.0},
            "base": {"kind": "body", "root": boxed.0},
            "bodies": [lid.0],
            "origin": [40.0, 20.0, 2.0],
            "axis": [1.0, 0.0, 0.0],
            "angle": "-10 deg",
            "angle_min": "-120 deg",
            "angle_max": "0 deg",
        }),
    );
    let mut ws = Workspace::new(b.finish("蓋つきの箱"), None, jc_bundle::modules());
    ws.wait();
    let joint = ws.cur.work.doc().data().features.last().unwrap().id;
    (ws, joint)
}

fn angle(h: &Harness<'_, Workspace>, joint: FeatureId) -> String {
    h.state().cur.work.doc().data().feature(joint).unwrap().data["angle"]
        .as_str()
        .unwrap_or("")
        .to_string()
}

/// 角度の欄(今の値 `now` の入った欄)を全部選んで `text` を打ち、Enter。
fn type_angle(h: &mut Harness<'_, Workspace>, now: &str, text: &str) {
    h.get(by().role(Role::TextInput).value(now)).focus();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.event(egui::Event::Text(text.into()));
    h.run_steps(2);
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
}

#[test]
fn out_of_range_values_are_refused_and_rest_and_preview_work() {
    let (mut ws, joint) = hinged();
    ws.select(joint);
    ws.cur.editing = Some(joint);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run_steps(3);

    // 人の手順 1: 角度に範囲の外の 120 deg を打って Enter → 入らず、赤い「範囲外」(理由は吹き出し)を出し、直すのは続く
    type_angle(&mut h, "-10 deg", "120 deg");
    assert_eq!(angle(&h, joint), "-10 deg");
    assert!(h.query_by_label("範囲外").is_some(), "範囲の外の印が出る");
    let key = crate::doc_id(&h.ctx, ("field", joint, "angle"));
    let why = crate::feature_ui::refused(&h.ctx, key);
    assert_eq!(
        why.as_deref(),
        Some("120 deg は範囲 -120 deg〜0 deg の外なので入れられません")
    );
    assert!(
        h.state().is_editing(joint),
        "受け付けない Enter では決めない"
    );

    // 2: 範囲の中の -30 deg なら入り、理由は消える
    type_angle(&mut h, "-10 deg", "-30 deg");
    assert_eq!(angle(&h, joint), "-30 deg");
    assert!(
        h.query_by_label("範囲外").is_none(),
        "ほかの値を入れると印は消える"
    );
    assert_eq!(
        h.state()
            .cur
            .work
            .report()
            .feature(joint)
            .map(|s| matches!(s, FeatureStatus::Ok { .. })),
        Some(true)
    );

    // 3: 戻る位置を -90 deg にする
    let mut data = h
        .state()
        .cur
        .work
        .doc()
        .data()
        .feature(joint)
        .unwrap()
        .data
        .clone();
    data["angle_rest"] = "-90 deg".into();
    h.state_mut().set_feature_data(joint, data);
    h.state_mut().select(joint);
    h.state_mut().cur.editing = Some(joint);
    h.run_steps(3);

    // 4: 欄の下のスライダを引いて離すと、戻る位置 -90 deg に戻る
    let slider = h.get_by_role(Role::Slider).rect();
    let (y, x0, x1) = (slider.center().y, slider.left() + 2.0, slider.right() - 2.0);
    for e in [
        egui::Event::PointerMoved(egui::pos2(x1, y)),
        egui::Event::PointerButton {
            pos: egui::pos2(x1, y),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        },
    ] {
        h.event(e);
        h.step();
    }
    for x in [x1 - 40.0, (x0 + x1) / 2.0, x0 + 20.0] {
        h.event(egui::Event::PointerMoved(egui::pos2(x, y)));
        h.step();
    }
    assert_ne!(angle(&h, joint), "-90 deg", "引いている間は動く");
    h.event(egui::Event::PointerButton {
        pos: egui::pos2(x0 + 20.0, y),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(3);
    assert_eq!(angle(&h, joint), "-90 deg", "離すと戻る位置に戻る");

    // 5: ▶ で動きを見る → 蓋の網だけが動く(値は変えない)。■ で止めると元の位置に戻る
    let before = h.state().cur.scene.bounds();
    h.get_by_label("▶").click();
    h.run_steps(20);
    assert!(h.state().cur.scene.is_posed(), "動いて見える");
    assert_eq!(angle(&h, joint), "-90 deg", "値は変えない");
    h.get_by_label("■").click();
    h.run_steps(3);
    assert!(!h.state().cur.scene.is_posed(), "止めると元の位置");
    assert_eq!(h.state().cur.scene.bounds(), before);

    // 6: 見ている途中にほかの物を選ぶ(パネルが替わる)と止まる
    h.get_by_label("▶").click();
    h.run_steps(5);
    assert!(h.state().cur.scene.is_posed());
    let first = h.state().cur.work.doc().data().features[0].id;
    h.state_mut().select(first);
    h.run_steps(5);
    assert!(!h.state().cur.scene.is_posed(), "パネルを閉じると止まる");
}

/// 3D のビューで蓋を引いて離した時も戻る位置に戻り、押した時と違う値に戻れば「位置を残す/元に戻す」の
/// 帯が出る(元に戻すと押す前の -10 度)。戻る位置が押した時と同じなら、何も残さず帯も出ない。
#[test]
fn dragging_in_the_view_returns_to_rest_and_asks_to_keep_it() {
    let (mut ws, joint) = hinged();
    let lid = ws.body_roots()[1].1;
    let drag_of = |ws: &Workspace| {
        ws.win
            .extensions
            .drag(
                &ws.context(),
                &jc_core::FacePick {
                    feature: lid,
                    face: "f".into(),
                },
            )
            .expect("蓋は回せる")
    };
    let angle =
        |ws: &Workspace| ws.cur.work.doc().data().feature(joint).unwrap().data["angle"].clone();
    let mut data = ws
        .cur
        .work
        .doc()
        .data()
        .feature(joint)
        .unwrap()
        .data
        .clone();
    data["angle_rest"] = "-90 deg".into();
    ws.set_feature_data(joint, data);
    ws.wait();
    let depth = ws.cur.work.doc().undo_depth();
    let d = drag_of(&ws);
    ws.drag_joint_to(d, -30.0);
    ws.wait();
    assert_eq!(angle(&ws), "-90 deg", "離すと戻る位置");
    assert_eq!(
        ws.cur.work.doc().undo_depth(),
        depth + 1,
        "引いた分と合わせて 1 つの取り消し単位"
    );
    assert!(ws.moved_pending(), "押した時と違う値なので帯を出す");
    ws.settle_moved(false);
    ws.wait();
    assert_eq!(angle(&ws), "-10 deg");
    // 戻る位置を押した時の値にすると、引いても何も残らない
    let mut data = ws
        .cur
        .work
        .doc()
        .data()
        .feature(joint)
        .unwrap()
        .data
        .clone();
    data["angle_rest"] = "-10 deg".into();
    ws.set_feature_data(joint, data);
    ws.wait();
    let depth = ws.cur.work.doc().undo_depth();
    let d = drag_of(&ws);
    ws.drag_joint_to(d, -60.0);
    ws.wait();
    assert_eq!(angle(&ws), "-10 deg");
    assert_eq!(ws.cur.work.doc().undo_depth(), depth);
    assert!(!ws.moved_pending());
}

#[test]
fn delete_while_editing_a_joint_removes_it_and_ends_the_edit() {
    // 10-22 a4: ジョイントを直している途中の Delete で、そのジョイントを消して直すのを終える
    let (mut ws, joint) = hinged();
    ws.open_edit(joint);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run_steps(3);
    assert!(h.state().is_editing(joint));
    h.key_press(egui::Key::Delete);
    h.run_steps(3);
    assert!(h.state().cur.work.doc().data().feature(joint).is_none());
    assert_eq!(h.state().cur.editing, None, "消えた機能を直し続けない");
    // 取り消すと戻る
    h.state_mut().undo();
    assert!(h.state().cur.work.doc().data().feature(joint).is_some());
}

/// 採用分 10-24 u9(試験だけ): 欄に数を打った後(Enter を押さずに)スライダを引き、欄の外を押しても、
/// 打ちかけの数へ戻されず、引いた値のまま残る。
#[test]
fn a_typed_value_does_not_come_back_after_dragging_the_slider() {
    let (mut ws, joint) = hinged();
    ws.select(joint);
    ws.cur.editing = Some(joint);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run_steps(3);
    // 人の手順 1: 角度の欄に -12 deg と打つ(Enter は押さない)
    h.get(by().role(Role::TextInput).value("-10 deg")).focus();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.event(egui::Event::Text("-12 deg".into()));
    h.run_steps(2);
    // 2: 欄の下のスライダを左の端の近くまで引いて離す
    let slider = h.get_by_role(Role::Slider).rect();
    let (y, x0, x1) = (slider.center().y, slider.left() + 2.0, slider.right() - 2.0);
    let press = |pos: egui::Pos2, pressed: bool| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    h.event(egui::Event::PointerMoved(egui::pos2(x1, y)));
    h.step();
    h.event(press(egui::pos2(x1, y), true));
    h.step();
    for x in [x1 - 40.0, (x0 + x1) / 2.0, x0 + 20.0] {
        h.event(egui::Event::PointerMoved(egui::pos2(x, y)));
        h.step();
    }
    h.event(press(egui::pos2(x0 + 20.0, y), false));
    h.run_steps(3);
    let dragged = angle(&h, joint);
    assert!(
        dragged != "-12 deg" && dragged != "-10 deg",
        "引いた値が入る: {dragged}"
    );
    // 3: 欄の外(ビューの何も無い所)を押す → 引いた値のまま。打ちかけの数に戻らない
    let away = egui::pos2(20.0, 780.0);
    h.event(egui::Event::PointerMoved(away));
    h.event(press(away, true));
    h.step();
    h.event(press(away, false));
    h.run_steps(3);
    assert_eq!(angle(&h, joint), dragged);
    assert!(
        h.query_by_label("範囲外").is_none(),
        "打ちかけの数の印も残らない"
    );
}
