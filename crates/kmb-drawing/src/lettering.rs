//! 文字を共通の文字配置で線と輪郭へ直す。描く時は文書の字形だけを使う。

use crate::dimension::{add, scale};
use kmb_core::Segment;
use kmb_interchange::drawing::{Item, LineStyle, Path, Pen};
use kmb_text::{Font, OutlineFont, Style};

pub(super) type Bounds = ([f64; 2], [f64; 2]);

/// 文字を置く操作の時だけ呼ぶ。文字列の字と数字を埋め、描く時にはフォントを読まない。
pub(super) fn embed(
    index: &mut kmb_text::fonts::FontIndex,
    text: &str,
) -> Result<Option<serde_json::Value>, String> {
    if text
        .chars()
        .all(|c| c.is_ascii() || matches!(c, '°' | '±' | '⌀'))
    {
        return Ok(None);
    }
    let mut faces = index.faces().to_vec();
    faces.retain(|f| f.embeddable);
    faces.sort_by_key(|f| {
        (
            !f.style.eq_ignore_ascii_case("regular"),
            f.label.clone(),
            f.path.clone(),
            f.index,
        )
    });
    let wanted = text.to_string();
    for face in faces {
        let Ok(bytes) = std::fs::read(&face.path) else {
            continue;
        };
        if let Ok(font) = embedded(&bytes, face.index, &wanted) {
            return serde_json::to_value(font)
                .map(Some)
                .map_err(|e| e.to_string());
        }
    }
    Err(
        "必要な文字を文書に入れられる書体が見つかりません。書体を入れて注記を作り直してください"
            .into(),
    )
}

fn embedded(bytes: &[u8], index: u32, text: &str) -> Result<OutlineFont, String> {
    let mut font = OutlineFont::open(bytes, index)?;
    let absent = font.add_glyphs(bytes, index, text)?;
    if !absent.is_empty() {
        return Err("書体に必要な文字がありません".into());
    }
    Ok(font)
}

pub(super) fn stroke(owner: &str, line: LineStyle, path: Path) -> Item {
    Item::Stroke {
        pen: Pen::Thin,
        line,
        path,
        owner: owner.into(),
    }
}

fn segments(owner: &str, lines: &[Segment], items: &mut Vec<Item>) {
    for s in lines {
        let path = match s {
            Segment::Line { a, b } => Path::Line { a: *a, b: *b },
            Segment::Arc {
                center,
                radius,
                start,
                end,
            } => Path::Arc {
                c: *center,
                r: *radius,
                start_deg: start.to_degrees(),
                sweep_deg: (end - start).to_degrees(),
            },
            Segment::Circle { center, radius } => Path::Arc {
                c: *center,
                r: *radius,
                start_deg: 0.0,
                sweep_deg: 360.0,
            },
        };
        items.push(stroke(owner, LineStyle::Solid, path));
    }
}

