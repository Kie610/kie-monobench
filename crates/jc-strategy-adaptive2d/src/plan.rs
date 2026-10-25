//! 負荷制御粗取りの、平面の上の経路の計画。
//!
//! 削った所を格子で持ち、工具を少しずつ進める。1 歩ごとに、新たに削る面積が
//! 「横の切り込み × 歩幅」に合うように進む向きを選ぶ。材料を右に見て進む
//! (主軸が時計回りの時のダウンカット)。削れる材料がなくなればその線を終え、
//! 削った所の縁で今の位置に最も近い所から次の線を始める。削った所の中を通って
//! 行けるなら工具を下げたまま移る。

use jc_cam_geom::{Area, P2, offset_area};

/// 計画の条件。長さは mm。
#[derive(Debug, Clone, Copy)]
pub struct Params {
    /// 工具の半径
    pub radius: f64,
    /// 横の切り込み(保つ負荷)
    pub load: f64,
    /// 格子の大きさ(0 なら工具の半径から決める)
    pub cell: f64,
    /// ストックの範囲(左下と右上)。この外には材料がない。開いた面では、工具が外から入れる
    pub stock: Option<(P2, P2)>,
    /// 最初に入る所の目安(利用者が置いた入る位置)。そこに最も近い、入れる所から入る
    pub entry: Option<P2>,
    /// らせんの半径の上限(0 なら工具の半径の 0.6 倍)
    pub helix: f64,
    /// らせんを使わず真下に入る
    pub plunge: bool,
    /// 開いた面で、材料の外から入れるならそこから入る
    pub outside: bool,
}

impl Params {
    /// 半径・負荷・格子・ストックだけを決めた条件(入り方は既定)。
    pub fn new(radius: f64, load: f64, cell: f64, stock: Option<(P2, P2)>) -> Params {
        Params {
            radius,
            load,
            cell,
            stock,
            entry: None,
            helix: 0.0,
            plunge: false,
            outside: true,
        }
    }
}

/// 経路の 1 区切り。
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// らせんで入る。`radius` が 0 なら真下に入る。入った後は円の右端(中心 + (半径, 0))にいる
    Entry { center: P2, radius: f64 },
    /// 削る線。`link` があれば、前の位置からその折れ線(削った所の中)を下げたまま移る。
    /// なければ上げて移る
    Cut {
        link: Option<Vec<P2>>,
        points: Vec<P2>,
    },
}

/// 計画の結果。
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub steps: Vec<Step>,
    /// 工具が届くのに削り残した面積(mm²)。壁の 1 周で削れる帯の中は数えない
    pub left: f64,
    /// 1 歩で削った面積の、目標に対する最大の比(入りのらせんは除く)
    pub max_ratio: f64,
    /// 目標の 1.5 倍を超えた歩の割合
    pub overloaded: f64,
    /// 格子の大きさ
    pub cell: f64,
}

/// 格子の数の上限。超える時は格子を粗くする。
const MAX_CELLS: f64 = 3.0e6;
/// 1 歩で向きを変えられる最大の角度
const TURN: f64 = 75.0 * std::f64::consts::PI / 180.0;
const FAR: f64 = 1e20;
/// 向きを試す数(右の端から左の端まで)
const SAMPLES: usize = 10;

struct Grid {
    x0: f64,
    y0: f64,
    h: f64,
    nx: usize,
    ny: usize,
    /// まだ削っていない材料(格子の中心で判定)
    mat: Vec<bool>,
    /// 格子のうち材料が残っている割合(0〜1)。工具の円の縁を格子より細かく扱い、
    /// 1 歩で削る面積を数える時の誤差を減らす。`mat` は割合が半分より大きい所
    cov: Vec<f32>,
    /// 工具の中心が入ってよい
    ok: Vec<bool>,
}

impl Grid {
    fn center(&self, i: usize, j: usize) -> P2 {
        [
            self.x0 + (i as f64 + 0.5) * self.h,
            self.y0 + (j as f64 + 0.5) * self.h,
        ]
    }

    fn cell(&self, p: P2) -> Option<(usize, usize)> {
        let i = ((p[0] - self.x0) / self.h).floor();
        let j = ((p[1] - self.y0) / self.h).floor();
        (i >= 0.0 && j >= 0.0 && (i as usize) < self.nx && (j as usize) < self.ny)
            .then_some((i as usize, j as usize))
    }

    fn allowed(&self, p: P2) -> bool {
        self.cell(p).is_some_and(|(i, j)| self.ok[j * self.nx + i])
    }

