//! 点を端に置くロフトの形、保存、参照を確かめる。
use crate::tests::registry as base_registry;
use kmb_cad::{Operation, PointRef};
use kmb_cad_sweep::{LOFT, Loft, PathRef, Section, SectionKind};
use kmb_core::{
    Document, FeatureId, FeatureStatus, Kernel, Op, PointId, Registry, Transaction, Value,
};
use kmb_kernel_smlib::SmlibKernel;
use kmb_samples::sample::{DocBuilder, SketchBuilder};
use kmb_sketch::SketchPlane;
use serde_json::json;

fn registry() -> Registry {
    let mut reg = base_registry();
    kmb_cad_datum::register(&mut reg);
    reg
}

fn profile(id: FeatureId) -> Section {
    Section {
        profile: Some(id),
        ..Default::default()
    }
}

fn point(p: PointRef) -> Section {
    Section {
        kind: SectionKind::Point,
        point: Some(p),
        ..Default::default()
    }
}

fn shape(doc: &mut Document, kernel: &SmlibKernel, id: FeatureId) -> kmb_core::ShapeHandle {
    let report = doc.recompute(&registry(), kernel);
    crate::tests::assert_ok(&report);
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(id) else {
        panic!();
    };
    let Some(Value::Shape(h)) = outputs.get("body") else {
        panic!();
    };
    assert_eq!(kernel.check(*h).unwrap(), (true, 1, 0));
    *h
}

fn cone(rails: bool, first: bool) -> (Document, FeatureId) {
    let mut b = DocBuilder::new();
    b.param("height", "20 mm");
    let mut circle = SketchBuilder::new(SketchPlane::Xy);
    let c = circle.fixed_point(0.0, 0.0);
    circle.circle(c, "10 mm", 5.0);
    let base = b.feature(kmb_sketch::SKETCH, "底面", circle.build());
    let mut tip = SketchBuilder::new(SketchPlane::Xy);
    let p = tip.fixed_point(0.0, 0.0);
    let mut tip = tip.build();
    tip.offset = Some("height".into());
    let tip = b.feature(kmb_sketch::SKETCH, "先の点", tip);
    let mut paths = vec![];
    if rails {
        let mut rail = SketchBuilder::new(SketchPlane::Zx);
        let a = rail.fixed_point(0.0, 5.0);
        let end = rail.point_from(a, "height", "5 mm", [20.0, 0.0]);
        let line = rail.line(a, end);
        let sk = b.feature(kmb_sketch::SKETCH, "案内線", rail.build());
        paths.push(PathRef {
            sketch: sk,
            entity: Some(line.0),
        });
    }
    let mut sections = vec![
        profile(base),
        point(PointRef::Sketch {
            feature: tip,
            point: PointId(p.0),
        }),
    ];
    if first {
        sections.reverse();
    }
    let loft = b.feature(
        LOFT,
        "円すい",
        Loft {
            sections,
            rails: paths,
            operation: Operation::New,
            target: None,
        },
    );
    (b.finish("点へのロフト"), loft)
}

#[test]
fn a_cone_loft_reaches_a_sketch_point_with_or_without_a_rail() {
    let kernel = SmlibKernel::new();
    for rails in [false, true] {
        for first in [false, true] {
            let (mut doc, loft) = cone(rails, first);
            for height in [20.0, 30.0] {
                let id = doc.data().param_by_name("height").unwrap().id;
                doc.apply(Transaction::new(
                    "高さ",
                    vec![Op::SetParamExpr {
                        id,
                        expr: format!("{height} mm"),
                    }],
                ))
                .unwrap();
                let h = shape(&mut doc, &kernel, loft);
                let want = std::f64::consts::PI * 25.0 * height / 3.0;
                let v = kernel.volume(h).unwrap();
                assert!(
                    (v - want).abs() < 1e-2,
                    "rails={rails} first={first}: {v} vs {want}"
                );
                let faces = kernel.faces(h).unwrap();
                let absent = format!("f{}:{}", loft.0, if first { "start" } else { "end" });
                assert!(!faces.iter().any(|f| f.name == absent));
                assert!(
                    faces
                        .iter()
                        .any(|f| f.name.starts_with(&format!("f{}:side:", loft.0)))
                );
                let mut loaded = Document::from_json(&doc.to_json()).unwrap();
                assert!(
                    (kernel.volume(shape(&mut loaded, &kernel, loft)).unwrap() - want).abs() < 1e-2
                );
            }
        }
    }
}

