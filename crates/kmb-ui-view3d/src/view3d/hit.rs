//! 拾う: 画面の点・範囲にある面・辺・点・ボディを探す。

use super::*;

/// 画面上の点にある、いちばん手前の面。
pub fn pick_at(scene: &Scene, cam: &Camera, rect: Rect, pos: Pos2) -> Option<Pick> {
    picks_at(scene, cam, rect, pos)
        .into_iter()
        .next()
        .map(|h| h.0)
}

/// 画面上の点にある面を、手前から順に(面ごとに 1 つ)。当たった空間の点つき。
/// 点を通る視線と、ボディごとの三角形の箱の木で調べる(三角形を全部は写さない)。
pub fn picks_at(scene: &Scene, cam: &Camera, rect: Rect, pos: Pos2) -> Vec<(Pick, [f32; 3])> {
    crate::perf::timed(crate::perf::Part::Pick, || {
        picks_on_ray(scene, cam, rect, pos)
    })
}

pub(super) fn picks_on_ray(
    scene: &Scene,
    cam: &Camera,
    rect: Rect,
    pos: Pos2,
) -> Vec<(Pick, [f32; 3])> {
    let (o, d) = cam.ray(pos, rect);
    let (_, _, e) = cam.basis();
    let near = cam.near_limit(rect);
    // 透視投影は視点から先だけ、平行投影は視線の全部(画面の奥行きに限りはない)
    let (o, t_min) = match cam.eye_position(rect) {
        Some(eye) => (eye, 0.0),
        None => (o, f32::NEG_INFINITY),
    };
    let mut hits: Vec<(f32, Pick, [f32; 3])> = Vec::new();
    for b in &scene.bodies {
        let m = &b.mesh;
        b.ray(o, d, t_min, |t, at| {
            let p = [0, 1, 2].map(|i| o[i] + d[i] * at);
            let depth = dot(sub(p, cam.target), e);
            // 視点に近すぎて描かない所と、断面で切り取った所は拾わない
            if near.is_some_and(|zmax| depth > zmax)
                || scene.section.is_some_and(|sec| sec.removes(p))
            {
                return;
            }
            let face = m.triangle_faces[t as usize] as usize;
            let pick = Pick {
                feature: b.feature,
                face: m.face_names[face].clone(),
            };
            match hits.iter_mut().find(|h| h.1 == pick) {
                Some(h) if h.0 < depth => (h.0, h.2) = (depth, p),
                Some(_) => {}
                None => hits.push((depth, pick, p)),
            }
        });
    }
    hits.sort_by(|a, b| b.0.total_cmp(&a.0));
    hits.into_iter().map(|(_, p, at)| (p, at)).collect()
}

/// 三角形を全部写して調べる、前の拾い方(箱の木の答えと比べる試験と、速さの比べに使う)。
#[doc(hidden)]
pub fn picks_at_brute(scene: &Scene, cam: &Camera, rect: Rect, pos: Pos2) -> Vec<(Pick, [f32; 3])> {
    let mut hits: Vec<(f32, Pick)> = Vec::new();
    let near = cam.near_limit(rect);
    for b in &scene.bodies {
        let m = &b.mesh;
        let pts: Vec<(Pos2, f32)> = m.positions.iter().map(|p| cam.project(*p, rect)).collect();
        for (t, face) in m.triangles.iter().zip(&m.triangle_faces) {
            let corners = [0, 1, 2].map(|i| m.positions[t[i] as usize]);
            let v = [0, 1, 2].map(|i| pts[t[i] as usize]);
            let depth = if near.is_some_and(|zmax| v.iter().any(|q| q.1 > zmax)) {
                // 視点に近すぎる所を切った残りで当たりを見る
                clip_near(cam, rect, corners)
                    .into_iter()
                    .filter_map(|tri| {
                        let v = tri.map(|p| cam.project(p, rect));
                        hit_depth(cam, rect, v, pos)
                    })
                    .reduce(f32::max)
            } else {
                hit_depth(cam, rect, v, pos)
            };
            let Some(depth) = depth else { continue };
            let pick = Pick {
                feature: b.feature,
                face: m.face_names[*face as usize].clone(),
            };
            match hits.iter_mut().find(|h| h.1 == pick) {
                Some(h) => h.0 = h.0.max(depth),
                None => hits.push((depth, pick)),
            }
        }
    }
    hits.sort_by(|a, b| b.0.total_cmp(&a.0));
    hits.into_iter()
        .map(|(d, p)| (p, cam.unproject(pos, d, rect)))
        .collect()
}

