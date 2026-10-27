//! ねじの陰影: ねじを付けた円筒の面の三角形を、軸に直角な面で半ピッチごとの帯に切り、帯ごとに法線を
//! 軸の向きへ交互に傾ける。形(位置)は変えず、陰影だけがねじ山の斜面のように明暗の縞になる。
//! 帯は軸を回る輪で、らせんの線は作らない(避ける形: docs/避ける形.md)。

use kmb_core::Mesh;
use kmb_interchange::ThreadMark;

/// 帯の法線を半径の向きから軸の向きへ傾ける角(rad)。
pub const THREAD_TILT: f64 = 0.55;

type V = [f64; 3];

fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(a: V) -> V {
    let l = dot(a, a).sqrt();
    if l < 1e-12 { a } else { a.map(|x| x / l) }
}
fn lerp(a: V, b: V, s: f64) -> V {
    [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * s)
}

/// 印 `m` の円筒に乗る三角形か(3 つの頂点が円筒の上にあり、面が軸に沿い、ねじの範囲に掛かる)。
/// 乗れば頂点の軸の向きの位置(始まりから)を返す。
fn on_cylinder(m: &ThreadMark, axis: V, p: [V; 3]) -> Option<[f64; 3]> {
    let tol = 1e-3 + 1e-3 * m.radius;
    let mut t = [0.0; 3];
    for (k, v) in p.iter().enumerate() {
        let d = sub(*v, m.start);
        t[k] = dot(d, axis);
        let radial = sub(d, axis.map(|a| a * t[k]));
        if (dot(radial, radial).sqrt() - m.radius).abs() > tol {
            return None;
        }
    }
    // 円の端の平らな面(頂点がみな縁の上の扇)を除く
    let n = norm(cross(sub(p[1], p[0]), sub(p[2], p[0])));
    if dot(n, axis).abs() > 0.5 {
        return None;
    }
    let (lo, hi) = (t[0].min(t[1]).min(t[2]), t[0].max(t[1]).max(t[2]));
    (hi > tol && lo < m.length - tol).then_some(t)
}

/// 凸の多角形を、軸の向きの位置が `lo`〜`hi` の所だけに切る(Sutherland–Hodgman を 2 回)。
fn clip(poly: &[(V, f64)], lo: f64, hi: f64) -> Vec<(V, f64)> {
    let cut = |poly: &[(V, f64)], keep: &dyn Fn(f64) -> bool, at: f64| {
        let mut out = Vec::new();
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            if keep(a.1) {
                out.push(a);
            }
            if keep(a.1) != keep(b.1) {
                let s = (at - a.1) / (b.1 - a.1);
                out.push((lerp(a.0, b.0, s), at));
            }
        }
        out
    };
    let p = cut(poly, &|t| t >= lo, lo);
    cut(&p, &|t| t <= hi, hi)
}

