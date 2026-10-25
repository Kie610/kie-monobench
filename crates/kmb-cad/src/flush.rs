//! 同じ平面で接する面の扱い。
//!
//! truck のブーリアンは、道具の面が相手の面と同じ平面に重なると失敗する
//! (上面と同じ高さから掘るポケット、縁に揃えた切り欠き、上面に載せたボスなど)。
//! SMLib も一部の重なりで失敗する(状態 1001・1006。3D の見本の総当たりで、ずらしを
//! 外すと面から押し出す 4 手が新たに誤りになり、直る手は無かった)ので、どちらの
//! カーネルにも効かせる。
//! そこで、重なる面のうち、はみ出させても結果が変わらない面だけを
//! [`EXTENSION`] だけ外へずらす。切り取りでは相手の外向きの法線と同じ向きの面
//! (材料の外へ出る)、結合では逆向きの面(相手の材料の中へ入る)がそれに当たる。

use crate::Operation;
use kmb_core::kernel::{self as k, FaceInfo, P2, Profile, ProfileLoop, Segment};

/// はみ出させる量(mm)の候補。ブーリアンの許容差(0.05 mm)より大きくする(A)。
/// truck のブーリアンは寸法の組み合わせによって失敗することがある(実測)ので、
/// 失敗したら次の量で作り直す。
pub const EXTENSIONS: [f64; 4] = [0.1, 0.27, 0.53, 0.91];

const ANGLE_TOL: f64 = 1e-9;
const DIST_TOL: f64 = 1e-6;

fn should_extend(op: Operation, point: k::P3, outward: k::P3, faces: &[FaceInfo]) -> bool {
    faces.iter().filter_map(|f| f.planar).any(|p| {
        let n = k::normalize(p.normal);
        let par = k::cross(n, outward);
        let coplanar = k::dot(par, par) < ANGLE_TOL
            && k::dot(k::sub(p.point, point), outward).abs() < DIST_TOL;
        let same = k::dot(n, outward) > 0.0;
        coplanar
            && match op {
                Operation::Cut => same,
                Operation::Join => !same,
                _ => false,
            }
    })
}

/// 押し出しの範囲と輪郭を、重なる面がはみ出すように直す。
pub fn extend_flush(
    profile: &Profile,
    from: f64,
    to: f64,
    op: Operation,
    target_faces: &[FaceInfo],
    ext: f64,
) -> (Profile, f64, f64) {
    if !matches!(op, Operation::Cut | Operation::Join) {
        return (profile.clone(), from, to);
    }
    let plane = profile.plane;
    let n = plane.normal();
    let mut from = from;
    let mut to = to;
    if should_extend(
        op,
        plane.offset(from).origin,
        k::scale(n, -1.0),
        target_faces,
    ) {
        from -= ext;
    }
    if should_extend(op, plane.offset(to).origin, n, target_faces) {
        to += ext;
    }
    let mut out = profile.clone();
    for r in &mut out.regions {
        extend_walls(&mut r.outer, &plane, op, target_faces, ext);
        for h in &mut r.holes {
            extend_walls(h, &plane, op, target_faces, ext);
        }
    }
    (out, from, to)
}

fn line_of(e: &k::ProfileEdge) -> Option<(P2, P2)> {
    match e.segment {
        Segment::Line { .. } => Some((e.from_point(), e.to_point())),
        _ => None,
    }
}

/// たどる向きの右側(領域の外側)を向く単位法線。
fn right_normal(a: P2, b: P2) -> P2 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let l = d[0].hypot(d[1]);
    [d[1] / l, -d[0] / l]
}

fn intersect(a0: P2, a1: P2, b0: P2, b1: P2) -> Option<P2> {
    let d1 = [a1[0] - a0[0], a1[1] - a0[1]];
    let d2 = [b1[0] - b0[0], b1[1] - b0[1]];
    let den = d1[0] * d2[1] - d1[1] * d2[0];
    if den.abs() < 1e-12 * (d1[0].hypot(d1[1]) * d2[0].hypot(d2[1])).max(1e-300) {
        return None;
    }
    let t = ((b0[0] - a0[0]) * d2[1] - (b0[1] - a0[1]) * d2[0]) / den;
    Some([a0[0] + t * d1[0], a0[1] + t * d1[1]])
}

/// 線分の側面が相手の面と重なるなら、その線分を外へずらし、隣の線分を延ばしてつなぐ。
/// 隣が円弧の線分や、隣と平行な線分は動かさない。
fn extend_walls(
    lp: &mut ProfileLoop,
    plane: &k::Plane,
    op: Operation,
    faces: &[FaceInfo],
    ext: f64,
) {
    let n = lp.edges.len();
    if n < 3 {
        return;
    }
    let lines: Vec<Option<(P2, P2)>> = lp.edges.iter().map(line_of).collect();
    let mut shift: Vec<Option<P2>> = vec![None; n];
    for i in 0..n {
        let Some((a, b)) = lines[i] else { continue };
        let (prev, next) = (lines[(i + n - 1) % n], lines[(i + 1) % n]);
        let (Some(p), Some(q)) = (prev, next) else {
            continue;
        };
        if intersect(p.0, p.1, a, b).is_none() || intersect(a, b, q.0, q.1).is_none() {
            continue;
        }
        let o = right_normal(a, b);
        let outward = k::normalize(k::add(
            k::scale(plane.x_axis, o[0]),
            k::scale(plane.y_axis, o[1]),
        ));
        if should_extend(op, plane.to_world(a), outward, faces) {
            shift[i] = Some([o[0] * ext, o[1] * ext]);
        }
    }
    if shift.iter().all(Option::is_none) {
        return;
    }
    let moved: Vec<Option<(P2, P2)>> = (0..n)
        .map(|i| {
            lines[i].map(|(a, b)| match shift[i] {
                Some(s) => ([a[0] + s[0], a[1] + s[1]], [b[0] + s[0], b[1] + s[1]]),
                None => (a, b),
            })
        })
        .collect();
    // 頂点 i は辺 i の終点 = 辺 i+1 の始点
    let mut vertex: Vec<Option<P2>> = vec![None; n];
    for i in 0..n {
        let j = (i + 1) % n;
        if shift[i].is_none() && shift[j].is_none() {
            continue;
        }
        if let (Some(p), Some(q)) = (moved[i], moved[j]) {
            vertex[i] = intersect(p.0, p.1, q.0, q.1);
        }
    }
    for i in 0..n {
        let start = vertex[(i + n - 1) % n];
        let end = vertex[i];
        if start.is_none() && end.is_none() {
            continue;
        }
        let e = &mut lp.edges[i];
        let (mut f, mut t) = (e.from_point(), e.to_point());
        if let Some(s) = start {
            f = s;
        }
        if let Some(v) = end {
            t = v;
        }
        let (a, b) = if e.reversed { (t, f) } else { (f, t) };
        e.segment = Segment::Line { a, b };
    }
}
