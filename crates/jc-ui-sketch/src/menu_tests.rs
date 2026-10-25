//! 右クリックのメニュー・キー・オフセットと測る道具を、人の操作(クリック、キー、ホイール)で確かめる。

use crate::Tool;
use crate::tests::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use jc_sketch::{ConstraintKind, Entity, EntityId, Sketch};
use jc_ui::{FeatureEditor, Workspace};

fn lines(s: &Sketch) -> Vec<EntityId> {
    s.entities
        .iter()
        .filter_map(|e| match e {
            Entity::Line { id, .. } => Some(*id),
            _ => None,
        })
        .collect()
}

fn is_construction(s: &Sketch, id: EntityId) -> bool {
    matches!(
        s.entity(id),
        Some(Entity::Line {
            construction: true,
            ..
        })
    )
}

/// 線の点を囲む箱の幅と高さ。
fn size_of(s: &Sketch, ids: &[EntityId]) -> [f64; 2] {
    let values = jc_sketch::dimension_values(s, &|_| None).unwrap();
    let sol = jc_sketch::solve(s, &values).unwrap();
    let pts: Vec<[f64; 2]> = ids
        .iter()
        .filter_map(|id| match s.entity(*id) {
            Some(Entity::Line { a, b, .. }) => Some([sol.points[a], sol.points[b]]),
            _ => None,
        })
        .flatten()
        .collect();
    let span = |i: usize| {
        let lo = pts.iter().map(|p| p[i]).fold(f64::MAX, f64::min);
        let hi = pts.iter().map(|p| p[i]).fold(f64::MIN, f64::max);
        hi - lo
    };
    [span(0), span(1)]
}

pub(crate) fn click_with(h: &mut Harness<'static, Workspace>, p: [f64; 2], m: egui::Modifiers) {
    let pos = screen(h, p);
    click_with_px(h, pos, m);
}

pub(crate) fn click_with_px(
    h: &mut Harness<'static, Workspace>,
    pos: egui::Pos2,
    m: egui::Modifiers,
) {
    h.event(egui::Event::ModifiersChanged(m));
    h.hover_at(pos);
    h.run();
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: m,
        });
        // 押して離すまでを 1 コマずつ(run は 1 コマを 0.25 秒と数えるので、何コマも回ると
        // 長押しになってクリックと見なされない)
        h.step();
    }
    h.run();
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    h.run();
}

fn hover(h: &mut Harness<'static, Workspace>, p: [f64; 2]) {
    move_to(h, p);
    h.run();
}

#[test]
fn the_menu_in_a_tool_makes_the_typed_shape_with_ok() {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Rectangle);
    h.run();
    click_at(&mut h, [1.19, 16.36]);
    hover(&mut h, [11.62, 8.54]);
    type_text(&mut h, "40");
    right_click_at(&mut h, [11.62, 8.54]);
    // 道具の中: OK・キャンセル・補助線で描く
    h.get_by_label("キャンセル(Esc)");
    h.get_by_label("補助線で描く(X)");
    h.get_by_label("OK(Enter)").click();
    h.run();
    h.run();
    let s = current_sketch(h.state());
    assert_eq!(lines(&s).len(), 4, "打った幅で矩形を作る");
    assert!(
        s.constraints
            .iter()
            .any(|c| matches!(&c.kind, ConstraintKind::Length { value, .. } if value == "40 mm"))
    );
    assert_eq!(
        editor(h.state_mut()).editor.tool,
        Tool::Select,
        "OK で道具を終える"
    );
}

#[test]
fn x_draws_construction_lines_and_toggles_the_selection() {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Line);
    h.run();
    // 描く前に X: 次に描く線が補助線になる
    press_key(&mut h, egui::Key::X);
    click_at(&mut h, [-4.03, 0.72]);
    click_at(&mut h, [6.4, 4.89]);
    let s = current_sketch(h.state());
    let first = lines(&s)[0];
    assert!(is_construction(&s, first), "X の後に描いた線は補助線");
    // もう一度 X で戻す(描いている途中でも、欄に打っていなければ効く)
    press_key(&mut h, egui::Key::X);
    click_at(&mut h, [16.83, 6.98]);
    let s = current_sketch(h.state());
    let second = *lines(&s).iter().find(|l| **l != first).unwrap();
    assert!(!is_construction(&s, second));
    press_key(&mut h, egui::Key::Escape);
    press_key(&mut h, egui::Key::Escape);
    // 選ぶ道具: 選んだ線を X で切り替える
    click_at(&mut h, [11.62, 5.93]);
    assert!(editor(h.state_mut()).editor.selection.contains(&second));
    press_key(&mut h, egui::Key::X);
    assert!(is_construction(&current_sketch(h.state()), second));
}

