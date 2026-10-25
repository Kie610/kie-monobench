//! 格子ボルツマン法の解き方(2D は D2Q9、3D は D3Q19)。衝突は 2 つの緩和時間(TRT)に Smagorinsky の渦粘性、
//! 壁は Bouzidi の補間つき跳ね返し(壁までの距離が無ければ半分の所の跳ね返し)、入口・出口・横は
//! 非平衡の外挿、壁が受ける力は運動量交換で出す。
//!
//! 値はすべて格子の単位(セルの大きさ 1、時間の刻み 1、密度 1 のまわり)。流れはいつも +x に向け、
//! 入口は x = 0 の面、出口は x の最後の面。分布は 2 組を持ち、引いて動かしてから衝突させる。

/// 速度の組。
#[derive(Debug)]
pub struct Velocities {
    pub c: &'static [[i32; 3]],
    pub w: &'static [f32],
    pub opp: &'static [usize],
}

pub const D2Q9: Velocities = Velocities {
    c: &[
        [0, 0, 0],
        [1, 0, 0],
        [-1, 0, 0],
        [0, 1, 0],
        [0, -1, 0],
        [1, 1, 0],
        [-1, -1, 0],
        [1, -1, 0],
        [-1, 1, 0],
    ],
    w: &[
        4.0 / 9.0,
        1.0 / 9.0,
        1.0 / 9.0,
        1.0 / 9.0,
        1.0 / 9.0,
        1.0 / 36.0,
        1.0 / 36.0,
        1.0 / 36.0,
        1.0 / 36.0,
    ],
    opp: &[0, 2, 1, 4, 3, 6, 5, 8, 7],
};

pub const D3Q19: Velocities = Velocities {
    c: &[
        [0, 0, 0],
        [1, 0, 0],
        [-1, 0, 0],
        [0, 1, 0],
        [0, -1, 0],
        [0, 0, 1],
        [0, 0, -1],
        [1, 1, 0],
        [-1, -1, 0],
        [1, -1, 0],
        [-1, 1, 0],
        [1, 0, 1],
        [-1, 0, -1],
        [1, 0, -1],
        [-1, 0, 1],
        [0, 1, 1],
        [0, -1, -1],
        [0, 1, -1],
        [0, -1, 1],
    ],
    w: &[
        1.0 / 3.0,
        1.0 / 18.0,
        1.0 / 18.0,
        1.0 / 18.0,
        1.0 / 18.0,
        1.0 / 18.0,
        1.0 / 18.0,
        1.0 / 36.0,
        1.0 / 36.0,
        1.0 / 36.0,
        1.0 / 36.0,
        1.0 / 36.0,
        1.0 / 36.0,
        1.0 / 36.0,
        1.0 / 36.0,
        1.0 / 36.0,
        1.0 / 36.0,
        1.0 / 36.0,
        1.0 / 36.0,
    ],
    opp: &[
        0, 2, 1, 4, 3, 6, 5, 8, 7, 10, 9, 12, 11, 14, 13, 16, 15, 18, 17,
    ],
};

/// TRT の魔法の数(壁の位置が粘性によらない)。
const MAGIC: f32 = 3.0 / 16.0;
/// 差の分の緩和時間の上限。粘性が小さい(τ が 0.5 に近い)と Λ を保つ τ- が大きくなりすぎ、差の分が
/// ほとんど緩まずに育って発散する(Schäfer–Turek 2D-2 の τ 0.524 で τ- 8.3、3000 刻みほど。3D では
/// 入口の内側の速さが入口の 4 倍に出る)。上限 4 なら τ 0.55 までは Λ = 3/16 のまま
const TAU_MINUS_MAX: f32 = 4.0;

/// セルの種類。
pub mod cell {
    pub const FLUID: u8 = 0;
    /// 力を数えない壁(管の壁など)
    pub const WALL: u8 = 1;
    /// 力を数える物(調べるボディ)
    pub const BODY: u8 = 2;
    /// 入口(速さを決める)
    pub const INLET: u8 = 3;
    /// 出口(密度 1、速さは内側と同じ)
    pub const OUTLET: u8 = 4;
    /// 遠くの流れ(横の面。速さを決め、密度は内側と同じ)
    pub const FAR: u8 = 5;
}

