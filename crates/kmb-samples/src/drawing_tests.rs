//! 確かめること: 最新の形の三面図・曲面・隠れ・断面・尺度と区画の操作が同じ入口で使える。
use kmb_core::{Context, Document, Op, Transaction};
use kmb_drawing::*;
use kmb_interchange::drawing::{Item, LineStyle, Path, SheetPicture};

thread_local! {
    // 文書の再計算の控えにあるハンドルを、同じカーネルで読む。
    static MODULES: kmb_core::Modules = kmb_bundle::modules();
}
fn modules() -> kmb_core::Modules {
    MODULES.with(Clone::clone)
}

fn picture(doc: &mut Document) -> SheetPicture {
    let m = modules();
    let r = doc.recompute(&m.registry, m.kernel.as_ref());
    assert_eq!(r.error_count(), 0, "{r:?}");
    sheet_picture(
        &Context::new(doc.data(), &r, &m.registry, &m.kernel),
        "sheet",
    )
    .unwrap()
}
fn strokes<'a>(p: &'a SheetPicture, owner: &str) -> Vec<&'a Path> {
    p.items
        .iter()
        .filter_map(|i| match i {
            Item::Stroke { owner: o, path, .. } if o == owner => Some(path),
            _ => None,
        })
        .collect()
}
fn bounds(paths: &[&Path]) -> [f64; 2] {
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for p in paths {
        let pts = match p {
            Path::Line { a, b } => vec![*a, *b],
            Path::Poly { points } => points.clone(),
            Path::Arc { c, r, .. } => vec![[c[0] - r, c[1] - r], [c[0] + r, c[1] + r]],
        };
        for q in pts {
            for k in 0..2 {
                lo[k] = lo[k].min(q[k]);
                hi[k] = hi[k].max(q[k]);
            }
        }
    }
    [hi[0] - lo[0], hi[1] - lo[1]]
}
fn edit(doc: &mut Document, f: impl FnOnce(&mut Drawings)) {
    let mut d: Drawings = doc.data().section().unwrap();
    f(&mut d);
    doc.apply(Transaction::new(
        "図面の変更",
        vec![doc.data().section_op(&d).unwrap()],
    ))
    .unwrap();
}
#[test]
fn box_three_views_have_four_outlines_with_expected_lengths_and_scale() {
    let mut doc = crate::drawing_box_document();
    edit(&mut doc, |d| d.sheets[0].views.truncate(3));
    let p = picture(&mut doc);
    assert!(p.problems.iter().all(|p| p.warning), "{:?}", p.problems);
    for (id, size) in [
        ("front", [40., 10.]),
        ("top", [40., 20.]),
        ("right", [20., 10.]),
    ] {
        let lines = strokes(&p, id);
        assert_eq!(lines.len(), 4, "{id}: {lines:?}");
        assert_eq!(bounds(&lines), size);
    }
    edit(&mut doc, |d| {
        d.sheets[0].views[0].scale = Ratio { num: 1., den: 2. }
    });
    let p = picture(&mut doc);
    assert_eq!(bounds(&strokes(&p, "front")), [20., 5.]);
    // 親の位置を変えると投影ビューも同じだけ動く。
    edit(&mut doc, |d| d.sheets[0].views[0].at[0] += 12.);
    let q = picture(&mut doc);
    for (a, b) in strokes(&p, "top").iter().zip(strokes(&q, "top")) {
        if let (Path::Line { a: x, .. }, Path::Line { a: y, .. }) = (a, b) {
            assert!((y[0] - x[0] - 12.).abs() < 1e-8);
        }
    }
}
#[test]
fn hole_hidden_lines_are_dashed() {
    let mut hole = crate::drawing_hole_document();
    let p = picture(&mut hole);
    assert!(
        p.items
            .iter()
            .any(|i| matches!(i,Item::Stroke{owner,line:LineStyle::Dashed,..}if owner=="front")),
        "{p:?}"
    );
}
#[test]
fn cylinder_named_ends_are_arcs() {
    if modules().kernel.name() == "truck" {
        kmb_contract::skip!("truck は円の辺を名前付きで列挙できないため、網の折れ線で描く");
    }
    let mut cylinder = crate::drawing_cylinder_document();
    let p = picture(&mut cylinder);
    assert!(
        p.items
            .iter()
            .any(|i| matches!(i,Item::Stroke{owner,path:Path::Arc{..},..}if owner=="top")),
        "{p:?}"
    );
}
#[test]
fn cylinder_silhouettes_follow_the_view_direction() {
    let mut cylinder = crate::drawing_cylinder_document();
    let p = picture(&mut cylinder);
    let side = bounds(&strokes(&p, "right"));
    let top = bounds(&strokes(&p, "top"));
    assert!(
        (side[0] - 20.).abs() < 0.1 && (side[1] - 10.).abs() < 0.01,
        "{side:?}"
    );
    assert!(
        (top[0] - 20.).abs() < 0.1 && (top[1] - 20.).abs() < 0.1,
        "{top:?}"
    );
}
#[test]
fn section_area_tracks_model_and_json_is_repeatable() {
    let mut doc = crate::drawing_box_document();
    let p = picture(&mut doc);
    assert!(!p.problems.iter().any(|p| !p.warning), "{:?}", p.problems);
    let area = |p: &SheetPicture| {
        p.items
            .iter()
            .filter_map(|i| {
                if let Item::Hatch { loops, .. } = i {
                    Some(
                        loops
                            .iter()
                            .map(|l| {
                                l.windows(2)
                                    .map(|w| w[0][0] * w[1][1] - w[1][0] * w[0][1])
                                    .sum::<f64>()
                                    .abs()
                                    / 2.
                            })
                            .sum::<f64>(),
                    )
                } else {
                    None
                }
            })
            .sum::<f64>()
    };
    assert!((area(&p) - 200.).abs() < 0.01, "{}", area(&p));
    assert_eq!(
        serde_json::to_string(&p).unwrap(),
        serde_json::to_string(&picture(&mut doc)).unwrap()
    );
    let saved = Document::from_json(&doc.to_json()).unwrap();
    let mut back = saved;
    let restored = picture(&mut back);
    assert_eq!(restored.items.len(), p.items.len());
    for (i, (a, b)) in restored.items.iter().zip(&p.items).enumerate() {
        assert_eq!(a, b, "restored item {i}");
    }
    assert_eq!(restored.problems, p.problems);
    let height = doc
        .data()
        .params
        .iter()
        .find(|p| p.name == "height")
        .unwrap()
        .id;
    doc.apply(Transaction::new(
        "高さ",
        vec![Op::SetParamExpr {
            id: height,
            expr: "15 mm".into(),
        }],
    ))
    .unwrap();
    let q = picture(&mut doc);
    assert!((area(&q) - 300.).abs() < 0.01, "{}", area(&q));
    assert!(bounds(&strokes(&q, "right"))[1] > bounds(&strokes(&p, "right"))[1] + 4.9);
    let mut hole = crate::drawing_hole_document();
    let hole_picture = picture(&mut hole);
    assert!(
        (area(&hole_picture) - 120.).abs() < 0.2,
        "{:?}: {}",
        hole_picture.problems,
        area(&hole_picture)
    );
    let mut cylinder = crate::drawing_cylinder_document();
    let cylinder_picture = picture(&mut cylinder);
    assert!(
        (area(&cylinder_picture) - 200.).abs() < 0.2,
        "{:?}: {}",
        cylinder_picture.problems,
        area(&cylinder_picture)
    );
}
#[test]
fn drawing_actions_share_declared_arguments_and_undoable_section() {
    let m = modules();
    let mut doc = Document::new();
    let r = doc.recompute(&m.registry, m.kernel.as_ref());
    let args = serde_json::json!({"id":"new","name":"図面"});
    let mut cx = Context::new(doc.data(), &r, &m.registry, &m.kernel);
    cx.args = &args;
    let outcome = m.extensions.run("drawing.add_sheet", &cx).unwrap();
    let kmb_core::Outcome::Change { edits, .. } = outcome else {
        panic!("区画の変更")
    };
    doc.apply_edits("用紙", edits).unwrap();
    assert_eq!(doc.data().section::<Drawings>().unwrap().sheets.len(), 1);
    doc.undo();
    assert!(doc.data().section::<Drawings>().unwrap().sheets.is_empty());
}

