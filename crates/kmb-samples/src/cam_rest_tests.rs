//! 残り削り(細い工具・V ビット・テーパーで前の工具の残りだけを削る)と工具の順を基準部品で確かめる。

use crate::cam_tests::*;
use crate::reference_cam;
use kmb_cam_commands::*;
use kmb_core::{FeatureStatus, Registry};
use kmb_strategy_adaptive2d::ADAPTIVE;
use kmb_strategy_contour2d::{CONTOUR, Contour, Side};
use kmb_strategy_pocket2d::POCKET;

#[test]
fn a_rest_pocket_with_a_thinner_tool_cuts_only_the_corners() {
    let reg = registry();
    let kernel = kmb_bundle::kernel();
    let kernel = kernel.as_ref();
    let mut cam = reference_cam();
    let mut data = cam
        .part
        .doc
        .data()
        .feature(cam.pocket)
        .unwrap()
        .data
        .clone();
    // 3.175 mm の後に、2 mm で隅だけ
    data["tool"] = serde_json::json!(3);
    data["rest_tool"] = serde_json::json!(2);
    let rest = insert_op(&mut cam, kmb_strategy_pocket2d::POCKET, "隅の残り削り", data);
    let report = cam.part.doc.recompute(&reg, kernel);
    assert_eq!(report.error_count(), 0, "{:?}", report.features);
    let s = &setups(cam.part.doc.data(), &report, &reg)[0];
    let tp = s.paths.iter().find(|p| p.name.contains("残り")).unwrap();
    let pts: Vec<[f64; 3]> = tp
        .moves
        .iter()
        .filter(|m| !matches!(m, kmb_toolpath::Move::Rapid { .. }))
        .map(|m| s.info.to_world(m.to()))
        .collect();
    // ポケット(左下 (8, 8) から 15 × 12)の四隅のそば(2 本の工具の半径の和の中)だけを削る
    let corners = [[8.0, 8.0], [23.0, 8.0], [8.0, 20.0], [23.0, 20.0]];
    let zmax = pts.iter().map(|p| p[2]).fold(f64::MIN, f64::max);
    let low: Vec<&[f64; 3]> = pts.iter().filter(|p| p[2] < zmax - 0.3).collect();
    assert!(!low.is_empty());
    for p in &low {
        let d = corners
            .iter()
            .map(|c| (p[0] - c[0]).hypot(p[1] - c[1]))
            .fold(f64::MAX, f64::min);
        assert!(d < 3.0, "{p:?} は隅から {d} mm");
    }
    for c in corners {
        assert!(
            low.iter().any(|p| (p[0] - c[0]).hypot(p[1] - c[1]) < 1.5),
            "隅 {c:?} を削らない"
        );
    }
    let sim = simulate(kernel, s, SimOptions::default()).unwrap();
    assert!(sim.is_ok(), "{}", sim.summary());
    the_previous_tool_by_size_cuts_the_same(&mut cam, &reg, kernel, rest, "隅の残り削り");

    // 前の工具が細いと誤り
    let mut data = cam
        .part
        .doc
        .data()
        .feature(cam.pocket)
        .unwrap()
        .data
        .clone();
    data["rest_tool"] = serde_json::json!(3);
    set_data(&mut cam.part.doc, cam.pocket, data);
    let report = cam.part.doc.recompute(&reg, kernel);
    assert!(report.error_count() > 0);
}

