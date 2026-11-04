//! 閉じた輪郭の、円形工具の中心経路から届かない所を見積もる。
//! 輪郭の種類を知らず、削る側と工具の半径だけで確かめる。経路は変えない。

use kmb_cam_geom::{Loop, P2, Seg, offset_loop_outward, seg_dist, sweep_of};
use std::f64::consts::TAU;

#[derive(Debug)]
pub(super) struct Clearance {
    pub regions: usize,
    pub diameter: f64,
}

/// 到達できない連続した境界の数と、届く工具径の目安。
pub(super) fn check(boundary: &Loop, tool_out: bool, radius: f64) -> Option<Clearance> {
    if !radius.is_finite() || radius <= 0.0 || boundary.verts.len() < 2 {
        return None;
    }
    let boundary = if boundary.signed_area() < 0.0 {
        boundary.reversed()
    } else {
        boundary.clone()
    };
    // 曲線の弦の誤差とオフセット計算の許容差を、到達できない厚さと取り違えない。
    let tolerance = (radius * 0.002).clamp(1e-4, 0.005);
    let points = sample(&boundary, radius, tolerance);
    let bad = unreachable(&boundary, &points, tool_out, radius, tolerance * 4.0);
    let regions = groups(&bad);
    if regions == 0 {
        return None;
    }
    let (mut low, mut high) = (0.0, radius);
    for _ in 0..12 {
        let mid = (low + high) / 2.0;
        let mid_tolerance = (mid * 0.002).clamp(1e-4, 0.005);
        let mid_points = sample(&boundary, mid, mid_tolerance);
        if unreachable(&boundary, &mid_points, tool_out, mid, mid_tolerance * 4.0)
            .iter()
            .any(|b| *b)
        {
            high = mid;
        } else {
            low = mid;
        }
    }
    Some(Clearance {
        regions,
        diameter: 2.0 * low,
    })
}

fn sample(boundary: &Loop, radius: f64, tolerance: f64) -> Vec<P2> {
    let mut out = Vec::new();
    for segment in boundary.segments() {
        match segment {
            Seg::Line { a, b } => {
                let length = (a[0] - b[0]).hypot(a[1] - b[1]);
                let n = (length / (radius * 0.5).clamp(0.05, 0.5))
                    .ceil()
                    .clamp(1.0, 128.0) as usize;
                for i in 0..n {
                    let t = i as f64 / n as f64;
                    out.push([a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])]);
                }
            }
            Seg::Arc { a, b, center, ccw } => {
                let r = (a[0] - center[0]).hypot(a[1] - center[1]);
                let sweep = sweep_of(a, b, center, ccw);
                let step = if r > tolerance {
                    2.0 * (1.0 - tolerance / r).clamp(-1.0, 1.0).acos()
                } else {
                    std::f64::consts::FRAC_PI_4
                };
                let n = (sweep.abs() / step.max(1e-3)).ceil().max(1.0) as usize;
                let from = (a[1] - center[1]).atan2(a[0] - center[0]);
                for i in 0..n {
                    let angle = from + sweep * i as f64 / n as f64;
                    out.push([center[0] + r * angle.cos(), center[1] + r * angle.sin()]);
                }
            }
        }
    }
    out
}

fn groups(bad: &[bool]) -> usize {
    if bad.is_empty() || bad.iter().all(|v| !*v) {
        return 0;
    }
    if bad.iter().all(|v| *v) {
        return 1;
    }
    (0..bad.len())
        .filter(|i| bad[*i] && !bad[(*i + bad.len() - 1) % bad.len()])
        .count()
}

fn unreachable(
    boundary: &Loop,
    points: &[P2],
    tool_out: bool,
    radius: f64,
    tolerance: f64,
) -> Vec<bool> {
    let paths = offset_loop_outward(boundary, if tool_out { radius } else { -radius });
    let segments: Vec<_> = paths.iter().flat_map(Loop::segments).collect();
    points
        .iter()
        .map(|p| {
            let d = segments
                .iter()
                .map(|s| distance(*p, *s))
                .fold(f64::INFINITY, f64::min);
            d > radius + tolerance
        })
        .collect()
}

fn distance(p: P2, segment: Seg) -> f64 {
    match segment {
        Seg::Line { a, b } => seg_dist(p, a, b),
        Seg::Arc { a, b, center, ccw } => {
            let radius = (a[0] - center[0]).hypot(a[1] - center[1]);
            let from = (a[1] - center[1]).atan2(a[0] - center[0]);
            let at = (p[1] - center[1]).atan2(p[0] - center[0]);
            let sweep = sweep_of(a, b, center, ccw).abs();
            let travelled = if ccw {
                (at - from).rem_euclid(TAU)
            } else {
                (from - at).rem_euclid(TAU)
            };
            if travelled <= sweep + 1e-12 {
                ((p[0] - center[0]).hypot(p[1] - center[1]) - radius).abs()
            } else {
                (p[0] - a[0])
                    .hypot(p[1] - a[1])
                    .min((p[0] - b[0]).hypot(p[1] - b[1]))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kmb_cam_geom::Vertex;
    fn notch() -> Loop {
        Loop {
            verts: [
                (0.0, 0.0, 0.0),
                (10.0, 0.0, 0.0),
                (10.0, 10.0, 0.0),
                (7.0, 10.0, 0.0),
                (7.0, 5.0, -1.0),
                (3.0, 5.0, 0.0),
                (3.0, 10.0, 0.0),
                (0.0, 10.0, 0.0),
            ]
            .map(|(x, y, bulge)| Vertex { x, y, bulge })
            .to_vec(),
        }
    }
    #[test]
    fn a_rounded_notch_warns_only_when_the_tool_cannot_reach_the_bottom() {
        let shape = notch();
        assert!(check(&shape, true, 1.0).is_none());
        let c = check(&shape, true, 2.5).unwrap();
        assert_eq!(c.regions, 1);
        assert!((c.diameter - 4.0).abs() < 0.15, "{}", c.diameter);
        let reversed = check(&shape.reversed(), true, 2.5).unwrap();
        assert_eq!(reversed.regions, c.regions);
        assert!((reversed.diameter - c.diameter).abs() < 1e-8);
    }
    #[test]
    fn an_ordinary_convex_outline_has_no_tool_clearance_warning() {
        let shape = Loop::from_points(&[[0.0, 0.0], [20.0, 0.0], [20.0, 10.0], [0.0, 10.0]]);
        for radius in [0.25, 1.5, 4.0] {
            assert!(check(&shape, true, radius).is_none());
        }
    }
    #[test]
    fn regions_crossing_the_start_are_counted_once() {
        assert_eq!(groups(&[true, true, false, true]), 1);
        assert_eq!(groups(&[true, false, true, false]), 2);
        assert_eq!(groups(&[true; 4]), 1);
        assert_eq!(groups(&[false; 4]), 0);
    }
}
