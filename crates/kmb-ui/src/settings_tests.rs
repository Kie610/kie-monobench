//! 設定の窓(U39)とキーの割り当て(U36)を、人の操作(クリックとキー)で通して試す。

use super::*;
use crate::layout::Brightness;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kmb_samples::reference_document;

fn harness() -> Harness<'static, Workspace> {
    let mut ws = Workspace::new(reference_document(), None, kmb_bundle::modules());
    ws.layout_mut().brightness = Brightness::Light;
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h
}

fn press(h: &mut Harness<'static, Workspace>, key: egui::Key, modifiers: egui::Modifiers) {
    for pressed in [true, false] {
        h.event(egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        });
    }
    h.run();
}

/// 設定の窓の中にある、`label` の部品(ツールバーや作業空間の切り替えの同じ名前と取り違えない)。
fn in_settings<'a>(h: &'a Harness<'static, Workspace>, label: &'a str) -> egui_kittest::Node<'a> {
    let win = h
        .ctx
        .memory(|m| m.area_rect(egui::Id::new("settings")))
        .expect("設定の窓");
    // 窓の下に隠れた行もあるので、横の位置だけで見る。窓はツールバーに重なって描くので、
    // 同じ名前のツールバーのボタンが窓の幅の中にあれば、後に描いた窓の方を取る
    h.get_all_by_label(label)
        .rfind(|n| {
            let r = n.rect();
            r.left() >= win.left() && r.right() <= win.right() && r.top() >= win.top()
        })
        .unwrap_or_else(|| panic!("設定の窓に「{label}」"))
}

/// キーの割り当ての表で、働きが `text` の行の、`button` のボタンを押す。
fn row_button(h: &mut Harness<'static, Workspace>, text: &str, button: &str) {
    in_settings(h, text).scroll_to_me();
    h.run();
    let y = in_settings(h, text).rect().center().y;
    h.get_all_by_label(button)
        .min_by_key(|b| (b.rect().center().y - y).abs() as i32)
        .expect("行のボタン")
        .click();
    h.run();
}

fn change_row(h: &mut Harness<'static, Workspace>, text: &str) {
    row_button(h, text, "変える");
}

