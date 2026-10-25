//! 寸法の操作を、人の操作(クリック、キー、引く)で確かめる: `名前=値` でパラメータを作る(F7)、
//! 札を選んで Delete で消す(F6)、札を引いて動かす(F5)、数字から打つと値の欄へ移る(F8)、
//! 右クリックで寸法の種類を選び直す(F1)。

use crate::Tool;
use crate::tests::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use jc_sketch::{ConstraintId, ConstraintKind};
use jc_ui::Workspace;

/// 長方形の上の辺に長さの寸法(30 と打つ)を置き、選ぶ道具へ戻ったところ。置いた寸法の番号。
fn rectangle_with_top_length(h: &mut Harness<'static, Workspace>) -> ConstraintId {
    press_key(h, egui::Key::D);
    click_at(h, [6.4, 16.36]);
    move_to(h, [6.4, 19.49]);
    h.run();
    quick_click(h, [6.4, 19.49]);
    type_text(h, "30");
    press_enter(h);
    press_key(h, egui::Key::Escape);
    press_key(h, egui::Key::Escape);
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::Select);
    h.run();
    current_sketch(h.state())
        .constraints
        .iter()
        .find(|c| matches!(c.kind, ConstraintKind::Length { .. }))
        .expect("長さの寸法")
        .id
}

/// 寸法の値の札の画面での中心。
fn label_of(h: &mut Harness<'static, Workspace>, id: ConstraintId) -> egui::Pos2 {
    editor(h.state_mut())
        .view
        .labels
        .iter()
        .find(|(c, _)| *c == id)
        .map(|(_, r)| r.center())
        .expect("札が描かれている")
}

/// 寸法の札を置いた位置(平面の座標。今の形について)。
fn label_world(h: &Harness<'static, Workspace>, id: ConstraintId) -> [f64; 2] {
    let s = current_sketch(h.state());
    let v = jc_sketch::dimension_values(&s, &|_| None).unwrap();
    let sol = jc_sketch::solve(&s, &v).unwrap();
    let c = s.constraints.iter().find(|c| c.id == id).unwrap();
    s.label_at(c, &sol).expect("札を置いた位置")
}

#[test]
fn typing_name_equals_value_makes_a_parameter_and_uses_it() {
    let mut h = sketch_with_rectangle();
    press_key(&mut h, egui::Key::D);
    click_at(&mut h, [6.4, 16.36]);
    move_to(&mut h, [6.4, 19.49]);
    h.run();
    quick_click(&mut h, [6.4, 19.49]);
    assert!(editor(h.state_mut()).editor.placing.is_some());
    // 札の欄は測った値を全部選んでいるので、打つと置き換わる
    type_text(&mut h, "w=30");
    press_enter(&mut h);
    h.run();
    h.run();
    let doc = h.state().document().data();
    let p = doc
        .params
        .iter()
        .find(|p| p.name == "w")
        .expect("パラメータ w");
    assert_eq!(p.expr, "30 mm");
    let s = current_sketch(h.state());
    let c = s
        .constraints
        .iter()
        .find(|c| matches!(c.kind, ConstraintKind::Length { .. }))
        .expect("長さの寸法");
    assert_eq!(c.kind.value(), Some("w"));
    assert_eq!(h.state().report().error_count(), 0);
    // 同じ名前をもう一度作ろうとすると断る(寸法は置かない)。パネルの寸法ボタンから
    let before = current_sketch(h.state()).constraints.len();
    press_key(&mut h, egui::Key::Escape);
    press_key(&mut h, egui::Key::Escape);
    let side = current_sketch(h.state())
        .entities
        .iter()
        .filter(|e| is_line(e))
        .map(|e| e.id())
        .find(|id| {
            !s.constraints
                .iter()
                .any(|c| jc_sketch::refs(&c.kind).contains(id) && c.kind.value().is_some())
        })
        .expect("寸法のない辺");
    editor(h.state_mut()).editor.selection.insert(side);
    h.run();
    editor(h.state_mut()).editor.dim_text = "w=20".into();
    h.run();
    h.query_all_by_label("長さ")
        .next()
        .expect("長さのボタン")
        .click();
    h.run();
    assert_eq!(current_sketch(h.state()).constraints.len(), before);
    let err = editor(h.state_mut()).editor.dim_error.clone();
    assert!(
        err.as_deref()
            .is_some_and(|e| e.contains("パラメータ「w」は既にあります")),
        "{err:?}"
    );
}