#[test]
fn right_clicking_a_line_selects_it_and_offers_fix_and_horizontal() {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Line);
    h.run();
    click_at(&mut h, [-4.03, 0.72]);
    click_at(&mut h, [6.4, 5.93]);
    press_key(&mut h, egui::Key::Escape);
    press_key(&mut h, egui::Key::Escape);
    let line = lines(&current_sketch(h.state()))[0];
    assert!(editor(h.state_mut()).editor.selection.is_empty());
    // 選んでいない線の上で右クリック: その線を選んでメニューを出す
    right_click_at(&mut h, [1.19, 3.33]);
    assert!(editor(h.state_mut()).editor.selection.contains(&line));
    h.get_by_label("スケッチ寸法(D)");
    h.get_by_label("標準/補助線(X)");
    h.get_by_label("オフセット(O)");
    h.get_by_label("固定する").click();
    h.run();
    let fixes = |s: &Sketch| {
        s.constraints
            .iter()
            .filter(|c| matches!(c.kind, ConstraintKind::Fix { .. }))
            .count()
    };
    assert_eq!(fixes(&current_sketch(h.state())), 2, "線の両端を固定");
    right_click_at(&mut h, [1.19, 3.33]);
    h.get_by_label("固定解除").click();
    h.run();
    assert_eq!(fixes(&current_sketch(h.state())), 0);
    right_click_at(&mut h, [1.19, 3.33]);
    h.get_by_label("水平にする").click();
    h.run();
    assert!(
        current_sketch(h.state())
            .constraints
            .iter()
            .any(|c| matches!(c.kind, ConstraintKind::Horizontal { line: l } if l == line))
    );
}

#[test]
fn the_empty_menu_undoes_fits_and_offers_recent_tools() {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Circle);
    h.run();
    click_at(&mut h, [1.19, 11.15]);
    click_at(&mut h, [3.27, 11.15]);
    press_key(&mut h, egui::Key::Escape);
    let circles = |h: &Harness<'static, Workspace>| {
        current_sketch(h.state())
            .entities
            .iter()
            .filter(|e| matches!(e, Entity::Circle { .. }))
            .count()
    };
    assert_eq!(circles(&h), 1);
    // ホイールでカーソルの所へ寄せて拡大する
    hover(&mut h, [22.04, 18.97]);
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        phase: egui::TouchPhase::Move,
        delta: egui::vec2(0.0, 120.0),
        modifiers: egui::Modifiers::NONE,
    });
    for _ in 0..20 {
        h.run();
    }
    let zoomed = editor(h.state_mut()).view.zoom;
    assert!(zoomed > 1.05, "拡大した: {zoomed}");
    right_click_at(&mut h, [22.04, 4.31]);
    // 最近使った道具に円が出る
    h.get_by_label("中心と直径の円(C)");
    h.get_by_label("やり直し(Ctrl+Y)");
    h.get_by_label("すべて表示(ホイール二度押し)").click();
    h.run();
    let v = &editor(h.state_mut()).view;
    assert_eq!(
        (v.zoom, v.pan),
        (1.0, egui::Vec2::ZERO),
        "全体が収まる表示に戻す"
    );
    right_click_at(&mut h, [22.04, 0.72]);
    h.get_by_label("元に戻す(Ctrl+Z)").click();
    h.run();
    h.run();
    assert_eq!(circles(&h), 0, "メニューから取り消せる");
}

#[test]
fn the_wheel_zooms_about_the_cursor() {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Point);
    h.run();
    click_at(&mut h, [22.04, 18.97]);
    press_key(&mut h, egui::Key::Escape);
    let point = current_sketch(h.state()).entities[0].id();
    hover(&mut h, [22.04, 18.97]);
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        phase: egui::TouchPhase::Move,
        delta: egui::vec2(0.0, 300.0),
        modifiers: egui::Modifiers::NONE,
    });
    for _ in 0..20 {
        h.run();
    }
    assert!(editor(h.state_mut()).view.zoom > 1.3);
    // 拡大した後も、点はカーソルの下にある
    click_at(&mut h, [22.04, 18.97]);
    assert!(editor(h.state_mut()).editor.selection.contains(&point));
}

