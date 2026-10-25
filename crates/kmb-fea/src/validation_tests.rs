//! 既知の解と比べる検算(docs/verify.md、調査 構造と熱の解析.md 5.4 節)。箱の検算に加えて、
//! 曲面の例(厚肉の円筒の内圧・中空の円筒の熱・丸棒の片持ち梁の共振)を理論と比べる。

use crate::amg::rigid_modes;
use crate::boundary::Prepared;
use crate::modal::{ModalInput, solve_modal};
use crate::solve::{apply_fixed, assemble, solve};
use crate::statics::{Dir, Load, LoadKind, StaticInput, solve_static};
use crate::statics::{fixed_mask, load_vector, stresses};
use crate::tet10;
use crate::tetmesh::tests::{annulus, cylinder};
use crate::thermal::{ThermalInput, solve_thermal};
use crate::topology::{TopologyInput, solve_topology};
use kmb_core::Mesh;

/// 箱の網(面の名前は x0・x1・y0・y1・z0・z1)。
pub(crate) fn box_mesh(l: [f32; 3]) -> Mesh {
    let mut m = Mesh::default();
    let faces: [(&str, [[f32; 3]; 4]); 6] = [
        (
            "x0",
            [[0., 0., 0.], [0., 0., 1.], [0., 1., 1.], [0., 1., 0.]],
        ),
        (
            "x1",
            [[1., 0., 0.], [1., 1., 0.], [1., 1., 1.], [1., 0., 1.]],
        ),
        (
            "y0",
            [[0., 0., 0.], [1., 0., 0.], [1., 0., 1.], [0., 0., 1.]],
        ),
        (
            "y1",
            [[0., 1., 0.], [0., 1., 1.], [1., 1., 1.], [1., 1., 0.]],
        ),
        (
            "z0",
            [[0., 0., 0.], [0., 1., 0.], [1., 1., 0.], [1., 0., 0.]],
        ),
        (
            "z1",
            [[0., 0., 1.], [1., 0., 1.], [1., 1., 1.], [0., 1., 1.]],
        ),
    ];
    for (fi, (name, q)) in faces.iter().enumerate() {
        let base = m.positions.len() as u32;
        for p in q {
            m.positions.push([p[0] * l[0], p[1] * l[1], p[2] * l[2]]);
        }
        m.triangles.push([base, base + 1, base + 2]);
        m.triangles.push([base, base + 2, base + 3]);
        m.triangle_faces.extend([fi as u32, fi as u32]);
        m.face_names.push(name.to_string());
    }
    m
}

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn near(got: f64, want: f64, rel: f64, what: &str) {
    let e = (got - want).abs() / want.abs();
    assert!(
        e <= rel,
        "{what}: {got} と既知の解 {want} の差 {:.2}%(許容 {:.1}%)",
        e * 100.0,
        rel * 100.0
    );
    eprintln!("{what}: {got:.5}(既知の解 {want:.5}、差 {:.2}%)", e * 100.0);
}

fn prepared(m: &Mesh, h: f64) -> Prepared {
    let p = Prepared::new(m, h, &|| false).unwrap();
    eprintln!(
        "網: 要素 {}・節点 {}・{:?}",
        p.mesh.tets.len(),
        p.mesh.points.len(),
        p.mesh.stats
    );
    p
}

/// 条件に合う節点の値の平均。
fn mean_over(p: &Prepared, pick: impl Fn([f64; 3]) -> bool, value: impl Fn(usize) -> f64) -> f64 {
    let v: Vec<f64> = (0..p.mesh.points.len())
        .filter(|n| pick(p.mesh.points[*n]))
        .map(value)
        .collect();
    assert!(!v.is_empty(), "条件に合う節点がない");
    v.iter().sum::<f64>() / v.len() as f64
}

fn beam() -> Prepared {
    prepared(&box_mesh([100.0, 10.0, 10.0]), 2.5)
}

