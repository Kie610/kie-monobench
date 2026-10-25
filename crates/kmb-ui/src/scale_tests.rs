//! 字とアイコンを大きくした時の崩れの試験: 字とアイコンを 250 % にしても、ビューの周り・
//! タイムライン・編集パネル・文書のタブの部品が重ならず、枠に収まる(固定の px の枠に倍率を掛けた
//! アイコンと字を置くと、250 % で重なる)。

use super::*;
use crate::layout::{Brightness, Space};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use kmb_samples::reference_document;

fn big(size: egui::Vec2, k: f32) -> Harness<'static, Workspace> {
    let mut ws = Workspace::new(reference_document(), None, kmb_bundle::modules());
    ws.layout_mut().brightness = Brightness::Light;
    ws.layout_mut().appearance.text_scale = k;
    ws.layout_mut().appearance.icon_scale = k;
    let mut h = Harness::builder()
        .with_size(size)
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    // ナビの帯などは前のこまの幅で置き直すので、何こまか回す
    for _ in 0..4 {
        h.run();
    }
    h
}

fn rects(h: &Harness<'static, Workspace>, label: &str) -> Vec<egui::Rect> {
    h.query_all_by_label(label).map(|n| n.rect()).collect()
}

#[test]
fn the_view_the_timeline_and_the_panel_keep_apart_at_the_largest_size() {
    let mut h = big(egui::vec2(1920.0, 1080.0), 2.5);
    // 右の様子の欄に全体の数(「9 機能」)を出すため、選びを外しておく
    h.state_mut().clear_selection();
    h.run();
    h.run();

    // 選ぶ種類はツールバーの「選択」の 1 か所だけ、表示の形は下の帯の「表示 ▾」だけ
    assert_eq!(rects(&h, "頂点").len(), 1, "選ぶ種類が 2 か所にある");
    assert!(
        rects(&h, "表示設定").is_empty(),
        "右上に表示設定が残っている"
    );

    // 右上: 視点の軸の玉は、全画面と等角のボタンの行の下(重ならない)
    let gizmo = h.get_by_label("視点の軸").rect();
    for b in ["全画面表示", "等角"] {
        let r = h.get_by_label(b).rect();
        assert!(
            !gizmo.intersects(r),
            "軸の玉が「{b}」に重なる: {gizmo:?} {r:?}"
        );
    }

    // タイムライン: 札はアイコンより広く、隣の札と重ならない
    let back = h.get_by_label("ロールバックの棒を先頭へ").rect();
    let data = h.state().cur.work.doc().data().clone();
    let mut chips: Vec<egui::Rect> = data
        .features
        .iter()
        .filter_map(|f| {
            h.query_all_by_label(&f.name)
                .map(|n| n.rect())
                .find(|r| r.left() > back.right() && (r.center().y - back.center().y).abs() < 60.0)
        })
        .collect();
    assert!(
        chips.len() >= 3,
        "タイムラインの札が見つからない: {chips:?}"
    );
    chips.sort_by(|a, b| a.left().total_cmp(&b.left()));
    let icon = theme::icon_px(18.0);
    for c in &chips {
        assert!(
            c.width() >= icon + 8.0 && c.height() >= icon + 8.0,
            "札 {c:?} がアイコン {icon} より小さい"
        );
    }
    for w in chips.windows(2) {
        assert!(w[0].right() <= w[1].left() + 0.5, "札が重なる: {w:?}");
    }
    // 右の様子の欄(「9 機能」)は札と同じ高さの真ん中にある(帯の上に寄らない)
    let info = h
        .query_all(egui_kittest::kittest::By::new())
        .find(|n| {
            let a = n.accesskit_node();
            a.label()
                .or(a.value())
                .is_some_and(|t| t.ends_with(" 機能") || t.contains(" 機能 ・"))
        })
        .unwrap_or_else(|| {
            let all: Vec<String> = h
                .query_all(egui_kittest::kittest::By::new())
                .filter_map(|n| n.accesskit_node().label().or(n.accesskit_node().value()))
                .filter(|t| t.contains("機能"))
                .collect();
            panic!("タイムラインの様子の欄が無い: {all:?}")
        })
        .rect();
    let chip = chips[0];
    assert!(
        (info.center().y - chip.center().y).abs() < chip.height() / 2.0,
        "様子の欄 {info:?} が札 {chip:?} の高さから外れる"
    );

    // 編集パネル: 機能を選ぶと見出しの × がアイコンの倍率で大きくなり、見出しの帯に収まる
    let id = data.features.last().unwrap().id;
    h.state_mut().select(id);
    h.run();
    h.run();
    let x = h.get_by_label("編集パネルを隠す").rect();
    assert!(x.width() >= 20.0 * 2.5 - 0.5, "見出しの × が小さい: {x:?}");

    // ブラウザを隠すと出る左の端のつまみは、縦に並べた「ブラウザ」の 4 字が入る高さと幅
    h.state_mut().win.layout.browser_open = false;
    h.run();
    h.run();
    let grip = h.get_by_label("ブラウザを出す").rect();
    let ch = h.ctx.fonts_mut(|f| {
        f.layout_no_wrap("ブ".into(), theme::prop(11.0), egui::Color32::WHITE)
            .size()
    });
    assert!(
        grip.width() >= ch.x + 4.0,
        "つまみの幅 {grip:?} に字 {ch:?} が入らない"
    );
    assert!(
        grip.height() >= 4.0 * ch.y * 0.8,
        "つまみの高さ {grip:?} に 4 字が入らない"
    );
    // つまみはビューの左上の案内と重ならない(案内はつまみの右から)
    h.state_mut().win.layout.browser_open = true;

    // レンダリングと解析では下のタイムラインを出さない。設計へ戻すとまた出る
    for s in [Space::Render, Space::Simulation] {
        h.state_mut().switch_space(s);
        h.run();
        h.run();
        assert!(
            rects(&h, "ロールバックの棒を先頭へ").is_empty(),
            "{s:?} にタイムラインが出ている"
        );
        assert!(h.state().win.layout.timeline_open, "隠す印は変えない");
    }
    h.state_mut().switch_space(Space::Design);
    h.run();
    h.run();
    assert_eq!(rects(&h, "ロールバックの棒を先頭へ").len(), 1);
}

