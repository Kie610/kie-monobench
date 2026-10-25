//! 3D スケッチのビューで、マウスの下の物を探す(点・線・面・ボディ・投影と交差の下絵)。

use super::*;

/// マウスの下の物。
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Hit {
    Point(EntityId),
    Line(EntityId),
    Circle(EntityId),
    Face(FaceRef),
    /// 作図面(名前の札)
    Plane(u32),
    /// 寸法の値か拘束の印
    Constraint(ConstraintId),
    /// ボディの取り込める角・辺・円(`body_items` の番号)
    Body(usize),
    /// 面の縁かボディ全体として、まとめて取り込む角・辺・円(`body_items` の番号)
    Bodies(Vec<usize>),
    /// ボディから求めた線
    Derived(u32),
    /// 投影の道具で押すと写す物(`geoms` の番号と、その `named` の番号。空ならボディ全体)
    Proj {
        body: usize,
        items: Vec<usize>,
    },
    /// 交差の道具で押すと写す断面の線(`OnPlane::pieces` の番号)
    Piece(usize),
}

/// 作図面で切った面の線 1 本(ボディの番号、面、面の中の番号、線)。
pub(super) type Piece = (usize, String, u32, Vec<P3>);

/// 投影・交差の道具の、今の作図面へ写した下絵。作図面とボディが同じ間は使い回す。
pub(crate) struct OnPlane {
    pub(super) key: [i64; 4],
    /// ボディごとの、辺と角すべてを作図面へ写した線(投影の道具の下絵)
    pub(crate) edges: Option<Vec<Vec<Vec<P3>>>>,
    /// 作図面で切った面ごとの線(交差の道具の下絵)
    pub(crate) pieces: Option<Vec<Piece>>,
    /// ボディごとのシルエット(マウスを乗せた時に求める)
    pub(super) silhouettes: HashMap<usize, Vec<Vec<P3>>>,
    /// マウスの下を押すと写す線
    pub(crate) hover: Vec<Vec<P3>>,
}

/// 作図面の見分け(向きと原点からの距離。作図面の上で描く位置が動いても同じ)。
pub(super) fn plane_key(o: P3, n: P3) -> [i64; 4] {
    let n = geom::unit(n);
    let r = |v: f64| (v * 1e6).round() as i64;
    [r(n[0]), r(n[1]), r(n[2]), r(geom::dot(o, n))]
}

/// 投影の道具で、マウスの下を押すと写す物(写す物の選びに従う)。角と円の中心を先に、次に辺、
/// 無ければマウスの下の面(面・辺・点なら面の縁、ほかはそのボディ)。
pub(super) fn project_hit(ed: &Sketch3dEditor, pos: Pos2) -> Option<Hit> {
    let at = |p: &P3| world_at(&ed.cam, ed.rect, *p);
    let mut best: Option<(f32, usize, usize)> = None;
    for (gi, g) in ed.geoms.iter().enumerate() {
        for (ni, (_, pts)) in g.named.iter().enumerate() {
            if let [p] = pts[..] {
                let d = (at(&p) - pos).length();
                if d < SNAP_PX && best.is_none_or(|b| d < b.0) {
                    best = Some((d, gi, ni));
                }
            }
        }
    }
    if best.is_none() {
        for (gi, g) in ed.geoms.iter().enumerate() {
            for (ni, (_, pts)) in g.named.iter().enumerate() {
                let d = pts
                    .windows(2)
                    .map(|w| seg_dist(pos, at(&w[0]), at(&w[1])))
                    .fold(f32::MAX, f32::min);
                if d < LINE_PX && best.is_none_or(|b| d < b.0) {
                    best = Some((d, gi, ni));
                }
            }
        }
    }
    if ed.project_pick == ProjectPick::Parts {
        if let Some((_, body, ni)) = best {
            return Some(Hit::Proj {
                body,
                items: vec![ni],
            });
        }
        let face = kmb_ui::view3d::pick_at(&ed.scene, &ed.cam, ed.rect, pos)?;
        let body = ed.geoms.iter().position(|g| g.feature == face.feature)?;
        let items: Vec<usize> = ed.geoms[body]
            .named
            .iter()
            .enumerate()
            .filter(|(_, (src, _))| {
                matches!(src, ProjectionSource::Edge { faces, .. } if faces.contains(&face.face))
            })
            .map(|(i, _)| i)
            .collect();
        return (!items.is_empty()).then_some(Hit::Proj { body, items });
    }
    let body = match best {
        Some((_, gi, _)) => gi,
        None => {
            let face = kmb_ui::view3d::pick_at(&ed.scene, &ed.cam, ed.rect, pos)?;
            ed.geoms.iter().position(|g| g.feature == face.feature)?
        }
    };
    Some(Hit::Proj {
        body,
        items: vec![],
    })
}

