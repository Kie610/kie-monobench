//! 確かめること: 宣言された操作、形への追従、切れた参照、字形の保存と部品表の対応。
use super::*;
use kmb_core::Outcome;
use kmb_interchange::drawing::{EdgeKey, LineKey};
use serde_json::json;

fn action(doc: &mut Document, id: &str, args: serde_json::Value) {
    let m = modules();
    let r = doc.recompute(&m.registry, m.kernel.as_ref());
    assert_eq!(r.error_count(), 0, "{r:?}");
    let mut cx = Context::new(doc.data(), &r, &m.registry, &m.kernel);
    cx.args = &args;
    let out = m
        .extensions
        .run(id, &cx)
        .unwrap_or_else(|e| panic!("{id}: {e}"));
    let (label, edits) = match out {
        Outcome::Change { label, edits } => (label, edits),
        Outcome::Text { title, body } => panic!("{id}: {title}: {body}"),
        _ => panic!("{id} の変更を期待"),
    };
    doc.apply_edits(&label, edits).unwrap();
}
fn width_reference(doc: &mut Document) -> EdgeKey {
    let m = modules();
    let r = doc.recompute(&m.registry, m.kernel.as_ref());
    assert_eq!(r.error_count(), 0, "{r:?}");
    let (_, views) = sheet_picture_with_lines(
        &Context::new(doc.data(), &r, &m.registry, &m.kernel),
        "sheet",
    )
    .unwrap();
    views["front"]
        .iter()
        .find_map(|l| match (&l.key, &l.path) {
            (LineKey::Edge { key }, Path::Line { a, b })
                if l.visible && (a[1] - b[1]).abs() < 1e-7 && (a[0] - b[0]).abs() > 30.0 =>
            {
                Some(key.clone())
            }
            _ => None,
        })
        .expect("箱の名前付きの横の辺")
}
fn dimension(doc: &mut Document) -> EdgeKey {
    let key = width_reference(doc);
    action(
        doc,
        "drawing.dimension",
        json!({"sheet":"sheet","id":"dim","dimension":"horizontal","refs":[{"view":"front","key":key,"at":"a"},{"view":"front","key":key,"at":"b"}],"at":[70,90]}),
    );
    key
}
fn span(p: &SheetPicture, owner: &str) -> f64 {
    strokes(p, owner)
        .into_iter()
        .filter_map(|p| match p {
            Path::Line { a, b } if (a[1] - b[1]).abs() < 1e-7 => Some((a[0] - b[0]).abs()),
            _ => None,
        })
        .fold(0.0, f64::max)
}
#[test]
fn drawing_dimensions_follow_model_edits_and_undo_through_declared_actions() {
    let mut doc = crate::drawing_box_document();
    edit(&mut doc, |d| d.sheets[0].views.truncate(1));
    dimension(&mut doc);
    let before = picture(&mut doc);
    assert!((span(&before, "dim") - 40.0).abs() < 1e-7);
    assert!(
        before.problems.iter().all(|p| p.warning),
        "{:?}",
        before.problems
    );
    let width = doc
        .data()
        .params
        .iter()
        .find(|p| p.name == "width")
        .unwrap()
        .id;
    doc.apply(Transaction::new(
        "幅",
        vec![Op::SetParamExpr {
            id: width,
            expr: "60 mm".into(),
        }],
    ))
    .unwrap();
    let changed = picture(&mut doc);
    assert!((span(&changed, "dim") - 60.0).abs() < 1e-7);
    assert!(
        changed.problems.iter().all(|p| p.warning),
        "{:?}",
        changed.problems
    );
    doc.undo();
    assert!((span(&picture(&mut doc), "dim") - 40.0).abs() < 1e-7);
    action(
        &mut doc,
        "drawing.delete",
        json!({"sheet":"sheet","id":"dim"}),
    );
    assert!(
        doc.data().section::<Drawings>().unwrap().sheets[0]
            .notes
            .is_empty()
    );
    doc.undo();
    let restored = picture(&mut doc);
    assert!(
        !strokes(&restored, "dim").is_empty(),
        "notes={:?}; problems={:?}",
        doc.data().section::<Drawings>().unwrap().sheets[0].notes,
        restored.problems
    );
    let saved = doc.to_json();
    let mut reopened = Document::from_json(&saved).unwrap();
    assert_eq!(
        serde_json::to_value(picture(&mut doc)).unwrap(),
        serde_json::to_value(picture(&mut reopened)).unwrap()
    );
}
#[test]
fn removing_a_dimensioned_edge_reports_only_that_annotation() {
    let mut doc = crate::drawing_box_document();
    edit(&mut doc, |d| d.sheets[0].views.truncate(1));
    let key = dimension(&mut doc);
    action(
        &mut doc,
        "drawing.note",
        json!({"sheet":"sheet","id":"plain","text":"OK","at":[120,120]}),
    );
    action(
        &mut doc,
        kmb_cad_edge::CHAMFER_FACE,
        json!({"edges":[{"feature":key.body,"faces":key.faces}],"distance":0.5}),
    );
    let changed = picture(&mut doc);
    assert!(
        changed.problems.iter().any(|p| p.target == "dim"
            && !p.warning
            && p.problem.message.contains("辺がありません")),
        "{:?}",
        changed.problems
    );
    assert!(strokes(&changed, "dim").is_empty());
    assert!(!strokes(&changed, "front").is_empty());
    assert!(!strokes(&changed, "plain").is_empty());
    doc.undo();
    assert!(!strokes(&picture(&mut doc), "dim").is_empty());
}
#[test]
fn circular_dimensions_and_stored_japanese_glyphs_reopen_together() {
    let mut doc = crate::drawing_cylinder_document();
    let m = modules();
    let r = doc.recompute(&m.registry, m.kernel.as_ref());
    assert_eq!(r.error_count(), 0, "{r:?}");
    let (_, views) = sheet_picture_with_lines(
        &Context::new(doc.data(), &r, &m.registry, &m.kernel),
        "sheet",
    )
    .unwrap();
    let (key, c, radius) = views["top"]
        .iter()
        .find_map(|l| match (&l.key, &l.path) {
            (LineKey::Edge { key }, Path::Arc { c, r, .. }) if l.visible => {
                Some((key.clone(), *c, *r))
            }
            _ => None,
        })
        .expect("円柱の上の円");
    let reference = json!({"view":"top","key":key,"at":"center"});
    action(
        &mut doc,
        "drawing.dimension",
        json!({"sheet":"sheet","id":"radius","dimension":"radius","refs":[reference],"at":[c[0]+radius+10.0,c[1]]}),
    );
    action(
        &mut doc,
        "drawing.center_mark",
        json!({"sheet":"sheet","id":"center","reference":reference}),
    );
    let bytes = include_bytes!("../../../kmb-text/tests/fonts/kmb-test-jp.ttf");
    let mut font = kmb_text::OutlineFont::open(bytes, 0).unwrap();
    assert!(font.add_glyphs(bytes, 0, "自作回8").unwrap().is_empty());
    edit(&mut doc, |d| {
        d.sheets[0]
            .notes
            .push(json!({"id":"jp","kind":"note","text":"自作回8","at":[200,180],"outline":font}))
    });
    let p = picture(&mut doc);
    assert!((span(&p, "radius") - radius).abs() < 1e-7);
    assert!(
        p.items
            .iter()
            .any(|i| matches!(i,Item::Fill {owner,..} if owner=="jp"))
    );
    assert!(
        p.items
            .iter()
            .any(|i| matches!(i,Item::Stroke {owner,line:LineStyle::Chain,..} if owner=="center"))
    );
    let mut reopened = Document::from_json(&doc.to_json()).unwrap();
    assert_eq!(
        serde_json::to_value(&p).unwrap(),
        serde_json::to_value(picture(&mut reopened)).unwrap()
    );
}

