//! 10 節点の四面体要素(2 次)。角の 4 点と辺の中点の 6 点で、辺の中点を本当の曲面に乗せると
//! 要素の面が曲がり、穴や丸みの形に沿う。
//!
//! 節点の順は角 0〜3、辺の中点 4:(0,1)・5:(1,2)・6:(0,2)・7:(0,3)・8:(1,3)・9:(2,3)。
//! 自然座標 (ξ, η, ζ) で角 0 が原点、重心座標は
//! L0 = 1−ξ−η−ζ、L1 = ξ、L2 = η、L3 = ζ。
//!
//! 剛性と熱伝導は 4 点のガウス積分(2 次まで正確、C3D10 の標準)、質量と体積は円錐の積
//! (Duffy の変換とガウス・ルジャンドル 4 点)で 5 次まで正確に積分する。要素が裏返って
//! いないかは、ヤコビアンの行列式(3 次の多項式)を Bernstein の係数に直し、その最小が正か
//! で確かめる(節点や積分点だけを調べると負の所を見逃す。Johnen・Remacle・Geuzaine 2013)。

use crate::geom::P3;
use nalgebra::{DMatrix, Matrix3};
use std::sync::OnceLock;

/// 辺の中点の節点(4〜9)の両端の角。
pub const EDGES: [[usize; 2]; 6] = [[0, 1], [1, 2], [0, 2], [0, 3], [1, 3], [2, 3]];

/// 外向きの面(角の番号、外から見て反時計回り)と、その面の辺の中点の節点
/// (角 a→b、b→c、c→a の順)。向きは角 0→1, 0→2, 0→3 が右手系(体積が正)の時。
pub const FACES: [([usize; 3], [usize; 3]); 4] = [
    ([0, 2, 1], [6, 5, 4]),
    ([0, 1, 3], [4, 8, 7]),
    ([1, 2, 3], [5, 9, 8]),
    ([0, 3, 2], [7, 9, 6]),
];

/// 節点の自然座標。
pub const NODE_XI: [[f64; 3]; 10] = [
    [0.0, 0.0, 0.0],
    [1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, 0.0, 1.0],
    [0.5, 0.0, 0.0],
    [0.5, 0.5, 0.0],
    [0.0, 0.5, 0.0],
    [0.0, 0.0, 0.5],
    [0.5, 0.0, 0.5],
    [0.0, 0.5, 0.5],
];

fn bary(x: [f64; 3]) -> [f64; 4] {
    [1.0 - x[0] - x[1] - x[2], x[0], x[1], x[2]]
}

/// 重心座標の自然座標での微分。
const DL: [[f64; 3]; 4] = [
    [-1.0, -1.0, -1.0],
    [1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, 0.0, 1.0],
];

/// 形関数。
pub fn shape(x: [f64; 3]) -> [f64; 10] {
    let l = bary(x);
    let mut n = [0.0; 10];
    for i in 0..4 {
        n[i] = l[i] * (2.0 * l[i] - 1.0);
    }
    for (e, [a, b]) in EDGES.iter().enumerate() {
        n[4 + e] = 4.0 * l[*a] * l[*b];
    }
    n
}

/// 形関数の自然座標での微分。
pub fn dshape(x: [f64; 3]) -> [[f64; 3]; 10] {
    let l = bary(x);
    let mut d = [[0.0; 3]; 10];
    for i in 0..4 {
        for k in 0..3 {
            d[i][k] = (4.0 * l[i] - 1.0) * DL[i][k];
        }
    }
    for (e, [a, b]) in EDGES.iter().enumerate() {
        for k in 0..3 {
            d[4 + e][k] = 4.0 * (DL[*a][k] * l[*b] + l[*a] * DL[*b][k]);
        }
    }
    d
}

/// 4 点のガウス積分(重みは体積 1/6 込み)。
pub fn gauss4() -> [([f64; 3], f64); 4] {
    let a = 0.585_410_196_624_968_5;
    let b = 0.138_196_601_125_010_5;
    let w = 1.0 / 24.0;
    [
        ([b, b, b], w),
        ([a, b, b], w),
        ([b, a, b], w),
        ([b, b, a], w),
    ]
}

