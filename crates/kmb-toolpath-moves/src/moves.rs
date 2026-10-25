//! 輪郭に沿う動き: 1 周、斜めに下りる、タブの上で持ち上げる、入りと抜けの円弧、始点を回す、
//! 安全高さへ上げる・下げる。

use kmb_cam_geom::{Loop, P2, Seg};
use kmb_toolpath::{ArcDir, Move, P3, Toolpath};

/// 輪郭を高さ `z` で一周する動き。工具は始点(`l.verts[0]`)にいるとする。
pub fn loop_moves(l: &Loop, z: f64, feed: f64) -> Vec<Move> {
    l.segments()
        .into_iter()
        .map(|s| match s {
            Seg::Line { b, .. } => Move::Linear {
                to: [b[0], b[1], z],
                feed,
            },
            Seg::Arc { b, center, ccw, .. } => Move::Arc {
                to: [b[0], b[1], z],
                center,
                dir: if ccw { ArcDir::Ccw } else { ArcDir::Cw },
                feed,
            },
        })
        .collect()
}

/// 輪郭の 1 周の長さ。
pub fn loop_length(l: &Loop) -> f64 {
    l.segments().into_iter().map(|s| seg_length(&s)).sum()
}

pub(crate) fn seg_length(s: &Seg) -> f64 {
    match *s {
        Seg::Line { a, b } => (b[0] - a[0]).hypot(b[1] - a[1]),
        Seg::Arc { a, b, center, ccw } => {
            let r = (a[0] - center[0]).hypot(a[1] - center[1]);
            let dir = if ccw { ArcDir::Ccw } else { ArcDir::Cw };
            let sweep = kmb_toolpath::arc_sweep([a[0], a[1], 0.0], [b[0], b[1], 0.0], center, dir);
            r * sweep.abs()
        }
    }
}

/// 輪郭 `l` に沿って、高さ `from` から `to` まで傾き `angle`(ラジアン)で下がる動き。
/// 下がりきるまで何周でも回り(輪郭が小さければらせんになる)、始点の `to` で終わる。
/// 工具は始点の `from` にいるとする。輪郭がほとんど長さを持たなければ真下に下げる。
pub fn ramp_moves(l: &Loop, from: f64, to: f64, angle: f64, feed: f64, plunge: f64) -> Vec<Move> {
    let segs = l.segments();
    let per = loop_length(l);
    let drop = from - to;
    let start = [l.verts[0].x, l.verts[0].y];
    if drop <= 1e-9 {
        return Vec::new();
    }
    if per < 1e-3 {
        return vec![Move::Linear {
            to: [start[0], start[1], to],
            feed: plunge,
        }];
    }
    let need = drop / angle.tan();
    let laps = (need / per - 1e-9).ceil().max(1.0) as usize;
    let total = laps as f64 * per;
    let mut out = Vec::new();
    let mut done = 0.0;
    for _ in 0..laps {
        for s in &segs {
            done += seg_length(s);
            let z = from - drop * (done / total).min(1.0);
            out.push(match *s {
                Seg::Line { b, .. } => Move::Linear {
                    to: [b[0], b[1], z],
                    feed,
                },
                Seg::Arc { b, center, ccw, .. } => Move::Arc {
                    to: [b[0], b[1], z],
                    center,
                    dir: if ccw { ArcDir::Ccw } else { ArcDir::Cw },
                    feed,
                },
            });
        }
    }
    out
}

/// 輪郭の上に等間隔に置いたタブの中心(`n` 個。始点から半間隔ずらす)。
pub fn tab_centers(l: &Loop, n: u32) -> Vec<[f64; 2]> {
    let pts = l.flatten(0.01);
    if n == 0 || pts.len() < 2 {
        return Vec::new();
    }
    let mut ring = pts.clone();
    ring.push(pts[0]);
    let lens: Vec<f64> = ring
        .windows(2)
        .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
        .collect();
    let per: f64 = lens.iter().sum();
    let mut out = Vec::new();
    for i in 0..n {
        let mut want = per * (i as f64 + 0.5) / n as f64;
        for (k, len) in lens.iter().enumerate() {
            if want <= *len || k + 1 == lens.len() {
                let t = if *len > 0.0 {
                    (want / len).min(1.0)
                } else {
                    0.0
                };
                let (a, b) = (ring[k], ring[k + 1]);
                out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
                break;
            }
            want -= len;
        }
    }
    out
}

