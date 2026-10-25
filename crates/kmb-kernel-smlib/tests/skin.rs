//! スイープとロフト(M7)の試験。体積は、断面が経路に垂直なスイープなら断面の面積 × 中心の線の長さ
//! (直線の角を二等分の平面で継いでも、円弧で曲げても同じ)になることで確かめる。
use kmb_core::kernel::*;
use kmb_kernel_smlib::*;
use std::f64::consts::{PI, TAU};

fn edge(name: &str, segment: Segment) -> ProfileEdge {
    ProfileEdge {
        name: name.into(),
        segment,
        reversed: false,
    }
}

fn line(name: &str, a: [f64; 2], b: [f64; 2]) -> ProfileEdge {
    edge(name, Segment::Line { a, b })
}

fn polygon(pts: &[[f64; 2]], base: u32) -> ProfileLoop {
    ProfileLoop {
        edges: (0..pts.len())
            .map(|i| {
                line(
                    &format!("e{}", base + i as u32),
                    pts[i],
                    pts[(i + 1) % pts.len()],
                )
            })
            .collect(),
    }
}

fn square(c: [f64; 2], half: f64, angle: f64, base: u32) -> ProfileLoop {
    let pts: Vec<[f64; 2]> = (0..4)
        .map(|i| {
            let a = angle + PI / 4.0 + i as f64 * PI / 2.0;
            let r = half * 2f64.sqrt();
            [c[0] + r * a.cos(), c[1] + r * a.sin()]
        })
        .collect();
    polygon(&pts, base)
}

fn circle(c: [f64; 2], r: f64, name: &str) -> ProfileLoop {
    ProfileLoop {
        edges: vec![edge(
            name,
            Segment::Circle {
                center: c,
                radius: r,
            },
        )],
    }
}

fn at_z(z: f64, l: ProfileLoop) -> Profile {
    Profile {
        plane: Plane::XY.offset(z),
        regions: vec![Region {
            outer: l,
            holes: vec![],
        }],
    }
}

fn solid(k: &SmlibKernel, h: ShapeHandle) -> f64 {
    let (manifold, solids, voids) = k.check(h).unwrap();
    assert!(
        manifold && solids == 1 && voids == 0,
        "閉じた 1 つの立体: {manifold} {solids} {voids}"
    );
    k.volume(h).unwrap()
}

/// 網の点の外形(外形の箱は制御点で測るので、曲面では少し大きく出る)。
fn extent(k: &SmlibKernel, h: ShapeHandle) -> ([f64; 3], [f64; 3]) {
    let m = k.mesh(h, 0.01).unwrap();
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in &m.positions {
        for i in 0..3 {
            lo[i] = lo[i].min(p[i] as f64);
            hi[i] = hi[i].max(p[i] as f64);
        }
    }
    (lo, hi)
}

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs()
}

/// XY 平面の円(半径 2)を、XZ 平面(ZX)に描いた経路に沿って動かす。
fn pipe(path: Vec<ProfileEdge>) -> (SmlibKernel, Result<ShapeHandle, KernelError>) {
    let k = SmlibKernel::new();
    let profile = at_z(0.0, circle([0.0, 0.0], 2.0, "e1"));
    // ZX 平面の座標は (z, x)
    let r = k.sweep(
        &profile,
        &PlanarPath {
            plane: Plane::ZX,
            edges: path,
        },
        0.0,
        1.0,
        "f1",
    );
    (k, r)
}

#[test]
fn a_straight_sweep_is_a_prism_and_names_its_faces() {
    let k = SmlibKernel::new();
    let profile = at_z(0.0, square([0.0, 0.0], 5.0, 0.0, 1));
    let h = k
        .sweep(
            &profile,
            &PlanarPath {
                plane: Plane::ZX,
                edges: vec![line("e9", [0.0, 0.0], [30.0, 0.0])],
            },
            0.0,
            1.0,
            "f2",
        )
        .unwrap();
    assert!(close(solid(&k, h), 100.0 * 30.0, 1e-6));
    let mut names: Vec<String> = k.faces(h).unwrap().into_iter().map(|f| f.name).collect();
    names.sort();
    assert_eq!(
        names,
        [
            "f2:end",
            "f2:side:e1:e9",
            "f2:side:e2:e9",
            "f2:side:e3:e9",
            "f2:side:e4:e9",
            "f2:start"
        ]
    );
    let top = k
        .faces(h)
        .unwrap()
        .into_iter()
        .find(|f| f.name == "f2:end")
        .unwrap();
    let p = top.planar.unwrap();
    assert!((p.normal[2] - 1.0).abs() < 1e-9 && (p.point[2] - 30.0).abs() < 1e-9);
}

