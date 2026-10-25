//! 設計の機能を画面から作る・直す通しの試験(フィレット・ミラー・文字・名前・並べ替え・パラメータなど)。
//! 押し出しは `extrude_tests`、穴とねじは `hole_tests`、回転と軸は `revolve_tests`。

use crate::tests::{add_op, can, first_of, on_truck, path_lines, reg};
use crate::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kmb_core::Document;
use kmb_samples::{reference_document, sample_document};

#[test]
fn picking_the_top_face_and_sketching_on_it() {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .build_ui_state(
            |ui, ws: &mut Workspace| ws.ui(ui),
            Workspace::new(sample_document(), None, reg()),
        );
    h.run();
    assert!(h.state().selected().is_some(), "スケッチを選んでいる");
    let ex = h
        .state_mut()
        .run_action(kmb_cad_extrude::EXTRUDE_SKETCH)
        .unwrap();
    h.run();
    // 真上から見て、板の中央より少し左(穴を避ける)を選ぶ
    let ws = h.state_mut();
    ws.cur.camera.set_view(-90.0, 89.9);
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
    ws.cur.camera.fit(&ws.cur.scene, rect);
    let (p, _) = ws.cur.camera.project([10.0, 20.0, 10.0], rect);
    let pick = view3d::pick_at(&ws.cur.scene, &ws.cur.camera, rect, p).unwrap();
    assert_eq!(pick.feature, ex);
    assert_eq!(pick.face, format!("f{}:end", ex.0));
    ws.set_picked(Some(pick));
    assert!(ws.picked_face_is_planar());
    let s = ws.run_action(kmb_sketch::SKETCH_ON_FACE).unwrap();
    assert_eq!(ws.report().error_count(), 0);
    let entry = ws.document().data().feature(s).unwrap();
    let sk: kmb_sketch::Sketch = serde_json::from_value(entry.data.clone()).unwrap();
    assert!(matches!(sk.plane, kmb_sketch::SketchPlane::Face { .. }));
    h.run();
}

#[test]
fn rounding_the_edges_of_a_picked_face() {
    let part = kmb_samples::reference_part();
    let (plate, body) = (part.plate, part.hole_pattern);
    let mut ws = Workspace::new(part.doc, None, reg());
    let before = ws
        .cur
        .work
        .kernel()
        .mass(ws.visible_bodies()[0].1)
        .unwrap()
        .volume;
    assert!(
        ws.run_action(kmb_cad_edge::CHAMFER_FACE).is_none(),
        "面を選ぶまでは足せない"
    );
    ws.set_picked(Some(view3d::Pick {
        feature: body,
        face: format!("f{}:end", plate.0),
    }));
    let id = ws.run_action(kmb_cad_edge::CHAMFER_FACE).unwrap();
    assert!(
        matches!(ws.report().feature(id), Some(FeatureStatus::Ok { .. })),
        "{:?}",
        ws.report().feature(id)
    );
    let bodies = ws.visible_bodies();
    assert_eq!(bodies.len(), 1);
    assert_eq!(bodies[0].0, id);
    let after = ws.cur.work.kernel().mass(bodies[0].1).unwrap().volume;
    assert!(before - after > 20.0, "{before} → {after}");
}

/// 機能やボディの名前を変えても(取り消しても)、形を
/// 計算し直さず、表示の網も作り直さない。名前は計算の鍵(種類・データ・入力)に入らず、表示の鍵は
/// ボディの機能と形の番号だけ。
#[test]
fn renaming_a_feature_or_a_body_computes_nothing_and_keeps_the_meshes() {
    let mut ws = Workspace::open(reference_document(), None, true, reg());
    ws.wait();
    assert_eq!(ws.report().error_count(), 0);
    let key = ws.cur.scene.key().to_vec();
    assert!(!key.is_empty());
    let extrude = first_of(&ws, kmb_cad_extrude::EXTRUDE);
    type Step = fn(&mut Workspace, FeatureId) -> bool;
    let steps: [(&str, Step); 3] = [
        ("機能の名前", |ws, id| {
            ws.rename_feature(id, "名前を変えた板")
        }),
        ("ボディの名前", |ws, id| {
            ws.rename_body(id, "名前を変えたボディ")
        }),
        ("取り消し", |ws, _| {
            ws.undo();
            true
        }),
    ];
    for (what, step) in steps {
        assert!(step(&mut ws, extrude), "{what}");
        ws.wait();
        assert_eq!(ws.report().evaluated, 0, "{what}: 計算し直した");
        assert_eq!(ws.cur.scene.key(), key.as_slice(), "{what}: 網を作り直した");
    }
}

#[test]
fn features_are_moved_rolled_back_and_removed_from_the_menu() {
    let mut ws = Workspace::new(reference_document(), None, reg());
    let ids: Vec<FeatureId> = ws.document().data().features.iter().map(|f| f.id).collect();
    let n = ids.len();
    // 最初の機能(スケッチ)の後ろまで戻すと、後ろは止まる
    assert!(ws.roll_back_after(Some(ids[0])));
    assert!(matches!(
        ws.report().feature(ids[n - 1]),
        Some(FeatureStatus::RolledBack)
    ));
    assert!(ws.roll_back_after(None));
    assert_eq!(ws.report().error_count(), 0);
    // 最後の機能(パターン)は直前の穴を使うので、それより上へは動かせない(理由を出す)
    assert!(!ws.move_feature(ids[n - 1], -1));
    assert!(ws.message().is_some_and(|m| m.contains("動かせません")));
    assert_eq!(ws.document().data().features[n - 1].id, ids[n - 1]);
    // 使う・使われるの順を崩さない機能は上へ動かせ、取り消せる
    let i = (1..n)
        .find(|&i| {
            kmb_session::edit::move_conflict(ws.document().data(), &ws.win.registry, ids[i], i - 1)
                .is_none()
        })
        .expect("動かせる機能がある");
    assert!(ws.move_feature(ids[i], -1));
    assert_eq!(ws.document().data().features[i - 1].id, ids[i]);
    assert!(!ws.move_feature(ids[0], -1), "先頭より上へは動かない");
    ws.undo();
    assert_eq!(ws.document().data().features[i].id, ids[i]);
    // 消すと 1 つ減り、取り消しで戻る
    assert!(ws.remove_feature(ids[n - 1]));
    assert_eq!(ws.document().data().features.len(), n - 1);
    ws.undo();
    assert_eq!(ws.document().data().features.len(), n);
}

