//! 形状カーネル(SMLib)にほかで見つかった不具合が、JC の呼び方でも起きるかを当てる試験。
//! 1 件 1 つの形で、正しい結果(体積の理論の値、閉じた立体であること、断るなら誤りを返すこと)を
//! 確かめる。形は JC が呼ぶ口(押し出し・回転・掃引・ロフト・丸め・面取り・シェル・オフセット・
//! ブーリアン・STEP・表示用の網)を通す。起きた物と直し方は
//! `docs/機能/2026-11-27-形状カーネルの既知の不具合を当てる-x5uuq9.md`。
//! 今のカーネルで起きる物は、正しい結果を確かめる関数をそのまま置き、`still_occurs` で
//! 「まだ落ちる」ことを確かめる。カーネルを上げて直ると落ちるので、その時は関数を試験に戻す。
use kmb_core::kernel::*;
use kmb_kernel_smlib::*;
use std::f64::consts::{PI, TAU};

fn line(name: &str, a: [f64; 2], b: [f64; 2]) -> ProfileEdge {
    ProfileEdge {
        curve: None,
        name: name.into(),
        segment: Segment::Line { a, b },
        reversed: false,
    }
}

fn arc(name: &str, center: [f64; 2], radius: f64, start: f64, end: f64) -> ProfileEdge {
    ProfileEdge {
        curve: None,
        name: name.into(),
        segment: Segment::Arc {
            center,
            radius,
            start,
            end,
        },
        reversed: false,
    }
}

fn circle(name: &str, center: [f64; 2], radius: f64) -> ProfileLoop {
    ProfileLoop {
        edges: vec![ProfileEdge {
            curve: None,
            name: name.into(),
            segment: Segment::Circle { center, radius },
            reversed: false,
        }],
    }
}

/// 点を順につないだ閉じた輪。辺の名前は e1 から。
fn polygon(pts: &[[f64; 2]]) -> ProfileLoop {
    let n = pts.len();
    ProfileLoop {
        edges: (0..n)
            .map(|i| line(&format!("e{}", i + 1), pts[i], pts[(i + 1) % n]))
            .collect(),
    }
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> ProfileLoop {
    polygon(&[[x0, y0], [x1, y0], [x1, y1], [x0, y1]])
}

fn on(plane: Plane, outer: ProfileLoop) -> Profile {
    Profile {
        plane,
        regions: vec![Region {
            outer,
            holes: vec![],
        }],
    }
}

/// 箱。面は `<tag>:side:e1`(y = 下)・`e2`(x = 右)・`e3`(y = 上)・`e4`(x = 左)・`:end`(上)・`:start`(下)。
fn cuboid(k: &SmlibKernel, lo: [f64; 3], hi: [f64; 3], tag: &str) -> ShapeHandle {
    k.extrude(
        &on(Plane::XY.offset(lo[2]), rect(lo[0], lo[1], hi[0], hi[1])),
        0.0,
        hi[2] - lo[2],
        tag,
    )
    .unwrap()
}

/// z 向きの円柱。側面は `<tag>:side:c1`。
fn cylinder(k: &SmlibKernel, c: [f64; 2], r: f64, z0: f64, z1: f64, tag: &str) -> ShapeHandle {
    k.extrude(
        &on(Plane::XY.offset(z0), circle("c1", c, r)),
        0.0,
        z1 - z0,
        tag,
    )
    .unwrap()
}

/// ZX 平面((z, x) の座標)の輪を z 軸のまわりに回す。
fn revolve_z(
    k: &SmlibKernel,
    l: ProfileLoop,
    angle: f64,
    tag: &str,
) -> Result<ShapeHandle, KernelError> {
    k.revolve(&on(Plane::ZX, l), [0.0; 3], [0.0, 0.0, 1.0], angle, tag)
}

fn blend(a: &str, b: &str, point: P3, size: f64) -> EdgeBlend {
    EdgeBlend::new([a.into(), b.into()], point, size, false)
}

fn names(k: &SmlibKernel, h: ShapeHandle) -> Vec<String> {
    k.faces(h).unwrap().into_iter().map(|f| f.name).collect()
}

fn face_starting(k: &SmlibKernel, h: ShapeHandle, head: &str) -> String {
    names(k, h)
        .into_iter()
        .find(|n| n.starts_with(head))
        .unwrap_or_else(|| panic!("{head} の面が無い: {:?}", names(k, h)))
}

/// 閉じた立体(空洞は `voids` 個)で、体積を返す。表示用の網も作れる。
fn solid(k: &SmlibKernel, h: ShapeHandle, voids: i64) -> f64 {
    let (manifold, solids, v) = k.check(h).unwrap();
    assert!(
        manifold && solids == 1 && v == voids,
        "閉じた 1 つの立体(空洞 {voids}): 多様体 {manifold} 立体 {solids} 空洞 {v}"
    );
    let m = k.mesh(h, 0.05).unwrap();
    assert!(!m.triangles.is_empty(), "網が空");
    k.volume(h).unwrap()
}

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs().max(1.0)
}