#[test]
fn a_pyramid_loft_reaches_a_datum_point() {
    let kernel = SmlibKernel::new();
    let mut b = DocBuilder::new();
    let mut square = SketchBuilder::new(SketchPlane::Xy);
    let c = square.fixed_point(0.0, 0.0);
    square.rectangle(c, "10 mm", "10 mm", [0.0, 0.0, 10.0, 10.0]);
    let base = b.feature(kmb_sketch::SKETCH, "四角", square.build());
    let tip = b.feature(
        kmb_cad_datum::DATUM_POINT,
        "先の点",
        kmb_cad_datum::DatumPointFeature {
            point: kmb_cad_datum::Point::At {
                at: [5.0, 5.0, 20.0],
            },
        },
    );
    let loft = b.feature(
        LOFT,
        "四角すい",
        Loft {
            sections: vec![profile(base), point(PointRef::Datum { feature: tip })],
            rails: vec![],
            operation: Operation::New,
            target: None,
        },
    );
    let mut doc = b.finish("四角すい");
    let h = shape(&mut doc, &kernel, loft);
    assert!((kernel.volume(h).unwrap() - 100.0 * 20.0 / 3.0).abs() < 1e-3);
    assert_eq!(kernel.faces(h).unwrap().len(), 5);
}

#[test]
fn point_lofts_reject_an_interior_point_two_points_and_a_point_on_the_profile_plane() {
    let kernel = SmlibKernel::new();
    let (mut doc, loft) = cone(false, false);
    let old: Loft = serde_json::from_value(doc.data().feature(loft).unwrap().data.clone()).unwrap();
    for sections in [
        vec![
            old.sections[0].clone(),
            old.sections[1].clone(),
            old.sections[0].clone(),
        ],
        vec![
            old.sections[1].clone(),
            old.sections[0].clone(),
            old.sections[1].clone(),
        ],
    ] {
        let mut data = old.clone();
        data.sections = sections;
        doc.apply(Transaction::new(
            "点の位置",
            vec![Op::SetFeatureData {
                id: loft,
                data: serde_json::to_value(data).unwrap(),
            }],
        ))
        .unwrap();
        assert!(matches!(
            doc.recompute(&registry(), &kernel).feature(loft),
            Some(FeatureStatus::Error { .. })
        ));
    }
    doc.apply(Transaction::new(
        "戻す",
        vec![Op::SetFeatureData {
            id: loft,
            data: serde_json::to_value(old).unwrap(),
        }],
    ))
    .unwrap();
    let id = doc.data().param_by_name("height").unwrap().id;
    doc.apply(Transaction::new(
        "同じ平面",
        vec![Op::SetParamExpr {
            id,
            expr: "0 mm".into(),
        }],
    ))
    .unwrap();
    let status = format!("{:?}", doc.recompute(&registry(), &kernel).feature(loft));
    assert!(status.contains("同じ平面"), "{status}");
}

#[test]
fn old_loft_sections_keep_their_saved_shape_and_truck_reports_unsupported() {
    let old = json!({"sections":[{"profile":1},{"profile":2}],"operation":"new"});
    let data: Loft = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(serde_json::to_value(data).unwrap(), old);
    let err = kmb_kernel_truck::TruckKernel::new()
        .loft_to_point(&[], [0.0; 3], false, &[], "point")
        .unwrap_err();
    assert!(matches!(err, kmb_core::KernelError::Unsupported(_)));
}

