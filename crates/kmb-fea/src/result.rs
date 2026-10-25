//! 解析の網を結果の形(kmb-study の [`Geometry::Tets`])にすることと、数の書き方。結果の形と
//! 見せ方は kmb-study。

use crate::tet10::FACES;
use crate::tetmesh::TetMesh;
use kmb_study::{Geometry, Tets};
use std::collections::HashMap;

/// 残す要素(`keep[e]`)の網。外の面は、残す要素どうしで共有しない要素の面。
pub fn geometry(m: &TetMesh, keep: &[bool]) -> Geometry {
    let points = m.points.iter().map(|p| p.map(|v| v as f32)).collect();
    let all = keep.iter().all(|k| *k);
    let tets: Vec<[u32; 10]> = m
        .tets
        .iter()
        .zip(keep)
        .filter(|(_, k)| **k)
        .map(|(t, _)| *t)
        .collect();
    let (faces, face_ids): (Vec<[u32; 6]>, Vec<u32>) = if all {
        m.faces.iter().map(|f| (f.nodes, f.face)).unzip()
    } else {
        // 残した要素の外の面のうち、元の網の表面の面は形の面の番号を引き継ぐ
        let label: HashMap<[u32; 3], u32> = m
            .faces
            .iter()
            .map(|f| {
                let mut k = [f.nodes[0], f.nodes[1], f.nodes[2]];
                k.sort_unstable();
                (k, f.face)
            })
            .collect();
        let mut count: HashMap<[u32; 3], (u32, [u32; 6])> = HashMap::new();
        for t in &tets {
            for (c, mid) in FACES {
                let nodes = [c[0], c[1], c[2], mid[0], mid[1], mid[2]].map(|i| t[i]);
                let mut k = [nodes[0], nodes[1], nodes[2]];
                k.sort_unstable();
                count
                    .entry(k)
                    .and_modify(|x| x.0 += 1)
                    .or_insert((1, nodes));
            }
        }
        let mut f: Vec<[u32; 6]> = count
            .into_values()
            .filter(|x| x.0 == 1)
            .map(|x| x.1)
            .collect();
        f.sort_unstable();
        f.into_iter()
            .map(|n| {
                let mut k = [n[0], n[1], n[2]];
                k.sort_unstable();
                (n, label.get(&k).copied().unwrap_or(u32::MAX))
            })
            .unzip()
    };
    Geometry::Tets(Tets {
        points,
        tets,
        faces,
        face_ids,
    })
}

/// 数を短く(有効数字 3 桁ほど)。
pub fn fmt(v: f64) -> String {
    let a = v.abs();
    if a == 0.0 {
        "0".into()
    } else if a >= 1000.0 {
        format!("{v:.0}")
    } else if a >= 100.0 {
        format!("{v:.1}")
    } else if a >= 1.0 {
        format!("{v:.2}")
    } else if a >= 0.01 {
        format!("{v:.3}")
    } else {
        format!("{v:.2e}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boundary::Prepared;

    /// 結果の表面の三角形は形の面の番号を持つ(表示の陰を形の面の中だけで滑らかにするため)。
    /// 要素を一部だけ残した時は、元の表面の三角形は番号を引き継ぎ、新しくできた外の面は
    /// 分からない(`u32::MAX`)。
    #[test]
    fn the_surface_keeps_the_face_numbers() {
        let p = Prepared::new(
            &crate::validation_tests::box_mesh([10.0, 6.0, 4.0]),
            2.0,
            &|| false,
        )
        .unwrap();
        let m = &p.mesh;
        let Geometry::Tets(t) = geometry(m, &vec![true; m.tets.len()]) else {
            panic!("四面体の網でない");
        };
        assert_eq!(
            t.face_ids,
            m.faces.iter().map(|f| f.face).collect::<Vec<_>>()
        );
        assert!(
            t.face_ids
                .iter()
                .all(|f| (*f as usize) < p.face_names.len())
        );
        // x < 5 の要素だけ残す
        let keep: Vec<bool> = (0..m.tets.len())
            .map(|e| {
                (0..4)
                    .map(|k| m.points[m.tets[e][k] as usize][0])
                    .sum::<f64>()
                    < 20.0
            })
            .collect();
        let Geometry::Tets(t) = geometry(m, &keep) else {
            panic!("四面体の網でない");
        };
        assert_eq!(t.face_ids.len(), t.faces.len());
        let (mut known, mut cut) = (0, 0);
        for (f, id) in t.faces.iter().zip(&t.face_ids) {
            let on_box = f[..3].iter().all(|v| {
                let q = m.points[*v as usize];
                (0..3).any(|a| q[a].abs() < 1e-9 || (q[a] - [10.0, 6.0, 4.0][a]).abs() < 1e-9)
            });
            if *id == u32::MAX {
                cut += 1;
            } else {
                known += 1;
                assert!(on_box, "形の面の番号を持つ三角形が箱の面に無い");
            }
        }
        assert!(known > 0 && cut > 0, "番号あり {known}・無し {cut}");
    }
}
