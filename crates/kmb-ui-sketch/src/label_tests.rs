//! 寸法の札を、人の操作(クリック、引く、右クリック)で確かめる(Kie のレビュー K3・K4・K5・K9):
//! 図形を動かしても札が付いて来る、平行な 2 線の距離の札を動かせる、角度の札を反対側へ
//! 動かせる(値は同じ)、補角の側で置けば補角になる、直径の札を円から離せて半径に切り替えられる。

use crate::Tool;
use crate::tests::{
    click_at, current_sketch, drag_px, editor, empty_sketch, move_to, press_enter, press_key,
    quick_click, right_click_px, screen, sketch_with_rectangle, type_text,
};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kmb_sketch::{ConstraintId, ConstraintKind, Entity, EntityId, Sketch, Solution};
use kmb_ui::Workspace;

fn label_of(h: &mut Harness<'static, Workspace>, id: ConstraintId) -> egui::Pos2 {
    editor(h.state_mut())
        .view
        .labels
        .iter()
        .find(|(c, _)| *c == id)
        .map(|(_, r)| r.center())
        .expect("札が描かれている")
}

/// 今の形を、寸法の値も入れて解いた位置。
fn solved(ws: &Workspace) -> Solution {
    let s = current_sketch(ws);
    let v = kmb_sketch::dimension_values(&s, &|_| None).unwrap();
    kmb_sketch::solve(&s, &v).unwrap()
}

fn label_world(h: &Harness<'static, Workspace>, id: ConstraintId) -> [f64; 2] {
    let s = current_sketch(h.state());
    let c = s.constraints.iter().find(|c| c.id == id).unwrap();
    s.label_at(c, &solved(h.state())).expect("札を置いた位置")
}

fn to_screen(h: &mut Harness<'static, Workspace>, p: [f64; 2]) -> egui::Pos2 {
    let t = editor(h.state_mut()).view.last.clone().expect("描いた後");
    t.to_screen(p)
}

fn line_mid(s: &Sketch, sol: &Solution, line: EntityId) -> [f64; 2] {
    let Some(Entity::Line { a, b, .. }) = s.entity(line) else {
        panic!("線分でない")
    };
    let (p, q) = (sol.points[a], sol.points[b]);
    [(p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0]
}

fn dim_of(h: &Harness<'static, Workspace>, f: impl Fn(&ConstraintKind) -> bool) -> ConstraintId {
    current_sketch(h.state())
        .constraints
        .iter()
        .find(|c| f(&c.kind))
        .expect("寸法")
        .id
}

fn to_select(h: &mut Harness<'static, Workspace>) {
    press_key(h, egui::Key::Escape);
    press_key(h, egui::Key::Escape);
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::Select);
    h.run();
}

/// 長方形の上の辺に長さの寸法(30)を置き、選ぶ道具へ戻ったところ。
fn rectangle_with_top_length(h: &mut Harness<'static, Workspace>) -> ConstraintId {
    press_key(h, egui::Key::D);
    click_at(h, [6.4, 16.36]);
    move_to(h, [6.4, 19.49]);
    h.run();
    quick_click(h, [6.4, 19.49]);
    type_text(h, "30");
    press_enter(h);
    to_select(h);
    dim_of(h, |k| matches!(k, ConstraintKind::Length { .. }))
}

#[test]
fn a_dimension_label_follows_the_shape_when_the_shape_is_dragged() {
    let mut h = sketch_with_rectangle();
    let id = rectangle_with_top_length(&mut h);
    // 札を斜め上へずらしておく
    let at = label_of(&mut h, id);
    drag_px(
        &mut h,
        at,
        at + egui::vec2(50.0, -30.0),
        egui::Modifiers::NONE,
    );
    let line = match current_sketch(h.state())
        .constraints
        .iter()
        .find(|c| c.id == id)
        .unwrap()
        .kind
    {
        ConstraintKind::Length { line, .. } => line,
        _ => unreachable!(),
    };
    let (s, sol) = (current_sketch(h.state()), solved(h.state()));
    let mid = line_mid(&s, &sol, line);
    let gap = |p: [f64; 2], m: [f64; 2]| [p[0] - m[0], p[1] - m[1]];
    let before = gap(label_world(&h, id), mid);
    // 上の辺を掴んで上へ引く(長方形の高さが変わり、辺が動く)
    let from = to_screen(&mut h, mid);
    drag_px(
        &mut h,
        from,
        from + egui::vec2(0.0, -60.0),
        egui::Modifiers::NONE,
    );
    let (s, sol) = (current_sketch(h.state()), solved(h.state()));
    let moved = line_mid(&s, &sol, line);
    assert!(
        (moved[1] - mid[1]).abs() > 1.0,
        "辺が動く {mid:?} → {moved:?}"
    );
    let after = gap(label_world(&h, id), moved);
    assert!(
        (after[0] - before[0]).abs() < 1e-6 && (after[1] - before[1]).abs() < 1e-6,
        "札は辺との相対の位置のまま {before:?} → {after:?}"
    );
}

