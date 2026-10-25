//! 穴を削る順。
//!
//! 近い順は、ストックの左手前の角にいちばん近い穴から始め、次はまだ削っていない穴のうち最も近い
//! ものへ進み、最後に 2 本の移動を入れ替えて短くなる所を直す(2-opt)。最短の巡回の近似で、
//! 3 軸の XY の移動だけを見る(穴の向きや機械の軸は見ない)。

use serde::{Deserialize, Serialize};

/// 穴を削る順。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Order {
    /// 移動が短くなる順(最短に近い巡回)
    #[default]
    Nearest,
    /// X の小さい順(同じなら Y)
    X,
    /// Y の小さい順(同じなら X)
    Y,
    /// 選んだ順(3D のビューで選んだ穴、穴の機能の点、スケッチの点の順)
    Selected,
}

fn d(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// 位置 `pts` を削る順の番号の並びにする。`rank` は選んだ順(同じなら位置の順)。
pub fn order(pts: &[[f64; 2]], rank: &[usize], how: Order, start: [f64; 2]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..pts.len()).collect();
    let by_x = |a: &usize, b: &usize| {
        pts[*a][0]
            .total_cmp(&pts[*b][0])
            .then(pts[*a][1].total_cmp(&pts[*b][1]))
    };
    match how {
        Order::X => idx.sort_by(by_x),
        Order::Y => idx.sort_by(|a, b| {
            pts[*a][1]
                .total_cmp(&pts[*b][1])
                .then(pts[*a][0].total_cmp(&pts[*b][0]))
        }),
        Order::Selected => idx.sort_by(|a, b| rank[*a].cmp(&rank[*b]).then(by_x(a, b))),
        Order::Nearest => idx = nearest(pts, start),
    }
    idx
}

fn nearest(pts: &[[f64; 2]], start: [f64; 2]) -> Vec<usize> {
    let n = pts.len();
    let mut left: Vec<usize> = (0..n).collect();
    let mut out = Vec::with_capacity(n);
    let mut cur = start;
    while !left.is_empty() {
        let (k, _) = left
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| d(pts[**a], cur).total_cmp(&d(pts[**b], cur)))
            .expect("残りがある");
        let i = left.remove(k);
        cur = pts[i];
        out.push(i);
    }
    // 2-opt: 区間 i..=j を逆にして、前後のつなぎが短くなれば入れ替える(始まりの穴は動かさない)
    let len = |o: &[usize], a: usize, b: usize| d(pts[o[a]], pts[o[b]]);
    let mut better = true;
    let mut rounds = 0;
    while better && rounds < 50 {
        better = false;
        rounds += 1;
        for i in 1..n.saturating_sub(1) {
            for j in i + 1..n {
                let tail = |a: usize| if j + 1 < n { len(&out, a, j + 1) } else { 0.0 };
                let before = len(&out, i - 1, i) + tail(j);
                let after = len(&out, i - 1, j) + tail(i);
                if after < before - 1e-9 {
                    out[i..=j].reverse();
                    better = true;
                }
            }
        }
    }
    out
}

/// 並べた順で移る長さの合計。
pub fn travel(pts: &[[f64; 2]], idx: &[usize]) -> f64 {
    idx.windows(2).map(|w| d(pts[w[0]], pts[w[1]])).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_order_is_shorter_than_x_order_on_a_zigzag() {
        // 2 列の穴。X の順だと前後に行き来する
        let pts: Vec<[f64; 2]> = (0..6)
            .flat_map(|i| [[i as f64 * 10.0, 0.0], [i as f64 * 10.0, 50.0]])
            .collect();
        let rank: Vec<usize> = (0..pts.len()).collect();
        let x = order(&pts, &rank, Order::X, [0.0, 0.0]);
        let n = order(&pts, &rank, Order::Nearest, [0.0, 0.0]);
        let mut sorted = n.clone();
        sorted.sort();
        assert_eq!(sorted, rank, "全部の穴を 1 回ずつ");
        assert!(
            travel(&pts, &n) < travel(&pts, &x) * 0.6,
            "{} {}",
            travel(&pts, &n),
            travel(&pts, &x)
        );
        assert_eq!(n[0], 0, "左手前から始める");
    }

    #[test]
    fn x_y_and_selected_orders_are_kept() {
        let pts = [[5.0, 1.0], [1.0, 9.0], [1.0, 2.0]];
        let rank = [1, 2, 0];
        assert_eq!(order(&pts, &rank, Order::X, [0.0; 2]), vec![2, 1, 0]);
        assert_eq!(order(&pts, &rank, Order::Y, [0.0; 2]), vec![0, 2, 1]);
        assert_eq!(order(&pts, &rank, Order::Selected, [0.0; 2]), vec![2, 0, 1]);
    }
}
