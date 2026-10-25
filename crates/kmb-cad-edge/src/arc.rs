//! 円の辺(平面とそれに垂直な円筒の境目)の丸め・面取りと、辺の端の切り方。
//!
//! 円の辺は、まっすぐな辺と同じ断面([`cutter`])を、円筒の軸まわりに回して柱にする。
//!
//! 辺の端で、同じ平面の上の別の辺と出会い、その平面の輪郭が内側へ折れる(へこんだ角)か、
//! 接して続く所では、柱を辺の先へ延ばしてから、2 本の辺から等しい距離の所で切る。
//! 延ばしたままだと、隣の材料(前方後円墳なら円柱の上面)に溝を掘るため。
//! 丸めの高さは辺(側面)からの距離だけで決まるので、等しい距離の所で切れば 2 つの丸めが
//! 段なくつながる。まっすぐな辺どうしなら 2 本の向きを二等分する面、円の辺が入ると曲がった
//! 面(円と直線なら放物線)になるので、両方の辺を内へずらした線の交点を折れ線にして切る。
//! まっすぐな辺どうしのへこんだ角では、切り口を二等分の面そのものに置き、2 本の柱を同じ大きさに
//! する(辺ごとの大きさのずらしをそろえる)。切り口で 2 本の断面が重なり、つなぎ目に面が残らない。
//! 柱を二等分の面の先へ残すと、その分が隣の丸めの外まで削り、残した幅の細い面が角ごとにできる
//! (それを横切る押し出しで SMLib が立体を失った)。
//! 接する所と円の辺が入る角は、切り口をわずか(`OVERLAP`)越えて重ね、切り口が同じ面に並ばない
//! ようにする(SMLib は、接する所で同じ断面の柱が切り口でちょうど接すると、削る量を落とした)。

use super::{EXTEND, Section, Shape, cutter};
use kmb_core::kernel::{self as k, P3, Plane, Profile, ProfileLoop, Region};
use kmb_core::{BooleanOp, EdgeInfo, FaceInfo, Kernel, KernelError, ShapeHandle};

/// 接する所と円の辺が入る角で、切り口を越えて柱を残す量(mm)
const OVERLAP: f64 = 0.01;
/// 端点が同じとみなす距離(mm)
const SAME_POINT: f64 = 1e-5;

/// 辺の端での切り方。`p` を通り、`m` の側(`m` との内積が正)を残す。
/// `curve` が空でなければ、平らな面の上で 2 本の辺から等しい距離の折れ線(辺の外から中へ)で
/// 切り、その `-m` の側を除く。空なら `p` を通り `m` に垂直な面で切る。
#[derive(Debug, Clone)]
pub(crate) struct Clip {
    p: P3,
    m: P3,
    /// 切る面に含まれる、平面の面の法線
    n: P3,
    curve: Vec<P3>,
    /// 切り口を越えて柱を残す量
    overlap: f64,
    /// 切り口で断面を重ねる相手の辺(`clips` に渡した辺すべての中の番号)。まっすぐな辺どうしの
    /// へこんだ角だけ。相手と同じ大きさにしないと、断面の差の幅の面が切り口に残る
    pub(crate) with: Option<usize>,
}

/// 平らな面の上での辺の線(側面の跡)。`t` だけ面の中へずらした線を返す。
#[derive(Debug, Clone, Copy)]
enum Trace {
    Line { q: P3, d: P3, c: P3 },
    Circle { center: P3, radius: f64, grow: f64 },
}

impl Trace {
    fn of(faces: &[FaceInfo], e: &EdgeInfo, face: &str, p: P3) -> Option<Trace> {
        if let Some(arc) = e.arc {
            return Some(Trace::Circle {
                center: arc.center,
                radius: arc.radius,
                grow: if arc.inside { -1.0 } else { 1.0 },
            });
        }
        Some(Trace::Line {
            q: e.a,
            d: unit(k::sub(e.b, e.a)),
            c: into_face(faces, e, face, p)?,
        })
    }
}

/// 平面の上の 2D の座標(原点 `o`、軸 `u`・`v`)。
struct Frame {
    o: P3,
    u: P3,
    v: P3,
}