#[test]
fn balloons_share_bom_rows_and_deleted_parts_fail_without_retargeting() {
    use kmb_asm::inspect::{BomSource, bom_rows, bom_rows_with_sources, bom_table};
    let mut doc = crate::drawing_assembly_document();
    edit(&mut doc, |d| d.sheets[0].views.truncate(1));
    let roots = doc.data().components[0].bodies.clone();
    let ids = [doc.alloc_feature_id().0, doc.alloc_feature_id().0];
    let components = roots
        .iter()
        .zip(ids)
        .enumerate()
        .map(|(i, (root, id))| kmb_core::Component {
            id,
            name: format!("part{}", i + 1),
            parent: None,
            bodies: vec![*root],
        })
        .collect::<Vec<_>>();
    doc.apply(Transaction::new(
        "部品",
        vec![Op::SetComponents {
            components: components.clone(),
        }],
    ))
    .unwrap();
    let m = modules();
    let r = doc.recompute(&m.registry, m.kernel.as_ref());
    let cx = Context::new(doc.data(), &r, &m.registry, &m.kernel);
    let (rows, sources) = bom_rows_with_sources(&cx);
    assert_eq!(
        serde_json::to_value(bom_table(&rows)).unwrap(),
        serde_json::to_value(bom_table(&bom_rows(&cx))).unwrap()
    );
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].count, 2);
    for id in ids {
        assert_eq!(sources[&BomSource::Component { id }], 0);
    }
    for root in &roots {
        assert_eq!(sources[&BomSource::Body { root: *root }], 0);
    }
    for (i, id) in ids.into_iter().enumerate() {
        action(
            &mut doc,
            "drawing.balloon",
            json!({"sheet":"sheet","id":format!("b{i}"),"view":"front","part":{"kind":"component","id":id},"tip":[70.0+i as f64*55.0,65.0],"at":[70.0+i as f64*55.0,95.0+i as f64*5.0]}),
        );
    }
    action(
        &mut doc,
        "drawing.align_balloons",
        json!({"sheet":"sheet","ids":["b0","b1"],"axis":"y"}),
    );
    let drawings: Drawings = doc.data().section().unwrap();
    for note in &drawings.sheets[0].notes {
        assert_eq!(note["number"], 1);
        assert_eq!(note["at"][1], 95.0);
    }
    // 表の字形は試験内で自作し、組立の行と描画の対応をOSの書体から独立させる。
    let font = json!({"family":"Test","units_per_em":1000.0,"cap_height":700.0,"ascent":800.0,"descent":-200.0,"glyphs":{"番":{"advance":800.0,"path":"M 0 0 L 600 0 L 600 700 L 0 700 Z"},"号":{"advance":800.0,"path":"M 0 0 L 600 0 L 600 700 L 0 700 Z"}}});
    edit(&mut doc, |d| {
        d.sheets[0]
            .notes
            .push(json!({"id":"table","kind":"parts_table","columns":["item"],"outline":font}))
    });
    let p = picture(&mut doc);
    assert!(p.problems.iter().all(|p| p.warning), "{:?}", p.problems);
    assert!(!strokes(&p, "b0").is_empty() && !strokes(&p, "b1").is_empty());
    assert!(
        p.items
            .iter()
            .any(|i| matches!(i,Item::Fill {owner,..} if owner=="table"))
    );
    let table = strokes(&p, "table");
    let min_y = table
        .iter()
        .filter_map(|p| match p {
            Path::Line { a, b } => Some(a[1].min(b[1])),
            _ => None,
        })
        .fold(f64::INFINITY, f64::min);
    assert!((min_y - 10.0).abs() < 1e-7);
    doc.apply(Transaction::new(
        "部品を消す",
        vec![
            Op::SetComponents {
                components: components[1..].to_vec(),
            },
            Op::RemoveFeature { id: roots[0] },
        ],
    ))
    .unwrap();
    let deleted = picture(&mut doc);
    assert!(
        deleted.problems.iter().any(|p| p.target == "b0"
            && !p.warning
            && p.problem.message.contains("部品がありません")),
        "{:?}",
        deleted.problems
    );
    assert!(strokes(&deleted, "b0").is_empty());
    assert!(!strokes(&deleted, "b1").is_empty());
    doc.undo();
    assert!(!strokes(&picture(&mut doc), "b0").is_empty());
    edit(&mut doc, |d| d.sheets[0].notes[0]["number"] = json!(9));
    assert!(
        picture(&mut doc)
            .problems
            .iter()
            .any(|p| p.target == "b0" && p.problem.message.contains("振り直"))
    );
    action(
        &mut doc,
        "drawing.renumber_balloons",
        json!({"sheet":"sheet"}),
    );
    assert!(picture(&mut doc).problems.iter().all(|p| p.warning));
}

