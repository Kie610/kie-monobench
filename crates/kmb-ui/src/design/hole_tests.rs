//! 穴とねじの画面の試験: 面 → 中心 → 径のクリック、近い辺からの寸法、吸い付き、欄での種類の替え、ねじの大きさの候補。

use crate::tests::{on_truck, reg};
use crate::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

#[test]
fn the_hole_is_edited_in_fields_made_from_its_descriptor() {
    let part = kmb_samples::reference_part();
    let holes = part.holes;
    let mut ws = Workspace::new(part.doc, None, reg());
    ws.open_edit(holes);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    // 穴には専用の画面のコードがない。記述子の欄だけで出る
    h.get_by_label("直径");
    h.get_by_label("深さ");
    h.get_by_label("詳細(JSON)");
    let before = h.state().document().data().feature(holes).unwrap().data["diameter"].clone();
    assert!(before.is_string(), "{before}");
}

/// 穴の数と、既存の穴のデータ(新しく作る操作で変わっていないかを見る)。
fn holes_of(ws: &Workspace) -> Vec<(FeatureId, serde_json::Value)> {
    ws.document()
        .data()
        .features
        .iter()
        .filter(|f| f.type_id == kmb_cad_hole::HOLE)
        .map(|f| (f.id, f.data.clone()))
        .collect()
}

#[test]
fn the_hole_button_asks_for_a_face_and_never_touches_existing_holes() {
    // ツールバーの「穴」は押しただけでは穴を足さず、面 → 中心 → 径をクリックで決める(押した瞬間に
    // 直前のスケッチ(既存の穴の位置)で足すと、既存の穴を書き換えたように見える)
    let part = kmb_samples::reference_part();
    let (plate, body) = (part.plate, part.hole_pattern);
    let mut ws = Workspace::new(part.doc, None, reg());
    ws.clear_selection();
    let before = holes_of(&ws);
    let n = ws.document().data().features.len();
    ws.run_item("a:cad.hole_on_face");
    let g = ws.cur.gesture.as_ref().expect("クリックで決める流れに入る");
    assert!(matches!(
        g.current().unwrap().gesture,
        kmb_core::Gesture::Face
    ));
    assert_eq!(
        ws.document().data().features.len(),
        n,
        "押しただけでは足さない"
    );
    // 面をクリックした(ビューがすることを直接)
    ws.set_picked(Some(Pick {
        feature: body,
        face: format!("f{}:end", plate.0),
    }));
    assert!(ws.picked_face_is_planar(), "板の上の面は平ら");
    ws.gesture_commit_step(serde_json::Value::String(format!("f{}:end", plate.0)));
    let g = ws.cur.gesture.as_ref().unwrap();
    assert!(matches!(
        g.current().unwrap().gesture,
        kmb_core::Gesture::Point { .. }
    ));
    // 中心と径。径をクリックで決めたら、そのまま穴ができる(深さは後の欄で直す)
    ws.gesture_commit_step(serde_json::json!([10.0, 30.0, 10.0]));
    ws.gesture_commit_step(serde_json::json!(6.0));
    // 径を決めたら作り(もう一度クリックしない)、続けて同じ穴を足す手順に入る
    let id = ws.selected().expect("作った穴を選ぶ");
    assert_eq!(ws.creating(), Some(id), "作っている途中で欄を直せる");
    let g = ws.cur.gesture.as_ref().expect("同じ穴を足す手順に続く");
    assert_eq!(g.action, "cad.hole_add");
    assert!(matches!(
        g.current().unwrap().gesture,
        kmb_core::Gesture::Point { .. }
    ));
    // もう 1 か所クリックすると、同じ機能・同じスケッチに点が増える(機能もスケッチも増えない)
    ws.gesture_commit_step(serde_json::json!([20.0, 30.0, 10.0]));
    assert_eq!(
        ws.document().data().features.len(),
        n + 2,
        "穴とその点のスケッチだけ"
    );
    assert!(ws.cur.gesture.is_some(), "続けて足せる");
    // Enter(値なしで決める)で終える
    ws.gesture_commit_step(serde_json::Value::Null);
    assert!(ws.cur.gesture.is_none(), "Enter で終える");
    assert!(ws.creating().is_none(), "決まる");
    let after = holes_of(&ws);
    assert_eq!(after.len(), before.len() + 1, "穴の機能が 1 つ増える");
    for (h, d) in &before {
        assert_eq!(
            after.iter().find(|(a, _)| a == h).map(|(_, d)| d),
            Some(d),
            "既存の穴は変わらない"
        );
    }
    let new = ws.document().data().feature(id).unwrap();
    assert_eq!(new.data["diameter"], "6 mm");
    // 作った後は選んだだけ(直すのは開いた時だけ)
    assert_eq!(ws.selected(), Some(id));
    assert!(!ws.is_editing(id));
    assert_eq!(ws.report().error_count(), 0);
}

