//! 輪郭のフォントの字を、線分・円弧の閉じた輪と、押し出し・ポケット・V 彫りの領域にする。
//!
//! 字の輪の向きはフォントで逆(TrueType は外が時計回り、CFF は外が反時計回りが慣例)なので、
//! 向きに頼らず入れ子の深さで外と穴を決める(偶数 = 外、奇数 = 穴)。「回」は外・穴・中の島・
//! 中の穴の 4 重で、中の島は別の領域になる。輪どうしが交わる字(可変フォントの字、字の間隔を
//! 詰めて隣の字と重なった時)は、交わる輪だけを非ゼロの塗りで 1 つにまとめる(i_overlay。
//! まとめた輪は折れ線になる)。交わらない輪は円弧のまま残す。

use crate::outline::{Contour, Piece};
use kmb_core::{ProfileEdge, ProfileLoop, Region, Segment};

type P2 = [f64; 2];

/// 字の輪郭を線分・円弧にする時の許容差(mm)。スケッチのスプラインを輪郭にする時と同じ。
pub const TOLERANCE: f64 = 0.002;

fn near(a: P2, b: P2, tol: f64) -> bool {
    (a[0] - b[0]).hypot(a[1] - b[1]) <= tol
}

/// 区間の輪(mm)を、たどる向きつきの線分・円弧の輪にする。
pub fn approximate(c: &Contour, tol: f64) -> Vec<(Segment, bool)> {
    let mut out = Vec::new();
    let Some(first) = c.first() else {
        return out;
    };
    let mut at = first.points()[0];
    for p in c {
        let shapes = match *p {
            Piece::Line(a, b) => vec![kmb_sketch::Shape::Line { a, b }],
            Piece::Cubic(q) => {
                kmb_sketch::approximate_tol(&kmb_sketch::Shape::Bezier { p: q }, tol)
            }
        };
        for s in shapes {
            match s {
                kmb_sketch::Shape::Line { a, b } => {
                    let to = if near(a, at, 1e-7) { b } else { a };
                    if !near(at, to, 1e-12) {
                        out.push((Segment::Line { a: at, b: to }, false));
                        at = to;
                    }
                }
                kmb_sketch::Shape::Arc {
                    center,
                    radius,
                    start,
                    end,
                } => {
                    let seg = Segment::Arc {
                        center,
                        radius,
                        start,
                        end,
                    };
                    let (s0, s1) = (seg.start_point(), seg.end_point());
                    let back = !near(s0, at, 1e-6) && near(s1, at, 1e-6);
                    at = if back { s0 } else { s1 };
                    out.push((seg, back));
                }
                _ => {}
            }
        }
    }
    // 終わりを始まりへ(丸めの誤差の分)
    let start = first.points()[0];
    if !near(at, start, 1e-9) {
        out.push((Segment::Line { a: at, b: start }, false));
    }
    out
}

/// 区間を `f`(回転 `theta` と平行移動だけの写し)で動かす。
pub fn rigid(seg: &Segment, f: &impl Fn(P2) -> P2, theta: f64) -> Segment {
    match *seg {
        Segment::Line { a, b } => Segment::Line { a: f(a), b: f(b) },
        Segment::Arc {
            center,
            radius,
            start,
            end,
        } => Segment::Arc {
            center: f(center),
            radius,
            start: start + theta,
            end: end + theta,
        },
        Segment::Circle { center, radius } => Segment::Circle {
            center: f(center),
            radius,
        },
    }
}

/// 輪の折れ線(たどる向き)と外接の箱。
struct Ring {
    edges: Vec<(Segment, bool)>,
    pts: Vec<P2>,
    lo: P2,
    hi: P2,
}

