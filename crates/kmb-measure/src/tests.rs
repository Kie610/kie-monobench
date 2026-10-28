use super::*;

fn quad(z: f64, x0: f64, x1: f64) -> Vec<[P; 3]> {
    let (a, b, c, d) = ([x0, 0.0, z], [x1, 0.0, z], [x1, 10.0, z], [x0, 10.0, z]);
    vec![[a, b, c], [a, c, d]]
}

fn face(tris: Vec<[P; 3]>, normal: P) -> Geo {
    Geo {
        kind: Kind::Plane {
            normal,
            point: tris[0][0],
            area: 0.0,
        },
        points: vec![],
        segs: vec![],
        tris,
    }
}

fn point(p: P) -> Geo {
    Geo {
        kind: Kind::Point(p),
        points: vec![p],
        segs: vec![],
        tris: vec![],
    }
}

fn row<'a>(r: &'a Report, k: &str) -> &'a str {
    &r.rows.iter().find(|x| x.0 == k).unwrap().1
}

#[test]
fn values_follow_the_document_unit_on_the_screen() {
    // 候補 19: 画面の単位がインチなら、長さ・位置・面積をインチで書く(向きは変えない)
    let line = Geo {
        kind: Kind::Line {
            a: [0.0, 0.0, 0.0],
            b: [25.4, 0.0, 0.0],
        },
        points: vec![],
        segs: vec![],
        tris: vec![],
    };
    let plane = Geo {
        kind: Kind::Plane {
            normal: [0.0, 0.0, 1.0],
            point: [0.0; 3],
            area: 645.16,
        },
        points: vec![],
        segs: vec![],
        tris: vec![],
    };
    kmb_core::set_display_unit(kmb_core::LengthUnit::In);
    let d = describe(&line);
    let p = describe(&plane);
    kmb_core::set_display_unit(kmb_core::LengthUnit::Mm);
    assert_eq!(d[0].1, "1.000 in");
    assert_eq!(d[2].1, "1.000, 0.000, 0.000 in");
    assert_eq!(p[0].1, "1.000 in²");
    assert_eq!(p[1].1, "0, 0, 1");
    assert_eq!(describe(&line)[0].1, "25.400 mm");
    assert_eq!(describe(&line)[2].1, "25.400, 0.000, 0.000");
}

#[test]
fn parallel_faces_give_the_gap_and_offset_faces_the_shortest_distance() {
    // 重なる平行な面: 最短の距離 = 間隔 = 8
    let top = face(quad(8.0, 0.0, 10.0), [0.0, 0.0, 1.0]);
    let bottom = face(quad(0.0, 0.0, 10.0), [0.0, 0.0, -1.0]);
    let r = compare(&top, &bottom, Basis::Center, true);
    assert_eq!(row(&r, "最短の距離"), "8.000 mm");
    assert_eq!(row(&r, "平行な面の間隔"), "8.000 mm");
    assert_eq!(row(&r, "角度"), "平行");
    // ずれた平行な面: 最短の距離は斜め、間隔は 8 のまま
    let off = face(quad(0.0, 16.0, 20.0), [0.0, 0.0, -1.0]);
    let r = compare(&top, &off, Basis::Center, true);
    assert_eq!(row(&r, "最短の距離"), "10.000 mm"); // √(6² + 8²)
    assert_eq!(row(&r, "平行な面の間隔"), "8.000 mm");
    assert_eq!(row(&r, "ΔX"), "6.000 mm");
    assert_eq!(row(&r, "ΔZ"), "8.000 mm");
}

#[test]
fn angle_between_faces_and_point_to_face() {
    let top = face(quad(8.0, 0.0, 10.0), [0.0, 0.0, 1.0]);
    let side = face(
        vec![
            [[0.0, 0.0, 0.0], [0.0, 10.0, 0.0], [0.0, 10.0, 8.0]],
            [[0.0, 0.0, 0.0], [0.0, 10.0, 8.0], [0.0, 0.0, 8.0]],
        ],
        [-1.0, 0.0, 0.0],
    );
    let r = compare(&top, &side, Basis::Center, true);
    assert_eq!(row(&r, "最短の距離"), "0.000 mm");
    assert_eq!(row(&r, "角度"), "90.00°");
    let r = compare(&point([5.0, 5.0, 20.0]), &top, Basis::Center, true);
    assert_eq!(row(&r, "最短の距離"), "12.000 mm");
    assert_eq!(row(&r, "面の平面までの距離"), "12.000 mm");
    // 面の外の点: 最短の距離は斜め、平面までの距離は高さ
    assert_eq!(r.line, Some(([5.0, 5.0, 20.0], [5.0, 5.0, 8.0])));
    let r = compare(&point([13.0, 5.0, 12.0]), &top, Basis::Center, true);
    assert_eq!(row(&r, "最短の距離"), "5.000 mm");
    assert_eq!(row(&r, "面の平面までの距離"), "4.000 mm");
}