pub fn is_solid(t: u8) -> bool {
    t == cell::WALL || t == cell::BODY
}

/// 解く問題。
pub struct Setup {
    pub v: &'static Velocities,
    /// セルの数(2D は n[2] = 1)
    pub n: [usize; 3],
    /// セルの種類(`i + n0 * (j + n1 * k)` の順)
    pub cell: Vec<u8>,
    /// 入口と遠くの流れの速さ(セルごと。空なら `u_in`)
    pub u_cell: Vec<[f32; 3]>,
    pub u_in: [f32; 3],
    /// 格子の動粘性係数(τ = 3ν + 0.5)
    pub nu: f32,
    /// Smagorinsky の定数(0 なら使わない)
    pub smag: f32,
    /// 壁までの距離(セル × 方向。NaN は半分の所)。空なら全部半分の所
    pub q: Vec<f32>,
    /// y・z の向きに周期でつなぐか(つながない向きの端は、種類を決めたセルにする)
    pub periodic: [bool; 3],
    /// 体積力(管の流れの試験に使う。圧力の勾配の代わり)
    pub force: [f32; 3],
    /// x の列ごとに足す動粘性係数(出口の前の吸い込みの帯。渦を出口の前で弱め、出口で跳ね返さない)。
    /// 空なら足さない
    pub sponge: Vec<f32>,
    /// 入口と遠くの流れの速さを、止まった所からこの刻み数をかけて滑らかに上げる(0 なら初めから
    /// 全部の速さ)。一度に風を当てると強い圧力の波(音)が出て、箱の中を往復して残るため
    pub ramp: usize,
    /// 出口を音の波が抜ける形にする(出口の密度を外へ出ていく波の式 ∂ρ/∂t + c ∂ρ/∂x = 0 で進め、
    /// 1 へゆっくり戻す)。密度を 1 に固定すると波が跳ね返り、入口との間を往復して消えない
    pub open_outlet: bool,
}

impl Setup {
    pub fn new(v: &'static Velocities, n: [usize; 3]) -> Self {
        Setup {
            v,
            n,
            cell: vec![cell::FLUID; n[0] * n[1] * n[2]],
            u_cell: Vec::new(),
            u_in: [0.0; 3],
            nu: 0.1,
            smag: 0.0,
            q: Vec::new(),
            periodic: [false; 3],
            force: [0.0; 3],
            sponge: Vec::new(),
            ramp: 0,
            open_outlet: false,
        }
    }

    pub fn index(&self, i: usize, j: usize, k: usize) -> usize {
        i + self.n[0] * (j + self.n[1] * k)
    }

    pub fn count(&self) -> usize {
        self.cell.len()
    }
}

/// 境界のセル(非平衡の外挿で決める)。
#[derive(Clone, Copy)]
struct Edge {
    cell: u32,
    /// 内側の隣
    inner: u32,
    kind: u8,
}

/// 刻みごとの並列の計算で、糸 1 本に持たせるセルの数の下限。
const MIN_CELLS_PER_THREAD: usize = 4096;

/// 解いている途中の状態。
pub struct Solver {
    pub setup: Setup,
    q_count: usize,
    a: Vec<f32>,
    b: Vec<f32>,
    edges: Vec<Edge>,
    /// 出口のセルの密度(`open_outlet` の時、辺の順)
    out_rho: Vec<f32>,
    /// 方向ごとの、隣のセルの番号の差
    offsets: Vec<isize>,
    /// 進めた刻みの数
    pub steps: usize,
    /// 刻みごとのボディの力(格子の単位)
    pub forces: Vec<[f64; 3]>,
    threads: usize,
}

