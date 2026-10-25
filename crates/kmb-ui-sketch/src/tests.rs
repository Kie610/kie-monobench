use super::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kmb_core::{Document, FeatureId, FeatureStatus};
use kmb_samples::sample_document;
use kmb_sketch::ConstraintKind;
use kmb_ui::Workspace;

/// スケッチの編集器を組み込んだ作業場。
pub(crate) fn ws_of(
    doc: Document,
    path: Option<std::path::PathBuf>,
    modules: kmb_core::Modules,
) -> Workspace {
    Workspace::new(doc, path, modules).with_editor(SketchEditor::new())
}

pub(crate) fn reg() -> kmb_core::Modules {
    kmb_bundle::modules()
}

pub(crate) fn editor(ws: &mut Workspace) -> &mut SketchEditor {
    ws.editor_mut::<SketchEditor>().expect("組み込んである")
}

/// スケッチに閉じた輪郭があるか。
pub(crate) fn has_profile(ws: &Workspace, id: FeatureId) -> bool {
    match ws.report().feature(id) {
        Some(FeatureStatus::Ok { outputs, .. }) => outputs
            .get("profile")
            .and_then(kmb_sketch::profile_value)
            .is_some_and(|p| !p.regions.is_empty()),
        _ => false,
    }
}

/// 見本のスケッチを開いて直している作業場(選んだだけでは直さないので、編集で開く)。
fn harness() -> Harness<'static, Workspace> {
    let mut ws = ws_of(sample_document(), None, reg());
    let id = ws.document().data().features[0].id;
    ws.open_edit(id);
    Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws)
}

#[test]
fn sample_is_well_constrained_and_shown() {
    let mut h = harness();
    h.run();
    h.get_by_label("完全拘束");
    // ブラウザとタイムラインの両方に出る
    assert!(h.get_all_by_label_contains("板の輪郭").count() >= 2);
    assert_eq!(h.state().report().error_count(), 0);
}

#[test]
fn editing_a_parameter_updates_the_sketch_and_can_be_undone() {
    let mut h = harness();
    h.run();
    let id = h
        .state()
        .document()
        .data()
        .param_by_name("width")
        .unwrap()
        .id;
    assert!(h.state_mut().set_param_expr(id, "80 mm"));
    h.run();
    let point = |ws: &Workspace| match ws.report().features[0].status {
        FeatureStatus::Ok { ref outputs, .. } => {
            kmb_sketch::solution_of(&outputs["sketch"]).unwrap().points[&kmb_sketch::EntityId(5)]
        }
        _ => panic!("スケッチが解けていません"),
    };
    assert!((point(h.state())[0] - 40.0).abs() < 1e-6);
    h.get_by_label("元に戻す").click();
    h.run();
    assert!((point(h.state())[0] - 30.0).abs() < 1e-6);
}

#[test]
fn removing_a_dimension_shows_remaining_freedom() {
    let mut h = harness();
    let ws = h.state_mut();
    let f = ws.document().data().features[0].clone();
    let mut sketch: kmb_sketch::Sketch = serde_json::from_value(f.data).unwrap();
    sketch
        .constraints
        .retain(|c| c.id != kmb_sketch::ConstraintId(10));
    let data = serde_json::to_value(sketch).unwrap();
    assert!(ws.set_feature_data(f.id, data));
    h.run();
    h.get_by_label("拘束不足(自由度 1)");
}

#[test]
fn renaming_keeps_the_sketch_solved() {
    let mut h = harness();
    let id = h
        .state()
        .document()
        .data()
        .param_by_name("width")
        .unwrap()
        .id;
    assert!(h.state_mut().rename_param(id, "w"));
    h.run();
    assert_eq!(h.state().report().error_count(), 0);
    h.get_by_label("完全拘束");
}

/// スケッチの座標(mm)の点の、画面の位置(前のフレームの変換で。画面の配置が変わっても同じ点を指す)。
pub(crate) fn screen(h: &Harness<'static, Workspace>, p: [f64; 2]) -> egui::Pos2 {
    let ed = h.state().editor::<SketchEditor>().expect("組み込んである");
    ed.view
        .last
        .as_ref()
        .expect("作図面を描いた後")
        .to_screen(p)
}

/// スケッチの座標の点へカーソルを動かす(フレームは進めない)。
pub(crate) fn move_to(h: &mut Harness<'static, Workspace>, p: [f64; 2]) {
    let pos = screen(h, p);
    h.hover_at(pos);
}

/// スケッチの座標の点をクリックする。
pub(crate) fn click_at(h: &mut Harness<'static, Workspace>, p: [f64; 2]) {
    let pos = screen(h, p);
    click_px(h, pos);
}

/// 画面の位置をクリックする(札や印など、画面の矩形から出した位置)。
pub(crate) fn click_px(h: &mut Harness<'static, Workspace>, pos: egui::Pos2) {
    h.hover_at(pos);
    h.run();
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.run();
    }
}

pub(crate) fn current_sketch(ws: &Workspace) -> kmb_sketch::Sketch {
    let id = ws.selected().unwrap();
    serde_json::from_value(ws.document().data().feature(id).unwrap().data.clone()).unwrap()
}

#[test]
fn drawing_a_closed_rectangle_with_the_line_tool() {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .build_ui_state(
            |ui, ws: &mut Workspace| ws.ui(ui),
            ws_of(Document::new(), None, reg()),
        );
    h.run();
    h.state_mut().run_action(kmb_sketch::ADD_SKETCH);
    editor(h.state_mut()).set_tool(Tool::Line);
    h.run();
    for p in [
        [6.4, 8.54],
        [16.83, 8.54],
        [16.83, 16.36],
        [6.4, 16.36],
        [6.4, 8.54],
    ] {
        click_at(&mut h, p);
    }
    let s = current_sketch(h.state());
    let lines = s
        .entities
        .iter()
        .filter(|e| matches!(e, kmb_sketch::Entity::Line { .. }))
        .count();
    let points = s
        .entities
        .iter()
        .filter(|e| matches!(e, kmb_sketch::Entity::Point { .. }))
        .count();
    assert_eq!(
        (lines, points),
        (4, 4),
        "閉じた長方形になっていません: {s:?}"
    );
    let hv = s
        .constraints
        .iter()
        .filter(|c| {
            matches!(
                c.kind,
                kmb_sketch::ConstraintKind::Horizontal { .. }
                    | kmb_sketch::ConstraintKind::Vertical { .. }
            )
        })
        .count();
    assert_eq!(hv, 4, "水平・垂直の拘束が自動で付いていません");
    // 1 回の線ごとに取り消せる
    h.state_mut().undo();
    let s = current_sketch(h.state());
    assert_eq!(
        s.entities
            .iter()
            .filter(|e| matches!(e, kmb_sketch::Entity::Line { .. }))
            .count(),
        3
    );
}

#[test]
fn selecting_a_line_offers_a_length_dimension() {
    let mut h = harness();
    h.run();
    editor(h.state_mut())
        .editor
        .selection
        .insert(kmb_sketch::EntityId(11));
    h.run();
    h.get_by_label("水平/垂直");
    // スケッチ中はパネルの下の欄が 2 段で高いので、窓の低い試験では巻物を送ってから押す
    h.get_by_label("長さ").scroll_to_me();
    h.run();
    h.get_by_label("長さ").click();
    h.run();
    let s = current_sketch(h.state());
    // 下の辺には既に長さがあるので、2 つ目は参照寸法として置き、その旨を出す(網羅版 E9。
    // 過剰な拘束のまま足さない)
    assert_eq!(s.constraints.len(), 11);
    assert!(s.constraints.last().unwrap().reference);
    h.get_by_label("完全拘束");
    let note = editor(h.state_mut())
        .editor
        .notice
        .clone()
        .unwrap_or_default();
    assert!(note.contains("参照寸法"), "{note}");
}

#[test]
fn a_bare_number_for_a_dimension_takes_mm_and_keeps_the_sketch() {
    // 人の点検(2026-10-15): 寸法の値に「40」とだけ打つと、スケッチもボディも全部消えた
    // (形が決まっていない長方形の上の辺に付ける。決まっていれば参照寸法になり値は測った値)
    let mut h = sketch_with_rectangle();
    let n = current_sketch(h.state()).constraints.len();
    click_at(&mut h, [6.4, 16.36]);
    editor(h.state_mut()).editor.dim_text = "40".into();
    h.run();
    h.get_by_label("長さ").click();
    h.run();
    let s = current_sketch(h.state());
    assert_eq!(s.constraints.len(), n + 1);
    assert_eq!(s.constraints.last().unwrap().kind.value(), Some("40 mm"));
    let err = format!("{:?}", h.state().report());
    assert!(!err.contains("単位が必要"), "{err}");
}

#[test]
fn a_dimension_that_cannot_be_computed_is_refused_not_placed() {
    // 計算できない値(知らない名前)を置くと、スケッチと後ろの形が全部消えるので、置かずに断る
    let mut h = sketch_with_rectangle();
    let n = current_sketch(h.state()).constraints.len();
    click_at(&mut h, [6.4, 16.36]);
    editor(h.state_mut()).editor.dim_text = "nosuchname".into();
    h.run();
    h.get_by_label("長さ").click();
    h.run();
    h.run();
    assert_eq!(current_sketch(h.state()).constraints.len(), n);
    assert!(editor(h.state_mut()).editor.dim_error.is_some());
    h.get_by_label_contains("この値では置けません");
    assert_eq!(h.state().report().error_count(), 0);
}