/// 前の工具を番号でなく直径とコーナー半径で打っても、同じ道を作る。見本の 2 番は直径 3.175 mm の平
fn the_previous_tool_by_size_cuts_the_same(
    cam: &mut crate::ReferenceCam,
    reg: &Registry,
    kernel: &dyn kmb_core::Kernel,
    id: kmb_core::FeatureId,
    name: &str,
) {
    let moves = |cam: &mut crate::ReferenceCam| {
        let report = cam.part.doc.recompute(reg, kernel);
        assert_eq!(report.error_count(), 0, "{:?}", report.features);
        let s = &setups(cam.part.doc.data(), &report, reg)[0];
        let tp = s.paths.iter().find(|p| p.name == name).unwrap();
        format!("{:?}", tp.moves)
    };
    let by_number = moves(cam);
    let mut data = cam.part.doc.data().feature(id).unwrap().data.clone();
    data["rest_tool"] = serde_json::Value::Null;
    data["rest_diameter"] = serde_json::json!("3.175 mm");
    set_data(&mut cam.part.doc, id, data.clone());
    assert_eq!(moves(cam), by_number, "{name}");
    // コーナー半径だけでは使えない。半径が直径の半分を超えても誤り
    for (d, r) in [(None, "0.5 mm"), (Some("3.175 mm"), "2 mm")] {
        let mut bad = data.clone();
        bad["rest_diameter"] = serde_json::json!(d);
        bad["rest_corner"] = serde_json::json!(r);
        set_data(&mut cam.part.doc, id, bad);
        let report = cam.part.doc.recompute(reg, kernel);
        assert!(
            matches!(report.feature(id), Some(FeatureStatus::Error { message, .. }) if message.contains("前の工具")),
            "{name} {d:?} {r}: {:?}",
            report.feature(id)
        );
    }
    let mut back = data;
    back["rest_tool"] = serde_json::json!(2);
    back.as_object_mut().unwrap().remove("rest_diameter");
    set_data(&mut cam.part.doc, id, back);
}