#[test]
fn open_section_is_an_error_while_other_views_are_kept() {
    struct OpenMesh;
    impl kmb_core::Kernel for OpenMesh {
        fn name(&self) -> &str {
            "open drawing mesh"
        }
        fn mesh(
            &self,
            _shape: kmb_core::ShapeHandle,
            _tolerance: f64,
        ) -> Result<kmb_core::Mesh, kmb_core::KernelError> {
            Ok(kmb_core::Mesh {
                positions: vec![[0., 0., 0.], [40., 0., 0.], [40., 20., 10.]],
                triangles: vec![[0, 1, 2]],
                edges: vec![vec![[0., 0., 0.], [40., 20., 10.]]],
                ..Default::default()
            })
        }
    }
    let m = modules();
    let mut doc = crate::drawing_box_document();
    let r = doc.recompute(&m.registry, m.kernel.as_ref());
    let kernel: std::sync::Arc<dyn kmb_core::Kernel> = std::sync::Arc::new(OpenMesh);
    let p = sheet_picture(&Context::new(doc.data(), &r, &m.registry, &kernel), "sheet").unwrap();
    assert!(
        p.problems
            .iter()
            .any(|p| p.target == "section" && !p.warning)
    );
    assert!(strokes(&p, "section").is_empty());
    assert!(!strokes(&p, "front").is_empty());
}