#[test]
fn the_dimension_tool_measures_and_places_a_reference_when_already_determined() {
    let mut h = harness();
    h.run();
    // D で寸法の道具。長さが既にある下の辺を選ぶ
    h.key_press(egui::Key::D);
    h.run();
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::Dimension);
    editor(h.state_mut())
        .editor
        .selection
        .insert(kmb_sketch::EntityId(11));
    h.run();
    h.run();
    assert_eq!(editor(h.state_mut()).editor.dim_text, "60 mm");
    h.get_by_label_contains("既に決まっているので、参照寸法(60 mm)として置きます");
    h.key_press(egui::Key::Enter);
    h.run();
    let s = current_sketch(h.state());
    assert_eq!(s.constraints.len(), 11);
    let c = s.constraints.last().unwrap();
    assert!(c.reference, "{c:?}");
    // 参照寸法は形を決めないので、過剰な拘束にならない(図には括弧つきで出る)
    h.get_by_label("完全拘束");
    // 駆動へ戻すのは、余分なので断る
    let id = c.id;
    // 辺を選び直すと、それを決めている拘束の一覧に出る(全部の一覧は畳んだまま)
    h.get_by_label("拘束(11)");
    editor(h.state_mut())
        .editor
        .selection
        .insert(kmb_sketch::EntityId(11));
    h.run();
    h.get_by_label("駆動にする").click();
    h.run();
    h.get_by_label_contains(&format!("{id} は"));
    assert!(
        current_sketch(h.state())
            .constraints
            .last()
            .unwrap()
            .reference
    );
}

#[test]
fn drawing_with_the_rectangle_point_arc_and_trim_tools() {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 1200.0))
        .build_ui_state(
            |ui, ws: &mut Workspace| ws.ui(ui),
            ws_of(Document::new(), None, reg()),
        );
    h.run();
    h.state_mut().run_action(kmb_sketch::ADD_SKETCH);
    editor(h.state_mut()).set_tool(Tool::Rectangle);
    h.run();
    click_at(&mut h, [7.6, 9.03]);
    click_at(&mut h, [14.55, 14.24]);
    let count = |s: &kmb_sketch::Sketch| {
        let n = |f: fn(&kmb_sketch::Entity) -> bool| s.entities.iter().filter(|e| f(e)).count();
        (
            n(|e| matches!(e, kmb_sketch::Entity::Line { .. })),
            n(|e| matches!(e, kmb_sketch::Entity::Point { .. })),
            n(|e| matches!(e, kmb_sketch::Entity::Arc { .. })),
        )
    };
    assert_eq!(count(&current_sketch(h.state())), (4, 4, 0));
    let id = h.state().selected().unwrap();
    assert!(has_profile(h.state(), id), "矩形は閉じた輪郭");
    editor(h.state_mut()).set_tool(Tool::Point);
    h.run();
    click_at(&mut h, [11.08, 11.46]);
    assert_eq!(count(&current_sketch(h.state())), (4, 5, 0));
    assert_eq!(current_sketch(h.state()).lone_points().len(), 1);
    // 円弧: 中心、始点、終点
    editor(h.state_mut()).set_tool(Tool::Arc);
    h.run();
    for p in [[11.08, 17.72], [12.82, 17.72], [11.08, 19.45]] {
        click_at(&mut h, p);
    }
    assert_eq!(count(&current_sketch(h.state())).2, 1);
    // トリム: 交わりのない円弧はまるごと消える
    editor(h.state_mut()).set_tool(Tool::Trim);
    h.run();
    click_at(&mut h, [12.29, 18.93]);
    assert_eq!(count(&current_sketch(h.state())), (4, 5, 0));
    // 接線円弧: 矩形の隅から続ける
    editor(h.state_mut()).set_tool(Tool::TangentArc);
    h.run();
    click_at(&mut h, [14.55, 14.24]);
    click_at(&mut h, [16.64, 16.33]);
    let s = current_sketch(h.state());
    assert_eq!(count(&s).2, 1);
    assert!(
        s.constraints
            .iter()
            .any(|c| matches!(c.kind, kmb_sketch::ConstraintKind::Tangent { .. }))
    );
    // 角の丸め: 矩形の 1 つ目の隅
    editor(h.state_mut()).set_tool(Tool::Fillet);
    h.run();
    click_at(&mut h, [7.6, 9.03]);
    let s = current_sketch(h.state());
    assert_eq!(count(&s).2, 2);
    assert!(s.constraints.iter().any(|c| matches!(
        &c.kind,
        kmb_sketch::ConstraintKind::Radius { value, .. } if value == "2 mm"
    )));
    // 接線円弧が隅から枝分かれしているので輪郭にはならないが、スケッチは解ける
    assert!(matches!(
        h.state().report().feature(id),
        Some(FeatureStatus::Ok { .. })
    ));
}

