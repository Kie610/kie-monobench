//! 確かめること: 操作「歯車」が輪郭とボディを 1 つの取り消し単位で足し、「歯車の輪郭」は輪郭だけを
//! 足す。中心は平面の座標になり、平面を選んでいなければ平面 → 中心の手順から始める。宣言に無い
//! 引数・作れない値は誤り。足した輪郭が計算でき、置いた中心に歯車がある。

use super::*;
use kmb_core::{
    Document, Edit, Extension, Extensions, FeatureStatus, Gesture, NullKernel, Outcome, Registry,
};
use serde_json::{Value, json};
use std::sync::Arc;

fn registry() -> Registry {
    let mut reg = Registry::new();
    register(&mut reg);
    body::register(&mut reg);
    reg
}

/// 空の文書で操作 `id` を引数 `args` で流した結果。
fn run(id: &str, args: Value) -> Result<Outcome, kmb_core::Problem> {
    let mut doc = Document::new();
    let reg = registry();
    let kernel: Arc<dyn kmb_core::Kernel> = Arc::new(NullKernel);
    let report = doc.recompute(&reg, kernel.as_ref());
    let mut cx = kmb_core::Context::new(doc.data(), &report, &reg, &kernel);
    cx.args = &args;
    Extensions(vec![Arc::new(GearCommands)]).run(id, &cx)
}

fn inserts(o: Outcome) -> Vec<(String, Value)> {
    let Outcome::Change { edits, .. } = o else {
        panic!("文書を変える");
    };
    edits
        .into_iter()
        .map(|e| match e {
            Edit::Insert { type_id, data, .. } => (type_id, data),
            other => panic!("足すだけ: {other:?}"),
        })
        .collect()
}

#[test]
fn the_gear_action_adds_an_outline_and_its_body_together() {
    let out = run(
        GEAR_ADD,
        json!({"plane": "xy", "point": [10.0, 5.0, 0.0], "teeth": 24, "module": "2 mm", "width": 6.0}),
    )
    .unwrap();
    let added = inserts(out);
    assert_eq!(added.len(), 2);
    assert_eq!(added[0].0, GEAR);
    let g = &added[0].1;
    assert_eq!(g["plane"], json!({"kind": "origin_xy"}));
    assert_eq!(
        (g["x"].as_str(), g["y"].as_str()),
        (Some("10 mm"), Some("5 mm"))
    );
    assert_eq!(
        (g["teeth"].as_str(), g["module"].as_str()),
        (Some("24"), Some("2 mm"))
    );
    assert!(g.get("kind").is_none() && g.get("bore").is_none(), "{g}");
    assert_eq!(added[1].0, GEAR_BODY);
    assert_eq!(added[1].1, json!({"gear": {"$new": 0}, "width": "6 mm"}));
    // 輪郭だけの操作はボディを足さず、歯の幅を受け取らない
    let only = inserts(run(GEAR_OUTLINE_ADD, json!({"plane": "xy"})).unwrap());
    assert_eq!(only.len(), 1);
    assert!(run(GEAR_OUTLINE_ADD, json!({"plane": "xy", "width": 6.0})).is_err());
}

#[test]
fn the_center_is_taken_in_the_coordinates_of_the_chosen_plane() {
    let added = inserts(
        run(
            GEAR_OUTLINE_ADD,
            json!({"plane": "yz", "point": [0.0, 3.0, 4.0]}),
        )
        .unwrap(),
    );
    let [a, b] = kmb_core::Plane::YZ.to_local([0.0, 3.0, 4.0]);
    let g = &added[0].1;
    assert_eq!(g["x"], json!(format!("{a} mm")));
    assert_eq!(g["y"], json!(format!("{b} mm")));
}

#[test]
fn values_the_gear_cannot_take_are_refused() {
    for bad in [
        json!({"plane": "xy", "teeth": 2}),
        json!({"plane": "xy", "module": 0.0}),
        json!({"plane": "xy", "width": -1.0}),
        json!({"plane": "xy", "kind": "internal"}),
        json!({"plane": "xy", "tooth": 20}),
        json!({"plane": "nowhere"}),
    ] {
        assert!(run(GEAR_ADD, bad.clone()).is_err(), "{bad}");
    }
}