#[test]
fn the_distance_label_between_two_parallel_lines_can_be_moved() {
    let mut h = sketch_with_rectangle();
    // 上と下の辺(平行)を選ぶと距離の寸法
    press_key(&mut h, egui::Key::D);
    click_at(&mut h, [6.4, 16.36]);
    click_at(&mut h, [6.4, 8.54]);
    move_to(&mut h, [8.49, 12.45]);
    h.run();
    let ghost = editor(h.state_mut()).editor.ghost.clone();
    assert!(
        matches!(ghost, Some((ConstraintKind::PointLineDistance { .. }, _))),
        "{ghost:?}"
    );
    quick_click(&mut h, [8.49, 12.45]);
    press_enter(&mut h);
    to_select(&mut h);
    let id = dim_of(&h, |k| {
        matches!(k, ConstraintKind::PointLineDistance { .. })
    });
    let at = label_of(&mut h, id);
    // 右の外へ引く: 札が付いて来る(線の真ん中に残らない)
    drag_px(
        &mut h,
        at,
        at + egui::vec2(160.0, -20.0),
        egui::Modifiers::NONE,
    );
    let moved = label_of(&mut h, id);
    assert!(
        (moved.x - (at.x + 160.0)).abs() < 3.0 && (moved.y - (at.y - 20.0)).abs() < 3.0,
        "札が引いた所へ {at:?} → {moved:?}"
    );
}

/// 頂点 (400, 500) で交わる 2 本の線(横と、右上へ)を描く。
fn two_lines(h: &mut Harness<'static, Workspace>) {
    editor(h.state_mut()).set_tool(Tool::Line);
    h.run();
    click_at(h, [-4.03, 5.93]);
    click_at(h, [6.4, 5.93]);
    to_select(h);
    editor(h.state_mut()).set_tool(Tool::Line);
    h.run();
    click_at(h, [-4.03, 5.93]);
    click_at(h, [3.8, 12.19]);
    to_select(h);
}

#[test]
fn an_angle_label_moves_to_the_opposite_side_with_the_lines_extended() {
    let mut h = empty_sketch();
    two_lines(&mut h);
    press_key(&mut h, egui::Key::D);
    click_at(&mut h, [1.19, 5.93]);
    click_at(&mut h, [-0.11, 9.06]);
    // 内角の側に置く
    move_to(&mut h, [0.15, 6.98]);
    h.run();
    quick_click(&mut h, [0.15, 6.98]);
    press_enter(&mut h);
    to_select(&mut h);
    let id = dim_of(&h, |k| matches!(k, ConstraintKind::Angle { .. }));
    let value = |h: &Harness<'static, Workspace>| {
        let s = current_sketch(h.state());
        let c = s.constraints.iter().find(|c| c.id == id).unwrap().clone();
        (c.kind.value().unwrap().to_string(), c.kind)
    };
    let (v0, k0) = value(&h);
    let at = label_of(&mut h, id);
    assert!(at.x > 400.0 && at.y < 500.0, "内角の側 {at:?}");
    // 反対側(左下)へ引く: 値と線の順は同じまま、札は向かいの角に
    let to = screen(&h, [-7.15, 3.59]);
    drag_px(&mut h, at, to, egui::Modifiers::NONE);
    let moved = label_of(&mut h, id);
    assert!(moved.x < 400.0 && moved.y > 500.0, "向かいの角 {moved:?}");
    assert_eq!(value(&h), (v0, k0), "値は変わらない");
}

#[test]
fn placing_an_angle_on_the_supplementary_side_measures_the_supplement() {
    let mut h = empty_sketch();
    two_lines(&mut h);
    press_key(&mut h, egui::Key::D);
    click_at(&mut h, [1.19, 5.93]);
    click_at(&mut h, [-0.11, 9.06]);
    let measured = |h: &mut Harness<'static, Workspace>, x: f32, y: f32| {
        h.hover_at(egui::pos2(x, y));
        h.run();
        let (s, sol) = (current_sketch(h.state()), solved(h.state()));
        let (k, _) = editor(h.state_mut()).editor.ghost.clone().expect("角度");
        kmb_sketch::measure(&s, &sol, &k).unwrap().to_degrees()
    };
    let inner = measured(&mut h, 480.0, 480.0);
    // 頂点の左上(横の線を左へ延ばした側と、右上の線の間): 補角
    let outer = measured(&mut h, 340.0, 470.0);
    assert!(inner < 90.0, "内角 {inner}");
    assert!(
        (inner + outer - 180.0).abs() < 1e-6,
        "補角 {inner} + {outer}"
    );
    quick_click(&mut h, [-7.15, 7.5]);
    press_enter(&mut h);
    let s = current_sketch(h.state());
    let c = s
        .constraints
        .iter()
        .find(|c| matches!(c.kind, ConstraintKind::Angle { .. }))
        .unwrap();
    let v: f64 = c
        .kind
        .value()
        .unwrap()
        .trim_end_matches("deg")
        .trim()
        .parse()
        .unwrap();
    assert!((v - outer).abs() < 1e-3, "置いた値は補角 {v} ≠ {outer}");
}

