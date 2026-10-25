//! 薄い押し出しの輪郭: 線(開いた線も閉じた線も)を、壁の厚さだけ太らせた閉じた領域にする。
//! 線のオフセットは cavalier_contours に任せ、円弧は円弧のまま返す。
use cavalier_contours::polyline::{PlineSource, PlineSourceMut, Polyline};
use jc_core::kernel::{
    P2, Plane, Profile, ProfileEdge, ProfileLoop, Region, Segment, arc_sweep, canonical_loop,
};
use serde::{Deserialize, Serialize};

/// 壁を線のどちら側に付けるか。開いた線の「内側」はたどる向きの左、閉じた線は囲む側。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Wall {
    /// 線を真ん中に、両側へ厚さの半分ずつ
    #[default]
    Center,
    Inside,
    Outside,
}

/// 線のつながり 1 本(点とふくらみ。閉じていれば最後から最初へもつながる)。
#[derive(Debug, Clone, PartialEq)]
pub struct Chain {
    /// 点と、次の点までのふくらみ(tan(回転角 / 4)、0 なら直線。反時計回りが正)
    pub verts: Vec<(P2, f64)>,
    pub closed: bool,
}

const TOL: f64 = 1e-6;

fn same(a: P2, b: P2) -> bool {
    (a[0] - b[0]).hypot(a[1] - b[1]) < TOL
}

/// 区間の始点・終点と、始点から終点へたどる時のふくらみ。円は `None`。
fn ends(s: &Segment) -> Option<(P2, P2, f64)> {
    match *s {
        Segment::Line { a, b } => Some((a, b, 0.0)),
        Segment::Arc { start, end, .. } => Some((
            s.start_point(),
            s.end_point(),
            (arc_sweep(start, end) / 4.0).tan(),
        )),
        Segment::Circle { .. } => None,
    }
}

/// 線の集まりを、端の点のつながりでたどった線の並びにする。分かれ道(3 本以上が集まる点)では
/// 線を分ける。円は 1 本の閉じた線。
pub fn chains(segments: &[Segment]) -> Vec<Chain> {
    let mut out = Vec::new();
    let mut open: Vec<(P2, P2, f64)> = Vec::new();
    for s in segments {
        match (s, ends(s)) {
            (Segment::Circle { center, radius }, _) => out.push(Chain {
                verts: vec![
                    ([center[0] + radius, center[1]], 1.0),
                    ([center[0] - radius, center[1]], 1.0),
                ],
                closed: true,
            }),
            (_, Some(e)) if !same(e.0, e.1) => open.push(e),
            _ => {}
        }
    }
    let degree = |p: P2, list: &[(P2, P2, f64)]| {
        list.iter()
            .map(|(a, b, _)| same(*a, p) as usize + same(*b, p) as usize)
            .sum::<usize>()
    };
    let mut used = vec![false; open.len()];
    // 端(つながる線が 1 本だけの点)か分かれ道から始めた線を先に、残り(輪になった線)を後に
    for pass in 0..2 {
        for i in 0..open.len() {
            if used[i] {
                continue;
            }
            let (a, b, _) = open[i];
            let start = match pass {
                0 if degree(a, &open) != 2 => a,
                0 if degree(b, &open) != 2 => b,
                0 => continue,
                _ => a,
            };
            used[i] = true;
            let (mut verts, mut at) = if same(start, a) {
                (vec![(a, open[i].2)], b)
            } else {
                (vec![(b, -open[i].2)], a)
            };
            loop {
                if same(at, verts[0].0) || degree(at, &open) != 2 {
                    break;
                }
                let Some(j) = (0..open.len())
                    .find(|&j| !used[j] && (same(open[j].0, at) || same(open[j].1, at)))
                else {
                    break;
                };
                used[j] = true;
                let (p, q, bulge) = open[j];
                if same(p, at) {
                    verts.push((p, bulge));
                    at = q;
                } else {
                    verts.push((q, -bulge));
                    at = p;
                }
            }
            let closed = same(at, verts[0].0);
            if !closed {
                verts.push((at, 0.0));
            }
            out.push(Chain { verts, closed });
        }
    }
    out
}

/// 閉じた輪郭(外周と穴)を、線のつながりにする。
pub fn loop_chain(l: &ProfileLoop) -> Chain {
    let mut verts = Vec::new();
    for e in &l.edges {
        match (&e.segment, ends(&e.segment)) {
            (Segment::Circle { center, radius }, _) => {
                let b = if e.reversed { -1.0 } else { 1.0 };
                verts.push(([center[0] + radius, center[1]], b));
                verts.push(([center[0] - radius, center[1]], b));
            }
            (_, Some((_, _, bulge))) => {
                verts.push((e.from_point(), if e.reversed { -bulge } else { bulge }))
            }
            _ => {}
        }
    }
    Chain {
        verts,
        closed: true,
    }
}

