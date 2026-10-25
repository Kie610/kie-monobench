//! ボディをスケッチの平面で切った断面(網羅版 K2 の「交差」と A6 の「スライス」)。
//! ボディの三角形の網を平面で切って線分を集め、つなげてから、面ごとに線分・円弧・円へ直す
//! (平らな面は線分、円筒は円弧か円になる。どちらにも合わなければ短い線分の並び)。

use crate::geom::{angle_of, cross, len, sub};
use crate::import::Shape;
use crate::model::ProjectionSource;
use crate::reference::{RefItem, RefShape};
use kmb_core::{Mesh, Plane};
use std::collections::HashMap;

type P2 = [f64; 2];

/// 網の点が平面の上にあるとみなす距離(mm)。
const ON: f64 = 1e-7;
/// 線分・円に合うとみなすずれ(mm)。網の細かさ(0.01 mm)より少し大きく
pub const SECTION_TOL: f64 = 0.03;

fn key(p: P2) -> (i64, i64) {
    ((p[0] * 1e5).round() as i64, (p[1] * 1e5).round() as i64)
}

/// 網を平面で切った線分(スケッチの座標)と、その三角形の面の番号。平面に乗った三角形は除く。
pub fn section_segments(mesh: &Mesh, plane: &Plane) -> Vec<([P2; 2], u32)> {
    let n = plane.normal();
    let d3 = |p: [f64; 3]| {
        (p[0] - plane.origin[0]) * n[0]
            + (p[1] - plane.origin[1]) * n[1]
            + (p[2] - plane.origin[2]) * n[2]
    };
    let pos = |i: u32| mesh.positions[i as usize].map(f64::from);
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (k, t) in mesh.triangles.iter().enumerate() {
        let v = t.map(pos);
        let d = v.map(d3);
        if d.iter().all(|x| x.abs() <= ON) {
            continue;
        }
        let mut pts: Vec<[f64; 3]> = Vec::new();
        let mut push = |p: [f64; 3]| {
            if !pts
                .iter()
                .any(|q| (0..3).all(|i| (q[i] - p[i]).abs() < 1e-9))
            {
                pts.push(p);
            }
        };
        for i in 0..3 {
            let j = (i + 1) % 3;
            if d[i].abs() <= ON {
                push(v[i]);
            }
            if (d[i] > ON && d[j] < -ON) || (d[i] < -ON && d[j] > ON) {
                let s = d[i] / (d[i] - d[j]);
                push([0, 1, 2].map(|c| v[i][c] + (v[j][c] - v[i][c]) * s));
            }
        }
        if pts.len() != 2 {
            continue;
        }
        let (a, b) = (plane.to_local(pts[0]), plane.to_local(pts[1]));
        if len(sub(a, b)) < 1e-9 {
            continue;
        }
        // 平面に乗った辺は両側の三角形から出るので 1 本にする
        let (ka, kb) = (key(a), key(b));
        if !seen.insert(if ka < kb { (ka, kb) } else { (kb, ka) }) {
            continue;
        }
        let face = mesh.triangle_faces.get(k).copied().unwrap_or(0);
        out.push(([a, b], face));
    }
    out
}

/// 線分をつなげた列(点と、点 i→i+1 の線分の面の番号)。閉じた列は最初と最後の点が同じ。
fn chains(segs: &[([P2; 2], u32)]) -> Vec<(Vec<P2>, Vec<u32>)> {
    let mut at: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, (s, _)) in segs.iter().enumerate() {
        at.entry(key(s[0])).or_default().push(i);
        at.entry(key(s[1])).or_default().push(i);
    }
    let mut used = vec![false; segs.len()];
    let mut out = Vec::new();
    // 端(つながる線分が 1 本の点)から始める列を先に、残りは閉じた列
    let mut starts: Vec<usize> = (0..segs.len())
        .filter(|&i| {
            segs[i]
                .0
                .iter()
                .any(|p| at.get(&key(*p)).is_some_and(|v| v.len() == 1))
        })
        .collect();
    starts.extend(0..segs.len());
    for s in starts {
        if used[s] {
            continue;
        }
        used[s] = true;
        let (seg, face) = segs[s];
        // 端から始めるなら、端の側を先に
        let lone = |p: P2| at.get(&key(p)).is_some_and(|v| v.len() == 1);
        let (first, mut cur) = if lone(seg[1]) && !lone(seg[0]) {
            (seg[1], seg[0])
        } else {
            (seg[0], seg[1])
        };
        let mut pts = vec![first, cur];
        let mut faces = vec![face];
        loop {
            let next = at
                .get(&key(cur))
                .and_then(|v| v.iter().copied().find(|&i| !used[i]));
            let Some(i) = next else { break };
            used[i] = true;
            let (seg, face) = segs[i];
            cur = if key(seg[0]) == key(cur) {
                seg[1]
            } else {
                seg[0]
            };
            pts.push(cur);
            faces.push(face);
        }
        out.push((pts, faces));
    }
    out
}

