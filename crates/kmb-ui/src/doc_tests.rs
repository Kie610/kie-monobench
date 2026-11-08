//! タイムライン・ファイル・文書の試験: 複数の並べ替え(U45)、最近使ったファイルの
//! 手入れ(U15)、設計の属性(U13)、名前を付けた視点(U20)、計算に失敗した間の前の形(M27)、
//! STEP を立てる(U12)、機能のコピー・貼り付け(U42)、開く間の進み具合とやめる(U8)。

use super::*;
use kmb_core::Document;
use kmb_samples::{reference_document, reference_part, sample_document};

fn reg() -> kmb_core::Modules {
    kmb_bundle::modules()
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("kmb-ui-doc-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&d).ok();
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn order(ws: &Workspace) -> Vec<FeatureId> {
    ws.document().data().features.iter().map(|f| f.id).collect()
}

#[test]
fn several_selected_features_move_together_but_not_before_what_they_use() {
    let part = reference_part();
    let ws = Workspace::new(part.doc, None, reg());
    let before = order(&ws);
    // ボスと四隅の穴(続いていない 2 つ)を、ポケットの前へまとめて動かす
    let ids = [part.boss, part.hole_pattern];
    let tx = edit::move_features(
        ws.document().data(),
        &ws.win.registry,
        &ids,
        part.pocket,
        false,
    );
    // パターンは穴を使うので、穴より前へは動かせない(理由を出して何もしない)
    let why = tx.expect_err("使う機能より前には動かせない");
    assert!(why.contains("使うので"), "{why}");
    assert_eq!(order(&ws), before);
    // 続いていない 2 つ(板の輪郭のスケッチとポケット)を最後へ: 板が輪郭を使うので動かせない
    let why = edit::move_features(
        ws.document().data(),
        &ws.win.registry,
        &[part.plate_sketch, part.pocket],
        part.hole_pattern,
        true,
    )
    .expect_err("板より後ろへは動かせない");
    assert!(why.contains("板の輪郭"), "{why}");
    // 動かしてよい並び(順を保ったまま 1 回で戻る)は kmb-session の試験で確かめる
    assert_eq!(order(&ws), before);
}

#[test]
fn recent_files_are_marked_missing_pinned_and_removed() {
    let d = temp_dir("recent");
    let data = d.join("data");
    let (a, b) = (d.join("a.json"), d.join("b.json"));
    for p in [&a, &b] {
        kmb_session::save(p, &sample_document()).unwrap();
    }
    let mut ws = Workspace::new(Document::new(), None, reg()).with_data_dir(data.clone());
    assert!(ws.load_path(&a));
    assert!(ws.load_path(&b));
    // 消したファイルは一覧に残り、見つからない印が付く(開こうとすると理由を出す)
    std::fs::remove_file(&a).unwrap();
    ws.check_recent();
    settle_recent(&mut ws);
    let e = ws.recent_entries();
    assert_eq!(e.len(), 2);
    assert!(
        e.iter()
            .any(|(p, missing)| p.ends_with("a.json") && *missing)
    );
    assert!(
        e.iter()
            .any(|(p, missing)| p.ends_with("b.json") && !*missing)
    );
    let missing = e.iter().find(|(_, m)| *m).unwrap().0.clone();
    ws.open_recent(&missing);
    assert!(ws.message().unwrap().contains("見つかりません"));
    // 留めると先頭に出て、「一覧を消す」でも残る。次に起動しても覚えている
    let kept = e.iter().find(|(_, m)| !*m).unwrap().0.clone();
    ws.recent_action(&missing, RecentAction::Pin(true));
    assert_eq!(ws.recent_entries()[0].0, missing);
    ws.recent_action(Path::new(""), RecentAction::ClearUnpinned);
    assert_eq!(ws.recent_entries().len(), 1);
    assert!(!ws.recent_entries().iter().any(|(p, _)| *p == kept));
    let mut again = Workspace::new(Document::new(), None, reg()).with_data_dir(data.clone());
    settle_recent(&mut again);
    assert_eq!(again.recent_entries(), vec![(missing.clone(), true)]);
    // 一覧から外す
    ws.recent_action(&missing, RecentAction::Remove);
    assert!(ws.recent_entries().is_empty());
    std::fs::remove_dir_all(&d).ok();
}

use std::path::Path;

/// 最近のファイルの有無は裏のスレッドで調べる(開始画面を止めない)。調べ終えるのを待つ。
fn settle_recent(ws: &mut Workspace) {
    while ws.home.recent_missing.pending.is_some() {
        std::thread::sleep(std::time::Duration::from_millis(2));
        ws.poll_recent();
    }
}

#[test]
fn the_design_properties_are_kept_in_the_document_and_undone_at_once() {
    let d = temp_dir("props");
    let file = d.join("p.json");
    let mut ws = Workspace::new(reference_document(), None, reg());
    let mut p = kmb_core::Properties::default();
    p.set("title", "ブラケット");
    p.set("part_number", "JC-001");
    p.set("検査", "済み");
    assert!(ws.set_properties(p.clone()));
    assert!(ws.is_modified());
    assert_eq!(ws.document().data().properties, p);
    kmb_session::save(&file, ws.document()).unwrap();
    let back = kmb_session::load(&file).unwrap();
    assert_eq!(back.data().properties, p);
    ws.undo();
    assert!(ws.document().data().properties.is_empty());
    // 窓は今の値から直し始める
    ws.redo();
    ws.open_properties();
    assert_eq!(ws.cur.props_draft.as_ref(), Some(&p));
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn a_named_view_brings_back_the_same_direction_and_size() {
    let mut ws = Workspace::new(reference_document(), None, reg());
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0));
    ws.win.view_rect = rect;
    ws.cur.camera.fit(&ws.cur.scene, rect);
    ws.cur.camera.set_view(-90.0, 10.0);
    let saved = ws.cur.camera.snapshot(rect);
    let id = ws.save_named_view("正面").expect("保存できる");
    assert_eq!(ws.document().data().named_view(id).unwrap().name, "正面");
    // 回して動かして広げた後で戻す
    ws.cur.camera.orbit(egui::vec2(120.0, -40.0));
    ws.cur.camera.pan_by(egui::vec2(30.0, 10.0));
    ws.cur.camera.zoom_at(2.0, egui::pos2(100.0, 100.0), rect);
    let before = ws.cur.camera.snapshot(rect);
    assert!(ws.apply_named_view(id));
    // 回り始める前に真ん中だけ先に飛ばない(向きと一緒に移す)
    ws.cur.camera.step_turn(0.0);
    let start = ws.cur.camera.snapshot(rect);
    for i in 0..3 {
        assert!(
            (start.0[i] - before.0[i]).abs() < 1e-3,
            "{start:?} {before:?}"
        );
    }
    // 向きは滑らかに替えるので、替え終わった所で比べる
    ws.cur.camera.step_turn(10.0);
    let now = ws.cur.camera.snapshot(rect);
    for i in 0..3 {
        assert!((now.0[i] - saved.0[i]).abs() < 1e-3, "{now:?} {saved:?}");
    }
    assert!((now.1 - saved.1).abs() < 1e-3 && (now.2 - saved.2).abs() < 1e-3);
    assert!((now.3 - saved.3).abs() < 1e-3);
    // 大きさの違うビューでも、見える高さは同じ
    let small = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(450.0, 350.0));
    ws.win.view_rect = small;
    ws.apply_named_view(id);
    assert!((ws.cur.camera.snapshot(small).3 - saved.3).abs() < 1e-3);
    // 傾けて保存した視点は、傾きも戻る
    ws.win.view_rect = rect;
    ws.cur.camera.orbit_free(egui::vec2(0.0, 0.0));
    ws.cur.camera.orbit_free(egui::vec2(40.0, 25.0));
    let tilt = ws.cur.camera.tilt_deg();
    assert!(tilt.abs() > 0.1, "傾いている: {tilt}");
    let tilted = ws.save_named_view("傾き").unwrap();
    assert!((ws.document().data().named_view(tilted).unwrap().roll - f64::from(tilt)).abs() < 1e-3);
    ws.cur.camera.set_view(-90.0, 10.0);
    assert!(ws.apply_named_view(tilted));
    ws.cur.camera.step_turn(0.0);
    ws.cur.camera.step_turn(10.0);
    assert!((ws.cur.camera.tilt_deg() - tilt).abs() < 1e-3);
    // 名前を変える・消す(取り消せる)
    assert!(ws.rename_named_view(id, "前から"));
    assert!(ws.remove_named_view(id));
    assert_eq!(ws.document().data().named_views.len(), 1);
    ws.undo();
    assert_eq!(ws.document().data().named_view(id).unwrap().name, "前から");
}

