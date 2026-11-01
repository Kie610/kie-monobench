//! 外の空間の境界要素法(区分一定の三角形、重心での選点、Burton–Miller の結合)。
//!
//! 表面の音圧 p について、通常の積分方程式(CBIE)と法線で微分した式(HBIE)を α = −j/k で
//! 結ぶ(e^{jωt} の向きでの標準の選び方。虚部が 0 でなければ内部の共振で解が乱れない):
//!
//! - CBIE: ½p(x) − ∫p ∂G/∂n_y dS + ∫q G dS = p_in(x)
//! - HBIE: ½q(x) − ∫p ∂²G/∂n_x∂n_y dS + ∫q ∂G/∂n_x dS = ∂p_in/∂n(x)
//!
//! q = ∂p/∂n は境界から決まる: 振動する面は q = −jωρv、音を吸う面は q = jkβp。
//! 行列は密で、複素の f32 で持ち、足し合わせは f64。反復は対角で前処理した GMRES。

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use num_complex::{Complex32, Complex64 as C};

use crate::gmres;
use crate::job::{BemJob, BemOut, Boundary, FrequencyOut, Receivers, Source, WaveError};
use crate::mesh::{Elem, dot, norm, scale, sub, unit};
use crate::quad;

/// 解いている時の様子を伝え、中止を受ける相手。計算の糸から呼ぶので `Sync`。
pub trait Watch: Sync {
    /// 中止されたか(されたら、解き終えた周波数までを返す)
    fn cancelled(&self) -> bool {
        false
    }
    /// 全体の進み(0〜1)と今の様子
    fn progress(&self, _fraction: f64, _note: &str) {}
}

/// 様子を見ない時。
impl Watch for () {}

/// 仕事を解く。周波数ごとに表面の音圧と受け取る所の値を返す。
pub fn solve_bem(job: &BemJob, watch: &dyn Watch) -> Result<BemOut, WaveError> {
    let elems = job.mesh.elements()?;
    check(job, elems.len())?;
    let walls = walls(job, elems.len());
    let n = elems.len();
    log::info!(
        "境界要素法: 要素 {n}、周波数 {}、受け取り {}",
        job.frequencies.len(),
        receiver_count(&job.receivers)
    );
    let mut out = BemOut::default();
    let nf = job.frequencies.len();
    for (fi, &f) in job.frequencies.iter().enumerate() {
        if watch.cancelled() {
            out.cancelled = true;
            break;
        }
        let start = Instant::now();
        let c = job.medium.speed_of_sound;
        let rho = job.medium.density;
        let omega = 2.0 * std::f64::consts::PI * f;
        let k = omega / c;
        let alpha = if job.solver.burton_miller {
            C::new(0.0, -1.0 / k)
        } else {
            C::new(0.0, 0.0)
        };
        // 三角形ごとの q = a + b·p
        let ab: Vec<(C, C)> = walls
            .iter()
            .map(|w| match w {
                Boundary::Rigid => (C::new(0.0, 0.0), C::new(0.0, 0.0)),
                Boundary::Velocity { velocity } => {
                    (C::new(0.0, -omega * rho) * velocity, C::new(0.0, 0.0))
                }
                Boundary::Admittance { admittance } => {
                    (C::new(0.0, 0.0), C::new(0.0, k) * admittance)
                }
            })
            .collect();
        let note = format!("{f:.0} Hz");
        watch.progress(fi as f64 / nf as f64, &format!("{note}: 行列を組み立て中"));
        let Some((a, rhs)) = assemble(&elems, &ab, k, alpha, &job.sources, watch) else {
            out.cancelled = true;
            break;
        };
        watch.progress(
            (fi as f64 + 0.5) / nf as f64,
            &format!("{note}: 反復で解いています"),
        );
        let diag_inv: Vec<C> = (0..n)
            .map(|i| {
                let d = C::new(a[i * n + i].re as f64, a[i * n + i].im as f64);
                if d.norm() > 0.0 {
                    1.0 / d
                } else {
                    C::new(1.0, 0.0)
                }
            })
            .collect();
        let mv = |v: &[C], o: &mut [C]| matvec(&a, n, v, o);
        let stop = || watch.cancelled();
        let sol = gmres::solve(
            &rhs,
            &diag_inv,
            &mv,
            job.solver.restart,
            job.solver.max_iterations,
            job.solver.tolerance,
            &stop,
        );
        if sol.cancelled {
            out.cancelled = true;
            break;
        }
        if sol.residual.is_nan() || sol.residual > job.solver.tolerance {
            return Err(WaveError::compute(format!(
                "{note} で反復が収まりませんでした(残り {:.1e}、反復 {} 回)",
                sol.residual, sol.iterations
            )));
        }
        let p = sol.x;
        let q: Vec<C> = p.iter().zip(&ab).map(|(p, (a, b))| a + b * p).collect();
        let receivers = receive(&job.receivers, &elems, &p, &q, k, &job.sources);
        let size = elems.iter().map(|e| e.size).fold(0.0, f64::max);
        let fo = FrequencyOut {
            frequency: f,
            surface: p,
            receivers,
            iterations: sol.iterations,
            residual: sol.residual,
            elements_per_wavelength: c / f / size,
            seconds: start.elapsed().as_secs_f64(),
        };
        log::info!(
            "境界要素法: {note} 反復 {} 回、残り {:.1e}、{:.2} s",
            fo.iterations,
            fo.residual,
            fo.seconds
        );
        out.frequencies.push(fo);
    }
    watch.progress(1.0, "終わり");
    Ok(out)
}

