//! 固定具(クランプ)と、経路が固定具を避けること。
//!
//! 固定具はセットアップに置き、ベッドのネジ穴に吸い付けて置く(穴の位置は機械のベッドの値から画面と
//! 操作が決める)。固定具の外形に [`FIXTURE_MARGIN`] を足した箱を「避ける範囲」にし、経路のうち
//! その箱に入る所は、箱の上まで上げて越えてから元の深さへ戻す(固定具の下は削らずに残る)。
//! 干渉を調べる考え方は広く知られた一般の方法。

use jc_toolpath::{Move, P3, Toolpath};
use serde::{Deserialize, Serialize};

/// 固定具の外形に足す、避ける範囲の幅(mm)。
pub const FIXTURE_MARGIN: f64 = 2.0;

fn clamp_size() -> [f64; 3] {
    [30.0, 15.0, 20.0]
}

/// 固定具。位置はセットアップの座標の XY(固定具の中心)、大きさは幅・奥行き・高さ(ストックの底から)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fixture {
    pub at: [f64; 2],
    #[serde(default = "clamp_size")]
    pub size: [f64; 3],
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
}

impl Fixture {
    /// 固定具そのものの箱(`bottom` はストックの底の高さ)。
    pub fn body(&self, bottom: f64) -> (P3, P3) {
        let [w, d, h] = self.size;
        (
            [self.at[0] - w / 2.0, self.at[1] - d / 2.0, bottom],
            [self.at[0] + w / 2.0, self.at[1] + d / 2.0, bottom + h],
        )
    }

    /// 避ける範囲の箱。
    pub fn keep_out(&self, bottom: f64) -> (P3, P3) {
        let (lo, hi) = self.body(bottom);
        let m = FIXTURE_MARGIN;
        (
            [lo[0] - m, lo[1] - m, lo[2]],
            [hi[0] + m, hi[1] + m, hi[2] + m],
        )
    }
}

/// 置いた固定具とストックの横の間(mm)。経路はストックの外へ工具の半径ほど出るので、
/// 避ける範囲([`FIXTURE_MARGIN`])を足してもその外になるよう、6 mm の工具までの分を空ける。
pub const FIXTURE_GAP: f64 = 8.0;

/// クリックした所 `at` に置く固定具の位置と大きさ。長い方をストックのいちばん近い辺に沿わせ、
/// ストック(XY が `lo`〜`hi`)から [`FIXTURE_GAP`] 以内に掛かるなら、その辺の外へずらす。
pub fn place_clamp(at: [f64; 2], lo: P3, hi: P3) -> ([f64; 2], [f64; 3]) {
    let [long, short, h] = clamp_size();
    // 各辺から外へ向かう距離(内側なら負)。いちばん大きい辺がいちばん近い外の辺
    let out = [lo[0] - at[0], at[0] - hi[0], lo[1] - at[1], at[1] - hi[1]];
    // 角のように同じ近さなら、左・右・手前・奥の順で先の辺
    let side = (1..4).fold(0, |s, i| if out[i] > out[s] { i } else { s });
    let size = if side < 2 {
        [short, long, h]
    } else {
        [long, short, h]
    };
    let mut at = at;
    let g = FIXTURE_GAP;
    let overlaps = at[0] + size[0] / 2.0 > lo[0] - g
        && at[0] - size[0] / 2.0 < hi[0] + g
        && at[1] + size[1] / 2.0 > lo[1] - g
        && at[1] - size[1] / 2.0 < hi[1] + g;
    if overlaps {
        match side {
            0 => at[0] = lo[0] - g - size[0] / 2.0,
            1 => at[0] = hi[0] + g + size[0] / 2.0,
            2 => at[1] = lo[1] - g - size[1] / 2.0,
            _ => at[1] = hi[1] + g + size[1] / 2.0,
        }
    }
    (at, size)
}

/// 経路(早送りも)が XY で避ける範囲の中を通るか(範囲の上を越える所も含む)。
pub fn passes_over(tp: &Toolpath, zones: &[(P3, P3)]) -> bool {
    let mut prev: Option<P3> = None;
    for m in &tp.moves {
        let to = m.to();
        if let Some(a) = prev
            && zones.iter().any(|z| clip(a, to, *z).is_some())
        {
            return true;
        }
        prev = Some(to);
    }
    false
}

/// 点が箱の中か(XY は内側、Z は上面より下)。縁の上(計算の誤差の幅)は外とみなす。
fn inside(p: P3, (lo, hi): (P3, P3)) -> bool {
    const E: f64 = 1e-6;
    p[0] > lo[0] + E && p[0] < hi[0] - E && p[1] > lo[1] + E && p[1] < hi[1] - E && p[2] < hi[2] - E
}

