//! 直す道具(網羅版 D1・D2・D3・D4・D6)を、人の操作(クリック、キー、引く)で確かめる。

use crate::Tool;
use crate::tests::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use kmb_sketch::{ConstraintKind, Entity, EntityId};
use kmb_ui::Workspace;

fn solution(h: &Harness<'static, Workspace>) -> kmb_sketch::Solution {
    let s = current_sketch(h.state());
    let v = kmb_sketch::dimension_values(&s, &|_| None).unwrap();
    kmb_sketch::solve(&s, &v).unwrap()
}

fn line_ends(h: &Harness<'static, Workspace>) -> Vec<(EntityId, [f64; 2], [f64; 2])> {
    let s = current_sketch(h.state());
    let sol = solution(h);
    s.entities
        .iter()
        .filter_map(|e| match e {
            Entity::Line { id, a, b, .. } => Some((*id, sol.points[a], sol.points[b])),
            _ => None,
        })
        .collect()
}

/// 長方形と、その真ん中を左右にはみ出して横切る線分。
fn rectangle_with_crossing_line() -> Harness<'static, Workspace> {
    let mut h = sketch_with_rectangle();
    editor(h.state_mut()).set_tool(Tool::Line);
    h.run();
    click_at(&mut h, [-1.42, 12.45]);
    click_at(&mut h, [14.22, 12.45]);
    press_key(&mut h, egui::Key::Escape);
    press_key(&mut h, egui::Key::Escape);
    assert_eq!(line_ends(&h).len(), 5);
    h
}

fn x_range(h: &Harness<'static, Workspace>) -> (f64, f64) {
    let sol = solution(h);
    let s = current_sketch(h.state());
    // 長方形の角(線分 4 本が使う点)の x の幅
    let xs: Vec<f64> = s
        .entities
        .iter()
        .take(8)
        .filter_map(|e| match e {
            Entity::Point { id, .. } => Some(sol.points[id][0]),
            _ => None,
        })
        .collect();
    (
        xs.iter().cloned().fold(f64::MAX, f64::min),
        xs.iter().cloned().fold(f64::MIN, f64::max),
    )
}

#[test]
fn dragging_across_curves_in_trim_removes_each_crossed_span() {
    let mut h = rectangle_with_crossing_line();
    let (lo, hi) = x_range(&h);
    editor(h.state_mut()).set_tool(Tool::Trim);
    h.run();
    // 左右にはみ出した所をなぞって消す(長方形の中は通らない)
    drag_on(
        &mut h,
        [-0.38, 13.49],
        [-0.38, 11.41],
        egui::Modifiers::NONE,
    );
    drag_on(
        &mut h,
        [13.18, 13.49],
        [13.18, 11.41],
        egui::Modifiers::NONE,
    );
    let ls = line_ends(&h);
    assert_eq!(ls.len(), 5, "はみ出しだけ消える {ls:?}");
    for (_, p, q) in ls {
        for x in [p[0], q[0]] {
            assert!(x >= lo - 1e-6 && x <= hi + 1e-6, "はみ出しが残る x={x}");
        }
    }
}

#[test]
fn extend_reaches_the_next_curve_and_break_splits_at_a_crossing() {
    let mut h = sketch_with_rectangle();
    let (_, hi) = x_range(&h);
    editor(h.state_mut()).set_tool(Tool::Line);
    h.run();
    click_at(&mut h, [3.8, 12.45]);
    click_at(&mut h, [6.4, 12.45]);
    press_key(&mut h, egui::Key::Escape);
    press_key(&mut h, egui::Key::Escape);
    editor(h.state_mut()).set_tool(Tool::Extend);
    h.run();
    click_at(&mut h, [6.14, 12.45]);
    let ends = line_ends(&h);
    let (_, p, q) = ends.last().unwrap();
    assert!(
        (p[0].max(q[0]) - hi).abs() < 1e-6,
        "右の辺まで延びる {p:?} {q:?} 右 {hi}"
    );
    // 破断: 延ばした線は右の辺で終わるので、右の辺を延ばした線の所で 2 つに分ける
    editor(h.state_mut()).set_tool(Tool::Break);
    h.run();
    let n = line_ends(&h).len();
    click_at(&mut h, [11.62, 14.28]);
    assert_eq!(line_ends(&h).len(), n + 1, "右の辺が 2 本になる");
    assert_eq!(h.state().report().error_count(), 0);
}

