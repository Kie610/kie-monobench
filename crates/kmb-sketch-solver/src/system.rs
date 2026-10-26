//! 拘束の式と、その数値の解き方。スケッチ(kmb-sketch)が拘束を残差の式([`Residual`])に変えて
//! [`System`] を作り、ここで最小ノルムのガウス・ニュートン法で解く。
//!
//! 最小ノルムの更新を使うので、拘束が足りない要素は、保存されている位置から
//! なるべく動かない。自由度はヤコビ行列の階数から求める。

use crate::curve::Shape;
use crate::sparse::{Factor, Row};
use crate::{ConstraintId, EntityId};
use nalgebra::{DMatrix, DVector};
use std::collections::BTreeMap;
use std::sync::Arc;

pub const MAX_ITERATIONS: usize = 200;
pub const TOLERANCE: f64 = 1e-10;
/// 拘束を満たしたとみなす残差の上限
pub const RESIDUAL_OK: f64 = 1e-7;

/// 変数の位置。点は (x, y) の 2 つ、円は半径の 1 つ。
#[derive(Clone, Copy)]
pub struct Pt(pub usize);

impl Pt {
    pub fn get(self, x: &[f64]) -> (f64, f64) {
        (x[self.0], x[self.0 + 1])
    }
}

#[derive(Clone, Copy)]
pub enum Radius {
    /// 円の半径の変数
    Var(usize),
    /// 円弧の半径(中心から始点までの距離)
    Arc(Pt, Pt),
}

impl Radius {
    pub fn get(self, x: &[f64]) -> f64 {
        match self {
            Radius::Var(i) => x[i],
            Radius::Arc(c, s) => dist(c.get(x), s.get(x)),
        }
    }
}

/// 曲線の形を、変数の位置で表したもの(楕円・スプライン・円錐曲線の拘束に使う)。
#[derive(Clone)]
pub enum CurveDef {
    Line(Pt, Pt),
    Circle(Pt, Radius),
    Arc(Pt, Pt, Pt),
    Ellipse(Pt, Pt, Pt, Option<(Pt, Pt)>),
    Spline {
        pts: Vec<Pt>,
        control: bool,
        closed: bool,
        degree: u8,
        knots: Option<Vec<f64>>,
    },
    Conic(Pt, Pt, Pt, f64),
}

impl CurveDef {
    pub fn shape(&self, x: &[f64]) -> Option<Shape> {
        let g = |p: &Pt| {
            let (a, b) = p.get(x);
            [a, b]
        };
        match self {
            CurveDef::Line(a, b) => Some(Shape::Line(g(a), g(b))),
            CurveDef::Circle(c, r) => Shape::circle(g(c), r.get(x)),
            CurveDef::Arc(c, s, e) => Shape::arc(g(c), g(s), g(e)),
            CurveDef::Ellipse(c, ma, mi, ends) => {
                Shape::ellipse(g(c), g(ma), g(mi), ends.map(|(s, e)| (g(&s), g(&e))))
            }
            CurveDef::Spline {
                pts,
                control,
                closed,
                degree,
                knots,
            } => {
                let p: Vec<[f64; 2]> = pts.iter().map(g).collect();
                Shape::spline(&p, *control, *closed, *degree, knots.as_deref())
            }
            CurveDef::Conic(s, e, a, rho) => Shape::conic(g(s), g(e), g(a), *rho),
        }
    }
}

/// 曲線の上の位置: 始点、終点、または補助の変数(パラメータ)。
#[derive(Clone, Copy, PartialEq)]
pub enum At {
    Start,
    End,
    Var(usize),
}

impl At {
    pub fn param(self, shape: &Shape, x: &[f64]) -> f64 {
        match self {
            At::Start => shape.domain().0,
            At::End => shape.domain().1,
            At::Var(i) => x[i],
        }
    }
}

/// 形にならない(長さ 0 の軸など)時の残差。0 でない値にして、満たしたとみなさない
pub const NO_SHAPE: f64 = 1.0;

/// 2 つの曲線の、それぞれの位置での単位の接線(外積, 内積)。向きがそろうと外積が 0。
pub fn tangent_pair(
    a: &Arc<CurveDef>,
    at_a: At,
    b: &Arc<CurveDef>,
    at_b: At,
    x: &[f64],
    memo: &mut Memo,
) -> Option<(f64, f64)> {
    let (sa, sb) = (memo.shape(a, x)?, memo.shape(b, x)?);
    let (ta, tb) = (
        sa.tangent(at_a.param(&sa, x)),
        sb.tangent(at_b.param(&sb, x)),
    );
    Some((ta[0] * tb[1] - ta[1] * tb[0], ta[0] * tb[0] + ta[1] * tb[1]))
}

/// 楕円の上にあるかの式: 楕円の座標で ((u/a)² + (v/b)²)^(1/2) - 1 に √(ab) を掛けたもの。
pub fn on_ellipse(c: Pt, major: Pt, minor: Pt, p: Pt, x: &[f64]) -> f64 {
    let (cc, ma, mi, pp) = (c.get(x), major.get(x), minor.get(x), p.get(x));
    let m = (ma.0 - cc.0, ma.1 - cc.1);
    let a = safe(m.0.hypot(m.1));
    let b = safe(dist(mi, cc));
    let d = (pp.0 - cc.0, pp.1 - cc.1);
    let u = (d.0 * m.0 + d.1 * m.1) / a;
    let v = (m.0 * d.1 - m.1 * d.0) / a;
    ((u / a).hypot(v / b) - 1.0) * (a * b).sqrt()
}

/// 楕円弧の離心角の開き(0 以上 2π 未満)。
pub fn ellipse_sweep(c: Pt, major: Pt, minor: Pt, s: Pt, e: Pt, x: &[f64]) -> Option<f64> {
    let g = |p: Pt| {
        let (a, b) = p.get(x);
        [a, b]
    };
    let (u, v) = crate::curve::ellipse_axes(g(c), g(major), g(minor))?;
    let t0 = crate::curve::ellipse_angle(g(c), u, v, g(s));
    let t1 = crate::curve::ellipse_angle(g(c), u, v, g(e));
    Some((t1 - t0).rem_euclid(std::f64::consts::TAU))
}

