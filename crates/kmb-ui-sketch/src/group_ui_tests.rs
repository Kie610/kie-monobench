//! まとまりの拘束(オフセット・ポリゴン)と円の固定を、人の操作(クリック、右クリック、キー、
//! パネルの欄)で確かめる(レビューの K7・K8・K11・K14)。

use crate::Tool;
use crate::tests::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kmb_sketch::{ConstraintKind, Entity, EntityId, Sketch};
use kmb_ui::Workspace;

fn hover(h: &mut Harness<'static, Workspace>, p: [f64; 2]) {
    move_to(h, p);
    h.run();
}

/// 寸法の値も入れて解いた形。
fn solution(h: &Harness<'static, Workspace>) -> kmb_sketch::Solution {
    let s = current_sketch(h.state());
    let v = kmb_sketch::dimension_values(&s, &|_| None).unwrap();
    kmb_sketch::solve(&s, &v).unwrap()
}

fn count(s: &Sketch, f: impl Fn(&ConstraintKind) -> bool) -> usize {
    s.constraints.iter().filter(|c| f(&c.kind)).count()
}

fn arcs(s: &Sketch) -> Vec<EntityId> {
    s.entities
        .iter()
        .filter_map(|e| match e {
            Entity::Arc { id, .. } => Some(*id),
            _ => None,
        })
        .collect()
}

fn lines(s: &Sketch) -> Vec<EntityId> {
    s.entities
        .iter()
        .filter_map(|e| match e {
            Entity::Line { id, .. } => Some(*id),
            _ => None,
        })
        .collect()
}

#[test]
fn an_offset_shows_one_mark_and_one_dimension() {
    let mut h = sketch_with_rectangle();
    let before = current_sketch(h.state()).constraints.len();
    press_key(&mut h, egui::Key::O);
    click_at(&mut h, [6.4, 16.36]);
    hover(&mut h, [6.4, 17.92]);
    type_text(&mut h, "5");
    press_enter(&mut h);
    let s = current_sketch(h.state());
    assert_eq!(lines(&s).len(), 8);
    // オフセットの拘束 1 つと距離の寸法 1 つだけ(辺ごとの平行や距離の寸法は出ない)
    assert_eq!(s.constraints.len(), before + 2, "{:?}", s.constraints);
    assert_eq!(count(&s, |k| matches!(k, ConstraintKind::Offset { .. })), 1);
    assert_eq!(count(&s, |k| k.value().is_some()), 1);
    h.run();
    let marks = editor(h.state_mut()).view.marks.clone();
    let offset = s
        .constraints
        .iter()
        .find(|c| matches!(c.kind, ConstraintKind::Offset { .. }))
        .unwrap()
        .id;
    assert_eq!(
        marks.iter().filter(|m| m.id == offset).count(),
        1,
        "印は 1 つ"
    );
    // 印を押すと拘束を選び、Delete で外すと距離の寸法は残る
    let m = marks.iter().find(|m| m.id == offset).unwrap().clone();
    click_px(&mut h, m.rect.center());
    assert_eq!(editor(h.state_mut()).editor.picked_constraint, Some(offset));
    press_key(&mut h, egui::Key::Delete);
    let s = current_sketch(h.state());
    assert_eq!(count(&s, |k| matches!(k, ConstraintKind::Offset { .. })), 0);
}

#[test]
fn the_offset_menu_rounds_the_outer_corners() {
    let mut h = sketch_with_rectangle();
    press_key(&mut h, egui::Key::O);
    click_at(&mut h, [6.4, 16.36]);
    // 右クリックで角の作り方を選ぶ
    right_click_at(&mut h, [6.4, 17.92]);
    h.get_all_by_label("角: 丸める(角の点から等距離)")
        .last()
        .unwrap()
        .click();
    h.run();
    assert!(editor(h.state_mut()).editor.offset.round);
    hover(&mut h, [6.4, 17.92]);
    type_text(&mut h, "4");
    press_enter(&mut h);
    let s = current_sketch(h.state());
    let corners: Vec<EntityId> = s
        .entities
        .iter()
        .filter_map(|e| match e {
            Entity::Line { a, .. } => Some(*a),
            _ => None,
        })
        .take(4)
        .collect();
    let made = arcs(&s);
    assert_eq!(made.len(), 4, "角ごとに円弧");
    let sol = solution(&h);
    for a in made {
        let Some(Entity::Arc { center, start, .. }) = s.entity(a) else {
            unreachable!()
        };
        assert!(corners.contains(center), "元の角の点が中心");
        let (c, p) = (sol.points[center], sol.points[start]);
        assert!(((c[0] - p[0]).hypot(c[1] - p[1]) - 4.0).abs() < 1e-6);
    }
    // 角の作り方は次のオフセットでも覚えている(パネルにも出る)
    press_key(&mut h, egui::Key::O);
    assert!(editor(h.state_mut()).editor.offset.round);
    let _ = h.get_by_label("角: 辺を延ばす");
}