#[test]
fn segments_cross_and_skew() {
    let a = Geo {
        kind: Kind::Line {
            a: [0.0, 0.0, 0.0],
            b: [10.0, 0.0, 0.0],
        },
        points: vec![],
        segs: vec![[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]]],
        tris: vec![],
    };
    let b = Geo {
        kind: Kind::Line {
            a: [5.0, -5.0, 3.0],
            b: [5.0, 5.0, 3.0],
        },
        points: vec![],
        segs: vec![[[5.0, -5.0, 3.0], [5.0, 5.0, 3.0]]],
        tris: vec![],
    };
    let r = compare(&a, &b, Basis::Center, true);
    assert_eq!(row(&r, "最短の距離"), "3.000 mm");
    assert_eq!(row(&r, "角度"), "90.00°");
}

#[test]
fn circle_from_mesh_points_gives_diameter_and_center() {
    let c = [12.0, 6.0, 8.0];
    let r = 1.6;
    let pts: Vec<P> = (0..=48)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / 48.0;
            // 網は f32 なので丸める
            [c[0] + r * a.cos(), c[1] + r * a.sin(), c[2]].map(|v| v as f32 as f64)
        })
        .collect();
    let k = fit_circle(&pts).unwrap();
    assert!(k.closed);
    assert!((k.radius - r).abs() < 1e-5, "{}", k.radius);
    assert!(dist(k.center, c) < 1e-5);
    // 直線は円でない
    assert!(fit_circle(&[[0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0], [3.0, 0.0, 0.0]]).is_none());
    // 円と板の縁: 中心からの距離
    let g = Geo {
        kind: Kind::Circle(k),
        points: vec![],
        segs: pts.windows(2).map(|w| [w[0], w[1]]).collect(),
        tris: vec![],
    };
    let edge = Geo {
        kind: Kind::Line {
            a: [0.0, 0.0, 8.0],
            b: [0.0, 40.0, 8.0],
        },
        points: vec![],
        segs: vec![[[0.0, 0.0, 8.0], [0.0, 40.0, 8.0]]],
        tris: vec![],
    };
    let rep = compare(&g, &edge, Basis::Center, true);
    assert_eq!(row(&rep, "中心からの距離"), "12.000 mm");
    assert_eq!(row(&rep, "最短の距離"), "10.400 mm");
    assert_eq!(describe(&g)[0], ("直径".into(), "3.200 mm".into()));
}

#[test]
fn circle_edges_of_the_mesh_are_measured_on_the_true_circle() {
    // 網の稜線の円(48 区間)と、板の縁。弦のまま測ると 10.403 mm になる
    let (c, r) = ([12.0f32, 6.0, 8.0], 1.6f32);
    let circle: Vec<[f32; 3]> = (0..=48)
        .map(|i| {
            let a = std::f32::consts::TAU * (i % 48) as f32 / 48.0 + 0.5;
            [c[0] + r * a.cos(), c[1] + r * a.sin(), c[2]]
        })
        .collect();
    let mesh = kmb_core::Mesh {
        edges: vec![circle, vec![[0.0, 0.0, 8.0], [0.0, 40.0, 8.0]]],
        ..Default::default()
    };
    let f = FeatureId(7);
    let scene = vec![(f, mesh)];
    let hole = resolve(
        &scene,
        &Target::Edge {
            feature: f,
            edge: 0,
        },
    )
    .unwrap();
    let rim = resolve(
        &scene,
        &Target::Edge {
            feature: f,
            edge: 1,
        },
    )
    .unwrap();
    assert!(matches!(hole.kind, Kind::Circle(k) if k.closed));
    assert!(matches!(rim.kind, Kind::Line { .. }));
    let rep = compare(&hole, &rim, Basis::Center, true);
    assert_eq!(row(&rep, "最短の距離"), "10.400 mm");
    assert_eq!(row(&rep, "中心からの距離"), "12.000 mm");
    let center = resolve(
        &scene,
        &Target::Center {
            feature: f,
            edge: 0,
        },
    )
    .unwrap();
    assert_eq!(describe(&center)[0].1, "12.000, 6.000, 8.000");
}

