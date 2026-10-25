//! 楕円・スプライン・円錐曲線・ブレンド曲線の道具(網羅版 C10・C11・D8)を、人の操作で確かめる。

use crate::Tool;
use crate::tests::*;
use egui_kittest::Harness;
use jc_sketch::{ConstraintKind, Entity};
use jc_ui::Workspace;

fn stop(h: &mut Harness<'static, Workspace>) {
    press_key(h, egui::Key::Escape);
    press_key(h, egui::Key::Escape);
}

fn clicks(h: &mut Harness<'static, Workspace>, tool: Tool, pts: &[[f64; 2]]) {
    editor(h.state_mut()).set_tool(tool);
    h.run();
    for p in pts {
        click_at(h, *p);
    }
}

#[test]
fn ellipse_from_center_major_and_minor() {
    let mut h = empty_sketch();
    clicks(
        &mut h,
        Tool::Ellipse,
        &[[1.19, 11.15], [6.4, 11.15], [1.19, 13.75]],
    );
    stop(&mut h);
    let s = current_sketch(h.state());
    assert_eq!(count_of(&s, |e| matches!(e, Entity::Ellipse { .. })), 1);
    // 長軸・短軸の補助線も足す
    assert_eq!(
        count_of(&s, |e| matches!(
            e,
            Entity::Line {
                construction: true,
                ..
            }
        )),
        2
    );
    let sol = solved(h.state());
    let (c, a, b) = s
        .entities
        .iter()
        .find_map(|e| match e {
            Entity::Ellipse {
                center,
                major,
                minor,
                ..
            } => Some((sol.points[center], sol.points[major], sol.points[minor])),
            _ => None,
        })
        .unwrap();
    let len = |p: [f64; 2]| (p[0] - c[0]).hypot(p[1] - c[1]);
    assert!(
        (len(a) / len(b) - 2.0).abs() < 0.05,
        "長半径は短半径の 2 倍"
    );
}

#[test]
fn fit_spline_ends_with_enter_and_closes_on_the_first_point() {
    let mut h = empty_sketch();
    clicks(
        &mut h,
        Tool::FitSpline,
        &[[-4.03, 11.15], [0.15, 14.28], [4.32, 10.1]],
    );
    press_enter(&mut h);
    let s = current_sketch(h.state());
    let open = s.entities.iter().find_map(|e| match e {
        Entity::Spline {
            points,
            control,
            closed,
            ..
        } => Some((points.len(), *control, *closed)),
        _ => None,
    });
    assert_eq!(open, Some((3, false, false)));
    // 道具は続く。次は 4 点目で最初の点を押して閉じる
    for p in [[-4.03, 3.33], [0.15, 5.93], [4.32, 2.81], [-4.03, 3.33]] {
        click_at(&mut h, p);
    }
    stop(&mut h);
    let s = current_sketch(h.state());
    let closed: Vec<(usize, bool)> = s
        .entities
        .iter()
        .filter_map(|e| match e {
            Entity::Spline { points, closed, .. } => Some((points.len(), *closed)),
            _ => None,
        })
        .collect();
    assert_eq!(closed, vec![(3, false), (3, true)]);
}

#[test]
fn conic_uses_the_typed_rho() {
    let mut h = empty_sketch();
    editor(h.state_mut()).set_tool(Tool::Conic);
    h.run();
    click_at(&mut h, [-4.03, 8.54]);
    type_text(&mut h, "0.3");
    click_at(&mut h, [6.4, 8.54]);
    click_at(&mut h, [1.19, 14.8]);
    stop(&mut h);
    let s = current_sketch(h.state());
    let rho = s.entities.iter().find_map(|e| match e {
        Entity::Conic { rho, .. } => Some(*rho),
        _ => None,
    });
    assert_eq!(rho, Some(0.3));
}

#[test]
fn blend_joins_two_line_ends_with_curvature_or_tangency() {
    for g1 in [false, true] {
        let mut h = empty_sketch();
        clicks(&mut h, Tool::Line, &[[-6.63, 8.54], [-1.42, 8.54]]);
        stop(&mut h);
        clicks(&mut h, Tool::Line, &[[3.8, 13.75], [9.01, 13.75]]);
        stop(&mut h);
        editor(h.state_mut()).set_tool(Tool::Blend);
        editor(h.state_mut()).editor.blend_g1 = g1;
        h.run();
        click_at(&mut h, [-1.42, 8.54]);
        click_at(&mut h, [3.8, 13.75]);
        stop(&mut h);
        let s = current_sketch(h.state());
        assert_eq!(count_of(&s, |e| matches!(e, Entity::Spline { .. })), 1);
        let curv = s
            .constraints
            .iter()
            .filter(|c| matches!(c.kind, ConstraintKind::Curvature { .. }))
            .count();
        let tang = s
            .constraints
            .iter()
            .filter(|c| matches!(c.kind, ConstraintKind::TangentCurve { .. }))
            .count();
        if g1 {
            assert_eq!((curv, tang), (0, 2));
        } else {
            assert_eq!((curv, tang), (2, 0));
        }
    }
}
