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

/// 確かめること: 異厚のはすばを勝手に同厚へ変えず、保存された全入力を保って明示拒否する。
#[test]
fn helical_parts_reject_unequal_thickness_without_changing_input_data() {
    for beta in [25, 35] {
        let k = TestKernel::new();
        let mut b = DocBuilder::new();
        let gear = b.feature(
            kmb_cad_gear::GEAR,
            "はすば",
            kmb_cad_gear::Gear {
                module: "2 mm".into(),
                teeth: "24".into(),
                helix_angle: Some(format!("{beta} deg")),
                bore: Some("8 mm".into()),
                boss_diameter: Some("16 mm".into()),
                keyway: true,
                lightening_count: Some("4".into()),
                lightening_diameter: Some("3 mm".into()),
                lightening_circle: Some("26 mm".into()),
                ..Default::default()
            },
        );
        let data = serde_json::to_value(kmb_cad_gear::GearBody {
            width: "8 mm".into(),
            web: Some("3 mm".into()),
            boss: Some("12 mm".into()),
            alignment: kmb_cad_gear::body::Alignment::Start,
            ..kmb_cad_gear::GearBody::new(gear)
        })
        .unwrap();
        let body = b.feature(kmb_cad_gear::GEAR_BODY, "異厚", data.clone());
        let mut doc = b.finish("異厚は未対応");
        let report = doc.recompute(&registry(), &*k);
        let Some(FeatureStatus::Error { message }) = report.feature(body) else {
            panic!("異厚を明示拒否")
        };
        assert!(
            message.contains("異なる厚さには対応していません")
                && message.contains("Different thicknesses are not supported"),
            "{message}"
        );
        assert_eq!(doc.data().feature(body).unwrap().data, data);
    }
}

/// 確かめること: 許された同厚の円板は穴とキー溝も一緒にねじり、位相ごとの穴と体積を保つ。
#[test]
fn whole_plate_helices_keep_twisted_holes_and_keyway_at_equal_thickness() {
    for herringbone in [false, true] {
        let k = TestKernel::new();
        let mut b = DocBuilder::new();
        let gear = b.feature(
            kmb_cad_gear::GEAR,
            "同厚のはすば",
            kmb_cad_gear::Gear {
                module: "2 mm".into(),
                teeth: "24".into(),
                helix_angle: Some("25 deg".into()),
                bore: Some("8 mm".into()),
                boss_diameter: Some("16 mm".into()),
                keyway: true,
                lightening_count: Some("4".into()),
                lightening_diameter: Some("3 mm".into()),
                lightening_circle: Some("26 mm".into()),
                ..Default::default()
            },
        );
        let body = b.feature(
            kmb_cad_gear::GEAR_BODY,
            "同厚の円板",
            kmb_cad_gear::GearBody {
                width: "8 mm".into(),
                web: Some("8 mm".into()),
                boss: Some("8 mm".into()),
                herringbone,
                ..kmb_cad_gear::GearBody::new(gear)
            },
        );
        let mut doc = b.finish("円板全体をねじる");
        let report = doc.recompute(&registry(), &*k);
        if k.is_truck() {
            let Some(FeatureStatus::Error { message }) = report.feature(body) else {
                panic!("未対応のスイープ")
            };
            assert!(message.contains("スイープ") && message.contains("対応していません"));
            continue;
        }
        assert_ok(&report);
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
        assert!(close(
            k.mass(shape).unwrap().volume,
            area(&profile) * 8.0,
            0.01
        ));
        let mesh = k.mesh(shape, 0.025).unwrap();
        for z in [0.05_f64, 3.95, 4.05, 7.95] {
            let distance = if herringbone { z.min(8.0 - z) } else { z };
            let turn = distance * spec.helix_angle.tan() / spec.pitch_radius;
            let p = |x: f64, y: f64| {
                [
                    x * turn.cos() - y * turn.sin(),
                    x * turn.sin() + y * turn.cos(),
                    z,
                ]
            };
            for t in [-1.35, 0.0, 1.35] {
                assert!(!mesh.contains(p(13.0, t)), "穴の位相 {z}");
            }
            for x in [-0.85, 0.85] {
                assert!(!mesh.contains(p(x, 4.7)), "キー溝の位相 {z}");
            }
            assert!(mesh.contains(p(11.0, 0.0)));
            assert!(mesh.contains(p(5.0, 0.0)));
        }
    }
}

/// 確かめること: 軸からずれた穴を持つやまばの中央溝を、穴の位置を変えて勝手に埋めない。
#[test]
fn herringbone_center_groove_rejects_offset_holes_before_kernel_work() {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    let gear = b.feature(
        kmb_cad_gear::GEAR,
        "穴付き",
        kmb_cad_gear::Gear {
            module: "2 mm".into(),
            teeth: "24".into(),
            helix_angle: Some("25 deg".into()),
            bore: Some("8 mm".into()),
            keyway: true,
            ..Default::default()
        },
    );
    let body = b.feature(
        kmb_cad_gear::GEAR_BODY,
        "未対応の溝",
        kmb_cad_gear::GearBody {
            width: "8 mm".into(),
            herringbone: true,
            groove: Some("1 mm".into()),
            ..kmb_cad_gear::GearBody::new(gear)
        },
    );
    let mut doc = b.finish("溝を明示拒否");
    let report = doc.recompute(&registry(), &*k);
    let Some(FeatureStatus::Error { message }) = report.feature(body) else {
        panic!("穴を保って拒否")
    };
    assert!(
        message.contains("この組み合わせには対応していません")
            && message.contains("This combination is not supported")
    );
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
