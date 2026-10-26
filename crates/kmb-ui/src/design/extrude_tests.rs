//! 押し出しの画面の試験: 領域のクリック、重なった形の領域、作った後の領域の選び直し、止める面、
//! 高さを決める間に面に載せて止める、線だけのスケッチの薄い押し出し。

use crate::tests::{can, on_truck, reg};
use crate::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kmb_samples::sample_document;

#[test]
fn extrude_button_makes_a_body_and_3d_view_shows_it() {
    // 幅 1000 では「作成」のまとまりが ▾ 1 つに畳まれ、押し出しのボタンが帯に出ない
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 700.0))
        .build_ui_state(
            |ui, ws: &mut Workspace| ws.ui(ui),
            Workspace::new(sample_document(), None, reg()),
        );
    h.run();
    // 押し出しはクリックで作る: ボタン → 領域を選ぶ → 高さをマウスか数字で決める → Enter で確定
    // (最後の値を数字と Enter で決めたら)
    h.get_by_label("押し出し").click();
    h.run();
    assert!(h.state().cur.gesture.is_some(), "押し出しの流れに入る");
    assert_eq!(h.state().document().data().features.len(), 1);
    // 板と穴の円の中の 2 つの領域があるので、領域を選ぶ手順から。選ばずに Enter でスケッチ全体
    // (穴は穴のまま)
    assert!(matches!(
        h.state()
            .cur
            .gesture
            .as_ref()
            .unwrap()
            .current()
            .unwrap()
            .gesture,
        kmb_core::Gesture::Regions
    ));
    h.key_press(egui::Key::Enter);
    h.run();
    assert!(matches!(
        h.state()
            .cur
            .gesture
            .as_ref()
            .unwrap()
            .current()
            .unwrap()
            .gesture,
        kmb_core::Gesture::Distance { .. }
    ));
    h.event(egui::Event::Text("10".into()));
    h.key_press(egui::Key::Enter);
    h.run();
    let ws = h.state();
    assert!(ws.cur.gesture.is_none());
    assert_eq!(ws.document().data().features.len(), 2);
    assert_eq!(ws.report().error_count(), 0);
    let bodies = ws.visible_bodies();
    assert_eq!(bodies.len(), 1);
    let m = ws.cur.work.kernel().mass(bodies[0].1).unwrap();
    let v = 60.0 * 40.0 * 10.0 - std::f64::consts::PI * 16.0 * 10.0;
    assert!((m.volume - v).abs() < 1e-3 * v, "{}", m.volume);
    // OK で決めると編集パネルは閉じる。開き直すと設定欄が出る
    h.get_by_label("全体表示");
    assert!(h.query_by_label("距離").is_none());
    let last = h.state().document().data().features.last().unwrap().id;
    h.state_mut().open_edit(last);
    h.run();
    h.get_by_label("距離");
    assert!(!h.state().cur.scene.is_empty());
}

#[test]
fn the_extrude_button_asks_for_a_profile_when_nothing_is_selected() {
    // 閉じた輪郭のあるスケッチを先に選ばなくても押せ、押してから輪郭を選べる
    let part = kmb_samples::reference_part();
    let mut ws = Workspace::new(part.doc, None, reg());
    ws.clear_selection();
    assert!(
        can(&ws, kmb_cad_extrude::EXTRUDE_SKETCH),
        "何も選ばずに押せる"
    );
    let n = ws.document().data().features.len();
    ws.start_action(kmb_cad_extrude::EXTRUDE_SKETCH);
    let g = ws.cur.gesture.as_ref().expect("領域を選ぶ手順に入る");
    let h = g.current().unwrap();
    assert!(matches!(h.gesture, kmb_core::Gesture::Regions));
    assert!(h.choices.len() > 1, "押し出せるスケッチの領域が候補に出る");
    assert!(h.choices.iter().all(|c| !c.outline.is_empty()));
    let pick = h
        .choices
        .iter()
        .find(|c| c.label == "ボスの輪郭")
        .expect("ボスの輪郭が候補にある")
        .value
        .clone();
    ws.gesture_commit_step(serde_json::json!([pick.clone()]));
    let g = ws.cur.gesture.as_ref().unwrap();
    assert!(matches!(
        g.current().unwrap().gesture,
        kmb_core::Gesture::Distance { .. }
    ));
    assert!(
        !g.current().unwrap().outline.is_empty(),
        "高さの手順は選んだ輪郭で出し直す"
    );
    // 高さを決めたら、そのまま作る(もう一度クリックや Enter を求めない)
    ws.gesture_commit_step(serde_json::json!(5.0));
    assert!(ws.cur.gesture.is_none(), "最後の手順を決めたら作る");
    let id = ws.selected().expect("作ったものを選ぶ");
    assert_eq!(ws.document().data().features.len(), n + 1);
    let d = &ws.document().data().feature(id).unwrap().data;
    assert_eq!(d["profile"], pick["profile"]);
    assert!(
        d.get("regions").is_none(),
        "領域が 1 つのスケッチは名前を覚えない"
    );
    assert_eq!(d["distance"], "5 mm");
    assert_eq!(
        ws.creating(),
        Some(id),
        "作っている途中のまま、欄で値を直せる"
    );
    ws.finish_edit();
    assert_eq!(ws.creating(), None, "OK で決まる");
    // 流れを見る試験なので、truck のブーリアンの弱さ(同じ平面で接する面)は問わない
    if !on_truck() {
        assert_eq!(ws.report().error_count(), 0, "{:?}", ws.report());
    }
}