/// 円錐の積の積分点(Duffy の変換 + ガウス・ルジャンドル 4 点、5 次まで正確、64 点)。
pub fn gauss_fine() -> &'static [([f64; 3], f64)] {
    static RULE: OnceLock<Vec<([f64; 3], f64)>> = OnceLock::new();
    RULE.get_or_init(|| {
        let g = [
            (-0.861_136_311_594_052_6, 0.347_854_845_137_453_8),
            (-0.339_981_043_584_856_3, 0.652_145_154_862_546_1),
            (0.339_981_043_584_856_3, 0.652_145_154_862_546_1),
            (0.861_136_311_594_052_6, 0.347_854_845_137_453_8),
        ]
        .map(|(x, w)| ((x + 1.0) / 2.0, w / 2.0));
        let mut out = Vec::with_capacity(64);
        for (u, wu) in g {
            for (v, wv) in g {
                for (w, ww) in g {
                    let p = [u, (1.0 - u) * v, (1.0 - u) * (1.0 - v) * w];
                    out.push((p, wu * wv * ww * (1.0 - u) * (1.0 - u) * (1.0 - v)));
                }
            }
        }
        out
    })
}

/// ヤコビ行列 J[i][k] = ∂x_i/∂ξ_k と、その行列式。
pub fn jacobian(x: &[P3; 10], xi: [f64; 3]) -> (Matrix3<f64>, f64) {
    let d = dshape(xi);
    let mut j = Matrix3::zeros();
    for (a, da) in d.iter().enumerate() {
        for i in 0..3 {
            for k in 0..3 {
                j[(i, k)] += x[a][i] * da[k];
            }
        }
    }
    let det = j.determinant();
    (j, det)
}

/// 形関数の空間での勾配と、ヤコビアンの行列式。
pub fn gradients(x: &[P3; 10], xi: [f64; 3]) -> ([[f64; 3]; 10], f64) {
    let d = dshape(xi);
    let (j, det) = jacobian(x, xi);
    let inv = j.try_inverse().unwrap_or_else(Matrix3::zeros);
    let mut g = [[0.0; 3]; 10];
    for a in 0..10 {
        // ∂N/∂x_i = Σ_k ∂N/∂ξ_k ∂ξ_k/∂x_i
        for i in 0..3 {
            g[a][i] = (0..3).map(|k| d[a][k] * inv[(k, i)]).sum();
        }
    }
    (g, det)
}

/// ラメの定数 (λ, μ)。
pub fn lame(e: f64, nu: f64) -> (f64, f64) {
    (
        e * nu / ((1.0 + nu) * (1.0 - 2.0 * nu)),
        e / (2.0 * (1.0 + nu)),
    )
}

/// 弾性の剛性行列(30×30、行優先。節点ごとに x, y, z)。
pub fn stiffness(x: &[P3; 10], e: f64, nu: f64) -> Vec<f64> {
    let (l, m) = lame(e, nu);
    let mut k = vec![0.0; 900];
    for (xi, w) in gauss4() {
        let (g, det) = gradients(x, xi);
        let wd = w * det;
        for a in 0..10 {
            for b in 0..10 {
                let gg = g[a][0] * g[b][0] + g[a][1] * g[b][1] + g[a][2] * g[b][2];
                for i in 0..3 {
                    let row = (3 * a + i) * 30 + 3 * b;
                    for j in 0..3 {
                        let mut v = l * g[a][i] * g[b][j] + m * g[a][j] * g[b][i];
                        if i == j {
                            v += m * gg;
                        }
                        k[row + j] += wd * v;
                    }
                }
            }
        }
    }
    k
}

/// 熱伝導の行列(10×10、行優先。熱伝導率 1)。
pub fn conduction(x: &[P3; 10]) -> Vec<f64> {
    let mut k = vec![0.0; 100];
    for (xi, w) in gauss4() {
        let (g, det) = gradients(x, xi);
        for a in 0..10 {
            for b in 0..10 {
                k[a * 10 + b] +=
                    w * det * (g[a][0] * g[b][0] + g[a][1] * g[b][1] + g[a][2] * g[b][2]);
            }
        }
    }
    k
}