/// 円柱の面の網(`n` 区間、角度 `a0` から `sweep` まで、高さ `z0`〜`z1`)。対角を交互に変えて切る。
fn cylinder_mesh(c: [f64; 2], r: f64, a0: f64, sweep: f64, n: usize, z: (f64, f64)) -> Vec<[P; 3]> {
    let at = |i: usize, top: bool| {
        let a = a0 + sweep * i as f64 / n as f64;
        [
            c[0] + r * a.cos(),
            c[1] + r * a.sin(),
            if top { z.1 } else { z.0 },
        ]
        .map(|v| v as f32 as f64)
    };
    let mut t = Vec::new();
    for i in 0..n {
        let (a, b, cc, d) = (at(i, false), at(i + 1, false), at(i + 1, true), at(i, true));
        if i % 2 == 0 {
            t.push([a, b, cc]);
            t.push([a, cc, d]);
        } else {
            t.push([a, b, d]);
            t.push([b, cc, d]);
        }
    }
    t
}

#[test]
fn cylinder_faces_give_diameter_axis_and_sweep() {
    use std::f64::consts::{PI, TAU};
    let full = fit_cylinder(&cylinder_mesh([12.0, 6.0], 4.0, 0.3, TAU, 36, (0.0, 8.0))).unwrap();
    assert!(full.closed);
    assert!((full.radius - 4.0).abs() < 1e-5, "{}", full.radius);
    assert!(dot(full.axis, [0.0, 0.0, 1.0]).abs() > 1.0 - 1e-9);
    assert!(dist(full.center, [12.0, 6.0, 4.0]) < 1e-4);
    assert!((full.length - 8.0).abs() < 1e-5);
    let half = fit_cylinder(&cylinder_mesh(
        [0.0, 0.0],
        3.0,
        PI / 2.0,
        PI,
        18,
        (0.0, 5.0),
    ))
    .unwrap();
    assert!(!half.closed);
    assert!((half.sweep - PI).abs() < 1e-6, "{}", half.sweep);
    // 弧の真ん中は -X の向き
    assert!(dot(half.bulge, [-1.0, 0.0, 0.0]) > 0.999);
    // 平らな面は円柱でない
    assert!(fit_cylinder(&quad(0.0, 0.0, 10.0)).is_none());
    let g = Geo {
        kind: Kind::Cylinder(full),
        points: vec![],
        segs: vec![],
        tris: vec![],
    };
    let d = describe(&g);
    assert_eq!(d[0], ("直径".into(), "8.000 mm".into()));
    assert_eq!(d[2], ("軸の向き".into(), "0, 0, 1".into()));
}

fn cyl_geo(c: [f64; 2], r: f64) -> Geo {
    let k = fit_cylinder(&cylinder_mesh(
        c,
        r,
        0.0,
        std::f64::consts::TAU,
        36,
        (0.0, 8.0),
    ))
    .unwrap();
    Geo {
        kind: Kind::Cylinder(k),
        points: vec![],
        segs: vec![],
        tris: cylinder_tris(&k),
    }
}

#[test]
fn holes_measure_axis_to_axis_and_axis_to_face_with_min_and_max() {
    let a = cyl_geo([10.0, 5.0], 2.0);
    let b = cyl_geo([40.0, 45.0], 3.0);
    let r = compare(&a, &b, Basis::Center, true);
    assert_eq!(r.bases, vec![Basis::Center, Basis::Min, Basis::Max]);
    assert_eq!(row(&r, "軸どうしの距離"), "50.000 mm");
    assert_eq!(row(&r, "最短の距離"), "45.000 mm");
    assert_eq!(row(&r, "最長の距離"), "55.579 mm"); // √(55² + 8²)
    assert_eq!(row(&r, "ΔX"), "30.000 mm");
    assert_eq!(row(&r, "ΔY"), "40.000 mm");
    assert_eq!(row(&r, "ΔZ"), "0.000 mm");
    assert_eq!(r.label.as_deref(), Some("50.000 mm"));
    // 最短に切り替えると、線と ΔX・ΔY・ΔZ も最短の 2 点で
    let m = compare(&a, &b, Basis::Min, true);
    assert_eq!(m.basis, Basis::Min);
    assert_eq!(m.rows[m.primary].0, "最短の距離");
    assert_eq!(compare(&a, &b, Basis::Max, true).rows[2].0, "最長の距離");
    assert_eq!(compare(&a, &b, Basis::Max, true).primary, 2);
    assert_eq!(m.label.as_deref(), Some("45.000 mm"));
    // 最短の 2 点は刻んだ円柱の上なので、位置は刻みの分だけずれる
    let dx: f64 = row(&m, "ΔX").trim_end_matches(" mm").parse().unwrap();
    assert!((dx - 27.0).abs() < 0.02, "{dx}");
    // ΔX・ΔY・ΔZ を切ると行が消える
    assert!(
        compare(&a, &b, Basis::Center, false)
            .rows
            .iter()
            .all(|x| x.0 != "ΔX")
    );
    // 軸と、軸に平行な横の面(x = 0): 軸からの距離
    let side = face(
        vec![
            [[0.0, 0.0, 0.0], [0.0, 10.0, 0.0], [0.0, 10.0, 8.0]],
            [[0.0, 0.0, 0.0], [0.0, 10.0, 8.0], [0.0, 0.0, 8.0]],
        ],
        [-1.0, 0.0, 0.0],
    );
    let r = compare(&a, &side, Basis::Center, true);
    assert_eq!(row(&r, "軸からの距離"), "10.000 mm");
    assert_eq!(row(&r, "最短の距離"), "8.000 mm");
    // 軸に垂直な面とは中心で測れないので、最短と最長だけ
    let top = face(quad(8.0, 0.0, 10.0), [0.0, 0.0, 1.0]);
    let r = compare(&a, &top, Basis::Center, true);
    assert_eq!(r.bases, vec![Basis::Min, Basis::Max]);
    assert_eq!(r.basis, Basis::Min);
    // 丸い物が無ければ切り替えは出さない
    let r = compare(&top, &side, Basis::Center, true);
    assert!(r.bases.is_empty());
}

