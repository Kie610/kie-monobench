//! 確かめること: 3点の円の点・線への吸い付きの拘束と再計算した位置。

use super::*;
use crate::view::{Editor, Tool};

fn draw_circle(before: &Sketch, at: [[f64; 2]; 3]) -> (Sketch, EntityId) {
    let sol = kmb_sketch::solve(before, &Default::default()).unwrap();
    let mut editor = Editor::default();
    editor.set_tool(Tool::ThreePointCircle);
    let mut result = None;
    for cursor in at {
        let snapped = snap_cursor(before, &sol, cursor, 0.1, &editor.infer);
        if let Some(curve) = snapped.on {
            editor.infer.placed.push((snapped.pos, curve));
        }
        let placed = editor.infer.placed.clone();
        if let Some(mut made) = crate::more_tools::interact(
            &mut editor,
            &crate::more_tools::Input {
                sketch: before,
                sol: &sol,
                cursor: Some(snapped.pos),
                clicked: true,
                enter: false,
                tol: 0.1,
                shift: false,
            },
        ) {
            attach_placed(before, &mut made, &placed, None);
            result = Some(made);
        }
    }
    let made = result.expect("3クリックで円を作る");
    let circle = made
        .entities
        .iter()
        .find(|e| before.entity(e.id()).is_none() && matches!(e, Entity::Circle { .. }))
        .unwrap()
        .id();
    (made, circle)
}

fn fixed(s: &mut Sketch, at: [f64; 2]) -> EntityId {
    let point = s.add_point(at[0], at[1]);
    s.add_constraint(ConstraintKind::Fix { point });
    point
}

fn assert_recompute_stays_at_the_drawn_circle(s: &Sketch, circle: EntityId) {
    let Entity::Circle { center, radius, .. } = s.entity(circle).unwrap() else {
        panic!("円")
    };
    let Entity::Point { x, y, .. } = s.entity(*center).unwrap() else {
        panic!("中心")
    };
    let expected = [*x, *y, *radius];
    let mut again = s.clone();
    for _ in 0..5 {
        let sol = kmb_sketch::solve(&again, &Default::default()).unwrap();
        let actual = [
            sol.points[center][0],
            sol.points[center][1],
            sol.radii[&circle],
        ];
        for (a, b) in actual.into_iter().zip(expected) {
            assert!((a - b).abs() < 1e-8, "{actual:?} != {expected:?}");
        }
        again.adopt_solution(&sol);
    }
}

#[test]
fn three_point_circle_keeps_the_third_existing_point_on_its_circumference() {
    let mut s = Sketch::default();
    let at = [[0.0, 0.0], [10.0, 0.0], [5.0, 10.0]];
    let points = at.map(|p| fixed(&mut s, p));
    let (made, circle) = draw_circle(&s, at);
    for point in points {
        assert!(
            made.constraints
                .iter()
                .any(|c| c.kind == ConstraintKind::PointOnCircle { point, circle })
        );
    }
    assert_recompute_stays_at_the_drawn_circle(&made, circle);
}

#[test]
fn three_point_circle_keeps_its_snapped_third_point_on_the_line() {
    let mut s = Sketch::default();
    fixed(&mut s, [0.0, 0.0]);
    fixed(&mut s, [10.0, 0.0]);
    let a = fixed(&mut s, [-20.0, 10.0]);
    let b = fixed(&mut s, [20.0, 10.0]);
    let line = s.add_line(a, b);
    let (made, circle) = draw_circle(&s, [[0.0, 0.0], [10.0, 0.0], [5.0, 10.02]]);
    let point = made
        .constraints
        .iter()
        .find_map(|c| match c.kind {
            ConstraintKind::PointOnLine { point, line: l } if l == line => Some(point),
            _ => None,
        })
        .expect("吸い付いた線上の拘束を付ける");
    assert!(
        made.constraints
            .iter()
            .any(|c| c.kind == ConstraintKind::PointOnCircle { point, circle })
    );
    assert_recompute_stays_at_the_drawn_circle(&made, circle);
}

#[test]
fn three_point_circle_without_a_snap_does_not_add_constraints() {
    let (made, circle) = draw_circle(&Sketch::default(), [[0.0, 0.0], [10.0, 0.0], [5.0, 10.0]]);
    assert!(made.constraints.is_empty());
    assert_recompute_stays_at_the_drawn_circle(&made, circle);
}