#[test]
fn without_a_plane_the_steps_start_from_the_plane() {
    let mut doc = Document::new();
    let reg = registry();
    let kernel: Arc<dyn kmb_core::Kernel> = Arc::new(NullKernel);
    let report = doc.recompute(&reg, kernel.as_ref());
    let mut cx = kmb_core::Context::new(doc.data(), &report, &reg, &kernel);
    let ext = GearCommands;
    let hs = ext.handles(GEAR_ADD, &cx).unwrap();
    assert_eq!(hs.len(), 2);
    assert!(matches!(hs[0].gesture, Gesture::Choice) && hs[0].faces);
    assert_eq!(hs[0].choices.len(), 3, "原点の 3 つの平面");
    // 平面を決めると、中心はその平面の上の点
    let args = json!({"plane": "zx"});
    cx.args = &args;
    let hs = ext.handles(GEAR_ADD, &cx).unwrap();
    let Gesture::Point { normal, .. } = hs[1].gesture else {
        panic!("中心は平面の上の点");
    };
    assert_eq!(normal, kmb_core::Plane::ZX.normal());
    assert!(ext.handles("cad.other", &cx).is_none());
}

#[test]
fn the_added_outline_evaluates_around_the_given_center() {
    let added = inserts(
        run(
            GEAR_OUTLINE_ADD,
            json!({"plane": "xy", "point": [30.0, 0.0, 0.0], "teeth": 12}),
        )
        .unwrap(),
    );
    let mut doc = Document::new();
    let reg = registry();
    let (type_id, data) = added.into_iter().next().unwrap();
    let id = doc
        .apply_edits(
            "歯車",
            vec![Edit::Insert {
                type_id,
                name: "歯車1".into(),
                data,
            }],
        )
        .unwrap()[0];
    let report = doc.recompute(&reg, &NullKernel);
    assert_eq!(report.error_count(), 0, "{report:?}");
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(id) else {
        panic!("輪郭を計算できる");
    };
    let spec =
        kmb_interchange::GearSpec::from_value(&outputs[kmb_interchange::GEAR_OUTPUT]).unwrap();
    assert_eq!(spec.teeth, 12);
    assert!((spec.axis.origin[0] - 30.0).abs() < 1e-9, "{:?}", spec.axis);
}

#[test]
fn sprocket_actions_preserve_manual_dimensions_and_leave_default_width_automatic() {
    let added = inserts(run(GEAR_ADD, json!({"kind":"sprocket","chain":"manual","chain_pitch":12.71234567,"roller_diameter":7.9,"chain_inner_width":8.1234567,"teeth":17})).unwrap());
    assert_eq!(added.len(), 2);
    assert_eq!(added[0].1["chain_pitch"], json!("12.71234567 mm"));
    assert!(added[1].1.get("width").is_none());
    let external = inserts(run(GEAR_ADD, json!({})).unwrap());
    assert!(external[1].1.get("width").is_none());
    let explicit = inserts(
        run(
            GEAR_ADD,
            json!({"kind":"sprocket","chain":"35","teeth":17,"width":3.5}),
        )
        .unwrap(),
    );
    assert_eq!(explicit[1].1["width"], json!("3.5 mm"));
    for args in [
        json!({"kind":"sprocket","chain":"manual"}),
        json!({"kind":"sprocket","chain":"manual","chain_pitch":12.7,"roller_diameter":13.0,"chain_inner_width":8.0}),
        json!({"kind":"sprocket","bore":-1.0}),
        json!({"kind":"sprocket","chain_pitch":12.7}),
    ] {
        assert!(run(GEAR_ADD, args.clone()).is_err(), "{args}");
    }
}