/// 画面に写した三角形 `v`(位置と近さ)に点 `pos` が入っていれば、そこの近さ。
pub(super) fn hit_depth(cam: &Camera, rect: Rect, v: [(Pos2, f32); 3], pos: Pos2) -> Option<f32> {
    let [(a, da), (bb, db), (c, dc)] = v;
    let den = (bb.y - c.y) * (a.x - c.x) + (c.x - bb.x) * (a.y - c.y);
    if den.abs() < 1e-9 {
        return None;
    }
    let l1 = ((bb.y - c.y) * (pos.x - c.x) + (c.x - bb.x) * (pos.y - c.y)) / den;
    let l2 = ((c.y - a.y) * (pos.x - c.x) + (a.x - c.x) * (pos.y - c.y)) / den;
    let l3 = 1.0 - l1 - l2;
    if l1 < 0.0 || l2 < 0.0 || l3 < 0.0 {
        return None;
    }
    Some(cam.depth_at(rect, &[l1, l2, l3], &[da, db, dc]))
}

/// 透視投影で、三角形の視点に近すぎる所を切り落とした残り(0〜2 個の三角形)。
pub(super) fn clip_near(cam: &Camera, rect: Rect, tri: [[f32; 3]; 3]) -> Vec<[[f32; 3]; 3]> {
    clip_near_with(cam, rect, tri.map(|p| (p, 0.0)))
        .into_iter()
        .map(|t| t.map(|v| v.0))
        .collect()
}

/// [`clip_near`] の、点ごとの値(明るさ)も一緒に補間するもの。
pub(super) fn clip_near_with(
    cam: &Camera,
    rect: Rect,
    tri: [([f32; 3], f32); 3],
) -> Vec<[([f32; 3], f32); 3]> {
    let Some(zmax) = cam.near_limit(rect) else {
        return vec![tri];
    };
    let (_, _, e) = cam.basis();
    let z = |p: [f32; 3]| dot(sub(p, cam.target), e);
    let mut poly: Vec<([f32; 3], f32)> = Vec::with_capacity(4);
    for i in 0..3 {
        let (a, b) = (tri[i], tri[(i + 1) % 3]);
        let (za, zb) = (z(a.0), z(b.0));
        if za <= zmax {
            poly.push(a);
        }
        if (za <= zmax) != (zb <= zmax) {
            let t = (zmax - za) / (zb - za);
            poly.push((
                [0, 1, 2].map(|k| a.0[k] + (b.0[k] - a.0[k]) * t),
                a.1 + (b.1 - a.1) * t,
            ));
        }
    }
    (1..poly.len().saturating_sub(1))
        .map(|i| [poly[0], poly[i], poly[i + 1]])
        .collect()
}

/// 断面 `sec` で切り取る側を落とした三角形の残り(0〜2 個。点ごとの値も補間する)。
pub(super) fn clip_section_with(
    sec: Option<Section>,
    tri: [([f32; 3], f32); 3],
) -> Vec<[([f32; 3], f32); 3]> {
    let Some(sec) = sec else {
        return vec![tri];
    };
    let d = |p: [f32; 3]| dot(p, sec.normal) - sec.offset;
    if tri.iter().all(|v| d(v.0) <= 0.0) {
        return vec![tri];
    }
    let mut poly: Vec<([f32; 3], f32)> = Vec::with_capacity(4);
    for i in 0..3 {
        let (a, b) = (tri[i], tri[(i + 1) % 3]);
        let (da, db) = (d(a.0), d(b.0));
        if da <= 0.0 {
            poly.push(a);
        }
        if (da <= 0.0) != (db <= 0.0) {
            let t = da / (da - db);
            poly.push((
                [0, 1, 2].map(|k| a.0[k] + (b.0[k] - a.0[k]) * t),
                a.1 + (b.1 - a.1) * t,
            ));
        }
    }
    (1..poly.len().saturating_sub(1))
        .map(|i| [poly[0], poly[i], poly[i + 1]])
        .collect()
}

