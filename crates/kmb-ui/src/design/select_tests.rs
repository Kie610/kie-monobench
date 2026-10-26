//! 選ぶ物の試験: 頂点・辺・面を混ぜて選ぶ、下の帯の数値、選ぶ欄(ブーリアンの対象と道具)。

use crate::select_ui::Hit;
use crate::{PickFilter, Workspace};
use egui::{Pos2, Rect};
use kmb_cad::{Direction, Operation};
use kmb_cad_extrude::{EXTRUDE, Extrude};
use kmb_core::{FeatureId, Ref};
use kmb_samples::sample::{DocBuilder, SketchBuilder};
use kmb_sketch::SketchPlane;

fn reg() -> kmb_core::Modules {
    kmb_bundle::modules()
}

fn rect() -> Rect {
    Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0))
}

/// 長方形を押し出した新しいボディ(底は z0)。
fn block(b: &mut DocBuilder, name: &str, x: f64, y: f64, w: f64, h: f64, z: f64) -> FeatureId {
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let o = s.fixed_point(0.0, 0.0);
    let c = s.point_from(o, &format!("{x} mm"), &format!("{y} mm"), [x, y]);
    s.rectangle(c, &format!("{w} mm"), &format!("{h} mm"), [x, y, w, h]);
    let sk = b.feature(kmb_sketch::SKETCH, &format!("{name}のスケッチ"), s.build());
    b.feature(
        EXTRUDE,
        name,
        Extrude {
            profile: Some(sk),
            regions: Vec::new(),
            distance: format!("{z} mm"),
            direction: Direction::Normal,
            distance2: None,
            extent: Default::default(),
            to_face: None,
            to_offset: None,
            operation: Operation::New,
            target: None,
            ..Default::default()
        },
    )
}

/// 30×20×10 の板と、板を突き抜ける 4×4×20 の柱 2 本(どれも別のボディ)。
fn plate_and_posts() -> (Workspace, [FeatureId; 3]) {
    let mut b = DocBuilder::new();
    let plate = block(&mut b, "板", 0.0, 0.0, 30.0, 20.0, 10.0);
    let a = block(&mut b, "柱A", 5.0, 5.0, 4.0, 4.0, 20.0);
    let c = block(&mut b, "柱B", 20.0, 5.0, 4.0, 4.0, 20.0);
    let ws = Workspace::new(b.finish("板と柱"), None, reg());
    (ws, [plate, a, c])
}

fn at(ws: &Workspace, p: [f64; 3]) -> Pos2 {
    ws.cur.camera.project(p.map(|v| v as f32), rect()).0
}

fn volume(ws: &Workspace) -> f64 {
    ws.visible_bodies()
        .iter()
        .map(|b| ws.cur.work.kernel().mass(b.1).unwrap().volume)
        .sum()
}

#[test]
fn vertices_edges_and_faces_are_picked_together_and_the_bar_shows_values() {
    // 頂点も選べ、面・辺・ボディ・頂点の種類を混ぜて選べる
    let part = kmb_samples::reference_part();
    let mut ws = Workspace::new(part.doc, None, reg());
    assert_eq!(ws.win.pick_filter, PickFilter::default());
    ws.cur.camera.fit(&ws.cur.scene, rect());
    // 板の上の面の原点の角(60×40×10 の板。ポケットや穴から離れた角)
    let corner = at(&ws, [0.0, 0.0, 10.0]);
    let Some(Hit::Vertex(v)) = ws.hit_at(rect(), corner, ws.win.pick_filter) else {
        panic!("角の上は頂点を拾う");
    };
    assert!(v.vertex.contains("end"), "上の面を含む名前: {}", v.vertex);
    ws.pick_hit(Some(Hit::Vertex(v.clone())), false, false);
    let info = ws.selection_info().expect("数値を出す");
    assert_eq!(info, "頂点 | X: 0.000 Y: 0.000 Z: 10.000 mm");
    // 角から離れた辺の真ん中は辺(手前の長い辺)
    let mid = at(&ws, [30.0, 0.0, 10.0]);
    let Some(Hit::Edge(e)) = ws.hit_at(rect(), mid, ws.win.pick_filter) else {
        panic!("辺の上は辺を拾う");
    };
    // Shift で足すと、頂点と辺を一緒に選べる
    ws.pick_hit(Some(Hit::Edge(e.clone())), true, false);
    assert_eq!(ws.picked_vertices().len(), 1);
    assert_eq!(ws.picked_edges().len(), 1);
    let info = ws.selection_info().unwrap();
    assert_eq!(info, "1 頂点 1 辺 | 最小距離: 0.000 mm", "角は辺の端");
    // 面の真ん中は面。Shift で足すと 3 種類
    let face = at(&ws, [50.0, 35.0, 10.0]);
    let hit = ws.hit_at(rect(), face, ws.win.pick_filter);
    assert!(matches!(hit, Some(Hit::Face(_))), "{hit:?}");
    ws.pick_hit(hit, true, false);
    assert_eq!(ws.picked_refs().len(), 3);
    assert_eq!(ws.selection_info().unwrap(), "1 頂点 1 辺 1 面");
    // 修飾キーなしのクリックは、その 1 つだけにする(ほかの種類も外す)
    ws.pick_hit(Some(Hit::Edge(e)), false, false);
    assert!(ws.picked_vertices().is_empty() && ws.picked().is_none());
    let info = ws.selection_info().unwrap();
    assert_eq!(info, "辺 | 長さ: 60.000 mm");
    // 面だけにすると、角の上でも面を拾う
    ws.set_pick_filter(PickFilter::FACE);
    assert!(ws.picked_edges().is_empty(), "選べなくした種類の選びは外す");
    assert!(matches!(
        ws.hit_at(rect(), corner, ws.win.pick_filter),
        Some(Hit::Face(_)) | None
    ));
    // ボディを選ぶと、頂点・辺・面とは混ぜない
    ws.toggle_pick_kind(3);
    assert!(ws.win.pick_filter.body && !ws.win.pick_filter.face_on());
    ws.toggle_pick_kind(0);
    assert!(
        !ws.win.pick_filter.body && ws.win.pick_filter.vertex_on() && ws.win.pick_filter.face_on()
    );
}

