//! 頭の網から HRTF を計算する(相反の定理と境界要素法)。
//!
//! 方向ごとに外から来る音を解く代わりに、耳の穴の入口の小さな面を振動させて 1 回解き、外へ出た音を
//! 方向ごとに読む(音の通り道は向きを逆にしても同じ伝わり方をする)。こうすると方向がいくつあっても、
//! 片耳につき周波数ごとに 1 回の計算で済む。
//!
//! - 伝達関数は「耳の位置の音源から各方向へ出た音」÷「頭が無い時に、頭の中心に同じ強さの音源を置いて
//!   出した音」。低い周波数では 1(0 dB)に近づく。
//! - 周波数は IR の長さ F の DFT の点(fs/F 刻み)で解き、逆 FFT で HRIR にする。上限より上は大きさを
//!   余弦でナイキストの 0 へ下げ、位相は上限の傾きで伸ばす。
//! - 全方向で一番早い立ち上がりが `lead` sample になるよう、共通の遅れを全部に同じだけ足す。
//! - 解き方は `kmb_wave::solve_bem`。座標は SOFA の決まり(x が前、y が左、z が上、m)。
//!
//! 入口は純関数 [`compute`](と途中の [`transfer`]・[`to_hrir`])。仕事([`HeadJob`])は JSON で往復できる。

use std::collections::BTreeMap;
use std::f64::consts::PI;

use kmb_wave::grid::SphereGrid;
use kmb_wave::{
    BemJob, BemMesh, Boundary, Complex64, Medium, Receivers, Solver, Watch, WaveError,
    WaveErrorKind, solve_bem,
};
use serde::{Deserialize, Serialize};

use crate::dsp::OnsetFinder;
use crate::fft::{C, Fft, signed_bin};
use crate::grid::{Rings, unit};
use crate::head::{Grading, regrade, tidy};
use crate::{Dir, Frame, Grid, HrirSet, HrtfError};

/// 頭の網から HRTF を計算する仕事。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HeadJob {
    /// 頭(と胴)の表面。閉じた三角形の網で法線は外向き(m)。全面を剛い面として解く
    pub mesh: BemMesh,
    /// 頭の中心(聴く人の位置)。方向と距離はここから測る(m)
    #[serde(default)]
    pub center: [f64; 3],
    /// 左・右の耳の穴の入口の点(m)
    pub ears: [[f64; 3]; 2],
    /// 入口として振動させる範囲の半径(m)。重心がこの中にある三角形を振動させる。
    /// 入口の点に一番近い三角形はいつも入る
    #[serde(default = "default_ear_radius")]
    pub ear_radius: f64,
    /// 耳からの距離で要素の大きさを変える作り直し。無ければ網をそのまま使う(確かめと小さな穴の
    /// 埋めはいつもする)
    #[serde(default)]
    pub grading: Option<Grading>,
    /// 音源の方向の格子
    #[serde(default)]
    pub directions: Directions,
    /// 音源までの距離(m)。無ければ遠方
    #[serde(default)]
    pub distance: Option<f64>,
    /// サンプリング周波数(Hz)
    #[serde(default = "default_fs")]
    pub fs: f64,
    /// HRIR の長さ(tap、2 の累乗)。解く周波数の刻みは fs / taps
    #[serde(default = "default_taps")]
    pub taps: usize,
    /// 解く周波数の上限(Hz)。その上はなだらかに下げる
    #[serde(default = "default_max_hz")]
    pub max_hz: f64,
    /// 全方向で一番早い立ち上がりの前に置く余白(sample)
    #[serde(default = "default_lead")]
    pub lead: f64,
    /// 空気の値
    #[serde(default)]
    pub medium: Medium,
    /// 解き方の設定
    #[serde(default)]
    pub solver: Solver,
}

fn default_ear_radius() -> f64 {
    0.002
}
fn default_fs() -> f64 {
    48_000.0
}
fn default_taps() -> usize {
    256
}
fn default_max_hz() -> f64 {
    20_000.0
}
fn default_lead() -> f64 {
    24.0
}

