use crate::*;
use std::collections::HashMap;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

/// 寸法の式を数として読んで解く(`12 mm` → 12、`30 deg` → rad)。
fn solve_plain(s: &Sketch) -> Solution {
    let values: HashMap<ConstraintId, f64> = s
        .constraints
        .iter()
        .filter(|c| !c.reference)
        .filter_map(|c| {
            let v = c.kind.value()?;
            let n: f64 = v.split_whitespace().next()?.parse().ok()?;
            Some((c.id, if c.kind.is_angle() { n.to_radians() } else { n }))
        })
        .collect();
    solve(s, &values).expect("解ける")
}

fn near(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6
}

fn pos(s: &Sketch, p: EntityId) -> [f64; 2] {
    match s.entity(p) {
        Some(Entity::Point { x, y, .. }) => [*x, *y],
        _ => panic!("{p} は点ではない"),
    }
}

fn line_ends(s: &Sketch, l: EntityId) -> (EntityId, EntityId) {
    match s.entity(l) {
        Some(Entity::Line { a, b, .. }) => (*a, *b),
        _ => panic!("{l} は線分ではない"),
    }
}

fn line(s: &mut Sketch, a: [f64; 2], b: [f64; 2]) -> EntityId {
    let (p, q) = (s.add_point(a[0], a[1]), s.add_point(b[0], b[1]));
    s.add_line(p, q)
}

/// 写しの円の中心の位置(パターンの写しの順)。
fn copy_centers(s: &Sketch, id: PatternId) -> Vec<([i32; 2], [f64; 2])> {
    let p = s.pattern(id).unwrap();
    p.copies
        .iter()
        .map(|c| {
            let center = c
                .entities
                .iter()
                .find_map(|e| match s.entity(*e) {
                    Some(Entity::Circle { center, .. }) => Some(*center),
                    _ => None,
                })
                .expect("写しの円");
            (c.index, pos(s, center))
        })
        .collect()
}

/// 点 (10, 0) を中心にした半径 1 の円。
fn small_circle(s: &mut Sketch) -> EntityId {
    let c = s.add_point(10.0, 0.0);
    s.add_circle(c, 1.0)
}

fn circular(
    entities: Vec<EntityId>,
    count: usize,
    spacing: AngularSpacing,
    angle: f64,
) -> CircularPattern {
    CircularPattern {
        entities,
        center: PatternCenter::At([0.0, 0.0]),
        count,
        spacing,
        angle,
        suppressed: vec![],
    }
}

#[test]
fn a_full_circular_pattern_spreads_copies_evenly() {
    let mut s = Sketch::default();
    let c = small_circle(&mut s);
    let id = s
        .add_circular_pattern(circular(vec![c], 4, AngularSpacing::Total, TAU))
        .unwrap();
    let got = copy_centers(&s, id);
    assert_eq!(got.len(), 3);
    assert!(near(got[0].1, [0.0, 10.0]) && got[0].0 == [1, 0]);
    assert!(near(got[1].1, [-10.0, 0.0]));
    assert!(near(got[2].1, [0.0, -10.0]));
    assert_eq!(s.pattern_of(c), Some((id, None)));
    let copy = s.pattern(id).unwrap().copies[1].entities[0];
    assert_eq!(s.pattern_of(copy), Some((id, Some([2, 0]))));
    // 写しは形を決める
    let sol = solve_plain(&s);
    assert!(near(
        sol.points[&s.pattern(id).unwrap().copies[0].entities[0]],
        [0.0, 10.0]
    ));
}

#[test]
fn step_and_symmetric_angles_place_copies_where_asked() {
    let mut s = Sketch::default();
    let c = small_circle(&mut s);
    let id = s
        .add_circular_pattern(circular(vec![c], 3, AngularSpacing::Step, FRAC_PI_2 / 3.0))
        .unwrap();
    let got = copy_centers(&s, id);
    let at = |deg: f64| [10.0 * deg.to_radians().cos(), 10.0 * deg.to_radians().sin()];
    assert!(near(got[0].1, at(30.0)) && near(got[1].1, at(60.0)));
    // 対称: 片側 2 つ(元を含む)、両側を合わせて 90°
    let mut s = Sketch::default();
    let c = small_circle(&mut s);
    let id = s
        .add_circular_pattern(circular(vec![c], 2, AngularSpacing::Symmetric, FRAC_PI_2))
        .unwrap();
    let got = copy_centers(&s, id);
    assert_eq!(got.len(), 2);
    assert_eq!(got[0].0, [-1, 0]);
    assert!(near(got[0].1, at(-45.0)) && near(got[1].1, at(45.0)));
    // 一周より狭い総角度は、両端を含めて等分
    let mut s = Sketch::default();
    let c = small_circle(&mut s);
    let id = s
        .add_circular_pattern(circular(vec![c], 3, AngularSpacing::Total, PI))
        .unwrap();
    let got = copy_centers(&s, id);
    assert!(near(got[0].1, at(90.0)) && near(got[1].1, at(180.0)));
}

#[test]
fn an_unusable_pattern_is_not_added() {
    let mut s = Sketch::default();
    let c = small_circle(&mut s);
    let before = s.clone();
    assert!(
        s.add_circular_pattern(circular(vec![c], 3, AngularSpacing::Total, 0.0))
            .is_none()
    );
    assert!(
        s.add_circular_pattern(circular(vec![], 3, AngularSpacing::Total, TAU))
            .is_none()
    );
    let mut def = circular(vec![c], 3, AngularSpacing::Total, TAU);
    def.center = PatternCenter::Point(EntityId(999));
    assert!(s.add_circular_pattern(def).is_none());
    assert_eq!(s, before, "何も変えない");
}