#[test]
fn a_failed_edit_keeps_the_last_good_shape_on_screen() {
    let part = reference_part();
    let mut ws = Workspace::new(part.doc, None, reg());
    let good: Vec<_> = ws.cur.scene.key().to_vec();
    assert!(ws.held_features().is_empty());
    // ポケットを板より深くすると、切り取りが空になって失敗する
    let mut data = ws
        .document()
        .data()
        .feature(part.pocket)
        .unwrap()
        .data
        .clone();
    data["distance"] = serde_json::json!("0 mm");
    assert!(ws.set_feature_data(part.pocket, data));
    assert!(
        matches!(
            ws.report().feature(part.pocket),
            Some(kmb_core::FeatureStatus::Error { .. })
        ),
        "{:?}",
        ws.report().feature(part.pocket)
    );
    // 形は前のまま(消えない)で、どの機能のせいかを出す
    assert_eq!(ws.held_features(), &[part.pocket]);
    assert_eq!(ws.cur.scene.key(), &good[..]);
    // 前の形はカーネルに残っている(面を数えられる)
    for (_, h) in &good {
        assert!(ws.cur.work.kernel().faces(*h).is_ok());
    }
    // もう一度失敗する値にしても、前の形のまま
    let mut data = ws
        .document()
        .data()
        .feature(part.pocket)
        .unwrap()
        .data
        .clone();
    data["distance"] = serde_json::json!("-1 mm");
    ws.set_feature_data(part.pocket, data);
    if matches!(
        ws.report().feature(part.pocket),
        Some(kmb_core::FeatureStatus::Error { .. })
    ) {
        assert_eq!(ws.cur.scene.key(), &good[..]);
    }
    // 直すと今の形に替わる
    ws.undo();
    ws.undo();
    assert!(ws.held_features().is_empty());
    assert_eq!(ws.report().error_count(), 0);
}