/// 仕事の中身を確かめる(網は [`crate::mesh::BemMesh::elements`] が確かめる)。
fn check(job: &BemJob, n: usize) -> Result<(), WaveError> {
    if job.frequencies.is_empty() {
        return Err(WaveError::input("周波数がありません"));
    }
    if let Some(f) = job
        .frequencies
        .iter()
        .find(|f| !(f.is_finite() && **f > 0.0))
    {
        return Err(WaveError::input(format!(
            "周波数 {f} は正の数ではありません"
        )));
    }
    let m = &job.medium;
    if !(m.speed_of_sound > 0.0
        && m.density > 0.0
        && m.speed_of_sound.is_finite()
        && m.density.is_finite())
    {
        return Err(WaveError::input("音速と密度は正の数にしてください"));
    }
    for s in &job.sources {
        match s {
            Source::PlaneWave { direction, .. } if !positive(norm(*direction)) => {
                return Err(WaveError::input("平面波の向きが 0 のベクトルです"));
            }
            _ => {}
        }
    }
    match &job.receivers {
        Receivers::Directions {
            directions,
            distance,
        } => {
            if directions.iter().any(|d| !positive(norm(*d))) {
                return Err(WaveError::input("受け取りの方向に 0 のベクトルがあります"));
            }
            if distance.is_some_and(|r| !(r > 0.0 && r.is_finite())) {
                return Err(WaveError::input("受け取りの距離は正の数にしてください"));
            }
        }
        Receivers::Points { .. } | Receivers::None => {}
    }
    let s = &job.solver;
    if !positive(s.tolerance) || s.restart == 0 || s.max_iterations == 0 {
        return Err(WaveError::input(
            "反復の設定(残り・再始動・上限)は正の数にしてください",
        ));
    }
    // 行列は複素の f32(8 バイト)で n×n
    let bytes = (n as u64) * (n as u64) * 8;
    if bytes > s.max_matrix_bytes {
        let fit = ((s.max_matrix_bytes / 8) as f64).sqrt().floor() as u64;
        return Err(WaveError::unavailable(format!(
            "要素 {n} 個の行列は {:.2} GB で、上限 {:.2} GB を超えます。網を要素 {fit} 個以下に粗くしてください",
            bytes as f64 / 1e9,
            s.max_matrix_bytes as f64 / 1e9
        )));
    }
    Ok(())
}

/// 有限の正の数か。
fn positive(x: f64) -> bool {
    x.is_finite() && x > 0.0
}

/// 三角形ごとの境界の種類(組の番号で引く。無ければ剛い面)。
fn walls(job: &BemJob, n: usize) -> Vec<Boundary> {
    (0..n)
        .map(|t| {
            let g = job.mesh.groups.get(t).copied().unwrap_or(0) as usize;
            job.boundaries.get(g).copied().unwrap_or(Boundary::Rigid)
        })
        .collect()
}

fn receiver_count(r: &Receivers) -> usize {
    match r {
        Receivers::None => 0,
        Receivers::Points { points } => points.len(),
        Receivers::Directions { directions, .. } => directions.len(),
    }
}

/// 使う糸の数。
fn threads(work: usize) -> usize {
    let t = std::thread::available_parallelism().map_or(1, |n| n.get());
    t.min(work.div_ceil(16)).max(1)
}

/// 行列と右辺を組み立てる。中止されたら None。
fn assemble(
    elems: &[Elem],
    ab: &[(C, C)],
    k: f64,
    alpha: C,
    sources: &[Source],
    watch: &dyn Watch,
) -> Option<(Vec<Complex32>, Vec<C>)> {
    let n = elems.len();
    let mut a = vec![Complex32::new(0.0, 0.0); n * n];
    let mut rhs = vec![C::new(0.0, 0.0); n];
    let stopped = AtomicBool::new(false);
    let rows = n.div_ceil(threads(n));
    std::thread::scope(|s| {
        for (chunk, (arows, brows)) in a.chunks_mut(rows * n).zip(rhs.chunks_mut(rows)).enumerate()
        {
            let stopped = &stopped;
            s.spawn(move || {
                for (r, (arow, b)) in arows.chunks_mut(n).zip(brows.iter_mut()).enumerate() {
                    if stopped.load(Ordering::Relaxed) || watch.cancelled() {
                        stopped.store(true, Ordering::Relaxed);
                        return;
                    }
                    let i = chunk * rows + r;
                    let ei = &elems[i];
                    let (x, nx) = (ei.c, ei.n);
                    let (pin, dpin) = incident(sources, x, nx, k);
                    let mut bi = pin + alpha * dpin;
                    for (j, (ej, aij)) in elems.iter().zip(arow.iter_mut()).enumerate() {
                        let kn = quad::element(x, nx, ej, k, i == j);
                        let (aj, bj) = ab[j];
                        let ql = kn.l + alpha * kn.mt;
                        let mut coef = -kn.m - alpha * kn.n + ql * bj;
                        bi -= ql * aj;
                        if i == j {
                            coef += 0.5 + alpha * 0.5 * bj;
                            bi -= alpha * 0.5 * aj;
                        }
                        *aij = Complex32::new(coef.re as f32, coef.im as f32);
                    }
                    *b = bi;
                }
            });
        }
    });
    if stopped.load(Ordering::Relaxed) {
        return None;
    }
    Some((a, rhs))
}