#[test]
fn editing_a_pattern_rebuilds_its_copies() {
    let mut s = Sketch::default();
    let c = small_circle(&mut s);
    let id = s
        .add_circular_pattern(circular(vec![c], 4, AngularSpacing::Total, TAU))
        .unwrap();
    let copies = s.pattern(id).unwrap().copies.clone();
    // 写しに後から寸法を付ける(1 番目は残り、3 番目は消える)
    let circle_of = |cp: &PatternCopy, s: &Sketch| {
        *cp.entities
            .iter()
            .find(|e| matches!(s.entity(**e), Some(Entity::Circle { .. })))
            .unwrap()
    };
    let (c1, c3) = (circle_of(&copies[0], &s), circle_of(&copies[2], &s));
    let kept = s.add_constraint(ConstraintKind::Diameter {
        circle: c1,
        value: "3 mm".into(),
    });
    let lost = s.add_constraint(ConstraintKind::Radius {
        circle: c3,
        value: "2 mm".into(),
    });
    let mut def = s.pattern(id).unwrap().def.clone();
    let PatternDef::Circular(cd) = &mut def else {
        panic!("円形状")
    };
    cd.count = 3;
    assert!(s.edit_pattern(id, def));
    let got = copy_centers(&s, id);
    assert_eq!(got.len(), 2, "3 つに減らすと写しは 2 つ");
    let at = |deg: f64| [10.0 * deg.to_radians().cos(), 10.0 * deg.to_radians().sin()];
    assert!(near(got[0].1, at(120.0)) && near(got[1].1, at(240.0)));
    // 前の写しは消えている
    for cp in &copies {
        for e in &cp.entities {
            assert!(s.entity(*e).is_none(), "前の写し {e} が残っている");
        }
    }
    let circles = s
        .entities
        .iter()
        .filter(|e| matches!(e, Entity::Circle { .. }))
        .count();
    assert_eq!(circles, 3);
    // 残る番号の写しに付けた寸法は、新しい写しへ付いて行き、消えた写しの寸法は落ちる
    let new_c1 = circle_of(&s.pattern(id).unwrap().copies[0], &s);
    let k = s.constraints.iter().find(|k| k.id == kept).expect("残る");
    assert_eq!(
        k.kind,
        ConstraintKind::Diameter {
            circle: new_c1,
            value: "3 mm".into()
        }
    );
    assert!(s.constraints.iter().all(|k| k.id != lost));
    solve_plain(&s);
}

#[test]
fn a_shape_drawn_from_a_copy_keeps_its_line_when_the_pattern_is_rebuilt() {
    let mut s = Sketch::default();
    let p = s.add_point(10.0, 0.0);
    let id = s
        .add_circular_pattern(circular(vec![p], 2, AngularSpacing::Total, TAU))
        .unwrap();
    let copy = s.pattern(id).unwrap().copies[0].entities[0];
    let q = s.add_point(0.0, 0.0);
    let l = s.add_line(copy, q);
    let mut def = s.pattern(id).unwrap().def.clone();
    let PatternDef::Circular(cd) = &mut def else {
        panic!()
    };
    cd.angle = PI / 2.0;
    cd.spacing = AngularSpacing::Step;
    assert!(s.edit_pattern(id, def));
    let (a, _) = line_ends(&s, l);
    let new = s.pattern(id).unwrap().copies[0].entities[0];
    assert_eq!(a, new, "線は新しい写しの点へ付け替わる");
    assert!(near(pos(&s, a), [0.0, 10.0]));
}

#[test]
fn suppressed_instances_are_not_made() {
    let mut s = Sketch::default();
    let c = small_circle(&mut s);
    let mut def = circular(vec![c], 4, AngularSpacing::Total, TAU);
    def.suppressed = vec![2];
    let id = s.add_circular_pattern(def).unwrap();
    let got = copy_centers(&s, id);
    assert_eq!(got.iter().map(|g| g.0[0]).collect::<Vec<_>>(), vec![1, 3]);
    // 写しを消すと、その番号が抑制になる(作り直しても戻らない)
    let one = s.pattern(id).unwrap().copies[0].entities.clone();
    let circle = *one
        .iter()
        .find(|e| matches!(s.entity(**e), Some(Entity::Circle { .. })))
        .unwrap();
    s.remove_entities(&[circle]);
    let PatternDef::Circular(cd) = &s.pattern(id).unwrap().def else {
        panic!()
    };
    assert_eq!(cd.suppressed, vec![2, 1]);
    assert_eq!(s.pattern(id).unwrap().copies.len(), 1);
}

fn rect_dir(axis: PatternAxis, count: usize, distance: f64) -> PatternDirection {
    PatternDirection {
        axis,
        count,
        distance,
        distance_type: DistanceType::Spacing,
        symmetric: false,
    }
}

#[test]
fn a_rectangular_pattern_fills_rows_and_columns() {
    let mut s = Sketch::default();
    let c0 = s.add_point(0.0, 0.0);
    let c = s.add_circle(c0, 1.0);
    let id = s
        .add_rectangular_pattern(RectangularPattern {
            entities: vec![c],
            directions: [
                rect_dir(PatternAxis::Angle(0.0), 3, 10.0),
                rect_dir(PatternAxis::Angle(FRAC_PI_2), 2, 5.0),
            ],
            suppressed: vec![[2, 1]],
        })
        .unwrap();
    let got = copy_centers(&s, id);
    let want: Vec<([i32; 2], [f64; 2])> = vec![
        ([0, 1], [0.0, 5.0]),
        ([1, 0], [10.0, 0.0]),
        ([1, 1], [10.0, 5.0]),
        ([2, 0], [20.0, 0.0]),
    ];
    assert_eq!(got.len(), want.len());
    for (g, w) in got.iter().zip(&want) {
        assert_eq!(g.0, w.0);
        assert!(near(g.1, w.1), "{g:?} {w:?}");
    }
}