/// 線分 a→b が XY で箱に入る区間(0〜1 の割合)。Liang–Barsky。
fn clip(a: P3, b: P3, (lo, hi): (P3, P3)) -> Option<(f64, f64)> {
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    let d = [b[0] - a[0], b[1] - a[1]];
    for k in 0..2 {
        for (p, q) in [(-d[k], a[k] - lo[k]), (d[k], hi[k] - a[k])] {
            if p.abs() < 1e-12 {
                if q < 0.0 {
                    return None;
                }
            } else {
                let t = q / p;
                if p < 0.0 {
                    t0 = t0.max(t);
                } else {
                    t1 = t1.min(t);
                }
            }
        }
    }
    (t1 - t0 > 1e-9).then_some((t0, t1))
}

fn lerp(a: P3, b: P3, t: f64) -> P3 {
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t)
}

/// 経路が避ける範囲に入る所を、範囲の上を越える動きに替える。早送りも同じように越える。
/// 替えた切削の所の数を返す(早送りは数えない)。
/// 円弧は短い直線に分けてから調べる(範囲に掛からない円弧はそのまま残す)。
pub fn avoid(tp: &mut Toolpath, zones: &[(P3, P3)]) -> usize {
    if zones.is_empty() || tp.moves.is_empty() {
        return 0;
    }
    // 範囲の中で終わる動きは、範囲の上で止める(次の動きはそこから)
    let lift = |p: P3| {
        zones
            .iter()
            .filter(|z| inside(p, **z))
            .fold(p, |p, z| [p[0], p[1], p[2].max(z.1[2] + 1.0)])
    };
    let mut out: Vec<Move> = Vec::with_capacity(tp.moves.len());
    let mut cur: Option<P3> = None;
    let mut detours = 0;
    // 元の動きごとの、新しい列での始めの番号(役目の印とつなぎの記録を付け直す)
    let mut starts: Vec<usize> = Vec::with_capacity(tp.moves.len() + 1);
    for m in std::mem::take(&mut tp.moves) {
        starts.push(out.len());
        let Some(from) = cur else {
            cur = Some(m.to());
            let to = lift(m.to());
            out.push(match m {
                Move::Rapid { .. } => Move::Rapid { to },
                _ if to != m.to() => Move::Rapid { to },
                _ => m,
            });
            continue;
        };
        let to = m.to();
        cur = Some(to);
        let hits = |a: P3, b: P3| {
            zones
                .iter()
                .any(|z| clip(a, b, *z).is_some() && (a[2] < z.1[2] || b[2] < z.1[2]))
        };
        // 送り(早送りは None)ごとの線分
        let pieces: Vec<(P3, P3, Option<f64>)> = match m {
            Move::Rapid { .. } => vec![(from, to, None)],
            Move::Linear { feed, .. } => vec![(from, to, Some(feed))],
            Move::Arc { feed, .. } => {
                let pts = m.points(from, 0.5);
                if !pts
                    .iter()
                    .scan(from, |p, q| {
                        let r = (*p, *q);
                        *p = *q;
                        Some(r)
                    })
                    .any(|(a, b)| hits(a, b))
                {
                    out.push(m);
                    continue;
                }
                let mut prev = from;
                pts.into_iter()
                    .map(|q| {
                        let r = (prev, q, Some(feed));
                        prev = q;
                        r
                    })
                    .collect()
            }
        };
        let mv = |to: P3, feed: Option<f64>| match feed {
            Some(feed) => Move::Linear { to, feed },
            None => Move::Rapid { to },
        };
        for (k, (a, b, feed)) in pieces.into_iter().enumerate() {
            // 最初の線分は、実際にいる所(前の動きが上で止まったならそこ)から
            let mut a = match (k, out.last()) {
                (0, Some(l)) => l.to(),
                _ => a,
            };
            let b = lift(b);
            loop {
                // 残りの線分が入る範囲のうち、最初に入るもの
                let hit = zones
                    .iter()
                    .filter_map(|z| clip(a, b, *z).map(|t| (t, *z)))
                    .filter(|((t0, t1), z)| {
                        let (p, q) = (lerp(a, b, *t0), lerp(a, b, *t1));
                        p[2] < z.1[2] || q[2] < z.1[2] || inside(lerp(a, b, (t0 + t1) / 2.0), *z)
                    })
                    .min_by(|x, y| x.0.0.total_cmp(&y.0.0));
                let Some(((t0, t1), z)) = hit else {
                    out.push(mv(b, feed));
                    break;
                };
                let (p, q) = (lerp(a, b, t0), lerp(a, b, t1));
                let top = z.1[2] + 1.0;
                // 入る手前まで動き、上げて越え、出た所で元の高さへ戻る
                out.push(mv(p, feed));
                out.push(mv([p[0], p[1], top.max(p[2])], feed));
                out.push(mv([q[0], q[1], top.max(q[2])], feed));
                out.push(mv(q, feed));
                if feed.is_some() {
                    detours += 1;
                }
                if t1 >= 1.0 - 1e-9 {
                    break;
                }
                a = q;
            }
        }
    }
    starts.push(out.len());
    tp.links.remap(|i| starts[i.min(starts.len() - 1)]);
    tp.moves = out;
    tp.remap_spans(&starts);
    detours
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(moves: Vec<Move>) -> Toolpath {
        Toolpath {
            name: "t".into(),
            tool: jc_cam_tool::ToolLibrary::starter().tools[0].clone(),
            rpm: 10000.0,
            safe_z: 5.0,
            retract_z: 2.0,
            moves,
            edge_break: 0.0,
            dwells: vec![],
            spans: vec![],
            links: Default::default(),
        }
    }

    #[test]
    fn a_clamp_clicked_on_the_stock_corner_moves_outside() {
        // ストック 0〜64 × 0〜44。左手前の角の近くのネジ穴 (-4.5, -4.5) に置いた
        let (lo, hi) = ([0.0, 0.0, -8.0], [64.0, 44.0, 0.0]);
        let (at, size) = place_clamp([-4.5, -4.5], lo, hi);
        assert_eq!(size, [15.0, 30.0, 20.0]);
        let f = Fixture {
            at,
            size,
            name: String::new(),
        };
        let (blo, bhi) = f.body(-8.0);
        // 左の辺の外、間は FIXTURE_GAP
        assert!(
            (bhi[0] - (lo[0] - FIXTURE_GAP)).abs() < 1e-9,
            "{blo:?} {bhi:?}"
        );
        // 手前の辺の外に置けば、長い方は X に沿う
        let (at, size) = place_clamp([30.0, -3.0], lo, hi);
        assert_eq!(size, [30.0, 15.0, 20.0]);
        assert!((at[1] + size[1] / 2.0 - (lo[1] - FIXTURE_GAP)).abs() < 1e-9);
        // 十分に離れた所はそのまま
        let (at, _) = place_clamp([30.0, -40.0], lo, hi);
        assert_eq!(at, [30.0, -40.0]);
    }

    #[test]
    fn a_cut_through_a_clamp_goes_over_it_and_back_down() {
        let f = Fixture {
            at: [50.0, 0.0],
            size: [10.0, 10.0, 8.0],
            name: String::new(),
        };
        let zone = f.keep_out(-10.0);
        assert_eq!(zone.0, [43.0, -7.0, -10.0]);
        assert_eq!(zone.1, [57.0, 7.0, 0.0]);
        let mut tp = path(vec![
            Move::Rapid {
                to: [0.0, 0.0, 5.0],
            },
            Move::Linear {
                to: [0.0, 0.0, -1.0],
                feed: 100.0,
            },
            Move::Linear {
                to: [100.0, 0.0, -1.0],
                feed: 500.0,
            },
            // 範囲の外の円弧はそのまま
            Move::Arc {
                to: [100.0, 20.0, -1.0],
                center: [100.0, 10.0],
                dir: jc_toolpath::ArcDir::Ccw,
                feed: 500.0,
            },
        ]);
        assert_eq!(avoid(&mut tp, &[zone]), 1);
        let pts: Vec<P3> = tp.moves.iter().map(|m| m.to()).collect();
        let has = |q: P3| {
            pts.iter()
                .any(|p| (0..3).all(|i| (p[i] - q[i]).abs() < 1e-9))
        };
        assert!(has([43.0, 0.0, -1.0]));
        assert!(has([43.0, 0.0, 1.0]), "{pts:?}");
        assert!(has([57.0, 0.0, 1.0]));
        assert!(has([57.0, 0.0, -1.0]));
        assert_eq!(*pts.last().unwrap(), [100.0, 20.0, -1.0]);
        assert!(matches!(tp.moves.last(), Some(Move::Arc { .. })));
        // 範囲の中の点を、上面より下で通る切削がない
        let mut prev = pts[0];
        for p in &pts[1..] {
            for k in 0..=20 {
                let q = lerp(prev, *p, k as f64 / 20.0);
                assert!(!inside(q, zone), "{q:?}");
            }
            prev = *p;
        }
        // 上を通る経路は変えない
        let mut high = path(vec![
            Move::Rapid {
                to: [0.0, 0.0, 5.0],
            },
            Move::Linear {
                to: [100.0, 0.0, 3.0],
                feed: 500.0,
            },
        ]);
        assert_eq!(avoid(&mut high, &[zone]), 0);
    }
}