    /// 中心 `p`、半径 `r` の円に入る格子の範囲。
    fn range(&self, p: P2, r: f64) -> (usize, usize, usize, usize) {
        let lo = |v: f64, o: f64| (((v - r - o) / self.h).floor().max(0.0)) as usize;
        let hi = |v: f64, o: f64, n: usize| ((((v + r - o) / self.h).ceil()) as usize).min(n);
        (
            lo(p[0], self.x0),
            hi(p[0], self.x0, self.nx),
            lo(p[1], self.y0),
            hi(p[1], self.y0, self.ny),
        )
    }

    /// 円の中の格子の番号を順に渡す(中心で判定)。
    #[cfg(test)]
    fn each_in(&self, p: P2, r: f64, mut f: impl FnMut(usize)) {
        let (i0, i1, j0, j1) = self.range(p, r);
        let r2 = r * r;
        for j in j0..j1 {
            let dy = self.y0 + (j as f64 + 0.5) * self.h - p[1];
            let dy2 = dy * dy;
            if dy2 > r2 {
                continue;
            }
            for i in i0..i1 {
                let dx = self.x0 + (i as f64 + 0.5) * self.h - p[0];
                if dx * dx + dy2 <= r2 {
                    f(j * self.nx + i);
                }
            }
        }
    }

    /// 円の縁にかかる格子も含めて、格子の番号と、格子のうち円の中にある割合を渡す。
    /// 割合は、縁を格子の中でまっすぐとみなした近似(縁が格子の辺に平行なら正確)。
    fn each_part(&self, p: P2, r: f64, mut f: impl FnMut(usize, f32)) {
        let (i0, i1, j0, j1) = self.range(p, r + self.h);
        for j in j0..j1 {
            let dy = self.y0 + (j as f64 + 0.5) * self.h - p[1];
            for i in i0..i1 {
                let dx = self.x0 + (i as f64 + 0.5) * self.h - p[0];
                let inside = ((r - dx.hypot(dy)) / self.h + 0.5).clamp(0.0, 1.0);
                if inside > 0.0 {
                    f(j * self.nx + i, inside as f32);
                }
            }
        }
    }

    /// 円の中の材料の面積(格子の数)。
    fn count(&self, p: P2, r: f64) -> f64 {
        let mut n = 0.0f64;
        self.each_part(p, r, |k, inside| n += f64::from(self.cov[k].min(inside)));
        n
    }

    /// 円の中の材料を削る。
    fn cut(&mut self, p: P2, r: f64) {
        let mut hit = Vec::new();
        self.each_part(p, r, |k, inside| hit.push((k, inside)));
        for (k, inside) in hit {
            let c = self.cov[k].min(1.0 - inside);
            self.cov[k] = c;
            self.mat[k] = c > 0.5;
        }
    }
}

/// 領域の内側を格子に塗る(走査線。外周と穴の偶奇で塗る)。
fn raster(areas: &[Area], g: &Grid) -> Vec<bool> {
    let mut edges: Vec<(P2, P2)> = Vec::new();
    for a in areas {
        for l in std::iter::once(&a.outer).chain(&a.holes) {
            let pts = l.flatten(g.h / 4.0);
            let n = pts.len();
            for k in 0..n {
                edges.push((pts[k], pts[(k + 1) % n]));
            }
        }
    }
    let mut out = vec![false; g.nx * g.ny];
    let mut xs: Vec<f64> = Vec::new();
    for j in 0..g.ny {
        let y = g.y0 + (j as f64 + 0.5) * g.h;
        xs.clear();
        for (a, b) in &edges {
            if (a[1] <= y) != (b[1] <= y) {
                xs.push(a[0] + (y - a[1]) / (b[1] - a[1]) * (b[0] - a[0]));
            }
        }
        xs.sort_by(f64::total_cmp);
        for pair in xs.as_chunks::<2>().0 {
            let i0 = ((pair[0] - g.x0) / g.h - 0.5).ceil().max(0.0) as usize;
            let i1 = (((pair[1] - g.x0) / g.h - 0.5).floor() + 1.0).max(0.0) as usize;
            for i in i0..i1.min(g.nx) {
                out[j * g.nx + i] = true;
            }
        }
    }
    out
}

/// 1 次元の距離の 2 乗の変換(Felzenszwalb と Huttenlocher の方法)。
fn dt1(f: &[f64], d: &mut [f64], v: &mut [usize], z: &mut [f64]) {
    let n = f.len();
    if n == 0 {
        return;
    }
    let mut k = 0;
    v[0] = 0;
    z[0] = -FAR;
    z[1] = FAR;
    let meet = |q: usize, p: usize| {
        let (qf, pf) = (q as f64, p as f64);
        ((f[q] + qf * qf) - (f[p] + pf * pf)) / (2.0 * qf - 2.0 * pf)
    };
    for q in 1..n {
        let mut s = meet(q, v[k]);
        while s <= z[k] {
            k -= 1;
            s = meet(q, v[k]);
        }
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = FAR;
    }
    k = 0;
    for (q, out) in d.iter_mut().enumerate() {
        while z[k + 1] < q as f64 {
            k += 1;
        }
        let dq = q as f64 - v[k] as f64;
        *out = dq * dq + f[v[k]];
    }
}

