//! 四面体の網の形を良くする(形に沿ったメッシュの 3 段目)。Delaunay で埋めた網には、表面の
//! 近くに平たい四面体(4 点がほぼ同じ平面に並ぶ物)が残るので、次の 3 つを繰り返して直す
//! (Klingner・Shewchuk 2007「Aggressive Tetrahedral Mesh Improvement」の考え。コードは
//! どこからも写していない):
//! - 辺の取り除き: 中の辺を囲む四面体の輪(3〜7 個)を、辺を使わない分け方のうち最も良い物に替える
//!   (3 個の輪は 3-2 の入れ替え)。
//! - 面の取り除き: 中の面で接する 2 つの四面体を、向かいの 2 点を結ぶ 3 つに替える(2-3)。
//! - 点の移動: 悪い四面体の点を、まわりのいちばん悪い四面体が良くなる所へ動かす(中の点は自由、
//!   面の点は面の上で表面の辺の弦のたるみを大きくしない所だけ、鎖の点・角・鋭い縁に沿う帯の点は
//!   動かさない。帯の点は、止めたままでは直らない四面体を直す最後の段でだけ動かす)。
//!   面の点と鎖の点も面・鎖に沿って自由に動かすと、曲面の上に長い辺ができ(見本の部品の
//!   直径 3.4 mm の穴で弦のたるみ 0.165 → 0.405 mm)、2 次の要素の中点を面へ乗せると裏返るので
//!   中点をまっすぐへ戻すことになる(中点が穴の面から 0.3 mm 外れる)。等しい間にそろえた鎖の点と
//!   帯の点も崩れる。
//!
//! 形の良さは、6 つの二面角の正弦の最小(潰れても開きすぎても 0 に近づき、裏返ると負)と、
//! 表面の辺を持つ四面体では、その辺の中点を形へ乗せた 2 次の要素のヤコビアンの下限
//! ([`curved_quality`])の小さい方。まっすぐな四面体だけを良くすると、曲面に沿って平たい四面体が
//! 残り、`tetmesh` が中点を面へ乗せると裏返るので中点をまっすぐへ戻していた(見本の部品 h 2 で
//! 要素 6 個、中点が面から 0.14 mm 外れた。乗せた後の形も良くすると 1 個・0.06 mm)。
//! 表面の三角形はつなぎ方を変えないので、面の名前はそのまま残る。

use crate::geom::{P3, add, cross, dist, norm, scale, sub, tet_volume};
use crate::surfmesh::{Kind, Reference};
use crate::volmesh::Linear;
use std::collections::{HashMap, HashSet};

/// 四面体の形の良さ: 二面角の正弦の最小(裏返りは負)。
pub fn min_sine(p: [P3; 4]) -> f64 {
    let v = tet_volume(p[0], p[1], p[2], p[3]);
    // 面 k は点 k の向かい
    let area = |a: P3, b: P3, c: P3| 0.5 * norm(cross(sub(b, a), sub(c, a)));
    let ar = [
        area(p[1], p[2], p[3]),
        area(p[0], p[2], p[3]),
        area(p[0], p[1], p[3]),
        area(p[0], p[1], p[2]),
    ];
    let mut m = f64::INFINITY;
    for i in 0..4 {
        for j in i + 1..4 {
            // 辺 ij を共にする面は、残りの 2 点 k・l の向かいの面
            let (k, l) = match (i, j) {
                (0, 1) => (2, 3),
                (0, 2) => (1, 3),
                (0, 3) => (1, 2),
                (1, 2) => (0, 3),
                (1, 3) => (0, 2),
                _ => (0, 1),
            };
            let den = ar[k] * ar[l];
            let s = if den > 0.0 {
                1.5 * v * dist(p[i], p[j]) / den
            } else {
                -1.0
            };
            m = m.min(s);
        }
    }
    m
}

/// 目指す二面角の最小(度)。見本の部品で、辺の取り除きと点の移動を合わせて届いた値。
pub const GOAL_DEG: f64 = 25.0;