#[test]
fn a_mitered_corner_and_a_bent_pipe_keep_the_area_times_length() {
    let area = PI * 4.0;
    // 直線どうしの角(L の字)
    let (k, h) = pipe(vec![
        line("e5", [0.0, 0.0], [20.0, 0.0]),
        line("e6", [20.0, 0.0], [20.0, 15.0]),
    ]);
    let v = solid(&k, h.unwrap());
    assert!(close(v, area * 35.0, 1e-4), "{v}");
    // 直線 → 接する円弧(半径 10、90 度)→ 直線
    let (k, h) = pipe(vec![
        line("e5", [0.0, 0.0], [20.0, 0.0]),
        edge(
            "e6",
            Segment::Arc {
                center: [20.0, 10.0],
                radius: 10.0,
                start: -PI / 2.0,
                end: 0.0,
            },
        ),
        line("e7", [30.0, 10.0], [30.0, 25.0]),
    ]);
    let v = solid(&k, h.unwrap());
    assert!(
        close(v, area * (20.0 + 10.0 * PI / 2.0 + 15.0), 1e-4),
        "{v}"
    );
}

#[test]
fn a_sweep_starting_at_the_far_end_is_turned_around() {
    let (k, h) = pipe(vec![line("e5", [20.0, 0.0], [0.0, 0.0])]);
    let v = solid(&k, h.unwrap());
    assert!(close(v, PI * 4.0 * 20.0, 1e-6));
}

#[test]
fn a_bend_tighter_than_the_profile_is_refused() {
    let (_, h) = pipe(vec![
        line("e5", [0.0, 0.0], [20.0, 0.0]),
        edge(
            "e6",
            Segment::Arc {
                center: [20.0, 1.0],
                radius: 1.0,
                start: -PI / 2.0,
                end: 0.0,
            },
        ),
    ]);
    let e = h.unwrap_err().to_string();
    assert!(e.contains("自分と重なります"), "{e}");
}

/// 輪郭 `profile` を ZX 平面の経路に沿って、ねじれ `twist`(度)と終わりの大きさ `scale` で動かす。
fn shaped(
    profile: Profile,
    path: Vec<ProfileEdge>,
    twist: f64,
    scale: f64,
) -> (SmlibKernel, Result<ShapeHandle, KernelError>) {
    let k = SmlibKernel::new();
    let r = k.sweep(
        &profile,
        &PlanarPath {
            plane: Plane::ZX,
            edges: path,
        },
        twist.to_radians(),
        scale,
        "f3",
    );
    (k, r)
}

#[test]
fn an_end_scale_makes_a_frustum_and_a_twist_keeps_the_volume() {
    let straight = || vec![line("e9", [0.0, 0.0], [30.0, 0.0])];
    // 10 mm 角を半分の大きさへ: 角錐台 h/3·(A1 + A2 + √(A1·A2))
    let (k, h) = shaped(
        at_z(0.0, square([0.0, 0.0], 5.0, 0.0, 1)),
        straight(),
        0.0,
        0.5,
    );
    let h = h.unwrap();
    let v = solid(&k, h);
    assert!(close(v, 10.0 * (100.0 + 25.0 + 50.0), 1e-6), "{v}");
    let (lo, hi) = extent(&k, h);
    assert!((hi[0] - 5.0).abs() < 1e-6 && (lo[2]).abs() < 1e-6 && (hi[2] - 30.0).abs() < 1e-6);
    let mut names: Vec<String> = k.faces(h).unwrap().into_iter().map(|f| f.name).collect();
    names.sort();
    assert_eq!(
        names[..2],
        ["f3:end".to_string(), "f3:side:e1:e9".to_string()]
    );
    // 90° ねじっても、どの高さの断面も同じ正方形なので体積は変わらない
    let (k, h) = shaped(
        at_z(0.0, square([0.0, 0.0], 5.0, 0.0, 1)),
        straight(),
        90.0,
        1.0,
    );
    let v = solid(&k, h.unwrap());
    assert!(close(v, 100.0 * 30.0, 1e-4), "{v}");
}

