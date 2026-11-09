//! 確かめること: 端と許容内の交点は切れ目にならず、零長さの片や延長を作らない。
use crate::*;
use crate::curve::Shape;
use std::collections::BTreeSet;

fn curve(kind: usize) -> (Sketch, EntityId) {
    let mut s = Sketch::default();
    let id = match kind {
        0 => {
            let a = s.add_point(0.0, 0.0);
            let b = s.add_point(20.0, 0.0);
            s.add_line(a, b)
        }
        1 => {
            let c = s.add_point(0.0, 0.0);
            let a = s.add_point(10.0, 0.0);
            let b = s.add_point(0.0, 10.0);
            s.add_arc(c, a, b)
        }
        2 => {
            let center = s.add_point(0.0, 0.0);
            let major = s.add_point(20.0, 0.0);
            let minor = s.add_point(0.0, 10.0);
            let a = s.add_point(20.0, 0.0);
            let b = s.add_point(0.0, 10.0);
            s.add_entity(&Entity::Ellipse {
                id: EntityId(0), center, major, minor, ends: Some((a, b)), construction: false,
            })
        }
        3 => s.add_spline(&[(None, [0.0, 0.0]), (None, [10.0, 5.0]), (None, [20.0, 0.0])], true, false).unwrap(),
        4 => s.add_conic((None, [0.0, 0.0]), (None, [20.0, 0.0]), (None, [10.0, 10.0]), 0.5).unwrap(),
        _ => unreachable!(),
    };
    (s, id)
}
fn at(shape: &Shape, u: f64) -> [f64; 2] {
    let (lo, hi) = shape.domain();
    shape.at(lo + (hi - lo) * u)
}
fn crossbar(s: &mut Sketch, shape: &Shape, u: f64) -> EntityId {
    let p = at(shape, u);
    let (a, b) = (at(shape, u - 1e-5), at(shape, u + 1e-5));
    let d = [b[0] - a[0], b[1] - a[1]];
    let l = d[0].hypot(d[1]);
    let n = [-d[1] * 40.0 / l, d[0] * 40.0 / l];
    let a = s.add_point(p[0] - n[0], p[1] - n[1]);
    let b = s.add_point(p[0] + n[0], p[1] + n[1]);
    s.add_line(a, b)
}
fn trim_near_end(kind: usize) {
    for side in [0.0, 1.0] {
        for offset in [0.0, -5e-8, 5e-8] {
            let (mut s, id) = curve(kind);
            let original = s.shape(id).unwrap();
            let cutters = BTreeSet::from([
                crossbar(&mut s, &original, side + offset),
                crossbar(&mut s, &original, 0.5),
            ]);
            assert!(s.trim(id, at(&original, if side == 0.0 { 0.25 } else { 0.75 })));
            let pieces: Vec<_> = s.entities.iter().filter_map(|e| {
                (!cutters.contains(&e.id())).then(|| s.shape(e.id())).flatten()
            }).collect();
            assert_eq!(pieces.len(), 1, "{kind} {side} {offset}: 端の極小片は残さない");
            let piece = &pieces[0];
            let (a, b) = (at(piece, 0.0), at(piece, 1.0));
            assert!((a[0] - b[0]).hypot(a[1] - b[1]) > 1.0);
            for i in 0..=10 {
                assert!(original.closest(at(piece, i as f64 / 10.0)).1 < 1e-6);
            }
        }
    }
}
#[test] fn line_trim_ignores_endpoint_intersections() { trim_near_end(0); }
#[test] fn arc_trim_ignores_endpoint_intersections() { trim_near_end(1); }
#[test] fn ellipse_arc_trim_ignores_endpoint_intersections() { trim_near_end(2); }
#[test] fn spline_trim_ignores_endpoint_intersections() { trim_near_end(3); }
#[test] fn conic_trim_ignores_endpoint_intersections() { trim_near_end(4); }
#[test]
fn line_extend_ignores_an_intersection_within_endpoint_tolerance() {
    for offset in [0.0, -5e-9, 5e-9] {
        let (mut s, id) = curve(0);
        let a = s.add_point(20.0 + offset, -10.0);
        let b = s.add_point(20.0 + offset, 10.0);
        s.add_line(a, b);
        let before = serde_json::to_value(&s).unwrap();
        assert!(!s.extend(id, [20.0, 0.0]));
        assert_eq!(serde_json::to_value(&s).unwrap(), before);
    }
}
#[test]
fn arc_extend_ignores_an_intersection_within_endpoint_tolerance() {
    for offset in [0.0, -5e-12_f64, 5e-12_f64] {
        let (mut s, id) = curve(1);
        let angle = std::f64::consts::FRAC_PI_2 + offset;
        let p = [10.0 * angle.cos(), 10.0 * angle.sin()];
        let tangent = [-angle.sin() * 20.0, angle.cos() * 20.0];
        let a = s.add_point(p[0] - tangent[0], p[1] - tangent[1]);
        let b = s.add_point(p[0] + tangent[0], p[1] + tangent[1]);
        s.add_line(a, b);
        let before = serde_json::to_value(&s).unwrap();
        assert!(!s.extend(id, [0.0, 10.0]));
        assert_eq!(serde_json::to_value(&s).unwrap(), before);
    }
}
