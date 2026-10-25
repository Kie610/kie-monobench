//! ブラウザとタイムラインの試験(ボディの行、まとまりの目、今の位置の印、設計に加工を出さない)。

use crate::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

fn reg() -> kmb_core::Modules {
    kmb_bundle::modules()
}

fn harness(ws: Workspace) -> Harness<'static, Workspace> {
    // 1 フレームを短くする(既定の 0.25 秒では、続けた 2 回のクリックが二度押しにならない)
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .with_step_dt(0.05)
        .with_max_steps(40)
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h
}

#[test]
fn a_hidden_body_stays_hidden_and_its_row_shows_only_body_commands() {
    // 1 つの部品の画面で順に確かめる:
    // - 隠したボディは、最後の機能が替わっても隠したまま(元 a_hidden_body_stays_hidden_when_its_last_feature_changes)
    // - ボディの行はボディの名前とボディの操作だけを出す(元 a_body_row_shows_the_body_and_only_body_commands)
    let part = kmb_samples::reference_part();
    let last = part.hole_pattern;
    let last_name = part.doc.data().feature(last).unwrap().name.clone();
    let (holes, pattern) = (part.holes, part.hole_pattern);
    let mut h = harness(Workspace::new(part.doc, None, reg()));
    {
        // 隠したことを最後の機能で覚えると、機能を足した(最後の機能が替わった)時にまた出てしまう
        let ws = h.state_mut();
        let (last, root) = ws.body_roots()[0];
        assert_eq!(last, pattern);
        ws.set_body_hidden(root, true);
        assert!(ws.is_hidden(pattern) && ws.cur.scene.is_empty());
        // 穴のパターンの手前まで戻すと、最後の機能は穴になる。それでも隠したまま
        assert!(ws.roll_back_after(Some(holes)));
        ws.wait();
        assert_eq!(ws.body_roots()[0], (holes, root));
        assert!(ws.is_hidden(holes), "最後の機能が替わっても隠したまま");
        assert!(ws.cur.scene.is_empty());
        assert!(ws.roll_back_after(None));
        ws.wait();
        assert!(ws.is_hidden(pattern) && ws.cur.scene.is_empty());
        ws.set_body_hidden(root, false);
        assert!(!ws.is_hidden(pattern) && !ws.cur.scene.is_empty());
    }
    h.run();
    // 行の名前はボディの名前だけ(脇に最後の機能の名前を出さない)。ブラウザに最後の機能の名前は出ない
    h.get_by_label("ボディ1");
    assert!(
        h.query_all_by_label(&last_name)
            .all(|n| n.rect().left() > 200.0),
        "ブラウザに最後の機能の名前を出さない"
    );
    // 右クリックはボディの操作だけ(最後の機能の編集・抑制・消すは出さない)
    h.get_by_label("ボディ1").click_secondary();
    h.run();
    h.get_by_label("名前変更");
    h.get_by_label("非表示");
    for not in ["フィーチャを編集", "抑制", "削除", "ここまで戻す"] {
        assert!(h.query_by_label(not).is_none(), "{not} は出さない");
    }
    h.get_by_label("非表示").click();
    h.run();
    assert!(h.state().is_hidden(last));
    assert!(h.state().cur.scene.is_empty());
    // ダブルクリックは名前の変更(最後の機能の編集は開かない)。前のクリックから時間を空ける
    // (空けないと三度押しに数えられる)
    for _ in 0..20 {
        h.step();
    }
    let row = h.get_by_label("ボディ1");
    row.click();
    row.click();
    h.run();
    assert!(
        matches!(
            h.state().cur.renaming,
            Some((menus_ui::Renaming::Body(_), _))
        ),
        "ボディの名前を変える窓"
    );
    assert!(!h.state().is_editing(last));
}