#[test]
fn the_boolean_takes_its_target_and_tools_by_clicking_bodies() {
    // 対象と道具を 3D のビューのクリックで入れ、道具はいくつでも入る
    let (mut ws, [plate, a, b]) = plate_and_posts();
    let full = 30.0 * 20.0 * 10.0 + 2.0 * 4.0 * 4.0 * 20.0;
    assert!((volume(&ws) - full).abs() < 1e-3);
    let id = ws.add_described(kmb_cad_boolean::BOOLEAN).expect("足せる");
    assert_eq!(ws.creating(), Some(id));
    // 何も選んでいなければ、前と同じく新しい 2 つで埋め(計算できる形のまま)、対象の欄が受ける
    let data = ws.cur.work.doc().data().feature(id).unwrap().data.clone();
    assert_eq!(data["target"], serde_json::json!(a.0), "{data}");
    assert_eq!(data["tool"], serde_json::json!(b.0), "{data}");
    assert!(ws.is_pick_field(id, "target"));
    // 受けている間は、ブーリアンの手前の形(3 つのボディ)を出す
    ws.sync_scene_stage();
    assert_eq!(ws.cur.scene.bodies.len(), 3);
    ws.cur.camera.fit(&ws.cur.scene, rect());
    ws.cur.camera.set_view(-90.0, 90.0);
    ws.cur.camera.fit(&ws.cur.scene, rect());
    // 板をクリック: 対象が板に替わり、道具の欄へ進む
    ws.pick_field_click(rect(), at(&ws, [15.0, 17.0, 10.0]), false);
    let data = ws.cur.work.doc().data().feature(id).unwrap().data.clone();
    assert_eq!(data["target"], serde_json::json!(plate.0), "{data}");
    assert!(ws.is_pick_field(id, "tool"), "埋まったら次の欄");
    // 対象の板は道具に入れない。仮に埋めた道具はそのまま
    ws.pick_field_click(rect(), at(&ws, [15.0, 17.0, 10.0]), false);
    let data = ws.cur.work.doc().data().feature(id).unwrap().data.clone();
    assert_eq!(data["tool"], serde_json::json!(b.0), "{data}");
    assert!(
        ws.win
            .message
            .as_deref()
            .unwrap_or("")
            .contains("「ターゲットボディ」に入っています")
    );
    // 候補で埋めた道具は、最初のクリックで選び直す。2 本目は足す
    ws.pick_field_click(rect(), at(&ws, [7.0, 7.0, 20.0]), false);
    ws.pick_field_click(rect(), at(&ws, [22.0, 7.0, 20.0]), false);
    let data = ws.cur.work.doc().data().feature(id).unwrap().data.clone();
    assert_eq!(data["tool"], serde_json::json!([a.0, b.0]), "{data}");
    // もう一度押すと外し、また押すと足す
    ws.pick_field_click(rect(), at(&ws, [7.0, 7.0, 20.0]), false);
    let data = ws.cur.work.doc().data().feature(id).unwrap().data.clone();
    assert_eq!(data["tool"], serde_json::json!(b.0), "{data}");
    ws.pick_field_click(rect(), at(&ws, [7.0, 7.0, 20.0]), false);
    // 欄の中身の名前
    let tools = kmb_core::Field::pick("tool", "", &[], 0, None)
        .refs(&ws.cur.work.doc().data().feature(id).unwrap().data);
    assert_eq!(tools, vec![Ref::Body(b), Ref::Body(a)]);
    assert_eq!(ws.ref_label(&Ref::Body(a)), "ボディ(柱A)");
    // Esc の 1 段で受けるのをやめ、最後までの形(切り取った板)に戻る
    assert!(ws.leave_pick_field());
    ws.sync_scene_stage();
    assert_eq!(ws.cur.scene.bodies.len(), 1);
    let cut = 30.0 * 20.0 * 10.0 - 2.0 * 4.0 * 4.0 * 10.0;
    assert!((volume(&ws) - cut).abs() < 1e-3, "{}", volume(&ws));
    assert_eq!(ws.creating(), Some(id), "作るのは続ける");
    ws.finish_edit();
    assert_eq!(ws.report().error_count(), 0);
}

#[test]
fn a_preselected_face_gives_its_body_to_the_boolean() {
    let (mut ws, [plate, a, b]) = plate_and_posts();
    ws.cur.camera.fit(&ws.cur.scene, rect());
    // 柱A の面を先に選んでからブーリアン: 対象は柱A、道具は残りのいちばん新しい柱B
    ws.set_picked(Some(kmb_core::FacePick {
        feature: a,
        face: format!("f{}:end", a.0),
    }));
    let id = ws.add_described(kmb_cad_boolean::BOOLEAN).unwrap();
    let data = ws.cur.work.doc().data().feature(id).unwrap().data.clone();
    assert_eq!(data["target"], serde_json::json!(a.0), "{data}");
    assert_eq!(data["tool"], serde_json::json!(b.0), "{data}");
    assert!(ws.picked().is_none(), "先に選んだ物は欄へ移す");
    // 一覧からも選べる(▾)。対象を板にし、道具に柱A を足すのはデータの書き換えで確かめる
    let mut data = data;
    data["target"] = serde_json::json!(plate.0);
    data["tool"] = serde_json::json!([a.0, b.0]);
    ws.set_feature_data(id, data);
    assert_eq!(ws.report().error_count(), 0, "{:?}", ws.report());
    let cut = 30.0 * 20.0 * 10.0 - 2.0 * 4.0 * 4.0 * 10.0;
    assert!((volume(&ws) - cut).abs() < 1e-3);
}

