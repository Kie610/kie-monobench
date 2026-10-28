//! 確かめること: 名前で道具を押し、複数の等しいと直径の吸い付きを画面で使える。

use crate::tests::*;
use egui_kittest::Harness;
use kmb_core::{Document, FeatureEntry, Op, Transaction};
use kmb_sketch::{ConstraintKind as K, Entity, Sketch};
use kmb_ui::Workspace;

fn fixture(sketch: Sketch) -> Harness<'static, Workspace> {
    let mut doc = Document::new();
    let id = doc.alloc_feature_id();
    doc.apply(Transaction::new(
        "スケッチの見本",
        vec![Op::InsertFeature {
            index: None,
            entry: FeatureEntry {
                id,
                type_id: kmb_sketch::SKETCH.into(),
                name: "拘束の見本".into(),
                data: serde_json::to_value(sketch).unwrap(),
                suppressed: false,
                pin_bodies: false,
            },
        }],
    ))
    .unwrap();
    let mut ws = ws_of(doc, None, reg());
    ws.open_edit(id);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, ws: &mut Workspace| ws.ui(ui), ws);
    h.run();
    h
}

fn script(h: &mut Harness<'static, Workspace>, text: &str) {
    let batch = h.state_mut().run_script(text).unwrap();
    let began = std::time::Instant::now();
    loop {
        h.step();
        if let Some(result) = h.state_mut().script_result(batch) {
            assert!(result.ok, "{}", result.report());
            break;
        }
        assert!(began.elapsed().as_secs() < 15, "台本が終わらない");
    }
}

#[test]
fn a_named_equal_button_constrains_three_lines_in_click_order() {
    let mut sketch = Sketch::default();
    let mut ids = Vec::new();
    for (length, y) in [(10.0, 0.0), (15.0, 10.0), (20.0, 20.0)] {
        let a = sketch.add_point(0.0, y);
        let b = sketch.add_point(length, y);
        ids.push(sketch.add_line(a, b));
    }
    sketch.add_constraint(K::Length {
        line: ids[2],
        value: "20 mm".into(),
    });
    let mut h = fixture(sketch);
    for p in [[10.0, 20.0], [5.0, 0.0], [7.5, 10.0]] {
        crate::menu::tests::click_with(&mut h, p, egui::Modifiers::SHIFT);
    }
    assert_eq!(editor(h.state_mut()).editor.selection.len(), 3);
    script(&mut h, include_str!("../tests/ui/multi-equal.jcs"));
    let out = current_sketch(h.state());
    let pairs: Vec<_> = out
        .constraints
        .iter()
        .filter_map(|c| match c.kind {
            K::Equal { a, b } => Some((a, b)),
            _ => None,
        })
        .collect();
    assert_eq!(pairs, [(ids[2], ids[0]), (ids[2], ids[1])]);
    let sol = solved(h.state());
    for id in ids {
        let (a, b) = crate::annot::line_ends(&out, &sol, id).unwrap();
        assert!(((a[0] - b[0]).hypot(a[1] - b[1]) - 20.0).abs() < 1e-6);
    }
    assert!(!matches!(
        sol.status,
        kmb_sketch::SolveStatus::OverConstrained { .. }
            | kmb_sketch::SolveStatus::Inconsistent { .. }
    ));
}

#[test]
fn a_named_line_tool_snaps_only_the_clicked_opposite_point() {
    let mut sketch = Sketch::default();
    let center = sketch.add_point(0.0, 0.0);
    let circle = sketch.add_circle(center, 5.0);
    let start = sketch.add_point(3.0, 4.0);
    sketch.add_constraint(K::Fix { point: center });
    sketch.add_constraint(K::Radius {
        circle,
        value: "5 mm".into(),
    });
    sketch.add_constraint(K::PointOnCircle {
        point: start,
        circle,
    });
    let old = sketch.constraints.clone();
    let mut h = fixture(sketch);
    script(&mut h, include_str!("../tests/ui/diameter-line.jcs"));
    quick_click(&mut h, [3.0, 4.0]);
    move_to(&mut h, [-3.02, -4.03]);
    h.run();
    quick_click(&mut h, [-3.02, -4.03]);
    press_key(&mut h, egui::Key::Escape);
    let out = current_sketch(h.state());
    assert_eq!(out.constraints.len(), old.len() + 1);
    for c in &old {
        assert!(out.constraints.contains(c));
    }
    let (line, a, b) = out
        .entities
        .iter()
        .find_map(|e| match e {
            Entity::Line { id, a, b, .. } => Some((*id, *a, *b)),
            _ => None,
        })
        .expect("クリックで線を作った");
    assert!(
        out.constraints.iter().any(
            |c| matches!(c.kind, K::Midpoint { point, line:l } if point == center && l == line)
        )
    );
    let sol = solved(h.state());
    assert_eq!(
        sol.status,
        kmb_sketch::SolveStatus::UnderConstrained { dof: 1 }
    );
    for axis in 0..2 {
        assert!(
            ((sol.points[&a][axis] + sol.points[&b][axis]) / 2.0 - sol.points[&center][axis]).abs()
                < 1e-6
        );
    }
}
