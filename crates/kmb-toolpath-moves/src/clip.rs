//! 区画の窓による経路の切り分け。円弧は円と窓の辺の交点で分ける。

use kmb_toolpath::{Move, P3, Toolpath, Xform2};
use serde::{Deserialize, Serialize};

/// セットアップを知らずに窓を読むための口。座標の所有者がこれを実装する。
pub trait SectionWindow {
    fn cut_min(&self) -> [f64; 2];
    fn cut_max(&self) -> [f64; 2];
    fn owns(&self, point: [f64; 2]) -> bool;
    fn shift(&self) -> [f64; 2];
    /// 始点の外側が、後の区画で削る側か。
    fn uncut_entry(&self, point: [f64; 2]) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ClipOpts {
    pub clearance_z: f64,
    /// 加工の入りの傾斜(ラジアン)。
    pub ramp_angle: f64,
    pub plunge: f64,
    #[serde(default)]
    pub ramp_feed: Option<f64>,
}

fn inside<S: SectionWindow + ?Sized>(s: &S, p: P3) -> bool {
    let (lo, hi) = (s.cut_min(), s.cut_max());
    (0..2).all(|k| p[k] >= lo[k] - 1e-9 && p[k] <= hi[k] + 1e-9)
}

fn at(from: P3, m: Move, t: f64) -> P3 {
    let to = m.to();
    if t <= 1e-12 {
        return from;
    }
    if t >= 1.0 - 1e-12 {
        return to;
    }
    match m {
        Move::Arc { center, dir, .. } => {
            let r = (from[0] - center[0]).hypot(from[1] - center[1]);
            let a = (from[1] - center[1]).atan2(from[0] - center[0])
                + kmb_toolpath::arc_sweep(from, to, center, dir) * t;
            [
                center[0] + r * a.cos(),
                center[1] + r * a.sin(),
                from[2] + (to[2] - from[2]) * t,
            ]
        }
        _ => [0, 1, 2].map(|k| from[k] + (to[k] - from[k]) * t),
    }
}

fn end_at(m: Move, to: P3) -> Move {
    match m {
        Move::Linear { feed, .. } => Move::Linear { to, feed },
        Move::Arc {
            center, dir, feed, ..
        } => Move::Arc {
            to,
            center,
            dir,
            feed,
        },
        Move::Rapid { .. } => Move::Rapid { to },
    }
}

fn cuts<S: SectionWindow + ?Sized>(from: P3, m: Move, s: &S) -> Vec<(f64, f64)> {
    let mut ts = vec![0.0, 1.0];
    let to = m.to();
    match m {
        Move::Arc { center, dir, .. } => {
            let radius = (from[0] - center[0]).hypot(from[1] - center[1]);
            let start = (from[1] - center[1]).atan2(from[0] - center[0]);
            let sweep = kmb_toolpath::arc_sweep(from, to, center, dir);
            for k in 0..2 {
                for edge in [s.cut_min()[k], s.cut_max()[k]] {
                    if !edge.is_finite() {
                        continue;
                    }
                    let d = edge - center[k];
                    if d.abs() > radius + 1e-9 {
                        continue;
                    }
                    let other = (radius * radius - d * d).max(0.0).sqrt();
                    for sign in [-1.0, 1.0] {
                        let mut p = center;
                        p[k] = edge;
                        p[1 - k] += sign * other;
                        let a = (p[1] - center[1]).atan2(p[0] - center[0]);
                        let travel = if sweep >= 0.0 {
                            (a - start).rem_euclid(std::f64::consts::TAU)
                        } else {
                            -(start - a).rem_euclid(std::f64::consts::TAU)
                        };
                        let t = travel / sweep;
                        if t > 1e-12 && t < 1.0 - 1e-12 {
                            ts.push(t);
                        }
                    }
                }
            }
        }
        _ => {
            for k in 0..2 {
                if (to[k] - from[k]).abs() < 1e-12 {
                    continue;
                }
                for edge in [s.cut_min()[k], s.cut_max()[k]] {
                    let t = (edge - from[k]) / (to[k] - from[k]);
                    if t > 1e-12 && t < 1.0 - 1e-12 {
                        ts.push(t);
                    }
                }
            }
        }
    }
    ts.sort_by(f64::total_cmp);
    ts.dedup_by(|a, b| (*a - *b).abs() < 1e-10);
    ts.windows(2)
        .filter_map(|pair| {
            inside(s, at(from, m, (pair[0] + pair[1]) / 2.0)).then_some((pair[0], pair[1]))
        })
        .collect()
}

fn xy_same(a: P3, b: P3) -> bool {
    (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-9
}

fn connect<S: SectionWindow + ?Sized>(
    out: &mut Vec<Move>,
    start: P3,
    first: Move,
    s: &S,
    tp: &Toolpath,
    opts: &ClipOpts,
    warnings: &mut Vec<String>,
) {
    let clearance = opts.clearance_z.max(tp.safe_z).max(tp.retract_z);
    if let Some(previous) = out.last().map(Move::to) {
        out.push(Move::Rapid {
            to: [previous[0], previous[1], clearance],
        });
    }
    out.push(Move::Rapid {
        to: [start[0], start[1], clearance],
    });
    out.push(Move::Rapid {
        to: [start[0], start[1], tp.safe_z],
    });
    out.push(Move::Rapid {
        to: [start[0], start[1], tp.retract_z],
    });
    if start[2] >= tp.retract_z {
        out.push(Move::Rapid { to: start });
        return;
    }
    if s.uncut_entry([start[0], start[1]]) {
        let to = first.to();
        let length = match first {
            Move::Arc { center, dir, .. } => {
                (start[0] - center[0]).hypot(start[1] - center[1])
                    * kmb_toolpath::arc_sweep(start, to, center, dir).abs()
            }
            _ => (to[0] - start[0]).hypot(to[1] - start[1]),
        };
        let need = (tp.retract_z - start[2]) / opts.ramp_angle.tan();
        if opts.ramp_angle > 0.0
            && opts.ramp_angle < std::f64::consts::FRAC_PI_2
            && length >= need
            && length > 1e-9
        {
            let p = at(start, first, (need / (2.0 * length)).min(0.5));
            // 往復する輪郭は切れ端の中に収まり、始点の切削深さへ戻る。
            let mut l = kmb_cam_geom::Loop::from_points(&[[start[0], start[1]], [p[0], p[1]]]);
            if let Move::Arc { center, dir, .. } = first {
                let sweep = kmb_toolpath::arc_sweep(start, p, center, dir);
                l.verts[0].bulge = (sweep / 4.0).tan();
                l.verts[1].bulge = -l.verts[0].bulge;
            }
            let feed = opts.ramp_feed.unwrap_or(match first {
                Move::Linear { feed, .. } | Move::Arc { feed, .. } => feed,
                _ => opts.plunge,
            });
            out.extend(crate::ramp_moves(
                &l,
                tp.retract_z,
                start[2],
                opts.ramp_angle,
                feed,
                opts.plunge,
            ));
            return;
        }
        warnings
            .push("区画の境から傾斜して入る長さが足りないため、突っ込みの送りで下げます".into());
    }
    out.push(Move::Linear {
        to: start,
        feed: opts.plunge,
    });
}

/// 切削の切れ端を安全な早送りでつなぎ、区画 1 の座標へ動かす。
/// XY が変わらない穴の動きは、半開区間の所有者だけへ渡す。
pub fn clip_to_section<S: SectionWindow + ?Sized>(
    tp: &Toolpath,
    sec: &S,
    opts: &ClipOpts,
) -> (Toolpath, Vec<String>) {
    let mut out = tp.clone();
    out.moves.clear();
    out.dwells.clear();
    out.spans.clear();
    out.links = Default::default();
    let mut warnings = Vec::new();
    let mut from = None;
    let mut connected = false;
    for &m in &tp.moves {
        let to = m.to();
        if let Some(start) = from {
            if matches!(m, Move::Rapid { .. }) {
                connected = false;
            } else if xy_same(start, to) && !matches!(m, Move::Arc { .. }) {
                if sec.owns([to[0], to[1]]) {
                    if !connected {
                        connect(&mut out.moves, start, m, sec, tp, opts, &mut warnings);
                    }
                    out.moves.push(m);
                    connected = true;
                } else {
                    connected = false;
                }
            } else {
                let fragments = cuts(start, m, sec);
                for (a, b) in fragments {
                    let begin = at(start, m, a);
                    let end = at(start, m, b);
                    let fragment = end_at(m, end);
                    if !connected
                        || out
                            .moves
                            .last()
                            .is_none_or(|p| kmb_toolpath::dist(p.to(), begin) > 1e-7)
                    {
                        connect(
                            &mut out.moves,
                            begin,
                            fragment,
                            sec,
                            tp,
                            opts,
                            &mut warnings,
                        );
                    }
                    out.moves.push(fragment);
                    connected = b >= 1.0 - 1e-12;
                }
                if !inside(sec, to) {
                    connected = false;
                }
            }
        }
        from = Some(to);
    }
    for dwell in &tp.dwells {
        if out.moves.iter().any(|m| {
            !matches!(m, Move::Rapid { .. }) && kmb_toolpath::dist(m.to(), dwell.at) < 1e-7
        }) {
            out.dwells.push(*dwell);
        }
    }
    if let Some(last) = out.moves.last().map(Move::to) {
        out.moves.push(Move::Rapid {
            to: [last[0], last[1], opts.clearance_z.max(tp.safe_z)],
        });
    }
    let shift = sec.shift();
    let xf = Xform2::translate(shift[0], shift[1]);
    out.moves = out.moves.iter().map(|m| xf.apply(m)).collect();
    for d in &mut out.dwells {
        d.at = xf.point(d.at);
    }
    (out, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kmb_toolpath::{ArcDir, Dwell};

    struct Window {
        min: [f64; 2],
        max: [f64; 2],
        shift: [f64; 2],
        uncut: bool,
    }
    impl SectionWindow for Window {
        fn cut_min(&self) -> [f64; 2] {
            self.min
        }
        fn cut_max(&self) -> [f64; 2] {
            self.max
        }
        fn owns(&self, p: [f64; 2]) -> bool {
            (0..2).all(|k| p[k] >= self.min[k] && p[k] < self.max[k])
        }
        fn shift(&self) -> [f64; 2] {
            self.shift
        }
        fn uncut_entry(&self, p: [f64; 2]) -> bool {
            self.uncut && (p[0] - self.max[0]).abs() < 1e-8
        }
    }
    fn tp(moves: Vec<Move>) -> Toolpath {
        Toolpath {
            name: "窓".into(),
            tool: kmb_cam_tool::ToolLibrary::starter().tools[0].clone(),
            rpm: 10000.0,
            safe_z: 5.0,
            retract_z: 1.0,
            moves,
            edge_break: 0.0,
            dwells: vec![],
            spans: vec![],
            links: Default::default(),
        }
    }
    fn opts() -> ClipOpts {
        ClipOpts {
            clearance_z: 20.0,
            ramp_angle: 3.0f64.to_radians(),
            plunge: 50.0,
            ramp_feed: None,
        }
    }
    fn window(min: f64, max: f64) -> Window {
        Window {
            min: [min, -20.0],
            max: [max, 20.0],
            shift: [0.0; 2],
            uncut: false,
        }
    }
    fn xy_length(tp: &Toolpath) -> f64 {
        let mut length = 0.0;
        for pair in tp.moves.windows(2) {
            let a = pair[0].to();
            let m = pair[1];
            let b = m.to();
            length += match m {
                Move::Rapid { .. } => 0.0,
                Move::Linear { .. } => (b[0] - a[0]).hypot(b[1] - a[1]),
                Move::Arc { center, dir, .. } => {
                    (a[0] - center[0]).hypot(a[1] - center[1])
                        * kmb_toolpath::arc_sweep(a, b, center, dir).abs()
                }
            };
        }
        length
    }

    #[test]
    fn neighboring_windows_preserve_linear_cut_length_and_shift_the_second_sheet() {
        let original = tp(vec![
            Move::Rapid {
                to: [-10.0, 0.0, 5.0],
            },
            Move::Linear {
                to: [-10.0, 0.0, -1.0],
                feed: 50.0,
            },
            Move::Linear {
                to: [10.0, 0.0, -1.0],
                feed: 300.0,
            },
        ]);
        let (a, _) = clip_to_section(&original, &window(-20.0, 0.0), &opts());
        let mut second = window(0.0, 20.0);
        second.shift = [-20.0, 0.0];
        let (b, _) = clip_to_section(&original, &second, &opts());
        assert!((xy_length(&a) + xy_length(&b) - 20.0).abs() < 1e-8);
        assert!(
            b.moves
                .iter()
                .all(|m| m.to()[0] <= -10.0 + 1e-8 && m.to()[0] >= -20.0 - 1e-8)
        );
        assert!(
            b.moves
                .windows(2)
                .all(|p| !matches!(p[1], Move::Rapid { .. })
                    || xy_same(p[0].to(), p[1].to())
                    || p[1].to()[2] >= 5.0)
        );
    }

    #[test]
    fn circle_line_intersections_split_arcs_without_polyline_approximation() {
        for dir in [ArcDir::Ccw, ArcDir::Cw] {
            let original = tp(vec![
                Move::Rapid {
                    to: [10.0, 0.0, 5.0],
                },
                Move::Linear {
                    to: [10.0, 0.0, -1.0],
                    feed: 50.0,
                },
                Move::Arc {
                    to: [10.0, 0.0, -1.0],
                    center: [0.0; 2],
                    dir,
                    feed: 300.0,
                },
            ]);
            let (left, _) = clip_to_section(&original, &window(-20.0, 0.0), &opts());
            let (right, _) = clip_to_section(&original, &window(0.0, 20.0), &opts());
            assert!(
                (xy_length(&left) + xy_length(&right) - std::f64::consts::TAU * 10.0).abs() < 1e-7
            );
            assert!(left.moves.iter().any(|m| matches!(m, Move::Arc { .. })));
            for m in &left.moves {
                if let Move::Arc { to, center, .. } = m {
                    assert!((to[0].hypot(to[1]) - 10.0).abs() < 1e-8);
                    assert_eq!(*center, [0.0; 2]);
                }
            }
        }
    }

    #[test]
    fn boundary_hole_is_owned_once_and_dwell_follows_translation() {
        let mut original = tp(vec![
            Move::Rapid {
                to: [0.0, 0.0, 5.0],
            },
            Move::Linear {
                to: [0.0, 0.0, -3.0],
                feed: 50.0,
            },
            Move::Rapid {
                to: [0.0, 0.0, 5.0],
            },
        ]);
        original.dwells.push(Dwell {
            at: [0.0, 0.0, -3.0],
            seconds: 0.5,
        });
        let (a, _) = clip_to_section(&original, &window(-10.0, 0.0), &opts());
        let mut second = window(0.0, 10.0);
        second.shift = [-30.0, 4.0];
        let (b, _) = clip_to_section(&original, &second, &opts());
        assert!(a.moves.is_empty());
        assert!(a.dwells.is_empty());
        assert_eq!(
            b.dwells,
            vec![Dwell {
                at: [-30.0, 4.0, -3.0],
                seconds: 0.5
            }]
        );
        assert_eq!(
            b.moves
                .iter()
                .filter(|m| matches!(m,Move::Linear { to,.. } if to[2]==-3.0))
                .count(),
            1
        );
        assert!(b.spans.is_empty());
        assert!(b.links.is_empty());
    }

    #[test]
    fn short_uncut_boundary_entry_uses_plunge_and_reports_warning() {
        let original = tp(vec![
            Move::Rapid {
                to: [20.0, 0.0, 5.0],
            },
            Move::Linear {
                to: [19.0, 0.0, -2.0],
                feed: 100.0,
            },
            Move::Linear {
                to: [0.0, 0.0, -2.0],
                feed: 300.0,
            },
        ]);
        let mut s = window(0.0, 10.0);
        s.uncut = true;
        let (out, warnings) = clip_to_section(&original, &s, &opts());
        assert!(!warnings.is_empty());
        assert!(
            out.moves
                .iter()
                .any(|m| matches!(m,Move::Linear { feed,.. } if *feed==50.0))
        );
        assert_eq!(out.moves.last().unwrap().to()[2], 20.0);
    }

    #[test]
    fn long_boundary_entries_follow_lines_and_arcs_at_the_configured_ramp_feed() {
        let deep = 1.0 - std::f64::consts::TAU * 10.0 * 10.0f64.to_radians().tan() * (1.0 - 5e-13);
        let cases = [
            tp(vec![
                Move::Rapid {
                    to: [20.0, 0.0, 5.0],
                },
                Move::Linear {
                    to: [20.0, 0.0, -2.0],
                    feed: 50.0,
                },
                Move::Linear {
                    to: [-100.0, 0.0, -2.0],
                    feed: 300.0,
                },
            ]),
            tp(vec![
                Move::Rapid {
                    to: [10.0, 0.0, 5.0],
                },
                Move::Linear {
                    to: [10.0, 0.0, -0.1],
                    feed: 50.0,
                },
                Move::Arc {
                    to: [-10.0, 0.0, -0.1],
                    center: [0.0; 2],
                    dir: ArcDir::Ccw,
                    feed: 300.0,
                },
            ]),
            tp(vec![
                Move::Rapid {
                    to: [10.0, 0.0, 5.0],
                },
                Move::Linear {
                    to: [10.0, 0.0, deep],
                    feed: 50.0,
                },
                Move::Arc {
                    to: [10.0, 0.0, deep],
                    center: [0.0; 2],
                    dir: ArcDir::Ccw,
                    feed: 300.0,
                },
            ]),
        ];
        let mut s = window(-100.0, 10.0);
        s.uncut = true;
        let mut opts = opts();
        opts.ramp_angle = 10.0f64.to_radians();
        opts.ramp_feed = Some(80.0);
        for original in cases {
            let (out, warnings) = clip_to_section(&original, &s, &opts);
            assert!(warnings.is_empty(), "{warnings:?}");
            let mut ramps = 0;
            for pair in out.moves.windows(2) {
                let a = pair[0].to();
                let b = pair[1].to();
                let length = match pair[1] {
                    Move::Linear { feed, .. } if feed == 80.0 => (b[0] - a[0]).hypot(b[1] - a[1]),
                    Move::Arc {
                        center, dir, feed, ..
                    } if feed == 80.0 => {
                        assert!(center[0].hypot(center[1]) < 1e-7);
                        (a[0] - center[0]).hypot(a[1] - center[1])
                            * kmb_toolpath::arc_sweep(a, b, center, dir).abs()
                    }
                    _ => continue,
                };
                ramps += 1;
                assert!(length > 0.0);
                assert!(a[2] - b[2] <= length * opts.ramp_angle.tan() + 1e-7);
            }
            assert!(ramps >= 2);
            assert!(
                !out.moves
                    .iter()
                    .any(|m| matches!(m,Move::Linear { feed,.. } if *feed==50.0))
            );
        }
    }
}
