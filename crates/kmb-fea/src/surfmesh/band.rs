//! 鋭い縁に沿うそろった帯(表面)。固定した縁などの特異点のまわりでは、縁に沿って網のつながりが
//! 点ごとに違うと、値が点ごとに上下する(固定した箱で縁から 1 mm のむら 2.6%)。つながりがそろって
//! いれば、点の位置が少しずれても 0.4% に収まる(格子の点を崩して確かめた)。そこで、張り直した
//! 表面の鋭い出っ張った縁の両側の面に、縁の点ごとに縁から w だけ離れた点の列を置き、縁と列の間を
//! 同じ向きの対角で分けた三角形の帯にする。中の網は、縁の点と両側の列の点から平行四辺形の
//! 4 つ目の点を足す(`volmesh`)。帯のまわりは、帯の三角形を除いてから、縁に沿う座標(縁に沿う
//! 長さと、縁から面に沿う距離)の上で耳を切って埋め直す。

use super::{Kind, Work, min_angle, tri_normal};
use crate::geom::{P3, add, cross, dist, dot, lerp, norm, scale, sub, unit};
use std::collections::{HashMap, HashSet};

/// 縁から列までの距離(縁の点の大きさの場に対する割合)。
const WIDTH: f64 = 0.7;
/// 縁から列までの距離の下限(両隣の縁の点までの長さの平均に対する割合)。縁の点の間が大きさの場
/// より長い所で、帯の三角形が細くならないように。
const MIN_WIDTH: f64 = 0.6;
/// 帯を置く面の間の角(材料の側)の範囲(度)。平らに近い縁と細すぎる縁には置かない。
const MIN_ANGLE: f64 = 30.0;
const MAX_ANGLE: f64 = 150.0;
/// 列の点を縁に沿って戻す量(縁の点の間の長さに対する割合)。帯の四角は長方形だと 4 点が同じ円に
/// 乗り、中の網の Delaunay がどちらの対角を選ぶか決まらない。少し傾けて、帯の対角
/// (c_i と r_{i+1})を短い方にする。縁の点ごとの断面(縁の点・両側の列の点・中の点の平行四辺形)
/// も、長方形だと 4 点が同じ円に乗って潰れた四面体ができるので、どの点も傾ける。
const SHEAR: f64 = 0.05;
/// 1 本の帯で縁が曲がってよい角(度)。曲がった縁(穴の縁など)は、この角ごとに分ける(分けた所の
/// 縁の点と列の点は両方の帯で使う)。縁に沿う座標が一通りに決まるように(輪を 1 本にすると、輪の
/// 両端の近くで座標が重なる)。
const MAX_TURN: f64 = 90.0;
/// 帯を置く曲がった縁の点の間の倍率。帯の三角形は 2 つの角が縁の点の上にあるので、重心が弦の
/// たるみの 8/9 まで面から離れる(帯の無い網では、縁の辺の向かいの角が辺の中ほどにあり 2/3)。
/// 同じたるみにするには、縁の点の間を √(3/4) 倍にする。
const CURVED_SCALE: f64 = 0.866;

/// 帯の 1 本(1 つの面の上の、続いた縁の点)。
struct Run {
    face: u32,
    /// 縁の点
    c: Vec<u32>,
    /// 面に沿って縁から離れる向き
    u: Vec<P3>,
    /// 列の点の位置
    r: Vec<P3>,
}

impl Run {
    fn trim(&mut self) {
        let n = self.c.len();
        self.c = self.c[1..n - 1].to_vec();
        self.u = self.u[1..n - 1].to_vec();
        self.r = self.r[1..n - 1].to_vec();
    }
}

