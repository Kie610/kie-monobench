//! 穴 1 つ分の動き(固定サイクルは使わず G0・G1・G2・G3 に展開する)と、穴と穴の間の移り方。
//!
//! 高さは 2 つ: 削り始める高さ `top`(穴の上か、ストックの上面)と、その上の送り高さ `r`
//! (`top` + セットアップの退避高さの分。退避高さを超えない)。早送りで下げてよいのは退避高さ
//! までなので、退避高さから `r` までは切削の送りで下げる。つつきで戻るのは `r` まで。

use jc_toolpath::{ArcDir, Dwell, Move, Toolpath};
use serde::{Deserialize, Serialize};

/// まっすぐ下げる穴のサイクル。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cycle {
    /// 工具で決める(ドリルと同じ径の平はつつき、細い平はらせん、V はもみつけ)
    #[default]
    Auto,
    /// 1 回で底まで下げる
    Straight,
    /// つつき: ペックごとに送り高さまで戻して切りくずを出す
    Peck,
    /// 切りくず切断: ペックごとに少しだけ戻して切りくずを切る
    ChipBreak,
}

/// 切りくず切断で戻す量(mm)。
pub const CHIP_LIFT: f64 = 0.5;
/// つつきで前に削った深さの上まで速めに戻る時の余裕(mm)。
const ABOVE_CUT: f64 = 0.2;

/// 穴 1 つの高さ。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Heights {
    /// 削り始める高さ
    pub top: f64,
    /// 送り高さ(つつきで戻る高さ)
    pub r: f64,
}

/// まっすぐ下げる時の決め事。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Plunge {
    /// `Auto` は使わない(呼ぶ側がつつきにする)
    pub cycle: Cycle,
    /// 1 回に下げる深さ
    pub peck: f64,
    /// 底で止まる秒数
    pub dwell: f64,
    pub feed: f64,
    pub plunge: f64,
}

fn last(tp: &Toolpath) -> [f64; 3] {
    tp.moves.last().map_or([0.0, 0.0, tp.safe_z], |m| m.to())
}

/// 送りで直線に動く(長さがなければ足さない)。
fn line(tp: &mut Toolpath, to: [f64; 3], feed: f64) {
    if jc_toolpath::dist(last(tp), to) > 1e-9 {
        tp.moves.push(Move::Linear { to, feed });
    }
}

fn rapid_up(tp: &mut Toolpath, to: [f64; 3]) {
    if to[2] > last(tp)[2] + 1e-9 {
        tp.moves.push(Move::Rapid { to });
    }
}

fn dwell(tp: &mut Toolpath, at: [f64; 3], seconds: f64) {
    if seconds > 0.0 {
        tp.dwells.push(Dwell { at, seconds });
    }
}

/// 穴の上(`xy`)にいる工具を、送り高さ `r` まで下降の送りで下げる。穴の上より上は前の加工で
/// 削れている前提だが、削れていなくても突っ込まないよう、切削の送りではなく下降の送りにする。
pub fn down_to(tp: &mut Toolpath, xy: [f64; 2], r: f64, feed: f64) {
    let cur = last(tp);
    if cur[2] > r + 1e-9 {
        line(tp, [xy[0], xy[1], r], feed);
    }
}

