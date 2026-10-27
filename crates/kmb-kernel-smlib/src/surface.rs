//! 厚みの無い面の作成と縁の検査。面の名前と縁の名前を独立に残す。
use super::*;
use k::{BodyKind, BoundaryCurve, BoundarySegment};

const GAP: f64 = 1e-4;
const NO_VOLUME: &str = "面のボディには体積がありません。厚みを付けると体積と重さが求まります";

impl SmlibKernel {
    pub(super) fn require_solid(
        &self,
        shape: ShapeHandle,
        operation: &str,
    ) -> Result<(), KernelError> {
        if self.body_kind(shape)? == BodyKind::Sheet {
            let message = if operation == "体積" {
                NO_VOLUME.into()
            } else {
                format!("面のボディは{operation}に使えません。先に厚みを付けてください")
            };
            return Err(KernelError::Failed(message));
        }
        Ok(())
    }
}

fn error(message: impl Into<String>) -> KernelError {
    KernelError::Failed(message.into())
}
fn distance(a: P3, b: P3) -> f64 {
    let d = k::sub(a, b);
    k::dot(d, d).sqrt()
}

/// 折れ線の交差・重なり。隣の点だけが接する場合は呼ぶ側で外す。
fn crossing(a: P3, b: P3, c: P3, d: P3) -> bool {
    let u = k::sub(b, a);
    let v = k::sub(d, c);
    let w = k::sub(a, c);
    let aa = k::dot(u, u);
    let bb = k::dot(u, v);
    let cc = k::dot(v, v);
    let dd = k::dot(u, w);
    let ee = k::dot(v, w);
    let det = aa * cc - bb * bb;
    if det > 1e-20 {
        let s = (bb * ee - cc * dd) / det;
        let t = (aa * ee - bb * dd) / det;
        if (0.0..=1.0).contains(&s) && (0.0..=1.0).contains(&t) {
            return distance(k::add(a, k::scale(u, s)), k::add(c, k::scale(v, t))) <= 1e-8;
        }
    }
    [a, b]
        .iter()
        .any(|p| polyline_distance(&[c, d], *p).0 <= 1e-8)
        || [c, d]
            .iter()
            .any(|p| polyline_distance(&[a, b], *p).0 <= 1e-8)
}

fn validate(lines: &[(String, Vec<P3>)]) -> Result<(), KernelError> {
    if lines.is_empty() || lines.len() > 32 {
        return Err(error(
            "縁は 1〜32 区間にしてください。長い輪郭は区間を減らしてください",
        ));
    }
    let mut pieces = Vec::new();
    for (i, (name, pts)) in lines.iter().enumerate() {
        if pts.len() < 2 || pts.iter().flatten().any(|v| !v.is_finite()) {
            return Err(error(format!(
                "縁 {name} の点が不正です。有限の座標で描き直してください"
            )));
        }
        if pts.windows(2).any(|w| distance(w[0], w[1]) <= 1e-9) {
            return Err(error(format!(
                "縁 {name} に長さ 0 の区間があります。重なった点を離してください"
            )));
        }
        let next = &lines[(i + 1) % lines.len()];
        if next.1.is_empty() || distance(*pts.last().expect("2 点以上"), next.1[0]) > GAP {
            return Err(error(format!(
                "縁 {name} の終わりと縁 {} の始まりがつながっていません。隙間を 0.0001 mm 以内にしてください",
                next.0
            )));
        }
        pieces.extend(pts.windows(2).map(|p| (name.as_str(), p[0], p[1])));
    }
    for i in 0..pieces.len() {
        for j in i + 1..pieces.len() {
            if j == i + 1 || (i == 0 && j + 1 == pieces.len()) {
                let (_, a, b) = pieces[i];
                let (_, c, d) = pieces[j];
                let u = k::sub(b, a);
                let v = k::sub(d, c);
                let cross = k::cross(u, v);
                if k::dot(cross, cross) < 1e-20 && k::dot(u, v) < 0.0 {
                    return Err(error(format!(
                        "縁 {} と縁 {} が折り返して重なっています。重なる区間を外してください",
                        pieces[i].0, pieces[j].0
                    )));
                }
                continue;
            }
            let (a, p, q) = pieces[i];
            let (b, r, s) = pieces[j];
            if crossing(p, q, r, s) {
                return Err(error(format!(
                    "縁 {a} と縁 {b} が交差または重なっています。交わらない 1 つの輪に描き直してください"
                )));
            }
        }
    }
    Ok(())
}

