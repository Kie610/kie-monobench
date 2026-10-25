//! SMLib の辺の丸め(`Kernel::blend_edges`。半径の種類は M16)と、曲がった辺の一覧
//! (`Kernel::curve_edges`)の試験。体積は理論の値と比べる。
use kmb_core::kernel::*;
use kmb_kernel_smlib::*;
use std::f64::consts::PI;

fn line(name: &str, a: [f64; 2], b: [f64; 2]) -> ProfileEdge {
    ProfileEdge {
        name: name.into(),
        segment: Segment::Line { a, b },
        reversed: false,
    }
}

fn rect(x0: f64, y0: f64, w: f64, h: f64) -> Profile {
    let p = [[x0, y0], [x0 + w, y0], [x0 + w, y0 + h], [x0, y0 + h]];
    Profile {
        plane: Plane::XY,
        regions: vec![Region {
            outer: ProfileLoop {
                edges: (0..4)
                    .map(|i| line(&format!("e{}", i + 1), p[i], p[(i + 1) % 4]))
                    .collect(),
            },
            holes: vec![],
        }],
    }
}

/// 角を半径 `r` で丸めた長方形(辺の名前は下の線 e1 から反時計回りに e1〜e8、弧は偶数)
fn rounded_rect(x0: f64, y0: f64, w: f64, h: f64, r: f64) -> Profile {
    let (x1, y1) = (x0 + w, y0 + h);
    let arc = |name: &str, c: [f64; 2], start: f64| ProfileEdge {
        name: name.into(),
        segment: Segment::Arc {
            center: c,
            radius: r,
            start,
            end: start + PI / 2.0,
        },
        reversed: false,
    };
    Profile {
        plane: Plane::XY,
        regions: vec![Region {
            outer: ProfileLoop {
                edges: vec![
                    line("e1", [x0 + r, y0], [x1 - r, y0]),
                    arc("e2", [x1 - r, y0 + r], -PI / 2.0),
                    line("e3", [x1, y0 + r], [x1, y1 - r]),
                    arc("e4", [x1 - r, y1 - r], 0.0),
                    line("e5", [x1 - r, y1], [x0 + r, y1]),
                    arc("e6", [x0 + r, y1 - r], PI / 2.0),
                    line("e7", [x0, y1 - r], [x0, y0 + r]),
                    arc("e8", [x0 + r, y0 + r], PI),
                ],
            },
            holes: vec![],
        }],
    }
}

fn blend(a: &str, b: &str, point: P3, size: f64) -> EdgeBlend {
    EdgeBlend::new([a.into(), b.into()], point, size, false)
}

/// 60×40×10 の箱(面は f1:side:e1〜e4、上 f1:end、下 f1:start)
fn plate(k: &SmlibKernel) -> ShapeHandle {
    k.extrude(&rect(0.0, 0.0, 60.0, 40.0), 0.0, 10.0, "f1")
        .unwrap()
}

const PLATE: f64 = 60.0 * 40.0 * 10.0;

fn vertical_edges(r: f64) -> Vec<EdgeBlend> {
    let side = |i: u32| format!("f1:side:e{i}");
    [
        ((4, 1), [0.0, 0.0]),
        ((1, 2), [60.0, 0.0]),
        ((2, 3), [60.0, 40.0]),
        ((3, 4), [0.0, 40.0]),
    ]
    .iter()
    .map(|((a, b), [x, y])| blend(&side(*a), &side(*b), [*x, *y, 5.0], r))
    .collect()
}