/// 音源の方向の格子。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Directions {
    /// 仰角が Gauss–Legendre の点、方位が等間隔。既定は 89 × 180(16020 方向)
    GaussLegendre { elevations: usize, azimuths: usize },
    /// 輪の格子(`crate::grid::Rings`、刻み °)
    Rings { step: f64 },
    /// 方向の並び(方位・仰角 °。距離は使わない)
    List { dirs: Vec<Dir> },
}

impl Default for Directions {
    fn default() -> Self {
        Directions::GaussLegendre {
            elevations: 89,
            azimuths: 180,
        }
    }
}

impl Directions {
    /// 方向(方位、仰角 °)の並びと、格子の種類。
    pub fn expand(&self) -> Result<(Vec<(f64, f64)>, Grid), HrtfError> {
        match self {
            Directions::GaussLegendre {
                elevations,
                azimuths,
            } => {
                if *elevations == 0 || *azimuths == 0 {
                    return Err(HrtfError::input("方向の格子の段の数が 0"));
                }
                let g = SphereGrid::gauss_legendre(*elevations, *azimuths);
                let mut out = Vec::with_capacity(g.len());
                for &e in &g.elevations {
                    for &a in &g.azimuths {
                        out.push((a, e));
                    }
                }
                Ok((
                    out,
                    Grid::GaussLegendre {
                        el: *elevations,
                        az: *azimuths,
                    },
                ))
            }
            Directions::Rings { step } => {
                let r = Rings::new(*step)?;
                Ok((r.dirs(), Grid::Rings { step: *step }))
            }
            Directions::List { dirs } => {
                if dirs.is_empty() {
                    return Err(HrtfError::input("方向の並びが空"));
                }
                if dirs
                    .iter()
                    .any(|d| !(d.az.is_finite() && (-90.0..=90.0).contains(&d.el)))
                {
                    return Err(HrtfError::input("方向の並びに球座標の範囲に無い値がある"));
                }
                Ok((dirs.iter().map(|d| (d.az, d.el)).collect(), Grid::Any))
            }
        }
    }
}

/// 周波数 1 つ・耳 1 つの解き方の記録。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SolveNote {
    /// 0 が左、1 が右
    pub ear: usize,
    pub frequency: f64,
    pub iterations: usize,
    pub residual: f64,
    /// 1 波長に入る要素の数の目安。6 を下回ると粗い
    pub elements_per_wavelength: f64,
    pub seconds: f64,
}

/// 周波数ごとの伝達関数(逆 FFT の前)。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transfer {
    /// 解いた周波数(Hz、fs / taps の 1 倍から順に)
    pub frequencies: Vec<f64>,
    /// 方向(方位、仰角 °)
    pub dirs: Vec<(f64, f64)>,
    pub grid: Grid,
    /// 耳ごと・周波数ごと・方向ごとの伝達関数(単位の無い複素数)
    pub h: [Vec<Vec<Complex64>>; 2],
    /// 耳ごとに振動させた三角形の番号(その耳を解いた網の番号)
    pub ear_triangles: [Vec<usize>; 2],
    /// 耳ごとに解いた網の三角形の数
    #[serde(default)]
    pub mesh_triangles: [usize; 2],
    /// 解き方の記録
    pub notes: Vec<SolveNote>,
    /// 中止されて、周波数が途中までか
    pub cancelled: bool,
}

/// 計算の結果。
#[derive(Clone, Debug, PartialEq)]
pub struct HeadOut {
    pub set: HrirSet,
    pub transfer: Transfer,
}

/// 頭の網から HRIR の集まりを計算する。
pub fn compute(job: &HeadJob, watch: &dyn Watch) -> Result<HeadOut, HrtfError> {
    let transfer = transfer(job, watch)?;
    let set = to_hrir(job, &transfer)?;
    Ok(HeadOut { set, transfer })
}

