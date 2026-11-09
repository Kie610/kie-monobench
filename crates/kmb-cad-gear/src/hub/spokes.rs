//! 腕の中心線・幅から、輪とボスへつながる同じ境界を作る純計算。
use super::{Layout, Parts, circle, reverse};
use crate::tooth::{self, P2, arc, edge, fit_biarcs};
use kmb_core::{PlanarPath, Plane, Profile, ProfileEdge, ProfileLoop, Region, Segment};
use serde::{Deserialize, Serialize};
use std::f64::consts::{PI, TAU};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Params {
    pub count: u32,
    pub root_width: f64,
    pub tip_width: f64,
    pub angle: f64,
    pub round: f64,
}

/// 厚さを持たない中心線。断面の作り方だけがボディ側の責務。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Arms {
    pub paths: Vec<PlanarPath>,
    pub root_width: f64,
    pub tip_width: f64,
    pub root_pads: Option<Profile>,
}

#[derive(Clone, Copy)]
struct Curve {
    x0: f64,
    x1: f64,
    angle: f64,
    root: f64,
    tip: f64,
}
fn norm(p: P2) -> f64 {
    p[0].hypot(p[1])
}
fn left(p: P2) -> P2 {
    [-p[1], p[0]]
}
fn add(a: P2, b: P2) -> P2 {
    [a[0] + b[0], a[1] + b[1]]
}
fn mul(p: P2, s: f64) -> P2 {
    [p[0] * s, p[1] * s]
}
impl Curve {
    fn at(self, t: f64) -> (P2, P2, f64) {
        let d = self.x1 - self.x0;
        if self.angle.abs() < 1e-10 {
            return ([self.x0 + d * t, 0.0], [1.0, 0.0], d);
        }
        let a = self.angle;
        let r = d / (2.0 * (a / 2.0).sin());
        let p = [
            self.x0 + 2.0 * r * (a * (t - 1.0) / 2.0).cos() * (a * t / 2.0).sin(),
            -2.0 * r * (a * t / 2.0).sin() * (a * (1.0 - t) / 2.0).sin(),
        ];
        let u = a * (t - 0.5);
        (p, [u.cos(), u.sin()], r * a)
    }
    fn side(self, sign: f64, t: f64) -> (P2, P2) {
        let (p, tangent, speed) = self.at(t);
        let h = (self.root + (self.tip - self.root) * t) / 2.0;
        let derivative = add(
            mul(tangent, speed - sign * h * self.angle),
            mul(left(tangent), sign * (self.tip - self.root) / 2.0),
        );
        (
            add(p, mul(left(tangent), sign * h)),
            mul(derivative, 1.0 / norm(derivative)),
        )
    }
    fn path(self, name: &str) -> ProfileEdge {
        let a = [self.x0, 0.0];
        let b = [self.x1, 0.0];
        if self.angle.abs() < 1e-10 {
            return edge(name, Segment::Line { a, b }, a);
        }
        let r = (self.x1 - self.x0) / (2.0 * (self.angle / 2.0).sin());
        arc(
            name,
            [(self.x0 + self.x1) / 2.0, r * (self.angle / 2.0).cos()],
            r.abs(),
            a,
            b,
            self.angle > 0.0,
        )
    }
}

// Root searches are bracketed, never projected/clamped to a different requested size.
fn crossing(f: impl Fn(f64) -> f64) -> Result<f64, String> {
    let mut previous = f(0.0);
    let mut lo = 0.0;
    let mut found = None;
    for i in 1..=256 {
        let hi = i as f64 / 256.0;
        let value = f(hi);
        if !value.is_finite() || !previous.is_finite() {
            return Err("腕の寸法から有限の境界を作れません".into());
        }
        if previous <= 0.0 && value >= 0.0 && value > previous {
            if found.is_some() {
                return Err(
                    "腕が同じ円を何度も横切ります。曲がり角か幅を小さくしてください".into(),
                );
            }
            let (mut a, mut b) = (lo, hi);
            for _ in 0..60 {
                let m = (a + b) / 2.0;
                if f(m) < 0.0 {
                    a = m;
                } else {
                    b = m;
                }
            }
            found = Some((a + b) / 2.0);
        }
        if previous > 0.0 && value < 0.0 {
            return Err("腕が輪の内側へ戻ります。曲がり角か幅を小さくしてください".into());
        }
        previous = value;
        lo = hi;
    }
    found.ok_or_else(|| "腕がボスからリムへ届きません。幅か曲がり角を小さくしてください".into())
}

