//! 歯車の輪郭とボディ(kmb-cad-gear)の試験。確かめること: ボディの体積が輪郭の面積 × 歯の幅、面の
//! 名前が歯の番号と歯面から決まること、諸元に歯の幅が入ること、パラメータの歯数を変えると作り直すこと。

use super::tests::{TestKernel, assert_ok, close, final_mass, registry};
use kmb_core::{FeatureStatus, Payload, Profile, visible_bodies};
use kmb_samples::sample::DocBuilder;

fn area(p: &Profile) -> f64 {
    p.regions
        .iter()
        .map(|r| r.outer.signed_area() + r.holes.iter().map(|h| h.signed_area()).sum::<f64>())
        .sum()
}

#[test]
fn an_extruded_gear_has_the_outline_area_times_its_width() {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    b.param("z", "14");
    let g = b.feature(
        kmb_cad_gear::GEAR,
        "歯車",
        kmb_cad_gear::Gear {
            module: "1.5 mm".into(),
            teeth: "z".into(),
            bore: Some("6 mm".into()),
            ..Default::default()
        },
    );
    let e = b.feature(
        kmb_cad_gear::GEAR_BODY,
        "歯車のボディ",
        kmb_cad_gear::GearBody {
            width: "5 mm".into(),
            ..kmb_cad_gear::GearBody::new(g)
        },
    );
    let mut doc = b.finish("歯車");
    let reg = registry();
    let profile_area = |doc: &mut kmb_core::Document| {
        let report = doc.recompute(&reg, &*k);
        assert_ok(&report);
        let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(g) else {
            unreachable!()
        };
        area(&Profile::from_value(&outputs["profile"]).unwrap())
    };
    let a = profile_area(&mut doc);
    // ピッチ円 21 mm の円板から軸の穴を除いた面積の近く
    let disc = std::f64::consts::PI * (10.5f64.powi(2) - 3.0f64.powi(2));
    assert!((a - disc).abs() < 0.1 * disc, "面積 {a} 円板 {disc}");
    let m = final_mass(&mut doc, &k);
    assert!(
        close(m.volume, a * 5.0, 2e-3),
        "{} vs {}",
        m.volume,
        a * 5.0
    );
    let report = doc.recompute(&reg, &*k);
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(e) else {
        unreachable!()
    };
    let spec = kmb_interchange::GearSpec::from_value(&outputs["gear"]).unwrap();
    assert_eq!((spec.teeth, spec.face_width), (14, Some(5.0)));
    assert!((spec.pitch_radius - 10.5).abs() < 1e-12);
    let bodies = visible_bodies(doc.data(), &reg, &report);
    let names = k.face_names(bodies[0].1).unwrap();
    for part in ["t0:tip", "t13:root", "t5:r0", "t5:l0", "bore"] {
        let want = format!("f{}:side:{part}", e.0);
        assert!(
            names
                .iter()
                .any(|n| n == &want || n.starts_with(&format!("{want}."))),
            "{want} が無い: {names:?}"
        );
    }
    // 歯数を変えると作り直す(14 番目の歯は消える)
    let z = doc.data().param_by_name("z").unwrap().id;
    doc.apply(kmb_core::Transaction::new(
        "歯数",
        vec![kmb_core::Op::SetParamExpr {
            id: z,
            expr: "12".into(),
        }],
    ))
    .unwrap();
    let report = doc.recompute(&reg, &*k);
    assert_ok(&report);
    let bodies = visible_bodies(doc.data(), &reg, &report);
    let names = k.face_names(bodies[0].1).unwrap();
    assert!(names.iter().any(|n| n.contains(":side:t11:tip")));
    assert!(!names.iter().any(|n| n.contains(":side:t13:")), "{names:?}");
}

