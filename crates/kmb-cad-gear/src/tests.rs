//! 確かめること: 機能のデータの読み書き(知らない鍵は誤り、省いた欄は既定)、式の欄の並び、
//! 作れない種類の誤り、歯車のボディのデータ。

use super::*;
use kmb_cad::Exprs;
use kmb_core::Profile;
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

        chain_pitch: Some("s".into()),
        roller_diameter: Some("s".into()),
        chain_inner_width: Some("s".into()),
        lightening_count: Some("s".into()),
        lightening_diameter: Some("s".into()),
        lightening_circle: Some("s".into()),
        boss_diameter: Some("s".into()),
        key_width: Some("s".into()),
        key_depth: Some("s".into()),
        rack_length: Some("s".into()),
        rack_back: Some("s".into()),
        outer_diameter: Some("s".into()),
        ..Gear::default()
    };
    assert_eq!(g.exprs_mut().len(), 31);
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
        (
            json!({"kind":"sprocket","teeth":"17","bore":"6 mm"}),
            12.7,
            7.95 * 0.93,
        ),
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
        let profile = Profile::from_value(&outputs["profile"]).unwrap();
        let parts = hub::Parts::from_value(&outputs[hub::PARTS_OUTPUT]).unwrap();
        if spec.kind == GearKind::Sprocket {
            assert_eq!(parts.rim, profile);
            assert!(parts.web.is_none() && parts.boss.is_none());
        } else {
            assert_eq!(spec.kind, GearKind::External);
            assert_eq!(parts.rim.regions[0].outer, profile.regions[0].outer);
            assert_eq!(parts.rim.regions[0].holes.len(), 1);
            assert!(parts.web.is_some() && parts.boss.is_none());
        }
        if want_pitch == 12.7 {
            assert_eq!(profile.regions[0].holes.len(), 1);
            assert_eq!(profile.regions[0].holes[0].edges[0].name, "bore");
        }
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
        kmb_core::FeatureStatus::Ok { outputs, .. } => {
            let profile = Profile::from_value(&outputs["profile"]).unwrap();
            let parts = hub::Parts::from_value(&outputs[hub::PARTS_OUTPUT]).unwrap();
            assert_eq!(parts.rim, profile);
            assert!(parts.web.is_none() && parts.boss.is_none());
            Ok((
                profile,
                GearSpec::from_value(&outputs[GEAR_OUTPUT]).unwrap(),
            ))
        }
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

#[test]
fn internal_and_rack_fields_remain_available_for_their_own_kinds() {
    let mut reg = Registry::new();
    register(&mut reg);
    let descriptor = reg.descriptor(GEAR).unwrap();
    for kind in [GearKind::External, GearKind::Internal, GearKind::Rack] {
        let data = json!({"kind":kind.key(),"rack_size":"teeth"});
        let applies = |key: &str| {
            descriptor
                .fields
                .iter()
                .find(|field| field.key == key)
                .unwrap()
                .applies(&data)
        };
        assert!(applies("teeth"));
        assert_eq!(applies("profile_shift"), kind != GearKind::Rack);
        assert_eq!(applies("bore"), kind == GearKind::External);
        assert_eq!(applies("outer_diameter"), kind == GearKind::Internal);
        assert_eq!(applies("rack_size"), kind == GearKind::Rack);
        assert_eq!(applies("rack_back"), kind == GearKind::Rack);
        assert!(!applies("rack_length"));
    }
    let length = json!({"kind":"rack","rack_size":"length"});
    for (key, expected) in [
        ("teeth", false),
        ("rack_length", true),
        ("rack_back", true),
        ("outer_diameter", false),
    ] {
        let field = descriptor
            .fields
            .iter()
            .find(|field| field.key == key)
            .unwrap();
        assert_eq!(field.applies(&length), expected, "{key}");
    }
}

#[test]
fn default_rack_parts_keep_the_finite_placed_profile() {
    let (profile, spec) = internal_profile(json!({
        "kind":"rack", "module":"2 mm", "teeth":"12", "rack_back":"5 mm",
        "phase":"30 deg", "x":"7 mm", "y":"11 mm"
    }))
    .unwrap();
    assert_eq!(spec.kind, GearKind::Rack);
    assert_eq!(spec.pitch_radius, 0.0);
    assert!(profile.regions[0].holes.is_empty());
    assert!(profile.regions[0].outer.signed_area().is_finite());
}

#[test]
fn hub_options_are_external_only_and_do_not_replace_other_profiles() {
    for kind in ["internal", "rack"] {
        let data = json!({"kind":kind, "outer_diameter":"60 mm", "boss_diameter":"6 mm"});
        assert!(internal_profile(data).unwrap_err().contains("平歯車で指定"));
    }
    let mut reg = Registry::new();
    register(&mut reg);
    for field in &reg.descriptor(GEAR).unwrap().fields {
        if [
            "lightening_count",
            "lightening_diameter",
            "lightening_circle",
            "boss_diameter",
            "keyway",
            "key_width",
            "key_depth",
        ]
        .contains(&field.key.as_str())
        {
            assert!(field.applies(&json!({"kind":"external"})), "{}", field.key);
            assert!(!field.applies(&json!({"kind":"internal"})), "{}", field.key);
            assert!(!field.applies(&json!({"kind":"rack"})), "{}", field.key);
        }
    }
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
        assert_eq!(profile.plane.origin, Plane::YZ.to_world([7.0, 11.0]));
        assert!((spec.phase - 30f64.to_radians()).abs() < 1e-12);
        for i in 0..3 {
            let expected = Plane::YZ.x_axis[i] * 30f64.to_radians().cos()
                + Plane::YZ.y_axis[i] * 30f64.to_radians().sin();
            assert!((profile.plane.x_axis[i] - expected).abs() < 1e-12);
        }
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