#[test]
fn a_length_and_a_diameter_can_be_typed_while_drawing() {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .build_ui_state(
            |ui, ws: &mut Workspace| ws.ui(ui),
            ws_of(Document::new(), None, reg()),
        );
    h.run();
    h.state_mut().run_action(kmb_sketch::ADD_SKETCH);
    editor(h.state_mut()).set_tool(Tool::Line);
    h.run();
    let type_and_enter = |h: &mut Harness<'static, Workspace>, text: &str| {
        h.event(egui::Event::Text(text.into()));
        h.run();
        for pressed in [true, false] {
            h.event(egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
            h.run();
        }
    };
    // 始点をクリックし、右へカーソルを向けて長さを打つ
    click_at(&mut h, [1.19, 11.15]);
    move_to(&mut h, [11.62, 11.15]);
    h.run();
    type_and_enter(&mut h, "25");
    let s = current_sketch(h.state());
    assert!(s.constraints.iter().any(|c| matches!(
        &c.kind,
        kmb_sketch::ConstraintKind::Length { value, .. } if value == "25 mm"
    )));
    assert!(
        s.constraints
            .iter()
            .any(|c| matches!(c.kind, kmb_sketch::ConstraintKind::Horizontal { .. }))
    );
    // 円: 中心をクリックして直径を打つ
    editor(h.state_mut()).set_tool(Tool::Circle);
    h.run();
    click_at(&mut h, [6.4, 18.97]);
    move_to(&mut h, [8.49, 18.97]);
    h.run();
    type_and_enter(&mut h, "10 mm");
    let s = current_sketch(h.state());
    let circle = s.constraints.iter().find_map(|c| match &c.kind {
        kmb_sketch::ConstraintKind::Diameter { circle, value } if value == "10 mm" => Some(*circle),
        _ => None,
    });
    let circle =
        circle.unwrap_or_else(|| panic!("直径の寸法 {:?} {:?}", s.constraints, s.entities));
    let id = h.state().selected().unwrap();
    let sol = match h.state().report().feature(id) {
        Some(FeatureStatus::Ok { outputs, .. }) => {
            kmb_sketch::solution_of(&outputs["sketch"]).unwrap()
        }
        other => panic!("{other:?}"),
    };
    assert!((sol.radii[&circle] - 5.0).abs() < 1e-6);
    // 矩形: 1 つ目の隅をクリックして「幅, 高さ」を打つ
    editor(h.state_mut()).set_tool(Tool::Rectangle);
    h.run();
    click_at(&mut h, [-1.42, 4.89]);
    move_to(&mut h, [1.19, 6.98]);
    h.run();
    type_and_enter(&mut h, "30, 20");
    let s = current_sketch(h.state());
    let lengths: Vec<&str> = s
        .constraints
        .iter()
        .filter_map(|c| match &c.kind {
            kmb_sketch::ConstraintKind::Length { value, .. } => Some(value.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(lengths, ["25 mm", "30 mm", "20 mm"]);
    assert!(has_profile(h.state(), id), "矩形は閉じた輪郭");
}

#[test]
fn a_slot_and_a_hexagon_are_drawn_in_the_sketch() {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .build_ui_state(
            |ui, ws: &mut Workspace| ws.ui(ui),
            ws_of(Document::new(), None, reg()),
        );
    h.run();
    h.state_mut().run_action(kmb_sketch::ADD_SKETCH);
    // 長穴: 2 つの中心をクリックして、幅を打つ
    editor(h.state_mut()).set_tool(Tool::Slot);
    h.run();
    click_at(&mut h, [-1.42, 11.15]);
    click_at(&mut h, [6.4, 11.15]);
    move_to(&mut h, [2.23, 9.58]);
    h.run();
    h.event(egui::Event::Text("8".into()));
    h.run();
    for pressed in [true, false] {
        h.event(egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        h.run();
    }
    let s = current_sketch(h.state());
    assert!(
        s.constraints.iter().any(|c| matches!(
            &c.kind,
            kmb_sketch::ConstraintKind::Diameter { value, .. } if value == "8 mm"
        )),
        "{:?}",
        s.constraints
    );
    // 多角形: 中心をクリックし、辺の数 5 を打って頂点をクリック
    editor(h.state_mut()).set_tool(Tool::Polygon);
    h.run();
    click_at(&mut h, [1.19, 18.97]);
    h.event(egui::Event::Text("5".into()));
    h.run();
    click_at(&mut h, [4.32, 18.97]);
    let s = current_sketch(h.state());
    let lines = s
        .entities
        .iter()
        .filter(|e| {
            matches!(
                e,
                kmb_sketch::Entity::Line {
                    construction: false,
                    ..
                }
            )
        })
        .count();
    // 長穴の 2 本と五角形の 5 本
    assert_eq!(lines, 7);
    let id = h.state().selected().unwrap();
    assert!(
        matches!(
            h.state().report().feature(id),
            Some(FeatureStatus::Ok { .. })
        ),
        "{:?}",
        h.state().report().feature(id)
    );
    assert!(has_profile(h.state(), id));
}

#[test]
fn a_sketch_on_a_face_shows_the_closed_outline_of_that_face() {
    // 面の上のスケッチを開くと、どこに描いているか分かるよう面の閉じた輪郭を出す。
    // 網の三角形の内側の辺(同じ位置の点が別の番号の時)は縁として描かない
    let part = kmb_samples::reference_part();
    let (plate, body) = (part.plate, part.hole_pattern);
    let modules = reg();
    let kernel = modules.kernel.clone();
    let ws = ws_of(part.doc, None, modules);
    let h = ws
        .visible_bodies()
        .into_iter()
        .find(|b| b.0 == body)
        .unwrap()
        .1;
    // 作業場と同じカーネルで測る(形の番号は作業場のもの)
    let face = format!("f{}:end", plate.0);
    let (plane, _) =
        kmb_sketch::face_plane(kernel.as_ref(), h, &face, &kmb_sketch::FaceOrigin::World).unwrap();
    let lines = face_outline(kernel.as_ref(), h, &face, &plane);
    assert!(lines.len() > 8, "外形と穴の縁: {}", lines.len());
    let (lo, hi) = kernel.bounds(h).unwrap();
    let xs = lines.iter().flatten().map(|p| p[0]);
    let ys = lines.iter().flatten().map(|p| p[1]);
    let w = xs.clone().fold(f64::MIN, f64::max) - xs.fold(f64::MAX, f64::min);
    let d = ys.clone().fold(f64::MIN, f64::max) - ys.fold(f64::MAX, f64::min);
    let (bw, bd) = (hi[0] - lo[0], hi[1] - lo[1]);
    // 平面の座標の軸の向きは決め方による(縦横が入れ替わってもよい)
    let same = |a: f64, b: f64| (a - b).abs() < 0.1;
    assert!(
        (same(w, bw) && same(d, bd)) || (same(w, bd) && same(d, bw)),
        "{w}×{d} と {bw}×{bd}"
    );
    // 縁の線分は、どの端点も別の線分とつながる(閉じた輪の集まり)
    let key = |p: [f64; 2]| ((p[0] * 1e3).round() as i64, (p[1] * 1e3).round() as i64);
    let mut ends: HashMap<(i64, i64), usize> = HashMap::new();
    for [a, b] in &lines {
        *ends.entry(key(*a)).or_default() += 1;
        *ends.entry(key(*b)).or_default() += 1;
    }
    assert!(ends.values().all(|n| *n == 2), "{ends:?}");
}

/// 空のスケッチを開いた作業場。
pub(crate) fn empty_sketch() -> Harness<'static, Workspace> {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .build_ui_state(
            |ui, ws: &mut Workspace| ws.ui(ui),
            ws_of(Document::new(), None, reg()),
        );
    h.run();
    h.state_mut().run_action(kmb_sketch::ADD_SKETCH);
    h.run();
    h
}

pub(crate) fn press_enter(h: &mut Harness<'static, Workspace>) {
    for pressed in [true, false] {
        h.event(egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        h.run();
    }
}

pub(crate) fn count_of(s: &kmb_sketch::Sketch, f: impl Fn(&kmb_sketch::Entity) -> bool) -> usize {
    s.entities.iter().filter(|e| f(e)).count()
}

pub(crate) fn is_line(e: &kmb_sketch::Entity) -> bool {
    matches!(
        e,
        kmb_sketch::Entity::Line {
            construction: false,
            ..
        }
    )
}

#[test]
fn three_point_rectangle_circle_and_arc_tools() {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::ThreePointRectangle);
    h.run();
    for p in [[-4.03, 5.93], [2.23, 9.06], [-0.38, 14.8]] {
        click_at(&mut h, p);
    }
    let s = current_sketch(h.state());
    assert_eq!(count_of(&s, is_line), 4);
    let id = h.state().selected().unwrap();
    assert!(has_profile(h.state(), id));

    editor(h.state_mut()).set_tool(Tool::ThreePointCircle);
    h.run();
    for p in [[9.01, 16.36], [11.62, 13.75], [9.01, 11.15]] {
        click_at(&mut h, p);
    }
    editor(h.state_mut()).set_tool(Tool::ThreePointArc);
    h.run();
    for p in [[1.19, 18.97], [6.4, 18.97], [3.8, 20.53]] {
        click_at(&mut h, p);
    }
    let s = current_sketch(h.state());
    assert_eq!(
        count_of(&s, |e| matches!(e, kmb_sketch::Entity::Circle { .. })),
        1
    );
    assert_eq!(
        count_of(&s, |e| matches!(e, kmb_sketch::Entity::Arc { .. })),
        1
    );
    assert!(matches!(
        h.state().report().feature(id),
        Some(FeatureStatus::Ok { .. })
    ));
}

#[test]
fn an_outer_hexagon_and_an_arc_slot_with_a_typed_width() {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::OuterPolygon);
    h.run();
    click_at(&mut h, [1.19, 11.15]);
    click_at(&mut h, [4.32, 11.15]);
    let s = current_sketch(h.state());
    assert_eq!(count_of(&s, is_line), 6);

    editor(h.state_mut()).set_tool(Tool::ArcSlot);
    h.run();
    click_at(&mut h, [22.04, 8.54]);
    click_at(&mut h, [27.26, 8.54]);
    click_at(&mut h, [22.04, 13.75]);
    move_to(&mut h, [27.78, 8.54]);
    h.run();
    h.event(egui::Event::Text("6".into()));
    h.run();
    press_enter(&mut h);
    let s = current_sketch(h.state());
    assert!(
        s.constraints.iter().any(|c| matches!(
            &c.kind,
            kmb_sketch::ConstraintKind::Diameter { value, .. } if value == "6 mm"
        )),
        "{:?}",
        s.constraints
    );
    assert_eq!(
        count_of(&s, |e| matches!(
            e,
            kmb_sketch::Entity::Arc {
                construction: false,
                ..
            }
        )),
        4
    );
    let id = h.state().selected().unwrap();
    assert!(has_profile(h.state(), id));
}

#[test]
fn patterns_and_moves_work_on_the_selection() {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Circle);
    h.run();
    click_at(&mut h, [1.19, 11.15]);
    click_at(&mut h, [2.23, 11.15]);
    let s = current_sketch(h.state());
    let circle = s
        .entities
        .iter()
        .find_map(|e| match e {
            kmb_sketch::Entity::Circle { id, .. } => Some(*id),
            _ => None,
        })
        .unwrap();
    // 選ばずに並べる道具を使っても何も起きない
    editor(h.state_mut()).set_tool(Tool::LinearPattern);
    h.run();
    click_at(&mut h, [6.4, 0.72]);
    assert_eq!(current_sketch(h.state()).entities.len(), s.entities.len());

    editor(h.state_mut()).editor.selection.insert(circle);
    editor(h.state_mut()).set_tool(Tool::LinearPattern);
    h.run();
    click_at(&mut h, [1.19, 11.15]);
    h.event(egui::Event::Text("4".into()));
    h.run();
    click_at(&mut h, [4.32, 11.15]);
    let s = current_sketch(h.state());
    assert_eq!(
        count_of(&s, |e| matches!(e, kmb_sketch::Entity::Circle { .. })),
        4
    );

    // 円形に並べる: 中心をクリックし、数を打って Enter
    editor(h.state_mut()).set_tool(Tool::CircularPattern);
    h.run();
    click_at(&mut h, [1.19, 16.36]);
    h.event(egui::Event::Text("3".into()));
    h.run();
    press_enter(&mut h);
    let s = current_sketch(h.state());
    assert_eq!(
        count_of(&s, |e| matches!(e, kmb_sketch::Entity::Circle { .. })),
        6
    );

    // 動かす: 「x, y」を打って Enter
    editor(h.state_mut()).set_tool(Tool::Move);
    h.run();
    click_at(&mut h, [1.19, 11.15]);
    for t in ["10", ",", " -5"] {
        h.event(egui::Event::Text(t.into()));
    }
    h.run();
    press_enter(&mut h);
    let after = current_sketch(h.state());
    let center_of = |s: &kmb_sketch::Sketch| {
        s.entities.iter().find_map(|e| match e {
            kmb_sketch::Entity::Circle { id, center, .. } if *id == circle => {
                s.entities.iter().find_map(|p| match p {
                    kmb_sketch::Entity::Point { id, x, y } if id == center => Some([*x, *y]),
                    _ => None,
                })
            }
            _ => None,
        })
    };
    let (b, a) = (center_of(&s).unwrap(), center_of(&after).unwrap());
    assert!((a[0] - b[0] - 10.0).abs() < 1e-6 && (a[1] - b[1] + 5.0).abs() < 1e-6);
}

#[test]
fn new_constraints_are_offered_for_two_circles_and_two_lines() {
    let mut s = kmb_sketch::Sketch::default();
    let c1 = s.add_point(0.0, 0.0);
    let a = s.add_circle(c1, 5.0);
    let c2 = s.add_point(20.0, 0.0);
    let b = s.add_circle(c2, 3.0);
    let (p, q) = (s.add_point(0.0, 10.0), s.add_point(10.0, 10.0));
    let (r, t) = (s.add_point(12.0, 11.0), s.add_point(20.0, 12.0));
    let l1 = s.add_line(p, q);
    let l2 = s.add_line(r, t);
    let sol = kmb_sketch::solve(&s, &Default::default()).unwrap();
    let ed = view::Editor::default();
    let names = |sel: &[kmb_sketch::EntityId]| -> Vec<&str> {
        constraints_ui::options(&ed, &s, &sol, sel, false)
            .into_iter()
            .map(|(n, _)| n)
            .collect()
    };
    let rounds = names(&[a, b]);
    assert!(
        rounds.contains(&"接線") && rounds.contains(&"同心円"),
        "{rounds:?}"
    );
    assert!(names(&[l1, l2]).contains(&"同一直線上"));
}

