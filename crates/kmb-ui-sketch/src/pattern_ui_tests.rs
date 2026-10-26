//! 後から直せるパターン(網羅版 C15・C16)を、人の操作(クリック、キー)で確かめる。

use crate::Tool;
use crate::tests::*;
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kmb_sketch::{Entity, PatternDef};
use kmb_ui::Workspace;

fn circles(h: &Harness<'static, Workspace>) -> usize {
    count_of(&current_sketch(h.state()), |e| {
        matches!(e, Entity::Circle { .. })
    })
}

/// 円を 1 つ描き、それを直線に 4 つ並べる(先に数を打ち、間隔はクリックで決める)。
fn four_circles_in_a_row() -> Harness<'static, Workspace> {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Circle);
    h.run();
    click_at(&mut h, [1.19, 11.15]);
    click_at(&mut h, [2.23, 11.15]);
    press_key(&mut h, egui::Key::Escape);
    press_key(&mut h, egui::Key::Escape);
    // 円を選ぶ(円周をクリック)
    click_at(&mut h, [2.23, 11.15]);
    assert_eq!(editor(h.state_mut()).editor.selection.len(), 1);
    editor(h.state_mut()).set_tool(Tool::LinearPattern);
    h.run();
    click_at(&mut h, [1.19, 11.15]);
    h.event(egui::Event::Text("4".into()));
    h.run();
    click_at(&mut h, [4.32, 11.15]);
    press_key(&mut h, egui::Key::Escape);
    press_key(&mut h, egui::Key::Escape);
    assert_eq!(circles(&h), 4);
    assert_eq!(current_sketch(h.state()).patterns.len(), 1);
    h
}

/// 式の欄をクリックして値を打ち直し Enter。
fn retype(h: &mut Harness<'static, Workspace>, label: &str, nth: usize, value: &str) {
    let rect = {
        let nodes: Vec<_> = h
            .query_all_by_role_and_label(Role::TextInput, label)
            .collect();
        assert!(
            nodes.len() > nth,
            "欄「{label}」が {} 個しかない",
            nodes.len()
        );
        nodes[nth].rect()
    };
    let c = rect.center();
    quick_click_px(h, c);
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    type_text(h, value);
    press_enter(h);
}

#[test]
fn choosing_a_copy_shows_the_pattern_and_count_can_be_changed_later() {
    let mut h = four_circles_in_a_row();
    // 3 つ目の写しの円周をクリックして選ぶと、パターンの欄が出る
    // (円周のすぐ外側を押す。円周の真上だと円の中の領域とも重なり、重なりの一覧が出ることがある)
    click_at(&mut h, [8.52, 11.15]);
    h.run();
    assert!(h.query_by_label("矩形状パターン(P1)").is_some());
    // 方向 1 の数を 6 に打ち直す
    retype(&mut h, "数", 0, "6");
    h.run();
    assert_eq!(circles(&h), 6);
    let s = current_sketch(h.state());
    match &s.patterns[0].def {
        PatternDef::Rectangular(r) => assert_eq!(r.directions[0].count, 6),
        d => panic!("矩形状のはず: {d:?}"),
    }
}

#[test]
fn suppressing_a_copy_removes_it_and_restoring_brings_it_back() {
    let mut h = four_circles_in_a_row();
    click_at(&mut h, [8.52, 11.15]);
    h.run();
    h.get_by_label("選んだ写しを抑制").click();
    h.run();
    h.run();
    assert_eq!(circles(&h), 3);
    // 残った写しを選ぶと「抑制を全部戻す」が出る
    click_at(&mut h, [5.39, 11.15]);
    h.run();
    h.get_by_label("抑制を全部戻す").click();
    h.run();
    h.run();
    assert_eq!(circles(&h), 4);
}

#[test]
fn releasing_keeps_the_copies_and_deleting_removes_them() {
    let mut h = four_circles_in_a_row();
    click_at(&mut h, [5.39, 11.15]);
    h.run();
    h.get_by_label("パターンを解除").click();
    h.run();
    h.run();
    assert_eq!(circles(&h), 4);
    assert!(current_sketch(h.state()).patterns.is_empty());

    let mut h = four_circles_in_a_row();
    click_at(&mut h, [5.39, 11.15]);
    h.run();
    h.get_by_label("パターンを消す").click();
    h.run();
    h.run();
    assert_eq!(circles(&h), 1);
    assert!(current_sketch(h.state()).patterns.is_empty());
}