#[test]
fn a_dimension_label_is_picked_by_a_click_and_deleted_with_delete() {
    let mut h = sketch_with_rectangle();
    let id = rectangle_with_top_length(&mut h);
    let at = label_of(&mut h, id);
    quick_click_px(&mut h, at);
    h.run();
    let e = editor(h.state_mut());
    assert_eq!(e.editor.picked_constraint, Some(id));
    assert!(e.editor.selection.is_empty());
    press_key(&mut h, egui::Key::Delete);
    h.run();
    assert!(
        current_sketch(h.state())
            .constraints
            .iter()
            .all(|c| c.id != id),
        "Delete で寸法が消える"
    );
}

#[test]
fn dragging_a_dimension_label_moves_where_it_is_shown() {
    let mut h = sketch_with_rectangle();
    let id = rectangle_with_top_length(&mut h);
    let before = label_world(&h, id);
    let at = label_of(&mut h, id);
    drag_px(
        &mut h,
        at,
        at + egui::vec2(40.0, -60.0),
        egui::Modifiers::NONE,
    );
    let after = label_world(&h, id);
    let s = current_sketch(h.state());
    let c = s.constraints.iter().find(|c| c.id == id).unwrap();
    assert!(
        after[1] > before[1] + 1.0,
        "上へ動く {before:?} → {after:?}"
    );
    assert!(
        after[0] > before[0] + 1.0,
        "右へ動く {before:?} → {after:?}"
    );
    // 形は動かない(値は 30 のまま)、辺も選ばれない
    assert_eq!(c.kind.value(), Some("30 mm"));
    assert!(editor(h.state_mut()).editor.selection.is_empty());
    let moved = label_of(&mut h, id);
    assert!(moved.y < at.y - 20.0, "札が上に出る {at:?} → {moved:?}");
}

#[test]
fn a_digit_in_the_dimension_tool_moves_input_to_the_value_field() {
    let mut h = sketch_with_rectangle();
    press_key(&mut h, egui::Key::D);
    click_at(&mut h, [6.4, 16.36]);
    move_to(&mut h, [6.4, 19.49]);
    h.run();
    assert!(editor(h.state_mut()).editor.ghost.is_some());
    // 文字のキーは道具の切り替えのまま。数字から打つと値の欄へ入る
    type_text(&mut h, "4");
    type_text(&mut h, "5");
    assert_eq!(editor(h.state_mut()).editor.dim_text, "45");
    press_enter(&mut h);
    let s = current_sketch(h.state());
    let c = s
        .constraints
        .iter()
        .find(|c| matches!(c.kind, ConstraintKind::Length { .. }))
        .expect("長さの寸法");
    assert_eq!(c.kind.value(), Some("45 mm"));
}

#[test]
fn right_click_offers_a_diameter_for_a_circle_and_driven_for_any() {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Circle);
    h.run();
    click_at(&mut h, [6.4, 13.75]);
    click_at(&mut h, [9.01, 13.75]);
    press_key(&mut h, egui::Key::Escape);
    press_key(&mut h, egui::Key::D);
    click_at(&mut h, [9.01, 13.75]);
    move_to(&mut h, [12.66, 16.36]);
    h.run();
    let ghost = editor(h.state_mut()).editor.ghost.clone();
    assert!(
        matches!(ghost, Some((ConstraintKind::Diameter { .. }, _))),
        "{ghost:?}"
    );
    right_click_at(&mut h, [12.66, 16.36]);
    h.get_by_label("半径にする").click();
    h.run();
    move_to(&mut h, [12.76, 16.26]);
    h.run();
    let ghost = editor(h.state_mut()).editor.ghost.clone();
    assert!(
        matches!(ghost, Some((ConstraintKind::Radius { .. }, _))),
        "{ghost:?}"
    );
    right_click_at(&mut h, [12.76, 16.26]);
    h.get_by_label("参照寸法にする").click();
    h.run();
    move_to(&mut h, [12.87, 16.15]);
    h.run();
    quick_click(&mut h, [12.87, 16.15]);
    press_enter(&mut h);
    let s = current_sketch(h.state());
    let c = s
        .constraints
        .iter()
        .find(|c| matches!(c.kind, ConstraintKind::Radius { .. }))
        .expect("半径の寸法");
    assert!(c.reference, "被駆動(参照寸法)で置く");
}