/// 断るなら誤り、作るなら閉じた立体。誤りを返さずに崩れた形を返すのが不具合。
fn refuse_or_solid(k: &SmlibKernel, r: Result<ShapeHandle, KernelError>) -> Option<f64> {
    match r {
        Err(e) => {
            eprintln!("断った: {e}");
            None
        }
        Ok(h) => Some(solid(k, h, 0)),
    }
}

/// まだ起きる不具合: 正しい結果を確かめる `check` が、今のカーネルでは落ちることを確かめる。
/// 落ちなくなったら直ったので、`still_occurs` を外して `check` を試験に戻す。
fn still_occurs(name: &str, check: fn()) {
    let fixed = std::panic::catch_unwind(check).is_ok();
    assert!(
        !fixed,
        "{name} が直った。still_occurs を外し、正しい結果を確かめる試験に戻す"
    );
}

// ---- 依頼の目的の形(あと少しの形で丸め・オフセットが失敗する恐れ)----

/// 3° 傾いた円柱のボスを板に立て、根元を丸める。
/// 今のカーネルではまだ起きる。
#[test]
fn round_at_the_foot_of_a_slightly_tilted_boss_still_occurs() {
    still_occurs(
        "round_at_the_foot_of_a_slightly_tilted_boss",
        round_at_the_foot_of_a_slightly_tilted_boss,
    );
}

fn round_at_the_foot_of_a_slightly_tilted_boss() {
    let k = SmlibKernel::new();
    let plate = cuboid(&k, [0.0; 3], [60.0, 40.0, 10.0], "f1");
    let t = 3f64.to_radians();
    let tilted = Plane {
        origin: [30.0, 20.0, 5.0],
        x_axis: [1.0, 0.0, 0.0],
        y_axis: [0.0, t.cos(), t.sin()],
    };
    let boss = k
        .extrude(&on(tilted, circle("c1", [0.0, 0.0], 6.0)), 0.0, 15.0, "b")
        .unwrap();
    let body = k.boolean(BooleanOp::Union, plate, boss).unwrap();
    let v0 = solid(&k, body, 0);
    let r = 2.0;
    let out = k
        .blend_edges(
            body,
            &[blend("f1:end", "b:side:c1", [36.0, 20.0, 10.0], r)],
            "f2",
        )
        .unwrap();
    let v = solid(&k, out, 0);
    // 入り隅の丸めは肉を足す。根元の円周 2π6 に断面 r²(1 - π/4) を掛けた分(傾きで少し増える)
    let want = TAU * (6.0 + 0.22 * r) * r * r * (1.0 - PI / 4.0);
    assert!(close(v - v0, want, 0.05), "足した分 {} 理論 {want}", v - v0);
}

/// 段の手前で止まる丸め: 低い段の上の縁を丸めると、縁は高い段の壁で終わる。
#[test]
fn round_that_stops_at_a_step_wall() {
    let k = SmlibKernel::new();
    // XZ 平面の L 字(x 0〜40、低い段は z 10、高い段は x 20〜40 で z 20)を y の負の向きへ 20 押し出す。
    // 低い段の上の面は e5、手前(y = 0)の面は start
    let xz = Plane {
        origin: [0.0; 3],
        x_axis: [1.0, 0.0, 0.0],
        y_axis: [0.0, 0.0, 1.0],
    };
    let l = polygon(&[
        [0.0, 0.0],
        [40.0, 0.0],
        [40.0, 20.0],
        [20.0, 20.0],
        [20.0, 10.0],
        [0.0, 10.0],
    ]);
    let body = k.extrude(&on(xz, l), 0.0, 20.0, "f1").unwrap();
    let v0 = solid(&k, body, 0);
    assert!(close(v0, (40.0 * 10.0 + 20.0 * 10.0) * 20.0, 1e-9));
    let r = 2.0;
    let out = k
        .blend_edges(
            body,
            &[blend("f1:side:e5", "f1:start", [10.0, 0.0, 10.0], r)],
            "f2",
        )
        .unwrap();
    let v = solid(&k, out, 0);
    let want = r * r * (1.0 - PI / 4.0) * 20.0;
    assert!(close(v0 - v, want, 0.05), "除いた分 {} 理論 {want}", v0 - v);
}

/// ドーナツ形(円環)の面を外へ・内へオフセットする。
#[test]
fn offset_the_face_of_a_torus_out_and_in() {
    let k = SmlibKernel::new();
    let torus = revolve_z(&k, circle("c1", [0.0, 20.0], 5.0), TAU, "t").unwrap();
    let face = names(&k, torus)[0].clone();
    let vol = |r: f64| 2.0 * PI * PI * 20.0 * r * r;
    assert!(close(solid(&k, torus, 0), vol(5.0), 1e-4));
    for (d, r) in [(1.0, 6.0), (-1.0, 4.0)] {
        let out = k
            .offset_faces(torus, std::slice::from_ref(&face), d, "o")
            .unwrap_or_else(|e| panic!("{d}: {e}"));
        let v = solid(&k, out, 0);
        assert!(close(v, vol(r), 1e-3), "{d}: {v} 理論 {}", vol(r));
    }
}

