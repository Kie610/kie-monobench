//! 設計のタイムラインの高さとグループ(2026-10-21 Kie): 既定はアイコンだけが入る低い帯で、境を
//! 引いて広げると道具とグループの名前が出る。グループは枠でなく色の下線(縦の一覧は左の縦線)。
//! グループを作った後の名前の欄は、Enter・Esc・欄の外を押すで抜けられる。人の操作で通して試す。

use crate::dock_ui::Pane;
use crate::*;
use egui::Key;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kmb_samples::reference_document;
use kmb_session::edit::GroupMode;

fn harness() -> Harness<'static, Workspace> {
    let ws = Workspace::new(reference_document(), None, kmb_bundle::modules());
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h
}

fn timeline_rect(ws: &Workspace) -> egui::Rect {
    let d = ws.docking.tree.as_ref().unwrap();
    let path = d.find_tab(&Pane::Timeline).unwrap();
    d[path.surface][path.node].get_leaf().unwrap().rect
}

/// タイムラインの中の、名前 `label` の部品(ブラウザの同じ名前の行は除く)。
fn in_timeline(h: &Harness<'static, Workspace>, label: &str) -> Option<egui::Rect> {
    let t = timeline_rect(h.state());
    h.query_all_by_label(label)
        .map(|n| n.rect())
        .find(|r| t.contains(r.center()))
}

/// 2 つ目と 3 つ目の機能をグループにする(人が札を選んで Ctrl+G を押したのと同じ入口)。
fn group_two(h: &mut Harness<'static, Workspace>) -> u64 {
    let ids: Vec<_> = h.state().cur.work.doc().data().features[1..=2]
        .iter()
        .map(|f| f.id)
        .collect();
    let gid = h
        .state_mut()
        .group_features(&ids, GroupMode::Exact)
        .unwrap();
    h.run();
    gid
}

fn group_name(h: &Harness<'static, Workspace>, gid: u64) -> String {
    let data = h.state().cur.work.doc().data();
    data.groups
        .iter()
        .find(|g| g.id == gid)
        .unwrap()
        .name
        .clone()
}

#[test]
fn the_new_group_name_box_lets_go_on_enter_escape_and_a_click_elsewhere() {
    // Enter: 打った名前に決まり、欄が消える(注目を毎こま戻さないので抜けられる)
    let mut h = harness();
    let gid = group_two(&mut h);
    assert!(h.state().cur.renaming_group.is_some());
    h.key_press_modifiers(egui::Modifiers::COMMAND, Key::A);
    h.run();
    h.get_by_label("グループの名前").type_text("ボス");
    h.run();
    h.key_press(Key::Enter);
    h.run();
    h.run();
    assert!(h.state().cur.renaming_group.is_none(), "Enter で抜けない");
    assert_eq!(group_name(&h, gid), "ボス");

    // Esc: 名前は変えずに抜ける
    let mut h = harness();
    let gid = group_two(&mut h);
    let before = group_name(&h, gid);
    h.get_by_label("グループの名前").type_text("やめる");
    h.run();
    h.key_press(Key::Escape);
    h.run();
    h.run();
    assert!(h.state().cur.renaming_group.is_none(), "Esc で抜けない");
    assert_eq!(group_name(&h, gid), before);

    // 欄の外(3D のビュー)を押す: 打った所までで決まる
    let mut h = harness();
    let gid = group_two(&mut h);
    h.key_press_modifiers(egui::Modifiers::COMMAND, Key::A);
    h.run();
    h.get_by_label("グループの名前").type_text("台");
    h.run();
    let view = {
        let d = h.state().docking.tree.as_ref().unwrap();
        let path = d.find_tab(&Pane::View).unwrap();
        d[path.surface][path.node].get_leaf().unwrap().rect
    };
    let p = view.center();
    h.event(egui::Event::PointerMoved(p));
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    h.run();
    h.run();
    assert!(
        h.state().cur.renaming_group.is_none(),
        "外を押しても抜けない"
    );
    assert_eq!(group_name(&h, gid), "台");
}

