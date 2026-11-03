//! 受け取り方向の格子。
//!
//! 角度の決まり: 方位は +x を 0° とし +y(左)へ向かって 0〜360°、仰角は水平を 0° とし +z(上)を +90°。
//! 方向のベクトルは (cos 仰角 cos 方位, cos 仰角 sin 方位, sin 仰角)。

use serde::{Deserialize, Serialize};

/// 全球の格子。仰角はガウス・ルジャンドルの点(sin 仰角が点)、方位は等しい間隔。
/// 重みの和は 4π で、格子の上の和で球面の積分ができる。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SphereGrid {
    /// 仰角(°、小さい順)
    pub elevations: Vec<f64>,
    /// 仰角ごとの重み(立体角。方位の刻みを含まない ∫d(sin 仰角) の分)
    pub elevation_weights: Vec<f64>,
    /// 方位(°、0 から等しい間隔)
    pub azimuths: Vec<f64>,
}

impl SphereGrid {
    /// 仰角 `n_elevation` 段 × 方位 `n_azimuth` の格子。
    pub fn gauss_legendre(n_elevation: usize, n_azimuth: usize) -> Self {
        let (x, w) = gauss_legendre(n_elevation);
        Self {
            elevations: x.iter().map(|s| s.asin().to_degrees()).collect(),
            elevation_weights: w,
            azimuths: (0..n_azimuth)
                .map(|i| 360.0 * i as f64 / n_azimuth as f64)
                .collect(),
        }
    }

    /// 方向の数。
    pub fn len(&self) -> usize {
        self.elevations.len() * self.azimuths.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 方向の単位ベクトル(仰角ごとに方位を回す並び)。
    pub fn directions(&self) -> Vec<[f64; 3]> {
        let mut out = Vec::with_capacity(self.len());
        for e in &self.elevations {
            for a in &self.azimuths {
                out.push(direction(*e, *a));
            }
        }
        out
    }

    /// 方向ごとの立体角の重み([`Self::directions`] と同じ並び。和は 4π)。
    pub fn weights(&self) -> Vec<f64> {
        let da = 2.0 * std::f64::consts::PI / self.azimuths.len() as f64;
        let mut out = Vec::with_capacity(self.len());
        for w in &self.elevation_weights {
            out.extend(std::iter::repeat_n(w * da, self.azimuths.len()));
        }
        out
    }
}

/// 仰角と方位(°)から方向の単位ベクトル。
pub fn direction(elevation_deg: f64, azimuth_deg: f64) -> [f64; 3] {
    let (e, a) = (elevation_deg.to_radians(), azimuth_deg.to_radians());
    [e.cos() * a.cos(), e.cos() * a.sin(), e.sin()]
}

/// [−1, 1] の n 点のガウス・ルジャンドル求積(点は小さい順)。ニュートン法で根を求める。
pub fn gauss_legendre(n: usize) -> (Vec<f64>, Vec<f64>) {
    let mut x = vec![0.0; n];
    let mut w = vec![0.0; n];
    for i in 0..n.div_ceil(2) {
        let mut z = (std::f64::consts::PI * (i as f64 + 0.75) / (n as f64 + 0.5)).cos();
        let mut dp = 0.0;
        for _ in 0..100 {
            let (p, d) = legendre_and_derivative(n, z);
            dp = d;
            let dz = p / d;
            z -= dz;
            if dz.abs() < 1e-15 {
                break;
            }
        }
        let (_, d) = legendre_and_derivative(n, z);
        dp = if d != 0.0 { d } else { dp };
        let wi = 2.0 / ((1.0 - z * z) * dp * dp);
        x[i] = -z;
        x[n - 1 - i] = z;
        w[i] = wi;
        w[n - 1 - i] = wi;
    }
    (x, w)
}

fn legendre_and_derivative(n: usize, z: f64) -> (f64, f64) {
    let (mut p0, mut p1) = (1.0, z);
    if n == 0 {
        return (1.0, 0.0);
    }
    for k in 2..=n {
        let p2 = ((2 * k - 1) as f64 * z * p1 - (k - 1) as f64 * p0) / k as f64;
        p0 = p1;
        p1 = p2;
    }
    (p1, n as f64 * (z * p1 - p0) / (z * z - 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 確かめること: 16020 方向の格子(仰角 89 段 × 方位 2°)の重みの和が 4π で、
    /// 球面調和の低い次数(z²)の積分が 4π/3 になる。仰角は上下で対称。
    #[test]
    fn full_sphere_grid_integrates_exactly() {
        let g = SphereGrid::gauss_legendre(89, 180);
        assert_eq!(g.len(), 16020);
        let w = g.weights();
        let d = g.directions();
        let four_pi = 4.0 * std::f64::consts::PI;
        assert!((w.iter().sum::<f64>() - four_pi).abs() < 1e-10);
        let z2: f64 = d.iter().zip(&w).map(|(d, w)| d[2] * d[2] * w).sum();
        assert!((z2 - four_pi / 3.0).abs() < 1e-10);
        assert!((g.elevations[0] + g.elevations[88]).abs() < 1e-10);
        assert!(g.elevations[44].abs() < 1e-10);
        let left = direction(0.0, 90.0);
        assert!(left[0].abs() < 1e-15 && (left[1] - 1.0).abs() < 1e-15);
    }
}