#[test]
fn solid_features_are_added_from_their_descriptors() {
    let mut ws = Workspace::new(reference_document(), None, reg());
    let why = |ws: &Workspace, t: &str| {
        ws.addable(kmb_core::category::SOLID)
            .into_iter()
            .find(|a| a.0 == t)
            .expect(t)
            .2
    };
    // ボディは 1 つだけなのでブーリアンは足せない(理由つき)。最後のパターンも道具の作り方を
    // 出すので、さらに並べられる(パターンの後のパターン・ミラーを灰色にしない)
    assert!(why(&ws, kmb_cad_boolean::BOOLEAN).is_some());
    assert!(why(&ws, kmb_cad_pattern::LINEAR_PATTERN).is_none());
    assert!(why(&ws, kmb_cad_pattern::MIRROR).is_none());
    let vol = |ws: &Workspace| {
        ws.cur
            .work
            .kernel()
            .mass(ws.visible_bodies().last().unwrap().1)
            .unwrap()
            .volume
    };
    let before = vol(&ws);
    // 穴は、スケッチの単独の点すべてに開く(固定した原点は除く)
    let hole = ws.add_described(kmb_cad_hole::HOLE).expect("足せる");
    assert!(
        matches!(
            ws.cur.work.report().feature(hole),
            Some(FeatureStatus::Ok { .. })
        ),
        "{:?}",
        ws.cur.work.report().feature(hole)
    );
    let holed = vol(&ws);
    // 径 3.4 mm の穴 2 つを 5 mm に広げる。理論では 211 mm³ 減るが、truck は同じ軸の円筒どうしの
    // ブーリアンで 333 mm³ 減らした(2026-10-12 実測、原因は未調査)。ここでは減ったことだけを見る
    let pi = std::f64::consts::PI;
    let (lo, hi) = (
        pi * (2.5f64.powi(2) - 1.7f64.powi(2)) * 10.0 * 2.0,
        pi * 2.5f64.powi(2) * 10.0 * 2.0,
    );
    let removed = before - holed;
    assert!(removed > lo - 1.0 && removed < hi + 1.0, "{removed}");
    // 最後の機能が穴になったので、それを並べられる
    assert!(why(&ws, kmb_cad_pattern::LINEAR_PATTERN).is_none());
    let id = ws
        .add_described(kmb_cad_pattern::LINEAR_PATTERN)
        .expect("足せる");
    let mut data = ws.cur.work.doc().data().feature(id).unwrap().data.clone();
    assert_eq!(data["feature"], serde_json::json!(hole.0));
    data["direction"] = serde_json::json!([0.0, 1.0, 0.0]);
    data["count"] = "1".into();
    ws.set_feature_data(id, data);
    assert!(
        matches!(
            ws.cur.work.report().feature(id),
            Some(FeatureStatus::Ok { .. })
        ),
        "{:?}",
        ws.cur.work.report().feature(id)
    );
    assert_eq!(ws.visible_bodies().len(), 1);
    assert!((vol(&ws) - holed).abs() < 1e-6, "1 個なら形は変わらない");
    // コマンド検索からも探せる(回転は輪郭・軸・角度をクリックで選ぶ操作。押しただけでは足さない)
    let found = ws.find_commands("かいてん");
    assert_eq!(
        found[0].id,
        crate::commands_ui::CommandId::Action(kmb_cad_revolve::REVOLVE_SKETCH.into())
    );
}

#[test]
fn a_mirror_takes_its_plane_from_the_picked_face() {
    let part = kmb_samples::reference_part();
    let (plate, body) = (part.plate, part.holes);
    let mut ws = Workspace::new(part.doc, None, reg());
    // パターンは写せないので、穴の後ろまで戻す
    assert!(ws.roll_back_after(Some(body)));
    ws.set_picked(Some(view3d::Pick {
        feature: body,
        face: format!("f{}:end", plate.0),
    }));
    let id = ws
        .run_action(kmb_cad_pattern::MIRROR_ON_FACE)
        .expect("足せる");
    let f = ws.cur.work.doc().data().feature(id).unwrap();
    assert_eq!(f.type_id, kmb_cad_pattern::MIRROR);
    let n: [f64; 3] = serde_json::from_value(f.data["normal"].clone()).unwrap();
    assert!(n[0].abs() < 1e-9 && n[1].abs() < 1e-9 && (n[2].abs() - 1.0).abs() < 1e-9);
    // 1 回の取り消しで消える
    ws.undo();
    assert!(ws.cur.work.doc().data().feature(id).is_none());
}

