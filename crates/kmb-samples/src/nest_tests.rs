//! 確かめること: 派生した写しの配置、寸法の再計算、元の面と辺の参照の全写しへの適用。

use kmb_cam_setup::nest::{NestLayout, SHEET_GAP};
use kmb_cam_setup::{SETUP, Setup};
use kmb_core::{FeatureId, FeatureStatus, Op, Payload, Transaction};
use kmb_toolpath::Move;

fn layout(report: &kmb_core::RecomputeReport, id: FeatureId) -> NestLayout {
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(id) else {
        panic!("配置の評価: {:?}", report.feature(id));
    };
    NestLayout::from_value(&outputs["nest"]).expect("配置の出力")
}

#[test]
fn nesting_commands_use_numeric_defaults_and_convert_declared_length_units() {
    let m = kmb_bundle::modules();
    for (args, edge, spacing) in [
        (serde_json::json!({}), "5 mm", "8 mm"),
        (
            serde_json::json!({"edge":"1 mm","spacing":"0.25 in"}),
            "1 mm",
            "6.35 mm",
        ),
    ] {
        let mut doc = crate::reference_document();
        let report = doc.recompute(&m.registry, m.kernel.as_ref());
        let selected=doc.data().features.iter().rev().find(|f|matches!(report.feature(f.id),Some(FeatureStatus::Ok { outputs,.. }) if outputs.contains_key("body"))).unwrap().id;
        let mut before = doc.data().clone();
        let cx = kmb_core::Context {
            selected: Some(selected),
            args: &args,
            ..kmb_core::Context::new(doc.data(), &report, &m.registry, &m.kernel)
        };
        assert!(
            m.extensions
                .actions(&cx)
                .iter()
                .filter(|a| a.id.starts_with("cam.nest."))
                .all(|a| !a.is_at(kmb_core::Place::Panel)),
            "配置以外の編集パネルに配置のボタンを出さない"
        );
        let kmb_core::Outcome::Change { label, edits } = m
            .extensions
            .run("cam.nest.create", &cx)
            .expect("宣言した既定と長さ")
        else {
            panic!("配置の作成は取り消せる変更を返す");
        };
        doc.apply_edits(&label, edits).unwrap();
        let data = &doc.data().features.last().unwrap().data;
        for field in &m.registry.descriptor(SETUP).unwrap().fields {
            assert!(
                field.applies(data) || field.get(data).is_none(),
                "作成直後に画面が不要な欄を消して別の変更を作らない: {}",
                field.key
            );
        }
        let setup: Setup = serde_json::from_value(data.clone()).unwrap();
        let nest = setup.nest.unwrap();
        assert_eq!(nest.edge, edge);
        assert_eq!(nest.spacing, spacing);
        assert_eq!(nest.grid, "1 mm");
        assert_eq!(setup.stock.width, "100 mm");
        assert_eq!(setup.stock.depth, "100 mm");
        assert_eq!(setup.stock.height, "20 mm");
        let report = doc.recompute(&m.registry, m.kernel.as_ref());
        assert_eq!(report.error_count(), 0);
        let cx = kmb_core::Context {
            selected: Some(doc.data().features.last().unwrap().id),
            ..kmb_core::Context::new(doc.data(), &report, &m.registry, &m.kernel)
        };
        for id in ["cam.nest.add_part", "cam.nest.remove_part"] {
            assert!(
                m.extensions
                    .actions(&cx)
                    .iter()
                    .any(|a| a.id == id && a.is_at(kmb_core::Place::Panel))
            );
        }
        let kmb_core::Outcome::Change { label, edits } =
            m.extensions.run("cam.add.contour", &cx).unwrap()
        else {
            panic!("加工を足す変更");
        };
        let mut probe = kmb_core::Document::from_json(&doc.to_json()).unwrap();
        probe.apply_edits(&label, edits).unwrap();
        let cx = kmb_core::Context {
            selected: Some(probe.data().features.last().unwrap().id),
            ..kmb_core::Context::new(probe.data(), &report, &m.registry, &m.kernel)
        };
        assert!(
            m.extensions
                .actions(&cx)
                .iter()
                .filter(|a| a.id.starts_with("cam.nest."))
                .all(|a| !a.is_at(kmb_core::Place::Panel)),
            "配置配下の加工の欄にも部品編集のボタンを混ぜない"
        );
        assert!(doc.undo());
        // 割り当てた ID は取り消し後にも再利用しない。
        assert!(doc.data().next_id >= before.next_id);
        before.next_id = doc.data().next_id;
        assert_eq!(doc.data(), &before);
    }
}