/// 長穴(中心 (0,0) と (20,0)、幅 6、深さ 5)の側面の網。端の半円は 2 つに分けた物も試す。
fn slot_scene(split: bool) -> (Vec<(FeatureId, Mesh)>, FeatureId) {
    use std::f64::consts::PI;
    let mut faces: Vec<Vec<[P; 3]>> = Vec::new();
    if split {
        faces.push(cylinder_mesh(
            [0.0, 0.0],
            3.0,
            PI / 2.0,
            PI / 2.0,
            9,
            (0.0, 5.0),
        ));
        faces.push(cylinder_mesh([0.0, 0.0], 3.0, PI, PI / 2.0, 9, (0.0, 5.0)));
    } else {
        faces.push(cylinder_mesh([0.0, 0.0], 3.0, PI / 2.0, PI, 18, (0.0, 5.0)));
    }
    faces.push(cylinder_mesh(
        [20.0, 0.0],
        3.0,
        -PI / 2.0,
        PI,
        18,
        (0.0, 5.0),
    ));
    // まっすぐな側面(y = ±3)
    for y in [3.0, -3.0] {
        let (a, b, c, d) = ([0.0, y, 0.0], [20.0, y, 0.0], [20.0, y, 5.0], [0.0, y, 5.0]);
        faces.push(vec![[a, b, c], [a, c, d]]);
    }
    // 離れた所の別の半円(長穴の相手にならない)
    faces.push(cylinder_mesh([0.0, 30.0], 3.0, 0.0, PI, 18, (0.0, 5.0)));
    let mut mesh = kmb_core::Mesh::default();
    for (k, tris) in faces.iter().enumerate() {
        mesh.face_names.push(format!("f1:{k}"));
        for t in tris {
            let base = mesh.positions.len() as u32;
            for p in t {
                mesh.positions.push(p.map(|v| v as f32));
                mesh.normals.push([0.0, 0.0, 1.0]);
            }
            mesh.triangles.push([base, base + 1, base + 2]);
            mesh.triangle_faces.push(k as u32);
        }
    }
    // 上の縁: 2 つの円弧
    let arc = |c: [f32; 2], a0: f32| -> Vec<[f32; 3]> {
        (0..=18)
            .map(|i| {
                let a = a0 + std::f32::consts::PI * i as f32 / 18.0;
                [c[0] + 3.0 * a.cos(), c[1] + 3.0 * a.sin(), 5.0]
            })
            .collect()
    };
    mesh.edges
        .push(arc([0.0, 0.0], std::f32::consts::FRAC_PI_2));
    mesh.edges
        .push(arc([20.0, 0.0], -std::f32::consts::FRAC_PI_2));
    mesh.edges.push(vec![[0.0, 3.0, 5.0], [20.0, 3.0, 5.0]]);
    let f = FeatureId(1);
    let scene = vec![(f, mesh)];
    (scene, f)
}

