//! truck 版と結果が違うところを、SMLib 版の振る舞いとして確かめる。
//! (写した試験 kernel.rs・solid.rs のうち、truck の振る舞いを前提にして落ちるもの)
use kmb_contract::kernel::rect_loop;
use kmb_core::kernel::*;
use kmb_kernel_smlib::SmlibKernel;

fn plate(k: &SmlibKernel) -> ShapeHandle {
    let profile = Profile {
        plane: Plane::XY,
        regions: vec![Region {
            outer: rect_loop(0.0, 0.0, 60.0, 40.0, 1),
            holes: vec![ProfileLoop {
                edges: vec![ProfileEdge {
                    name: "e9".into(),
                    segment: Segment::Circle {
                        center: [30.0, 20.0],
                        radius: 4.0,
                    },
                    reversed: true,
                }],
            }],
        }],
    };
    k.extrude(&profile, 0.0, 10.0, "f1").unwrap()
}

fn pocket_tool(k: &SmlibKernel, top: f64) -> ShapeHandle {
    let profile = Profile {
        plane: Plane::XY,
        regions: vec![Region {
            outer: rect_loop(5.0, 5.0, 15.0, 10.0, 20),
            holes: vec![],
        }],
    };
    k.extrude(&profile, 5.0, top, "f2").unwrap()
}

/// truck は円を 2 つに割って `.a` `.b` と名付ける。SMLib は円の側面が 1 面なので、区間の名前のまま。
#[test]
fn a_circle_side_is_one_face() {
    let k = SmlibKernel::new();
    let h = plate(&k);
    let names: Vec<String> = k.faces(h).unwrap().into_iter().map(|f| f.name).collect();
    assert!(names.contains(&"f1:side:e9".to_string()), "{names:?}");
    assert_eq!(names.len(), 7, "{names:?}");
}

/// truck は上面とぴったり重なる切り取りに失敗する。SMLib は正しく切り取る。
#[test]
fn a_flush_cut_works() {
    let k = SmlibKernel::new();
    let p = plate(&k);
    let tool = pocket_tool(&k, 10.0);
    let cut = k.boolean(BooleanOp::Subtract, p, tool).unwrap();
    let v = k.mass(cut).unwrap().volume;
    let expect = 60.0 * 40.0 * 10.0 - std::f64::consts::PI * 160.0 - 15.0 * 10.0 * 5.0;
    assert!((v - expect).abs() < 1e-6 * expect, "{v} vs {expect}");
    let (solid, solids, voids) = k.check(cut).unwrap();
    assert!(solid && solids == 1 && voids == 0);
    let names: Vec<String> = k.faces(cut).unwrap().into_iter().map(|f| f.name).collect();
    assert!(names.contains(&"f2:start".to_string()), "{names:?}");
    assert!(!names.iter().any(|n| n == "unnamed"), "{names:?}");
}

/// truck の辺の一覧は、ブーリアンでできた辺(交線)を落とす。SMLib はポケットの縁の辺も返す。
#[test]
fn the_pocket_rim_is_a_convex_edge_of_the_top_face() {
    let k = SmlibKernel::new();
    let p = plate(&k);
    let tool = pocket_tool(&k, 10.01);
    let cut = k.boolean(BooleanOp::Subtract, p, tool).unwrap();
    let rim: Vec<EdgeInfo> = k
        .edges(cut)
        .unwrap()
        .into_iter()
        .filter(|e| e.faces.contains(&"f1:end".to_string()))
        .filter(|e| e.faces.iter().any(|f| f.starts_with("f2:")))
        .collect();
    assert_eq!(rim.len(), 4, "{rim:?}");
    assert!(rim.iter().all(|e| e.convex));
    // ポケットの底と壁の境目はへこんだ辺
    let floor: Vec<EdgeInfo> = k
        .edges(cut)
        .unwrap()
        .into_iter()
        .filter(|e| e.faces.contains(&"f2:start".to_string()))
        .collect();
    assert_eq!(floor.len(), 4, "{floor:?}");
    assert!(floor.iter().all(|e| !e.convex));
}

/// 長い板の小さな穴は、上の面が分かれても穴のまま。truck は穴を失う
/// (kmb-contract の説明)。
#[test]
fn a_small_hole_in_a_long_plate_stays_a_hole_when_its_face_is_divided() {
    kmb_contract::kernel::a_small_hole_in_a_long_plate_stays_a_hole_when_its_face_is_divided(
        &SmlibKernel::new(),
    );
}
