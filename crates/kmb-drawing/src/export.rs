//! 同じ用紙の線・塗りを、SVG・DXF・PDF に書く。ハッチの展開も共有する。

use crate::{Pens, Sheet};
use kmb_format::{dxf, pdf, svg};
use kmb_interchange::drawing::{Item, LineStyle, Path, Pen, SheetPicture};
use std::fmt::Write as _;

pub(super) const LAYERS: [&str; 8] = [
    "OUTLINE",
    "HIDDEN",
    "CENTER",
    "CUT",
    "HATCH",
    "DIMENSION",
    "TEXT",
    "FRAME",
];

fn width(pen: Pen, pens: &Pens) -> f64 {
    match pen {
        Pen::Thick => pens.thick,
        Pen::Thin => pens.thin,
        Pen::ExtraThick => pens.extra_thick,
    }
}

fn hatch(loops: &[Vec<[f64; 2]>], angle: f64, spacing: f64) -> Result<Vec<Path>, String> {
    if !angle.is_finite() || !spacing.is_finite() || spacing <= 0.0 {
        return Err("ハッチの角度と正の間隔を指定してください".into());
    }
    let (s, c) = angle.to_radians().sin_cos();
    let rotate = |p: [f64; 2]| [p[0] * c + p[1] * s, -p[0] * s + p[1] * c];
    let back = |p: [f64; 2]| [p[0] * c - p[1] * s, p[0] * s + p[1] * c];
    let loops = loops
        .iter()
        .map(|l| l.iter().copied().map(rotate).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for p in loops.iter().flatten() {
        if !p.iter().all(|v| v.is_finite()) {
            return Err("ハッチの輪に有限でない点があります".into());
        }
        lo = lo.min(p[1]);
        hi = hi.max(p[1]);
    }
    if lo > hi {
        return Ok(vec![]);
    }
    let start = (lo / spacing).floor() * spacing;
    let count = ((hi - start) / spacing).ceil() as usize;
    if count > 100_000 {
        return Err("ハッチの線が多すぎます。間隔を広げてください".into());
    }
    let mut out = Vec::new();
    for i in 0..=count {
        let y = start + i as f64 * spacing;
        let mut xs = Vec::new();
        for l in &loops {
            if l.len() < 3 {
                continue;
            }
            for j in 0..l.len() {
                let (a, b) = (l[j], l[(j + 1) % l.len()]);
                if (a[1] <= y && b[1] > y) || (b[1] <= y && a[1] > y) {
                    xs.push(a[0] + (b[0] - a[0]) * (y - a[1]) / (b[1] - a[1]));
                }
            }
        }
        xs.sort_by(f64::total_cmp);
        if !xs.len().is_multiple_of(2) {
            return Err("ハッチの輪が閉じていません".into());
        }
        for pair in xs.as_chunks::<2>().0 {
            if pair[1] - pair[0] > 1e-9 {
                out.push(Path::Line {
                    a: back([pair[0], y]),
                    b: back([pair[1], y]),
                });
            }
        }
    }
    Ok(out)
}

fn expanded(items: &[Item]) -> Result<Vec<Item>, String> {
    let mut out = Vec::new();
    for item in items {
        match item {
            Item::Hatch {
                loops,
                angle_deg,
                spacing_mm,
                pen,
                owner,
            } => {
                for path in hatch(loops, *angle_deg, *spacing_mm)? {
                    out.push(Item::Stroke {
                        pen: *pen,
                        line: LineStyle::Solid,
                        path,
                        owner: owner.clone(),
                    });
                }
            }
            _ => out.push(item.clone()),
        }
    }
    Ok(out)
}

fn svg_path(path: &Path) -> String {
    let n = svg::num;
    let point = |p: [f64; 2]| format!("{} {}", n(p[0]), n(p[1]));
    match path {
        Path::Line { a, b } => format!("M {} L {}", point(*a), point(*b)),
        Path::Poly { points } => {
            let mut out = String::new();
            for (i, p) in points.iter().enumerate() {
                write!(out, "{} {} ", if i == 0 { "M" } else { "L" }, point(*p)).unwrap();
            }
            out
        }
        Path::Arc {
            c,
            r,
            start_deg,
            sweep_deg,
        } => {
            let at = |a: f64| {
                [
                    c[0] + r * a.to_radians().cos(),
                    c[1] + r * a.to_radians().sin(),
                ]
            };
            let count = if sweep_deg.abs() < 360.0 - 1e-7 { 1 } else { 2 };
            let sweep = *sweep_deg / count as f64;
            let mut out = format!("M {}", point(at(*start_deg)));
            for i in 1..=count {
                write!(
                    out,
                    " A {} {} 0 {} {} {}",
                    n(*r),
                    n(*r),
                    u8::from(sweep.abs() > 180.0),
                    u8::from(sweep >= 0.0),
                    point(at(start_deg + i as f64 * sweep))
                )
                .unwrap();
            }
            out
        }
    }
}

pub fn export_svg(picture: &SheetPicture, sheet: &Sheet) -> Result<Vec<u8>, String> {
    let mut out = svg::begin_mm(picture.size[0], picture.size[1]);
    writeln!(
        out,
        "<g transform=\"translate(0 {}) scale(1 -1)\">",
        svg::num(picture.size[1])
    )
    .unwrap();
    for item in expanded(&picture.items)? {
        match item {
            Item::Stroke {
                pen, line, path, ..
            } => {
                let w = width(pen, &sheet.pens);
                let dash = line
                    .pattern(w)
                    .iter()
                    .map(|v| svg::num(*v))
                    .collect::<Vec<_>>()
                    .join(" ");
                let shape = match &path {
                    Path::Arc {
                        c, r, sweep_deg, ..
                    } if sweep_deg.abs() >= 360.0 - 1e-7 => format!(
                        "circle cx=\"{}\" cy=\"{}\" r=\"{}\"",
                        svg::num(c[0]),
                        svg::num(c[1]),
                        svg::num(*r)
                    ),
                    _ => format!("path d=\"{}\"", svg_path(&path)),
                };
                writeln!(out,"<{shape} fill=\"none\" stroke=\"#000\" stroke-width=\"{}\" stroke-dasharray=\"{}\"/>",svg::num(w),dash).unwrap();
            }
            Item::Fill { loops, .. } => {
                let path = loops
                    .iter()
                    .filter(|l| l.len() >= 3)
                    .map(|l| format!("{} Z", svg_path(&Path::Poly { points: l.clone() })))
                    .collect::<Vec<_>>()
                    .join(" ");
                writeln!(
                    out,
                    "<path d=\"{path}\" fill=\"#000\" fill-rule=\"evenodd\"/>"
                )
                .unwrap();
            }
            Item::Hatch { .. } => unreachable!(),
        }
    }
    out.push_str("</g>\n</svg>\n");
    Ok(out.into_bytes())
}

fn role(item: &Item, sheet: &Sheet) -> &'static str {
    let owner = match item {
        Item::Stroke { owner, .. } | Item::Fill { owner, .. } | Item::Hatch { owner, .. } => owner,
    };
    if owner == &sheet.id {
        return "FRAME";
    }
    if let Some(note) = sheet
        .notes
        .iter()
        .find(|n| n.get("id").and_then(|v| v.as_str()) == Some(owner))
    {
        return match note.get("kind").and_then(|v| v.as_str()) {
            Some("center_mark" | "center_line") => "CENTER",
            Some("note" | "parts_table") => "TEXT",
            _ => "DIMENSION",
        };
    }
    match item {
        Item::Hatch { .. } => "HATCH",
        Item::Fill { .. } => "TEXT",
        Item::Stroke {
            line: LineStyle::Dashed,
            ..
        } => "HIDDEN",
        Item::Stroke {
            line: LineStyle::Chain,
            ..
        } => "CUT",
        Item::Stroke { pen: Pen::Thin, .. } => "TEXT",
        _ => "OUTLINE",
    }
}

