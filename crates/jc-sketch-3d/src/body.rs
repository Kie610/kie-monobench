//! ボディの形を 3D スケッチへ取り込む(角・辺・円の中心)と、ボディから線を求める
//! (平面との交わり、線を伸ばした面との交わり、表への投影)。ボディは三角形の網で扱う。

use crate::geom::{add, cross, dot, len, scale, sub};
use crate::model::{DerivedDef, Entity3d, P3, ProjectWhat, SectionPiece, Sketch3d};
use crate::{Solution3d, basis, plane_at};
use jc_core::{EdgeInfo, FeatureId, Kernel, Plane, ShapeHandle};
use jc_sketch::{EntityId, ProjectionSource};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// 取り込める形(世界座標)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BodyShape {
    Point(P3),
    Line(P3, P3),
    /// 一周の円(中心、半径、軸)
    Circle {
        center: P3,
        radius: f64,
        axis: P3,
    },
}

/// ボディの取り込める角・辺・円の中心(元と形)。
#[derive(Debug, Clone, PartialEq)]
pub struct BodyItem {
    pub source: ProjectionSource,
    pub shape: BodyShape,
}

fn key(p: P3) -> (i64, i64, i64) {
    let r = |v: f64| (v * 1e4).round() as i64;
    (r(p[0]), r(p[1]), r(p[2]))
}

fn full(e: &EdgeInfo) -> bool {
    e.arc
        .is_some_and(|a| (a.sweep.abs() - std::f64::consts::TAU).abs() < 1e-6)
}

/// ボディの角・まっすぐな辺・一周の円の辺と中心(一部だけの円弧の辺は取り込まない)。
pub fn body_items(kernel: &dyn Kernel, feature: FeatureId, shape: ShapeHandle) -> Vec<BodyItem> {
    let mut edges = kernel.edges(shape).unwrap_or_default();
    edges.extend(kernel.arc_edges(shape).unwrap_or_default());
    let mut corners: BTreeMap<(i64, i64, i64), (P3, BTreeSet<String>)> = BTreeMap::new();
    for e in edges.iter().filter(|e| !full(e)) {
        for p in [e.a, e.b] {
            let c = corners.entry(key(p)).or_insert((p, BTreeSet::new()));
            c.1.extend(e.faces.iter().cloned());
        }
    }
    let mut out: Vec<BodyItem> = corners
        .into_values()
        .map(|(p, faces)| BodyItem {
            source: ProjectionSource::Vertex {
                feature,
                faces: faces.into_iter().collect(),
            },
            shape: BodyShape::Point(p),
        })
        .collect();
    for e in &edges {
        match e.arc {
            None => out.push(BodyItem {
                source: ProjectionSource::Edge {
                    feature,
                    faces: e.faces.clone(),
                },
                shape: BodyShape::Line(e.a, e.b),
            }),
            Some(a) if full(e) => {
                out.push(BodyItem {
                    source: ProjectionSource::Edge {
                        feature,
                        faces: e.faces.clone(),
                    },
                    shape: BodyShape::Circle {
                        center: a.center,
                        radius: a.radius,
                        axis: a.axis,
                    },
                });
                out.push(BodyItem {
                    source: ProjectionSource::Center {
                        feature,
                        faces: e.faces.clone(),
                    },
                    shape: BodyShape::Point(a.center),
                });
            }
            Some(_) => {}
        }
    }
    out
}

