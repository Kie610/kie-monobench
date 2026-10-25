//! 面を拾うための、ボディの網の三角形を入れた箱の木(BVH)。
//!
//! マウスの下の面は毎フレーム拾うので、三角形を全部写して調べると網が大きいほど遅くなる。
//! 網ごとに一度だけ、三角形を軸に平行な箱で入れ子に分けておき、視線が通る箱の中だけを調べる。
//! 箱の木と視線と三角形の交わりは、教科書にある昔からの手法(Rubin・Whitted 1980、
//! Möller・Trumbore 1997)。

use kmb_core::Mesh;

/// 箱の木の節。`count` が 0 なら中の節で、子は `index + 1` と `first`。
/// 0 より大きければ葉で、`tris[first..first + count]` の三角形を持つ。
#[derive(Clone, Copy, Debug)]
struct Node {
    min: [f32; 3],
    max: [f32; 3],
    first: u32,
    count: u32,
}

/// 1 つの網の箱の木。
#[derive(Debug, Default)]
pub struct Bvh {
    nodes: Vec<Node>,
    /// 三角形の番号(葉ごとにまとまる順)
    tris: Vec<u32>,
}

/// 葉に入れる三角形の数の上限。
const LEAF: usize = 4;

impl Bvh {
    /// 網から作る。三角形の重心の広がりがいちばん長い軸で、半分ずつに分ける。
    pub fn build(m: &Mesh) -> Bvh {
        let n = m.triangles.len();
        let mut b = Bvh {
            nodes: Vec::with_capacity(2 * n / LEAF + 1),
            tris: (0..n as u32).collect(),
        };
        if n == 0 {
            return b;
        }
        let center: Vec<[f32; 3]> = m
            .triangles
            .iter()
            .map(|t| {
                let p = t.map(|i| m.positions[i as usize]);
                [0, 1, 2].map(|k| (p[0][k] + p[1][k] + p[2][k]) / 3.0)
            })
            .collect();
        b.split(m, &center, 0, n);
        b
    }

    /// `tris[lo..hi]` の節を足し(左の子はすぐ後ろ、右の子は左の木の後ろ)、番号を返す。
    fn split(&mut self, m: &Mesh, center: &[[f32; 3]], lo: usize, hi: usize) -> usize {
        let (min, max) = bounds(m, &self.tris[lo..hi]);
        let at = self.nodes.len();
        self.nodes.push(Node {
            min,
            max,
            first: lo as u32,
            count: (hi - lo) as u32,
        });
        if hi - lo <= LEAF {
            return at;
        }
        let (mut cmin, mut cmax) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        for &t in &self.tris[lo..hi] {
            for k in 0..3 {
                cmin[k] = cmin[k].min(center[t as usize][k]);
                cmax[k] = cmax[k].max(center[t as usize][k]);
            }
        }
        let axis = (0..3)
            .max_by(|&a, &c| (cmax[a] - cmin[a]).total_cmp(&(cmax[c] - cmin[c])))
            .unwrap_or(0);
        let mid = (lo + hi) / 2;
        self.tris[lo..hi].select_nth_unstable_by(mid - lo, |&a, &c| {
            center[a as usize][axis].total_cmp(&center[c as usize][axis])
        });
        self.split(m, center, lo, mid);
        let right = self.split(m, center, mid, hi);
        self.nodes[at].first = right as u32;
        self.nodes[at].count = 0;
        at
    }