#[test]
fn a_circle_off_the_path_twisted_twice_is_a_coil() {
    // 経路から 10 mm 離した半径 2 の円を 720° ねじる: 2 巻きのコイル(断面は円のまま)
    let (k, h) = shaped(
        at_z(0.0, circle([10.0, 0.0], 2.0, "e1")),
        vec![line("e9", [0.0, 0.0], [30.0, 0.0])],
        720.0,
        1.0,
    );
    let h = h.unwrap();
    let v = solid(&k, h);
    assert!(close(v, PI * 4.0 * 30.0, 1e-3), "{v}");
    // 巻いているので、外形は経路のまわりの両側に広がる
    let (lo, hi) = extent(&k, h);
    for i in 0..2 {
        assert!(lo[i] < -11.5 && hi[i] > 11.5, "{lo:?} {hi:?}");
    }
}

#[test]
fn a_twisted_pipe_through_a_corner_and_a_bend_keeps_the_area_times_length() {
    // 経路に中心を置いた円はねじっても同じ形。角と円弧を通っても面積 × 長さ
    let area = PI * 4.0;
    let (k, h) = shaped(
        at_z(0.0, circle([0.0, 0.0], 2.0, "e1")),
        vec![
            line("e5", [0.0, 0.0], [20.0, 0.0]),
            line("e6", [20.0, 0.0], [20.0, 15.0]),
        ],
        200.0,
        1.0,
    );
    let v = solid(&k, h.unwrap());
    assert!(close(v, area * 35.0, 1e-3), "{v}");
    let (k, h) = shaped(
        at_z(0.0, circle([0.0, 0.0], 2.0, "e1")),
        vec![
            line("e5", [0.0, 0.0], [20.0, 0.0]),
            edge(
                "e6",
                Segment::Arc {
                    center: [20.0, 10.0],
                    radius: 10.0,
                    start: -PI / 2.0,
                    end: 0.0,
                },
            ),
        ],
        90.0,
        1.0,
    );
    let v = solid(&k, h.unwrap());
    assert!(close(v, area * (20.0 + 10.0 * PI / 2.0), 1e-3), "{v}");
}

#[test]
fn a_twist_on_a_closed_path_and_a_zero_scale_are_refused() {
    let ring = vec![
        line("e1", [0.0, 0.0], [20.0, 0.0]),
        line("e2", [20.0, 0.0], [20.0, 20.0]),
        line("e3", [20.0, 20.0], [0.0, 0.0]),
    ];
    let (_, h) = shaped(at_z(0.0, circle([0.0, 0.0], 1.0, "e9")), ring, 90.0, 1.0);
    let e = h.unwrap_err().to_string();
    assert!(e.contains("閉じた経路"), "{e}");
    let (_, h) = shaped(
        at_z(0.0, circle([0.0, 0.0], 1.0, "e9")),
        vec![line("e5", [0.0, 0.0], [20.0, 0.0])],
        0.0,
        0.0,
    );
    assert!(h.is_err());
}

#[test]
fn a_square_to_circle_loft_is_one_closed_solid() {
    let k = SmlibKernel::new();
    let h = k
        .loft(
            &[
                at_z(0.0, square([0.0, 0.0], 10.0, 0.0, 1)),
                at_z(30.0, circle([0.0, 0.0], 10.0, "e9")),
            ],
            &[],
            "f3",
        )
        .unwrap();
    let v = solid(&k, h);
    // 円柱(π·100·30)と角柱(400·30)の間
    assert!(v > PI * 100.0 * 30.0 && v < 400.0 * 30.0, "{v}");
    let names: Vec<String> = k.faces(h).unwrap().into_iter().map(|f| f.name).collect();
    assert!(names.contains(&"f3:start".to_string()) && names.contains(&"f3:end".to_string()));
    assert!(
        names.iter().filter(|n| n.starts_with("f3:side:e1")).count() >= 1,
        "{names:?}"
    );
}

/// 一辺 20 の正方形に、円の穴 `holes`(中心・半径)をあけた高さ `z` の断面。
fn holed(z: f64, holes: &[([f64; 2], f64)], base: u32) -> Profile {
    let mut p = at_z(z, square([0.0, 0.0], 10.0, 0.0, base));
    p.regions[0].holes = holes
        .iter()
        .enumerate()
        .map(|(i, (c, r))| {
            let mut l = circle(*c, *r, &format!("e{}", base + 10 + i as u32));
            l.edges[0].reversed = true;
            l
        })
        .collect();
    p
}

