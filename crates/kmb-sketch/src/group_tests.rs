//! まとまりの拘束(オフセット・ポリゴン・パターンの並び)と、円と中心の別々の固定の試験。

use crate::*;
use std::collections::HashMap;
use std::f64::consts::TAU;

/// 寸法の値を測った値のまま(`set` にある寸法だけ置き換えて)解く。
fn solve_with(s: &Sketch, set: &[(ConstraintId, f64)]) -> Solution {
    let mut values: HashMap<ConstraintId, f64> = HashMap::new();
    let pos: Solution = Solution {
        status: SolveStatus::WellConstrained,
        dof: 0,
        points: s
            .entities
            .iter()
            .filter_map(|e| match e {
                Entity::Point { id, x, y } => Some((*id, [*x, *y])),
                _ => None,
            })
            .collect(),
        radii: s
            .entities
            .iter()
            .filter_map(|e| match e {
                Entity::Circle { id, radius, .. } => Some((*id, *radius)),
                _ => None,
            })
            .collect(),
        fixed: Default::default(),
        lone: vec![],
        iterations: 0,
        max_residual: 0.0,
        measured: Default::default(),
    };
    for c in s.constraints.iter().filter(|c| c.kind.value().is_some()) {
        values.insert(c.id, measure(s, &pos, &c.kind).expect("測れる"));
    }
    for (id, v) in set {
        values.insert(*id, *v);
    }
    solve(s, &values).expect("解ける")
}

fn of(s: &Sketch, f: impl Fn(&ConstraintKind) -> bool) -> Vec<&Constraint> {
    s.constraints.iter().filter(|c| f(&c.kind)).collect()
}

fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// 点から線分を延ばした直線までの距離。
fn to_line(sol: &Solution, s: &Sketch, p: EntityId, line: EntityId) -> f64 {
    let Some(Entity::Line { a, b, .. }) = s.entity(line) else {
        panic!("線分")
    };
    let (a, b, p) = (sol.points[a], sol.points[b], sol.points[&p]);
    let d = [b[0] - a[0], b[1] - a[1]];
    ((p[0] - a[0]) * d[1] - (p[1] - a[1]) * d[0]).abs() / d[0].hypot(d[1])
}

/// 位置と大きさを固定した 20×10 の矩形(左下の点の固定、幅と高さの寸法)。
fn fixed_rectangle() -> (Sketch, [EntityId; 4]) {
    let mut s = Sketch::default();
    let l = s
        .add_rectangle((None, [0.0, 0.0]), (None, [20.0, 10.0]))
        .unwrap();
    let Some(Entity::Line { a, .. }) = s.entity(l[0]).cloned() else {
        unreachable!()
    };
    s.add_constraint(ConstraintKind::Fix { point: a });
    for (line, v) in [(l[0], "20 mm"), (l[1], "10 mm")] {
        s.add_constraint(ConstraintKind::Length {
            line,
            value: v.into(),
        });
    }
    (s, l)
}

#[test]
fn an_offset_is_one_constraint_with_one_dimension() {
    let (mut s, l) = fixed_rectangle();
    let before = s.constraints.len();
    let new = s.offset(&l, 2.0, "2 mm", true);
    assert_eq!(new.len(), 4);
    // 足したのはオフセットの拘束 1 つと、距離の寸法 1 つだけ(辺ごとの平行・距離は出さない)
    assert_eq!(s.constraints.len(), before + 2, "{:?}", s.constraints);
    assert_eq!(
        of(&s, |k| matches!(k, ConstraintKind::Offset { .. })).len(),
        1
    );
    let dims = of(&s, |k| {
        matches!(k, ConstraintKind::PointLineDistance { .. })
    });
    assert_eq!(dims.len(), 1);
    let dim = dims[0].id;
    let sol = solve_with(&s, &[]);
    assert_eq!(sol.status, SolveStatus::WellConstrained, "{:?}", sol.status);
    // 寸法を 5 にすると、4 辺とも 5 mm 離れる
    let sol = solve_with(&s, &[(dim, 5.0)]);
    assert_eq!(sol.status, SolveStatus::WellConstrained, "{:?}", sol.status);
    for (old, new) in l.iter().zip(&new) {
        let Some(Entity::Line { a, b, .. }) = s.entity(*new) else {
            panic!()
        };
        for p in [a, b] {
            assert!((to_line(&sol, &s, *p, *old) - 5.0).abs() < 1e-6);
        }
    }
    // 寸法を消しても同じ距離の関係は残る(動けるのは距離の 1 つだけ)
    s.remove_constraint(dim);
    let sol = solve_with(&s, &[]);
    assert_eq!(sol.status, SolveStatus::UnderConstrained { dof: 1 });
}

