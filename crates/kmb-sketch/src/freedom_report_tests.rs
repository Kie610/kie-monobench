//! 確かめること: 要素が共有する自由な動きと、各要素へ現れる動きの次元。
use super::*;

#[test]
fn a_partly_constrained_sketch_reports_free_points_lines_and_radius() {
    let mut s = Sketch::default();
    let a = s.add_point(0.0, 0.0);
    let b = s.add_point(10.0, 0.0);
    let line = s.add_line(a, b);
    s.add_constraint(ConstraintKind::Fix { point: a });
    s.add_constraint(ConstraintKind::Horizontal { line });
    let p = s.add_point(20.0, 5.0);
    let center = s.add_point(30.0, 0.0);
    let circle = s.add_circle(center, 4.0);
    s.add_constraint(ConstraintKind::Fix { point: center });
    let values = HashMap::new();
    let sol = solve(&s, &values).unwrap();
    let r = freedom_report(&s, &sol, &values).unwrap();
    assert_eq!(r.dof, 4);
    assert_eq!(
        r.entities,
        BTreeMap::from([(b, 1), (line, 1), (p, 2), (circle, 1)])
    );
}

#[test]
fn a_point_that_can_only_rotate_has_one_freedom_not_two() {
    let mut s = Sketch::default();
    let a = s.add_point(0.0, 0.0);
    let b = s.add_point(6.0, 8.0);
    let line = s.add_line(a, b);
    s.add_constraint(ConstraintKind::Fix { point: a });
    let c = s.add_constraint(ConstraintKind::Length {
        line,
        value: "10 mm".into(),
    });
    let values = HashMap::from([(c, 10.0)]);
    let sol = solve(&s, &values).unwrap();
    let r = freedom_report(&s, &sol, &values).unwrap();
    assert_eq!(r.dof, 1);
    assert_eq!(r.entities, BTreeMap::from([(b, 1), (line, 1)]));
}

#[test]
fn shared_motion_is_not_added_and_fixed_or_empty_sketches_have_no_rows() {
    let mut s = Sketch::default();
    let a = s.add_point(0.0, 0.0);
    let b = s.add_point(10.0, 0.0);
    let line = s.add_line(a, b);
    s.add_constraint(ConstraintKind::Horizontal { line });
    let values = HashMap::new();
    let sol = solve(&s, &values).unwrap();
    let r = freedom_report(&s, &sol, &values).unwrap();
    assert_eq!(r.dof, 3);
    assert_eq!(r.entities, BTreeMap::from([(a, 2), (b, 2), (line, 3)]));
    s.add_constraint(ConstraintKind::Fix { point: a });
    s.add_constraint(ConstraintKind::Fix { point: b });
    let sol = solve(&s, &values).unwrap();
    let r = freedom_report(&s, &sol, &values).unwrap();
    assert_eq!(r.dof, 0);
    assert!(r.entities.is_empty());
    let s = Sketch::default();
    let sol = solve(&s, &values).unwrap();
    let r = freedom_report(&s, &sol, &values).unwrap();
    assert_eq!(r.dof, 0);
    assert!(r.entities.is_empty());
}