/// 円(半径 10)と、右半分に重ねた長方形(0〜25 × -5〜5)を 1 つのスケッチに描いた文書(前方後円墳)。
#[test]
fn re_picking_the_profile_before_ok_shows_the_first_sketch_again() {
    // 作る途中(OK の前)に輪郭を別のスケッチへ選び替えると、足した時に隠したスケッチが出る。
    // OK の後に選び替えても出し直さない
    let part = kmb_samples::reference_part();
    let mut ws = Workspace::new(part.doc, None, reg());
    let s = ws
        .document()
        .data()
        .feature(part.plate_sketch)
        .unwrap()
        .clone();
    let mut copy = || {
        ws.cur
            .work
            .add_feature(&s.type_id, None, s.data.clone())
            .unwrap()
    };
    let (s2, s3) = (copy(), copy());
    ws.after_change(Ok(()));
    ws.wait();
    ws.clear_selection();
    ws.start_action(kmb_cad_extrude::EXTRUDE_SKETCH);
    let pick = ws
        .cur
        .gesture
        .as_ref()
        .unwrap()
        .current()
        .unwrap()
        .choices
        .iter()
        .find(|c| c.value["profile"] == serde_json::json!(s2.0))
        .expect("足したスケッチの領域が候補にある")
        .value
        .clone();
    ws.gesture_commit_step(serde_json::json!([pick]));
    ws.gesture_commit_step(serde_json::json!(5.0));
    let id = ws.creating().expect("作っている途中");
    assert!(ws.is_own_hidden(s2));
    let profile = |ws: &mut Workspace, sketch: FeatureId| {
        let mut d = ws.document().data().feature(id).unwrap().data.clone();
        d["profile"] = serde_json::json!(sketch.0);
        d.as_object_mut().unwrap().remove("regions");
        ws.set_feature_data(id, d);
    };
    profile(&mut ws, s3);
    assert!(ws.is_own_hidden(s3));
    assert!(!ws.is_own_hidden(s2), "選び替えた前のスケッチが隠れたまま");
    assert!(!ws.is_hidden(s2), "3D に出ない");
    ws.finish_edit();
    profile(&mut ws, s2);
    assert!(ws.is_own_hidden(s3), "OK の後に出し直した");
}

fn keyhole_document() -> kmb_core::Document {
    use kmb_core::{Document, FeatureEntry, Op, Transaction};
    let mut s = kmb_sketch::Sketch::default();
    let c = s.add_point(0.0, 0.0);
    s.add_circle(c, 10.0);
    s.add_rectangle((None, [0.0, -5.0]), (None, [25.0, 5.0]))
        .unwrap();
    let mut doc = Document::new();
    let f = doc.alloc_feature_id();
    doc.apply(Transaction::new(
        "スケッチ",
        vec![Op::InsertFeature {
            index: None,
            entry: FeatureEntry {
                id: f,
                type_id: kmb_sketch::SKETCH.into(),
                name: "スケッチ1".into(),
                data: serde_json::to_value(s).unwrap(),
                suppressed: false,
            },
        }],
    ))
    .unwrap();
    doc
}