fn add(doc: &mut Document, kind: &str, name: &str, data: serde_json::Value) -> FeatureId {
    let id = doc.alloc_feature_id();
    doc.apply(Transaction::new(
        name,
        vec![Op::InsertFeature {
            index: None,
            entry: kmb_core::FeatureEntry {
                id,
                type_id: kind.into(),
                name: name.into(),
                data,
                suppressed: false,
                pin_bodies: false,
            },
        }],
    ))
    .unwrap();
    id
}

#[test]
fn point_loft_surfaces_close_at_an_exact_tip_for_square_circle_and_teardrop_profiles() {
    use kmb_core::kernel::{Plane, Profile, ProfileEdge, ProfileLoop, Region, Segment};
    let line = |a, b| Segment::Line { a, b };
    let square = vec![
        line([-5.0, -5.0], [5.0, -5.0]),
        line([5.0, -5.0], [5.0, 5.0]),
        line([5.0, 5.0], [-5.0, 5.0]),
        line([-5.0, 5.0], [-5.0, -5.0]),
    ];
    let circle = vec![Segment::Circle {
        center: [0.0; 2],
        radius: 5.0,
    }];
    let tear = vec![
        Segment::Arc {
            center: [0.0; 2],
            radius: 5.0,
            start: 0.0,
            end: std::f64::consts::PI,
        },
        line([-5.0, 0.0], [0.0, -8.0]),
        line([0.0, -8.0], [5.0, 0.0]),
    ];
    let kernel = SmlibKernel::new();
    for (segments, area, count) in [
        (square, 100.0, 5),
        (circle, 25.0 * std::f64::consts::PI, 2),
        (tear, 12.5 * std::f64::consts::PI + 40.0, 4),
    ] {
        let p = Profile {
            plane: Plane::XY,
            regions: vec![Region {
                outer: ProfileLoop {
                    edges: segments
                        .into_iter()
                        .enumerate()
                        .map(|(i, segment)| ProfileEdge {
                            name: format!("e{i}"),
                            segment,
                            reversed: false,
                        })
                        .collect(),
                },
                holes: vec![],
            }],
        };
        for first in [false, true] {
            let body = kernel
                .loft_to_point(
                    std::slice::from_ref(&p),
                    [0.0, 0.0, 10.0],
                    first,
                    &[],
                    "tip",
                )
                .unwrap();
            assert_eq!(kernel.check(body).unwrap(), (true, 1, 0));
            assert!((kernel.volume(body).unwrap() - area * 10.0 / 3.0).abs() < 1e-3);
            let faces = kernel.faces(body).unwrap();
            assert_eq!(faces.len(), count);
            assert!(
                !faces
                    .iter()
                    .any(|f| f.name == if first { "tip:start" } else { "tip:end" })
            );
        }
    }
}

