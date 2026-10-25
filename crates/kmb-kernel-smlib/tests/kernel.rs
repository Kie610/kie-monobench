//! SMLib のカーネルの試験。truck と共通の確かめは kmb-contract の契約(kernel_contract_tests!)で、
//! ここには SMLib だけで確かめるものを置く。truck と振る舞いが違う所
//! (ぴったり重なる切り取りは成功する など)は differences.rs で確かめる。
use kmb_core::kernel::*;
use kmb_kernel_smlib::*;
use kmb_kernel_truck::TruckKernel;

/// 組み立てと同じく、SMLib で立体に組めない STEP は truck の網で読む。
fn kernel() -> SmlibKernel {
    SmlibKernel::new().with_step_import(TruckKernel::new())
}

/// SMLib は円の側面を 1 面にする。
const QUIRKS: kmb_contract::kernel::KernelQuirks<SmlibKernel> = kmb_contract::kernel::KernelQuirks {
    shape_count: SmlibKernel::shape_count,
    hole_side_faces: &["f1:side:e9"],
    step_is_solid: true,
};

kmb_contract::kernel_contract_tests!(kernel, QUIRKS);

/// JC が作る形を STEP に書いて読み直す(truck は網で読むと崩れるので契約の一覧の外)。
#[test]
fn shapes_we_make_read_back_from_step() {
    kmb_contract::kernel::shapes_we_make_read_back_from_step(&kernel(), &QUIRKS);
}

use kmb_contract::kernel::{circle_loop, plate_profile, rect_loop};
use kmb_core::kernel::{Plane, ProfileEdge};

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs().max(1.0)
}

/// 円柱・穴の空いた板・リング・円環・円錐台を 1 回だけ作り、STEP に書き出して truck で読み直す。
/// - 読み直した体積と表面積が元と合う。円筒・円環・円錐を含む形で、面の向き(ADVANCED_FACE の
///   same_sense)が正しいことも確かめる(向きが逆なら体積の符号や値が狂う)
/// - 閉じた曲面の継ぎ目(縦の線)は、網の辺(画面に描く線)に入れない。曲面の作り方の都合でできる
///   線で、形の上に特別な所は無いため。STEP から読み直した形でも同じ
#[test]
fn step_round_trip_keeps_curved_faces_and_seams_are_not_drawn() {
    let k = kernel();
    let tau = std::f64::consts::TAU;
    let revolve = |outer: ProfileLoop, name: &str| {
        let profile = Profile {
            plane: Plane::ZX,
            regions: vec![Region {
                outer,
                holes: vec![],
            }],
        };
        k.revolve(&profile, [0.0; 3], [0.0, 0.0, 1.0], tau, name)
            .unwrap()
    };
    // 板に穴(円筒)
    let plate = k.extrude(&plate_profile(), 0.0, 10.0, "f1").unwrap();
    // 長方形を回したリング(平面と円筒)
    let ring = revolve(rect_loop(0.0, 10.0, 10.0, 5.0, 1), "f2");
    // 円を回したドーナツ(円環)
    let torus = revolve(circle_loop([5.0, 20.0], 3.0, "e1", false), "f3");
    // 台形を回した円錐台(頂点のある円錐は truck が読み違えるので、頂点のない形にする)
    let cone = revolve(
        ProfileLoop {
            edges: [[0.0, 0.0], [0.0, 8.0], [12.0, 3.0], [12.0, 0.0]]
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    let b = [[0.0, 8.0], [12.0, 3.0], [12.0, 0.0], [0.0, 0.0]][i];
                    ProfileEdge {
                        name: format!("e{}", i + 1),
                        segment: Segment::Line { a: *a, b },
                        reversed: false,
                    }
                })
                .collect(),
        },
        "f4",
    );
    let cylinder = k
        .extrude(
            &Profile {
                plane: Plane::XY,
                regions: vec![Region {
                    outer: circle_loop([0.0, 0.0], 5.0, "e1", false),
                    holes: vec![],
                }],
            },
            0.0,
            20.0,
            "f5",
        )
        .unwrap();
    let truck = TruckKernel::new();
    // (形, 体積と表面積を比べるか, 描く辺の数): 円柱は上下の円、板は外形の 12 本と穴の上下の円、
    // リングは上下・内外の円 4 本、円環は辺が無い
    for (what, h, mass, edges) in [
        ("板", plate, true, Some(14)),
        ("リング", ring, true, Some(4)),
        ("円環", torus, true, Some(0)),
        ("円錐台", cone, true, None),
        ("円柱", cylinder, false, Some(2)),
    ] {
        let step = k.export_step(&[h]).unwrap();
        assert!(step.starts_with("ISO-10303-21;"), "{what}");
        assert!(step.contains("MANIFOLD_SOLID_BREP"), "{what}");
        let back = truck
            .import_step(&step)
            .unwrap_or_else(|e| panic!("{what}: {e}"));
        if mass {
            let before = k.mass(h).unwrap();
            let after = truck.mass(back).unwrap();
            println!(
                "{what}: 体積 {} → {}、表面積 {} → {}",
                before.volume, after.volume, before.area, after.area
            );
            // truck は読み込んだ形を許容差 0.01 mm の網で測るので、千分の数の差は出る
            assert!(
                close(after.volume, before.volume, 5e-3),
                "{what}: 体積 {} 元 {}",
                after.volume,
                before.volume
            );
            assert!(
                close(after.area, before.area, 5e-3),
                "{what}: 表面積 {} 元 {}",
                after.area,
                before.area
            );
        }
        if let Some(count) = edges {
            let mesh = k.mesh(h, 0.05).unwrap();
            assert_eq!(mesh.edges.len(), count, "{what}: {:?}", mesh.edges);
            let mesh = truck.mesh(back, 0.05).unwrap();
            assert_eq!(mesh.edges.len(), count, "{what}(STEP)");
        }
    }
    if let Ok(dir) = std::env::var("JC_STEP_DUMP") {
        // 別の読み手で確かめるための書き出し
        for (file, h) in [
            ("plate", plate),
            ("ring", ring),
            ("torus", torus),
            ("cone", cone),
        ] {
            std::fs::write(format!("{dir}/{file}.step"), k.export_step(&[h]).unwrap()).unwrap();
        }
    }
}

