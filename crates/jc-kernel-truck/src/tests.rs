//! truck のカーネルの試験。SMLib と共通の確かめは jc-contract の契約(kernel_contract_tests!)で、
//! ここには truck だけの振る舞いを置く。
use super::*;
use crate::parts::adopt;
use jc_contract::kernel::{plate_profile, rect_loop};
use jc_core::kernel::{Plane, ProfileEdge};

fn kernel() -> TruckKernel {
    TruckKernel::new()
}

/// truck は円を 2 つに割るので、穴の側面は 2 面。
const QUIRKS: jc_contract::kernel::KernelQuirks<TruckKernel> = jc_contract::kernel::KernelQuirks {
    shape_count: TruckKernel::shape_count,
    hole_side_faces: &["f1:side:e9.a", "f1:side:e9.b"],
    step_is_solid: false,
};

jc_contract::kernel_contract_tests!(kernel, QUIRKS);

#[test]
fn flush_cut_fails_with_a_clear_message() {
    let k = TruckKernel::new();
    let plate = k.extrude(&plate_profile(), 0.0, 10.0, "f1").unwrap();
    let tool_profile = Profile {
        plane: Plane::XY,
        regions: vec![Region {
            outer: rect_loop(5.0, 5.0, 15.0, 10.0, 20),
            holes: vec![],
        }],
    };
    let tool = k.extrude(&tool_profile, 5.0, 10.0, "f2").unwrap();
    let err = k.boolean(BooleanOp::Subtract, plate, tool).unwrap_err();
    assert!(err.to_string().contains("同じ平面"), "{err}");
}

// ---- 空洞(内側の殻)のあるブーリアン。体積を、軸に沿った箱の集合として厳密に数えた値と比べる。
// truck の結果は殻の並び順(実行ごとに変わる)で揺れることがあるので、揺れた不具合の物は
// 同じ操作を何回か繰り返す。

/// 揺れた不具合を確かめる繰り返しの回数(直す前は 2〜3 回に 1 回落ちた)
const REPEAT: usize = 10;

fn cuboid(k: &TruckKernel, lo: P3, hi: P3, tag: &str) -> ShapeHandle {
    let p = Profile {
        plane: Plane::XY,
        regions: vec![Region {
            outer: rect_loop(lo[0], lo[1], hi[0] - lo[0], hi[1] - lo[1], 1),
            holes: vec![],
        }],
    };
    k.extrude(&p, lo[2], hi[2], tag).unwrap()
}

/// 外 `lo..hi`、空洞 `vlo..vhi` の箱
fn hollow(k: &TruckKernel, lo: P3, hi: P3, vlo: P3, vhi: P3) -> ShapeHandle {
    let a = cuboid(k, lo, hi, "a");
    let v = cuboid(k, vlo, vhi, "v");
    k.boolean(BooleanOp::Subtract, a, v).unwrap()
}

/// 開いた箱 `lo..hi` の中か
fn inb(p: P3, lo: P3, hi: P3) -> bool {
    (0..3).all(|i| lo[i] < p[i] && p[i] < hi[i])
}

/// 軸に沿った箱だけでできた集合の体積。箱の座標でできる格子のセルの中心で数える(厳密)
fn set_volume(boxes: &[(P3, P3)], member: impl Fn(P3) -> bool) -> f64 {
    let mut ax: Vec<Vec<f64>> = vec![Vec::new(); 3];
    for (lo, hi) in boxes {
        for i in 0..3 {
            ax[i].push(lo[i]);
            ax[i].push(hi[i]);
        }
    }
    for a in &mut ax {
        a.sort_by(f64::total_cmp);
        a.dedup();
    }
    let mut v = 0.0;
    for x in ax[0].windows(2) {
        for y in ax[1].windows(2) {
            for z in ax[2].windows(2) {
                let c = [
                    (x[0] + x[1]) / 2.0,
                    (y[0] + y[1]) / 2.0,
                    (z[0] + z[1]) / 2.0,
                ];
                if member(c) {
                    v += (x[1] - x[0]) * (y[1] - y[0]) * (z[1] - z[0]);
                }
            }
        }
    }
    v
}

