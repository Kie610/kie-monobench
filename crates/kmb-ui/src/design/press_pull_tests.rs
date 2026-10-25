//! プレス/プルの試験: 選んだ物の種類で押し出し・フィレット・機能を直すへ振り分け、
//! どれもタイムラインに残る。

use crate::press_pull_ui::{PRESS_PULL, PressPull};
use crate::select_ui::Hit;
use crate::{PickFilter, Workspace};
use egui::{Pos2, Rect};
use kmb_cad_extrude::{EXTRUDE, Extrude};
use kmb_samples::sample::{DocBuilder, SketchBuilder};
use kmb_sketch::SketchPlane;

fn reg() -> kmb_core::Modules {
    kmb_bundle::modules()
}

fn rect() -> Rect {
    Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0))
}

fn at(ws: &Workspace, p: [f64; 3]) -> Pos2 {
    ws.cur.camera.project(p.map(|v| v as f32), rect()).0
}

/// 30×20×10 の板。
fn plate() -> Workspace {
    let mut b = DocBuilder::new();
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let o = s.fixed_point(0.0, 0.0);
    s.rectangle(o, "30 mm", "20 mm", [0.0, 0.0, 30.0, 20.0]);
    let sk = b.feature(kmb_sketch::SKETCH, "板のスケッチ", s.build());
    b.feature(
        EXTRUDE,
        "板",
        Extrude {
            profile: Some(sk),
            distance: "10 mm".into(),
            ..Default::default()
        },
    );
    let mut ws = Workspace::new(b.finish("板"), None, reg());
    ws.wait();
    ws.cur.camera.set_view(-60.0, 30.0);
    ws.cur.camera.fit(&ws.cur.scene, rect());
    ws
}

fn click(ws: &mut Workspace, p: [f64; 3], f: PickFilter) -> Hit {
    let hit = ws.hit_at(rect(), at(ws, p), f).expect("何かを拾う");
    ws.pick_hit(Some(hit.clone()), false, false);
    hit
}

fn types(ws: &Workspace) -> Vec<String> {
    ws.cur
        .work
        .doc()
        .data()
        .features
        .iter()
        .map(|f| f.type_id.clone())
        .collect()
}

#[test]
fn press_pull_sits_in_modify_on_q_and_pushes_a_flat_face_into_a_join() {
    // 1 つの板で順に確かめる:
    // - プレス/プルは「修正」にあり、キーは Q(元 q_is_the_key_for_press_pull_and_the_item_sits_in_modify)
    // - 平らな面を押すと結合の押し出しになり、タイムラインに残る
    //   (元 a_flat_face_is_pushed_into_a_join_and_stays_in_the_timeline)
    let mut ws = plate();
    let it = ws.item_of(PRESS_PULL).expect("ツールバーにある");
    assert_eq!(it.label, "プレス/プル");
    assert_eq!(ws.key_of_item(PRESS_PULL).as_deref(), Some("Q"));
    let group = ws
        .ribbon_tabs()
        .into_iter()
        .flat_map(|t| t.groups)
        .find(|g| g.items.iter().any(|i| i.key == PRESS_PULL))
        .unwrap();
    assert_eq!(group.name, "修正");
    // 上と側面を一緒に選ぶと平面が違うので、押し出さずに理由を出す
    click(&mut ws, [15.0, 10.0, 10.0], PickFilter::FACE);
    let Hit::Face(top) = ws
        .hit_at(rect(), at(&ws, [15.0, 10.0, 10.0]), PickFilter::FACE)
        .unwrap()
    else {
        panic!()
    };
    click(&mut ws, [15.0, 0.0, 5.0], PickFilter::FACE);
    ws.pick_hit(Some(Hit::Face(top)), true, false);
    assert!(
        matches!(ws.press_pull_target(), PressPull::Refuse(ref m) if m.contains("同じ平面")),
        "{:?}",
        ws.press_pull_target()
    );
    click(&mut ws, [15.0, 10.0, 10.0], PickFilter::FACE);
    assert_eq!(ws.press_pull_target(), PressPull::Extrude);
    let n = ws.cur.work.doc().data().features.len();
    ws.run_item(PRESS_PULL);
    assert_eq!(
        ws.cur.gesture.as_ref().map(|g| g.action.as_str()),
        Some("cad.extrude"),
        "面の押し出しの流れに入る"
    );
    assert!(
        ws.win.message.as_deref().unwrap_or("").contains("押し出し"),
        "何になったかを出す: {:?}",
        ws.win.message
    );
    ws.gesture_commit_step(serde_json::json!(5.0));
    if ws.cur.gesture.is_some() {
        ws.gesture_finish();
    }
    ws.wait();
    assert_eq!(
        ws.cur.work.doc().data().features.len(),
        n + 1,
        "{:?}",
        types(&ws)
    );
    assert_eq!(types(&ws).last().unwrap(), EXTRUDE);
    let v: f64 = ws
        .visible_bodies()
        .iter()
        .map(|b| ws.cur.work.kernel().mass(b.1).unwrap().volume)
        .sum();
    assert!((v - 30.0 * 20.0 * 15.0).abs() < 1e-3, "上へ 5 足す: {v}");
}

