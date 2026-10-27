//! 面を張る機能を、文書・寸法・参照の保存と実際のカーネルで確かめる。
use super::tests::{TestKernel, assert_ok};
use kmb_core::{Document, FeatureId, FeatureStatus, Op, Transaction, Value};
use kmb_samples::sample::{DocBuilder, SketchBuilder};
use kmb_sketch::SketchPlane;
use serde_json::json;

fn registry() -> kmb_core::Registry {
    let mut r = super::tests::registry();
    kmb_sketch_3d::register(&mut r);
    r
}
fn shape(report: &kmb_core::RecomputeReport, id: FeatureId) -> kmb_core::ShapeHandle {
    let FeatureStatus::Ok { outputs, .. } = report.feature(id).unwrap() else {
        panic!("{:?}", report.feature(id));
    };
    let Value::Shape(h) = outputs["body"] else {
        panic!("body が形ではない");
    };
    h
}
fn check(doc: &mut Document, id: FeatureId, k: &TestKernel) -> kmb_core::ShapeHandle {
    let report = doc.recompute(&registry(), &**k);
    assert_ok(&report);
    shape(&report, id)
}
fn round_trip_keeps_boundary_references(doc: &Document, id: FeatureId) {
    let mut restored = Document::from_json(&doc.to_json()).unwrap();
    assert_eq!(
        restored.data().feature(id).unwrap().data,
        doc.data().feature(id).unwrap().data
    );
    let kernel = TestKernel::new();
    let h = check(&mut restored, id, &kernel);
    assert_eq!(
        kernel.body_kind(h).unwrap(),
        kmb_core::kernel::BodyKind::Sheet
    );
}

