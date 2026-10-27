//! ボディの名前と三角形の網を OBJ に書く。色と材料のファイルは作らない。

use kmb_core::{LengthUnit, Mesh};
use std::io::{self, Write};

/// 頂点はボディ内で共有し、法線は面ごとに持つ。番号はファイル全体で 1 から続ける。
/// OBJ は単位を持たないので、座標を指定の単位へ換算し、コメントにも単位を残す。
pub fn write_obj<W: Write>(
    bodies: &[(String, Mesh)],
    unit: LengthUnit,
    mut writer: W,
) -> io::Result<()> {
    writeln!(writer, "# kieMonoBench OBJ; unit: {}", unit.name())?;
    let mut vertex_base = 1usize;
    let mut normal = 1usize;
    for (name, mesh) in bodies {
        let name: String = name
            .chars()
            .map(|c| if c.is_control() || c == '#' { '_' } else { c })
            .collect();
        writeln!(writer, "o {name}")?;
        let (positions, triangles) = crate::threemf::welded(mesh);
        let welded = Mesh {
            positions,
            triangles,
            ..Mesh::default()
        };
        for point in &welded.positions {
            let p = point.map(|v| f64::from(v) / unit.mm());
            writeln!(writer, "v {} {} {}", p[0], p[1], p[2])?;
        }
        for triangle in &welded.triangles {
            let n = crate::facet_normal(&welded, triangle);
            writeln!(writer, "vn {} {} {}", n[0], n[1], n[2])?;
            let v = triangle.map(|i| vertex_base + i as usize);
            writeln!(
                writer,
                "f {}//{normal} {}//{normal} {}//{normal}",
                v[0], v[1], v[2]
            )?;
            normal += 1;
        }
        vertex_base += welded.positions.len();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tetrahedron_shares_vertices_and_keeps_outward_normals() {
        let mesh = crate::tests::tetra();
        // 面ごとに分かれた頂点も同じ位置で共有する。
        let split = Mesh {
            positions: mesh
                .triangles
                .iter()
                .flatten()
                .map(|i| mesh.positions[*i as usize])
                .collect(),
            triangles: (0..4).map(|i| [3 * i, 3 * i + 1, 3 * i + 2]).collect(),
            ..Mesh::default()
        };
        let mut out = Vec::new();
        write_obj(&[("四面体".into(), split)], LengthUnit::Mm, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.lines().filter(|l| l.starts_with("v ")).count(), 4);
        assert_eq!(text.lines().filter(|l| l.starts_with("vn ")).count(), 4);
        assert_eq!(text.lines().filter(|l| l.starts_with("f ")).count(), 4);
        assert!(text.contains("o 四面体\n"));
        assert!(text.contains("vn 0 0 -1\nf 1//1 2//1 3//1"));
        assert!(!text.contains("mtllib"));
    }

    #[test]
    fn vertex_coordinates_use_at_most_six_decimal_places_after_unit_conversion() {
        let mut mesh = crate::tests::tetra();
        mesh.positions[0] = [12.3, -1.2345678, -0.00000001];
        let mut out = Vec::new();
        write_obj(&[("形".into(), mesh)], LengthUnit::Mm, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(
            text.lines().find(|l| l.starts_with("v ")),
            Some("v 12.3 -1.234568 0")
        );

        let mut mesh = crate::tests::tetra();
        mesh.positions[0] = [25.4, 12.7, -0.000001];
        let mut out = Vec::new();
        write_obj(&[("形".into(), mesh)], LengthUnit::In, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(
            text.lines().find(|l| l.starts_with("v ")),
            Some("v 1 0.5 0")
        );
    }

    #[test]
    fn object_indices_continue_and_units_and_names_are_safe() {
        let mut mesh = crate::tests::tetra();
        for point in &mut mesh.positions {
            for v in point {
                *v *= 25.4;
            }
        }
        let mut out = Vec::new();
        write_obj(
            &[
                ("板\n#f 9 9 9".into(), mesh.clone()),
                ("板(2)".into(), mesh),
            ],
            LengthUnit::In,
            &mut out,
        )
        .unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("o 板__f 9 9 9\n"));
        assert!(text.contains("o 板(2)\n"));
        let v: Vec<f64> = text
            .lines()
            .filter(|l| l.starts_with("v "))
            .nth(1)
            .unwrap()
            .split_whitespace()
            .skip(1)
            .map(|s| s.parse().unwrap())
            .collect();
        assert!((v[0] - 1.0).abs() < 1e-6);
        let last = text.lines().rfind(|l| l.starts_with("f ")).unwrap();
        assert_eq!(last, "f 6//8 7//8 8//8");
    }
}