    /// 視線 `o + t d`(`t_min < t`)と交わる三角形ごとに、`hit(三角形の番号, t)` を呼ぶ。
    /// 三角形は表も裏も調べる。
    pub fn ray(
        &self,
        m: &Mesh,
        o: [f32; 3],
        d: [f32; 3],
        t_min: f32,
        mut hit: impl FnMut(u32, f32),
    ) {
        if self.nodes.is_empty() {
            return;
        }
        let inv = d.map(|v| 1.0 / v);
        let mut stack = vec![0usize];
        while let Some(i) = stack.pop() {
            let node = self.nodes[i];
            if !slab(node.min, node.max, o, d, inv, t_min) {
                continue;
            }
            if node.count == 0 {
                stack.push(node.first as usize);
                stack.push(i + 1);
                continue;
            }
            let (a, b) = (node.first as usize, (node.first + node.count) as usize);
            for &t in &self.tris[a..b] {
                let p = m.triangles[t as usize].map(|k| m.positions[k as usize]);
                if let Some(at) = triangle(p, o, d)
                    && at > t_min
                {
                    hit(t, at);
                }
            }
        }
    }

    /// 木を使わず、三角形を全部調べる [`Self::ray`](木を別のスレッドで作っている間に使う。
    /// 25 万三角形で 6 ms ほど。答えは木を通した時と同じ)。
    pub fn ray_all(m: &Mesh, o: [f32; 3], d: [f32; 3], t_min: f32, mut hit: impl FnMut(u32, f32)) {
        for (t, tri) in m.triangles.iter().enumerate() {
            let p = tri.map(|k| m.positions[k as usize]);
            if let Some(at) = triangle(p, o, d)
                && at > t_min
            {
                hit(t as u32, at);
            }
        }
    }
}

fn bounds(m: &Mesh, tris: &[u32]) -> ([f32; 3], [f32; 3]) {
    let (mut min, mut max) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    for &t in tris {
        for &i in &m.triangles[t as usize] {
            let p = m.positions[i as usize];
            for k in 0..3 {
                min[k] = min[k].min(p[k]);
                max[k] = max[k].max(p[k]);
            }
        }
    }
    (min, max)
}

/// 視線が箱を通るか(`t_min` より先で)。向きの成分が 0 の軸は、始点が箱の幅に入っているかで見る。
fn slab(min: [f32; 3], max: [f32; 3], o: [f32; 3], d: [f32; 3], inv: [f32; 3], t_min: f32) -> bool {
    // 平らな箱でも当たるよう、少しだけ太らせる
    let pad = 1e-4 * (0..3).map(|k| max[k] - min[k]).fold(1.0f32, f32::max);
    let (mut lo, mut hi) = (t_min, f32::INFINITY);
    for k in 0..3 {
        let (a, b) = (min[k] - pad, max[k] + pad);
        if d[k] == 0.0 {
            if o[k] < a || o[k] > b {
                return false;
            }
            continue;
        }
        let (t0, t1) = ((a - o[k]) * inv[k], (b - o[k]) * inv[k]);
        let (t0, t1) = if t0 <= t1 { (t0, t1) } else { (t1, t0) };
        lo = lo.max(t0);
        hi = hi.min(t1);
        if lo > hi {
            return false;
        }
    }
    true
}

