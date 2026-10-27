//! 読み込んだ形の重複と、開いた端点の小さな隙間を整理する。

use kmb_core::kernel::P2;
use kmb_sketch::{Shape, WELD_TOL};
use std::collections::{BTreeMap, HashMap, HashSet};

/// 整理した形と、取り除いた形・つないだ端点の組の数。
#[derive(Debug)]
pub struct Cleaned {
    pub shapes: Vec<Shape>,
    pub removed: usize,
    pub joined: usize,
}

fn distance(a: P2, b: P2) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}
fn dot(a: P2, b: P2) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
fn cell(p: P2, tol: f64) -> (i64, i64) {
    ((p[0] / tol).floor() as i64, (p[1] / tol).floor() as i64)
}
fn neighbors((x, y): (i64, i64)) -> impl Iterator<Item = (i64, i64)> {
    (-1i64..=1)
        .flat_map(move |dx| (-1i64..=1).map(move |dy| (x.saturating_add(dx), y.saturating_add(dy))))
}

#[derive(Clone, Copy)]
struct End {
    shape: usize,
    side: usize,
    point: P2,
    arc: bool,
}

/// 同じ直線の上で重なる線をまとめ、別の形の開いた端点だけを `tolerance` mm 以内でつなぐ。
/// 端点は一度だけ組にするので、連鎖して許容差より遠くへ動かさない。
pub fn clean(shapes: &[Shape], tolerance: f64) -> Result<Cleaned, String> {
    if !tolerance.is_finite() || !(WELD_TOL..=1.0).contains(&tolerance) {
        return Err("端点をつなぐ許容差は 0.000001〜1 mm にしてください".into());
    }
    // スケッチが保持できない自由曲線は、挿入と同じ mm の許容差で区間にする。
    // 近似の途中の共有点は閉じた端点として残し、曲線の両端の隙間だけをつなぐ。
    if shapes.iter().any(|s| !s.is_finite()) {
        return Err("掃除する形の座標が正しくありません".into());
    }
    let normalized: Vec<_> = shapes
        .iter()
        .flat_map(|s| match s {
            Shape::Bezier { .. } | Shape::Ellipse { .. } | Shape::BSpline { .. } => {
                kmb_sketch::approximate(s)
            }
            _ => vec![s.clone()],
        })
        .collect();
    let shapes = normalized.as_slice();
    const EPS: f64 = 1e-9;
    type Span = (usize, P2, P2, f64, f64);
    let mut lines: BTreeMap<[i64; 3], Vec<Span>> = BTreeMap::new();
    let mut other = Vec::new();
    let mut seen = HashSet::new();
    let mut removed = 0;
    for (i, s) in shapes.iter().enumerate() {
        if let Shape::Line { a, b } = s {
            let length = distance(*a, *b);
            if length > EPS {
                let mut d = [(b[0] - a[0]) / length, (b[1] - a[1]) / length];
                if d[0] < 0.0 || (d[0] == 0.0 && d[1] < 0.0) {
                    d = d.map(|v| -v);
                }
                let offset = d[0] * a[1] - d[1] * a[0];
                let key = [d[0], d[1], offset].map(|v| (v / EPS).round() as i64);
                let (ta, tb) = (dot(*a, d), dot(*b, d));
                let (p, q, lo, hi) = if ta <= tb {
                    (*a, *b, ta, tb)
                } else {
                    (*b, *a, tb, ta)
                };
                lines.entry(key).or_default().push((i, p, q, lo, hi));
                continue;
            }
        }
        let key = serde_json::to_string(s).map_err(|e| e.to_string())?;
        if seen.insert(key) {
            other.push((i, s.clone()));
        } else {
            removed += 1;
        }
    }
    let merged = |cur: Span| {
        let Shape::Line { a, b } = &shapes[cur.0] else {
            unreachable!("線の区間")
        };
        let forward = dot(
            [b[0] - a[0], b[1] - a[1]],
            [cur.2[0] - cur.1[0], cur.2[1] - cur.1[1]],
        ) >= 0.0;
        if forward {
            Shape::Line { a: cur.1, b: cur.2 }
        } else {
            Shape::Line { a: cur.2, b: cur.1 }
        }
    };
    for spans in lines.values_mut() {
        spans.sort_by(|a, b| a.3.total_cmp(&b.3).then(a.0.cmp(&b.0)));
        let mut current: Option<Span> = None;
        for &next in spans.iter() {
            if let Some(mut cur) = current {
                let d = [cur.2[0] - cur.1[0], cur.2[1] - cur.1[1]];
                let collinear = [next.1, next.2].iter().all(|p| {
                    ((p[0] - cur.1[0]) * d[1] - (p[1] - cur.1[1]) * d[0]).abs()
                        <= EPS * distance(cur.1, cur.2)
                });
                if next.3 < cur.4 - EPS && collinear {
                    cur.0 = cur.0.min(next.0);
                    if next.4 > cur.4 {
                        cur.2 = next.2;
                        cur.4 = next.4;
                    }
                    current = Some(cur);
                    removed += 1;
                    continue;
                }
                other.push((cur.0, merged(cur)));
            }
            current = Some(next);
        }
        if let Some(cur) = current {
            other.push((cur.0, merged(cur)));
        }
    }
    other.sort_by_key(|(i, _)| *i);
    let mut shapes: Vec<_> = other.into_iter().map(|(_, s)| s).collect();
    let mut ends = Vec::new();
    for (shape, s) in shapes.iter().enumerate() {
        let (points, arc) = match s {
            Shape::Line { a, b } => ([*a, *b], false),
            Shape::Arc {
                center,
                radius,
                start,
                end,
            } => {
                let point = |a: f64| [center[0] + radius * a.cos(), center[1] + radius * a.sin()];
                ([point(*start), point(*end)], true)
            }
            _ => continue,
        };
        if arc && distance(points[0], points[1]) <= WELD_TOL {
            continue;
        }
        for (side, point) in points.into_iter().enumerate() {
            ends.push(End {
                shape,
                side,
                point,
                arc,
            });
        }
    }
    let mut exact: HashMap<_, Vec<usize>> = HashMap::new();
    let mut cells: HashMap<_, Vec<usize>> = HashMap::new();
    for (i, e) in ends.iter().enumerate() {
        exact.entry(cell(e.point, WELD_TOL)).or_default().push(i);
        cells.entry(cell(e.point, tolerance)).or_default().push(i);
    }
    let mut closed: Vec<bool> = ends
        .iter()
        .enumerate()
        .map(|(i, e)| {
            neighbors(cell(e.point, WELD_TOL))
                .flat_map(|c| exact.get(&c).into_iter().flatten())
                .any(|j| {
                    *j != i
                        && ends[*j].shape != e.shape
                        && distance(e.point, ends[*j].point) <= WELD_TOL
                })
        })
        .collect();
    let mut joined = 0;
    for (i, e) in ends.iter().enumerate() {
        if closed[i] {
            continue;
        }
        let candidate = neighbors(cell(e.point, tolerance))
            .flat_map(|c| cells.get(&c).into_iter().flatten())
            .filter(|j| **j > i && !closed[**j] && ends[**j].shape != e.shape)
            .map(|j| (*j, distance(e.point, ends[*j].point)))
            .filter(|(_, d)| *d <= tolerance)
            .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        let Some((j, _)) = candidate else { continue };
        let q = ends[j];
        let (moved, anchor) = if q.arc { (*e, q.point) } else { (q, e.point) };
        if let Shape::Line { a, b } = &shapes[moved.shape] {
            let other = if moved.side == 0 { *b } else { *a };
            if distance(other, anchor) <= WELD_TOL {
                continue;
            }
        }
        if e.arc && q.arc {
            shapes.push(Shape::Line {
                a: e.point,
                b: q.point,
            });
        } else if let Shape::Line { a, b } = &mut shapes[moved.shape] {
            if moved.side == 0 {
                *a = anchor;
            } else {
                *b = anchor;
            }
        }
        closed[i] = true;
        closed[j] = true;
        joined += 1;
    }
    Ok(Cleaned {
        shapes,
        removed,
        joined,
    })
}
