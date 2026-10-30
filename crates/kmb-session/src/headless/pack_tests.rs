//! 確かめること: 同梱した文書と書き出し、除外の理由、zipからの未保存の文書の復元。
use super::*;
use kmb_core::Document;

fn work(cam: bool) -> Headless {
    let make = if cam {
        kmb_bundle::Sample::Cam
    } else {
        kmb_bundle::Sample::Part
    };
    Headless::with_document(kmb_bundle::modules(), make.make()(), None)
}
fn now() -> DateTime<Utc> {
    "2026-10-30T12:00:00Z".parse().unwrap()
}
fn files(p: &Packed) -> std::collections::BTreeMap<String, Vec<u8>> {
    kmb_format::zip::read(&p.contents)
        .unwrap()
        .into_iter()
        .collect()
}
fn setup(h: &Headless) -> (String, FeatureId) {
    h.doc
        .data()
        .features
        .iter()
        .find(|f| {
            h.modules
                .registry
                .descriptor(&f.type_id)
                .is_some_and(|d| d.category == kmb_core::category::SETUP)
        })
        .map(|f| (f.type_id.clone(), f.id))
        .unwrap()
}

#[test]
fn packed_document_and_outputs_match_the_individual_exports() {
    let mut h = work(true);
    let (_, id) = setup(&h);
    let before = h.doc.to_json();
    let undo = h.doc.undo_depth();
    let gcode = h
        .action_for_role(&kmb_interchange::role::action::EXPORT_GCODE)
        .unwrap();
    let sheet = h
        .action_for_role(&kmb_interchange::role::action::SETUP_SHEET)
        .unwrap();
    let (_, gcode) = h.pack_file(&gcode, Some(id), &json!({}), None).unwrap();
    let (_, sheet) = h
        .pack_file(&sheet, Some(id), &json!({"format":"html"}), None)
        .unwrap();
    assert!(std::str::from_utf8(&sheet).unwrap().contains("<html"));
    let p = h.pack_at(PackOptions::default(), None, now()).unwrap();
    assert!(p.skipped.is_empty(), "{:?}", p.skipped);
    let fs = files(&p);
    let saved = fs
        .iter()
        .find(|(n, _)| n.ends_with(&format!(".{}", kmb_core::DOCUMENT_EXTENSION)))
        .unwrap()
        .1;
    assert_eq!(saved, before.as_bytes());
    assert_eq!(
        fs.iter().find(|(n, _)| n.starts_with("gcode/")).unwrap().1,
        &gcode
    );
    assert_eq!(
        fs.iter().find(|(n, _)| n.ends_with(".html")).unwrap().1,
        &sheet
    );
    assert!(
        std::str::from_utf8(&fs["README.txt"])
            .unwrap()
            .contains("2026-10-30T12:00:00Z")
    );
    assert_eq!(h.doc.to_json(), before);
    assert_eq!(h.doc.undo_depth(), undo);
    let twice = h.pack_at(PackOptions::default(), None, now()).unwrap();
    assert!(
        p.contents == twice.contents,
        "同じ日時と入力のzipが一致しません"
    );
}

#[test]
fn models_are_optional_and_no_setup_has_named_reasons() {
    let mut h = work(false);
    let p = h
        .pack_at(
            PackOptions {
                models: true,
                ..PackOptions::default()
            },
            None,
            now(),
        )
        .unwrap();
    let twice = h
        .pack_at(
            PackOptions {
                models: true,
                ..PackOptions::default()
            },
            None,
            now(),
        )
        .unwrap();
    assert!(
        p.contents == twice.contents,
        "同じ日時と入力のzipが一致しません"
    );
    assert_eq!(p.skipped.len(), 2);
    let fs = files(&p);
    let step = fs.iter().find(|(n, _)| n.starts_with("models/")).unwrap().1;
    assert!(
        std::str::from_utf8(step)
            .unwrap()
            .contains("'2026-10-30T12:00:00'")
    );
    assert!(
        fs.keys()
            .any(|n| n.starts_with("models/") && n.ends_with(".step")),
        "{:?}",
        p.skipped
    );
    assert!(
        std::str::from_utf8(&fs["README.txt"])
            .unwrap()
            .contains("セットアップがありません")
    );
    let p = h
        .pack_at(
            PackOptions {
                gcode: false,
                sheet: false,
                models: false,
            },
            None,
            now(),
        )
        .unwrap();
    assert!(p.skipped.is_empty());
    assert_eq!(p.included.len(), 2);
}

