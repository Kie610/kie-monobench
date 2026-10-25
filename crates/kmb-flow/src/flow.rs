//! ボディのまわりの流れを格子ボルツマン法で解く(2D は断面、3D は箱の中)。形を流れの座標の
//! 格子に置き、入口・出口・横の面を決め、壁までの距離を網から出して、流れ始めから時刻を進めて
//! 解く。決まった間隔でセルごとの速さ・圧力をコマとして渡し、力の係数を返す。
//!
//! 単位: 長さは mm(格子)、速さは m/s、動粘性係数は m²/s。

use crate::geom::Shape;
use crate::lbm::{D2Q9, D3Q19, Setup, Solver, cell, is_solid};

/// 流体の値。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fluid {
    pub name: &'static str,
    /// 密度 kg/m³ と動粘性係数 m²/s(20 ℃)
    pub rho: f64,
    pub nu: f64,
}

pub const AIR: Fluid = Fluid {
    name: "空気",
    rho: 1.204,
    nu: 1.516e-5,
};

pub const WATER: Fluid = Fluid {
    name: "水",
    rho: 998.2,
    nu: 1.004e-6,
};

pub fn fluid(key: &str) -> Fluid {
    if key == "water" { WATER } else { AIR }
}

/// 解く条件(長さは流れの座標の mm)。
#[derive(Debug, Clone, Copy)]
pub struct FlowSpec {
    /// 2 か 3
    pub dims: usize,
    /// 2D の断面(3D は見せる断面)の z
    pub plane_z: f64,
    /// セルの大きさ(None なら自動)
    pub h: Option<f64>,
    pub speed: f64,
    pub fluid: Fluid,
    /// 残すコマの数(終わりまでを等しい間隔で。最後の状態はいつも残す)
    pub frames: usize,
    /// 終わりの時刻(s)。None なら抗力が落ち着くまで
    pub end: Option<f64>,
}

/// 解いている時の様子を伝え、中止を受ける相手。
pub trait Watch {
    /// 中止されたか(されたら、その時の状態を最後のコマにして返す)
    fn cancelled(&self) -> bool {
        false
    }
    /// 進み具合(0〜1)と、今の時刻(s)
    fn progress(&mut self, _fraction: f64, _time: f64) {}
    /// 抗力係数の推移(時刻 s と係数)
    fn drag(&mut self, _time: f64, _cd: f64) {}
    /// 解き始める時に 1 回: 格子のセルの数・セル (0,0,0) の角(流れの座標 mm)・1 辺・セルの種類。
    /// どれも吸い込みの帯を除いた所(結果に出す所)
    fn start(&mut self, _n: [usize; 3], _origin: [f64; 3], _h: f64, _cell: &[u8]) {}
    /// コマ 1 つ: 時刻(s)、速さ(m/s、流れの座標)、圧力(Pa、入口との差)。並びは [`Watch::start`] の格子
    fn frame(&mut self, _time: f64, _vel: &[[f32; 3]], _pressure: &[f32]) {}
}

/// 様子を見ない時。
pub struct NoWatch;

impl Watch for NoWatch {}

/// 解いた結果(セルの値はコマで [`Watch::frame`] に渡す)。
pub struct FlowOut {
    pub n: [usize; 3],
    pub origin: [f64; 3],
    pub h: f64,
    pub cell: Vec<u8>,
    /// 力の係数(流れの向き・横・上)と、もとにした面積(3D は mm²、2D は mm)
    pub coef: [f64; 3],
    pub area: f64,
    /// 力(3D は N、2D は奥行き 1 m あたりの N)
    pub force: [f64; 3],
    /// 大きさの代表(流れに直角の幅、mm)
    pub size: f64,
    pub re_real: f64,
    pub re_calc: f64,
    pub steps: usize,
    /// 1 刻みの時間(s)と、解いた時刻(s)
    pub dt: f64,
    pub time: f64,
    /// 中止して途中で止めたか
    pub cancelled: bool,
}

/// 格子の速さ(音速の 0.14 倍ほど。圧縮性の誤差が小さい)。
const U_LAT: f64 = 0.08;

