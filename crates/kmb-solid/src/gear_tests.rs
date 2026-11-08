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
fn gear_hub_web_and_keyway_have_the_calculated_volume_and_alignment() {
    use kmb_cad_gear::body::Alignment;
    use std::f64::consts::PI;
    for alignment in [Alignment::Centered, Alignment::Start] {
        let k = TestKernel::new();
        if k.is_truck() {
            kmb_contract::skip!(
                "truck はリム・腕・ボスの同じ円筒で接する領域のブーリアンに失敗する"
            );
        }
        let mut b = DocBuilder::new();
        let g = b.feature(
            kmb_cad_gear::GEAR,
            "穴とボスの歯車",
            kmb_cad_gear::Gear {
                module: "2 mm".into(),
                teeth: "24".into(),
                bore: Some("8 mm".into()),
                lightening_count: Some("4".into()),
                lightening_diameter: Some("3 mm".into()),
                lightening_circle: Some("26 mm".into()),
                boss_diameter: Some("16 mm".into()),
                keyway: true,
                ..Default::default()
            },
        );
        let e = b.feature(
            kmb_cad_gear::GEAR_BODY,
            "厚さの違う歯車",
            kmb_cad_gear::GearBody {
                width: "6 mm".into(),
                web: Some("3 mm".into()),
                boss: Some("12 mm".into()),
                alignment,
                ..kmb_cad_gear::GearBody::new(g)
            },
        );
        let mut doc = b.finish("歯車の穴とボス");
        let reg = registry();
        let report = doc.recompute(&reg, &*k);
        assert_ok(&report);
        let FeatureStatus::Ok { outputs, .. } = report.feature(g).unwrap() else {
            unreachable!()
        };
        let profile = Profile::from_value(&outputs["profile"]).unwrap();
        let tooth_area = profile.regions[0].outer.signed_area();
        // 軸の穴 8 mm のキーは幅 2 mm・深さ 1 mm。円の上の弓形を長方形から除く。
        let y = 15.0f64.sqrt();
        let key_extra = 2.0 * (5.0 - y) - (16.0 * (y / 4.0).acos() - y);
        let rim_area = tooth_area - PI * 19.5f64.powi(2);
        let web_area = PI * (19.5f64.powi(2) - 8.0f64.powi(2) - 4.0 * 1.5f64.powi(2));
        let boss_area = PI * (8.0f64.powi(2) - 4.0f64.powi(2)) - key_extra;
        let expected = rim_area * 6.0 + web_area * 3.0 + boss_area * 12.0;
        let mass = final_mass(&mut doc, &k);
        assert!(
            close(mass.volume, expected, 2e-3),
            "{} vs {expected}",
            mass.volume
        );
        let (z0, z1) = if alignment == Alignment::Centered {
            (-3.0, 9.0)
        } else {
            (0.0, 12.0)
        };
        assert!(
            close(mass.min[2], z0, 1e-6) && close(mass.max[2], z1, 1e-6),
            "{mass:?}"
        );
        let FeatureStatus::Ok { outputs, .. } = report.feature(e).unwrap() else {
            unreachable!()
        };
        let recipe = kmb_cad::Recipe::from_value(&outputs["recipe"]).unwrap();
        let kmb_cad::Recipe::Many { parts } = recipe else {
            panic!("厚さごとの作り方");
        };
        assert_eq!(parts.len(), 3);
        assert!(
            parts
                .iter()
                .all(|p| p.operation() == kmb_cad::Operation::New)
        );
    }
}

