//! 図形どうしの対称(2 組の点の対称を 1 つの拘束にした物)と、一部が重なる拘束の余分の判定(10-24 t4)。

use crate::tests::{e, free_rectangle};
use crate::*;
use ConstraintKind as K;
use std::collections::HashMap;

/// 幅 40 の寸法を付けた長方形(点 1〜4、0..40 × 0..20)と、軸(固定した 2 点の線)。
fn rectangle_and_axis(to: [f64; 2]) -> (Sketch, EntityId, HashMap<ConstraintId, f64>) {
    let mut s = free_rectangle();
    s.constraints.push(Constraint::new(
        ConstraintId(20),
        K::Length {
            line: e(11),
            value: "40 mm".into(),
        },
    ));
    let a = s.add_point(25.0, -10.0);
    let b = s.add_point(to[0], to[1]);
    let axis = s.add_line(a, b);
    s.add_constraint(K::Fix { point: a });
    s.add_constraint(K::Fix { point: b });
    (s, axis, [(ConstraintId(20), 40.0)].into_iter().collect())
}

/// 左の辺(4→1)と右の辺(3→2)を組にした対称。1↔2、4↔3。
fn pairs(axis: EntityId) -> K {
    K::SymmetricPairs {
        a1: e(1),
        b1: e(2),
        a2: e(4),
        b2: e(3),
        line: axis,
    }
}

fn status(s: &Sketch, v: &HashMap<ConstraintId, f64>) -> (SolveStatus, Solution) {
    let sol = solve(s, v).unwrap();
    (sol.status.clone(), sol)
}

#[test]
fn sides_of_a_dimensioned_rectangle_take_one_pair_symmetry() {
    let (mut s, axis, v) = rectangle_and_axis([25.0, 40.0]);
    s.add_constraint(pairs(axis));
    let (st, sol) = status(&s, &v);
    assert!(
        matches!(
            st,
            SolveStatus::WellConstrained | SolveStatus::UnderConstrained { .. }
        ),
        "{st:?}"
    );
    // 長方形は軸(x = 25)の真ん中へ寄る
    assert!(
        (sol.points[&e(1)][0] - 5.0).abs() < 1e-6,
        "{:?}",
        sol.points[&e(1)]
    );
    assert!(
        (sol.points[&e(3)][0] - 45.0).abs() < 1e-6,
        "{:?}",
        sol.points[&e(3)]
    );
}

#[test]
fn two_separate_point_symmetries_still_overlap() {
    // 点の対称を 2 つ別々に付けると、2 つ目は全部が 1 つ目と辺の拘束で決まるので余分
    let (mut s, axis, v) = rectangle_and_axis([25.0, 40.0]);
    s.add_constraint(K::Symmetric {
        a: e(1),
        b: e(2),
        line: axis,
    });
    let second = s.add_constraint(K::Symmetric {
        a: e(4),
        b: e(3),
        line: axis,
    });
    let (st, _) = status(&s, &v);
    assert_eq!(
        st,
        SolveStatus::OverConstrained {
            redundant: vec![second]
        }
    );
}

#[test]
fn one_point_symmetry_on_a_horizontal_side_is_taken() {
    // 上の 2 隅だけ: 結ぶ線が軸に直角なのは上の辺の水平で決まっているが、中点が軸の上は新しい
    let (mut s, axis, v) = rectangle_and_axis([25.0, 40.0]);
    s.add_constraint(K::Symmetric {
        a: e(4),
        b: e(3),
        line: axis,
    });
    let (st, _) = status(&s, &v);
    assert!(
        !matches!(
            st,
            SolveStatus::OverConstrained { .. } | SolveStatus::Inconsistent { .. }
        ),
        "{st:?}"
    );
}

#[test]
fn the_same_symmetry_twice_is_redundant() {
    let (mut s, axis, v) = rectangle_and_axis([25.0, 40.0]);
    s.add_constraint(pairs(axis));
    let again = s.add_constraint(K::Symmetric {
        a: e(4),
        b: e(3),
        line: axis,
    });
    let (st, _) = status(&s, &v);
    assert_eq!(
        st,
        SolveStatus::OverConstrained {
            redundant: vec![again]
        }
    );
}

#[test]
fn a_slanted_axis_cannot_mirror_vertical_sides() {
    let (mut s, axis, v) = rectangle_and_axis([45.0, 10.0]);
    s.add_constraint(pairs(axis));
    let (st, _) = status(&s, &v);
    assert!(matches!(st, SolveStatus::Inconsistent { .. }), "{st:?}");
}

#[test]
fn pair_symmetry_round_trips_and_follows_deleted_points() {
    let (mut s, axis, _) = rectangle_and_axis([25.0, 40.0]);
    let id = s.add_constraint(pairs(axis));
    let json = serde_json::to_string(&s).unwrap();
    let back: Sketch = serde_json::from_str(&json).unwrap();
    assert_eq!(back.constraints, s.constraints);
    // 組の点を消すと拘束も消える
    s.remove_entities(&[e(3)]);
    assert!(s.constraints.iter().all(|c| c.id != id));
}
