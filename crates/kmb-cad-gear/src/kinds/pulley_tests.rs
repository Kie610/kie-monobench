//! 確かめること: V 溝の規格値と上書き、溝数・幅、平ベルト車の中高、閉じた軸断面。

use super::*;
use kmb_core::ProfileLoop;

#[test]
fn v_sections_select_the_angle_at_the_diameter_boundaries() {
    for (section, cases, w, above, below) in [
        (
            VSection::A,
            [(118.0, 34.0), (118.01, 36.0), (190.0, 36.0), (190.01, 38.0)],
            11.0,
            2.75,
            8.7,
        ),
        (
            VSection::B,
            [(190.0, 34.0), (190.01, 36.0), (315.0, 36.0), (315.01, 38.0)],
            14.0,
            3.5,
            10.8,
        ),
    ] {
        for (diameter, angle) in cases {
            let d = section.dimensions(diameter);
            assert!((d.angle.to_degrees() - angle).abs() < 1e-12);
            assert_eq!((d.pitch_width, d.above, d.below), (w, above, below));
        }
    }
}

#[test]
fn groove_count_opening_width_depth_and_angle_match_the_section() {
    let p = Pulley {
        diameter: 150.0,
        width: 60.0,
        grooves: 3,
        ..Pulley::default()
    };
    let o = outline(
        &Params {
            pulley: Some(p),
            ..Params::default()
        },
        true,
    )
    .unwrap();
    assert!(o.axial);
    assert_eq!(
        o.edges
            .iter()
            .filter(|e| e.name.ends_with(":floor"))
            .count(),
        3
    );
    let d = p.section.dimensions(p.diameter);
    for i in 0..3 {
        let left = o
            .edges
            .iter()
            .find(|e| e.name == format!("g{i}:left"))
            .unwrap();
        let right = o
            .edges
            .iter()
            .find(|e| e.name == format!("g{i}:right"))
            .unwrap();
        let a = left.from_point();
        let b = left.to_point();
        assert!(((b[1] - a[1]) / (a[0] - b[0]) - (d.angle / 2.0).tan()).abs() < 1e-12);
        assert!((a[0] - b[0] - (d.above + d.below)).abs() < 1e-12);
        let opening = right.to_point()[1] - a[1];
        assert!((opening - (d.pitch_width + 2.0 * d.above * (d.angle / 2.0).tan())).abs() < 1e-12);
    }
    let ring = ProfileLoop { edges: o.edges };
    assert!(ring.signed_area() > 0.0);
    for i in 0..ring.edges.len() {
        let a = ring.edges[i].to_point();
        let b = ring.edges[(i + 1) % ring.edges.len()].from_point();
        assert!((a[0] - b[0]).hypot(a[1] - b[1]) < 1e-9);
    }
    let manual = outline(
        &Params {
            pulley: Some(Pulley {
                width: 75.0,
                angle: Some(40f64.to_radians()),
                top_width: Some(20.0),
                depth: Some(12.0),
                ..p
            }),
            ..Params::default()
        },
        true,
    )
    .unwrap();
    let a = manual.edges.iter().find(|e| e.name == "g0:left").unwrap();
    assert!((a.from_point()[0] - a.to_point()[0] - 12.0).abs() < 1e-12);
}

#[test]
fn flat_pulleys_have_the_requested_crown_height() {
    for height in [0.0, 0.5, 2.0] {
        let p = Pulley {
            crown: height,
            ..Pulley::default()
        };
        let o = outline(
            &Params {
                pulley: Some(p),
                ..Params::default()
            },
            false,
        )
        .unwrap();
        let crown = o.edges.iter().find(|e| e.name == "crown").unwrap();
        match crown.segment {
            Segment::Line { .. } => assert_eq!(height, 0.0),
            Segment::Arc { center, radius, .. } => {
                assert!((center[0] + radius - p.diameter / 2.0 - height).abs() < 1e-9)
            }
            _ => panic!("線か円弧"),
        }
    }
}

#[test]
fn overlapping_grooves_and_invalid_radial_dimensions_are_rejected() {
    for p in [
        Pulley {
            width: 10.0,
            ..Pulley::default()
        },
        Pulley {
            grooves: 0,
            ..Pulley::default()
        },
        Pulley {
            angle: Some(0.0),
            ..Pulley::default()
        },
        Pulley {
            depth: Some(100.0),
            ..Pulley::default()
        },
        Pulley {
            bore: 100.0,
            ..Pulley::default()
        },
        Pulley {
            diameter: f64::INFINITY,
            ..Pulley::default()
        },
    ] {
        assert!(
            outline(
                &Params {
                    pulley: Some(p),
                    ..Params::default()
                },
                true
            )
            .is_err(),
            "{p:?}"
        );
    }
    assert!(
        outline(
            &Params {
                pulley: Some(Pulley {
                    crown: -1.0,
                    ..Pulley::default()
                }),
                ..Params::default()
            },
            false
        )
        .is_err()
    );
}

#[test]
fn crowns_that_cannot_preserve_closed_endpoints_are_rejected() {
    let result = outline(
        &Params {
            pulley: Some(Pulley {
                crown: 1e-30,
                ..Pulley::default()
            }),
            ..Params::default()
        },
        false,
    );
    assert!(result.is_err());
}