#[test]
fn rectangle_samples_have_one_and_two_sheets_and_hold_named_copies() {
    let reg = kmb_bundle::registry();
    let kernel = kmb_bundle::kernel();
    for (two, count) in [(false, 1), (true, 2)] {
        let mut doc = crate::nesting_boxes_document(two);
        let id = doc
            .data()
            .features
            .iter()
            .find(|f| f.type_id == SETUP)
            .unwrap()
            .id;
        let report = doc.recompute(&reg, kernel.as_ref());
        assert_eq!(report.error_count(), 0, "{:?}", report.features);
        let layout = layout(&report, id);
        assert_eq!(layout.sheets.len(), count, "{layout:?}");
        assert!(layout.unplaced.is_empty());
        let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(id) else {
            unreachable!();
        };
        assert_eq!(
            outputs.keys().filter(|k| k.starts_with("nest.")).count(),
            12
        );
        let setup = kmb_cam_setup::SetupInfo::from_value(&outputs["setup"]).unwrap();
        assert_eq!(setup.more_bodies.len(), 11);
        assert_eq!(setup.sections.as_ref().unwrap().list.len(), count);
        for sheet in &layout.sheets {
            for part in &sheet.parts {
                let n = layout
                    .sheets
                    .iter()
                    .take_while(|s| s.index < sheet.index)
                    .map(|s| s.parts.len())
                    .sum::<usize>()
                    + sheet.parts.iter().position(|p| p == part).unwrap()
                    + 1;
                let kmb_core::Value::Shape(handle) = outputs[&format!("nest.{n}")] else {
                    panic!("写し");
                };
                let Some(FeatureStatus::Ok {
                    outputs: source_outputs,
                    ..
                }) = report.feature(part.body)
                else {
                    panic!("元のボディ");
                };
                let kmb_core::Value::Shape(original) = source_outputs["body"] else {
                    panic!("元の形");
                };
                assert_eq!(
                    kernel.mesh(handle, 0.01).unwrap().face_names,
                    kernel.mesh(original, 0.01).unwrap().face_names
                );
                let (min, max) = kernel.bounds(handle).unwrap();
                assert!(min[0] >= f64::from(sheet.index - 1) * (sheet.size[0] + SHEET_GAP) - 1e-5);
                assert!(
                    max[0]
                        <= f64::from(sheet.index - 1) * (sheet.size[0] + SHEET_GAP)
                            + sheet.size[0]
                            + 1e-5
                );
            }
        }
    }
}

#[test]
fn changing_a_part_dimension_recomputes_layout_without_duplicating_operations() {
    let mut doc = crate::nesting_boxes_document(true);
    let reg = kmb_bundle::registry();
    let kernel = kmb_bundle::kernel();
    let id = doc
        .data()
        .features
        .iter()
        .find(|f| f.type_id == SETUP)
        .unwrap()
        .id;
    let before = doc.recompute(&reg, kernel.as_ref());
    let old = layout(&before, id);
    let parameter = doc.data().param_by_name("部品の幅").unwrap().id;
    let features = doc.data().features.len();
    doc.apply(Transaction::new(
        "部品の幅",
        vec![Op::SetParamExpr {
            id: parameter,
            expr: "52 mm".into(),
        }],
    ))
    .unwrap();
    let after = doc.recompute(&reg, kernel.as_ref());
    assert_eq!(after.error_count(), 0, "{:?}", after.features);
    let new = layout(&after, id);
    assert_ne!(
        serde_json::to_string(&old).unwrap(),
        serde_json::to_string(&new).unwrap()
    );
    assert_eq!(doc.data().features.len(), features);
}

