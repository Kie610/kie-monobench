//! 3D スケッチの解き方と面の見つけ方の確かめ。

use crate::*;
use kmb_sketch::{EntityId, SolveStatus};
use std::collections::HashMap;

fn deg(d: f64) -> f64 {
    d.to_radians()
}

/// 寸法の値を、式の代わりに数で渡す(`value` は表示用の文字)。
fn vals(s: &Sketch3d, v: &[f64]) -> HashMap<kmb_sketch::ConstraintId, f64> {
    s.constraints
        .iter()
        .filter(|c| c.kind.value().is_some())
        .map(|c| c.id)
        .zip(v.iter().copied())
        .collect()
}

#[test]
fn a_line_along_z_with_a_length_is_well_constrained() {
    let mut s = Sketch3d::default();
    let l = s.add_segment([0.0, 0.0, 0.0], [1.0, 2.0, 7.0]);
    let (a, b) = s.line_ends(l).unwrap();
    s.add_constraint(Kind3d::Fix { point: a });
    s.add_constraint(Kind3d::Along {
        line: l,
        axis: Axis::Z,
    });
    s.add_constraint(Kind3d::Length {
        line: l,
        value: "12 mm".into(),
    });
    let sol = solve3d(&s, &vals(&s, &[12.0])).unwrap();
    assert_eq!(sol.status, SolveStatus::WellConstrained);
    let q = sol.points[&b];
    assert!(q[0].abs() < 1e-9 && q[1].abs() < 1e-9, "{q:?}");
    assert!((q[2] - 12.0).abs() < 1e-9, "{q:?}");
    assert!(sol.free.is_empty());
}

#[test]
fn heights_and_orthogonal_drawings_live_in_one_sketch() {
    // 高さの違う 2 本の線分(XY に平行)と、直交する YZ の向きの線分を 1 つのスケッチに
    let mut s = Sketch3d::default();
    let low = s.add_segment([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]);
    let high = s.add_segment([0.0, 0.0, 5.0], [10.0, 1.0, 6.0]);
    let (la, _) = s.line_ends(low).unwrap();
    let (ha, hb) = s.line_ends(high).unwrap();
    s.add_constraint(Kind3d::ParallelToPlane {
        line: high,
        plane: PlaneRef::Axis {
            plane: AxisPlane::Xy,
        },
    });
    s.add_constraint(Kind3d::AxisDistance {
        a: la,
        b: ha,
        axis: Axis::Z,
        value: "8 mm".into(),
        flip: false,
    });
    let sol = solve3d(&s, &vals(&s, &[8.0])).unwrap();
    let z = |id| sol.points[&id][2];
    assert!((z(ha) - z(la) - 8.0).abs() < 1e-9);
    assert!(
        (z(hb) - z(ha)).abs() < 1e-9,
        "XY に平行なので両端が同じ高さ"
    );
    assert!(matches!(sol.status, SolveStatus::UnderConstrained { .. }));
}