/// 平衡の分布。
#[inline]
fn feq(v: &Velocities, i: usize, rho: f32, u: [f32; 3]) -> f32 {
    let c = v.c[i];
    let cu = c[0] as f32 * u[0] + c[1] as f32 * u[1] + c[2] as f32 * u[2];
    let uu = u[0] * u[0] + u[1] * u[1] + u[2] * u[2];
    v.w[i] * rho * (1.0 + 3.0 * cu + 4.5 * cu * cu - 1.5 * uu)
}

/// 分布から密度と速さ。
#[inline]
fn moments(v: &Velocities, f: &[f32]) -> (f32, [f32; 3]) {
    let mut rho = 0.0;
    let mut m = [0.0f32; 3];
    for (i, fi) in f.iter().enumerate() {
        rho += fi;
        let c = v.c[i];
        m[0] += c[0] as f32 * fi;
        m[1] += c[1] as f32 * fi;
        m[2] += c[2] as f32 * fi;
    }
    let r = rho.max(1e-6);
    (rho, [m[0] / r, m[1] / r, m[2] / r])
}

impl Solver {
    pub fn new(setup: Setup) -> Self {
        let q = setup.v.c.len();
        let n = setup.count();
        let mut a = vec![0.0; n * q];
        for x in 0..n {
            let f = setup.ramp_at(0);
            let u = if is_solid(setup.cell[x]) {
                [0.0; 3]
            } else {
                setup.u_at(x).map(|c| c * f)
            };
            for i in 0..q {
                a[x * q + i] = feq(setup.v, i, 1.0, u);
            }
        }
        let b = a.clone();
        let mut edges = Vec::new();
        let [n0, n1, n2] = setup.n;
        for k in 0..n2 {
            for j in 0..n1 {
                for i in 0..n0 {
                    let x = setup.index(i, j, k);
                    let t = setup.cell[x];
                    match t {
                        cell::INLET | cell::OUTLET | cell::FAR => {
                            // 内側の隣: 入口は +x、出口は -x、横は面の内側へ
                            let mut p = [i as isize, j as isize, k as isize];
                            let lim = [n0 as isize, n1 as isize, n2 as isize];
                            if t == cell::INLET {
                                p[0] += 1;
                            } else if t == cell::OUTLET {
                                p[0] -= 1;
                            } else {
                                for (a, l) in lim.iter().enumerate().skip(1) {
                                    if *l > 1 {
                                        if p[a] == 0 {
                                            p[a] += 1;
                                        } else if p[a] == l - 1 {
                                            p[a] -= 1;
                                        }
                                    }
                                }
                                if p[0] == 0 {
                                    p[0] += 1;
                                } else if p[0] == lim[0] - 1 {
                                    p[0] -= 1;
                                }
                            }
                            let inner = setup.index(p[0] as usize, p[1] as usize, p[2] as usize);
                            edges.push(Edge {
                                cell: x as u32,
                                inner: inner as u32,
                                kind: t,
                            });
                        }
                        _ => {}
                    }
                }
            }
        }
        let threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .clamp(1, 16);
        let (s0, s1) = (setup.n[0] as isize, (setup.n[0] * setup.n[1]) as isize);
        let offsets = setup
            .v
            .c
            .iter()
            .map(|c| c[0] as isize + c[1] as isize * s0 + c[2] as isize * s1)
            .collect();
        Solver {
            offsets,
            q_count: q,
            a,
            b,
            out_rho: vec![1.0; edges.len()],
            edges,
            steps: 0,
            forces: Vec::new(),
            threads,
            setup,
        }
    }

    /// `n` 刻み進める。
    pub fn run(&mut self, n: usize) {
        for _ in 0..n {
            self.step();
        }
    }

