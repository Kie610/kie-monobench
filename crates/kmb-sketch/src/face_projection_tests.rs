//! 確かめること: 丸めた角を含む面の写しが閉じ、大きなすき間は閉じない。

use super::*;
use kmb_core::{
    Document, FacePick, FeatureEntry, FeatureId, FeatureStatus, Op, Plane, Transaction,
};
use kmb_session::{Handled, Headless, Picks};
use serde_json::json;

fn rounded_rectangle(scale: f64) -> Sketch {
    let mut sketch = Sketch::default();
    let corners = [
        ([36.0, 4.0], [36.0, 0.0], [40.0, 4.0]),
        ([36.0, 26.0], [40.0, 26.0], [36.0, 30.0]),
        ([4.0, 26.0], [4.0, 30.0], [0.0, 26.0]),
        ([4.0, 4.0], [0.0, 4.0], [4.0, 0.0]),
    ];
    let points: Vec<_> = corners
        .into_iter()
        .map(|(c, a, b)| {
            let mut point = |p: [f64; 2]| sketch.add_point(p[0] * scale, p[1] * scale);
            (point(c), point(a), point(b))
        })
        .collect();
    for (i, &(center, start, end)) in points.iter().enumerate() {
        sketch.add_line(points[(i + 3) % 4].2, start);
        sketch.add_arc(center, start, end);
    }
    sketch
}

fn insert(doc: &mut Document, type_id: &str, data: serde_json::Value) -> FeatureId {
    let id = doc.alloc_feature_id();
    doc.apply(Transaction::new(
        "試験の形",
        vec![Op::InsertFeature {
            index: None,
            entry: FeatureEntry {
                id,
                type_id: type_id.into(),
                name: format!("形{}", id.0),
                data,
                suppressed: false,
                pin_bodies: false,
            },
        }],
    ))
    .unwrap();
    id
}

fn sketch_on_cap(work: &mut Headless, body: FeatureId, cap: &str, expected_area: f64) {
    assert_eq!(work.report().error_count(), 0, "{:?}", work.report());
    let pick = FacePick {
        feature: body,
        face: format!("f{}:{cap}", body.0),
    };
    let Handled::Changed(added) = work
        .run_with(
            SKETCH_ON_FACE,
            &Picks {
                selected: Some(body),
                picked: Some(&pick),
                ..Default::default()
            },
            None,
            &json!({}),
        )
        .unwrap()
    else {
        panic!("面にスケッチを置く操作が文書を変えませんでした");
    };
    assert_eq!(added.len(), 1);
    assert_eq!(work.report().error_count(), 0, "{:?}", work.report());
    let Some(FeatureStatus::Ok { outputs, .. }) = work.report().feature(added[0]) else {
        panic!("面のスケッチの計算が通りませんでした");
    };
    let profile = profile_value(&outputs["profile"]).unwrap();
    assert_eq!(profile.regions.len(), 1, "{profile:?}");
    assert!(profile.regions[0].holes.is_empty());
    let area = profile.regions[0].outer.signed_area();
    assert!(
        (area - expected_area).abs() < expected_area * 0.002,
        "{area}"
    );
    let sketch: Sketch =
        serde_json::from_value(work.doc().data().feature(added[0]).unwrap().data.clone()).unwrap();
    assert!(!sketch.projections.is_empty());
    let solution = solve(&sketch, &Default::default()).unwrap();
    assert!(open_ends_of(&sketch, &solution).is_empty());
}

#[test]
fn a_plain_box_cap_still_projects_to_one_closed_region() {
    let mut sketch = Sketch::default();
    let points: Vec<_> = [[0.0, 0.0], [40.0, 0.0], [40.0, 30.0], [0.0, 30.0]]
        .into_iter()
        .map(|p| sketch.add_point(p[0], p[1]))
        .collect();
    for i in 0..4 {
        sketch.add_line(points[i], points[(i + 1) % 4]);
    }
    let mut doc = Document::new();
    let profile = insert(&mut doc, SKETCH, serde_json::to_value(sketch).unwrap());
    let body = insert(
        &mut doc,
        "solid.extrude@1",
        json!({"profile": profile, "distance": "20 mm"}),
    );
    let mut work = Headless::with_document(kmb_bundle::modules(), doc, None);
    sketch_on_cap(&mut work, body, "end", 1200.0);
}

fn projected_with_gap(scale: f64, gap: f64) -> Sketch {
    use kmb_core::Segment;
    let source = rounded_rectangle(scale);
    let solution = solve(&source, &Default::default()).unwrap();
    let (profile, unused) = profile_of(&source, &solution, Plane::XY);
    assert!(unused.is_empty());
    assert_eq!(profile.regions.len(), 1);
    let mut sketch = Sketch::default();
    let mut shifted = false;
    for (i, edge) in profile.regions[0].outer.edges.iter().enumerate() {
        let shape = match edge.segment.clone() {
            Segment::Line { mut a, b } => {
                if !shifted {
                    a[0] += gap;
                    shifted = true;
                }
                RefShape::Line(a, b)
            }
            Segment::Arc {
                center,
                radius,
                start,
                end,
            } => {
                let at = |a: f64| [center[0] + radius * a.cos(), center[1] + radius * a.sin()];
                RefShape::Circle {
                    center,
                    radius,
                    arc: Some((at(start), at(end))),
                }
            }
            Segment::Circle { .. } => unreachable!(),
        };
        sketch.add_projection_as(
            &RefItem {
                source: ProjectionSource::Edge {
                    feature: FeatureId(999),
                    faces: ["cap".into(), format!("side{i}")],
                },
                shape,
            },
            false,
        );
    }
    assert!(shifted);
    sketch
}

#[test]
fn projected_rounded_edges_close_for_tiny_gaps() {
    for scale in [0.1, 1.0, 10.0] {
        let sketch = projected_with_gap(scale, scale * 1e-8);
        let solution = solve(&sketch, &Default::default()).unwrap();
        let (profile, unused) = profile_of(&sketch, &solution, Plane::XY);
        assert_eq!(profile.regions.len(), 1, "倍率 {scale}");
        assert!(unused.is_empty());
        assert!(open_ends_of(&sketch, &solution).is_empty());
    }
}

#[test]
fn projected_rounded_edges_keep_large_gaps_open() {
    for scale in [0.1, 1.0, 10.0] {
        let sketch = projected_with_gap(scale, scale * 1e-2);
        let solution = solve(&sketch, &Default::default()).unwrap();
        let (profile, unused) = profile_of(&sketch, &solution, Plane::XY);
        assert!(profile.regions.is_empty(), "倍率 {scale}");
        assert!(!unused.is_empty());
        assert!(!open_ends_of(&sketch, &solution).is_empty());
    }
}
