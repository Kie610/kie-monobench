//! パラメータの窓の試験: 式の欄の名前の候補(U51)、お気に入りと並べ替え(U52)、
//! 書き出し・読み込み(U54)、消したパラメータを戻す(N2)。人の操作(クリックとキー)で通す。

use crate::*;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use kmb_samples::reference_document;
use kmb_session::param_io;

fn harness(ws: Workspace) -> Harness<'static, Workspace> {
    Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws)
}

fn open_params() -> Workspace {
    let mut ws = Workspace::new(reference_document(), None, kmb_bundle::modules());
    ws.win.show_params = true;
    ws.wait();
    ws
}

/// 値が `value` の入力欄をクリックする。
fn click_field(h: &mut Harness<'_, Workspace>, value: &str) {
    h.query_all(egui_kittest::kittest::by().role(egui::accesskit::Role::TextInput))
        .find(|n| n.accesskit_node().value().as_deref() == Some(value))
        .unwrap_or_else(|| panic!("値 {value} の欄"))
        .click();
    h.run();
}

fn param_expr(h: &Harness<'_, Workspace>, name: &str) -> String {
    h.state()
        .document()
        .data()
        .param_by_name(name)
        .unwrap()
        .expr
        .clone()
}

#[test]
fn typing_part_of_a_name_offers_it_and_tab_or_enter_puts_it_in() {
    // 1 つのパラメータの窓で順に確かめる:
    // - 名前の候補はパラメータと寸法から(元 the_name_list_comes_from_parameters_and_dimensions)
    // - 名前の一部を打つと候補が出て、Tab で入れる(元 typing_part_of_a_name_offers_it_and_tab_puts_it_in)
    // - Esc は一覧だけ閉じ、Enter で打ったものを決める
    //   (元 escape_closes_the_list_and_enter_then_commits_what_was_typed)
    let ws = open_params();
    let names: Vec<String> = ws.expr_candidates().into_iter().map(|c| c.name).collect();
    assert!(names.contains(&"width".into()) && names.contains(&"d1".into()));
    let mut h = harness(ws);
    h.run();
    // height の式の欄に「wi」と打つと、下に width が出る
    click_field(&mut h, "40 mm");
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.event(egui::Event::Text("wi".into()));
    h.run();
    h.run();
    assert!(
        h.query_by_label("↑↓ で選び Tab・Enter で入れる。Esc で閉じる")
            .is_some(),
        "候補の一覧が出る"
    );
    assert!(
        h.query_all_by_label_contains("60 mm")
            .any(|n| n.accesskit_node().role() != egui::accesskit::Role::TextInput),
        "候補に今の値を添える"
    );
    // Tab で入れる(欄から外れない)。続けて打ち、Enter で決める
    h.key_press(egui::Key::Tab);
    h.run();
    h.event(egui::Event::Text(" / 2".into()));
    h.run();
    // 数字の後では一覧を出さない(Enter が d2 などを入れてしまわない)
    assert!(
        h.query_by_label("↑↓ で選び Tab・Enter で入れる。Esc で閉じる")
            .is_none()
    );
    h.key_press(egui::Key::Enter);
    h.run();
    assert_eq!(param_expr(&h, "height"), "width / 2");
    assert_eq!(h.state().report().error_count(), 0);

    // hole の欄の後ろに打つ
    click_field(&mut h, "8 mm");
    h.key_press(egui::Key::End);
    h.event(egui::Event::Text(" + co".into()));
    h.run();
    h.run();
    let list = "↑↓ で選び Tab・Enter で入れる。Esc で閉じる";
    assert!(h.query_by_label(list).is_some());
    // ↓ で 2 つめ(cos などの前に、同じ頭の corner_hole が先に並ぶ)
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.query_by_label(list).is_none(), "Esc で一覧だけ閉じる");
    // 欄は打ったまま残る
    assert!(
        h.query_all(egui_kittest::kittest::by().role(egui::accesskit::Role::TextInput))
            .any(|n| n.accesskit_node().value().as_deref() == Some("8 mm + co"))
    );
    h.event(egui::Event::Text("rner_hole".into()));
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    assert_eq!(param_expr(&h, "hole"), "8 mm + corner_hole");
}

