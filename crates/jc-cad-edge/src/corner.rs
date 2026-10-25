//! 3 本の出っ張った辺が集まる角の埋め方(ローリングボールと三角の面)。

use super::*;

/// 選んだ 3 本の出っ張った辺が集まる頂点。
pub(super) struct Vertex {
    pub(super) at: P3,
    /// 3 本の辺(`edges` の添字)
    pub(super) edges: [usize; 3],
    /// 辺ごとの、頂点から辺の向こうの端への向き
    dirs: [P3; 3],
    /// 3 つの面の外向きの法線と、面ごとに乗る 2 本の辺(`edges` の中の位置 0〜2)
    faces: [(P3, [usize; 2]); 3],
}

/// 選んだ出っ張った辺のうち、ちょうど 3 本が 1 つの頂点に集まり、その 3 本が 3 つの平面の
/// 境目になっている所。
pub(super) fn corners(edges: &[EdgeInfo], faces: &[FaceInfo]) -> Vec<Vertex> {
    let normal = |name: &str| {
        faces
            .iter()
            .find(|f| f.name == name)
            .and_then(|f| f.planar)
            .map(|p| p.normal)
    };
    let near = |a: P3, b: P3| {
        let d = k::sub(a, b);
        k::dot(d, d) < 1e-10
    };
    let mut points: Vec<P3> = Vec::new();
    // 円の辺は角の形の対象にしない(交わりのまま)
    for e in edges.iter().filter(|e| e.convex && e.arc.is_none()) {
        for p in [e.a, e.b] {
            if !points.iter().any(|q| near(*q, p)) {
                points.push(p);
            }
        }
    }
    let mut out = Vec::new();
    for at in points {
        let mut ids = Vec::new();
        let mut dirs = Vec::new();
        for (i, e) in edges
            .iter()
            .enumerate()
            .filter(|(_, e)| e.convex && e.arc.is_none())
        {
            if near(e.a, at) {
                ids.push(i);
                dirs.push(k::normalize(k::sub(e.b, e.a)));
            } else if near(e.b, at) {
                ids.push(i);
                dirs.push(k::normalize(k::sub(e.a, e.b)));
            }
        }
        let (&[i0, i1, i2], &[d0, d1, d2]) = (&ids[..], &dirs[..]) else {
            continue;
        };
        let ids = [i0, i1, i2];
        let mut names: Vec<&String> = Vec::new();
        for &i in &ids {
            for f in &edges[i].faces {
                if !names.contains(&f) {
                    names.push(f);
                }
            }
        }
        if names.len() != 3 {
            continue;
        }
        let mut fs = Vec::new();
        for name in &names {
            let on: Vec<usize> = (0..3)
                .filter(|&j| edges[ids[j]].faces.contains(name))
                .collect();
            match (normal(name), &on[..]) {
                (Some(n), [a, b]) => fs.push((n, [*a, *b])),
                _ => break,
            }
        }
        let Ok(faces) = <[(P3, [usize; 2]); 3]>::try_from(fs) else {
            continue;
        };
        out.push(Vertex {
            at,
            edges: ids,
            dirs: [d0, d1, d2],
            faces,
        });
    }
    out
}

/// 3 行の連立 1 次方程式 `rows · x = rhs` を解く(クラメルの公式)。解けなければ `None`。
fn solve3(rows: [P3; 3], rhs: [f64; 3]) -> Option<P3> {
    let det = k::dot(rows[0], k::cross(rows[1], rows[2]));
    if det.abs() < 1e-12 {
        return None;
    }
    let col = |i: usize| {
        let mut m = rows;
        for (r, v) in m.iter_mut().zip(rhs) {
            r[i] = v;
        }
        k::dot(m[0], k::cross(m[1], m[2])) / det
    };
    Some([col(0), col(1), col(2)])
}

