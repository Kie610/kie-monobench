//! 画面の骨組みの通しの試験(取り消し・裏の再計算・コマンド・ヘルプ)と、作業空間ごとの試験
//! (design・machining・files の `*_tests`)が使う手伝い。

use super::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use jc_core::Document;
use jc_samples::{reference_document, sample_document};

/// 操作が今使えるか。
pub(crate) fn can(ws: &Workspace, id: &str) -> bool {
    ws.actions()
        .iter()
        .any(|a| a.id == id && a.disabled.is_none())
}

/// 3D のビューの線(早送りなら真)。
pub(crate) fn path_lines(ws: &Workspace) -> Vec<(Vec<[f64; 3]>, bool)> {
    ws.overlay()
        .lines
        .into_iter()
        .map(|(p, k)| {
            (
                p,
                matches!(
                    k.ink,
                    jc_interchange::ink::line::TRAVEL | jc_interchange::ink::line::TRAVEL_LOUD
                ),
            )
        })
        .collect()
}

#[test]
fn bad_input_is_reported_without_changing_the_document() {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .build_ui_state(
            |ui, ws: &mut Workspace| ws.ui(ui),
            Workspace::new(sample_document(), None, reg()),
        );
    let before = h.state().document().data().clone();
    assert!(!h.state_mut().add_param("mm", "1"));
    assert!(h.state().message().is_some());
    assert_eq!(h.state().document().data().params, before.params);
    h.run();
}

#[test]
fn background_recompute_matches_the_direct_one() {
    let mut direct = Workspace::new(reference_document(), None, reg());
    let mut bg = Workspace::open(reference_document(), None, true, reg());
    assert!(bg.is_busy());
    bg.wait();
    assert!(!bg.is_busy());
    assert_eq!(bg.report().error_count(), 0);
    assert_eq!(bg.report().features.len(), direct.report().features.len());
    assert!(!bg.cur.scene.is_empty());

    // 計算中に 2 回変えても、最後の値で計算し直される
    let width = bg.document().data().param_by_name("width").unwrap().id;
    bg.set_param_expr(width, "70 mm");
    assert!(bg.is_busy());
    bg.set_param_expr(width, "75 mm");
    bg.wait();
    direct.set_param_expr(width, "75 mm");
    let vol = |ws: &Workspace| {
        ws.cur
            .work
            .kernel()
            .mass(ws.visible_bodies()[0].1)
            .unwrap()
            .volume
    };
    // 作り直しの順(60→70→75 と 60→75)で truck の再試行の許容差が変わることがあるので、
    // 一致は理論値との差の範囲で見る
    assert!(
        (vol(&bg) - vol(&direct)).abs() < 0.5,
        "{} と {}",
        vol(&bg),
        vol(&direct)
    );
    assert!((vol(&bg) - jc_samples::reference_values(75.0).0).abs() < 1.0);
}

