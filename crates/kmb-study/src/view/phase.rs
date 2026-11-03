use super::*;

/// 同じコマ・色分けのまま揺らす時に、色の区切りを解き直さないための控え。
pub(crate) struct PhaseMesh {
    motions: Vec<Motion>,
    template: OverlayShape,
}

struct Motion {
    zero: [[f64; 3]; 3],
    one: [[f64; 3]; 3],
    face: u32,
}

impl PhaseMesh {
    pub(crate) fn new(r: &SimResult, look: &Look) -> Option<Self> {
        // 切り口や線の管は、位相で交点や断面が変わる。表面の平らな三角形だけを使い回す。
        if !look.deform
            || look.section.is_some()
            || look.streamlines
            || !r.marks.is_empty()
            || r.parts
                .iter()
                .any(|p| !matches!(p.geom, Geometry::Surface { .. }))
            || !r
                .quantities
                .iter()
                .any(|q| q.role == Role::Deform && q.comps == 3)
        {
            return None;
        }
        let mut at = look.clone();
        at.phase = 0.0;
        let (_, zero) = draw_recorded(r, &at, 0, true);
        at.phase = 1.0;
        let (mut template, one) = draw_recorded(r, &at, 0, true);
        if zero.len() != one.len() || zero.iter().zip(&one).any(|(z, o)| z.1 != o.1) {
            return None;
        }
        let motions = zero
            .into_iter()
            .zip(one)
            .map(|((zero, face), (one, _))| Motion { zero, one, face })
            .collect();
        let face_names = std::mem::take(&mut template.mesh.face_names);
        template.mesh = Mesh {
            face_names,
            ..Mesh::default()
        };
        Some(Self { motions, template })
    }

