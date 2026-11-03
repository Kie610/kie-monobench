//! 同じ軸の全回転で、幾何学的に包含を証明できる道具だけを整理する。
use kmb_cad::{Operation, Recipe};
use kmb_core::kernel::{self as k, P2, P3, Profile, Segment};

pub(super) fn remove_contained(parts: Vec<Recipe>) -> Vec<Recipe> {
    if parts.len() < 2 {
        return parts;
    }
    let Some(Recipe::Revolve { origin, dir, .. }) = parts.first() else {
        return parts;
    };
    let length = k::dot(*dir, *dir).sqrt();
    if !length.is_finite() || length == 0.0 {
        return parts;
    }
    let axis = k::scale(*dir, 1.0 / length);
    let Some(polygons) = parts
        .iter()
        .map(|p| meridian(p, *origin, axis))
        .collect::<Option<Vec<_>>>()
    else {
        return parts;
    };
    let convex: Vec<bool> = polygons.iter().map(|p| contains(p, p)).collect();
    let keep: Vec<bool> = (0..parts.len())
        .map(|i| {
            !(0..parts.len()).any(|j| {
                i != j
                    && convex[j]
                    && contains(&polygons[j], &polygons[i])
                    && (!convex[i] || !contains(&polygons[i], &polygons[j]) || j < i)
            })
        })
        .collect();
    parts
        .into_iter()
        .zip(keep)
        .filter_map(|(p, keep)| keep.then_some(p))
        .collect()
}

fn meridian(recipe: &Recipe, origin: P3, axis: P3) -> Option<Vec<P2>> {
    let Recipe::Revolve {
        operation: Operation::New,
        profile,
        origin: o,
        dir,
        angle,
    } = recipe
    else {
        return None;
    };
    if *angle != std::f64::consts::TAU {
        return None;
    }
    let length = k::dot(*dir, *dir).sqrt();
    if !length.is_finite() || length == 0.0 {
        return None;
    }
    let d = k::scale(*dir, 1.0 / length);
    // 丸めで同軸か決めきれない物は残す。全回転なので軸の向きが逆でも同じ道具になる。
    if (d != axis && d != k::scale(axis, -1.0)) || k::cross(k::sub(*o, origin), axis) != [0.0; 3] {
        return None;
    }
    project(profile, origin, axis)
}

fn project(profile: &Profile, origin: P3, axis: P3) -> Option<Vec<P2>> {
    let [region] = profile.regions.as_slice() else {
        return None;
    };
    if !region.holes.is_empty() {
        return None;
    }
    let edges = &region.outer.edges;
    if edges.len() < 3
        || edges
            .iter()
            .any(|e| !matches!(e.segment, Segment::Line { .. }))
    {
        return None;
    }
    if edges
        .iter()
        .enumerate()
        .any(|(i, e)| e.to_point() != edges[(i + 1) % edges.len()].from_point())
    {
        return None;
    }
    let n = profile.plane.normal();
    if k::dot(n, axis) != 0.0 || k::dot(k::sub(profile.plane.origin, origin), n) != 0.0 {
        return None;
    }
    let radial = k::normalize(k::cross(n, axis));
    let mut points: Vec<P2> = edges
        .iter()
        .map(|e| {
            let p = k::sub(profile.plane.to_world(e.from_point()), origin);
            [k::dot(p, axis), k::dot(p, radial)]
        })
        .collect();
    if points.iter().flatten().any(|v| !v.is_finite()) {
        return None;
    }
    if points.iter().all(|p| p[1] <= 0.0) {
        for p in &mut points {
            p[1] = -p[1];
        }
    } else if points.iter().any(|p| p[1] < 0.0) {
        return None;
    }
    Some(points)
}