struct Boundary {
    foot: P2,
    rim: P2,
    outward: Vec<ProfileEdge>,
    pad: Vec<ProfileEdge>,
}
fn boundary(
    c: Curve,
    sign: f64,
    boss: f64,
    rim: f64,
    round: f64,
    tol: f64,
    name: &str,
) -> Result<Boundary, String> {
    let tb = crossing(|t| norm(c.side(sign, t).0) - boss)?;
    let tr = crossing(|t| norm(c.side(sign, t).0) - rim)?;
    let center = |t: f64| {
        let (p, v) = c.side(sign, t);
        add(p, mul(left(v), sign * round))
    };
    let t = if round > 0.0 {
        crossing(|t| norm(center(t)) - (boss + round))?
    } else {
        tb
    };
    if t >= tr || tr >= 1.0 || t < tb {
        return Err(format!(
            "付け根の丸みは {:.3} mm 未満にしてください",
            (rim - boss) / 2.0
        ));
    }
    let p = c.side(sign, t).0;
    let foot = if round > 0.0 {
        mul(center(t), boss / (boss + round))
    } else {
        p
    };
    let mut outward = Vec::new();
    if round > 0.0 {
        outward.push(arc(
            format!("{name}:root"),
            center(t),
            round,
            foot,
            p,
            sign > 0.0,
        ));
    }
    let append = |a: f64, b: f64, label: &str, out: &mut Vec<ProfileEdge>| {
        let mut from = c.side(sign, a).0;
        for (i, segment) in fit_biarcs(&|t| c.side(sign, t), a, b, tol)
            .into_iter()
            .enumerate()
        {
            let e = edge(format!("{name}:{label}:{i}"), segment, from);
            from = e.to_point();
            out.push(e);
        }
    };
    // A short full-width reinforcement overlaps the swept section instead of replacing it.
    let pad_radius = (boss + 2.1 * round).min(boss + (rim - boss) / 2.0);
    let tp = if round > 0.0 {
        crossing(|t| norm(c.side(sign, t).0) - pad_radius)?
    } else {
        t
    };
    if round > 0.0 && (tp <= t || tp >= tr) {
        return Err(format!(
            "付け根の丸みは {:.3} mm 未満を目安に小さくしてください",
            (rim - boss) / 2.0
        ));
    }
    let mut pad = outward.clone();
    if round > 0.0 {
        append(t, tp, "pad", &mut pad);
    }
    append(t, tr, "side", &mut outward);
    Ok(Boundary {
        foot,
        rim: c.side(sign, tr).0,
        outward,
        pad,
    })
}
fn rotated(edges: &[ProfileEdge], a: f64) -> Vec<ProfileEdge> {
    edges.iter().map(|e| tooth::rotate(e, a)).collect()
}
fn rev(edges: &[ProfileEdge]) -> Vec<ProfileEdge> {
    reverse(ProfileLoop {
        edges: edges.to_vec(),
    })
    .edges
}
fn rotation(p: P2, a: f64) -> P2 {
    [
        p[0] * a.cos() - p[1] * a.sin(),
        p[0] * a.sin() + p[1] * a.cos(),
    ]
}
fn angle(p: P2) -> f64 {
    p[1].atan2(p[0])
}