#[test]
fn an_extruded_rack_has_the_profile_area_times_its_width() {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    let g = b.feature(
        kmb_cad_gear::GEAR,
        "ラック",
        kmb_cad_gear::Gear {
            kind: kmb_interchange::GearKind::Rack,
            module: "2 mm".into(),
            teeth: "12".into(),
            rack_back: Some("5 mm".into()),
            tip_round: Some("0.15".into()),
            phase: Some("30 deg".into()),
            x: "7 mm".into(),
            y: "11 mm".into(),
            ..Default::default()
        },
    );
    let e = b.feature(
        kmb_cad_gear::GEAR_BODY,
        "ラックのボディ",
        kmb_cad_gear::GearBody {
            width: "4 mm".into(),
            ..kmb_cad_gear::GearBody::new(g)
        },
    );
    let mut doc = b.finish("ラック");
    let reg = registry();
    let report = doc.recompute(&reg, &*k);
    assert_ok(&report);
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(g) else {
        panic!("輪郭")
    };
    let a = area(&Profile::from_value(&outputs["profile"]).unwrap());
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(e) else {
        panic!("ボディ")
    };
    let spec = kmb_interchange::GearSpec::from_value(&outputs["gear"]).unwrap();
    assert_eq!(spec.kind, kmb_interchange::GearKind::Rack);
    assert_eq!(
        (spec.teeth, spec.face_width, spec.pitch_radius),
        (12, Some(4.0), 0.0)
    );
    let mass = final_mass(&mut doc, &k);
    assert!(
        close(mass.volume, a * 4.0, 2e-3),
        "{} vs {}",
        mass.volume,
        a * 4.0
    );
}

#[test]
fn a_48_tooth_internal_gear_body_has_the_ring_area_times_the_width() {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    let g = b.feature(
        kmb_cad_gear::GEAR,
        "内歯車",
        kmb_cad_gear::Gear {
            kind: kmb_interchange::GearKind::Internal,
            module: "1 mm".into(),
            teeth: "48".into(),
            outer_diameter: Some("60 mm".into()),
            ..Default::default()
        },
    );
    let body = b.feature(
        kmb_cad_gear::GEAR_BODY,
        "内歯車のボディ",
        kmb_cad_gear::GearBody {
            width: "5 mm".into(),
            ..kmb_cad_gear::GearBody::new(g)
        },
    );
    let mut doc = b.finish("内歯車");
    let reg = registry();
    let report = doc.recompute(&reg, &*k);
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(g) else {
        panic!("輪郭を計算できる: {report:?}")
    };
    let profile = Profile::from_value(&outputs["profile"]).unwrap();
    // 大きな外径の円を 64 分割する近似は、薄い輪の面積では誤差が拡大する。
    // 体積との比較では外径の円を細かく刻み、短い歯面の区間の点は増やし過ぎない。
    let loop_area = |loop_: &kmb_core::ProfileLoop| {
        let points: Vec<_> = loop_
            .edges
            .iter()
            .flat_map(|edge| {
                edge.polyline(
                    if matches!(edge.segment, kmb_core::Segment::Circle { .. }) {
                        4096
                    } else {
                        128
                    },
                )
            })
            .collect();
        kmb_core::kernel::polygon_area(&points)
    };
    let area: f64 = profile
        .regions
        .iter()
        .map(|region| loop_area(&region.outer) + region.holes.iter().map(loop_area).sum::<f64>())
        .sum();
    let theory = std::f64::consts::PI / 4.0 * (60.0f64.powi(2) - 48.0f64.powi(2));
    assert!((area - theory).abs() < 0.05 * theory, "{area} vs {theory}");
    if k.is_truck() && matches!(report.feature(body), Some(FeatureStatus::Error { .. })) {
        let Some(FeatureStatus::Error { message }) = report.feature(body) else {
            unreachable!()
        };
        assert!(
            message.contains("このカーネルは") && message.contains("対応していません"),
            "{message}"
        );
        assert!(
            matches!(
                k.extrude(&profile, 0.0, 5.0, "internal-unsupported"),
                Err(kmb_core::KernelError::Unsupported(_))
            ),
            "未対応の型を確かめる"
        );
    } else {
        assert_ok(&report);
        let mass = final_mass(&mut doc, &k);
        assert!(
            close(mass.volume, area * 5.0, 2e-3),
            "{} vs {}",
            mass.volume,
            area * 5.0
        );
        let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(body) else {
            unreachable!()
        };
        let spec = kmb_interchange::GearSpec::from_value(&outputs["gear"]).unwrap();
        assert_eq!(spec.kind, kmb_interchange::GearKind::Internal);
        assert_eq!(spec.face_width, Some(5.0));
        let bodies = visible_bodies(doc.data(), &reg, &report);
        let names = k.face_names(bodies[0].1).unwrap();
        for part in ["rim", "t0:tip", "t47:root", "t4:r0", "t4:l0"] {
            let want = format!("f{}:side:{part}", body.0);
            assert!(
                names
                    .iter()
                    .any(|name| name == &want || name.starts_with(&format!("{want}."))),
                "{want}"
            );
        }
    }
}