fn rectangle() -> (DocBuilder, FeatureId) {
    let mut b = DocBuilder::new();
    b.param("width", "10 mm");
    let mut s = SketchBuilder::new(SketchPlane::Xy);
    let o = s.fixed_point(0., 0.);
    s.rectangle(o, "width", "10 mm", [0., 0., 10., 10.]);
    let sk = b.sketch("輪郭", s);
    (b, sk)
}
#[test]
fn planar_surface_follows_dimensions_and_survives_document_round_trip() {
    let k = TestKernel::new();
    if k.is_truck() {
        kmb_contract::skip!("truck は面のボディを作れない");
    }
    let (mut b, sk) = rectangle();
    let id = b.feature(
        kmb_cad_surface::SURFACE_PATCH,
        "面",
        json!({"boundary":[{"source":"region","profile":sk}]}),
    );
    let mut doc = b.finish("面");
    let h = check(&mut doc, id, &k);
    assert_eq!(k.body_kind(h).unwrap(), kmb_core::kernel::BodyKind::Sheet);
    assert!((k.faces(h).unwrap().iter().map(|f| f.area).sum::<f64>() - 100.).abs() < 1e-5);
    let names = k.face_names(h).unwrap();
    let rims = k
        .curve_edges(h)
        .unwrap()
        .iter()
        .map(|e| e.faces.clone())
        .collect::<Vec<_>>();
    let p = doc.data().param_by_name("width").unwrap().id;
    doc.apply(Transaction::new(
        "幅",
        vec![Op::SetParamExpr {
            id: p,
            expr: "20 mm".into(),
        }],
    ))
    .unwrap();
    let h = check(&mut doc, id, &k);
    assert!((k.faces(h).unwrap().iter().map(|f| f.area).sum::<f64>() - 200.).abs() < 1e-5);
    assert_eq!(names, k.face_names(h).unwrap());
    assert_eq!(
        rims,
        k.curve_edges(h)
            .unwrap()
            .iter()
            .map(|e| e.faces.clone())
            .collect::<Vec<_>>()
    );
    let saved = doc.to_json();
    let mut restored = Document::from_json(&saved).unwrap();
    let h = check(&mut restored, id, &k);
    assert_eq!(k.body_kind(h).unwrap(), kmb_core::kernel::BodyKind::Sheet);
}
#[test]
fn spatial_sketch_lines_form_a_sheet_and_invalid_wire_reference_stops() {
    let k = TestKernel::new();
    if k.is_truck() {
        kmb_contract::skip!("truck は面のボディを作れない");
    }
    let mut b = DocBuilder::new();
    let pts = [[0., 0., 0.], [10., 0., 1.], [10., 10., 0.], [0., 10., 1.]];
    let mut entities = Vec::new();
    for (i, p) in pts.iter().enumerate() {
        entities.push(json!({"type":"point","id":i+1,"p":p}));
    }
    for i in 0..4 {
        entities.push(json!({"type":"line","id":i+5,"a":i+1,"b":(i+1)%4+1}));
    }
    let sk = b.feature(
        kmb_sketch_3d::SKETCH3D,
        "立体の縁",
        json!({"entities":entities}),
    );
    let refs: Vec<_> = [5, 7, 8, 6]
        .iter()
        .map(|i| json!({"source":"wire","sketch":sk,"line":i}))
        .collect();
    let id = b.feature(
        kmb_cad_surface::SURFACE_PATCH,
        "曲面",
        json!({"boundary":refs}),
    );
    let mut doc = b.finish("曲面");
    let h = check(&mut doc, id, &k);
    assert_eq!(k.body_kind(h).unwrap(), kmb_core::kernel::BodyKind::Sheet);
    assert!(k.faces(h).unwrap().iter().map(|f| f.area).sum::<f64>() > 100.);
    round_trip_keeps_boundary_references(&doc, id);
    let mut data = doc.data().feature(id).unwrap().data.clone();
    data["boundary"][0]["line"] = json!(99);
    doc.apply(Transaction::new(
        "線を変える",
        vec![Op::SetFeatureData { id, data }],
    ))
    .unwrap();
    let r = doc.recompute(&registry(), &*k);
    assert!(
        matches!(r.feature(id),Some(FeatureStatus::Error{message}) if message.contains("線分 99 がありません"))
    );
}
#[test]
fn a_body_edge_loop_is_preserved_and_older_solid_document_is_unchanged() {
    let k = TestKernel::new();
    if k.is_truck() {
        kmb_contract::skip!("truck は面のボディを作れない");
    }
    let (mut b, sk) = rectangle();
    let body = b.feature(
        kmb_cad_extrude::EXTRUDE,
        "板",
        json!({"profile":sk,"distance":"5 mm"}),
    );
    let mut doc = b.finish("板");
    let r = doc.recompute(&registry(), &*k);
    assert_ok(&r);
    let before = doc.to_json();
    let solid = shape(&r, body);
    let volume = k.volume(solid).unwrap();
    let mut edges = k.edges(solid).unwrap();
    edges.extend(k.arc_edges(solid).unwrap());
    edges.extend(k.curve_edges(solid).unwrap());
    let refs: Vec<_> = edges
        .into_iter()
        .filter(|e| e.faces.iter().any(|f| f == &format!("f{}:end", body.0)))
        .map(|e| json!({"source":"edge","edge":{"kind":"edge","feature":body,"faces":e.faces}}))
        .collect();
    assert_eq!(refs.len(), 4);
    let id = doc.alloc_feature_id();
    doc.apply(Transaction::new(
        "面",
        vec![Op::InsertFeature {
            index: None,
            entry: kmb_core::FeatureEntry {
                id,
                type_id: kmb_cad_surface::SURFACE_PATCH.into(),
                name: "面".into(),
                data: json!({"boundary":refs}),
                suppressed: false,
                pin_bodies: false,
            },
        }],
    ))
    .unwrap();
    let h = check(&mut doc, id, &k);
    assert_eq!(k.body_kind(h).unwrap(), kmb_core::kernel::BodyKind::Sheet);
    assert!((k.faces(h).unwrap().iter().map(|f| f.area).sum::<f64>() - 100.).abs() < 1e-5);
    assert_eq!(k.volume(solid).unwrap(), volume);
    round_trip_keeps_boundary_references(&doc, id);
    let mut old = Document::from_json(&before).unwrap();
    let r = old.recompute(&registry(), &*k);
    assert_ok(&r);
    assert!((k.volume(shape(&r, body)).unwrap() - volume).abs() < 1e-5);
    assert_eq!(old.to_json(), before);
}

