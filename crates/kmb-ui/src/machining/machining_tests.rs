//! 加工の作業空間の通しの試験(セットアップ・操作・経路・シミュレーション・G コード)。

use crate::tests::{FakeDialogs, add_op, can, first_of, path_lines, reg, temp_dir};
use crate::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kmb_samples::{reference_cam_document, reference_document};

#[test]
fn cam_setup_shows_toolpaths_and_writes_gcode() {
    let dir = std::env::temp_dir().join(format!("kmb-ui-cam-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("part.json");
    let mut ws = Workspace::new(reference_cam_document(), Some(path.clone()), reg());
    assert_eq!(ws.report().error_count(), 0, "{:?}", ws.report().features);
    let (setup, name) = ws
        .document()
        .data()
        .features
        .iter()
        .find(|f| f.type_id == kmb_cam_commands::SETUP)
        .map(|f| (f.id, f.name.replace(' ', "")))
        .unwrap();
    ws.select(setup);
    let lines = path_lines(&ws);
    assert!(lines.iter().any(|l| l.1) && lines.iter().any(|l| !l.1));
    ws.run_action(kmb_cam_commands::EXPORT_GCODE);
    // ファイル名は内部の番号でなく、セットアップの名前
    let nc = dir.join(format!("part-{name}.nc"));
    let g = std::fs::read_to_string(&nc).unwrap();
    assert!(g.contains("M6 T2"), "{:?}", ws.message());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn adding_a_setup_and_a_face_operation() {
    let mut ws = Workspace::new(reference_document(), None, reg());
    let face = add_op(kmb_strategy_face::FACE);
    assert!(!can(&ws, face));
    ws.run_action(kmb_cam_commands::ADD_SETUP).unwrap();
    assert!(can(&ws, face));
    assert!(
        !can(&ws, add_op(kmb_strategy_pocket2d::POCKET)),
        "面を選ぶまではポケットを足せない"
    );
    let id = ws.run_action(face).unwrap();
    assert!(
        matches!(ws.report().feature(id), Some(FeatureStatus::Ok { .. })),
        "{:?}",
        ws.report().feature(id)
    );
    assert!(!path_lines(&ws).is_empty());
}

#[test]
fn a_broken_face_reference_stops_with_a_reason_and_can_be_repaired() {
    let cam = kmb_samples::reference_cam();
    let (pocket, op, plate, body) = (
        cam.part.pocket,
        cam.pocket,
        cam.part.plate,
        cam.part.hole_pattern,
    );
    let mut ws = Workspace::new(cam.part.doc, None, reg());
    assert_eq!(ws.report().error_count(), 0);
    assert!(ws.set_suppressed(pocket, true));
    match ws.report().feature(op) {
        Some(FeatureStatus::Error { message }) => {
            assert!(message.contains("参照が切れた"), "{message}")
        }
        other => panic!("止まるはず: {other:?}"),
    }
    let setup = kmb_cam_commands::setups(ws.document().data(), ws.report(), &kmb_bundle::registry())
        .into_iter()
        .next()
        .unwrap();
    assert!(
        kmb_cam_commands::gcode(
            &setup,
            &kmb_post_carvera_air::CarveraAir,
            &kmb_post_carvera_air::profile()
        )
        .is_err()
    );
    assert!(
        !ws.can_repair_with_picked(op),
        "面を選ぶまでは付け替えられない"
    );
    ws.set_picked(Some(view3d::Pick {
        feature: body,
        face: format!("f{}:end", plate.0),
    }));
    assert!(ws.can_repair_with_picked(op));
    assert!(ws.repair_with_picked(op));
    assert!(
        matches!(ws.report().feature(op), Some(FeatureStatus::Ok { .. })),
        "{:?}",
        ws.report().feature(op)
    );
    assert_eq!(ws.report().error_count(), 0);
    // 付け替えは 1 回の取り消しで戻る
    ws.undo();
    assert!(matches!(
        ws.report().feature(op),
        Some(FeatureStatus::Error { .. })
    ));
}

#[test]
fn the_simulation_runs_in_the_background_and_reports_back() {
    let mut ws = Workspace::open(reference_cam_document(), None, true, reg());
    ws.wait();
    let setup = ws
        .document()
        .data()
        .features
        .iter()
        .find(|f| f.type_id == kmb_cam_commands::SETUP)
        .unwrap()
        .id;
    ws.select(setup);
    ws.run_action(kmb_cam_commands::SIMULATE);
    assert!(ws.is_running());
    assert_eq!(ws.message(), Some("計算中…"));
    // 下の帯に経った秒数を出すため、走り始めた時刻を持つ(U66)
    assert!(ws.task_for().is_some());
    let start = std::time::Instant::now();
    while ws.is_running() {
        assert!(
            start.elapsed().as_secs() < 300,
            "シミュレーションが終わらない"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
        ws.poll_task();
    }
    let m = ws.message().unwrap();
    assert!(m.contains("食い込み 0 件"), "{m}");
    assert!(ws.task_for().is_none());
}

#[test]
fn the_setup_shows_its_stock_origin_and_machine_notes() {
    let mut ws = Workspace::new(reference_cam_document(), None, reg());
    let setup = first_of(&ws, kmb_cam_commands::SETUP);
    ws.select(setup);
    let o = ws.overlay();
    let of = |o: &kmb_core::Overlay, k| o.boxes.iter().find(|b| b.2.ink == k).map(|b| (b.0, b.1));
    let (lo, hi) = of(&o, kmb_interchange::ink::boxes::WORK).unwrap();
    // 基準部品 60 × 40 × 15 mm に、横 2 mm・上 1 mm の余白
    assert!((hi[0] - lo[0] - 64.0).abs() < 1e-3 && (hi[2] - lo[2] - 16.0).abs() < 1e-3);
    let (elo, ehi) = of(&o, kmb_interchange::ink::boxes::LIMIT).unwrap();
    // 作業範囲は機械の形の置き場所(Carvera Air はベッドの角から 3, 11。ストックの角は L 字の角で
    // ベッドの角から 15, 15)なので、ストックの角から X −12・Y −4
    assert_eq!(elo, [lo[0] - 12.0, lo[1] - 4.0, lo[2]]);
    assert!((ehi[0] - elo[0] - 300.0).abs() < 1e-3);
    assert!(!o.points.is_empty() && !o.lines.is_empty());
    // 平 6 mm(シャンク 6 mm)は別売りのコレットが要る
    let notes = ws.notes(setup);
    assert!(notes.iter().any(|n| n.contains("交換")), "{notes:?}");
    // 加工を選んでも、そのセットアップのストックを出す
    let face = first_of(&ws, kmb_strategy_face::FACE);
    ws.select(face);
    assert_eq!(
        of(&ws.overlay(), kmb_interchange::ink::boxes::WORK),
        Some((lo, hi))
    );
}

#[test]
fn a_stopped_operation_points_to_the_failing_setup() {
    let mut ws = Workspace::new(reference_cam_document(), None, reg());
    let setup = first_of(&ws, kmb_cam_commands::SETUP);
    let face = first_of(&ws, kmb_strategy_face::FACE);
    assert_eq!(ws.root_cause(face), None, "止まっていなければ原因はない");
    let mut data = ws.document().data().feature(setup).unwrap().data.clone();
    data["safe_height"] = serde_json::json!("0.5 mm");
    ws.set_feature_data(setup, data);
    assert!(matches!(
        ws.report().feature(setup),
        Some(FeatureStatus::Error { .. })
    ));
    assert_eq!(ws.root_cause(face), Some(setup));
    // 知らせは履歴に残る
    ws.win.message = Some("一つ目".into());
    ws.log_message();
    ws.win.message = Some("二つ目".into());
    ws.log_message();
    ws.log_message();
    assert_eq!(ws.message_history().len(), 2);
    // 空の知らせ(結果の表示の切り替えなど)は、今の知らせを消さず履歴にも足さない
    ws.take_outcome(kmb_core::Outcome::Message(String::new()), None);
    assert_eq!(ws.message(), Some("二つ目"));
    ws.log_message();
    assert_eq!(ws.message_history().len(), 2);
}

#[test]
fn a_rest_pocket_is_added_with_the_next_thinner_flat_tool() {
    let cam = kmb_samples::reference_cam();
    let op = cam.pocket;
    let mut ws = Workspace::new(cam.part.doc, None, reg());
    // 3.175 mm の次に細い平は 2 mm(3 番)
    ws.select(op);
    assert!(can(&ws, kmb_cam_commands::REST_POCKET));
    let rest = ws.run_action(kmb_cam_commands::REST_POCKET).expect("足せる");
    assert!(
        matches!(ws.report().feature(rest), Some(FeatureStatus::Ok { .. })),
        "{:?}",
        ws.report().feature(rest)
    );
    let f = ws.cur.work.doc().data().feature(rest).unwrap();
    assert_eq!(f.data["rest_tool"], 2);
    assert_eq!(f.data["tool"], 3);
    // 残り削りの残り削りは出さない
    assert!(
        ws.actions()
            .iter()
            .all(|a| a.id != kmb_cam_commands::REST_POCKET)
    );
}

#[test]
fn a_rest_contour_is_added_with_the_next_thinner_flat_tool() {
    let cam = kmb_samples::reference_cam();
    let op = cam.contour;
    // 外形の代わりに、ポケットの壁を内側から 1 周する輪郭にする(四隅に削り残しが出る)
    let mut d = cam.part.doc.data().feature(op).unwrap().data.clone();
    d["side"] = "inside".into();
    d["chains"] =
        serde_json::json!([{ "face": format!("f{}:start", cam.part.pocket.0), "ring": 0 }]);
    let mut ws = Workspace::new(cam.part.doc, None, reg());
    ws.set_feature_data(op, d);
    ws.select(op);
    assert!(can(&ws, kmb_cam_commands::REST_POCKET));
    let rest = ws.run_action(kmb_cam_commands::REST_POCKET).expect("足せる");
    assert!(
        matches!(ws.report().feature(rest), Some(FeatureStatus::Ok { .. })),
        "{:?}",
        ws.report().feature(rest)
    );
    let f = ws.cur.work.doc().data().feature(rest).unwrap();
    assert_eq!(f.type_id, "cam.contour2d@1");
    assert_eq!(f.data["rest_tool"], 2);
    assert_eq!(f.data["tool"], 3);
}

#[test]
fn a_setup_shows_the_total_machining_time() {
    let cam = kmb_samples::reference_cam();
    let (setup, pocket) = (cam.setup, cam.pocket);
    let ws = Workspace::new(cam.part.doc, None, reg());
    let s = ws.summary(setup).expect("セットアップの要約");
    assert!(
        s.contains("加工 6 個") && s.contains("工具交換 2 回") && s.contains("時間の目安"),
        "{s}"
    );
    assert!(ws.summary(pocket).unwrap().contains("時間の目安"));
}

#[test]
fn the_simulation_marks_leftovers_until_the_paths_change() {
    let mut ws = Workspace::new(reference_cam_document(), None, reg());
    let setup = first_of(&ws, kmb_cam_commands::SETUP);
    ws.select(setup);
    assert!(ws.overlay().marks.is_empty());
    ws.run_action(kmb_cam_commands::SIMULATE);
    // 基準部品はポケットの内側の角(工具の半径の丸み)が残る
    let marks = ws.overlay().marks;
    let left: Vec<_> = marks
        .iter()
        .filter(|m| m.1 == kmb_interchange::ink::mark::LEFTOVER)
        .collect();
    assert!(!left.is_empty(), "{:?}", ws.win.message);
    assert!(
        !marks
            .iter()
            .any(|m| m.1 == kmb_interchange::ink::mark::GOUGE)
    );
    // 経路が変わったら、古い印は出さない
    let face = first_of(&ws, kmb_strategy_face::FACE);
    assert!(ws.set_suppressed(face, true));
    ws.select(setup);
    assert!(ws.overlay().marks.is_empty());
}

#[test]
fn the_gcode_is_shown_with_line_numbers_and_saved_where_chosen() {
    let mut ws = Workspace::new(reference_cam_document(), None, reg());
    let setup = first_of(&ws, kmb_cam_commands::SETUP);
    ws.select(setup);
    ws.run_action(kmb_cam_commands::VIEW_GCODE);
    let (title, lines) = ws.viewer().expect("G-code の窓");
    assert!(title.contains("G-code") && title.contains("行"), "{title}");
    assert!(lines[1].contains("時間の目安"), "{}", lines[1]);
    assert_eq!(lines.last().map(String::as_str), Some("M30"));
    let n = lines.len();
    // 窓に行番号つきで出る
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h.get_by_label_contains("1  (kieMonoBench / Carvera Air)");
    let mut ws = h.into_state();
    // 名前を付けて書き出すと、選んだ所へ同じ中身を書く
    let d = temp_dir("gcode");
    let out = d.join("板.nc");
    ws.win.dialogs = Box::new(FakeDialogs {
        open: None,
        save: Some(out.clone()),
        unsaved: Unsaved::Cancel,
    });
    ws.run_action(kmb_cam_commands::EXPORT_GCODE_AS);
    let written = std::fs::read_to_string(&out).unwrap();
    assert_eq!(written.lines().count(), n);
}

#[test]
fn machine_settings_change_the_checks_and_are_kept_in_the_data_folder() {
    let d = temp_dir("settings");
    let open = || Workspace::new(reference_cam_document(), None, reg()).with_data_dir(d.clone());
    let mut ws = open();
    let setup = first_of(&ws, kmb_cam_commands::SETUP);
    let over = |ws: &Workspace, s: FeatureId| ws.notes(s).iter().any(|n| n.contains("作業範囲"));
    // 設定の機械の値は、新しく足したセットアップが写して持つ(今あるセットアップは変わらない)
    let added = |ws: &mut Workspace| ws.run_action(kmb_cam_commands::ADD_SETUP).expect("足せる");
    let before = ws.summary(setup).expect("セットアップの要約");
    assert!(!over(&ws, setup));
    // 作業範囲を狭め、早送りを遅くする。今あるセットアップは文書に写した値のままで変わらず、
    // 新しいセットアップは検査に出る
    let id = kmb_cam_commands::MACHINE_SETTINGS;
    assert!(ws.change_settings(
        id,
        &serde_json::json!({"travel_x": 10.0, "rapid_feed": 300.0})
    ));
    assert!(!over(&ws, setup), "{:?}", ws.notes(setup));
    assert_eq!(ws.summary(setup).unwrap(), before);
    let narrow = added(&mut ws);
    assert!(over(&ws, narrow), "{:?}", ws.notes(narrow));
    // 正しくない値は何も変えずに知らせる
    assert!(!ws.change_settings(
        id,
        &serde_json::json!({"min_rpm": 30000.0, "max_rpm": 100.0})
    ));
    assert!(ws.message().is_some_and(|m| m.contains("回転数")));
    let narrow = added(&mut ws);
    assert!(over(&ws, narrow));
    // 開き直しても残る(データフォルダの settings.json)
    assert!(d.join(kmb_session::SETTINGS_FILE).exists());
    let mut again = open();
    let narrow = added(&mut again);
    assert!(over(&again, narrow));
    // 画面: 窓に欄と説明が出て、初めの値に戻せる(足したセットアップの欄と見分けるため、開き直した文書で)
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 900.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), open());
    h.run();
    h.get_by_label("設定").click();
    h.run();
    // 設定の窓は頁に分かれている(U39): 一般の単位、加工の機械・ポスト
    h.get_by_label("単位").click();
    h.run();
    h.get_by_label_contains("この文書の長さの単位");
    h.get_by_label("加工").click();
    h.run();
    h.get_by_label("ポスト(G-code の書き方)").click();
    h.run();
    h.get_by_label("プラグイン(.wasm のファイル)");
    h.get_by_label("機械").click();
    h.run();
    h.get_by_label("作業範囲 X(mm)");
    // 窓の中の設定が増えて下に隠れることがあるので、ボタンまで送ってから押す
    h.get_by_label("機械を初めの値に戻す").scroll_to_me();
    h.run();
    h.get_by_label("機械を初めの値に戻す").click();
    h.run();
    let mut ws = h.into_state();
    let wide = added(&mut ws);
    assert!(!over(&ws, wide));
    let mut again = open();
    assert_eq!(again.summary(setup).unwrap(), before);
    let wide = added(&mut again);
    assert!(!over(&again, wide));
}

#[test]
fn a_post_plugin_chosen_in_the_settings_writes_the_gcode() {
    let mut ws = Workspace::new(reference_cam_document(), None, reg());
    let setup = first_of(&ws, kmb_cam_commands::SETUP);
    let id = kmb_cam_commands::POST_SETTINGS;
    let wasm = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins/post-example.wasm")
        .canonicalize()
        .unwrap();
    // 読めないファイルは断り、何も変えない
    assert!(!ws.change_settings(id, &serde_json::json!({"plugin": "/nonexistent/x.wasm"})));
    // AI から来た値(画面の欄に打った物も)は、AI に許したフォルダの外のプラグインを断る
    let work = temp_dir("post-roots");
    ws.cmdline.ai_roots = Some(kmb_session::roots::Roots::new(work.clone()));
    let plugin = serde_json::json!({"plugin": wasm.display().to_string()});
    assert!(!ws.change_settings(id, &plugin));
    ws.cmdline.ai_roots = None;
    let _ = std::fs::remove_dir_all(&work);
    ws.select(setup);
    let first = |ws: &mut Workspace| {
        ws.run_action(kmb_cam_commands::VIEW_GCODE);
        ws.viewer().expect("G-code の窓").1[0].clone()
    };
    assert!(first(&mut ws).contains("kieMonoBench / Carvera Air"));
    // 選んだプラグインで書く
    assert!(ws.change_settings(
        id,
        &serde_json::json!({"plugin": wasm.display().to_string()})
    ));
    assert!(first(&mut ws).starts_with("(example-grbl"));
    // 空にすると内蔵のポストに戻る
    assert!(ws.change_settings(id, &serde_json::json!({"plugin": ""})));
    assert!(first(&mut ws).contains("kieMonoBench / Carvera Air"));
}

#[test]
fn the_mesh_tolerance_is_chosen_per_setup() {
    let mut ws = Workspace::new(reference_cam_document(), None, reg());
    let setup = first_of(&ws, kmb_cam_commands::SETUP);
    let tolerance = |ws: &Workspace| {
        kmb_cam_commands::setups(ws.document().data(), ws.report(), &ws.win.registry)[0]
            .info
            .tolerance
    };
    assert_eq!(tolerance(&ws), 0.01, "空なら既定の 0.01 mm");
    let data = ws.document().data().feature(setup).unwrap().data.clone();
    let with = |t: &str| {
        let mut d = data.clone();
        d["tolerance"] = serde_json::json!(t);
        d
    };
    // 範囲の外は理由つきで止まる
    ws.set_feature_data(setup, with("0 mm"));
    match ws.report().feature(setup) {
        Some(FeatureStatus::Error { message }) => {
            assert!(message.contains("網の許容差"), "{message}")
        }
        other => panic!("止まるはず: {other:?}"),
    }
    // 粗くしても加工は通り、シミュレーションも同じ値を使う
    ws.set_feature_data(setup, with("0.05 mm"));
    assert_eq!(ws.report().error_count(), 0);
    assert!((tolerance(&ws) - 0.05).abs() < 1e-12);
    // 設定欄に出る
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.state_mut().select(setup);
    h.run();
    h.get_by_label("網の許容差");
}

#[test]
fn the_stage_band_in_machining_shows_the_shape_after_an_operation() {
    let mut ws = Workspace::new(reference_cam_document(), None, reg());
    ws.layout_mut().space = Space::Machining;
    let setup = first_of(&ws, kmb_cam_commands::SETUP);
    ws.select(setup);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h.get_by_label("素材");
    h.get_by_label("3 ポケット T2").click();
    h.run();
    let shape = h.state().overlay().shape.expect("段の形");
    assert_eq!(shape.label, "ポケット の後");
    assert_eq!(h.state().stage_band().unwrap().selected, Some(3));
    // 途中の形の時は、最後の形の印は出さない
    assert!(h.state().overlay().marks.is_empty());
    // 「次へ」で次の段
    h.get_by_label("次へ").click();
    h.run();
    assert_eq!(h.state().stage_band().unwrap().selected, Some(4));
    h.get_by_label("前へ").click();
    h.run();
    assert_eq!(h.state().stage_band().unwrap().selected, Some(3));
    // 部品の形に戻す
    h.get_by_label("部品を見る").click();
    h.run();
    assert!(h.state().overlay().shape.is_none());
}

#[test]
fn the_simulation_plays_move_by_move_with_the_tool_and_colors() {
    let mut ws = Workspace::new(reference_cam_document(), None, reg());
    ws.layout_mut().space = Space::Machining;
    let setup = first_of(&ws, kmb_cam_commands::SETUP);
    ws.select(setup);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    assert!(h.state().playback().is_none());
    // 段の帯の「再生」で用意する(シミュレーションとは別の粗い格子)
    h.get_by_label("再生").click();
    h.run();
    let pb = h.state().playback().expect("再生の帯");
    assert_eq!(pb.at, 0.0);
    assert!(pb.total > 10.0, "{}", pb.total);
    // 印は加工ごとに 1 つ、始まりの時刻の順
    let marks: Vec<&str> = pb.marks.iter().map(|m| m.1.as_str()).collect();
    assert_eq!(
        marks,
        [
            "上面のフェイス",
            "ボスの周り",
            "ポケット",
            "中央の穴",
            "四隅の穴",
            "外形"
        ]
    );
    assert!(
        pb.marks.windows(2).all(|w| w[0].0 < w[1].0),
        "{:?}",
        pb.marks
    );
    let names: Vec<&str> = pb.info.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(
        names,
        ["時刻", "操作", "工具", "動き", "工具の位置", "動きの番号"]
    );
    // 形は工具の網を含む
    let shape = h.state().overlay().shape.expect("再生の形");
    assert!(shape.mesh.face_names.iter().any(|n| n == "刃"));
    // 次の操作: 2 つ目の操作の始めへ
    h.get_by_label("次の操作").click();
    h.run();
    let pb = h.state().playback().unwrap();
    assert!(
        (pb.at - pb.marks[1].0).abs() < 1e-6,
        "{} {:?}",
        pb.at,
        pb.marks
    );
    // 次の動き・前の動き
    h.get_by_label("次の動き").click();
    h.run();
    let after = h.state().playback().unwrap().at;
    assert!(after > pb.at);
    h.get_by_label("前の動き").click();
    h.run();
    assert!(h.state().playback().unwrap().at < after);
    // 末尾へ。比べる色にすると凡例が出る
    h.get_by_label("末尾").click();
    h.run();
    let pb = h.state().playback().unwrap();
    assert_eq!(pb.at, pb.total);
    h.get_by_label("比較").click();
    h.run();
    let shape = h.state().overlay().shape.unwrap();
    assert_eq!(shape.legend.len(), 4, "{:?}", shape.legend);
    assert!(shape.face_colors.iter().any(|c| c.is_some()));
    // 色は役で渡し、画面のテーマが塗る(コア API 0.13.0 の約束)
    kmb_contract::check_overlay_shape(&shape);
    assert_eq!(shape.legend[0].0, kmb_interchange::ink::shape::WITHIN);
    // 再生すると時刻が進む(先頭から)
    h.get_by_label("先頭").click();
    h.run();
    h.get_by_label("再生").click();
    // 再生の間は描き直しが続くので、コマを決まった数だけ進める
    for _ in 0..3 {
        h.step();
    }
    assert!(h.state().playback().unwrap().at > 0.0);
    // 早送りの線を隠せる
    h.get_by_label("一時停止").click();
    h.step();
    h.run();
    h.get_by_label("早送り").click();
    h.run();
    let mut lines = h.state().overlay().lines;
    h.state().play_filter_lines(&mut lines);
    assert!(
        lines
            .iter()
            .all(|l| l.1.ink != kmb_interchange::ink::line::TRAVEL)
    );
    // 再生している時刻の削った形を STL に書き出せる(工具は含めない)
    let dir = std::env::temp_dir().join(format!("kmb-stock-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("削った形.stl");
    h.state_mut()
        .run_action_with(kmb_cam_commands::EXPORT_STOCK, Some(file.clone()));
    let bytes = std::fs::read(&file).expect("STL が書かれる");
    let tris = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
    assert!(tris > 100, "{tris}");
    assert_eq!(bytes.len(), 84 + tris * 50);
    std::fs::remove_dir_all(&dir).ok();
    // やめると部品の形に戻る(段の帯と再生の帯の両方にある)
    h.get_all_by_label("再生をやめる").next().unwrap().click();
    h.run();
    assert!(h.state().playback().is_none());
    assert!(h.state().overlay().shape.is_none());
}

#[test]
fn hidden_bodies_leave_the_view_but_not_the_machining() {
    let mut ws = Workspace::new(reference_cam_document(), None, reg());
    let setup = first_of(&ws, kmb_cam_commands::SETUP);
    ws.select(setup);
    let lines = path_lines(&ws);
    let bodies = ws.visible_bodies();
    let body = bodies[0].0;
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
    ws.cur.camera.set_view(-90.0, 89.9);
    ws.cur.camera.fit(&ws.cur.scene, rect);
    let p = ws.cur.camera.project([10.0, 20.0, 10.0], rect).0;
    let face = view3d::pick_at(&ws.cur.scene, &ws.cur.camera, rect, p).expect("面に当たる");
    ws.set_picked(Some(face));
    ws.set_hidden(body, true);
    assert!(ws.is_hidden(body));
    // 3D のビューの網から外れ、拾えず、選びからも外れる
    assert!(ws.cur.scene.is_empty());
    assert!(ws.cur.scene.bodies.iter().all(|b| b.feature != body));
    assert!(view3d::pick_at(&ws.cur.scene, &ws.cur.camera, rect, p).is_none());
    assert!(view3d::picks_in(&ws.cur.scene, &ws.cur.camera, rect, rect, false).is_empty());
    assert!(ws.picked().is_none());
    let edges = ws.scene_edges().to_vec();
    assert!(!edges.is_empty(), "辺の表は持ったまま");
    assert!(
        view3d::edge_at(&ws.cur.scene, &ws.cur.camera, rect, p, &edges, true).is_none(),
        "隠したボディの辺は拾わない"
    );
    // 計算と加工は変わらない(見えないだけ)
    assert_eq!(ws.visible_bodies(), bodies);
    assert_eq!(path_lines(&ws), lines);
    assert_eq!(ws.report().error_count(), 0);
    // 文書は変えない
    assert!(!ws.is_modified());
    // 出すと網を作り直さずに戻る
    let key = ws.cur.scene.key().to_vec();
    ws.set_hidden(body, false);
    assert_eq!(ws.cur.scene.key(), key.as_slice());
    assert!(view3d::pick_at(&ws.cur.scene, &ws.cur.camera, rect, p).is_some());
    // 隠したまま形を変えても(作り直しても)隠したまま
    ws.set_hidden(body, true);
    let holes = ws
        .document()
        .data()
        .features
        .iter()
        .find(|f| f.type_id == kmb_cad_hole::HOLE)
        .unwrap()
        .id;
    let mut d = ws.document().data().feature(holes).unwrap().data.clone();
    d["diameter"] = serde_json::json!("9 mm");
    assert!(ws.set_feature_data(holes, d));
    assert_ne!(ws.cur.scene.key(), key.as_slice(), "網を作り直した");
    assert!(ws.cur.scene.is_empty());
}

#[test]
fn the_rest_tool_reads_from_the_previous_tool_to_the_new_one() {
    // Z9: ブラウザは「T5→T2」(今→前)、パネルの工具の順番は「T2 → T5」(前→今)で向きが逆だった
    let f = kmb_core::FeatureEntry {
        id: FeatureId(9),
        type_id: kmb_strategy_pocket2d::POCKET.into(),
        name: "ポケットの残り".into(),
        data: serde_json::json!({ "tool": 5, "rest_tool": 2 }),
        suppressed: false,
    };
    assert_eq!(crate::browser_ui::tool_meta(&f), "T2→T5");
    // 前の工具を直径で打った時は、その直径から
    let sized = kmb_core::FeatureEntry {
        data: serde_json::json!({ "tool": 5, "rest_diameter": "3.175 mm" }),
        ..f
    };
    assert_eq!(crate::browser_ui::tool_meta(&sized), "Ø3.175 mm→T5");
}

#[test]
fn the_machining_help_names_the_operations_as_the_buttons_do() {
    // Z12: F1 の手引きが「面」「外形」と書くが、ボタンは「フェイス」「輪郭」だった
    let (_, steps) = crate::help_ui::FLOWS
        .iter()
        .find(|(t, _)| *t == "削る(加工)")
        .unwrap();
    let line = steps.iter().find(|s| s.contains("ポケット")).unwrap();
    let names: Vec<&str> = line
        .split('「')
        .skip(1)
        .filter_map(|s| s.split_once('」').map(|(n, _)| n))
        .collect();
    assert!(names.len() >= 3, "{line}");
    for n in names {
        assert!(
            kmb_bundle::strategies().iter().any(|s| s.label == n),
            "「{n}」というボタンはない"
        );
    }
}

#[test]
fn a_rest_pocket_does_not_carry_over_the_cutting_overrides() {
    // Z8: 「取残し加工」は元の回転数・送りの上書きと入る位置を写したまま工具を替えていた
    // (「工具の順番に足す」は消す)。太い工具の送りが細い工具に掛かる
    let mut cam = kmb_samples::reference_cam();
    let op = cam.pocket;
    let mut data = cam.part.doc.data().feature(op).unwrap().data.clone();
    data["rpm"] = serde_json::json!("9000");
    data["feed"] = serde_json::json!("1500 mm");
    data["entry"] = serde_json::json!([15.0, 15.0]);
    cam.part
        .doc
        .apply(kmb_core::Transaction::new(
            "設定",
            vec![kmb_core::Op::SetFeatureData { id: op, data }],
        ))
        .unwrap();
    let mut ws = Workspace::new(cam.part.doc, None, reg());
    ws.select(op);
    let rest = ws.run_action(kmb_cam_commands::REST_POCKET).expect("足せる");
    let d = &ws.cur.work.doc().data().feature(rest).unwrap().data;
    assert_eq!(d["tool"], 3);
    assert_eq!(d["rest_tool"], 2);
    for k in ["rpm", "feed", "entry", "finish"] {
        assert!(d.get(k).is_none_or(|v| v.is_null()), "{k} が残った: {d}");
    }
}

#[test]
fn an_operation_is_added_to_the_selected_setup() {
    // Z5: 加工の操作は、選んでいるセットアップではなく、いつも最後のセットアップに足されていた
    let cam = kmb_samples::reference_cam();
    let first = cam.setup;
    let mut ws = Workspace::new(cam.part.doc, None, reg());
    let second = ws.run_action(kmb_cam_commands::ADD_SETUP).expect("足せる");
    ws.finish_edit();
    assert_ne!(first, second);
    let face = add_op(kmb_strategy_face::FACE);
    ws.select(first);
    let op = ws.run_action(face).expect("足せる");
    assert_eq!(
        ws.cur.work.doc().data().feature(op).unwrap().data["setup"],
        first.0,
        "選んだ 1 つ目のセットアップに足す"
    );
    ws.finish_edit();
    ws.select(second);
    let op = ws.run_action(face).expect("足せる");
    assert_eq!(
        ws.cur.work.doc().data().feature(op).unwrap().data["setup"],
        second.0
    );
}

#[test]
fn the_tool_library_button_adds_one_when_the_document_has_none() {
    // Z6: 工具ライブラリの機能が文書に無い時、リボンの「工具ライブラリ」は
    // 「工具ライブラリの種類がありません」と出て足せなかった(記述子にひな形が無い)
    let mut ws = Workspace::new(reference_document(), None, reg());
    let has_tools = |ws: &Workspace| {
        ws.document()
            .data()
            .features
            .iter()
            .any(|f| f.type_id == kmb_cam_commands::TOOLS)
    };
    assert!(!has_tools(&ws));
    ws.switch_space(Space::Machining);
    ws.run_item("u:tools");
    assert!(has_tools(&ws), "{:?}", ws.message());
    // Z7: 機械は 1 台なので「マシンライブラリ」でなく「機械の設定」
    let item = ws.item_of("u:machines").expect("加工のリボンにある");
    assert_eq!(item.label, "機械の設定");
}

#[test]
fn stepping_the_stage_band_selects_the_operation_of_the_stage() {
    // Z21(網羅の調べ J63): 段の帯の矢印でコマ送りすると、削った形は進むが、右のパネルと
    // 表示中の経路は前に選んだ操作のままだった
    let mut ws = Workspace::new(reference_cam_document(), None, reg());
    ws.layout_mut().space = Space::Machining;
    let setup = first_of(&ws, kmb_cam_commands::SETUP);
    ws.select(setup);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h.get_by_label("3 ポケット T2").click();
    h.run();
    let pocket = h.state().selected().expect("ポケットを選んだ");
    let lines = path_lines(h.state());
    h.get_by_label("次へ").click();
    h.run();
    let band = h.state().stage_band().unwrap();
    assert_eq!(band.selected, Some(4));
    let name = band.stages[4].label.strip_prefix("4 ").unwrap().to_string();
    let next = h.state().selected().expect("選んでいる");
    assert_ne!(next, pocket, "次の段の操作を選ぶ");
    assert!(
        name.starts_with(&h.state().document().data().feature(next).unwrap().name),
        "{name}"
    );
    assert_ne!(path_lines(h.state()), lines, "表示する経路も次の操作のもの");
    // 素材まで戻すと、セットアップを選ぶ
    for _ in 0..4 {
        h.get_by_label("前へ").click();
        h.run();
    }
    assert_eq!(h.state().stage_band().unwrap().selected, Some(0));
    assert_eq!(h.state().selected(), Some(setup));
}

#[test]
fn two_operations_with_the_same_name_show_only_their_own_paths() {
    // Z18: 経路と加工の対応を名前で取っていたので、同じ名前の加工が 2 つあると、
    // 選んだ加工に別の加工の経路も重ねて出た
    let alone = {
        let cam = kmb_samples::reference_cam();
        let mut ws = Workspace::new(cam.part.doc, None, reg());
        ws.select(cam.pocket);
        path_lines(&ws)
    };
    assert!(!alone.is_empty());
    let mut cam = kmb_samples::reference_cam();
    let (pocket, contour) = (cam.pocket, cam.contour);
    let pocket_name = cam.part.doc.data().feature(pocket).unwrap().name.clone();
    cam.part
        .doc
        .apply(kmb_core::Transaction::new(
            "名前",
            vec![kmb_core::Op::RenameFeature {
                id: contour,
                name: pocket_name,
            }],
        ))
        .unwrap();
    let mut ws = Workspace::new(cam.part.doc, None, reg());
    ws.select(pocket);
    assert_eq!(path_lines(&ws), alone, "輪郭の経路を重ねない");
}

#[test]
fn the_simulation_marks_have_a_legend_in_the_view() {
    // Z22(網羅の調べ J69): シミュレーションの紫の点(削り残し)に凡例が無く、何の印か分からなかった
    let mut ws = Workspace::new(reference_cam_document(), None, reg());
    ws.layout_mut().space = Space::Machining;
    let setup = first_of(&ws, kmb_cam_commands::SETUP);
    ws.select(setup);
    ws.run_action(kmb_cam_commands::SIMULATE);
    assert!(!ws.overlay().marks.is_empty());
    let mut h = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h.get_by_label_contains("紫の点: 削り残し");
}

#[test]
fn right_clicking_the_setup_folder_offers_a_new_setup() {
    // Z4(網羅の調べ J09): ブラウザのセットアップの木の行を右クリックしても、何も出なかった
    let mut ws = Workspace::new(reference_document(), None, reg());
    ws.layout_mut().space = Space::Machining;
    let mut h = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    let n = h.state().document().data().features.len();
    // リボンの「セットアップ」のボタンとまとまりの名前もあるので、左のブラウザの行を選ぶ
    h.query_all_by_label("セットアップ")
        .find(|n| n.rect().left() < 250.0 && n.rect().top() > 140.0)
        .expect("ブラウザの行")
        .click_secondary();
    h.run();
    h.get_by_label("新しいセットアップ").click();
    h.run();
    assert!(
        h.state()
            .document()
            .data()
            .features
            .iter()
            .any(|f| f.type_id == kmb_cam_commands::SETUP),
        "{:?}",
        h.state().message()
    );
    assert!(h.state().document().data().features.len() > n);
}

#[test]
fn a_chamfer_operation_can_be_moved_to_the_picked_face() {
    // Z11: 面取り加工の面は、欄も付け替えのボタンも無く、後から変えられなかった
    let cam = kmb_samples::reference_cam();
    let (plate, body, setup) = (cam.part.plate, cam.part.hole_pattern, cam.setup);
    let mut ws = Workspace::new(cam.part.doc, None, reg());
    ws.select(setup);
    let op = ws
        .run_action(add_op(kmb_strategy_chamfer::CHAMFER))
        .expect("足せる");
    ws.finish_edit();
    assert!(
        matches!(ws.report().feature(op), Some(FeatureStatus::Ok { .. })),
        "{:?}",
        ws.report().feature(op)
    );
    let before = ws.document().data().feature(op).unwrap().data.clone();
    assert!(!ws.can_retarget_to_picked(op), "面を選ぶまでは替えられない");
    let top = format!("f{}:end", plate.0);
    ws.set_picked(Some(view3d::Pick {
        feature: body,
        face: top.clone(),
    }));
    assert!(ws.can_retarget_to_picked(op), "{before}");
    assert!(ws.retarget_to_picked(op));
    let after = &ws.document().data().feature(op).unwrap().data;
    assert_ne!(*after, before);
    assert!(
        after["face"].as_str().is_some_and(|f| f.contains(&top)),
        "{after}"
    );
    assert!(
        matches!(ws.report().feature(op), Some(FeatureStatus::Ok { .. })),
        "{:?}",
        ws.report().feature(op)
    );
    // 同じ面を選び直しても替える物はない
    ws.set_picked(Some(view3d::Pick {
        feature: body,
        face: top,
    }));
    assert!(!ws.can_retarget_to_picked(op));
}

#[test]
fn deleting_an_operation_says_how_to_bring_it_back() {
    // 右クリックの「削除」は確かめなしで操作を消すが、
    // 戻せることが分からなかった
    let cam = kmb_samples::reference_cam();
    let op = cam.contour;
    let mut ws = Workspace::new(cam.part.doc, None, reg());
    assert!(ws.ask_remove_features(&[op]));
    assert!(ws.document().data().feature(op).is_none());
    let m = ws.message().unwrap_or_default();
    assert!(m.contains("消しました") && m.contains("Ctrl+Z"), "{m}");
    ws.undo();
    assert!(ws.document().data().feature(op).is_some());
}

#[test]
fn the_selected_operation_is_scrolled_into_the_stage_band() {
    // Z26(網羅の調べ J59): 操作が多いと段の帯が切れ、選んだ(足した)操作の札が見えなかった
    let cam = kmb_samples::reference_cam();
    let ops: Vec<FeatureId> = cam
        .part
        .doc
        .data()
        .features
        .iter()
        .filter(|f| f.data.get("setup").is_some() && f.data.get("tool").is_some())
        .map(|f| f.id)
        .collect();
    let mut ws = Workspace::new(cam.part.doc, None, reg());
    ws.layout_mut().space = Space::Machining;
    let w = 760.0;
    let mut h = Harness::builder()
        .with_size(egui::vec2(w, 700.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    // 札(段の帯のボタン)の左右の端
    let chip = |h: &Harness<'_, Workspace>, k: usize| {
        let r = h
            .query_all_by_label_contains(&format!("{k} "))
            .map(|n| n.rect())
            .find(|r| r.top() > 600.0 && r.height() > 20.0)
            .expect("段の帯の札");
        (r.left(), r.right())
    };
    let band = (90.0, w - 100.0);
    for (k, op) in [(1, ops[0]), (ops.len(), *ops.last().unwrap()), (1, ops[0])] {
        h.state_mut().select(op);
        for _ in 0..4 {
            h.run();
        }
        let (l, r) = chip(&h, k);
        assert!(l >= band.0 && r <= band.1, "{k} 番目の札が帯の外: {l}..{r}");
    }
}

#[test]
fn the_machining_ribbon_keeps_its_buttons_when_a_setup_is_selected() {
    // Z3(網羅の調べ J32): セットアップを選ぶとリボンに「固定具を置く」が増え、後ろのボタンの位置がずれた
    let mut ws = Workspace::new(reference_cam_document(), None, reg());
    ws.switch_space(Space::Machining);
    let keys = |ws: &Workspace| -> Vec<String> {
        ws.ribbon_tabs()
            .into_iter()
            .flat_map(|t| t.groups)
            .flat_map(|g| g.items)
            .map(|i| i.key)
            .collect()
    };
    ws.clear_selection();
    let none = keys(&ws);
    let setup = first_of(&ws, kmb_cam_commands::SETUP);
    ws.select(setup);
    assert_eq!(keys(&ws), none, "選んでもボタンの並びは同じ");
    let fixture = ws
        .item_of(&format!("a:{}", kmb_cam_commands::ADD_FIXTURE))
        .expect("固定具を置く");
    assert!(fixture.disabled.is_none());
    ws.clear_selection();
    let fixture = ws
        .item_of(&format!("a:{}", kmb_cam_commands::ADD_FIXTURE))
        .expect("選んでいない時も出る");
    assert!(fixture.disabled.is_some(), "灰色");
}

#[test]
fn the_setup_part_is_chosen_as_a_stage_of_a_named_body() {
    let mut ws = Workspace::new(reference_cam_document(), None, reg());
    let setup = first_of(&ws, kmb_cam_commands::SETUP);
    let body = ws.document().data().feature(setup).unwrap().data["body"]
        .as_u64()
        .map(FeatureId)
        .unwrap();
    let last = ws.document().data().feature(body).unwrap().name.clone();
    ws.open_edit(setup);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    use egui_kittest::kittest::NodeT;
    // 欄はボディの名前と、どの機能の後の形か(最新か途中か)で出す
    let shown = format!("ボディ1・最新({last} の後)");
    let combo = |h: &Harness<'_, Workspace>| {
        h.query_all(egui_kittest::kittest::by().role(egui::accesskit::Role::ComboBox))
            .map(|n| n.accesskit_node().value().unwrap_or_default().to_string())
            .collect::<Vec<_>>()
    };
    assert!(combo(&h).contains(&shown), "{:?}", combo(&h));
    for click in [false, true] {
        let n = h
            .query_all(egui_kittest::kittest::by().role(egui::accesskit::Role::ComboBox))
            .find(|n| n.accesskit_node().value().as_deref() == Some(shown.as_str()))
            .unwrap();
        if click {
            n.click();
        } else {
            n.scroll_to_me();
        }
        h.run();
    }
    assert!(egui::Popup::is_any_open(&h.ctx), "一覧が開いた");
    // 一覧はボディごとに段階を並べる。途中の段階を選ぶと、その機能を部品にする
    let items: Vec<String> = h
        .get_all_by_label_contains("の後")
        .map(|n| n.accesskit_node().label().unwrap_or_default().to_string())
        .collect();
    // 基準部品は 1 つのボディを 5 つの機能で作る(板・ポケット・ボス・四隅の穴・穴の並び)
    assert_eq!(items.len(), 5, "{items:?}");
    assert!(items[0].ends_with("板 の後(1/5 段目)"), "{items:?}");
    assert!(
        items[4].ends_with(&format!("{last} の後(最新)")),
        "{items:?}"
    );
    assert!(h.get_all_by_label("ボディ1").count() >= 1);
    let first = h
        .get_all_by_label_contains("1/")
        .next()
        .expect("途中の段階")
        .accesskit_node()
        .label()
        .unwrap_or_default()
        .to_string();
    h.get_by_label(&first).click();
    h.run();
    let ws = h.state();
    let now = ws.document().data().feature(setup).unwrap().data["body"]
        .as_u64()
        .map(FeatureId)
        .unwrap();
    assert_ne!(now, body, "{first}");
    let name = &ws.document().data().feature(now).unwrap().name;
    assert!(first.contains(name.as_str()), "{first} {name}");
}

/// ブラウザの「機械」の行は、もう一度押すと設定の窓を閉じる(「パラメータ」の行と同じ)。ほかの頁を
/// 出している時は機械の頁へ替える。
#[test]
fn the_machine_row_toggles_the_machine_settings() {
    let mut ws = Workspace::new(reference_cam_document(), None, reg());
    ws.toggle_machine_settings();
    assert!(ws.settings.open);
    let machine = ws.win.layout.settings_page.clone();
    ws.toggle_machine_settings();
    assert!(!ws.settings.open, "もう一度押すと閉じる");
    ws.open_settings_at("ほかの頁");
    ws.toggle_machine_settings();
    assert!(ws.settings.open, "ほかの頁からは機械の頁へ替える");
    assert_eq!(ws.win.layout.settings_page, machine);
}

/// 知らせの強さは出す所が決め、文の言い回しによらない。写す文は古い順に時刻つき。
#[test]
fn a_notice_keeps_the_tone_its_sender_chose() {
    let mut ws = Workspace::new(reference_cam_document(), None, reg());
    ws.win.fail("穴を開けました");
    ws.log_message();
    // 「できません」を含んでも、普通の知らせは普通のまま
    ws.win.message = Some("できませんでした、ではなく、できました".into());
    ws.log_message();
    ws.win.warn("工具が短い");
    ws.log_message();
    use crate::Tone;
    assert_eq!(
        ws.win.history_tone,
        vec![Tone::Error, Tone::Info, Tone::Warn]
    );
    let text = crate::frame_ui::notices_text(&["一\n二".into(), "三".into()], &[0, 60]);
    assert_eq!(
        text,
        "1970-01-01T00:00:00.000Z 一\n  二\n1970-01-01T00:01:00.000Z 三"
    );
}

#[test]
fn a_chamfer_with_picked_edges_cuts_only_those_edges() {
    // 台帳の X1: 「選んだ辺だけを面取りする」は辺を書くが、戦略が読まず、全部の縁を削っていた
    let cam = kmb_samples::reference_cam();
    let (plate, body, setup) = (cam.part.plate, cam.part.hole_pattern, cam.setup);
    let mut ws = Workspace::new(cam.part.doc, None, reg());
    ws.select(setup);
    let op = ws
        .run_action(add_op(kmb_strategy_chamfer::CHAMFER))
        .expect("足せる");
    ws.finish_edit();
    // 削る長さ(送りの動きの xy の長さの和。円弧は弦で数える)
    let cut = |ws: &Workspace| -> Option<f64> {
        let Some(FeatureStatus::Ok { outputs, .. }) = ws.report().feature(op) else {
            return None;
        };
        let (_, tp) = outputs
            .values()
            .find_map(kmb_cam_commands::toolpath_of)
            .expect("経路");
        let mut len = 0.0;
        let mut at: Option<[f64; 3]> = None;
        for m in &tp.moves {
            let to = m.to();
            let feed = serde_json::to_value(m).unwrap()["kind"] != "rapid";
            if let (true, Some(a)) = (feed, at) {
                len += (to[0] - a[0]).hypot(to[1] - a[1]);
            }
            at = Some(to);
        }
        Some(len)
    };
    // 板の上の面の縁を全部削る長さ(初めはいちばん高いボスの面なので、板の面に付け替える)
    let top = format!("f{}:end", plate.0);
    ws.set_picked(Some(view3d::Pick {
        feature: body,
        face: top.clone(),
    }));
    assert!(ws.retarget_to_picked(op));
    ws.wait();
    let all = cut(&ws).expect("全部の縁を削れる");
    let data = ws.document().data().feature(op).unwrap().data.clone();
    // 板の上の面と側面の 1 つの辺だけを選ぶ(側面の名前は形から探す)
    let one = (0..16).find_map(|k| {
        let mut d = data.clone();
        d["selected"] = serde_json::json!([[top.clone(), format!("f{}:side:e{k}", plate.0)]]);
        assert!(ws.set_feature_data(op, d.clone()));
        ws.wait();
        cut(&ws).map(|len| (d, len))
    });
    let (d, one) = one.expect("板の側面の辺を選ぶと削れる");
    assert!(
        one > 1.0 && one < all * 0.5,
        "選んだ 1 本だけを削る(全部 {all:.1} mm、選んだ辺 {one:.1} mm)"
    );
    // 縁に無い辺を選ぶと、黙って全部を削らず理由を出す
    let mut bad = d.clone();
    bad["selected"] = serde_json::json!([["f9999:end", "f9999:side:e0"]]);
    assert!(ws.set_feature_data(op, bad));
    ws.wait();
    assert!(cut(&ws).is_none(), "{:?}", ws.report().feature(op));
}
