//! 確かめること: 網の読み込みの共通の操作、埋め込み、名前、取り消しと加工への受け渡し。
use kmb_cad_import_step::mesh::{IMPORT_MESH_FILE, ImportMeshCommands};
use kmb_core::{
    Context, Document, Extensions, FeatureId, FeatureStatus, Kernel, LengthUnit, Outcome, Value,
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

fn kernel() -> Arc<dyn Kernel> {
    kmb_bundle::kernel()
}
fn shape(r: &kmb_core::RecomputeReport, id: FeatureId) -> kmb_core::ShapeHandle {
    let Some(FeatureStatus::Ok { outputs, .. }) = r.feature(id) else {
        panic!("{:?}", r.feature(id))
    };
    let Value::Shape(h) = outputs["body"] else {
        panic!("shape")
    };
    h
}
fn dir(label: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("mesh-{label}-{}", std::process::id()));
    std::fs::create_dir_all(&p).unwrap();
    p
}
fn import(
    doc: &mut Document,
    k: &Arc<dyn Kernel>,
    path: &Path,
    args: serde_json::Value,
) -> Vec<FeatureId> {
    let reg = kmb_bundle::registry();
    let report = doc.recompute(&reg, k.as_ref());
    let ex = Extensions(vec![Arc::new(ImportMeshCommands)]);
    let mut args = args;
    args["path"] = json!(path);
    let outcome = ex
        .run(
            IMPORT_MESH_FILE,
            &Context {
                args: &args,
                ..Context::new(doc.data(), &report, &reg, k)
            },
        )
        .unwrap();
    let Outcome::Change { label, edits } = outcome else {
        panic!("change")
    };
    doc.apply_edits(&label, edits).unwrap()
}
fn stl(path: &Path, reverse: bool) {
    let mut m = kmb_contract::kernel::mesh_cube();
    if reverse {
        for t in &mut m.triangles {
            t.swap(1, 2);
        }
    }
    let mut b = Vec::new();
    kmb_io_stl::write_binary(&[m], &mut b).unwrap();
    std::fs::write(path, b).unwrap();
}

#[test]
fn imported_stl_is_embedded_named_and_undoable_from_the_common_action() {
    let k = kernel();
    let reg = kmb_bundle::registry();
    let mut doc = Document::new();
    let folder = dir("roundtrip");
    let path = folder.join("箱.STL");
    stl(&path, true);
    let ids = import(&mut doc, &k, &path, json!({}));
    assert_eq!(ids.len(), 1);
    assert_eq!(doc.data().body_names[&ids[0]], "箱");
    assert!(doc.data().components.is_empty());
    let saved = doc.to_json();
    std::fs::remove_dir_all(folder).unwrap();
    let mut loaded = Document::from_json(&saved).unwrap();
    let report = loaded.recompute(&reg, k.as_ref());
    let h = shape(&report, ids[0]);
    assert!((k.mass(h).unwrap().volume - 8.).abs() < 1e-9);
    assert_eq!(k.body_kind(h).unwrap(), kmb_core::kernel::BodyKind::Mesh);
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(ids[0]) else {
        panic!()
    };
    assert!(
        kmb_core::warnings_of(outputs)
            .iter()
            .any(|s| s.contains("外向き"))
    );
    assert!(doc.undo());
    assert!(doc.data().features.is_empty());
    assert!(doc.data().body_names.is_empty());
    assert!(doc.redo());
    let data = doc.data().feature(ids[0]).unwrap().data.clone();
    let mut scaled = data;
    scaled["unit"] = json!("cm");
    doc.apply_edits(
        "単位",
        vec![kmb_core::Edit::SetData {
            id: ids[0],
            data: scaled,
        }],
    )
    .unwrap();
    let r = doc.recompute(&reg, k.as_ref());
    assert!((k.volume(shape(&r, ids[0])).unwrap() - 8000.).abs() < 1e-6);
}

#[test]
fn threemf_objects_keep_names_and_units_and_share_one_component() {
    let bodies = (0..2)
        .map(|_| kmb_io_stl::threemf::Body3mf {
            name: "同じ & 部品".into(),
            mesh: kmb_contract::kernel::mesh_cube(),
            color: [100, 120, 140],
            material: "試験".into(),
            faces: BTreeMap::new(),
        })
        .collect::<Vec<_>>();
    let bytes = kmb_io_stl::threemf::write_3mf(&bodies, LengthUnit::In, &[]);
    let folder = dir("3mf");
    let path = folder.join("組.3MF");
    std::fs::write(&path, bytes).unwrap();
    let k = kernel();
    let reg = kmb_bundle::registry();
    let mut doc = Document::new();
    let ids = import(&mut doc, &k, &path, json!({"unit":"m"}));
    assert_eq!(ids.len(), 2);
    assert_eq!(doc.data().components.len(), 1);
    assert_eq!(doc.data().components[0].name, "組");
    assert_eq!(doc.data().components[0].bodies, ids);
    assert_eq!(doc.data().body_names[&ids[0]], "同じ & 部品");
    assert_eq!(doc.data().body_names[&ids[1]], "同じ & 部品(2)");
    let r = doc.recompute(&reg, k.as_ref());
    for id in &ids {
        assert!((k.volume(shape(&r, *id)).unwrap() - 8.).abs() < 1e-5);
        assert_eq!(doc.data().feature(*id).unwrap().data["unit"], "in");
    }
    let next = import(&mut doc, &k, &path, json!({}));
    assert_eq!(doc.data().body_names[&next[0]], "同じ & 部品(3)");
    assert!(doc.undo());
    assert_eq!(doc.data().features.len(), 2);
    assert!(doc.undo());
    assert!(doc.data().features.is_empty());
    assert!(doc.data().components.is_empty());
    std::fs::remove_dir_all(folder).unwrap();
}

