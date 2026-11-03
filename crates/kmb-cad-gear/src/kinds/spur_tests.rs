//! 確かめること: 歯先円・歯底円の径、ピッチ円の上の歯の厚さ、輪が閉じて自分と交わらないこと、
//! 区間の名前、切り下げの注意、入力の誤り。

use super::*;
use kmb_core::{ProfileEdge, ProfileLoop};

fn gear(teeth: u32, x: f64) -> Outline {
    outline(&Params {
        module: 2.0,
        teeth,
        profile_shift: x,
        ..Params::default()
    })
    .unwrap()
}

fn points(edges: &[ProfileEdge]) -> Vec<P2> {
    edges.iter().flat_map(|e| e.polyline(720)).collect()
}

#[test]
fn tip_and_root_circles_bound_the_outline() {
    for (z, x) in [(12, 0.0), (20, 0.0), (30, 0.3), (50, -0.2)] {
        let o = gear(z, x);
        let r = |p: &P2| p[0].hypot(p[1]);
        let pts = points(&o.edges);
        let hi = pts.iter().map(r).fold(0.0, f64::max);
        let lo = pts.iter().map(r).fold(f64::INFINITY, f64::min);
        let d = 2.0 * z as f64;
        assert!(
            (hi - (d / 2.0 + 2.0 * (1.0 + x))).abs() < 1e-6,
            "{z} 歯先 {hi}"
        );
        assert!(
            (lo - (d / 2.0 - 2.0 * (1.25 - x))).abs() < 1e-6,
            "{z} 歯底 {lo}"
        );
    }
}

#[test]
fn the_tooth_is_half_a_pitch_thick_on_the_pitch_circle() {
    let o = gear(20, 0.0);
    let r = 20.0;
    // 1 枚目の歯の右の歯面がピッチ円を横切る角
    let cross = o
        .edges
        .iter()
        .filter(|e| e.name.starts_with("t0:r") && e.name[4..].chars().all(|c| c.is_ascii_digit()))
        .flat_map(|e| e.polyline(200_000))
        .min_by(|a, b| {
            (a[0].hypot(a[1]) - r)
                .abs()
                .total_cmp(&(b[0].hypot(b[1]) - r).abs())
        })
        .unwrap();
    let half = -cross[1].atan2(cross[0]);
    let thickness = 2.0 * half * r;
    assert!(
        (thickness - PI * 2.0 / 2.0).abs() < 2e-3,
        "歯の厚さ {thickness}"
    );
}

#[test]
fn the_outline_is_one_closed_ring_that_does_not_cross_itself() {
    for z in [12, 17, 45] {
        let o = gear(z, 0.0);
        let n = o.edges.len();
        for i in 0..n {
            let a = o.edges[i].to_point();
            let b = o.edges[(i + 1) % n].from_point();
            assert!(
                (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-7,
                "{z}: {} と {} が離れている",
                o.edges[i].name,
                o.edges[(i + 1) % n].name
            );
        }
        let ring = ProfileLoop {
            edges: o.edges.clone(),
        };
        assert!(ring.signed_area() > 0.0, "反時計回り");
        let p = ring.polyline();
        let m = p.len();
        let cross = |a: P2, b: P2, c: P2, d: P2| {
            let o =
                |p: P2, q: P2, r: P2| (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]);
            o(a, b, c) * o(a, b, d) < -1e-18 && o(c, d, a) * o(c, d, b) < -1e-18
        };
        for i in 0..m {
            for j in i + 2..m {
                if i == 0 && j == m - 1 {
                    continue;
                }
                assert!(
                    !cross(p[i], p[(i + 1) % m], p[j], p[(j + 1) % m]),
                    "{z}: 自分と交わる {i} {j}"
                );
            }
        }
    }
}

#[test]
fn edge_names_come_from_the_tooth_number_and_side() {
    let o = gear(12, 0.0);
    let names: std::collections::BTreeSet<&str> = o.edges.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names.len(), o.edges.len(), "名前が重ならない");
    for i in 0..12 {
        for part in ["tip", "root", "r0", "l0", "rfillet", "lfillet"] {
            assert!(
                names.contains(format!("t{i}:{part}").as_str()),
                "t{i}:{part}"
            );
        }
    }
    // 歯数 12 は基礎円が歯底円より外なので、半径の向きの線でつなぐ
    assert!(names.contains("t0:rneck"));
    // 歯数 45 は歯底円が基礎円より外なので、線は無い
    let o = gear(45, 0.0);
    assert!(!o.edges.iter().any(|e| e.name.ends_with("neck")));
}

#[test]
fn few_teeth_warn_of_undercut_and_bad_inputs_are_errors() {
    assert!(
        gear(12, 0.0)
            .warnings
            .iter()
            .any(|w| w.contains("切り下げ"))
    );
    assert!(
        gear(20, 0.0).warnings.is_empty(),
        "{:?}",
        gear(20, 0.0).warnings
    );
    assert!(
        gear(12, 0.5)
            .warnings
            .iter()
            .all(|w| !w.contains("切り下げ"))
    );
    let bad = |p: Params| outline(&p).unwrap_err();
    assert!(
        bad(Params {
            module: 0.0,
            ..Params::default()
        })
        .contains("モジュール")
    );
    assert!(
        bad(Params {
            teeth: 2,
            ..Params::default()
        })
        .contains("歯数")
    );
    assert!(
        bad(Params {
            profile_shift: 3.0,
            ..Params::default()
        })
        .contains("とがります")
    );
}

#[test]
fn rounds_at_the_tip_are_tangent_to_the_tip_circle() {
    let o = outline(&Params {
        module: 2.0,
        teeth: 20,
        tip_round: 0.2,
        ..Params::default()
    })
    .unwrap();
    assert!(o.warnings.is_empty(), "{:?}", o.warnings);
    let round = o.edges.iter().find(|e| e.name == "t0:rround").unwrap();
    let kmb_core::Segment::Arc { center, radius, .. } = round.segment else {
        panic!("円弧")
    };
    assert!((radius - 0.4).abs() < 1e-9);
    assert!((center[0].hypot(center[1]) + radius - 22.0).abs() < 1e-6);
}