/// 取り込んだ要素の位置を元の形から取り直す(`frame` はスケッチの座標の平面)。
/// 元が見つからない要素の番号を返す(前の位置のまま)。
pub fn refresh_includes(sk: &mut Sketch3d, items: &[BodyItem], frame: &Plane) -> Vec<EntityId> {
    let local = |p: P3| crate::to_local(frame, p);
    let mut lost = Vec::new();
    for inc in sk.includes.clone() {
        let Some(item) = items.iter().find(|i| i.source.same(&inc.source)) else {
            lost.push(inc.entity);
            continue;
        };
        match (sk.entity(inc.entity).cloned(), item.shape) {
            (Some(Entity3d::Point { id, .. }), BodyShape::Point(p)) => set_point(sk, id, local(p)),
            (Some(Entity3d::Line { a, b, .. }), BodyShape::Line(p, q)) => {
                // 端の向きは近い方に合わせる(面の名前は向きを持たない)
                let (pa, pb) = (local(p), local(q));
                let now = sk.point(a).unwrap_or(pa);
                let (pa, pb) = if len(sub(now, pa)) <= len(sub(now, pb)) {
                    (pa, pb)
                } else {
                    (pb, pa)
                };
                set_point(sk, a, pa);
                set_point(sk, b, pb);
            }
            (
                Some(Entity3d::Circle { id, center, .. }),
                BodyShape::Circle {
                    center: c,
                    radius,
                    axis,
                },
            ) => {
                set_point(sk, center, local(c));
                let n = sub(local(add(c, axis)), local(c));
                if let Some(Entity3d::Circle {
                    radius: r, normal, ..
                }) = sk.entity_mut(id)
                {
                    *r = radius;
                    *normal = crate::geom::unit(n);
                }
            }
            _ => lost.push(inc.entity),
        }
    }
    lost
}

fn set_point(sk: &mut Sketch3d, id: EntityId, p: P3) {
    if let Some(Entity3d::Point { p: q, .. }) = sk.entity_mut(id) {
        *q = p;
    }
}

/// ボディの形を取り込む(要素を足し、取り込みに記す)。足した要素の番号を返す。
/// 既に取り込んであれば、その要素の番号。
pub fn include_item(sk: &mut Sketch3d, item: &BodyItem, frame: &Plane) -> EntityId {
    if let Some(i) = sk.includes.iter().find(|i| i.source.same(&item.source)) {
        return i.entity;
    }
    let local = |p: P3| crate::to_local(frame, p);
    let id = match item.shape {
        BodyShape::Point(p) => sk.add_point(local(p)),
        BodyShape::Line(p, q) => sk.add_segment(local(p), local(q)),
        BodyShape::Circle {
            center,
            radius,
            axis,
        } => {
            let c = sk.add_point(local(center));
            let n = sub(local(add(center, axis)), local(center));
            sk.add_circle(c, radius, n)
        }
    };
    sk.includes.push(crate::model::Include3d {
        entity: id,
        source: item.source.clone(),
    });
    id
}

/// ボディの三角形(スケッチの座標)。
pub type Tri = [P3; 3];

/// 三角形の網をスケッチの座標の三角形にする。
pub fn mesh_tris(mesh: &jc_core::Mesh, frame: &Plane) -> Vec<Tri> {
    let p = |i: u32| {
        let q = mesh.positions[i as usize];
        crate::to_local(frame, [q[0] as f64, q[1] as f64, q[2] as f64])
    };
    mesh.triangles
        .iter()
        .map(|t| [p(t[0]), p(t[1]), p(t[2])])
        .collect()
}

/// 線分の端をつないで折れ線にする(端の位置の丸めで同じ点とみなす)。
fn chain(segs: Vec<(P3, P3)>, tol: f64) -> Vec<Vec<P3>> {
    let k = |p: P3| {
        let r = |v: f64| (v / tol).round() as i64;
        (r(p[0]), r(p[1]), r(p[2]))
    };
    let mut at: HashMap<(i64, i64, i64), Vec<usize>> = HashMap::new();
    for (i, (a, b)) in segs.iter().enumerate() {
        at.entry(k(*a)).or_default().push(i);
        at.entry(k(*b)).or_default().push(i);
    }
    let mut used = vec![false; segs.len()];
    let mut out = Vec::new();
    // 端(つながりが 1 つの点)から始め、残りは輪
    let mut order: Vec<usize> = (0..segs.len()).collect();
    order.sort_by_key(|&i| {
        let (a, b) = segs[i];
        let ends = at[&k(a)].len().min(at[&k(b)].len());
        ends != 1
    });
    for s in order {
        if used[s] {
            continue;
        }
        used[s] = true;
        let (a, b) = segs[s];
        // 開いた線は端から始める(輪はどこからでも)
        let (start, mut cur) = if at[&k(b)].len() == 1 && at[&k(a)].len() != 1 {
            (b, a)
        } else {
            (a, b)
        };
        let mut line = vec![start, cur];
        loop {
            let next = at[&k(cur)].iter().copied().find(|&j| !used[j]);
            let Some(j) = next else { break };
            used[j] = true;
            let (p, q) = segs[j];
            cur = if k(p) == k(cur) { q } else { p };
            line.push(cur);
        }
        out.push(line);
    }
    out
}

