use super::*;
use kmb_core::{Op, Param, Transaction};

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("kmb-session-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn doc_with(expr: &str) -> Document {
    let mut d = Document::new();
    let id = d.alloc_param_id();
    d.apply(Transaction::new(
        "p",
        vec![Op::AddParam {
            index: None,
            param: Param::new(id, "w", expr),
        }],
    ))
    .unwrap();
    d
}

#[test]
fn saving_replaces_the_file_and_keeps_the_previous_version() {
    let d = temp("save");
    let p = d.join("部品.json");
    save(&p, &doc_with("1 mm")).unwrap();
    assert!(
        !backup_path(&p).exists(),
        "初めての保存では .bak を作らない"
    );
    save(&p, &doc_with("2 mm")).unwrap();
    let now = load(&p).unwrap();
    let old = load(&backup_path(&p)).unwrap();
    assert_eq!(now.data().params[0].expr, "2 mm");
    assert_eq!(old.data().params[0].expr, "1 mm");
    let left: Vec<_> = std::fs::read_dir(&d)
        .unwrap()
        .flatten()
        .filter(|e| e.path().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(left.is_empty(), "一時ファイルが残らない");
    std::fs::remove_dir_all(&d).ok();
}

// 同じフォルダで続けて確かめる:
// - 最近のファイルは新しい順で重ならず、壊れたファイルは空として読む
// - 留める・外す・一覧を消すができ、前の版の recent.json も読める
#[test]
fn recent_files_are_newest_first_pinned_and_cleared_and_survive_a_broken_file() {
    let d = temp("recent");
    let mut r = Recent::default();
    for i in 0..12 {
        r.push(&d.join(format!("{i}.json")));
    }
    r.push(&d.join("3.json"));
    assert_eq!(r.files.len(), RECENT_MAX);
    assert!(r.files[0].ends_with("3.json"));
    assert_eq!(r.files.iter().filter(|f| f.ends_with("3.json")).count(), 1);
    r.save(&d).unwrap();
    assert_eq!(Recent::load(&d), r);
    std::fs::write(d.join("recent.json"), "{壊れた").unwrap();
    assert_eq!(Recent::load(&d), Recent::default(), "読めなければ空");

    let mut r = Recent::default();
    for i in 0..3 {
        r.push(&d.join(format!("{i}.json")));
    }
    let [two, one, zero] = [r.files[0].clone(), r.files[1].clone(), r.files[2].clone()];
    // 留めたものは先頭に出て、上限を超えて押し出されても残る
    r.set_pinned(&zero, true);
    assert!(r.is_pinned(&zero));
    assert_eq!(r.list(), vec![zero.clone(), two.clone(), one.clone()]);
    for i in 10..30 {
        r.push(&d.join(format!("{i}.json")));
    }
    assert!(!r.files.contains(&zero));
    assert_eq!(r.list()[0], zero);
    // 一覧を消しても留めたものは残る。外すと消える
    r.clear_unpinned();
    assert_eq!(r.list(), vec![zero.clone()]);
    r.remove(&zero);
    assert!(r.list().is_empty());
    // 前の版の recent.json(留めの欄が無い)も読める
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("recent.json"), r#"{"files":["/a.json"]}"#).unwrap();
    assert_eq!(
        Recent::load(&d).list(),
        vec![std::path::PathBuf::from("/a.json")]
    );
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn autosave_waits_writes_and_is_recovered_until_cleared() {
    let d = temp("auto");
    let mut a = Autosave::new(&d);
    let t0 = SystemTime::now();
    assert!(!a.due(t0, false));
    assert!(!a.due(t0, true), "最初の変更からも間隔を待つ");
    assert!(!a.due(t0 + Duration::from_secs(60), true));
    assert!(a.due(t0 + AUTOSAVE_INTERVAL, true));
    let orig = d.join("部品.json");
    a.write(&doc_with("3 mm"), Some(&orig), t0 + AUTOSAVE_INTERVAL)
        .unwrap();
    let rs = recoveries(&d);
    assert_eq!(rs.len(), 1);
    assert_eq!(rs[0].original.as_deref(), Some(orig.as_path()));
    assert_eq!(restore(&rs[0]).unwrap().data().params[0].expr, "3 mm");
    a.clear();
    assert!(recoveries(&d).is_empty());
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn settings_of_unknown_modules_are_reported_and_kept() {
    let d = temp("settings");
    let ext = kmb_core::Extensions::default();
    // 設定のファイルがなければ何もしない
    assert!(load_settings(&ext, &d).is_empty());
    std::fs::write(d.join(SETTINGS_FILE), r#"{"other.module": {"a": 1}}"#).unwrap();
    let problems = load_settings(&ext, &d);
    assert_eq!(problems.len(), 1);
    assert!(problems[0].contains("other.module"), "{problems:?}");
    // 書く時も、知らない設定は消さない
    save_settings(&ext, &d).unwrap();
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(d.join(SETTINGS_FILE)).unwrap()).unwrap();
    assert_eq!(v["other.module"]["a"], 1);
    // 壊れたファイルは理由を返す
    std::fs::write(d.join(SETTINGS_FILE), "{").unwrap();
    assert_eq!(load_settings(&ext, &d).len(), 1);
}

#[test]
fn an_unlocked_session_marker_becomes_an_abnormal_exit_report() {
    let data = temp("crash-marker");
    let dir = crash::crash_dir(&data);
    std::fs::create_dir_all(&dir).unwrap();
    // 持ち主が落ちた印(鍵なし)と、動いている起動の印(鍵あり)
    let dead = dir.join("session-1-111.json");
    std::fs::write(
        &dead,
        r#"{"app":"kmb-app","version":"0.1.0","pid":111,"started":5,"commands":["押し出し"],"document":{"params":[]},"active_component":{"id":3,"name":"脚"}}"#,
    )
    .unwrap();
    let alive = dir.join("session-2-222.json");
    std::fs::write(&alive, "{}").unwrap();
    let held = std::fs::File::options()
        .read(true)
        .write(true)
        .open(&alive)
        .unwrap();
    held.try_lock().unwrap();

    assert_eq!(crash::collect_abnormal_exits(&dir), 1);
    assert!(!dead.exists());
    assert!(alive.exists(), "動いている起動の印は残す");
    let list = crash::list(&data);
    assert_eq!(list.len(), 1, "{list:?}");
    assert_eq!(list[0].kind, "abnormal_exit");
    assert_eq!(list[0].app, "kmb-app");
    let r = crash::read(&data, &list[0].name).unwrap();
    assert_eq!(r["commands"][0], "押し出し");
    assert!(r["document"].is_object());
    assert_eq!(
        r["active_component"]["name"], "脚",
        "作業中の部品も報告に移す"
    );
    // フォルダの外は読めない
    assert!(crash::read(&data, "../settings.json").is_err());
    assert!(crash::read(&data, "session-2-222.json").is_err());
    drop(held);
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
fn old_and_excess_crash_reports_are_pruned() {
    let data = temp("crash-prune");
    let dir = crash::crash_dir(&data);
    std::fs::create_dir_all(&dir).unwrap();
    for i in 0..(crash::MAX_REPORTS + 3) {
        std::fs::write(dir.join(format!("crash-{i:03}-1.json")), "{}").unwrap();
    }
    // 数の上限を超えた 3 つを消す
    let now = SystemTime::now();
    assert_eq!(crash::prune(&dir, now), 3);
    // 日数を過ぎたものは全部消す
    let later = now + crash::MAX_AGE + Duration::from_secs(60);
    assert_eq!(crash::prune(&dir, later), crash::MAX_REPORTS);
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
fn install_keeps_a_marker_with_recent_commands_until_dropped() {
    let data = temp("crash-install");
    let guard = crash::install(&data, "test").unwrap();
    let dir = crash::crash_dir(&data);
    let marker = || {
        std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .find(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("session-")
            })
    };
    let m = marker().expect("印がある");
    // 動いている間は、異常終了として拾わない
    assert_eq!(crash::collect_abnormal_exits(&dir), 0);
    crash::note(&doc_with("10 mm"), None);
    // 作業中の部品が変われば、文書が同じでも書き直す(落ちた時にどの部品で作業していたかを残す)
    crash::note(&doc_with("10 mm"), Some(7));
    let kept: serde_json::Value =
        serde_json::from_str(&crash::read_marker().expect("印を読める")).unwrap();
    assert_eq!(kept["active_component"]["id"], 7, "{kept}");
    drop(guard);
    assert!(!m.exists(), "正しく終われば印を消す");
    assert!(marker().is_none());
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
fn a_panic_is_not_reported_again_as_an_abnormal_exit() {
    let data = temp("crash-panic");
    let guard = crash::install(&data, "test").unwrap();
    let dir = crash::crash_dir(&data);
    crash::note(&doc_with("10 mm"), None);
    let _ = std::panic::catch_unwind(|| panic!("試しのパニック"));
    // パニックで終わると印が鍵の外れたまま残る。その中身を持ち主のいない印として置く
    let left = crash::read_marker().expect("印を読める");
    let dead = dir.join("session-9-999.json");
    std::fs::write(&dead, &left).unwrap();
    assert_eq!(
        crash::collect_abnormal_exits(&dir),
        0,
        "パニックの報告があるので重ねない: {left}"
    );
    assert!(!dead.exists(), "印は片付ける");
    let kinds: Vec<String> = crash::list(&data).into_iter().map(|s| s.kind).collect();
    assert_eq!(kinds, ["panic"]);
    // パニックの後も動いて作業が進めば(ここでは作業中の部品が変わる)、この後の異常終了は報告する
    crash::note(&doc_with("10 mm"), Some(3));
    let kept: serde_json::Value =
        serde_json::from_str(&crash::read_marker().expect("印を読める")).unwrap();
    assert!(kept["panic_report"].is_null(), "{kept}");
    drop(guard);
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
fn the_log_filter_reads_levels_per_target() {
    use log::LevelFilter as L;
    let (f, bad) = logging::Filter::parse("");
    assert!(bad.is_empty());
    assert_eq!(f.level("kmb_core::recompute"), L::Info);
    assert_eq!(f.level("kie_monobench"), L::Info);
    assert_eq!(
        f.level("wgpu_core::device"),
        L::Warn,
        "ほかのライブラリは注意から"
    );

    let (f, bad) =
        logging::Filter::parse("debug, kmb_cam=trace,kmb_cam_commands::sheet=warn, *=info, x=loud");
    assert_eq!(bad, ["x=loud"], "読めない項目は飛ばして知らせる");
    assert_eq!(f.level("kmb_core"), L::Debug);
    assert_eq!(f.level("kmb_cam_commands::entry"), L::Trace);
    assert_eq!(
        f.level("kmb_cam_commands::sheet"),
        L::Warn,
        "長く一致した方を使う"
    );
    assert_eq!(f.level("wgpu_core"), L::Info);
    assert_eq!(f.max(), L::Trace);
}

#[test]
fn the_log_is_written_line_by_line_and_old_files_are_removed() {
    use log::Log;
    let d = temp("log");
    let logs = logging::log_dir(&d);
    std::fs::create_dir_all(&logs).unwrap();
    for i in 0..logging::MAX_FILES + 3 {
        let p = logs.join(format!("test-old{i:02}.log"));
        std::fs::write(&p, "x\n").unwrap();
        let past = SystemTime::now() - Duration::from_secs(3600 - i as u64);
        std::fs::File::options()
            .write(true)
            .open(&p)
            .unwrap()
            .set_modified(past)
            .unwrap();
    }
    // ほかのアプリのログは消さない
    std::fs::write(logs.join("kmb-cli-1-1.log"), "x\n").unwrap();
    let (filter, _) = logging::Filter::parse("");
    let logger = logging::Logger::new(Some(&logs), "test", filter, false);
    let path = logger.path().unwrap().to_path_buf();
    assert_eq!(
        logging::files(&logs, "test").len(),
        logging::MAX_FILES,
        "古いものから消す"
    );
    assert_eq!(logging::files(&logs, "test")[0], path, "新しい順");
    assert_eq!(logging::files(&logs, "kmb-cli").len(), 1);

    let rec = |level, target: &str, msg: &str| {
        logger.log(
            &log::Record::builder()
                .level(level)
                .target(target)
                .args(format_args!("{msg}"))
                .build(),
        )
    };
    rec(log::Level::Info, "kmb_core::document", "変更: 押し出し");
    rec(log::Level::Debug, "kmb_core::recompute", "書かない");
    rec(log::Level::Info, "wgpu_core", "書かない");
    rec(log::Level::Warn, "wgpu_core", "描画の注意");

    // 閉じなくても(落ちても)行は書けている
    let text = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "{text}");
    assert!(
        lines[0].starts_with("20"),
        "UTC の日時で始まる: {}",
        lines[0]
    );
    assert!(lines[0].contains(" INFO  ["), "{}", lines[0]);
    assert!(
        lines[0].ends_with("] kmb_core::document: 変更: 押し出し"),
        "{}",
        lines[0]
    );
    assert!(lines[1].contains("WARN") && lines[1].contains("描画の注意"));
    assert_eq!(logger.recent().len(), 2);
    assert_eq!(logging::tail(&path, 1), [lines[1].to_string()]);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn log_times_are_written_in_utc() {
    let t = SystemTime::UNIX_EPOCH + Duration::from_millis(1_791_979_200_123);
    assert_eq!(logging::utc(t), "2026-10-14T12:00:00.123Z");
    assert_eq!(
        logging::utc(SystemTime::UNIX_EPOCH),
        "1970-01-01T00:00:00.000Z"
    );
    let leap = SystemTime::UNIX_EPOCH + Duration::from_secs(951_782_400); // 2000-02-29
    assert_eq!(logging::utc(leap), "2000-02-29T00:00:00.000Z");
}

#[test]
fn an_abnormal_exit_report_carries_the_end_of_that_run_log() {
    let data = temp("crash-log");
    let dir = crash::crash_dir(&data);
    std::fs::create_dir_all(&dir).unwrap();
    let log = data.join("run.log");
    std::fs::write(&log, "1 行目\n2 行目\nSMLib の足し算を始める\n").unwrap();
    std::fs::write(
        dir.join("session-3-333.json"),
        serde_json::json!({"app": "kmb-app", "commands": [], "log_file": log}).to_string(),
    )
    .unwrap();
    assert_eq!(crash::collect_abnormal_exits(&dir), 1);
    let list = crash::list(&data);
    let r = crash::read(&data, &list[0].name).unwrap();
    assert_eq!(r["log_file"], serde_json::json!(log));
    assert_eq!(r["log"].as_array().unwrap().len(), 3);
    assert_eq!(r["log"][2], "SMLib の足し算を始める");
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
fn removed_parameters_stay_in_the_document_and_can_be_restored() {
    // まとめてレビュー N2: 消したパラメータを「消した物も出す」で出し、「戻す」で戻す
    let mut d = Document::new();
    let tx = crate::edit::add_param(&mut d, "gap", "3 mm");
    d.apply(tx).unwrap();
    let id = d.data().params[0].id;
    d.apply(crate::edit::remove_param(d.data(), id)).unwrap();
    assert!(d.data().params.is_empty());
    assert_eq!(d.data().removed_params[0].name, "gap");
    // 保存しても残る
    let back = Document::from_json(&d.to_json()).unwrap();
    assert_eq!(back.data().removed_params.len(), 1);
    // 同じ名前が使われていれば戻さない
    let tx = crate::edit::add_param(&mut d, "gap", "1 mm");
    d.apply(tx).unwrap();
    let err = crate::edit::restore_param(d.data(), &[], id).unwrap_err();
    assert!(err.contains("gap"), "{err}");
    assert!(d.undo());
    // 寸法の名前と同じでも戻さない
    assert!(crate::edit::restore_param(d.data(), &["gap".into()], id).is_err());
    let tx = crate::edit::restore_param(d.data(), &[], id).unwrap();
    d.apply(tx).unwrap();
    assert_eq!(d.data().params[0].id, id, "前と同じ番号で戻る");
    assert!(d.data().removed_params.is_empty());
    // 戻すのも消すのも取り消せる
    assert!(d.undo());
    assert!(d.data().params.is_empty());
    assert!(d.undo());
    assert_eq!(d.data().params[0].name, "gap");
    assert!(d.data().removed_params.is_empty());
}

#[test]
fn favorites_are_one_view_and_rows_move_within_a_view() {
    use kmb_core::{ParamKey, RecomputeReport, Registry};
    let (reg, report) = (Registry::default(), RecomputeReport::default());
    let mut d = Document::new();
    for n in ["a", "b", "c"] {
        let tx = crate::edit::add_param(&mut d, n, "1 mm");
        d.apply(tx).unwrap();
    }
    let key = |n: &str| ParamKey::User {
        id: d.data().param_by_name(n).unwrap().id,
    };
    let (a, b, c) = (key("a"), key("b"), key("c"));
    // ☆ を初めて押すとお気に入りの表ができる。2 つめは同じ表に入る
    let tx = crate::params::set_favorite(&mut d, &c, true);
    d.apply(tx).unwrap();
    let tx = crate::params::set_favorite(&mut d, &a, true);
    d.apply(tx).unwrap();
    assert_eq!(d.data().param_views.len(), 1);
    let fav = d.data().favorites_view().unwrap().clone();
    assert_eq!(fav.members, vec![c.clone(), a.clone()]);
    assert!(crate::params::is_favorite(d.data(), &a));
    assert!(!crate::params::is_favorite(d.data(), &b));
    // お気に入りの表は 2 つ持てない
    let mut two = d.data().param_views.clone();
    two.push(kmb_core::ParamView {
        id: d.alloc_view_id(),
        ..fav.clone()
    });
    assert!(
        d.apply(kmb_core::Transaction::new(
            "x",
            vec![Op::SetParamViews { views: two }]
        ))
        .is_err()
    );
    // 上へ・下へ(端では何もしない)
    let up = crate::params::move_in_view(d.data(), &reg, &report, fav.id, &a, true)
        .unwrap()
        .unwrap();
    d.apply(up).unwrap();
    assert_eq!(
        d.data().favorites_view().unwrap().members,
        vec![a.clone(), c]
    );
    assert!(
        crate::params::move_in_view(d.data(), &reg, &report, fav.id, &a, true)
            .unwrap()
            .is_none()
    );
    // 外す
    let tx = crate::params::set_favorite(&mut d, &a, false);
    d.apply(tx).unwrap();
    assert!(!crate::params::is_favorite(d.data(), &a));
    // 保存して開き直しても、お気に入りの印は残る
    let back = Document::from_json(&d.to_json()).unwrap();
    assert!(back.data().favorites_view().is_some());
}

/// settings.json が壊れていても、保存でほかのモジュールの設定ごと上書きしない。壊れたファイルは
/// `settings.json.broken-<時刻>` に移してから書き、毎回の保存は直前の中身を `.bak` に残す。
#[test]
fn a_broken_settings_file_is_kept_aside_before_it_is_replaced() {
    let d = temp("settings-broken");
    let ext = kmb_core::Extensions::default();
    let broken = r#"{"cam.machine": {"id": "carvera-air", "#;
    std::fs::write(d.join(SETTINGS_FILE), broken).unwrap();
    save_settings(&ext, &d).unwrap();
    let kept: Vec<_> = std::fs::read_dir(&d)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("settings.json.broken-")
        })
        .collect();
    assert_eq!(kept.len(), 1, "壊れたファイルを残す");
    assert_eq!(std::fs::read_to_string(kept[0].path()).unwrap(), broken);
    // 書いた方は読める
    let text = std::fs::read_to_string(d.join(SETTINGS_FILE)).unwrap();
    serde_json::from_str::<serde_json::Value>(&text).unwrap();
    // 読める設定を書き直す時は、直前の中身を .bak に残す
    std::fs::write(d.join(SETTINGS_FILE), r#"{"other.module": {"a": 1}}"#).unwrap();
    save_settings(&ext, &d).unwrap();
    let bak = std::fs::read_to_string(backup_path(&d.join(SETTINGS_FILE))).unwrap();
    assert_eq!(bak, r#"{"other.module": {"a": 1}}"#);
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn outputs_are_written_whole_and_binary_stays_binary() {
    let d = temp("write-output");
    let p = d.join("部品.stl");
    // 文字でない中身(二進の STL・PNG など)もそのまま。置き換えなので一時ファイルは残らない
    let bytes: Vec<u8> = (0..=255u8).collect();
    write_output(&p, &bytes).unwrap();
    assert_eq!(std::fs::read(&p).unwrap(), bytes);
    write_output(&p, b"2").unwrap();
    assert_eq!(std::fs::read(&p).unwrap(), b"2");
    let names: Vec<_> = std::fs::read_dir(&d)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, vec![std::ffi::OsString::from("部品.stl")]);
    // 書けない場所は誤り(フォルダが無い)
    assert!(write_output(&d.join("無い").join("x.nc"), b"G0").is_err());
}

/// 次の起動で見つけた異常終了の報告は、見つけた時刻でなく、落ちた起動が
/// 最後に印を書いた時刻を `time` にし、見つけた時刻を `detected` に、遅れて見つけた印を残す。
#[test]
fn an_abnormal_exit_found_later_says_so_and_keeps_the_last_noted_time() {
    let data = temp("crash-later");
    let dir = crash::crash_dir(&data);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("session-1-333.json"),
        r#"{"app":"kmb-app","version":"0.1.0","pid":333,"started":50,"noted":120,"commands":[]}"#,
    )
    .unwrap();
    assert_eq!(crash::collect_abnormal_exits(&dir), 1);
    let list = crash::list(&data);
    assert_eq!(list.len(), 1, "{list:?}");
    assert_eq!(list[0].time, 120, "最後に印を書いた時刻");
    assert!(list[0].detected_later);
    let r = crash::read(&data, &list[0].name).unwrap();
    assert_eq!(r["started"], 50, "起動した時刻は書き直しで変わらない");
    assert!(r["detected"].as_u64().unwrap() > 120, "見つけた時刻: {r}");
    assert!(
        r["message"]
            .as_str()
            .unwrap()
            .contains("次の起動で見つけた")
    );
}

#[test]
fn a_length_field_with_an_angle_is_told_at_once() {
    use crate::params::unit_mismatch;
    use kmb_core::{Dim, FieldUnit};
    assert!(unit_mismatch(Some(&FieldUnit::Length), Dim::ANGLE).is_some());
    assert!(unit_mismatch(Some(&FieldUnit::Angle), Dim::LENGTH).is_some());
    assert!(unit_mismatch(Some(&FieldUnit::Length), Dim::LENGTH).is_none());
    // 送りの速さのように長さの欄で別の量を書く物は咎めない
    assert!(unit_mismatch(Some(&FieldUnit::Length), Dim::NONE).is_none());
    assert!(unit_mismatch(None, Dim::ANGLE).is_none());
}

#[test]
fn the_data_folder_of_the_old_name_is_carried_over_once_and_kept() {
    let d = temp("carry-over");
    let old = d.join(OLD_DATA_DIR_NAME);
    let new = d.join(DATA_DIR_NAME);
    std::fs::create_dir_all(old.join("data")).unwrap();
    std::fs::create_dir_all(old.join("logs")).unwrap();
    std::fs::write(old.join("settings.json"), "{\"units\":\"in\"}").unwrap();
    std::fs::write(old.join("data").join("app.ron"), "(window)").unwrap();
    std::fs::write(old.join("logs").join("a.log"), "old log").unwrap();
    assert!(carry_over(&old, &new).unwrap());
    // 設定と窓の記憶は写り、ログは写らない。前のフォルダは残る
    assert_eq!(
        std::fs::read_to_string(new.join("settings.json")).unwrap(),
        "{\"units\":\"in\"}"
    );
    assert_eq!(
        std::fs::read_to_string(new.join("data").join("app.ron")).unwrap(),
        "(window)"
    );
    assert!(!new.join("logs").exists());
    assert!(old.join("settings.json").exists());
    assert!(!new.with_extension("moving").exists());
    // 新しいフォルダができた後は写し直さない(新しい方の変更を古い方で上書きしない)
    std::fs::write(new.join("settings.json"), "{\"units\":\"mm\"}").unwrap();
    assert!(!carry_over(&old, &new).unwrap());
    assert_eq!(
        std::fs::read_to_string(new.join("settings.json")).unwrap(),
        "{\"units\":\"mm\"}"
    );
    // 前のフォルダが無ければ何もしない
    let fresh = d.join("fresh");
    assert!(!carry_over(&d.join("none"), &fresh).unwrap());
    assert!(!fresh.exists());
    std::fs::remove_dir_all(d).ok();
}

fn semantic_guard_refuses_before_save(expr: &str, suffix: &str) {
    let mut h = Headless::with_document(kmb_bundle::modules(), doc_with("10 mm"), None);
    let before = h.doc().to_json();
    let result = h.set_param("w", expr);
    if result.is_ok() {
        let dir = temp(&format!("semantic-guard-{suffix}"));
        let path = dir.join("shape.json");
        save(&path, h.doc()).unwrap();
        let saved = load(&path).unwrap();
        assert_eq!(
            saved.data().param_by_name("w").unwrap().expr,
            expr,
            "誤った式の保存まで通る"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
    assert!(result.is_err(), "誤った式を保存前に断る: {expr}");
    assert_eq!(h.doc().to_json(), before, "誤りなら文書を変えない");
}

#[test]
fn semantic_guard_refuses_unknown_names_before_saving() {
    semantic_guard_refuses_before_save("missing_name + 1 mm", "unknown");
}

#[test]
fn semantic_guard_refuses_incompatible_units_before_saving() {
    semantic_guard_refuses_before_save("1 mm + 1 deg", "units");
}

#[test]
fn semantic_guard_refuses_a_self_reference_before_saving() {
    semantic_guard_refuses_before_save("w + 1 mm", "cycle");
}

#[test]
fn semantic_guard_refuses_an_invalid_configuration_cell_before_saving() {
    let mut h = Headless::with_document(kmb_bundle::modules(), doc_with("10 mm"), None);
    h.run(
        "cad.config.add_column",
        None,
        None,
        &serde_json::json!({"param":"w"}),
    )
    .unwrap();
    h.run(
        "cad.config.add_row",
        None,
        None,
        &serde_json::json!({"name":"row"}),
    )
    .unwrap();
    let before = h.doc().to_json();
    let result = h.run(
        "cad.config.set",
        None,
        None,
        &serde_json::json!({"row":"row","column":1,"value":"missing_name + 1 mm"}),
    );
    let problem = result.unwrap_err();
    assert_eq!(problem.kind, kmb_core::ProblemKind::Input);
    assert_eq!(h.doc().to_json(), before);
}

#[test]
fn semantic_guard_rejects_a_new_parameter_without_consuming_an_id() {
    let mut h = Headless::with_document(kmb_bundle::modules(), doc_with("10 mm"), None);
    let before = h.doc().to_json();
    let mark = h.mark();
    let why = h.add_param("new_value", "missing_name + 1 mm").unwrap_err();
    assert!(why.contains("missing_name"), "{why}");
    assert_eq!(h.doc().to_json(), before);
    assert_eq!(h.mark(), mark);
    h.add_param("new_value", "w * 2").unwrap();
    let before: serde_json::Value = serde_json::from_str(&before).unwrap();
    assert_eq!(
        h.param_id("new_value").unwrap().0,
        before["next_id"].as_u64().unwrap()
    );
}

struct GuardDimensions(serde_json::Value);
impl kmb_core::Feature for GuardDimensions {
    fn inputs(&self) -> Result<Vec<kmb_core::InputRef>, kmb_core::EvalError> {
        Ok(vec![])
    }
    fn evaluate(
        &self,
        _: &mut kmb_core::EvalCtx<'_>,
    ) -> Result<kmb_core::Outputs, kmb_core::EvalError> {
        for key in ["size", "extra"] {
            let q = kmb_core::eval_str(self.0[key].as_str().unwrap(), &|_| None)?;
            kmb_core::ParamUnit::Length
                .check(q.dim)
                .map_err(kmb_core::EvalError::InvalidData)?;
        }
        Ok(Default::default())
    }
    fn model_params(&self) -> Vec<kmb_core::ModelParam> {
        vec![kmb_core::ModelParam {
            key: "size".into(),
            name: "size_dim".into(),
            expr: self.0["size"].as_str().unwrap().into(),
            label: "寸法".into(),
        }]
    }
    fn set_model_param(&self, name: &str, expr: &str) -> Option<serde_json::Value> {
        if name != "size_dim" {
            return None;
        }
        let mut data = self.0.clone();
        data["size"] = expr.into();
        Some(data)
    }
}

fn guard_modules() -> kmb_core::Modules {
    let module = kmb_core::Module::new("test", |r| {
        r.register_described(
            kmb_core::Descriptor::new("test.sized@1", kmb_core::category::SOLID, "寸法の見本")
                .field(kmb_core::Field::expr("extra", "追加の長さ").length()),
            |data| Ok(Box::new(GuardDimensions(data.clone()))),
        );
    });
    kmb_core::Modules::assemble(vec![module], std::sync::Arc::new(kmb_core::NullKernel))
}

fn guard_dimensions() -> (Headless, kmb_core::FeatureId) {
    let mut h = Headless::new(guard_modules());
    let id = h
        .add_feature(
            "test.sized@1",
            Some("寸法の見本"),
            serde_json::json!({"size":"2 mm","extra":"3 mm"}),
        )
        .unwrap();
    (h, id)
}

#[test]
fn semantic_guard_refuses_bad_model_dimension_expressions() {
    let (mut h, _) = guard_dimensions();
    let before = h.doc().to_json();
    for expr in ["30 deg", "missing_name", "size_dim + 1 mm"] {
        assert!(h.set_model_param("size_dim", expr).is_err(), "{expr}");
        assert_eq!(h.doc().to_json(), before);
    }
}

#[test]
fn semantic_guard_refuses_bad_feature_value_expressions() {
    let (mut h, id) = guard_dimensions();
    let before = h.doc().to_json();
    for expr in ["30 deg", "1 mm + 1 deg"] {
        assert!(h.set_value(id, "extra", expr).is_err(), "{expr}");
        assert_eq!(h.doc().to_json(), before);
    }
}

#[test]
fn semantic_guard_keeps_legacy_errors_readable_and_allows_repair() {
    let (h, id) = guard_dimensions();
    let mut data = h.doc().data().clone();
    data.features[0].data["size"] = "30 deg".into();
    data.features[0].data["extra"] = "30 deg".into();
    let json = serde_json::to_string(&data).unwrap();
    let doc = Document::from_json(&json).unwrap();
    let saved = doc.to_json();
    let mut h = Headless::with_document(guard_modules(), doc, None);
    assert_eq!(h.doc().to_json(), saved);
    assert!(h.report().error_count() > 0);
    let rows = crate::params::feature_values(h.doc().data(), &h.modules().registry, h.report());
    assert!(matches!(
        rows[0].status,
        kmb_core::ParamStatus::Error { .. }
    ));
    h.set_value(id, "extra", "4 mm").unwrap();
    h.set_model_param("size_dim", "5 mm").unwrap();
    assert_eq!(h.report().error_count(), 0);
}

#[test]
fn semantic_guard_legacy_parameter_errors_do_not_block_comments_or_other_rows() {
    let mut h = Headless::with_document(kmb_bundle::modules(), doc_with("missing_name"), None);
    h.set_param_info(
        "w",
        None,
        &crate::edit::ParamInfo {
            comment: Some("直す必要のある式".into()),
            unit: None,
        },
    )
    .unwrap();
    h.add_param("other", "2 mm").unwrap();
    assert_eq!(h.doc().data().params[0].expr, "missing_name");
    assert!(matches!(
        h.report().params[0].status,
        kmb_core::ParamStatus::Error { .. }
    ));
    h.set_param("w", "3 mm").unwrap();
    assert_eq!(h.report().error_count(), 0);
}

#[test]
fn semantic_guard_importing_bad_new_rows_keeps_the_document_and_id_counter() {
    let mut h = Headless::with_document(kmb_bundle::modules(), doc_with("10 mm"), None);
    let before = h.doc().to_json();
    let mark = h.mark();
    let bad = "row,expr\nw,20 mm\nfresh,missing_name + 1 mm\n";
    assert!(h.import_params(bad, crate::param_io::Format::Csv).is_err());
    assert_eq!(h.doc().to_json(), before);
    assert_eq!(h.mark(), mark);
    let done = h
        .import_params(
            "row,expr\nw,20 mm\nfresh,w * 2\n",
            crate::param_io::Format::Csv,
        )
        .unwrap();
    assert_eq!((done.added, done.changed), (1, 1));
    assert_eq!(h.mark(), mark + 1);
    assert!(
        matches!(h.report().params.iter().find(|p| p.name=="fresh").unwrap().status,kmb_core::ParamStatus::Ok { value } if value.value==40.0)
    );
}