#[test]
fn the_offset_tool_follows_the_cursor_side_and_takes_a_typed_distance() {
    let mut h = sketch_with_rectangle();
    let old = lines(&current_sketch(h.state()));
    let size = size_of(&current_sketch(h.state()), &old);
    press_key(&mut h, egui::Key::O);
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::Offset);
    // 上の辺をクリック → つながった 4 本をずらす。外(上)へカーソルを出して 5 を打つ
    click_at(&mut h, [6.4, 16.36]);
    assert_eq!(editor(h.state_mut()).editor.offset.chain.len(), 4);
    hover(&mut h, [6.4, 17.92]);
    type_text(&mut h, "5");
    press_enter(&mut h);
    let s = current_sketch(h.state());
    let new: Vec<EntityId> = lines(&s).into_iter().filter(|l| !old.contains(l)).collect();
    assert_eq!(new.len(), 4);
    let big = size_of(&s, &new);
    assert!((big[0] - size[0] - 10.0).abs() < 1e-6, "{big:?} {size:?}");
    let e = &editor(h.state_mut()).editor;
    assert_eq!(e.tool, Tool::Select, "作ったら選ぶ道具へ");
    assert_eq!(e.selection.len(), 4, "新しい線を選ぶ");
    // 内側へ: 選んでから O、カーソルを内へ(辺の真ん中には拘束の印があるので、ずらして押す)
    click_with(&mut h, [3.8, 16.36], egui::Modifiers::NONE);
    assert_eq!(editor(h.state_mut()).editor.selection.len(), 1);
    press_key(&mut h, egui::Key::O);
    hover(&mut h, [6.4, 15.32]);
    type_text(&mut h, "1");
    press_enter(&mut h);
    let s = current_sketch(h.state());
    let all = lines(&s);
    assert_eq!(all.len(), 12);
    let inner: Vec<EntityId> = all
        .iter()
        .copied()
        .filter(|l| !old.contains(l) && !new.contains(l))
        .collect();
    let small = size_of(&s, &inner);
    assert!(
        (size[0] - small[0] - 2.0).abs() < 1e-6,
        "{small:?} {size:?}"
    );
}

#[test]
fn the_offset_tool_can_go_both_ways_from_its_menu() {
    let mut h = sketch_with_rectangle();
    let old = lines(&current_sketch(h.state()));
    let size = size_of(&current_sketch(h.state()), &old);
    press_key(&mut h, egui::Key::O);
    click_at(&mut h, [6.4, 16.36]);
    right_click_at(&mut h, [6.4, 17.92]);
    // 右クリックのメニュー(パネルにも同じ選びがある)
    h.get_all_by_label("対称").last().unwrap().click();
    h.run();
    hover(&mut h, [6.4, 17.92]);
    // 矩形は約 10.4×7.8 mm。内へ 3 mm なら裏返らない
    type_text(&mut h, "6");
    press_enter(&mut h);
    let s = current_sketch(h.state());
    let new: Vec<EntityId> = lines(&s).into_iter().filter(|l| !old.contains(l)).collect();
    assert_eq!(new.len(), 8, "両側へ半分ずつ");
    let widths: Vec<f64> = [&new[..4], &new[4..]]
        .iter()
        .map(|ids| size_of(&s, ids)[0])
        .collect();
    let mut d: Vec<f64> = widths.iter().map(|w| w - size[0]).collect();
    d.sort_by(f64::total_cmp);
    assert!(
        (d[0] + 6.0).abs() < 1e-6 && (d[1] - 6.0).abs() < 1e-6,
        "{d:?}"
    );
}

#[test]
fn the_measure_tool_tells_the_length_without_adding_a_dimension() {
    let mut h = sketch_with_rectangle();
    let before = current_sketch(h.state()).constraints.len();
    press_key(&mut h, egui::Key::I);
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::Measure);
    click_at(&mut h, [6.4, 16.36]);
    let m = editor(h.state_mut()).editor.measured.clone().unwrap();
    assert!(m.contains("長さ"), "{m}");
    // 2 本目の辺: 線分 2 本の距離か角度
    click_at(&mut h, [1.19, 12.45]);
    let m = editor(h.state_mut()).editor.measured.clone().unwrap();
    assert!(m.contains("測った値"), "{m}");
    assert_eq!(current_sketch(h.state()).constraints.len(), before);
    // 右下に選んだ物の数と長さ
    let s = current_sketch(h.state());
    let sol = solved(h.state());
    let info = crate::view::selection_info(&s, &sol, &editor(h.state_mut()).editor).unwrap();
    assert!(info.starts_with("2 スケッチ曲線 | 長さ: "), "{info}");
}

