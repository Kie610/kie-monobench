//! 回転とパターンの軸の画面の試験: 原点の軸・スケッチの線・ボディの辺を軸に選ぶ、輪郭 → 軸 → 角度の順のクリック。

use crate::tests::{can, reg};
use crate::*;

#[test]
fn revolve_and_pattern_axes_are_picked_by_clicking_origin_axes_and_sketch_lines() {
    // 軸を選ぶ間は原点の軸(と目安の原点の平面)が出て、原点の軸と
    // 見えているスケッチの線をクリックで軸にできる
    use kmb_samples::sample::{DocBuilder, SketchBuilder};
    use kmb_sketch::SketchPlane;
    let mut b = DocBuilder::new();
    // 輪郭: ZX 平面の長方形(世界の x 10〜15、z 0〜8)
    let mut p = SketchBuilder::new(SketchPlane::Zx);
    let o = p.fixed_point(0.0, 0.0);
    let c = p.point_from(o, "0 mm", "10 mm", [0.0, 10.0]);
    p.rectangle(c, "8 mm", "5 mm", [0.0, 10.0, 8.0, 5.0]);
    let profile = b.feature(kmb_sketch::SKETCH, "輪郭", p.build());
    // 別のスケッチの線: XY 平面の (-5, 30)〜(25, 30)
    let mut l = SketchBuilder::new(SketchPlane::Xy);
    let la = l.fixed_point(-5.0, 30.0);
    let lb = l.fixed_point(25.0, 30.0);
    let line = l.line(la, lb).0;
    let other = b.feature(kmb_sketch::SKETCH, "軸の線", l.build());
    let rev = b.feature(
        kmb_cad_revolve::REVOLVE,
        "回転",
        serde_json::json!({"profile": profile, "axis": {"kind": "auto"}, "angle": "360 deg"}),
    );
    let pat = b.feature(
        kmb_cad_pattern::CIRCULAR_PATTERN,
        "並び",
        serde_json::json!({"feature": rev, "axis": [0, 0, 1], "count": "3", "angle": "360 deg"}),
    );
    let mut ws = Workspace::new(b.finish("軸"), None, reg());
    ws.wait();
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
    ws.cur.camera.fit(&ws.cur.scene, rect);
    let values = |ws: &Workspace| -> Vec<serde_json::Value> {
        let h = ws.cur.gesture.as_ref().unwrap().current().unwrap();
        assert!(h.show_origin, "原点の平面を目安に出す");
        h.choices.iter().map(|c| c.value.clone()).collect()
    };
    let click = |ws: &mut Workspace, at: [f32; 3]| {
        let p = ws.cur.camera.project(at, rect).0;
        let ev = view3d::ViewEvents {
            click: Some(p),
            hover: Some(p),
            ..Default::default()
        };
        ws.gesture_events(&ev, rect);
    };

    // 回転: 欄の軸を「スケッチの線(クリックで選ぶ)」にすると、軸を選ぶ手順に入る
    ws.open_edit(rev);
    let entry = ws.document().data().feature(rev).unwrap().clone();
    let mut data = entry.data.clone();
    data["axis"] = serde_json::json!({"kind": "points"});
    ws.set_panel_data(&entry, data);
    let v = values(&ws);
    // 輪郭の平面(ZX)に垂直な Y 軸は出さない。輪郭の線は出す。別のスケッチの線は、輪郭と
    // 同じ平面に無い(XY 平面の y = 30)ので出さない(回転の軸は輪郭と同じ平面に置く)
    assert!(v.contains(&serde_json::json!("x")) && v.contains(&serde_json::json!("z")));
    assert!(!v.contains(&serde_json::json!("y")), "{v:?}");
    assert!(v.iter().any(|x| x.is_u64()), "輪郭の線: {v:?}");
    assert!(
        !v.contains(&serde_json::json!({"sketch": other.0, "line": line})),
        "{v:?}"
    );
    // 原点の X 軸(画面の上で一定の長さ)の根元の近くをクリック
    let s = view3d::origin_size(&ws.cur.camera, rect);
    click(&mut ws, [s * 0.3, 0.0, 0.0]);
    assert!(ws.cur.gesture.is_none(), "クリックで決まる");
    assert!(ws.is_editing(rev), "決めた後も直している途中");
    let data = &ws.document().data().feature(rev).unwrap().data;
    assert_eq!(data["axis"], serde_json::json!({"kind": "origin_x"}));
    ws.wait();
    assert!(
        matches!(
            ws.report().feature(rev),
            Some(kmb_core::FeatureStatus::Ok { .. })
        ),
        "{:?}",
        ws.report().feature(rev)
    );
    ws.finish_edit();

    // 円形のパターン: 隠したスケッチの線は選べず、出すと選べる
    ws.set_hidden(other, true);
    ws.open_edit(pat);
    let entry = ws.document().data().feature(pat).unwrap().clone();
    let mut data = entry.data.clone();
    data["axis_ref"] = serde_json::json!({"kind": "line"});
    ws.set_panel_data(&entry, data);
    let v = values(&ws);
    assert!(
        v.contains(&serde_json::json!("y")),
        "パターンは垂直の軸も選べる"
    );
    assert!(!v.iter().any(|x| x.get("sketch").is_some()), "{v:?}");
    ws.cur.gesture = None;
    ws.set_hidden(other, false);
    ws.begin_pick(kmb_cad_pattern::PATTERN_AXIS);
    let v = values(&ws);
    assert!(
        v.contains(&serde_json::json!({"sketch": other.0, "line": line})),
        "{v:?}"
    );
    // 線の真ん中(原点の軸から離れた所)をクリック
    click(&mut ws, [10.0, 30.0, 0.0]);
    assert!(ws.cur.gesture.is_none());
    let data = &ws.document().data().feature(pat).unwrap().data;
    assert_eq!(
        data["axis_ref"],
        serde_json::json!({"kind": "line", "sketch": other.0, "a": la.0, "b": lb.0})
    );
    ws.wait();
    assert!(
        matches!(
            ws.report().feature(pat),
            Some(kmb_core::FeatureStatus::Ok { .. })
        ),
        "{:?}",
        ws.report().feature(pat)
    );
}

