//! 画面で選ぶ・置く物(入る位置・タブ・輪郭の輪・スケッチの線・クランプ)を基準部品で確かめる。

use crate::cam_tests::*;
use crate::reference_cam;
use kmb_cam_commands::*;
use kmb_core::FeatureStatus;

#[test]
fn tabs_keep_bridges_on_the_outline_and_a_cut_out_without_tabs_warns() {
    let mut cam = reference_cam();
    let reg = registry();
    let kernel = kmb_bundle::kernel();
    let kernel = kernel.as_ref();
    let report = cam.part.doc.recompute(&reg, kernel);
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(cam.contour) else {
        panic!()
    };
    // 基準のストックは下の余白が 0 なので、外形の輪郭は切り抜きになる
    assert!(
        kmb_core::warnings_of(outputs)
            .iter()
            .any(|w| w.contains("タブ")),
        "{:?}",
        kmb_core::warnings_of(outputs)
    );
    let mut data = cam
        .part
        .doc
        .data()
        .feature(cam.contour)
        .unwrap()
        .data
        .clone();
    data["tabs"] = serde_json::json!(4);
    set_data(&mut cam.part.doc, cam.contour, data);
    let report = cam.part.doc.recompute(&reg, kernel);
    assert_eq!(report.error_count(), 0, "{:?}", report.features);
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(cam.contour) else {
        panic!()
    };
    assert!(
        kmb_core::warnings_of(outputs)
            .iter()
            .all(|w| !w.contains("タブ"))
    );
    let s = &setups(cam.part.doc.data(), &report, &reg)[0];
    let name = &cam.part.doc.data().feature(cam.contour).unwrap().name;
    let tp = s.paths.iter().find(|p| &p.name == name).unwrap();
    // 底は部品の下面(上面から 16 mm 下)よりさらに 0.3 mm 下、タブの上は部品の下面から 1.5 mm 上
    let zs: Vec<f64> = tp.moves.iter().map(|m| m.to()[2]).collect();
    let low = zs.iter().cloned().fold(f64::MAX, f64::min);
    assert!((low + 16.3).abs() < 1e-6, "{low}");
    let at_tab = zs.iter().filter(|z| (**z + 14.5).abs() < 1e-6).count();
    assert!(at_tab >= 4, "{at_tab}");
    let sim = simulate(kernel, s, SimOptions::default()).unwrap();
    assert!(sim.is_ok(), "{}", sim.summary());
}

fn pocket_path(
    cam: &mut crate::ReferenceCam,
    id: kmb_core::FeatureId,
) -> (SetupInfo, Toolpath, Vec<String>) {
    let m = modules();
    let report = cam.part.doc.recompute(&m.registry, m.kernel.as_ref());
    assert_eq!(report.error_count(), 0, "{:?}", report.features);
    let s = setups(cam.part.doc.data(), &report, &m.registry).remove(0);
    let name = &cam.part.doc.data().feature(id).unwrap().name;
    let tp = s
        .generated(s.paths.iter().position(|p| &p.name == name).unwrap())
        .clone();
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(id) else {
        panic!()
    };
    (s.info.clone(), tp, kmb_core::warnings_of(outputs))
}

