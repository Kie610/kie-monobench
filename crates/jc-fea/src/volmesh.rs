//! 張り直した表面の中を四面体で埋める(形に沿ったメッシュの 2 段目)。
//!
//! 1. 表面の点で 3 次元の Delaunay の分割を作る([`crate::delaunay`])。
//! 2. 分割に表面の三角形が欠けていたら、欠けた辺(辺はあるのに三角形が無い時は最も長い辺)を
//!    割って中点を形の上へ置き、点を足す。欠けが無くなるまで繰り返す(境界に合わせた Delaunay、
//!    conforming)。表面の三角形はどれも 1 つの面に属したまま細かくなる。
//! 3. 表面の三角形の内側の四面体から、表面を越えずにたどれる物を中とする。
//! 4. 中の四面体のうち、外接球が要素の大きさより大きい物と、外接球の半径と最短の辺の比が
//!    大きい物の外接球の中心に点を足す(Delaunay の細分。Shewchuk 1998 の考え)。中心が表面の
//!    三角形を越える(足すと表面の三角形が消える)時は、代わりにその三角形の最も長い辺を割る。
//!
//! コードはどこからも写していない。

use crate::delaunay::{Dt, FACE, Insert, NONE, spatial_order};
use crate::geom::{P3, add, cross, dist, dot, scale, sub, tet_volume};
use crate::surfmesh::{Kind, Reference, Surface, Work};
use std::collections::{HashMap, HashSet};

/// 中を埋めた四面体の網(1 次。角の点だけ)。
#[derive(Debug, Clone, Default)]
pub struct Linear {
    pub points: Vec<P3>,
    /// 表面の点の種類(中の点は None)
    pub kind: Vec<Option<Kind>>,
    /// 正の向きの四面体
    pub tets: Vec<[u32; 4]>,
    /// 表面の三角形(外向き)と面の番号
    pub faces: Vec<([u32; 3], u32)>,
    /// 点ごとの、鋭い縁に沿う帯の点か(縁の点・両側の列の点・中の点)。形を良くする時に
    /// 動かさない(動かすと縁に沿ってそろえたつながりの形が崩れる)
    pub band: Vec<bool>,
    pub stats: FillStats,
}

/// 中を埋めた時の数。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FillStats {
    /// 表面の三角形を取り戻すのに割った辺の数と、繰り返した回数
    pub recovery_splits: usize,
    pub recovery_rounds: usize,
    /// 取り戻せなかった表面の三角形の数(0 でなければ誤り)
    pub missing: usize,
    /// 細分で足した中の点の数と、表面を割った数
    pub inserted: usize,
    pub encroached_splits: usize,
    /// 細分をあきらめた(表面の辺がもう短い)四面体の数
    pub given_up: usize,
    /// 鋭い縁に沿う帯の中に足した点の数
    pub band_inner: usize,
}

/// 外接球の中心と半径。
pub fn circumsphere(a: P3, b: P3, c: P3, d: P3) -> (P3, f64) {
    let (u, v, w) = (sub(b, a), sub(c, a), sub(d, a));
    let den = 2.0 * dot(u, cross(v, w));
    if den.abs() < 1e-300 {
        return (a, f64::INFINITY);
    }
    let num = add(
        add(scale(cross(v, w), dot(u, u)), scale(cross(w, u), dot(v, v))),
        scale(cross(u, v), dot(w, w)),
    );
    let o = scale(num, 1.0 / den);
    (add(a, o), dot(o, o).sqrt())
}

/// 点 q が三角形 x の赤道の球(外接円を赤道にする球)の真に中にあるか。
pub fn encroaches(x: [P3; 3], q: P3) -> bool {
    let (u, v) = (sub(x[1], x[0]), sub(x[2], x[0]));
    let w = cross(u, v);
    let ww = dot(w, w);
    if ww <= 0.0 {
        return false;
    }
    let c = add(
        x[0],
        scale(
            add(scale(cross(w, u), dot(v, v)), scale(cross(v, w), dot(u, u))),
            1.0 / (2.0 * ww),
        ),
    );
    let r2 = dot(sub(x[0], c), sub(x[0], c));
    dot(sub(q, c), sub(q, c)) < r2 * (1.0 - 1e-9)
}