#[test]
fn slot_ends_give_width_pitch_and_overall_length() {
    for split in [false, true] {
        let (scene, f) = slot_scene(split);
        let pick = |face: &str| {
            Target::Face(Pick {
                feature: f,
                face: face.into(),
            })
        };
        let far = if split { "f1:2" } else { "f1:1" };
        for t in [
            pick("f1:0"),
            pick(far),
            Target::Edge {
                feature: f,
                edge: 0,
            },
        ] {
            let s = slot(&scene, &t).unwrap_or_else(|| panic!("{split} {t:?}"));
            let rows = describe_slot(&s);
            assert_eq!(rows[0].1, "6.000 mm");
            assert_eq!(rows[1].1, "20.000 mm");
            assert_eq!(rows[2].1, "26.000 mm");
        }
        // まっすぐな側面や、相手のいない半円は長穴にしない
        let lone = if split { "f1:5" } else { "f1:4" };
        assert!(slot(&scene, &pick(lone)).is_none());
        let side = if split { "f1:3" } else { "f1:2" };
        assert!(slot(&scene, &pick(side)).is_none());
    }
}

#[test]
fn two_edges_sharing_a_corner_report_the_obtuse_angle() {
    // 10-24 u14: 120° の角を共有する 2 本の辺は 120° と出す(前は 60°)。共有しない辺は 0〜90° のまま
    let line = |a: P, b: P| Geo {
        kind: Kind::Line { a, b },
        points: vec![],
        segs: vec![[a, b]],
        tris: vec![],
    };
    let r = 120f64.to_radians();
    let e1 = line([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]);
    let e2 = line([10.0 * r.cos(), 10.0 * r.sin(), 0.0], [0.0, 0.0, 0.0]);
    let rep = compare(&e1, &e2, Basis::Center, true);
    assert_eq!(row(&rep, "角度"), "120.00°");
    let apart = line([10.0 * r.cos(), 10.0 * r.sin(), 5.0], [0.0, 0.0, 5.0]);
    let rep = compare(&e1, &apart, Basis::Center, true);
    assert_eq!(row(&rep, "角度"), "60.00°");
}

/// 同じ部品を何百個置いても、網にするのは元の形 1 回だけ。出現の網は元の網を行列で動かした物で、
/// 鏡に映して置いた出現は三角形の向きを裏返して表を外へ保つ。
#[test]
fn occurrences_reuse_the_mesh_of_their_source() {
    use kmb_core::kernel::{Matrix4, translation};
    struct Placed {
        made: std::sync::atomic::AtomicUsize,
    }
    fn place(h: ShapeHandle) -> Matrix4 {
        if h.0 == 999 {
            let mut m = translation([0.0, 0.0, 0.0]);
            m[0][0] = -1.0;
            return m;
        }
        translation([h.0 as f64 * 20.0, 0.0, 0.0])
    }
    impl Kernel for Placed {
        fn name(&self) -> &str {
            "placed-test"
        }
        fn instance_of(&self, h: ShapeHandle) -> Option<(ShapeHandle, Matrix4)> {
            (h.0 > 1).then(|| (ShapeHandle(1), place(h)))
        }
        fn mesh(&self, h: ShapeHandle, _: f64) -> Result<Mesh, kmb_core::KernelError> {
            self.made.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let m = Mesh {
                positions: vec![[0., 0., 0.], [10., 0., 0.], [0., 10., 0.]],
                normals: vec![[0., 0., 1.]; 3],
                triangles: vec![[0, 1, 2]],
                triangle_faces: vec![0],
                face_names: vec!["上".into()],
                edges: vec![vec![[0., 0., 0.], [10., 0., 0.]]],
            };
            Ok(match self.instance_of(h) {
                Some((_, p)) => placed_mesh(&m, &p),
                None => m,
            })
        }
    }
    let k = Placed {
        made: Default::default(),
    };
    let bodies: Vec<(FeatureId, ShapeHandle)> = (1..=400)
        .chain([999])
        .map(|i| (FeatureId(i), ShapeHandle(i)))
        .collect();
    let out = meshes(&k, &bodies);
    assert_eq!(out.len(), 401);
    assert_eq!(
        k.made.load(std::sync::atomic::Ordering::Relaxed),
        1,
        "元の形だけ網にする"
    );
    let (_, third) = &out[2];
    assert_eq!(third.positions[1], [70.0, 0.0, 0.0]);
    assert_eq!(third.edges[0][0], [60.0, 0.0, 0.0]);
    assert_eq!(third.face_names, ["上"]);
    let (_, mirrored) = &out[400];
    assert_eq!(mirrored.positions[1], [-10.0, 0.0, 0.0]);
    assert_eq!(mirrored.triangles, [[0, 2, 1]], "鏡に映したら向きを裏返す");
    assert_eq!(mirrored.normals[0], [0.0, 0.0, 1.0]);
}
