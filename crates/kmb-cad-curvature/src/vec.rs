//! 3 次元のベクトルの計算。足し引き・内積・外積はコアの物を使い、ここには長さと、長さが
//! ほぼ 0 の時に向きを決めない単位化だけを置く(コアの `normalize` はそのまま返すので、
//! 曲率の向きが決まらない所を見分けられない)。

use kmb_core::kernel::P3;
pub use kmb_core::kernel::{add, cross, dot, scale, sub};

pub fn norm(a: P3) -> f64 {
    dot(a, a).sqrt()
}

/// 長さ 1 にした向き。長さがほぼ 0 なら `None`。
pub fn unit(a: P3) -> Option<P3> {
    let n = norm(a);
    (n > 1e-12).then(|| scale(a, 1.0 / n))
}