/// 箱を 1 回だけ作り、縦の 4 辺を丸める・面取りする。
/// - 丸めの体積が理論と合い、丸めの面は両側の面の名前から名付け、元の面は名前を保つ。
///   上の面と丸めの面の境は円の辺として拾える(次の丸めに使える)
/// - 面取り: 断面が直線。直角の辺では、面に接する円の半径が面取りの距離と同じ
#[test]
fn rounding_and_chamfering_the_four_vertical_edges_of_a_box() {
    let k = SmlibKernel::new();
    let h = plate(&k);
    let r = 2.0;
    let out = k.blend_edges(h, &vertical_edges(r), "f2").unwrap();
    let v = k.mass(out).unwrap().volume;
    let want = PLATE - 4.0 * r * r * (1.0 - PI / 4.0) * 10.0;
    assert!((v - want).abs() < 1e-3, "{v} 理論 {want}");
    // 丸めの面は、両側の面の名前から名付ける。元の面は名前を保つ
    let names: Vec<String> = k.faces(out).unwrap().into_iter().map(|f| f.name).collect();
    assert!(
        names.contains(&"f2:f1:side:e1|f1:side:e2".to_string()),
        "{names:?}"
    );
    for n in ["f1:end", "f1:start", "f1:side:e1"] {
        assert!(names.contains(&n.to_string()), "{n}: {names:?}");
    }
    assert_eq!(names.len(), 10, "{names:?}");
    // 上の面と丸めの面の境は、平面と垂直な円筒の円の辺として拾える(次の丸めに使える)
    let arcs = k.arc_edges(out).unwrap();
    assert_eq!(arcs.len(), 8, "上と下の 4 つずつ: {arcs:?}");

    // 面取り
    let d = 1.0;
    let edges: Vec<EdgeBlend> = vertical_edges(d)
        .into_iter()
        .map(|b| EdgeBlend { flat: true, ..b })
        .collect();
    let out = k.blend_edges(h, &edges, "f2").unwrap();
    let v = k.mass(out).unwrap().volume;
    let want = PLATE - 4.0 * d * d / 2.0 * 10.0;
    assert!((v - want).abs() < 1e-3, "{v} 理論 {want}");
}

/// 箱を 1 回だけ作り、角で出会う辺を丸める。角は球(角の立方体から球の 8 分の 1 を除いた分)。
/// - 1 つの角で出会う 3 本: 滑らかな角ができ、角の面は名前で分かる
/// - 12 本全部: 8 つの角がどれも球の角になる
#[test]
fn edges_meeting_at_corners_get_smooth_sphere_corners() {
    let k = SmlibKernel::new();
    let h = plate(&k);
    let r = 2.0;
    let edges = vec![
        blend("f1:side:e1", "f1:side:e2", [60.0, 0.0, 5.0], r),
        blend("f1:end", "f1:side:e1", [30.0, 0.0, 10.0], r),
        blend("f1:end", "f1:side:e2", [60.0, 20.0, 10.0], r),
    ];
    let out = k.blend_edges(h, &edges, "f2").unwrap();
    let v = k.mass(out).unwrap().volume;
    // 球の角: 3 本の丸め(角から r まで除く)と、角の立方体から球の 8 分の 1 を除いた分
    let lengths = (60.0 - r) + (40.0 - r) + (10.0 - r);
    let want = PLATE - r * r * (1.0 - PI / 4.0) * lengths - r.powi(3) * (1.0 - PI / 6.0);
    assert!((v - want).abs() < 0.05, "{v} 理論 {want}");
    let names: Vec<String> = k.faces(out).unwrap().into_iter().map(|f| f.name).collect();
    assert!(
        names.iter().any(|n| n.starts_with("f2:corner")),
        "{names:?}"
    );

    // 12 本全部
    let r = 1.0;
    let mut edges = vertical_edges(r);
    for (face, mid) in [
        ("f1:side:e1", [30.0, 0.0]),
        ("f1:side:e2", [60.0, 20.0]),
        ("f1:side:e3", [30.0, 40.0]),
        ("f1:side:e4", [0.0, 20.0]),
    ] {
        edges.push(blend("f1:end", face, [mid[0], mid[1], 10.0], r));
        edges.push(blend("f1:start", face, [mid[0], mid[1], 0.0], r));
    }
    let out = k.blend_edges(h, &edges, "f2").unwrap();
    let v = k.mass(out).unwrap().volume;
    let lengths = 4.0 * (60.0 - 2.0 * r) + 4.0 * (40.0 - 2.0 * r) + 4.0 * (10.0 - 2.0 * r);
    let want = PLATE - r * r * (1.0 - PI / 4.0) * lengths - 8.0 * r.powi(3) * (1.0 - PI / 6.0);
    assert!((v - want).abs() < 0.05, "{v} 理論 {want}");
}