fn ek(a: u32, b: u32) -> (u32, u32) {
    (a.min(b), a.max(b))
}

fn key3(mut f: [u32; 3]) -> [u32; 3] {
    f.sort_unstable();
    f
}

struct Filler<'a> {
    r: &'a Reference,
    w: Work<'a>,
    dt: Dt,
    /// 表面の点(Work の番号)→ 分割の点の番号
    to_dt: Vec<u32>,
    /// 分割の点 → 表面の点(中の点は NONE)
    to_w: Vec<u32>,
    /// 表面の三角形(分割の点の番号、小さい順)→ 外向きの 3 点と面の番号
    surf: HashMap<[u32; 3], ([u32; 3], u32)>,
    inside: Vec<bool>,
    /// 帯の中に足した点(分割の点の番号)
    band_dt: Vec<u32>,
    stats: FillStats,
}

impl<'a> Filler<'a> {
    fn add_point(&mut self, wv: u32, hint: u32) {
        let p = self.w.p[wv as usize];
        let id = match self.dt.insert(p, hint) {
            Insert::New(v) | Insert::Same(v) => v,
        };
        if self.to_dt.len() <= wv as usize {
            self.to_dt.resize(wv as usize + 1, NONE);
        }
        self.to_dt[wv as usize] = id;
        if self.to_w.len() <= id as usize {
            self.to_w.resize(id as usize + 1, NONE);
        }
        self.to_w[id as usize] = wv;
    }

    /// 表面の辺を割り、中点を分割へ足す。
    fn split_surface(&mut self, a: u32, b: u32) -> bool {
        let Some(m) = self.w.split(a, b) else {
            return false;
        };
        let hint = self.dt.vt[self.to_dt[a as usize] as usize];
        self.add_point(m, hint);
        true
    }

    fn rebuild_surf(&mut self) {
        self.surf.clear();
        for (ti, t) in self.w.t.iter().enumerate() {
            if !self.w.alive[ti] {
                continue;
            }
            let d = t.map(|v| self.to_dt[v as usize]);
            self.surf.insert(key3(d), (d, self.w.lab[ti]));
        }
    }