#[test]
fn extent_symmetric_and_line_directions() {
    let mut s = Sketch::default();
    let c0 = s.add_point(0.0, 0.0);
    let c = s.add_circle(c0, 1.0);
    // 向きは斜めの線分
    let axis = line(&mut s, [50.0, 50.0], [53.0, 54.0]);
    let id = s
        .add_rectangular_pattern(RectangularPattern {
            entities: vec![c],
            directions: [
                PatternDirection {
                    distance_type: DistanceType::Extent,
                    symmetric: true,
                    ..rect_dir(PatternAxis::Line(axis), 3, 20.0)
                },
                rect_dir(PatternAxis::Angle(0.0), 1, 0.0),
            ],
            suppressed: vec![],
        })
        .unwrap();
    let got = copy_centers(&s, id);
    // 片側 3(元を含む)、両側で範囲 20 → 間隔 5
    let u = [0.6, 0.8];
    let at = |k: f64| [u[0] * 5.0 * k, u[1] * 5.0 * k];
    assert_eq!(got.len(), 4);
    assert_eq!(got[0].0, [-2, 0]);
    assert!(near(got[0].1, at(-2.0)) && near(got[3].1, at(2.0)));
    // 向きの線分を消すと、向きは角度で覚える
    s.remove_entities(&[axis]);
    let PatternDef::Rectangular(r) = &s.pattern(id).unwrap().def else {
        panic!()
    };
    let PatternAxis::Angle(a) = r.directions[0].axis else {
        panic!("角度になる")
    };
    assert!((a - 0.8f64.atan2(0.6)).abs() < 1e-12);
}

#[test]
fn patterns_survive_saving_and_follow_renumbering() {
    let mut s = Sketch::default();
    let p = s.add_point(0.0, 0.0);
    let c = small_circle(&mut s);
    let mut def = circular(vec![c], 3, AngularSpacing::Total, TAU);
    def.center = PatternCenter::Point(p);
    def.suppressed = vec![2];
    let id = s.add_circular_pattern(def).unwrap();
    let text = serde_json::to_string(&s).unwrap();
    let back: Sketch = serde_json::from_str(&text).unwrap();
    assert_eq!(back, s);
    assert!(text.contains("\"patterns\""));
    // パターンがなければ書かない
    let plain = serde_json::to_string(&Sketch::default()).unwrap();
    assert!(!plain.contains("patterns"));
    let _ = id;
}

#[test]
fn renumbering_carries_the_pattern_along() {
    let mut s = Sketch::default();
    let gap = s.add_point(99.0, 99.0);
    let dummy = s.add_constraint(ConstraintKind::Fix { point: gap });
    let c = small_circle(&mut s);
    s.add_constraint(ConstraintKind::Radius {
        circle: c,
        value: "1 mm".into(),
    });
    let mut def = circular(vec![c], 3, AngularSpacing::Total, TAU);
    let center = s.add_point(0.0, 0.0);
    def.center = PatternCenter::Point(center);
    let id = s.add_circular_pattern(def).unwrap();
    s.remove_constraint(dummy);
    s.remove_entities(&[gap]);
    let before = copy_centers(&s, id);
    let changed = s.renumber_after(0, 0);
    assert!(!changed.is_empty());
    let p = s.pattern(id).unwrap();
    assert!(p.def.entities().iter().all(|e| s.entity(*e).is_some()));
    let PatternDef::Circular(cd) = &p.def else {
        panic!()
    };
    assert_eq!(cd.center, PatternCenter::Point(changed[&center]));
    for cp in &p.copies {
        assert!(cp.entities.iter().all(|e| s.entity(*e).is_some()));
        // 写しに拘束は写さない(並びの拘束が元から決める)。元との対応は番号が付いて行く
        assert!(cp.constraints.is_empty());
        assert_eq!(cp.from.len(), cp.entities.len());
        assert!(cp.from.iter().all(|e| {
            p.def.entities().contains(e)
                || s.entity(*e)
                    .is_some_and(|x| matches!(x, Entity::Point { .. }))
        }));
    }
    assert!(s.pattern_constraint(id).is_some(), "並びの拘束が 1 つ");
    assert_eq!(
        copy_centers(&s, id).iter().map(|c| c.1).collect::<Vec<_>>(),
        before.iter().map(|c| c.1).collect::<Vec<_>>()
    );
    // 詰めた後も直せる
    let def = s.pattern(id).unwrap().def.clone();
    assert!(s.edit_pattern(id, def));
    assert_eq!(copy_centers(&s, id).len(), 2);
}

#[test]
fn removing_a_pattern_can_keep_or_drop_its_copies() {
    let mut s = Sketch::default();
    let c = small_circle(&mut s);
    let id = s
        .add_circular_pattern(circular(vec![c], 3, AngularSpacing::Total, TAU))
        .unwrap();
    let mut keep = s.clone();
    assert!(keep.remove_pattern(id, true));
    assert!(keep.patterns.is_empty());
    assert_eq!(keep.entities.len(), s.entities.len());
    assert!(s.remove_pattern(id, false));
    assert_eq!(s.entities.len(), 2, "元の円と中心だけ");
    assert!(!s.remove_pattern(id, false));
    // 元を消すと定義も消える(写しは残る)
    let mut t = Sketch::default();
    let c = small_circle(&mut t);
    t.add_circular_pattern(circular(vec![c], 3, AngularSpacing::Total, TAU))
        .unwrap();
    t.remove_entities(&[c]);
    assert!(t.patterns.is_empty());
    assert_eq!(
        t.entities
            .iter()
            .filter(|e| matches!(e, Entity::Circle { .. }))
            .count(),
        2
    );
}