#[test]
fn an_arc_ending_past_a_full_turn_is_extruded() {
    // 始まり 100°・中心角 310° の円弧(終わりが 410°)は、そのまま渡すと SmApiCreateArc が失敗する
    // (状態 1007)。重なった円を交点で分けた円弧もこうなる
    let d = std::f64::consts::PI / 180.0;
    let at = |a: f64| [10.0 * (a * d).cos(), 10.0 * (a * d).sin()];
    let p = Profile {
        plane: Plane::XY,
        regions: vec![Region {
            outer: ProfileLoop {
                edges: vec![
                    ProfileEdge {
                        name: "e1".into(),
                        segment: Segment::Arc {
                            center: [0.0, 0.0],
                            radius: 10.0,
                            start: 100.0 * d,
                            end: 410.0 * d,
                        },
                        reversed: false,
                    },
                    ProfileEdge {
                        name: "e2".into(),
                        segment: Segment::Line {
                            a: at(50.0),
                            b: at(100.0),
                        },
                        reversed: false,
                    },
                ],
            },
            holes: vec![],
        }],
    };
    let k = kernel();
    let h = k.extrude(&p, 0.0, 10.0, "f1").unwrap();
    // 扇形の残り(中心角 310°)と、弦で切った三角形の和
    let sector = 0.5 * 100.0 * 310.0 * d;
    let triangle = 0.5 * 100.0 * (50.0 * d).sin();
    let v = k.mass(h).unwrap().volume;
    assert!(close(v, (sector + triangle) * 10.0, 1e-3), "{v}");
}