#[test]
fn holes_boss_and_keyway_arguments_reach_the_same_feature_fields() {
    let out = inserts(
        run(
            GEAR_ADD,
            json!({
                "plane": "xy", "module": 2, "teeth": 24, "bore": 8,
                "lightening_count": 4, "lightening_diameter": 3,
                "lightening_circle": 26, "boss_diameter": 16,
                "keyway": true, "key_width": 2, "key_depth": 1,
                "web": 3, "boss": 12, "alignment": "start"
            }),
        )
        .unwrap(),
    );
    assert_eq!(out[0].1["lightening_count"], "4");
    for (key, want) in [
        ("lightening_diameter", "3 mm"),
        ("lightening_circle", "26 mm"),
        ("boss_diameter", "16 mm"),
        ("key_width", "2 mm"),
        ("key_depth", "1 mm"),
    ] {
        assert_eq!(out[0].1[key], want);
    }
    assert_eq!(out[0].1["keyway"], true);
    assert_eq!(out[1].1["web"], "3 mm");
    assert_eq!(out[1].1["boss"], "12 mm");
    assert_eq!(out[1].1["alignment"], "start");
    assert!(run(GEAR_ADD, json!({"lightening_count": -1})).is_err());
    assert!(run(GEAR_OUTLINE_ADD, json!({"web": 3})).is_err());
}