#[test]
fn an_edge_becomes_a_fillet_and_its_face_then_edits_that_fillet() {
    let mut ws = plate();
    let Hit::Edge(_) = click(&mut ws, [15.0, 0.0, 10.0], PickFilter::EDGE) else {
        panic!("手前の上の辺");
    };
    assert_eq!(ws.press_pull_target(), PressPull::Fillet);
    ws.run_item(PRESS_PULL);
    ws.wait();
    assert_eq!(types(&ws).last().unwrap(), "solid.fillet@1");
    let fillet = ws.cur.work.doc().data().features.last().unwrap().id;
    let rows = ws.cur.work.doc().data().feature(fillet).unwrap().data["rows"].clone();
    assert_eq!(
        rows[0]["edges"]["kind"], "edge",
        "選んだ辺が行に入る: {rows}"
    );
    assert!(
        ws.win
            .message
            .as_deref()
            .unwrap_or("")
            .contains("フィレット")
    );
    ws.finish_edit();
    ws.wait();

    // 丸めた面をプレス/プルと、新しい機能は足さずにそのフィレットを直す
    let n = ws.cur.work.doc().data().features.len();
    let r: f64 = rows[0]["radius"]
        .as_str()
        .unwrap()
        .trim_end_matches(" mm")
        .parse()
        .unwrap();
    let s = r * (1.0 - std::f64::consts::FRAC_1_SQRT_2);
    let Hit::Face(face) = click(&mut ws, [15.0, s, 10.0 - s], PickFilter::FACE) else {
        panic!("丸めた面");
    };
    assert_eq!(kmb_core::face_feature(&face.face), Some(fillet), "{face:?}");
    assert!(matches!(ws.press_pull_target(), PressPull::Edit(id, _) if id == fillet));
    ws.run_item(PRESS_PULL);
    assert_eq!(ws.cur.work.doc().data().features.len(), n);
    assert_eq!(ws.cur.editing, Some(fillet));
    assert!(
        ws.win
            .message
            .as_deref()
            .unwrap_or("")
            .contains("フィレットの面"),
        "{:?}",
        ws.win.message
    );
}