#[test]
fn a_hole_keeps_its_distance_to_the_nearest_plate_edges() {
    // 穴の位置は近い 2 辺からの寸法で持ち、辺はボディから写すので、幅を変えると右の縁からの距離の
    // まま動く(絶対座標で持つと、板の幅を変えても穴がついてこない)
    let part = kmb_samples::reference_part();
    let (plate, body) = (part.plate, part.hole_pattern);
    let mut ws = Workspace::new(part.doc, None, reg());
    ws.clear_selection();
    ws.run_item("a:cad.hole_on_face");
    let face = format!("f{}:end", plate.0);
    ws.set_picked(Some(Pick {
        feature: body,
        face: face.clone(),
    }));
    ws.gesture_commit_step(serde_json::Value::String(face));
    // 右の縁から 8 mm、上の縁から 9 mm
    ws.gesture_commit_step(serde_json::json!([52.0, 31.0, 10.0]));
    ws.gesture_commit_step(serde_json::json!(3.0));
    let hole = ws.selected().expect("作った穴");
    // 続けて足す 2 つ目(右の縁から 8 mm、上の縁から 4 mm)も同じ縁を使う
    ws.gesture_commit_step(serde_json::json!([52.0, 36.0, 10.0]));
    ws.gesture_commit_step(serde_json::Value::Null);
    let data = ws.document().data();
    let sketch = FeatureId(data.feature(hole).unwrap().data["sketch"].as_u64().unwrap());
    let sk = &data.feature(sketch).unwrap().data;
    assert_eq!(
        sk["projections"].as_array().map(|a| a.len()),
        Some(2),
        "{sk}"
    );
    let mut dims: Vec<String> = sk["constraints"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["value"].as_str().unwrap_or_default().to_string())
        .collect();
    dims.sort();
    assert_eq!(dims, ["4 mm", "8 mm", "8 mm", "9 mm"]);
    let second = sk["entities"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["type"] == "point")
        .find(|e| (e["y"].as_f64().unwrap() - 36.0).abs() < 1e-6)
        .and_then(|e| e["id"].as_u64())
        .map(|id| kmb_sketch::EntityId(id as u32))
        .expect("2 つ目の点");
    let center = |ws: &Workspace| {
        let Some(FeatureStatus::Ok { outputs, .. }) = ws.report().feature(sketch) else {
            panic!("穴の位置のスケッチが計算できる");
        };
        let sol = kmb_sketch::solution_of(&outputs["sketch"]).unwrap();
        assert_eq!(sol.status, kmb_sketch::SolveStatus::WellConstrained);
        [sol.points[&kmb_sketch::EntityId(1)], sol.points[&second]]
    };
    let before = center(&ws);
    let width = ws.document().data().param_by_name("width").unwrap().id;
    ws.set_param_expr(width, "80 mm");
    let after = center(&ws);
    for (b, a) in before.iter().zip(&after) {
        assert!((a[0] - b[0] - 20.0).abs() < 1e-6, "{before:?} → {after:?}");
        assert!((a[1] - b[1]).abs() < 1e-6);
    }
    assert_eq!(ws.report().error_count(), 0);
}