#[test]
fn a_pocket_is_entered_where_the_user_clicked_with_a_helix() {
    let mut cam = reference_cam();
    let pocket = cam.pocket;
    let (info, _, _) = pocket_path(&mut cam, pocket);
    // ポケット(左下 (8, 8) から 15 × 12)の真ん中をクリック
    let top = info.to_world([0.0, 0.0, info.stock_max[2]])[2];
    run_cam(
        &mut cam,
        PLACE_ENTRY,
        pocket,
        serde_json::json!({"point": [15.5, 14.0, top]}),
    );
    let (info, tp, warnings) = pocket_path(&mut cam, pocket);
    assert!(
        !warnings.iter().any(|w| w.contains("進入位置")),
        "{warnings:?}"
    );
    let e = [15.5 - info.origin[0], 14.0 - info.origin[1]];
    let rh = 0.5 * tp.tool.radius();
    // 最初の早送りの後は、入る位置のまわりを螺旋で下りる
    let first = tp
        .moves
        .iter()
        .position(|m| !matches!(m, kmb_toolpath::Move::Rapid { .. }))
        .unwrap();
    let Some(kmb_toolpath::Move::Rapid { to }) = tp.moves.get(first - 1) else {
        panic!()
    };
    assert!(
        (to[0] - e[0]).abs() < 1e-6 && (to[1] - e[1]).abs() < 1e-6,
        "{to:?}"
    );
    // らせんは入る位置を中心にした半周ずつの円弧(G2・G3)で、入りの印が付く
    // 入る位置の上で下り、らせんの始まり(半径の所)へ寄ってから回る
    let first = first
        + tp.moves[first..]
            .iter()
            .position(|m| matches!(m, kmb_toolpath::Move::Arc { .. }))
            .unwrap();
    let arcs: Vec<&kmb_toolpath::Move> = tp.moves[first..]
        .iter()
        .take_while(|m| {
            matches!(m, kmb_toolpath::Move::Arc { center, .. }
                if (center[0] - e[0]).abs() < 1e-6 && (center[1] - e[1]).abs() < 1e-6)
        })
        .collect();
    assert!(arcs.len() >= 4, "{:?}", &tp.moves[first..first + 6]);
    let helix: Vec<[f64; 3]> = arcs.iter().map(|m| m.to()).collect();
    assert!(
        helix
            .iter()
            .all(|p| ((p[0] - e[0]).hypot(p[1] - e[1]) - rh).abs() < 1e-6),
        "{helix:?}"
    );
    assert!(helix.windows(2).all(|w| w[1][2] <= w[0][2] + 1e-9));
    let roles = tp.roles();
    assert!(
        (first..first + arcs.len()).all(|k| roles[k] == Some(kmb_toolpath::Role::Entry)),
        "{:?}",
        &roles[first..first + arcs.len()]
    );
    // 削る所の外に置くと、注意を出して輪の始まりから入る
    run_cam(
        &mut cam,
        PLACE_ENTRY,
        pocket,
        serde_json::json!({"point": [2.0, 2.0, top]}),
    );
    let (_, _, warnings) = pocket_path(&mut cam, pocket);
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("輪の始まりから入ります")),
        "{warnings:?}"
    );
    // 自動に戻す
    run_cam(&mut cam, AUTO_ENTRY, pocket, serde_json::Value::Null);
    let d = &cam.part.doc.data().feature(pocket).unwrap().data;
    assert!(d.get("entry").is_none(), "{d}");
}

#[test]
fn a_clamp_on_a_bed_hole_is_avoided_by_the_paths() {
    let mut cam = reference_cam();
    let (pocket, setup) = (cam.pocket, cam.setup);
    // 置く手順は、ベッドのネジ穴に吸い付く
    let m = modules();
    let report = cam.part.doc.recompute(&m.registry, m.kernel.as_ref());
    let cx = kmb_core::Context {
        selected: Some(setup),
        ..kmb_core::Context::new(cam.part.doc.data(), &report, &m.registry, &m.kernel)
    };
    let h = m.extensions.handles(ADD_FIXTURE, &cx).expect("置く手順");
    assert!(h[0].snap_to.len() > 10, "{}", h[0].snap_to.len());
    // ストックの上で押すと、いちばん近い辺の外へずれる
    let (info, _, _) = pocket_path(&mut cam, pocket);
    run_cam(
        &mut cam,
        ADD_FIXTURE,
        setup,
        serde_json::json!({"point": [15.5, 14.0, 0.0]}),
    );
    let (placed, _, _) = pocket_path(&mut cam, pocket);
    let (flo, fhi) = placed.fixtures[0];
    let (slo, shi) = (placed.stock_min, placed.stock_max);
    assert!(
        fhi[0] <= slo[0] || flo[0] >= shi[0] || fhi[1] <= slo[1] || flo[1] >= shi[1],
        "{flo:?} {fhi:?} がストック {slo:?} {shi:?} に重なる"
    );
    // 前の版で置いた、ポケットの真ん中の固定具は、削らずに上を越える
    let mut d = cam.part.doc.data().feature(setup).unwrap().data.clone();
    d["fixtures"][0]["at"] = serde_json::json!([15.5, 14.0]);
    set_data(&mut cam.part.doc, setup, d);
    let (info2, tp, warnings) = pocket_path(&mut cam, pocket);
    assert_eq!(info.origin, info2.origin);
    assert!(
        warnings.iter().any(|w| w.contains("固定具")),
        "{warnings:?}"
    );
    let (lo, hi) = info2.keep_out[0];
    let mut prev = tp.moves[0].to();
    for mv in &tp.moves[1..] {
        let p = mv.to();
        for k in 0..=10 {
            let t = k as f64 / 10.0;
            let q = [0, 1, 2].map(|i| prev[i] + (p[i] - prev[i]) * t);
            let inside = q[0] > lo[0] + 1e-6
                && q[0] < hi[0] - 1e-6
                && q[1] > lo[1] + 1e-6
                && q[1] < hi[1] - 1e-6;
            assert!(!inside || q[2] >= hi[2] - 1e-6, "{q:?} が避ける範囲に入る");
        }
        prev = p;
    }
    run_cam(&mut cam, CLEAR_FIXTURES, setup, serde_json::Value::Null);
    let (info3, _, _) = pocket_path(&mut cam, pocket);
    assert!(info3.keep_out.is_empty());
}

