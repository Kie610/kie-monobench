//! 確かめること: 3 点への穴は、共通の使用済みの処理で位置のスケッチを隠し、直す間だけ見せる。

use super::*;
use kmb_core::{FeatureStatus, SectionType};
use kmb_interchange::visibility::{self as vis, Visibility};
use serde_json::json;

fn three_hole_points() -> (Headless, FeatureId, FeatureId) {
    let mut work = Headless::new(kmb_bundle::modules());
    let sketch = work
        .add_feature(
            "cad.sketch@1",
            Some("板の輪郭"),
            json!({
                "entities": [
                    {"type":"point","id":1,"x":0.0,"y":0.0},
                    {"type":"point","id":2,"x":20.0,"y":0.0},
                    {"type":"point","id":3,"x":20.0,"y":20.0},
                    {"type":"point","id":4,"x":0.0,"y":20.0},
                    {"type":"line","id":5,"a":1,"b":2},
                    {"type":"line","id":6,"a":2,"b":3},
                    {"type":"line","id":7,"a":3,"b":4},
                    {"type":"line","id":8,"a":4,"b":1}
                ], "constraints":[]
            }),
        )
        .unwrap();
    let plate = work
        .add_feature(
            "solid.extrude@1",
            Some("板"),
            json!({
                "profile":sketch.0,"distance":"10 mm","operation":"new","direction":"normal"
            }),
        )
        .unwrap();
    let points = work
        .add_feature(
            "cad.sketch@1",
            Some("穴の位置"),
            json!({
                "offset":"10 mm", "entities":[
                    {"type":"point","id":1,"x":5.0,"y":5.0},
                    {"type":"point","id":2,"x":15.0,"y":5.0},
                    {"type":"point","id":3,"x":10.0,"y":15.0}
                ],"constraints":[]
            }),
        )
        .unwrap();
    assert!(
        work.visibility().hidden.contains(&sketch),
        "押し出しが使ったスケッチは隠れる"
    );
    assert!(
        !work.hidden_features().contains(&points),
        "穴をあける前は点が見える"
    );
    (work, plate, points)
}

#[test]
fn three_point_holes_hide_the_sketch_and_restore_its_eye_with_undo() {
    for explicit_points in [true, false] {
        let (mut work, plate, points) = three_hole_points();
        let before = work.doc().undo_depth();
        let hole = work
            .add_feature(
                "solid.hole@1",
                Some("3つの穴"),
                json!({
                    "target":plate.0,"sketch":points.0,
                    "points":if explicit_points {vec![1,2,3]} else {vec![]},
                    "diameter":"2 mm","depth":"5 mm"
                }),
            )
            .unwrap();
        assert!(
            matches!(work.report().feature(hole), Some(FeatureStatus::Ok { .. })),
            "{}",
            work.report_text()
        );
        let mass = work.mass().unwrap();
        assert_eq!(mass.len(), 1);
        let removed = 4000.0 - mass[0].props.as_ref().unwrap().volume;
        let expected = 15.0 * std::f64::consts::PI;
        // 体積の測りの誤差を許しつつ、2 点だけ開いた形とは区別する。
        assert!(
            (removed - expected).abs() < expected * 0.01,
            "3点すべてに穴があく: removed={removed}"
        );
        assert!(
            work.hidden_features().contains(&points),
            "穴が使った点のスケッチは隠れる"
        );
        assert_eq!(
            work.doc().undo_depth(),
            before + 1,
            "穴と目の変更は同じ取り消し単位"
        );
        work.undo().unwrap();
        assert!(work.doc().data().feature(hole).is_none());
        assert!(
            !work.hidden_features().contains(&points),
            "取り消すと点が見える"
        );
        work.redo().unwrap();
        assert!(work.hidden_features().contains(&points));

        let saved = work.doc().to_json();
        let reopened = Headless::with_document(
            kmb_bundle::modules(),
            Document::from_json(&saved).unwrap(),
            None,
        );
        assert!(
            reopened.hidden_features().contains(&points),
            "開き直しても隠れる"
        );
        let cx = reopened.context(None, None, &Json::Null);
        let eyes_before = reopened.doc().data().sections.get(Visibility::KEY).cloned();
        assert!(
            vis::shown_while_editing(&cx, hole).contains(&points),
            "穴を直す間は位置の点を見せる"
        );
        assert_eq!(
            reopened.doc().data().sections.get(Visibility::KEY),
            eyes_before.as_ref(),
            "一時的に見せても保存する目は変えない"
        );
    }
}