/// 底が 10 mm 角の四角錐。底の四隅は固定、頂点は面の角度 3 つで決める。
#[test]
fn face_angles_place_the_apex_of_a_pyramid() {
    let mut s = Sketch3d::default();
    let c = [[-5.0, -5.0], [5.0, -5.0], [5.0, 5.0], [-5.0, 5.0]];
    let pts: Vec<EntityId> = c.iter().map(|q| s.add_point([q[0], q[1], 0.0])).collect();
    let apex = s.add_point([1.0, 0.5, 4.0]);
    let mut base = vec![];
    let mut side = vec![];
    for k in 0..4 {
        base.push(s.add_line(pts[k], pts[(k + 1) % 4]));
        side.push(s.add_line(pts[k], apex));
    }
    for p in &pts {
        s.add_constraint(Kind3d::Fix { point: *p });
    }
    let face = |k: usize| FaceRef::new(vec![base[k], side[k], side[(k + 1) % 4]]);
    for k in 0..3 {
        s.add_constraint(Kind3d::FaceAngle {
            a: PlaneRef::Face { lines: face(k) },
            b: PlaneRef::Axis {
                plane: AxisPlane::Xy,
            },
            value: "50 deg".into(),
        });
    }
    let sol = solve3d(&s, &vals(&s, &[deg(50.0); 3])).unwrap();
    let a = sol.points[&apex];
    assert!(a[0].abs() < 1e-6 && a[1].abs() < 1e-6, "{a:?}");
    assert!((a[2] - 5.0 * deg(50.0).tan()).abs() < 1e-6, "{a:?}");
    assert_eq!(sol.status, SolveStatus::WellConstrained);
    // 底の四角と側面 4 つが面として見つかる
    assert_eq!(sol.faces.len(), 5, "{:?}", sol.faces);
    // 4 つ目の側面の角度も 50°(測った値)
    let k4 = Kind3d::FaceAngle {
        a: PlaneRef::Face { lines: face(3) },
        b: PlaneRef::Axis {
            plane: AxisPlane::Xy,
        },
        value: String::new(),
    };
    assert!((measure(&s, &sol, &k4).unwrap() - deg(50.0)).abs() < 1e-6);
}

#[test]
fn a_cube_has_six_faces() {
    let mut s = Sketch3d::default();
    let mut p = vec![];
    for i in 0..8 {
        p.push(s.add_point([
            (i & 1) as f64 * 10.0,
            ((i >> 1) & 1) as f64 * 10.0,
            ((i >> 2) & 1) as f64 * 10.0,
        ]));
    }
    for i in 0..8usize {
        for bit in [1, 2, 4] {
            if i & bit == 0 {
                s.add_line(p[i], p[i | bit]);
            }
        }
    }
    let sol = solve3d(&s, &HashMap::new()).unwrap();
    assert_eq!(sol.faces.len(), 6);
    assert_eq!(sol.status, SolveStatus::UnderConstrained { dof: 24 });
}

#[test]
fn a_line_at_an_angle_to_a_plane_and_another_line() {
    let mut s = Sketch3d::default();
    let base = s.add_segment([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]);
    let (o, e) = s.line_ends(base).unwrap();
    let up = s.add_point([3.0, 3.0, 3.0]);
    let l = s.add_line(o, up);
    s.add_constraint(Kind3d::Fix { point: o });
    s.add_constraint(Kind3d::Fix { point: e });
    s.add_constraint(Kind3d::Length {
        line: l,
        value: "10 mm".into(),
    });
    s.add_constraint(Kind3d::PlaneAngle {
        line: l,
        plane: PlaneRef::Axis {
            plane: AxisPlane::Xy,
        },
        value: "30 deg".into(),
    });
    s.add_constraint(Kind3d::Angle {
        a: base,
        b: l,
        value: "60 deg".into(),
    });
    let sol = solve3d(&s, &vals(&s, &[10.0, deg(30.0), deg(60.0)])).unwrap();
    let q = sol.points[&up];
    assert!((q[2] - 5.0).abs() < 1e-6, "高さ 10 sin 30° = 5: {q:?}");
    assert!((q[0] - 5.0).abs() < 1e-6, "x は 10 cos 60° = 5: {q:?}");
    assert_eq!(sol.status, SolveStatus::WellConstrained);
}

#[test]
fn duplicate_and_conflicting_constraints_are_reported() {
    let mut s = Sketch3d::default();
    let l = s.add_segment([0.0, 0.0, 0.0], [5.0, 0.1, 0.0]);
    s.add_constraint(Kind3d::Along {
        line: l,
        axis: Axis::X,
    });
    let dup = s.add_constraint(Kind3d::Along {
        line: l,
        axis: Axis::X,
    });
    let sol = solve3d(&s, &HashMap::new()).unwrap();
    assert_eq!(
        sol.status,
        SolveStatus::OverConstrained {
            redundant: vec![dup]
        }
    );
    let mut s = Sketch3d::default();
    let l = s.add_segment([0.0, 0.0, 0.0], [5.0, 0.0, 0.0]);
    let (a, b) = s.line_ends(l).unwrap();
    s.add_constraint(Kind3d::Length {
        line: l,
        value: "10 mm".into(),
    });
    s.add_constraint(Kind3d::Distance {
        a,
        b,
        value: "20 mm".into(),
    });
    let sol = solve3d(&s, &vals(&s, &[10.0, 20.0])).unwrap();
    assert!(matches!(sol.status, SolveStatus::Inconsistent { .. }));
}