/// 残差の式。0 になれば拘束を満たす。
#[derive(Clone)]
pub enum Residual {
    /// x[i] - x[j]
    Diff(usize, usize),
    /// x[i] - v
    Value(usize, f64),
    /// |p - q| - v
    Dist(Pt, Pt, f64),
    /// s * (x[i] - x[j]) - v
    SignedDiff(usize, usize, f64, f64),
    /// 方向の外積 / 長さの積(平行で 0)
    Cross(Pt, Pt, Pt, Pt),
    /// 方向の内積 / 長さの積(垂直で 0)
    Dot(Pt, Pt, Pt, Pt),
    /// 線分 a-b と線分 c-d の長さの差
    EqualLen(Pt, Pt, Pt, Pt),
    EqualRadius(Radius, Radius),
    /// 点から直線への符号つき距離
    OnLine(Pt, Pt, Pt),
    /// |p - c| - r
    OnCircle(Pt, Pt, Radius),
    /// s * (直線から中心への符号つき距離) - r
    Tangent(Pt, Pt, Pt, Radius, f64),
    /// 中点の x(true)または y(false)
    Mid(Pt, Pt, Pt, bool),
    /// 長さ a-b - v
    Len(Pt, Pt, f64),
    /// k * r - v
    Rad(Radius, f64, f64),
    /// 線分 a-b から c-d への向きつきの角度が v(-π〜π に折り返した差。v + π の解を持たない)
    Angle(Pt, Pt, Pt, Pt, f64),
    /// 円弧の終点が始点と同じ半径にある
    ArcEnd(Pt, Pt, Pt),
    /// 点 a と点 b の中点から、直線 p-q への符号つき距離
    MidOnLine(Pt, Pt, Pt, Pt),
    /// s * (点から直線 a-b への符号つき距離) - v
    SideDistance(Pt, Pt, Pt, f64, f64),
    /// s * (点から直線 a-b への符号つき距離) - x[d](d はオフセットの距離の変数)
    SideDistanceVar(Pt, Pt, Pt, f64, usize),
    /// 半径 r - (相手の半径 + k * x[d])。相手が無ければ r - k * x[d](d はオフセットの距離の変数)
    RadiusGap(Radius, Option<Radius>, f64, usize),
    /// 写しの点 p と、元の点 q を中心(点か、決まった座標)のまわりに (cos, sin) だけ回した点の
    /// x(true)または y(false)の差
    Turned(Pt, Pt, Option<Pt>, [f64; 2], f64, f64, bool),
    /// 写しの点 p と、元の点 q を 2 つの向きにそれぞれの長さだけずらした点の x(true)または
    /// y(false)の差。向きは線分(単位にする)か、決まった単位ベクトル
    Shifted(Pt, Pt, [Shift; 2], bool),
    /// 円どうしが接する: |c1 - c2| - (k1 * r1 + k2 * r2)。外は k1 = k2 = 1、内は符号が逆
    RoundTangent(Pt, Radius, Pt, Radius, f64, f64),
    /// 円周からの距離: s * d + k1 * r1 + k2 * r2 - v。d は中心 c から点 q まで(`Some(q2)` なら
    /// 直線 q-q2 までの符号つき)の距離。r2 は相手も円の時だけ
    Gap(
        Pt,
        Pt,
        Option<Pt>,
        f64,
        Radius,
        f64,
        Option<(Radius, f64)>,
        f64,
    ),
    /// 点 p が楕円(中心、長軸の端、短軸の端)の上にある
    OnEllipse(Pt, Pt, Pt, Pt),
    /// 点 p と、曲線のパラメータ t(変数)の点の x(true)または y(false)の差
    OnCurve(Arc<CurveDef>, usize, Pt, bool),
    /// 2 つの曲線の、それぞれのパラメータ(変数)の点の x(true)または y(false)の差
    Meet(Arc<CurveDef>, usize, Arc<CurveDef>, usize, bool),
    /// 2 つの曲線の、それぞれの位置での単位の接線の外積(向きがそろうと 0)
    Turn(Arc<CurveDef>, At, Arc<CurveDef>, At),
    /// 端を共有する 2 つの曲線の曲率の差 k * (sa * κa - sb * κb)。符号 sa・sb で、つなぎ目を
    /// a から b へ通る向きにそろえる
    Bend(Arc<CurveDef>, At, f64, Arc<CurveDef>, At, f64, f64),
}

/// パターンのずらす向き 1 つ(線分の向きか決まった単位ベクトル)と長さ。
#[derive(Clone, Copy)]
pub struct Shift {
    pub line: Option<(Pt, Pt)>,
    pub unit: [f64; 2],
    pub amount: f64,
}

impl Shift {
    pub fn get(&self, x: &[f64]) -> (f64, f64) {
        let (ux, uy) = match self.line {
            Some((a, b)) => {
                let d = dir(a, b, x);
                let l = safe(d.0.hypot(d.1));
                (d.0 / l, d.1 / l)
            }
            None => (self.unit[0], self.unit[1]),
        };
        (ux * self.amount, uy * self.amount)
    }
}

pub fn dist(p: (f64, f64), q: (f64, f64)) -> f64 {
    (p.0 - q.0).hypot(p.1 - q.1)
}

pub fn dir(a: Pt, b: Pt, x: &[f64]) -> (f64, f64) {
    let (p, q) = (a.get(x), b.get(x));
    (q.0 - p.0, q.1 - p.1)
}

/// 長さ 0 の線分で割らないための下限
pub fn safe(v: f64) -> f64 {
    if v.abs() < 1e-12 { 1e-12 } else { v }
}

/// 同じ位置 `x` で何度も使う曲線の形(式ごとに作り直さない)。曲線は [`Arc`] の指す先で見分ける。
#[derive(Default)]
pub struct Memo(Vec<(*const CurveDef, Option<Shape>)>);

impl Memo {
    pub fn shape(&mut self, def: &Arc<CurveDef>, x: &[f64]) -> Option<Shape> {
        let key = Arc::as_ptr(def);
        if let Some((_, s)) = self.0.iter().find(|(k, _)| *k == key) {
            return s.clone();
        }
        let s = def.shape(x);
        self.0.push((key, s.clone()));
        s
    }
}

pub fn pt_vars(p: Pt, out: &mut Vec<usize>) {
    out.extend([p.0, p.0 + 1]);
}

pub fn radius_vars(r: Radius, out: &mut Vec<usize>) {
    match r {
        Radius::Var(i) => out.push(i),
        Radius::Arc(c, s) => {
            pt_vars(c, out);
            pt_vars(s, out);
        }
    }
}

impl CurveDef {
    /// 形が使う変数。
    pub fn vars(&self, out: &mut Vec<usize>) {
        match self {
            CurveDef::Line(a, b) => [*a, *b].into_iter().for_each(|p| pt_vars(p, out)),
            CurveDef::Circle(c, r) => {
                pt_vars(*c, out);
                radius_vars(*r, out);
            }
            CurveDef::Arc(c, s, e) | CurveDef::Conic(c, s, e, _) => {
                [*c, *s, *e].into_iter().for_each(|p| pt_vars(p, out))
            }
            CurveDef::Ellipse(c, ma, mi, ends) => {
                [*c, *ma, *mi].into_iter().for_each(|p| pt_vars(p, out));
                if let Some((s, e)) = ends {
                    pt_vars(*s, out);
                    pt_vars(*e, out);
                }
            }
            CurveDef::Spline { pts, .. } => pts.iter().for_each(|p| pt_vars(*p, out)),
        }
    }
}