/// 面の点を動かす時の、表面の辺の弦のたるみの上限(要素の大きさに対する割合)。曲率で決めた
/// 大きさ(半径の 0.7 倍)の辺のたるみがおよそ 0.09 倍なので、それを少し超えるまで。もう超えて
/// いる所は大きくしない。見本の部品・穴・丸みで 0.05・0.1・0.15 を比べ、0.15 では中点をまっすぐへ
/// 戻す要素が出始め、0.05 では半径 0.3 mm の穴に二面角 5° の四面体が残った。
const MAX_SAG: f64 = 0.1;

/// 表面の辺の中点を形へ乗せた 2 次の要素の、ヤコビアンの下限 / 線形の値 r を、二面角の正弦と
/// 比べる形の良さにする。r が目指す二面角の正弦以上ならそのまま、下では [`CURVED_FLOOR`] へ
/// 向かって滑らかに下がる(r が負でも CURVED_FLOOR より下にしない)。中点を乗せると潰れる四面体を
/// 悪いとして直すが、そのために二面角 15° より平たい四面体を作らないように。下限なしでは見本の
/// 部品に 3.9° の四面体ができ、下限 sin 10° では 11.7°、sin 15° では 14° 以上だった。目指す値を
/// 下げる(r ≥ 0.25 で良いとする)と速いが、見本の部品で中点が面から 0.09 mm 外れた(0.42 では 0.06)。
fn curved_quality(r: f64) -> f64 {
    let g = GOAL_DEG.to_radians().sin();
    if r >= g {
        r
    } else {
        CURVED_FLOOR + (g - CURVED_FLOOR) * ((r - g) / (g - CURVED_FLOOR)).exp()
    }
}

/// [`curved_quality`] の下限(二面角 15° の正弦)。
const CURVED_FLOOR: f64 = 0.258_819;

/// 直した数。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ImproveStats {
    pub edge_removals: usize,
    pub face_removals: usize,
    pub moves: usize,
    pub passes: usize,
    /// 前と後の、いちばん悪い四面体の二面角の正弦
    pub before: f64,
    pub after: f64,
}

struct Mesh<'a> {
    r: &'a Reference,
    p: Vec<P3>,
    kind: Vec<Option<Kind>>,
    t: Vec<[u32; 4]>,
    alive: Vec<bool>,
    vt: Vec<Vec<u32>>,
    /// 表面の三角形(3 点を小さい順)
    surf: HashSet<[u32; 3]>,
    /// 鋭い縁に沿う帯の点
    band: Vec<bool>,
    /// 表面の点ごとの、表面の辺でつながる点
    snb: Vec<Vec<u32>>,
    /// 表面の辺(2 点を小さい順)の両側の面の番号
    edge_lab: HashMap<(u32, u32), [u32; 2]>,
    /// 表面の辺の、2 次の要素で中点を形へ乗せる時の動き(0 でない物)
    bend: HashMap<(u32, u32), P3>,
    /// 点ごとの要素の大きさ(始めの位置で)
    size: Vec<f64>,
}

fn key3(mut f: [u32; 3]) -> [u32; 3] {
    f.sort_unstable();
    f
}

/// 多角形(輪の点の番号 0..n)の三角形への分け方を全部。
fn triangulations(n: usize) -> Vec<Vec<[usize; 3]>> {
    fn rec(poly: &[usize]) -> Vec<Vec<[usize; 3]>> {
        if poly.len() < 3 {
            return vec![Vec::new()];
        }
        if poly.len() == 3 {
            return vec![vec![[poly[0], poly[1], poly[2]]]];
        }
        let mut out = Vec::new();
        // 辺 poly[0]–poly[last] を含む三角形の 3 点目で分ける
        let last = poly.len() - 1;
        for k in 1..last {
            let left = rec(&poly[..=k]);
            let right = rec(&poly[k..]);
            for l in &left {
                for r in &right {
                    let mut v = l.clone();
                    v.extend(r.iter().copied());
                    v.push([poly[0], poly[k], poly[last]]);
                    out.push(v);
                }
            }
        }
        out
    }
    rec(&(0..n).collect::<Vec<_>>())
}

