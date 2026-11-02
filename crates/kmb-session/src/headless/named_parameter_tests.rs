//! 確かめること: 数の欄の名前付きの式が、パラメータと欄を1回で変更する。

use super::*;
use serde_json::json;

fn extrude() -> (Headless, FeatureId) {
    let mut work = Headless::new(kmb_bundle::modules());
    let sketch = work
        .add_feature(
            "cad.sketch@1",
            Some("輪郭"),
            json!({
                "entities":[
                    {"type":"point","id":1,"x":0.0,"y":0.0},
                    {"type":"point","id":2,"x":20.0,"y":0.0},
                    {"type":"point","id":3,"x":20.0,"y":10.0},
                    {"type":"point","id":4,"x":0.0,"y":10.0},
                    {"type":"line","id":5,"a":1,"b":2},
                    {"type":"line","id":6,"a":2,"b":3},
                    {"type":"line","id":7,"a":3,"b":4},
                    {"type":"line","id":8,"a":4,"b":1}
                ],"constraints":[]
            }),
        )
        .unwrap();
    let feature = work
        .add_feature(
            "solid.extrude@1",
            Some("板"),
            json!({
                "profile":sketch.0,"distance":"10 mm","operation":"new","direction":"normal"
            }),
        )
        .unwrap();
    assert_eq!(work.report().error_count(), 0);
    (work, feature)
}

fn assert_one_change_and_undo(work: &mut Headless, feature: FeatureId, before: usize) {
    assert_eq!(work.doc().undo_depth(), before + 1);
    assert_eq!(work.doc().data().param_by_name("w").unwrap().expr, "20 mm");
    assert_eq!(
        work.doc().data().feature(feature).unwrap().data["distance"],
        "w"
    );
    assert_eq!(work.report().error_count(), 0);
    work.undo().unwrap();
    assert!(work.doc().data().param_by_name("w").is_none());
    assert_eq!(
        work.doc().data().feature(feature).unwrap().data["distance"],
        "10 mm"
    );
    assert_eq!(work.report().error_count(), 0);
}

#[test]
fn named_parameter_set_value_adds_the_parameter_and_replaces_the_expression() {
    let (mut work, feature) = extrude();
    let before = work.doc().undo_depth();
    work.set_value(feature, "distance", "w=20").unwrap();
    assert_one_change_and_undo(&mut work, feature, before);
}

#[test]
fn named_parameter_feature_data_uses_the_same_atomic_change() {
    let (mut work, feature) = extrude();
    let before = work.doc().undo_depth();
    let mut data = work.doc().data().feature(feature).unwrap().data.clone();
    data["distance"] = json!("w=20");
    let tx = crate::edit::set_feature_data(work.doc().data(), feature, data);
    work.apply(tx).unwrap();
    assert_one_change_and_undo(&mut work, feature, before);
}

#[test]
fn named_parameter_rejections_leave_the_document_and_undo_unchanged() {
    let (mut work, feature) = extrude();
    work.add_param("existing", "4 mm").unwrap();
    let before = work.doc().data().clone();
    let undo = work.doc().undo_depth();
    for text in [
        "existing=20",
        "mm=20",
        "sin=20",
        "bad-name=20",
        "w=",
        "=20",
        "w=2=3",
        "w=missing+2",
        "w=20 deg",
    ] {
        assert!(work.set_value(feature, "distance", text).is_err(), "{text}");
        let mut data = work.doc().data().feature(feature).unwrap().data.clone();
        data["distance"] = json!(text);
        let tx = crate::edit::set_feature_data(work.doc().data(), feature, data);
        assert!(work.apply(tx).is_err(), "{text}");
        assert_eq!(work.doc().data(), &before, "{text}");
        assert_eq!(work.doc().undo_depth(), undo, "{text}");
    }
}

#[test]
fn named_parameter_assembly_fields_use_the_declared_angle_and_document_length_unit() {
    let (mut work, body) = extrude();
    let id = work
        .add_feature(
            "asm.position@1",
            Some("位置"),
            json!({
                "moving":{"kind":"body","root":body.0},"bodies":[body.0]
            }),
        )
        .unwrap();
    assert_eq!(work.report().error_count(), 0);
    let mut properties = work.doc().data().properties.clone();
    properties.units = kmb_core::LengthUnit::In;
    work.apply(Transaction::new(
        "単位",
        vec![kmb_core::Op::SetProperties {
            properties: Box::new(properties),
        }],
    ))
    .unwrap();
    for (key, typed, name, expr) in [
        ("x", "offset=0.5", "offset", "0.5 in"),
        ("rz", "turn=90", "turn", "90 deg"),
    ] {
        let before = work.doc().undo_depth();
        work.set_value(id, key, typed).unwrap();
        assert_eq!(work.doc().data().param_by_name(name).unwrap().expr, expr);
        assert_eq!(work.doc().data().feature(id).unwrap().data[key], name);
        assert_eq!(work.doc().undo_depth(), before + 1);
        assert_eq!(work.report().error_count(), 0);
        work.undo().unwrap();
        assert!(work.doc().data().param_by_name(name).is_none());
    }
}