    pub(crate) fn draw(&self, phase: f64, key: u64) -> Option<OverlayShape> {
        if !phase.is_finite() || !(-1.0..=1.0).contains(&phase) {
            return None;
        }
        let n = self.motions.len();
        let mut b = Builder::default();
        b.mesh.positions.reserve(3 * n);
        b.mesh.normals.reserve(3 * n);
        b.mesh.triangles.reserve(n);
        b.mesh.triangle_faces.reserve(n);
        for m in &self.motions {
            let p = if phase == 0.0 {
                m.zero
            } else if phase == 1.0 {
                m.one
            } else {
                std::array::from_fn(|i| {
                    std::array::from_fn(|a| m.zero[i][a] + phase * (m.one[i][a] - m.zero[i][a]))
                })
            };
            // 途中の位相で退化する物も、元の描き方と同じ法線・面積の判定で出し入れする。
            b.tri(p, m.face);
        }
        let mut shape = self.template.clone();
        b.mesh.face_names = std::mem::take(&mut shape.mesh.face_names);
        shape.mesh = b.mesh;
        shape.key = key;
        Some(shape)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result::{Frame, Part, Quantity};

    fn surface(offset: f32) -> SimResult {
        SimResult {
            kind: "modal".into(),
            parts: vec![Part {
                name: "面".into(),
                geom: Geometry::Surface {
                    points: vec![
                        [offset, 0.0, 0.0],
                        [offset + 100.0, 0.0, 0.0],
                        [offset + 100.0, 20.0, 0.0],
                        [offset, 20.0, 0.0],
                    ],
                    tris: vec![[0, 1, 2], [0, 2, 3]],
                },
            }],
            quantities: vec![Quantity::vector("揺れ", "相対", 0, Role::Deform)],
            frames: vec![Frame {
                label: "形".into(),
                time: None,
                values: vec![Values::exact(vec![
                    0.0, 0.0, 0.0, 0.2, 0.1, 1.0, -0.1, 0.3, 0.7, 0.0, 0.4, -0.5,
                ])],
            }],
            ..SimResult::default()
        }
    }

    fn same(r: &SimResult, look: &Look, cache: &PhaseMesh, phase: f64) {
        let exact = draw(
            r,
            &Look {
                phase,
                ..look.clone()
            },
            42,
        );
        let fast = cache.draw(phase, 42).unwrap();
        assert_eq!(exact.key, fast.key);
        assert_eq!(exact.mesh.triangles, fast.mesh.triangles);
        assert_eq!(exact.mesh.triangle_faces, fast.mesh.triangle_faces);
        assert_eq!(exact.mesh.face_names, fast.mesh.face_names);
        assert_eq!(exact.face_colors, fast.face_colors);
        assert_eq!(exact.legend, fast.legend);
        assert_eq!(exact.more_legends, fast.more_legends);
        assert_eq!(exact.label, fast.label);
        assert_eq!(exact.badge, fast.badge);
        for (a, b) in exact.mesh.positions.iter().zip(&fast.mesh.positions) {
            for k in 0..3 {
                assert!(
                    (a[k] - b[k]).abs() <= 8.0 * f32::EPSILON * a[k].abs().max(1.0),
                    "点: {a:?} {b:?}"
                );
            }
        }
        for (a, b) in exact.mesh.normals.iter().zip(&fast.mesh.normals) {
            for k in 0..3 {
                assert!((a[k] - b[k]).abs() < 2e-4, "法線: {a:?} {b:?}");
            }
        }
        if phase == 0.0 || phase == 1.0 {
            assert_eq!(exact.mesh, fast.mesh);
        }
    }

    #[test]
    fn the_phase_cache_matches_full_draw_for_colors_normals_and_large_coordinates() {
        for offset in [0.0, 1_000_000.0] {
            let r = surface(offset);
            for (surface, smooth) in [(None, true), (Some(0), false), (Some(0), true)] {
                let look = Look {
                    surface,
                    smooth,
                    ..Look::default()
                };
                let cache = PhaseMesh::new(&r, &look).unwrap();
                for phase in [-1.0, 0.0, 0.25, 0.63, 1.0] {
                    same(&r, &look, &cache, phase);
                }
            }
        }
    }

    #[test]
    fn the_phase_cache_keeps_candidates_that_are_degenerate_at_the_reference_phase() {
        let mut r = surface(0.0);
        r.parts[0].geom = Geometry::Surface {
            points: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [2.0, 0.0, 0.0],
                [100.0, 0.0, 0.0],
            ],
            tris: vec![[0, 1, 2]],
        };
        r.frames[0].values[0] = Values::exact(vec![
            0.0, 0.0, 0.0, -1.0, 1.0, 0.0, -2.0, 3.0, 0.0, 0.0, 0.0, 0.0,
        ]);
        let look = Look::default();
        assert!(
            draw(
                &r,
                &Look {
                    phase: 0.0,
                    ..look.clone()
                },
                0
            )
            .mesh
            .triangles
            .is_empty()
        );
        let cache = PhaseMesh::new(&r, &look).unwrap();
        assert!(!cache.motions.is_empty());
        assert!(!cache.draw(-1.0, 0).unwrap().mesh.triangles.is_empty());
        for phase in [-1.0, 0.0, 0.25, 0.5, 0.75, 1.0] {
            same(&r, &look, &cache, phase);
        }
    }

    #[test]
    fn phase_cache_unsupported_looks_use_full_draw_instead() {
        let r = surface(0.0);
        for look in [
            Look {
                deform: false,
                ..Look::default()
            },
            Look {
                section: Some((0, 2, 0.0)),
                ..Look::default()
            },
            Look {
                streamlines: true,
                ..Look::default()
            },
        ] {
            assert!(PhaseMesh::new(&r, &look).is_none());
        }
        let mut marked = r.clone();
        marked.marks.push(crate::result::Mark::default());
        assert!(PhaseMesh::new(&marked, &Look::default()).is_none());
        let mut mixed = r;
        mixed.parts.push(Part {
            name: "格子".into(),
            geom: Geometry::Grid(Grid {
                n: [1, 1, 1],
                origin: [0.0; 3],
                h: 1.0,
                axes: [0, 1, 2],
                signs: [1.0; 3],
                solid: vec![false],
                body: None,
            }),
        });
        assert!(PhaseMesh::new(&mixed, &Look::default()).is_none());
        mixed.parts.pop();
        mixed.parts[0].geom = Geometry::Tets(Tets {
            points: vec![],
            tets: vec![],
            faces: vec![],
            face_ids: vec![],
        });
        assert!(PhaseMesh::new(&mixed, &Look::default()).is_none());
    }
}
