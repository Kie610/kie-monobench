//! 日本語を表示するため、OS に入っているフォントを読み込む。
//! フォントのファイルは同梱しない(ライセンスと容量のため)。
//!
//! フォントごとに行の高さ(縦の寸法)が大きく違う(IPA は 1.0 em、游ゴシックやメイリオは 1.5 em 前後)。
//! そのままだと、同じ画面でも PC によって行の高さと文字の上下の位置が変わり、2 行の名前が下の名前に
//! 重なる。読み込む前に縦の寸法を揃え(`LINE`)、文字の四角(em)を行の真ん中に置く。

use egui::{FontData, FontFamily};
use std::sync::atomic::{AtomicU64, Ordering};

/// 探す順。見つかった最初の 1 つを使う(TTC は最初の書体)。
/// 画面の試作は IBM Plex Sans JP。入っていればそれを使う。
/// Windows の YuGothM.ttc の最初の書体は Yu Gothic Medium で、仮名の幅が IBM Plex Sans JP に近い
/// (同じ TTC の 2 つ目の Yu Gothic UI は仮名が詰まっていて、試作より狭くなる)。
const CANDIDATES: &[&str] = &[
    "%LOCALAPPDATA%\\Microsoft\\Windows\\Fonts\\IBMPlexSansJP-Regular.ttf",
    "C:\\Windows\\Fonts\\IBMPlexSansJP-Regular.ttf",
    "C:\\Windows\\Fonts\\YuGothM.ttc",
    "C:\\Windows\\Fonts\\meiryo.ttc",
    "C:\\Windows\\Fonts\\msgothic.ttc",
    "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/opentype/ipafont-gothic/ipagp.ttf",
];

/// 控えの字体。日本語の字体に無い字(⌀ U+2300 などの技術の記号、中国語の漢字、ハングル)を描く。
/// 見つかった物を全部、この順で既定の字体(英数字と絵文字)の後ろに重ねる。egui は字ごとに前から
/// 順に字形を持つ字体を探し、どれにも無ければ □ を描く。
/// 游ゴシック・IPA・egui の既定の字体には ⌀ が無い(IPA と egui の既定は試験で確かめている)。
const FALLBACKS: &[&str] = &[
    // Segoe UI Symbol(技術の記号)、Microsoft YaHei(中国語)、Malgun Gothic(ハングル)
    "C:\\Windows\\Fonts\\seguisym.ttf",
    "C:\\Windows\\Fonts\\msyh.ttc",
    "C:\\Windows\\Fonts\\malgun.ttf",
    "/System/Library/Fonts/Apple Symbols.ttf",
    // DejaVu Sans(Debian・Ubuntu、Fedora、Arch の道)
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
];

/// 決まった道([`CANDIDATES`])に日本語の字体が無い時に探す、系と利用者の字体のフォルダ
/// (字体を入れた場所が PC ごとに違っても、日本語の画面を □ にしないため)。
const FONT_DIRS: &[&str] = &[
    "%WINDIR%\\Fonts",
    "%LOCALAPPDATA%\\Microsoft\\Windows\\Fonts",
    "/System/Library/Fonts",
    "/Library/Fonts",
    "%HOME%/Library/Fonts",
    "/usr/share/fonts",
    "/usr/local/share/fonts",
    "%HOME%/.local/share/fonts",
    "%HOME%/.fonts",
];

/// [`FONT_DIRS`] を下へたどる深さ(Linux の /usr/share/fonts/opentype/<名前>/ の 3 段まで)。
const FONT_DEPTH: usize = 3;

/// 起動の記録で、描ける字体を書いておく字(無ければ □ になる)。
const WATCHED: &[char] = &['⌀', 'Ø', '°', '±'];