impl Frame {
    fn to2(&self, p: P3) -> [f64; 2] {
        let d = k::sub(p, self.o);
        [k::dot(d, self.u), k::dot(d, self.v)]
    }
    fn to3(&self, q: [f64; 2]) -> P3 {
        k::add(
            self.o,
            k::add(k::scale(self.u, q[0]), k::scale(self.v, q[1])),
        )
    }
}

/// 2D の直線(点と向き)か円。
enum Curve2 {
    Line([f64; 2], [f64; 2]),
    Circle([f64; 2], f64),
}

fn offset(fr: &Frame, tr: &Trace, t: f64) -> Option<Curve2> {
    Some(match *tr {
        Trace::Line { q, d, c } => {
            let q = k::add(q, k::scale(c, t));
            let a = fr.to2(q);
            let b = fr.to2(k::add(q, d));
            Curve2::Line(a, [b[0] - a[0], b[1] - a[1]])
        }
        Trace::Circle {
            center,
            radius,
            grow,
        } => {
            let r = radius + grow * t;
            if r <= 1e-6 {
                return None;
            }
            Curve2::Circle(fr.to2(center), r)
        }
    })
}

/// 2 本の線の交点のうち、`near` にいちばん近いもの。
fn meet(a: &Curve2, b: &Curve2, near: [f64; 2]) -> Option<[f64; 2]> {
    let dist = |p: &[f64; 2]| (p[0] - near[0]).hypot(p[1] - near[1]);
    let line_circle = |p: [f64; 2], d: [f64; 2], c: [f64; 2], r: f64| -> Vec<[f64; 2]> {
        let l = d[0].hypot(d[1]);
        let d = [d[0] / l, d[1] / l];
        let w = [p[0] - c[0], p[1] - c[1]];
        let bq = w[0] * d[0] + w[1] * d[1];
        let cq = w[0] * w[0] + w[1] * w[1] - r * r;
        let disc = bq * bq - cq;
        if disc < 0.0 {
            return vec![];
        }
        let s = disc.sqrt();
        [-bq - s, -bq + s]
            .iter()
            .map(|t| [p[0] + d[0] * t, p[1] + d[1] * t])
            .collect()
    };
    let pts: Vec<[f64; 2]> = match (a, b) {
        (Curve2::Line(p, d), Curve2::Line(q, e)) => {
            let den = d[0] * e[1] - d[1] * e[0];
            if den.abs() < 1e-12 {
                return None;
            }
            let w = [q[0] - p[0], q[1] - p[1]];
            let s = (w[0] * e[1] - w[1] * e[0]) / den;
            vec![[p[0] + d[0] * s, p[1] + d[1] * s]]
        }
        (Curve2::Line(p, d), Curve2::Circle(c, r)) | (Curve2::Circle(c, r), Curve2::Line(p, d)) => {
            line_circle(*p, *d, *c, *r)
        }
        (Curve2::Circle(c0, r0), Curve2::Circle(c1, r1)) => {
            let dx = [c1[0] - c0[0], c1[1] - c0[1]];
            let dd = dx[0].hypot(dx[1]);
            if dd < 1e-12 || dd > r0 + r1 || dd < (r0 - r1).abs() {
                return None;
            }
            let a = (r0 * r0 - r1 * r1 + dd * dd) / (2.0 * dd);
            let h = (r0 * r0 - a * a).max(0.0).sqrt();
            let m = [c0[0] + dx[0] * a / dd, c0[1] + dx[1] * a / dd];
            vec![
                [m[0] - dx[1] * h / dd, m[1] + dx[0] * h / dd],
                [m[0] + dx[1] * h / dd, m[1] - dx[0] * h / dd],
            ]
        }
    };
    pts.into_iter().min_by(|p, q| dist(p).total_cmp(&dist(q)))
}

