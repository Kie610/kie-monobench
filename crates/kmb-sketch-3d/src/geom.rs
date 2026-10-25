//! 3 次元の小さな計算(長さ、角度、輪の法線と重心)。足し引き・内積・外積はコアの物を使う。

use crate::P3;

pub use kmb_core::kernel::{add, cross, dot, scale, sub};

pub fn len(a: P3) -> f64 {
    dot(a, a).sqrt()
}

/// 長さ 1 にする(長さ 0 ならそのまま)。
pub fn unit(a: P3) -> P3 {
    let l = len(a);
    if l > 1e-15 { scale(a, 1.0 / l) } else { a }
}

/// 2 本の向きの間の角度(0〜π)。
pub fn angle(a: P3, b: P3) -> f64 {
    len(cross(a, b)).atan2(dot(a, b))
}

/// 輪の点の並びの法線(ニューウェルの方法。長さは面積の 2 倍)。
pub fn newell(pts: &[P3]) -> P3 {
    let mut n = [0.0; 3];
    for i in 0..pts.len() {
        let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
        n[0] += (p[1] - q[1]) * (p[2] + q[2]);
        n[1] += (p[2] - q[2]) * (p[0] + q[0]);
        n[2] += (p[0] - q[0]) * (p[1] + q[1]);
    }
    n
}

/// 点の並びの重心。
pub fn centroid(pts: &[P3]) -> P3 {
    let mut c = [0.0; 3];
    for p in pts {
        c = add(c, *p);
    }
    scale(c, 1.0 / pts.len().max(1) as f64)
}