#[test]
fn a_rest_contour_with_a_thinner_tool_cuts_only_the_inside_corners() {
    let reg = registry();
    let kernel = kmb_bundle::kernel();
    let kernel = kernel.as_ref();
    let mut cam = reference_cam();
    // ポケットの壁を 3.175 mm で内側から 1 周、その後に 2 mm で隅だけ
    let wall = Contour {
        side: Side::Inside,
        chains: vec![kmb_strategy_contour2d::ChainRef::Face {
            face: format!("f{}:start", cam.part.pocket.0),
            ring: Some(0),
        }],
        ..serde_json::from_value(
            cam.part
                .doc
                .data()
                .feature(cam.contour)
                .unwrap()
                .data
                .clone(),
        )
        .unwrap()
    };
    insert_op(&mut cam, CONTOUR, "壁", json(&wall));
    let mut data = json(&wall);
    data["tool"] = serde_json::json!(3);
    data["rest_tool"] = serde_json::json!(2);
    let rest = insert_op(&mut cam, CONTOUR, "壁の残り", data);
    let report = cam.part.doc.recompute(&reg, kernel);
    assert_eq!(report.error_count(), 0, "{:?}", report.features);
    let s = &setups(cam.part.doc.data(), &report, &reg)[0];
    let tp = s.paths.iter().find(|p| p.name == "壁の残り").unwrap();
    let pts: Vec<[f64; 3]> = tp
        .moves
        .iter()
        .filter(|m| !matches!(m, kmb_toolpath::Move::Rapid { .. }))
        .map(|m| s.info.to_world(m.to()))
        .collect();
    // ポケット(左下 (8, 8) から 15 × 12)の四隅のそば(2 本の工具の半径の和の中)だけを削る
    let corners = [[8.0, 8.0], [23.0, 8.0], [8.0, 20.0], [23.0, 20.0]];
    let zmax = pts.iter().map(|p| p[2]).fold(f64::MIN, f64::max);
    let low: Vec<&[f64; 3]> = pts.iter().filter(|p| p[2] < zmax - 0.3).collect();
    assert!(!low.is_empty());
    for p in &low {
        let d = corners
            .iter()
            .map(|c| (p[0] - c[0]).hypot(p[1] - c[1]))
            .fold(f64::MAX, f64::min);
        assert!(d < 3.0, "{p:?} は隅から {d} mm");
    }
    for c in corners {
        assert!(
            low.iter().any(|p| (p[0] - c[0]).hypot(p[1] - c[1]) < 1.5),
            "隅 {c:?} を削らない"
        );
    }
    let sim = simulate(kernel, s, SimOptions::default()).unwrap();
    assert!(sim.is_ok(), "{}", sim.summary());
    the_previous_tool_by_size_cuts_the_same(&mut cam, &reg, kernel, rest, "壁の残り");

    // 部品の外形(へこみのない四角)の外側には削り残しがない(外側の仮の箱の隅を削らない)
    let mut data = cam
        .part
        .doc
        .data()
        .feature(cam.contour)
        .unwrap()
        .data
        .clone();
    // 6 mm の後に 3.175 mm(2 mm は刃長が切り抜く深さに足りない)
    data["tool"] = serde_json::json!(2);
    data["rest_tool"] = serde_json::json!(1);
    let outer = insert_op(&mut cam, CONTOUR, "外形の残り", data);
    let report = cam.part.doc.recompute(&reg, kernel);
    match report.feature(outer) {
        Some(FeatureStatus::Error { message, .. }) => {
            assert!(message.contains("削り残しはありません"), "{message}")
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_adaptive_rest_with_a_thinner_tool_cuts_only_the_corners() {
    let reg = registry();
    let kernel = kmb_bundle::kernel();
    let kernel = kernel.as_ref();
    let mut cam = reference_cam();
    let pocket = cam.pocket;
    let first = replace_with_adaptive(&mut cam, pocket, "負荷一定(ポケット)");
    // 3.175 mm の後に、2 mm で隅だけ
    let mut data = cam.part.doc.data().feature(first).unwrap().data.clone();
    data["tool"] = serde_json::json!(3);
    data["rest_tool"] = serde_json::json!(2);
    let rest = insert_op(&mut cam, ADAPTIVE, "負荷一定の残り", data);
    let report = cam.part.doc.recompute(&reg, kernel);
    assert_eq!(report.error_count(), 0, "{:?}", report.features);
    let s = &setups(cam.part.doc.data(), &report, &reg)[0];
    let tp = s.paths.iter().find(|p| p.name.contains("残り")).unwrap();
    let pts: Vec<[f64; 3]> = tp
        .moves
        .iter()
        .filter(|m| !matches!(m, kmb_toolpath::Move::Rapid { .. }))
        .map(|m| s.info.to_world(m.to()))
        .collect();
    // ポケット(左下 (8, 8) から 15 × 12)の底近くで削る所は、四隅のそばか壁の 1 周の上だけ
    let corners = [[8.0, 8.0], [23.0, 8.0], [8.0, 20.0], [23.0, 20.0]];
    let r = 1.0;
    let zmax = pts.iter().map(|p| p[2]).fold(f64::MIN, f64::max);
    let low: Vec<&[f64; 3]> = pts.iter().filter(|p| p[2] < zmax - 0.3).collect();
    assert!(!low.is_empty());
    for p in &low {
        let to_corner = corners
            .iter()
            .map(|c| (p[0] - c[0]).hypot(p[1] - c[1]))
            .fold(f64::MAX, f64::min);
        // 壁の 1 周は、壁から工具の半径の所
        let on_wall = [p[0] - 8.0, 23.0 - p[0], p[1] - 8.0, 20.0 - p[1]]
            .iter()
            .any(|d| (d - r).abs() < 0.05);
        assert!(to_corner < 3.5 || on_wall, "{p:?} は隅から {to_corner} mm");
    }
    for c in corners {
        assert!(
            low.iter().any(|p| (p[0] - c[0]).hypot(p[1] - c[1]) < 2.0),
            "隅 {c:?} を削らない"
        );
    }
    let sim = simulate(kernel, s, SimOptions::default()).unwrap();
    assert!(sim.is_ok(), "{}", sim.summary());
    println!("{}", sim.summary());
    the_previous_tool_by_size_cuts_the_same(&mut cam, &reg, kernel, rest, "負荷一定の残り");
}

#[test]
fn a_thinner_tool_is_added_to_the_pockets_tool_order() {
    let mut cam = reference_cam();
    let pocket = cam.pocket;
    let m = modules();
    let report = cam.part.doc.recompute(&m.registry, m.kernel.as_ref());
    let cx = kmb_core::Context {
        selected: Some(pocket),
        ..kmb_core::Context::new(cam.part.doc.data(), &report, &m.registry, &m.kernel)
    };
    let acts = m.extensions.actions(&cx);
    let add: Vec<_> = acts.iter().filter(|a| a.id.starts_with(ADD_TOOL)).collect();
    assert!(!add.is_empty());
    let thin = add
        .iter()
        .find(|a| a.disabled.is_none())
        .expect("細い平の工具を足せる");
    let id = thin.id.clone();
    run_cam(&mut cam, &id, pocket, serde_json::Value::Null);
    let n: u32 = id.strip_prefix(ADD_TOOL).unwrap().parse().unwrap();
    let added = cam.part.doc.data().features.last().unwrap();
    assert_eq!(added.data["tool"], serde_json::json!(n));
    assert_eq!(added.data["rest_tool"], serde_json::json!(2));
    let added = added.id;
    let report = cam.part.doc.recompute(&m.registry, m.kernel.as_ref());
    assert_eq!(report.error_count(), 0, "{:?}", report.features);
    let cx = kmb_core::Context::new(cam.part.doc.data(), &report, &m.registry, &m.kernel);
    let s = m.extensions.summary(&cx, added).unwrap();
    assert!(
        s.contains("工具の順番: T2") && s.contains(&format!("▶T{n}")),
        "{s}"
    );
}

/// V 30°(上端 3.175 mm、刃長 10 mm)と、テーパー(先 0.5 mm、片側 5°)。
fn thin_tools() -> (Tool, Tool) {
    let base = ToolLibrary::starter().tools[1].clone();
    let v = Tool {
        number: 7,
        name: "V 30°".into(),
        kind: ToolKind::VBit,
        tip_angle: 30.0,
        flute_length: 10.0,
        flutes: 1,
        ..base.clone()
    };
    let t = Tool {
        number: 8,
        name: "テーパー 0.5 mm".into(),
        kind: ToolKind::TaperEnd,
        tip_angle: 10.0,
        tip_diameter: Some(0.5),
        flute_length: 12.0,
        ..base
    };
    (v, t)
}

#[test]
fn a_v_bit_and_a_taper_clean_the_corners_after_a_flat_endmill_without_gouging() {
    let reg = registry();
    let kernel = kmb_bundle::kernel();
    let kernel = kernel.as_ref();
    let corners = [[8.0, 8.0], [23.0, 8.0], [8.0, 20.0], [23.0, 20.0]];
    // 平 3.175 mm の後に、V 30°(先がとがる)か、テーパー(先 0.5 mm)で隅だけ
    for (number, name) in [(7, "V の残り"), (8, "テーパーの残り")] {
        let mut cam = reference_cam();
        let (v, taper) = thin_tools();
        add_tool(&mut cam, v);
        add_tool(&mut cam, taper);
        let mut d = cam
            .part
            .doc
            .data()
            .feature(cam.pocket)
            .unwrap()
            .data
            .clone();
        d["tool"] = serde_json::json!(number);
        d["rest_tool"] = serde_json::json!(2);
        insert_op(&mut cam, kmb_strategy_pocket2d::POCKET, name, d);
        let report = cam.part.doc.recompute(&reg, kernel);
        assert_eq!(report.error_count(), 0, "{:?}", report.features);
        let s = &setups(cam.part.doc.data(), &report, &reg)[0];
        let top = s.info.stock_max[2];
        let tp = s.paths.iter().find(|p| p.name == name).unwrap();
        let cut: Vec<[f64; 3]> = tp
            .moves
            .iter()
            .filter(|m| !matches!(m, kmb_toolpath::Move::Rapid { .. }))
            .map(|m| m.to())
            .filter(|p| p[2] < top - 1e-9)
            .collect();
        assert!(!cut.is_empty(), "{name}");
        // 隅のそばだけを削り、深い段ほど壁から離れる(壁の上端での工具の太さ r(d) だけ。
        // 壁の上端は板の上面で、ストックの上面の 6 mm 下)。壁の上端より上は削らない
        let wall_top = top - 6.0;
        assert!(cut.iter().all(|p| p[2] < wall_top + 1e-9), "{name}");
        for p in &cut {
            let w = s.info.to_world(*p);
            let dc = corners
                .iter()
                .map(|c| (w[0] - c[0]).hypot(w[1] - c[1]))
                .fold(f64::MAX, f64::min);
            assert!(dc < 4.0, "{name}: {w:?} は隅から {dc} mm");
            let r = tp.tool.radius_at((wall_top - p[2]).max(0.0)).unwrap();
            let wall = (w[0] - 8.0)
                .min(23.0 - w[0])
                .min(w[1] - 8.0)
                .min(20.0 - w[1]);
            assert!(
                wall >= r - 1e-3,
                "{name}: {w:?} は壁から {wall}、工具の太さ {r}"
            );
        }
        let sim = simulate(kernel, s, SimOptions::default()).unwrap();
        assert!(sim.is_ok(), "{name}: {}", sim.summary());
        let v = sim.removed_by_op.iter().find(|(n, _)| n == name).unwrap().1;
        assert!(v > 0.05, "{name} が削らない: {v} {:?}", sim.removed_by_op);
        // 先が前の工具より太いと誤り(V の後のテーパーは削る所がない)
        if number == 7 {
            let mut d = cam
                .part
                .doc
                .data()
                .feature(cam.pocket)
                .unwrap()
                .data
                .clone();
            d["tool"] = serde_json::json!(8);
            d["rest_tool"] = serde_json::json!(7);
            let bad = insert_op(&mut cam, kmb_strategy_pocket2d::POCKET, "逆", d);
            let report = cam.part.doc.recompute(&reg, kernel);
            let Some(FeatureStatus::Error { message }) = report.feature(bad) else {
                panic!("誤りにならない")
            };
            assert!(message.contains("先より太く"), "{message}");
        }
    }
}

#[test]
fn a_v_bit_after_an_adaptive_clearing_is_added_as_a_rest_pocket() {
    let mut cam = reference_cam();
    let (v, _) = thin_tools();
    add_tool(&mut cam, v);
    let pocket = cam.pocket;
    let adaptive = replace_with_adaptive(&mut cam, pocket, "荒削り");
    let m = modules();
    let report = cam.part.doc.recompute(&m.registry, m.kernel.as_ref());
    let cx = kmb_core::Context {
        selected: Some(adaptive),
        ..kmb_core::Context::new(cam.part.doc.data(), &report, &m.registry, &m.kernel)
    };
    let acts = m.extensions.actions(&cx);
    let add = acts
        .iter()
        .find(|a| a.id == format!("{ADD_TOOL}7"))
        .expect("V を足す操作");
    assert!(add.disabled.is_none(), "{:?}", add.disabled);
    // ボールは足せない
    let ball = acts
        .iter()
        .find(|a| a.id == format!("{ADD_TOOL}4"))
        .unwrap();
    assert!(ball.disabled.is_some());
    run_cam(
        &mut cam,
        &format!("{ADD_TOOL}7"),
        adaptive,
        serde_json::Value::Null,
    );
    let added = cam.part.doc.data().features.last().unwrap().clone();
    assert_eq!(added.type_id, POCKET);
    assert_eq!(added.data["rest_tool"], serde_json::json!(2));
    let report = cam.part.doc.recompute(&m.registry, m.kernel.as_ref());
    assert_eq!(report.error_count(), 0, "{:?}", report.features);
    let cx = kmb_core::Context::new(cam.part.doc.data(), &report, &m.registry, &m.kernel);
    let s = m.extensions.summary(&cx, added.id).unwrap();
    assert!(s.contains("工具の順番: T2") && s.contains("▶T7"), "{s}");
}
