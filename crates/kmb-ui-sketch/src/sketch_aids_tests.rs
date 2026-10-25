//! スケッチの補助(角の丸い長方形、点を離して拘束、開いた端と「端をつなぐ」、スケッチの中の文字、
//! 面の縁の自動の投影)を、人の操作の順で確かめる。

use crate::Tool;
use crate::tests::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kmb_sketch::{ConstraintKind, Entity, SketchPlane};
use kmb_ui::Workspace;

fn count(s: &kmb_sketch::Sketch, f: impl Fn(&Entity) -> bool) -> usize {
    s.entities.iter().filter(|e| f(e)).count()
}

#[test]
fn a_rounded_rectangle_is_drawn_with_two_clicks_and_one_radius() {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::RoundedRectangle);
    h.run();
    click_at(&mut h, [1.19, 16.36]);
    click_at(&mut h, [11.62, 8.54]);
    let s = current_sketch(h.state());
    assert_eq!(count(&s, |e| matches!(e, Entity::Line { .. })), 4);
    assert_eq!(count(&s, |e| matches!(e, Entity::Arc { .. })), 4);
    let radius: Vec<&str> = s
        .constraints
        .iter()
        .filter_map(|c| match &c.kind {
            ConstraintKind::Radius { value, .. } => Some(value.as_str()),
            _ => None,
        })
        .collect();
    // 打たなければ短い辺の 1/5(7.82 mm の 1/5)
    assert_eq!(radius, vec!["1.56 mm"], "半径の寸法は 1 つ");
    let id = h.state().selected().unwrap();
    assert!(has_profile(h.state(), id), "閉じた輪郭");
    // 道具は続く
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::RoundedRectangle);
}

#[test]
fn typed_width_height_and_radius_become_dimensions() {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::RoundedRectangle);
    h.run();
    click_at(&mut h, [1.19, 16.36]);
    move_to(&mut h, [11.62, 8.54]);
    type_text(&mut h, "30, 20, 3");
    press_enter(&mut h);
    let s = current_sketch(h.state());
    let has = |f: &dyn Fn(&ConstraintKind) -> bool| s.constraints.iter().any(|c| f(&c.kind));
    assert!(has(
        &|k| matches!(k, ConstraintKind::Radius { value, .. } if value == "3 mm")
    ));
    assert!(has(
        &|k| matches!(k, ConstraintKind::HorizontalDistance { value, .. } if value == "30 mm")
    ));
    assert!(has(
        &|k| matches!(k, ConstraintKind::VerticalDistance { value, .. } if value == "20 mm")
    ));
    let sol = solved(h.state());
    assert_eq!(sol.dof, 2, "位置だけ残る");
}

/// 2 本の離れた線分(左の線の右端と、右の線の左端が離れている)を描いた空のスケッチ。
fn two_lines() -> Harness<'static, Workspace> {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Line);
    h.run();
    click_at(&mut h, [1.0, 12.0]);
    click_at(&mut h, [5.0, 12.0]);
    press_key(&mut h, egui::Key::Escape);
    editor(h.state_mut()).set_tool(Tool::Line);
    h.run();
    click_at(&mut h, [7.0, 12.0]);
    click_at(&mut h, [11.0, 15.0]);
    press_key(&mut h, egui::Key::Escape);
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::Select);
    let s = current_sketch(h.state());
    assert_eq!(
        count(&s, |e| matches!(e, Entity::Line { .. })),
        2,
        "{:?}",
        s.entities
    );
    h
}

#[test]
fn dragging_an_end_onto_another_end_makes_them_coincident() {
    let mut h = two_lines();
    let before = current_sketch(h.state()).constraints.len();
    drag_on(&mut h, [5.0, 12.0], [7.0, 12.0], egui::Modifiers::NONE);
    let s = current_sketch(h.state());
    assert_eq!(s.constraints.len(), before + 1);
    assert!(
        matches!(
            s.constraints.last().unwrap().kind,
            ConstraintKind::Coincident { .. }
        ),
        "{:?}",
        s.constraints.last()
    );
    // 2 本はつながった(開いた端は外側の 2 つだけ)
    let sol = solved(h.state());
    assert_eq!(kmb_sketch::loose_ends(&s, &sol).open.len(), 2);
}

#[test]
fn dragging_an_end_onto_a_line_puts_it_on_the_line_and_ctrl_does_not() {
    let mut h = two_lines();
    let before = current_sketch(h.state()).constraints.len();
    // 右の線の右上の端(水平の拘束は無い)を、左の線の途中へ。Ctrl を押して離すと付けない(動かすだけ)
    drag_on(&mut h, [11.0, 15.0], [3.0, 12.0], egui::Modifiers::COMMAND);
    assert_eq!(current_sketch(h.state()).constraints.len(), before);
    // 今度は Ctrl なしで線の途中で離す
    drag_on(&mut h, [3.0, 12.0], [2.0, 12.0], egui::Modifiers::NONE);
    let s = current_sketch(h.state());
    assert!(
        matches!(
            s.constraints.last().unwrap().kind,
            ConstraintKind::PointOnLine { .. }
        ),
        "{:?}",
        s.constraints.last()
    );
}