/// 円柱の底面のスケッチで側面まで貫いて切り取ると、側面が 2 つの面に分かれ、元の継ぎ目の所が
/// 2 つの面の境の辺になる。同じ曲面の上の境で形の上に特別な所は無いので、継ぎ目と同じく描かない。
/// 切り口の角の縦の辺は描く。
#[test]
fn a_cut_from_the_bottom_of_a_cylinder_draws_no_line_where_its_side_was_split() {
    let k = kernel();
    let bottom = Plane::from_point_normal([0.0; 3], [0.0, 0.0, -1.0]);
    // 底面の座標(y が世界の -y)で、円柱の円に重なる円弧と弦で囲んだ弓形。角度は世界の向き
    let bow = |a: f64, b: f64| {
        let d = std::f64::consts::PI / 180.0;
        let at = |t: f64| [5.0 * (t * d).cos(), -5.0 * (t * d).sin()];
        ProfileLoop {
            edges: vec![
                ProfileEdge {
                    name: "e1".into(),
                    segment: Segment::Arc {
                        center: [0.0, 0.0],
                        radius: 5.0,
                        start: -b * d,
                        end: -a * d,
                    },
                    reversed: false,
                },
                ProfileEdge {
                    name: "e2".into(),
                    segment: Segment::Line { a: at(a), b: at(b) },
                    reversed: false,
                },
            ],
        }
    };
    // (輪郭, 切り口の角の縦の辺の数): 中央の帯で 2 つに切ると角が 4 本、弓形は 2 本
    for (what, outer, corners) in [
        ("中央の帯", rect_loop(-2.0, -8.0, 4.0, 16.0, 1), 4),
        ("継ぎ目を残す弓形", bow(30.0, 150.0), 2),
    ] {
        let cylinder = k
            .extrude(
                &Profile {
                    plane: Plane::XY,
                    regions: vec![Region {
                        outer: circle_loop([0.0, 0.0], 5.0, "e1", false),
                        holes: vec![],
                    }],
                },
                0.0,
                20.0,
                "f1",
            )
            .unwrap();
        // 範囲「すべて」と同じく、底面の後ろから上面の先まで
        let tool = k
            .extrude(
                &Profile {
                    plane: bottom,
                    regions: vec![Region {
                        outer,
                        holes: vec![],
                    }],
                },
                -21.0,
                1.0,
                "f2",
            )
            .unwrap();
        let cut = k.boolean(BooleanOp::Subtract, cylinder, tool).unwrap();
        let mesh = k.mesh(cut, 0.05).unwrap();
        // 側面の上の縦の辺
        let vertical: Vec<_> = mesh
            .edges
            .iter()
            .filter(|e| {
                let on_side = e
                    .iter()
                    .all(|p| ((p[0] * p[0] + p[1] * p[1]).sqrt() - 5.0).abs() < 1e-3);
                let (lo, hi) = e
                    .iter()
                    .fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), p| {
                        (a.min(p[2]), b.max(p[2]))
                    });
                on_side && hi - lo > 1e-3
            })
            .map(|e| e[0])
            .collect();
        assert_eq!(vertical.len(), corners, "{what}: {vertical:?}");
        assert!(
            vertical
                .iter()
                .all(|p| (p[0] - 5.0).abs() > 1e-3 || p[1].abs() > 1e-3),
            "{what}: 元の継ぎ目の所に線がある {vertical:?}"
        );
    }
}

/// 傾けた押し出し: 外周は先へ行くほど細く、穴は太くなる。向き(表・裏)と面の名前も。
#[test]
fn taper_extrude_shrinks_the_outer_wall_and_widens_holes() {
    let k = kernel();
    let tan = 10f64.to_radians().tan();
    let sq = Profile {
        plane: Plane::XY,
        regions: vec![Region {
            outer: rect_loop(0.0, 0.0, 10.0, 10.0, 1),
            holes: vec![],
        }],
    };
    // 角すい台: 下 10×10、上 (10 − 2·10·tan)²、高さ 10
    let frustum = |h: f64, a: f64, b: f64| h / 3.0 * (a * a + b * b + a * b);
    let top = 10.0 - 2.0 * 10.0 * tan;
    let up = k
        .extrude_taper(&sq, 0.0, 10.0, 10f64.to_radians(), "f1")
        .unwrap();
    let v = k.mass(up).unwrap().volume;
    assert!(close(v, frustum(10.0, 10.0, top), 1e-3), "{v}");
    let (lo, hi) = k.bounds(up).unwrap();
    assert!(lo[2] > -1e-3 && hi[2] < 10.0 + 1e-3, "{lo:?} {hi:?}");
    let names: Vec<String> = k.faces(up).unwrap().into_iter().map(|f| f.name).collect();
    for n in ["f1:start", "f1:end", "f1:side:e1", "f1:side:e3"] {
        assert!(names.iter().any(|x| x == n), "{n} {names:?}");
    }
    // 裏へ: 同じ形が z < 0 に。負の角度は太くなる
    let down = k
        .extrude_taper(&sq, -10.0, 0.0, (-10f64).to_radians(), "f2")
        .unwrap();
    let wide = 10.0 + 2.0 * 10.0 * tan;
    let v = k.mass(down).unwrap().volume;
    assert!(
        close(v, frustum(10.0, 10.0, wide), 1e-3),
        "{v} {}",
        frustum(10.0, 10.0, wide)
    );
    let (lo, hi) = k.bounds(down).unwrap();
    assert!(hi[2] < 1e-3 && lo[2] > -10.0 - 1e-3, "{lo:?} {hi:?}");
    // 両側: 表 5 と裏 5 を足した形
    let both = k
        .extrude_taper(&sq, -5.0, 5.0, 10f64.to_radians(), "f3")
        .unwrap();
    let half = frustum(5.0, 10.0, 10.0 - 2.0 * 5.0 * tan);
    assert!(close(k.mass(both).unwrap().volume, 2.0 * half, 1e-3));
    // 穴のある板: 穴の円すいは上へ行くほど太る(外周が細るのと逆)
    let plate = Profile {
        plane: Plane::XY,
        regions: vec![Region {
            outer: rect_loop(-10.0, -10.0, 20.0, 20.0, 1),
            holes: vec![circle_loop([0.0, 0.0], 3.0, "e9", true)],
        }],
    };
    let p = k
        .extrude_taper(&plate, 0.0, 5.0, 5f64.to_radians(), "f4")
        .unwrap();
    let t5 = 5f64.to_radians().tan();
    let outer = frustum(5.0, 20.0, 20.0 - 2.0 * 5.0 * t5);
    let r1 = 3.0 + 5.0 * t5;
    let hole = std::f64::consts::PI * 5.0 / 3.0 * (9.0 + r1 * r1 + 3.0 * r1);
    let v = k.mass(p).unwrap().volume;
    assert!(close(v, outer - hole, 2e-3), "{v} {}", outer - hole);
}

