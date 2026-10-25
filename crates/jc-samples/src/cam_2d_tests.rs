//! 2D の加工(輪郭・ポケット・面取り・彫刻・V 彫り・文字・アダプティブ)を基準部品で確かめる。

use crate::cam_tests::*;
use crate::reference_cam;
use jc_cam_commands::*;
use jc_core::FeatureStatus;
use jc_strategy_chamfer::{CHAMFER, Chamfer2d, Edges};
use jc_strategy_drill::{DRILL, Drill};
use jc_strategy_engrave::{ENGRAVE, Engrave, VCARVE, VCarve};

#[test]
fn a_v_bit_chamfers_the_edges_that_fall_away() {
    let reg = registry();
    let kernel = jc_bundle::kernel();
    let kernel = kernel.as_ref();
    let mut cam = reference_cam();
    let mut chamfer = Chamfer2d {
        base: OpBase::new(cam.setup, cam.tools, 5),
        face: None,
        edges: Edges::All,
        width: "0.5 mm".into(),
        tip_offset: "0.2 mm".into(),
        selected: Vec::new(),
    };
    // 面を選ばなければ、いちばん高い面(ボスの上面)の外周
    insert_op(&mut cam, CHAMFER, "ボスの面取り", json(&chamfer));
    // 板の上面: 外周と隅の穴は削り、ボスの根元(外側に壁が立つ)は飛ばす
    chamfer.face = Some(format!("f{}:end", cam.part.plate.0));
    let plate = insert_op(&mut cam, CHAMFER, "板の面取り", json(&chamfer));
    let report = cam.part.doc.recompute(&reg, kernel);
    assert_eq!(report.error_count(), 0, "{:?}", report.features);
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(plate) else {
        panic!()
    };
    assert!(
        jc_core::warnings_of(outputs)
            .iter()
            .any(|w| w.contains("壁")),
        "{:?}",
        jc_core::warnings_of(outputs)
    );
    let s = &setups(cam.part.doc.data(), &report, &reg)[0];
    let path = |name: &str| s.paths.iter().find(|p| p.name == name).unwrap();
    let low = |tp: &Toolpath| tp.moves.iter().map(|m| m.to()[2]).fold(f64::MAX, f64::min);
    // 90° の V ビット: 先端は面から 0.2 + 0.5 mm 下
    let boss_top = s.info.stock_max[2] - 1.0;
    assert!(
        (low(path("ボスの面取り")) - (boss_top - 0.7)).abs() < 1e-6,
        "{}",
        low(path("ボスの面取り"))
    );
    let plate_tp = path("板の面取り");
    // 板の上面の高さで回る輪の数: 外周 1 + ポケットの縁 1 + 穴(中央と四隅)5
    let zp = low(plate_tp);
    let loops = plate_tp
        .moves
        .windows(2)
        .filter(|w| w[0].to()[2] > zp + 1e-6 && (w[1].to()[2] - zp).abs() < 1e-9)
        .count();
    assert_eq!(loops, 7);
    let sim = simulate(kernel, s, SimOptions::default()).unwrap();
    assert!(sim.is_ok(), "{}", sim.summary());
}

#[test]
fn a_chamfer_needs_a_v_bit() {
    let reg = registry();
    let kernel = jc_bundle::kernel();
    let kernel = kernel.as_ref();
    let mut cam = reference_cam();
    let base = OpBase::new(cam.setup, cam.tools, 1);
    let id = insert_op(
        &mut cam,
        CHAMFER,
        "面取り",
        json(&Chamfer2d {
            base,
            face: None,
            edges: Edges::All,
            width: "0.5 mm".into(),
            tip_offset: "0.2 mm".into(),
            selected: Vec::new(),
        }),
    );
    let report = cam.part.doc.recompute(&reg, kernel);
    let Some(FeatureStatus::Error { message }) = report.feature(id) else {
        panic!("{:?}", report.feature(id))
    };
    assert!(message.contains("V ビット"), "{message}");
}

