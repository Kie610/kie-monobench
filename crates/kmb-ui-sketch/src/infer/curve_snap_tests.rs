//! 確かめること: 曲線の式の上への吸い付き、置いた点の拘束、線・円との交点。
use super::*;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug)]
enum CurveKind {
    Ellipse,
    Spline,
    Conic,
}

fn fixture(kind: CurveKind) -> (Sketch, EntityId, [f64; 2], [f64; 2]) {
    let mut s = Sketch::default();
    let spot = |p| (None, p);
    let (id, at, tangent) = match kind {
        CurveKind::Ellipse => {
            let angle: f64 = 0.61;
            let id = s
                .add_ellipse(spot([20.0, 20.0]), spot([25.0, 20.0]), [20.0, 23.0])
                .unwrap();
            (
                id,
                [20.0 + 5.0 * angle.cos(), 20.0 + 3.0 * angle.sin()],
                [-5.0 * angle.sin(), 3.0 * angle.cos()],
            )
        }
        CurveKind::Spline | CurveKind::Conic => {
            let id = if matches!(kind, CurveKind::Spline) {
                s.add_spline(
                    &[spot([20.0, 20.0]), spot([25.0, 30.0]), spot([30.0, 20.0])],
                    true,
                    false,
                )
            } else {
                s.add_conic(
                    spot([20.0, 20.0]),
                    spot([30.0, 20.0]),
                    spot([25.0, 30.0]),
                    0.5,
                )
            }
            .unwrap();
            let t = 0.37;
            (
                id,
                [20.0 + 10.0 * t, 20.0 + 20.0 * t * (1.0 - t)],
                [10.0, 20.0 - 40.0 * t],
            )
        }
    };
    (s, id, at, tangent)
}

fn solved(s: &Sketch) -> Solution {
    kmb_sketch::solve(s, &HashMap::new()).unwrap()
}

fn snaps_and_constrains(kind: CurveKind) {
    let (s, curve, at, tangent) = fixture(kind);
    let norm = tangent[0].hypot(tangent[1]);
    let cursor = [
        at[0] - tangent[1] * 0.02 / norm,
        at[1] + tangent[0] * 0.02 / norm,
    ];
    let sol = solved(&s);
    let snapped = snap_cursor(&s, &sol, cursor, 0.08, &State::default());
    assert_eq!(snapped.on, Some(curve), "{kind:?}");
    assert!(
        (snapped.pos[0] - at[0]).hypot(snapped.pos[1] - at[1]) < 1e-7,
        "{kind:?}: {:?} != {at:?}",
        snapped.pos
    );
    assert!(snapped.label.is_some(), "既存の吸い付きの印と札を描く");
    let mut made = s.clone();
    let point = made.add_point(snapped.pos[0], snapped.pos[1]);
    attach_placed(&s, &mut made, &[(snapped.pos, curve)], None);
    assert!(made.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::PointOnCurve { point: p, curve: c } if p == point && c == curve)));
    let far = snap_cursor(
        &s,
        &sol,
        [cursor[0] + 100.0, cursor[1] + 100.0],
        0.08,
        &State::default(),
    );
    assert!(far.on.is_none());
}

#[test]
fn an_ellipse_snap_lies_on_the_curve_and_attaches_its_constraint() {
    snaps_and_constrains(CurveKind::Ellipse);
}

#[test]
fn a_spline_snap_lies_on_the_curve_and_attaches_its_constraint() {
    snaps_and_constrains(CurveKind::Spline);
}

#[test]
fn a_conic_snap_lies_on_the_curve_and_attaches_its_constraint() {
    snaps_and_constrains(CurveKind::Conic);
}

#[test]
fn free_curves_snap_to_intersections_with_finite_lines_and_circles() {
    for kind in [CurveKind::Ellipse, CurveKind::Spline, CurveKind::Conic] {
        for circle in [false, true] {
            let (mut s, curve, at, _) = fixture(kind);
            let other = if circle {
                let c = s.add_point(at[0] + 0.9, at[1]);
                s.add_circle(c, 0.9)
            } else {
                let a = s.add_point(at[0], at[1] - 0.8);
                let b = s.add_point(at[0], at[1] + 0.8);
                s.add_line(a, b)
            };
            let found = crossings(&s, &solved(&s));
            assert!(
                found.iter().any(|((p, k), on)| *k == Kind::Intersection
                    && on.contains(&curve)
                    && on.contains(&other)
                    && (p[0] - at[0]).hypot(p[1] - at[1]) < 1e-7),
                "{kind:?}, circle={circle}: {found:?}"
            );
        }
    }
}