#[test]
fn dragging_keeps_the_constraints() {
    let mut s = Sketch3d::default();
    let l = s.add_segment([0.0, 0.0, 0.0], [5.0, 0.0, 0.0]);
    let (a, b) = s.line_ends(l).unwrap();
    s.add_constraint(Kind3d::Fix { point: a });
    s.add_constraint(Kind3d::Along {
        line: l,
        axis: Axis::X,
    });
    let sol = solve3d_dragging(&s, &HashMap::new(), b, [12.0, 4.0, -3.0]).unwrap();
    let q = sol.points[&b];
    assert!((q[0] - 12.0).abs() < 1e-3, "{q:?}");
    assert!(q[1].abs() < 1e-9 && q[2].abs() < 1e-9, "{q:?}");
}

/// 長さの決まった折れ線の端を引く: 隣の点だけが付いて来て、その先の点は動かない
/// (2D と同じく、掴んだ点の近くから順に動かす)。
#[test]
fn dragging_an_end_moves_only_what_must_follow() {
    let mut s = Sketch3d::default();
    let ab = s.add_segment([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]);
    let (a, b) = s.line_ends(ab).unwrap();
    let c = s.add_point([20.0, 0.0, 0.0]);
    let d = s.add_point([30.0, 0.0, 0.0]);
    let bc = s.add_line(b, c);
    let cd = s.add_line(c, d);
    for line in [ab, bc, cd] {
        s.add_constraint(Kind3d::Length {
            line,
            value: "10 mm".into(),
        });
    }
    let v = vals(&s, &[10.0, 10.0, 10.0]);
    let sol = solve3d_dragging(&s, &v, a, [2.0, 3.0, 1.0]).unwrap();
    let pa = sol.points[&a];
    assert!(
        (pa[0] - 2.0).abs() < 1e-6 && (pa[1] - 3.0).abs() < 1e-6,
        "{pa:?}"
    );
    for (p, at) in [(c, [20.0, 0.0, 0.0]), (d, [30.0, 0.0, 0.0])] {
        let q = sol.points[&p];
        assert!(
            (0..3).all(|k| (q[k] - at[k]).abs() < 1e-12),
            "動かない点が動いた: {q:?}"
        );
    }
    let (pb, pc) = (sol.points[&b], sol.points[&c]);
    let l = |p: [f64; 3], q: [f64; 3]| {
        ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt()
    };
    assert!((l(pa, pb) - 10.0).abs() < 1e-6 && (l(pb, pc) - 10.0).abs() < 1e-6);
}

#[test]
fn a_circle_has_a_radius_and_fixing_it_stops_its_size() {
    let mut s = Sketch3d::default();
    let c = s.add_point([0.0, 0.0, 3.0]);
    let circle = s.add_circle(c, 4.0, [0.0, 1.0, 0.0]);
    s.add_constraint(Kind3d::Fix { point: circle });
    let sol = solve3d(&s, &HashMap::new()).unwrap();
    assert_eq!(sol.status, SolveStatus::WellConstrained);
    let lines = polylines(&s, &sol);
    // 法線が Y なので、円は y = 0 の平面の上
    assert!(lines[0].0.iter().all(|p| p[1].abs() < 1e-9));
}

#[test]
fn the_sketch_round_trips_through_json() {
    let mut s = Sketch3d::default();
    let l = s.add_segment([0.0, 0.0, 0.0], [5.0, 0.0, 0.0]);
    s.add_constraint(Kind3d::Along {
        line: l,
        axis: Axis::X,
    });
    let v = serde_json::to_value(&s).unwrap();
    let back: Sketch3d = serde_json::from_value(v).unwrap();
    assert_eq!(back, s);
}