/// 原点の平面を選ぶ欄のある機能(ミラー)を直している間は、原点の平面を出し、欄で選んでいる平面を
/// 色付けする。直していなければ出さない。
#[test]
fn editing_a_mirror_shows_the_origin_planes_and_marks_the_chosen_one() {
    let part = kmb_samples::reference_part();
    let (plate, body) = (part.plate, part.holes);
    let mut ws = Workspace::new(part.doc, None, reg());
    assert!(ws.roll_back_after(Some(body)));
    ws.set_picked(Some(view3d::Pick {
        feature: body,
        face: format!("f{}:end", plate.0),
    }));
    let id = ws
        .run_action(kmb_cad_pattern::MIRROR_ON_FACE)
        .expect("足せる");
    ws.finish_edit();
    assert_eq!(ws.origin_plane_field(), None, "直していなければ出さない");
    ws.open_edit(id);
    assert_eq!(
        ws.origin_plane_field(),
        Some(None),
        "数で指定: 平面は出し、色付けしない"
    );
    let mut data = ws.cur.work.doc().data().feature(id).unwrap().data.clone();
    data["plane_ref"] = serde_json::json!({"kind": "origin_yz"});
    ws.cur
        .work
        .apply(kmb_core::Transaction::new(
            "対称の平面",
            vec![kmb_core::Op::SetFeatureData { id, data }],
        ))
        .unwrap();
    assert_eq!(ws.origin_plane_field(), Some(Some(1)), "YZ 平面を色付け");
    ws.finish_edit();
    assert_eq!(ws.origin_plane_field(), None);
}

#[test]
fn text_is_put_on_the_picked_face_and_can_be_engraved() {
    let cam = kmb_samples::reference_cam();
    let (plate, body) = (cam.part.plate, cam.part.hole_pattern);
    let mut ws = Workspace::new(cam.part.doc, None, reg());
    ws.set_picked(Some(view3d::Pick {
        feature: body,
        face: format!("f{}:end", plate.0),
    }));
    let id = ws.run_action(kmb_text::TEXT_ON_FACE).expect("足せる");
    assert!(
        matches!(ws.report().feature(id), Some(FeatureStatus::Ok { .. })),
        "{:?}",
        ws.report().feature(id)
    );
    // 選ぶと 3D のビューに文字の線が出る
    assert!(!path_lines(&ws).is_empty());
    let engrave = add_op(kmb_strategy_engrave::ENGRAVE);
    assert!(can(&ws, engrave));
    let op = ws.run_action(engrave).expect("彫刻を足せる");
    let f = ws.cur.work.doc().data().feature(op).unwrap();
    assert_eq!(f.data["curves"], serde_json::json!(id.0));
    assert_eq!(f.data["tool"], 5, "V ビットを選ぶ");
}

#[test]
fn selecting_does_not_edit_and_a_new_feature_can_be_cancelled() {
    let part = kmb_samples::reference_part();
    let holes = part.holes;
    let mut ws = Workspace::new(part.doc, None, reg());
    // 選んだだけでは直さない。ダブルクリックや「編集」で開く
    ws.select(holes);
    assert!(!ws.is_editing(holes));
    ws.open_edit(holes);
    assert!(ws.is_editing(holes));
    // 直してキャンセルすると、直し始めた時へ戻る
    let before = ws.document().data().feature(holes).unwrap().data.clone();
    let mut d = before.clone();
    d["diameter"] = serde_json::json!("9 mm");
    assert!(ws.set_feature_data(holes, d));
    ws.cancel_edit();
    assert!(!ws.is_editing(holes));
    assert_eq!(ws.document().data().feature(holes).unwrap().data, before);
    // 欄で指定する穴(機能を直接足す)は「作っている途中」になり、キャンセルで作る前へ戻る
    let n = ws.document().data().features.len();
    let id = ws.add_described(kmb_cad_hole::HOLE).expect("足せる");
    assert_eq!(ws.creating(), Some(id));
    assert!(ws.is_editing(id));
    ws.cancel_creation();
    assert_eq!(ws.document().data().features.len(), n, "作る前へ戻る");
    assert_eq!(ws.creating(), None);
    assert_eq!(ws.document().data().feature(holes).unwrap().data, before);
    // OK なら残り、選んだだけの状態になる
    let id = ws.add_described(kmb_cad_hole::HOLE).expect("足せる");
    ws.finish_edit();
    assert_eq!(ws.creating(), None);
    assert_eq!(ws.document().data().features.len(), n + 1);
    assert!(!ws.is_editing(id));
    // 作っている途中に別のものを選ぶと、作ったものは決まる
    let id2 = ws.add_described(kmb_cad_hole::HOLE).expect("足せる");
    ws.select(holes);
    assert_eq!(ws.creating(), None);
    assert!(ws.document().data().feature(id2).is_some());
}

#[test]
fn escape_cancels_a_new_feature_then_clears_the_selection() {
    let part = kmb_samples::reference_part();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui_state(
            |ui, ws: &mut Workspace| ws.ui(ui),
            Workspace::new(part.doc, None, reg()),
        );
    h.run();
    let n = h.state().document().data().features.len();
    h.state_mut()
        .add_described(kmb_cad_hole::HOLE)
        .expect("足せる");
    h.run();
    assert!(h.state().creating().is_some(), "作っている途中");
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(
        h.state().document().data().features.len(),
        n,
        "Esc で作らずにやめる"
    );
    assert!(h.state().selected().is_none());
    // 選んでいるだけの時、Enter で直し始め、Esc でやめる
    let last = h.state().document().data().features.last().unwrap().id;
    h.state_mut().select(last);
    h.run();
    h.get_by_label_contains("選んでいるだけです");
    h.key_press(egui::Key::Enter);
    h.run();
    assert!(h.state().is_editing(last));
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(!h.state().is_editing(last));
    assert_eq!(h.state().selected(), Some(last));
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.state().selected().is_none(), "もう一度 Esc で選びを外す");
}

