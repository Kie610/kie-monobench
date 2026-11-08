//! 慣性テンソルの計算(平行軸の定理と主軸)。
//!
//! テンソルは世界の軸に平行な軸についての 3×3 の対称行列で、対角が慣性モーメント
//! (`Ixx = ∫(y²+z²)`)、対角の外が慣性乗積の符号を変えた値(`Ixy = -∫xy`)。
//! 形状カーネルは密度 1 の値(体積で重みを付けた値、長さの 5 乗)を返し、質量の慣性は
//! 使う側が密度を掛けて作る。

use super::P3;

/// 3×3 の慣性テンソル(行と列は x・y・z)。
pub type InertiaTensor = [[f64; 3]; 3];

/// 慣性の主軸と主慣性モーメント。モーメントは小さい順で、`axes[i]` が `moments[i]` の軸
/// (長さ 1)。3 本は右手系で、各軸は絶対値のいちばん大きい成分が正になる向きにそろえる。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PrincipalInertia {
    pub moments: [f64; 3],
    pub axes: [P3; 3],
}

/// 点のまわりの 2 次の積分から慣性テンソルを組む。`moments` は `∫(y²+z²)`・`∫(z²+x²)`・
/// `∫(x²+y²)`、`products` は `∫yz`・`∫zx`・`∫xy`(正の積分のまま)。
pub fn inertia_from_integrals(moments: [f64; 3], products: [f64; 3]) -> InertiaTensor {
    let [yz, zx, xy] = products;
    [
        [moments[0], -xy, -zx],
        [-xy, moments[1], -yz],
        [-zx, -yz, moments[2]],
    ]
}

/// 平行軸の定理の足し分 `m(|d|²E - d dᵀ)`。
fn steiner(mass: f64, d: P3) -> InertiaTensor {
    let dd = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
    let mut t = [[0.0; 3]; 3];
    for (i, row) in t.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            let delta = if i == j { dd } else { 0.0 };
            *v = mass * (delta - d[i] * d[j]);
        }
    }
    t
}

/// 重心まわりのテンソル `at_centroid` を、点 `point` を通る平行な軸のまわりへ移す(平行軸の定理)。
/// `mass` は密度 1 なら体積。組立で部品の慣性を足し合わせる時にも使う。
pub fn inertia_about(
    at_centroid: InertiaTensor,
    mass: f64,
    centroid: P3,
    point: P3,
) -> InertiaTensor {
    let d = [
        centroid[0] - point[0],
        centroid[1] - point[1],
        centroid[2] - point[2],
    ];
    let s = steiner(mass, d);
    let mut t = at_centroid;
    for i in 0..3 {
        for j in 0..3 {
            t[i][j] += s[i][j];
        }
    }
    t
}

/// 点 `point` のまわりのテンソルを重心のまわりへ戻す([`inertia_about`] の逆)。
pub fn inertia_at_centroid(
    about_point: InertiaTensor,
    mass: f64,
    centroid: P3,
    point: P3,
) -> InertiaTensor {
    let d = [
        centroid[0] - point[0],
        centroid[1] - point[1],
        centroid[2] - point[2],
    ];
    let s = steiner(mass, d);
    let mut t = about_point;
    for i in 0..3 {
        for j in 0..3 {
            t[i][j] -= s[i][j];
        }
    }
    t
}