/// 中心 `c` で、`h.top` から `bottom` まで下げる。終わりは `[c, h.r]`。
pub fn plunge(tp: &mut Toolpath, c: [f64; 2], h: Heights, bottom: f64, p: &Plunge) {
    down_to(tp, c, h.r, p.plunge);
    let at = |z: f64| [c[0], c[1], z];
    match p.cycle {
        Cycle::Auto | Cycle::Peck => {
            let mut z = h.top;
            while z > bottom + 1e-9 {
                let next = (z - p.peck).max(bottom);
                // 前に削った深さの少し上までは速めに戻る
                line(tp, at((z + ABOVE_CUT).min(h.r)), p.feed);
                line(tp, at(next), p.plunge);
                if next > bottom + 1e-9 {
                    // 切りくずを出す
                    rapid_up(tp, at(h.r));
                }
                z = next;
            }
        }
        Cycle::ChipBreak => {
            line(tp, at((h.top + ABOVE_CUT).min(h.r)), p.feed);
            let mut z = h.top;
            while z > bottom + 1e-9 {
                let next = (z - p.peck).max(bottom);
                line(tp, at(next), p.plunge);
                if next > bottom + 1e-9 {
                    // 少しだけ戻して切りくずを切る
                    rapid_up(tp, at((next + CHIP_LIFT).min(h.r)));
                }
                z = next;
            }
        }
        Cycle::Straight => {
            line(tp, at((h.top + ABOVE_CUT).min(h.r)), p.feed);
            line(tp, at(bottom), p.plunge);
        }
    }
    dwell(tp, at(bottom), p.dwell);
    rapid_up(tp, at(h.r));
}

/// らせんの周の半径(内側から外側へ。中心に芯が残らないように分ける)。
pub fn helical_radii(r_path: f64, tool_r: f64, step: f64) -> Vec<f64> {
    let n = ((r_path - tool_r * 0.9) / step).ceil().max(0.0) as usize + 1;
    (1..=n).map(|k| r_path * k as f64 / n as f64).collect()
}

/// 中心 `c` のまわりを、半径 `radii` の周ごとにらせんで `bottom` まで下げる。1 周で `pitch` 下がり、
/// 最後は底で 1 周する。始めは最初の周の始点の上にいる。終わりは `[c, h.r]`。
/// 送りは `p` の送りと下降の送りを使う(サイクル・ペック・停留は使わない)。
pub fn helical(
    tp: &mut Toolpath,
    c: [f64; 2],
    radii: &[f64],
    h: Heights,
    bottom: f64,
    pitch: f64,
    p: &Plunge,
) {
    let (feed, plunge_feed) = (p.feed, p.plunge);
    for (k, r) in radii.iter().enumerate() {
        let start = [c[0] + r, c[1]];
        if k == 0 {
            down_to(tp, start, h.r, plunge_feed);
        } else {
            // 内側の周で削った穴の上を通って外へ移る
            line(tp, [start[0], start[1], h.r], feed);
        }
        line(tp, [start[0], start[1], h.top], plunge_feed);
        let turns = ((h.top - bottom) / pitch).ceil().max(1.0) as usize;
        for i in 1..=turns {
            let z = h.top - (h.top - bottom) * i as f64 / turns as f64;
            tp.moves.push(Move::Arc {
                to: [start[0], start[1], z],
                center: c,
                dir: ArcDir::Ccw,
                feed,
            });
        }
        tp.moves.push(Move::Arc {
            to: [start[0], start[1], bottom],
            center: c,
            dir: ArcDir::Ccw,
            feed,
        });
        // 壁から離れて真上へ抜ける
        line(tp, [c[0], c[1], bottom], feed);
        rapid_up(tp, [c[0], c[1], h.r]);
    }
}

/// V ビットで、中心 `c` の先端を `tip` まで下げ、半径 `ring` の円を 1 周する(0 なら下げるだけ)。
/// もみつけと皿の面取りに使う。終わりは `[c, h.r]`。
pub fn spot(tp: &mut Toolpath, c: [f64; 2], h: Heights, tip: f64, ring: f64, p: &Plunge) {
    down_to(tp, c, h.r, p.plunge);
    let at = |z: f64| [c[0], c[1], z];
    line(tp, at(tip), p.plunge);
    if ring > 1e-3 {
        let s = [c[0] + ring, c[1], tip];
        line(tp, s, p.feed);
        tp.moves.push(Move::Arc {
            to: s,
            center: c,
            dir: ArcDir::Ccw,
            feed: p.feed,
        });
        line(tp, at(tip), p.feed);
    }
    dwell(tp, at(tip), p.dwell);
    rapid_up(tp, at(h.r));
}

#[cfg(test)]
mod tests {
    use super::*;
    use jc_cam_tool::ToolLibrary;