#[test]
fn box_is_meshed_to_its_volume() {
    let p = beam();
    near(p.mesh.stats.volume, 10_000.0, 1e-9, "箱の網の体積(mm³)");
    // 外の三角形はどれも面に結び付き、6 面とも使う
    assert!(p.mesh.faces.iter().all(|f| (f.face as usize) < 6));
    for f in ["x0", "x1", "y0", "y1", "z0", "z1"] {
        let fs = p.faces_of(&names(&[f])).unwrap();
        let want = if f.starts_with('x') { 100.0 } else { 1000.0 };
        near(p.area(&fs), want, 1e-9, &format!("面 {f} の面積(mm²)"));
    }
}

/// 片持ち梁(長さ 100、断面 10×10 のアルミ、先に 100 N 下向き)。先のたわみは
/// PL³/(3EI) + PL/(κGA)(せん断の分、κ = 5/6)。
#[test]
fn cantilever_tip_deflection_matches_beam_theory() {
    let p = beam();
    let (e, nu) = (70_000.0, 0.3);
    let input = StaticInput {
        e,
        nu,
        fixed: names(&["x0"]),
        loads: vec![Load {
            faces: names(&["x1"]),
            kind: LoadKind::Force(100.0),
            dir: Dir::Fixed([0.0, 0.0, -1.0]),
        }],
    };
    let out = solve_static(&p, &input).unwrap();
    eprintln!("共役勾配法 {} 回", out.iterations);
    let i = 10.0f64.powi(4) / 12.0;
    let g = e / (2.0 * (1.0 + nu));
    let want = 100.0 * 100.0f64.powi(3) / (3.0 * e * i) + 100.0 * 100.0 / (5.0 / 6.0 * g * 100.0);
    let tip = mean_over(&p, |x| x[0] > 100.0 - 1e-9, |n| -out.u[n][2]);
    near(tip, want, 0.02, "片持ち梁の先のたわみ(mm)");
    // 真ん中(x = 50)の上の面の曲げ応力 σxx = M c / I = 100·50·5/I = 30 MPa
    let top = mean_over(
        &p,
        |x| (x[0] - 50.0).abs() < 1.3 && x[2] > 10.0 - 1e-9 && (x[1] - 5.0).abs() < 3.0,
        |n| out.node_stress[n][0],
    );
    near(top, 30.0, 0.04, "真ん中の上の面の曲げ応力(MPa)");
}

/// 棒の引張(長さ 100、断面 10×10、先に 1000 N)。伸び FL/(EA)、応力 F/A。2 次の要素は
/// 一様な引張を丸めの誤差まで表す。
#[test]
fn bar_in_tension_matches_hookes_law() {
    let p = beam();
    let input = StaticInput {
        e: 200_000.0,
        nu: 0.0,
        fixed: names(&["x0"]),
        loads: vec![Load {
            faces: names(&["x1"]),
            kind: LoadKind::Pressure(-10.0), // 引っ張る(押す向きの逆)
            dir: Dir::Normal,
        }],
    };
    let out = solve_static(&p, &input).unwrap();
    let tip = mean_over(&p, |x| x[0] > 100.0 - 1e-9, |n| out.u[n][0]);
    near(
        tip,
        1000.0 * 100.0 / (200_000.0 * 100.0),
        1e-5,
        "棒の伸び(mm)",
    );
    let mid = mean_over(&p, |x| (x[0] - 50.0).abs() < 10.0, |n| out.node_vm[n]);
    near(mid, 10.0, 1e-5, "棒の中ほどの応力(MPa)");
}

/// 両端の温度を決めた棒は、温度が直線になる。
#[test]
fn bar_between_two_temperatures_is_linear() {
    let p = beam();
    let out = solve_thermal(
        &p,
        &ThermalInput {
            k: 200.0,
            temps: vec![(names(&["x0"]), 100.0), (names(&["x1"]), 0.0)],
            heats: vec![],
            convs: vec![],
            rest_h: None,
            ambient: 20.0,
        },
    )
    .unwrap();
    for (n, x) in p.mesh.points.iter().enumerate() {
        let want = 100.0 * (1.0 - x[0] / 100.0);
        assert!(
            (out.t[n] - want).abs() < 1e-5,
            "{x:?}: {} と {want}",
            out.t[n]
        );
    }
}

