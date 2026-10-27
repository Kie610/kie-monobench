//! 断面の半空間切断と、詳細ビューの円による二次元切断。
use kmb_core::kernel::{Mesh, P3, dot, sub};
use kmb_interchange::drawing::Path;

pub fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}
pub fn lerp(a: P3, b: P3, t: f64) -> P3 {
    [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * t)
}
#[derive(Clone, Copy)]
pub struct Cut {
    pub origin: P3,
    pub normal: P3,
}
impl Cut {
    pub fn depth(self, p: P3) -> f64 {
        dot(sub(p, self.origin), self.normal)
    }
    pub fn segment(self, a: P3, b: P3) -> Option<[P3; 2]> {
        let (da, db) = (self.depth(a), self.depth(b));
        if da > 1e-8 && db > 1e-8 {
            return None;
        }
        if da <= 1e-8 && db <= 1e-8 {
            return Some([a, b]);
        }
        let q = lerp(a, b, da / (da - db));
        Some(if da > 0. { [q, b] } else { [a, q] })
    }
    /// 切った網と、切り口の閉じた輪。開いた切り口は誤りにする。
    pub fn mesh(self, m: &Mesh) -> Result<(Mesh, Vec<Vec<P3>>), String> {
        let mut result = Mesh {
            face_names: m.face_names.clone(),
            ..Default::default()
        };
        let mut cuts = Vec::new();
        for (i, t) in m.triangles.iter().enumerate() {
            let pts = t.map(|j| m.positions[j as usize].map(f64::from));
            let mut poly = Vec::new();
            let mut crossings = Vec::new();
            for k in 0..3 {
                let (a, b) = (pts[k], pts[(k + 1) % 3]);
                let (da, db) = (self.depth(a), self.depth(b));
                if da <= 1e-8 {
                    poly.push(a);
                }
                if (da > 1e-8 && db < -1e-8) || (da < -1e-8 && db > 1e-8) {
                    let q = lerp(a, b, da / (da - db));
                    poly.push(q);
                    crossings.push(q);
                } else if da.abs() <= 1e-8 {
                    crossings.push(a);
                }
            }
            crossings.dedup_by(|a, b| length(sub(*a, *b)) < 1e-8);
            if crossings.len() == 2 && pts.iter().any(|p| self.depth(*p).abs() > 1e-8) {
                cuts.push([crossings[0], crossings[1]]);
            }
            if poly.len() >= 3 {
                let base = result.positions.len() as u32;
                result
                    .positions
                    .extend(poly.iter().map(|p| p.map(|n| n as f32)));
                result.normals.extend(vec![[0.; 3]; poly.len()]);
                for j in 1..poly.len() - 1 {
                    result
                        .triangles
                        .push([base, base + j as u32, base + j as u32 + 1]);
                    result
                        .triangle_faces
                        .push(m.triangle_faces.get(i).copied().unwrap_or(0));
                }
            }
        }
        for e in &m.edges {
            for w in e.windows(2) {
                if let Some([a, b]) = self.segment(w[0].map(f64::from), w[1].map(f64::from)) {
                    result
                        .edges
                        .push(vec![a.map(|n| n as f32), b.map(|n| n as f32)]);
                }
            }
        }
        let loops = closed_loops(cuts)?;
        for l in &loops {
            result
                .edges
                .push(l.iter().map(|p| p.map(|n| n as f32)).collect());
        }
        Ok((result, loops))
    }
}
fn length(p: P3) -> f64 {
    dot(p, p).sqrt()
}
pub fn closed_loops(segments: Vec<[P3; 2]>) -> Result<Vec<Vec<P3>>, String> {
    use std::collections::BTreeMap;
    let key = |p: P3| p.map(|n| (n * 1e5).round() as i64);
    let mut edges = BTreeMap::new();
    for [a, b] in segments {
        let (ka, kb) = (key(a), key(b));
        if ka != kb {
            edges
                .entry(if ka < kb { (ka, kb) } else { (kb, ka) })
                .or_insert([a, b]);
        }
    }
    let mut adjacency: BTreeMap<[i64; 3], usize> = BTreeMap::new();
    for (a, b) in edges.keys() {
        *adjacency.entry(*a).or_default() += 1;
        *adjacency.entry(*b).or_default() += 1;
    }
    if adjacency.values().any(|n| *n != 2) {
        return Err("切り口の輪が閉じていません".into());
    }
    let mut loops = Vec::new();
    while let Some((_, [a, b])) = edges.pop_first() {
        let mut l = vec![a, b];
        while key(*l.last().unwrap()) != key(a) {
            let tip = key(*l.last().unwrap());
            let Some(k) = edges.keys().find(|(a, b)| *a == tip || *b == tip).copied() else {
                return Err("切り口の輪が閉じていません".into());
            };
            let [u, v] = edges.remove(&k).unwrap();
            l.push(if key(u) == tip { v } else { u });
        }
        loops.push(l);
    }
    Ok(loops)
}

