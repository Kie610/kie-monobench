//! ボディの網を glTF 2.0 のバイナリ(.glb)に書く。
//!
//! 1 つのファイルに、JSON の塊(形の並びと材質)と BIN の塊(頂点と索引)を入れる。
//! ボディごとに node・mesh・material(pbrMetallicRoughness)を 1 つずつ作る。
//!
//! - 長さ: JC は mm、glTF は m なので 0.001 倍して書く。
//! - 向き: JC は Z が上、glTF は Y が上。頂点はそのまま書き、根の node に X 軸まわり -90° の回転を入れる
//!   (JC の上 +Z が glTF の上 +Y に、JC の前 -Y が glTF の手前 +Z になる)。
//! - 色: 受け取る色は sRGB の 0〜1。glTF の baseColorFactor は線形なので直して書く。
//! - 法線: 網の法線を使う。無い・長さ 0 の頂点は、まわりの三角形の法線(面積の重みつき)で補う。
//! - 塊の長さは 4 バイトの倍数にそろえる(JSON は空白、BIN は 0 で詰める)。

use kmb_core::Mesh;
use serde_json::{Value, json};
use std::io::{self, Write};

/// 書き出すボディ 1 つ。
#[derive(Debug, Clone)]
pub struct GltfBody {
    pub name: String,
    pub mesh: Mesh,
    /// sRGB の 0〜1 の値(RGBA)。アルファが 1 より小さければ透けるように書く。
    pub color: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
}

const GLB_MAGIC: u32 = 0x4654_6C67; // "glTF"
const CHUNK_JSON: u32 = 0x4E4F_534A; // "JSON"
const CHUNK_BIN: u32 = 0x004E_4942; // "BIN\0"
const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;
const FLOAT: u32 = 5126;
const UNSIGNED_INT: u32 = 5125;
const MM_TO_M: f32 = 0.001;

/// sRGB の値(0〜1)を線形に直す。
pub fn srgb_to_linear(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn invalid(msg: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, msg)
}

/// 頂点ごとの法線。網の法線が使えればそれを、使えなければ三角形の法線を足した向きを使う。
fn vertex_normals(m: &Mesh) -> Vec<[f32; 3]> {
    let n = m.positions.len();
    // 三角形の法線(外積。長さが面積の 2 倍なので、足すと面積の重みになる)を頂点に足す
    let mut acc = vec![[0.0f32; 3]; n];
    for t in &m.triangles {
        let p = |i: u32| m.positions[i as usize];
        let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let f = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        for &i in t {
            for k in 0..3 {
                acc[i as usize][k] += f[k];
            }
        }
    }
    let unit = |v: [f32; 3]| {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        (l > 1e-20 && l.is_finite()).then(|| [v[0] / l, v[1] / l, v[2] / l])
    };
    (0..n)
        .map(|i| {
            m.normals
                .get(i)
                .and_then(|&v| unit(v))
                .or_else(|| unit(acc[i]))
                // どの三角形にも使われない頂点など。上向きにしておく
                .unwrap_or([0.0, 0.0, 1.0])
        })
        .collect()
}

fn push_f32s(bin: &mut Vec<u8>, vs: &[[f32; 3]]) {
    for v in vs {
        for x in v {
            bin.extend_from_slice(&x.to_le_bytes());
        }
    }
}