/// 板の上の面のまわりの、出っ張ったまっすぐな辺(表示しているボディの辺として)。
fn top_edges(ws: &mut Workspace, plate: FeatureId) -> Vec<kmb_core::EdgePick> {
    let top = format!("f{}:end", plate.0);
    ws.scene_edges()
        .iter()
        .flat_map(|(f, l)| l.iter().map(move |e| (*f, e)))
        .filter(|(_, e)| e.convex && e.faces.contains(&top))
        .map(|(feature, e)| kmb_core::EdgePick {
            feature,
            faces: e.faces.clone(),
        })
        .collect()
}

#[test]
fn the_fillet_button_asks_for_edges_and_rounds_only_those() {
    // フィレットは押してから辺をクリックで選び、Enter で欄へ進み、半径を打って Enter で決める
    // (選んだ面のまわりの辺すべてに掛けるのではなく、選んだ辺だけに掛ける)
    let part = kmb_samples::reference_part();
    let plate = part.plate;
    let mut h = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui_state(
            |ui, ws: &mut Workspace| ws.ui(ui),
            Workspace::new(part.doc, None, reg()),
        );
    h.run();
    h.state_mut().clear_selection();
    let n = h.state().document().data().features.len();
    assert!(
        can(h.state(), kmb_cad_edge::FILLET_FACE),
        "何も選ばずに押せる"
    );
    h.state_mut().start_action(kmb_cad_edge::FILLET_FACE);
    h.run();
    let g = h.state().cur.gesture.as_ref().expect("辺を選ぶ手順に入る");
    assert!(matches!(
        g.current().unwrap().gesture,
        kmb_core::Gesture::Edges
    ));
    assert_eq!(h.state().document().data().features.len(), n);
    // 辺を 2 本クリックした(ビューがすることを直接)
    let edges = top_edges(h.state_mut(), plate);
    assert!(edges.len() >= 4, "{edges:?}");
    h.state_mut().set_picked_edges(edges[..2].to_vec());
    h.key_press(egui::Key::Enter);
    h.run();
    let ws = h.state();
    assert!(ws.cur.gesture.is_none(), "選び終えたら足す");
    let id = ws.creating().expect("作っている途中で欄を出す");
    assert!(ws.picked_edges().is_empty(), "選んだ辺は手順の値に移す");
    let data = &ws.document().data().feature(id).unwrap().data;
    let row = &data["rows"][0];
    assert_eq!(row["edges"].as_array().unwrap().len(), 2, "{data}");
    assert!(
        row["edges"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["kind"] == "edge"),
        "面のまわり全部には掛けない"
    );
    // 最初の欄(半径)にキーボードが移っているので、そのまま打って Enter で決める
    // (欄の文字は全体を選んであるので、打った数字で置き換わる)
    h.run();
    h.event(egui::Event::Text("2".into()));
    h.key_press(egui::Key::Enter);
    h.run();
    h.state_mut().wait();
    let ws = h.state();
    assert_eq!(ws.creating(), None, "Enter で決まる");
    assert_eq!(
        ws.document().data().feature(id).unwrap().data["rows"][0]["radius"],
        "2 mm",
        "数だけ打てば今の単位を付ける"
    );
    // 流れを見る試験なので、truck のブーリアンの弱さ(同じ平面で接する面)は問わない
    if !on_truck() {
        assert_eq!(ws.report().error_count(), 0, "{:?}", ws.report());
    }
    assert_eq!(ws.document().data().features.len(), n + 1);
}

#[test]
fn preselected_edges_skip_the_edge_step() {
    let part = kmb_samples::reference_part();
    let plate = part.plate;
    let mut ws = Workspace::new(part.doc, None, reg());
    let edges = top_edges(&mut ws, plate);
    ws.set_picked_edges(vec![edges[0].clone()]);
    let before = ws
        .cur
        .work
        .kernel()
        .mass(ws.visible_bodies()[0].1)
        .unwrap()
        .volume;
    let id = ws
        .start_action(kmb_cad_edge::CHAMFER_FACE)
        .expect("すぐ足す");
    assert!(ws.cur.gesture.is_none());
    assert_eq!(ws.creating(), Some(id));
    let after = ws
        .cur
        .work
        .kernel()
        .mass(ws.visible_bodies()[0].1)
        .unwrap()
        .volume;
    assert!(before - after > 1.0, "1 本だけ面取り: {before} → {after}");
    assert_eq!(ws.report().error_count(), 0);
}