fn ek(a: u32, b: u32) -> (u32, u32) {
    (a.min(b), a.max(b))
}

impl Mesh<'_> {
    fn q(&self, t: [u32; 4]) -> f64 {
        self.qual(t, u32::MAX, [0.0; 3], &[])
    }

    /// 表面の辺 ab(両端が pa・pb)の中点を形へ乗せる時の動き(`tetmesh::build` と同じ乗せ先:
    /// 面の中の辺は面へ、面の境の辺は境の鎖へ)。乗せ先が無ければ None。
    fn chord_move(&self, a: u32, b: u32, pa: P3, pb: P3) -> Option<P3> {
        let [f, g] = *self.edge_lab.get(&ek(a, b))?;
        let mid = scale(add(pa, pb), 0.5);
        let q = if f == g {
            self.r.on_face(f, mid)
        } else {
            self.r.on_chain(self.r.group(f, g)?, mid)
        };
        Some(sub(q, mid))
    }

    /// 2 次の要素で中点を本当に動かすか(`tetmesh::build` は辺の長さの半分以上なら動かさない)。
    fn bend_of(&self, a: u32, b: u32, pa: P3, pb: P3) -> Option<P3> {
        self.chord_move(a, b, pa, pb).filter(|d| {
            let l = norm(*d);
            l > 0.0 && l < 0.5 * dist(pa, pb)
        })
    }

    /// 点 v を at に置いた時の、v に付く表面の辺の中点の動き(他の端, 動き)と、弦のたるみ
    /// (中点から形までの距離)を要素の大きさ(両端の小さい方)で割った物の最大。
    fn edge_moves(&self, v: u32, at: P3) -> (Vec<(u32, P3)>, f64) {
        let mut out = Vec::new();
        let mut sag = 0.0f64;
        if self.kind[v as usize].is_none() {
            return (out, sag);
        }
        for &w in &self.snb[v as usize] {
            let pw = self.p[w as usize];
            let Some(d) = self.chord_move(v, w, at, pw) else {
                continue;
            };
            let l = norm(d);
            sag = sag.max(l / self.size[v as usize].min(self.size[w as usize]));
            if l > 0.0 && l < 0.5 * dist(at, pw) {
                out.push((w, d));
            }
        }
        (out, sag)
    }

    /// 四面体 t の形の良さ(点 v を at に置き、v に付く表面の辺の中点の動きを `local` とした時)。
    /// 二面角の正弦の最小と、表面の辺の中点を形へ乗せた 2 次の要素のヤコビアンの下限 / 線形
    /// の値の小さい方(中点を乗せると潰れる四面体も悪いとする)。
    fn qual(&self, t: [u32; 4], v: u32, at: P3, local: &[(u32, P3)]) -> f64 {
        let p = t.map(|x| if x == v { at } else { self.p[x as usize] });
        let s = min_sine(p);
        if s <= 0.0 {
            return s;
        }
        let mut x = [[0.0; 3]; 10];
        x[..4].copy_from_slice(&p);
        let mut any = false;
        for (k, [i, j]) in crate::tet10::EDGES.iter().enumerate() {
            let (a, b) = (t[*i], t[*j]);
            let mut m = scale(add(p[*i], p[*j]), 0.5);
            let d = if a == v || b == v {
                let o = if a == v { b } else { a };
                local.iter().find(|(w, _)| *w == o).map(|(_, d)| *d)
            } else {
                self.bend.get(&ek(a, b)).copied()
            };
            if let Some(d) = d {
                m = add(m, d);
                any = true;
            }
            x[4 + k] = m;
        }
        if !any {
            return s;
        }
        let (low, _) = crate::tet10::jacobian_bounds(&x);
        s.min(curved_quality(
            low / (6.0 * tet_volume(p[0], p[1], p[2], p[3])),
        ))
    }

    fn add(&mut self, t: [u32; 4]) -> u32 {
        let i = self.t.len() as u32;
        self.t.push(t);
        self.alive.push(true);
        for v in t {
            self.vt[v as usize].push(i);
        }
        i
    }

    fn kill(&mut self, i: u32) {
        self.alive[i as usize] = false;
        for v in self.t[i as usize] {
            self.vt[v as usize].retain(|x| *x != i);
        }
    }

    /// 辺 ab を囲む四面体の輪(輪の点の順と四面体)。閉じない(表面の辺)なら None。
    fn ring(&self, a: u32, b: u32) -> Option<(Vec<u32>, Vec<u32>)> {
        let around: Vec<u32> = self.vt[a as usize]
            .iter()
            .copied()
            .filter(|t| self.t[*t as usize].contains(&b))
            .collect();
        if around.len() < 3 || around.len() > 7 {
            return None;
        }
        let others = |t: u32| -> [u32; 2] {
            let tt = self.t[t as usize];
            let o: Vec<u32> = tt.iter().copied().filter(|v| *v != a && *v != b).collect();
            [o[0], o[1]]
        };
        let first = around[0];
        let [p0, mut cur] = others(first);
        let mut verts = vec![p0];
        let mut tets = vec![first];
        let mut prev_t = first;
        loop {
            if cur == p0 {
                break;
            }
            verts.push(cur);
            let next = around
                .iter()
                .copied()
                .find(|t| *t != prev_t && others(*t).contains(&cur))?;
            if tets.contains(&next) {
                return None;
            }
            tets.push(next);
            let o = others(next);
            let nxt = if o[0] == cur { o[1] } else { o[0] };
            prev_t = next;
            cur = nxt;
            if verts.len() > around.len() {
                return None;
            }
        }
        if tets.len() != around.len() {
            return None;
        }
        // 輪の面(a, b, 輪の点)が表面なら取り除かない(中の辺だけ)
        for v in &verts {
            if self.surf.contains(&key3([a, b, *v])) {
                return None;
            }
        }
        Some((verts, tets))
    }

    /// 辺 ab を取り除いて良くなるなら替える。
    fn remove_edge(&mut self, a: u32, b: u32, tris: &[Vec<Vec<[usize; 3]>>]) -> bool {
        let Some((ring, old)) = self.ring(a, b) else {
            return false;
        };
        let before = old
            .iter()
            .map(|t| self.q(self.t[*t as usize]))
            .fold(f64::INFINITY, f64::min);
        let mut best: Option<(f64, Vec<[u32; 4]>)> = None;
        for tri in &tris[ring.len()] {
            let mut worst = f64::INFINITY;
            let mut new = Vec::with_capacity(2 * tri.len());
            for t in tri {
                let (x, y, z) = (ring[t[0]], ring[t[1]], ring[t[2]]);
                let mut ta = [x, y, z, a];
                let mut tb = [x, y, z, b];
                let pa = ta.map(|v| self.p[v as usize]);
                let pb = tb.map(|v| self.p[v as usize]);
                let (va, vb) = (
                    tet_volume(pa[0], pa[1], pa[2], pa[3]),
                    tet_volume(pb[0], pb[1], pb[2], pb[3]),
                );
                if va * vb >= 0.0 {
                    worst = -1.0;
                    break;
                }
                if va < 0.0 {
                    ta.swap(0, 1);
                } else {
                    tb.swap(0, 1);
                }
                worst = worst.min(self.q(ta)).min(self.q(tb));
                if worst <= before {
                    break;
                }
                new.push(ta);
                new.push(tb);
            }
            if worst > before && best.as_ref().is_none_or(|b| worst > b.0) {
                best = Some((worst, new));
            }
        }
        let Some((_, new)) = best else {
            return false;
        };
        // 輪の外へはみ出す分け方(体積の和が変わる)は使わない
        let vol = |t: &[u32; 4]| {
            let p = t.map(|v| self.p[v as usize]);
            tet_volume(p[0], p[1], p[2], p[3])
        };
        let old_v: f64 = old.iter().map(|t| vol(&self.t[*t as usize])).sum();
        let new_v: f64 = new.iter().map(vol).sum();
        if (old_v - new_v).abs() > 1e-9 * old_v.abs().max(1e-30) {
            return false;
        }
        for t in old {
            self.kill(t);
        }
        for t in new {
            self.add(t);
        }
        true
    }

    /// 面 abc(中の面)を取り除いて 3 つの四面体にする(良くなる時だけ)。
    fn remove_face(&mut self, f: [u32; 3]) -> bool {
        let both: Vec<u32> = self.vt[f[0] as usize]
            .iter()
            .copied()
            .filter(|t| {
                let tt = self.t[*t as usize];
                tt.contains(&f[1]) && tt.contains(&f[2])
            })
            .collect();
        if both.len() != 2 || self.surf.contains(&key3(f)) {
            return false;
        }
        let opp = |t: u32| {
            *self.t[t as usize]
                .iter()
                .find(|v| !f.contains(v))
                .expect("向かいの点")
        };
        let (d, e) = (opp(both[0]), opp(both[1]));
        let before = self
            .q(self.t[both[0] as usize])
            .min(self.q(self.t[both[1] as usize]));
        let mut new = Vec::new();
        let mut worst = f64::INFINITY;
        for (x, y) in [(f[0], f[1]), (f[1], f[2]), (f[2], f[0])] {
            let mut t = [x, y, d, e];
            let p = t.map(|v| self.p[v as usize]);
            if tet_volume(p[0], p[1], p[2], p[3]) < 0.0 {
                t.swap(0, 1);
            }
            let q = self.q(t);
            worst = worst.min(q);
            new.push(t);
        }
        if worst <= before || worst <= 0.0 {
            return false;
        }
        // 3 つの体積の和が元の 2 つと同じでなければ(de が面の外を通る)替えない
        let vol = |t: [u32; 4]| {
            let p = t.map(|v| self.p[v as usize]);
            tet_volume(p[0], p[1], p[2], p[3])
        };
        let old_v = vol(self.t[both[0] as usize]) + vol(self.t[both[1] as usize]);
        let new_v: f64 = new.iter().map(|t| vol(*t)).sum();
        if (old_v - new_v).abs() > 1e-9 * old_v.abs().max(1e-30) {
            return false;
        }
        for t in both {
            self.kill(t);
        }
        for t in new {
            self.add(t);
        }
        true
    }

    /// 点 v を at に置き、v に付く表面の辺の中点の動きを `local` とした時の、まわりの
    /// いちばん悪い四面体の形の良さ。
    fn worst_around(&self, v: u32, at: P3, local: &[(u32, P3)]) -> f64 {
        let mut w = f64::INFINITY;
        for &t in &self.vt[v as usize] {
            w = w.min(self.qual(self.t[t as usize], v, at, local));
        }
        w
    }

    /// 点 v を、まわりのいちばん悪い四面体が良くなる所へ動かす。
    ///
    /// 動かす点: 中の点は自由に。面の点は面の上で、付く表面の辺の弦のたるみが要素の大きさの
    /// [`MAX_SAG`] 倍(もう超えていれば今の値)を超えない所だけ(たるみが大きいと 2 次の要素の
    /// 中点を面へ乗せられない)。鎖の点(角から角まで等しい間にそろえた点)と角は動かさない。
    /// 鋭い縁に沿う帯の点は、`rescue`(帯を止めたままでは直らない四面体を直す最後の段)の時
    /// だけ、縁の上の点を除いて動かす。
    fn smooth(&mut self, v: u32, rescue: bool) -> bool {
        let k = self.kind[v as usize];
        if self.band.get(v as usize).copied().unwrap_or(false) && !rescue {
            return false;
        }
        let face = match k {
            None => None,
            Some(Kind::Face(f)) => Some(f),
            Some(Kind::Chain(_) | Kind::Corner) => return false,
        };
        let p0 = self.p[v as usize];
        let (here, sag0) = self.edge_moves(v, p0);
        let limit = sag0.max(MAX_SAG) + 1e-12;
        // q に置いた時の形の良さ(弦のたるみが上限を超えるなら None)
        let try_at = |q: P3| -> Option<f64> {
            let (local, sag) = self.edge_moves(v, q);
            (sag <= limit).then(|| self.worst_around(v, q, &local))
        };
        let start = self.worst_around(v, p0, &here);
        let mut best = (start, p0);
        // 隣の点の重心
        let mut nbs: Vec<u32> = self.vt[v as usize]
            .iter()
            .flat_map(|t| self.t[*t as usize])
            .filter(|x| *x != v)
            .collect();
        nbs.sort_unstable();
        nbs.dedup();
        if nbs.is_empty() {
            return false;
        }
        let mut c = [0.0; 3];
        for n in &nbs {
            c = add(c, self.p[*n as usize]);
        }
        let c = scale(c, 1.0 / nbs.len() as f64);
        let lmin = nbs
            .iter()
            .map(|n| dist(self.p[*n as usize], p0))
            .fold(f64::INFINITY, f64::min);
        let project = |q: P3| match face {
            None => q,
            Some(f) => self.r.on_face(f, q),
        };
        for t in [0.25, 0.5, 1.0] {
            let q = project(add(p0, scale(sub(c, p0), t)));
            if let Some(w) = try_at(q)
                && w > best.0
            {
                best = (w, q);
            }
        }
        // 向きを変えながら探す
        let mut step = 0.3 * lmin;
        for _ in 0..8 {
            let mut moved = false;
            for a in 0..3 {
                for s in [-1.0, 1.0] {
                    let mut q = best.1;
                    q[a] += s * step;
                    let q = project(q);
                    if let Some(w) = try_at(q)
                        && w > best.0 + 1e-12
                    {
                        best = (w, q);
                        moved = true;
                    }
                }
            }
            if !moved {
                step *= 0.5;
            }
        }
        if best.0 > start + 1e-9 {
            self.p[v as usize] = best.1;
            if self.kind[v as usize].is_some() {
                for w in self.snb[v as usize].clone() {
                    match self.bend_of(v, w, best.1, self.p[w as usize]) {
                        Some(d) => self.bend.insert(ek(v, w), d),
                        None => self.bend.remove(&ek(v, w)),
                    };
                }
            }
            true
        } else {
            false
        }
    }
}