#[test]
fn a_pattern_takes_bodies_from_the_browser_and_fills_an_empty_field_before_it() {
    // 1 つの板と柱で順に確かめる(足した途中のパターンはキャンセルで消す):
    // - ブラウザでボディの行を押してからのパターンはボディのパターン、タイムラインで選んだ機能は
    //   機能のパターン(元 a_body_picked_in_the_browser_makes_a_body_pattern)
    // - 欄を埋めると、前に空いた欄へも移る(元 filling_a_field_moves_to_an_empty_field_before_it_too)
    //
    // 不具合(2026-10-19 の写真撮りで見つけた): ブラウザでボディの行を押してから直線のパターンを
    // 押しても、種類が「ボディ」にならなかった(3D のビューで選んだ時はなる)
    let (mut ws, [_, a, _]) = plate_and_posts();
    ws.select_body_row(a, false);
    let id = ws.add_described(kmb_cad_pattern::LINEAR_PATTERN).unwrap();
    let data = ws.cur.work.doc().data().feature(id).unwrap().data.clone();
    assert_eq!(data["kind"], "bodies", "{data}");
    assert_eq!(data["bodies"], serde_json::json!(a.0), "{data}");
    let n = ws.cur.work.doc().data().features.len() - 1;
    ws.cancel_edit();
    assert_eq!(
        ws.cur.work.doc().data().features.len(),
        n,
        "途中のパターンは消える"
    );
    assert!(ws.cur.work.doc().data().feature(id).is_none());
    // タイムラインで選んだ機能は機能のパターン
    ws.select(a);
    let id = ws.add_described(kmb_cad_pattern::LINEAR_PATTERN).unwrap();
    let data = ws.cur.work.doc().data().feature(id).unwrap().data.clone();
    assert_ne!(data["kind"], "bodies", "{data}");
    ws.cancel_edit();
    assert_eq!(ws.cur.work.doc().data().features.len(), n);
    assert!(ws.selection().is_empty());

    // 埋まった欄より前に空いた欄が残っていれば、そこへ戻って受ける(X1 の 3)
    let id = ws
        .add_described(kmb_cad_pattern::LINEAR_PATTERN)
        .expect("足せる");
    let entry = ws.cur.work.doc().data().feature(id).unwrap().clone();
    let mut d = entry.data.clone();
    kmb_core::Field::pick("feature", "", &[], 0, None).set_refs(&mut d, &[]);
    ws.set_panel_data(&entry, d);
    ws.toggle_pick_field(id, "body");
    assert!(ws.is_pick_field(id, "body"));
    ws.sync_scene_stage();
    ws.cur.camera.fit(&ws.cur.scene, rect());
    ws.cur.camera.set_view(-90.0, 90.0);
    ws.cur.camera.fit(&ws.cur.scene, rect());
    // 柱 A の上の面: 足すボディに柱 A が入り、前の空いた「並べるフィーチャ」へ移る
    ws.pick_field_click(rect(), at(&ws, [7.0, 7.0, 20.0]), false);
    let data = ws.cur.work.doc().data().feature(id).unwrap().data.clone();
    assert_eq!(data["body"], serde_json::json!(a.0), "{data}");
    assert!(ws.is_pick_field(id, "feature"), "前の空いた欄へ戻る");
}

#[test]
fn enter_in_a_value_field_finishes_editing_too() {
    // 2026-10-18 Kie: 値の欄の Enter はコマンドを決める。直している途中も
    let part = kmb_samples::reference_part();
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui_state(
            |ui, ws: &mut Workspace| ws.ui(ui),
            Workspace::new(part.doc, None, reg()),
        );
    h.run();
    let ext = part.plate;
    h.state_mut().open_edit(ext);
    h.run();
    assert!(h.state().is_editing(ext));
    // 押し出しの距離の欄にキーボードを移し、打って Enter
    let id = crate::doc_id(&h.ctx, ("field", ext, "distance")).with("edit");
    h.ctx.memory_mut(|m| m.request_focus(id));
    h.run();
    h.event(egui::Event::Key {
        key: egui::Key::A,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::COMMAND,
    });
    h.event(egui::Event::Text("12".into()));
    h.key_press(egui::Key::Enter);
    h.run();
    h.state_mut().wait();
    let ws = h.state();
    assert!(!ws.is_editing(ext), "Enter で直すのを終える");
    assert_eq!(
        ws.document().data().feature(ext).unwrap().data["distance"],
        "12 mm"
    );
}

#[test]
fn a_fillet_gets_a_new_row_from_plus_and_the_next_click() {
    // フィレット 1 つで違う半径を持てる(機能を分けずに済む)
    let mut b = DocBuilder::new();
    let plate = block(&mut b, "板", 0.0, 0.0, 30.0, 20.0, 10.0);
    let mut ws = Workspace::new(b.finish("板"), None, reg());
    ws.cur.camera.fit(&ws.cur.scene, rect());
    ws.cur.camera.set_view(-90.0, 90.0);
    ws.cur.camera.fit(&ws.cur.scene, rect());
    let (front, back) = ([15.0, 0.0, 10.0], [15.0, 20.0, 10.0]);
    let Some(Hit::Edge(e)) = ws.hit_at(rect(), at(&ws, front), PickFilter::EDGE) else {
        panic!("手前の上の辺");
    };
    ws.set_picked_edges(vec![e.clone()]);
    let id = ws
        .start_action(kmb_cad_edge::FILLET_FACE)
        .expect("すぐ足す");
    let rows = |ws: &Workspace| ws.cur.work.doc().data().feature(id).unwrap().data["rows"].clone();
    assert_eq!(rows(&ws).as_array().unwrap().len(), 1);
    // 「+」は新しい行を待つ状態にするだけ(空の行は作らない)
    ws.toggle_pick_field(id, "rows.1.edges");
    assert_eq!(rows(&ws).as_array().unwrap().len(), 1);
    ws.sync_scene_stage();
    // 次のクリックで、前の行の半径のまま 2 行目ができ、続けてその行が受ける
    ws.pick_field_click(rect(), at(&ws, back), false);
    let r = rows(&ws);
    assert_eq!(r.as_array().unwrap().len(), 2, "{r}");
    assert_eq!(r[1]["radius"], r[0]["radius"]);
    assert_eq!(r[1]["edges"]["kind"], "edge", "{r}");
    assert!(ws.is_pick_field(id, "rows.1.edges"));
    // 1 行目の辺は 2 行目に入れない
    ws.pick_field_click(rect(), at(&ws, front), false);
    assert!(
        ws.win
            .message
            .as_deref()
            .unwrap_or("")
            .contains("「選択セット 1」に入っています"),
        "{:?}",
        ws.win.message
    );
    // 2 行目の半径を変えると、その辺だけ大きく丸まる
    let mut data = ws.cur.work.doc().data().feature(id).unwrap().data.clone();
    data["rows"][1]["radius"] = serde_json::json!("3 mm");
    ws.set_feature_data(id, data);
    ws.wait();
    let r0 = rows(&ws)[0]["radius"].as_str().unwrap().replace(" mm", "");
    let r0: f64 = r0.parse().unwrap();
    let removed = (r0 * r0 + 9.0) * (1.0 - std::f64::consts::PI / 4.0) * 30.0;
    let full = 30.0 * 20.0 * 10.0;
    assert!(
        (full - volume(&ws) - removed).abs() < 0.5,
        "{} は {removed} 削るはず",
        volume(&ws)
    );
    // 2 行目の辺をもう一度押して外すと、空になった行は消える
    ws.pick_field_click(rect(), at(&ws, back), false);
    assert_eq!(rows(&ws).as_array().unwrap().len(), 1, "{}", rows(&ws));
    assert!(!ws.is_pick_field(id, "rows.1.edges"));
    let _ = plate;
}