#[test]
fn shift_only_adds_and_ctrl_toggles() {
    let mut h = sketch_with_rectangle();
    click_at(&mut h, [6.4, 16.36]);
    let top = *editor(h.state_mut())
        .editor
        .selection
        .iter()
        .next()
        .unwrap();
    click_with(&mut h, [6.4, 16.36], egui::Modifiers::SHIFT);
    assert!(
        editor(h.state_mut()).editor.selection.contains(&top),
        "Shift は足すだけ"
    );
    // 辺の真ん中には拘束の印があるので、少しずらして押す
    click_with(&mut h, [3.8, 8.54], egui::Modifiers::SHIFT);
    assert_eq!(editor(h.state_mut()).editor.selection.len(), 2);
    click_with(&mut h, [6.4, 16.36], egui::Modifiers::COMMAND);
    let e = &editor(h.state_mut()).editor;
    assert!(!e.selection.contains(&top), "Ctrl は足し外し");
    assert_eq!(e.selection.len(), 1);
}

#[test]
fn a_tool_that_cannot_make_the_shape_says_why() {
    let mut h = sketch_with_rectangle();
    editor(h.state_mut()).set_tool(Tool::TangentArc);
    h.run();
    click_at(&mut h, [22.04, 0.72]);
    let n = editor(h.state_mut())
        .editor
        .notice
        .clone()
        .unwrap_or_default();
    assert!(n.contains("線分か円弧の端"), "{n}");
    // 片方の値だけ打った矩形も作る(打っていない方はカーソルで決まる。網羅版 L7)
    editor(h.state_mut()).set_tool(Tool::Rectangle);
    h.run();
    click_at(&mut h, [16.83, 21.57]);
    hover(&mut h, [22.04, 18.45]);
    type_text(&mut h, "30");
    press_enter(&mut h);
    assert_eq!(lines(&current_sketch(h.state())).len(), 8);
}

#[test]
fn the_check_button_beside_the_fields_makes_the_shape() {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Line);
    h.run();
    click_at(&mut h, [-4.03, 0.72]);
    hover(&mut h, [2.23, 0.72]);
    type_text(&mut h, "30");
    h.get_by_label("✓").click();
    h.run();
    h.run();
    let s = current_sketch(h.state());
    assert_eq!(lines(&s).len(), 1);
    assert!(
        s.constraints
            .iter()
            .any(|c| matches!(&c.kind, ConstraintKind::Length { value, .. } if value == "30 mm"))
    );
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::Line, "道具は続く");
}

#[test]
fn s_opens_the_search_with_sketch_tools_and_the_bar_shows_sketch_keys() {
    let mut h = sketch_with_rectangle();
    let hints = editor(h.state_mut()).status_hints();
    assert!(
        hints.iter().any(|(k, d)| k == "右" && d == "メニュー"),
        "{hints:?}"
    );
    h.get_by_label("つながった線を選ぶ");
    press_key(&mut h, egui::Key::S);
    h.run();
    assert!(h.state().is_command_palette_open());
    let found = h.state().find_commands("オフセット");
    let first = found.first().expect("スケッチの道具も探せる");
    assert_eq!(first.name, "オフセット");
    h.state_mut().run_command(first.id.clone());
    h.run();
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::Offset);
}

#[test]
fn the_offset_tool_can_move_only_the_clicked_line_and_the_menu_breaks_the_link() {
    // K27: 「チェーン選択」を外すと押した 1 本だけ。K28: 右クリックで連動を外す
    let mut h = sketch_with_rectangle();
    let old = lines(&current_sketch(h.state()));
    press_key(&mut h, egui::Key::O);
    h.get_by_label("チェーン選択").click();
    h.run();
    assert!(editor(h.state_mut()).editor.offset.single);
    click_at(&mut h, [6.4, 16.36]);
    assert_eq!(editor(h.state_mut()).editor.offset.chain.len(), 1);
    hover(&mut h, [6.4, 17.92]);
    type_text(&mut h, "5");
    press_enter(&mut h);
    let s = current_sketch(h.state());
    let new: Vec<EntityId> = lines(&s).into_iter().filter(|l| !old.contains(l)).collect();
    assert_eq!(new.len(), 1, "1 本だけ写す");
    let offsets = |s: &Sketch| {
        s.constraints
            .iter()
            .filter(|c| matches!(c.kind, ConstraintKind::Offset { .. }))
            .count()
    };
    assert_eq!(offsets(&s), 1);
    // 写した線の上で右クリック → 「オフセットの連動を外す」
    right_click_at(&mut h, [3.8, 21.36]);
    h.get_by_label("オフセットの連動を外す").click();
    h.run();
    let s = current_sketch(h.state());
    assert_eq!(offsets(&s), 0);
    assert_eq!(lines(&s).len(), 5, "線は残る");
}

