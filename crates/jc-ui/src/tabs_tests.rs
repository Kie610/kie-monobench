//! 文書のタブ(候補 20)の試験: 別のタブに開く、タブごとの取り消しと形、未保存の確かめ。

use super::*;
use jc_core::Document;
use jc_samples::{reference_document, sample_document};

fn reg() -> jc_core::Modules {
    jc_bundle::modules()
}

struct Answer(Unsaved);

impl Dialogs for Answer {
    fn open_document(&self) -> Option<PathBuf> {
        None
    }
    fn save_document(&self, _: &str) -> Option<PathBuf> {
        None
    }
    fn open_file(&self, _: &str, _: &str, _: &[&str]) -> Option<PathBuf> {
        None
    }
    fn save_file(&self, _: &str, _: &str, _: &[&str], _: &str) -> Option<PathBuf> {
        None
    }
    fn unsaved(&self, _: &str) -> Unsaved {
        self.0
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("jc-ui-tabs-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 見えているボディの体積の和(形がカーネルに残っているか)。
fn volume(ws: &Workspace) -> f64 {
    ws.visible_bodies()
        .iter()
        .map(|(_, h)| ws.cur.work.kernel().volume(*h).expect("形がカーネルにある"))
        .sum()
}

#[test]
fn files_open_in_their_own_tabs_and_keep_their_own_undo_and_shapes() {
    let d = temp_dir("open");
    let (a, b) = (d.join("天板.json"), d.join("脚.json"));
    jc_session::save(&a, &reference_document()).unwrap();
    jc_session::save(&b, &sample_document()).unwrap();
    let mut ws = Workspace::new(Document::new(), None, reg())
        .with_kernel_factory(Arc::new(jc_bundle::kernel))
        .with_data_dir(d.join("data"));
    // 無題の空の文書のタブは使い回す
    assert!(ws.open_in_tab(&a));
    assert_eq!(ws.tab_count(), 1);
    let va = volume(&ws);
    assert!(va > 0.0);
    ws.add_param("extra", "1 mm");
    assert!(ws.is_modified());
    // 次のファイルは新しいタブに開き、前の文書はそのまま残る
    assert!(ws.open_in_tab(&b));
    assert_eq!(ws.tab_count(), 2);
    assert_eq!(ws.active_tab(), 1);
    assert_eq!(
        ws.tab_titles(),
        vec![("天板".to_string(), true), ("脚".to_string(), false)]
    );
    assert!(!ws.document().can_undo(), "取り消しの履歴はタブごと");
    let vb = volume(&ws);
    // 戻ると、前の文書・未保存の印・取り消し・形が残っている(カーネルはタブごと)
    ws.switch_tab(0);
    assert_eq!(ws.path(), Some(a.as_path()));
    assert!(ws.is_modified());
    assert!(ws.document().can_undo());
    assert!((volume(&ws) - va).abs() < 1e-6);
    ws.undo();
    assert!(!ws.is_modified());
    // 開いているファイルを開くと、そのタブへ移るだけ
    assert!(ws.open_in_tab(&b));
    assert_eq!((ws.tab_count(), ws.active_tab()), (2, 1));
    assert!((volume(&ws) - vb).abs() < 1e-6);
    // 次・前は端で回る
    ws.cycle_tab(1);
    assert_eq!(ws.active_tab(), 0);
    ws.cycle_tab(-1);
    assert_eq!(ws.active_tab(), 1);
    // 開けないファイルなら、作ったタブを閉じて元のタブに戻る
    assert!(!ws.open_in_tab(&d.join("無い.json")));
    assert_eq!((ws.tab_count(), ws.active_tab()), (2, 1));
    let _ = std::fs::remove_dir_all(&d);
}

/// 裏のタブの未保存の変更も自動保存する。手前のタブだけを見ると、変更して
/// から別のタブへ移った時に、落ちるとその変更が消える。
#[test]
fn autosave_writes_every_unsaved_tab_not_only_the_front_one() {
    let d = temp_dir("autosave");
    let (a, b) = (d.join("天板.json"), d.join("脚.json"));
    jc_session::save(&a, &reference_document()).unwrap();
    jc_session::save(&b, &sample_document()).unwrap();
    let mut ws = Workspace::new(Document::new(), None, reg())
        .with_kernel_factory(Arc::new(jc_bundle::kernel))
        .with_data_dir(d.join("data"));
    ws.win.layout.autosave_minutes = 10;
    assert!(ws.open_in_tab(&a));
    ws.add_param("extra", "1 mm");
    // 変更した文書を裏に回し、手前のタブは変更しない
    assert!(ws.open_in_tab(&b));
    let now = std::time::SystemTime::now();
    let later = |m: u64| now + std::time::Duration::from_secs(m * 60);
    let written = || jc_session::recoveries(&d.join("data"));
    ws.autosave_tick(now);
    ws.autosave_tick(later(9));
    assert!(written().is_empty(), "10 分たつまで書かない");
    ws.autosave_tick(later(10));
    let r = written();
    assert_eq!(r.len(), 1, "裏のタブの写しを書く: {r:?}");
    assert_eq!(r[0].original.as_deref(), Some(a.as_path()));
    // 手前のタブも変更すれば、タブごとに別の写しになる
    ws.add_param("extra2", "2 mm");
    ws.autosave_tick(later(11));
    ws.autosave_tick(later(21));
    assert_eq!(written().len(), 2);
    // 裏のタブを保存すれば、その写しだけを消す
    ws.switch_tab(0);
    ws.save();
    let r = written();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].original.as_deref(), Some(b.as_path()));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn tabs_that_share_one_kernel_are_computed_again_when_shown() {
    // カーネルの作り方を渡さないと分け合う。戻ったタブは一から計算し直すので、形は消えない
    let mut ws = Workspace::new(reference_document(), None, reg());
    let va = volume(&ws);
    ws.new_tab();
    ws.replace_document(reference_document(), None);
    assert!(volume(&ws) > 0.0);
    ws.switch_tab(0);
    assert!((volume(&ws) - va).abs() < 1e-6);
}

#[test]
fn closing_an_unsaved_tab_asks_and_the_last_tab_goes_back_to_the_start_screen() {
    let mut ws = Workspace::new(sample_document(), None, reg())
        .with_dialogs(Box::new(Answer(Unsaved::Cancel)));
    ws.add_param("extra", "1 mm");
    // 新しい文書は別のタブに開く(前の文書は残る)
    ws.new_document();
    assert_eq!((ws.tab_count(), ws.active_tab()), (2, 1));
    assert!(ws.document().data().features.is_empty());
    // 空のタブで新しい文書を作っても増やさない
    ws.new_document();
    assert_eq!(ws.tab_count(), 2);
    // 窓を閉じる時は未保存のタブを開いて聞く。やめたら閉じない
    assert!(!ws.confirm_close());
    assert_eq!(ws.active_tab(), 0, "聞いているタブを出す");
    // 閉じるのをやめたら残る
    assert!(!ws.close_tab(0));
    assert_eq!(ws.tab_count(), 2);
    ws = ws.with_dialogs(Box::new(Answer(Unsaved::Discard)));
    assert!(ws.close_tab(0));
    assert_eq!(ws.tab_count(), 1);
    assert!(ws.document().data().features.is_empty());
    assert!(ws.confirm_close());
    // 最後のタブを閉じると、無題の文書と開始画面
    ws.add_param("extra", "1 mm");
    assert!(ws.close_tab(0));
    assert_eq!(ws.tab_count(), 1);
    assert!(ws.is_start_screen());
    assert!(!ws.is_modified());
}

/// 時間のかかる操作(解析など)は、それを始めた文書のもの。別のタブで新しい文書を作っても止まらず、
/// その文書のタブを閉じた時に止まる(前は止め口が窓全体にあり、A で解析中に Ctrl+N を押すと
/// A の解析が止まった)。
#[test]
fn a_long_task_belongs_to_its_document_and_stops_only_when_that_tab_closes() {
    let mut ws = Workspace::new(sample_document(), None, reg())
        .with_dialogs(Box::new(Answer(Unsaved::Discard)));
    let ctl = Arc::new(jc_core::JobControl::new());
    ws.cur.task_ctl = Some(ctl.clone());
    ws.new_document();
    assert_eq!(ws.tab_count(), 2);
    assert!(!ctl.cancelled(), "別のタブの新しい文書で止まらない");
    assert!(ws.cur.task_ctl.is_none(), "新しい文書には仕事が無い");
    ws.switch_tab(0);
    assert!(ws.cur.task_ctl.is_some(), "戻ると仕事の進みが見える");
    ws.switch_tab(1);
    assert!(ws.close_tab(0));
    assert!(ctl.cancelled(), "始めた文書のタブを閉じたら止まる");
}

/// 最後のタブを閉じると、文書ごとの状態を丸ごと作り直す(前はカメラ・アクティブな部品・部品の
/// 番号などが次の無題の文書に残った)。
#[test]
fn closing_the_last_tab_starts_the_next_document_from_a_clean_state() {
    let mut ws = Workspace::new(sample_document(), None, reg())
        .with_dialogs(Box::new(Answer(Unsaved::Discard)));
    ws.cur.camera.set_view(10.0, 20.0);
    let c = ws.new_component(serde_json::json!({ "selection": [], "name": "前の部品" }));
    ws.activate_component(c);
    assert!(ws.active_component().is_some());
    ws.cur.collapsed.insert(3);
    ws.cur.props_tags = "前の文書".into();
    let ctl = Arc::new(jc_core::JobControl::new());
    ws.cur.task_ctl = Some(ctl.clone());
    assert!(ws.close_tab(0));
    assert!(ws.is_start_screen());
    let fresh = view3d::Camera::default();
    assert_eq!(
        (ws.cur.camera.yaw, ws.cur.camera.pitch),
        (fresh.yaw, fresh.pitch)
    );
    assert_eq!(ws.active_component(), None);
    assert!(ws.cur.collapsed.is_empty());
    assert!(ws.cur.props_tags.is_empty());
    assert!(ctl.cancelled(), "閉じた文書の仕事は止める");
}

#[test]
fn tabs_are_reordered_and_the_front_tab_stays_in_front() {
    // タブを引いて並べ替えても、今のタブの文書はそのまま
    let mut ws = Workspace::new(sample_document(), None, reg());
    ws.new_tab();
    ws.replace_document(reference_document(), None);
    let middle = ws.document().data().clone();
    ws.new_tab();
    let names = |ws: &Workspace| ws.win.doc_tabs.iter().map(|t| t.uid).collect::<Vec<_>>();
    let before = names(&ws);
    assert_eq!(ws.active_tab(), 2);
    let front = ws.document().data().clone();
    // 今のタブ(右端)を左端へ
    ws.move_tab(2, 0);
    assert_eq!(names(&ws), vec![before[2], before[0], before[1]]);
    assert_eq!(ws.active_tab(), 0);
    assert_eq!(*ws.document().data(), front);
    // しまったタブを動かしても今のタブは替わらない
    ws.move_tab(1, 2);
    assert_eq!(names(&ws), vec![before[2], before[1], before[0]]);
    assert_eq!(ws.active_tab(), 0);
    // 外れた番号は何もしない
    ws.move_tab(5, 0);
    assert_eq!(names(&ws), vec![before[2], before[1], before[0]]);
    // 動かした後も、切り替えれば元の文書が出る
    ws.switch_tab(1);
    assert_eq!(*ws.document().data(), middle);
}

#[test]
fn closing_other_tabs_keeps_the_chosen_one() {
    let mut ws = Workspace::new(sample_document(), None, reg())
        .with_dialogs(Box::new(Answer(Unsaved::Discard)));
    ws.new_tab();
    ws.replace_document(reference_document(), None);
    ws.new_tab();
    ws.replace_document(sample_document(), None);
    assert_eq!(ws.tab_count(), 3);
    let keep = ws.document().data().clone();
    ws.close_other_tabs(2);
    assert_eq!((ws.tab_count(), ws.active_tab()), (1, 0));
    assert_eq!(*ws.document().data(), keep);
}

#[test]
fn saved_versions_open_in_a_tab_and_come_back_as_the_latest() {
    let d = temp_dir("versions");
    let file = d.join("板.json");
    jc_session::save(&file, &Document::new()).unwrap();
    let mut ws = Workspace::new(Document::new(), None, reg())
        .with_dialogs(Box::new(Answer(Unsaved::Cancel)))
        .with_data_dir(d.join("data"));
    assert!(ws.open_in_tab(&file));
    ws.replace_document(reference_document(), Some(file.clone()));
    ws.save();
    ws.add_param("extra", "1 mm");
    ws.save();
    let v = ws.versions();
    assert_eq!(
        v.iter().map(|v| v.number).collect::<Vec<_>>(),
        vec![3, 2, 1]
    );
    assert!(v[0].thumbnail, "形のある版は縮小図つき");
    assert!(!v[2].thumbnail, "形の無い版は絵なし");
    // 版 2 を別のタブに写しとして開く(元のファイルとはつながない)
    assert!(ws.open_version(&v[1].id));
    assert_eq!(ws.tab_count(), 2);
    assert_eq!(ws.path(), None);
    assert_eq!(ws.tab_titles()[1].0, "板(版 2)");
    assert!(
        !ws.document()
            .data()
            .params
            .iter()
            .any(|p| p.name == "extra")
    );
    ws.switch_tab(0);
    // 節目の名前
    assert!(ws.name_version(&v[1].id, "穴あき"));
    assert_eq!(ws.versions()[1].milestone, "穴あき");
    // 未保存の変更があれば聞く。やめたら戻さない
    ws.add_param("more", "2 mm");
    assert!(!ws.restore_version(&v[1].id));
    ws.undo();
    assert!(ws.restore_version(&v[1].id));
    assert!(!ws.is_modified(), "戻したら保存する");
    let after = ws.versions();
    assert_eq!(after[0].number, 4);
    assert_eq!(after[0].note, "版 2 に戻した");
    assert!(
        !ws.document()
            .data()
            .params
            .iter()
            .any(|p| p.name == "extra")
    );
    let saved = jc_session::load(&file).unwrap();
    assert_eq!(saved.data(), ws.document().data());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_saved_version_is_compared_with_the_document_as_a_table_and_an_overlaid_shape() {
    let d = temp_dir("compare");
    let file = d.join("台.json");
    jc_session::save(&file, &reference_document()).unwrap();
    let mut ws = Workspace::new(Document::new(), None, reg())
        .with_kernel_factory(Arc::new(jc_bundle::kernel))
        .with_data_dir(d.join("data"));
    assert!(ws.open_in_tab(&file));
    // 版 1 は読み込んだ形のまま。今の文書でパラメータを足し、機能の名前とデータを変える
    let v1 = ws.versions()[0].id.clone();
    ws.add_param("extra", "1 mm");
    let f = ws.document().data().features[1].clone();
    assert!(ws.rename_feature(f.id, "変えた名前"));
    let mut data = f.data.clone();
    data["note"] = serde_json::json!("足した");
    assert!(ws.set_feature_data(f.id, data));
    assert!(ws.compare_version(&v1));
    let ch = ws.compared_changes();
    let row = |what: &str, name: &str| {
        ch.iter()
            .find(|c| c.what == what && c.name == name)
            .unwrap_or_else(|| panic!("{what} {name} が無い: {ch:?}"))
            .clone()
    };
    let p = row("パラメータ", "extra");
    assert_eq!((p.before.as_str(), p.after.as_str()), ("", "1 mm"));
    let fe = row("機能", "変えた名前");
    assert!(fe.before.contains(&format!("名前: {}", f.name)), "{fe:?}");
    assert!(
        fe.after.contains("名前: 変えた名前") && fe.after.contains("note: 足した"),
        "{fe:?}"
    );
    // 版の形は別のカーネルで計算し、今の文書の形は消えない
    let va = volume(&ws);
    let c = ws.cur.versions.compare.as_mut().unwrap();
    c.wait();
    assert!(c.shape_triangles() > 0);
    assert_eq!(ws.compare_ghosts().len(), 1);
    assert!((volume(&ws) - va).abs() < 1e-6);
    // 重ねるのをやめられる。取り消すと違いは減る
    ws.cur.versions.compare.as_mut().unwrap().overlay = false;
    assert!(ws.compare_ghosts().is_empty());
    ws.undo();
    ws.undo();
    let ch = ws.compared_changes();
    assert_eq!(ch.len(), 1, "{ch:?}");
    // 見比べはタブごと
    ws.new_tab();
    assert!(ws.cur.versions.compare.is_none());
    ws.switch_tab(0);
    assert!(ws.cur.versions.compare.is_some());
    let _ = std::fs::remove_dir_all(&d);
}

/// 画面の側を回しながら、別のスレッドから MCP と同じ要求を送る。
fn ask(
    ws: &mut Workspace,
    dir: &std::path::Path,
    tool: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let (d, t) = (dir.to_path_buf(), tool.to_string());
    let h = std::thread::spawn(move || jc_session::live::call(&d, &t, args));
    while !h.is_finished() {
        ws.live_tick();
        ws.wait();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    h.join().unwrap()
}

#[test]
fn claude_reads_and_changes_the_open_document_through_the_live_link() {
    let d = temp_dir("live");
    let data = d.join("data");
    let mut ws = Workspace::new(reference_document(), None, reg()).with_data_dir(data.clone());
    // 既定は待ち受けない
    ws.live_tick();
    assert_eq!(ws.live_state(), LiveState::Off);
    assert!(jc_session::live::read_ticket(&data).is_none());
    ws.win.layout.ai_live = true;
    ws.live_tick();
    assert_eq!(ws.live_state(), LiveState::Listening);
    // 読む: 文書の要約・選んでいる物・画面の向きの絵
    let s = ask(&mut ws, &data, "status", serde_json::json!({})).unwrap();
    assert_eq!(s["params"][0]["name"], "width", "{s}");
    // 答えている間の要求は、動かしている一覧に出る(接続をまたいで 1 つの一覧。m3)
    assert_eq!(s["running"][0]["tool"], "status", "{s}");
    assert_eq!(ws.live_state(), LiveState::Connected);
    let shot = ask(
        &mut ws,
        &data,
        "screenshot",
        serde_json::json!({"width": 200, "height": 150}),
    )
    .unwrap();
    assert!(
        shot["png"].as_str().unwrap().starts_with("iVBOR"),
        "PNG の頭"
    );
    assert_eq!(shot["width"], 200);
    // 変える: 人と同じ取り消しの履歴に「Claude: 」付きで入り、再計算の後の要約で答える
    let s = ask(
        &mut ws,
        &data,
        "set_param",
        serde_json::json!({"name": "width", "expr": "70 mm"}),
    )
    .unwrap();
    assert_eq!(s["params"][0]["expr"], "70 mm");
    assert_eq!(s["errors"], 0, "{s}");
    assert_eq!(
        ws.document().undo_labels().last().copied(),
        Some("Claude: width を 70 mm に")
    );
    assert_eq!(ws.live_state(), LiveState::Writing);
    ask(
        &mut ws,
        &data,
        "add_param",
        serde_json::json!({"name": "gap", "expr": "0.2 mm"}),
    )
    .unwrap();
    assert!(
        ask(
            &mut ws,
            &data,
            "add_param",
            serde_json::json!({"name": "gap", "expr": "1 mm"})
        )
        .is_err()
    );
    ask(&mut ws, &data, "undo", serde_json::json!({})).unwrap();
    assert!(!ws.document().data().params.iter().any(|p| p.name == "gap"));
    // 知らない道具・足りない引数は理由つきで断る
    assert!(
        ask(&mut ws, &data, "explode", serde_json::json!({}))
            .unwrap_err()
            .contains("知りません")
    );
    assert!(
        ask(
            &mut ws,
            &data,
            "set_param",
            serde_json::json!({"name": "width"})
        )
        .unwrap_err()
        .contains("expr")
    );
    // 人が機能を編集している間は変えない(読むのは通す)
    ws.cur.editing = Some(ws.document().data().features[0].id);
    let e = ask(
        &mut ws,
        &data,
        "set_param",
        serde_json::json!({"name": "width", "expr": "80 mm"}),
    )
    .unwrap_err();
    assert!(e.contains("編集している途中"), "{e}");
    assert!(ask(&mut ws, &data, "status", serde_json::json!({})).is_ok());
    ws.cur.editing = None;
    // 切ると live.json を消し、要求は届かない
    ws.live_disconnect();
    assert_eq!(ws.live_state(), LiveState::Off);
    assert!(!ws.win.layout.ai_live);
    assert!(jc_session::live::read_ticket(&data).is_none());
    assert!(jc_session::live::call(&data, "status", serde_json::json!({})).is_err());
    let _ = std::fs::remove_dir_all(&d);
}

/// AI が文書を変えると、再計算の後に変えた機能とそのボディを光らせ、下の帯に道具の名前を出す。
/// 読むだけの要求と、文書の変わらない要求では光らせない。
#[test]
fn claude_changes_light_up_the_changed_features_and_bodies() {
    let d = temp_dir("live_mark");
    let data = d.join("data");
    let mut ws = Workspace::new(reference_document(), None, reg()).with_data_dir(data.clone());
    ws.win.layout.ai_live = true;
    ws.live_tick();
    let ctx = egui::Context::default();
    ask(&mut ws, &data, "status", serde_json::json!({})).unwrap();
    assert!(ws.live.mark.is_none(), "読むだけでは光らせない");
    ask(
        &mut ws,
        &data,
        "set_param",
        serde_json::json!({"name": "width", "expr": "70 mm"}),
    )
    .unwrap();
    let m = ws.live.mark.as_ref().expect("変えたので光らせる");
    // パラメータだけ変えた時は、結果の変わった機能(幅を使う機能)を光らせる
    assert!(!m.features.is_empty());
    assert!(!m.bodies.is_empty(), "幅を変えたボディ");
    let (lit, at) = (*m.features.iter().next().unwrap(), m.at);
    assert!(ws.ai_glow(&ctx, lit, None).is_some());
    ws.sync_ai_glow(&ctx);
    let (bodies, _, step) = ws.cur.scene.glow().expect("3D のビューでも光らせる");
    assert!(*step > 0 && bodies == &ws.live.mark.as_ref().unwrap().bodies);
    let i = ws
        .cur
        .scene
        .bodies
        .iter()
        .position(|b| bodies.contains(&b.feature))
        .unwrap();
    assert!(ws.cur.scene.glows(i));
    assert_eq!(
        ws.live.wrote_by.as_deref(),
        Some("set_param(width = 70 mm)")
    );
    // 読むだけの要求は印を替えない
    ask(&mut ws, &data, "status", serde_json::json!({})).unwrap();
    assert_eq!(ws.live.mark.as_ref().unwrap().at, at);
    // しばらくすると消える
    ws.live.mark.as_mut().unwrap().at -= std::time::Duration::from_secs(3);
    assert!(ws.ai_glow(&ctx, lit, None).is_none());
    ws.sync_ai_glow(&ctx);
    assert!(ws.cur.scene.glow().is_none());
    // 取り消しも AI の変更なので光らせる
    ask(&mut ws, &data, "undo", serde_json::json!({})).unwrap();
    assert!(ws.live.mark.as_ref().unwrap().at > at);
    let _ = std::fs::remove_dir_all(&d);
}

/// 画面の状態は「文書ごと」([`DocState`])・「窓全体」([`WinState`])・「部品ごと」(ドック・コマンド
/// 検索・設定の窓など、部品の struct)のどれかに置き、作業場そのものには欄を足さない。前は作業場に
/// 171 欄が平らに並び、文書ごとの欄を 1 つずつ入れ替えていたため、一覧に載せ忘れた欄(ストーリー
/// ボード・表の窓・解析の仕事の止め口など)が、タブを替えても前の文書のまま残った(構造のレビュー C1)。
///
/// [`DocState`]: crate::doc_state::DocState
/// [`WinState`]: crate::window_state::WinState
#[test]
fn every_workspace_field_is_either_per_document_or_per_window() {
    /// 作業場が持つ物: 文書ごと・窓全体の状態と、部品ごとの状態。
    const PARTS: &[&str] = &[
        "cur",
        "win",
        "docking",
        "search",
        "settings",
        "browser",
        "view_menu",
        "home",
        "live",
        "cmdline",
        "thumbs",
    ];
    let fields = struct_fields(include_str!("lib.rs"), "pub struct Workspace");
    assert_eq!(
        fields, PARTS,
        "作業場に欄を足さない。文書ごとなら doc_state.rs の DocState、窓全体なら window_state.rs の \
         WinState、部品の物ならその部品の struct に置く"
    );
    let per_doc = struct_fields(include_str!("doc_state.rs"), "pub(crate) struct DocState");
    let per_window = struct_fields(
        include_str!("window_state.rs"),
        "pub(crate) struct WinState",
    );
    for w in &per_window {
        assert!(
            !per_doc.contains(w),
            "{w} が DocState と WinState の両方にある"
        );
    }
    // 解析などの時間のかかる操作の止め口は文書ごと(前は窓全体で、A の解析中に Ctrl+N を押すと止まった)
    assert!(per_doc.iter().any(|f| f == "task_ctl"));
    // 画面の上の位置を持つ途中の状態は文書ごとに置かない(タブを替えると閉じる)
    for f in ["candidates", "context_others", "drop_rows"] {
        assert!(!per_doc.iter().any(|d| d == f), "{f} が DocState にある");
    }
}

/// `src` の `head { ... }` の struct の欄の名前(上の階だけ)。
fn struct_fields(src: &str, head: &str) -> Vec<String> {
    let start = src.find(head).expect("struct がある");
    let body = &src[start..];
    let open = body.find('{').unwrap();
    let mut depth = 0;
    let mut end = open;
    for (i, c) in body[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = open + i;
                    break;
                }
            }
            _ => {}
        }
    }
    body[open + 1..end]
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with("//") && !l.starts_with('#'))
        .filter_map(|l| {
            let l = l
                .strip_prefix("pub(crate) ")
                .or(l.strip_prefix("pub "))
                .unwrap_or(l);
            let (name, _) = l.split_once(':')?;
            (!name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'))
            .then(|| name.to_string())
        })
        .collect()
}

#[test]
fn screen_memory_of_a_document_stays_with_its_tab() {
    // 同じ見本を 2 つのタブに開く(機能と変数の番号が同じ)。片方の文書の画面の記憶(通らなかった
    // 入力の理由など)は、もう片方のタブへ移らず、戻ると残っている(段 2)
    let mut ws = Workspace::new(reference_document(), None, reg());
    ws.new_tab();
    ws.replace_document(reference_document(), None);
    ws.switch_tab(0);
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run_steps(2);
    let first = crate::doc_id(&h.ctx, ("param_failed", 1));
    h.ctx
        .data_mut(|d| d.insert_temp(first, "式が読めません".to_string()));
    h.state_mut().switch_tab(1);
    h.run_steps(2);
    let second = crate::doc_id(&h.ctx, ("param_failed", 1));
    assert_ne!(first, second, "タブを替えると別の鍵");
    assert_eq!(h.ctx.data(|d| d.get_temp::<String>(second)), None);
    h.state_mut().switch_tab(0);
    h.run_steps(2);
    assert_eq!(crate::doc_id(&h.ctx, ("param_failed", 1)), first);
    assert_eq!(
        h.ctx.data(|d| d.get_temp::<String>(first)).as_deref(),
        Some("式が読めません")
    );
}

/// ホームで取り消す(Ctrl+Z)と、文書でなく直前の仕分けを戻す。開いているデザインを移した時は
/// タブの保存先も移した先へ付け替え、戻すと元へ戻る(保存先が jc-session の Headless に移った後も)。
#[test]
fn undo_on_home_puts_back_the_last_move_and_the_open_path() {
    let dir = std::env::temp_dir().join(format!("jc-ui-home-undo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (a, b) = (dir.join("A"), dir.join("B"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let file = a.join("台.json");
    jc_session::save(&file, &Document::new()).unwrap();
    let mut ws = Workspace::new(Document::new(), Some(file.clone()), reg());
    ws.add_param("extra", "1 mm");
    ws.show_home();
    ws.home_request(HomeRequest::Move {
        designs: vec![file.clone()],
        to: b.clone(),
    });
    let moved = b.join("台.json");
    assert!(moved.exists() && !file.exists());
    assert_eq!(
        ws.cur.work.path(),
        Some(moved.as_path()),
        "開いているタブも付け替える"
    );
    ws.undo();
    assert!(
        file.exists() && !moved.exists(),
        "ホームの取り消しは仕分けを戻す"
    );
    assert_eq!(ws.cur.work.path(), Some(file.as_path()));
    assert!(
        ws.cur.work.doc().data().param_by_name("extra").is_some(),
        "文書の変更は取り消さない"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