/// 面のオフセット(M29)。板と、板の上の円柱(半径 5、高さ 6)を 1 回だけ作る。
/// - 穴の壁を内へ動かすと穴が広がり、動かした面は元の名前のまま
/// - 円柱の側面を外へ動かすと太くなる
/// - 内へ動かした後、同じ面をもう一度外へ動かせる(実際のアプリの点検で落ちた形)
#[test]
fn offset_faces_widens_a_hole_and_thickens_a_boss_even_twice() {
    let k = kernel();
    let plate = k.extrude(&plate_profile(), 0.0, 10.0, "f1").unwrap();
    let v0 = k.mass(plate).unwrap().volume;
    let wall = "f1:side:e9".to_string();
    let wider = k
        .offset_faces(plate, std::slice::from_ref(&wall), -1.0, "f2")
        .unwrap();
    let v1 = k.mass(wider).unwrap().volume;
    let want = std::f64::consts::PI * (25.0 - 16.0) * 10.0;
    assert!(
        close(v0 - v1, want, 1e-3),
        "穴が広がる: {} (欲しい {want})",
        v0 - v1
    );
    let names: Vec<String> = k
        .faces(wider)
        .unwrap()
        .into_iter()
        .map(|f| f.name)
        .collect();
    assert!(names.contains(&wall), "動かした面は元の名前: {names:?}");

    // 板の上の円柱(半径 5、高さ 6)の側面を外へ 1
    let boss = Profile {
        plane: Plane::XY.offset(10.0),
        regions: vec![Region {
            outer: circle_loop([10.0, 10.0], 5.0, "c1", false),
            holes: vec![],
        }],
    };
    let b = k.extrude(&boss, 0.0, 6.0, "f3").unwrap();
    let body = k.boolean(BooleanOp::Union, plate, b).unwrap();
    let side = k
        .faces(body)
        .unwrap()
        .into_iter()
        .find(|f| f.name.starts_with("f3:side"))
        .unwrap()
        .name;
    let vb = k.mass(body).unwrap().volume;
    let fat = k
        .offset_faces(body, std::slice::from_ref(&side), 1.0, "f4")
        .unwrap();
    let want = std::f64::consts::PI * (36.0 - 25.0) * 6.0;
    let got = k.mass(fat).unwrap().volume - vb;
    assert!(close(got, want, 1e-3), "円柱が太る: {got} (欲しい {want})");

    // 内へ 1 動かした後、同じ面を外へ 2
    let thin = k
        .offset_faces(body, std::slice::from_ref(&side), -1.0, "f4")
        .unwrap();
    let names: Vec<String> = k.faces(thin).unwrap().into_iter().map(|f| f.name).collect();
    eprintln!("1 回目の後の面: {names:?}");
    let vt = k.mass(thin).unwrap().volume;
    let fat = k.offset_faces(thin, &[side], 2.0, "f5").unwrap();
    let want = std::f64::consts::PI * (36.0 - 16.0) * 6.0;
    let got = k.mass(fat).unwrap().volume - vt;
    assert!(close(got, want, 1e-3), "{got} (欲しい {want})");
}

