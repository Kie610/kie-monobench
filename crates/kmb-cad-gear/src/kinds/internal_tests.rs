//! 確かめること: 内歯の歯先・歯底・歯溝幅、閉じた交差の無い輪、転位と丸み、入力の拒否。

use super::*;
use kmb_core::{ProfileLoop, Segment};

fn shape(z: u32, shift: f64) -> Outline {
    outline(&Params {
        module: 2.0,
        teeth: z,
        profile_shift: shift,
        ..Params::default()
    })
    .unwrap()
}

fn radius(o: &Outline, name: &str) -> f64 {
    o.reference.iter().find(|(n, _, _)| *n == name).unwrap().2
}

#[test]
fn inner_tip_and_outer_root_follow_the_internal_gear_dimensions() {
    for (z, x) in [(12, 0.0), (20, 0.3), (30, -0.2)] {
        let o = shape(z, x);
        assert!((2.0 * radius(&o, "tip") - (2.0 * z as f64 - 4.0 + 4.0 * x)).abs() < 1e-9);
        assert!((2.0 * radius(&o, "root") - (2.0 * z as f64 + 5.0 + 4.0 * x)).abs() < 1e-9);
        for e in &o.edges {
            for q in e.polyline(16) {
                let r = q[0].hypot(q[1]);
                assert!(
                    r >= radius(&o, "tip") - 1e-7 && r <= radius(&o, "root") + 1e-7,
                    "{z} {r}"
                );
            }
        }
    }
}

#[test]
fn a_pitch_circle_gap_is_half_a_pitch_wide() {
    let z = 20;
    let o = shape(z, 0.0);
    let r = radius(&o, "pitch");
    // 歯溝の中心は半ピッチの位置。両歯面はそこから πm/4 だけ離れる。
    for angle in [PI / (2.0 * z as f64), 3.0 * PI / (2.0 * z as f64)] {
        let q = polar(r, angle);
        let distance = o
            .edges
            .iter()
            .map(|e| e.segment.distance(q))
            .fold(f64::INFINITY, f64::min);
        assert!(distance < tolerance(2.0), "{angle}: {distance}");
    }
    let gap = (3.0 * PI / (2.0 * z as f64) - PI / (2.0 * z as f64)) * r;
    assert!((gap - PI * 2.0 / 2.0).abs() < 1e-12);
}

#[test]
fn internal_boundaries_are_closed_simple_and_keep_tooth_names() {
    for z in [12, 20, 30] {
        let o = shape(z, 0.0);
        let names: std::collections::BTreeSet<_> = o.edges.iter().map(|e| e.name.clone()).collect();
        assert_eq!(names.len(), o.edges.len());
        for i in 0..o.edges.len() {
            let a = o.edges[i].to_point();
            let b = o.edges[(i + 1) % o.edges.len()].from_point();
            assert!(
                (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-7,
                "{z}: {}",
                o.edges[i].name
            );
        }
        for i in 0..z {
            for part in ["tip", "root", "r0", "l0"] {
                assert!(names.contains(&format!("t{i}:{part}")));
            }
        }
        let ring = ProfileLoop {
            edges: o.edges.clone(),
        };
        assert!(ring.signed_area() > 0.0);
        let points: Vec<_> = o.edges.iter().flat_map(|e| e.polyline(12)).collect();
        let orient =
            |a: P2, b: P2, c: P2| (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        for i in 0..points.len() {
            for j in i + 2..points.len() {
                if i == 0 && j == points.len() - 1 {
                    continue;
                }
                let (a, b, c, d) = (
                    points[i],
                    points[(i + 1) % points.len()],
                    points[j],
                    points[(j + 1) % points.len()],
                );
                assert!(
                    !(orient(a, b, c) * orient(a, b, d) < -1e-18
                        && orient(c, d, a) * orient(c, d, b) < -1e-18),
                    "{z}: {i} {j}"
                );
            }
        }
    }
}

#[test]
fn tip_and_root_rounds_touch_the_correct_internal_circles() {
    let p = Params {
        module: 2.0,
        teeth: 30,
        tip_round: 0.1,
        root_round: 0.1,
        ..Params::default()
    };
    let o = outline(&p).unwrap();
    for (part, circle, sign) in [("rround", "tip", -1.0), ("rfillet", "root", 1.0)] {
        let e = o
            .edges
            .iter()
            .find(|e| e.name == format!("t0:{part}"))
            .unwrap();
        let Segment::Arc {
            center, radius: r, ..
        } = e.segment
        else {
            panic!("丸みは円弧")
        };
        assert!((r - 0.2).abs() < 1e-9);
        assert!((center[0].hypot(center[1]) + sign * r - radius(&o, circle)).abs() < 1e-7);
    }
    assert!(o.warnings.is_empty(), "{:?}", o.warnings);
}

#[test]
fn impossible_internal_teeth_are_errors_without_mating_warnings() {
    for p in [
        Params {
            teeth: 3,
            ..Params::default()
        },
        Params {
            profile_shift: 5.0,
            ..Params::default()
        },
        Params {
            tip_round: -1.0,
            ..Params::default()
        },
        Params {
            module: f64::INFINITY,
            ..Params::default()
        },
        Params {
            module: 1e100,
            root_round: 1e300,
            ..Params::default()
        },
    ] {
        assert!(outline(&p).is_err(), "{p:?}");
    }
    assert!(
        shape(12, 0.0)
            .warnings
            .iter()
            .all(|w| !w.contains("切り下げ") && !w.contains("相手"))
    );
}
