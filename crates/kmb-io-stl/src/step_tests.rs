//! 確かめること: STEP の両書き出しで、属性と形の互換を保つ。
use crate::commands::{EXPORT_STEP, EXPORT_STEP_EACH, ExportCommands};
use kmb_core::{
    Context, DocumentData, Extensions, FeatureId, FeatureResult, FeatureStatus, Kernel,
    KernelError, Outcome, Properties, RecomputeReport, Registry, ShapeHandle, Value,
};
use serde_json::json;
use std::sync::Arc;

const HEADER: &str = "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('kieMonoBench model'),'2;1');\nFILE_NAME('part','stamp',(''),(''),'kieMonoBench','kieMonoBench (SMLib)','');\nFILE_SCHEMA(('schema'));\nENDSEC;\nDATA;\n#1=PRODUCT('part','part','',());\nENDSEC;\nEND-ISO-10303-21;\n";
struct HeaderKernel;
impl Kernel for HeaderKernel {
    fn name(&self) -> &str {
        "header-test"
    }
    fn export_step_named(
        &self,
        _: &[ShapeHandle],
        _: &[String],
        _: &str,
    ) -> Result<String, KernelError> {
        Ok(HEADER.into())
    }
}
fn export(
    kernel: &Arc<dyn Kernel>,
    shape: ShapeHandle,
    properties: Properties,
    id: &str,
    args: &serde_json::Value,
) -> Result<Vec<String>, kmb_core::Problem> {
    let mut data = DocumentData {
        properties,
        ..Default::default()
    };
    data.body_names.insert(FeatureId(1), "箱".into());
    let mut report = RecomputeReport::default();
    report.features.push(FeatureResult {
        id: FeatureId(1),
        name: "箱".into(),
        status: FeatureStatus::Ok {
            outputs: [("body".into(), Value::Shape(shape))].into(),
            reused: false,
        },
    });
    let reg = Registry::new();
    let cx = Context {
        args,
        ..Context::new(&data, &report, &reg, kernel)
    };
    let outcome = Extensions(vec![Arc::new(ExportCommands)]).run(id, &cx)?;
    Ok(match outcome {
        Outcome::File { contents, .. } => vec![String::from_utf8(contents).unwrap()],
        Outcome::Files { files, .. } => files
            .into_iter()
            .map(|(_, bytes)| String::from_utf8(bytes).unwrap())
            .collect(),
        _ => panic!("STEP のファイル"),
    })
}
#[test]
fn step_actions_write_the_design_title() {
    let kernel: Arc<dyn Kernel> = Arc::new(HeaderKernel);
    for (id, args) in [
        (EXPORT_STEP, json!({})),
        (EXPORT_STEP_EACH, json!({})),
        (EXPORT_STEP, json!({"each":true})),
    ] {
        let texts = export(
            &kernel,
            ShapeHandle(1),
            Properties {
                title: "Inspection plate".into(),
                ..Default::default()
            },
            id,
            &args,
        )
        .unwrap();
        assert_eq!(texts.len(), 1);
        assert!(
            texts[0].contains("FILE_NAME('Inspection plate','stamp'"),
            "{id}: {}",
            texts[0]
        );
    }
}
#[test]
fn empty_step_attributes_keep_the_same_kernel_bytes_on_both_paths() {
    let kernel: Arc<dyn Kernel> = Arc::new(HeaderKernel);
    for id in [EXPORT_STEP, EXPORT_STEP_EACH] {
        let texts = export(
            &kernel,
            ShapeHandle(1),
            Properties {
                part_number: "P-123".into(),
                ..Default::default()
            },
            id,
            &json!({}),
        )
        .unwrap();
        assert_eq!(texts, [HEADER]);
    }
}

fn small_box(kernel: &Arc<dyn Kernel>) -> ShapeHandle {
    use kmb_core::{Plane, Profile, ProfileEdge, ProfileLoop, Region, Segment};
    let points = [[0., 0.], [4., 0.], [4., 3.], [0., 3.]];
    let profile = Profile {
        plane: Plane::XY,
        regions: vec![Region {
            outer: ProfileLoop {
                edges: (0..4)
                    .map(|i| ProfileEdge {
                        name: format!("edge{i}"),
                        segment: Segment::Line {
                            a: points[i],
                            b: points[(i + 1) % 4],
                        },
                        reversed: false,
                    })
                    .collect(),
            },
            holes: vec![],
        }],
    };
    kernel.extrude(&profile, 0., 10., "box").unwrap()
}