impl Ring {
    fn new(edges: Vec<(Segment, bool)>) -> Ring {
        let pts: Vec<P2> = edges
            .iter()
            .flat_map(|(s, back)| {
                ProfileEdge {
                    name: String::new(),
                    segment: s.clone(),
                    reversed: *back,
                }
                .polyline(64)
            })
            .collect();
        let mut lo = [f64::MAX; 2];
        let mut hi = [f64::MIN; 2];
        for p in &pts {
            for i in 0..2 {
                lo[i] = lo[i].min(p[i]);
                hi[i] = hi[i].max(p[i]);
            }
        }
        Ring { edges, pts, lo, hi }
    }

    fn boxes_meet(&self, o: &Ring) -> bool {
        self.lo[0] <= o.hi[0]
            && o.lo[0] <= self.hi[0]
            && self.lo[1] <= o.hi[1]
            && o.lo[1] <= self.hi[1]
    }

    /// 点が輪の内側か(偶奇の数え方)。
    fn contains(&self, p: P2) -> bool {
        let n = self.pts.len();
        let mut inside = false;
        for i in 0..n {
            let (a, b) = (self.pts[i], self.pts[(i + 1) % n]);
            if (a[1] > p[1]) != (b[1] > p[1]) {
                let x = a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
                if p[0] < x {
                    inside = !inside;
                }
            }
        }
        inside
    }

    fn area(&self) -> f64 {
        kmb_core::kernel::polygon_area(&self.pts)
    }

    /// 輪の上の、角でない点(最初の区間の中ほど)。
    fn sample(&self) -> P2 {
        let (s, back) = &self.edges[0];
        let pl = ProfileEdge {
            name: String::new(),
            segment: s.clone(),
            reversed: *back,
        }
        .polyline(64);
        match pl.len() {
            0 => self.pts[0],
            1 => {
                let e = if *back {
                    s.start_point()
                } else {
                    s.end_point()
                };
                [(pl[0][0] + e[0]) / 2.0, (pl[0][1] + e[1]) / 2.0]
            }
            n => pl[n / 2],
        }
    }
}

/// 線分どうしが交わるか(端の触れ合いも含む)。
fn cross(a: P2, b: P2, c: P2, d: P2) -> bool {
    let o = |p: P2, q: P2, r: P2| (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]);
    let (d1, d2, d3, d4) = (o(c, d, a), o(c, d, b), o(a, b, c), o(a, b, d));
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
    {
        return true;
    }
    let on = |p: P2, q: P2, r: P2, v: f64| {
        v.abs() < 1e-12
            && r[0] >= p[0].min(q[0]) - 1e-12
            && r[0] <= p[0].max(q[0]) + 1e-12
            && r[1] >= p[1].min(q[1]) - 1e-12
            && r[1] <= p[1].max(q[1]) + 1e-12
    };
    on(c, d, a, d1) || on(c, d, b, d2) || on(a, b, c, d3) || on(a, b, d, d4)
}

fn rings_meet(a: &Ring, b: &Ring) -> bool {
    if !a.boxes_meet(b) {
        return false;
    }
    let (n, m) = (a.pts.len(), b.pts.len());
    (0..n).any(|i| {
        let (p, q) = (a.pts[i], a.pts[(i + 1) % n]);
        let (lo, hi) = (
            [p[0].min(q[0]), p[1].min(q[1])],
            [p[0].max(q[0]), p[1].max(q[1])],
        );
        if hi[0] < b.lo[0] || lo[0] > b.hi[0] || hi[1] < b.lo[1] || lo[1] > b.hi[1] {
            return false;
        }
        (0..m).any(|j| cross(p, q, b.pts[j], b.pts[(j + 1) % m]))
    })
}

/// 自分自身と交わる輪か(隣り合わない辺どうし)。
fn self_crossing(r: &Ring) -> bool {
    let n = r.pts.len();
    if n < 4 {
        return false;
    }
    (0..n).any(|i| {
        ((i + 2)..n).any(|j| {
            if i == 0 && j == n - 1 {
                return false;
            }
            cross(r.pts[i], r.pts[(i + 1) % n], r.pts[j], r.pts[(j + 1) % n])
        })
    })
}

fn find(p: &mut [usize], i: usize) -> usize {
    let mut i = i;
    while p[i] != i {
        p[i] = p[p[i]];
        i = p[i];
    }
    i
}