#[test]
fn the_panel_counts_open_ends_and_ties_close_ones() {
    let mut h = two_lines();
    // 2 本の端を少しだけ離す(0.05 mm)
    let ws = h.state_mut();
    let id = ws.selected().unwrap();
    let mut s = current_sketch(ws);
    s.adopt_solution(&solved(ws));
    for e in &mut s.entities {
        if let Entity::Point { x, y, .. } = e
            && (*x - 7.0).abs() < 0.2
            && (*y - 12.0).abs() < 0.2
        {
            *x = 5.0 + 0.05;
            *y = 12.0;
        }
    }
    assert!(ws.set_feature_data(id, serde_json::to_value(&s).unwrap()));
    h.run();
    h.run();
    h.get_by_label_contains("開いた端 4");
    h.get_by_label("端をつなぐ(1 組)").click();
    h.run();
    h.run();
    let s = current_sketch(h.state());
    let sol = solved(h.state());
    assert_eq!(kmb_sketch::loose_ends(&s, &sol).open.len(), 2);
    assert!(
        h.query_by_label_contains("開いた端 2").is_some(),
        "数が減る"
    );
}

#[test]
fn the_text_tool_places_a_point_and_starts_text_on_it() {
    let mut h = empty_sketch();
    let sketch = h.state().selected().unwrap();
    editor(h.state_mut()).set_tool(Tool::Text);
    h.run();
    click_at(&mut h, [6.0, 12.0]);
    h.run();
    h.run();
    let ws = h.state();
    let data = &ws.document().data();
    let text = data
        .features
        .iter()
        .find(|f| f.type_id == "cad.text@1")
        .expect("文字ができる");
    assert_eq!(text.data["anchor"], serde_json::json!(sketch.0));
    let point = text.data["anchor_point"].as_u64().expect("置いた点が基準");
    let s = current_sketch_of(ws, sketch);
    assert!(matches!(
        s.entity(kmb_sketch::EntityId(point as u32)),
        Some(Entity::Point { .. })
    ));
}

/// 機能 `id` のスケッチ。
fn current_sketch_of(ws: &Workspace, id: kmb_core::FeatureId) -> kmb_sketch::Sketch {
    serde_json::from_value(ws.document().data().feature(id).unwrap().data.clone()).unwrap()
}

#[test]
fn a_sketch_on_a_face_projects_the_face_edges_as_a_profile() {
    let part = kmb_samples::reference_part();
    let plate = part.plate;
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .build_ui_state(
            |ui, ws: &mut Workspace| ws.ui(ui),
            ws_of(part.doc, None, reg()),
        );
    h.run();
    let ws = h.state_mut();
    ws.set_picked(Some(kmb_ui::Pick {
        feature: plate,
        face: format!("f{}:end", plate.0),
    }));
    ws.run_action(kmb_sketch::SKETCH_ON_FACE);
    h.run();
    h.run();
    let ws = h.state();
    let s = current_sketch(ws);
    assert!(matches!(s.plane, SketchPlane::Face { .. }));
    // 板の上面の縁: 外形の 4 辺と、真ん中の穴の縁
    let edges = s
        .projections
        .iter()
        .filter(|p| matches!(p.source, kmb_sketch::ProjectionSource::Edge { .. }))
        .count();
    assert_eq!(edges, 5, "面の縁を写す");
    assert!(
        s.projections
            .iter()
            .all(|p| !s.entity(p.entity).is_some_and(|e| e.is_construction())),
        "補助線にしない(押し出せる輪郭。Kie の決定)"
    );
    let id = ws.selected().unwrap();
    assert!(has_profile(ws, id), "面の形が輪郭になる");
    assert_eq!(ws.report().error_count(), 0);
    // 面の中に円を描くと、領域は面の縁で分かれる。領域を選ばない押し出しの既定は前と同じく円だけ
    editor(h.state_mut()).set_tool(Tool::Circle);
    h.run();
    click_at(&mut h, [20.0, 20.0]);
    click_at(&mut h, [25.0, 20.0]);
    let outputs = match h.state().report().feature(id) {
        Some(kmb_core::FeatureStatus::Ok { outputs, .. }) => outputs.clone(),
        other => panic!("{other:?}"),
    };
    let faces = <kmb_core::ProfileFaces as kmb_core::Payload>::from_value(&outputs["faces"]).unwrap();
    assert_eq!(faces.faces.len(), 3, "面 − 穴 − 円、穴、円");
    let profile = kmb_sketch::profile_value(&outputs["profile"]).unwrap();
    assert_eq!(profile.regions.len(), 1);
    assert!(
        profile.regions[0].holes.is_empty(),
        "既定は円だけ(面の縁は使わない)"
    );
}

