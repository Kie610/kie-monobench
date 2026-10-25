//! 三角形の箱の木(光線追跡で、光線が当たる三角形を速く探す)。表面積の見積もり(SAH)で 12 の
//! 区切りから分け方を選ぶ。

use crate::math::*;
use crate::raster::{Tri, hit_tri};

#[derive(Debug, Clone, Copy)]
struct Node {
    min: V3,
    max: V3,
    /// 葉なら最初の三角形の番号(`order` の中)、節なら左の子の番号(右は +1)
    first: u32,
    /// 葉の三角形の数(0 なら節)
    count: u32,
}

/// 箱の木。
pub struct Bvh {
    nodes: Vec<Node>,
    /// 三角形の番号を、葉ごとに続けて並べたもの
    order: Vec<u32>,
}

const LEAF: usize = 4;
const BINS: usize = 12;

fn grow(min: &mut V3, max: &mut V3, p: V3) {
    for k in 0..3 {
        min[k] = min[k].min(p[k]);
        max[k] = max[k].max(p[k]);
    }
}

fn area(min: V3, max: V3) -> f32 {
    let d = sub(max, min);
    if d[0] < 0.0 {
        return 0.0;
    }
    2.0 * (d[0] * d[1] + d[1] * d[2] + d[2] * d[0])
}

impl Bvh {
    pub(crate) fn build(tris: &[Tri]) -> Bvh {
        let centers: Vec<V3> = tris
            .iter()
            .map(|t| scale(add(add(t.p[0], t.p[1]), t.p[2]), 1.0 / 3.0))
            .collect();
        let mut b = Bvh {
            nodes: Vec::with_capacity(tris.len() * 2 / LEAF + 1),
            order: (0..tris.len() as u32).collect(),
        };
        b.nodes.push(Node {
            min: [0.0; 3],
            max: [0.0; 3],
            first: 0,
            count: tris.len() as u32,
        });
        if !tris.is_empty() {
            b.split(0, tris, &centers);
        }
        b
    }

    fn bounds(&self, tris: &[Tri], first: usize, count: usize) -> (V3, V3) {
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for &i in &self.order[first..first + count] {
            for p in tris[i as usize].p {
                grow(&mut min, &mut max, p);
            }
        }
        (min, max)
    }

