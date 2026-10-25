//! 描く物の一式(網と材質)。網は世界の座標(mm、Z が上)。

use crate::Look;
use crate::math::V3;
use jc_core::Mesh;

/// 描く物 1 つ。
#[derive(Debug, Clone)]
pub struct Object {
    pub mesh: Mesh,
    pub look: Look,
}

impl Object {
    /// 網と材質から。木目の芯は網の箱に合わせる。
    pub fn new(mesh: Mesh, look: Look) -> Object {
        let (min, max) = bounds_of(std::slice::from_ref(&mesh));
        Object {
            look: look.frame_for(min, max),
            mesh,
        }
    }

    /// 面ごとに材質を替えたボディ。`faces` は `mesh.face_names` の順の面の材質(`None` と足りない
    /// 面はボディの材質 `look`)。材質ごとに網を分け、木目の芯はどれもボディ全体の箱に合わせる
    /// (面を替えても年輪がつながる)。
    pub fn with_faces(mesh: Mesh, look: Look, faces: &[Option<Look>]) -> Vec<Object> {
        let (min, max) = bounds_of(std::slice::from_ref(&mesh));
        let fit = |l: &Look| l.clone().frame_for(min, max);
        // デカールは面ごとにその面の平面へ合わせる(同じ材質でも面が違えば別の材質になる)
        let faces: Vec<Option<Look>> = faces
            .iter()
            .enumerate()
            .map(|(i, f)| f.clone().map(|l| l.fit_decal(&mesh, i)))
            .collect();
        if faces.iter().all(Option::is_none) {
            return vec![Object {
                look: fit(&look),
                mesh,
            }];
        }
        // 材質ごとの番号(0 はボディの材質)
        let mut looks: Vec<Look> = vec![look];
        let face_slot: Vec<usize> = faces
            .iter()
            .map(|f| match f {
                None => 0,
                Some(l) => match looks.iter().position(|x| x == l) {
                    Some(i) => i,
                    None => {
                        looks.push(l.clone());
                        looks.len() - 1
                    }
                },
            })
            .collect();
        let slot_of = |tri: usize| {
            mesh.triangle_faces
                .get(tri)
                .and_then(|f| face_slot.get(*f as usize))
                .copied()
                .unwrap_or(0)
        };
        let mut out = Vec::new();
        for (k, l) in looks.iter().enumerate() {
            let mut part = Mesh {
                face_names: mesh.face_names.clone(),
                // 面の境目の線は 1 つ目にだけ持たせる(重ねて描かない)
                edges: if out.is_empty() {
                    mesh.edges.clone()
                } else {
                    Vec::new()
                },
                ..Mesh::default()
            };
            let mut index: std::collections::HashMap<u32, u32> = Default::default();
            for (ti, t) in mesh.triangles.iter().enumerate() {
                if slot_of(ti) != k {
                    continue;
                }
                let t = t.map(|v| {
                    *index.entry(v).or_insert_with(|| {
                        part.positions.push(mesh.positions[v as usize]);
                        if let Some(n) = mesh.normals.get(v as usize) {
                            part.normals.push(*n);
                        }
                        (part.positions.len() - 1) as u32
                    })
                });
                part.triangles.push(t);
                part.triangle_faces
                    .push(mesh.triangle_faces.get(ti).copied().unwrap_or(0));
            }
            if !part.triangles.is_empty() {
                out.push(Object {
                    look: fit(l),
                    mesh: part,
                });
            }
        }
        out
    }
}

/// 描く物の一式。
#[derive(Debug, Clone, Default)]
pub struct RenderScene {
    pub objects: Vec<Object>,
    /// 文書の点光源とスポットライト(描く時に環境の光に足す)
    pub lamps: Vec<crate::lamp::Lamp>,
}

impl RenderScene {
    pub fn new(objects: Vec<Object>) -> RenderScene {
        RenderScene {
            objects,
            lamps: Vec::new(),
        }
    }

    /// 点光源とスポットライトを足した物。
    pub fn with_lamps(mut self, lamps: Vec<crate::lamp::Lamp>) -> RenderScene {
        self.lamps = lamps;
        self
    }

    /// 全体の箱(物が無ければ原点のまわりの 10 mm の箱)。
    pub fn bounds(&self) -> (V3, V3) {
        let meshes: Vec<&Mesh> = self.objects.iter().map(|o| &o.mesh).collect();
        let (min, max) = bounds_of_refs(&meshes);
        if min[0] > max[0] {
            return ([-5.0; 3], [5.0; 3]);
        }
        (min, max)
    }

    /// 床の高さ(いちばん下の点)。
    pub fn floor_z(&self) -> f32 {
        self.bounds().0[2]
    }

    /// 三角形の数。
    pub fn triangles(&self) -> usize {
        self.objects.iter().map(|o| o.mesh.triangles.len()).sum()
    }
}

fn bounds_of(meshes: &[Mesh]) -> (V3, V3) {
    bounds_of_refs(&meshes.iter().collect::<Vec<_>>())
}

fn bounds_of_refs(meshes: &[&Mesh]) -> (V3, V3) {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for m in meshes {
        for p in &m.positions {
            for k in 0..3 {
                min[k] = min[k].min(p[k]);
                max[k] = max[k].max(p[k]);
            }
        }
    }
    (min, max)
}

/// 三角形の頂点の法線(網の法線が無いか長さ 0 なら三角形の向き)。
pub(crate) fn vertex_normal(m: &Mesh, i: u32, face: V3) -> V3 {
    match m.normals.get(i as usize) {
        Some(n) if crate::math::dot(*n, *n) > 1e-12 => crate::math::normalize(*n),
        _ => face,
    }
}