#[test]
fn the_timeline_is_low_by_default_and_shows_names_when_widened() {
    let mut h = harness();
    let gid = group_two(&mut h);
    h.key_press(Key::Enter);
    h.run();
    h.run();
    let name = group_name(&h, gid);
    let t = timeline_rect(h.state());
    // 既定はアイコンの札が入る最小(加工の段の帯の 72 より低い)
    assert!(
        (t.height() - timeline_ui::band_min_height()).abs() < 3.0 && t.height() < 60.0,
        "既定の高さ {t:?}"
    );
    // 札はタイムラインの中にあり、グループの名前はまだ出さない(開閉の印の説明だけ)
    let first = h.state().cur.work.doc().data().features[0].name.clone();
    let chip = in_timeline(&h, &first).unwrap();
    assert!(t.contains_rect(chip), "札 {chip:?} が帯 {t:?} からはみ出す");
    assert!(
        in_timeline(&h, &name).is_none(),
        "低い帯でグループの名前が出ている"
    );
    h.get_by_label(&format!("グループ「{name}」を畳む"));
    let low_w = chip.width();

    // 境を引いて広げる: 道具の名前の行とグループの名前の行が足される
    let from = band_edge(&mut h);
    drag(&mut h, from, from - egui::vec2(0.0, 60.0));
    let t2 = timeline_rect(h.state());
    assert!(
        t2.height() > t.height() + 40.0,
        "広がらない: {t:?} → {t2:?}"
    );
    let label = in_timeline(&h, &name).expect("広げてもグループの名前が出ない");
    let member = h.state().cur.work.doc().data().features[1].name.clone();
    let m = in_timeline(&h, &member).unwrap();
    assert!(
        label.bottom() <= m.top() + name_row() + 1.0 && label.center().y < m.center().y,
        "グループの名前 {label:?} が札 {m:?} の上に無い"
    );
    let chip2 = in_timeline(&h, &first).unwrap();
    assert!(chip2.height() > chip.height(), "名前の行が足されない");
    assert!(chip2.width() >= low_w);
    // グループの名前を押すと中身をまとめて選ぶ
    let at = label.center();
    h.event(egui::Event::PointerMoved(at));
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    h.run();
    assert_eq!(h.state().selection().len(), 2);
}

/// 広げた時の名前の 1 行の高さ。
fn name_row() -> f32 {
    theme::fs(10.5) * 1.25 + 2.0
}

#[test]
fn the_band_keeps_its_own_height_in_each_workspace() {
    // 加工の段の帯は名前と工具の行が要るので前の高さ、設計へ戻すとまた低いタイムライン
    let mut h = harness();
    let low = timeline_rect(h.state()).height();
    h.state_mut().switch_space(Space::Machining);
    h.run();
    h.run();
    let cam = timeline_rect(h.state()).height();
    assert!((cam - 72.0).abs() < 3.0, "段の帯の高さ {cam}");
    h.state_mut().switch_space(Space::Design);
    h.run();
    h.run();
    let back = timeline_rect(h.state()).height();
    assert!(
        (back - low).abs() < 2.0,
        "設計へ戻した高さ {back}(前 {low})"
    );
}

