//! 解いたスケッチから、押し出しや回転に使う閉じた輪郭を取り出す。

use crate::geom::{cross, dist, sub};
use crate::{Entity, EntityId, Sketch, Solution};
use kmb_core::kernel::{
    Curves, P2, Plane, Profile, ProfileEdge, ProfileFace, ProfileFaces, ProfileLoop, Region,
    Segment, arc_sweep, canonical_loop, merge_regions, natural_key, point_in_polygon, polygon_area,
};
use std::collections::HashMap;

/// 補助線でない線分・円弧・円を、平面の上の区間にする(スケッチの順)。
fn drawn_segments(sketch: &Sketch, sol: &Solution) -> Vec<(EntityId, Segment)> {
    entity_segments(sketch, sol, false)
}

/// 線分・円弧・円を、平面の上の区間にする(スケッチの順)。`construction` なら補助線も含める。
/// 楕円・スプライン・円錐曲線は、線分と円弧の並び(バイアーク。許容差
/// [`crate::ARC_TOLERANCE`])にして、同じ要素の番号で続けて入れる(CNC の G2/G3 にそのまま使える)。
pub fn entity_segments(
    sketch: &Sketch,
    sol: &Solution,
    construction: bool,
) -> Vec<(EntityId, Segment)> {
    let pt = |id: &EntityId| sol.points.get(id).copied();
    let mut out = Vec::new();
    for e in &sketch.entities {
        match e {
            Entity::Line {
                id,
                a,
                b,
                construction: c,
            } if construction || !c => {
                if let (Some(a), Some(b)) = (pt(a), pt(b)) {
                    out.push((*id, Segment::Line { a, b }));
                }
            }
            Entity::Arc {
                id,
                center,
                start,
                end,
                construction: c,
            } if construction || !c => {
                if let (Some(c), Some(s), Some(t)) = (pt(center), pt(start), pt(end)) {
                    let radius = (s[0] - c[0]).hypot(s[1] - c[1]);
                    out.push((
                        *id,
                        Segment::Arc {
                            center: c,
                            radius,
                            start: (s[1] - c[1]).atan2(s[0] - c[0]),
                            end: (t[1] - c[1]).atan2(t[0] - c[0]),
                        },
                    ));
                }
            }
            Entity::Circle {
                id,
                center,
                construction: c,
                ..
            } if construction || !c => {
                if let (Some(c), Some(r)) = (pt(center), sol.radii.get(id)) {
                    out.push((
                        *id,
                        Segment::Circle {
                            center: c,
                            radius: *r,
                        },
                    ));
                }
            }
            Entity::Ellipse { id, .. } | Entity::Spline { id, .. } | Entity::Conic { id, .. }
                if construction || !e.is_construction() =>
            {
                if let Some(s) = sketch.solved_shape(sol, *id) {
                    out.extend(
                        s.segments(crate::curve::ARC_TOLERANCE)
                            .into_iter()
                            .map(|seg| (*id, seg)),
                    );
                }
            }
            _ => {}
        }
    }
    out
}

/// 補助線でない線を全部(閉じていなくてもよい)。彫刻など、線をなぞる加工に渡す。
pub fn curves_of(sketch: &Sketch, sol: &Solution, plane: Plane) -> Curves {
    Curves {
        plane,
        segments: drawn_segments(sketch, sol)
            .into_iter()
            .map(|(_, s)| s)
            .collect(),
    }
}

/// 補助線でない線分・円弧・円を交点で分け、閉じた領域を探す。
/// 1 つ目の値は押し出しなどの既定の輪郭(重なった形は合わせ、入れ子の形は偶奇で穴にする)、
/// 2 つ目はどの領域にも使われない線(途切れた線など)の名前。
pub fn profile_of(sketch: &Sketch, sol: &Solution, plane: Plane) -> (Profile, Vec<String>) {
    let a = Arrangement::new(&drawn_segments(sketch, sol));
    let filled: Vec<&Region> = a
        .faces
        .iter()
        .filter(|f| f.filled)
        .map(|f| &f.region)
        .collect();
    (
        Profile {
            plane,
            regions: merge_regions(&filled),
        },
        a.unused,
    )
}