/// 線分の、断面で切り取る側を落とした残り。
pub(super) fn clip_section_segment(
    sec: Option<Section>,
    a: [f32; 3],
    b: [f32; 3],
) -> Option<([f32; 3], [f32; 3])> {
    let Some(sec) = sec else {
        return Some((a, b));
    };
    let (da, db) = (
        dot(a, sec.normal) - sec.offset,
        dot(b, sec.normal) - sec.offset,
    );
    let at = |t: f32| [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t);
    match (da <= 0.0, db <= 0.0) {
        (true, true) => Some((a, b)),
        (false, false) => None,
        (true, false) => Some((a, at(da / (da - db)))),
        (false, true) => Some((at(da / (da - db)), b)),
    }
}

/// 切り口の蓋の色(断面で切った時に見える裏の面。ボディの色を差し色へ少し寄せ、暗くする)。
pub fn cap_color(colors: &Colors) -> Color32 {
    shade(crate::mix(colors.body, colors.picked, 0.35), 0.8)
}

/// 陰影と隠れ線の、隠れた辺の点線の描く長さとあける長さ(point)。
pub const DASH_ON: f32 = 4.0;
pub const DASH_OFF: f32 = 3.0;

/// 辺を拾う、画面の上の距離(点)。
pub const EDGE_PX: f32 = 8.0;

/// 画面上の点のいちばん近くにある、見えている辺(`edges` はボディごとの辺)。
/// 手前の面に隠れた辺は拾わない(辺のいちばん近い点で、面の箱の木を引いた手前の面より奥なら隠れている)。
/// `see_through` が真(線だけの表示)なら、隠れた辺も拾う。隠したボディの辺は拾わない。
/// 辺を拾う道はこれ 1 本(GPU の有無で拾う辺が変わらない。2026-10-24 Kie)。
pub fn edge_at(
    scene: &Scene,
    cam: &Camera,
    rect: Rect,
    pos: Pos2,
    edges: &[(FeatureId, Vec<kmb_core::EdgeInfo>)],
    see_through: bool,
) -> Option<kmb_core::EdgePick> {
    crate::perf::timed(crate::perf::Part::Pick, || {
        edge_near(scene, cam, rect, pos, edges, see_through)
    })
}

pub(super) fn edge_near(
    scene: &Scene,
    cam: &Camera,
    rect: Rect,
    pos: Pos2,
    edges: &[(FeatureId, Vec<kmb_core::EdgeInfo>)],
    see_through: bool,
) -> Option<kmb_core::EdgePick> {
    let size = sub(scene.max, scene.min);
    let eps = 0.01 * dot(size, size).sqrt().max(1.0);
    let f32s = |p: [f64; 3]| p.map(|v| v as f32);
    // 近い区間ごとに(画面の距離、辺の上のいちばん近い点、その近さ、辺)
    let mut near: Vec<(f32, Pos2, f32, &FeatureId, &kmb_core::EdgeInfo)> = Vec::new();
    for (feature, list) in edges {
        if !scene.bodies.iter().any(|b| b.feature == *feature) {
            continue;
        }
        for e in list {
            // 辺を包む球が画面でマウスから遠ければ、折れ線を作らずに飛ばす(辺の多い形で軽くする)
            if let Some((c, r)) = edge_sphere(e)
                && let Some(area) = box_on_screen(cam, rect, c, r)
                && !area.expand(EDGE_PX).contains(pos)
            {
                continue;
            }
            for w in edge_polyline(e).windows(2) {
                let Some((p, q)) = clip_section_segment(scene.section, f32s(w[0]), f32s(w[1]))
                else {
                    continue;
                };
                let Some((p, q)) = cam.clip_segment(p, q, rect) else {
                    continue;
                };
                let (a, da) = cam.project(p, rect);
                let (b, db) = cam.project(q, rect);
                let ab = b - a;
                let len2 = ab.length_sq();
                let t = if len2 < 1e-6 {
                    0.0
                } else {
                    ((pos - a).dot(ab) / len2).clamp(0.0, 1.0)
                };
                let at = a + ab * t;
                let d = at.distance(pos);
                if d < EDGE_PX {
                    let depth = cam.depth_at(rect, &[1.0 - t, t], &[da, db]);
                    near.push((d, at, depth, feature, e));
                }
            }
        }
    }
    near.sort_by(|x, y| x.0.total_cmp(&y.0));
    // 近い順に、辺の上のその点で手前の面より奥でない物を拾う
    near.into_iter()
        .find(|&(_, at, depth, _, _)| {
            see_through
                || picks_at(scene, cam, rect, at)
                    .first()
                    .is_none_or(|(_, hit)| depth >= cam.project(*hit, rect).1 - eps)
        })
        .map(|(_, _, _, feature, e)| kmb_core::EdgePick {
            feature: *feature,
            faces: e.faces.clone(),
        })
}