#[test]
fn sections_with_holes_loft_into_a_hollow_solid() {
    let k = SmlibKernel::new();
    // 上の断面は穴を逆の順に描いても、近い穴どうしをつなぐ(半径 2 と 3 のまっすぐな穴)
    let h = k
        .loft(
            &[
                holed(0.0, &[([-5.0, 0.0], 2.0), ([5.0, 0.0], 3.0)], 1),
                holed(30.0, &[([5.0, 0.0], 3.0), ([-5.0, 0.0], 2.0)], 21),
            ],
            &[],
            "f4",
        )
        .unwrap();
    let v = solid(&k, h);
    assert!(
        close(v, 400.0 * 30.0 - PI * (4.0 + 9.0) * 30.0, 1e-4),
        "{v}"
    );
    let names: Vec<String> = k.faces(h).unwrap().into_iter().map(|f| f.name).collect();
    for want in ["f4:start", "f4:end", "f4:side:e11", "f4:side:e12"] {
        assert!(
            names.iter().any(|n| n.starts_with(want)),
            "{want} {names:?}"
        );
    }
    // 穴が半径 4 から 2 へ細くなる: 円錐台の分だけ引く
    let h = k
        .loft(
            &[
                holed(0.0, &[([0.0, 0.0], 4.0)], 1),
                holed(30.0, &[([0.0, 0.0], 2.0)], 21),
            ],
            &[],
            "f5",
        )
        .unwrap();
    let v = solid(&k, h);
    assert!(
        close(v, 400.0 * 30.0 - PI * 10.0 * (16.0 + 4.0 + 8.0), 1e-4),
        "{v}"
    );
}

#[test]
fn sections_with_different_numbers_of_holes_are_refused() {
    let k = SmlibKernel::new();
    for sections in [
        [
            holed(0.0, &[([0.0, 0.0], 4.0)], 1),
            holed(30.0, &[([-5.0, 0.0], 2.0), ([5.0, 0.0], 2.0)], 21),
        ],
        [holed(0.0, &[], 1), holed(30.0, &[([0.0, 0.0], 2.0)], 21)],
    ] {
        let e = k.loft(&sections, &[], "f6").unwrap_err().to_string();
        assert!(e.contains("同じ数の穴"), "{e}");
    }
}

#[test]
fn a_loft_through_a_square_twisted_past_ninety_degrees_matches_the_nearest_corner() {
    // k16: 100 度回した四角は、10 度だけねじれた形になる(90 度で止まったり、ねじれで潰れたりしない)
    let k = SmlibKernel::new();
    let h = k
        .loft(
            &[
                at_z(0.0, square([0.0, 0.0], 10.0, 0.0, 1)),
                at_z(20.0, square([0.0, 0.0], 10.0, 100f64.to_radians(), 5)),
            ],
            &[],
            "f3",
        )
        .unwrap();
    let v = solid(&k, h);
    assert!(v > 0.97 * 400.0 * 20.0 && v <= 400.0 * 20.0 + 1e-6, "{v}");
}

#[test]
fn a_loft_from_a_non_star_shape_to_a_circle_through_three_sections() {
    // k16: 中心から見えない所がある L の字の断面と円を、間の断面を通して張る
    let k = SmlibKernel::new();
    let l = polygon(
        &[
            [-10.0, -10.0],
            [10.0, -10.0],
            [10.0, -4.0],
            [-4.0, -4.0],
            [-4.0, 10.0],
            [-10.0, 10.0],
        ],
        1,
    );
    let h = k
        .loft(
            &[
                at_z(0.0, l),
                at_z(15.0, circle([0.0, 0.0], 8.0, "e9")),
                at_z(30.0, square([0.0, 0.0], 5.0, 0.3, 11)),
            ],
            &[],
            "f4",
        )
        .unwrap();
    let v = solid(&k, h);
    assert!(v > 0.0, "{v}");
}

#[test]
fn consecutive_sections_on_one_plane_are_refused_with_a_reason() {
    // k9
    let k = SmlibKernel::new();
    let e = k
        .loft(
            &[
                at_z(0.0, square([0.0, 0.0], 10.0, 0.0, 1)),
                at_z(0.0, circle([0.0, 0.0], 4.0, "e9")),
            ],
            &[],
            "f3",
        )
        .unwrap_err()
        .to_string();
    assert!(e.contains("断面 1 と断面 2 が同じ平面にあります"), "{e}");
}