/// 線で囲まれた一番小さな領域の全部(押し出しで 1 つずつ選べる)。
pub fn faces_of(sketch: &Sketch, sol: &Solution, plane: Plane) -> ProfileFaces {
    let a = Arrangement::new(&drawn_segments(sketch, sol));
    ProfileFaces {
        plane,
        faces: a
            .faces
            .into_iter()
            .map(|f| ProfileFace {
                key: f.key,
                region: f.region,
            })
            .collect(),
    }
}

/// 閉じていない輪郭の端(どの線ともつながらない線の端)の位置。補助線は数えない。押し出せない理由を図の上の印で見せるのに使う。
pub fn open_ends_of(sketch: &Sketch, sol: &Solution) -> Vec<[f64; 2]> {
    Arrangement::new(&drawn_segments(sketch, sol)).open_ends
}

/// 点を同じとみなす距離(mm)。
const EPS: f64 = 1e-6;
const TAU: f64 = std::f64::consts::TAU;

/// 交点で分けた区間 1 つ。`from`・`to` は頂点の番号(切れ目のない円は `None`)。
struct Piece {
    name: String,
    /// 名前の並び(スケッチの順、区間の順)
    segment: Segment,
    /// 同じ区間を描いた要素の名前。輪郭に使われた重複を未使用とは数えない。
    sources: Vec<String>,
    ends: Option<(usize, usize)>,
    alive: bool,
}

struct Face {
    key: String,
    region: Region,
    filled: bool,
}

/// 線を交点で分けた図(頂点・区間・領域)。
struct Arrangement {
    faces: Vec<Face>,
    unused: Vec<String>,
    /// どの線ともつながらない端(押し出せない理由の目印)
    open_ends: Vec<P2>,
}

/// 円の中心・半径と、円弧なら範囲(始まりの角度・中心角)。
type CircleOf = (P2, f64, Option<(f64, f64)>);

/// 円(中心・半径)と、円弧なら範囲(始まりの角度・中心角)。
fn circle_of(s: &Segment) -> Option<CircleOf> {
    match s {
        Segment::Line { .. } => None,
        Segment::Arc {
            center,
            radius,
            start,
            end,
        } => Some((*center, *radius, Some((*start, arc_sweep(*start, *end))))),
        Segment::Circle { center, radius } => Some((*center, *radius, None)),
    }
}