/// 三角形と平面(点 `o`、法線 `n`)の交わりの線分。
fn cut(t: &Tri, o: P3, n: P3) -> Option<(P3, P3)> {
    let d: Vec<f64> = t.iter().map(|p| dot(sub(*p, o), n)).collect();
    let mut pts = Vec::new();
    for i in 0..3 {
        let j = (i + 1) % 3;
        let (di, dj) = (d[i], d[j]);
        if (di > 0.0) != (dj > 0.0) {
            let s = di / (di - dj);
            pts.push(add(t[i], scale(sub(t[j], t[i]), s)));
        }
    }
    (pts.len() == 2 && len(sub(pts[0], pts[1])) > 1e-9).then(|| (pts[0], pts[1]))
}

/// 三角形の網と平面(点 `o`、法線 `n`)の交わりの折れ線。
pub fn section(tris: &[Tri], o: P3, n: P3) -> Vec<Vec<P3>> {
    chain(tris.iter().filter_map(|t| cut(t, o, n)).collect(), 1e-6)
}

/// 三角形の網をスケッチの座標の三角形にし、面の名前を付ける。
pub fn mesh_tris_named(mesh: &jc_core::Mesh, frame: &Plane) -> Vec<(Tri, String)> {
    let tris = mesh_tris(mesh, frame);
    tris.into_iter()
        .enumerate()
        .map(|(i, t)| {
            let name = mesh
                .triangle_faces
                .get(i)
                .and_then(|f| mesh.face_names.get(*f as usize))
                .cloned()
                .unwrap_or_default();
            (t, name)
        })
        .collect()
}

/// 網と平面の交わりを、面ごとにつなげた線(面の名前、その面の中の何番目、折れ線)。
/// 交差の「押した線」はこの面と番号で持つ(2D の交差と同じ考え)。
pub fn section_pieces(tris: &[(Tri, String)], o: P3, n: P3) -> Vec<(String, u32, Vec<P3>)> {
    let mut by_face: BTreeMap<&str, Vec<(P3, P3)>> = BTreeMap::new();
    for (t, face) in tris {
        if let Some(seg) = cut(t, o, n) {
            by_face.entry(face.as_str()).or_default().push(seg);
        }
    }
    let mut out = Vec::new();
    for (face, segs) in by_face {
        for (i, line) in chain(segs, 1e-6).into_iter().enumerate() {
            out.push((face.to_string(), i as u32, line));
        }
    }
    out
}

/// 点 `p` を通り向き `dir` の直線と網の交わり(直線の上の位置 t。小さい順)。
fn hits(tris: &[Tri], p: P3, dir: P3) -> Vec<f64> {
    let mut out = Vec::new();
    for t in tris {
        // メラー・トランボアの方法
        let e1 = sub(t[1], t[0]);
        let e2 = sub(t[2], t[0]);
        let h = cross(dir, e2);
        let a = dot(e1, h);
        if a.abs() < 1e-12 {
            continue;
        }
        let f = 1.0 / a;
        let s = sub(p, t[0]);
        let u = f * dot(s, h);
        if !(-1e-9..=1.0 + 1e-9).contains(&u) {
            continue;
        }
        let q = cross(s, e1);
        let v = f * dot(dir, q);
        if v < -1e-9 || u + v > 1.0 + 1e-9 {
            continue;
        }
        out.push(f * dot(e2, q));
    }
    out.sort_by(f64::total_cmp);
    out.dedup_by(|a, b| (*a - *b).abs() < 1e-7);
    out
}