#[test]
fn the_hole_snaps_to_corners_and_existing_holes_and_takes_a_depth() {
    // 径のクリックで穴ができ、そのまま作っている途中の欄で深さを直せる(作った後の「編集」を待たない)。
    // 中心は 0.5 mm の格子だけでなく、角と既存の穴にも吸い付く
    let part = kmb_samples::reference_part();
    let (plate, body) = (part.plate, part.hole_pattern);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui_state(
            |ui, ws: &mut Workspace| ws.ui(ui),
            Workspace::new(part.doc, None, reg()),
        );
    h.run();
    let ws = h.state_mut();
    ws.clear_selection();
    ws.set_picked(Some(Pick {
        feature: body,
        face: format!("f{}:end", plate.0),
    }));
    ws.start_action(jc_cad_hole_id());
    let g = ws.cur.gesture.as_ref().unwrap();
    let pt = g.current().unwrap();
    assert!(matches!(pt.gesture, kmb_core::Gesture::Point { .. }));
    // 板は 60 × 40 mm(外形の箱はカーネルで余白が付くことがあるので、寸法で見る)
    let (lo, hi) = ([0.0, 0.0], [60.0, 40.0]);
    let v0 = ws
        .cur
        .work
        .kernel()
        .mass(ws.visible_bodies()[0].1)
        .unwrap()
        .volume;
    // 板の上の面の高さ(ボスの分だけ外形の箱より低い)
    let top = ws.context().picked_plane().unwrap().point[2];
    let has = |p: [f64; 3]| {
        pt.snap_to
            .iter()
            .any(|q| (0..3).all(|i| (q[i] - p[i]).abs() < 0.05))
    };
    assert!(has([lo[0], lo[1], top]), "角に吸い付く: {:?}", pt.snap_to);
    assert!(
        has([(lo[0] + hi[0]) / 2.0, lo[1], top]),
        "辺の中点に吸い付く"
    );
    // 四隅の穴(中心から 4 mm)と、真ん中の穴の中心
    assert!(
        has([lo[0] + 4.0, lo[1] + 4.0, top]),
        "既存の穴の中心に吸い付く"
    );
    assert!(has([30.0, 20.0, top]), "{:?}", pt.snap_to);
    // 穴の縁の円をあてはめて中心を出す(頂点の平均ではない)。四角いポケットの縁は円でないので中心を出さない
    assert!(!has([15.5, 14.0, top]), "{:?}", pt.snap_to);
    // 中心 → 径で穴ができ、作っている途中の欄で深さを 3 mm にして Enter で決める
    ws.gesture_commit_step(serde_json::json!([20.0, 30.0, top]));
    ws.gesture_commit_step(serde_json::json!(4.0));
    assert_eq!(
        ws.cur.gesture.as_ref().map(|g| g.action.as_str()),
        Some("cad.hole_add"),
        "径を決めたら作り、続けて同じ穴を足す手順に入る"
    );
    let id = ws.selected().unwrap();
    assert_eq!(ws.creating(), Some(id));
    let mut d = ws.document().data().feature(id).unwrap().data.clone();
    d["depth"] = serde_json::json!("3 mm");
    assert!(ws.set_feature_data(id, d));
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    let ws = h.state_mut();
    assert_eq!(ws.creating(), None, "Enter で決まる");
    ws.wait();
    let d = &ws.document().data().feature(id).unwrap().data;
    assert_eq!(d["depth"], "3 mm", "{d}");
    assert_eq!(ws.report().error_count(), 0);
    // 径 4 mm・深さ 3 mm の分だけ体積が減る(形に穴が本当にあく)
    let v1 = ws
        .cur
        .work
        .kernel()
        .mass(ws.visible_bodies()[0].1)
        .unwrap()
        .volume;
    let cut = std::f64::consts::PI * 4.0 * 3.0;
    assert!((v0 - v1 - cut).abs() < 0.02 * cut, "{v0} → {v1}");
}