// ---- 追加の 1: 失敗や欠けが出る恐れのある形 ----

/// 箱の辺から 1e-4 だけずれたスケッチで切り取る(はみ出す側と、薄く残す側)。
#[test]
fn cut_with_a_sketch_a_hair_off_the_box_edge() {
    let k = SmlibKernel::new();
    let v_box = 40.0 * 20.0 * 10.0;
    for (y0, depth_y) in [(-1e-4, 10.0), (1e-4, 10.0 - 1e-4)] {
        let b = cuboid(&k, [0.0; 3], [40.0, 20.0, 10.0], "f1");
        let tool = cuboid(&k, [10.0, y0, 5.0], [30.0, 10.0, 15.0], "t");
        let out = k
            .boolean(BooleanOp::Subtract, b, tool)
            .unwrap_or_else(|e| panic!("{y0}: {e}"));
        let v = solid(&k, out, 0);
        let want = v_box - 20.0 * depth_y * 5.0;
        assert!(close(v, want, 1e-6), "{y0}: {v} 理論 {want}");
    }
}

/// 平らな所(幅 4)より大きい半径・距離の丸めと面取り。断るか、閉じた立体を作る。
#[test]
fn round_and_chamfer_wider_than_the_flat_face() {
    let k = SmlibKernel::new();
    let b = cuboid(&k, [0.0; 3], [40.0, 4.0, 10.0], "f1");
    for flat in [false, true] {
        let mut e = blend("f1:end", "f1:side:e1", [20.0, 0.0, 10.0], 5.0);
        e.flat = flat;
        let r = k.blend_edges(b, &[e], "f2");
        refuse_or_solid(&k, r);
    }
}

/// 幅 6 の柱の縦の 4 辺を半径 3 より少し大きく丸める(向かいの丸めが重なる)。
#[test]
fn rounds_on_opposite_edges_wider_than_the_post() {
    let k = SmlibKernel::new();
    let post = cuboid(&k, [0.0; 3], [14.0, 6.0, 10.0], "f1");
    let s = |i: u32| format!("f1:side:e{i}");
    for r in [3.03, 3.5, 4.0] {
        let edges = vec![
            blend(&s(4), &s(1), [0.0, 0.0, 5.0], r),
            blend(&s(1), &s(2), [14.0, 0.0, 5.0], r),
            blend(&s(2), &s(3), [14.0, 6.0, 5.0], r),
            blend(&s(3), &s(4), [0.0, 6.0, 5.0], r),
        ];
        let out = k.blend_edges(post, &edges, "f2");
        refuse_or_solid(&k, out);
    }
}

/// 回して作った円柱の上の縁を、小さな半径(0.2)で丸める。
#[test]
fn small_round_on_the_rim_of_a_revolved_body() {
    let k = SmlibKernel::new();
    let body = revolve_z(
        &k,
        polygon(&[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]),
        TAU,
        "f1",
    )
    .unwrap();
    let v0 = solid(&k, body, 0);
    assert!(close(v0, PI * 100.0 * 10.0, 1e-4));
    // 上の面(z = 10)と側面(x = 10)
    let top = k
        .faces(body)
        .unwrap()
        .into_iter()
        .find(|f| f.planar.is_some_and(|p| p.normal[2] > 0.5))
        .unwrap()
        .name;
    let side = names(&k, body)
        .into_iter()
        .find(|n| {
            *n != top
                && !k
                    .faces(body)
                    .unwrap()
                    .iter()
                    .any(|f| f.name == *n && f.planar.is_some())
        })
        .unwrap();
    let r = 0.2;
    let out = k
        .blend_edges(body, &[blend(&top, &side, [10.0, 0.0, 10.0], r)], "f2")
        .unwrap();
    let v = solid(&k, out, 0);
    // 除く断面 r²(1 - π/4) の重心は角から r (10 - 3π) / (12 - 3π) だけ内側(パップスの定理)
    let d = r * (10.0 - 3.0 * PI) / (12.0 - 3.0 * PI);
    let want = TAU * (10.0 - d) * r * r * (1.0 - PI / 4.0);
    assert!(close(v0 - v, want, 1e-2), "除いた分 {} 理論 {want}", v0 - v);
}

/// 極短い線(1e-4・1e-5)のある輪郭を、まっすぐと勾配つきで押し出す。1e-4 は作れ、1e-5 は断ってもよい。
#[test]
fn extrude_a_profile_with_a_very_short_line() {
    let k = SmlibKernel::new();
    for s in [1e-4, 1e-5] {
        let l = polygon(&[
            [0.0, 0.0],
            [20.0, 0.0],
            [20.0, 10.0],
            [s, 10.0],
            [0.0, 10.0 - s],
        ]);
        let p = on(Plane::XY, l);
        let area = 200.0 - s * s / 2.0;
        let h = k.extrude(&p, 0.0, 5.0, "f1");
        if s < 1e-4 {
            // カーネルの許容差(1e-5)と同じ長さの線は、断って誤りを返してもよい
            if let Some(v) = refuse_or_solid(&k, h) {
                assert!(close(v, area * 5.0, 1e-6), "{s}");
            }
        } else {
            let h = h.unwrap_or_else(|e| panic!("{s}: {e}"));
            assert!(close(solid(&k, h, 0), area * 5.0, 1e-6), "{s}");
        }
        // 勾配は断っても誤りで返ればよい(落ちたり崩れた形を返したりしない)
        let t = k.extrude_taper(&p, 0.0, 5.0, 3f64.to_radians(), "f2");
        refuse_or_solid(&k, t);
    }
}