pub fn layout(
    outer: ProfileLoop,
    root: f64,
    module: f64,
    rim: f64,
    boss: f64,
    bore: Option<ProfileLoop>,
    p: &Params,
) -> Result<Layout, String> {
    if !(3..=12).contains(&p.count) || boss <= 0.0 {
        return Err("腕には 0 より大きいボスの外径と、3 から 12 本の腕が必要です".into());
    }
    if [p.root_width, p.tip_width, p.angle, p.round, boss, rim, root]
        .iter()
        .any(|v| !v.is_finite())
        || p.root_width <= 0.0
        || p.tip_width <= 0.0
        || p.round < 0.0
    {
        return Err("腕の幅は有限の 0 より大きい値、丸みは有限の 0 以上の値にしてください".into());
    }
    let max_width = 2.0 * boss * (PI / p.count as f64).sin();
    if p.root_width >= max_width {
        return Err(format!(
            "付け根の幅は {:.3} mm 未満にしてください(ボスの周囲に腕が収まる幅)",
            max_width
        ));
    }
    if p.angle.abs() >= TAU {
        return Err("腕の中心線は一周未満の円弧にしてください".into());
    }
    let half = p.angle / 2.0;
    let inset = |radius: f64, width: f64| {
        let transverse = half.cos() * width / 2.0;
        if transverse.abs() >= radius {
            return None;
        }
        let x = radius * ((1.0 - (transverse / radius).powi(2)).max(0.0)).sqrt()
            - half.sin().abs() * width / 2.0;
        (x.is_finite() && x > 0.0).then_some(x)
    };
    let x0 = inset(boss, p.root_width)
        .ok_or_else(|| "付け根の幅がボスの外径より大きすぎます".to_string())?
        * (1.0 - 1e-10);
    let x1 = inset(root, p.tip_width)
        .ok_or_else(|| format!("先の幅は {:.3} mm 未満にしてください", 2.0 * root))?;
    if x1 <= rim || x1 <= x0 {
        return Err(format!(
            "腕がリムに届きません。先の幅は {:.3} mm 未満を目安に小さくしてください",
            2.0 * (root * root - rim * rim).max(0.0).sqrt()
        ));
    }
    let c = Curve {
        x0,
        x1,
        angle: p.angle,
        root: p.root_width,
        tip: p.tip_width,
    };
    let (_, _, speed) = c.at(0.0);
    if !speed.is_finite() || speed <= p.root_width.max(p.tip_width) * p.angle.abs() / 2.0 {
        return Err("腕の幅が曲がりの半径にかかります。幅か曲がり角を小さくしてください".into());
    }
    let tolerance = tooth::tolerance(module);
    let minus = boundary(c, -1.0, boss, rim, p.round, tolerance, "arm:minus")?;
    let plus = boundary(c, 1.0, boss, rim, p.round, tolerance, "arm:plus")?;
    let step = TAU / p.count as f64;
    // Both sides must stay strictly inside their own angular sector at every station.
    // This also rejects neighboring arms touching before any loop is constructed.
    for i in 0..=512 {
        let t = i as f64 / 512.0;
        let a = angle(c.side(-1.0, t).0);
        let b = angle(c.side(1.0, t).0);
        if b <= a || b - a >= step || a <= -step / 2.0 || b >= step / 2.0 {
            return Err(format!(
                "腕どうしが重なります。付け根の幅は {:.3} mm 未満を目安に、先の幅か曲がり角も小さくしてください",
                max_width
            ));
        }
    }
    if angle(plus.foot) - angle(minus.foot) >= step {
        return Err("付け根の丸みが隣の腕にかかります。丸みを小さくしてください".into());
    }
    let mut gaps = Vec::new();
    let mut webs = Vec::new();
    let mut pads = Vec::new();
    let mut paths = Vec::new();
    for i in 0..p.count {
        let a = step * i as f64;
        let b = a + step;
        let rename = |mut e: ProfileEdge| {
            e.name = format!("spoke:{i}:{}", e.name);
            e
        };
        let mut gap = rotated(&plus.outward, a);
        gap.push(arc(
            "gap:rim",
            [0.0; 2],
            rim,
            rotation(plus.rim, a),
            rotation(minus.rim, b),
            true,
        ));
        gap.extend(rotated(&rev(&minus.outward), b));
        gap.push(arc(
            "gap:boss",
            [0.0; 2],
            boss,
            rotation(minus.foot, b),
            rotation(plus.foot, a),
            false,
        ));
        gaps.push(reverse(ProfileLoop {
            edges: gap.into_iter().map(rename).collect(),
        }));
        let mut web = minus.outward.clone();
        web.push(arc("web:rim", [0.0; 2], rim, minus.rim, plus.rim, true));
        web.extend(rev(&plus.outward));
        web.push(arc(
            "web:boss", [0.0; 2], boss, plus.foot, minus.foot, false,
        ));
        webs.push(Region {
            outer: ProfileLoop {
                edges: rotated(&web, a).into_iter().map(rename).collect(),
            },
            holes: vec![],
        });
        if p.round > 0.0 {
            let mut pad = minus.pad.clone();
            let m = pad.last().unwrap().to_point();
            let q = plus.pad.last().unwrap().to_point();
            pad.push(arc("pad:cap", [0.0; 2], norm(m), m, q, true));
            pad.extend(rev(&plus.pad));
            pad.push(arc(
                "pad:boss", [0.0; 2], boss, plus.foot, minus.foot, false,
            ));
            pads.push(Region {
                outer: ProfileLoop {
                    edges: rotated(&pad, a).into_iter().map(rename).collect(),
                },
                holes: vec![],
            });
        }
        paths.push(PlanarPath {
            plane: Plane::XY,
            edges: vec![tooth::rotate(&c.path(&format!("spoke:{i}:path")), a)],
        });
    }
    let finite = |edge: &ProfileEdge| match edge.segment {
        Segment::Line { a, b } => {
            a.into_iter().chain(b).all(f64::is_finite) && norm(add(a, mul(b, -1.0))) > 0.0
        }
        Segment::Arc {
            center,
            radius,
            start,
            end,
        } => {
            center
                .into_iter()
                .chain([radius, start, end])
                .all(f64::is_finite)
                && radius > 0.0
                && end > start
        }
        Segment::Circle { center, radius } => {
            center.into_iter().chain([radius]).all(f64::is_finite) && radius > 0.0
        }
    };
    if gaps
        .iter()
        .flat_map(|r| &r.edges)
        .chain(webs.iter().chain(&pads).flat_map(|r| &r.outer.edges))
        .chain(paths.iter().flat_map(|p| &p.edges))
        .any(|edge| !finite(edge))
    {
        return Err("腕の寸法から有限で長さのある境界を作れません".into());
    }
    if gaps.iter().any(|ring| !simple(ring))
        || webs
            .iter()
            .chain(&pads)
            .any(|region| !simple(&region.outer))
    {
        return Err("腕の境界が交差します。幅・曲がり角・付け根の丸みを小さくしてください".into());
    }
    for i in 0..gaps.len() {
        for j in (i + 1)..gaps.len() {
            if gaps[i].edges.iter().any(|a| {
                gaps[j]
                    .edges
                    .iter()
                    .any(|b| intersects(&a.segment, &b.segment))
            }) {
                return Err("腕の間の穴が接触します。幅か曲がり角を小さくしてください".into());
            }
        }
    }
    let make = |regions| Profile {
        plane: Plane::XY,
        regions,
    };
    let rim_profile = make(vec![Region {
        outer: outer.clone(),
        holes: vec![reverse(circle("rim:inner", [0.0; 2], rim))],
    }]);
    let boss_profile = make(vec![Region {
        outer: circle("boss:outer", [0.0; 2], boss),
        holes: bore.clone().into_iter().collect(),
    }]);
    gaps.extend(bore);
    let profile = make(vec![Region { outer, holes: gaps }]);
    Ok(Layout {
        profile,
        parts: Parts {
            rim: rim_profile,
            web: Some(make(webs)),
            boss: Some(boss_profile),
            spokes: Some(Arms {
                paths,
                root_width: p.root_width,
                tip_width: p.tip_width,
                root_pads: (!pads.is_empty()).then(|| make(pads)),
            }),
        },
    })
}

