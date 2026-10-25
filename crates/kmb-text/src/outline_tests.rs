//! 輪郭のフォントの文字(K31)の試験。フォントは tests/fonts の小さな物(OFL のフォントから数字ほどの
//! 字だけを抜いた。tests/fonts/README.md)。

use super::*;
use crate::fonts::FontIndex;
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fonts")
}

fn jp() -> Vec<u8> {
    std::fs::read(dir().join("kmb-test-jp.ttf")).unwrap()
}

fn cff() -> Vec<u8> {
    std::fs::read(dir().join("kmb-test-cff.otf")).unwrap()
}

/// フォント `bytes` で `text` の字を入れた物。
fn font(bytes: &[u8], text: &str) -> OutlineFont {
    let mut f = OutlineFont::open(bytes, 0).unwrap();
    let absent = f.add_glyphs(bytes, 0, text).unwrap();
    assert!(absent.is_empty(), "フォントに無い字: {absent:?}");
    f
}

fn regions_of(text: &str, f: &OutlineFont, style: &Style) -> Vec<kmb_core::Region> {
    glyph::regions(lay_out_with(text, style, Some(f), [0.0, 0.0]).rings)
}

fn area(r: &kmb_core::Region) -> f64 {
    r.outer.signed_area() + r.holes.iter().map(|h| h.signed_area()).sum::<f64>()
}

fn bounds(segs: &[Segment]) -> ([f64; 2], [f64; 2]) {
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for s in segs {
        let mut pts = s.polyline(256);
        pts.push(s.end_point());
        for p in pts {
            for i in 0..2 {
                lo[i] = lo[i].min(p[i]);
                hi[i] = hi[i].max(p[i]);
            }
        }
    }
    (lo, hi)
}

#[test]
fn the_index_finds_the_test_fonts_by_name() {
    let mut idx = FontIndex::default();
    idx.add_dir(&dir());
    let jp = idx.find("JC Test JP", "").expect("試験のフォント");
    assert_eq!(jp.style, "Regular");
    assert!(jp.embeddable);
    assert!(idx.find("JC Test CFF", "Regular").is_some());
    assert!(idx.find("JC Test CFF", "Bold").is_none());
    assert!(idx.find("無いフォント", "").is_none());
}

#[test]
fn kanji_with_nested_rings_become_islands_and_holes() {
    let f = font(&jp(), "回口の");
    let style = Style::new(Font::Sans, 10.0);
    // 「回」は外・穴・中の島・中の穴の 4 重 → 穴の 1 つある領域が 2 つ
    let r = regions_of("回", &f, &style);
    assert_eq!(r.len(), 2, "{r:?}");
    assert!(r.iter().all(|x| x.holes.len() == 1));
    for x in &r {
        assert!(x.outer.signed_area() > 0.0, "外は反時計回り");
        assert!(x.holes[0].signed_area() < 0.0, "穴は時計回り");
        assert!(area(x) > 0.0);
    }
    // 「口」は穴が 1 つ
    let r = regions_of("口", &f, &style);
    assert_eq!((r.len(), r[0].holes.len()), (1, 1));
    // 「の」は 1 本の輪が自分と交わる(払いが輪の中を通る)。塗りでまとめ、内の空きが穴になる
    let r = regions_of("の", &f, &style);
    assert_eq!((r.len(), r[0].holes.len()), (1, 1));
    assert!(area(&r[0]) > 0.0);
}

#[test]
fn the_height_follows_the_capital_or_the_em_box() {
    let f = font(&cff(), "AO");
    let mut style = Style::new(Font::Sans, 10.0);
    let laid = lay_out_with("A", &style, Some(&f), [0.0, 0.0]);
    let (lo, hi) = bounds(&laid.segments);
    assert!(
        (lo[1]).abs() < 0.05 && (hi[1] - 10.0).abs() < 0.05,
        "A は基準線から大文字の高さまで: {lo:?} {hi:?}"
    );
    // 字の枠: 1 em を 10 mm にすると、大文字はそれより低い
    style.basis = HeightBasis::Em;
    let laid = lay_out_with("A", &style, Some(&f), [0.0, 0.0]);
    let (_, hi) = bounds(&laid.segments);
    let cap = f.cap_height / f.units_per_em * 10.0;
    assert!((hi[1] - cap).abs() < 0.05, "{hi:?} {cap}");
    // CFF の字は 3 次のベジエのまま入る
    assert!(f.glyph('O').unwrap().path.contains('C'));
    let r = regions_of("O", &f, &Style::new(Font::Sans, 10.0));
    assert_eq!((r.len(), r[0].holes.len()), (1, 1));
}