/// 採用分 10-16 E5(試験だけ): 重い形の機能を編集パネルで直している間も、画面は計算を待たない。
/// 直した値は文書にすぐ入り、形は裏で計算して後から入れ替わる(その場で計算する時より、直す呼び出しが
/// ずっと短い)。
#[test]
fn editing_a_feature_of_a_heavy_part_does_not_hold_the_screen() {
    // 穴を 2 列 × 8 個に増やして重くする
    let heavy = || {
        let part = jc_samples::reference_part();
        let mut doc = part.doc;
        let mut d = doc.data().feature(part.hole_pattern).unwrap().data.clone();
        d["count"] = serde_json::json!("8");
        d["spacing"] = serde_json::json!("6 mm");
        let tx = jc_session::edit::set_feature_data(doc.data(), part.hole_pattern, d);
        doc.apply(tx).unwrap();
        (doc, part.pocket)
    };
    let (doc, pocket) = heavy();
    let mut direct = Workspace::new(doc, None, reg());
    let (doc, _) = heavy();
    let mut bg = Workspace::open(doc, None, true, reg());
    bg.wait();
    assert_eq!(bg.report().error_count(), 0);
    let deeper = |ws: &Workspace| {
        let mut d = ws.document().data().feature(pocket).unwrap().data.clone();
        d["distance"] = serde_json::json!("6 mm");
        d
    };
    // その場で計算する組み立て(CLI と同じ)にかかる時間
    let t = std::time::Instant::now();
    let d = deeper(&direct);
    assert!(direct.set_feature_data(pocket, d));
    let computed = t.elapsed();
    // 画面: 編集パネルを開いて深さを直す
    bg.open_edit(pocket);
    let entry = bg.document().data().feature(pocket).unwrap().clone();
    let t = std::time::Instant::now();
    bg.set_panel_data(&entry, deeper(&bg));
    let held = t.elapsed();
    assert_eq!(
        bg.document().data().feature(pocket).unwrap().data["distance"],
        "6 mm",
        "値はすぐ文書に入る"
    );
    assert!(bg.is_busy(), "形は裏で計算する");
    assert!(
        held * 4 < computed,
        "直す呼び出し {held:?} が計算 {computed:?} を待っている"
    );
    bg.wait();
    assert_eq!(outcome(&bg), outcome(&direct));
}

/// 機能ごとの計算の具合と、見えるボディの体積(使い回しの数と形状の番号は比べない)。
fn outcome(ws: &Workspace) -> (Vec<String>, Vec<i64>) {
    let kinds = ws
        .report()
        .features
        .iter()
        .map(|f| {
            let k = match &f.status {
                jc_core::FeatureStatus::Ok { .. } => "ok",
                jc_core::FeatureStatus::Error { .. } => "error",
                jc_core::FeatureStatus::Blocked { .. } => "blocked",
                jc_core::FeatureStatus::Suppressed => "suppressed",
                jc_core::FeatureStatus::RolledBack => "rolled_back",
            };
            format!("{}:{k}", f.name)
        })
        .collect();
    let vols = ws
        .visible_bodies()
        .iter()
        .map(|b| (ws.cur.work.kernel().mass(b.1).unwrap().volume * 10.0).round() as i64)
        .collect();
    (kinds, vols)
}

/// 走っている裏の再計算が、やめる印を立てられているか(終わった後で計算し直す印も)。
fn superseded(ws: &Workspace) -> bool {
    ws.cur.work.is_superseded()
}

#[test]
fn undo_redo_and_rollback_compute_in_the_background_and_drop_the_old_compute() {
    let mut direct = Workspace::new(reference_document(), None, reg());
    let mut bg = Workspace::open(reference_document(), None, true, reg());
    bg.wait();
    let width = bg.document().data().param_by_name("width").unwrap().id;
    let vol = |ws: &Workspace| {
        ws.cur
            .work
            .kernel()
            .mass(ws.visible_bodies()[0].1)
            .unwrap()
            .volume
    };
    let before = vol(&bg);
    for ws in [&mut direct, &mut bg] {
        ws.set_param_expr(width, "70 mm");
        ws.set_param_expr(width, "75 mm");
    }
    // 値を続けて変える時は、途中の形を出すため走っている計算をやめない
    assert!(bg.is_busy() && !superseded(&bg));
    bg.wait();

    // 取り消し: 文書はすぐ戻り、形は裏で計算する。続けて押すと前の計算はやめる
    bg.undo();
    assert!(bg.is_busy() && !superseded(&bg));
    assert_eq!(
        bg.document().data().param_by_name("width").unwrap().expr,
        "70 mm"
    );
    bg.undo();
    assert!(superseded(&bg), "2 回目の取り消しで 1 回目の計算をやめる");
    assert_eq!(
        bg.document().data().param_by_name("width").unwrap().expr,
        "60 mm"
    );
    assert!(bg.busy_for().is_some());
    bg.wait();
    assert!(!bg.is_busy() && bg.busy_for().is_none());
    direct.undo();
    direct.undo();
    assert_eq!(outcome(&bg), outcome(&direct));
    assert!((vol(&bg) - before).abs() < 0.5);

    // やり直しと履歴の窓の移動も同じ
    bg.redo();
    bg.go_to_history(bg.document().undo_labels().len() + 1);
    assert!(superseded(&bg));
    bg.wait();
    direct.redo();
    direct.redo();
    assert_eq!(outcome(&bg), outcome(&direct));

    // ロールバックの棒: 行き先だけを計算する
    bg.step_rollback(-1);
    assert!(
        bg.is_busy() && !superseded(&bg),
        "この変更で始めた計算はやめない"
    );
    bg.step_rollback(-1);
    assert!(superseded(&bg));
    bg.wait();
    direct.step_rollback(-1);
    direct.step_rollback(-1);
    assert_eq!(outcome(&bg), outcome(&direct));
    assert!(outcome(&bg).0.iter().any(|k| k.ends_with(":rolled_back")));
    assert_eq!(bg.cur.scene.key().len(), direct.cur.scene.key().len());
}