/// 線分 (0,0)-(5,0) と、x = 10・x = 20 の縦の線分。
fn walls() -> (Sketch, EntityId, EntityId) {
    let mut s = Sketch::default();
    let l = line(&mut s, [0.0, 0.0], [5.0, 0.0]);
    let w = line(&mut s, [10.0, -5.0], [10.0, 5.0]);
    line(&mut s, [20.0, -5.0], [20.0, 5.0]);
    (s, l, w)
}

#[test]
fn a_line_extends_to_the_next_curve_it_meets() {
    let (mut s, l, w) = walls();
    s.add_constraint(ConstraintKind::Length {
        line: l,
        value: "5 mm".into(),
    });
    s.add_constraint(ConstraintKind::Horizontal { line: l });
    let sol = solve_plain(&s);
    let path = s.extend_preview(&sol, l, [4.0, 0.0]).unwrap();
    assert!(near(path[0], [5.0, 0.0]) && near(path[1], [10.0, 0.0]));
    assert!(s.extend(l, [4.0, 0.0]));
    let (a, b) = line_ends(&s, l);
    assert!(near(pos(&s, a), [0.0, 0.0]) && near(pos(&s, b), [10.0, 0.0]));
    assert!(s.constraints.iter().any(|c| matches!(
        c.kind,
        ConstraintKind::PointOnLine { point, line } if point == b && line == w
    )));
    assert!(
        !s.constraints
            .iter()
            .any(|c| matches!(c.kind, ConstraintKind::Length { .. })),
        "長さの寸法は外す"
    );
    assert!(
        s.constraints
            .iter()
            .any(|c| matches!(c.kind, ConstraintKind::Horizontal { .. }))
    );
    let sol = solve_plain(&s);
    assert!(near(sol.points[&b], [10.0, 0.0]));
    // 次は x = 20 まで
    assert!(s.extend(l, [9.0, 0.0]));
    let (_, b) = line_ends(&s, l);
    assert!(near(pos(&s, b), [20.0, 0.0]));
    // 左には何もない
    let before = s.clone();
    assert!(!s.extend(l, [1.0, 0.0]));
    assert_eq!(s, before);
    assert!(s.extend_preview(&solve_plain(&s), l, [1.0, 0.0]).is_none());
}

#[test]
fn an_arc_extends_around_its_circle() {
    let mut s = Sketch::default();
    let c = s.add_point(0.0, 0.0);
    let (p, q) = (s.add_point(10.0, 0.0), s.add_point(0.0, 10.0));
    let arc = s.add_arc(c, p, q);
    line(&mut s, [-20.0, 5.0], [-1.0, 5.0]);
    assert!(s.extend(arc, [1.0, 9.9]));
    let Some(Entity::Arc { start, end, .. }) = s.entity(arc).cloned() else {
        panic!()
    };
    assert_eq!(start, p);
    let want = [-(75.0f64).sqrt(), 5.0];
    assert!(near(pos(&s, end), want), "{:?}", pos(&s, end));
    let sol = solve_plain(&s);
    assert!(near(sol.points[&end], want));
}

#[test]
fn an_elliptical_arc_extends_too() {
    let mut s = Sketch::default();
    let e = s
        .add_ellipse((None, [0.0, 0.0]), (None, [10.0, 0.0]), [0.0, 5.0])
        .unwrap();
    // 楕円を縦の線でトリムして楕円弧にし、その線より先の線まで延ばす
    let cut = line(&mut s, [-20.0, 0.0], [20.0, 0.0]);
    assert!(s.trim(e, [0.0, -5.0]));
    s.remove_entities(&[cut]);
    let arc = s
        .entities
        .iter()
        .find(|x| matches!(x, Entity::Ellipse { ends: Some(_), .. }))
        .map(Entity::id)
        .unwrap();
    line(&mut s, [-20.0, -3.0], [-5.0, -3.0]);
    assert!(s.extend(arc, [-9.9, 0.2]));
    let Some(Entity::Ellipse {
        ends: Some((_, t)), ..
    }) = s.entity(arc).cloned()
    else {
        panic!()
    };
    // x²/100 + y²/25 = 1、y = -3 → x = -8
    assert!(near(pos(&s, t), [-8.0, -3.0]), "{:?}", pos(&s, t));
}

