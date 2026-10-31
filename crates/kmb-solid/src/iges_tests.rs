//! 確かめること: 読み込みの共通の操作、許可範囲、文書の埋め込みと取り消し。
use crate::tests::{TestKernel, assert_ok, registry};
use kmb_cad_import_step::{IMPORT_IGES, IMPORT_IGES_FILE, ImportIges, ImportIgesCommands};
use kmb_core::{
    Context, Document, Extensions, FeatureId, FeatureStatus, Kernel, KernelError, Outcome, Value,
};
use kmb_samples::sample::DocBuilder;
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

fn fixture() -> String {
    let parameter =
        "128,1,1,1,1,0,0,1,0,0,0,0,1,1,0,0,1,1,1,1,1,1,0,0,0,10,0,0,0,10,0,10,10,0,0,1,0,1;";
    let mut g = vec![String::new(); 20];
    g[0] = "1H,".into();
    g[1] = "1H;".into();
    g[2] = "12HkieMonoBench".into();
    g[12] = "1".into();
    g[13] = "2".into();
    g[14] = "2HMM".into();
    g[18] = "1D-6".into();
    let rec = |s: &str, c: char, n: usize| format!("{s:<72}{c}{n:7}");
    let g = (g.join(",") + ";")
        .as_bytes()
        .chunks(72)
        .enumerate()
        .map(|(i, b)| rec(std::str::from_utf8(b).unwrap(), 'G', i + 1))
        .collect::<Vec<_>>();
    let p = parameter
        .as_bytes()
        .chunks(64)
        .enumerate()
        .map(|(i, b)| format!("{:<64}{:8}P{:7}", std::str::from_utf8(b).unwrap(), 1, i + 1))
        .collect::<Vec<_>>();
    let d = [
        rec(
            &format!(
                "{:8}{:8}{:8}{:8}{:8}{:8}{:8}{:8}00000000",
                128, 1, 0, 0, 0, 0, 0, 0
            ),
            'D',
            1,
        ),
        rec(
            &format!(
                "{:8}{:8}{:8}{:8}{:8}{:8}{:8}{:8}{:8}",
                128,
                0,
                0,
                p.len(),
                0,
                0,
                0,
                0,
                0
            ),
            'D',
            2,
        ),
    ];
    let t = rec(
        &format!("S{:7}G{:7}D{:7}P{:7}", 1, g.len(), 2, p.len()),
        'T',
        1,
    );
    std::iter::once(rec("test", 'S', 1))
        .chain(g)
        .chain(d)
        .chain(p)
        .chain([t])
        .collect::<Vec<_>>()
        .join("\n")
}
fn shape(report: &kmb_core::RecomputeReport, id: FeatureId) -> kmb_core::ShapeHandle {
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(id) else {
        panic!("{:?}", report.feature(id))
    };
    let Value::Shape(h) = outputs["body"] else {
        panic!("body が形ではない")
    };
    h
}
#[test]
fn embedded_iges_reopens_without_the_source_and_can_be_stood_up_and_undone() {
    let k = TestKernel::new();
    if k.is_truck() {
        assert!(matches!(
            k.import_iges(&fixture()),
            Err(KernelError::Unsupported(_))
        ));
    } else {
        let data = ImportIges {
            file: "surface.igs".into(),
            iges: fixture(),
            stand_up: false,
            upright: false,
        };
        let mut b = DocBuilder::new();
        let id = b.feature(IMPORT_IGES, "面", &data);
        let mut doc = b.finish("IGES");
        let reg = registry();
        let report = doc.recompute(&reg, &*k);
        assert_ok(&report);
        let h = shape(&report, id);
        let names = k.face_names(h).unwrap();
        assert_eq!(k.body_kind(h).unwrap(), kmb_core::kernel::BodyKind::Sheet);
        let saved = doc.to_json();
        let mut restored = Document::from_json(&saved).unwrap();
        let other = TestKernel::new();
        let report = restored.recompute(&reg, &*other);
        assert_ok(&report);
        assert_eq!(names, other.face_names(shape(&report, id)).unwrap());
        let mut rotated = serde_json::to_value(&data).unwrap();
        rotated["stand_up"] = json!(true);
        doc.apply(kmb_core::Transaction::new(
            "立てる",
            vec![kmb_core::Op::SetFeatureData { id, data: rotated }],
        ))
        .unwrap();
        let report = doc.recompute(&reg, &*k);
        assert_ok(&report);
        let bounds = k.bounds(shape(&report, id)).unwrap();
        assert!(
            (bounds.1[2] - 10.).abs() < 1e-5 && bounds.1[1].abs() < 1e-5,
            "{bounds:?}"
        );
        assert!(doc.undo());
        let report = doc.recompute(&reg, &*k);
        assert_ok(&report);
        assert_eq!(
            doc.data().feature(id).unwrap().data,
            serde_json::to_value(&data).unwrap()
        );
        assert_eq!(k.face_names(shape(&report, id)).unwrap(), names);
    }
}
#[derive(Debug)]
struct Deny;
impl kmb_core::FileGate for Deny {
    fn open(&self, _: &Path) -> Result<PathBuf, String> {
        Err("許したフォルダの外です".into())
    }
}
#[test]
fn the_import_action_checks_file_arguments_and_does_not_reopen_embedded_data() {
    let reg = registry();
    let kernel: Arc<dyn Kernel> = if std::env::var("JC_KERNEL").as_deref() == Ok("truck") {
        Arc::new(kmb_kernel_truck::TruckKernel::new())
    } else {
        Arc::new(kmb_kernel_smlib::SmlibKernel::new())
    };
    let mut doc = Document::new();
    let report = doc.recompute(&reg, kernel.as_ref());
    let extensions = Extensions(vec![Arc::new(ImportIgesCommands)]);
    let dir = std::env::temp_dir().join(format!("iges-action-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("面.IGS");
    std::fs::write(&path, fixture()).unwrap();
    let arguments = json!({"path":path});
    let cx = Context {
        args: &arguments,
        ..Context::new(doc.data(), &report, &reg, &kernel)
    };
    let data = ImportIges::from_file(&path).unwrap();
    assert!(data.upright);
    let outcome = extensions.run(IMPORT_IGES_FILE, &cx).unwrap();
    assert!(
        extensions
            .run(
                IMPORT_IGES_FILE,
                &Context {
                    files: Some(&Deny),
                    ..cx
                }
            )
            .unwrap_err()
            .message
            .contains("フォルダの外")
    );
    assert!(
        extensions
            .run(
                IMPORT_IGES_FILE,
                &Context {
                    args: &json!({"unknown":1}),
                    ..cx
                }
            )
            .is_err()
    );
    let Outcome::Change { label, edits } = outcome else {
        panic!("文書の変更ではない")
    };
    let ids = doc.apply_edits(&label, edits).unwrap();
    assert_eq!(ids.len(), 1);
    let serialized = doc.to_json();
    assert!(serialized.contains("128,1,1"));
    std::fs::remove_dir_all(&dir).unwrap();
    let report = doc.recompute(&reg, kernel.as_ref());
    if kernel.name() == "truck" {
        assert!(matches!(
            report.feature(ids[0]),
            Some(FeatureStatus::Error { .. })
        ));
    } else {
        assert_ok(&report);
        assert_eq!(kernel.faces(shape(&report, ids[0])).unwrap().len(), 1);
    }
    assert!(doc.undo());
    assert!(doc.data().features.is_empty());
}
