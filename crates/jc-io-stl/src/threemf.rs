//! 三角形の網を 3MF(3D Manufacturing Format の Core 仕様)に書き出す。
//!
//! 3MF は zip の中に XML を入れた形。中身は 3 つだけ: 種類の表 `[Content_Types].xml`、入り口
//! `_rels/.rels`、形 `3D/3dmodel.model`。ボディごとに `object` を 1 つ作り、色は Core 仕様の
//! `basematerials`(`displaycolor`)で、単位は `model` の `unit` で書く(座標はその単位の値)。
//! 面に外観を付けた面の三角形は、その色を三角形の `pid`・`p1` で書く(3D プリンターのスライサーで
//! 面ごとに色を付け直さなくてよいように)。
//! 設計の属性は Core 仕様の決まった名前の `metadata`(Title・Designer・Description・Copyright・
//! LicenseTerms)に入れる。スライサー独自の色の書き方は書かない。
//!
//! 網は面ごとに頂点が分かれているので、位置で 1 つにまとめ、潰れた三角形を除いてから書く
//! (3MF は閉じた網を求め、辺は 2 つの三角形で共有する)。

use jc_core::{LengthUnit, Mesh};
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;

/// 書き出す 1 つのボディ。
#[derive(Debug, Clone)]
pub struct Body3mf {
    pub name: String,
    pub mesh: Mesh,
    /// 表示の色(外観の色、無ければ物理材料の色)
    pub color: [u8; 3],
    /// 色の名前(外観か物理材料の名前)
    pub material: String,
    /// 面の外観(面の名前 → 色の名前と色)。網の `face_names` の名前で引く
    pub faces: BTreeMap<String, (String, [u8; 3])>,
}

/// 3MF の `unit` の名前。
pub fn unit_name(u: LengthUnit) -> &'static str {
    match u {
        LengthUnit::Mm => "millimeter",
        LengthUnit::Cm => "centimeter",
        LengthUnit::M => "meter",
        LengthUnit::In => "inch",
        LengthUnit::Ft => "foot",
    }
}

/// XML の属性と本文に入れる字を逃がす。
fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            // XML 1.0 で書けない制御文字は落とす(改行とタブは残す)
            c if (c as u32) < 0x20 && !matches!(c, '\n' | '\r' | '\t') => {}
            c => o.push(c),
        }
    }
    o
}

/// 数を短く書く(小数 6 桁まで、末尾の 0 は落とす。`-0` は `0`)。
fn num(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() {
        "0".into()
    } else {
        s.to_string()
    }
}

/// 位置で頂点をまとめた網(頂点、三角形)。潰れた三角形(2 つ以上の頂点が同じ)は除く。
/// まとめる幅は 1e-4 mm(面ごとに分かれた同じ位置の頂点は同じ値なので、これで足りる)。
pub fn welded(m: &Mesh) -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
    let (verts, tris) = weld(m);
    (verts, tris.into_iter().map(|(_, t)| t).collect())
}

/// 元の網での番号を添えた三角形。
type SourcedTri = (usize, [u32; 3]);

/// [`welded`] と同じで、三角形に元の網での番号を添える(面の色を引くため)。
fn weld(m: &Mesh) -> (Vec<[f32; 3]>, Vec<SourcedTri>) {
    let mut map: HashMap<[i64; 3], u32> = HashMap::new();
    let mut verts = Vec::new();
    let id: Vec<u32> = m
        .positions
        .iter()
        .map(|p| {
            let key = p.map(|v| (f64::from(v) * 1e4).round() as i64);
            *map.entry(key).or_insert_with(|| {
                verts.push(*p);
                (verts.len() - 1) as u32
            })
        })
        .collect();
    let tris = m
        .triangles
        .iter()
        .map(|t| t.map(|i| id[i as usize]))
        .enumerate()
        .filter(|(_, t)| t[0] != t[1] && t[1] != t[2] && t[0] != t[2])
        .collect();
    (verts, tris)
}