#[test]
fn points_on_an_offset_work_plane_follow_its_offset() {
    let mut s = Sketch3d::default();
    let w = s.add_work_plane(PlaneDef::Offset {
        base: AxisPlane::Xy,
        offset: 10.0,
    });
    let p = s.add_point([3.0, 4.0, 7.0]);
    s.add_constraint(Kind3d::PointOnPlane {
        point: p,
        plane: PlaneRef::Work { id: w },
    });
    let sol = solve3d(&s, &HashMap::new()).unwrap();
    assert!(
        (sol.points[&p][2] - 10.0).abs() < 1e-9,
        "{:?}",
        sol.points[&p]
    );
    // 作図面をずらすと点も付いて動く
    if let Some(PlaneDef::Offset { offset, .. }) = s.work_plane_mut(w).map(|x| &mut x.def) {
        *offset = -2.5;
    }
    let sol = solve3d(&s, &HashMap::new()).unwrap();
    assert!(
        (sol.points[&p][2] + 2.5).abs() < 1e-9,
        "{:?}",
        sol.points[&p]
    );
}

#[test]
fn a_tilted_work_plane_makes_its_angle_with_the_base() {
    // XY 平面の上の X に沿う線分を軸に、XY 平面から 30° 傾けた作図面。作図面に乗る Y の向きの
    // 線分は、XY 平面と 30° になる
    let mut s = Sketch3d::default();
    let hinge = s.add_segment([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]);
    let (ha, hb) = s.line_ends(hinge).unwrap();
    s.add_constraint(Kind3d::Fix { point: ha });
    s.add_constraint(Kind3d::Fix { point: hb });
    let w = s.add_work_plane(PlaneDef::Tilted {
        line: hinge,
        base: PlaneRef::Axis {
            plane: AxisPlane::Xy,
        },
        angle: deg(30.0),
        flip: false,
    });
    let a = s.add_point([0.0, 0.0, 0.0]);
    let b = s.add_point([0.0, 8.0, 1.0]);
    let l = s.add_line(a, b);
    s.add_constraint(Kind3d::Coincident { a, b: ha });
    s.add_constraint(Kind3d::PointOnPlane {
        point: b,
        plane: PlaneRef::Work { id: w },
    });
    s.add_constraint(Kind3d::Perpendicular { a: l, b: hinge });
    s.add_constraint(Kind3d::PlaneAngle {
        line: l,
        plane: PlaneRef::Axis {
            plane: AxisPlane::Xy,
        },
        value: "0 deg".into(),
    });
    // 角度の寸法は被駆動にして、今の角度を測る
    s.constraints.last_mut().unwrap().reference = true;
    let sol = solve3d(&s, &HashMap::new()).unwrap();
    let id = s.constraints.last().unwrap().id;
    assert!(
        (sol.measured[&id] - deg(30.0)).abs() < 1e-6,
        "{}",
        sol.measured[&id].to_degrees()
    );
    let (o, n) = plane_at(&s, &sol, &PlaneRef::Work { id: w }).unwrap();
    assert!(geom::dot(geom::sub(sol.points[&b], o), n).abs() < 1e-9);
    // 反対へ傾けると z の符号が替わる
    let z = sol.points[&b][2];
    if let Some(PlaneDef::Tilted { flip, .. }) = s.work_plane_mut(w).map(|x| &mut x.def) {
        *flip = true;
    }
    let sol = solve3d(&s, &HashMap::new()).unwrap();
    assert!(sol.points[&b][2] * z < 0.0, "{} {}", z, sol.points[&b][2]);
}