// ---- 追加の 2: 黙って違う形になったり失敗したりする 14 の形 ----

/// 小さな円柱(半径 1)を箱の縦の辺の上に置いて引く・共通部分を取る。
#[test]
fn small_cylinder_on_a_box_edge_cut_and_intersect() {
    let k = SmlibKernel::new();
    let quarter = PI * 1.0 * 10.0 / 4.0;
    let b = cuboid(&k, [0.0; 3], [40.0, 20.0, 10.0], "f1");
    let c = cylinder(&k, [40.0, 20.0], 1.0, -1.0, 11.0, "c");
    let cut = k.boolean(BooleanOp::Subtract, b, c).unwrap();
    assert!(close(solid(&k, cut, 0), 8000.0 - quarter, 1e-6));
    let b = cuboid(&k, [0.0; 3], [40.0, 20.0, 10.0], "f1");
    let c = cylinder(&k, [40.0, 20.0], 1.0, -1.0, 11.0, "c");
    let common = k.boolean(BooleanOp::Intersect, b, c).unwrap();
    assert!(close(solid(&k, common, 0), quarter, 1e-6));
}

/// 薄い輪郭(0.5 × 5)を、直角に折れた経路に沿って動かす。断るか、正しい体積の立体。
#[test]
fn sweep_a_thin_profile_along_a_sharp_corner() {
    let k = SmlibKernel::new();
    // 輪郭は XY 平面(z 0)、経路は ZX 平面((z, x) の座標)で z 0→30、そこから x 0→30
    let p = on(Plane::XY, rect(-0.25, -2.5, 0.25, 2.5));
    let path = PlanarPath {
        plane: Plane::ZX,
        edges: vec![
            line("p1", [0.0, 0.0], [30.0, 0.0]),
            line("p2", [30.0, 0.0], [30.0, 30.0]),
        ],
    };
    match k.sweep(&p, &path, 0.0, 1.0, "f1") {
        Err(e) => eprintln!("断った: {e}"),
        Ok(h) => {
            let v = solid(&k, h, 0);
            assert!(close(v, 2.5 * 60.0, 1e-3), "{v}");
        }
    }
}

/// 面で接する 2 つの箱を足す(面の全部と一部)。
#[test]
fn union_of_boxes_touching_on_a_face() {
    let k = SmlibKernel::new();
    for (lo, hi, add) in [
        ([20.0, 0.0, 0.0], [40.0, 20.0, 10.0], 4000.0),
        ([20.0, 5.0, 2.0], [40.0, 15.0, 8.0], 1200.0),
    ] {
        let a = cuboid(&k, [0.0; 3], [20.0, 20.0, 10.0], "a");
        let b = cuboid(&k, lo, hi, "b");
        let u = k.boolean(BooleanOp::Union, a, b).unwrap();
        assert!(close(solid(&k, u, 0), 4000.0 + add, 1e-6));
    }
}

/// 端の断面が同じ形のロフト(角柱になる)と、平行な四角から円へのロフト。
#[test]
fn loft_with_equal_end_sections_and_square_to_circle() {
    let k = SmlibKernel::new();
    let sq = |z: f64| on(Plane::XY.offset(z), rect(-10.0, -10.0, 10.0, 10.0));
    let h = k.loft(&[sq(0.0), sq(10.0)], &[], "f1").unwrap();
    assert!(close(solid(&k, h, 0), 4000.0, 1e-4));
    let h = k.loft(&[sq(0.0), sq(10.0), sq(20.0)], &[], "f2").unwrap();
    assert!(close(solid(&k, h, 0), 8000.0, 1e-3));
    let c = on(Plane::XY.offset(20.0), circle("c1", [0.0, 0.0], 8.0));
    let h = k.loft(&[sq(0.0), c], &[], "f3").unwrap();
    let v = solid(&k, h, 0);
    assert!(v > PI * 64.0 * 20.0 && v < 8000.0, "{v}");
}

/// 辺で接する 2 つの長方形の輪郭を押し出す(1 つの立体)。
#[test]
fn extrude_two_rectangles_touching_on_an_edge() {
    let k = SmlibKernel::new();
    let p = Profile {
        plane: Plane::XY,
        regions: vec![
            Region {
                outer: rect(0.0, 0.0, 10.0, 10.0),
                holes: vec![],
            },
            Region {
                outer: rect(10.0, 2.0, 20.0, 8.0),
                holes: vec![],
            },
        ],
    };
    match k.extrude(&p, 0.0, 5.0, "f1") {
        Err(e) => eprintln!("断った: {e}"),
        Ok(h) => {
            let (_, solids, _) = k.check(h).unwrap();
            assert!(solids >= 1);
            assert!(close(k.volume(h).unwrap(), (100.0 + 60.0) * 5.0, 1e-6));
        }
    }
}