#[test]
fn the_polygon_count_is_set_in_the_panel_and_changed_later() {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Polygon);
    h.run();
    // パネルの「辺の数」に 8 を打ってから描く
    let field = h.get_by_label("辺の数");
    field.click();
    h.run();
    field_type(&mut h, "8");
    click_at(&mut h, [6.4, 11.15]);
    click_at(&mut h, [10.57, 11.15]);
    press_key(&mut h, egui::Key::Escape);
    let s = current_sketch(h.state());
    assert_eq!(lines(&s).len(), 8);
    assert_eq!(
        count(&s, |k| matches!(k, ConstraintKind::Polygon { .. })),
        1
    );
    // 辺を選ぶとパネルにポリゴンの欄が出て、辺の数を 5 に直せる
    press_key(&mut h, egui::Key::Escape);
    let side = lines(&s)[0];
    editor(h.state_mut()).editor.selection.insert(side);
    h.run();
    let field = h.get_by_label("辺の数");
    field.click();
    h.run();
    field_type(&mut h, "5");
    let s = current_sketch(h.state());
    assert_eq!(lines(&s).len(), 5, "{:?}", lines(&s));
    let sol = solved(h.state());
    let lens: Vec<f64> = lines(&s)
        .iter()
        .map(|l| match s.entity(*l) {
            Some(Entity::Line { a, b, .. }) => {
                let (p, q) = (sol.points[a], sol.points[b]);
                (p[0] - q[0]).hypot(p[1] - q[1])
            }
            _ => unreachable!(),
        })
        .collect();
    assert!(
        lens.iter().all(|l| (l - lens[0]).abs() < 1e-6),
        "正五角形: {lens:?}"
    );
}

/// 焦点のある欄の字を全部消して `text` を打ち、Enter で決める。
fn field_type(h: &mut Harness<'static, Workspace>, text: &str) {
    h.event(egui::Event::Key {
        key: egui::Key::A,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::COMMAND,
    });
    h.run();
    h.event(egui::Event::Text(text.into()));
    h.run();
    press_enter(h);
}

#[test]
fn a_circle_and_its_center_are_fixed_from_the_menu_separately() {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Circle);
    h.run();
    click_at(&mut h, [6.4, 11.15]);
    click_at(&mut h, [9.53, 11.15]);
    press_key(&mut h, egui::Key::Escape);
    let s = current_sketch(h.state());
    let (circle, center) = s
        .entities
        .iter()
        .find_map(|e| match e {
            Entity::Circle { id, center, .. } => Some((*id, *center)),
            _ => None,
        })
        .unwrap();
    let fixed = |h: &Harness<'static, Workspace>, id: EntityId| {
        current_sketch(h.state())
            .constraints
            .iter()
            .any(|c| matches!(c.kind, ConstraintKind::Fix { point } if point == id))
    };
    // 円周を右クリック → 中心だけ固定する
    right_click_at(&mut h, [6.4, 14.28]);
    h.get_by_label("中心だけ固定する").click();
    h.run();
    assert!(fixed(&h, center) && !fixed(&h, circle));
    assert_eq!(solved(h.state()).dof, 1, "大きさは動く");
    // 円も固定する(余分にならない)
    right_click_at(&mut h, [6.4, 14.28]);
    h.get_by_label("固定する").click();
    h.run();
    assert!(fixed(&h, center) && fixed(&h, circle));
    assert_eq!(
        solved(h.state()).status,
        kmb_sketch::SolveStatus::WellConstrained
    );
    // 円の固定を外しても、中心の固定は残る
    right_click_at(&mut h, [6.4, 14.28]);
    h.get_by_label("固定解除").click();
    h.run();
    assert!(fixed(&h, center) && !fixed(&h, circle));
    // 拘束の道具の固定は半径の寸法を足さない(右クリックで選んだ円の選びは外してから)
    press_key(&mut h, egui::Key::Escape);
    let n = current_sketch(h.state()).constraints.len();
    editor(h.state_mut()).set_tool(Tool::Constraint("固定"));
    h.run();
    click_at(&mut h, [6.4, 14.28]);
    let s = current_sketch(h.state());
    assert_eq!(s.constraints.len(), n + 1);
    assert_eq!(count(&s, |k| k.value().is_some()), 0);
}

#[test]
fn a_typed_distance_larger_than_the_shape_still_goes_to_the_cursor_side() {
    let mut h = sketch_with_rectangle();
    let sol = solved(h.state());
    let ys: Vec<f64> = sol.points.values().map(|p| p[1]).collect();
    let tall =
        ys.iter().cloned().fold(f64::MIN, f64::max) - ys.iter().cloned().fold(f64::MAX, f64::min);
    // カーソルは上の辺のすぐ外。打つ距離は形の高さより大きい
    let far = (tall * 1.5).ceil();
    press_key(&mut h, egui::Key::O);
    click_at(&mut h, [6.4, 16.36]);
    hover(&mut h, [6.4, 16.78]);
    type_text(&mut h, &far.to_string());
    press_enter(&mut h);
    let s = current_sketch(h.state());
    assert_eq!(lines(&s).len(), 8, "外へ作る");
    let sol2 = solution(&h);
    let top = ys.iter().cloned().fold(f64::MIN, f64::max);
    let new_top = sol2.points.values().map(|p| p[1]).fold(f64::MIN, f64::max);
    assert!((new_top - top - far).abs() < 1e-6, "{new_top} {top} {far}");
}

#[test]
fn a_symmetric_offset_too_wide_for_the_inside_makes_only_the_outside() {
    let mut h = sketch_with_rectangle();
    let before = lines(&current_sketch(h.state())).len();
    press_key(&mut h, egui::Key::O);
    click_at(&mut h, [6.4, 16.36]);
    right_click_at(&mut h, [6.4, 17.92]);
    h.get_all_by_label("対称").last().unwrap().click();
    h.run();
    hover(&mut h, [6.4, 17.92]);
    // 矩形の高さ(約 7.8 mm)より大きい 10 mm: 内へ 5 mm は上下の線が入れ替わるので作らない
    type_text(&mut h, "10");
    press_enter(&mut h);
    let s = current_sketch(h.state());
    assert_eq!(lines(&s).len(), before + 4, "外側だけ");
    let notice = editor(h.state_mut())
        .editor
        .notice
        .clone()
        .unwrap_or_default();
    assert!(notice.contains("内側は作れません"), "{notice}");
    solution(&h);
}