impl At {
    pub fn vars(self, out: &mut Vec<usize>) {
        if let At::Var(i) = self {
            out.push(i);
        }
    }
}

impl Residual {
    /// 式が使う変数(ヤコビ行列で、この変数を動かした時に計り直す式を選ぶのに使う)。
    pub fn vars(&self, out: &mut Vec<usize>) {
        let pts = |ps: &[Pt], out: &mut Vec<usize>| ps.iter().for_each(|p| pt_vars(*p, out));
        match self {
            Residual::Diff(i, j) | Residual::SignedDiff(i, j, ..) => out.extend([*i, *j]),
            Residual::Value(i, _) => out.push(*i),
            Residual::Dist(p, q, _) | Residual::Len(p, q, _) => pts(&[*p, *q], out),
            Residual::Cross(a, b, c, d)
            | Residual::Dot(a, b, c, d)
            | Residual::EqualLen(a, b, c, d)
            | Residual::Angle(a, b, c, d, _)
            | Residual::MidOnLine(a, b, c, d)
            | Residual::OnEllipse(a, b, c, d) => pts(&[*a, *b, *c, *d], out),
            Residual::EqualRadius(r, s) => {
                radius_vars(*r, out);
                radius_vars(*s, out);
            }
            Residual::OnLine(p, a, b)
            | Residual::Mid(p, a, b, _)
            | Residual::ArcEnd(p, a, b)
            | Residual::SideDistance(p, a, b, ..) => pts(&[*p, *a, *b], out),
            Residual::SideDistanceVar(p, a, b, _, d) => {
                pts(&[*p, *a, *b], out);
                out.push(*d);
            }
            Residual::RadiusGap(r, other, _, d) => {
                radius_vars(*r, out);
                if let Some(o) = other {
                    radius_vars(*o, out);
                }
                out.push(*d);
            }
            Residual::Turned(p, q, c, ..) => {
                pts(&[*p, *q], out);
                if let Some(c) = c {
                    pt_vars(*c, out);
                }
            }
            Residual::Shifted(p, q, dirs, _) => {
                pts(&[*p, *q], out);
                for d in dirs {
                    if let Some((a, b)) = d.line {
                        pts(&[a, b], out);
                    }
                }
            }
            Residual::OnCircle(p, c, r) => {
                pts(&[*p, *c], out);
                radius_vars(*r, out);
            }
            Residual::Tangent(a, b, c, r, _) => {
                pts(&[*a, *b, *c], out);
                radius_vars(*r, out);
            }
            Residual::Rad(r, ..) => radius_vars(*r, out),
            Residual::RoundTangent(c1, r1, c2, r2, ..) => {
                pts(&[*c1, *c2], out);
                radius_vars(*r1, out);
                radius_vars(*r2, out);
            }
            Residual::Gap(c, q, q2, _, r1, _, r2, _) => {
                pts(&[*c, *q], out);
                if let Some(q2) = q2 {
                    pt_vars(*q2, out);
                }
                radius_vars(*r1, out);
                if let Some((r2, _)) = r2 {
                    radius_vars(*r2, out);
                }
            }
            Residual::OnCurve(def, t, p, _) => {
                def.vars(out);
                out.push(*t);
                pt_vars(*p, out);
            }
            Residual::Meet(a, ta, b, tb, _) => {
                a.vars(out);
                b.vars(out);
                out.extend([*ta, *tb]);
            }
            Residual::Turn(a, at_a, b, at_b) | Residual::Bend(a, at_a, _, b, at_b, ..) => {
                a.vars(out);
                b.vars(out);
                at_a.vars(out);
                at_b.vars(out);
            }
        }
    }

    pub fn eval(&self, x: &[f64]) -> f64 {
        self.eval_with(x, &mut Memo::default())
    }