#[test]
fn a_loft_side_keeps_its_name_for_a_sketch_and_chamfer_when_the_profile_changes() {
    let kernel = SmlibKernel::new();
    let mut b = DocBuilder::new();
    b.param("width", "10 mm");
    let mut square = SketchBuilder::new(SketchPlane::Xy);
    let o = square.fixed_point(0.0, 0.0);
    square.rectangle(o, "width", "10 mm", [0.0, 0.0, 10.0, 10.0]);
    let base = b.feature(kmb_sketch::SKETCH, "底面", square.build());
    let tip = b.feature(
        kmb_cad_datum::DATUM_POINT,
        "先の点",
        json!({"point":{"kind":"at","at":[5.0,5.0,20.0]}}),
    );
    let loft = b.feature(
        LOFT,
        "四角すい",
        Loft {
            sections: vec![profile(base), point(PointRef::Datum { feature: tip })],
            rails: vec![],
            operation: Operation::New,
            target: None,
        },
    );
    let mut doc = b.finish("名前で参照する四角すい");
    let body = shape(&mut doc, &kernel, loft);
    let side = kernel
        .faces(body)
        .unwrap()
        .into_iter()
        .find(|f| f.planar.as_ref().is_some_and(|p| p.normal[0] > 0.2))
        .unwrap();
    let edge = kernel
        .edges(body)
        .unwrap()
        .into_iter()
        .find(|e| e.faces.contains(&side.name) && e.faces.contains(&format!("f{}:start", loft.0)))
        .unwrap();
    let mut sk = SketchBuilder::new(SketchPlane::face(loft, side.name.clone()));
    let c = sk.fixed_point(0.0, 0.0);
    sk.circle(c, "1 mm", 0.5);
    let sketch = add(
        &mut doc,
        kmb_sketch::SKETCH,
        "側面のスケッチ",
        serde_json::to_value(sk.build()).unwrap(),
    );
    let chamfer = add(
        &mut doc,
        crate::CHAMFER,
        "縁の面取り",
        json!({"target":loft,"rows":[{"edges":{"kind":"edge","feature":loft,"faces":edge.faces},"distance":"0.2 mm"}]}),
    );
    for width in [10.0, 12.0] {
        let id = doc.data().param_by_name("width").unwrap().id;
        doc.apply(Transaction::new(
            "幅",
            vec![Op::SetParamExpr {
                id,
                expr: format!("{width} mm"),
            }],
        ))
        .unwrap();
        let h = shape(&mut doc, &kernel, chamfer);
        let volume = kernel.volume(h).unwrap();
        let full = width * 10.0 * 20.0 / 3.0;
        assert!(volume < full && volume > full * 0.98, "{volume} vs {full}");
        let report = doc.recompute(&registry(), &kernel);
        let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(loft) else {
            panic!();
        };
        let Some(Value::Shape(body)) = outputs.get("body") else {
            panic!();
        };
        let current = kernel
            .faces(*body)
            .unwrap()
            .into_iter()
            .find(|f| f.name == side.name)
            .unwrap()
            .planar
            .unwrap();
        let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(sketch) else {
            panic!();
        };
        let p = kmb_sketch::profile_value(&outputs["profile"]).unwrap();
        use kmb_core::kernel::{dot, normalize, sub};
        assert!(dot(normalize(p.plane.normal()), normalize(current.normal)).abs() > 1.0 - 1e-6);
        assert!(
            dot(
                sub(p.plane.origin, current.point),
                normalize(current.normal)
            )
            .abs()
                < 1e-6
        );
    }
}

#[test]
fn a_body_vertex_can_be_the_tip_without_consuming_the_referenced_body() {
    let kernel = SmlibKernel::new();
    let mut b = DocBuilder::new();
    let mut sk = SketchBuilder::new(SketchPlane::Xy);
    let o = sk.fixed_point(0.0, 0.0);
    sk.rectangle(o, "10 mm", "10 mm", [0.0, 0.0, 10.0, 10.0]);
    let base = b.feature(kmb_sketch::SKETCH, "底面", sk.build());
    let tip = b.feature(
        kmb_cad_datum::DATUM_POINT,
        "先の点",
        json!({"point":{"kind":"at","at":[5.0,5.0,20.0]}}),
    );
    let first = b.feature(
        LOFT,
        "四角すい",
        Loft {
            sections: vec![profile(base), point(PointRef::Datum { feature: tip })],
            rails: vec![],
            operation: Operation::New,
            target: None,
        },
    );
    let mut doc = b.finish("頂点へのロフト");
    let body = shape(&mut doc, &kernel, first);
    let vertex = kmb_core::vertices(&kernel, body)
        .into_iter()
        .find(|v| (v.at[2] - 20.0).abs() < 1e-6)
        .unwrap();
    let old: Loft =
        serde_json::from_value(doc.data().feature(first).unwrap().data.clone()).unwrap();
    let second = add(
        &mut doc,
        LOFT,
        "頂点へのロフト",
        serde_json::to_value(Loft {
            sections: vec![
                old.sections[0].clone(),
                point(PointRef::Vertex {
                    feature: first,
                    vertex: vertex.name,
                }),
            ],
            ..old
        })
        .unwrap(),
    );
    let h = shape(&mut doc, &kernel, second);
    assert!((kernel.volume(h).unwrap() - 100.0 * 20.0 / 3.0).abs() < 1e-3);
    let reg = registry();
    let report = doc.recompute(&reg, &kernel);
    let ids: Vec<_> = kmb_core::visible_bodies(doc.data(), &reg, &report)
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert_eq!(ids, vec![first, second]);
}