/// 交差の道具で、マウスの下の断面の線。
pub(super) fn piece_hit(ed: &Sketch3dEditor, pos: Pos2, pr: &Proj<'_>) -> Option<Hit> {
    let pieces = ed.on_plane.as_ref()?.pieces.as_ref()?;
    let mut best: Option<(f32, usize)> = None;
    for (k, (_, _, _, l)) in pieces.iter().enumerate() {
        for w in l.windows(2) {
            if let Some((a, b)) = pr.seg(w[0], w[1]) {
                let d = seg_dist(pos, a, b);
                if d < LINE_PX && best.is_none_or(|x| d < x.0) {
                    best = Some((d, k));
                }
            }
        }
    }
    best.map(|(_, k)| Hit::Piece(k))
}

/// 投影・交差の道具の下絵を、今の作図面へ用意する(ボディの形を集め、作図面が替わったら求め直す)。
pub(super) fn prepare_on_plane(
    ed: &mut Sketch3dEditor,
    kernel: &dyn kmb_core::Kernel,
    dp: &DrawPlane,
) {
    if !matches!(ed.tool, Tool3::Project | Tool3::Intersect) {
        return;
    }
    if ed.geoms_of != ed.bodies {
        ed.geoms = ed
            .bodies
            .iter()
            .filter_map(|(f, h)| body_geom(kernel, *f, *h))
            .collect();
        ed.geoms_of = ed.bodies.clone();
        ed.on_plane = None;
    }
    let key = plane_key(dp.origin, dp.n);
    if ed.on_plane.as_ref().is_none_or(|c| c.key != key) {
        ed.on_plane = Some(OnPlane {
            key,
            edges: None,
            pieces: None,
            silhouettes: HashMap::new(),
            hover: Vec::new(),
        });
    }
    let frame = ed.frame;
    let geoms = &ed.geoms;
    let c = ed.on_plane.as_mut().expect("作った");
    if ed.tool == Tool3::Project && c.edges.is_none() {
        c.edges = Some(
            geoms
                .iter()
                .map(|g| project_body(&ProjectWhat::Edges, g, kernel, &frame, dp.origin, dp.n))
                .collect(),
        );
    }
    if ed.tool == Tool3::Intersect && c.pieces.is_none() {
        c.pieces = Some(
            geoms
                .iter()
                .enumerate()
                .flat_map(|(gi, g)| {
                    section_pieces(&mesh_tris_named(&g.mesh, &frame), dp.origin, dp.n)
                        .into_iter()
                        .map(move |(face, i, l)| (gi, face, i, l))
                })
                .collect(),
        );
    }
}

/// マウスの下を押すと写す線(下絵の中から。シルエットは初めて乗せた時に求める)。
pub(super) fn hover_lines(
    ed: &mut Sketch3dEditor,
    kernel: &dyn kmb_core::Kernel,
    dp: &DrawPlane,
    hover: Option<&Hit>,
) -> Vec<Vec<P3>> {
    let frame = ed.frame;
    let pick = ed.project_pick;
    let whole = ed.intersect_body;
    let geoms = &ed.geoms;
    let Some(c) = ed.on_plane.as_mut() else {
        return vec![];
    };
    match (ed.tool, hover) {
        (Tool3::Project, Some(Hit::Proj { body, items })) => {
            let Some(g) = geoms.get(*body) else {
                return vec![];
            };
            match pick {
                ProjectPick::Parts => {
                    let sources = items.iter().map(|i| g.named[*i].0.clone()).collect();
                    project_body(
                        &ProjectWhat::Parts { sources },
                        g,
                        kernel,
                        &frame,
                        dp.origin,
                        dp.n,
                    )
                }
                ProjectPick::Silhouette => c
                    .silhouettes
                    .entry(*body)
                    .or_insert_with(|| {
                        project_body(&ProjectWhat::Silhouette, g, kernel, &frame, dp.origin, dp.n)
                    })
                    .clone(),
                ProjectPick::Edges => c
                    .edges
                    .as_ref()
                    .and_then(|e| e.get(*body))
                    .cloned()
                    .unwrap_or_default(),
            }
        }
        (Tool3::Intersect, Some(Hit::Piece(k))) => {
            let Some(pieces) = &c.pieces else {
                return vec![];
            };
            let Some(p) = pieces.get(*k) else {
                return vec![];
            };
            if whole {
                pieces
                    .iter()
                    .filter(|q| q.0 == p.0)
                    .map(|q| q.3.clone())
                    .collect()
            } else {
                vec![p.3.clone()]
            }
        }
        _ => vec![],
    }
}