/// スケッチの線(線分・円)を細かく分けた点の並び(解いた位置)。
pub fn curve_samples(sk: &Sketch3d, sol: &Solution3d, ids: &[EntityId]) -> Vec<Vec<P3>> {
    let mut out = Vec::new();
    for id in ids {
        match sk.entity(*id) {
            Some(Entity3d::Line { a, b, .. }) => {
                let (Some(p), Some(q)) = (sol.points.get(a), sol.points.get(b)) else {
                    continue;
                };
                out.push(
                    (0..=64)
                        .map(|k| add(*p, scale(sub(*q, *p), k as f64 / 64.0)))
                        .collect(),
                );
            }
            Some(Entity3d::Circle {
                id, center, normal, ..
            }) => {
                let (Some(c), Some(r)) = (sol.points.get(center), sol.radii.get(id)) else {
                    continue;
                };
                let (u, v) = basis(*sol.normals.get(id).unwrap_or(normal));
                out.push(
                    (0..=96)
                        .map(|k| {
                            let t = std::f64::consts::TAU * k as f64 / 96.0;
                            add(*c, add(scale(u, r * t.cos()), scale(v, r * t.sin())))
                        })
                        .collect(),
                );
            }
            _ => {}
        }
    }
    out
}

/// 点の並びを向き `dir` に沿って網へ写す。`all` なら交わりをすべて(層ごとの線に)、
/// 偽なら一番近い交わりだけ。交わらない所で線を切る。
fn along(samples: &[Vec<P3>], tris: &[Tri], dir: P3, all: bool) -> Vec<Vec<P3>> {
    let dir = crate::geom::unit(dir);
    let mut out: Vec<Vec<P3>> = Vec::new();
    for pts in samples {
        // 層(k 番目の交わり)ごとに、つながっている間の線を伸ばす
        let mut open: Vec<Vec<P3>> = Vec::new();
        for p in pts {
            let ts = hits(tris, *p, dir);
            let picked: Vec<f64> = if all {
                ts
            } else {
                ts.iter()
                    .copied()
                    .min_by(|a, b| a.abs().total_cmp(&b.abs()))
                    .into_iter()
                    .collect()
            };
            if picked.len() != open.len() {
                out.extend(open.drain(..).filter(|l| l.len() >= 2));
                open = vec![Vec::new(); picked.len()];
            }
            for (l, t) in open.iter_mut().zip(picked) {
                l.push(add(*p, scale(dir, t)));
            }
        }
        out.extend(open.into_iter().filter(|l| l.len() >= 2));
    }
    out
}

/// ボディから求める線の折れ線(スケッチの座標)。求められなければ空。
pub fn derived_lines(
    sk: &Sketch3d,
    sol: &Solution3d,
    def: &DerivedDef,
    tris: &[Tri],
) -> Vec<Vec<P3>> {
    match def {
        DerivedDef::Section { plane, .. } => match plane_at(sk, sol, plane) {
            Some((o, n)) => section(tris, o, n),
            None => vec![],
        },
        DerivedDef::Intersection { curves, dir, .. } => {
            along(&curve_samples(sk, sol, curves), tris, *dir, true)
        }
        DerivedDef::OnSurface { curves, dir, .. } => {
            along(&curve_samples(sk, sol, curves), tris, *dir, false)
        }
        // 網の三角形だけでは求められない(名前の付いた形が要る)。[`derived_lines_geom`]
        DerivedDef::Project { .. } => vec![],
    }
}

/// 網だけでなく、ボディの名前の付いた形が要る線か(交差の押した線と投影)。
pub fn needs_geom(def: &DerivedDef) -> bool {
    match def {
        DerivedDef::Section { pieces, .. } => !pieces.is_empty(),
        DerivedDef::Project { .. } => true,
        _ => false,
    }
}