#[test]
fn regions_are_found_for_filling_and_holes_are_left_out() {
    let mut s = kmb_sketch::Sketch::default();
    s.add_rectangle((None, [0.0, 0.0]), (None, [40.0, 20.0]))
        .unwrap();
    let c = s.add_point(20.0, 10.0);
    s.add_circle(c, 5.0);
    let sol = kmb_sketch::solve(&s, &Default::default()).unwrap();
    let regions = annot::regions(&s, &sol);
    assert_eq!(regions.len(), 2, "穴のある板と、穴の円の中");
    let plate = regions
        .iter()
        .find(|r| r.loops.len() == 2)
        .expect("穴のある板");
    assert!(annot::contains(&plate.loops, [5.0, 5.0]));
    assert!(
        !annot::contains(&plate.loops, [20.0, 10.0]),
        "穴の中は板に入れない"
    );
    assert!(!annot::contains(&plate.loops, [50.0, 10.0]));
    // 円と長方形を重ねると、円だけ・重なり・長方形だけの 3 つに分かれ、どの点も 1 つにだけ入る
    let mut s = kmb_sketch::Sketch::default();
    let c = s.add_point(0.0, 0.0);
    s.add_circle(c, 10.0);
    s.add_rectangle((None, [0.0, -5.0]), (None, [25.0, 5.0]))
        .unwrap();
    let sol = kmb_sketch::solve(&s, &Default::default()).unwrap();
    let regions = annot::regions(&s, &sol);
    assert_eq!(regions.len(), 3);
    for p in [[-5.0, 0.0], [4.0, 0.0], [20.0, 0.0], [3.0, 8.0]] {
        let n = regions
            .iter()
            .filter(|r| annot::contains(&r.loops, p))
            .count();
        assert_eq!(n, 1, "{p:?}");
    }
}

pub(crate) fn press_key(h: &mut Harness<'static, Workspace>, key: egui::Key) {
    for pressed in [true, false] {
        h.event(egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        h.run();
    }
}

pub(crate) fn right_click_at(h: &mut Harness<'static, Workspace>, p: [f64; 2]) {
    let pos = screen(h, p);
    right_click_px(h, pos);
}

pub(crate) fn right_click_px(h: &mut Harness<'static, Workspace>, pos: egui::Pos2) {
    h.hover_at(pos);
    h.run();
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.run();
    }
}

#[test]
fn a_drawing_tool_continues_after_a_shape_and_can_be_repeated() {
    let mut h = empty_sketch();
    // 円を 1 つ作っても円の道具が続く(使っている道具はリボンで光る)
    editor(h.state_mut()).set_tool(Tool::Circle);
    h.run();
    click_at(&mut h, [1.19, 11.15]);
    click_at(&mut h, [2.75, 11.15]);
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::Circle);
    click_at(&mut h, [9.01, 16.36]);
    click_at(&mut h, [10.57, 16.36]);
    let circles = |h: &Harness<'static, Workspace>| {
        count_of(&current_sketch(h.state()), |e| {
            matches!(e, kmb_sketch::Entity::Circle { .. })
        })
    };
    assert_eq!(circles(&h), 2);
    // Esc で道具を終え、右クリックのメニューから前の道具を繰り返す
    press_key(&mut h, egui::Key::Escape);
    let e = &editor(h.state_mut()).editor;
    assert_eq!((e.tool, e.last_tool), (Tool::Select, Some(Tool::Circle)));
    right_click_at(&mut h, [22.04, 0.72]);
    h.get_by_label("繰り返し: 中心と直径の円").click();
    h.run();
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::Circle);
    click_at(&mut h, [22.04, 0.72]);
    click_at(&mut h, [23.61, 0.72]);
    assert_eq!(circles(&h), 3);
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::Circle);
    press_key(&mut h, egui::Key::Escape);

    // 線はつなげて描け、最初の点へ戻って輪が閉じたら終わる
    editor(h.state_mut()).set_tool(Tool::Line);
    h.run();
    for p in [[-9.24, 5.93], [-4.03, 5.93], [-4.03, 0.72]] {
        click_at(&mut h, p);
    }
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::Line);
    click_at(&mut h, [-9.24, 5.93]);
    // 輪が閉じたらつながりを区切る(道具は続く)
    let e = &editor(h.state_mut()).editor;
    assert_eq!((e.tool, e.pending.is_none()), (Tool::Line, true));
    assert_eq!(count_of(&current_sketch(h.state()), is_line), 3);

    // 何も打たずに Enter でもつながりを区切る(描いた線は残る)
    click_at(&mut h, [16.83, 5.93]);
    click_at(&mut h, [22.04, 5.93]);
    press_enter(&mut h);
    let e = &editor(h.state_mut()).editor;
    assert_eq!((e.tool, e.pending.is_none()), (Tool::Line, true));
    assert_eq!(count_of(&current_sketch(h.state()), is_line), 4);
    press_key(&mut h, egui::Key::Escape);
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::Select);

    // 道具の中の右クリックはメニューを出す。「キャンセル」で描きかけを捨てて道具を終える
    editor(h.state_mut()).set_tool(Tool::Rectangle);
    h.run();
    click_at(&mut h, [16.83, 16.36]);
    right_click_at(&mut h, [19.44, 18.97]);
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::Rectangle);
    h.get_by_label("キャンセル(Esc)").click();
    h.run();
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::Select);
    assert_eq!(count_of(&current_sketch(h.state()), is_line), 4);

    // Esc: 道具の中では道具だけを終えて選びは残し、選ぶ道具では選びを外す
    let line = current_sketch(h.state())
        .entities
        .iter()
        .find_map(|e| match e {
            kmb_sketch::Entity::Line { id, .. } => Some(*id),
            _ => None,
        })
        .unwrap();
    editor(h.state_mut()).editor.selection.insert(line);
    editor(h.state_mut()).set_tool(Tool::Circle);
    h.run();
    press_key(&mut h, egui::Key::Escape);
    let e = &editor(h.state_mut()).editor;
    assert_eq!(e.tool, Tool::Select);
    assert!(e.selection.contains(&line));
    press_key(&mut h, egui::Key::Escape);
    assert!(editor(h.state_mut()).editor.selection.is_empty());
}

/// 戻す物が何も無い時に Esc を素早く 2 回押すとスケッチを終える。
/// 1 段ずつ戻す途中の Esc(選びを外す)は数えず、間の空いた 2 回でも終えない。
#[test]
fn esc_twice_with_nothing_to_undo_finishes_the_sketch() {
    // 1 こまを 1/60 秒にする(既定の 1/4 秒では押して離す間に 0.4 秒を過ぎる)
    let mut ws = ws_of(sample_document(), None, reg());
    let id = ws.document().data().features[0].id;
    ws.open_edit(id);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .with_step_dt(1.0 / 60.0)
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    let line = current_sketch(h.state())
        .entities
        .iter()
        .find_map(|e| match e {
            kmb_sketch::Entity::Line { id, .. } => Some(*id),
            _ => None,
        })
        .unwrap();
    // 選んでいる時: 1 回目で選びを外し、すぐの 2 回目は戻す物が無い 1 回目なので終えない
    editor(h.state_mut()).editor.selection.insert(line);
    press_key(&mut h, egui::Key::Escape);
    assert!(editor(h.state_mut()).editor.selection.is_empty());
    press_key(&mut h, egui::Key::Escape);
    assert!(h.state().is_editing(id));
    // 間を空けると数え直す
    for _ in 0..60 {
        h.step();
    }
    press_key(&mut h, egui::Key::Escape);
    assert!(h.state().is_editing(id));
    // 戻す物が無いまま素早く 2 回で終える(スケッチは選んだまま)
    press_key(&mut h, egui::Key::Escape);
    h.run();
    assert!(!h.state().is_editing(id));
}

/// 押して `to` まで少しずつ動かして離す(人がマウスで引くのと同じ)。
pub(crate) fn drag_on(
    h: &mut Harness<'static, Workspace>,
    from: [f64; 2],
    to: [f64; 2],
    modifiers: egui::Modifiers,
) {
    let (a, b) = (screen(h, from), screen(h, to));
    drag_px(h, a, b, modifiers);
}

/// 画面の位置から位置へ引く。
pub(crate) fn drag_px(
    h: &mut Harness<'static, Workspace>,
    a: egui::Pos2,
    b: egui::Pos2,
    modifiers: egui::Modifiers,
) {
    h.event(egui::Event::ModifiersChanged(modifiers));
    h.hover_at(a);
    h.run();
    h.event(egui::Event::PointerButton {
        pos: a,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers,
    });
    h.run();
    for k in 1..=10 {
        h.event(egui::Event::PointerMoved(a + (b - a) * (k as f32 / 10.0)));
        h.run();
    }
    h.event(egui::Event::PointerButton {
        pos: b,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers,
    });
    h.run();
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    h.run();
}

/// 今の形を解いた点の位置。
pub(crate) fn solved(ws: &Workspace) -> kmb_sketch::Solution {
    let s = current_sketch(ws);
    let params = ws.report().param_values();
    let values = kmb_sketch::dimension_values(&s, &|n| params.get(n).copied()).unwrap();
    kmb_sketch::solve(&s, &values).unwrap()
}

/// 画面の (500, 300) から (700, 450) へ長方形を描いた空のスケッチ。
pub(crate) fn sketch_with_rectangle() -> Harness<'static, Workspace> {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Rectangle);
    h.run();
    click_at(&mut h, [1.19, 16.36]);
    click_at(&mut h, [11.62, 8.54]);
    // 描く道具は続くので、Esc で選ぶ道具へ戻る
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::Rectangle);
    press_key(&mut h, egui::Key::Escape);
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::Select);
    h
}