/// 世界座標の点の画面の位置。
pub(super) fn world_at(cam: &Camera, rect: Rect, p: P3) -> Pos2 {
    cam.project(p.map(|v| v as f32), rect).0
}

/// 取り込める物の画面の折れ線(点は 1 つ)。
pub(super) fn item_screen(cam: &Camera, rect: Rect, shape: &BodyShape) -> Vec<Pos2> {
    match *shape {
        BodyShape::Point(p) => vec![world_at(cam, rect, p)],
        BodyShape::Line(a, b) => cam
            .segment(a.map(|v| v as f32), b.map(|v| v as f32), rect)
            .map(|(a, b)| vec![a, b])
            .unwrap_or_default(),
        BodyShape::Circle {
            center,
            radius,
            axis,
        } => circle_points(center, radius, axis)
            .into_iter()
            .map(|q| world_at(cam, rect, q))
            .collect(),
    }
}

/// 取り込みの道具で、マウスの下を押すと取り込む物(取り込む物の選びに従う)。
pub(super) fn include_hit(ed: &Sketch3dEditor, pos: Pos2) -> Option<Hit> {
    use super::super::IncludePick as P;
    if ed.include_pick == P::Parts {
        return body_hit(ed, pos);
    }
    let face = kmb_ui::view3d::pick_at(&ed.scene, &ed.cam, ed.rect, pos);
    let items: Vec<usize> = if ed.include_pick == P::Face {
        let face = face?;
        ed.body_items
            .iter()
            .enumerate()
            .filter(|(_, it)| {
                matches!(&it.source, ProjectionSource::Edge { feature, faces }
                    if *feature == face.feature && faces.contains(&face.face))
            })
            .map(|(i, _)| i)
            .collect()
    } else {
        // 押した角や辺のボディ。無ければマウスの下の面のボディ
        let feature = match body_hit(ed, pos) {
            Some(Hit::Body(i)) => ed.body_items.get(i).and_then(|it| it.source.feature()),
            _ => None,
        }
        .or(face.map(|f| f.feature))?;
        ed.body_items
            .iter()
            .enumerate()
            .filter(|(_, it)| it.source.feature() == Some(feature))
            .map(|(i, _)| i)
            .collect()
    };
    (!items.is_empty()).then_some(Hit::Bodies(items))
}

/// マウスの下の、ボディの取り込める物(角を先に、次に辺と円)。
pub(super) fn body_hit(ed: &Sketch3dEditor, pos: Pos2) -> Option<Hit> {
    let mut best: Option<(f32, usize)> = None;
    for (i, it) in ed.body_items.iter().enumerate() {
        if let BodyShape::Point(p) = it.shape {
            let d = (world_at(&ed.cam, ed.rect, p) - pos).length();
            if d < SNAP_PX && best.is_none_or(|b| d < b.0) {
                best = Some((d, i));
            }
        }
    }
    if let Some((_, i)) = best {
        return Some(Hit::Body(i));
    }
    for (i, it) in ed.body_items.iter().enumerate() {
        if matches!(it.shape, BodyShape::Point(_)) {
            continue;
        }
        let pts = item_screen(&ed.cam, ed.rect, &it.shape);
        let d = pts
            .windows(2)
            .map(|w| seg_dist(pos, w[0], w[1]))
            .fold(f32::MAX, f32::min);
        if d < LINE_PX && best.is_none_or(|b| d < b.0) {
            best = Some((d, i));
        }
    }
    best.map(|(_, i)| Hit::Body(i))
}