#[test]
fn a_diameter_label_can_be_moved_off_the_circle_and_switched_to_radius() {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Circle);
    h.run();
    click_at(&mut h, [6.4, 13.75]);
    click_at(&mut h, [9.01, 13.75]);
    press_key(&mut h, egui::Key::Escape);
    press_key(&mut h, egui::Key::D);
    click_at(&mut h, [9.01, 13.75]);
    move_to(&mut h, [11.62, 15.84]);
    h.run();
    quick_click(&mut h, [11.62, 15.84]);
    press_enter(&mut h);
    to_select(&mut h);
    let id = dim_of(&h, |k| matches!(k, ConstraintKind::Diameter { .. }));
    let at = label_of(&mut h, id);
    // 円から遠くへ引く: 札は引いた所(円周から引出し線)
    drag_px(
        &mut h,
        at,
        at + egui::vec2(80.0, -60.0),
        egui::Modifiers::NONE,
    );
    let moved = label_of(&mut h, id);
    assert!(
        (moved.x - (at.x + 80.0)).abs() < 3.0 && (moved.y - (at.y - 60.0)).abs() < 3.0,
        "札が引いた所へ {at:?} → {moved:?}"
    );
    let d = current_sketch(h.state())
        .constraints
        .iter()
        .find(|c| c.id == id)
        .unwrap()
        .kind
        .value()
        .unwrap()
        .trim_end_matches("mm")
        .trim()
        .parse::<f64>()
        .unwrap();
    // 札の右クリックで半径にする: 同じ円のまま、値は半分、札はその場
    right_click_px(&mut h, moved);
    h.get_by_label("半径の寸法にする").click();
    h.run();
    h.run();
    let s = current_sketch(h.state());
    let c = s.constraints.iter().find(|c| c.id == id).unwrap();
    let ConstraintKind::Radius { value, .. } = &c.kind else {
        panic!("半径になる {:?}", c.kind)
    };
    let r: f64 = value.trim_end_matches("mm").trim().parse().unwrap();
    assert!((r * 2.0 - d).abs() < 1e-9, "半径 {r} ・直径 {d}");
    let now = label_of(&mut h, id);
    assert!(
        (now - moved).length() < 3.0,
        "札はその場 {moved:?} → {now:?}"
    );
    // もう一度で直径に戻る
    right_click_px(&mut h, now);
    h.get_by_label("直径の寸法にする").click();
    h.run();
    let s = current_sketch(h.state());
    assert!(matches!(
        s.constraints.iter().find(|c| c.id == id).unwrap().kind,
        ConstraintKind::Diameter { .. }
    ));
}

/// 値は寸法線の上に書き(地の箱なし)、札を掴んで引くと引いた分だけ動く
/// (字を寸法線から浮かせても、掴んだ時に跳ねない)。長さと角度で確かめる。
#[test]
fn a_label_sits_above_its_line_and_moves_exactly_as_dragged() {
    let mut h = sketch_with_rectangle();
    let id = rectangle_with_top_length(&mut h);
    let at = label_of(&mut h, id);
    // 上の辺の上に置いた寸法線より、さらに上に字(辺から 60 px より上)。画面の px で決め打ちすると、
    // 下のタイムラインの高さが変わった時にビューがずれて落ちるので、辺と比べる
    let edge = to_screen(&mut h, [6.4, 16.36]);
    assert!(at.y < edge.y - 60.0, "字は寸法線の上 {at:?}(辺 {edge:?})");
    for (dx, dy) in [(40.0, -20.0), (-25.0, -10.0)] {
        let at = label_of(&mut h, id);
        drag_px(&mut h, at, at + egui::vec2(dx, dy), egui::Modifiers::NONE);
        let moved = label_of(&mut h, id);
        assert!(
            (moved.x - (at.x + dx)).abs() < 3.0 && (moved.y - (at.y + dy)).abs() < 3.0,
            "長さの札が引いた所へ {at:?} → {moved:?}"
        );
    }

    let mut h = empty_sketch();
    two_lines(&mut h);
    press_key(&mut h, egui::Key::D);
    click_at(&mut h, [1.19, 5.93]);
    click_at(&mut h, [-0.11, 9.06]);
    move_to(&mut h, [2.23, 7.5]);
    h.run();
    quick_click(&mut h, [2.23, 7.5]);
    press_enter(&mut h);
    to_select(&mut h);
    let id = dim_of(&h, |k| matches!(k, ConstraintKind::Angle { .. }));
    // 扇の中で外へ引く
    let at = label_of(&mut h, id);
    drag_px(
        &mut h,
        at,
        at + egui::vec2(30.0, -12.0),
        egui::Modifiers::NONE,
    );
    let moved = label_of(&mut h, id);
    assert!(
        (moved.x - (at.x + 30.0)).abs() < 3.0 && (moved.y - (at.y - 12.0)).abs() < 3.0,
        "角度の札が引いた所へ {at:?} → {moved:?}"
    );
}

