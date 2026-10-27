//! 三角形の網([`kmb_core::Mesh`])を STL(単位は mm)・3MF([`threemf`]。色と単位も)・OBJ に書き出す。
//! 書き出しの操作(STL・STEP・3MF・OBJ)も持つ([`ExportCommands`])。今の視点の線の DXF・SVG は kmb-io-view。

mod commands;
pub mod obj;
pub mod threemf;
pub use commands::{EXPORT_3MF, EXPORT_OBJ, EXPORT_STEP, EXPORT_STL, ExportCommands};
pub use obj::write_obj;
pub use threemf::{Body3mf, write_3mf};

use kmb_core::Mesh;
use std::io::{self, Write};

fn facet_normal(m: &Mesh, t: &[u32; 3]) -> [f32; 3] {
    let p = |i: u32| m.positions[i as usize];
    let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let n = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if l > 0.0 {
        [n[0] / l, n[1] / l, n[2] / l]
    } else {
        [0.0; 3]
    }
}

/// バイナリ形式で書く。
pub fn write_binary<W: Write>(meshes: &[Mesh], mut w: W) -> io::Result<()> {
    let mut header = [0u8; 80];
    let text = b"kieMonoBench binary STL";
    header[..text.len()].copy_from_slice(text);
    w.write_all(&header)?;
    let count: usize = meshes.iter().map(|m| m.triangles.len()).sum();
    let count = u32::try_from(count)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "三角形が多すぎます"))?;
    w.write_all(&count.to_le_bytes())?;
    for m in meshes {
        for t in &m.triangles {
            for v in facet_normal(m, t) {
                w.write_all(&v.to_le_bytes())?;
            }
            for i in t {
                for v in m.positions[*i as usize] {
                    w.write_all(&v.to_le_bytes())?;
                }
            }
            w.write_all(&[0, 0])?;
        }
    }
    Ok(())
}

/// ASCII 形式で書く。
pub fn write_ascii<W: Write>(meshes: &[Mesh], name: &str, mut w: W) -> io::Result<()> {
    writeln!(w, "solid {name}")?;
    for m in meshes {
        for t in &m.triangles {
            let n = facet_normal(m, t);
            writeln!(w, "  facet normal {:e} {:e} {:e}", n[0], n[1], n[2])?;
            writeln!(w, "    outer loop")?;
            for i in t {
                let p = m.positions[*i as usize];
                writeln!(w, "      vertex {:e} {:e} {:e}", p[0], p[1], p[2])?;
            }
            writeln!(w, "    endloop")?;
            writeln!(w, "  endfacet")?;
        }
    }
    writeln!(w, "endsolid {name}")
}

/// この crate の名乗り(操作)。組み立て(kmb-bundle)が 1 回だけ並べる。
pub fn module() -> kmb_core::Module {
    kmb_core::Module::commands("kmb-io-stl").with_extension(ExportCommands)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn tetra() -> Mesh {
        Mesh {
            positions: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            normals: vec![[0.0; 3]; 4],
            triangles: vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
            triangle_faces: vec![0; 4],
            face_names: vec!["f".into()],
            edges: vec![],
        }
    }

    #[test]
    fn binary_layout() {
        let mut out = Vec::new();
        write_binary(&[tetra()], &mut out).unwrap();
        assert_eq!(out.len(), 80 + 4 + 4 * 50);
        assert_eq!(u32::from_le_bytes(out[80..84].try_into().unwrap()), 4);
        // 1 つ目の三角形 (0,2,1) の法線は -z
        let nz = f32::from_le_bytes(out[92..96].try_into().unwrap());
        assert_eq!(nz, -1.0);
    }

    #[test]
    fn ascii_layout() {
        let mut out = Vec::new();
        write_ascii(&[tetra()], "part", &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.starts_with("solid part\n"));
        assert_eq!(s.matches("facet normal").count(), 4);
        assert!(s.trim_end().ends_with("endsolid part"));
    }
}