    /// 1 刻み: 流体のセルを引いて動かして衝突させ(並列)、境界のセルを外挿で決める。
    pub fn step(&mut self) {
        let q = self.q_count;
        let s = &self.setup;
        let a = &self.a;
        let n = s.count();
        // 糸は刻みごとに作るので、1 本の受け持ちが少ないと作る手間が計算を上回る(小さな格子で
        // 数万刻み回すと、計算より糸を作る時間の方が長い)。1 本に MIN_CELLS_PER_THREAD 以上を持たせる
        let threads = self.threads.min(n.div_ceil(MIN_CELLS_PER_THREAD)).max(1);
        let chunk = n.div_ceil(threads).max(1);
        let off = &self.offsets;
        // 方向の数を型に持たせ、方向ごとの繰り返しを開いて速さの組を定数にする
        let work = move |ci: usize, out: &mut [f32]| {
            if q == D2Q9.c.len() {
                sweep::<9>(s, off, a, ci * chunk, out)
            } else {
                sweep::<19>(s, off, a, ci * chunk, out)
            }
        };
        // セルを続いた範囲に分け、範囲ごとに書く先を分けて並列に計算する
        let parts: Vec<[f64; 3]> = if threads == 1 {
            vec![work(0, &mut self.b[..])]
        } else {
            std::thread::scope(|sc| {
                let hs: Vec<_> = self
                    .b
                    .chunks_mut(chunk * q)
                    .enumerate()
                    .map(|(ci, out)| sc.spawn(move || work(ci, out)))
                    .collect();
                hs.into_iter()
                    .map(|h| h.join().expect("計算の糸"))
                    .collect()
            })
        };
        let mut total = [0.0; 3];
        for p in parts {
            for a in 0..3 {
                total[a] += p[a];
            }
        }
        // 境界のセル: 内側の隣(衝突の後)から非平衡の外挿
        let v = s.v;
        let f = s.ramp_at(self.steps + 1);
        // 外へ出ていく波の速さ(音速 + 流れ、格子の単位)と、1 へ戻す強さ
        let (wave, back) = (1.0 / 3f32.sqrt() + s.u_in[0], 0.002);
        for (n, e) in self.edges.iter().enumerate() {
            let inner = e.inner as usize;
            let fi = &self.b[inner * q..inner * q + q];
            let (rho_i, u_i) = moments(v, fi);
            let (rho, u) = match e.kind {
                cell::OUTLET if s.open_outlet => {
                    let r = &mut self.out_rho[n];
                    *r -= wave * (*r - rho_i) + back * (*r - 1.0);
                    (*r, u_i)
                }
                cell::OUTLET => (1.0, u_i),
                _ => (rho_i, s.u_at(e.cell as usize).map(|c| c * f)),
            };
            let x = e.cell as usize;
            for i in 0..q {
                let neq = self.b[inner * q + i] - feq(v, i, rho_i, u_i);
                self.b[x * q + i] = feq(v, i, rho, u) + neq;
            }
        }
        std::mem::swap(&mut self.a, &mut self.b);
        self.forces.push(total);
        self.steps += 1;
    }

    /// セルの密度と速さ(固い所は密度 1・速さ 0)。
    pub fn macro_at(&self, x: usize) -> (f32, [f32; 3]) {
        if is_solid(self.setup.cell[x]) {
            return (1.0, [0.0; 3]);
        }
        let q = self.q_count;
        moments(self.setup.v, &self.a[x * q..x * q + q])
    }

    /// 最後の `n` 刻みの力の平均。
    pub fn mean_force(&self, n: usize) -> [f64; 3] {
        let n = n.clamp(1, self.forces.len().max(1));
        let tail = &self.forces[self.forces.len().saturating_sub(n)..];
        let mut m = [0.0; 3];
        for f in tail {
            for a in 0..3 {
                m[a] += f[a];
            }
        }
        m.map(|v| v / tail.len().max(1) as f64)
    }

    /// 分布に壊れた値(NaN・無限)があるか。
    pub fn blew_up(&self) -> bool {
        self.a.iter().step_by(97).any(|v| !v.is_finite())
    }
}

impl Setup {
    /// 刻み `step` での入口の速さの割合(0〜1、cos で滑らかに上げる)。
    fn ramp_at(&self, step: usize) -> f32 {
        if self.ramp == 0 || step >= self.ramp {
            return 1.0;
        }
        let t = step as f32 / self.ramp as f32;
        0.5 - 0.5 * (std::f32::consts::PI * t).cos()
    }

