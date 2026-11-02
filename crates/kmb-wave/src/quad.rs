//! 要素の上の核の積分。
//!
//! 核は自由空間のグリーン関数 G = e^{−jkr}/(4πr)(r = |y − x|)と、その法線の微分。
//! 選点 x(法線 n_x)と要素の上の点 y(法線 n_y)について 4 つを同時に出す:
//! L = ∫G、M = ∫∂G/∂n_y、Mt = ∫∂G/∂n_x、N = ∫∂²G/∂n_x∂n_y。
//!
//! - 離れた要素は 7 点の三角形の求積(5 次まで正確)。
//! - 近い要素(選点から部分三角形の重心までが辺の [`NEAR`] 倍より近い)は 4 つに分けて繰り返す。
//! - 自分の要素は選点(重心)を中心にした極座標で、半径の向きを式で積分する。平らな要素の上では
//!   (y − x)·n = 0 なので M と Mt は 0。L の 1/r は面積の ρ で消える。N の 1/r³ はハダマールの
//!   有限部分(選点のまわりの小さな円を除いて 1/ε の項を捨てる)で、法線の微分した式の跳び ½·∂p/∂n
//!   と組になる。角度の向きは辺に沿った 8 点のガウス求積。

use num_complex::Complex64 as C;

use crate::mesh::{Elem, add, cross, dot, norm, scale, sub};

const FOUR_PI: f64 = 4.0 * std::f64::consts::PI;

/// 近いと見る距離(部分三角形の一番長い辺に対する倍率)
const NEAR: f64 = 2.5;
/// 近い要素を分ける回数の上限
const MAX_DEPTH: u32 = 6;

/// 7 点の三角形の求積(重心座標と重み。重みの和は 1)
const TRI7: [([f64; 3], f64); 7] = {
    const A1: f64 = 0.059_715_871_789_770;
    const B1: f64 = 0.470_142_064_105_115;
    const W1: f64 = 0.132_394_152_788_506;
    const A2: f64 = 0.797_426_985_353_087;
    const B2: f64 = 0.101_286_507_323_456;
    const W2: f64 = 0.125_939_180_544_827;
    [
        ([1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0], 0.225),
        ([A1, B1, B1], W1),
        ([B1, A1, B1], W1),
        ([B1, B1, A1], W1),
        ([A2, B2, B2], W2),
        ([B2, A2, B2], W2),
        ([B2, B2, A2], W2),
    ]
};

/// [−1, 1] の 8 点のガウス・ルジャンドル求積(点と重み)
const GL8: [(f64, f64); 8] = [
    (-0.960_289_856_497_536_3, 0.101_228_536_290_376_3),
    (-0.796_666_477_413_626_7, 0.222_381_034_453_374_5),
    (-0.525_532_409_916_329, 0.313_706_645_877_887_3),
    (-0.183_434_642_495_649_8, 0.362_683_783_378_362),
    (0.183_434_642_495_649_8, 0.362_683_783_378_362),
    (0.525_532_409_916_329, 0.313_706_645_877_887_3),
    (0.796_666_477_413_626_7, 0.222_381_034_453_374_5),
    (0.960_289_856_497_536_3, 0.101_228_536_290_376_3),
];

/// 要素 1 つの 4 つの積分。
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Kern {
    pub l: C,
    pub m: C,
    pub mt: C,
    pub n: C,
}

impl std::ops::AddAssign for Kern {
    fn add_assign(&mut self, o: Self) {
        self.l += o.l;
        self.m += o.m;
        self.mt += o.mt;
        self.n += o.n;
    }
}

/// 点 x(法線 n_x。外の空間の点なら 0 のベクトル)から見た要素 `e` の積分。
/// `own` は x が `e` の重心(自分の要素の選点)の時。
pub(crate) fn element(x: [f64; 3], nx: [f64; 3], e: &Elem, k: f64, own: bool) -> Kern {
    if own {
        return own_element(e, k);
    }
    let mut acc = Kern::default();
    near(x, nx, e.n, e.v, k, 0, &mut acc);
    acc
}