/// 外側の全辺の同じ半平面に全頂点が入るか。外側自身にも適用して凸性を確かめる。
/// 外側は凸なので、内側の頂点を結ぶ全線分とその囲む領域も含む。境界の外へ許容を足さない。
fn contains(outer: &[P2], inner: &[P2]) -> bool {
    let cross = |a: P2, b: P2| a[0] * b[1] - a[1] * b[0];
    let area: f64 = (0..outer.len())
        .map(|i| cross(outer[i], outer[(i + 1) % outer.len()]))
        .sum();
    if !area.is_finite() || area == 0.0 {
        return false;
    }
    (0..outer.len()).all(|i| {
        let a = outer[i];
        let b = outer[(i + 1) % outer.len()];
        inner.iter().all(|p| {
            cross([b[0] - a[0], b[1] - a[1]], [p[0] - a[0], p[1] - a[1]]) * area.signum() >= 0.0
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kmb_core::kernel::{Plane, ProfileEdge, ProfileLoop, Region};

    fn tool(points: &[P2]) -> Recipe {
        Recipe::Revolve {
            operation: Operation::New,
            origin: [0.0; 3],
            dir: [1.0, 0.0, 0.0],
            angle: std::f64::consts::TAU,
            profile: Profile {
                plane: Plane::XY,
                regions: vec![Region {
                    holes: vec![],
                    outer: ProfileLoop {
                        edges: (0..points.len())
                            .map(|i| ProfileEdge {
                                name: format!("edge{i}"),
                                reversed: false,
                                segment: Segment::Line {
                                    a: points[i],
                                    b: points[(i + 1) % points.len()],
                                },
                            })
                            .collect(),
                    },
                }],
            },
        }
    }

    /// 確かめること: 包含した道具だけを除き、残す順と区間の名前を保つ。
    #[test]
    fn full_revolutions_remove_proven_containment_and_keep_original_names() {
        let outer = tool(&[[0., 0.], [10., 0.], [10., 4.], [0., 4.]]);
        let inner = tool(&[[0., 0.], [10., 0.], [10., 3.], [0., 3.]]);
        assert_eq!(
            remove_contained(vec![inner, outer.clone()]),
            vec![outer.clone()]
        );
        let mut duplicate = outer.clone();
        let Recipe::Revolve { profile, .. } = &mut duplicate else {
            unreachable!()
        };
        for e in &mut profile.regions[0].outer.edges {
            e.name.push_str("_later");
        }
        assert_eq!(
            remove_contained(vec![outer.clone(), duplicate]),
            vec![outer]
        );
    }

    /// 確かめること: 穴・凹み・部分回転・異なる軸では包含を推測しない。
    #[test]
    fn uncertain_revolution_containment_keeps_all_tools() {
        let outer = tool(&[[0., 0.], [10., 0.], [10., 4.], [0., 4.]]);
        let inner = tool(&[[0., 0.], [10., 0.], [10., 3.], [0., 3.]]);
        let mut hole = outer.clone();
        let Recipe::Revolve { profile, .. } = &mut hole else {
            unreachable!()
        };
        let Recipe::Revolve {
            profile: hole_profile,
            ..
        } = tool(&[[2., 1.], [3., 1.], [3., 2.], [2., 2.]])
        else {
            unreachable!()
        };
        profile.regions[0].holes.push(k::canonical_loop(
            hole_profile.regions[0].outer.clone(),
            false,
        ));
        let concave = tool(&[
            [0., 0.],
            [10., 0.],
            [10., 4.],
            [6., 4.],
            [6., 2.],
            [4., 2.],
            [4., 4.],
            [0., 4.],
        ]);
        let mut partial = outer.clone();
        let Recipe::Revolve { angle, .. } = &mut partial else {
            unreachable!()
        };
        *angle = std::f64::consts::PI;
        let mut other_axis = outer;
        let Recipe::Revolve { origin, .. } = &mut other_axis else {
            unreachable!()
        };
        origin[1] = -1.0;
        for other in [hole, concave, partial, other_axis] {
            let tools = vec![inner.clone(), other];
            assert_eq!(remove_contained(tools.clone()), tools);
        }
    }
}