#[test]
fn a_sketch_on_the_top_face_is_engraved_along_its_lines() {
    let reg = registry();
    let kernel = jc_bundle::kernel();
    let kernel = kernel.as_ref();
    let mut cam = reference_cam();
    let plate = cam.part.plate;
    let mut s = crate::sample::SketchBuilder::new(jc_sketch::SketchPlane::face(
        plate,
        format!("f{}:end", plate.0),
    ));
    let a = s.fixed_point(26.0, 28.0);
    let b = s.fixed_point(40.0, 28.0);
    s.line(a, b);
    let c = s.fixed_point(33.0, 33.0);
    s.circle(c, "4 mm", 2.0);
    let sketch = insert_op(&mut cam, jc_sketch::SKETCH, "彫る線", json(&s.build()));
    let base = OpBase::new(cam.setup, cam.tools, 5);
    let id = insert_op(
        &mut cam,
        ENGRAVE,
        "彫刻",
        json(&Engrave {
            base,
            curves: sketch,
            depth: "0.3 mm".into(),
            side: Default::default(),
        }),
    );
    let report = cam.part.doc.recompute(&reg, kernel);
    assert!(
        matches!(report.feature(id), Some(FeatureStatus::Ok { .. })),
        "{:?}",
        report.feature(id)
    );
    let s = &setups(cam.part.doc.data(), &report, &reg)[0];
    let tp = s.paths.iter().find(|p| p.name == "彫刻").unwrap();
    let cut: Vec<[f64; 3]> = tp
        .moves
        .iter()
        .filter(|m| matches!(m, jc_toolpath::Move::Linear { .. }))
        .map(|m| s.info.to_world(m.to()))
        .collect();
    // 板の上面(高さ 10)から 0.3 mm 下を、線の上だけなぞる
    assert!(cut.iter().all(|p| (p[2] - 9.7).abs() < 1e-6), "{cut:?}");
    let on_line =
        |p: &[f64; 3]| (p[1] - 28.0).abs() < 1e-6 && (26.0 - 1e-6..=40.0 + 1e-6).contains(&p[0]);
    let on_circle = |p: &[f64; 3]| ((p[0] - 33.0).hypot(p[1] - 33.0) - 2.0).abs() < 1e-6;
    assert!(cut.iter().all(|p| on_line(p) || on_circle(p)));
    assert!(cut.iter().any(on_line) && cut.iter().any(on_circle));
    let sim = simulate(kernel, s, SimOptions::default()).unwrap();
    assert!(sim.is_ok(), "{}", sim.summary());
}

#[test]
fn a_contour_with_a_finishing_allowance_ends_with_one_full_depth_lap() {
    let reg = registry();
    let kernel = jc_bundle::kernel();
    let kernel = kernel.as_ref();
    let mut cam = reference_cam();
    let mut data = cam
        .part
        .doc
        .data()
        .feature(cam.contour)
        .unwrap()
        .data
        .clone();
    data["finish"] = serde_json::json!("0.2 mm");
    data["tabs"] = serde_json::json!(4);
    set_data(&mut cam.part.doc, cam.contour, data);
    let report = cam.part.doc.recompute(&reg, kernel);
    assert_eq!(report.error_count(), 0, "{:?}", report.features);
    let s = &setups(cam.part.doc.data(), &report, &reg)[0];
    let name = &cam.part.doc.data().feature(cam.contour).unwrap().name;
    let tp = s.paths.iter().find(|p| &p.name == name).unwrap();
    let r = tp.tool.radius();
    let xs: Vec<[f64; 3]> = tp
        .moves
        .iter()
        .filter(|m| !matches!(m, jc_toolpath::Move::Rapid { .. }))
        .map(|m| s.info.to_world(m.to()))
        .collect();
    let min_x = |pts: &[[f64; 3]]| pts.iter().map(|p| p[0]).fold(f64::MAX, f64::min);
    // 粗削りは壁から 0.2 mm 離れ、最後の 1 周は壁に沿う(板の左の壁は x = 0)
    let half = xs.len() / 2;
    assert!(
        (min_x(&xs[..half]) + r + 0.2).abs() < 1e-3,
        "{}",
        min_x(&xs[..half])
    );
    let lap: Vec<&[f64; 3]> = xs.iter().filter(|p| (p[0] + r).abs() < 1e-3).collect();
    assert!(!lap.is_empty(), "壁に沿う 1 周");
    // 仕上げの 1 周は底の深さだけ(タブの上では持ち上がる)
    let bottom = xs.iter().map(|p| p[2]).fold(f64::MAX, f64::min);
    assert!(lap.iter().any(|p| (p[2] - bottom).abs() < 1e-6));
    let sim = simulate(kernel, s, SimOptions::default()).unwrap();
    assert!(sim.is_ok(), "{}", sim.summary());
}