#[test]
fn a_rounded_offset_puts_arcs_on_the_old_corners() {
    let (mut s, l) = fixed_rectangle();
    let corners: Vec<EntityId> = l
        .iter()
        .filter_map(|id| match s.entity(*id) {
            Some(Entity::Line { a, .. }) => Some(*a),
            _ => None,
        })
        .collect();
    let new = s.offset_corners(&l, 2.0, "2 mm", true, true);
    let arcs: Vec<(EntityId, EntityId)> = new
        .iter()
        .filter_map(|id| match s.entity(*id) {
            Some(Entity::Arc { center, start, .. }) => Some((*center, *start)),
            _ => None,
        })
        .collect();
    assert_eq!(new.len(), 8, "辺 4 本と角の円弧 4 つ");
    assert_eq!(arcs.len(), 4);
    for (c, _) in &arcs {
        assert!(corners.contains(c), "角の点が中心");
    }
    assert_eq!(
        of(&s, |k| matches!(k, ConstraintKind::Offset { .. })).len(),
        1
    );
    let dim = of(&s, |k| {
        matches!(k, ConstraintKind::PointLineDistance { .. })
    })[0]
        .id;
    let sol = solve_with(&s, &[(dim, 3.0)]);
    assert_eq!(sol.status, SolveStatus::WellConstrained, "{:?}", sol.status);
    for (c, p) in arcs {
        assert!((dist(sol.points[&c], sol.points[&p]) - 3.0).abs() < 1e-6);
    }
    // 内へずらす時は角が閉じるので、丸めずに交わる所で角を作る
    let (mut s, l) = fixed_rectangle();
    let new = s.offset_corners(&l, 2.0, "2 mm", false, true);
    assert_eq!(new.len(), 4);
    assert_eq!(solve_with(&s, &[]).status, SolveStatus::WellConstrained);
}

#[test]
fn deleting_one_offset_line_keeps_the_rest_of_the_offset() {
    let (mut s, l) = fixed_rectangle();
    let new = s.offset(&l, 2.0, "2 mm", true);
    s.remove_entities(&[new[0]]);
    let off = of(&s, |k| matches!(k, ConstraintKind::Offset { .. }));
    assert_eq!(off.len(), 1);
    let ConstraintKind::Offset { parts } = &off[0].kind else {
        unreachable!()
    };
    assert!(parts.iter().all(|p| !p.refs().contains(&new[0])));
}