#[test]
fn a_rack_keeps_its_length_start_direction_and_actual_tooth_count() {
    let length = 12.75 * std::f64::consts::PI * 2.0;
    let added = inserts(
        run(
            GEAR_OUTLINE_ADD,
            json!({
                "kind":"rack", "module":2.0, "rack_size":"length", "rack_length":length,
                "rack_back":5.0,"phase":90.0,"point":[10.0,20.0,0.0]
            }),
        )
        .unwrap(),
    );
    let mut doc = Document::new();
    let reg = registry();
    let (type_id, data) = added.into_iter().next().unwrap();
    let id = doc
        .apply_edits(
            "ラック",
            vec![Edit::Insert {
                type_id,
                name: "ラック".into(),
                data,
            }],
        )
        .unwrap()[0];
    let report = doc.recompute(&reg, &NullKernel);
    assert_eq!(report.error_count(), 0, "{report:?}");
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(id) else {
        panic!("輪郭")
    };
    let spec =
        kmb_interchange::GearSpec::from_value(&outputs[kmb_interchange::GEAR_OUTPUT]).unwrap();
    assert_eq!(spec.kind, GearKind::Rack);
    assert_eq!(spec.teeth, 12);
    assert_eq!(spec.pitch_radius, 0.0);
    assert_eq!(spec.axis.origin, [10.0, 20.0, 0.0]);
    assert_eq!(spec.axis.reference, [1.0, 0.0, 0.0]);
    assert!((spec.phase - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    let reference = kmb_core::Curves::from_value(&outputs[crate::REFERENCE_OUTPUT]).unwrap();
    assert_eq!(reference.segments.len(), 1);
    let kmb_core::Segment::Line { a, b } = reference.segments[0] else {
        panic!("ピッチ線")
    };
    assert!((a[0] - 10.0).abs() < 1e-12 && (a[1] - 20.0).abs() < 1e-12);
    assert!((b[0] - 10.0).abs() < 1e-10 && (b[1] - 20.0 - length).abs() < 1e-10);
}

#[test]
fn rack_actions_accept_one_tooth_and_refuse_incomplete_or_conflicting_data() {
    assert!(run(GEAR_OUTLINE_ADD, json!({"kind":"rack","teeth":1})).is_ok());
    for args in [
        json!({"kind":"rack","rack_size":"length"}),
        json!({"kind":"rack","rack_size":"length","rack_length":1.0}),
        json!({"kind":"rack","rack_back":0.0}),
        json!({"kind":"rack","bore":2.0}),
        json!({"rack_back":2.0}),
    ] {
        assert!(run(GEAR_OUTLINE_ADD, args.clone()).is_err(), "{args}");
    }
}

#[test]
fn a_racks_module_and_pitch_boundary_keep_the_input_precision() {
    for module in [0.9999999, 0.0001, 2.3456789] {
        let length = 12.0 * std::f64::consts::PI * module;
        let added = inserts(
            run(
                GEAR_OUTLINE_ADD,
                json!({"kind":"rack","module":module,"rack_size":"length","rack_length":length}),
            )
            .unwrap(),
        );
        let mut doc = Document::new();
        let reg = registry();
        let (type_id, data) = added.into_iter().next().unwrap();
        let id = doc
            .apply_edits(
                "ラック",
                vec![Edit::Insert {
                    type_id,
                    name: "ラック".into(),
                    data,
                }],
            )
            .unwrap()[0];
        let report = doc.recompute(&reg, &NullKernel);
        assert_eq!(report.error_count(), 0, "{module}: {report:?}");
        let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(id) else {
            panic!("輪郭")
        };
        let spec =
            kmb_interchange::GearSpec::from_value(&outputs[kmb_interchange::GEAR_OUTPUT]).unwrap();
        assert_eq!(spec.teeth, 12, "{module}");
        assert_eq!(spec.module, module);
    }
}

#[test]
fn the_internal_gear_action_requires_a_ring_diameter_and_preserves_it() {
    let added = inserts(
        run(
            GEAR_OUTLINE_ADD,
            json!({"kind":"internal", "module":"2 mm", "teeth":20,"outer_diameter":"60 mm"}),
        )
        .unwrap(),
    );
    assert_eq!(added[0].1["kind"], "internal");
    assert_eq!(added[0].1["outer_diameter"], "60 mm");
    assert!(
        run(
            GEAR_OUTLINE_ADD,
            json!({"kind":"internal","module":"2 mm","teeth":20,"outer_diameter":"48 mm"})
        )
        .unwrap_err()
        .to_string()
        .contains("49.000 mm")
    );
    assert!(run(GEAR_OUTLINE_ADD, json!({"outer_diameter":"60 mm"})).is_err());
}

#[test]
fn an_internal_action_keeps_module_and_outer_diameter_precision_at_the_limit() {
    let module = 1.0000001;
    let minimum = 2.0 * (module * 20.0 / 2.0 + 1.25 * module) + 2.0 * module;
    let added = inserts(
        run(
            GEAR_OUTLINE_ADD,
            json!({"kind":"internal","module":module,"teeth":20,"outer_diameter":minimum}),
        )
        .unwrap(),
    );
    let mut doc = Document::new();
    let (type_id, data) = added.into_iter().next().unwrap();
    doc.apply_edits(
        "内歯車",
        vec![Edit::Insert {
            type_id,
            name: "内歯車".into(),
            data,
        }],
    )
    .unwrap();
    let report = doc.recompute(&registry(), &NullKernel);
    assert_eq!(report.error_count(), 0, "{report:?}");
    assert!(
        run(
            GEAR_OUTLINE_ADD,
            json!({"kind":"internal","outer_diameter":30.0,"bore":-1.0})
        )
        .is_err()
    );
}

#[test]
fn helical_actions_keep_normal_values_and_herringbone_body_fields() {
    let added = inserts(
        run(
            GEAR_ADD,
            json!({"module":2.0,"helix_angle":25.0,"hand":"left","herringbone":true,"groove":1.0}),
        )
        .unwrap(),
    );
    assert_eq!(added[0].1["module"], "2 mm");
    assert_eq!(added[0].1["helix_angle"], "25 deg");
    assert_eq!(added[0].1["hand"], "left");
    assert_eq!(added[1].1["herringbone"], true);
    assert_eq!(added[1].1["groove"], "1 mm");
    for args in [
        json!({"helix_angle":90}),
        json!({"helix_angle":-5}),
        json!({"herringbone":true}),
        json!({"helix_angle":20,"groove":1}),
    ] {
        assert!(run(GEAR_ADD, args.clone()).is_err(), "{args}");
    }
}

#[test]
fn helical_internal_actions_check_the_transverse_ring_limit_and_keep_precision() {
    let module = 1.0000001;
    let beta = 25.0f64;
    let mt = module / beta.to_radians().cos();
    let minimum = 2.0 * (mt * 20.0 / 2.0 + 1.25 * mt) + 2.0 * mt;
    let added = inserts(
        run(
            GEAR_OUTLINE_ADD,
            json!({
                "kind":"internal", "module":module, "teeth":20,
                "helix_angle":beta, "outer_diameter":minimum
            }),
        )
        .unwrap(),
    );
    let mut doc = Document::new();
    let (type_id, data) = added.into_iter().next().unwrap();
    let id = doc
        .apply_edits(
            "内歯",
            vec![Edit::Insert {
                type_id,
                name: "内歯".into(),
                data,
            }],
        )
        .unwrap()[0];
    let report = doc.recompute(&registry(), &NullKernel);
    assert_eq!(report.error_count(), 0, "{report:?}");
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(id) else {
        panic!("輪郭")
    };
    let spec = kmb_interchange::GearSpec::from_value(&outputs["gear"]).unwrap();
    assert_eq!(spec.module, module);
    for args in [
        json!({"kind":"rack","helix_angle":20}),
        json!({"kind":"rack","hand":"left"}),
    ] {
        assert!(run(GEAR_OUTLINE_ADD, args.clone()).is_err(), "{args}");
    }
}

#[test]
fn helical_body_actions_keep_main_part_dimensions_and_automatic_width() {
    let added = inserts(run(GEAR_ADD,json!({"helix_angle":25.0,"herringbone":true,"groove":1.0,"web":10.0,"boss":10.0,"alignment":"start"})).unwrap());
    let body = &added[1].1;
    assert!(body.get("width").is_none());
    assert_eq!(body["web"], "10 mm");
    assert_eq!(body["boss"], "10 mm");
    assert_eq!(body["alignment"], "start");
    assert_eq!(body["herringbone"], true);
    assert_eq!(body["groove"], "1 mm");
    for args in [
        json!({"kind":"sprocket","helix_angle":20}),
        json!({"kind":"sprocket","hand":"left"}),
    ] {
        assert!(run(GEAR_ADD, args).is_err());
    }
}

#[test]
fn pulley_actions_preserve_section_values_and_reject_fields_for_other_kinds() {
    let a=inserts(run(GEAR_ADD,json!({"kind":"v_pulley","pulley_diameter":150.1234567,"pulley_width":60.0,"groove_count":3,"v_section":"B","groove_angle":36.0,"groove_top_width":20.0,"groove_depth":12.0})).unwrap());
    assert_eq!(a.len(), 2);
    assert_eq!(a[0].1["pulley_diameter"], json!("150.1234567 mm"));
    assert_eq!(a[0].1["groove_count"], json!("3"));
    assert_eq!(a[0].1["groove_angle"], json!("36 deg"));
    for args in [
        json!({"pulley_diameter":100.0}),
        json!({"kind":"flat_pulley","groove_count":2}),
        json!({"kind":"rope_sheave","rope_diameter":0.0}),
        json!({"kind":"v_pulley","groove_count":-1}),
    ] {
        assert!(run(GEAR_ADD, args.clone()).is_err(), "{args}");
    }
}

/// 確かめること: CLIの未対応組合せはUnavailableで、変更の提案も文書の変更も返さない。
#[test]
fn unsupported_helical_combinations_return_unavailable_without_mutation() {
    let mut doc = Document::new();
    let reg = registry();
    let kernel: Arc<dyn kmb_core::Kernel> = Arc::new(NullKernel);
    let report = doc.recompute(&reg, kernel.as_ref());
    let before = serde_json::to_value(doc.data()).unwrap();
    for args in [
        json!({"helix_angle":25,"width":8,"web":3,"boss":12}),
        json!({"helix_angle":25,"herringbone":true,"groove":1,"bore":8,"keyway":true}),
        json!({"helix_angle":25,"herringbone":true,"groove":1,"lightening_count":4,"lightening_diameter":3,"lightening_circle":26}),
        json!({"kind":"flat_pulley","helix_angle":20}),
    ] {
        let mut cx = kmb_core::Context::new(doc.data(), &report, &reg, &kernel);
        cx.args = &args;
        let error = Extensions(vec![Arc::new(GearCommands)])
            .run(GEAR_ADD, &cx)
            .unwrap_err();
        assert_eq!(
            error.kind,
            kmb_core::ProblemKind::Unavailable,
            "{args}: {error}"
        );
        assert!(error.message.contains(" / "), "日本語と英語の理由");
        assert_eq!(serde_json::to_value(doc.data()).unwrap(), before);
    }
}
