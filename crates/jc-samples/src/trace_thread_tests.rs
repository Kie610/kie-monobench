//! トレース(`cam.trace@1`)とねじ切り(`cam.thread@1`)を基準部品で確かめる。経路の形と、
//! Carvera Air の G-code(らせんの円弧の Z、送り)と、シミュレーションで食い込みが出ないこと。

use crate::reference_cam;
use jc_cam_commands::*;
use jc_cam_tool::{ThreadForm, Tool, ToolKind};
use jc_core::{EdgeInfo, FeatureId, FeatureStatus, Ref};
use jc_strategy_drill::{Hand, THREAD, Thread};
use jc_strategy_engrave::{TRACE, Trace};
use jc_toolpath::{ArcDir, Move};

thread_local! {
    /// 同じ文書を別のカーネルで計算し直さない
    static MODULES: jc_core::Modules = jc_bundle::modules();
}

fn modules() -> jc_core::Modules {
    MODULES.with(|m| m.clone())
}

fn json<T: serde::Serialize>(v: &T) -> serde_json::Value {
    serde_json::to_value(v).expect("JSON にできる")
}

fn insert_op(
    doc: &mut jc_core::Document,
    type_id: &str,
    name: &str,
    data: serde_json::Value,
) -> FeatureId {
    let id = doc.alloc_feature_id();
    doc.apply(jc_core::Transaction::new(
        name,
        vec![jc_core::Op::InsertFeature {
            index: None,
            entry: jc_core::FeatureEntry {
                id,
                type_id: type_id.into(),
                name: name.into(),
                data,
                suppressed: false,
            },
        }],
    ))
    .unwrap();
    id
}

/// ねじ切りミル(刃の径 6 mm、首 4 mm、1 山、60 度)を 8 番に足す。
fn add_thread_mill(cam: &mut crate::ReferenceCam, teeth: u32, pitch: Option<f64>) {
    let mut lib: ToolLibrary =
        serde_json::from_value(cam.part.doc.data().feature(cam.tools).unwrap().data.clone())
            .unwrap();
    lib.tools.push(Tool {
        number: 8,
        name: "ねじ切り 6 mm".into(),
        kind: ToolKind::ThreadMill,
        diameter: 6.0,
        flute_length: 14.0,
        tip_angle: 60.0,
        shank_diameter: Some(6.0),
        thread: Some(ThreadForm {
            teeth,
            pitch,
            neck_diameter: Some(4.0),
        }),
        ..lib.tools[0].clone()
    });
    lib.check().unwrap();
    cam.part
        .doc
        .apply(jc_core::Transaction::new(
            "工具",
            vec![jc_core::Op::SetFeatureData {
                id: cam.tools,
                data: json(&lib),
            }],
        ))
        .unwrap();
}

/// 加工するボディの円の辺のうち、半径 `r`・高さ `z`(世界の座標)の物。
fn arc_edge(cam: &mut crate::ReferenceCam, r: f64, z: f64) -> EdgeInfo {
    let m = modules();
    let report = cam.part.doc.recompute(&m.registry, m.kernel.as_ref());
    let s = &setups(cam.part.doc.data(), &report, &m.registry)[0];
    m.kernel
        .arc_edges(s.info.body)
        .unwrap()
        .into_iter()
        .find(|e| {
            e.arc
                .is_some_and(|a| (a.radius - r).abs() < 1e-6 && (a.center[2] - z).abs() < 1e-6)
        })
        .unwrap_or_else(|| panic!("半径 {r}・高さ {z} の円の辺がない"))
}

fn ok(cam: &mut crate::ReferenceCam, id: FeatureId) -> SetupPaths {
    let m = modules();
    let report = cam.part.doc.recompute(&m.registry, m.kernel.as_ref());
    assert!(
        matches!(report.feature(id), Some(FeatureStatus::Ok { .. })),
        "{:?}",
        report.feature(id)
    );
    setups(cam.part.doc.data(), &report, &m.registry).remove(0)
}

fn error(cam: &mut crate::ReferenceCam, id: FeatureId) -> String {
    let m = modules();
    let report = cam.part.doc.recompute(&m.registry, m.kernel.as_ref());
    match report.feature(id) {
        Some(FeatureStatus::Error { message }) => message.clone(),
        other => panic!("誤りにならない: {other:?}"),
    }
}

fn gcode_of(s: &SetupPaths) -> String {
    gcode(
        s,
        &jc_post_carvera_air::CarveraAir,
        &jc_post_carvera_air::profile(),
    )
    .unwrap()
}