    fn tp() -> Toolpath {
        Toolpath {
            name: "穴".into(),
            tool: ToolLibrary::starter().tools[2].clone(),
            rpm: 10000.0,
            safe_z: 5.0,
            retract_z: 1.0,
            edge_break: 0.0,
            moves: vec![
                Move::Rapid {
                    to: [0.0, 0.0, 5.0],
                },
                Move::Rapid {
                    to: [0.0, 0.0, 1.0],
                },
            ],
            dwells: vec![],
            spans: vec![],
            links: Default::default(),
        }
    }

    fn cycle(c: Cycle) -> Plunge {
        Plunge {
            cycle: c,
            peck: 1.0,
            dwell: 0.0,
            feed: 300.0,
            plunge: 100.0,
        }
    }

    fn tops(t: &Toolpath) -> Vec<f64> {
        // 上へ戻った高さ
        t.moves
            .windows(2)
            .filter(|w| w[1].to()[2] > w[0].to()[2] + 1e-9)
            .map(|w| w[1].to()[2])
            .collect()
    }

    #[test]
    fn peck_returns_to_the_feed_height_and_chip_break_only_lifts_a_little() {
        let h = Heights { top: 0.0, r: 1.0 };
        let mut a = tp();
        plunge(&mut a, [0.0, 0.0], h, -3.0, &cycle(Cycle::Peck));
        assert_eq!(tops(&a), vec![1.0, 1.0, 1.0]);
        let mut b = tp();
        plunge(&mut b, [0.0, 0.0], h, -3.0, &cycle(Cycle::ChipBreak));
        assert_eq!(tops(&b), vec![-0.5, -1.5, 1.0]);
        let mut s = tp();
        plunge(&mut s, [0.0, 0.0], h, -3.0, &cycle(Cycle::Straight));
        assert_eq!(tops(&s), vec![1.0]);
        for t in [&a, &b, &s] {
            assert!(
                jc_toolpath::check(t)
                    .iter()
                    .all(|p| p.message.contains("最後"))
            );
            let low = t.moves.iter().map(|m| m.to()[2]).fold(f64::MAX, f64::min);
            assert_eq!(low, -3.0);
        }
        assert!(
            b.lengths().0 < a.lengths().0,
            "切りくず切断の方が動きが短い"
        );
    }

    #[test]
    fn a_hole_in_a_pocket_starts_at_its_top_and_dwells_at_the_bottom() {
        // 穴の上が -10(ポケットの底)。送り高さは -9
        let h = Heights {
            top: -10.0,
            r: -9.0,
        };
        let mut t = tp();
        let p = Plunge {
            dwell: 0.5,
            ..cycle(Cycle::Peck)
        };
        plunge(&mut t, [0.0, 0.0], h, -12.0, &p);
        // 退避高さ 1 から送り高さ -9 まで下降の送りで下り、つつきは -9 まで戻る
        assert!(
            matches!(t.moves[2], Move::Linear { to, feed } if to == [0.0, 0.0, -9.0] && feed == 100.0)
        );
        assert!(tops(&t).iter().all(|z| *z == -9.0), "{:?}", tops(&t));
        assert_eq!(
            t.dwells,
            vec![Dwell {
                at: [0.0, 0.0, -12.0],
                seconds: 0.5
            }]
        );
        let program = jc_toolpath::Program::build("p", 1, 20.0, &[t]);
        let waits = program
            .ops
            .iter()
            .filter(|o| matches!(o, jc_toolpath::Op::Dwell { seconds } if *seconds == 0.5))
            .count();
        assert_eq!(waits, 1);
    }

    #[test]
    fn a_v_bit_rings_to_widen_a_countersink() {
        let h = Heights { top: 0.0, r: 1.0 };
        let mut t = tp();
        spot(&mut t, [0.0, 0.0], h, -1.5, 1.6, &cycle(Cycle::Auto));
        assert!(t.moves.iter().any(|m| matches!(m, Move::Arc { .. })));
        assert!(
            jc_toolpath::check(&t)
                .iter()
                .all(|p| p.message.contains("最後"))
        );
    }
}