/// 解く周波数(fs / taps の 1 倍から、上限とナイキストの手前まで)。
pub fn frequencies(job: &HeadJob) -> Result<Vec<f64>, HrtfError> {
    if !(job.fs.is_finite() && job.fs > 0.0) {
        return Err(HrtfError::input(format!(
            "サンプリング周波数 {} Hz が正の数でない",
            job.fs
        )));
    }
    if job.taps < 4 || !job.taps.is_power_of_two() {
        return Err(HrtfError::input(format!(
            "HRIR の長さ {} が 4 以上の 2 の累乗でない",
            job.taps
        )));
    }
    if !(job.max_hz.is_finite() && job.max_hz > 0.0) {
        return Err(HrtfError::input("周波数の上限が正の数でない"));
    }
    let df = job.fs / job.taps as f64;
    let top = ((job.max_hz / df).floor() as usize).min(job.taps / 2 - 1);
    if top == 0 {
        return Err(HrtfError::input(format!(
            "周波数の上限 {} Hz が刻み {df} Hz より小さい",
            job.max_hz
        )));
    }
    Ok((1..=top).map(|k| k as f64 * df).collect())
}

/// 耳ごとに入口の面を振動させて解き、方向ごとの伝達関数を返す。
pub fn transfer(job: &HeadJob, watch: &dyn Watch) -> Result<Transfer, HrtfError> {
    let freqs = frequencies(job)?;
    let (dirs, grid) = job.directions.expand()?;
    if let Some(r) = job.distance
        && !(r.is_finite() && r > 0.0)
    {
        return Err(HrtfError::input(format!(
            "音源までの距離 {r} m が正の数でない"
        )));
    }
    if !(job.ear_radius.is_finite() && job.ear_radius >= 0.0) {
        return Err(HrtfError::input("耳の入口の半径が 0 以上の数でない"));
    }
    watch.progress(0.0, "頭の網を整えています");
    let meshes = head_meshes(job, &|| watch.cancelled())?;
    let ears = job.ears.map(|e| sub(e, job.center));
    let patches = [
        ear_patch(&meshes[0], ears[0], job.ear_radius).map_err(|e| e.at("左の耳"))?,
        ear_patch(&meshes[1], ears[1], job.ear_radius).map_err(|e| e.at("右の耳"))?,
    ];
    let shared = meshes[0] == meshes[1];
    if shared && patches[0].iter().any(|t| patches[1].contains(t)) {
        return Err(HrtfError::input("左と右の耳の入口の三角形が重なっている"));
    }
    let directions: Vec<[f64; 3]> = dirs.iter().map(|&(a, e)| unit(a, e)).collect();
    let rho = job.medium.density;
    let c = job.medium.speed_of_sound;
    let mut h: [Vec<Vec<Complex64>>; 2] = [Vec::new(), Vec::new()];
    let mut notes = Vec::new();
    let mut cancelled = false;
    for ear in 0..2 {
        let v = Boundary::Velocity {
            velocity: Complex64::new(1.0, 0.0),
        };
        let mut mesh = meshes[ear].clone();
        let mut groups = vec![0u32; mesh.triangles.len()];
        for &t in &patches[ear] {
            groups[t] = 1;
        }
        mesh.groups = groups;
        let bem = BemJob {
            mesh,
            boundaries: vec![Boundary::Rigid, v],
            sources: Vec::new(),
            frequencies: freqs.clone(),
            receivers: Receivers::Directions {
                directions: directions.clone(),
                distance: job.distance,
            },
            medium: job.medium,
            solver: job.solver,
        };
        // 頭が無い時に中心に置く音源の強さ(体積速度 m³/s)。振動の速さは 1 m/s。
        let q: f64 = patches[ear].iter().map(|&t| area(&meshes[ear], t)).sum();
        let side = if ear == 0 { "左の耳" } else { "右の耳" };
        let w = EarWatch {
            inner: watch,
            ear,
            side,
        };
        let out = solve_bem(&bem, &w).map_err(|e| wave_error(e).at(side))?;
        cancelled |= out.cancelled;
        for fo in &out.frequencies {
            let k = 2.0 * PI * fo.frequency / c;
            let jwrq = Complex64::new(0.0, 2.0 * PI * fo.frequency * rho * q / (4.0 * PI));
            let reference = match job.distance {
                None => jwrq,
                Some(r) => jwrq * Complex64::from_polar(1.0 / r, -k * r),
            };
            h[ear].push(fo.receivers.iter().map(|p| p / reference).collect());
            notes.push(SolveNote {
                ear,
                frequency: fo.frequency,
                iterations: fo.iterations,
                residual: fo.residual,
                elements_per_wavelength: fo.elements_per_wavelength,
                seconds: fo.seconds,
            });
        }
        if out.cancelled {
            break;
        }
    }
    // 中止された時は、両耳とも解けた周波数までにそろえる。
    let solved = h[0].len().min(h[1].len());
    h[0].truncate(solved);
    h[1].truncate(solved);
    let mut freqs = freqs;
    cancelled |= solved < freqs.len();
    freqs.truncate(solved);
    Ok(Transfer {
        frequencies: freqs,
        dirs,
        grid,
        h,
        ear_triangles: patches,
        mesh_triangles: meshes.each_ref().map(|m| m.triangles.len()),
        notes,
        cancelled,
    })
}

