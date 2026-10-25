//! レンダリングのギャラリー。描いた静止画を、描いた時の版に結び付けて
//! 版の履歴の横に残す。文書のファイルと保存形式は変えない(版の履歴と同じ付録)。
//!
//! 置き場所: 版の履歴のフォルダ([`crate::history::dir`])の `renders/`
//! - `index.json` … 絵の一覧(古い順。番号・版・日時・大きさ・説明)
//! - `<番号>.png` … 描いた絵そのもの
//! - `<番号>-small.png` … 一覧に出す縮小図(背景が透明な絵は灰色の上に置いた RGB)
//!
//! 絵は描き始めた時の文書の、いちばん新しい版に付く。保存の後に変えてから描いた絵は「保存後の
//! 変更あり」の印を持つ(どの版にもぴったりは当たらない)。

use crate::history::{self, THUMB_SIZE};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// ギャラリーの絵 1 枚。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Render {
    /// 1 から数える絵の番号(消しても使い回さない)
    pub number: u32,
    /// 描いた時のいちばん新しい版の指紋(版が無ければ無し)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// その版の番号
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_number: Option<u32>,
    /// 版を残した後に変えてから描いた
    #[serde(default)]
    pub changed: bool,
    /// 残した時刻(UNIX 時刻の秒)
    pub saved_at: u64,
    pub width: u32,
    pub height: u32,
    /// 説明(例: 「光線追跡 64 回・12.3 秒」)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Index {
    #[serde(default)]
    next: u32,
    renders: Vec<Render>,
}

/// 読める絵の辺の長さの上限(画素)。
const MAX_SIDE: u32 = 16384;

/// `path` の文書のギャラリーのフォルダ。
pub fn dir(path: &Path) -> PathBuf {
    history::dir(path).join("renders")
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

/// 絵の縮小図(PNG の RGB)。枠 [`THUMB_SIZE`] に収まる大きさに、画素を平均して縮める。
/// 透明な所は灰色の上に置く。
fn small(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let s = (THUMB_SIZE.0 as f32 / width as f32)
        .min(THUMB_SIZE.1 as f32 / height as f32)
        .min(1.0);
    let w = ((width as f32 * s).round() as u32).max(1);
    let h = ((height as f32 * s).round() as u32).max(1);
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        let (y0, y1) = (
            y * height / h,
            ((y + 1) * height / h).max(y * height / h + 1),
        );
        for x in 0..w {
            let (x0, x1) = (x * width / w, ((x + 1) * width / w).max(x * width / w + 1));
            let mut sum = [0.0f32; 3];
            let mut n = 0.0;
            for yy in y0..y1.min(height) {
                for xx in x0..x1.min(width) {
                    let i = ((yy * width + xx) * 4) as usize;
                    let a = f32::from(rgba[i + 3]) / 255.0;
                    for (c, s) in sum.iter_mut().enumerate() {
                        *s += f32::from(rgba[i + c]) * a + 128.0 * (1.0 - a);
                    }
                    n += 1.0;
                }
            }
            rgb.extend(sum.map(|s| (s / n).round().clamp(0.0, 255.0) as u8));
        }
    }
    jc_format::png::write_rgb(w, h, &rgb)
}

/// 描いた絵 `png`(JC が書いた PNG)を、文書 `path` のギャラリーに足す。`changed` は最後の版の
/// 後に文書を変えていたか。`note` は説明。残すのは新しい方から `keep` 枚(超えた古い絵は消す。
/// 気づかずに場所を取り続けないように)。
pub fn add(
    path: &Path,
    png: &[u8],
    changed: bool,
    note: &str,
    keep: usize,
) -> Result<Render, String> {
    let (width, height, rgba) =
        jc_format::png::read_rgba(png, MAX_SIDE).ok_or("JC が書いた形の PNG ではありません")?;
    let d = dir(path);
    std::fs::create_dir_all(&d).map_err(|e| format!("{} を作れません: {e}", d.display()))?;
    let mut ix = read_index(&d);
    let number = ix
        .next
        .max(ix.renders.iter().map(|r| r.number).max().unwrap_or(0))
        + 1;
    let write = |name: String, bytes: &[u8]| {
        let f = d.join(name);
        std::fs::write(&f, bytes).map_err(|e| format!("{} に書けません: {e}", f.display()))
    };
    write(format!("{number}.png"), png)?;
    write(format!("{number}-small.png"), &small(width, height, &rgba))?;
    let latest = history::list(path).into_iter().next();
    let r = Render {
        number,
        version_number: latest.as_ref().map(|v| v.number),
        version: latest.map(|v| v.id),
        changed,
        saved_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
        width,
        height,
        note: note.trim().to_string(),
    };
    ix.next = number;
    ix.renders.push(r.clone());
    let over = ix.renders.len().saturating_sub(keep.max(1));
    let old: Vec<u32> = ix.renders.drain(..over).map(|r| r.number).collect();
    write_index(&d, &ix)?;
    for n in &old {
        for f in [format!("{n}.png"), format!("{n}-small.png")] {
            let _ = std::fs::remove_file(d.join(f));
        }
    }
    if !old.is_empty() {
        log::info!("ギャラリーの古い絵 {old:?} を消した(残すのは {keep} 枚)");
    }
    log::info!(
        "レンダリングの絵 {number} をギャラリーに残した(版 {:?}): {}",
        r.version_number,
        path.display()
    );
    Ok(r)
}