/// 根元を 100 ℃ にしたアルミの棒の放熱(ピン・フィン)。先の温度は
/// θ(L)/θ₀ = 1 / (cosh mL + (h/mk) sinh mL)、m = √(hP/(kA))。
#[test]
fn pin_fin_tip_temperature_matches_fin_equation() {
    let p = beam();
    let (k, h, amb) = (200.0, 50.0, 20.0);
    let out = solve_thermal(
        &p,
        &ThermalInput {
            k,
            temps: vec![(names(&["x0"]), 100.0)],
            heats: vec![],
            convs: vec![],
            rest_h: Some(h),
            ambient: amb,
        },
    )
    .unwrap();
    let (km, hm) = (k / 1000.0, h / 1e6);
    let m = (hm * 40.0 / (km * 100.0)).sqrt();
    let ml = m * 100.0;
    let want = amb + 80.0 / (ml.cosh() + hm / (m * km) * ml.sinh());
    let tip = mean_over(&p, |x| x[0] > 100.0 - 1e-9, |n| out.t[n]);
    near(tip, want, 0.01, "ピン・フィンの先の温度(℃)");
    // 入った熱は空気へ逃げた熱とつり合う(根元の熱は温度の分布から見積もらず、逃げた熱が正)
    assert!(out.to_air > 0.0);
}

/// 片持ち梁の 1 次の固有振動数 f₁ = (1.875²/2π)·√(EI/(ρAL⁴))。
#[test]
fn cantilever_first_frequency_matches_beam_theory() {
    let p = beam();
    let (e, rho) = (70_000.0, 2.7);
    let out = solve_modal(
        &p,
        &ModalInput {
            e,
            nu: 0.3,
            density: rho,
            fixed: names(&["x0"]),
            count: 3,
        },
    )
    .unwrap();
    let i = 10.0f64.powi(4) / 12.0;
    let want = 1.875f64.powi(2) / std::f64::consts::TAU
        * (e * i / (rho * 1e-9 * 100.0 * 100.0f64.powi(4))).sqrt();
    // 正方形の断面なので、上下と左右の 2 つの曲げがほぼ同じ周波数
    near(out.freqs[0], want, 0.03, "片持ち梁の 1 次の固有振動数(Hz)");
    near(
        out.freqs[1],
        want,
        0.03,
        "片持ち梁の 1 次の固有振動数・横(Hz)",
    );
    assert!(out.freqs[2] > out.freqs[1] * 1.5);
}

/// 軽い形: 体積を決めた割合に抑え、繰り返すほどたわみにくくなる。
#[test]
fn topology_keeps_the_volume_and_lowers_compliance() {
    let p = prepared(&box_mesh([60.0, 4.0, 20.0]), 3.0);
    let out = solve_topology(
        &p,
        &TopologyInput {
            e: 70_000.0,
            nu: 0.3,
            fixed: names(&["x0"]),
            loads: vec![Load {
                faces: names(&["x1"]),
                kind: LoadKind::Force(100.0),
                dir: Dir::Fixed([0.0, 0.0, -1.0]),
            }],
            volume: 0.4,
            radius: 3.0,
            iterations: 25,
        },
        |_, _| {},
    )
    .unwrap();
    let vol: Vec<f64> = (0..p.mesh.tets.len())
        .map(|e| tet10::volume(&p.mesh.tet_points(e)))
        .collect();
    let frac = out
        .density
        .iter()
        .zip(&vol)
        .map(|(d, v)| d * v)
        .sum::<f64>()
        / vol.iter().sum::<f64>();
    assert!((frac - 0.4).abs() < 0.01, "体積の割合 {frac}");
    let c = &out.compliance;
    assert!(c[c.len() - 1] < c[1], "コンプライアンス {c:?}");
}