/// 輪郭が 2 つの ZX 平面のスケッチ(世界の x 10〜15 と 20〜25、z 0〜8)と、その平面に辺を持つ台
/// (x -10〜-5、y 0〜5、z 0〜10。x = -5・y = 0 の縦の辺が輪郭と同じ平面にある)。
fn two_profiles_and_a_block() -> (Workspace, FeatureId) {
    use kmb_samples::sample::{DocBuilder, SketchBuilder};
    use kmb_sketch::SketchPlane;
    let mut b = DocBuilder::new();
    let mut p = SketchBuilder::new(SketchPlane::Zx);
    let o = p.fixed_point(0.0, 0.0);
    let c1 = p.point_from(o, "0 mm", "10 mm", [0.0, 10.0]);
    p.rectangle(c1, "8 mm", "5 mm", [0.0, 10.0, 8.0, 5.0]);
    let c2 = p.point_from(o, "0 mm", "20 mm", [0.0, 20.0]);
    p.rectangle(c2, "8 mm", "5 mm", [0.0, 20.0, 8.0, 5.0]);
    b.feature(kmb_sketch::SKETCH, "輪郭", p.build());
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let c = s.fixed_point(-10.0, 0.0);
    s.rectangle(c, "5 mm", "5 mm", [-10.0, 0.0, 5.0, 5.0]);
    let sk = b.feature(kmb_sketch::SKETCH, "台のスケッチ", s.build());
    let block = b.feature(
        kmb_cad_extrude::EXTRUDE,
        "台",
        kmb_cad_extrude::Extrude {
            profile: Some(sk),
            distance: "10 mm".into(),
            ..Default::default()
        },
    );
    let mut ws = Workspace::new(b.finish("回転"), None, reg());
    ws.wait();
    assert_eq!(ws.report().error_count(), 0, "{:?}", ws.report());
    assert_eq!(ws.visible_bodies().len(), 1, "台");
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
    ws.cur.camera.fit(&ws.cur.scene, rect);
    ws.clear_selection();
    (ws, block)
}

