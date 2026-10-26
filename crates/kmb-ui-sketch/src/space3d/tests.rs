//! 3D スケッチの編集器を、人の操作(クリックとキー)で通して試す。

use super::*;
use crate::tests::{click_px, press_key, reg};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kmb_core::Document;
use kmb_sketch_3d::{Axis, PlaneDef, PlaneRef};
use kmb_ui::Workspace;

fn ws() -> Workspace {
    Workspace::new(Document::new(), None, reg())
        .with_editor(crate::SketchEditor::new())
        .with_editor(Sketch3dEditor::new())
}

fn ed(ws: &mut Workspace) -> &mut Sketch3dEditor {
    ws.editor_mut::<Sketch3dEditor>().expect("組み込んである")
}

/// 空の 3D スケッチを開いた作業場(上から見る)。
fn empty() -> Harness<'static, Workspace> {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws());
    h.run();
    h.state_mut().run_action(kmb_sketch_3d::ADD_SKETCH3D);
    h.run();
    h.run();
    h
}

fn current(ws: &Workspace) -> Sketch3d {
    let id = ws.selected().unwrap();
    serde_json::from_value(ws.document().data().feature(id).unwrap().data.clone()).unwrap()
}

fn solution(ws: &Workspace) -> Solution3d {
    let id = ws.selected().unwrap();
    match ws.report().feature(id) {
        Some(kmb_core::FeatureStatus::Ok { outputs, .. }) => {
            Solution3d::from_value(&outputs["sketch"]).unwrap()
        }
        other => panic!("3D スケッチが計算できていません: {other:?}"),
    }
}

/// スケッチの座標 `p` の画面の位置をクリックする。
fn click3(h: &mut Harness<'static, Workspace>, p: P3) {
    let at = ed(h.state_mut()).screen_of(p);
    click_px(h, at);
}

fn type_text(h: &mut Harness<'static, Workspace>, t: &str) {
    h.event(egui::Event::Text(t.into()));
    h.run();
}

fn count<F: Fn(&Entity3d) -> bool>(s: &Sketch3d, f: F) -> usize {
    s.entities.iter().filter(|e| f(e)).count()
}

fn lines(s: &Sketch3d) -> usize {
    count(s, |e| matches!(e, Entity3d::Line { .. }))
}

/// XY の作図面に 40×30 の四角を線分の道具で描く(原点から)。
fn square(h: &mut Harness<'static, Workspace>) {
    press_key(h, egui::Key::L);
    for p in [
        [0.0, 0.0, 0.0],
        [40.0, 0.0, 0.0],
        [40.0, 30.0, 0.0],
        [0.0, 30.0, 0.0],
        [0.0, 0.0, 0.0],
    ] {
        click3(h, p);
    }
}

#[test]
fn adding_a_3d_sketch_opens_its_editor() {
    let h = empty();
    let id = h.state().selected().unwrap();
    assert!(h.state().is_editing(id));
    assert_eq!(
        h.state().document().data().feature(id).unwrap().type_id,
        kmb_sketch_3d::SKETCH3D
    );
    // パネルに状態と作図面が出る
    // 何も描いていないので「完全拘束」と出さない
    h.get_by_label(kmb_sketch::NO_SHAPE);
    assert!(h.get_all_by_label("XY 平面").count() >= 1);
    h.get_by_label("上から(+Z)");
}

#[test]
fn drawing_a_closed_square_on_the_xy_plane_finds_a_face() {
    let mut h = empty();
    square(&mut h);
    h.run();
    let s = current(h.state());
    assert_eq!(lines(&s), 4, "{s:?}");
    assert_eq!(count(&s, |e| matches!(e, Entity3d::Point { .. })), 4);
    let along: Vec<Axis> = s
        .constraints
        .iter()
        .filter_map(|c| match c.kind {
            Kind3d::Along { axis, .. } => Some(axis),
            _ => None,
        })
        .collect();
    assert_eq!(along, vec![Axis::X, Axis::Y, Axis::X, Axis::Y]);
    // 原点に吸い付いた点は固定になる
    assert!(
        s.constraints
            .iter()
            .any(|c| matches!(c.kind, Kind3d::Fix { .. }))
    );
    let sol = solution(h.state());
    assert_eq!(sol.faces.len(), 1, "閉じた四角が面になっていません");
    // 閉じたら線分の道具はつなぐのを終える(道具は残る)
    assert!(ed(h.state_mut()).start.is_none());
    assert_eq!(ed(h.state_mut()).tool, Tool3::Line);
    // 1 本ずつ取り消せる
    h.state_mut().undo();
    h.run();
    assert_eq!(lines(&current(h.state())), 3);
}

#[test]
fn tab_switches_the_drawing_plane_to_draw_a_vertical_line() {
    let mut h = empty();
    square(&mut h);
    // 角の点から始め、Tab で YZ → ZX の作図面にして上へ引く
    click3(&mut h, [40.0, 0.0, 0.0]);
    press_key(&mut h, egui::Key::Tab);
    assert_eq!(ed(h.state_mut()).plane, AxisPlane::Yz);
    press_key(&mut h, egui::Key::Tab);
    assert_eq!(ed(h.state_mut()).plane, AxisPlane::Zx);
    click3(&mut h, [40.0, 0.0, 25.0]);
    h.run();
    let s = current(h.state());
    assert_eq!(lines(&s), 5);
    let vertical = s.entities.iter().find_map(|e| match e {
        Entity3d::Line { id, a, b, .. } if *id == s.entities.last().unwrap().id() => {
            Some((*id, *a, *b))
        }
        _ => None,
    });
    let (id, _, b) = vertical.expect("線分");
    assert!(
        s.constraints.iter().any(|c| c.kind
            == Kind3d::Along {
                line: id,
                axis: Axis::Z
            }),
        "上へ引いた線が Z に沿っていません"
    );
    let top = solution(h.state()).points[&b];
    assert!((top[2] - 25.0).abs() < 0.5, "{top:?}");
    assert!(
        (top[0] - 40.0).abs() < 0.05 && top[1].abs() < 0.05,
        "{top:?}"
    );
}

