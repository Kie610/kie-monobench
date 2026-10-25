//! 画面の部品を自由に並べる(N4)を、人の操作(タブを引く・右クリック・ボタン)で通して試す。

use super::*;
use crate::dock_ui::{Pane, dock_from_json, dock_shape};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kmb_samples::reference_document;

fn harness_with(ws: Workspace) -> Harness<'static, Workspace> {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h
}

fn harness() -> Harness<'static, Workspace> {
    harness_with(Workspace::new(
        reference_document(),
        None,
        kmb_bundle::modules(),
    ))
}

/// 部品の場所(並びの中の四角)。浮かせた窓や並びに無い部品は `None`。
fn rect_of(ws: &Workspace, p: Pane) -> Option<egui::Rect> {
    let d = ws.docking.tree.as_ref()?;
    let path = d.find_tab(&p)?;
    Some(d[path.surface][path.node].get_leaf()?.rect)
}

/// 同じ所にタブで重なっている部品。
fn same_leaf(ws: &Workspace, a: Pane, b: Pane) -> bool {
    let d = ws.docking.tree.as_ref().unwrap();
    let (pa, pb) = (d.find_tab(&a).unwrap(), d.find_tab(&b).unwrap());
    pa.surface == pb.surface && pa.node == pb.node
}

/// 部品のタブの位置(タブの帯の左から 2 つ目までの真ん中あたり)。
fn tab_pos(ws: &Workspace, p: Pane) -> egui::Pos2 {
    let d = ws.docking.tree.as_ref().unwrap();
    let path = d.find_tab(&p).unwrap();
    let leaf = d[path.surface][path.node].get_leaf().unwrap();
    let i = leaf.tabs.iter().position(|t| *t == p).unwrap() as f32;
    leaf.rect.left_top() + egui::vec2(40.0 + 90.0 * i, 10.0)
}

/// 左ボタンで引いて落とす(途中を何回かに分けて動かす)。
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

fn right_click(h: &mut Harness<'static, Workspace>, at: egui::Pos2) {
    h.event(egui::Event::PointerMoved(at));
    h.run();
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.run();
    }
}