#[test]
fn reordering_the_same_number_of_sketch_lines_keeps_the_selected_entities() {
    use std::sync::Arc;
    if std::env::var("JC_KERNEL").as_deref() == Ok("truck") {
        kmb_contract::skip!("truck は面のボディを作れない");
    }
    let kernel: Arc<dyn kmb_core::Kernel> = Arc::new(kmb_kernel_smlib::SmlibKernel::new());
    let modules = kmb_core::Modules::assemble(
        vec![kmb_sketch_3d::module(), kmb_cad_surface::module()],
        kernel,
    );
    let points = [
        [0., 0., 0.],
        [10., 0., 0.],
        [10., 10., 0.],
        [0., 10., 0.],
        [30., 0., 0.],
        [50., 0., 0.],
        [50., 20., 0.],
        [30., 20., 0.],
    ];
    let mut entities = Vec::new();
    for (i, p) in points.iter().enumerate() {
        entities.push(json!({"type":"point","id":i+1,"p":p}));
    }
    for i in 0..8 {
        let base = if i < 4 { 0 } else { 4 };
        entities.push(json!({"type":"line","id":i+9,"a":i+1,"b":base+(i+1-base)%4+1}));
    }
    let mut builder = DocBuilder::new();
    let sketch = builder.feature(
        kmb_sketch_3d::SKETCH3D,
        "2 つの輪",
        json!({"entities":entities}),
    );
    let mut doc = builder.finish("輪郭");
    let report = doc.recompute(&modules.registry, modules.kernel.as_ref());
    assert_ok(&report);
    let boundary: Vec<_> = (0..4)
        .map(|wire| json!({"source":"wire","sketch":sketch,"wire":wire}))
        .collect();
    let args = json!({"boundary":boundary});
    let cx = kmb_core::Context {
        args: &args,
        ..kmb_core::Context::new(doc.data(), &report, &modules.registry, &modules.kernel)
    };
    let kmb_core::Outcome::Change { label, edits } =
        modules.extensions.run(kmb_cad_surface::PATCH, &cx).unwrap()
    else {
        panic!("面を足す操作");
    };
    let added = doc.apply_edits(&label, edits).unwrap();
    let id = *added.last().unwrap();
    let report = doc.recompute(&modules.registry, modules.kernel.as_ref());
    assert_ok(&report);
    let before = shape(&report, id);
    assert!(
        (modules
            .kernel
            .faces(before)
            .unwrap()
            .iter()
            .map(|f| f.area)
            .sum::<f64>()
            - 100.)
            .abs()
            < 1e-5
    );
    let mut data = doc.data().feature(sketch).unwrap().data.clone();
    let entities = data["entities"].as_array_mut().unwrap();
    let lines = entities.split_off(8);
    entities.extend_from_slice(&lines[4..]);
    entities.extend_from_slice(&lines[..4]);
    doc.apply(Transaction::new(
        "線の並び",
        vec![Op::SetFeatureData { id: sketch, data }],
    ))
    .unwrap();
    let report = doc.recompute(&modules.registry, modules.kernel.as_ref());
    assert_ok(&report);
    let after = shape(&report, id);
    let area = modules
        .kernel
        .faces(after)
        .unwrap()
        .iter()
        .map(|f| f.area)
        .sum::<f64>();
    assert!(
        (area - 100.).abs() < 1e-5,
        "同じEntity IDの輪を読む。出力の順を入れ替えても面積は100のまま: {area}"
    );
    let saved = &doc.data().feature(id).unwrap().data["boundary"];
    assert!(
        saved
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r.get("wire").is_none() && r["line"].as_u64().is_some())
    );
    let old_lines = match report.feature(sketch).unwrap() {
        FeatureStatus::Ok { outputs, .. } => outputs["lines"].clone(),
        _ => unreachable!(),
    };
    let mut data = doc.data().feature(sketch).unwrap().data.clone();
    let entries = data["entities"].as_array_mut().unwrap();
    let old = entries
        .iter_mut()
        .find(|e| e["type"] == "line" && e["id"] == 9)
        .unwrap();
    old["id"] = json!(99);
    doc.apply(Transaction::new(
        "同じ座標の別の線分",
        vec![Op::SetFeatureData { id: sketch, data }],
    ))
    .unwrap();
    let report = doc.recompute(&modules.registry, modules.kernel.as_ref());
    let new_lines = match report.feature(sketch).unwrap() {
        FeatureStatus::Ok { outputs, .. } => outputs["lines"].clone(),
        _ => unreachable!(),
    };
    assert_eq!(
        old_lines, new_lines,
        "出力の幾何だけでは別IDへの差し替えが分からない"
    );
    assert!(
        matches!(report.feature(id),Some(FeatureStatus::Error{message}) if message.contains("線分 9 がありません")),
        "{:?}",
        report.feature(id)
    );
}

