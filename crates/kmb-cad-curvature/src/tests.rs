//! 確かめること: くしの曲率と棘の向きが教科書の値と合い、辺の両側の面のつながりの区分が
//! 形どおりに分かれること(媒介変数の取り方によらないことも)。

use super::*;
use kmb_core::kernel::P3;

const EPS: f64 = 1e-9;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-7 * (1.0 + b.abs())
}

fn circle(r: f64, t: f64, speed: f64) -> CurveJet {
    // 媒介変数を speed 倍の速さで進める(t·speed が角度)
    let a = t * speed;
    CurveJet {
        p: [r * a.cos(), r * a.sin(), 0.0],
        d1: [-r * speed * a.sin(), r * speed * a.cos(), 0.0],
        d2: [
            -r * speed * speed * a.cos(),
            -r * speed * speed * a.sin(),
            0.0,
        ],
    }
}

fn sine(amp: f64, x: f64) -> CurveJet {
    CurveJet {
        p: [x, amp * x.sin(), 0.0],
        d1: [1.0, amp * x.cos(), 0.0],
        d2: [0.0, -amp * x.sin(), 0.0],
    }
}

fn dist(a: P3, b: P3) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

#[test]
fn a_circle_has_the_reciprocal_of_its_radius_and_spines_point_outward() {
    let jets: Vec<_> = (0..12).map(|i| circle(10.0, i as f64 * 0.5, 1.0)).collect();
    let c = comb(&jets, Some([0.0, 0.0, 1.0]), CombScale::Fixed(20.0));
    assert_eq!(c.spines.len(), 12);
    for s in &c.spines {
        // 反時計回りの円は左へ曲がるので正。棘は中心と反対(外)へ 0.1 × 20 = 2 mm
        assert!(close(s.curvature, 0.1), "{}", s.curvature);
        assert!(close(dist(s.tip, [0.0; 3]), 12.0));
    }
    assert!(close(c.min_radius().unwrap(), 10.0));
}

#[test]
fn the_curvature_does_not_depend_on_how_fast_the_parameter_runs() {
    let slow = curvature(&circle(4.0, 0.3, 1.0)).unwrap().0;
    let fast = curvature(&circle(4.0, 0.15, 2.0)).unwrap().0;
    assert!(close(slow, 0.25) && close(fast, 0.25));
}

#[test]
fn a_helix_matches_the_textbook_curvature() {
    // (r cos t, r sin t, c t) の曲率は r / (r² + c²)
    let (r, c, t) = (5.0_f64, 2.0_f64, 0.7_f64);
    let j = CurveJet {
        p: [r * t.cos(), r * t.sin(), c * t],
        d1: [-r * t.sin(), r * t.cos(), c],
        d2: [-r * t.cos(), -r * t.sin(), 0.0],
    };
    let (k, toward) = curvature(&j).unwrap();
    assert!(close(k, r / (r * r + c * c)));
    // 主法線は軸へ水平に向かう
    let n = toward.unwrap();
    assert!(close(n[0], -t.cos()) && close(n[1], -t.sin()) && n[2].abs() < EPS);
    // 空間の曲線のくしは、棘を軸から外へ立てる
    let comb = comb(&[j], None, CombScale::Fit(3.0));
    assert!(close(dist(comb.spines[0].tip, j.p), 3.0));
    let tip = comb.spines[0].tip;
    assert!(tip[0].hypot(tip[1]) > r);
}

#[test]
fn the_spines_cross_the_curve_at_an_inflection() {
    // y = sin x は x = π で変曲する。前は右へ、後は左へ曲がる
    let before = sine(1.0, std::f64::consts::PI - 0.5);
    let after = sine(1.0, std::f64::consts::PI + 0.5);
    let c = comb(
        &[before, after],
        Some([0.0, 0.0, 1.0]),
        CombScale::Fixed(1.0),
    );
    let (b, a) = (c.spines[0], c.spines[1]);
    assert!(b.curvature < 0.0 && a.curvature > 0.0);
    // 前(右へ曲がり、中心は下)は棘が上へ、後(左へ曲がり、中心は上)は棘が下へ
    assert!(b.tip[1] > b.at[1] && a.tip[1] < a.at[1]);
    // 符号つきの曲率は y'' / (1 + y'²)^1.5
    let x: f64 = std::f64::consts::PI + 0.5;
    assert!(close(
        a.curvature,
        -x.sin() / (1.0 + x.cos().powi(2)).powf(1.5)
    ));
}

