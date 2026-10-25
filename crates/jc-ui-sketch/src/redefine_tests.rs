//! スケッチ平面を再定義を確かめる。

use crate::project_tests::later_sketch;
use crate::tests::*;
use egui_kittest::kittest::Queryable;
use jc_sketch::SketchPlane;

fn plane_of(ws: &jc_ui::Workspace, id: jc_core::FeatureId) -> SketchPlane {
    let e = ws.document().data().feature(id).unwrap();
    serde_json::from_value::<jc_sketch::Sketch>(e.data.clone())
        .unwrap()
        .plane
}

#[test]
fn the_panel_button_ends_the_sketch_and_asks_for_the_new_plane() {
    let mut h = later_sketch(|s, _| {
        let (a, b) = (s.add_point(0.0, 0.0), s.add_point(10.0, 0.0));
        s.add_line(a, b);
    });
    let id = editor(h.state_mut()).editor.feature.unwrap();
    h.get_by_label("平面を再定義…").click();
    h.run();
    h.run();
    assert!(!h.state().is_editing(id), "スケッチを終えて 3D のビューへ");
    assert_eq!(h.state().selected(), Some(id), "スケッチは選んだまま");
    // 案内は編集パネルの手順の一覧と 3D のビューの左上の案内の 2 か所に出る
    assert!(
        h.query_all_by_label_contains("スケッチを移す平面")
            .next()
            .is_some(),
        "移す先をクリックする案内"
    );
}

#[test]
fn a_sketch_moves_to_another_base_plane_and_keeps_its_lines() {
    let mut h = later_sketch(|s, _| {
        let (a, b) = (s.add_point(0.0, 0.0), s.add_point(10.0, 0.0));
        s.add_line(a, b);
    });
    let id = editor(h.state_mut()).editor.feature.unwrap();
    let ws = h.state_mut();
    ws.finish_edit();
    ws.select(id);
    ws.run_action_args(
        jc_sketch::REDEFINE_PLANE,
        serde_json::json!({ "plane": "yz" }),
    );
    assert_eq!(plane_of(ws, id), SketchPlane::Yz);
    assert_eq!(current_sketch(ws).entities.len(), 3, "線と端の点はそのまま");
    ws.undo();
    assert_eq!(plane_of(ws, id), SketchPlane::Xy, "取り消しで元の平面");
}

#[test]
fn a_sketch_moves_onto_an_earlier_face_but_not_onto_a_later_one() {
    let part = jc_samples::reference_part();
    let (plate, plate_sketch) = (part.plate, part.plate_sketch);
    let mut h = later_sketch(|s, _| {
        let (a, b) = (s.add_point(0.0, 0.0), s.add_point(10.0, 0.0));
        s.add_line(a, b);
    });
    let id = editor(h.state_mut()).editor.feature.unwrap();
    let ws = h.state_mut();
    ws.finish_edit();
    let top = jc_ui::Pick {
        feature: plate,
        face: format!("f{}:end", plate.0),
    };
    ws.select(id);
    ws.set_picked(Some(top.clone()));
    ws.run_action(jc_sketch::REDEFINE_PLANE);
    assert!(
        matches!(plane_of(ws, id), SketchPlane::Face { feature, .. } if feature == plate),
        "{:?}",
        plane_of(ws, id)
    );
    // 板の元のスケッチを、その板の面へは移せない(計算の順が逆になる)
    ws.select(plate_sketch);
    ws.set_picked(Some(top));
    ws.run_action(jc_sketch::REDEFINE_PLANE);
    assert_eq!(plane_of(ws, plate_sketch), SketchPlane::Xy);
    let m = ws.message().unwrap_or_default().to_string();
    assert!(m.contains("後にできた"), "{m}");
}
