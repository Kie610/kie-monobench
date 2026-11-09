//! 確かめること: 穴の配置と許容径、キー表の境界、単一のキー溝境界、領域の面積の保存。
use super::*;
use crate::kinds::{self, Params as ToothParams};
use kmb_interchange::GearKind;

fn make(p: &Params) -> Result<Layout, String> {
    let shape = kinds::outline(
        GearKind::External,
        &ToothParams {
            module: 2.0,
            teeth: 24,
            ..Default::default()
        },
    )
    .unwrap();
    layout(ProfileLoop { edges: shape.edges }, 21.5, 2.0, 4.0, p)
}

fn area(profile: &Profile) -> f64 {
    profile
        .regions
        .iter()
        .map(|r| r.outer.signed_area() + r.holes.iter().map(ProfileLoop::signed_area).sum::<f64>())
        .sum()
}

#[test]
fn the_key_table_preserves_every_interval_boundary() {
    let rows = [
        (6.0, 2.0, 1.0),
        (8.0, 2.0, 1.0),
        (10.0, 3.0, 1.4),
        (12.0, 4.0, 1.8),
        (17.0, 5.0, 2.3),
        (22.0, 6.0, 2.8),
        (30.0, 8.0, 3.3),
        (38.0, 10.0, 3.3),
        (44.0, 12.0, 3.3),
        (50.0, 14.0, 3.8),
        (58.0, 16.0, 4.3),
        (65.0, 18.0, 4.4),
    ];
    for (diameter, width, depth) in rows {
        assert_eq!(key_size(diameter), Some((width, depth)));
    }
    assert_eq!(key_size(8.0001), Some((3.0, 1.4)));
    for value in [5.999, 65.001, f64::NAN, f64::INFINITY] {
        assert_eq!(key_size(value), None);
    }
}

#[test]
fn lightening_holes_lie_on_the_given_circle_and_inside_the_web() {
    let p = Params {
        count: 4,
        hole_diameter: 3.0,
        circle_diameter: 26.0,
        boss_diameter: 16.0,
        ..Default::default()
    };
    let got = make(&p).unwrap();
    let holes = &got.profile.regions[0].holes;
    assert_eq!(holes.len(), 5);
    for (i, ring) in holes[..4].iter().enumerate() {
        let Segment::Circle { center, radius } = ring.edges[0].segment else {
            panic!("円の穴");
        };
        let expected = polar(13.0, TAU * i as f64 / 4.0);
        assert!((center[0] - expected[0]).hypot(center[1] - expected[1]) < 1e-12);
        assert_eq!(radius, 1.5);
        assert!(ring.signed_area() < 0.0);
    }
    let sum = area(&got.parts.rim)
        + area(got.parts.web.as_ref().unwrap())
        + area(got.parts.boss.as_ref().unwrap());
    assert!((sum - area(&got.profile)).abs() < 1e-8);
}

#[test]
fn holes_that_reach_the_rim_or_boss_report_the_maximum_diameter() {
    for (circle_diameter, hole_diameter, max) in [
        (38.0, 1.0, "1.000"),
        (18.0, 2.0, "2.000"),
        (26.0, 11.0, "10.000"),
    ] {
        let p = Params {
            count: 4,
            hole_diameter,
            circle_diameter,
            boss_diameter: 16.0,
            ..Default::default()
        };
        let why = make(&p).err().unwrap();
        assert!(why.contains(max), "{why}");
    }
}

#[test]
fn the_keyway_is_one_closed_hole_with_the_table_width_and_depth() {
    let got = make(&Params {
        keyway: true,
        boss_diameter: 16.0,
        ..Default::default()
    })
    .unwrap();
    let hole = &got.profile.regions[0].holes[0];
    assert_eq!(hole.edges.len(), 4);
    for i in 0..hole.edges.len() {
        let a = hole.edges[i].to_point();
        let b = hole.edges[(i + 1) % hole.edges.len()].from_point();
        assert!((a[0] - b[0]).hypot(a[1] - b[1]) < 1e-9);
    }
    assert!(hole.signed_area() < -std::f64::consts::PI * 16.0);
    let top = hole
        .edges
        .iter()
        .find(|e| e.name == "bore:key:top")
        .unwrap();
    let (a, b) = (top.from_point(), top.to_point());
    assert!(((a[0] - b[0]).abs() - 2.0).abs() < 1e-12);
    assert_eq!(a[1], 5.0);
    let boss_hole = &got.parts.boss.as_ref().unwrap().regions[0].holes[0];
    assert_eq!(boss_hole, hole);
}

#[test]
fn key_overrides_require_positive_values_and_keep_material_around_the_slot() {
    assert!(
        make(&Params {
            keyway: true,
            key_width: Some(4.0),
            key_depth: Some(2.0),
            boss_diameter: 16.0,
            ..Default::default()
        })
        .is_ok()
    );
    for p in [
        Params {
            keyway: true,
            key_width: Some(0.0),
            ..Default::default()
        },
        Params {
            keyway: true,
            key_depth: Some(-1.0),
            ..Default::default()
        },
        Params {
            keyway: true,
            key_depth: Some(5.0),
            boss_diameter: 16.0,
            ..Default::default()
        },
        Params {
            count: 1001,
            ..Default::default()
        },
        Params {
            boss_diameter: f64::INFINITY,
            ..Default::default()
        },
    ] {
        assert!(make(&p).is_err());
    }
    let mut outline = kinds::outline(GearKind::External, &ToothParams::default()).unwrap();
    assert!(
        layout(
            ProfileLoop {
                edges: std::mem::take(&mut outline.edges)
            },
            8.75,
            1.0,
            2.5,
            &Params {
                keyway: true,
                ..Default::default()
            }
        )
        .is_err()
    );
}

#[test]
fn no_extra_fields_preserve_the_complete_profile_and_partition_area() {
    let plain = make(&Params::default()).unwrap();
    assert_eq!(plain.profile.regions[0].holes.len(), 1);
    let sum = area(&plain.parts.rim) + area(plain.parts.web.as_ref().unwrap());
    assert!((sum - area(&plain.profile)).abs() < 1e-8);
    assert!(plain.parts.boss.is_none());
}

#[test]
fn positive_rim_band_is_exact_even_below_one_module_and_zero_keeps_legacy_band() {
    for (requested, expected) in [(0.0, 19.5), (0.5, 21.0), (3.0, 18.5)] {
        let got = make(&Params {
            rim_width: requested,
            ..Default::default()
        })
        .unwrap();
        let Segment::Circle { radius, .. } = got.parts.rim.regions[0].holes[0].edges[0].segment
        else {
            panic!("rim circle")
        };
        assert_eq!(radius, expected);
    }
    assert!(
        make(&Params {
            rim_width: 14.0,
            boss_diameter: 16.0,
            ..Default::default()
        })
        .err()
        .unwrap()
        .contains("13.500")
    );
}