fn with_second_setup(broken: bool) -> Headless {
    let h = work(true);
    let (_, id) = setup(&h);
    let mut data: serde_json::Value = serde_json::from_str(&h.doc.to_json()).unwrap();
    let features = data["features"].as_array_mut().unwrap();
    let mut next = features
        .iter()
        .map(|f| f["id"].as_u64().unwrap())
        .max()
        .unwrap()
        + 100;
    let new_id = next;
    next += 1;
    let mut second = features.iter().find(|f| f["id"] == id.0).unwrap().clone();
    second["id"] = json!(new_id);
    second["name"] = json!("同名/板");
    features.iter_mut().find(|f| f["id"] == id.0).unwrap()["name"] = json!("同名/板");
    if broken {
        second["data"]["body"] = json!(999999);
    }
    let mut copied = vec![second];
    for f in features.iter().filter(|f| f["data"]["setup"] == id.0) {
        let mut f = f.clone();
        f["id"] = json!(next);
        next += 1;
        f["data"]["setup"] = json!(new_id);
        copied.push(f);
    }
    features.extend(copied);
    data["next_id"] = json!(next);
    let doc = Document::from_json(&data.to_string()).unwrap();
    Headless::with_document(kmb_bundle::modules(), doc, None)
}

#[test]
fn duplicate_setup_names_are_portable_and_both_sheets_remain() {
    let mut h = with_second_setup(false);
    let p = h.pack_at(PackOptions::default(), None, now()).unwrap();
    assert!(p.skipped.is_empty(), "{:?}", p.skipped);
    let fs = files(&p);
    let names: Vec<_> = fs.keys().filter(|n| n.starts_with("gcode/")).collect();
    assert_eq!(names.len(), 2);
    assert!(names.iter().any(|n| n.contains("(2)")));
    assert!(names.iter().all(|n| n.matches('/').count() == 1));
    let html =
        std::str::from_utf8(fs.iter().find(|(n, _)| n.ends_with(".html")).unwrap().1).unwrap();
    assert_eq!(html.matches("<iframe ").count(), 2);
}

#[test]
fn an_invalid_setup_is_reported_and_the_other_one_is_included() {
    let mut h = with_second_setup(true);
    assert!(h.report.error_count() > 0);
    let p = h.pack_at(PackOptions::default(), None, now()).unwrap();
    assert!(
        p.included.iter().any(|n| n.starts_with("gcode/")),
        "{:?}",
        p.skipped
    );
    assert!(
        p.skipped
            .iter()
            .any(|s| s.name.contains("同名/板") && !s.reason.is_empty())
    );
    let fs = files(&p);
    let readme = std::str::from_utf8(&fs["README.txt"]).unwrap();
    for s in &p.skipped {
        assert!(readme.contains(&s.reason));
    }
}

#[test]
fn a_zip_opens_as_an_unsaved_document_and_rejects_ambiguous_contents() {
    let mut h = work(false);
    let mut data: serde_json::Value = serde_json::from_str(&h.doc.to_json()).unwrap();
    data["sections"]["future.pack"] = json!({"version":1,"data":{"unknown":[1,2,3]}});
    let doc = Document::from_json(&data.to_string()).unwrap();
    h.replace(doc, None);
    let p = h
        .pack_at(
            PackOptions {
                gcode: false,
                sheet: false,
                models: false,
            },
            None,
            now(),
        )
        .unwrap();
    let dir = std::env::temp_dir().join(format!("kmb-pack-open-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("package.zip");
    crate::write_output(&path, &p.contents).unwrap();
    let opened = load_design(&path).unwrap();
    assert!(opened.path.is_none());
    assert_eq!(opened.document.to_json(), h.doc.to_json());
    let h2 = Headless::open(kmb_bundle::modules(), &path).unwrap();
    assert!(h2.path().is_none());
    assert_eq!(h2.doc.to_json(), h.doc.to_json());
    for bytes in [
        b"broken".to_vec(),
        kmb_format::zip::write(&[("README.txt", b"only text")]),
        kmb_format::zip::write(&[
            ("a.kmb", h.doc.to_json().as_bytes()),
            ("b.kmb", h.doc.to_json().as_bytes()),
        ]),
    ] {
        crate::write_output(&path, &bytes).unwrap();
        assert_eq!(load_design(&path).err().unwrap().kind, ProblemKind::Input);
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn portable_names_and_case_insensitive_collisions() {
    let mut used = BTreeSet::new();
    assert_eq!(unique("A.nc", &mut used), "A.nc");
    assert_eq!(unique("a.nc", &mut used), "a(2).nc");
    assert_eq!(file_name("../CON<>:"), ".._CON___");
    assert_eq!(file_name("CON.txt"), "_CON.txt");
}

#[test]
fn step_time_replacement_preserves_escaped_names_and_the_data() {
    for name in ["a,''b", "ENDSEC; a,''b"] {
        for description in [
            "",
            "FILE_DESCRIPTION(('FILE_NAME(''fake'',''fake'')'),'2;1');",
        ] {
            let s = format!(
                "ISO-10303-21;HEADER;{description}FILE_NAME('{name}','old',(''),(''),'writer','system','');ENDSEC;DATA;#1=X('old');ENDSEC;"
            );
            let got = String::from_utf8(
                step_created_at(s.as_bytes().to_vec(), "2000-01-01T00:00:00").unwrap(),
            )
            .unwrap();
            assert_eq!(got, s.replace("'old',('')", "'2000-01-01T00:00:00',('')"));
        }
    }
    assert!(step_created_at(b"DATA;".to_vec(), "2000-01-01T00:00:00").is_err());
}
