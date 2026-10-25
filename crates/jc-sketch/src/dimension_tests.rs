//! 円周から測る距離と、中心線からの直径の寸法。

use crate::*;
use std::collections::HashMap;

fn values_of(s: &Sketch) -> HashMap<ConstraintId, f64> {
    dimension_values(s, &|_| None).unwrap()
}

/// 寸法 `kind` を値 `v` mm で付けて解き、測り直した値。
fn solve_with(mut s: Sketch, kind: ConstraintKind, v: f64) -> f64 {
    let mut k = kind;
    *k.value_mut().unwrap() = format!("{v} mm");
    let id = s.add_constraint(k);
    let sol = solve(&s, &values_of(&s)).unwrap();
    let c = s.constraints.iter().find(|c| c.id == id).unwrap();
    measure(&s, &sol, &c.kind).unwrap()
}

/// 原点に固定した半径 5 の円(半径の寸法つき)と、ほかの要素を足す前のスケッチ。
fn fixed_circle() -> (Sketch, EntityId, EntityId) {
    let mut s = Sketch::default();
    let c = s.add_point(0.0, 0.0);
    s.add_constraint(ConstraintKind::Fix { point: c });
    let circle = s.add_circle(c, 5.0);
    s.add_constraint(ConstraintKind::Radius {
        circle,
        value: "5 mm".into(),
    });
    (s, c, circle)
}

#[test]
fn distance_from_a_circle_edge_to_a_point_a_line_and_a_circle() {
    // 点: 近い側(外)と遠い側
    let (mut s, _, circle) = fixed_circle();
    let p = s.add_point(20.0, 0.0);
    for (far, v) in [(false, 12.0), (true, 30.0)] {
        let got = solve_with(
            s.clone(),
            ConstraintKind::CircleDistance {
                circle,
                other: p,
                far,
                value: String::new(),
            },
            v,
        );
        assert!((got - v).abs() < 1e-6, "点 far={far}: {got}");
    }
    // 直線: 近い側
    let (mut s, _, circle) = fixed_circle();
    let a = s.add_point(-10.0, 12.0);
    let b = s.add_point(10.0, 12.0);
    let l = s.add_line(a, b);
    s.add_constraint(ConstraintKind::Horizontal { line: l });
    let got = solve_with(
        s.clone(),
        ConstraintKind::CircleDistance {
            circle,
            other: l,
            far: false,
            value: String::new(),
        },
        3.0,
    );
    assert!((got - 3.0).abs() < 1e-6, "直線: {got}");
    // 円どうし: 外どうしの近い側と遠い側
    let (mut s, _, circle) = fixed_circle();
    let c2 = s.add_point(30.0, 0.0);
    let other = s.add_circle(c2, 2.0);
    s.add_constraint(ConstraintKind::Radius {
        circle: other,
        value: "2 mm".into(),
    });
    for (far, v) in [(false, 10.0), (true, 40.0)] {
        let got = solve_with(
            s.clone(),
            ConstraintKind::CircleDistance {
                circle,
                other,
                far,
                value: String::new(),
            },
            v,
        );
        assert!((got - v).abs() < 1e-6, "円 far={far}: {got}");
    }
}

#[test]
fn a_centerline_dimension_is_twice_the_distance_and_is_the_default() {
    let mut s = Sketch::default();
    let a = s.add_point(0.0, 0.0);
    let b = s.add_point(0.0, 50.0);
    let axis = s.add_line(a, b);
    s.add_constraint(ConstraintKind::Fix { point: a });
    s.add_constraint(ConstraintKind::Vertical { line: axis });
    s.toggle_centerline(axis);
    let p = s.add_point(8.0, 10.0);
    let sol = solve(&s, &values_of(&s)).unwrap();
    // 中心線と点を選ぶと直径の寸法になる
    let k = dimension_for(&s, &sol, &[axis, p]).unwrap();
    assert!(matches!(k, ConstraintKind::LinearDiameter { .. }), "{k:?}");
    assert!((measure(&s, &sol, &k).unwrap() - 16.0).abs() < 1e-9);
    let got = solve_with(s.clone(), k, 30.0);
    assert!((got - 30.0).abs() < 1e-6, "{got}");
    // 保存して読み戻せる
    let mut t = s.clone();
    t.add_constraint(ConstraintKind::LinearDiameter {
        point: p,
        line: axis,
        value: "30 mm".into(),
    });
    let back: Sketch = serde_json::from_str(&serde_json::to_string(&t).unwrap()).unwrap();
    assert_eq!(back, t);
}

#[test]
fn a_circle_and_a_point_are_measured_from_the_center_by_default() {
    let (mut s, c, circle) = fixed_circle();
    let p = s.add_point(20.0, 0.0);
    let sol = solve(&s, &values_of(&s)).unwrap();
    let k = dimension_for(&s, &sol, &[circle, p]).unwrap();
    assert_eq!(
        k,
        ConstraintKind::Distance {
            a: c,
            b: p,
            value: String::new()
        }
    );
}