pub(super) fn planar(
    kernel: &SmlibKernel,
    profile: &Profile,
    tag: &str,
) -> Result<ShapeHandle, KernelError> {
    if profile.regions.is_empty() {
        return Err(error("閉じた輪郭がありません。縁を閉じてください"));
    }
    let loops = all_loops(profile);
    if loops.iter().map(|l| l.edges.len()).sum::<usize>() > 32 {
        return Err(error(
            "縁は 32 区間までにしてください。輪郭の区間を減らしてください",
        ));
    }
    let mut boundary: Vec<(String, Vec<P3>)> = Vec::new();
    for l in &loops {
        let lines: Vec<_> = l
            .edges
            .iter()
            .map(|e| {
                let mut pts = e.segment.polyline(64);
                pts.push(e.segment.end_point());
                if e.reversed {
                    pts.reverse();
                }
                (
                    e.name.clone(),
                    pts.into_iter().map(|p| profile.plane.to_world(p)).collect(),
                )
            })
            .collect();
        validate(&lines)?;
        for (name, points) in &lines {
            for (other, previous) in &boundary {
                if points.windows(2).any(|p| {
                    previous
                        .windows(2)
                        .any(|q| crossing(p[0], p[1], q[0], q[1]))
                }) {
                    return Err(error(format!(
                        "縁 {name} と縁 {other} が交差しています。穴と外側の輪郭を離してください"
                    )));
                }
            }
        }
        boundary.extend(lines);
    }
    let (segs, lens) = segments(&loops);
    let _g = ffi::lock();
    let brep = ffi::planar_sheet(&jc_plane(&profile.plane), &segs, &lens).map_err(|why| {
        error(format!(
            "平らな面を張れませんでした({why})。縁を交わらない輪に描き直してください"
        ))
    })?;
    put(kernel, brep, tag, "face", &boundary)
}

