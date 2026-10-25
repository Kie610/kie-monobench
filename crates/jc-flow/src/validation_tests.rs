//! 解き方の検算: 解析解や標準の問題と比べる。

use crate::lbm::{D2Q9, D3Q19, Setup, Solver, cell};

/// 平行な板の間の流れ(ポアズイユ): 体積力で押すと速さが放物線になる。
#[test]
fn channel_flow_is_a_parabola() {
    let h = 20;
    let mut s = Setup::new(&D2Q9, [4, h + 2, 1]);
    s.periodic = [true, false, false];
    s.nu = 0.1;
    s.force = [1e-5, 0.0, 0.0];
    for i in 0..4 {
        let a = s.index(i, 0, 0);
        let b = s.index(i, h + 1, 0);
        s.cell[a] = cell::WALL;
        s.cell[b] = cell::WALL;
    }
    let mut sv = Solver::new(s);
    sv.run(20_000);
    let mut worst: f64 = 0.0;
    let umax = 1e-5 / (2.0 * 0.1) * (h as f64 / 2.0).powi(2);
    for j in 1..=h {
        let y = j as f64 - 0.5;
        let want = 1e-5 / (2.0 * 0.1) * y * (h as f64 - y);
        let (_, u) = sv.macro_at(sv.setup.index(1, j, 0));
        worst = worst.max((u[0] as f64 - want).abs() / umax);
    }
    assert!(worst < 0.01, "放物線からのずれ {worst}");
}

/// 四角い管の流れ(3D): 中心の速さを級数の解と比べる。
#[test]
fn square_duct_flow_matches_the_series_solution() {
    let a = 12;
    let mut s = Setup::new(&D3Q19, [2, a + 2, a + 2]);
    s.periodic = [true, false, false];
    s.nu = 0.1;
    s.force = [1e-5, 0.0, 0.0];
    for k in 0..a + 2 {
        for j in 0..a + 2 {
            if j == 0 || k == 0 || j == a + 1 || k == a + 1 {
                for i in 0..2 {
                    let x = s.index(i, j, k);
                    s.cell[x] = cell::WALL;
                }
            }
        }
    }
    let mut sv = Solver::new(s);
    sv.run(8_000);
    // 一辺 a の正方形の管の中心: u = 4 a² g / π³ Σ (-1)^((n-1)/2) / n³ (1 - 1/cosh(nπ/2))(n は奇数、g = 力 / ν)
    let g = 1e-5 / 0.1;
    let af = a as f64;
    let mut sum = 0.0;
    for n in (1..40).step_by(2) {
        let nf = n as f64;
        let sign = if (n / 2) % 2 == 0 { 1.0 } else { -1.0 };
        sum += sign / nf.powi(3) * (1.0 - 1.0 / (nf * std::f64::consts::PI / 2.0).cosh());
    }
    let want = 4.0 * g * af * af / std::f64::consts::PI.powi(3) * sum;
    // 中心は 4 つのセルの間なので、その平均
    let mut u = 0.0;
    for (dj, dk) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
        u += sv.macro_at(sv.setup.index(1, a / 2 + dj, a / 2 + dk)).1[0] as f64;
    }
    u /= 4.0;
    assert!((u - want).abs() / want < 0.03, "中心の速さ {u} と {want}");
}

/// Schäfer–Turek(1996)の 2D-1: 管の中の円柱、Re 20。抗力係数は 5.57〜5.59。
/// `d` は直径のセルの数、`umax` は入口の最大の速さ(格子の単位)、`bouzidi` は壁までの距離を使うか。
/// 直径 10 セルで 5.66(1.5% 高い)。速さ 0.1 では 5.82(圧縮性の誤差)、距離を使わないと 6.02、
/// BGK では 5.79(直径を 30 セルにしても 5.80 で、壁の位置が粘性で動く誤差が残る)。
fn cylinder_2d(d: usize, umax: f32, bouzidi: bool) -> (f64, f64, usize) {
    let mut sv = Solver::new(cylinder_setup(d, umax, bouzidi, 20.0));
    let umean = 2.0 / 3.0 * umax as f64;
    let mut last = f64::NAN;
    let mut steps = 0;
    for _ in 0..200 {
        sv.run(500);
        steps += 500;
        let fx = sv.mean_force(500)[0];
        if (fx - last).abs() < 3e-4 * fx.abs() {
            break;
        }
        last = fx;
    }
    let f = sv.mean_force(500);
    let k = 2.0 / (umean * umean * d as f64);
    (f[0] * k, f[1] * k, steps)
}