#[track_caller]
fn assert_volume(k: &TruckKernel, r: Result<ShapeHandle, KernelError>, want: f64) {
    let h = r.unwrap_or_else(|e| panic!("失敗しました: {e}(体積 {want} のはず)"));
    let v = k.mass(h).unwrap().volume;
    assert!(
        (v - want).abs() <= 1e-5 * want.max(1.0),
        "体積 {v}、{want} のはず"
    );
}

/// A: 外 0..40、空洞 10..30
const A: (P3, P3) = ([0.0; 3], [40.0; 3]);
const AV: (P3, P3) = ([10.0; 3], [30.0; 3]);
/// B: 外 x20..60、空洞 x25..45(A の空洞と x25..30 で重なる)
const B: (P3, P3) = ([20.0, -3.0, -3.0], [60.0, 43.0, 43.0]);
const BV: (P3, P3) = ([25.0, 8.0, 8.0], [45.0, 32.0, 32.0]);
const AB: [(P3, P3); 4] = [A, AV, B, BV];

fn in_a(p: P3) -> bool {
    inb(p, A.0, A.1) && !inb(p, AV.0, AV.1)
}
fn in_b(p: P3) -> bool {
    inb(p, B.0, B.1) && !inb(p, BV.0, BV.1)
}
fn hollow_a(k: &TruckKernel) -> ShapeHandle {
    hollow(k, A.0, A.1, AV.0, AV.1)
}
fn hollow_b(k: &TruckKernel) -> ShapeHandle {
    hollow(k, B.0, B.1, BV.0, BV.1)
}

/// 空洞の行き先: 空洞を持つ箱を L 字の溝で切ると、空洞は角の塊に付く。空洞の外形の箱は、
/// それを囲む L 字の残りの外形の箱にも入る(以前は外形の箱で選び、L 字の残りに付けていた)
#[test]
fn a_void_stays_with_the_piece_that_encloses_it() {
    let k = kernel();
    let a = hollow(
        &k,
        [0.0; 3],
        [60.0, 40.0, 20.0],
        [40.0, 5.0, 5.0],
        [50.0, 15.0, 15.0],
    );
    let t1 = cuboid(&k, [30.0, -5.0, -5.0], [33.0, 23.0, 25.0], "t1");
    let t2 = cuboid(&k, [31.0, 20.0, -6.0], [65.0, 22.5, 26.0], "t2");
    let t = k.boolean(BooleanOp::Union, t1, t2).unwrap();
    let pick = cuboid(&k, [32.0, -1.0, -1.0], [61.0, 19.0, 21.0], "k");
    for _ in 0..REPEAT {
        let cut = k.boolean(BooleanOp::Subtract, a, t).unwrap();
        // 角の塊(x33..60、y0..20)と pick(y..19)の共通 = 27×19×20 − 空洞 1000
        let corner = k.boolean(BooleanOp::Intersect, cut, pick);
        assert_volume(&k, corner, 27.0 * 19.0 * 20.0 - 1000.0);
    }
}

/// 結果の殻を分ける所で、空洞を、それを囲む外側の殻(光線で数える)に付ける
#[test]
fn adopt_gives_a_void_to_the_shell_around_it() {
    let k = kernel();
    // L 字(外形の箱 0..60 × 0..40)と、その凹みの中の塊(40..60 × 0..20)。空洞は塊の中
    let pts = [
        [0.0, 0.0],
        [30.0, 0.0],
        [30.0, 25.0],
        [60.0, 25.0],
        [60.0, 40.0],
        [0.0, 40.0],
    ];
    let outer = ProfileLoop {
        edges: (0..6)
            .map(|i| ProfileEdge {
                name: format!("e{i}"),
                segment: Segment::Line {
                    a: pts[i],
                    b: pts[(i + 1) % 6],
                },
                reversed: false,
            })
            .collect(),
    };
    let l_profile = Profile {
        plane: Plane::XY,
        regions: vec![Region {
            outer,
            holes: vec![],
        }],
    };
    let l = k.extrude(&l_profile, 0.0, 20.0, "l").unwrap();
    let block = hollow(
        &k,
        [40.0, 0.0, 0.0],
        [60.0, 20.0, 20.0],
        [45.0, 5.0, 5.0],
        [55.0, 15.0, 15.0],
    );
    let lp = k.get(l).unwrap().parts.remove(0);
    let bp = k.get(block).unwrap().parts.remove(0);
    assert_eq!(bp.solid.boundaries().len(), 2);
    for order in [[0, 1], [1, 0]] {
        let mut shells = vec![lp.solid.boundaries()[0].clone()];
        let block_shells = bp.solid.boundaries();
        shells.extend(order.map(|i| block_shells[i].clone()));
        let parts = adopt(tm::Solid::new(shells), &[&lp, &bp]).unwrap();
        let mut vols: Vec<(usize, f64)> = parts
            .iter()
            .map(|p| (p.solid.boundaries().len(), p.volume))
            .collect();
        vols.sort_by(|x, y| x.1.total_cmp(&y.1));
        assert_eq!(vols.len(), 2);
        assert_eq!(vols[0].0, 2, "空洞は塊に付く: {vols:?}");
        assert!(
            (vols[0].1 - 7000.0).abs() < 1.0,
            "塊の体積は空洞を除く: {vols:?}"
        );
        assert_eq!(vols[1].0, 1, "L 字には付かない: {vols:?}");
    }
}