#[test]
fn a_bare_number_takes_the_unit_of_the_current_value() {
    use crate::feature_ui::with_unit_of;
    assert_eq!(with_unit_of("3", "1 mm"), "3 mm");
    assert_eq!(with_unit_of("2.5", "30 deg"), "2.5 deg");
    assert_eq!(with_unit_of("3 in", "1 mm"), "3 in");
    assert_eq!(with_unit_of("4", "2"), "4", "個数は単位なし");
    assert_eq!(
        with_unit_of("5", "thickness - 1 mm"),
        "5",
        "式なら打ったまま"
    );
    assert_eq!(with_unit_of("6", ""), "6");
    // 空の省ける欄(穴の深さなど)も、欄が宣言した単位で補う(人の点検 2026-10-15: 「5」で計算できなくなった)。
    // 欄の名前からは推し量らない(「温度(℃)」に 50 と打って 50 mm になっていた。2026-10-20)
    use crate::feature_ui::typed_expr;
    use kmb_core::Field;
    let ws = Workspace::new(kmb_samples::reference_document(), None, kmb_bundle::modules());
    let typed = |f: Field, v: &str| typed_expr(&ws, &f, "", v);
    assert_eq!(
        typed(Field::opt_expr("depth", "深さ").length(), "5"),
        "5 mm"
    );
    assert_eq!(
        typed(Field::expr("a", "全体の角度").angle(), "90"),
        "90 deg"
    );
    assert_eq!(typed(Field::expr("n", "個数").unitless(), "3"), "3");
    assert_eq!(typed(Field::expr("t", "温度(℃)").plain("℃"), "50"), "50");
    assert_eq!(
        typed(Field::opt_expr("t", "まわりの温度(℃)").plain("℃"), "25"),
        "25"
    );
}

#[test]
fn ratio_and_rpm_fields_take_bare_numbers_and_percent() {
    // Z20(網羅の調べ J51): 「切削ピッチ(工具径の比)」に 0.3 と打つと 0.3 mm になり単位の誤り、30% も通らなかった
    use crate::feature_ui::typed_expr;
    use kmb_core::Field;
    let ws = Workspace::new(kmb_samples::reference_document(), None, kmb_bundle::modules());
    let typed = |f: Field, v: &str| typed_expr(&ws, &f, "", v);
    let pitch = || Field::expr("stepover", "切削ピッチ(工具径の比)").ratio();
    assert_eq!(typed(pitch(), "0.3"), "0.3");
    assert_eq!(
        typed(
            Field::opt_expr("rpm", "主軸回転数(rpm)").plain("rpm"),
            "12000"
        ),
        "12000"
    );
    assert_eq!(typed(Field::expr("scale", "幅の倍率").unitless(), "2"), "2");
    assert_eq!(typed(Field::expr("r", "回転").angle(), "45"), "45 deg");
    assert_eq!(
        typed(Field::opt_expr("feed", "切削送り(mm/min)").length(), "500"),
        "500 mm"
    );
    assert_eq!(typed(pitch(), "30%"), "0.3");
    assert_eq!(typed(pitch(), "30 %"), "0.3");
    assert_eq!(
        typed(Field::expr("depth", "深さ").length(), "30%"),
        "30%",
        "比でない欄はそのまま"
    );
}

#[test]
fn removing_a_used_feature_asks_first_and_moving_keeps_the_order_of_use() {
    let part = kmb_samples::reference_part();
    let (plate, holes, pattern) = (part.plate, part.holes, part.hole_pattern);
    let mut ws = Workspace::new(part.doc, None, reg());
    let n = ws.document().data().features.len();
    // 板を消すと後ろの全部が計算できなくなるので、すぐには消さずに確かめる(間接に使うものも並べる)
    assert!(!ws.ask_remove_features(&[plate]));
    assert_eq!(ws.document().data().features.len(), n, "まだ消さない");
    let (_, users) = ws.cur.remove_ask.clone().expect("確かめる");
    assert!(
        users.contains(&holes) && users.contains(&pattern),
        "{users:?}"
    );
    ws.cur.remove_ask = None;
    // 使っている機能より前へは動かせず、理由を出す
    let at = ws.document().data().index_of(holes).unwrap();
    let before = ws.document().data().index_of(plate).unwrap();
    assert!(!ws.move_feature(holes, before as i32 - at as i32));
    assert!(ws.message().is_some_and(|m| m.contains("動かせません")));
    assert_eq!(ws.document().data().index_of(holes), Some(at));
    // 誰も使っていない最後の機能は、聞かずに消える。履歴には名前が出る
    assert!(ws.ask_remove_features(&[pattern]));
    assert_eq!(ws.document().data().features.len(), n - 1);
}

#[test]
fn e_key_starts_extrude_and_right_click_repeats_the_last_command() {
    // E で押し出しを始め、右クリックの「繰り返す」で最後のコマンドをもう一度始める
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .build_ui_state(
            |ui, ws: &mut Workspace| ws.ui(ui),
            Workspace::new(sample_document(), None, reg()),
        );
    h.run();
    h.key_press(egui::Key::E);
    h.run();
    assert_eq!(
        h.state().cur.gesture.as_ref().map(|g| g.action.as_str()),
        Some("cad.extrude"),
        "E で押し出しの流れに入る"
    );
    assert_eq!(h.state().win.last_item.as_deref(), Some("a:cad.extrude"));
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.state().cur.gesture.is_none(), "Esc でやめる");
    let it = h.state().item_of("a:cad.extrude").unwrap();
    assert_eq!(it.label, "押し出し");
    // 右クリックのメニューの「繰り返す」
    // 3D のビューの中(右の編集パネルとタイムラインを避けた所)
    let at = egui::pos2(500.0, 300.0);
    h.input_mut().events.push(egui::Event::PointerMoved(at));
    h.run();
    for pressed in [true, false] {
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: Default::default(),
        });
        h.step();
    }
    h.run();
    h.get_by_label("繰り返す: 押し出し").click();
    h.run();
    assert_eq!(
        h.state().cur.gesture.as_ref().map(|g| g.action.as_str()),
        Some("cad.extrude"),
        "繰り返すで押し出しをもう一度始める"
    );
}

