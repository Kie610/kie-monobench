//! PC に入っている輪郭のフォント(TrueType・OpenType)の一覧。
//!
//! OS のフォントのフォルダ(Windows は `%WINDIR%\Fonts` と自分だけ入れた
//! `%LOCALAPPDATA%\Microsoft\Windows\Fonts`)と、[`FontIndex::add_dir`] で足したフォルダ(JC の
//! データフォルダの `fonts`、試験のフォント)の `.ttf`・`.otf`・`.ttc`・`.otc` を読む。一覧を作る
//! 時はファイルの頭と `name` 表だけを読み(フォントは数百あり、1 つが 10 MB を超える物もある)、
//! 字の形はフォントを選んだ時に [`crate::outline`] が読む。一覧は持ち主([`FontIndex`])が 1 回
//! 作って覚える(フォルダを足すと作り直す)。

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// フォントの 1 書体(TTC の中の 1 つも 1 行)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Face {
    pub path: PathBuf,
    /// TTC の中の番号(TTC でなければ 0)
    pub index: u32,
    /// 書体の名前(英語の名前。文書にはこれを書く。PC の言葉が替わっても同じ)
    pub family: String,
    /// 太さ・斜体の名前(英語。例 `Regular`・`Bold`)
    pub style: String,
    /// 画面に出す名前(日本語の名前があればそれ。例「游ゴシック Medium」)
    pub label: String,
    /// 文書へ字の形を入れてよいか(OS/2 の fsType が「制限つき」「ビットマップだけ」でない)
    pub embeddable: bool,
}

/// フォントの一覧と、読むフォルダ。
#[derive(Debug, Default)]
pub struct FontIndex {
    extra: Vec<PathBuf>,
    /// 作った一覧と、作った時のフォルダ
    built: Option<(Vec<PathBuf>, Vec<Face>)>,
}

impl FontIndex {
    /// 読むフォルダを足す。
    pub fn add_dir(&mut self, dir: &Path) {
        if !self.extra.iter().any(|d| d == dir) {
            self.extra.push(dir.to_path_buf());
        }
    }

    /// 足したフォルダ。
    pub fn added_dirs(&self) -> &[PathBuf] {
        &self.extra
    }

    /// OS のフォントのフォルダ(在る物だけ)と、足したフォルダ。
    pub fn dirs(&self) -> Vec<PathBuf> {
        let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
        let mut out: Vec<PathBuf> = Vec::new();
        if cfg!(windows) {
            out.extend(env("WINDIR").map(|w| w.join("Fonts")));
            out.extend(
                env("LOCALAPPDATA").map(|l| l.join("Microsoft").join("Windows").join("Fonts")),
            );
        } else if cfg!(target_os = "macos") {
            out.push("/System/Library/Fonts".into());
            out.push("/Library/Fonts".into());
            out.extend(env("HOME").map(|h| h.join("Library").join("Fonts")));
        } else {
            out.push("/usr/share/fonts".into());
            out.push("/usr/local/share/fonts".into());
            out.extend(env("HOME").map(|h| h.join(".local").join("share").join("fonts")));
            out.extend(env("HOME").map(|h| h.join(".fonts")));
        }
        out.retain(|d| d.is_dir());
        out.extend(self.extra.iter().cloned());
        out
    }

    /// フォントの一覧(画面の名前の順)。初めて呼んだ時とフォルダを足した後に作る。
    pub fn faces(&mut self) -> &[Face] {
        let dirs = self.dirs();
        if self.built.as_ref().is_none_or(|(d, _)| *d != dirs) {
            let mut faces = Vec::new();
            for d in &dirs {
                scan(d, 0, &mut faces);
            }
            faces.sort_by(|a, b| a.label.cmp(&b.label).then(a.path.cmp(&b.path)));
            faces.dedup_by(|a, b| a.family == b.family && a.style == b.style);
            log::info!(
                "フォントの一覧を作った: {} 書体({} か所のフォルダ)",
                faces.len(),
                dirs.len()
            );
            self.built = Some((dirs, faces));
        }
        self.built.as_ref().map_or(&[], |(_, f)| f.as_slice())
    }

    /// 書体の名前と太さの名前で探す(英語の名前でも画面の名前でもよい。太さが空なら Regular を先に)。
    pub fn find(&mut self, family: &str, style: &str) -> Option<Face> {
        let all = self.faces();
        let same = |f: &&Face| {
            f.family.eq_ignore_ascii_case(family)
                || (!family.is_empty() && f.label.starts_with(family))
        };
        let named = |f: &&Face| f.style.eq_ignore_ascii_case(style);
        let mut hits = all.iter().filter(same);
        if style.is_empty() {
            all.iter()
                .filter(same)
                .find(|f| f.style.eq_ignore_ascii_case("regular"))
                .or_else(|| hits.next())
                .cloned()
        } else {
            hits.find(named).cloned()
        }
    }
}

fn scan(dir: &Path, depth: usize, out: &mut Vec<Face>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if depth < 4 {
                scan(&p, depth + 1, out);
            }
            continue;
        }
        let ext = p
            .extension()
            .and_then(|x| x.to_str())
            .map(str::to_ascii_lowercase);
        if matches!(ext.as_deref(), Some("ttf" | "otf" | "ttc" | "otc")) {
            match faces_in(&p) {
                Ok(f) => out.extend(f),
                Err(e) => log::debug!("フォント {} を読めない: {e}", p.display()),
            }
        }
    }
}

