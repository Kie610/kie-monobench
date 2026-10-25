//! 閉じた折れ線の輪(点の列。最後の点から最初の点へ戻る)をたどる小さな部品。
//! 輪を混ぜながら下りる経路(モーフィング スパイラル・ランプ)が、2 つの輪の始まりをそろえ、
//! 弧長の割合で同じ位置の点を取るのに使う。

use crate::P2;

/// 2 点の距離。
pub fn dist(a: P2, b: P2) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// 各点までの弧長(長さは点の数 + 1。最後は 1 周の長さ)。
pub fn cumulative(pts: &[P2]) -> Vec<f64> {
    let n = pts.len();
    let mut c = vec![0.0; n + 1];
    for i in 0..n {
        c[i + 1] = c[i] + dist(pts[i], pts[(i + 1) % n]);
    }
    c
}

/// 始まりから弧長の割合 `t`(0〜1)の点。`cum` は [`cumulative`] の弧長。
pub fn at(pts: &[P2], cum: &[f64], t: f64) -> P2 {
    let s = t.rem_euclid(1.0) * cum[pts.len()];
    let i = cum
        .partition_point(|&c| c <= s)
        .saturating_sub(1)
        .min(pts.len() - 1);
    let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
    let seg = cum[i + 1] - cum[i];
    let u = if seg < 1e-12 { 0.0 } else { (s - cum[i]) / seg };
    [a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u]
}

/// `q` にいちばん近い点の番号(空なら 0)。
pub fn nearest(pts: &[P2], q: P2) -> usize {
    (0..pts.len())
        .min_by(|&a, &b| dist(pts[a], q).total_cmp(&dist(pts[b], q)))
        .unwrap_or(0)
}

/// 輪を `start` の点から始まるよう回す。
pub fn rotate(pts: &[P2], start: usize) -> Vec<P2> {
    (0..pts.len())
        .map(|m| pts[(start + m) % pts.len()])
        .collect()
}
