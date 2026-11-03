//! 確かめること: 機能のデータの読み書き(知らない鍵は誤り、省いた欄は既定)、式の欄の並び、
//! 作れない種類の誤り、歯車のボディのデータ。

use super::*;
use kmb_cad::Exprs;
use serde_json::json;

#[test]
fn unknown_keys_are_errors_and_omitted_fields_take_defaults() {
    let g: Gear = serde_json::from_value(json!({"module": "2 mm", "teeth": "24"})).unwrap();
    assert_eq!(g.kind, GearKind::External);
    assert_eq!(g.pressure_angle, "20 deg");
    assert_eq!(g.x, "0 mm");
    assert!(serde_json::from_value::<Gear>(json!({"modul": "2 mm"})).is_err());
    // 書き戻すと既定の種類と空の欄は省く
    let v = serde_json::to_value(&g).unwrap();
    assert!(v.get("kind").is_none() && v.get("bore").is_none(), "{v}");
}

#[test]
fn every_expression_field_is_listed_for_renaming() {
    let mut g = Gear {
        profile_shift: Some("s".into()),
        tip_round: Some("s".into()),
        root_round: Some("s".into()),
        bore: Some("s".into()),
        phase: Some("s".into()),
        ..Gear::default()
    };
    assert_eq!(g.exprs_mut().len(), 10);
}

#[test]
fn kinds_without_a_shape_yet_say_so() {
    for k in GearKind::ALL {
        let r = kinds::outline(k, &kinds::Params::default());
        assert_eq!(r.is_ok(), kinds::available(k), "{k:?}");
        if let Err(e) = r {
            assert!(e.contains("まだ作れません") && e.contains(k.label()), "{e}");
        }
    }
}

#[test]
fn the_body_reads_its_gear_and_takes_the_default_width() {
    let b: GearBody = serde_json::from_value(json!({"gear": 3})).unwrap();
    assert_eq!(b, GearBody::new(FeatureId(3)));
    assert_eq!(b.width, "10 mm");
    assert!(serde_json::from_value::<GearBody>(json!({"gear": 3, "side": "normal"})).is_err());
    let v = serde_json::to_value(&b).unwrap();
    assert_eq!(v, json!({"gear": 3, "width": "10 mm"}));
}

fn internal_profile(data: serde_json::Value) -> Result<(Profile, GearSpec), String> {
    let mut doc = kmb_core::Document::new();
    let mut reg = Registry::new();
    register(&mut reg);
    let id = doc
        .apply_edits(
            "内歯車",
            vec![kmb_core::Edit::Insert {
                type_id: GEAR.into(),
                name: "内歯車".into(),
                data,
            }],
        )
        .unwrap()[0];
    let report = doc.recompute(&reg, &kmb_core::NullKernel);
    match report.feature(id).unwrap() {
        kmb_core::FeatureStatus::Ok { outputs, .. } => Ok((
            Profile::from_value(&outputs["profile"]).unwrap(),
            GearSpec::from_value(&outputs[GEAR_OUTPUT]).unwrap(),
        )),
        kmb_core::FeatureStatus::Error { message } => Err(message.clone()),
        other => panic!("{other:?}"),
    }
}

#[test]
fn internal_gears_are_a_ring_with_a_clockwise_tooth_hole_and_internal_spec() {
    let (p, spec) = internal_profile(
        json!({"kind":"internal", "module":"2 mm", "teeth":"20", "outer_diameter":"60 mm"}),
    )
    .unwrap();
    assert_eq!(spec.kind, GearKind::Internal);
    assert_eq!(spec.teeth, 20);
    assert_eq!(spec.pitch_radius, 20.0);
    let region = &p.regions[0];
    assert_eq!(region.outer.edges[0].name, "rim");
    assert_eq!(region.holes.len(), 1);
    assert!(region.outer.signed_area() > 0.0 && region.holes[0].signed_area() < 0.0);
    let area = region.outer.signed_area() + region.holes[0].signed_area();
    let pitch_ring = std::f64::consts::PI / 4.0 * (60.0f64.powi(2) - 40.0f64.powi(2));
    assert!(
        (area - pitch_ring).abs() < 0.05 * pitch_ring,
        "{area} vs {pitch_ring}"
    );
}

#[test]
fn an_internal_outer_diameter_is_required_and_the_minimum_is_in_the_error() {
    let base = json!({"kind":"internal", "module":"2 mm", "teeth":"20"});
    assert!(
        internal_profile(base.clone())
            .unwrap_err()
            .contains("輪の外径")
    );
    let mut data = base.clone();
    data["outer_diameter"] = json!("48 mm");
    assert!(
        internal_profile(data.clone())
            .unwrap_err()
            .contains("49.000 mm")
    );
    data["outer_diameter"] = json!("49 mm");
    assert!(
        internal_profile(data.clone()).is_ok(),
        "最小の外径ちょうどは使える"
    );
    data["outer_diameter"] = json!("2 in");
    let (profile, _) = internal_profile(data).unwrap();
    let Segment::Circle { radius, .. } = profile.regions[0].outer.edges[0].segment else {
        panic!("外径の円")
    };
    assert!((radius - 25.4).abs() < 1e-10);

    let mut data = base;
    data["outer_diameter"] = json!("60 mm");
    data["bore"] = json!("2 mm");
    assert!(internal_profile(data).unwrap_err().contains("軸の穴"));
}

#[test]
fn internal_outer_diameter_is_an_expression_for_dependency_and_rename() {
    let mut gear = Gear {
        kind: GearKind::Internal,
        outer_diameter: Some("rim_size".into()),
        ..Gear::default()
    };
    assert!(gear.exprs_mut().iter().any(|v| v.as_str() == "rim_size"));
    let renamed = gear.rename_param("rim_size", "ring_size").unwrap().unwrap();
    assert_eq!(renamed["outer_diameter"], "ring_size");
}