pub fn export_dxf(picture: &SheetPicture, sheet: &Sheet) -> Result<Vec<u8>, String> {
    let mut expanded_items = Vec::new();
    for item in &picture.items {
        let role = role(item, sheet);
        expanded_items.extend(
            expanded(std::slice::from_ref(item))?
                .into_iter()
                .map(|i| (role, i)),
        );
    }
    let colors = LAYERS
        .iter()
        .enumerate()
        .map(|(i, name)| {
            (
                *name,
                sheet
                    .paper
                    .colors
                    .get(*name)
                    .copied()
                    .unwrap_or(i as i32 + 1),
                dxf::CONTINUOUS,
            )
        })
        .collect::<Vec<_>>();
    let pattern = |line: LineStyle| {
        line.pattern(0.25)
            .into_iter()
            .enumerate()
            .map(|(i, v)| if i % 2 == 0 { v } else { -v })
            .collect::<Vec<_>>()
    };
    let dashed = pattern(LineStyle::Dashed);
    let chain = pattern(LineStyle::Chain);
    let double = pattern(LineStyle::DoubleChain);
    let types = [
        dxf::LineType {
            name: "DRAWING_DASHED",
            description: "Dashed line",
            pattern: &dashed,
        },
        dxf::LineType {
            name: dxf::CHAIN,
            description: "Center line",
            pattern: &chain,
        },
        dxf::LineType {
            name: dxf::DOUBLE_CHAIN,
            description: "Double center line",
            pattern: &double,
        },
    ];
    let mut out = dxf::R12::begin_with_linetypes(svg::num, [0.0; 2], picture.size, &colors, &types);
    for (layer, item) in expanded_items {
        match item {
            Item::Stroke {
                path, pen, line, ..
            } => {
                let kind = match line {
                    LineStyle::Solid => dxf::CONTINUOUS,
                    LineStyle::Dashed => "DRAWING_DASHED",
                    LineStyle::Chain => dxf::CHAIN,
                    LineStyle::DoubleChain => dxf::DOUBLE_CHAIN,
                };
                // 折れ線の頂点は別の実体なので、線種と尺度は POLYLINE の頭に付ける。
                match path {
                    Path::Line { a, b } => {
                        out.line(layer, a, b);
                        out.pair(6, kind);
                        out.pair(48, svg::num(width(pen, &sheet.pens) / 0.25));
                    }
                    Path::Arc {
                        c,
                        r,
                        start_deg,
                        sweep_deg,
                    } => {
                        if sweep_deg.abs() >= 360.0 - 1e-7 {
                            out.circle(layer, c, r);
                        } else {
                            let (a, b) = if sweep_deg >= 0.0 {
                                (start_deg, start_deg + sweep_deg)
                            } else {
                                (start_deg + sweep_deg, start_deg)
                            };
                            out.arc(layer, c, r, a.to_radians(), b.to_radians());
                        }
                        out.pair(6, kind);
                        out.pair(48, svg::num(width(pen, &sheet.pens) / 0.25));
                    }
                    Path::Poly { points } => {
                        out.pair(0, "POLYLINE");
                        out.pair(8, layer);
                        out.pair(6, kind);
                        out.pair(48, svg::num(width(pen, &sheet.pens) / 0.25));
                        out.pair(66, 1);
                        out.xy(10, [0.0; 2]);
                        out.pair(70, 0);
                        for p in points {
                            out.pair(0, "VERTEX");
                            out.pair(8, layer);
                            out.xy(10, p);
                        }
                        out.pair(0, "SEQEND");
                        out.pair(8, layer);
                    }
                }
            }
            Item::Fill { loops, .. } => {
                for mut points in loops {
                    if points.len() < 3 {
                        continue;
                    }
                    if points.first() != points.last() {
                        points.push(points[0]);
                    }
                    out.polyline(layer, &points);
                }
            }
            Item::Hatch { .. } => unreachable!(),
        }
    }
    Ok(out.finish().into_bytes())
}

