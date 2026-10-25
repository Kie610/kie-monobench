//! 流れの格子に置く形。ボディの網を「流れの座標」(x が流れの向き、z が断面の法線)へ並べ替え、
//! 点が中か(z の向きの直線と三角形の交わりの偶奇)と、格子の線が面に当たる所(壁までの距離)を
//! 調べる。三角形は XY の升目に振り分けて絞る。

use kmb_core::Mesh;

pub type P3 = [f64; 3];

/// 流れの座標と世界の座標の対応。流れの座標の軸 a は世界の軸 `axes[a]` に `signs[a]` を掛けた物。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub axes: [usize; 3],
    pub signs: [f64; 3],
}

impl Frame {
    /// 流れの向き(世界の軸 `flow` と符号 `sign`)と、断面の法線の軸 `normal`。
    pub fn new(flow: usize, sign: f64, normal: usize) -> Frame {
        let third = 3 - flow - normal;
        Frame {
            axes: [flow, third, normal],
            signs: [sign, 1.0, 1.0],
        }
    }

    pub fn to_local(&self, p: P3) -> P3 {
        [0, 1, 2].map(|a| p[self.axes[a]] * self.signs[a])
    }

    pub fn to_world(&self, p: P3) -> P3 {
        let mut w = [0.0; 3];
        for a in 0..3 {
            w[self.axes[a]] = p[a] * self.signs[a];
        }
        w
    }

    /// 流れの座標の向きを世界の向きへ(力・速さ)。
    pub fn vec_to_world(&self, v: [f64; 3]) -> [f64; 3] {
        self.to_world(v)
    }
}

/// 形(三角形の集まり)。
pub struct Shape {
    pub tris: Vec<[P3; 3]>,
    pub lo: P3,
    pub hi: P3,
    cell: f64,
    nb: [usize; 2],
    buckets: Vec<Vec<u32>>,
}

impl Shape {
    /// 世界の座標の網を、流れの座標へ並べ替えて持つ。
    pub fn new(meshes: &[&Mesh], frame: &Frame) -> Option<Shape> {
        let mut tris = Vec::new();
        for m in meshes {
            let at = |i: u32| frame.to_local(m.positions[i as usize].map(f64::from));
            for t in &m.triangles {
                tris.push([at(t[0]), at(t[1]), at(t[2])]);
            }
        }
        if tris.is_empty() {
            return None;
        }
        let mut lo = [f64::MAX; 3];
        let mut hi = [f64::MIN; 3];
        for t in &tris {
            for p in t {
                for a in 0..3 {
                    lo[a] = lo[a].min(p[a]);
                    hi[a] = hi[a].max(p[a]);
                }
            }
        }
        // 升目は三角形の数に合わせる(1 升に数個)
        let area = ((hi[0] - lo[0]) * (hi[1] - lo[1])).max(1e-12);
        let cell = (area / (tris.len() as f64 / 4.0).max(1.0)).sqrt().max(1e-6);
        let nb = [
            (((hi[0] - lo[0]) / cell).ceil() as usize).clamp(1, 2048),
            (((hi[1] - lo[1]) / cell).ceil() as usize).clamp(1, 2048),
        ];
        let mut s = Shape {
            tris,
            lo,
            hi,
            cell,
            nb,
            buckets: vec![Vec::new(); nb[0] * nb[1]],
        };
        for (ti, t) in s.tris.iter().enumerate() {
            let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
            for p in t {
                x0 = x0.min(p[0]);
                x1 = x1.max(p[0]);
                y0 = y0.min(p[1]);
                y1 = y1.max(p[1]);
            }
            let (i0, i1) = (s.bin(x0, 0), s.bin(x1, 0));
            let (j0, j1) = (s.bin(y0, 1), s.bin(y1, 1));
            for j in j0..=j1 {
                for i in i0..=i1 {
                    s.buckets[i + nb[0] * j].push(ti as u32);
                }
            }
        }
        Some(s)
    }

    fn bin(&self, v: f64, a: usize) -> usize {
        (((v - self.lo[a]) / self.cell).floor().max(0.0) as usize).min(self.nb[a] - 1)
    }

    /// 直線 (x, y, ·) が面を横切る z(小さい順)。
    pub fn crossings(&self, x: f64, y: f64) -> Vec<f64> {
        let mut zs = Vec::new();
        if x < self.lo[0] || x > self.hi[0] || y < self.lo[1] || y > self.hi[1] {
            return zs;
        }
        // 辺や頂点をかすめないよう少しずらす
        let (x, y) = (x + self.cell * 1.234_567e-5, y + self.cell * 2.345_678e-5);
        for &ti in &self.buckets[self.bin(x, 0) + self.nb[0] * self.bin(y, 1)] {
            let [a, b, c] = self.tris[ti as usize];
            let d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
            if d.abs() < 1e-18 {
                continue;
            }
            let l1 = ((b[1] - c[1]) * (x - c[0]) + (c[0] - b[0]) * (y - c[1])) / d;
            let l2 = ((c[1] - a[1]) * (x - c[0]) + (a[0] - c[0]) * (y - c[1])) / d;
            let l3 = 1.0 - l1 - l2;
            if l1 >= 0.0 && l2 >= 0.0 && l3 >= 0.0 {
                zs.push(l1 * a[2] + l2 * b[2] + l3 * c[2]);
            }
        }
        zs.sort_by(f64::total_cmp);
        zs
    }