/// 経路の切削の動き(早送り以外)の終わりの点(世界座標)と、その役目。
fn cuts_world(
    info: &SetupInfo,
    tp: &kmb_toolpath::Toolpath,
) -> Vec<([f64; 3], Option<kmb_toolpath::Role>)> {
    let roles = tp.roles();
    tp.moves
        .iter()
        .zip(roles)
        .filter(|(m, _)| !matches!(m, kmb_toolpath::Move::Rapid { .. }))
        .map(|(m, r)| (info.to_world(m.to()), r))
        .collect()
}

#[test]
fn a_contour_on_the_top_face_cuts_its_holes_first_and_leaves_the_boss() {
    let reg = registry();
    let kernel = kmb_bundle::kernel();
    let kernel = kernel.as_ref();
    let mut cam = reference_cam();
    let plate = cam.part.plate;
    let mut c = feature_data(&cam, cam.contour);
    // 板の上面の縁を全部(外周、中央の穴、四隅の穴、ポケットの口、ボスの根元)
    c["chains"] = serde_json::json!([{ "face": format!("f{}:end", plate.0) }]);
    c["entry_kind"] = serde_json::json!("helix");
    set_data(&mut cam.part.doc, cam.contour, c);
    let s = recompute_ok(&mut cam, &reg, kernel);
    let tp = s.paths.iter().find(|p| p.name == "外形").unwrap();
    let pts = cuts_world(&s.info, tp);
    // 穴の縁を先に削る(最初の切削は板の内側)
    let first = pts[0].0;
    assert!(
        (0.0..=60.0).contains(&first[0]) && (0.0..=40.0).contains(&first[1]),
        "{first:?}"
    );
    // 中央の穴(直径 8)の中を、穴の底(板の底)まで回る
    let r = tp.tool.radius();
    let in_hole = pts
        .iter()
        .filter(|(p, _)| ((p[0] - 30.0).hypot(p[1] - 20.0) - (4.0 - r)).abs() < 1e-3)
        .map(|(p, _)| p[2])
        .fold(f64::INFINITY, f64::min);
    assert!(in_hole <= 1e-6, "{in_hole}");
    // らせんの入りは G2・G3 の円弧で、入りの印が付く
    assert!(tp.moves.iter().zip(tp.roles()).any(|(m, r)| matches!(
        m,
        kmb_toolpath::Move::Arc { .. }
    ) && r
        == Some(kmb_toolpath::Role::Entry)));
    let sim = simulate(kernel, &s, SimOptions::default()).unwrap();
    assert!(sim.is_ok(), "{}", sim.summary());
}