/// 窓 `[i0, i1) × [j0, j1)` の中で、`src` が真の格子までの距離(格子の数)。窓の外は FAR。
fn distance(
    nx: usize,
    ny: usize,
    src: impl Fn(usize) -> bool,
    (i0, i1, j0, j1): (usize, usize, usize, usize),
) -> Vec<f64> {
    let (w, hgt) = (i1 - i0, j1 - j0);
    let mut out = vec![FAR; nx * ny];
    if w == 0 || hgt == 0 {
        return out;
    }
    let n = w.max(hgt);
    let (mut f, mut d) = (vec![0.0; n], vec![0.0; n]);
    let (mut v, mut z) = (vec![0usize; n], vec![0.0; n + 1]);
    let mut tmp = vec![FAR; w * hgt];
    // 列ごと
    for i in 0..w {
        for (j, fj) in f[..hgt].iter_mut().enumerate() {
            *fj = if src((j0 + j) * nx + i0 + i) {
                0.0
            } else {
                FAR
            };
        }
        dt1(&f[..hgt], &mut d[..hgt], &mut v, &mut z);
        for j in 0..hgt {
            tmp[j * w + i] = d[j];
        }
    }
    // 行ごと
    for j in 0..hgt {
        f[..w].copy_from_slice(&tmp[j * w..(j + 1) * w]);
        dt1(&f[..w], &mut d[..w], &mut v, &mut z);
        for i in 0..w {
            out[(j0 + j) * nx + i0 + i] = d[i].sqrt();
        }
    }
    out
}

/// 折れ線を許容差 `tol` で間引く(Douglas–Peucker)。
fn simplify(pts: &[P2], tol: f64) -> Vec<P2> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    keep[pts.len() - 1] = true;
    let mut stack = vec![(0, pts.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let (p, q) = (pts[a], pts[b]);
        let d = [q[0] - p[0], q[1] - p[1]];
        let len = d[0].hypot(d[1]);
        let mut worst = (0.0, a);
        for (k, x) in pts.iter().enumerate().take(b).skip(a + 1) {
            let e = if len < 1e-12 {
                (x[0] - p[0]).hypot(x[1] - p[1])
            } else {
                ((x[0] - p[0]) * d[1] - (x[1] - p[1]) * d[0]).abs() / len
            };
            if e > worst.0 {
                worst = (e, k);
            }
        }
        if worst.0 > tol {
            keep[worst.1] = true;
            stack.push((a, worst.1));
            stack.push((worst.1, b));
        }
    }
    pts.iter()
        .zip(&keep)
        .filter(|(_, k)| **k)
        .map(|(p, _)| *p)
        .collect()
}

/// `p` のまわり半径 `s` の格子に印を付ける。
fn block(g: &Grid, blocked: &mut [bool], p: P2, s: f64) {
    let (i0, i1, j0, j1) = g.range(p, s);
    for j in j0..j1 {
        for i in i0..i1 {
            blocked[j * g.nx + i] = true;
        }
    }
}

/// `from` から、`safe` な格子だけを通って最初に着く `goal` の格子と、そこまでの折れ線
/// (`from` から格子の中心をたどる)。窓 `win` の中だけを探す。
/// `prev` は作業用(全部 u32::MAX で渡し、そのまま返す)。
fn walk(
    g: &Grid,
    from: P2,
    win: (usize, usize, usize, usize),
    goal: &dyn Fn(usize) -> bool,
    safe: &dyn Fn(usize) -> bool,
    prev: &mut [u32],
) -> Option<(usize, Vec<P2>)> {
    let (si, sj) = g.cell(from)?;
    let nx = g.nx;
    let s0 = sj * nx + si;
    let mut queue = std::collections::VecDeque::from([s0]);
    let mut seen = vec![s0];
    prev[s0] = s0 as u32;
    let mut hit = None;
    while let Some(k) = queue.pop_front() {
        if k != s0 && goal(k) {
            hit = Some(k);
            break;
        }
        let (i, j) = ((k % nx) as isize, (k / nx) as isize);
        for (di, dj) in [
            (1, 0),
            (-1, 0),
            (0, 1),
            (0, -1),
            (1, 1),
            (1, -1),
            (-1, 1),
            (-1, -1),
        ] {
            let (a, b) = (i + di, j + dj);
            if a < win.0 as isize
                || b < win.2 as isize
                || a >= win.1 as isize
                || b >= win.3 as isize
            {
                continue;
            }
            let n = b as usize * nx + a as usize;
            if prev[n] != u32::MAX || !(safe(n) || goal(n)) {
                continue;
            }
            prev[n] = k as u32;
            seen.push(n);
            queue.push_back(n);
        }
    }
    let path = hit.map(|end| {
        let mut cells = vec![end];
        let mut k = end;
        while k != s0 {
            k = prev[k] as usize;
            cells.push(k);
        }
        cells.reverse();
        let mut pts = vec![from];
        pts.extend(cells.iter().skip(1).map(|&k| g.center(k % nx, k / nx)));
        (end, pts)
    });
    for k in seen {
        prev[k] = u32::MAX;
    }
    path
}