#[test]
fn one_contour_operation_reaches_every_nested_part() {
    let mut doc = crate::nesting_boxes_document(true);
    let reg = kmb_bundle::registry();
    let kernel = kmb_bundle::kernel();
    let id = doc
        .data()
        .features
        .iter()
        .find(|f| f.type_id == SETUP)
        .unwrap()
        .id;
    let report = doc.recompute(&reg, kernel.as_ref());
    let layout = layout(&report, id);
    let setups = kmb_cam_commands::setups(doc.data(), &report, &reg);
    let s = &setups[0];
    assert_eq!(s.paths.len(), 1);
    let radius = s.paths[0].tool.radius();
    for sheet in &layout.sheets {
        for part in &sheet.parts {
            let x = f64::from(sheet.index - 1) * (sheet.size[0] + SHEET_GAP) + part.position[0];
            let y = part.position[1];
            assert!(
                s.paths[0].moves.iter().any(|m| {
                    !matches!(m, Move::Rapid { .. })
                        && m.to()[2] <= -4.99
                        && m.to()[0] >= x - radius - 1e-4
                        && m.to()[0] <= x + part.size[0] + radius + 1e-4
                        && m.to()[1] >= y - radius - 1e-4
                        && m.to()[1] <= y + part.size[1] + radius + 1e-4
                }),
                "板 {} の部品 {} の {} 個目に加工が届きません",
                sheet.index,
                part.part,
                part.instance
            );
        }
    }
}

#[test]
fn shape_sample_preserves_instances_and_shows_its_actual_grid() {
    let mut doc = crate::nesting_shapes_document();
    let reg = kmb_bundle::registry();
    let kernel = kmb_bundle::kernel();
    let id = doc
        .data()
        .features
        .iter()
        .find(|f| f.type_id == SETUP)
        .unwrap()
        .id;
    let report = doc.recompute(&reg, kernel.as_ref());
    assert_eq!(report.error_count(), 0, "{:?}", report.features);
    let layout = layout(&report, id);
    assert_eq!(
        layout.sheets.iter().map(|s| s.parts.len()).sum::<usize>(),
        7
    );
    assert!(layout.unplaced.is_empty());
    assert_eq!(layout.grid, 1.0);
}

#[test]
fn nesting_rejects_incompatible_setup_and_only_blocks_its_downstream() {
    let mut doc = crate::nesting_boxes_document(false);
    let reg = kmb_bundle::registry();
    let kernel = kmb_bundle::kernel();
    let entry = doc
        .data()
        .features
        .iter()
        .find(|f| f.type_id == SETUP)
        .unwrap();
    let id = entry.id;
    let mut setup: Setup = serde_json::from_value(entry.data.clone()).unwrap();
    setup.flip = kmb_cam_setup::Flip::AroundX;
    doc.apply(Transaction::new(
        "向き",
        vec![Op::SetFeatureData {
            id,
            data: serde_json::to_value(setup).unwrap(),
        }],
    ))
    .unwrap();
    let report = doc.recompute(&reg, kernel.as_ref());
    assert!(matches!(
        report.feature(id),
        Some(FeatureStatus::Error { .. })
    ));
    assert!(
        doc.data()
            .features
            .iter()
            .filter(|f| f.type_id == kmb_cad_extrude::EXTRUDE)
            .all(|f| matches!(report.feature(f.id), Some(FeatureStatus::Ok { .. })))
    );
}