/// 元の行の高さは Yu Gothic Medium 1.60 em(上 0.88・下 0.22・行間 0.5 で、字が行の上に寄る)、
/// Meiryo 1.50 em、IPA P ゴシック 1.00 em(Windows 11 と Ubuntu で確認)。
///
/// 行の高さ(文字の大きさに対する比)。試作のツールバーの名前(1.15)と本文(1.4)の間。
/// egui のボタンの高さ(22)を変えない大きさにする(12.5 の文字で 15.6 + 上下 3)。
pub const LINE: f32 = 1.25;

/// 日本語の字の四角(em)の、基準線から上と下の割合(日本語のフォントの普通の値)。
const EM_ASCENT: f32 = 0.88;
const EM_DESCENT: f32 = 0.12;

/// 日本語のフォントを追加する。使ったフォントの説明(パス、書体、元の縦の寸法)を返す。
///
/// 文章の書体では既定のフォントより前に置く。後ろ(足りない文字の代わり)に置くと、
/// 英数字と括弧だけが既定のフォントで描かれ、日本語と高さや大きさがずれて見えるため。
/// 等幅の書体では、桁がそろうよう既定のフォントの後ろに置く。
///
/// 環境変数 `JC_FONT`(前の名前 `JISAKU_CADCAM_FONT` も見る)にフォントのパスがあれば、それを最初に試す。
///
/// 起動用: 日本語の字体だけをすぐ入れ、控えの字体([`FALLBACKS`])は裏のスレッドで読んで後から
/// 足す(控えには中国語の字体のような大きなファイルがあり、最初の画面を待たせないため)。
/// 控えを足した時に、控えの名前と [`WATCHED`] の字を描く字体を記録に書く。
pub fn install_system_fonts(ctx: &egui::Context) -> Option<String> {
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let (defs, used, primary) = primary_definitions(None);
    ctx.set_fonts(defs.clone());
    let ctx = ctx.clone();
    let spawned = std::thread::Builder::new()
        .name("字体の控え".into())
        .spawn(move || {
            let started = std::time::Instant::now();
            let mut defs = defs;
            let mut primary = primary;
            // 決まった道に無ければ、字体のフォルダから日本語の字を持つ字体を探す(裏で。起動は待たせない)
            if primary.is_none() {
                let dirs: Vec<String> = FONT_DIRS.iter().map(|p| expand(p)).collect();
                match find_japanese_font(&dirs) {
                    Some(found) => {
                        let (d, used, p) = primary_definitions(Some(&found));
                        log::info!(
                            "日本語の字体を字体のフォルダで見つけた: {}",
                            used.unwrap_or_default()
                        );
                        defs = d;
                        primary = p;
                    }
                    None => log::warn!(
                        "日本語の字体が見つかりません(字体のフォルダ {} を探した)",
                        dirs.join("、")
                    ),
                }
            }
            let added = add_fallbacks(&mut defs, primary.as_deref());
            // 読む間に設定で字体を替えていたら、古い並びで上書きしない
            if GENERATION.load(Ordering::SeqCst) == generation {
                ctx.set_fonts(defs);
                ctx.request_repaint();
                log::info!("字体の控え: {added}({} ms)", started.elapsed().as_millis());
            }
        });
    if let Err(e) = spawned {
        log::warn!("字体の控えを読むスレッドを作れません: {e}");
    }
    used
}

/// [`install_system_fonts`] と同じく字を入れる。`chosen`(設定の「字の書体」で選んだファイル)が
/// あれば、それを最初に試す(読めなければ自動で選ぶ)。前に入れた字は外す(動いている間に替えられる)。
/// 控えの字体もこの場で読んで重ねる(設定を替えた時と、画面の写真を撮る試験で使う)。
pub fn install_fonts(ctx: &egui::Context, chosen: Option<&str>) -> Option<String> {
    GENERATION.fetch_add(1, Ordering::SeqCst);
    let (defs, used) = font_definitions(chosen);
    ctx.set_fonts(defs);
    used
}