#[test]
fn the_angle_between_two_faces_is_set_by_a_dimension() {
    let mut h = empty();
    square(&mut h);
    // 上の辺の右端から斜めに立ち上がる四角を足す(YZ → XY → YZ の作図面で順に引く)
    click3(&mut h, [40.0, 30.0, 0.0]);
    press_key(&mut h, egui::Key::Tab);
    click3(&mut h, [40.0, 50.0, 20.0]);
    press_key(&mut h, egui::Key::Tab);
    press_key(&mut h, egui::Key::Tab);
    click3(&mut h, [0.0, 50.0, 20.0]);
    press_key(&mut h, egui::Key::Tab);
    click3(&mut h, [0.0, 30.0, 0.0]);
    h.run();
    let sol = solution(h.state());
    assert_eq!(
        sol.faces.len(),
        2,
        "2 枚の面: {:?} {:?} {:?}",
        sol.faces,
        current(h.state()),
        sol.points
    );
    press_key(&mut h, egui::Key::Escape);
    press_key(&mut h, egui::Key::Escape);
    // 2 枚の面を中の点で選ぶ(2 枚目は Shift で足す)
    let c0 = sol.faces[0].center;
    let c1 = sol.faces[1].center;
    click3(&mut h, c0);
    let at = ed(h.state_mut()).screen_of(c1);
    h.hover_at(at);
    h.run();
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::SHIFT,
        });
        h.run();
    }
    assert_eq!(
        ops::classify(&current(h.state()), &ed(h.state_mut()).selection)
            .faces
            .len(),
        2
    );
    h.get_by_label("面の角度…").click();
    h.run();
    type_text(&mut h, "60");
    press_key(&mut h, egui::Key::Enter);
    h.run();
    let s = current(h.state());
    let fa = s
        .constraints
        .iter()
        .find(|c| matches!(c.kind, Kind3d::FaceAngle { .. }))
        .expect("面の角度");
    let sol = solution(h.state());
    let now = sol.measured[&fa.id].to_degrees();
    assert!((now - 60.0).abs() < 1e-4, "面の角度 {now}");
}

#[test]
fn x_makes_a_line_construction_and_delete_removes_it() {
    let mut h = empty();
    square(&mut h);
    press_key(&mut h, egui::Key::Escape);
    press_key(&mut h, egui::Key::Escape);
    click3(&mut h, [40.0, 15.0, 0.0]);
    press_key(&mut h, egui::Key::X);
    let s = current(h.state());
    assert_eq!(
        count(&s, |e| matches!(
            e,
            Entity3d::Line {
                construction: true,
                ..
            }
        )),
        1
    );
    // 補助線は面に使わない
    h.run();
    assert!(solution(h.state()).faces.is_empty());
    press_key(&mut h, egui::Key::Delete);
    assert_eq!(lines(&current(h.state())), 3);
    // 何も選ばずに X: 補助線で描くかを替える
    press_key(&mut h, egui::Key::Escape);
    press_key(&mut h, egui::Key::X);
    assert!(ed(h.state_mut()).construction);
}

#[test]
fn dragging_a_free_point_moves_it_within_the_constraints() {
    let mut h = empty();
    square(&mut h);
    press_key(&mut h, egui::Key::Escape);
    press_key(&mut h, egui::Key::Escape);
    let from = ed(h.state_mut()).screen_of([40.0, 30.0, 0.0]);
    let to = from + egui::vec2(60.0, 0.0);
    h.hover_at(from);
    h.run();
    h.event(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
    for k in 1..=6 {
        h.hover_at(from + (to - from) * (k as f32 / 6.0));
        h.run();
    }
    h.event(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
    h.run();
    let s = current(h.state());
    let sol = solution(h.state());
    // 四角のまま(軸に沿う拘束は保つ)で、角が動いた
    let moved = s
        .entities
        .iter()
        .filter_map(|e| match e {
            Entity3d::Point { id, .. } => sol.points.get(id),
            _ => None,
        })
        .any(|p| (p[0] - 40.0).abs() > 1.0 || (p[1] - 30.0).abs() > 1.0);
    assert!(moved, "点が動いていません: {:?}", sol.points);
    assert!(
        !matches!(
            sol.status,
            SolveStatus::Inconsistent { .. } | SolveStatus::OverConstrained { .. }
        ),
        "{:?}",
        sol.status
    );
}

#[test]
fn a_constraint_needs_a_matching_selection_and_says_why() {
    let mut h = empty();
    square(&mut h);
    press_key(&mut h, egui::Key::Escape);
    press_key(&mut h, egui::Key::Escape);
    // 何も選ばずに「直交」
    h.get_by_label("直交").click();
    h.run();
    assert!(
        ed(h.state_mut())
            .note
            .as_deref()
            .is_some_and(|n| n.contains("線分を 2 本")),
        "{:?}",
        ed(h.state_mut()).note
    );
}

#[test]
fn finishing_shows_the_3d_lines_in_the_main_view() {
    let mut h = empty();
    square(&mut h);
    let id = h.state().selected().unwrap();
    h.state_mut().finish_sketch();
    h.run();
    match h.state().report().feature(id) {
        Some(kmb_core::FeatureStatus::Ok { outputs, .. }) => {
            let w = <kmb_core::Wires as kmb_core::Payload>::from_value(&outputs["lines"]).unwrap();
            assert_eq!(w.0.len(), 4);
        }
        other => panic!("{other:?}"),
    }
}

fn work_constraints(s: &Sketch3d) -> usize {
    s.constraints
        .iter()
        .filter(|c| {
            matches!(
                c.kind,
                Kind3d::PointOnPlane {
                    plane: PlaneRef::Work { .. },
                    ..
                }
            )
        })
        .count()
}

#[test]
fn a_tilted_work_plane_from_an_edge_draws_a_facet_at_its_angle() {
    let mut h = empty();
    square(&mut h);
    press_key(&mut h, egui::Key::Escape);
    press_key(&mut h, egui::Key::Escape);
    // 下の辺を選んで作図面を作る: 辺を通り XY 平面から傾けた作図面になり、傾きを打つ欄が開く
    click3(&mut h, [20.0, 0.0, 0.0]);
    h.get_by_label("作図面を作る…").click();
    h.run();
    let s = current(h.state());
    assert_eq!(s.planes.len(), 1, "{:?}", s.planes);
    let w = s.planes[0].id;
    assert!(matches!(s.planes[0].def, PlaneDef::Tilted { .. }));
    assert_eq!(ed(h.state_mut()).work, Some(w));
    assert!(
        ed(h.state_mut()).plane_edit.is_some(),
        "傾きの欄が開いていません"
    );
    type_text(&mut h, "40");
    press_key(&mut h, egui::Key::Enter);
    h.run();
    let s = current(h.state());
    let PlaneDef::Tilted {
        angle, line: hinge, ..
    } = s.planes[0].def
    else {
        panic!("{:?}", s.planes);
    };
    assert!(
        (angle.to_degrees() - 40.0).abs() < 1e-9,
        "{}",
        angle.to_degrees()
    );
    // 辺の端から、作図面の上を辺に直角な向きへ線を引く
    press_key(&mut h, egui::Key::L);
    click3(&mut h, [0.0, 0.0, 0.0]);
    let t = 40f64.to_radians();
    click3(&mut h, [0.0, 20.0 * t.cos(), 20.0 * t.sin()]);
    press_key(&mut h, egui::Key::Escape);
    h.run();
    let s = current(h.state());
    let Some(Entity3d::Line { id: l, b, .. }) = s.entities.last().cloned() else {
        panic!("線分がありません: {s:?}");
    };
    assert!(
        s.constraints
            .iter()
            .any(|c| c.kind == Kind3d::Perpendicular { a: l, b: hinge }),
        "辺に直角の拘束が付いていません: {:?}",
        s.constraints
    );
    assert_eq!(
        work_constraints(&s),
        1,
        "引いた先の点が作図面に乗っていません"
    );
    let q = solution(h.state()).points[&b];
    let slope = q[2].atan2(q[1]).to_degrees();
    assert!((slope - 40.0).abs() < 1e-4, "傾き {slope}° {q:?}");
    // 反対へ傾けると、作図面に乗った点も付いて動く
    h.get_by_label("反対へ").click();
    h.run();
    let q = solution(h.state()).points[&b];
    assert!(q[2] < 0.0, "{q:?}");
}

#[test]
fn an_offset_work_plane_holds_a_rectangle_and_tab_reaches_it() {
    let mut h = empty();
    // 何も選ばずに作ると、今の作図面(XY)に平行な作図面。ずらす距離を打つ
    h.get_by_label("作図面を作る…").click();
    h.run();
    type_text(&mut h, "15");
    press_key(&mut h, egui::Key::Enter);
    h.run();
    let s = current(h.state());
    assert!(
        matches!(s.planes[0].def, PlaneDef::Offset { base: AxisPlane::Xy, offset } if (offset - 15.0).abs() < 1e-9),
        "{:?}",
        s.planes
    );
    press_key(&mut h, egui::Key::R);
    click3(&mut h, [0.0, 0.0, 15.0]);
    click3(&mut h, [30.0, 20.0, 15.0]);
    h.run();
    let s = current(h.state());
    assert_eq!(lines(&s), 4);
    assert_eq!(work_constraints(&s), 4);
    let sol = solution(h.state());
    assert!(
        sol.points.values().all(|p| (p[2] - 15.0).abs() < 1e-9),
        "{:?}",
        sol.points
    );
    // Tab は XY → YZ → ZX → 作図面1 と回る
    press_key(&mut h, egui::Key::Escape);
    press_key(&mut h, egui::Key::Tab);
    assert_eq!(ed(h.state_mut()).work, None);
    assert_eq!(ed(h.state_mut()).plane, AxisPlane::Xy);
    for _ in 0..3 {
        press_key(&mut h, egui::Key::Tab);
    }
    assert_eq!(ed(h.state_mut()).work, Some(s.planes[0].id));
    // 名前の札をクリックで選び、Delete で消す(点は残り、作図面に乗せる拘束が外れる)
    press_key(&mut h, egui::Key::Escape);
    let (sk, sol) = (current(h.state()), solution(h.state()));
    let tags = super::view::plane_tags(ed(h.state_mut()), &sk, &sol);
    let at = tags[0].1 + egui::vec2(8.0, -6.0);
    click_px(&mut h, at);
    assert_eq!(
        ed(h.state_mut()).selection,
        vec![Sel::Plane(s.planes[0].id)]
    );
    press_key(&mut h, egui::Key::Delete);
    let s = current(h.state());
    assert!(s.planes.is_empty());
    assert_eq!(work_constraints(&s), 0);
    assert_eq!(lines(&s), 4);
}

/// 基準部品(60 × 40 × 10 mm の板に穴・ポケット・ボス)の後ろに置いた空の 3D スケッチ(斜めから見る)。
fn on_part() -> Harness<'static, Workspace> {
    let part = kmb_samples::reference_part();
    let mut v: serde_json::Value = serde_json::from_str(&part.doc.to_json()).unwrap();
    let id = v["next_id"].as_u64().unwrap();
    v["next_id"] = (id + 1).into();
    v["features"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "id": id,
            "type_id": kmb_sketch_3d::SKETCH3D,
            "name": "3D スケッチ",
            "data": serde_json::to_value(Sketch3d::default()).unwrap(),
        }));
    let doc = Document::from_json(&v.to_string()).unwrap();
    let mut ws = Workspace::new(doc, None, reg())
        .with_editor(crate::SketchEditor::new())
        .with_editor(Sketch3dEditor::new());
    ws.open_edit(kmb_core::FeatureId(id));
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    // 初めから斜めから見ている(ボタンをもう一度押すと斜め後ろからになるので押さない)
    assert!(h.query_by_label("斜めから").is_some());
    h.run();
    assert!(
        !ed(h.state_mut()).body_items.is_empty(),
        "ボディの角や辺が拾えていません"
    );
    h
}