impl Work<'_> {
    /// 鋭い出っ張った縁に沿って、両側の面へ帯を置く。網を変えたか。
    pub(super) fn add_bands(&mut self) -> bool {
        let mut rows: HashMap<(u32, u32), u32> = HashMap::new();
        let mut pairs: Vec<(u32, u32, u32)> = Vec::new();
        for path in self.chain_paths() {
            let Some((f1, f2)) = self.sides(path[0], path[1]) else {
                continue;
            };
            for (f, o) in [(f1, f2), (f2, f1)] {
                for mut run in self.runs(&path, f, o) {
                    // 置けなければ両端を 1 点ずつ縮めてやり直す(端のまわりで細い三角形が
                    // できる時)
                    for _ in 0..3 {
                        if let Some(made) = self.strip(&run, &rows) {
                            for (c, r) in made {
                                rows.insert((c, f), r);
                            }
                            break;
                        }
                        if run.c.len() < 5 {
                            break;
                        }
                        run.trim();
                    }
                }
            }
            let stations = if path[0] == path[path.len() - 1] {
                &path[..path.len() - 1]
            } else {
                &path[..]
            };
            for &c in stations {
                pairs.push((c, f1, f2));
            }
        }
        let mut n = 0;
        for (c, f1, f2) in pairs {
            if let (Some(&r1), Some(&r2)) = (rows.get(&(c, f1)), rows.get(&(c, f2))) {
                self.band.push([c, r1, r2]);
                n += 1;
            }
        }
        // 片側の面にしか置けなかった帯は、中の点を足せないので守らない
        let kept: HashSet<u32> = self.band.iter().flatten().copied().collect();
        for ti in 0..self.t.len() {
            if self.frozen[ti] && !self.t[ti].iter().all(|v| kept.contains(v)) {
                self.frozen[ti] = false;
            }
        }
        log::debug!("縁に沿う帯: 縁の点 {n}・置いた帯 {}", rows.len());
        !rows.is_empty()
    }

    /// 帯を置く縁のうち曲がった物(弦の中点が縁から辺の長さの 1% より離れる)の、縁の点の間を
    /// 細かくする(大きさの倍率を CURVED_SCALE にする)。変えたか。
    pub(super) fn finer_curved_chains(&mut self) -> bool {
        let mut changed = false;
        for path in self.chain_paths() {
            let Some((f1, f2)) = self.sides(path[0], path[1]) else {
                continue;
            };
            let Some(g) = path.iter().find_map(|v| match self.kind[*v as usize] {
                Kind::Chain(g) => Some(g),
                _ => None,
            }) else {
                continue;
            };
            if self.chain_scale.contains_key(&g) {
                continue;
            }
            let curved = path.windows(2).any(|w| {
                let m = lerp(self.p[w[0] as usize], self.p[w[1] as usize], 0.5);
                dist(m, self.r.project(Kind::Chain(g), m))
                    > 0.01 * dist(self.p[w[0] as usize], self.p[w[1] as usize])
            });
            if !curved
                || (self.runs(&path, f1, f2).is_empty() && self.runs(&path, f2, f1).is_empty())
            {
                continue;
            }
            self.chain_scale.insert(g, CURVED_SCALE);
            for v in &path {
                if self.kind[*v as usize] == Kind::Chain(g) {
                    self.size[*v as usize] *= CURVED_SCALE;
                }
            }
            changed = true;
        }
        changed
    }

    /// 鎖の辺 ab の両側の面。
    fn sides(&self, a: u32, b: u32) -> Option<(u32, u32)> {
        let ts = self.edge_tris(a, b);
        if ts.len() != 2 {
            return None;
        }
        let (l0, l1) = (self.lab[ts[0] as usize], self.lab[ts[1] as usize]);
        (l0 != l1).then_some((l0, l1))
    }

    /// 点 v のまわりの、面 f の三角形の法線(面積の重み、単位)と重心の平均。
    fn face_at(&self, v: u32, f: u32) -> Option<(P3, P3)> {
        let mut n = [0.0; 3];
        let mut g = [0.0; 3];
        let mut k = 0.0;
        for &ti in &self.vt[v as usize] {
            if self.lab[ti as usize] != f {
                continue;
            }
            let x = self.t[ti as usize].map(|w| self.p[w as usize]);
            n = add(n, tri_normal(x[0], x[1], x[2]));
            g = add(g, scale(add(add(x[0], x[1]), x[2]), 1.0 / 3.0));
            k += 1.0;
        }
        (k > 0.0 && norm(n) > 0.0).then(|| (unit(n), scale(g, 1.0 / k)))
    }

    /// 面 f の上で、帯を置ける縁の点の続き(3 点以上)。
    fn runs(&self, path: &[u32], f: u32, o: u32) -> Vec<Run> {
        let closed = path[0] == path[path.len() - 1];
        let st: Vec<u32> = if closed {
            path[..path.len() - 1].to_vec()
        } else {
            path.to_vec()
        };
        let n = st.len();
        if n < 4 {
            return Vec::new();
        }
        let g_self = match self.kind[st[1] as usize] {
            Kind::Chain(g) => Some(g),
            _ => None,
        };
        let cos_lo = (180.0 - MAX_ANGLE).to_radians().cos();
        let cos_hi = (180.0 - MIN_ANGLE).to_radians().cos();
        // 面に沿って縁から離れる向き・列の点・縁の向き
        let mut ok: Vec<Option<(P3, P3, P3)>> = vec![None; n];
        for i in 0..n {
            if !closed && (i == 0 || i == n - 1) {
                continue;
            }
            let c = st[i];
            let pc = self.p[c as usize];
            let (prev, next) = (st[(i + n - 1) % n], st[(i + 1) % n]);
            let t = unit(sub(self.p[next as usize], self.p[prev as usize]));
            let (Some((nf, gf)), Some((no, go))) = (self.face_at(c, f), self.face_at(c, o)) else {
                continue;
            };
            // 出っ張った縁(もう片側の面が、この面の裏にある)で、面の間の角が範囲の中
            let cn = dot(nf, no);
            if dot(nf, sub(go, pc)) >= 0.0 || cn > cos_lo || cn < cos_hi {
                continue;
            }
            let mut u = unit(cross(nf, t));
            if dot(u, sub(gf, pc)) < 0.0 {
                u = scale(u, -1.0);
            }
            let (lp, ln) = (
                dist(pc, self.p[prev as usize]),
                dist(pc, self.p[next as usize]),
            );
            let w = (WIDTH * self.size[c as usize]).max(MIN_WIDTH * 0.5 * (lp + ln));
            // 縁に沿って少し戻す(SHEAR)。帯の端の点も、隣の帯と同じ所になるように
            let q = sub(add(pc, scale(u, w)), scale(t, SHEAR * lp.min(ln)));
            let r = self.r.on_face(f, q);
            if dist(r, q) > 0.25 * w {
                continue;
            }
            // ほかの鎖から離れている(角の近くと細い面には置かない)
            if self.r.near_other_chain(r, g_self, 2.0 * w) {
                continue;
            }
            ok[i] = Some((u, r, t));
        }
        // 続いた所を集める。曲がりが MAX_TURN を超えたら分け、分けた所の点は両方の帯で使う
        // (列の点も同じ点)。輪で全部の点が使えるなら、最後の帯は最初の点で閉じる。
        let full = closed && ok.iter().all(|x| x.is_some());
        let start = if closed && !full {
            (0..n).find(|i| ok[*i].is_none()).unwrap_or(0)
        } else {
            0
        };
        let make = |cur: &[usize]| Run {
            face: f,
            c: cur.iter().map(|i| st[*i]).collect(),
            u: cur.iter().map(|i| ok[*i].unwrap().0).collect(),
            r: cur.iter().map(|i| ok[*i].unwrap().1).collect(),
        };
        let mut out: Vec<Vec<usize>> = Vec::new();
        let mut cur: Vec<usize> = Vec::new();
        let mut turn = 0.0;
        let flush = |cur: &mut Vec<usize>, out: &mut Vec<Vec<usize>>, joined: bool| {
            if cur.len() >= 3 {
                out.push(std::mem::take(cur));
            } else if joined && cur.len() == 2 && !out.is_empty() {
                // 短い残りは前の帯へ足す
                out.last_mut().unwrap().push(cur[1]);
                cur.clear();
            } else {
                cur.clear();
            }
        };
        for k in 0..=n {
            let i = (start + k) % n;
            let valid = if full { true } else { k < n && ok[i].is_some() };
            if !valid {
                let joined = cur.len() == 2 && out.last().is_some_and(|r| r.last() == cur.first());
                flush(&mut cur, &mut out, joined);
                turn = 0.0;
                continue;
            }
            if let Some(&j) = cur.last() {
                turn += angle(ok[j].unwrap().2, ok[i].unwrap().2);
            }
            cur.push(i);
            if turn > MAX_TURN && cur.len() >= 3 {
                out.push(std::mem::replace(&mut cur, vec![i]));
                turn = 0.0;
            }
        }
        let joined = cur.len() == 2 && out.last().is_some_and(|r| r.last() == cur.first());
        flush(&mut cur, &mut out, joined);
        out.iter().map(|c| make(c)).collect::<Vec<_>>()
    }

    /// 帯を 1 本置く。置けたら (縁の点, 列の点) の組。置けなければ網を元に戻す。
    fn strip(&mut self, run: &Run, have: &HashMap<(u32, u32), u32>) -> Option<Vec<(u32, u32)>> {
        let f = run.face;
        let m = run.c.len();
        let cs: Vec<P3> = run.c.iter().map(|v| self.p[*v as usize]).collect();
        let lmin = run
            .c
            .iter()
            .map(|v| self.size[*v as usize])
            .fold(f64::INFINITY, f64::min);
        let wmax = run
            .c
            .iter()
            .map(|v| WIDTH * self.size[*v as usize])
            .chain(run.r.iter().zip(&cs).map(|(r, c)| dist(*r, *c)))
            .fold(0.0, f64::max);
        let chart = Chart::new(&cs, &run.u);
        let (s_lo, s_hi) = (0.0, chart.len());
        // 帯と、その外の少しの間(列のまわりの三角形が帯に掛からないように)に掛かる三角形を除く
        // (端の列の点は縁に沿って戻してあり、帯の端より少し外に出る)
        let eps = (0.05 + 2.0 * SHEAR) * lmin;
        let clear = 0.6 * lmin;
        let inside = |p: P3| {
            let (q, off) = chart.at_off(p);
            q[0] > s_lo - eps
                && q[0] < s_hi + eps
                && q[1] > -eps
                && q[1] < wmax + clear
                && off < 0.25 * lmin + 0.1 * q[1].abs()
        };
        let cset: HashSet<u32> = run.c.iter().copied().collect();
        let mut cand: Vec<u32> = Vec::new();
        let mut seen: HashSet<u32> = HashSet::new();
        // 縁の点のまわりから広げて探す
        let mut stack: Vec<u32> = run.c.clone();
        let mut vseen: HashSet<u32> = run.c.iter().copied().collect();
        while let Some(v) = stack.pop() {
            for &ti in &self.vt[v as usize].clone() {
                if self.lab[ti as usize] != f || self.frozen[ti as usize] || !seen.insert(ti) {
                    continue;
                }
                let x = self.t[ti as usize].map(|w| self.p[w as usize]);
                let samples = [
                    x[0],
                    x[1],
                    x[2],
                    lerp(x[0], x[1], 0.5),
                    lerp(x[1], x[2], 0.5),
                    lerp(x[2], x[0], 0.5),
                    scale(add(add(x[0], x[1]), x[2]), 1.0 / 3.0),
                ];
                let hit = samples.iter().any(|q| inside(*q))
                    || self.t[ti as usize]
                        .iter()
                        .filter(|w| cset.contains(w))
                        .count()
                        >= 2;
                if hit {
                    cand.push(ti);
                    for w in self.t[ti as usize] {
                        if vseen.insert(w) {
                            stack.push(w);
                        }
                    }
                }
            }
        }
        if cand.is_empty() {
            return None;
        }
        // 除く三角形の境を 1 つの輪にする
        let mut next: HashMap<u32, u32> = HashMap::new();
        let mut directed: HashSet<(u32, u32)> = HashSet::new();
        for &ti in &cand {
            let t = self.t[ti as usize];
            for k in 0..3 {
                directed.insert((t[k], t[(k + 1) % 3]));
            }
        }
        for &(a, b) in &directed {
            if !directed.contains(&(b, a)) && next.insert(a, b).is_some() {
                log::debug!("帯を置けない: 境が 1 つの輪でない(分かれる点)");
                return None;
            }
        }
        let first = *next.keys().min()?;
        let mut cycle = vec![first];
        let mut v = next[&first];
        while v != first {
            cycle.push(v);
            let Some(&nv) = next.get(&v) else {
                log::debug!("帯を置けない: 輪が切れる");
                return None;
            };
            v = nv;
            if cycle.len() > next.len() {
                log::debug!("帯を置けない: 輪が長すぎる");
                return None;
            }
        }
        if cycle.len() != next.len() {
            log::debug!(
                "帯を置けない: 境が 2 つ以上の輪 {} {}",
                cycle.len(),
                next.len()
            );
            return None;
        }
        // 輪の中の縁の点の並びを、列を回る道に置き換える
        let pos = |x: u32| cycle.iter().position(|y| *y == x);
        let (Some(pa), Some(pb)) = (pos(run.c[0]), pos(run.c[m - 1])) else {
            log::debug!("帯を置けない: 端の点が輪に無い");
            return None;
        };
        let nc = cycle.len();
        let forward = (0..m).all(|i| cycle[(pa + i) % nc] == run.c[i]);
        let backward = (0..m).all(|i| cycle[(pb + i) % nc] == run.c[m - 1 - i]);
        if !forward && !backward {
            log::debug!("帯を置けない: 縁の点が輪の中で続かない");
            return None;
        }
        // 列の点を足す(前の帯と分け合う端では、その帯の列の点を使う)
        let old: Vec<Option<u32>> = run.c.iter().map(|c| have.get(&(*c, f)).copied()).collect();
        let rows: Vec<P3> = (0..m)
            .map(|i| old[i].map_or(run.r[i], |r| self.p[r as usize]))
            .collect();
        let rid: Vec<u32> = (0..m)
            .map(|i| old[i].unwrap_or_else(|| self.add_point(rows[i], Kind::Face(f))))
            .collect();
        let fresh: Vec<u32> = (0..m)
            .filter(|i| old[*i].is_none())
            .map(|i| rid[i])
            .collect();
        let mut poly: Vec<u32> = Vec::new();
        if forward {
            // ... c0 → (列 r0 … r_{m−1}) → c_{m−1} ...
            poly.push(run.c[0]);
            poly.extend(rid.iter().copied());
            poly.push(run.c[m - 1]);
            for k in 1..nc - m + 1 {
                poly.push(cycle[(pa + m - 1 + k) % nc]);
            }
        } else {
            poly.push(run.c[m - 1]);
            poly.extend(rid.iter().rev().copied());
            poly.push(run.c[0]);
            for k in 1..nc - m + 1 {
                poly.push(cycle[(pb + m - 1 + k) % nc]);
            }
        }
        // 前の帯と分け合う端では、縁の点が輪の中へとがって出る(x, c, x): 除く
        loop {
            let k = poly.len();
            let spike = (0..k).find(|&i| poly[(i + k - 1) % k] == poly[(i + 1) % k]);
            let dup = (0..k).find(|&i| poly[i] == poly[(i + 1) % k]);
            if let Some(i) = spike {
                poly.remove(i);
                let k = poly.len();
                poly.remove(i % k);
            } else if let Some(i) = dup {
                poly.remove(i);
            } else {
                break;
            }
            if poly.len() < 3 {
                log::debug!("帯を置けない: 多角形が潰れる");
                self.drop_points(&fresh);
                return None;
            }
        }
        let uv: Vec<[f64; 2]> = poly.iter().map(|v| chart.at(self.p[*v as usize])).collect();
        let Some(ears) = ear_clip(&uv) else {
            log::debug!("帯を置けない: 耳で切れない");
            self.drop_points(&fresh);
            return None;
        };
        // 元の三角形の向き(外向き)に合わせる(曲面と輪の縁では、近くの面の向きで)
        let nf = {
            let t = self.t[cand[0] as usize].map(|w| self.p[w as usize]);
            unit(tri_normal(t[0], t[1], t[2]))
        };
        let mut new_tris: Vec<([u32; 3], bool)> = Vec::new();
        for e in ears {
            new_tris.push((e.map(|i| poly[i]), false));
        }
        for i in 0..m - 1 {
            new_tris.push(([rid[i], run.c[i], rid[i + 1]], true));
            new_tris.push(([run.c[i], run.c[i + 1], rid[i + 1]], true));
        }
        // 除く三角形と置く三角形の面積が同じ(穴をまたぐ・重なる三角形が無い)
        let area = |t: [u32; 3]| {
            let x = t.map(|w| self.p[w as usize]);
            norm(tri_normal(x[0], x[1], x[2]))
        };
        let a0: f64 = cand.iter().map(|ti| area(self.t[*ti as usize])).sum();
        let a1: f64 = new_tris.iter().map(|(t, _)| area(*t)).sum();
        if (a1 - a0).abs() > 0.02 * a0 {
            log::debug!("帯を置けない: 面積 {a0} → {a1}");
            self.drop_points(&fresh);
            return None;
        }
        let mut fixed: Vec<([u32; 3], bool)> = Vec::new();
        for (mut t, fr) in new_tris {
            let local = self.face_normal_near(&t, f).unwrap_or(nf);
            let x = t.map(|w| self.p[w as usize]);
            let n = tri_normal(x[0], x[1], x[2]);
            if dot(n, local) < 0.0 {
                t.swap(1, 2);
            }
            let x = t.map(|w| self.p[w as usize]);
            let n = unit(tri_normal(x[0], x[1], x[2]));
            // 向きがそろわない・潰れた三角形ができるなら置かない
            if dot(n, local) < 0.5 || min_angle(x[0], x[1], x[2]) < 3.0 {
                log::debug!(
                    "帯を置けない: 三角形 {:.2} {:.1}°",
                    dot(n, local),
                    min_angle(x[0], x[1], x[2])
                );
                self.drop_points(&fresh);
                return None;
            }
            fixed.push((t, fr));
        }
        for &ti in &cand {
            self.kill_tri(ti);
        }
        for (t, fr) in fixed {
            self.add_tri(t, f);
            let last = self.frozen.len() - 1;
            self.frozen[last] = fr;
        }
        // 列の点の大きさは列の点の間に合わせる(帯の外の三角形が列に合う大きさになる)
        for i in 0..m {
            let mut l = 0.0;
            let mut k = 0.0;
            for j in [i.wrapping_sub(1), i + 1] {
                if j < m {
                    l += dist(rows[i], rows[j]);
                    k += 1.0;
                }
            }
            let v = rid[i] as usize;
            self.size[v] = self.size[v].min(l / k);
        }
        Some(run.c.iter().copied().zip(rid).collect())
    }

    /// 三角形 t の角のまわりの面 f の法線(元の三角形から)。
    fn face_normal_near(&self, t: &[u32; 3], f: u32) -> Option<P3> {
        let mut n = [0.0; 3];
        for v in t {
            if let Some((x, _)) = self.face_at(*v, f) {
                n = add(n, x);
            }
        }
        (norm(n) > 0.0).then(|| unit(n))
    }

    /// 足したが使わなかった点を消す(最後に足した点だけ)。
    fn drop_points(&mut self, ids: &[u32]) {
        let mut ids = ids.to_vec();
        ids.sort_unstable();
        while let Some(v) = ids.pop() {
            if v as usize == self.p.len() - 1 && self.vt[v as usize].is_empty() {
                self.p.pop();
                self.kind.pop();
                self.size.pop();
                self.vt.pop();
            }
        }
    }
}