#[test]
fn selecting_an_original_face_and_edge_applies_to_every_copy_of_that_body() {
    let mut doc = crate::nesting_boxes_document(false);
    let reg = kmb_bundle::registry();
    let kernel = kmb_bundle::kernel();
    let entry = doc
        .data()
        .features
        .iter()
        .find(|f| f.type_id == SETUP)
        .unwrap();
    let sid = entry.id;
    let setup: Setup = serde_json::from_value(entry.data.clone()).unwrap();
    let body = setup.nest.as_ref().unwrap().parts[0].body;
    let contour = doc
        .data()
        .features
        .iter()
        .find(|f| f.type_id == kmb_strategy_contour2d::CONTOUR)
        .unwrap();
    let cid = contour.id;
    let mut data = contour.data.clone();
    data["face"] = serde_json::json!(format!("f{}:start", body.0));
    let tools = doc
        .data()
        .features
        .iter()
        .find(|f| f.type_id == kmb_cam_commands::TOOLS)
        .unwrap()
        .id;
    let trace = doc.alloc_feature_id();
    doc.apply(Transaction::new(
        "元の面と辺",
        vec![
            Op::SetFeatureData { id: cid, data },
            Op::InsertFeature {
                index: None,
                entry: kmb_core::FeatureEntry {
                    id: trace,
                    type_id: kmb_strategy_engrave::TRACE.into(),
                    name: "元の辺".into(),
                    suppressed: false,
                    pin_bodies: false,
                    data: serde_json::to_value(kmb_strategy_engrave::Trace {
                        edges: vec![kmb_core::Ref::Edge {
                            feature: body,
                            faces: [format!("f{}:start", body.0), format!("f{}:side:e5", body.0)],
                        }],
                        ..kmb_strategy_engrave::Trace::new(kmb_cam_commands::OpBase::new(
                            sid, tools, 1,
                        ))
                    })
                    .unwrap(),
                },
            },
        ],
    ))
    .unwrap();
    let report = doc.recompute(&reg, kernel.as_ref());
    assert_eq!(report.error_count(), 0, "{:?}", report.features);
    let layout = layout(&report, sid);
    let setups = kmb_cam_commands::setups(doc.data(), &report, &reg);
    let s = &setups[0];
    for op in [cid, trace] {
        let path = &s.paths[s.sources.iter().position(|p| p.id == op).unwrap()];
        let radius = path.tool.radius();
        for sheet in &layout.sheets {
            for part in sheet.parts.iter().filter(|p| p.body == body) {
                let [x, y] = part.position;
                assert!(
                    path.moves.iter().any(|m| !matches!(m, Move::Rapid { .. })
                        && m.to()[2] < -4.99
                        && m.to()[0] >= x - radius - 1e-4
                        && m.to()[0] <= x + part.size[0] + radius + 1e-4
                        && m.to()[1] >= y - radius - 1e-4
                        && m.to()[1] <= y + part.size[1] + radius + 1e-4),
                    "加工 {op} / 写し {}",
                    part.instance
                );
            }
        }
    }
}

fn reference_pair() -> crate::ReferenceCam {
    let mut cam = crate::reference_cam();
    let entry = cam.part.doc.data().feature(cam.setup).unwrap();
    let mut setup: Setup = serde_json::from_value(entry.data.clone()).unwrap();
    setup.stock.mode = kmb_cam_setup::StockMode::Fixed;
    setup.stock.width = "180 mm".into();
    setup.stock.depth = "100 mm".into();
    setup.stock.height = "16 mm".into();
    setup.nest = Some(kmb_cam_setup::nest::Nest::new(vec![
        kmb_cam_setup::nest::NestPart {
            body: setup.body,
            count: 2,
            up: Default::default(),
            rotate: kmb_cam_setup::nest::NestRotate::Fixed,
        },
    ]));
    cam.part
        .doc
        .apply(Transaction::new(
            "2 個の部品",
            vec![Op::SetFeatureData {
                id: cam.setup,
                data: serde_json::to_value(setup).unwrap(),
            }],
        ))
        .unwrap();
    cam
}

#[test]
fn selecting_an_original_pocket_floor_machines_both_copies() {
    let mut cam = reference_pair();
    let reg = kmb_bundle::registry();
    let kernel = kmb_bundle::kernel();
    let report = cam.part.doc.recompute(&reg, kernel.as_ref());
    assert_eq!(report.error_count(), 0, "{:?}", report.features);
    let layout = layout(&report, cam.setup);
    let setups = kmb_cam_commands::setups(cam.part.doc.data(), &report, &reg);
    let s = &setups[0];
    let path = &s.paths[s.sources.iter().position(|p| p.id == cam.pocket).unwrap()];
    for sheet in &layout.sheets {
        for part in &sheet.parts {
            let x = part.position[0] - s.info.origin[0];
            let y = part.position[1] - s.info.origin[1];
            assert!(
                path.moves.iter().any(|m| !matches!(m, Move::Rapid { .. })
                    && m.to()[2] < -1.0
                    && m.to()[0] >= x
                    && m.to()[0] <= x + part.size[0]
                    && m.to()[1] >= y
                    && m.to()[1] <= y + part.size[1]),
                "写し {} のポケット",
                part.instance
            );
        }
    }
}

