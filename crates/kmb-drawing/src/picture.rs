use crate::{geometry::*, *};
use kmb_core::kernel::{EdgeInfo, Mesh, P3, Seam, dot, sub};
use kmb_core::{Context, FeatureId, Problem, ProblemKind};
use kmb_interchange::drawing::{
    DrawingProblem, EdgeKey, Item, LineKey, LineStyle, Path, Pen, SheetPicture, ViewLine,
};
use kmb_view_lines::{Projector, View, silhouette_lines};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
struct Body {
    id: FeatureId,
    mesh: Mesh,
    edges: Vec<(EdgeKey, EdgeInfo)>,
    named: bool,
    cuts: Vec<Cut>,
    cut_lines: Vec<Vec<P3>>,
    shape: kmb_core::ShapeHandle,
}
#[derive(Clone)]
struct Picture {
    view: View,
    at: [f64; 2],
    scale: f64,
    bodies: Vec<Body>,
    items: Vec<Item>,
    lines: Vec<ViewLine>,
}

/// 用紙の定義だけを保存し、線は最新の再計算結果から作る。
pub fn sheet_picture(cx: &Context<'_>, sheet_id: &str) -> Result<SheetPicture, Problem> {
    Ok(sheet_picture_with_lines(cx, sheet_id)?.0)
}
/// 寸法が引く名前付きの線の表も返す。表のキーはビューの id。
pub fn sheet_picture_with_lines(
    cx: &Context<'_>,
    sheet_id: &str,
) -> Result<(SheetPicture, BTreeMap<String, Vec<ViewLine>>), Problem> {
    picture_dated(cx, sheet_id, None)
}

