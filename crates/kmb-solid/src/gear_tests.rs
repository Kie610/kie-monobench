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