/// out = A v(行を糸に分ける)。
fn matvec(a: &[Complex32], n: usize, v: &[C], out: &mut [C]) {
    let rows = n.div_ceil(threads(n));
    std::thread::scope(|s| {
        for (arows, o) in a.chunks(rows * n).zip(out.chunks_mut(rows)) {
            s.spawn(move || {
                for (row, oi) in arows.chunks(n).zip(o.iter_mut()) {
                    let (mut re, mut im) = (0.0f64, 0.0f64);
                    for (aij, vj) in row.iter().zip(v) {
                        let (ar, ai) = (aij.re as f64, aij.im as f64);
                        re += ar * vj.re - ai * vj.im;
                        im += ar * vj.im + ai * vj.re;
                    }
                    *oi = C::new(re, im);
                }
            });
        }
    });
}

/// 来た音の音圧と、法線 `nx` の向きの微分(法線が 0 なら微分は 0)。
fn incident(sources: &[Source], x: [f64; 3], nx: [f64; 3], k: f64) -> (C, C) {
    let mut p = C::new(0.0, 0.0);
    let mut dp = C::new(0.0, 0.0);
    for s in sources {
        match *s {
            Source::PlaneWave {
                direction,
                amplitude,
            } => {
                let d = unit(direction);
                let ph = -k * dot(d, x);
                let v = amplitude * C::new(ph.cos(), ph.sin());
                p += v;
                dp += v * C::new(0.0, -k * dot(d, nx));
            }
            Source::Point {
                position,
                amplitude,
            } => {
                let r = sub(x, position);
                let rr = norm(r);
                let e = C::new((k * rr).cos(), -(k * rr).sin());
                let v = amplitude * e / rr;
                p += v;
                // ∂/∂R (e^{−jkR}/R) = −(jk + 1/R) e^{−jkR}/R
                dp += -v * C::new(1.0 / rr, k) * (dot(r, nx) / rr);
            }
        }
    }
    (p, dp)
}

/// 受け取る所の値。
fn receive(r: &Receivers, elems: &[Elem], p: &[C], q: &[C], k: f64, sources: &[Source]) -> Vec<C> {
    let field = |x: [f64; 3]| {
        let mut v = incident(sources, x, [0.0; 3], k).0;
        for (j, e) in elems.iter().enumerate() {
            let kn = quad::element(x, [0.0; 3], e, k, false);
            v += kn.m * p[j] - kn.l * q[j];
        }
        v
    };
    match r {
        Receivers::None => vec![],
        Receivers::Points { points } => par_map(points, |x| field(*x)),
        Receivers::Directions {
            directions,
            distance: Some(d),
        } => par_map(directions, |dir| field(scale(unit(*dir), *d))),
        Receivers::Directions {
            directions,
            distance: None,
        } => par_map(directions, |dir| {
            // F(x̂) = (1/4π) Σ ∫ (jk(x̂·n)p − q) e^{jk x̂·y} dS
            let dir = unit(*dir);
            let mut f = C::new(0.0, 0.0);
            for (j, e) in elems.iter().enumerate() {
                let w = C::new(0.0, k * dot(dir, e.n)) * p[j] - q[j];
                f += w * quad::far(dir, e, k);
            }
            f / (4.0 * std::f64::consts::PI)
        }),
    }
}

/// 並びの各項に `f` をかける(糸に分ける)。
fn par_map<T: Sync>(items: &[T], f: impl Fn(&T) -> C + Sync) -> Vec<C> {
    let mut out = vec![C::new(0.0, 0.0); items.len()];
    if items.is_empty() {
        return out;
    }
    let per = items.len().div_ceil(threads(items.len()));
    let f = &f;
    std::thread::scope(|s| {
        for (it, o) in items.chunks(per).zip(out.chunks_mut(per)) {
            s.spawn(move || {
                for (x, v) in it.iter().zip(o.iter_mut()) {
                    *v = f(x);
                }
            });
        }
    });
    out
}
