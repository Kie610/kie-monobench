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
        lightening_count: Some("s".into()),
        lightening_diameter: Some("s".into()),
        lightening_circle: Some("s".into()),
        boss_diameter: Some("s".into()),
        key_width: Some("s".into()),
        key_depth: Some("s".into()),
        ..Gear::default()
    };
    assert_eq!(g.exprs_mut().len(), 16);
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

#[test]
fn default_hub_fields_do_not_change_saved_gear_data() {
    let gear = Gear::default();
    let saved = serde_json::to_value(&gear).unwrap();
    for key in [
        "lightening_count",
        "lightening_diameter",
        "lightening_circle",
        "boss_diameter",
        "keyway",
        "key_width",
        "key_depth",
    ] {
        assert!(saved.get(key).is_none(), "{key}: {saved}");
    }
    assert_eq!(body::Alignment::default(), body::Alignment::Centered);
}

#[test]
fn a_count_expression_rounded_to_an_integer_keeps_every_hole() {
    let mut doc = kmb_core::Document::new();
    let mut reg = Registry::new();
    register(&mut reg);
    let gear = Gear {
        module: "2 mm".into(),
        teeth: "24".into(),
        lightening_count: Some("0.3 / 0.1".into()),
        lightening_diameter: Some("2 mm".into()),
        lightening_circle: Some("20 mm".into()),
        ..Default::default()
    };
    let id = doc
        .apply_edits(
            "穴の数",
            vec![kmb_core::Edit::Insert {
                type_id: GEAR.into(),
                name: "歯車".into(),
                data: serde_json::to_value(gear).unwrap(),
            }],
        )
        .unwrap()[0];
    let report = doc.recompute(&reg, &kmb_core::NullKernel);
    assert_eq!(report.error_count(), 0, "{report:?}");
    let Some(kmb_core::FeatureStatus::Ok { outputs, .. }) = report.feature(id) else {
        panic!("輪郭を計算できる");
    };
    let profile = kmb_core::Profile::from_value(&outputs["profile"]).unwrap();
    assert_eq!(profile.regions[0].holes.len(), 3);
}

#[test]
fn keyway_size_fields_are_available_with_the_boolean_enabled() {
    let mut reg = Registry::new();
    register(&mut reg);
    let descriptor = reg.descriptor(GEAR).unwrap();
    let data = json!({"keyway": true});
    for key in ["key_width", "key_depth"] {
        let field = descriptor.fields.iter().find(|f| f.key == key).unwrap();
        assert!(field.applies(&data));
    }
}