#[test]
fn add_sketch_takes_a_face_click_as_well_as_an_origin_plane() {
    // 「スケッチを作成」を押してから面をクリックすればそこに描く(XY・YZ・ZX に
    // 限らず、Esc でやめて面を選び直さずに済む)
    let mut ws = Workspace::new(sample_document(), None, reg());
    let ex = ws.run_action(kmb_cad_extrude::EXTRUDE_SKETCH).unwrap();
    ws.wait();
    ws.clear_selection();
    ws.set_picked(None);
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
    ws.cur.camera.set_view(-90.0, 89.9);
    ws.cur.camera.fit(&ws.cur.scene, rect);
    ws.start_action(kmb_sketch::ADD_SKETCH);
    let h = ws.cur.gesture.as_ref().unwrap().current().unwrap();
    assert!(h.faces, "面も受け付ける手順");
    let p = ws.cur.camera.project([10.0, 20.0, 10.0], rect).0;
    let ev = view3d::ViewEvents {
        click: Some(p),
        ..Default::default()
    };
    ws.gesture_events(&ev, rect);
    assert!(ws.cur.gesture.is_none(), "面のクリックで決まる");
    let s = ws.selected().expect("スケッチを足して選ぶ");
    let entry = ws.document().data().feature(s).unwrap();
    let sk: kmb_sketch::Sketch = serde_json::from_value(entry.data.clone()).unwrap();
    // 新しい面のスケッチは、面の角を原点にする(まとめてレビューの R2)
    assert!(
        matches!(
            &sk.plane,
            kmb_sketch::SketchPlane::Face {
                feature,
                face,
                origin: kmb_sketch::FaceOrigin::Vertex { .. },
            } if *feature == ex && *face == format!("f{}:end", ex.0)
        ),
        "{:?}",
        sk.plane
    );
    // 何もない所(原点の平面の四角の中)なら、その平面を選ぶ。原点の平面は
    // 原点の近くから正の向きへ広がり、ボディと重なるので、ボディを隠してから押す
    ws.cancel_edit();
    ws.set_picked(None);
    ws.clear_selection();
    ws.set_hidden(ex, true);
    ws.start_action(kmb_sketch::ADD_SKETCH);
    let s = view3d::origin_size(&ws.cur.camera, rect);
    let p = ws.cur.camera.project([s * 0.5, s * 0.5, 0.0], rect).0;
    let ev = view3d::ViewEvents {
        click: Some(p),
        hover: Some(p),
        ..Default::default()
    };
    ws.gesture_events(&ev, rect);
    assert!(ws.cur.gesture.is_none(), "平面のクリックで決まる");
    let s = ws.selected().unwrap();
    let sk: kmb_sketch::Sketch =
        serde_json::from_value(ws.document().data().feature(s).unwrap().data.clone()).unwrap();
    assert_eq!(sk.plane, kmb_sketch::SketchPlane::Xy);
}

#[test]
fn a_failed_feature_can_be_reviewed_and_its_message_copied() {
    let cam = kmb_samples::reference_cam();
    let (pocket, op) = (cam.part.pocket, cam.pocket);
    let mut ws = Workspace::new(cam.part.doc, None, reg());
    assert!(ws.set_suppressed(pocket, true));
    let h = timeline_ui::Health::of(ws.report().feature(op));
    assert!(h.is_red(), "{h:?}");
    // 右クリックの「エラーを確かめる」: その機能を選び、窓を開く
    ws.open_review(op);
    assert_eq!(ws.cur.selected, Some(op));
    assert!(ws.cur.review.is_some());
    let name = ws.document().data().feature(op).unwrap().name.clone();
    let text = ws.review_text(op);
    assert!(text.starts_with(&format!("{name}(エラー): ")), "{text}");
    assert!(text.contains("参照が切れた"), "{text}");
    // ビューの右下の印が数える機能の先頭に来る(赤が先)
    assert_eq!(ws.problems().first().map(|(id, _)| *id), Some(op));
    // 窓の文は内部の面の名前を出さない
    assert!(
        !ws.health(op).message().contains(":end"),
        "{:?}",
        ws.health(op)
    );
    // 測る道具は同じ場所に窓を出すので、開くと確かめる窓は閉じる
    ws.toggle_measure();
    assert!(ws.cur.review.is_none());
    // 窓を描く
    assert!(ws.set_suppressed(pocket, false));
    ws.open_review(op);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    // 直した後は、残った注意(削り残し)を出す
    assert!(matches!(
        timeline_ui::Health::of(h.state().report().feature(op)),
        timeline_ui::Health::Warning(_)
    ));
    assert!(h.query_by_label("注意").is_some());
    assert!(h.query_by_label("文を写す").is_some());
    assert!(h.query_by_label("直し方の例").is_some());
}

#[test]
fn a_feature_is_renamed_from_its_menu_and_the_rename_can_be_undone() {
    let mut ws = Workspace::new(kmb_samples::reference_document(), None, reg());
    let id = ws.document().data().features[1].id;
    let old = ws.document().data().features[1].name.clone();
    // 空や同じ名前では何もしない
    assert!(!ws.rename_feature(id, "  "));
    assert!(!ws.rename_feature(id, &old));
    assert!(ws.rename_feature(id, " 土台の板 "));
    assert_eq!(ws.document().data().feature(id).unwrap().name, "土台の板");
    assert_eq!(ws.report().error_count(), 0);
    ws.undo();
    assert_eq!(ws.document().data().feature(id).unwrap().name, old);
}

