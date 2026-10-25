//! 3 次元の Delaunay の四面体分割(点を 1 つずつ足す Bowyer–Watson の方法)。向きと球の内外は
//! 正確な判定(robust、Shewchuk の適応精度の判定)で決めるので、CAD の形のように同じ平面や同じ
//! 球の上に点が並んでも崩れない。
//!
//! 全部の点を囲む大きな四面体(最初の 4 点)から始め、点を足すたびに、その点を外接球の中に
//! 含む四面体(空洞)を消し、空洞の境の面と点を結んで埋め直す。外接球の内は「真に内」だけを
//! 数える(球の上の点は外)。これで空洞は点から見て星形になり、平たい四面体はできない。

use crate::geom::P3;
use robust::Coord3D;
use std::collections::HashMap;

/// 隣が無い印。
pub const NONE: u32 = u32::MAX;

fn c3(p: P3) -> Coord3D<f64> {
    Coord3D {
        x: p[0],
        y: p[1],
        z: p[2],
    }
}

/// 四面体 abcd の向き(`geom::tet_volume` と同じ符号。正確)。
pub fn orient(a: P3, b: P3, c: P3, d: P3) -> f64 {
    -robust::orient3d(c3(a), c3(b), c3(c), c3(d))
}

/// 正の向きの四面体 abcd の外接球の中に e があれば正(正確)。
pub fn insphere(a: P3, b: P3, c: P3, d: P3, e: P3) -> f64 {
    -robust::insphere(c3(a), c3(b), c3(c), c3(d), c3(e))
}

/// 四面体の面(k 番の点の向かい)の 3 点の位置。
pub const FACE: [[usize; 3]; 4] = [[1, 2, 3], [0, 3, 2], [0, 1, 3], [0, 2, 1]];

/// Delaunay の四面体分割。四面体はどれも正の向き。
#[derive(Debug, Clone, Default)]
pub struct Dt {
    /// 点(最初の 4 つは囲む四面体の点)
    pub p: Vec<P3>,
    pub t: Vec<[u32; 4]>,
    /// k 番の点の向かいの面で接する四面体
    pub nb: Vec<[u32; 4]>,
    pub dead: Vec<bool>,
    free: Vec<u32>,
    /// 点を探し始める四面体
    last: u32,
    /// 点ごとの、その点を含む四面体の 1 つ
    pub vt: Vec<u32>,
    rng: u64,
    // 足す時に使い回す入れ物
    stack: Vec<u32>,
    cavity: Vec<u32>,
    mark: Vec<u32>,
    stamp: u32,
}

/// 点を足した結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Insert {
    /// 新しい点の番号
    New(u32),
    /// 同じ位置に点があった(その番号)
    Same(u32),
}

impl Dt {
    /// 箱 [lo, hi] を十分に囲む四面体から始める。
    pub fn new(lo: P3, hi: P3) -> Self {
        let c = [0, 1, 2].map(|a| 0.5 * (lo[a] + hi[a]));
        let r = (0..3)
            .map(|a| hi[a] - lo[a])
            .fold(0.0f64, f64::max)
            .max(1e-9)
            * 20.0;
        let mut v: [P3; 4] = [
            [1.0, 1.0, 1.0],
            [1.0, -1.0, -1.0],
            [-1.0, 1.0, -1.0],
            [-1.0, -1.0, 1.0],
        ]
        .map(|s| {
            [
                c[0] + 3.0 * r * s[0],
                c[1] + 3.0 * r * s[1],
                c[2] + 3.0 * r * s[2],
            ]
        });
        if orient(v[0], v[1], v[2], v[3]) < 0.0 {
            v.swap(0, 1);
        }
        Dt {
            p: v.to_vec(),
            t: vec![[0, 1, 2, 3]],
            nb: vec![[NONE; 4]],
            dead: vec![false],
            free: Vec::new(),
            last: 0,
            vt: vec![0; 4],
            rng: 0x9e37_79b9_7f4a_7c15,
            stack: Vec::new(),
            cavity: Vec::new(),
            mark: vec![0],
            stamp: 0,
        }
    }