/// 空洞が 2 つある形で、片方の空洞の上を開ける。以前は殻の並びにより truck-shapeops の中で
/// 「index out of bounds」の panic になった
#[test]
fn opening_one_of_two_voids() {
    let k = kernel();
    let a = hollow(&k, [0.0; 3], [60.0, 20.0, 20.0], [5.0; 3], [15.0; 3]);
    let v2 = cuboid(&k, [45.0, 5.0, 5.0], [55.0, 15.0, 15.0], "v2");
    let a = k.boolean(BooleanOp::Subtract, a, v2).unwrap();
    let base = 60.0 * 400.0 - 2000.0;
    assert_volume(&k, Ok(a), base);
    let t = cuboid(&k, [40.0, -5.0, 12.0], [70.0, 25.0, 25.0], "t");
    for _ in 0..REPEAT {
        let want = base - 20.0 * 20.0 * 8.0 + 10.0 * 10.0 * 3.0;
        assert_volume(&k, k.boolean(BooleanOp::Subtract, a, t), want);
    }
    // どちらの空洞にも触れずに切る
    let t = cuboid(&k, [25.0, -5.0, 17.0], [35.0, 25.0, 25.0], "t");
    assert_volume(
        &k,
        k.boolean(BooleanOp::Subtract, a, t),
        base - 10.0 * 20.0 * 3.0,
    );
}

/// 空洞のある 2 つの箱を足す。空洞どうしが重なる(以前は重なりを 2 回引いて失敗した)
#[test]
fn union_of_bodies_whose_voids_overlap() {
    let k = kernel();
    let (a, b) = (hollow_a(&k), hollow_b(&k));
    let want = set_volume(&AB, |p| in_a(p) || in_b(p));
    for _ in 0..REPEAT / 2 {
        assert_volume(&k, k.boolean(BooleanOp::Union, a, b), want);
    }
    // 空洞が相手の中身とだけ重なる
    let (blo, bhi, bvlo, bvhi) = (B.0, B.1, [33.0, 8.0, 8.0], [50.0, 32.0, 32.0]);
    let b = hollow(&k, blo, bhi, bvlo, bvhi);
    let boxes = [A, AV, (blo, bhi), (bvlo, bvhi)];
    let want = set_volume(&boxes, |p| {
        in_a(p) || (inb(p, blo, bhi) && !inb(p, bvlo, bvhi))
    });
    assert_volume(&k, k.boolean(BooleanOp::Union, a, b), want);
}

/// 空洞のある 2 つの箱の共通部分と差。空洞どうしが重なる
#[test]
fn intersect_and_subtract_bodies_whose_voids_overlap() {
    let k = kernel();
    let (a, b) = (hollow_a(&k), hollow_b(&k));
    for _ in 0..REPEAT / 2 {
        let want = set_volume(&AB, |p| in_a(p) && in_b(p));
        assert_volume(&k, k.boolean(BooleanOp::Intersect, a, b), want);
        let want = set_volume(&AB, |p| in_a(p) && !in_b(p));
        assert_volume(&k, k.boolean(BooleanOp::Subtract, a, b), want);
    }
    // 空洞のある道具で、空洞の無い箱を引く。道具の空洞の中の中身は残る
    let plain = cuboid(&k, A.0, A.1, "p");
    let want = set_volume(&AB, |p| inb(p, A.0, A.1) && !in_b(p));
    assert_volume(&k, k.boolean(BooleanOp::Subtract, plain, b), want);
}