/// 決まった答えを返すダイアログ。
pub(crate) struct FakeDialogs {
    pub(crate) open: Option<std::path::PathBuf>,
    pub(crate) save: Option<std::path::PathBuf>,
    pub(crate) unsaved: Unsaved,
}

impl Dialogs for FakeDialogs {
    fn open_document(&self) -> Option<std::path::PathBuf> {
        self.open.clone()
    }
    fn save_document(&self, _: &str) -> Option<std::path::PathBuf> {
        self.save.clone()
    }
    fn open_file(&self, _: &str, _: &str, _: &[&str]) -> Option<std::path::PathBuf> {
        None
    }
    fn save_file(&self, _: &str, _: &str, _: &[&str], _: &str) -> Option<std::path::PathBuf> {
        self.save.clone()
    }
    fn unsaved(&self, _: &str) -> Unsaved {
        self.unsaved
    }
}

pub(crate) fn temp_dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("jc-ui-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub(crate) fn first_of(ws: &Workspace, type_id: &str) -> FeatureId {
    ws.document()
        .data()
        .features
        .iter()
        .find(|f| f.type_id == type_id)
        .unwrap()
        .id
}

#[test]
fn a_command_is_found_by_its_reading_and_run() {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .build_ui_state(
            |ui, ws: &mut Workspace| ws.ui(ui),
            Workspace::new(reference_document(), None, reg()),
        );
    h.run();
    // フィレットは何も選ばずに使える(押してから辺を選ぶ)
    let f = h.state().find_commands("ふぃれ");
    assert_eq!(f[0].id, CommandId::Action(jc_cad_edge::FILLET_FACE.into()));
    assert!(f[0].disabled.is_none());
    // セットアップは読みで見つかり、使える
    let n = h.state().document().data().features.len();
    h.state_mut().switch_space(Space::Machining);
    h.run();
    h.get_by_label("コマンド検索").click();
    h.run();
    assert!(h.state().is_command_palette_open());
    h.state_mut().search.text = Some("せっと".into());
    h.run();
    assert!(h.state().is_command_palette_open());
    // 窓の一覧に出る(加工のツールバーのボタンと合わせて 2 つ以上)
    let shown = h.get_all_by_label_contains("セットアップ").count();
    assert!(shown >= 2, "{shown}");
    let c = h.state().find_commands("せっと");
    assert_eq!(
        c[0].id,
        CommandId::Action(jc_cam_commands::ADD_SETUP.into())
    );
    h.state_mut().run_command(c[0].id.clone());
    h.run();
    assert_eq!(
        h.state().document().data().features.len(),
        n + 2,
        "工具とセットアップ"
    );
}

pub(crate) fn reg() -> jc_core::Modules {
    jc_bundle::modules()
}