/// 形の XML(`3D/3dmodel.model`)。`meta` は(Core 仕様の名前、値)。空の値は書かない。
pub fn model_xml(bodies: &[Body3mf], unit: LengthUnit, meta: &[(&str, String)]) -> String {
    let k = 1.0 / unit.mm();
    let mut o = String::new();
    o.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let _ = writeln!(
        o,
        "<model unit=\"{}\" xml:lang=\"ja-JP\" xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\">",
        unit_name(unit)
    );
    for (name, value) in meta {
        if !value.trim().is_empty() {
            let _ = writeln!(
                o,
                " <metadata name=\"{}\">{}</metadata>",
                esc(name),
                esc(value.trim())
            );
        }
    }
    o.push_str(" <resources>\n");
    // 色は 1 つの basematerials に並べ、同じ名前と色は 1 つにする
    let mut bases: Vec<(String, [u8; 3])> = Vec::new();
    let mut base_of = |key: (String, [u8; 3])| {
        bases.iter().position(|x| *x == key).unwrap_or_else(|| {
            bases.push(key);
            bases.len() - 1
        })
    };
    let index: Vec<usize> = bodies
        .iter()
        .map(|b| base_of((b.material.clone(), b.color)))
        .collect();
    // 面の外観の色(ボディごとに、網の面の番号 → 色の番号)
    let face_index: Vec<HashMap<u32, usize>> = bodies
        .iter()
        .map(|b| {
            b.mesh
                .face_names
                .iter()
                .enumerate()
                .filter_map(|(f, name)| {
                    let (m, c) = b.faces.get(name)?;
                    Some((f as u32, base_of((m.clone(), *c))))
                })
                .collect()
        })
        .collect();
    o.push_str("  <basematerials id=\"1\">\n");
    for (name, c) in &bases {
        let _ = writeln!(
            o,
            "   <base name=\"{}\" displaycolor=\"#{:02X}{:02X}{:02X}FF\"/>",
            esc(name),
            c[0],
            c[1],
            c[2]
        );
    }
    o.push_str("  </basematerials>\n");
    for (i, b) in bodies.iter().enumerate() {
        let (verts, tris) = weld(&b.mesh);
        let _ = writeln!(
            o,
            "  <object id=\"{}\" type=\"model\" name=\"{}\" pid=\"1\" pindex=\"{}\">",
            i + 2,
            esc(&b.name),
            index[i]
        );
        o.push_str("   <mesh>\n    <vertices>\n");
        for p in &verts {
            let _ = writeln!(
                o,
                "     <vertex x=\"{}\" y=\"{}\" z=\"{}\"/>",
                num(f64::from(p[0]) * k),
                num(f64::from(p[1]) * k),
                num(f64::from(p[2]) * k)
            );
        }
        o.push_str("    </vertices>\n    <triangles>\n");
        for (src, t) in &tris {
            let face = b.mesh.triangle_faces.get(*src);
            match face.and_then(|f| face_index[i].get(f)) {
                Some(p) => {
                    let _ = writeln!(
                        o,
                        "     <triangle v1=\"{}\" v2=\"{}\" v3=\"{}\" pid=\"1\" p1=\"{p}\"/>",
                        t[0], t[1], t[2]
                    );
                }
                None => {
                    let _ = writeln!(
                        o,
                        "     <triangle v1=\"{}\" v2=\"{}\" v3=\"{}\"/>",
                        t[0], t[1], t[2]
                    );
                }
            }
        }
        o.push_str("    </triangles>\n   </mesh>\n  </object>\n");
    }
    o.push_str(" </resources>\n <build>\n");
    for i in 0..bodies.len() {
        let _ = writeln!(o, "  <item objectid=\"{}\"/>", i + 2);
    }
    o.push_str(" </build>\n</model>\n");
    o
}

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
 <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
 <Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/>
</Types>
"#;

const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
 <Relationship Target="/3D/3dmodel.model" Id="rel0" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/>
</Relationships>
"#;

