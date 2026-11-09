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
    moved.set_point(a, 0.2, 0.3);
    let sol = solve_dragging(&s, &moved, &HashMap::new(), &BTreeSet::from([a]), None).unwrap();
    let (a, b) = (sol.points[&a], sol.points[&b]);
    assert!(
        (a[0] - 0.2).hypot(a[1] - 0.3) < 1e-6,
        "引いた端が目標へ動く: {a:?}, {b:?}"
    );
    assert!(
        (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-6,
        "相手の端も一緒に動く: {a:?}, {b:?}"
    );
}

#[test]
fn surviving_constraints_are_kept_without_a_duplicate_coincidence() {
    let (mut s, middle, a, b) = hub();
    let direct = s.add_constraint(ConstraintKind::Coincident { a, b });
    let line = s
        .entities
        .iter()
        .find_map(|e| match e {
            Entity::Line { id, a: p, b: q, .. } if *p == a || *q == a => Some(*id),
            _ => None,
        })
        .unwrap();
    let horizontal = s.add_constraint(ConstraintKind::Horizontal { line });
    let before: Vec<_> = s
        .constraints
        .iter()
        .filter(|c| c.id == direct || c.id == horizontal)
        .map(|c| serde_json::to_value(c).unwrap())
        .collect();
    s.remove_entities(&[middle]);
    let after: Vec<_> = s
        .constraints
        .iter()
        .map(|c| serde_json::to_value(c).unwrap())
        .collect();
    assert_eq!(
        after, before,
        "既存の残る関係を変えず、重ねて一致を足さない"
    );
}
fn many_ends(positions: &[f64]) -> (Sketch, EntityId, Vec<EntityId>) {
    let mut s = Sketch::default();
    let hub = s.add_point(0.0, 0.0);
    let bottom = s.add_point(0.0, -10.0);
    let middle = s.add_line(hub, bottom);
    let mut ends = Vec::new();
    for (i, x) in positions.iter().enumerate() {
        let end = s.add_point(*x, 0.0);
        let far = s.add_point(10.0 + i as f64, 10.0);
        s.add_line(end, far);
        s.add_constraint(ConstraintKind::Coincident { a: hub, b: end });
        ends.push(end);
    }
    (s, middle, ends)
}
fn pairs(s: &Sketch) -> Vec<(EntityId, EntityId)> {
    s.constraints
        .iter()
        .filter_map(|c| match c.kind {
            ConstraintKind::Coincident { a, b } => Some((a.min(b), a.max(b))),
            _ => None,
        })
        .collect()
}
#[test]
fn three_remaining_ends_pair_the_nearest_once_and_leave_the_odd_end() {
    let (mut s, middle, ends) = many_ends(&[0.0, 0.03, 0.10]);
    s.remove_entities(&[middle]);
    assert_eq!(pairs(&s), vec![(ends[0], ends[1])]);
    assert!(s.entity(ends[2]).is_some(), "余った端も位置も消さない");
    assert!(
        !tied(&s, ends[0], ends[2]),
        "1対1の規則は3残端の全連結を意味しない"
    );
}
#[test]
fn four_remaining_ends_use_each_point_in_only_one_new_pair() {
    let (mut s, middle, ends) = many_ends(&[0.0, 0.03, 0.10, 0.11]);
    s.remove_entities(&[middle]);
    assert_eq!(pairs(&s), vec![(ends[2], ends[3]), (ends[0], ends[1])]);
    let used: Vec<_> = pairs(&s).into_iter().flat_map(|(a, b)| [a, b]).collect();
    assert_eq!(
        used.iter().copied().collect::<BTreeSet<_>>().len(),
        used.len()
    );
    assert!(
        !tied(&s, ends[0], ends[2]),
        "点対を代表点の星形や鎖に替えない"
    );
}