#[test]
fn a_contour_follows_a_sketch_inside_and_an_open_line_from_where_the_user_clicked() {
    // 入る位置を置く操作と同じ形状カーネルで計算する(キャッシュの形状を共有する)
    let m = modules();
    let (reg, kernel) = (&m.registry, m.kernel.as_ref());
    let mut cam = reference_cam();
    let plate = cam.part.plate;
    // 上面に、ポケットの口と同じ長方形と、板の手前の外に開いた線を描く
    let mut s = crate::sample::SketchBuilder::new(kmb_sketch::SketchPlane::face(
        plate,
        format!("f{}:end", plate.0),
    ));
    let q = [
        s.fixed_point(8.0, 8.0),
        s.fixed_point(23.0, 8.0),
        s.fixed_point(23.0, 20.0),
        s.fixed_point(8.0, 20.0),
    ];
    for k in 0..4 {
        s.line(q[k], q[(k + 1) % 4]);
    }
    let a = s.fixed_point(5.0, -1.7);
    let b = s.fixed_point(55.0, -1.7);
    s.line(a, b);
    // 輪郭より前に置く(後ろの機能は参照できない)
    let sketch = cam.part.doc.alloc_feature_id();
    let at = cam.part.doc.data().index_of(cam.contour);
    cam.part
        .doc
        .apply(kmb_core::Transaction::new(
            "削る線",
            vec![kmb_core::Op::InsertFeature {
                index: at,
                entry: kmb_core::FeatureEntry {
                    id: sketch,
                    type_id: kmb_sketch::SKETCH.into(),
                    name: "削る線".into(),
                    data: json(&s.build()),
                    suppressed: false,
                },
            }],
        ))
        .unwrap();
    let mut c = feature_data(&cam, cam.contour);
    c["chains"] = serde_json::json!([{ "sketch": sketch.0 }]);
    c["side"] = serde_json::json!("inside");
    c["depth"] = serde_json::json!("4 mm");
    c["extra_depth"] = serde_json::json!("0 mm");
    c["entry_kind"] = serde_json::json!("helix");
    set_data(&mut cam.part.doc, cam.contour, c);
    // 入る位置を長方形の真ん中に置く(輪郭でも置ける)
    let report = cam.part.doc.recompute(reg, kernel);
    let s0 = setups(cam.part.doc.data(), &report, reg).remove(0);
    let top = s0.info.to_world([0.0, 0.0, s0.info.stock_max[2]])[2];
    let contour = cam.contour;
    run_cam(
        &mut cam,
        PLACE_ENTRY,
        contour,
        serde_json::json!({"point": [15.5, 14.0, top]}),
    );
    let s = recompute_ok(&mut cam, reg, kernel);
    let tp = s.paths.iter().find(|p| p.name == "外形").unwrap();
    let pts = cuts_world(&s.info, tp);
    // 長方形の中は、入る位置を中心にしたらせんで下りる
    let e = s.info.to_world([0.0, 0.0, 0.0]);
    let e = [15.5 - e[0], 14.0 - e[1]];
    let e = {
        let w = s.info.to_world([e[0], e[1], 0.0]);
        [w[0], w[1]]
    };
    assert!(
        tp.moves
            .iter()
            .any(|m| matches!(m, kmb_toolpath::Move::Arc { center, .. }
            if { let w = s.info.to_world([center[0], center[1], 0.0]);
                 (w[0] - e[0]).hypot(w[1] - e[1]) < 1e-6 })),
        "らせんの中心が入る位置にない"
    );
    // 長方形の壁と開いた線を、上面から 4 mm(ポケットの底、高さ 6)まで
    let bottom = pts.iter().map(|(p, _)| p[2]).fold(f64::INFINITY, f64::min);
    assert!((bottom - 6.0).abs() < 1e-6, "{bottom}");
    let on_line: Vec<_> = pts
        .iter()
        .filter(|(p, _)| (p[1] + 1.7).abs() < 1e-6 && (p[2] - 6.0).abs() < 1e-6)
        .collect();
    assert!(!on_line.is_empty(), "開いた線をなぞる");
    let sim = simulate(kernel, &s, SimOptions::default()).unwrap();
    assert!(sim.is_ok(), "{}", sim.summary());
}

