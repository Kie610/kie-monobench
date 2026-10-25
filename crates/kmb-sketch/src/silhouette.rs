//! ボディをスケッチの平面へ写した外形(シルエット)と、平面の上の位置にある面
//! (投影の道具で「ボディのシルエット」と「面」を写す)。
//! ボディは三角形の網で扱う。網の細かさは画面と計算で [`BODY_MESH_TOL`] にそろえる。

use crate::geom::{cross, dot, len, sub};
use crate::import::Shape;
use crate::model::ProjectionSource;
use crate::reference::{RefItem, RefShape};
use crate::section::{SECTION_TOL, fit, ref_shape};
use i_overlay::core::fill_rule::FillRule;
use i_overlay::float::simplify::SimplifyShape;
use kmb_core::{FeatureId, Mesh, Plane};

type P2 = [f64; 2];

/// 断面・シルエット・面を求める網の細かさ(mm)。画面と計算で同じにし、線の数え方をそろえる。
pub const BODY_MESH_TOL: f64 = 0.01;

/// 外形を線に直す時に、ここより大きく曲がる所で分ける(網の円の 1 辺の曲がりより大きく)。
const CORNER: f64 = 30.0 * std::f64::consts::PI / 180.0;

/// 網の三角形をスケッチの平面へ写したもの(反時計回り)と、平面から手前への高さ(3 つの角)、
/// 面の番号。写すと潰れる三角形(平面に垂直な面)は除く。
fn flat_triangles(mesh: &Mesh, plane: &Plane) -> Vec<([P2; 3], [f64; 3], u32)> {
    let n = plane.normal();
    let height = |p: [f64; 3]| {
        (p[0] - plane.origin[0]) * n[0]
            + (p[1] - plane.origin[1]) * n[1]
            + (p[2] - plane.origin[2]) * n[2]
    };
    mesh.triangles
        .iter()
        .enumerate()
        .filter_map(|(k, t)| {
            let v = t.map(|i| mesh.positions[i as usize].map(f64::from));
            let mut q = v.map(|p| plane.to_local(p));
            let mut h = v.map(height);
            let area = cross(sub(q[1], q[0]), sub(q[2], q[0]));
            if area.abs() < 1e-9 {
                return None;
            }
            if area < 0.0 {
                q.swap(1, 2);
                h.swap(1, 2);
            }
            Some((q, h, mesh.triangle_faces.get(k).copied().unwrap_or(0)))
        })
        .collect()
}

/// 平面の上の位置 `p` の真上(手前)に見えるボディの面の名前と、その面の平面からの高さ
/// (手前が正)。平面に垂直な面は選ばない。
pub fn face_under(mesh: &Mesh, plane: &Plane, p: P2) -> Option<(String, f64)> {
    let mut best: Option<(f64, u32)> = None;
    for (q, h, face) in flat_triangles(mesh, plane) {
        let w = [
            cross(sub(q[2], q[1]), sub(p, q[1])),
            cross(sub(q[0], q[2]), sub(p, q[2])),
            cross(sub(q[1], q[0]), sub(p, q[0])),
        ];
        if w.iter().any(|x| *x < -1e-12) {
            continue;
        }
        let total = w[0] + w[1] + w[2];
        let at = (w[0] * h[0] + w[1] * h[1] + w[2] * h[2]) / total;
        if best.is_none_or(|(b, _)| at > b + 1e-9) {
            best = Some((at, face));
        }
    }
    best.map(|(h, f)| (crate::section::face_name(mesh, f), h))
}