/// 30×20×10 の箱の、縦の 1 辺(x = 30、y = 20)を半径 `r` で丸めた形。
fn box_with_round_corner(k: &SmlibKernel, r: f64) -> ShapeHandle {
    let p = Profile {
        plane: Plane::XY,
        regions: vec![Region {
            outer: rect_loop(0.0, 0.0, 30.0, 20.0, 1),
            holes: vec![],
        }],
    };
    let b = k.extrude(&p, 0.0, 10.0, "f1").unwrap();
    let e = k
        .edges(b)
        .unwrap()
        .into_iter()
        .find(|e| {
            let m = [(e.a[0] + e.b[0]) / 2.0, (e.a[1] + e.b[1]) / 2.0];
            (m[0] - 30.0).abs() < 1e-6 && (m[1] - 20.0).abs() < 1e-6
        })
        .expect("縦の辺");
    k.blend_edges(
        b,
        &[EdgeBlend::new(e.faces, [30.0, 20.0, 5.0], r, false)],
        "f2",
    )
    .unwrap()
}

/// 閉じた空洞(開ける面の無いシェル)のある箱を、空洞に触れない所で切り取る・交わらせる。
/// 空洞の内向きの殻を中身と読んで結果が空にならないことを確かめる(契約の試験は引き算で作った空洞、ここはシェルで作った空洞)。
#[test]
fn cut_beside_a_closed_shell_void_keeps_the_body() {
    let k = kernel();
    let p = Profile {
        plane: Plane::XY,
        regions: vec![Region {
            outer: rect_loop(0.0, 0.0, 40.0, 40.0, 1),
            holes: vec![],
        }],
    };
    let solid = k.extrude(&p, 0.0, 20.0, "f1").unwrap();
    let hollow = k.shell(solid, &[], 2.0, "f2").unwrap();
    let v0 = 40.0 * 40.0 * 20.0 - 36.0 * 36.0 * 16.0;
    assert!(close(k.mass(hollow).unwrap().volume, v0, 1e-6));
    // 外の壁だけを 1 mm 削る(空洞は x 2〜38)
    let tool = Profile {
        plane: Plane::XY,
        regions: vec![Region {
            outer: rect_loop(39.0, -5.0, 10.0, 50.0, 11),
            holes: vec![],
        }],
    };
    let t = k.extrude(&tool, -5.0, 25.0, "f3").unwrap();
    let cut = k.boolean(BooleanOp::Subtract, hollow, t).unwrap();
    let want = v0 - 1.0 * 40.0 * 20.0;
    let got = k.mass(cut).unwrap().volume;
    assert!(close(got, want, 1e-6), "切り取り: {got} vs {want}");
    let common = k.boolean(BooleanOp::Intersect, hollow, t).unwrap();
    let got = k.mass(common).unwrap().volume;
    assert!(close(got, 800.0, 1e-6), "交差: {got} vs 800");
}

/// 角を丸めた箱を、丸めの半径より厚いシェルにする(オフセットで弧が反対回りになる形: 天面の弧の辺は両端で側面に接し、内へ半径より多く動かすと弧の両端が中点を越える)。
/// 内側の角はとがり(丸めは厚みに飲み込まれる)、体積が合うか、理由つきで止まること。
/// 半径より薄いシェルは内側にも丸めが残る(比べるための正しい形)。
#[test]
fn shell_thicker_than_a_round_corner_does_not_turn_the_arc_round() {
    let k = kernel();
    let r = 1.0;
    let corner = (1.0 - std::f64::consts::FRAC_PI_4) * r * r;
    let outer = 30.0 * 20.0 * 10.0 - corner * 10.0;
    let b = box_with_round_corner(&k, r);
    assert!(close(k.mass(b).unwrap().volume, outer, 1e-6));
    // 天面(f1:end)を開ける。薄い(0.5): 内側の丸めは半径 0.5
    let thin = k.shell(b, &["f1:end".to_string()], 0.5, "f3").unwrap();
    let inner = 29.0 * 19.0 * 9.5 - (1.0 - std::f64::consts::FRAC_PI_4) * 0.25 * 9.5;
    let got = k.mass(thin).unwrap().volume;
    assert!(
        close(got, outer - inner, 1e-5),
        "薄い: {got} vs {}",
        outer - inner
    );
    // 厚い(2): 内側の角はとがる
    let inner = 26.0 * 16.0 * 8.0;
    match k.shell(b, &["f1:end".to_string()], 2.0, "f4") {
        Ok(thick) => {
            let got = k.mass(thick).unwrap().volume;
            assert!(
                close(got, outer - inner, 1e-5),
                "厚い: {got} vs {}",
                outer - inner
            );
        }
        Err(e) => eprintln!("厚いシェルは止まった: {e}"),
    }
}