    /// 囲む四面体の点か。
    pub fn is_super(v: u32) -> bool {
        v < 4
    }

    fn next_rand(&mut self) -> u64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }

    /// 点 q を含む四面体(歩いて探す)。
    pub fn locate(&mut self, q: P3, start: u32) -> u32 {
        let mut t = if start != NONE && !self.dead[start as usize] {
            start
        } else {
            self.last
        };
        if self.dead[t as usize] {
            t = (0..self.t.len() as u32)
                .find(|x| !self.dead[*x as usize])
                .expect("生きた四面体");
        }
        let mut steps = 0usize;
        'walk: loop {
            steps += 1;
            if steps > 4 * self.t.len() + 1000 {
                // 歩く道が回り続けた時(起きないはず)は全部を調べる
                for (i, tt) in self.t.iter().enumerate() {
                    if self.dead[i] {
                        continue;
                    }
                    let x = tt.map(|v| self.p[v as usize]);
                    if (0..4).all(|k| {
                        let mut y = x;
                        y[k] = q;
                        orient(y[0], y[1], y[2], y[3]) >= 0.0
                    }) {
                        return i as u32;
                    }
                }
                return t;
            }
            let x = self.t[t as usize].map(|v| self.p[v as usize]);
            let s = (self.next_rand() % 4) as usize;
            for i in 0..4 {
                let k = (s + i) % 4;
                let mut y = x;
                y[k] = q;
                if orient(y[0], y[1], y[2], y[3]) < 0.0 {
                    let n = self.nb[t as usize][k];
                    if n == NONE {
                        // 囲む四面体の外(起きない)
                        return t;
                    }
                    t = n;
                    continue 'walk;
                }
            }
            return t;
        }
    }

    fn new_tet(&mut self, t: [u32; 4]) -> u32 {
        if let Some(i) = self.free.pop() {
            self.t[i as usize] = t;
            self.nb[i as usize] = [NONE; 4];
            self.dead[i as usize] = false;
            self.mark[i as usize] = 0;
            i
        } else {
            self.t.push(t);
            self.nb.push([NONE; 4]);
            self.dead.push(false);
            self.mark.push(0);
            (self.t.len() - 1) as u32
        }
    }

    /// 点 q を足したら消える四面体(空洞)を集める。`start` は q を含む四面体。
    /// `wall` が真の面(3 点)は越えない(越える必要があれば None を返す)。
    pub fn cavity_of(
        &mut self,
        q: P3,
        start: u32,
        mut wall: impl FnMut([u32; 3]) -> bool,
    ) -> Option<Vec<u32>> {
        self.stamp = self.stamp.wrapping_add(1);
        if self.stamp == 0 {
            self.mark.iter_mut().for_each(|m| *m = 0);
            self.stamp = 1;
        }
        let stamp = self.stamp;
        self.cavity.clear();
        self.stack.clear();
        self.stack.push(start);
        self.mark[start as usize] = stamp;
        let mut blocked = false;
        while let Some(t) = self.stack.pop() {
            self.cavity.push(t);
            for (k, face) in FACE.iter().enumerate() {
                let n = self.nb[t as usize][k];
                if n == NONE || self.mark[n as usize] == stamp {
                    continue;
                }
                let x = self.t[n as usize].map(|v| self.p[v as usize]);
                if insphere(x[0], x[1], x[2], x[3], q) > 0.0 {
                    let tt = self.t[t as usize];
                    if wall(face.map(|i| tt[i])) {
                        blocked = true;
                        continue;
                    }
                    self.mark[n as usize] = stamp;
                    self.stack.push(n);
                }
            }
        }
        if blocked {
            return None;
        }
        Some(self.cavity.clone())
    }

    /// 空洞 `cav` を消し、点 q で埋め直す。新しい点の番号。
    pub fn fill(&mut self, q: P3, cav: &[u32]) -> u32 {
        let v = self.p.len() as u32;
        self.p.push(q);
        self.vt.push(NONE);
        let stamp = self.stamp;
        // 空洞の境の面ごとに四面体を作る
        let mut edge: HashMap<(u32, u32), (u32, usize)> = HashMap::new();
        let mut created: Vec<u32> = Vec::new();
        for &t in cav {
            for k in 0..4 {
                let n = self.nb[t as usize][k];
                if n != NONE && self.mark[n as usize] == stamp {
                    continue;
                }
                let mut tt = self.t[t as usize];
                tt[k] = v;
                created.push(self.new_tet_deferred(tt, n, t, k));
            }
        }
        // 新しい四面体どうしをつなぐ(点 v を含む面は、v 以外の 2 点の辺で決まる)
        for &c in &created {
            let tt = self.t[c as usize];
            let kv = tt.iter().position(|x| *x == v).expect("新しい点");
            for j in 0..4 {
                if j == kv {
                    continue;
                }
                let others: Vec<u32> = (0..4)
                    .filter(|i| *i != j && *i != kv)
                    .map(|i| tt[i])
                    .collect();
                let key = (others[0].min(others[1]), others[0].max(others[1]));
                if let Some((o, oj)) = edge.remove(&key) {
                    self.nb[c as usize][j] = o;
                    self.nb[o as usize][oj] = c;
                } else {
                    edge.insert(key, (c, j));
                }
            }
        }
        for &t in cav {
            self.dead[t as usize] = true;
            self.free.push(t);
        }
        for &c in &created {
            for x in self.t[c as usize] {
                self.vt[x as usize] = c;
            }
        }
        self.last = created[0];
        v
    }

    /// 四面体を作り、外の隣 n(k の向かいの面で、空洞の四面体 t と接していた)とつなぐ。
    fn new_tet_deferred(&mut self, tt: [u32; 4], n: u32, t: u32, k: usize) -> u32 {
        let c = self.new_tet(tt);
        self.nb[c as usize][k] = n;
        if n != NONE {
            let back = self.nb[n as usize]
                .iter()
                .position(|x| *x == t)
                .expect("隣の戻り");
            self.nb[n as usize][back] = c;
        }
        c
    }

    /// 点 q を足す(`start` は探し始める四面体。NONE なら前の続き)。
    pub fn insert(&mut self, q: P3, start: u32) -> Insert {
        let t = self.locate(q, start);
        for v in self.t[t as usize] {
            if self.p[v as usize] == q {
                return Insert::Same(v);
            }
        }
        let cav = self.cavity_of(q, t, |_| false).expect("壁なし");
        Insert::New(self.fill(q, &cav))
    }

    /// 生きた四面体の番号。
    pub fn live(&self) -> impl Iterator<Item = u32> + '_ {
        (0..self.t.len() as u32).filter(|i| !self.dead[*i as usize])
    }

    /// 面(3 点を小さい順)から (四面体, 面の番号) の表。
    pub fn face_map(&self) -> HashMap<[u32; 3], (u32, usize)> {
        let mut m = HashMap::with_capacity(self.t.len() * 2);
        for t in self.live() {
            let tt = self.t[t as usize];
            for (k, face) in FACE.iter().enumerate() {
                let mut f = face.map(|i| tt[i]);
                f.sort_unstable();
                m.entry(f).or_insert((t, k));
            }
        }
        m
    }
}