/// 字体を入れた回数。裏で控えを読み終えた時に、その間に別の字体を入れていないかを見る。
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// [`install_fonts`] が入れる字体の並び(日本語の字体と控え)と、その説明。
fn font_definitions(chosen: Option<&str>) -> (egui::FontDefinitions, Option<String>) {
    let (mut defs, used, primary) = primary_definitions(chosen);
    let added = add_fallbacks(&mut defs, primary.as_deref());
    (defs, used.map(|u| format!("{u}、控え {added}")))
}

/// 既定の字体の前(等幅では後ろ)に日本語の字体を 1 つ入れた並び。使った字体の説明とファイルの道も
/// 返す(見つからなければ両方 `None`)。
fn primary_definitions(
    chosen: Option<&str>,
) -> (egui::FontDefinitions, Option<String>, Option<String>) {
    let custom = std::env::var("JC_FONT")
        .or_else(|_| std::env::var("JISAKU_CADCAM_FONT"))
        .ok();
    let paths = chosen
        .map(str::to_string)
        .into_iter()
        .chain(custom)
        .chain(CANDIDATES.iter().map(|p| expand(p)));
    let mut defs = egui::FontDefinitions::default();
    for path in paths {
        let Some((data, name, before)) = load(&path) else {
            continue;
        };
        // egui の add_font の FontPriority::Highest(文章の先頭)と Lowest(等幅の後ろ)と同じ並べ方
        let key = "system-cjk".to_string();
        defs.font_data
            .insert(key.clone(), std::sync::Arc::new(data));
        defs.families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, key.clone());
        defs.families
            .entry(FontFamily::Monospace)
            .or_default()
            .push(key);
        let before = before.map_or("不明".to_string(), |m| m.to_string());
        let used = format!("{path} [0: {name}](元の縦の寸法 {before})");
        return (defs, Some(used), Some(path));
    }
    (defs, None, None)
}

/// 見つかった控えの字体を、文章と等幅の両方の最後に重ねる(日本語の字体と同じファイルは除く)。
/// 縦の寸法も日本語の字体と同じに揃え、基準線を合わせる(egui は字体ごとの上の高さで字を置くため、
/// 揃えないと控えの字だけ上下にずれる)。足した字体の名前と、[`WATCHED`] の字を描く字体を返す。
fn add_fallbacks(defs: &mut egui::FontDefinitions, primary: Option<&str>) -> String {
    let mut names = vec![];
    for path in FALLBACKS.iter().map(|p| expand(p)) {
        if primary == Some(path.as_str()) {
            continue;
        }
        let Some((data, name, _)) = load(&path) else {
            continue;
        };
        let key = format!("fallback-{}", names.len());
        defs.font_data
            .insert(key.clone(), std::sync::Arc::new(data));
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            defs.families.entry(family).or_default().push(key.clone());
        }
        names.push(name);
    }
    let names = if names.is_empty() {
        "無し".to_string()
    } else {
        names.join("、")
    };
    format!("{names}、{}", coverage(defs))
}

/// フォントのファイルを読み、縦の寸法を揃えた最初の書体(TTC は最初の書体)と、その名前と元の
/// 縦の寸法を返す。読めないかフォントでなければ `None`。
fn load(path: &str) -> Option<(FontData, String, Option<VMetrics>)> {
    let mut bytes = std::fs::read(path).ok()?;
    let name = face_name(&bytes)?;
    let before = normalize_metrics(&mut bytes, 0);
    let mut data = FontData::from_owned(bytes);
    data.index = 0;
    Some((data, name, before))
}

/// [`WATCHED`] の字を、文章の書体のどの字体が描くか(「⌀: DejaVu Sans」の形。どれにも無ければ
/// 「⌀: 無し(□ になる)」)。
fn coverage(defs: &egui::FontDefinitions) -> String {
    let order = defs
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    WATCHED
        .iter()
        .map(|&c| {
            let who = order.iter().find_map(|key| {
                let data = defs.font_data.get(key)?;
                let face = *faces(&data.font).get(data.index as usize)?;
                has_char(&data.font, face, c)
                    .then(|| full_name(&data.font, face).unwrap_or_else(|| key.clone()))
            });
            format!(
                "{c}: {}",
                who.unwrap_or_else(|| "無し(□ になる)".to_string())
            )
        })
        .collect::<Vec<_>>()
        .join("・")
}

