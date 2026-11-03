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
