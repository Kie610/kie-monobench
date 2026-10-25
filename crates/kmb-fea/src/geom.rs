//! 網を作る時の幾何の部品: ベクトルの計算、点と三角形の最も近い点、線分と三角形の交わり、
//! 三角形の木(AABB の二分木。最も近い点と線分の交わりを速く探す)。

pub type P3 = [f64; 3];

pub use kmb_core::kernel::{add, cross, dot, scale, sub};

pub fn norm(a: P3) -> f64 {
    dot(a, a).sqrt()
}
pub fn dist(a: P3, b: P3) -> f64 {
    norm(sub(a, b))
}
pub fn lerp(a: P3, b: P3, t: f64) -> P3 {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}
pub fn unit(a: P3) -> P3 {
    let l = norm(a);
    if l > 0.0 { scale(a, 1.0 / l) } else { a }
}

/// 四面体 abcd の符号つき体積(a→b, a→c, a→d が右手系なら正)。
pub fn tet_volume(a: P3, b: P3, c: P3, d: P3) -> f64 {
    dot(cross(sub(b, a), sub(c, a)), sub(d, a)) / 6.0
}

/// 点 p と三角形 abc の最も近い点(Ericson『Real-Time Collision Detection』5.1.5 の考え方)。
pub fn closest(p: P3, a: P3, b: P3, c: P3) -> P3 {
    let ab = sub(b, a);
    let ac = sub(c, a);
    let ap = sub(p, a);
    let d1 = dot(ab, ap);
    let d2 = dot(ac, ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = sub(p, b);
    let d3 = dot(ab, bp);
    let d4 = dot(ac, bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return add(a, scale(ab, v));
    }
    let cp = sub(p, c);
    let d5 = dot(ab, cp);
    let d6 = dot(ac, cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return add(a, scale(ac, w));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return add(b, scale(sub(c, b), w));
    }
    let denom = 1.0 / (va + vb + vc);
    add(a, add(scale(ab, vb * denom), scale(ac, vc * denom)))
}

/// 点 p と線分 ab の最も近い点と、その線分の上の割合(0〜1)。
pub fn closest_on_segment(p: P3, a: P3, b: P3) -> (P3, f64) {
    let ab = sub(b, a);
    let l2 = dot(ab, ab);
    let t = if l2 > 0.0 {
        (dot(sub(p, a), ab) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (add(a, scale(ab, t)), t)
}

/// 線分 p→q と三角形 abc の交わりの割合 t(0〜1)。縁も含む(Möller・Trumbore)。
pub fn segment_triangle(p: P3, q: P3, a: P3, b: P3, c: P3) -> Option<f64> {
    let d = sub(q, p);
    let e1 = sub(b, a);
    let e2 = sub(c, a);
    let h = cross(d, e2);
    let det = dot(e1, h);
    let scale_ = norm(e1) * norm(e2) * norm(d);
    if det.abs() <= 1e-14 * scale_ {
        return None;
    }
    let inv = 1.0 / det;
    let s = sub(p, a);
    let u = dot(s, h) * inv;
    if !(-1e-12..=1.0 + 1e-12).contains(&u) {
        return None;
    }
    let qv = cross(s, e1);
    let v = dot(d, qv) * inv;
    if v < -1e-12 || u + v > 1.0 + 1e-12 {
        return None;
    }
    let t = dot(e2, qv) * inv;
    (-1e-12..=1.0 + 1e-12)
        .contains(&t)
        .then_some(t.clamp(0.0, 1.0))
}

#[derive(Debug, Clone, Copy)]
struct Node {
    lo: P3,
    hi: P3,
    /// 葉なら三角形の範囲 [start, start+count)、枝なら子の番号 start(左)と start+1(右)
    start: u32,
    count: u32,
}

/// 三角形(面の番号つき)の AABB の二分木。
#[derive(Debug, Clone, Default)]
pub struct Tree {
    pub tris: Vec<[P3; 3]>,
    pub face: Vec<u32>,
    order: Vec<u32>,
    nodes: Vec<Node>,
}

fn box_of(t: &[P3; 3]) -> (P3, P3) {
    let mut lo = t[0];
    let mut hi = t[0];
    for p in &t[1..] {
        for a in 0..3 {
            lo[a] = lo[a].min(p[a]);
            hi[a] = hi[a].max(p[a]);
        }
    }
    (lo, hi)
}

/// 点と箱の距離の 2 乗。
fn box_dist2(p: P3, lo: P3, hi: P3) -> f64 {
    let mut d = 0.0;
    for a in 0..3 {
        let v = if p[a] < lo[a] {
            lo[a] - p[a]
        } else if p[a] > hi[a] {
            p[a] - hi[a]
        } else {
            0.0
        };
        d += v * v;
    }
    d
}

impl Tree {
    pub fn new(tris: Vec<[P3; 3]>, face: Vec<u32>) -> Self {
        let n = tris.len();
        let mut t = Tree {
            tris,
            face,
            order: (0..n as u32).collect(),
            nodes: Vec::new(),
        };
        if n > 0 {
            let centers: Vec<P3> = t
                .tris
                .iter()
                .map(|x| scale(add(add(x[0], x[1]), x[2]), 1.0 / 3.0))
                .collect();
            t.nodes.push(Node {
                lo: [0.0; 3],
                hi: [0.0; 3],
                start: 0,
                count: n as u32,
            });
            t.split(0, &centers);
        }
        t
    }

    fn split(&mut self, ni: usize, centers: &[P3]) {
        let (s, c) = (self.nodes[ni].start as usize, self.nodes[ni].count as usize);
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for &i in &self.order[s..s + c] {
            let (l, h) = box_of(&self.tris[i as usize]);
            for a in 0..3 {
                lo[a] = lo[a].min(l[a]);
                hi[a] = hi[a].max(h[a]);
            }
        }
        self.nodes[ni].lo = lo;
        self.nodes[ni].hi = hi;
        if c <= 4 {
            return;
        }
        let axis = (0..3)
            .max_by(|a, b| (hi[*a] - lo[*a]).total_cmp(&(hi[*b] - lo[*b])))
            .unwrap_or(0);
        let mid = c / 2;
        self.order[s..s + c].select_nth_unstable_by(mid, |a, b| {
            centers[*a as usize][axis].total_cmp(&centers[*b as usize][axis])
        });
        let left = self.nodes.len();
        self.nodes.push(Node {
            lo,
            hi,
            start: s as u32,
            count: mid as u32,
        });
        self.nodes.push(Node {
            lo,
            hi,
            start: (s + mid) as u32,
            count: (c - mid) as u32,
        });
        self.nodes[ni].start = left as u32;
        self.nodes[ni].count = 0;
        self.split(left, centers);
        self.split(left + 1, centers);
    }

    pub fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }

    /// 点 p に最も近い三角形の上の点(`keep` が真の三角形だけ)。(距離、点、三角形の番号)。
    pub fn nearest(&self, p: P3, keep: impl Fn(usize) -> bool) -> Option<(f64, P3, usize)> {
        self.nearest_within(p, f64::INFINITY, keep)
    }

    /// `nearest` の、距離 r より近い物だけを探す形。
    pub fn nearest_within(
        &self,
        p: P3,
        r: f64,
        keep: impl Fn(usize) -> bool,
    ) -> Option<(f64, P3, usize)> {
        let mut best: Option<(f64, P3, usize)> = None;
        let mut best2 = r * r;
        if self.nodes.is_empty() {
            return None;
        }
        let mut stack = vec![0usize];
        while let Some(ni) = stack.pop() {
            let n = self.nodes[ni];
            if box_dist2(p, n.lo, n.hi) >= best2 {
                continue;
            }
            if n.count > 0 {
                for &ti in &self.order[n.start as usize..(n.start + n.count) as usize] {
                    let ti = ti as usize;
                    if !keep(ti) {
                        continue;
                    }
                    let t = &self.tris[ti];
                    let q = closest(p, t[0], t[1], t[2]);
                    let d2 = dot(sub(q, p), sub(q, p));
                    if d2 < best2 {
                        best2 = d2;
                        best = Some((d2.sqrt(), q, ti));
                    }
                }
            } else {
                let (l, r) = (n.start as usize, n.start as usize + 1);
                let dl = box_dist2(p, self.nodes[l].lo, self.nodes[l].hi);
                let dr = box_dist2(p, self.nodes[r].lo, self.nodes[r].hi);
                // 近い方を後に積む(先に調べる)
                if dl < dr {
                    stack.push(r);
                    stack.push(l);
                } else {
                    stack.push(l);
                    stack.push(r);
                }
            }
        }
        best
    }

    /// 線分 p→q と交わる三角形の割合 t(小さい順、重なりは除かない)。
    pub fn hits(&self, p: P3, q: P3, out: &mut Vec<f64>) {
        out.clear();
        if self.nodes.is_empty() {
            return;
        }
        let mut lo = p;
        let mut hi = p;
        for a in 0..3 {
            lo[a] = lo[a].min(q[a]);
            hi[a] = hi[a].max(q[a]);
        }
        let mut stack = vec![0usize];
        while let Some(ni) = stack.pop() {
            let n = self.nodes[ni];
            if (0..3).any(|a| n.hi[a] < lo[a] || n.lo[a] > hi[a]) {
                continue;
            }
            if n.count > 0 {
                for &ti in &self.order[n.start as usize..(n.start + n.count) as usize] {
                    let t = &self.tris[ti as usize];
                    if let Some(s) = segment_triangle(p, q, t[0], t[1], t[2]) {
                        out.push(s);
                    }
                }
            } else {
                stack.push(n.start as usize);
                stack.push(n.start as usize + 1);
            }
        }
        out.sort_by(f64::total_cmp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_point_on_two_triangles() {
        let t = Tree::new(
            vec![
                [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
                [[0., 0., 5.], [1., 0., 5.], [0., 1., 5.]],
            ],
            vec![0, 1],
        );
        let (d, q, i) = t.nearest([0.2, 0.2, 4.0], |_| true).unwrap();
        assert_eq!(i, 1);
        assert!((d - 1.0).abs() < 1e-12 && (q[2] - 5.0).abs() < 1e-12);
        let (_, _, i) = t.nearest([0.2, 0.2, 4.0], |i| i == 0).unwrap();
        assert_eq!(i, 0);
        let mut hits = Vec::new();
        t.hits([0.1, 0.1, -1.0], [0.1, 0.1, 9.0], &mut hits);
        assert_eq!(hits.len(), 2);
        assert!((hits[0] - 0.1).abs() < 1e-12 && (hits[1] - 0.6).abs() < 1e-12);
    }
}