#[test]
fn folder_eyes_hide_sketches_and_bodies_and_keep_the_row_eyes() {
    let part = kmb_samples::reference_part();
    let (sketch, body) = (part.top_sketch, part.hole_pattern);
    let mut ws = Workspace::new(part.doc, None, reg());
    // 使い終わったスケッチは自動で隠れるので、1 つ出しておく
    ws.set_hidden(sketch, false);
    let lines = ws.sketch_lines().len();
    assert!(lines > 0);
    let sketch_name = ws.document().data().feature(sketch).unwrap().name.clone();
    let mut h = harness(ws);
    // ボディのまとまりの目: 中のボディを全部隠す。行の目は開いたまま(薄く出る)
    h.get_by_label("ボディ を隠す").click();
    h.run();
    assert!(h.state().is_hidden(body));
    assert!(h.state().cur.scene.is_empty());
    h.get_by_label("ボディ1 を隠す");
    assert!(h.state().is_folder_hidden(browser_ui::Kind::Body));
    h.get_by_label("ボディ を出す").click();
    h.run();
    assert!(!h.state().is_hidden(body) && !h.state().cur.scene.is_empty());
    // スケッチのまとまりの目: 3D のビューのスケッチの線が消え、出すと戻る
    h.get_by_label("スケッチ を隠す").click();
    h.run();
    assert!(h.state().sketch_lines().is_empty());
    h.get_by_label(&format!("{sketch_name} を隠す"));
    h.get_by_label("スケッチ を出す").click();
    h.run();
    assert_eq!(h.state().sketch_lines().len(), lines);
    // まとまりの目を押してもまとまりは畳まない
    h.get_by_label(&format!("{sketch_name} を隠す"));
    // 原点の目: 既定は隠す。開けると原点の平面をいつも出す
    assert!(!h.state().is_origin_shown());
    h.get_by_label("原点 を出す").click();
    h.run();
    assert!(h.state().is_origin_shown());
    h.get_by_label("原点 を隠す").click();
    h.run();
    assert!(!h.state().is_origin_shown());
}

#[test]
fn the_design_timeline_menus_and_rollback_buttons_leave_out_machining() {
    // 1 つの加工の見本の画面で順に確かめる:
    // - 設計のタイムラインとコマンド検索に加工を出さない(元 the_design_timeline_and_menus_leave_out_machining)
    // - ロールバックのボタンは加工の機能を飛ばす(元 the_rollback_buttons_step_over_machining_features)
    let cam = kmb_samples::reference_cam();
    let contour_name = cam
        .part
        .doc
        .data()
        .feature(cam.contour)
        .unwrap()
        .name
        .clone();
    let setup_name = cam.part.doc.data().feature(cam.setup).unwrap().name.clone();
    let ws = Workspace::new(cam.part.doc, None, reg());
    // コマンド検索: 設計では加工の操作(ポケットなど)を出さず、加工では出す
    let cam_found = |ws: &Workspace| {
        ws.find_commands("ぽけっと")
            .iter()
            .any(|c| matches!(&c.id, CommandId::Action(a) if a.starts_with("cam.")))
    };
    assert!(!cam_found(&ws));
    let mut h = harness(ws);
    h.state_mut().clear_selection();
    h.run();
    // 設計のタイムラインに加工の札を出さない(ブラウザにも出ない)
    assert!(h.query_by_label(&setup_name).is_none());
    assert!(h.query_by_label(&contour_name).is_none());
    h.get_by_label("今の位置の印");
    // 機能の数も設計の物だけ
    let design = h
        .state()
        .document()
        .data()
        .features
        .iter()
        .filter(|f| !h.state().is_cam_feature(&f.type_id))
        .count();
    h.get_by_label(&format!("{design} 機能"));
    h.state_mut().switch_space(Space::Machining);
    h.run();
    assert!(cam_found(h.state()));

    // ロールバックのボタン(作業空間に関わらない)
    let ws = h.state_mut();
    let data = ws.document().data().clone();
    let n = data.features.len();
    let last_design = data
        .features
        .iter()
        .rposition(|f| !ws.is_cam_feature(&f.type_id))
        .unwrap();
    assert!(last_design + 1 < n, "見本は設計の後ろに加工がある");
    // 1 つ戻すと、いちばん後ろの設計の機能が無効になる(後ろの加工も一緒に)
    assert_eq!(ws.rollback_target(&data, -1), last_design);
    assert!(ws.step_rollback(last_design as i32 - n as i32));
    let data = ws.document().data().clone();
    // 1 つ進めると、後ろに設計の機能が無いので最後まで(加工も有効)
    assert_eq!(ws.rollback_target(&data, 1), n);
    assert_eq!(ws.rollback_target(&data, -(n as i32)), 0);
    assert_eq!(ws.rollback_target(&data, n as i32), n);
}