#[test]
fn the_outline_can_be_entered_from_outside_the_stock_and_finished_in_several_laps() {
    let reg = registry();
    let kernel = kmb_bundle::kernel();
    let kernel = kernel.as_ref();
    // ストックの外から入る: 最初の下りはストックの外(ストックは部品の 2 mm 外まで)
    let mut cam = reference_cam();
    let mut c = feature_data(&cam, cam.contour);
    c["entry_kind"] = serde_json::json!("outside");
    set_data(&mut cam.part.doc, cam.contour, c);
    let s = recompute_ok(&mut cam, &reg, kernel);
    let tp = s.paths.iter().find(|p| p.name == "外形").unwrap();
    let pts = cuts_world(&s.info, tp);
    let p = pts[0].0;
    assert!(
        p[0] < -2.0 || p[0] > 62.0 || p[1] < -2.0 || p[1] > 42.0,
        "{p:?}"
    );
    assert_eq!(pts[0].1, Some(kmb_toolpath::Role::Entry));
    let sim = simulate(kernel, &s, SimOptions::default()).unwrap();
    assert!(sim.is_ok(), "{}", sim.summary());

    // 横の荒取り 2 周、仕上げ 2 周ともう 1 周、重ね、45° のリードと直線、三角のタブ
    let mut cam = reference_cam();
    let mut c = feature_data(&cam, cam.contour);
    for (k, v) in [
        ("rough_passes", serde_json::json!(2)),
        ("finish", serde_json::json!("0.3 mm")),
        ("finish_passes", serde_json::json!(2)),
        ("spring_pass", serde_json::json!(true)),
        ("overlap", serde_json::json!("1 mm")),
        ("finish_feed", serde_json::json!("300")),
        ("lead", serde_json::json!("2 mm")),
        ("lead_angle", serde_json::json!("45 deg")),
        ("lead_line", serde_json::json!("1 mm")),
        ("tabs", serde_json::json!(4)),
        ("tab_shape", serde_json::json!("triangle")),
    ] {
        c[k] = v;
    }
    set_data(&mut cam.part.doc, cam.contour, c);
    let s = recompute_ok(&mut cam, &reg, kernel);
    let tp = s.paths.iter().find(|p| p.name == "外形").unwrap();
    let r = tp.tool.radius();
    let pts = cuts_world(&s.info, tp);
    let left = |d: f64| pts.iter().filter(|(p, _)| (p[0] + d).abs() < 1e-3).count();
    // 粗削りは壁から 0.3 mm と、さらに工具径の半分外。仕上げは 0.15 mm と壁(2 回)
    assert!(left(r + 0.3) > 0 && left(r + 0.3 + r) > 0);
    assert!(left(r + 0.15) > 0 && left(r) > 0);
    // 仕上げは決めた送りで、リードの印が付く
    assert!(tp.moves.iter().any(
        |m| matches!(m, kmb_toolpath::Move::Linear { feed, .. } if (*feed - 300.0).abs() < 1e-9)
    ));
    assert!(tp.roles().contains(&Some(kmb_toolpath::Role::Lead)));
    let sim = simulate(kernel, &s, SimOptions::default()).unwrap();
    assert!(sim.is_ok(), "{}", sim.summary());
}