#[test]
fn an_m10_thread_is_milled_up_the_center_hole_and_down_the_boss() {
    let mut cam = reference_cam();
    add_thread_mill(&mut cam, 1, None);
    let hole = arc_edge(&mut cam, 4.0, 10.0);
    let boss = arc_edge(&mut cam, 5.0, 15.0);
    let face = |e: &EdgeInfo, part| Ref::Face {
        feature: part,
        face: e.faces[1].clone(),
    };
    let body = cam.part.hole_pattern;
    let base = OpBase::new(cam.setup, cam.tools, 8);
    // 穴は下穴 8 mm、M9.35 相当(外径は空: 8 + 2 × 0.541 × 1.25)。2 周に分けて仕上げでもう 1 周
    let inner = insert_op(
        &mut cam.part.doc,
        THREAD,
        "ねじ切り",
        json(&Thread {
            faces: vec![face(&hole, body)],
            pitch: "1.25 mm".into(),
            passes: 2,
            spring: true,
            ..Thread::new(base.clone())
        }),
    );
    let s = ok(&mut cam, inner);
    let tp = s.paths.iter().find(|p| p.name == "ねじ切り").unwrap();
    let c = s.info.to_setup([30.0, 20.0, 0.0]);
    let helix: Vec<(&[f64; 3], &ArcDir)> = tp
        .moves
        .iter()
        .filter_map(|m| match m {
            Move::Arc {
                to, center, dir, ..
            } if (center[0] - c[0]).abs() < 1e-6 && (center[1] - c[1]).abs() < 1e-6 => {
                Some((to, dir))
            }
            _ => None,
        })
        .collect();
    // 右ねじのダウンカット: 反時計回りで下から上へ、1 周 1.25 mm
    assert!(helix.iter().all(|h| *h.1 == ArcDir::Ccw));
    let top = s.info.to_setup([0.0, 0.0, 10.0])[2];
    let bottom = s.info.to_setup([0.0, 0.0, 0.0])[2];
    let first: Vec<f64> = helix.iter().take(9).map(|h| h.0[2]).collect();
    assert!(
        first.windows(2).all(|w| (w[1] - w[0] - 1.25).abs() < 1e-9),
        "{first:?}"
    );
    assert!(
        (first[0] - (bottom - 0.625 + 1.25)).abs() < 1e-9,
        "貫通は半ピッチ下から"
    );
    assert!(first[8] >= top - 1e-9, "上まで届く");
    // 最後の周の半径: 歯の先がねじの外径の円に届く
    let r_last = {
        let (to, _) = helix.last().unwrap();
        (to[0] - c[0]).hypot(to[1] - c[1])
    };
    let major = 8.0 + 2.0 * 0.541_266 * 1.25;
    assert!((r_last + 3.0 - major / 2.0).abs() < 1e-6, "{r_last}");
    // 周は 2 + 仕上げの 1 = 3 回(それぞれ 9 周のらせん)
    assert_eq!(helix.len(), 3 * 9);
    // 空振りを省いても、壁から真上へ上げない: 周ごとに半円で中心へ戻ってから上がる
    // (中心へ戻る半円を空振りとして省くと、溝に歯が入ったまま真上へ早送りしてしまう)
    let k = s.paths.iter().position(|p| p.name == "ねじ切り").unwrap();
    assert!(
        s.sources[k].original.is_none(),
        "ねじ切りの経路は空振りを省かない"
    );
    let back = tp
        .moves
        .windows(2)
        .filter(|w| {
            matches!(w[0], Move::Arc { to, .. } if (to[0] - c[0]).abs() < 1e-6 && (to[1] - c[1]).abs() < 1e-6)
                && matches!(w[1], Move::Rapid { to } if (to[0] - c[0]).abs() < 1e-6 && (to[1] - c[1]).abs() < 1e-6)
        })
        .count();
    assert_eq!(back, 3, "周ごとに中心へ戻ってから上がる");
    // G-code: G3 でらせん(Z が進む円弧)
    let g = gcode_of(&s);
    assert!(
        g.lines()
            .any(|l| l.starts_with("G3") && l.contains('Z') && l.contains('I')),
        "{g}"
    );
    let m = modules();
    let sim = simulate(m.kernel.as_ref(), &s, SimOptions::default()).unwrap();
    assert!(sim.is_ok(), "{}", sim.summary());

    // ボス: 外から寄せて、時計回りで上から下へ(右ねじのダウンカット)。左ねじは逆
    let bc = s.info.to_setup([47.0, 20.0, 0.0]);
    for (hand, ccw, up) in [(Hand::Right, false, false), (Hand::Left, false, true)] {
        let outer = insert_op(
            &mut cam.part.doc,
            THREAD,
            "おねじ",
            json(&Thread {
                faces: vec![face(&boss, cam.part.boss)],
                pitch: "1 mm".into(),
                hand,
                ..Thread::new(base.clone())
            }),
        );
        let s = ok(&mut cam, outer);
        let tp = s.paths.iter().find(|p| p.name == "おねじ").unwrap();
        let zs: Vec<f64> = tp
            .moves
            .iter()
            .filter_map(|m| match m {
                Move::Arc {
                    to, center, dir, ..
                } if (center[0] - bc[0]).abs() < 1e-6 && (center[1] - bc[1]).abs() < 1e-6 => {
                    assert_eq!(*dir == ArcDir::Ccw, ccw);
                    Some(to[2])
                }
                _ => None,
            })
            .collect();
        assert_eq!(zs.len(), 5, "高さ 5 mm を 1 周 1 mm で");
        assert_eq!(zs[1] > zs[0], up, "{hand:?} {zs:?}");
        let sim = simulate(m.kernel.as_ref(), &s, SimOptions::default()).unwrap();
        assert!(sim.is_ok(), "{}", sim.summary());
        cam.part
            .doc
            .apply(jc_core::Transaction::new(
                "消す",
                vec![jc_core::Op::RemoveFeature { id: outer }],
            ))
            .unwrap();
    }
}