/// 長方形を押し出してボディ `target` に結合・切り取りする。
fn on_body(
    b: &mut DocBuilder,
    name: &str,
    [x, y, w, h, z]: [f64; 5],
    operation: Operation,
    target: FeatureId,
) -> FeatureId {
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let o = s.fixed_point(0.0, 0.0);
    let c = s.point_from(o, &format!("{x} mm"), &format!("{y} mm"), [x, y]);
    s.rectangle(c, &format!("{w} mm"), &format!("{h} mm"), [x, y, w, h]);
    let sk = b.feature(kmb_sketch::SKETCH, &format!("{name}のスケッチ"), s.build());
    b.feature(
        EXTRUDE,
        name,
        Extrude {
            profile: Some(sk),
            distance: format!("{z} mm"),
            operation,
            target: Some(target),
            ..Default::default()
        },
    )
}

#[test]
fn a_pattern_takes_its_features_by_clicking_their_faces() {
    // 「並べるフィーチャ」はいくつでも選べ、前の機能を選んでもボディが 2 つに分かれない
    let mut b = DocBuilder::new();
    let plate = block(&mut b, "板", 0.0, 0.0, 30.0, 20.0, 10.0);
    let boss = on_body(
        &mut b,
        "柱",
        [2.0, 2.0, 4.0, 4.0, 20.0],
        Operation::Join,
        plate,
    );
    let cut = on_body(
        &mut b,
        "角穴",
        [20.0, 12.0, 4.0, 4.0, 10.0],
        Operation::Cut,
        boss,
    );
    let mut ws = Workspace::new(b.finish("板と柱と角穴"), None, reg());
    let id = ws
        .add_described(kmb_cad_pattern::LINEAR_PATTERN)
        .expect("足せる");
    // 何も選んでいなければ、最後の機能(角穴)で仮に埋めて、機能の欄が受ける
    let data = |ws: &Workspace| ws.cur.work.doc().data().feature(id).unwrap().data.clone();
    assert_eq!(data(&ws)["feature"], serde_json::json!(cut.0));
    assert!(ws.is_pick_field(id, "feature"));
    ws.sync_scene_stage();
    ws.cur.camera.fit(&ws.cur.scene, rect());
    ws.cur.camera.set_view(-90.0, 90.0);
    ws.cur.camera.fit(&ws.cur.scene, rect());
    // 柱の上の面: 仮の角穴に替わって柱が入り、足すボディはそのボディ(角穴の後)になる
    ws.pick_field_click(rect(), at(&ws, [4.0, 4.0, 20.0]), false);
    assert_eq!(
        data(&ws)["feature"],
        serde_json::json!(boss.0),
        "{} {:?}",
        data(&ws),
        ws.win.message
    );
    assert_eq!(data(&ws)["body"], serde_json::json!(cut.0), "{}", data(&ws));
    assert!(
        ws.is_pick_field(id, "feature"),
        "いくつでも受けるので続ける"
    );
    // 板の上の面で板も足す。タイムラインの順に並ぶ。もう一度押すと外す
    ws.pick_field_click(rect(), at(&ws, [15.0, 8.0, 10.0]), false);
    assert_eq!(data(&ws)["feature"], serde_json::json!([plate.0, boss.0]));
    ws.pick_field_click(rect(), at(&ws, [15.0, 8.0, 10.0]), false);
    assert_eq!(data(&ws)["feature"], serde_json::json!(boss.0));
    // 名前は機能の名前
    assert_eq!(
        ws.field_ref_label(&[kmb_core::RefKind::Feature], &Ref::Body(boss)),
        "柱"
    );
    // 柱を x へ 10 mm で 2 つ: ボディは 1 つのまま、角穴も残る
    let mut d = data(&ws);
    d["spacing"] = serde_json::json!("10 mm");
    d["count"] = serde_json::json!("2");
    d["direction"] = serde_json::json!([1.0, 0.0, 0.0]);
    let entry = ws.cur.work.doc().data().feature(id).unwrap().clone();
    ws.set_panel_data(&entry, d);
    assert!(ws.leave_pick_field());
    ws.finish_edit();
    ws.sync_scene_stage();
    assert_eq!(ws.visible_bodies().len(), 1);
    let v = 30.0 * 20.0 * 10.0 + 2.0 * 4.0 * 4.0 * 10.0 - 4.0 * 4.0 * 10.0;
    assert!((volume(&ws) - v).abs() < 1e-3, "{}", volume(&ws));
}