/// 点の y の値を小さい順に(重なりは 1 つに)。
fn ys(sol: &kmb_sketch::Solution) -> Vec<f64> {
    let mut v: Vec<f64> = sol
        .points
        .values()
        .map(|p| (p[1] * 1e4).round() / 1e4)
        .collect();
    v.sort_by(f64::total_cmp);
    v.dedup();
    v
}

#[test]
fn dragging_a_line_moves_it_keeping_constraints_and_selects_it() {
    let mut h = sketch_with_rectangle();
    let before = solved(h.state());
    let [bottom, top] = ys(&before)[..] else {
        panic!("長方形の辺は 2 つの高さ")
    };
    // 上の辺の真ん中を押して上へ引く(選んでいなくても掴める)
    drag_on(&mut h, [6.4, 16.36], [7.97, 18.97], egui::Modifiers::NONE);
    let after = solved(h.state());
    let [b2, t2] = ys(&after)[..] else {
        panic!("横と縦の拘束が保たれる")
    };
    assert!((b2 - bottom).abs() < 1e-3, "下の辺は動かない");
    assert!(t2 > top + 1.0, "上の辺が上へ動く: {top} → {t2}");
    // 横へは引いた分だけ長方形ごと動く(左右の辺は縦のまま)
    let xs = |s: &kmb_sketch::Solution| {
        let mut v: Vec<f64> = s
            .points
            .values()
            .map(|p| (p[0] * 1e4).round() / 1e4)
            .collect();
        v.sort_by(f64::total_cmp);
        v.dedup();
        v
    };
    assert_eq!(xs(&after).len(), 2);
    // 引いた辺が選ばれる
    let e = &editor(h.state_mut()).editor;
    assert_eq!(e.selection.len(), 1);
    let id = *e.selection.iter().next().unwrap();
    assert!(matches!(
        current_sketch(h.state()).entity(id),
        Some(kmb_sketch::Entity::Line { .. })
    ));
}

#[test]
fn dragging_a_circle_edge_changes_the_radius_and_the_center_moves_it() {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Circle);
    h.run();
    click_at(&mut h, [1.19, 11.15]);
    click_at(&mut h, [4.32, 11.15]);
    press_key(&mut h, egui::Key::Escape);
    let radius =
        |h: &Harness<'static, Workspace>| *solved(h.state()).radii.values().next().unwrap();
    let center =
        |h: &Harness<'static, Workspace>| *solved(h.state()).points.values().next().unwrap();
    let (r0, c0) = (radius(&h), center(&h));
    // 円周を外へ引くと半径が大きくなり、中心は動かない
    drag_on(&mut h, [4.32, 11.15], [7.45, 11.15], egui::Modifiers::NONE);
    let (r1, c1) = (radius(&h), center(&h));
    assert!((r1 / r0 - 2.0).abs() < 0.05, "{r0} → {r1}");
    assert!((c1[0] - c0[0]).abs() < 1e-3 && (c1[1] - c0[1]).abs() < 1e-3);
    // 中心を引くと円が動き、半径は変わらない
    drag_on(&mut h, [1.19, 11.15], [-1.42, 11.15], egui::Modifiers::NONE);
    let (r2, c2) = (radius(&h), center(&h));
    assert!((r2 - r1).abs() < 1e-3);
    assert!(c2[0] < c1[0] - 1.0);
}

#[test]
fn a_window_selects_inside_to_the_right_and_touching_to_the_left() {
    let mut h = sketch_with_rectangle();
    let sel = |h: &mut Harness<'static, Workspace>| editor(h.state_mut()).editor.selection.len();
    // 右へ全体を囲む: 辺 4 本と隅 4 つ
    drag_on(&mut h, [0.15, 17.4], [12.66, 7.5], egui::Modifiers::NONE);
    assert_eq!(sel(&mut h), 8);
    assert!(editor(h.state_mut()).editor.regions.is_empty());
    // 右へ左半分を囲む: 中に入った左の辺と、その両端だけ
    drag_on(&mut h, [0.15, 17.4], [6.4, 7.5], egui::Modifiers::NONE);
    assert_eq!(sel(&mut h), 3);
    // 左へ同じ所を囲む: 触れた上・下・左の辺と左の両端、それに閉じた領域
    drag_on(&mut h, [6.4, 7.5], [0.15, 17.4], egui::Modifiers::NONE);
    assert_eq!(sel(&mut h), 5);
    assert_eq!(editor(h.state_mut()).editor.regions.len(), 1);
    // Shift を押して囲むと足す(右の辺を足す)
    drag_on(&mut h, [11.09, 17.4], [12.66, 7.5], egui::Modifiers::SHIFT);
    assert_eq!(sel(&mut h), 8);
    // 空いた所のクリックで外れる
    click_at(&mut h, [22.04, 0.72]);
    assert_eq!(sel(&mut h), 0);
    assert!(editor(h.state_mut()).editor.regions.is_empty());
}

#[test]
fn dragging_one_of_the_selection_moves_all_of_it() {
    let mut h = sketch_with_rectangle();
    // 円を足しておき、長方形だけを囲んで選ぶ
    editor(h.state_mut()).set_tool(Tool::Circle);
    h.run();
    click_at(&mut h, [22.04, 11.15]);
    click_at(&mut h, [23.61, 11.15]);
    press_key(&mut h, egui::Key::Escape);
    drag_on(&mut h, [0.15, 17.4], [12.66, 7.5], egui::Modifiers::NONE);
    assert_eq!(editor(h.state_mut()).editor.selection.len(), 8);
    let before = solved(h.state());
    // 右の辺を引くと、選んだ長方形全部が同じだけ動き、選びは残る
    drag_on(
        &mut h,
        [11.62, 12.45],
        [14.74, 12.45],
        egui::Modifiers::NONE,
    );
    let after = solved(h.state());
    assert_eq!(editor(h.state_mut()).editor.selection.len(), 8);
    let moved: Vec<f64> = before
        .points
        .iter()
        .map(|(id, p)| after.points[id][0] - p[0])
        .collect();
    // 長方形の 4 隅は同じだけ右へ、円の中心は動かない
    let rect: Vec<f64> = moved.iter().copied().filter(|d| d.abs() > 1e-3).collect();
    assert_eq!(rect.len(), 4, "{moved:?}");
    assert!(rect.iter().all(|d| (d - rect[0]).abs() < 1e-3 && *d > 1.0));
}