/// 辺の端 `p` から、2 本の辺の線を `t` ずつ面の中へずらした線の交点をたどった折れ線
/// (`t` は `-out`〜`depth`、辺の外から中へ)。途中で交わらなくなったらそこまで。
fn equidistant(fr: &Frame, a: &Trace, b: &Trace, out: f64, depth: f64) -> Vec<P3> {
    const STEPS: usize = 31;
    let walk = |end: f64| -> Vec<[f64; 2]> {
        let mut prev = [0.0, 0.0];
        let mut pts = Vec::new();
        for i in 1..=STEPS {
            let t = end * i as f64 / STEPS as f64;
            let (Some(ca), Some(cb)) = (offset(fr, a, t), offset(fr, b, t)) else {
                break;
            };
            let Some(q) = meet(&ca, &cb, prev) else { break };
            // 前の点から跳んだら(別の交点に移った)そこまで
            if (q[0] - prev[0]).hypot(q[1] - prev[1]) > 4.0 * end.abs() / STEPS as f64 + 1.0 {
                break;
            }
            pts.push(q);
            prev = q;
        }
        pts
    };
    let mut outside = walk(-out);
    outside.reverse();
    outside
        .into_iter()
        .chain(std::iter::once([0.0, 0.0]))
        .chain(walk(depth))
        .map(|q| fr.to3(q))
        .collect()
}

fn unit(v: P3) -> P3 {
    let l = k::dot(v, v).sqrt();
    k::scale(v, 1.0 / l)
}

fn near(p: P3, q: P3) -> bool {
    let d = k::sub(p, q);
    k::dot(d, d).sqrt() < SAME_POINT
}

fn planar_normal(faces: &[FaceInfo], name: &str) -> Option<P3> {
    faces
        .iter()
        .find(|f| f.name == name)
        .and_then(|f| f.planar)
        .map(|p| p.normal)
}

/// 辺 `e` の端 `p` から、平面の面 `face` の中へ向かう向き(面の上で辺に直角)。
fn into_face(faces: &[FaceInfo], e: &EdgeInfo, face: &str, p: P3) -> Option<P3> {
    if let Some(arc) = e.arc {
        if e.faces[0] != face {
            return None;
        }
        let r = k::sub(p, arc.center);
        let r = k::sub(r, k::scale(arc.axis, k::dot(r, arc.axis)));
        let r = unit(r);
        return Some(if arc.inside { k::scale(r, -1.0) } else { r });
    }
    let other = if e.faces[0] == face {
        &e.faces[1]
    } else if e.faces[1] == face {
        &e.faces[0]
    } else {
        return None;
    };
    let n = planar_normal(faces, face)?;
    let no = planar_normal(faces, other)?;
    let c = unit(k::cross(n, unit(k::sub(e.b, e.a))));
    // 出っ張った辺では相手の面の法線と逆向き、へこんだ辺では同じ向きが面の中
    let toward = k::dot(c, no) > 0.0;
    Some(if toward != e.convex {
        c
    } else {
        k::scale(c, -1.0)
    })
}

/// 辺 `e` の両端(`a` と `b`)それぞれの切り方。`all` は形の辺すべて(まっすぐな辺と円の辺)。
/// 切らない端(出っ張った角や、隣の辺が無い所)は `None`。
pub(crate) fn clips(
    faces: &[FaceInfo],
    all: &[EdgeInfo],
    e: &EdgeInfo,
    size: f64,
) -> [Option<Clip>; 2] {
    let closed = e.arc.is_some() && near(e.a, e.b);
    if closed {
        return [None, None];
    }
    [false, true].map(|at_b| {
        let p = if at_b { e.b } else { e.a };
        let te = e.tangent_into(at_b);
        for (with, g) in all.iter().enumerate() {
            if g == e || g.convex != e.convex {
                continue;
            }
            let g_at_b = if near(g.a, p) {
                false
            } else if near(g.b, p) {
                true
            } else {
                continue;
            };
            // 同じ平面の面の上で出会う辺だけ
            let Some(face) = e
                .faces
                .iter()
                .find(|f| g.faces.contains(f) && planar_normal(faces, f).is_some())
            else {
                continue;
            };
            let (Some(n), Some(inside)) =
                (planar_normal(faces, face), into_face(faces, e, face, p))
            else {
                continue;
            };
            let tg = g.tangent_into(g_at_b);
            let tangent = k::dot(te, tg) < -1.0 + 1e-6;
            let reflex = k::dot(inside, tg) < -1e-9;
            if !(tangent || reflex) {
                continue;
            }
            let m = if tangent { te } else { unit(k::sub(te, tg)) };
            // 円の辺が入る角は、2 本の辺から等しい距離の曲がった線で切る
            let curve = if !tangent && (e.arc.is_some() || g.arc.is_some()) {
                let fr = Frame {
                    o: p,
                    u: te,
                    v: k::cross(n, te),
                };
                match (Trace::of(faces, e, face, p), Trace::of(faces, g, face, p)) {
                    (Some(a), Some(b)) => equidistant(&fr, &a, &b, 1.0, 1.5 * size + 1.0),
                    _ => Vec::new(),
                }
            } else {
                Vec::new()
            };
            let exact = !tangent && curve.is_empty();
            return Some(Clip {
                p,
                m,
                n,
                curve,
                overlap: if exact { 0.0 } else { OVERLAP },
                with: exact.then_some(with),
            });
        }
        None
    })
}

