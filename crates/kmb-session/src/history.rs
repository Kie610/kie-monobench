//! 版の履歴(候補 35)。保存のたびに、その時の文書を「版」として文書の横の隠しフォルダに残す。
//! 文書のファイルと保存形式は変えない(履歴は付録で、書けなくても保存は成功のまま)。
//!
//! 置き場所: `<文書のフォルダ>/.jc/history/<ファイル名>/`
//! - `index.json` … 版の一覧(古い順。日時・説明・節目の名前・親の版・戻した元の版)
//! - `<指紋>.json.z` … 版の中身(文書の JSON を zlib で固めた物。同じ中身は 1 つだけ)
//! - `<指紋>.png` … 保存した時の縮小図(画面と MCP の保存で作る。無くてもよい)
//!
//! 指紋は文書の JSON の FNV-1a 128 ビット(暗号ではない。同じ中身を 1 回だけ持つための名前)。
//! 版の一覧は親の版を持つ木の形にしておく(分岐は作らないが、後で足す時に形式を変えずに済む)。
//! `.` で始まるフォルダは「プロジェクトから探す」が飛ばすので、版がモデルとして出ることはない。

use kmb_core::Document;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 版 1 つ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Version {
    /// 中身の指紋(32 桁の 16 進)
    pub id: String,
    /// 1 から数える版の番号(残した順)
    pub number: u32,
    /// 前の版(初めの版は無し)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// 保存した時刻(UNIX 時刻の秒)
    pub saved_at: u64,
    /// 説明(例: 「保存」「版 3 に戻した」)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// 節目の名前(マイルストーン。空なら節目でない)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub milestone: String,
    /// 機能の数(一覧に出す)
    #[serde(default)]
    pub features: usize,
    /// 縮小図があるか
    #[serde(default)]
    pub thumbnail: bool,
    /// 保存した時の形の数(モデル管理の「形が近い」に使う。形が無い・古い版は無し)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<Shape>,
}

/// 形の数(体積・外形の箱・面の数)。似ているデザインを、絵を使わずに比べるための物。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Shape {
    /// 体積(mm³。ボディの合計)
    pub volume: f64,
    /// 外形の箱の 3 辺(mm。向きに左右されないよう、大きい順)
    pub size: [f64; 3],
    /// 面の数(ボディの合計)
    pub faces: usize,
}

impl Shape {
    /// `other` と、体積・外形の箱の各辺・面の数がどれも `tol`(割合。0.02 で 2%)以内か。
    pub fn near(&self, other: &Shape, tol: f64) -> bool {
        let close = |a: f64, b: f64| {
            let m = a.abs().max(b.abs());
            m <= f64::EPSILON || (a - b).abs() <= tol * m
        };
        close(self.volume, other.volume)
            && (0..3).all(|k| close(self.size[k], other.size[k]))
            && close(self.faces as f64, other.faces as f64)
    }
}

/// 形の数を網から数える。ボディが無いか体積が無ければ `None`。
pub fn shape_of(bodies: &[(kmb_core::FeatureId, &kmb_core::Mesh)]) -> Option<Shape> {
    let mut volume = 0.0;
    let mut faces = 0;
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for (_, m) in bodies {
        let at = |i: u32| m.positions[i as usize].map(f64::from);
        let mut v = 0.0;
        for t in &m.triangles {
            let (a, b, c) = (at(t[0]), at(t[1]), at(t[2]));
            // 原点と三角形で作る四面体の符号つきの体積の和(閉じた網なら中の体積)
            v += a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                + a[2] * (b[0] * c[1] - b[1] * c[0]);
        }
        volume += (v / 6.0).abs();
        let mut ids: Vec<u32> = m.triangle_faces.clone();
        ids.sort_unstable();
        ids.dedup();
        faces += ids.len();
        for p in &m.positions {
            for k in 0..3 {
                lo[k] = lo[k].min(f64::from(p[k]));
                hi[k] = hi[k].max(f64::from(p[k]));
            }
        }
    }
    if volume <= 0.0 {
        return None;
    }
    let mut size = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
    size.sort_by(|a, b| b.total_cmp(a));
    Some(Shape {
        volume,
        size,
        faces,
    })
}