/// タブ(中心 `tabs` から XY の距離 `radius` 以内)の上では、高さ `top` より下へ削らないよう、
/// `moves[first..]` の動きを持ち上げる。タブに掛かる動きだけを細かい直線に分ける。
/// `from` は `moves[first]` の前の工具の位置。
pub fn lift_over_tabs(
    moves: &mut Vec<Move>,
    first: usize,
    from: P3,
    tabs: &[[f64; 2]],
    radius: f64,
    top: f64,
) {
    lift_over_tabs_shaped(moves, first, from, tabs, radius, top, None);
}

/// [`lift_over_tabs`] と同じ。`triangle` に高さ(タブの高さ)を渡すと、タブを三角の形にする
/// (中心で `top`、縁で `top - 高さ` へ直線で下がる)。印 `spans` があれば付け直す。
pub fn lift_over_tabs_shaped(
    moves: &mut Vec<Move>,
    first: usize,
    from: P3,
    tabs: &[[f64; 2]],
    radius: f64,
    top: f64,
    triangle: Option<f64>,
) -> Vec<usize> {
    let n = moves.len();
    if tabs.is_empty() {
        return (0..=n).collect();
    }
    // 中心からの距離 d の所で、削らずに残す高さ(タブの外は None)
    let floor_at = |q: P3| -> Option<f64> {
        let d = tabs
            .iter()
            .map(|c| (q[0] - c[0]).hypot(q[1] - c[1]))
            .fold(f64::INFINITY, f64::min);
        if d > radius {
            return None;
        }
        Some(match triangle {
            Some(h) if radius > 1e-9 => top - h * (d / radius),
            _ => top,
        })
    };
    let mut starts: Vec<usize> = (0..first).collect();
    let mut out: Vec<Move> = moves[..first].to_vec();
    let mut cur = from;
    for m in &moves[first..] {
        starts.push(out.len());
        let pts = m.points(cur, 0.2);
        let hits = matches!(m, Move::Linear { .. } | Move::Arc { .. })
            && pts.iter().any(|q| floor_at(*q).is_some_and(|t| q[2] < t));
        if !hits {
            out.push(*m);
        } else {
            let feed = match *m {
                Move::Linear { feed, .. } | Move::Arc { feed, .. } => feed,
                Move::Rapid { .. } => 0.0,
            };
            let mut last = cur;
            for q in pts {
                let z = match floor_at(q) {
                    Some(t) => q[2].max(t),
                    None => q[2],
                };
                // タブの縁では、先に上げてから横へ、横へ出てから下げる
                if z > last[2] + 1e-9 {
                    out.push(Move::Linear {
                        to: [last[0], last[1], z],
                        feed,
                    });
                } else if z < last[2] - 1e-9 {
                    out.push(Move::Linear {
                        to: [q[0], q[1], last[2]],
                        feed,
                    });
                }
                out.push(Move::Linear {
                    to: [q[0], q[1], z],
                    feed,
                });
                last = [q[0], q[1], z];
            }
        }
        cur = m.to();
    }
    starts.push(out.len());
    *moves = out;
    starts
}

/// 輪郭の始まりに接して入る円弧と、終わりに接して抜ける円弧(どちらも 4 分の 1 周)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LeadArcs {
    /// 円弧の半径
    pub radius: f64,
    /// 入りの円弧の始まり(ここで下りる)
    pub start: P2,
    /// 抜けの円弧の終わり
    pub end: P2,
    center_in: P2,
    center_out: P2,
    /// 輪郭の始まり(= 終わり)
    on: P2,
    dir: ArcDir,
}

