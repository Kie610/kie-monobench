//! 平面の小さな幾何: 向きの足し引き(kmb-sketch-solver の `vec2`)、点の角度、交点を求めるための
//! 形だけの曲線([`Curve`])と交点([`intersect`])。

use crate::curve::Shape;
use std::f64::consts::TAU;

pub(crate) use kmb_sketch_solver::vec2::{add, cross, dist, dot, len, perp, scale, sub};

pub(crate) const EPS: f64 = 1e-9;

pub(crate) fn angle_of(c: [f64; 2], p: [f64; 2]) -> f64 {
    (p[1] - c[1]).atan2(p[0] - c[0]).rem_euclid(TAU)
}

/// 形だけの曲線(交点を求めるため)。
#[derive(Debug, Clone)]
pub(crate) enum Curve {
    Seg([f64; 2], [f64; 2]),
    /// 中心、半径、始まりの角度、反時計回りの広がり(円なら TAU)
    Arc([f64; 2], f64, f64, f64),
    /// 楕円・スプライン・円錐曲線(交点は折れ線で探してから曲線の上へ寄せる)
    Free(Box<Shape>),
}

impl Curve {
    /// 点が曲線の範囲にあるか(点は曲線の延長上にあるとする)。
    pub(crate) fn contains(&self, p: [f64; 2]) -> bool {
        match *self {
            Curve::Seg(a, b) => {
                let d = sub(b, a);
                let t = dot(sub(p, a), d) / dot(d, d).max(EPS);
                (-1e-9..=1.0 + 1e-9).contains(&t)
            }
            Curve::Arc(c, _, a0, span) => {
                span >= TAU - EPS || (angle_of(c, p) - a0).rem_euclid(TAU) <= span + 1e-9
            }
            Curve::Free(_) => true,
        }
    }
}

/// 自由曲線 `s` と曲線 `other` の交点。`s` を細かい折れ線にして交わる所を探し、
/// ニュートン法で曲線の上へ寄せる。
fn free_intersect(s: &Shape, other: &Curve) -> Vec<[f64; 2]> {
    let tol = (1e-4 * s.size()).clamp(1e-5, 1e-3);
    let poly = s.polyline(tol);
    let other_poly: Option<Vec<(f64, [f64; 2])>> = match other {
        Curve::Free(o) => Some(o.polyline((1e-4 * o.size()).clamp(1e-5, 1e-3))),
        _ => None,
    };
    let mut out: Vec<[f64; 2]> = vec![];
    let (lo, hi) = s.domain();
    let inside = |sh: &Shape, t: f64| {
        let (lo, hi) = sh.domain();
        sh.closed() || (t >= lo - 1e-9 * (hi - lo) && t <= hi + 1e-9 * (hi - lo))
    };
    let mut found = |p: [f64; 2]| {
        if !out.iter().any(|q| len(sub(*q, p)) < 1e-7) {
            out.push(p);
        }
    };
    for w in poly.windows(2) {
        let ((t0, p0), (t1, p1)) = (w[0], w[1]);
        let chord = Curve::Seg(p0, p1);
        let at = |q: [f64; 2]| {
            let d = len(sub(p1, p0)).max(EPS);
            t0 + (t1 - t0) * (len(sub(q, p0)) / d).clamp(0.0, 1.0)
        };
        match (other, &other_poly) {
            (Curve::Free(o), Some(op)) => {
                for v in op.windows(2) {
                    for q in intersect(&chord, &Curve::Seg(v[0].1, v[1].1)) {
                        let d = len(sub(v[1].1, v[0].1)).max(EPS);
                        let u =
                            v[0].0 + (v[1].0 - v[0].0) * (len(sub(q, v[0].1)) / d).clamp(0.0, 1.0);
                        // 2 つの曲線のパラメータを同時に寄せる
                        let (mut t, mut u) = (at(q), u);
                        let mut ok = false;
                        for _ in 0..30 {
                            let ([a, da, _], [b, db, _]) = (s.ders(t), o.ders(u));
                            let f = sub(a, b);
                            if len(f) < 1e-11 {
                                ok = true;
                                break;
                            }
                            let det = cross(da, scale(db, -1.0));
                            if det.abs() < 1e-300 {
                                break;
                            }
                            // [da, -db] [dt, du]^T = -f
                            let dt = cross(scale(f, -1.0), scale(db, -1.0)) / det;
                            let du = cross(da, scale(f, -1.0)) / det;
                            t += dt;
                            u += du;
                        }
                        let p = s.at(t);
                        if (ok || len(sub(p, o.at(u))) < 1e-8) && inside(s, t) && inside(o, u) {
                            found(p);
                        }
                    }
                }
            }
            _ => {
                for q in intersect(&chord, other) {
                    let mut t = at(q);
                    let g = |t: f64| -> (f64, f64) {
                        let [c, d, _] = s.ders(t);
                        match *other {
                            Curve::Seg(a, b) => {
                                let e = sub(b, a);
                                let l = len(e).max(EPS);
                                (cross(e, sub(c, a)) / l, cross(e, d) / l)
                            }
                            Curve::Arc(cc, r, ..) => {
                                let v = sub(c, cc);
                                let l = len(v).max(EPS);
                                (l - r, dot(v, d) / l)
                            }
                            Curve::Free(_) => (0.0, 1.0),
                        }
                    };
                    let mut ok = false;
                    for _ in 0..30 {
                        let (f, df) = g(t);
                        if f.abs() < 1e-11 {
                            ok = true;
                            break;
                        }
                        if df.abs() < 1e-300 {
                            break;
                        }
                        t -= f / df;
                    }
                    let p = s.at(t);
                    if (ok || g(t).0.abs() < 1e-8)
                        && inside(s, t)
                        && other.contains(p)
                        && (s.closed() || (t >= lo - 1e-9 && t <= hi + 1e-9))
                    {
                        found(p);
                    }
                }
            }
        }
    }
    out
}

