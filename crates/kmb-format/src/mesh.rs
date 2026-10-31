//! 三角形の網の読み手と、文書に埋める小さなバイナリ形式。座標はファイルの単位で持つ。

use std::collections::BTreeMap;

mod threemf;
pub use threemf::read_3mf;

pub const MAX_TRIANGLES: usize = 5_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Micron,
    Millimeter,
    Centimeter,
    Meter,
    Inch,
    Foot,
}

impl Unit {
    pub fn mm(self) -> f64 {
        match self {
            Self::Micron => 0.001,
            Self::Millimeter => 1.0,
            Self::Centimeter => 10.0,
            Self::Meter => 1000.0,
            Self::Inch => 25.4,
            Self::Foot => 304.8,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Geometry {
    pub vertices: Vec<[f32; 3]>,
    pub triangles: Vec<[u32; 3]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Object {
    pub name: String,
    pub unit: Unit,
    pub geometry: Geometry,
    pub notes: Vec<String>,
}

pub fn check_count(n: usize) -> Result<(), String> {
    if n > MAX_TRIANGLES {
        Err(format!(
            "三角形が {n} あります。5,000,000 以下に減らしてから読み込んでください"
        ))
    } else {
        Ok(())
    }
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}
fn key(p: [f32; 3]) -> [u32; 3] {
    p.map(|x| if x == 0.0 { 0 } else { x.to_bits() })
}

/// 頂点番号の分かれに依存せず、位置が同じ辺の使用回数と向きを調べる。
/// 戻り値は(閉じない辺の数、符号つき体積、表面積、重心)。
pub type MeshProperties = (usize, f64, f64, Option<[f64; 3]>);

/// 網の辺と量を位置から調べる。
pub fn properties(g: &Geometry) -> Result<MeshProperties, String> {
    check_count(g.triangles.len())?;
    if g.triangles.is_empty() || g.vertices.iter().flatten().any(|x| !x.is_finite()) {
        return Err("網の頂点か三角形が不正です".into());
    }
    let mut edges = BTreeMap::<([u32; 3], [u32; 3]), (usize, i32)>::new();
    // 網の近くを四面体の基準にする。大きな世界座標どうしの差し引きによる桁落ちを避ける。
    let origin = g
        .triangles
        .first()
        .and_then(|t| g.vertices.get(t[0] as usize))
        .copied()
        .unwrap_or([0.0; 3])
        .map(f64::from);
    let (mut volume, mut area, mut moment) = (0.0, 0.0, [0.0; 3]);
    for t in &g.triangles {
        let mut p = [[0.0; 3]; 3];
        for i in 0..3 {
            p[i] = g
                .vertices
                .get(t[i] as usize)
                .ok_or("網の頂点番号が範囲の外です")?
                .map(f64::from);
        }
        let c = cross(sub(p[1], p[0]), sub(p[2], p[0]));
        area += dot(c, c).sqrt() * 0.5;
        let relative = p.map(|p| sub(p, origin));
        let v = dot(relative[0], cross(relative[1], relative[2])) / 6.0;
        volume += v;
        for (i, m) in moment.iter_mut().enumerate() {
            *m += v * (relative[0][i] + relative[1][i] + relative[2][i]) / 4.0;
        }
        for i in 0..3 {
            let a = key(g.vertices[t[i] as usize]);
            let b = key(g.vertices[t[(i + 1) % 3] as usize]);
            let (edge, sign) = if a < b { ((a, b), 1) } else { ((b, a), -1) };
            let e = edges.entry(edge).or_default();
            e.0 += 1;
            e.1 += sign;
        }
    }
    let open = edges
        .values()
        .filter(|&&(count, direction)| count != 2 || direction != 0)
        .count();
    Ok((
        open,
        volume,
        area,
        (volume.abs() > 1e-30).then(|| std::array::from_fn(|i| origin[i] + moment[i] / volume)),
    ))
}

/// ビットが同じ頂点をまとめ、面積の無い三角形を除く。閉じた網だけ向きをそろえる。
pub fn clean(mut object: Object) -> Result<Object, String> {
    let g = &mut object.geometry;
    check_count(g.triangles.len())?;
    let mut map = BTreeMap::new();
    let mut remap = Vec::with_capacity(g.vertices.len());
    let mut vertices = Vec::new();
    for p in &g.vertices {
        if p.iter().any(|x| !x.is_finite()) {
            return Err("網に NaN または無限大の座標があります".into());
        }
        let id = *map.entry(key(*p)).or_insert_with(|| {
            let n = vertices.len() as u32;
            vertices.push(p.map(|x| if x == 0.0 { 0.0 } else { x }));
            n
        });
        remap.push(id);
    }
    let mut triangles = Vec::with_capacity(g.triangles.len());
    let mut removed = 0;
    for t in &g.triangles {
        let mut ids = [0; 3];
        for i in 0..3 {
            ids[i] = *remap
                .get(t[i] as usize)
                .ok_or("網の頂点番号が範囲の外です")?;
        }
        let [a, b, c] = ids.map(|i| vertices[i as usize].map(f64::from));
        let n = cross(sub(b, a), sub(c, a));
        if ids[0] == ids[1]
            || ids[1] == ids[2]
            || ids[2] == ids[0]
            || dot(n, n).sqrt() * 0.5 * object.unit.mm().powi(2) < 1e-12
        {
            removed += 1;
        } else {
            triangles.push(ids);
        }
    }
    *g = Geometry {
        vertices,
        triangles,
    };
    let (open, volume, _, _) = properties(g)?;
    if removed > 0 {
        object
            .notes
            .push(format!("面積の無い三角形 {removed} 枚を除きました"));
    }
    if open == 0 && volume < 0.0 {
        for t in &mut g.triangles {
            t.swap(1, 2);
        }
        object
            .notes
            .push("閉じた網の三角形を外向きにそろえました".into());
    }
    Ok(object)
}

/// 頭の字ではなく、三角形数とバイト数の一致を先に見て STL の形式を決める。
pub fn read_stl(bytes: &[u8], unit: Unit) -> Result<Vec<Object>, String> {
    if let Some(n) = bytes
        .get(80..84)
        .map(|b| u32::from_le_bytes(b.try_into().expect("4 bytes")) as usize)
        && n.checked_mul(50).and_then(|n| n.checked_add(84)) == Some(bytes.len())
    {
        check_count(n)?;
        let mut geometry = Geometry::default();
        for facet in bytes[84..].as_chunks::<50>().0 {
            let base = geometry.vertices.len() as u32;
            for p in facet[12..48].as_chunks::<12>().0 {
                geometry.vertices.push(std::array::from_fn(|i| {
                    f32::from_le_bytes(p[i * 4..i * 4 + 4].try_into().expect("4 bytes"))
                }));
            }
            geometry.triangles.push([base, base + 1, base + 2]);
        }
        return Ok(vec![clean(Object {
            name: String::new(),
            unit,
            geometry,
            notes: vec![],
        })?]);
    }
    let text =
        std::str::from_utf8(bytes).map_err(|_| "STL の三角形数とファイルの大きさが合いません")?;
    let mut lines = text
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .peekable();
    let mut out = Vec::new();
    let mut total = 0;
    while let Some(start) = lines.next() {
        let name = start
            .strip_prefix("solid")
            .filter(|s| s.is_empty() || s.starts_with(char::is_whitespace))
            .ok_or("STL の solid がありません")?
            .trim()
            .to_string();
        let mut g = Geometry::default();
        let mut ended = false;
        while let Some(line) = lines.next() {
            if line == "endsolid" || line.starts_with("endsolid ") {
                ended = true;
                break;
            }
            let f = line.split_whitespace().collect::<Vec<_>>();
            if f.len() != 5 || f[..2] != ["facet", "normal"] {
                return Err("STL の facet が不正です".into());
            }
            for n in &f[2..] {
                let x = n.parse::<f32>().map_err(|_| "STL の法線が不正です")?;
                if !x.is_finite() {
                    return Err("STL の法線が有限の数ではありません".into());
                }
            }
            if lines.next() != Some("outer loop") {
                return Err("STL の outer loop がありません".into());
            }
            total += 1;
            check_count(total)?;
            let base = g.vertices.len() as u32;
            for _ in 0..3 {
                let v = lines
                    .next()
                    .ok_or("STL の三角形が途中で切れています")?
                    .split_whitespace()
                    .collect::<Vec<_>>();
                if v.len() != 4 || v[0] != "vertex" {
                    return Err("STL の vertex が不正です".into());
                }
                let mut p = [0.0; 3];
                for i in 0..3 {
                    p[i] = v[i + 1].parse().map_err(|_| "STL の座標が不正です")?;
                }
                g.vertices.push(p);
            }
            if lines.next() != Some("endloop") || lines.next() != Some("endfacet") {
                return Err("STL の三角形が途中で切れています".into());
            }
            g.triangles.push([base, base + 1, base + 2]);
        }
        if !ended {
            return Err("STL の endsolid がありません".into());
        }
        out.push(clean(Object {
            name,
            unit,
            geometry: g,
            notes: vec![],
        })?);
    }
    if out.is_empty() {
        return Err("STL に三角形がありません".into());
    }
    Ok(out)
}

/// 単位を変える前の網を KMBMESH1・deflate・base64 の順で埋め込む。
pub fn encode(g: &Geometry) -> Result<String, String> {
    properties(g)?;
    let mut b = b"KMBMESH1".to_vec();
    b.extend((g.vertices.len() as u32).to_le_bytes());
    b.extend((g.triangles.len() as u32).to_le_bytes());
    for p in &g.vertices {
        for x in p {
            b.extend(x.to_le_bytes());
        }
    }
    for t in &g.triangles {
        for x in t {
            b.extend(x.to_le_bytes());
        }
    }
    Ok(crate::base64::encode(
        &miniz_oxide::deflate::compress_to_vec(&b, 6),
    ))
}

pub fn decode(text: &str) -> Result<Geometry, String> {
    let b = crate::base64::decode(text).ok_or("埋め込んだ網の base64 が不正です")?;
    let b = miniz_oxide::inflate::decompress_to_vec_with_limit(&b, 16 + MAX_TRIANGLES * 48)
        .map_err(|_| "埋め込んだ網を展開できません")?;
    if b.get(..8) != Some(b"KMBMESH1") || b.len() < 16 {
        return Err("埋め込んだ網の形式が違います".into());
    }
    let n = u32::from_le_bytes(b[8..12].try_into().expect("4 bytes")) as usize;
    let m = u32::from_le_bytes(b[12..16].try_into().expect("4 bytes")) as usize;
    check_count(m)?;
    if n > MAX_TRIANGLES * 3
        || 16usize
            .checked_add(n * 12)
            .and_then(|x| x.checked_add(m * 12))
            != Some(b.len())
    {
        return Err("埋め込んだ網の大きさが合いません".into());
    }
    let vertices = b[16..16 + n * 12]
        .as_chunks::<12>()
        .0
        .iter()
        .map(|p| {
            std::array::from_fn(|i| {
                f32::from_le_bytes(p[i * 4..i * 4 + 4].try_into().expect("4 bytes"))
            })
        })
        .collect();
    let triangles = b[16 + n * 12..]
        .as_chunks::<12>()
        .0
        .iter()
        .map(|p| {
            std::array::from_fn(|i| {
                u32::from_le_bytes(p[i * 4..i * 4 + 4].try_into().expect("4 bytes"))
            })
        })
        .collect();
    let g = Geometry {
        vertices,
        triangles,
    };
    properties(&g)?;
    Ok(g)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn closed_mesh_moments_do_not_depend_on_distance_from_origin() {
        let g = Geometry {
            vertices: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            triangles: vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
        };
        let mut moved = g.clone();
        for p in &mut moved.vertices {
            for x in p {
                *x += 1_000_000.;
            }
        }
        let (_, v, _, c) = properties(&moved).unwrap();
        assert!((v - 1. / 6.).abs() < 1e-12, "{v}");
        for x in c.unwrap() {
            assert!((x - 1_000_000.25).abs() < 1e-8, "{x}");
        }
    }
    fn facet() -> &'static str {
        "facet normal 0 0 1\nouter loop\nvertex -0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\n"
    }
    #[test]
    fn ascii_stl_keeps_solids_and_welds_exact_positions() {
        let text = format!(
            "solid A\n{}{}endsolid A\nsolid B\n{}endsolid B",
            facet(),
            facet(),
            facet()
        );
        let o = read_stl(text.as_bytes(), Unit::Inch).unwrap();
        assert_eq!(o.len(), 2);
        assert_eq!(o[0].name, "A");
        assert_eq!(o[0].geometry.vertices.len(), 3);
        assert_eq!(o[0].geometry.triangles.len(), 2);
        assert_eq!(o[0].unit, Unit::Inch);
        assert_eq!(
            decode(&encode(&o[0].geometry).unwrap()).unwrap(),
            o[0].geometry
        );
    }
    #[test]
    fn binary_stl_uses_length_even_with_solid_header_and_rejects_damage() {
        let mut b = vec![0; 80];
        b[..5].copy_from_slice(b"solid");
        b.extend(1u32.to_le_bytes());
        b.extend([0; 12]);
        for p in [[0f32, 0., 0.], [1., 0., 0.], [0., 1., 0.]] {
            for x in p {
                b.extend(x.to_le_bytes());
            }
        }
        b.extend([0; 2]);
        assert_eq!(
            read_stl(&b, Unit::Millimeter).unwrap()[0]
                .geometry
                .triangles
                .len(),
            1
        );
        b.pop();
        assert!(read_stl(&b, Unit::Millimeter).is_err());
        assert!(read_stl(b"solid\nendsolid", Unit::Millimeter).is_err());
        assert!(
            read_stl(
                format!("solid\n{}endsolid", facet().replace("1 0 0", "NaN 0 0")).as_bytes(),
                Unit::Millimeter
            )
            .is_err()
        );
    }
    #[test]
    fn cleaning_reports_degenerate_facets_and_reverses_only_closed_meshes() {
        let g = Geometry {
            vertices: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            triangles: vec![[0, 1, 2], [0, 3, 1], [0, 2, 3], [1, 3, 2], [0, 0, 1]],
        };
        let o = clean(Object {
            name: String::new(),
            unit: Unit::Millimeter,
            geometry: g,
            notes: vec![],
        })
        .unwrap();
        let (open, v, _, c) = properties(&o.geometry).unwrap();
        assert_eq!(open, 0);
        assert!((v - 1. / 6.).abs() < 1e-12);
        assert_eq!(o.notes.len(), 2);
        assert_eq!(c, Some([0.25; 3]));
    }
    #[test]
    fn threemf_reads_names_units_nested_parts_and_build_transforms() {
        let main=br#"<model unit="inch" xmlns:p="urn:part"><resources><object id="1" name="A &amp; B"><components><component objectid="7" p:path="/3D/part.model" transform="1 0 0 0 1 0 0 0 1 2 0 0"/></components></object><object id="2" type="support"/></resources><build><item objectid="1" transform="1 0 0 0 1 0 0 0 1 3 0 0"/><item objectid="2"/></build></model>"#;
        let part=br#"<model unit="inch"><resources><object id="7"><mesh><vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/></vertices><triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object></resources><build/></model>"#;
        let rel=br#"<Relationships><Relationship Type="urn:package/3dmodel" Target="/3D/main.model"/></Relationships>"#;
        let b = crate::zip::write(&[
            ("_rels/.rels", rel),
            ("3D/main.model", main),
            ("3D/part.model", part),
        ]);
        let o = read_3mf(&b).unwrap();
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].name, "A & B");
        assert_eq!(o[0].unit, Unit::Inch);
        assert_eq!(o[0].geometry.vertices[0], [5., 0., 0.]);
    }
    #[test]
    fn zip_reader_checks_central_directory_crc_and_descriptor_flags() {
        let b = crate::zip::write(&[("x", b"abc")]);
        assert_eq!(crate::zip::read(&b).unwrap()["x"], b"abc");
        let mut bad = b.clone();
        bad[31] ^= 1;
        assert!(crate::zip::read(&bad).is_err());
        let cd = b.windows(4).position(|x| x == b"PK\x01\x02").unwrap();
        let mut bad = b.clone();
        bad[cd + 8] |= 8;
        assert!(crate::zip::read(&bad).unwrap_err().contains("データ記述子"));
        let mut bad = b;
        bad[cd + 24..cd + 28].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(crate::zip::read(&bad).unwrap_err().contains("zip64"));
    }

    #[test]
    fn a_stored_zip_entry_and_resource_limits_are_checked() {
        let mut b = crate::zip::write(&[("x", b"abc")]);
        let old = b.windows(4).position(|x| x == b"PK\x01\x02").unwrap();
        b.splice(31..old, b"abc".iter().copied());
        let cd = 34;
        b[8..10].copy_from_slice(&0u16.to_le_bytes());
        b[18..22].copy_from_slice(&3u32.to_le_bytes());
        b[cd + 10..cd + 12].copy_from_slice(&0u16.to_le_bytes());
        b[cd + 20..cd + 24].copy_from_slice(&3u32.to_le_bytes());
        let end = b.len() - 22;
        b[end + 16..end + 20].copy_from_slice(&(cd as u32).to_le_bytes());
        assert_eq!(crate::zip::read(&b).unwrap()["x"], b"abc");
        assert!(
            check_count(MAX_TRIANGLES + 1)
                .unwrap_err()
                .contains("5,000,000")
        );
    }

    #[test]
    fn threemf_refuses_cyclic_components_and_external_entities() {
        let cyclic=br#"<model><resources><object id="1"><components><component objectid="1"/></components></object></resources><build><item objectid="1"/></build></model>"#;
        assert!(
            read_3mf(&crate::zip::write(&[("3D/3dmodel.model", cyclic)]))
                .unwrap_err()
                .contains("循環")
        );
        let declaration = br#"<!DOCTYPE model [<!ENTITY x SYSTEM "file:///missing">]><model/>"#;
        assert!(
            read_3mf(&crate::zip::write(&[("3D/3dmodel.model", declaration)]))
                .unwrap_err()
                .contains("宣言")
        );
    }

    #[test]
    fn threemf_skips_beam_lattice_objects_and_reports_their_kind() {
        let model=br#"<model xmlns:b="urn:beam"><resources><object id="1"><mesh><vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/></vertices><triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object><object id="2"><mesh><vertices/><b:beamlattice><b:beams/></b:beamlattice></mesh></object></resources><build><item objectid="1"/><item objectid="2"/></build></model>"#;
        let o = read_3mf(&crate::zip::write(&[("3D/3dmodel.model", model)])).unwrap();
        assert_eq!(o.len(), 1);
        assert!(o[0].notes.iter().any(|n| n.contains("beamlattice")));
    }

    #[test]
    fn reflected_threemf_components_keep_their_outward_winding() {
        let model=br#"<model><resources><object id="1"><mesh><vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/><vertex x="0" y="0" z="1"/></vertices><triangles><triangle v1="0" v2="2" v3="1"/><triangle v1="0" v2="1" v3="3"/><triangle v1="0" v2="3" v3="2"/><triangle v1="1" v2="2" v3="3"/></triangles></mesh></object><object id="2"><components><component objectid="1"/><component objectid="1" transform="-1 0 0 0 1 0 0 0 1 3 0 0"/></components></object></resources><build><item objectid="2"/></build></model>"#;
        let o = read_3mf(&crate::zip::write(&[("3D/3dmodel.model", model)])).unwrap();
        let (open, volume, _, _) = properties(&o[0].geometry).unwrap();
        assert_eq!(open, 0);
        assert!((volume - 1. / 3.).abs() < 1e-12, "{volume}");
    }
}