fn pdf_path(path: &Path, out: &mut Vec<pdf::Op>) {
    match path {
        Path::Line { a, b } => {
            out.push(pdf::Op::Move(*a));
            out.push(pdf::Op::Line(*b));
        }
        Path::Poly { points } => {
            for (i, p) in points.iter().enumerate() {
                out.push(if i == 0 {
                    pdf::Op::Move(*p)
                } else {
                    pdf::Op::Line(*p)
                });
            }
        }
        Path::Arc {
            c,
            r,
            start_deg,
            sweep_deg,
        } => {
            // 四分円の最大の半径方向の誤差の上限を、角度の六乗で縮める。
            let max_angle = std::f64::consts::FRAC_PI_2
                * (0.0025 / (0.00028 * r.abs().max(1e-12)))
                    .powf(1.0 / 6.0)
                    .min(1.0);
            let n = (sweep_deg.to_radians().abs() / max_angle).ceil().max(1.0) as usize;
            let step = sweep_deg.to_radians() / n as f64;
            let at = |a: f64| [c[0] + r * a.cos(), c[1] + r * a.sin()];
            let start = start_deg.to_radians();
            out.push(pdf::Op::Move(at(start)));
            for i in 0..n {
                let a = start + i as f64 * step;
                let b = a + step;
                let k = 4.0 / 3.0 * (step / 4.0).tan();
                let p = at(a);
                let q = at(b);
                out.push(pdf::Op::Cubic([
                    [p[0] - k * r * a.sin(), p[1] + k * r * a.cos()],
                    [q[0] + k * r * b.sin(), q[1] - k * r * b.cos()],
                    q,
                ]));
            }
        }
    }
}