#[test]
fn cached_crossings_follow_points_radii_and_the_curve_definition() {
    let (mut s, curve, at, _) = fixture(CurveKind::Conic);
    let center = s.add_point(at[0] + 0.9, at[1]);
    let circle = s.add_circle(center, 0.9);
    let sol = solved(&s);
    let mut cache = CrossingCache::default();
    let original = cache.get(&s, &sol).to_vec();
    let on_both = |found: &[Crossing]| {
        found
            .iter()
            .any(|(_, on)| on.contains(&curve) && on.contains(&circle))
    };
    assert!(on_both(&original));
    assert_eq!(
        cache.get(&s, &sol),
        &original,
        "カーソルだけの変化では同じ仮の点になる"
    );
    let mut moved = sol.clone();
    moved.points.get_mut(&center).unwrap()[0] += 100.0;
    assert!(!on_both(cache.get(&s, &moved)));
    let mut resized = sol.clone();
    resized.radii.insert(circle, 0.001);
    assert!(!on_both(cache.get(&s, &resized)));
    assert_eq!(cache.get(&s, &sol), &original);
    let changed = s.entities.iter_mut().find(|e| e.id() == curve).unwrap();
    let Entity::Conic { rho, .. } = changed else {
        panic!("円錐曲線")
    };
    *rho = 0.8;
    assert!(!on_both(cache.get(&s, &sol)));
}

#[test]
fn an_elliptic_arc_does_not_snap_to_its_missing_half() {
    let (mut s, curve, _, _) = fixture(CurveKind::Ellipse);
    let end = s.add_point(15.0, 20.0);
    let Entity::Ellipse { major, ends, .. } =
        s.entities.iter_mut().find(|e| e.id() == curve).unwrap()
    else {
        panic!("楕円")
    };
    *ends = Some((*major, end));
    let sol = solved(&s);
    let snapped = snap_cursor(&s, &sol, [20.0, 17.02], 0.08, &State::default());
    assert!(snapped.on.is_none());
}

#[test]
fn the_line_tool_draws_its_snap_mark_on_the_curve_and_keeps_the_constraint() {
    use egui_kittest::Harness;
    use kmb_samples::sample::DocBuilder;
    use kmb_ui::Workspace;
    let (s, curve, at, tangent) = fixture(CurveKind::Ellipse);
    let mut b = DocBuilder::new();
    let id = b.feature(
        kmb_sketch::SKETCH,
        "曲線の見本",
        serde_json::to_value(&s).unwrap(),
    );
    let mut ws = crate::tests::ws_of(b.finish("曲線の吸い付き"), None, crate::tests::reg());
    ws.open_edit(id);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    fn script(h: &mut Harness<'static, Workspace>, text: &str) {
        let batch = h.state_mut().run_script(text).unwrap();
        let start = std::time::Instant::now();
        loop {
            h.step();
            if let Some(result) = h.state_mut().script_result(batch) {
                assert!(result.ok, "{}", result.report());
                break;
            }
            assert!(start.elapsed().as_secs() < 15);
        }
        h.run();
    }
    script(&mut h, "hover \"スケッチの図\"\nkey L");
    let norm = tangent[0].hypot(tangent[1]);
    let cursor = [
        at[0] - tangent[1] * 0.02 / norm,
        at[1] + tangent[0] * 0.02 / norm,
    ];
    crate::tests::move_to(&mut h, cursor);
    h.run();
    let mark_at = crate::tests::screen(&h, at);
    fn has_mark(shape: &egui::Shape, at: egui::Pos2) -> bool {
        match shape {
            egui::Shape::Circle(circle) => {
                circle.center.distance(at) < 0.1
                    && (4.0..=6.0).contains(&circle.radius)
                    && circle.stroke.width > 0.0
            }
            egui::Shape::Vec(shapes) => shapes.iter().any(|shape| has_mark(shape, at)),
            _ => false,
        }
    }
    assert!(
        h.output()
            .shapes
            .iter()
            .any(|shape| has_mark(&shape.shape, mark_at)),
        "曲線の上に既存の吸い付きの丸を描く"
    );
    crate::tests::click_at(&mut h, cursor);
    crate::tests::click_at(&mut h, [at[0] + 7.0, at[1] + 4.0]);
    script(&mut h, "key Escape");
    let made = crate::tests::current_sketch(h.state());
    assert!(
        made.constraints.iter().any(
            |c| matches!(c.kind, ConstraintKind::PointOnCurve { curve: id, .. } if id == curve)
        )
    );
    assert_eq!(h.state().report().error_count(), 0);
}
