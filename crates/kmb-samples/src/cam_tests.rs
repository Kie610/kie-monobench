//! 基準部品の加工の通しの試験(kmb-cam の機能を、見本の文書で確かめる)。

use crate::reference_cam;
use kmb_cam_commands::*;
use kmb_core::{FeatureStatus, Registry};
use kmb_post_carvera_air::CarveraAir;
use kmb_strategy_adaptive2d::{ADAPTIVE, Adaptive};
use kmb_strategy_contour2d::{CONTOUR, Contour, Side};
use kmb_strategy_drill::{DRILL, Drill, PIN_HOLES, PinHoles, PinLine};
use kmb_strategy_pocket2d::Pocket;
use kmb_strategy_waterline::{WATERLINE, Waterline};

pub(crate) fn registry() -> Registry {
    kmb_bundle::registry()
}

#[test]
fn changing_the_width_rebuilds_the_toolpaths() {
    let mut cam = reference_cam();
    let reg = registry();
    let kernel = kmb_bundle::kernel();
    let kernel = kernel.as_ref();
    let before = cam.part.doc.recompute(&reg, kernel);
    let g0 = gcode(
        &setups(cam.part.doc.data(), &before, &reg)[0],
        &CarveraAir,
        &kmb_post_carvera_air::profile(),
    )
    .unwrap();
    let width = cam.part.doc.data().param_by_name("width").unwrap().id;
    cam.part
        .doc
        .apply(kmb_core::Transaction::new(
            "幅",
            vec![kmb_core::Op::SetParamExpr {
                id: width,
                expr: "75 mm".into(),
            }],
        ))
        .unwrap();
    let after = cam.part.doc.recompute(&reg, kernel);
    assert_eq!(after.error_count(), 0, "{:?}", after.features);
    let s = &setups(cam.part.doc.data(), &after, &reg)[0];
    assert!((s.info.stock_max[0] - 79.0).abs() < 1e-6);
    let g1 = gcode(s, &CarveraAir, &kmb_post_carvera_air::profile()).unwrap();
    assert_ne!(g0, g1);
    let sim = simulate(kernel, s, SimOptions::default()).unwrap();
    assert!(sim.is_ok(), "{}", sim.summary());
    assert!(sim.remaining_volume < 20.0, "{}", sim.summary());
}

#[test]
fn a_tool_too_short_for_the_depth_is_an_error() {
    let mut cam = reference_cam();
    let reg = registry();
    let kernel = kmb_bundle::kernel();
    let kernel = kernel.as_ref();
    // 外形の加工を 6 mm の工具(刃長 20 mm)から、刃の短い 2 mm の工具に替える
    let mut data = cam
        .part
        .doc
        .data()
        .feature(cam.contour)
        .unwrap()
        .data
        .clone();
    data["tool"] = serde_json::json!(3);
    cam.part
        .doc
        .apply(kmb_core::Transaction::new(
            "工具",
            vec![kmb_core::Op::SetFeatureData {
                id: cam.contour,
                data,
            }],
        ))
        .unwrap();
    let report = cam.part.doc.recompute(&reg, kernel);
    match report.feature(cam.contour) {
        Some(FeatureStatus::Error { message }) => {
            assert!(message.contains("刃長"), "{message}")
        }
        other => panic!("{other:?}"),
    }
    let s = &setups(cam.part.doc.data(), &report, &reg)[0];
    assert!(gcode(s, &CarveraAir, &kmb_post_carvera_air::profile()).is_err());
}