#[test]
fn a_value_that_cannot_make_the_shape_clears_the_preview_while_editing() {
    // 採用分の点検 B14: 編集パネルの欄に形が作れない値を打ったら、予告の形を消す
    // (前に作れた形を出し続けない)
    let part = reference_part();
    let mut ws = Workspace::new(part.doc, None, reg());
    let good: Vec<_> = ws.cur.scene.key().to_vec();
    ws.open_edit(part.pocket);
    let mut data = ws
        .document()
        .data()
        .feature(part.pocket)
        .unwrap()
        .data
        .clone();
    data["distance"] = serde_json::json!("0 mm");
    assert!(ws.set_feature_data(part.pocket, data));
    assert!(matches!(
        ws.report().feature(part.pocket),
        Some(kmb_core::FeatureStatus::Error { .. })
    ));
    assert!(ws.held_features().is_empty(), "前の形を出し続けない");
    assert_ne!(ws.cur.scene.key(), &good[..], "予告の形が消える");
    // 作れる値に戻すと形が出る
    ws.undo();
    assert_eq!(ws.report().error_count(), 0);
    assert!(ws.held_features().is_empty());
}

#[test]
fn a_step_file_is_read_and_stood_up_when_the_user_says_so() {
    // 順に確かめる:
    // - STEP を読み込むと名前の付いた 1 つのボディになり、STEP でないファイルは断る
    //   (元 tests.rs の a_step_file_is_read_into_the_document)
    // - 読み込んだ後に 1 回聞き、立てる・取り消す・次から聞かない(元 a_step_file_is_stood_up_when_the_user_says_so)
    let path = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../kmb-kernel-truck/testdata/reference.step"
    ));
    let mut ws = Workspace::new(Document::new(), None, reg());
    let id = ws
        .run_action_with(kmb_cad_import_step::IMPORT_STEP_FILE, Some(path.into()))
        .expect("読み込める");
    assert_eq!(ws.report().error_count(), 0, "{:?}", ws.report());
    let entry = ws.document().data().feature(id).unwrap();
    assert_eq!(entry.name, "reference");
    assert_eq!(ws.visible_bodies().len(), 1);
    // 読み込んだすぐ後に 1 回だけ聞く
    assert_eq!(ws.step_up_question(), Some(id));
    let bounds = |ws: &Workspace| {
        let (_, h) = ws.visible_bodies()[0];
        ws.cur.work.kernel().bounds(h).unwrap()
    };
    let (lo, hi) = bounds(&ws);
    ws.answer_step_up(StepUp::StandUp);
    assert!(ws.step_up_question().is_none());
    assert_eq!(ws.report().error_count(), 0, "{:?}", ws.report());
    // X 軸のまわりに 90 度: 前の Y の幅が Z の幅に、前の Z の幅が Y の幅になる
    let (lo2, hi2) = bounds(&ws);
    assert!(((hi2[2] - lo2[2]) - (hi[1] - lo[1])).abs() < 1e-6);
    assert!(((hi2[1] - lo2[1]) - (hi[2] - lo[2])).abs() < 1e-6);
    assert!(((hi2[0] - lo2[0]) - (hi[0] - lo[0])).abs() < 1e-6);
    // 1 回の取り消しで戻る
    ws.undo();
    let (lo3, hi3) = bounds(&ws);
    assert!((hi3[2] - lo3[2] - (hi[2] - lo[2])).abs() < 1e-6);
    // 「次から聞かない」の後は聞かない
    ws.undo();
    ws.run_action_with(kmb_cad_import_step::IMPORT_STEP_FILE, Some(path.into()));
    ws.answer_step_up(StepUp::NeverAsk);
    assert!(!ws.win.layout.ask_step_up);
    ws.run_action_with(kmb_cad_import_step::IMPORT_STEP_FILE, Some(path.into()));
    assert!(ws.step_up_question().is_none());
    // STEP でないファイルは断る
    let not_step = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"));
    assert!(
        ws.run_action_with(kmb_cad_import_step::IMPORT_STEP_FILE, Some(not_step.into()))
            .is_none()
    );
    assert!(ws.message().unwrap().contains("STEP"));
}