#[test]
fn loft_point_actions_and_row_fields_share_the_declared_point_data() {
    use kmb_core::{Context, Extension, Extensions};
    let kernel: std::sync::Arc<dyn Kernel> = std::sync::Arc::new(SmlibKernel::new());
    let (mut doc, loft) = cone(false, false);
    let reg = registry();
    let report = doc.recompute(&reg, kernel.as_ref());
    let data = doc.data().feature(loft).unwrap().data.clone();
    let mut editing = data.clone();
    assert!(!reg.descriptor(LOFT).unwrap().prune_rows(&mut editing));
    assert_eq!(editing["sections"].as_array().unwrap().len(), 2);

    let args = json!({"sections":data["sections"]});
    let cx = Context {
        args: &args,
        ..Context::new(doc.data(), &report, &reg, &kernel)
    };
    let extensions = Extensions(vec![std::sync::Arc::new(kmb_cad_sweep::SweepCommands)]);
    let kmb_core::Outcome::Change { edits, .. } =
        extensions.run(kmb_cad_sweep::LOFT_SKETCH, &cx).unwrap()
    else {
        panic!();
    };
    let kmb_core::Edit::Insert { data: inserted, .. } = &edits[0] else {
        panic!();
    };
    let inserted: Loft = serde_json::from_value(inserted.clone()).unwrap();
    assert_eq!(inserted.sections[1].kind, SectionKind::Point);
    let handles = kmb_cad_sweep::SweepCommands
        .handles(kmb_cad_sweep::LOFT_SKETCH, &cx)
        .unwrap();
    assert!(
        handles[0]
            .choices
            .iter()
            .any(|c| c.value.get("kind") == Some(&json!("point")))
    );
    let descriptor = reg.descriptor(LOFT).unwrap();
    for (key, shown) in [
        ("sections.1.point.kind", true),
        ("sections.1.point.sketch", true),
        ("sections.1.point.point", true),
        ("sections.1.point.datum", false),
        ("sections.1.point.vertex", false),
        ("sections.1.profile", false),
    ] {
        assert_eq!(
            descriptor.field_at(key).unwrap().applies(&data),
            shown,
            "{key}"
        );
    }
    let invalid = json!({"not_an_argument":1});
    let cx = Context {
        args: &invalid,
        ..cx
    };
    assert_eq!(
        extensions
            .run(kmb_cad_sweep::LOFT_SKETCH, &cx)
            .unwrap_err()
            .kind,
        kmb_core::ProblemKind::Input
    );
}

#[test]
fn loft_point_recipes_move_and_mirror_the_tip_with_the_profiles() {
    use kmb_core::Payload;
    let kernel = SmlibKernel::new();
    let (mut doc, loft) = cone(false, false);
    let report = doc.recompute(&registry(), &kernel);
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(loft) else {
        panic!();
    };
    let recipe = kmb_cad::Recipe::from_value(&outputs["recipe"]).unwrap();
    let moved = recipe.moved(
        &kmb_core::kernel::translation([1.0, 2.0, 3.0]),
        Operation::New,
    );
    let mirrored = moved.mirrored([0.0; 3], [1.0, 0.0, 0.0], Operation::New);
    let kmb_cad::Recipe::Loft {
        point: Some(point),
        sections,
        ..
    } = mirrored
    else {
        panic!();
    };
    assert_eq!(point, [-1.0, 2.0, 23.0]);
    assert_eq!(sections[0].plane.origin, [-1.0, 2.0, 3.0]);
}