/// 作図面へ写すボディ(世界座標)。網と、名前の付いた角・辺・円の中心。
pub struct BodyGeom {
    pub feature: FeatureId,
    pub shape: ShapeHandle,
    /// 網(2D の投影と同じ細かさ [`jc_sketch::BODY_MESH_TOL`])
    pub mesh: jc_core::Mesh,
    /// 名前の付いた角・辺・円の中心(世界座標の折れ線。角と中心は点 1 つ)。円弧の辺も含む
    pub named: Vec<(ProjectionSource, Vec<P3>)>,
}

/// 円の辺の点の並び(`a` から軸まわりに回す。一周なら 96 分割)。
fn arc_points(a: P3, arc: &jc_core::ArcEdge) -> Vec<P3> {
    let k = crate::geom::unit(arc.axis);
    let v = sub(a, arc.center);
    let n = ((arc.sweep.abs() / std::f64::consts::TAU * 96.0).ceil() as usize).max(4);
    (0..=n)
        .map(|i| {
            let t = arc.sweep * i as f64 / n as f64;
            let (s, c) = t.sin_cos();
            // ロドリゲスの回転
            let r = add(
                add(scale(v, c), scale(cross(k, v), s)),
                scale(k, dot(k, v) * (1.0 - c)),
            );
            add(arc.center, r)
        })
        .collect()
}

/// ボディの網と、名前の付いた角・辺・円の中心を集める。網が作れなければ `None`。
pub fn body_geom(kernel: &dyn Kernel, feature: FeatureId, shape: ShapeHandle) -> Option<BodyGeom> {
    let mesh = kernel.mesh(shape, jc_sketch::BODY_MESH_TOL).ok()?;
    let mut edges = kernel.edges(shape).unwrap_or_default();
    edges.extend(kernel.arc_edges(shape).unwrap_or_default());
    let mut named: Vec<(ProjectionSource, Vec<P3>)> = Vec::new();
    // 角は body_items と同じ(一周の円の継ぎ目は角にしない)
    for it in body_items(kernel, feature, shape) {
        if let (ProjectionSource::Vertex { .. }, BodyShape::Point(p)) = (&it.source, it.shape) {
            named.push((it.source, vec![p]));
        }
    }
    for e in edges {
        let faces = e.faces.clone();
        match e.arc {
            None => named.push((ProjectionSource::Edge { feature, faces }, vec![e.a, e.b])),
            Some(arc) => {
                named.push((
                    ProjectionSource::Edge {
                        feature,
                        faces: faces.clone(),
                    },
                    arc_points(e.a, &arc),
                ));
                named.push((
                    ProjectionSource::Center { feature, faces },
                    vec![arc.center],
                ));
            }
        }
    }
    Some(BodyGeom {
        feature,
        shape,
        mesh,
        named,
    })
}

/// 写した形(2D の平面の座標)の点の並び。線分は両端、円と円弧は細かく。
fn ref_points(shape: &jc_sketch::RefShape) -> Vec<[f64; 2]> {
    match *shape {
        jc_sketch::RefShape::Line(a, b) => vec![a, b],
        ref s => jc_sketch::reference_samples(s, 96),
    }
}

/// 写した線を片付ける: 長さ 0 につぶれた線(平面に垂直な辺)は捨て、同じ位置に重なる線は 1 本に。
fn tidy(lines: Vec<Vec<P3>>) -> Vec<Vec<P3>> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for l in lines {
        if l.len() >= 2 && l.windows(2).map(|w| len(sub(w[1], w[0]))).sum::<f64>() < 1e-6 {
            continue;
        }
        let ks: Vec<(i64, i64, i64)> = l.iter().map(|p| key(*p)).collect();
        let mut rev = ks.clone();
        rev.reverse();
        let k = ks.clone().min(rev);
        if seen.insert(k) {
            out.push(l);
        }
    }
    out
}