impl LeadArcs {
    /// 入りの円弧(`start` から輪郭の始まりへ、高さ `z`)。
    pub fn arc_in(&self, z: f64, feed: f64) -> Move {
        Move::Arc {
            to: [self.on[0], self.on[1], z],
            center: self.center_in,
            dir: self.dir,
            feed,
        }
    }

    /// 抜けの円弧(輪郭の終わりから `end` へ、高さ `z`)。
    pub fn arc_out(&self, z: f64, feed: f64) -> Move {
        Move::Arc {
            to: [self.end[0], self.end[1], z],
            center: self.center_out,
            dir: self.dir,
            feed,
        }
    }
}

/// 区間の始まり(`at_end` なら終わり)での進む向き(長さ 1)。
fn tangent(s: &Seg, at_end: bool) -> Option<P2> {
    let t = match *s {
        Seg::Line { a, b } => [b[0] - a[0], b[1] - a[1]],
        Seg::Arc { a, b, center, ccw } => {
            let q = if at_end { b } else { a };
            let u = [q[0] - center[0], q[1] - center[1]];
            if ccw { [-u[1], u[0]] } else { [u[1], -u[0]] }
        }
    };
    let n = t[0].hypot(t[1]);
    (n > 1e-12).then(|| [t[0] / n, t[1] / n])
}

/// 輪郭 `l` の進む向きの左が、工具が入ってよい側か。`free_inside` は輪郭の内側が入ってよい側か。
pub fn free_on_left(l: &Loop, free_inside: bool) -> bool {
    (l.signed_area() > 0.0) == free_inside
}

/// 輪郭 `l`(始点から 1 周)の入りと抜けの円弧。`free_left` は進む向きの左が入ってよい側か。
/// 円弧の上の点がすべて `fits` を満たす半径を、`radius` から半分ずつ 3 回まで小さくして探す。
/// どれも入らなければ `None`。
pub fn lead_arcs(
    l: &Loop,
    radius: f64,
    free_left: bool,
    fits: impl Fn(P2) -> bool,
) -> Option<LeadArcs> {
    let segs = l.segments();
    let t0 = tangent(segs.first()?, false)?;
    let t1 = tangent(segs.last()?, true)?;
    let on = [l.verts[0].x, l.verts[0].y];
    // 入ってよい側を向く法線
    let side = |t: P2| {
        if free_left {
            [-t[1], t[0]]
        } else {
            [t[1], -t[0]]
        }
    };
    let (n0, n1) = (side(t0), side(t1));
    let dir = if free_left { ArcDir::Ccw } else { ArcDir::Cw };
    let mut r = radius;
    for _ in 0..4 {
        let c_in = [on[0] + n0[0] * r, on[1] + n0[1] * r];
        let c_out = [on[0] + n1[0] * r, on[1] + n1[1] * r];
        // 円の上の点: 角 θ で、θ = 0 が輪郭の上、θ = -90° が入りの始まり、+90° が抜けの終わり
        let at = |c: P2, n: P2, t: P2, th: f64| {
            [
                c[0] + r * (-n[0] * th.cos() + t[0] * th.sin()),
                c[1] + r * (-n[1] * th.cos() + t[1] * th.sin()),
            ]
        };
        let q = std::f64::consts::FRAC_PI_2;
        let ok = (1..=8).all(|k| {
            let th = q * k as f64 / 8.0;
            fits(at(c_in, n0, t0, -th)) && fits(at(c_out, n1, t1, th))
        });
        if ok {
            return Some(LeadArcs {
                radius: r,
                start: at(c_in, n0, t0, -q),
                end: at(c_out, n1, t1, q),
                center_in: c_in,
                center_out: c_out,
                on,
                dir,
            });
        }
        r /= 2.0;
    }
    None
}