#[test]
fn a_point_loft_interpolates_multiple_profiles_before_the_tip() {
    use kmb_core::kernel::{Plane, Profile, ProfileEdge, ProfileLoop, Region, Segment};
    let circle = |z, r| Profile {
        plane: Plane::XY.offset(z),
        regions: vec![Region {
            outer: ProfileLoop {
                edges: vec![ProfileEdge {
                    name: "circle".into(),
                    segment: Segment::Circle {
                        center: [0.0; 2],
                        radius: r,
                    },
                    reversed: false,
                }],
            },
            holes: vec![],
        }],
    };
    let kernel = SmlibKernel::new();
    let body = kernel
        .loft_to_point(
            &[circle(0.0, 5.0), circle(10.0, 3.0)],
            [0.0, 0.0, 20.0],
            false,
            &[],
            "profiles",
        )
        .unwrap();
    assert_eq!(kernel.check(body).unwrap(), (true, 1, 0));
    // r(z)=5−0.15z−0.005z² を0〜20で積分した、π r(z)² の体積。
    let expected = std::f64::consts::PI * (500.0 - 300.0 - 220.0 / 3.0 + 60.0 + 16.0);
    assert!((kernel.volume(body).unwrap() - expected).abs() < 1e-3);
}

#[test]
fn a_nearby_rail_does_not_move_the_selected_loft_tip() {
    use kmb_core::kernel::{PlanarPath, Plane, Profile, ProfileEdge, ProfileLoop, Region, Segment};
    let edge = |name: &str, segment| ProfileEdge {
        name: name.into(),
        segment,
        reversed: false,
    };
    let base = Profile {
        plane: Plane::XY,
        regions: vec![Region {
            outer: ProfileLoop {
                edges: vec![edge(
                    "circle",
                    Segment::Circle {
                        center: [0.0; 2],
                        radius: 5.0,
                    },
                )],
            },
            holes: vec![],
        }],
    };
    let rail = PlanarPath {
        plane: Plane::ZX,
        edges: vec![edge(
            "rail",
            Segment::Line {
                a: [0.0, 5.0],
                b: [20.0, 0.0005],
            },
        )],
    };
    let kernel = SmlibKernel::new();
    for first in [false, true] {
        let body = kernel
            .loft_to_point(
                std::slice::from_ref(&base),
                [0.0, 0.0, 20.0],
                first,
                std::slice::from_ref(&rail),
                "tip",
            )
            .unwrap();
        let mesh = kernel.mesh(body, 0.1).unwrap();
        let tips: Vec<_> = mesh
            .positions
            .iter()
            .filter(|p| (p[2] as f64 - 20.0).abs() < 1e-6)
            .collect();
        assert!(!tips.is_empty());
        assert!(
            tips.iter().all(|p| p[0].abs() < 1e-6 && p[1].abs() < 1e-6),
            "{tips:?}"
        );
    }
}