/// 設定の「字の書体」で選べるフォントのファイル(道と、画面に出す名前)。OS の候補のうち入って
/// いる物と、データフォルダの `fonts/` に置いた `.ttf`・`.otf`・`.ttc`。
pub fn font_choices(data_dir: Option<&std::path::Path>) -> Vec<(String, String)> {
    let mut paths: Vec<String> = CANDIDATES.iter().map(|p| expand(p)).collect();
    if let Some(dir) = data_dir.map(|d| d.join("fonts"))
        && let Ok(entries) = std::fs::read_dir(dir)
    {
        let mut extra: Vec<String> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.extension().is_some_and(|x| {
                    ["ttf", "otf", "ttc"].contains(&x.to_string_lossy().to_lowercase().as_str())
                })
            })
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        extra.sort();
        paths.extend(extra);
    }
    paths
        .into_iter()
        .filter_map(|path| {
            // 名前の表は先頭にあるので、全部は読まずに書体の名前を探す(大きな TTC でも重くしない)
            let head = read_head(&path, 1 << 20)?;
            let name = face_name(&head).unwrap_or_else(|| {
                std::path::Path::new(&path)
                    .file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_default()
            });
            Some((path, name))
        })
        .collect()
}

/// フォルダ `dirs` を [`FONT_DEPTH`] 段まで下へたどり、最初の書体が日本語の字(仮名と漢字)を持つ
/// 字体のファイルを返す(名前の順で最初の物。無ければ `None`)。
fn find_japanese_font(dirs: &[String]) -> Option<String> {
    let mut files = vec![];
    let mut stack: Vec<(std::path::PathBuf, usize)> = dirs.iter().map(|d| (d.into(), 0)).collect();
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for p in entries.filter_map(|e| e.ok().map(|e| e.path())) {
            if p.is_dir() {
                if depth < FONT_DEPTH {
                    stack.push((p, depth + 1));
                }
            } else if p.extension().is_some_and(|x| {
                ["ttf", "otf", "ttc"].contains(&x.to_string_lossy().to_lowercase().as_str())
            }) {
                files.push(p);
            }
        }
    }
    files.sort();
    files.into_iter().find_map(|p| {
        let b = std::fs::read(&p).ok()?;
        let face = *faces(&b).first()?;
        (has_char(&b, face, 'あ') && has_char(&b, face, '漢'))
            .then(|| p.to_string_lossy().into_owned())
    })
}

/// ファイルの先頭 `n` バイト(無ければ `None`)。
fn read_head(path: &str, n: u64) -> Option<Vec<u8>> {
    use std::io::Read;
    let f = std::fs::File::open(path).ok()?;
    let mut out = vec![];
    f.take(n).read_to_end(&mut out).ok()?;
    Some(out)
}

/// `%NAME%` を環境変数の値に置き換える(なければそのまま)。
fn expand(path: &str) -> String {
    let mut parts = path.split('%');
    let mut out = parts.next().unwrap_or_default().to_string();
    let rest: Vec<&str> = parts.collect();
    for (i, p) in rest.iter().enumerate() {
        if i % 2 == 0 {
            match std::env::var(p) {
                Ok(v) => out.push_str(&v),
                Err(_) => return path.to_string(),
            }
        } else {
            out.push_str(p);
        }
    }
    out
}

fn be16(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(o..o + 2)?.try_into().ok()?))
}

fn be32(b: &[u8], o: usize) -> Option<usize> {
    Some(u32::from_be_bytes(b.get(o..o + 4)?.try_into().ok()?) as usize)
}