#[test]
fn fit_makes_the_longest_spine_the_given_length_and_a_line_has_none() {
    let jets: Vec<_> = (0..20).map(|i| sine(2.0, i as f64 * 0.3)).collect();
    let c = comb(&jets, Some([0.0, 0.0, 1.0]), CombScale::Fit(5.0));
    let longest = c
        .spines
        .iter()
        .map(|s| dist(s.at, s.tip))
        .fold(0.0, f64::max);
    assert!(close(longest, 5.0));
    assert_eq!(c.envelope.len(), c.spines.len());

    let line: Vec<_> = (0..3)
        .map(|i| CurveJet {
            p: [i as f64, 0.0, 0.0],
            d1: [1.0, 0.0, 0.0],
            d2: [0.0; 3],
        })
        .collect();
    let c = comb(&line, None, CombScale::Fit(5.0));
    assert_eq!(c.min_radius(), None);
    assert!(c.spines.iter().all(|s| s.tip == s.at));
}

#[test]
fn a_point_where_the_parameter_stops_is_skipped_not_guessed() {
    let stop = CurveJet {
        p: [0.0; 3],
        d1: [0.0; 3],
        d2: [1.0, 0.0, 0.0],
    };
    let c = comb(&[stop, circle(1.0, 0.0, 1.0)], None, CombScale::Fixed(1.0));
    assert_eq!((c.spines.len(), c.skipped), (1, 1));
}

// ---- 面 ----

/// z = h の平面(外向き +z)。媒介変数は (x, y) を `s` 倍した物。
fn plane_at(x: f64, h: f64, s: f64) -> SurfaceJet {
    SurfaceJet {
        p: [x, 0.0, h],
        su: [s, 0.0, 0.0],
        sv: [0.0, s, 0.0],
        suu: [0.0; 3],
        suv: [0.0; 3],
        svv: [0.0; 3],
        outward: [0.0, 0.0, 1.0],
    }
}

/// x 軸に平行で中心 (y, z) = (0, −r) の円筒の、真上(z = 0)の点。外向きの法線は軸から外へ。
/// 媒介変数 v は角度を `s` 倍した物。
fn cylinder_top(x: f64, r: f64, s: f64) -> SurfaceJet {
    let v = std::f64::consts::FRAC_PI_2;
    SurfaceJet {
        p: [x, r * v.cos(), -r + r * v.sin()],
        su: [1.0, 0.0, 0.0],
        sv: [0.0, -r * s * v.sin(), r * s * v.cos()],
        suu: [0.0; 3],
        suv: [0.0; 3],
        svv: [0.0, -r * s * s * v.cos(), -r * s * s * v.sin()],
        outward: [0.0, 0.0, 1.0],
    }
}

fn edge(x: f64, a: SurfaceJet, b: SurfaceJet) -> EdgeSample {
    EdgeSample {
        p: [x, 0.0, 0.0],
        tangent: [1.0, 0.0, 0.0],
        faces: [a, b],
    }
}

#[test]
fn a_cylinder_bends_away_from_its_outward_normal_only_around_the_axis() {
    let c = cylinder_top(0.0, 4.0, 1.0);
    assert!(close(normal_curvature(&c, [0.0, 1.0, 0.0]).unwrap(), -0.25));
    assert!(normal_curvature(&c, [1.0, 0.0, 0.0]).unwrap().abs() < EPS);
    // 45° の向きはオイラーの式どおり半分
    assert!(close(
        normal_curvature(&c, [1.0, 1.0, 0.0]).unwrap(),
        -0.125
    ));
    // 裏から見る(外向きを −z にする)と符号が変わる
    let inside = SurfaceJet {
        outward: [0.0, 0.0, -1.0],
        ..c
    };
    assert!(close(
        normal_curvature(&inside, [0.0, 1.0, 0.0]).unwrap(),
        0.25
    ));
}