/// 接線から 3° だけ折れた角(出隅と浅い入り隅)を丸める。
#[test]
fn round_a_corner_a_few_degrees_off_tangent() {
    let k = SmlibKernel::new();
    let d = 20.0 * 3f64.to_radians().tan();
    for (dy, convex) in [(d, true), (-d, false)] {
        let l = polygon(&[
            [0.0, 0.0],
            [20.0, 0.0],
            [40.0, dy],
            [40.0, 20.0],
            [0.0, 20.0],
        ]);
        let b = k.extrude(&on(Plane::XY, l), 0.0, 10.0, "f1").unwrap();
        let v0 = solid(&k, b, 0);
        for r in [1.0, 5.0, 20.0] {
            let out = k
                .blend_edges(
                    b,
                    &[blend("f1:side:e1", "f1:side:e2", [20.0, 0.0, 5.0], r)],
                    "f2",
                )
                .unwrap_or_else(|e| panic!("出隅 {convex} 半径 {r}: {e}"));
            let v = solid(&k, out, 0);
            // 折れ角 θ の丸めの断面は r²(tan(θ/2) - θ/2)
            let th = 3f64.to_radians();
            let want = r * r * ((th / 2.0).tan() - th / 2.0) * 10.0;
            let got = if convex { v0 - v } else { v - v0 };
            assert!(
                close(got, want, 0.05),
                "出隅 {convex} 半径 {r}: {got} 理論 {want}"
            );
        }
    }
}

/// 丸めの隣の面をオフセットする(丸めを付けたまま面を動かす)。
#[test]
fn offset_the_face_next_to_a_round() {
    let k = SmlibKernel::new();
    let b = cuboid(&k, [0.0; 3], [40.0, 20.0, 10.0], "f1");
    let r = 2.0;
    let rounded = k
        .blend_edges(
            b,
            &[blend("f1:end", "f1:side:e1", [20.0, 0.0, 10.0], r)],
            "f2",
        )
        .unwrap();
    let v0 = solid(&k, rounded, 0);
    let out = k
        .offset_faces(rounded, &["f1:side:e3".into()], 1.0, "o")
        .unwrap();
    assert!(close(solid(&k, out, 0) - v0, 400.0, 1e-4));
    // 丸めに接する面そのもの
    let out = k.offset_faces(rounded, &["f1:side:e1".into()], -1.0, "o");
    if let Some(v) = refuse_or_solid(&k, out) {
        assert!(close(v0 - v, 40.0 * (10.0 - r), 0.05), "{}", v0 - v);
    }
}

/// 中の空洞をちょうど埋める箱を足すと、空洞の無い 1 つの箱になる。
/// 今のカーネルではまだ起きる。
#[test]
fn union_that_fills_a_cavity_still_occurs() {
    still_occurs("union_that_fills_a_cavity", union_that_fills_a_cavity);
}

fn union_that_fills_a_cavity() {
    let k = SmlibKernel::new();
    let a = cuboid(&k, [0.0; 3], [40.0; 3], "a");
    let hole = cuboid(&k, [10.0; 3], [30.0; 3], "b");
    let hollow = k.boolean(BooleanOp::Subtract, a, hole).unwrap();
    assert!(close(solid(&k, hollow, 1), 64000.0 - 8000.0, 1e-6));
    let fill = cuboid(&k, [10.0; 3], [30.0; 3], "c");
    let full = k.boolean(BooleanOp::Union, hollow, fill).unwrap();
    assert!(close(solid(&k, full, 0), 64000.0, 1e-6));
}

/// 頂点のある円錐(回して作る)を、底を開けてシェルにする。
#[test]
fn shell_a_cone_with_an_apex() {
    let k = SmlibKernel::new();
    // (z, x): 底の円 x 10、頂点 z 10
    let cone = revolve_z(
        &k,
        polygon(&[[0.0, 0.0], [0.0, 10.0], [10.0, 0.0]]),
        TAU,
        "f1",
    )
    .unwrap();
    assert!(close(solid(&k, cone, 0), PI * 1000.0 / 3.0, 1e-4));
    let base = k
        .faces(cone)
        .unwrap()
        .into_iter()
        .find(|f| f.planar.is_some())
        .unwrap()
        .name;
    let out = k.shell(cone, &[base], 1.0, "s");
    if let Some(v) = refuse_or_solid(&k, out) {
        // 斜面 x + z = 10 から内へ 1(x + z = 10 - √2)。中は高さと半径が 10 - √2 の円錐
        let h = 10.0 - 2f64.sqrt();
        let want = PI / 3.0 * (1000.0 - h * h * h);
        assert!(close(v, want, 0.01), "{v} 理論 {want}");
    }
}