/// ファイルの中の書体の始まり(TTC なら複数)。
fn faces(b: &[u8]) -> Vec<usize> {
    if b.get(0..4) == Some(b"ttcf") {
        let n = be32(b, 8).unwrap_or(0).min(64);
        (0..n).filter_map(|i| be32(b, 12 + 4 * i)).collect()
    } else {
        vec![0]
    }
}

/// 書体の中の表の位置と長さ。
fn table(b: &[u8], face: usize, tag: &[u8; 4]) -> Option<(usize, usize)> {
    let n = be16(b, face + 4)? as usize;
    (0..n).find_map(|k| {
        let r = face + 12 + 16 * k;
        (b.get(r..r + 4)? == tag).then(|| Some((be32(b, r + 8)?, be32(b, r + 12)?)))?
    })
}

/// 書体の英語の正式名(name 表の 4 番、Windows の英語)。
fn full_name(b: &[u8], face: usize) -> Option<String> {
    let (t, _) = table(b, face, b"name")?;
    let count = be16(b, t + 2)? as usize;
    let strings = t + be16(b, t + 4)? as usize;
    (0..count).find_map(|k| {
        let r = t + 6 + 12 * k;
        let (platform, lang, id) = (be16(b, r)?, be16(b, r + 4)?, be16(b, r + 6)?);
        if platform != 3 || lang != 0x409 || id != 4 {
            return None;
        }
        let (len, off) = (be16(b, r + 8)? as usize, be16(b, r + 10)? as usize);
        let units: Vec<u16> = b
            .get(strings + off..strings + off + len)?
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_be_bytes(*c))
            .collect();
        String::from_utf16(&units).ok()
    })
}

/// 書体 `face` の文字の表(cmap)に字 `c` があるか(形式 4 と 12 を読む。ほかの形式は無いとみなす)。
fn has_char(b: &[u8], face: usize, c: char) -> bool {
    let c = c as u32;
    let Some((t, _)) = table(b, face, b"cmap") else {
        return false;
    };
    let n = be16(b, t + 2).unwrap_or(0) as usize;
    (0..n).any(|k| {
        let r = t + 4 + 8 * k;
        let (Some(platform), Some(encoding), Some(off)) =
            (be16(b, r), be16(b, r + 2), be32(b, r + 4))
        else {
            return false;
        };
        // Unicode の表(Windows の 3/1・3/10 と Unicode の 0/*)だけを見る
        if !(platform == 0 || (platform == 3 && matches!(encoding, 1 | 10))) {
            return false;
        }
        let s = t + off;
        match be16(b, s) {
            Some(4) => cmap4(b, s, c).unwrap_or(false),
            Some(12) => cmap12(b, s, c).unwrap_or(false),
            _ => false,
        }
    })
}

fn cmap4(b: &[u8], s: usize, c: u32) -> Option<bool> {
    let Ok(c) = u16::try_from(c) else {
        return Some(false);
    };
    let segs = be16(b, s + 6)? as usize / 2;
    let ends = s + 14;
    let starts = ends + 2 * segs + 2;
    let deltas = starts + 2 * segs;
    let ranges = deltas + 2 * segs;
    for i in 0..segs {
        if c > be16(b, ends + 2 * i)? {
            continue;
        }
        let start = be16(b, starts + 2 * i)?;
        if c < start {
            return Some(false);
        }
        let delta = be16(b, deltas + 2 * i)?;
        let range = be16(b, ranges + 2 * i)? as usize;
        let glyph = if range == 0 {
            c.wrapping_add(delta)
        } else {
            let at = ranges + 2 * i + range + 2 * (c - start) as usize;
            match be16(b, at)? {
                0 => 0,
                g => g.wrapping_add(delta),
            }
        };
        return Some(glyph != 0);
    }
    Some(false)
}

