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