    /// 欠けた表面の三角形を、辺を割って取り戻す。
    fn recover(&mut self, cancelled: &dyn Fn() -> bool) -> Result<(), String> {
        for round in 0..60 {
            self.rebuild_surf();
            let fm = self.dt.face_map();
            let mut edges: HashSet<(u32, u32)> = HashSet::new();
            for t in self.dt.live() {
                let tt = self.dt.t[t as usize];
                for i in 0..4 {
                    for j in i + 1..4 {
                        edges.insert((tt[i].min(tt[j]), tt[i].max(tt[j])));
                    }
                }
            }
            let mut split: Vec<(u32, u32)> = Vec::new();
            let mut missing = 0usize;
            for (k, (d, _)) in &self.surf {
                if fm.contains_key(k) {
                    continue;
                }
                missing += 1;
                let mut any = false;
                for i in 0..3 {
                    let (a, b) = (d[i], d[(i + 1) % 3]);
                    if !edges.contains(&(a.min(b), a.max(b))) {
                        split.push(ek(self.to_w[a as usize], self.to_w[b as usize]));
                        any = true;
                    }
                }
                if !any {
                    let (i, _) = (0..3)
                        .map(|i| {
                            let (a, b) = (d[i], d[(i + 1) % 3]);
                            (i, dist(self.dt.p[a as usize], self.dt.p[b as usize]))
                        })
                        .fold((0, -1.0), |x, y| if y.1 > x.1 { y } else { x });
                    split.push(ek(
                        self.to_w[d[i] as usize],
                        self.to_w[d[(i + 1) % 3] as usize],
                    ));
                }
            }
            self.stats.missing = missing;
            if missing == 0 {
                return Ok(());
            }
            self.stats.recovery_rounds += 1;
            // 辺は小さい番号を先にしてそろえる(表面の三角形を集める順は HashMap の順なので、
            // 向きを残すと同じ入力で割る向きが変わり、網が毎回変わった)
            split.sort_unstable();
            split.dedup();
            let mut done = 0;
            for (a, b) in split {
                if self.split_surface(a, b) {
                    self.stats.recovery_splits += 1;
                    done += 1;
                }
            }
            if done == 0 {
                // 帯の三角形に阻まれて割れない(帯の三角形そのものか、割った点が帯の三角形の
                // 外接円に入る): 欠けた三角形の点に付く帯の三角形を割れるようにする
                let near: HashSet<u32> = self
                    .w
                    .t
                    .iter()
                    .enumerate()
                    .filter(|(ti, t)| {
                        self.w.alive[*ti]
                            && !fm.contains_key(&key3(t.map(|v| self.to_dt[v as usize])))
                    })
                    .flat_map(|(_, t)| *t)
                    .collect();
                for ti in 0..self.w.t.len() {
                    if self.w.alive[ti]
                        && self.w.frozen[ti]
                        && self.w.t[ti].iter().any(|v| near.contains(v))
                    {
                        self.w.unfreeze(ti as u32);
                    }
                }
            }
            // 割った後の表面を Delaunay に近づける(平らな面で外接円に点が入った三角形は、
            // 割っても取り戻せない)
            self.w.flip_all();
            log::debug!("表面の取り戻し {round}: 欠け {missing}");
            if cancelled() {
                return Err("中止しました".into());
            }
        }
        self.rebuild_surf();
        let fm = self.dt.face_map();
        self.stats.missing = self.surf.keys().filter(|k| !fm.contains_key(*k)).count();
        if self.stats.missing > 0 {
            return Err(format!(
                "表面の三角形 {} 個を四面体の網に取り戻せませんでした",
                self.stats.missing
            ));
        }
        Ok(())
    }

    /// 中の四面体を決める(表面の三角形の内側から、表面を越えずにたどる)。
    fn classify(&mut self) -> Result<(), String> {
        let fm = self.dt.face_map();
        self.inside = vec![false; self.dt.t.len()];
        let mut seen = vec![false; self.dt.t.len()];
        let mut stack: Vec<u32> = Vec::new();
        for (k, (d, _)) in &self.surf {
            let Some(&(t, f)) = fm.get(k) else {
                continue;
            };
            // 面を含む 2 つの四面体のうち、外向きの三角形の裏(中)にある方
            let tt = self.dt.t[t as usize];
            let opp = tt[f];
            let x = d.map(|v| self.dt.p[v as usize]);
            let s = tet_volume(x[0], x[1], x[2], self.dt.p[opp as usize]);
            let inner = if s < 0.0 {
                t
            } else {
                self.dt.nb[t as usize][f]
            };
            if inner != NONE && !seen[inner as usize] {
                seen[inner as usize] = true;
                stack.push(inner);
            }
        }
        while let Some(t) = stack.pop() {
            self.inside[t as usize] = true;
            let tt = self.dt.t[t as usize];
            for (k, face) in FACE.iter().enumerate() {
                let n = self.dt.nb[t as usize][k];
                if n == NONE || seen[n as usize] {
                    continue;
                }
                if self.surf.contains_key(&key3(face.map(|i| tt[i]))) {
                    continue;
                }
                seen[n as usize] = true;
                stack.push(n);
            }
        }
        for t in self.dt.live() {
            if self.inside[t as usize] && self.dt.t[t as usize].iter().any(|v| Dt::is_super(*v)) {
                return Err(
                    "四面体の網の中と外を分けられませんでした(表面が閉じていない所があります)"
                        .into(),
                );
            }
        }
        Ok(())
    }