pub(super) fn patch(
    kernel: &SmlibKernel,
    boundary: &[BoundarySegment],
    tag: &str,
) -> Result<ShapeHandle, KernelError> {
    let mut lines = Vec::new();
    // 参照の解決は錠を取る前に行う。SMLib の呼び出しは再入できない。
    for segment in boundary {
        let pts = match &segment.curve {
            BoundaryCurve::Line { a, b } => vec![*a, *b],
            BoundaryCurve::Through(p) => p.clone(),
            BoundaryCurve::Arc { from, arc } => {
                let radius = k::sub(*from, arc.center);
                if !arc.radius.is_finite()
                    || arc.radius <= 0.0
                    || !arc.sweep.is_finite()
                    || arc.sweep.abs() < 1e-12
                    || (k::dot(arc.axis, arc.axis) - 1.0).abs() > 1e-6
                    || k::dot(radius, arc.axis).abs() > GAP
                    || (distance(*from, arc.center) - arc.radius).abs() > GAP
                {
                    return Err(error(format!(
                        "縁 {} の円弧が不正です。半径と始まりの点、軸、回る角度を確かめてください",
                        segment.name
                    )));
                }
                (0..=128)
                    .map(|i| arc.point(*from, arc.sweep * i as f64 / 128.0))
                    .collect()
            }
            BoundaryCurve::BodyEdge {
                shape,
                faces,
                point,
            } => {
                if kernel.imported(*shape).is_some() {
                    return Err(KernelError::Unsupported(
                        "別のカーネルで読み込んだボディの辺に面を張る".into(),
                    ));
                }
                let mut edges = kernel.edges(*shape)?;
                edges.extend(kernel.arc_edges(*shape)?);
                edges.extend(kernel.curve_edges(*shape)?);
                let same = |e: &EdgeInfo| {
                    e.faces == *faces || e.faces == [faces[1].clone(), faces[0].clone()]
                };
                let found = edges
                    .iter()
                    .filter(|e| same(e))
                    .min_by(|a, b| {
                        polyline_distance(&a.polyline(128), *point)
                            .0
                            .total_cmp(&polyline_distance(&b.polyline(128), *point).0)
                    })
                    .ok_or_else(|| {
                        error(format!(
                            "縁 {} の辺が見つかりません。辺を選び直してください",
                            segment.name
                        ))
                    })?;
                found.polyline(128)
            }
        };
        lines.push((segment.name.clone(), pts));
    }
    // ボディの辺は両側の面の組で指すため、向きは隣の区間との接続から決める。
    if lines.len() > 1 {
        if matches!(boundary[0].curve, BoundaryCurve::BodyEdge { .. }) {
            let next = &lines[1].1;
            if lines[0].1.len() >= 2 && next.len() >= 2 {
                let last = *lines[0].1.last().expect("2 点以上");
                if distance(last, next[0]).min(distance(last, *next.last().expect("2 点以上")))
                    > GAP
                {
                    lines[0].1.reverse();
                }
            }
        }
        for i in 1..lines.len() {
            if matches!(boundary[i].curve, BoundaryCurve::BodyEdge { .. })
                && !lines[i - 1].1.is_empty()
                && lines[i].1.len() >= 2
            {
                let last = *lines[i - 1].1.last().expect("前の区間");
                if distance(last, lines[i].1[0]) > GAP {
                    lines[i].1.reverse();
                }
            }
        }
    }
    validate(&lines)?;
    let _g = ffi::lock();
    let store = kernel.lock();
    let mut raw = Vec::new();
    for (segment, (_, pts)) in boundary.iter().zip(&lines) {
        let mut c = ffi::JcBoundary {
            kind: 0,
            count: pts.len() as u32,
            points: pts.as_ptr().cast(),
            brep: std::ptr::null(),
            f0: 0,
            f1: 0,
            center: [0.0; 3],
            axis: [0.0; 3],
            sweep: 0.0,
        };
        match &segment.curve {
            BoundaryCurve::Line { .. } => {}
            BoundaryCurve::Through(_) => c.kind = 1,
            BoundaryCurve::Arc { arc, .. } => {
                c.kind = 2;
                c.center = arc.center;
                c.axis = arc.axis;
                c.sweep = arc.sweep;
            }
            BoundaryCurve::BodyEdge {
                shape,
                faces,
                point,
            } => {
                let sh = store.get(*shape)?;
                c.kind = 3;
                c.brep = sh.brep.raw();
                c.center = *point;
                c.f0 = sh
                    .names
                    .iter()
                    .position(|n| n == &faces[0])
                    .ok_or_else(|| error("縁の面がありません。辺を選び直してください"))?
                    as u32;
                c.f1 = sh
                    .names
                    .iter()
                    .position(|n| n == &faces[1])
                    .map(|i| i as u32)
                    .or_else(|| {
                        sh.rims
                            .iter()
                            .any(|n| n.as_deref() == Some(faces[1].as_str()))
                            .then_some(u32::MAX)
                    })
                    .ok_or_else(|| error("縁の面がありません。辺を選び直してください"))?;
            }
        }
        raw.push(c);
    }
    let points: Vec<_> = lines.iter().flat_map(|(_, p)| p).copied().collect();
    let planar = points
        .iter()
        .skip(2)
        .find_map(|p| {
            let normal = k::cross(k::sub(points[1], points[0]), k::sub(*p, points[0]));
            let length = k::dot(normal, normal).sqrt();
            (length > 1e-9).then(|| {
                points
                    .iter()
                    .all(|q| k::dot(k::sub(*q, points[0]), normal).abs() / length < 1e-7)
            })
        })
        .unwrap_or(false);
    if !planar && boundary.len() < 3 {
        return Err(error(
            "立体的な縁は 3 区間以上必要です。縁を分けて選び直してください",
        ));
    }
    let brep = ffi::patch_sheet(&raw, planar).map_err(|why| {
        error(format!(
            "面を張れませんでした({why})。縁を滑らかにつなぎ、交わらない輪に描き直してください"
        ))
    })?;
    drop(store);
    put(kernel, brep, tag, "patch", &lines)
}