#[test]
fn a_polygon_is_one_constraint_and_its_sides_can_be_changed() {
    let mut s = Sketch::default();
    let sides = s
        .add_polygon((None, [0.0, 0.0]), (None, [10.0, 0.0]), 6)
        .unwrap();
    let polys = of(&s, |k| matches!(k, ConstraintKind::Polygon { .. }));
    assert_eq!(polys.len(), 1, "ポリゴンの拘束 1 つ");
    assert!(of(&s, |k| matches!(k, ConstraintKind::Equal { .. })).is_empty());
    assert!(of(&s, |k| matches!(k, ConstraintKind::PointOnCircle { .. })).is_empty());
    let id = polys[0].id;
    assert_eq!(s.polygon_of(sides[3]), Some(id));
    // 正多角形の自由度は、中心 2・大きさ 1・回り 1
    assert_eq!(solve_with(&s, &[]).dof, 4);
    // 最初の辺に付けた寸法は辺の数を変えても残る
    let len = s.add_constraint(ConstraintKind::Length {
        line: sides[0],
        value: "10 mm".into(),
    });
    assert!(s.set_polygon_sides(id, 8));
    let ConstraintKind::Polygon { sides: now, .. } =
        &s.constraints.iter().find(|c| c.id == id).unwrap().kind
    else {
        panic!("番号は変わらない")
    };
    assert_eq!(now.len(), 8);
    assert_eq!(now[0], sides[0]);
    assert!(s.constraints.iter().any(|c| c.id == len));
    let sol = solve_with(&s, &[(len, 4.0)]);
    assert_eq!(sol.dof, 3);
    let lines: Vec<f64> = now
        .iter()
        .map(|l| match s.entity(*l) {
            Some(Entity::Line { a, b, .. }) => dist(sol.points[a], sol.points[b]),
            _ => panic!(),
        })
        .collect();
    assert!(lines.iter().all(|x| (x - 4.0).abs() < 1e-6), "{lines:?}");
    // 線分は 8 本だけ(前の辺は消える)
    assert_eq!(
        s.entities
            .iter()
            .filter(|e| matches!(e, Entity::Line { .. }))
            .count(),
        8
    );
    assert!(!s.set_polygon_sides(id, 2));
}

/// 中心を固定した半径 1 の円(中心 (10, 0))と半径の寸法。
fn source_circle(s: &mut Sketch) -> (EntityId, EntityId, ConstraintId) {
    let c = s.add_point(10.0, 0.0);
    s.add_constraint(ConstraintKind::Fix { point: c });
    let circle = s.add_circle(c, 1.0);
    let r = s.add_constraint(ConstraintKind::Radius {
        circle,
        value: "1 mm".into(),
    });
    (c, circle, r)
}

#[test]
fn a_pattern_keeps_its_copies_on_the_original() {
    let mut s = Sketch::default();
    let (_, circle, r) = source_circle(&mut s);
    let before = s.constraints.len();
    let id = s
        .add_circular_pattern(CircularPattern {
            entities: vec![circle],
            center: PatternCenter::At([0.0, 0.0]),
            count: 4,
            spacing: AngularSpacing::Total,
            angle: TAU,
            suppressed: vec![],
        })
        .unwrap();
    // 写しに拘束を写さず、並びの拘束 1 つだけを足す
    assert_eq!(s.constraints.len(), before + 1);
    assert!(s.pattern_constraint(id).is_some());
    let sol = solve_with(&s, &[(r, 2.5)]);
    assert_eq!(sol.status, SolveStatus::WellConstrained, "{:?}", sol.status);
    for c in &s.pattern(id).unwrap().copies {
        for e in &c.entities {
            if let Some(Entity::Circle { .. }) = s.entity(*e) {
                assert!((sol.radii[e] - 2.5).abs() < 1e-9, "写しも元の大きさ");
            }
        }
    }
    // 並びの拘束を消すと、パターンの定義も消える(写しは残る)
    let n = s.entities.len();
    s.remove_constraint(s.pattern_constraint(id).unwrap());
    assert!(s.patterns.is_empty());
    assert_eq!(s.entities.len(), n);
}