#[test]
fn the_undo_history_goes_back_and_forth_to_any_change() {
    let mut ws = Workspace::new(reference_document(), None, reg());
    let start = ws.document().undo_labels().len();
    let width = ws.document().data().param_by_name("width").unwrap().id;
    ws.set_param_expr(width, "70 mm");
    ws.set_param_expr(width, "75 mm");
    assert_eq!(
        ws.document().undo_labels()[start..],
        ["width を 70 mm に変更", "width を 75 mm に変更"]
    );
    let vol = |ws: &Workspace| {
        ws.cur
            .work
            .kernel()
            .mass(ws.visible_bodies()[0].1)
            .unwrap()
            .volume
    };
    let width_of = |w: f64| jc_samples::reference_values(w).0;
    // 画面のボタンで窓を開き、行を押すと、その時点へ戻る(やり直しの行も残る)
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h.get_by_label("取り消しの履歴").click();
    h.run();
    assert!(h.state().undo_history_open());
    h.get_by_label(&format!("{}. width を 70 mm に変更", start + 1))
        .click();
    h.run();
    assert!((vol(h.state()) - width_of(70.0)).abs() < 1.0);
    assert_eq!(
        h.state().document().redo_labels(),
        ["width を 75 mm に変更"]
    );
    h.get_by_label(&format!("{}. width を 75 mm に変更", start + 2))
        .click();
    h.run();
    assert!((vol(h.state()) - width_of(75.0)).abs() < 1.0);
    // 何段でも一度に戻れる
    h.get_by_label("0. 最初の状態").click();
    h.run();
    let ws = h.state();
    assert!(!ws.document().can_undo());
    assert_eq!(ws.document().redo_labels().len(), start + 2);
    assert!((vol(ws) - width_of(60.0)).abs() < 1.0);
}