pub(super) fn picture_dated(
    cx: &Context<'_>,
    sheet_id: &str,
    date: Option<&str>,
) -> Result<(SheetPicture, BTreeMap<String, Vec<ViewLine>>), Problem> {
    let drawings: Drawings = cx
        .data
        .section()
        .map_err(|e| Problem::new(ProblemKind::Input, e.to_string()))?;
    let sheet = drawings
        .sheets
        .iter()
        .find(|s| s.id == sheet_id)
        .ok_or("用紙がありません")?;
    let mut pictures: BTreeMap<String, Result<Picture, Problem>> = BTreeMap::new();
    let mut problems = vec![];
    for v in &sheet.views {
        calculate(cx, sheet, v, &mut pictures, &mut problems);
    }
    // 親への切断線・円は、親自身の線の後に、子の定義順で入れる。
    let mut marks: BTreeMap<String, Vec<Item>> = BTreeMap::new();
    for v in &sheet.views {
        if !matches!(pictures.get(&v.id), Some(Ok(_))) {
            continue;
        }
        let Some(parent) = v.parent() else {
            continue;
        };
        if let Some(Ok(p)) = pictures.get(parent) {
            let mark = marks.entry(parent.to_string()).or_default();
            match &v.kind {
                ViewKind::Section { line, letter, .. } => {
                    let to = |q: [f64; 2]| [p.at[0] + q[0] * p.scale, p.at[1] + q[1] * p.scale];
                    let (a, b) = (to(line[0]), to(line[1]));
                    mark.push(stroke(
                        parent,
                        Pen::Thin,
                        LineStyle::Chain,
                        Path::Line { a, b },
                    ));
                    let d = dist(a, b);
                    let t = [(b[0] - a[0]) / d, (b[1] - a[1]) / d];
                    let n = [-t[1], t[0]];
                    for (q, sign) in [(a, 1.), (b, -1.)] {
                        mark.push(stroke(
                            parent,
                            Pen::Thick,
                            LineStyle::Solid,
                            Path::Line {
                                a: q,
                                b: [q[0] + sign * t[0] * 3., q[1] + sign * t[1] * 3.],
                            },
                        ));
                        let tip = [q[0] + n[0] * 7., q[1] + n[1] * 7.];
                        mark.push(Item::Fill {
                            loops: vec![vec![
                                tip,
                                [tip[0] - n[0] * 3. + t[0], tip[1] - n[1] * 3. + t[1]],
                                [tip[0] - n[0] * 3. - t[0], tip[1] - n[1] * 3. - t[1]],
                                tip,
                            ]],
                            owner: parent.into(),
                        });
                        text(mark, parent, letter, [tip[0] + 2., tip[1] + 2.]);
                    }
                }
                ViewKind::Detail {
                    center,
                    radius,
                    letter,
                    ..
                } => {
                    let c = [p.at[0] + center[0] * p.scale, p.at[1] + center[1] * p.scale];
                    let r = radius * p.scale;
                    mark.push(stroke(
                        parent,
                        Pen::Thin,
                        LineStyle::Solid,
                        Path::Arc {
                            c,
                            r,
                            start_deg: 0.,
                            sweep_deg: 360.,
                        },
                    ));
                    text(mark, parent, letter, [c[0] + r + 2., c[1]]);
                }
                _ => {}
            }
        }
    }
    let mut items = vec![];
    let mut tables = BTreeMap::new();
    for v in &sheet.views {
        match pictures.remove(&v.id).unwrap() {
            Ok(p) => {
                items.extend(p.items);
                items.extend(marks.remove(&v.id).unwrap_or_default());
                tables.insert(v.id.clone(), p.lines);
            }
            Err(problem) => problems.push(DrawingProblem {
                target: v.id.clone(),
                problem,
                warning: false,
            }),
        }
    }
    let mut picture = SheetPicture {
        sheet: sheet.id.clone(),
        size: sheet.paper.size(),
        items,
        problems,
    };
    let index = drawings
        .sheets
        .iter()
        .position(|s| s.id == sheet_id)
        .unwrap();
    crate::paper::append(cx, sheet, index, drawings.sheets.len(), date, &mut picture);
    Ok((picture, tables))
}
fn calculate(
    cx: &Context<'_>,
    sheet: &Sheet,
    v: &DrawingView,
    done: &mut BTreeMap<String, Result<Picture, Problem>>,
    problems: &mut Vec<DrawingProblem>,
) {
    if done.contains_key(&v.id) {
        return;
    }
    if let Some(id) = v.parent() {
        let p = sheet
            .views
            .iter()
            .find(|p| p.id == id)
            .expect("区画が親を検査する");
        calculate(cx, sheet, p, done, problems);
    }
    let result = (|| -> Result<Picture, Problem> {
        let parent = match v.parent() {
            Some(id) => Some(
                done[id]
                    .as_ref()
                    .map_err(|_| Problem::new(ProblemKind::Compute, "親のビューを作れません"))?,
            ),
            None => None,
        };
        let scale = v.scale.value()?;
        let tol = (0.01 / scale).clamp(0.001, 0.1);
        let (view, at, mut bodies) = match &v.kind {
            ViewKind::Base {
                source,
                orientation,
                rotate,
            } => {
                let mut view = View::named(orientation.name()).unwrap();
                let a = (*rotate as f64).to_radians();
                let (r, u) = (view.right, view.up);
                view.right = [0, 1, 2].map(|k| r[k] * a.cos() - u[k] * a.sin());
                view.up = [0, 1, 2].map(|k| r[k] * a.sin() + u[k] * a.cos());
                (view, v.at, load(cx, source, tol, &v.id, problems)?)
            }
            ViewKind::Projected { side, .. } => {
                let p = parent.unwrap();
                let axis = side.axis();
                let sign = if sheet.projection == Projection::ThirdAngle {
                    1.
                } else {
                    -1.
                };
                let eye = if axis[0] != 0. && axis[1] != 0. {
                    [0, 1, 2].map(|k| {
                        (p.view.eye[k]
                            + sign * (axis[0] * p.view.right[k] + axis[1] * p.view.up[k]))
                            / 3f64.sqrt()
                    })
                } else {
                    [0, 1, 2].map(|k| sign * (axis[0] * p.view.right[k] + axis[1] * p.view.up[k]))
                };
                let up = if axis[0] == 0. {
                    [0, 1, 2].map(|k| -sign * axis[1] * p.view.eye[k])
                } else {
                    p.view.up
                };
                let view = View::new(eye, up).ok_or("投影の向きを作れません")?;
                let d = axis[0].hypot(axis[1]);
                (
                    view,
                    [
                        p.at[0] + axis[0] / d * v.at[0],
                        p.at[1] + axis[1] / d * v.at[0],
                    ],
                    p.bodies.clone(),
                )
            }
            ViewKind::Section { line, .. } => {
                let p = parent.unwrap();
                let d = dist(line[0], line[1]);
                let axis = [
                    (line[1][1] - line[0][1]) / d,
                    -(line[1][0] - line[0][0]) / d,
                ];
                let normal = [0, 1, 2].map(|k| axis[0] * p.view.right[k] + axis[1] * p.view.up[k]);
                let up = p.view.eye;
                let view = View::new(normal, up).ok_or("断面の向きを作れません")?;
                (
                    view,
                    [p.at[0] + axis[0] * v.at[0], p.at[1] + axis[1] * v.at[0]],
                    p.bodies.clone(),
                )
            }
            ViewKind::Detail {
                center,
                radius,
                scale: ratio,
                letter,
                ..
            } => {
                let p = parent.unwrap();
                let detail_scale = ratio.value()?;
                let factor = detail_scale / p.scale;
                let c = [p.at[0] + center[0] * p.scale, p.at[1] + center[1] * p.scale];
                let r = radius * p.scale;
                let f = |q: [f64; 2]| {
                    [
                        v.at[0] + (q[0] - c[0]) * factor,
                        v.at[1] + (q[1] - c[1]) * factor,
                    ]
                };
                let mut items = vec![];
                for item in &p.items {
                    if let Item::Stroke {
                        pen, line, path, ..
                    } = item
                    {
                        for path in clip_circle(path, c, r) {
                            items.push(stroke(&v.id, *pen, *line, transform(&path, f, factor)));
                        }
                    } else if let Item::Hatch {
                        loops,
                        angle_deg,
                        spacing_mm,
                        pen,
                        ..
                    } = item
                    {
                        let loops = loops
                            .iter()
                            .map(|l| clip_loop_circle(l, c, r, 0.005 / factor))
                            .filter(|l| !l.is_empty())
                            .map(|l| l.into_iter().map(f).collect())
                            .collect::<Vec<_>>();
                        if !loops.is_empty() {
                            items.push(Item::Hatch {
                                loops,
                                angle_deg: *angle_deg,
                                spacing_mm: *spacing_mm,
                                pen: *pen,
                                owner: v.id.clone(),
                            });
                        }
                    }
                }
                let mut lines = vec![];
                for line in &p.lines {
                    for path in clip_circle(&line.path, c, r) {
                        lines.push(ViewLine {
                            key: line.key.clone(),
                            path: transform(&path, f, factor),
                            visible: line.visible,
                        });
                    }
                }
                text(
                    &mut items,
                    &v.id,
                    &format!("{letter} ({}:{})", ratio.num, ratio.den),
                    [v.at[0], v.at[1] - r * factor - 6.],
                );
                return Ok(Picture {
                    view: p.view,
                    at: v.at,
                    scale: detail_scale,
                    bodies: p.bodies.clone(),
                    items,
                    lines,
                });
            }
        };
        if parent.is_some() {
            for b in &mut bodies {
                let mut mesh = cx
                    .kernel
                    .mesh(b.shape, tol)
                    .map_err(|e| Problem::new(ProblemKind::Compute, e.to_string()))?;
                b.cut_lines.clear();
                for cut in &b.cuts {
                    let (clipped, loops) = cut
                        .mesh(&mesh)
                        .map_err(|e| Problem::new(ProblemKind::Compute, e))?;
                    mesh = clipped;
                    b.cut_lines.extend(loops);
                }
                b.mesh = mesh;
            }
        }
        let mut hatches = vec![];
        if let ViewKind::Section { line, uncut, .. } = &v.kind {
            if uncut.iter().any(|id| !bodies.iter().any(|b| b.id == *id)) {
                return Err(Problem::new(
                    ProblemKind::Compute,
                    "切らないボディの参照が切れています",
                ));
            }
            let p = parent.unwrap();
            let origin =
                [0, 1, 2].map(|k| line[0][0] * p.view.right[k] + line[0][1] * p.view.up[k]);
            let cut = Cut {
                origin,
                normal: view.eye,
            };
            for (i, b) in bodies.iter_mut().enumerate() {
                if uncut.contains(&b.id) {
                    continue;
                }
                let (mesh, loops) = cut
                    .mesh(&b.mesh)
                    .map_err(|e| Problem::new(ProblemKind::Compute, e))?;
                b.mesh = mesh;
                b.cuts.push(cut);
                b.cut_lines.extend(loops.clone());
                if !loops.is_empty() {
                    hatches.push(Item::Hatch {
                        loops: loops
                            .iter()
                            .map(|l| {
                                canonical_loop(
                                    l.iter().map(|p| to_paper(&view, at, scale, *p)).collect(),
                                )
                            })
                            .collect(),
                        angle_deg: if i % 2 == 0 { 45. } else { 135. },
                        spacing_mm: 3.,
                        pen: Pen::Thin,
                        owner: v.id.clone(),
                    });
                }
            }
        }
        let (mut items, lines) = project(&bodies, &view, at, scale, &v.options, &v.id, tol);
        items.extend(hatches);
        if let ViewKind::Section { letter, .. } = &v.kind {
            let y = items
                .iter()
                .filter_map(|i| {
                    if let Item::Stroke { path, .. } = i {
                        Some(
                            path_points(path, 0.1)
                                .into_iter()
                                .map(|p| p[1])
                                .fold(f64::INFINITY, f64::min),
                        )
                    } else {
                        None
                    }
                })
                .fold(at[1], f64::min);
            text(
                &mut items,
                &v.id,
                &format!("{letter}-{letter}"),
                [at[0], y - 6.],
            );
        }
        Ok(Picture {
            view,
            at,
            scale,
            bodies,
            items,
            lines,
        })
    })();
    done.insert(v.id.clone(), result);
}
fn load(
    cx: &Context<'_>,
    source: &Source,
    tol: f64,
    target: &str,
    problems: &mut Vec<DrawingProblem>,
) -> Result<Vec<Body>, Problem> {
    let selected = match source {
        Source::All => None,
        Source::Bodies { bodies } => Some(bodies.clone()),
        Source::Component { id } => {
            if cx.data.component(*id).is_none() {
                return Err("元の部品がありません".into());
            }
            Some(cx.data.component_bodies(*id))
        }
    };
    let mut all = cx
        .visible_bodies()
        .into_iter()
        .map(|(id, h)| (kmb_core::body_root(cx.data, cx.registry, id), h))
        .collect::<Vec<_>>();
    all.sort_by_key(|x| x.0);
    if let Some(ids) = &selected {
        if ids.is_empty() || ids.iter().any(|id| !all.iter().any(|(b, _)| b == id)) {
            return Err(Problem::new(
                ProblemKind::Compute,
                "元のボディがありません。参照を選び直してください",
            ));
        }
        all.retain(|(id, _)| ids.contains(id));
    }
    if all.is_empty() {
        return Err(Problem::new(
            ProblemKind::Compute,
            "図面にするボディがありません",
        ));
    }
    let mut bodies = vec![];
    for (id, h) in all {
        let mesh = cx
            .kernel
            .mesh(h, tol)
            .map_err(|e| Problem::new(ProblemKind::Compute, e.to_string()))?;
        let mut edges = vec![];
        for list in [
            cx.kernel.edges(h),
            cx.kernel.arc_edges(h),
            cx.kernel.curve_edges(h),
        ]
        .into_iter()
        .flatten()
        {
            edges.extend(list);
        }
        // 同じ辺を複数の口が返しても一つにする。別の位置の辺は残す。
        let mut seen = BTreeSet::new();
        edges.retain_mut(|e| {
            e.faces.sort();
            let mut pts = e
                .polyline(32)
                .iter()
                .map(|p| p.map(|v| (v * 1e6).round() as i64))
                .collect::<Vec<_>>();
            if pts.first() > pts.last() {
                pts.reverse();
            }
            seen.insert((e.faces.clone(), pts))
        });
        let named = !edges.is_empty() && mesh_edges_covered(&mesh, &edges, tol);
        if !named {
            problems.push(DrawingProblem{target:target.into(),problem:Problem::new(ProblemKind::Unavailable,format!("ボディ {} は名前付きの辺を取得できません。網の線を使うため寸法を付けられません",id.0)),warning:true});
        }
        let names: Vec<String> = edges.iter().map(|e| e.faces.join("\0")).collect();
        let reps = edges
            .iter()
            .map(|e| {
                let p = e.polyline(32);
                let n = p.len() as f64;
                [0, 1, 2].map(|k| p.iter().map(|p| p[k]).sum::<f64>() / n)
            })
            .collect::<Vec<_>>();
        let unique = kmb_core::kernel::dedupe_names(names.clone(), &reps);
        let mut edges = edges
            .into_iter()
            .zip(unique)
            .enumerate()
            .map(|(i, (e, n))| {
                let index = if n == names[i] {
                    0
                } else {
                    n.rsplit_once('#')
                        .and_then(|(_, i)| i.parse().ok())
                        .unwrap_or(0)
                };
                (
                    EdgeKey {
                        body: id,
                        faces: e.faces.clone(),
                        index,
                    },
                    e,
                )
            })
            .collect::<Vec<_>>();
        edges.sort_by(|a, b| a.0.cmp(&b.0));
        bodies.push(Body {
            id,
            mesh,
            edges,
            named,
            cuts: vec![],
            cut_lines: vec![],
            shape: h,
        });
    }
    Ok(bodies)
}
/// 網にある辺の一部しかカーネルが列挙できない場合も、網に戻して線を欠かさない。
fn mesh_edges_covered(mesh: &Mesh, edges: &[EdgeInfo], tol: f64) -> bool {
    let polys = edges
        .iter()
        .map(|e| edge_polyline(e, tol))
        .collect::<Vec<_>>();
    mesh.edges.iter().all(|e| {
        e.iter().step_by((e.len() / 16).max(1)).all(|p| {
            let p = p.map(f64::from);
            polys.iter().any(|line| {
                line.windows(2).any(|w| {
                    let d = sub(w[1], w[0]);
                    let q = sub(p, w[0]);
                    let dd = dot(d, d);
                    let t = if dd > 1e-20 {
                        (dot(q, d) / dd).clamp(0., 1.)
                    } else {
                        0.
                    };
                    let delta = sub(p, lerp(w[0], w[1], t));
                    dot(delta, delta) <= (3. * tol + 1e-4).powi(2)
                })
            })
        })
    })
}
fn edge_polyline(e: &EdgeInfo, tol: f64) -> Vec<P3> {
    let n = e
        .arc
        .map(|a| {
            let angle = 2. * (1. - tol / a.radius).clamp(-1., 1.).acos();
            (a.sweep.abs() / angle.max(1e-6)).ceil().clamp(4., 65536.) as usize
        })
        .unwrap_or(32);
    e.polyline(n)
}
fn to_paper(view: &View, at: [f64; 2], scale: f64, p: P3) -> [f64; 2] {
    let q = view.project(p);
    [at[0] + q[0] * scale, at[1] + q[1] * scale]
}
fn stroke(owner: &str, pen: Pen, line: LineStyle, path: Path) -> Item {
    Item::Stroke {
        pen,
        line,
        path: canonical_path(path),
        owner: owner.into(),
    }
}
fn poly_path(p: Vec<[f64; 2]>) -> Path {
    if p.len() == 2 {
        Path::Line { a: p[0], b: p[1] }
    } else {
        Path::Poly { points: p }
    }
}
fn project(
    bodies: &[Body],
    view: &View,
    at: [f64; 2],
    scale: f64,
    options: &ViewOptions,
    owner: &str,
    tol: f64,
) -> (Vec<Item>, Vec<ViewLine>) {
    let meshes = bodies.iter().map(|b| b.mesh.clone()).collect::<Vec<_>>();
    let projector = Projector::new(&meshes, view, tol);
    let mut items = vec![];
    let mut table = vec![];
    let to = |q: [f64; 2]| [at[0] + q[0] * scale, at[1] + q[1] * scale];
    for b in bodies {
        let mut lines: Vec<(Option<LineKey>, Vec<P3>, Option<&EdgeInfo>)> = vec![];
        if b.named {
            for (key, e) in &b.edges {
                if e.seam == Seam::Smooth && !options.tangent {
                    continue;
                }
                let mut polys = vec![edge_polyline(e, tol)];
                for cut in &b.cuts {
                    polys = polys
                        .into_iter()
                        .flat_map(|p| {
                            p.windows(2)
                                .filter_map(|w| cut.segment(w[0], w[1]))
                                .map(|s| s.to_vec())
                                .collect::<Vec<_>>()
                        })
                        .collect();
                }
                for p in polys {
                    lines.push((Some(LineKey::Edge { key: key.clone() }), p, Some(e)));
                }
            }
        } else {
            lines.extend(
                b.mesh
                    .edges
                    .iter()
                    .map(|p| (None, p.iter().map(|p| p.map(f64::from)).collect(), None)),
            );
        }
        lines.extend(b.cut_lines.iter().cloned().map(|p| (None, p, None)));
        lines.extend(
            silhouette_lines(&b.mesh, view)
                .into_iter()
                .map(|(face, p)| {
                    (
                        if b.named {
                            face.map(|face| LineKey::Silhouette { body: b.id, face })
                        } else {
                            None
                        },
                        p,
                        None,
                    )
                }),
        );
        for (key, p, e) in lines {
            let runs = projector.lines(&[p]);
            for (points, visible) in &runs {
                let path = if let Some(e) = e
                    && let Some(arc) = e.arc
                    && b.cuts.is_empty()
                    && dot(arc.axis, view.eye).abs() > 1. - 1e-8
                {
                    let c = view.project(arc.center);
                    let sign = dot(arc.axis, view.eye).signum();
                    let mut sweep = 0.;
                    for w in points.windows(2) {
                        let a = (w[0][1] - c[1]).atan2(w[0][0] - c[0]);
                        let b = (w[1][1] - c[1]).atan2(w[1][0] - c[0]);
                        let d = (b - a + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
                            - std::f64::consts::PI;
                        sweep += d;
                    }
                    if runs.len() == 1 {
                        sweep = arc.sweep * sign;
                    }
                    Path::Arc {
                        c: to([c[0], c[1]]),
                        r: arc.radius * scale,
                        start_deg: (points[0][1] - c[1])
                            .atan2(points[0][0] - c[0])
                            .to_degrees(),
                        sweep_deg: sweep.to_degrees(),
                    }
                } else {
                    poly_path(points.iter().map(|p| to(*p)).collect())
                };
                let path = canonical_path(path);
                if let Some(key) = &key {
                    table.push(ViewLine {
                        key: key.clone(),
                        path: path.clone(),
                        visible: *visible,
                    });
                }
                if *visible || options.hidden {
                    let pen = if !visible || e.is_some_and(|e| e.seam == Seam::Smooth) {
                        Pen::Thin
                    } else {
                        Pen::Thick
                    };
                    items.push(stroke(
                        owner,
                        pen,
                        if *visible {
                            LineStyle::Solid
                        } else {
                            LineStyle::Dashed
                        },
                        path,
                    ));
                }
            }
        }
    }
    // 同じ投影線は一度だけ描き、見える線を優先する。参照の表は辺ごとに残す。
    let mut seen = BTreeMap::new();
    let mut keep = vec![true; items.len()];
    for (i, item) in items.iter().enumerate() {
        if let Item::Stroke {
            path, line, pen, ..
        } = item
        {
            let mut pts = if let Path::Arc {
                c, r, sweep_deg, ..
            } = path
                && sweep_deg.abs() >= 360. - 1e-8
            {
                vec![
                    [i64::MIN, 0],
                    c.map(|n| (n * 1e5).round() as i64),
                    [(r * 1e5).round() as i64, 0],
                ]
            } else {
                path_points(path, 0.005)
                    .into_iter()
                    .map(|p| p.map(|n| (n * 1e5).round() as i64))
                    .collect::<Vec<_>>()
            };
            let rev = pts.iter().rev().copied().collect::<Vec<_>>();
            if rev < pts {
                pts = rev;
            }
            let rank = if *line == LineStyle::Solid { 2 } else { 0 } + u8::from(*pen != Pen::Thin);
            if let Some((old, old_rank)) = seen.get(&pts).copied() {
                if rank > old_rank {
                    keep[old] = false;
                    seen.insert(pts, (i, rank));
                } else {
                    keep[i] = false;
                }
            } else {
                seen.insert(pts, (i, rank));
            }
        }
    }
    (
        items
            .into_iter()
            .zip(keep)
            .filter_map(|(i, k)| k.then_some(i))
            .collect(),
        table,
    )
}

/// 小さな輪郭文字。用紙の文字も Fill で渡し、表示側のフォントに依存させない。
fn text(items: &mut Vec<Item>, owner: &str, s: &str, at: [f64; 2]) {
    let mut loops = vec![];
    for (i, c) in s.chars().enumerate() {
        let rows: [u8; 7] = match c {
            'A' => [14, 17, 17, 31, 17, 17, 17],
            'B' => [30, 17, 17, 30, 17, 17, 30],
            'C' => [14, 17, 16, 16, 16, 17, 14],
            'D' => [30, 17, 17, 17, 17, 17, 30],
            'E' => [31, 16, 16, 30, 16, 16, 31],
            'F' => [31, 16, 16, 30, 16, 16, 16],
            'G' => [14, 17, 16, 23, 17, 17, 14],
            'H' => [17, 17, 17, 31, 17, 17, 17],
            'I' => [14, 4, 4, 4, 4, 4, 14],
            'J' => [7, 2, 2, 2, 18, 18, 12],
            'K' => [17, 18, 20, 24, 20, 18, 17],
            'L' => [16, 16, 16, 16, 16, 16, 31],
            'M' => [17, 27, 21, 21, 17, 17, 17],
            'N' => [17, 25, 21, 19, 17, 17, 17],
            'O' => [14, 17, 17, 17, 17, 17, 14],
            'P' => [30, 17, 17, 30, 16, 16, 16],
            'Q' => [14, 17, 17, 17, 21, 18, 13],
            'R' => [30, 17, 17, 30, 20, 18, 17],
            'S' => [15, 16, 16, 14, 1, 1, 30],
            'T' => [31, 4, 4, 4, 4, 4, 4],
            'U' => [17, 17, 17, 17, 17, 17, 14],
            'V' => [17, 17, 17, 17, 17, 10, 4],
            'W' => [17, 17, 17, 21, 21, 27, 17],
            'X' => [17, 17, 10, 4, 10, 17, 17],
            'Y' => [17, 17, 10, 4, 4, 4, 4],
            'Z' => [31, 1, 2, 4, 8, 16, 31],
            '0' => [14, 17, 19, 21, 25, 17, 14],
            '1' => [4, 12, 4, 4, 4, 4, 14],
            '2' => [14, 17, 1, 2, 4, 8, 31],
            '3' => [30, 1, 1, 14, 1, 1, 30],
            '4' => [2, 6, 10, 18, 31, 2, 2],
            '5' => [31, 16, 16, 30, 1, 1, 30],
            '6' => [14, 16, 16, 30, 17, 17, 14],
            '7' => [31, 1, 2, 4, 8, 8, 8],
            '8' => [14, 17, 17, 14, 17, 17, 14],
            '9' => [14, 17, 17, 15, 1, 1, 14],
            '-' => [0, 0, 0, 31, 0, 0, 0],
            ':' => [0, 4, 4, 0, 4, 4, 0],
            '(' => [2, 4, 8, 8, 8, 4, 2],
            ')' => [8, 4, 2, 2, 2, 4, 8],
            '.' => [0, 0, 0, 0, 0, 6, 6],
            _ => [0; 7],
        };
        for (y, row) in rows.iter().enumerate() {
            for x in 0..5 {
                if row & (1 << (4 - x)) != 0 {
                    let a = [
                        at[0] + i as f64 * 3. + x as f64 * 0.45,
                        at[1] + (6 - y) as f64 * 0.45,
                    ];
                    loops.push(vec![
                        a,
                        [a[0] + 0.45, a[1]],
                        [a[0] + 0.45, a[1] + 0.45],
                        [a[0], a[1] + 0.45],
                        a,
                    ]);
                }
            }
        }
    }
    items.push(Item::Fill {
        loops,
        owner: owner.into(),
    });
}