#[test]
fn an_open_floor_does_not_gouge_the_other_nested_copy() {
    let mut cam = reference_pair();
    let reg = kmb_bundle::registry();
    let kernel = kmb_bundle::kernel();
    let report = cam.part.doc.recompute(&reg, kernel.as_ref());
    assert_eq!(report.error_count(), 0, "{:?}", report.features);
    let setups = kmb_cam_commands::setups(cam.part.doc.data(), &report, &reg);
    let s = &setups[0];
    let sim = kmb_cam_commands::simulate(
        kernel.as_ref(),
        s,
        kmb_cam_commands::SimOptions {
            cell: 0.5,
            rough_cell: 0.5,
            stage_cells: 0,
            ..Default::default()
        },
    )
    .unwrap();
    let layout = layout(&report, cam.setup);
    assert_eq!(
        layout.sheets.iter().map(|s| s.parts.len()).sum::<usize>(),
        2
    );
    let height = |p: [f64; 2]| {
        let x = ((p[0] - sim.result.origin[0]) / sim.result.cell).floor() as usize;
        let y = ((p[1] - sim.result.origin[1]) / sim.result.cell).floor() as usize;
        assert!(x < sim.result.nx && y < sim.result.ny);
        sim.result.z[y * sim.result.nx + x]
    };
    for sheet in &layout.sheets {
        for part in &sheet.parts {
            let x = f64::from(sheet.index - 1) * (sheet.size[0] + SHEET_GAP) + part.position[0]
                - s.info.origin[0];
            let y = part.position[1] - s.info.origin[1];
            // 見本のポケットの床は Z6、ボスの上面は Z15。配置の格子や経路の点は判定に使わない。
            let pocket = height([x + 15.5, y + 14.0]);
            let boss = height([x + 47.0, y + 20.0]);
            assert!(
                (pocket - (6.0 - s.info.origin[2])).abs() <= 0.1,
                "写し {} の床 {pocket}",
                part.instance
            );
            assert!(
                (boss - (15.0 - s.info.origin[2])).abs() <= 0.1,
                "写し {} のボス {boss}",
                part.instance
            );
        }
    }
    assert!(sim.gouges.is_empty(), "{}", sim.summary());
}

#[test]
fn all_too_big_parts_keep_layout_diagnostics_without_exposing_an_unplaced_body() {
    let mut doc = crate::nesting_boxes_document(false);
    let reg = kmb_bundle::registry();
    let kernel = kmb_bundle::kernel();
    let entry = doc
        .data()
        .features
        .iter()
        .find(|f| f.type_id == SETUP)
        .unwrap();
    let sid = entry.id;
    let mut setup: Setup = serde_json::from_value(entry.data.clone()).unwrap();
    setup.stock.width = "2 mm".into();
    setup.stock.depth = "2 mm".into();
    doc.apply(Transaction::new(
        "小さい板",
        vec![Op::SetFeatureData {
            id: sid,
            data: serde_json::to_value(setup).unwrap(),
        }],
    ))
    .unwrap();
    let report = doc.recompute(&reg, kernel.as_ref());
    let layout = layout(&report, sid);
    assert!(layout.sheets.is_empty());
    assert_eq!(layout.unplaced.len(), 12);
    assert!(
        layout
            .unplaced
            .iter()
            .all(|u| u.reason == kmb_cam_setup::nest::UnplacedReason::TooBig)
    );
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(sid) else {
        unreachable!();
    };
    assert!(!outputs.contains_key("setup"));
    assert!(!outputs.keys().any(|k| k.starts_with("nest.")));
    assert!(kmb_cam_commands::setups(doc.data(), &report, &reg).is_empty());
    assert!(
        doc.data()
            .features
            .iter()
            .filter(|f| f.type_id == kmb_cad_extrude::EXTRUDE)
            .all(|f| matches!(report.feature(f.id), Some(FeatureStatus::Ok { .. })))
    );
}

