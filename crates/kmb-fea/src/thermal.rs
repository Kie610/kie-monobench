//! 定常と時間で変わる熱伝導(温度の分布)。単位は mm・W・℃。熱伝導率は W/(m·K)、熱伝達係数は
//! W/(m²·K) で受け取り、mm に直して使う。

use crate::boundary::{Prepared, face_quadrature};
use crate::solve::{apply_fixed, assemble, solve};
use crate::tet10;

/// 熱の条件。
#[derive(Debug, Clone, PartialEq)]
pub struct ThermalInput {
    /// 熱伝導率 W/(m·K)
    pub k: f64,
    /// 温度を決める面と温度(℃)
    pub temps: Vec<(Vec<String>, f64)>,
    /// 熱が入る面と熱の量(W、面全体の合計)
    pub heats: Vec<(Vec<String>, f64)>,
    /// 空気へ逃げる面と熱伝達係数 W/(m²·K)
    pub convs: Vec<(Vec<String>, f64)>,
    /// 上で選んでいない残りの外の面の熱伝達係数(`None` なら逃げない)
    pub rest_h: Option<f64>,
    /// まわりの空気の温度(℃)
    pub ambient: f64,
}

#[derive(Debug, Clone)]
pub struct ThermalOutput {
    /// 節点ごとの温度(℃)
    pub t: Vec<f64>,
    /// 空気へ逃げた熱の合計(W)
    pub to_air: f64,
    pub iterations: usize,
}

struct System {
    a: crate::sparse::Csr,
    b: Vec<f64>,
    fixed: Vec<(usize, f64)>,
    conv: Vec<(usize, f64)>,
}

fn system(p: &Prepared, input: &ThermalInput, steady: bool) -> Result<System, String> {
    let m = &p.mesh;
    let n = m.points.len();
    let k_mm = input.k / 1000.0;
    let mut a = assemble(m, 1, |e| {
        let mut ke = tet10::conduction(&m.tet_points(e));
        ke.iter_mut().for_each(|v| *v *= k_mm);
        Some(ke)
    });
    let mut b = vec![0.0; n];
    let mut used = vec![false; m.faces.len()];
    for (faces, w) in &input.heats {
        let fs = p.faces_of(faces)?;
        let q = w / p.area(&fs).max(1e-30);
        if steady {
            p.add_flux(&fs, q, &mut b);
        }
        for f in fs {
            used[f] = true;
        }
    }
    let mut fixed = Vec::new();
    for (faces, t) in &input.temps {
        let fs = p.faces_of(faces)?;
        fixed.extend(p.nodes_of(&fs).into_iter().map(|nd| (nd as usize, *t)));
        for f in fs {
            used[f] = true;
        }
    }
    let mut conv: Vec<(usize, f64)> = Vec::new(); // (表面の三角形、熱伝達係数 W/(mm²·K))
    for (faces, hc) in &input.convs {
        for f in p.faces_of(faces)? {
            used[f] = true;
            conv.push((f, hc / 1e6));
        }
    }
    if let Some(hc) = input.rest_h {
        conv.extend(
            (0..m.faces.len())
                .filter(|f| !used[*f])
                .map(|f| (f, hc / 1e6)),
        );
    }
    if steady && fixed.is_empty() && conv.iter().all(|c| c.1 <= 0.0) {
        return Err("温度を決める面か、空気へ逃げる面(熱伝達係数)を付けてください".into());
    }
    // 空気へ逃げる熱: hc ∫ N_i N_j dA を行列へ、hc T∞ ∫ N_i dA を右辺へ
    for &(f, hc) in &conv {
        let nodes = m.faces[f].nodes;
        for (nn, _, w) in face_quadrature(&m.face_points(f)) {
            for i in 0..6 {
                b[nodes[i] as usize] += hc * input.ambient * nn[i] * w;
                for j in 0..6 {
                    a.add(nodes[i] as usize, nodes[j], hc * nn[i] * nn[j] * w);
                }
            }
        }
    }
    Ok(System { a, b, fixed, conv })
}