#[test]
fn moving_the_original_moves_the_copies() {
    let mut s = Sketch::default();
    let c = s.add_point(10.0, 0.0);
    let circle = s.add_circle(c, 1.0);
    let o = s.add_point(0.0, 0.0);
    s.add_constraint(ConstraintKind::Fix { point: o });
    let dx = s.add_constraint(ConstraintKind::HorizontalDistance {
        a: o,
        b: c,
        value: "10 mm".into(),
    });
    s.add_constraint(ConstraintKind::VerticalDistance {
        a: o,
        b: c,
        value: "0 mm".into(),
    });
    let id = s
        .add_rectangular_pattern(RectangularPattern {
            entities: vec![circle],
            directions: [
                PatternDirection {
                    axis: PatternAxis::Angle(0.0),
                    count: 3,
                    distance: 5.0,
                    distance_type: DistanceType::Spacing,
                    symmetric: false,
                },
                PatternDirection {
                    axis: PatternAxis::Angle(TAU / 4.0),
                    count: 1,
                    distance: 5.0,
                    distance_type: DistanceType::Spacing,
                    symmetric: false,
                },
            ],
            suppressed: vec![],
        })
        .unwrap();
    let sol = solve_with(&s, &[(dx, 20.0)]);
    let mut xs: Vec<f64> = s
        .pattern(id)
        .unwrap()
        .copies
        .iter()
        .flat_map(|cp| cp.entities.iter())
        .filter_map(|e| match s.entity(*e) {
            Some(Entity::Circle { center, .. }) => Some(sol.points[center][0]),
            _ => None,
        })
        .collect();
    xs.sort_by(f64::total_cmp);
    assert!(
        (xs[0] - 25.0).abs() < 1e-6 && (xs[1] - 30.0).abs() < 1e-6,
        "{xs:?}"
    );
}

#[test]
fn old_patterns_drop_their_copied_constraints_on_reading() {
    let mut s = Sketch::default();
    let (_, circle, _) = source_circle(&mut s);
    let id = s
        .add_circular_pattern(CircularPattern {
            entities: vec![circle],
            center: PatternCenter::At([0.0, 0.0]),
            count: 3,
            spacing: AngularSpacing::Total,
            angle: TAU,
            suppressed: vec![],
        })
        .unwrap();
    // 古い書き方: 写しごとに半径の寸法を写し、並びの拘束も対応も無い
    let mut old = s.clone();
    old.constraints
        .retain(|c| !matches!(c.kind, ConstraintKind::Pattern { .. }));
    let copies: Vec<EntityId> = old.patterns[0]
        .copies
        .iter()
        .flat_map(|c| c.entities.clone())
        .collect();
    for (k, cp) in old.patterns[0].copies.clone().iter().enumerate() {
        let cc = cp
            .entities
            .iter()
            .copied()
            .find(|e| matches!(old.entity(*e), Some(Entity::Circle { .. })))
            .unwrap();
        let rid = old.add_constraint(ConstraintKind::Radius {
            circle: cc,
            value: "1 mm".into(),
        });
        old.patterns[0].copies[k].constraints.push(rid);
        old.patterns[0].copies[k].from.clear();
    }
    let text = serde_json::to_string(&old).unwrap();
    let back: Sketch = serde_json::from_str(&text).unwrap();
    assert!(back.pattern_constraint(id).is_some());
    assert!(
        back.patterns[0]
            .copies
            .iter()
            .all(|c| c.constraints.is_empty() && c.from.len() == c.entities.len())
    );
    assert!(!back.constraints.iter().any(|c| matches!(
        c.kind,
        ConstraintKind::Radius { circle, .. } if copies.contains(&circle)
    )));
    let sol = solve_with(&back, &[]);
    assert_eq!(sol.status, SolveStatus::WellConstrained, "{:?}", sol.status);
}

#[test]
fn a_circle_and_its_center_are_fixed_separately() {
    let mut s = Sketch::default();
    let c = s.add_point(0.0, 0.0);
    let circle = s.add_circle(c, 5.0);
    // 中心だけ: 大きさは動く
    let fc = s.add_constraint(ConstraintKind::Fix { point: c });
    assert_eq!(solve_with(&s, &[]).dof, 1);
    // 円も固定: 余分にならずに全部止まる
    let fr = s.add_constraint(ConstraintKind::Fix { point: circle });
    let sol = solve_with(&s, &[]);
    assert_eq!(sol.status, SolveStatus::WellConstrained, "{:?}", sol.status);
    // 中心の固定を外しても、円の固定で止まったまま
    s.remove_constraint(fc);
    assert_eq!(solve_with(&s, &[]).status, SolveStatus::WellConstrained);
    // 円の固定を外し、中心だけ固定し直すと大きさが動く
    s.remove_constraint(fr);
    s.add_constraint(ConstraintKind::Fix { point: c });
    assert_eq!(solve_with(&s, &[]).dof, 1);
}