/// マウスの下の、ボディから求めた線。
pub(super) fn derived_hit(pos: Pos2, sol: &Solution3d, pr: &Proj<'_>) -> Option<Hit> {
    let mut best: Option<(f32, u32)> = None;
    for (id, lines) in &sol.derived {
        for l in lines {
            // 写した角や円の中心(点 1 つ)
            if let [p] = l[..] {
                let d = (pr.at(p) - pos).length();
                if d < SNAP_PX && best.is_none_or(|x| d < x.0) {
                    best = Some((d, *id));
                }
            }
            for w in l.windows(2) {
                if let Some((a, b)) = pr.seg(w[0], w[1]) {
                    let d = seg_dist(pos, a, b);
                    if d < LINE_PX && best.is_none_or(|x| d < x.0) {
                        best = Some((d, *id));
                    }
                }
            }
        }
    }
    best.map(|(_, id)| Hit::Derived(id))
}

/// マウスの下の物を探す(点、寸法と印、線分、円、面の順)。
pub(super) fn hit_at(
    ui: &egui::Ui,
    pos: Pos2,
    sk: &Sketch3d,
    sol: &Solution3d,
    pr: &Proj<'_>,
    marks: &[Label],
    dim_rects: &[(ConstraintId, Rect)],
) -> Option<Hit> {
    let mut best: Option<(f32, EntityId)> = None;
    for e in &sk.entities {
        if let Entity3d::Point { id, .. } = e
            && let Some(q) = sol.points.get(id)
        {
            let d = (pr.at(*q) - pos).length();
            if d < SNAP_PX && best.is_none_or(|b| d < b.0) {
                best = Some((d, *id));
            }
        }
    }
    if let Some((_, id)) = best {
        return Some(Hit::Point(id));
    }
    // 寸法の値の札と拘束の印(札は回した字を囲む箱なので、下の点を先に拾う)
    if let Some((id, _)) = dim_rects.iter().find(|(_, r)| r.contains(pos)) {
        return Some(Hit::Constraint(*id));
    }
    if let Some(l) = marks.iter().find(|l| label_rect(ui, l).contains(pos)) {
        return Some(Hit::Constraint(l.id));
    }
    let mut best: Option<(f32, Hit)> = None;
    for e in &sk.entities {
        let (d, hit) = match e {
            Entity3d::Line { id, a, b, .. } => {
                let (Some(a), Some(b)) = (sol.points.get(a), sol.points.get(b)) else {
                    continue;
                };
                let Some((sa, sb)) = pr.seg(*a, *b) else {
                    continue;
                };
                (seg_dist(pos, sa, sb), Hit::Line(*id))
            }
            Entity3d::Circle {
                id, center, normal, ..
            } => {
                let (Some(c), Some(r)) = (sol.points.get(center), sol.radii.get(id)) else {
                    continue;
                };
                let pts: Vec<Pos2> = circle_points(*c, *r, *sol.normals.get(id).unwrap_or(normal))
                    .into_iter()
                    .map(|q| pr.at(q))
                    .collect();
                let d = pts
                    .windows(2)
                    .map(|w| seg_dist(pos, w[0], w[1]))
                    .fold(f32::MAX, f32::min);
                (d, Hit::Circle(*id))
            }
            Entity3d::Point { .. } => continue,
        };
        if d < LINE_PX && best.as_ref().is_none_or(|b| d < b.0) {
            best = Some((d, hit));
        }
    }
    if let Some((_, h)) = best {
        return Some(h);
    }
    // 面は手前の物から(重心が視点に近い順)
    let mut faces: Vec<(f32, FaceRef)> = sol
        .faces
        .iter()
        .filter(|f| {
            let poly: Vec<Pos2> = f
                .points
                .iter()
                .filter_map(|p| sol.points.get(p))
                .map(|q| pr.at(*q))
                .collect();
            poly.len() >= 3 && inside(pos, &poly)
        })
        .map(|f| {
            let w = to_world(&pr.frame, f.center);
            (
                pr.cam.project(w.map(|v| v as f32), pr.rect).1,
                f.lines.clone(),
            )
        })
        .collect();
    faces.sort_by(|a, b| b.0.total_cmp(&a.0));
    faces.into_iter().next().map(|(_, f)| Hit::Face(f))
}