#[test]
fn fillet_by_two_lines_with_a_typed_radius() {
    let mut h = sketch_with_rectangle();
    editor(h.state_mut()).set_tool(Tool::Fillet);
    h.run();
    type_text(&mut h, "5");
    click_at(&mut h, [6.4, 16.36]);
    click_at(&mut h, [11.62, 12.19]);
    let s = current_sketch(h.state());
    assert!(s.entities.iter().any(|e| matches!(e, Entity::Arc { .. })));
    assert!(
        s.constraints
            .iter()
            .any(|c| matches!(&c.kind, ConstraintKind::Radius { value, .. } if value == "5 mm"))
    );
    // 道具は続く
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::Fillet);
}

#[test]
fn chamfer_with_a_distance_and_an_angle() {
    let mut h = sketch_with_rectangle();
    editor(h.state_mut()).set_tool(Tool::Chamfer);
    h.run();
    type_text(&mut h, "4<30");
    click_at(&mut h, [6.4, 16.36]);
    click_at(&mut h, [11.62, 12.19]);
    let s = current_sketch(h.state());
    assert!(
        s.constraints
            .iter()
            .any(|c| matches!(&c.kind, ConstraintKind::Angle { value, .. } if value == "30 deg")),
        "{:?}",
        s.constraints
    );
    assert_eq!(h.state().report().error_count(), 0);
}

#[test]
fn move_can_make_a_copy_and_keep_the_original() {
    let mut h = sketch_with_rectangle();
    let n = line_ends(&h).len();
    click_at(&mut h, [6.4, 16.36]);
    editor(h.state_mut()).set_tool(Tool::Move);
    editor(h.state_mut()).editor.copy_mode = true;
    h.run();
    click_at(&mut h, [6.4, 21.57]);
    click_at(&mut h, [6.4, 24.18]);
    assert_eq!(line_ends(&h).len(), n + 1, "写しが 1 本増える");
}

#[test]
fn trimming_a_dimensioned_line_says_which_dimension_went() {
    let mut h = crate::tests::empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Line);
    h.run();
    // 長さ 25 を打った横の線と、それを横切る縦の線
    click_at(&mut h, [1.19, 11.15]);
    move_to(&mut h, [11.62, 11.15]);
    h.run();
    h.event(egui::Event::Text("25".into()));
    h.run();
    press_enter(&mut h);
    press_key(&mut h, egui::Key::Escape);
    click_at(&mut h, [6.4, 8.0]);
    click_at(&mut h, [6.4, 14.0]);
    press_key(&mut h, egui::Key::Escape);
    press_key(&mut h, egui::Key::Escape);
    let lengths = |h: &Harness<'static, Workspace>| {
        current_sketch(h.state())
            .constraints
            .iter()
            .filter(|c| matches!(c.kind, ConstraintKind::Length { .. }))
            .count()
    };
    assert_eq!(lengths(&h), 1);
    // 右の区間を切ると、線の長さの寸法は残った片に付け替えられないので外れる。黙って消さず知らせる
    editor(h.state_mut()).set_tool(Tool::Trim);
    h.run();
    click_at(&mut h, [16.0, 11.15]);
    assert_eq!(lengths(&h), 0);
    let notice = editor(h.state_mut())
        .editor
        .notice
        .clone()
        .unwrap_or_default();
    assert!(
        notice.starts_with("トリムで寸法 1 個を外しました"),
        "外した寸法を知らせない: {notice}"
    );
}

#[test]
fn a_click_with_a_slightly_moving_hand_still_places_a_point() {
    let mut h = crate::tests::empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Point);
    h.run();
    let points = |h: &Harness<'static, Workspace>| {
        current_sketch(h.state())
            .entities
            .iter()
            .filter(|e| matches!(e, Entity::Point { .. }))
            .count()
    };
    let a = crate::tests::screen(&h, [6.4, 11.15]);
    // 押してから離すまでに 7 点動いた(egui のクリックの 4 点を超える)
    crate::tests::drag_px(&mut h, a, a + egui::vec2(7.0, 0.0), egui::Modifiers::NONE);
    assert_eq!(points(&h), 1, "手が少し動くと点が置かれない");
    // はっきり引いた時はクリックにしない
    let b = crate::tests::screen(&h, [12.0, 11.15]);
    crate::tests::drag_px(&mut h, b, b + egui::vec2(40.0, 0.0), egui::Modifiers::NONE);
    assert_eq!(points(&h), 1);
}

#[test]
fn a_two_point_circle_takes_a_typed_diameter() {
    let mut h = crate::tests::empty_sketch();
    editor(h.state_mut()).set_tool(Tool::TwoPointCircle);
    h.run();
    // 片端をクリックし、右へカーソルを向けて直径を打って Enter
    click_at(&mut h, [6.4, 11.15]);
    move_to(&mut h, [9.0, 11.15]);
    h.event(egui::Event::Text("20".into()));
    h.run();
    press_enter(&mut h);
    let s = current_sketch(h.state());
    let r = s.entities.iter().find_map(|e| match e {
        Entity::Circle { radius, .. } => Some(*radius),
        _ => None,
    });
    assert_eq!(r, Some(10.0), "直径 20 の円にならない: {s:?}");
    assert!(s.constraints.iter().any(|c| matches!(
        &c.kind,
        ConstraintKind::Diameter { value, .. } if value == "20 mm"
    )));
}

