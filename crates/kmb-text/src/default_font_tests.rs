//! 確かめること: 新しい配置は既定のゴシックを使い、書体が無い時と前の文書は1本線のまま。
use super::*;
use crate::fonts::FontIndex;
use crate::test_fonts::GothicFonts;
use kmb_core::{
    Document, FeatureEntry, FeatureId, FeatureResult, FeatureStatus, Kernel, Op, Registry,
    Transaction, Value,
};
use std::sync::Arc;

fn source() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fonts/kmb-test-jp.ttf")
}

fn commands(faces: Vec<crate::fonts::Face>) -> TextCommands {
    TextCommands {
        fonts: std::sync::Mutex::new(FontIndex::for_test(faces)),
    }
}

fn inserted(outcome: Outcome) -> (String, Text) {
    let Outcome::Change { label, edits } = outcome else {
        panic!("追加する変更")
    };
    let [Edit::Insert { type_id, data, .. }] = &edits[..] else {
        panic!("1つの文字を追加")
    };
    assert_eq!(type_id, TEXT);
    (label, serde_json::from_value(data.clone()).unwrap())
}

#[derive(Debug)]
struct PlaneOnly;
impl Kernel for PlaneOnly {
    fn name(&self) -> &str {
        "plane-only"
    }
    fn faces(
        &self,
        _: kmb_core::ShapeHandle,
    ) -> Result<Vec<kmb_core::FaceInfo>, kmb_core::KernelError> {
        Ok(vec![kmb_core::FaceInfo {
            name: "front".into(),
            planar: Some(kmb_core::PlanarFace {
                point: [0.0; 3],
                normal: [0.0, 0.0, 1.0],
            }),
            area: 1.0,
        }])
    }
}

#[test]
fn all_three_text_placement_actions_use_the_gothic_face() {
    let fonts = GothicFonts::new(&source());
    let mut index = FontIndex::default();
    index.add_dir(fonts.path());
    let face = index.find("IPA Gothic", "Regular").unwrap();
    let placing = commands(vec![face.clone()]);
    let mut reg = Registry::new();
    kmb_sketch::register(&mut reg);
    let mut sketch = Sketch::default();
    sketch.add_point(0.0, 0.0);
    let a = sketch.add_point(10.0, 0.0);
    let b = sketch.add_point(30.0, 0.0);
    sketch.add_line(a, b);
    let mut doc = Document::new();
    let sketch_id = doc.alloc_feature_id();
    doc.apply(Transaction::new(
        "基準のスケッチ",
        vec![Op::InsertFeature {
            index: None,
            entry: FeatureEntry {
                id: sketch_id,
                type_id: SKETCH.into(),
                name: "基準".into(),
                data: serde_json::to_value(sketch).unwrap(),
                suppressed: false,
                pin_bodies: false,
            },
        }],
    ))
    .unwrap();
    let kernel: Arc<dyn Kernel> = Arc::new(PlaneOnly);
    let mut report = doc.recompute(&reg, kernel.as_ref());
    report.features.push(FeatureResult {
        id: FeatureId(9),
        name: "平らな面".into(),
        status: FeatureStatus::Ok {
            outputs: [("body".into(), Value::Shape(kmb_core::ShapeHandle(9)))].into(),
            reused: false,
        },
    });
    let pick = kmb_core::FacePick {
        feature: FeatureId(9),
        face: "front".into(),
    };
    let cx = Context {
        selected: Some(sketch_id),
        picked: Some(&pick),
        ..Context::new(doc.data(), &report, &reg, &kernel)
    };
    for action in [TEXT_ON_FACE, TEXT_AT_POINT, TEXT_ON_PATH] {
        let (fallback, plain) = inserted(commands(vec![]).run(action, &cx).unwrap().unwrap());
        assert!(plain.outline.is_none());
        assert!(
            fallback.contains("ゴシック体が見つからないので 1 本線にしました"),
            "{action}: {fallback}"
        );
        let (_, t) = inserted(placing.run(action, &cx).unwrap().unwrap());
        let font = t.outline.expect("新しい配置は輪郭のフォント");
        assert_eq!(font.family, face.family, "{action}");
        assert_eq!(font.style, "Regular");
        assert!(font.missing(&t.text).is_empty(), "{action}");
        assert!(font.glyphs.values().all(|g| !g.path.is_empty()), "{action}");
        assert_eq!(font.glyphs.len(), 3, "使うT・E・Xの字形だけを入れる");
    }
}