fn import_through_action(
    modules: &kmb_core::Modules,
    text: &str,
    original: ShapeHandle,
) -> (ShapeHandle, String) {
    struct Temp(std::path::PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp = Temp(std::env::temp_dir().join(format!(
        "kmb-step-attributes-{}-{stamp}.step",
        std::process::id()
    )));
    std::fs::write(&temp.0, text).unwrap();
    let mut doc = kmb_core::Document::new();
    let report = RecomputeReport::default();
    let cx = Context {
        file: Some(&temp.0),
        ..Context::new(doc.data(), &report, &modules.registry, &modules.kernel)
    };
    let action = modules
        .extensions
        .actions(&cx)
        .into_iter()
        .find(|a| a.role == Some(kmb_interchange::role::action::IMPORT_STEP))
        .unwrap();
    let Outcome::Change { label, edits } = modules.extensions.run(&action.id, &cx).unwrap() else {
        panic!("読み込みの変更")
    };
    let ids = doc.apply_edits(&label, edits).unwrap();
    let entry = &doc.data().features[0];
    assert_eq!(entry.data["upright"], true, "書き出した設計を見分ける");
    let name = entry.name.clone();
    // 比較する元の箱はこの文書の機能ではないので、再計算の回収から保護する。
    doc.hold_shapes([original]);
    let report = doc.recompute(&modules.registry, modules.kernel.as_ref());
    assert_eq!(report.error_count(), 0, "{:?}", report.features);
    let feature = report.features.iter().find(|f| f.id == ids[0]).unwrap();
    let FeatureStatus::Ok { outputs, .. } = &feature.status else {
        panic!("読み込みの形")
    };
    let Some(Value::Shape(shape)) = outputs.get("body") else {
        panic!("読み込みのボディ")
    };
    (*shape, name)
}

#[test]
fn smlib_step_attributes_keep_native_geometry_and_the_existing_name_reader() {
    // この試験は SMLib の書き手と読み手を確かめる。網への代替読み込みは混在書き出しで検出する。
    let kernel = kmb_bundle::kernel_of(kmb_bundle::KernelKind::Smlib);
    assert_eq!(kernel.name(), "smlib", "SMLib を含む組み立てで確かめる");
    let modules = kmb_core::Modules::assemble(kmb_bundle::all(), kernel.clone());
    let original = small_box(&kernel);
    let raw = kernel
        .export_step_named(&[original], &["箱".into()], "kieMonoBench")
        .unwrap();
    assert_eq!(
        crate::step_header::apply(&raw, &Properties::default())
            .unwrap()
            .as_bytes(),
        raw.as_bytes()
    );
    let label = "部品 O'Reilly\\tool😀";
    let encoded = "'\\X2\\90E854C1\\X0\\ O''Reilly\\\\tool\\X4\\0001F600\\X0\\'";
    let properties = Properties {
        title: label.into(),
        designer: label.into(),
        organization: label.into(),
        description: label.into(),
        ..Default::default()
    };
    for (id, properties) in [
        (EXPORT_STEP, Properties::default()),
        (EXPORT_STEP, properties.clone()),
        (EXPORT_STEP_EACH, properties),
    ] {
        let texts = export(&kernel, original, properties.clone(), id, &json!({})).unwrap();
        assert_eq!(texts.len(), 1);
        let text = &texts[0];
        if !properties.title.is_empty() {
            assert_eq!(text.matches(encoded).count(), 4);
        }
        let (shape, _) = import_through_action(&modules, text, original);
        assert!((kernel.volume(shape).unwrap() - 120.).abs() < 1e-6);
        let (lo, hi) = kernel.bounds(shape).unwrap();
        let mesh = kernel.mesh(shape, 0.001).unwrap();
        assert!(!mesh.positions.is_empty());
        for (i, end) in [4., 3., 10.].iter().enumerate() {
            let mesh_lo = mesh
                .positions
                .iter()
                .map(|p| f64::from(p[i]))
                .fold(f64::INFINITY, f64::min);
            let mesh_hi = mesh
                .positions
                .iter()
                .map(|p| f64::from(p[i]))
                .fold(f64::NEG_INFINITY, f64::max);
            assert!(mesh_lo.abs() < 1e-6);
            assert!((mesh_hi - end).abs() < 1e-6);
            assert!(
                lo[i] <= mesh_lo && hi[i] >= mesh_hi,
                "外形の箱は実際の頂点を包む"
            );
        }
        for operation in [
            kmb_core::kernel::BooleanOp::Union,
            kmb_core::kernel::BooleanOp::Intersect,
        ] {
            let combined = kernel.boolean(operation, original, shape).unwrap();
            assert!((kernel.volume(combined).unwrap() - 120.).abs() < 1e-6);
        }
        kernel
            .export_step_named(
                &[original, shape],
                &["元".into(), "読戻し".into()],
                "native",
            )
            .expect("元の native の箱と読戻しを一緒に書く");
    }
    // DATA の部品名は、公開の読み込み操作を通して文字の読み手へ渡す。
    let needle = "PRODUCT('kieMonoBench','kieMonoBench'";
    assert!(raw.contains(needle));
    let decoder_fixture = raw.replacen(needle, &format!("PRODUCT({encoded},{encoded}"), 1);
    let (shape, name) = import_through_action(&modules, &decoder_fixture, original);
    assert_eq!(name, label);
    kernel
        .export_step_named(&[original, shape], &["元".into(), name], "native")
        .unwrap();
}
