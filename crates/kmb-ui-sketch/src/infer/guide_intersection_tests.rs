//! 確かめること: 補助線の交点の優先、中心・中点の記憶、既定の並びと Ctrl の保持。

use super::*;
use crate::view::{Editor, Tool};

fn solution(sketch: &Sketch) -> Solution {
    kmb_sketch::solve(sketch, &Default::default()).unwrap()
}

fn editor(on: bool) -> Editor {
    Editor {
        guide_intersections: on,
        tool: Tool::Line,
        ..Editor::default()
    }
}

fn pass(editor: &mut Editor, sketch: &Sketch, sol: &Solution, at: [f64; 2]) -> Snapped {
    steer(editor, sketch, sol, Some(at), 0.5, true).1.unwrap()
}

#[test]
fn enabled_recent_guide_intersection_wins_over_nearly_horizontal_line_inference() {
    let mut sketch = Sketch::default();
    sketch.add_point(10.0, 3.0);
    sketch.add_point(30.0, 20.0);
    let sol = solution(&sketch);
    let mut editor = editor(true);
    pass(&mut editor, &sketch, &sol, [10.0, 3.0]);
    pass(&mut editor, &sketch, &sol, [30.0, 20.0]);
    editor.pending = Some(crate::view::Pick {
        point: None,
        pos: [0.0, 20.3],
    });
    let (point, snapped, direction) =
        steer(&mut editor, &sketch, &sol, Some([10.2, 20.2]), 0.5, true);
    assert_eq!(point, Some([10.0, 20.0]));
    assert_eq!(snapped.unwrap().guides.len(), 2);
    assert_eq!(direction, None, "交点を角度の案内で上書きしない");
}

#[test]
fn disabled_setting_keeps_existing_point_alignment_and_line_inference() {
    let mut sketch = Sketch::default();
    sketch.add_point(10.0, 3.0);
    sketch.add_point(30.0, 20.0);
    let sol = solution(&sketch);
    let mut editor = editor(false);
    pass(&mut editor, &sketch, &sol, [10.0, 3.0]);
    pass(&mut editor, &sketch, &sol, [30.0, 20.0]);
    let aligned = pass(&mut editor, &sketch, &sol, [10.2, 20.2]);
    assert_eq!(aligned.pos, [10.0, 20.0]);
    assert_eq!(aligned.guides.len(), 2, "既存の 2 軸の寄せは設定切でも保つ");
    editor.pending = Some(crate::view::Pick {
        point: None,
        pos: [0.0, 20.3],
    });
    let (_, snapped, direction) = steer(&mut editor, &sketch, &sol, Some([10.2, 20.2]), 0.5, true);
    assert_eq!(direction, Some(LineRel::Horizontal));
    assert_eq!(snapped.unwrap().pos[1], 20.3);
}

#[test]
fn recent_pair_wins_over_a_closer_older_alignment() {
    let mut sketch = Sketch::default();
    for point in [[10.2, -20.0], [10.0, 3.0], [30.0, 20.0]] {
        sketch.add_point(point[0], point[1]);
    }
    let sol = solution(&sketch);
    let mut editor = editor(true);
    for point in [[10.2, -20.0], [10.0, 3.0], [30.0, 20.0]] {
        pass(&mut editor, &sketch, &sol, point);
    }
    let found = pass(&mut editor, &sketch, &sol, [10.2, 20.2]);
    assert_eq!(found.pos, [10.0, 20.0]);
    assert_eq!(
        found.guides,
        vec![([10.0, 3.0], found.pos), ([30.0, 20.0], found.pos)]
    );
}

#[test]
fn circle_and_arc_centers_are_already_tracked_points() {
    let mut sketch = Sketch::default();
    let circle_center = sketch.add_point(10.0, 3.0);
    sketch.add_circle(circle_center, 2.0);
    let arc_center = sketch.add_point(30.0, 20.0);
    let start = sketch.add_point(32.0, 20.0);
    let end = sketch.add_point(30.0, 22.0);
    sketch.add_arc(arc_center, start, end);
    let sol = solution(&sketch);
    let mut state = State::default();
    state.notice(&sketch, &sol, [10.0, 3.0], 0.5);
    state.notice(&sketch, &sol, [30.0, 20.0], 0.5);
    assert_eq!(state.tracked, vec![circle_center, arc_center]);
    let found = snap_cursor(&sketch, &sol, [10.2, 20.2], 0.5, &state);
    assert_eq!(found.pos, [10.0, 20.0]);
    assert_eq!(found.guides.len(), 2);
}