#[test]
fn new_japanese_glyphs_are_embedded_and_reload_without_the_font_file() {
    let fonts = GothicFonts::new(&source());
    let mut index = FontIndex::default();
    index.add_dir(fonts.path());
    let face = index.find("IPA Gothic", "Regular").unwrap();
    let commands = commands(vec![face]);
    let doc = Document::new();
    let report = kmb_core::RecomputeReport::default();
    let reg = Registry::new();
    let kernel: Arc<dyn Kernel> = Arc::new(kmb_core::NullKernel);
    let cx = Context::new(doc.data(), &report, &reg, &kernel);
    let (label, text) = inserted(
        commands
            .insert_text(
                &cx,
                "文字を追加",
                Text {
                    text: "自作".into(),
                    ..Default::default()
                },
            )
            .unwrap(),
    );
    assert_eq!(label, "文字を追加");
    let font = text.outline.as_ref().unwrap();
    assert_eq!(font.glyphs.len(), 2, "使う字だけを持つ");
    assert!(!font.glyph('自').unwrap().path.is_empty());
    let mut saved = Document::new();
    let id = saved.alloc_feature_id();
    saved
        .apply(Transaction::new(
            "日本語の文字",
            vec![Op::InsertFeature {
                index: None,
                entry: FeatureEntry {
                    id,
                    type_id: TEXT.into(),
                    name: "名入れ".into(),
                    data: serde_json::to_value(&text).unwrap(),
                    suppressed: false,
                    pin_bodies: false,
                },
            }],
        ))
        .unwrap();
    let mut registry = Registry::new();
    crate::register(&mut registry);
    let before = saved.recompute(&registry, kernel.as_ref());
    assert_eq!(before.error_count(), 0);
    let bytes = saved.to_json();
    drop(fonts);
    let mut loaded = Document::from_json(&bytes).unwrap();
    assert_eq!(loaded.data(), saved.data());
    let back: Text =
        serde_json::from_value(loaded.data().feature(id).unwrap().data.clone()).unwrap();
    assert_eq!(back, text);
    let mut filled = back.clone();
    assert_eq!(
        refresh_glyphs(&mut filled, &mut FontIndex::for_test(vec![])).unwrap(),
        None
    );
    assert_eq!(
        filled, back,
        "字形が埋め込まれていればフォントの無い一覧を読まない"
    );
    let after = loaded.recompute(&registry, kernel.as_ref());
    assert_eq!(after.error_count(), 0);
    let curves = |report: &kmb_core::RecomputeReport| {
        let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(id) else {
            panic!("保存した文字の輪郭")
        };
        Curves::from_value(&outputs["curves"]).unwrap()
    };
    let a = curves(&before);
    let b = curves(&after);
    let profile = |report: &kmb_core::RecomputeReport| {
        let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(id) else {
            panic!("文字の輪郭")
        };
        kmb_core::Profile::from_value(&outputs["profile"]).unwrap()
    };
    assert!(!profile(&before).regions.is_empty());
    assert_eq!(profile(&before), profile(&after));
    assert!(!a.segments.is_empty());
    assert_eq!(a, b);
}

#[test]
fn missing_gothic_keeps_single_lines_and_names_the_fallback() {
    let commands = commands(vec![]);
    let doc = Document::new();
    let report = kmb_core::RecomputeReport::default();
    let reg = Registry::new();
    let kernel: Arc<dyn Kernel> = Arc::new(kmb_core::NullKernel);
    let cx = Context::new(doc.data(), &report, &reg, &kernel);
    let (label, text) = inserted(
        commands
            .insert_text(&cx, "文字を追加", Text::default())
            .unwrap(),
    );
    assert!(label.contains("ゴシック体が見つからないので 1 本線にしました"));
    assert!(text.outline.is_none());
    assert_eq!(text, Text::default());
}

#[test]
fn old_text_without_a_font_keeps_the_same_single_line_geometry() {
    let before = serde_json::json!({"text":"A8", "height":"5 mm", "x":"0 mm", "y":"0 mm"});
    let text: Text = serde_json::from_value(before).unwrap();
    assert!(text.outline.is_none());
    let saved = serde_json::to_string(&text).unwrap();
    assert!(!saved.contains("outline"));
    let back: Text = serde_json::from_str(&saved).unwrap();
    let style = crate::Style::new(crate::Font::Sans, 5.0);
    let a = crate::lay_out_with(&text.text, &style, text.outline.as_ref(), [0.0; 2]);
    let b = crate::lay_out_with(&back.text, &style, back.outline.as_ref(), [0.0; 2]);
    assert_eq!(a.segments, b.segments);
    assert_eq!(a.segments, crate::lay_out("A8", &style, [0.0; 2]).segments);
    assert_eq!(text.font, crate::Font::Sans);
    assert!(!b.segments.is_empty());
    assert!(Text::default().outline.is_none());
    let mut registry = Registry::new();
    crate::register(&mut registry);
    let descriptor = registry.descriptor(TEXT).unwrap();
    let template = descriptor.template.as_ref().unwrap();
    assert!(template.get("outline").is_none());
}