#[test]
fn the_loft_tip_can_be_repicked_while_its_reference_is_incomplete() {
    use kmb_core::{Context, Extension, Extensions};
    let kernel: std::sync::Arc<dyn Kernel> = std::sync::Arc::new(SmlibKernel::new());
    let (mut doc, loft) = cone(false, false);
    let reg = registry();
    let report = doc.recompute(&reg, kernel.as_ref());
    let original = doc.data().feature(loft).unwrap().data.clone();
    let args = json!({"point":original["sections"][1]["point"]});
    let mut data = doc.data().clone();
    data.features
        .iter_mut()
        .find(|f| f.id == loft)
        .unwrap()
        .data["sections"][1]["point"] = json!({"kind":"vertex"});
    let cx = Context {
        selected: Some(loft),
        args: &args,
        ..Context::new(&data, &report, &reg, &kernel)
    };
    let extensions = Extensions(vec![std::sync::Arc::new(kmb_cad_sweep::SweepCommands)]);
    let action = extensions.action(kmb_cad_sweep::LOFT_POINT, &cx).unwrap();
    assert!(action.disabled.is_none());
    assert!(action.is_at(kmb_core::Place::Panel));
    assert!(!action.is_at(kmb_core::Place::FeatureMenu));
    let unrelated = Context {
        selected: Some(FeatureId(1)),
        ..cx
    };
    assert!(
        extensions
            .action(kmb_cad_sweep::LOFT_POINT, &unrelated)
            .is_none()
    );
    let none = Context {
        selected: None,
        ..cx
    };
    let hidden = extensions.action(kmb_cad_sweep::LOFT_POINT, &none).unwrap();
    assert!(!hidden.places.is_empty());
    assert!(!hidden.is_at(kmb_core::Place::Toolbar) && !hidden.is_at(kmb_core::Place::Menu));

    let handles = kmb_cad_sweep::SweepCommands
        .handles(kmb_cad_sweep::LOFT_POINT, &cx)
        .unwrap();
    assert!(!handles[0].choices.is_empty());
    for choice in &handles[0].choices {
        let point: PointRef = serde_json::from_value(choice.value.clone()).unwrap();
        let feature = match point.input() {
            kmb_core::InputRef::Output { feature, .. } => feature,
            _ => panic!(),
        };
        assert!(data.index_of(feature).unwrap() < data.index_of(loft).unwrap());
    }
    let kmb_core::Outcome::Change { edits, .. } =
        extensions.run(kmb_cad_sweep::LOFT_POINT, &cx).unwrap()
    else {
        panic!();
    };
    let kmb_core::Edit::SetData { id, data } = &edits[0] else {
        panic!();
    };
    assert_eq!(*id, loft);
    let changed: Loft = serde_json::from_value(data.clone()).unwrap();
    assert_eq!(
        serde_json::to_value(&changed.sections[1].point).unwrap(),
        args["point"]
    );
}

/// 確かめること: 作業点の削除を理由つきで止め、他の点でロフトを続けない。
#[test]
fn a_loft_tip_stops_with_a_reason_when_its_datum_point_is_deleted() {
    let kernel = SmlibKernel::new();
    let mut b = DocBuilder::new();
    let mut circle = SketchBuilder::xy();
    let c = circle.fixed_point(0.0, 0.0);
    circle.circle(c, "10 mm", 5.0);
    let base = b.sketch("底面", circle);
    let tip = b.feature(
        kmb_cad_datum::DATUM_POINT,
        "端の作業点",
        json!({"point":{"kind":"at","at":[0.0,0.0,20.0]}}),
    );
    let loft = b.feature(
        LOFT,
        "点へのロフト",
        Loft {
            sections: vec![profile(base), point(PointRef::Datum { feature: tip })],
            rails: vec![],
            operation: Operation::New,
            target: None,
        },
    );
    let mut doc = b.finish("作業点を参照するロフト");
    shape(&mut doc, &kernel, loft);
    doc.apply(Transaction::new(
        "作業点を削除",
        vec![Op::RemoveFeature { id: tip }],
    ))
    .unwrap();
    let report = doc.recompute(&registry(), &kernel);
    let status = format!("{:?}", report.feature(loft));
    assert!(
        status.contains(&format!("参照先の機能 {tip} がありません")),
        "{status}"
    );
    assert!(!matches!(
        report.feature(loft),
        Some(FeatureStatus::Ok { .. })
    ));
}

/// 確かめること: スケッチから消えた点を、残った別の点に付け替えず止める。
#[test]
fn a_loft_tip_stops_with_a_reason_when_its_sketch_point_is_deleted() {
    let kernel = SmlibKernel::new();
    let mut b = DocBuilder::new();
    let mut circle = SketchBuilder::xy();
    let c = circle.fixed_point(0.0, 0.0);
    circle.circle(c, "10 mm", 5.0);
    let base = b.sketch("底面", circle);
    let mut tip = SketchBuilder::xy();
    tip.fixed_point(1.0, 1.0);
    let p = tip.fixed_point(0.0, 0.0);
    let mut tip = tip.build();
    tip.offset = Some("20 mm".into());
    let tip = b.feature(kmb_sketch::SKETCH, "端の点", tip);
    let loft = b.feature(
        LOFT,
        "点へのロフト",
        Loft {
            sections: vec![
                profile(base),
                point(PointRef::Sketch {
                    feature: tip,
                    point: PointId(p.0),
                }),
            ],
            rails: vec![],
            operation: Operation::New,
            target: None,
        },
    );
    let mut doc = b.finish("スケッチの点を参照するロフト");
    shape(&mut doc, &kernel, loft);
    let mut remaining = SketchBuilder::xy();
    remaining.fixed_point(1.0, 1.0);
    let mut remaining = remaining.build();
    remaining.offset = Some("20 mm".into());
    doc.apply(Transaction::new(
        "端の点を削除",
        vec![Op::SetFeatureData {
            id: tip,
            data: serde_json::to_value(remaining).unwrap(),
        }],
    ))
    .unwrap();
    let report = doc.recompute(&registry(), &kernel);
    assert!(matches!(
        report.feature(tip),
        Some(FeatureStatus::Ok { .. })
    ));
    let status = format!("{:?}", report.feature(loft));
    assert!(
        status.contains(&format!("スケッチの点 {} がありません", p.0))
            && status.contains("端の点を選び直してください"),
        "{status}"
    );
}

