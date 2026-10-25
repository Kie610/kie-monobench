//! 3 次元の点の計算と 4×4 の変換行列。

use super::*;

/// 4×4 の変換行列(行優先、`m[行][列]`)。
pub type Matrix4 = [[f64; 4]; 4];

pub const IDENTITY: Matrix4 = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

pub fn translation(v: P3) -> Matrix4 {
    let mut m = IDENTITY;
    m[0][3] = v[0];
    m[1][3] = v[1];
    m[2][3] = v[2];
    m
}

/// 点 `origin` を通る軸 `axis` まわりに `angle` ラジアン回す行列。
pub fn rotation(origin: P3, axis: P3, angle: f64) -> Matrix4 {
    let [x, y, z] = normalize(axis);
    let (s, c) = angle.sin_cos();
    let t = 1.0 - c;
    let r = [
        [t * x * x + c, t * x * y - s * z, t * x * z + s * y],
        [t * x * y + s * z, t * y * y + c, t * y * z - s * x],
        [t * x * z - s * y, t * y * z + s * x, t * z * z + c],
    ];
    let mut m = IDENTITY;
    for i in 0..3 {
        m[i][..3].copy_from_slice(&r[i]);
        // 平行移動 = origin - R * origin
        m[i][3] = origin[i] - dot(r[i], origin);
    }
    m
}

/// 形を置く行列として使えるかを確かめる。使えなければ理由を返す。最後の行が (0, 0, 0, 1) でない
/// (遠近の成分がある)、数でない値がある、向きの部分がつぶれている(行列式が 0)行列は、面を作っても
/// 網や体積が意味をなさない(足し引きが 30 分かかった)。
/// 行列は保存したファイルや MCP の引数から来るので、カーネルは移動の前にこれで断る。拡大縮小と
/// 鏡映(行列式が負)は通す。
pub fn placement_problem(m: &Matrix4) -> Option<&'static str> {
    if m.iter().flatten().any(|v| !v.is_finite()) {
        return Some("移動の行列に数でない値があります");
    }
    if m[3] != [0.0, 0.0, 0.0, 1.0] {
        return Some("移動の行列の最後の行が (0, 0, 0, 1) ではありません(遠近の成分は使えません)");
    }
    let r = |i: usize, j: usize| m[i][j];
    let det = r(0, 0) * (r(1, 1) * r(2, 2) - r(1, 2) * r(2, 1))
        - r(0, 1) * (r(1, 0) * r(2, 2) - r(1, 2) * r(2, 0))
        + r(0, 2) * (r(1, 0) * r(2, 1) - r(1, 1) * r(2, 0));
    // 向きの部分の大きさに合わせて比べる(列の長さの積で割ると、つぶれ方の割合になる)
    let len = |j: usize| (r(0, j).powi(2) + r(1, j).powi(2) + r(2, j).powi(2)).sqrt();
    let size = len(0) * len(1) * len(2);
    if size == 0.0 || det.abs() <= 1e-12 * size {
        return Some("移動の行列がつぶれています(向きの部分の行列式が 0)");
    }
    None
}

/// 点に変換行列をかける。
pub fn transform_point(m: &Matrix4, p: P3) -> P3 {
    let mut out = [0.0; 3];
    for (i, o) in out.iter_mut().enumerate() {
        *o = m[i][0] * p[0] + m[i][1] * p[1] + m[i][2] * p[2] + m[i][3];
    }
    out
}

pub fn add(a: P3, b: P3) -> P3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub fn scale(a: P3, s: f64) -> P3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

pub fn dot(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn cross(a: P3, b: P3) -> P3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub fn normalize(a: P3) -> P3 {
    let l = dot(a, a).sqrt();
    if l < 1e-300 { a } else { scale(a, 1.0 / l) }
}