/// 保存の時に版へ添える物(縮小図の PNG と形の数)。どちらも無くてよい。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub png: Option<Vec<u8>>,
    pub shape: Option<Shape>,
}

/// 縮小図と形の数を網から作る。
pub fn snapshot_of(bodies: &[(kmb_core::FeatureId, &kmb_core::Mesh)]) -> Snapshot {
    Snapshot {
        png: thumbnail_of(bodies),
        shape: shape_of(bodies),
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Index {
    versions: Vec<Version>,
}

/// `path` の文書の版を置くフォルダ。
pub fn dir(path: &Path) -> PathBuf {
    let parent = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .map_or_else(|| "無題".into(), |n| n.to_os_string());
    parent.join(".jc").join("history").join(name)
}

/// 文書の JSON の指紋(FNV-1a 128 ビット)。
pub fn fingerprint(json: &str) -> String {
    const OFFSET: u128 = 0x6c62272e07bb014262b821756295c58d;
    const PRIME: u128 = 0x0000000001000000000000000000013B;
    let mut h = OFFSET;
    for b in json.as_bytes() {
        h ^= u128::from(*b);
        h = h.wrapping_mul(PRIME);
    }
    format!("{h:032x}")
}

fn read_index(d: &Path) -> Index {
    std::fs::read_to_string(d.join("index.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_index(d: &Path, ix: &Index) -> Result<(), String> {
    let s = serde_json::to_string_pretty(ix).expect("JSON にできる");
    crate::write_atomic(&d.join("index.json"), &s, false)
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// 縮小図の大きさ(画素)。
pub const THUMB_SIZE: (u32, u32) = (256, 192);

/// 版の縮小図(PNG)を作る。等角から見た形。ボディが無ければ `None`。
pub fn thumbnail_of(bodies: &[(kmb_core::FeatureId, &kmb_core::Mesh)]) -> Option<Vec<u8>> {
    if bodies.is_empty() {
        return None;
    }
    let b: Vec<kmb_render::picture::Body> = bodies
        .iter()
        .map(|(f, m)| kmb_render::picture::Body {
            feature: *f,
            mesh: m,
        })
        .collect();
    let p = kmb_render::picture::render(
        &b,
        &kmb_render::picture::Options {
            width: THUMB_SIZE.0,
            height: THUMB_SIZE.1,
            ..Default::default()
        },
    );
    Some(p.png())
}

/// 縮小図の PNG を画素(幅・高さ・RGB)に戻す(画面に出す。このモジュールが書いた形だけ読める)。
pub fn decode_thumbnail(png: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    kmb_render::picture::Picture::from_png(png).map(|p| (p.width, p.height, p.rgb))
}

/// 版を残す。中身が最後の版と同じなら残さず `None`。`snap` は縮小図と形の数。
/// `note` は説明(空なら「保存」)。
pub fn record(
    path: &Path,
    doc: &Document,
    snap: &Snapshot,
    note: &str,
) -> Result<Option<Version>, String> {
    let json = doc.to_json();
    let id = fingerprint(&json);
    let d = dir(path);
    let mut ix = read_index(&d);
    if ix.versions.last().is_some_and(|v| v.id == id) {
        return Ok(None);
    }
    std::fs::create_dir_all(&d).map_err(|e| format!("{} を作れません: {e}", d.display()))?;
    let blob = d.join(format!("{id}.json.z"));
    if !blob.exists() {
        let z = miniz_oxide::deflate::compress_to_vec_zlib(json.as_bytes(), 6);
        std::fs::write(&blob, z).map_err(|e| format!("{} に書けません: {e}", blob.display()))?;
    }
    let has_thumb = match snap.png.as_deref() {
        Some(png) => std::fs::write(d.join(format!("{id}.png")), png).is_ok(),
        None => d.join(format!("{id}.png")).exists(),
    };
    let v = Version {
        id,
        number: ix.versions.last().map_or(1, |v| v.number + 1),
        parent: ix.versions.last().map(|v| v.id.clone()),
        saved_at: now(),
        note: if note.trim().is_empty() {
            "保存".into()
        } else {
            note.trim().into()
        },
        milestone: String::new(),
        features: doc.data().features.len(),
        thumbnail: has_thumb,
        shape: snap.shape,
    };
    ix.versions.push(v.clone());
    write_index(&d, &ix)?;
    log::info!("版 {} を残した: {}", v.number, path.display());
    Ok(Some(v))
}

/// 版の一覧(新しい順)。無ければ空。
pub fn list(path: &Path) -> Vec<Version> {
    let mut v = read_index(&dir(path)).versions;
    v.reverse();
    v
}

/// 版 `id` の文書。
pub fn read(path: &Path, id: &str) -> Result<Document, String> {
    let f = dir(path).join(format!("{id}.json.z"));
    let z = std::fs::read(&f).map_err(|e| format!("{} を読めません: {e}", f.display()))?;
    let raw = miniz_oxide::inflate::decompress_to_vec_zlib(&z)
        .map_err(|e| format!("{} がこわれています: {e:?}", f.display()))?;
    let s = String::from_utf8(raw).map_err(|_| format!("{} が文字ではありません", f.display()))?;
    Document::from_json(&s).map_err(|e| format!("{}: {e}", f.display()))
}

/// 版 `id` の縮小図(PNG)。
pub fn thumbnail(path: &Path, id: &str) -> Option<Vec<u8>> {
    std::fs::read(dir(path).join(format!("{id}.png"))).ok()
}

/// いちばん新しい縮小図(PNG)と、その版の保存の時刻。一覧に出す。
pub fn latest_thumbnail(path: &Path) -> Option<(Vec<u8>, u64)> {
    let d = dir(path);
    read_index(&d)
        .versions
        .iter()
        .rev()
        .filter(|v| v.thumbnail)
        .find_map(|v| {
            Some((
                std::fs::read(d.join(format!("{}.png", v.id))).ok()?,
                v.saved_at,
            ))
        })
}

/// いちばん新しい版の縮小図と形の数(分類だけ変えた版へ引き継ぐ。形は変わらないので)。
pub fn latest_snapshot(path: &Path) -> Snapshot {
    let shape = read_index(&dir(path))
        .versions
        .iter()
        .rev()
        .find_map(|v| v.shape);
    Snapshot {
        png: latest_thumbnail(path).map(|(png, _)| png),
        shape,
    }
}

/// 版 `id` を節目にする(名前を付ける)。空の名前なら節目を外す。
pub fn set_milestone(path: &Path, id: &str, name: &str) -> Result<(), String> {
    let d = dir(path);
    let mut ix = read_index(&d);
    let v = ix
        .versions
        .iter_mut()
        .find(|v| v.id == id)
        .ok_or_else(|| format!("版 {id} がありません"))?;
    v.milestone = name.trim().to_string();
    write_index(&d, &ix)
}

/// 2 つの版の違い 1 つ(版の見比べ。候補 37)。`before` が空なら足した物、`after` が空なら消した物。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Change {
    /// 「パラメータ」「機能」「属性」
    pub what: &'static str,
    pub name: String,
    pub before: String,
    pub after: String,
}

/// JSON の値を 1 行の短い文字にする(表に出す)。
fn brief(v: &serde_json::Value) -> String {
    let s = match v {
        serde_json::Value::String(s) => s.clone(),
        v => v.to_string(),
    };
    if s.chars().count() > 60 {
        format!("{}…", s.chars().take(59).collect::<String>())
    } else {
        s
    }
}

/// 機能の中身の違い(名前・抑制・データの鍵ごと)を「鍵: 値」の並びにする。
fn feature_diff(a: &kmb_core::FeatureEntry, b: &kmb_core::FeatureEntry) -> (String, String) {
    let (mut before, mut after) = (Vec::new(), Vec::new());
    if a.name != b.name {
        before.push(format!("名前: {}", a.name));
        after.push(format!("名前: {}", b.name));
    }
    if a.suppressed != b.suppressed {
        let s = |x: bool| if x { "抑制" } else { "有効" };
        before.push(s(a.suppressed).into());
        after.push(s(b.suppressed).into());
    }
    if a.type_id != b.type_id {
        before.push(a.type_id.clone());
        after.push(b.type_id.clone());
    }
    let none = serde_json::Map::new();
    let (ma, mb) = (
        a.data.as_object().unwrap_or(&none),
        b.data.as_object().unwrap_or(&none),
    );
    if a.data.is_object() && b.data.is_object() {
        let mut keys: Vec<&String> = ma.keys().chain(mb.keys()).collect();
        keys.sort();
        keys.dedup();
        for k in keys {
            let (x, y) = (ma.get(k), mb.get(k));
            if x != y {
                before.push(format!("{k}: {}", x.map_or("-".into(), brief)));
                after.push(format!("{k}: {}", y.map_or("-".into(), brief)));
            }
        }
    } else if a.data != b.data {
        before.push(brief(&a.data));
        after.push(brief(&b.data));
    }
    (before.join("、"), after.join("、"))
}

/// 版 `before` から `after` への違い(パラメータ・機能・属性の順。ID で突き合わせるので、
/// 名前を変えた物も同じ物として比べる)。形の違いは画面が重ねて見せる。
pub fn compare(before: &Document, after: &Document) -> Vec<Change> {
    let (a, b) = (before.data(), after.data());
    let mut out = Vec::new();
    for p in &b.params {
        match a.params.iter().find(|q| q.id == p.id) {
            None => out.push(Change {
                what: "パラメータ",
                name: p.name.clone(),
                before: String::new(),
                after: p.expr.clone(),
            }),
            Some(q) if q.name != p.name || q.expr != p.expr => out.push(Change {
                what: "パラメータ",
                name: if q.name == p.name {
                    p.name.clone()
                } else {
                    format!("{} → {}", q.name, p.name)
                },
                before: q.expr.clone(),
                after: p.expr.clone(),
            }),
            _ => {}
        }
    }
    for q in a
        .params
        .iter()
        .filter(|q| !b.params.iter().any(|p| p.id == q.id))
    {
        out.push(Change {
            what: "パラメータ",
            name: q.name.clone(),
            before: q.expr.clone(),
            after: String::new(),
        });
    }
    for f in &b.features {
        match a.features.iter().find(|g| g.id == f.id) {
            None => out.push(Change {
                what: "機能",
                name: f.name.clone(),
                before: String::new(),
                after: "足した".into(),
            }),
            Some(g) if g != f => {
                let (x, y) = feature_diff(g, f);
                out.push(Change {
                    what: "機能",
                    name: f.name.clone(),
                    before: x,
                    after: y,
                });
            }
            _ => {}
        }
    }
    for g in a
        .features
        .iter()
        .filter(|g| !b.features.iter().any(|f| f.id == g.id))
    {
        out.push(Change {
            what: "機能",
            name: g.name.clone(),
            before: "あった".into(),
            after: String::new(),
        });
    }
    let (pa, pb) = (&a.properties, &b.properties);
    for (key, label) in kmb_core::Properties::FIELDS {
        let (x, y) = (pa.get(key).unwrap_or(""), pb.get(key).unwrap_or(""));
        if x != y {
            out.push(Change {
                what: "属性",
                name: label.into(),
                before: x.into(),
                after: y.into(),
            });
        }
    }
    if pa.tags != pb.tags {
        out.push(Change {
            what: "属性",
            name: "タグ".into(),
            before: pa.tags_text(),
            after: pb.tags_text(),
        });
    }
    out
}

/// 版の形(3D のビューに重ねる、見えるボディの三角形と稜線)。
#[derive(Debug, Clone, Default)]
pub struct VersionShape {
    pub tris: Vec<[[f32; 3]; 3]>,
    pub edges: Vec<[[f32; 3]; 2]>,
}

/// 版の文書の形を、今の文書と切り離して計算する。見えるボディが無ければ `None`。
/// カーネルは今の文書と分けた物を渡す(分け合うと、キャッシュにない今の文書の形が捨てられる)。
pub fn version_shape(
    doc: &kmb_core::DocumentData,
    registry: &kmb_core::Registry,
    kernel: &dyn kmb_core::Kernel,
    tolerance: f64,
) -> Option<VersionShape> {
    let mut cache = kmb_core::Cache::default();
    let report = kmb_core::recompute_detached(doc, registry, kernel, &mut cache);
    let mut s = VersionShape::default();
    let bodies = kmb_core::visible_bodies(doc, registry, &report);
    for (_, h) in &bodies {
        let Ok(m) = kernel.mesh(*h, tolerance) else {
            continue;
        };
        let p = |i: u32| m.positions[i as usize];
        s.tris
            .extend(m.triangles.iter().map(|&[a, b, c]| [p(a), p(b), p(c)]));
        s.edges.extend(
            m.edges
                .iter()
                .flat_map(|l| l.windows(2).map(|w| [w[0], w[1]])),
        );
    }
    log::info!(
        "版の見比べ: 版の形を計算した(ボディ {}、三角形 {})",
        bodies.len(),
        s.tris.len()
    );
    (!s.tris.is_empty()).then_some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_kept_beside_the_file_once_per_content() {
        let base = std::env::temp_dir().join(format!("kmb-history-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let file = base.join("天板.json");
        let mut doc = Document::new();
        let a = record(&file, &doc, &Snapshot::default(), "")
            .unwrap()
            .expect("初めの版");
        assert_eq!((a.number, a.note.as_str()), (1, "保存"));
        // 同じ中身なら残さない
        assert!(
            record(&file, &doc, &Snapshot::default(), "")
                .unwrap()
                .is_none()
        );
        // 変えたら次の版。親は前の版
        let p = doc.alloc_param_id();
        doc.apply(kmb_core::Transaction::new(
            "足す",
            vec![kmb_core::Op::AddParam {
                param: kmb_core::Param::new(p, "width", "60 mm"),
                index: None,
            }],
        ))
        .unwrap();
        let b = record(
            &file,
            &doc,
            &Snapshot {
                png: Some(b"png".to_vec()),
                shape: None,
            },
            "幅を足した",
        )
        .unwrap()
        .unwrap();
        assert_eq!(b.number, 2);
        assert_eq!(b.parent.as_deref(), Some(a.id.as_str()));
        assert!(b.thumbnail);
        // 前の版に戻すと、同じ中身の版が新しい番号で増える(中身のファイルは 1 つのまま)
        let old = read(&file, &a.id).unwrap();
        assert!(old.data().params.is_empty());
        let c = record(&file, &old, &Snapshot::default(), "版 1 に戻した")
            .unwrap()
            .unwrap();
        assert_eq!((c.number, c.id.clone()), (3, a.id.clone()));
        let blobs = std::fs::read_dir(dir(&file))
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".json.z"))
            .count();
        assert_eq!(blobs, 2);
        // 一覧は新しい順。節目の名前を付けて外せる
        let l = list(&file);
        assert_eq!(
            l.iter().map(|v| v.number).collect::<Vec<_>>(),
            vec![3, 2, 1]
        );
        set_milestone(&file, &b.id, " 初めて削れた ").unwrap();
        assert_eq!(list(&file)[1].milestone, "初めて削れた");
        assert_eq!(latest_thumbnail(&file).unwrap().0, b"png");
        set_milestone(&file, &b.id, "").unwrap();
        assert!(list(&file)[1].milestone.is_empty());
        // 版は隠しフォルダにあるので、プロジェクトの一覧にはモデルとして出ない
        // 版の見比べ: 版 1 → 版 2 でパラメータを足した。逆向きなら消した
        let d12 = compare(&read(&file, &a.id).unwrap(), &doc);
        assert_eq!(
            d12,
            vec![Change {
                what: "パラメータ",
                name: "width".into(),
                before: String::new(),
                after: "60 mm".into()
            }]
        );
        assert_eq!(compare(&doc, &old)[0].after, "");
        assert!(compare(&doc, &doc).is_empty());
        let f = crate::projects::scan(&base, 4);
        assert!(f.folders.is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }
}
