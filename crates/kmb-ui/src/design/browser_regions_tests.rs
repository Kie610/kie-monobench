//! 確かめること: 輪郭を受ける間のブラウザの行は、穴を保って全部を足し外しする。
use crate::*;
use egui_kittest::{Harness, kittest::Queryable};
use kmb_core::{Document, FeatureEntry, Op, Transaction};

fn document() -> Document {
    let mut sketch = kmb_sketch::Sketch::default();
    sketch
        .add_rectangle((None, [0.0, 0.0]), (None, [20.0, 20.0]))
        .unwrap();
    let center = sketch.add_point(10.0, 10.0);
    sketch.add_circle(center, 3.0);
    sketch
        .add_rectangle((None, [30.0, 0.0]), (None, [40.0, 10.0]))
        .unwrap();
    let mut doc = Document::new();
    let id = doc.alloc_feature_id();
    doc.apply(Transaction::new(
        "輪郭の見本",
        vec![Op::InsertFeature {
            index: None,
            entry: FeatureEntry {
                id,
                type_id: kmb_sketch::SKETCH.into(),
                name: "輪郭の見本".into(),
                data: serde_json::to_value(sketch).unwrap(),
                suppressed: false,
                pin_bodies: false,
            },
        }],
    ))
    .unwrap();
    doc
}

fn harness() -> Harness<'static, Workspace> {
    let mut ws = Workspace::new(document(), None, kmb_bundle::modules());
    ws.clear_selection();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .with_step_dt(0.05)
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h
}

fn click_row(h: &mut Harness<'static, Workspace>) {
    h.query_all_by_label("輪郭の見本")
        .find(|n| n.rect().left() < 300.0 && n.rect().top() < 700.0)
        .expect("ブラウザのスケッチの行")
        .click();
    h.run();
}

#[test]
fn a_browser_sketch_row_toggles_all_regions_without_filling_its_hole() {
    let mut h = harness();
    h.get_by_label("押し出し").click();
    h.run();
    assert!(matches!(
        h.state()
            .cur
            .gesture
            .as_ref()
            .unwrap()
            .current()
            .unwrap()
            .gesture,
        kmb_core::Gesture::Regions
    ));
    click_row(&mut h);
    let g = h
        .state()
        .cur
        .gesture
        .as_ref()
        .expect("輪郭を選ぶ手順を終えない");
    assert!(!g.picks.is_empty(), "輪郭の全体を選ぶ");
    assert!(h.state().document().data().features.len() == 1);
    h.run_steps(12);
    click_row(&mut h);
    assert!(h.state().cur.gesture.as_ref().unwrap().picks.is_empty());
    h.run_steps(12);
    click_row(&mut h);
    h.key_press(egui::Key::Enter);
    h.run();
    h.event(egui::Event::Text("10".into()));
    h.key_press(egui::Key::Enter);
    h.run();
    h.state_mut().wait();
    assert_eq!(h.state().report().error_count(), 0);
    let bodies = h.state().visible_bodies();
    let volume: f64 = bodies
        .iter()
        .map(|(_, shape)| h.state().cur.work.kernel().mass(*shape).unwrap().volume)
        .sum();
    let expected = (500.0 - std::f64::consts::PI * 9.0) * 10.0;
    assert!(
        (volume - expected).abs() < expected * 1e-3,
        "{volume} != {expected}"
    );
}

#[test]
fn a_revolve_region_step_uses_the_same_row_toggle_even_for_two_quick_clicks() {
    let mut h = harness();
    h.get_by_label("回転").click();
    h.run();
    click_row(&mut h);
    assert!(!h.state().cur.gesture.as_ref().unwrap().picks.is_empty());
    click_row(&mut h);
    assert!(h.state().cur.gesture.as_ref().unwrap().picks.is_empty());
    assert!(h.state().cur.editing.is_none(), "スケッチの編集を開かない");
}

