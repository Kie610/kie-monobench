//! 円の縁・面積・網の点と法線を確かめた、全周の円筒どうしの面の一致。

use super::PLANE_GAP;
use kmb_core::kernel::{
    EdgeInfo, FaceInfo, Kernel, KernelError, Mesh, P3, add, dot, normalize, scale, sub,
};
use kmb_core::{ShapeHandle, TableMark};
use std::f64::consts::TAU;

#[derive(Clone)]
struct Cylinder {
    face: String,
    origin: P3,
    axis: P3,
    radius: f64,
    length: f64,
    inside: bool,
}

pub(super) struct Pair {
    a: Cylinder,
    b: Cylinder,
    lo: f64,
    hi: f64,
}

fn norm(v: P3) -> f64 {
    dot(v, v).sqrt()
}

fn radial(v: P3, axis: P3) -> P3 {
    sub(v, scale(axis, dot(v, axis)))
}

/// 端の円だけでは、途中でくびれた面も候補になる。全面積も円筒の値と合う物に絞る。
fn cylinders(faces: &[FaceInfo], edges: &[EdgeInfo]) -> Vec<Cylinder> {
    faces
        .iter()
        .filter(|f| f.planar.is_none())
        .filter_map(|face| {
            let ends: Vec<_> = edges.iter().filter(|e| e.faces[1] == face.name).collect();
            let [a, b] = ends.as_slice() else { return None };
            let (x, y) = (a.arc?, b.arc?);
            if ![x, y].into_iter().all(|e| {
                e.center
                    .into_iter()
                    .chain(e.axis)
                    .chain([e.radius, e.sweep])
                    .all(f64::is_finite)
            }) {
                return None;
            }
            let axis = normalize(x.axis);
            let delta = sub(y.center, x.center);
            let distance = dot(delta, axis);
            let length = distance.abs();
            if !x.radius.is_finite()
                || x.radius <= PLANE_GAP
                || !length.is_finite()
                || length <= PLANE_GAP
                || (x.sweep.abs() - TAU).abs() > 1e-6
                || (y.sweep.abs() - TAU).abs() > 1e-6
                || (x.radius - y.radius).abs() > PLANE_GAP
                || dot(axis, normalize(y.axis)).abs() < 1.0 - 1e-9
                || norm(radial(delta, axis)) > PLANE_GAP
                || x.inside != y.inside
            {
                return None;
            }
            let area = TAU * x.radius * length;
            // 面積は数値積分の近似値なので、半径の検査は網の点と法線でも行う。
            if !face.area.is_finite() || (face.area - area).abs() > area * 1e-4 + PLANE_GAP {
                return None;
            }
            Some(Cylinder {
                face: face.name.clone(),
                origin: if distance >= 0.0 { x.center } else { y.center },
                axis,
                radius: x.radius,
                length,
                inside: x.inside,
            })
        })
        .collect()
}

fn arcs(kernel: &dyn Kernel, shape: ShapeHandle) -> Result<Vec<EdgeInfo>, KernelError> {
    match kernel.arc_edges(shape) {
        // 円の情報を持たない形は平面の接触と、呼び出し元のブーリアンの経路で調べる。
        Err(KernelError::Unsupported(_)) => Ok(vec![]),
        result => result,
    }
}

pub(super) fn pairs(
    kernel: &dyn Kernel,
    a: ShapeHandle,
    b: ShapeHandle,
    fa: &[FaceInfo],
    fb: &[FaceInfo],
) -> Result<Vec<Pair>, KernelError> {
    if !fa.iter().any(|f| f.planar.is_none()) || !fb.iter().any(|f| f.planar.is_none()) {
        return Ok(vec![]);
    }
    let a = cylinders(fa, &arcs(kernel, a)?);
    let b = cylinders(fb, &arcs(kernel, b)?);
    let mut pairs = Vec::new();
    for x in a {
        for y in &b {
            let offset = sub(y.origin, x.origin);
            let parallel = dot(x.axis, y.axis);
            if x.inside == y.inside
                || (x.radius - y.radius).abs() > PLANE_GAP
                || parallel.abs() < 1.0 - 1e-9
                || norm(radial(offset, x.axis)) > PLANE_GAP
            {
                continue;
            }
            let from = dot(offset, x.axis);
            let to = from + parallel * y.length;
            let lo = from.min(to).max(0.0);
            let hi = from.max(to).min(x.length);
            // 僅かな傾きも長さに沿って広がるため、重なる両端で軸線の隔たりを確かめる。
            let same_line = |t| {
                let on_y = add(offset, scale(y.axis, (t - from) / parallel));
                norm(radial(on_y, x.axis)) <= PLANE_GAP
            };
            if hi - lo > PLANE_GAP && same_line(lo) && same_line(hi) {
                pairs.push(Pair {
                    a: x.clone(),
                    b: y.clone(),
                    lo,
                    hi,
                });
            }
        }
    }
    Ok(pairs)
}

