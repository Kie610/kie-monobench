//! 格子の速度を、一定の間隔に置いた点から向きと長さへ直す。

use super::*;

/// 矢印の置き方(格子のローカルの位置と単位の向き、長さmm、速さm/s)。
#[derive(Debug, Clone, PartialEq)]
pub struct Arrow {
    pub point: [f64; 3],
    pub direction: [f64; 3],
    pub length: f64,
    pub speed: f32,
}

/// 平面(世界の軸と位置)か空間に、速さに比例した矢印を並べる。
pub fn arrows(
    g: &Grid,
    velocity: &Values,
    plane: Option<(usize, f64)>,
    spacing: f64,
    maximum: f32,
) -> Vec<Arrow> {
    if !spacing.is_finite()
        || spacing <= 0.0
        || !maximum.is_finite()
        || maximum <= 0.0
        || !g.h.is_finite()
        || g.h <= 0.0
        || g.n.contains(&0)
    {
        return Vec::new();
    }
    let spacing = spacing.max(g.h);
    let mut lo = g.center(0, 0, 0);
    let mut hi = g.center(g.n[0] - 1, g.n[1] - 1, g.n[2] - 1);
    let plane = if g.flat() {
        Some((g.axes[2], g.origin[2] * g.signs[2]))
    } else {
        plane
    };
    if let Some((world_axis, at)) = plane {
        if world_axis >= 3 || !at.is_finite() {
            return Vec::new();
        }
        let axis = g.local_axis(world_axis);
        let at = at * g.signs[axis];
        if at < g.origin[axis] || at > g.origin[axis] + g.n[axis] as f64 * g.h {
            return Vec::new();
        }
        lo[axis] = at;
        hi[axis] = at;
    }
    let counts: [usize; 3] =
        std::array::from_fn(|a| ((hi[a] - lo[a]) / spacing).floor() as usize + 1);
    let mut out = Vec::new();
    for k in 0..counts[2] {
        for j in 0..counts[1] {
            for i in 0..counts[0] {
                let p = [i, j, k];
                let point = std::array::from_fn(|a| lo[a] + p[a] as f64 * spacing);
                let Some(cell) = super::stream::cell_of(g, point) else {
                    continue;
                };
                if g.solid[cell] {
                    continue;
                }
                let Some(mut v) = sample(g, velocity, 3, point).map(|v| v.map(f64::from)) else {
                    continue;
                };
                if g.flat() {
                    v[2] = 0.0;
                }
                let speed = v.iter().map(|x| x * x).sum::<f64>().sqrt();
                let length = 0.9 * spacing * speed / maximum as f64;
                if !length.is_finite() || length < 0.03 * spacing {
                    continue;
                }
                out.push(Arrow {
                    point,
                    direction: v.map(|x| x / speed),
                    length,
                    speed: speed as f32,
                });
            }
        }
    }
    out
}

/// 自動は断面の長辺に約30本、空間の長辺に約12本。セルより細かくはしない。
pub(super) fn spacing(g: &Grid, plane: Option<(usize, f64)>, requested: Option<f64>) -> f64 {
    let axis = if g.flat() {
        Some(2)
    } else {
        plane.map(|(a, _)| g.local_axis(a))
    };
    let longest = (0..3)
        .filter(|a| Some(*a) != axis)
        .map(|a| g.n[a] as f64 * g.h)
        .fold(g.h, f64::max);
    let target = if axis.is_some() { 30.0 } else { 12.0 };
    requested
        .unwrap_or_else(|| (longest / (target * g.h)).ceil().max(1.0) * g.h)
        .max(g.h)
}

