//! 確かめること: 機能のデータの読み書き(知らない鍵は誤り、省いた欄は既定)、式の欄の並び、
//! 作れない種類の誤り、歯車のボディのデータ。

use super::*;
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
    assert_eq!(exprs(&mut g).len(), 10);
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