#[test]
fn a_pocket_with_a_wall_allowance_finishes_the_wall_at_the_floor() {
    let reg = registry();
    let kernel = jc_bundle::kernel();
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
    data["finish"] = serde_json::json!("0.2 mm");
    set_data(&mut cam.part.doc, cam.pocket, data);
    let report = cam.part.doc.recompute(&reg, kernel);
    assert_eq!(report.error_count(), 0, "{:?}", report.features);
    let s = &setups(cam.part.doc.data(), &report, &reg)[0];
    let name = &cam.part.doc.data().feature(cam.pocket).unwrap().name;
    let tp = s.paths.iter().find(|p| &p.name == name).unwrap();
    let r = tp.tool.radius();
    let pts: Vec<[f64; 3]> = tp
        .moves
        .iter()
        .filter(|m| !matches!(m, jc_toolpath::Move::Rapid { .. }))
        .map(|m| s.info.to_world(m.to()))
        .collect();
    // ポケット(左下 (8, 8) から 15 × 12)の左の壁 x = 8 に、仕上げだけが工具の半径まで寄る
    let closest = pts.iter().map(|p| p[0]).fold(f64::MAX, f64::min);
    assert!((closest - (8.0 + r)).abs() < 1e-3, "{closest}");
    let n_close = pts
        .iter()
        .filter(|p| (p[0] - (8.0 + r)).abs() < 1e-3)
        .count();
    let n_rough = pts
        .iter()
        .filter(|p| (p[0] - (8.0 + r + 0.2)).abs() < 1e-3)
        .count();
    assert!(n_close >= 1 && n_rough > n_close, "{n_close} {n_rough}");
    // 四隅は角のままなので、半径 1.59 mm の工具では 4 × r²(1 − π/4) ≒ 2.2 mm² 残る
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(cam.pocket) else {
        panic!()
    };
    let w = jc_core::warnings_of(outputs);
    let rest = w
        .iter()
        .find(|w| w.contains("削り残ります"))
        .expect("削り残しの注意");
    let expect = 4.0 * r * r * (1.0 - std::f64::consts::FRAC_PI_4);
    assert!(
        rest.contains(&format!("約 {expect:.2} mm²")),
        "{rest} {expect}"
    );
    let sim = simulate(kernel, s, SimOptions::default()).unwrap();
    assert!(sim.is_ok(), "{}", sim.summary());
}

#[test]
fn a_v_bit_spots_each_hole_with_a_small_rim_chamfer() {
    let reg = registry();
    let kernel = jc_bundle::kernel();
    let kernel = kernel.as_ref();
    let mut cam = reference_cam();
    let spot = Drill {
        diameter: Some("corner_hole".into()),
        ..Drill::new(OpBase::new(cam.setup, cam.tools, 5))
    };
    let id = insert_op(&mut cam, DRILL, "もみつけ", json(&spot));
    let report = cam.part.doc.recompute(&reg, kernel);
    assert_eq!(report.error_count(), 0, "{:?}", report.features);
    let s = &setups(cam.part.doc.data(), &report, &reg)[0];
    let tp = s.paths.iter().find(|p| p.name == "もみつけ").unwrap();
    // 四隅の穴ごとに 1 回だけ、穴の上(板の上面。ボスより低いので穴の上の送り高さまで下降の
    // 送りで下りてから)より下へ下げる。板の上面はストックの上面(ボスの上 + 1 mm)の 6 mm 下
    let plate_top = s.info.stock_max[2] - 6.0;
    let plunges = tp
        .moves
        .iter()
        .filter(|m| matches!(m, jc_toolpath::Move::Linear { .. }) && m.to()[2] < plate_top - 1e-6)
        .count();
    assert_eq!(plunges, 4);
    assert!(tp.edge_break > 0.2);
    let sim = simulate(kernel, s, SimOptions::default()).unwrap();
    assert!(sim.is_ok(), "{}", sim.summary());
    let _ = id;
}

