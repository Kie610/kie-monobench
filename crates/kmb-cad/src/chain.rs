//! 接してつながる辺をたどる(フィレット・面取りの「つながるエッジも入れる」と、辺のダブルクリック)。
//!
//! 辺の端の点が重なり、そこで向きがほぼ同じ(許しの角度以下)辺へ進む。進むのは、カーネルが角と
//! 答えた辺([`Seam::Corner`])だけ。なめらかな継ぎ目には丸める角が無いので進まず、
//! カーネルが分からない辺([`Seam::Unknown`])でも黙って進まずに止まる。
//! 形の似た辺を探して足すことはしない(docs/避ける形.md)。

use kmb_core::kernel::{EdgeInfo, Seam, dot, sub};
use std::collections::{HashMap, HashSet};

/// つながると見なす、端の向きの角度の既定(度)。
pub const TANGENT_DEG: f64 = 2.0;
/// 端の点が同じと見なす距離(mm)
const POINT_TOL: f64 = 1e-3;

/// `seed` 番目の辺から、接してつながる辺を両側へたどった辺の番号(`seed` を先頭に、たどった順)。
/// 分かれ道では角度が一番小さい辺だけへ進む。一周して戻ったら止める。
pub fn tangent_chain(edges: &[EdgeInfo], seed: usize, max_deg: f64) -> Vec<usize> {
    follow(edges, &Ends::new(edges), seed, max_deg)
}

/// `seeds` の辺それぞれから接してつながる辺をたどり、重ねずに並べる。端の表は 1 回だけ作る。
pub fn tangent_chains(edges: &[EdgeInfo], seeds: &[usize], max_deg: f64) -> Vec<usize> {
    let ends = Ends::new(edges);
    let mut out: Vec<usize> = Vec::new();
    let mut seen = HashSet::new();
    for &s in seeds {
        if seen.contains(&s) {
            continue;
        }
        for i in follow(edges, &ends, s, max_deg) {
            if seen.insert(i) {
                out.push(i);
            }
        }
    }
    out
}

/// 進める辺(角の辺で、一周の円でも長さ 0 でもない)の端を、[`POINT_TOL`] の升に入れた表。
/// 端の近い辺だけを見て、辺の多い形でも本数の 2 乗にしない。
struct Ends(HashMap<[i64; 3], Vec<(usize, bool)>>);

impl Ends {
    fn new(edges: &[EdgeInfo]) -> Self {
        let mut map: HashMap<[i64; 3], Vec<(usize, bool)>> = HashMap::new();
        for (i, c) in edges.iter().enumerate() {
            if closed(c) || degenerate(c) || c.seam != Seam::Corner {
                continue;
            }
            for at_b in [false, true] {
                let q = if at_b { c.b } else { c.a };
                map.entry(cell(q)).or_default().push((i, at_b));
            }
        }
        Ends(map)
    }

    /// 点 `p` の升と隣の升にある端(升の境目をまたいで近い端も拾う)。
    fn near(&self, p: [f64; 3]) -> impl Iterator<Item = (usize, bool)> + '_ {
        let [x, y, z] = cell(p);
        (-1..=1).flat_map(move |dx| {
            (-1..=1).flat_map(move |dy| {
                (-1..=1).flat_map(move |dz| {
                    self.0
                        .get(&[x + dx, y + dy, z + dz])
                        .into_iter()
                        .flatten()
                        .copied()
                })
            })
        })
    }
}

fn cell(p: [f64; 3]) -> [i64; 3] {
    p.map(|v| (v / POINT_TOL).floor() as i64)
}