/// 回して作った球から、半径の違う球(同心と、ずらした物)を回して作って引く。
#[test]
fn cut_a_revolved_sphere_by_spheres_of_other_radii() {
    let k = SmlibKernel::new();
    // (z, x) の半円(z 軸の上の直径と、右の半円)
    let half = |r: f64, z: f64, name: &str| ProfileLoop {
        edges: vec![
            line(&format!("{name}a"), [z - r, 0.0], [z + r, 0.0]),
            arc(&format!("{name}b"), [z, 0.0], r, 0.0, PI),
        ],
    };
    let ball = |r: f64, z: f64, tag: &str| revolve_z(&k, half(r, z, tag), TAU, tag).unwrap();
    let vol = |r: f64| 4.0 / 3.0 * PI * r * r * r;
    let big = ball(10.0, 0.0, "a");
    assert!(close(solid(&k, big, 0), vol(10.0), 1e-4));
    let hollow = k
        .boolean(BooleanOp::Subtract, big, ball(6.0, 0.0, "b"))
        .unwrap();
    assert!(close(solid(&k, hollow, 1), vol(10.0) - vol(6.0), 1e-4));
    // 中心を軸の上で 8 ずらした半径 6(表を抜ける)。重なりはレンズ(2 つの球の欠片)
    let big = ball(10.0, 0.0, "c");
    let bite = k
        .boolean(BooleanOp::Subtract, big, ball(6.0, 8.0, "d"))
        .unwrap();
    let (r1, r2, d) = (10.0f64, 6.0f64, 8.0f64);
    let lens = PI
        * (r1 + r2 - d).powi(2)
        * (d * d + 2.0 * d * r2 - 3.0 * r2 * r2 + 2.0 * d * r1 + 6.0 * r2 * r1 - 3.0 * r1 * r1)
        / (12.0 * d);
    assert!(close(solid(&k, bite, 0), vol(10.0) - lens, 1e-4));
}

/// STEP に書き出した円錐台と球を読み直すと、閉じた立体で体積が同じ。
#[test]
fn step_round_trip_of_a_cone_and_a_sphere() {
    let k = SmlibKernel::new();
    let cone = revolve_z(
        &k,
        polygon(&[[0.0, 0.0], [0.0, 10.0], [10.0, 4.0], [10.0, 0.0]]),
        TAU,
        "a",
    )
    .unwrap();
    let ball = revolve_z(
        &k,
        ProfileLoop {
            edges: vec![
                line("l", [-5.0, 0.0], [5.0, 0.0]),
                arc("r", [0.0, 0.0], 5.0, 0.0, PI),
            ],
        },
        TAU,
        "b",
    )
    .unwrap();
    for h in [cone, ball] {
        let v0 = solid(&k, h, 0);
        let step = k.export_step(&[h]).unwrap();
        let back = k.import_step(&step).unwrap();
        assert!(close(solid(&k, back, 0), v0, 1e-4));
    }
}

// ---- 追加の 3: 一周回した形どうしの切り取り・浅い入り隅 ----

/// 一周回した円柱から、一周回したリング(中に埋まる物と、表を抜ける物)を引く。
#[test]
fn cut_a_full_revolve_by_a_full_revolve() {
    let k = SmlibKernel::new();
    let body = || revolve_z(&k, rect(0.0, 0.0, 20.0, 20.0), TAU, "a").unwrap();
    let v0 = PI * 400.0 * 20.0;
    let inner = revolve_z(&k, rect(5.0, 10.0, 15.0, 15.0), TAU, "b").unwrap();
    let out = k.boolean(BooleanOp::Subtract, body(), inner).unwrap();
    assert!(close(
        solid(&k, out, 1),
        v0 - PI * (225.0 - 100.0) * 10.0,
        1e-5
    ));
    let groove = revolve_z(&k, rect(5.0, 15.0, 15.0, 25.0), TAU, "c").unwrap();
    let out = k.boolean(BooleanOp::Subtract, body(), groove).unwrap();
    assert!(close(
        solid(&k, out, 0),
        v0 - PI * (400.0 - 225.0) * 10.0,
        1e-5
    ));
}

// ---- 追加の 4 ----

/// 同じ形の写しを引くと、何も残らない(誤りか空)。
#[test]
fn subtracting_an_equal_copy_leaves_nothing() {
    let k = SmlibKernel::new();
    let a = cuboid(&k, [0.0; 3], [20.0, 10.0, 5.0], "a");
    let b = cuboid(&k, [0.0; 3], [20.0, 10.0, 5.0], "b");
    match k.boolean(BooleanOp::Subtract, a, b) {
        Err(e) => eprintln!("誤り: {e}"),
        Ok(h) => {
            let v = k.volume(h).unwrap_or(0.0);
            assert!(v.abs() < 1e-6, "空にならない: 体積 {v}");
        }
    }
}

