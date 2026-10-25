//! 2 つの形(点・線分・三角形の集まり)のいちばん近い点の組。

use super::*;

/// 線分 `s` の上で `p` にいちばん近い点。
pub(crate) fn closest_on_segment(p: P, s: [P; 2]) -> P {
    let d = sub(s[1], s[0]);
    let l2 = dot(d, d);
    if l2 < 1e-300 {
        return s[0];
    }
    let t = (dot(sub(p, s[0]), d) / l2).clamp(0.0, 1.0);
    add(s[0], scale(d, t))
}

/// 三角形の上で `p` にいちばん近い点(Ericson, Real-Time Collision Detection 5.1.5)。
pub(crate) fn closest_on_triangle(p: P, t: [P; 3]) -> P {
    let [a, b, c] = t;
    let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
    let (d1, d2) = (dot(ab, ap), dot(ac, ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = sub(p, b);
    let (d3, d4) = (dot(ab, bp), dot(ac, bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return add(a, scale(ab, d1 / (d1 - d3)));
    }
    let cp = sub(p, c);
    let (d5, d6) = (dot(ab, cp), dot(ac, cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return add(a, scale(ac, d2 / (d2 - d6)));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return add(b, scale(sub(c, b), (d4 - d3) / ((d4 - d3) + (d5 - d6))));
    }
    let den = 1.0 / (va + vb + vc);
    add(a, add(scale(ab, vb * den), scale(ac, vc * den)))
}

/// 2 本の線分のいちばん近い点の組(Ericson 5.1.9)。
pub(crate) fn closest_segments(s1: [P; 2], s2: [P; 2]) -> (P, P) {
    let (d1, d2) = (sub(s1[1], s1[0]), sub(s2[1], s2[0]));
    let r = sub(s1[0], s2[0]);
    let (a, e, f) = (dot(d1, d1), dot(d2, d2), dot(d2, r));
    let eps = 1e-300;
    let (s, t);
    if a <= eps && e <= eps {
        return (s1[0], s2[0]);
    }
    if a <= eps {
        s = 0.0;
        t = (f / e).clamp(0.0, 1.0);
    } else {
        let c = dot(d1, r);
        if e <= eps {
            t = 0.0;
            s = (-c / a).clamp(0.0, 1.0);
        } else {
            let b = dot(d1, d2);
            let den = a * e - b * b;
            let mut ss = if den > 1e-12 * a * e {
                ((b * f - c * e) / den).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let mut tt = (b * ss + f) / e;
            if tt < 0.0 {
                tt = 0.0;
                ss = (-c / a).clamp(0.0, 1.0);
            } else if tt > 1.0 {
                tt = 1.0;
                ss = ((b - c) / a).clamp(0.0, 1.0);
            }
            s = ss;
            t = tt;
        }
    }
    (add(s1[0], scale(d1, s)), add(s2[0], scale(d2, t)))
}

/// 線分が三角形を突き抜ける点。
pub(crate) fn segment_hits_triangle(s: [P; 2], t: [P; 3]) -> Option<P> {
    let d = sub(s[1], s[0]);
    let (e1, e2) = (sub(t[1], t[0]), sub(t[2], t[0]));
    let h = cross(d, e2);
    let a = dot(e1, h);
    if a.abs() < 1e-14 {
        return None;
    }
    let f = 1.0 / a;
    let q = sub(s[0], t[0]);
    let u = f * dot(q, h);
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let qe = cross(q, e1);
    let v = f * dot(d, qe);
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let k = f * dot(e2, qe);
    (0.0..=1.0).contains(&k).then(|| add(s[0], scale(d, k)))
}

/// 線分と三角形のいちばん近い点の組。
pub(crate) fn closest_segment_triangle(s: [P; 2], t: [P; 3]) -> (P, P) {
    if let Some(x) = segment_hits_triangle(s, t) {
        return (x, x);
    }
    let mut best = (f64::INFINITY, s[0], t[0]);
    let mut keep = |p: P, q: P| {
        let d = dist(p, q);
        if d < best.0 {
            best = (d, p, q);
        }
    };
    for p in s {
        keep(p, closest_on_triangle(p, t));
    }
    for i in 0..3 {
        let (p, q) = closest_segments(s, [t[i], t[(i + 1) % 3]]);
        keep(p, q);
    }
    (best.1, best.2)
}

/// 形のかけら(点・線分・三角形)。
#[derive(Clone, Copy)]
pub(crate) enum Prim {
    Pt(P),
    Seg([P; 2]),
    Tri([P; 3]),
}

impl Prim {
    fn bounds(&self) -> (P, P) {
        let pts: &[P] = match self {
            Prim::Pt(p) => std::slice::from_ref(p),
            Prim::Seg(s) => s,
            Prim::Tri(t) => t,
        };
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for p in pts {
            for i in 0..3 {
                lo[i] = lo[i].min(p[i]);
                hi[i] = hi[i].max(p[i]);
            }
        }
        (lo, hi)
    }
}

pub(crate) fn prims(g: &Geo) -> Vec<Prim> {
    g.points
        .iter()
        .map(|p| Prim::Pt(*p))
        .chain(g.segs.iter().map(|s| Prim::Seg(*s)))
        .chain(g.tris.iter().map(|t| Prim::Tri(*t)))
        .collect()
}

pub(crate) fn closest_prims(a: Prim, b: Prim) -> (P, P) {
    use Prim::*;
    match (a, b) {
        (Pt(p), Pt(q)) => (p, q),
        (Pt(p), Seg(s)) => (p, closest_on_segment(p, s)),
        (Seg(s), Pt(p)) => (closest_on_segment(p, s), p),
        (Pt(p), Tri(t)) => (p, closest_on_triangle(p, t)),
        (Tri(t), Pt(p)) => (closest_on_triangle(p, t), p),
        (Seg(s), Seg(r)) => closest_segments(s, r),
        (Seg(s), Tri(t)) => closest_segment_triangle(s, t),
        (Tri(t), Seg(s)) => {
            let (p, q) = closest_segment_triangle(s, t);
            (q, p)
        }
        (Tri(t), Tri(u)) => {
            // 交わっていれば、どちらかの辺がもう一方を突き抜ける
            let mut best = (f64::INFINITY, t[0], u[0]);
            for i in 0..3 {
                let (p, q) = closest_segment_triangle([t[i], t[(i + 1) % 3]], u);
                if dist(p, q) < best.0 {
                    best = (dist(p, q), p, q);
                }
                let (q, p) = closest_segment_triangle([u[i], u[(i + 1) % 3]], t);
                if dist(p, q) < best.0 {
                    best = (dist(p, q), p, q);
                }
            }
            (best.1, best.2)
        }
    }
}

/// 箱どうしの距離(中身の距離はこれより小さくならない)。
pub(crate) fn box_gap(a: (P, P), b: (P, P)) -> f64 {
    let mut s = 0.0;
    for i in 0..3 {
        let d = (b.0[i] - a.1[i]).max(a.0[i] - b.1[i]).max(0.0);
        s += d * d;
    }
    s.sqrt()
}

/// 2 つの形のいちばん近い点の組と距離。
pub fn closest(a: &Geo, b: &Geo) -> Option<(f64, P, P)> {
    let (pa, pb) = (prims(a), prims(b));
    let ba: Vec<(P, P)> = pa.iter().map(Prim::bounds).collect();
    let bb: Vec<(P, P)> = pb.iter().map(Prim::bounds).collect();
    let mut best: Option<(f64, P, P)> = None;
    for (x, bx) in pa.iter().zip(&ba) {
        for (y, by) in pb.iter().zip(&bb) {
            if best.is_some_and(|b| box_gap(*bx, *by) >= b.0) {
                continue;
            }
            let (p, q) = closest_prims(*x, *y);
            let d = dist(p, q);
            if best.is_none_or(|b| d < b.0) {
                best = Some((d, p, q));
            }
        }
    }
    best
}