/// ボディ `b` を平面(点 `o`、法線 `n`、スケッチの座標)へ垂直に写した線(スケッチの座標。角と
/// 円の中心は点 1 つの並び)。`frame` はスケッチの座標の平面。
pub fn project_body(
    what: &ProjectWhat,
    b: &BodyGeom,
    kernel: &dyn Kernel,
    frame: &Plane,
    o: P3,
    n: P3,
) -> Vec<Vec<P3>> {
    let n = crate::geom::unit(n);
    let flat = |p: P3| sub(p, scale(n, dot(sub(p, o), n)));
    let local = |w: P3| flat(crate::to_local(frame, w));
    let named = |keep: &dyn Fn(&ProjectionSource) -> bool| -> Vec<Vec<P3>> {
        b.named
            .iter()
            .filter(|(s, _)| keep(s))
            .map(|(_, pts)| pts.iter().map(|p| local(*p)).collect())
            .collect()
    };
    let lines = match what {
        ProjectWhat::Parts { sources } => named(&|s| sources.iter().any(|x| x.same(s))),
        ProjectWhat::Edges => {
            // 角と円の中心、辺は網の辺(名前の付かない曲面どうしの辺も写す)
            let mut v = named(&|s| {
                matches!(
                    s,
                    ProjectionSource::Vertex { .. } | ProjectionSource::Center { .. }
                )
            });
            if b.mesh.edges.is_empty() {
                v.extend(named(&|s| matches!(s, ProjectionSource::Edge { .. })));
            } else {
                v.extend(b.mesh.edges.iter().map(|l| {
                    l.iter()
                        .map(|q| local([q[0] as f64, q[1] as f64, q[2] as f64]))
                        .collect()
                }));
            }
            v
        }
        ProjectWhat::Silhouette => {
            // 平面を世界座標にし、2D の投影と同じシルエット(外形に乗る辺はその辺の形)を求める
            let wo = crate::to_world(frame, o);
            let wn = sub(crate::to_world(frame, add(o, n)), wo);
            let plane = Plane::from_point_normal(wo, wn);
            let refs = jc_sketch::body_refs(kernel, b.feature, b.shape, &plane);
            jc_sketch::silhouette_refs(&b.mesh, &plane, b.feature, &refs)
                .iter()
                .map(|r| {
                    ref_points(&r.shape)
                        .into_iter()
                        .map(|q| local(plane.to_world(q)))
                        .collect()
                })
                .collect()
        }
    };
    tidy(lines)
}

/// ボディから求める線のうち [`needs_geom`] の物(交差の押した線と投影)。`bodies` はその線の
/// 相手のボディ。
pub fn derived_lines_geom(
    sk: &Sketch3d,
    sol: &Solution3d,
    def: &DerivedDef,
    bodies: &[&BodyGeom],
    kernel: &dyn Kernel,
    frame: &Plane,
) -> Vec<Vec<P3>> {
    let Some((o, n)) = def.plane().and_then(|p| plane_at(sk, sol, p)) else {
        return vec![];
    };
    match def {
        DerivedDef::Section { pieces, .. } => {
            let tris: Vec<(Tri, String)> = bodies
                .iter()
                .flat_map(|b| mesh_tris_named(&b.mesh, frame))
                .collect();
            section_pieces(&tris, o, n)
                .into_iter()
                .filter(|(face, index, _)| {
                    pieces.iter().any(|p| p.face == *face && p.index == *index)
                })
                .map(|(_, _, l)| l)
                .collect()
        }
        DerivedDef::Project { what, .. } => bodies
            .iter()
            .flat_map(|b| project_body(what, b, kernel, frame, o, n))
            .collect(),
        _ => vec![],
    }
}

/// 断面の線を指す(面と番号)。
pub fn piece(face: &str, index: u32) -> SectionPiece {
    SectionPiece {
        face: face.to_string(),
        index,
    }
}