fn follow(edges: &[EdgeInfo], ends: &Ends, seed: usize, max_deg: f64) -> Vec<usize> {
    let mut out = vec![seed];
    if seed >= edges.len() || closed(&edges[seed]) {
        out.truncate(usize::from(seed < edges.len()));
        return out;
    }
    let mut seen = HashSet::from([seed]);
    let cos_tol = max_deg.to_radians().cos();
    for start_at_b in [true, false] {
        let (mut cur, mut at_b) = (seed, start_at_b);
        loop {
            let e = &edges[cur];
            let p = if at_b { e.b } else { e.a };
            // この端で辺から出て行く向き
            let leave = neg(e.tangent_into(at_b));
            let mut best: Option<(usize, bool, f64)> = None;
            for (i, c_at_b) in ends.near(p) {
                if i == cur {
                    continue;
                }
                let c = &edges[i];
                let q = if c_at_b { c.b } else { c.a };
                if dist(p, q) > POINT_TOL {
                    continue;
                }
                let cos = dot(leave, c.tangent_into(c_at_b));
                // 同じ角度なら番号の小さい辺へ(升の並びによらず同じ答えにする)
                let better =
                    best.is_none_or(|b| cos > b.2 || (cos == b.2 && (i, c_at_b) < (b.0, b.1)));
                if cos >= cos_tol && better {
                    best = Some((i, c_at_b, cos));
                }
            }
            let Some((next, entered_at_b, _)) = best else {
                break;
            };
            if !seen.insert(next) {
                break;
            }
            out.push(next);
            cur = next;
            at_b = !entered_at_b;
        }
    }
    out
}

/// 選んだ面 `seeds` から、なめらかな継ぎ目([`Seam::Smooth`])の辺でつながる面をたどった名前
/// (選んだ面が先、たどった面は後ろ)。シェルの「接線チェーン」で開ける面を、辺のたどりと同じ
/// カーネルの答えで決める。分からない辺([`Seam::Unknown`])が 1 本でもあれば `None`(呼ぶ側が
/// 別の見分け方にする)
pub fn smooth_faces(edges: &[EdgeInfo], seeds: &[String]) -> Option<Vec<String>> {
    if edges.iter().any(|e| e.seam.is_unknown()) {
        return None;
    }
    let mut out: Vec<String> = seeds.to_vec();
    let mut todo: Vec<String> = seeds.to_vec();
    while let Some(f) = todo.pop() {
        for e in edges.iter().filter(|e| e.seam == Seam::Smooth) {
            let other = if e.faces[0] == f {
                &e.faces[1]
            } else if e.faces[1] == f {
                &e.faces[0]
            } else {
                continue;
            };
            if !out.contains(other) {
                out.push(other.clone());
                todo.push(other.clone());
            }
        }
    }
    Some(out)
}

/// 一周の円の辺(端が無い)
fn closed(e: &EdgeInfo) -> bool {
    e.arc
        .is_some_and(|a| a.sweep.abs() > std::f64::consts::TAU - 1e-6)
}

fn degenerate(e: &EdgeInfo) -> bool {
    e.arc.is_none() && dist(e.a, e.b) <= POINT_TOL
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    let d = sub(a, b);
    dot(d, d).sqrt()
}

fn neg(v: [f64; 3]) -> [f64; 3] {
    [-v[0], -v[1], -v[2]]
}

#[cfg(test)]
mod tests {
    use super::*;
    use kmb_core::kernel::ArcEdge;

    fn line(a: [f64; 3], b: [f64; 3]) -> EdgeInfo {
        EdgeInfo {
            curve: Vec::new(),
            faces: ["x".into(), "y".into()],
            a,
            b,
            convex: true,
            seam: Seam::Corner,
            arc: None,
        }
    }

    /// 中心 c、半径 r の円の、角度 s から s + sweep までの弧(z 軸まわり、z = 0)
    fn arc(c: [f64; 2], r: f64, s: f64, sweep: f64) -> EdgeInfo {
        let at = |t: f64| [c[0] + r * t.cos(), c[1] + r * t.sin(), 0.0];
        EdgeInfo {
            curve: Vec::new(),
            faces: ["x".into(), "z".into()],
            a: at(s),
            b: at(s + sweep),
            convex: true,
            seam: Seam::Corner,
            arc: Some(ArcEdge {
                center: [c[0], c[1], 0.0],
                axis: [0.0, 0.0, 1.0],
                radius: r,
                sweep,
                inside: true,
            }),
        }
    }