/// 丸めや円柱の半径以上の厚みのシェルは、SMLib が理由を返さずに失敗する(状態 1001)。
/// 失敗の文に、考えられる理由(厚みと半径)が出ること(状態の番号だけでは直し方が分からない)。SMLib が作れるようになったら、作れた形の体積を確かめる試験に替える
#[test]
fn shell_thicker_than_a_radius_says_why_it_failed() {
    let k = kernel();
    let open = ["f1:end".to_string()];
    let round = box_with_round_corner(&k, 1.0);
    let cylinder = {
        let p = Profile {
            plane: Plane::XY,
            regions: vec![Region {
                outer: circle_loop([0.0, 0.0], 3.0, "e1", false),
                holes: vec![],
            }],
        };
        k.extrude(&p, 0.0, 10.0, "f1").unwrap()
    };
    for (shape, t) in [(round, 1.5), (cylinder, 3.1)] {
        let e = k.shell(shape, &open, t, "f3").unwrap_err().to_string();
        assert!(e.contains("シェルを作れませんでした"), "{e}");
        assert!(e.contains("丸めや曲面の半径"), "{e}");
        assert!(!e.contains("シェル: シェル"), "{e}");
    }
}

/// 丸めの面そのものを、半径より多く内へオフセットする(円筒の面が裏返る)。
/// 裏返った形を作らず、理由つきで止まるか、丸めが消えた正しい形になること。
#[test]
fn offsetting_a_round_face_past_its_radius_is_refused_or_right() {
    let k = kernel();
    let b = box_with_round_corner(&k, 1.0);
    let round = k
        .faces(b)
        .unwrap()
        .into_iter()
        .find(|f| f.name.starts_with("f2:"))
        .expect("丸めの面")
        .name;
    let v = k.mass(b).unwrap().volume;
    match k.offset_faces(b, std::slice::from_ref(&round), -2.0, "f3") {
        Ok(h) => {
            let got = k.mass(h).unwrap().volume;
            // 円筒が内へ 2 動くと、角の削れは半径 1 の丸めより大きい。体積は減り、正でなければならない
            assert!(got > 0.0 && got < v, "{got} vs {v}");
            eprintln!("丸めの面のオフセット: {v} → {got}");
        }
        Err(e) => eprintln!("丸めの面のオフセットは止まった: {e}"),
    }
}