#[test]
fn ctrl_comma_opens_the_settings_whose_pages_and_search_box_find_settings() {
    // 1 つの設定の窓で順に確かめる:
    // - Ctrl+, で開き、タブと頁を替え、閉じて開き直すと同じ頁
    //   (元 ctrl_comma_opens_the_settings_on_tabs_and_pages_and_remembers_the_page)
    // - 先頭の検索の欄で、頁をまたいで設定とキーの働きが当たる(元 the_search_box_finds_settings_across_pages)
    let mut h = harness();
    press(&mut h, egui::Key::Comma, egui::Modifiers::COMMAND);
    assert!(h.state().settings.open, "Ctrl+, で設定の窓");
    // 一般のタブの先頭は「画面」の頁
    h.get_by_label("テーマ");
    assert!(h.query_by_label("ホイール押しドラッグ").is_none());
    // 左の木で「マウスとキー」の頁へ
    in_settings(&h, "マウスとキー").click();
    h.run();
    h.get_by_label("ホイール押しドラッグ");
    assert!(
        h.query_by_label("テーマ").is_none(),
        "ほかの頁の欄は出さない"
    );
    assert_eq!(h.state().win.layout.settings_page, "general/input");
    // 設計のタブ: 3D の表示
    in_settings(&h, "設計").click();
    h.run();
    h.get_by_label("平行投影");
    // 加工のタブ: モジュールの設定が頁ごとに並ぶ
    in_settings(&h, "加工").click();
    h.run();
    h.get_by_label("作業範囲 X(mm)");
    assert!(h.query_by_label("ポスト(G-code の書き方)").is_some());
    // 閉じて開き直すと、同じ頁が出る
    press(&mut h, egui::Key::Comma, egui::Modifiers::COMMAND);
    assert!(!h.state().settings.open);
    press(&mut h, egui::Key::Comma, egui::Modifiers::COMMAND);
    h.get_by_label("作業範囲 X(mm)");

    // 開いたままの窓の先頭の検索の欄に打つ
    let win = h
        .ctx
        .memory(|m| m.area_rect(egui::Id::new("settings")))
        .unwrap();
    // 窓の下に隠れたブラウザの欄と取り違えないよう、窓の幅いっぱいの欄を選ぶ
    h.get_all_by_role(egui::accesskit::Role::TextInput)
        .find(|n| win.contains_rect(n.rect()) && n.rect().width() > win.width() * 0.8)
        .expect("検索の欄")
        .click();
    h.run();
    h.event(egui::Event::Text("投影".into()));
    h.run();
    assert_eq!(h.state().settings.query, "投影");
    h.get_by_label("設計 › 3D の表示");
    h.get_by_label("透視投影");
    assert!(h.query_by_label("テーマ").is_none());
    // キーの働きの名前でも当たる(当たった行だけを出す)
    h.state_mut().settings.query = "トリム".into();
    h.run();
    h.get_by_label("一般 › マウスとキー");
    h.get_by_label("スケッチ: トリム");
    assert!(h.query_by_label("スケッチ: 線分").is_none());
    // 描画(U26)と STEP の読み込み(U12)の設定も頁に入っている
    h.state_mut().settings.query = "アンチエイリアス".into();
    h.run();
    h.get_by_label("一般 › 描画");
    h.state_mut().settings.query = "Vulkan".into();
    h.run();
    h.get_by_label("描画の API");
    h.state_mut().settings.query = "STEP".into();
    h.run();
    h.get_by_label("一般 › ファイル");
    h.get_by_label("STEP・IGES を読み込んだら、立てるかを聞く");
    h.state_mut().settings.query = "IGES".into();
    h.run();
    h.get_by_label("一般 › ファイル");
    h.get_by_label("STEP・IGES を読み込んだら、立てるかを聞く");
    // 当たらなければそう出す
    h.state_mut().settings.query = "存在しない設定".into();
    h.run();
    h.get_by_label("当たる設定はありません");
}

#[test]
fn a_key_is_remapped_by_pressing_it_and_escape_stops_listening() {
    // 1 つの設定の窓で順に確かめる:
    // - 押したキーに付け替わり、前のキーは働かない。重なりの印・戻す・全部を既定に戻す
    //   (元 a_key_is_remapped_by_pressing_it_and_the_old_key_stops_working)
    // - Esc は受けるのをやめるだけでキーを変えず、Ctrl と組み合わせたキーも受ける
    //   (元 escape_stops_listening_without_changing_the_key)
    let mut h = harness();
    h.state_mut().open_settings_at("general/input");
    h.run();
    // 「計測」を W にする(押した次のキーが入る)
    change_row(&mut h, "計測");
    assert!(h.state().settings.key_capture.is_some());
    press(&mut h, egui::Key::W, egui::Modifiers::NONE);
    assert_eq!(h.state().settings.key_capture, None);
    assert_eq!(h.state().win.layout.keys["measure"], vec!["W".to_string()]);
    assert!(h.state().cur.measure.is_none(), "受けたキーは働かせない");
    // 付け替えられないキーは理由を出して断り、受けたまま
    change_row(&mut h, "穴を始める");
    press(&mut h, egui::Key::Num5, egui::Modifiers::NONE);
    h.get_by_label_contains("値を打つのに使うキー");
    assert!(h.state().settings.key_capture.is_some());
    // 同じキーを付けると、両方の行に ⚠ が出る
    press(&mut h, egui::Key::W, egui::Modifiers::NONE);
    assert_eq!(h.state().win.layout.keys["hole"], vec!["W".to_string()]);
    assert_eq!(h.query_all_by_label("⚠").count(), 2);
    h.get_by_label_contains("「計測」と重なっています");
    // 穴は「戻す」で既定の H に
    row_button(&mut h, "穴を始める", "戻す");
    assert!(!h.state().win.layout.keys.contains_key("hole"));
    assert_eq!(h.query_all_by_label("⚠").count(), 0);
    // 窓を閉じると、W で測り、I では測らない。コマンド検索の行の右と手引きも W
    h.state_mut().toggle_settings();
    h.run();
    press(&mut h, egui::Key::I, egui::Modifiers::NONE);
    assert!(h.state().cur.measure.is_none());
    press(&mut h, egui::Key::W, egui::Modifiers::NONE);
    assert!(h.state().cur.measure.is_some());
    assert_eq!(
        h.state().key_of(&crate::commands_ui::CommandId::Measure),
        "W"
    );
    // 付け替えは layout.json に書く物に入り、全部を既定に戻せる
    let saved = serde_json::to_string(h.state().layout()).unwrap();
    assert!(saved.contains("\"measure\""), "{saved}");
    h.state_mut().open_settings_at("general/input");
    h.run();
    h.get_by_label("キーを全部既定に戻す").scroll_to_me();
    h.run();
    h.get_by_label("キーを全部既定に戻す").click();
    h.run();
    assert!(h.state().win.layout.keys.is_empty());
    assert_eq!(
        h.state().key_of(&crate::commands_ui::CommandId::Measure),
        "I"
    );

    // 既定に戻した窓のまま、受けている間の Esc
    change_row(&mut h, "全部を計算し直す");
    press(&mut h, egui::Key::Escape, egui::Modifiers::NONE);
    assert_eq!(h.state().settings.key_capture, None);
    assert!(h.state().win.layout.keys.is_empty());
    assert!(h.state().settings.open, "Esc は受けるのをやめるだけ");
    // Ctrl と組み合わせたキーも受ける
    change_row(&mut h, "全部を計算し直す");
    press(
        &mut h,
        egui::Key::S,
        egui::Modifiers::COMMAND | egui::Modifiers::ALT,
    );
    assert_eq!(
        h.state().win.layout.keys["recompute"],
        vec!["Ctrl+Alt+S".to_string()]
    );
}