/// 3MF のファイルの中身(zip)。
pub fn write_3mf(bodies: &[Body3mf], unit: LengthUnit, meta: &[(&str, String)]) -> Vec<u8> {
    let model = model_xml(bodies, unit, meta);
    jc_format::zip::write(&[
        ("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
        ("_rels/.rels", RELS.as_bytes()),
        ("3D/3dmodel.model", model.as_bytes()),
    ])
}

/// [`write_3mf`] が書いた 3MF(zip)から、名前 `part` の中身を取り出す(読み直しと試験に使う。
/// 中央の目録は見ず、頭から順に読むので、ほかのソフトが書いた 3MF は読めないことがある)。
pub fn unzip_part(b: &[u8], part: &str) -> Option<Vec<u8>> {
    jc_format::zip::read_written(b, part)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 軸に平行な箱(面ごとに頂点を分けた、外向きの巻き)。
    fn boxed(lo: [f32; 3], hi: [f32; 3]) -> Mesh {
        let c = |i: usize| {
            [
                if i & 1 == 0 { lo[0] } else { hi[0] },
                if i & 2 == 0 { lo[1] } else { hi[1] },
                if i & 4 == 0 { lo[2] } else { hi[2] },
            ]
        };
        // 各面の角(外から見て反時計回り)
        let faces = [
            [0, 2, 3, 1],
            [4, 5, 7, 6],
            [0, 1, 5, 4],
            [2, 6, 7, 3],
            [0, 4, 6, 2],
            [1, 3, 7, 5],
        ];
        let mut m = Mesh {
            positions: vec![],
            normals: vec![],
            triangles: vec![],
            triangle_faces: vec![],
            face_names: vec![],
            edges: vec![],
        };
        for (k, f) in faces.iter().enumerate() {
            let base = m.positions.len() as u32;
            for i in f {
                m.positions.push(c(*i));
                m.normals.push([0.0; 3]);
            }
            m.triangles.push([base, base + 1, base + 2]);
            m.triangles.push([base, base + 2, base + 3]);
            m.triangle_faces.extend([k as u32; 2]);
            m.face_names.push(format!("f{k}"));
        }
        m
    }

    fn body(name: &str, mesh: Mesh, color: [u8; 3], material: &str) -> Body3mf {
        Body3mf {
            name: name.into(),
            mesh,
            color,
            material: material.into(),
            faces: BTreeMap::new(),
        }
    }

    #[test]
    fn a_face_with_its_own_appearance_writes_its_triangles_in_that_color() {
        let mut b = body("板", boxed([0.0; 3], [10.0; 3]), [140, 140, 140], "鋼");
        b.faces.insert("f1".into(), ("赤".into(), [200, 70, 64]));
        // 網に無い面の名前(形を直して消えた面)は書かず、ほかの面に付け替えない
        b.faces.insert("消えた面".into(), ("青".into(), [0, 0, 255]));
        let m = model_xml(&[b], LengthUnit::Mm, &[]);
        assert_eq!(m.matches("<base ").count(), 2, "{m}");
        assert!(m.contains("<base name=\"赤\" displaycolor=\"#C84640FF\"/>"));
        assert!(!m.contains("青"));
        // f1 の 2 つの三角形だけが赤(1 番)、ほかはボディの色(object の pindex)のまま
        assert_eq!(m.matches("pid=\"1\" p1=\"1\"").count(), 2, "{m}");
        assert_eq!(m.matches("<triangle ").count(), 12);
    }

    #[test]
    fn a_box_is_welded_into_8_vertices_and_12_triangles_with_every_edge_shared_twice() {
        let (v, t) = welded(&boxed([0.0; 3], [10.0, 20.0, 5.0]));
        assert_eq!((v.len(), t.len()), (8, 12));
        let mut uses: HashMap<(u32, u32), i32> = HashMap::new();
        for tri in &t {
            for j in 0..3 {
                // 向きのある辺: 向かい合う三角形は逆向きに通る
                *uses.entry((tri[j], tri[(j + 1) % 3])).or_default() += 1;
            }
        }
        for ((a, b), n) in &uses {
            assert_eq!(*n, 1, "辺 {a}-{b} を同じ向きに 2 回通る");
            assert_eq!(uses.get(&(*b, *a)), Some(&1), "辺 {a}-{b} の向かいが無い");
        }
    }

    #[test]
    fn the_package_has_the_three_parts_and_the_model_keeps_units_colors_and_properties() {
        let bodies = [
            body(
                "板",
                boxed([0.0; 3], [25.4, 50.8, 2.54]),
                [200, 70, 64],
                "赤",
            ),
            body("脚 & 台", boxed([0.0; 3], [1.0; 3]), [140, 140, 140], "鋼"),
            body("脚2", boxed([2.0; 3], [3.0; 3]), [140, 140, 140], "鋼"),
        ];
        let meta = [
            ("Title", "棚 <試作>".to_string()),
            ("Designer", "Kie".to_string()),
            ("Description", String::new()),
        ];
        let z = write_3mf(&bodies, LengthUnit::In, &meta);
        assert!(unzip_part(&z, "[Content_Types].xml").is_some());
        let rels = String::from_utf8(unzip_part(&z, "_rels/.rels").unwrap()).unwrap();
        assert!(rels.contains("Target=\"/3D/3dmodel.model\""));
        let m = String::from_utf8(unzip_part(&z, "3D/3dmodel.model").unwrap()).unwrap();
        assert!(m.contains("unit=\"inch\""));
        // 25.4 mm の辺は 1 in で書く
        assert!(m.contains("x=\"1\" y=\"2\" z=\"0.1\""), "{m}");
        assert!(m.contains("<metadata name=\"Title\">棚 &lt;試作&gt;</metadata>"));
        assert!(m.contains("<metadata name=\"Designer\">Kie</metadata>"));
        assert!(!m.contains("Description"), "空の属性は書かない");
        // 色は 2 つ(鋼は 1 つにまとめる)、ボディは 3 つ
        assert_eq!(m.matches("<base ").count(), 2);
        assert!(m.contains("<base name=\"赤\" displaycolor=\"#C84640FF\"/>"));
        assert!(m.contains("name=\"脚 &amp; 台\" pid=\"1\" pindex=\"1\""));
        assert!(m.contains("name=\"脚2\" pid=\"1\" pindex=\"1\""));
        assert_eq!(m.matches("<item objectid=").count(), 3);
        assert_eq!(m.matches("<triangle ").count(), 36);
    }

    #[test]
    fn numbers_are_short_and_never_negative_zero() {
        assert_eq!(num(-0.0), "0");
        assert_eq!(num(-0.0000001), "0");
        assert_eq!(num(1.5), "1.5");
        assert_eq!(num(10.0), "10");
        assert_eq!(num(0.1234567), "0.123457");
    }
}