fn pline(c: &Chain) -> Polyline<f64> {
    let mut p = if c.closed {
        Polyline::new_closed()
    } else {
        Polyline::new()
    };
    for (v, b) in &c.verts {
        p.add(v[0], v[1], *b);
    }
    p
}

/// 弦 `a`→`b` とふくらみ `bulge` の円弧の中心と半径。
fn arc_of(a: P2, b: P2, bulge: f64) -> (P2, f64) {
    // 反時計回りなら弦の左に中心。半周を超えれば逆
    let sweep = 4.0 * bulge.atan();
    let chord = [b[0] - a[0], b[1] - a[1]];
    let c = chord[0].hypot(chord[1]);
    let r = c / (2.0 * (sweep / 2.0).sin().abs());
    let h = (r * r - c * c / 4.0).max(0.0).sqrt();
    let left = [-chord[1] / c, chord[0] / c];
    let side = if (bulge > 0.0) == (sweep.abs() < std::f64::consts::PI) {
        1.0
    } else {
        -1.0
    };
    let center = [
        (a[0] + b[0]) / 2.0 + left[0] * h * side,
        (a[1] + b[1]) / 2.0 + left[1] * h * side,
    ];
    (center, r)
}

/// ずらした線 `q` の角の丸め(cavalier が角に足す、元の線 `p` の点を中心にした半径 `d` の円弧)を、
/// 前後の線を延ばしてとがった角にする。とても鋭い角は丸めのまま。
fn sharpen(q: Polyline<f64>, p: &Polyline<f64>, d: f64) -> Polyline<f64> {
    let corners: Vec<P2> = (0..p.vertex_count())
        .map(|i| [p.at(i).x, p.at(i).y])
        .collect();
    let n = q.vertex_count();
    let mut out = if q.is_closed() {
        Polyline::new_closed()
    } else {
        Polyline::new()
    };
    for i in 0..n {
        let v = q.at(i);
        let a = [v.x, v.y];
        let next = (i + 1 < n || q.is_closed()).then(|| q.at((i + 1) % n));
        let miter = next.filter(|_| v.bulge.abs() > 1e-12).and_then(|w| {
            let b = [w.x, w.y];
            if same(a, b) {
                return None;
            }
            let (c, r) = arc_of(a, b, v.bulge);
            if (r - d.abs()).abs() > 1e-6 * (1.0 + r) || !corners.iter().any(|k| same(*k, c)) {
                return None;
            }
            // 半周の 5/6 を超える角は、とがらせると遠くまで伸びるので丸めのまま
            if 4.0 * v.bulge.atan().abs() > std::f64::consts::PI * 5.0 / 6.0 {
                return None;
            }
            // 円弧の両端の接線の交わり(二等分線の上、中心から r / cos(半分の角))
            let mid = [(a[0] + b[0]) / 2.0 - c[0], (a[1] + b[1]) / 2.0 - c[1]];
            let m = mid[0].hypot(mid[1]);
            (m > 1e-12).then(|| {
                let k = r * r / (m * m);
                [c[0] + mid[0] * k, c[1] + mid[1] * k]
            })
        });
        match miter {
            Some(m) => {
                out.add(a[0], a[1], 0.0);
                out.add(m[0], m[1], 0.0);
            }
            None => out.add(a[0], a[1], v.bulge),
        }
    }
    out
}

/// 線を左へ `d`(負なら右へ)ずらす。1 本にならなければ `None`。
fn offset(p: &Polyline<f64>, d: f64) -> Option<Polyline<f64>> {
    if d.abs() < 1e-12 {
        return Some(p.clone());
    }
    let mut v = p.parallel_offset(d);
    (v.len() == 1).then(|| sharpen(v.pop().expect("1 つある"), p, d))
}

/// 閉じた線を外へ `d` だけ広げる(負なら内へ縮める)。縮めて消えれば空。
fn grow(p: &Polyline<f64>, d: f64) -> Vec<Polyline<f64>> {
    if d.abs() < 1e-12 {
        return vec![p.clone()];
    }
    // cavalier は反時計回りの線を正の値で内へずらす
    let ccw = p.area() > 0.0;
    let s = if ccw { -d } else { d };
    p.parallel_offset(s)
        .into_iter()
        .map(|q| sharpen(q, p, d))
        .collect()
}