fn put(
    kernel: &SmlibKernel,
    brep: Brep,
    tag: &str,
    face: &str,
    lines: &[(String, Vec<P3>)],
) -> Result<ShapeHandle, KernelError> {
    if !brep.is_sheet().map_err(failed("ボディの種類"))? {
        return Err(error(
            "面のボディを作れませんでした。縁を描き直してください",
        ));
    }
    let faces = brep
        .faces(ffi::FACE_INNER_POINT)
        .map_err(failed("面の列挙"))?;
    if faces.is_empty() {
        return Err(error(
            "面を張れませんでした。囲む面積がある縁に描き直してください",
        ));
    }
    let mut names: Vec<_> = faces.iter().map(|_| format!("{tag}:{face}")).collect();
    let edges = brep.all_edges().map_err(failed("縁の列挙"))?;
    let mut rims = Vec::new();
    for e in &edges {
        let name = if e.faces[1] == u32::MAX as usize {
            let ends = |p: &[P3]| {
                distance(p[0], e.points[0])
                    + distance(
                        *p.last().expect("2 点以上"),
                        *e.points.last().expect("辺の点"),
                    )
            };
            let nearest = lines
                .iter()
                .min_by(|a, b| {
                    let score = |p: &[P3]| {
                        let mut rev = p.to_vec();
                        rev.reverse();
                        ends(p).min(ends(&rev))
                    };
                    score(&a.1).total_cmp(&score(&b.1))
                })
                .expect("縁は空でない");
            Some(format!("{tag}:rim:{}", nearest.0))
        } else {
            None
        };
        if face == "patch"
            && lines.len() >= 5
            && faces.len() == lines.len()
            && let Some(rim) = &name
        {
            names[e.faces[0]] = rim.replacen(":rim:", ":patch:", 1);
        }
        rims.push(name);
    }
    let names = dedupe_names(names, &points(&faces));
    let mut store = kernel.lock();
    let h = store.put(brep, names)?;
    store.shapes.get_mut(&h.0).expect("しまった形").rims = rims;
    Ok(h)
}