#[test]
fn two_selected_lines_offer_the_constraints_that_fit_them() {
    // K20: 2 本の線分を選んで右クリック → 平行・直角・等しい・同一直線を出し、押すと付く
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Line);
    h.run();
    click_at(&mut h, [-4.03, 0.72]);
    click_at(&mut h, [6.4, 5.93]);
    press_key(&mut h, egui::Key::Escape);
    press_key(&mut h, egui::Key::Escape);
    editor(h.state_mut()).set_tool(Tool::Line);
    h.run();
    click_at(&mut h, [-4.0, 12.0]);
    click_at(&mut h, [6.0, 15.0]);
    press_key(&mut h, egui::Key::Escape);
    press_key(&mut h, egui::Key::Escape);
    let ls = lines(&current_sketch(h.state()));
    assert_eq!(ls.len(), 2);
    click_at(&mut h, [1.19, 3.33]);
    click_with(&mut h, [1.0, 13.5], egui::Modifiers::SHIFT);
    assert_eq!(editor(h.state_mut()).editor.selection.len(), 2);
    right_click_at(&mut h, [1.0, 13.5]);
    h.get_by_label("直交させる");
    h.get_by_label("等しくする");
    h.get_by_label("同一直線上にする");
    h.get_by_label("平行にする").click();
    h.run();
    let s = current_sketch(h.state());
    assert!(
        s.constraints
            .iter()
            .any(|c| matches!(c.kind, ConstraintKind::Parallel { .. })),
        "{:?}",
        s.constraints
    );
}

#[test]
fn the_palette_linetype_sets_the_next_line_and_the_selection() {
    // K4: 選ばずに「中心線」→ 次に描く線分が中心線。選んで「標準」→ 普通の線に戻す
    let mut h = empty_sketch();
    h.get_by_label("中心線").click();
    h.run();
    assert!(editor(h.state_mut()).editor.draw_centerline);
    editor(h.state_mut()).set_tool(Tool::Line);
    h.run();
    click_at(&mut h, [-4.03, 0.72]);
    click_at(&mut h, [6.4, 5.93]);
    press_key(&mut h, egui::Key::Escape);
    press_key(&mut h, egui::Key::Escape);
    let s = current_sketch(h.state());
    let line = lines(&s)[0];
    assert!(s.is_centerline(line));
    click_at(&mut h, [1.19, 3.33]);
    // 選ぶと拘束のボタンが増えて線種は下へ隠れるので、転がして出す
    h.get_by_label("標準").scroll_to_me();
    h.run();
    h.get_by_label("標準").click();
    h.run();
    let s = current_sketch(h.state());
    assert!(!s.is_centerline(line) && !is_construction(&s, line));
    h.get_by_label("補助線").click();
    h.run();
    assert!(is_construction(&current_sketch(h.state()), line));
    // 3D スケッチの入口の案内(K14)
    h.get_by_label_contains("作成 → 3D スケッチ");
}

#[test]
fn the_control_point_spline_is_degree_five_unless_three_is_chosen() {
    // 制御点のスプラインの道具は、押した点をそのまま制御点にして Enter で作る
    // K3: 制御点のスプラインの次数は既定 5(Kie の決定)。パレットで 3 にできる
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::ControlSpline);
    h.run();
    assert_eq!(editor(h.state_mut()).editor.spline_degree, 5);
    let pts = [
        [-8.0, 0.0],
        [-5.0, 4.0],
        [-2.0, -3.0],
        [1.0, 4.0],
        [4.0, -3.0],
        [7.0, 2.0],
    ];
    for p in pts {
        click_at(&mut h, p);
    }
    press_enter(&mut h);
    assert!(current_sketch(h.state()).entities.iter().any(|e| matches!(
        e,
        Entity::Spline { points, control: true, .. } if points.len() == 6
    )));
    let degree = |s: &Sketch| {
        s.entities
            .iter()
            .filter_map(|e| match e {
                Entity::Spline { degree, .. } => Some(*degree),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(degree(&current_sketch(h.state())), vec![5]);
    h.get_by_label("3").scroll_to_me();
    h.run();
    h.get_by_label("3").click();
    h.run();
    editor(h.state_mut()).set_tool(Tool::ControlSpline);
    h.run();
    for p in pts {
        click_at(&mut h, [p[0], p[1] + 12.0]);
    }
    press_enter(&mut h);
    assert_eq!(degree(&current_sketch(h.state())), vec![5, 3]);
}