    /// `memo` は同じ `x` で作った曲線の形(式の間で使い回す)。
    pub fn eval_with(&self, x: &[f64], memo: &mut Memo) -> f64 {
        match *self {
            Residual::Diff(i, j) => x[i] - x[j],
            Residual::Value(i, v) => x[i] - v,
            Residual::Dist(p, q, v) => dist(p.get(x), q.get(x)) - v,
            Residual::SignedDiff(i, j, s, v) => s * (x[i] - x[j]) - v,
            Residual::Cross(a, b, c, d) => {
                let (u, w) = (dir(a, b, x), dir(c, d, x));
                (u.0 * w.1 - u.1 * w.0) / safe(u.0.hypot(u.1) * w.0.hypot(w.1))
            }
            Residual::Dot(a, b, c, d) => {
                let (u, w) = (dir(a, b, x), dir(c, d, x));
                (u.0 * w.0 + u.1 * w.1) / safe(u.0.hypot(u.1) * w.0.hypot(w.1))
            }
            Residual::EqualLen(a, b, c, d) => dist(a.get(x), b.get(x)) - dist(c.get(x), d.get(x)),
            Residual::EqualRadius(r, s) => r.get(x) - s.get(x),
            Residual::OnLine(p, a, b) => signed_distance(p, a, b, x),
            Residual::OnCircle(p, c, r) => dist(p.get(x), c.get(x)) - r.get(x),
            Residual::Tangent(a, b, c, r, s) => s * signed_distance(c, a, b, x) - r.get(x),
            Residual::Mid(p, a, b, is_x) => {
                let (pp, pa, pb) = (p.get(x), a.get(x), b.get(x));
                if is_x {
                    pp.0 - (pa.0 + pb.0) / 2.0
                } else {
                    pp.1 - (pa.1 + pb.1) / 2.0
                }
            }
            Residual::Len(a, b, v) => dist(a.get(x), b.get(x)) - v,
            Residual::Rad(r, k, v) => k * r.get(x) - v,
            Residual::Angle(a, b, c, d, v) => wrap_pi(turn(a, b, c, d, x) - v),
            Residual::ArcEnd(c, s, e) => dist(e.get(x), c.get(x)) - dist(s.get(x), c.get(x)),
            Residual::SideDistance(p, a, b, s, v) => s * signed_distance(p, a, b, x) - v,
            Residual::SideDistanceVar(p, a, b, s, d) => s * signed_distance(p, a, b, x) - x[d],
            Residual::RadiusGap(r, other, k, d) => {
                r.get(x) - other.map_or(0.0, |o| o.get(x)) - k * x[d]
            }
            Residual::Turned(p, q, c, at, co, si, is_x) => {
                let c = c.map_or((at[0], at[1]), |c| c.get(x));
                let (qq, pp) = (q.get(x), p.get(x));
                let (dx, dy) = (qq.0 - c.0, qq.1 - c.1);
                if is_x {
                    pp.0 - (c.0 + co * dx - si * dy)
                } else {
                    pp.1 - (c.1 + si * dx + co * dy)
                }
            }
            Residual::Shifted(p, q, dirs, is_x) => {
                let (qq, pp) = (q.get(x), p.get(x));
                let (s0, s1) = (dirs[0].get(x), dirs[1].get(x));
                if is_x {
                    pp.0 - (qq.0 + s0.0 + s1.0)
                } else {
                    pp.1 - (qq.1 + s0.1 + s1.1)
                }
            }
            Residual::RoundTangent(c1, r1, c2, r2, k1, k2) => {
                dist(c1.get(x), c2.get(x)) - (k1 * r1.get(x) + k2 * r2.get(x))
            }
            Residual::Gap(c, q, q2, sd, r1, k1, r2, v) => {
                let d = match q2 {
                    Some(q2) => signed_distance(c, q, q2, x),
                    None => dist(c.get(x), q.get(x)),
                };
                let extra = r2.map_or(0.0, |(r, k)| k * r.get(x));
                sd * d + k1 * r1.get(x) + extra - v
            }
            Residual::MidOnLine(a, b, p, q) => {
                let ((ax, ay), (bx, by)) = (a.get(x), b.get(x));
                let m = ((ax + bx) / 2.0, (ay + by) / 2.0);
                let pp = p.get(x);
                let u = dir(p, q, x);
                (u.0 * (m.1 - pp.1) - u.1 * (m.0 - pp.0)) / safe(u.0.hypot(u.1))
            }
            Residual::OnEllipse(c, ma, mi, p) => on_ellipse(c, ma, mi, p, x),
            Residual::OnCurve(ref def, t, p, is_x) => match memo.shape(def, x) {
                Some(sh) => {
                    let q = sh.at(x[t]);
                    let pp = p.get(x);
                    if is_x { pp.0 - q[0] } else { pp.1 - q[1] }
                }
                None => NO_SHAPE,
            },
            Residual::Meet(ref a, ta, ref b, tb, is_x) => {
                match (memo.shape(a, x), memo.shape(b, x)) {
                    (Some(sa), Some(sb)) => {
                        let (p, q) = (sa.at(x[ta]), sb.at(x[tb]));
                        if is_x { p[0] - q[0] } else { p[1] - q[1] }
                    }
                    _ => NO_SHAPE,
                }
            }
            Residual::Turn(ref a, at_a, ref b, at_b) => {
                tangent_pair(a, at_a, b, at_b, x, memo).map_or(NO_SHAPE, |t| t.0)
            }
            Residual::Bend(ref a, at_a, sa, ref b, at_b, sb, k) => {
                match (memo.shape(a, x), memo.shape(b, x)) {
                    (Some(ca), Some(cb)) => {
                        let ka = ca.curvature(at_a.param(&ca, x));
                        let kb = cb.curvature(at_b.param(&cb, x));
                        k * (sa * ka - sb * kb)
                    }
                    _ => NO_SHAPE,
                }
            }
        }
    }
}

/// 線分 a-b の向きから c-d の向きへの角度(-π〜π、反時計回りが正)
pub fn turn(a: Pt, b: Pt, c: Pt, d: Pt, x: &[f64]) -> f64 {
    let (u, w) = (dir(a, b, x), dir(c, d, x));
    (u.0 * w.1 - u.1 * w.0).atan2(u.0 * w.0 + u.1 * w.1)
}

/// 角度を -π〜π に折り返す
pub fn wrap_pi(a: f64) -> f64 {
    use std::f64::consts::{PI, TAU};
    (a + PI).rem_euclid(TAU) - PI
}

pub fn signed_distance(p: Pt, a: Pt, b: Pt, x: &[f64]) -> f64 {
    let (pa, pp) = (a.get(x), p.get(x));
    let u = dir(a, b, x);
    (u.0 * (pp.1 - pa.1) - u.1 * (pp.0 - pa.0)) / safe(u.0.hypot(u.1))
}

/// 円周から点・直線までの距離の式の符号(中心からの距離 d0、半径 r0 で選ぶ)。
/// 遠い側は d + r、近い側は外なら d - r、中なら r - d。
pub fn gap_signs(far: bool, d0: f64, r0: f64) -> (f64, f64) {
    if far {
        (1.0, 1.0)
    } else if d0 >= r0 {
        (1.0, -1.0)
    } else {
        (-1.0, 1.0)
    }
}

pub fn sign_of(v: f64) -> f64 {
    if v < 0.0 { -1.0 } else { 1.0 }
}

pub struct System {
    pub x0: Vec<f64>,
    /// (由来の拘束。None は円弧の内部の式, 残差)
    pub equations: Vec<(Option<ConstraintId>, Residual)>,
    pub points: BTreeMap<EntityId, Pt>,
    pub circles: BTreeMap<EntityId, usize>,
    /// 要素ごとの変数の位置(完全拘束の判定に使う)
    pub entity_vars: BTreeMap<EntityId, Vec<usize>>,
    /// 解の合否の判定に使う形の性質(拘束には足さない)
    pub traits: Vec<Trait>,
    /// 円と円弧の半径(ドラッグで、引いていない円の大きさを保つのに使う)
    pub rounds: Vec<Radius>,
    /// まとまりの拘束(オフセット)の関係の式: (式の番号, 拘束, 関係の番号)
    pub part_rows: Vec<(usize, ConstraintId, usize)>,
}

/// 形の性質。拘束の式は満たしたまま、形が裏返ったり交差したりした解を見分ける。
/// 拘束の式には足さず、少しずつ解く時の 1 歩ごとの合否にだけ使う(方式案 3.2)。
#[derive(Clone)]
pub enum Trait {
    /// 点 p が直線 a-b のどちら側にあるか(接する線と円の中心、対称の点)
    Side(Pt, Pt, Pt),
    /// 向き a-b と c-d の内積の符号(平行が逆向きにならない。端を共有して接する円の中心の側)
    DotSign(Pt, Pt, Pt, Pt),
    /// 向き a-b から c-d への外積の符号(直角の向き)
    CrossSign(Pt, Pt, Pt, Pt),
    /// 円弧の開き(中心、始点、終点。始点から反時計回り)が 1 歩で大きく跳ばない
    Sweep(Pt, Pt, Pt),
    /// 線分の向きが 1 歩で裏返らない
    Direction(Pt, Pt),
    /// 円の半径が正のまま
    Positive(usize),
    /// 2 つの曲線の、それぞれの位置での接線の内積の符号(接する所で折り返さない)
    TangentDot(Arc<CurveDef>, At, Arc<CurveDef>, At),
    /// 楕円弧(中心、長軸の端、短軸の端、始点、終点)の開きが 1 歩で大きく跳ばない
    EllipseSweep(Pt, Pt, Pt, Pt, Pt),
}