#[test]
fn the_default_arrangement_floats_a_pane_and_keeps_the_window_buttons() {
    // 1 つの画面で順に確かめる:
    // - 既定の並びは前の版の固定の配置と同じ(元 the_default_arrangement_matches_the_old_fixed_layout)
    // - 見出しの右クリックで窓に浮かせ、浮かせた窓にはタブの帯を出す
    //   (元 a_pane_floats_from_its_header_menu_and_stacked_panes_show_tabs)
    // - 窓のボタンは上の帯にあり、標準の帯にすると出さない(元 the_window_buttons_are_on_the_top_band)
    let mut h = harness();
    let ws = h.state();
    let view = rect_of(ws, Pane::View).unwrap();
    let browser = rect_of(ws, Pane::Browser).unwrap();
    let panel = rect_of(ws, Pane::Panel).unwrap();
    let timeline = rect_of(ws, Pane::Timeline).unwrap();
    // 左にブラウザ、右に編集パネル、真ん中に 3D ビュー、下いっぱいにタイムライン
    assert!(browser.right() <= view.left() + 1.0, "{browser:?} {view:?}");
    assert!(panel.left() >= view.right() - 1.0, "{panel:?} {view:?}");
    assert!(timeline.top() >= view.bottom() - 1.0);
    assert!(
        timeline.width() > 1300.0,
        "タイムラインは下いっぱい: {timeline:?}"
    );
    // 幅は前の版の既定(ブラウザ 250・編集パネル 312)に近い
    assert!(
        (browser.width() - 250.0).abs() < 20.0,
        "{}",
        browser.width()
    );
    assert!((panel.width() - 312.0).abs() < 20.0, "{}", panel.width());
    assert!(ws.dock_is_default());
    // 何も動かしていなければ、並びは書かない
    assert!(ws.win.layout.dock.is_none());

    // 部品が 1 つだけの所はタブの帯を出さない(3D ビューの大きさは前の版と同じ)
    let ws = h.state();
    let d = ws.docking.tree.as_ref().unwrap();
    assert!(d.iter_leaves().all(|(_, l)| l.tab_bar_hidden));
    // ブラウザの見出しを右クリックして「窓に浮かせる」
    let at = tab_pos(h.state(), Pane::Browser);
    right_click(&mut h, at);
    h.get_by_label("窓に浮かせる").click();
    h.run();
    h.run();
    let d = h.state().docking.tree.as_ref().unwrap();
    let path = d.find_tab(&Pane::Browser).unwrap();
    assert!(!path.surface.is_main(), "窓に浮く");
    assert!(!h.state().dock_is_default());
    // 部品 1 つの浮かせた窓は、タブの帯の代わりに外の窓と同じ帯(真ん中に名前、右に閉じる)
    let leaf = d[path.surface][path.node].get_leaf().unwrap();
    assert!(leaf.tab_bar_hidden);
    let closes = h.get_all_by_label("閉じる").count();
    assert_eq!(closes, 2, "本体の閉じると、浮かせた窓の閉じる");
    // 浮かせた窓の閉じるで、浮かせる前の場所へ戻る
    // (本体の閉じるは右の端なので、左にある方)
    h.get_all_by_label("閉じる")
        .min_by(|a, b| a.rect().left().total_cmp(&b.rect().left()))
        .unwrap()
        .click();
    h.run();
    h.run();
    let back = rect_of(h.state(), Pane::Browser).unwrap();
    assert!(
        h.state()
            .docking
            .tree
            .as_ref()
            .unwrap()
            .find_tab(&Pane::Browser)
            .unwrap()
            .surface
            .is_main(),
        "並びへ戻る"
    );
    assert!(
        (back.left() - browser.left()).abs() < 2.0,
        "{back:?} {browser:?}"
    );
    assert!(
        (back.width() - browser.width()).abs() < 2.0,
        "{back:?} {browser:?}"
    );

    // 窓のボタンは上の帯
    for name in ["最小化", "最大化", "閉じる"] {
        h.get_by_label(name);
    }
    // 標準の帯にすると、アプリのボタンは出さない
    h.state_mut().layout_mut().system_title_bar = true;
    h.run();
    assert!(h.query_by_label("最小化").is_none());
}