#[test]
fn a_step_file_written_by_jc_is_not_asked_to_stand_up() {
    // JC は Z を上にして書くので、JC の書いた STEP には立てるかを聞かない(U12)
    let src = include_str!("../../kmb-kernel-truck/testdata/reference.step");
    let ours = src.replace(
        "/* originating_system */ ''",
        "/* originating_system */ 'kieMonoBench'",
    );
    assert_ne!(ours, src);
    let path = crate::tests::temp_dir("step-ours").join("ours.step");
    std::fs::write(&path, ours).unwrap();
    let mut ws = Workspace::new(Document::new(), None, reg());
    ws.run_action_with(kmb_cad_import_step::IMPORT_STEP_FILE, Some(path))
        .expect("読み込める");
    assert!(ws.step_up_question().is_none());
}

#[test]
fn a_pasted_feature_works_on_the_body_now_and_has_its_picks_cleared() {
    // 1 つの文書で順に確かめる(見本の部品は reference_document と同じ文書):
    // - 貼った機能は今のボディを対象にする(元 a_pasted_feature_works_on_the_body_as_it_is_now)
    // - 写したフィレットは辺を空にして貼り、選び直す欄から直す
    //   (元 a_copied_feature_is_pasted_with_its_picks_cleared_to_choose_again)
    //
    // ボスの対象はポケット。最後(穴の並びの後)に貼ると、今のボディ(穴の並び)を対象にする。
    // 写した時のままだと、ポケットの形から枝分かれした 2 つ目のボディができる
    let part = reference_part();
    let mut ws = Workspace::new(part.doc, None, reg());
    let n0 = ws.document().data().features.len();
    assert_eq!(ws.visible_bodies().len(), 1);
    assert_eq!(ws.copy_features(None, &[part.boss]), 1);
    let made = ws.paste_features();
    assert_eq!(made.len(), 1);
    let new = ws.document().data().feature(made[0]).unwrap();
    assert_eq!(new.data["target"], part.hole_pattern.0, "{}", new.data);
    assert_eq!(ws.report().error_count(), 0, "{:?}", ws.report());
    assert_eq!(ws.visible_bodies().len(), 1);
    assert_eq!(ws.visible_bodies()[0].0, made[0]);
    // 貼ったボスを取り消して、元の文書でフィレットを写す
    ws.undo();
    assert_eq!(ws.document().data().features.len(), n0);

    let plate = ws.document().data().features[1].id;
    let fillet = ws
        .take_outcome(
            kmb_core::Outcome::insert(
                "フィレット",
                kmb_cad_edge::FILLET,
                "フィレット1".to_string(),
                serde_json::json!({"target": plate.0, "edges": [["f2:end", "f2:side:e1"]], "radius": "radius_r"}),
            ),
            None,
        )
        .expect("足せる");
    ws.add_param("radius_r", "1.5 mm");
    // スケッチは写さない(理由を出す)
    let sketch = ws.document().data().features[0].id;
    assert_eq!(ws.copy_features(None, &[sketch]), 0);
    assert!(ws.message().unwrap().contains("写せるのは"));
    assert_eq!(ws.copy_features(None, &[fillet]), 1);
    let n = ws.document().data().features.len();
    let (tip, _) = ws.visible_bodies()[0];
    assert_ne!(tip, plate);
    let made = ws.paste_features();
    assert_eq!(made.len(), 1);
    let doc = ws.document().data();
    assert_eq!(doc.features.len(), n + 1);
    let new = doc.feature(made[0]).unwrap();
    assert_eq!(new.name, "フィレット1 (2)");
    // 式と対象のボディは写り、辺は空(貼る先で選び直す)
    let d = ws
        .win
        .registry
        .descriptor(kmb_cad_edge::FILLET)
        .unwrap()
        .upgraded(&new.data);
    assert_eq!(d["rows"][0]["radius"], "radius_r");
    // 対象のボディは、写した時の形(板)でなく今の形(板に続く最後の機能)
    assert_eq!(d["target"], tip.0);
    let desc = ws.win.registry.descriptor(kmb_cad_edge::FILLET).unwrap();
    assert!(desc.pick_fields(&d).iter().all(|f| f.refs(&d).is_empty()));
    // 最初の選ぶ欄がクリックを受ける状態で、貼ったものを直している
    assert!(ws.active_pick_field().is_some_and(|(e, _)| e.id == made[0]));
    assert!(ws.message().unwrap().contains("選び直して"));
    // 1 回で取り消せる
    ws.undo();
    assert_eq!(ws.document().data().features.len(), n);
}

