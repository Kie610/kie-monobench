//! ボディの平らな面の縁を、押し出しの輪郭([`Profile`])にする。
//!
//! 縁はカーネルの辺の一覧([`Kernel::edges`] と [`Kernel::arc_edges`])から直線と円弧で作る。
//! それで閉じない面(縁に自由な曲線がある、など)は、網の縁の折れ線で近い形にする。

use jc_core::Profile;
use jc_core::kernel::{
    EdgeInfo, Kernel, P2, P3, Plane, ProfileEdge, ProfileLoop, Region, Segment, ShapeHandle,
    canonical_loop, dot, natural_key, polygon_area, sub,
};
use std::collections::HashMap;

/// 面の縁から作った輪郭。`approximated` は網の折れ線で近い形にしたか。
#[derive(Debug, Clone, PartialEq)]
pub struct FaceProfile {
    pub profile: Profile,
    pub approximated: bool,
}

/// ボディ `shape` の平らな面 `faces`(どれも同じ平面)の縁の輪郭。平面の法線は面の外向き
/// (正の距離で面の外へ押し出す)。面ごとに 1 つの領域(外周と穴)になる。
pub fn face_profile(
    kernel: &dyn Kernel,
    shape: ShapeHandle,
    faces: &[String],
) -> Result<FaceProfile, String> {
    let first = faces.first().ok_or("押し出す面を選んでください")?;
    let infos = kernel.faces(shape).map_err(|e| e.to_string())?;
    let planar = |name: &str| {
        let f = infos.iter().find(|f| f.name == name).ok_or_else(|| {
            format!(
                "面 {name} が見つかりません(形が変わって面が無くなった時は、選び直してください)"
            )
        })?;
        f.planar
            .ok_or_else(|| format!("面 {name} は平らではありません(平らな面だけ押し出せます)"))
    };
    let p0 = planar(first)?;
    let plane = Plane::from_point_normal(p0.point, p0.normal);
    let n = plane.normal();
    for name in &faces[1..] {
        let p = planar(name)?;
        let len = dot(p.normal, p.normal).sqrt().max(1e-12);
        if dot(p.normal, n) / len < 1.0 - 1e-6 || dot(sub(p.point, plane.origin), n).abs() > 1e-6 {
            return Err(
                "選んだ面が同じ平面にありません(押し出す面は 1 つの平面に揃えてください)".into(),
            );
        }
    }
    let mut edges = kernel.edges(shape).unwrap_or_default();
    edges.extend(kernel.arc_edges(shape).unwrap_or_default());
    let mut regions = Vec::new();
    let mut approximated = false;
    for name in faces {
        let area = infos.iter().find(|f| &f.name == name).map(|f| f.area);
        // 辺の一覧に無い縁(丸めた面との境目の円など)があると、穴が抜けた輪になる。
        // 面積が面と合わない時は、網の縁にだけある輪を足す。それでも合わなければ網の縁で作る
        let exact = edge_loops(&edges, name, &plane).and_then(|l| match area {
            Some(a) if !covers(&l, a) => fill_missing(l, kernel, shape, name, &plane, a),
            _ => Some(l),
        });
        let loops = match exact {
            Some(l) => l,
            None => {
                approximated = true;
                mesh_loops(kernel, shape, name, &plane)
                    .ok_or_else(|| format!("面 {name} の縁をたどれませんでした"))?
            }
        };
        regions.push(region_of(loops).ok_or_else(|| format!("面 {name} の縁が閉じていません"))?);
    }
    Ok(FaceProfile {
        profile: Profile { plane, regions },
        approximated,
    })
}

/// 外周(囲む面積がいちばん大きい輪)と穴に分け、向きを揃える。
fn region_of(mut loops: Vec<ProfileLoop>) -> Option<Region> {
    let i = (0..loops.len()).max_by(|&a, &b| {
        loops[a]
            .signed_area()
            .abs()
            .total_cmp(&loops[b].signed_area().abs())
    })?;
    let outer = canonical_loop(loops.remove(i), true);
    let mut holes: Vec<_> = loops
        .into_iter()
        .map(|l| canonical_loop(l, false))
        .collect();
    holes.sort_by(|a, b| natural_key(&a.edges[0].name).cmp(&natural_key(&b.edges[0].name)));
    Some(Region { outer, holes })
}

