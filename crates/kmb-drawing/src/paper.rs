//! 用紙の枠と表題欄。文字の輪郭は保存した物だけを使い、日付は呼ぶ側から受ける。

use crate::*;
use kmb_core::{Context, Problem, ProblemKind, Segment};
use kmb_interchange::drawing::{DrawingProblem, Item, LineStyle, Path, Pen, SheetPicture};
use kmb_text::{Font, OutlineFont, Style};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TitleValue {
    Text(String),
    Source {
        source: TitleSource,
        #[serde(flatten)]
        rest: Rest,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TitleSource {
    Document,
    PartNumber,
    Scale,
    Projection,
    Date,
    Designer,
    Material,
    Sheet,
    Unit,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Title {
    #[serde(default)]
    pub fields: BTreeMap<String, TitleValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline: Option<OutlineFont>,
    #[serde(flatten)]
    pub rest: Rest,
}
const FIELDS: [(&str, &str, TitleSource); 9] = [
    ("title", "TITLE", TitleSource::Document),
    ("number", "NO.", TitleSource::PartNumber),
    ("scale", "SCALE", TitleSource::Scale),
    ("projection", "PROJECTION", TitleSource::Projection),
    ("date", "DATE", TitleSource::Date),
    ("designer", "DESIGNER", TitleSource::Designer),
    ("material", "MATERIAL", TitleSource::Material),
    ("sheet", "SHEET", TitleSource::Sheet),
    ("unit", "UNIT", TitleSource::Unit),
];
pub(super) fn title_of(sheet: &Sheet) -> Result<Title, String> {
    if sheet.title.is_null() {
        Ok(Title::default())
    } else {
        serde_json::from_value(sheet.title.clone()).map_err(|e| format!("表題欄を読めません: {e}"))
    }
}
pub(super) fn field_keys() -> Vec<&'static str> {
    FIELDS.iter().map(|f| f.0).collect()
}

fn values(
    cx: &Context<'_>,
    sheet: &Sheet,
    title: &Title,
    index: usize,
    total: usize,
    date: Option<&str>,
) -> Vec<(&'static str, String)> {
    let value = |source| match source {
        TitleSource::Document => {
            if !cx.data.properties.title.is_empty() {
                cx.data.properties.title.clone()
            } else {
                cx.doc_path
                    .and_then(|p| p.file_stem())
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default()
            }
        }
        TitleSource::PartNumber => cx.data.properties.part_number.clone(),
        TitleSource::Scale => sheet
            .views
            .iter()
            .find(|v| matches!(v.kind, ViewKind::Base { .. }))
            .map(|v| {
                format!(
                    "{}:{}",
                    kmb_format::svg::num(v.scale.num),
                    kmb_format::svg::num(v.scale.den)
                )
            })
            .unwrap_or_default(),
        TitleSource::Projection => match sheet.projection {
            Projection::ThirdAngle => "THIRD".into(),
            Projection::FirstAngle => "FIRST".into(),
        },
        TitleSource::Date => date.unwrap_or_default().into(),
        TitleSource::Designer => cx.data.properties.designer.clone(),
        TitleSource::Material => cx.data.properties.material.clone(),
        TitleSource::Sheet => format!("{}/{}", index + 1, total),
        TitleSource::Unit => cx.data.properties.units.name().into(),
    };
    FIELDS
        .iter()
        .map(|(key, label, source)| {
            (
                *label,
                match title.fields.get(*key) {
                    Some(TitleValue::Text(s)) => s.clone(),
                    Some(TitleValue::Source { source, .. }) => value(*source),
                    None => value(*source),
                },
            )
        })
        .collect()
}

/// 表題の操作時に呼び、使う字だけを埋める。フォントのパス自体は文書に保存しない。
pub(super) fn prepare(
    cx: &Context<'_>,
    sheet: &Sheet,
    title: &mut Title,
    font: Option<&std::path::Path>,
    face: u32,
    index: usize,
    total: usize,
) -> Result<(), Problem> {
    let text = values(cx, sheet, title, index, total, None)
        .into_iter()
        .map(|(_, s)| s)
        .filter(|s| !s.is_ascii())
        .collect::<Vec<_>>()
        .join("\n");
    if text.is_ascii() {
        return Ok(());
    }
    if let Some(path) = font {
        let path = cx.open_path(path)?;
        let bytes = std::fs::read(&path)
            .map_err(|e| Problem::new(ProblemKind::File, format!("書体を読めません: {e}")))?;
        let mut loaded = OutlineFont::open(&bytes, face)?;
        let missing = loaded.add_glyphs(&bytes, face, &text)?;
        if !missing.is_empty() {
            return Err(format!("書体に文字 {missing:?} がありません").into());
        }
        title.outline = Some(loaded);
    }
    if title
        .outline
        .as_ref()
        .is_none_or(|f| !f.missing(&text).is_empty())
    {
        return Err("表題欄に必要な字形がありません。set_title の font に文書へ入れられる書体のファイルを指定してください".into());
    }
    Ok(())
}

fn stroke(owner: &str, pen: Pen, path: Path) -> Item {
    Item::Stroke {
        pen,
        line: LineStyle::Solid,
        path,
        owner: owner.into(),
    }
}
fn line(items: &mut Vec<Item>, owner: &str, pen: Pen, a: [f64; 2], b: [f64; 2]) {
    items.push(stroke(owner, pen, Path::Line { a, b }));
}

pub(super) fn frame(paper: &Paper) -> Result<[f64; 4], String> {
    let size = paper.size();
    let margin = if matches!(paper.format, PaperFormat::A0 | PaperFormat::A1) {
        20.0
    } else {
        10.0
    };
    let left = if paper.binding { 20.0 } else { margin };
    let rect = [left, margin, size[0] - margin, size[1] - margin];
    if rect[2] <= rect[0] || rect[3] <= rect[1] {
        return Err("用紙が枠と余白より小さいので、大きい用紙を指定してください".into());
    }
    Ok(rect)
}

fn text(
    items: &mut Vec<Item>,
    owner: &str,
    s: &str,
    at: [f64; 2],
    height: f64,
    max_width: f64,
    font: Option<&OutlineFont>,
) -> Result<(), String> {
    if s.is_empty() {
        return Ok(());
    }
    if s.lines().count() > 1 {
        return Err("表題欄の値は一行にしてください".into());
    }
    let font = if s.is_ascii() {
        None
    } else {
        Some(font.ok_or("表題欄の字形がありません。表題欄で書体を指定してください")?)
    };
    if font.is_some_and(|f| !f.missing(s).is_empty()) {
        return Err(
            "表題欄の値に文書の字形がない文字があります。表題欄で書体を指定し直してください".into(),
        );
    }
    let mut style = Style::new(Font::Sans, height);
    let fitted = style.fit_width_with(s, max_width, false, font)?;
    if fitted.width < style.width {
        style = fitted;
    }
    let laid = kmb_text::lay_out_with(s, &style, font, at);
    if laid.missing != 0 {
        return Err("表題欄の文字を描けません".into());
    }
    if font.is_some() {
        let loops = laid
            .rings
            .into_iter()
            .map(|ring| {
                let mut out = Vec::new();
                for (segment, back) in ring {
                    let mut points = segment.polyline(128);
                    if back {
                        points.reverse();
                    }
                    if out.last() == points.first() {
                        points.remove(0);
                    }
                    out.extend(points);
                }
                out
            })
            .filter(|l| l.len() >= 3)
            .collect::<Vec<_>>();
        if !loops.is_empty() {
            items.push(Item::Fill {
                loops,
                owner: owner.into(),
            });
        }
    } else {
        for segment in laid.segments {
            let path = match segment {
                Segment::Line { a, b } => Path::Line { a, b },
                Segment::Arc {
                    center,
                    radius,
                    start,
                    end,
                } => Path::Arc {
                    c: center,
                    r: radius,
                    start_deg: start.to_degrees(),
                    sweep_deg: (end - start).to_degrees(),
                },
                Segment::Circle { center, radius } => Path::Arc {
                    c: center,
                    r: radius,
                    start_deg: 0.0,
                    sweep_deg: 360.0,
                },
            };
            items.push(stroke(owner, Pen::Thin, path));
        }
    }
    Ok(())
}

fn projection_symbol(items: &mut Vec<Item>, owner: &str, at: [f64; 2], projection: &Projection) {
    let circle_x = if *projection == Projection::ThirdAngle {
        at[0] + 12.0
    } else {
        at[0] - 5.0
    };
    for radius in [1.5, 3.0] {
        items.push(stroke(
            owner,
            Pen::Thin,
            Path::Arc {
                c: [circle_x, at[1]],
                r: radius,
                start_deg: 0.0,
                sweep_deg: 360.0,
            },
        ));
    }
    let poly = vec![
        [at[0], at[1] - 3.0],
        [at[0] + 6.0, at[1] - 1.5],
        [at[0] + 6.0, at[1] + 1.5],
        [at[0], at[1] + 3.0],
        [at[0], at[1] - 3.0],
    ];
    items.push(stroke(owner, Pen::Thin, Path::Poly { points: poly }));
    line(
        items,
        owner,
        Pen::Thin,
        [at[0] - 8.0, at[1]],
        [at[0] + 16.0, at[1]],
    );
}

pub(super) fn append(
    cx: &Context<'_>,
    sheet: &Sheet,
    index: usize,
    total: usize,
    date: Option<&str>,
    picture: &mut SheetPicture,
) {
    let mut border = Vec::new();
    let result = (|| -> Result<Vec<Item>, String> {
        let [left, bottom, right, top] = frame(&sheet.paper)?;
        border.push(stroke(
            &sheet.id,
            Pen::ExtraThick,
            Path::Poly {
                points: vec![
                    [left, bottom],
                    [right, bottom],
                    [right, top],
                    [left, top],
                    [left, bottom],
                ],
            },
        ));
        let size = sheet.paper.size();
        for (a, b) in [
            ([0.0, size[1] / 2.0], [left + 3.0, size[1] / 2.0]),
            ([right - 3.0, size[1] / 2.0], [size[0], size[1] / 2.0]),
            ([size[0] / 2.0, 0.0], [size[0] / 2.0, bottom + 3.0]),
            ([size[0] / 2.0, top - 3.0], [size[0] / 2.0, size[1]]),
        ] {
            line(&mut border, &sheet.id, Pen::Thin, a, b);
        }
        let title = title_of(sheet)?;
        let owner = format!("{}:title", sheet.id);
        let mut items = Vec::new();
        let width = (right - left).min(180.0);
        let height = (top - bottom).min(45.0);
        let start = [right - width, bottom + height];
        let (cw, ch) = (width / 3.0, height / 3.0);
        for i in 0..=3 {
            line(
                &mut items,
                &owner,
                Pen::Thin,
                [start[0] + i as f64 * cw, bottom],
                [start[0] + i as f64 * cw, start[1]],
            );
            line(
                &mut items,
                &owner,
                Pen::Thin,
                [start[0], bottom + i as f64 * ch],
                [right, bottom + i as f64 * ch],
            );
        }
        for (i, (label, value)) in values(cx, sheet, &title, index, total, date)
            .into_iter()
            .enumerate()
        {
            let at = [
                start[0] + (i % 3) as f64 * cw + 1.5,
                start[1] - (i / 3) as f64 * ch,
            ];
            text(
                &mut items,
                &owner,
                label,
                [at[0], at[1] - 3.5],
                1.8,
                cw - 3.0,
                None,
            )?;
            let space = if i == 3 { cw - 30.0 } else { cw - 3.0 };
            text(
                &mut items,
                &owner,
                &value,
                [at[0], at[1] - 8.0],
                2.5,
                space,
                title.outline.as_ref(),
            )?;
            if i == 3 {
                projection_symbol(
                    &mut items,
                    &owner,
                    [at[0] + cw - 19.0, at[1] - 8.0],
                    &sheet.projection,
                );
            }
        }
        Ok(items)
    })();
    picture.items.extend(border);
    match result {
        Ok(items) => picture.items.extend(items),
        Err(message) => picture.problems.push(DrawingProblem {
            target: format!("{}:title", sheet.id),
            problem: kmb_core::Problem::new(kmb_core::ProblemKind::Compute, message),
            warning: false,
        }),
    }
}