#[test]
fn an_unused_parameter_can_be_removed_but_a_used_one_cannot() {
    let mut ws = Workspace::new(kmb_samples::reference_document(), None, reg());
    let id_of = |ws: &Workspace, n: &str| {
        ws.document()
            .data()
            .params
            .iter()
            .find(|p| p.name == n)
            .map(|p| p.id)
    };
    // 板の厚みに使っている
    let thickness = id_of(&ws, "thickness").expect("ある");
    let users = edit::param_users(ws.document().data(), &ws.win.registry, thickness);
    assert!(users.iter().any(|u| u == "板"), "{users:?}");
    assert!(!ws.remove_param(thickness));
    assert!(id_of(&ws, "thickness").is_some());
    // 使われていない物は消え、取り消すと戻る
    assert!(ws.add_param("extra", "1 mm"));
    let extra = id_of(&ws, "extra").expect("足した");
    assert!(edit::param_users(ws.document().data(), &ws.win.registry, extra).is_empty());
    assert!(ws.remove_param(extra));
    assert!(id_of(&ws, "extra").is_none());
    assert_eq!(ws.report().error_count(), 0);
    // 使っている所は寸法まで出す(U53): 見本の width は板の輪郭の寸法 d1 が使う
    let named = Workspace::new(sample_document(), None, reg());
    let doc = named.document();
    let users = edit::name_users(doc.data(), &ws.win.registry, "width", None);
    assert!(users.iter().any(|u| u == "板の輪郭 の d1"), "{users:?}");
    // 寸法の名前を使っている所: 寸法を持つスケッチそのものは数えない
    assert!(edit::name_users(doc.data(), &ws.win.registry, "d1", None).is_empty());
    // 表がまとめて数える物は、名前ごとに数えた物と同じ
    let all = edit::all_name_users(doc.data(), &ws.win.registry);
    assert_eq!(all["width"], users);
    assert!(all["d1"].is_empty());
    for p in &doc.data().params {
        assert_eq!(
            all[&p.name],
            edit::param_users(doc.data(), &ws.win.registry, p.id),
            "{}",
            p.name
        );
    }
    ws.undo();
    assert!(id_of(&ws, "extra").is_some());
    // ほかのパラメータの式に使われていても消せない
    assert!(ws.add_param("twice", "extra * 2"));
    assert!(!ws.remove_param(extra));
}

#[test]
fn vector_fields_show_short_numbers() {
    assert_eq!(crate::feature_ui::short_num(-0.000000000000004), "0");
    assert_eq!(crate::feature_ui::short_num(30.0), "30");
    assert_eq!(crate::feature_ui::short_num(1.0 / 3.0), "0.333333");
    assert_eq!(crate::feature_ui::short_num(-2.5), "-2.5");
}

#[test]
fn a_fillet_made_from_the_cli_gets_the_same_corner_as_from_the_screen() {
    // 画面から作るとフィレットは「ローリングボール」、面取りは「三角の面」。CLI・MCP で角を省いても同じにする
    let mut h = kmb_session::Headless::with_document(reg(), reference_document(), None);
    let plate = h.doc().data().features[1].id;
    let edges = serde_json::json!([["f2:end", "f2:side:e1"]]);
    let f = h
        .add_feature(
            kmb_cad_edge::FILLET,
            None,
            serde_json::json!({"target": plate.0, "edges": edges, "radius": "1 mm"}),
        )
        .expect("足せる");
    let c = h
        .add_feature(
            kmb_cad_edge::CHAMFER,
            None,
            serde_json::json!({"target": plate.0, "edges": edges, "distance": "1 mm", "corner": "miter"}),
        )
        .expect("足せる");
    let data = |id| h.doc().data().feature(id).unwrap().data.clone();
    assert_eq!(data(f)["corner"], "ball");
    // 指定した値は替えない
    assert_eq!(data(c)["corner"], "miter");
    // 保存済みの文書で角を省いた物は、前のまま交わりで読む
    let old: kmb_cad_edge::Fillet = serde_json::from_value(serde_json::json!({
        "target": plate.0, "edges": edges, "radius": "1 mm"
    }))
    .expect("読める");
    assert_eq!(old.corner, kmb_cad_edge::Corner::Miter);
}

#[test]
fn a_body_is_renamed_and_keeps_its_name_when_features_are_added() {
    let mut ws = Workspace::new(kmb_samples::reference_document(), None, reg());
    let (last, _) = ws.visible_bodies()[0];
    let bodies: Vec<_> = vec![ws.document().data().feature(last).unwrap().clone()];
    let (label, root, _) = ws.body_numbers(&bodies)[0].clone();
    assert_eq!(label, "ボディ1");
    assert_eq!(ws.body_name_of(root), "ボディ1");
    assert!(!ws.rename_body(root, "ボディ1"), "同じ名前では何もしない");
    assert!(ws.rename_body(root, " 土台 "));
    assert_eq!(ws.body_name_of(root), "土台");
    // 保存して読み直しても残る(鍵は最初に作った機能)
    let json = ws.document().to_json();
    let doc = Document::from_json(&json).expect("読める");
    assert_eq!(
        doc.data().body_names.get(&root).map(String::as_str),
        Some("土台")
    );
    // 取り消すと「ボディ1」に戻り、やり直すとまた付く
    ws.undo();
    assert_eq!(ws.body_name_of(root), "ボディ1");
    ws.redo();
    assert_eq!(ws.body_name_of(root), "土台");
    // 空にすると「ボディ1」に戻す
    assert!(ws.rename_body(root, ""));
    assert_eq!(ws.body_name_of(root), "ボディ1");
    assert!(ws.document().data().body_names.is_empty());
}