pub fn pdf_page(picture: &SheetPicture, sheet: &Sheet) -> Result<pdf::Page, String> {
    let mut ops = Vec::new();
    for item in expanded(&picture.items)? {
        match item {
            Item::Stroke {
                pen, line, path, ..
            } => {
                let start = ops.len();
                pdf_path(&path, &mut ops);
                if ops.len() > start {
                    let w = width(pen, &sheet.pens);
                    ops.push(pdf::Op::Stroke {
                        width: w,
                        dash: line.pattern(w),
                    });
                }
            }
            Item::Fill { loops, .. } => {
                let start = ops.len();
                for points in loops.iter().filter(|l| l.len() >= 3) {
                    pdf_path(
                        &Path::Poly {
                            points: points.clone(),
                        },
                        &mut ops,
                    );
                    ops.push(pdf::Op::Close);
                }
                if ops.len() > start {
                    ops.push(pdf::Op::FillEvenOdd);
                }
            }
            Item::Hatch { .. } => unreachable!(),
        }
    }
    Ok(pdf::Page {
        size_mm: picture.size,
        ops,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hatches_keep_holes_and_pdf_arcs_stay_within_paper_tolerance() {
        let loops = vec![
            vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
            vec![[4.0, 4.0], [6.0, 4.0], [6.0, 6.0], [4.0, 6.0]],
        ];
        let lines = hatch(&loops, 0.0, 1.0).unwrap();
        assert!(
            lines
                .iter()
                .any(|l| matches!(l,Path::Line {a,b} if a==&[0.0,5.0]&&b==&[4.0,5.0]))
        );
        assert!(
            !lines
                .iter()
                .any(|l| matches!(l,Path::Line {a,b} if a[1]==5.0 && a[0]<5.0 && b[0]>5.0))
        );
        for radius in [0.1, 20.0, 1000.0] {
            let mut ops = Vec::new();
            pdf_path(
                &Path::Arc {
                    c: [0.0; 2],
                    r: radius,
                    start_deg: 13.0,
                    sweep_deg: -287.0,
                },
                &mut ops,
            );
            let mut start = [0.0; 2];
            for op in ops {
                match op {
                    pdf::Op::Move(p) => start = p,
                    pdf::Op::Cubic([a, b, end]) => {
                        for i in 0..=100 {
                            let t = i as f64 / 100.0;
                            let u = 1.0 - t;
                            let p = std::array::from_fn::<_, 2, _>(|k| {
                                u * u * u * start[k]
                                    + 3.0 * u * u * t * a[k]
                                    + 3.0 * u * t * t * b[k]
                                    + t * t * t * end[k]
                            });
                            assert!((p[0].hypot(p[1]) - radius).abs() < 0.005);
                        }
                        start = end;
                    }
                    _ => unreachable!(),
                }
            }
        }
    }
}