fn be16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn read_at(f: &mut std::fs::File, at: u64, len: usize) -> std::io::Result<Vec<u8>> {
    f.seek(SeekFrom::Start(at))?;
    let mut b = vec![0; len];
    f.read_exact(&mut b)?;
    Ok(b)
}

/// ファイルの中の書体(頭の表と `name`・`OS/2` だけを読む)。
pub fn faces_in(path: &Path) -> Result<Vec<Face>, String> {
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let head = read_at(&mut f, 0, 12).map_err(|e| e.to_string())?;
    let starts: Vec<u32> = if &head[..4] == b"ttcf" {
        let n = be32(&head, 8).unwrap_or(0).min(256) as usize;
        let t = read_at(&mut f, 12, 4 * n).map_err(|e| e.to_string())?;
        (0..n).filter_map(|i| be32(&t, 4 * i)).collect()
    } else {
        vec![0]
    };
    let mut out = Vec::new();
    for (i, start) in starts.into_iter().enumerate() {
        if let Some(face) = face_at(&mut f, start as u64)? {
            out.push(Face {
                path: path.to_path_buf(),
                index: i as u32,
                ..face
            });
        }
    }
    Ok(out)
}

fn face_at(f: &mut std::fs::File, start: u64) -> Result<Option<Face>, String> {
    let io = |e: std::io::Error| e.to_string();
    let head = read_at(f, start, 12).map_err(io)?;
    let n = be16(&head, 4).unwrap_or(0) as usize;
    let dir = read_at(f, start + 12, 16 * n).map_err(io)?;
    let table = |tag: &[u8; 4]| {
        (0..n).find_map(|i| {
            let r = &dir[16 * i..16 * i + 16];
            (&r[..4] == tag).then(|| (be32(r, 8).unwrap_or(0), be32(r, 12).unwrap_or(0)))
        })
    };
    // 字の形の無いフォント(ビットマップだけ)は除く
    if table(b"glyf").is_none() && table(b"CFF ").is_none() && table(b"CFF2").is_none() {
        return Ok(None);
    }
    let Some((at, len)) = table(b"name") else {
        return Ok(None);
    };
    let name = read_at(f, at as u64, (len as usize).min(1 << 20)).map_err(io)?;
    let fs_type = match table(b"OS/2") {
        Some((at, len)) if len >= 10 => read_at(f, at as u64 + 8, 2)
            .ok()
            .and_then(|b| be16(&b, 0))
            .unwrap_or(0),
        _ => 0,
    };
    let names = Names::read(&name);
    let Some(family) = names.english(16).or_else(|| names.english(1)) else {
        return Ok(None);
    };
    let style = names
        .english(17)
        .or_else(|| names.english(2))
        .unwrap_or_else(|| "Regular".into());
    let ja_family = names.japanese(16).or_else(|| names.japanese(1));
    let ja_style = names.japanese(17).or_else(|| names.japanese(2));
    let label = match ja_family {
        Some(fam) => format!("{fam} {}", ja_style.unwrap_or_else(|| style.clone())),
        None => format!("{family} {style}"),
    };
    Ok(Some(Face {
        path: PathBuf::new(),
        index: 0,
        family,
        style,
        label,
        embeddable: embeddable(fs_type),
    }))
}

/// OS/2 の fsType で、字の形を文書へ入れてよいか(0x0002 制限つき、0x0200 ビットマップだけは入れない)。
pub fn embeddable(fs_type: u16) -> bool {
    fs_type & 0x0002 == 0 && fs_type & 0x0200 == 0
}

/// `name` 表の文字列(Windows の UTF-16 と、Mac の英語の物)。
struct Names(Vec<(u16, u16, u16, String)>);

impl Names {
    fn read(b: &[u8]) -> Names {
        let (Some(count), Some(base)) = (be16(b, 2), be16(b, 4)) else {
            return Names(Vec::new());
        };
        let mut out = Vec::new();
        for i in 0..count as usize {
            let r = 6 + 12 * i;
            let (Some(platform), Some(lang), Some(id), Some(len), Some(off)) = (
                be16(b, r),
                be16(b, r + 4),
                be16(b, r + 6),
                be16(b, r + 8),
                be16(b, r + 10),
            ) else {
                break;
            };
            let at = base as usize + off as usize;
            let Some(raw) = b.get(at..at + len as usize) else {
                continue;
            };
            let s = match platform {
                // Unicode と Windows は UTF-16 の big endian
                0 | 3 => String::from_utf16_lossy(
                    &raw.as_chunks::<2>()
                        .0
                        .iter()
                        .map(|c| u16::from_be_bytes(*c))
                        .collect::<Vec<_>>(),
                ),
                // Mac の英語(Roman)
                1 if lang == 0 => raw.iter().map(|&c| c as char).collect(),
                _ => continue,
            };
            out.push((platform, lang, id, s.trim().to_string()));
        }
        Names(out)
    }

    fn english(&self, id: u16) -> Option<String> {
        self.0
            .iter()
            .filter(|(_, _, i, s)| *i == id && !s.is_empty())
            .find(|(p, l, ..)| *p == 3 && *l == 0x0409)
            .or_else(|| {
                self.0.iter().find(|(p, l, i, s)| {
                    *i == id && !s.is_empty() && (*p == 1 || *p == 0 || *l & 0xff == 0x09)
                })
            })
            .map(|(.., s)| s.clone())
    }

    fn japanese(&self, id: u16) -> Option<String> {
        self.0
            .iter()
            .find(|(p, l, i, s)| *p == 3 && *l == 0x0411 && *i == id && !s.is_empty())
            .map(|(.., s)| s.clone())
    }
}