/// 中空の箱から、その空洞を囲む中空の道具を引く。以前は空洞を足した後の体積を数え直さず、
/// 結果の検査が正しい結果を拒んだ
#[test]
fn subtract_a_hollow_tool_around_the_void() {
    let k = kernel();
    let a = hollow(&k, [0.0; 3], [40.0; 3], [17.0; 3], [23.0; 3]);
    let b = hollow(&k, [5.0; 3], [35.0; 3], [12.0; 3], [28.0; 3]);
    let boxes = [
        ([0.0; 3], [40.0; 3]),
        ([17.0; 3], [23.0; 3]),
        ([5.0; 3], [35.0; 3]),
        ([12.0; 3], [28.0; 3]),
    ];
    let want = set_volume(&boxes, |p| {
        let ia = inb(p, [0.0; 3], [40.0; 3]) && !inb(p, [17.0; 3], [23.0; 3]);
        let ib = inb(p, [5.0; 3], [35.0; 3]) && !inb(p, [12.0; 3], [28.0; 3]);
        ia && !ib
    });
    for _ in 0..REPEAT / 2 {
        assert_volume(&k, k.boolean(BooleanOp::Subtract, a, b), want);
    }
}

/// 空洞の中に浮いた島を足す・引く。足すと 2 つの物になり体積は和(以前は「空洞を足し直せません」)、
/// 引くと変わらない
#[test]
fn an_island_floating_in_the_void() {
    let k = kernel();
    let a = hollow_a(&k);
    let island = cuboid(&k, [15.0; 3], [25.0; 3], "b");
    let base = 64000.0 - 8000.0;
    let u = k.boolean(BooleanOp::Union, a, island);
    assert_volume(&k, u.clone(), base + 1000.0);
    assert_volume(&k, k.boolean(BooleanOp::Union, island, a), base + 1000.0);
    assert_volume(&k, k.boolean(BooleanOp::Subtract, a, island), base);
    // 島と外側の殻を、壁を通る棒でつなぐと 1 つの物になる
    let rod = cuboid(&k, [19.0, 19.0, -5.0], [21.0, 21.0, 20.0], "r");
    let joined = k.boolean(BooleanOp::Union, u.unwrap(), rod).unwrap();
    assert_eq!(k.get(joined).unwrap().parts.len(), 1);
    assert_volume(&k, Ok(joined), base + 1000.0 + 4.0 * 5.0 + 4.0 * 5.0);
}

/// 空洞を広げる・外から差し込んだ棒で空洞の一部を埋める
#[test]
fn growing_and_filling_a_void() {
    let k = kernel();
    let a = hollow(&k, [0.0; 3], [40.0; 3], [15.0; 3], [25.0; 3]);
    let b = cuboid(&k, [10.0; 3], [30.0; 3], "b");
    assert_volume(&k, k.boolean(BooleanOp::Subtract, a, b), 64000.0 - 8000.0);
    let a = hollow_a(&k);
    let rod = cuboid(&k, [17.0, 17.0, -5.0], [23.0, 23.0, 20.0], "b");
    // A + 棒のうち A の外(z -5..0)と空洞の中(z 10..20)
    let want = 64000.0 - 8000.0 + 36.0 * 5.0 + 36.0 * 10.0;
    assert_volume(&k, k.boolean(BooleanOp::Union, a, rod), want);
}

