//! 確かめること: 用紙の枠、表題の字形と保存、三つの書き出しの実寸と再現性。
use super::*;
use kmb_core::Outcome;
use serde_json::json;

fn run(doc: &mut Document, id: &str, args: serde_json::Value) -> Outcome {
    let m = modules();
    let report = doc.recompute(&m.registry, m.kernel.as_ref());
    assert_eq!(report.error_count(), 0, "{report:?}");
    let mut cx = Context::new(doc.data(), &report, &m.registry, &m.kernel);
    cx.args = &args;
    m.extensions
        .run(id, &cx)
        .unwrap_or_else(|e| panic!("{id}: {e}"))
}
fn change(doc: &mut Document, id: &str, args: serde_json::Value) {
    let Outcome::Change { label, edits } = run(doc, id, args) else {
        panic!("変更を期待")
    };
    doc.apply_edits(&label, edits).unwrap();
}
fn file(doc: &mut Document, id: &str, args: serde_json::Value) -> Vec<u8> {
    let Outcome::File { contents, info, .. } = run(doc, id, args) else {
        panic!("ファイルを期待")
    };
    assert!(info["problems"].as_array().unwrap().is_empty(), "{info}");
    contents
}
#[test]
fn paper_frames_follow_format_orientation_binding_and_preserve_unknown_fields() {
    let mut doc = crate::drawing_box_document();
    edit(&mut doc, |d| {
        d.sheets[0].views.truncate(1);
        d.sheets[0]
            .paper
            .rest
            .insert("future".into(), json!({"keep":true}));
    });
    change(
        &mut doc,
        "drawing.set_paper",
        json!({"sheet":"sheet","paper":{"format":{"name":"a4"},"portrait":true,"binding":true}}),
    );
    let d: Drawings = doc.data().section().unwrap();
    assert_eq!(d.sheets[0].paper.size(), [210.0, 297.0]);
    assert_eq!(d.sheets[0].paper.rest["future"], json!({"keep":true}));
    let p = picture(&mut doc);
    assert!(p.problems.is_empty(), "{:?}", p.problems);
    assert!(p.items.iter().any(|i|matches!(i,Item::Stroke {owner,pen:kmb_interchange::drawing::Pen::ExtraThick,path:Path::Poly {points},..} if owner=="sheet" && points==&vec![[20.0,10.0],[200.0,10.0],[200.0,287.0],[20.0,287.0],[20.0,10.0]])));
    change(
        &mut doc,
        "drawing.set_paper",
        json!({"sheet":"sheet","paper":{"format":{"name":"a3"},"portrait":false}}),
    );
    assert_eq!(picture(&mut doc).size, [420.0, 297.0]);
    doc.undo();
    assert_eq!(picture(&mut doc).size, [210.0, 297.0]);
}
#[test]
fn svg_and_dxf_read_back_the_same_sheet_lines_and_pdf_has_all_pages() {
    let mut doc = crate::drawing_box_document();
    edit(&mut doc, |d| d.sheets[0].views.truncate(1));
    let svg = file(&mut doc, "drawing.export_svg", json!({"sheet":"sheet"}));
    let dxf = file(&mut doc, "drawing.export_dxf", json!({"sheet":"sheet"}));
    let a = kmb_io_svg::from_svg(std::str::from_utf8(&svg).unwrap()).unwrap();
    let b = kmb_io_dxf::from_dxf(std::str::from_utf8(&dxf).unwrap()).unwrap();
    assert_eq!(a.shapes.len(), b.shapes.len());
    assert!(a.shapes.len() > 30);
    assert!(
        std::str::from_utf8(&svg)
            .unwrap()
            .contains("width=\"297mm\" height=\"210mm\"")
    );
    assert!(std::str::from_utf8(&dxf).unwrap().contains("CENTER"));
    assert!(std::str::from_utf8(&dxf).unwrap().contains("PHANTOM"));
    assert_eq!(
        svg,
        file(&mut doc, "drawing.export_svg", json!({"sheet":"sheet"}))
    );
    assert_eq!(
        dxf,
        file(&mut doc, "drawing.export_dxf", json!({"sheet":"sheet"}))
    );
    change(
        &mut doc,
        "drawing.add_sheet",
        json!({"id":"second","name":"2","paper":{"format":{"name":"a4"},"portrait":true}}),
    );
    let pdf = file(&mut doc, "drawing.export_pdf", json!({"date":"2026-10-30"}));
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.contains("/Count 2"));
    assert!(text.contains("/CreationDate (D:20261030000000Z)"));
    assert_eq!(
        pdf,
        file(&mut doc, "drawing.export_pdf", json!({"date":"2026-10-30"}))
    );
    let no_date = file(&mut doc, "drawing.export_pdf", json!({"sheet":"second"}));
    let text = String::from_utf8_lossy(&no_date);
    assert!(text.contains("/Count 1"));
    assert!(!text.contains("CreationDate"));
    assert!(text.contains("/MediaBox [0 0 595.275591 841.889764]"));
}
#[test]
fn japanese_title_embeds_only_its_used_glyphs_and_reopens_without_the_font() {
    let mut doc = crate::drawing_box_document();
    edit(&mut doc, |d| d.sheets[0].views.truncate(1));
    let font = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../kmb-text/tests/fonts/kmb-test-jp.ttf");
    change(
        &mut doc,
        "drawing.set_title",
        json!({"sheet":"sheet","fields":{"title":"自作回8","number":"A-01"},"font":font}),
    );
    let d: Drawings = doc.data().section().unwrap();
    let title: &serde_json::Value = &d.sheets[0].title;
    assert!(title["outline"]["glyphs"]["自"].is_object());
    assert!(title["outline"].get("path").is_none());
    let p = picture(&mut doc);
    assert!(p.problems.is_empty(), "{:?}", p.problems);
    assert!(
        p.items
            .iter()
            .any(|i| matches!(i,Item::Fill {owner,..} if owner=="sheet:title"))
    );
    let bytes = file(&mut doc, "drawing.export_pdf", json!({"sheet":"sheet"}));
    assert!(!String::from_utf8_lossy(&bytes).contains("/Font"));
    let before = serde_json::to_value(picture(&mut doc)).unwrap();
    let mut reopened = Document::from_json(&doc.to_json()).unwrap();
    assert_eq!(
        before,
        serde_json::to_value(picture(&mut reopened)).unwrap()
    );
}
