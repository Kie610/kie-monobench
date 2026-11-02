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

fn small_dxf(path: &std::path::Path) {
    std::fs::write(path, b"0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n10\n21\n0\n0\nLINE\n8\n0\n10\n0\n20\n5\n11\n10\n21\n5\n0\nENDSEC\n0\nEOF\n").unwrap();
}

/// 確かめること: DXF は別の未保存タブとなり、1 回の取り消しとやり直しで空/読んだ形へ戻る。
#[test]
fn a_dxf_opens_in_a_new_unsaved_tab_with_one_undo_step() {
    let d = temp_dir("open-dxf");
    let file = d.join("外の 図.DXF");
    small_dxf(&file);
    let mut ws = Workspace::new(kmb_samples::reference_document(), None, reg()).with_dialogs(
        Box::new(FakeDialogs {
            open: None,
            save: Some(d.join("保存した設計")),
            unsaved: Unsaved::Cancel,
        }),
    );
    ws.add_param("extra", "1 mm");
    let original = ws.document().data().clone();
    let undo: Vec<String> = ws
        .document()
        .undo_labels()
        .into_iter()
        .map(str::to_owned)
        .collect();
    let volumes: Vec<_> = ws
        .visible_bodies()
        .iter()
        .map(|(_, h)| ws.cur.work.kernel().volume(*h).unwrap())
        .collect();
    assert!(ws.open_in_tab(&file));
    assert_eq!((ws.tab_count(), ws.active_tab()), (2, 1));
    assert_eq!(ws.tab_titles()[1], ("外の 図".into(), true));
    assert!(ws.path().is_none());
    assert_eq!(ws.document().data().features.len(), 1);
    let sketch: kmb_sketch::Sketch =
        serde_json::from_value(ws.document().data().features[0].data.clone()).unwrap();
    assert_eq!(
        sketch
            .entities
            .iter()
            .filter(|e| matches!(e, kmb_sketch::Entity::Line { .. }))
            .count(),
        2
    );
    assert_eq!(ws.document().undo_labels().len(), 1);
    let imported = ws.document().data().clone();
    ws.undo();
    assert!(ws.document().data().features.is_empty());
    assert!(!ws.is_modified());
    ws.redo();
    assert_eq!(ws.document().data(), &imported);
    assert!(ws.is_modified());
    let source = std::fs::read(&file).unwrap();
    assert_eq!(ws.document_name(), "外の 図");
    ws.save();
    let saved = d.join(format!("保存した設計.{}", kmb_core::DOCUMENT_EXTENSION));
    assert_eq!(ws.path(), Some(saved.as_path()));
    assert!(!ws.is_modified());
    assert_eq!(
        std::fs::read(&file).unwrap(),
        source,
        "元の DXF は保存先にしない"
    );
    assert_eq!(kmb_session::load(&saved).unwrap().data().features.len(), 1);
    ws.switch_tab(0);
    assert_eq!(ws.document().data(), &original);
    assert_eq!(ws.document().undo_labels(), undo);
    assert!(ws.is_modified());
    let now: Vec<_> = ws
        .visible_bodies()
        .iter()
        .map(|(_, h)| ws.cur.work.kernel().volume(*h).unwrap())
        .collect();
    assert_eq!(now, volumes);
    std::fs::remove_dir_all(&d).unwrap();
}

/// 確かめること: 読めない・空の DXF は、新しいタブも元の文書の形の破棄も起こさない。
#[test]
fn a_failed_dxf_open_preserves_the_document_tabs_history_and_shapes() {
    let d = temp_dir("bad-dxf");
    let mut ws = Workspace::new(kmb_samples::reference_document(), None, reg());
    ws.add_param("extra", "1 mm");
    ws.add_param("redo", "2 mm");
    ws.undo();
    let before = ws.document().data().clone();
    let undo: Vec<String> = ws
        .document()
        .undo_labels()
        .into_iter()
        .map(str::to_owned)
        .collect();
    let redo: Vec<String> = ws
        .document()
        .redo_labels()
        .into_iter()
        .map(str::to_owned)
        .collect();
    let tab_seq = ws.win.tab_seq;
    let bodies = ws.visible_bodies();
    let volumes: Vec<_> = bodies
        .iter()
        .map(|(_, h)| ws.cur.work.kernel().volume(*h).unwrap())
        .collect();
    for (name, bytes) in [
        ("壊れた.dxf", "壊れた図面"),
        ("空.dxf", "0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n"),
    ] {
        let path = d.join(name);
        std::fs::write(&path, bytes).unwrap();
        assert!(!ws.open_in_tab(&path));
        assert_eq!(
            (ws.tab_count(), ws.active_tab(), ws.win.tab_seq),
            (1, 0, tab_seq)
        );
        assert_eq!(ws.document().data(), &before);
        assert_eq!(ws.document().undo_labels(), undo);
        assert_eq!(ws.document().redo_labels(), redo);
        assert!(ws.is_modified());
        for ((_, h), volume) in bodies.iter().zip(&volumes) {
            assert_eq!(ws.cur.work.kernel().volume(*h).unwrap(), *volume);
        }
    }
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn a_dxf_is_opened_from_the_start_screen_as_an_unsaved_design() {
    let d = temp_dir("start-dxf");
    let file = d.join("輪郭.dxf");
    small_dxf(&file);
    let ws = Workspace::new(Document::new(), None, reg())
        .with_dialogs(Box::new(FakeDialogs {
            open: Some(file),
            save: None,
            unsaved: Unsaved::Cancel,
        }))
        .show_start();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000., 700.))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h.get_by_label("開く…").click();
    h.run();
    assert!(!h.state().is_start_screen());
    assert_eq!(h.state().tab_count(), 1);
    assert_eq!(h.state().tab_titles(), [("輪郭".to_string(), true)]);
    assert!(h.state().path().is_none());
    assert_eq!(h.state().document().data().features.len(), 1);
    std::fs::remove_dir_all(&d).unwrap();
}
