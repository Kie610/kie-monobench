//! 境界要素法を球の解析解と比べる。網は半径 0.0875 m(頭ほどの大きさ)の 20 面体を 3 回分けた球
//! (三角形 1280)。比べるのは三角形の重心の向きでの音圧の大きさ(dB)。

use kmb_wave::exact::{pulsating_sphere, sphere_plane_wave, sphere_plane_wave_far};
use kmb_wave::{
    BemJob, Boundary, Complex64 as C, Medium, Receivers, Solver, Source, WaveErrorKind, icosphere,
    solve_bem,
};

const A: f64 = 0.0875;

fn freq(ka: f64) -> f64 {
    ka / A * Medium::default().speed_of_sound / (2.0 * std::f64::consts::PI)
}

fn job(subdivisions: u32, kas: &[f64]) -> BemJob {
    BemJob {
        mesh: icosphere(A, subdivisions),
        boundaries: vec![],
        sources: vec![],
        frequencies: kas.iter().map(|&ka| freq(ka)).collect(),
        receivers: Receivers::None,
        field_points: Vec::new(),
        medium: Medium::default(),
        solver: Solver::default(),
    }
}

fn plane_wave() -> Source {
    Source::PlaneWave {
        direction: [0.0, 0.0, 1.0],
        amplitude: C::new(1.0, 0.0),
    }
}