#[test]
fn typing_x_and_y_places_a_dimensioned_point() {
    let mut h = crate::tests::empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Point);
    h.run();
    move_to(&mut h, [6.4, 11.15]);
    h.event(egui::Event::Text("12, -3.5".into()));
    h.run();
    press_enter(&mut h);
    let s = current_sketch(h.state());
    let sol = solution(&h);
    let placed = s.entities.iter().find_map(|e| match e {
        Entity::Point { id, .. } if !s.projections.iter().any(|p| p.entity == *id) => {
            Some(sol.points[id])
        }
        _ => None,
    });
    assert_eq!(placed, Some([12.0, -3.5]), "{s:?}");
    let dims: Vec<&str> = s
        .constraints
        .iter()
        .filter_map(|c| match &c.kind {
            ConstraintKind::HorizontalDistance { value, .. }
            | ConstraintKind::VerticalDistance { value, .. } => Some(value.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(dims, ["12 mm", "3.5 mm"]);
}

#[test]
fn the_rounded_corner_is_previewed_with_the_typed_radius() {
    let mut h = sketch_with_rectangle();
    editor(h.state_mut()).set_tool(Tool::Fillet);
    h.run();
    let s = current_sketch(h.state());
    let sol = solution(&h);
    let corner = s.nearest_point(&sol, 1.19, 8.54, 0.5).unwrap().entity;
    let chord = |typed: &str, h: &mut Harness<'static, Workspace>| {
        let ed = &mut editor(h.state_mut()).editor;
        ed.typed = typed.into();
        let lines = crate::view::corner_preview(ed, &s, &sol, corner);
        // 丸めた角の円弧(点の多い折れ線)の両端の間の長さ
        let arc = lines.iter().find(|p| p.len() > 2).expect("円弧が見えない");
        let (a, b) = (arc[0], arc[arc.len() - 1]);
        assert!(lines.len() >= 3, "作り直す 2 本の線も見せる");
        (a[0] - b[0]).hypot(a[1] - b[1])
    };
    // 打った半径に合わせて形が変わる(直角の角の円弧の弦は半径の √2 倍)
    assert!((chord("3", &mut h) - 3.0 * 2f64.sqrt()).abs() < 1e-6);
    assert!((chord("1", &mut h) - 2f64.sqrt()).abs() < 1e-6);
}

fn corner_rectangle() -> Harness<'static, Workspace> {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Rectangle);
    h.run();
    click_at(&mut h, [1.19, 18.97]);
    click_at(&mut h, [21.19, -1.03]);
    press_key(&mut h, egui::Key::Escape);
    h
}

fn select_corners(h: &mut Harness<'static, Workspace>, modifiers: egui::Modifiers) {
    assert_eq!(editor(h.state_mut()).editor.tool, Tool::Select);
    let sol = solution(h);
    let points: Vec<_> = current_sketch(h.state())
        .entities
        .iter()
        .filter(|e| matches!(e, Entity::Point { .. }))
        .map(|e| sol.points[&e.id()])
        .collect();
    assert_eq!(points.len(), 4);
    for (i, p) in points.into_iter().enumerate() {
        let mods = if i == 0 {
            egui::Modifiers::NONE
        } else {
            modifiers
        };
        let pos = screen(h, p);
        assert!(
            editor(h.state_mut())
                .view
                .seen
                .as_ref()
                .unwrap()
                .1
                .contains(pos),
            "point {p:?} at {pos:?}"
        );
        h.event(egui::Event::ModifiersChanged(mods));
        h.hover_at(pos);
        h.run();
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: mods,
            });
            h.step();
        }
    }
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    h.run();
    let points = current_sketch(h.state()).entities;
    let e = &editor(h.state_mut()).editor;
    assert_eq!(
        e.selection.len(),
        4,
        "tool {:?}, points {points:?}, selection {:?}",
        e.tool,
        e.selection
    );
}

#[test]
fn selected_corners_fillet_with_one_value_and_one_undo() {
    let mut h = corner_rectangle();
    select_corners(&mut h, egui::Modifiers::SHIFT);
    let before = current_sketch(h.state());
    editor(h.state_mut()).set_tool(Tool::Fillet);
    h.run();
    type_text(&mut h, "5");
    press_enter(&mut h);
    assert_eq!(
        count_of(&current_sketch(h.state()), |e| matches!(
            e,
            Entity::Arc { .. }
        )),
        4
    );
    h.get_by_label("元に戻す").click();
    h.run();
    assert_eq!(current_sketch(h.state()), before);
}