#[test]
fn a_part_read_from_step_is_machined_without_gouging() {
    // ほかの CAD で作った基準部品(外周の縦の角を丸めたもの)の STEP を読み込んで加工する
    let reg = registry();
    let kernel = kmb_bundle::kernel();
    let kernel = kernel.as_ref();
    let step = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../kmb-kernel-truck/testdata/reference-fillet.step"
    ))
    .unwrap();
    let mut doc = kmb_core::Document::new();
    let mut ops = Vec::new();
    let mut add =
        |doc: &mut kmb_core::Document, type_id: &str, name: &str, data: serde_json::Value| {
            let id = doc.alloc_feature_id();
            ops.push(kmb_core::Op::InsertFeature {
                index: None,
                entry: kmb_core::FeatureEntry {
                    id,
                    type_id: type_id.into(),
                    name: name.into(),
                    data,
                    suppressed: false,
                    pin_bodies: false,
                },
            });
            id
        };
    let part = add(
        &mut doc,
        kmb_cad_import_step::IMPORT_STEP,
        "読み込んだ部品",
        json(&kmb_cad_import_step::ImportStep {
            file: "reference-fillet.step".into(),
            step,
            stand_up: false,
            upright: false,
            units: true,
        }),
    );
    let tools = add(&mut doc, TOOLS, "工具", json(&ToolLibrary::starter()));
    let setup = add(&mut doc, SETUP, "セットアップ 1", json(&Setup::new(part)));
    let base = |tool| OpBase::new(setup, tools, tool);
    add(
        &mut doc,
        CONTOUR,
        "外形",
        json(&Contour {
            base: base(2),
            face: None,
            side: Side::Outside,
            extra_depth: "0.3 mm".into(),
            leave: "0 mm".into(),
            ramp_angle: None,
            tabs: 0,
            tab_width: "4 mm".into(),
            tab_height: "1.5 mm".into(),
            finish: None,
            direction: Default::default(),
            lead: None,
            chains: vec![],
            depth: None,
            entry: None,
            entry_opts: Default::default(),
            rough_passes: 0,
            rough_step: None,
            tab_shape: Default::default(),
            tab_points: Vec::new(),
            finish_opts: Default::default(),
            lead_opts: Default::default(),
            rest_tool: None,
            rest_size: Default::default(),
        }),
    );
    add(
        &mut doc,
        WATERLINE,
        "壁の仕上げ",
        json(&Waterline {
            base: base(4),
            leave: "0 mm".into(),
            step: Some("1 mm".into()),
            resolution: "0.1 mm".into(),
            depth: Some("11 mm".into()),
            ramp_angle: None,
            entry: None,
            order: Default::default(),
            limits: Default::default(),
        }),
    );
    doc.apply(kmb_core::Transaction::new("加工を追加", ops))
        .unwrap();
    let report = doc.recompute(&reg, kernel);
    for f in &report.features {
        assert!(
            matches!(f.status, FeatureStatus::Ok { .. }),
            "{} {}: {:?}",
            f.id,
            f.name,
            f.status
        );
    }
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(part) else {
        unreachable!()
    };
    let Some(kmb_core::Value::Shape(im)) = outputs.get("body") else {
        panic!("読み込んだボディがない");
    };
    let after = kernel.mass(*im).unwrap();
    assert!(
        (after.volume - 22772.540).abs() < 1e-3 * 22772.540,
        "{}",
        after.volume
    );
    let s = &setups(doc.data(), &report, &reg)[0];
    assert_eq!(s.paths.len(), 2);
    let sim = simulate(kernel, s, SimOptions::default()).unwrap();
    println!("{}", sim.summary());
    assert!(sim.is_ok(), "{}", sim.summary());
}

pub(crate) fn json<T: serde::Serialize>(v: &T) -> serde_json::Value {
    serde_json::to_value(v).expect("JSON にできる")
}

#[test]
fn cutting_conditions_follow_a_parameter() {
    let mut cam = reference_cam();
    let reg = registry();
    let kernel = kmb_bundle::kernel();
    let kernel = kernel.as_ref();
    let mut data = cam.part.doc.data().feature(cam.face).unwrap().data.clone();
    data["rpm"] = serde_json::json!("spindle * 2");
    let pid = cam.part.doc.alloc_param_id();
    cam.part
        .doc
        .apply(kmb_core::Transaction::new(
            "回転数",
            vec![
                kmb_core::Op::AddParam {
                    index: None,
                    param: kmb_core::Param::new(pid, "spindle", "5000"),
                },
                kmb_core::Op::SetFeatureData { id: cam.face, data },
            ],
        ))
        .unwrap();
    let rpm = |doc: &mut kmb_core::Document| {
        let report = doc.recompute(&reg, kernel);
        setups(doc.data(), &report, &reg)[0].paths[0].rpm
    };
    assert_eq!(rpm(&mut cam.part.doc), 10000.0);
    cam.part
        .doc
        .apply(kmb_core::Transaction::new(
            "回転数",
            vec![kmb_core::Op::SetParamExpr {
                id: pid,
                expr: "6000".into(),
            }],
        ))
        .unwrap();
    assert_eq!(rpm(&mut cam.part.doc), 12000.0);
}