/// 円柱の上の縁(平面と円筒の円の辺)を丸める: 丸めの断面の重心が軸のまわりを回る(パップスの定理)。
#[test]
fn rounding_the_top_rim_of_a_cylinder() {
    let k = SmlibKernel::new();
    let p = Profile {
        plane: Plane::XY,
        regions: vec![Region {
            outer: ProfileLoop {
                edges: vec![ProfileEdge {
                    name: "e1".into(),
                    segment: Segment::Circle {
                        center: [0.0, 0.0],
                        radius: 10.0,
                    },
                    reversed: false,
                }],
            },
            holes: vec![],
        }],
    };
    let h = k.extrude(&p, 0.0, 10.0, "f1").unwrap();
    let r = 2.0;
    let out = k
        .blend_edges(
            h,
            &[blend("f1:end", "f1:side:e1", [10.0, 0.0, 10.0], r)],
            "f2",
        )
        .unwrap();
    let v = k.mass(out).unwrap().volume;
    let area = r * r * (1.0 - PI / 4.0);
    let d = r * (10.0 - 3.0 * PI) / (12.0 - 3.0 * PI);
    let want = PI * 100.0 * 10.0 - area * 2.0 * PI * (10.0 - d);
    assert!((v - want).abs() < 0.01, "{v} 理論 {want}");
}

/// 丸めの後にできる「平面と円筒のまっすぐな境」や、曲面どうしの境は、曲がった辺の一覧に入る。
#[test]
fn curve_edges_list_the_edges_the_other_lists_leave_out() {
    let k = SmlibKernel::new();
    let h = plate(&k);
    let before = k.curve_edges(h).unwrap();
    assert!(
        before.is_empty(),
        "箱の辺は、まっすぐな辺の一覧にある: {before:?}"
    );
    // 上の手前の辺を丸めると、丸めの面と上の面・手前の面の境(まっすぐな 2 本)ができる
    let out = k
        .blend_edges(
            h,
            &[blend("f1:end", "f1:side:e1", [30.0, 0.0, 10.0], 2.0)],
            "f2",
        )
        .unwrap();
    let curves = k.curve_edges(out).unwrap();
    let fillet = "f2:f1:end|f1:side:e1";
    let with_fillet: Vec<&EdgeInfo> = curves
        .iter()
        .filter(|e| e.faces.iter().any(|f| f == fillet))
        .collect();
    // 長い 2 本(両端の横の面との境の四分円 2 本は、円の辺の一覧にある)
    assert_eq!(with_fillet.len(), 2, "{curves:?}");
    let arcs = k.arc_edges(out).unwrap();
    assert_eq!(
        arcs.iter()
            .filter(|e| e.faces.iter().any(|f| f == fillet))
            .count(),
        2,
        "{arcs:?}"
    );
    for e in &with_fillet {
        assert!(e.curve.len() >= 2);
        assert_eq!(e.curve[0], e.a);
        assert_eq!(*e.curve.last().unwrap(), e.b);
    }
    let long = with_fillet
        .iter()
        .filter(|e| {
            let d = sub(e.b, e.a);
            dot(d, d).sqrt() > 50.0
        })
        .count();
    assert_eq!(long, 2);
}

fn pocket(k: &SmlibKernel) -> ShapeHandle {
    let h = plate(k);
    let tool = k
        .extrude(&rounded_rect(15.0, 10.0, 30.0, 20.0, 4.0), 6.0, 11.0, "f2")
        .unwrap();
    k.boolean(BooleanOp::Subtract, h, tool).unwrap()
}

/// ポケットの底の縁(へこんだ辺)を丸めると、中身が足される。
#[test]
fn rounding_the_floor_rim_of_a_pocket_adds_material() {
    let k = SmlibKernel::new();
    let h = pocket(&k);
    let before = k.mass(h).unwrap().volume;
    let floor: Vec<EdgeInfo> = k
        .edges(h)
        .unwrap()
        .into_iter()
        .chain(k.arc_edges(h).unwrap())
        .filter(|e| e.faces.contains(&"f2:start".to_string()))
        .collect();
    assert_eq!(floor.len(), 8, "{floor:?}");
    assert!(floor.iter().all(|e| !e.convex), "底の縁はへこんだ辺");
    let r = 1.0;
    let edges: Vec<EdgeBlend> = floor
        .iter()
        .map(|e| {
            let line = e.polyline(8);
            blend(&e.faces[0], &e.faces[1], line[line.len() / 2], r)
        })
        .collect();
    let out = k.blend_edges(h, &edges, "f3").unwrap();
    let added = k.mass(out).unwrap().volume - before;
    let area = r * r * (1.0 - PI / 4.0);
    let d = r * (10.0 - 3.0 * PI) / (12.0 - 3.0 * PI);
    let want = area * (2.0 * 22.0 + 2.0 * 12.0) + area * 2.0 * PI * (4.0 - d);
    assert!((added - want).abs() < 0.01, "{added} 理論 {want}");
}