/// 輪郭の始点を、いちばん長い直線の区間の中点に移す(円弧で入って抜ける時に角を避ける)。
/// 直線の区間がなければそのまま。
pub fn start_mid_line(l: &Loop) -> Loop {
    let n = l.verts.len();
    let len = |i: usize| {
        let (a, b) = (l.verts[i], l.verts[(i + 1) % n]);
        (b.x - a.x).hypot(b.y - a.y)
    };
    let Some(k) = (0..n)
        .filter(|&i| l.verts[i].bulge.abs() < 1e-12)
        // 同じ長さなら先の区間
        .max_by(|&i, &j| len(i).total_cmp(&len(j)).then(j.cmp(&i)))
    else {
        return l.clone();
    };
    let (a, b) = (l.verts[k], l.verts[(k + 1) % n]);
    let mid = kmb_cam_geom::Vertex {
        x: (a.x + b.x) / 2.0,
        y: (a.y + b.y) / 2.0,
        bulge: 0.0,
    };
    Loop {
        verts: std::iter::once(mid)
            .chain((1..=n).map(|i| l.verts[(k + i) % n]))
            .collect(),
    }
}

/// 輪郭の始点を、`p` に最も近い頂点に回す。
pub fn start_near(l: &Loop, p: [f64; 2]) -> Loop {
    let n = l.verts.len();
    let k = (0..n)
        .min_by(|&i, &j| {
            let d = |v: &kmb_cam_geom::Vertex| (v.x - p[0]).hypot(v.y - p[1]);
            d(&l.verts[i]).total_cmp(&d(&l.verts[j]))
        })
        .unwrap_or(0);
    Loop {
        verts: (0..n).map(|i| l.verts[(k + i) % n]).collect(),
    }
}

/// 経路の終わりに安全高さへ上げる。
pub fn retract(tp: &mut Toolpath) {
    if let Some(last) = tp.moves.last().map(|m| m.to())
        && last[2] < tp.safe_z
    {
        tp.moves.push(Move::Rapid {
            to: [last[0], last[1], tp.safe_z],
        });
    }
}