#[test]
fn a_v_carve_cuts_each_point_as_deep_as_its_distance_from_the_edge() {
    let reg = registry();
    let kernel = jc_bundle::kernel();
    let kernel = kernel.as_ref();
    let mut cam = reference_cam();
    let plate = cam.part.plate;
    let mut s = crate::sample::SketchBuilder::new(jc_sketch::SketchPlane::face(
        plate,
        format!("f{}:end", plate.0),
    ));
    let corners = [[26.0, 26.0], [36.0, 26.0], [31.0, 30.0]];
    let p: Vec<_> = corners.iter().map(|c| s.fixed_point(c[0], c[1])).collect();
    for k in 0..3 {
        s.line(p[k], p[(k + 1) % 3]);
    }
    let sketch = insert_op(&mut cam, jc_sketch::SKETCH, "三角", json(&s.build()));
    let base = OpBase::new(cam.setup, cam.tools, 5);
    let id = insert_op(
        &mut cam,
        VCARVE,
        "V 彫り",
        json(&VCarve {
            base,
            profile: sketch,
            max_depth: "2 mm".into(),
            spacing: "0.2 mm".into(),
        }),
    );
    let report = cam.part.doc.recompute(&reg, kernel);
    assert!(
        matches!(report.feature(id), Some(FeatureStatus::Ok { .. })),
        "{:?}",
        report.feature(id)
    );
    let s = &setups(cam.part.doc.data(), &report, &reg)[0];
    let tp = s.paths.iter().find(|p| p.name == "V 彫り").unwrap();
    let dist = |q: [f64; 3]| {
        (0..3)
            .map(|k| {
                let (a, b) = (corners[k], corners[(k + 1) % 3]);
                let d = [b[0] - a[0], b[1] - a[1]];
                let t = (((q[0] - a[0]) * d[0] + (q[1] - a[1]) * d[1])
                    / (d[0] * d[0] + d[1] * d[1]))
                    .clamp(0.0, 1.0);
                (q[0] - a[0] - t * d[0]).hypot(q[1] - a[1] - t * d[1])
            })
            .fold(f64::MAX, f64::min)
    };
    // 90° の V ビット: 板の上面(高さ 10)から、縁までの距離と同じだけ下
    let cut: Vec<[f64; 3]> = tp
        .moves
        .iter()
        .filter(|m| !matches!(m, jc_toolpath::Move::Rapid { .. }))
        .map(|m| s.info.to_world(m.to()))
        .filter(|p| p[2] < 10.0 - 1e-6)
        .collect();
    assert!(!cut.is_empty());
    for q in &cut {
        assert!(
            (10.0 - q[2] - dist(*q)).abs() < 2e-3,
            "{q:?} 縁から {}",
            dist(*q)
        );
    }
    // 内接円の半径(面積 20 / 半周 11.4 ≒ 1.75)近くまで深くなる
    let deepest = cut.iter().map(|q| 10.0 - q[2]).fold(0.0, f64::max);
    assert!(deepest > 1.5 && deepest < 1.76, "{deepest}");
    let sim = simulate(kernel, s, SimOptions::default()).unwrap();
    assert!(sim.is_ok(), "{}", sim.summary());
}

/// 上面のスケッチ(長方形 25×12 と円 φ8)を V 彫りで彫る。V 彫りの経路とセットアップを返す。
fn v_carve_rectangle_and_circle(
    kernel: &dyn jc_core::Kernel,
) -> (jc_cam_commands::SetupPaths, jc_toolpath::Toolpath) {
    let reg = registry();
    let mut cam = reference_cam();
    let plate = cam.part.plate;
    let mut s = crate::sample::SketchBuilder::new(jc_sketch::SketchPlane::face(
        plate,
        format!("f{}:end", plate.0),
    ));
    // ボス(中心 47, 20、半径 5)に掛からない所
    let o = s.fixed_point(26.0, 27.0);
    s.rectangle(o, "25 mm", "12 mm", [26.0, 27.0, 25.0, 12.0]);
    let c = s.fixed_point(14.0, 30.0);
    s.circle(c, "8 mm", 4.0);
    let sketch = insert_op(&mut cam, jc_sketch::SKETCH, "長方形と円", json(&s.build()));
    let base = OpBase::new(cam.setup, cam.tools, 5);
    let id = insert_op(
        &mut cam,
        VCARVE,
        "V 彫り",
        json(&VCarve {
            base,
            profile: sketch,
            max_depth: "2 mm".into(),
            spacing: "0.1 mm".into(),
        }),
    );
    let report = cam.part.doc.recompute(&reg, kernel);
    assert!(
        matches!(report.feature(id), Some(FeatureStatus::Ok { .. })),
        "{:?}",
        report.feature(id)
    );
    let s = setups(cam.part.doc.data(), &report, &reg).remove(0);
    let tp = s.paths.iter().find(|p| p.name == "V 彫り").unwrap().clone();
    (s, tp)
}