/// 帯の「取り込み ▾」の中の道具を押す。
fn more_tool(h: &mut Harness<'static, Workspace>, label: &str) {
    h.get_by_label("取り込み ▾").click();
    h.run();
    h.get_by_label(label).click();
    h.run();
    h.run();
}

#[test]
fn including_a_body_corner_and_edge_links_them_to_the_body() {
    let mut h = on_part();
    h.get_by_label("3D 取り込み").click();
    h.run();
    assert_eq!(ed(h.state_mut()).tool, Tool3::Include);
    // 板の奥の上の角と、手前の上の辺をクリック
    click3(&mut h, [60.0, 40.0, 10.0]);
    click3(&mut h, [30.0, 0.0, 10.0]);
    h.run();
    let s = current(h.state());
    assert_eq!(s.includes.len(), 2, "{:?}", s.includes);
    let sol = solution(h.state());
    let corner = s.includes[0].entity;
    assert_eq!(sol.points[&corner], [60.0, 40.0, 10.0]);
    let (a, b) = s.line_ends(s.includes[1].entity).expect("辺は線分になる");
    let mut ends = [sol.points[&a], sol.points[&b]];
    ends.sort_by(|p, q| p[0].total_cmp(&q[0]));
    assert!(
        geom::len(geom::sub(ends[0], [0.0, 0.0, 10.0])) < 1e-6,
        "{ends:?}"
    );
    assert!(
        geom::len(geom::sub(ends[1], [60.0, 0.0, 10.0])) < 1e-6,
        "{ends:?}"
    );
    // 同じ角をもう一度押しても増えない
    click3(&mut h, [60.0, 40.0, 10.0]);
    assert_eq!(current(h.state()).includes.len(), 2);
    // 取り込んだ角から、YZ の作図面で上へ線を引く
    press_key(&mut h, egui::Key::Escape);
    press_key(&mut h, egui::Key::L);
    click3(&mut h, [60.0, 40.0, 10.0]);
    press_key(&mut h, egui::Key::Tab);
    assert_eq!(ed(h.state_mut()).plane, AxisPlane::Yz);
    click3(&mut h, [60.0, 40.0, 30.0]);
    press_key(&mut h, egui::Key::Escape);
    h.run();
    let s = current(h.state());
    let Some(Entity3d::Line { a: p, b: q, .. }) = s.entities.last().cloned() else {
        panic!("{s:?}");
    };
    assert_eq!(p, corner, "取り込んだ角につながっていません");
    // 板を厚くすると、取り込んだ角と、そこから引いた線が付いて上がる
    let t = h
        .state()
        .document()
        .data()
        .param_by_name("thickness")
        .unwrap()
        .id;
    assert!(h.state_mut().set_param_expr(t, "12 mm"));
    h.run();
    let sol = solution(h.state());
    assert!(
        (sol.points[&corner][2] - 12.0).abs() < 1e-9,
        "{:?}",
        sol.points[&corner]
    );
    assert!(sol.points[&q][2] > 12.0 + 1.0, "{:?}", sol.points[&q]);
}

