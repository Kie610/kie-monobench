//! ツールバーを、画板「JC の画面」で Kie と決めた並び(2026-10-21、`board/toolbar.json`)と比べる
//! 試験。まとまりの順・前に出すボタン・絵と役・▾ の一覧を見る。まだ入っていない物は
//! `board/known-diffs.txt` に書く(比べ方は [`crate::board`])。スケッチは kmb-ui-sketch の
//! `the_sketch_toolbar_follows_the_board`。

use crate::Space;
use crate::board::{self, Actual};
use crate::{Workspace, ribbon};
use kmb_samples::{reference_cam_document, reference_document};

/// 作業空間の最初のタブのまとまり(どのまとまりにも属さない「その他」は除く)。
fn actual(ws: &Workspace, space: Space) -> Vec<Actual> {
    let tabs = ws.ribbon_tabs();
    let tab = &tabs[0];
    tab.groups
        .iter()
        .filter(|g| g.name != "その他")
        .map(|g| {
            let front = ribbon::front(space, &tab.name, g, &ws.win.layout);
            Actual {
                name: g.name.clone(),
                front: front
                    .iter()
                    .map(|i| (i.label.clone(), i.icon.clone()))
                    .collect(),
                small: vec![],
                menu: g
                    .items
                    .iter()
                    .filter(|i| !front.iter().any(|f| f.key == i.key))
                    .map(|i| i.label.clone())
                    .collect(),
            }
        })
        .collect()
}

#[test]
fn the_design_toolbar_follows_the_board() {
    let ws = Workspace::new(reference_document(), None, kmb_bundle::modules());
    if let Err(e) = board::check("設計", &actual(&ws, Space::Design)) {
        panic!("{e}");
    }
}

#[test]
fn the_machining_toolbar_follows_the_board() {
    let mut ws = Workspace::new(reference_cam_document(), None, kmb_bundle::modules());
    ws.switch_space(Space::Machining);
    if let Err(e) = board::check("加工", &actual(&ws, Space::Machining)) {
        panic!("{e}");
    }
}

/// 設計と加工のツールバーの道具は、▾ の一覧と「その他」の物も、役の色の変わる部分を持つか 1 層の表にある。
#[test]
fn every_design_and_machining_tool_has_a_role_or_one_layer() {
    let mut tools = vec![];
    for (doc, space) in [
        (reference_document(), Space::Design),
        (reference_cam_document(), Space::Machining),
    ] {
        let mut ws = Workspace::new(doc, None, kmb_bundle::modules());
        ws.switch_space(space);
        for tab in ws.ribbon_tabs() {
            for g in &tab.groups {
                tools.extend(g.items.iter().map(|i| (i.label.clone(), i.icon.clone())));
            }
        }
    }
    let gaps = board::layer_gaps(&tools);
    assert!(gaps.is_empty(), "{}", gaps.join("\n"));
}

/// 既知の差の表の行は、どれも今の差のどれかに当たる(書き損じで何にも当たらない行を残さない)。
#[test]
fn every_known_difference_names_a_space_on_the_board() {
    let all =
        board::known("設計").len() + board::known("加工").len() + board::known("スケッチ").len();
    let lines = include_str!("../board/known-diffs.txt")
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .count();
    assert_eq!(
        all, lines,
        "known-diffs.txt に、設計・加工・スケッチのどれでもない行がある"
    );
}

/// 測るだけ(R17): ツールバーの項目を 1 回組むのにかかる時間と、1 コマの時間。
#[test]
#[ignore = "道具: ツールバーを組む時間を測る(R17)"]
fn measure_ribbon_tabs() {
    let mut ws = Workspace::new(
        kmb_samples::reference_document(),
        None,
        kmb_bundle::modules(),
    );
    ws.wait();
    let t = std::time::Instant::now();
    for _ in 0..100 {
        std::hint::black_box(ws.ribbon_tabs());
    }
    let one = t.elapsed().as_secs_f64() * 1000.0 / 100.0;
    let t = std::time::Instant::now();
    for _ in 0..100 {
        std::hint::black_box(ws.actions());
    }
    let acts = t.elapsed().as_secs_f64() * 1000.0 / 100.0;
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    let t = std::time::Instant::now();
    for _ in 0..20 {
        h.step();
    }
    let frame = t.elapsed().as_secs_f64() * 1000.0 / 20.0;
    eprintln!("ツールバー 1 回 {one:.2} ms(うち操作の一覧 {acts:.2} ms)、1 コマ {frame:.1} ms");
}

/// 足せる機能の一覧の控え(10-23 v8)は、文書が変わると作り直す: ボディのある文書ではミラーを
/// 足せ、ボディを作る押し出しを抑制すると足せない。
#[test]
fn the_addable_list_is_rebuilt_when_the_document_changes() {
    let doc = {
        let mut b = kmb_samples::sample::DocBuilder::new();
        let mut s = kmb_samples::sample::SketchBuilder::xy();
        let c = s.point(0.0, 0.0);
        s.rectangle(c, "10 mm", "10 mm", [0.0, 0.0, 10.0, 10.0]);
        let sk = b.sketch("スケッチ1", s);
        b.feature(
            kmb_cad_extrude::EXTRUDE,
            "押し出し1",
            kmb_cad_extrude::Extrude {
                profile: Some(sk),
                distance: "5 mm".into(),
                ..Default::default()
            },
        );
        b.finish("箱")
    };
    let extrude = doc.data().features[1].id;
    let mut ws = Workspace::new(doc, None, kmb_bundle::modules());
    ws.wait();
    let why = |ws: &Workspace| {
        ws.addable(kmb_core::category::SOLID)
            .into_iter()
            .find(|(t, _, _)| t == "solid.mirror@1")
            .unwrap_or_else(|| panic!("{:?}", ws.addable(kmb_core::category::SOLID)))
            .2
    };
    assert_eq!(why(&ws), None, "ボディがあれば足せる");
    assert_eq!(why(&ws), None, "控えから同じ答え");
    assert!(ws.set_suppressed(extrude, true));
    ws.wait();
    assert!(why(&ws).is_some(), "ボディが無くなると足せない");
}