#[test]
fn a_star_puts_the_row_in_favorites_where_rows_can_be_moved() {
    let mut h = harness(open_params());
    h.run();
    // 1 行目(width)と 4 行目(hole)の ☆ を押す
    let stars = |h: &Harness<'_, Workspace>| -> Vec<egui::Rect> {
        h.get_all_by_label("☆")
            .filter(|n| n.accesskit_node().role() == egui::accesskit::Role::Button)
            .map(|n| n.rect())
            .collect()
    };
    let s = stars(&h);
    h.get_all_by_label("☆")
        .filter(|n| n.accesskit_node().role() == egui::accesskit::Role::Button)
        .nth(3)
        .unwrap()
        .click();
    h.run();
    h.get_all_by_label("☆")
        .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Button)
        .unwrap()
        .click();
    h.run();
    assert_eq!(stars(&h).len(), s.len() - 2);
    assert_eq!(h.get_all_by_label("★").count(), 2);
    let data = h.state().document().data();
    let fav = data.favorites_view().expect("お気に入りの表ができる");
    let names: Vec<String> = fav
        .members
        .iter()
        .filter_map(|k| match k {
            kmb_core::ParamKey::User { id } => data
                .params
                .iter()
                .find(|p| p.id == *id)
                .map(|p| p.name.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(names, ["hole", "width"], "押した順");
    // お気に入りのタブには 2 行だけ出る
    h.get_by_label("☆ お気に入り").click();
    h.run();
    let fields: Vec<String> = h
        .query_all(egui_kittest::kittest::by().role(egui::accesskit::Role::TextInput))
        .filter_map(|n| n.accesskit_node().value().map(|v| v.to_string()))
        .collect();
    assert!(fields.contains(&"hole".into()) && fields.contains(&"width".into()));
    assert!(!fields.contains(&"height".into()), "{fields:?}");
    // width を右クリックして「上へ」
    h.query_all(egui_kittest::kittest::by().role(egui::accesskit::Role::TextInput))
        .find(|n| n.accesskit_node().value().as_deref() == Some("width"))
        .unwrap()
        .click_secondary();
    h.run();
    h.get_by_label("上へ").click();
    h.run();
    let data = h.state().document().data();
    let first = &data.favorites_view().unwrap().members[0];
    assert_eq!(
        first,
        &kmb_core::ParamKey::User {
            id: data.param_by_name("width").unwrap().id
        }
    );
}

#[test]
fn a_removed_parameter_is_shown_faint_and_restored() {
    let mut ws = open_params();
    assert!(ws.add_param("extra", "1 mm"));
    let mut h = harness(ws);
    h.run();
    // extra の行の「削除」(名前の欄にいちばん近い物)を押す
    let field = |h: &Harness<'_, Workspace>| {
        h.query_all(egui_kittest::kittest::by().role(egui::accesskit::Role::TextInput))
            .find(|n| n.accesskit_node().value().as_deref() == Some("extra"))
            .expect("extra の欄")
            .rect()
            .center()
    };
    h.query_all(egui_kittest::kittest::by().role(egui::accesskit::Role::TextInput))
        .find(|n| n.accesskit_node().value().as_deref() == Some("extra"))
        .unwrap()
        .scroll_to_me();
    h.run();
    let at = field(&h);
    h.get_all_by_label("削除")
        .filter(|n| !n.accesskit_node().is_disabled())
        .min_by(|a, b| {
            let d = |n: &egui_kittest::Node<'_>| (n.rect().center().y - at.y).abs();
            d(a).total_cmp(&d(b))
        })
        .expect("押せる「削除」")
        .click();
    h.run();
    assert!(h.state().document().data().param_by_name("extra").is_none());
    assert!(
        h.query_by_label("戻す").is_none(),
        "チェックを入れるまでは出さない"
    );
    // 表を下へ送っているので、チェックが見える所まで戻してから押す
    h.get_by_label("消した物も出す(1)").scroll_to_me();
    h.run();
    h.get_by_label("消した物も出す(1)").click();
    h.run();
    h.get_by_label("戻す").scroll_to_me();
    h.run();
    h.get_by_label("戻す").click();
    h.run();
    let data = h.state().document().data();
    assert_eq!(data.param_by_name("extra").unwrap().expr, "1 mm");
    assert!(data.removed_params.is_empty());
}

#[test]
fn the_table_is_written_and_read_back_as_csv_and_json() {
    let mut ws = open_params();
    ws.wait();
    let dir = std::env::temp_dir().join(format!("kmb-param-table-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (ext, f) in [
        ("csv", param_io::Format::Csv),
        ("json", param_io::Format::Json),
    ] {
        let path = dir.join(format!("table.{ext}"));
        let keys =
            kmb_session::params::all_keys(ws.document().data(), &ws.win.registry, ws.report());
        let rows = param_io::rows(ws.document().data(), &ws.win.registry, ws.report(), &keys);
        ws.write_param_table(&path, "全部", rows, f);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("width") && text.contains("d1"), "{text}");
        // 表計算で width を直したとして読み込む
        std::fs::write(&path, text.replace("60 mm", "64 mm")).unwrap();
        assert!(ws.read_param_table(&path), "{:?}", ws.win.message);
        ws.wait();
        assert_eq!(
            ws.document().data().param_by_name("width").unwrap().expr,
            "64 mm"
        );
        assert!(ws.win.message.as_deref().unwrap().contains("変えた 1"));
        ws.undo();
        ws.wait();
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// パラメータ `np` 個(p0 から順に 1 つ前 + 0.1 mm)と、寸法 2 つの長方形のスケッチ `ns` 個の文書。
fn large_design(np: usize, ns: usize) -> String {
    use kmb_samples::sample::{DocBuilder, SketchBuilder};
    let mut b = DocBuilder::new();
    b.param("p0", "10 mm");
    for i in 1..np {
        b.param(&format!("p{i}"), &format!("p{} + 0.1 mm", i - 1));
    }
    for i in 0..ns {
        let mut s = SketchBuilder::xy();
        let c = s.point(0.0, 0.0);
        let (w, h) = (format!("p{}", i % np), format!("p{} * 2", (i + 1) % np));
        s.rectangle(c, &w, &h, [0.0, 0.0, 10.0, 20.0]);
        b.sketch(&format!("スケッチ{i}"), s);
    }
    b.finish("大きな設計").to_json()
}

/// 測るだけ(R15): パラメータと寸法の多い文書で、窓を開いた時と閉じた時の 1 コマの時間。
/// 数は `JC_PARAMS`(既定 300)・`JC_SKETCHES`(既定 100、寸法はその 2 倍)。
#[test]
#[ignore = "道具: 大きな設計でパラメータの窓の重さを測る(R15)"]
fn measure_params_window_on_a_large_design() {
    let n = |k: &str, d: usize| {
        std::env::var(k)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(d)
    };
    let doc = large_design(n("JC_PARAMS", 300), n("JC_SKETCHES", 100));
    let time = |open: bool| {
        let mut ws = Workspace::new(
            kmb_core::Document::from_json(&doc).unwrap(),
            None,
            kmb_bundle::modules(),
        );
        ws.win.show_params = open;
        ws.wait();
        let mut h = harness(ws);
        h.run();
        let t = std::time::Instant::now();
        for _ in 0..20 {
            h.step();
        }
        t.elapsed().as_secs_f64() * 1000.0 / 20.0
    };
    let closed = time(false);
    let open = time(true);
    eprintln!("1 コマ: 閉じた {closed:.1} ms、開いた {open:.1} ms");
}

/// 大きな設計(R15): 見えていない行は欄を置かない(軽くする)が、巻けば出てきて、打って決められる。
/// 人の手順: パラメータの窓を開く → 最後の p119 の欄は見えない → ホイールで下まで巻く → p119 の式を
/// 押して打ち、Enter で決まる。
#[test]
fn rows_out_of_view_are_not_built_but_scroll_in_and_can_be_edited() {
    let doc = kmb_core::Document::from_json(&large_design(120, 0)).unwrap();
    let mut ws = Workspace::new(doc, None, kmb_bundle::modules());
    ws.win.show_params = true;
    ws.wait();
    let mut h = harness(ws);
    h.run();
    let field = |h: &Harness<'_, Workspace>, v: &str| {
        h.query_all(egui_kittest::kittest::by().role(egui::accesskit::Role::TextInput))
            .any(|n| n.accesskit_node().value().as_deref() == Some(v))
    };
    assert!(field(&h, "p0"), "上の行は出る");
    assert!(!field(&h, "p119"), "見えない行の欄は置かない");
    let n = h
        .query_all(egui_kittest::kittest::by().role(egui::accesskit::Role::TextInput))
        .count();
    assert!(n < 120, "欄は見えている行の分だけ: {n}");
    h.query_all(egui_kittest::kittest::by().role(egui::accesskit::Role::TextInput))
        .find(|n| n.accesskit_node().value().as_deref() == Some("p0"))
        .unwrap()
        .hover();
    h.run();
    for _ in 0..40 {
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -2000.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.step();
    }
    h.run_steps(30);
    assert!(field(&h, "p119"), "巻くと出てくる");
    assert!(!field(&h, "p0"), "上へ外れた行は置かない");
    click_field(&mut h, "p118 + 0.1 mm");
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.event(egui::Event::Text("3 mm".into()));
    h.key_press(egui::Key::Enter);
    h.run();
    assert_eq!(param_expr(&h, "p119"), "3 mm");
}

#[test]
fn mcp_keys_open_the_empty_expression_list_in_the_table_and_feature_panel() {
    use serde_json::json;
    struct Scratch(std::path::PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn app(
        h: &mut Harness<'static, Workspace>,
        dir: &std::path::Path,
        tool: &str,
        args: serde_json::Value,
    ) {
        let (dir, tool) = (dir.to_owned(), tool.to_owned());
        let request = std::thread::spawn(move || kmb_session::live::call(&dir, &tool, args));
        let start = std::time::Instant::now();
        while !request.is_finished() {
            h.step();
            std::thread::sleep(std::time::Duration::from_millis(2));
            assert!(start.elapsed().as_secs() < 20, "画面の要求が時間切れ");
        }
        let result = request.join().unwrap().unwrap();
        assert_eq!(result["ok"], true, "{result}");
        h.run_steps(12);
    }
    for table in [true, false] {
        let part = kmb_samples::reference_part();
        let plate = part.plate;
        let dir = Scratch(
            std::env::temp_dir().join(format!("empty-completion-{}-{table}", std::process::id())),
        );
        let mut ws =
            Workspace::new(part.doc, None, kmb_bundle::modules()).with_data_dir(dir.0.clone());
        ws.layout_mut().ai_live = true;
        ws.win.show_params = table;
        if !table {
            ws.open_edit(plate);
        }
        let mut h = harness(ws);
        h.run();
        let target = if table { "label:式#1" } else { "label:距離" };
        let before = h.state().document().to_json();
        app(
            &mut h,
            &dir.0,
            "ui_type",
            json!({"target":target,"text":"","commit":"none"}),
        );
        app(&mut h, &dir.0, "ui_key", json!({"keys":"Down"}));
        app(
            &mut h,
            &dir.0,
            "ui_expect",
            json!({"target":"↑↓ で選び Tab・Enter で入れる。Esc で閉じる"}),
        );
        assert_eq!(
            before,
            h.state().document().to_json(),
            "候補を見るだけでは文書を変えない"
        );
        assert!(
            h.query_all_by_label_contains("3.4 mm")
                .any(|n| n.accesskit_node().role() != egui::accesskit::Role::TextInput),
            "値と単位を添える"
        );
        app(&mut h, &dir.0, "ui_key", json!({"keys":"Enter"}));
        app(&mut h, &dir.0, "ui_key", json!({"keys":"Tab"}));
        if table {
            assert_eq!(param_expr(&h, "width"), "corner_hole");
        } else {
            assert_eq!(
                h.state().document().data().feature(plate).unwrap().data["distance"],
                "corner_hole"
            );
        }
        h.state_mut().live_disconnect();
        drop(h);
        drop(dir);
    }
}