#[test]
fn a_picking_step_moves_on_after_one_click_and_ctrl_keeps_adding() {
    // Kie 2026-10-21: 1 つ選んだら次の欄へ。1 つ目の欄に複数選ぶ時は Ctrl を押しながら
    let (mut ws, _) = two_profiles_and_a_block();
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
    let click = |ws: &mut Workspace, at: [f32; 3], ctrl: bool| {
        let p = ws.cur.camera.project(at, rect).0;
        let ev = view3d::ViewEvents {
            click: Some(p),
            hover: Some(p),
            add: ctrl,
            ..Default::default()
        };
        ws.gesture_events(&ev, rect);
    };
    let step = |ws: &Workspace| ws.cur.gesture.as_ref().unwrap().current().unwrap().clone();
    ws.start_action(kmb_cad_revolve::REVOLVE_SKETCH);
    assert!(matches!(step(&ws).gesture, kmb_core::Gesture::Regions));
    // Ctrl: 足して待つ。もう一度 Ctrl で外す
    click(&mut ws, [12.5, 0.0, 4.0], true);
    assert!(matches!(step(&ws).gesture, kmb_core::Gesture::Regions));
    assert_eq!(ws.cur.gesture.as_ref().unwrap().picks.len(), 1);
    click(&mut ws, [12.5, 0.0, 4.0], true);
    assert_eq!(ws.cur.gesture.as_ref().unwrap().picks.len(), 0);
    click(&mut ws, [12.5, 0.0, 4.0], true);
    // 修飾なしのクリックで 2 つ目を足し、そのまま軸へ
    click(&mut ws, [22.5, 0.0, 4.0], false);
    assert!(
        matches!(step(&ws).gesture, kmb_core::Gesture::Choice),
        "軸へ進む"
    );
    let regions = &ws.cur.gesture.as_ref().unwrap().args["regions"];
    assert_eq!(regions.as_array().map(Vec::len), Some(2), "{regions}");
}

#[test]
fn a_body_edge_in_the_profile_plane_can_be_the_revolve_axis() {
    // 回転の軸にボディのまっすぐな辺を選べる(原点の軸とスケッチの線に限らない)
    let (mut ws, block) = two_profiles_and_a_block();
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
    let click = |ws: &mut Workspace, at: [f32; 3]| {
        let p = ws.cur.camera.project(at, rect).0;
        let ev = view3d::ViewEvents {
            click: Some(p),
            hover: Some(p),
            ..Default::default()
        };
        ws.gesture_events(&ev, rect);
    };
    ws.start_action(kmb_cad_revolve::REVOLVE_SKETCH);
    click(&mut ws, [12.5, 0.0, 4.0]);
    let h = ws.cur.gesture.as_ref().unwrap().current().unwrap().clone();
    let edges: Vec<&serde_json::Value> = h
        .choices
        .iter()
        .map(|c| &c.value)
        .filter(|v| v.get("edge").is_some())
        .collect();
    // 台の手前の面(y = 0)の 4 本だけが輪郭と同じ平面にある
    assert_eq!(edges.len(), 4, "{edges:?}");
    assert!(edges.iter().all(|v| v["feature"] == block.0));
    // x = -5 の縦の辺の真ん中
    click(&mut ws, [-5.0, 0.0, 5.0]);
    let h = ws.cur.gesture.as_ref().unwrap().current().unwrap().clone();
    assert!(
        matches!(h.gesture, kmb_core::Gesture::Angle { .. }),
        "{:?} {:?}",
        h.label,
        ws.win.message
    );
    ws.command_ok();
    let id = ws.selected().unwrap();
    let d = ws.document().data().feature(id).unwrap().data.clone();
    assert_eq!(d["axis"]["kind"], "edge", "{d}");
    ws.finish_edit();
    ws.wait();
    assert_eq!(ws.report().error_count(), 0, "{:?}", ws.report());
    let body = ws
        .visible_bodies()
        .into_iter()
        .find(|b| b.0 == id)
        .expect("回転のボディ");
    let v = ws.cur.work.kernel().mass(body.1).unwrap().volume;
    // x 10〜15 の輪郭を x = -5 の軸で回した輪(半径 15〜20、高さ 8)
    let ring = std::f64::consts::PI * (400.0 - 225.0) * 8.0;
    assert!((v - ring).abs() < ring * 2e-3, "{v} {ring}");
}