#[test]
fn gear_parts_keep_join_cut_and_same_thickness_intersection() {
    use super::tests::rect_sketch;
    use super::{Direction, EXTRUDE, Extrude, Operation};
    use kmb_sketch::SketchPlane;
    let stock_volume = 80.0 * 80.0 * 30.0;
    for operation in [Operation::Join, Operation::Cut, Operation::Intersect] {
        let k = TestKernel::new();
        if k.is_truck() {
            kmb_contract::skip!(
                "truck はリム・腕・ボスの同じ円筒で接する領域のブーリアンに失敗する"
            );
        }
        let mut b = DocBuilder::new();
        let sk = rect_sketch(&mut b, SketchPlane::Xy, -40.0, -40.0, 80.0, 80.0);
        let stock = b.feature(
            EXTRUDE,
            "材料",
            Extrude {
                profile: Some(sk),
                distance: "30 mm".into(),
                direction: Direction::Symmetric,
                ..Default::default()
            },
        );
        let g = b.feature(
            kmb_cad_gear::GEAR,
            "歯車",
            kmb_cad_gear::Gear {
                module: "2 mm".into(),
                teeth: "24".into(),
                bore: Some("8 mm".into()),
                lightening_count: Some("4".into()),
                lightening_diameter: Some("3 mm".into()),
                lightening_circle: Some("26 mm".into()),
                boss_diameter: Some("16 mm".into()),
                ..Default::default()
            },
        );
        let unequal = operation != Operation::Intersect;
        let e = b.feature(
            kmb_cad_gear::GEAR_BODY,
            "演算した歯車",
            kmb_cad_gear::GearBody {
                width: "6 mm".into(),
                web: unequal.then(|| "3 mm".into()),
                boss: unequal.then(|| "12 mm".into()),
                operation,
                target: Some(stock),
                ..kmb_cad_gear::GearBody::new(g)
            },
        );
        let mut doc = b.finish("歯車の演算");
        let reg = registry();
        let report = doc.recompute(&reg, &*k);
        assert_ok(&report);
        let FeatureStatus::Ok { outputs, .. } = report.feature(g).unwrap() else {
            unreachable!()
        };
        let profile = Profile::from_value(&outputs["profile"]).unwrap();
        let parts = kmb_cad_gear::hub::Parts::from_value(&outputs[kmb_cad_gear::hub::PARTS_OUTPUT])
            .unwrap();
        let gear_volume = if unequal {
            area(&parts.rim) * 6.0
                + area(parts.web.as_ref().unwrap()) * 3.0
                + area(parts.boss.as_ref().unwrap()) * 12.0
        } else {
            area(&profile) * 6.0
        };
        let expected = match operation {
            Operation::Join => stock_volume,
            Operation::Cut => stock_volume - gear_volume,
            Operation::Intersect => gear_volume,
            _ => unreachable!(),
        };
        let mass = final_mass(&mut doc, &k);
        assert!(
            close(mass.volume, expected, 2e-3),
            "{operation:?}: {} vs {expected}",
            mass.volume
        );
        let FeatureStatus::Ok { outputs, .. } = report.feature(e).unwrap() else {
            unreachable!()
        };
        let recipe = kmb_cad::Recipe::from_value(&outputs["recipe"]).unwrap();
        assert_eq!(recipe.operation(), operation);
        if operation == Operation::Intersect {
            let mut changed = doc.data().feature(e).unwrap().data.clone();
            changed["web"] = serde_json::json!("3 mm");
            doc.apply_edits(
                "腕の厚さ",
                vec![kmb_core::Edit::SetData {
                    id: e,
                    data: changed,
                }],
            )
            .unwrap();
            let report = doc.recompute(&reg, &*k);
            assert!(
                matches!(report.feature(e), Some(FeatureStatus::Error { message }) if message.contains("厚さが異なる歯車の交差には対応していません")),
                "{report:?}"
            );
        }
    }
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

#[test]
fn sprocket_default_width_and_volume_follow_the_chain() {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    let g = b.feature(
        kmb_cad_gear::GEAR,
        "鎖車",
        kmb_cad_gear::Gear {
            kind: kmb_interchange::GearKind::Sprocket,
            teeth: "17".into(),
            bore: Some("6 mm".into()),
            ..Default::default()
        },
    );
    let e = b.feature(
        kmb_cad_gear::GEAR_BODY,
        "鎖車のボディ",
        kmb_cad_gear::GearBody::new(g),
    );
    let mut doc = b.finish("鎖車");
    let reg = registry();
    let report = doc.recompute(&reg, &*k);
    assert_ok(&report);
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(g) else {
        panic!("輪郭")
    };
    let profile_area = area(&Profile::from_value(&outputs["profile"]).unwrap());
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(e) else {
        panic!("ボディ")
    };
    let spec = kmb_interchange::GearSpec::from_value(&outputs["gear"]).unwrap();
    let width = 7.95 * 0.93;
    assert!((spec.face_width.unwrap() - width).abs() < 1e-12);
    assert_eq!(spec.chain_pitch, Some(12.7));
    let mass = final_mass(&mut doc, &k);
    assert!(
        close(mass.volume, profile_area * width, 2e-3),
        "{} vs {}",
        mass.volume,
        profile_area * width
    );
}

#[test]
fn external_gears_keep_ten_mm_for_omitted_empty_and_explicit_widths() {
    let k = TestKernel::new();
    for width in [None, Some(""), Some("10 mm")] {
        let mut b = DocBuilder::new();
        let g = b.feature(
            kmb_cad_gear::GEAR,
            "平歯車",
            kmb_cad_gear::Gear {
                teeth: "12".into(),
                ..Default::default()
            },
        );
        let mut data = serde_json::json!({"gear":g});
        if let Some(width) = width {
            data["width"] = serde_json::json!(width);
        }
        let body: kmb_cad_gear::GearBody = serde_json::from_value(data).unwrap();
        let e = b.feature(kmb_cad_gear::GEAR_BODY, "ボディ", body);
        let mut doc = b.finish("平歯車");
        let report = doc.recompute(&registry(), &*k);
        assert_ok(&report);
        let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(e) else {
            panic!("ボディ")
        };
        let spec = kmb_interchange::GearSpec::from_value(&outputs["gear"]).unwrap();
        assert_eq!(spec.face_width, Some(10.0));
    }
}

#[test]
fn pulleys_and_rope_sheaves_revolve_their_sections_with_the_outline_width() {
    let k = TestKernel::new();
    for kind in [
        kmb_interchange::GearKind::FlatPulley,
        kmb_interchange::GearKind::VPulley,
        kmb_interchange::GearKind::RopeSheave,
    ] {
        for side in [
            kmb_cad_gear::body::Side::Normal,
            kmb_cad_gear::body::Side::Reverse,
            kmb_cad_gear::body::Side::Symmetric,
        ] {
            let mut b = DocBuilder::new();
            let g = b.feature(
                kmb_cad_gear::GEAR,
                "車",
                kmb_cad_gear::Gear {
                    kind,
                    pulley_width: Some("50 mm".into()),
                    groove_count: Some("2".into()),
                    bore: Some("10 mm".into()),
                    ..Default::default()
                },
            );
            let e = b.feature(
                kmb_cad_gear::GEAR_BODY,
                "ボディ",
                kmb_cad_gear::GearBody {
                    width: "5 mm".into(),
                    direction: side,
                    ..kmb_cad_gear::GearBody::new(g)
                },
            );
            let mut doc = b.finish("車");
            let reg = registry();
            let report = doc.recompute(&reg, &*k);
            assert_ok(&report);
            let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(g) else {
                panic!("断面")
            };
            let profile = Profile::from_value(&outputs["profile"]).unwrap();
            // 各細分の半径を直線とみなし、断面の回転体積 π∫r²dy を積分する。
            let integral: f64 = profile
                .regions
                .iter()
                .flat_map(|r| &r.outer.edges)
                .map(|edge| {
                    let mut points = edge.polyline(256);
                    points.push(edge.to_point());
                    points
                        .windows(2)
                        .map(|w| {
                            let (a, b) = (w[0], w[1]);
                            (b[1] - a[1]) * (a[0] * a[0] + a[0] * b[0] + b[0] * b[0]) / 3.0
                        })
                        .sum::<f64>()
                })
                .sum();
            let want = std::f64::consts::PI * integral;
            let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(e) else {
                panic!("ボディ")
            };
            let spec = kmb_interchange::GearSpec::from_value(&outputs["gear"]).unwrap();
            assert_eq!(spec.face_width, Some(50.0));
            assert!(
                kmb_core::warnings_of(outputs)
                    .iter()
                    .any(|s| s.contains("輪郭の幅"))
            );
            let mass = final_mass(&mut doc, &k);
            assert!(
                close(mass.volume, want, 2e-3),
                "{kind:?}: {} vs {want}",
                mass.volume
            );
            let (lo, hi) = match side {
                kmb_cad_gear::body::Side::Normal => (0.0, 50.0),
                kmb_cad_gear::body::Side::Reverse => (-50.0, 0.0),
                kmb_cad_gear::body::Side::Symmetric => (-25.0, 25.0),
            };
            assert!(
                (mass.min[2] - lo).abs() < 1e-5 && (mass.max[2] - hi).abs() < 1e-5,
                "{kind:?}: {:?} {:?}",
                mass.min,
                mass.max
            );
        }
    }
}

#[test]
fn an_axial_pulley_section_keeps_the_world_axis_when_parts_are_also_exported() {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    let g = b.feature(
        kmb_cad_gear::GEAR,
        "軸断面",
        kmb_cad_gear::Gear {
            kind: kmb_interchange::GearKind::VPulley,
            plane: kmb_cad::plane::PlaneRef::OriginYz,
            pulley_diameter: Some("80 mm".into()),
            pulley_width: Some("30 mm".into()),
            x: "7 mm".into(),
            y: "11 mm".into(),
            phase: Some("30 deg".into()),
            ..Default::default()
        },
    );
    let e = b.feature(
        kmb_cad_gear::GEAR_BODY,
        "車",
        kmb_cad_gear::GearBody {
            width: "5 mm".into(),
            direction: kmb_cad_gear::body::Side::Reverse,
            ..kmb_cad_gear::GearBody::new(g)
        },
    );
    let mut doc = b.finish("軸断面の車");
    let reg = registry();
    let report = doc.recompute(&reg, &*k);
    assert_ok(&report);
    let FeatureStatus::Ok { outputs, .. } = report.feature(e).unwrap() else {
        panic!("ボディ")
    };
    let spec = kmb_interchange::GearSpec::from_value(&outputs["gear"]).unwrap();
    assert_eq!(spec.axis.origin, [-30.0, 7.0, 11.0]);
    assert_eq!(spec.face_width, Some(30.0));
    let mass = final_mass(&mut doc, &k);
    assert!(
        (mass.min[0] + 30.0).abs() < 1e-5 && mass.max[0].abs() < 1e-5,
        "{mass:?}"
    );
    assert!(
        ((mass.min[1] + mass.max[1]) / 2.0 - 7.0).abs() < 1e-5,
        "{mass:?}"
    );
    assert!(
        ((mass.min[2] + mass.max[2]) / 2.0 - 11.0).abs() < 1e-5,
        "{mass:?}"
    );
}

/// Actual-kernel coverage for the new arms; geometry is generated through the normal features.
fn spoke_volume(section: kmb_cad_gear::SpokeSection, angle: f64, round: f64) -> f64 {
    let k = TestKernel::new();
    assert!(!k.is_truck(), "SMLib actual-shape helper");
    let mut b = DocBuilder::new();
    let g = b.feature(
        kmb_cad_gear::GEAR,
        "腕の歯車",
        kmb_cad_gear::Gear {
            module: "2 mm".into(),
            teeth: "24".into(),
            bore: Some("6 mm".into()),
            boss_diameter: Some("16 mm".into()),
            web_form: kmb_cad_gear::WebForm::Spokes,
            spoke_count: Some("5".into()),
            spoke_root_width: Some("2 mm".into()),
            spoke_tip_width: Some("1.5 mm".into()),
            spoke_curve: kmb_cad_gear::SpokeCurve::Curved,
            spoke_curve_angle: Some(format!("{angle} deg")),
            spoke_root_round: Some(format!("{round} mm")),
            ..Default::default()
        },
    );
    let e = b.feature(
        kmb_cad_gear::GEAR_BODY,
        "腕の立体",
        kmb_cad_gear::GearBody {
            width: "6 mm".into(),
            web: Some("3 mm".into()),
            boss: Some("10 mm".into()),
            spoke_section: section,
            ..kmb_cad_gear::GearBody::new(g)
        },
    );
    let mut doc = b.finish("腕の断面");
    let reg = registry();
    let report = doc.recompute(&reg, &*k);
    assert_ok(&report);
    let mass = final_mass(&mut doc, &k);
    assert!(mass.volume.is_finite() && mass.volume > 0.0);
    let FeatureStatus::Ok { outputs, .. } = report.feature(e).unwrap() else {
        unreachable!()
    };
    let recipe = kmb_cad::Recipe::from_value(&outputs["recipe"]).unwrap();
    let expected = independent_spoke_union_volume(&k, &recipe, section);
    assert!(
        close(mass.volume, expected, 2e-3),
        "{section:?}/{angle}/{round}: assembled {} vs independently summed/overlap-subtracted {expected}",
        mass.volume
    );
    mass.volume
}
#[test]
fn rectangular_elliptic_and_cross_spokes_have_distinct_positive_actual_volumes() {
    if TestKernel::new().is_truck() {
        assert_truck_spoke_sweep_is_unsupported();
        kmb_contract::skip!("truck は断面を相似に変える腕の Sweep に未対応");
    }
    use kmb_cad_gear::SpokeSection::{Cross, Ellipse, Rectangle};
    assert_swept_spoke_volume_and_connected_overlap();
    for angle in [0.0, 30.0] {
        let rectangle = spoke_volume(Rectangle, angle, 0.0);
        let ellipse = spoke_volume(Ellipse, angle, 0.0);
        let cross = spoke_volume(Cross, angle, 0.0);
        assert!(
            cross < ellipse && ellipse < rectangle,
            "cross {cross}, ellipse {ellipse}, rectangle {rectangle}"
        );
        if angle == 0.0 {
            for (section, volume) in [(Rectangle, rectangle), (Ellipse, ellipse), (Cross, cross)] {
                assert_spoke_target_operations(section, volume);
            }
        }
    }
}
#[test]
fn root_rounding_increases_actual_volume_for_all_three_sections() {
    if TestKernel::new().is_truck() {
        assert_truck_spoke_sweep_is_unsupported();
        kmb_contract::skip!("truck は断面を相似に変える腕の Sweep に未対応");
    }
    use kmb_cad_gear::SpokeSection::{Cross, Ellipse, Rectangle};
    for section in [Rectangle, Ellipse, Cross] {
        assert!(
            spoke_volume(section, 30.0, 0.5) > spoke_volume(section, 30.0, 0.0),
            "{section:?}"
        );
    }
}

fn assert_swept_spoke_volume_and_connected_overlap() {
    use kmb_cad_gear::SpokeSection::{Cross, Ellipse};
    let k = TestKernel::new();
    for section in [Ellipse, Cross] {
        for angle in [0.0, 30.0] {
            let mut b = DocBuilder::new();
            let g = b.feature(
                kmb_cad_gear::GEAR,
                "腕の輪郭",
                kmb_cad_gear::Gear {
                    module: "2 mm".into(),
                    teeth: "24".into(),
                    boss_diameter: Some("16 mm".into()),
                    web_form: kmb_cad_gear::WebForm::Spokes,
                    spoke_count: Some("5".into()),
                    spoke_root_width: Some("2 mm".into()),
                    spoke_tip_width: Some("1.5 mm".into()),
                    spoke_curve: kmb_cad_gear::SpokeCurve::Curved,
                    spoke_curve_angle: Some(format!("{angle} deg")),
                    ..Default::default()
                },
            );
            let e = b.feature(
                kmb_cad_gear::GEAR_BODY,
                "腕の立体",
                kmb_cad_gear::GearBody {
                    width: "6 mm".into(),
                    web: Some("3 mm".into()),
                    spoke_section: section,
                    ..kmb_cad_gear::GearBody::new(g)
                },
            );
            let mut doc = b.finish("断面積と体積");
            let report = doc.recompute(&registry(), &*k);
            assert_ok(&report);
            let FeatureStatus::Ok { outputs, .. } = report.feature(e).unwrap() else {
                unreachable!()
            };
            let kmb_cad::Recipe::Many { parts } =
                kmb_cad::Recipe::from_value(&outputs["recipe"]).unwrap()
            else {
                panic!("compound")
            };
            let extruded: Vec<_> = parts
                .iter()
                .filter_map(|recipe| {
                    if let kmb_cad::Recipe::Extrude {
                        profile, from, to, ..
                    } = recipe
                    {
                        Some(k.extrude(profile, *from, *to, "connection").unwrap())
                    } else {
                        None
                    }
                })
                .collect();
            assert_eq!(extruded.len(), 2, "rim and boss");
            let mut tested = 0;
            for recipe in parts {
                let kmb_cad::Recipe::Sweep {
                    profile,
                    path,
                    twist,
                    scale,
                    ..
                } = recipe
                else {
                    continue;
                };
                let length: f64 = path
                    .edges
                    .iter()
                    .map(|e| match e.segment {
                        kmb_core::Segment::Line { a, b } => (b[0] - a[0]).hypot(b[1] - a[1]),
                        kmb_core::Segment::Arc {
                            radius, start, end, ..
                        } => radius * (end - start),
                        _ => panic!("one line or arc"),
                    })
                    .sum();
                let section_area = match section {
                    Ellipse => std::f64::consts::PI * 2.0 * 3.0 / 4.0,
                    Cross => 5.0 * 2.0 * 3.0 / 9.0,
                    _ => unreachable!(),
                };
                let expected = section_area * length * (1.0 + 0.75 + 0.75 * 0.75) / 3.0;
                assert_eq!(scale, 0.75);
                assert_eq!(twist, 0.0);
                let shape = k
                    .sweep(&profile, &path, twist, scale, "section-volume")
                    .unwrap();
                let actual = k.mass(shape).unwrap().volume;
                // Positive intersection with each independently extruded neighbor proves
                // volumetric connection, rather than just one compound output handle.
                for neighbor in &extruded {
                    let overlap = k
                        .boolean(kmb_core::BooleanOp::Intersect, shape, *neighbor)
                        .unwrap();
                    let overlap_volume = k.mass(overlap).unwrap().volume;
                    assert!(
                        overlap_volume.is_finite() && overlap_volume > 1e-6,
                        "{section:?}/{angle}: arm must overlap rim and boss, got {overlap_volume}"
                    );
                }
                assert!(
                    close(actual, expected, 2e-3),
                    "{section:?}/{angle}: {actual} vs {expected}"
                );
                tested += 1;
            }
            assert_eq!(tested, 5);
        }
    }
}

fn assert_truck_spoke_sweep_is_unsupported() {
    let k = TestKernel::new();
    assert!(k.is_truck());
    let profile = Profile {
        plane: kmb_core::Plane::YZ,
        regions: vec![kmb_core::Region {
            outer: kmb_core::ProfileLoop {
                edges: vec![kmb_core::ProfileEdge {
                    name: "section".into(),
                    segment: kmb_core::Segment::Circle {
                        center: [0.0; 2],
                        radius: 1.0,
                    },
                    reversed: false,
                }],
            },
            holes: vec![],
        }],
    };
    let path = kmb_core::PlanarPath {
        plane: kmb_core::Plane::XY,
        edges: vec![kmb_core::ProfileEdge {
            name: "path".into(),
            segment: kmb_core::Segment::Line {
                a: [0.0; 2],
                b: [10.0, 0.0],
            },
            reversed: false,
        }],
    };
    assert!(matches!(
        k.sweep(&profile, &path, 0.0, 0.75, "spoke-backend"),
        Err(kmb_core::KernelError::Unsupported(_))
    ));
}

/// The ring, boss and root pads have disjoint interiors. Distinct arms occupy
/// disjoint angular sectors. Pads are outside the boss and inside the rim's hole,
/// so there are no triple intersections. Only sweep/extrusion pairs overlap.
/// This uses intersection, not union, to subtract the shared volume.
fn independent_spoke_union_volume(
    k: &TestKernel,
    recipe: &kmb_cad::Recipe,
    section: kmb_cad_gear::SpokeSection,
) -> f64 {
    let kmb_cad::Recipe::Many { parts } = recipe else {
        panic!("different-thickness parts")
    };
    let mut expected = 0.0;
    let mut extrusions = Vec::new();
    let mut sweeps = Vec::new();
    for part in parts {
        match part {
            kmb_cad::Recipe::Extrude {
                profile, from, to, ..
            } => {
                expected += area(profile) * (to - from);
                extrusions.push(
                    k.extrude(profile, *from, *to, "union-oracle-extrusion")
                        .unwrap(),
                );
            }
            kmb_cad::Recipe::Sweep {
                profile,
                path,
                twist,
                scale,
                ..
            } => {
                let length: f64 = path
                    .edges
                    .iter()
                    .map(|e| match e.segment {
                        kmb_core::Segment::Line { a, b } => (b[0] - a[0]).hypot(b[1] - a[1]),
                        kmb_core::Segment::Arc {
                            radius, start, end, ..
                        } => radius * (end - start),
                        _ => panic!("one line or arc"),
                    })
                    .sum();
                let root_area = match section {
                    kmb_cad_gear::SpokeSection::Ellipse => std::f64::consts::PI * 2.0 * 3.0 / 4.0,
                    kmb_cad_gear::SpokeSection::Cross => 5.0 * 2.0 * 3.0 / 9.0,
                    _ => panic!("swept section"),
                };
                expected += root_area * length * (1.0 + scale + scale * scale) / 3.0;
                sweeps.push(
                    k.sweep(profile, path, *twist, *scale, "union-oracle-arm")
                        .unwrap(),
                );
            }
            _ => panic!("extrusion or sweep"),
        }
    }
    for arm in sweeps {
        for neighbor in &extrusions {
            let overlap = k
                .boolean(kmb_core::BooleanOp::Intersect, arm, *neighbor)
                .unwrap();
            let volume = k.mass(overlap).unwrap().volume;
            assert!(
                volume.is_finite() && volume > 1e-6,
                "positive-volume arm connection: {volume}"
            );
            expected -= volume;
        }
    }
    assert!(expected.is_finite() && expected > 0.0);
    expected
}

fn assert_spoke_target_operations(section: kmb_cad_gear::SpokeSection, new_volume: f64) {
    use super::tests::rect_sketch;
    use super::{Direction, EXTRUDE, Extrude, Operation};
    for operation in [Operation::Join, Operation::Cut, Operation::Intersect] {
        let k = TestKernel::new();
        let mut b = DocBuilder::new();
        let sk = rect_sketch(
            &mut b,
            kmb_sketch::SketchPlane::Xy,
            -40.0,
            -40.0,
            80.0,
            80.0,
        );
        let stock = b.feature(
            EXTRUDE,
            "材料",
            Extrude {
                profile: Some(sk),
                distance: "30 mm".into(),
                direction: Direction::Symmetric,
                ..Default::default()
            },
        );
        let g = b.feature(
            kmb_cad_gear::GEAR,
            "腕の輪郭",
            kmb_cad_gear::Gear {
                module: "2 mm".into(),
                teeth: "24".into(),
                bore: Some("6 mm".into()),
                boss_diameter: Some("16 mm".into()),
                web_form: kmb_cad_gear::WebForm::Spokes,
                spoke_count: Some("5".into()),
                spoke_root_width: Some("2 mm".into()),
                spoke_tip_width: Some("1.5 mm".into()),
                ..Default::default()
            },
        );
        let intersection = operation == Operation::Intersect;
        let e = b.feature(
            kmb_cad_gear::GEAR_BODY,
            "材料との演算",
            kmb_cad_gear::GearBody {
                width: "6 mm".into(),
                web: (!intersection).then(|| "3 mm".into()),
                boss: (!intersection).then(|| "10 mm".into()),
                spoke_section: section,
                operation,
                target: Some(stock),
                ..kmb_cad_gear::GearBody::new(g)
            },
        );
        let mut doc = b.finish("腕と材料");
        let report = doc.recompute(&registry(), &*k);
        if intersection && section != kmb_cad_gear::SpokeSection::Rectangle {
            let status = &report.feature(e).unwrap();
            assert!(matches!(status, FeatureStatus::Error { .. }), "{status:?}");
            assert!(format!("{status:?}").contains("新しいボディ"));
            continue;
        }
        assert_ok(&report);
        let expected = match operation {
            Operation::Join => 80.0 * 80.0 * 30.0,
            Operation::Cut => 80.0 * 80.0 * 30.0 - new_volume,
            Operation::Intersect => {
                let FeatureStatus::Ok { outputs, .. } = report.feature(g).unwrap() else {
                    unreachable!()
                };
                area(&Profile::from_value(&outputs["profile"]).unwrap()) * 6.0
            }
            _ => unreachable!(),
        };
        let mass = final_mass(&mut doc, &k);
        assert!(
            close(mass.volume, expected, 2e-3),
            "{section:?}/{operation:?}: {} vs {expected}",
            mass.volume
        );
    }
}
