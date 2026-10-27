//! 確かめること: らせんの掃引が、名前付きの閉じた 1 立体と見込みの体積を作る。
use kmb_core::kernel::*;
use kmb_kernel_smlib::SmlibKernel;
use std::f64::consts::{PI, TAU};

#[test]
fn a_helical_sweep_is_a_named_closed_solid_with_the_expected_volume() {
    let kernel = SmlibKernel::new();
    let profile = Profile {
        plane: Plane::ZX,
        regions: vec![Region {
            outer: ProfileLoop { edges: vec![ProfileEdge {
                name: "circle".into(), segment: Segment::Circle { center: [0.0, 10.0], radius: 1.0 }, reversed: false,
            }] }, holes: vec![],
        }],
    };
    for left in [false, true] {
        let helix = Helix { origin: [0.0; 3], axis: [0.0, 0.0, 1.0], left,
            spans: vec![HelixSpan { turns: 2.25, pitch: 3.0 }], blend: 0.0, run_out: None };
        let shape = kernel.helical_sweep(&profile, &helix, "coil").unwrap();
        assert_eq!(kernel.check(shape).unwrap(), (true, 1, 0));
        let expected = PI * TAU * 10.0 * 2.25;
        let actual = kernel.volume(shape).unwrap();
        assert!((actual - expected).abs() < expected * 0.0001, "{actual} / {expected}");
        let names: Vec<_> = kernel.faces(shape).unwrap().into_iter().map(|f| f.name).collect();
        assert_eq!(names.len(), 5, "{names:?}");
        for name in ["coil:start", "coil:end", "coil:side:circle:0", "coil:side:circle:1", "coil:side:circle:2"] {
            assert!(names.iter().any(|n| n == name), "{names:?}");
        }
    }
}