/// 単位の向き a と b の間の角(度)。
fn angle(a: P3, b: P3) -> f64 {
    dot(a, b).clamp(-1.0, 1.0).acos().to_degrees()
}

/// 縁に沿う座標: 縁に沿う長さ s と、縁から面に沿って離れる距離 t。縁の両端の先は端の辺を
/// まっすぐ延ばして測る。
struct Chart {
    c: Vec<P3>,
    u: Vec<P3>,
    acc: Vec<f64>,
}

impl Chart {
    fn new(c: &[P3], u: &[P3]) -> Self {
        let mut acc = vec![0.0];
        for w in c.windows(2) {
            acc.push(acc[acc.len() - 1] + dist(w[0], w[1]));
        }
        Chart {
            c: c.to_vec(),
            u: u.to_vec(),
            acc,
        }
    }

    fn len(&self) -> f64 {
        self.acc[self.acc.len() - 1]
    }

    fn at(&self, p: P3) -> [f64; 2] {
        self.at_off(p).0
    }

    /// 座標と、座標から戻した点と p のずれ(縁から u の向きに離れていない点、例えば円柱の裏の点で
    /// 大きい)。
    fn at_off(&self, p: P3) -> ([f64; 2], f64) {
        let n = self.c.len();
        let mut best = (f64::INFINITY, [0.0; 2], 0.0);
        for j in 0..n - 1 {
            let (a, b) = (self.c[j], self.c[j + 1]);
            let ab = sub(b, a);
            let l2 = dot(ab, ab).max(1e-300);
            let mut lam = dot(sub(p, a), ab) / l2;
            let lo = if j == 0 { f64::NEG_INFINITY } else { 0.0 };
            let hi = if j == n - 2 { f64::INFINITY } else { 1.0 };
            lam = lam.clamp(lo, hi);
            let foot = add(a, scale(ab, lam));
            let d = dist(p, foot);
            if d < best.0 {
                let u = unit(lerp(self.u[j], self.u[j + 1], lam.clamp(0.0, 1.0)));
                let s = self.acc[j] + lam * l2.sqrt();
                let t = dot(sub(p, foot), u);
                best = (d, [s, t], norm(sub(sub(p, foot), scale(u, t))));
            }
        }
        (best.1, best.2)
    }
}