#[cfg(test)]
mod tests;

/// Exact line/circle candidate intersections, filtered by each finite arc's extent.
/// Names and the sampling used by the fitter do not determine validity.
fn intersects(a: &Segment, b: &Segment) -> bool {
    let epsilon = 1e-8;
    let sub = |a: P2, b: P2| add(a, mul(b, -1.0));
    let dot = |a: P2, b: P2| a[0] * b[0] + a[1] * b[1];
    let cross = |a: P2, b: P2| a[0] * b[1] - a[1] * b[0];
    let mut candidates = Vec::new();
    match (a, b) {
        (Segment::Line { a: p, b: q }, Segment::Line { a: r, b: s }) => {
            let d = sub(*q, *p);
            let e = sub(*s, *r);
            let det = cross(d, e);
            if det.abs() > epsilon * norm(d) * norm(e) {
                let t = cross(sub(*r, *p), e) / det;
                candidates.push(add(*p, mul(d, t)));
            } else {
                candidates.extend([*p, *q, *r, *s]);
            }
        }
        (Segment::Line { .. }, _) | (_, Segment::Line { .. }) => {
            let (line, circle) = if matches!(a, Segment::Line { .. }) {
                (a, b)
            } else {
                (b, a)
            };
            let Segment::Line { a: p, b: q } = line else {
                unreachable!()
            };
            let (center, radius) = match circle {
                Segment::Arc { center, radius, .. } | Segment::Circle { center, radius } => {
                    (*center, *radius)
                }
                _ => unreachable!(),
            };
            let d = sub(*q, *p);
            let length = norm(d);
            let unit = mul(d, 1.0 / length);
            let v = sub(center, *p);
            let projection = dot(v, unit);
            let distance = cross(v, unit).abs();
            if distance <= radius + epsilon {
                let half = ((radius - distance).max(0.0) * (radius + distance)).sqrt();
                for t in [projection - half, projection + half] {
                    candidates.push(add(*p, mul(unit, t)));
                }
            }
        }
        _ => {
            let circle = |s: &Segment| match s {
                Segment::Arc { center, radius, .. } | Segment::Circle { center, radius } => {
                    (*center, *radius)
                }
                _ => unreachable!(),
            };
            let (ca, ra) = circle(a);
            let (cb, rb) = circle(b);
            let d = norm(sub(cb, ca));
            if d <= epsilon && (ra - rb).abs() <= epsilon {
                candidates.extend([
                    a.start_point(),
                    a.end_point(),
                    b.start_point(),
                    b.end_point(),
                ]);
            } else if d > epsilon && d <= ra + rb + epsilon && d >= (ra - rb).abs() - epsilon {
                let x = (d + (ra - rb) * (ra + rb) / d) / 2.0;
                let h = ((ra - x).max(0.0) * (ra + x)).max(0.0).sqrt();
                let unit = mul(sub(cb, ca), 1.0 / d);
                let mid = add(ca, mul(unit, x));
                candidates.extend([add(mid, mul(left(unit), h)), add(mid, mul(left(unit), -h))]);
            }
        }
    }
    candidates
        .into_iter()
        .any(|p| a.distance(p) <= epsilon && b.distance(p) <= epsilon)
}
fn simple(ring: &ProfileLoop) -> bool {
    let n = ring.edges.len();
    (0..n).all(|i| {
        ((i + 1)..n)
            .filter(|j| *j != i + 1 && !(i == 0 && *j == n - 1))
            .all(|j| !intersects(&ring.edges[i].segment, &ring.edges[j].segment))
    })
}
