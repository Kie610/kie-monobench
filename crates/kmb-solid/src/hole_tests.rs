//! 穴(kmb-cad-hole)の種類と寸法の通しの試験。

use super::*;
use crate::tests::*;

fn cyl(r: f64, h: f64) -> f64 {
    std::f64::consts::PI * r * r * h
}

fn cone(r1: f64, r2: f64, h: f64) -> f64 {
    std::f64::consts::PI * h / 3.0 * (r1 * r1 + r1 * r2 + r2 * r2)
}

#[test]
fn counterbored_clearance_hole_takes_its_sizes_from_the_thread_table() {
    let k = TestKernel::new();
    // M3 の通し穴(中 3.4)、ザグリ 6.5 × 3.3、貫通
    let mut doc = plate_with_hole(
        &[[15.0, 10.0]],
        |t, s, points| Hole {
            points,
            kind: HoleKind::Counterbore,
            tap: Tap::Clearance,
            size: Some("M3".into()),
            ..Hole::new(t, s)
        },
        false,
    );
    let v = removed(&mut doc, &k);
    let expect = cyl(1.7, 10.0) + cyl(3.25, 3.3) - cyl(1.7, 3.3);
    assert!(close(v, expect, 1e-2), "{v} vs {expect}");
    // ザグリの壁の面に名前がある(後の機能が参照できる)
    let reg = registry();
    let report = doc.recompute(&reg, &*k);
    let bodies = visible_bodies(doc.data(), &reg, &report);
    let names: Vec<String> = k
        .faces(bodies[0].1)
        .unwrap()
        .into_iter()
        .map(|f| f.name)
        .collect();
    assert!(
        names
            .iter()
            .any(|n| n.contains(":side:p") && n.contains("_counterbore")),
        "{names:?}"
    );
}

#[test]
fn countersink_and_drill_point_follow_their_angles() {
    let k = TestKernel::new();
    // 径 5、深さ 8 の止まり穴、皿 10 × 90°(深さ 2.5)、平らな底
    let mut doc = plate_with_hole(
        &[[15.0, 10.0]],
        |t, s, points| Hole {
            points,
            diameter: "5 mm".into(),
            depth: Some("8 mm".into()),
            kind: HoleKind::Countersink,
            countersink_diameter: Some("10 mm".into()),
            ..Hole::new(t, s)
        },
        false,
    );
    // truck は円すいの回転面とのブーリアンが安定しない(同じ形でも実行ごとに通ったり
    // 失敗したりする。表の並びが実行ごとに変わるため。実測)。truck では、失敗しても
    // パニックせず誤りとして知らせることだけを確かめる
    if let Some(v) = removed_or_reported(&mut doc, &k) {
        let expect = cone(5.0, 2.5, 2.5) + cyl(2.5, 8.0 - 2.5);
        assert!(close(v, expect, 1e-2), "{v} vs {expect}");
    }
    // M4 のネジ穴(下穴 3.3)、深さ 6、ドリルの先 118°
    let mut doc = plate_with_hole(
        &[[15.0, 10.0]],
        |t, s, points| Hole {
            points,
            depth: Some("6 mm".into()),
            tip: Tip::Angled,
            tap: Tap::Tapped,
            size: Some("M4".into()),
            ..Hole::new(t, s)
        },
        false,
    );
    if let Some(v) = removed_or_reported(&mut doc, &k) {
        let r: f64 = 1.65;
        let expect = cyl(r, 6.0) + cone(r, 0.0, r / 59f64.to_radians().tan());
        assert!(close(v, expect, 1e-2), "{v} vs {expect}");
    }
}

#[test]
fn a_tapped_hole_takes_a_typed_size_outside_the_table() {
    // 表(M2〜M12 並目)に無い呼びも打てばネジ穴になる: 細目 M6×0.75 は下穴 5.25、M16 は 14
    let k = TestKernel::new();
    let tapped = |size: &str, tap: Tap| {
        plate_with_hole(
            &[[15.0, 10.0]],
            |t, s, points| Hole {
                points,
                tap,
                size: Some(size.into()),
                ..Hole::new(t, s)
            },
            false,
        )
    };
    for (size, d) in [("M6x0.75", 5.25), ("m16", 14.0), ("M6", 5.0)] {
        let v = removed(&mut tapped(size, Tap::Tapped), &k);
        let expect = cyl(d / 2.0, 10.0);
        assert!(close(v, expect, 1e-2), "{size}: {v} vs {expect}");
    }
    // 通し穴は表の呼びだけ。読めない呼びと一緒に、理由を出して止まる
    for (size, tap, want) in [
        ("M18", Tap::Clearance, "通し穴の径は表の"),
        ("M7", Tap::Tapped, "読めません"),
    ] {
        let mut doc = tapped(size, tap);
        let report = doc.recompute(&registry(), &*k);
        let text = format!("{:?}", report.features.last().unwrap().status);
        assert!(text.contains(want), "{size}: {text}");
    }
}

