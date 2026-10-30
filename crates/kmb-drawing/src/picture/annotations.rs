//! 注記だけの失敗を用紙の問題へ分け、ほかのビューや注記を残す。

use super::*;
use crate::dimension::{self as dim, ViewGeometry};
use crate::lettering::{self, Bounds};
use kmb_asm::inspect::{BomRow, BomSource, bom_rows_with_sources, bom_table};

pub(super) fn draw(
    cx: &Context<'_>,
    sheet: &Sheet,
    pictures: &BTreeMap<String, Result<Picture, Problem>>,
    problems: &mut Vec<DrawingProblem>,
) -> Vec<Item> {
    let views = super::reference_views(cx, pictures);
    let (rows, sources) = if sheet.notes.iter().any(|n| {
        matches!(
            n.get("kind").and_then(|v| v.as_str()),
            Some("balloon" | "parts_table")
        )
    }) {
        bom_rows_with_sources(cx)
    } else {
        (vec![], BTreeMap::new())
    };
    let mut items = Vec::new();
    let mut labels: Vec<(String, Bounds)> = Vec::new();
    for (i, value) in sheet.notes.iter().enumerate() {
        let target = value
            .get("id")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| format!("注記 {}", i + 1));
        let result = (|| {
            let note: Annotation = serde_json::from_value(value.clone())
                .map_err(|e| format!("注記を読めません: {e}"))?;
            let style = style_of(&sheet.dim_style)?;
            let (made, bounds) = one(
                &note,
                &style,
                cx.data.properties.units,
                sheet.paper.size(),
                &views,
                &rows,
                &sources,
            )?;
            Ok::<_, String>((made, bounds))
        })();
        match result {
            Err(e) => problems.push(DrawingProblem {
                target,
                problem: Problem::new(ProblemKind::Compute, e),
                warning: false,
            }),
            Ok((made, bounds)) => {
                for b in bounds {
                    if let Some((other, _)) = labels.iter().find(|(_, a)| overlap(*a, b)) {
                        problems.push(DrawingProblem {
                            target: target.clone(),
                            problem: Problem::new(
                                ProblemKind::Found,
                                format!(
                                    "文字が注記 {other} と重なっています。位置を変えてください"
                                ),
                            ),
                            warning: true,
                        });
                    }
                    labels.push((target.clone(), b));
                }
                items.extend(made);
            }
        }
    }
    items
}

fn overlap(a: Bounds, b: Bounds) -> bool {
    (0..2).all(|i| a.0[i] < b.1[i] && b.0[i] < a.1[i])
}
fn line(out: &mut Vec<Item>, id: &str, a: [f64; 2], b: [f64; 2], kind: LineStyle) {
    out.push(lettering::stroke(id, kind, Path::Line { a, b }));
}
fn shift(items: &mut [Item], delta: [f64; 2]) {
    for item in items {
        match item {
            Item::Stroke { path, .. } => *path = transform(path, |p| dim::add(p, delta), 1.0),
            Item::Fill { loops, .. } | Item::Hatch { loops, .. } => {
                for p in loops.iter_mut().flatten() {
                    *p = dim::add(*p, delta);
                }
            }
        }
    }
}
fn arrow(
    out: &mut Vec<Item>,
    id: &str,
    tip: [f64; 2],
    direction: [f64; 2],
    style: &DimStyle,
    kind: Arrow,
) {
    let length = dim::norm(direction);
    if length < 1e-12 {
        return;
    }
    let d = dim::scale(direction, 1.0 / length);
    let n = [-d[1], d[0]];
    match kind {
        Arrow::Triangle => {
            let base = dim::add(tip, dim::scale(d, style.arrow_length));
            let width = style.arrow_length * (style.arrow_angle.to_radians() * 0.5).tan();
            out.push(Item::Fill {
                loops: vec![vec![
                    tip,
                    dim::add(base, dim::scale(n, width)),
                    dim::add(base, dim::scale(n, -width)),
                    tip,
                ]],
                owner: id.into(),
            });
        }
        Arrow::Dot => {
            let r = 0.2 * style.arrow_length;
            let points = (0..=24)
                .map(|i| {
                    let t = std::f64::consts::TAU * i as f64 / 24.0;
                    dim::add(tip, [r * t.cos(), r * t.sin()])
                })
                .collect();
            out.push(Item::Fill {
                loops: vec![points],
                owner: id.into(),
            });
        }
        Arrow::Slash => {
            let t = dim::scale(dim::add(d, n), style.arrow_length * 0.35);
            line(
                out,
                id,
                dim::sub(tip, t),
                dim::add(tip, t),
                LineStyle::Solid,
            );
        }
    }
}
fn label(
    out: &mut Vec<Item>,
    note: &Annotation,
    text: &str,
    height: f64,
    at: [f64; 2],
    centered: bool,
) -> Result<Bounds, String> {
    let start = out.len();
    let bounds = lettering::draw(&note.id, text, height, [0.0; 2], note.outline.as_ref(), out)?;
    let offset = [
        at[0]
            - if centered {
                0.5 * (bounds.0[0] + bounds.1[0])
            } else {
                bounds.0[0]
            },
        at[1] - bounds.0[1],
    ];
    shift(&mut out[start..], offset);
    Ok((dim::add(bounds.0, offset), dim::add(bounds.1, offset)))
}