#[test]
fn a_pocket_can_be_entered_straight_down_or_from_a_predrilled_hole() {
    let mut cam = reference_cam();
    let pocket = cam.pocket;
    let mut d = feature_data(&cam, pocket);
    d["entry_kind"] = serde_json::json!("plunge");
    set_data(&mut cam.part.doc, pocket, d);
    let (_, tp, _) = pocket_path(&mut cam, pocket);
    let roles = tp.roles();
    // 真下: 入りの動きは真下へ下りる直線だけ
    let mut cur = [0.0, 0.0, f64::INFINITY];
    for (m, r) in tp.moves.iter().zip(&roles) {
        let to = m.to();
        if *r == Some(kmb_toolpath::Role::Entry) {
            assert!(
                matches!(m, kmb_toolpath::Move::Linear { .. })
                    && (to[0] - cur[0]).hypot(to[1] - cur[1]) < 1e-9,
                "{m:?}"
            );
        }
        cur = to;
    }
    assert!(roles.contains(&Some(kmb_toolpath::Role::Entry)));
    // 下穴: 入る位置がなければ注意を出す
    let mut d = feature_data(&cam, pocket);
    d["entry_kind"] = serde_json::json!("predrill");
    set_data(&mut cam.part.doc, pocket, d);
    let (info, _, warnings) = pocket_path(&mut cam, pocket);
    assert!(warnings.iter().any(|w| w.contains("下穴")), "{warnings:?}");
    // 入る位置を置くと、そこで真下に下りる
    let top = info.to_world([0.0, 0.0, info.stock_max[2]])[2];
    run_cam(
        &mut cam,
        PLACE_ENTRY,
        pocket,
        serde_json::json!({"point": [15.5, 14.0, top]}),
    );
    let (info, tp, warnings) = pocket_path(&mut cam, pocket);
    assert!(!warnings.iter().any(|w| w.contains("下穴")), "{warnings:?}");
    let e = [15.5 - info.origin[0], 14.0 - info.origin[1]];
    let down = tp
        .moves
        .iter()
        .zip(tp.roles())
        .find(|(_, r)| *r == Some(kmb_toolpath::Role::Entry))
        .map(|(m, _)| m.to())
        .unwrap();
    assert!((down[0] - e[0]).hypot(down[1] - e[1]) < 1e-6, "{down:?}");
}

#[test]
fn tabs_can_be_placed_by_clicking_on_the_path() {
    let m = modules();
    let (reg, kernel) = (&m.registry, m.kernel.as_ref());
    let mut cam = reference_cam();
    let contour = cam.contour;
    let report = cam.part.doc.recompute(reg, kernel);
    let cx = kmb_core::Context {
        selected: Some(contour),
        ..kmb_core::Context::new(cam.part.doc.data(), &report, &m.registry, &m.kernel)
    };
    let h = m
        .extensions
        .handles(PLACE_TABS, &cx)
        .expect("タブを置く手順");
    assert!(matches!(h[0].gesture, kmb_core::Gesture::Regions));
    assert!(h[0].picked.is_empty());
    let n = h[0].choices.len();
    assert!(n > 20, "{n}");
    // 経路の上の 2 か所をクリックして Enter
    let picks = vec![
        h[0].choices[2].value.clone(),
        h[0].choices[n / 2].value.clone(),
    ];
    let at: Vec<[f64; 2]> = picks
        .iter()
        .map(|v| serde_json::from_value(v.clone()).unwrap())
        .collect();
    run_cam(
        &mut cam,
        PLACE_TABS,
        contour,
        serde_json::json!({ "tabs": picks }),
    );
    let report = cam.part.doc.recompute(reg, kernel);
    assert_eq!(report.error_count(), 0, "{:?}", report.features);
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(contour) else {
        panic!()
    };
    // 切り抜きでもタブがあるので、タブがない注意は出ない
    let warnings = kmb_core::warnings_of(outputs);
    assert!(
        warnings.iter().all(|w| !w.contains("タブがありません")),
        "{warnings:?}"
    );
    let s = &setups(cam.part.doc.data(), &report, reg)[0];
    let name = &cam.part.doc.data().feature(contour).unwrap().name;
    let tp = s.paths.iter().find(|p| &p.name == name).unwrap();
    // タブの上(部品の下面から 1.5 mm 上)を通る所は、置いた 2 か所の近くだけ
    let reach = 2.0 + tp.tool.radius() + 0.5;
    let on_tab: Vec<[f64; 3]> = tp
        .moves
        .iter()
        .map(|m| m.to())
        .filter(|p| (p[2] + 14.5).abs() < 1e-6)
        .collect();
    assert!(!on_tab.is_empty());
    let near = |p: [f64; 3], q: [f64; 2]| (p[0] - q[0]).hypot(p[1] - q[1]) <= reach;
    assert!(
        on_tab.iter().all(|p| at.iter().any(|q| near(*p, *q))),
        "{on_tab:?} {at:?}"
    );
    assert!(
        at.iter().all(|q| on_tab.iter().any(|p| near(*p, *q))),
        "{on_tab:?} {at:?}"
    );
    let sim = simulate(kernel, s, SimOptions::default()).unwrap();
    assert!(sim.is_ok(), "{}", sim.summary());
    // もう一度開くと、置いた 2 か所を選んだまま始まる
    let cx = kmb_core::Context {
        selected: Some(contour),
        ..kmb_core::Context::new(cam.part.doc.data(), &report, &m.registry, &m.kernel)
    };
    let h = m.extensions.handles(PLACE_TABS, &cx).unwrap();
    assert_eq!(h[0].picked.len(), 2);
    // 選ばずに Enter で、手で置いたタブを消す
    run_cam(
        &mut cam,
        PLACE_TABS,
        contour,
        serde_json::json!({ "tabs": null }),
    );
    let data = &cam.part.doc.data().feature(contour).unwrap().data;
    assert!(data.get("tab_points").is_none(), "{data}");
}

