//! DXF の読み書きの試験(読み込みの壊れた入力の試験は SVG も一緒に)。

use crate::*;
use kmb_sketch::*;

use kmb_core::kernel::{P2, Plane, Segment};
use std::collections::HashMap;
use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

fn inserted_test_sketch(bytes: &[u8]) -> Sketch {
    inserted_test_result(bytes, serde_json::Value::Null).0
}

fn inserted_test_result(bytes: &[u8], args: serde_json::Value) -> (Sketch, String) {
    let result = inserted_test_outcome(bytes, args);
    let kmb_core::Outcome::Change { edits, label } = result.unwrap() else {
        panic!("スケッチの挿入")
    };
    let kmb_core::Edit::Insert { data, .. } = &edits[0] else {
        panic!("新しいスケッチ")
    };
    (serde_json::from_value(data.clone()).unwrap(), label)
}

fn inserted_test_outcome(
    bytes: &[u8],
    args: serde_json::Value,
) -> Result<kmb_core::Outcome, kmb_core::Problem> {
    use kmb_core::{Context, DocumentData, Extensions, NullKernel, RecomputeReport, Registry};
    use std::sync::Arc;
    let path = std::env::temp_dir().join(format!(
        "dxf-upgrade-{}-{}.dxf",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(&path, bytes).unwrap();
    let data = DocumentData::default();
    let report = RecomputeReport::default();
    let registry = Registry::new();
    let kernel: Arc<dyn kmb_core::Kernel> = Arc::new(NullKernel);
    let mut cx = Context::new(&data, &report, &registry, &kernel);
    cx.file = Some(&path);
    cx.args = &args;
    let result = Extensions(vec![Arc::new(DxfCommands)]).run(INSERT_DXF, &cx);
    std::fs::remove_file(&path).unwrap();
    result
}

#[test]
fn nearly_vertical_reversed_lines_merge_despite_coordinate_noise() {
    let cleaned = clean(
        &[
            Shape::Line {
                a: [0., 0.],
                b: [1e-17, 1.],
            },
            Shape::Line {
                a: [-1e-17, 1.],
                b: [0., 0.],
            },
        ],
        0.01,
    )
    .unwrap();
    assert_eq!(cleaned.removed, 1);
    assert_eq!(cleaned.shapes.len(), 1);
}

#[test]
fn invalid_cleanup_tolerance_is_rejected_even_when_cleanup_is_disabled() {
    let text = line_dxf(&[([0., 0.], [1., 1.])]);
    for cleanup in [false, true] {
        for tolerance in [0., -1., 0.0000005, 2.] {
            let result = inserted_test_outcome(
                text.as_bytes(),
                serde_json::json!({"cleanup":cleanup,"cleanup_tolerance":tolerance}),
            );
            let error = result.expect_err("整理を止めても指定した許容差を確かめる");
            assert_eq!(error.kind, kmb_core::ProblemKind::Input);
            assert!(error.message.contains("許容差"), "{error}");
        }
    }
}

#[test]
fn layer_line_breaks_are_reported_without_panicking() {
    let sketch = sample_sketch();
    let sol = solved(&sketch);
    for construction in [false, true] {
        let mut options = DxfOptions::default();
        if construction {
            options.construction_layer = "線\n不正な行".into();
        } else {
            options.layer = "線\n不正な行".into();
        }
        let result = std::panic::catch_unwind(|| to_dxf(&sketch, &sol, &options));
        let error = result
            .expect("画層名の改行をpanicにしない")
            .expect_err("改行を含む画層名は誤りにする");
        assert!(
            error.contains("画層名") && error.contains("改行"),
            "{error}"
        );
    }
}

#[test]
fn dxf_cleaning_can_be_disabled_and_reports_its_counts() {
    let bytes = line_dxf(&[
        ([0., 0.], [1., 0.]),
        ([1., 0.], [0., 0.]),
        ([1.005, 0.], [1., 1.]),
    ]);
    let (clean, label) = inserted_test_result(bytes.as_bytes(), serde_json::json!({}));
    assert_eq!(
        clean
            .entities
            .iter()
            .filter(|e| matches!(e, Entity::Line { .. }))
            .count(),
        2
    );
    assert!(
        label.contains("重複を 1 本除去") && label.contains("端点を 1 組接続"),
        "{label}"
    );
    let (raw, label) = inserted_test_result(bytes.as_bytes(), serde_json::json!({"cleanup":false}));
    assert_eq!(
        raw.entities
            .iter()
            .filter(|e| matches!(e, Entity::Line { .. }))
            .count(),
        3
    );
    assert_eq!(label, "DXF を挿入");
}

#[test]
fn cleanup_keeps_short_details_and_does_not_chain_endpoint_moves() {
    let shapes = vec![
        Shape::Line {
            a: [0., 0.],
            b: [1., 0.],
        },
        Shape::Line {
            a: [1.005, 0.],
            b: [1.005, 1.],
        },
        Shape::Line {
            a: [1.015, 0.],
            b: [2., 1.],
        },
    ];
    let cleaned = clean(&shapes, 0.01).unwrap();
    assert_eq!(cleaned.joined, 1);
    assert_eq!(cleaned.shapes[2], shapes[2]);
    let short = vec![
        Shape::Line {
            a: [0., 0.],
            b: [0.005, 0.],
        },
        Shape::Line {
            a: [0., 0.],
            b: [0., 1.],
        },
    ];
    assert_eq!(clean(&short, 0.01).unwrap().shapes, short);
    for tol in [0., -1., f64::NAN, f64::INFINITY, 2.] {
        assert!(clean(&shapes, tol).is_err());
    }
}

#[test]
fn cleaning_connects_free_curve_ends_using_the_sketch_approximation() {
    let shapes = vec![
        Shape::Bezier {
            p: [[0., 0.], [0.3, 0.], [0.7, 0.], [1., 0.]],
        },
        Shape::Line {
            a: [1.005, 0.],
            b: [1., 1.],
        },
    ];
    let cleaned = clean(&shapes, 0.01).unwrap();
    assert_eq!(cleaned.joined, 1);
    assert_eq!(
        cleaned.shapes,
        vec![
            Shape::Line {
                a: [0., 0.],
                b: [1., 0.]
            },
            Shape::Line {
                a: [1., 0.],
                b: [1., 1.]
            }
        ]
    );
}

#[test]
fn cleaning_does_not_treat_a_full_arc_as_an_open_endpoint() {
    let shapes = vec![
        Shape::Arc {
            center: [0., 0.],
            radius: 1.,
            start: 0.,
            end: TAU,
        },
        Shape::Line {
            a: [1.005, 0.],
            b: [2., 0.],
        },
    ];
    let cleaned = clean(&shapes, 0.01).unwrap();
    assert_eq!(cleaned.joined, 0);
    assert_eq!(cleaned.shapes, shapes);
}

fn line_dxf(lines: &[(P2, P2)]) -> String {
    let mut text = "0\nSECTION\n2\nENTITIES\n".to_string();
    for (a, b) in lines {
        text.push_str(&format!(
            "0\nLINE\n10\n{}\n20\n{}\n11\n{}\n21\n{}\n",
            a[0], a[1], b[0], b[1]
        ));
    }
    text.push_str("0\nENDSEC\n0\nEOF\n");
    text
}

#[test]
fn dxf_upgrade_merges_reversed_and_partly_overlapping_lines() {
    let sketch = inserted_test_sketch(
        line_dxf(&[
            ([0., 0.], [10., 0.]),
            ([10., 0.], [0., 0.]),
            ([5., 0.], [15., 0.]),
        ])
        .as_bytes(),
    );
    let lines: Vec<_> = sketch
        .entities
        .iter()
        .filter(|e| matches!(e, Entity::Line { .. }))
        .collect();
    assert_eq!(lines.len(), 1, "重複と重なった部分は一本にする");
    let points: Vec<_> = sketch
        .entities
        .iter()
        .filter_map(|e| {
            if let Entity::Point { x, y, .. } = e {
                Some([*x, *y])
            } else {
                None
            }
        })
        .collect();
    assert!(points.contains(&[0., 0.]) && points.contains(&[15., 0.]));
}

#[test]
fn dxf_upgrade_closes_only_endpoint_gaps_within_tolerance() {
    let sketch = inserted_test_sketch(
        line_dxf(&[
            ([0., 0.], [1., 0.]),
            ([1.005, 0.], [1., 1.]),
            ([1.03, 1.], [2., 1.]),
        ])
        .as_bytes(),
    );
    let lines: Vec<_> = sketch
        .entities
        .iter()
        .filter_map(|e| {
            if let Entity::Line { a, b, .. } = e {
                Some((*a, *b))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(lines[0].1, lines[1].0, "0.005 mm の隙間を閉じる");
    assert_ne!(lines[1].1, lines[2].0, "0.03 mm の隙間は残す");
}

#[test]
fn dxf_upgrade_binary_insertion_matches_ascii() {
    let text = line_dxf(&[([1.25, 2.5], [4.5, 8.75])]);
    let binary = binary_dxf(&text, true);
    let ascii = inserted_test_sketch(text.as_bytes());
    let read = inserted_test_sketch(&binary);
    assert_eq!(ascii.entities, read.entities);
}

fn binary_dxf(text: &str, modern: bool) -> Vec<u8> {
    let mut binary = b"AutoCAD Binary DXF\r\n\x1a\0".to_vec();
    let lines: Vec<_> = text.lines().collect();
    for pair in lines.as_chunks::<2>().0 {
        let code: u16 = pair[0].trim().parse().unwrap();
        if modern {
            binary.extend(code.to_le_bytes());
        } else if code < 255 {
            binary.push(code as u8);
        } else {
            binary.push(255);
            binary.extend(code.to_le_bytes());
        }
        if (10..60).contains(&code) || (140..148).contains(&code) {
            binary.extend(pair[1].parse::<f64>().unwrap().to_le_bytes());
        } else if (60..80).contains(&code)
            || (170..180).contains(&code)
            || (270..290).contains(&code)
            || (370..373).contains(&code)
        {
            binary.extend(pair[1].parse::<i16>().unwrap().to_le_bytes());
        } else {
            binary.extend(pair[1].as_bytes());
            binary.push(0);
        }
    }
    binary
}

#[test]
fn binary_code_widths_and_truncated_values_are_checked() {
    let sketch = sample_sketch();
    let text = to_dxf(&sketch, &solved(&sketch), &DxfOptions::default()).unwrap();
    let ascii = from_dxf(&text).unwrap();
    for modern in [true, false] {
        assert_eq!(
            from_dxf_bytes(&binary_dxf(&text, modern)).unwrap().shapes,
            ascii.shapes
        );
    }
    let binary = binary_dxf(&line_dxf(&[([0., 0.], [1., 1.])]), true);
    for length in 0..binary.len() {
        assert!(
            from_dxf_bytes(&binary[..length]).is_err(),
            "途中で切れた {length} バイト"
        );
    }
}

#[test]
fn dxf_upgrade_writes_r2000_handles_and_subclasses() {
    let sketch = sample_sketch();
    let text = to_dxf(&sketch, &solved(&sketch), &DxfOptions::default()).unwrap();
    assert!(text.contains("AC1015"), "R2000 の版を書く");
    assert!(
        text.contains("AcDbSymbolTableRecord")
            && text.contains("AcDbEntity")
            && text.contains("AcDbLine")
    );
    let lines: Vec<_> = text.lines().collect();
    let handles: Vec<_> = lines
        .as_chunks::<2>()
        .0
        .iter()
        .filter(|p| p[0].trim() == "5")
        .map(|p| p[1])
        .collect();
    assert!(!handles.is_empty());
    assert_eq!(
        handles.len(),
        handles
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
    );
    assert_eq!(from_dxf(&text).unwrap().shapes.len(), 7);
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}
fn close2(a: P2, b: P2, tol: f64) -> bool {
    close(a[0], b[0], tol) && close(a[1], b[1], tol)
}
fn polar(c: P2, r: f64, a: f64) -> P2 {
    [c[0] + r * a.cos(), c[1] + r * a.sin()]
}
/// 角度が 2π の違いを除いて等しい
fn same_angle(a: f64, b: f64) -> bool {
    let d = (a - b).rem_euclid(TAU);
    d < 1e-9 || TAU - d < 1e-9
}

fn solved(s: &Sketch) -> Solution {
    solve(s, &HashMap::new()).unwrap()
}

/// 長方形(穴の円つき)と、円弧と線分で閉じた半円。補助線を 1 本。
fn sample_sketch() -> Sketch {
    let mut s = Sketch::default();
    let p: Vec<EntityId> = [[0.0, 0.0], [40.0, 0.0], [40.0, 20.0], [0.0, 20.0]]
        .iter()
        .map(|q| s.add_point(q[0], q[1]))
        .collect();
    for k in 0..4 {
        s.add_line(p[k], p[(k + 1) % 4]);
    }
    let c = s.add_point(20.0, 10.0);
    s.add_circle(c, 5.0);
    let ac = s.add_point(60.0, 0.0);
    let a0 = s.add_point(70.0, 0.0);
    let a1 = s.add_point(50.0, 0.0);
    s.add_arc(ac, a0, a1);
    s.add_line(a1, a0);
    let k = s.add_line(p[0], p[2]);
    s.toggle_construction(k);
    s
}

fn kinds(segs: &[(EntityId, Segment)]) -> (usize, usize, usize) {
    let mut n = (0, 0, 0);
    for (_, s) in segs {
        match s {
            Segment::Line { .. } => n.0 += 1,
            Segment::Circle { .. } => n.1 += 1,
            Segment::Arc { .. } => n.2 += 1,
        }
    }
    n
}

fn same_segment(a: &Segment, b: &Segment, tol: f64) -> bool {
    match (a, b) {
        (Segment::Line { a: p, b: q }, Segment::Line { a: r, b: s }) => {
            close2(*p, *r, tol) && close2(*q, *s, tol)
        }
        (
            Segment::Circle {
                center: c,
                radius: r,
            },
            Segment::Circle {
                center: d,
                radius: q,
            },
        ) => close2(*c, *d, tol) && close(*r, *q, tol),
        (Segment::Arc { center: c, .. }, Segment::Arc { center: d, .. }) => {
            close2(*c, *d, tol)
                && close2(a.start_point(), b.start_point(), tol)
                && close2(a.end_point(), b.end_point(), tol)
        }
        _ => false,
    }
}

#[test]
fn dxf_round_trip_keeps_shapes_and_closes_regions() {
    let s = sample_sketch();
    let sol = solved(&s);
    let text = to_dxf(&s, &sol, &DxfOptions::default()).unwrap();
    assert!(text.contains("AC1015") && text.contains("$INSUNITS"));
    assert!(
        !text.contains("CONSTRUCTION\n 10"),
        "補助線は既定で書かない"
    );
    let imp = from_dxf(&text).unwrap();
    assert!(imp.skipped.is_empty(), "{:?}", imp.skipped);
    assert_eq!(imp.shapes.len(), 7);
    let (lo, hi) = imp.bounds.unwrap();
    assert!(close2(lo, [0.0, 0.0], 1e-9) && close2(hi, [70.0, 20.0], 1e-9));

    let mut t = Sketch::default();
    let ids = t.insert_shapes(&imp.shapes, Place::default());
    assert_eq!(ids.len(), 7);
    assert!(t.constraints.is_empty(), "拘束は足さない");
    // 端を共有する: 長方形 4 点 + 円の中心 + 弧の中心 + 弧の端 2 点
    let points = t
        .entities
        .iter()
        .filter(|e| matches!(e, Entity::Point { .. }))
        .count();
    assert_eq!(points, 8);

    let sol2 = solved(&t);
    let a = entity_segments(&s, &sol, false);
    let b = entity_segments(&t, &sol2, false);
    assert_eq!(kinds(&a), kinds(&b));
    assert_eq!(kinds(&b), (5, 1, 1));
    for ((_, x), (_, y)) in a.iter().zip(&b) {
        assert!(same_segment(x, y, 1e-6), "{x:?} != {y:?}");
    }
    let (profile, rejected) = profile_of(&t, &sol2, Plane::XY);
    assert!(rejected.is_empty(), "{rejected:?}");
    assert_eq!(profile.regions.len(), 2);
    assert_eq!(
        profile.regions.iter().map(|r| r.holes.len()).sum::<usize>(),
        1
    );
}

#[test]
fn dxf_construction_lines_on_their_own_layer() {
    let s = sample_sketch();
    let sol = solved(&s);
    let opts = DxfOptions {
        construction: true,
        points: true,
        ..Default::default()
    };
    let text = to_dxf(&s, &sol, &opts).unwrap();
    let imp = from_dxf(&text).unwrap();
    assert_eq!(imp.shapes.len(), 8);
    assert_eq!(
        text.matches("\nCONSTRUCTION\n").count(),
        2,
        "画層の表と線 1 本"
    );
    assert_eq!(text.matches("\nSKETCH\n").count(), 8, "画層の表と線 7 本");
}

/// 手で書いた見本の DXF を組み立てる(組の並びから)
fn dxf(header: &[(i32, &str)], entities: &[(i32, &str)]) -> String {
    let mut out = String::new();
    let mut put = |c: i32, v: &str| out.push_str(&format!("{c:>3}\n{v}\n"));
    put(0, "SECTION");
    put(2, "HEADER");
    for (c, v) in header {
        put(*c, v);
    }
    put(0, "ENDSEC");
    put(0, "SECTION");
    put(2, "ENTITIES");
    for (c, v) in entities {
        put(*c, v);
    }
    put(0, "ENDSEC");
    put(0, "EOF");
    out
}

#[test]
fn lwpolyline_bulge_arcs_have_right_direction_and_radius() {
    // 正方形の右の辺を、外へ膨らむ半円(bulge = 1、反時計回り)にした閉じた折れ線
    let text = dxf(
        &[],
        &[
            (0, "LWPOLYLINE"),
            (8, "0"),
            (90, "4"),
            (70, "1"),
            (10, "0"),
            (20, "0"),
            (10, "10"),
            (20, "0"),
            (42, "1"),
            (10, "10"),
            (20, "10"),
            (10, "0"),
            (20, "10"),
            // 時計回りの 90° の弧(bulge = -tan 22.5°)
            (0, "LWPOLYLINE"),
            (90, "2"),
            (70, "0"),
            (10, "0"),
            (20, "0"),
            (42, "-0.41421356237309503"),
            (10, "10"),
            (20, "0"),
        ],
    );
    let imp = from_dxf(&text).unwrap();
    assert_eq!(imp.shapes.len(), 5);
    let Shape::Arc {
        center,
        radius,
        start,
        end,
    } = imp.shapes[1]
    else {
        panic!("{:?}", imp.shapes[1]);
    };
    assert!(close2(center, [10.0, 5.0], 1e-9) && close(radius, 5.0, 1e-9));
    assert!(same_angle(start, -FRAC_PI_2) && same_angle(end, FRAC_PI_2));
    // 右へ膨らむ(弧の中央が x = 15)
    assert!(close2(
        polar(center, radius, 0.5 * (start + end)),
        [15.0, 5.0],
        1e-9
    ));

    let Shape::Arc {
        center,
        radius,
        start,
        end,
    } = imp.shapes[4]
    else {
        panic!("{:?}", imp.shapes[4]);
    };
    assert!(close2(center, [5.0, -5.0], 1e-9));
    assert!(close(radius, 50f64.sqrt(), 1e-9));
    assert!(same_angle(start, FRAC_PI_4) && same_angle(end, 3.0 * FRAC_PI_4));

    // 閉じた領域になり、面積は 100 + 半円
    let mut t = Sketch::default();
    t.insert_shapes(&imp.shapes[..4], Place::default());
    let sol = solved(&t);
    let (profile, rejected) = profile_of(&t, &sol, Plane::XY);
    assert!(rejected.is_empty());
    assert_eq!(profile.regions.len(), 1);
    let area = profile.regions[0].outer.signed_area();
    // 面積は円弧を折れ線にして測るので、少し小さめに出る
    assert!(close(area, 100.0 + PI * 12.5, 0.2), "{area}");
}

#[test]
fn dxf_inches_become_mm() {
    let text = dxf(
        &[(9, "$INSUNITS"), (70, "1")],
        &[
            (0, "LINE"),
            (10, "0"),
            (20, "0"),
            (11, "1"),
            (21, "2"),
            (0, "CIRCLE"),
            (10, "1"),
            (20, "1"),
            (40, "0.5"),
        ],
    );
    let imp = from_dxf(&text).unwrap();
    assert_eq!(
        imp.shapes,
        vec![
            Shape::Line {
                a: [0.0, 0.0],
                b: [25.4, 50.8]
            },
            Shape::Circle {
                center: [25.4, 25.4],
                radius: 12.7
            }
        ]
    );
    // 単位が無ければ mm
    let text = dxf(&[], &[(0, "POINT"), (10, "3"), (20, "4")]);
    assert_eq!(
        from_dxf(&text).unwrap().shapes,
        vec![Shape::Point { p: [3.0, 4.0] }]
    );
}

#[test]
fn dxf_counts_unreadable_entities() {
    let text = dxf(
        &[],
        &[
            (0, "TEXT"),
            (1, "abc"),
            (0, "HATCH"),
            (0, "HATCH"),
            (0, "MTEXT"),
            (0, "INSERT"),
            (2, "NOPE"),
            (0, "LINE"),
            (10, "x"),
            (0, "LINE"),
            (67, "1"),
            (10, "0"),
            (11, "1"),
        ],
    );
    let imp = from_dxf(&text).unwrap();
    assert!(imp.shapes.is_empty());
    assert!(imp.bounds.is_none());
    assert_eq!(
        imp.skipped,
        vec![
            ("HATCH".to_string(), 2),
            ("INSERT".to_string(), 1),
            ("LINE".to_string(), 1),
            ("MTEXT".to_string(), 1),
            ("TEXT".to_string(), 1),
            ("ペーパー空間の要素".to_string(), 1),
        ]
    );
}

#[test]
fn dxf_reads_polyline_ellipse_spline_ocs_and_blocks() {
    let text = "  0\nSECTION\n  2\nBLOCKS\n  0\nBLOCK\n  2\nB\n 70\n0\n 10\n1\n 20\n0\n\
  0\nLINE\n 10\n1\n 20\n0\n 11\n2\n 21\n0\n\
  0\nCIRCLE\n 10\n1\n 20\n0\n 40\n1\n\
  0\nENDBLK\n  0\nENDSEC\n\
  0\nSECTION\n  2\nENTITIES\n\
  0\nPOLYLINE\n 66\n1\n 70\n1\n\
  0\nVERTEX\n 10\n0\n 20\n0\n  0\nVERTEX\n 10\n4\n 20\n0\n  0\nVERTEX\n 10\n0\n 20\n3\n  0\nSEQEND\n\
  0\nELLIPSE\n 10\n0\n 20\n0\n 30\n0\n 11\n10\n 21\n0\n 31\n0\n 40\n0.5\n 41\n0\n 42\n6.283185307179586\n\
  0\nSPLINE\n 70\n8\n 71\n3\n 72\n8\n 73\n4\n\
 40\n0\n 40\n0\n 40\n0\n 40\n0\n 40\n1\n 40\n1\n 40\n1\n 40\n1\n\
 10\n0\n 20\n0\n 10\n1\n 20\n2\n 10\n3\n 20\n2\n 10\n4\n 20\n0\n\
  0\nARC\n 10\n10\n 20\n0\n 40\n5\n 50\n0\n 51\n90\n 210\n0\n 220\n0\n 230\n-1\n\
  0\nINSERT\n  2\nB\n 10\n10\n 20\n5\n 41\n2\n 42\n2\n 50\n90\n\
  0\nINSERT\n  2\nB\n 10\n0\n 20\n0\n 41\n2\n 42\n1\n\
  0\nENDSEC\n  0\nEOF\n";
    let imp = from_dxf(text).unwrap();
    assert!(imp.skipped.is_empty(), "{:?}", imp.skipped);
    let s = &imp.shapes;
    assert_eq!(s.len(), 3 + 1 + 1 + 1 + 2 + 2, "{s:#?}");
    assert_eq!(
        s[2],
        Shape::Line {
            a: [0.0, 3.0],
            b: [0.0, 0.0]
        }
    );
    assert!(matches!(s[3], Shape::Ellipse { ratio, .. } if ratio == 0.5));
    assert!(matches!(&s[4], Shape::BSpline { degree: 3, control, .. } if control.len() == 4));
    // 押し出しが -Z の弧は x が反転し、90°〜180° になる
    let Shape::Arc {
        center,
        radius,
        start,
        end,
    } = s[5]
    else {
        panic!("{:?}", s[5]);
    };
    assert!(close2(center, [-10.0, 0.0], 1e-12) && radius == 5.0);
    assert!(same_angle(start, FRAC_PI_2) && same_angle(end, PI));
    // 2 倍・90° 回して (10, 5) に置いたブロック(基点 (1, 0))
    let Shape::Line { a, b } = s[6] else {
        panic!("{:?}", s[6]);
    };
    assert!(close2(a, [10.0, 5.0], 1e-12) && close2(b, [10.0, 7.0], 1e-12));
    assert!(
        matches!(s[7], Shape::Circle { center, radius } if close2(center, [10.0, 5.0], 1e-12) && close(radius, 2.0, 1e-12))
    );
    // x だけ 2 倍のブロックの円は楕円になる
    let Shape::Ellipse {
        center,
        major,
        ratio,
        ..
    } = s[9]
    else {
        panic!("{:?}", s[9]);
    };
    assert!(close2(center, [0.0, 0.0], 1e-12));
    assert!(close(major[0].hypot(major[1]), 2.0, 1e-12) && close(ratio, 0.5, 1e-12));
}

/// 近似した線分と円弧が、元の曲線から許容差の中にあり、つながっている
#[test]
fn insert_into_existing_sketch_uses_new_ids() {
    let mut s = sample_sketch();
    let before = s.entities.len();
    let max = s.entities.iter().map(|e| e.id().0).max().unwrap();
    // 1000 本の線でできた閉じた多角形(番号を数え直さずに速く入る)
    let n = 1000;
    let shapes: Vec<Shape> = (0..n)
        .map(|k| {
            let a = TAU * k as f64 / n as f64;
            let b = TAU * (k + 1) as f64 / n as f64;
            Shape::Line {
                a: polar([100.0, 0.0], 10.0, a),
                b: polar([100.0, 0.0], 10.0, b),
            }
        })
        .collect();
    let ids = s.insert_shapes(&shapes, Place::default());
    assert_eq!(ids.len(), n);
    assert!(ids.iter().all(|id| id.0 > max));
    assert_eq!(s.entities.len(), before + 2 * n, "点は共有して n 個");
    let sol = solved(&s);
    let (profile, rejected) = profile_of(&s, &sol, Plane::XY);
    assert!(rejected.is_empty());
    assert_eq!(profile.regions.len(), 3);
    // 画層 0 に描く選び方
    let opts = DxfOptions {
        layer: "0".into(),
        ..Default::default()
    };
    let text = to_dxf(&s, &sol, &opts).unwrap();
    // 表の名前 1 つと、画層 0・補助線の 2 つ
    assert_eq!(text.matches("\nLAYER\n").count(), 3);
    assert_eq!(from_dxf(&text).unwrap().shapes.len(), 7 + n);
}

const SVG_PATH: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!-- 手で書いた見本 -->
<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" "http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd">
<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="50mm" viewBox="0 0 100 50">
  <style><![CDATA[ path { stroke: <black> } ]]></style>
  <path d="M 10 10 L 30 10 C 30 20 40 20 40 30 A 10 10 0 0 1 20 30 Z" fill="none"/>
</svg>"#;

#[test]
fn broken_inputs_are_errors_not_panics() {
    assert!(from_dxf("").is_err());
    assert!(from_dxf("hello\nworld\n").is_err());
    assert!(from_dxf("AutoCAD Binary DXF\r\n").is_err());
    assert!(kmb_io_svg::from_svg("").is_err());
    assert!(kmb_io_svg::from_svg("<html></html>").is_err());
    // 途中で切れていても、読めたところまで
    let cut = &SVG_PATH[..SVG_PATH.find("A 10").unwrap()];
    assert!(kmb_io_svg::from_svg(cut).is_ok());
    for text in [
        r#"<svg><path d="M 0 0 L 1e400 0 L 1 1 Q"/></svg>"#,
        r#"<svg><path d="+"/></svg>"#,
    ] {
        match kmb_io_svg::from_svg(text) {
            Ok(imp) => {
                assert!(imp.shapes.iter().all(|s| s.is_finite()));
                Sketch::default().insert_shapes(&imp.shapes, Place::default());
            }
            Err(reason) => assert!(
                reason.contains("倍率") || reason.contains("形の大きさが 0"),
                "{reason}"
            ),
        }
    }
    let text = dxf(&[], &[(0, "CIRCLE"), (10, "1e400"), (40, "1")]);
    assert_eq!(from_dxf(&text).unwrap().skipped, vec![("CIRCLE".into(), 1)]);
    // 自分を入れるブロックでも止まる
    let text = "  0\nSECTION\n  2\nBLOCKS\n  0\nBLOCK\n  2\nA\n\
  0\nINSERT\n  2\nA\n 70\n1000\n 71\n1000\n  0\nENDBLK\n  0\nENDSEC\n\
  0\nSECTION\n  2\nENTITIES\n  0\nINSERT\n  2\nA\n  0\nENDSEC\n  0\nEOF\n";
    assert!(from_dxf(text).is_ok());
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(200))]

    /// でたらめな文字列でパニックしない
    #[test]
    fn random_text_never_panics(s in ".{0,200}") {
        let _ = from_dxf(&s);
        let _ = kmb_io_svg::from_svg(&s);
    }

    /// 見本を途中で切ったり 1 文字変えたりしてもパニックしない
    #[test]
    fn mutated_samples_never_panic(cut in 0usize..2000, at in 0usize..2000, ch in proptest::char::any()) {
        let d = to_dxf(&sample_sketch(), &solved(&sample_sketch()), &DxfOptions::default()).unwrap();
        for base in [d.as_str(), SVG_PATH] {
            let mut chars: Vec<char> = base.chars().collect();
            if !chars.is_empty() {
                let k = at % chars.len();
                chars[k] = ch;
            }
            let m: String = chars.iter().collect();
            let cut = cut % (m.chars().count() + 1);
            let m2: String = m.chars().take(cut).collect();
            for t in [&m, &m2] {
                if let Ok(imp) = from_dxf(t) {
                    Sketch::default().insert_shapes(&imp.shapes, Place::default());
                }
                if let Ok(imp) = kmb_io_svg::from_svg(t) {
                    Sketch::default().insert_shapes(&imp.shapes, Place::default());
                }
            }
        }
    }

    /// でたらめな path の d でパニックせず、形は有限
    #[test]
    fn random_path_data(d in "[MmLlHhVvCcSsQqTtAaZz0-9 ,.eE+-]{0,80}") {
        let text = format!(r#"<svg viewBox="0 0 10 10"><path d="{d}"/></svg>"#);
        match kmb_io_svg::from_svg(&text) {
            Ok(imp) => {
                proptest::prop_assert!(imp.shapes.iter().all(|s| s.is_finite()));
                Sketch::default().insert_shapes(&imp.shapes, Place::default());
            }
            Err(reason) => proptest::prop_assert!(reason.contains("倍率") || reason.contains("形の大きさが 0")),
        }
    }
}

fn r2000_records(text: &str) -> Vec<Vec<(i32, &str)>> {
    let lines: Vec<_> = text.lines().collect();
    assert!(lines.len().is_multiple_of(2));
    let mut records: Vec<Vec<_>> = Vec::new();
    for p in lines.as_chunks::<2>().0 {
        let code = p[0].trim().parse().unwrap();
        if code == 0 || records.is_empty() {
            records.push(Vec::new());
        }
        records.last_mut().unwrap().push((code, p[1]));
    }
    records
}

#[test]
fn r2000_required_tables_have_named_records_and_an_empty_classes_section() {
    let sketch = sample_sketch();
    let text = to_dxf(&sketch, &solved(&sketch), &DxfOptions::default()).unwrap();
    let records = r2000_records(&text);
    let value = |record: &Vec<(i32, &str)>, code| {
        record.iter().find(|p| p.0 == code).map(|p| p.1.to_string())
    };
    let missing: Vec<_> = ["VPORT", "VIEW", "UCS", "APPID", "DIMSTYLE", "STYLE"]
        .into_iter()
        .filter(|table| {
            !records
                .iter()
                .any(|r| r[0].1 == "TABLE" && value(r, 2).as_deref() == Some(*table))
        })
        .collect();
    assert!(missing.is_empty(), "R2000の表が足りない: {missing:?}");
    let classes = records
        .iter()
        .position(|r| r[0].1 == "SECTION" && value(r, 2).as_deref() == Some("CLASSES"))
        .expect("CLASSES の区画");
    assert_eq!(records[classes + 1][0].1, "ENDSEC", "CLASSES は空");
    for (kind, name, class) in [
        ("VPORT", "*ACTIVE", "AcDbViewportTableRecord"),
        ("APPID", "ACAD", "AcDbRegAppTableRecord"),
        ("STYLE", "STANDARD", "AcDbTextStyleTableRecord"),
        ("DIMSTYLE", "STANDARD", "AcDbDimStyleTableRecord"),
    ] {
        let r = records
            .iter()
            .find(|r| r[0].1 == kind && value(r, 2).as_deref() == Some(name))
            .expect("既定の項目");
        assert!(r.contains(&(100, "AcDbSymbolTableRecord")) && r.contains(&(100, class)));
    }
    let viewport = records.iter().find(|r| r[0].1 == "VPORT").unwrap();
    let number = |code| value(viewport, code).unwrap().parse::<f64>().unwrap();
    assert!((number(12) - 35.0).abs() < 1e-9 && (number(22) - 10.0).abs() < 1e-9);
    assert!(
        (number(40) - 77.0).abs() < 1e-9,
        "図形の全体を既定の表示範囲に収める"
    );
    // 付け足す表で、線・円・円弧の読み戻しを妨げない。
    let read = from_dxf(&text).unwrap();
    assert!(read.skipped.is_empty());
    assert_eq!(read.shapes.len(), 7);
    let (lo, hi) = read.bounds.unwrap();
    assert!(close2(lo, [0.0, 0.0], 1e-9) && close2(hi, [70.0, 20.0], 1e-9));
}

#[test]
fn r2000_required_tables_keep_handles_unique_and_owners_and_text_style_resolvable() {
    for many in [false, true] {
        let mut sketch = sample_sketch();
        if many {
            for i in 0..220 {
                sketch.add_point(i as f64, 100.0);
            }
        }
        let text = to_dxf(
            &sketch,
            &solved(&sketch),
            &DxfOptions {
                points: many,
                ..Default::default()
            },
        )
        .unwrap();
        let records = r2000_records(&text);
        let mut handles = std::collections::BTreeMap::new();
        for record in &records {
            if record[0].1 == "SECTION" {
                continue;
            }
            for (_, handle) in record.iter().filter(|p| matches!(p.0, 5 | 105)) {
                assert!(
                    handles.insert(*handle, record).is_none(),
                    "ハンドルの重複: {handle}"
                );
            }
        }
        for record in &records {
            if let Some((_, owner)) = record.iter().find(|p| p.0 == 330) {
                assert!(
                    *owner == "0" || handles.contains_key(owner),
                    "持ち主が無い: {record:?}"
                );
            }
            if record[0].1 == "TABLE" {
                assert!(record.contains(&(100, "AcDbSymbolTable")));
                assert!(record.iter().any(|p| p.0 == 330));
            }
            if [
                "VPORT",
                "STYLE",
                "APPID",
                "DIMSTYLE",
                "LTYPE",
                "LAYER",
                "BLOCK_RECORD",
            ]
            .contains(&record[0].1)
            {
                let owner = record.iter().find(|p| p.0 == 330).unwrap().1;
                let parent = handles.get(owner).unwrap();
                assert_eq!(parent[0].1, "TABLE");
                assert!(parent.contains(&(2, record[0].1)));
            }
        }
        let dim = records
            .iter()
            .find(|r| r[0].1 == "DIMSTYLE")
            .expect("寸法スタイル");
        assert!(
            dim.iter().any(|p| p.0 == 105),
            "寸法スタイルのハンドルは105"
        );
        let style = dim
            .iter()
            .find(|p| p.0 == 340)
            .expect("文字スタイルへの参照")
            .1;
        let style = handles.get(style).unwrap();
        assert_eq!(style[0].1, "STYLE");
        assert!(style.contains(&(2, "STANDARD")));
        let header = records
            .iter()
            .find(|r| r[0].1 == "SECTION" && r.contains(&(2, "HEADER")))
            .unwrap();
        let pos = header.iter().position(|p| *p == (9, "$HANDSEED")).unwrap();
        assert_eq!(header[pos + 1].0, 5);
        let seed = u64::from_str_radix(header[pos + 1].1, 16).unwrap();
        let max = handles
            .keys()
            .map(|h| u64::from_str_radix(h, 16).unwrap())
            .max()
            .unwrap();
        assert!(seed > max, "次のハンドルを確保する");
        if many {
            assert!(max > 255, "多くの図形でも桁をまたいで重複しない");
        }
    }
}