#[test]
fn a_tab_is_dragged_reset_and_a_hidden_pane_comes_back_where_it_was() {
    // 1 つの画面で順に確かめる:
    // - タブを引いて別の側へ・重ねて、右クリックで既定に戻す
    //   (元 a_tab_is_dragged_to_another_side_and_reset_from_its_menu)
    // - 隠した部品は元の場所に戻り、タイムラインも下いっぱいに戻る(元 a_hidden_pane_comes_back_where_it_was)
    let mut h = harness();
    let view = rect_of(h.state(), Pane::View).unwrap();
    // ブラウザのタブを引いて、3D ビューの右の端へ落とす
    let from = tab_pos(h.state(), Pane::Browser);
    drag(
        &mut h,
        from,
        egui::pos2(view.right() - 15.0, view.center().y),
    );
    let ws = h.state();
    let browser = rect_of(ws, Pane::Browser).unwrap();
    let view = rect_of(ws, Pane::View).unwrap();
    assert!(
        browser.left() >= view.right() - 1.0,
        "ブラウザが右へ: {browser:?} {view:?}"
    );
    assert!(!ws.dock_is_default());
    assert!(ws.win.layout.dock.is_some(), "並びを覚える");
    // 半分ずつに分けず、前の版の幅のまま
    assert!(
        (browser.width() - 250.0).abs() < 20.0,
        "{}",
        browser.width()
    );

    // 編集パネルのタブを引いて、ブラウザに重ねる(真ん中へ落とす)
    let from = tab_pos(h.state(), Pane::Panel);
    let to = rect_of(h.state(), Pane::Browser).unwrap().center();
    drag(&mut h, from, to);
    assert!(
        same_leaf(h.state(), Pane::Panel, Pane::Browser),
        "タブで重なる"
    );
    // 重ねた所にはタブの帯が出る(タブで選び替える)
    let d = h.state().docking.tree.as_ref().unwrap();
    let p = d.find_tab(&Pane::Browser).unwrap();
    assert!(!d[p.surface][p.node].get_leaf().unwrap().tab_bar_hidden);

    // タブの右クリックの「並びを既定に戻す」
    let at = tab_pos(h.state(), Pane::Browser);
    right_click(&mut h, at);
    h.get_by_label("並びを既定に戻す").click();
    h.run();
    h.run();
    let ws = h.state();
    assert!(ws.dock_is_default());
    assert!(ws.win.layout.dock.is_none());
    let browser = rect_of(ws, Pane::Browser).unwrap();
    let view = rect_of(ws, Pane::View).unwrap();
    assert!(browser.right() <= view.left() + 1.0, "左へ戻る");

    // 既定に戻した並びから、もう一度ブラウザを右へ
    let view = rect_of(h.state(), Pane::View).unwrap();
    let from = tab_pos(h.state(), Pane::Browser);
    drag(
        &mut h,
        from,
        egui::pos2(view.right() - 15.0, view.center().y),
    );
    let before = rect_of(h.state(), Pane::Browser).unwrap();
    // ブラウザの見出しの「隠す」で隠し、3D ビューの左の端のつまみで出す
    h.get_by_label("ブラウザを隠す").click();
    h.run();
    h.run();
    assert!(
        rect_of(h.state(), Pane::Browser).is_none(),
        "並びから外れる"
    );
    h.get_by_label("ブラウザを出す").click();
    h.run();
    h.run();
    let after = rect_of(h.state(), Pane::Browser).unwrap();
    assert!(
        (after.left() - before.left()).abs() < 2.0 && (after.width() - before.width()).abs() < 2.0,
        "元の場所に戻る: {before:?} → {after:?}"
    );

    // タブの「隠す」(閉じるの印)でも隠せる。表示のメニューの印も外れる
    h.state_mut().set_pane_open(Pane::Timeline, false);
    h.run();
    h.run();
    assert!(rect_of(h.state(), Pane::Timeline).is_none());
    assert!(!h.state().win.layout.timeline_open);
    h.state_mut().set_pane_open(Pane::Timeline, true);
    h.run();
    h.run();
    let t = rect_of(h.state(), Pane::Timeline).unwrap();
    assert!(t.width() > 1300.0, "下いっぱいに戻る: {t:?}");
}

#[test]
fn the_arrangement_is_remembered_across_restarts_unless_it_is_broken() {
    // 順に確かめる:
    // - 書いた配置で開き直すと同じ並び(元 the_arrangement_is_remembered_across_restarts)
    // - 壊れた並びは使わず既定に戻し、覚える形に四角を入れない
    //   (元 a_broken_saved_arrangement_falls_back_to_the_default)
    let mut h = harness();
    let view = rect_of(h.state(), Pane::View).unwrap();
    let from = tab_pos(h.state(), Pane::Timeline);
    // タイムラインを 3D ビューの上の端へ
    drag(&mut h, from, egui::pos2(view.center().x, view.top() + 50.0));
    let saved = h.state().win.layout.clone();
    let t = rect_of(h.state(), Pane::Timeline).unwrap();
    assert!(t.bottom() <= rect_of(h.state(), Pane::View).unwrap().top() + 1.0);
    assert!(
        (t.height() - timeline_ui::band_min_height()).abs() < 4.0,
        "タイムラインの高さのまま: {t:?}"
    );

    // 書いた配置で開き直すと、同じ並び
    let json = serde_json::to_string(&saved).unwrap();
    let back: Layout = serde_json::from_str(&json).unwrap();
    let mut ws = Workspace::new(reference_document(), None, kmb_bundle::modules());
    *ws.layout_mut() = back;
    let h2 = harness_with(ws);
    let t2 = rect_of(h2.state(), Pane::Timeline).unwrap();
    assert!((t2.top() - t.top()).abs() < 2.0 && (t2.height() - t.height()).abs() < 2.0);
    assert!(!h2.state().dock_is_default());
    drop(h2);

    // 3D ビューの無い並び・読めない JSON は使わない
    assert!(dock_from_json(&serde_json::json!({"surfaces": []})).is_none());
    assert!(dock_from_json(&serde_json::json!("broken")).is_none());
    // 最初の画面の並びを捨て、壊れた並びを書いた所から作り直させる(開いた時と同じ道)
    h.state_mut().reset_dock();
    h.state_mut().layout_mut().dock = Some(serde_json::json!({"nonsense": 1}));
    h.run();
    assert!(rect_of(h.state(), Pane::View).is_some());
    assert!(rect_of(h.state(), Pane::Browser).is_some());
    // 覚える形には、描くたびに変わる四角を入れない
    let shape = dock_shape(h.state().docking.tree.as_ref().unwrap())
        .unwrap()
        .to_string();
    assert!(!shape.contains("\"rect\""), "{shape}");
}