#[test]
fn old_documents_with_numeric_cutting_conditions_still_load() {
    let b: OpBase = serde_json::from_value(serde_json::json!(
        {"setup": 1, "tools": 2, "tool": 1, "rpm": 12000, "stepover": 0.4}
    ))
    .unwrap();
    assert_eq!(b.rpm.as_deref(), Some("12000"));
    assert_eq!(b.stepover.as_deref(), Some("0.4"));
}

pub(crate) fn set_data(
    doc: &mut kmb_core::Document,
    id: kmb_core::FeatureId,
    data: serde_json::Value,
) {
    doc.apply(kmb_core::Transaction::new(
        "設定",
        vec![kmb_core::Op::SetFeatureData { id, data }],
    ))
    .unwrap();
}

#[test]
fn the_work_offset_is_checked_on_the_setup_not_only_at_export() {
    // Z17: 作業座標系の欄は任意の整数を受け、内蔵のポストは書き出しの時に 1 以外を誤りにしていた
    // (欄の段階では何も知らせない)
    let reg = registry();
    let kernel = kmb_bundle::kernel();
    let kernel = kernel.as_ref();
    let mut cam = reference_cam();
    let mut data = cam.part.doc.data().feature(cam.setup).unwrap().data.clone();
    data["wcs"] = serde_json::json!(7);
    set_data(&mut cam.part.doc, cam.setup, data.clone());
    let report = cam.part.doc.recompute(&reg, kernel);
    assert!(
        matches!(report.feature(cam.setup), Some(FeatureStatus::Error { message }) if message.contains("1〜6")),
        "{:?}",
        report.feature(cam.setup)
    );
    data["wcs"] = serde_json::json!(2);
    set_data(&mut cam.part.doc, cam.setup, data);
    let report = cam.part.doc.recompute(&reg, kernel);
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(cam.setup) else {
        panic!("{:?}", report.feature(cam.setup));
    };
    let w = kmb_core::warnings_of(outputs);
    assert!(w.iter().any(|w| w.contains("G55")), "{w:?}");
}