#[test]
fn an_original_chamfer_face_cuts_the_rim_of_each_copy() {
    let mut doc = crate::nesting_boxes_document(false);
    let reg = kmb_bundle::registry();
    let kernel = kmb_bundle::kernel();
    let entry = doc
        .data()
        .features
        .iter()
        .find(|f| f.type_id == SETUP)
        .unwrap();
    let sid = entry.id;
    let setup: Setup = serde_json::from_value(entry.data.clone()).unwrap();
    let body = setup.nest.as_ref().unwrap().parts[0].body;
    let tools = doc
        .data()
        .features
        .iter()
        .find(|f| f.type_id == kmb_cam_commands::TOOLS)
        .unwrap()
        .id;
    let id = doc.alloc_feature_id();
    let data = serde_json::to_value(kmb_strategy_chamfer::Chamfer2d {
        base: kmb_cam_commands::OpBase::new(sid, tools, 5),
        face: Some(format!("f{}:end", body.0)),
        edges: kmb_strategy_chamfer::Edges::Outer,
        width: "0.5 mm".into(),
        tip_offset: "0.2 mm".into(),
        selected: vec![],
    })
    .unwrap();
    doc.apply(Transaction::new(
        "元の面の面取り",
        vec![Op::InsertFeature {
            index: None,
            entry: kmb_core::FeatureEntry {
                id,
                type_id: kmb_strategy_chamfer::CHAMFER.into(),
                name: "元の面の面取り".into(),
                data,
                suppressed: false,
                pin_bodies: false,
            },
        }],
    ))
    .unwrap();
    let report = doc.recompute(&reg, kernel.as_ref());
    assert_eq!(report.error_count(), 0, "{:?}", report.features);
    let layout = layout(&report, sid);
    let setups = kmb_cam_commands::setups(doc.data(), &report, &reg);
    let s = &setups[0];
    let path = &s.paths[s.sources.iter().position(|p| p.id == id).unwrap()];
    let r = path.tool.radius();
    for sheet in &layout.sheets {
        for part in sheet.parts.iter().filter(|p| p.body == body) {
            let [x, y] = part.position;
            assert!(
                path.moves.windows(2).any(|pair| {
                    let a = pair[0].to();
                    let b = pair[1].to();
                    !matches!(pair[1], Move::Rapid { .. })
                        && (b[0] - a[0]).hypot(b[1] - a[1]) > 1e-6
                        && b[2] < -0.1
                        && b[0] >= x - r
                        && b[0] <= x + part.size[0] + r
                        && b[1] >= y - r
                        && b[1] <= y + part.size[1] + r
                }),
                "写し {} の面取り",
                part.instance
            );
        }
    }
}