#[test]
fn the_glyphs_go_into_the_document_and_come_back_the_same() {
    let f = font(&jp(), "自作");
    let t = Text {
        text: "自作".into(),
        outline: Some(f),
        basis: HeightBasis::Em,
        ..Default::default()
    };
    let v = serde_json::to_value(&t).unwrap();
    let back: Text = serde_json::from_value(v.clone()).unwrap();
    assert_eq!(back, t);
    assert_eq!(v["basis"], "em");
    // 1 本線の文字は前と同じ形のまま(欄を足さない)
    let old = serde_json::to_value(Text::default()).unwrap();
    assert!(old.get("outline").is_none() && old.get("basis").is_none());
}

#[test]
fn characters_not_in_the_document_or_the_font_are_told() {
    let mut f = OutlineFont::open(&jp(), 0).unwrap();
    let absent = f.add_glyphs(&jp(), 0, "自作X").unwrap();
    assert_eq!(absent, vec!['X'], "試験のフォントに X は無い");
    assert_eq!(f.missing("自作名X"), vec!['X', '名']);
    let laid = lay_out_with("自X作", &Style::new(Font::Sans, 5.0), Some(&f), [0.0, 0.0]);
    assert_eq!(laid.missing, 1);
}

#[test]
fn a_font_that_forbids_embedding_is_refused() {
    let mut b = jp();
    // OS/2 の fsType(表の頭から 8 バイト目)を「制限つき」にする
    let n = u16::from_be_bytes([b[4], b[5]]) as usize;
    let rec = (0..n)
        .map(|i| 12 + 16 * i)
        .find(|&r| &b[r..r + 4] == b"OS/2")
        .unwrap();
    let at = u32::from_be_bytes(b[rec + 8..rec + 12].try_into().unwrap()) as usize;
    b[at + 8..at + 10].copy_from_slice(&0x0002u16.to_be_bytes());
    let e = OutlineFont::open(&b, 0).unwrap_err();
    assert!(e.contains("許していません"), "{e}");
}

#[test]
fn overlapping_rings_are_filled_into_one_region() {
    // 重なる 2 つの四角(可変フォントの字のよう): 1 つの領域になり、面積は和
    let mut f = OutlineFont::open(&jp(), 0).unwrap();
    f.glyphs.insert(
        "□".into(),
        OutlineGlyph {
            advance: 200.0,
            path: "M 0 0 L 100 0 L 100 100 L 0 100 Z M 50 50 L 150 50 L 150 150 L 50 150 Z".into(),
        },
    );
    let mut style = Style::new(Font::Sans, 1.0);
    style.basis = HeightBasis::Em;
    let r = regions_of("□", &f, &style);
    assert_eq!(r.len(), 1, "{r:?}");
    let k = 1.0 / f.units_per_em;
    let want = (2.0 * 100.0 * 100.0 - 50.0 * 50.0) * k * k;
    assert!(
        (area(&r[0]) - want).abs() < want * 1e-6,
        "{} {want}",
        area(&r[0])
    );
}

#[test]
fn outline_letters_follow_a_circle() {
    let f = font(&jp(), "回");
    let path = Path::new(
        Segment::Circle {
            center: [0.0, 0.0],
            radius: 20.0,
        },
        true,
    );
    let laid = lay_along_with(
        "回回",
        &Style::new(Font::Sans, 4.0),
        Some(&f),
        &path,
        [0.0, 0.0],
    )
    .unwrap();
    let r = glyph::regions(laid.rings);
    assert_eq!(r.len(), 4, "字ごとに 2 つ");
    // どの点も円のまわり(半径 20 mm から字の高さほど)
    let (lo, hi) = bounds(&laid.segments);
    assert!(hi[1] > 18.0 && lo[1] > 10.0, "{lo:?} {hi:?}");
}

#[test]
fn the_width_can_be_fitted_with_an_outline_font() {
    let f = font(&cff(), "AO");
    let style = Style::new(Font::Sans, 10.0);
    let fitted = style.fit_width_with("AO", 30.0, false, Some(&f)).unwrap();
    let laid = lay_out_with("AO", &fitted, Some(&f), [0.0, 0.0]);
    let w: f64 = "AO"
        .chars()
        .map(|c| f.glyph(c).unwrap().advance)
        .sum::<f64>()
        * fitted.width
        * fitted.height
        / f.cap_height;
    assert!((w - 30.0).abs() < 1e-9, "{w}");
    assert!(!laid.segments.is_empty());
}