#[test]
fn model_dimension_import_uses_sketch_labels_and_named_visible_geometry_once() {
    let mut doc = crate::drawing_box_document();
    edit(&mut doc, |d| d.sheets[0].views.truncate(2));
    let sketch_id = doc
        .data()
        .features
        .iter()
        .find(|f| f.type_id == kmb_sketch::SKETCH)
        .unwrap()
        .id;
    let mut sketch: kmb_sketch::Sketch =
        serde_json::from_value(doc.data().feature(sketch_id).unwrap().data.clone()).unwrap();
    let m = modules();
    let r = doc.recompute(&m.registry, m.kernel.as_ref());
    let cx = Context::new(doc.data(), &r, &m.registry, &m.kernel);
    let sol = kmb_sketch::solution_of(cx.output(sketch_id, "sketch").unwrap()).unwrap();
    let constraint = sketch
        .constraints
        .iter()
        .find(|c| matches!(&c.kind,kmb_sketch::ConstraintKind::Length {value,..} if value=="width"))
        .unwrap()
        .id;
    sketch.place_label(constraint, [20.0, -8.0], &sol);
    doc.apply(Transaction::new(
        "寸法の札",
        vec![Op::SetFeatureData {
            id: sketch_id,
            data: serde_json::to_value(sketch).unwrap(),
        }],
    ))
    .unwrap();
    action(
        &mut doc,
        "drawing.import_dimensions",
        json!({"sheet":"sheet","id":"model","view":"top","features":[sketch_id]}),
    );
    let saved: Drawings = doc.data().section().unwrap();
    assert_eq!(saved.sheets[0].notes.len(), 1);
    let owner = saved.sheets[0].notes[0]["id"].as_str().unwrap().to_string();
    assert!((span(&picture(&mut doc), &owner) - 40.0).abs() < 1e-7);
    let before = saved.sheets[0].notes[0]["at"].clone();
    let width = doc
        .data()
        .params
        .iter()
        .find(|p| p.name == "width")
        .unwrap()
        .id;
    doc.apply(Transaction::new(
        "幅",
        vec![Op::SetParamExpr {
            id: width,
            expr: "60 mm".into(),
        }],
    ))
    .unwrap();
    assert!((span(&picture(&mut doc), &owner) - 60.0).abs() < 1e-7);
    let after: Drawings = doc.data().section().unwrap();
    assert_eq!(after.sheets[0].notes[0]["at"], before);
}