#[test]
fn a_hole_face_edits_the_hole_and_a_curved_face_becomes_an_offset() {
    let part = kmb_samples::reference_part();
    let mut ws = Workspace::new(part.doc, None, reg());
    ws.wait();
    let data = ws.cur.work.doc().data().clone();
    let hole = data
        .features
        .iter()
        .find(|f| f.type_id == "solid.hole@1")
        .unwrap()
        .id;
    // 見本の部品の、穴が作った面と、ボス(押し出した円柱)の側面
    let (body, h) = *ws.visible_bodies().last().unwrap();
    let faces = ws.cur.work.kernel().faces(h).unwrap();
    let hole_face = faces
        .iter()
        .find(|f| kmb_core::face_feature(&f.name) == Some(hole) && f.planar.is_none())
        .expect("穴の壁");
    ws.pick_hit(
        Some(Hit::Face(kmb_core::FacePick {
            feature: body,
            face: hole_face.name.clone(),
        })),
        false,
        false,
    );
    assert!(matches!(ws.press_pull_target(), PressPull::Edit(id, _) if id == hole));
    let n = data.features.len();
    ws.run_item(PRESS_PULL);
    assert_eq!(ws.cur.work.doc().data().features.len(), n);
    assert_eq!(ws.cur.editing, Some(hole));
    assert!(ws.win.message.as_deref().unwrap_or("").contains("穴の面"));
    ws.finish_edit();

    let boss = data.features.iter().find(|f| f.name == "ボス").unwrap().id;
    let side = faces
        .iter()
        .find(|f| kmb_core::face_feature(&f.name) == Some(boss) && f.planar.is_none())
        .expect("ボスの側面");
    ws.pick_hit(
        Some(Hit::Face(kmb_core::FacePick {
            feature: body,
            face: side.name.clone(),
        })),
        false,
        false,
    );
    let before = ws.cur.work.kernel().mass(h).unwrap().volume;
    assert!(matches!(ws.press_pull_target(), PressPull::Offset(_)));
    ws.run_item(PRESS_PULL);
    // 曲がった面は面のオフセット(M29)。タイムラインに残り、選んでいた面が面の欄に入る
    assert_eq!(ws.cur.work.doc().data().features.len(), n + 1);
    let off = ws.creating().expect("作っている途中");
    let entry = ws.cur.work.doc().data().feature(off).unwrap().clone();
    assert_eq!(entry.type_id, kmb_cad_modify::OFFSET_FACES);
    assert!(
        entry.data["faces"].to_string().contains(&side.name),
        "{}",
        entry.data
    );
    assert!(
        ws.win
            .message
            .as_deref()
            .unwrap_or("")
            .contains("面のオフセット"),
        "{:?}",
        ws.win.message
    );
    ws.wait();
    let (_, h2) = *ws.visible_bodies().last().unwrap();
    assert!(
        ws.cur.work.kernel().mass(h2).unwrap().volume > before + 1.0,
        "初めの 2 mm でボスが太る"
    );
}

/// 穴の縁を丸めた面を押しても、穴はふさがらない(丸めた面との境目の円が辺の一覧に
/// 無くても、網の縁から足す)。
#[test]
fn pushing_a_face_keeps_a_filleted_hole_open() {
    let part = kmb_samples::reference_part();
    let mut ws = Workspace::new(part.doc, None, reg());
    ws.wait();
    let (body, h) = *ws.visible_bodies().last().unwrap();
    let rim = ws
        .cur
        .work
        .kernel()
        .arc_edges(h)
        .unwrap()
        .into_iter()
        .find(|e| {
            e.arc
                .is_some_and(|a| (a.radius - 4.0).abs() < 1e-6 && (a.center[2] - 10.0).abs() < 1e-6)
        })
        .expect("真ん中の穴の上の縁");
    ws.set_picked_edges(vec![kmb_core::EdgePick {
        feature: body,
        faces: rim.faces.clone(),
    }]);
    ws.run_item(PRESS_PULL);
    ws.finish_edit();
    ws.wait();
    let volume = |ws: &Workspace| -> f64 {
        ws.visible_bodies()
            .iter()
            .map(|b| ws.cur.work.kernel().mass(b.1).unwrap().volume)
            .sum()
    };
    let (body, h) = *ws.visible_bodies().last().unwrap();
    let top = ws
        .cur
        .work
        .kernel()
        .faces(h)
        .unwrap()
        .into_iter()
        .filter(|f| {
            f.planar
                .is_some_and(|p| p.normal[2] > 0.99 && (p.point[2] - 10.0).abs() < 1e-6)
        })
        .max_by(|a, b| a.area.total_cmp(&b.area))
        .unwrap();
    let v0 = volume(&ws);
    ws.pick_hit(
        Some(Hit::Face(kmb_core::FacePick {
            feature: body,
            face: top.name.clone(),
        })),
        false,
        false,
    );
    ws.run_item(PRESS_PULL);
    ws.gesture_commit_step(serde_json::json!(3.0));
    if ws.cur.gesture.is_some() {
        ws.gesture_finish();
    }
    ws.wait();
    let added = volume(&ws) - v0;
    // 足すのは面の形のままの 3 mm(穴をふさぐと π·5²·3 ほど多くなる)
    assert!(
        (added - top.area * 3.0).abs() < 0.01 * top.area,
        "{added} vs {}",
        top.area * 3.0
    );
}