fn cmap12(b: &[u8], s: usize, c: u32) -> Option<bool> {
    let groups = be32(b, s + 12)?;
    for i in 0..groups {
        let g = s + 16 + 12 * i;
        let (start, end) = (be32(b, g)? as u32, be32(b, g + 4)? as u32);
        if (start..=end).contains(&c) {
            return Some(be32(b, g + 8)? as u32 + (c - start) != 0);
        }
    }
    Some(false)
}

/// 最初の書体の名前。フォントとして読めないファイルは `None`。
fn face_name(b: &[u8]) -> Option<String> {
    let face = *faces(b).first()?;
    table(b, face, b"hhea")?;
    Some(full_name(b, face).unwrap_or_default())
}

/// 元の縦の寸法(em に対する比)。記録用。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VMetrics {
    pub ascent: f32,
    pub descent: f32,
    pub gap: f32,
}

impl std::fmt::Display for VMetrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "上 {:.3}・下 {:.3}・行間 {:.3} = {:.3} em",
            self.ascent,
            self.descent,
            self.gap,
            self.ascent + self.descent + self.gap
        )
    }
}

/// 書体 `index` の縦の寸法(hhea と OS/2)を、行の高さ `LINE` で字の四角が真ん中に来る値に書き換える。
/// egui(skrifa)は OS/2 の USE_TYPO_METRICS で hhea と OS/2 のどちらかを使うので、両方を同じにする。
/// 書き換える前の値(egui が使っていた方)を返す。読めなければ何もしない。
fn normalize_metrics(b: &mut [u8], index: u32) -> Option<VMetrics> {
    let face = *faces(b).get(index as usize)?;
    let (head, _) = table(b, face, b"head")?;
    let upm = f32::from(be16(b, head + 18)?);
    let (hhea, _) = table(b, face, b"hhea")?;
    let os2 = table(b, face, b"OS/2").filter(|(_, len)| *len >= 78);
    let i16_at = |b: &[u8], o: usize| be16(b, o).map(|v| f32::from(v as i16) / upm);
    let use_typo = os2.is_some_and(|(t, _)| be16(b, t + 62).is_some_and(|v| v & 0x80 != 0));
    let src = match os2 {
        Some((t, _)) if use_typo => t + 68,
        _ => hhea + 4,
    };
    let before = VMetrics {
        ascent: i16_at(b, src)?,
        descent: -i16_at(b, src + 2)?,
        gap: i16_at(b, src + 4)?,
    };
    let pad = (LINE - 1.0) / 2.0;
    let asc = ((EM_ASCENT + pad) * upm).round() as i16;
    let desc = ((EM_DESCENT + pad) * upm).round() as i16;
    let put = |b: &mut [u8], o: usize, v: i16| b[o..o + 2].copy_from_slice(&v.to_be_bytes());
    put(b, hhea + 4, asc);
    put(b, hhea + 6, -desc);
    put(b, hhea + 8, 0);
    if let Some((t, _)) = os2 {
        put(b, t + 68, asc);
        put(b, t + 70, -desc);
        put(b, t + 72, 0);
        put(b, t + 74, asc);
        put(b, t + 76, desc);
    }
    Some(before)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 表を 1 つずつ持つ最小のフォント(head・hhea・OS/2・name)を作る。
    fn tiny_font(upm: u16, asc: i16, desc: i16, gap: i16, name: &str) -> Vec<u8> {
        let mut head = vec![0u8; 54];
        head[18..20].copy_from_slice(&upm.to_be_bytes());
        let mut hhea = vec![0u8; 36];
        hhea[4..6].copy_from_slice(&asc.to_be_bytes());
        hhea[6..8].copy_from_slice(&desc.to_be_bytes());
        hhea[8..10].copy_from_slice(&gap.to_be_bytes());
        let os2 = vec![0u8; 96];
        let s: Vec<u8> = name.encode_utf16().flat_map(u16::to_be_bytes).collect();
        let mut nm = Vec::new();
        for v in [0u16, 1, 18, 3, 1, 0x409, 4, s.len() as u16, 0] {
            nm.extend(v.to_be_bytes());
        }
        nm.extend(&s);
        let tables: [(&[u8; 4], Vec<u8>); 4] = [
            (b"OS/2", os2),
            (b"head", head),
            (b"hhea", hhea),
            (b"name", nm),
        ];
        let mut out = vec![0u8; 12 + 16 * tables.len()];
        out[4..6].copy_from_slice(&(tables.len() as u16).to_be_bytes());
        for (k, (tag, data)) in tables.iter().enumerate() {
            let r = 12 + 16 * k;
            let off = out.len() as u32;
            out[r..r + 4].copy_from_slice(*tag);
            out[r + 8..r + 12].copy_from_slice(&off.to_be_bytes());
            out[r + 12..r + 16].copy_from_slice(&(data.len() as u32).to_be_bytes());
            out.extend(data);
        }
        out
    }

    #[test]
    fn row_height_is_the_same_whatever_the_font() {
        // 游ゴシックのように行の高さが大きいフォントでも、IPA のように 1.0 em のフォントでも、
        // 書き換えた後は LINE em で、字の四角が行の真ん中に来る
        for (asc, desc, gap) in [(1802, -246, 0), (2200, -1000, 0), (1900, -500, 300)] {
            let mut b = tiny_font(2048, asc, desc, gap, "Test");
            let before = normalize_metrics(&mut b, 0).unwrap();
            assert!((before.ascent - f32::from(asc) / 2048.0).abs() < 1e-6);
            let after = normalize_metrics(&mut b, 0).unwrap();
            assert!((after.ascent + after.descent + after.gap - LINE).abs() < 1e-3);
            let pad_top = after.ascent - EM_ASCENT;
            let pad_bottom = after.descent - EM_DESCENT;
            assert!((pad_top - pad_bottom).abs() < 1e-3);
        }
    }

    #[test]
    fn the_face_name_is_read_and_other_files_are_skipped() {
        let f = tiny_font(1000, 880, -120, 0, "Yu Gothic Medium");
        assert_eq!(face_name(&f), Some("Yu Gothic Medium".to_string()));
        assert_eq!(face_name(b"not a font"), None);
    }

    /// 文章の書体で `c` を描く字体の名前(どれにも無ければ `None`)。
    fn drawn_by(defs: &egui::FontDefinitions, c: char) -> Option<String> {
        defs.families[&FontFamily::Proportional]
            .iter()
            .find_map(|key| {
                let data = &defs.font_data[key];
                let face = *faces(&data.font).get(data.index as usize)?;
                has_char(&data.font, face, c).then(|| key.clone())
            })
    }

    /// 決まった道に無い日本語の字体も、字体のフォルダを下へたどって見つける(日本語の字の無い字体は
    /// 選ばない)。この PC の日本語の字体を、決まっていない場所へ写して確かめる。
    #[test]
    fn a_japanese_font_is_found_outside_the_fixed_paths() {
        let (_, _, primary) = primary_definitions(None);
        let Some(src) = primary else {
            kmb_contract::skip!("日本語の字体が入っていない");
        };
        let tmp = std::env::temp_dir().join(format!("kmb-fonts-{}", std::process::id()));
        let deep = tmp.join("a").join("b");
        std::fs::create_dir_all(&deep).unwrap();
        let ext = std::path::Path::new(&src).extension().unwrap().to_owned();
        let copy = deep.join("x").with_extension(ext);
        std::fs::copy(&src, &copy).unwrap();
        // 名前の順で先に来る、日本語の字の無い字体
        let defs = egui::FontDefinitions::default();
        std::fs::write(tmp.join("0.ttf"), &*defs.font_data["Ubuntu-Light"].font).unwrap();
        let dirs = [tmp.to_string_lossy().into_owned()];
        let found = find_japanese_font(&dirs);
        assert_eq!(found.as_deref(), Some(copy.to_string_lossy().as_ref()));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn the_character_table_is_read() {
        // egui の既定の字体(Ubuntu Light)は英字を持ち、日本語と ⌀ を持たない
        let defs = egui::FontDefinitions::default();
        let ubuntu = &defs.font_data["Ubuntu-Light"];
        assert!(has_char(&ubuntu.font, 0, 'A'));
        assert!(has_char(&ubuntu.font, 0, 'Ø'));
        assert!(!has_char(&ubuntu.font, 0, 'あ'));
        assert!(!has_char(&ubuntu.font, 0, '⌀'));
        assert!(!has_char(b"not a font", 0, 'A'));
    }

    /// ⌀ が □ になっていた根本の原因: 日本語の字体 1 つと egui の既定の字体のどれにも ⌀ が無かった。
    /// 控えの字体を重ねると描ける。この PC に入っている字体(起動と同じ探し方)で確かめるので、
    /// Windows では游ゴシックなどと Segoe UI Symbol、クラウドでは IPA と DejaVu Sans を見る。
    #[test]
    fn the_diameter_sign_is_drawn_by_a_fallback() {
        let (before, _, primary) = primary_definitions(None);
        if primary.is_none() {
            kmb_contract::skip!("日本語の字体が入っていない");
        }
        // 前: 日本語の字体と既定の字体だけでは ⌀ を描く字体が無い(日本語の字体が ⌀ を持つ PC
        // では、前の □ は起きない)
        let primary_has_it = drawn_by(&before, '⌀').is_some();
        if primary_has_it {
            assert_eq!(drawn_by(&before, '⌀').as_deref(), Some("system-cjk"));
        }
        assert_eq!(drawn_by(&before, 'あ').as_deref(), Some("system-cjk"));

        // 後: 控えを重ねると ⌀ を描く字体があり、日本語と Ø は日本語の字体のまま描く
        let (after, used) = font_definitions(None);
        let used = used.unwrap();
        let who =
            drawn_by(&after, '⌀').unwrap_or_else(|| panic!("⌀ を描く字体が無い(□ になる): {used}"));
        if !primary_has_it {
            assert!(who.starts_with("fallback-"), "{who}: {used}");
        }
        assert_eq!(drawn_by(&after, 'あ').as_deref(), Some("system-cjk"));
        assert_eq!(drawn_by(&after, 'Ø').as_deref(), Some("system-cjk"));
        assert!(!used.contains("⌀: 無し"), "{used}");
        // Windows の控えは OS に必ず入っている Segoe UI Symbol(⌀ を描く)
        if cfg!(windows) {
            assert!(used.contains("Segoe UI Symbol"), "{used}");
        }

        // 控えの縦の寸法も日本語の字体と同じに揃う(基準線が合う)
        if who.starts_with("fallback-") {
            let mut bytes = after.font_data[&who].font.to_vec();
            let m = normalize_metrics(&mut bytes, after.font_data[&who].index).unwrap();
            assert!((m.ascent + m.descent + m.gap - LINE).abs() < 1e-3);
            assert!((m.ascent - (EM_ASCENT + (LINE - 1.0) / 2.0)).abs() < 1e-3);
        }

        // egui に入れると ⌀ に字形がある(□ にならない)
        let ctx = egui::Context::default();
        ctx.set_fonts(after);
        ctx.run_ui(Default::default(), |_| {})
            .textures_delta
            .clear();
        let id = egui::FontId::proportional(12.5);
        assert!(ctx.fonts_mut(|f| f.has_glyph(&id, '⌀')));
    }

    #[test]
    fn environment_names_are_expanded() {
        assert_eq!(expand("C:\\a\\b.ttf"), "C:\\a\\b.ttf");
        assert_eq!(
            expand("%JC_SURELY_NOT_SET_42%\\x.ttf"),
            "%JC_SURELY_NOT_SET_42%\\x.ttf"
        );
    }
}