pub fn run(shape: &Shape, spec: &FlowSpec, watch: &mut dyn Watch) -> Result<FlowOut, String> {
    let (lo, hi) = (shape.lo, shape.hi);
    let ext = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
    let three = spec.dims == 3;
    if !three && !(lo[2] < spec.plane_z && spec.plane_z < hi[2]) {
        return Err(format!(
            "断面の位置がボディの外です(ボディは {:.1}〜{:.1} mm)",
            lo[2], hi[2]
        ));
    }
    let size = if three { ext[1].max(ext[2]) } else { ext[1] }.max(1e-6);
    // 箱: 前に 1.5 倍、後ろに 4 倍、横に 2D は 2 倍・3D は 1.5 倍ずつ(ボディの幅に対して)。
    // 後ろにはさらに 1 倍の吸い込みの帯を足す(結果には出さない)
    let side = if three { 1.5 } else { 2.0 };
    let mut blo = [lo[0] - 1.5 * size, lo[1] - side * size, lo[2] - side * size];
    let mut bhi = [hi[0] + 5.0 * size, hi[1] + side * size, hi[2] + side * size];
    if !three {
        blo[2] = spec.plane_z;
        bhi[2] = spec.plane_z;
    }
    let target = if three { 220_000.0 } else { 60_000.0 };
    let h = match spec.h {
        Some(h) => h,
        None => {
            let a = (bhi[0] - blo[0]) * (bhi[1] - blo[1]);
            if three {
                (a * (bhi[2] - blo[2]) / target).cbrt()
            } else {
                (a / target).sqrt()
            }
        }
    };
    if h.is_nan() || h <= 0.0 {
        return Err("格子の大きさは 0 より大きくしてください".into());
    }
    let n = [
        ((bhi[0] - blo[0]) / h).ceil() as usize + 1,
        ((bhi[1] - blo[1]) / h).ceil() as usize + 1,
        if three {
            ((bhi[2] - blo[2]) / h).ceil() as usize + 1
        } else {
            1
        },
    ];
    let total = n[0] * n[1] * n[2];
    if total > if three { 3_000_000 } else { 1_500_000 } {
        return Err(format!(
            "格子 {}×{}×{} は細かすぎます。格子の大きさを大きくしてください",
            n[0], n[1], n[2]
        ));
    }
    if size / h < 6.0 {
        return Err(format!(
            "格子 {h:.2} mm ではボディの幅 {size:.1} mm が {:.0} セルしかありません。格子を小さくしてください",
            size / h
        ));
    }
    // セルの中心: origin + (i + 0.5) h(2D の z は断面)
    let origin = [
        blo[0] - 0.5 * h,
        blo[1] - 0.5 * h,
        if three {
            blo[2] - 0.5 * h
        } else {
            spec.plane_z
        },
    ];
    let center = |i: usize, j: usize, k: usize| {
        [
            origin[0] + (i as f64 + 0.5) * h,
            origin[1] + (j as f64 + 0.5) * h,
            if three {
                origin[2] + (k as f64 + 0.5) * h
            } else {
                spec.plane_z
            },
        ]
    };
    let v = if three { &D3Q19 } else { &D2Q9 };
    let mut s = Setup::new(v, n);
    // ボディの中のセル(列ごとに交わりを数える)
    let mut body_cells = 0usize;
    for j in 0..n[1] {
        for i in 0..n[0] {
            let c = center(i, j, 0);
            let zs = shape.crossings(c[0], c[1]);
            if zs.is_empty() {
                continue;
            }
            for k in 0..n[2] {
                let z = center(i, j, k)[2];
                let above = zs.iter().filter(|w| **w > z).count();
                if above % 2 == 1 {
                    let x = s.index(i, j, k);
                    s.cell[x] = cell::BODY;
                    body_cells += 1;
                }
            }
        }
    }
    if body_cells == 0 {
        return Err("格子にボディが入りません(格子を小さくしてください)".into());
    }
    // 端の面: 入口・出口・横(ボディの所は除く)
    for k in 0..n[2] {
        for j in 0..n[1] {
            for i in 0..n[0] {
                let x = s.index(i, j, k);
                if is_solid(s.cell[x]) {
                    continue;
                }
                let side = j == 0 || j == n[1] - 1 || (three && (k == 0 || k == n[2] - 1));
                s.cell[x] = if i == 0 {
                    cell::INLET
                } else if i == n[0] - 1 {
                    cell::OUTLET
                } else if side {
                    cell::FAR
                } else {
                    cell::FLUID
                };
            }
        }
    }
    // 壁までの距離
    let q = v.c.len();
    s.q = vec![f32::NAN; total * q];
    let mut links = 0usize;
    for k in 0..n[2] {
        for j in 0..n[1] {
            for i in 0..n[0] {
                let x = s.index(i, j, k);
                if s.cell[x] != cell::FLUID {
                    continue;
                }
                let p = center(i, j, k);
                for d in 1..q {
                    let c = v.c[d];
                    let (ni, nj, nk) = (
                        i as isize + c[0] as isize,
                        j as isize + c[1] as isize,
                        k as isize + c[2] as isize,
                    );
                    if ni < 0 || nj < 0 || nk < 0 {
                        continue;
                    }
                    let (ni, nj, nk) = (ni as usize, nj as usize, nk as usize);
                    if ni >= n[0] || nj >= n[1] || nk >= n[2] {
                        continue;
                    }
                    if s.cell[s.index(ni, nj, nk)] != cell::BODY {
                        continue;
                    }
                    let e = [
                        p[0] + c[0] as f64 * h,
                        p[1] + c[1] as f64 * h,
                        p[2] + c[2] as f64 * h,
                    ];
                    if let Some(t) = shape.hit(p, e) {
                        s.q[x * q + d] = t as f32;
                    }
                    links += 1;
                }
            }
        }
    }
    // 粘性: 実際の Re に合わせ、格子で表せない時は下限で解く(渦粘性を足す)
    let cells_across = size / h;
    let re_real = spec.speed * size / 1000.0 / spec.fluid.nu;
    let nu_min = if three { 0.004 } else { 0.008 };
    let nu_lat = (U_LAT * cells_across / re_real.max(1e-9)).max(nu_min);
    let re_calc = U_LAT * cells_across / nu_lat;
    s.nu = nu_lat as f32;
    // 渦粘性(Smagorinsky)はいつも入れる。粗い格子の角でひずみが大きい所だけ効き、Re が低くても
    // 角から発散するのを防ぐ(格子で解ける所ではほとんど効かない)
    s.smag = 0.12;
    s.u_in = [U_LAT as f32, 0.0, 0.0];
    // 吸い込みの帯: 出口の前の 1 倍の幅で、粘性を 2 乗で上げる
    let band = ((size / h).round() as usize).clamp(4, n[0] / 4);
    let keep = n[0] - band;
    s.sponge = (0..n[0])
        .map(|i| {
            let t = (i as f64 - keep as f64).max(0.0) / band as f64;
            (0.1 * t * t) as f32
        })
        .collect();
    log::info!(
        "流れ: 格子 {}×{}×{}(ボディ {body_cells} セル、壁の線 {links} 本)、{h:.3} mm、Re {re_real:.0}(計算 {re_calc:.0})、ν {nu_lat:.4}",
        n[0],
        n[1],
        n[2]
    );
    // 1 刻みの時間: 格子の速さ U_LAT が実際の速さに当たる
    let dt = h / 1000.0 * U_LAT / spec.speed;
    let through = (n[0] as f64 / U_LAT).ceil() as usize;
    let (min_steps, max_steps) = match spec.end {
        Some(t) => {
            let s = (t / dt).round() as usize;
            if s == 0 {
                return Err("終わりの時刻が短すぎます(1 刻みより短い)".into());
            }
            if s > 20 * through {
                return Err(format!(
                    "終わりの時刻 {t} s は長すぎます(流れが箱を {} 回通り抜けるまで、{} s まで)",
                    20,
                    20.0 * through as f64 * dt
                ));
            }
            (s, s)
        }
        None if three => (through, 2 * through),
        None => (through, (2.5 * through as f64) as usize),
    };
    // 風は止まった所から流れが箱の半分を通るまでに上げる(音の波が箱を数往復する間。一度に
    // 当てると強い圧力の波が出て、終わりまで場全体の圧力が上下する)
    let ramp = (through / 2).min(min_steps / 2);
    let (min_steps, max_steps) = if spec.end.is_some() {
        (min_steps, max_steps)
    } else {
        (min_steps + ramp, max_steps + ramp)
    };
    s.ramp = ramp;
    s.open_outlet = true;
    let mut sv = Solver::new(s);
    let batch = (through / 10).max(100).min(max_steps);
    // k 番目のコマの刻み(風が全部の速さになってから終わりまでを等しく分け、最後のコマは終わりの
    // 刻み)。風を上げている間は流れを速めるための圧力の差が場全体にかかり、落ち着いた後の何倍にも
    // なるので、コマにしない(凡例の幅がそれで決まってしまう)
    let frames = spec.frames.max(1);
    let frame_at = |k: usize| ramp + (k * (max_steps - ramp)).div_ceil(frames).max(1);
    // 結果は吸い込みの帯を除いた所
    let out_n = [keep, n[1], n[2]];
    let kept = out_n[0] * out_n[1] * out_n[2];
    let scale = spec.speed / U_LAT;
    let mut cells = Vec::with_capacity(kept);
    for k in 0..n[2] {
        for j in 0..n[1] {
            for i in 0..keep {
                cells.push(sv.setup.cell[sv.setup.index(i, j, k)]);
            }
        }
    }
    let mut vel = vec![[0.0f32; 3]; kept];
    let mut pressure = vec![0.0f32; kept];
    let rho = spec.fluid.rho;
    let mut snapshot = |sv: &Solver, watch: &mut dyn Watch| {
        // 圧力の基準: 入口のすぐ内側の面の平均(場全体が一様に上下する分も除く)
        let (mut sum, mut count) = (0.0f64, 0usize);
        for k in 0..n[2] {
            for j in 0..n[1] {
                let x = sv.setup.index(1, j, k);
                if sv.setup.cell[x] == cell::FLUID {
                    sum += sv.macro_at(x).0 as f64;
                    count += 1;
                }
            }
        }
        let r0 = if count > 0 { sum / count as f64 } else { 1.0 };
        let mut x = 0;
        for k in 0..n[2] {
            for j in 0..n[1] {
                for i in 0..keep {
                    let (r, u) = sv.macro_at(sv.setup.index(i, j, k));
                    vel[x] = u.map(|c| (c as f64 * scale) as f32);
                    pressure[x] = ((r as f64 - r0) / 3.0 * rho * scale * scale) as f32;
                    x += 1;
                }
            }
        }
        watch.frame(sv.steps as f64 * dt, &vel, &pressure);
    };
    watch.start(out_n, origin, h, &cells);
    let qd = |cells: f64| 0.5 * U_LAT * U_LAT * cells;
    let mut last = f64::NAN;
    let mut next_check = batch;
    let mut framed_k = 1;
    let mut next_frame = frame_at(framed_k);
    let mut framed = usize::MAX;
    let mut cancelled = false;
    while sv.steps < max_steps {
        let to = next_check.min(next_frame).min(max_steps);
        sv.run(to - sv.steps);
        if sv.blew_up() {
            return Err("計算が発散しました。格子を小さくするか、速さを下げてください".into());
        }
        if sv.steps >= next_frame {
            snapshot(&sv, watch);
            framed = sv.steps;
            while next_frame <= sv.steps {
                framed_k += 1;
                next_frame = frame_at(framed_k);
            }
        }
        if sv.steps >= next_check {
            next_check += batch;
            let fx = sv.mean_force(batch)[0];
            let t = sv.steps as f64 * dt;
            // 推移の抗力係数は 2D の幅で割った目安(正面の面積は最後に数える)
            watch.drag(t, fx / qd(size / h));
            watch.progress(sv.steps as f64 / max_steps as f64, t);
            if spec.end.is_none()
                && sv.steps >= min_steps
                && (fx - last).abs() < 0.003 * fx.abs().max(1e-12)
            {
                break;
            }
            last = fx;
        }
        if watch.cancelled() {
            cancelled = true;
            break;
        }
    }
    if framed != sv.steps {
        snapshot(&sv, watch);
    }
    let window = (sv.steps / 4).max(batch).min(sv.steps.max(1));
    let f = sv.mean_force(window);
    // 正面の面積(3D は流れの向きに見たボディのセルの数、2D は幅)
    let area_cells = if three {
        let mut seen = 0usize;
        for k in 0..n[2] {
            for j in 0..n[1] {
                if (0..n[0]).any(|i| sv.setup.cell[sv.setup.index(i, j, k)] == cell::BODY) {
                    seen += 1;
                }
            }
        }
        seen as f64
    } else {
        cells_across
    };
    let coef = f.map(|v| v / qd(area_cells));
    let (area, real_area) = if three {
        (area_cells * h * h, area_cells * h * h * 1e-6)
    } else {
        (size, size * 1e-3)
    };
    let dyn_p = 0.5 * spec.fluid.rho * spec.speed * spec.speed;
    let force = coef.map(|c| c * dyn_p * real_area);
    Ok(FlowOut {
        n: out_n,
        origin,
        h,
        cell: cells,
        coef,
        area,
        force,
        size,
        re_real,
        re_calc,
        steps: sv.steps,
        dt,
        time: sv.steps as f64 * dt,
        cancelled,
    })
}

