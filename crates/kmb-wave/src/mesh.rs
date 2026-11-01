//! 表面の三角形の網と、要素の形の値。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::job::WaveError;

/// 閉じた三角形の網(m)。頂点の並びは外から見て左回り(法線が外向き)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct BemMesh {
    pub vertices: Vec<[f64; 3]>,
    pub triangles: Vec<[u32; 3]>,
    /// 三角形ごとの面の組の番号(境界の種類を引く)。空なら全部 0
    #[serde(default)]
    pub groups: Vec<u32>,
}

/// 平らな三角形の要素 1 つの形。
#[derive(Debug, Clone, Copy)]
pub(crate) struct Elem {
    pub v: [[f64; 3]; 3],
    /// 重心(選点)
    pub c: [f64; 3],
    /// 外向きの単位法線
    pub n: [f64; 3],
    pub area: f64,
    /// 一番長い辺
    pub size: f64,
}

impl Elem {
    pub fn new(v: [[f64; 3]; 3]) -> Self {
        let e1 = sub(v[1], v[0]);
        let e2 = sub(v[2], v[0]);
        let cr = cross(e1, e2);
        let a2 = norm(cr);
        let n = if a2 > 0.0 { scale(cr, 1.0 / a2) } else { cr };
        let c = scale(add(add(v[0], v[1]), v[2]), 1.0 / 3.0);
        let size = norm(e1).max(norm(e2)).max(norm(sub(v[2], v[1])));
        Self {
            v,
            c,
            n,
            area: 0.5 * a2,
            size,
        }
    }
}

impl BemMesh {
    /// 要素の形を作り、網が閉じていて向きがそろい、外向きかを確かめる。
    pub(crate) fn elements(&self) -> Result<Vec<Elem>, WaveError> {
        if self.triangles.is_empty() {
            return Err(WaveError::input("網に三角形がありません"));
        }
        if !self.groups.is_empty() && self.groups.len() != self.triangles.len() {
            return Err(WaveError::input(format!(
                "面の組の数({})が三角形の数({})と合いません",
                self.groups.len(),
                self.triangles.len()
            )));
        }
        let nv = self.vertices.len();
        let mut elems = Vec::with_capacity(self.triangles.len());
        // 向きのある辺の数。閉じて向きがそろった網なら、どの辺も (a,b) と (b,a) が 1 本ずつ
        let mut edges: HashMap<(u32, u32), i32> = HashMap::new();
        let mut volume = 0.0;
        for (t, tri) in self.triangles.iter().enumerate() {
            if tri.iter().any(|&i| i as usize >= nv) {
                return Err(WaveError::input(format!(
                    "三角形 {t} が無い頂点を指しています"
                )));
            }
            let v = tri.map(|i| self.vertices[i as usize]);
            if v.iter().flatten().any(|x| !x.is_finite()) {
                return Err(WaveError::input(format!(
                    "三角形 {t} の頂点に数でない値があります"
                )));
            }
            let e = Elem::new(v);
            if e.area <= 1e-14 * e.size * e.size || e.area == 0.0 {
                return Err(WaveError::input(format!(
                    "三角形 {t} が潰れています(面積が 0)"
                )));
            }
            volume += dot(v[0], cross(v[1], v[2])) / 6.0;
            for k in 0..3 {
                let (a, b) = (tri[k], tri[(k + 1) % 3]);
                *edges.entry((a, b)).or_default() += 1;
            }
            elems.push(e);
        }
        for (&(a, b), &count) in &edges {
            if count != 1 || edges.get(&(b, a)) != Some(&1) {
                return Err(WaveError::input(format!(
                    "網が閉じていないか、三角形の向きがそろっていません(頂点 {a}–{b} の辺)"
                )));
            }
        }
        if volume <= 0.0 {
            return Err(WaveError::input(
                "網の法線が内向きです(頂点の並びを外から見て左回りにしてください)",
            ));
        }
        Ok(elems)
    }
}

/// 半径 `radius` の球を正 20 面体から `subdivisions` 回分けて作る(三角形は 20·4^回)。
/// 頂点は球の上に乗り、法線は外向き。確かめと見本の網に使う。
pub fn icosphere(radius: f64, subdivisions: u32) -> BemMesh {
    let t = (1.0 + 5f64.sqrt()) / 2.0;
    let mut vertices: Vec<[f64; 3]> = [
        [-1.0, t, 0.0],
        [1.0, t, 0.0],
        [-1.0, -t, 0.0],
        [1.0, -t, 0.0],
        [0.0, -1.0, t],
        [0.0, 1.0, t],
        [0.0, -1.0, -t],
        [0.0, 1.0, -t],
        [t, 0.0, -1.0],
        [t, 0.0, 1.0],
        [-t, 0.0, -1.0],
        [-t, 0.0, 1.0],
    ]
    .map(unit)
    .to_vec();
    let mut triangles: Vec<[u32; 3]> = vec![
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    for _ in 0..subdivisions {
        let mut mid: HashMap<(u32, u32), u32> = HashMap::new();
        let mut next = Vec::with_capacity(triangles.len() * 4);
        let mut midpoint = |a: u32, b: u32, vertices: &mut Vec<[f64; 3]>| {
            let key = (a.min(b), a.max(b));
            *mid.entry(key).or_insert_with(|| {
                let p = unit(add(vertices[a as usize], vertices[b as usize]));
                vertices.push(p);
                (vertices.len() - 1) as u32
            })
        };
        for [a, b, c] in triangles {
            let ab = midpoint(a, b, &mut vertices);
            let bc = midpoint(b, c, &mut vertices);
            let ca = midpoint(c, a, &mut vertices);
            next.extend([[a, ab, ca], [b, bc, ab], [c, ca, bc], [ab, bc, ca]]);
        }
        triangles = next;
    }
    BemMesh {
        vertices: vertices.into_iter().map(|p| scale(p, radius)).collect(),
        triangles,
        groups: vec![],
    }
}

pub(crate) fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub(crate) fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub(crate) fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}
pub(crate) fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub(crate) fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub(crate) fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}
pub(crate) fn unit(a: [f64; 3]) -> [f64; 3] {
    scale(a, 1.0 / norm(a))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icosphere_is_closed_and_outward() {
        let m = icosphere(2.0, 2);
        assert_eq!(m.triangles.len(), 320);
        let e = m.elements().unwrap();
        let area: f64 = e.iter().map(|e| e.area).sum();
        // 内接する多面体なので球の面積より少し小さい
        let sphere = 4.0 * std::f64::consts::PI * 4.0;
        assert!(area < sphere && area > 0.98 * sphere, "{area} / {sphere}");
        assert!(e.iter().all(|e| dot(e.n, e.c) > 0.0));
    }

    #[test]
    fn flipped_or_open_meshes_are_input_errors() {
        let mut m = icosphere(1.0, 0);
        for t in &mut m.triangles {
            t.swap(1, 2);
        }
        assert!(m.elements().unwrap_err().message.contains("内向き"));
        let mut m = icosphere(1.0, 0);
        m.triangles.pop();
        assert!(m.elements().unwrap_err().message.contains("閉じていない"));
    }
}