#[test]
fn breaking_a_line_splits_it_at_the_crossings_around_the_click() {
    let mut s = Sketch::default();
    let l = line(&mut s, [0.0, 0.0], [30.0, 0.0]);
    for x in [10.0, 20.0] {
        line(&mut s, [x, -5.0], [x, 5.0]);
    }
    s.add_constraint(ConstraintKind::Horizontal { line: l });
    let sol = solve_plain(&s);
    let span = s.break_preview(&sol, l, [15.0, 0.0]).unwrap();
    assert!(near(span[0], [10.0, 0.0]) && near(*span.last().unwrap(), [20.0, 0.0]));
    let pieces = s.break_at(l, [15.0, 0.0]);
    assert_eq!(pieces.len(), 3);
    assert!(s.entity(l).is_none());
    let ends: Vec<([f64; 2], [f64; 2])> = pieces
        .iter()
        .map(|p| {
            let (a, b) = line_ends(&s, *p);
            (pos(&s, a), pos(&s, b))
        })
        .collect();
    assert!(near(ends[0].0, [0.0, 0.0]) && near(ends[0].1, [10.0, 0.0]));
    assert!(near(ends[1].0, [10.0, 0.0]) && near(ends[1].1, [20.0, 0.0]));
    assert!(near(ends[2].0, [20.0, 0.0]) && near(ends[2].1, [30.0, 0.0]));
    // 片どうしは切った点を共有する
    assert_eq!(line_ends(&s, pieces[0]).1, line_ends(&s, pieces[1]).0);
    // はじめの片に水平が付け替わり、後ろの片の端ははじめの片の直線の上
    assert!(
        s.constraints
            .iter()
            .any(|c| matches!(c.kind, ConstraintKind::Horizontal { line } if line == pieces[0]))
    );
    let on_first = s
        .constraints
        .iter()
        .filter(|c| matches!(c.kind, ConstraintKind::PointOnLine { line, .. } if line == pieces[0]))
        .count();
    assert_eq!(on_first, 2);
    let sol = solve_plain(&s);
    assert!(near(sol.points[&line_ends(&s, pieces[2]).1], [30.0, 0.0]));
    // 端の近く(交点が片側だけ)なら 2 つ
    let mut t = Sketch::default();
    let l = line(&mut t, [0.0, 0.0], [30.0, 0.0]);
    line(&mut t, [10.0, -5.0], [10.0, 5.0]);
    assert_eq!(t.break_at(l, [3.0, 0.0]).len(), 2);
    // 交点がなければ何もしない
    let mut u = Sketch::default();
    let l = line(&mut u, [0.0, 0.0], [30.0, 0.0]);
    let before = u.clone();
    assert!(u.break_at(l, [3.0, 0.0]).is_empty());
    assert_eq!(u, before);
}

#[test]
fn breaking_a_circle_makes_two_arcs_on_the_same_circle() {
    let mut s = Sketch::default();
    let c = s.add_point(0.0, 0.0);
    let circle = s.add_circle(c, 10.0);
    s.add_constraint(ConstraintKind::Radius {
        circle,
        value: "10 mm".into(),
    });
    line(&mut s, [-20.0, 0.0], [20.0, 0.0]);
    let pieces = s.break_at(circle, [0.0, 10.0]);
    assert_eq!(pieces.len(), 2);
    let (
        Some(Entity::Arc {
            center: c0,
            start: s0,
            end: e0,
            ..
        }),
        Some(Entity::Arc {
            center: c1,
            start: s1,
            end: e1,
            ..
        }),
    ) = (s.entity(pieces[0]).cloned(), s.entity(pieces[1]).cloned())
    else {
        panic!("円弧 2 つ")
    };
    assert_eq!((c0, c1), (c, c));
    assert_eq!((s0, e0), (e1, s1), "端を共有して輪になる");
    // 押した上側の片は (10,0) から (-10,0) へ反時計回り
    assert!(near(pos(&s, s0), [10.0, 0.0]) && near(pos(&s, e0), [-10.0, 0.0]));
    let sol = solve_plain(&s);
    assert!(sol.max_residual < 1e-7);
}

#[test]
fn breaking_a_spline_keeps_it_smooth() {
    let mut s = Sketch::default();
    let sp = s
        .add_spline(
            &[
                (None, [0.0, 0.0]),
                (None, [10.0, 5.0]),
                (None, [20.0, -5.0]),
                (None, [30.0, 0.0]),
            ],
            false,
            false,
        )
        .unwrap();
    for x in [8.0, 22.0] {
        line(&mut s, [x, -20.0], [x, 20.0]);
    }
    let sol = solve_plain(&s);
    let shape_before = s.curve_polyline(&sol, sp, 0.01).unwrap();
    let pieces = s.break_at(sp, [15.0, 0.0]);
    assert_eq!(pieces.len(), 3);
    let g2 = s
        .constraints
        .iter()
        .filter(|c| matches!(c.kind, ConstraintKind::Curvature { .. }))
        .count();
    assert_eq!(g2, 2);
    let sol = solve_plain(&s);
    assert!(sol.max_residual < 1e-7);
    // 分けた片はもとの曲線の上
    for p in &pieces {
        for q in s.curve_polyline(&sol, *p, 0.01).unwrap() {
            let d = shape_before
                .windows(2)
                .map(|w| {
                    let (a, b) = (w[0], w[1]);
                    let ab = [b[0] - a[0], b[1] - a[1]];
                    let t = (((q[0] - a[0]) * ab[0] + (q[1] - a[1]) * ab[1])
                        / (ab[0] * ab[0] + ab[1] * ab[1]))
                        .clamp(0.0, 1.0);
                    (a[0] + ab[0] * t - q[0]).hypot(a[1] + ab[1] * t - q[1])
                })
                .fold(f64::INFINITY, f64::min);
            assert!(
                d < 0.05,
                "片 {p} の点 {q:?} がもとの曲線から {d} 離れている"
            );
        }
    }
}

/// 角が (0,0) の L 字(横は (20,0) まで、縦は (0,10) まで)。
fn corner() -> (Sketch, EntityId, EntityId, EntityId) {
    let mut s = Sketch::default();
    let c = s.add_point(0.0, 0.0);
    let (p, q) = (s.add_point(20.0, 0.0), s.add_point(0.0, 10.0));
    let h = s.add_line(c, p);
    let v = s.add_line(q, c);
    s.add_constraint(ConstraintKind::Fix { point: c });
    s.add_constraint(ConstraintKind::Horizontal { line: h });
    s.add_constraint(ConstraintKind::Vertical { line: v });
    (s, c, h, v)
}