#[test]
fn a_thread_needs_a_thread_mill_that_fits_and_a_picked_wall() {
    let mut cam = reference_cam();
    add_thread_mill(&mut cam, 3, Some(1.0));
    let hole = arc_edge(&mut cam, 4.0, 10.0);
    let wall = Ref::Face {
        feature: cam.part.hole_pattern,
        face: hole.faces[1].clone(),
    };
    // 平のエンドミルでは切れない
    let id = insert_op(
        &mut cam.part.doc,
        THREAD,
        "ねじ切り",
        json(&Thread {
            faces: vec![wall.clone()],
            ..Thread::new(OpBase::new(cam.setup, cam.tools, 1))
        }),
    );
    assert!(error(&mut cam, id).contains("ねじ切りミル"));
    // 歯の間隔(1 mm)とピッチ(1.25 mm)が違う 3 段の工具
    let mut t = Thread {
        faces: vec![wall],
        pitch: "1.25 mm".into(),
        ..Thread::new(OpBase::new(cam.setup, cam.tools, 8))
    };
    let set = |cam: &mut crate::ReferenceCam, t: &Thread| {
        cam.part
            .doc
            .apply(jc_core::Transaction::new(
                "設定",
                vec![jc_core::Op::SetFeatureData { id, data: json(t) }],
            ))
            .unwrap();
    };
    set(&mut cam, &t);
    assert!(error(&mut cam, id).contains("歯の間隔"));
    // 同じピッチなら、3 段の歯の分だけ周が少ない(10 mm + 半ピッチを 1 mm で: 11 周 − 2)
    t.pitch = "1 mm".into();
    set(&mut cam, &t);
    let s = ok(&mut cam, id);
    let tp = s.paths.iter().find(|p| p.name == "ねじ切り").unwrap();
    let turns = tp
        .moves
        .iter()
        .filter(|m| matches!(m, Move::Arc { to, center, .. } if (to[0] - center[0]).hypot(to[1] - center[1]) > 1.0))
        .count();
    assert_eq!(turns, 9, "{turns}");
    // 選んでいなければ、すべての穴を探さずに誤りにする
    t.faces.clear();
    set(&mut cam, &t);
    assert!(error(&mut cam, id).contains("選んで"));
}