#[test]
fn overlapping_shapes_in_a_sketch_are_extruded_by_the_regions_clicked() {
    // 円と長方形を重ねると領域に分け、押し出せる(分けないと何も出ない)
    let lens = 2.0 * (0.5 * 5.0 * 75f64.sqrt() + 0.5 * 100.0 * (0.5f64).asin());
    let circle = std::f64::consts::PI * 100.0;
    let extrude = |picks: &[usize]| {
        let mut ws = Workspace::new(keyhole_document(), None, reg());
        ws.clear_selection();
        ws.start_action(kmb_cad_extrude::EXTRUDE_SKETCH);
        let g = ws.cur.gesture.as_ref().expect("領域を選ぶ手順に入る");
        let h = g.current().unwrap();
        assert!(matches!(h.gesture, kmb_core::Gesture::Regions));
        assert_eq!(h.choices.len(), 3, "円だけ・重なり・長方形だけ");
        // 領域の面積の順に並べて選ぶ(0: 重なり 95.7、1: 長方形だけ 154.3、2: 円だけ 218.5)
        let mut by_area: Vec<(f64, serde_json::Value)> = h
            .choices
            .iter()
            .map(|c| {
                let pts: Vec<[f64; 2]> = c.outline[0].iter().map(|p| [p[0], p[1]]).collect();
                (kmb_core::kernel::polygon_area(&pts).abs(), c.value.clone())
            })
            .collect();
        by_area.sort_by(|a, b| a.0.total_cmp(&b.0));
        let v: Vec<serde_json::Value> = picks.iter().map(|&k| by_area[k].1.clone()).collect();
        ws.gesture_commit_step(serde_json::Value::Array(v));
        ws.gesture_commit_step(serde_json::json!(10.0));
        assert!(ws.cur.gesture.is_none());
        ws.finish_edit();
        let errors: Vec<String> = ws
            .report()
            .features
            .iter()
            .filter_map(|f| match &f.status {
                kmb_core::FeatureStatus::Error { message } => Some(message.clone()),
                _ => None,
            })
            .collect();
        assert!(errors.is_empty(), "{picks:?}: {errors:?}");
        let bodies = ws.visible_bodies();
        assert_eq!(bodies.len(), 1);
        ws.cur.work.kernel().mass(bodies[0].1).unwrap().volume
    };
    let tol = 0.01;
    let v = extrude(&[0, 2]);
    assert!(
        (v - circle * 10.0).abs() < tol * v,
        "円だけと重なり = 円: {v}"
    );
    let v = extrude(&[0, 1]);
    assert!(
        (v - 250.0 * 10.0).abs() < tol * v,
        "重なりと長方形だけ = 長方形: {v}"
    );
    let v = extrude(&[0, 1, 2]);
    let union = (circle + 250.0 - lens) * 10.0;
    assert!((v - union).abs() < tol * v, "全部 = 円と長方形の和: {v}");
}

#[test]
fn the_regions_of_a_made_extrude_are_picked_again_in_the_view() {
    // 作った後に押し出す領域を 3D で選び直せる(作り直さずに済む。X1 の 4)
    let mut ws = Workspace::new(keyhole_document(), None, reg());
    ws.clear_selection();
    ws.start_action(kmb_cad_extrude::EXTRUDE_SKETCH);
    let first = ws.cur.gesture.as_ref().unwrap().current().unwrap().choices[0]
        .value
        .clone();
    ws.gesture_commit_step(serde_json::json!([first.clone()]));
    ws.gesture_commit_step(serde_json::json!(10.0));
    ws.finish_edit();
    let id = ws.cur.selected.expect("作った押し出しを選んでいる");
    let volume = |ws: &Workspace| {
        let b = ws.visible_bodies();
        ws.cur.work.kernel().mass(b[0].1).unwrap().volume
    };
    let one = volume(&ws);
    // 右クリック(と編集の欄)の「領域を選び直す」: 今の領域を選んだまま始まる
    assert!(
        ws.actions_for(id)
            .iter()
            .any(|a| a.id == kmb_cad_extrude::EXTRUDE_REGIONS
                && a.is_at(kmb_core::Place::FeatureMenu))
    );
    ws.start_action(kmb_cad_extrude::EXTRUDE_REGIONS);
    let g = ws.cur.gesture.as_ref().expect("領域を選ぶ手順に入る");
    assert_eq!(g.picks, vec![first]);
    let all: Vec<serde_json::Value> = g
        .current()
        .unwrap()
        .choices
        .iter()
        .map(|c| c.value.clone())
        .collect();
    assert_eq!(all.len(), 3);
    ws.gesture_commit_step(serde_json::Value::Array(all));
    assert!(ws.cur.gesture.is_none());
    let data = ws.cur.work.doc().data().feature(id).unwrap().data.clone();
    // 全部を選ぶとスケッチ全体と同じなので、領域を持たず全体として残す
    assert!(
        data["regions"].as_array().is_none_or(|a| a.is_empty()),
        "{data}"
    );
    assert!(volume(&ws) > one * 2.0, "{} {}", volume(&ws), one);
    // 1 回の取り消しで前の領域に戻る
    ws.undo();
    assert!((volume(&ws) - one).abs() < 1e-6);
}

