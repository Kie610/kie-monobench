//! 網の点から円・円柱を求め直す(網の弦のずれを消して径・中心・軸を出す)。

use super::*;

/// 折れ線が円・円弧か。網の点から 3 点で円を求め、全部の点が同じ平面の上で、中心から同じ距離なら円。
pub fn fit_circle(pts: &[P]) -> Option<Circle> {
    if pts.len() < 4 {
        return None;
    }
    let closed = dist(pts[0], pts[pts.len() - 1]) < 1e-6 * (1.0 + len(pts[0]));
    let n = if closed { pts.len() - 1 } else { pts.len() };
    let (a, b, c) = (pts[0], pts[n / 3], pts[2 * n / 3]);
    let (ab, ac) = (sub(b, a), sub(c, a));
    let nrm = cross(ab, ac);
    let nn = dot(nrm, nrm);
    if nn < 1e-18 {
        return None;
    }
    // 外接円の中心(3 点を通る円)
    let center = add(
        a,
        scale(
            add(
                scale(cross(nrm, ab), dot(ac, ac)),
                scale(cross(ac, nrm), dot(ab, ab)),
            ),
            0.5 / nn,
        ),
    );
    let radius = dist(a, center);
    let normal = normalize(nrm);
    // f32 の網なので、ずれの許しは大きさに合わせる
    let tol = 1e-4 * (1.0 + radius) + 2e-6 * len(center);
    if radius < 1e-6 || radius > 1e6 {
        return None;
    }
    for p in pts {
        if (dist(*p, center) - radius).abs() > tol || dot(sub(*p, center), normal).abs() > tol {
            return None;
        }
    }
    // 回った角度(隣の点との角度の和)
    let sweep: f64 = pts
        .windows(2)
        .map(|w| {
            let (u, v) = (sub(w[0], center), sub(w[1], center));
            cross(u, v)
                .iter()
                .map(|x| x * x)
                .sum::<f64>()
                .sqrt()
                .atan2(dot(u, v))
        })
        .sum();
    Some(Circle {
        center,
        normal,
        radius,
        closed,
        sweep: if closed { std::f64::consts::TAU } else { sweep },
    })
}

/// 対称な 3×3 の行列のいちばん小さい固有値の固有ベクトル(ヤコビ法)。
pub(crate) fn smallest_eigenvector(mut m: [[f64; 3]; 3]) -> P {
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..50 {
        // いちばん大きい対角でない要素を消す
        let (p, q) = [(0, 1), (0, 2), (1, 2)]
            .into_iter()
            .max_by(|a, b| m[a.0][a.1].abs().total_cmp(&m[b.0][b.1].abs()))
            .expect("3 つある");
        if m[p][q].abs() < 1e-30 {
            break;
        }
        let theta = 0.5 * (2.0 * m[p][q]).atan2(m[q][q] - m[p][p]);
        let (s, c) = theta.sin_cos();
        // m ← Jᵀ m J、v ← v J
        for row in &mut m {
            let (mkp, mkq) = (row[p], row[q]);
            row[p] = c * mkp - s * mkq;
            row[q] = s * mkp + c * mkq;
        }
        let (rp, rq) = (m[p], m[q]);
        m[p] = std::array::from_fn(|k| c * rp[k] - s * rq[k]);
        m[q] = std::array::from_fn(|k| s * rp[k] + c * rq[k]);
        for row in &mut v {
            let (vp, vq) = (row[p], row[q]);
            row[p] = c * vp - s * vq;
            row[q] = s * vp + c * vq;
        }
    }
    let k = (0..3)
        .min_by(|a, b| m[*a][*a].total_cmp(&m[*b][*b]))
        .expect("3 つある");
    normalize([v[0][k], v[1][k], v[2][k]])
}