// 同じ V 彫りを 1 回作り、シミュレーションも 1 回にして、次をまとめて確かめる:
// - 長方形と円の V 彫りが panic せずに作れ、最大の深さで止まる
// - 近い輪の間は下げたまま移り、上げずに移っても縁を越えて削らない
#[test]
fn a_v_carve_of_a_rectangle_and_a_circle_is_made_and_stays_down_between_rings() {
    // 上面のスケッチ(長方形と円)を V 彫りで選ぶと、円を内側へ寄せて消える
    // 寸前の輪で cavalier_contours の検査が panic しないこと
    let kernel = jc_bundle::kernel();
    let (s, tp) = v_carve_rectangle_and_circle(kernel.as_ref());
    // 円(半径 4)の真ん中は 90° の V ビットで 4 mm 近くまで届くが、最大の深さ 2 mm で止まる
    let deepest = tp
        .moves
        .iter()
        .map(|m| 10.0 - s.info.to_world(m.to())[2])
        .fold(0.0, f64::max);
    assert!((deepest - 2.0).abs() < 1e-6, "{deepest}");

    // Z14: V 彫りは輪ごとに安全高さへ上げて真下に下りていた(0.1 mm の間隔だと輪の数だけ上げ下げ)
    let lifts = tp
        .moves
        .iter()
        .filter(
            |m| matches!(m, jc_toolpath::Move::Rapid { to } if (to[2] - tp.safe_z).abs() < 1e-9),
        )
        .count();
    // 長方形は 60 輪、円は 40 輪。上げるのは領域を移る時と、離れた輪へ移る時だけ
    assert!(lifts <= 10, "安全高さへ {lifts} 回上げた");
    // 上げずに移っても縁を越えて削らない: 削る動きのどの点でも、V ビット(90°)の上面での半径
    // (= 深さ)が縁までの距離を超えない
    let edge = |q: [f64; 3]| {
        let rect = (q[0] - 26.0)
            .min(51.0 - q[0])
            .min(q[1] - 27.0)
            .min(39.0 - q[1]);
        let circle = 4.0 - (q[0] - 14.0).hypot(q[1] - 30.0);
        rect.max(circle)
    };
    let mut from = s.info.to_world(tp.moves[0].to());
    for m in &tp.moves {
        let to = s.info.to_world(m.to());
        if matches!(m, jc_toolpath::Move::Linear { .. }) {
            for k in 0..=8 {
                let t = k as f64 / 8.0;
                let q = [0, 1, 2].map(|i| from[i] + (to[i] - from[i]) * t);
                let depth = 10.0 - q[2];
                if depth > 1e-6 {
                    assert!(
                        depth <= edge(q) + 2e-3,
                        "{q:?} 深さ {depth} 縁まで {}",
                        edge(q)
                    );
                }
            }
        }
        from = to;
    }
    let sim = simulate(kernel.as_ref(), &s, SimOptions::default()).unwrap();
    assert!(sim.is_ok(), "{}", sim.summary());
}

#[test]
fn text_on_the_top_face_is_engraved_in_single_lines() {
    let reg = registry();
    let kernel = jc_bundle::kernel();
    let kernel = kernel.as_ref();
    let mut cam = reference_cam();
    let plate = cam.part.plate;
    let text = jc_text::Text {
        plane: jc_sketch::SketchPlane::face(plate, format!("f{}:end", plate.0)),
        text: "JC 1".into(),
        height: "5 mm".into(),
        x: "20 mm".into(),
        y: "26 mm".into(),
        font: jc_text::Font::Sans,
        ..Default::default()
    };
    let t = insert_op(&mut cam, jc_text::TEXT, "文字", json(&text));
    let base = OpBase::new(cam.setup, cam.tools, 5);
    let id = insert_op(
        &mut cam,
        ENGRAVE,
        "文字の彫刻",
        json(&Engrave {
            base,
            curves: t,
            depth: "0.2 mm".into(),
            side: Default::default(),
        }),
    );
    let report = cam.part.doc.recompute(&reg, kernel);
    assert!(
        matches!(report.feature(id), Some(FeatureStatus::Ok { .. })),
        "{:?}",
        report.feature(id)
    );
    let s = &setups(cam.part.doc.data(), &report, &reg)[0];
    let tp = s.paths.iter().find(|p| p.name == "文字の彫刻").unwrap();
    let cut: Vec<[f64; 3]> = tp
        .moves
        .iter()
        .filter(|m| matches!(m, jc_toolpath::Move::Linear { .. }))
        .map(|m| s.info.to_world(m.to()))
        .collect();
    // 板の上面(高さ 10)から 0.2 mm 下で、文字の高さ 5 mm の帯の中だけ
    assert!(cut.iter().all(|p| (p[2] - 9.8).abs() < 1e-6));
    assert!(
        cut.iter()
            .all(|p| p[0] >= 20.0 - 1e-6 && (26.0 - 2.0..=31.0 + 1e-6).contains(&p[1]))
    );
    let sim = simulate(kernel, s, SimOptions::default()).unwrap();
    assert!(sim.is_ok(), "{}", sim.summary());
}