/// 採用分 10-20 f27: スケッチの線を変えて押し出しの領域が無くなっても、黙って別の領域に付け替えない。
/// 押し出しは理由つきで止まり、「領域を選び直す」を始めると、囲む線のいちばん近い領域を選んだ形で
/// 見せ、Enter で決めると直る。
#[test]
fn a_lost_region_is_offered_again_from_its_nearest_lines_and_not_swapped_silently() {
    let mut ws = Workspace::new(keyhole_document(), None, reg());
    ws.clear_selection();
    ws.start_action(kmb_cad_extrude::EXTRUDE_SKETCH);
    let first = ws.cur.gesture.as_ref().unwrap().current().unwrap().choices[0]
        .value
        .clone();
    ws.gesture_commit_step(serde_json::json!([first.clone()]));
    ws.gesture_commit_step(serde_json::json!(10.0));
    ws.finish_edit();
    let id = ws.cur.selected.expect("作った押し出しを選んでいる");
    // 線を変えた後と同じく、覚えている領域の名前の区間の 1 つがスケッチに無い名前になる
    let key = first["key"].as_str().unwrap().to_string();
    let mut segs: Vec<&str> = key.split('+').collect();
    segs.pop();
    segs.push("e99");
    let stale = segs.join("+");
    let mut data = ws.cur.work.doc().data().feature(id).unwrap().data.clone();
    data["regions"] = serde_json::json!([stale]);
    assert!(ws.set_feature_data(id, data));
    let why = match ws.report().feature(id) {
        Some(kmb_core::FeatureStatus::Error { message }) => message.clone(),
        other => panic!("止まる: {other:?}"),
    };
    assert!(why.contains("選び直して"), "{why}");
    // 選び直しの手順: 線の近い元の領域を選んだ形で始まり、理由を手順の説明に出す
    ws.select(id);
    ws.start_action(kmb_cad_extrude::EXTRUDE_REGIONS);
    let g = ws.cur.gesture.as_ref().expect("領域を選ぶ手順に入る");
    assert_eq!(g.picks, vec![first.clone()]);
    let label = g.current().unwrap().label.clone();
    assert!(label.contains("前の領域がありません"), "{label}");
    // まだ決めていないので、押し出しは止まったまま(黙って付け替えない)
    assert!(!matches!(
        ws.report().feature(id),
        Some(kmb_core::FeatureStatus::Ok { .. })
    ));
    ws.gesture_commit_step(serde_json::json!([first]));
    assert!(matches!(
        ws.report().feature(id),
        Some(kmb_core::FeatureStatus::Ok { .. })
    ));
    let data = ws.cur.work.doc().data().feature(id).unwrap().data.clone();
    assert_eq!(data["regions"], serde_json::json!([key]));
}