#[test]
fn a_group_in_the_side_list_is_marked_with_a_line_on_the_left() {
    // 縦長の所に置いた時: グループの見出しの行と、中身の行の左の縦線(枠では囲まない)
    let mut ws = Workspace::new(reference_document(), None, kmb_bundle::modules());
    let ids: Vec<_> = ws.cur.work.doc().data().features[1..=2]
        .iter()
        .map(|f| f.id)
        .collect();
    let gid = ws.group_features(&ids, GroupMode::Exact).unwrap();
    ws.cur.renaming_group = None;
    let mut h = Harness::builder()
        .with_size(egui::vec2(280.0, 600.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.timeline_ui(ui), ws);
    h.run();
    assert!(h.state().win.timeline_vertical);
    let name = group_name(&h, gid);
    let head = h.get_by_label(&name).rect();
    let toggle = h.get_by_label(&format!("グループ「{name}」を畳む")).rect();
    let member = h.state().cur.work.doc().data().features[1].name.clone();
    let m = h.get_by_label(&member).rect();
    assert!(m.top() >= head.bottom() - 1.0, "中身の行が見出しの下に無い");
    assert!(
        m.left() >= toggle.right(),
        "中身の行が下がっていない: {m:?} {toggle:?}"
    );
}

/// 3D ビューとタイムラインの境(マウスを置くと上下の矢印に変わる所)。
fn band_edge(h: &mut Harness<'static, Workspace>) -> egui::Pos2 {
    let t = timeline_rect(h.state());
    for k in 0..16 {
        let p = egui::pos2(t.center().x, t.top() + 2.0 - k as f32);
        h.event(egui::Event::PointerMoved(p));
        h.step();
        if h.output().platform_output.cursor_icon == egui::CursorIcon::ResizeVertical {
            return p;
        }
    }
    panic!("タイムラインの上の境が見つからない: {t:?}");
}

fn drag(h: &mut Harness<'static, Workspace>, from: egui::Pos2, to: egui::Pos2) {
    h.event(egui::Event::PointerMoved(from));
    h.step();
    h.event(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    h.step();
    for k in 1..=12 {
        let p = from + (to - from) * (k as f32 / 12.0);
        h.event(egui::Event::PointerMoved(p));
        h.step();
    }
    h.event(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(4);
}

/// 機能が帯に収まらない時、帯の上で縦のホイールを回すと横へ送る
/// (Shift や横のホイールが無くても動く)。
#[test]
fn the_vertical_wheel_scrolls_the_timeline_sideways() {
    // 帯に収まらないよう、スケッチを 40 個並べる
    use kmb_samples::sample::{DocBuilder, SketchBuilder};
    let mut b = DocBuilder::new();
    for k in 0..40 {
        let mut s = SketchBuilder::new(kmb_sketch::SketchPlane::Xy);
        let a = s.fixed_point(0.0, 0.0);
        let c = s.fixed_point(10.0, k as f64);
        s.line(a, c);
        b.feature(kmb_sketch::SKETCH, &format!("線{k}"), s.build());
    }
    let ws = Workspace::new(b.finish("多い"), None, kmb_bundle::modules());
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    let first = h.state().cur.work.doc().data().features[0].name.clone();
    let before = in_timeline(&h, &first).expect("最初の札");
    let t = timeline_rect(h.state());
    h.hover_at(egui::pos2(t.center().x, before.center().y));
    h.run();
    for _ in 0..4 {
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -120.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.step();
    }
    // 送りは滑らかに動くので、何コマか進めてから見る
    h.run_steps(60);
    let after = in_timeline(&h, &first).map_or(f32::NEG_INFINITY, |r| r.left());
    assert!(
        after < before.left() - 50.0,
        "縦のホイールで横へ送られない: {} → {after}",
        before.left()
    );
}

/// 視点の名前の欄も、機能・グループの名前と同じく Tab・Enter で決まり、Esc でやめる
#[test]
fn the_named_view_name_box_lets_go_on_tab_and_escape() {
    let mut h = harness();
    let id = h.state_mut().save_named_view("正面").unwrap();
    let name = |h: &Harness<'static, Workspace>| {
        h.state()
            .cur
            .work
            .doc()
            .data()
            .named_view(id)
            .unwrap()
            .name
            .clone()
    };
    // ブラウザの「名前の付いたビュー」を開く
    h.run();
    h.get_by_label_contains("名前の付いたビュー").click();
    h.run();
    // Tab: 打った名前に決まり、欄が消える
    h.state_mut().cur.renaming_view = Some((id, String::new()));
    h.run();
    h.run();
    h.get_by_label("視点の名前").type_text("前から");
    h.run();
    h.key_press(Key::Tab);
    h.run();
    h.run();
    assert!(h.state().cur.renaming_view.is_none(), "Tab で抜けない");
    assert_eq!(name(&h), "前から");
    // Esc: 名前は変えずに抜ける
    h.state_mut().cur.renaming_view = Some((id, String::new()));
    h.run();
    h.run();
    h.get_by_label("視点の名前").type_text("やめる");
    h.run();
    h.key_press(Key::Escape);
    h.run();
    h.run();
    assert!(h.state().cur.renaming_view.is_none(), "Esc で抜けない");
    assert_eq!(name(&h), "前から");
}

/// タイムラインの札の右クリックから、その機能のボディをブラウザで探せる(3D のビューの右クリックと同じ)
#[test]
fn a_timeline_chip_finds_its_body_in_the_browser() {
    let mut h = harness();
    let (id, name) = {
        let ws = h.state();
        let f = ws
            .cur
            .work
            .doc()
            .data()
            .features
            .iter()
            .find(|f| ws.output(f.id, "body").is_some())
            .expect("ボディを作る機能がある");
        (f.id, f.name.clone())
    };
    let t = timeline_rect(h.state());
    h.query_all_by_label(&name)
        .find(|n| t.contains(n.rect().center()))
        .expect("タイムラインの札")
        .click_secondary();
    h.run();
    h.get_by_label("ブラウザ内を検索").click();
    // 光らせている間は描き直し続けるので、決まった回数だけ進める
    h.run_steps(2);
    let root = h.state().body_root(id);
    assert!(matches!(h.state().cur.found, Some((Found::Browser(r), _)) if r == root));
}