/// 点の列のずれ(`a`→`b` の直線から)の最大。
fn off_line(pts: &[P2], a: P2, b: P2) -> f64 {
    let ab = sub(b, a);
    let l = len(ab);
    pts.iter()
        .map(|p| {
            if l < 1e-12 {
                len(sub(*p, a))
            } else {
                (cross(ab, sub(*p, a)) / l).abs()
            }
        })
        .fold(0.0, f64::max)
}

/// 点の列を、ずれ `tol` 以内でできるだけ長い線分に分ける。
fn lines_of(pts: &[P2], tol: f64, out: &mut Vec<Shape>) {
    let mut i = 0;
    while i + 1 < pts.len() {
        let mut j = i + 1;
        while j + 1 < pts.len() && off_line(&pts[i..=j + 1], pts[i], pts[j + 1]) <= tol {
            j += 1;
        }
        out.push(Shape::Line {
            a: pts[i],
            b: pts[j],
        });
        i = j;
    }
}

/// 1 つの面の点の列を、線分・円弧・円にする。
pub(crate) fn fit(pts: &[P2], closed: bool, tol: f64, out: &mut Vec<Shape>) {
    let (a, b) = (pts[0], pts[pts.len() - 1]);
    if !closed && off_line(pts, a, b) <= tol {
        out.push(Shape::Line { a, b });
        return;
    }
    if pts.len() >= 4 {
        let n = pts.len() - 1;
        let (p, q, r) = if closed {
            (pts[0], pts[n / 3], pts[2 * n / 3])
        } else {
            (a, pts[n / 2], b)
        };
        if let Some(c) = crate::circumcenter(p, q, r) {
            let radius = len(sub(p, c));
            let worst = pts
                .iter()
                .map(|x| (len(sub(*x, c)) - radius).abs())
                .fold(0.0, f64::max);
            // 円弧の進む向きが揃っている(折り返していない)
            let turns = pts
                .windows(2)
                .map(|w| cross(sub(w[0], c), sub(w[1], c)))
                .collect::<Vec<_>>();
            let ccw = turns.iter().all(|t| *t >= -1e-12);
            let cw = turns.iter().all(|t| *t <= 1e-12);
            if worst <= tol && radius > tol && (ccw || cw) {
                if closed {
                    out.push(Shape::Circle { center: c, radius });
                } else {
                    let (s, e) = if ccw { (a, b) } else { (b, a) };
                    out.push(Shape::Arc {
                        center: c,
                        radius,
                        start: angle_of(c, s),
                        end: angle_of(c, e),
                    });
                }
                return;
            }
        }
    }
    lines_of(pts, tol, out);
}

/// 網を平面で切った断面を、線分・円弧・円にする。
pub fn section(mesh: &Mesh, plane: &Plane) -> Vec<Shape> {
    section_pieces(mesh, plane)
        .into_iter()
        .map(|(s, _)| s)
        .collect()
}

/// 網を平面で切った断面を、線分・円弧・円と、それを切り出した面の番号にする
/// (面の名前は `mesh.face_names`)。1 つの円にまとめた円弧は、最初の円弧の面。
pub fn section_pieces(mesh: &Mesh, plane: &Plane) -> Vec<(Shape, u32)> {
    let mut out = Vec::new();
    for (mut pts, mut faces) in chains(&section_segments(mesh, plane)) {
        let closed = pts.len() > 2 && key(pts[0]) == key(pts[pts.len() - 1]);
        // 閉じた列は面の変わり目から始める(1 つの面が 2 つに割れないように)
        if closed && let Some(k) = (1..faces.len()).find(|&k| faces[k] != faces[k - 1]) {
            pts.pop();
            pts.rotate_left(k);
            let first = pts[0];
            pts.push(first);
            faces.rotate_left(k);
        }
        let one_face = faces.iter().all(|f| *f == faces[0]);
        let mut pieces = Vec::new();
        let mut owner = Vec::new();
        let mut i = 0;
        while i < faces.len() {
            let mut j = i;
            while j + 1 < faces.len() && faces[j + 1] == faces[i] {
                j += 1;
            }
            fit(
                &pts[i..=j + 1],
                closed && one_face,
                SECTION_TOL,
                &mut pieces,
            );
            owner.resize(pieces.len(), faces[i]);
            i = j + 1;
        }
        // 円筒が幾つかの面に割れている形(truck のカーネルなど)では、閉じた列が同じ中心と
        // 半径の円弧だけになる。その時は 1 つの円にする
        match closed.then(|| one_circle(&pieces)).flatten() {
            // 持ち主は名前の一番小さい面(割れた面のどちらが先に来ても同じ元になる)
            Some(c) => {
                let first = owner
                    .iter()
                    .copied()
                    .min_by_key(|f| face_name(mesh, *f))
                    .unwrap_or(owner[0]);
                out.push((c, first))
            }
            None => out.extend(pieces.into_iter().zip(owner)),
        }
    }
    out
}