/// 円弧の開き(0 以上 2π 未満)。
pub fn sweep(c: Pt, s: Pt, e: Pt, x: &[f64]) -> f64 {
    let (u, w) = (dir(c, s, x), dir(c, e, x));
    (w.1.atan2(w.0) - u.1.atan2(u.0)).rem_euclid(std::f64::consts::TAU)
}

impl Trait {
    /// 符号を持つ性質の値(正規化した量)。開きと向きは `None`。
    pub fn signed(&self, x: &[f64]) -> Option<f64> {
        let norm = |a: Pt, b: Pt, c: Pt, d: Pt| {
            let (u, w) = (dir(a, b, x), dir(c, d, x));
            (u, w, safe(u.0.hypot(u.1) * w.0.hypot(w.1)))
        };
        Some(match *self {
            Trait::Side(p, a, b) => signed_distance(p, a, b, x),
            Trait::DotSign(a, b, c, d) => {
                let (u, w, n) = norm(a, b, c, d);
                (u.0 * w.0 + u.1 * w.1) / n
            }
            Trait::CrossSign(a, b, c, d) => {
                let (u, w, n) = norm(a, b, c, d);
                (u.0 * w.1 - u.1 * w.0) / n
            }
            Trait::Positive(i) => x[i],
            Trait::TangentDot(ref a, at_a, ref b, at_b) => {
                tangent_pair(a, at_a, b, at_b, x, &mut Memo::default())?.1
            }
            Trait::Sweep(..) | Trait::Direction(..) | Trait::EllipseSweep(..) => return None,
        })
    }

    /// 前の形 `a` から次の形 `b` へ、この性質が保たれたか。`step` はこの 1 歩で引いた量(mm)。
    pub fn kept(&self, a: &[f64], b: &[f64], step: f64) -> bool {
        /// これより小さい値の符号は決まっていないとみなす
        const CLEAR: f64 = 1e-6;
        match *self {
            Trait::EllipseSweep(c, ma, mi, s, e) => match (
                ellipse_sweep(c, ma, mi, s, e, a),
                ellipse_sweep(c, ma, mi, s, e, b),
            ) {
                (Some(u), Some(w)) => (u - w).abs() <= std::f64::consts::FRAC_PI_2,
                _ => false,
            },
            Trait::Sweep(c, s, e) => {
                // 開きが 1 歩で 90° より大きく変わるのは、端が中心の向こうへ跳んだとき
                (sweep(c, s, e, b) - sweep(c, s, e, a)).abs() <= std::f64::consts::FRAC_PI_2
            }
            Trait::Direction(p, q) => {
                let (u, w) = (dir(p, q, a), dir(p, q, b));
                // 引いた量より短い線分は、端が向こう側へ抜けることがある(自由な線の端を引く時)
                let short = 4.0 * step + 1e-9;
                u.0.hypot(u.1) <= short || w.0.hypot(w.1) <= short || u.0 * w.0 + u.1 * w.1 > 0.0
            }
            _ => match (self.signed(a), self.signed(b)) {
                (Some(va), Some(vb)) if va.abs() > CLEAR => va * vb > 0.0,
                _ => true,
            },
        }
    }
}

/// 形の性質がすべて保たれたか。
pub fn shape_kept(traits: &[Trait], a: &[f64], b: &[f64], step: f64) -> bool {
    traits.iter().all(|t| t.kept(a, b, step))
}

pub fn residuals(eqs: &[(Option<ConstraintId>, Residual)], x: &[f64]) -> DVector<f64> {
    let mut memo = Memo::default();
    DVector::from_iterator(
        eqs.len(),
        eqs.iter().map(|(_, r)| r.eval_with(x, &mut memo)),
    )
}

/// 中心差分によるヤコビ行列。変数ごとに、その変数を使う式だけを計り直す(ほかの式の微分は 0)。
pub fn jacobian(eqs: &[(Option<ConstraintId>, Residual)], x: &[f64]) -> DMatrix<f64> {
    let all: Vec<usize> = (0..x.len()).collect();
    jacobian_of(eqs, x, &all)
}

/// 変数 `cols` の列だけのヤコビ行列(列の順は `cols` の順)。
pub fn jacobian_of(
    eqs: &[(Option<ConstraintId>, Residual)],
    x: &[f64],
    cols: &[usize],
) -> DMatrix<f64> {
    to_dense(&sparse_jacobian_of(eqs, x, cols), cols.len())
}

/// 疎な行を密な行列にする。
pub fn to_dense(rows: &[Row], n: usize) -> DMatrix<f64> {
    let mut j = DMatrix::zeros(rows.len(), n);
    for (i, row) in rows.iter().enumerate() {
        for &(c, v) in row {
            j[(i, c)] = v;
        }
    }
    j
}

/// [`jacobian`] を疎な行で。
pub fn sparse_jacobian(eqs: &[(Option<ConstraintId>, Residual)], x: &[f64]) -> Vec<Row> {
    let all: Vec<usize> = (0..x.len()).collect();
    sparse_jacobian_of(eqs, x, &all)
}

