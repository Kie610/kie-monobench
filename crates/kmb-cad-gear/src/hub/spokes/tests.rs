//! 新しい純計算の試験。カーネルの実形状試験は kmb-solid 側。
use super::*;
fn params(count: u32, angle: f64, round: f64) -> Params {
    Params {
        count,
        root_width: 2.0,
        tip_width: 1.5,
        angle,
        round,
    }
}
fn make(p: &Params) -> Result<Layout, String> {
    layout(
        circle("outside", [0.0; 2], 24.0),
        21.5,
        2.0,
        19.5,
        8.0,
        Some(reverse(circle("bore", [0.0; 2], 3.0))),
        p,
    )
}
fn area(p: &Profile) -> f64 {
    p.regions
        .iter()
        .map(|r| r.outer.signed_area() + r.holes.iter().map(ProfileLoop::signed_area).sum::<f64>())
        .sum()
}
fn closed(r: &ProfileLoop) {
    for (i, e) in r.edges.iter().enumerate() {
        let next = &r.edges[(i + 1) % r.edges.len()];
        assert!(
            norm(add(e.to_point(), mul(next.from_point(), -1.0))) < 1e-8,
            "{} -> {}",
            e.name,
            next.name
        );
    }
    assert!(r.signed_area().is_finite());
}
#[test]
fn counts_and_both_curvature_directions_have_closed_disjoint_regions_and_conserve_area() {
    for count in [3, 5, 12] {
        for angle in [-30f64.to_radians(), 0.0, 30f64.to_radians()] {
            let got = make(&params(count, angle, 0.0)).unwrap();
            assert_eq!(got.profile.regions[0].holes.len(), count as usize + 1);
            for r in &got.profile.regions {
                closed(&r.outer);
                for h in &r.holes {
                    closed(h);
                    assert!(h.signed_area() < 0.0);
                }
            }
            let web = got.parts.web.as_ref().unwrap();
            assert_eq!(web.regions.len(), count as usize);
            for r in &web.regions {
                closed(&r.outer);
                assert!(r.outer.signed_area() > 0.0);
            }
            let split = area(&got.parts.rim) + area(web) + area(got.parts.boss.as_ref().unwrap());
            assert!((split - area(&got.profile)).abs() < 1e-7);
            assert_eq!(got.parts.spokes.unwrap().paths.len(), count as usize);
        }
    }
}
#[test]
fn zero_boss_overlap_and_bad_dimensions_are_explicit_errors() {
    let outer = || circle("outside", [0.0; 2], 24.0);
    assert!(layout(outer(), 21.5, 2.0, 19.5, 0.0, None, &params(5, 0.0, 0.0)).is_err());
    for v in [0.0, -1.0, f64::INFINITY, f64::NAN, 100.0] {
        let mut p = params(5, 0.0, 0.0);
        p.root_width = v;
        assert!(make(&p).is_err());
    }
    for count in [0, 2, 13, u32::MAX] {
        assert!(make(&params(count, 0.0, 0.0)).is_err());
    }
    assert!(make(&params(5, TAU, 0.0)).is_err());
    assert!(make(&params(5, 0.0, 100.0)).is_err());
}
#[test]
fn root_rounding_adds_material_without_changing_rim_boss_or_centerlines() {
    for angle in [0.0, 30f64.to_radians()] {
        let sharp = make(&params(5, angle, 0.0)).unwrap();
        let round = make(&params(5, angle, 0.5)).unwrap();
        assert!(area(&round.profile) > area(&sharp.profile));
        assert_eq!(round.parts.rim, sharp.parts.rim);
        assert_eq!(round.parts.boss, sharp.parts.boss);
        let a = round.parts.spokes.as_ref().unwrap();
        let b = sharp.parts.spokes.as_ref().unwrap();
        assert_eq!(a.paths, b.paths);
        assert!(a.root_pads.is_some());
        assert!(b.root_pads.is_none());
        for r in &a.root_pads.as_ref().unwrap().regions {
            closed(&r.outer);
            assert!(r.outer.signed_area() > 0.0);
        }
    }
}
#[test]
fn placed_paths_and_reinforcement_use_the_same_plane_phase_and_center() {
    let raw = make(&params(5, 0.3, 0.5)).unwrap();
    let originals = raw.parts.spokes.clone().unwrap();
    let placed = raw.placed(Plane::YZ, 0.4, [7.0, -3.0]);
    let arms = placed.parts.spokes.unwrap();
    for (a, b) in arms.paths.iter().zip(originals.paths) {
        assert_eq!(a.plane, Plane::YZ);
        assert_eq!(
            a.edges[0],
            tooth::translate(&tooth::rotate(&b.edges[0], 0.4), [7.0, -3.0])
        );
    }
    assert_eq!(arms.root_pads.unwrap().plane, Plane::YZ);
}
#[test]
fn old_parts_without_spokes_serialize_without_an_added_key() {
    let p = Parts {
        rim: Profile {
            plane: Plane::XY,
            regions: vec![],
        },
        web: None,
        boss: None,
        spokes: None,
    };
    let v = serde_json::to_value(&p).unwrap();
    assert!(v.get("spokes").is_none());
    assert_eq!(serde_json::from_value::<Parts>(v).unwrap(), p);
}

#[test]
fn analytic_intersections_reject_crossing_lines_and_arcs_but_not_separated_arcs() {
    let line = Segment::Line {
        a: [-2.0, 0.0],
        b: [2.0, 0.0],
    };
    assert!(intersects(
        &line,
        &Segment::Circle {
            center: [0.0; 2],
            radius: 1.0
        }
    ));
    assert!(!intersects(
        &line,
        &Segment::Arc {
            center: [0.0; 2],
            radius: 1.0,
            start: 0.2,
            end: 1.0
        }
    ));
    assert!(intersects(
        &line,
        &Segment::Line {
            a: [0.0, -2.0],
            b: [0.0, 2.0]
        }
    ));
    assert!(!intersects(
        &line,
        &Segment::Line {
            a: [3.0, 0.0],
            b: [4.0, 0.0]
        }
    ));
    let a = Segment::Arc {
        center: [0.0; 2],
        radius: 2.0,
        start: 0.0,
        end: 0.5,
    };
    let b = Segment::Arc {
        center: [0.0; 2],
        radius: 2.0,
        start: 1.0,
        end: 1.5,
    };
    assert!(!intersects(&a, &b));
    for angle in [-0.5, 0.0, 0.5] {
        let got = make(&params(5, angle, 0.5)).unwrap();
        for ring in &got.profile.regions[0].holes {
            assert!(simple(ring));
        }
    }
}