#[test]
fn outer_windows_come_back_on_reset_and_when_closed() {
    // 順に確かめる:
    // - 開いた時から外の窓の部品は、並びを既定に戻すと戻る(元 reset_brings_outer_windows_back)
    // - 見出しの右クリックでアプリの外の窓に出し、閉じると元の場所へ戻る
    //   (元 a_pane_goes_out_to_its_own_window_and_comes_back_when_closed)
    let mut ws = Workspace::new(reference_document(), None, kmb_bundle::modules());
    ws.layout_mut()
        .outer
        .insert(Pane::Panel, [100.0, 100.0, 320.0, 600.0]);
    let mut h = harness_with(ws);
    assert!(
        rect_of(h.state(), Pane::Panel).is_none(),
        "開いた時から外の窓"
    );
    h.state_mut().reset_dock();
    h.run();
    h.run();
    assert!(h.state().win.layout.outer.is_empty());
    assert!(rect_of(h.state(), Pane::Panel).is_some());
    assert!(h.state().dock_is_default());

    // 既定に戻った並びから、ブラウザを外の窓へ
    let before = rect_of(h.state(), Pane::Browser).unwrap();
    // ブラウザの見出しの右クリックの「アプリの外の窓に出す」
    let at = tab_pos(h.state(), Pane::Browser);
    right_click(&mut h, at);
    h.get_by_label("アプリの外の窓に出す").click();
    h.run();
    h.run();
    let ws = h.state();
    assert!(rect_of(ws, Pane::Browser).is_none(), "並びから外れる");
    let r = ws.win.layout.outer[&Pane::Browser];
    assert!(r[2] >= 220.0 && r[3] >= 160.0, "今の大きさで出す: {r:?}");
    assert!(!ws.dock_is_default());
    // 試験の画面は窓を分けられないので、中の窓で出る(中身はブラウザ)
    h.get_by_label("ブラウザを隠す");
    // 外の窓を閉じると、元の場所へ戻る(開き直しても位置が残るよう、閉じるまでは layout に書いてある)
    h.state_mut().pop_in(Pane::Browser);
    h.run();
    h.run();
    let after = rect_of(h.state(), Pane::Browser).unwrap();
    assert!(
        (after.left() - before.left()).abs() < 2.0 && (after.width() - before.width()).abs() < 2.0,
        "元の場所に戻る: {before:?} → {after:?}"
    );
    assert!(h.state().win.layout.outer.is_empty());
}

#[test]
fn the_shared_workspace_knows_itself() {
    let ws = Workspace::new(reference_document(), None, kmb_bundle::modules());
    let shared = ws.into_shared();
    let inner = shared.lock().unwrap();
    let back = inner.win.shared.as_ref().and_then(|w| w.upgrade()).unwrap();
    assert!(std::sync::Arc::ptr_eq(&back, &shared));
}