#[test]
fn a_body_field_does_not_take_profiles_from_a_sketch_row() {
    let mut doc = document();
    let sketch = doc.data().features[0].id;
    for distance in [10, 20] {
        let id = doc.alloc_feature_id();
        let data = kmb_cad_extrude::Extrude {
            profile: Some(sketch),
            distance: format!("{distance} mm"),
            operation: kmb_cad::Operation::New,
            ..Default::default()
        };
        doc.apply(Transaction::new(
            "見本のボディ",
            vec![Op::InsertFeature {
                index: None,
                entry: FeatureEntry {
                    id,
                    type_id: kmb_cad_extrude::EXTRUDE.into(),
                    name: format!("ボディ{distance}"),
                    data: serde_json::to_value(data).unwrap(),
                    suppressed: false,
                    pin_bodies: false,
                },
            }],
        ))
        .unwrap();
    }
    let mut ws = Workspace::new(doc, None, kmb_bundle::modules());
    ws.clear_selection();
    assert_eq!(ws.visible_bodies().len(), 2);
    let id = ws.add_described(kmb_cad_boolean::BOOLEAN).unwrap();
    let before = ws.document().to_json();
    assert!(ws.is_pick_field(id, "target"));
    assert!(!ws.pick_field_profile_row(sketch));
    assert_eq!(ws.document().to_json(), before);
    assert!(ws.is_pick_field(id, "target"));
}

#[test]
fn editing_an_extrude_keeps_the_browser_and_its_hidden_source_body_visible() {
    let part = kmb_samples::reference_part();
    let mut ws = Workspace::new(part.doc, None, kmb_bundle::modules());
    ws.set_body_hidden(part.plate, true);
    let visibility = serde_json::to_value(ws.cur.work.visibility()).unwrap();
    assert!(ws.cur.scene.is_empty());
    ws.open_edit(part.pocket);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    assert!(
        !h.state().cur.scene.is_empty(),
        "元のボディを編集の間だけ見せる"
    );
    assert!(
        h.query_all_by_label("ボディ1")
            .any(|n| n.rect().left() < 300.0)
    );
    let name = &h
        .state()
        .document()
        .data()
        .feature(part.plate_sketch)
        .unwrap()
        .name;
    assert!(
        h.query_all_by_label(name)
            .any(|n| n.rect().left() < 300.0 && n.rect().top() < 700.0)
    );
    assert_eq!(
        serde_json::to_value(h.state().cur.work.visibility()).unwrap(),
        visibility
    );
    h.state_mut().cancel_edit();
    h.run();
    assert!(h.state().cur.scene.is_empty());
    assert_eq!(
        serde_json::to_value(h.state().cur.work.visibility()).unwrap(),
        visibility
    );
}

#[test]
fn mcp_clicks_select_and_clear_a_browser_sketch_while_the_extrude_is_open() {
    let dir = std::env::temp_dir().join(format!(
        "browser-regions-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut ws = Workspace::new(document(), None, kmb_bundle::modules()).with_data_dir(dir.clone());
    ws.clear_selection();
    ws.layout_mut().ai_live = true;
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    let app = |h: &mut Harness<'static, Workspace>, tool: &str, args: serde_json::Value| {
        let (dir, tool) = (dir.clone(), tool.to_owned());
        let request = std::thread::spawn(move || kmb_session::live::call(&dir, &tool, args));
        let start = std::time::Instant::now();
        while !request.is_finished() {
            h.step();
            std::thread::sleep(std::time::Duration::from_millis(2));
            assert!(start.elapsed().as_secs() < 20, "画面の要求が時間切れ");
        }
        let result = request.join().unwrap().unwrap();
        assert_eq!(result["ok"], true, "{result}");
        h.run_steps(12);
    };
    app(&mut h, "ui_click", serde_json::json!({"target":"押し出し"}));
    // 同名のタイムラインの行の後に、ブラウザの行がある。
    app(
        &mut h,
        "ui_click",
        serde_json::json!({"target":"輪郭の見本#2"}),
    );
    assert!(!h.state().cur.gesture.as_ref().unwrap().picks.is_empty());
    app(
        &mut h,
        "ui_click",
        serde_json::json!({"target":"輪郭の見本#2"}),
    );
    assert!(h.state().cur.gesture.as_ref().unwrap().picks.is_empty());
    assert_eq!(h.state().document().data().features.len(), 1);
    h.state_mut().live_disconnect();
    drop(h);
    let _ = std::fs::remove_dir_all(dir);
}