/// 輪 `loops`(外周と穴)が囲む面積が、面の面積 `area` と合うか。
fn covers(loops: &[ProfileLoop], area: f64) -> bool {
    // 円弧は細かく刻む(64 分割では円の面積が 0.2% ほど小さく出る)
    let areas: Vec<f64> = loops
        .iter()
        .map(|l| {
            let pts: Vec<P2> = l.edges.iter().flat_map(|e| e.polyline(1024)).collect();
            polygon_area(&pts).abs()
        })
        .collect();
    let outer = areas.iter().copied().fold(0.0, f64::max);
    let inside = 2.0 * outer - areas.iter().sum::<f64>();
    (inside - area).abs() <= 1e-3 * area.max(1.0)
}

/// 辺の一覧から作った輪 `exact` に、網の縁にだけある輪を足す。円の輪は正確な円にし、
/// それ以外は折れ線にする。足しても面積が合わなければ `None`(全体を網の縁で作る)。
fn fill_missing(
    mut exact: Vec<ProfileLoop>,
    kernel: &dyn Kernel,
    shape: ShapeHandle,
    face: &str,
    plane: &Plane,
    area: f64,
) -> Option<Vec<ProfileLoop>> {
    let mesh = mesh_loops(kernel, shape, face, plane)?;
    let known: Vec<(P2, f64)> = exact.iter().map(|l| shape_of(&l.polyline())).collect();
    for (k, l) in mesh.into_iter().enumerate() {
        let pts = l.polyline();
        let (c, a) = shape_of(&pts);
        let size = a.abs().sqrt().max(1e-6);
        let same = |&(c2, a2): &(P2, f64)| {
            (c[0] - c2[0]).hypot(c[1] - c2[1]) < 0.05 * size
                && (a.abs() - a2.abs()).abs() < 0.05 * a.abs()
        };
        if known.iter().any(same) {
            continue;
        }
        let d: Vec<f64> = pts
            .iter()
            .map(|p| (p[0] - c[0]).hypot(p[1] - c[1]))
            .collect();
        let r = d.iter().sum::<f64>() / d.len().max(1) as f64;
        let round = d.len() >= 8 && d.iter().all(|x| (x - r).abs() < 1e-3 * r.max(1.0));
        exact.push(if round {
            ProfileLoop {
                edges: vec![ProfileEdge {
                    name: format!("c{k}"),
                    segment: Segment::Circle {
                        center: c,
                        // 網の点は円の上にあり、弦の中ほどが内側へ寄るので、点までの距離を使う
                        radius: d.iter().copied().fold(0.0, f64::max),
                    },
                    reversed: false,
                }],
            }
        } else {
            l
        });
    }
    covers(&exact, area).then_some(exact)
}

/// 折れ線の輪の重心と符号つきの面積。
fn shape_of(pts: &[P2]) -> (P2, f64) {
    let n = pts.len();
    let (mut a, mut cx, mut cy) = (0.0, 0.0, 0.0);
    for i in 0..n {
        let (p, q) = (pts[i], pts[(i + 1) % n]);
        let w = p[0] * q[1] - q[0] * p[1];
        a += w;
        cx += (p[0] + q[0]) * w;
        cy += (p[1] + q[1]) * w;
    }
    if a.abs() < 1e-12 {
        return (pts.first().copied().unwrap_or([0.0, 0.0]), 0.0);
    }
    ([cx / (3.0 * a), cy / (3.0 * a)], a / 2.0)
}