pub(super) fn one(
    note: &Annotation,
    style: &DimStyle,
    unit: kmb_core::LengthUnit,
    paper: [f64; 2],
    views: &BTreeMap<String, ViewGeometry>,
    rows: &[BomRow],
    sources: &BTreeMap<BomSource, usize>,
) -> Result<(Vec<Item>, Vec<Bounds>), String> {
    let mut out = Vec::new();
    let mut labels = Vec::new();
    let id = &note.id;
    match &note.kind {
        AnnotationKind::Dimension {
            dimension,
            refs,
            at,
        } => {
            let g = dim::dimension(*dimension, refs, *at, views)?;
            for i in 0..2 {
                let delta = dim::sub(g.ends[i], g.origins[i]);
                let length = dim::norm(delta);
                if length > style.extension_gap {
                    let d = dim::scale(delta, 1.0 / length);
                    line(
                        &mut out,
                        id,
                        dim::add(g.origins[i], dim::scale(d, style.extension_gap)),
                        dim::add(g.ends[i], dim::scale(d, style.extension_over)),
                        LineStyle::Solid,
                    );
                }
            }
            let mut directions = [
                dim::sub(g.ends[1], g.ends[0]),
                dim::sub(g.ends[0], g.ends[1]),
            ];
            if let Some(path) = g.arc {
                if let Path::Arc {
                    start_deg,
                    sweep_deg,
                    ..
                } = &path
                {
                    let (a, b) = (start_deg.to_radians(), (start_deg + sweep_deg).to_radians());
                    let sign = sweep_deg.signum();
                    directions = [
                        [-a.sin() * sign, a.cos() * sign],
                        [b.sin() * sign, -b.cos() * sign],
                    ];
                }
                out.push(lettering::stroke(id, LineStyle::Solid, path));
            } else {
                line(&mut out, id, g.ends[0], g.ends[1], LineStyle::Solid);
            }
            for (i, direction) in directions.into_iter().enumerate() {
                if *dimension != DimKind::Radius || i == 1 {
                    arrow(&mut out, id, g.ends[i], direction, style, style.arrow);
                }
            }
            let text = format!("{}{}", g.prefix, style.value_text(g.value, g.angle, unit));
            labels.push(label(
                &mut out,
                note,
                &text,
                style.height,
                dim::add(g.text, [0.0, 0.3 * style.height]),
                true,
            )?);
        }
        AnnotationKind::Note { text, at, leader } => {
            if let Some(leader) = leader {
                let tip = match &leader.reference {
                    Some(r) => dim::resolve(r, views)?.point,
                    None => leader.tip,
                };
                line(&mut out, id, tip, *at, LineStyle::Solid);
                arrow(&mut out, id, tip, dim::sub(*at, tip), style, leader.arrow);
            }
            labels.push(label(&mut out, note, text, style.height, *at, false)?);
        }
        AnnotationKind::CenterMark { reference } => {
            let r = dim::resolve(reference, views)?;
            let a = r.edge.arc.ok_or("中心の印には円か円弧を選んでください")?;
            let c = r.space.point(a.center).0;
            let half = a.radius * r.space.scale + style.height;
            for d in [[half, 0.0], [0.0, half]] {
                line(
                    &mut out,
                    id,
                    dim::sub(c, d),
                    dim::add(c, d),
                    LineStyle::Chain,
                );
            }
        }
        AnnotationKind::CenterLine { refs } => {
            let (a, b) = (
                dim::resolve(&refs[0], views)?,
                dim::resolve(&refs[1], views)?,
            );
            if refs[0].view != refs[1].view {
                return Err("中心線の辺は同じビューで選んでください".into());
            }
            let (start, end) = match (a.edge.arc, b.edge.arc) {
                (Some(c), Some(d)) => {
                    let delta = kmb_core::kernel::sub(d.center, c.center);
                    let length = kmb_core::kernel::dot(delta, delta).sqrt();
                    let along = kmb_core::kernel::dot(delta, c.axis);
                    let radial = std::array::from_fn::<_, 3, _>(|i| delta[i] - along * c.axis[i]);
                    if length < 1e-9
                        || kmb_core::kernel::dot(radial, radial).sqrt() > 1e-6
                        || kmb_core::kernel::dot(c.axis, d.axis).abs() < 1.0 - 1e-6
                        || (c.radius - d.radius).abs() > 1e-6
                        || kmb_core::kernel::dot(c.axis, a.space.frame.eye).abs() > 1e-6
                    {
                        return Err("円筒の中心線には、軸を横から見るビューで同軸・同じ半径の両端の円を選んでください".into());
                    }
                    (a.space.point(c.center).0, b.space.point(d.center).0)
                }
                (None, None) if a.edge.curve.is_empty() && b.edge.curve.is_empty() => {
                    let a = [a.space.point(a.edge.a).0, a.space.point(a.edge.b).0];
                    let mut b = [b.space.point(b.edge.a).0, b.space.point(b.edge.b).0];
                    let d = dim::sub(a[1], a[0]);
                    let e = dim::sub(b[1], b[0]);
                    if dim::norm(d) < 1e-12
                        || dim::norm(e) < 1e-12
                        || dim::cross(d, e).abs() > 1e-6 * dim::norm(d) * dim::norm(e)
                    {
                        return Err("中心線には平行な直線を選んでください".into());
                    }
                    if dim::dot2(d, e) < 0.0 {
                        b.swap(0, 1);
                    }
                    (
                        dim::scale(dim::add(a[0], b[0]), 0.5),
                        dim::scale(dim::add(a[1], b[1]), 0.5),
                    )
                }
                _ => {
                    return Err("中心線には平行な直線二本か、円筒の両端の円を選んでください".into());
                }
            };
            let d = dim::sub(end, start);
            if dim::norm(d) < 1e-12 {
                return Err("中心線の両端が重なっています".into());
            }
            let d = dim::scale(d, style.height / dim::norm(d));
            line(
                &mut out,
                id,
                dim::sub(start, d),
                dim::add(end, d),
                LineStyle::Chain,
            );
        }
        AnnotationKind::Balloon {
            view,
            part,
            tip,
            at,
            number,
        } => {
            let p = views
                .get(view)
                .ok_or("番号札のビューがありません。ビューを選び直してください")?;
            let row = *sources
                .get(part)
                .ok_or("番号札の部品がありません。部品を選び直してください")?;
            if *number != row + 1 {
                return Err("部品表の順が変わりました。番号札を振り直してください".into());
            }
            let tip = dim::add(p.at, dim::scale(*tip, p.scale));
            let delta = dim::sub(tip, *at);
            let length = dim::norm(delta);
            let radius = style.height.max(3.0);
            if length < radius {
                return Err("番号札は指す点から離して置いてください".into());
            }
            let end = dim::add(*at, dim::scale(delta, radius / length));
            line(&mut out, id, tip, end, LineStyle::Solid);
            arrow(&mut out, id, tip, dim::sub(end, tip), style, style.arrow);
            out.push(lettering::stroke(
                id,
                LineStyle::Solid,
                Path::Arc {
                    c: *at,
                    r: radius,
                    start_deg: 0.0,
                    sweep_deg: 360.0,
                },
            ));
            labels.push(label(
                &mut out,
                note,
                &number.to_string(),
                style.height,
                dim::add(*at, [0.0, -0.5 * style.height]),
                true,
            )?);
        }
        AnnotationKind::PartsTable { at, columns } => {
            let table = bom_table(rows);
            let keys = if columns.is_empty() {
                vec![
                    "item",
                    "name",
                    "count",
                    "material",
                    "length",
                    "width",
                    "thickness",
                    "mass",
                ]
            } else {
                columns.iter().map(String::as_str).collect()
            };
            let indices = keys
                .iter()
                .map(|k| {
                    table
                        .columns
                        .iter()
                        .position(|c| c.id == *k)
                        .ok_or_else(|| format!("部品表に列 {k} はありません"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            if indices.len() > 12 {
                return Err("部品表の列が多すぎます。12列までにしてください".into());
            }
            let mut data = vec![
                indices
                    .iter()
                    .map(|i| table.columns[*i].label.clone())
                    .collect::<Vec<_>>(),
            ];
            data.extend(
                table
                    .rows
                    .iter()
                    .map(|r| indices.iter().map(|i| r[*i].clone()).collect()),
            );
            let mut cells = Vec::new();
            let mut widths = vec![0.0f64; indices.len()];
            let mut row_height = 1.5 * style.height;
            for row in data {
                let mut made = Vec::new();
                for (i, text) in row.iter().enumerate() {
                    let mut items = Vec::new();
                    let b = lettering::draw(
                        id,
                        text,
                        style.height,
                        [0.0; 2],
                        note.outline.as_ref(),
                        &mut items,
                    )?;
                    widths[i] = widths[i].max(b.1[0] - b.0[0] + 2.0);
                    row_height = row_height.max(b.1[1] - b.0[1] + 2.0);
                    made.push((items, b));
                }
                cells.push(made);
            }
            let width: f64 = widths.iter().sum();
            let height = row_height * cells.len() as f64;
            let at = at.unwrap_or([paper[0] - 10.0 - width, 10.0 + height]);
            let mut x = at[0];
            for w in widths.iter().chain(std::iter::once(&0.0)) {
                line(
                    &mut out,
                    id,
                    [x, at[1]],
                    [x, at[1] - height],
                    LineStyle::Solid,
                );
                x += w;
            }
            for r in 0..=cells.len() {
                let y = at[1] - r as f64 * row_height;
                line(
                    &mut out,
                    id,
                    [at[0], y],
                    [at[0] + width, y],
                    LineStyle::Solid,
                );
            }
            for (row, cells) in cells.into_iter().enumerate() {
                let mut x = at[0] + 1.0;
                for ((mut items, bounds), w) in cells.into_iter().zip(&widths) {
                    let delta = [
                        x - bounds.0[0],
                        at[1] - (row + 1) as f64 * row_height + 1.0 - bounds.0[1],
                    ];
                    shift(&mut items, delta);
                    out.extend(items);
                    x += w;
                }
            }
            labels.push(([at[0], at[1] - height], [at[0] + width, at[1]]));
        }
    }
    Ok((out, labels))
}