/// 視線と三角形の交わる所の `t`(Möller・Trumbore。縁の上も当たりにする)。
fn triangle(p: [[f32; 3]; 3], o: [f32; 3], d: [f32; 3]) -> Option<f32> {
    let e1 = sub(p[1], p[0]);
    let e2 = sub(p[2], p[0]);
    let h = cross(d, e2);
    let det = dot(e1, h);
    // 視線に平行な三角形(画面の上で線になる)は拾わない
    let scale = dot(e1, e1).max(dot(e2, e2));
    if det.abs() <= 1e-9 * scale {
        return None;
    }
    let f = 1.0 / det;
    let s = sub(o, p[0]);
    let u = f * dot(s, h);
    const EPS: f32 = 1e-5;
    if !(-EPS..=1.0 + EPS).contains(&u) {
        return None;
    }
    let q = cross(s, e1);
    let v = f * dot(d, q);
    if v < -EPS || u + v > 1.0 + EPS {
        return None;
    }
    Some(f * dot(e2, q))
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 細かく分けた板(上下の面)。`n × n` の升に 2 つずつの三角形。
    fn plate(n: usize) -> Mesh {
        let mut m = Mesh::default();
        for z in [0.0f32, 1.0] {
            let base = m.positions.len() as u32;
            for j in 0..=n {
                for i in 0..=n {
                    m.positions.push([i as f32, j as f32, z]);
                    m.normals.push([0.0, 0.0, 1.0]);
                }
            }
            let w = (n + 1) as u32;
            for j in 0..n as u32 {
                for i in 0..n as u32 {
                    let a = base + j * w + i;
                    m.triangles.push([a, a + 1, a + w + 1]);
                    m.triangles.push([a, a + w + 1, a + w]);
                }
            }
        }
        m
    }

    /// 全部の三角形を調べた答えと、木を通した答えが同じになる。
    #[test]
    fn the_tree_finds_the_same_hits_as_checking_every_triangle() {
        let m = plate(37);
        let bvh = Bvh::build(&m);
        // どの三角形も、どこかの葉にちょうど 1 回入る
        let mut seen = vec![0; m.triangles.len()];
        for t in &bvh.tris {
            seen[*t as usize] += 1;
        }
        assert!(seen.iter().all(|&c| c == 1));
        let rays = [
            ([3.3, 4.7, 5.0], [0.0, 0.0, -1.0]),
            ([10.0, 10.0, 10.0], [0.3, 0.2, -1.0]),
            ([-5.0, 18.2, 0.5], [1.0, 0.0, 0.0]),
            ([5.0, 5.0, 5.0], [0.0, 0.0, 1.0]),
            ([12.0, 12.0, 0.5], [0.4, -0.7, 0.6]),
        ];
        for (o, d) in rays {
            for t_min in [f32::NEG_INFINITY, 0.0] {
                let mut want: Vec<(u32, f32)> = (0..m.triangles.len() as u32)
                    .filter_map(|t| {
                        let p = m.triangles[t as usize].map(|k| m.positions[k as usize]);
                        triangle(p, o, d).filter(|at| *at > t_min).map(|at| (t, at))
                    })
                    .collect();
                let mut all = Vec::new();
                Bvh::ray_all(&m, o, d, t_min, |t, at| all.push((t, at)));
                assert_eq!(all, want);
                let mut got = Vec::new();
                bvh.ray(&m, o, d, t_min, |t, at| got.push((t, at)));
                want.sort_by_key(|h| h.0);
                got.sort_by_key(|h| h.0);
                assert_eq!(got, want, "{o:?} {d:?} {t_min}");
            }
        }
    }

    /// 大きな網の箱の木を作る時間と、木を使わずに全部調べる時間を測る:
    /// `cargo test -p kmb-ui-view3d --release tree_build_speed -- --ignored --nocapture`。
    #[test]
    #[ignore = "速さを測る道具(--release で手で流す)"]
    fn tree_build_speed() {
        for n in [70usize, 250, 500] {
            let m = plate(n);
            let t = std::time::Instant::now();
            let bvh = Bvh::build(&m);
            let build = t.elapsed().as_secs_f64() * 1000.0;
            let (o, d) = ([n as f32 / 3.0, n as f32 / 2.0, 5.0], [0.1, 0.05, -1.0]);
            let t = std::time::Instant::now();
            let mut k = 0;
            for _ in 0..20 {
                Bvh::ray_all(&m, o, d, 0.0, |_, _| k += 1);
            }
            let all = t.elapsed().as_secs_f64() * 1000.0 / 20.0;
            let t = std::time::Instant::now();
            for _ in 0..20 {
                bvh.ray(&m, o, d, 0.0, |_, _| k += 1);
            }
            let tree = t.elapsed().as_secs_f64() * 1000.0 / 20.0;
            eprintln!(
                "三角形 {}: 木を作る {build:.1} ms、全部調べる 1 回 {all:.3} ms、木で 1 回 {tree:.4} ms({k})",
                m.triangles.len()
            );
        }
    }
}