#[test]
fn an_inward_offset_that_turns_the_lines_over_is_refused() {
    let mut s = Sketch::default();
    let l = s
        .add_rectangle((None, [0.0, 0.0]), (None, [11.2, 7.5]))
        .unwrap();
    let before = s.clone();
    // 内へ 4 mm は高さ 7.5 を超えて上下の線が入れ替わる: 作らず、スケッチも変えない
    assert!(s.offset(&l, 4.0, "4 mm", false).is_empty());
    assert_eq!(s, before);
    // 外へは大きくても作れる
    assert_eq!(s.offset(&l, 8.0, "8 mm", true).len(), 4);
}

#[test]
fn trimming_an_offset_line_keeps_the_offset_of_the_others() {
    // 写しの下の辺(y = -2)を縦の線で切り、左の区間を消す。オフセットの拘束は 1 つなので、
    // 前は切った辺の関係と一緒に、残りの 3 辺の関係まで黙って消えていた
    let (mut s, l) = fixed_rectangle();
    let new = s.offset(&l, 2.0, "2 mm", true);
    let (p, q) = (s.add_point(10.0, -5.0), s.add_point(10.0, 1.0));
    s.add_line(p, q);
    assert!(s.trim(new[0], [5.0, -2.0]));
    let offsets = of(&s, |k| matches!(k, ConstraintKind::Offset { .. }));
    assert_eq!(
        offsets.len(),
        1,
        "オフセットの拘束が残る: {:?}",
        s.constraints
    );
    let ConstraintKind::Offset { parts } = &offsets[0].kind else {
        unreachable!()
    };
    let refs: Vec<EntityId> = parts.iter().flat_map(|p| p.refs()).collect();
    for side in &new[1..] {
        assert!(refs.contains(side), "ほかの辺の関係は残る");
    }
    assert!(!refs.contains(&new[0]), "消えた線を指さない");
    // 切った片(右の区間)も元の辺と平行のまま
    let piece = s
        .entities
        .iter()
        .find_map(|e| match e {
            Entity::Line { id, a, b, .. }
                if !new.contains(id) && !l.contains(id) && *a != p && *b != p =>
            {
                Some(*id)
            }
            _ => None,
        })
        .expect("切った片");
    assert!(refs.contains(&piece), "切った片へ付け替える");
}

// 他のソフトで起きた不具合が JC では起きないことを、症状のまま確かめる(判定「試験だけ」)。

/// 解けた(余分な拘束も矛盾も無い)。
fn solved(sol: &Solution) -> bool {
    matches!(
        sol.status,
        SolveStatus::WellConstrained | SolveStatus::UnderConstrained { .. }
    )
}

#[test]
fn extending_an_offset_line_keeps_the_offset_distance() {
    // 10-22 t5: オフセットした線を延長しても、何も起きなかったり距離が崩れたりしない
    let (mut s, l) = fixed_rectangle();
    let new = s.offset(&l[..1], 2.0, "2 mm", true);
    assert_eq!(new.len(), 1);
    let (p, q) = (s.add_point(30.0, -10.0), s.add_point(30.0, 10.0));
    s.add_line(p, q);
    let Some(Entity::Line { a, b, .. }) = s.entity(new[0]).cloned() else {
        panic!("線分")
    };
    let right = |s: &Sketch, p: EntityId| match s.entity(p) {
        Some(Entity::Point { x, .. }) => *x,
        _ => panic!("点"),
    };
    let end = if right(&s, a) > right(&s, b) { a } else { b };
    let Some(Entity::Point { x, y, .. }) = s.entity(end).cloned() else {
        panic!()
    };
    assert!(s.extend(new[0], [x, y]), "延ばせる");
    let sol = solve_with(&s, &[]);
    assert!(solved(&sol), "{:?}", sol.status);
    let Some(Entity::Line { a, b, .. }) = s.entity(new[0]).cloned() else {
        panic!("線分")
    };
    let xs = [sol.points[&a][0], sol.points[&b][0]];
    assert!(
        xs.iter().any(|x| (x - 30.0).abs() < 1e-6),
        "縦の線まで: {xs:?}"
    );
    for p in [a, b] {
        assert!(
            (to_line(&sol, &s, p, l[0]) - 2.0).abs() < 1e-6,
            "距離 2 のまま"
        );
    }
}