/// 点 `p` から線分 `a`-`b` までの距離。
fn to_segment(p: P2, a: P2, b: P2) -> f64 {
    let ab = sub(b, a);
    let l2 = dot(ab, ab);
    let k = if l2 > 0.0 {
        (dot(sub(p, a), ab) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    len(sub(p, [a[0] + ab[0] * k, a[1] + ab[1] * k]))
}

/// 参照の線(線分・円・円弧)の上に並べた点(端を含む)。
pub fn samples(shape: &RefShape, n: usize) -> Vec<P2> {
    match *shape {
        RefShape::Point(p) => vec![p],
        RefShape::Line(a, b) => (0..=n)
            .map(|i| {
                let t = i as f64 / n as f64;
                [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
            })
            .collect(),
        RefShape::Circle {
            center,
            radius,
            arc,
        } => {
            let (t0, sweep) = match arc {
                None => (0.0, std::f64::consts::TAU),
                Some((s, e)) => {
                    let a0 = (s[1] - center[1]).atan2(s[0] - center[0]);
                    let a1 = (e[1] - center[1]).atan2(e[0] - center[0]);
                    let mut d = a1 - a0;
                    if d <= 0.0 {
                        d += std::f64::consts::TAU;
                    }
                    (a0, d)
                }
            };
            (0..=n)
                .map(|i| {
                    let t = t0 + sweep * i as f64 / n as f64;
                    [center[0] + radius * t.cos(), center[1] + radius * t.sin()]
                })
                .collect()
        }
    }
}

/// 点 `p` から参照の線までの距離(円弧は円として測る。端の外は端までの距離)。
fn to_shape(p: P2, shape: &RefShape) -> f64 {
    match *shape {
        RefShape::Point(q) => len(sub(p, q)),
        RefShape::Line(a, b) => to_segment(p, a, b),
        RefShape::Circle {
            center,
            radius,
            arc,
        } => {
            let on = (len(sub(p, center)) - radius).abs();
            match arc {
                None => on,
                Some((s, e)) => {
                    // 円弧の外の向きなら、近い方の端まで
                    let (u, v, w) = (sub(s, center), sub(e, center), sub(p, center));
                    let inside = if cross(u, v) >= 0.0 {
                        cross(u, w) >= 0.0 && cross(w, v) >= 0.0
                    } else {
                        cross(u, w) >= 0.0 || cross(w, v) >= 0.0
                    };
                    if inside {
                        on
                    } else {
                        len(sub(p, s)).min(len(sub(p, e)))
                    }
                }
            }
        }
    }
}

/// ボディ(網 `mesh`、機能 `feature`)をスケッチの平面へ写した外形を、写せる参照にする。
/// 外形に乗るボディの辺(`body` は [`crate::body_refs`] の辺)はその辺のまま写し(元の辺に
/// 付いて動く)、乗らない部分(寝た円柱の側面など)は網から線分・円弧・円に直して
/// [`ProjectionSource::Silhouette`] で持つ。
pub fn silhouette_refs(
    mesh: &Mesh,
    plane: &Plane,
    feature: FeatureId,
    body: &[RefItem],
) -> Vec<RefItem> {
    let tris: Vec<Vec<P2>> = flat_triangles(mesh, plane)
        .into_iter()
        .map(|(q, _, _)| q.to_vec())
        .collect();
    if tris.is_empty() {
        return Vec::new();
    }
    let outline: Vec<Vec<P2>> = tris
        .simplify_shape(FillRule::NonZero)
        .into_iter()
        .flatten()
        .filter(|c| c.len() >= 3)
        .collect();
    let segs: Vec<(P2, P2)> = outline
        .iter()
        .flat_map(|c| (0..c.len()).map(|i| (c[i], c[(i + 1) % c.len()])))
        .collect();
    let on_outline = |p: P2| {
        segs.iter()
            .any(|&(a, b)| to_segment(p, a, b) <= SECTION_TOL)
    };
    // 外形に乗る辺(全部の点が外形の上にある辺)。穴の上下の縁のように同じ形に写る辺は 1 つ
    let mut edges: Vec<RefItem> = Vec::new();
    for r in body.iter().filter(|r| {
        r.source.feature() == Some(feature)
            && matches!(r.source, ProjectionSource::Edge { .. })
            && !matches!(r.shape, RefShape::Point(_))
    }) {
        let pts = samples(&r.shape, 16);
        let twin = edges.iter().any(|e| {
            pts.iter().all(|p| to_shape(*p, &e.shape) <= 1e-6)
                && samples(&e.shape, 16)
                    .into_iter()
                    .all(|p| to_shape(p, &r.shape) <= 1e-6)
        });
        if !twin && pts.into_iter().all(on_outline) {
            edges.push(r.clone());
        }
    }
    let covered = |a: P2, b: P2| {
        let m = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
        edges.iter().any(|e| to_shape(m, &e.shape) <= SECTION_TOL)
    };
    // 辺の端(辺に当たらない部分の端を、ここへ寄せてつなげる)
    let ends: Vec<P2> = edges
        .iter()
        .flat_map(|e| match e.shape {
            RefShape::Line(a, b) => vec![a, b],
            RefShape::Circle {
                arc: Some((s, e)), ..
            } => vec![s, e],
            _ => Vec::new(),
        })
        .collect();
    let snap = |p: P2| {
        ends.iter()
            .copied()
            .filter(|q| len(sub(p, *q)) <= SECTION_TOL)
            .min_by(|a, b| len(sub(p, *a)).total_cmp(&len(sub(p, *b))))
            .unwrap_or(p)
    };
    let mut rest: Vec<Shape> = Vec::new();
    for c in &outline {
        let n = c.len();
        let free: Vec<bool> = (0..n).map(|i| !covered(c[i], c[(i + 1) % n])).collect();
        if free.iter().all(|f| *f) {
            let mut pts = c.clone();
            pts.push(c[0]);
            fit_runs(&pts, true, &mut rest);
            continue;
        }
        // 辺に当たる区間の後から一周たどり、当たらない区間の続きを 1 本ずつ直す
        let Some(start) = (0..n).find(|&i| !free[i]) else {
            continue;
        };
        let mut run: Vec<P2> = Vec::new();
        for k in 1..=n {
            let i = (start + k) % n;
            if free[i] {
                if run.is_empty() {
                    run.push(snap(c[i]));
                }
                run.push(c[(i + 1) % n]);
            } else if !run.is_empty() {
                let last = run.len() - 1;
                run[last] = snap(run[last]);
                fit_runs(&run, false, &mut rest);
                run.clear();
            }
        }
        if !run.is_empty() {
            let last = run.len() - 1;
            run[last] = snap(run[last]);
            fit_runs(&run, false, &mut rest);
        }
    }
    let mut out = edges;
    out.extend(
        rest.iter()
            .filter_map(ref_shape)
            // 網の外形の小さな折れ返しから出る、長さの無い線は写さない
            .filter(|s| !matches!(s, RefShape::Line(a, b) if len(sub(*a, *b)) < SECTION_TOL))
            .enumerate()
            .map(|(i, shape)| RefItem {
                source: ProjectionSource::Silhouette {
                    feature,
                    index: i as u32,
                },
                shape,
            }),
    );
    out
}

/// 点の列を、大きく曲がる所で分けてから、線分・円弧・円に直す。
fn fit_runs(pts: &[P2], closed: bool, out: &mut Vec<Shape>) {
    if pts.len() < 2 {
        return;
    }
    let turn = |i: usize| {
        let (u, v) = (sub(pts[i], pts[i - 1]), sub(pts[i + 1], pts[i]));
        cross(u, v).atan2(dot(u, v)).abs()
    };
    let mut cuts: Vec<usize> = (1..pts.len() - 1).filter(|&i| turn(i) > CORNER).collect();
    if closed && !cuts.is_empty() {
        // 閉じた列は角から始め直す(継ぎ目が角でない所に残らないように)
        let k = cuts[0];
        let mut p: Vec<P2> = pts[k..pts.len() - 1].to_vec();
        p.extend_from_slice(&pts[..=k]);
        let m = pts.len() - 1;
        cuts = cuts
            .iter()
            .map(|c| (c + m - k) % m)
            .filter(|c| *c > 0)
            .collect();
        cuts.sort_unstable();
        return fit_cut(&p, &cuts, out);
    }
    if cuts.is_empty() {
        fit(pts, closed, SECTION_TOL, out);
        return;
    }
    fit_cut(pts, &cuts, out);
}

fn fit_cut(pts: &[P2], cuts: &[usize], out: &mut Vec<Shape>) {
    let mut from = 0;
    for &c in cuts.iter().chain(std::iter::once(&(pts.len() - 1))) {
        if c > from {
            fit(&pts[from..=c], false, SECTION_TOL, out);
        }
        from = c;
    }
}

impl RefShape {
    /// 点 `p` からこの形までの距離(円弧の外の向きなら近い方の端まで)。
    pub fn distance(&self, p: P2) -> f64 {
        to_shape(p, self)
    }
}