#[test]
fn many_document_tabs_stay_inside_the_top_bar() {
    // 10 枚のタブを開いても、「+」とコマンド検索の欄は重ならずに画面に残る。入らないタブは
    // 「ほかのタブ」の一覧から選べる(狭い 1280 幅の字 100 % と、PC の 4K・200 % に当たる 1920 幅の
    // 字 250 %)。1280 幅の 250 % は上の帯の右の部品だけで幅を越えるので、ここでは見ない
    // (上の帯を収めるのはツールバーを収める直しの方)
    for (w, k) in [(1280.0, 1.0), (1920.0, 2.5)] {
        let mut h = big(egui::vec2(w, 1034.0), k);
        for _ in 0..9 {
            h.state_mut().new_tab();
        }
        h.run();
        h.run();
        assert_eq!(h.state().win.doc_tabs.len(), 10);
        let plus = h.get_by_label("新しいタブ").rect();
        let search = h.get_by_label("コマンド検索").rect();
        assert!(
            plus.right() <= search.left(),
            "{k}: 「+」{plus:?} がコマンド検索 {search:?} に重なる"
        );
        let more = h.get_by_label("ほかのタブ").rect();
        assert!(more.right() <= plus.left() + 0.5, "{k}: {more:?} {plus:?}");
        // 一覧から先頭のタブへ移れる
        h.get_by_label("ほかのタブ").click();
        h.run();
        // 一覧の行(上の帯より下)のいちばん上が先頭のタブ
        let first = h.state().tab_titles()[0].0.clone();
        h.query_all_by_label(&first)
            .filter(|n| n.rect().top() > plus.bottom())
            .min_by(|a, b| a.rect().top().total_cmp(&b.rect().top()))
            .expect("一覧の先頭のタブ")
            .click();
        h.run();
        assert_eq!(h.state().win.doc_tab, 0, "{k}");
    }
    // 字 250 % で 1 枚だけの時も、タブはコマンド検索の欄に重ならない
    let h = big(egui::vec2(1920.0, 1034.0), 2.5);
    let search = h.get_by_label("コマンド検索").rect();
    let plus = h.get_by_label("新しいタブ").rect();
    assert!(plus.right() <= search.left(), "{plus:?} {search:?}");
}
