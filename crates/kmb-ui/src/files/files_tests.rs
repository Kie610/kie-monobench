//! ファイルの扱いの通しの試験(保存・開く・未保存の確認・自動保存・書き出し)。

use crate::tests::{FakeDialogs, first_of, reg, temp_dir};
use crate::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kmb_core::Document;
use kmb_samples::{reference_cam_document, sample_document};

#[test]
fn an_untitled_document_is_saved_with_a_name_and_remembered() {
    let d = temp_dir("saveas");
    let file = d.join("板");
    let mut ws = Workspace::new(sample_document(), None, reg())
        .with_dialogs(Box::new(FakeDialogs {
            open: None,
            save: Some(file.clone()),
            unsaved: Unsaved::Cancel,
        }))
        .with_data_dir(d.join("data"));
    ws.save();
    let saved = d.join(format!("板.{}", kmb_core::DOCUMENT_EXTENSION));
    assert_eq!(ws.path(), Some(saved.as_path()), "拡張子を足す");
    assert!(!ws.is_modified());
    assert!(kmb_session::load(&saved).is_ok());
    assert_eq!(ws.recent_files().len(), 1);
    // 次に起動した時も最近使ったファイルに出る
    let again = Workspace::new(Document::new(), None, reg()).with_data_dir(d.join("data"));
    assert_eq!(again.recent_files(), ws.recent_files());
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn unsaved_changes_are_kept_unless_the_user_discards_them() {
    let mut ws =
        Workspace::new(sample_document(), None, reg()).with_dialogs(Box::new(FakeDialogs {
            open: None,
            save: None,
            unsaved: Unsaved::Cancel,
        }));
    ws.add_param("extra", "1 mm");
    assert!(ws.is_modified());
    let before = ws.document().data().features.len();
    // 文書を閉じる時に聞く(新しい文書は別のタブに開くので、閉じるまでは聞かない。候補 20)
    assert!(!ws.close_tab(0));
    assert_eq!(
        ws.document().data().features.len(),
        before,
        "やめたら文書はそのまま"
    );
    ws = ws.with_dialogs(Box::new(FakeDialogs {
        open: None,
        save: None,
        unsaved: Unsaved::Discard,
    }));
    assert!(ws.close_tab(0));
    assert!(ws.document().data().features.is_empty());
    assert!(!ws.is_modified());
}

#[test]
fn a_document_is_opened_from_the_start_screen_and_autosaved_until_saved() {
    let d = temp_dir("start");
    let file = d.join("部品.json");
    kmb_session::save(&file, &kmb_samples::reference_document()).unwrap();
    let ws = Workspace::new(Document::new(), None, reg())
        .with_dialogs(Box::new(FakeDialogs {
            open: Some(file.clone()),
            save: None,
            unsaved: Unsaved::Cancel,
        }))
        .with_data_dir(d.join("data"))
        .with_samples(vec![("基準部品".into(), kmb_samples::reference_document)])
        .show_start();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    // ホームの中身(最近使ったデザイン・見本・手順)は kmb-ui-library の試験で確かめる。
    // 拡張の無い時も、新規デザインと開くは出す
    h.get_by_label("新規デザイン");
    h.get_by_label("開く…").click();
    h.run();
    assert!(!h.state().is_start_screen());
    assert_eq!(h.state().path(), Some(file.as_path()));
    assert!(h.state().document().data().features.len() > 3);
    // 変更して時間がたつと、写しが書かれる
    let mut ws = h.into_state();
    ws.add_param("extra", "1 mm");
    let now = std::time::SystemTime::now();
    let later = |m: u64| now + std::time::Duration::from_secs(m * 60);
    // 設定の間隔で書く(U11)。「しない」なら書かない
    ws.win.layout.autosave_minutes = 0;
    ws.autosave_tick(now);
    ws.autosave_tick(later(90));
    assert!(
        kmb_session::recoveries(&d.join("data")).is_empty(),
        "しない"
    );
    ws.win.layout.autosave_minutes = 10;
    ws.autosave_tick(now);
    ws.autosave_tick(later(9));
    assert!(
        kmb_session::recoveries(&d.join("data")).is_empty(),
        "10 分たつまで書かない"
    );
    ws.autosave_tick(later(10));
    assert_eq!(kmb_session::recoveries(&d.join("data")).len(), 1);
    ws.save();
    assert!(
        kmb_session::recoveries(&d.join("data")).is_empty(),
        "保存したら写しを消す"
    );
    assert!(kmb_session::backup_path(&file).exists(), "直前の版を残す");
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn exporting_from_an_unsaved_document_asks_where_to_write() {
    // Z23(網羅の調べ J65): 下の帯の「書き出す」は、文書を保存先つきで開いていないと
    // 「文書を保存先つきで開くと、書き出せます」と出て何もしなかった
    let d = temp_dir("export-unsaved");
    let out = d.join("板.nc");
    let mut ws =
        Workspace::new(reference_cam_document(), None, reg()).with_dialogs(Box::new(FakeDialogs {
            open: None,
            save: Some(out.clone()),
            unsaved: Unsaved::Cancel,
        }));
    let setup = first_of(&ws, kmb_cam_commands::SETUP);
    ws.select(setup);
    ws.run_action(kmb_cam_commands::EXPORT_GCODE);
    let g = std::fs::read_to_string(&out).unwrap_or_else(|e| panic!("{e}: {:?}", ws.message()));
    assert!(g.contains("M6"), "{g}");
    std::fs::remove_dir_all(&d).ok();
}

/// 窓を出せないダイアログ(Linux で zenity も portal も無い時)。
struct NoWindowDialogs;

impl Dialogs for NoWindowDialogs {
    fn open_document(&self) -> Option<std::path::PathBuf> {
        None
    }
    fn save_document(&self, _: &str) -> Option<std::path::PathBuf> {
        None
    }
    fn open_file(&self, _: &str, _: &str, _: &[&str]) -> Option<std::path::PathBuf> {
        None
    }
    fn save_file(&self, _: &str, _: &str, _: &[&str], _: &str) -> Option<std::path::PathBuf> {
        None
    }
    fn unsaved(&self, _: &str) -> Unsaved {
        Unsaved::Cancel
    }
    fn unavailable(&self) -> Option<String> {
        Some("zenity が見つかりません".into())
    }
}

#[test]
fn a_save_window_that_cannot_open_is_reported_as_such() {
    // Z24: OS の保存の窓が出せない時も「保存先が選ばれなかったので、やめました」と出ていた
    let mut ws = Workspace::new(reference_cam_document(), None, reg())
        .with_dialogs(Box::new(NoWindowDialogs));
    let setup = first_of(&ws, kmb_cam_commands::SETUP);
    ws.select(setup);
    ws.start_action(kmb_cam_commands::EXPORT_GCODE_AS);
    let m = ws.message().unwrap_or_default().to_string();
    assert!(m.contains("出せません") && m.contains("zenity"), "{m}");
}

#[test]
fn bodies_are_exported_one_file_each_with_their_names_and_hidden_ones_are_not() {
    // 2 つのボディの文書で順に確かめる:
    // - 隠したボディは書き出さず、全部隠すと理由を出す(元 hidden_bodies_are_not_exported)
    // - M55: ボディごとに分けて書き出す。名前はブラウザと同じ(付けた名前か「ボディN」)
    //   (元 bodies_are_exported_one_file_each_with_their_names)
    let d = temp_dir("export_each");
    let mut doc = kmb_samples::reference_document();
    // 最初の押し出しをもう 1 つ新しいボディとして足し、ボディを 2 つにする
    let mut second = doc
        .data()
        .features
        .iter()
        .find(|f| f.type_id.starts_with("solid.extrude"))
        .unwrap()
        .clone();
    second.id = doc.alloc_feature_id();
    second.name = "二枚目".into();
    doc.apply(kmb_core::Transaction::new(
        "足す",
        vec![kmb_core::Op::InsertFeature {
            index: None,
            entry: second.clone(),
        }],
    ))
    .unwrap();
    let mut ws = Workspace::new(doc, None, reg());
    assert_eq!(ws.visible_bodies().len(), 2);
    assert!(ws.rename_body(second.id, "ふた"));
    // 隠したボディだけなら書かない。見えていれば書く
    let bodies: Vec<FeatureId> = ws.visible_bodies().iter().map(|b| b.0).collect();
    let file = d.join("形.stl");
    for &b in &bodies {
        ws.set_hidden(b, true);
    }
    ws.run_action_with("io.export_stl", Some(file.clone()));
    assert!(!file.exists(), "隠したボディだけなら書かない");
    assert!(
        ws.message()
            .is_some_and(|m| m.contains("見えているボディがありません")),
        "{:?}",
        ws.message()
    );
    for &b in &bodies {
        ws.set_hidden(b, false);
    }
    ws.run_action_with("io.export_stl", Some(file.clone()));
    assert!(file.exists(), "見えていれば書く");
    assert_eq!(ws.visible_bodies().len(), 2);
    let file = d.join("部品.stl");
    ws.run_action_with("io.export_stl_each", Some(file.clone()));
    assert!(!file.exists(), "まとめたファイルは作らない");
    assert!(d.join("部品-ボディ1.stl").exists(), "{:?}", ws.message());
    assert!(d.join("部品-ふた.stl").exists(), "{:?}", ws.message());
    assert!(
        ws.message()
            .is_some_and(|m| m.contains("2 個書き出しました"))
    );
    // 隠したボディは書かない(番号は変えない)
    let first = ws.visible_bodies()[0].0;
    ws.set_hidden(first, true);
    let file = d.join("b.step");
    ws.run_action_with("io.export_step_each", Some(file));
    assert!(!d.join("b-ボディ1.step").exists());
    assert!(d.join("b-ふた.step").exists(), "{:?}", ws.message());
    std::fs::remove_dir_all(&d).ok();
}

fn bulk_recovery_fixture(count: usize) -> (Workspace, std::path::PathBuf) {
    let dir = temp_dir("bulk-recoveries");
    let mut ws = Workspace::new(Document::new(), None, reg()).show_start();
    ws.win.layout.appearance.language = "ja".into();
    for i in 0..count {
        let file = dir.join(format!("copy-{i}.json"));
        std::fs::write(&file, "試験の写し").unwrap();
        ws.home.recoveries.push(kmb_session::Recovery {
            file,
            original: Some(dir.join(format!("部品{i}.json"))),
            saved_at: std::time::SystemTime::UNIX_EPOCH,
        });
    }
    (ws, dir)
}

fn bulk_recovery_tool(h: &mut Harness<'_, Workspace>, tool: &str, args: serde_json::Value) {
    let crate::live_ui::Answer::AfterScript(batch) =
        h.state_mut().live_request(tool, &args).unwrap()
    else {
        panic!("画面の台本で操作する");
    };
    let started = std::time::Instant::now();
    loop {
        h.step();
        if let Some(result) = h.state_mut().script_result(batch) {
            assert!(result.ok, "{}", result.report());
            break;
        }
        assert!(started.elapsed().as_secs() < 60, "復旧の操作が終わりません");
    }
}

#[test]
fn bulk_recovery_discard_confirms_then_removes_three_copies_through_ui_tools() {
    let (ws, dir) = bulk_recovery_fixture(3);
    let paths: Vec<_> = ws.home.recoveries.iter().map(|r| r.file.clone()).collect();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h.get_by_label("全部捨てる");
    bulk_recovery_tool(
        &mut h,
        "ui_click",
        serde_json::json!({"target":"全部捨てる"}),
    );
    h.get_by_label("3 個の写しを捨てます。元に戻せません");
    assert!(paths.iter().all(|p| p.is_file()), "1回目は捨てない");
    bulk_recovery_tool(&mut h, "ui_key", serde_json::json!({"keys":"Escape"}));
    assert!(
        h.query_by_label("3 個の写しを捨てます。元に戻せません")
            .is_none()
    );
    assert!(paths.iter().all(|p| p.is_file()));
    for _ in 0..2 {
        bulk_recovery_tool(
            &mut h,
            "ui_click",
            serde_json::json!({"target":"全部捨てる"}),
        );
    }
    assert!(h.state().home.recoveries.is_empty());
    assert!(paths.iter().all(|p| !p.exists()));
    assert!(h.query_by_label("全部捨てる").is_none());
    assert!(!h.state().is_modified());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn bulk_recovery_discard_keeps_an_undeletable_copy_and_its_reason() {
    let (mut ws, dir) = bulk_recovery_fixture(1);
    let file = ws.home.recoveries[0].file.clone();
    std::fs::remove_file(&file).unwrap();
    std::fs::create_dir(&file).unwrap();
    let problem = ws.discard_recovery_at(0).unwrap_err();
    assert_eq!(problem.kind, kmb_core::ProblemKind::File);
    ws.discard_recovery(0);
    assert_eq!(ws.home.recoveries.len(), 1, "捨てられなかった写しは残す");
    assert!(file.is_dir());
    assert!(
        ws.message()
            .is_some_and(|message| message.contains("捨てられません"))
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn bulk_recovery_discard_finishes_other_copies_and_keeps_failures_in_both_languages() {
    for (lang, discard, error) in [
        ("ja", "全部捨てる", "捨てられません"),
        ("en", "Discard all", "Could not discard"),
    ] {
        let (mut ws, dir) = bulk_recovery_fixture(3);
        ws.win.layout.appearance.language = lang.into();
        let files: Vec<_> = ws.home.recoveries.iter().map(|r| r.file.clone()).collect();
        std::fs::remove_file(&files[1]).unwrap();
        std::fs::create_dir(&files[1]).unwrap();
        let mut h = Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
        h.run();
        for _ in 0..2 {
            bulk_recovery_tool(&mut h, "ui_click", serde_json::json!({"target":discard}));
        }
        assert_eq!(h.state().home.recoveries.len(), 1);
        assert_eq!(h.state().home.recoveries[0].file, files[1]);
        assert!(files[1].is_dir());
        assert!(!files[0].exists() && !files[2].exists());
        assert!(
            h.state()
                .message()
                .is_some_and(|message| message.contains(error) && message.contains("部品1"))
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn bulk_recovery_discard_is_hidden_for_fewer_than_two_copies() {
    for count in [0, 1] {
        let (ws, dir) = bulk_recovery_fixture(count);
        let mut h = Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
        h.run();
        assert!(h.query_by_label("全部捨てる").is_none());
        assert_eq!(h.state().home.recoveries.len(), count);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn bulk_recovery_discard_reconfirms_when_the_copy_list_changes() {
    let (ws, dir) = bulk_recovery_fixture(3);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    bulk_recovery_tool(
        &mut h,
        "ui_click",
        serde_json::json!({"target":"全部捨てる"}),
    );
    // 確かめている間に別の自動保存の写しが到着した場合。
    let mut arrived = h.state().home.recoveries[0].clone();
    arrived.file = dir.join("arrived.json");
    std::fs::write(&arrived.file, "届いた写し").unwrap();
    h.state_mut().home.recoveries.push(arrived);
    h.run();
    assert!(h.state().home.discard_recoveries.is_none());
    bulk_recovery_tool(
        &mut h,
        "ui_click",
        serde_json::json!({"target":"全部捨てる"}),
    );
    h.get_by_label("4 個の写しを捨てます。元に戻せません");
    assert!(h.state().home.recoveries.iter().all(|r| r.file.is_file()));
    std::fs::remove_dir_all(dir).unwrap();
}
