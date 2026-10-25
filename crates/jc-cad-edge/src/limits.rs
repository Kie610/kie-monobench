//! 半径(面取りの距離)が辺の両側の面の幅に収まるかを網から測る。

use super::*;

/// 収まる最大の値 `max` から、実際に使う値(0.01 mm 単位に切り下げ、面が消えない余裕を残す)。
pub(super) fn fit_value(max: f64) -> f64 {
    ((max - FIT_MARGIN) * 100.0).floor() / 100.0
}

/// 辺ごとの、収まる最大の半径(面取りなら距離)。測れない辺は `None`。
///
/// 辺の両側の面それぞれで、辺に沿った何か所かから面の上を辺と直角に進み、面の縁までの
/// 距離(面の幅)を網から測る。丸めが面に乗る幅は r·tan(φ/2)(φ は両面の法線の間の角)、
/// 面取りは距離そのものなので、幅をそれで割った値が上限になる。面の向こうの縁も選んだ辺なら、
/// 両方の丸めで幅を分け合うので半分にする。
pub(super) fn limits(
    mesh: &k::Mesh,
    faces: &[FaceInfo],
    edges: &[EdgeInfo],
    shapes: &[Shape],
) -> Vec<Option<f64>> {
    let normal = |name: &str| {
        faces
            .iter()
            .find(|f| f.name == name)
            .and_then(|f| f.planar)
            .map(|p| p.normal)
    };
    edges
        .iter()
        .zip(shapes)
        .map(|(e, shape)| {
            let (na, nb) = (normal(&e.faces[0])?, normal(&e.faces[1])?);
            // 大きさ 1 あたりの、面 A・B の上の幅
            let (wa, wb) = shape.with_size(1.0).widths(k::dot(na, nb));
            let mut max = f64::INFINITY;
            for (face, n, other, per) in [(&e.faces[0], na, nb, wa), (&e.faces[1], nb, na, wb)] {
                if let Some(w) = face_width(mesh, face, e, n, other, edges) {
                    max = max.min(w / per);
                }
            }
            max.is_finite().then_some(max)
        })
        .collect()
}