/// 耳ごとに解く網(左・右。頭の中心を原点へ動かした m)。網を確かめて小さな穴を埋め、作り直しの
/// 設定があれば耳からの距離で作り直す(耳ごとに作るなら、その耳の近くだけ細かい)。
pub fn head_meshes(job: &HeadJob, cancelled: &dyn Fn() -> bool) -> Result<[BemMesh; 2], HrtfError> {
    let mut mesh = job.mesh.clone();
    for v in &mut mesh.vertices {
        *v = sub(*v, job.center);
    }
    let ears = job.ears.map(|e| sub(e, job.center));
    let Some(g) = &job.grading else {
        let m = tidy(&mesh)?.mesh;
        return Ok([m.clone(), m]);
    };
    let grade = g.grade_for(&ears)?;
    if g.per_ear {
        let left = regrade(&mesh, &ears[..1], g.near, g.far, grade, cancelled)
            .map_err(|e| e.at("左の耳の網"))?;
        let right = regrade(&mesh, &ears[1..], g.near, g.far, grade, cancelled)
            .map_err(|e| e.at("右の耳の網"))?;
        Ok([left, right])
    } else {
        let m = regrade(&mesh, &ears, g.near, g.far, grade, cancelled)?;
        Ok([m.clone(), m])
    }
}