#[test]
fn the_timeline_becomes_a_vertical_list_when_put_at_a_side() {
    // タイムラインの見出しを引いて 3D ビューの左の端へ落とすと、縦長の所になり、上から古い順の
    // 縦の一覧で出る(幅は縦の一覧の幅に合わせ、半分ずつにしない)。既定に戻すと横に戻る
    let mut h = harness();
    assert!(!h.state().win.timeline_vertical);
    let view = rect_of(h.state(), Pane::View).unwrap();
    let from = tab_pos(h.state(), Pane::Timeline);
    drag(
        &mut h,
        from,
        egui::pos2(view.left() + 15.0, view.center().y),
    );
    h.run();
    h.run();
    let t = rect_of(h.state(), Pane::Timeline).unwrap();
    assert!(t.height() > t.width(), "縦長: {t:?}");
    assert!((t.width() - 240.0).abs() < 20.0, "縦の一覧の幅: {t:?}");
    assert!(h.state().win.timeline_vertical);
    // 行は上から古い順(機能の名前の行が縦に並ぶ)
    let data = h.state().cur.work.doc().data().clone();
    let first = &data.features[0].name;
    let last = &data.features[data.features.len() - 1].name;
    let in_timeline = |name: &str| {
        h.get_all_by_label(name)
            .map(|n| n.rect())
            .find(|r| t.contains(r.center()))
            .unwrap()
    };
    let (a, b) = (in_timeline(first), in_timeline(last));
    assert!(a.top() < b.top(), "{a:?} {b:?}");
    assert!((a.left() - b.left()).abs() < 1.0, "同じ列: {a:?} {b:?}");
    // 今の位置の印は行の間にある横の棒
    let m = h.get_by_label("今の位置の印").rect();
    assert!(m.width() > m.height(), "{m:?}");
    h.state_mut().reset_dock();
    h.run();
    h.run();
    assert!(!h.state().win.timeline_vertical);
}

#[test]
fn a_pane_dropped_outside_the_main_window_goes_out_and_comes_back_when_closed() {
    // ブラウザの見出しを引いて本体の窓の外(右の外)で離すと外の窓に出て、閉じると引く前の場所へ戻る
    let mut h = harness();
    let before = rect_of(h.state(), Pane::Browser).unwrap();
    let from = tab_pos(h.state(), Pane::Browser);
    drag(&mut h, from, egui::pos2(1500.0, 300.0));
    h.run();
    assert!(h.state().win.layout.outer.contains_key(&Pane::Browser));
    assert!(
        rect_of(h.state(), Pane::Browser).is_none(),
        "並びから外れる"
    );
    h.state_mut().pop_in(Pane::Browser);
    h.run();
    h.run();
    let back = rect_of(h.state(), Pane::Browser).unwrap();
    assert!(
        (back.left() - before.left()).abs() < 2.0,
        "{back:?} {before:?}"
    );
    assert!(
        (back.width() - before.width()).abs() < 2.0,
        "{back:?} {before:?}"
    );
}

/// 採用分 10-24 u10(試験だけ): 部品のタブを 3D のビューの形の上へ引いている間、下の形は指した時の
/// ように光らない(選ばれそうに見えない)。引かずに指せば光る。
#[test]
fn dragging_a_tab_over_the_shape_does_not_light_it() {
    let mut h = harness();
    let view = rect_of(h.state(), Pane::View).unwrap();
    let over = view.center();
    // 引かずに形の真ん中を指すと、指した面がある(ブラウザのボディにも印)
    h.event(egui::Event::PointerMoved(over));
    h.run_steps(3);
    assert!(
        h.state().cur.hover_feature.is_some(),
        "形の真ん中を指している"
    );
    h.event(egui::Event::PointerMoved(
        view.left_top() + egui::vec2(5.0, 5.0),
    ));
    h.run_steps(3);
    // タイムラインのタブを押して、形の真ん中まで引く(まだ離さない)
    let from = tab_pos(h.state(), Pane::Timeline);
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
        let p = from + (over - from) * (k as f32 / 12.0);
        h.event(egui::Event::PointerMoved(p));
        h.step();
        assert!(
            h.state().cur.hover_feature.is_none() && h.state().cur.hover_kind.is_none(),
            "引いている間は形を光らせない({p:?})"
        );
    }
    // 元の所で離して並びを変えずに終える
    h.event(egui::Event::PointerMoved(from));
    h.step();
    h.event(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(4);
}