/// 断面を、スケッチに写せる参照にする(交差。元は面の名前と、面ごとの何番目か)。
/// 計算の時に同じ網の細かさ([`crate::BODY_MESH_TOL`])で求め直し、位置を取り直す。
pub fn section_refs(mesh: &Mesh, plane: &Plane, feature: kmb_core::FeatureId) -> Vec<RefItem> {
    let mut count: HashMap<u32, u32> = HashMap::new();
    section_pieces(mesh, plane)
        .into_iter()
        .filter_map(|(shape, face)| {
            let shape = ref_shape(&shape)?;
            let n = count.entry(face).or_insert(0);
            let index = *n;
            *n += 1;
            Some(RefItem {
                source: ProjectionSource::Section {
                    feature,
                    face: face_name(mesh, face),
                    index,
                },
                shape,
            })
        })
        .collect()
}

/// 網の面の番号の名前(名前が無ければ番号)。
pub(crate) fn face_name(mesh: &Mesh, face: u32) -> String {
    mesh.face_names
        .get(face as usize)
        .cloned()
        .unwrap_or_else(|| format!("#{face}"))
}

/// 線分・円弧・円を参照の形にする(ほかの形は写さない)。
pub(crate) fn ref_shape(s: &Shape) -> Option<RefShape> {
    let at = |c: P2, r: f64, t: f64| [c[0] + r * t.cos(), c[1] + r * t.sin()];
    match *s {
        Shape::Line { a, b } => Some(RefShape::Line(a, b)),
        Shape::Circle { center, radius } => Some(RefShape::Circle {
            center,
            radius,
            arc: None,
        }),
        Shape::Arc {
            center,
            radius,
            start,
            end,
        } => Some(RefShape::Circle {
            center,
            radius,
            arc: ((end - start).abs() > 1e-12)
                .then(|| (at(center, radius, start), at(center, radius, end))),
        }),
        _ => None,
    }
}

/// 円弧がすべて同じ中心と半径なら、その円。
fn one_circle(pieces: &[Shape]) -> Option<Shape> {
    let (c0, r0) = match pieces.first()? {
        Shape::Arc { center, radius, .. } => (*center, *radius),
        _ => return None,
    };
    if pieces.len() < 2 {
        return None;
    }
    pieces
        .iter()
        .all(|p| match p {
            Shape::Arc { center, radius, .. } => {
                len(sub(*center, c0)) <= SECTION_TOL && (radius - r0).abs() <= SECTION_TOL
            }
            _ => false,
        })
        .then_some(Shape::Circle {
            center: c0,
            radius: r0,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arcs_of_one_circle_split_into_faces_become_one_circle() {
        let arc = |start: f64, end: f64| Shape::Arc {
            center: [30.0, 20.0],
            radius: 4.0,
            start,
            end,
        };
        let pi = std::f64::consts::PI;
        assert_eq!(
            one_circle(&[arc(0.0, pi), arc(pi, 2.0 * pi)]),
            Some(Shape::Circle {
                center: [30.0, 20.0],
                radius: 4.0
            })
        );
        // 違う半径や線分が混じれば円にしない
        let other = Shape::Arc {
            center: [30.0, 20.0],
            radius: 5.0,
            start: pi,
            end: 2.0 * pi,
        };
        assert_eq!(one_circle(&[arc(0.0, pi), other]), None);
        let line = Shape::Line {
            a: [0.0, 0.0],
            b: [1.0, 0.0],
        };
        assert_eq!(one_circle(&[arc(0.0, pi), line]), None);
    }
}