/// 区間の端が同じ点か(面の大きさに対して十分小さい隔たり)。
fn near(a: P2, b: P2) -> bool {
    (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-5
}

/// 辺の一覧から、面 `face` の縁を直線と円弧の輪にする。閉じなければ `None`。
fn edge_loops(edges: &[EdgeInfo], face: &str, plane: &Plane) -> Option<Vec<ProfileLoop>> {
    let n = plane.normal();
    // 区間の名前は、隣の面の名前の順で決める(形を計算し直しても同じ並びになる)
    let mut mine: Vec<(&EdgeInfo, &str)> = edges
        .iter()
        .filter_map(|e| {
            let other = match &e.faces {
                [a, b] if a == face => b,
                [a, b] if b == face => a,
                _ => return None,
            };
            Some((e, other.as_str()))
        })
        .collect();
    if mine.is_empty() {
        return None;
    }
    mine.sort_by(|a, b| {
        natural_key(a.1)
            .cmp(&natural_key(b.1))
            .then(a.0.a[0].total_cmp(&b.0.a[0]))
            .then(a.0.a[1].total_cmp(&b.0.a[1]))
            .then(a.0.a[2].total_cmp(&b.0.a[2]))
    });
    let local = |p: P3| plane.to_local(p);
    let mut segs: Vec<ProfileEdge> = Vec::new();
    for (k, (e, _)) in mine.iter().enumerate() {
        let segment = match e.arc {
            None => Segment::Line {
                a: local(e.a),
                b: local(e.b),
            },
            Some(arc) => {
                let c = local(arc.center);
                let ang = |p: P3| {
                    let q = local(p);
                    (q[1] - c[1]).atan2(q[0] - c[0])
                };
                // 平面の法線と円の軸が逆なら、平面の上では回る向きが逆
                let sweep = if dot(arc.axis, n) < 0.0 {
                    -arc.sweep
                } else {
                    arc.sweep
                };
                if (sweep.abs() - std::f64::consts::TAU).abs() < 1e-6 {
                    Segment::Circle {
                        center: c,
                        radius: arc.radius,
                    }
                } else if sweep > 0.0 {
                    Segment::Arc {
                        center: c,
                        radius: arc.radius,
                        start: ang(e.a),
                        end: ang(e.a) + sweep,
                    }
                } else {
                    Segment::Arc {
                        center: c,
                        radius: arc.radius,
                        start: ang(e.b),
                        end: ang(e.b) - sweep,
                    }
                }
            }
        };
        segs.push(ProfileEdge {
            name: format!("e{k}"),
            segment,
            reversed: false,
        });
    }
    chain(segs)
}

/// 区間をつないで閉じた輪にする。つながらない区間が残れば `None`。
fn chain(segs: Vec<ProfileEdge>) -> Option<Vec<ProfileLoop>> {
    let mut left: Vec<Option<ProfileEdge>> = segs.into_iter().map(Some).collect();
    let mut loops = Vec::new();
    while let Some(i) = left.iter().position(Option::is_some) {
        let first = left[i].take()?;
        if matches!(first.segment, Segment::Circle { .. }) {
            loops.push(ProfileLoop { edges: vec![first] });
            continue;
        }
        let start = first.from_point();
        let mut end = first.to_point();
        let mut edges = vec![first];
        while !near(end, start) {
            let j = left.iter().position(|s| {
                s.as_ref().is_some_and(|s| {
                    !matches!(s.segment, Segment::Circle { .. })
                        && (near(s.segment.start_point(), end) || near(s.segment.end_point(), end))
                })
            })?;
            let mut s = left[j].take()?;
            s.reversed = !near(s.segment.start_point(), end);
            end = s.to_point();
            edges.push(s);
        }
        loops.push(ProfileLoop { edges });
    }
    Some(loops)
}

/// 網の三角形のうち、面 `face` の 1 つの三角形にしか使われない辺(面の縁)を折れ線の輪にする。
fn mesh_loops(
    kernel: &dyn Kernel,
    shape: ShapeHandle,
    face: &str,
    plane: &Plane,
) -> Option<Vec<ProfileLoop>> {
    let mesh = kernel.mesh(shape, 0.01).ok()?;
    let fi = mesh.face_names.iter().position(|n| n == face)? as u32;
    type Key = (i64, i64, i64);
    let key = |p: [f32; 3]| -> Key {
        let r = |v: f32| (v as f64 * 1e4).round() as i64;
        (r(p[0]), r(p[1]), r(p[2]))
    };
    let mut uses: HashMap<(Key, Key), (P3, P3, i32)> = HashMap::new();
    for (t, _) in mesh
        .triangles
        .iter()
        .zip(&mesh.triangle_faces)
        .filter(|(_, f)| **f == fi)
    {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let (pa, pb) = (mesh.positions[a as usize], mesh.positions[b as usize]);
            let (ka, kb) = (key(pa), key(pb));
            let k = if ka <= kb { (ka, kb) } else { (kb, ka) };
            let f = |p: [f32; 3]| p.map(f64::from);
            uses.entry(k).or_insert((f(pa), f(pb), 0)).2 += 1;
        }
    }
    let mut lines: Vec<_> = uses.into_values().filter(|u| u.2 == 1).collect();
    // 網の順は計算ごとに変わり得るので、位置の順に並べてから名前を付ける
    lines.sort_by(|a, b| {
        a.0.iter()
            .chain(&a.1)
            .zip(b.0.iter().chain(&b.1))
            .map(|(x, y)| x.total_cmp(y))
            .find(|o| o.is_ne())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let segs = lines
        .into_iter()
        .enumerate()
        .map(|(k, (a, b, _))| ProfileEdge {
            name: format!("m{k}"),
            segment: Segment::Line {
                a: plane.to_local(a),
                b: plane.to_local(b),
            },
            reversed: false,
        })
        .collect();
    chain(segs)
}