/// 交差の道具(まとめてレビュー R4): 今の作図面でボディを切った線を、押したボディの全体か、押した線だけ
/// 写す。写した線は作図面に付いて動く。
#[test]
fn a_section_of_the_plate_through_an_offset_plane_has_its_outline_and_holes() {
    let mut h = on_part();
    // 何も選ばずに作図面を作り、底から 5 mm へずらす
    h.get_by_label("作図面を作る…").click();
    h.run();
    type_text(&mut h, "5");
    press_key(&mut h, egui::Key::Enter);
    h.run();
    h.get_by_label("交差").click();
    h.run();
    assert_eq!(ed(h.state_mut()).tool, Tool3::Intersect);
    // 道具の間は、作図面で切った線を下絵に描く
    let under = ed(h.state_mut())
        .on_plane
        .as_ref()
        .and_then(|c| c.pieces.as_ref())
        .map_or(0, Vec::len);
    assert!(under >= 6, "下絵の断面の線 {under}");
    h.get_by_label("ボディ全体(押したボディの断面すべて)")
        .click();
    h.run();
    // 板の手前の側面の断面の線を押す
    click3(&mut h, [30.0, 0.0, 5.0]);
    h.run();
    let s = current(h.state());
    assert_eq!(s.derived.len(), 1, "{:?}", s.derived);
    let id = s.derived[0].id;
    let lines = solution(h.state()).derived[&id].clone();
    // 外形と、真ん中の穴、四隅の穴(ポケットは上から 4 mm なので掛からない)
    assert_eq!(lines.len(), 6, "{}", lines.len());
    assert!(lines.iter().flatten().all(|p| (p[2] - 5.0).abs() < 1e-6));
    let per = |l: &Vec<P3>| -> f64 { l.windows(2).map(|w| geom::len(geom::sub(w[1], w[0]))).sum() };
    assert!(
        lines.iter().any(|l| (per(l) - 200.0).abs() < 0.01),
        "外形の周 200 mm: {:?}",
        lines.iter().map(per).collect::<Vec<_>>()
    );
    // 同じ所をもう一度押しても増えない
    click3(&mut h, [30.0, 0.0, 5.0]);
    assert_eq!(current(h.state()).derived.len(), 1);
    // 作図面の高さを直すと、交わりの線も付いて動く
    let w = s.planes[0].id;
    ed(h.state_mut()).plane_edit = Some((w, "5".into()));
    h.run();
    type_text(&mut h, "7");
    press_key(&mut h, egui::Key::Enter);
    h.run();
    let lines = solution(h.state()).derived[&id].clone();
    assert!(lines.iter().flatten().all(|p| (p[2] - 7.0).abs() < 1e-6));
    // ポケット(上から 4 mm)に掛かるので、輪が 1 つ増える
    assert_eq!(lines.len(), 7, "{}", lines.len());
    // 「押した線」なら、押した真ん中の穴(径 8 mm)の断面だけ
    h.get_by_label("押した線(面ごとの断面)").click();
    h.run();
    click3(&mut h, [34.0, 20.0, 7.0]);
    h.run();
    let s = current(h.state());
    assert_eq!(s.derived.len(), 2, "{:?}", s.derived);
    let hole = solution(h.state()).derived[&s.derived[1].id].clone();
    assert!(!hole.is_empty());
    assert!(
        hole.iter().flatten().all(|p| {
            let r = geom::len(geom::sub(*p, [30.0, 20.0, 7.0]));
            (r - 4.0).abs() < 0.05
        }),
        "{hole:?}"
    );
    // 線を選んで Delete で消せる
    press_key(&mut h, egui::Key::Escape);
    ed(h.state_mut()).selection = vec![Sel::Derived(id)];
    press_key(&mut h, egui::Key::Delete);
    assert_eq!(current(h.state()).derived.len(), 1);
}

/// 投影の道具(まとめてレビュー R4): ボディの角・辺・面、シルエット、辺と角すべてを、今の作図面へ
/// 垂直に写す(2D の投影と同じ 3 択)。写した線は作図面に付いて動く。
#[test]
fn projecting_onto_the_drawing_plane_takes_parts_the_silhouette_or_all_edges() {
    let mut h = on_part();
    // 板の上 20 mm の作図面
    h.get_by_label("作図面を作る…").click();
    h.run();
    type_text(&mut h, "20");
    press_key(&mut h, egui::Key::Enter);
    h.run();
    h.get_by_label("投影").click();
    h.run();
    assert_eq!(ed(h.state_mut()).tool, Tool3::Project);
    // 道具の間は、作図面へ写したボディの辺を下絵に描く
    assert!(
        ed(h.state_mut())
            .on_plane
            .as_ref()
            .and_then(|c| c.edges.as_ref())
            .is_some_and(|e| !e.is_empty())
    );
    // 面・辺・点: 手前の上の辺、奥の角、真ん中の穴の上の縁
    click3(&mut h, [30.0, 0.0, 10.0]);
    click3(&mut h, [60.0, 40.0, 10.0]);
    click3(&mut h, [34.0, 20.0, 10.0]);
    h.run();
    let s = current(h.state());
    assert_eq!(s.derived.len(), 3, "{:?}", s.derived);
    let sol = solution(h.state());
    let edge = &sol.derived[&s.derived[0].id];
    assert_eq!(edge.len(), 1, "{edge:?}");
    let mut ends = edge[0].clone();
    ends.sort_by(|p, q| p[0].total_cmp(&q[0]));
    assert!(
        geom::len(geom::sub(ends[0], [0.0, 0.0, 20.0])) < 1e-6,
        "{ends:?}"
    );
    assert!(
        geom::len(geom::sub(ends[1], [60.0, 0.0, 20.0])) < 1e-6,
        "{ends:?}"
    );
    assert_eq!(
        sol.derived[&s.derived[1].id],
        vec![vec![[60.0, 40.0, 20.0]]]
    );
    let rim = &sol.derived[&s.derived[2].id];
    assert!(
        rim.iter().flatten().all(|p| {
            (p[2] - 20.0).abs() < 1e-6
                && (geom::len(geom::sub(*p, [30.0, 20.0, 20.0])) - 4.0).abs() < 1e-6
        }),
        "{rim:?}"
    );
    // シルエット: 板の上面を押す。外形と貫通穴だけで、ポケットの縁(y = 8)は入らない
    h.get_by_label("ボディのシルエット(押したボディの外形)")
        .click();
    h.run();
    click3(&mut h, [20.0, 30.0, 10.0]);
    h.run();
    let s = current(h.state());
    assert_eq!(s.derived.len(), 4, "{:?}", s.derived);
    let sil = solution(h.state()).derived[&s.derived[3].id].clone();
    let pts: Vec<P3> = sil.iter().flatten().copied().collect();
    assert!(pts.iter().all(|p| (p[2] - 20.0).abs() < 1e-6));
    let near = |pts: &[P3], q: P3| pts.iter().any(|p| geom::len(geom::sub(*p, q)) < 1e-3);
    assert!(near(&pts, [0.0, 0.0, 20.0]) && near(&pts, [60.0, 40.0, 20.0]));
    let pocket = |pts: &[P3]| {
        pts.iter()
            .any(|p| (p[1] - 8.0).abs() < 1e-6 && p[0] > 7.0 && p[0] < 24.0)
    };
    assert!(!pocket(&pts), "シルエットにポケットの縁が入っています");
    // 辺と角すべて: ポケットの縁も入る
    h.get_by_label("ボディの辺と角すべて").click();
    h.run();
    click3(&mut h, [20.0, 30.0, 10.0]);
    h.run();
    let s = current(h.state());
    assert_eq!(s.derived.len(), 5, "{:?}", s.derived);
    let all: Vec<P3> = solution(h.state()).derived[&s.derived[4].id]
        .iter()
        .flatten()
        .copied()
        .collect();
    assert!(all.iter().all(|p| (p[2] - 20.0).abs() < 1e-6));
    assert!(pocket(&all), "辺すべてにポケットの縁がありません");
    // 作図面の高さを直すと、写した線もみな付いて動く
    let w = s.planes[0].id;
    ed(h.state_mut()).plane_edit = Some((w, "20".into()));
    h.run();
    type_text(&mut h, "30");
    press_key(&mut h, egui::Key::Enter);
    h.run();
    let sol = solution(h.state());
    assert!(
        sol.derived
            .values()
            .flatten()
            .flatten()
            .all(|p| (p[2] - 30.0).abs() < 1e-6)
    );
    // 道具を終えると下絵は描かない(写した線は残る)
    press_key(&mut h, egui::Key::Escape);
    h.run();
    assert_eq!(ed(h.state_mut()).tool, Tool3::Select);
    assert_eq!(current(h.state()).derived.len(), 5);
}