pub fn solve_thermal(p: &Prepared, input: &ThermalInput) -> Result<ThermalOutput, String> {
    let System {
        mut a,
        mut b,
        fixed,
        conv,
    } = system(p, input, true)?;
    let n = p.mesh.points.len();
    apply_fixed(&mut a, &mut b, &fixed);
    let mut free = vec![1.0; n];
    for (d, _) in &fixed {
        free[*d] = 0.0;
    }
    let mut t = vec![input.ambient; n];
    for (d, v) in &fixed {
        t[*d] = *v;
    }
    let (rep, _) = solve(a, &b, &mut t, 1, vec![free], 1e-9)?;
    let to_air = to_air(p, input.ambient, &conv, &t);
    Ok(ThermalOutput {
        t,
        to_air,
        iterations: rep.iterations,
    })
}

fn to_air(p: &Prepared, ambient: f64, conv: &[(usize, f64)], t: &[f64]) -> f64 {
    conv.iter()
        .map(|&(f, hc)| {
            let nodes = p.mesh.faces[f].nodes;
            face_quadrature(&p.mesh.face_points(f))
                .iter()
                .map(|(nn, _, w)| {
                    let tf: f64 = (0..6).map(|i| nn[i] * t[nodes[i] as usize]).sum();
                    hc * (tf - ambient) * w
                })
                .sum::<f64>()
        })
        .sum()
}