    fn u_at(&self, x: usize) -> [f32; 3] {
        self.u_cell.get(x).copied().unwrap_or(self.u_in)
    }

    /// セル x から方向 c へ 1 つ進んだセル(周期でつなぐ)。外なら None。
    #[inline]
    fn neighbor(&self, x: usize, c: [i32; 3], sign: i32) -> Option<usize> {
        let n = self.n;
        let mut p = [x % n[0], (x / n[0]) % n[1], x / (n[0] * n[1])];
        for a in 0..3 {
            let d = c[a] * sign;
            if d == 0 {
                continue;
            }
            let v = p[a] as i64 + d as i64;
            let l = n[a] as i64;
            p[a] = if v < 0 || v >= l {
                if self.periodic[a] {
                    v.rem_euclid(l) as usize
                } else {
                    return None;
                }
            } else {
                v as usize
            };
        }
        Some(p[0] + n[0] * (p[1] + n[1] * p[2]))
    }
}

/// 番号 `x0` から続くセルの流体のセルを引いて動かし、衝突させて `out` に入れる。ボディが受ける
/// 力の和を返す。`Q` は方向の数(9 なら D2Q9、19 なら D3Q19)。
fn sweep<const Q: usize>(
    s: &Setup,
    off: &[isize],
    a: &[f32],
    x0: usize,
    out: &mut [f32],
) -> [f64; 3] {
    let mut force = [0.0f64; 3];
    let mut f = [0.0f32; Q];
    for (o, x) in out.as_chunks_mut::<Q>().0.iter_mut().zip(x0..) {
        if s.cell[x] != cell::FLUID {
            continue;
        }
        collide_cell::<Q>(s, off, a, x, &mut f, &mut force);
        *o = f;
    }
    force
}

