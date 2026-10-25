//! 部品を機械に置く向き。
//!
//! セットアップは、部品のどの面の向きを上(工具の側)へ向けるか([`Up`])と、上から見て何度回すか
//! (90° 刻み)で部品を置く。裏返し([`crate::Flip`])は、上へ向ける面を選ばない時の古い書き方として
//! そのまま読む。どの置き方も、部品の外形の箱の中心のまわりの回転(行列の成分は 0 と ±1)で表す。

use kmb_core::Mesh;
use serde::{Deserialize, Serialize};

pub type Mat3 = [[f64; 3]; 3];

pub const IDENTITY: Mat3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// 上(工具の側)へ向ける部品の面の向き。部品を作った時の座標の軸で言う。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Up {
    /// 部品の +Z(作ったままの向き)
    #[default]
    PlusZ,
    /// -Z(裏)。X 軸のまわりに 180° 回す
    MinusZ,
    /// +X の側面
    PlusX,
    MinusX,
    /// +Y の側面
    PlusY,
    MinusY,
}

impl Up {
    pub fn is_default(&self) -> bool {
        *self == Up::PlusZ
    }

    /// 部品の座標の点を、選んだ面が +Z を向くように回す行列。
    pub fn matrix(self) -> Mat3 {
        match self {
            Up::PlusZ => IDENTITY,
            Up::MinusZ => [[1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, -1.0]],
            // (x, y, z) → (-z, y, x): +X が +Z へ
            Up::PlusX => [[0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]],
            // (x, y, z) → (z, y, -x): -X が +Z へ
            Up::MinusX => [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]],
            // (x, y, z) → (x, -z, y): +Y が +Z へ
            Up::PlusY => [[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]],
            // (x, y, z) → (x, z, -y): -Y が +Z へ
            Up::MinusY => [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]],
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Up::PlusZ => "上面(作ったままの向き)",
            Up::MinusZ => "下面(裏返す)",
            Up::PlusX => "右の側面(+X)",
            Up::MinusX => "左の側面(-X)",
            Up::PlusY => "奥の側面(+Y)",
            Up::MinusY => "手前の側面(-Y)",
        }
    }
}

/// 上から見て反時計回りに `deg` 度(90 の倍数に丸める)回す行列。
pub fn turn_matrix(deg: f64) -> Mat3 {
    let k = ((deg / 90.0).round() as i64).rem_euclid(4);
    match k {
        0 => IDENTITY,
        1 => [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
        2 => [[-1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0]],
        _ => [[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
    }
}

pub fn mul(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut m = [[0.0; 3]; 3];
    for (i, row) in m.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    m
}

pub fn apply(m: &Mat3, v: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|i| m[i][0] * v[0] + m[i][1] * v[1] + m[i][2] * v[2])
}

pub fn transpose(m: &Mat3) -> Mat3 {
    [0, 1, 2].map(|i| [0, 1, 2].map(|j| m[j][i]))
}

/// 中心 `c` のまわりに `m` で回す(点)。
pub fn rotate_about(m: &Mat3, p: [f64; 3], c: [f64; 3]) -> [f64; 3] {
    let q = apply(m, [p[0] - c[0], p[1] - c[1], p[2] - c[2]]);
    [q[0] + c[0], q[1] + c[1], q[2] + c[2]]
}

/// 網を中心 `c` のまわりに `m` で回す(面の名前と三角形の並びはそのまま)。
pub fn rotate_mesh(m: &Mat3, mesh: &Mesh, c: [f64; 3]) -> Mesh {
    if *m == IDENTITY {
        return mesh.clone();
    }
    let pt = |p: &[f32; 3]| {
        let q = rotate_about(m, [p[0] as f64, p[1] as f64, p[2] as f64], c);
        [q[0] as f32, q[1] as f32, q[2] as f32]
    };
    let dir = |n: &[f32; 3]| {
        let q = apply(m, [n[0] as f64, n[1] as f64, n[2] as f64]);
        [q[0] as f32, q[1] as f32, q[2] as f32]
    };
    Mesh {
        positions: mesh.positions.iter().map(pt).collect(),
        normals: mesh.normals.iter().map(dir).collect(),
        triangles: mesh.triangles.clone(),
        triangle_faces: mesh.triangle_faces.clone(),
        face_names: mesh.face_names.clone(),
        edges: mesh
            .edges
            .iter()
            .map(|e| e.iter().map(pt).collect())
            .collect(),
    }
}

/// 網をつなげる(複数のボディを 1 つのセットアップで削る時)。面の名前は機能ごとに違うので
/// そのまま並べる。
pub fn merge_meshes(meshes: Vec<Mesh>) -> Mesh {
    let mut out = Mesh::default();
    for m in meshes {
        let base = out.positions.len() as u32;
        let face_base = out.face_names.len() as u32;
        out.positions.extend(m.positions);
        out.normals.extend(m.normals);
        out.triangles
            .extend(m.triangles.iter().map(|t| t.map(|i| i + base)));
        out.triangle_faces
            .extend(m.triangle_faces.iter().map(|f| f + face_base));
        out.face_names.extend(m.face_names);
        out.edges.extend(m.edges);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_up_choice_turns_that_side_to_the_top() {
        for (up, n) in [
            (Up::PlusZ, [0.0, 0.0, 1.0]),
            (Up::MinusZ, [0.0, 0.0, -1.0]),
            (Up::PlusX, [1.0, 0.0, 0.0]),
            (Up::MinusX, [-1.0, 0.0, 0.0]),
            (Up::PlusY, [0.0, 1.0, 0.0]),
            (Up::MinusY, [0.0, -1.0, 0.0]),
        ] {
            assert_eq!(apply(&up.matrix(), n), [0.0, 0.0, 1.0], "{up:?}");
            // 回転(行列式 1)で、鏡に映さない
            let m = up.matrix();
            let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
                - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
                + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
            assert_eq!(det, 1.0, "{up:?}");
        }
    }

    #[test]
    fn turning_keeps_z_and_four_quarter_turns_return() {
        let q = turn_matrix(90.0);
        assert_eq!(apply(&q, [1.0, 0.0, 2.0]), [0.0, 1.0, 2.0]);
        let mut m = IDENTITY;
        for _ in 0..4 {
            m = mul(&q, &m);
        }
        assert_eq!(m, IDENTITY);
        assert_eq!(turn_matrix(-90.0), turn_matrix(270.0));
        assert_eq!(mul(&q, &transpose(&q)), IDENTITY);
    }
}