#[test]
fn circular_pattern_count_and_angle_can_be_changed_later() {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Circle);
    h.run();
    click_at(&mut h, [6.4, 11.15]);
    click_at(&mut h, [7.18, 11.15]);
    press_key(&mut h, egui::Key::Escape);
    press_key(&mut h, egui::Key::Escape);
    click_at(&mut h, [7.18, 11.15]);
    editor(h.state_mut()).set_tool(Tool::CircularPattern);
    h.run();
    click_at(&mut h, [1.19, 11.15]);
    h.event(egui::Event::Text("3".into()));
    h.run();
    press_enter(&mut h);
    press_key(&mut h, egui::Key::Escape);
    press_key(&mut h, egui::Key::Escape);
    assert_eq!(circles(&h), 3);
    // 元の円を選ぶと円形状パターンの欄
    click_at(&mut h, [7.18, 11.15]);
    h.run();
    assert!(h.query_by_label("円形状パターン(P1)").is_some());
    retype(&mut h, "数", 0, "5");
    h.run();
    assert_eq!(circles(&h), 5);
    retype(&mut h, "角度", 0, "90");
    h.run();
    match &current_sketch(h.state()).patterns[0].def {
        PatternDef::Circular(c) => {
            assert_eq!(c.count, 5);
            assert!((c.angle.to_degrees() - 90.0).abs() < 1e-6, "{}", c.angle);
        }
        d => panic!("円形状のはず: {d:?}"),
    }
}

#[test]
fn dragging_the_original_brings_the_copies_along() {
    let mut h = four_circles_in_a_row();
    let centers = |h: &Harness<'static, Workspace>| -> Vec<[f64; 2]> {
        let s = current_sketch(h.state());
        let sol = solved(h.state());
        s.entities
            .iter()
            .filter_map(|e| match e {
                Entity::Circle { center, .. } => Some(sol.points[center]),
                _ => None,
            })
            .collect()
    };
    let before = centers(&h);
    // 元の円の中心を上へ引く
    drag_on(&mut h, [1.19, 11.15], [1.19, 14.28], egui::Modifiers::NONE);
    let after = centers(&h);
    assert_eq!(after.len(), 4);
    let dy = after[0][1] - before[0][1];
    assert!(dy.abs() > 1.0, "元が動いていない: {before:?} → {after:?}");
    for (a, b) in before.iter().zip(&after) {
        assert!(
            (b[1] - a[1] - dy).abs() < 1e-6,
            "写しが付いて来ない: {before:?} → {after:?}"
        );
    }
}

/// 数を入力している間は見本だけを更新し、Enter で文書へ入れる。
#[test]
fn dragging_the_count_shows_the_result_before_release() {
    let mut h = four_circles_in_a_row();
    click_at(&mut h, [8.52, 11.15]);
    h.run();
    let a = h
        .query_all_by_role_and_label(Role::TextInput, "数")
        .next()
        .expect("数の欄")
        .rect()
        .center();
    quick_click_px(&mut h, a);
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    type_text(&mut h, "3 * 2");
    h.run();
    let shown = editor(h.state_mut())
        .editor
        .pattern_ghost
        .as_ref()
        .map(|(_, s, _)| count_of(s, |e| matches!(e, Entity::Circle { .. })));
    assert_eq!(shown, Some(6));
    assert_eq!(circles(&h), 4);
    press_enter(&mut h);
    h.run();
    assert_eq!(circles(&h), 6);
    assert!(editor(h.state_mut()).editor.pattern_ghost.is_none());
    let data = serde_json::to_value(current_sketch(h.state())).unwrap();
    assert_eq!(
        data["patterns"][0]["expressions"]["directions"][0]["count"],
        "3 * 2"
    );
}

/// 台本からの入力も、欄の式と文書の単位を使う。
#[test]
fn a_script_enters_pattern_expressions_using_the_document_unit() {
    let mut h = four_circles_in_a_row();
    click_at(&mut h, [8.52, 11.15]);
    h.run();
    let batch = h
        .state_mut()
        .run_script(include_str!("../tests/ui/pattern-expressions.jcs"))
        .unwrap();
    let start = std::time::Instant::now();
    loop {
        h.step();
        if let Some(result) = h.state_mut().script_result(batch) {
            assert!(result.ok, "{}", result.report());
            break;
        }
        assert!(start.elapsed().as_secs() < 15, "台本が終わらない");
    }
    assert_eq!(circles(&h), 5);
    match &current_sketch(h.state()).patterns[0].def {
        PatternDef::Rectangular(r) => assert!((r.directions[0].distance - 25.4).abs() < 1e-9),
        other => panic!("{other:?}"),
    }
    kmb_core::set_display_unit(kmb_core::LengthUnit::Mm);
}