/// 1 点から出る 2 本の線の角度寸法は、線を描いた向き(端の順)によらず、
/// カーソルのある扇の角を測る。付けて値を変えて解いても、線は裏返らず値どおりに開く。
/// 鈍角の V(0° と 120°)で、2 本の向きの 4 通りを確かめる。
#[test]
fn an_angle_from_one_point_measures_the_sector_whatever_the_line_directions() {
    use std::collections::HashMap;
    for (flip_a, flip_b) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut s = Sketch::default();
        let o = s.add_point(0.0, 0.0);
        let p = s.add_point(10.0, 0.0);
        let q = s.add_point(-5.0, 75f64.sqrt());
        let la = if flip_a {
            s.add_line(p, o)
        } else {
            s.add_line(o, p)
        };
        let lb = if flip_b {
            s.add_line(q, o)
        } else {
            s.add_line(o, q)
        };
        let sol = kmb_sketch::solve(&s, &HashMap::new()).unwrap();
        let case = format!("1 本目の向きを逆に {flip_a}、2 本目を逆に {flip_b}");
        // 扇の向き(度)と、そこにカーソルを出した時に測る角
        for (dir, want) in [(60.0, 120.0), (270.0, 120.0), (150.0, 60.0), (330.0, 60.0)] {
            let r: f64 = f64::to_radians(dir);
            let w = [3.0 * r.cos(), 3.0 * r.sin()];
            let k = crate::view::angle_toward(&s, &sol, la, lb, "", w).unwrap();
            let got = kmb_sketch::measure(&s, &sol, &k).unwrap().to_degrees();
            assert!(
                (got - want).abs() < 1e-6,
                "{case}: {dir}° の扇で {got}°(欲しいのは {want}°)"
            );
        }
        // V の内側に置いた寸法を 100° にして解く: 2 本目は上の側のまま 100° に閉じる
        let k = crate::view::angle_toward(&s, &sol, la, lb, "100 deg", [1.0, 1.7]).unwrap();
        let id = s.add_constraint(k.clone());
        let values = HashMap::from([(id, 100f64.to_radians())]);
        let after = kmb_sketch::solve(&s, &values).unwrap();
        let got = kmb_sketch::measure(&s, &after, &k).unwrap().to_degrees();
        assert!((got - 100.0).abs() < 1e-4, "{case}: 解いた角 {got}°");
        let (po, pa, pb) = (after.points[&o], after.points[&p], after.points[&q]);
        let (ua, ub) = (
            [pa[0] - po[0], pa[1] - po[1]],
            [pb[0] - po[0], pb[1] - po[1]],
        );
        let open = (ua[0] * ub[1] - ua[1] * ub[0])
            .atan2(ua[0] * ub[0] + ua[1] * ub[1])
            .to_degrees()
            .abs();
        assert!(
            (open - 100.0).abs() < 1e-3,
            "{case}: 2 本は頂点から {open}° に開く(80° なら裏返った)"
        );
    }
}

/// 寸法の札を右クリックして、駆動の寸法と参照の寸法を切り替える(編集パネルを開かずに)。
#[test]
fn a_dimension_is_switched_between_driving_and_reference_from_its_label() {
    let mut h = sketch_with_rectangle();
    let id = rectangle_with_top_length(&mut h);
    let reference = |h: &Harness<'static, Workspace>| {
        current_sketch(h.state())
            .constraints
            .iter()
            .find(|c| c.id == id)
            .unwrap()
            .reference
    };
    assert!(!reference(&h));
    let at = label_of(&mut h, id);
    right_click_px(&mut h, at);
    h.get_by_label("参照にする").click();
    h.run();
    h.run();
    assert!(reference(&h), "参照の寸法になる");
    let at = label_of(&mut h, id);
    right_click_px(&mut h, at);
    h.get_by_label("駆動にする").click();
    h.run();
    h.run();
    assert!(!reference(&h), "駆動の寸法に戻る");
}
