//! 球と円環を頂点の最小二乗で当てはめ、全頂点の誤差で確かめる。

use super::*;

fn solve<const N: usize>(mut a: [[f64; N]; N], mut b: [f64; N]) -> Option<[f64; N]> {
    let scale = a.iter().flatten().map(|v| v.abs()).fold(0.0, f64::max);
    if !scale.is_finite() || scale == 0.0 {
        return None;
    }
    for k in 0..N {
        let pivot = (k..N).max_by(|&i, &j| a[i][k].abs().total_cmp(&a[j][k].abs()))?;
        if a[pivot][k].abs() < 1e-12 * scale {
            return None;
        }
        a.swap(k, pivot);
        b.swap(k, pivot);
        let d = a[k][k];
        for value in &mut a[k][k..] {
            *value /= d;
        }
        b[k] /= d;
        let pivot_row = a[k];
        for i in 0..N {
            if i == k {
                continue;
            }
            let d = a[i][k];
            for (value, pivot) in a[i][k..].iter_mut().zip(&pivot_row[k..]) {
                *value -= d * pivot;
            }
            b[i] -= d * b[k];
        }
    }
    b.iter().all(|v| v.is_finite()).then_some(b)
}

struct Points {
    all: Vec<P>,
    mean: P,
    size: f64,
}
impl Points {
    fn new(tris: &[[P; 3]]) -> Option<Self> {
        let mut all: Vec<P> = tris.iter().flatten().copied().collect();
        if all.iter().flatten().any(|v| !v.is_finite()) {
            return None;
        }
        all.sort_by(|a, b| {
            a[0].total_cmp(&b[0])
                .then(a[1].total_cmp(&b[1]))
                .then(a[2].total_cmp(&b[2]))
        });
        all.dedup();
        if all.len() < 12 {
            return None;
        }
        let mean = scale(
            all.iter().fold([0.0; 3], |s, p| add(s, *p)),
            1.0 / all.len() as f64,
        );
        let size = all.iter().map(|p| dist(*p, mean)).fold(0.0, f64::max);
        if size < 1e-8 || !size.is_finite() {
            return None;
        }
        for p in &mut all {
            *p = scale(sub(*p, mean), 1.0 / size);
        }
        Some(Self { all, mean, size })
    }
    fn world(&self, p: P) -> P {
        add(self.mean, scale(p, self.size))
    }
    fn sample(&self) -> Vec<P> {
        let n = self.all.len().min(256);
        (0..n)
            .map(|i| self.all[i * (self.all.len() - 1) / (n - 1)])
            .collect()
    }
}

pub(crate) fn triangle_area(tris: &[[P; 3]]) -> f64 {
    tris.iter()
        .map(|t| 0.5 * len(cross(sub(t[1], t[0]), sub(t[2], t[0]))))
        .sum()
}
fn tolerance(radius: f64, center: P) -> f64 {
    1e-4 * (1.0 + radius) + 2e-6 * len(center)
}
fn radius_ok(radius: f64) -> bool {
    (1e-6..=1e6).contains(&radius)
}

/// 球の式を平均と縮尺で整えて解き、中心からの距離で全頂点を確かめる。
pub fn fit_sphere(tris: &[[P; 3]]) -> Option<Sphere> {
    let pts = Points::new(tris)?;
    let mut a = [[0.0; 4]; 4];
    let mut b = [0.0; 4];
    for p in &pts.all {
        let row = [2.0 * p[0], 2.0 * p[1], 2.0 * p[2], 1.0];
        let rhs = dot(*p, *p);
        for i in 0..4 {
            for j in 0..4 {
                a[i][j] += row[i] * row[j];
            }
            b[i] += row[i] * rhs;
        }
    }
    let v = solve(a, b)?;
    let local = [v[0], v[1], v[2]];
    let r2 = dot(local, local) + v[3];
    if r2 <= 0.0 {
        return None;
    }
    let radius = r2.sqrt() * pts.size;
    let center = pts.world(local);
    if !radius_ok(radius) {
        return None;
    }
    let tol = tolerance(radius, center) / pts.size;
    if pts
        .all
        .iter()
        .any(|p| (dist(*p, local) - r2.sqrt()).abs() > tol)
    {
        return None;
    }
    Some(Sphere {
        center,
        radius,
        area: triangle_area(tris),
    })
}

fn axes(m: [[f64; 3]; 3]) -> [P; 3] {
    let a = smallest_eigenvector(m);
    let b = smallest_eigenvector(m.map(|r| r.map(|v| -v)));
    let b = if dot(a, b).abs() > 0.99 {
        basis_of(a).0
    } else {
        normalize(sub(b, scale(a, dot(a, b))))
    };
    [a, b, normalize(cross(a, b))]
}

fn initial(pts: &[P], axis: P) -> Option<[f64; 8]> {
    let mut a = [[0.0; 3]; 3];
    let mut b = [0.0; 3];
    for p in pts {
        let z = dot(*p, axis);
        let rho = len(cross(*p, axis));
        let row = [2.0 * rho, 2.0 * z, 1.0];
        let rhs = rho * rho + z * z;
        for i in 0..3 {
            for j in 0..3 {
                a[i][j] += row[i] * row[j];
            }
            b[i] += row[i] * rhs;
        }
    }
    let v = solve(a, b)?;
    let r2 = v[0] * v[0] + v[1] * v[1] + v[2];
    if r2 <= 0.0 || v[0] <= r2.sqrt() {
        return None;
    }
    let c = scale(axis, v[1]);
    Some([c[0], c[1], c[2], axis[0], axis[1], axis[2], v[0], r2.sqrt()])
}

