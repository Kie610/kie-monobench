//! 中心の円を既にある点の上で終えると、円がその点を通ることを、人の操作の順で確かめる。

use crate::Tool;
use crate::tests::*;
use kmb_sketch::{ConstraintKind, Entity};

#[test]
fn a_center_circle_ended_on_a_point_passes_through_it() {
    // 線の端に円周を合わせて描いた中心の円は、その端を通る円になる。
    // 半径を測るだけだと、端を動かした時に円が離れる
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Line);
    h.run();
    click_at(&mut h, [1.19, 11.15]);
    click_at(&mut h, [5.36, 11.15]);
    press_key(&mut h, egui::Key::Escape);
    let end = current_sketch(h.state())
        .entities
        .iter()
        .find_map(|e| match e {
            Entity::Line { b, .. } => Some(*b),
            _ => None,
        })
        .expect("線");
    editor(h.state_mut()).set_tool(Tool::Circle);
    h.run();
    click_at(&mut h, [9.01, 11.15]);
    click_at(&mut h, [5.36, 11.15]);
    let s = current_sketch(h.state());
    let circle = s
        .entities
        .iter()
        .find_map(|e| match e {
            Entity::Circle { id, .. } => Some(*id),
            _ => None,
        })
        .expect("円");
    assert!(
        s.constraints
            .iter()
            .any(|c| c.kind == ConstraintKind::PointOnCircle { point: end, circle }),
        "円周が線の端を通る: {:?}",
        s.constraints
    );
}

#[test]
fn a_center_circle_ended_in_empty_space_gets_no_point_on_it() {
    // 何も無い所で終えた円には、円周上の拘束を付けない
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Circle);
    h.run();
    click_at(&mut h, [9.01, 11.15]);
    click_at(&mut h, [12.0, 11.15]);
    let s = current_sketch(h.state());
    assert!(
        !s.constraints
            .iter()
            .any(|c| matches!(c.kind, ConstraintKind::PointOnCircle { .. })),
        "{:?}",
        s.constraints
    );
}