#[test]
fn a_tapped_hole_marks_its_wall_for_the_thread_shading() {
    // ネジ穴は、下穴の壁にねじ山の陰影を付ける印を出す(穴の口から深さまで、ピッチは呼びの物)
    let k = TestKernel::new();
    let mut doc = plate_with_hole(
        &[[15.0, 10.0]],
        |t, s, points| Hole {
            points,
            depth: Some("6 mm".into()),
            tap: Tap::Tapped,
            size: Some("M6x0.75".into()),
            ..Hole::new(t, s)
        },
        false,
    );
    let report = doc.recompute(&registry(), &*k);
    let last = report.features.last().unwrap();
    let kmb_core::FeatureStatus::Ok { outputs, .. } = &last.status else {
        panic!("{:?}", last.status);
    };
    let marks = <kmb_interchange::ThreadMarks as kmb_core::Payload>::from_value(
        &outputs[kmb_interchange::THREAD_MARKS_OUTPUT],
    )
    .expect("ねじの印");
    let [m] = marks.marks[..] else {
        panic!("{marks:?}");
    };
    assert!(m.internal);
    assert!((m.radius - 5.25 / 2.0).abs() < 1e-9, "{m:?}");
    assert!((m.pitch - 0.75).abs() < 1e-9 && (m.length - 6.0).abs() < 1e-9);
    // 板の上の面(z = 10)の (15, 10) から下へ
    assert!(
        m.start
            .iter()
            .zip([15.0, 10.0, 10.0])
            .all(|(a, b)| (a - b).abs() < 1e-6)
    );
    assert!((m.axis[2] + 1.0).abs() < 1e-9, "{m:?}");
    // ねじでない穴は印を出さない
    let mut plain = plate_with_hole(
        &[[15.0, 10.0]],
        |t, s, points| Hole {
            points,
            diameter: "5 mm".into(),
            ..Hole::new(t, s)
        },
        false,
    );
    let report = plain.recompute(&registry(), &*k);
    let kmb_core::FeatureStatus::Ok { outputs, .. } = &report.features.last().unwrap().status
    else {
        panic!("穴が開かない");
    };
    assert!(!outputs.contains_key(kmb_interchange::THREAD_MARKS_OUTPUT));
}

#[test]
fn counterbored_holes_on_several_points_can_be_patterned() {
    let k = TestKernel::new();
    let hole = |t, s, points| Hole {
        points,
        diameter: "4 mm".into(),
        kind: HoleKind::Counterbore,
        counterbore_diameter: Some("7 mm".into()),
        counterbore_depth: Some("3 mm".into()),
        ..Hole::new(t, s)
    };
    let one = cyl(2.0, 10.0) + cyl(3.5, 3.0) - cyl(2.0, 3.0);
    let mut doc = plate_with_hole(&[[5.0, 5.0], [5.0, 15.0]], hole, false);
    let v = removed(&mut doc, &k);
    assert!(close(v, 2.0 * one, 1e-2), "{v} vs {}", 2.0 * one);
    // パターンで右へ 10 mm 並べると 4 つ
    let mut doc = plate_with_hole(&[[5.0, 5.0], [5.0, 15.0]], hole, true);
    let v = removed(&mut doc, &k);
    assert!(close(v, 4.0 * one, 1e-2), "{v} vs {}", 4.0 * one);
}

#[test]
fn hole_sizes_that_do_not_fit_are_reported() {
    let k = TestKernel::new();
    let reg = registry();
    let mut doc = plate_with_hole(
        &[[15.0, 10.0]],
        |t, s, points| Hole {
            points,
            diameter: "5 mm".into(),
            kind: HoleKind::Counterbore,
            counterbore_diameter: Some("4 mm".into()),
            ..Hole::new(t, s)
        },
        false,
    );
    let report = doc.recompute(&reg, &*k);
    let st = &report
        .features
        .iter()
        .find(|f| f.name == "穴")
        .unwrap()
        .status;
    assert!(format!("{st:?}").contains("ザグリの径"), "{st:?}");
    let mut doc = plate_with_hole(
        &[[15.0, 10.0]],
        |t, s, points| Hole {
            points,
            tap: Tap::Tapped,
            size: Some("M7".into()),
            ..Hole::new(t, s)
        },
        false,
    );
    let report = doc.recompute(&reg, &*k);
    let st = &report
        .features
        .iter()
        .find(|f| f.name == "穴")
        .unwrap()
        .status;
    assert!(format!("{st:?}").contains("M7"), "{st:?}");
}

