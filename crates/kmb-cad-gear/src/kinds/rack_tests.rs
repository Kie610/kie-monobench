//! 確かめること: 円ピッチと長さ、歯厚と歯面の傾き、閉じた輪郭、丸みの接続、入力の誤り。

use super::*;
use kmb_core::ProfileLoop;

fn params() -> Params {
    Params {
        module: 2.0,
        teeth: 12,
        root_round: 0.0,
        ..Params::default()
    }
}

#[test]
fn length_contains_only_complete_teeth_and_keeps_the_remaining_back() {
    let pitch = 2.0 * PI;
    for n in [1, 12, 20, 30] {
        assert_eq!(teeth_for_length(2.0, n as f64 * pitch).unwrap(), n);
        assert_eq!(teeth_for_length(2.0, (n as f64 + 0.75) * pitch).unwrap(), n);
    }
    let length = pitch * 12.75;
    let o = outline(&Params {
        rack_length: Some(length),
        ..params()
    })
    .unwrap();
    assert_eq!(
        o.edges.iter().filter(|e| e.name.ends_with(":tip")).count(),
        12
    );
    assert_eq!(o.reference_lines[0].to_point(), [length, 0.0]);
    assert!(teeth_for_length(2.0, pitch * 0.99).is_err());
    assert!(teeth_for_length(0.0, 12.0).is_err());
    assert!(teeth_for_length(2.0, f64::INFINITY).is_err());
}

#[test]
fn pitch_thickness_and_straight_flanks_follow_the_pressure_angle() {
    for degrees in [15.0f64, 20.0, 25.0] {
        let p = Params {
            pressure_angle: degrees.to_radians(),
            ..params()
        };
        let o = outline(&p).unwrap();
        let mut crosses = Vec::new();
        for name in ["t0:l0", "t0:r0"] {
            let e = o.edges.iter().find(|e| e.name == name).unwrap();
            let Segment::Line { a, b } = e.segment else {
                panic!("直線")
            };
            assert!(
                ((b[0] - a[0]).abs() / (b[1] - a[1]).abs() - p.pressure_angle.tan()).abs() < 1e-12
            );
            crosses.push(a[0] - a[1] * (b[0] - a[0]) / (b[1] - a[1]));
        }
        assert!(((crosses[1] - crosses[0]).abs() - PI * p.module / 2.0).abs() < 1e-12);
        let ys: Vec<_> = o
            .edges
            .iter()
            .flat_map(|e| [e.from_point()[1], e.to_point()[1]])
            .collect();
        assert!((ys.iter().copied().fold(f64::NEG_INFINITY, f64::max) - p.module).abs() < 1e-12);
        assert!((ys.iter().copied().fold(f64::INFINITY, f64::min) + 3.25 * p.module).abs() < 1e-12);
    }
}

#[test]
fn rounded_racks_close_without_crossings_and_have_unique_names() {
    for (teeth, angle) in [(12, 15.0f64), (20, 20.0), (30, 25.0)] {
        let p = Params {
            teeth,
            pressure_angle: angle.to_radians(),
            tip_round: 0.2,
            root_round: 0.38,
            ..params()
        };
        let o = outline(&p).unwrap();
        let n = o.edges.len();
        for i in 0..n {
            let a = o.edges[i].to_point();
            let b = o.edges[(i + 1) % n].from_point();
            assert!(
                (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-9,
                "{} -> {}",
                o.edges[i].name,
                o.edges[(i + 1) % n].name
            );
        }
        let names: std::collections::BTreeSet<_> = o.edges.iter().map(|e| &e.name).collect();
        assert_eq!(names.len(), n);
        let ring = ProfileLoop { edges: o.edges };
        assert!(ring.signed_area() > 0.0);
        let v = ring.polyline();
        let orient =
            |a: P2, b: P2, c: P2| (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        for i in 0..v.len() {
            for j in i + 2..v.len() {
                if i == 0 && j == v.len() - 1 {
                    continue;
                }
                let (a, b, c, d) = (v[i], v[(i + 1) % v.len()], v[j], v[(j + 1) % v.len()]);
                assert!(
                    !(orient(a, b, c) * orient(a, b, d) < -1e-18
                        && orient(c, d, a) * orient(c, d, b) < -1e-18),
                    "交差 {i} {j}"
                );
            }
        }
    }
}

#[test]
fn sharp_rack_area_matches_the_trapezoids_and_back() {
    let p = params();
    let o = outline(&p).unwrap();
    let area = ProfileLoop { edges: o.edges }.signed_area();
    let pitch = PI * p.module;
    let root_width = pitch / 2.0 + 2.5 * p.module * p.pressure_angle.tan();
    let tip_width = pitch / 2.0 - 2.0 * p.module * p.pressure_angle.tan();
    let want = pitch * p.teeth as f64 * 2.0 * p.module
        + (root_width + tip_width) / 2.0 * 2.25 * p.module * p.teeth as f64;
    assert!((area - want).abs() < 1e-8, "{area} {want}");
}

#[test]
fn invalid_back_rounds_and_overlapping_teeth_are_errors() {
    for p in [
        Params {
            rack_back: Some(0.0),
            ..params()
        },
        Params {
            root_round: -1.0,
            ..params()
        },
        Params {
            tip_round: f64::NAN,
            ..params()
        },
        Params {
            pressure_angle: 40f64.to_radians(),
            ..params()
        },
        Params {
            profile_shift: 0.5,
            ..params()
        },
    ] {
        assert!(outline(&p).is_err(), "{p:?}");
    }
}

#[test]
fn finite_inputs_with_overflowing_dimensions_are_refused() {
    for p in [
        Params {
            root_round: f64::MAX,
            ..params()
        },
        Params {
            module: f64::MAX,
            ..params()
        },
        Params {
            module: 1e307,
            teeth: 1,
            rack_back: Some(f64::MAX),
            ..params()
        },
    ] {
        assert!(outline(&p).is_err(), "{p:?}");
    }
}

#[test]
fn length_mode_keeps_the_same_thousand_tooth_limit_as_count_expressions() {
    let pitch = std::f64::consts::PI * 0.001;
    assert_eq!(teeth_for_length(0.001, pitch * 1000.9).unwrap(), 1000);
    assert!(
        teeth_for_length(0.001, pitch * 1001.0)
            .unwrap_err()
            .contains("1000")
    );
    assert!(teeth_for_length(0.001, 1e6).is_err());
    assert!(
        outline(&Params {
            teeth: 1001,
            ..params()
        })
        .is_err()
    );
}
