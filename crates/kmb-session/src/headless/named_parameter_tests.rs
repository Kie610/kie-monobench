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