#[test]
fn the_stock_can_be_a_fixed_block_with_the_origin_on_its_bottom() {
    let mut cam = reference_cam();
    let reg = registry();
    let kernel = kmb_bundle::kernel();
    let kernel = kernel.as_ref();
    let mut data = cam.part.doc.data().feature(cam.setup).unwrap().data.clone();
    data["stock"] = serde_json::json!({
        "mode": "fixed", "width": "100 mm", "depth": "80 mm", "height": "20 mm",
        "align": "front_left", "margin_top": "1 mm", "material": "アルミ"
    });
    data["origin_z"] = serde_json::json!("bottom");
    set_data(&mut cam.part.doc, cam.setup, data);
    let report = cam.part.doc.recompute(&reg, kernel);
    assert_eq!(report.error_count(), 0, "{:?}", report.features);
    let s = &setups(cam.part.doc.data(), &report, &reg)[0];
    let (lo, hi) = (s.info.stock_min, s.info.stock_max);
    for (i, size) in [100.0, 80.0, 20.0].into_iter().enumerate() {
        assert!((hi[i] - lo[i] - size).abs() < 1e-6, "{lo:?} {hi:?}");
    }
    // 原点は手前左の下の角。部品は手前左に寄せ、上面から 1 mm 下げて置く
    assert!(lo.iter().all(|v| v.abs() < 1e-6), "{lo:?}");
    assert!((s.info.safe_z - 25.0).abs() < 1e-6);
    assert_eq!(s.info.material, "アルミ");
    // アルミのプリセットを使う(平 3.175 mm の送りは 300 mm/min)
    let t2 = s.paths.iter().find(|p| p.tool.number == 2).unwrap();
    assert_eq!(t2.rpm, 12000.0);
    let feeds: Vec<f64> = t2
        .moves
        .iter()
        .filter_map(|m| match m {
            kmb_toolpath::Move::Linear { feed, .. } => Some(*feed),
            _ => None,
        })
        .collect();
    assert!(feeds.contains(&300.0), "{feeds:?}");
    assert!(gcode(s, &CarveraAir, &kmb_post_carvera_air::profile()).is_ok());

    // 部品より小さいストックは誤り
    let mut data = cam.part.doc.data().feature(cam.setup).unwrap().data.clone();
    data["stock"]["width"] = serde_json::json!("50 mm");
    set_data(&mut cam.part.doc, cam.setup, data);
    let report = cam.part.doc.recompute(&reg, kernel);
    match report.feature(cam.setup) {
        Some(FeatureStatus::Error { message }) => assert!(message.contains("入りません")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn margins_per_side_are_rounded_and_unknown_materials_warn() {
    let mut cam = reference_cam();
    let reg = registry();
    let kernel = kmb_bundle::kernel();
    let kernel = kernel.as_ref();
    let mut data = cam.part.doc.data().feature(cam.setup).unwrap().data.clone();
    data["stock"] = serde_json::json!({
        "margin_side": "2 mm", "margin_left": "5 mm", "round_to": "10 mm", "material": "MDF"
    });
    set_data(&mut cam.part.doc, cam.setup, data);
    let report = cam.part.doc.recompute(&reg, kernel);
    assert_eq!(report.error_count(), 0, "{:?}", report.features);
    let s = &setups(cam.part.doc.data(), &report, &reg)[0];
    let size: Vec<f64> = (0..3)
        .map(|i| s.info.stock_max[i] - s.info.stock_min[i])
        .collect();
    // 60 + 5 + 2 = 67 → 70、40 + 2 + 2 = 44 → 50、15 + 1 = 16 → 20
    assert_eq!(
        size.iter()
            .map(|v| (v * 1e6).round() / 1e6)
            .collect::<Vec<_>>(),
        vec![70.0, 50.0, 20.0]
    );
    // 上面は部品の上面 + 1 mm のまま(丸めの分は下に足す)
    assert!(s.info.stock_max[2].abs() < 1e-6);
    // MDF のプリセットはないので、警告を出して既定(木材)の条件で削る
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(cam.contour) else {
        panic!()
    };
    let w = kmb_core::warnings_of(outputs);
    assert!(w.iter().any(|m| m.contains("MDF")), "{w:?}");
}

#[test]
fn a_flipped_setup_machines_the_back_without_gouging() {
    let mut cam = reference_cam();
    let reg = registry();
    let kernel = kmb_bundle::kernel();
    let kernel = kernel.as_ref();
    let mut data = cam.part.doc.data().feature(cam.setup).unwrap().data.clone();
    data["flip"] = serde_json::json!("around_y");
    data["origin"] = serde_json::json!("center");
    set_data(&mut cam.part.doc, cam.setup, data);
    // 表のポケットは裏からは削れないので止める
    for id in [cam.top_pocket, cam.pocket] {
        cam.part
            .doc
            .apply(kmb_core::Transaction::new(
                "抑制",
                vec![kmb_core::Op::SetSuppressed {
                    id,
                    suppressed: true,
                }],
            ))
            .unwrap();
    }
    let report = cam.part.doc.recompute(&reg, kernel);
    assert_eq!(report.error_count(), 0, "{:?}", report.features);
    let s = &setups(cam.part.doc.data(), &report, &reg)[0];
    assert_eq!(s.info.flip, Flip::AroundY);
    // 原点(中央・上面)は、部品の向きに戻すと板の下面の中央の下 1 mm(上の余白)
    let o = s.info.to_world([0.0; 3]);
    assert!(
        (o[0] - 30.0).abs() < 1e-6 && (o[1] - 20.0).abs() < 1e-6,
        "{o:?}"
    );
    assert!((o[2] + 1.0).abs() < 1e-6, "{o:?}");
    // 隅の穴(らせんで広げる)の経路は、部品の向きに戻すと元の穴の中に来る
    let holes = s
        .paths
        .iter()
        .find(|p| p.name == cam.part.doc.data().feature(cam.corner_holes).unwrap().name)
        .expect("隅の穴の経路");
    let lowest = holes
        .moves
        .iter()
        .map(|m| m.to())
        .min_by(|a, b| a[2].total_cmp(&b[2]))
        .unwrap();
    let w = s.info.to_world(lowest);
    let near = |x: f64, y: f64| (w[0] - x).hypot(w[1] - y) < 1.7;
    assert!(
        near(4.0, 4.0) || near(4.0, 36.0) || near(56.0, 4.0) || near(56.0, 36.0),
        "{w:?}"
    );
    let sim = simulate(kernel, s, SimOptions::default()).unwrap();
    assert!(sim.is_ok(), "{}", sim.summary());
}

#[test]
fn pin_holes_land_on_the_same_spots_when_flipped() {
    let reg = registry();
    let kernel = kmb_bundle::kernel();
    let kernel = kernel.as_ref();
    let mut spots: Vec<Vec<[f64; 2]>> = vec![];
    for flip in ["none", "around_y"] {
        let mut cam = reference_cam();
        let mut data = cam.part.doc.data().feature(cam.setup).unwrap().data.clone();
        data["flip"] = serde_json::json!(flip);
        data["stock"]["margin_front"] = serde_json::json!("15 mm");
        data["stock"]["margin_back"] = serde_json::json!("15 mm");
        set_data(&mut cam.part.doc, cam.setup, data);
        let id = cam.part.doc.alloc_feature_id();
        let pins = PinHoles {
            base: OpBase::new(cam.setup, cam.tools, 2),
            diameter: "3.175 mm".into(),
            inset: "5 mm".into(),
            below: "0.8 mm".into(),
            line: PinLine::AlongY,
        };
        cam.part
            .doc
            .apply(kmb_core::Transaction::new(
                "ピン穴",
                vec![kmb_core::Op::InsertFeature {
                    index: None,
                    entry: kmb_core::FeatureEntry {
                        id,
                        type_id: PIN_HOLES.into(),
                        name: "ピン穴".into(),
                        data: json(&pins),
                        suppressed: false,
                        pin_bodies: false,
                    },
                }],
            ))
            .unwrap();
        let report = cam.part.doc.recompute(&reg, kernel);
        assert!(
            matches!(report.feature(id), Some(FeatureStatus::Ok { .. })),
            "{flip}: {:?}",
            report.feature(id)
        );
        let s = setups(cam.part.doc.data(), &report, &reg)
            .into_iter()
            .next()
            .unwrap();
        let tp = s.paths.iter().find(|p| p.name == "ピン穴").unwrap();
        let bottom = tp
            .moves
            .iter()
            .map(|m| m.to()[2])
            .fold(f64::INFINITY, f64::min);
        assert!(
            (bottom - (s.info.stock_min[2] - 0.8)).abs() < 1e-6,
            "捨て板まで"
        );
        let mut v: Vec<[f64; 2]> = tp
            .moves
            .iter()
            .filter(|m| (m.to()[2] - bottom).abs() < 1e-9)
            .map(|m| {
                let w = s.info.to_world(m.to());
                [(w[0] * 1e3).round() / 1e3, (w[1] * 1e3).round() / 1e3]
            })
            .collect();
        v.sort_by(|a, b| a[1].total_cmp(&b[1]));
        v.dedup();
        assert_eq!(v.len(), 2, "{v:?}");
        spots.push(v);
    }
    assert_eq!(spots[0], spots[1], "表と裏で同じ位置");
    // 部品(幅 60 × 奥行 40)の中心の線の上、ストックの端から 5 mm
    assert_eq!(spots[0], vec![[30.0, -10.0], [30.0, 50.0]]);
}

#[test]
fn pin_holes_drilled_through_add_the_drill_tip() {
    // Z16: ピン穴をドリルで開ける時、穴あけの貫通と違って先の円すいの長さを足さず、
    // 捨て板の深さでは径いっぱいになっていなかった
    let reg = registry();
    let kernel = kmb_bundle::kernel();
    let kernel = kernel.as_ref();
    let mut cam = reference_cam();
    let mut lib: ToolLibrary =
        serde_json::from_value(cam.part.doc.data().feature(cam.tools).unwrap().data.clone())
            .unwrap();
    let t = lib.tools.iter_mut().find(|t| t.number == 2).unwrap();
    t.kind = ToolKind::Drill;
    t.tip_angle = 118.0;
    t.flute_length = 25.0;
    let tip = t.tip_length();
    assert!(tip > 0.5, "{tip}");
    set_data(&mut cam.part.doc, cam.tools, json(&lib));
    let mut data = cam.part.doc.data().feature(cam.setup).unwrap().data.clone();
    data["stock"]["margin_front"] = serde_json::json!("15 mm");
    data["stock"]["margin_back"] = serde_json::json!("15 mm");
    set_data(&mut cam.part.doc, cam.setup, data);
    let diameter = format!("{} mm", t_diameter(&lib, 2));
    let base = OpBase::new(cam.setup, cam.tools, 2);
    let id = insert_op(
        &mut cam,
        PIN_HOLES,
        "ピン穴",
        json(&PinHoles {
            base,
            diameter,
            inset: "5 mm".into(),
            below: "0.8 mm".into(),
            line: PinLine::AlongY,
        }),
    );
    let report = cam.part.doc.recompute(&reg, kernel);
    assert!(
        matches!(report.feature(id), Some(FeatureStatus::Ok { .. })),
        "{:?}",
        report.feature(id)
    );
    let s = &setups(cam.part.doc.data(), &report, &reg)[0];
    let tp = s.paths.iter().find(|p| p.name == "ピン穴").unwrap();
    let bottom = tp
        .moves
        .iter()
        .map(|m| m.to()[2])
        .fold(f64::INFINITY, f64::min);
    assert!(
        (bottom - (s.info.stock_min[2] - 0.8 - tip)).abs() < 1e-6,
        "先の円すいの分だけ深く: {bottom} {}",
        s.info.stock_min[2]
    );
}

pub(crate) fn t_diameter(lib: &ToolLibrary, n: u32) -> f64 {
    lib.tools.iter().find(|t| t.number == n).unwrap().diameter
}

/// 加工の機能を見本の文書の最後に足す。
pub(crate) fn insert_op(
    cam: &mut crate::ReferenceCam,
    type_id: &str,
    name: &str,
    data: serde_json::Value,
) -> kmb_core::FeatureId {
    let id = cam.part.doc.alloc_feature_id();
    cam.part
        .doc
        .apply(kmb_core::Transaction::new(
            name,
            vec![kmb_core::Op::InsertFeature {
                index: None,
                entry: kmb_core::FeatureEntry {
                    id,
                    type_id: type_id.into(),
                    name: name.into(),
                    data,
                    suppressed: false,
                    pin_bodies: false,
                },
            }],
        ))
        .unwrap();
    id
}

#[test]
fn the_back_setup_starts_from_what_the_front_setup_left() {
    let reg = registry();
    let kernel = kmb_bundle::kernel();
    let kernel = kernel.as_ref();
    let mut cam = reference_cam();
    // 裏返したセットアップで、表で貫通させた隅の穴をもう一度あける
    let mut data = cam.part.doc.data().feature(cam.setup).unwrap().data.clone();
    data["flip"] = serde_json::json!("around_y");
    let back = insert_op(&mut cam, SETUP, "裏", data);
    let drill = Drill {
        diameter: Some("corner_hole".into()),
        ..Drill::new(OpBase::new(back, cam.tools, 2))
    };
    insert_op(&mut cam, DRILL, "裏の穴", json(&drill));
    let report = cam.part.doc.recompute(&reg, kernel);
    assert_eq!(report.error_count(), 0, "{:?}", report.features);
    let all = setups(cam.part.doc.data(), &report, &reg);
    let i = all.iter().position(|s| s.setup == back).unwrap();
    let fresh = simulate(kernel, &all[i], SimOptions::default()).unwrap();
    let chained = simulate_chain(kernel, &all, i, SimOptions::default()).unwrap();
    // 表で貫通した穴は、裏では初めから空いている
    assert!(fresh.removed_volume > 10.0, "{}", fresh.removed_volume);
    assert!(
        chained.removed_volume < fresh.removed_volume * 0.1,
        "{} {}",
        chained.removed_volume,
        fresh.removed_volume
    );
    assert!(chained.stock_volume < fresh.stock_volume);
}

/// 見本のポケット `pocket` を止め、同じ面と工具の負荷制御粗取り(名前 `name`)をすぐ後に足す。
pub(crate) fn replace_with_adaptive(
    cam: &mut crate::ReferenceCam,
    pocket: kmb_core::FeatureId,
    name: &str,
) -> kmb_core::FeatureId {
    let data = cam.part.doc.data().feature(pocket).unwrap().data.clone();
    let p: Pocket = serde_json::from_value(data).unwrap();
    let adaptive = Adaptive::on_face(p.base, p.face);
    let index = cam
        .part
        .doc
        .data()
        .features
        .iter()
        .position(|f| f.id == pocket)
        .unwrap();
    let id = cam.part.doc.alloc_feature_id();
    cam.part
        .doc
        .apply(kmb_core::Transaction::new(
            "負荷一定に替える",
            vec![
                kmb_core::Op::SetSuppressed {
                    id: pocket,
                    suppressed: true,
                },
                kmb_core::Op::InsertFeature {
                    index: Some(index + 1),
                    entry: kmb_core::FeatureEntry {
                        id,
                        type_id: ADAPTIVE.into(),
                        name: name.into(),
                        data: json(&adaptive),
                        suppressed: false,
                        pin_bodies: false,
                    },
                },
            ],
        ))
        .unwrap();
    id
}

thread_local! {
    /// テストごと(スレッドごと)に 1 つのモジュール一式。文書は形をカーネルに持つので、
    /// 同じ文書を別のカーネルで計算し直さない
    static MODULES: kmb_core::Modules = kmb_bundle::modules();
}

pub(crate) fn modules() -> kmb_core::Modules {
    MODULES.with(|m| m.clone())
}

/// 拡張の操作を、選んだ機能と指定で実行し、文書の変更を当てる。
pub(crate) fn run_cam(
    cam: &mut crate::ReferenceCam,
    id: &str,
    selected: kmb_core::FeatureId,
    args: serde_json::Value,
) {
    let m = modules();
    let report = cam.part.doc.recompute(&m.registry, m.kernel.as_ref());
    let cx = kmb_core::Context {
        selected: Some(selected),
        args: &args,
        ..kmb_core::Context::new(cam.part.doc.data(), &report, &m.registry, &m.kernel)
    };
    let Ok(kmb_core::Outcome::Change { label, edits }) = m.extensions.run(id, &cx) else {
        panic!("{id} が文書を変えない");
    };
    cam.part.doc.apply_edits(&label, edits).unwrap();
}

/// 文書の工具ライブラリに工具を足す。
pub(crate) fn add_tool(cam: &mut crate::ReferenceCam, t: Tool) {
    let mut lib: ToolLibrary =
        serde_json::from_value(cam.part.doc.data().feature(cam.tools).unwrap().data.clone())
            .unwrap();
    lib.tools.push(t);
    set_data(&mut cam.part.doc, cam.tools, json(&lib));
}

#[test]
fn the_stage_band_shows_the_shape_after_each_operation() {
    let mut cam = reference_cam();
    let pocket = cam.pocket;
    let m = modules();
    let report = cam.part.doc.recompute(&m.registry, m.kernel.as_ref());
    let cx = kmb_core::Context {
        selected: Some(pocket),
        ..kmb_core::Context::new(cam.part.doc.data(), &report, &m.registry, &m.kernel)
    };
    let band = m.extensions.stages(&cx).expect("段の帯");
    assert_eq!(band.stages[0].label, "素材");
    assert!(band.stages.iter().any(|s| s.label.ends_with("ポケット T2")));
    assert_eq!(band.selected, None);
    assert!(band.note.contains("シミュレーション"), "{}", band.note);
    // 段を押すと、シミュレーションを走らせてからその段を出す
    let k = band
        .stages
        .iter()
        .position(|s| s.label.ends_with("ポケット T2"))
        .unwrap();
    let Ok(kmb_core::Outcome::Job(job)) = m.extensions.run(&band.stages[k].action, &cx) else {
        panic!("シミュレーションを走らせない")
    };
    let r = job(&kmb_core::JobControl::new());
    assert!(r.ok, "{}", r.text);
    let band = m.extensions.stages(&cx).unwrap();
    assert_eq!(band.selected, Some(k));
    let shape = m.extensions.overlay(&cx).shape.expect("段の形");
    assert_eq!(shape.label, "ポケット の後");
    assert!(shape.mesh.triangles.len() > 1000);
    // 前の段の形とは違い、ポケットの所が低い
    let Ok(kmb_core::Outcome::Message(_)) = m.extensions.run(&band.stages[k - 1].action, &cx)
    else {
        panic!("走らせ直さずに出す")
    };
    let before = m.extensions.overlay(&cx).shape.unwrap();
    assert!(before.key != shape.key);
    // ポケット(左下 (8, 8) から 15 × 12、深さ 4)の内側でいちばん高い所: 前は板の上面 10、後は底 6
    let in_pocket = |s: &kmb_core::OverlayShape| {
        s.mesh
            .positions
            .iter()
            .filter(|p| (12.0..19.0).contains(&p[0]) && (11.0..17.0).contains(&p[1]))
            .map(|p| p[2])
            .fold(f32::MIN, f32::max)
    };
    assert!(
        (in_pocket(&before) - 10.0).abs() < 1e-3,
        "{}",
        in_pocket(&before)
    );
    assert!(
        (in_pocket(&shape) - 6.0).abs() < 1e-3,
        "{}",
        in_pocket(&shape)
    );
    // 部品の形に戻す
    let clear = band.clear.clone().unwrap();
    let _ = m.extensions.run(&clear, &cx);
    assert!(m.extensions.overlay(&cx).shape.is_none());
    // 経路を変えると、古い形は出さない
    let _ = m.extensions.run(&band.stages[k].action, &cx);
    assert!(m.extensions.overlay(&cx).shape.is_some());
    let mut d = cam.part.doc.data().feature(pocket).unwrap().data.clone();
    d["leave"] = serde_json::json!("0.3 mm");
    set_data(&mut cam.part.doc, pocket, d);
    let report = cam.part.doc.recompute(&m.registry, m.kernel.as_ref());
    let cx = kmb_core::Context {
        selected: Some(pocket),
        ..kmb_core::Context::new(cam.part.doc.data(), &report, &m.registry, &m.kernel)
    };
    assert!(m.extensions.overlay(&cx).shape.is_none());
    assert_eq!(m.extensions.stages(&cx).unwrap().selected, None);
}

/// 3D の見本を計算し直し、すべての機能が通ることを確かめて、最初のセットアップを返す。
pub(crate) fn recompute_ok(
    cam: &mut crate::ReferenceCam,
    reg: &Registry,
    kernel: &dyn kmb_core::Kernel,
) -> SetupPaths {
    let report = cam.part.doc.recompute(reg, kernel);
    for f in &report.features {
        assert!(
            matches!(f.status, FeatureStatus::Ok { .. }),
            "{} {}: {:?}",
            f.id,
            f.name,
            f.status
        );
    }
    setups(cam.part.doc.data(), &report, reg).remove(0)
}

pub(crate) fn feature_data(
    cam: &crate::ReferenceCam,
    id: kmb_core::FeatureId,
) -> serde_json::Value {
    cam.part.doc.data().feature(id).unwrap().data.clone()
}

/// セットアップはボディを「作った時の最後の機能」の番号で持つので、後で設計の末尾にフィレットを
/// 足しても、フィレットの前の形を削る。黙って削らないよう、セットアップの注意と CLI・MCP の状態に
/// 受け継いでいない機能を出す。
#[test]
fn a_design_feature_added_after_the_setup_is_flagged_on_the_setup() {
    let mut cam = reference_cam();
    let m = modules();
    let check = |cam: &mut crate::ReferenceCam| {
        let report = cam.part.doc.recompute(&m.registry, m.kernel.as_ref());
        let cx = kmb_core::Context::new(cam.part.doc.data(), &report, &m.registry, &m.kernel);
        let notes = m.extensions.notes(&cx, cam.setup);
        let status = m.extensions.status(&cx);
        let setups = status
            .iter()
            .find(|(k, _)| k == "setups")
            .map(|(_, v)| v.clone())
            .unwrap();
        (notes, setups)
    };
    let (notes, setups) = check(&mut cam);
    assert!(
        !notes.iter().any(|n| n.contains("加工に入っていません")),
        "{notes:?}"
    );
    assert!(setups[0].get("warnings").is_none(), "{setups}");
    let p = cam.part.plate.0;
    let side = |i: u32| format!("f{p}:side:e{i}");
    let fillet = kmb_cad_edge::Fillet::one(
        cam.part.hole_pattern,
        kmb_cad_edge::EdgeSelection {
            edges: [(5, 6), (6, 7), (7, 8), (5, 8)]
                .iter()
                .map(|(a, b)| [side(*a), side(*b)])
                .collect(),
            faces: vec![],
            arcs: false,
        },
        "2 mm",
        false,
        kmb_cad_edge::Corner::Miter,
    );
    let id = cam.part.doc.alloc_feature_id();
    cam.part
        .doc
        .apply(kmb_core::Transaction::new(
            "後から丸める",
            vec![kmb_core::Op::InsertFeature {
                index: None,
                entry: kmb_core::FeatureEntry {
                    id,
                    type_id: kmb_cad_edge::FILLET.into(),
                    name: "後から足した丸め".into(),
                    data: serde_json::to_value(&fillet).unwrap(),
                    suppressed: false,
                    pin_bodies: false,
                },
            }],
        ))
        .unwrap();
    let (notes, setups) = check(&mut cam);
    assert!(
        notes
            .iter()
            .any(|n| n.contains("後から足した丸め") && n.contains("加工に入っていません")),
        "{notes:?}"
    );
    assert!(
        setups[0]["warnings"][0]
            .as_str()
            .is_some_and(|w| w.contains("後から足した丸め")),
        "{setups}"
    );
}
