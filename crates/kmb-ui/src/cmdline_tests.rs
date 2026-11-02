//! コマンド欄の試験(画面の部品を名前で探し、人と同じ入力で操作する)。

use super::*;
use crate::cmdline::{Kind, ScriptResult};
use egui_kittest::Harness;
use kmb_samples::reference_document;

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
        kmb_bundle::modules(),
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
    let mut ws = Workspace::new(reference_document(), None, kmb_bundle::modules())
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

/// 道具の一覧(kmb_session::live::APP_TOOLS)の名前は、どれも画面の振り分けに届く(知らない道具に
/// ならない)。一覧に無い名前は、一覧を並べた文で断る。
#[test]
fn every_app_tool_in_the_list_reaches_the_screen() {
    let mut h = part();
    for tool in kmb_session::live::APP_TOOLS {
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
    assert!(e.contains(&kmb_session::live::APP_TOOLS.join("・")), "{e}");
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

/// 同じ名前の部品が幾つもある時、番号の無い `click` は黙って最初の物を押さずに落ち、候補を挙げる。
/// 番号を付ければ押せる。
#[test]
fn a_name_that_matches_several_parts_fails_instead_of_pressing_the_first() {
    let open = "click ~作業空間:\nclick \"解析\"\nclick \"熱\"\n";
    let mut h = part();
    let r = run(&mut h, &format!("{open}click \"+\"\n"));
    assert!(!r.ok, "{}", r.report());
    assert!(r.report().contains("3 個あります"), "{}", r.report());
    assert!(r.report().contains("#2:"), "{}", r.report());
    let mut h = part();
    let r = run(&mut h, &format!("{open}click \"+\"#2\n"));
    assert!(r.ok, "{}", r.report());
}

/// 動いている画面へ AI が送った台本は、途中で失敗すると頼む前の形へ戻す(AI の道のまとめて実行と
/// 同じ)。keep_on_error なら途中までの変更を残す。
#[test]
fn a_live_script_that_fails_puts_the_document_back() {
    let d = crate::tests::temp_dir("live-script-back");
    let mut ws = Workspace::new(reference_document(), None, kmb_bundle::modules())
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
        let t = std::thread::spawn(move || kmb_session::live::call(&dir, "script", args));
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

#[path = "cmdline_tests/adopted_tests.rs"]
mod adopted;

fn widget_glow_status(h: &mut Harness<'static, Workspace>) -> serde_json::Value {
    match h
        .state_mut()
        .live_request("status", &serde_json::json!({}))
        .unwrap()
    {
        crate::live_ui::Answer::Now(value) => value,
        _ => panic!("状態はすぐに返す"),
    }
}

fn widget_glow_harness() -> Harness<'static, Workspace> {
    harness(Workspace::new(
        kmb_core::Document::new(),
        None,
        kmb_bundle::modules(),
    ))
}

/// 確かめること: ファイル台本の実クリックを光らせる記録が状態から読める。
#[test]
fn script_widget_glow_records_the_actual_button() {
    let mut h = widget_glow_harness();
    let result = run(&mut h, "click \"作業空間: 設計\"");
    assert!(result.ok, "{}", result.report());
    let status = widget_glow_status(&mut h);
    let marks = status["ui_glow"].as_array().expect("押した部品の光る記録");
    assert!(
        marks
            .iter()
            .any(|m| m["name"] == "作業空間: 設計" && m["source"] == "script"),
        "{status}"
    );
}

/// 確かめること: フォルダの制限を持たないMCP要求でも出どころを保持する。
#[test]
fn mcp_widget_glow_is_independent_of_file_roots() {
    let mut h = widget_glow_harness();
    let crate::live_ui::Answer::AfterScript(batch) = h
        .state_mut()
        .live_request("ui_click", &serde_json::json!({"target": "作業空間: 設計"}))
        .unwrap()
    else {
        panic!("画面のクリックは台本として実行する");
    };
    let started = std::time::Instant::now();
    loop {
        h.step();
        if let Some(result) = h.state_mut().script_result(batch) {
            assert!(result.ok, "{}", result.report());
            break;
        }
        assert!(started.elapsed().as_secs() < 30, "クリックが終わらない");
    }
    let status = widget_glow_status(&mut h);
    let marks = status["ui_glow"]
        .as_array()
        .expect("MCPが押した部品の光る記録");
    assert!(
        marks
            .iter()
            .any(|m| m["name"] == "作業空間: 設計" && m["source"] == "mcp"),
        "{status}"
    );
}

/// 確かめること: 人のキー入力で送信したコマンド欄のクリックは光らせない。
#[test]
fn human_command_widget_glow_does_not_inherit_the_typing_script_origin() {
    let mut h = widget_glow_harness();
    assert!(run(&mut h, "pane コマンド on").ok);
    let field = {
        let nodes = h
            .ctx
            .plugin::<crate::drive::UiDriver>()
            .lock()
            .nodes(h.ctx.pixels_per_point());
        let label = crate::drive::Query::parse("コマンド")
            .one(&nodes, h.ctx.content_rect())
            .unwrap();
        crate::drive::field_for(label, &nodes).unwrap().clone()
    };
    h.input_mut().events.extend(crate::drive::click_at(
        field.rect.center(),
        egui::PointerButton::Primary,
        egui::Modifiers::NONE,
    ));
    h.step();
    h.input_mut()
        .events
        .push(egui::Event::Text("click \"作業空間: 設計\"".into()));
    h.step();
    h.input_mut().events.extend(crate::drive::key_events(
        egui::Key::Enter,
        egui::Modifiers::NONE,
    ));
    let started = std::time::Instant::now();
    loop {
        h.step();
        if !h.state().script_running()
            && h.state()
                .command_log()
                .iter()
                .any(|(kind, s)| *kind == Kind::Input && s == "click \"作業空間: 設計\"")
        {
            break;
        }
        assert!(
            started.elapsed().as_secs() < 10,
            "人のコマンドが終わらない: {:?}",
            h.state().command_log()
        );
    }
    let status = widget_glow_status(&mut h);
    assert!(
        !status["ui_glow"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["name"] == "作業空間: 設計"),
        "{status}"
    );
    assert!(
        h.state()
            .command_log()
            .iter()
            .any(|(kind, s)| *kind == Kind::Input && s == "click \"作業空間: 設計\"")
    );
}

#[test]
fn disabled_button_widget_glow_is_not_recorded() {
    let mut h = widget_glow_harness();
    let result = run(&mut h, "click \"元に戻す\"");
    assert!(!result.ok);
    assert!(
        result.report().contains("押せない状態"),
        "{}",
        result.report()
    );
    assert!(
        widget_glow_status(&mut h)["ui_glow"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

/// 確かめること: 実際の接続からの台本クリックと、状態を読む道具で印を確かめる。
#[test]
fn connected_ui_tools_widget_glow_end_to_end() {
    let dir = std::env::temp_dir().join(format!(
        "widget-glow-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut ws = Workspace::new(kmb_core::Document::new(), None, kmb_bundle::modules())
        .with_data_dir(dir.clone());
    ws.layout_mut().ai_live = true;
    let mut h = harness(ws);
    let ask = |h: &mut Harness<'static, Workspace>, tool: &str, args: serde_json::Value| {
        let (data, name) = (dir.clone(), tool.to_string());
        let request = std::thread::spawn(move || kmb_session::live::call(&data, &name, args));
        let start = std::time::Instant::now();
        while !request.is_finished() {
            h.step();
            assert!(start.elapsed().as_secs() < 30, "画面への要求が終わらない");
            std::thread::yield_now();
        }
        request.join().unwrap().unwrap()
    };
    let result = ask(
        &mut h,
        "script",
        serde_json::json!({"text":"click \"作業空間: 設計\""}),
    );
    assert_eq!(result["ok"], true, "{result}");
    let status = ask(&mut h, "status", serde_json::json!({}));
    assert!(
        status["ui_glow"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["name"] == "作業空間: 設計"
                && m["source"] == "mcp"
                && m["alpha"].as_f64().unwrap() > 0.0),
        "{status}"
    );
    h.state_mut().live_disconnect();
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn type_and_select_widget_glow_use_the_actual_field_and_combo() {
    let mut h = widget_glow_harness();
    let typed = run(
        &mut h,
        "click \"設定\"\nclick \"画面\"\ntype \"字の大きさ\" 150",
    );
    assert!(typed.ok, "{}", typed.report());
    let status = widget_glow_status(&mut h);
    assert!(
        status["ui_glow"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["role"] == "SpinButton"),
        "{status}"
    );
    let selected = run(
        &mut h,
        "click \"単位\"\nselect \"この文書の長さの単位\" \"インチ(in)\"",
    );
    assert!(selected.ok, "{}", selected.report());
    let status = widget_glow_status(&mut h);
    assert!(
        status["ui_glow"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["role"] == "ComboBox"),
        "{status}"
    );
    assert_eq!(h.state().length_unit(), kmb_core::LengthUnit::In);
}
