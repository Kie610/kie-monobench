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
        pulley_diameter: Some("s".into()),
        pulley_width: Some("s".into()),
        crown_height: Some("s".into()),
        groove_count: Some("s".into()),
        groove_angle: Some("s".into()),
        groove_top_width: Some("s".into()),
        groove_depth: Some("s".into()),
        rope_diameter: Some("s".into()),
        flange_height: Some("s".into()),

        ..Gear::default()
    };
    assert_eq!(g.exprs_mut().len(), 19);
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
fn pulley_profiles_contain_the_axis_and_curves_stay_perpendicular_to_it() {
    use kmb_core::{Document, Edit, FeatureStatus, NullKernel};
    for kind in [
        GearKind::FlatPulley,
        GearKind::VPulley,
        GearKind::RopeSheave,
    ] {
        let mut reg = Registry::new();
        register(&mut reg);
        let mut doc = Document::new();
        let data = serde_json::to_value(Gear {
            kind,
            plane: PlaneRef::OriginYz,
            pulley_diameter: Some("80 mm".into()),
            pulley_width: Some("30 mm".into()),
            x: "7 mm".into(),
            y: "11 mm".into(),
            phase: Some("30 deg".into()),
            ..Default::default()
        })
        .unwrap();
        let id = doc
            .apply_edits(
                "輪郭",
                vec![Edit::Insert {
                    type_id: GEAR.into(),
                    name: "車".into(),
                    data,
                }],
            )
            .unwrap()[0];
        let report = doc.recompute(&reg, &NullKernel);
        assert_eq!(report.error_count(), 0, "{report:?}");
        let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(id) else {
            panic!("輪郭")
        };
        let profile = Profile::from_value(&outputs["profile"]).unwrap();
        let curves = Curves::from_value(&outputs["curves"]).unwrap();
        let spec = GearSpec::from_value(&outputs[GEAR_OUTPUT]).unwrap();
        assert_eq!(spec.kind, kind);
        assert_eq!(spec.module, 0.0);
        assert_eq!(spec.teeth, 0);
        assert_eq!(spec.pitch_radius, 40.0);
        assert_eq!(profile.plane.y_axis, spec.axis.direction);
        assert_eq!(curves.plane.normal(), spec.axis.direction);
        assert_eq!(curves.segments.len(), 1);
        assert!(matches!(curves.segments[0], Segment::Circle { .. }));
        assert!(
            kmb_core::warnings_of(outputs)
                .iter()
                .any(|s| s.contains("軸断面") && s.contains("2D"))
        );
    }
}