/// 厚肉の円筒の内圧(Lamé の解、平面ひずみ)。内の半径 a = 5、外 b = 10、内圧 p = 10 MPa。
/// 内の面の周の応力 σθ(a) = p (a² + b²)/(b² − a²)、半径の向きの変位
/// u(a) = (1+ν) p a² ((1−2ν) a + b²/a) / (E (b² − a²))。上と下の面は高さの向きだけ止め
/// (平面ひずみ)、回転と平行移動は 3 つの未知数だけで止める(静定なので反力が出ない)。
#[test]
fn a_thick_cylinder_under_pressure_matches_lame() {
    let (a, b, l, pr) = (5.0, 10.0, 3.0, 10.0);
    let (e, nu) = (200_000.0, 0.3);
    let p = prepared(&annulus(a, b, l, 120, 2), 0.9);
    let m = &p.mesh;
    let mut k = assemble(m, 3, |el| Some(tet10::stiffness(&m.tet_points(el), e, nu)));
    let mut f = load_vector(
        &p,
        &[Load {
            faces: names(&["inner"]),
            kind: LoadKind::Pressure(pr),
            dir: Dir::Normal,
        }],
    )
    .unwrap();
    let mut fixed: Vec<(usize, f64)> = Vec::new();
    let ends = p.faces_of(&names(&["bottom", "top"])).unwrap();
    for n in p.nodes_of(&ends) {
        fixed.push((3 * n as usize + 2, 0.0));
    }
    // 外の面の (b, 0) と (−b, 0) に近い節点(高さは真ん中に近い物)
    let nearest = |q: [f64; 3]| {
        (0..m.points.len())
            .min_by(|i, j| {
                crate::geom::dist(m.points[*i], q).total_cmp(&crate::geom::dist(m.points[*j], q))
            })
            .unwrap()
    };
    let na = nearest([b, 0.0, 0.5 * l]);
    let nb = nearest([-b, 0.0, 0.5 * l]);
    fixed.extend([(3 * na, 0.0), (3 * na + 1, 0.0), (3 * nb + 1, 0.0)]);
    apply_fixed(&mut k, &mut f, &fixed);
    let modes = rigid_modes(&m.points, &fixed_mask(k.rows, &fixed));
    let mut x = vec![0.0; f.len()];
    let (rep, _) = solve(k, &f, &mut x, 3, modes, 1e-10).unwrap();
    eprintln!("共役勾配法 {} 回", rep.iterations);
    let (stress, _, _) = stresses(&p, nu, &x, |_| e);
    let r_of = |q: [f64; 3]| (q[0] * q[0] + q[1] * q[1]).sqrt();
    let inner = |q: [f64; 3]| (r_of(q) - a).abs() < 5e-3;
    // 剛体の動きを除く: 内の面の節点の変位を、平行移動 (tx, ty)・半径の向きの u・回転 ω の
    // 和 u_i = t + u e_r + ω e_θ に最小二乗で合わせる
    let mut ata = nalgebra::Matrix4::<f64>::zeros();
    let mut atb = nalgebra::Vector4::<f64>::zeros();
    for n in (0..m.points.len()).filter(|n| inner(m.points[*n])) {
        let q = m.points[n];
        let r = r_of(q);
        let (c, s) = (q[0] / r, q[1] / r);
        for (row, rhs) in [
            ([1.0, 0.0, c, -s], x[3 * n]),
            ([0.0, 1.0, s, c], x[3 * n + 1]),
        ] {
            let v = nalgebra::Vector4::from(row);
            ata += v * v.transpose();
            atb += v * rhs;
        }
    }
    let fit = ata.lu().solve(&atb).unwrap();
    let ur = fit[2];
    let want_u =
        (1.0 + nu) * pr * a * a * ((1.0 - 2.0 * nu) * a + b * b / a) / (e * (b * b - a * a));
    near(ur, want_u, 0.01, "厚肉の円筒の内の面の変位(mm)");
    let hoop = |n: usize| {
        let q = m.points[n];
        let r2 = q[0] * q[0] + q[1] * q[1];
        let s = stress[n];
        (s[0] * q[1] * q[1] + s[1] * q[0] * q[0] - 2.0 * s[3] * q[0] * q[1]) / r2
    };
    let want_s = pr * (a * a + b * b) / (b * b - a * a);
    near(
        mean_over(&p, inner, hoop),
        want_s,
        0.01,
        "厚肉の円筒の内の面の周の応力(MPa)",
    );
    // 曲面の上の辺の中点は、両端の角の値を回してから平均し、辺に沿う曲がりを足すので、角より
    // 正解から離れない(そのまま平均すると半径の向きの応力が混ざり、中点だけ 0.6% ほど小さく
    // 出る。曲がりを足さないと角と同じ値で −0.46%、足すと −0.12%)
    let part = |corner: bool| {
        let v: Vec<f64> = (0..m.points.len())
            .filter(|n| (*n < m.corners) == corner && inner(m.points[*n]))
            .map(hoop)
            .collect();
        v.iter().sum::<f64>() / v.len() as f64
    };
    let (hc, hm) = (part(true), part(false));
    assert!(
        (hm - want_s).abs() < (hc - want_s).abs() + 0.001 * want_s
            && (hm - want_s).abs() < 0.005 * want_s,
        "内の面の周の応力: 角 {hc:.4}・辺の中点 {hm:.4}(正解 {want_s:.4})"
    );
    let outer = |q: [f64; 3]| (r_of(q) - b).abs() < 5e-3;
    near(
        mean_over(&p, outer, hoop),
        2.0 * pr * a * a / (b * b - a * a),
        0.03,
        "厚肉の円筒の外の面の周の応力(MPa)",
    );
}

