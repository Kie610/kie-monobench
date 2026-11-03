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
        chain_pitch: Some("s".into()),
        roller_diameter: Some("s".into()),
        chain_inner_width: Some("s".into()),
        ..Gear::default()
    };
    assert_eq!(g.exprs_mut().len(), 13);
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
    assert_eq!(b.width, "");
    assert!(serde_json::from_value::<GearBody>(json!({"gear": 3, "side": "normal"})).is_err());
    let v = serde_json::to_value(&b).unwrap();
    assert_eq!(v, json!({"gear": 3}));
}

#[test]
fn sprocket_outputs_chain_pitch_and_its_default_body_width() {
    use kmb_core::{Document, Edit, FeatureStatus, NullKernel};
    for (data, want_pitch, want_width) in [
        (json!({"kind":"sprocket","teeth":"17"}), 12.7, 7.95 * 0.93),
        (
            json!({"kind":"sprocket","teeth":"17","chain":"manual","chain_pitch":"14 mm","roller_diameter":"8 mm","chain_inner_width":"9 mm"}),
            14.0,
            9.0 * 0.95,
        ),
        (json!({"teeth":"20"}), 0.0, 10.0),
    ] {
        let mut doc = Document::new();
        let mut reg = Registry::new();
        register(&mut reg);
        let id = doc
            .apply_edits(
                "輪郭",
                vec![Edit::Insert {
                    type_id: GEAR.into(),
                    name: "輪郭".into(),
                    data,
                }],
            )
            .unwrap()[0];
        let report = doc.recompute(&reg, &NullKernel);
        assert_eq!(report.error_count(), 0, "{report:?}");
        let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(id) else {
            panic!("輪郭")
        };
        let spec = GearSpec::from_value(&outputs[GEAR_OUTPUT]).unwrap();
        assert_eq!(spec.chain_pitch.unwrap_or(0.0), want_pitch);
        assert!(spec.face_width.is_none());
        if want_pitch > 0.0 {
            assert_eq!(spec.module, 0.0);
        }
        let kmb_core::Value::Scalar(width) = &outputs[DEFAULT_WIDTH_OUTPUT] else {
            panic!("長さ")
        };
        assert_eq!(width.dim, Dim::LENGTH);
        assert!((width.value - want_width).abs() < 1e-12);
    }
}

#[test]
fn omitted_and_empty_body_widths_remain_automatic_but_explicit_width_is_preserved() {
    for data in [json!({"gear":3}), json!({"gear":3,"width":""})] {
        let body: GearBody = serde_json::from_value(data).unwrap();
        assert!(body.width.is_empty());
        assert!(
            body.inputs()
                .unwrap()
                .contains(&output(FeatureId(3), DEFAULT_WIDTH_OUTPUT))
        );
    }
    let body: GearBody = serde_json::from_value(json!({"gear":3,"width":"10 mm"})).unwrap();
    assert_eq!(body.width, "10 mm");
    assert!(
        !body
            .inputs()
            .unwrap()
            .contains(&output(FeatureId(3), DEFAULT_WIDTH_OUTPUT))
    );
}
