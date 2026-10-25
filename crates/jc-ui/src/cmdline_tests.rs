//! コマンド欄の試験(画面の部品を名前で探し、人と同じ入力で操作する)。

use super::*;
use crate::cmdline::{Kind, ScriptResult};
use egui_kittest::Harness;
use jc_samples::reference_document;

fn harness(ws: Workspace) -> Harness<'static, Workspace> {
    Harness::builder()
        .with_size(egui::vec2(1600.0, 900.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws)
}

/// スクリプトを流し終えるまで画面を回す。
pub(crate) fn run(h: &mut Harness<'static, Workspace>, script: &str) -> ScriptResult {
    let b = h.state_mut().run_script(script).expect("読める");
    let t0 = std::time::Instant::now();
    loop {
        h.step();
        if let Some(r) = h.state_mut().script_result(b) {
            return r;
        }
        assert!(
            t0.elapsed().as_secs() < 180,
            "終わらない:\n{}",
            h.state()
                .command_log()
                .iter()
                .map(|(_, t)| t.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        );
        if h.state().is_busy() {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
}

fn part() -> Harness<'static, Workspace> {
    harness(Workspace::new(
        reference_document(),
        None,
        jc_bundle::modules(),
    ))
}

/// 作業空間の一覧を人と同じに押して替えると、解析のボタンが出る。
#[test]
fn a_script_presses_buttons_by_their_names_like_a_person() {
    let mut h = part();
    let r = run(
        &mut h,
        r#"
        click ~作業空間:
        click "解析"
        expect "静的応力" enabled
        expect "熱" exists
        "#,
    );
    assert!(r.ok, "{}", r.report());
    assert_eq!(h.state().layout().space, crate::layout::Space::Simulation);
    assert!(
        r.lines[0]
            .1
            .as_ref()
            .unwrap()
            .contains("押した: Button \"作業空間: 設計\"")
    );
}

/// ホームの中身を、描いた回数が決まった数になるまで「読んでいます」とする試験用の頁。
struct SlowHome {
    left: u32,
}

impl crate::HomePage for SlowHome {
    fn ui(&mut self, ui: &mut egui::Ui, _host: &mut crate::HomeHost<'_>) {
        self.left = self.left.saturating_sub(1);
        if self.left == 0 {
            ui.label("読めた一覧");
        }
    }

    fn loading(&self, _pass: u64) -> bool {
        self.left > 0
    }
}

/// 確かめること: ホームがフォルダを裏で読み直している間は、台本は次の確かめを待つ(読み直す前の
/// 一覧で確かめて、その時だけ通ったり落ちたりしない)。
#[test]
fn a_script_waits_while_the_home_reads_folders_again() {
    let mut ws = Workspace::new(reference_document(), None, jc_bundle::modules())
        .with_home(SlowHome { left: 30 });
    ws.show_home();
    let mut h = harness(ws);
    let r = run(&mut h, "expect \"読めた一覧\" exists");
    assert!(r.ok, "{}", r.report());
}

/// 失敗した行の後は流さず、理由に近い名前を挙げる。
#[test]
fn a_failed_line_stops_the_rest_of_the_script() {
    let mut h = part();
    let r = run(&mut h, "click \"作業空間: 説計\"\necho ここは流さない");
    assert!(!r.ok);
    assert_eq!(r.lines.len(), 1, "{}", r.report());
    let e = r.lines[0].1.as_ref().unwrap_err();
    assert!(
        e.contains("見つかりません") && e.contains("作業空間: 設計"),
        "{e}"
    );
    // 読めない行があれば何も流さない
    assert!(
        h.state_mut()
            .run_script("echo a\nclik b")
            .unwrap_err()
            .contains("2 行目")
    );
}

/// コマンド欄はコマンドで出せ、欄そのものにもコマンドで打てる(打った行が流れる)。
#[test]
fn the_command_box_opens_by_command_and_runs_what_is_typed_into_it() {
    let mut h = part();
    let r = run(
        &mut h,
        r#"
        expect "コマンド" missing
        pane コマンド on
        type "コマンド" "echo こんにちは" enter
        until "こんにちは" 10s
        expect "> echo こんにちは"
        "#,
    );
    assert!(r.ok, "{}", r.report());
    let log = h.state().command_log();
    assert!(
        log.iter()
            .any(|(k, t)| *k == Kind::Input && t == "echo こんにちは"),
        "{log:?}"
    );
    // 欄はまた打てるように、キーボードを持ったまま
    let r = run(&mut h, "expect \"コマンド\" exists");
    assert!(r.ok);
}

/// MCP の app の画面の道具は、コマンド欄のスクリプトとして流れ、行ごとの結果で答える。
#[test]
fn live_ui_tools_run_as_scripts_and_answer_per_line() {
    let mut h = part();
    let ans = h
        .state_mut()
        .live_request("ui_click", &serde_json::json!({"target": "作業空間: 設計"}))
        .unwrap();
    let crate::live_ui::Answer::AfterScript(b) = ans else {
        panic!("スクリプトとして流す");
    };
    let t0 = std::time::Instant::now();
    let r = loop {
        h.step();
        if let Some(r) = h.state_mut().script_result(b) {
            break r;
        }
        assert!(t0.elapsed().as_secs() < 60);
    };
    assert!(r.ok, "{}", r.report());
    let j = r.to_json();
    assert_eq!(j["lines"][0]["command"], "click \"作業空間: 設計\"");
    assert!(
        h.state_mut()
            .live_request("ui_type", &serde_json::json!({"target": "x"}))
            .is_err_and(|e| e.contains("text"))
    );
}

/// 道具の一覧(jc_session::live::APP_TOOLS)の名前は、どれも画面の振り分けに届く(知らない道具に
/// ならない)。一覧に無い名前は、一覧を並べた文で断る。
#[test]
fn every_app_tool_in_the_list_reaches_the_screen() {
    let mut h = part();
    for tool in jc_session::live::APP_TOOLS {
        // 引数は空(要る引数が無いと断る道具もある。知らない道具でなければよい)
        let r = h
            .state_mut()
            .live_request(tool, &serde_json::json!({"width": 64, "height": 64}));
        if let Err(e) = r {
            assert!(!e.contains("は知りません"), "{tool}: {e}");
        }
    }
    let e = h
        .state_mut()
        .live_request("no_such_tool", &serde_json::json!({}))
        .err()
        .expect("知らない道具は断る");
    assert!(e.contains(&jc_session::live::APP_TOOLS.join("・")), "{e}");
}

#[test]
#[ignore = "道具: JC_DUMP の台本を流して画面の部品の木を出す"]
fn dump_tree() {
    let mut h = part();
    let r = run(&mut h, &std::env::var("JC_DUMP").unwrap());
    println!("{}", r.report());
}

/// スライダの横の数の欄(DragValue)も、既定の Tab の確定で打った値が入る。入らなければ台本は落ちる。
#[test]
fn typing_into_a_slider_number_field_sets_the_value() {
    let mut h = part();
    let r = run(
        &mut h,
        r#"
        click "設定"
        click "画面"
        type "字の大きさ" 150
        "#,
    );
    eprintln!("{}", r.report());
    assert!(r.ok, "{}", r.report());
    assert_eq!(h.state().win.layout.appearance.text_scale, 1.5);
}

/// `expect "X"#n` は n 番目があること、`expect "X"#n missing` は n 番目が無いことを見る
/// (1 つ目があるだけで決めない)。熱の研究の編集パネルには
/// 「+」が 3 つある(選択セットを足すボタン)。
#[test]
fn expect_with_a_number_looks_at_that_one() {
    let open = "click ~作業空間:\nclick \"解析\"\nclick \"熱\"\n";
    for (check, ok) in [
        ("expect \"+\"#3", true),
        ("expect \"+\"#4", false),
        ("expect \"+\"#4 missing", true),
        ("expect \"+\"#3 missing", false),
    ] {
        let mut h = part();
        let r = run(&mut h, &format!("{open}{check}\n"));
        assert_eq!(r.ok, ok, "{check}\n{}", r.report());
    }
}

/// 動いている画面へ AI が送った台本は、途中で失敗すると頼む前の形へ戻す(AI の道のまとめて実行と
/// 同じ)。keep_on_error なら途中までの変更を残す。
#[test]
fn a_live_script_that_fails_puts_the_document_back() {
    let d = crate::tests::temp_dir("live-script-back");
    let mut ws = Workspace::new(reference_document(), None, jc_bundle::modules())
        .with_data_dir(d.join("data"));
    ws.win.layout.ai_live = true;
    let mut h = harness(ws);
    let width = |h: &Harness<'static, Workspace>| {
        let data = h.state().cur.work.doc().data();
        data.params
            .iter()
            .find(|p| p.name == "width")
            .unwrap()
            .expr
            .clone()
    };
    let before = width(&h);
    let ask = |h: &mut Harness<'static, Workspace>, args: serde_json::Value| {
        let dir = d.join("data");
        let t = std::thread::spawn(move || jc_session::live::call(&dir, "script", args));
        let t0 = std::time::Instant::now();
        while !t.is_finished() {
            h.step();
            std::thread::sleep(std::time::Duration::from_millis(5));
            assert!(t0.elapsed().as_secs() < 60);
        }
        t.join().unwrap().unwrap()
    };
    h.step();
    let lines = "set_param width 77\nexpect \"無い部品\" exists";
    let r = ask(&mut h, serde_json::json!({"text": lines}));
    assert_eq!(r["ok"], false, "{r}");
    assert_eq!(r["rolled_back"], true, "{r}");
    assert_eq!(width(&h), before);
    let r = ask(
        &mut h,
        serde_json::json!({"text": lines, "keep_on_error": true}),
    );
    assert!(r.get("rolled_back").is_none(), "{r}");
    assert_eq!(width(&h), "77");
    let _ = std::fs::remove_dir_all(&d);
}