/// 伝達関数を逆 FFT して HRIR の集まりにする。
pub fn to_hrir(job: &HeadJob, t: &Transfer) -> Result<HrirSet, HrtfError> {
    let want = frequencies(job)?;
    if t.cancelled || t.frequencies.len() != want.len() {
        return Err(HrtfError::input(format!(
            "周波数が {} 個のうち {} 個しか解けていないので HRIR にできない",
            want.len(),
            t.frequencies.len()
        )));
    }
    let n = job.taps;
    let fft = Fft::new(n);
    let m = t.dirs.len();
    // 先に、耳が中心より音源に近い分(最大で耳の中心からの距離 / 音速)を足して因果にしておく。
    let reach = job
        .ears
        .iter()
        .map(|e| norm(sub(*e, job.center)))
        .fold(0.0, f64::max);
    let first = job.lead + (reach / job.medium.speed_of_sound * job.fs).ceil();
    let make = |delay: f64| -> Vec<[Vec<f64>; 2]> {
        (0..m)
            .map(|d| {
                [0, 1].map(|ear| {
                    let col: Vec<Complex64> = t.h[ear].iter().map(|row| row[d]).collect();
                    fft.real(full_spectrum(&col, n, delay))
                })
            })
            .collect()
    };
    let trial = make(first);
    let finder = OnsetFinder::new(n, job.fs);
    let earliest = trial
        .iter()
        .flatten()
        .map(|ir| finder.find(ir))
        .fold(f64::INFINITY, f64::min);
    let delay = (first + job.lead - earliest).round().max(0.0);
    let ir = if delay == first { trial } else { make(delay) };
    if ir.iter().flatten().flatten().any(|v| !v.is_finite()) {
        return Err(HrtfError::compute("HRIR に無限大か非数が出た"));
    }
    let dist = job.distance.unwrap_or(1.0);
    let dirs = t
        .dirs
        .iter()
        .map(|&(az, el)| Dir {
            az: az.rem_euclid(360.0),
            el,
            dist,
        })
        .collect();
    let tris = if t.mesh_triangles[0] == t.mesh_triangles[1] {
        t.mesh_triangles[0].to_string()
    } else {
        format!("左 {}・右 {}", t.mesh_triangles[0], t.mesh_triangles[1])
    };
    let mut meta = BTreeMap::new();
    meta.insert("Title".into(), "境界要素法で計算した HRTF".into());
    meta.insert(
        "Comment".into(),
        format!(
            "頭の網 三角形 {tris}、耳の入口 左 {}・右 {} 個、周波数 {} 個({:.1}〜{:.1} Hz)、{}",
            t.ear_triangles[0].len(),
            t.ear_triangles[1].len(),
            t.frequencies.len(),
            t.frequencies[0],
            t.frequencies[t.frequencies.len() - 1],
            match job.distance {
                None => "遠方(距離 1 m は名目の値)".to_string(),
                Some(r) => format!("距離 {r} m"),
            }
        ),
    );
    let history = vec![format!(
        "compute: 境界要素法(相反の定理)で {} 方向・{} tap・fs {} Hz、共通の遅れ {delay} sample",
        m, n, job.fs
    )];
    let set = HrirSet {
        fs: job.fs,
        dirs,
        ir,
        delay: vec![[0.0, 0.0]; m],
        onset: None,
        frame: Frame::Linear,
        grid: t.grid,
        meta,
        history,
    };
    set.check()?;
    Ok(set)
}

/// 0 Hz から上限までの値から、長さ n の全部の周波数の値(実数の IR になる対称の形)を作り、
/// `delay` sample 遅らせる。0 Hz は 1、上限より上は大きさを余弦でナイキストの 0 まで下げ、
/// 位相は上限の 1 刻みの傾きで伸ばす。
fn full_spectrum(h: &[Complex64], n: usize, delay: f64) -> Vec<C> {
    let half = n / 2;
    let top = h.len();
    let mut s = vec![C::new(0.0, 0.0); n];
    s[0] = C::new(1.0, 0.0);
    for (k, v) in h.iter().enumerate() {
        s[k + 1] = C::new(v.re, v.im);
    }
    let last = h[top - 1];
    let slope = if top >= 2 {
        (last / h[top - 2]).arg()
    } else {
        0.0
    };
    for (k, v) in s.iter_mut().enumerate().take(half + 1).skip(top + 1) {
        let x = (k - top) as f64 / (half - top) as f64;
        let w = 0.5 + 0.5 * (PI * x).cos();
        let phase = last.arg() + slope * (k - top) as f64;
        *v = C::from_polar(last.norm() * w, phase);
    }
    s[half] = C::new(0.0, 0.0);
    for k in 1..half {
        s[n - k] = s[k].conj();
    }
    for (k, v) in s.iter_mut().enumerate() {
        let a = -2.0 * PI * signed_bin(k, n) * delay / n as f64;
        *v *= C::new(a.cos(), a.sin());
    }
    s
}