#[test]
fn revolve_turns_only_the_picked_region() {
    // スケッチに輪郭が 2 つあれば、クリックした領域だけを回す(押し出しと同じ選び方)
    use kmb_samples::sample::{DocBuilder, SketchBuilder};
    use kmb_sketch::SketchPlane;
    let mut b = DocBuilder::new();
    // ZX 平面の長方形 2 つ(世界の x 10〜15 と 20〜25、z 0〜8)
    let mut p = SketchBuilder::new(SketchPlane::Zx);
    let o = p.fixed_point(0.0, 0.0);
    let c1 = p.point_from(o, "0 mm", "10 mm", [0.0, 10.0]);
    p.rectangle(c1, "8 mm", "5 mm", [0.0, 10.0, 8.0, 5.0]);
    let c2 = p.point_from(o, "0 mm", "20 mm", [0.0, 20.0]);
    p.rectangle(c2, "8 mm", "5 mm", [0.0, 20.0, 8.0, 5.0]);
    b.feature(kmb_sketch::SKETCH, "輪郭", p.build());
    let mut ws = Workspace::new(b.finish("回転"), None, reg());
    ws.wait();
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
    ws.cur.camera.fit(&ws.cur.scene, rect);
    let click = |ws: &mut Workspace, at: [f32; 3]| {
        let p = ws.cur.camera.project(at, rect).0;
        let ev = view3d::ViewEvents {
            click: Some(p),
            hover: Some(p),
            ..Default::default()
        };
        ws.gesture_events(&ev, rect);
    };
    ws.clear_selection();
    ws.start_action(kmb_cad_revolve::REVOLVE_SKETCH);
    let h = ws.cur.gesture.as_ref().unwrap().current().unwrap().clone();
    assert_eq!(h.choices.len(), 2, "領域ごとに選べる");
    click(&mut ws, [22.5, 0.0, 4.0]);
    ws.command_ok();
    let s = view3d::origin_size(&ws.cur.camera, rect);
    click(&mut ws, [0.0, 0.0, s * 0.3]);
    ws.command_ok();
    let id = ws.selected().unwrap();
    let d = ws.document().data().feature(id).unwrap().data.clone();
    assert_eq!(d["regions"].as_array().map(Vec::len), Some(1), "{d}");
    ws.wait();
    let bodies = ws.visible_bodies();
    assert_eq!(bodies.len(), 1);
    let v = ws.cur.work.kernel().mass(bodies[0].1).unwrap().volume;
    let ring = std::f64::consts::PI * (625.0 - 400.0) * 8.0;
    assert!((v - ring).abs() < ring * 2e-3, "{v} {ring}");

    // 軸を選び直した後でも、領域を選び直せる(Kie 2026-10-19)。手順ごとに選べる物が決まっていて、前の選びが邪魔をしない
    let step = |ws: &Workspace| ws.cur.gesture.as_ref().unwrap().current().unwrap().clone();
    ws.select(id);
    ws.start_action(kmb_cad_revolve::REVOLVE_AXIS);
    assert!(matches!(step(&ws).gesture, kmb_core::Gesture::Choice));
    click(&mut ws, [0.0, 0.0, s * 0.3]);
    assert!(ws.cur.gesture.is_none(), "{:?}", ws.win.message);
    let d = ws.document().data().feature(id).unwrap().data.clone();
    assert_eq!(d["axis"], serde_json::json!({"kind": "origin_z"}), "{d}");
    ws.select(id);
    ws.start_action(kmb_cad_revolve::REVOLVE_REGIONS);
    let h = step(&ws);
    assert!(
        matches!(h.gesture, kmb_core::Gesture::Regions),
        "{:?}",
        h.label
    );
    assert_eq!(h.choices.len(), 2, "軸を選んだ後も領域を選べる");
    click(&mut ws, [12.5, 0.0, 4.0]);
    assert_eq!(
        ws.cur.gesture.as_ref().unwrap().picks.len(),
        2,
        "今の領域に足す"
    );
    ws.command_ok();
    let d = ws.document().data().feature(id).unwrap().data.clone();
    assert!(d.get("regions").is_none(), "両方の領域(全部)を回す: {d}");
    assert_eq!(
        d["axis"],
        serde_json::json!({"kind": "origin_z"}),
        "軸はそのまま"
    );
}