#[test]
fn a_panel_dimension_that_is_already_determined_becomes_a_reference() {
    // 網羅版 E9: パネルの寸法ボタンは、過剰になる寸法を参照寸法として置き、その旨を出す
    let mut h = sketch_with_rectangle();
    let _ = rectangle_with_top_length(&mut h);
    // 上の辺の長さが決まっている。同じ辺にもう一度長さをパネルから足す
    click_at(&mut h, [6.4, 16.36]);
    h.query_all_by_label("長さ")
        .next()
        .expect("長さのボタン")
        .click();
    h.run();
    let s = current_sketch(h.state());
    let lengths: Vec<_> = s
        .constraints
        .iter()
        .filter(|c| matches!(c.kind, ConstraintKind::Length { .. }))
        .collect();
    assert_eq!(lengths.len(), 2);
    assert!(lengths[1].reference);
    let note = editor(h.state_mut())
        .editor
        .notice
        .clone()
        .unwrap_or_default();
    assert!(note.contains("参照寸法"), "{note}");
    assert!(
        !format!(
            "{:?}",
            jc_sketch::solve(&s, &Default::default()).map(|s| s.status)
        )
        .contains("OverConstrained")
    );
}

/// 長方形の左の辺(番号)と、右下の角の点(番号)。
fn left_edge_and_far_corner(
    h: &Harness<'static, Workspace>,
) -> (jc_sketch::EntityId, jc_sketch::EntityId) {
    let s = current_sketch(h.state());
    let sol = solved(h.state());
    let x = |id: &jc_sketch::EntityId| sol.points[id][0];
    let left = s
        .entities
        .iter()
        .find_map(|e| match e {
            jc_sketch::Entity::Line { id, a, b, .. }
                if (x(a) - x(b)).abs() < 1e-6
                    && sol.points.values().all(|p| p[0] >= x(a) - 1e-6) =>
            {
                Some(*id)
            }
            _ => None,
        })
        .expect("左の辺");
    let corner = *sol
        .points
        .iter()
        .max_by(|p, q| (p.1[0] - p.1[1]).total_cmp(&(q.1[0] - q.1[1])))
        .unwrap()
        .0;
    (left, corner)
}

#[test]
fn a_centerline_and_a_point_give_a_diameter_or_a_distance_from_the_menu() {
    let mut h = sketch_with_rectangle();
    let (left, corner) = left_edge_and_far_corner(&h);
    // 左の辺を中心線にする(パネルのボタン)
    editor(h.state_mut()).editor.selection.insert(left);
    h.run();
    h.get_by_label("中心線の切り替え").click();
    h.run();
    assert!(current_sketch(h.state()).is_centerline(left));
    press_key(&mut h, egui::Key::D);
    {
        let e = &mut editor(h.state_mut()).editor;
        e.selection.clear();
        e.selection.extend([left, corner]);
    }
    move_to(&mut h, [6.4, 4.89]);
    h.run();
    h.run();
    let ghost = editor(h.state_mut()).editor.ghost.clone();
    assert!(
        matches!(ghost, Some((ConstraintKind::LinearDiameter { .. }, _))),
        "中心線からの直径(回転体の直径){ghost:?}"
    );
    right_click_at(&mut h, [6.4, 4.89]);
    h.get_by_label("中心線からの距離にする").click();
    h.run();
    move_to(&mut h, [6.51, 4.79]);
    h.run();
    let ghost = editor(h.state_mut()).editor.ghost.clone();
    assert!(
        matches!(ghost, Some((ConstraintKind::PointLineDistance { .. }, _))),
        "{ghost:?}"
    );
}