#[test]
fn sources_named_edges_uncut_bodies_and_visibility_are_independent() {
    let m = modules();
    let mut doc = crate::drawing_assembly_document();
    let roots = doc
        .data()
        .features
        .iter()
        .filter(|f| f.type_id == kmb_cad_extrude::EXTRUDE)
        .map(|f| f.id)
        .collect::<Vec<_>>();
    let component = doc.data().components[0].id;
    edit(&mut doc, |d| {
        if let ViewKind::Base { source, .. } = &mut d.sheets[0].views[0].kind {
            *source = Source::Component { id: component };
        }
        if let ViewKind::Section { uncut, .. } = &mut d.sheets[0].views[3].kind {
            *uncut = roots.clone();
        }
    });
    let p = picture(&mut doc);
    assert!(p.problems.iter().all(|p| p.warning), "{:?}", p.problems);
    assert!(!p.items.iter().any(|i| matches!(i, Item::Hatch { .. })));
    let visibility = kmb_interchange::visibility::Visibility {
        hidden: roots.iter().copied().collect(),
        hidden_components: [component].into_iter().collect(),
        ..Default::default()
    };
    doc.apply(Transaction::new(
        "表示",
        vec![doc.data().section_op(&visibility).unwrap()],
    ))
    .unwrap();
    assert_eq!(picture(&mut doc), p);
    let r = doc.recompute(&m.registry, m.kernel.as_ref());
    let cx = Context::new(doc.data(), &r, &m.registry, &m.kernel);
    let (_, table) = sheet_picture_with_lines(&cx, "sheet").unwrap();
    assert!(!table["front"].is_empty());
    assert!(table["front"].iter().all(|l| match &l.key {
        kmb_interchange::drawing::LineKey::Edge { key } => roots.contains(&key.body),
        _ => false,
    }));
    assert_eq!(table, sheet_picture_with_lines(&cx, "sheet").unwrap().1);
    edit(&mut doc, |d| {
        if let ViewKind::Base { source, .. } = &mut d.sheets[0].views[0].kind {
            *source = Source::Bodies {
                bodies: vec![kmb_core::FeatureId(999999)],
            };
        }
    });
    let p = picture(&mut doc);
    assert!(p.problems.iter().any(|p| p.target == "front" && !p.warning));
    assert!(strokes(&p, "front").is_empty());
}
#[test]
fn first_angle_and_in_plane_rotation_follow_named_axes() {
    let mut doc = crate::drawing_box_document();
    edit(&mut doc, |d| d.sheets[0].views.truncate(3));
    let p = picture(&mut doc);
    edit(&mut doc, |d| {
        d.sheets[0].projection = Projection::FirstAngle
    });
    let q = picture(&mut doc);
    let min_y = |p: &SheetPicture| {
        strokes(p, "top")
            .iter()
            .filter_map(|p| match p {
                Path::Line { a, b } => Some(a[1].min(b[1])),
                _ => None,
            })
            .fold(f64::INFINITY, f64::min)
    };
    assert!((min_y(&p) - min_y(&q) - 20.).abs() < 0.01);
    edit(&mut doc, |d| {
        if let ViewKind::Base { rotate, .. } = &mut d.sheets[0].views[0].kind {
            *rotate = 90;
        }
    });
    let rotated = picture(&mut doc);
    let size = bounds(&strokes(&rotated, "front"));
    assert!(
        (size[0] - 10.).abs() < 0.01 && (size[1] - 40.).abs() < 0.01,
        "{size:?}"
    );
}