/// 座標の平面に平行で原点を通らない作図面へ写すと、その位置の作図面を作って写す(作図面に付いて行く)。
/// 横から見たシルエットには、上面のボス(高さ 5 mm)も入る。
#[test]
fn a_silhouette_onto_a_side_plane_makes_its_work_plane() {
    let mut h = on_part();
    ed(h.state_mut()).set_plane(AxisPlane::Yz);
    ed(h.state_mut()).anchor = [-20.0, 0.0, 0.0];
    h.get_by_label("投影").click();
    h.run();
    h.get_by_label("ボディのシルエット(押したボディの外形)")
        .click();
    h.run();
    click3(&mut h, [20.0, 30.0, 10.0]);
    h.run();
    let s = current(h.state());
    assert_eq!(s.planes.len(), 1, "作図面を作る: {:?}", s.planes);
    assert!(matches!(
        s.planes[0].def,
        PlaneDef::Offset {
            base: AxisPlane::Yz,
            ..
        }
    ));
    let pts: Vec<P3> = solution(h.state()).derived[&s.derived[0].id]
        .iter()
        .flatten()
        .copied()
        .collect();
    assert!(!pts.is_empty());
    assert!(pts.iter().all(|p| (p[0] + 20.0).abs() < 1e-6), "{pts:?}");
    let top = pts.iter().map(|p| p[2]).fold(f64::MIN, f64::max);
    assert!((top - 15.0).abs() < 1e-3, "ボスの上 15 mm: {top}");
    let (lo, hi) = pts
        .iter()
        .fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p[1]), b.max(p[1])));
    assert!(lo.abs() < 1e-3 && (hi - 40.0).abs() < 1e-3, "{lo} {hi}");
}

#[test]
fn a_line_above_the_plate_is_projected_onto_it_and_intersected_through_it() {
    let mut h = on_part();
    // 板の上 20 mm の作図面に線分を引く
    ed(h.state_mut()).anchor = [0.0, 0.0, 20.0];
    press_key(&mut h, egui::Key::L);
    click3(&mut h, [33.0, 30.0, 20.0]);
    click3(&mut h, [45.0, 30.0, 20.0]);
    press_key(&mut h, egui::Key::Escape);
    press_key(&mut h, egui::Key::Escape);
    h.run();
    let s = current(h.state());
    let line = s
        .entities
        .iter()
        .find(|e| matches!(e, Entity3d::Line { .. }))
        .unwrap()
        .id();
    // 選ばずに押すと理由が出る
    more_tool(&mut h, "サーフェスに投影");
    assert!(current(h.state()).derived.is_empty());
    assert!(
        ed(h.state_mut())
            .note
            .as_deref()
            .unwrap_or("")
            .contains("選んでから")
    );
    // 線を選んで「サーフェスに投影」: 板の上面(z = 10)へ写る
    ed(h.state_mut()).selection = vec![Sel::Ent(line)];
    more_tool(&mut h, "サーフェスに投影");
    let s = current(h.state());
    assert_eq!(s.derived.len(), 1);
    let on = solution(h.state()).derived[&s.derived[0].id].clone();
    assert_eq!(on.len(), 1, "{on:?}");
    assert!(
        on[0].iter().all(|p| (p[2] - 10.0).abs() < 1e-6),
        "{:?}",
        on[0]
    );
    // 「交差曲線」: 線を縦に伸ばした面と板の上面・底面の交わり
    ed(h.state_mut()).selection = vec![Sel::Ent(line)];
    more_tool(&mut h, "交差曲線");
    let s = current(h.state());
    assert_eq!(s.derived.len(), 2);
    let both = solution(h.state()).derived[&s.derived[1].id].clone();
    let mut zs: Vec<f64> = both.iter().map(|l| l[0][2]).collect();
    zs.sort_by(f64::total_cmp);
    assert_eq!(zs.len(), 2, "{both:?}");
    assert!(
        (zs[0] - 0.0).abs() < 1e-6 && (zs[1] - 10.0).abs() < 1e-6,
        "{zs:?}"
    );
    // 元の線を消すと、写した線も消える
    let mut s2 = current(h.state());
    s2.remove_entities(&[line]);
    assert!(s2.derived.is_empty());
}