/// `n` に垂直な 2 本の向き。
pub(crate) fn basis_of(n: P) -> (P, P) {
    let other = if n[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let u = normalize(cross(n, other));
    (u, cross(n, u))
}

/// 面の網(三角形)が円柱か。三角形の向きのいちばん外れる向きを軸にし、軸に垂直な平面へ写した点が
/// どれも同じ円の上にあれば円柱(網の点は面の上にある)。
pub fn fit_cylinder(tris: &[[P; 3]]) -> Option<Cylinder> {
    if tris.len() < 2 {
        return None;
    }
    // 三角形の向きの散らばり(面積で重み付け)。円柱ならどの向きも軸に垂直
    let mut m = [[0.0; 3]; 3];
    for t in tris {
        let n = cross(sub(t[1], t[0]), sub(t[2], t[0]));
        let a = len(n);
        if a < 1e-300 {
            continue;
        }
        for i in 0..3 {
            for j in 0..3 {
                m[i][j] += n[i] * n[j] / a;
            }
        }
    }
    let axis = smallest_eigenvector(m);
    let (u, v) = basis_of(axis);
    let pts: Vec<P> = tris.iter().flatten().copied().collect();
    let mean = scale(
        pts.iter().fold([0.0; 3], |a, p| add(a, *p)),
        1.0 / pts.len() as f64,
    );
    // 平面の上の円を最小二乗で(x² + y² + D x + E y + F = 0)
    let xy: Vec<(f64, f64)> = pts
        .iter()
        .map(|p| {
            let d = sub(*p, mean);
            (dot(d, u), dot(d, v))
        })
        .collect();
    let mut a = [[0.0; 3]; 3];
    let mut b = [0.0; 3];
    for (x, y) in &xy {
        let row = [*x, *y, 1.0];
        let r = -(x * x + y * y);
        for i in 0..3 {
            for j in 0..3 {
                a[i][j] += row[i] * row[j];
            }
            b[i] += row[i] * r;
        }
    }
    let det = |m: [[f64; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let d0 = det(a);
    if d0.abs() < 1e-300 {
        return None;
    }
    let solve = |k: usize| {
        let mut m = a;
        for i in 0..3 {
            m[i][k] = b[i];
        }
        det(m) / d0
    };
    let (dd, ee, ff) = (solve(0), solve(1), solve(2));
    let (cx, cy) = (-dd / 2.0, -ee / 2.0);
    let r2 = cx * cx + cy * cy - ff;
    if r2 <= 0.0 {
        return None;
    }
    let radius = r2.sqrt();
    let c0 = add(mean, add(scale(u, cx), scale(v, cy)));
    let tol = 1e-4 * (1.0 + radius) + 2e-6 * len(c0);
    if !(1e-6..=1e6).contains(&radius) {
        return None;
    }
    let mut angles = Vec::with_capacity(xy.len());
    for (x, y) in &xy {
        let (dx, dy) = (x - cx, y - cy);
        if ((dx * dx + dy * dy).sqrt() - radius).abs() > tol {
            return None;
        }
        angles.push(dy.atan2(dx));
    }
    let hs: Vec<f64> = pts.iter().map(|p| dot(sub(*p, c0), axis)).collect();
    let (h0, h1) = hs
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), h| {
            (a.min(*h), b.max(*h))
        });
    let length = h1 - h0;
    // 平らな帯(長さが無い)は円柱と呼ばない
    if length < tol {
        return None;
    }
    // 回った角度: 角度を並べ、いちばん大きいすき間の外側
    angles.sort_by(f64::total_cmp);
    angles.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    let mut gap = (
        std::f64::consts::TAU - (angles[angles.len() - 1] - angles[0]),
        angles[0],
    );
    for w in angles.windows(2) {
        if w[1] - w[0] > gap.0 {
            gap = (w[1] - w[0], w[1]);
        }
    }
    // 網の 1 区間はふつう 45° より細かいので、それより狭いすき間しか無ければ一周
    let closed = gap.0 < 0.8;
    let sweep = if closed {
        std::f64::consts::TAU
    } else {
        std::f64::consts::TAU - gap.0
    };
    let bulge = if closed {
        [0.0; 3]
    } else {
        let mid = gap.1 + sweep / 2.0;
        add(scale(u, mid.cos()), scale(v, mid.sin()))
    };
    Some(Cylinder {
        center: add(c0, scale(axis, (h0 + h1) / 2.0)),
        axis,
        radius,
        length,
        closed,
        sweep,
        bulge,
    })
}

/// 求めた円柱の面を細かい三角形に(一周 720 区間、軸の向きは 1 区間)。
pub fn cylinder_tris(c: &Cylinder) -> Vec<[P; 3]> {
    let (u, v) = basis_of(c.axis);
    let start = if c.closed {
        0.0
    } else {
        dot(c.bulge, v).atan2(dot(c.bulge, u)) - c.sweep / 2.0
    };
    let n = ((c.sweep / std::f64::consts::TAU) * 720.0).ceil().max(4.0) as usize;
    let half = scale(c.axis, c.length / 2.0);
    let at = |i: usize, s: f64| {
        let a = start + c.sweep * i as f64 / n as f64;
        let r = add(scale(u, c.radius * a.cos()), scale(v, c.radius * a.sin()));
        add(add(c.center, r), scale(half, s))
    };
    (0..n)
        .flat_map(|i| {
            let (a, b, cc, d) = (at(i, -1.0), at(i + 1, -1.0), at(i + 1, 1.0), at(i, 1.0));
            [[a, b, cc], [a, cc, d]]
        })
        .collect()
}
