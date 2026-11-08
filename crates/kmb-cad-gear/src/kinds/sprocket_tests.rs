//! 確かめること: 鎖の呼びの寸法、歯先と座の寸法、接線、閉じた交差の無い輪、入力の拒否。

use super::*;
use kmb_core::{ProfileLoop, Segment};

#[test]
fn chain_sizes_and_widths_follow_the_dimension_table() {
    for (size, want) in [
        (ChainSize::Size25, (6.35, 3.30, 3.18)),
        (ChainSize::Size35, (9.525, 5.08, 4.77)),
        (ChainSize::Size40, (12.7, 7.92, 7.95)),
        (ChainSize::Size50, (15.875, 10.16, 9.53)),
        (ChainSize::Size60, (19.05, 11.91, 12.70)),
        (ChainSize::Size80, (25.40, 15.88, 15.88)),
    ] {
        let c = size.dimensions().unwrap();
        assert_eq!((c.pitch, c.roller, c.inner_width), want);
        assert!(
            (c.default_width() - c.inner_width * if c.pitch <= 12.7 { 0.93 } else { 0.95 }).abs()
                < 1e-12
        );
    }
    assert!(ChainSize::Manual.dimensions().is_none());
}

#[test]
fn seventeen_teeth_have_the_expected_pitch_diameter_and_standard_midpoints() {
    let c = ChainSize::Size40.dimensions().unwrap();
    let d = dimensions(c, 17).unwrap();
    assert!((2.0 * d.pitch_radius - 69.12).abs() < 0.01);
    assert!((2.0 * d.root_radius - (2.0 * d.pitch_radius - c.roller)).abs() < 1e-12);
    assert!((d.seat_angle.to_degrees() - (130.0 - 90.0 / 17.0)).abs() < 1e-12);
    assert!((d.seat_radius - (0.505 * c.roller + 0.0345 * c.roller.cbrt())).abs() < 1e-12);
    let minimum = 2.0 * d.pitch_radius + c.pitch * (1.0 - 1.6 / 17.0) - c.roller;
    let maximum = 2.0 * d.pitch_radius + 1.25 * c.pitch - c.roller;
    assert!((2.0 * d.tip_radius - (minimum + maximum) / 2.0).abs() < 1e-12);
}

#[test]
fn seats_and_flanks_are_tangent_and_the_rings_are_simple() {
    for teeth in [12, 17, 30] {
        let o = outline(&Params {
            teeth,
            ..Params::default()
        })
        .unwrap();
        assert_eq!(o.edges.len(), teeth as usize * 4);
        let names: std::collections::BTreeSet<_> = o.edges.iter().map(|e| &e.name).collect();
        assert_eq!(names.len(), o.edges.len());
        for i in 0..o.edges.len() {
            let a = o.edges[i].to_point();
            let b = o.edges[(i + 1) % o.edges.len()].from_point();
            assert!((a[0] - b[0]).hypot(a[1] - b[1]) < 1e-8);
        }
        let (
            Segment::Arc {
                center: c0,
                radius: r0,
                ..
            },
            Segment::Arc {
                center: c1,
                radius: r1,
                ..
            },
        ) = (&o.edges[0].segment, &o.edges[1].segment)
        else {
            panic!("座と側面は円弧")
        };
        assert!(((c0[0] - c1[0]).hypot(c0[1] - c1[1]) - r0 - r1).abs() < 1e-9);
        let ring = ProfileLoop { edges: o.edges };
        assert!(ring.signed_area() > 0.0);
        let v: Vec<_> = ring
            .edges
            .iter()
            .flat_map(|e| {
                let mut p = e.polyline(5);
                p.pop();
                p
            })
            .collect();
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
                    "{teeth}: {i} {j}"
                );
            }
        }
    }
}

#[test]
fn invalid_manual_chain_dimensions_are_rejected() {
    let base = ChainSize::Size40.dimensions().unwrap();
    for chain in [
        Chain { pitch: 0.0, ..base },
        Chain {
            roller: 12.7,
            ..base
        },
        Chain {
            inner_width: -1.0,
            ..base
        },
        Chain {
            roller: f64::NAN,
            ..base
        },
        Chain {
            pitch: f64::MAX,
            ..base
        },
    ] {
        assert!(dimensions(chain, 17).is_err(), "{chain:?}");
    }
    assert!(dimensions(base, 2).is_err());
}