/// セル (i,j,k) の渦(1/s)。2D は面に直角の成分(向き付き)、3D は大きさ。
pub fn vorticity(
    n: [usize; 3],
    h: f64,
    cell: &[u8],
    vel: &[[f32; 3]],
    i: usize,
    j: usize,
    k: usize,
) -> f32 {
    let index = |i: usize, j: usize, k: usize| i + n[0] * (j + n[1] * k);
    let at = |i: usize, j: usize, k: usize| -> [f32; 3] {
        let x = index(i, j, k);
        if is_solid(cell[x]) { [0.0; 3] } else { vel[x] }
    };
    // 軸 a の向きの、成分 c の差分
    let d = |a: usize, c: usize| -> f32 {
        let mut lo = [i, j, k];
        let mut hi = [i, j, k];
        let p = [i, j, k][a];
        lo[a] = p.saturating_sub(1);
        hi[a] = (p + 1).min(n[a] - 1);
        let w = (hi[a] - lo[a]).max(1) as f32;
        (at(hi[0], hi[1], hi[2])[c] - at(lo[0], lo[1], lo[2])[c]) / w
    };
    let s = 1.0 / (h as f32 * 1e-3);
    let wz = d(0, 1) - d(1, 0);
    if n[2] == 1 {
        return wz * s;
    }
    let wx = d(1, 2) - d(2, 1);
    let wy = d(2, 0) - d(0, 2);
    (wx * wx + wy * wy + wz * wz).sqrt() * s
}