#[test]
fn truck_reports_sheet_creation_as_unsupported() {
    let kernel = kmb_kernel_truck::TruckKernel::new();
    let (mut builder, sketch) = rectangle();
    let id = builder.feature(
        kmb_cad_surface::SURFACE_PATCH,
        "面",
        json!({"boundary":[{"source":"region","profile":sketch}]}),
    );
    let mut doc = builder.finish("面");
    let report = doc.recompute(&registry(), &kernel);
    assert!(
        matches!(report.feature(id),Some(FeatureStatus::Error{message}) if message.contains("対応していません")),
        "{:?}",
        report.feature(id)
    );
}

#[test]
fn one_curve_whole_output_is_explicit_and_refuses_an_added_second_curve() {
    let kernel = TestKernel::new();
    if kernel.is_truck() {
        kmb_contract::skip!("truck は面のボディを作れない");
    }
    let mut builder = DocBuilder::new();
    let sketch=builder.feature(kmb_sketch_3d::SKETCH3D,"円の全体",json!({"entities":[{"type":"point","id":1,"p":[0.,0.,0.]},{"type":"circle","id":2,"center":1,"radius":3.,"normal":[0.,0.,1.]}]}));
    let id = builder.feature(
        kmb_cad_surface::SURFACE_PATCH,
        "面",
        json!({"boundary":[{"source":"wire","sketch":sketch,"whole":true}]}),
    );
    let mut doc = builder.finish("曲線の全体");
    let h = check(&mut doc, id, &kernel);
    assert!(
        (kernel.faces(h).unwrap().iter().map(|f| f.area).sum::<f64>() - std::f64::consts::PI * 9.)
            .abs()
            < 1e-3
    );
    round_trip_keeps_boundary_references(&doc, id);
    let mut data = doc.data().feature(sketch).unwrap().data.clone();
    data["entities"].as_array_mut().unwrap().extend([
        json!({"type":"point","id":3,"p":[20.,0.,0.]}),
        json!({"type":"circle","id":4,"center":3,"radius":5.,"normal":[0.,0.,1.]}),
    ]);
    doc.apply(Transaction::new(
        "別の曲線",
        vec![Op::SetFeatureData { id: sketch, data }],
    ))
    .unwrap();
    let report = doc.recompute(&registry(), &*kernel);
    assert!(
        matches!(report.feature(id),Some(FeatureStatus::Error{message}) if message.contains("線が1本")),
        "{:?}",
        report.feature(id)
    );
}
