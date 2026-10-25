//! 文字(kmb-text)をスケッチの点・線・円に沿わせる通しの試験。

use crate::tests::*;
use kmb_core::{Document, FeatureStatus, Op};
use kmb_samples::sample::{DocBuilder, SketchBuilder};
use kmb_sketch::SketchPlane;

#[test]
fn text_follows_the_sketch_point_it_is_anchored_to() {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    b.param("px", "30 mm");
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let o = s.fixed_point(0.0, 0.0);
    s.point_from(o, "px", "10 mm", [30.0, 10.0]);
    let sk = b.feature(kmb_sketch::SKETCH, "スケッチ", s.build());
    let t = b.feature(
        kmb_text::TEXT,
        "文字",
        kmb_text::Text {
            text: "HI".into(),
            anchor: Some(sk),
            align: kmb_text::HAlign::Center,
            valign: kmb_text::VAlign::Middle,
            angle: Some("90 deg".into()),
            ..Default::default()
        },
    );
    let mut doc = b.finish("文字");
    let reg = registry();
    let center = |doc: &mut Document| {
        let report = doc.recompute(&reg, &*k);
        assert_ok(&report);
        let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(t) else {
            unreachable!()
        };
        let frame = kmb_core::Curves::from_value(&outputs["frame"]).unwrap();
        assert_eq!(frame.segments.len(), 4);
        let mut c = [0.0; 2];
        for seg in &frame.segments {
            let p = seg.end_point();
            c = [c[0] + p[0] / 4.0, c[1] + p[1] / 4.0];
        }
        c
    };
    let c = center(&mut doc);
    assert!(
        (c[0] - 30.0).abs() < 0.6 && (c[1] - 10.0).abs() < 0.6,
        "{c:?}"
    );
    let px = doc.data().param_by_name("px").unwrap().id;
    doc.apply(kmb_core::Transaction::new(
        "動かす",
        vec![Op::SetParamExpr {
            id: px,
            expr: "45 mm".into(),
        }],
    ))
    .unwrap();
    let c = center(&mut doc);
    assert!((c[0] - 45.0).abs() < 0.6, "{c:?}");
}

#[test]
fn text_follows_connected_lines_around_a_corner() {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    // コの字の折れ線(下の辺 → 右の辺 → 上の辺)
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let p = [
        s.fixed_point(0.0, 0.0),
        s.fixed_point(40.0, 0.0),
        s.fixed_point(40.0, 30.0),
        s.fixed_point(0.0, 30.0),
    ];
    for w in p.windows(2) {
        s.line(w[0], w[1]);
    }
    let sk = b.feature(kmb_sketch::SKETCH, "コの字", s.build());
    let text = |chain| kmb_text::Text {
        text: "AROUND THE CORNER".into(),
        height: "3 mm".into(),
        path: Some(sk),
        x: "25 mm".into(),
        chain,
        ..Default::default()
    };
    let one = b.feature(kmb_text::TEXT, "1 本", text(false));
    let all = b.feature(kmb_text::TEXT, "つながり", text(true));
    let mut doc = b.finish("折れ線に沿う文字");
    let reg = registry();
    let report = doc.recompute(&reg, &*k);
    let outputs = |id| {
        let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(id) else {
            panic!("{:?}", report.feature(id))
        };
        outputs.clone()
    };
    // 1 本だけなら、下の辺の延長にはみ出す
    let o = outputs(one);
    assert_eq!(
        kmb_core::warnings_of(&o).len(),
        1,
        "{:?}",
        kmb_core::warnings_of(&o)
    );
    // つながりをたどれば、右の辺を上る(字は進む向きの左 = コの字の内側)
    let o = outputs(all);
    assert!(
        kmb_core::warnings_of(&o).is_empty(),
        "{:?}",
        kmb_core::warnings_of(&o)
    );
    let c = kmb_core::Curves::from_value(&o["curves"]).unwrap();
    let pts: Vec<[f64; 2]> = c
        .segments
        .iter()
        .flat_map(|s| [s.start_point(), s.end_point()])
        .collect();
    assert!(pts.iter().all(|q| q[0] < 40.6), "右の辺の外に出ない");
    assert!(
        pts.iter().any(|q| q[1] > 5.0 && q[0] > 36.0),
        "右の辺に沿う字がある"
    );
}

#[test]
fn text_follows_a_construction_circle_drawn_in_a_sketch() {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    b.param("d", "60 mm");
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let o = s.fixed_point(0.0, 0.0);
    s.circle(o, "d", 30.0);
    let mut sketch = s.build();
    // 補助線の円でも沿わせられる(彫刻はなぞらない)
    for e in &mut sketch.entities {
        if let kmb_sketch::Entity::Circle { construction, .. } = e {
            *construction = true;
        }
    }
    let sk = b.feature(kmb_sketch::SKETCH, "円", sketch);
    let t = b.feature(
        kmb_text::TEXT,
        "文字",
        kmb_text::Text {
            text: "JISAKU".into(),
            path: Some(sk),
            reverse: true,
            align: kmb_text::HAlign::Center,
            y: "1 mm".into(),
            ..Default::default()
        },
    );
    let mut doc = b.finish("円に沿う文字");
    let reg = registry();
    let radii = |doc: &mut Document| {
        let report = doc.recompute(&reg, &*k);
        assert_ok(&report);
        let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(t) else {
            unreachable!()
        };
        let c = kmb_core::Curves::from_value(&outputs["curves"]).unwrap();
        let mut lo = f64::MAX;
        let mut hi = f64::MIN;
        let mut top = f64::MIN;
        for seg in &c.segments {
            for p in [seg.start_point(), seg.end_point()] {
                let r = p[0].hypot(p[1]);
                lo = lo.min(r);
                hi = hi.max(r);
                top = top.max(p[1]);
            }
        }
        (lo, hi, top)
    };
    let (lo, hi, top) = radii(&mut doc);
    // 円の 1 mm 外から、大文字の高さ 5 mm まで。真上にある
    assert!(lo > 30.9 && hi < 36.6 && top > 35.0, "{lo} {hi} {top}");
    let d = doc.data().param_by_name("d").unwrap().id;
    doc.apply(kmb_core::Transaction::new(
        "円を大きく",
        vec![Op::SetParamExpr {
            id: d,
            expr: "80 mm".into(),
        }],
    ))
    .unwrap();
    let (lo, hi, _) = radii(&mut doc);
    assert!(lo > 40.9 && hi < 46.6, "{lo} {hi}");
}
