//! 確かめること: ロープの溝の半径、ピッチの位置、断面の閉じ方、縁と軸の穴の入力。

use super::*;
use kmb_core::{ProfileLoop, Segment};

#[test]
fn rope_grooves_are_semicircles_centered_on_the_pitch_radius() {
    for rope in [6.0, 10.0, 16.0] {
        let p = Sheave {
            rope,
            ..Sheave::default()
        };
        let o = outline(&Params {
            sheave: Some(p),
            ..Params::default()
        })
        .unwrap();
        let groove = o.edges.iter().find(|e| e.name == "groove").unwrap();
        let Segment::Arc {
            center,
            radius,
            start,
            end,
        } = groove.segment
        else {
            panic!("円弧")
        };
        assert_eq!(center, [p.diameter / 2.0, p.width / 2.0]);
        assert!((radius - rope * 0.53).abs() < 1e-12);
        assert!((end - start - std::f64::consts::PI).abs() < 1e-12);
        assert!(groove.reversed);
        let ring = ProfileLoop { edges: o.edges };
        assert!(ring.signed_area() > 0.0);
        for i in 0..ring.edges.len() {
            let a = ring.edges[i].to_point();
            let b = ring.edges[(i + 1) % ring.edges.len()].from_point();
            assert!((a[0] - b[0]).hypot(a[1] - b[1]) < 1e-9);
        }
    }
}

#[test]
fn rope_width_flange_and_bore_cannot_cross_the_groove() {
    for p in [
        Sheave {
            width: 10.0,
            ..Sheave::default()
        },
        Sheave {
            rope: 0.0,
            ..Sheave::default()
        },
        Sheave {
            flange: -1.0,
            ..Sheave::default()
        },
        Sheave {
            bore: 90.0,
            ..Sheave::default()
        },
        Sheave {
            diameter: f64::MAX,
            flange: f64::MAX,
            ..Sheave::default()
        },
    ] {
        assert!(
            outline(&Params {
                sheave: Some(p),
                ..Params::default()
            })
            .is_err(),
            "{p:?}"
        );
    }
    assert!(
        outline(&Params {
            sheave: Some(Sheave {
                flange: 0.0,
                ..Sheave::default()
            }),
            ..Params::default()
        })
        .is_ok()
    );
}