#[test]
fn revolve_is_made_by_picking_the_profile_then_the_axis_then_the_angle() {
    // 押しただけでは作らない(自動の軸で作ったり、輪郭を直前の候補で埋めたりしない)。
    // 輪郭 → 軸 → 角度の順にクリックで選ぶ。軸は輪郭と同じ平面にあるものだけ
    // 選べる(2026-10-18 Kie)。右クリックの OK・キャンセルも同じ流れで使える
    use kmb_samples::sample::{DocBuilder, SketchBuilder};
    use kmb_sketch::SketchPlane;
    let mut b = DocBuilder::new();
    // 輪郭: ZX 平面の長方形(世界の x 10〜15、z 0〜8)
    let mut p = SketchBuilder::new(SketchPlane::Zx);
    let o = p.fixed_point(0.0, 0.0);
    let c = p.point_from(o, "0 mm", "10 mm", [0.0, 10.0]);
    p.rectangle(c, "8 mm", "5 mm", [0.0, 10.0, 8.0, 5.0]);
    let profile = b.feature(kmb_sketch::SKETCH, "輪郭", p.build());
    // 輪郭と同じ平面に無い線: XY 平面の (-5, 30)〜(25, 30)
    let mut l = SketchBuilder::new(SketchPlane::Xy);
    let la = l.fixed_point(-5.0, 30.0);
    let lb = l.fixed_point(25.0, 30.0);
    let line = l.line(la, lb).0;
    let other = b.feature(kmb_sketch::SKETCH, "軸の線", l.build());
    let mut ws = Workspace::new(b.finish("回転"), None, reg());
    ws.wait();
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
    ws.cur.camera.fit(&ws.cur.scene, rect);
    let click = |ws: &mut Workspace, at: [f32; 3]| {
        let p = ws.cur.camera.project(at, rect).0;
        let ev = view3d::ViewEvents {
            click: Some(p),
            hover: Some(p),
            ..Default::default()
        };
        ws.gesture_events(&ev, rect);
    };
    let step = |ws: &Workspace| ws.cur.gesture.as_ref().unwrap().current().unwrap().clone();
    let n = ws.document().data().features.len();
    ws.clear_selection();
    assert!(can(&ws, kmb_cad_revolve::REVOLVE_SKETCH));
    // 輪郭・軸・角度を選んで、角度の手順まで進める
    let to_angle = |ws: &mut Workspace| {
        let before = ws.document().data().features.len();
        ws.start_action(kmb_cad_revolve::REVOLVE_SKETCH);
        assert_eq!(
            ws.document().data().features.len(),
            before,
            "押しただけでは作らない"
        );
        let h = step(ws);
        assert!(matches!(h.gesture, kmb_core::Gesture::Regions), "まず輪郭");
        assert_eq!(h.choices.len(), 1, "{:?}", h.choices);
        assert!(ws.ok_blocked().is_some(), "輪郭を選ぶまで OK は押せない");
        // 輪郭を 1 つクリックすると、Enter を待たずに軸の手順へ進む(Kie 2026-10-21。
        // クリックの後に Enter か OK を求めない)
        click(ws, [12.5, 0.0, 4.0]);
        let h = step(ws);
        assert!(matches!(h.gesture, kmb_core::Gesture::Choice), "次に軸");
        assert!(h.show_origin);
        let v: Vec<serde_json::Value> = h.choices.iter().map(|c| c.value.clone()).collect();
        // 輪郭の平面(ZX)にある原点の X・Z 軸と輪郭の線だけ。垂直な Y 軸と、平面に無い線は出さない
        assert!(v.contains(&serde_json::json!("x")) && v.contains(&serde_json::json!("z")));
        assert!(!v.contains(&serde_json::json!("y")), "{v:?}");
        assert!(
            v.iter()
                .any(|x| x.get("line").is_some() && x.get("sketch").is_none())
        );
        assert!(
            !v.contains(&serde_json::json!({"sketch": other.0, "line": line})),
            "{v:?}"
        );
        // 軸は自動で選ばない(レビュー 2 回目 M4)。クリックするまで OK は押せない
        assert!(
            ws.cur.gesture.as_ref().unwrap().picks.is_empty(),
            "{}",
            h.label
        );
        assert!(ws.ok_blocked().is_some(), "軸を選ぶまで OK は押せない");
        // 原点の Z 軸の根元の近く
        let s = view3d::origin_size(&ws.cur.camera, rect);
        click(ws, [0.0, 0.0, s * 0.3]);
        let h = step(ws);
        assert!(
            matches!(h.gesture, kmb_core::Gesture::Angle { .. }),
            "最後に角度"
        );
        assert!(!h.outline.is_empty(), "回す輪郭を描く");
        assert!(ws.ok_blocked().is_none(), "角度は省ける(一周)");
    };

    // 1 回目: マウスを Z 軸まわりに 90 度の所へ。OK(Enter と同じ)は一周で作って決める
    to_angle(&mut ws);
    let at = |deg: f64| {
        let q = crate::gesture::rotate([12.5, 0.0, 4.0], [0.0; 3], [0.0, 0.0, 1.0], deg);
        q.map(|v| v as f32)
    };
    let p = ws.cur.camera.project(at(90.0), rect).0;
    let hover = view3d::ViewEvents {
        hover: Some(p),
        ..Default::default()
    };
    ws.gesture_events(&hover, rect);
    assert_eq!(
        ws.cur.gesture.as_ref().unwrap().live,
        Some(serde_json::json!(90.0)),
        "マウスの所の角度"
    );
    ws.command_ok();
    assert!(ws.cur.gesture.is_none());
    assert_eq!(ws.creating(), None, "OK で作って決める");
    let id = ws.selected().expect("作ったものを選ぶ");
    assert_eq!(ws.document().data().features.len(), n + 1);
    let d = ws.document().data().feature(id).unwrap().data.clone();
    assert_eq!(d["profile"], serde_json::json!(profile.0));
    assert_eq!(d["axis"], serde_json::json!({"kind": "origin_z"}));
    assert_eq!(d["angle"], "360 deg");
    ws.wait();
    assert!(
        matches!(
            ws.report().feature(id),
            Some(kmb_core::FeatureStatus::Ok { .. })
        ),
        "{:?}",
        ws.report().feature(id)
    );

    // 2 回目: 90 度の所をクリックで作り、作っている途中を右クリックのキャンセルでやめる
    to_angle(&mut ws);
    click(&mut ws, at(90.0));
    let id2 = ws.creating().expect("作っている途中");
    assert_eq!(
        ws.document().data().feature(id2).unwrap().data["angle"],
        "90 deg"
    );
    assert!(ws.ok_blocked().is_none());
    ws.command_cancel();
    assert_eq!(ws.creating(), None);
    assert_eq!(
        ws.document().data().features.len(),
        n + 1,
        "キャンセルで作る前に戻す"
    );

    // 3 回目: 軸の手順で何もクリックせず Enter しても、軸は自動で決まらず軸の手順に留まる
    // (レビュー 2 回目 M4)。原点の X 軸をクリックすると角度へ進む
    let before = ws.document().data().features.len();
    ws.start_action(kmb_cad_revolve::REVOLVE_SKETCH);
    click(&mut ws, [12.5, 0.0, 4.0]);
    ws.command_ok();
    ws.gesture_enter();
    let h = step(&ws);
    assert!(
        matches!(h.gesture, kmb_core::Gesture::Choice),
        "{:?}",
        h.label
    );
    let s = view3d::origin_size(&ws.cur.camera, rect);
    click(&mut ws, [s * 0.3, 0.0, 0.0]);
    let h = step(&ws);
    assert!(
        matches!(h.gesture, kmb_core::Gesture::Angle { .. }),
        "{:?}",
        h.label
    );
    ws.gesture_enter();
    assert!(ws.cur.gesture.is_none());
    let id3 = ws.selected().expect("作ったものを選ぶ");
    assert_eq!(ws.document().data().features.len(), before + 1);
    let d = ws.document().data().feature(id3).unwrap().data.clone();
    // X 軸の上には輪郭の辺が重なっているので、クリックはその線(どちらでも同じ軸)
    assert!(
        d["axis"] == serde_json::json!({"kind": "origin_x"}) || d["axis"]["kind"] == "points",
        "{d}"
    );
    assert!(
        matches!(
            ws.report().feature(id3),
            Some(kmb_core::FeatureStatus::Ok { .. })
        ),
        "{:?}",
        ws.report().feature(id3)
    );

    // 輪郭と同じ平面に無い軸は、CLI・MCP の指定でも断る
    let before = ws.document().data().features.len();
    ws.run_action_args(
        kmb_cad_revolve::REVOLVE_SKETCH,
        serde_json::json!({
            "profile": profile.0,
            "axis": {"sketch": other.0, "line": line},
        }),
    );
    assert_eq!(ws.document().data().features.len(), before);
    assert!(
        ws.win
            .message
            .as_deref()
            .is_some_and(|m| m.contains("平面の上にありません")),
        "{:?}",
        ws.win.message
    );
}