/// STEP の空洞(BREP_WITH_VOIDS の内側の殻)は、殻の向きの印(ORIENTED_CLOSED_SHELL の .T./.F.)
/// によらず空洞として読む。以前は殻をどれも外向きにそろえたので、空洞が
/// 中身として足された(体積が 外 + 空洞 になった)。
#[test]
fn a_step_void_is_read_as_a_cavity_whichever_way_its_flag_points() {
    let k = TruckKernel::new();
    let h = hollow(&k, [0.0; 3], [40.0; 3], [10.0; 3], [30.0; 3]);
    let step = k.export_step(&[h]).unwrap();
    assert!(step.contains("BREP_WITH_VOIDS"), "空洞のある形として書く");
    let want = 40.0f64.powi(3) - 20.0f64.powi(3);
    let flags: Vec<&str> = step
        .lines()
        .filter(|l| l.contains("ORIENTED_CLOSED_SHELL"))
        .collect();
    assert_eq!(flags.len(), 1, "{flags:?}");
    // 書いたまま、印を逆にした物
    let flipped = if flags[0].contains(".T.") {
        step.replace(flags[0], &flags[0].replace(".T.", ".F."))
    } else {
        step.replace(flags[0], &flags[0].replace(".F.", ".T."))
    };
    for text in [&step, &flipped] {
        let i = k.import_step(text).unwrap();
        let v = k.mass(i).unwrap().volume;
        assert!((v - want).abs() < 1e-3 * want, "体積 {v}、{want} のはず");
    }
}

/// 先が 1 点にすぼまる円錐の STEP を、縁の書き方(底の円だけ・頂点の VERTEX_LOOP・継ぎ目の直線)に
/// よらず正しい形で読む。以前は truck の網が頂点の側を囲めず、体積が
/// 111 mm³(円だけ・VERTEX_LOOP)や 0(継ぎ目)になった
#[test]
fn a_cone_to_a_point_reads_whole_however_its_edges_are_written() {
    let k = TruckKernel::new();
    let want = 256.0 * std::f64::consts::PI;
    for file in ["cone-point", "cone-point-vertex", "cone-point-seam"] {
        let path = format!("{}/testdata/{file}.step", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(&path).unwrap();
        let h = k.import_step(&text).unwrap();
        let m = k.mass(h).unwrap();
        // 底の円の折れ線(許容差 0.01 mm)の分だけ小さい
        assert!(
            (m.volume - want).abs() < 5e-3 * want,
            "{file}: 体積 {}",
            m.volume
        );
        let side = std::f64::consts::PI * 8.0 * (8.0f64.powi(2) + 12.0f64.powi(2)).sqrt();
        let area = side + 64.0 * std::f64::consts::PI;
        assert!(
            (m.area - area).abs() < 5e-3 * area,
            "{file}: 表面積 {}",
            m.area
        );
        let (lo, hi) = k.bounds(h).unwrap();
        assert!(
            lo[2].abs() < 1e-6 && (hi[2] - 12.0).abs() < 1e-6,
            "{file}: {lo:?} {hi:?}"
        );
        // 網は閉じていて、外向き(体積が正)
        assert!(m.volume > 0.0);
    }
}

#[test]
fn the_volume_of_a_mesh_with_a_gap_does_not_depend_on_where_it_is() {
    // 網を作れなかった面(None)があっても、置いた場所で体積が変わらない。
    // 前は世界の原点のまわりで足したので、1 面欠けた 10 mm の立方体を 1000 mm 離すと値が大きくずれた
    let v = tm::builder::vertex(tm::Point3::new(0.0, 0.0, 0.0));
    let e = tm::builder::tsweep(&v, tm::Vector3::new(10.0, 0.0, 0.0));
    let f = tm::builder::tsweep(&e, tm::Vector3::new(0.0, 10.0, 0.0));
    let cube: tm::Solid = tm::builder::tsweep(&f, tm::Vector3::new(0.0, 0.0, 10.0));
    let gap = |solid: &tm::Solid| {
        let mut m = face_meshes(solid, 0.01);
        m[0] = None;
        signed_volume(&m)
    };
    let here = gap(&cube);
    let far = gap(&tm::builder::translated(
        &cube,
        tm::Vector3::new(1000.0, -700.0, 400.0),
    ));
    assert!((here - far).abs() < 1e-6, "{here} と {far}");
    // 閉じた網はこれまでどおり 1000 mm³
    let whole = signed_volume(&face_meshes(&cube, 0.01)).abs();
    assert!((whole - 1000.0).abs() < 1e-6, "{whole}");
}