/// 切る端で、柱を辺の先へ延ばす長さ(切るので、二等分の面を十分越える長さ)。
pub(crate) fn clip_extension(size: f64) -> f64 {
    2.0 * size + EXTEND
}

/// 柱 `tool` を `clip` の面で切る(`m` の逆側を、端のまわり `reach` の範囲だけ除く)。
pub(crate) fn apply_clip(
    kernel: &dyn Kernel,
    tool: ShapeHandle,
    clip: &Clip,
    size: f64,
    tag: &str,
) -> Result<ShapeHandle, KernelError> {
    // 端のまわりだけを除く箱(遠くの柱を切らない大きさ)
    let half = 3.0 * size + 2.0;
    let m = clip.m;
    if clip.curve.len() >= 2 {
        return apply_curve_clip(kernel, tool, clip, half, tag);
    }
    let origin = k::sub(clip.p, k::scale(m, clip.overlap));
    // 法線(x × y)が -m を向く面から -m の向きへ押し出す
    let x = clip.n;
    let y = k::cross(k::scale(m, -1.0), x);
    let plane = Plane {
        origin,
        x_axis: x,
        y_axis: y,
    };
    let corners = [[-half, -half], [half, -half], [half, half], [-half, half]];
    let lp = ProfileLoop {
        edges: (0..4)
            .map(|i| super::line(&format!("clip{i}"), corners[i], corners[(i + 1) % 4]))
            .collect(),
    };
    let profile = Profile {
        plane,
        regions: vec![Region {
            outer: lp,
            holes: vec![],
        }],
    };
    let cut = kernel.extrude(&profile, 0.0, 2.0 * half, tag)?;
    kernel.boolean(BooleanOp::Subtract, tool, cut)
}

/// 等しい距離の折れ線 `clip.curve` の `-m` の側(端のまわり `half` の範囲)を、柱から除く。
fn apply_curve_clip(
    kernel: &dyn Kernel,
    tool: ShapeHandle,
    clip: &Clip,
    half: f64,
    tag: &str,
) -> Result<ShapeHandle, KernelError> {
    let n = clip.n;
    let u = unit(k::cross(clip.m, n));
    let fr = Frame {
        o: k::sub(clip.p, k::scale(n, half)),
        u,
        v: k::cross(n, u),
    };
    let back = k::scale(clip.m, -1.0);
    // 折れ線を切り口の分だけ `-m` へずらし、その先を `half` 先まで囲む
    let mut pts: Vec<[f64; 2]> = clip
        .curve
        .iter()
        .map(|q| fr.to2(k::add(*q, k::scale(back, clip.overlap))))
        .collect();
    let last = *clip.curve.last().expect("2 点以上");
    let first = clip.curve[0];
    pts.push(fr.to2(k::add(last, k::scale(back, half))));
    pts.push(fr.to2(k::add(first, k::scale(back, half))));
    // 左回りにそろえる
    let area: f64 = (0..pts.len())
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            a[0] * b[1] - a[1] * b[0]
        })
        .sum();
    if area < 0.0 {
        pts.reverse();
    }
    let lp = ProfileLoop {
        edges: (0..pts.len())
            .map(|i| super::line(&format!("clip{i}"), pts[i], pts[(i + 1) % pts.len()]))
            .collect(),
    };
    let profile = Profile {
        plane: Plane {
            origin: fr.o,
            x_axis: fr.u,
            y_axis: fr.v,
        },
        regions: vec![Region {
            outer: lp,
            holes: vec![],
        }],
    };
    let cut = kernel.extrude(&profile, 0.0, 2.0 * half, tag)?;
    kernel.boolean(BooleanOp::Subtract, tool, cut)
}