/// 面 `face`(外向きの法線 `n`)の上を、辺 `e` から直角に進んで縁に当たるまでの距離の最小。
/// `other` は辺の向こうの面の法線。面の三角形が網になければ `None`。
fn face_width(
    mesh: &k::Mesh,
    face: &str,
    e: &EdgeInfo,
    n: P3,
    other: P3,
    edges: &[EdgeInfo],
) -> Option<f64> {
    use std::collections::HashMap;
    let fi = mesh.face_names.iter().position(|f| f == face)? as u32;
    let v = k::sub(e.b, e.a);
    let length = k::dot(v, v).sqrt();
    let d = k::scale(v, 1.0 / length);
    // 面の上を辺から離れる向き(出っ張った辺では相手の面の法線と逆、へこんだ辺では同じ側)
    let mut w = k::cross(d, n);
    if (k::dot(w, other) > 0.0) == e.convex {
        w = k::scale(w, -1.0);
    }
    let to2 = |p: [f32; 3]| {
        let q = k::sub([p[0] as f64, p[1] as f64, p[2] as f64], e.a);
        [k::dot(q, d), k::dot(q, w)]
    };
    // 面の三角形の辺のうち、1 つの三角形にしか使われないものが面の縁
    let key = |p: [f64; 2]| ((p[0] * 1e4).round() as i64, (p[1] * 1e4).round() as i64);
    // 端点の組 → (向き付きの端点, 使う三角形の数)
    type Uses = HashMap<((i64, i64), (i64, i64)), ([f64; 2], [f64; 2], i32)>;
    let mut count: Uses = HashMap::new();
    for (t, _) in mesh
        .triangles
        .iter()
        .zip(&mesh.triangle_faces)
        .filter(|(_, f)| **f == fi)
    {
        let c = t.map(|i| to2(mesh.positions[i as usize]));
        for (a, b) in [(c[0], c[1]), (c[1], c[2]), (c[2], c[0])] {
            let (ka, kb) = (key(a), key(b));
            let k = if ka <= kb { (ka, kb) } else { (kb, ka) };
            count.entry(k).or_insert((a, b, 0)).2 += 1;
        }
    }
    if count.is_empty() {
        return None;
    }
    let border: Vec<([f64; 2], [f64; 2])> = count
        .into_values()
        .filter(|(_, _, c)| *c == 1)
        .map(|(a, b, _)| (a, b))
        .collect();
    let border = outer_loop(&border, key);
    // 面の向こうの縁が、同じ面に掛ける別の選んだ辺の上にあるか
    let shared = |a: [f64; 2], b: [f64; 2]| {
        edges.iter().any(|o| {
            o != e
                && o.arc.is_none()
                && o.faces.iter().any(|f| f == face)
                && [a, b].iter().all(|p| {
                    let p3 = k::add(e.a, k::add(k::scale(d, p[0]), k::scale(w, p[1])));
                    let ov = k::sub(o.b, o.a);
                    let ol = k::dot(ov, ov).sqrt();
                    let q = k::sub(p3, o.a);
                    let along = k::dot(q, ov) / ol;
                    let off = k::sub(q, k::scale(ov, along / ol));
                    k::dot(off, off).sqrt() < 1e-3
                })
        })
    };
    let mut best: Option<f64> = None;
    for s in SAMPLES {
        let x = s * length;
        // x の位置から面の上を進み、最初に当たる縁
        let mut hit: Option<(f64, bool)> = None;
        for &(a, b) in &border {
            if (a[0] - x) * (b[0] - x) > 0.0 || (a[0] - b[0]).abs() < 1e-9 {
                continue;
            }
            let y = a[1] + (x - a[0]) / (b[0] - a[0]) * (b[1] - a[1]);
            if y > 1e-4 && hit.is_none_or(|(h, _)| y < h) {
                hit = Some((y, shared(a, b)));
            }
        }
        if let Some((y, half)) = hit {
            let y = if half { y / 2.0 } else { y };
            best = Some(best.map_or(y, |b: f64| b.min(y)));
        }
    }
    best
}

/// 面の縁のうち、外側の輪(囲む面積が最も大きい輪)だけ。穴やポケットの縁(内側の輪)は
/// 面の幅を狭めない(出っ張った辺の柱は穴の上を削っても形が変わらず、へこんだ辺の柱は
/// 穴の上を埋めない。`shadow`)。縁の向きは三角形の向きのままなので、輪ごとの符号付き面積が求まる。
fn outer_loop(
    border: &[([f64; 2], [f64; 2])],
    key: impl Fn([f64; 2]) -> (i64, i64),
) -> Vec<([f64; 2], [f64; 2])> {
    use std::collections::HashMap;
    // 端点のつながりで輪に分ける(union-find)
    let mut ids: HashMap<(i64, i64), usize> = HashMap::new();
    let mut parent: Vec<usize> = Vec::new();
    let mut id = |p: [f64; 2], parent: &mut Vec<usize>| {
        *ids.entry(key(p)).or_insert_with(|| {
            parent.push(parent.len());
            parent.len() - 1
        })
    };
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let ends: Vec<(usize, usize)> = border
        .iter()
        .map(|&(a, b)| (id(a, &mut parent), id(b, &mut parent)))
        .collect();
    for &(a, b) in &ends {
        let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
        parent[ra] = rb;
    }
    let mut area: HashMap<usize, f64> = HashMap::new();
    let roots: Vec<usize> = ends.iter().map(|&(a, _)| find(&mut parent, a)).collect();
    for (&(a, b), r) in border.iter().zip(&roots) {
        *area.entry(*r).or_default() += a[0] * b[1] - a[1] * b[0];
    }
    let Some(outer) = area
        .iter()
        .max_by(|x, y| x.1.abs().total_cmp(&y.1.abs()))
        .map(|(r, _)| *r)
    else {
        return vec![];
    };
    border
        .iter()
        .zip(&roots)
        .filter(|(_, r)| **r == outer)
        .map(|(s, _)| *s)
        .collect()
}