/// 辺を包む球(真ん中と半径)。曲がった辺は折れ線の点を包む。
fn edge_sphere(e: &kmb_core::EdgeInfo) -> Option<([f32; 3], f32)> {
    let f32s = |p: [f64; 3]| p.map(|v| v as f32);
    if let Some(arc) = e.arc {
        return Some((f32s(arc.center), arc.radius as f32));
    }
    let (a, b) = (f32s(e.a), f32s(e.b));
    let c = [0, 1, 2].map(|i| (a[i] + b[i]) / 2.0);
    let far = |p: [f32; 3]| dot(sub(p, c), sub(p, c)).sqrt();
    let r = e.curve.iter().map(|p| far(f32s(*p))).fold(far(a), f32::max);
    Some((c, r))
}

/// 真ん中 `c`・半径 `r` の球を包む箱が画面で占める四角(角を写した範囲。箱の写しは角の写しの
/// 凸包に入る)。角が視点に近すぎて決まらなければ `None`。
fn box_on_screen(cam: &Camera, rect: Rect, c: [f32; 3], r: f32) -> Option<Rect> {
    let dist = cam.distance(rect);
    let mut out = Rect::NOTHING;
    for k in 0..8 {
        let s = |bit: usize| if k & bit == 0 { -r } else { r };
        let p = [c[0] + s(1), c[1] + s(2), c[2] + s(4)];
        let (q, z) = cam.project(p, rect);
        if dist.is_some_and(|d| d - z <= d * super::camera::NEAR) {
            return None;
        }
        out.extend_with(q);
    }
    Some(out)
}

/// 測る道具で点を拾う、画面の上の距離(点)。辺より先に拾う。
pub(super) const POINT_PX: f32 = 9.0;

/// 測る道具で、画面上の点 `pos` にある物(頂点・円の中心・辺・面の順に近いものを先に)。
/// 頂点と辺は網の稜線から取るので、STEP の形でも拾える。手前の面に隠れた物は拾わない。
pub fn measure_hit(
    scene: &Scene,
    cam: &Camera,
    rect: Rect,
    pos: Pos2,
    see_through: bool,
) -> Option<kmb_measure::Target> {
    crate::perf::timed(crate::perf::Part::Pick, || {
        measure_near(scene, cam, rect, pos, see_through)
    })
}