/// 上の角を丸めた棒(上の平面と端の円筒がなめらかにつながる)に、壁が継ぎ目の上に乗る溝を切る。
/// 溝の上の縁(壁と円筒の境目)から接してつながる辺をたどっても、縁の両側へまっすぐ続く継ぎ目へは
/// 進まない(継ぎ目には丸める角が無い)。カーネルは継ぎ目を [`Seam::Smooth`]、
/// 角のある縁と残りの辺を [`Seam::Corner`] と答える(たどり自体の試験は kmb-cad の chain.rs)。
#[test]
fn the_tangent_chain_does_not_run_on_into_the_seams_beside_a_slot() {
    use std::f64::consts::PI;
    let k = kernel();
    let edge = |name: &str, segment| ProfileEdge {
        name: name.into(),
        segment,
        reversed: false,
    };
    let bar = Profile {
        plane: Plane::XY,
        regions: vec![Region {
            outer: ProfileLoop {
                edges: vec![
                    edge(
                        "e1",
                        Segment::Line {
                            a: [0.0, 0.0],
                            b: [40.0, 0.0],
                        },
                    ),
                    edge(
                        "e2",
                        Segment::Line {
                            a: [40.0, 0.0],
                            b: [40.0, 6.0],
                        },
                    ),
                    edge(
                        "e3",
                        Segment::Arc {
                            center: [36.0, 6.0],
                            radius: 4.0,
                            start: 0.0,
                            end: PI / 2.0,
                        },
                    ),
                    edge(
                        "e4",
                        Segment::Line {
                            a: [36.0, 10.0],
                            b: [4.0, 10.0],
                        },
                    ),
                    edge(
                        "e5",
                        Segment::Arc {
                            center: [4.0, 6.0],
                            radius: 4.0,
                            start: PI / 2.0,
                            end: PI,
                        },
                    ),
                    edge(
                        "e6",
                        Segment::Line {
                            a: [0.0, 6.0],
                            b: [0.0, 0.0],
                        },
                    ),
                ],
            },
            holes: vec![],
        }],
    };
    let bar = k.extrude(&bar, 0.0, 20.0, "b").unwrap();
    // 溝の壁 x = 4 は、上の平面(y = 10)と左の円筒の継ぎ目の上
    let slot = Profile {
        plane: Plane::XY,
        regions: vec![Region {
            outer: rect_loop(4.0, 7.0, 8.0, 4.0, 1),
            holes: vec![],
        }],
    };
    let slot = k.extrude(&slot, 5.0, 15.0, "s").unwrap();
    let r = k.boolean(BooleanOp::Subtract, bar, slot).unwrap();
    let mut all = k.edges(r).unwrap();
    all.extend(k.arc_edges(r).unwrap());
    all.extend(k.curve_edges(r).unwrap());
    let on_seam_line = |e: &EdgeInfo| {
        [e.a, e.b]
            .iter()
            .all(|p| (p[0] - 4.0).abs() < 1e-6 && (p[1] - 10.0).abs() < 1e-6)
    };
    let line: Vec<usize> = (0..all.len()).filter(|&i| on_seam_line(&all[i])).collect();
    // 溝の縁 1 本と、その上下の継ぎ目 2 本
    assert_eq!(
        line.len(),
        3,
        "{:?}",
        line.iter().map(|&i| &all[i]).collect::<Vec<_>>()
    );
    let rim = *line
        .iter()
        .find(|&&i| all[i].a[2].min(all[i].b[2]) > 4.9 && all[i].a[2].max(all[i].b[2]) < 15.1)
        .unwrap();
    for &i in &line {
        let want = if i == rim { Seam::Corner } else { Seam::Smooth };
        assert_eq!(all[i].seam, want, "{:?}", all[i]);
    }
    // どの辺も、角か継ぎ目かが分かっている
    assert!(
        all.iter().all(|e| e.seam != Seam::Unknown),
        "{:?}",
        all.iter()
            .filter(|e| e.seam == Seam::Unknown)
            .collect::<Vec<_>>()
    );
    // 継ぎ目の辺は、弧と隣の辺の継ぎ目(右の 2 本、左の 1 本、溝で 2 つに分かれた 1 本)だけ
    let smooth: Vec<&[String; 2]> = all
        .iter()
        .filter(|e| e.seam == Seam::Smooth)
        .map(|e| &e.faces)
        .collect();
    assert_eq!(smooth.len(), 5, "{smooth:?}");
    assert!(
        smooth
            .iter()
            .all(|f| f.iter().all(|n| n.starts_with("b:side:"))
                && f.iter().any(|n| n.ends_with("e3") || n.ends_with("e5"))),
        "{smooth:?}"
    );
}