/// 平面の上の、向き `n` に直交する 2 本の軸(右手系で `x × y = n`)。
fn axes(n: P3) -> (P3, P3) {
    let t = if n[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let x = k::normalize(k::cross(t, n));
    (x, k::cross(n, x))
}

/// 点 `origin` を通り、法線 `n` の側を `len` まで埋める板(一辺 2 `half` の正方形を押し出す)。
fn slab(
    kernel: &dyn k::Kernel,
    origin: P3,
    n: P3,
    half: f64,
    len: f64,
    tag: &str,
) -> Result<k::ShapeHandle, k::KernelError> {
    let (x, y) = axes(n);
    let sq = |a: f64, b: f64| [a * half, b * half];
    let lp = ProfileLoop {
        edges: vec![
            line("q1", sq(-1.0, -1.0), sq(1.0, -1.0)),
            line("q2", sq(1.0, -1.0), sq(1.0, 1.0)),
            line("q3", sq(1.0, 1.0), sq(-1.0, 1.0)),
            line("q4", sq(-1.0, 1.0), sq(-1.0, -1.0)),
        ],
    };
    let profile = Profile {
        plane: Plane {
            origin,
            x_axis: x,
            y_axis: y,
        },
        regions: vec![Region {
            outer: lp,
            holes: vec![],
        }],
    };
    kernel.extrude(&profile, 0.0, len, tag)
}

/// 球の角: 3 つの面に接する半径 `r` の球で角をつなぐ。
///
/// 球の中心 c は 3 つの面から r 内側の点。辺ごとの丸めは、c を通り辺に直交する平面で終わる
/// (球と丸めの円筒がその平面で接する)。3 つの平面より頂点の側(角の区域)のうち、球の外を
/// 削る。辺の丸めは角の区域の中で球を削らない(球は丸めの円筒の内側にある)ので、角の区域は
/// 球だけが残る。丸めは、球の接点と中心を通る平面で終える。
///
/// 丸めの円は面から `OFFSETS` の分だけ外へずらし、辺ごとに大きさも少し変えてある
/// (`actual`)。球を円筒にちょうど接させるとブーリアンが失敗するため、球は円筒より少し
/// 大きくし(`BALL_GROW`)、中心も円と同じだけ面へ寄せる。
pub(super) fn ball_corner(
    kernel: &dyn k::Kernel,
    body: k::ShapeHandle,
    v: &Vertex,
    r: f64,
    actual: &[f64],
    tag: &str,
) -> Result<k::ShapeHandle, k::KernelError> {
    let n = [v.faces[0].0, v.faces[1].0, v.faces[2].0];
    let inset = r - OFFSETS[0];
    let rel = solve3(n, [-inset, -inset, -inset])
        .ok_or_else(|| k::KernelError::Failed("角の 3 つの面が 1 点で交わりません".into()))?;
    let c = k::add(v.at, rel);
    let big = actual.iter().cloned().fold(r, f64::max);
    let rho = big + BALL_GROW;
    let reach = 1.2 * k::dot(rel, rel).sqrt() + 0.1;
    // 角の区域: c を通り、辺ごとに辺と直交する平面より頂点の側
    let mut zone = slab(kernel, c, k::scale(v.dirs[0], -1.0), reach, reach, tag)?;
    for d in &v.dirs[1..] {
        let s = slab(kernel, c, k::scale(*d, -1.0), reach, reach, tag)?;
        zone = kernel.boolean(BooleanOp::Intersect, zone, s)?;
    }
    // 球: 半円を軸まわりに一周させる
    let (x, y) = axes(n[0]);
    let half = ProfileLoop {
        edges: vec![
            ProfileEdge {
                name: "ball".into(),
                segment: Segment::Arc {
                    center: [0.0, 0.0],
                    radius: rho,
                    start: -std::f64::consts::FRAC_PI_2,
                    end: std::f64::consts::FRAC_PI_2,
                },
                reversed: false,
            },
            line("axis", [0.0, rho], [0.0, -rho]),
        ],
    };
    let profile = Profile {
        plane: Plane {
            origin: c,
            x_axis: x,
            y_axis: y,
        },
        regions: vec![Region {
            outer: half,
            holes: vec![],
        }],
    };
    let ball = kernel.revolve(&profile, c, y, std::f64::consts::TAU, tag)?;
    let tool = kernel.boolean(BooleanOp::Subtract, zone, ball)?;
    kernel.boolean(BooleanOp::Subtract, body, tool)
}

/// 三角の面の角: 面ごとに、その面に乗る 2 本の面取りの境の線が交わる点を 3 つ求め、その
/// 3 点を通る平面より頂点の側を削る。面取りの面は三角の面で終わる。
///
/// 平面を 3 点にちょうど通すと、面取りの面どうしと元の面が交わる点を通ってブーリアンが
/// 失敗しやすいので、頂点の側へ `PATCH_INSET` だけずらす。
pub(super) fn patch_corner(
    kernel: &dyn k::Kernel,
    body: k::ShapeHandle,
    v: &Vertex,
    actual: &[f64],
    tag: &str,
) -> Result<k::ShapeHandle, k::KernelError> {
    let fail = |m: &str| k::KernelError::Failed(m.into());
    let mut q = Vec::new();
    for (j, (n, [a, b])) in v.faces.iter().enumerate() {
        // 面の上で辺から離れる向き: 辺の向こうの面の法線を、この面と辺へ直交にした逆向き
        let away = |e: usize| {
            let other = v
                .faces
                .iter()
                .enumerate()
                .find(|(i, (_, on))| *i != j && on.contains(&e))
                .map(|(_, (m, _))| *m)
                .unwrap_or(*n);
            let d = v.dirs[e];
            let t = k::sub(other, k::scale(*n, k::dot(other, *n)));
            k::scale(k::normalize(k::sub(t, k::scale(d, k::dot(t, d)))), -1.0)
        };
        let rel = solve3([*n, away(*a), away(*b)], [0.0, actual[*a], actual[*b]])
            .ok_or_else(|| fail("面取りの境の線が交わりません"))?;
        q.push(k::add(v.at, rel));
    }
    let centroid = k::scale(k::add(q[0], k::add(q[1], q[2])), 1.0 / 3.0);
    let mut m = k::normalize(k::cross(k::sub(q[1], q[0]), k::sub(q[2], q[0])));
    if k::dot(m, k::sub(v.at, centroid)) < 0.0 {
        m = k::scale(m, -1.0);
    }
    let depth = k::dot(m, k::sub(v.at, centroid));
    if depth <= PATCH_INSET {
        return Err(fail("面取りが小さすぎて三角の面を置けません"));
    }
    let (x, y) = axes(m);
    let origin = k::add(centroid, k::scale(m, PATCH_INSET));
    // 三角を重心から 5 倍に広げる(角の材料を覆う大きさ)
    let pts: Vec<[f64; 2]> = q
        .iter()
        .map(|p| {
            let d = k::scale(k::sub(*p, centroid), 5.0);
            [k::dot(d, x), k::dot(d, y)]
        })
        .collect();
    let mut lp = ProfileLoop {
        edges: vec![
            line("t1", pts[0], pts[1]),
            line("t2", pts[1], pts[2]),
            line("t3", pts[2], pts[0]),
        ],
    };
    if lp.signed_area() < 0.0 {
        lp.reverse();
    }
    let profile = Profile {
        plane: Plane {
            origin,
            x_axis: x,
            y_axis: y,
        },
        regions: vec![Region {
            outer: lp,
            holes: vec![],
        }],
    };
    let tool = kernel.extrude(&profile, 0.0, depth + EXTEND, tag)?;
    kernel.boolean(BooleanOp::Subtract, body, tool)
}