/// 点を空間の順(Morton の順)に並べた番号。近い点を続けて足すと探す道が短い。
pub fn spatial_order(pts: &[P3]) -> Vec<usize> {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in pts {
        for a in 0..3 {
            lo[a] = lo[a].min(p[a]);
            hi[a] = hi[a].max(p[a]);
        }
    }
    let span = (0..3).map(|a| hi[a] - lo[a]).fold(1e-12f64, f64::max);
    let key = |p: &P3| -> u64 {
        let q = [0, 1, 2].map(|a| (((p[a] - lo[a]) / span) * 1_048_575.0) as u64);
        let mut k = 0u64;
        for b in (0..20).rev() {
            for (a, qa) in q.iter().enumerate() {
                k |= ((qa >> b) & 1) << (3 * b + a);
            }
        }
        k
    };
    let mut idx: Vec<usize> = (0..pts.len()).collect();
    idx.sort_by_key(|i| key(&pts[*i]));
    idx
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::tet_volume;

    #[test]
    fn the_signs_match_the_volume() {
        let (a, b, c, d) = ([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
        assert!(tet_volume(a, b, c, d) > 0.0 && orient(a, b, c, d) > 0.0);
        assert!(insphere(a, b, c, d, [0.2, 0.2, 0.2]) > 0.0);
        assert!(insphere(a, b, c, d, [2.0, 2.0, 2.0]) < 0.0);
        // 外接球の上(立方体の角)
        assert_eq!(insphere(a, b, c, d, [1.0, 1.0, 1.0]), 0.0);
    }

    fn check(dt: &Dt, n_in: usize) {
        let mut vol = 0.0;
        for t in dt.live() {
            let x = dt.t[t as usize].map(|v| dt.p[v as usize]);
            assert!(orient(x[0], x[1], x[2], x[3]) > 0.0, "向き");
            vol += tet_volume(x[0], x[1], x[2], x[3]);
            for k in 0..4 {
                let n = dt.nb[t as usize][k];
                if n == NONE {
                    continue;
                }
                assert!(!dt.dead[n as usize]);
                assert!(dt.nb[n as usize].contains(&t), "隣が戻る");
                // 空の外接球: 隣の向かいの点は外接球の外か上
                let opp = dt.t[n as usize]
                    .iter()
                    .find(|v| !dt.t[t as usize].contains(v))
                    .copied()
                    .unwrap();
                assert!(insphere(x[0], x[1], x[2], x[3], dt.p[opp as usize]) <= 0.0);
            }
        }
        let s = dt.p[..4].to_vec();
        let full = tet_volume(s[0], s[1], s[2], s[3]);
        assert!((vol - full).abs() < 1e-9 * full);
        assert_eq!(dt.p.len(), n_in + 4);
    }

    #[test]
    fn random_points_make_a_delaunay_mesh() {
        let mut s = 7u64;
        let mut r = || {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (s >> 11) as f64 / (1u64 << 53) as f64
        };
        let pts: Vec<P3> = (0..2000).map(|_| [r(), r(), r()]).collect();
        let mut dt = Dt::new([0.0; 3], [1.0; 3]);
        for i in spatial_order(&pts) {
            assert!(matches!(dt.insert(pts[i], NONE), Insert::New(_)));
        }
        check(&dt, pts.len());
    }

    #[test]
    fn a_grid_of_cospherical_points_does_not_break() {
        // 立方体の格子(どの 8 点も同じ球の上、同じ平面に多くの点)
        let mut pts = Vec::new();
        for i in 0..8 {
            for j in 0..8 {
                for k in 0..8 {
                    pts.push([i as f64, j as f64, k as f64]);
                }
            }
        }
        let mut dt = Dt::new([0.0; 3], [7.0; 3]);
        for i in spatial_order(&pts) {
            dt.insert(pts[i], NONE);
        }
        assert!(matches!(dt.insert([3.0, 3.0, 3.0], NONE), Insert::Same(_)));
        check(&dt, pts.len());
        // 格子の中の四面体の体積の和は 7³
        let vol: f64 = dt
            .live()
            .filter(|t| dt.t[*t as usize].iter().all(|v| !Dt::is_super(*v)))
            .map(|t| {
                let x = dt.t[t as usize].map(|v| dt.p[v as usize]);
                tet_volume(x[0], x[1], x[2], x[3])
            })
            .sum();
        assert!((vol - 343.0).abs() < 1e-9, "体積 {vol}");
    }
}