#[test]
fn a_circle_and_a_line_can_be_measured_from_the_near_edge() {
    let mut h = sketch_with_rectangle();
    let (left, _) = left_edge_and_far_corner(&h);
    editor(h.state_mut()).set_tool(Tool::Circle);
    h.run();
    click_at(&mut h, [6.4, 12.45]);
    click_at(&mut h, [7.97, 12.45]);
    press_key(&mut h, egui::Key::Escape);
    let circle = current_sketch(h.state())
        .entities
        .iter()
        .find(|e| matches!(e, jc_sketch::Entity::Circle { .. }))
        .map(|e| e.id())
        .expect("円");
    press_key(&mut h, egui::Key::D);
    {
        let e = &mut editor(h.state_mut()).editor;
        e.selection.clear();
        e.selection.extend([circle, left]);
    }
    move_to(&mut h, [4.32, 18.97]);
    h.run();
    h.run();
    let ghost = editor(h.state_mut()).editor.ghost.clone();
    assert!(
        matches!(ghost, Some((ConstraintKind::PointLineDistance { .. }, _))),
        "まずは中心から {ghost:?}"
    );
    right_click_at(&mut h, [4.32, 18.97]);
    h.get_by_label("円周の近い側から測る").click();
    h.run();
    move_to(&mut h, [4.42, 18.86]);
    h.run();
    let ghost = editor(h.state_mut()).editor.ghost.clone();
    let Some((ConstraintKind::CircleDistance { far, .. }, text)) = ghost else {
        panic!("円周からの距離 {ghost:?}");
    };
    assert!(!far);
    // 中心から左の辺まで 100 px 相当、半径 30 px 相当の差だけ短い
    quick_click(&mut h, [4.42, 18.86]);
    press_enter(&mut h);
    let s = current_sketch(h.state());
    assert!(
        s.constraints
            .iter()
            .any(|c| matches!(c.kind, ConstraintKind::CircleDistance { far: false, .. })),
        "置いた {text}"
    );
    assert_eq!(h.state().report().error_count(), 0);
}

#[test]
fn the_panel_offers_circle_edge_distances_like_the_dimension_tool() {
    let mut h = sketch_with_rectangle();
    let (left, _) = left_edge_and_far_corner(&h);
    editor(h.state_mut()).set_tool(Tool::Circle);
    h.run();
    click_at(&mut h, [6.4, 12.45]);
    click_at(&mut h, [7.97, 12.45]);
    press_key(&mut h, egui::Key::Escape);
    let circle = current_sketch(h.state())
        .entities
        .iter()
        .find(|e| matches!(e, jc_sketch::Entity::Circle { .. }))
        .map(|e| e.id())
        .expect("円");
    editor(h.state_mut())
        .editor
        .selection
        .extend([circle, left]);
    h.run();
    h.get_by_label("中心からの距離");
    h.get_by_label("円周から(遠い側)");
    h.get_by_label("円周から(近い側)").click();
    h.run();
    assert!(
        current_sketch(h.state())
            .constraints
            .iter()
            .any(|c| matches!(c.kind, ConstraintKind::CircleDistance { far: false, .. }))
    );
    assert_eq!(h.state().report().error_count(), 0);
}