    /// 角を半径 2 で丸めた 20×10 の長方形の縁(直線 4 本と四分円 4 本)と、角から上へ立つ辺 1 本
    fn rounded_rect() -> Vec<EdgeInfo> {
        use std::f64::consts::FRAC_PI_2 as Q;
        vec![
            line([2.0, 0.0, 0.0], [18.0, 0.0, 0.0]),
            arc([18.0, 2.0], 2.0, -Q, Q),
            line([20.0, 2.0, 0.0], [20.0, 8.0, 0.0]),
            arc([18.0, 8.0], 2.0, 0.0, Q),
            // 向きを逆にした辺も、端が重なればたどる
            line([2.0, 10.0, 0.0], [18.0, 10.0, 0.0]),
            arc([2.0, 8.0], 2.0, Q, Q),
            line([0.0, 8.0, 0.0], [0.0, 2.0, 0.0]),
            arc([2.0, 2.0], 2.0, 2.0 * Q, Q),
            line([2.0, 0.0, 0.0], [2.0, 0.0, 5.0]),
        ]
    }

    /// 辺の多い形でも待たない: 細かく分けた輪(どの継ぎ目も 1° 未満で曲がる)の 4000 本と、つながらない
    /// 辺 4000 本から、全部の辺をたどる。前は辺ごとに全部の辺を見ていたので本数の 2 乗で重かった。
    #[test]
    fn a_ring_of_many_edges_is_followed_quickly() {
        let n = 4000;
        let at = |k: usize| {
            let t = std::f64::consts::TAU * (k % n) as f64 / n as f64;
            [100.0 * t.cos(), 100.0 * t.sin(), 0.0]
        };
        let mut edges: Vec<EdgeInfo> = (0..n).map(|k| line(at(k), at(k + 1))).collect();
        edges.extend((0..n).map(|k| line([k as f64, 0.0, 10.0], [k as f64, 0.5, 10.0])));
        let started = std::time::Instant::now();
        let chain = tangent_chains(&edges, &[0, n + 1], TANGENT_DEG);
        let took = started.elapsed();
        assert_eq!(chain.len(), n + 1);
        // 速い PC で数 ms。遅い検査の機械でも十分に収まる上限
        assert!(took.as_millis() < 300, "{took:?}");
    }

    #[test]
    fn a_rounded_rectangle_is_one_chain_from_any_of_its_edges() {
        let edges = rounded_rect();
        for seed in 0..8 {
            let mut got = tangent_chain(&edges, seed, TANGENT_DEG);
            assert_eq!(got[0], seed);
            got.sort();
            assert_eq!(got, (0..8).collect::<Vec<_>>(), "種 {seed}");
        }
        // 角で直角に立つ辺はつながらない
        assert_eq!(tangent_chain(&edges, 8, TANGENT_DEG), vec![8]);
    }

    /// 溝の縁がなめらかな継ぎ目(平面と円筒が接する所)に沿う時、縁の続きの継ぎ目へは進まない。
    /// 継ぎ目でない同じ並びの辺へは進む。カーネルが分からない辺へも進まない。
    #[test]
    fn the_chain_does_not_run_on_into_a_smooth_seam() {
        let with = |seam, a, b| EdgeInfo { seam, ..line(a, b) };
        let edges = vec![
            line([4.0, 10.0, 5.0], [4.0, 10.0, 15.0]),
            with(Seam::Smooth, [4.0, 10.0, 15.0], [4.0, 10.0, 20.0]),
            with(Seam::Smooth, [4.0, 10.0, 0.0], [4.0, 10.0, 5.0]),
        ];
        assert_eq!(tangent_chain(&edges, 0, TANGENT_DEG), vec![0]);
        let mut sharp = edges.clone();
        sharp[1].seam = Seam::Corner;
        assert_eq!(tangent_chain(&sharp, 0, TANGENT_DEG), vec![0, 1]);
        sharp[1].seam = Seam::Unknown;
        assert_eq!(tangent_chain(&sharp, 0, TANGENT_DEG), vec![0]);
    }