/// 中空の円筒の熱伝導(内 100 ℃・外 0 ℃)。T(r) = 100 ln(b/r) / ln(b/a)。
#[test]
fn a_hollow_cylinder_conducts_with_a_log_profile() {
    let (a, b, l) = (5.0, 10.0, 3.0);
    let p = prepared(&annulus(a, b, l, 120, 2), 0.9);
    let out = solve_thermal(
        &p,
        &ThermalInput {
            k: 50.0,
            temps: vec![(names(&["inner"]), 100.0), (names(&["outer"]), 0.0)],
            heats: vec![],
            convs: vec![],
            rest_h: None,
            ambient: 20.0,
        },
    )
    .unwrap();
    let mut worst: f64 = 0.0;
    for (n, q) in p.mesh.points.iter().enumerate() {
        let r = (q[0] * q[0] + q[1] * q[1]).sqrt();
        let want = 100.0 * (b / r).ln() / (b / a).ln();
        worst = worst.max((out.t[n] - want).abs());
    }
    eprintln!("中空の円筒の温度の最大の差 {worst:.4} ℃");
    assert!(worst < 1.0, "温度の差 {worst} ℃");
    // 流れる熱 Q = 2π k L ΔT / ln(b/a) は、外の面の節点の温度の傾きでなく、内外の温度だけで決まる
}

/// 丸棒(半径 5、長さ 100)の片持ち梁の 1 次の固有振動数。I = π r⁴/4、A = π r²。
#[test]
fn a_round_cantilever_first_frequency_matches_beam_theory() {
    let (r, l) = (5.0, 100.0);
    let p = prepared(&cylinder(r, l, 96), 2.0);
    let (e, rho) = (70_000.0, 2.7);
    let out = solve_modal(
        &p,
        &ModalInput {
            e,
            nu: 0.3,
            density: rho,
            fixed: names(&["bottom"]),
            count: 2,
        },
    )
    .unwrap();
    let i = std::f64::consts::PI * r.powi(4) / 4.0;
    let area = std::f64::consts::PI * r * r;
    let want =
        1.875f64.powi(2) / std::f64::consts::TAU * (e * i / (rho * 1e-9 * area * l.powi(4))).sqrt();
    // 細長さ L/d = 10 なので、せん断と回転の慣性でオイラーの梁より 1% ほど低い
    near(
        out.freqs[0],
        want,
        0.03,
        "丸棒の片持ち梁の 1 次の固有振動数(Hz)",
    );
    near(
        out.freqs[1],
        want,
        0.03,
        "丸棒の片持ち梁の 1 次の固有振動数・横(Hz)",
    );
}