    /// 点が中か。
    pub fn inside(&self, p: P3) -> bool {
        self.crossings(p[0], p[1])
            .iter()
            .filter(|z| **z > p[2])
            .count()
            % 2
            == 1
    }

    /// 線分 a → b が面に最初に当たる所(a からの割合 0〜1)。
    pub fn hit(&self, a: P3, b: P3) -> Option<f64> {
        let (i0, i1) = (self.bin(a[0].min(b[0]), 0), self.bin(a[0].max(b[0]), 0));
        let (j0, j1) = (self.bin(a[1].min(b[1]), 1), self.bin(a[1].max(b[1]), 1));
        let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let mut best: Option<f64> = None;
        for j in j0..=j1 {
            for i in i0..=i1 {
                for &ti in &self.buckets[i + self.nb[0] * j] {
                    if let Some(t) = segment_triangle(a, d, &self.tris[ti as usize])
                        && best.is_none_or(|b| t < b)
                    {
                        best = Some(t);
                    }
                }
            }
        }
        best
    }

    /// 断面 z = const で切った線分(x, y)。
    pub fn slice(&self, z: f64) -> Vec<[[f64; 2]; 2]> {
        let mut out = Vec::new();
        for t in &self.tris {
            let mut pts = Vec::with_capacity(2);
            for e in 0..3 {
                let (p, q) = (t[e], t[(e + 1) % 3]);
                if (p[2] - z) * (q[2] - z) < 0.0 {
                    let s = (z - p[2]) / (q[2] - p[2]);
                    pts.push([p[0] + s * (q[0] - p[0]), p[1] + s * (q[1] - p[1])]);
                }
            }
            if pts.len() == 2 {
                out.push([pts[0], pts[1]]);
            }
        }
        out
    }
}

/// Möller–Trumbore(線分の割合 0〜1 だけ)。
fn segment_triangle(o: P3, d: P3, t: &[P3; 3]) -> Option<f64> {
    let sub = |a: P3, b: P3| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let cross = |a: P3, b: P3| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let dot = |a: P3, b: P3| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let (e1, e2) = (sub(t[1], t[0]), sub(t[2], t[0]));
    let h = cross(d, e2);
    let det = dot(e1, h);
    if det.abs() < 1e-14 {
        return None;
    }
    let inv = 1.0 / det;
    let s = sub(o, t[0]);
    let u = inv * dot(s, h);
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let qv = cross(s, e1);
    let v = inv * dot(d, qv);
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let tt = inv * dot(e2, qv);
    (0.0..=1.0).contains(&tt).then_some(tt)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// 中心 c・大きさ s の箱の網(外向き)。
    pub fn box_mesh(c: P3, s: P3) -> Mesh {
        let mut m = Mesh::default();
        let h = [s[0] / 2.0, s[1] / 2.0, s[2] / 2.0];
        for k in 0..8 {
            let p = [
                c[0] + if k & 1 == 0 { -h[0] } else { h[0] },
                c[1] + if k & 2 == 0 { -h[1] } else { h[1] },
                c[2] + if k & 4 == 0 { -h[2] } else { h[2] },
            ];
            m.positions.push(p.map(|v| v as f32));
        }
        let quads = [
            [0, 2, 3, 1],
            [4, 5, 7, 6],
            [0, 1, 5, 4],
            [2, 6, 7, 3],
            [0, 4, 6, 2],
            [1, 3, 7, 5],
        ];
        for (f, q) in quads.iter().enumerate() {
            m.triangles.push([q[0], q[1], q[2]]);
            m.triangles.push([q[0], q[2], q[3]]);
            m.triangle_faces.extend([f as u32, f as u32]);
            m.face_names.push(format!("f{f}"));
        }
        m
    }

    #[test]
    fn inside_and_hits_of_a_box() {
        let m = box_mesh([0.0, 0.0, 0.0], [2.0, 4.0, 6.0]);
        let s = Shape::new(&[&m], &Frame::new(0, 1.0, 2)).unwrap();
        assert!(s.inside([0.1, 0.2, 0.3]));
        assert!(!s.inside([1.5, 0.0, 0.0]));
        let t = s.hit([2.0, 0.0, 0.0], [0.0, 0.0, 0.0]).unwrap();
        assert!((t - 0.5).abs() < 1e-9);
        assert_eq!(s.slice(0.0).len(), 8);
        // 流れを -Y にすると、流れの座標の x は世界の -y
        let f = Frame::new(1, -1.0, 2);
        assert_eq!(f.to_local([1.0, 2.0, 3.0]), [-2.0, 1.0, 3.0]);
        assert_eq!(f.to_world(f.to_local([1.0, 2.0, 3.0])), [1.0, 2.0, 3.0]);
    }
}
