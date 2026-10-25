//! 平面の向きと位置の小さな計算(足し引き、内積、外積、長さ)。スケッチ(kmb-sketch)も同じ物を使う。

pub type P2 = [f64; 2];

pub fn sub(a: P2, b: P2) -> P2 {
    [a[0] - b[0], a[1] - b[1]]
}

pub fn add(a: P2, b: P2) -> P2 {
    [a[0] + b[0], a[1] + b[1]]
}

pub fn scale(a: P2, k: f64) -> P2 {
    [a[0] * k, a[1] * k]
}

pub fn dot(a: P2, b: P2) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

/// 外積の大きさ(a から b へ反時計回りなら正)。
pub fn cross(a: P2, b: P2) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

pub fn len(a: P2) -> f64 {
    a[0].hypot(a[1])
}

pub fn dist(a: P2, b: P2) -> f64 {
    len(sub(a, b))
}

/// a から b へ `t` の割合だけ進んだ位置。
pub fn lerp(a: P2, b: P2, t: f64) -> P2 {
    add(a, scale(sub(b, a), t))
}

/// 反時計回りに 90° 回す。
pub fn perp(a: P2) -> P2 {
    [-a[1], a[0]]
}