/// Kie のレビュー(K10): 寸法を直している間に別の寸法の札をクリックすると、その名前(d1 など)が
/// 式に入り、続けて打った式で形が決まる。
#[test]
fn clicking_another_label_while_editing_inserts_its_name() {
    let mut h = sketch_with_rectangle();
    let top = rectangle_with_top_length(&mut h);
    // 右の辺に長さの寸法(20)を置く(上の寸法で形が縮んだので、辺の今の位置を求める)
    let right = {
        let sol = solved(h.state());
        current_sketch(h.state())
            .entities
            .iter()
            .filter_map(|e| match e {
                jc_sketch::Entity::Line { a, b, .. }
                    if (sol.points[a][0] - sol.points[b][0]).abs() < 1e-6 =>
                {
                    let (p, q) = (sol.points[a], sol.points[b]);
                    Some([p[0], (p[1] + q[1]) / 2.0])
                }
                _ => None,
            })
            .max_by(|p, q| p[0].total_cmp(&q[0]))
            .expect("右の辺")
    };
    let mid = editor(h.state_mut())
        .view
        .last
        .clone()
        .expect("描いた後")
        .to_screen(right);
    press_key(&mut h, egui::Key::D);
    click_px(&mut h, mid);
    h.hover_at(mid - egui::vec2(60.0, 0.0));
    h.run();
    quick_click_px(&mut h, mid - egui::vec2(60.0, 0.0));
    type_text(&mut h, "20");
    press_enter(&mut h);
    press_key(&mut h, egui::Key::Escape);
    press_key(&mut h, egui::Key::Escape);
    h.run();
    let s = current_sketch(h.state());
    let side = s
        .constraints
        .iter()
        .find(|c| c.id != top && matches!(c.kind, ConstraintKind::Length { .. }))
        .expect("右の辺の寸法")
        .id;
    let top_name = s.dim_name(top).expect("寸法に名前が付く").to_string();
    assert!(s.dim_name(side).is_some());
    // 右の辺の札をダブルクリックして直し始め、上の辺の札をクリックする
    let at = label_of(&mut h, side);
    h.hover_at(at);
    h.run();
    // 2 回の押し離しを 1 コマで送る(h.event は 1 件ごとに 0.25 秒のコマを進めるので、分けると二度押しにならない)
    for _ in 0..2 {
        for pressed in [true, false] {
            h.input_mut().events.push(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
    }
    h.step();
    h.run();
    assert!(
        editor(h.state_mut())
            .editor
            .edit_dim
            .as_ref()
            .is_some_and(|(c, _)| *c == side),
        "ダブルクリックで値の欄が出る"
    );
    let at_top = label_of(&mut h, top);
    quick_click_px(&mut h, at_top);
    h.run();
    let typed = editor(h.state_mut()).editor.edit_dim.clone();
    assert_eq!(typed, Some((side, top_name.clone())), "名前が入る");
    // 札のクリックで寸法の選びは変わらない
    assert_ne!(editor(h.state_mut()).editor.picked_constraint, Some(top));
    type_text(&mut h, " / 3");
    press_enter(&mut h);
    h.run();
    let s = current_sketch(h.state());
    let c = s.constraints.iter().find(|c| c.id == side).unwrap();
    assert_eq!(c.kind.value(), Some(format!("{top_name} / 3").as_str()));
    assert_eq!(h.state().report().error_count(), 0);
    // 形が付いて来る: 上の辺 30 の 1/3 = 10
    let sol = crate::tests::solved(h.state());
    let ys = {
        let mut v: Vec<f64> = sol.points.values().map(|p| p[1]).collect();
        v.sort_by(f64::total_cmp);
        v.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        v
    };
    assert!((ys.last().unwrap() - ys[0] - 10.0).abs() < 1e-6, "{ys:?}");
}

#[test]
fn insert_name_replaces_the_selection_or_goes_at_the_caret() {
    use crate::view::insert_name;
    assert_eq!(insert_name("20 mm", 0..5, "d1"), ("d1".into(), 2));
    assert_eq!(insert_name("2 * ", 4..4, "d3"), ("2 * d3".into(), 6));
    assert_eq!(insert_name("幅 + ", 9..9, "d2"), ("幅 + d2".into(), 6));
}

/// Kie のレビュー(K10): パラメータの画面の「モデルパラメータ」で、寸法の式をその場で直せる
/// (ブラウザの「パラメータ」を押す → d1 の式の欄をクリック → 「 + 10」と打って Enter)。
#[test]
fn a_dimension_can_be_edited_from_the_parameter_list() {
    let mut h = sketch_with_rectangle();
    let top = rectangle_with_top_length(&mut h);
    let name = current_sketch(h.state()).dim_name(top).unwrap().to_string();
    // ブラウザの「パラメータ」の行(フィルターの「パラメータ」と同じ名前なので最後の方)
    h.get_all_by_label("パラメータ").last().unwrap().click();
    h.run();
    h.get_by(|n| {
        n.role() == egui::accesskit::Role::TextInput && n.value().as_deref() == Some("30 mm")
    })
    .click();
    h.run();
    press_key(&mut h, egui::Key::End);
    type_text(&mut h, " + 10");
    press_enter(&mut h);
    h.run();
    let s = current_sketch(h.state());
    let c = s.constraints.iter().find(|c| c.id == top).unwrap();
    assert_eq!(c.kind.value(), Some("30 mm + 10"));
    let v = h.state().report().param_values()[name.as_str()];
    assert!((v.value - 40.0).abs() < 1e-9, "{v:?}");
}