/// 2 つの曲線の交点。
pub(crate) fn intersect(a: &Curve, b: &Curve) -> Vec<[f64; 2]> {
    if let Curve::Free(s) = a {
        return free_intersect(s, b);
    }
    if let Curve::Free(s) = b {
        return free_intersect(s, a);
    }
    let raw = match (a.clone(), b.clone()) {
        (Curve::Seg(p, q), Curve::Seg(r, s)) => {
            let (d, e) = (sub(q, p), sub(s, r));
            let den = cross(d, e);
            if den.abs() < EPS {
                vec![]
            } else {
                let t = cross(sub(r, p), e) / den;
                vec![add(p, scale(d, t))]
            }
        }
        (Curve::Seg(p, q), Curve::Arc(c, r, ..)) | (Curve::Arc(c, r, ..), Curve::Seg(p, q)) => {
            let d = sub(q, p);
            let f = sub(p, c);
            let aa = dot(d, d);
            if aa < EPS {
                vec![]
            } else {
                // 直線への距離で接触を判定し、判別式の大きな項どうしの差を避ける。
                // 接点の許しは、端や範囲の判定と同じ EPS を使う。
                let t = -dot(f, d) / aa;
                let distance = cross(f, d).abs() / aa.sqrt();
                let gap = r - distance;
                if gap < -EPS {
                    vec![]
                } else if gap.abs() <= EPS {
                    vec![add(p, scale(d, t))]
                } else {
                    let dt = (gap * (r + distance) / aa).sqrt();
                    [t - dt, t + dt]
                        .into_iter()
                        .map(|t| add(p, scale(d, t)))
                        .collect()
                }
            }
        }
        (Curve::Arc(c1, r1, ..), Curve::Arc(c2, r2, ..)) => {
            let d = sub(c2, c1);
            let dl = len(d);
            if dl < EPS || dl > r1 + r2 || dl < (r1 - r2).abs() {
                vec![]
            } else {
                let x = (dl * dl + r1 * r1 - r2 * r2) / (2.0 * dl);
                let h = (r1 * r1 - x * x).max(0.0).sqrt();
                let u = scale(d, 1.0 / dl);
                let m = add(c1, scale(u, x));
                let n = [-u[1], u[0]];
                vec![add(m, scale(n, h)), sub(m, scale(n, h))]
            }
        }
        _ => vec![],
    };
    raw.into_iter()
        .filter(|p| a.contains(*p) && b.contains(*p))
        .collect()
}