/// 面に収まらない半径は、形を壊さずに誤りを返す(元の形はそのまま使える)。
#[test]
fn a_radius_too_large_fails_and_keeps_the_shape() {
    let k = SmlibKernel::new();
    let h = plate(&k);
    let err = k.blend_edges(
        h,
        &[blend("f1:end", "f1:side:e1", [30.0, 0.0, 10.0], 12.0)],
        "f2",
    );
    assert!(err.is_err(), "{err:?}");
    assert!((k.mass(h).unwrap().volume - PLATE).abs() < 1e-6);
}

/// 円錐の底の縁(平面と円錐の境の円)は、平面と円筒の辺の一覧には無く、曲がった辺の一覧に入る。
/// カーネルの丸めなら、この辺も丸められる。
#[test]
fn the_rim_of_a_cone_is_a_curve_edge_and_can_be_rounded() {
    let k = SmlibKernel::new();
    // YZ 平面の直角三角形(y 0〜10、z 0〜10)を z 軸まわりに一周: 底の半径 10、高さ 10 の円錐
    let p = Profile {
        plane: Plane::YZ,
        regions: vec![Region {
            outer: ProfileLoop {
                edges: vec![
                    line("e1", [0.0, 0.0], [10.0, 0.0]),
                    line("e2", [10.0, 0.0], [0.0, 10.0]),
                    line("e3", [0.0, 10.0], [0.0, 0.0]),
                ],
            },
            holes: vec![],
        }],
    };
    let h = k
        .revolve(&p, [0.0; 3], [0.0, 0.0, 1.0], 2.0 * PI, "f1")
        .unwrap();
    let before = k.mass(h).unwrap().volume;
    assert!((before - PI * 100.0 * 10.0 / 3.0).abs() < 0.01, "{before}");
    assert!(k.arc_edges(h).unwrap().is_empty());
    let curves = k.curve_edges(h).unwrap();
    assert_eq!(curves.len(), 1, "底の縁: {curves:?}");
    let rim = &curves[0];
    assert!(rim.convex, "{rim:?}");
    let line = rim.polyline(8);
    let r = 1.0;
    let out = k
        .blend_edges(
            h,
            &[blend(&rim.faces[0], &rim.faces[1], line[line.len() / 2], r)],
            "f2",
        )
        .unwrap();
    // 45° の角を半径 1 で丸めて削る断面は r²(cot 22.5° − 3π/8) ≈ 1.236。重心は縁から 1 mm ほど内側
    let removed = before - k.mass(out).unwrap().volume;
    let area = r * r * (1.0 / (PI / 8.0).tan() - 3.0 * PI / 8.0);
    assert!(
        removed > area * 2.0 * PI * 8.5 && removed < area * 2.0 * PI * 10.0,
        "{removed}"
    );
}

/// 上の手前の辺(x の向き、長さ 60)。
fn top_front(size: f64) -> EdgeBlend {
    blend("f1:end", "f1:side:e1", [30.0, 0.0, 10.0], size)
}

/// 丸めた形のうち、x が `x0..x1` の所で削れた体積。
fn removed_between(k: &SmlibKernel, out: ShapeHandle, x0: f64, x1: f64) -> f64 {
    let slab = k
        .extrude(&rect(x0, -1.0, x1 - x0, 42.0), -1.0, 11.0, "s")
        .unwrap();
    let kept = k.boolean(BooleanOp::Intersect, out, slab).unwrap();
    (x1 - x0) * 40.0 * 10.0 - k.mass(kept).unwrap().volume
}