/// 主軸と主慣性モーメント(対称行列の固有値と固有ベクトル。ヤコビ法)。
/// 主慣性が重なる形(円柱の軸に直交する 2 本、立方体の 3 本など)は、その中の軸が一意に
/// 決まらない。対角のテンソルは世界の軸をそのまま返すので、同じ形からは同じ軸が出る。
pub fn principal_inertia(t: &InertiaTensor) -> PrincipalInertia {
    let mut a = *t;
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let scale = (0..3).map(|i| a[i][i].abs()).fold(0.0_f64, f64::max);
    for _ in 0..50 {
        let off = a[0][1].abs() + a[0][2].abs() + a[1][2].abs();
        if off <= 1e-15 * scale.max(f64::MIN_POSITIVE) {
            break;
        }
        for (p, q) in [(0, 1), (0, 2), (1, 2)] {
            if a[p][q].abs() <= f64::MIN_POSITIVE {
                continue;
            }
            // a[p][q] を 0 にする回転
            let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
            let tan = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
            let c = 1.0 / (tan * tan + 1.0).sqrt();
            let s = tan * c;
            for row in a.iter_mut() {
                let (akp, akq) = (row[p], row[q]);
                row[p] = c * akp - s * akq;
                row[q] = s * akp + c * akq;
            }
            let (rp, rq) = (a[p], a[q]);
            a[p] = std::array::from_fn(|k| c * rp[k] - s * rq[k]);
            a[q] = std::array::from_fn(|k| s * rp[k] + c * rq[k]);
            for row in v.iter_mut() {
                let (vp, vq) = (row[p], row[q]);
                row[p] = c * vp - s * vq;
                row[q] = s * vp + c * vq;
            }
        }
    }
    // 列 i が固有値 a[i][i] の固有ベクトル
    let mut order = [0, 1, 2];
    order.sort_by(|&i, &j| a[i][i].total_cmp(&a[j][j]));
    let column = |i: usize| -> P3 {
        let mut c = [v[0][i], v[1][i], v[2][i]];
        let big = (0..3)
            .max_by(|&x, &y| c[x].abs().total_cmp(&c[y].abs()))
            .unwrap_or(0);
        if c[big] < 0.0 {
            c = [-c[0], -c[1], -c[2]];
        }
        c
    };
    let a0 = column(order[0]);
    let a1 = column(order[1]);
    let a2 = [
        a0[1] * a1[2] - a0[2] * a1[1],
        a0[2] * a1[0] - a0[0] * a1[2],
        a0[0] * a1[1] - a0[1] * a1[0],
    ];
    PrincipalInertia {
        moments: [
            a[order[0]][order[0]],
            a[order[1]][order[1]],
            a[order[2]][order[2]],
        ],
        axes: [a0, a1, a2],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-9 * b.abs().max(1.0)
    }

    /// 確かめること: 重心から離れた点へ移して戻すと元に戻り、移した値は平行軸の定理の式と合う。
    #[test]
    fn moving_to_a_point_and_back_returns_the_same_tensor() {
        // 1×2×3 の直方体(密度 1、体積 6)の重心まわり
        let (a, b, c) = (1.0, 2.0, 3.0);
        let m = a * b * c;
        let t = [
            [m * (b * b + c * c) / 12.0, 0.0, 0.0],
            [0.0, m * (a * a + c * c) / 12.0, 0.0],
            [0.0, 0.0, m * (a * a + b * b) / 12.0],
        ];
        let centroid = [0.5, 1.0, 1.5];
        let corner = [0.0, 0.0, 0.0];
        let at = inertia_about(t, m, centroid, corner);
        // 角のまわり: Ixx = m(b²+c²)/3、Ixy = -m·a·b/4
        assert!(near(at[0][0], m * (b * b + c * c) / 3.0));
        assert!(near(at[0][1], -m * a * b / 4.0));
        assert!(near(at[1][2], -m * b * c / 4.0));
        let back = inertia_at_centroid(at, m, centroid, corner);
        for i in 0..3 {
            for j in 0..3 {
                assert!(near(back[i][j], t[i][j]), "{back:?}");
            }
        }
    }

    /// 確かめること: z 軸のまわりに回した直方体のテンソルから、元の向きの軸と主慣性が戻る。
    #[test]
    fn principal_axes_undo_a_rotation() {
        let d = [1.0, 4.0, 9.0];
        let (s, c) = (30f64.to_radians().sin(), 30f64.to_radians().cos());
        let r = [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]];
        // R diag(d) Rᵀ
        let mut t = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                t[i][j] = (0..3).map(|k| r[i][k] * d[k] * r[j][k]).sum();
            }
        }
        let p = principal_inertia(&t);
        for (got, want) in p.moments.iter().zip(d) {
            assert!(near(*got, want), "{p:?}");
        }
        assert!(near(p.axes[0][0], c) && near(p.axes[0][1], s), "{p:?}");
        assert!(near(p.axes[2][2], 1.0), "{p:?}");
    }

    /// 確かめること: 慣性の欄の無い前の形の値も読め、読んだ値は慣性を対応していない誤りにする。
    /// 慣性の無い値は書き出しても欄が増えない(前の文書の質量の値が変わらない)。
    #[test]
    fn mass_props_without_inertia_still_read_and_write_the_same() {
        use super::super::{KernelError, MassProps};
        let old = r#"{"volume":6.0,"area":22.0,"min":[0.0,0.0,0.0],"max":[1.0,2.0,3.0],"centroid":[0.5,1.0,1.5]}"#;
        let m: MassProps = serde_json::from_str(old).unwrap();
        assert_eq!(m.volume, 6.0);
        assert_eq!(m.centroid, Some([0.5, 1.0, 1.5]));
        assert!(matches!(m.inertia(), Err(KernelError::Unsupported(_))));
        assert!(m.principal().is_err());
        assert_eq!(serde_json::to_string(&m).unwrap(), old);
    }

    /// 確かめること: 対角のテンソルは世界の軸を小さい順に返し、重なった値でも軸が揺れない。
    #[test]
    fn a_diagonal_tensor_keeps_the_world_axes() {
        let p = principal_inertia(&[[5.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 2.0]]);
        assert_eq!(p.moments, [2.0, 2.0, 5.0]);
        assert_eq!(p.axes, [[0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]]);
    }
}