fn residual(p: P, v: &[f64; 8]) -> f64 {
    let d = sub(p, [v[0], v[1], v[2]]);
    let axis = normalize([v[3], v[4], v[5]]);
    let z = dot(d, axis);
    let rho = len(cross(d, axis));
    ((rho - v[6]).powi(2) + z * z).sqrt() - v[7]
}
fn cost(pts: &[P], v: &[f64; 8]) -> f64 {
    pts.iter().map(|p| residual(*p, v).powi(2)).sum::<f64>() / pts.len() as f64
}
fn valid(v: &[f64; 8]) -> bool {
    v.iter().all(|x| x.is_finite()) && len([v[3], v[4], v[5]]) > 1e-8 && v[7] > 1e-8 && v[6] > v[7]
}

/// 円環の中心・向き・2つの半径を、管の円までの距離で補正する。
fn refine(pts: &[P], mut v: [f64; 8]) -> [f64; 8] {
    let mut lambda = 1e-4;
    let mut now = cost(pts, &v);
    for _ in 0..40 {
        let rs: Vec<_> = pts.iter().map(|p| residual(*p, &v)).collect();
        let deriv: Vec<Vec<f64>> = (0..8)
            .map(|i| {
                let step = 1e-6 * (1.0 + v[i].abs());
                let mut next = v;
                next[i] += step;
                pts.iter()
                    .zip(&rs)
                    .map(|(p, r)| (residual(*p, &next) - r) / step)
                    .collect()
            })
            .collect();
        let mut a = [[0.0; 8]; 8];
        let mut b = [0.0; 8];
        for i in 0..8 {
            for j in 0..8 {
                a[i][j] = deriv[i].iter().zip(&deriv[j]).map(|(x, y)| x * y).sum();
            }
            b[i] = -deriv[i].iter().zip(&rs).map(|(x, y)| x * y).sum::<f64>();
            a[i][i] += lambda * a[i][i].max(1.0);
        }
        let Some(delta) = solve(a, b) else { break };
        let trial = std::array::from_fn(|i| v[i] + delta[i]);
        let next = cost(pts, &trial);
        if valid(&trial) && next < now {
            v = trial;
            now = next;
            lambda = (lambda * 0.25).max(1e-8);
            if delta.iter().map(|x| x.abs()).fold(0.0, f64::max) < 1e-10 || now < 1e-20 {
                break;
            }
        } else {
            lambda *= 10.0;
            if lambda > 1e10 {
                break;
            }
        }
    }
    v
}

fn checked(pts: &Points, tris: &[[P; 3]], v: &[f64; 8]) -> Option<Torus> {
    if !valid(v) {
        return None;
    }
    let center = pts.world([v[0], v[1], v[2]]);
    let major_radius = v[6] * pts.size;
    let minor_radius = v[7] * pts.size;
    if !radius_ok(major_radius) || !radius_ok(minor_radius) {
        return None;
    }
    let tol = tolerance(minor_radius, center) / pts.size;
    if pts.all.iter().any(|p| residual(*p, v).abs() > tol) {
        return None;
    }
    let mut axis = normalize([v[3], v[4], v[5]]);
    let k = (0..3).max_by(|&i, &j| axis[i].abs().total_cmp(&axis[j].abs()))?;
    if axis[k] < 0.0 {
        axis = scale(axis, -1.0)
    }
    Some(Torus {
        center,
        axis,
        major_radius,
        minor_radius,
        area: triangle_area(tris),
    })
}

/// 複数の軸から最小二乗で円環を求める。反復用の点を絞っても、判定には全頂点を使う。
pub fn fit_torus(tris: &[[P; 3]]) -> Option<Torus> {
    let pts = Points::new(tris)?;
    let sample = pts.sample();
    let mut covariance = [[0.0; 3]; 3];
    let mut normals = [[0.0; 3]; 3];
    for p in &pts.all {
        for i in 0..3 {
            for j in 0..3 {
                covariance[i][j] += p[i] * p[j];
            }
        }
    }
    for t in tris {
        let n = cross(sub(t[1], t[0]), sub(t[2], t[0]));
        let size = len(n);
        if size < 1e-300 {
            continue;
        }
        for i in 0..3 {
            for j in 0..3 {
                normals[i][j] += n[i] * n[j] / size;
            }
        }
    }
    let mut candidates = Vec::new();
    let mut seen: Vec<P> = Vec::new();
    for axis in axes(covariance).into_iter().chain(axes(normals)) {
        if seen
            .iter()
            .any(|other| dot(*other, axis).abs() > 1.0 - 1e-6)
        {
            continue;
        }
        seen.push(axis);
        if let Some(v) = initial(&sample, axis) {
            if let Some(torus) = checked(&pts, tris, &v) {
                return Some(torus);
            }
            let error = cost(&sample, &v);
            if error.is_finite() && error < 0.15_f64.powi(2) {
                candidates.push((error, v));
            }
        }
    }
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, v) in candidates.into_iter().take(3) {
        let v = refine(&sample, v);
        if let Some(torus) = checked(&pts, tris, &v) {
            return Some(torus);
        }
    }
    None
}