/// `bodies` を 1 つの .glb に書く。三角形の無いボディは飛ばす。
pub fn write_glb(bodies: &[GltfBody], mut w: impl Write) -> io::Result<()> {
    let mut bin: Vec<u8> = Vec::new();
    let mut views: Vec<Value> = Vec::new();
    let mut accessors: Vec<Value> = Vec::new();
    let mut meshes: Vec<Value> = Vec::new();
    let mut materials: Vec<Value> = Vec::new();
    let mut nodes: Vec<Value> = vec![Value::Null]; // 0 番は根。最後に埋める
    let mut children: Vec<usize> = Vec::new();

    for b in bodies {
        let m = &b.mesh;
        if m.triangles.is_empty() || m.positions.is_empty() {
            continue;
        }
        let count = m.positions.len();
        if let Some(t) = m
            .triangles
            .iter()
            .find(|t| t.iter().any(|&i| i as usize >= count))
        {
            return Err(invalid(format!(
                "「{}」の三角形 {:?} が、無い頂点を指しています(頂点は {count} 個)",
                b.name, t
            )));
        }
        let positions: Vec<[f32; 3]> = m
            .positions
            .iter()
            .map(|p| [p[0] * MM_TO_M, p[1] * MM_TO_M, p[2] * MM_TO_M])
            .collect();
        if positions.iter().flatten().any(|x| !x.is_finite()) {
            return Err(invalid(format!(
                "「{}」の頂点に数でない値があります",
                b.name
            )));
        }
        let normals = vertex_normals(m);
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for p in &positions {
            for k in 0..3 {
                min[k] = min[k].min(p[k]);
                max[k] = max[k].max(p[k]);
            }
        }

        // 頂点・法線・索引の順に BIN へ足す(どれも 4 バイトの倍数なので詰め物は要らない)
        let pos_view = views.len();
        let start = bin.len();
        push_f32s(&mut bin, &positions);
        views.push(json!({"buffer": 0, "byteOffset": start, "byteLength": bin.len() - start, "target": ARRAY_BUFFER}));
        let start = bin.len();
        push_f32s(&mut bin, &normals);
        views.push(json!({"buffer": 0, "byteOffset": start, "byteLength": bin.len() - start, "target": ARRAY_BUFFER}));
        let start = bin.len();
        for t in &m.triangles {
            for i in t {
                bin.extend_from_slice(&i.to_le_bytes());
            }
        }
        views.push(json!({"buffer": 0, "byteOffset": start, "byteLength": bin.len() - start, "target": ELEMENT_ARRAY_BUFFER}));

        let acc = accessors.len();
        accessors.push(json!({
            "bufferView": pos_view, "componentType": FLOAT, "count": count, "type": "VEC3",
            "min": min, "max": max,
        }));
        accessors.push(json!({"bufferView": pos_view + 1, "componentType": FLOAT, "count": count, "type": "VEC3"}));
        accessors.push(json!({
            "bufferView": pos_view + 2, "componentType": UNSIGNED_INT,
            "count": m.triangles.len() * 3, "type": "SCALAR",
        }));

        let [r, g, bl, a] = b.color;
        let alpha = if a.is_finite() {
            a.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let mut material = json!({
            "name": b.name,
            "pbrMetallicRoughness": {
                "baseColorFactor": [srgb_to_linear(r), srgb_to_linear(g), srgb_to_linear(bl), alpha],
                "metallicFactor": b.metallic.clamp(0.0, 1.0),
                "roughnessFactor": b.roughness.clamp(0.0, 1.0),
            },
        });
        if alpha < 1.0 {
            material["alphaMode"] = json!("BLEND");
        }
        materials.push(material);
        meshes.push(json!({
            "name": b.name,
            "primitives": [{
                "attributes": {"POSITION": acc, "NORMAL": acc + 1},
                "indices": acc + 2,
                "material": materials.len() - 1,
                "mode": 4,
            }],
        }));
        children.push(nodes.len());
        nodes.push(json!({"name": b.name, "mesh": meshes.len() - 1}));
    }

    // 根: X 軸まわりに -90°(四元数 x, y, z, w)。JC の Z 上を glTF の Y 上にする
    let h = std::f32::consts::FRAC_1_SQRT_2;
    let mut root = json!({"name": "kieMonoBench", "rotation": [-h, 0.0, 0.0, h]});
    if !children.is_empty() {
        root["children"] = json!(children);
    }
    nodes[0] = root;

    let mut doc = json!({
        "asset": {"version": "2.0", "generator": "kieMonoBench"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": nodes,
    });
    // glTF の配列は空にできないので、ボディが無い時は書かない
    if !meshes.is_empty() {
        doc["meshes"] = json!(meshes);
        doc["materials"] = json!(materials);
        doc["accessors"] = json!(accessors);
        doc["bufferViews"] = json!(views);
        doc["buffers"] = json!([{"byteLength": bin.len()}]);
    }

    let mut json_bytes = serde_json::to_vec(&doc).map_err(io::Error::other)?;
    while !json_bytes.len().is_multiple_of(4) {
        json_bytes.push(b' ');
    }
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let bin_chunk = if bin.is_empty() { 0 } else { 8 + bin.len() };
    let total = 12 + 8 + json_bytes.len() + bin_chunk;
    let total = u32::try_from(total).map_err(|_| invalid("glTF が 4 GB を超えます".into()))?;

    w.write_all(&GLB_MAGIC.to_le_bytes())?;
    w.write_all(&2u32.to_le_bytes())?;
    w.write_all(&total.to_le_bytes())?;
    w.write_all(&(json_bytes.len() as u32).to_le_bytes())?;
    w.write_all(&CHUNK_JSON.to_le_bytes())?;
    w.write_all(&json_bytes)?;
    if !bin.is_empty() {
        w.write_all(&(bin.len() as u32).to_le_bytes())?;
        w.write_all(&CHUNK_BIN.to_le_bytes())?;
        w.write_all(&bin)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 三角柱(底は XY 平面、高さ 10 mm)。法線は空にして、補う道を通す。
    fn prism(dx: f32) -> Mesh {
        let base = [[0.0, 0.0], [20.0, 0.0], [0.0, 30.0]];
        let mut positions = Vec::new();
        for z in [0.0, 10.0] {
            for b in base {
                positions.push([b[0] + dx, b[1], z]);
            }
        }
        Mesh {
            positions,
            normals: Vec::new(),
            triangles: vec![
                [0, 2, 1],
                [3, 4, 5],
                [0, 1, 4],
                [0, 4, 3],
                [1, 2, 5],
                [1, 5, 4],
                [2, 0, 3],
                [2, 3, 5],
            ],
            ..Default::default()
        }
    }

    struct Glb {
        json: Value,
        bin: Vec<u8>,
    }

    /// 書いた .glb を自前で読み直す(枠の決まりもここで確かめる)。
    fn read_glb(bytes: &[u8]) -> Glb {
        let u32_at = |i: usize| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
        assert_eq!(u32_at(0), GLB_MAGIC);
        assert_eq!(u32_at(4), 2);
        assert_eq!(u32_at(8) as usize, bytes.len());
        assert_eq!(bytes.len() % 4, 0);
        let jlen = u32_at(12) as usize;
        assert_eq!(jlen % 4, 0);
        assert_eq!(u32_at(16), CHUNK_JSON);
        let json: Value = serde_json::from_slice(&bytes[20..20 + jlen]).unwrap();
        let mut bin = Vec::new();
        let at = 20 + jlen;
        if at < bytes.len() {
            let blen = u32_at(at) as usize;
            assert_eq!(blen % 4, 0);
            assert_eq!(u32_at(at + 4), CHUNK_BIN);
            bin = bytes[at + 8..at + 8 + blen].to_vec();
            assert_eq!(at + 8 + blen, bytes.len());
        }
        Glb { json, bin }
    }

    fn floats(g: &Glb, accessor: usize) -> Vec<f32> {
        let a = &g.json["accessors"][accessor];
        let v = &g.json["bufferViews"][a["bufferView"].as_u64().unwrap() as usize];
        let off = v["byteOffset"].as_u64().unwrap() as usize;
        let len = v["byteLength"].as_u64().unwrap() as usize;
        g.bin[off..off + len]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| f32::from_le_bytes(*c))
            .collect()
    }

    fn body(name: &str, mesh: Mesh) -> GltfBody {
        GltfBody {
            name: name.into(),
            mesh,
            color: [0.5, 0.5, 0.5, 1.0],
            metallic: 0.2,
            roughness: 0.6,
        }
    }

    #[test]
    fn two_bodies_are_written_and_read_back() {
        let bodies = [body("台", prism(0.0)), body("ふた", prism(100.0))];
        let mut out = Vec::new();
        write_glb(&bodies, &mut out).unwrap();
        let g = read_glb(&out);
        let j = &g.json;
        assert_eq!(j["asset"]["version"], "2.0");
        assert_eq!(j["asset"]["generator"], "kieMonoBench");
        assert_eq!(j["nodes"].as_array().unwrap().len(), 3);
        assert_eq!(j["nodes"][0]["children"], json!([1, 2]));
        let r = j["nodes"][0]["rotation"].as_array().unwrap();
        assert!((r[0].as_f64().unwrap() + 0.70710677).abs() < 1e-6);
        assert!((r[3].as_f64().unwrap() - 0.70710677).abs() < 1e-6);
        assert_eq!(j["meshes"].as_array().unwrap().len(), 2);
        assert_eq!(
            j["buffers"][0]["byteLength"].as_u64().unwrap() as usize,
            g.bin.len()
        );

        // 2 つ目のボディ: 頂点 6 個、索引 24 個、min/max は m
        let pos = &j["accessors"][3];
        assert_eq!(pos["count"], 6);
        let close = |v: &Value, e: [f32; 3]| {
            let v: Vec<f64> = v
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_f64().unwrap())
                .collect();
            for k in 0..3 {
                assert!((v[k] - e[k] as f64).abs() < 1e-6, "{v:?} {e:?}");
            }
        };
        close(&pos["min"], [0.1, 0.0, 0.0]);
        close(&pos["max"], [0.12, 0.03, 0.01]);
        let p = floats(&g, 3);
        assert_eq!(p.len(), 18);
        assert!((p[3] - 0.12).abs() < 1e-7);

        // 索引は u32 で、元の三角形と同じ
        let ia = &j["accessors"][5];
        assert_eq!(ia["componentType"], UNSIGNED_INT);
        assert_eq!(ia["count"], 24);
        let v = &j["bufferViews"][ia["bufferView"].as_u64().unwrap() as usize];
        let off = v["byteOffset"].as_u64().unwrap() as usize;
        let idx: Vec<u32> = g.bin[off..off + 96]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| u32::from_le_bytes(*c))
            .collect();
        let expected: Vec<u32> = prism(0.0).triangles.iter().flatten().copied().collect();
        assert_eq!(idx, expected);

        // 法線は補われ、長さ 1。底の頂点 0 は下・横の面に囲まれるので z は負
        let n = floats(&g, 1);
        for c in n.as_chunks::<3>().0.iter() {
            let l = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
            assert!((l - 1.0).abs() < 1e-5, "{c:?}");
        }
        assert!(n[2] < 0.0, "{:?}", &n[..3]);
    }

    #[test]
    fn mesh_normals_are_used_and_colour_is_made_linear() {
        let mut m = prism(0.0);
        m.normals = vec![[0.0, 0.0, 2.0]; 6];
        m.normals[1] = [0.0; 3]; // 長さ 0 は三角形の法線で補う
        let mut b = body("x", m);
        b.color = [1.0, 0.5, 0.0, 0.5];
        let mut out = Vec::new();
        write_glb(&[b], &mut out).unwrap();
        let g = read_glb(&out);
        let n = floats(&g, 1);
        assert_eq!(&n[..3], &[0.0, 0.0, 1.0]);
        assert!(n[3..6] != [0.0, 0.0, 1.0]);
        let mat = &g.json["materials"][0];
        let c = mat["pbrMetallicRoughness"]["baseColorFactor"]
            .as_array()
            .unwrap();
        assert!((c[0].as_f64().unwrap() - 1.0).abs() < 1e-6);
        assert!((c[1].as_f64().unwrap() - 0.21404).abs() < 1e-4);
        assert_eq!(c[3].as_f64().unwrap(), 0.5);
        assert_eq!(mat["alphaMode"], "BLEND");
    }

    #[test]
    fn no_bodies_gives_a_file_without_a_bin_chunk() {
        let mut out = Vec::new();
        write_glb(&[body("空", Mesh::default())], &mut out).unwrap();
        let g = read_glb(&out);
        assert!(g.bin.is_empty());
        assert!(g.json.get("buffers").is_none());
        assert_eq!(g.json["nodes"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn a_bad_index_is_an_error() {
        let mut m = prism(0.0);
        m.triangles.push([0, 1, 99]);
        let e = write_glb(&[body("壊れ", m)], Vec::new()).unwrap_err();
        assert!(e.to_string().contains("壊れ"), "{e}");
    }

    /// 手で中身を見る時: `JC_GLB_OUT=a.glb cargo test -p kmb-io-render glb_for_hand_check -- --ignored`
    /// (ignore にして、何もしない回を検査の「通過」に数えない)
    #[test]
    #[ignore = "道具: JC_GLB_OUT に手で見る glb を書く"]
    fn glb_for_hand_check() {
        let path = std::env::var("JC_GLB_OUT").expect("書き出し先を JC_GLB_OUT に入れる");
        let mut b = body("台", prism(0.0));
        b.color = [0.8, 0.3, 0.2, 1.0];
        let file = std::fs::File::create(path).unwrap();
        write_glb(&[b, body("ふた", prism(40.0))], file).unwrap();
    }
}