#[test]
fn an_adaptive_clearing_keeps_the_tool_engagement_low() {
    let reg = registry();
    let kernel = jc_bundle::kernel();
    let kernel = kernel.as_ref();
    let mut cam = reference_cam();
    let report = cam.part.doc.recompute(&reg, kernel);
    let s = &setups(cam.part.doc.data(), &report, &reg)[0];
    let before = simulate(kernel, s, SimOptions::default()).unwrap();
    let name = |id| cam.part.doc.data().feature(id).unwrap().name.clone();
    let (open_name, closed_name) = (name(cam.top_pocket), name(cam.pocket));

    // 開いた上面(ボスの周り)と、閉じたポケットの両方を替える
    let (top, pocket) = (cam.top_pocket, cam.pocket);
    let open = replace_with_adaptive(&mut cam, top, "負荷一定(上面)");
    let closed = replace_with_adaptive(&mut cam, pocket, "負荷一定(ポケット)");
    let report = cam.part.doc.recompute(&reg, kernel);
    assert_eq!(report.error_count(), 0, "{:?}", report.features);
    for id in [open, closed] {
        let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(id) else {
            panic!()
        };
        // 注意は、工具が届かない内側の角の削り残しだけ
        for w in jc_core::warnings_of(outputs) {
            assert!(w.contains("届かない隅"), "{w}");
        }
    }
    let s = &setups(cam.part.doc.data(), &report, &reg)[0];
    let after = simulate(kernel, s, SimOptions::default()).unwrap();
    println!("ポケット: {}", before.summary());
    println!("負荷一定: {}", after.summary());
    assert!(after.is_ok(), "{}", after.summary());
    // 削り残しはポケットより増えない
    assert!(
        after.remaining_volume <= before.remaining_volume + 1.0,
        "{} {}",
        after.remaining_volume,
        before.remaining_volume
    );
    let e = |r: &SimReport, n: &str| r.engagement.iter().find(|e| e.op == n).cloned().unwrap();
    // 開いた面: 外から入るので全溝にならず、工具の掛かりは横の切り込み(工具径の 15 %)の角度
    // arccos(1 − 0.3) ≒ 46° 前後に収まる
    let (p, a) = (e(&before, &open_name), e(&after, "負荷一定(上面)"));
    assert!(p.slot_length > 100.0, "{p:?}");
    assert!(
        a.slot_length < 1.0 && a.max < 100.0 && a.mean < 50.0,
        "{a:?}"
    );
    // 閉じたポケット: 全溝はらせんで入る所だけ。1 段が深い(段が少ない)ので、らせんの長さの
    // 割合が大きい。全溝を除いた平均(全溝は 175° として引く)はポケットの 6 割より小さい
    let (p, a) = (e(&before, &closed_name), e(&after, "負荷一定(ポケット)"));
    println!("{p:?}\n{a:?}");
    let rest = (a.mean * a.length - 175.0 * a.slot_length) / (a.length - a.slot_length);
    assert!(rest < p.mean * 0.6, "{rest} {a:?} {p:?}");
}

/// 経路の中で、高さ `z` で切削する動きの向き(XY の符号つき面積。反時計回りが正)。
fn turning_at(tp: &jc_toolpath::Toolpath, z: f64) -> f64 {
    let mut cur = [0.0, 0.0, f64::INFINITY];
    let mut area = 0.0;
    for m in &tp.moves {
        let to = m.to();
        let cutting = !matches!(m, jc_toolpath::Move::Rapid { .. });
        if cutting && (cur[2] - z).abs() < 1e-6 && (to[2] - z).abs() < 1e-6 {
            for q in m.points(cur, 0.5) {
                area += (cur[0] * q[1] - q[0] * cur[1]) / 2.0;
                cur = q;
            }
        }
        cur = to;
    }
    area
}