/// 単純な多角形を耳で切って三角形に分ける(いちばん良い角の耳から)。向きは多角形の向きのまま。
fn ear_clip(q: &[[f64; 2]]) -> Option<Vec<[usize; 3]>> {
    let n = q.len();
    if n < 3 {
        return None;
    }
    let area: f64 = (0..n)
        .map(|i| {
            let (a, b) = (q[i], q[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum();
    let sign = if area > 0.0 { 1.0 } else { -1.0 };
    let cross2 = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    };
    let angle = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| {
        let f = |p: [f64; 2], x: [f64; 2], y: [f64; 2]| {
            let (u, v) = ([x[0] - p[0], x[1] - p[1]], [y[0] - p[0], y[1] - p[1]]);
            let d = (u[0] * u[0] + u[1] * u[1]).sqrt() * (v[0] * v[0] + v[1] * v[1]).sqrt();
            if d <= 0.0 {
                0.0
            } else {
                ((u[0] * v[0] + u[1] * v[1]) / d).clamp(-1.0, 1.0).acos()
            }
        };
        f(a, b, c).min(f(b, c, a)).min(f(c, a, b))
    };
    let mut idx: Vec<usize> = (0..n).collect();
    let mut out = Vec::with_capacity(n - 2);
    while idx.len() > 3 {
        let k = idx.len();
        let mut best: Option<(f64, usize)> = None;
        for i in 0..k {
            let (a, b, c) = (idx[(i + k - 1) % k], idx[i], idx[(i + 1) % k]);
            if sign * cross2(q[a], q[b], q[c]) <= 0.0 {
                continue;
            }
            // 耳の中にほかの点が無い
            let blocked = idx.iter().any(|&j| {
                j != a
                    && j != b
                    && j != c
                    && sign * cross2(q[a], q[b], q[j]) >= 0.0
                    && sign * cross2(q[b], q[c], q[j]) >= 0.0
                    && sign * cross2(q[c], q[a], q[j]) >= 0.0
            });
            if blocked {
                continue;
            }
            let g = angle(q[a], q[b], q[c]);
            if best.is_none_or(|x| g > x.0) {
                best = Some((g, i));
            }
        }
        let (_, i) = best?;
        let k = idx.len();
        out.push([idx[(i + k - 1) % k], idx[i], idx[(i + 1) % k]]);
        idx.remove(i);
    }
    out.push([idx[0], idx[1], idx[2]]);
    delaunay_flips(q, &mut out);
    Some(out)
}

/// 耳で切った三角形の中の辺を、向かいの角の和が 180° を超える間入れ替える(多角形の中の
/// Delaunay)。耳を順に切るだけだと、細長い所で 1 点からの扇ができる。
fn delaunay_flips(q: &[[f64; 2]], t: &mut [[usize; 3]]) {
    let opp = |p: [f64; 2], a: [f64; 2], b: [f64; 2]| {
        let (u, v) = ([a[0] - p[0], a[1] - p[1]], [b[0] - p[0], b[1] - p[1]]);
        let d = (u[0] * u[0] + u[1] * u[1]).sqrt() * (v[0] * v[0] + v[1] * v[1]).sqrt();
        ((u[0] * v[0] + u[1] * v[1]) / d.max(1e-300))
            .clamp(-1.0, 1.0)
            .acos()
    };
    let cross2 = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    };
    for _ in 0..10 * t.len() + 10 {
        let mut flipped = false;
        let mut edge: HashMap<(usize, usize), (usize, usize)> = HashMap::new();
        for (i, tri) in t.iter().enumerate() {
            for k in 0..3 {
                edge.insert((tri[k], tri[(k + 1) % 3]), (i, tri[(k + 2) % 3]));
            }
        }
        for (&(a, b), &(i, c)) in &edge {
            let Some(&(j, d)) = edge.get(&(b, a)) else {
                continue;
            };
            if a > b {
                continue;
            }
            // 四角 a d b c が凸で、c と d の角の和が 180° を超える
            if opp(q[c], q[a], q[b]) + opp(q[d], q[a], q[b]) <= std::f64::consts::PI + 1e-9 {
                continue;
            }
            let s = cross2(q[a], q[b], q[c]).signum();
            if cross2(q[c], q[d], q[a]).signum() == s || cross2(q[d], q[c], q[b]).signum() == s {
                continue;
            }
            // a b c(a→b の向き)と b a d を、c d b と d c a にする
            t[i] = [c, d, b];
            t[j] = [d, c, a];
            if cross2(q[c], q[d], q[b]).signum() != s {
                t[i] = [c, b, d];
                t[j] = [d, a, c];
            }
            flipped = true;
            break;
        }
        if !flipped {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_square_with_a_notch_is_cut_into_ears() {
        let q = [[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [1.0, 1.0], [0.0, 2.0]];
        let t = ear_clip(&q).unwrap();
        assert_eq!(t.len(), 3);
        let area: f64 = t
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| q[i]);
                0.5 * ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]))
            })
            .sum();
        assert!((area - 3.0).abs() < 1e-12, "面積 {area}");
    }

    #[test]
    fn a_long_thin_polygon_is_not_cut_into_a_fan() {
        // 下は 1 mm おきの列、上はぎざぎざ(帯の外の細長い所の形)
        let mut q: Vec<[f64; 2]> = (0..=10).map(|i| [i as f64, 0.0]).collect();
        for i in (0..=10).rev() {
            q.push([i as f64, if i % 2 == 0 { 2.0 } else { 2.6 }]);
        }
        let t = ear_clip(&q).unwrap();
        let worst = t
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| [q[i][0], q[i][1], 0.0]);
                min_angle(a, b, c)
            })
            .fold(180.0, f64::min);
        assert!(worst > 20.0, "いちばん小さい角 {worst}");
    }

    fn remeshed(m: &kmb_core::Mesh, h: f64) -> (super::super::Reference, super::super::Surface) {
        let (r, s0) = super::super::reference(m, h, super::super::SizeRule::default()).unwrap();
        let s = super::super::remesh(&r, s0, &|| false).unwrap();
        (r, s)
    }

    #[test]
    fn the_sharp_edges_of_a_box_get_bands() {
        let m = crate::validation_tests::box_mesh([60.0, 40.0, 10.0]);
        let (r, s) = remeshed(&m, 2.0);
        let st = s.stats(&r);
        assert_eq!(st.open_edges, 0);
        assert!(st.min_angle > 20.0, "最小角 {}", st.min_angle);
        // 12 本の縁のうち、角から離れた所の点にはどれも帯がある
        assert!(s.band.len() > 100, "帯の点 {}", s.band.len());
        let on_edge = |q: P3| {
            ((q[0] - 60.0).abs() < 1e-9 && (q[2] - 10.0).abs() < 1e-9) && q[1] > 6.0 && q[1] < 34.0
        };
        let stations = s.points.iter().filter(|q| on_edge(**q)).count();
        let banded = s
            .band
            .iter()
            .filter(|b| on_edge(s.points[b[0] as usize]))
            .count();
        assert_eq!(stations, banded, "固定した縁の点 {stations}・帯 {banded}");
        // 列は縁から面に沿って同じくらい離れる
        for b in &s.band {
            let [c, r1, r2] = b.map(|v| s.points[v as usize]);
            let (d1, d2) = (dist(c, r1), dist(c, r2));
            assert!((d1 - d2).abs() < 0.2 * d1.max(d2), "列の距離 {d1} {d2}");
        }
        assert!(s.strip.iter().filter(|x| **x).count() >= 2 * s.band.len());
    }

    #[test]
    fn the_round_edges_of_a_cylinder_get_bands_all_round() {
        let m = crate::tetmesh::tests::cylinder(10.0, 20.0, 96);
        let (r, s) = remeshed(&m, 2.5);
        let st = s.stats(&r);
        assert_eq!(st.open_edges, 0);
        assert!(st.min_angle > 20.0, "最小角 {}", st.min_angle);
        for z in [0.0, 20.0] {
            let ring = |q: P3| (q[2] - z).abs() < 1e-9 && (q[0] * q[0] + q[1] * q[1]).sqrt() > 9.9;
            let stations = s.points.iter().filter(|q| ring(**q)).count();
            let banded = s
                .band
                .iter()
                .filter(|b| ring(s.points[b[0] as usize]))
                .count();
            assert_eq!(stations, banded, "z {z}: 縁の点 {stations}・帯 {banded}");
        }
    }
}
