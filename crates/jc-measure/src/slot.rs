//! 長穴(両端が半円の形)を、選んだ円柱の面か円弧の辺から見つける。

use super::*;

/// 半分の円の候補(円柱の面か円弧の辺)。
#[derive(Clone, Copy, Debug)]
pub(crate) struct Round {
    center: P,
    axis: P,
    radius: f64,
    sweep: f64,
    bulge: P,
    /// 軸の向きの長さ(辺なら 0)
    length: f64,
}

/// 円弧の辺の「軸から弧の真ん中へ」の向き。
pub(crate) fn arc_bulge(c: &Circle, start: P) -> P {
    let r0 = normalize(sub(start, c.center));
    let (sn, cs) = (c.sweep / 2.0).sin_cos();
    normalize(add(scale(r0, cs), scale(cross(c.normal, r0), sn)))
}

/// 同じボディの、選んだ物と同じ種類の丸い物(面なら円柱の面、辺なら円弧)。
pub(crate) fn rounds_of(scene: &(impl Meshes + ?Sized), t: &Target) -> Option<(Round, Vec<Round>)> {
    match t {
        Target::Face(p) => {
            let m = scene.mesh(p.feature)?;
            let mut picked = None;
            let mut all = Vec::new();
            for (k, name) in m.face_names.iter().enumerate() {
                let tris: Vec<[P; 3]> = m
                    .triangles
                    .iter()
                    .zip(&m.triangle_faces)
                    .filter(|(_, f)| **f == k as u32)
                    .map(|(t, _)| t.map(|i| f64s(m.positions[i as usize])))
                    .collect();
                let Some(c) = fit_cylinder(&tris) else {
                    continue;
                };
                if c.closed {
                    continue;
                }
                let r = Round {
                    center: c.center,
                    axis: c.axis,
                    radius: c.radius,
                    sweep: c.sweep,
                    bulge: c.bulge,
                    length: c.length,
                };
                if *name == p.face {
                    picked = Some(r);
                }
                all.push(r);
            }
            Some((picked?, all))
        }
        Target::Edge { feature, edge } => {
            let m = scene.mesh(*feature)?;
            let mut picked = None;
            let mut all = Vec::new();
            for (i, line) in m.edges.iter().enumerate() {
                let pts: Vec<P> = line.iter().map(|p| f64s(*p)).collect();
                let Some(c) = fit_circle(&pts) else { continue };
                if c.closed {
                    continue;
                }
                let r = Round {
                    center: c.center,
                    axis: c.normal,
                    radius: c.radius,
                    sweep: c.sweep,
                    bulge: arc_bulge(&c, pts[0]),
                    length: 0.0,
                };
                if i == *edge {
                    picked = Some(r);
                }
                all.push(r);
            }
            Some((picked?, all))
        }
        _ => None,
    }
}

/// 選んだ物が長穴の端(半円の円柱の面か円弧の辺)なら、向かい合う端を同じボディから探して長穴の寸法を出す。
/// 端の半円が 2 つに分かれていても、同じ軸のものをまとめて半円と見る。
pub fn slot(scene: &(impl Meshes + ?Sized), t: &Target) -> Option<Slot> {
    let (me, all) = rounds_of(scene, t)?;
    let tol = 5e-4 * (1.0 + me.radius) + 2e-6 * len(me.center);
    let same_size =
        |r: &Round| (r.radius - me.radius).abs() < tol && dot(r.axis, me.axis).abs() > 1.0 - 1e-6;
    // 軸に垂直な向きのずれと、軸の向きのずれ
    let offset = |r: &Round, from: P| {
        let d = sub(r.center, from);
        let along = dot(d, me.axis);
        (sub(d, scale(me.axis, along)), along)
    };
    // 同じ軸の物をまとめた半円(回った角度の和と、膨らむ向きの平均)
    let group = |at: P| {
        let mut sweep = 0.0;
        let mut bulge = [0.0; 3];
        for r in all.iter().filter(|r| same_size(r)) {
            let (perp, along) = offset(r, at);
            let same_plane = me.length > 0.0 || along.abs() < tol;
            if len(perp) < tol && same_plane {
                sweep += r.sweep;
                bulge = add(bulge, scale(r.bulge, r.sweep));
            }
        }
        (sweep, normalize(bulge))
    };
    let half = |sweep: f64| (sweep - std::f64::consts::PI).abs() < 0.05;
    let (sweep0, bulge0) = group(me.center);
    if !half(sweep0) {
        return None;
    }
    let mut best: Option<(f64, P)> = None;
    for r in all.iter().filter(|r| same_size(r)) {
        let (perp, along) = offset(r, me.center);
        let d = len(perp);
        // 向かい合う端は、同じ平面(辺)か、高さが重なる所(面)にある
        let level = if me.length > 0.0 {
            along.abs() < (me.length + r.length) / 2.0
        } else {
            along.abs() < tol
        };
        if d < tol || !level || best.is_some_and(|b| b.0 <= d) {
            continue;
        }
        let dir = scale(perp, 1.0 / d);
        let other = add(me.center, perp);
        let (sweep1, bulge1) = group(other);
        if half(sweep1) && dot(bulge0, dir) < -0.99 && dot(bulge1, dir) > 0.99 {
            best = Some((d, other));
        }
    }
    let (_, other) = best?;
    Some(Slot {
        width: 2.0 * me.radius,
        centers: (me.center, other),
    })
}