/// 1 周する円筒・円錐の面の輪(輪・継ぎ目・輪・継ぎ目)は、継ぎ目を初めて使った後の輪が、書いた軸の
/// まわりに角度の増える向き(反時計回り)へ回る。読み手は継ぎ目の初めの
/// 使いを角度 0 の側に置いて次の輪を広げるので、逆だとほかの CAD で側面が欠ける。
/// 形は板に、2 度の抜きの付いた脚 4 本と、まっすぐなピン
#[test]
fn seam_loops_turn_the_way_the_angle_grows() {
    let k = kernel();
    let disc = |c: [f64; 2], r: f64| Profile {
        plane: Plane::XY,
        regions: vec![Region {
            outer: circle_loop(c, r, "e1", false),
            holes: vec![],
        }],
    };
    let plate = Profile {
        plane: Plane::XY,
        regions: vec![Region {
            outer: rect_loop(-60.0, -40.0, 120.0, 80.0, 1),
            holes: vec![],
        }],
    };
    let mut b = k.extrude(&plate, 0.0, 10.0, "f1").unwrap();
    for (i, c) in [[-45.0, -30.0], [45.0, -30.0], [45.0, 30.0], [-45.0, 30.0]]
        .iter()
        .enumerate()
    {
        let leg = k
            .extrude_taper(
                &disc(*c, 5.0),
                -20.0,
                0.0,
                2f64.to_radians(),
                &format!("f{}", i + 2),
            )
            .unwrap();
        b = k.boolean(BooleanOp::Union, b, leg).unwrap();
    }
    let peg = k.extrude(&disc([0.0, 0.0], 4.0), 10.0, 18.0, "f9").unwrap();
    b = k.boolean(BooleanOp::Union, b, peg).unwrap();
    let text = k.export_step(&[b]).unwrap();
    let records = step_records(&text);
    let refs = |s: &str| -> Vec<u64> {
        s.split('#')
            .skip(1)
            .filter_map(|x| {
                let d: String = x.chars().take_while(char::is_ascii_digit).collect();
                d.parse().ok()
            })
            .collect()
    };
    let kind = |i: u64| records[&i].split('(').next().unwrap().trim().to_string();
    let numbers = |i: u64| -> [f64; 3] {
        let s = &records[&i];
        let inner = &s[s.rfind('(').unwrap() + 1..s.find(')').unwrap()];
        let v: Vec<f64> = inner
            .split(',')
            .map(|x| x.trim().parse().unwrap())
            .collect();
        [v[0], v[1], v[2]]
    };
    let flag = |i: u64| records[&i].trim_end().ends_with(".T.)");
    let mut seamed = 0;
    for (id, face) in &records {
        if kind(*id) != "ADVANCED_FACE" {
            continue;
        }
        let r = refs(face);
        let (bounds, surface) = r.split_at(r.len() - 1);
        if !matches!(
            kind(surface[0]).as_str(),
            "CONICAL_SURFACE" | "CYLINDRICAL_SURFACE"
        ) {
            continue;
        }
        let axis = refs(&records[&refs(&records[&surface[0]])[0]]);
        let z = numbers(axis[1]);
        for bound in bounds {
            let lp = refs(&records[bound])[0];
            let used: Vec<(u64, bool)> = refs(&records[&lp])
                .into_iter()
                .map(|oe| (refs(&records[&oe])[0], flag(oe)))
                .collect();
            let twice = |e: u64| used.iter().filter(|x| x.0 == e).count() == 2;
            let Some(i) = used.iter().position(|x| twice(x.0)) else {
                continue;
            };
            let (edge, fwd) = used[(i + 1) % used.len()];
            let er = refs(&records[&edge]);
            let curve = er[2];
            // 辺と使いの向きで、曲線をたどる向きが決まる
            let along = flag(edge) == fwd;
            let turn = if kind(curve) == "CIRCLE" {
                // 円は自分の軸のまわりに反時計回り
                let circle_z = numbers(refs(&records[&refs(&records[&curve])[0]])[1]);
                dot(circle_z, z) * if along { 1.0 } else { -1.0 }
            } else {
                // B スプライン(脚と板の交わり): たどり始めの 2 つの制御点が軸のまわりに回る向き
                let o = numbers(axis[0]);
                let mut poles: Vec<[f64; 3]> =
                    refs(&records[&curve]).into_iter().map(numbers).collect();
                if !along {
                    poles.reverse();
                }
                let radial = |q: [f64; 3]| {
                    let v = sub(q, o);
                    sub(v, scale(z, dot(v, z)))
                };
                dot(cross(radial(poles[0]), radial(poles[1])), z)
            };
            assert!(turn > 0.0, "面 #{id}: 継ぎ目の後の輪が角度と逆に回る");
            seamed += 1;
        }
    }
    assert_eq!(seamed, 5, "脚 4 本とピンの側面");
    // 並べ替えても読み直した形は同じ
    let truck = TruckKernel::new();
    let back = truck.import_step(&text).unwrap();
    let (before, after) = (k.mass(b).unwrap().volume, truck.mass(back).unwrap().volume);
    assert!(close(after, before, 5e-3), "体積 {after} 元 {before}");
}

/// STEP の DATA 節の件(番号 → 中身)。
fn step_records(text: &str) -> std::collections::BTreeMap<u64, String> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            let (id, body) = l.strip_prefix('#')?.split_once('=')?;
            Some((
                id.trim().parse().ok()?,
                body.trim_end_matches(';').to_string(),
            ))
        })
        .collect()
}