/// 確かめること: 正面の断面積を保つねじり、右左の端面の位置、歯数30・幅20の生成時間。
#[test]
fn helical_gears_keep_volume_and_turn_the_end_teeth_in_opposite_directions() {
    use kmb_interchange::gear::Hand;
    for hand in [Hand::Right, Hand::Left] {
        let k = TestKernel::new();
        let mut b = DocBuilder::new();
        let gear = b.feature(
            kmb_cad_gear::GEAR,
            "はすば",
            kmb_cad_gear::Gear {
                module: "2 mm".into(),
                teeth: "30".into(),
                pressure_angle: "20 deg".into(),
                helix_angle: Some("20 deg".into()),
                hand,
                x: "11 mm".into(),
                y: "-7 mm".into(),
                ..Default::default()
            },
        );
        let body = b.feature(
            kmb_cad_gear::GEAR_BODY,
            "はすばのボディ",
            kmb_cad_gear::GearBody {
                width: "20 mm".into(),
                ..kmb_cad_gear::GearBody::new(gear)
            },
        );
        let mut doc = b.finish("はすば");
        let reg = registry();
        let start = std::time::Instant::now();
        let report = doc.recompute(&reg, &*k);
        let elapsed = start.elapsed();
        if k.is_truck() {
            let Some(FeatureStatus::Error { message }) = report.feature(body) else {
                panic!("truck のねじりは対応していない誤り: {report:?}")
            };
            assert!(
                message.contains("スイープ") && message.contains("対応していません"),
                "{message}"
            );
            continue;
        }
        assert_ok(&report);
        eprintln!(
            "はすば 歯数30 幅20 hand={hand:?}: {:.6} s",
            elapsed.as_secs_f64()
        );
        let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(gear) else {
            panic!("輪郭")
        };
        let profile = Profile::from_value(&outputs["profile"]).unwrap();
        let spec = kmb_interchange::GearSpec::from_value(&outputs["gear"]).unwrap();
        let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(body) else {
            panic!("ボディ")
        };
        let kmb_core::Value::Shape(shape) = outputs["body"] else {
            panic!("立体")
        };
        let mass = k.mass(shape).unwrap();
        assert!(
            close(mass.volume, area(&profile) * 20.0, 0.01),
            "{} vs {}",
            mass.volume,
            area(&profile) * 20.0
        );
        let turn = kmb_cad_gear::helical::twist(20.0, &spec).unwrap();
        let mesh = k.mesh(shape, 0.04).unwrap();
        let mut count = 0;
        let mut opposite_error = 0.0f64;
        for q in mesh
            .edges
            .iter()
            .flatten()
            .filter(|q| (f64::from(q[2]) - 20.0).abs() < 1e-3)
        {
            let (x, y) = (f64::from(q[0]) - 11.0, f64::from(q[1]) + 7.0);
            let check = |a: f64| {
                let (sin, cos) = a.sin_cos();
                let local = [11.0 + cos * x - sin * y, -7.0 + sin * x + cos * y];
                profile.regions[0]
                    .outer
                    .edges
                    .iter()
                    .map(|e| e.segment.distance(local))
                    .fold(f64::INFINITY, f64::min)
            };
            assert!(check(-turn) < 0.08, "{hand:?} 端面の位置 {q:?}");
            opposite_error = opposite_error.max(check(turn));
            count += 1;
        }
        assert!(count > 20, "端面の辺の点が無い");
        assert!(opposite_error > 0.1, "左右のねじりが同じになっている");
    }
}