/// [`jacobian_of`] を疎な行で(行ごとに、0 でない値を列の順に持つ)。
pub fn sparse_jacobian_of(
    eqs: &[(Option<ConstraintId>, Residual)],
    x: &[f64],
    cols: &[usize],
) -> Vec<Row> {
    let mut j: Vec<Row> = vec![vec![]; eqs.len()];
    let mut users: Vec<Vec<usize>> = vec![vec![]; x.len()];
    let mut vs = vec![];
    for (i, (_, r)) in eqs.iter().enumerate() {
        vs.clear();
        r.vars(&mut vs);
        vs.sort_unstable();
        vs.dedup();
        for &v in &vs {
            if v < x.len() {
                users[v].push(i);
            }
        }
    }
    let mut xp = x.to_vec();
    for (out, &col) in cols.iter().enumerate() {
        if users[col].is_empty() {
            continue;
        }
        let h = 1e-7 * x[col].abs().max(1.0);
        let at = |xp: &[f64]| {
            let mut memo = Memo::default();
            users[col]
                .iter()
                .map(|&i| eqs[i].1.eval_with(xp, &mut memo))
                .collect::<Vec<f64>>()
        };
        xp[col] = x[col] + h;
        let fp = at(&xp);
        xp[col] = x[col] - h;
        let fm = at(&xp);
        xp[col] = x[col];
        for (k, &i) in users[col].iter().enumerate() {
            let d = (fp[k] - fm[k]) / (2.0 * h);
            if d != 0.0 {
                j[i].push((out, d));
            }
        }
    }
    #[cfg(any(test, feature = "check"))]
    if x.len() <= 40 {
        // 式が使う変数の書き漏らしがないか、すべての式を計り直した行列と比べる
        let dense = dense_jacobian(eqs, x).select_columns(cols.iter());
        let got = to_dense(&j, cols.len());
        assert!(
            (&dense - &got).amax() == 0.0,
            "ヤコビ行列の変数の書き漏らし: {}",
            (&dense - &got).amax()
        );
    }
    j
}

#[cfg(any(test, feature = "check"))]
pub fn dense_jacobian(eqs: &[(Option<ConstraintId>, Residual)], x: &[f64]) -> DMatrix<f64> {
    let mut j = DMatrix::zeros(eqs.len(), x.len());
    let mut xp = x.to_vec();
    for col in 0..x.len() {
        let h = 1e-7 * x[col].abs().max(1.0);
        xp[col] = x[col] + h;
        let fp = residuals(eqs, &xp);
        xp[col] = x[col] - h;
        let fm = residuals(eqs, &xp);
        xp[col] = x[col];
        j.set_column(col, &((fp - fm) / (2.0 * h)));
    }
    j
}

pub fn rank(m: &DMatrix<f64>) -> usize {
    if m.nrows() == 0 || m.ncols() == 0 {
        return 0;
    }
    let sv = m.clone().svd(false, false).singular_values;
    let tol = rank_tol(&sv);
    sv.iter().filter(|&&s| s > tol).count()
}

/// [`rank`] で 0 と見なす特異値の大きさ。
fn rank_tol(sv: &DVector<f64>) -> f64 {
    1e-8 * sv.max().max(1.0)
}

pub fn max_abs(v: &DVector<f64>) -> f64 {
    v.iter().fold(0.0_f64, |m, x| m.max(x.abs()))
}

/// 持たせた要素の変数の重み(小さいほど動きにくい。動かす量の 2 乗に 1 / 重み² がかかる)
pub const HELD_WEIGHT: f64 = 1e-3;
/// ドラッグで始めの位置へ寄せる動きを止める大きさ(mm)と、それまでの反復の上限
pub const PULL_TOLERANCE: f64 = 1e-7;
pub const PULL_ITERATIONS: usize = 10;
/// 少しずつ解く時の刻みの下限(寸法の変化・引いた量の何分の 1 まで刻むか)
pub const MIN_STEP: f64 = 1.0 / 256.0;

/// 最小ノルムのガウス・ニュートン法。`shift` があれば、残差からそれを引いた式を 0 にする。
/// `weight` があれば、重みの小さい変数ほど動かさない。(解, 残差, 反復の回数)を返す。
pub fn gauss_newton(
    eqs: &[(Option<ConstraintId>, Residual)],
    mut x: Vec<f64>,
    shift: Option<&DVector<f64>>,
    near: Option<&Near<'_>>,
) -> (Vec<f64>, DVector<f64>, usize) {
    let eval = |x: &[f64]| {
        let f = residuals(eqs, x);
        match shift {
            Some(s) => f - s,
            None => f,
        }
    };
    let n = x.len();
    let w: Vec<f64> = near.map_or_else(|| vec![1.0; n], |g| g.weight.to_vec());
    // 重み 0 の変数は動かさないので、行列から外して解く(ドラッグで、動かす範囲だけを解く)
    let cols: Vec<usize> = (0..n).filter(|&v| w[v] > 0.0).collect();
    let reference = x.clone();
    let mut f = eval(&x);
    let mut iterations = 0;
    let pulling = near.is_some();
    while iterations < MAX_ITERATIONS && !eqs.is_empty() && !cols.is_empty() {
        if !pulling && max_abs(&f) <= TOLERANCE {
            break;
        }
        // 重みで変数を測り直す(y = x / w)。重みの小さい変数ほど動かない
        let mut j = sparse_jacobian_of(eqs, &x, &cols);
        for row in &mut j {
            for (k, d) in row.iter_mut() {
                *d *= w[cols[*k]];
            }
        }
        let step = match near {
            // 最小ノルムの更新: 拘束が足りない方向には動かさない(疎な QR 分解で解く)
            None => {
                let rhs: Vec<f64> = f.iter().map(|v| -v).collect();
                DVector::from_vec(Factor::new(&j, cols.len(), Some(&rhs)).min_norm())
            }
            // ドラッグ: 拘束を満たす向きに加えて、拘束の式を変えない向き(零空間)の中で
            // 始めの形にいちばん近い所へ寄せる
            Some(g) => match g.step(&j, &f, &x, &reference, &w, &cols) {
                Some(d) => d,
                None => break,
            },
        };
        // 測り直した変数の歩を、もとの変数の歩に戻す
        let mut full = DVector::zeros(n);
        for (k, &v) in cols.iter().enumerate() {
            full[v] = step[k] * w[v];
        }
        let step = full;
        if max_abs(&f) <= TOLERANCE
            && (!pulling || step.amax() <= PULL_TOLERANCE || iterations >= PULL_ITERATIONS)
        {
            break;
        }
        iterations += 1;
        let norm = f.norm();
        let mut alpha = 1.0;
        let mut improved = false;
        while alpha > 1e-4 {
            let trial: Vec<f64> = x
                .iter()
                .zip(step.iter())
                .map(|(a, d)| a + alpha * d)
                .collect();
            let ft = eval(&trial);
            if ft.norm() < norm || ft.norm() <= TOLERANCE {
                x = trial;
                f = ft;
                improved = true;
                break;
            }
            alpha /= 2.0;
        }
        if !improved {
            break;
        }
    }
    (x, f, iterations)
}

/// ドラッグで、どの解を採るかの基準。始めの形からの(重みで測った)点の動きと、
/// 押した時の半径からの円・円弧の大きさの変化の 2 乗和がいちばん小さい解。
pub struct Near<'a> {
    pub weight: &'a [f64],
    pub rounds: &'a [Radius],
    /// 押した時の半径
    pub radii: Vec<f64>,
}