/// 熱の行ごとの時刻。入れ始めを過ぎ、入れ終わりまでの刻みに熱を加える。
#[derive(Debug, Clone, PartialEq)]
pub struct HeatTime {
    pub from: f64,
    pub until: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TransientInput {
    pub thermal: ThermalInput,
    /// 密度 g/cm³
    pub density: f64,
    /// 比熱 J/(kg·K)
    pub cp: f64,
    pub end: f64,
    pub frames: usize,
    pub initial: f64,
    /// thermal.heats と同じ順。
    pub heat_times: Vec<HeatTime>,
}

pub struct ThermalFrame {
    pub time: f64,
    pub t: Vec<f64>,
    pub to_air: f64,
}

pub struct TransientOutput {
    pub frames: Vec<ThermalFrame>,
    pub steps: usize,
    pub min_step: f64,
    pub coarse: bool,
    pub cancelled: bool,
}

pub fn solve_transient(
    p: &Prepared,
    input: &TransientInput,
    cancelled: &dyn Fn() -> bool,
    progress: &dyn Fn(f64),
) -> Result<TransientOutput, String> {
    transient(p, input, cancelled, progress, None)
}

fn transient(
    p: &Prepared,
    input: &TransientInput,
    cancelled: &dyn Fn() -> bool,
    progress: &dyn Fn(f64),
    fixed_step: Option<f64>,
) -> Result<TransientOutput, String> {
    use crate::solve::ReusableSolve;
    if !input.end.is_finite()
        || input.end <= 0.0
        || !(2..=200).contains(&input.frames)
        || !input.initial.is_finite()
        || !input.cp.is_finite()
        || input.cp <= 0.0
        || !input.density.is_finite()
        || input.density <= 0.0
        || !input.thermal.k.is_finite()
        || input.thermal.k <= 0.0
    {
        return Err(
            "終わりの時刻・比熱・密度・熱伝導率は正、コマの数は 2 から 200 にしてください".into(),
        );
    }
    if input.heat_times.len() != input.thermal.heats.len()
        || input.heat_times.iter().any(|h| {
            !h.from.is_finite()
                || !h.until.is_finite()
                || h.from < 0.0
                || h.until <= h.from
                || h.until > input.end
        })
    {
        return Err("熱の時刻は 0 ≦ 入れ始め < 入れ終わり ≦ 終わりの時刻にしてください".into());
    }
    let System {
        a: conduction,
        b,
        fixed,
        conv,
    } = system(p, &input.thermal, false)?;
    // 時刻で変わる熱を右辺と分けて持つ。面の使用済みの印は全ての熱の行を含める。
    let mut fluxes = Vec::new();
    for (faces, watts) in &input.thermal.heats {
        let fs = p.faces_of(faces)?;
        let mut f = vec![0.0; b.len()];
        p.add_flux(&fs, watts / p.area(&fs).max(1e-30), &mut f);
        fluxes.push(f);
    }
    let capacity = input.density * 1e-6 * input.cp;
    let mass = assemble(&p.mesh, 1, |e| {
        let mut m = tet10::mass(&p.mesh.tet_points(e));
        m.iter_mut().for_each(|v| *v *= capacity);
        Some(m)
    });
    let interval = input.end / input.frames as f64;
    let floor =
        (capacity * p.mesh.stats.h.powi(2) / (6.0 * input.thermal.k / 1000.0)).min(interval);
    if !floor.is_finite() || floor <= 0.0 || interval <= 0.0 {
        return Err("時刻の刻みを表せません。終わりの時刻と要素の大きさを見直してください".into());
    }
    let mut dt = fixed_step.unwrap_or(interval);
    let mut t = vec![input.initial; b.len()];
    for &(d, v) in &fixed {
        t[d] = v;
    }
    let mut out = TransientOutput {
        frames: vec![ThermalFrame {
            time: 0.0,
            to_air: to_air(p, input.thermal.ambient, &conv, &t),
            t: t.clone(),
        }],
        steps: 0,
        min_step: f64::INFINITY,
        coarse: false,
        cancelled: false,
    };
    let mut time = 0.0;
    let mut low = t.iter().copied().fold(f64::INFINITY, f64::min);
    let mut high = t.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let mut cache: Option<(f64, ReusableSolve)> = None;
    for frame in 1..=input.frames {
        let target = input.end * (frame as f64 / input.frames as f64);
        while time < target {
            if cancelled() {
                out.cancelled = true;
                return Ok(out);
            }
            let stop = input
                .heat_times
                .iter()
                .flat_map(|h| [h.from, h.until])
                .filter(|v| *v > time)
                .fold(target, f64::min);
            let remaining = stop - time;
            let step = if dt >= remaining * (1.0 - 1e-12) {
                remaining
            } else {
                dt
            };
            let next = if step == stop - time {
                stop
            } else {
                time + step
            };
            if next <= time {
                return Err("時刻の刻みが小さすぎます".into());
            }
            // コマ・加熱の切り替えで縮めた最後の刻みだけは下限より短くできる。
            if cache
                .as_ref()
                .is_none_or(|(d, _)| (*d - step).abs() > step * 1e-12)
            {
                let mut a = conduction.clone();
                for (a, m) in a.val.iter_mut().zip(&mass.val) {
                    *a += m / step;
                }
                cache = Some((step, ReusableSolve::new(a, &fixed)?));
            }
            let mut rhs = vec![0.0; t.len()];
            mass.mul(&t, &mut rhs);
            for (rhs, b) in rhs.iter_mut().zip(&b) {
                *rhs = *rhs / step + b;
            }
            for (heat, f) in input.heat_times.iter().zip(&fluxes) {
                if next > heat.from && next <= heat.until {
                    for (rhs, f) in rhs.iter_mut().zip(f) {
                        *rhs += f;
                    }
                }
            }
            let mut trial = t.clone();
            cache.as_ref().unwrap().1.solve(&rhs, &mut trial)?;
            let change = trial
                .iter()
                .zip(&t)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0, f64::max);
            let goal = 0.5_f64.max((high - low) * 0.02);
            if fixed_step.is_none() && change > 2.0 * goal && step > floor * (1.0 + 1e-12) {
                dt = (step / 2.0).max(floor);
                continue;
            }
            if fixed_step.is_none() && change > goal && step <= floor * (1.0 + 1e-12) {
                out.coarse = true;
            }
            t = trial;
            time = next;
            out.steps += 1;
            out.min_step = out.min_step.min(step);
            for &v in &t {
                low = low.min(v);
                high = high.max(v);
            }
            progress(time / input.end);
            if fixed_step.is_none() && change < 0.5 * goal {
                dt = (dt * 2.0).min(interval);
            }
        }
        out.frames.push(ThermalFrame {
            time: target,
            to_air: to_air(p, input.thermal.ambient, &conv, &t),
            t: t.clone(),
        });
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) fn solve_transient_fixed(
    p: &Prepared,
    input: &TransientInput,
    dt: f64,
) -> TransientOutput {
    transient(p, input, &|| false, &|_| {}, Some(dt)).unwrap()
}