#[test]
fn a_rail_bulges_the_loft_out() {
    // f11: 同じ四角の間を、膨らんだレール(XZ 平面の円弧)に沿わせると、まっすぐな柱より太くなる
    let k = SmlibKernel::new();
    let sections = [
        at_z(0.0, square([0.0, 0.0], 10.0, 0.0, 1)),
        at_z(30.0, square([0.0, 0.0], 10.0, 0.0, 5)),
    ];
    let straight = solid(&k, k.loft(&sections, &[], "f3").unwrap());
    // ZX 平面の (z, x) で、(0, 10) から (30, 10) へ、真ん中で x = 14 まで膨らむ円弧。四角の x = 10 の
    // 辺の中点(y = 0)を通る
    let r = (15f64 * 15.0 + 4.0 * 4.0) / 8.0;
    let center = [15.0, 14.0 - r];
    let a0 = (10.0 - center[1]).atan2(15.0);
    let rail = PlanarPath {
        plane: Plane::ZX,
        edges: vec![edge(
            "e20",
            Segment::Arc {
                center,
                radius: r,
                start: a0,
                end: PI - a0,
            },
        )],
    };
    let bulged = solid(
        &k,
        k.loft(&sections, std::slice::from_ref(&rail), "f3")
            .unwrap(),
    );
    assert!(bulged > straight * 1.05, "{bulged} {straight}");
    // レールの側だけが膨らむ(反対側の x = -10 は動かない)
    let h = k
        .loft(&sections, std::slice::from_ref(&rail), "f3")
        .unwrap();
    let (min, max) = extent(&k, h);
    assert!(
        (max[0] - 14.0).abs() < 0.05 && (min[0] + 10.0).abs() < 1e-3,
        "{min:?} {max:?}"
    );
    // レール 2 本(両側): 断面は 2 本の間で大きさを変える
    let mirror = PlanarPath {
        plane: Plane::ZX,
        edges: vec![edge(
            "e21",
            Segment::Arc {
                center: [15.0, -center[1]],
                radius: r,
                start: PI + a0,
                end: TAU - a0,
            },
        )],
    };
    let h = k.loft(&sections, &[rail, mirror], "f3").unwrap();
    solid(&k, h);
    let (min, max) = extent(&k, h);
    assert!(
        (max[0] - 14.0).abs() < 0.05 && (min[0] + 14.0).abs() < 0.05,
        "{min:?} {max:?}"
    );
    assert!((max[1] - 14.0).abs() < 0.1, "{max:?}");
}

#[test]
fn a_closed_frame_and_a_tube_with_a_hole() {
    // 閉じた四角の経路(角は 4 つとも二等分の平面で継ぐ)。端の面は無い
    let (k, h) = pipe(vec![
        line("e5", [0.0, 0.0], [20.0, 0.0]),
        line("e6", [20.0, 0.0], [20.0, 20.0]),
        line("e7", [20.0, 20.0], [0.0, 20.0]),
        line("e8", [0.0, 20.0], [0.0, 0.0]),
    ]);
    let h = h.unwrap();
    let v = solid(&k, h);
    assert!(close(v, PI * 4.0 * 80.0, 1e-4), "{v}");
    assert!(
        k.faces(h)
            .unwrap()
            .iter()
            .all(|f| !f.name.ends_with(":start"))
    );
    // 穴のある輪郭は管になる
    let k = SmlibKernel::new();
    let mut hole = circle([0.0, 0.0], 2.0, "e9");
    hole.edges[0].reversed = true;
    let profile = Profile {
        plane: Plane::XY,
        regions: vec![Region {
            outer: square([0.0, 0.0], 5.0, 0.0, 1),
            holes: vec![hole],
        }],
    };
    let h = k
        .sweep(
            &profile,
            &PlanarPath {
                plane: Plane::ZX,
                edges: vec![line("e10", [0.0, 0.0], [30.0, 0.0])],
            },
            0.0,
            1.0,
            "f2",
        )
        .unwrap();
    let v = solid(&k, h);
    assert!(close(v, (100.0 - PI * 4.0) * 30.0, 1e-5), "{v}");
}