#[test]
fn regions_touching_only_at_a_point_are_refused_with_a_reason() {
    // 円だけの所と長方形だけの所は、円と長方形の交点でだけ接する(1 つの立体にならない)
    let mut ws = Workspace::new(keyhole_document(), None, reg());
    ws.clear_selection();
    ws.start_action(kmb_cad_extrude::EXTRUDE_SKETCH);
    let h = ws.cur.gesture.as_ref().unwrap().current().unwrap().clone();
    let rect_only = h
        .choices
        .iter()
        .find(|c| c.value["key"].as_str().unwrap().contains("e8"))
        .map(|c| c.value["key"].as_str().unwrap().to_string());
    let picks: Vec<serde_json::Value> = h
        .choices
        .iter()
        .filter(|c| {
            let k = c.value["key"].as_str().unwrap();
            // 長方形だけ(e8 を含む)と、円だけ(円の 0 番目の区間を含む)
            k.contains("e8") || k.starts_with("e2.0")
        })
        .map(|c| c.value.clone())
        .collect();
    assert!(rect_only.is_some());
    assert_eq!(picks.len(), 2);
    ws.gesture_commit_step(serde_json::Value::Array(picks));
    // 高さの手順は出ない(押し出す輪郭が作れない)ので、決めると理由を知らせる
    ws.gesture_commit_step(serde_json::json!(10.0));
    let m = ws.message().unwrap_or_default().to_string();
    let errors: Vec<String> = ws
        .report()
        .features
        .iter()
        .filter_map(|f| match &f.status {
            kmb_core::FeatureStatus::Error { message } => Some(message.clone()),
            _ => None,
        })
        .collect();
    // 理由は知らせの文に出し、機能は足さない(誤りの機能も残らない)。接する点は円と長方形の交点 (5√3, 5)
    assert!(
        m.contains("点 (8.66, 5.00) で接していて、1 つの形になりません"),
        "{m}"
    );
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn an_extrude_is_set_to_stop_at_a_clicked_face_from_the_menu_or_the_panel() {
    // 1 つの文書で順に確かめる(段ごとに取り消しかキャンセルで元へ戻す):
    // - 右クリックの「対象まで」で、クリックした面まで押し出し、後で出来た面は断る
    //   (元 an_extrude_is_set_to_stop_at_a_clicked_face)
    // - 手で選んだ演算は、面までにしても替えない
    //   (元 an_operation_picked_by_hand_is_not_changed_when_stopping_at_a_face)
    // - 編集パネルで「対象まで(面)」を選ぶと面を選ぶ手順に入り、直している途中のまま
    //   (元 choosing_to_face_in_the_panel_starts_the_face_pick_and_keeps_editing)
    let mut ws = Workspace::new(kmb_samples::reference_document(), None, reg());
    let id_of = |ws: &Workspace, n: &str| {
        ws.document()
            .data()
            .features
            .iter()
            .find(|f| f.name == n)
            .map(|f| f.id)
            .unwrap()
    };
    let plate = id_of(&ws, "板");
    let boss = id_of(&ws, "ボス");
    ws.select(boss);
    // 右クリックのメニューに出て、押すと面をクリックする手順に入る
    assert!(
        ws.actions_for(boss)
            .iter()
            .any(|a| a.id == kmb_cad_extrude::EXTRUDE_TO_FACE && a.disabled.is_none())
    );
    ws.start_action(kmb_cad_extrude::EXTRUDE_TO_FACE);
    assert!(
        ws.cur
            .gesture
            .as_ref()
            .and_then(|g| g.current())
            .is_some_and(|h| matches!(h.gesture, kmb_core::Gesture::Face))
    );
    ws.cur.gesture = None;
    // 板の下の面まで(ボスより前からある面)
    ws.run_action_args(
        kmb_cad_extrude::EXTRUDE_TO_FACE,
        serde_json::json!({"face": format!("f{}:start", plate.0)}),
    );
    let data = ws.document().data().feature(boss).unwrap().data.clone();
    assert_eq!(data["extent"], "to_face", "{:?}", ws.message());
    // 板の下面まで下ろすと板の中を通るので、結合から切り取りに変わる
    assert_eq!(data["operation"], "cut");
    // 面はボスの直前のボディ(ボスの対象)から取る
    let to = FeatureId(data["to_face"]["feature"].as_u64().unwrap());
    let at = |id| ws.document().data().index_of(id).unwrap();
    assert!(at(to) < at(boss) && at(to) >= at(plate));
    assert_eq!(data["to_face"]["face"], format!("f{}:start", plate.0));
    // 形の計算(面までの距離)は kmb-solid の試験で確かめる。ここで選んだ板の下面までだと、ボスの
    // 底が板の下面と重なり、truck のカーネルではブーリアンに失敗するため、計算の成否は見ない
    // ボスより後で出来た面は断る
    let holes = id_of(&ws, "四隅の穴");
    ws.select(boss);
    ws.run_action_args(
        kmb_cad_extrude::EXTRUDE_TO_FACE,
        serde_json::json!({"face": format!("f{}:end", holes.0), "feature": holes.0}),
    );
    assert!(
        ws.message().is_some_and(|m| m.contains("後で出来た面")),
        "{:?}",
        ws.message()
    );
    // 面までにしたのを取り消す(断った方は何も変えていない)
    let boss_data = |ws: &Workspace| ws.document().data().feature(boss).unwrap().data.clone();
    ws.undo();
    let original = boss_data(&ws);
    assert_ne!(original["extent"], "to_face");
    ws.clear_selection();

    // 演算を手で選んだら、もう自動で替えない
    let find = |ws: &Workspace, n: &str| {
        ws.document()
            .data()
            .features
            .iter()
            .find(|f| f.name == n)
            .cloned()
            .unwrap()
    };
    ws.open_edit(boss);
    // 欄で切り取りにして、また結合に戻す(どちらも手で選んだ印が付く)
    for op in ["cut", "join"] {
        let entry = find(&ws, "ボス");
        let mut data = entry.data.clone();
        data["operation"] = serde_json::json!(op);
        ws.set_panel_data(&entry, data);
    }
    let data = boss_data(&ws);
    assert_eq!(data["op_manual"], true, "{data}");
    assert_eq!(data["operation"], "join");
    // 板の下面まで(板の中を通る)にしても、結合のまま
    ws.select(boss);
    ws.run_action_args(
        kmb_cad_extrude::EXTRUDE_TO_FACE,
        serde_json::json!({"face": format!("f{}:start", plate.0)}),
    );
    let data = boss_data(&ws);
    assert_eq!(data["extent"], "to_face", "{:?}", ws.message());
    assert_eq!(data["operation"], "join", "手で選んだ演算は替えない");
    // キャンセルで直し始める前(手で選んだ印の無いボス)へ戻す
    ws.cancel_edit();
    assert_eq!(boss_data(&ws), original);

    // 編集パネルの範囲の一覧で「対象まで(面)」を選ぶ: 面をクリックする手順に入り、直している途中のまま
    let boss = find(&ws, "ボス");
    ws.open_edit(boss.id);
    let mut data = boss.data.clone();
    data["extent"] = serde_json::json!("to_face");
    ws.set_panel_data(&boss, data);
    assert!(
        ws.cur
            .gesture
            .as_ref()
            .and_then(|g| g.current())
            .is_some_and(|h| matches!(h.gesture, kmb_core::Gesture::Face)),
        "面を選ぶ手順に入る"
    );
    assert!(ws.is_editing(boss.id));
    // 面を選ぶまでは距離のまま作り、道具の箱(輪郭と矢印)は消えない(2026-10-19 の写真撮りで、
    // 計算が失敗して箱が消え、面も選べずに詰まった)
    ws.wait();
    assert!(
        ws.output(boss.id, kmb_core::TOOL_PREVIEW).is_some(),
        "{:?}",
        ws.report().feature(boss.id)
    );
    // 板の上面(ボスの底と同じ平面)より上は無いので、板の下面をクリック: 切り取りになる
    ws.cur.picked = Some(kmb_core::FacePick {
        feature: plate,
        face: format!("f{}:start", plate.0),
    });
    ws.gesture_commit_step(serde_json::json!(format!("f{}:start", plate.0)));
    assert!(ws.cur.gesture.is_none());
    assert!(ws.is_editing(boss.id), "面を選んだ後も直している途中");
    let data = ws.document().data().feature(boss.id).unwrap().data.clone();
    assert_eq!(data["to_face"]["face"], format!("f{}:start", plate.0));
    assert_eq!(data["operation"], "cut");
    // 範囲が「対象まで」の間は、編集の欄に面の選び直しのボタンが出る
    assert!(
        ws.actions_for(boss.id)
            .iter()
            .any(|a| a.id == kmb_cad_extrude::EXTRUDE_TO_FACE
                && a.is_at(kmb_core::Place::Panel)
                && a.keep_edit)
    );
}

#[test]
fn hovering_a_face_while_pulling_the_height_extrudes_up_to_that_face() {
    // 確かめること:
    // - 高さを決める間に別のボディの平らな面へマウスを載せると、高さがその面までになり、
    //   クリックで範囲を「対象まで」(その面)にして作る(範囲を切り替える手間を省く)
    // - 押し出す向きに沿う面(横の壁)は止める面にしない(マウスの所の高さのまま)
    // - 数字を打てば打った値が先(面には止めない)
    // - CLI・MCP で知らない面を渡すと、黙って距離にせず断る
    use kmb_cad::{Direction, Operation};
    use kmb_cad_extrude::{EXTRUDE, Extrude};
    use kmb_samples::sample::{DocBuilder, SketchBuilder};
    use kmb_sketch::SketchPlane;
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0));
    let mut b = DocBuilder::new();
    let rectangle = |b: &mut DocBuilder, name: &str, [x, y, w, h]: [f64; 4]| {
        let mut s = SketchBuilder::new(SketchPlane::Xy);
        let o = s.fixed_point(0.0, 0.0);
        let c = s.point_from(o, &format!("{x} mm"), &format!("{y} mm"), [x, y]);
        s.rectangle(c, &format!("{w} mm"), &format!("{h} mm"), [x, y, w, h]);
        b.feature(kmb_sketch::SKETCH, name, s.build())
    };
    // 底が z 0 の 30×10×5 の台と、高さ 20 の柱(別のボディ)。柱の上の面(z 20)まで伸ばす
    let block = |b: &mut DocBuilder, name: &str, r: [f64; 4], z: f64| {
        let sk = rectangle(b, &format!("{name}のスケッチ"), r);
        b.feature(
            EXTRUDE,
            name,
            Extrude {
                profile: Some(sk),
                distance: format!("{z} mm"),
                direction: Direction::Normal,
                operation: Operation::New,
                ..Default::default()
            },
        )
    };
    block(&mut b, "台", [0.0, 0.0, 30.0, 10.0], 5.0);
    let post = block(&mut b, "柱", [20.0, 20.0, 6.0, 6.0], 20.0);
    let sketch = rectangle(&mut b, "伸ばすスケッチ", [0.0, 20.0, 6.0, 6.0]);
    let mut ws = Workspace::new(b.finish("台と柱"), None, reg());
    ws.wait();
    ws.cur.camera.fit(&ws.cur.scene, rect);
    let at = |ws: &Workspace, p: [f64; 3]| ws.cur.camera.project(p.map(|v| v as f32), rect).0;
    let hover = |p: egui::Pos2| crate::view3d::ViewEvents {
        hover: Some(p),
        ..Default::default()
    };
    ws.select(sketch);
    ws.start_action(kmb_cad_extrude::EXTRUDE_SKETCH);
    fn g(ws: &Workspace) -> &crate::gesture::Run {
        ws.cur.gesture.as_ref().unwrap()
    }
    assert!(matches!(
        g(&ws).current().unwrap().gesture,
        kmb_core::Gesture::Distance { .. }
    ));
    // 台の手前の面(向きに沿う横の壁): 止めない
    let wall = at(&ws, [15.0, 0.0, 2.5]);
    ws.gesture_events(&hover(wall), rect);
    assert!(g(&ws).stop.is_none(), "{:?}", g(&ws).stop);
    // 柱の上の面: 高さはその面まで
    let top = at(&ws, [23.0, 23.0, 20.0]);
    ws.gesture_events(&hover(top), rect);
    let face = format!("f{}:end", post.0);
    assert_eq!(
        g(&ws).stop,
        Some(serde_json::json!({"feature": post.0, "face": face}))
    );
    assert!(
        (g(&ws).live.as_ref().unwrap().as_f64().unwrap() - 20.0).abs() < 1e-6,
        "{:?}",
        g(&ws).live
    );
    assert!(g(&ws).hint().contains("面まで"), "{}", g(&ws).hint());
    // 数字を打っている間は面に止めない
    ws.cur.gesture.as_mut().unwrap().typed = "7".into();
    ws.gesture_events(&hover(top), rect);
    assert!(g(&ws).stop.is_none());
    ws.cur.gesture.as_mut().unwrap().typed.clear();
    // クリックで決める: 範囲が「対象まで」(柱の上の面)の押し出しができる
    ws.gesture_events(&hover(top), rect);
    ws.gesture_events(
        &crate::view3d::ViewEvents {
            hover: Some(top),
            click: Some(top),
            ..Default::default()
        },
        rect,
    );
    assert!(ws.cur.gesture.is_none(), "{:?}", ws.message());
    let made = ws.document().data().features.last().unwrap().clone();
    assert_eq!(made.type_id, EXTRUDE);
    assert_eq!(made.data["extent"], "to_face", "{}", made.data);
    assert_eq!(made.data["to_face"]["feature"], post.0);
    assert_eq!(made.data["to_face"]["face"], face);
    assert_eq!(made.data["direction"], "normal");
    assert_eq!(made.data["distance"], "20 mm");
    ws.cancel_edit();
    // CLI・MCP: 相手のボディに無い面は断る
    ws.select(sketch);
    let n = ws.document().data().features.len();
    ws.run_action_args(
        kmb_cad_extrude::EXTRUDE_SKETCH,
        serde_json::json!({"to_face": {"feature": post.0, "face": "f999:end"}}),
    );
    assert_eq!(ws.document().data().features.len(), n);
    assert!(
        ws.message()
            .is_some_and(|m| m.contains("ボディにありません")),
        "{:?}",
        ws.message()
    );
}