pub fn path_points(p: &Path, tol: f64) -> Vec<[f64; 2]> {
    match p {
        Path::Line { a, b } => vec![*a, *b],
        Path::Poly { points } => points.clone(),
        Path::Arc {
            c,
            r,
            start_deg,
            sweep_deg,
        } => {
            let step = if *r > tol {
                2. * (1. - tol / r).clamp(-1., 1.).acos()
            } else {
                std::f64::consts::FRAC_PI_2
            };
            let n = (sweep_deg.to_radians().abs() / step.max(1e-6))
                .ceil()
                .clamp(1., 65536.) as usize;
            (0..=n)
                .map(|i| {
                    let a = (start_deg + sweep_deg * i as f64 / n as f64).to_radians();
                    [c[0] + r * a.cos(), c[1] + r * a.sin()]
                })
                .collect()
        }
    }
}
fn circle_segment(a: [f64; 2], b: [f64; 2], c: [f64; 2], r: f64) -> Option<[[f64; 2]; 2]> {
    let d = [b[0] - a[0], b[1] - a[1]];
    let q = [a[0] - c[0], a[1] - c[1]];
    let aa = d[0] * d[0] + d[1] * d[1];
    if aa < 1e-20 {
        return None;
    }
    let bb = 2. * (q[0] * d[0] + q[1] * d[1]);
    let cc = q[0] * q[0] + q[1] * q[1] - r * r;
    let det = bb * bb - 4. * aa * cc;
    if det <= 0. {
        return None;
    }
    let lo = ((-bb - det.sqrt()) / (2. * aa)).max(0.);
    let hi = ((-bb + det.sqrt()) / (2. * aa)).min(1.);
    (hi > lo + 1e-12).then(|| [lo, hi].map(|t| [a[0] + d[0] * t, a[1] + d[1] * t]))
}
/// 円弧は円との交点の角で切り、円弧のまま返す。
pub fn clip_circle(p: &Path, c: [f64; 2], r: f64) -> Vec<Path> {
    if let Path::Arc {
        c: ac,
        r: ar,
        start_deg,
        sweep_deg,
    } = p
    {
        let d = dist(*ac, c);
        let mut ts = vec![0., 1.];
        if d > 1e-12 && d <= r + ar && d >= (r - ar).abs() {
            let a = (ar * ar + d * d - r * r) / (2. * ar * d);
            let base = (c[1] - ac[1]).atan2(c[0] - ac[0]).to_degrees();
            let off = a.clamp(-1., 1.).acos().to_degrees();
            for angle in [base - off, base + off] {
                for k in -3..=3 {
                    let t = (angle + 360. * k as f64 - start_deg) / sweep_deg;
                    if t > 0. && t < 1. {
                        ts.push(t);
                    }
                }
            }
        }
        ts.sort_by(f64::total_cmp);
        ts.dedup_by(|a, b| (*a - *b).abs() < 1e-10);
        return ts
            .windows(2)
            .filter_map(|w| {
                let a = (start_deg + sweep_deg * (w[0] + w[1]) / 2.).to_radians();
                let q = [ac[0] + ar * a.cos(), ac[1] + ar * a.sin()];
                (dist(q, c) <= r + 1e-8).then(|| Path::Arc {
                    c: *ac,
                    r: *ar,
                    start_deg: start_deg + sweep_deg * w[0],
                    sweep_deg: sweep_deg * (w[1] - w[0]),
                })
            })
            .collect();
    }
    let pts = path_points(p, 0.001);
    pts.windows(2)
        .filter_map(|w| circle_segment(w[0], w[1], c, r))
        .map(|[a, b]| Path::Line { a, b })
        .collect()
}
pub fn transform(p: &Path, f: impl Fn([f64; 2]) -> [f64; 2], scale: f64) -> Path {
    match p {
        Path::Line { a, b } => Path::Line { a: f(*a), b: f(*b) },
        Path::Poly { points } => Path::Poly {
            points: points.iter().map(|p| f(*p)).collect(),
        },
        Path::Arc {
            c,
            r,
            start_deg,
            sweep_deg,
        } => Path::Arc {
            c: f(*c),
            r: r * scale,
            start_deg: *start_deg,
            sweep_deg: *sweep_deg,
        },
    }
}