#[test]
fn a_fixed_point_is_not_dragged_and_says_why() {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Point);
    h.run();
    click_at(&mut h, [6.4, 11.15]);
    press_key(&mut h, egui::Key::Escape);
    // 点に固定の拘束を付ける
    let ws = h.state_mut();
    let id = ws.selected().unwrap();
    let mut s = current_sketch(ws);
    let p = s
        .entities
        .iter()
        .find_map(|e| match e {
            kmb_sketch::Entity::Point { id, .. } => Some(*id),
            _ => None,
        })
        .unwrap();
    s.add_constraint(kmb_sketch::ConstraintKind::Fix { point: p });
    assert!(ws.set_feature_data(id, serde_json::to_value(&s).unwrap()));
    h.run();
    let before = solved(h.state()).points[&p];
    // 押したまま引いている間は理由を出し、離しても動かない
    let a = egui::pos2(600.0, 400.0);
    h.hover_at(a);
    h.run();
    h.event(egui::Event::PointerButton {
        pos: a,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
    for k in 1..=5 {
        h.event(egui::Event::PointerMoved(
            a + egui::vec2(10.0 * k as f32, 0.0),
        ));
        h.run();
    }
    assert_eq!(
        editor(h.state_mut())
            .editor
            .drag
            .as_ref()
            .and_then(|g| g.blocked),
        Some(crate::drag::FIXED)
    );
    h.event(egui::Event::PointerButton {
        pos: a + egui::vec2(50.0, 0.0),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
    assert_eq!(solved(h.state()).points[&p], before);
    // 掴んだ点は選ばれる
    assert!(editor(h.state_mut()).editor.selection.contains(&p));
}

/// 押してすぐ離すクリック(入力欄の点滅などで画面の時間が進み、押す時間が長くなりすぎないよう、
/// 1 フレームずつ進める)。
pub(crate) fn quick_click(h: &mut Harness<'static, Workspace>, p: [f64; 2]) {
    let pos = screen(h, p);
    quick_click_px(h, pos);
}

pub(crate) fn quick_click_px(h: &mut Harness<'static, Workspace>, pos: egui::Pos2) {
    h.hover_at(pos);
    h.step();
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.step();
    }
    h.step();
    h.step();
}

pub(crate) fn type_text(h: &mut Harness<'static, Workspace>, text: &str) {
    h.event(egui::Event::Text(text.into()));
    h.run();
}

#[test]
fn a_dimension_follows_the_cursor_and_takes_its_value_where_it_is_placed() {
    let mut h = sketch_with_rectangle();
    press_key(&mut h, egui::Key::D);
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::Dimension);
    // 上の辺を選び、上の空いた所へカーソルを出すと長さの寸法がついてくる
    click_at(&mut h, [6.4, 16.36]);
    move_to(&mut h, [6.4, 19.49]);
    h.run();
    assert!(matches!(
        editor(h.state_mut()).editor.ghost,
        Some((ConstraintKind::Length { .. }, _))
    ));
    // クリックで置くと、その場に値の欄が出る。30 と Enter で付く(mm を補う)
    quick_click(&mut h, [6.4, 19.49]);
    assert!(editor(h.state_mut()).editor.placing.is_some());
    type_text(&mut h, "30");
    press_enter(&mut h);
    let s = current_sketch(h.state());
    let c = s
        .constraints
        .iter()
        .find(|c| matches!(c.kind, ConstraintKind::Length { .. }))
        .expect("長さの寸法");
    assert_eq!(c.kind.value(), Some("30 mm"));
    assert!(c.label.is_some(), "置いた位置を(形に対する相対で)覚える");
    assert!(c.at.is_none(), "古い絶対座標は書かない");
    // 道具は続き、選びは外れる
    let e = editor(h.state_mut());
    assert_eq!(e.editor.tool, Tool::Dimension);
    assert!(e.editor.selection.is_empty() && e.editor.placing.is_none());
    let s = current_sketch(h.state());
    let v = kmb_sketch::dimension_values(&s, &|_| None).unwrap();
    let sol = kmb_sketch::solve(&s, &v).unwrap();
    let xs: Vec<f64> = sol.points.values().map(|p| p[0]).collect();
    let w =
        xs.iter().cloned().fold(f64::MIN, f64::max) - xs.iter().cloned().fold(f64::MAX, f64::min);
    assert!((w - 30.0).abs() < 1e-6, "幅 {w}");
}

#[test]
fn esc_while_placing_a_dimension_cancels_only_the_value() {
    // 寸法を置く途中の Esc は値の欄だけをやめ、選びと道具は残る
    // 値を打った後でも Esc は欄をやめ、寸法は付かない(欄の外のクリックで決まるようになっても)
    let mut h = sketch_with_rectangle();
    press_key(&mut h, egui::Key::D);
    click_at(&mut h, [1.19, 12.19]);
    quick_click(&mut h, [-1.94, 12.19]);
    assert!(editor(h.state_mut()).editor.placing.is_some());
    type_text(&mut h, "30");
    press_key(&mut h, egui::Key::Escape);
    let e = editor(h.state_mut());
    assert!(e.editor.placing.is_none());
    assert_eq!(e.editor.selection.len(), 1, "選びは残る");
    assert_eq!(e.editor.tool, Tool::Dimension);
    assert!(
        !current_sketch(h.state())
            .constraints
            .iter()
            .any(|c| c.kind.value().is_some())
    );
}

#[test]
fn the_cursor_side_picks_horizontal_vertical_or_aligned_for_a_slanted_line() {
    let mut s = kmb_sketch::Sketch::default();
    let a = s.add_point(0.0, 0.0);
    let b = s.add_point(10.0, 5.0);
    let l = s.add_line(a, b);
    let sol = kmb_sketch::solve(&s, &Default::default()).unwrap();
    let at = |w| crate::view::dimension_at(&s, &sol, &[l], w).unwrap();
    assert!(matches!(
        at([5.0, 9.0]),
        ConstraintKind::HorizontalDistance { .. }
    ));
    assert!(matches!(
        at([14.0, 2.0]),
        ConstraintKind::VerticalDistance { .. }
    ));
    assert!(matches!(at([12.0, 8.0]), ConstraintKind::Length { .. }));
}

#[test]
fn clicking_inside_a_closed_shape_selects_it_and_e_extrudes_only_that() {
    let mut h = sketch_with_rectangle();
    let id = h.state().selected().unwrap();
    click_at(&mut h, [6.4, 12.19]);
    {
        let e = editor(h.state_mut());
        assert_eq!(e.editor.regions.len(), 1);
        assert!(e.editor.selection.is_empty());
    }
    // 空いた所のクリックで外れる
    click_at(&mut h, [22.04, 0.72]);
    assert!(editor(h.state_mut()).editor.regions.is_empty());
    click_at(&mut h, [6.4, 12.19]);
    // E でスケッチを終えて押し出しへ。高さを打って Enter
    press_key(&mut h, egui::Key::E);
    h.run();
    assert!(!h.state().is_editing(id), "スケッチを終了");
    type_text(&mut h, "5");
    press_enter(&mut h);
    h.state_mut().wait();
    let data = h.state().document().data();
    let ex = data
        .features
        .iter()
        .find(|f| f.type_id == "solid.extrude@1")
        .expect("押し出しができる");
    assert_eq!(ex.data["profile"], serde_json::json!(id.0));
    assert_eq!(ex.data["distance"], serde_json::json!("5 mm"));
    // 長方形は領域が 1 つなので、名前は覚えずに全体を押し出す
    assert!(ex.data.get("regions").is_none());
}

#[test]
fn a_long_press_lists_what_overlaps_and_picks_one() {
    let mut h = sketch_with_rectangle();
    press_key(&mut h, egui::Key::Escape);
    assert!(editor(h.state_mut()).editor.selection.is_empty());
    // 隅を長押し: 隅の点・2 本の辺・閉じた領域が重なる
    let pos = egui::pos2(500.0, 300.0);
    h.hover_at(pos);
    h.run();
    h.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    for _ in 0..6 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(120));
    }
    h.run();
    h.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
    let list = editor(h.state_mut())
        .editor
        .others
        .clone()
        .expect("重なりの一覧")
        .1;
    assert!(list.len() >= 3, "{list:?}");
    assert!(
        editor(h.state_mut()).editor.selection.is_empty(),
        "離した時のクリックでは選ばない"
    );
    // 一覧の 2 つ目(辺)を押すと、それだけを選ぶ
    // (帯の道具「線分」と分けるため、番号の前の空白まで含めて探す)
    h.get_all_by_label_contains("線分 ")
        .next()
        .expect("辺の候補")
        .click();
    h.run();
    let e = editor(h.state_mut());
    assert!(e.editor.others.is_none());
    assert_eq!(e.editor.selection.len(), 1);
}

#[test]
fn alt_click_also_lists_what_overlaps() {
    // 長押しのほかの入口: Alt+クリックで同じ一覧を出す(何も選ばない)
    let mut h = sketch_with_rectangle();
    press_key(&mut h, egui::Key::Escape);
    let pos = egui::pos2(500.0, 300.0);
    h.hover_at(pos);
    h.step();
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::ALT));
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::ALT,
        });
        h.step();
    }
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    h.run();
    let list = editor(h.state_mut())
        .editor
        .others
        .clone()
        .expect("重なりの一覧")
        .1;
    assert!(list.len() >= 3, "{list:?}");
    assert!(editor(h.state_mut()).editor.selection.is_empty());
}

/// 円弧の向き(中心から見て start から end へ反時計回りに測った角度、度)。
fn arc_sweep(ws: &Workspace) -> f64 {
    let s = current_sketch(ws);
    let sol = solved(ws);
    let (c, a, b) = s
        .entities
        .iter()
        .rev()
        .find_map(|e| match e {
            kmb_sketch::Entity::Arc {
                center, start, end, ..
            } => Some((sol.points[center], sol.points[start], sol.points[end])),
            _ => None,
        })
        .expect("円弧がある");
    let ang = |p: [f64; 2]| (p[1] - c[1]).atan2(p[0] - c[0]);
    (ang(b) - ang(a))
        .rem_euclid(std::f64::consts::TAU)
        .to_degrees()
}

#[test]
fn tab_locks_a_typed_width_and_the_height_is_typed_next() {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Rectangle);
    h.run();
    click_at(&mut h, [-1.42, 4.89]);
    move_to(&mut h, [1.19, 6.98]);
    h.run();
    type_text(&mut h, "30");
    press_key(&mut h, egui::Key::Tab);
    {
        let hud = &editor(h.state_mut()).editor.hud;
        assert!(hud.fields[0].locked, "Tab で幅が固定される");
        assert_eq!(hud.active, 1, "次は高さの欄");
    }
    // 固定した幅はカーソルを動かしても変わらない。高さを打って Enter
    move_to(&mut h, [22.04, 16.36]);
    h.run();
    type_text(&mut h, "20");
    press_enter(&mut h);
    let s = current_sketch(h.state());
    let lengths: Vec<&str> = s
        .constraints
        .iter()
        .filter_map(|c| match &c.kind {
            ConstraintKind::Length { value, .. } => Some(value.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(lengths, ["30 mm", "20 mm"]);
    // 描く道具は続き、欄は空に戻る
    let e = &editor(h.state_mut()).editor;
    assert_eq!(e.tool, Tool::Rectangle);
    assert!(e.hud.is_blank());
}

#[test]
fn an_arc_turns_the_way_the_cursor_moved() {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Arc);
    h.run();
    // 中心の右に始点を置き、下へ回して(画面で時計回り)真下で決める
    click_at(&mut h, [6.4, 11.15]);
    click_at(&mut h, [10.57, 11.15]);
    for (x, y) in [
        (679.0, 410.0),
        (670.0, 440.0),
        (640.0, 470.0),
        (600.0, 480.0),
    ] {
        h.hover_at(egui::pos2(x, y));
        h.run();
    }
    click_at(&mut h, [6.4, 6.98]);
    let cw = arc_sweep(h.state());
    assert!((cw - 90.0).abs() < 1.0, "時計回りの 4 分の 1: {cw}");
    // 同じ所から上へ回して(反時計回り)真下で決めると 4 分の 3
    click_at(&mut h, [22.04, 11.15]);
    click_at(&mut h, [26.21, 11.15]);
    for (x, y) in [
        (979.0, 390.0),
        (960.0, 340.0),
        (900.0, 320.0),
        (840.0, 340.0),
        (820.0, 400.0),
        (840.0, 460.0),
        (900.0, 480.0),
    ] {
        h.hover_at(egui::pos2(x, y));
        h.run();
    }
    click_at(&mut h, [22.04, 6.98]);
    let ccw = arc_sweep(h.state());
    assert!((ccw - 270.0).abs() < 1.0, "反時計回りの 4 分の 3: {ccw}");
}

#[test]
fn a_constraint_mark_is_shown_picked_and_deleted() {
    let mut h = sketch_with_rectangle();
    h.run();
    let marks = editor(h.state_mut()).view.marks.clone();
    let before = current_sketch(h.state()).constraints.len();
    // 矩形の水平 2 つと垂直 2 つの印が図の上に出る
    assert_eq!(marks.len(), 4, "{marks:?}");
    let m = marks[0].clone();
    click_px(&mut h, m.rect.center());
    {
        let e = &editor(h.state_mut()).editor;
        assert_eq!(e.picked_constraint, Some(m.id));
        assert!(e.selection.is_empty(), "印を選ぶと線の選びは外れる");
    }
    press_key(&mut h, egui::Key::Delete);
    let s = current_sketch(h.state());
    assert_eq!(s.constraints.len(), before - 1);
    assert!(s.constraints.iter().all(|c| c.id != m.id));
}

#[test]
fn tab_before_the_first_point_does_not_move_focus_to_a_button() {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Line);
    h.run();
    move_to(&mut h, [6.4, 11.15]);
    h.run();
    // 欄が出る前の Tab でも、焦点がファイルなどのボタンへ移らない(移ると続く Enter でメニューが開く)
    press_key(&mut h, egui::Key::Tab);
    press_key(&mut h, egui::Key::Tab);
    assert!(h.ctx.memory(|m| m.focused().is_none()), "焦点が移った");
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::Line);
}

