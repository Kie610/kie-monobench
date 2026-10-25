//! 穴あけの通しの試験: 穴の機能から加工を作る、選んだ穴・同じ径の穴、スケッチの点、サイクル、
//! ポケットの底の穴、穴の間を低く移る。

use crate::sample::{DocBuilder, SketchBuilder};
use jc_cad::{Direction, Operation};
use jc_cad_extrude::{EXTRUDE, Extrude};
use jc_cad_hole::{HOLE, Hole, HoleKind, Tap};
use jc_cam_commands::*;
use jc_core::{Document, FeatureId, FeatureStatus, Modules, Outcome, Value};
use jc_interchange::HolePart;
use jc_sketch::SketchPlane;
use jc_strategy_drill::{
    AUTO_HOLES, CHIP_LIFT, Cycle, DRILL, Drill, HOLE_OPS, HOLE_TOOLS, SAME_DIAMETER, Start,
};
use jc_strategy_pocket2d::{POCKET, Pocket};
use jc_toolpath::{Move, Op};

thread_local! {
    static MODULES: Modules = jc_bundle::modules();
}

fn modules() -> Modules {
    MODULES.with(|m| m.clone())
}

/// 穴の見本: 80 × 50 × 8 mm の板に、M3 のネジ穴 4 個(深さ 6 mm)、⌀6 の通し穴 2 個、
/// M3 のザグリ穴 1 個。`pocket` なら左の中ほどに深さ 3 mm のポケットを掘り、その底に ⌀3.4 の穴 2 個。
struct Part {
    doc: Document,
    tapped: FeatureId,
    plain: FeatureId,
    counterbore: FeatureId,
    pocket_holes: Option<FeatureId>,
    tools: FeatureId,
    setup: FeatureId,
}

/// 点だけのスケッチと、その点の番号。
fn points_sketch(
    b: &mut DocBuilder,
    plane: SketchPlane,
    pts: &[[f64; 2]],
) -> (FeatureId, Vec<jc_core::PointId>) {
    let mut s = SketchBuilder::new(plane);
    let ids = pts
        .iter()
        .map(|p| jc_core::PointId(s.fixed_point(p[0], p[1]).0))
        .collect();
    (b.feature(jc_sketch::SKETCH, "穴の位置", s.build()), ids)
}