#[test]
fn the_marker_is_dragged_to_roll_back_and_forward() {
    let part = kmb_samples::reference_part();
    let boss = part.boss;
    let boss_name = part.doc.data().feature(boss).unwrap().name.clone();
    let n = part.doc.data().features.len();
    let mut h = harness(Workspace::new(part.doc, None, reg()));
    // 最後まで有効な時も印がある
    let marker = h.get_by_label("今の位置の印").rect();
    let chip = h
        .query_all_by_label(&boss_name)
        .find(|c| c.rect().top() > 700.0)
        .expect("タイムラインの札")
        .rect();
    // 印をボスの札の右へ引くと、ボスの後ろまで戻す
    h.drag_at(marker.center());
    h.run();
    h.hover_at(marker.center() - egui::vec2(20.0, 0.0));
    h.run();
    h.hover_at(chip.right_center() + egui::vec2(2.0, 0.0));
    h.run();
    h.drop_at(chip.right_center() + egui::vec2(2.0, 0.0));
    h.run();
    let data = h.state().document().data().clone();
    assert_eq!(data.active_len(), data.index_of(boss).unwrap() + 1);
    // 印は戻した位置に出て、最後へ引くとすべて有効に戻る
    let marker = h.get_by_label("今の位置の印").rect();
    assert!((marker.center().x - chip.right()).abs() < 20.0);
    h.drag_at(marker.center());
    h.run();
    h.hover_at(marker.center() + egui::vec2(30.0, 0.0));
    h.run();
    h.hover_at(marker.center() + egui::vec2(400.0, 0.0));
    h.run();
    h.drop_at(marker.center() + egui::vec2(400.0, 0.0));
    h.run();
    assert_eq!(h.state().document().data().active_len(), n);
    assert!(h.state().document().data().rollback.is_none());
}

#[test]
fn a_face_picked_or_pointed_in_3d_marks_its_body_row() {
    // 3D で面を選ぶ・指すと、そのボディ(最初に作った機能)を印の対象にする。選びは変えない
    let part = kmb_samples::reference_part();
    let pattern = part.hole_pattern;
    let mut ws = Workspace::new(part.doc, None, reg());
    ws.wait();
    let (_, root) = ws.body_roots()[0];
    assert!(ws.marked_bodies().is_empty());
    // 穴のパターンが作った面を選ぶと、たどった元のボディが印の対象
    ws.cur.picked = Some(kmb_core::FacePick {
        feature: pattern,
        face: format!("f{}:x", pattern.0),
    });
    assert_eq!(ws.marked_bodies(), vec![root]);
    assert!(!ws.selection().contains(&root), "選びは変えない");
    // 指しているだけでも同じ(選んだ物と重なっても 1 つ)
    ws.cur.picked = None;
    ws.cur.hover_feature = Some(pattern);
    assert_eq!(ws.marked_bodies(), vec![root]);
    // 指すのはマウスが 3D のビューを離れると消えるので、描く試験は選んだ面で確かめる
    ws.cur.picked = Some(kmb_core::FacePick {
        feature: pattern,
        face: format!("f{}:x", pattern.0),
    });
    let mut h = harness(ws);
    h.run();
    assert_eq!(h.state().browser.marked, vec![root], "ブラウザを描くと印の対象を集める");
}