/// 点 `p` から最も近い材料の格子の中心(半径 `rr` の中で)。
fn nearest_material(g: &Grid, p: P2, rr: f64) -> Option<P2> {
    let mut near: Option<(f64, P2)> = None;
    let (i0, i1, j0, j1) = g.range(p, rr);
    for j in j0..j1 {
        for i in i0..i1 {
            if g.mat[j * g.nx + i] {
                let c = g.center(i, j);
                let e = (c[0] - p[0]).hypot(c[1] - p[1]);
                if near.is_none_or(|b| e < b.0) {
                    near = Some((e, c));
                }
            }
        }
    }
    near.map(|n| n.1)
}

/// 候補の格子 `k` から、削った所と材料の境目を上流へたどった端の候補。
fn upstream(g: &Grid, cand: &dyn Fn(usize) -> bool, k: usize, r: f64, s: f64) -> usize {
    let first = g.center(k % g.nx, k / g.nx);
    let (mut p, mut cur) = (first, k);
    for n in 0..20_000 {
        let Some(m) = nearest_material(g, p, r + 4.0 * g.h) else {
            break;
        };
        let d = [m[0] - p[0], m[1] - p[1]];
        let len = d[0].hypot(d[1]).max(1e-12);
        // 進む向きは材料の向きを左へ 90° 回したもの。その逆へ戻る
        let back = [p[0] + d[1] / len * s, p[1] - d[0] / len * s];
        let mut best: Option<(f64, usize)> = None;
        let (i0, i1, j0, j1) = g.range(back, s * 0.6);
        for j in j0..j1 {
            for i in i0..i1 {
                let c = j * g.nx + i;
                if cand(c) {
                    let q = g.center(i, j);
                    let e = (q[0] - back[0]).hypot(q[1] - back[1]);
                    if best.is_none_or(|b| e < b.0) {
                        best = Some((e, c));
                    }
                }
            }
        }
        let Some((_, c)) = best else { break };
        let q = g.center(c % g.nx, c / g.nx);
        // 一周して戻った(閉じた境目): 最初の所から始める
        if n > 3 && (q[0] - first[0]).hypot(q[1] - first[1]) < s {
            return k;
        }
        p = q;
        cur = c;
    }
    cur
}

fn at(p: P2, th: f64, s: f64) -> P2 {
    [p[0] + s * th.cos(), p[1] + s * th.sin()]
}

/// 削る材料の領域 `material`(壁から残す厚さだけ内側)と、工具の中心が入ってよい領域
/// `allowed`(壁から工具の半径と残す厚さだけ内側)から、経路を計画する。
pub fn plan(material: &[Area], allowed: &[Area], prm: Params) -> Result<Plan, String> {
    plan_rest(material, &[], allowed, prm)
}