#[test]
fn a_new_hole_can_become_a_counterbored_screw_hole_in_its_fields() {
    // 径のクリックで穴ができた後、作っている途中の欄で種類(ザグリ)とネジ(M3 の通し穴)を選ぶ
    let part = kmb_samples::reference_part();
    let (plate, body) = (part.plate, part.hole_pattern);
    let mut ws = Workspace::new(part.doc, None, reg());
    ws.clear_selection();
    ws.set_picked(Some(Pick {
        feature: body,
        face: format!("f{}:end", plate.0),
    }));
    let v0 = ws
        .cur
        .work
        .kernel()
        .mass(ws.visible_bodies()[0].1)
        .unwrap()
        .volume;
    let top = ws.context().picked_plane().unwrap().point[2];
    ws.start_action(jc_cad_hole_id());
    ws.gesture_commit_step(serde_json::json!([20.0, 30.0, top]));
    ws.gesture_commit_step(serde_json::json!(6.0));
    let id = ws.creating().expect("作っている途中");
    let desc = ws.win.registry.descriptor(kmb_cad_hole::HOLE).unwrap();
    let shown = |d: &serde_json::Value| -> Vec<String> {
        desc.fields
            .iter()
            .filter(|f| f.applies(d))
            .map(|f| f.label.clone())
            .collect()
    };
    let mut d = ws.document().data().feature(id).unwrap().data.clone();
    assert!(!shown(&d).contains(&"ザグリの径".to_string()));
    assert!(shown(&d).contains(&"直径".to_string()));
    d["kind"] = serde_json::json!("counterbore");
    d["tap"] = serde_json::json!("clearance");
    d["size"] = serde_json::json!("M3");
    d.as_object_mut().unwrap().remove("diameter");
    let labels = shown(&d);
    assert!(labels.contains(&"ザグリの径".to_string()), "{labels:?}");
    assert!(labels.contains(&"はめあい".to_string()), "{labels:?}");
    assert!(
        !labels.contains(&"直径".to_string()),
        "径はネジの表から決める"
    );
    assert!(ws.set_feature_data(id, d));
    ws.finish_edit();
    ws.wait();
    assert_eq!(ws.report().error_count(), 0, "{:?}", ws.report());
    // 貫通の 3.4 mm の穴と、6.5 × 3.3 のザグリ
    let v1 = ws
        .cur
        .work
        .kernel()
        .mass(ws.visible_bodies()[0].1)
        .unwrap()
        .volume;
    let pi = std::f64::consts::PI;
    let thick = top
        - ws.cur
            .work
            .kernel()
            .bounds(ws.visible_bodies()[0].1)
            .unwrap()
            .0[2];
    let cut = pi * 1.7 * 1.7 * thick + pi * (3.25 * 3.25 - 1.7 * 1.7) * 3.3;
    assert!((v0 - v1 - cut).abs() < 0.02 * cut, "{v0} → {v1}, {cut}");
}

#[test]
fn a_hole_center_off_the_picked_face_is_refused() {
    // 面の外(何もない所)のクリックは受け付けない(平面に写して受けると、形の外に穴ができて
    // 何も変わらないまま確定する)
    let part = kmb_samples::reference_part();
    let (plate, body) = (part.plate, part.hole_pattern);
    let mut ws = Workspace::new(part.doc, None, reg());
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
    ws.cur.camera.fit(&ws.cur.scene, rect);
    ws.clear_selection();
    ws.set_picked(Some(Pick {
        feature: body,
        face: format!("f{}:end", plate.0),
    }));
    ws.start_action(jc_cad_hole_id());
    let click = |ws: &mut Workspace, p: egui::Pos2| {
        let ev = view3d::ViewEvents {
            click: Some(p),
            hover: Some(p),
            ..Default::default()
        };
        ws.gesture_events(&ev, rect);
    };
    // 画面の隅は何もない所
    click(&mut ws, egui::pos2(5.0, 5.0));
    let g = ws.cur.gesture.as_ref().unwrap();
    assert_eq!(g.step, 0, "面の外は受け付けない");
    assert!(ws.message().is_some_and(|m| m.contains("中をクリック")));
    // 面の上(穴もポケットもない所)は受け付ける
    let top = ws.context().picked_plane().unwrap().point[2] as f32;
    let p = ws.cur.camera.project([20.0, 30.0, top], rect).0;
    click(&mut ws, p);
    assert_eq!(
        ws.cur.gesture.as_ref().unwrap().step,
        1,
        "面の上なら次の手順へ"
    );
}

fn jc_cad_hole_id() -> &'static str {
    "cad.hole_on_face"
}

