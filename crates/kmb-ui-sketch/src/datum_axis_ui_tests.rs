//! 確かめること: 投影の道具のクリックが共通の操作を通り、軸とのリンクを残す。
use crate::tests::*;
use egui_kittest::Harness;
use kmb_core::{Document, Edit, FeatureStatus, Payload};
use kmb_interchange::DatumAxis;
use kmb_sketch::{ProjectionSource, Sketch};
use kmb_ui::Workspace;
use serde_json::json;

fn script(h: &mut Harness<'static, Workspace>, text: &str) {
    let batch = h.state_mut().run_script(text).unwrap();
    let start = std::time::Instant::now();
    loop {
        h.step();
        if let Some(result) = h.state_mut().script_result(batch) {
            assert!(result.ok, "{}", result.report());
            break;
        }
        assert!(start.elapsed().as_secs() < 15, "台本の時間切れ");
    }
    h.run();
}

fn project_fixture(points: serde_json::Value, at: [f64; 2], has_point: bool) {
    let mut doc = Document::new();
    let mut initial = Sketch::default();
    if has_point {
        initial.add_point(0.0, 0.0);
    }
    let ids = doc
        .apply_edits(
            "見本",
            vec![
                Edit::Insert {
                    type_id: "cad.datum_axis@1".into(),
                    name: "基準の軸".into(),
                    data: json!({"points":points}),
                },
                Edit::Insert {
                    type_id: kmb_sketch::SKETCH.into(),
                    name: "投影先".into(),
                    data: serde_json::to_value(initial).unwrap(),
                },
            ],
        )
        .unwrap();
    let mut ws = ws_of(doc, None, reg());
    ws.open_edit(ids[1]);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    let FeatureStatus::Ok { outputs, .. } = h.state().report().feature(ids[0]).unwrap() else {
        panic!("作業軸を計算する");
    };
    assert!(
        outputs
            .get(kmb_interchange::DATUM_OUTPUT)
            .and_then(DatumAxis::from_value)
            .is_some()
    );
    script(
        &mut h,
        "click \"作成 ▾\"\nhover \"投影/取り込み\"\nclick \"投影\"",
    );
    click_at(&mut h, at);
    h.run();
    let data = &h.state().document().data().feature(ids[1]).unwrap().data;
    let saved: Sketch = serde_json::from_value(data.clone()).unwrap();
    assert!(
        saved
            .projections
            .iter()
            .any(|p| matches!(p.source,ProjectionSource::DatumAxis{feature} if feature==ids[0])),
        "{data}"
    );
    assert_eq!(h.state().report().error_count(), 0);
    if has_point {
        let Some(FeatureStatus::Ok { outputs, .. }) = h.state().report().feature(ids[1]) else {
            panic!("投影を計算する");
        };
        let sol = outputs
            .get("sketch")
            .and_then(kmb_sketch::solution_of)
            .unwrap();
        assert_eq!(
            saved.hit_test(&sol, 0.0, 0.0, 0.1).unwrap().entity,
            kmb_sketch::EntityId(1),
            "描く時は自分の点が先"
        );
    }
    script(&mut h, "key Esc");
    h.state_mut().undo();
    h.run();
    let saved: Sketch = serde_json::from_value(
        h.state()
            .document()
            .data()
            .feature(ids[1])
            .unwrap()
            .data
            .clone(),
    )
    .unwrap();
    assert!(saved.projections.is_empty());
}

#[test]
fn the_project_tool_uses_the_declared_datum_axis_action() {
    project_fixture(
        json!([{"kind":"at","at":[15,20,5]},{"kind":"at","at":[25,20,5]}]),
        [18.0, 20.0],
        false,
    );
}

#[test]
fn a_normal_axis_can_be_projected_at_the_sketch_origin() {
    project_fixture(
        json!([{"kind":"at","at":[0,0,5]},{"kind":"at","at":[0,0,15]}]),
        [0.0, 0.0],
        false,
    );
}

#[test]
fn a_normal_axis_can_be_projected_over_a_drawn_point() {
    project_fixture(
        json!([{"kind":"at","at":[0,0,5]},{"kind":"at","at":[0,0,15]}]),
        [0.0, 0.0],
        true,
    );
}