#[test]
fn expanded_thread_tapped_holes_use_all_families_and_survive_reopening() {
    use kmb_samples::sample::{DocBuilder, SketchBuilder};
    use kmb_sketch::SketchPlane;
    let k = TestKernel::new();
    if k.is_truck() {
        kmb_contract::skip!("truck は曲面を網で近似するため、M14 ねじ穴の体積が 0.183 mm³ ずれる");
    }
    for (name, diameter) in [
        ("M14", 12.0),
        ("M16", 14.0),
        ("M20", 17.5),
        ("M24", 21.0),
        ("M30", 26.5),
        ("1/4-20 UNC", 0.201 * 25.4),
        ("#10-32 UNF", 0.159 * 25.4),
        ("G1/8", 8.8),
        ("Rp1/4", 11.8),
        ("M6x1", 5.0),
    ] {
        let mut b = DocBuilder::new();
        let sketch = rect_sketch(&mut b, SketchPlane::Xy, 0.0, 0.0, 100.0, 80.0);
        let plate = b.feature(
            EXTRUDE,
            "板",
            Extrude {
                profile: Some(sketch),
                distance: "12 mm".into(),
                ..Default::default()
            },
        );
        let mut points = SketchBuilder::new(SketchPlane::face(plate, format!("f{}:end", plate.0)));
        let point = points.fixed_point(50.0, 40.0);
        let sketch = b.feature(kmb_sketch::SKETCH, "穴の位置", points.build());
        b.feature(
            HOLE,
            "ねじ穴",
            Hole {
                points: vec![kmb_core::PointId(point.0)],
                tap: Tap::Tapped,
                size: Some(name.into()),
                depth: Some("8 mm".into()),
                ..Hole::new(plate, sketch)
            },
        );
        let mut doc = b.finish("ねじの族");
        let expected = 100.0 * 80.0 * 12.0 - cyl(diameter / 2.0, 8.0);
        for reopen in [false, true] {
            if reopen {
                doc = kmb_core::Document::from_json(&doc.to_json()).unwrap();
            }
            let report = doc.recompute(&registry(), &*k);
            assert_ok(&report);
            let bodies = visible_bodies(doc.data(), &registry(), &report);
            assert_eq!(bodies.len(), 1);
            let volume = k.mass(bodies[0].1).unwrap().volume;
            assert!(
                (volume - expected).abs() < 0.01,
                "{name}: {volume} vs {expected}"
            );
        }
    }
}