    fn is_bad(&self, t: u32, ratio: f64, size_k: f64) -> Option<(f64, P3)> {
        let x = self.dt.t[t as usize].map(|v| self.dt.p[v as usize]);
        let (c, r) = circumsphere(x[0], x[1], x[2], x[3]);
        let mut lmin = f64::INFINITY;
        for i in 0..4 {
            for j in i + 1..4 {
                lmin = lmin.min(dist(x[i], x[j]));
            }
        }
        let g = scale(add(add(x[0], x[1]), add(x[2], x[3])), 0.25);
        let l = self.r.sizing.at(g);
        let q = (r / (size_k * l)).max(r / lmin / ratio);
        (q > 1.0).then_some((q, c))
    }

    /// 中の四面体を細かくする。
    fn refine(&mut self, cancelled: &dyn Fn() -> bool, max_points: usize) -> Result<(), String> {
        const RATIO: f64 = 2.0;
        const SIZE_K: f64 = 0.7;
        for pass in 0..40 {
            let mut queue: Vec<(f64, u32)> = self
                .dt
                .live()
                .filter(|t| self.inside[*t as usize])
                .filter_map(|t| self.is_bad(t, RATIO, SIZE_K).map(|(q, _)| (q, t)))
                .collect();
            if queue.is_empty() {
                return Ok(());
            }
            queue.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            let mut to_split: Vec<[u32; 3]> = Vec::new();
            let mut inserted = 0usize;
            while let Some((_, t)) = queue.pop() {
                if self.dt.dead[t as usize] || !self.inside[t as usize] {
                    continue;
                }
                let Some((_, c)) = self.is_bad(t, RATIO, SIZE_K) else {
                    continue;
                };
                // 潰れた四面体(4 点が同じ面の上)の中心は角の点になる: 同じ所に点を足すと
                // 潰れた四面体が増え続けるので、足さない
                let tt = self.dt.t[t as usize];
                if tt.iter().any(|v| dist(self.dt.p[*v as usize], c) < 1e-9) {
                    self.stats.given_up += 1;
                    continue;
                }
                let start = self.dt.locate(c, t);
                if !self.inside.get(start as usize).copied().unwrap_or(false) {
                    // 中心が形の外: 越える表面の三角形を割る
                    if let Some(f) = self.crossed_face(t, c) {
                        to_split.push(f);
                    } else {
                        self.stats.given_up += 1;
                    }
                    continue;
                }
                let surf = &self.surf;
                let mut walls: Vec<[u32; 3]> = Vec::new();
                let cav = self.dt.cavity_of(c, start, |f| {
                    let f = key3(f);
                    if surf.contains_key(&f) {
                        walls.push(f);
                        true
                    } else {
                        false
                    }
                });
                // 空洞の四面体の面にある表面の三角形の、赤道の球(外接円を赤道にする球)の中に
                // 中心が入るなら足さない(足すと、後で表面を割った時に取り戻せなくなる)
                let cav = cav.and_then(|cav| {
                    let mut hit = None;
                    'faces: for &t in &cav {
                        let tt = self.dt.t[t as usize];
                        for fk in FACE {
                            let f = key3(fk.map(|i| tt[i]));
                            if self.surf.contains_key(&f) {
                                let x = f.map(|v| self.dt.p[v as usize]);
                                if encroaches(x, c) {
                                    hit = Some(f);
                                    break 'faces;
                                }
                            }
                        }
                    }
                    match hit {
                        Some(f) => {
                            walls.push(f);
                            None
                        }
                        None => Some(cav),
                    }
                });
                match cav {
                    Some(cav) => {
                        let v = self.dt.fill(c, &cav);
                        if self.to_w.len() <= v as usize {
                            self.to_w.resize(v as usize + 1, NONE);
                        }
                        self.inside.resize(self.dt.t.len(), false);
                        self.mark_new_inside(v);
                        inserted += 1;
                        self.stats.inserted += 1;
                        for n in self.star(v) {
                            if let Some((q, _)) = self.is_bad(n, RATIO, SIZE_K) {
                                queue.push((q, n));
                            }
                        }
                    }
                    None => to_split.extend(walls),
                }
                if self.dt.p.len() > max_points {
                    return Err(format!(
                        "四面体の網の点が {} を超えました。要素の大きさを大きくしてください",
                        max_points
                    ));
                }
            }
            if cancelled() {
                return Err("中止しました".into());
            }
            // 越えた表面の三角形の最も長い辺を割る(短すぎる辺は割らない)
            to_split.sort_unstable();
            to_split.dedup();
            let mut splits = 0usize;
            for f in to_split {
                let Some(&(d, _)) = self.surf.get(&f) else {
                    continue;
                };
                let (i, len) = (0..3)
                    .map(|i| {
                        (
                            i,
                            dist(self.dt.p[d[i] as usize], self.dt.p[d[(i + 1) % 3] as usize]),
                        )
                    })
                    .fold((0, -1.0), |x, y| if y.1 > x.1 { y } else { x });
                let (a, b) = (d[i], d[(i + 1) % 3]);
                let mid = scale(add(self.dt.p[a as usize], self.dt.p[b as usize]), 0.5);
                if len < 0.3 * self.r.sizing.at(mid) {
                    self.stats.given_up += 1;
                    continue;
                }
                if self.split_surface(self.to_w[a as usize], self.to_w[b as usize]) {
                    splits += 1;
                    self.stats.encroached_splits += 1;
                }
            }
            if splits > 0 {
                self.w.flip_all();
            }
            log::debug!("中の細分 {pass}: 足した点 {inserted}・表面を割った {splits}");
            if splits > 0 {
                self.recover(cancelled)?;
                self.classify()?;
            } else if inserted == 0 {
                return Ok(());
            }
        }
        Ok(())
    }

    /// 鋭い縁に沿う帯(`surfmesh::band`)の中の列: 縁の点 c と両側の面の列の点 r1・r2 から、
    /// 平行四辺形の 4 つ目の点 r1 + r2 − c を足す。縁に沿って四面体のつながりがそろう。
    fn band_points(&mut self) -> usize {
        let band = self.w.band.clone();
        let mut added = 0;
        for [c, r1, r2] in band {
            let [pc, p1, p2] = [c, r1, r2].map(|v| self.w.p[v as usize]);
            let q = sub(add(p1, p2), pc);
            let start = self
                .dt
                .locate(q, self.dt.vt[self.to_dt[c as usize] as usize]);
            if !self.inside.get(start as usize).copied().unwrap_or(false) {
                continue;
            }
            let surf = &self.surf;
            let Some(cav) = self.dt.cavity_of(q, start, |f| surf.contains_key(&key3(f))) else {
                continue;
            };
            let encroached = cav.iter().any(|&t| {
                let tt = self.dt.t[t as usize];
                FACE.iter().any(|fk| {
                    let f = key3(fk.map(|i| tt[i]));
                    self.surf.contains_key(&f) && encroaches(f.map(|x| self.dt.p[x as usize]), q)
                })
            });
            if encroached {
                continue;
            }
            let v = self.dt.fill(q, &cav);
            if self.to_w.len() <= v as usize {
                self.to_w.resize(v as usize + 1, NONE);
            }
            self.inside.resize(self.dt.t.len(), false);
            self.mark_new_inside(v);
            self.band_dt.push(v);
            added += 1;
        }
        added
    }

    /// 点 v を含む生きた四面体(v の星)。
    fn star(&self, v: u32) -> Vec<u32> {
        let start = self.dt.vt[v as usize];
        let mut out = vec![start];
        let mut seen: HashSet<u32> = HashSet::from([start]);
        let mut i = 0;
        while i < out.len() {
            let t = out[i];
            i += 1;
            for n in self.dt.nb[t as usize] {
                if n != NONE && !seen.contains(&n) && self.dt.t[n as usize].contains(&v) {
                    seen.insert(n);
                    out.push(n);
                }
            }
        }
        out
    }

    fn mark_new_inside(&mut self, v: u32) {
        for t in self.star(v) {
            self.inside[t as usize] = true;
        }
    }

    /// 四面体 t の重心から点 c へ向かって最初に越える表面の三角形。
    fn crossed_face(&mut self, t: u32, c: P3) -> Option<[u32; 3]> {
        let mut cur = t;
        for _ in 0..10_000 {
            let tt = self.dt.t[cur as usize];
            let x = tt.map(|v| self.dt.p[v as usize]);
            let mut next = None;
            for k in 0..4 {
                let mut y = x;
                y[k] = c;
                if crate::delaunay::orient(y[0], y[1], y[2], y[3]) < 0.0 {
                    next = Some(k);
                    break;
                }
            }
            let k = next?;
            let f = key3(FACE[k].map(|i| tt[i]));
            if self.surf.contains_key(&f) {
                return Some(f);
            }
            let n = self.dt.nb[cur as usize][k];
            if n == NONE {
                return None;
            }
            cur = n;
        }
        None
    }
}