/// ギャラリーの絵の一覧(新しい順)。無ければ空。
pub fn list(path: &Path) -> Vec<Render> {
    let mut v = read_index(&dir(path)).renders;
    v.reverse();
    v
}

/// 絵 `number` の PNG。
pub fn image(path: &Path, number: u32) -> Option<Vec<u8>> {
    std::fs::read(dir(path).join(format!("{number}.png"))).ok()
}

/// 絵 `number` の縮小図(PNG の RGB。[`history::decode_thumbnail`] で読める)。
pub fn thumbnail(path: &Path, number: u32) -> Option<Vec<u8>> {
    std::fs::read(dir(path).join(format!("{number}-small.png"))).ok()
}

/// 絵の PNG を画素(幅・高さ・RGBA)に戻す(画面に出す)。
pub fn decode(png: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    jc_format::png::read_rgba(png, MAX_SIDE)
}

/// 絵 `number` を消す(番号は使い回さない)。
pub fn remove(path: &Path, number: u32) -> Result<(), String> {
    let d = dir(path);
    let mut ix = read_index(&d);
    let n = ix.renders.len();
    ix.renders.retain(|r| r.number != number);
    if ix.renders.len() == n {
        return Err(format!("絵 {number} がありません"));
    }
    ix.next = ix.next.max(number);
    write_index(&d, &ix)?;
    for f in [format!("{number}.png"), format!("{number}-small.png")] {
        let _ = std::fs::remove_file(d.join(f));
    }
    log::info!("ギャラリーの絵 {number} を消した: {}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use jc_core::Document;

    #[test]
    fn renders_are_kept_with_the_version_they_were_drawn_from() {
        let base = std::env::temp_dir().join(format!("jc-gallery-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let file = base.join("天板.json");
        // 半分が透明な 600×300 の絵
        let mut rgba = Vec::new();
        for _ in 0..300 {
            for x in 0..600 {
                rgba.extend([200, 40, 40, if x < 300 { 255 } else { 0 }]);
            }
        }
        let png = jc_format::png::write_rgba(600, 300, &rgba);
        // 版が無い文書でも残せる(版は無し)
        let a = add(&file, &png, true, "", 50).unwrap();
        assert_eq!((a.number, a.version.as_deref()), (1, None));
        let v = history::record(&file, &Document::new(), &Default::default(), "")
            .unwrap()
            .unwrap();
        let b = add(&file, &png, false, " 光線追跡 64 回 ", 50).unwrap();
        assert_eq!(b.number, 2);
        assert_eq!(b.version.as_deref(), Some(v.id.as_str()));
        assert_eq!(b.version_number, Some(1));
        assert_eq!(
            (b.width, b.height, b.note.as_str()),
            (600, 300, "光線追跡 64 回")
        );
        // 一覧は新しい順。絵はそのまま、縮小図は枠に収まり、透明な所は灰色
        assert_eq!(
            list(&file).iter().map(|r| r.number).collect::<Vec<_>>(),
            vec![2, 1]
        );
        assert_eq!(image(&file, 2).unwrap(), png);
        let (w, h, rgb) = history::decode_thumbnail(&thumbnail(&file, 2).unwrap()).unwrap();
        assert_eq!((w, h), (256, 128));
        assert_eq!(&rgb[..3], &[200, 40, 40]);
        let last = ((w * h - 1) * 3) as usize;
        assert_eq!(&rgb[last..last + 3], &[128, 128, 128]);
        assert_eq!(decode(&png).unwrap().2, rgba);
        // 消しても番号は使い回さない
        remove(&file, 2).unwrap();
        assert!(remove(&file, 2).is_err());
        assert!(image(&file, 2).is_none());
        assert_eq!(add(&file, &png, false, "", 50).unwrap().number, 3);
        assert!(add(&file, b"png", false, "", 50).is_err());
        // 残す枚数を超えたら古い絵から消す(番号は使い回さない)
        assert_eq!(add(&file, &png, false, "", 2).unwrap().number, 4);
        assert_eq!(
            list(&file).iter().map(|r| r.number).collect::<Vec<_>>(),
            vec![4, 3]
        );
        assert!(image(&file, 1).is_none() && thumbnail(&file, 1).is_none());
        let _ = std::fs::remove_dir_all(&base);
    }
}