/// 重なった 2 本の円柱を足し、交わりの入り隅を丸める。
#[test]
fn round_where_two_overlapping_cylinders_meet() {
    let k = SmlibKernel::new();
    let a = cylinder(&k, [0.0, 0.0], 5.0, 0.0, 10.0, "a");
    let b = cylinder(&k, [6.0, 0.0], 5.0, 0.0, 10.0, "b");
    let u = k.boolean(BooleanOp::Union, a, b).unwrap();
    let v0 = solid(&k, u, 0);
    // 足すと b の側面は a に切られて 2 枚(#0・#1)に分かれる。どちらが y の正の側かは決めない
    let pair = |p: &str, q: &str| {
        k.blend_edges(
            u,
            &[
                blend("a:side:c1", p, [3.0, 4.0, 5.0], 1.0),
                blend("a:side:c1", q, [3.0, -4.0, 5.0], 1.0),
            ],
            "f2",
        )
    };
    let out = pair("b:side:c1#0", "b:side:c1#1")
        .or_else(|_| pair("b:side:c1#1", "b:side:c1#0"))
        .unwrap();
    let v = solid(&k, out, 0);
    assert!(v > v0 && v - v0 < 2.0 * 10.0 * 1.0, "{}", v - v0);
}

/// 四角から小さな四角へのロフトの側面は平面(スケッチを置ける)。
/// 今のカーネルではまだ起きる。
#[test]
fn flat_sides_of_a_loft_are_planes_still_occurs() {
    still_occurs(
        "flat_sides_of_a_loft_are_planes",
        flat_sides_of_a_loft_are_planes,
    );
}

fn flat_sides_of_a_loft_are_planes() {
    let k = SmlibKernel::new();
    let a = on(Plane::XY, rect(-10.0, -10.0, 10.0, 10.0));
    let b = on(Plane::XY.offset(10.0), rect(-5.0, -5.0, 5.0, 5.0));
    let h = k.loft(&[a, b], &[], "f1").unwrap();
    let v = solid(&k, h, 0);
    assert!(close(v, 10.0 / 3.0 * (400.0 + 100.0 + 200.0), 1e-4), "{v}");
    let faces = k.faces(h).unwrap();
    let flat = faces.iter().filter(|f| f.planar.is_some()).count();
    assert_eq!(flat, faces.len(), "{faces:?}");
}

// ---- 一覧の形(JC が呼ぶ口に当たる物)----

/// 半分のリングを x 軸のまわりに一周回すと、空洞のある球の殻。中の箱を引いても何も変わらない。
#[test]
fn full_revolve_of_a_half_ring_is_a_hollow_ball() {
    let k = SmlibKernel::new();
    let ring = ProfileLoop {
        edges: vec![
            line("e1", [18.5, 0.0], [40.0, 0.0]),
            arc("e2", [0.0, 0.0], 40.0, 0.0, PI),
            line("e3", [-40.0, 0.0], [-18.5, 0.0]),
            ProfileEdge {
                reversed: true,
                ..arc("e4", [0.0, 0.0], 18.5, 0.0, PI)
            },
        ],
    };
    let shell = k
        .revolve(
            &on(Plane::XY, ring.clone()),
            [0.0; 3],
            [1.0, 0.0, 0.0],
            TAU,
            "s",
        )
        .unwrap();
    let want = 4.0 / 3.0 * PI * (40f64.powi(3) - 18.5f64.powi(3));
    assert!(close(solid(&k, shell, 1), want, 1e-4));
    // 空洞の中の箱から殻を引いても箱はそのまま
    let shell = k
        .revolve(&on(Plane::XY, ring), [0.0; 3], [1.0, 0.0, 0.0], TAU, "s2")
        .unwrap();
    let b = cuboid(&k, [-4.0; 3], [4.0; 3], "b");
    let out = k.boolean(BooleanOp::Subtract, b, shell).unwrap();
    assert!(close(solid(&k, out, 0), 512.0, 1e-6));
}

/// 円を、円に接する軸のまわりに 90° 回す(輪郭の途中が軸に触れる)。
#[test]
fn partial_revolve_of_a_circle_touching_the_axis() {
    let k = SmlibKernel::new();
    let r = revolve_z(&k, circle("c1", [0.0, 5.0], 5.0), PI / 2.0, "f1");
    match r {
        Err(e) => eprintln!("断った: {e}"),
        Ok(h) => {
            let want = 0.25 * TAU * 5.0 * PI * 25.0;
            let (_, solids, _) = k.check(h).unwrap();
            assert!(solids >= 1);
            assert!(close(k.volume(h).unwrap(), want, 1e-4));
        }
    }
}

/// 角を丸めた箱を内へシェルにする(閉じたまま・上を開けて)。同じ形を 3 回作って同じ結果。
/// 今のカーネルではまだ起きる。
#[test]
fn shell_a_box_with_rounded_edges_still_occurs() {
    still_occurs(
        "shell_a_box_with_rounded_edges",
        shell_a_box_with_rounded_edges,
    );
}