    fn split(&mut self, node: usize, tris: &[Tri], centers: &[V3]) {
        let (first, count) = (
            self.nodes[node].first as usize,
            self.nodes[node].count as usize,
        );
        let (min, max) = self.bounds(tris, first, count);
        self.nodes[node].min = min;
        self.nodes[node].max = max;
        if count <= LEAF {
            return;
        }
        // 中心の箱
        let mut cmin = [f32::INFINITY; 3];
        let mut cmax = [f32::NEG_INFINITY; 3];
        for &i in &self.order[first..first + count] {
            grow(&mut cmin, &mut cmax, centers[i as usize]);
        }
        let mut best = (f32::INFINITY, 0usize, 0.0f32);
        for axis in 0..3 {
            let ext = cmax[axis] - cmin[axis];
            if ext <= 1e-9 {
                continue;
            }
            let mut bins = [([f32::INFINITY; 3], [f32::NEG_INFINITY; 3], 0usize); BINS];
            let k = BINS as f32 / ext;
            for &i in &self.order[first..first + count] {
                let b = (((centers[i as usize][axis] - cmin[axis]) * k) as usize).min(BINS - 1);
                for p in tris[i as usize].p {
                    grow(&mut bins[b].0, &mut bins[b].1, p);
                }
                bins[b].2 += 1;
            }
            // 左から積んだ面積と数、右から積んだ面積と数
            let mut left = [(0.0f32, 0usize); BINS - 1];
            let (mut lmin, mut lmax, mut ln) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3], 0);
            for i in 0..BINS - 1 {
                grow(&mut lmin, &mut lmax, bins[i].0);
                grow(&mut lmin, &mut lmax, bins[i].1);
                ln += bins[i].2;
                left[i] = (area(lmin, lmax), ln);
            }
            let (mut rmin, mut rmax, mut rn) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3], 0);
            for i in (1..BINS).rev() {
                grow(&mut rmin, &mut rmax, bins[i].0);
                grow(&mut rmin, &mut rmax, bins[i].1);
                rn += bins[i].2;
                let (la, lc) = left[i - 1];
                if lc == 0 || rn == 0 {
                    continue;
                }
                let cost = la * lc as f32 + area(rmin, rmax) * rn as f32;
                if cost < best.0 {
                    best = (cost, axis, cmin[axis] + i as f32 / k);
                }
            }
        }
        let leaf_cost = area(min, max) * count as f32;
        if best.0 >= leaf_cost && count <= LEAF * 4 {
            return;
        }
        let (axis, at) = if best.0.is_finite() {
            (best.1, best.2)
        } else {
            return;
        };
        // 分ける
        let slice = &mut self.order[first..first + count];
        let mut i = 0;
        let mut j = slice.len();
        while i < j {
            if centers[slice[i] as usize][axis] < at {
                i += 1;
            } else {
                j -= 1;
                slice.swap(i, j);
            }
        }
        if i == 0 || i == count {
            return;
        }
        let l = self.nodes.len();
        self.nodes.push(Node {
            min: [0.0; 3],
            max: [0.0; 3],
            first: first as u32,
            count: i as u32,
        });
        self.nodes.push(Node {
            min: [0.0; 3],
            max: [0.0; 3],
            first: (first + i) as u32,
            count: (count - i) as u32,
        });
        self.nodes[node].first = l as u32;
        self.nodes[node].count = 0;
        self.split(l, tris, centers);
        self.split(l + 1, tris, centers);
    }

    /// いちばん近い交わり(距離、三角形の番号、重心座標の b・c)。`tmax` より遠くは見ない。
    pub(crate) fn hit(
        &self,
        tris: &[Tri],
        o: V3,
        d: V3,
        tmax: f32,
    ) -> Option<(f32, u32, f32, f32)> {
        if tris.is_empty() {
            return None;
        }
        let inv = d.map(|v| {
            if v.abs() > 1e-12 {
                1.0 / v
            } else {
                1e12f32.copysign(v)
            }
        });
        let mut best: Option<(f32, u32, f32, f32)> = None;
        let mut limit = tmax;
        let mut stack = [0u32; 64];
        let mut sp = 1;
        while sp > 0 {
            sp -= 1;
            let n = self.nodes[stack[sp] as usize];
            if slab(n.min, n.max, o, inv, limit).is_none() {
                continue;
            }
            if n.count > 0 {
                for &ti in &self.order[n.first as usize..(n.first + n.count) as usize] {
                    if let Some((t, b, c)) = hit_tri(o, d, &tris[ti as usize])
                        && t < limit
                    {
                        limit = t;
                        best = Some((t, ti, b, c));
                    }
                }
            } else if sp + 2 <= stack.len() {
                // 近い子を後から積む(先に見る)
                let (a, b) = (n.first, n.first + 1);
                let da = slab(
                    self.nodes[a as usize].min,
                    self.nodes[a as usize].max,
                    o,
                    inv,
                    limit,
                );
                let db = slab(
                    self.nodes[b as usize].min,
                    self.nodes[b as usize].max,
                    o,
                    inv,
                    limit,
                );
                match (da, db) {
                    (Some(x), Some(y)) => {
                        let (near, far) = if x <= y { (a, b) } else { (b, a) };
                        stack[sp] = far;
                        stack[sp + 1] = near;
                        sp += 2;
                    }
                    (Some(_), None) => {
                        stack[sp] = a;
                        sp += 1;
                    }
                    (None, Some(_)) => {
                        stack[sp] = b;
                        sp += 1;
                    }
                    (None, None) => {}
                }
            }
        }
        best
    }

    /// `tmax` までに何かに当たるか(影を見る)。
    pub(crate) fn occluded(&self, tris: &[Tri], o: V3, d: V3, tmax: f32) -> bool {
        if tris.is_empty() {
            return false;
        }
        let inv = d.map(|v| {
            if v.abs() > 1e-12 {
                1.0 / v
            } else {
                1e12f32.copysign(v)
            }
        });
        let mut stack = [0u32; 64];
        let mut sp = 1;
        while sp > 0 {
            sp -= 1;
            let n = self.nodes[stack[sp] as usize];
            if slab(n.min, n.max, o, inv, tmax).is_none() {
                continue;
            }
            if n.count > 0 {
                for &ti in &self.order[n.first as usize..(n.first + n.count) as usize] {
                    if hit_tri(o, d, &tris[ti as usize]).is_some_and(|(t, _, _)| t < tmax) {
                        return true;
                    }
                }
            } else if sp + 2 <= stack.len() {
                stack[sp] = n.first;
                stack[sp + 1] = n.first + 1;
                sp += 2;
            }
        }
        false
    }
}

/// 光線と箱の交わりの入る距離(当たらなければ `None`)。
fn slab(min: V3, max: V3, o: V3, inv: V3, tmax: f32) -> Option<f32> {
    let mut t0 = 0.0f32;
    let mut t1 = tmax;
    for k in 0..3 {
        let a = (min[k] - o[k]) * inv[k];
        let b = (max[k] - o[k]) * inv[k];
        let (a, b) = if a < b { (a, b) } else { (b, a) };
        t0 = t0.max(a);
        t1 = t1.min(b);
        if t0 > t1 * 1.000_001 + 1e-6 {
            return None;
        }
    }
    Some(t0)
}