#[test]
fn a_round_on_a_flat_face_is_tangent_but_not_curvature_continuous() {
    let tol = Tolerances::default();
    let s = sample_continuity(
        &edge(1.0, plane_at(1.0, 0.0, 1.0), cylinder_top(1.0, 3.0, 1.0)),
        &tol,
    );
    assert_eq!(s.class, Continuity::Tangent);
    assert!(s.angle_deg < 1e-6 && s.gap < EPS);
    let [a, b] = s.curvature.unwrap();
    assert!(a.abs() < EPS && close(b, -1.0 / 3.0));
    assert!(close(s.curvature_diff().unwrap(), 1.0 / 3.0));
}

#[test]
fn the_same_cylinder_split_in_two_is_curvature_continuous_whatever_the_parameters() {
    let tol = Tolerances::default();
    let s = sample_continuity(
        &edge(
            0.0,
            cylinder_top(0.0, 3.0, 1.0),
            cylinder_top(0.0, 3.0, 2.5),
        ),
        &tol,
    );
    assert_eq!(s.class, Continuity::Curvature);
    // 平面どうしもそろう(どちらも曲率 0)
    let s = sample_continuity(
        &edge(0.0, plane_at(0.0, 0.0, 1.0), plane_at(0.0, 0.0, 7.0)),
        &tol,
    );
    assert_eq!(s.class, Continuity::Curvature);
}

#[test]
fn a_sharp_edge_a_gap_and_a_broken_face_are_told_apart() {
    let tol = Tolerances::default();
    // 上の面(+z)と横の面(+y)の角は 90°
    let side = SurfaceJet {
        p: [0.0; 3],
        su: [1.0, 0.0, 0.0],
        sv: [0.0, 0.0, 1.0],
        suu: [0.0; 3],
        suv: [0.0; 3],
        svv: [0.0; 3],
        outward: [0.0, 1.0, 0.0],
    };
    let s = sample_continuity(&edge(0.0, plane_at(0.0, 0.0, 1.0), side), &tol);
    assert_eq!(s.class, Continuity::Position);
    assert!(close(s.angle_deg, 90.0));
    // 0.01 mm 離れた面は隙間
    let s = sample_continuity(
        &edge(0.0, plane_at(0.0, 0.0, 1.0), plane_at(0.0, 0.01, 1.0)),
        &tol,
    );
    assert_eq!(s.class, Continuity::Gap);
    // 偏微分が平行で法線の無い面は、区分を決めない
    let broken = SurfaceJet {
        sv: [2.0, 0.0, 0.0],
        ..plane_at(0.0, 0.0, 1.0)
    };
    let s = sample_continuity(&edge(0.0, plane_at(0.0, 0.0, 1.0), broken), &tol);
    assert_eq!(s.class, Continuity::Unknown);
}

#[test]
fn the_angle_tolerance_decides_tangent_or_not() {
    let tilted = |deg: f64| {
        let a = deg.to_radians();
        SurfaceJet {
            sv: [0.0, a.cos(), a.sin()],
            outward: [0.0, -a.sin(), a.cos()],
            ..plane_at(0.0, 0.0, 1.0)
        }
    };
    let tol = Tolerances::default();
    let flat = plane_at(0.0, 0.0, 1.0);
    assert_eq!(
        sample_continuity(&edge(0.0, flat, tilted(0.4)), &tol).class,
        Continuity::Curvature
    );
    assert_eq!(
        sample_continuity(&edge(0.0, flat, tilted(0.6)), &tol).class,
        Continuity::Position
    );
}

#[test]
fn an_edge_takes_its_worst_point() {
    let tol = Tolerances::default();
    let good = edge(
        0.0,
        cylinder_top(0.0, 3.0, 1.0),
        cylinder_top(0.0, 3.0, 1.0),
    );
    let round = edge(1.0, plane_at(1.0, 0.0, 1.0), cylinder_top(1.0, 3.0, 1.0));
    let e = edge_continuity(&[good, round, good], &tol);
    assert_eq!(e.class, Continuity::Tangent);
    assert_eq!(e.samples.len(), 3);
    assert!(close(e.max_curvature_diff.unwrap(), 1.0 / 3.0));
    assert_eq!(edge_continuity(&[], &tol).class, Continuity::Unknown);
}