/// `xy` の上へ安全高さで移り、退避高さまで早送りで下げる。
pub fn approach(tp: &mut Toolpath, xy: [f64; 2]) {
    let cur = tp.moves.last().map(|m| m.to());
    if let Some(c) = cur
        && c[2] < tp.safe_z
    {
        tp.moves.push(Move::Rapid {
            to: [c[0], c[1], tp.safe_z],
        });
    }
    tp.moves.push(Move::Rapid {
        to: [xy[0], xy[1], tp.safe_z],
    });
    tp.moves.push(Move::Rapid {
        to: [xy[0], xy[1], tp.retract_z],
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn end(m: &[Move]) -> P3 {
        m.last().unwrap().to()
    }

    #[test]
    fn tabs_lift_the_cut_only_over_themselves() {
        let sq = Loop::from_points(&[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]);
        let c = tab_centers(&sq, 2);
        assert_eq!(c.len(), 2);
        // 周の 1/4 と 3/4(10 mm と 30 mm の所)
        assert!(
            (c[0][0] - 10.0).abs() < 1e-6 && (c[0][1] - 0.0).abs() < 1e-6,
            "{c:?}"
        );
        assert!(
            (c[1][0] - 0.0).abs() < 1e-6 && (c[1][1] - 10.0).abs() < 1e-6,
            "{c:?}"
        );
        let mut m = vec![Move::Linear {
            to: [10.0, 0.0, -3.0],
            feed: 500.0,
        }];
        // 下の辺の上、x = 2 から 10 へ高さ -3 で削る。タブは (10, 0) の半径 2 mm
        lift_over_tabs(&mut m, 0, [2.0, 0.0, -3.0], &c, 2.0, -1.0);
        let max_z = m.iter().map(|x| x.to()[2]).fold(f64::MIN, f64::max);
        assert!((max_z + 1.0).abs() < 1e-9);
        for x in &m {
            let p = x.to();
            if p[0] < 7.7 {
                assert_eq!(p[2], -3.0, "{p:?}");
            }
            if p[0] > 8.1 {
                assert_eq!(p[2], -1.0, "{p:?}");
            }
        }
        // タブより上の動きは変えない
        let mut m = vec![Move::Linear {
            to: [10.0, 0.0, 0.0],
            feed: 500.0,
        }];
        lift_over_tabs(&mut m, 0, [2.0, 0.0, 0.0], &c, 2.0, -1.0);
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn lead_arcs_touch_the_loop_on_the_free_side_and_avoid_corners() {
        // 反時計回りの 20 × 10 の四角の内側を回る(左が内側)
        let sq = Loop::from_points(&[[0.0, 0.0], [20.0, 0.0], [20.0, 10.0], [0.0, 10.0]]);
        let l = start_mid_line(&sq);
        assert_eq!([l.verts[0].x, l.verts[0].y], [10.0, 0.0]);
        assert_eq!(loop_length(&l), loop_length(&sq));
        let free_left = free_on_left(&l, true);
        assert!(free_left);
        let a = lead_arcs(&l, 2.0, free_left, |q| sq.contains(q)).unwrap();
        assert_eq!(a.radius, 2.0);
        // 入りは (8, 2) から中心 (10, 2) を反時計回りに (10, 0) へ。抜けは (12, 2) へ
        let close = |p: P2, q: P2| (p[0] - q[0]).hypot(p[1] - q[1]) < 1e-9;
        assert!(close(a.start, [8.0, 2.0]) && close(a.end, [12.0, 2.0]));
        assert!(matches!(
            a.arc_in(-1.0, 100.0),
            Move::Arc {
                to: [10.0, 0.0, -1.0],
                center: [10.0, 2.0],
                dir: ArcDir::Ccw,
                ..
            }
        ));
        // 角から始めると、入りの円弧は四角の外へ出るので入らない
        assert!(lead_arcs(&sq, 2.0, true, |q| sq.contains(q)).is_none());
        // 高さ 10 の四角に半径 12 は入らないので、半分の 6 にする
        assert_eq!(
            lead_arcs(&l, 12.0, true, |q| sq.contains(q))
                .unwrap()
                .radius,
            6.0
        );
        // 外側を時計回りに回る時は、左が外
        let cw = sq.reversed();
        assert!(free_on_left(&cw, false));
    }

    #[test]
    fn the_ramp_goes_down_the_loop_no_steeper_than_its_angle() {
        let sq = Loop::from_points(&[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]);
        assert!((loop_length(&sq) - 40.0).abs() < 1e-9);
        let a = 3f64.to_radians();
        let m = ramp_moves(&sq, 0.0, -1.0, a, 500.0, 100.0);
        // 1 周(40 mm)で 1 mm 下がる。必要な長さは 19.1 mm なので 1 周で足りる
        assert_eq!(m.len(), 4);
        assert_eq!(end(&m), [0.0, 0.0, -1.0]);
        let mut cur = [0.0, 0.0, 0.0];
        for mv in &m {
            let to = mv.to();
            let run = (to[0] - cur[0]).hypot(to[1] - cur[1]);
            assert!(cur[2] - to[2] <= run * a.tan() + 1e-9);
            cur = to;
        }
        // 小さな輪では何周も回る(らせん)
        let small = Loop::from_points(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        let m = ramp_moves(&small, 0.0, -1.0, a, 500.0, 100.0);
        assert_eq!(m.len() % 4, 0);
        assert!(m.len() / 4 >= 5, "{}", m.len());
        assert_eq!(end(&m), [0.0, 0.0, -1.0]);
        // 長さのない輪では真下に下げる
        let dot = Loop::from_points(&[[2.0, 2.0], [2.0, 2.0]]);
        let m = ramp_moves(&dot, 0.0, -1.0, a, 500.0, 100.0);
        assert!(matches!(m[..], [Move::Linear { feed, .. }] if feed == 100.0));
    }
}