/// 箱を 1 回だけ作り、上の手前の辺を半径の決め方を替えて丸める。
/// - 直角の辺では、弦の長さ c の丸めは半径 c / √2 の丸めと同じ形
/// - G2 の断面は円より膨らみが違うが、面に接する所(半径の分だけ辺から離れた所)は同じ
#[test]
fn chord_length_and_curvature_continuous_fillets_keep_the_reach_of_the_radius() {
    // 直角の辺では、弦の長さ c の丸めは半径 c / √2 の丸めと同じ形
    let k = SmlibKernel::new();
    let h = plate(&k);
    let r = 2.0;
    let out = k
        .blend_edges(
            h,
            &[EdgeBlend {
                law: BlendLaw::Chord,
                ..top_front(r * 2f64.sqrt())
            }],
            "f2",
        )
        .unwrap();
    let v = k.mass(out).unwrap().volume;
    let want = PLATE - r * r * (1.0 - PI / 4.0) * 60.0;
    assert!((v - want).abs() < 0.05, "{v} 理論 {want}");

    // G2 の断面は円より膨らみが違うが、面に接する所(半径の分だけ辺から離れた所)は同じ
    let out = k
        .blend_edges(
            h,
            &[EdgeBlend {
                g2: true,
                ..top_front(r)
            }],
            "f2",
        )
        .unwrap();
    let v = k.mass(out).unwrap().volume;
    let circle = PLATE - r * r * (1.0 - PI / 4.0) * 60.0;
    // 削る量は、円の丸め(r²(1−π/4))と、面に接する 2 点を結ぶ面取り(r²/2)の間ほど
    assert!(v < PLATE && v > PLATE - r * r * 0.5 * 60.0, "{v}");
    assert!((v - circle).abs() > 1e-3, "円の断面と同じ形になった: {v}");
    let names: Vec<String> = k.faces(out).unwrap().into_iter().map(|f| f.name).collect();
    assert!(
        names.contains(&"f2:f1:end|f1:side:e1".to_string()),
        "{names:?}"
    );
}

/// 箱を 1 回だけ作り、上の手前の辺を可変の半径で丸める。
/// - 始めから終わりへ半径が直線で変わり、始めの点の側が細い。始めの点を反対の端にすると入れ替わる
/// - 途中の点の半径を太くすると、真ん中が太く両端が細い
#[test]
fn variable_radius_changes_along_the_edge_and_through_a_middle_point() {
    let k = SmlibKernel::new();
    let h = plate(&k);
    let (r0, r1) = (1.0, 3.0);
    let law = |start: P3| BlendLaw::Variable {
        start,
        radii: vec![(0.0, r0), (1.0, r1)],
    };
    let out = k
        .blend_edges(
            h,
            &[EdgeBlend {
                law: law([0.0, 0.0, 10.0]),
                ..top_front(r0)
            }],
            "f2",
        )
        .unwrap();
    // 半径が直線で変わる: 削る断面は r² (1 − π/4) で、長さ 60 で積むと (r0² + r0·r1 + r1²) / 3
    let v = k.mass(out).unwrap().volume;
    let want = PLATE - (1.0 - PI / 4.0) * 60.0 * (r0 * r0 + r0 * r1 + r1 * r1) / 3.0;
    assert!((v - want).abs() < 0.2, "{v} 理論 {want}");
    // 始めの点(x = 0)の側が細い
    let near = removed_between(&k, out, 0.0, 5.0);
    let far = removed_between(&k, out, 55.0, 60.0);
    assert!(near < far / 3.0, "始めの側 {near} 終わりの側 {far}");
    // 始めの点を反対の端にすると、太い側も入れ替わる
    let out = k
        .blend_edges(
            h,
            &[EdgeBlend {
                law: law([60.0, 0.0, 10.0]),
                ..top_front(r0)
            }],
            "f2",
        )
        .unwrap();
    let near = removed_between(&k, out, 0.0, 5.0);
    let far = removed_between(&k, out, 55.0, 60.0);
    assert!(far < near / 3.0, "始めの側 {far} 終わりの側 {near}");

    // 途中の点(真ん中)を太くする
    let out = k
        .blend_edges(
            h,
            &[EdgeBlend {
                law: BlendLaw::Variable {
                    start: [0.0, 0.0, 10.0],
                    radii: vec![(0.0, 1.0), (0.5, 3.0), (1.0, 1.0)],
                },
                ..top_front(1.0)
            }],
            "f2",
        )
        .unwrap();
    // 真ん中が太く、両端が細い
    let start = removed_between(&k, out, 0.0, 5.0);
    let mid = removed_between(&k, out, 27.5, 32.5);
    let end = removed_between(&k, out, 55.0, 60.0);
    let ring = |r: f64| r * r * (1.0 - PI / 4.0) * 5.0;
    assert!((mid - ring(3.0)).abs() < 0.1 * ring(3.0), "真ん中 {mid}");
    assert!(start < mid / 3.0 && end < mid / 3.0, "{start} {mid} {end}");
    assert!(
        (start - end).abs() < 0.1 * mid,
        "両端はそろう: {start} {end}"
    );
}