#[test]
fn a_field_keeps_its_own_pick_kinds_and_the_body_mode_does_not_leak_in_or_out() {
    // 左上を「ボディ」にしたままでも、シェルの「面/ボディ」の欄で面を押せば面が入る(X1 の 1)。
    // 欄の間に左上で替えた種類は、欄を出た後の普段の選択に残さない(X1 の 2)
    let (mut ws, [plate, ..]) = plate_and_posts();
    ws.toggle_pick_kind(3);
    assert!(ws.win.pick_filter.body, "普段はボディを選ぶ");
    let id = ws.add_described(kmb_cad_edge::SHELL).expect("足せる");
    assert!(ws.is_pick_field(id, "faces"));
    // 欄の種類は面から。頂点・辺は受けないので押せず、ボディは押せる
    let kinds = |ws: &Workspace| ws.pick_filter_items().map(|i| (i.1, i.2));
    assert_eq!(
        kinds(&ws),
        [(false, false), (false, false), (true, true), (false, true)]
    );
    ws.toggle_pick_kind(0);
    assert_eq!(
        kinds(&ws)[0],
        (false, false),
        "受けない種類は押しても替わらない"
    );
    ws.sync_scene_stage();
    ws.cur.camera.fit(&ws.cur.scene, rect());
    ws.cur.camera.set_view(-90.0, 90.0);
    ws.cur.camera.fit(&ws.cur.scene, rect());
    let refs = |ws: &Workspace| {
        kmb_core::Field::pick("faces", "", &[], 0, None)
            .refs(&ws.cur.work.doc().data().feature(id).unwrap().data)
    };
    let top = at(&ws, [15.0, 17.0, 10.0]);
    ws.pick_field_click(rect(), top, false);
    assert!(
        matches!(refs(&ws)[..], [Ref::Face { feature, .. }] if feature == plate),
        "普段がボディでも、欄では面が入る: {:?}",
        refs(&ws)
    );
    // 欄で「ボディ」にすると、面を押してもそのボディ(M21)
    ws.toggle_pick_kind(3);
    assert_eq!(kinds(&ws)[3], (true, true));
    ws.pick_field_click(rect(), top, false);
    assert_eq!(refs(&ws), vec![Ref::Body(plate)]);
    // 欄で替えても普段の種類は替わらず、欄を出ると普段の種類(ボディ)に戻る
    assert!(ws.win.pick_filter.body && !ws.win.pick_filter.face_on());
    assert!(ws.leave_pick_field());
    assert_eq!(kinds(&ws)[3], (true, true));
    ws.toggle_pick_kind(2);
    assert_eq!(
        ws.win.pick_filter,
        PickFilter::default(),
        "普段の種類は普段どおり替わる"
    );
    // 欄に戻ると、欄の初めの種類(面)から
    ws.toggle_pick_field(id, "faces");
    assert_eq!(kinds(&ws)[2], (true, true));
    assert_eq!(kinds(&ws)[3], (false, true));
}

/// 30×20×10 の板に、角を半径 3 で丸めた 20×10 の深さ 3 のポケット。
fn rounded_pocket_plate() -> Workspace {
    let mut b = DocBuilder::new();
    let plate = block(&mut b, "板", 0.0, 0.0, 30.0, 20.0, 10.0);
    let mut s = SketchBuilder::new(SketchPlane::face(plate, format!("f{}:end", plate.0)));
    s.rounded_rectangle(5.0, 5.0, 20.0, 10.0, 3.0);
    let sk = b.feature(kmb_sketch::SKETCH, "ポケットの輪郭", s.build());
    b.feature(
        EXTRUDE,
        "ポケット",
        Extrude {
            profile: Some(sk),
            regions: Vec::new(),
            distance: "3 mm".into(),
            direction: Direction::Reverse,
            distance2: None,
            extent: Default::default(),
            to_face: None,
            to_offset: None,
            operation: Operation::Cut,
            target: Some(plate),
            ..Default::default()
        },
    );
    let mut ws = Workspace::new(b.finish("角を丸めたポケット"), None, reg());
    ws.cur.camera.fit(&ws.cur.scene, rect());
    ws
}

fn clicks(p: Pos2, double: bool) -> crate::view3d::ViewEvents {
    crate::view3d::ViewEvents {
        click: Some(p),
        double_click: double.then_some(p),
        ..Default::default()
    }
}

#[test]
fn double_clicking_an_edge_picks_its_chain_and_the_fillet_tool_takes_it_on_one_click() {
    // 1 つの角を丸めたポケットの板で順に確かめる:
    // - 辺のダブルクリックで接するつながりを選ぶ(元 double_clicking_an_edge_picks_its_tangent_chain)
    // - フィレットの道具では 1 回のクリックでつながりを取り、ダブルクリックで外さない
    //   (元 in_the_fillet_tool_one_click_takes_the_chain_and_a_double_click_does_not_undo_it)
    //
    // 3D の辺のダブルクリックでつながった輪を選ぶ(8 本を 1 本ずつ押さずに済む)
    let mut ws = rounded_pocket_plate();
    let rim = at(&ws, [15.0, 5.0, 10.0]);
    let none = egui::Modifiers::NONE;
    ws.pick_events_with(&clicks(rim, false), rect(), none);
    assert_eq!(ws.picked_edges().len(), 1, "1 回目は 1 本");
    assert_eq!(
        ws.shown_edges().len(),
        1,
        "ツールの外では選んだ物だけを光らせる"
    );
    ws.pick_events_with(&clicks(rim, true), rect(), none);
    assert_eq!(
        ws.picked_edges().len(),
        8,
        "上の縁の直線 4 本と弧 4 本: {:?}",
        ws.picked_edges()
    );
    assert!(
        ws.picked_edges()[0].faces[1].ends_with(":side:e13"),
        "押した辺が先頭"
    );
    // 板の外の縁(直角につながる 4 本)は輪にならない。Shift で 1 本だけ足す
    let outer = at(&ws, [15.0, 0.0, 10.0]);
    let shift = egui::Modifiers::SHIFT;
    ws.pick_events_with(&clicks(outer, false), rect(), shift);
    ws.pick_events_with(&clicks(outer, true), rect(), shift);
    assert_eq!(ws.picked_edges().len(), 9);
    // Ctrl のダブルクリック: 1 回目で外した辺の輪を外す
    let ctrl = egui::Modifiers::COMMAND;
    ws.pick_events_with(&clicks(rim, false), rect(), ctrl);
    ws.pick_events_with(&clicks(rim, true), rect(), ctrl);
    assert_eq!(ws.picked_edges().len(), 1, "外の縁の 1 本が残る");

    // フィレットの道具: 選んだ辺が残っていると辺の手順を飛ばすので、外してから
    // フィレットの辺を選ぶ手順のダブルクリックで、足してすぐ外して何も残らない、とならない
    ws.set_picked_edges(vec![]);
    let before = volume(&ws);
    ws.start_action(kmb_cad_edge::FILLET_FACE);
    let rim = at(&ws, [15.0, 5.0, 10.0]);
    // Ctrl で押す(修飾なしの 1 回目は、足してすぐ作る。Kie 2026-10-21)
    let ctrl_clicks = |double: bool| crate::view3d::ViewEvents {
        add: true,
        ..clicks(rim, double)
    };
    ws.gesture_events(&ctrl_clicks(false), rect());
    ws.gesture_events(&ctrl_clicks(true), rect());
    assert_eq!(ws.picked_edges().len(), 1, "種の 1 本だけを持つ");
    assert_eq!(ws.shown_edges().len(), 8, "入るつながる辺も光らせる");
    ws.gesture_enter();
    ws.wait();
    let id = ws.creating().expect("作っている途中");
    let data = ws.document().data().feature(id).unwrap().data.clone();
    assert_eq!(
        data["chain"], true,
        "画面から作るものは、つながる辺も入れる: {data}"
    );
    ws.finish_edit();
    ws.wait();
    assert_eq!(ws.report().error_count(), 0, "{:?}", ws.report());
    // 半径 1 の丸めで縁の全部(直線 32 mm と半径 3 の四分円 4 つ)を削る
    use std::f64::consts::PI;
    let area = 1.0 - PI / 4.0;
    let d = (10.0 - 3.0 * PI) / (12.0 - 3.0 * PI);
    let want = area * 2.0 * (14.0 + 4.0) + area * 2.0 * PI * (3.0 + d);
    let removed = before - volume(&ws);
    assert!((removed - want).abs() < 0.1, "{removed} 理論 {want}");
}