/// 網 `lin` の四面体の形を良くする。`goal` は目指す二面角の正弦(これより悪い四面体を直す)。
pub fn improve(
    r: &Reference,
    lin: &mut Linear,
    goal: f64,
    cancelled: &dyn Fn() -> bool,
) -> Result<ImproveStats, String> {
    let mut m = Mesh {
        r,
        p: lin.points.clone(),
        kind: lin.kind.clone(),
        t: lin.tets.clone(),
        alive: vec![true; lin.tets.len()],
        vt: vec![Vec::new(); lin.points.len()],
        surf: lin.faces.iter().map(|(f, _)| key3(*f)).collect(),
        band: lin.band.clone(),
        snb: vec![Vec::new(); lin.points.len()],
        edge_lab: HashMap::new(),
        bend: HashMap::new(),
        size: lin
            .points
            .iter()
            .zip(&lin.kind)
            .map(|(p, k)| k.map_or(0.0, |_| r.sizing.at(*p)))
            .collect(),
    };
    for (f, l) in &lin.faces {
        for k in 0..3 {
            let (a, b) = (f[k], f[(k + 1) % 3]);
            m.snb[a as usize].push(b);
            m.snb[b as usize].push(a);
            m.edge_lab
                .entry(ek(a, b))
                .and_modify(|x| x[1] = *l)
                .or_insert([*l, *l]);
        }
    }
    let edges: Vec<(u32, u32)> = m.edge_lab.keys().copied().collect();
    for (a, b) in edges {
        if let Some(d) = m.bend_of(a, b, m.p[a as usize], m.p[b as usize]) {
            m.bend.insert((a, b), d);
        }
    }
    for n in &mut m.snb {
        n.sort_unstable();
        n.dedup();
    }
    for (i, t) in m.t.iter().enumerate() {
        for v in t {
            m.vt[*v as usize].push(i as u32);
        }
    }
    let tris: Vec<Vec<Vec<[usize; 3]>>> = (0..=7).map(triangulations).collect();
    let worst = |m: &Mesh| {
        (0..m.t.len())
            .filter(|i| m.alive[*i])
            .map(|i| min_sine(m.t[i].map(|v| m.p[v as usize])))
            .fold(f64::INFINITY, f64::min)
    };
    let mut st = ImproveStats {
        before: worst(&m),
        ..ImproveStats::default()
    };
    // 帯の点を止めて直し、それでも残った悪い四面体だけ、帯の点も動かして直す
    for rescue in [false, true] {
        for _ in 0..8 {
            st.passes += 1;
            let bad: Vec<u32> = (0..m.t.len() as u32)
                .filter(|i| m.alive[*i as usize] && m.q(m.t[*i as usize]) < goal)
                .collect();
            if bad.is_empty() {
                break;
            }
            let mut changed = 0usize;
            // つなぎ方を替える
            for &t in &bad {
                if !m.alive[t as usize] || m.q(m.t[t as usize]) >= goal {
                    continue;
                }
                let tt = m.t[t as usize];
                let mut done = false;
                for i in 0..4 {
                    for j in i + 1..4 {
                        if !done && m.remove_edge(tt[i], tt[j], &tris) {
                            st.edge_removals += 1;
                            done = true;
                        }
                    }
                }
                if !done {
                    for k in 0..4 {
                        let f = [tt[(k + 1) % 4], tt[(k + 2) % 4], tt[(k + 3) % 4]];
                        if !done && m.remove_face(f) {
                            st.face_removals += 1;
                            done = true;
                        }
                    }
                }
                if done {
                    changed += 1;
                }
            }
            // 点を動かす
            let mut verts: Vec<u32> = (0..m.t.len())
                .filter(|i| m.alive[*i] && m.q(m.t[*i]) < goal)
                .flat_map(|i| m.t[i])
                .collect();
            verts.sort_unstable();
            verts.dedup();
            for v in verts {
                if m.smooth(v, rescue) {
                    st.moves += 1;
                    changed += 1;
                }
            }
            log::debug!(
                "四面体の形の改善 {}(帯も動かす {rescue}): 悪い四面体 {}・替えた {changed}",
                st.passes,
                bad.len()
            );
            if cancelled() {
                return Err("中止しました".into());
            }
            if changed == 0 {
                break;
            }
        }
    }
    st.after = worst(&m);
    // 詰め直す
    let tets: Vec<[u32; 4]> = (0..m.t.len())
        .filter(|i| m.alive[*i])
        .map(|i| m.t[i])
        .collect();
    lin.points = m.p;
    lin.tets = tets;
    Ok(st)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::volmesh::tests::{linear, report};

    #[test]
    fn the_quality_measure_is_one_for_a_regular_tet() {
        let s = 1.0 / 2f64.sqrt();
        let p = [
            [1.0, 0.0, -s],
            [-1.0, 0.0, -s],
            [0.0, 1.0, s],
            [0.0, -1.0, s],
        ];
        let p = if tet_volume(p[0], p[1], p[2], p[3]) < 0.0 {
            [p[1], p[0], p[2], p[3]]
        } else {
            p
        };
        // 正四面体の二面角 70.53° の正弦
        assert!((min_sine(p) - 70.528_779f64.to_radians().sin()).abs() < 1e-6);
        assert!(min_sine([p[1], p[0], p[2], p[3]]) < 0.0);
        assert_eq!(triangulations(5).len(), 5);
        assert_eq!(triangulations(7).len(), 42);
    }

    #[test]
    fn slivers_of_the_sample_part_are_removed() {
        let m = crate::surfmesh::tests::sample_part();
        let (r, mut l) = linear(&m, 2.0);
        let (v0, _) = report("前", &l);
        let st = improve(&r, &mut l, GOAL_DEG.to_radians().sin(), &|| false).unwrap();
        let (v1, s1) = report("後", &l);
        eprintln!("{st:?}");
        assert!((v1 - s1).abs() < 1e-6 * s1, "体積 {v1} と表面 {s1}");
        let _ = v0;
        // 二面角は 12° 以上で、20° 未満は 1 万個に 1 個まで(目指すのは 25°)。表面の点を面に
        // 沿って自由に動かせば 25° に届くが、曲面の上に長い辺ができて 2 次の要素の中点を面へ
        // 乗せられなくなる。表面の点だけの四面体(曲面の上の 4 点の平たい物と、帯の点の物)が
        // 少し残る
        assert!(st.after > 12f64.to_radians().sin(), "{st:?}");
        let low = l
            .tets
            .iter()
            .filter(|t| min_sine(t.map(|v| l.points[v as usize])) < 20f64.to_radians().sin())
            .count();
        assert!(
            low * 10_000 <= l.tets.len(),
            "20° 未満 {low}/{}",
            l.tets.len()
        );
    }

    /// 表面の辺の、弦のたるみ / 大きさ(始めの位置での両端の大きさの小さい方)の最大。
    fn worst_sag(r: &Reference, l: &Linear, size: &[f64]) -> f64 {
        let mut w = 0.0f64;
        for (f, lab) in &l.faces {
            for k in 0..3 {
                let (a, b) = (f[k] as usize, f[(k + 1) % 3] as usize);
                let mid = scale(add(l.points[a], l.points[b]), 0.5);
                w = w.max(dist(mid, r.on_face(*lab, mid)) / size[a].min(size[b]));
            }
        }
        w
    }

    /// 形を良くしても、表面の辺の弦のたるみが(大きさの 10% を超えて)増えず、面の境の鎖の点と
    /// 鋭い縁に沿う帯の点は動かない(面の点を面に沿って自由に動かすと、曲面の上に長い辺ができ、
    /// 2 次の要素の中点を面へ乗せられずにまっすぐへ戻すことになる。鎖を等しい間にそろえた点と帯の
    /// 点も崩れる)。
    #[test]
    fn improving_keeps_the_surface_chords_chains_and_bands() {
        // 曲がった縁(穴の縁)の帯は、点を止めたままでは平たい四面体が残るので、最後の手段として
        // 帯の点を動かす(帯の点の一部だけ)。まっすぐな縁(箱)の帯は動かさない
        let cases = [
            (
                crate::tetmesh::tests::annulus(1.7, 12.0, 6.0, 128, 2),
                2.0,
                0.2,
            ),
            (
                crate::validation_tests::box_mesh([30.0, 20.0, 10.0]),
                2.0,
                0.0,
            ),
        ];
        for (m, h, band_share) in cases {
            let (r, mut l) = linear(&m, h);
            let before = l.points.clone();
            let size: Vec<f64> = before.iter().map(|p| r.sizing.at(*p)).collect();
            let sag0 = worst_sag(&r, &l, &size);
            let st = improve(&r, &mut l, GOAL_DEG.to_radians().sin(), &|| false).unwrap();
            let sag1 = worst_sag(&r, &l, &size);
            let moved = |i: usize| dist(before[i], l.points[i]) > 1e-12;
            let chain = (0..l.points.len())
                .filter(|i| matches!(l.kind[*i], Some(Kind::Chain(_))) && moved(*i))
                .count();
            let band = (0..l.points.len())
                .filter(|i| l.band[*i] && moved(*i))
                .count();
            let band_n = l.band.iter().filter(|b| **b).count();
            eprintln!(
                "たるみ {sag0:.4} → {sag1:.4}、動いた鎖の点 {chain}・帯の点 {band}/{band_n}、{st:?}"
            );
            assert!(sag1 <= sag0.max(MAX_SAG) + 1e-9, "たるみ {sag0} → {sag1}");
            assert_eq!(chain, 0, "鎖の点が動いた");
            assert!(band_n > 0);
            assert!(
                band as f64 <= band_share * band_n as f64,
                "帯の点が動いた {band}/{band_n}"
            );
            assert!(st.after > 20f64.to_radians().sin(), "{st:?}");
        }
    }
}