fn cut_ends(s: &Sketch, sol: &Solution, cut: EntityId) -> ([f64; 2], [f64; 2]) {
    let (a, b) = line_ends(s, cut);
    (sol.points[&a], sol.points[&b])
}

#[test]
fn a_chamfer_with_two_distances_has_both_dimensions() {
    let (mut s, c, h, v) = corner();
    let (cut, dims) = s
        .chamfer_corner_with(
            c,
            Some(v),
            &Chamfer::TwoDistances {
                d1: 2.0,
                d2: 4.0,
                value1: "2 mm".into(),
                value2: "4 mm".into(),
            },
        )
        .unwrap();
    assert_eq!(dims.len(), 2);
    let sol = solve_plain(&s);
    let (a, b) = cut_ends(&s, &sol, cut);
    assert!(near(a, [0.0, 2.0]) && near(b, [4.0, 0.0]), "{a:?} {b:?}");
    // 値を直すと形が付いて行く
    for k in &mut s.constraints {
        if k.id == dims[1] {
            *k.kind.value_mut().unwrap() = "6 mm".into();
        }
    }
    let sol = solve_plain(&s);
    let (_, b) = cut_ends(&s, &sol, cut);
    assert!(near(b, [6.0, 0.0]));
    let _ = h;
}

#[test]
fn a_chamfer_by_distance_and_angle_measures_the_angle_inside_the_corner() {
    let (mut s, c, h, _) = corner();
    let (cut, dims) = s
        .chamfer_corner_with(
            c,
            Some(h),
            &Chamfer::DistanceAngle {
                d: 3.0,
                angle: 30f64.to_radians(),
                value: "3 mm".into(),
                angle_value: "30 deg".into(),
            },
        )
        .unwrap();
    let sol = solve_plain(&s);
    let (a, b) = cut_ends(&s, &sol, cut);
    // 横に 3、縦に 3 tan 30°
    let y = 3.0 * 30f64.to_radians().tan();
    assert!(near(a, [3.0, 0.0]) && near(b, [0.0, y]), "{a:?} {b:?}");
    let angle = s.constraints.iter().find(|k| k.id == dims[1]).unwrap();
    let m = measure(&s, &sol, &angle.kind).unwrap();
    assert!((m - 30f64.to_radians()).abs() < 1e-9, "{}", m.to_degrees());
    // 角度を 45° にすると縦も 3
    for k in &mut s.constraints {
        if k.id == dims[1] {
            *k.kind.value_mut().unwrap() = "45 deg".into();
        }
    }
    let sol = solve_plain(&s);
    let (_, b) = cut_ends(&s, &sol, cut);
    assert!(near(b, [0.0, 3.0]), "{b:?}");
    // 線の向きを逆に選んでも、角の内側で測る
    let (mut t, c, _, v) = corner();
    let (cut, _) = t
        .chamfer_corner_with(
            c,
            Some(v),
            &Chamfer::DistanceAngle {
                d: 3.0,
                angle: 60f64.to_radians(),
                value: "3 mm".into(),
                angle_value: "60 deg".into(),
            },
        )
        .unwrap();
    let sol = solve_plain(&t);
    let (a, b) = cut_ends(&t, &sol, cut);
    assert!(near(a, [0.0, 3.0]) && near(b, [3.0 * 60f64.to_radians().tan(), 0.0]));
}

#[test]
fn an_equal_chamfer_matches_the_old_one_and_bad_values_change_nothing() {
    let (mut s, c, _, _) = corner();
    let (cut, dims) = s
        .chamfer_corner_with(
            c,
            None,
            &Chamfer::Equal {
                d: 2.0,
                value: "2 mm".into(),
            },
        )
        .unwrap();
    assert_eq!(dims.len(), 1);
    let sol = solve_plain(&s);
    let (a, b) = cut_ends(&s, &sol, cut);
    assert!((a[0] + a[1] - 2.0).abs() < 1e-9 && (b[0] + b[1] - 2.0).abs() < 1e-9);
    let (mut t, c, h, _) = corner();
    let before = t.clone();
    let too_wide = Chamfer::DistanceAngle {
        d: 3.0,
        angle: 100f64.to_radians(),
        value: "3 mm".into(),
        angle_value: "100 deg".into(),
    };
    assert!(t.chamfer_corner_with(c, Some(h), &too_wide).is_none());
    let too_long = Chamfer::TwoDistances {
        d1: 2.0,
        d2: 30.0,
        value1: "2 mm".into(),
        value2: "30 mm".into(),
    };
    assert!(t.chamfer_corner_with(c, Some(h), &too_long).is_none());
    assert_eq!(t, before);
}

/// 長方形 (0,0)-(20,10) と、その左下の角に一致でつながる外の線分 (-10,-10)-(0,0)(外の端は固定)。
fn rect_with_tail() -> (Sketch, Vec<EntityId>, EntityId, EntityId) {
    let mut s = Sketch::default();
    let rect = s
        .add_rectangle((None, [0.0, 0.0]), (None, [20.0, 10.0]))
        .unwrap();
    let corner = line_ends(&s, rect[0]).0;
    let (far, near_end) = (s.add_point(-10.0, -10.0), s.add_point(0.0, 0.0));
    let tail = s.add_line(far, near_end);
    s.add_constraint(ConstraintKind::Fix { point: far });
    s.add_constraint(ConstraintKind::Coincident {
        a: corner,
        b: near_end,
    });
    s.add_constraint(ConstraintKind::Length {
        line: rect[0],
        value: "20 mm".into(),
    });
    (s, rect.to_vec(), corner, tail)
}