/// 円筒の候補面の全三角形で半径・軸方向の範囲と外向き法線を確かめる。
/// 端の円が同じでも、途中で細くなる面や、面の情報と網が合わない物は採らない。
fn verified(mesh: &Mesh, cylinder: &Cylinder) -> bool {
    if mesh.triangles.len() != mesh.triangle_faces.len() {
        return false;
    }
    let Some(face) = mesh.face_names.iter().position(|f| f == &cylinder.face) else {
        return false;
    };
    let mut seen = false;
    for (triangle, &owner) in mesh.triangles.iter().zip(&mesh.triangle_faces) {
        if owner as usize != face {
            continue;
        }
        seen = true;
        for &index in triangle {
            let (Some(p), Some(n)) = (
                mesh.positions.get(index as usize),
                mesh.normals.get(index as usize),
            ) else {
                return false;
            };
            let delta = sub(p.map(f64::from), cylinder.origin);
            let along = dot(delta, cylinder.axis);
            let r = radial(delta, cylinder.axis);
            let sign = if cylinder.inside { 1.0 } else { -1.0 };
            let n = n.map(f64::from);
            if !along.is_finite()
                || along < -PLANE_GAP
                || along > cylinder.length + PLANE_GAP
                || (norm(r) - cylinder.radius).abs() > PLANE_GAP
                || !norm(n).is_finite()
                || norm(n) < 1e-9
                || dot(normalize(n), normalize(r)) * sign < 1.0 - 1e-5
            {
                return false;
            }
        }
    }
    seen
}

fn clipped(mut poly: Vec<P3>, c: &Cylinder, lo: f64, hi: f64) -> Vec<P3> {
    for (at, sign) in [(lo, 1.0), (hi, -1.0)] {
        let side = |p| (dot(sub(p, c.origin), c.axis) - at) * sign;
        let mut out = Vec::new();
        for i in 0..poly.len() {
            let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
            let (sp, sq) = (side(p), side(q));
            if sp >= 0.0 {
                out.push(p);
            }
            if (sp >= 0.0) != (sq >= 0.0) {
                out.push(add(p, scale(sub(q, p), sp / (sp - sq))));
            }
        }
        poly = out;
        if poly.len() < 3 {
            break;
        }
    }
    poly
}

pub(super) fn contact(pairs: &[Pair], a: &Mesh, b: &Mesh) -> (TableMark, f64) {
    let mut mark = TableMark::default();
    let mut area = 0.0;
    for pair in pairs {
        if !verified(a, &pair.a) || !verified(b, &pair.b) {
            continue;
        }
        let face = a
            .face_names
            .iter()
            .position(|f| f == &pair.a.face)
            .expect("検査済みの面");
        for (triangle, &owner) in a.triangles.iter().zip(&a.triangle_faces) {
            if owner as usize != face {
                continue;
            }
            let poly = clipped(
                triangle
                    .map(|i| a.positions[i as usize].map(f64::from))
                    .to_vec(),
                &pair.a,
                pair.lo,
                pair.hi,
            );
            for i in 1..poly.len().saturating_sub(1) {
                let [p, q, r] = [poly[0], poly[i], poly[i + 1]].map(|p| p.map(|v| v as f32));
                mark.tris.push([p, q, r]);
                mark.tris.push([p, r, q]);
            }
        }
        area += TAU * pair.a.radius * (pair.hi - pair.lo);
    }
    (mark, area)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kmb_core::kernel::{ArcEdge, Seam, cross};

    /// 面の候補判定だけに必要な、直線の軸と両端の円を返す。
    struct Ends {
        origin: P3,
        axis: P3,
        length: f64,
    }

    impl Kernel for Ends {
        fn name(&self) -> &str {
            "試験の円筒の辺"
        }

        fn arc_edges(&self, h: ShapeHandle) -> Result<Vec<EdgeInfo>, KernelError> {
            let (origin, axis, length) = if h.0 == 0 {
                ([0.0; 3], [0.0, 0.0, 1.0], 10.0)
            } else {
                (self.origin, self.axis, self.length)
            };
            let r = scale(normalize(cross(axis, [0.0, 1.0, 0.0])), 2.5);
            Ok([(length, 1.0), (0.0, -1.0)]
                .into_iter()
                .map(|(z, sign)| {
                    let center = add(origin, scale(axis, z));
                    let point = add(center, r);
                    EdgeInfo {
                        faces: [format!("cap{z}"), "side".into()],
                        a: point,
                        b: point,
                        convex: true,
                        seam: Seam::Corner,
                        arc: Some(ArcEdge {
                            center,
                            axis: scale(axis, sign),
                            radius: 2.5,
                            sweep: TAU * sign,
                            inside: h.0 != 0,
                        }),
                        curve: vec![],
                    }
                })
                .collect())
        }
    }

    fn count(origin: P3, axis: P3, length: f64) -> usize {
        let kernel = Ends {
            origin,
            axis,
            length,
        };
        let face = |length| FaceInfo {
            name: "side".into(),
            planar: None,
            area: TAU * 2.5 * length,
        };
        pairs(
            &kernel,
            ShapeHandle(0),
            ShapeHandle(1),
            &[face(10.0)],
            &[face(length)],
        )
        .unwrap()
        .len()
    }

    #[test]
    fn nearly_parallel_axes_must_remain_coaxial_at_both_overlap_ends() {
        assert_eq!(
            count([0.0; 3], [0.0, 0.0, 1.0], 10.0),
            1,
            "同軸の候補は残る"
        );
        let (sin, cos) = 3e-5_f64.sin_cos();
        assert!(cos >= 1.0 - 1e-9, "向きの許容差だけなら平行とみなす角度");
        assert!(10.0 * sin > PLANE_GAP, "軸の端では同一線の許容差を超える");
        let counts = [
            count([0.0; 3], [sin, 0.0, cos], 10.0),
            count([0.0, 0.0, 12.0], [sin, 0.0, -cos], 7.0),
        ];
        assert_eq!(
            counts,
            [0, 0],
            "重なる範囲の終端か始端で軸線が離れる組は外す"
        );
    }
}