/// 三角形の重心の向き(+z からの角度の余弦)。
fn cosines(job: &BemJob) -> Vec<f64> {
    job.mesh
        .triangles
        .iter()
        .map(|t| {
            let c = t.iter().fold([0.0; 3], |s, &i| {
                let v = job.mesh.vertices[i as usize];
                [s[0] + v[0], s[1] + v[1], s[2] + v[2]]
            });
            c[2] / (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt()
        })
        .collect()
}

/// 大きさの差(dB)の一番大きい所。
fn worst_db(got: &[C], want: impl Fn(usize) -> C) -> f64 {
    got.iter()
        .enumerate()
        .map(|(i, g)| (20.0 * (g.norm() / want(i).norm()).log10()).abs())
        .fold(0.0, f64::max)
}

/// 確かめること: 剛い球の平面波の散乱で、表面の音圧の大きさが級数解と ka = 0.5・1・3 で 1 dB 以内、
/// 遠方の振幅も 1 dB 以内に合う。
#[test]
fn rigid_sphere_scattering_matches_the_series() {
    let mut j = job(3, &[0.5, 1.0, 3.0]);
    j.sources = vec![plane_wave()];
    let dirs: Vec<[f64; 3]> = (0..7)
        .map(|i| {
            let t = std::f64::consts::PI * i as f64 / 6.0;
            [t.sin(), 0.0, t.cos()]
        })
        .collect();
    j.receivers = Receivers::Directions {
        directions: dirs.clone(),
        distance: None,
    };
    let mu = cosines(&j);
    let out = solve_bem(&j, &()).unwrap();
    for (fo, ka) in out.frequencies.iter().zip([0.5, 1.0, 3.0]) {
        let k = ka / A;
        let db = worst_db(&fo.surface, |i| {
            sphere_plane_wave(k, A, A, mu[i], C::new(0.0, 0.0))
        });
        assert!(db < 1.0, "ka {ka}: 表面の差 {db:.3} dB");
        let far = worst_db(&fo.receivers, |i| {
            sphere_plane_wave_far(k, A, dirs[i][2], C::new(0.0, 0.0))
        });
        assert!(far < 1.0, "ka {ka}: 遠方の差 {far:.3} dB");
        assert!(fo.residual <= 1e-4);
    }
}

/// 確かめること: 音を吸う球(比音響アドミタンス 1)の散乱も級数解と 1 dB 以内に合う。
#[test]
fn absorbing_sphere_scattering_matches_the_series() {
    let mut j = job(3, &[1.0]);
    j.sources = vec![plane_wave()];
    let beta = C::new(1.0, 0.0);
    j.boundaries = vec![Boundary::Admittance { admittance: beta }];
    let mu = cosines(&j);
    let out = solve_bem(&j, &()).unwrap();
    let k = 1.0 / A;
    let db = worst_db(&out.frequencies[0].surface, |i| {
        sphere_plane_wave(k, A, A, mu[i], beta)
    });
    assert!(db < 1.0, "表面の差 {db:.3} dB");
}

/// 確かめること: 脈動する球の放射で、表面と離れた点(2a・4a)の音圧が単極の解析解と 0.5 dB 以内に合う。
#[test]
fn pulsating_sphere_matches_the_monopole() {
    let mut j = job(3, &[1.0]);
    let v = C::new(1e-3, 0.0);
    j.boundaries = vec![Boundary::Velocity { velocity: v }];
    let pts = [
        [2.0 * A, 0.0, 0.0],
        [0.0, 0.0, -4.0 * A],
        [0.0, 3.0 * A, 0.0],
    ];
    j.receivers = Receivers::Points {
        points: pts.to_vec(),
    };
    let m = Medium::default();
    let out = solve_bem(&j, &()).unwrap();
    let k = 1.0 / A;
    let exact = |r: f64| pulsating_sphere(k, A, r, m.density, m.speed_of_sound, v);
    let fo = &out.frequencies[0];
    let db = worst_db(&fo.surface, |_| exact(A));
    assert!(db < 0.5, "表面の差 {db:.3} dB");
    let db = worst_db(&fo.receivers, |i| {
        let p = pts[i];
        exact((p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt())
    });
    assert!(db < 0.5, "離れた点の差 {db:.3} dB");
}

/// 確かめること: 球の内部の第 1 の共振(ka ≈ π)で、結合なしの式は解が乱れ、Burton–Miller の結合では
/// 解析解に 1 dB 以内で合う。
#[test]
fn burton_miller_removes_the_interior_resonance() {
    let v = C::new(1e-3, 0.0);
    let m = Medium::default();
    let mut worst_plain: f64 = 0.0;
    let mut worst_coupled: f64 = 0.0;
    // 内接する多面体の分だけ共振は π から少しずれるので、近くの数点を見る
    let kas = [3.10, 3.13, 3.16, 3.19];
    for coupled in [false, true] {
        let mut j = job(2, &kas);
        j.boundaries = vec![Boundary::Velocity { velocity: v }];
        j.solver.burton_miller = coupled;
        j.solver.max_iterations = 3000;
        let out = solve_bem(&j, &()).unwrap();
        for (fo, ka) in out.frequencies.iter().zip(kas) {
            let k = ka / A;
            let db = worst_db(&fo.surface, |_| {
                pulsating_sphere(k, A, A, m.density, m.speed_of_sound, v)
            });
            if coupled {
                worst_coupled = worst_coupled.max(db);
            } else {
                worst_plain = worst_plain.max(db);
            }
        }
    }
    assert!(worst_coupled < 1.0, "結合ありの差 {worst_coupled:.3} dB");
    assert!(
        worst_plain > 3.0,
        "結合なしでも乱れなかった({worst_plain:.3} dB)"
    );
}

/// 確かめること: 仕事を JSON に書いて読むと同じになる(複素数・境界の種類・受け取りを含む)。
/// 数の読み取りは最後の桁が 1 つずれることがあるので、数は相対 1e−15 で比べる。
#[test]
fn job_round_trips_through_json() {
    let mut j = job(1, &[100.0, 2000.0]);
    j.mesh.groups = vec![0; j.mesh.triangles.len()];
    j.mesh.groups[3] = 1;
    j.boundaries = vec![
        Boundary::Rigid,
        Boundary::Velocity {
            velocity: C::new(0.001, -0.002),
        },
    ];
    j.sources = vec![
        plane_wave(),
        Source::Point {
            position: [1.0, 2.0, 3.0],
            amplitude: C::new(0.5, 0.25),
        },
    ];
    j.receivers = Receivers::Directions {
        directions: vec![[1.0, 0.0, 0.0]],
        distance: Some(2.0),
    };
    j.medium = Medium::from_temperature(25.0);
    let text = serde_json::to_string(&j).unwrap();
    let back: BemJob = serde_json::from_str(&text).unwrap();
    let again = serde_json::to_value(&back).unwrap();
    same(&serde_json::to_value(&j).unwrap(), &again);
    assert_eq!(back.boundaries, j.boundaries);
    assert_eq!(back.sources, j.sources);
    assert_eq!(back.mesh.triangles, j.mesh.triangles);
    // 省いた欄は既定で読める
    let short = r#"{"mesh":{"vertices":[],"triangles":[]},"frequencies":[100]}"#;
    let s: BemJob = serde_json::from_str(short).unwrap();
    assert_eq!(s.solver, Solver::default());
    assert_eq!(s.receivers, Receivers::None);
}

/// 確かめること: 誤りは種類つきで、解き始める前に止まる(負の周波数は入力、メモリの上限超えは使えない)。
/// 中止されたら、解き終えた周波数までを返す。
#[test]
fn errors_are_typed_and_cancel_stops() {
    let mut j = job(1, &[1.0]);
    j.frequencies = vec![-5.0];
    assert_eq!(solve_bem(&j, &()).unwrap_err().kind, WaveErrorKind::Input);
    let mut j = job(1, &[1.0]);
    j.solver.max_matrix_bytes = 1000;
    let e = solve_bem(&j, &()).unwrap_err();
    assert_eq!(e.kind, WaveErrorKind::Unavailable);
    assert!(e.message.contains("粗く"), "{}", e.message);

    struct Stop;
    impl kmb_wave::Watch for Stop {
        fn cancelled(&self) -> bool {
            true
        }
    }
    let out = solve_bem(&job(1, &[1.0, 2.0]), &Stop).unwrap();
    assert!(out.cancelled);
    assert!(out.frequencies.is_empty());
}

fn same(a: &serde_json::Value, b: &serde_json::Value) {
    use serde_json::Value as V;
    match (a, b) {
        (V::Number(x), V::Number(y)) => {
            let (x, y) = (x.as_f64().unwrap(), y.as_f64().unwrap());
            assert!((x - y).abs() <= 1e-15 * x.abs().max(y.abs()), "{x} {y}");
        }
        (V::Array(x), V::Array(y)) => {
            assert_eq!(x.len(), y.len());
            x.iter().zip(y).for_each(|(x, y)| same(x, y));
        }
        (V::Object(x), V::Object(y)) => {
            assert_eq!(x.keys().collect::<Vec<_>>(), y.keys().collect::<Vec<_>>());
            x.iter().for_each(|(k, v)| same(v, &y[k]));
        }
        _ => assert_eq!(a, b),
    }
}