/// 30×20×10 の板(上面の (15, 10) に深さ 3 mm の穴)と、横の低い台(x 40〜60、高さ 4)。
/// `join` なら台を板に足して 1 つのボディにする(穴はそのボディに掘る)。
fn plate_with_hole_and_step(join: bool) -> (Workspace, FeatureId, FeatureId) {
    let mut b = DocBuilder::new();
    let plate = block(&mut b, "板", 0.0, 0.0, 30.0, 20.0, 10.0);
    let step = block(&mut b, "台", 40.0, 0.0, 20.0, 20.0, 4.0);
    let target = if join {
        b.feature(
            kmb_cad_boolean::BOOLEAN,
            "合わせる",
            serde_json::json!({"target": plate.0, "tool": step.0, "operation": "join"}),
        )
    } else {
        plate
    };
    let mut hs = SketchBuilder::new(SketchPlane::face(plate, format!("f{}:end", plate.0)));
    let o = hs.fixed_point(0.0, 0.0);
    let c = hs.point_from(o, "15 mm", "10 mm", [15.0, 10.0]);
    let sk = b.feature(kmb_sketch::SKETCH, "穴の位置", hs.build());
    let hole = b.feature(
        kmb_cad_hole::HOLE,
        "穴",
        kmb_cad_hole::Hole {
            points: vec![kmb_core::PointId(c.0)],
            diameter: "4 mm".into(),
            depth: Some("3 mm".into()),
            ..kmb_cad_hole::Hole::new(target, sk)
        },
    );
    let mut ws = Workspace::new(b.finish("穴と台"), None, reg());
    assert_eq!(ws.report().error_count(), 0, "{:?}", ws.report());
    ws.cur.camera.fit(&ws.cur.scene, rect());
    (ws, hole, step)
}

#[test]
fn a_hole_set_to_stop_at_an_object_takes_the_clicked_face_or_vertex() {
    // 不具合(2026-10-19 の写真撮りで見つけた): 穴の範囲を「対象まで」にした直後に面を押しても
    // 選べなかった。人の手順と同じく、欄を替えてから 3D のビューで台の上面(z = 4)をクリックする
    let (mut ws, hole, step) = plate_with_hole_and_step(false);
    ws.open_edit(hole);
    let entry = ws.cur.work.doc().data().feature(hole).unwrap().clone();
    let mut data = entry.data.clone();
    data["extent"] = serde_json::json!("to_face");
    ws.set_panel_data(&entry, data);
    let h = ws.cur.gesture.as_ref().and_then(|g| g.current()).cloned();
    assert!(
        h.as_ref()
            .is_some_and(|h| matches!(h.gesture, kmb_core::Gesture::Face)),
        "面を選ぶ手順に入る: {h:?}"
    );
    ws.gesture_events(&clicks(at(&ws, [50.0, 10.0, 4.0]), false), rect());
    assert!(ws.cur.gesture.is_none(), "{:?}", ws.win.message);
    let d = ws.cur.work.doc().data().feature(hole).unwrap().data.clone();
    assert_eq!(d["to_face"]["face"], format!("f{}:end", step.0), "{d}");
    assert_eq!(ws.report().error_count(), 0, "{:?}", ws.report());
    // 穴は台の上面の高さ(z = 4)まで: 深さ 6 mm
    // (直している間のビューは穴より前の形と穴の板だけを数える)
    let want = 30.0 * 20.0 * 10.0 - std::f64::consts::PI * 4.0 * 6.0;
    assert!(
        (volume(&ws) - want).abs() < 0.05,
        "{} vs {want}",
        volume(&ws)
    );

    // 頂点も選べる(台の上の角 (40, 0, 4) の近くを押す): 深さは同じ 6 mm
    let (mut ws, hole, _) = plate_with_hole_and_step(false);
    ws.open_edit(hole);
    let entry = ws.cur.work.doc().data().feature(hole).unwrap().clone();
    let mut data = entry.data.clone();
    data["extent"] = serde_json::json!("to_face");
    ws.set_panel_data(&entry, data);
    let p = at(&ws, [40.0, 0.0, 4.0]);
    ws.gesture_events(&clicks(p + egui::vec2(2.0, -1.0), false), rect());
    assert!(ws.cur.gesture.is_none(), "{:?}", ws.win.message);
    let d = ws.cur.work.doc().data().feature(hole).unwrap().data.clone();
    assert!(d["to_face"]["vertex"].is_string(), "{d}");
    // 「止める頂点」の欄は、頂点を面と書かない(写真撮りで「面(押し出し1)」と出ていた)
    let shown = crate::feature_ui::info_text(&ws, Some(&d["to_face"]["vertex"]));
    assert!(shown.starts_with("頂点("), "{shown}");
    assert_eq!(ws.report().error_count(), 0, "{:?}", ws.report());
    assert!(
        (volume(&ws) - want).abs() < 0.05,
        "{} vs {want}",
        volume(&ws)
    );
}

