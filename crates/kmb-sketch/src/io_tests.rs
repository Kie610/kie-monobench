//! 読んだ形をスケッチへ入れる操作と、曲線の近似の試験(DXF・SVG の読み書きは kmb-io-dxf・kmb-io-svg)。

use super::*;
use kmb_core::kernel::{P2, Plane, Segment};
use std::collections::HashMap;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}
fn close2(a: P2, b: P2, tol: f64) -> bool {
    close(a[0], b[0], tol) && close(a[1], b[1], tol)
}
fn polar(c: P2, r: f64, a: f64) -> P2 {
    [c[0] + r * a.cos(), c[1] + r * a.sin()]
}
fn solved(s: &Sketch) -> Solution {
    solve(s, &HashMap::new()).unwrap()
}

fn check_approx(curve: &dyn Fn(f64) -> P2, pieces: &[Shape], tol: f64) {
    assert!(!pieces.is_empty());
    let pts = |s: &Shape| -> Vec<P2> {
        match *s {
            Shape::Line { a, b } => (0..=8)
                .map(|k| {
                    let t = k as f64 / 8.0;
                    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
                })
                .collect(),
            Shape::Arc {
                center,
                radius,
                start,
                end,
            } => {
                let sw = kmb_core::kernel::arc_sweep(start, end);
                (0..=8)
                    .map(|k| polar(center, radius, start + sw * k as f64 / 8.0))
                    .collect()
            }
            _ => panic!("線分と円弧だけのはず: {s:?}"),
        }
    };
    // 曲線を細かい折れ線にして、そこまでの距離を測る(弦の誤差は 1e-4 mm 未満)
    let n = 2000;
    let dense: Vec<P2> = (0..=n).map(|k| curve(k as f64 / n as f64)).collect();
    let seg = |a: P2, b: P2, x: P2| {
        let d = [b[0] - a[0], b[1] - a[1]];
        let l2 = d[0] * d[0] + d[1] * d[1];
        let t = if l2 > 0.0 {
            (((x[0] - a[0]) * d[0] + (x[1] - a[1]) * d[1]) / l2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        (a[0] + d[0] * t - x[0]).hypot(a[1] + d[1] * t - x[1])
    };
    for s in pieces {
        for p in pts(s) {
            let d = dense
                .windows(2)
                .map(|w| seg(w[0], w[1], p))
                .fold(f64::INFINITY, f64::min);
            assert!(d <= tol + 1e-3 * tol + 1e-4, "{d} > {tol}: {s:?}");
        }
    }
}

#[test]
fn approximations_stay_within_tolerance_and_close_up() {
    let e = Shape::Ellipse {
        center: [5.0, 1.0],
        major: [30.0, 10.0],
        ratio: 0.3,
        start: 0.0,
        end: TAU,
    };
    let m: P2 = [30.0, 10.0];
    let n: P2 = [-3.0, 9.0];
    let pieces = approximate(&e);
    check_approx(
        &|t| {
            let t = t * TAU;
            [
                5.0 + m[0] * t.cos() + n[0] * t.sin(),
                1.0 + m[1] * t.cos() + n[1] * t.sin(),
            ]
        },
        &pieces,
        APPROX_TOL,
    );
    // 入れると閉じた領域になる。ソルバは円弧の数の 4 乗ほどで遅くなるので、
    // ここでは 1/5 に縮めて弧の数を減らす
    let mut sk = Sketch::default();
    let small = Place {
        scale: 0.2,
        ..Default::default()
    };
    sk.insert_shapes(&[e], small);
    let sol = solved(&sk);
    let (profile, rejected) = profile_of(&sk, &sol, Plane::XY);
    assert!(rejected.is_empty());
    assert_eq!(profile.regions.len(), 1);
    let area = profile.regions[0].outer.signed_area();
    let want = PI * 1000.0 * 0.3 * 0.04;
    assert!(close(area, want, want * 1e-3), "{area} vs {want}");

    let b = [[0.0, 0.0], [10.0, 20.0], [30.0, -20.0], [40.0, 0.0]];
    let pieces = approximate(&Shape::Bezier { p: b });
    check_approx(
        &|t| {
            let u = 1.0 - t;
            let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
            [
                (0..4).map(|k| w[k] * b[k][0]).sum(),
                (0..4).map(|k| w[k] * b[k][1]).sum(),
            ]
        },
        &pieces,
        APPROX_TOL,
    );
    // 両端を固定した 3 次の B スプライン(節点 0 0 0 0 1 1 1 1)は同じベジエ
    let bs = Shape::BSpline {
        degree: 3,
        control: b.to_vec(),
        knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        weights: vec![],
    };
    let pieces2 = approximate(&bs);
    assert_eq!(pieces.len(), pieces2.len());
    // 端と中央の位置が同じ(ほぼまっすぐな弧の中心は計算の誤差で大きく動くので比べない)
    let ends = |s: &Shape| -> [P2; 3] {
        match *s {
            Shape::Line { a, b } => [a, [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0], b],
            Shape::Arc {
                center,
                radius,
                start,
                end,
            } => {
                let sw = kmb_core::kernel::arc_sweep(start, end);
                [
                    polar(center, radius, start),
                    polar(center, radius, start + sw / 2.0),
                    polar(center, radius, end),
                ]
            }
            _ => panic!("{s:?}"),
        }
    };
    for (a, b) in pieces.iter().zip(&pieces2) {
        let (p, q) = (ends(a), ends(b));
        assert!((0..3).all(|k| close2(p[k], q[k], 1e-6)), "{a:?} {b:?}");
    }
    // 有理の 2 次で 4 分の 1 円(重み 1, √2/2, 1)
    let quarter = Shape::BSpline {
        degree: 2,
        control: vec![[10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        weights: vec![1.0, 0.5f64.sqrt(), 1.0],
    };
    for s in approximate(&quarter) {
        let Shape::Arc { center, radius, .. } = s else {
            panic!("{s:?}")
        };
        assert!(close2(center, [0.0, 0.0], 1e-3) && close(radius, 10.0, 1e-3));
    }
    // 壊れた B スプラインは空
    let bad = Shape::BSpline {
        degree: 3,
        control: b.to_vec(),
        knots: vec![0.0, 1.0],
        weights: vec![],
    };
    assert!(approximate(&bad).is_empty());
}

#[test]
fn insert_places_with_offset_scale_and_rotation() {
    let shapes = [
        Shape::Arc {
            center: [1.0, 0.0],
            radius: 1.0,
            start: 0.0,
            end: FRAC_PI_2,
        },
        Shape::Point { p: [2.0, 0.0] },
    ];
    let mut s = Sketch::default();
    let place = Place {
        offset: [10.0, 0.0],
        scale: 2.0,
        rotation: FRAC_PI_2,
    };
    let ids = s.insert_shapes(&shapes, place);
    assert_eq!(ids.len(), 2);
    let sol = solved(&s);
    let segs = entity_segments(&s, &sol, false);
    let Segment::Arc { center, radius, .. } = segs[0].1 else {
        panic!()
    };
    assert!(close2(center, [10.0, 2.0], 1e-12) && close(radius, 2.0, 1e-12));
    assert!(close2(segs[0].1.start_point(), [10.0, 4.0], 1e-9));
    assert!(close2(segs[0].1.end_point(), [8.0, 2.0], 1e-9));
    assert!(close2(sol.points[&ids[1]], [10.0, 4.0], 1e-9));
}