#[test]
fn moving_with_followed_links_is_not_pulled_back() {
    let (mut s, rect, corner, tail) = rect_with_tail();
    let moved = s.move_with(&rect, Transform::Translate([5.0, 3.0]), Links::Follow);
    let (_, near_end) = line_ends(&s, tail);
    assert_eq!(moved.followers, vec![near_end]);
    assert!(moved.holding.is_empty(), "{:?}", moved.holding);
    let sol = solve_plain(&s);
    assert!(
        near(sol.points[&corner], [5.0, 3.0]),
        "{:?}",
        sol.points[&corner]
    );
    assert!(
        near(sol.points[&near_end], [5.0, 3.0]),
        "外の線分の端も付いて来る"
    );
    // 前からの移動(選んだ点だけ動かす)は一致に引かれて戻される
    let (mut old, rect, corner, _) = rect_with_tail();
    old.move_entities(&rect, [5.0, 3.0]);
    let sol = solve_plain(&old);
    assert!(!near(sol.points[&corner], [5.0, 3.0]));
}

#[test]
fn moving_with_detached_links_drops_the_coincidence() {
    let (mut s, rect, corner, tail) = rect_with_tail();
    let moved = s.move_with(
        &rect,
        Transform::Rotate {
            center: [0.0, 0.0],
            angle: FRAC_PI_2,
        },
        Links::Detach,
    );
    assert_eq!(moved.dropped.len(), 1, "一致の 1 つ");
    let sol = solve_plain(&s);
    let (_, near_end) = line_ends(&s, tail);
    assert!(
        near(sol.points[&near_end], [0.0, 0.0]),
        "外の線分は動かない"
    );
    let far = line_ends(&s, rect[0]).1;
    assert!(
        near(sol.points[&far], [0.0, 20.0]),
        "{:?}",
        sol.points[&far]
    );
    assert!(near(sol.points[&corner], [0.0, 0.0]));
}

#[test]
fn detaching_splits_a_shared_end() {
    let mut s = Sketch::default();
    let (p0, p1, p2) = (
        s.add_point(0.0, 0.0),
        s.add_point(10.0, 0.0),
        s.add_point(10.0, 10.0),
    );
    let a = s.add_line(p0, p1);
    let b = s.add_line(p1, p2);
    s.move_with(&[a], Transform::Translate([0.0, -5.0]), Links::Detach);
    assert_eq!(line_ends(&s, b), (p1, p2), "選んでいない線分はそのまま");
    assert!(near(pos(&s, p1), [10.0, 0.0]));
    let (q0, q1) = line_ends(&s, a);
    assert_eq!(q0, p0);
    assert_ne!(q1, p1, "共有していた端は分かれる");
    assert!(near(pos(&s, q1), [10.0, -5.0]));
    // つなげたまま動かすと、共有する端で伸びる
    let mut t = Sketch::default();
    let (p0, p1, p2) = (
        t.add_point(0.0, 0.0),
        t.add_point(10.0, 0.0),
        t.add_point(10.0, 10.0),
    );
    let a = t.add_line(p0, p1);
    let b = t.add_line(p1, p2);
    t.add_constraint(ConstraintKind::Vertical { line: b });
    let moved = t.move_with(&[a], Transform::Translate([0.0, -5.0]), Links::Follow);
    assert_eq!(line_ends(&t, b), (p1, p2));
    assert!(near(pos(&t, p1), [10.0, -5.0]));
    assert_eq!(
        moved.holding.len(),
        1,
        "伸びる線分の垂直は、引き戻すかもしれない拘束として挙げる"
    );
}

#[test]
fn copying_and_moving_keeps_the_original() {
    let (mut s, rect, corner, _) = rect_with_tail();
    let before: Vec<[f64; 2]> = rect.iter().map(|l| pos(&s, line_ends(&s, *l).0)).collect();
    let new = s.copy_transformed(
        &rect,
        Transform::Rotate {
            center: [0.0, 0.0],
            angle: FRAC_PI_2,
        },
    );
    assert_eq!(new.len(), 8);
    let after: Vec<[f64; 2]> = rect.iter().map(|l| pos(&s, line_ends(&s, *l).0)).collect();
    assert_eq!(before, after, "元はそのまま");
    // 写しは一致を写さない(外の線分につながらない)。水平・垂直は入れ替わる
    let sol = solve_plain(&s);
    assert!(near(sol.points[&corner], [0.0, 0.0]));
    let lines: Vec<EntityId> = new
        .iter()
        .copied()
        .filter(|e| matches!(s.entity(*e), Some(Entity::Line { .. })))
        .collect();
    let (a, b) = line_ends(&s, lines[0]);
    assert!(near(sol.points[&a], [0.0, 0.0]) && near(sol.points[&b], [0.0, 20.0]));
    assert!(
        s.constraints
            .iter()
            .any(|c| matches!(c.kind, ConstraintKind::Vertical { line } if line == lines[0]))
    );
    // 拡大縮小の写し: 数の寸法も倍になる
    let (mut t, rect, _, _) = rect_with_tail();
    let new = t.copy_transformed(
        &rect,
        Transform::Scale {
            center: [0.0, 0.0],
            k: 2.0,
        },
    );
    let first = new
        .iter()
        .copied()
        .find(|e| matches!(t.entity(*e), Some(Entity::Line { .. })))
        .unwrap();
    assert!(t.constraints.iter().any(|c| matches!(
        &c.kind,
        ConstraintKind::Length { line, value } if *line == first && value == "40 mm"
    )));
    let sol = solve_plain(&t);
    let (_, b) = line_ends(&t, first);
    assert!(near(sol.points[&b], [40.0, 0.0]));
    // ずらす写し
    let (mut u, rect, _, _) = rect_with_tail();
    let n = u.entities.len();
    let new = u.copy_transformed(&rect, Transform::Translate([0.0, 30.0]));
    assert_eq!(u.entities.len(), n + new.len());
}