#[test]
fn opening_a_file_can_be_stopped_to_go_back_to_the_previous_document() {
    let d = temp_dir("opening");
    let file = d.join("ref.json");
    kmb_session::save(&file, &reference_document()).unwrap();
    let mut ws = Workspace::open(sample_document(), None, true, reg());
    ws.wait();
    let before = ws.document().data().clone();
    let shown = ws.cur.scene.key().to_vec();
    assert!(ws.load_path(&file));
    // 計算は裏で走り、終わるまで開いている途中(進み具合が分かる)
    assert!(ws.is_opening());
    let (_, total) = ws.progress().expect("走っている");
    assert_eq!(total, ws.document().data().features.len());
    // やめると前の文書に戻る(保存先も)
    assert!(ws.cancel_opening());
    assert!(!ws.is_opening());
    assert_eq!(ws.document().data(), &before);
    assert!(ws.path().is_none());
    ws.wait();
    assert_eq!(ws.report().error_count(), 0);
    assert_eq!(ws.cur.scene.key().len(), shown.len());
    // 最後まで待てば開ける
    assert!(ws.load_path(&file));
    ws.wait();
    assert_eq!(ws.path(), Some(file.as_path()));
    assert_eq!(ws.report().error_count(), 0, "{:?}", ws.report());
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn an_iges_import_offers_the_upright_question_and_uses_the_declared_file_filter() {
    let d = temp_dir("iges");
    for own in [false, true] {
        let record = |s: &str, c: char, n: usize| format!("{s:<72}{c}{n:7}");
        let mut global = vec![String::new(); 20];
        global[0] = "1H,".into();
        global[1] = "1H;".into();
        global[2] = if own { "12HkieMonoBench" } else { "4Htest" }.into();
        global[12] = "1".into();
        global[13] = "2".into();
        global[14] = "2HMM".into();
        global[18] = "1D-6".into();
        let g = (global.join(",") + ";")
            .as_bytes()
            .chunks(72)
            .enumerate()
            .map(|(i, b)| record(std::str::from_utf8(b).unwrap(), 'G', i + 1))
            .collect::<Vec<_>>();
        let mut records = vec![record("test", 'S', 1)];
        records.extend(g.iter().cloned());
        for (i, kind) in [116, 196].into_iter().enumerate() {
            records.push(record(
                &format!(
                    "{kind:8}{:8}{:8}{:8}{:8}{:8}{:8}{:8}00000000",
                    i + 1,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0
                ),
                'D',
                i * 2 + 1,
            ));
            records.push(record(
                &format!(
                    "{kind:8}{:8}{:8}{:8}{:8}{:8}{:8}{:8}{:8}",
                    0, 0, 1, 0, 0, 0, 0, 0
                ),
                'D',
                i * 2 + 2,
            ));
        }
        records.push(format!("{:<64}{:8}P{:7}", "116,0,0,0,0;", 1, 1));
        records.push(format!("{:<64}{:8}P{:7}", "196,1,5;", 3, 2));
        records.push(record(
            &format!("S{:7}G{:7}D{:7}P{:7}", 1, g.len(), 4, 2),
            'T',
            1,
        ));
        let path = d.join("surface.IGES");
        std::fs::write(&path, records.join("\n")).unwrap();
        let mut ws = Workspace::new(Document::new(), None, reg());
        let action = ws
            .actions()
            .into_iter()
            .find(|a| a.role == Some(kmb_interchange::role::action::IMPORT_IGES))
            .unwrap();
        let kmb_core::Input::OpenFile { extensions, .. } = &action.input else {
            panic!("ファイルを開く操作ではない")
        };
        assert!(extensions.iter().any(|s| s == "igs") && extensions.iter().any(|s| s == "IGES"));
        let id = ws
            .run_action_with(&action.id, Some(path.clone()))
            .expect("IGESを読める");
        assert_eq!(ws.report().error_count(), 0, "{:?}", ws.report());
        assert_eq!(ws.step_up_question(), (!own).then_some(id));
        if !own {
            ws.answer_step_up(StepUp::StandUp);
            assert_eq!(
                ws.document().data().feature(id).unwrap().data["stand_up"],
                serde_json::json!(true)
            );
        }
    }
    std::fs::remove_dir_all(d).unwrap();
}