/// 押して、`center` のまわりを `from` から角度 `turn`(rad、画面で反時計回りが正)だけ
/// 少しずつ回して離す(人がマウスで円を描くように引くのと同じ)。
fn drag_around(
    h: &mut Harness<'static, Workspace>,
    center: [f64; 2],
    from: [f64; 2],
    turn: f32,
    steps: usize,
) {
    let (c, f) = (screen(h, center), screen(h, from));
    let (cx, cy) = (c.x, c.y);
    let (dx, dy) = (f.x - cx, f.y - cy);
    let at = |k: usize| {
        let t = turn * k as f32 / steps as f32;
        // 画面の y は下向きなので、反時計回りは y を引く向き
        egui::pos2(
            cx + dx * t.cos() + dy * t.sin(),
            cy - dx * t.sin() + dy * t.cos(),
        )
    };
    h.hover_at(at(0));
    h.run();
    h.event(egui::Event::PointerButton {
        pos: at(0),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
    for k in 1..=steps {
        h.event(egui::Event::PointerMoved(at(k)));
        h.run();
    }
    h.event(egui::Event::PointerButton {
        pos: at(steps),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
}

/// 長穴(線分 2 本・円弧 2 本)が崩れていないか: 両端の円弧は半円で半径が `r`、
/// 2 本の線は両方の円弧に接し、両端の中心は線の同じ側にある(線が交差していない)。
fn assert_slot_intact(s: &kmb_sketch::Sketch, sol: &kmb_sketch::Solution, r: f64) {
    use kmb_sketch::Entity;
    let arcs: Vec<_> = s
        .entities
        .iter()
        .filter_map(|e| match e {
            Entity::Arc {
                center, start, end, ..
            } => Some((sol.points[center], sol.points[start], sol.points[end])),
            _ => None,
        })
        .collect();
    let lines: Vec<_> = s
        .entities
        .iter()
        .filter_map(|e| match e {
            Entity::Line {
                a,
                b,
                construction: false,
                ..
            } => Some((sol.points[a], sol.points[b])),
            _ => None,
        })
        .collect();
    assert_eq!((arcs.len(), lines.len()), (2, 2));
    for (c, p, q) in &arcs {
        let a0 = (p[1] - c[1]).atan2(p[0] - c[0]);
        let a1 = (q[1] - c[1]).atan2(q[0] - c[0]);
        let sweep = (a1 - a0).rem_euclid(std::f64::consts::TAU);
        assert!(
            (sweep - std::f64::consts::PI).abs() < 1e-3,
            "円弧が半円でない: {:.1}°",
            sweep.to_degrees()
        );
        let rr = (p[0] - c[0]).hypot(p[1] - c[1]);
        assert!(
            (rr - r).abs() < 1e-3 * r.max(1.0),
            "半径 {rr} が {r} から変わった"
        );
    }
    for (a, b) in &lines {
        let side = |c: [f64; 2]| {
            let (u, w) = ([b[0] - a[0], b[1] - a[1]], [c[0] - a[0], c[1] - a[1]]);
            (u[0] * w[1] - u[1] * w[0]) / u[0].hypot(u[1])
        };
        let (d1, d2) = (side(arcs[0].0), side(arcs[1].0));
        assert!(
            d1 * d2 > 0.0,
            "線が両端の中心の間を通る(交差した形): {d1:.3} {d2:.3}"
        );
        assert!(
            (d1.abs() - r).abs() < 1e-3 * r.max(1.0) && (d2.abs() - r).abs() < 1e-3 * r.max(1.0)
        );
    }
}

/// 長穴の右端の円弧の中心を、左端の中心のまわりに引いて回しても、長穴の形を保つ
/// (押した時の形から一度に解くと、少し回しただけで線が交差した形に跳ぶ)。
#[test]
fn rotating_a_slot_by_dragging_its_end_keeps_the_slot_shape() {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Slot);
    h.run();
    click_at(&mut h, [1.19, 11.15]);
    click_at(&mut h, [9.01, 11.15]);
    click_at(&mut h, [5.1, 9.58]);
    // 道具は続くので、Esc で選ぶ道具へ戻してから引く
    press_key(&mut h, egui::Key::Escape);
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::Select);
    let s = current_sketch(h.state());
    let sol = solved(h.state());
    let r = s
        .entities
        .iter()
        .find_map(|e| match e {
            kmb_sketch::Entity::Arc { center, start, .. } => {
                let (c, p) = (sol.points[center], sol.points[start]);
                Some((p[0] - c[0]).hypot(p[1] - c[1]))
            }
            _ => None,
        })
        .unwrap();
    assert_slot_intact(&s, &sol, r);
    // 右端の中心を、左端の中心のまわりに 150° 回す(画面で上へ)
    drag_around(
        &mut h,
        [1.19, 11.15],
        [9.01, 11.15],
        150f32.to_radians(),
        30,
    );
    let s = current_sketch(h.state());
    let sol = solved(h.state());
    assert_slot_intact(&s, &sol, r);
    // 右端の中心はカーソルについてきた(左上へ回った)
    let ends: Vec<[f64; 2]> = s
        .entities
        .iter()
        .filter_map(|e| match e {
            kmb_sketch::Entity::Arc { center, .. } => Some(sol.points[center]),
            _ => None,
        })
        .collect();
    let (c0, c1) = (ends[0], ends[1]);
    let ang = (c1[1] - c0[1]).atan2(c1[0] - c0[0]).to_degrees();
    let ang2 = (c0[1] - c1[1]).atan2(c0[0] - c1[0]).to_degrees();
    assert!(
        (ang - 150.0).abs() < 5.0 || (ang2 - 150.0).abs() < 5.0,
        "回った向き {ang:.1}° / {ang2:.1}°"
    );
    // 逆へ 300° 回しても崩れない
    // 中心から半径 7.82 mm(画面で 150 px)の 150° の所から
    let p = {
        let t = 150f64.to_radians();
        [1.19 + 7.82 * t.cos(), 11.15 + 7.82 * t.sin()]
    };
    drag_around(&mut h, [1.19, 11.15], p, -300f32.to_radians(), 60);
    assert_slot_intact(&current_sketch(h.state()), &solved(h.state()), r);
}

#[test]
fn the_panel_value_follows_the_dimension_drawn_at_the_cursor() {
    // 人の点検(2026-10-15、網羅版 L8): 斜めの線の上へカーソルを出すと図は横の距離を出すのに、
    // パネルの値の欄は斜めの長さを測り、Enter で斜めの寸法が付いた
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Line);
    h.run();
    click_at(&mut h, [1.19, 8.54]);
    click_at(&mut h, [11.62, 16.36]);
    editor(h.state_mut()).set_tool(Tool::Select);
    h.run();
    press_key(&mut h, egui::Key::D);
    click_at(&mut h, [6.4, 12.45]);
    // 線の横の範囲の上へ出すと、図は横の距離
    move_to(&mut h, [6.4, 18.97]);
    h.run();
    let e = editor(h.state_mut());
    assert!(matches!(
        e.editor.ghost,
        Some((ConstraintKind::HorizontalDistance { .. }, _))
    ));
    let horizontal = e.editor.ghost.clone().unwrap().1;
    // パネルの欄も横の距離を測り、種類の名前も同じ
    assert_eq!(e.editor.dim_text, horizontal);
    h.get_by_label("水平距離の寸法");
    // 欄に値を打って Enter: 図に出ていた横の距離が付く
    editor(h.state_mut()).editor.dim_text = "8".into();
    h.run();
    press_enter(&mut h);
    let s = current_sketch(h.state());
    let c = s.constraints.iter().find(|c| c.kind.value().is_some());
    assert!(
        matches!(
            c.map(|c| &c.kind),
            Some(ConstraintKind::HorizontalDistance { .. })
        ),
        "{c:?}"
    );
    assert_eq!(c.unwrap().kind.value(), Some("8 mm"));
    // 選び直して右へ出すと縦の距離に変わる(打っていなければ欄も測り直す)。
    // 横の距離が 8 mm になって線は動いたので、選びは直接入れる
    let line = s
        .entities
        .iter()
        .find(|e| is_line(e))
        .map(|e| e.id())
        .unwrap();
    editor(h.state_mut()).editor.selection.insert(line);
    h.run();
    move_to(&mut h, [27.26, 12.45]);
    h.run();
    h.run();
    let e = editor(h.state_mut());
    assert!(matches!(
        e.editor.ghost,
        Some((ConstraintKind::VerticalDistance { .. }, _))
    ));
    assert_eq!(
        Some(&e.editor.dim_text),
        e.editor.ghost.as_ref().map(|g| &g.1)
    );
}

#[test]
fn the_band_tiles_stay_put_when_the_tool_changes() {
    // 網羅版 L9: 「選ぶ」の札が選ぶ道具の時だけ描くのまとまりに出て、トリムから右の札が 1 枚分ずれた
    let mut h = empty_sketch();
    let x_of = |h: &mut Harness<'static, Workspace>, label: &str| {
        h.run();
        h.get_by_label(label).rect().left()
    };
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::Select);
    let trim = x_of(&mut h, "トリム");
    let dim = x_of(&mut h, "スケッチ寸法");
    let select = x_of(&mut h, "選ぶ");
    for t in [Tool::Line, Tool::Circle, Tool::Dimension] {
        editor(h.state_mut()).set_tool(t);
        assert_eq!(x_of(&mut h, "トリム"), trim, "{t:?}");
        assert_eq!(x_of(&mut h, "スケッチ寸法"), dim, "{t:?}");
        // 選ぶの札は、ほかの道具の間も同じ所に出ている
        assert_eq!(x_of(&mut h, "選ぶ"), select, "{t:?}");
    }
}