/// 円の辺の柱。断面を円の上の点に置き、軸まわりに辺の長さと両端の延長(`ext`)ぶん回す。
///
/// 切る端(`clips`)の延長は別の柱にして、それだけを切ってから本体に足す。
/// 円は曲がって戻るので、端の切る面の向こうに同じ円の反対の端が入ることがあり、
/// 柱全体を切ると反対の端の丸めまで消すため(幅の狭い前方後円墳で起きた)。
#[allow(clippy::too_many_arguments)]
pub(crate) fn arc_tool(
    kernel: &dyn Kernel,
    e: &EdgeInfo,
    shape: Shape,
    offset: f64,
    name: &str,
    ext: [f64; 2],
    clips: &[Option<Clip>; 2],
    size: f64,
    tag: &str,
) -> Result<ShapeHandle, KernelError> {
    let arc = e.arc.expect("円の辺");
    let tau = std::f64::consts::TAU;
    // `a` から辺の向きへ `from` だけ進んだ所から、`angle` 回した柱
    let span = |from: f64, angle: f64| span(kernel, e, shape, offset, name, from, angle, tag);
    if near(e.a, e.b) {
        return span(0.0, tau);
    }
    let sweep = arc.sweep.abs();
    let back = if clips[0].is_some() {
        0.0
    } else {
        ext[0] / arc.radius
    };
    let fwd = if clips[1].is_some() {
        0.0
    } else {
        ext[1] / arc.radius
    };
    let mut tool = span(-back, (sweep + back + fwd).min(tau - 1e-3))?;
    for (j, clip) in clips.iter().enumerate() {
        let Some(clip) = clip else { continue };
        let angle = (ext[j] / arc.radius).min((tau - sweep) / 2.0 - 1e-3);
        let from = if j == 0 { -angle } else { sweep };
        let piece = span(from, angle)?;
        let piece = apply_clip(kernel, piece, clip, size, tag)?;
        tool = kernel.boolean(BooleanOp::Union, tool, piece)?;
    }
    Ok(tool)
}

#[allow(clippy::too_many_arguments)]
fn span(
    kernel: &dyn Kernel,
    e: &EdgeInfo,
    shape: Shape,
    offset: f64,
    name: &str,
    from: f64,
    angle: f64,
    tag: &str,
) -> Result<ShapeHandle, KernelError> {
    let arc = e.arc.expect("円の辺");
    let dir = arc.sweep.signum();
    let start = arc.point(e.a, dir * from);
    let r = unit(k::sub(start, arc.center));
    let na = arc.axis;
    let inward = if arc.inside { k::scale(r, -1.0) } else { r };
    // 円筒の面の外向きの法線(出っ張った辺では平面の面の中と逆向き)
    let nb = if e.convex {
        k::scale(inward, -1.0)
    } else {
        inward
    };
    // 回す向きの接線
    let d = k::scale(unit(k::cross(na, r)), dir);
    let plane = Plane {
        origin: start,
        x_axis: na,
        y_axis: k::cross(d, na),
    };
    let c = Section {
        plane,
        length: 0.0,
        na: [1.0, 0.0],
        nb: [k::dot(nb, plane.x_axis), k::dot(nb, plane.y_axis)],
        convex: e.convex,
    };
    let profile: Profile = cutter(&c, shape, offset, name);
    kernel.revolve(&profile, arc.center, k::scale(na, dir), angle, tag)
}