#[test]
fn a_sketch_of_open_lines_is_extruded_as_a_thin_wall() {
    // 線だけのスケッチ(閉じた輪郭が無い)も押し出せ、薄い押し出し(真ん中、2 mm)になる(M3)
    use kmb_samples::sample::{DocBuilder, SketchBuilder};
    use kmb_sketch::SketchPlane;
    let mut b = DocBuilder::new();
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let p0 = s.fixed_point(0.0, 0.0);
    let p1 = s.fixed_point(20.0, 0.0);
    let p2 = s.fixed_point(20.0, 10.0);
    s.line(p0, p1);
    s.line(p1, p2);
    b.feature(kmb_sketch::SKETCH, "線", s.build());
    let mut ws = Workspace::new(b.finish("薄い押し出し"), None, reg());
    ws.wait();
    ws.clear_selection();
    assert!(
        can(&ws, kmb_cad_extrude::EXTRUDE_SKETCH),
        "線だけでも押せる"
    );
    let n = ws.document().data().features.len();
    ws.start_action(kmb_cad_extrude::EXTRUDE_SKETCH);
    let h = ws.cur.gesture.as_ref().unwrap().current().unwrap().clone();
    assert!(
        matches!(h.gesture, kmb_core::Gesture::Distance { .. }),
        "{:?}",
        h.label
    );
    assert!(!h.outline.is_empty(), "壁の下絵を描く");
    ws.gesture_commit_step(serde_json::json!(5.0));
    ws.command_ok();
    assert!(ws.cur.gesture.is_none());
    assert_eq!(ws.document().data().features.len(), n + 1);
    let id = ws.selected().expect("作ったものを選ぶ");
    let d = ws.document().data().feature(id).unwrap().data.clone();
    assert_eq!(d["thin"], "center", "{d}");
    ws.wait();
    let (_, body) = *ws.visible_bodies().last().unwrap();
    let v = ws.cur.work.kernel().mass(body).unwrap().volume;
    let want = (20.0 + 10.0) * 2.0 * 5.0;
    assert!((v - want).abs() < 1e-6, "{v} {want}");
}