pub(super) fn sheet_edges(shape: &Shape) -> Result<Vec<EdgeInfo>, KernelError> {
    let edges = shape.brep.all_edges().map_err(failed("縁の列挙"))?;
    Ok(edges
        .into_iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let a = *e.points.first()?;
            let b = *e.points.last()?;
            let face = shape.names.get(e.faces[0])?.clone();
            let other = if e.faces[1] == u32::MAX as usize {
                shape.rims.get(i)?.clone()?
            } else {
                shape.names.get(e.faces[1])?.clone()
            };
            Some(EdgeInfo {
                faces: [face, other],
                a,
                b,
                convex: false,
                seam: Seam::Unknown,
                arc: None,
                curve: e.points,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rectangle() -> Profile {
        let pts = [[0., 0.], [10., 0.], [10., 10.], [0., 10.]];
        Profile {
            plane: k::Plane::XY,
            regions: vec![k::Region {
                outer: ProfileLoop {
                    edges: (0..4)
                        .map(|i| ProfileEdge {
                            name: format!("e{i}"),
                            segment: Segment::Line {
                                a: pts[i],
                                b: pts[(i + 1) % 4],
                            },
                            reversed: false,
                        })
                        .collect(),
                },
                holes: vec![],
            }],
        }
    }
    fn saddle(n: usize) -> Vec<BoundarySegment> {
        let pts: Vec<_> = if n == 4 {
            vec![[0., 0., 0.], [10., 0., 1.], [10., 10., 0.], [0., 10., 1.]]
        } else {
            (0..n)
                .map(|i| {
                    let a = std::f64::consts::TAU * i as f64 / n as f64;
                    [10. * a.cos(), 10. * a.sin(), a.cos() * a.sin()]
                })
                .collect()
        };
        (0..n)
            .map(|i| BoundarySegment {
                name: format!("e{i}"),
                curve: BoundaryCurve::Line {
                    a: pts[i],
                    b: pts[(i + 1) % n],
                },
            })
            .collect()
    }
    #[test]
    fn planar_sheet_has_area_rims_and_no_volume() {
        let k = SmlibKernel::new();
        let h = k.planar_sheet(&rectangle(), "f1").unwrap();
        assert_eq!(k.body_kind(h).unwrap(), BodyKind::Sheet);
        assert!((k.faces(h).unwrap().iter().map(|f| f.area).sum::<f64>() - 100.).abs() < 1e-7);
        assert_eq!(k.face_names(h).unwrap(), ["f1:face"]);
        let edges = k.curve_edges(h).unwrap();
        assert_eq!(edges.len(), 4);
        for e in edges {
            assert!(e.faces[1].starts_with("f1:rim:e"));
            assert_eq!(e.seam, Seam::Unknown);
            assert!(!e.convex);
        }
        assert!(k.mass(h).unwrap_err().to_string().contains(NO_VOLUME));
        assert!(k.volume(h).is_err());
        assert!(k.boolean(BooleanOp::Union, h, h).is_err());
        assert!(!k.mesh(h, 0.05).unwrap().triangles.is_empty());
        assert!(k.bounds(h).is_ok());
        assert!(k.export_step(&[h]).is_ok());
    }
    #[test]
    fn spatial_patch_preserves_boundary_and_has_sheet_area() {
        let k = SmlibKernel::new();
        let h = k.patch_sheet(&saddle(4), "f2").unwrap();
        assert_eq!(k.body_kind(h).unwrap(), BodyKind::Sheet);
        let area = k.faces(h).unwrap().iter().map(|f| f.area).sum::<f64>();
        assert!(area > 100. && area < 102., "{area}");
        let e = k.curve_edges(h).unwrap();
        assert_eq!(e.len(), 4);
        assert!(e.iter().all(|e| e.faces[1].starts_with("f2:rim:e")));
        for segment in saddle(4) {
            let BoundaryCurve::Line { a, b } = segment.curve else {
                unreachable!()
            };
            let edge = e
                .iter()
                .find(|e| e.faces[1] == format!("f2:rim:{}", segment.name))
                .unwrap();
            assert!(
                (distance(a, edge.a) + distance(b, edge.b))
                    .min(distance(a, edge.b) + distance(b, edge.a))
                    < 1e-4
            );
            assert!(
                edge.curve
                    .iter()
                    .all(|p| polyline_distance(&[a, b], *p).0 < 1e-4)
            );
        }
    }
    #[test]
    fn planar_five_sided_boundary_keeps_one_patch_name() {
        let kernel = SmlibKernel::new();
        let boundary: Vec<_> = (0..5)
            .map(|i| {
                let point = |i: usize| {
                    let angle = std::f64::consts::TAU * i as f64 / 5.;
                    [10. * angle.cos(), 10. * angle.sin(), 0.]
                };
                BoundarySegment {
                    name: format!("e{i}"),
                    curve: BoundaryCurve::Line {
                        a: point(i),
                        b: point((i + 1) % 5),
                    },
                }
            })
            .collect();
        let h = kernel.patch_sheet(&boundary, "planar").unwrap();
        assert_eq!(kernel.face_names(h).unwrap(), ["planar:patch"]);
        let names: HashSet<_> = kernel
            .curve_edges(h)
            .unwrap()
            .into_iter()
            .map(|e| e.faces[1].clone())
            .collect();
        assert_eq!(names, (0..5).map(|i| format!("planar:rim:e{i}")).collect());
    }

    #[test]
    fn five_sided_patch_stitches_faces_and_names_them_after_the_boundary() {
        let kernel = SmlibKernel::new();
        let h = kernel.patch_sheet(&saddle(5), "f3").unwrap();
        assert_eq!(kernel.body_kind(h).unwrap(), BodyKind::Sheet);
        let faces = kernel.faces(h).unwrap();
        assert_eq!(faces.len(), 5);
        assert!(faces.iter().map(|f| f.area).sum::<f64>() > 200.);
        assert!(faces.iter().all(|f| f.name.starts_with("f3:patch:e")));
        let rims = kernel
            .curve_edges(h)
            .unwrap()
            .into_iter()
            .filter(|e| e.faces[1].starts_with("f3:rim:"))
            .collect::<Vec<_>>();
        let names: HashSet<_> = rims.iter().map(|e| e.faces[1].clone()).collect();
        assert_eq!(names.len(), 5, "{rims:?}");
    }
    #[test]
    fn three_curved_boundaries_form_a_sheet_and_arc_boundaries_remain_round() {
        let kernel = SmlibKernel::new();
        let a = [0., 0., 0.];
        let b = [10., 0., 0.];
        let c = [0., 10., 0.];
        let boundary = vec![
            BoundarySegment {
                name: "a".into(),
                curve: BoundaryCurve::Through(vec![a, [5., -1., 2.], b]),
            },
            BoundarySegment {
                name: "b".into(),
                curve: BoundaryCurve::Line { a: b, b: c },
            },
            BoundarySegment {
                name: "c".into(),
                curve: BoundaryCurve::Line { a: c, b: a },
            },
        ];
        let h = kernel.patch_sheet(&boundary, "three").unwrap();
        assert_eq!(kernel.body_kind(h).unwrap(), BodyKind::Sheet);
        assert!(kernel.faces(h).unwrap().iter().map(|f| f.area).sum::<f64>() > 50.);
        let arc = k::ArcEdge {
            center: [0.; 3],
            axis: [0., 0., 1.],
            radius: 5.,
            sweep: std::f64::consts::TAU,
            inside: true,
        };
        let h = kernel
            .patch_sheet(
                &[BoundarySegment {
                    name: "circle".into(),
                    curve: BoundaryCurve::Arc {
                        from: [5., 0., 0.],
                        arc,
                    },
                }],
                "round",
            )
            .unwrap();
        let faces = kernel.faces(h).unwrap();
        let area: f64 = faces.iter().map(|f| f.area).sum();
        // 面積の問い合わせは相対精度 1e-4 の数値積分で求める。
        assert!(
            (area - std::f64::consts::PI * 25.).abs() < std::f64::consts::PI * 25. * 1e-4,
            "面積 {area}, {faces:?}"
        );
    }
    #[test]
    fn sheets_keep_holes_names_and_kind_when_moved_and_refuse_solid_operations() {
        let kernel = SmlibKernel::new();
        let mut profile = rectangle();
        profile.regions[0].holes.push(ProfileLoop {
            edges: vec![ProfileEdge {
                name: "hole".into(),
                segment: Segment::Circle {
                    center: [5., 5.],
                    radius: 2.,
                },
                reversed: true,
            }],
        });
        let h = kernel.planar_sheet(&profile, "sheet").unwrap();
        let faces = kernel.faces(h).unwrap();
        let area: f64 = faces.iter().map(|f| f.area).sum();
        // 面積の問い合わせは相対精度 1e-4 の数値積分で求める。
        assert!(
            (area - (100. - std::f64::consts::PI * 4.)).abs()
                < (100. - std::f64::consts::PI * 4.) * 1e-4,
            "面積 {area}, {faces:?}"
        );
        let m = k::translation([1., 2., 3.]);
        let moved = kernel.transform(h, &m, "@copy").unwrap();
        assert_eq!(kernel.body_kind(moved).unwrap(), BodyKind::Sheet);
        assert!(
            kernel
                .curve_edges(moved)
                .unwrap()
                .iter()
                .all(|e| e.faces[1].ends_with("@copy"))
        );
        let instance = kernel.instance(h, &m).unwrap();
        assert_eq!(kernel.instance_of(instance), Some((h, m)));
        assert_eq!(kernel.body_kind(instance).unwrap(), BodyKind::Sheet);
        assert_eq!(
            kernel.face_names(instance).unwrap(),
            kernel.face_names(h).unwrap()
        );
        assert_eq!(
            kernel.bounds(instance).unwrap(),
            kernel.bounds(moved).unwrap()
        );
        assert_eq!(
            kernel
                .curve_edges(instance)
                .unwrap()
                .iter()
                .map(|e| e.faces.clone())
                .collect::<Vec<_>>(),
            kernel
                .curve_edges(h)
                .unwrap()
                .iter()
                .map(|e| e.faces.clone())
                .collect::<Vec<_>>()
        );
        assert!(
            kernel
                .volume(instance)
                .unwrap_err()
                .to_string()
                .contains(NO_VOLUME)
        );
        let transformed_instance = kernel.transform(h, &m, "").unwrap();
        assert_eq!(
            kernel.body_kind(transformed_instance).unwrap(),
            BodyKind::Sheet
        );
        assert_eq!(kernel.instance_of(transformed_instance), Some((h, m)));
        assert_eq!(
            kernel.curve_edges(transformed_instance).unwrap(),
            kernel.curve_edges(instance).unwrap()
        );
        assert!(
            kernel
                .shell(h, &[], 1., "shell")
                .unwrap_err()
                .to_string()
                .contains("先に厚み")
        );
        assert!(
            kernel
                .blend_edges(h, &[], "blend")
                .unwrap_err()
                .to_string()
                .contains("先に厚み")
        );
        assert!(
            kernel
                .offset_faces(h, &[], 1., "offset")
                .unwrap_err()
                .to_string()
                .contains("先に厚み")
        );
        let step = kernel.export_step(&[h, moved]).unwrap();
        assert!(step.contains("SHELL_BASED_SURFACE_MODEL"));
        assert!(step.contains("OPEN_SHELL"));
        assert!(!step.contains("MANIFOLD_SOLID_BREP"));
    }

    #[test]
    fn open_crossed_and_zero_length_boundaries_are_rejected() {
        let k = SmlibKernel::new();
        let mut lines = saddle(4);
        lines.pop();
        assert!(
            k.patch_sheet(&lines, "bad")
                .unwrap_err()
                .to_string()
                .contains("つながっていません")
        );
        let p = [[0., 0., 0.], [10., 10., 0.], [0., 10., 0.], [10., 0., 0.]];
        let crossed: Vec<_> = (0..4)
            .map(|i| BoundarySegment {
                name: format!("e{i}"),
                curve: BoundaryCurve::Line {
                    a: p[i],
                    b: p[(i + 1) % 4],
                },
            })
            .collect();
        assert!(
            k.patch_sheet(&crossed, "bad")
                .unwrap_err()
                .to_string()
                .contains("交差")
        );
        let mut lines = saddle(4);
        lines[0].curve = BoundaryCurve::Line { a: p[0], b: p[0] };
        assert!(
            k.patch_sheet(&lines, "bad")
                .unwrap_err()
                .to_string()
                .contains("長さ 0")
        );
    }
}