/// 線の点の並びを、名前 `name.番号` の区間の輪にする。
fn to_loop(p: &Polyline<f64>, name: &str) -> ProfileLoop {
    let n = p.vertex_count();
    let edges = (0..n)
        .filter_map(|i| {
            let v = p.at(i);
            let w = p.at((i + 1) % n);
            let (a, b) = ([v.x, v.y], [w.x, w.y]);
            if same(a, b) {
                return None;
            }
            let name = format!("{name}.{i}");
            if v.bulge.abs() < 1e-12 {
                return Some(ProfileEdge {
                    name,
                    segment: Segment::Line { a, b },
                    reversed: false,
                });
            }
            let (center, r) = arc_of(a, b, v.bulge);
            let ang = |q: P2| (q[1] - center[1]).atan2(q[0] - center[0]);
            let (start, end, reversed) = if v.bulge > 0.0 {
                (ang(a), ang(b), false)
            } else {
                (ang(b), ang(a), true)
            };
            Some(ProfileEdge {
                name,
                segment: Segment::Arc {
                    center,
                    radius: r,
                    start,
                    end,
                },
                reversed,
            })
        })
        .collect();
    ProfileLoop { edges }
}

/// 線 `c` を厚さ `t` の壁にした領域(閉じた線は外周と穴、開いた線は端を四角く閉じた輪)。
/// 名前は `name`(区間は `name.o.番号`・`name.i.番号` など)。
pub fn wall(c: &Chain, t: f64, wall: Wall, name: &str) -> Result<Vec<Region>, String> {
    if t <= 0.0 {
        return Err("壁の厚さは 0 より大きくしてください".into());
    }
    let p = pline(c);
    let (out_d, in_d) = match wall {
        Wall::Center => (t / 2.0, t / 2.0),
        Wall::Inside => (0.0, t),
        Wall::Outside => (t, 0.0),
    };
    if c.closed {
        let outer = grow(&p, out_d);
        let [outer] = &outer[..] else {
            return Err("壁を外へ広げると線が重なります。厚さを薄くしてください".into());
        };
        let holes = grow(&p, -in_d);
        return Ok(vec![Region {
            outer: canonical_loop(to_loop(outer, &format!("{name}.o")), true),
            holes: holes
                .iter()
                .enumerate()
                .map(|(k, h)| canonical_loop(to_loop(h, &format!("{name}.i{k}")), false))
                .collect(),
        }]);
    }
    // 開いた線: 左へずらした線をたどり、端を四角く閉じて、右へずらした線を戻る
    let crossed =
        || "線を厚くすると自分と重なります。厚さを薄くするか、線を分けてください".to_string();
    let (l, r) = match wall {
        Wall::Center => (offset(&p, t / 2.0), offset(&p, -t / 2.0)),
        Wall::Inside => (offset(&p, t), Some(p.clone())),
        Wall::Outside => (Some(p.clone()), offset(&p, -t)),
    };
    let (Some(l), Some(mut r)) = (l, r) else {
        return Err(crossed());
    };
    r.invert_direction_mut();
    let mut ring = Polyline::new_closed();
    for src in [&l, &r] {
        let n = src.vertex_count();
        for i in 0..n {
            let v = src.at(i);
            // 最後の点から次の線の始まりへは端の四角い線
            ring.add(v.x, v.y, if i + 1 == n { 0.0 } else { v.bulge });
        }
    }
    if ring.area().abs() < 1e-12 {
        return Err(crossed());
    }
    let mut lp = to_loop(&ring, name);
    if lp.signed_area() < 0.0 {
        lp.reverse();
    }
    Ok(vec![Region {
        outer: lp,
        holes: vec![],
    }])
}