/// 残り削りの計画: `cleared`(前の工具で削った所)を除いた材料だけを削る。
/// 削った所の中から入れるので、らせんで入る所は少ない。
pub fn plan_rest(
    material: &[Area],
    cleared: &[Area],
    allowed: &[Area],
    prm: Params,
) -> Result<Plan, String> {
    let r = prm.radius;
    if r <= 0.0 || prm.load <= 0.0 {
        return Err("工具の半径と横の切り込みは 0 より大きくしてください".into());
    }
    if prm.load > 2.0 * r {
        return Err("横の切り込みが工具の径より大きいです".into());
    }
    if allowed.is_empty() {
        return Err("工具がポケットに入りません(工具が太すぎます)".into());
    }
    // 格子の大きさと範囲
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for a in material {
        let (l, h) = a.outer.bounds();
        for k in 0..2 {
            lo[k] = lo[k].min(l[k]);
            hi[k] = hi[k].max(h[k]);
        }
    }
    if lo[0] > hi[0] {
        return Err("削る所がありません".into());
    }
    let pad = r * 0.25;
    let (w, hgt) = (hi[0] - lo[0] + 2.0 * pad, hi[1] - lo[1] + 2.0 * pad);
    let mut h = if prm.cell > 0.0 { prm.cell } else { r / 20.0 };
    if w * hgt / (h * h) > MAX_CELLS {
        h = (w * hgt / MAX_CELLS).sqrt();
    }
    let mut g = Grid {
        x0: lo[0] - pad,
        y0: lo[1] - pad,
        h,
        nx: (w / h).ceil() as usize,
        ny: (hgt / h).ceil() as usize,
        mat: Vec::new(),
        cov: Vec::new(),
        ok: Vec::new(),
    };
    let (nx, ny) = (g.nx, g.ny);
    // 格子の中の点がどこにあっても領域に入るよう、格子の対角の半分だけ内側を使う
    let inner: Vec<Area> = allowed
        .iter()
        .flat_map(|a| offset_area(a, 0.75 * h))
        .collect();
    g.ok = raster(&inner, &g);
    if !g.ok.iter().any(|b| *b) {
        return Err("工具がポケットに入りません(工具が太すぎます)".into());
    }
    g.mat = raster(material, &g);
    if !cleared.is_empty() {
        let done = raster(cleared, &g);
        for (m, c) in g.mat.iter_mut().zip(done) {
            if c {
                *m = false;
            }
        }
    }
    if let Some((smin, smax)) = prm.stock {
        for k in 0..nx * ny {
            let c = g.center(k % nx, k / nx);
            if c[0] < smin[0] || c[0] > smax[0] || c[1] < smin[1] || c[1] > smax[1] {
                g.mat[k] = false;
            }
        }
    }
    // 工具が届かない材料(壁の際や、工具より細い隅)は数えない。最後の壁の 1 周と残り削りの受け持ち
    let full = (0, nx, 0, ny);
    let to_ok = distance(nx, ny, |k| g.ok[k], full);
    // 入ってよい所の縁に工具がぴったり来るとは限らないので、縁から格子 2 つ分の皮も壁の 1 周に任せる
    let reach = r / h;
    for (m, t) in g.mat.iter_mut().zip(&to_ok) {
        if *t > reach - 2.0 {
            *m = false;
        }
    }
    g.cov = g.mat.iter().map(|&m| if m { 1.0 } else { 0.0 }).collect();
    // 入れる広さ(工具の中心が入ってよい所の縁までの距離)
    let room = distance(nx, ny, |k| !g.ok[k], full);
    drop(to_ok);

    let s = (r / 3.0).max(2.0 * h);
    let target = prm.load * s / (h * h);
    let mut steps: Vec<Step> = Vec::new();
    let mut blocked = vec![false; nx * ny];
    let mut pos: Option<P2> = None;
    let mut ratios: Vec<f64> = Vec::new();
    // 削った所の範囲(格子)。次の線の始まりはこの中から探す
    let mut cut_lo = [usize::MAX; 2];
    let mut cut_hi = [0usize; 2];
    let grow = |c_lo: &mut [usize; 2], c_hi: &mut [usize; 2], g: &Grid, p: P2, rr: f64| {
        let (i0, i1, j0, j1) = g.range(p, rr);
        c_lo[0] = c_lo[0].min(i0);
        c_lo[1] = c_lo[1].min(j0);
        c_hi[0] = c_hi[0].max(i1);
        c_hi[1] = c_hi[1].max(j1);
    };
    let mut dist: Option<Vec<f64>> = None;
    let mut prev = vec![u32::MAX; nx * ny];
    let mut guard = 0;
    loop {
        guard += 1;
        if guard > 100_000 {
            return Err("経路の計画が終わりませんでした".into());
        }
        // 最初は、材料の外から入れる所(工具の円に材料がかからない所)があるか、全体で調べる
        if dist.is_none() && pos.is_none() && prm.outside {
            dist = Some(distance(nx, ny, |k| g.mat[k], (0, nx, 0, ny)));
        }
        // 材料までの距離(削った範囲の窓の中)
        if dist.is_none() && cut_lo[0] != usize::MAX {
            let m = (reach + 8.0) as usize;
            let win = (
                cut_lo[0].saturating_sub(m),
                (cut_hi[0] + m).min(nx),
                cut_lo[1].saturating_sub(m),
                (cut_hi[1] + m).min(ny),
            );
            dist = Some(distance(nx, ny, |k| g.mat[k], win));
        }
        // 次の線の始まり: 工具の円に材料がなく、すぐ外に材料がある所(候補)。
        // 削った所の中を通って行ける候補のうち最も近いものを選び、なければ直線で最も近いもの
        let found = match (&dist, pos) {
            (Some(d), Some(p)) => {
                let cand =
                    |k: usize| g.ok[k] && !blocked[k] && d[k] > reach + 0.5 && d[k] <= reach + 3.0;
                let safe = |k: usize| g.ok[k] && d[k] > reach + 0.5;
                let m = (reach + 8.0) as usize;
                let win = (
                    cut_lo[0].saturating_sub(m),
                    (cut_hi[0] + m).min(nx),
                    cut_lo[1].saturating_sub(m),
                    (cut_hi[1] + m).min(ny),
                );
                let near = walk(&g, p, win, &cand, &safe, &mut prev).map(|(k, _)| k);
                // 境目の途中から始めると線が細切れになるので、境目を上流(材料を右に見て進む向きの逆)へたどった端から始める
                let near = near.map(|k| upstream(&g, &cand, k, r, s));
                let linked = near.and_then(|k| {
                    walk(&g, p, win, &|n| n == k, &safe, &mut prev)
                        .map(|(k, path)| (k, Some(simplify(&path, h * 0.25))))
                });
                match linked.or(near.map(|k| (k, None))) {
                    Some(x) => Some(x),
                    None => {
                        let mut best: Option<(f64, usize)> = None;
                        for j in win.2..win.3 {
                            for i in win.0..win.1 {
                                let k = j * nx + i;
                                if cand(k) {
                                    let c = g.center(i, j);
                                    let e = (c[0] - p[0]).hypot(c[1] - p[1]);
                                    if best.is_none_or(|b| e < b.0) {
                                        best = Some((e, k));
                                    }
                                }
                            }
                        }
                        best.map(|(_, k)| (k, None))
                    }
                }
            }
            (Some(d), None) => {
                // 材料の外から入る(開いた面): すぐ外に材料がある所の 1 つから、上げたまま下ろす
                let cand =
                    |k: usize| g.ok[k] && !blocked[k] && d[k] > reach + 0.5 && d[k] <= reach + 3.0;
                (0..nx * ny)
                    .find(|&k| cand(k))
                    .map(|k| (upstream(&g, &cand, k, r, s), None))
            }
            _ => None,
        };
        let Some((k, link)) = found else {
            // 続けられる所がない: まだ材料のある所に、らせんで入る(いちばん広い所)
            let max_rh = if prm.helix > 0.0 { prm.helix } else { 0.6 * r };
            let rh_at = |clear: f64| {
                let rh = max_rh.min((clear - 1.0) * h);
                if prm.plunge || rh < 0.2 * r.min(max_rh / 0.6) {
                    0.0
                } else {
                    rh
                }
            };
            let mut entry: Option<(f64, usize)> = None;
            match prm.entry.filter(|_| steps.is_empty()) {
                // 置いた入る位置に最も近い、らせんが入る所(なければ材料のある最も近い所)
                Some(e) => {
                    let mut best: Option<(bool, f64, usize)> = None;
                    for (k, &rk) in room.iter().enumerate() {
                        if !(g.ok[k] && g.mat[k]) {
                            continue;
                        }
                        let c = g.center(k % nx, k / nx);
                        let d = (c[0] - e[0]).hypot(c[1] - e[1]);
                        let fits = prm.plunge || rh_at(rk) > 0.0;
                        if best.is_none_or(|b| (fits, -d) > (b.0, -b.1)) {
                            best = Some((fits, d, k));
                        }
                    }
                    entry = best.map(|(_, _, k)| (room[k], k));
                }
                None => {
                    for (k, &rk) in room.iter().enumerate() {
                        if g.ok[k] && g.mat[k] && entry.is_none_or(|e| rk > e.0) {
                            entry = Some((rk, k));
                        }
                    }
                }
            }
            let Some((clear, k)) = entry else { break };
            let c = g.center(k % nx, k / nx);
            let rh = rh_at(clear);
            g.cut(c, rh + r);
            grow(&mut cut_lo, &mut cut_hi, &g, c, rh + r);
            steps.push(Step::Entry {
                center: c,
                radius: rh,
            });
            pos = Some([c[0] + rh, c[1]]);
            dist = None;
            continue;
        };
        let start = g.center(k % nx, k / nx);
        // 最初の向き: 最も近い材料を右に見る
        let Some(m) = nearest_material(&g, start, r + 4.0 * h) else {
            continue;
        };
        let mut th = (m[1] - start[1]).atan2(m[0] - start[0]) + std::f64::consts::FRAC_PI_2;
        // 1 歩ずつ進む
        let mut p = start;
        let mut pts = vec![start];
        for _ in 0..1_000_000 {
            let eval = |t: f64| -> Option<f64> {
                let q = at(p, t, s);
                g.allowed(q).then(|| g.count(q, r))
            };
            // 右(材料の側)から左へ向きを並べ、目標以下になる最も右の向きを探す
            let samples: Vec<(f64, Option<f64>)> = (0..=SAMPLES)
                .map(|k| {
                    let t = th - TURN + 2.0 * TURN * k as f64 / SAMPLES as f64;
                    (t, eval(t))
                })
                .collect();
            let Some(first) = samples.iter().position(|x| x.1.is_some()) else {
                break;
            };
            let ar = samples[first].1.expect("入ってよい向き");
            if ar < 0.1 * target {
                break;
            }
            let (t, a) = if ar <= target {
                // 壁や削った所に沿う: 右いっぱいでも目標に届かない
                (samples[first].0, ar)
            } else {
                match (first + 1..samples.len())
                    .find(|&k| samples[k].1.is_some_and(|a| a <= target))
                {
                    Some(k) => {
                        // k - 1 と k の間を二分する(k - 1 が入れない向きなら多すぎとみなす)
                        let mut ok = (samples[k].0, samples[k].1.expect("ある"));
                        let mut over = (samples[k - 1].0, samples[k - 1].1.unwrap_or(f64::MAX));
                        for _ in 0..8 {
                            let tm = (ok.0 + over.0) / 2.0;
                            match eval(tm) {
                                Some(am) if am <= target => ok = (tm, am),
                                Some(am) => over = (tm, am),
                                None => over = (tm, f64::MAX),
                            }
                        }
                        if over.1 < f64::MAX && (over.1 - target).abs() < (target - ok.1).abs() {
                            over
                        } else {
                            ok
                        }
                    }
                    // どの向きも多すぎる: いちばん少ない向き
                    None => samples
                        .iter()
                        .filter_map(|(t, a)| a.map(|a| (*t, a)))
                        .min_by(|x, y| x.1.total_cmp(&y.1))
                        .expect("入ってよい向きがある"),
                }
            };
            ratios.push(a / target);
            let q = at(p, t, s);
            g.cut(q, r);
            grow(&mut cut_lo, &mut cut_hi, &g, q, r);
            pts.push(q);
            p = q;
            th = t;
        }
        if pts.len() < 2 {
            // 進めなかった: 同じ所から何度も始めないよう印を付ける。材料は変わらないので距離はそのまま使える
            block(&g, &mut blocked, start, s);
            continue;
        }
        steps.push(Step::Cut {
            link,
            points: simplify(&pts, h * 0.25),
        });
        pos = Some(p);
        dist = None;
    }
    // 壁の 1 周(工具の中心が入ってよい所の縁をたどる)が削る帯の外に残った面積
    let left = (0..nx * ny)
        .filter(|&k| g.mat[k] && room[k] > reach + 1.0)
        .count() as f64
        * h
        * h;
    let max_ratio = ratios.iter().copied().fold(0.0, f64::max);
    let overloaded = if ratios.is_empty() {
        0.0
    } else {
        ratios.iter().filter(|x| **x > 1.5).count() as f64 / ratios.len() as f64
    };
    Ok(Plan {
        steps,
        left,
        max_ratio,
        overloaded,
        cell: h,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use jc_cam_geom::Loop;

    fn square(x0: f64, y0: f64, x1: f64, y1: f64) -> Loop {
        Loop::from_points(&[[x0, y0], [x1, y0], [x1, y1], [x0, y1]])
    }

    fn entries(p: &Plan) -> usize {
        p.steps
            .iter()
            .filter(|s| matches!(s, Step::Entry { .. }))
            .count()
    }

    fn plan_for(area: &Area, r: f64, load: f64) -> Plan {
        let material = offset_area(area, 0.0);
        let allowed = offset_area(area, r);
        plan(&material, &allowed, Params::new(r, load, 0.0, None)).unwrap()
    }

    #[test]
    fn distance_transform_is_exact_on_the_grid() {
        let (nx, ny) = (7, 5);
        let d = distance(nx, ny, |k| k == 2 * nx + 1, (0, nx, 0, ny));
        assert_eq!(d[2 * nx + 1], 0.0);
        assert!((d[2 * nx + 4] - 3.0).abs() < 1e-9);
        assert!((d[4 * nx + 3] - 8f64.sqrt()).abs() < 1e-9);
    }

    #[test]
    fn a_square_pocket_is_cleared_with_a_steady_load() {
        let area = Area {
            outer: square(0.0, 0.0, 20.0, 14.0),
            holes: vec![],
        };
        let r = 1.5;
        let p = plan_for(&area, r, 0.45);
        assert!(matches!(p.steps[0], Step::Entry { radius, .. } if radius > 0.5));
        // 一度下りたら、最後まで工具を上げずにつなぐ
        assert_eq!(entries(&p), 1);
        // 届く所はほとんど削る(残りは格子の縁の数個分)
        assert!(p.left < 0.5, "残り {} mm²", p.left);
        // 歩の大半は目標の 1.5 倍以下
        assert!(p.overloaded < 0.05, "{}", p.overloaded);
        let cuts: Vec<&Vec<P2>> = p
            .steps
            .iter()
            .filter_map(|s| match s {
                Step::Cut { points, .. } => Some(points),
                _ => None,
            })
            .collect();
        assert!(!cuts.is_empty());
        // 工具の中心はいつも入ってよい所の中
        let allowed = offset_area(&area, r);
        for c in &cuts {
            for q in c.iter() {
                assert!(allowed.iter().any(|a| a.contains(*q)), "{q:?}");
            }
        }
    }

    #[test]
    fn an_island_is_avoided_and_cleared_around() {
        let area = Area {
            outer: square(0.0, 0.0, 24.0, 24.0),
            holes: vec![square(9.0, 9.0, 15.0, 15.0).reversed()],
        };
        let r = 1.5;
        let p = plan_for(&area, r, 0.4);
        assert!(p.left < 0.5, "残り {} mm²", p.left);
        assert!(entries(&p) <= 2, "進入 {} 回", entries(&p));
        // 島(9..15 の四角)から工具の半径より近くに中心が来ない
        for s in &p.steps {
            if let Step::Cut { points, .. } = s {
                for q in points {
                    let dx = (9.0 - q[0]).max(q[0] - 15.0).max(0.0);
                    let dy = (9.0 - q[1]).max(q[1] - 15.0).max(0.0);
                    assert!(dx.hypot(dy) > r - 0.01, "島に食い込む: {q:?}");
                }
            }
        }
    }

    #[test]
    fn the_area_cut_in_one_step_is_measured_finer_than_the_cells() {
        // 前の線(左)で削った所の隣を、材料を右に見て進む。1 歩で新たに削る三日月の面積を、
        // 格子の中心で数えた場合と、格子の中の割合で数えた場合とで、細かい点で求めた値と比べる
        let (r, h) = (1.0, 0.05);
        let n = (6.0 / h) as usize;
        let mut seed = 12345u64;
        let mut rnd = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        for (ae, s) in [(0.3 * r, r / 3.0), (0.2 * r, r / 3.0), (0.6 * r, r / 3.0)] {
            let (mut e_cells, mut e_part, mut worst) = (0.0, 0.0, 0.0f64);
            let trials = 60;
            for _ in 0..trials {
                let th = rnd() * std::f64::consts::TAU;
                let d = [th.cos(), th.sin()];
                let left = [-d[1], d[0]];
                let o = [rnd() * h, rnd() * h];
                let mut g = Grid {
                    x0: -3.0,
                    y0: -3.0,
                    h,
                    nx: n,
                    ny: n,
                    mat: vec![true; n * n],
                    cov: vec![1.0; n * n],
                    ok: vec![true; n * n],
                };
                let l1 = [o[0] + left[0] * ae, o[1] + left[1] * ae];
                let mut t = -2.0;
                while t <= 2.0 {
                    g.cut([l1[0] + d[0] * t, l1[1] + d[1] * t], r);
                    t += h / 4.0;
                }
                g.cut(o, r);
                let q = [o[0] + d[0] * s, o[1] + d[1] * s];
                // 細かい点(半径の 1/300 ごと)で数えた面積
                let f = r / 300.0;
                let m = (r / f) as i64;
                let mut exact = 0.0;
                for a in -m..=m {
                    for b in -m..=m {
                        let x = [q[0] + a as f64 * f, q[1] + b as f64 * f];
                        let off = (x[0] - l1[0]) * left[0] + (x[1] - l1[1]) * left[1];
                        if (x[0] - q[0]).hypot(x[1] - q[1]) <= r
                            && (x[0] - o[0]).hypot(x[1] - o[1]) > r
                            && off.abs() > r
                        {
                            exact += f * f;
                        }
                    }
                }
                let exact = exact / (h * h);
                let mut cells = 0.0;
                g.each_in(q, r, |k| cells += f64::from(u8::from(g.mat[k])));
                let part = g.count(q, r);
                e_cells += (cells - exact).abs() / exact;
                e_part += (part - exact).abs() / exact;
                worst = worst.max((part - exact).abs() / exact);
            }
            let (e_cells, e_part) = (e_cells / trials as f64, e_part / trials as f64);
            println!(
                "ae/D {:.2}: 中心で数える {:.1} %、割合で数える {:.1} %(最大 {:.1} %)",
                ae / (2.0 * r),
                e_cells * 100.0,
                e_part * 100.0,
                worst * 100.0
            );
            // 2026-10-13 の測定: ae/D 0.10・0.15・0.30 で、中心 3.5・3.9・1.9 %、割合 3.2・2.0・1.0 %
            assert!(e_part <= e_cells && e_part < 0.04 && worst < 0.08);
        }
    }

    #[test]
    fn simplify_keeps_corners_and_drops_straight_points() {
        let pts = vec![[0.0, 0.0], [1.0, 0.0], [2.0, 0.0], [2.0, 1.0], [2.0, 2.0]];
        assert_eq!(
            simplify(&pts, 1e-6),
            vec![[0.0, 0.0], [2.0, 0.0], [2.0, 2.0]]
        );
    }
}