/// 張り直した表面 `s` の中を四面体で埋める。
pub fn fill(
    r: &Reference,
    s: Surface,
    max_points: usize,
    cancelled: &dyn Fn() -> bool,
) -> Result<Linear, String> {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in &s.points {
        for a in 0..3 {
            lo[a] = lo[a].min(p[a]);
            hi[a] = hi[a].max(p[a]);
        }
    }
    let order = spatial_order(&s.points);
    let mut f = Filler {
        r,
        w: Work::new(r, s),
        dt: Dt::new(lo, hi),
        to_dt: Vec::new(),
        to_w: Vec::new(),
        surf: HashMap::new(),
        inside: Vec::new(),
        band_dt: Vec::new(),
        stats: FillStats::default(),
    };
    for i in order {
        f.add_point(i as u32, NONE);
    }
    f.recover(cancelled)?;
    f.classify()?;
    f.stats.band_inner = f.band_points();
    f.refine(cancelled, max_points)?;
    f.rebuild_surf();
    // 出力(囲む四面体の 4 点を除いて番号を詰める)
    let mut map = vec![u32::MAX; f.dt.p.len()];
    let mut points = Vec::new();
    let mut kind = Vec::new();
    let mut tets = Vec::new();
    let mut idx = |v: u32, points: &mut Vec<P3>, kind: &mut Vec<Option<Kind>>| -> u32 {
        if map[v as usize] == u32::MAX {
            map[v as usize] = points.len() as u32;
            points.push(f.dt.p[v as usize]);
            let wv = f.to_w.get(v as usize).copied().unwrap_or(NONE);
            kind.push((wv != NONE).then(|| f.w.kind[wv as usize]));
        }
        map[v as usize]
    };
    let mut live: Vec<u32> = f.dt.live().filter(|t| f.inside[*t as usize]).collect();
    live.sort_unstable();
    for t in live {
        let tt = f.dt.t[t as usize];
        tets.push(tt.map(|v| idx(v, &mut points, &mut kind)));
    }
    let mut faces: Vec<([u32; 3], u32)> = f
        .surf
        .values()
        .map(|(d, l)| (d.map(|v| map[v as usize]), *l))
        .collect();
    faces.sort_unstable();
    if faces.iter().any(|(t, _)| t.contains(&u32::MAX)) {
        return Err("表面の三角形の点が四面体の網に入っていません".into());
    }
    let mut band = vec![false; points.len()];
    let band_w: HashSet<u32> = f.w.band.iter().flatten().copied().collect();
    for (v, &m) in map.iter().enumerate() {
        if m == u32::MAX {
            continue;
        }
        let wv = f.to_w.get(v).copied().unwrap_or(NONE);
        if wv != NONE && band_w.contains(&wv) {
            band[m as usize] = true;
        }
    }
    for &v in &f.band_dt {
        if let Some(&m) = map.get(v as usize)
            && m != u32::MAX
        {
            band[m as usize] = true;
        }
    }
    Ok(Linear {
        points,
        kind,
        tets,
        faces,
        band,
        stats: f.stats,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::surfmesh::{SizeRule, reference, remesh};
    use crate::tetmesh::{dihedral_angles, radius_ratio};
    use jc_core::Mesh;

    pub(crate) fn linear(m: &Mesh, h: f64) -> (Reference, Linear) {
        let (r, s0) = reference(m, h, SizeRule::default()).unwrap();
        let s = remesh(&r, s0, &|| false).unwrap();
        let lin = fill(&r, s, 2_000_000, &|| false).unwrap();
        (r, lin)
    }

    pub(crate) fn report(name: &str, l: &Linear) -> (f64, f64) {
        let vol: f64 = l
            .tets
            .iter()
            .map(|t| {
                let x = t.map(|v| l.points[v as usize]);
                tet_volume(x[0], x[1], x[2], x[3])
            })
            .sum();
        let surf: f64 = l
            .faces
            .iter()
            .map(|(t, _)| {
                let x = t.map(|v| l.points[v as usize]);
                tet_volume([0.0; 3], x[0], x[1], x[2])
            })
            .sum();
        let mut dmin: Vec<f64> = l
            .tets
            .iter()
            .map(|t| {
                let x = t.map(|v| l.points[v as usize]);
                dihedral_angles(x).iter().copied().fold(180.0, f64::min)
            })
            .collect();
        dmin.sort_by(f64::total_cmp);
        let mut rr: Vec<f64> = l
            .tets
            .iter()
            .map(|t| radius_ratio(t.map(|v| l.points[v as usize])))
            .collect();
        rr.sort_by(f64::total_cmp);
        let q = |v: &[f64], f: f64| v[((v.len() - 1) as f64 * f) as usize];
        let below = |x: f64| dmin.iter().filter(|d| **d < x).count() as f64 / dmin.len() as f64;
        eprintln!(
            "{name}: 四面体 {} 点 {} 体積 {vol:.2}(表面 {surf:.2})、最小の二面角 最小 {:.1}° 1% {:.1}° 中央 {:.1}°、10° 未満 {:.2}% 5° 未満 {:.2}%、半径比 最小 {:.3} 中央 {:.3}、{:?}",
            l.tets.len(),
            l.points.len(),
            dmin[0],
            q(&dmin, 0.01),
            q(&dmin, 0.5),
            100.0 * below(10.0),
            100.0 * below(5.0),
            rr[0],
            q(&rr, 0.5),
            l.stats
        );
        (vol, surf)
    }

    #[test]
    fn a_box_is_filled_to_its_volume() {
        let m = crate::validation_tests::box_mesh([30.0, 10.0, 8.0]);
        let (_, l) = linear(&m, 2.5);
        let (vol, surf) = report("箱", &l);
        assert!((vol - 2400.0).abs() < 1e-6 * 2400.0, "体積 {vol}");
        assert!((surf - 2400.0).abs() < 1e-6 * 2400.0);
        assert_eq!(l.stats.missing, 0);
        for f in 0..6 {
            assert!(l.faces.iter().any(|x| x.1 == f));
        }
    }

    #[test]
    fn the_sample_part_is_filled_to_its_volume() {
        let m = crate::surfmesh::tests::sample_part();
        for h in [2.0, 1.0] {
            let (_, l) = linear(&m, h);
            let (vol, surf) = report(&format!("見本の部品 h {h}"), &l);
            assert!((vol - surf).abs() < 1e-6 * surf, "体積 {vol} と {surf}");
            assert_eq!(l.stats.missing, 0);
        }
    }
}