#[test]
fn cylinder_centerline_follows_named_end_circles_in_a_side_view() {
    let mut doc = crate::drawing_cylinder_document();
    edit(&mut doc, |d| d.sheets[0].views.truncate(1));
    let m = modules();
    let r = doc.recompute(&m.registry, m.kernel.as_ref());
    let cx = Context::new(doc.data(), &r, &m.registry, &m.kernel);
    let (_, views) = sheet_picture_with_lines(&cx, "sheet").unwrap();
    let (_, shape) = cx.visible_bodies()[0];
    let edges = cx.kernel.arc_edges(shape).unwrap();
    let keys = views["front"]
        .iter()
        .filter_map(|line| match &line.key {
            LineKey::Edge { key }
                if edges.iter().any(|e| {
                    let mut faces = e.faces.clone();
                    faces.sort();
                    e.arc.is_some() && faces == key.faces
                }) =>
            {
                Some(key.clone())
            }
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(keys.len(), 2, "{keys:?}");
    let refs = keys
        .into_iter()
        .map(|key| json!({"view":"front","key":key,"at":"center"}))
        .collect::<Vec<_>>();
    action(
        &mut doc,
        "drawing.center_line",
        json!({"sheet":"sheet","id":"axis","refs":refs}),
    );
    let p = picture(&mut doc);
    assert!(p.problems.iter().all(|p| p.warning), "{:?}", p.problems);
    let paths = strokes(&p, "axis");
    assert_eq!(paths.len(), 1);
    let Path::Line { a, b } = paths[0] else {
        panic!("中心線")
    };
    assert!((a[0] - b[0]).abs() < 1e-7);
    assert!(((a[1] - b[1]).abs() - 17.0).abs() < 1e-7);
}