/// 確かめること: やまばの中央の溝が歯底円柱でつながり、両端の歯の位相が一致する。
#[test]
fn herringbone_gears_join_the_two_halves_and_fill_only_the_root_in_the_groove() {
    for gap in [0.0, 1.0] {
        let k = TestKernel::new();
        let mut b = DocBuilder::new();
        let gear = b.feature(
            kmb_cad_gear::GEAR,
            "やまば",
            kmb_cad_gear::Gear {
                module: "2 mm".into(),
                teeth: "12".into(),
                helix_angle: Some("20 deg".into()),
                ..Default::default()
            },
        );
        let body = b.feature(
            kmb_cad_gear::GEAR_BODY,
            "やまばのボディ",
            kmb_cad_gear::GearBody {
                width: "8 mm".into(),
                herringbone: true,
                groove: Some(format!("{gap} mm")),
                ..kmb_cad_gear::GearBody::new(gear)
            },
        );
        let mut doc = b.finish("やまば");
        let reg = registry();
        let report = doc.recompute(&reg, &*k);
        if k.is_truck() {
            let Some(FeatureStatus::Error { message }) = report.feature(body) else {
                panic!("対応していない誤り")
            };
            assert!(
                message.contains("スイープ") && message.contains("対応していません"),
                "{message}"
            );
            continue;
        }
        assert_ok(&report);
        let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(gear) else {
            panic!("輪郭")
        };
        let profile = Profile::from_value(&outputs["profile"]).unwrap();
        let spec = kmb_interchange::GearSpec::from_value(&outputs["gear"]).unwrap();
        let root = spec.pitch_radius - spec.transverse_module() * 1.25;
        let expected = area(&profile) * (8.0 - gap) + std::f64::consts::PI * root * root * gap;
        let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(body) else {
            panic!("ボディ")
        };
        let spec = kmb_interchange::GearSpec::from_value(&outputs["gear"]).unwrap();
        assert_eq!(spec.hand, kmb_interchange::gear::Hand::Both);
        let kmb_core::Value::Shape(shape) = outputs["body"] else {
            panic!("立体")
        };
        assert!(close(k.mass(shape).unwrap().volume, expected, 0.01));
        let mesh = k.mesh(shape, 0.04).unwrap();
        // 断面の歯と歯溝を実際の閉じた網で調べる。位相は半幅とねじれ角から独立に求める。
        let half_width = (8.0 - gap) / 2.0;
        let central_phase = half_width * spec.helix_angle.tan() / spec.pitch_radius;
        let flank_offset = 0.3 * std::f64::consts::PI / f64::from(spec.teeth);
        for z in [0.05, 4.0 - gap / 2.0 - 0.05, 4.0 + gap / 2.0 + 0.05, 7.95] {
            let phase = if z < 4.0 {
                z * spec.helix_angle.tan() / spec.pitch_radius
            } else {
                (8.0 - z) * spec.helix_angle.tan() / spec.pitch_radius
            };
            if (z - 4.0).abs() < 1.0 {
                assert!((phase - central_phase).abs() < 0.01);
            }
            for offset in [-flank_offset, flank_offset] {
                let angle = phase + offset;
                let p = [
                    spec.pitch_radius * angle.cos(),
                    spec.pitch_radius * angle.sin(),
                    z,
                ];
                assert!(mesh.contains(p), "歯の中 {gap} {z} {angle}");
            }
            let gap_angle = phase + std::f64::consts::PI / f64::from(spec.teeth);
            let p = [
                spec.pitch_radius * gap_angle.cos(),
                spec.pitch_radius * gap_angle.sin(),
                z,
            ];
            assert!(!mesh.contains(p), "歯溝 {gap} {z} {gap_angle}");
        }
        if gap == 0.0 {
            let p = [
                spec.pitch_radius * central_phase.cos(),
                spec.pitch_radius * central_phase.sin(),
                4.0,
            ];
            assert!(mesh.contains(p), "中央の歯は両半分でつながる");
        }

        for z in [0.0, 8.0] {
            let mut count = 0;
            for q in mesh
                .edges
                .iter()
                .flatten()
                .filter(|q| (f64::from(q[2]) - z).abs() < 1e-3)
            {
                let p = [f64::from(q[0]), f64::from(q[1])];
                let distance = profile.regions[0]
                    .outer
                    .edges
                    .iter()
                    .map(|e| e.segment.distance(p))
                    .fold(f64::INFINITY, f64::min);
                assert!(distance < 0.08, "端面の位相 {z} {p:?}");
                count += 1;
            }
            assert!(count > 10);
        }
    }
}