#[test]
fn a_hole_waiting_for_its_stop_face_takes_a_face_of_its_own_body() {
    // 不具合(2026-10-19 の写真撮りで見つけた): 範囲を「対象まで」にした直後は穴の計算が失敗して
    // 前の形を出すが、押した面には穴の番号が付き、穴に出力が無いので、どの面も選べなかった
    let (mut ws, hole, step) = plate_with_hole_and_step(true);
    ws.open_edit(hole);
    let entry = ws.cur.work.doc().data().feature(hole).unwrap().clone();
    let mut data = entry.data.clone();
    data["extent"] = serde_json::json!("to_face");
    ws.set_panel_data(&entry, data);
    ws.gesture_events(&clicks(at(&ws, [50.0, 10.0, 4.0]), false), rect());
    assert!(ws.cur.gesture.is_none(), "{:?}", ws.win.message);
    let d = ws.cur.work.doc().data().feature(hole).unwrap().data.clone();
    assert_eq!(d["to_face"]["face"], format!("f{}:end", step.0), "{d}");
    assert_eq!(ws.report().error_count(), 0, "{:?}", ws.report());
}

#[test]
fn the_fillet_tool_takes_a_face_and_rounds_the_edges_around_it() {
    // Kie 2026-10-21: フィレット・面取りで面も選べるように(面のまわりの辺が対象)。
    // 作る手順で辺だけでなく面も拾う(先に平らな面を選んでおかなくてよい)
    let (mut ws, [plate, _, _]) = plate_and_posts();
    ws.wait();
    ws.cur.camera.fit(&ws.cur.scene, rect());
    ws.cur.camera.set_view(-90.0, 90.0);
    ws.cur.camera.fit(&ws.cur.scene, rect());
    let before = volume(&ws);
    ws.clear_selection();
    ws.start_action(kmb_cad_edge::FILLET_FACE);
    assert!(ws.cur.gesture.is_some(), "辺か面を選ぶ手順");
    // 板の上面の、柱から離れた所を 1 回クリック: そのまま作る
    ws.gesture_events(&clicks(at(&ws, [15.0, 15.0, 10.0]), false), rect());
    assert!(ws.cur.gesture.is_none(), "{:?}", ws.win.message);
    let id = ws.creating().expect("作っている途中");
    let data = ws.document().data().feature(id).unwrap().data.clone();
    assert_eq!(
        data["rows"][0]["edges"],
        serde_json::json!({"kind": "face", "feature": plate.0, "face": format!("f{}:end", plate.0)}),
        "{data}"
    );
    ws.finish_edit();
    ws.wait();
    assert_eq!(ws.report().error_count(), 0, "{:?}", ws.report());
    // 上面のまわりの 4 本(周 100 mm)を半径 1 で丸めた分だけ減る(角の球の分は小さい)
    let removed = before - volume(&ws);
    let want = (1.0 - std::f64::consts::PI / 4.0) * 100.0;
    assert!((removed - want).abs() < 0.5, "{removed} 理論 {want}");
}

#[test]
fn a_preselected_body_goes_to_the_target_and_the_tool_field_takes_the_clicks() {
    // 先に選んだ物で埋まった欄は飛ばし、空いた欄から受ける
    let (mut ws, [plate, _, _]) = plate_and_posts();
    ws.wait();
    ws.set_pick_filter(PickFilter {
        body: true,
        ..PickFilter::default()
    });
    ws.select(plate);
    let id = ws.add_described(kmb_cad_boolean::BOOLEAN).expect("足せる");
    let data = ws.cur.work.doc().data().feature(id).unwrap().data.clone();
    assert_eq!(data["target"], serde_json::json!(plate.0), "{data}");
    assert!(ws.is_pick_field(id, "tool"), "ツールの欄から受ける");
}

/// ボディの面に置いたスケッチから「穴」を足すと、そのボディに開ける。
/// 対象の欄は面のボディで埋める(直前の候補(いちばん新しいボディ)で埋めると、別のボディを
/// 対象にして、後ろにあるボディに穴が開くか、何も削れずに止まる)。
#[test]
fn a_hole_from_a_sketch_on_a_face_drills_that_body() {
    let mut b = DocBuilder::new();
    let plate = block(&mut b, "板", 0.0, 0.0, 30.0, 20.0, 10.0);
    let other = block(&mut b, "別の箱", 40.0, 0.0, 20.0, 20.0, 10.0);
    // 穴のスケッチの後で板に足す出っ張り(対象は板の今の形 = この結合にする)
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let o = s.fixed_point(0.0, 0.0);
    s.rectangle(o, "5 mm", "5 mm", [0.0, 0.0, 5.0, 5.0]);
    let lump = b.feature(kmb_sketch::SKETCH, "出っ張りのスケッチ", s.build());
    let mut s = SketchBuilder::new(SketchPlane::face(plate, format!("f{}:end", plate.0)));
    s.point(15.0, 10.0);
    let sketch = b.feature(kmb_sketch::SKETCH, "穴の位置", s.build());
    let joined = b.feature(
        EXTRUDE,
        "出っ張り",
        Extrude {
            profile: Some(lump),
            distance: "15 mm".into(),
            operation: Operation::Join,
            target: Some(plate),
            ..Default::default()
        },
    );
    let mut ws = Workspace::new(b.finish("板と箱"), None, reg());
    ws.wait();
    let before = volume(&ws);
    let id = ws.add_described(kmb_cad_hole::HOLE).expect("穴を足せる");
    let data = ws.document().data().feature(id).unwrap().data.clone();
    assert_eq!(data["sketch"], serde_json::json!(sketch.0), "{data}");
    assert_eq!(
        data["target"],
        serde_json::json!(joined.0),
        "面のボディの今の形(別の箱 {other:?} でなく)"
    );
    ws.finish_edit();
    ws.wait();
    assert_eq!(ws.report().error_count(), 0, "{:?}", ws.report());
    // 径 5 の貫通穴の分だけ減る
    let removed = before - volume(&ws);
    let want = std::f64::consts::PI * 2.5 * 2.5 * 10.0;
    assert!((removed - want).abs() < 0.5, "{removed} 理論 {want}");
}