fn near(
    x: [f64; 3],
    nx: [f64; 3],
    ny: [f64; 3],
    v: [[f64; 3]; 3],
    k: f64,
    depth: u32,
    acc: &mut Kern,
) {
    let c = scale(add(add(v[0], v[1]), v[2]), 1.0 / 3.0);
    let s = norm(sub(v[1], v[0]))
        .max(norm(sub(v[2], v[1])))
        .max(norm(sub(v[0], v[2])));
    if depth >= MAX_DEPTH || norm(sub(c, x)) > NEAR * s {
        let area = 0.5 * norm(cross(sub(v[1], v[0]), sub(v[2], v[0])));
        for (b, w) in TRI7 {
            let y = [
                b[0] * v[0][0] + b[1] * v[1][0] + b[2] * v[2][0],
                b[0] * v[0][1] + b[1] * v[1][1] + b[2] * v[2][1],
                b[0] * v[0][2] + b[1] * v[1][2] + b[2] * v[2][2],
            ];
            point(x, nx, y, ny, k, w * area, acc);
        }
        return;
    }
    let m01 = scale(add(v[0], v[1]), 0.5);
    let m12 = scale(add(v[1], v[2]), 0.5);
    let m20 = scale(add(v[2], v[0]), 0.5);
    for t in [
        [v[0], m01, m20],
        [m01, v[1], m12],
        [m20, m12, v[2]],
        [m01, m12, m20],
    ] {
        near(x, nx, ny, t, k, depth + 1, acc);
    }
}

/// 求積の点 1 つの寄与(重み `w` は面積を含む)。
#[inline]
fn point(x: [f64; 3], nx: [f64; 3], y: [f64; 3], ny: [f64; 3], k: f64, w: f64, acc: &mut Kern) {
    let d = sub(y, x);
    let r = norm(d);
    let kr = k * r;
    // e^{−jkr}
    let e = C::new(kr.cos(), -kr.sin());
    let g = e / (FOUR_PI * r);
    // dG/dr = −G (1/r + jk)
    let dg = -g * C::new(1.0 / r, k);
    let dny = dot(d, ny);
    let dnx = dot(d, nx);
    acc.l += g * w;
    acc.m += dg * (dny / r * w);
    acc.mt += dg * (-dnx / r * w);
    // ∂²G/∂n_x∂n_y = −e(3 + 3jkr − k²r²)(d·n_x)(d·n_y)/(4πr⁵) + e(1 + jkr)(n_x·n_y)/(4πr³)
    let r2 = r * r;
    let r3 = r2 * r;
    let a = e * C::new(3.0 - kr * kr, 3.0 * kr) * (-dnx * dny / (FOUR_PI * r3 * r2));
    let b = e * C::new(1.0, kr) * (dot(nx, ny) / (FOUR_PI * r3));
    acc.n += (a + b) * w;
}

/// 自分の要素(選点は重心)。重心と辺で作る 3 つの三角形ごとに、角度の向きを辺に沿って積分する。
/// 辺の足(重心から辺へ下ろした垂線の足)から測った辺の上の位置を s、辺までの距離を h、
/// 距離を R とすると dθ = h/R² ds。k に依らない部分(∫R dθ と ∫dθ/R)は式で出し、
/// 残り(滑らかな部分)を辺に沿った 8 点のガウス求積で足す。
fn own_element(e: &Elem, k: f64) -> Kern {
    let x = e.c;
    let mut l = C::new(0.0, 0.0);
    let mut n = C::new(0.0, 0.0);
    for i in 0..3 {
        let a = e.v[i];
        let b = e.v[(i + 1) % 3];
        let ab = sub(b, a);
        let len = norm(ab);
        let u = scale(ab, 1.0 / len);
        // 重心から辺までの距離(三角形 x-a-b の高さ)と、足から測った両端の位置
        let h = norm(cross(sub(a, x), sub(b, x))) / len;
        let sa = dot(sub(a, x), u);
        let sb = sa + len;
        let (ra, rb) = (norm(sub(a, x)), norm(sub(b, x)));
        // ∫R dθ = h [asinh(s/h)]、∫dθ/R = [s/R]/h
        l += C::new(h * ((sb / h).asinh() - (sa / h).asinh()) / FOUR_PI, 0.0);
        n += C::new(-(sb / rb - sa / ra) / h / FOUR_PI, 0.0);
        for (t, w) in GL8 {
            let s = sa + 0.5 * (t + 1.0) * len;
            let rr = (s * s + h * h).sqrt();
            let dtheta = 0.5 * w * len * h / (rr * rr);
            let kr = k * rr;
            let ekr = C::new(kr.cos(), -kr.sin());
            // ∫₀^R e^{−jkρ}/(4π) dρ = (1 − e^{−jkR})/(4πjk)。R の分は上で足した
            let lr = (C::new(1.0, 0.0) - ekr) / C::new(0.0, k) - rr;
            l += lr / FOUR_PI * dtheta;
            // 有限部分 ∫₀^R e^{−jkρ}(1 + jkρ)/(4πρ²) dρ = (−e^{−jkR}/R − jk)/(4π)。−1/R の分は上で足した
            let nr = (C::new(1.0, 0.0) - ekr) / rr - C::new(0.0, k);
            n += nr / FOUR_PI * dtheta;
        }
    }
    Kern {
        l,
        m: C::new(0.0, 0.0),
        mt: C::new(0.0, 0.0),
        n,
    }
}