#[test]
fn expanded_thread_attributes_use_the_shared_action_without_changing_the_shape() {
    use kmb_core::{Context, Extensions, Kernel, Outcome, ProblemKind};
    use kmb_samples::sample::{DocBuilder, SketchBuilder};
    use kmb_sketch::SketchPlane;
    use std::sync::Arc;
    let kernel = TestKernel::new();
    if kernel.is_truck() {
        kmb_contract::skip!("truck は円筒の面を選ぶための円の辺を列挙しない");
    }
    let k: Arc<dyn Kernel> = match kernel {
        TestKernel::Smlib(k) => Arc::new(k),
        TestKernel::Truck(k) => Arc::new(k),
    };
    let reg = registry();
    let actions = Extensions(vec![Arc::new(kmb_cad_hole::thread::ThreadCommands)]);
    for (name, diameter) in [
        ("M24", None),
        ("M30", None),
        ("1/4-20 UNC", None),
        ("#10-32 UNF", None),
        ("G1/4", None),
        ("Rp1/4", None),
        ("M10", Some(9.7)),
        ("M8", Some(7.9)),
    ] {
        let size = kmb_cad_hole::thread::parse_size(name).unwrap();
        let diameter = diameter.unwrap_or(size.major);
        let mut b = DocBuilder::new();
        let mut circle = SketchBuilder::new(SketchPlane::Xy);
        let center = circle.fixed_point(0.0, 0.0);
        if name.starts_with("Rp") {
            circle.circle(
                center,
                &format!("{} mm", size.major + 8.0),
                size.major / 2.0 + 4.0,
            );
        }
        circle.circle(center, &format!("{diameter} mm"), diameter / 2.0);
        let sketch = b.feature(kmb_sketch::SKETCH, "円", circle.build());
        let cylinder = b.feature(
            EXTRUDE,
            "円筒",
            Extrude {
                profile: Some(sketch),
                distance: "10 mm".into(),
                ..Default::default()
            },
        );
        let mut doc = b.finish("ねじ属性");
        let report = doc.recompute(&reg, k.as_ref());
        assert_ok(&report);
        let body = visible_bodies(doc.data(), &reg, &report)[0].1;
        let volume = k.mass(body).unwrap().volume;
        let face = k
            .arc_edges(body)
            .unwrap()
            .into_iter()
            .find(|edge| {
                kmb_cad_hole::thread::cylinder_of(k.as_ref(), body, &edge.faces[1]).is_ok_and(|c| {
                    c.internal == name.starts_with("Rp") && (c.diameter - diameter).abs() < 1e-4
                })
            })
            .unwrap()
            .faces[1]
            .clone();
        let mut args = serde_json::json!({"feature":cylinder.0,"face":face});
        if diameter == size.major {
            args["size"] = serde_json::json!(name);
        }
        let cx = Context {
            args: &args,
            ..Context::new(doc.data(), &report, &reg, &k)
        };
        let Outcome::Change { label, edits } = actions.run(THREAD_ON_FACE, &cx).unwrap() else {
            panic!()
        };
        let bad = serde_json::json!({"feature":cylinder.0,"face":face,"size":name,"unknown":true});
        let invalid = Context { args: &bad, ..cx };
        assert_eq!(
            actions.run(THREAD_ON_FACE, &invalid).err().unwrap().kind,
            ProblemKind::Input
        );
        doc.apply_edits(&label, edits).unwrap();
        let saved = doc.to_json();
        let mut doc = kmb_core::Document::from_json(&saved).unwrap();
        assert_eq!(doc.to_json(), saved);
        let report = doc.recompute(&reg, k.as_ref());
        assert_ok(&report);
        let bodies = visible_bodies(doc.data(), &reg, &report);
        assert_eq!(bodies.len(), 1);
        assert!((k.mass(bodies[0].1).unwrap().volume - volume).abs() < 1e-5);
        assert_eq!(
            doc.data().features.last().unwrap().data["size"],
            size.name()
        );
    }
}

fn legacy_thread_recess_volume(
    kind: &str,
    name: &str,
    countersink_diameter: Option<f64>,
    k: &TestKernel,
) -> (f64, kmb_interchange::HoleInfo) {
    use kmb_samples::sample::{DocBuilder, SketchBuilder};
    use kmb_sketch::SketchPlane;
    let mut b = DocBuilder::new();
    let sketch = rect_sketch(&mut b, SketchPlane::Xy, 0.0, 0.0, 100.0, 80.0);
    let plate = b.feature(
        EXTRUDE,
        "板",
        Extrude {
            profile: Some(sketch),
            distance: "60 mm".into(),
            ..Default::default()
        },
    );
    let mut points = SketchBuilder::new(SketchPlane::face(plate, format!("f{}:end", plate.0)));
    let point = points.fixed_point(50.0, 40.0);
    let sketch = b.feature(kmb_sketch::SKETCH, "穴の位置", points.build());
    let mut hole = serde_json::json!({
        "target":plate.0,"sketch":sketch.0,"points":[point.0],
        "kind":kind,"tap":"tapped","size":name,"depth":"40 mm"
    });
    if let Some(diameter) = countersink_diameter {
        hole["countersink_diameter"] = serde_json::json!(format!("{diameter} mm"));
        hole["countersink_angle"] = serde_json::json!("90 deg");
    }
    b.feature("solid.hole@1", "既定の径の穴", hole);
    let mut doc = kmb_core::Document::from_json(&b.finish("保存した穴").to_json()).unwrap();
    let reg = registry();
    let report = doc.recompute(&reg, &**k);
    assert_ok(&report);
    let bodies = visible_bodies(doc.data(), &reg, &report);
    assert_eq!(bodies.len(), 1);
    let last = report.features.last().unwrap();
    let kmb_core::FeatureStatus::Ok { outputs, .. } = &last.status else {
        panic!("{:?}", last.status);
    };
    let info = <kmb_interchange::HoleInfo as kmb_core::Payload>::from_value(
        &outputs[kmb_interchange::HOLE_OUTPUT],
    )
    .unwrap();
    (k.mass(bodies[0].1).unwrap().volume, info)
}

