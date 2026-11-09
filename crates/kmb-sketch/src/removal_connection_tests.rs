//! 確かめること: 消える線の端を介した一致を残し、引いた端と相手が一緒に動く。
use crate::*;
use std::collections::{BTreeSet, HashMap};

fn hub() -> (Sketch, EntityId, EntityId, EntityId) {
    let mut s = Sketch::default();
    let a = s.add_point(-10.0, 0.0);
    let p1 = s.add_point(0.0, 0.0);
    s.add_line(a, p1);
    let p2 = s.add_point(0.0, 0.0);
    let b = s.add_point(0.0, -10.0);
    let middle = s.add_line(p2, b);
    let p3 = s.add_point(0.0, 0.0);
    let c = s.add_point(10.0, 0.0);
    s.add_line(p3, c);
    s.add_constraint(ConstraintKind::Coincident { a: p1, b: p2 });
    s.add_constraint(ConstraintKind::Coincident { a: p2, b: p3 });
    (s, middle, p1, p3)
}
fn tied(s: &Sketch, a: EntityId, b: EntityId) -> bool {
    let groups = crate::ends::tied_groups(s);
    groups.get(&a).copied().unwrap_or(a) == groups.get(&b).copied().unwrap_or(b)
}
#[test]
fn deleting_the_middle_line_keeps_the_surviving_ends_tied() {
    let (mut s, middle, a, b) = hub();
    assert!(tied(&s, a, b));
    s.remove_entities(&[middle]);
    assert!(s.entity(middle).is_none());
    assert!(tied(&s, a, b), "残る2本の端の一致を保つ");
}
#[test]
fn trimming_the_whole_middle_line_keeps_the_surviving_ends_tied() {
    let (mut s, middle, a, b) = hub();
    assert!(s.trim(middle, [0.0, -5.0]));
    assert!(s.entity(middle).is_none(), "交点が端だけの線を全部消す");
    assert!(tied(&s, a, b), "全体をトリムしても残端の一致を保つ");
}
#[test]
fn dragging_a_surviving_end_moves_its_partner_after_middle_line_removal() {
    let (mut s, middle, a, b) = hub();
    s.remove_entities(&[middle]);
    let mut moved = s.clone();
    moved.set_point(a, 2.0, 3.0);
    let sol = solve_dragging(&s, &moved, &HashMap::new(), &BTreeSet::from([a]), None).unwrap();
    let (a, b) = (sol.points[&a], sol.points[&b]);
    assert!((a[0] - 2.0).hypot(a[1] - 3.0) < 1e-6, "引いた端が目標へ動く");
    assert!((a[0] - b[0]).hypot(a[1] - b[1]) < 1e-6, "相手の端も一緒に動く");
}