fn selected_cylinder_reaches_nested_copies_only(kind: &str) {
    use kmb_cam_tool::{ThreadForm, Tool, ToolKind, ToolLibrary};
    use kmb_strategy_drill::{Circular, Drill, Thread};
    let mut cam = reference_pair();
    let reg = kmb_bundle::registry();
    let kernel = kmb_bundle::kernel();
    let report = cam.part.doc.recompute(&reg, kernel.as_ref());
    assert_eq!(report.error_count(), 0, "{:?}", report.features);
    let paths = kmb_cam_commands::setups(cam.part.doc.data(), &report, &reg);
    let info = &paths[0].info;
    let mesh = info.part_mesh(kernel.as_ref()).unwrap();
    let origin = info.origin;
    let model = kmb_cam_setup::testing::prepared(
        &info.placed_mesh(&mesh),
        info.stock_min,
        info.stock_max,
        ToolLibrary::starter().tools[0].clone(),
    )
    .model;
    let same_size: Vec<_> = model
        .bores
        .iter()
        .filter(|b| b.hole && (b.diameter() - 3.4).abs() < 0.05)
        .collect();
    let name = same_size[0].faces[0].clone();
    let wanted: Vec<_> = same_size
        .iter()
        .filter(|b| b.faces.contains(&name))
        .collect();
    assert_eq!(wanted.len(), 2, "元の円筒の壁の名前を2つの写しが保つ");
    assert!(same_size.len() > wanted.len(), "同径でも別名の穴は選ばない");
    let body: Setup =
        serde_json::from_value(cam.part.doc.data().feature(cam.setup).unwrap().data.clone())
            .unwrap();
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(body.body) else {
        panic!("元のボディ");
    };
    let kmb_core::Value::Shape(original) = outputs["body"] else {
        panic!("元の形");
    };
    assert!(
        kernel
            .mesh(original, 0.01)
            .unwrap()
            .face_names
            .contains(&name)
    );
    let mut library: ToolLibrary =
        serde_json::from_value(cam.part.doc.data().feature(cam.tools).unwrap().data.clone())
            .unwrap();
    library.tools.push(Tool {
        number: 8,
        name: "ねじ切り 2 mm".into(),
        kind: ToolKind::ThreadMill,
        diameter: 2.0,
        flute_length: 14.0,
        tip_angle: 60.0,
        shank_diameter: Some(2.0),
        thread: Some(ThreadForm {
            teeth: 1,
            pitch: None,
            neck_diameter: Some(1.0),
        }),
        ..library.tools[0].clone()
    });
    library.check().unwrap();
    let base = kmb_cam_commands::OpBase::new(cam.setup, cam.tools, 3);
    let data = match kind {
        kmb_strategy_drill::DRILL => serde_json::to_value(Drill {
            faces: vec![name],
            same_diameter: false,
            ..Drill::new(base)
        })
        .unwrap(),
        kmb_strategy_drill::CIRCULAR => serde_json::to_value(Circular {
            faces: vec![name],
            same_diameter: false,
            ..Circular::new(base)
        })
        .unwrap(),
        kmb_strategy_drill::THREAD => serde_json::to_value(Thread {
            faces: vec![kmb_core::Ref::Face {
                feature: body.body,
                face: name,
            }],
            pitch: "0.5 mm".into(),
            ..Thread::new(kmb_cam_commands::OpBase::new(cam.setup, cam.tools, 8))
        })
        .unwrap(),
        _ => unreachable!(),
    };
    let id = cam.part.doc.alloc_feature_id();
    cam.part
        .doc
        .apply(Transaction::new(
            "元の円筒面を指定",
            vec![
                Op::SetFeatureData {
                    id: cam.tools,
                    data: serde_json::to_value(library).unwrap(),
                },
                Op::InsertFeature {
                    index: None,
                    entry: kmb_core::FeatureEntry {
                        id,
                        type_id: kind.into(),
                        name: "選んだ円筒".into(),
                        data,
                        suppressed: false,
                        pin_bodies: false,
                    },
                },
            ],
        ))
        .unwrap();
    let report = cam.part.doc.recompute(&reg, kernel.as_ref());
    assert_eq!(report.error_count(), 0, "{:?}", report.features);
    let paths = kmb_cam_commands::setups(cam.part.doc.data(), &report, &reg);
    let s = &paths[0];
    let path = &s.paths[s.sources.iter().position(|p| p.id == id).unwrap()];
    let hit = |c: [f64; 2], z_max: f64| {
        path.moves.iter().any(|m| matches!(m, Move::Arc { center, to, .. } if (center[0]-(c[0]-origin[0])).hypot(center[1]-(c[1]-origin[1])) < 1e-4 && to[2] < z_max - origin[2] - 0.1))
    };
    assert!(
        wanted.iter().all(|b| hit(b.center, b.z_max)),
        "{kind}: 元の円筒面の全写しを削る。対象{} / 届いた{}",
        wanted.len(),
        wanted.iter().filter(|b| hit(b.center, b.z_max)).count()
    );
    assert_eq!(
        same_size.iter().filter(|b| hit(b.center, b.z_max)).count(),
        wanted.len(),
        "同径の別の穴まで広げない"
    );
}

#[test]
fn a_selected_drill_wall_reaches_each_nested_copy_only() {
    selected_cylinder_reaches_nested_copies_only(kmb_strategy_drill::DRILL);
}

#[test]
fn a_selected_circular_wall_reaches_each_nested_copy_only() {
    selected_cylinder_reaches_nested_copies_only(kmb_strategy_drill::CIRCULAR);
}

#[test]
fn a_selected_thread_wall_reaches_each_nested_copy_only() {
    selected_cylinder_reaches_nested_copies_only(kmb_strategy_drill::THREAD);
}