#[test]
fn the_contour_rings_are_picked_by_clicking_in_the_view() {
    let m = modules();
    let (reg, kernel) = (&m.registry, m.kernel.as_ref());
    let mut cam = reference_cam();
    let contour = cam.contour;
    let report = cam.part.doc.recompute(reg, kernel);
    let cx = kmb_core::Context {
        selected: Some(contour),
        ..kmb_core::Context::new(cam.part.doc.data(), &report, &m.registry, &m.kernel)
    };
    assert!(
        m.extensions
            .actions(&cx)
            .iter()
            .any(|a| a.id == PICK_CHAINS)
    );
    let h = m.extensions.handles(PICK_CHAINS, &cx).expect("選ぶ手順");
    assert!(matches!(h[0].gesture, kmb_core::Gesture::Regions));
    // 中央の穴(直径 8、中心 (30, 20))の縁を選ぶ
    let hole = h[0]
        .choices
        .iter()
        .find(|c| {
            c.label.contains("穴の縁")
                && c.outline[0]
                    .iter()
                    .all(|p| ((p[0] - 30.0).hypot(p[1] - 20.0) - 4.0).abs() < 0.05)
        })
        .unwrap_or_else(|| {
            panic!(
                "{:?}",
                h[0].choices.iter().map(|c| &c.label).collect::<Vec<_>>()
            )
        })
        .value
        .clone();
    run_cam(
        &mut cam,
        PICK_CHAINS,
        contour,
        serde_json::json!({ "chains": [hole] }),
    );
    let s = recompute_ok(&mut cam, reg, kernel);
    let tp = s.paths.iter().find(|p| p.name == "外形").unwrap();
    let r = tp.tool.radius();
    // 穴の中だけを、穴の壁に沿って回る
    let pts = cuts_world(&s.info, tp);
    assert!(
        pts.iter()
            .all(|(p, _)| (p[0] - 30.0).hypot(p[1] - 20.0) < 4.0 - r + 1e-3),
        "穴の外を削っている"
    );
    let sim = simulate(kernel, &s, SimOptions::default()).unwrap();
    assert!(sim.is_ok(), "{}", sim.summary());
    // もう一度開くと、選んだ穴の縁を選んだまま始まる
    let report = cam.part.doc.recompute(reg, kernel);
    let cx = kmb_core::Context {
        selected: Some(contour),
        ..kmb_core::Context::new(cam.part.doc.data(), &report, &m.registry, &m.kernel)
    };
    let h = m.extensions.handles(PICK_CHAINS, &cx).unwrap();
    assert_eq!(h[0].picked, vec![hole]);
    // 選ばずに決めると部品の外形に戻る
    run_cam(&mut cam, PICK_CHAINS, contour, serde_json::Value::Null);
    assert!(feature_data(&cam, contour).get("chains").is_none());
}