#[test]
fn crash_reports_left_on_the_pc_are_listed_under_help() {
    let d = std::env::temp_dir().join(format!("jc-ui-crash-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let data = d.join("data");
    std::fs::create_dir_all(data.join("crash")).unwrap();
    std::fs::write(
        data.join("crash").join("crash-1.json"),
        r#"{"kind": "panic", "time": 1, "app": "kie-monobench", "version": "0", "message": "試しの落ち方"}"#,
    )
    .unwrap();
    let ws = Workspace::new(Document::new(), None, reg()).with_data_dir(data);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h.get_by_label("ヘルプ").click();
    h.run();
    h.get_by_label("クラッシュ報告…").click();
    h.run();
    h.get_by_label("試しの落ち方");
    h.get_by_label_contains("内部の誤り");
    // 開発者へファイルで渡す手掛かり(フォルダを開くは OS の窓が出るので押さない)
    h.get_by_label("報告の入ったフォルダを開く");
    h.get_by_label("写す").click();
    h.run();
    assert_eq!(
        h.state().message(),
        Some("報告 crash-1.json をクリップボードへ写しました")
    );
    h.get_by_label("見る").click();
    h.run();
    assert_eq!(
        h.state().cur.viewer.as_ref().map(|v| v.0.as_str()),
        Some("crash-1.json")
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// 試験を truck のカーネルで動かしているか(`JC_KERNEL=truck`。既定は SMLib)。
pub(crate) fn on_truck() -> bool {
    std::env::var("JC_KERNEL").is_ok_and(|v| v.trim() == "truck")
}

#[test]
fn undoing_back_to_the_saved_state_clears_the_modified_mark() {
    // 保存した所まで取り消したら「変更あり」が消える(残ると、変えていないのに保存を聞かれる)
    let part = jc_samples::reference_part();
    let holes = part.holes;
    let mut ws = Workspace::new(part.doc, None, reg());
    assert!(!ws.is_modified());
    let mut d = ws.document().data().feature(holes).unwrap().data.clone();
    d["diameter"] = serde_json::json!("9 mm");
    assert!(ws.set_feature_data(holes, d));
    assert!(ws.is_modified());
    ws.undo();
    assert!(!ws.is_modified(), "保存した時と同じ中身に戻った");
    ws.redo();
    assert!(ws.is_modified());
    // 作って、キャンセルで作る前へ戻しても残らない
    ws.undo();
    ws.add_described(jc_cad_hole::HOLE).expect("足せる");
    assert!(ws.is_modified());
    ws.cancel_creation();
    assert!(!ws.is_modified());
    assert!(!ws.document().can_redo(), "やめたものはやり直しに残らない");
}

#[test]
fn escape_on_an_open_list_closes_only_the_list() {
    // Z2(網羅の調べ J16・J50): 作る途中で欄の一覧を Esc で閉じると、作りかけのセットアップごと消えていた
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(
            |ui, ws: &mut Workspace| ws.ui(ui),
            Workspace::new(reference_document(), None, reg()),
        );
    h.run();
    let n = h.state().document().data().features.len();
    h.state_mut()
        .run_action(jc_cam_commands::ADD_SETUP)
        .expect("足せる");
    h.run();
    let setup = h.state().creating().expect("作っている途中");
    {
        use egui_kittest::kittest::NodeT;
        // 欄「原点(XY)」の一覧(今の値が「手前左」)を開く。欄が多いので見える所へ送ってから
        for click in [false, true] {
            let n = h
                .query_all(egui_kittest::kittest::by().role(egui::accesskit::Role::ComboBox))
                .find(|n| n.accesskit_node().value().as_deref() == Some("手前左"))
                .expect("原点(XY)の一覧");
            if click {
                n.click();
            } else {
                n.scroll_to_me();
            }
            h.run();
        }
    }
    h.run();
    assert!(egui::Popup::is_any_open(&h.ctx), "一覧が開いた");
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(!egui::Popup::is_any_open(&h.ctx), "一覧は閉じる");
    assert_eq!(h.state().creating(), Some(setup), "作る途中のまま");
    assert!(h.state().document().data().features.len() > n);
    // 一覧が閉じていれば、Esc で作るのをやめる
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(h.state().creating(), None);
}

/// 加工の種類 `type_id` を足す操作の ID(戦略の名乗りから)。
pub(crate) fn add_op(type_id: &str) -> &'static str {
    jc_bundle::strategies()
        .iter()
        .find(|s| s.type_id == type_id)
        .expect("戦略がある")
        .action
}

/// 画面の操作も CLI・MCP と同じ jc-session の道を通る(段 2b-4)。今使えない操作は、ボタンを
/// 押せなくするだけでなく実行も断り、文書を変えない(前は画面だけが断っていなかった。D14)。
#[test]
fn an_unavailable_action_is_refused_on_screen_too() {
    let mut ws = Workspace::new(Document::new(), None, reg());
    ws.wait();
    let Some(a) = ws.actions().into_iter().find(|a| a.disabled.is_some()) else {
        panic!("空の文書で使えない操作がある");
    };
    let before = ws.document().data().clone();
    let undo = ws.document().undo_depth();
    assert_eq!(ws.run_action_args(&a.id, serde_json::Value::Null), None);
    let m = ws.message().unwrap_or_default();
    assert!(m.contains("今使えません"), "{}: {m}", a.id);
    assert_eq!(ws.document().data(), &before);
    assert_eq!(ws.document().undo_depth(), undo);
}

#[test]
fn a_quick_recompute_shows_no_busy_badge() {
    // 10-22 u11: すぐ終わる計算で「再計算中」の札がちらつかないよう、少し経ってから出す
    use crate::frame_ui::{BUSY_BADGE_DELAY, busy_badge};
    use std::time::Duration;
    assert_eq!(busy_badge(false, None), (false, None));
    let (show, wait) = busy_badge(true, Some(Duration::from_millis(100)));
    assert!(!show);
    assert_eq!(wait, Some(BUSY_BADGE_DELAY - Duration::from_millis(100)));
    assert_eq!(busy_badge(true, Some(BUSY_BADGE_DELAY)), (true, None));
    assert_eq!(busy_badge(true, Some(Duration::from_secs(3))), (true, None));
    // 走り始めの時刻が無くても、走っていれば出す
    assert_eq!(busy_badge(true, None), (true, None));
}
