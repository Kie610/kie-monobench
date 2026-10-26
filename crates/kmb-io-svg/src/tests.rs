//! SVG の読み込みの試験。

use crate::*;
use kmb_sketch::*;

use kmb_core::kernel::{P2, Plane};
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
/// 角度が 2π の違いを除いて等しい
fn same_angle(a: f64, b: f64) -> bool {
    let d = (a - b).rem_euclid(TAU);
    d < 1e-9 || TAU - d < 1e-9
}

fn solved(s: &Sketch) -> Solution {
    solve(s, &HashMap::new()).unwrap()
}

const SVG_PATH: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!-- 手で書いた見本 -->
<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" "http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd">
<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="50mm" viewBox="0 0 100 50">
  <style><![CDATA[ path { stroke: <black> } ]]></style>
  <path d="M 10 10 L 30 10 C 30 20 40 20 40 30 A 10 10 0 0 1 20 30 Z" fill="none"/>
</svg>"#;

#[test]
fn svg_path_becomes_mm_with_y_up() {
    let imp = from_svg(SVG_PATH).unwrap();
    assert!(imp.skipped.is_empty());
    let s = &imp.shapes;
    assert_eq!(s.len(), 4, "{s:#?}");
    assert_eq!(
        s[0],
        Shape::Line {
            a: [10.0, 40.0],
            b: [30.0, 40.0]
        }
    );
    assert_eq!(
        s[1],
        Shape::Bezier {
            p: [[30.0, 40.0], [30.0, 30.0], [40.0, 30.0], [40.0, 20.0]]
        }
    );
    // (40, 20) から (20, 20) へ、下の (30, 10) を通る半円
    let Shape::Arc {
        center,
        radius,
        start,
        end,
    } = s[2]
    else {
        panic!("{:?}", s[2]);
    };
    assert!(close2(center, [30.0, 20.0], 1e-9) && close(radius, 10.0, 1e-9));
    let sw = kmb_core::kernel::arc_sweep(start, end);
    assert!(close(sw, PI, 1e-9));
    assert!(close2(
        polar(center, radius, start + sw / 2.0),
        [30.0, 10.0],
        1e-9
    ));
    assert_eq!(
        s[3],
        Shape::Line {
            a: [20.0, 20.0],
            b: [10.0, 40.0]
        }
    );
    // 入れると閉じた領域になる
    let mut t = Sketch::default();
    t.insert_shapes(s, Place::default());
    let sol = solved(&t);
    let (profile, rejected) = profile_of(&t, &sol, Plane::XY);
    assert!(rejected.is_empty(), "{rejected:?}");
    assert_eq!(profile.regions.len(), 1);
}

#[test]
fn svg_rect_with_rounded_corners() {
    let text = r#"<svg width="100mm" height="100mm" viewBox="0 0 100 100">
        <rect x="10" y="10" width="40" height="20" rx="5"/></svg>"#;
    let imp = from_svg(text).unwrap();
    let s = &imp.shapes;
    assert_eq!(s.len(), 8);
    assert_eq!(
        s[0],
        Shape::Line {
            a: [15.0, 90.0],
            b: [45.0, 90.0]
        }
    );
    let Shape::Arc {
        center,
        radius,
        start,
        end,
    } = s[4]
    else {
        panic!("{:?}", s[4]);
    };
    // 右上の角: 中心 (45, 85)、0°〜90°
    assert!(close2(center, [45.0, 85.0], 1e-9) && close(radius, 5.0, 1e-9));
    assert!(same_angle(start, 0.0) && same_angle(end, FRAC_PI_2));
    let (lo, hi) = imp.bounds.unwrap();
    assert!(close2(lo, [10.0, 70.0], 1e-9) && close2(hi, [50.0, 90.0], 1e-9));
    let mut t = Sketch::default();
    t.insert_shapes(s, Place::default());
    let sol = solved(&t);
    let (profile, rejected) = profile_of(&t, &sol, Plane::XY);
    assert!(rejected.is_empty());
    let area = profile.regions[0].outer.signed_area();
    assert!(close(area, 800.0 - (4.0 - PI) * 25.0, 0.3), "{area}");
}