#[test]
fn occurrence_lines_keep_their_placement_and_body_keys_after_source_changes() {
    use kmb_core::{
        EvalCtx, EvalError, Feature, FeatureEntry, FeatureId, InputRef, Outputs, Value,
    };
    use kmb_interchange::drawing::LineKey;

    #[derive(serde::Deserialize)]
    struct PlacedBody {
        source: FeatureId,
    }
    impl Feature for PlacedBody {
        fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
            Ok(vec![kmb_cad::output(self.source, "body")])
        }
        fn instance_of(&self) -> Option<InputRef> {
            Some(kmb_cad::output(self.source, "body"))
        }
        fn evaluate(&self, cx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
            let source = kmb_cad::body_of(cx, self.source)?;
            let shape = cx
                .kernel()
                .instance(source, &kmb_core::kernel::translation([55., 0., 3.]))
                .map_err(kmb_cad::kernel_err)?;
            Ok([("body".into(), Value::Shape(shape))].into())
        }
    }
    let mut m = kmb_bundle::modules();
    std::sync::Arc::get_mut(&mut m.registry).unwrap().register(
        "test.drawing_occurrence@1",
        |value| {
            serde_json::from_value::<PlacedBody>(value.clone())
                .map(|f| Box::new(f) as Box<dyn Feature>)
                .map_err(|e| EvalError::InvalidData(e.to_string()))
        },
    );
    let mut doc = crate::drawing_box_document();
    let source = doc
        .data()
        .features
        .iter()
        .find(|f| f.type_id == kmb_cad_extrude::EXTRUDE)
        .unwrap()
        .id;
    let occurrence = doc.alloc_feature_id();
    let moved = doc.alloc_feature_id();
    let entry = |id, type_id: &str, data| FeatureEntry {
        id,
        type_id: type_id.into(),
        name: "図面のボディ".into(),
        data,
        suppressed: false,
        pin_bodies: false,
    };
    doc.apply(Transaction::new(
        "出現と元の移動",
        vec![
            Op::InsertFeature {
                index: None,
                entry: entry(
                    occurrence,
                    "test.drawing_occurrence@1",
                    serde_json::json!({"source":source}),
                ),
            },
            Op::InsertFeature {
                index: None,
                entry: entry(
                    moved,
                    "solid.move@1",
                    serde_json::json!({"body":source,"x":"10 mm"}),
                ),
            },
        ],
    ))
    .unwrap();
    edit(&mut doc, |d| {
        d.sheets[0].views.truncate(3);
        if let ViewKind::Base { source, .. } = &mut d.sheets[0].views[0].kind {
            *source = Source::Bodies {
                bodies: vec![occurrence],
            };
        }
    });
    let read = |doc: &mut Document| {
        let report = doc.recompute(&m.registry, m.kernel.as_ref());
        assert_eq!(report.error_count(), 0, "{report:?}");
        sheet_picture_with_lines(
            &Context::new(doc.data(), &report, &m.registry, &m.kernel),
            "sheet",
        )
        .unwrap()
    };
    let (p, lines) = read(&mut doc);
    assert!(p.problems.iter().all(|p| p.warning), "{:?}", p.problems);
    assert_eq!(bounds(&strokes(&p, "front")), [40., 10.]);
    let lower_left = |p: &SheetPicture| {
        strokes(p, "front")
            .iter()
            .flat_map(|p| match p {
                Path::Line { a, b } => vec![*a, *b],
                Path::Poly { points } => points.clone(),
                _ => panic!("箱の線"),
            })
            .fold([f64::INFINITY; 2], |lo, q| {
                [lo[0].min(q[0]), lo[1].min(q[1])]
            })
    };
    assert_eq!(lower_left(&p), [105., 63.]);
    assert!(!lines["front"].is_empty());
    assert!(
        lines["front"]
            .iter()
            .all(|line| { matches!(&line.key, LineKey::Edge { key } if key.body == occurrence) })
    );
    let keys =
        |lines: &std::collections::BTreeMap<String, Vec<kmb_interchange::drawing::ViewLine>>| {
            lines["front"]
                .iter()
                .map(|line| line.key.clone())
                .collect::<Vec<_>>()
        };
    let width = doc
        .data()
        .params
        .iter()
        .find(|p| p.name == "width")
        .unwrap()
        .id;
    doc.apply(Transaction::new(
        "元の幅を変更",
        vec![Op::SetParamExpr {
            id: width,
            expr: "45 mm".into(),
        }],
    ))
    .unwrap();
    let (q, updated) = read(&mut doc);
    assert_eq!(bounds(&strokes(&q, "front")), [45., 10.]);
    assert_eq!(lower_left(&q), [105., 63.]);
    assert_eq!(keys(&lines), keys(&updated));
    assert_eq!(q, read(&mut doc).0);
    edit(&mut doc, |d| {
        if let ViewKind::Base { source: s, .. } = &mut d.sheets[0].views[0].kind {
            *s = Source::Bodies {
                bodies: vec![source],
            };
        }
    });
    let (p, lines) = read(&mut doc);
    assert_eq!(lower_left(&p), [60., 60.]);
    assert_eq!(bounds(&strokes(&p, "front")), [45., 10.]);
    assert!(
        lines["front"]
            .iter()
            .all(|line| { matches!(&line.key, LineKey::Edge { key } if key.body == source) })
    );
}

