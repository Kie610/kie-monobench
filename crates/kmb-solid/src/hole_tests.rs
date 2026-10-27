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
        ("M16", Tap::Clearance, "通し穴の径は表の"),
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