#[test]
fn the_quick_profile_check_agrees_with_reading_the_profile() {
    // 操作の一覧は 1 コマごとに輪郭を読まずに見る(機能が多いと重い。10-24 v10)。読んだ時と同じ答え
    use kmb_samples::sample::{DocBuilder, SketchBuilder};
    use kmb_sketch::SketchPlane;
    let mut b = DocBuilder::new();
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let p0 = s.fixed_point(0.0, 0.0);
    let p1 = s.fixed_point(20.0, 0.0);
    s.line(p0, p1);
    b.feature(kmb_sketch::SKETCH, "線", s.build());
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let o = s.fixed_point(0.0, 0.0);
    s.rectangle(o, "10 mm", "10 mm", [0.0, 0.0, 10.0, 10.0]);
    b.feature(kmb_sketch::SKETCH, "四角", s.build());
    let mut ws = Workspace::new(b.finish("輪郭の有無"), None, reg());
    ws.wait();
    let cx = ws.context_with(None, None);
    let ids: Vec<_> = cx.data.features.iter().map(|f| f.id).collect();
    let quick: Vec<bool> = ids
        .iter()
        .map(|id| kmb_cad::regions::has_profile(&cx, *id))
        .collect();
    let read: Vec<bool> = ids
        .iter()
        .map(|id| kmb_cad::regions::profile_of(&cx, *id).is_some())
        .collect();
    assert_eq!(quick, vec![false, true]);
    assert_eq!(quick, read);
}