/// 線分 (0,0)-(10,0) と、中心 (25,10) 半径 5 の円弧(左端 (20,10) から下端 (25,5) へ反時計回り)。
fn blend_pair() -> (Sketch, EntityId, EntityId, EntityId, EntityId) {
    let mut s = Sketch::default();
    let l = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    let c = s.add_point(25.0, 10.0);
    let (p, q) = (s.add_point(20.0, 10.0), s.add_point(25.0, 5.0));
    let arc = s.add_arc(c, p, q);
    let end = line_ends(&s, l).1;
    s.add_constraint(ConstraintKind::Fix { point: c });
    s.add_constraint(ConstraintKind::Radius {
        circle: arc,
        value: "5 mm".into(),
    });
    (s, l, end, arc, p)
}

#[test]
fn a_g2_blend_matches_direction_and_curvature_at_both_ends() {
    let (mut s, l, end, arc, start) = blend_pair();
    let blend = s.add_blend((l, end), (arc, start), Continuity::G2).unwrap();
    let sol = solve_plain(&s);
    assert!(sol.max_residual < 1e-7, "{}", sol.max_residual);
    let (t0, k0) = s.curve_end(&sol, blend, false).unwrap();
    let (tl, kl) = s.curve_end(&sol, l, true).unwrap();
    assert!(near(t0, tl), "{t0:?} {tl:?}");
    assert!((k0 - kl).abs() < 1e-6, "{k0} {kl}");
    let (t1, k1) = s.curve_end(&sol, blend, true).unwrap();
    let (ta, ka) = s.curve_end(&sol, arc, false).unwrap();
    assert!(near(t1, ta), "{t1:?} {ta:?}");
    assert!((k1 - ka).abs() < 1e-6, "{k1} {ka}");
    assert!((ka - 0.2).abs() < 1e-9);
    // 端は共有している
    let Some(Entity::Spline { points, .. }) = s.entity(blend) else {
        panic!()
    };
    assert_eq!((points[0], *points.last().unwrap()), (end, start));
}

#[test]
fn a_g1_blend_is_tangent_and_bad_ends_are_refused() {
    let (mut s, l, end, arc, start) = blend_pair();
    let blend = s.add_blend((l, end), (arc, start), Continuity::G1).unwrap();
    assert_eq!(
        s.constraints
            .iter()
            .filter(|c| matches!(c.kind, ConstraintKind::TangentCurve { .. }))
            .count(),
        2
    );
    let sol = solve_plain(&s);
    let (t0, _) = s.curve_end(&sol, blend, false).unwrap();
    let (tl, _) = s.curve_end(&sol, l, true).unwrap();
    assert!(near(t0, tl));
    let (t1, _) = s.curve_end(&sol, blend, true).unwrap();
    let (ta, _) = s.curve_end(&sol, arc, false).unwrap();
    assert!(near(t1, ta));
    let before = s.clone();
    assert!(s.add_blend((l, end), (l, end), Continuity::G1).is_none());
    assert!(
        s.add_blend((l, start), (arc, start), Continuity::G1)
            .is_none(),
        "端でない"
    );
    assert_eq!(s, before);
}

#[test]
fn closed_curves_and_conics_break_too() {
    // 閉じたスプラインは 2 つに分かれ、継ぎ目 2 か所で曲率をそろえる
    let mut s = Sketch::default();
    let sp = s
        .add_spline(
            &[
                (None, [10.0, 0.0]),
                (None, [0.0, 10.0]),
                (None, [-10.0, 0.0]),
                (None, [0.0, -10.0]),
            ],
            false,
            true,
        )
        .unwrap();
    line(&mut s, [-20.0, 1.0], [20.0, 1.0]);
    let pieces = s.break_at(sp, [0.0, 10.0]);
    assert_eq!(pieces.len(), 2);
    let g2 = s
        .constraints
        .iter()
        .filter(|c| matches!(c.kind, ConstraintKind::Curvature { .. }))
        .count();
    assert_eq!(g2, 2);
    assert!(solve_plain(&s).max_residual < 1e-7);
    // 円錐曲線
    let mut t = Sketch::default();
    let cn = t
        .add_conic(
            (None, [0.0, 0.0]),
            (None, [20.0, 0.0]),
            (None, [10.0, 10.0]),
            0.5,
        )
        .unwrap();
    line(&mut t, [10.0, -5.0], [10.0, 20.0]);
    let pieces = t.break_at(cn, [3.0, 2.0]);
    assert_eq!(pieces.len(), 2);
    assert!(
        pieces
            .iter()
            .all(|p| matches!(t.entity(*p), Some(Entity::Conic { .. })))
    );
    assert!(solve_plain(&t).max_residual < 1e-7);
    // スプラインと円錐曲線は延ばさない
    assert!(!t.extend(pieces[0], [0.0, 0.0]));
    // 楕円(閉じた)
    let mut u = Sketch::default();
    let e = u
        .add_ellipse((None, [0.0, 0.0]), (None, [10.0, 0.0]), [0.0, 5.0])
        .unwrap();
    line(&mut u, [5.0, -10.0], [5.0, 10.0]);
    let pieces = u.break_at(e, [10.0, 0.0]);
    assert_eq!(pieces.len(), 2);
    assert!(solve_plain(&u).max_residual < 1e-7);
}