#[test]
fn a_thread_on_the_boss_suggests_the_nearest_metric_size() {
    let mut ws = Workspace::new(kmb_samples::reference_document(), None, reg());
    ws.wait();
    let (body_feature, body) = *ws.visible_bodies().last().unwrap();
    let arcs = ws.cur.work.kernel().arc_edges(body).unwrap();
    if on_truck() {
        // truck のカーネルは円の辺を出さないので、円筒が分からず理由つきで断る
        assert!(arcs.is_empty());
        kmb_contract::skip!("truck は円の辺を出さない(理由つきで断るのは確かめた)");
    }
    // ボス(径 10)の外側の円筒の面と、穴の内側の円筒の面
    let boss_face = arcs
        .iter()
        .find(|e| {
            e.arc
                .is_some_and(|a| a.inside && (a.radius - 5.0).abs() < 1e-6)
        })
        .map(|e| e.faces[1].clone())
        .expect("ボスの円筒");
    // 穴の口(平らな面が円の外で、出っ張りの辺)。ボスの根もと(円の外で、へこみ)は外ねじの側
    let hole = arcs
        .iter()
        .find(|e| e.convex && e.arc.is_some_and(|a| !a.inside))
        .cloned()
        .expect("穴の円筒");
    // ボタンを押すと、曲面も選べる面の手順に入る
    ws.start_action(kmb_cad_hole::THREAD_ON_FACE);
    assert!(
        ws.cur
            .gesture
            .as_ref()
            .and_then(|g| g.current())
            .is_some_and(|h| matches!(h.gesture, kmb_core::Gesture::AnyFace))
    );
    ws.cur.gesture = None;
    let id = ws
        .run_action_args(
            kmb_cad_hole::THREAD_ON_FACE,
            serde_json::json!({"feature": body_feature.0, "face": boss_face}),
        )
        .unwrap_or_else(|| panic!("{:?}", ws.message()));
    ws.wait();
    let data = ws.document().data().feature(id).unwrap().data.clone();
    assert_eq!(data["size"], "M10x1.5");
    let s = ws.summary(id).unwrap();
    assert!(s.contains("呼び径 10 mm・ピッチ 1.5 mm"), "{s}");
    assert!(s.contains("おねじ"), "{s}");
    // 一覧は面の径に近い順で、細目も選べる
    let sizes = kmb_core::choices_of(ws.output(id, "sizes").unwrap()).unwrap();
    assert_eq!(sizes[0].value, "M10x1.5");
    assert!(sizes[0].label.contains("近い"));
    assert!(sizes.iter().any(|c| c.value == "M10x1.25"));
    // ボディはそのまま渡す(形は変えない)
    assert!(ws.visible_bodies().iter().any(|b| b.0 == id));
    // 3D のビューは、ボスの円筒をねじ山の陰影の帯に切る(法線を軸の向きへ交互に傾ける。位置は同じ)
    let marks = ws
        .output(id, kmb_interchange::THREAD_MARKS_OUTPUT)
        .and_then(<kmb_interchange::ThreadMarks as kmb_core::Payload>::from_value)
        .expect("ねじの印");
    let m = marks.marks[0];
    assert!(!m.internal && (m.radius - 5.0).abs() < 1e-6 && (m.pitch - 1.5).abs() < 1e-9);
    ws.refresh_body_colors();
    let mesh = ws.cur.scene.mesh_of(id).expect("ボディの網");
    let tilt = view3d::THREAD_TILT.sin() as f32;
    let along =
        |n: &[f32; 3]| n[0] * m.axis[0] as f32 + n[1] * m.axis[1] as f32 + n[2] * m.axis[2] as f32;
    let up = mesh
        .normals
        .iter()
        .filter(|n| (along(n) - tilt).abs() < 1e-3)
        .count();
    let down = mesh
        .normals
        .iter()
        .filter(|n| (along(n) + tilt).abs() < 1e-3)
        .count();
    assert!(up > 0 && down > 0, "上向き {up}・下向き {down}");
    ws.finish_edit();
    // 穴の内側は、めねじ(穴の径を呼び径とみなす)
    let id = ws
        .run_action_args(
            kmb_cad_hole::THREAD_ON_FACE,
            serde_json::json!({"feature": id.0, "face": hole.faces[1]}),
        )
        .unwrap_or_else(|| panic!("{:?}", ws.message()));
    ws.wait();
    let s = ws.summary(id).unwrap();
    assert!(s.contains("めねじ"), "{s}");
    let size = kmb_cad_hole::thread::parse_size(
        ws.document().data().feature(id).unwrap().data["size"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let d = hole.arc.unwrap().radius * 2.0;
    assert!((size.major - d).abs() <= 0.5, "{d} → {}", size.name());
    assert!(s.contains("下穴の目安"), "{s}");
    // 平らな面は断る
    ws.run_action_args(
        kmb_cad_hole::THREAD_ON_FACE,
        serde_json::json!({"feature": id.0, "face": hole.faces[0]}),
    );
    assert!(
        ws.message().is_some_and(|m| m.contains("円筒")),
        "{:?}",
        ws.message()
    );
}