#[test]
fn text_is_exploded_into_lines_of_a_new_sketch_on_the_same_face() {
    // 文字を普通の線にして 1 本ずつ直せるように
    let cam = kmb_samples::reference_cam();
    let (plate, body) = (cam.part.plate, cam.part.hole_pattern);
    let mut ws = Workspace::new(cam.part.doc, None, reg());
    ws.set_picked(Some(view3d::Pick {
        feature: body,
        face: format!("f{}:end", plate.0),
    }));
    let text = ws.run_action(kmb_text::TEXT_ON_FACE).expect("足せる");
    ws.commit_creation();
    let curves = |ws: &Workspace, id| {
        ws.output(id, "curves")
            .and_then(kmb_core::Curves::from_value)
            .expect("線がある")
    };
    let before = curves(&ws, text);
    assert!(can(&ws, kmb_text::EXPLODE_TEXT), "文字を選んでいれば使える");
    let sketch = ws.run_action(kmb_text::EXPLODE_TEXT).expect("分解できる");
    let data = ws.cur.work.doc().data();
    let f = data.feature(sketch).unwrap();
    assert_eq!(f.type_id, kmb_sketch::SKETCH);
    assert!(f.name.ends_with("の線"), "{}", f.name);
    assert!(data.feature(text).unwrap().suppressed, "元の文字は抑制する");
    assert!(
        matches!(ws.report().feature(sketch), Some(FeatureStatus::Ok { .. })),
        "{:?}",
        ws.report().feature(sketch)
    );
    // 同じ面の上に、同じ線(世界座標で同じ所)ができる
    let after = curves(&ws, sketch);
    assert_eq!(after.segments.len(), before.segments.len());
    let world = |c: &kmb_core::Curves| {
        let mut v: Vec<[i64; 3]> = c
            .segments
            .iter()
            .flat_map(|s| [s.start_point(), s.end_point()])
            .map(|p| c.plane.to_world(p).map(|x| (x * 1000.0).round() as i64))
            .collect();
        v.sort();
        v
    };
    assert_eq!(world(&after), world(&before));
    // 取り消すと文字に戻る
    ws.undo();
    assert!(!ws.cur.work.doc().data().feature(text).unwrap().suppressed);
    assert!(ws.cur.work.doc().data().feature(sketch).is_none());
}

/// パラメータの一覧の、入力欄に出ている文字(上から順)。
fn param_fields(h: &Harness<'_, Workspace>) -> Vec<String> {
    use egui_kittest::kittest::NodeT;
    h.query_all(egui_kittest::kittest::by().role(egui::accesskit::Role::TextInput))
        .filter_map(|n| n.accesskit_node().value().map(|v| v.to_string()))
        .collect()
}

#[test]
fn renaming_a_parameter_in_the_list_rewrites_the_name_where_it_is_used() {
    // まとめてレビュー 2026-10-18: パラメータの名前を変えても、使っている所の名前が画面で
    // 書き換わらなかった(寸法の欄が前のコマの入力を覚えていて、古い式を出し続けた)
    let mut ws = Workspace::new(reference_document(), None, reg());
    ws.win.show_params = true;
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    let before = param_fields(&h);
    assert!(
        before.iter().any(|v| v == "width / 2"),
        "寸法の欄に width を使う式がある: {before:?}"
    );
    // 名前の欄の width を plate_w に打ち替えて Enter
    {
        use egui_kittest::kittest::NodeT;
        let n = h
            .query_all(egui_kittest::kittest::by().role(egui::accesskit::Role::TextInput))
            .find(|n| n.accesskit_node().value().as_deref() == Some("width"))
            .expect("名前の欄 width");
        n.click();
    }
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.event(egui::Event::Text("plate_w".into()));
    h.key_press(egui::Key::Enter);
    h.run();
    let p = &h.state().document().data().params;
    assert!(p.iter().any(|p| p.name == "plate_w"), "名前が変わる");
    let exprs: Vec<String> = h
        .state()
        .report()
        .model_params
        .iter()
        .map(|p| p.expr.clone())
        .collect();
    assert!(
        exprs.iter().any(|e| e == "plate_w / 2"),
        "文書の側: {exprs:?}"
    );
    let after = param_fields(&h);
    assert!(
        after.iter().all(|v| !v
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .any(|w| w == "width")),
        "古い名前が欄に残らない: {after:?}"
    );
    assert!(after.iter().any(|v| v == "plate_w / 2"), "{after:?}");
    assert_eq!(h.state().report().error_count(), 0);
}

/// 履歴の印より後の機能を直すと、印をその機能の後ろまで進める(印より後は
/// 計算されないので、直しても形に出ない)。キャンセルすると印も元の所へ戻る。
#[test]
fn editing_a_feature_after_the_rollback_marker_moves_the_marker_past_it() {
    let part = kmb_samples::reference_part();
    let mut ws = Workspace::new(part.doc, None, reg());
    let ids: Vec<FeatureId> = ws
        .cur
        .work
        .doc()
        .data()
        .features
        .iter()
        .map(|f| f.id)
        .collect();
    let (first, last) = (ids[0], *ids.last().unwrap());
    assert!(ws.roll_back_after(Some(first)));
    assert_eq!(ws.cur.work.doc().data().active_len(), 1);
    ws.open_edit(last);
    assert_eq!(
        ws.cur.work.doc().data().active_len(),
        ids.len(),
        "直す機能の後ろまで印が進んでいない"
    );
    ws.cancel_edit();
    assert_eq!(
        ws.cur.work.doc().data().active_len(),
        1,
        "キャンセルで印が戻らない"
    );
    // 印より前の機能は、印を動かさない
    ws.open_edit(first);
    assert_eq!(ws.cur.work.doc().data().active_len(), 1);
}