#[test]
fn a_three_point_work_plane_moves_with_its_points_and_carries_circles() {
    let mut s = Sketch3d::default();
    let a = s.add_point([0.0, 0.0, 0.0]);
    let b = s.add_point([10.0, 0.0, 0.0]);
    let c = s.add_point([0.0, 10.0, 0.0]);
    for p in [a, b] {
        s.add_constraint(Kind3d::Fix { point: p });
    }
    let w = s.add_work_plane(PlaneDef::ThreePoints { a, b, c });
    let center = s.add_point([2.0, 2.0, 0.0]);
    let circle = s.add_circle(center, 1.0, [0.0, 0.0, 1.0]);
    if let Some(Entity3d::Circle { plane, .. }) = s.entity_mut(circle) {
        *plane = Some(w);
    }
    s.add_constraint(Kind3d::PointOnPlane {
        point: center,
        plane: PlaneRef::Work { id: w },
    });
    // c を真上へ上げる(XZ 平面に立てる)と、円の中心と向きが付いてくる
    s.add_constraint(Kind3d::AxisDistance {
        a,
        b: c,
        axis: Axis::Z,
        value: "10 mm".into(),
        flip: false,
    });
    s.add_constraint(Kind3d::AxisDistance {
        a,
        b: c,
        axis: Axis::Y,
        value: "0 mm".into(),
        flip: false,
    });
    let sol = solve3d(&s, &vals(&s, &[10.0, 0.0])).unwrap();
    assert!(
        sol.points[&center][1].abs() < 1e-6,
        "{:?}",
        sol.points[&center]
    );
    let n = sol.normals[&circle];
    assert!((n[1].abs() - 1.0).abs() < 1e-6, "{n:?}");
    let lines = polylines(&s, &sol);
    assert!(
        lines[0]
            .0
            .iter()
            .all(|q| (q[1] - sol.points[&center][1]).abs() < 1e-6)
    );
}

#[test]
fn removing_a_point_removes_the_planes_and_constraints_built_on_it() {
    let mut s = Sketch3d::default();
    let a = s.add_point([0.0, 0.0, 0.0]);
    let b = s.add_point([1.0, 0.0, 0.0]);
    let c = s.add_point([0.0, 1.0, 0.0]);
    let w = s.add_work_plane(PlaneDef::ThreePoints { a, b, c });
    let hinge = s.add_line(a, b);
    let t = s.add_work_plane(PlaneDef::Tilted {
        line: hinge,
        base: PlaneRef::Work { id: w },
        angle: deg(10.0),
        flip: false,
    });
    let p = s.add_point([5.0, 5.0, 5.0]);
    s.add_constraint(Kind3d::PointOnPlane {
        point: p,
        plane: PlaneRef::Work { id: t },
    });
    s.remove_entities(&[c]);
    assert!(s.planes.is_empty(), "{:?}", s.planes);
    assert!(s.constraints.is_empty());
    assert!(s.entity(p).is_some());
    // 作図面を持たない古いデータも読める
    let old: Sketch3d =
        serde_json::from_value(serde_json::json!({"entities": [], "constraints": []})).unwrap();
    assert!(old.planes.is_empty());
}

/// 0〜10 の立方体の三角形(スケッチの座標)。
fn cube() -> Vec<body::Tri> {
    let v = |i: usize| -> P3 {
        [
            if i & 1 != 0 { 10.0 } else { 0.0 },
            if i & 2 != 0 { 10.0 } else { 0.0 },
            if i & 4 != 0 { 10.0 } else { 0.0 },
        ]
    };
    let quads = [
        [0, 1, 3, 2],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 3, 7, 6],
        [0, 2, 6, 4],
        [1, 3, 7, 5],
    ];
    quads
        .iter()
        .flat_map(|q| [[v(q[0]), v(q[1]), v(q[2])], [v(q[0]), v(q[2]), v(q[3])]])
        .collect()
}