/// 半径の変化を、点の動きの何倍に数えるか(2 乗の重み)
pub const KEEP_RADIUS: f64 = 10.0;

impl Near<'_> {
    /// 測り直した変数での 1 歩。拘束を満たす最小ノルムの歩 d0 に、零空間の中で
    /// 基準の残差 g(点の動きと半径の変化)を最小にする歩を足す(ガウス・ニュートン)。
    /// `j` は変数 `cols` の列だけの疎な行列で、歩も `cols` の順に返す(ほかの変数は動かさない)。
    ///
    /// 零空間の基底は作らない(変数の数の 2 乗の大きさになる)。基準の行列は単位行列に
    /// 半径の行 U(k 行)を足したものなので、零空間への写し P⊥ と U だけで解ける:
    /// 足す歩 e は零空間の中で ‖e + b1‖² + ‖U e + b2‖² を最小にし(b = g + G d0)、
    /// V = P⊥ Uᵀ、t = U e として (I + VᵀV) t = -Vᵀ b1 - VᵀV b2、e = -P⊥ b1 - V (t + b2)。
    /// P⊥ は疎な分解の行の空間への写しで求める(写しは k + 1 回)。
    pub fn step(
        &self,
        j: &[Row],
        f: &DVector<f64>,
        x: &[f64],
        reference: &[f64],
        w: &[f64],
        cols: &[usize],
    ) -> Option<DVector<f64>> {
        let n = cols.len();
        // 変数の位置 → 列(動かさない変数は None)
        let mut col_of = vec![None; x.len()];
        for (k, &v) in cols.iter().enumerate() {
            col_of[v] = Some(k);
        }
        let rhs: Vec<f64> = f.iter().map(|v| -v).collect();
        let mut a = Factor::new(j, n, Some(&rhs));
        let d0 = DVector::from_vec(a.min_norm());
        if a.rank() == n {
            return Some(d0);
        }
        // 零空間への写し(2 回外して丸めの誤差を抑える)
        let mut perp = |v: DVector<f64>| -> DVector<f64> {
            let v = &v - DVector::from_vec(a.project(v.as_slice()));
            &v - DVector::from_vec(a.project(v.as_slice()))
        };
        // 基準の残差とその微分(測り直した変数 y で)。動かす変数に関わる円・円弧の半径だけを数える
        let rounds: Vec<(Radius, f64)> = self
            .rounds
            .iter()
            .zip(&self.radii)
            .filter(|(r, _)| match **r {
                Radius::Var(v) => col_of[v].is_some(),
                Radius::Arc(c, p) => [c.0, c.0 + 1, p.0, p.0 + 1]
                    .iter()
                    .any(|&v| col_of[v].is_some()),
            })
            .map(|(r, r0)| (*r, *r0))
            .collect();
        let k = rounds.len();
        // 点の動きの行は単位行列(残差 g1)、半径の行は U(残差 g2)
        let mut g1 = DVector::zeros(n);
        for (c, &v) in cols.iter().enumerate() {
            g1[c] = (x[v] - reference[v]) / w[v];
        }
        let s = KEEP_RADIUS.sqrt();
        let mut g2 = DVector::zeros(k);
        let mut u = DMatrix::zeros(k, n);
        for (i, (r, r0)) in rounds.iter().enumerate() {
            g2[i] = s * (r.get(x) - r0);
            let mut add = |v: usize, d: f64| {
                if let Some(c) = col_of[v] {
                    u[(i, c)] += d * w[v];
                }
            };
            match *r {
                Radius::Var(v) => add(v, s),
                Radius::Arc(c, p) => {
                    let (cc, pp) = (c.get(x), p.get(x));
                    let l = safe(dist(cc, pp));
                    let (ux, uy) = ((pp.0 - cc.0) / l, (pp.1 - cc.1) / l);
                    add(p.0, s * ux);
                    add(p.0 + 1, s * uy);
                    add(c.0, -s * ux);
                    add(c.0 + 1, -s * uy);
                }
            }
        }
        let b1 = g1 + &d0;
        let b2 = g2 + &u * &d0;
        let pb1 = perp(b1.clone());
        if k == 0 {
            return Some(d0 - pb1);
        }
        let mut v = DMatrix::zeros(n, k);
        for i in 0..k {
            v.set_column(i, &perp(u.row(i).transpose()));
        }
        let vtv = v.transpose() * &v;
        let m = DMatrix::identity(k, k) + &vtv;
        let t = m.cholesky()?.solve(&(-(v.transpose() * &b1) - &vtv * &b2));
        Some(d0 - pb1 - v * (t + b2))
    }
}

/// 寸法の値などで解く。まず一度に解き、形の性質が変わったら少しずつ解き直す。
pub fn solve_values(sys: &System) -> (Vec<f64>, usize) {
    let eqs = &sys.equations;
    let x0 = &sys.x0;
    let (x1, f1, mut iterations) = gauss_newton(eqs, x0.clone(), None, None);
    if max_abs(&f1) <= RESIDUAL_OK && shape_kept(&sys.traits, x0, &x1, 0.0) {
        return (x1, iterations);
    }
    // 保存されている位置の残差 f0 を、(1 - t) f0 として t = 0 → 1 で 0 へ近づける
    let f0 = residuals(eqs, x0);
    let (mut x, mut t, mut h) = (x0.clone(), 0.0_f64, 0.25_f64);
    while t < 1.0 {
        let tn = (t + h).min(1.0);
        let shift = &f0 * (1.0 - tn);
        let (xn, fnext, k) = gauss_newton(eqs, x.clone(), Some(&shift), None);
        iterations += k;
        if max_abs(&fnext) <= RESIDUAL_OK && shape_kept(&sys.traits, &x, &xn, 0.0) {
            x = xn;
            t = tn;
            h = (h * 2.0).min(0.5);
        } else {
            h /= 2.0;
            if h < MIN_STEP {
                // 形を保って解けないので、一度に解いた形を使う
                return (x1, iterations);
            }
        }
    }
    (x, iterations)
}