/// 耳の入口として振動させる三角形(一番近い 1 つと、重心が半径の中にある物)。
fn ear_patch(mesh: &BemMesh, ear: [f64; 3], radius: f64) -> Result<Vec<usize>, HrtfError> {
    let mut best: Option<(usize, f64)> = None;
    let mut out = Vec::new();
    for (t, tri) in mesh.triangles.iter().enumerate() {
        let v = tri.map(|i| mesh.vertices.get(i as usize).copied());
        let [Some(a), Some(b), Some(c)] = v else {
            return Err(HrtfError::input(format!(
                "三角形 {t} が無い頂点を指している"
            )));
        };
        let g = [
            (a[0] + b[0] + c[0]) / 3.0,
            (a[1] + b[1] + c[1]) / 3.0,
            (a[2] + b[2] + c[2]) / 3.0,
        ];
        let d = norm(sub(g, ear));
        if d <= radius {
            out.push(t);
        }
        if best.is_none_or(|(_, bd)| d < bd) {
            best = Some((t, d));
        }
    }
    let Some((t, d)) = best else {
        return Err(HrtfError::input("網に三角形が無い"));
    };
    let size = longest_edge(mesh, t);
    if d > size.max(0.005) {
        return Err(HrtfError::input(format!(
            "入口の点が網の表面から {:.1} mm 離れている(一番近い三角形の重心まで)",
            d * 1000.0
        )));
    }
    if !out.contains(&t) {
        out.push(t);
    }
    out.sort_unstable();
    Ok(out)
}

fn area(mesh: &BemMesh, t: usize) -> f64 {
    let [a, b, c] = mesh.triangles[t].map(|i| mesh.vertices[i as usize]);
    0.5 * norm(cross(sub(b, a), sub(c, a)))
}

fn longest_edge(mesh: &BemMesh, t: usize) -> f64 {
    let [a, b, c] = mesh.triangles[t].map(|i| mesh.vertices[i as usize]);
    norm(sub(b, a)).max(norm(sub(c, b))).max(norm(sub(a, c)))
}

fn wave_error(e: WaveError) -> HrtfError {
    match e.kind {
        WaveErrorKind::Input => HrtfError::input(e.message),
        WaveErrorKind::Compute => HrtfError::compute(e.message),
        WaveErrorKind::Unavailable => HrtfError::unsupported(e.message),
    }
}

/// 片耳の進みを全体の半分に写す。
struct EarWatch<'a> {
    inner: &'a dyn Watch,
    ear: usize,
    side: &'static str,
}