/// 10-26 k15: 開始をずらした押し出しを「対象まで」にすると、結合か切り取りかを、ずらした開始の
/// 所から面までの間で決める。板(高さ 0〜10)の底の平面に描いた輪郭を、20 浮かせた所から下へ
/// 結合で押し出し、板の上の面までにする: 20 から 10 までは板の外なので結合のまま。
/// 直す前は輪郭の平面(高さ 0)から測り、板の中を通ると見て切り取りに変えていた。
#[test]
fn stopping_an_offset_extrude_at_a_face_judges_the_operation_from_its_start() {
    use kmb_samples::sample::{DocBuilder, SketchBuilder};
    let square = |b: &mut DocBuilder, name: &str, x: f64, w: f64| {
        let mut s = SketchBuilder::xy();
        let o = s.fixed_point(0.0, 0.0);
        let c = s.point_from(o, &format!("{x} mm"), &format!("{x} mm"), [x, x]);
        s.rectangle(c, &format!("{w} mm"), &format!("{w} mm"), [x, x, w, w]);
        b.sketch(name, s)
    };
    let mut b = DocBuilder::new();
    let ps = square(&mut b, "板の輪郭", 0.0, 30.0);
    let plate = b.feature(
        kmb_cad_extrude::EXTRUDE,
        "板",
        serde_json::json!({"profile": ps.0, "distance": "10 mm"}),
    );
    let bs = square(&mut b, "柱の輪郭", 10.0, 10.0);
    let post = b.feature(
        kmb_cad_extrude::EXTRUDE,
        "浮いた柱",
        serde_json::json!({
            "profile": bs.0, "distance": "5 mm", "direction": "reverse",
            "start": "offset", "start_offset": "20 mm",
            "operation": "join", "target": plate.0,
        }),
    );
    let mut ws = Workspace::new(b.finish("浮いた柱"), None, reg());
    ws.wait();
    ws.select(post);
    ws.run_action_args(
        kmb_cad_extrude::EXTRUDE_TO_FACE,
        serde_json::json!({"face": format!("f{}:end", plate.0)}),
    );
    let data = ws.document().data().feature(post).unwrap().data.clone();
    assert_eq!(data["extent"], "to_face", "{:?}", ws.message());
    assert_eq!(data["operation"], "join", "20 から 10 までは板の外");
}