#[test]
fn augmented_midpoints_emit_both_guides_and_survive_virtual_id_changes() {
    let mut sketch = Sketch::default();
    let a = sketch.add_point(8.0, 3.0);
    let b = sketch.add_point(12.0, 3.0);
    sketch.add_line(a, b);
    let c = sketch.add_point(28.0, 20.0);
    let d = sketch.add_point(32.0, 20.0);
    sketch.add_line(c, d);
    let augment = |sketch: &Sketch| {
        crate::snap::augment(
            sketch,
            &solution(sketch),
            &[],
            crate::view::snap_wants(Tool::Line),
        )
    };
    let mut editor = editor(true);
    let first = augment(&sketch);
    pass(&mut editor, &first.sketch, &first.sol, [10.0, 3.0]);
    pass(&mut editor, &first.sketch, &first.sol, [30.0, 20.0]);
    let existing = snap_cursor(&first.sketch, &first.sol, [10.2, 20.2], 0.5, &editor.infer);
    assert_eq!(existing.pos, [10.0, 20.0]);
    assert_eq!(
        existing.guides.len(),
        2,
        "仮点にした中点の並び自体は既存の機能"
    );
    // 実点が増えると仮点の番号は変わる。元の線分から中点を解き直す。
    sketch.add_point(100.0, 100.0);
    let second = augment(&sketch);
    editor.pending = Some(crate::view::Pick {
        point: None,
        pos: [0.0, 20.3],
    });
    let found = pass(&mut editor, &second.sketch, &second.sol, [10.2, 20.2]);
    assert_eq!(found.pos, [10.0, 20.0]);
    assert_eq!(found.guides.len(), 2);
    assert_eq!(found.guides[0].0, [10.0, 3.0]);
    assert_eq!(found.guides[1].0, [30.0, 20.0]);
}

#[test]
fn ctrl_bypasses_recent_intersection_and_does_not_record_a_new_point() {
    let mut sketch = Sketch::default();
    sketch.add_point(10.0, 3.0);
    sketch.add_point(30.0, 20.0);
    let sol = solution(&sketch);
    let mut editor = editor(true);
    pass(&mut editor, &sketch, &sol, [10.0, 3.0]);
    pass(&mut editor, &sketch, &sol, [30.0, 20.0]);
    let tracked = editor.infer.tracked.clone();
    let (pos, snap, direction) = steer(&mut editor, &sketch, &sol, Some([10.2, 20.2]), 0.5, false);
    assert_eq!(pos, Some([10.2, 20.2]));
    assert!(snap.is_none());
    assert!(direction.is_none());
    assert_eq!(editor.infer.tracked, tracked);
}

#[test]
fn projected_centers_keep_their_source_when_virtual_ids_are_reused() {
    use kmb_sketch::{ProjectionSource, RefItem, RefShape};
    let refs = [
        RefItem {
            source: ProjectionSource::Center {
                feature: kmb_core::FeatureId(41),
                faces: ["round-a".into(), "top-a".into()],
            },
            shape: RefShape::Point([10.0, 3.0]),
        },
        RefItem {
            source: ProjectionSource::Center {
                feature: kmb_core::FeatureId(42),
                faces: ["round-b".into(), "top-b".into()],
            },
            shape: RefShape::Point([30.0, 20.0]),
        },
    ];
    let mut sketch = Sketch::default();
    let augment = |sketch: &Sketch, refs: &[RefItem]| {
        crate::snap::augment(
            sketch,
            &solution(sketch),
            refs,
            crate::view::snap_wants(Tool::Line),
        )
    };
    let first = augment(&sketch, &refs);
    let mut editor = editor(true);
    pass(&mut editor, &first.sketch, &first.sol, [10.0, 3.0]);
    pass(&mut editor, &first.sketch, &first.sol, [30.0, 20.0]);
    let old_ids = editor.infer.tracked.clone();
    let a = sketch.add_point(100.0, 100.0);
    let b = sketch.add_point(200.0, 200.0);
    assert_eq!(old_ids, vec![a, b], "仮番号が実点へ再利用される条件を作る");
    // 投影の並びも入れ替わる。番号や列の順でなく source で探す。
    let second = augment(&sketch, &[refs[1].clone(), refs[0].clone()]);
    editor.pending = Some(crate::view::Pick {
        point: None,
        pos: [0.0, 20.3],
    });
    let found = pass(&mut editor, &second.sketch, &second.sol, [10.2, 20.2]);
    assert_eq!(found.pos, [10.0, 20.0]);
    assert_eq!(
        found.guides,
        vec![([10.0, 3.0], found.pos), ([30.0, 20.0], found.pos)]
    );
    editor.pending = None;
    let missing = augment(&sketch, &[refs[1].clone()]);
    let found = pass(&mut editor, &missing.sketch, &missing.sol, [100.2, 20.2]);
    assert_eq!(
        found.pos,
        [100.2, 20.0],
        "消えた参照の仮番号を実点として使わない"
    );
    assert_eq!(found.guides, vec![([30.0, 20.0], found.pos)]);
}