/// 確かめること: 内歯のねじりとやまばの中央の溝は、外径の輪を残して内側の歯だけを逃がす。
#[test]
fn internal_helical_and_herringbone_bodies_keep_the_ring_volume_and_axis() {
    for (herringbone, gap) in [(false, 0.0), (true, 0.0), (true, 1.0)] {
        let k = TestKernel::new();
        let mut b = DocBuilder::new();
        let gear = b.feature(
            kmb_cad_gear::GEAR,
            "内歯のはすば",
            kmb_cad_gear::Gear {
                kind: kmb_interchange::GearKind::Internal,
                module: "1 mm".into(),
                teeth: "24".into(),
                outer_diameter: Some("34 mm".into()),
                helix_angle: Some("20 deg".into()),
                x: "7 mm".into(),
                y: "-3 mm".into(),
                ..Default::default()
            },
        );
        let body = b.feature(
            kmb_cad_gear::GEAR_BODY,
            "内歯のボディ",
            kmb_cad_gear::GearBody {
                width: "6 mm".into(),
                direction: kmb_cad_gear::body::Side::Symmetric,
                herringbone,
                groove: Some(format!("{gap} mm")),
                ..kmb_cad_gear::GearBody::new(gear)
            },
        );
        let mut doc = b.finish("内歯の歯すじ");
        let report = doc.recompute(&registry(), &*k);
        if k.is_truck() {
            let Some(FeatureStatus::Error { message }) = report.feature(body) else {
                panic!("対応していない誤り: {report:?}")
            };
            assert!(
                message.contains("スイープ") && message.contains("対応していません"),
                "{message}"
            );
            continue;
        }
        assert_ok(&report);
        let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(gear) else {
            panic!("輪郭")
        };
        let profile = Profile::from_value(&outputs["profile"]).unwrap();
        let spec = kmb_interchange::GearSpec::from_value(&outputs["gear"]).unwrap();
        let loop_area = |loop_: &kmb_core::ProfileLoop| {
            let points: Vec<_> = loop_
                .edges
                .iter()
                .flat_map(|e| {
                    e.polyline(if matches!(e.segment, kmb_core::Segment::Circle { .. }) {
                        4096
                    } else {
                        128
                    })
                })
                .collect();
            kmb_core::kernel::polygon_area(&points)
        };
        let profile_area: f64 = profile
            .regions
            .iter()
            .map(|r| loop_area(&r.outer) + r.holes.iter().map(loop_area).sum::<f64>())
            .sum();
        let root = spec.pitch_radius + 1.25 * spec.transverse_module();
        let root_ring_area = std::f64::consts::PI * (17.0f64.powi(2) - root.powi(2));
        let expected = profile_area * (6.0 - gap) + root_ring_area * gap;
        let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(body) else {
            panic!("ボディ")
        };
        let kmb_core::Value::Shape(shape) = outputs["body"] else {
            panic!("立体")
        };
        let got = k.mass(shape).unwrap().volume;
        assert!(
            close(got, expected, 0.01),
            "herringbone={herringbone} gap={gap}: {got} vs {expected}"
        );
        let spec = kmb_interchange::GearSpec::from_value(&outputs["gear"]).unwrap();
        assert_eq!(spec.axis.origin, [7.0, -3.0, -3.0]);
        assert_eq!(spec.face_width, Some(6.0));
    }
}