/// 軸を三角柱、先を三角錐にして足す。
pub(super) fn mesh(b: &mut Builder, g: &Grid, arrow: &Arrow, scale: &Scale) {
    let p = g.to_world(arrow.point);
    let d = g.to_world(arrow.direction);
    let tip = std::array::from_fn(|a| p[a] + arrow.length * d[a]);
    let center = std::array::from_fn(|a| p[a] + 0.75 * arrow.length * d[a]);
    let face = scale.face((scale.t(arrow.speed).floor().max(0.0) as usize).min(scale.levels - 1));
    segment(b, p, center, 0.025 * arrow.length, face);
    let reference = if d[2].abs() < 0.9 {
        [0.0, 0.0, 1.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let Some(u) = unit(cross3(d, reference)) else {
        return;
    };
    let v = cross3(d, u);
    let ring: [[f64; 3]; 3] = std::array::from_fn(|i| {
        let angle = i as f64 * std::f64::consts::TAU / 3.0;
        std::array::from_fn(|a| {
            center[a] + 0.1 * arrow.length * (angle.cos() * u[a] + angle.sin() * v[a])
        })
    });
    for i in 0..3 {
        b.tri([ring[i], ring[(i + 1) % 3], tip], face);
    }
    b.tri([ring[2], ring[1], ring[0]], face);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> Grid {
        Grid {
            n: [5, 5, 1],
            origin: [0.0; 3],
            h: 2.0,
            axes: [0, 1, 2],
            signs: [1.0; 3],
            solid: vec![false; 25],
            body: None,
        }
    }

    #[test]
    fn velocity_arrows_have_regular_positions_and_equal_directions_and_lengths() {
        let g = grid();
        let v = Values::exact([4.0, 0.0, 0.0].repeat(g.len()));
        let a = arrows(&g, &v, None, 4.0, 4.0);
        assert_eq!(a.len(), 9);
        let mut expected = Vec::new();
        for y in [1.0, 5.0, 9.0] {
            for x in [1.0, 5.0, 9.0] {
                expected.push([x, y, 0.0]);
            }
        }
        assert_eq!(a.iter().map(|a| a.point).collect::<Vec<_>>(), expected);
        assert!(
            a.iter()
                .all(|a| a.direction == [1.0, 0.0, 0.0] && (a.length - 3.6).abs() < 1e-9)
        );
    }

    #[test]
    fn velocity_arrows_do_not_start_in_solid_cells() {
        let mut g = grid();
        let p = g.index(2, 2, 0);
        g.solid[p] = true;
        let v = Values::exact([4.0, 0.0, 0.0].repeat(g.len()));
        let a = arrows(&g, &v, None, 4.0, 4.0);
        assert_eq!(a.len(), 8);
        assert!(a.iter().all(|a| a.point != [5.0, 5.0, 0.0]));
    }

    #[test]
    fn velocity_arrows_scale_length_with_speed() {
        let g = grid();
        let mut values = [4.0, 0.0, 0.0].repeat(g.len());
        let p = g.index(2, 2, 0);
        values[p * 3] = 2.0;
        let a = arrows(&g, &Values::exact(values), None, 4.0, 4.0);
        let half = a.iter().find(|a| a.point == [5.0, 5.0, 0.0]).unwrap();
        let full = a.iter().find(|a| a.point == [1.0, 1.0, 0.0]).unwrap();
        assert!((half.length * 2.0 - full.length).abs() < 1e-9);
    }
    #[test]
    fn velocity_arrows_choose_cell_aligned_automatic_spacing_for_plane_and_volume() {
        let mut g = grid();
        g.n = [90, 30, 1];
        assert_eq!(spacing(&g, None, None), 6.0);
        g.n[2] = 20;
        assert_eq!(spacing(&g, None, None), 16.0);
        assert_eq!(spacing(&g, Some((2, 10.0)), None), 6.0);
        assert_eq!(spacing(&g, None, Some(3.0)), 3.0);
        assert_eq!(spacing(&g, None, Some(0.5)), g.h);
    }

    #[test]
    fn velocity_arrows_follow_a_world_plane_with_permuted_and_reversed_axes() {
        let mut g = grid();
        g.n[2] = 5;
        g.solid = vec![false; g.len()];
        g.axes = [1, 2, 0];
        g.signs = [-1.0, 1.0, -1.0];
        let v = Values::exact([4.0, 0.0, 0.0].repeat(g.len()));
        let a = arrows(&g, &v, Some((0, -4.0)), 4.0, 4.0);
        assert_eq!(a.len(), 9);
        assert!(a.iter().all(|a| g.to_world(a.point)[0] == -4.0));
        let mut b = Builder::default();
        let scale = Scale {
            lo: 0.0,
            span: 4.0,
            levels: 10,
            reverse: false,
            cap: None,
            base: 0,
        };
        mesh(&mut b, &g, &a[0], &scale);
        let tip = g.to_world(std::array::from_fn(|i| {
            a[0].point[i] + a[0].length * a[0].direction[i]
        }));
        assert!(
            b.mesh
                .positions
                .iter()
                .any(|p| (0..3).all(|i| (p[i] as f64 - tip[i]).abs() < 1e-6))
        );
    }

    #[test]
    fn velocity_arrows_skip_tiny_velocities_and_invalid_inputs() {
        let g = grid();
        let v = Values::exact([0.01, 0.0, 0.0].repeat(g.len()));
        assert!(arrows(&g, &v, None, 4.0, 4.0).is_empty());
        for s in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(arrows(&g, &v, None, s, 4.0).is_empty());
        }
        assert!(arrows(&g, &v, None, 4.0, 0.0).is_empty());
    }
    #[test]
    fn velocity_arrows_fill_a_volume_when_no_section_is_selected() {
        let mut g = grid();
        g.n[2] = 5;
        g.solid = vec![false; g.len()];
        let velocity = Values::exact([0.0, 0.0, 4.0].repeat(g.len()));
        let arrows = arrows(&g, &velocity, None, 4.0, 4.0);
        assert_eq!(arrows.len(), 27);
        for z in [1.0, 5.0, 9.0] {
            assert_eq!(arrows.iter().filter(|a| a.point[2] == z).count(), 9);
        }
        assert!(arrows.iter().all(|a| a.direction == [0.0, 0.0, 1.0]));
    }
}