#[test]
fn a_trace_follows_the_boss_edge_and_a_slanted_sketch_line_at_their_own_height() {
    let mut cam = reference_cam();
    let rim = arc_edge(&mut cam, 5.0, 15.0);
    let base = OpBase::new(cam.setup, cam.tools, 5); // V 90°
    let id = insert_op(
        &mut cam.part.doc,
        TRACE,
        "トレース",
        json(&Trace {
            edges: vec![Ref::Edge {
                feature: cam.part.boss,
                faces: rim.faces.clone(),
            }],
            axial_offset: "-0.5 mm".into(),
            ..Trace::new(base)
        }),
    );
    let s = ok(&mut cam, id);
    let tp = s.paths.iter().find(|p| p.name == "トレース").unwrap();
    let cut: Vec<[f64; 3]> = tp
        .moves
        .iter()
        .filter(|m| matches!(m, Move::Linear { .. }))
        .map(|m| s.info.to_world(m.to()))
        .collect();
    // ボスの上の縁(高さ 15、半径 5)の 0.5 mm 下を、縁の上でなぞる(V 90° で幅 1 mm の面取り)
    assert!(cut.len() > 20);
    assert!(cut.iter().all(|p| (p[2] - 14.5).abs() < 1e-6), "{cut:?}");
    assert!(
        cut.iter()
            .all(|p| ((p[0] - 47.0).hypot(p[1] - 20.0) - 5.0).abs() < 0.01)
    );
    let m = modules();
    let sim = simulate(m.kernel.as_ref(), &s, SimOptions::default()).unwrap();
    assert!(sim.is_ok(), "{}", sim.summary());
    let g = gcode_of(&s);
    assert!(
        g.lines().filter(|l| l.starts_with("G1")).count() > 20,
        "{g}"
    );

    // 前の面(XZ の平面)の斜めの線: 線の高さのまま下がり、段に分けると切り込みごと
    let mut sk = crate::sample::SketchBuilder::new(jc_sketch::SketchPlane::Zx);
    let a = sk.fixed_point(12.0, 5.0);
    let b = sk.fixed_point(8.0, 20.0);
    sk.line(a, b);
    let sketch = insert_op(
        &mut cam.part.doc,
        jc_sketch::SKETCH,
        "斜めの線",
        json(&sk.build()),
    );
    let t = insert_op(
        &mut cam.part.doc,
        TRACE,
        "斜めのトレース",
        json(&Trace {
            curves: Some(sketch),
            axial_offset: "-1 mm".into(),
            multiple_depths: true,
            ..Trace::new(OpBase::new(cam.setup, cam.tools, 3))
        }),
    );
    let s = ok(&mut cam, t);
    let tp = s.paths.iter().find(|p| p.name == "斜めのトレース").unwrap();
    let cut: Vec<[f64; 3]> = tp
        .moves
        .iter()
        .filter(|m| matches!(m, Move::Linear { .. }))
        .map(|m| s.info.to_world(m.to()))
        .collect();
    let zs: Vec<f64> = cut.iter().map(|p| p[2]).collect();
    let (lo, hi) = zs
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), z| {
            (a.min(*z), b.max(*z))
        });
    assert!(hi - lo > 2.0, "高さが変わる: {zs:?}");
    // 平 2 mm の切り込みは木材で 0.6 → 段は 2 つ(0.6・1.0)。開いた線は行って戻る
    assert!(cut.iter().all(|p| p[1].abs() < 1e-6), "{cut:?}");
    let ends: Vec<&[f64; 3]> = cut
        .iter()
        .filter(|p| (p[0] - 5.0).abs() < 1e-6 || (p[0] - 20.0).abs() < 1e-6)
        .collect();
    assert!(ends.len() >= 4, "{ends:?}");
}

#[test]
fn a_trace_refuses_side_offsets_on_a_line_whose_height_changes() {
    let mut cam = reference_cam();
    let mut sk = crate::sample::SketchBuilder::new(jc_sketch::SketchPlane::Zx);
    let a = sk.fixed_point(12.0, 5.0);
    let b = sk.fixed_point(8.0, 20.0);
    sk.line(a, b);
    let sketch = insert_op(
        &mut cam.part.doc,
        jc_sketch::SKETCH,
        "斜めの線",
        json(&sk.build()),
    );
    let id = insert_op(
        &mut cam.part.doc,
        TRACE,
        "トレース",
        json(&Trace {
            curves: Some(sketch),
            side: jc_strategy_engrave::TraceSide::Left,
            ..Trace::new(OpBase::new(cam.setup, cam.tools, 3))
        }),
    );
    assert!(error(&mut cam, id).contains("水平"));
    // 何も選んでいなければ誤り。ドリルとレーザーは使えない
    let mut t = Trace::new(OpBase::new(cam.setup, cam.tools, 3));
    cam.part
        .doc
        .apply(jc_core::Transaction::new(
            "設定",
            vec![jc_core::Op::SetFeatureData { id, data: json(&t) }],
        ))
        .unwrap();
    assert!(error(&mut cam, id).contains("なぞる辺か線"));
    t.curves = Some(sketch);
    t.base.tool = 5;
    t.axial_offset = "-30 mm".into();
    cam.part
        .doc
        .apply(jc_core::Transaction::new(
            "設定",
            vec![jc_core::Op::SetFeatureData { id, data: json(&t) }],
        ))
        .unwrap();
    assert!(error(&mut cam, id).contains("刃長"));
}