pub(super) fn measure_near(
    scene: &Scene,
    cam: &Camera,
    rect: Rect,
    pos: Pos2,
    see_through: bool,
) -> Option<kmb_measure::Target> {
    use kmb_measure::{Target, fit_circle};
    let size = sub(scene.max, scene.min);
    let eps = 0.01 * dot(size, size).sqrt().max(1.0);
    // 画面の点 `q` にある、いちばん手前の面の近さ
    let front_at = |q: Pos2| {
        picks_at(scene, cam, rect, q)
            .first()
            .map(|(_, at)| cam.project(*at, rect).1)
    };
    let visible = |p: [f32; 3]| {
        if see_through {
            return true;
        }
        let (q, d) = cam.project(p, rect);
        front_at(q).is_none_or(|f| d >= f - eps)
    };
    let f64s = |p: [f32; 3]| p.map(|v| v as f64);
    // 点の候補: 稜線の両端と、円・円弧の稜線の中心
    let mut points: Vec<(f32, Target, [f32; 3])> = Vec::new();
    for b in &scene.bodies {
        for (i, line) in b.mesh.edges.iter().enumerate() {
            let mut ends = vec![line[0], line[line.len() - 1]];
            if line.len() >= 4 {
                let pts: Vec<[f64; 3]> = line.iter().map(|p| f64s(*p)).collect();
                if let Some(c) = fit_circle(&pts) {
                    let at = c.center.map(|v| v as f32);
                    let d = cam.project(at, rect).0.distance(pos);
                    if d < POINT_PX {
                        points.push((
                            d,
                            Target::Center {
                                feature: b.feature,
                                edge: i,
                            },
                            at,
                        ));
                    }
                }
                if pts.first() == pts.last() {
                    ends.clear();
                }
            }
            for at in ends {
                let d = cam.project(at, rect).0.distance(pos);
                if d < POINT_PX {
                    points.push((
                        d,
                        Target::Vertex {
                            feature: b.feature,
                            at: f64s(at),
                        },
                        at,
                    ));
                }
            }
        }
    }
    points.sort_by(|a, b| a.0.total_cmp(&b.0));
    if let Some((_, t, _)) = points.into_iter().find(|(_, _, at)| visible(*at)) {
        return Some(t);
    }
    // 辺の候補: 稜線の近く(手前の面に隠れていないもの)
    let front = if see_through { None } else { front_at(pos) };
    let mut best: Option<(f32, Target)> = None;
    for b in &scene.bodies {
        for (i, line) in b.mesh.edges.iter().enumerate() {
            for w in line.windows(2) {
                let Some((p, q)) = cam.clip_segment(w[0], w[1], rect) else {
                    continue;
                };
                let (a, da) = cam.project(p, rect);
                let (c, dc) = cam.project(q, rect);
                let ac = c - a;
                let l2 = ac.length_sq();
                let t = if l2 < 1e-6 {
                    0.0
                } else {
                    ((pos - a).dot(ac) / l2).clamp(0.0, 1.0)
                };
                let d = (a + ac * t).distance(pos);
                let depth = cam.depth_at(rect, &[1.0 - t, t], &[da, dc]);
                let hidden = front.is_some_and(|f| depth < f - eps);
                if d < EDGE_PX && !hidden && best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                    best = Some((
                        d,
                        Target::Edge {
                            feature: b.feature,
                            edge: i,
                        },
                    ));
                }
            }
        }
    }
    if let Some((_, t)) = best {
        return Some(t);
    }
    pick_at(scene, cam, rect, pos).map(Target::Face)
}

/// 辺を折れ線にする(円の辺は一周 64 区間の細かさ)。
pub fn edge_polyline(e: &kmb_core::EdgeInfo) -> Vec<[f64; 3]> {
    let turn = e.arc.map_or(0.0, |a| a.sweep.abs() / std::f64::consts::TAU);
    e.polyline((turn * 64.0).ceil() as usize)
}