#[test]
fn filleting_a_corner_of_an_offset_keeps_it_solvable() {
    // 10-23 t1: オフセットした形の角を丸めても、角が丸ごとの円にならずスケッチが壊れない
    let (mut s, l) = fixed_rectangle();
    let new = s.offset(&l, 2.0, "2 mm", true);
    let ends = |s: &Sketch, id: EntityId| match s.entity(id) {
        Some(Entity::Line { a, b, .. }) => [*a, *b],
        _ => panic!("線分"),
    };
    let (e0, e1) = (ends(&s, new[0]), ends(&s, new[1]));
    let corner = *e0.iter().find(|p| e1.contains(p)).expect("角を共有する");
    let arc = s.fillet_corner(corner, 1.0, "1 mm").expect("丸められる");
    let sol = solve_with(&s, &[]);
    assert!(solved(&sol), "{:?}", sol.status);
    let Some(Entity::Arc {
        center, start, end, ..
    }) = s.entity(arc).cloned()
    else {
        panic!("円弧")
    };
    for p in [start, end] {
        let r = dist(sol.points[&center], sol.points[&p]);
        assert!((r - 1.0).abs() < 1e-6, "半径 1 の円弧のまま: {r}");
    }
    for (old, new) in l.iter().zip(&new) {
        for p in ends(&s, *new) {
            assert!(
                (to_line(&sol, &s, p, *old) - 2.0).abs() < 1e-6,
                "距離 2 のまま"
            );
        }
    }
}

#[test]
fn the_original_of_an_offset_can_still_take_a_horizontal_and_an_angle() {
    // 10-24 t1: 固定していない形をオフセットした後でも、元の辺に水平と角度を付けられる
    let mut s = Sketch::default();
    let ps: Vec<EntityId> = [[0.0, 0.0], [20.0, 1.0], [22.0, 12.0], [1.0, 10.0]]
        .iter()
        .map(|[x, y]| s.add_point(*x, *y))
        .collect();
    let l: Vec<EntityId> = (0..4).map(|i| s.add_line(ps[i], ps[(i + 1) % 4])).collect();
    let new = s.offset(&l, 2.0, "2 mm", true);
    assert_eq!(new.len(), 4);
    s.add_constraint(ConstraintKind::Horizontal { line: l[0] });
    s.add_constraint(ConstraintKind::Angle {
        a: l[0],
        b: l[1],
        value: "80 deg".into(),
    });
    let sol = solve_with(&s, &[]);
    assert!(solved(&sol), "断られない: {:?}", sol.status);
    let (a, b) = (sol.points[&ps[0]], sol.points[&ps[1]]);
    assert!((a[1] - b[1]).abs() < 1e-6, "元の辺が水平になる");
    let ends = |id: EntityId| match s.entity(id) {
        Some(Entity::Line { a, b, .. }) => [*a, *b],
        _ => panic!("線分"),
    };
    for (old, new) in l.iter().zip(&new) {
        for p in ends(*new) {
            assert!(
                (to_line(&sol, &s, p, *old) - 2.0).abs() < 1e-6,
                "写しは 2 mm 離れたまま"
            );
        }
    }
}