#[test]
fn selected_corners_chamfer_with_one_value_and_one_undo() {
    let mut h = corner_rectangle();
    select_corners(
        &mut h,
        egui::Modifiers {
            ctrl: true,
            command: true,
            ..egui::Modifiers::NONE
        },
    );
    let before = current_sketch(h.state());
    editor(h.state_mut()).set_tool(Tool::Chamfer);
    h.run();
    type_text(&mut h, "5");
    press_enter(&mut h);
    assert_eq!(
        count_of(&current_sketch(h.state()), |e| matches!(
            e,
            Entity::Line {
                construction: false,
                ..
            }
        )),
        8
    );
    h.get_by_label("元に戻す").click();
    h.run();
    assert_eq!(current_sketch(h.state()), before);
}

#[test]
fn selected_corners_from_a_box_are_processed_together() {
    let mut h = corner_rectangle();
    drag_on(&mut h, [0.15, 20.0], [22.23, -2.0], egui::Modifiers::NONE);
    assert_eq!(editor(h.state_mut()).editor.selection.len(), 8);
    editor(h.state_mut()).set_tool(Tool::Fillet);
    h.run();
    type_text(&mut h, "5");
    press_enter(&mut h);
    assert_eq!(
        count_of(&current_sketch(h.state()), |e| matches!(
            e,
            Entity::Arc { .. }
        )),
        4
    );
}

#[test]
fn selected_corners_refuse_all_when_only_one_corner_is_too_small() {
    for (tool, lang) in [
        (Tool::Fillet, "ja"),
        (Tool::Chamfer, "ja"),
        (Tool::Fillet, "en"),
        (Tool::Chamfer, "en"),
    ] {
        let mut h = corner_rectangle();
        h.state_mut().layout_mut().appearance.language = lang.into();
        h.run();
        let large = current_sketch(h.state());
        editor(h.state_mut()).set_tool(Tool::Rectangle);
        h.run();
        click_at(&mut h, [24.0, 10.0]);
        click_at(&mut h, [26.0, 8.0]);
        press_key(&mut h, egui::Key::Escape);
        let before = current_sketch(h.state());
        let sol = solution(&h);
        let corners: Vec<_> = before
            .entities
            .iter()
            .filter(|e| matches!(e, Entity::Point { .. }))
            .map(|e| sol.points[&e.id()])
            .collect();
        assert_eq!(corners.len(), 8);
        // 大きな四角の4隅と、小さな四角の1隅だけを、同じ囲みに入れる。
        drag_on(&mut h, [0.15, 20.0], [24.1, 9.0], egui::Modifiers::NONE);
        // この囲みでは大きな四角の下の隅が外れるので、Shift の短いクリックで足す。
        h.event(egui::Event::ModifiersChanged(egui::Modifiers::SHIFT));
        for p in &corners {
            if p[0] < 22.0 && p[1] < 9.0 {
                let pos = screen(&h, *p);
                h.hover_at(pos);
                h.step();
                for pressed in [true, false] {
                    h.event(egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::SHIFT,
                    });
                    h.step();
                }
            }
        }
        h.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        h.run();
        let selected_points = editor(h.state_mut())
            .editor
            .selection
            .iter()
            .filter(|id| matches!(before.entity(**id), Some(Entity::Point { .. })))
            .count();
        assert_eq!(selected_points, 5);
        editor(h.state_mut()).set_tool(tool);
        h.run();
        type_text(&mut h, "5");
        press_enter(&mut h);
        assert_eq!(current_sketch(h.state()), before, "{tool:?} は全部断る");
        let notice = editor(h.state_mut())
            .editor
            .notice
            .clone()
            .expect("作れない角の理由を出す");
        assert!(
            notice.starts_with(if lang == "en" {
                "The selected corners cannot be processed."
            } else {
                "選んだ角を処理できません。"
            }),
            "{notice}"
        );
        let maximum = notice
            .split_whitespace()
            .find_map(|s| s.trim_matches(['\u{2068}', '\u{2069}']).parse::<f64>().ok())
            .expect("使える上限の数を出す");
        // クリック位置は画面の単精度の座標を通る。
        assert!((maximum - 2.0).abs() < 1e-5, "{notice}");
        assert!(notice.contains("mm"), "{notice}");
        h.get_by_label(if lang == "en" { "Undo" } else { "元に戻す" })
            .click();
        h.run();
        assert_eq!(
            current_sketch(h.state()),
            large,
            "失敗で取り消しの段を増やさない"
        );
    }
}