#[test]
fn the_face_edges_are_not_projected_when_the_setting_is_off() {
    let part = kmb_samples::reference_part();
    let plate = part.plate;
    let mut ws = ws_of(part.doc, None, reg());
    ws.set_picked(Some(kmb_ui::Pick {
        feature: plate,
        face: format!("f{}:end", plate.0),
    }));
    // 設定の代わりに引数で切る(CLI・MCP と同じ口)
    let id = ws
        .run_action_args(
            kmb_sketch::SKETCH_ON_FACE,
            serde_json::json!({ "project_edges": false }),
        )
        .unwrap();
    let s = current_sketch_of(&ws, id);
    assert!(s.projections.is_empty());
}

/// 線分 1 本のスケッチ A(XY、(0,0)〜(20,0))と、その後の空のスケッチ B を持つ文書で、B を開いた作業場。
fn two_sketches(len: f64) -> (Harness<'static, Workspace>, kmb_core::FeatureId) {
    let mut a = kmb_sketch::Sketch::default();
    let (p, q) = (a.add_point(0.0, 0.0), a.add_point(len, 0.0));
    a.add_line(p, q);
    a.add_constraint(ConstraintKind::Fix { point: p });
    a.add_constraint(ConstraintKind::Fix { point: q });
    let mut v: serde_json::Value =
        serde_json::from_str(&kmb_core::Document::new().to_json()).unwrap();
    v["features"] = serde_json::json!([
        {"id": 1, "type_id": kmb_sketch::SKETCH, "name": "スケッチA", "data": serde_json::to_value(&a).unwrap()},
        {"id": 2, "type_id": kmb_sketch::SKETCH, "name": "スケッチB", "data": serde_json::to_value(kmb_sketch::Sketch::default()).unwrap()},
    ]);
    v["next_id"] = 3.into();
    let doc = kmb_core::Document::from_json(&v.to_string()).unwrap();
    let mut ws = ws_of(doc, None, reg());
    ws.open_edit(kmb_core::FeatureId(2));
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h.run();
    (h, kmb_core::FeatureId(1))
}

#[test]
fn a_line_projected_by_the_dimension_tool_follows_its_sketch_and_is_not_lost_when_hidden() {
    // 寸法の道具で前のスケッチの線を写すと参照寸法が付き、元の線を延ばすと写した線も付いて行く
    // 目を閉じたスケッチの線を写しても「元が見つからない投影」と出ない(2026-10-19 の写真撮りで
    // 見つけた不具合: 元はあるのに出た)
    let (mut h, a) = two_sketches(20.0);
    editor(h.state_mut()).set_tool(Tool::Dimension);
    h.run();
    // 前のスケッチの線を押し、上の空いた所に置いて Enter(写した線は決まっているので参照寸法)
    click_at(&mut h, [10.0, 0.0]);
    move_to(&mut h, [10.0, 4.0]);
    h.run();
    quick_click(&mut h, [10.0, 4.0]);
    press_enter(&mut h);
    let s = current_sketch(h.state());
    let from_a: Vec<_> = s
        .projections
        .iter()
        .filter(|p| p.source.sketch() == Some(a))
        .collect();
    assert_eq!(from_a.len(), 1, "{:?}", s.projections);
    assert!(!s.constraints.is_empty(), "寸法が付く");
    let lost = |h: &Harness<'static, Workspace>| {
        h.query_all_by_label_contains("元が見つからない投影")
            .count()
    };
    // 使った前のスケッチは自動で目を閉じるので、ここで既に隠れていることがある
    assert_eq!(lost(&h), 0);
    // 前のスケッチの線を 35 mm に延ばすと、写した線も付いて行く
    let mut sa: kmb_sketch::Sketch =
        serde_json::from_value(h.state().document().data().feature(a).unwrap().data.clone())
            .unwrap();
    for e in &mut sa.entities {
        if let Entity::Point { x, .. } = e
            && *x > 1.0
        {
            *x = 35.0;
        }
    }
    assert!(
        h.state_mut()
            .set_feature_data(a, serde_json::to_value(&sa).unwrap())
    );
    h.run();
    // 計算の結果(写した要素は計算の時に元から取り直す)
    let out = match h.state().report().feature(kmb_core::FeatureId(2)) {
        Some(kmb_core::FeatureStatus::Ok { outputs, .. }) => outputs["sketch"].clone(),
        other => panic!("{other:?}"),
    };
    let sol = kmb_sketch::solution_of(&out).unwrap();
    let xs: Vec<f64> = sol.points.values().map(|p| p[0]).collect();
    assert!(
        xs.iter().any(|x| (x - 35.0).abs() < 1e-6),
        "写した線の端が 35 mm へ: {xs:?}"
    );
    assert_eq!(lost(&h), 0);
    h.state_mut().set_hidden(a, true);
    h.run();
    h.run();
    assert_eq!(lost(&h), 0, "隠しても元はある");
}