/// 辺 `pick` を描く線分(世界座標。円の辺は折れ線の区間ごと。見つからなければ空)。
pub fn edge_ends(
    edges: &[(FeatureId, Vec<kmb_core::EdgeInfo>)],
    pick: &kmb_core::EdgePick,
) -> Vec<([f64; 3], [f64; 3])> {
    edges
        .iter()
        .filter(|(f, _)| *f == pick.feature)
        .flat_map(|(_, l)| l)
        .filter(|e| e.faces == pick.faces)
        .flat_map(|e| {
            edge_polyline(e)
                .windows(2)
                .map(|w| (w[0], w[1]))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// 見えている面ごとに(面、範囲にかかったか、範囲から出た所があるか)。
pub(super) fn face_coverage(
    scene: &Scene,
    cam: &Camera,
    rect: Rect,
    area: Rect,
) -> Vec<(Pick, bool, bool)> {
    let eye = cam.eye_position(rect);
    let near = cam.near_limit(rect);
    // 面ごとに(範囲にかかったか、範囲から出た所があるか)。見える向きの三角形だけで決める
    let mut faces: Vec<(Pick, bool, bool)> = Vec::new();
    for b in &scene.bodies {
        let m = &b.mesh;
        for (t, face) in m.triangles.iter().zip(&m.triangle_faces) {
            let w = |i: usize| m.positions[t[i] as usize];
            let c = [0, 1, 2].map(|k| (w(0)[k] + w(1)[k] + w(2)[k]) / 3.0);
            if dot(
                cross(sub(w(1), w(0)), sub(w(2), w(0))),
                cam.toward_eye(c, eye),
            ) <= 0.0
            {
                continue;
            }
            // 断面で切り取った三角形は数えない
            if scene
                .section
                .is_some_and(|sec| (0..3).all(|i| sec.removes(w(i))))
            {
                continue;
            }
            let v = [0, 1, 2].map(|i| cam.project(w(i), rect));
            if near.is_some_and(|zmax| v.iter().any(|q| q.1 > zmax)) {
                continue;
            }
            let q = v.map(|q| q.0);
            let touches = triangle_touches(area, q);
            let outside = !q.iter().all(|p| area.contains(*p));
            let p = Pick {
                feature: b.feature,
                face: m.face_names[*face as usize].clone(),
            };
            match faces.iter_mut().find(|f| f.0 == p) {
                Some(f) => {
                    f.1 |= touches;
                    f.2 |= outside;
                }
                None => faces.push((p, touches, outside)),
            }
        }
    }
    faces
}

/// 範囲で選ぶ面(U30)。`crossing`(右から左へ囲んだ)なら範囲にかかった面、そうでなければ見えている所が
/// 全部範囲に入った面。裏向きの三角形は数えず、手前の面に隠れた面は拾う。
pub fn picks_in(scene: &Scene, cam: &Camera, rect: Rect, area: Rect, crossing: bool) -> Vec<Pick> {
    face_coverage(scene, cam, rect, area)
        .into_iter()
        .filter(|(_, touches, outside)| if crossing { *touches } else { !*outside })
        .map(|f| f.0)
        .collect()
}

/// 範囲で選ぶボディ(U30)。`crossing` なら範囲にかかったボディ、そうでなければ見えている所が全部
/// 範囲に入ったボディ。
pub fn bodies_in(
    scene: &Scene,
    cam: &Camera,
    rect: Rect,
    area: Rect,
    crossing: bool,
) -> Vec<FeatureId> {
    let mut bodies: Vec<(FeatureId, bool, bool)> = Vec::new();
    for (p, touches, outside) in face_coverage(scene, cam, rect, area) {
        match bodies.iter_mut().find(|b| b.0 == p.feature) {
            Some(b) => {
                b.1 |= touches;
                b.2 |= outside;
            }
            None => bodies.push((p.feature, touches, outside)),
        }
    }
    bodies
        .into_iter()
        .filter(|(_, touches, outside)| if crossing { *touches } else { !*outside })
        .map(|b| b.0)
        .collect()
}

/// 面の点の平均(F で寄せる所)。
pub fn face_center(scene: &Scene, p: &Pick) -> Option<[f32; 3]> {
    let b = scene.bodies.iter().find(|b| b.feature == p.feature)?;
    let m = &b.mesh;
    let k = m.face_names.iter().position(|n| *n == p.face)? as u32;
    let mut sum = [0.0f32; 3];
    let mut n = 0.0;
    for (t, f) in m.triangles.iter().zip(&m.triangle_faces) {
        if *f == k {
            for &i in t {
                let q = m.positions[i as usize];
                for j in 0..3 {
                    sum[j] += q[j];
                }
                n += 1.0;
            }
        }
    }
    (n > 0.0).then(|| sum.map(|v| v / n))
}

/// 名前が `face` の面の外向きの法線(表示しているボディのどれかの面。最初の三角形で測る)。
pub fn face_normal(scene: &Scene, face: &str) -> Option<[f32; 3]> {
    scene.bodies.iter().find_map(|b| {
        let m = &b.mesh;
        let k = m.face_names.iter().position(|n| n == face)? as u32;
        let (t, _) = m
            .triangles
            .iter()
            .zip(&m.triangle_faces)
            .find(|(_, f)| **f == k)?;
        let w = |i: usize| m.positions[t[i] as usize];
        Some(normalize(cross(sub(w(1), w(0)), sub(w(2), w(0)))))
    })
}

/// ボディの外形(なければ `None`)。
pub fn body_bounds(scene: &Scene, feature: FeatureId) -> Option<([f32; 3], [f32; 3])> {
    let b = scene.bodies.iter().find(|b| b.feature == feature)?;
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for p in &b.mesh.positions {
        for i in 0..3 {
            min[i] = min[i].min(p[i]);
            max[i] = max[i].max(p[i]);
        }
    }
    (min[0] <= max[0]).then_some((min, max))
}