fn part(pocket: bool) -> Part {
    let mut b = DocBuilder::new();
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let o = s.fixed_point(0.0, 0.0);
    s.rectangle(o, "80 mm", "50 mm", [0.0, 0.0, 80.0, 50.0]);
    let sk = b.feature(jc_sketch::SKETCH, "板の輪郭", s.build());
    let plate = b.feature(
        EXTRUDE,
        "板",
        Extrude {
            profile: Some(sk),
            regions: vec![],
            distance: "8 mm".into(),
            direction: Direction::Normal,
            distance2: None,
            extent: Default::default(),
            to_face: None,
            to_offset: None,
            operation: Operation::New,
            target: None,
            ..Default::default()
        },
    );
    let top = || SketchPlane::face(plate, format!("f{}:end", plate.0));
    let mut body = plate;
    let mut pocket_holes = None;
    if pocket {
        let mut s = SketchBuilder::new(top());
        let o = s.fixed_point(20.0, 15.0);
        s.rectangle(o, "40 mm", "20 mm", [20.0, 15.0, 40.0, 20.0]);
        let sk = b.feature(jc_sketch::SKETCH, "ポケットの輪郭", s.build());
        body = b.feature(
            EXTRUDE,
            "ポケット",
            Extrude {
                profile: Some(sk),
                regions: vec![],
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
        let floor = SketchPlane::face(body, format!("f{}:start", body.0));
        let (sk, points) = points_sketch(&mut b, floor, &[[30.0, 25.0], [50.0, 25.0]]);
        body = b.feature(
            HOLE,
            "底の穴",
            Hole {
                points,
                diameter: "3.4 mm".into(),
                ..Hole::new(body, sk)
            },
        );
        pocket_holes = Some(body);
    }
    let (sk, points) = points_sketch(
        &mut b,
        top(),
        &[[8.0, 8.0], [72.0, 8.0], [8.0, 42.0], [72.0, 42.0]],
    );
    let tapped = b.feature(
        HOLE,
        "ネジ穴",
        Hole {
            points,
            tap: Tap::Tapped,
            size: Some("M3".into()),
            depth: Some("6 mm".into()),
            ..Hole::new(body, sk)
        },
    );
    let (y, x0, x1) = if pocket {
        (45.0, 25.0, 55.0)
    } else {
        (25.0, 30.0, 50.0)
    };
    let (sk, points) = points_sketch(&mut b, top(), &[[x0, y], [x1, y]]);
    let plain = b.feature(
        HOLE,
        "通し穴",
        Hole {
            points,
            diameter: "6 mm".into(),
            ..Hole::new(tapped, sk)
        },
    );
    let (sk, points) = points_sketch(&mut b, top(), &[[40.0, 8.0]]);
    let counterbore = b.feature(
        HOLE,
        "ザグリ穴",
        Hole {
            points,
            kind: HoleKind::Counterbore,
            tap: Tap::Clearance,
            size: Some("M3".into()),
            ..Hole::new(plain, sk)
        },
    );
    let tools = b.feature(TOOLS, "工具", ToolLibrary::starter());
    let setup = b.feature(SETUP, "セットアップ 1", Setup::new(counterbore));
    Part {
        doc: b.finish("穴の見本"),
        tapped,
        plain,
        counterbore,
        pocket_holes,
        tools,
        setup,
    }
}

fn recompute(doc: &mut Document) -> jc_core::RecomputeReport {
    let m = modules();
    doc.recompute(&m.registry, m.kernel.as_ref())
}

/// 拡張の操作を実行する。
fn run(doc: &mut Document, id: &str, selected: Option<FeatureId>) -> Result<(), String> {
    run_with(doc, id, selected, None)
}

fn run_with(
    doc: &mut Document,
    id: &str,
    selected: Option<FeatureId>,
    picked: Option<jc_core::FacePick>,
) -> Result<(), String> {
    let m = modules();
    let report = doc.recompute(&m.registry, m.kernel.as_ref());
    let cx = jc_core::Context {
        selected,
        picked: picked.as_ref(),
        ..jc_core::Context::new(doc.data(), &report, &m.registry, &m.kernel)
    };
    match m.extensions.run(id, &cx) {
        Ok(Outcome::Change { label, edits }) => {
            doc.apply_edits(&label, edits).unwrap();
            Ok(())
        }
        Ok(_) => Err("文書を変えない".into()),
        Err(e) => Err(e),
    }
}

fn drills(doc: &Document) -> Vec<(FeatureId, String, Drill)> {
    doc.data()
        .features
        .iter()
        .filter(|f| f.type_id == DRILL)
        .map(|f| {
            (
                f.id,
                f.name.clone(),
                serde_json::from_value(f.data.clone()).unwrap(),
            )
        })
        .collect()
}

fn path(doc: &mut Document, id: FeatureId) -> Toolpath {
    let m = modules();
    let report = recompute(doc);
    if let Some(FeatureStatus::Error { message }) = report.feature(id) {
        panic!("{id}: {message}");
    }
    let s = setups(doc.data(), &report, &m.registry).remove(0);
    let name = &doc.data().feature(id).unwrap().name;
    s.generated(s.paths.iter().position(|p| &p.name == name).unwrap())
        .clone()
}

fn insert(doc: &mut Document, name: &str, d: &Drill) -> FeatureId {
    let id = doc.alloc_feature_id();
    doc.apply(jc_core::Transaction::new(
        name,
        vec![jc_core::Op::InsertFeature {
            index: None,
            entry: jc_core::FeatureEntry {
                id,
                type_id: DRILL.into(),
                name: name.into(),
                data: serde_json::to_value(d).unwrap(),
                suppressed: false,
            },
        }],
    ))
    .unwrap();
    id
}

fn set(doc: &mut Document, id: FeatureId, key: &str, v: serde_json::Value) {
    let mut data = doc.data().feature(id).unwrap().data.clone();
    data[key] = v;
    doc.apply(jc_core::Transaction::new(
        "直す",
        vec![jc_core::Op::SetFeatureData { id, data }],
    ))
    .unwrap();
}

/// らせんや下げる所の中心(XY、0.01 mm に丸めて重なりを除く)。
fn centers(tp: &Toolpath) -> Vec<[i64; 2]> {
    let mut v: Vec<[i64; 2]> = tp
        .moves
        .iter()
        .filter_map(|m| match m {
            Move::Arc { center, .. } => Some(*center),
            _ => None,
        })
        .map(|c| [(c[0] * 100.0).round() as i64, (c[1] * 100.0).round() as i64])
        .collect();
    v.sort();
    v.dedup();
    v
}

#[test]
fn holes_become_drilling_operations_that_follow_the_hole() {
    let mut p = part(false);
    run(&mut p.doc, HOLE_OPS, Some(p.setup)).unwrap();
    let report = recompute(&mut p.doc);
    assert_eq!(report.error_count(), 0, "{:?}", report.features);
    let ops: Vec<(String, u32)> = drills(&p.doc)
        .into_iter()
        .map(|(_, n, d)| (n, d.base.tool))
        .collect();
    // ザグリ(平 6)、もみつけ(V)、ネジ穴の下穴(平 2)、通し穴(平 3.175)、ザグリ穴の穴(平 3.175)
    assert_eq!(
        ops,
        vec![
            ("ザグリ穴: ザグリ".to_string(), 1),
            ("ネジ穴: もみつけ".to_string(), 5),
            ("ネジ穴: 下穴".to_string(), 3),
            ("通し穴: 穴".to_string(), 2),
            ("ザグリ穴: 穴".to_string(), 2),
        ]
    );
    let m = modules();
    let s = &setups(p.doc.data(), &report, &m.registry)[0];
    let sim = simulate(m.kernel.as_ref(), s, SimOptions::default()).unwrap();
    assert!(sim.is_ok(), "{}", sim.summary());
    // 揃っていれば、もう一度押しても足さない
    assert!(run(&mut p.doc, HOLE_OPS, Some(p.setup)).is_err());

    // M3 を M4 に直すと、下穴の経路も太くなる(下穴 2.5 → 3.3)
    let pilot = drills(&p.doc)[2].0;
    let before = path(&mut p.doc, pilot);
    set(&mut p.doc, p.tapped, "size", "M4".into());
    let after = path(&mut p.doc, pilot);
    assert_eq!(centers(&before), centers(&after), "同じ 4 か所");
    assert_eq!(centers(&after).len(), 4);
    let radius = |tp: &Toolpath| {
        tp.moves
            .iter()
            .filter_map(|m| match m {
                Move::Arc { to, center, .. } => Some((to[0] - center[0]).hypot(to[1] - center[1])),
                _ => None,
            })
            .fold(0.0, f64::max)
    };
    assert!((radius(&before) - (1.25 - 1.0)).abs() < 1e-6);
    assert!((radius(&after) - (1.65 - 1.0)).abs() < 1e-6);

    // 通し穴をザグリにすると、もう一度押した時にザグリの加工だけ足す
    set(&mut p.doc, p.plain, "kind", "counterbore".into());
    run(&mut p.doc, HOLE_OPS, Some(p.setup)).unwrap();
    let names: Vec<String> = drills(&p.doc).into_iter().map(|(_, n, _)| n).collect();
    assert_eq!(names.len(), 6);
    assert_eq!(names[5], "通し穴: ザグリ");
    let report = recompute(&mut p.doc);
    assert_eq!(report.error_count(), 0, "{:?}", report.features);

    // 加工は CAD の crate に依存せず、穴の機能が出す穴の情報(受け渡しの型)だけを読む。
    // 読めるかを本物の出力で確かめる
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(p.counterbore) else {
        panic!()
    };
    let sec = <jc_interchange::HoleInfo as jc_core::Payload>::from_value(
        &outputs[jc_interchange::HOLE_OUTPUT],
    )
    .expect("穴の情報を読める");
    assert_eq!(
        (sec.kind, sec.tap),
        (
            jc_interchange::HoleKind::Counterbore,
            jc_interchange::Tap::Clearance
        )
    );
    assert!((sec.diameter - 3.4).abs() < 1e-9);
    assert_eq!(sec.count, 1);
    let (cd, ch) = sec.counterbore.unwrap();
    assert!((cd - 6.5).abs() < 1e-9 && (ch - 3.3).abs() < 1e-9);

    // truck は円すいの回転面とのブーリアンが安定しない(jc-solid の countersink の試験と同じ。実測)。
    // 皿の段は SMLib でだけ確かめる
    if std::env::var("JC_KERNEL").is_ok_and(|v| v.trim() == "truck") {
        jc_contract::skip!("truck は皿の段のブーリアンが安定しない");
    }
    // ザグリを皿にすると、V ビットで皿の径まで面取りする操作を足す(ザグリの操作は誤りになり、利用者が消す)
    set(&mut p.doc, p.counterbore, "kind", "countersink".into());
    run(&mut p.doc, HOLE_OPS, Some(p.setup)).unwrap();
    let d = drills(&p.doc);
    let (cs, name, op) = d.last().unwrap();
    assert_eq!(name, "ザグリ穴: 皿の面取り");
    assert_eq!((op.part, op.base.tool), (HolePart::Countersink, 5));
    let tp = path(&mut p.doc, *cs);
    let report = recompute(&mut p.doc);
    let errors: Vec<_> = report
        .features
        .iter()
        .filter(|f| matches!(f.status, FeatureStatus::Error { .. }))
        .map(|f| p.doc.data().feature(f.id).unwrap().name.clone())
        .collect();
    assert_eq!(errors, vec!["ザグリ穴: ザグリ".to_string()]);
    // M3 の皿の径 6.3 mm は V 3.175 mm より太いので、刃の径いっぱいまで下げて円を回る
    assert!(tp.moves.iter().any(|m| matches!(m, Move::Arc { .. })));
    let (sd, angle) = <jc_interchange::HoleInfo as jc_core::Payload>::from_value(
        match report.feature(p.counterbore) {
            Some(FeatureStatus::Ok { outputs, .. }) => &outputs[jc_interchange::HOLE_OUTPUT],
            _ => panic!(),
        },
    )
    .unwrap()
    .countersink
    .unwrap();
    assert!((sd - 6.3).abs() < 1e-6, "{sd}");
    assert!((angle.to_degrees() - 90.0).abs() < 1e-6);
    // 刃の径が皿より細いので、刃の先は板の上面から V の半径ぶん(1.5875 mm)より深くは下げない
    let s = setups(p.doc.data(), &report, &modules().registry).remove(0);
    let top = s.info.stock_max[2] - 1.0;
    let low = tp.moves.iter().map(|m| m.to()[2]).fold(f64::MAX, f64::min);
    assert!(low < top && top - low <= 1.5875 + 1e-6, "{top} {low}");
}

#[test]
fn a_missing_drill_is_named_and_can_be_added() {
    let mut p = part(false);
    // ⌀1.5 の穴は、ライブラリのどの工具でも削れない
    set(&mut p.doc, p.plain, "diameter", "1.5 mm".into());
    run(&mut p.doc, HOLE_OPS, Some(p.setup)).unwrap();
    assert!(!drills(&p.doc).iter().any(|(_, n, _)| n == "通し穴: 穴"));
    let m = modules();
    let report = recompute(&mut p.doc);
    let cx = jc_core::Context {
        selected: Some(p.setup),
        ..jc_core::Context::new(p.doc.data(), &report, &m.registry, &m.kernel)
    };
    let a = m.extensions.actions(&cx);
    let add = a.iter().find(|a| a.id == HOLE_TOOLS).expect("工具を足す案");
    assert!(add.hint.contains("ドリル 1.5 mm"), "{}", add.hint);
    run(&mut p.doc, HOLE_TOOLS, Some(p.setup)).unwrap();
    run(&mut p.doc, HOLE_OPS, Some(p.setup)).unwrap();
    let d = drills(&p.doc);
    let (id, _, op) = d.iter().find(|(_, n, _)| n == "通し穴: 穴").unwrap();
    let lib: ToolLibrary =
        serde_json::from_value(p.doc.data().feature(p.tools).unwrap().data.clone()).unwrap();
    assert_eq!(lib.get(op.base.tool).unwrap().kind, ToolKind::Drill);
    // ドリルで削る穴には、前にもみつけを足す
    assert!(d.iter().any(|(_, n, _)| n == "通し穴: もみつけ"));
    let tp = path(&mut p.doc, *id);
    assert!(!tp.moves.is_empty());
}

#[test]
fn clicking_one_hole_and_choosing_same_diameter_selects_both() {
    let mut p = part(false);
    let m = modules();
    let report = recompute(&mut p.doc);
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(p.counterbore) else {
        panic!()
    };
    let Some(Value::Shape(h)) = outputs.get("body") else {
        panic!()
    };
    let wall = m
        .kernel
        .faces(*h)
        .unwrap()
        .into_iter()
        .map(|f| f.name)
        .find(|n| n.starts_with(&format!("f{}:side:p", p.plain.0)))
        .unwrap();
    let pick = jc_core::FacePick {
        feature: p.counterbore,
        face: wall.clone(),
    };
    // 穴をクリックして「穴あけ」: その穴だけ
    run_with(&mut p.doc, "cam.add.drill", None, Some(pick.clone())).unwrap();
    let (id, _, d) = drills(&p.doc).pop().unwrap();
    assert_eq!(d.faces, vec![wall]);
    assert_eq!(centers(&path(&mut p.doc, id)).len(), 1);
    // 「同じ径の穴を全部選ぶ」: ⌀6 の 2 つ
    run_with(&mut p.doc, SAME_DIAMETER, Some(id), Some(pick)).unwrap();
    let tp = path(&mut p.doc, id);
    assert_eq!(centers(&tp).len(), 2);
    // 「穴を自動で拾う」に戻すと、全部の穴(ネジ穴 4、通し穴 2、ザグリ穴の穴とザグリ)
    run(&mut p.doc, AUTO_HOLES, Some(id)).unwrap();
    assert_eq!(centers(&path(&mut p.doc, id)).len(), 7);
}

#[test]
fn sketch_points_are_drilled_with_chip_breaking_and_a_dwell() {
    let mut p = part(false);
    // 上面のスケッチの点 2 つ(穴の無い所)に、平 2 mm で深さ 3 mm
    let plate = FeatureId(p.doc.data().features[1].id.0);
    let mut s = SketchBuilder::new(SketchPlane::face(plate, format!("f{}:end", plate.0)));
    // 固定していない点(固定した点は原点のような基準とみなし、単独の点に数えない)
    s.point(20.0, 20.0);
    s.point(60.0, 30.0);
    let sk = p.doc.alloc_feature_id();
    p.doc
        .apply(jc_core::Transaction::new(
            "点",
            vec![jc_core::Op::InsertFeature {
                index: None,
                entry: jc_core::FeatureEntry {
                    id: sk,
                    type_id: jc_sketch::SKETCH.into(),
                    name: "点".into(),
                    data: serde_json::to_value(s.build()).unwrap(),
                    suppressed: false,
                },
            }],
        ))
        .unwrap();
    let d = Drill {
        points: Some(sk),
        depth: Some("3 mm".into()),
        cycle: Cycle::ChipBreak,
        peck: Some("1 mm".into()),
        dwell: Some(0.5),
        ..Drill::new(OpBase::new(p.setup, p.tools, 3))
    };
    let id = insert(&mut p.doc, "点の穴", &d);
    let tp = path(&mut p.doc, id);
    let report = recompute(&mut p.doc);
    let s = setups(p.doc.data(), &report, &modules().registry).remove(0);
    // 部品の上面はストックの上面の 1 mm 下(ストックの上の余白)
    let top = s.info.stock_max[2] - 1.0;
    // 2 か所で、部品の上面から 3 mm 下まで
    let bottoms: Vec<[f64; 3]> = tp
        .moves
        .iter()
        .map(|m| m.to())
        .filter(|q| (q[2] - (top - 3.0)).abs() < 1e-6)
        .collect();
    assert_eq!(bottoms.len(), 2, "{bottoms:?}");
    // 切りくず切断: ストックの上面から 1 mm ごとに 0.5 mm だけ戻る(4 mm を 4 回で下げる)
    let lifts = tp
        .moves
        .windows(2)
        .filter(|w| {
            matches!(w[1], Move::Rapid { .. })
                && (w[1].to()[2] - w[0].to()[2] - CHIP_LIFT).abs() < 1e-6
        })
        .count();
    assert_eq!(lifts, 6);
    let program = s.program();
    let waits = program
        .ops
        .iter()
        .filter(|o| matches!(o, Op::Dwell { seconds } if *seconds == 0.5))
        .count();
    assert_eq!(waits, 2);
}

#[test]
fn holes_in_a_pocket_floor_start_at_the_floor_and_move_low_between_them() {
    let mut p = part(true);
    let floor_holes = p.pocket_holes.unwrap();
    // 先にポケットを削る(穴の上から削り始めるのは、上が削れている前提)
    let pocket = p
        .doc
        .data()
        .features
        .iter()
        .find(|f| f.name == "ポケット")
        .unwrap()
        .id;
    let pid = p.doc.alloc_feature_id();
    let data = serde_json::to_value(Pocket {
        ramp_angle: Some("3 deg".into()),
        ..Pocket::on_face(
            OpBase::new(p.setup, p.tools, 1),
            format!("f{}:start", pocket.0),
        )
    })
    .unwrap();
    p.doc
        .apply(jc_core::Transaction::new(
            "ポケット",
            vec![jc_core::Op::InsertFeature {
                index: None,
                entry: jc_core::FeatureEntry {
                    id: pid,
                    type_id: POCKET.into(),
                    name: "ポケットの加工".into(),
                    data,
                    suppressed: false,
                },
            }],
        ))
        .unwrap();
    let base = OpBase::new(p.setup, p.tools, 2);
    let from_stock = insert(
        &mut p.doc,
        "ストックの上から",
        &Drill {
            holes: vec![floor_holes],
            start: Start::Stock,
            ..Drill::new(base.clone())
        },
    );
    let from_hole = insert(
        &mut p.doc,
        "穴の上から",
        &Drill {
            holes: vec![floor_holes],
            stay_low: Some("1 mm".into()),
            ..Drill::new(base)
        },
    );
    let a = path(&mut p.doc, from_stock);
    let b = path(&mut p.doc, from_hole);
    let report = recompute(&mut p.doc);
    assert_eq!(report.error_count(), 0, "{:?}", report.features);
    let s = setups(p.doc.data(), &report, &modules().registry).remove(0);
    // 部品の上面はストックの上面の 1 mm 下。ポケットの床はその 3 mm 下
    let floor = s.info.stock_max[2] - 4.0;
    // 穴の上から: 退避高さから床 + 1 mm まで送りで下り、その上でらせんを始めない
    let arcs_above_floor = |tp: &Toolpath| {
        tp.moves
            .iter()
            .filter(|m| matches!(m, Move::Arc { .. }) && m.to()[2] > floor + 1e-6)
            .count()
    };
    assert!(arcs_above_floor(&a) > 0, "ストックの上面から削り始める");
    assert_eq!(arcs_above_floor(&b), 0, "床の上から削り始める");
    let turns = |tp: &Toolpath| {
        tp.moves
            .iter()
            .filter(|m| matches!(m, Move::Arc { .. }))
            .count()
    };
    assert!(turns(&b) < turns(&a), "{} {}", turns(&b), turns(&a));
    // 穴の間は安全高さへ上がらず、床の上の低い所で移る
    let highest_between = b
        .moves
        .iter()
        .skip_while(|m| !matches!(m, Move::Arc { .. }))
        .take_while(|m| !(matches!(m, Move::Rapid { .. }) && m.to()[2] >= s.info.safe_z - 1e-9))
        .map(|m| m.to()[2])
        .fold(f64::MIN, f64::max);
    assert!(highest_between < s.info.retract_z, "{highest_between}");
    let rises = b
        .moves
        .iter()
        .filter(|m| matches!(m, Move::Rapid { .. }) && m.to()[2] >= s.info.safe_z - 1e-9)
        .count();
    let rises_a = a
        .moves
        .iter()
        .filter(|m| matches!(m, Move::Rapid { .. }) && m.to()[2] >= s.info.safe_z - 1e-9)
        .count();
    assert!(rises < rises_a, "{rises} {rises_a}");
    let sim = simulate(modules().kernel.as_ref(), &s, SimOptions::default()).unwrap();
    assert!(sim.is_ok(), "{}", sim.summary());
}

/// 画面の確かめに使う、穴の見本の設計だけの文書を `JC_HOLE_DOC` のファイルに書く。
#[test]
#[ignore = "道具: JC_HOLE_DOC に穴の見本の文書を書く"]
fn write_the_hole_sample_document() {
    let Ok(path) = std::env::var("JC_HOLE_DOC") else {
        return;
    };
    let mut p = part(false);
    // 工具とセットアップは画面で足すので外す
    for id in [p.setup, p.tools] {
        p.doc
            .apply(jc_core::Transaction::new(
                "外す",
                vec![jc_core::Op::RemoveFeature { id }],
            ))
            .unwrap();
    }
    std::fs::write(path, p.doc.to_json()).unwrap();
}