/// 網 `mesh` の、印 `marks` の円筒に乗る三角形を帯に切った網。乗る三角形が無ければ `None`。
pub fn thread_bands(mesh: &Mesh, marks: &[ThreadMark]) -> Option<Mesh> {
    let mut out = Mesh {
        positions: mesh.positions.clone(),
        normals: mesh.normals.clone(),
        triangles: Vec::with_capacity(mesh.triangles.len()),
        triangle_faces: Vec::with_capacity(mesh.triangle_faces.len()),
        face_names: mesh.face_names.clone(),
        edges: mesh.edges.clone(),
    };
    let at = |i: u32| mesh.positions[i as usize].map(f64::from);
    let mut changed = false;
    for (ti, tri) in mesh.triangles.iter().enumerate() {
        let face = mesh.triangle_faces.get(ti).copied().unwrap_or(0);
        let p = tri.map(at);
        let hit = marks.iter().filter(|m| m.pitch > 0.0).find_map(|m| {
            let axis = norm(m.axis);
            on_cylinder(m, axis, p).map(|t| (m, axis, t))
        });
        let Some((m, axis, t)) = hit else {
            out.triangles.push(*tri);
            out.triangle_faces.push(face);
            continue;
        };
        changed = true;
        // 法線の外向き(おねじは軸から離れる向き、めねじは軸へ向かう向き)は元の網の法線に合わせる
        let n0 = mesh.normals.get(tri[0] as usize).map(|n| n.map(f64::from));
        let radial = |v: V| {
            let d = sub(v, m.start);
            norm(sub(d, axis.map(|a| a * dot(d, axis))))
        };
        let outward = match n0 {
            Some(n) => dot(n, radial(p[0])) >= 0.0,
            None => !m.internal,
        };
        let half = m.pitch / 2.0;
        let lo = t[0].min(t[1]).min(t[2]);
        let hi = t[0].max(t[1]).max(t[2]);
        let poly: Vec<(V, f64)> = (0..3).map(|k| (p[k], t[k])).collect();
        // 帯の境: ねじの範囲の外は 1 つの帯、範囲の中は半ピッチごと
        let mut cuts = vec![lo];
        let first = (lo.max(0.0) / half).floor() as i64;
        let last = (hi.min(m.length) / half).ceil() as i64;
        for k in first..=last {
            let c = (k as f64 * half).min(m.length);
            if c > lo && c < hi {
                cuts.push(c);
            }
        }
        if m.length > lo && m.length < hi {
            cuts.push(m.length);
        }
        cuts.push(hi);
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
        for w in cuts.windows(2) {
            let piece = clip(&poly, w[0], w[1]);
            if piece.len() < 3 {
                continue;
            }
            let mid = (w[0] + w[1]) / 2.0;
            // 範囲の中の帯は、偶数番を始まりの側へ、奇数番を先の側へ傾ける
            let tilt = if mid >= 0.0 && mid <= m.length {
                if ((mid / half).floor() as i64) % 2 == 0 {
                    THREAD_TILT
                } else {
                    -THREAD_TILT
                }
            } else {
                0.0
            };
            let base = out.positions.len() as u32;
            for (v, _) in &piece {
                let r = radial(*v);
                let r = if outward { r } else { r.map(|x| -x) };
                let n = norm([0, 1, 2].map(|k| r[k] * tilt.cos() + axis[k] * tilt.sin()));
                out.positions.push(v.map(|x| x as f32));
                out.normals.push(n.map(|x| x as f32));
            }
            for k in 1..piece.len() as u32 - 1 {
                out.triangles.push([base, base + k, base + k + 1]);
                out.triangle_faces.push(face);
            }
        }
    }
    changed.then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 半径 `r`・高さ `h` の円筒の横の面(`n` 角)の網。
    fn cylinder(r: f64, h: f64, n: usize) -> Mesh {
        let mut m = Mesh::default();
        for i in 0..n {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            let (x, y) = (r * a.cos(), r * a.sin());
            for z in [0.0, h] {
                m.positions.push([x as f32, y as f32, z as f32]);
                m.normals.push([a.cos() as f32, a.sin() as f32, 0.0]);
            }
        }
        for i in 0..n as u32 {
            let j = (i + 1) % n as u32;
            m.triangles.push([2 * i, 2 * j, 2 * j + 1]);
            m.triangles.push([2 * i, 2 * j + 1, 2 * i + 1]);
            m.triangle_faces.extend([0, 0]);
        }
        m.face_names = vec!["side".into()];
        m
    }

    fn area(m: &Mesh) -> f64 {
        let at = |i: u32| m.positions[i as usize].map(f64::from);
        m.triangles
            .iter()
            .map(|t| {
                let c = cross(sub(at(t[1]), at(t[0])), sub(at(t[2]), at(t[0])));
                dot(c, c).sqrt() / 2.0
            })
            .sum()
    }

    fn mark(length: f64) -> ThreadMark {
        ThreadMark {
            start: [0.0; 3],
            axis: [0.0, 0.0, 1.0],
            length,
            radius: 3.0,
            pitch: 1.0,
            internal: false,
        }
    }

    #[test]
    fn 帯に切っても形は同じで法線が交互に傾く() {
        let m = cylinder(3.0, 10.0, 24);
        let b = thread_bands(&m, &[mark(10.0)]).expect("円筒に乗る");
        assert!((area(&m) - area(&b)).abs() < 1e-3);
        // 半ピッチ 0.5 mm ごとの帯が 20 本。三角形 1 枚は、台形 19 個(2 枚ずつ)と三角形 1 個になる
        assert_eq!(b.triangles.len(), 24 * 2 * 39);
        assert_eq!(b.triangle_faces.len(), b.triangles.len());
        let up = b.normals.iter().filter(|n| n[2] > 0.3).count();
        let down = b.normals.iter().filter(|n| n[2] < -0.3).count();
        assert!(up > 0 && up == down, "上向き {up}・下向き {down}");
    }

    #[test]
    fn ねじの長さの先は元の向きのまま() {
        let m = cylinder(3.0, 10.0, 24);
        let b = thread_bands(&m, &[mark(4.0)]).expect("円筒に乗る");
        let at = |i: usize| b.positions[i];
        // 4 mm より先の頂点の法線は半径の向き(軸の向きの成分が無い)
        let beyond = b
            .triangles
            .iter()
            .flatten()
            .map(|&i| i as usize)
            .filter(|&i| at(i)[2] > 4.01)
            .all(|i| b.normals[i][2].abs() < 1e-4);
        assert!(beyond);
        assert!(b.normals.iter().any(|n| n[2].abs() > 0.3));
    }

    #[test]
    fn 半径の違う円筒と端の面は切らない() {
        let m = cylinder(3.0, 10.0, 24);
        let other = ThreadMark {
            radius: 4.0,
            ..mark(10.0)
        };
        assert!(thread_bands(&m, &[other]).is_none());
        // 縁の頂点だけで張った端の面(扇)
        let mut cap = cylinder(3.0, 10.0, 24);
        cap.triangles = (1..23u32).map(|i| [0, 2 * i, 2 * i + 2]).collect();
        cap.triangle_faces = vec![0; cap.triangles.len()];
        assert!(thread_bands(&cap, &[mark(10.0)]).is_none());
    }
}