pub(super) fn draw(
    owner: &str,
    text: &str,
    height: f64,
    at: [f64; 2],
    outline: Option<&serde_json::Value>,
    items: &mut Vec<Item>,
) -> Result<Bounds, String> {
    let font: Option<OutlineFont> = outline
        .map(|v| {
            serde_json::from_value(v.clone()).map_err(|e| format!("文書の字形を読めません: {e}"))
        })
        .transpose()?;
    let simple = text
        .chars()
        .all(|c| c.is_ascii() || matches!(c, '°' | '±' | '⌀'));
    let font = if simple {
        None
    } else {
        Some(
            font.as_ref()
                .ok_or("この文字の字形が文書にありません。注記を作り直してください")?,
        )
    };
    if let Some(font) = font
        && !font.missing(text).is_empty()
    {
        return Err("文書の字形に足りない文字があります。注記か部品表を作り直してください".into());
    }
    let style = Style::new(Font::Sans, height);
    let mut high = at;
    let mut low = at;
    for (row, line) in text.lines().enumerate() {
        let origin = [at[0], at[1] - row as f64 * height * 1.5];
        let mut x = origin[0];
        let mut spans: Vec<String> = Vec::new();
        if font.is_some() || line.is_ascii() {
            spans.push(line.to_string());
        } else {
            let mut ascii = String::new();
            for ch in line.chars() {
                if ch.is_ascii() {
                    ascii.push(ch);
                } else {
                    if !ascii.is_empty() {
                        spans.push(std::mem::take(&mut ascii));
                    }
                    spans.push(ch.to_string());
                }
            }
            if !ascii.is_empty() {
                spans.push(ascii);
            }
        }
        for span in spans {
            if span.is_empty() {
                continue;
            }
            let ch = span.chars().next().unwrap();
            let p = [x, origin[1]];
            let width;
            if simple && !ch.is_ascii() {
                width = height * 0.8;
                let a = |q| add(p, scale(q, height));
                match ch {
                    '±' => {
                        for (u, v) in [
                            ([0.1, 0.5], [0.7, 0.5]),
                            ([0.4, 0.2], [0.4, 0.8]),
                            ([0.1, 0.0], [0.7, 0.0]),
                        ] {
                            items.push(stroke(
                                owner,
                                LineStyle::Solid,
                                Path::Line { a: a(u), b: a(v) },
                            ));
                        }
                    }
                    '°' | '⌀' => {
                        let (c, r) = if ch == '°' {
                            (a([0.3, 0.8]), height * 0.15)
                        } else {
                            (a([0.4, 0.5]), height * 0.35)
                        };
                        items.push(stroke(
                            owner,
                            LineStyle::Solid,
                            Path::Arc {
                                c,
                                r,
                                start_deg: 0.0,
                                sweep_deg: 360.0,
                            },
                        ));
                        if ch == '⌀' {
                            items.push(stroke(
                                owner,
                                LineStyle::Solid,
                                Path::Line {
                                    a: a([0.0, 0.0]),
                                    b: a([0.8, 1.0]),
                                },
                            ));
                        }
                    }
                    _ => unreachable!(),
                }
            } else {
                let s = span;
                let laid = kmb_text::lay_out_with(&s, &style, font, p);
                for seg in &laid.frame {
                    for p in seg.polyline(4) {
                        for k in 0..2 {
                            low[k] = low[k].min(p[k]);
                            high[k] = high[k].max(p[k]);
                        }
                    }
                }
                if laid.missing != 0 {
                    return Err("文字を線にできません。文字か書体を選び直してください".into());
                }
                width = style
                    .fit_width_with(&s, 1.0, false, font)
                    .map_or(height * 0.5, |fit| style.width / fit.width);
                if font.is_some() {
                    let loops = laid
                        .rings
                        .iter()
                        .map(|ring| {
                            let mut points = Vec::new();
                            for (seg, back) in ring {
                                let mut part = seg.polyline(128);
                                if *back {
                                    part.reverse();
                                }
                                if points.last() == part.first() {
                                    part.remove(0);
                                }
                                points.extend(part);
                            }
                            points
                        })
                        .filter(|r| r.len() >= 3)
                        .collect::<Vec<_>>();
                    if !loops.is_empty() {
                        items.push(Item::Fill {
                            loops,
                            owner: owner.into(),
                        });
                    }
                } else {
                    segments(owner, &laid.segments, items);
                }
            }
            x += width;
        }
        high[0] = high[0].max(x);
        high[1] = high[1].max(origin[1] + height);
        low[1] = low[1].min(origin[1]);
    }
    Ok((low, high))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn japanese_glyphs_reopen_and_render_without_a_font_file() {
        let bytes = include_bytes!("../../kmb-text/tests/fonts/kmb-test-jp.ttf");
        let font = embedded(bytes, 0, "自作回の名入れ口8").unwrap();
        let json = serde_json::to_value(font).unwrap();
        let saved = serde_json::to_vec(&json).unwrap();
        let restored: serde_json::Value = serde_json::from_slice(&saved).unwrap();
        let mut a = Vec::new();
        let mut b = Vec::new();
        draw(
            "n",
            "自作回の名入れ口8",
            3.5,
            [10.0, 20.0],
            Some(&json),
            &mut a,
        )
        .unwrap();
        draw(
            "n",
            "自作回の名入れ口8",
            3.5,
            [10.0, 20.0],
            Some(&restored),
            &mut b,
        )
        .unwrap();
        assert_eq!(a, b);
        assert!(!a.is_empty());
        assert!(a.iter().all(|i| matches!(i, Item::Fill { .. })));
    }
    #[test]
    fn ascii_and_dimension_symbols_are_strokes_without_a_font() {
        let mut out = Vec::new();
        let b = draw("n", "R20 ±0.1\n90° ⌀6", 3.5, [10.0, 20.0], None, &mut out).unwrap();
        assert!(out.len() > 10);
        assert!(out.iter().all(|i| matches!(i, Item::Stroke { .. })));
        assert!(b.1[0] > b.0[0] && b.0[1] < 20.0);
        assert!(draw("n", "日本語", 3.5, [0.0; 2], None, &mut Vec::new()).is_err());
    }
}