/// 左ボタンで `from` から `to` へ引く。
fn drag3(h: &mut Harness<'static, Workspace>, from: egui::Pos2, to: egui::Pos2) {
    h.hover_at(from);
    h.run();
    h.event(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
    for k in 1..=6 {
        h.hover_at(from + (to - from) * (k as f32 / 6.0));
        h.run();
    }
    h.event(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
    h.run();
}

fn label_center(h: &mut Harness<'static, Workspace>, id: ConstraintId) -> egui::Pos2 {
    h.run();
    view::label_center(ed(h.state_mut()), id).expect("札が描かれている")
}

/// 線分の真ん中(画面)。
fn line_mid(h: &mut Harness<'static, Workspace>, line: EntityId) -> egui::Pos2 {
    let (sk, sol) = (current(h.state()), solution(h.state()));
    let (a, b) = sk.line_ends(line).unwrap();
    let m = kmb_sketch_3d::geom::scale(
        kmb_sketch_3d::geom::add(sol.points[&a], sol.points[&b]),
        0.5,
    );
    ed(h.state_mut()).screen_of(m)
}

#[test]
fn a_3d_length_dimension_is_typed_on_placing_and_its_label_follows_the_line() {
    // 長さの寸法は置いてすぐ値を打て、打った長さに解ける
    // Kie のレビュー K3・K5 を 3D スケッチにも: 札は引いて動かせ、形が変わっても線に付いて来る
    let mut h = empty();
    square(&mut h);
    press_key(&mut h, egui::Key::Escape);
    press_key(&mut h, egui::Key::Escape);
    assert_eq!(ed(h.state_mut()).tool, Tool3::Select);
    // 上の辺を選んで D で寸法
    click3(&mut h, [20.0, 30.0, 0.0]);
    assert_eq!(ed(h.state_mut()).selection.len(), 1);
    press_key(&mut h, egui::Key::D);
    assert!(
        ed(h.state_mut()).edit.is_some(),
        "値を打つ欄が開いていません"
    );
    // 欄は値を全部選んで開くので、打てば置き換わる
    type_text(&mut h, "55");
    press_key(&mut h, egui::Key::Enter);
    h.run();
    let s = current(h.state());
    let (id, line) = s
        .constraints
        .iter()
        .find_map(|c| match c.kind {
            Kind3d::Length { line, .. } => Some((c.id, line)),
            _ => None,
        })
        .expect("長さの寸法");
    assert_eq!(s.constraint(id).unwrap().kind.value(), Some("55 mm"));
    let sol = solution(h.state());
    let (a, b) = s.line_ends(line).unwrap();
    let d = kmb_sketch_3d::geom::len(kmb_sketch_3d::geom::sub(sol.points[&b], sol.points[&a]));
    assert!((d - 55.0).abs() < 1e-6, "長さ {d}");
    let at = label_center(&mut h, id);
    let to = at + egui::vec2(60.0, 70.0);
    drag3(&mut h, at, to);
    let s = current(h.state());
    let c = s.constraints.iter().find(|c| c.id == id).unwrap();
    assert!(c.label.is_some(), "動かした量を覚える");
    assert_eq!(c.kind.value(), Some("55 mm"), "値は変わらない");
    let moved = label_center(&mut h, id);
    assert!(
        (moved - to).length() < 3.0,
        "札が引いた所へ {to:?} → {moved:?}"
    );
    // 上の辺の右の端を上へ引く: 辺が動き、札も同じだけ動く
    let gap = moved - line_mid(&mut h, line);
    let m0 = line_mid(&mut h, line);
    let (sk, sol) = (current(h.state()), solution(h.state()));
    let (a, b) = sk.line_ends(line).unwrap();
    let end = if sol.points[&a][0] > sol.points[&b][0] {
        a
    } else {
        b
    };
    let from = ed(h.state_mut()).screen_of(sol.points[&end]);
    drag3(&mut h, from, from + egui::vec2(0.0, -60.0));
    let m1 = line_mid(&mut h, line);
    assert!((m1 - m0).length() > 5.0, "辺が動く {m0:?} → {m1:?}");
    let now = label_center(&mut h, id);
    assert!(
        (now - m1 - gap).length() < 2.0,
        "札は辺に付いて来る {gap:?} → {:?}",
        now - m1
    );
}

#[test]
fn a_3d_circle_dimension_switches_between_radius_and_diameter() {
    let mut s = Sketch3d::default();
    let c = s.add_point([0.0; 3]);
    let circle = s.add_circle(c, 5.0, [0.0, 0.0, 1.0]);
    let id = s.add_constraint(Kind3d::Diameter {
        circle,
        value: "10 mm".into(),
    });
    assert!(s.toggle_radius(id));
    assert!(matches!(&s.constraint(id).unwrap().kind,
        Kind3d::Radius { value, .. } if value == "5 mm"));
    assert!(s.toggle_radius(id));
    assert!(matches!(&s.constraint(id).unwrap().kind,
        Kind3d::Diameter { value, .. } if value == "10 mm"));
}

/// 線の長さを打ったままクリック: Enter と同じく、クリックした向きへ打った長さで引く。
#[test]
fn a_typed_line_length_is_drawn_by_a_click() {
    let mut h = empty();
    press_key(&mut h, egui::Key::L);
    click3(&mut h, [0.0, 0.0, 0.0]);
    type_text(&mut h, "25");
    assert_eq!(ed(h.state_mut()).typed, "25");
    click3(&mut h, [10.0, 0.0, 0.0]);
    h.run();
    let sol = solution(h.state());
    assert!(
        sol.points
            .values()
            .any(|p| (p[0] - 25.0).abs() < 1e-6 && p[1].abs() < 1e-6 && p[2].abs() < 1e-6),
        "{:?}",
        sol.points
    );
    assert!(ed(h.state_mut()).typed.is_empty());
}

#[test]
fn including_a_face_or_a_whole_body_takes_its_edges_together() {
    let mut h = on_part();
    h.get_by_label("3D 取り込み").click();
    h.run();
    // パネルで「面の縁」を選び、板の手前の側面(y = 0)の真ん中を押す
    h.get_by_label("面の縁(押した面の辺と円)").click();
    h.run();
    click3(&mut h, [30.0, 0.0, 5.0]);
    h.run();
    let s = current(h.state());
    let lines = s
        .includes
        .iter()
        .filter(|i| s.line_ends(i.entity).is_some())
        .count();
    assert_eq!(lines, 4, "側面の縁の 4 辺: {:?}", s.includes);
    // 「ボディの角・辺・円すべて」なら、押したボディの物をまとめて
    h.get_by_label("ボディの角・辺・円すべて").click();
    h.run();
    let all = ed(h.state_mut()).body_items.len();
    click3(&mut h, [30.0, 20.0, 10.0]);
    h.run();
    let n = current(h.state()).includes.len();
    assert!(n > 20 && n <= all, "{n} / {all}");
}

#[test]
fn the_3d_sketch_follows_the_perspective_setting() {
    // まとめてレビュー R7 の気づき: 表示の「透視投影」を選んでも、3D スケッチは平行投影のままだった
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws());
    h.run();
    // 表示の形と投影は、ビューの下の帯の「表示 ▾」で選ぶ
    h.get_by_label("表示").click();
    h.run();
    h.get_by_label("透視投影").click();
    h.run();
    h.state_mut().run_action(kmb_sketch_3d::ADD_SKETCH3D);
    h.run();
    h.run();
    assert!(ed(h.state_mut()).cam.perspective, "透視投影で描く");
    h.state_mut().layout_mut().projection = kmb_ui::Projection::Parallel;
    h.run();
    assert!(!ed(h.state_mut()).cam.perspective, "平行投影に戻せる");
}

/// まとめてレビュー R3: 道具を使っている間も Enter で終えられる。つないでいる線分は 1 回目の Enter で
/// つなぐのを終え(2D と同じ)、もう一度で道具を終える。終えた道具は右クリックで繰り返せる。
#[test]
fn enter_ends_the_chain_and_then_the_tool() {
    let mut h = empty();
    press_key(&mut h, egui::Key::L);
    click3(&mut h, [0.0, 0.0, 0.0]);
    click3(&mut h, [30.0, 0.0, 0.0]);
    assert!(ed(h.state_mut()).start.is_some(), "続けて引ける");
    press_key(&mut h, egui::Key::Enter);
    assert!(ed(h.state_mut()).start.is_none(), "つなぐのを終える");
    assert_eq!(ed(h.state_mut()).tool, Tool3::Line, "線分の道具は続く");
    press_key(&mut h, egui::Key::Enter);
    assert_eq!(ed(h.state_mut()).tool, Tool3::Select, "道具を終える");
    assert_eq!(lines(&current(h.state())), 1, "描いた線は残る");
    // 矩形・円は描きかけがあってもなくても、Enter で道具を終える
    press_key(&mut h, egui::Key::R);
    click3(&mut h, [0.0, 10.0, 0.0]);
    press_key(&mut h, egui::Key::Enter);
    assert_eq!(ed(h.state_mut()).tool, Tool3::Select);
    assert_eq!(lines(&current(h.state())), 1, "描きかけの矩形は捨てる");
    // 何も選ばずに右クリック: 「繰り返し: 長方形」で長方形の道具に戻る
    let r = ed(h.state_mut()).rect;
    crate::tests::right_click_px(&mut h, r.center() + egui::vec2(-200.0, 150.0));
    crate::tests::menu_item(&h, "元に戻す");
    h.get_by_label("繰り返し: 長方形").click();
    h.run();
    assert_eq!(ed(h.state_mut()).tool, Tool3::Rectangle);
}

/// まとめてレビュー R3: 道具の中の右クリックは OK・キャンセル。
#[test]
fn right_click_in_a_tool_offers_ok_and_cancel() {
    let mut h = empty();
    press_key(&mut h, egui::Key::L);
    click3(&mut h, [0.0, 0.0, 0.0]);
    click3(&mut h, [30.0, 0.0, 0.0]);
    // 長さを打ったまま右クリックの OK: 打った長さの線分を引いてから道具を終える
    type_text(&mut h, "12");
    let at = ed(h.state_mut()).screen_of([30.0, 40.0, 0.0]);
    crate::tests::right_click_px(&mut h, at);
    crate::tests::menu_item(&h, "キャンセル");
    // メニューの行(キーは名前の右に出す)とパネルの欄
    crate::tests::menu_item(&h, "補助線で描く");
    h.get_by_label("補助線で描く(X)");
    crate::tests::menu_item(&h, "OK").click();
    h.run();
    assert_eq!(ed(h.state_mut()).tool, Tool3::Select);
    assert_eq!(lines(&current(h.state())), 2);
    let sol = solution(h.state());
    assert!(
        sol.points
            .values()
            .any(|p| (p[0] - 30.0).abs() < 1e-4 && (p[1] - 12.0).abs() < 1e-4),
        "右クリックした向き(Y)へ 12 mm: {:?}",
        sol.points
    );
    // キャンセル: 描きかけを捨てて終える
    press_key(&mut h, egui::Key::C);
    click3(&mut h, [60.0, 0.0, 0.0]);
    let at = ed(h.state_mut()).screen_of([60.0, 20.0, 0.0]);
    crate::tests::right_click_px(&mut h, at);
    crate::tests::menu_item(&h, "キャンセル").click();
    h.run();
    assert_eq!(ed(h.state_mut()).tool, Tool3::Select);
    assert!(ed(h.state_mut()).start.is_none());
}

/// まとめてレビュー R3: 選ぶ道具で線を右クリックすると、その線を選んでメニューを出す。
#[test]
fn right_click_on_a_line_selects_it_and_offers_its_actions() {
    let mut h = empty();
    press_key(&mut h, egui::Key::L);
    click3(&mut h, [50.0, 0.0, 0.0]);
    click3(&mut h, [80.0, 20.0, 0.0]);
    press_key(&mut h, egui::Key::Escape);
    press_key(&mut h, egui::Key::Escape);
    let top = ed(h.state_mut()).screen_of([65.0, 10.0, 0.0]);
    crate::tests::right_click_px(&mut h, top);
    let s = current(h.state());
    let picked = ed(h.state_mut()).selection.clone();
    assert_eq!(picked.len(), 1, "右クリックした線を選ぶ");
    let Sel::Ent(line) = picked[0] else {
        panic!("線を選ぶ: {picked:?}")
    };
    assert!(s.line_ends(line).is_some());
    crate::tests::menu_item(&h, "スケッチ寸法");
    crate::tests::menu_item(&h, "標準/補助線");
    h.get_by_label("Z に沿わせる");
    crate::tests::menu_item(&h, "選択を外す");
    h.get_by_label("固定する").click();
    h.run();
    let s = current(h.state());
    let (a, b) = s.line_ends(line).unwrap();
    for p in [a, b] {
        assert!(
            s.constraints
                .iter()
                .any(|c| c.kind == Kind3d::Fix { point: p }),
            "線の両端を固定する"
        );
    }
    // もう一度右クリックすると「固定解除」
    crate::tests::right_click_px(&mut h, top);
    h.get_by_label("固定解除").click();
    h.run();
    let s = current(h.state());
    assert!(
        !s.constraints
            .iter()
            .any(|c| c.kind == Kind3d::Fix { point: a } || c.kind == Kind3d::Fix { point: b })
    );
}

/// まとめてレビュー R5: 線分の 1 点目から、画面で軸の線の方へ動かすと軸のガイドに乗り、作図面の外
/// (XY の作図面から Z の向き)にも終点を置ける。軸に沿う拘束が付く。
#[test]
fn the_axis_guide_draws_a_line_off_the_drawing_plane() {
    let mut h = empty();
    h.get_by_label("斜めから").click();
    h.run();
    press_key(&mut h, egui::Key::L);
    click3(&mut h, [10.0, 10.0, 0.0]);
    let up = ed(h.state_mut()).screen_of([10.0, 10.0, 20.0]);
    // 軸の線から少し(4 px)ずれた所でもガイドに乗る
    let near = up + egui::vec2(4.0, 0.0);
    let guess = ed(h.state_mut()).axis_guide([10.0, 10.0, 0.0], near);
    assert!(
        matches!(guess, Some((Axis::Z, p)) if (p[2] - 20.0).abs() < 1.5 && p[0] == 10.0),
        "{guess:?}"
    );
    crate::tests::click_px(&mut h, near);
    h.run();
    let s = current(h.state());
    let sol = solution(h.state());
    let top = sol
        .points
        .iter()
        .find(|(_, p)| p[2] > 10.0)
        .map(|(id, p)| (*id, *p))
        .expect("作図面の外の点");
    assert!((top.1[0] - 10.0).abs() < 1e-4 && (top.1[1] - 10.0).abs() < 1e-4);
    assert!(
        s.constraints
            .iter()
            .any(|c| matches!(c.kind, Kind3d::Along { axis: Axis::Z, .. })),
        "Z に沿う"
    );
    // 長さを打って Enter: ガイドの向き(ここでは X)へその長さ。続きは上の点から
    let x = ed(h.state_mut()).screen_of([40.0, 10.0, top.1[2]]);
    h.hover_at(x);
    h.run();
    type_text(&mut h, "15");
    press_key(&mut h, egui::Key::Enter);
    h.run();
    let sol = solution(h.state());
    assert!(
        sol.points.values().any(|p| (p[0] - 25.0).abs() < 1e-4
            && (p[1] - 10.0).abs() < 1e-4
            && (p[2] - top.1[2]).abs() < 1e-4),
        "{:?}",
        sol.points
    );
}

/// 寸法の値の欄は、Enter の代わりに欄の外(図の空いた所)を左クリックしても決まる(2D と同じ。K1)。
/// まとめてレビュー R7: 寸法の札は 2D と同じく、地の箱なしで寸法線の上に書く(札は形から離れる)。
/// 2 本の線の角度は、交わる角を中心に円弧で描く。
#[test]
fn dimensions_are_kept_by_clicking_outside_and_drawn_with_lines_like_the_2d_sketch() {
    let mut h = empty();
    square(&mut h);
    press_key(&mut h, egui::Key::Escape);
    press_key(&mut h, egui::Key::Escape);
    // 下の辺の長さ
    click3(&mut h, [20.0, 0.0, 0.0]);
    press_key(&mut h, egui::Key::D);
    press_key(&mut h, egui::Key::Enter);
    h.run();
    let s = current(h.state());
    let id = s
        .constraints
        .iter()
        .find(|c| matches!(c.kind, Kind3d::Length { .. }))
        .map(|c| c.id)
        .unwrap();
    let at = label_center(&mut h, id);
    let line = ed(h.state_mut()).screen_of([20.0, 0.0, 0.0]);
    let inside = ed(h.state_mut()).screen_of([20.0, 15.0, 0.0]);
    // 形の外側(四角の真ん中から離れる側)に、線から離して置く(字は寸法線の上に書く)
    assert!((at - line).length() > 10.0, "{at:?} / {line:?}");
    assert!(
        (at - inside).length() > (line - inside).length(),
        "外側に: {at:?}"
    );
    // 下の辺と右の辺の角度: 角を中心にした円弧
    click3(&mut h, [20.0, 0.0, 0.0]);
    let right = ed(h.state_mut()).screen_of([40.0, 15.0, 0.0]);
    crate::menu::tests::click_with_px(&mut h, right, egui::Modifiers::SHIFT);
    press_key(&mut h, egui::Key::D);
    press_key(&mut h, egui::Key::Enter);
    h.run();
    let (sk, sol) = (current(h.state()), solution(h.state()));
    let angle = sk
        .constraints
        .iter()
        .find(|c| matches!(c.kind, Kind3d::Angle { .. }))
        .map(|c| c.id)
        .expect("角度の寸法");
    let (cam, rect) = {
        let e = ed(h.state_mut());
        (e.cam, e.rect)
    };
    let pr = view::Proj {
        cam: &cam,
        rect,
        frame: kmb_core::Plane::XY,
        fmt: kmb_sketch::DimFormat::default(),
    };
    let d = view::dims(&sk, &sol, &pr)
        .into_iter()
        .find(|d| d.id == angle)
        .expect("角度の寸法を描く");
    let corner = pr.at([40.0, 0.0, 0.0]);
    match &d.shape {
        crate::annot::ScreenDim::Arc { pts, .. } => {
            // 円弧は角を中心に、画面で一番広い所が 40 px ほど(斜めから見ると楕円)
            let far = pts
                .iter()
                .map(|p| (*p - corner).length())
                .fold(0.0, f32::max);
            assert!(far > 30.0 && far < 41.0, "角から {far} px");
            assert!((pts[0] - corner).length() > 10.0);
        }
        _ => panic!("円弧で描く"),
    }
    // 右の辺の長さ(今の長さと同じ 30 を打ち、形は変えない)を、欄の外のクリックで決める
    // (上の辺はもう決まっていて測るだけの寸法になるので、まだ決まっていない右の辺で)
    click3(&mut h, [40.0, 15.0, 0.0]);
    press_key(&mut h, egui::Key::D);
    assert!(ed(h.state_mut()).edit.is_some(), "値を打つ欄が開く");
    type_text(&mut h, "30");
    // 図の空いた所(左下の隅の近く)を押してすぐ離す
    let r = ed(h.state_mut()).rect;
    crate::tests::quick_click_px(&mut h, egui::pos2(r.left() + 60.0, r.bottom() - 60.0));
    h.run();
    let (s, sol) = (current(h.state()), solution(h.state()));
    let len = s.constraints.iter().find_map(|c| match &c.kind {
        Kind3d::Length { line, value }
            if s.line_ends(*line)
                .is_some_and(|(a, b)| sol.points[&a][0] > 39.0 && sol.points[&b][0] > 39.0) =>
        {
            Some(value.clone())
        }
        _ => None,
    });
    assert_eq!(len.as_deref(), Some("30 mm"));
    assert!(ed(h.state_mut()).edit.is_none());
}

#[test]
fn look_at_faces_the_drawing_plane_and_s_opens_the_search() {
    // K9: パレットの「正面から見る」で今の作図面を真正面から見る(向きは滑らかに替わる)
    let mut h = empty();
    h.get_all_by_label("YZ 平面").next().unwrap().click();
    h.run();
    h.get_all_by_label("正面から見る").next().unwrap().click();
    h.run();
    let cam = ed(h.state_mut()).cam;
    assert!(cam.is_view(0.0, 0.0), "YZ の作図面を +X から");
    assert!(!cam.turning());
    h.get_all_by_label("XY 平面").next().unwrap().click();
    h.run();
    h.get_all_by_label("正面から見る").next().unwrap().click();
    h.run();
    assert!(ed(h.state_mut()).cam.is_view(-90.0, 89.9), "XY は上から");
    // S: カーソルの所にコマンド検索(3D のビューと同じ)
    press_key(&mut h, egui::Key::S);
    h.run();
    assert!(h.state().is_command_palette_open());
}

#[test]
fn every_3d_sketch_tool_has_a_role_or_one_layer() {
    // 3D スケッチの帯の道具も、2D と同じく役の色の変わる部分を持つか 1 層の表にある
    let mut h = empty();
    let groups = h.state_mut().view_tool_groups();
    assert!(!groups.is_empty());
    let gaps = kmb_ui::board::layer_gaps(&crate::tests::all_tools(&groups));
    assert!(gaps.is_empty(), "{}", gaps.join("\n"));
}