/// 区間の上の点 `p` の位置(線分は 0〜1、円弧は始まりからの角度、円は 0〜2π の角度)。
/// 区間の上になければ `None`。
fn param_on(s: &Segment, p: P2) -> Option<f64> {
    match s {
        Segment::Line { a, b } => {
            let d = sub(*b, *a);
            let len2 = d[0] * d[0] + d[1] * d[1];
            if len2 < EPS * EPS {
                return None;
            }
            let t = ((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / len2;
            let len = len2.sqrt();
            let off = cross(d, sub(p, *a)).abs() / len;
            (off < EPS && t > -EPS / len && t < 1.0 + EPS / len).then(|| t.clamp(0.0, 1.0))
        }
        _ => {
            let (c, r, range) = circle_of(s)?;
            if (dist(p, c) - r).abs() > EPS {
                return None;
            }
            let ang = (p[1] - c[1]).atan2(p[0] - c[0]);
            match range {
                None => Some(ang.rem_euclid(TAU)),
                Some((start, sweep)) => {
                    let mut o = (ang - start).rem_euclid(TAU);
                    let tol = EPS / r.max(EPS);
                    if o > TAU - tol {
                        o = 0.0;
                    }
                    (o < sweep + tol).then(|| o.min(sweep))
                }
            }
        }
    }
}

/// 2 つの区間の交点の候補(区間の上にあるかは後で確かめる)。
fn crossings(s: &Segment, t: &Segment) -> Vec<P2> {
    let mut out = Vec::new();
    // 端点が相手の上にある(T 字のつながり、重なった線)
    for e in [s, t] {
        if !matches!(e, Segment::Circle { .. }) {
            out.push(e.start_point());
            out.push(e.end_point());
        }
    }
    match (circle_of(s), circle_of(t)) {
        (None, None) => {
            let (Segment::Line { a: a1, b: b1 }, Segment::Line { a: a2, b: b2 }) = (s, t) else {
                return out;
            };
            let (d1, d2) = (sub(*b1, *a1), sub(*b2, *a2));
            let den = cross(d1, d2);
            if den.abs() > 1e-12 {
                let u = cross(sub(*a2, *a1), d2) / den;
                out.push([a1[0] + d1[0] * u, a1[1] + d1[1] * u]);
            }
        }
        (Some((c, r, _)), None) | (None, Some((c, r, _))) => {
            let Segment::Line { a, b } = (if circle_of(s).is_none() { s } else { t }) else {
                return out;
            };
            let d = sub(*b, *a);
            let len = (d[0] * d[0] + d[1] * d[1]).sqrt();
            if len < EPS {
                return out;
            }
            let u = [d[0] / len, d[1] / len];
            let k = (c[0] - a[0]) * u[0] + (c[1] - a[1]) * u[1];
            let foot = [a[0] + u[0] * k, a[1] + u[1] * k];
            let h = dist(foot, c);
            if h <= r + EPS {
                let w = (r * r - h * h).max(0.0).sqrt();
                out.push([foot[0] + u[0] * w, foot[1] + u[1] * w]);
                out.push([foot[0] - u[0] * w, foot[1] - u[1] * w]);
            }
        }
        (Some((c1, r1, _)), Some((c2, r2, _))) => {
            let d = dist(c1, c2);
            if d > EPS && d <= r1 + r2 + EPS && d >= (r1 - r2).abs() - EPS {
                let a = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
                let h = (r1 * r1 - a * a).max(0.0).sqrt();
                let u = [(c2[0] - c1[0]) / d, (c2[1] - c1[1]) / d];
                let m = [c1[0] + u[0] * a, c1[1] + u[1] * a];
                out.push([m[0] - u[1] * h, m[1] + u[0] * h]);
                out.push([m[0] + u[1] * h, m[1] - u[0] * h]);
            }
        }
    }
    out
}

/// 区間の位置 `a`〜`b` の部分。
fn part(s: &Segment, a: f64, b: f64) -> Segment {
    match s {
        Segment::Line { a: p, b: q } => {
            let at = |t: f64| [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t];
            Segment::Line { a: at(a), b: at(b) }
        }
        Segment::Arc {
            center,
            radius,
            start,
            ..
        } => Segment::Arc {
            center: *center,
            radius: *radius,
            start: start + a,
            end: start + b,
        },
        Segment::Circle { center, radius } => Segment::Arc {
            center: *center,
            radius: *radius,
            start: a,
            end: b,
        },
    }
}

/// 区間の長さ(mm)。
fn segment_length(s: &Segment) -> f64 {
    match s {
        Segment::Line { a, b } => dist(*a, *b),
        Segment::Arc {
            radius, start, end, ..
        } => radius * arc_sweep(*start, *end),
        Segment::Circle { radius, .. } => TAU * radius,
    }
}

/// 向きを除いて、位置と範囲が同じ区間か。同じ端の反対側の弧は別に扱う。
fn same_segment(a: &Segment, b: &Segment) -> bool {
    let close = |a, b| dist(a, b) < EPS;
    match (a, b) {
        (Segment::Line { a, b }, Segment::Line { a: c, b: d }) => {
            (close(*a, *c) && close(*b, *d)) || (close(*a, *d) && close(*b, *c))
        }
        _ => {
            let (Some((a_center, a_radius, a_range)), Some((b_center, b_radius, b_range))) =
                (circle_of(a), circle_of(b))
            else {
                return false;
            };
            if !close(a_center, b_center) || (a_radius - b_radius).abs() >= EPS {
                return false;
            }
            let a_sweep = a_range.map_or(TAU, |(_, sweep)| sweep);
            let b_sweep = b_range.map_or(TAU, |(_, sweep)| sweep);
            if (a_sweep - b_sweep).abs() * a_radius.max(b_radius) >= EPS {
                return false;
            }
            (a_sweep - TAU).abs() * a_radius < EPS && (b_sweep - TAU).abs() * b_radius < EPS
                || close(a.start_point(), b.start_point()) && close(a.end_point(), b.end_point())
        }
    }
}

/// 半辺の出ていく向き(角度、0〜2π)と、曲がり具合(左へ曲がるほど大きい)。
fn leaving(s: &Segment, forward: bool) -> (f64, f64) {
    let (ang, curv) = match (s, forward) {
        (Segment::Line { a, b }, true) => ((b[1] - a[1]).atan2(b[0] - a[0]), 0.0),
        (Segment::Line { a, b }, false) => ((a[1] - b[1]).atan2(a[0] - b[0]), 0.0),
        (Segment::Arc { radius, start, .. }, true) => {
            (start + std::f64::consts::FRAC_PI_2, 1.0 / radius)
        }
        (Segment::Arc { radius, end, .. }, false) => {
            (end - std::f64::consts::FRAC_PI_2, -1.0 / radius)
        }
        (Segment::Circle { .. }, _) => (0.0, 0.0),
    };
    let mut a = ang.rem_euclid(TAU);
    if a > TAU - 1e-9 {
        a = 0.0;
    }
    (a, curv)
}

impl Arrangement {
    fn new(segs: &[(EntityId, Segment)]) -> Arrangement {
        let n = segs.len();
        // 区間の名前。1 つの要素が何区間にもなる(自由曲線の近似)なら、`e5.0`、`e5.1` と番号を付ける
        let mut count: HashMap<EntityId, usize> = HashMap::new();
        for (id, _) in segs {
            *count.entry(*id).or_default() += 1;
        }
        let mut seen: HashMap<EntityId, usize> = HashMap::new();
        let names: Vec<String> = segs
            .iter()
            .map(|(id, _)| {
                if count[id] > 1 {
                    let k = seen.entry(*id).or_default();
                    *k += 1;
                    format!("e{}.{}", id.0, *k - 1)
                } else {
                    format!("e{}", id.0)
                }
            })
            .collect();
        // 交点と端点を、区間ごとの位置にする
        let mut cuts: Vec<Vec<P2>> = vec![Vec::new(); n];
        for (i, (_, s)) in segs.iter().enumerate() {
            if !matches!(s, Segment::Circle { .. }) {
                cuts[i].push(s.start_point());
                cuts[i].push(s.end_point());
            }
        }
        for i in 0..n {
            for j in i + 1..n {
                for p in crossings(&segs[i].1, &segs[j].1) {
                    if param_on(&segs[i].1, p).is_some() && param_on(&segs[j].1, p).is_some() {
                        cuts[i].push(p);
                        cuts[j].push(p);
                    }
                }
            }
        }
        // 頂点(近い点は 1 つにまとめる)
        let mut verts: Vec<P2> = Vec::new();
        let mut vid = |p: P2| -> usize {
            if let Some(k) = verts.iter().position(|q| dist(*q, p) < EPS * 10.0) {
                k
            } else {
                verts.push(p);
                verts.len() - 1
            }
        };
        let mut pieces: Vec<Piece> = Vec::new();
        for (i, (_, s)) in segs.iter().enumerate() {
            let mut at: Vec<(f64, usize)> = cuts[i]
                .iter()
                .filter_map(|p| Some((param_on(s, *p)?, vid(*p))))
                .collect();
            at.sort_by(|a, b| a.0.total_cmp(&b.0));
            at.dedup_by(|b, a| a.1 == b.1);
            let mut parts: Vec<(Segment, usize, usize)> = Vec::new();
            match s {
                Segment::Circle { .. } => {
                    if at.len() > 1 && at[0].1 == at[at.len() - 1].1 {
                        at.pop();
                    }
                    for k in 0..at.len() {
                        let (a, va) = at[k];
                        let (b, vb) = if k + 1 < at.len() {
                            at[k + 1]
                        } else {
                            (at[0].0 + TAU, at[0].1)
                        };
                        parts.push((part(s, a, b), va, vb));
                    }
                }
                _ => {
                    for w in at.windows(2) {
                        parts.push((part(s, w[0].0, w[1].0), w[0].1, w[1].1));
                    }
                }
            }
            let name = names[i].clone();
            if matches!(s, Segment::Circle { .. }) && parts.is_empty() {
                pieces.push(Piece {
                    sources: vec![name.clone()],
                    name,
                    segment: s.clone(),
                    ends: None,
                    alive: true,
                });
            } else if parts.len() == 1 && !matches!(s, Segment::Circle { .. }) {
                // 切れ目のない線分・円弧は、そのままの形と名前にする
                let (_, va, vb) = parts.pop().expect("1 つある");
                pieces.push(Piece {
                    sources: vec![name.clone()],
                    name,
                    segment: s.clone(),
                    ends: Some((va, vb)),
                    alive: true,
                });
            } else {
                for (k, (seg, va, vb)) in parts.into_iter().enumerate() {
                    let name = format!("{name}.{k}");
                    pieces.push(Piece {
                        sources: vec![name.clone()],
                        name,
                        segment: seg,
                        ends: Some((va, vb)),
                        alive: true,
                    });
                }
            }
        }

        // 許容差の近さは推移しないため、代表の名前と形を自然順で先に固定する。
        pieces.sort_by_cached_key(|p| natural_key(&p.name));
        // 同じ位置の区間を半辺として二重に数えない。
        let mut unique: Vec<Piece> = Vec::new();
        for p in pieces {
            match unique
                .iter()
                .position(|q| same_segment(&p.segment, &q.segment))
            {
                Some(i) => unique[i].sources.extend(p.sources),
                None => unique.push(p),
            }
        }
        let mut pieces = unique;
        // 重複の端が作った切れ目に別の線が来なければ、区間をつなぎ直す。
        // 本当の交差や分岐は残し、そこから順に番号を付けるので領域の名前も保てる。
        let has_duplicates = pieces.iter().any(|p| p.sources.len() > 1);
        for ((_, original), name) in segs.iter().zip(&names).filter(|_| has_duplicates) {
            let prefix = format!("{name}.");
            let belongs = |p: &Piece| p.name == *name || p.name.starts_with(&prefix);
            let parts: Vec<_> = pieces.iter().filter(|p| belongs(p)).collect();
            if parts.is_empty() || !parts.iter().any(|p| p.sources.len() > 1) {
                continue;
            }
            let length: f64 = parts.iter().map(|p| segment_length(&p.segment)).sum();
            if (length - segment_length(original)).abs() > EPS * parts.len() as f64 {
                continue;
            }
            let mut degree = vec![0usize; verts.len()];
            for p in &pieces {
                if let Some((a, b)) = p.ends {
                    degree[a] += 1;
                    degree[b] += 1;
                }
            }
            let mut groups = Vec::new();
            let mut rest = Vec::new();
            for p in pieces {
                if belongs(&p) {
                    groups.push(p);
                } else {
                    rest.push(p);
                }
            }
            groups.sort_by(|a, b| {
                param_on(original, a.segment.start_point())
                    .unwrap_or(0.0)
                    .total_cmp(&param_on(original, b.segment.start_point()).unwrap_or(0.0))
            });
            loop {
                let n = groups.len();
                let joined = (0..n).find_map(|i| {
                    let j = (i + 1) % n;
                    if i == j || j == 0 && !matches!(original, Segment::Circle { .. }) {
                        return None;
                    }
                    let (Some((a, at)), Some((from, b))) = (groups[i].ends, groups[j].ends) else {
                        return None;
                    };
                    if at != from || degree[at] != 2 {
                        return None;
                    }
                    let start = param_on(original, groups[i].segment.start_point())?;
                    let end = match original {
                        Segment::Line { .. } => param_on(original, groups[j].segment.end_point())?,
                        _ => {
                            start
                                + (segment_length(&groups[i].segment)
                                    + segment_length(&groups[j].segment))
                                    / circle_of(original)?.1
                        }
                    };
                    Some((i, j, part(original, start, end), (a, b)))
                });
                let Some((i, j, segment, ends)) = joined else {
                    break;
                };
                let mut sources = std::mem::take(&mut groups[j].sources);
                groups[i].sources.append(&mut sources);
                groups[i].segment = segment;
                groups[i].ends = Some(ends);
                groups.remove(j);
            }
            groups.sort_by(|a, b| {
                param_on(original, a.segment.start_point())
                    .unwrap_or(0.0)
                    .total_cmp(&param_on(original, b.segment.start_point()).unwrap_or(0.0))
            });
            let whole = groups.len() == 1
                && same_segment(&groups[0].segment, original)
                // 円に戻すと頂点が無くなる。接する別の線がある頂点は円弧の端として残す。
                && (!matches!(original, Segment::Circle { .. })
                    || groups[0].ends.is_none_or(|(a, b)| a == b && degree[a] == 2));
            for (i, p) in groups.iter_mut().enumerate() {
                if whole {
                    p.name = name.clone();
                    p.segment = original.clone();
                    if matches!(original, Segment::Circle { .. }) {
                        p.ends = None;
                    }
                } else {
                    p.name = format!("{name}.{i}");
                }
            }
            rest.extend(groups);
            pieces = rest;
        }

        // 行き止まりの区間(片方の端がどこにもつながらない)を、なくなるまで外す
        let mut degree = vec![0usize; verts.len()];
        for p in &pieces {
            if let Some((a, b)) = p.ends {
                degree[a] += 1;
                degree[b] += 1;
            }
        }
        // 1 本の線しか来ない頂点が、閉じていない輪郭の端(交点で分けた後なので、
        // 別の線の途中に乗った端はつながっているとみなす)
        let open_ends: Vec<P2> = verts
            .iter()
            .zip(&degree)
            .filter(|(_, d)| **d == 1)
            .map(|(p, _)| *p)
            .collect();
        loop {
            let mut changed = false;
            for p in pieces.iter_mut().filter(|p| p.alive) {
                if let Some((a, b)) = p.ends
                    && (degree[a] < 2 || degree[b] < 2)
                {
                    p.alive = false;
                    degree[a] -= 1;
                    degree[b] -= 1;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        // 半辺: 区間 k の順向きが 2k、逆向きが 2k+1。頂点ごとに出ていく向きで並べる
        let mut out: Vec<Vec<usize>> = vec![Vec::new(); verts.len()];
        for (k, p) in pieces.iter().enumerate().filter(|(_, p)| p.alive) {
            if let Some((a, b)) = p.ends {
                out[a].push(2 * k);
                out[b].push(2 * k + 1);
            }
        }
        let dir = |h: usize| leaving(&pieces[h / 2].segment, h.is_multiple_of(2));
        for list in &mut out {
            list.sort_by(|&x, &y| {
                let (ax, cx) = dir(x);
                let (ay, cy) = dir(y);
                if (ax - ay).abs() > 1e-9 {
                    ax.total_cmp(&ay)
                } else {
                    cx.total_cmp(&cy)
                }
            });
        }
        let head = |h: usize| {
            let (a, b) = pieces[h / 2].ends.expect("つながった区間");
            if h.is_multiple_of(2) { b } else { a }
        };
        // h の次: 着いた頂点で、h を逆にたどる半辺の 1 つ時計回り側(領域を左に見てたどる)
        let next = |h: usize| {
            let v = head(h);
            let list = &out[v];
            let i = list
                .iter()
                .position(|&x| x == h ^ 1)
                .expect("逆の半辺がある");
            list[(i + list.len() - 1) % list.len()]
        };
        let edge = |h: usize| ProfileEdge {
            name: pieces[h / 2].name.clone(),
            segment: pieces[h / 2].segment.clone(),
            reversed: h % 2 == 1,
        };

        // 輪郭をたどる。区間ごとのつながり(成分)も覚える
        let mut comp_of_vert: Vec<usize> = (0..verts.len()).collect();
        fn root(c: &mut [usize], mut v: usize) -> usize {
            while c[v] != v {
                c[v] = c[c[v]];
                v = c[v];
            }
            v
        }
        for p in pieces.iter().filter(|p| p.alive) {
            if let Some((a, b)) = p.ends {
                let (ra, rb) = (root(&mut comp_of_vert, a), root(&mut comp_of_vert, b));
                comp_of_vert[ra] = rb;
            }
        }
        // (輪郭, 成分の番号)
        let mut cycles: Vec<(ProfileLoop, usize)> = Vec::new();
        let mut seen = vec![false; pieces.len() * 2];
        for (k, p) in pieces.iter().enumerate() {
            if !p.alive {
                continue;
            }
            if p.ends.is_none() {
                // 切れ目のない円: 内側(反時計回り)と外側(時計回り)
                let comp = verts.len() + k;
                for rev in [false, true] {
                    cycles.push((
                        ProfileLoop {
                            edges: vec![ProfileEdge {
                                name: p.name.clone(),
                                segment: p.segment.clone(),
                                reversed: rev,
                            }],
                        },
                        comp,
                    ));
                }
                continue;
            }
            for h0 in [2 * k, 2 * k + 1] {
                if seen[h0] {
                    continue;
                }
                let mut edges = Vec::new();
                let mut h = h0;
                while !seen[h] {
                    seen[h] = true;
                    edges.push(edge(h));
                    h = next(h);
                }
                let comp = root(&mut comp_of_vert, head(h0));
                cycles.push((ProfileLoop { edges }, comp));
            }
        }

        let polys: Vec<Vec<P2>> = cycles.iter().map(|(l, _)| l.polyline()).collect();
        let areas: Vec<f64> = polys.iter().map(|p| polygon_area(p)).collect();
        let tiny = 1e-9;
        // 成分の外まわり(時計回りの輪郭)と、成分がいくつの外まわりに囲まれるか
        let outer_of: HashMap<usize, usize> = (0..cycles.len())
            .filter(|&i| areas[i] < -tiny)
            .map(|i| (cycles[i].1, i))
            .collect();
        let depth = |comp: usize, probe: P2| {
            outer_of
                .iter()
                .filter(|(c, i)| **c != comp && point_in_polygon(probe, &polys[**i]))
                .count()
        };
        let mut faces: Vec<(usize, Vec<ProfileLoop>)> = (0..cycles.len())
            .filter(|&i| areas[i] > tiny)
            .map(|i| (i, vec![cycles[i].0.clone()]))
            .collect();
        // 外まわりは、それを囲むいちばん小さな領域(別の成分)の穴にする
        for (&comp, &i) in &outer_of {
            let probe = polys[i][0];
            let parent = faces
                .iter()
                .map(|(f, _)| *f)
                .filter(|&f| cycles[f].1 != comp && point_in_polygon(probe, &polys[f]))
                .min_by(|&a, &b| areas[a].total_cmp(&areas[b]));
            if let Some(p) = parent
                && let Some((_, loops)) = faces.iter_mut().find(|(f, _)| *f == p)
            {
                loops.push(cycles[i].0.clone());
            }
        }
        let mut out_faces: Vec<Face> = faces
            .into_iter()
            .map(|(i, loops)| {
                let mut names: Vec<&str> = loops
                    .iter()
                    .flat_map(|l| &l.edges)
                    .map(|e| e.name.as_str())
                    .collect();
                names.sort_by_key(|n| natural_key(n));
                names.dedup();
                let key = names.join("+");
                let mut it = loops.into_iter();
                let outer = canonical_loop(it.next().expect("外周がある"), true);
                let mut holes: Vec<ProfileLoop> = it.map(|l| canonical_loop(l, false)).collect();
                holes.sort_by_key(|h| h.edges.iter().map(|e| natural_key(&e.name)).min());
                Face {
                    key,
                    region: Region { outer, holes },
                    filled: depth(cycles[i].1, polys[i][0]) % 2 == 0,
                }
            })
            .collect();
        out_faces.sort_by_key(|f| {
            f.region
                .outer
                .edges
                .iter()
                .map(|e| natural_key(&e.name))
                .min()
        });
        let mut unused: Vec<String> = Vec::new();
        let mut ids: Vec<EntityId> = segs.iter().map(|(id, _)| *id).collect();
        ids.dedup();
        for id in ids {
            let name = format!("e{}", id.0);
            let used = pieces.iter().any(|p| {
                p.alive
                    && p.sources
                        .iter()
                        .any(|s| s == &name || s.starts_with(&format!("{name}.")))
            });
            if !used {
                unused.push(name);
            }
        }
        Arrangement {
            faces: out_faces,
            unused,
            open_ends,
        }
    }
}

#[cfg(test)]
mod duplicate_tests {
    use super::*;
    use std::f64::consts::{FRAC_PI_2, PI};

    fn line(id: u32, a: P2, b: P2) -> (EntityId, Segment) {
        (EntityId(id), Segment::Line { a, b })
    }

    fn circle(id: u32) -> (EntityId, Segment) {
        (
            EntityId(id),
            Segment::Circle {
                center: [0.0, 0.0],
                radius: 5.0,
            },
        )
    }

    fn arc(id: u32, start: f64, end: f64) -> (EntityId, Segment) {
        (
            EntityId(id),
            Segment::Arc {
                center: [0.0, 0.0],
                radius: 5.0,
                start,
                end,
            },
        )
    }

    fn signature(segs: &[(EntityId, Segment)]) -> Vec<(String, Region, bool)> {
        let a = Arrangement::new(segs);
        assert!(a.open_ends.is_empty(), "{:?}", a.open_ends);
        assert!(a.unused.is_empty(), "{:?}", a.unused);
        a.faces
            .into_iter()
            .map(|f| (f.key, f.region, f.filled))
            .collect()
    }

    #[test]
    fn duplicate_circle_keeps_the_naturally_first_name_even_when_drawn_later() {
        let want = signature(&[circle(2)]);
        assert_eq!(signature(&[circle(10), circle(2)]), want);
        assert_eq!(signature(&[circle(2), circle(10)]), want);
        assert_eq!(want[0].0, "e2");
    }

    #[test]
    fn a_reversed_duplicate_line_does_not_change_the_outline_or_its_keys() {
        let base = vec![
            line(2, [0.0, 0.0], [20.0, 0.0]),
            line(3, [20.0, 0.0], [20.0, 10.0]),
            line(4, [20.0, 10.0], [0.0, 10.0]),
            line(5, [0.0, 10.0], [0.0, 0.0]),
        ];
        let mut doubled = base.clone();
        doubled.extend(
            base.iter()
                .enumerate()
                .map(|(i, (_, s))| line(i as u32 + 20, s.end_point(), s.start_point())),
        );
        assert_eq!(signature(&doubled), signature(&base));
    }

    #[test]
    fn duplicate_arcs_are_merged_but_opposite_semicircles_are_not() {
        let base = [arc(2, 0.0, PI), arc(3, PI, TAU)];
        assert!(!same_segment(&base[0].1, &base[1].1));
        assert_eq!(
            signature(&[
                base[0].clone(),
                base[1].clone(),
                arc(20, 0.0, PI),
                arc(21, PI, TAU)
            ]),
            signature(&base)
        );
        assert_eq!(signature(&base).len(), 1);
    }

    #[test]
    fn an_overlapping_part_of_a_line_preserves_real_intersections_and_region_keys() {
        for split in [false, true] {
            let mut base = vec![
                line(2, [0.0, 0.0], [20.0, 0.0]),
                line(3, [20.0, 0.0], [20.0, 10.0]),
                line(4, [20.0, 10.0], [0.0, 10.0]),
                line(5, [0.0, 10.0], [0.0, 0.0]),
            ];
            if split {
                base.push(line(6, [10.0, 0.0], [10.0, 10.0]));
            }
            let mut doubled = base.clone();
            doubled.push(line(20, [5.0, 0.0], [15.0, 0.0]));
            assert_eq!(signature(&doubled), signature(&base), "分割={split}");
        }
    }

    #[test]
    fn an_overlapping_arc_preserves_a_circle_and_its_real_intersections() {
        for split in [false, true] {
            let mut base = vec![circle(2)];
            if split {
                base.push(line(3, [-5.0, 0.0], [5.0, 0.0]));
            }
            let mut doubled = base.clone();
            doubled.push(arc(20, FRAC_PI_2 / 2.0, PI + FRAC_PI_2 / 2.0));
            assert_eq!(signature(&doubled), signature(&base), "分割={split}");
        }
    }

    #[test]
    fn a_duplicate_circle_keeps_a_real_tangent_vertex_and_region_names() {
        let base = vec![
            circle(2),
            line(3, [5.0, 0.0], [10.0, 0.0]),
            line(4, [10.0, 0.0], [10.0, 5.0]),
            line(5, [10.0, 5.0], [5.0, 5.0]),
            line(6, [5.0, 5.0], [5.0, 0.0]),
        ];
        let want = signature(&base);
        let mut doubled = base;
        doubled.push(circle(20));
        assert_eq!(signature(&doubled), want);
    }

    #[test]
    fn near_duplicate_representatives_do_not_depend_on_insertion_order() {
        let a = line(10, [0.0, 0.0], [10.0, 0.0]);
        let b = line(2, [0.0, EPS * 0.9], [10.0, EPS * 0.9]);
        let c = line(20, [0.0, EPS * 1.8], [10.0, EPS * 1.8]);
        let mut results = Vec::new();
        for order in [
            [a.clone(), b.clone(), c.clone()],
            [c.clone(), a.clone(), b.clone()],
            [b.clone(), c.clone(), a.clone()],
            [a.clone(), c.clone(), b.clone()],
            [b.clone(), a.clone(), c.clone()],
            [c.clone(), b.clone(), a.clone()],
        ] {
            let arrangement = Arrangement::new(&order);
            assert!(arrangement.faces.is_empty());
            results.push((arrangement.open_ends.len(), arrangement.unused.len()));
        }
        assert!(results.iter().all(|r| *r == (2, 3)), "{results:?}");
    }

    #[test]
    fn nearby_distinct_curves_are_not_deduplicated() {
        let a = circle(2).1;
        let b = Segment::Circle {
            center: [EPS * 2.0, 0.0],
            radius: 5.0,
        };
        assert!(!same_segment(&a, &b));
        let a = line(2, [0.0, 0.0], [10.0, 0.0]).1;
        let b = line(3, [0.0, EPS * 2.0], [10.0, EPS * 2.0]).1;
        assert!(!same_segment(&a, &b));
    }
}
