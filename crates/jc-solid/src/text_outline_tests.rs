//! 輪郭のフォントの文字(K31)を押し出す試験。字の形は文書に入れた物だけを使う(計算はフォントを
//! 読まない)。フォントは jc-text の試験のフォント(OFL のフォントから数字ほどの字を抜いた物)。

use super::tests::{TestKernel, assert_ok, close, final_mass, registry};
use super::*;
use jc_core::{FeatureStatus, Profile};
use jc_samples::sample::DocBuilder;

fn jp_font(text: &str) -> jc_text::OutlineFont {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../jc-text/tests/fonts/jc-test-jp.ttf"
    ))
    .unwrap();
    let mut f = jc_text::OutlineFont::open(&bytes, 0).unwrap();
    assert!(f.add_glyphs(&bytes, 0, text).unwrap().is_empty());
    f
}

fn area(p: &Profile) -> f64 {
    p.regions
        .iter()
        .map(|r| r.outer.signed_area() + r.holes.iter().map(|h| h.signed_area()).sum::<f64>())
        .sum()
}

#[test]
fn japanese_outline_letters_are_extruded_with_their_holes() {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    let t = b.feature(
        jc_text::TEXT,
        "名入れ",
        jc_text::Text {
            text: "口回".into(),
            height: "10 mm".into(),
            basis: jc_text::HeightBasis::Em,
            outline: Some(jp_font("口回")),
            ..Default::default()
        },
    );
    b.feature(
        EXTRUDE,
        "浮かせる",
        Extrude {
            profile: Some(t),
            distance: "2 mm".into(),
            operation: Operation::New,
            ..Default::default()
        },
    );
    let mut doc = b.finish("名入れ");
    let reg = registry();
    let report = doc.recompute(&reg, &*k);
    assert_ok(&report);
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(t) else {
        unreachable!()
    };
    let p = Profile::from_value(&outputs["profile"]).unwrap();
    // 「口」は穴が 1 つの 1 つ、「回」は穴が 1 つずつの 2 つ
    let holes: Vec<usize> = p.regions.iter().map(|r| r.holes.len()).collect();
    assert_eq!(holes, vec![1, 1, 1], "{holes:?}");
    let a = area(&p);
    assert!(a > 10.0 && a < 100.0, "字の面積 {a}");
    let m = final_mass(&mut doc, &k);
    assert!(
        close(m.volume, a * 2.0, 2e-3),
        "{} vs {}",
        m.volume,
        a * 2.0
    );
}

#[test]
fn a_text_without_the_shape_of_a_new_letter_tells_it_and_keeps_the_rest() {
    let k = TestKernel::new();
    let mut b = DocBuilder::new();
    let t = b.feature(
        jc_text::TEXT,
        "名入れ",
        jc_text::Text {
            // 「名」の形は文書に無い(フォントの無い PC で字を足した時)
            text: "口名".into(),
            outline: Some(jp_font("口")),
            ..Default::default()
        },
    );
    let mut doc = b.finish("名入れ");
    let report = doc.recompute(&registry(), &*k);
    let Some(FeatureStatus::Ok { outputs, .. }) = report.feature(t) else {
        panic!("{:?}", report.feature(t))
    };
    let warnings = jc_core::warnings_of(outputs);
    let p = Profile::from_value(&outputs["profile"]).unwrap();
    assert_eq!(p.regions.len(), 1, "「口」は残る");
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("字「名」の形が文書に入っていません")),
        "{warnings:?}"
    );
}