#[test]
fn drawing_commands_build_edit_export_and_delete_without_changing_the_model() {
    let m = modules();
    let mut doc = crate::drawing_box_document();
    let report = doc.recompute(&m.registry, m.kernel.as_ref());
    assert_eq!(report.error_count(), 0, "{report:?}");
    let features = doc.data().features.clone();
    let run = |doc: &mut Document, id: &str, args: serde_json::Value| {
        let report = doc.recompute(&m.registry, m.kernel.as_ref());
        let mut cx = Context::new(doc.data(), &report, &m.registry, &m.kernel);
        cx.args = &args;
        m.extensions.run(id, &cx)
    };
    let change = |doc: &mut Document, outcome| {
        let kmb_core::Outcome::Change { label, edits } = outcome else {
            panic!("図面の区画を変更する")
        };
        doc.apply_edits(&label, edits).unwrap();
    };
    for (id, args) in [
        (
            "drawing.add_sheet",
            serde_json::json!({"id":"commands","name":"操作の図面"}),
        ),
        (
            "drawing.base_view",
            serde_json::json!({"sheet":"commands","id":"base","orientation":"front","at":[50,60]}),
        ),
        (
            "drawing.projected_view",
            serde_json::json!({"sheet":"commands","id":"projected","parent":"base","side":"top","at":[60,0]}),
        ),
        (
            "drawing.section_view",
            serde_json::json!({"sheet":"commands","id":"cut","parent":"base","line":[[20,-5],[20,25]],"letter":"A","at":[120,0]}),
        ),
        (
            "drawing.detail_view",
            serde_json::json!({"sheet":"commands","id":"enlarged","parent":"base","center":[20,10],"radius":"8 mm","detail_scale":{"num":2,"den":1},"letter":"B","at":[200,140]}),
        ),
        (
            "drawing.move_view",
            serde_json::json!({"sheet":"commands","id":"base","at":[65,40]}),
        ),
        (
            "drawing.set_view",
            serde_json::json!({"sheet":"commands","id":"base","scale":{"num":1,"den":2},"options":{"hidden":true}}),
        ),
    ] {
        let outcome = run(&mut doc, id, args).unwrap();
        change(&mut doc, outcome);
    }
    let before_failed = doc.to_json();
    for (id, args) in [
        (
            "drawing.projected_view",
            serde_json::json!({"sheet":"commands","id":"broken","parent":"missing","side":"right"}),
        ),
        (
            "drawing.move_view",
            serde_json::json!({"sheet":"commands","id":"base","at":[0,0],"unknown":true}),
        ),
        (
            "drawing.delete",
            serde_json::json!({"sheet":"commands","id":"base"}),
        ),
    ] {
        assert!(run(&mut doc, id, args).is_err(), "{id}");
        assert_eq!(doc.to_json(), before_failed);
    }
    let kmb_core::Outcome::File { contents, .. } = run(
        &mut doc,
        "drawing.picture",
        serde_json::json!({"sheet":"commands"}),
    )
    .unwrap() else {
        panic!("図面の JSON を返す")
    };
    let picture: SheetPicture = serde_json::from_slice(&contents).unwrap();
    assert!(picture.problems.is_empty(), "{:?}", picture.problems);
    let report = doc.recompute(&m.registry, m.kernel.as_ref());
    let (_, lines) = sheet_picture_with_lines(
        &Context::new(doc.data(), &report, &m.registry, &m.kernel),
        "commands",
    )
    .unwrap();
    // 切断線や詳細の円を含まず、形の辺だけで尺度を測る。
    let model_paths = lines["base"]
        .iter()
        .map(|line| &line.path)
        .collect::<Vec<_>>();
    assert_eq!(bounds(&model_paths), [20., 5.]);
    assert!(
        picture
            .items
            .iter()
            .any(|item| matches!(item, Item::Hatch { owner, .. } if owner == "cut"))
    );
    assert!(
        picture
            .items
            .iter()
            .any(|item| matches!(item, Item::Stroke { owner, .. } if owner == "enlarged"))
    );
    assert_eq!(
        serde_json::from_slice::<SheetPicture>(&serde_json::to_vec(&picture).unwrap()).unwrap(),
        picture
    );
    for id in ["enlarged", "cut", "projected", "base", "commands"] {
        let outcome = run(
            &mut doc,
            "drawing.delete",
            serde_json::json!({"sheet":"commands","id":id}),
        )
        .unwrap();
        change(&mut doc, outcome);
    }
    assert!(
        !doc.data()
            .section::<Drawings>()
            .unwrap()
            .sheets
            .iter()
            .any(|s| s.id == "commands")
    );
    assert_eq!(doc.data().features, features);
    doc.undo();
    assert!(
        doc.data()
            .section::<Drawings>()
            .unwrap()
            .sheets
            .iter()
            .any(|s| s.id == "commands")
    );
}

mod annotations;