/// Schäfer–Turek の管の中の円柱(レイノルズ数 `re`、平均の速さと直径で)。
fn cylinder_setup(d: usize, umax: f32, bouzidi: bool, re: f64) -> Setup {
    let dx = 0.1 / d as f64;
    let nh = (0.41 / dx).round() as usize;
    let nl = (2.2 / dx).round() as usize;
    let mut s = Setup::new(&D2Q9, [nl, nh + 2, 1]);
    let umean = 2.0 / 3.0 * umax as f64;
    s.nu = (umean * d as f64 / re) as f32;
    let (xc, yc, r) = (0.2 / dx, 0.5 + 0.2 / dx, d as f64 / 2.0);
    let q = s.v.c.len();
    s.u_cell = vec![[0.0; 3]; s.count()];
    for j in 0..nh + 2 {
        for i in 0..nl {
            let x = s.index(i, j, 0);
            let (fx, fy) = (i as f64, j as f64);
            if j == 0 || j == nh + 1 {
                s.cell[x] = cell::WALL;
            } else if (fx - xc).powi(2) + (fy - yc).powi(2) < r * r {
                s.cell[x] = cell::BODY;
            } else if i == 0 {
                s.cell[x] = cell::INLET;
                let y = (j as f64 - 0.5) / nh as f64;
                s.u_cell[x] = [(4.0 * umax as f64 * y * (1.0 - y)) as f32, 0.0, 0.0];
            } else if i == nl - 1 {
                s.cell[x] = cell::OUTLET;
            }
        }
    }
    // 円までの距離(セルの中心から格子の線に沿って)
    s.q = vec![f32::NAN; s.count() * q];
    for j in 1..=nh {
        for i in 0..nl {
            let x = s.index(i, j, 0);
            if s.cell[x] != cell::FLUID {
                continue;
            }
            for k in 1..q {
                let c = s.v.c[k];
                let (ni, nj) = (i as i64 + c[0] as i64, j as i64 + c[1] as i64);
                let y = s.index(ni as usize, nj as usize, 0);
                if s.cell[y] != cell::BODY {
                    continue;
                }
                let (px, py) = (i as f64 - xc, j as f64 - yc);
                let (cx, cy) = (c[0] as f64, c[1] as f64);
                let aa = cx * cx + cy * cy;
                let bb = 2.0 * (px * cx + py * cy);
                let cc = px * px + py * py - r * r;
                let t = (-bb - (bb * bb - 4.0 * aa * cc).sqrt()) / (2.0 * aa);
                if bouzidi {
                    s.q[x * q + k] = t.clamp(0.0, 1.0) as f32;
                }
            }
        }
    }
    s
}

#[test]
fn cylinder_in_a_channel_matches_schafer_turek() {
    let t = std::time::Instant::now();
    let (cd, cl, steps) = cylinder_2d(10, 0.05, true);
    eprintln!("Cd {cd} Cl {cl} 刻み {steps} {:?}", t.elapsed());
    assert!((cd - 5.58).abs() / 5.58 < 0.03, "抗力係数 {cd}");
    assert!(cl.abs() < 0.03, "揚力係数 {cl}");
}

#[test]
fn vortex_shedding_matches_schafer_turek_2d_2() {
    // 2D-2: Re 100 で渦が周期的に出る。ストローハル数(平均の速さと直径で)は 0.295〜0.305。
    // 出口の前に研究と同じ吸い込みの帯を入れる。直径 12 セルで τ 0.518 になり、差の分の緩和時間に
    // 上限がないと 1000 刻みまでに発散する
    let (d, umax) = (12, 0.075f32);
    let mut s = cylinder_setup(d, umax, true, 100.0);
    let n0 = s.n[0];
    let band = n0 / 8;
    s.sponge = (0..n0)
        .map(|i| {
            let t = (i as f64 - (n0 - band) as f64).max(0.0) / band as f64;
            (0.1 * t * t) as f32
        })
        .collect();
    let smooth = (2.0 * s.n[1] as f64 * 3f64.sqrt()).ceil() as usize;
    let mut sv = Solver::new(s);
    for b in 0..17 {
        sv.run(1000);
        assert!(!sv.blew_up(), "発散 {} 刻み", (b + 1) * 1000);
    }
    let umean = 2.0 / 3.0 * umax as f64;
    let tail = &sv.forces[9_000..];
    let drag = tail.iter().map(|f| f[0]).sum::<f64>() / tail.len() as f64;
    let period = shedding_period(tail, drag, smooth).expect("渦が出る");
    let st = d as f64 / (period * umean);
    eprintln!("St {st}(周期 {period} 刻み)");
    assert!((st - 0.30).abs() < 0.015, "ストローハル数 {st}");
}