/// 閉じた輪を円の内側に切る。円は用紙上の許容差内の凸多角形にする。
pub fn clip_loop_circle(
    points: &[[f64; 2]],
    center: [f64; 2],
    radius: f64,
    tolerance: f64,
) -> Vec<[f64; 2]> {
    let step = 2. * (1. - tolerance / radius).clamp(-1., 1.).acos();
    let n = (std::f64::consts::TAU / step.max(1e-6))
        .ceil()
        .clamp(16., 65536.) as usize;
    let mut poly = points.to_vec();
    if poly.first() == poly.last() {
        poly.pop();
    }
    for i in 0..n {
        if poly.is_empty() {
            break;
        }
        let point = |k: usize| {
            let a = std::f64::consts::TAU * k as f64 / n as f64;
            [center[0] + radius * a.cos(), center[1] + radius * a.sin()]
        };
        let (a, b) = (point(i), point(i + 1));
        let depth = |p: [f64; 2]| (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
        let mut out = vec![];
        for j in 0..poly.len() {
            let (p, q) = (poly[j], poly[(j + 1) % poly.len()]);
            let (dp, dq) = (depth(p), depth(q));
            if dp >= -1e-10 {
                out.push(p);
            }
            if (dp > 0. && dq < 0.) || (dp < 0. && dq > 0.) {
                let t = dp / (dp - dq);
                out.push([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]);
            }
        }
        poly = out;
    }
    if poly.len() < 3 {
        vec![]
    } else {
        poly.push(poly[0]);
        poly
    }
}

fn rounded(x: f64) -> f64 {
    let q = (x * 1e8).round() / 1e8;
    if q == 0. { 0. } else { q }
}
fn point_order(a: &[f64; 2], b: &[f64; 2]) -> std::cmp::Ordering {
    a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1]))
}
/// 同じ幾何の端の向きと、閉じた輪の書き始めをそろえる。
pub fn canonical_path(path: Path) -> Path {
    match path {
        Path::Line { a, b } => {
            let (mut a, mut b) = (a.map(rounded), b.map(rounded));
            if point_order(&a, &b).is_gt() {
                std::mem::swap(&mut a, &mut b);
            }
            Path::Line { a, b }
        }
        Path::Poly { points } => {
            if points.len() > 2 && dist(points[0], *points.last().unwrap()) < 1e-7 {
                Path::Poly {
                    points: canonical_loop(points),
                }
            } else {
                let mut points = points
                    .into_iter()
                    .map(|p| p.map(rounded))
                    .collect::<Vec<_>>();
                if points
                    .first()
                    .zip(points.last())
                    .is_some_and(|(a, b)| point_order(a, b).is_gt())
                {
                    points.reverse();
                }
                Path::Poly { points }
            }
        }
        Path::Arc {
            c,
            r,
            mut start_deg,
            mut sweep_deg,
        } => {
            if sweep_deg < 0. {
                start_deg += sweep_deg;
                sweep_deg = -sweep_deg;
            }
            if (sweep_deg - 360.).abs() < 1e-7 {
                start_deg = 0.;
                sweep_deg = 360.;
            }
            Path::Arc {
                c: c.map(rounded),
                r: rounded(r),
                start_deg: rounded(start_deg.rem_euclid(360.)).rem_euclid(360.),
                sweep_deg: rounded(sweep_deg),
            }
        }
    }
}
pub fn canonical_loop(points: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    let mut p = points
        .into_iter()
        .map(|p| p.map(rounded))
        .collect::<Vec<_>>();
    if p.first() == p.last() {
        p.pop();
    }
    // 面の三角形の分け方でできる、直線の途中の点を除く。
    loop {
        if p.len() < 4 {
            break;
        }
        let Some(i) = (0..p.len()).find(|&i| {
            let (a, b, c) = (p[(i + p.len() - 1) % p.len()], p[i], p[(i + 1) % p.len()]);
            let u = [b[0] - a[0], b[1] - a[1]];
            let v = [c[0] - b[0], c[1] - b[1]];
            (u[0] * v[1] - u[1] * v[0]).abs() < 1e-8 && u[0] * v[0] + u[1] * v[1] >= 0.
        }) else {
            break;
        };
        p.remove(i);
    }
    if p.is_empty() {
        return p;
    }
    let rotate = |p: &mut Vec<[f64; 2]>| {
        let i = p
            .iter()
            .enumerate()
            .min_by(|a, b| point_order(a.1, b.1))
            .unwrap()
            .0;
        p.rotate_left(i);
    };
    rotate(&mut p);
    let mut rev = p.iter().rev().copied().collect::<Vec<_>>();
    rotate(&mut rev);
    if rev
        .iter()
        .zip(&p)
        .map(|(a, b)| point_order(a, b))
        .find(|o| !o.is_eq())
        .is_some_and(|o| o.is_lt())
    {
        p = rev;
    }
    p.push(p[0]);
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detail_clips_lines_and_arcs_without_reprojection() {
        let parts = clip_circle(
            &Path::Line {
                a: [-10., 0.],
                b: [10., 0.],
            },
            [0., 0.],
            2.,
        );
        assert_eq!(
            parts,
            vec![Path::Line {
                a: [-2., 0.],
                b: [2., 0.]
            }]
        );
        let arc = Path::Arc {
            c: [0., 0.],
            r: 5.,
            start_deg: 0.,
            sweep_deg: 360.,
        };
        let parts = clip_circle(&arc, [5., 0.], 5.);
        assert_eq!(parts.len(), 2);
        let total = parts
            .iter()
            .map(|p| match p {
                Path::Arc { sweep_deg, .. } => *sweep_deg,
                _ => panic!("円弧を保つ"),
            })
            .sum::<f64>();
        assert!((total - 120.).abs() < 1e-8);
        assert!(clip_circle(&arc, [0., 0.], 2.).is_empty());
        assert_eq!(clip_circle(&arc, [0., 0.], 8.), vec![arc]);
    }
    #[test]
    fn open_and_branched_section_boundaries_are_rejected() {
        assert!(closed_loops(vec![[[0., 0., 0.], [1., 0., 0.]]]).is_err());
        let l = closed_loops(vec![
            [[1., 0., 0.], [0., 0., 0.]],
            [[0., 1., 0.], [1., 0., 0.]],
            [[0., 0., 0.], [0., 1., 0.]],
        ])
        .unwrap();
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].first(), l[0].last());
    }
}