/// 流体のセル x を引いて動かし、衝突させた分布を `f` に入れる。壁で跳ね返る分の力を `force` に足す。
#[inline(always)]
fn collide_cell<const Q: usize>(
    s: &Setup,
    off: &[isize],
    a: &[f32],
    x: usize,
    f: &mut [f32; Q],
    force: &mut [f64; 3],
) {
    let v: &Velocities = if Q == 9 { &D2Q9 } else { &D3Q19 };
    let q = Q;
    // 端から離れたセルは、隣を番号の差で引く(端のセルだけ周期と外を調べる)
    let n = s.n;
    let (ci, cj, ck) = (x % n[0], (x / n[0]) % n[1], x / (n[0] * n[1]));
    let inner = |p: usize, l: usize| l == 1 || (p >= 1 && p + 1 < l);
    let interior = inner(ci, n[0]) && inner(cj, n[1]) && inner(ck, n[2]);
    for i in 0..q {
        let c = v.c[i];
        // 方向 i の分は、x - c_i から来る
        let src = if interior {
            Some((x as isize - off[i]) as usize)
        } else {
            s.neighbor(x, c, -1)
        };
        let solid = match src {
            Some(y) => is_solid(s.cell[y]),
            None => true,
        };
        if !solid {
            let y = src.expect("流体の隣");
            f[i] = a[y * q + i];
            continue;
        }
        // 壁へ向かう方向 k = opp(i) の分が跳ね返ってくる
        let k = v.opp[i];
        let fk = a[x * q + k];
        let qd = s.q.get(x * q + k).copied().unwrap_or(f32::NAN);
        let val = if qd.is_nan() || !(0.0..=1.0).contains(&qd) {
            fk
        } else if qd < 0.5 {
            // x - c_k(壁と反対の隣)が流体なら、その分と混ぜる
            match s.neighbor(x, v.c[k], -1) {
                Some(y) if s.cell[y] == cell::FLUID => {
                    2.0 * qd * fk + (1.0 - 2.0 * qd) * a[y * q + k]
                }
                _ => fk,
            }
        } else {
            let fi = a[x * q + i];
            fk / (2.0 * qd) + (2.0 * qd - 1.0) / (2.0 * qd) * fi
        };
        f[i] = val;
        if let Some(y) = src
            && s.cell[y] == cell::BODY
        {
            let ck = v.c[k];
            let m = (fk + val) as f64;
            force[0] += ck[0] as f64 * m;
            force[1] += ck[1] as f64 * m;
            force[2] += ck[2] as f64 * m;
        }
    }
    // 衝突(TRT + Smagorinsky、体積力は速さをずらす簡単な方法)
    let (rho, mut u) = moments(v, f);
    let extra = s.sponge.get(x % s.n[0]).copied().unwrap_or(0.0);
    let tau0 = 3.0 * (s.nu + extra) + 0.5;
    // 運動量は差の分(緩和時間 τ-)で緩むので、ずらす量は τ- に合わせる
    let tau_minus = MAGIC / (tau0 - 0.5) + 0.5;
    for (ua, fa) in u.iter_mut().zip(s.force) {
        *ua += fa * tau_minus / rho.max(1e-6);
    }
    let mut eq = [0.0f32; Q];
    for (i, e) in eq.iter_mut().enumerate() {
        *e = feq(v, i, rho, u);
    }
    let tau = if s.smag > 0.0 {
        let mut pi = [0.0f32; 6];
        for (i, (fi, e)) in f.iter().zip(&eq).enumerate() {
            let c = v.c[i].map(|x| x as f32);
            let ne = fi - e;
            pi[0] += c[0] * c[0] * ne;
            pi[1] += c[1] * c[1] * ne;
            pi[2] += c[2] * c[2] * ne;
            pi[3] += c[0] * c[1] * ne;
            pi[4] += c[0] * c[2] * ne;
            pi[5] += c[1] * c[2] * ne;
        }
        let qq = (pi[0] * pi[0]
            + pi[1] * pi[1]
            + pi[2] * pi[2]
            + 2.0 * (pi[3] * pi[3] + pi[4] * pi[4] + pi[5] * pi[5]))
            .sqrt();
        let c2 = s.smag * s.smag;
        0.5 * (tau0 + (tau0 * tau0 + 18.0 * std::f32::consts::SQRT_2 * c2 * qq / rho).sqrt())
    } else {
        tau0
    };
    // 2 つの緩和時間(TRT): 対の和の分を粘性の τ で、差の分を魔法の数 Λ = 3/16 になる τ で
    // 緩める(跳ね返しの壁の位置が粘性で動かない。BGK だと τ が大きい時に抗力が数 % 高く出る)
    let wp = 1.0 / tau;
    let wm = 1.0 / (MAGIC / (tau - 0.5) + 0.5).min(TAU_MINUS_MAX);
    let f0 = f[0];
    f[0] = f0 + wp * (eq[0] - f0);
    for i in 1..q {
        let j = v.opp[i];
        if j < i {
            continue;
        }
        let (fi, fj) = (f[i], f[j]);
        let (sp, sm) = (0.5 * (fi + fj), 0.5 * (fi - fj));
        let (ep, em) = (0.5 * (eq[i] + eq[j]), 0.5 * (eq[i] - eq[j]));
        let dp = wp * (ep - sp);
        let dm = wm * (em - sm);
        f[i] = fi + dp + dm;
        f[j] = fj + dp - dm;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn velocity_sets_are_consistent() {
        for v in [&D2Q9, &D3Q19] {
            let ws: f32 = v.w.iter().sum();
            assert!((ws - 1.0).abs() < 1e-6);
            for (i, c) in v.c.iter().enumerate() {
                let o = v.c[v.opp[i]];
                assert_eq!([-c[0], -c[1], -c[2]], o);
            }
            // 2 次のモーメント: Σ w c_a c_b = δ_ab / 3
            for a in 0..3 {
                let s: f32 =
                    v.c.iter()
                        .zip(v.w)
                        .map(|(c, w)| w * (c[a] * c[a]) as f32)
                        .sum();
                let want = if std::ptr::eq(v, &D2Q9) && a == 2 {
                    0.0
                } else {
                    1.0 / 3.0
                };
                assert!((s - want).abs() < 1e-6);
            }
        }
    }
}