/// 確かめること: 形の変更で頂点名が消えた時は、近くの頂点に付け替えず止める。
#[test]
fn a_loft_tip_stops_with_a_reason_when_its_vertex_name_disappears() {
    let kernel = SmlibKernel::new();
    let mut b = DocBuilder::new();
    let mut circle = SketchBuilder::xy();
    let c = circle.fixed_point(0.0, 0.0);
    circle.circle(c, "10 mm", 5.0);
    let base = b.sketch("底面", circle);
    let mut square = SketchBuilder::xy();
    let o = square.fixed_point(-2.0, -2.0);
    square.rectangle(o, "4 mm", "4 mm", [-2.0, -2.0, 4.0, 4.0]);
    let mut square = square.build();
    square.offset = Some("20 mm".into());
    let tip_profile = b.feature(kmb_sketch::SKETCH, "頂点を持つ板の輪郭", square);
    let tip_body = b.feature(
        crate::EXTRUDE,
        "頂点を持つ板",
        crate::Extrude {
            profile: Some(tip_profile),
            distance: "2 mm".into(),
            ..Default::default()
        },
    );
    let mut doc = b.finish("ボディの頂点を参照するロフト");
    let body = shape(&mut doc, &kernel, tip_body);
    let vertex = kmb_core::vertices(&kernel, body)
        .into_iter()
        .find(|v| {
            (v.at[0] - 2.0).abs() < 1e-6
                && (v.at[1] - 2.0).abs() < 1e-6
                && (v.at[2] - 22.0).abs() < 1e-6
        })
        .unwrap();
    let loft = add(
        &mut doc,
        LOFT,
        "頂点へのロフト",
        serde_json::to_value(Loft {
            sections: vec![
                profile(base),
                point(PointRef::Vertex {
                    feature: tip_body,
                    vertex: vertex.name.clone(),
                }),
            ],
            rails: vec![],
            operation: Operation::New,
            target: None,
        })
        .unwrap(),
    );
    shape(&mut doc, &kernel, loft);
    let mut rounded = SketchBuilder::xy();
    let c = rounded.fixed_point(0.0, 0.0);
    rounded.circle(c, "4 mm", 2.0);
    let mut rounded = rounded.build();
    rounded.offset = Some("20 mm".into());
    doc.apply(Transaction::new(
        "板を円板にする",
        vec![Op::SetFeatureData {
            id: tip_profile,
            data: serde_json::to_value(rounded).unwrap(),
        }],
    ))
    .unwrap();
    let report = doc.recompute(&registry(), &kernel);
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(tip_body) else {
        panic!("円板を作れる")
    };
    let Value::Shape(body) = outputs["body"] else {
        panic!("円板の形")
    };
    assert!(
        !kmb_core::vertices(&kernel, body)
            .iter()
            .any(|v| v.name == vertex.name)
    );
    let status = format!("{:?}", report.feature(loft));
    assert!(
        status.contains("端の頂点") && status.contains("がありません。点を選び直してください"),
        "{status}"
    );
}