    // 確かめること:
    // - 直角の角でつながりが止まる
    // - わずかな折れをつなぐかは、角度の許しで決まる
    // - 分かれ道では、いちばんまっすぐな辺だけをたどる
    #[test]
    fn the_chain_stops_at_a_corner_or_a_bend_and_follows_the_straightest_fork() {
        let edges = vec![
            line([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]),
            line([10.0, 0.0, 0.0], [10.0, 10.0, 0.0]),
            line([10.0, 10.0, 0.0], [20.0, 10.0, 0.0]),
        ];
        assert_eq!(tangent_chain(&edges, 0, TANGENT_DEG), vec![0]);
        assert_eq!(tangent_chain(&edges, 1, TANGENT_DEG), vec![1]);

        // 1° と 3° の折れ
        let bend = |deg: f64| {
            let t = deg.to_radians();
            vec![
                line([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]),
                line(
                    [10.0, 0.0, 0.0],
                    [10.0 + 10.0 * t.cos(), 10.0 * t.sin(), 0.0],
                ),
            ]
        };
        assert_eq!(tangent_chain(&bend(1.0), 0, TANGENT_DEG), vec![0, 1]);
        assert_eq!(tangent_chain(&bend(3.0), 0, TANGENT_DEG), vec![0]);
        assert_eq!(tangent_chain(&bend(3.0), 0, 5.0), vec![0, 1]);

        // 分かれ道
        let t = 1.5f64.to_radians();
        let edges = vec![
            line([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]),
            line(
                [10.0, 0.0, 0.0],
                [10.0 + 10.0 * t.cos(), 10.0 * t.sin(), 0.0],
            ),
            line([10.0, 0.0, 0.0], [20.0, 0.0, 0.0]),
        ];
        assert_eq!(tangent_chain(&edges, 0, TANGENT_DEG), vec![0, 2]);
    }

    #[test]
    fn a_full_circle_is_its_own_chain() {
        let edges = vec![arc([0.0, 0.0], 3.0, 0.0, std::f64::consts::TAU)];
        assert_eq!(tangent_chain(&edges, 0, TANGENT_DEG), vec![0]);
        assert!(tangent_chain(&edges, 5, TANGENT_DEG).is_empty());
    }

    #[test]
    fn chains_from_several_seeds_do_not_repeat_edges() {
        let mut edges = rounded_rect();
        edges.push(line([30.0, 0.0, 0.0], [40.0, 0.0, 0.0]));
        let got = tangent_chains(&edges, &[0, 3, 9], TANGENT_DEG);
        assert_eq!(got.len(), 9);
        assert_eq!(got[8], 9);
    }

    /// シェルの開ける面は、なめらかな継ぎ目の辺でつながる面をたどる。角の辺では止まり、
    /// 継ぎ目の分からない辺があれば答えない
    #[test]
    fn smooth_faces_follow_only_smooth_seams() {
        let e = |a: &str, b: &str, seam| EdgeInfo {
            faces: [a.into(), b.into()],
            seam,
            ..line([0.0; 3], [1.0, 0.0, 0.0])
        };
        let edges = vec![
            e("top", "round1", Seam::Smooth),
            e("round1", "side", Seam::Smooth),
            e("side", "bottom", Seam::Corner),
            e("top", "other", Seam::Corner),
        ];
        assert_eq!(
            smooth_faces(&edges, &["top".into()]),
            Some(vec!["top".into(), "round1".into(), "side".into()])
        );
        let mut unknown = edges.clone();
        unknown.push(e("bottom", "other", Seam::Unknown));
        assert_eq!(smooth_faces(&unknown, &["top".into()]), None);
    }
}