#[test]
fn a_plane_through_a_cube_gives_one_closed_section() {
    let lines = body::section(&cube(), [0.0, 0.0, 4.0], [0.0, 0.0, 1.0]);
    assert_eq!(lines.len(), 1, "{lines:?}");
    let l = &lines[0];
    assert!(
        geom::len(geom::sub(l[0], *l.last().unwrap())) < 1e-6,
        "閉じていない"
    );
    assert!(l.iter().all(|p| (p[2] - 4.0).abs() < 1e-9));
    // 周の長さは 40
    let per: f64 = l.windows(2).map(|w| geom::len(geom::sub(w[1], w[0]))).sum();
    assert!((per - 40.0).abs() < 1e-6, "{per}");
}

/// 交差の「押した線」は面ごとにつなげた線で数える(立方体の 4 つの側面の断面は 1 本ずつ)。
#[test]
fn section_pieces_are_counted_per_face() {
    let names = ["bottom", "top", "front", "back", "left", "right"];
    let tris: Vec<(body::Tri, String)> = cube()
        .into_iter()
        .enumerate()
        .map(|(i, t)| (t, names[i / 2].to_string()))
        .collect();
    let pieces = body::section_pieces(&tris, [0.0, 0.0, 4.0], [0.0, 0.0, 1.0]);
    let faces: Vec<&str> = pieces.iter().map(|p| p.0.as_str()).collect();
    assert_eq!(faces, ["back", "front", "left", "right"], "{pieces:?}");
    assert!(pieces.iter().all(|p| p.1 == 0));
    for (_, _, l) in &pieces {
        let per: f64 = l.windows(2).map(|w| geom::len(geom::sub(w[1], w[0]))).sum();
        assert!((per - 10.0).abs() < 1e-6, "{per}");
    }
}

/// 投影・押した線を足す前に保存した交差も読め、新しい形は書いて読み戻せる。
#[test]
fn derived_lines_onto_a_plane_read_old_data_and_round_trip() {
    let old: Sketch3d = serde_json::from_value(serde_json::json!({
        "derived": [{"id": 1, "by": "section", "plane": {"kind": "axis", "plane": "xy"}, "bodies": [3]}]
    }))
    .unwrap();
    assert!(matches!(
        &old.derived[0].def,
        DerivedDef::Section { pieces, .. } if pieces.is_empty()
    ));
    let mut s = Sketch3d::default();
    s.add_derived(DerivedDef::Project {
        plane: PlaneRef::Work { id: 2 },
        what: ProjectWhat::Parts {
            sources: vec![kmb_sketch::ProjectionSource::Edge {
                feature: kmb_core::FeatureId(3),
                faces: ["a".into(), "b".into()],
            }],
        },
        bodies: vec![kmb_core::FeatureId(3)],
    });
    s.add_derived(DerivedDef::Section {
        plane: PlaneRef::Axis {
            plane: AxisPlane::Zx,
        },
        bodies: vec![kmb_core::FeatureId(3)],
        pieces: vec![body::piece("side", 1)],
    });
    let back: Sketch3d = serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
    assert_eq!(back, s);
    // 写す先の作図面を消すと、写した線も消える
    s.remove_work_plane(2);
    assert_eq!(s.derived.len(), 1);
}

#[test]
fn a_line_above_a_cube_projects_onto_its_top_and_intersects_both_faces() {
    let mut s = Sketch3d::default();
    let l = s.add_segment([2.0, 5.0, 20.0], [8.0, 5.0, 20.0]);
    let sol = solve3d(&s, &HashMap::new()).unwrap();
    let on = body::derived_lines(
        &s,
        &sol,
        &DerivedDef::OnSurface {
            curves: vec![l],
            dir: [0.0, 0.0, -1.0],
            bodies: vec![],
        },
        &cube(),
    );
    assert_eq!(on.len(), 1, "{on:?}");
    assert!(
        on[0].iter().all(|p| (p[2] - 10.0).abs() < 1e-9),
        "{:?}",
        on[0]
    );
    let both = body::derived_lines(
        &s,
        &sol,
        &DerivedDef::Intersection {
            curves: vec![l],
            dir: [0.0, 0.0, 1.0],
            bodies: vec![],
        },
        &cube(),
    );
    assert_eq!(both.len(), 2, "上と下の面: {both:?}");
    // 外れる線は何も出ない
    let mut far = Sketch3d::default();
    let m = far.add_segment([20.0, 5.0, 20.0], [30.0, 5.0, 20.0]);
    let fsol = solve3d(&far, &HashMap::new()).unwrap();
    let none = body::derived_lines(
        &far,
        &fsol,
        &DerivedDef::OnSurface {
            curves: vec![m],
            dir: [0.0, 0.0, -1.0],
            bodies: vec![],
        },
        &cube(),
    );
    assert!(none.is_empty());
}