/// 整合した質量行列(10×10、行優先。密度 1)。
pub fn mass(x: &[P3; 10]) -> Vec<f64> {
    let mut m = vec![0.0; 100];
    for (xi, w) in gauss_fine() {
        let n = shape(*xi);
        let (_, det) = jacobian(x, *xi);
        for a in 0..10 {
            for b in 0..10 {
                m[a * 10 + b] += w * det * n[a] * n[b];
            }
        }
    }
    m
}

/// 要素の体積。
pub fn volume(x: &[P3; 10]) -> f64 {
    gauss_fine()
        .iter()
        .map(|(xi, w)| w * jacobian(x, *xi).1)
        .sum()
}

/// 変位 u(節点ごとに x, y, z)から、点 xi の応力(Voigt の順 xx, yy, zz, xy, yz, zx)。
pub fn stress_at(x: &[P3; 10], e: f64, nu: f64, u: &[f64; 30], xi: [f64; 3]) -> [f64; 6] {
    let (l, m) = lame(e, nu);
    let (g, _) = gradients(x, xi);
    let mut du = [[0.0; 3]; 3]; // du[i][j] = ∂u_i/∂x_j
    for a in 0..10 {
        for i in 0..3 {
            for j in 0..3 {
                du[i][j] += u[3 * a + i] * g[a][j];
            }
        }
    }
    let tr = du[0][0] + du[1][1] + du[2][2];
    [
        l * tr + 2.0 * m * du[0][0],
        l * tr + 2.0 * m * du[1][1],
        l * tr + 2.0 * m * du[2][2],
        m * (du[0][1] + du[1][0]),
        m * (du[1][2] + du[2][1]),
        m * (du[2][0] + du[0][2]),
    ]
}

/// 応力(Voigt の順)からミーゼス応力。
pub fn von_mises(s: &[f64]) -> f64 {
    let (a, b, c) = (s[0] - s[1], s[1] - s[2], s[2] - s[0]);
    (0.5 * (a * a + b * b + c * c) + 3.0 * (s[3] * s[3] + s[4] * s[4] + s[5] * s[5])).sqrt()
}

/// 3 次の Bernstein の係数の多重指数(和が 3)。
fn multi() -> &'static [[usize; 4]; 20] {
    static M: OnceLock<[[usize; 4]; 20]> = OnceLock::new();
    M.get_or_init(|| {
        let mut out = [[0; 4]; 20];
        let mut k = 0;
        for i in 0..=3 {
            for j in 0..=3 - i {
                for l in 0..=3 - i - j {
                    out[k] = [3 - i - j - l, i, j, l];
                    k += 1;
                }
            }
        }
        out
    })
}

/// 標本点の値から Bernstein の係数へ直す行列(20×20 の逆行列)。
fn to_bernstein() -> &'static DMatrix<f64> {
    static B: OnceLock<DMatrix<f64>> = OnceLock::new();
    B.get_or_init(|| {
        let ms = multi();
        let fact = [1.0, 1.0, 2.0, 6.0];
        let mut b = DMatrix::zeros(20, 20);
        for (p, mp) in ms.iter().enumerate() {
            let l = mp.map(|v| v as f64 / 3.0);
            for (q, mq) in ms.iter().enumerate() {
                let c = 6.0 / mq.iter().map(|v| fact[*v]).product::<f64>();
                b[(p, q)] = c * (0..4).map(|i| l[i].powi(mq[i] as i32)).product::<f64>();
            }
        }
        b.try_inverse().expect("Bernstein の標本の行列は正則")
    })
}