#[test]
fn changing_the_new_document_unit_also_sets_it_on_the_untouched_untitled_document() {
    // 10-24 u11: 起動してすぐ設定で新規デザインの単位を変えると、開いている空の文書にも入る
    let mut ws = Workspace::new(kmb_core::Document::new(), None, kmb_bundle::modules());
    ws.layout_mut().brightness = Brightness::Light;
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.state_mut().open_settings_at("general/new_doc");
    h.run();
    h.get(
        egui_kittest::kittest::by()
            .role(egui::accesskit::Role::ComboBox)
            .value("ミリメートル(mm)"),
    )
    .click();
    h.run();
    h.get_by_label("インチ(in)").click();
    h.run();
    assert_eq!(h.state().win.layout.new_doc.units, kmb_core::LengthUnit::In);
    assert_eq!(h.state().length_unit(), kmb_core::LengthUnit::In);
    assert!(!h.state().document().can_undo() && !h.state().is_modified());
}

#[test]
fn copy_cut_and_paste_keys_with_shift_are_refused_with_a_reason() {
    // Ctrl+C・Ctrl+X・Ctrl+V は Shift・Alt を足しても、PC から「写す」などとして届き、キーとしては
    // 来ない。受けたまま黙らず理由を出し、キーとして来ても同じ理由で断る(10-25 u6)
    let mut h = harness();
    h.state_mut().open_settings_at("general/input");
    h.run();
    change_row(&mut h, "全部を計算し直す");
    h.event(egui::Event::Copy);
    h.run();
    h.get_by_label_contains("写す・切り取る・貼り付ける");
    assert!(
        h.state().settings.key_capture.is_some(),
        "断った後も受けたまま"
    );
    press(
        &mut h,
        egui::Key::C,
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
    );
    assert!(
        h.state().settings.key_capture.is_some(),
        "Ctrl+Shift+C は付けない"
    );
    assert!(!h.state().win.layout.keys.contains_key("recompute"));
    // ほかの Ctrl+Shift の組み合わせは付く
    press(
        &mut h,
        egui::Key::K,
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
    );
    assert_eq!(
        h.state().win.layout.keys["recompute"],
        vec!["Ctrl+Shift+K".to_string()]
    );
}