fn shell_a_box_with_rounded_edges() {
    let k = SmlibKernel::new();
    let mut got = vec![];
    for i in 0..3 {
        let b = cuboid(&k, [0.0; 3], [30.0, 20.0, 10.0], "f1");
        let s = |i: u32| format!("f1:side:e{i}");
        let edges = vec![
            blend(&s(4), &s(1), [0.0, 0.0, 5.0], 3.0),
            blend(&s(1), &s(2), [30.0, 0.0, 5.0], 3.0),
            blend(&s(2), &s(3), [30.0, 20.0, 5.0], 3.0),
            blend(&s(3), &s(4), [0.0, 20.0, 5.0], 3.0),
        ];
        let rounded = k.blend_edges(b, &edges, &format!("r{i}")).unwrap();
        let open = k.shell(rounded, &["f1:end".into()], 2.0, &format!("s{i}"));
        let closed = k.shell(rounded, &[], 2.0, &format!("c{i}"));
        got.push((
            open.map(|h| (k.volume(h).unwrap() * 1e6).round())
                .map_err(|e| e.to_string()),
            closed
                .map(|h| (k.volume(h).unwrap() * 1e6).round())
                .map_err(|e| e.to_string()),
        ));
    }
    assert!(got[0].0.is_ok() && got[0].1.is_ok(), "{got:?}");
    assert!(got.iter().all(|g| *g == got[0]), "回ごとに違う: {got:?}");
}

/// 円板の片方の縁を丸めて、反対側を開けて内へシェルにする。
/// 今のカーネルではまだ起きる。
#[test]
fn shell_a_disc_with_one_rounded_rim_still_occurs() {
    still_occurs(
        "shell_a_disc_with_one_rounded_rim",
        shell_a_disc_with_one_rounded_rim,
    );
}

fn shell_a_disc_with_one_rounded_rim() {
    let k = SmlibKernel::new();
    let disc = cylinder(&k, [0.0, 0.0], 15.0, 0.0, 5.0, "f1");
    let rounded = k
        .blend_edges(
            disc,
            &[blend("f1:end", "f1:side:c1", [15.0, 0.0, 5.0], 1.5)],
            "f2",
        )
        .unwrap();
    let v0 = solid(&k, rounded, 0);
    for t in [1.6, 2.0] {
        let out = k
            .shell(rounded, &["f1:start".into()], t, "s")
            .unwrap_or_else(|e| panic!("{t}: {e}"));
        let v = solid(&k, out, 0);
        assert!(v > 0.0 && v < v0, "{t}: {v}");
    }
}

/// 通し穴が丸めの帯に食い込む辺を丸める。断るか、閉じた立体。
#[test]
fn round_an_edge_whose_band_a_hole_breaks_into() {
    let k = SmlibKernel::new();
    let b = cuboid(&k, [0.0; 3], [40.0, 20.0, 10.0], "f1");
    let h = cylinder(&k, [20.0, 14.5], 3.0, -1.0, 11.0, "h");
    let body = k.boolean(BooleanOp::Subtract, b, h).unwrap();
    let r = k.blend_edges(
        body,
        &[blend("f1:end", "f1:side:e3", [5.0, 20.0, 10.0], 4.0)],
        "f2",
    );
    refuse_or_solid(&k, r);
}

/// ボスの根元一周の丸めを付けて、その丸めを外すと元に戻る。
/// 今のカーネルではまだ起きる。
#[test]
fn removing_a_ring_round_at_the_foot_of_a_boss_still_occurs() {
    still_occurs(
        "removing_a_ring_round_at_the_foot_of_a_boss",
        removing_a_ring_round_at_the_foot_of_a_boss,
    );
}

fn removing_a_ring_round_at_the_foot_of_a_boss() {
    let k = SmlibKernel::new();
    let plate = cuboid(&k, [0.0; 3], [60.0, 40.0, 10.0], "f1");
    let boss = cylinder(&k, [30.0, 20.0], 10.0, 10.0, 20.0, "b");
    let body = k.boolean(BooleanOp::Union, plate, boss).unwrap();
    let v0 = solid(&k, body, 0);
    let rounded = k
        .blend_edges(
            body,
            &[blend("f1:end", "b:side:c1", [40.0, 20.0, 10.0], 2.0)],
            "f2",
        )
        .unwrap();
    assert!(solid(&k, rounded, 0) > v0);
    let round = face_starting(&k, rounded, "f2:");
    let back = k.remove_blend_faces(rounded, &[round], "f3").unwrap();
    assert!(close(solid(&k, back, 0), v0, 1e-6));
}

/// 中が残らないほど厚いシェル(10 の立方体を 5・6 で)は断る。
#[test]
fn shell_thicker_than_the_inradius_is_refused() {
    let k = SmlibKernel::new();
    let b = cuboid(&k, [0.0; 3], [10.0; 3], "f1");
    for t in [5.0, 6.0] {
        assert!(k.shell(b, &[], t, "s").is_err(), "{t}");
    }
}

/// 表示用の網は、ボディの全部の面を持つ(1 枚が網にならなくても、ほかの面は残す)。
#[test]
fn mesh_keeps_every_face_of_a_body() {
    let k = SmlibKernel::new();
    let b = cuboid(&k, [0.0; 3], [40.0, 20.0, 10.0], "f1");
    let h = cylinder(&k, [20.0, 10.0], 4.0, -1.0, 11.0, "h");
    let body = k.boolean(BooleanOp::Subtract, b, h).unwrap();
    let m = k.mesh(body, 0.05).unwrap();
    let mut seen: Vec<u32> = m.triangle_faces.clone();
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), names(&k, body).len());
}