/// 面のスケッチを押し出すと、その面のボディに結合する(後で作った別のボディでなく)。
/// 穴の対象と同じ決まり: 「最後に見えるボディ」ではなく、面のボディに結合する。
#[test]
fn extruding_a_sketch_on_a_face_joins_that_body() {
    let mut b = DocBuilder::new();
    let plate = block(&mut b, "板", 0.0, 0.0, 30.0, 20.0, 10.0);
    let mut s = SketchBuilder::new(SketchPlane::face(plate, format!("f{}:end", plate.0)));
    let o = s.fixed_point(0.0, 0.0);
    let c = s.point_from(o, "10 mm", "5 mm", [10.0, 5.0]);
    s.rectangle(c, "5 mm", "5 mm", [10.0, 5.0, 5.0, 5.0]);
    let sketch = b.feature(kmb_sketch::SKETCH, "板の上のスケッチ", s.build());
    let other = block(&mut b, "別の箱", 40.0, 0.0, 20.0, 20.0, 10.0);
    let mut ws = Workspace::new(b.finish("板と箱"), None, reg());
    ws.wait();
    assert_eq!(ws.report().error_count(), 0, "{:?}", ws.report());
    ws.select(sketch);
    let id = ws
        .run_action(kmb_cad_extrude::EXTRUDE_SKETCH)
        .expect("押し出しを足せる");
    let data = ws.document().data().feature(id).unwrap().data.clone();
    assert_eq!(
        data["target"],
        serde_json::json!(plate.0),
        "面のボディ(別の箱 {other:?} でなく): {data}"
    );
    assert_eq!(data["operation"], serde_json::json!("join"), "{data}");
}

/// 面・辺・頂点を選ぶと機能の選びが外れ、Delete で機能が消えない(Kie 2026-10-22。開いた直後に
/// 選ばれたままの最後の機能を、面を選んで Delete を押した時に消さない)
#[test]
fn picking_a_face_drops_the_feature_selection_so_delete_keeps_the_feature() {
    let part = kmb_samples::reference_part();
    let mut ws = Workspace::new(part.doc, None, reg());
    ws.cur.camera.fit(&ws.cur.scene, rect());
    let last = ws.document().data().features.last().unwrap().id;
    ws.select(last);
    assert_eq!(ws.selection(), vec![last]);
    let hit = ws.hit_at(rect(), at(&ws, [50.0, 35.0, 10.0]), ws.win.pick_filter);
    assert!(matches!(hit, Some(Hit::Face(_))), "{hit:?}");
    ws.pick_hit(hit, false, false);
    assert!(ws.selection().is_empty(), "面を選ぶと機能の選びは外れる");
    assert!(ws.picked().is_some());
    let n = ws.document().data().features.len();
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h.key_press(egui::Key::Delete);
    h.run();
    assert_eq!(
        h.state().document().data().features.len(),
        n,
        "機能は消えない"
    );
}

/// Shift で範囲を選び直しても、起点は最初に押した物のまま(範囲だけが替わる)
#[test]
fn shift_ranges_keep_the_first_clicked_row_as_the_anchor() {
    let (mut ws, [e1, e2, e3]) = plate_and_posts();
    ws.select(e3);
    ws.select_range(e2);
    assert_eq!(ws.selection().len(), 3, "e2 から e3 まで(間のスケッチも)");
    ws.select_range(e1);
    let sel = ws.selection();
    assert!(sel.contains(&e1) && sel.contains(&e3), "{sel:?}");
    assert_eq!(sel.len(), 5, "起点 e3 から e1 まで");
    // 起点まで戻すと起点だけ
    ws.select_range(e3);
    assert_eq!(ws.selection(), vec![e3]);
}

#[test]
fn a_body_row_pressed_while_a_field_takes_bodies_goes_into_the_field() {
    // 10-24 h5: 結合を作る途中にブラウザのボディの行を押すと、作っている機能を決めて終えず、
    // 受けている欄へそのボディが入る。ボディを受けない欄なら、何も決めずに理由を出す
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    let (mut ws, [plate, a, _]) = plate_and_posts();
    let id = ws.add_described(kmb_cad_boolean::BOOLEAN).expect("足せる");
    assert!(ws.is_pick_field(id, "target"));
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .with_step_dt(0.05)
        .with_max_steps(40)
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    // 板は最初のボディ(ブラウザの「ボディ1」)
    h.get_by_label("ボディ1").click();
    h.run();
    let ws = h.state_mut();
    assert_eq!(ws.creating(), Some(id), "押しても作っている途中のまま");
    let data = ws.cur.work.doc().data().feature(id).unwrap().data.clone();
    assert_eq!(data["target"], serde_json::json!(plate.0), "{data}");
    assert!(ws.is_pick_field(id, "tool"), "埋まったら次の欄");
    // 最後までの形では対象の行の最後の機能はブーリアン自身。直している機能の手前のボディに読み替える
    let root_a = kmb_core::body_root(ws.cur.work.doc().data(), &ws.win.registry, a);
    assert!(ws.pick_field_body_row(root_a, false));
    let data = ws.cur.work.doc().data().feature(id).unwrap().data.clone();
    assert_eq!(data["tool"], serde_json::json!(a.0), "{data}");
    assert_eq!(ws.creating(), Some(id));
    // 受けている欄が無ければ、前と同じく行を選ぶ(作っている途中の物は決める)
    assert!(ws.leave_pick_field());
    assert!(!ws.pick_field_body_row(plate, false));
}