/// 線の並びを壁にした輪郭(線ごとの領域。重なりは押し出した後にまとめる)。
pub fn walls(plane: Plane, chains: &[Chain], t: f64, w: Wall) -> Result<Vec<Profile>, String> {
    let mut out = Vec::new();
    for (k, c) in chains.iter().enumerate() {
        out.push(Profile {
            plane,
            regions: wall(c, t, w, &format!("w{k}"))?,
        });
    }
    if out.is_empty() {
        return Err("厚くする線がありません".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(rs: &[Region]) -> f64 {
        rs.iter()
            .map(|r| r.outer.signed_area() + r.holes.iter().map(|h| h.signed_area()).sum::<f64>())
            .sum()
    }

    #[test]
    fn an_open_line_becomes_a_rectangle_on_the_chosen_side() {
        let c = chains(&[Segment::Line {
            a: [0.0, 0.0],
            b: [10.0, 0.0],
        }]);
        assert_eq!(c.len(), 1);
        assert!(!c[0].closed);
        let center = wall(&c[0], 2.0, Wall::Center, "w").unwrap();
        assert!((area(&center) - 20.0).abs() < 1e-6);
        // 内側はたどる向きの左(+y)
        let inside = wall(&c[0], 2.0, Wall::Inside, "w").unwrap();
        let ys: Vec<f64> = inside[0].outer.polyline().iter().map(|p| p[1]).collect();
        assert!(ys.iter().all(|y| *y > -1e-9 && *y < 2.0 + 1e-9), "{ys:?}");
        let outside = wall(&c[0], 2.0, Wall::Outside, "w").unwrap();
        let ys: Vec<f64> = outside[0].outer.polyline().iter().map(|p| p[1]).collect();
        assert!(ys.iter().all(|y| *y < 1e-9 && *y > -2.0 - 1e-9), "{ys:?}");
    }

    #[test]
    fn a_closed_square_becomes_a_frame() {
        let sq = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let segs: Vec<Segment> = (0..4)
            .map(|i| Segment::Line {
                a: sq[i],
                b: sq[(i + 1) % 4],
            })
            .collect();
        let c = chains(&segs);
        assert_eq!(c.len(), 1);
        assert!(c[0].closed);
        // 内側: 外 10×10 − 内 8×8
        let f = wall(&c[0], 1.0, Wall::Inside, "w").unwrap();
        assert!((area(&f) - (100.0 - 64.0)).abs() < 1e-6, "{}", area(&f));
        assert_eq!(f[0].holes.len(), 1);
        // 外側: 角はとがったまま(12×12 − 10×10。丸めると 44 − (4 − π) になる)
        let f = wall(&c[0], 1.0, Wall::Outside, "w").unwrap();
        assert!((area(&f) - 44.0).abs() < 1e-6, "{}", area(&f));
    }

    #[test]
    fn the_outer_corner_of_a_thin_wall_stays_sharp() {
        let segs = [
            Segment::Line {
                a: [0.0, 0.0],
                b: [20.0, 0.0],
            },
            Segment::Line {
                a: [20.0, 0.0],
                b: [20.0, 10.0],
            },
        ];
        let c = chains(&segs);
        // 真ん中に 2 の壁: 外の角は (21, -1) まで、内の角は (19, 1)。面積は長さ×厚さ
        let f = wall(&c[0], 2.0, Wall::Center, "w").unwrap();
        assert!((area(&f) - 60.0).abs() < 1e-6, "{}", area(&f));
        let pts: Vec<P2> = f[0].outer.polyline();
        assert!(pts.iter().any(|p| same(*p, [21.0, -1.0])), "{pts:?}");
    }

    #[test]
    fn a_line_and_an_arc_chain_into_one_open_line() {
        // 直線の終わりから半円が続く(J の形)
        let segs = vec![
            Segment::Line {
                a: [0.0, 10.0],
                b: [0.0, 0.0],
            },
            Segment::Arc {
                center: [5.0, 0.0],
                radius: 5.0,
                start: std::f64::consts::PI,
                end: 0.0,
            },
        ];
        let c = chains(&segs);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].verts.len(), 3);
        let w = wall(&c[0], 1.0, Wall::Center, "w").unwrap();
        // 長さ 10 + π·5 の線を 1 mm に太らせた面積(面積は円弧を折れ線にして測るので少し小さい)
        let want = 10.0 + std::f64::consts::PI * 5.0;
        assert!((area(&w) - want).abs() < 0.05, "{} {want}", area(&w));
    }

    #[test]
    fn a_circle_becomes_a_ring() {
        let c = chains(&[Segment::Circle {
            center: [0.0, 0.0],
            radius: 5.0,
        }]);
        let w = wall(&c[0], 2.0, Wall::Center, "w").unwrap();
        // 面積は円弧を折れ線にして測るので少し小さい
        let want = std::f64::consts::PI * (36.0 - 16.0);
        assert!((area(&w) - want).abs() < 0.2, "{} {want}", area(&w));
        assert!(w[0].outer.edges.iter().all(|e| matches!(
            e.segment,
            Segment::Arc { radius, .. } if (radius - 6.0).abs() < 1e-9
        )));
    }
}