#[derive(Debug)]
struct Deny;
impl kmb_core::FileGate for Deny {
    fn open(&self, _: &Path) -> Result<PathBuf, String> {
        Err("許したフォルダの外です".into())
    }
}
#[test]
fn mesh_import_rejects_unknown_arguments_and_paths_outside_the_allowed_roots() {
    let k = kernel();
    let reg = kmb_bundle::registry();
    let mut doc = Document::new();
    let report = doc.recompute(&reg, k.as_ref());
    let ex = Extensions(vec![Arc::new(ImportMeshCommands)]);
    let folder = dir("roots");
    let path = folder.join("箱.stl");
    stl(&path, false);
    let args = json!({"path":path});
    let cx = Context {
        args: &args,
        files: Some(&Deny),
        ..Context::new(doc.data(), &report, &reg, &k)
    };
    assert!(
        ex.run(IMPORT_MESH_FILE, &cx)
            .unwrap_err()
            .message
            .contains("フォルダの外")
    );
    assert!(
        ex.run(
            IMPORT_MESH_FILE,
            &Context {
                args: &json!({"unknown":true}),
                ..cx
            }
        )
        .is_err()
    );
    assert!(doc.data().features.is_empty());
    std::fs::remove_dir_all(folder).unwrap();
}

#[test]
fn mesh_bodies_export_as_stl_and_are_omitted_with_a_reason_from_step() {
    use kmb_samples::sample::{DocBuilder, SketchBuilder};
    let k = kernel();
    let reg = kmb_bundle::registry();
    let mut b = DocBuilder::new();
    let mut s = SketchBuilder::xy();
    let o = s.fixed_point(10., 0.);
    s.rectangle(o, "2 mm", "2 mm", [10., 0., 12., 2.]);
    let sk = b.sketch("輪郭", s);
    let native = b.feature(
        crate::EXTRUDE,
        "立体",
        crate::Extrude {
            profile: Some(sk),
            distance: "2 mm".into(),
            ..Default::default()
        },
    );
    let mut doc = b.finish("立体");
    let folder = dir("export");
    let path = folder.join("網.stl");
    stl(&path, false);
    let _ids = import(&mut doc, &k, &path, json!({}));
    let r = doc.recompute(&reg, k.as_ref());
    let ex = Extensions(vec![Arc::new(kmb_io_stl::ExportCommands)]);
    let cx = Context::new(doc.data(), &r, &reg, &k);
    let Outcome::File { contents, info, .. } = ex.run(kmb_io_stl::EXPORT_STL, &cx).unwrap() else {
        panic!()
    };
    assert!(!contents.is_empty());
    assert_eq!(info["bodies"], 2);
    let Outcome::File { info, .. } = ex.run(kmb_io_stl::EXPORT_STEP, &cx).unwrap() else {
        panic!()
    };
    assert_eq!(info["bodies"], 1);
    assert!(
        info["warnings"][0]
            .as_str()
            .unwrap()
            .contains("網のボディ 1 個")
    );
    let selected = Context {
        args: &json!({"exclude":[native]}),
        ..cx
    };
    let err = ex.run(kmb_io_stl::EXPORT_STEP, &selected).unwrap_err();
    assert!(err.message.contains("STL・3MF・OBJ"));
    std::fs::remove_dir_all(folder).unwrap();
}

#[test]
fn an_imported_mesh_can_be_the_model_of_a_machining_setup() {
    let folder = dir("cam");
    let path = folder.join("箱.stl");
    stl(&path, false);
    let k = kernel();
    let reg = kmb_bundle::registry();
    let mut doc = Document::new();
    let ids = import(&mut doc, &k, &path, json!({}));
    let reference = kmb_samples::reference_cam();
    let data = reference.doc().data();
    let tools = data.feature(reference.tools).unwrap();
    let setup = data.feature(reference.setup).unwrap();
    let face = data.feature(reference.face).unwrap();
    let mut sd = setup.data.clone();
    sd["body"] = json!(ids[0]);
    let mut fd = face.data.clone();
    fd["setup"] = json!({"$new":1});
    fd["tools"] = json!({"$new":0});
    let ids = doc
        .apply_edits(
            "加工",
            vec![
                kmb_core::Edit::Insert {
                    type_id: tools.type_id.clone(),
                    name: "工具".into(),
                    data: tools.data.clone(),
                },
                kmb_core::Edit::Insert {
                    type_id: setup.type_id.clone(),
                    name: "セットアップ".into(),
                    data: sd,
                },
                kmb_core::Edit::Insert {
                    type_id: face.type_id.clone(),
                    name: "フェイス".into(),
                    data: fd,
                },
            ],
        )
        .unwrap();
    let r = doc.recompute(&reg, k.as_ref());
    crate::tests::assert_ok(&r);
    let Some(FeatureStatus::Ok { outputs, .. }) = r.feature(ids[2]) else {
        panic!()
    };
    let (_, path) = outputs
        .values()
        .find_map(kmb_cam_commands::toolpath_of)
        .unwrap();
    assert!(!path.moves.is_empty());
    std::fs::remove_dir_all(folder).unwrap();
}