/// 遠方の振幅 F の要素の寄与のための積分 ∫ e^{jk x̂·y} dS(7 点の求積)。
pub(crate) fn far(dir: [f64; 3], e: &Elem, k: f64) -> C {
    let mut s = C::new(0.0, 0.0);
    for (b, w) in TRI7 {
        let y = add(
            add(scale(e.v[0], b[0]), scale(e.v[1], b[1])),
            scale(e.v[2], b[2]),
        );
        let ph = k * dot(dir, y);
        s += C::new(ph.cos(), ph.sin()) * w;
    }
    s * e.area
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tri() -> Elem {
        Elem::new([[0.0, 0.0, 0.0], [0.01, 0.0, 0.0], [0.003, 0.008, 0.0]])
    }

    /// 確かめること: 自分の要素の極座標の積分が、選点のまわりを細かく分けた普通の求積と合う
    /// (L は 1/r の弱い特異なので分けるほど近づく)。
    #[test]
    fn own_single_layer_matches_refined_quadrature() {
        let e = tri();
        let k = 200.0;
        let exact = own_element(&e, k).l;
        let mut acc = Kern::default();
        // 重心で 3 つに分け、それぞれを深く分けて求積する
        for i in 0..3 {
            let t = [e.c, e.v[i], e.v[(i + 1) % 3]];
            fine(e.c, t, k, 0, &mut acc);
        }
        assert!(
            (acc.l - exact).norm() < 2e-3 * exact.norm(),
            "{} {}",
            acc.l,
            exact
        );
    }

    fn fine(x: [f64; 3], v: [[f64; 3]; 3], k: f64, depth: u32, acc: &mut Kern) {
        if depth == 7 {
            let area = 0.5 * norm(cross(sub(v[1], v[0]), sub(v[2], v[0])));
            for (b, w) in TRI7 {
                let y = add(add(scale(v[0], b[0]), scale(v[1], b[1])), scale(v[2], b[2]));
                point(x, [0.0; 3], y, [0.0, 0.0, 1.0], k, w * area, acc);
            }
            return;
        }
        let m01 = scale(add(v[0], v[1]), 0.5);
        let m12 = scale(add(v[1], v[2]), 0.5);
        let m20 = scale(add(v[2], v[0]), 0.5);
        for t in [
            [v[0], m01, m20],
            [m01, v[1], m12],
            [m20, m12, v[2]],
            [m01, m12, m20],
        ] {
            fine(x, t, k, depth + 1, acc);
        }
    }

    /// 確かめること: k → 0 の N の有限部分が、平らな要素の静的な値 −(1/4π)∮dθ/R と合う
    /// (辺に沿った求積を、角度で細かく刻んだ和と比べる)。
    #[test]
    fn own_hypersingular_static_value() {
        let e = tri();
        let n = own_element(&e, 1e-9).n;
        // 角度の向きの 1/R を別の道(角度で直接)で積分する
        let mut s = 0.0;
        for i in 0..3 {
            let a = sub(e.v[i], e.c);
            let b = sub(e.v[(i + 1) % 3], e.c);
            let steps = 20000;
            for j in 0..steps {
                let t0 = j as f64 / steps as f64;
                let t1 = (j + 1) as f64 / steps as f64;
                let p0 = add(a, scale(sub(b, a), t0));
                let p1 = add(a, scale(sub(b, a), t1));
                let pm = scale(add(p0, p1), 0.5);
                let dth = (norm(cross(p0, p1)) / (norm(p0) * norm(p1))).asin();
                s += dth / norm(pm);
            }
        }
        let want = -s / FOUR_PI;
        assert!((n.re - want).abs() < 1e-7 * want.abs(), "{} {}", n.re, want);
    }
}