impl Watch for EarWatch<'_> {
    fn cancelled(&self) -> bool {
        self.inner.cancelled()
    }
    fn progress(&self, fraction: f64, note: &str) {
        self.inner.progress(
            (self.ear as f64 + fraction) / 2.0,
            &format!("{}: {note}", self.side),
        );
    }
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kmb_wave::exact::sphere_plane_wave;
    use kmb_wave::icosphere;

    const A: f64 = 0.0875;

    /// 網の三角形のうち、重心が点に一番近い物の重心。
    fn nearest_centroid(mesh: &BemMesh, p: [f64; 3]) -> [f64; 3] {
        mesh.triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| mesh.vertices[i as usize]);
                [0, 1, 2].map(|k| (a[k] + b[k] + c[k]) / 3.0)
            })
            .min_by(|x, y| norm(sub(*x, p)).total_cmp(&norm(sub(*y, p))))
            .unwrap()
    }

    fn sphere_job(subdivisions: u32, dirs: Vec<Dir>) -> HeadJob {
        let mesh = icosphere(A, subdivisions);
        let ears = [
            nearest_centroid(&mesh, [0.0, A, 0.0]),
            nearest_centroid(&mesh, [0.0, -A, 0.0]),
        ];
        HeadJob {
            mesh,
            center: [0.0; 3],
            ears,
            ear_radius: 0.0,
            grading: None,
            directions: Directions::List { dirs },
            distance: None,
            fs: 8000.0,
            taps: 32,
            max_hz: 2000.0,
            lead: 4.0,
            medium: Medium::default(),
            solver: Solver::default(),
        }
    }

    fn dir(az: f64, el: f64) -> Dir {
        Dir { az, el, dist: 0.0 }
    }

    fn db(x: f64) -> f64 {
        20.0 * x.log10()
    }

    /// 確かめること: 剛い球の頭で、耳を表面の点にした時の伝達関数が、平面波の散乱の級数解(頭の中心で
    /// 振幅 1)と、ka ≈ 0.4〜3.2 の全部の方向で大きさ 0.5 dB 以内・位相 0.05 rad 以内に合う(相反の
    /// 定理と、頭が無い時で割る正規化が正しい。2026-11-01 の実測は 0.06 dB・0.013 rad)。
    #[test]
    fn rigid_sphere_head_matches_the_series() {
        let dirs: Vec<Dir> = [
            (0.0, 0.0),
            (45.0, 0.0),
            (90.0, 0.0),
            (135.0, 0.0),
            (180.0, 0.0),
            (270.0, 0.0),
            (0.0, 60.0),
            (90.0, -60.0),
        ]
        .map(|(a, e)| dir(a, e))
        .to_vec();
        let job = sphere_job(3, dirs.clone());
        let t = transfer(&job, &()).unwrap();
        assert!(!t.cancelled);
        assert_eq!(t.frequencies.len(), 8);
        let c = job.medium.speed_of_sound;
        let mut worst_db: f64 = 0.0;
        let mut worst_rad: f64 = 0.0;
        for ear in 0..2 {
            let e = job.ears[ear];
            let eu = e.map(|x| x / norm(e));
            for (fi, f) in t.frequencies.iter().enumerate() {
                let k = 2.0 * PI * f / c;
                for (di, d) in dirs.iter().enumerate() {
                    let u = unit(d.az, d.el);
                    // 波は音源の方向から頭へ向かって(−u の向きに)進む。
                    let cos = -(eu[0] * u[0] + eu[1] * u[1] + eu[2] * u[2]);
                    let want = sphere_plane_wave(k, A, norm(e), cos, Complex64::new(0.0, 0.0));
                    let got = t.h[ear][fi][di];
                    worst_db = worst_db.max((db(got.norm()) - db(want.norm())).abs());
                    worst_rad = worst_rad.max((got / want).arg().abs());
                }
            }
        }
        assert!(worst_db < 0.5, "大きさの差 {worst_db:.3} dB");
        assert!(worst_rad < 0.05, "位相の差 {worst_rad:.3} rad");
    }

    /// 確かめること: 粗い球で通しで計算すると、HRIR の集まりの決まりを満たし、左から来る音は左の耳が
    /// 大きく早く、一番早い立ち上がりが余白の位置に来て、SOFA に書いて読み直せる。
    #[test]
    fn coarse_sphere_runs_end_to_end() {
        let job = sphere_job(2, vec![dir(90.0, 0.0), dir(270.0, 0.0), dir(0.0, 0.0)]);
        let out = compute(&job, &()).unwrap();
        let set = &out.set;
        set.check().unwrap();
        assert_eq!(set.taps(), 32);
        assert_eq!(set.dirs.len(), 3);
        let energy = |x: &[f64]| x.iter().map(|v| v * v).sum::<f64>();
        // 左(方位 90°)から来る音は左が大きい。右(270°)は逆。
        assert!(energy(&set.ir[0][0]) > 1.5 * energy(&set.ir[0][1]));
        assert!(energy(&set.ir[1][1]) > 1.5 * energy(&set.ir[1][0]));
        let finder = OnsetFinder::new(32, job.fs);
        let onsets: Vec<f64> = set.ir.iter().flatten().map(|ir| finder.find(ir)).collect();
        let earliest = onsets.iter().cloned().fold(f64::INFINITY, f64::min);
        assert!(
            (earliest - job.lead).abs() <= 1.0,
            "一番早い立ち上がり {earliest}"
        );
        // 左から来る音は左の耳に早く着く。
        assert!(onsets[0] < onsets[1], "{onsets:?}");
        let bytes = crate::sofa::write(set, &crate::sofa::SofaSpec::default()).unwrap();
        let back = crate::sofa::read(&bytes).unwrap();
        assert_eq!(back.dirs.len(), 3);
        assert!(back.meta["Title"].contains("境界要素法"));
    }

    /// 確かめること: 伝達関数が純粋な遅れ・進みなら、HRIR は 0 Hz の値が 1(IR の和が 1)で、進みの
    /// 一番大きい方向の立ち上がりが余白の位置に来る。
    #[test]
    fn pure_delays_become_shifted_impulses() {
        let mut job = sphere_job(0, vec![dir(0.0, 0.0), dir(90.0, 0.0)]);
        job.fs = 48_000.0;
        job.taps = 64;
        job.max_hz = 20_000.0;
        job.lead = 8.0;
        job.ears = [[0.0, 0.09, 0.0], [0.0, -0.09, 0.0]];
        let freqs = frequencies(&job).unwrap();
        // 方向 0 は 3 sample 進み、方向 1 は 2 sample 遅れ(左右とも)。
        let shift = [3.0, -2.0];
        let row = |f: f64| -> Vec<Complex64> {
            shift
                .iter()
                .map(|s| Complex64::from_polar(1.0, 2.0 * PI * f * s / job.fs))
                .collect()
        };
        let h: Vec<Vec<Complex64>> = freqs.iter().map(|&f| row(f)).collect();
        let t = Transfer {
            frequencies: freqs,
            dirs: vec![(0.0, 0.0), (90.0, 0.0)],
            grid: Grid::Any,
            h: [h.clone(), h],
            ear_triangles: [vec![0], vec![1]],
            mesh_triangles: [20, 20],
            notes: Vec::new(),
            cancelled: false,
        };
        let set = to_hrir(&job, &t).unwrap();
        let peak = |x: &[f64]| {
            x.iter()
                .enumerate()
                .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
                .unwrap()
                .0
        };
        let p0 = peak(&set.ir[0][0]);
        let p1 = peak(&set.ir[1][0]);
        assert_eq!(p1 - p0, 5, "進みと遅れの差は 5 sample");
        let finder = OnsetFinder::new(64, job.fs);
        let o0 = finder.find(&set.ir[0][0]);
        assert!((o0 - job.lead).abs() <= 1.0, "立ち上がり {o0}");
        let sum: f64 = set.ir[0][0].iter().sum();
        assert!((sum - 1.0).abs() < 1e-9, "IR の和 {sum}");
    }

    /// 確かめること: 仕事は既定の値つきで JSON から読め、書いて読むと同じになる。
    #[test]
    fn job_reads_with_defaults_and_round_trips() {
        let text =
            r#"{"mesh":{"vertices":[[0,0,0]],"triangles":[]},"ears":[[0,0.09,0],[0,-0.09,0]]}"#;
        let job: HeadJob = serde_json::from_str(text).unwrap();
        assert_eq!(job.fs, 48_000.0);
        assert_eq!(job.taps, 256);
        assert_eq!(
            job.directions,
            Directions::GaussLegendre {
                elevations: 89,
                azimuths: 180
            }
        );
        assert_eq!(job.directions.expand().unwrap().0.len(), 16020);
        assert_eq!(frequencies(&job).unwrap().len(), 106);
        let back: HeadJob = serde_json::from_str(&serde_json::to_string(&job).unwrap()).unwrap();
        assert_eq!(back, job);
    }

    /// 確かめること: 耳の点が網から離れている・左右の入口が重なる・長さが 2 の累乗でない時は、解く前に
    /// 入力の誤りになる。
    #[test]
    fn bad_jobs_are_input_errors() {
        let base = sphere_job(1, vec![dir(0.0, 0.0)]);
        let mut far = base.clone();
        far.ears[0] = [0.0, 0.2, 0.0];
        let e = transfer(&far, &()).unwrap_err();
        assert_eq!(e.kind, crate::ErrorKind::Input);
        assert!(e.message.contains("左の耳"), "{}", e.message);
        let mut same = base.clone();
        same.ears[1] = same.ears[0];
        assert_eq!(
            transfer(&same, &()).unwrap_err().kind,
            crate::ErrorKind::Input
        );
        let mut taps = base;
        taps.taps = 48;
        assert_eq!(
            transfer(&taps, &()).unwrap_err().kind,
            crate::ErrorKind::Input
        );
    }
}