/// ヤコビアンの行列式の、要素全体での下限と上限(Bernstein の係数の最小と最大)。
/// 下限が正なら要素のどこでも裏返っていない。
pub fn jacobian_bounds(x: &[P3; 10]) -> (f64, f64) {
    let ms = multi();
    let vals = DMatrix::from_iterator(
        20,
        1,
        ms.iter().map(|m| {
            let xi = [m[1] as f64 / 3.0, m[2] as f64 / 3.0, m[3] as f64 / 3.0];
            jacobian(x, xi).1
        }),
    );
    let c = to_bernstein() * vals;
    (c.min(), c.max())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn straight(c: [P3; 4]) -> [P3; 10] {
        let mut x = [[0.0; 3]; 10];
        x[..4].copy_from_slice(&c);
        for (e, [a, b]) in EDGES.iter().enumerate() {
            x[4 + e] = crate::geom::lerp(c[*a], c[*b], 0.5);
        }
        x
    }

    fn unit() -> [P3; 10] {
        straight([[0., 0., 0.], [2., 0., 0.], [0., 3., 0.], [0., 0., 1.5]])
    }

    #[test]
    fn shape_functions_sum_to_one_and_hit_nodes() {
        for (i, xi) in NODE_XI.iter().enumerate() {
            let n = shape(*xi);
            for (j, v) in n.iter().enumerate() {
                assert!((v - if i == j { 1.0 } else { 0.0 }).abs() < 1e-14);
            }
        }
        let n = shape([0.2, 0.3, 0.1]);
        assert!((n.iter().sum::<f64>() - 1.0).abs() < 1e-14);
        let d = dshape([0.2, 0.3, 0.1]);
        for k in 0..3 {
            assert!(d.iter().map(|v| v[k]).sum::<f64>().abs() < 1e-13);
        }
    }

    #[test]
    fn volume_mass_and_faces() {
        let x = unit();
        assert!((volume(&x) - 1.5).abs() < 1e-12);
        let m = mass(&x);
        assert!((m.iter().sum::<f64>() - 1.5).abs() < 1e-12);
        // 外向きの面: 面の法線は向かいの角から離れる
        for (f, _) in FACES {
            let opp = (0..4).find(|v| !f.contains(v)).unwrap();
            let n = crate::geom::cross(
                crate::geom::sub(x[f[1]], x[f[0]]),
                crate::geom::sub(x[f[2]], x[f[0]]),
            );
            assert!(crate::geom::dot(n, crate::geom::sub(x[opp], x[f[0]])) < 0.0);
        }
        let (lo, hi) = jacobian_bounds(&x);
        assert!(
            (lo - 9.0).abs() < 1e-9 && (hi - 9.0).abs() < 1e-9,
            "{lo} {hi}"
        );
    }

    /// 一様な温度では熱が流れない(行の和が 0)。
    #[test]
    fn conduction_rows_sum_to_zero() {
        let k = conduction(&unit());
        for r in 0..10 {
            let s: f64 = (0..10).map(|c| k[r * 10 + c]).sum();
            assert!(s.abs() < 1e-12, "行 {r}: {s}");
        }
    }

    #[test]
    fn rigid_moves_make_no_force_and_stretch_gives_uniform_stress() {
        let x = unit();
        let k = stiffness(&x, 1.0, 0.3);
        type Move = Box<dyn Fn(P3) -> P3>;
        let moves: Vec<Move> = vec![
            Box::new(|_| [1.0, 0.0, 0.0]),
            Box::new(|p| [-p[1], p[0], 0.0]),
            Box::new(|p| [p[2], 0.0, -p[0]]),
        ];
        for mv in moves {
            let u: Vec<f64> = x.iter().flat_map(|p| mv(*p)).collect();
            for r in 0..30 {
                let f: f64 = (0..30).map(|c| k[r * 30 + c] * u[c]).sum();
                assert!(f.abs() < 1e-10, "剛体の動きで力 {f}");
            }
        }
        let u: [f64; 30] =
            std::array::from_fn(|i| if i % 3 == 0 { 1e-3 * x[i / 3][0] } else { 0.0 });
        let nu = 0.3;
        let want = (1.0 - nu) / ((1.0 + nu) * (1.0 - 2.0 * nu)) * 1e-3;
        for xi in NODE_XI {
            let s = stress_at(&x, 1.0, nu, &u, xi);
            assert!((s[0] - want).abs() < 1e-12);
        }
    }

    #[test]
    fn a_folded_midside_node_is_caught() {
        let mut x = unit();
        assert!(jacobian_bounds(&x).0 > 0.0);
        // 辺 0-1 の中点を角 2 の向こうまで押し込む
        x[4] = [1.0, 4.0, 0.0];
        assert!(jacobian_bounds(&x).0 < 0.0);
        // 少しだけ曲げるのは良い
        x[4] = [1.0, 0.2, 0.0];
        assert!(jacobian_bounds(&x).0 > 0.0);
    }
}