#[test]
fn a_square_in_air_at_10_m_s_does_not_blow_up() {
    // 研究の既定(空気 10 m/s)で 20 mm の角柱の 2D の流れ。Re 約 1.3 万は格子で表せず、粘性の
    // 下限で解く。格子は 1 mm(幅 20 セル)。セルの大きさで見た流れ(格子の速さと粘性の下限)は
    // 既定の格子と同じで、セルの数が 1/4・刻みが 1/2 になる
    use crate::flow::{AIR, FlowSpec, NoWatch, run};
    use crate::geom::{Frame, Shape, tests::box_mesh};
    let m = box_mesh([0.0, 0.0, 0.0], [20.0, 20.0, 30.0]);
    let s = Shape::new(&[&m], &Frame::new(0, 1.0, 2)).unwrap();
    let spec = FlowSpec {
        dims: 2,
        plane_z: 0.0,
        h: Some(1.0),
        speed: 10.0,
        fluid: AIR,
        frames: 1,
        end: None,
    };
    let o = run(&s, &spec, &mut NoWatch).expect("発散しない");
    // 角柱の抗力係数は高い Re で 2.0〜2.2 ほど(Re を下げて解くので幅を取る)
    assert!((1.6..3.0).contains(&o.coef[0]), "抗力係数 {}", o.coef[0]);
}

/// 横の力(1 刻みごと)の揺れの周期(刻み)。`smooth` 刻みの移動平均で細かい揺れを除き、上下の
/// 閾値を越えた時だけ向きが変わったと数える(シュミットトリガ)。揺れが小さいか、2 周期に満たな
/// ければ None。
fn shedding_period(forces: &[[f64; 3]], drag: f64, smooth: usize) -> Option<f64> {
    let k = smooth.max(1);
    if forces.len() < 2 * k {
        return None;
    }
    let mut sum = 0.0;
    let mut ys = Vec::with_capacity(forces.len() - k + 1);
    for (i, f) in forces.iter().enumerate() {
        sum += f[1];
        if i >= k {
            sum -= forces[i - k][1];
        }
        if i + 1 >= k {
            ys.push(sum / k as f64);
        }
    }
    let n = ys.len() as f64;
    let mean = ys.iter().sum::<f64>() / n;
    let rms = (ys.iter().map(|y| (y - mean).powi(2)).sum::<f64>() / n).sqrt();
    if rms < 0.01 * drag {
        return None;
    }
    let th = 0.5 * rms;
    let mut state = 0i8;
    let mut at: Vec<usize> = Vec::new();
    for (i, y) in ys.iter().enumerate() {
        let d = y - mean;
        if d > th && state <= 0 {
            if state < 0 {
                at.push(i);
            }
            state = 1;
        } else if d < -th && state >= 0 {
            if state > 0 {
                at.push(i);
            }
            state = -1;
        }
    }
    // 半周期ごとの切り替わりが 4 回(2 周期)以上
    (at.len() >= 4).then(|| 2.0 * (at[at.len() - 1] - at[0]) as f64 / (at.len() - 1) as f64)
}

#[test]
fn the_shedding_period_ignores_the_fast_pressure_waves() {
    // 周期 1000 刻みの揺れに、周期 400 刻みの大きな揺れが乗った力
    let f: Vec<[f64; 3]> = (0..6000)
        .map(|i| {
            let t = i as f64 * std::f64::consts::TAU;
            [1.0, 0.2 * (t / 1000.0).sin() + 0.5 * (t / 400.0).sin(), 0.0]
        })
        .collect();
    let p = shedding_period(&f, 1.0, 400).unwrap();
    assert!((p - 1000.0).abs() < 30.0, "周期 {p}");
    // 2 周期に満たなければ出さない
    assert!(shedding_period(&f[..1500], 1.0, 400).is_none());
}
