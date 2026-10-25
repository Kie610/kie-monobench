//! 網の表面の三角形(6 節点)を、ボディの面(表示用の網の面の名前)に結び付ける。固定や荷重は
//! ボディの面の名前で持つので、形を直しても同じ面に付き直る。
//!
//! 面の力・熱・空気へ逃げる熱は、曲がった 6 節点の三角形の上で積分して節点へ配る
//! (整合した節点の力。2 次の要素で角と辺の中点へ等分すると、角に逆向きの力が要るので
//! 合わない)。

use crate::geom::{P3, closest, dist, dot, norm, scale, sub};
use crate::tetmesh::{TetMesh, build, face_gauss, face_shape};
use kmb_core::Mesh;

/// 網と、ボディの面の情報。
#[derive(Debug, Clone)]
pub struct Prepared {
    pub mesh: TetMesh,
    pub face_names: Vec<String>,
    /// 面ごとの、ボディの網での面積(mm²)
    pub face_area: Vec<f64>,
    /// ボディの網の三角形(面ごと。小さな面に近い表面の三角形を探すため)
    tris: Vec<([P3; 3], u32)>,
}

/// 6 節点の三角形の上の積分点ごとの、形関数・外向きの単位法線・面積の重み。
pub fn face_quadrature(x: &[P3; 6]) -> Vec<([f64; 6], P3, f64)> {
    face_gauss()
        .iter()
        .map(|(r, s, w)| {
            let (n, j) = face_shape(x, *r, *s);
            let l = norm(j);
            let u = if l > 0.0 { scale(j, 1.0 / l) } else { j };
            (n, u, l * w)
        })
        .collect()
}

impl Prepared {
    /// ボディの網 `mesh` を、格子の間隔 `h` の四面体の網にする。
    pub fn new(mesh: &Mesh, h: f64, cancelled: &dyn Fn() -> bool) -> Result<Self, String> {
        Ok(Self::from_tets(mesh, build(mesh, h, cancelled)?))
    }

    /// 作った網 `tm` とボディの網 `mesh` から。
    pub fn from_tets(mesh: &Mesh, tm: TetMesh) -> Self {
        let at = |i: u32| mesh.positions[i as usize].map(f64::from);
        let mut face_area = vec![0.0; mesh.face_names.len()];
        let mut tris = Vec::with_capacity(mesh.triangles.len());
        for (t, f) in mesh.triangles.iter().zip(&mesh.triangle_faces) {
            let p = t.map(at);
            let n = crate::geom::cross(sub(p[1], p[0]), sub(p[2], p[0]));
            if let Some(a) = face_area.get_mut(*f as usize) {
                *a += norm(n) / 2.0;
            }
            tris.push((p, *f));
        }
        Prepared {
            mesh: tm,
            face_names: mesh.face_names.clone(),
            face_area,
            tris,
        }
    }

    pub fn h(&self) -> f64 {
        self.mesh.stats.h
    }

    /// 面の名前の番号。
    pub fn face_index(&self, name: &str) -> Result<u32, String> {
        self.face_names
            .iter()
            .position(|n| n == name)
            .map(|i| i as u32)
            .ok_or_else(|| {
                format!(
                    "面 {name} がボディにありません。形を直して面が無くなったら、選び直してください"
                )
            })
    }

    /// 面(いくつでも)に付く網の表面の三角形の番号。面が要素より小さくて当たらない時は、
    /// 面に近い表面の三角形を使う。
    pub fn faces_of(&self, names: &[String]) -> Result<Vec<usize>, String> {
        let mut out = Vec::new();
        for name in names {
            let f = self.face_index(name)?;
            let before = out.len();
            out.extend(
                self.mesh
                    .faces
                    .iter()
                    .enumerate()
                    .filter(|(_, b)| b.face == f)
                    .map(|(i, _)| i),
            );
            if out.len() == before {
                let reach = 0.6 * self.h();
                let mine: Vec<&[P3; 3]> = self
                    .tris
                    .iter()
                    .filter(|t| t.1 == f)
                    .map(|t| &t.0)
                    .collect();
                for i in 0..self.mesh.faces.len() {
                    let x = self.mesh.face_points(i);
                    let c = scale(
                        crate::geom::add(crate::geom::add(x[0], x[1]), x[2]),
                        1.0 / 3.0,
                    );
                    if mine
                        .iter()
                        .any(|t| dist(closest(c, t[0], t[1], t[2]), c) <= reach)
                    {
                        out.push(i);
                    }
                }
            }
            if out.len() == before {
                return Err(format!(
                    "面 {name} が網に乗りません。要素の大きさを小さくしてください"
                ));
            }
        }
        out.sort_unstable();
        out.dedup();
        Ok(out)
    }

    /// 表面の三角形の節点(重なりなし)。
    pub fn nodes_of(&self, faces: &[usize]) -> Vec<u32> {
        let mut v: Vec<u32> = faces
            .iter()
            .flat_map(|&f| self.mesh.faces[f].nodes)
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// 表面の三角形の面積の和(網の曲がった面で)。
    pub fn area(&self, faces: &[usize]) -> f64 {
        faces
            .iter()
            .map(|&f| {
                face_quadrature(&self.mesh.face_points(f))
                    .iter()
                    .map(|q| q.2)
                    .sum::<f64>()
            })
            .sum()
    }

    /// 面に一様な面の量 `per_area(法線)`(ベクトル)をかけた時の節点の力(節点ごとに 3 つ)を
    /// `f` に足す。
    pub fn add_traction(&self, faces: &[usize], per_area: impl Fn(P3) -> P3, f: &mut [f64]) {
        for &fi in faces {
            let b = &self.mesh.faces[fi];
            for (n, u, w) in face_quadrature(&self.mesh.face_points(fi)) {
                let t = per_area(u);
                for (nk, v) in n.iter().zip(b.nodes) {
                    for c in 0..3 {
                        f[3 * v as usize + c] += nk * t[c] * w;
                    }
                }
            }
        }
    }

    /// 面に一様な量 `q`(面積あたり)の節点の値(節点ごとに 1 つ)を `f` に足す。
    pub fn add_flux(&self, faces: &[usize], q: f64, f: &mut [f64]) {
        for &fi in faces {
            let b = &self.mesh.faces[fi];
            for (n, _, w) in face_quadrature(&self.mesh.face_points(fi)) {
                for k in 0..6 {
                    f[b.nodes[k] as usize] += n[k] * q * w;
                }
            }
        }
    }

    /// 平均の外向きの法線(網の面で)。
    pub fn mean_normal(&self, faces: &[usize]) -> P3 {
        let mut s = [0.0; 3];
        for &fi in faces {
            for (_, u, w) in face_quadrature(&self.mesh.face_points(fi)) {
                s = crate::geom::add(s, scale(u, w));
            }
        }
        let l = dot(s, s).sqrt();
        if l > 0.0 { scale(s, 1.0 / l) } else { s }
    }
}