/// 経路の円弧のうち、半径が `r` のものの数。
fn arcs_of_radius(tp: &jc_toolpath::Toolpath, r: f64) -> usize {
    tp.moves
        .iter()
        .filter(|m| match m {
            jc_toolpath::Move::Arc { to, center, .. } => {
                ((to[0] - center[0]).hypot(to[1] - center[1]) - r).abs() < 1e-6
            }
            _ => false,
        })
        .count()
}

#[test]
fn the_contour_and_the_pocket_finish_come_in_on_arcs_and_can_cut_conventionally() {
    let reg = registry();
    let kernel = jc_bundle::kernel();
    let kernel = kernel.as_ref();
    let run = |direction: &str| {
        let mut cam = reference_cam();
        let data =
            |cam: &crate::ReferenceCam, id| cam.part.doc.data().feature(id).unwrap().data.clone();
        let mut c = data(&cam, cam.contour);
        c["ramp_angle"] = serde_json::Value::Null;
        c["lead"] = serde_json::json!("2 mm");
        c["direction"] = serde_json::json!(direction);
        set_data(&mut cam.part.doc, cam.contour, c);
        let mut p = data(&cam, cam.pocket);
        p["finish"] = serde_json::json!("0.2 mm");
        p["lead"] = serde_json::json!("1 mm");
        p["direction"] = serde_json::json!(direction);
        set_data(&mut cam.part.doc, cam.pocket, p);
        let report = cam.part.doc.recompute(&reg, kernel);
        for f in &report.features {
            assert!(
                matches!(f.status, FeatureStatus::Ok { .. }),
                "{}: {:?}",
                f.name,
                f.status
            );
        }
        let s = setups(cam.part.doc.data(), &report, &reg).remove(0);
        let sim = simulate(kernel, &s, SimOptions::default()).unwrap();
        assert!(sim.is_ok(), "{direction}: {}", sim.summary());
        s
    };
    let climb = run("climb");
    let conv = run("conventional");
    let path = |s: &SetupPaths, name: &str| {
        s.generated(s.paths.iter().position(|p| p.name == name).unwrap())
            .clone()
    };
    for s in [&climb, &conv] {
        let c = path(s, "外形");
        // 下りた直後は、いつも入りの円弧(壁の上では下りない)
        let mut cur = [0.0, 0.0, f64::INFINITY];
        let mut plunges = 0;
        for (i, m) in c.moves.iter().enumerate() {
            let to = m.to();
            let down = matches!(m, jc_toolpath::Move::Linear { .. })
                && (to[0] - cur[0]).hypot(to[1] - cur[1]) < 1e-9
                && to[2] < cur[2] - 1e-9;
            let next_down = c.moves.get(i + 1).is_some_and(|n| {
                matches!(n, jc_toolpath::Move::Linear { to: q, .. }
                    if (q[0] - to[0]).hypot(q[1] - to[1]) < 1e-9 && q[2] < to[2])
            });
            if down && !next_down {
                plunges += 1;
                assert!(
                    matches!(c.moves.get(i + 1), Some(jc_toolpath::Move::Arc { .. })),
                    "{i} 番目で下りた後が円弧でない"
                );
            }
            cur = to;
        }
        assert!(plunges >= 2, "{plunges}");
        // 入りと抜けで、段ごとに 2 つ
        assert_eq!(arcs_of_radius(&c, 2.0), 2 * plunges);
        // ポケットの仕上げの 1 周も円弧で入って抜ける
        assert!(arcs_of_radius(&path(s, "ポケット"), 1.0) >= 2);
    }
    // 外形の外側はダウンカットなら時計回り、アップカットなら反時計回り。ポケットはその逆
    let bottom = |s: &SetupPaths, name: &str| {
        let p = path(s, name);
        p.moves
            .iter()
            .filter(|m| !matches!(m, jc_toolpath::Move::Rapid { .. }))
            .map(|m| m.to()[2])
            .fold(f64::INFINITY, f64::min)
    };
    let turn = |s: &SetupPaths, name: &str| turning_at(&path(s, name), bottom(s, name));
    assert!(turn(&climb, "外形") < 0.0 && turn(&conv, "外形") > 0.0);
    assert!(turn(&climb, "ポケット") > 0.0 && turn(&conv, "ポケット") < 0.0);
}