/// 画面に塗る値の誤差の分布: 片持ち梁(ν = 0、Saint-Venant の解)の横の面と真ん中の断面で、
/// 節点の応力の成分を補間してから求めたミーゼス応力(画面の塗り方と同じ)を、場所ごとの理論の値と
/// 比べる。節点の平均だけで比べると、節点の間に出る誤差(格子の模様)を見逃すため。
#[test]
fn displayed_von_mises_matches_saint_venant_between_the_nodes() {
    let p = beam();
    let (l, c, pf) = (100.0, 5.0, 100.0);
    let i = 10.0f64.powi(4) / 12.0;
    let input = StaticInput {
        e: 70_000.0,
        nu: 0.0,
        fixed: names(&["x0"]),
        loads: vec![Load {
            faces: names(&["x1"]),
            kind: LoadKind::Force(pf),
            dir: Dir::Fixed([0.0, 0.0, -1.0]),
        }],
    };
    let out = solve_static(&p, &input).unwrap();
    let exact = |x: [f64; 3]| {
        let z = x[2] - c;
        tet10::von_mises(&[
            pf * (l - x[0]) * z / i,
            0.0,
            0.0,
            0.0,
            0.0,
            pf / (2.0 * i) * (z * z - c * c),
        ])
    };
    let scale = pf * l * c / i;
    let m = &p.mesh;
    // 要素の角の重心座標で補間する(辺の中点は両端の角の平均なので、画面の 8 つに分けた補間と同じ)
    // 返す値は (成分を補間してからミーゼス, 節点のミーゼス応力を補間)
    let shown = |q: [f64; 3]| -> Option<(f64, f64)> {
        for t in &m.tets {
            let x = [0, 1, 2, 3].map(|k| m.points[t[k] as usize]);
            if (0..3).any(|a| {
                q[a] < x.iter().map(|v| v[a]).fold(f64::MAX, f64::min) - 1e-9
                    || q[a] > x.iter().map(|v| v[a]).fold(f64::MIN, f64::max) + 1e-9
            }) {
                continue;
            }
            let d = |a: [f64; 3], b: [f64; 3]| {
                nalgebra::Vector3::new(a[0] - b[0], a[1] - b[1], a[2] - b[2])
            };
            let mm =
                nalgebra::Matrix3::from_columns(&[d(x[1], x[0]), d(x[2], x[0]), d(x[3], x[0])]);
            let r = mm.try_inverse()? * d(q, x[0]);
            let b = [1.0 - r[0] - r[1] - r[2], r[0], r[1], r[2]];
            if b.iter().all(|v| *v >= -1e-9) {
                let s: [f64; 6] = std::array::from_fn(|k| {
                    (0..4)
                        .map(|j| b[j] * out.node_stress[t[j] as usize][k])
                        .sum()
                });
                let vm: f64 = (0..4).map(|j| b[j] * out.node_vm[t[j] as usize]).sum();
                return Some((tet10::von_mises(&s), vm));
            }
        }
        None
    };
    for (what, plane) in [("横の面 y = 0", 0.0), ("断面 y = 5", 5.0)] {
        let mut err = Vec::new();
        let mut old: f64 = 0.0;
        for xi in 0..=56 {
            for zi in 0..=40 {
                let q = [15.0 + xi as f64 * 1.25, plane, zi as f64 * 0.25];
                let (v, w) = shown(q).expect("点が網の中にない");
                err.push((v - exact(q)).abs() / scale);
                old = old.max((w - exact(q)).abs() / scale);
            }
        }
        let rms = (err.iter().map(|e| e * e).sum::<f64>() / err.len() as f64).sqrt();
        let max = err.iter().copied().fold(0.0, f64::max);
        eprintln!(
            "{what}: 塗る値の誤差 RMS {:.3}%・最大 {:.3}%(最大応力に対して)",
            rms * 100.0,
            max * 100.0
        );
        eprintln!("{what}: ミーゼス応力を補間した時の最大 {:.3}%", old * 100.0);
        assert!(rms < 0.005 && max < 0.03, "{what}: RMS {rms} 最大 {max}");
        // この試験は、前の塗り方(節点のミーゼス応力を補間)の誤差を見分けられる
        assert!(old > 3.0 * max, "{what}: {old} / {max}");
    }
}