#[test]
fn included_points_do_not_move_and_lines_can_hang_on_them() {
    let mut s = Sketch3d::default();
    let frame = kmb_core::Plane::XY;
    let item = body::BodyItem {
        source: kmb_sketch::ProjectionSource::Edge {
            feature: kmb_core::FeatureId(7),
            faces: ["a".into(), "b".into()],
        },
        shape: body::BodyShape::Line([0.0, 0.0, 5.0], [10.0, 0.0, 5.0]),
    };
    let e = body::include_item(&mut s, &item, &frame);
    assert_eq!(
        body::include_item(&mut s, &item, &frame),
        e,
        "二度は取り込まない"
    );
    let (a, _) = s.line_ends(e).unwrap();
    let l = s.add_segment([0.0, 0.0, 5.0], [3.0, 4.0, 9.0]);
    let (p, q) = s.line_ends(l).unwrap();
    s.add_constraint(Kind3d::Coincident { a: p, b: a });
    s.add_constraint(Kind3d::Length {
        line: l,
        value: "20 mm".into(),
    });
    let sol = solve3d(&s, &vals(&s, &[20.0])).unwrap();
    assert_eq!(sol.points[&a], [0.0, 0.0, 5.0]);
    assert!((geom::len(geom::sub(sol.points[&q], sol.points[&p])) - 20.0).abs() < 1e-6);
    // 元の辺が動けば、計算の時に付いて動く
    let moved = body::BodyItem {
        shape: body::BodyShape::Line([0.0, 0.0, 8.0], [10.0, 0.0, 8.0]),
        ..item
    };
    assert!(body::refresh_includes(&mut s, &[moved], &frame).is_empty());
    let sol = solve3d(&s, &vals(&s, &[20.0])).unwrap();
    assert!((sol.points[&p][2] - 8.0).abs() < 1e-9);
    // 消すと取り込みの記録も消える
    s.remove_entities(&[e]);
    assert!(s.includes.is_empty());
}

#[test]
fn a_3d_circle_and_its_center_are_fixed_separately() {
    let mut s = Sketch3d::default();
    let c = s.add_point([0.0, 0.0, 0.0]);
    let circle = s.add_circle(c, 5.0, [0.0, 0.0, 1.0]);
    s.add_constraint(Kind3d::Fix { point: c });
    s.add_constraint(Kind3d::Fix { point: circle });
    let sol = solve3d(&s, &HashMap::new()).unwrap();
    assert_eq!(
        sol.status,
        kmb_sketch::SolveStatus::WellConstrained,
        "{:?}",
        sol.status
    );
}

#[test]
fn sketch3d_dimensions_are_named_too() {
    let mut s = Sketch3d::default();
    let a = s.add_point([0.0, 0.0, 0.0]);
    let b = s.add_point([10.0, 0.0, 0.0]);
    let l = s.add_line(a, b);
    let c = s.add_constraint(Kind3d::Length {
        line: l,
        value: "12 mm".into(),
    });
    let s = s.with_dim_names(&mut kmb_core::Names::new(["d1"])).unwrap();
    assert_eq!(s.dim_name(c), Some("d2"));
    assert_eq!(s.model_params()[0].expr, "12 mm");
    let t = s.with_dim_value("d2", "d2b").unwrap();
    assert_eq!(t.constraint(c).unwrap().kind.value(), Some("d2b"));
    let u = s.renamed("d2", "len").unwrap().unwrap();
    assert_eq!(u.dim_name(c), Some("len"));
}