/// 字の輪(どれも閉じた輪)を領域にする。外は反時計回り、穴は時計回り。
pub fn regions(loops: Vec<Vec<(Segment, bool)>>) -> Vec<Region> {
    use i_overlay::core::fill_rule::FillRule;
    use i_overlay::float::simplify::SimplifyShape;
    let rings: Vec<Ring> = loops
        .into_iter()
        .filter(|l| !l.is_empty())
        .map(Ring::new)
        .collect();
    // 交わる輪を組にする
    let n = rings.len();
    let mut parent: Vec<usize> = (0..n).collect();
    let mut tangled = vec![false; n];
    for i in 0..n {
        if self_crossing(&rings[i]) {
            tangled[i] = true;
        }
        for j in i + 1..n {
            if rings_meet(&rings[i], &rings[j]) {
                tangled[i] = true;
                tangled[j] = true;
                let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                parent[a] = b;
            }
        }
    }
    let mut simple: Vec<Ring> = Vec::new();
    let mut groups: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
    for (i, r) in rings.into_iter().enumerate() {
        if tangled[i] {
            groups
                .entry(find(&mut parent, i))
                .or_default()
                .push(simple.len());
        }
        simple.push(r);
    }
    // 交わる輪の組は、非ゼロの塗りでまとめた折れ線の輪に替える
    let mut keep: Vec<Option<Ring>> = simple.into_iter().map(Some).collect();
    let mut merged: Vec<Ring> = Vec::new();
    for members in groups.values() {
        let paths: Vec<Vec<P2>> = members
            .iter()
            .filter_map(|&k| keep[k].take().map(|r| r.pts))
            .collect();
        for shape in paths.simplify_shape(FillRule::NonZero) {
            for c in shape {
                if c.len() < 3 {
                    continue;
                }
                let m = c.len();
                let edges = (0..m)
                    .map(|i| {
                        (
                            Segment::Line {
                                a: c[i],
                                b: c[(i + 1) % m],
                            },
                            false,
                        )
                    })
                    .collect();
                merged.push(Ring::new(edges));
            }
        }
    }
    let all: Vec<Ring> = keep.into_iter().flatten().chain(merged).collect();
    // 入れ子の深さ(自分を囲む輪の数)と、すぐ外の輪
    let depth_parent: Vec<(usize, Option<usize>)> = (0..all.len())
        .map(|i| {
            let p = all[i].sample();
            let around: Vec<usize> = (0..all.len())
                .filter(|&j| j != i && all[j].boxes_meet(&all[i]) && all[j].contains(p))
                .collect();
            let nearest = around
                .iter()
                .copied()
                .min_by(|&a, &b| all[a].area().abs().total_cmp(&all[b].area().abs()));
            (around.len(), nearest)
        })
        .collect();
    let to_loop = |k: usize, r: &Ring, ccw: bool| {
        let mut l = ProfileLoop {
            edges: r
                .edges
                .iter()
                .enumerate()
                .map(|(i, (s, back))| ProfileEdge {
                    name: format!("g{k}.{i}"),
                    segment: s.clone(),
                    reversed: *back,
                })
                .collect(),
        };
        if (l.signed_area() > 0.0) != ccw {
            l.reverse();
        }
        l
    };
    let mut out: Vec<(usize, Region)> = Vec::new();
    for (i, r) in all.iter().enumerate() {
        if depth_parent[i].0.is_multiple_of(2) {
            out.push((
                i,
                Region {
                    outer: to_loop(i, r, true),
                    holes: Vec::new(),
                },
            ));
        }
    }
    for (i, r) in all.iter().enumerate() {
        let (d, p) = depth_parent[i];
        if d % 2 == 1
            && let Some(p) = p
            && let Some((_, reg)) = out.iter_mut().find(|(k, _)| *k == p)
        {
            reg.holes.push(to_loop(i, r, false));
        }
    }
    out.into_iter().map(|(_, r)| r).collect()
}