#[test]
fn the_draw_list_puts_the_ways_of_drawing_under_their_kind() {
    // レビュー K2: 矩形の 2 点・3 点・中心は「矩形 ▸」の子
    let mut h = empty_sketch();
    h.run();
    // 帯の札は種類の名前(ミラーは描くへ移った。円弧は ▾ だけ、
    // オフセットは前に出す。Kie 2026-10-21 の画板)
    for l in [
        "線分",
        "長方形",
        "円",
        "長穴",
        "ミラー",
        "トリム",
        "フィレット",
        "オフセット",
    ] {
        assert!(h.query_by_label(l).is_some(), "{l}");
    }
    h.get_by_label("作成 ▾").click();
    h.run();
    // 子は親の行に指を置くまで出ない
    assert!(h.query_by_label("中心指定の長方形").is_none());
    let rows: Vec<_> = h.get_all_by_label("長方形").collect();
    assert_eq!(rows.len(), 2, "帯の札と一覧の親の行");
    rows[1].hover();
    h.run();
    h.run();
    h.get_by_label("中心指定の長方形").click();
    h.run();
    h.run();
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::CenterRectangle);
    // 一覧は閉じ、使っている道具は帯の後ろに出る(同じ種類が 2 つなので道具の名前で)
    assert!(h.query_by_label("3 点指定の長方形").is_none());
    assert!(h.query_by_label("中心指定の長方形").is_some());
    assert!(h.query_by_label("2 点指定の長方形").is_some());
}

#[test]
fn a_sketch_being_edited_looks_different_from_the_design_panel() {
    // レビュー K15: スケッチ中は差し色が水色になり、パネルの下は OK でなく「スケッチを終了」
    let mut h = empty_sketch();
    h.run();
    assert!(kmb_ui::theme::look(&h.ctx).sketch);
    let finish = "✓  スケッチを終了";
    assert_eq!(h.get_all_by_label(finish).count(), 2, "帯とパネルの下");
    assert!(h.query_by_label("OK").is_none());
    h.get_all_by_label(finish).last().unwrap().click();
    h.run();
    h.run();
    assert!(h.query_by_label(finish).is_none());
    assert!(!kmb_ui::theme::look(&h.ctx).sketch);
}

/// スケッチの道具のキーも設定の割り当て(U36)に従う。付け替えると元のキーは効かず、説明も替わる。
#[test]
fn sketch_tool_keys_follow_the_remapped_keys() {
    let mut h = harness();
    h.run();
    h.state_mut()
        .layout_mut()
        .keys
        .insert("sketch_line".into(), vec!["Shift+L".into()]);
    h.state_mut()
        .layout_mut()
        .keys
        .insert("sketch_circle".into(), vec![]);
    h.run();
    press_key(&mut h, egui::Key::C);
    assert_ne!(
        editor(h.state_mut()).editor.tool,
        view::Tool::Circle,
        "外した C"
    );
    press_key(&mut h, egui::Key::L);
    assert_ne!(
        editor(h.state_mut()).editor.tool,
        view::Tool::Line,
        "L は効かない"
    );
    for pressed in [true, false] {
        h.event(egui::Event::Key {
            key: egui::Key::L,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::SHIFT,
        });
        h.run();
    }
    assert_eq!(editor(h.state_mut()).editor.tool, view::Tool::Line);
    assert_eq!(view::tool_key(view::Tool::Line).as_deref(), Some("Shift+L"));
    assert_eq!(view::tool_key(view::Tool::Circle), None);
    // R は既定のまま
    press_key(&mut h, egui::Key::R);
    assert_eq!(editor(h.state_mut()).editor.tool, view::Tool::Rectangle);
}

#[test]
fn the_sketch_toolbar_follows_the_board() {
    // 画板「JC の画面」で Kie と決めた並び(2026-10-21、kmb-ui の board/toolbar.json)と比べる:
    // まとまりの順、前に出す道具、拘束の残りの 8 つは 2 段の小さなボタン、絵と役、▾ の一覧。
    // まだ入っていない物は kmb-ui の board/known-diffs.txt に書く
    let mut h = harness();
    h.run();
    let groups = h.state_mut().view_tool_groups();
    let tiles = |t: &[kmb_ui::EditorTool]| -> Vec<(String, String)> {
        t.iter()
            .map(|t| {
                (
                    t.sub.clone().unwrap_or_else(|| t.label.clone()),
                    t.icon.clone(),
                )
            })
            .collect()
    };
    let actual: Vec<kmb_ui::board::Actual> = groups
        .iter()
        .map(|g| kmb_ui::board::Actual {
            name: g.name.clone(),
            front: tiles(&g.tools),
            small: tiles(&g.small),
            menu: g.more.iter().map(|t| t.label.clone()).collect(),
        })
        .collect();
    if let Err(e) = kmb_ui::board::check("スケッチ", &actual) {
        panic!("{e}");
    }
    // 拘束の ▾ の一覧には、全部の拘束と自動拘束が出る
    let constraint = groups.iter().find(|g| g.name == "拘束").unwrap();
    let menu: Vec<&str> = constraint.menu().iter().map(|t| t.label.as_str()).collect();
    for n in [
        "水平/垂直",
        "接線",
        "線上",
        "円周上",
        "ポリゴン",
        "自動拘束",
    ] {
        assert!(menu.contains(&n), "{n}: {menu:?}");
    }
}

/// 道具の(名前, アイコンの名前)を、前・小さなボタン・▾ の一覧から全部。
pub(crate) fn all_tools(groups: &[kmb_ui::ToolGroup]) -> Vec<(String, String)> {
    groups
        .iter()
        .flat_map(|g| g.tools.iter().chain(&g.small).chain(&g.more))
        .map(|t| (t.label.clone(), t.icon.clone()))
        .collect()
}

#[test]
fn every_sketch_tool_has_a_role_or_one_layer() {
    // ▾ の一覧の道具(画板に絵の無い物)も、役の色の変わる部分を持つか 1 層の表にある
    let mut h = harness();
    h.run();
    let gaps = kmb_ui::board::layer_gaps(&all_tools(&h.state_mut().view_tool_groups()));
    assert!(gaps.is_empty(), "{}", gaps.join("\n"));
}

#[test]
fn a_new_sketch_says_it_has_no_shape_yet() {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .build_ui_state(
            |ui, ws: &mut Workspace| ws.ui(ui),
            ws_of(Document::new(), None, reg()),
        );
    h.run();
    h.state_mut().run_action(kmb_sketch::ADD_SKETCH);
    h.run();
    // 何も描いていないのに「完全拘束」と出すと、描き終えたと思わせる
    assert!(h.query_by_label_contains("完全拘束").is_none());
    assert!(h.query_by_label_contains(kmb_sketch::NO_SHAPE).is_some());
    editor(h.state_mut()).set_tool(Tool::Line);
    h.run();
    for p in [[6.4, 8.54], [16.83, 8.54]] {
        click_at(&mut h, p);
    }
    assert!(h.query_by_label_contains(kmb_sketch::NO_SHAPE).is_none());
}

#[test]
fn a_horizontal_dimension_on_a_slanted_line_does_not_move_its_points() {
    // 10-22 t3(起きない見込みの確かめ): 斜めの線を選び、カーソルを上へ出して横の距離の寸法を
    // 値を打たずに Enter で付けても、形が勝手に縮まない(測った値のまま付く)
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Line);
    h.run();
    click_at(&mut h, [1.19, 8.54]);
    click_at(&mut h, [11.62, 16.36]);
    editor(h.state_mut()).set_tool(Tool::Select);
    h.run();
    let before = current_sketch(h.state());
    let point = |s: &kmb_sketch::Sketch| -> Vec<[f64; 2]> {
        s.entities
            .iter()
            .filter_map(|e| match e {
                kmb_sketch::Entity::Point { x, y, .. } => Some([*x, *y]),
                _ => None,
            })
            .collect()
    };
    let drawn = point(&before);
    press_key(&mut h, egui::Key::D);
    click_at(&mut h, [6.4, 12.45]);
    move_to(&mut h, [6.4, 18.97]);
    h.run();
    assert!(matches!(
        editor(h.state_mut()).editor.ghost,
        Some((ConstraintKind::HorizontalDistance { .. }, _))
    ));
    press_enter(&mut h);
    let s = current_sketch(h.state());
    assert!(
        s.constraints
            .iter()
            .any(|c| matches!(c.kind, ConstraintKind::HorizontalDistance { .. })),
        "{:?}",
        s.constraints
    );
    let v = kmb_sketch::dimension_values(&s, &|_| None).unwrap();
    let sol = kmb_sketch::solve(&s, &v).unwrap();
    let mut s2 = s.clone();
    s2.adopt_solution(&sol);
    for (a, b) in drawn.iter().zip(point(&s2)) {
        assert!(
            (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6,
            "{a:?} → {b:?}"
        );
    }
}