#[test]
fn svg_circle_in_px_and_cm_units() {
    // 単位の無い width は 96 dpi の px(96 px = 25.4 mm)
    let imp =
        from_svg(r#"<svg width="96" height="96"><circle cx="48" cy="24" r="9.6"/></svg>"#).unwrap();
    let Shape::Circle { center, radius } = imp.shapes[0] else {
        panic!("{:?}", imp.shapes);
    };
    assert!(close2(center, [12.7, 19.05], 1e-9) && close(radius, 2.54, 1e-9));
    // cm と viewBox(1 単位 = 10 mm)
    let imp = from_svg(
        r#"<svg width="10cm" height="5cm" viewBox="0 0 10 5"><circle cx="1" cy="1" r="0.5"/></svg>"#,
    )
    .unwrap();
    let Shape::Circle { center, radius } = imp.shapes[0] else {
        panic!("{:?}", imp.shapes);
    };
    assert!(close2(center, [10.0, 40.0], 1e-9) && close(radius, 5.0, 1e-9));
}

#[test]
fn svg_group_and_element_transforms() {
    let text = r#"<svg xmlns:svg="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100">
      <g transform="translate(10,20) scale(2)">
        <line x1="0" y1="0" x2="5" y2="0" transform="rotate(90)"/>
        <g transform="matrix(1 0 0 1 5 5)"><svg:polyline points="0,0 1,0 1,1"/></g>
      </g>
      <circle r="10" transform="scale(2,1)"/>
      <path d="M0 0 h10" transform="skewX(45)"/>
      <path d="M0 10 h10" transform="skewY(45)"/>
      <path d="M0 0 l10 0" transform="rotate(90 10 0)"/>
    </svg>"#;
    let imp = from_svg(text).unwrap();
    let s = &imp.shapes;
    assert_eq!(s.len(), 7, "{s:#?}");
    let Shape::Line { a, b } = s[0] else { panic!() };
    assert!(close2(a, [10.0, 80.0], 1e-9) && close2(b, [10.0, 70.0], 1e-9));
    // (5,5)+(0,0) → 2 倍 → (10,20) ずらし → (20,30)。y 反転で (20,70)
    let Shape::Line { a, b } = s[1] else { panic!() };
    assert!(close2(a, [20.0, 70.0], 1e-9) && close2(b, [22.0, 70.0], 1e-9));
    let Shape::Ellipse { major, ratio, .. } = s[3] else {
        panic!("{:?}", s[3])
    };
    assert!(close(major[0].hypot(major[1]), 20.0, 1e-9) && close(ratio, 0.5, 1e-9));
    // skewX は y = 0 の線を動かさない
    let Shape::Line { a, b } = s[4] else { panic!() };
    assert!(close2(a, [0.0, 100.0], 1e-9) && close2(b, [10.0, 100.0], 1e-9));
    // skewY(45): (x, y) → (x, y + x)
    let Shape::Line { a, b } = s[5] else { panic!() };
    assert!(close2(a, [0.0, 90.0], 1e-9) && close2(b, [10.0, 80.0], 1e-9));
    // (10, 0) のまわりに 90°: (0,0) → (10,-10)、(10,0) → (10,0)
    let Shape::Line { a, b } = s[6] else { panic!() };
    assert!(close2(a, [10.0, 110.0], 1e-9) && close2(b, [10.0, 100.0], 1e-9));
}

#[test]
fn svg_path_commands_relative_and_smooth() {
    let text = r#"<svg width="100mm" height="100mm" viewBox="0 0 100 100"><path d="
        m10,10 20,0 h5 v5 H40 V20 l-5 5
        c0,10 10,10 10,0 s10-10 10,0
        q5 5 10 0 t10 0
        a5 5 0 1 0 10 0
        Q 80 20 90 20 T 95 30 S 98 40 99 50 z
        M 1 1 Z"/></svg>"#;
    let imp = from_svg(text).unwrap();
    let mut n = (0, 0, 0);
    for s in &imp.shapes {
        match s {
            Shape::Line { .. } => n.0 += 1,
            Shape::Bezier { .. } => n.1 += 1,
            Shape::Arc { .. } => n.2 += 1,
            other => panic!("{other:?}"),
        }
    }
    // 線: m の続き・h・v・H・V・l・z の 7 本。曲線: c s q t Q T S の 7 本。弧 1
    assert_eq!(n, (7, 7, 1), "{:#?}", imp.shapes);
    // s の最初の制御点は、前の c の 2 つ目の制御点を反転したもの
    let Shape::Bezier { p } = imp.shapes[7] else {
        panic!()
    };
    // c の終点 (45, 25)、2 つ目の制御点 (45, 35)。反転して (45, 15)。y 反転で (45, 85)
    assert!(close2(p[0], [45.0, 75.0], 1e-9) && close2(p[1], [45.0, 85.0], 1e-9));
    // 大きい弧の旗: 半径 5 で弦 10 は半円
    let Some(Shape::Arc { radius, .. }) =
        imp.shapes.iter().find(|s| matches!(s, Shape::Arc { .. }))
    else {
        panic!()
    };
    assert!(close(*radius, 5.0, 1e-9));
}

#[test]
fn svg_counts_unsupported_and_hides_defs() {
    let text = r##"<svg width="10mm" height="10mm" viewBox="0 0 10 10">
      <defs><circle id="c" r="1"/></defs>
      <text x="1" y="1">こんにちは <tspan>世界</tspan></text>
      <image href="a.png"/><image href="b.png"/>
      <use href="#c"/>
      <g display="none"><line x1="0" y1="0" x2="1" y2="1"/></g>
      <ellipse cx="5" cy="5" rx="2" ry="1"/>
    </svg>"##;
    let imp = from_svg(text).unwrap();
    assert_eq!(imp.shapes.len(), 1);
    assert!(matches!(imp.shapes[0], Shape::Ellipse { ratio, .. } if close(ratio, 0.5, 1e-12)));
    assert_eq!(
        imp.skipped,
        vec![
            ("image".to_string(), 2),
            ("text".to_string(), 1),
            ("use".to_string(), 1)
        ]
    );
}

// ---- 書き出し ----

/// 長方形(穴の円つき)と、円弧と線分で閉じた半円。補助線を 1 本(DXF の試験と同じ形)。
fn export_sample() -> Sketch {
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

#[test]
fn exported_svg_is_sized_in_mm_with_closed_black_outlines_for_lightburn() {
    let s = export_sample();
    let text = to_svg(&s, &solved(&s), &SvgOptions::default()).unwrap();
    // 大きさは mm、1 単位 = 1 mm(読む側の dpi に左右されない)
    assert!(
        text.contains("width=\"70mm\" height=\"20mm\" viewBox=\"0 0 70 20\""),
        "{text}"
    );
    assert!(!text.contains("transform"));
    // 線だけで塗らない。色は LightBurn の層 00 の黒。補助線は既定で書かない
    assert!(text.contains("fill=\"none\" stroke=\"#000000\""));
    assert!(!text.contains("#0000FF"));
    // 長方形と半円は、それぞれ 1 本の閉じた path。円は circle
    assert_eq!(text.matches("<path ").count(), 2, "{text}");
    assert_eq!(text.matches(" Z\"").count(), 2, "{text}");
    assert_eq!(text.matches("<circle ").count(), 1);
}

#[test]
fn exported_svg_reads_back_to_the_same_shapes_and_regions() {
    let s = export_sample();
    let sol = solved(&s);
    let text = to_svg(&s, &sol, &SvgOptions::default()).unwrap();
    let imp = from_svg(&text).unwrap();
    assert!(imp.skipped.is_empty(), "{:?}", imp.skipped);
    let (lo, hi) = imp.bounds.unwrap();
    assert!(
        close2(lo, [0.0, 0.0], 1e-6) && close2(hi, [70.0, 20.0], 1e-6),
        "{lo:?} {hi:?}"
    );
    let mut t = Sketch::default();
    t.insert_shapes(&imp.shapes, Place::default());
    let sol2 = solved(&t);
    // 半円の円弧は、元と同じく (70, 0) と (50, 0) を結び、上の (60, 10) を通る
    let arc = entity_segments(&t, &sol2, false)
        .into_iter()
        .find_map(|(_, seg)| match seg {
            kmb_core::kernel::Segment::Arc {
                center,
                radius,
                start,
                end,
            } => Some((center, radius, start, end)),
            _ => None,
        })
        .expect("円弧が戻る");
    assert!(close2(arc.0, [60.0, 0.0], 1e-6) && close(arc.1, 10.0, 1e-6));
    let mid = arc.2 + kmb_core::kernel::arc_sweep(arc.2, arc.3) / 2.0;
    assert!(close2(polar(arc.0, arc.1, mid), [60.0, 10.0], 1e-6));
    let (profile, rejected) = profile_of(&t, &sol2, Plane::XY);
    assert!(rejected.is_empty(), "{rejected:?}");
    assert_eq!(profile.regions.len(), 2);
    assert_eq!(
        profile.regions.iter().map(|r| r.holes.len()).sum::<usize>(),
        1
    );
}

#[test]
fn construction_lines_are_blue_dashed_only_when_asked() {
    let s = export_sample();
    let opts = SvgOptions {
        construction: true,
        title: "板 <1>".into(),
    };
    let text = to_svg(&s, &solved(&s), &opts).unwrap();
    assert!(text.contains("stroke=\"#0000FF\" stroke-width=\"0.1\" stroke-dasharray"));
    assert!(text.contains("<title>板 &lt;1&gt;</title>"));
    let imp = from_svg(&text).unwrap();
    assert_eq!(imp.shapes.len(), 8);
}

#[test]
fn an_empty_sketch_has_nothing_to_write() {
    let s = Sketch::default();
    assert!(to_svg(&s, &solved(&s), &SvgOptions::default()).is_none());
}

fn assert_bad_svg_scale(text: &str) {
    match from_svg(text) {
        Ok(imported) => panic!(
            "誤った倍率を受け入れた: bounds={:?}, shapes={:?}",
            imported.bounds, imported.shapes
        ),
        Err(reason) => assert!(reason.contains("倍率"), "{reason}"),
    }
}

#[test]
fn svg_scale_rejects_zero_viewbox() {
    assert_bad_svg_scale(
        r#"<svg width="10mm" height="10mm" viewBox="0 0 0 10"><path d="M0 0 L10 10"/></svg>"#,
    );
}

#[test]
fn svg_scale_rejects_huge_page() {
    assert_bad_svg_scale(
        r#"<svg width="1e9mm" height="1e9mm" viewBox="0 0 10 10"><path d="M0 0 L10 10"/></svg>"#,
    );
}

#[test]
fn svg_scale_rejects_huge_transform() {
    assert_bad_svg_scale(r#"<svg><path transform="scale(1e12)" d="M0 0 L10 10"/></svg>"#);
}

#[test]
fn svg_scale_rejects_collapsing_transform() {
    assert_bad_svg_scale(r#"<svg><path transform="scale(0)" d="M0 0 L10 10"/></svg>"#);
}

#[test]
fn svg_scale_rejects_nonfinite_values() {
    for value in ["NaN", "inf", "1e999"] {
        assert_bad_svg_scale(&format!(
            r#"<svg><path transform="scale({value})" d="M0 0 L10 10"/></svg>"#
        ));
    }
}

#[test]
fn svg_scale_rejects_nested_growth() {
    assert_bad_svg_scale(
        r#"<svg><g transform="scale(1e6)"><g transform="scale(1e6)"><path d="M0 0 L10 10"/></g></g></svg>"#,
    );
}

#[test]
fn svg_scale_rejects_tiny_and_negative_values() {
    for value in ["1e-12", "-1"] {
        assert_bad_svg_scale(&format!(
            r#"<svg><path transform="scale({value})" d="M0 0 L10 10"/></svg>"#
        ));
    }
}

#[test]
fn svg_scale_checks_page_dimensions_and_complete_transform_arguments() {
    for attr in ["width", "height"] {
        for value in [
            "0", "-1mm", "NaN", "inf", "1e999mm", "1e9mm", "1e-12mm", "wrong",
        ] {
            assert_bad_svg_scale(&format!(
                r#"<svg {attr}="{value}"><path d="M0 0 L10 10"/></svg>"#
            ));
        }
    }
    for viewbox in [
        "0 0 -1 10",
        "0 0 10 0",
        "0 0 NaN 10",
        "0 0 10 10 extra",
        "0 0 1e999 10",
    ] {
        assert_bad_svg_scale(&format!(
            r#"<svg viewBox="{viewbox}"><path d="M0 0 L10 10"/></svg>"#
        ));
    }
    for transform in [
        "scale(1 NaN)",
        "scale(1,-1)",
        "scale(1,0)",
        "scale(1) wrong",
        "matrix(0 0 0 1 0 0)",
        "translate(inf 0)",
    ] {
        assert_bad_svg_scale(&format!(
            r#"<svg><path transform="{transform}" d="M0 0 L10 10"/></svg>"#
        ));
    }
}

#[test]
fn svg_scale_checks_curves_before_subdivision_and_all_nested_products() {
    for shape in [
        r#"<path d="M0 0 C0 10 10 10 10 0"/>"#,
        r#"<ellipse rx="10" ry="5"/>"#,
        r#"<circle r="10"/>"#,
    ] {
        assert_bad_svg_scale(&format!(
            r#"<svg><g transform="scale(1e12)">{shape}</g></svg>"#
        ));
    }
    for scale in ["1e200", "1e-200"] {
        assert_bad_svg_scale(&format!(
            r#"<svg><g transform="scale({scale})"><g transform="scale({scale})"><path d="M0 0 L10 10"/></g></g></svg>"#
        ));
    }
}

#[test]
fn svg_curve_subdivision_uses_mm_after_scaling() {
    let read = |shape: &str| {
        from_svg(&format!(
            r#"<svg width="100mm" height="100mm" viewBox="0 0 100 100">{shape}</svg>"#
        ))
        .unwrap()
    };
    let a = read(r#"<path d="M10 10 C10 80 70 80 70 10"/>"#);
    let b = read(
        r#"<path transform="scale(1000000)" d="M0.00001 0.00001 C0.00001 0.00008 0.00007 0.00008 0.00007 0.00001"/>"#,
    );
    let (a, b) = (approximate(&a.shapes[0]), approximate(&b.shapes[0]));
    assert_eq!(a.len(), b.len(), "同じ mm の曲線は同じ数に細分する");
    let (a, b) = (shapes_bounds(&a).unwrap(), shapes_bounds(&b).unwrap());
    assert!(close2(a.0, b.0, APPROX_TOL) && close2(a.1, b.1, APPROX_TOL));
}

#[test]
fn svg_scale_error_is_a_typed_input_problem_without_document_changes() {
    use kmb_core::{
        Context, DocumentData, Extension, NullKernel, ProblemKind, RecomputeReport, Registry,
    };
    use std::sync::Arc;
    let path = std::env::temp_dir().join(format!("svg-scale-{}.svg", std::process::id()));
    std::fs::write(
        &path,
        r#"<svg><path transform="scale(0)" d="M0 0 L10 10"/></svg>"#,
    )
    .unwrap();
    let data = DocumentData::default();
    let report = RecomputeReport::default();
    let registry = Registry::new();
    let kernel: Arc<dyn kmb_core::Kernel> = Arc::new(NullKernel);
    let mut cx = Context::new(&data, &report, &registry, &kernel);
    cx.file = Some(&path);
    let result = SvgCommands.run(INSERT_SVG, &cx).unwrap().unwrap_err();
    std::fs::remove_file(path).unwrap();
    assert_eq!(result.kind, ProblemKind::Input);
    assert!(result.message.contains("倍率"));
    assert!(data.features.is_empty());
}
