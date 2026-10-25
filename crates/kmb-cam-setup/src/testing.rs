//! 機能の評価(文書と形状カーネル)を通さずに、加工の戦略を三角形の網だけで試すための材料。
//! 3D の加工の戦略の単体試験が使う。

use crate::{Flip, MESH_TOLERANCE, Prepared, SetupInfo};
use kmb_cam_geom::MachiningModel;
use kmb_cam_tool::Tool;
use kmb_core::{Mesh, ShapeHandle};
use kmb_toolpath::P3;

/// 面ごとの三角形(面の名前と三角形の列)から網を作る。頂点の法線は三角形の向き。
pub fn mesh_of(faces: &[(&str, Vec<[P3; 3]>)]) -> Mesh {
    let mut m = Mesh::default();
    for (f, (name, tris)) in faces.iter().enumerate() {
        m.face_names.push(name.to_string());
        for t in tris {
            let (a, b, c) = (t[0], t[1], t[2]);
            let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let n = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
            let k = m.positions.len() as u32;
            for p in t {
                m.positions.push(p.map(|x| x as f32));
                m.normals.push(n.map(|x| (x / l) as f32));
            }
            m.triangles.push([k, k + 1, k + 2]);
            m.triangle_faces.push(f as u32);
        }
    }
    m
}

/// 網と箱のストック(作業座標系)と工具から、加工の材料を作る。安全高さはストックの上面の 5 mm 上、
/// 退避高さは 1 mm 上。切削条件は工具の既定値。
pub fn prepared(mesh: &Mesh, stock_min: P3, stock_max: P3, tool: Tool) -> Prepared {
    let top = stock_max[2];
    Prepared {
        info: SetupInfo {
            body: ShapeHandle(0),
            origin: [0.0; 3],
            stock_min,
            stock_max,
            safe_z: top + 5.0,
            retract_z: top + 1.0,
            clearance_z: top + 20.0,
            wcs: 1,
            material: String::new(),
            flip: Flip::default(),
            flip_center: [0.0; 3],
            tolerance: MESH_TOLERANCE,
            keep_out: vec![],
            fixtures: vec![],
            rot: None,
            more_bodies: vec![],
            part_min: stock_min,
            part_max: stock_max,
            stock_body: None,
            program: None,
            comment: String::new(),
            bed_offset: [0.0; 2],
            machine: kmb_post::machines::GENERIC_GRBL.into(),
            machine_values: kmb_post::machines::generic_grbl(),
            leveling: None,
        },
        cutting: tool.cutting(),
        tool,
        model: MachiningModel::from_mesh(mesh, [0.0; 3]),
        warnings: vec![],
        fit: None,
        last_step: None,
        link: Default::default(),
        heights: Default::default(),
    }
}