#[test]
fn legacy_thread_counterbore_keeps_the_omitted_diameter_and_depth() {
    let k = TestKernel::new();
    if k.is_truck() {
        kmb_contract::skip!(
            "truck は曲面を網で近似するため、M14 ザグリの除去体積が 0.613 mm³ ずれる"
        );
    }
    for (name, diameter) in [
        ("M14", 12.0),
        ("M16", 14.0),
        ("M20", 17.5),
        ("M24", 21.0),
        ("M30", 26.5),
    ] {
        let r = diameter / 2.0;
        let removed = cyl(r, 40.0) + cyl(diameter * 0.9, diameter) - cyl(r, diameter);
        let actual =
            100.0 * 80.0 * 60.0 - legacy_thread_recess_volume("counterbore", name, None, &k).0;
        assert!(
            (actual - removed).abs() < 0.01,
            "{name}: {actual} vs {removed}"
        );
    }
}

#[test]
fn legacy_thread_countersink_keeps_the_omitted_diameter() {
    let k = TestKernel::new();
    if k.is_truck() {
        kmb_contract::skip!("truck は省略寸法の皿穴のブーリアンで閉じたシェルを作れない");
    }
    // 保存した省略値は、同じカーネルで固定寸法を明示した穴と同じ形になる。
    for (name, diameter, countersink) in [
        ("M14", 12.0, 24.0),
        ("M16", 14.0, 28.0),
        ("M20", 17.5, 35.0),
        ("M24", 21.0, 42.0),
        ("M30", 26.5, 53.0),
    ] {
        let (actual, info) = legacy_thread_recess_volume("countersink", name, None, &k);
        let (expected, _) = legacy_thread_recess_volume("countersink", name, Some(countersink), &k);
        assert!((info.diameter - diameter).abs() < 1e-9, "{name}: {info:?}");
        let (actual_diameter, angle) = info.countersink.unwrap();
        assert!((actual_diameter - countersink).abs() < 1e-9, "{name}");
        assert!((angle - 90f64.to_radians()).abs() < 1e-9, "{name}");
        assert!(
            (actual - expected).abs() < 0.01,
            "{name}: {actual} vs {expected}"
        );
    }
}

#[test]
fn expanded_thread_hole_actions_create_the_new_recess_defaults() {
    use kmb_core::{Context, Extensions, Kernel, Outcome};
    use kmb_samples::sample::DocBuilder;
    use kmb_sketch::SketchPlane;
    use std::sync::Arc;
    let k: Arc<dyn Kernel> = Arc::new(kmb_kernel_smlib::SmlibKernel::new());
    let mut b = DocBuilder::new();
    let sketch = rect_sketch(&mut b, SketchPlane::Xy, 0.0, 0.0, 100.0, 80.0);
    let body = b.feature(
        EXTRUDE,
        "板",
        Extrude {
            profile: Some(sketch),
            distance: "60 mm".into(),
            ..Default::default()
        },
    );
    let mut doc = b.finish("表のザグリ");
    let reg = registry();
    let report = doc.recompute(&reg, k.as_ref());
    assert_ok(&report);
    let actions = Extensions(vec![Arc::new(kmb_cad_hole::HoleCommands)]);
    let args = serde_json::json!({"feature":body.0,"face":format!("f{}:end",body.0),"point":[50.0,40.0,60.0],"kind":"counterbore","tap":"tapped","size":"M16","depth":"40 mm"});
    let cx = Context {
        args: &args,
        ..Context::new(doc.data(), &report, &reg, &k)
    };
    let Outcome::Change { label, edits } = actions.run(kmb_cad_hole::HOLE_ON_FACE, &cx).unwrap()
    else {
        panic!()
    };
    doc.apply_edits(&label, edits).unwrap();
    assert_eq!(
        doc.data().features.last().unwrap().data["standard_recesses"],
        true
    );
    let report = doc.recompute(&reg, k.as_ref());
    assert_ok(&report);
    let bodies = visible_bodies(doc.data(), &reg, &report);
    assert_eq!(bodies.len(), 1);
    let removed = 100.0 * 80.0 * 60.0 - k.mass(bodies[0].1).unwrap().volume;
    let expected = cyl(7.0, 40.0) + cyl(13.0, 17.0) - cyl(7.0, 17.0);
    assert!((removed - expected).abs() < 0.01, "{removed} vs {expected}");
}