/// ドラッグ: `from`(前のフレームの解)から `target` へ、引いた変数を少しずつ動かして解く。
pub fn solve_moving(
    sys: &System,
    from: &[f64],
    target: &[f64],
    weight: &[f64],
) -> (Vec<f64>, usize) {
    // 動かす変数を 1 つも使わない式は解いても変わらないので外す(つながらない所に解けない
    // 拘束が残っていても、ドラッグを止めない)
    let mut vs = vec![];
    let eqs: Vec<_> = sys
        .equations
        .iter()
        .filter(|(_, r)| {
            vs.clear();
            r.vars(&mut vs);
            vs.iter().any(|&v| weight.get(v).is_some_and(|w| *w > 0.0))
        })
        .cloned()
        .collect();
    let eqs = &eqs;
    let near = Near {
        weight,
        rounds: &sys.rounds,
        radii: sys.rounds.iter().map(|r| r.get(&sys.x0)).collect(),
    };
    let moved: Vec<usize> = (0..from.len())
        .filter(|&v| weight[v] == HELD_WEIGHT && (target[v] - from[v]).abs() > 1e-12)
        .collect();
    // 引いた点の動く量(mm)。円の半径も同じに数える
    let reach = moved
        .iter()
        .map(|&v| (target[v] - from[v]).abs())
        .fold(0.0, f64::max);
    let (mut x, mut t, mut h, mut iterations) = (from.to_vec(), 0.0_f64, 1.0_f64, 0);
    while t < 1.0 {
        let tn = (t + h).min(1.0);
        let mut start = x.clone();
        for &v in &moved {
            start[v] = from[v] + tn * (target[v] - from[v]);
        }
        let (xn, fnext, k) = gauss_newton(eqs, start, None, Some(&near));
        iterations += k;
        if max_abs(&fnext) <= RESIDUAL_OK && shape_kept(&sys.traits, &x, &xn, (tn - t) * reach) {
            x = xn;
            t = tn;
            h = (h * 2.0).min(1.0);
        } else {
            h /= 2.0;
            if h < MIN_STEP {
                // ここから先は形が崩れる(または解けない)ので、止めた形を返す
                break;
            }
        }
    }
    (x, iterations)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 前の密な SVD による引く時の 1 歩(零空間の基底を作って解く)。疎な [`Near::step`] と比べる。
    fn dense_step(
        near: &Near<'_>,
        j: &DMatrix<f64>,
        f: &DVector<f64>,
        x: &[f64],
        reference: &[f64],
        w: &[f64],
        cols: &[usize],
    ) -> DVector<f64> {
        let n = cols.len();
        let mut col_of = vec![None; x.len()];
        for (k, &v) in cols.iter().enumerate() {
            col_of[v] = Some(k);
        }
        let rows = j.nrows().max(n);
        let mut full = DMatrix::zeros(rows, n);
        full.view_mut((0, 0), (j.nrows(), n)).copy_from(j);
        let svd = full.svd(true, true);
        let v_t = svd.v_t.as_ref().expect("v_t");
        let tol = 1e-8 * svd.singular_values.max().max(1.0);
        let mut fz = DVector::zeros(rows);
        fz.rows_mut(0, f.len()).copy_from(&(-f));
        let d0 = svd.solve(&fz, 1e-12).expect("解ける");
        let null: Vec<usize> = (0..n).filter(|&k| svd.singular_values[k] <= tol).collect();
        let basis = DMatrix::from_fn(n, null.len(), |r, c| v_t[(null[c], r)]);
        // (半径はどれも動かす変数に関わるものを渡す)
        let k = near.rounds.len();
        let mut g = DVector::zeros(n + k);
        let mut gj = DMatrix::zeros(n + k, n);
        for (c, &v) in cols.iter().enumerate() {
            g[c] = (x[v] - reference[v]) / w[v];
            gj[(c, c)] = 1.0;
        }
        let s = KEEP_RADIUS.sqrt();
        let mut add = |row: usize, v: usize, d: f64| {
            if let Some(c) = col_of[v] {
                gj[(row, c)] += d * w[v];
            }
        };
        for (i, (r, r0)) in near.rounds.iter().zip(&near.radii).enumerate() {
            g[n + i] = s * (r.get(x) - r0);
            match *r {
                Radius::Var(v) => add(n + i, v, s),
                Radius::Arc(c, p) => {
                    let (cc, pp) = (c.get(x), p.get(x));
                    let l = safe(dist(cc, pp));
                    let (ux, uy) = ((pp.0 - cc.0) / l, (pp.1 - cc.1) / l);
                    add(n + i, p.0, s * ux);
                    add(n + i, p.0 + 1, s * uy);
                    add(n + i, c.0, -s * ux);
                    add(n + i, c.0 + 1, -s * uy);
                }
            }
        }
        let a = &gj * &basis;
        let b = -(g + &gj * &d0);
        let z = a.svd(true, true).solve(&b, 1e-12).expect("解ける");
        d0 + basis * z
    }

    /// 引く時の 1 歩が、零空間の基底を密な SVD で作って解く前のやり方と同じになる
    /// (余分な行・式に出ない変数・円の半径・円弧の半径・動かさない変数・重みを含む)。
    #[test]
    fn drag_step_agrees_with_dense_null_space() {
        // 変数 0..8: 点 (0,1) (2,3) (4,5)、円の半径 6、式に出ない 7
        let n = 8;
        let j = DMatrix::from_row_slice(
            4,
            n,
            &[
                1.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, //
                0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 0.5, 0.0, //
                2.0, 0.0, -2.0, 0.0, 0.0, 0.0, 0.0, 0.0, //
                0.0, 0.0, 0.3, 0.0, -0.7, 0.2, 0.0, 0.0,
            ],
        );
        let x = [0.1, 0.2, 1.3, 0.4, 2.5, 1.1, 0.9, 3.0];
        let reference = [0.0, 0.25, 1.0, 0.5, 2.4, 1.0, 1.0, 3.0];
        let f = DVector::from_vec(vec![0.01, -0.02, 0.02, 0.005]);
        let mut w = vec![1.0; n];
        w[2] = HELD_WEIGHT;
        w[3] = HELD_WEIGHT;
        let rounds = [Radius::Var(6), Radius::Arc(Pt(0), Pt(4))];
        let near = Near {
            weight: &w,
            rounds: &rounds,
            radii: vec![1.0, 2.0],
        };
        for cols in [(0..n).collect::<Vec<_>>(), vec![0, 1, 2, 3, 6]] {
            let jc = j.select_columns(cols.iter());
            let mut jw = jc.clone();
            for (c, &v) in cols.iter().enumerate() {
                jw.column_mut(c).scale_mut(w[v]);
            }
            let sparse_rows: Vec<Row> = (0..jw.nrows())
                .map(|i| {
                    (0..jw.ncols())
                        .filter(|&c| jw[(i, c)] != 0.0)
                        .map(|c| (c, jw[(i, c)]))
                        .collect()
                })
                .collect();
            let got = near
                .step(&sparse_rows, &f, &x, &reference, &w, &cols)
                .expect("解ける");
            let want = dense_step(&near, &jw, &f, &x, &reference, &w, &cols);
            assert!(
                (&got - &want).amax() < 1e-10,
                "列 {cols:?}: {got} と {want}"
            );
        }
    }
}
