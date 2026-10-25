//! 輪郭のフォント(TrueType・OpenType)の字の形。
//!
//! 計算はフォントのファイルを読まない(別の PC で開いても形が変わらず、試験が PC に依らない)。
//! フォントを選んだ時と字を足した時に、使う字の輪郭をフォントの単位のまま文字の機能のデータへ
//! 入れ([`OutlineFont`])、計算はそれだけを使う。フォントの OS/2 の fsType が「制限つき」
//! 「ビットマップだけ」の物は入れない(JC はフォントのファイルそのものは配らない)。
//!
//! 字の輪郭は skrifa でヒンティング無しに読む。TrueType(glyf)の 2 次のベジエは、誤差なしに
//! 3 次へ上げる。CFF の 3 次はそのまま。

use serde::{Deserialize, Serialize};
use skrifa::MetadataProvider;
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::raw::TableProvider;
use std::collections::BTreeMap;

type P2 = [f64; 2];

/// 文書に入れた輪郭のフォント: 名前、字の寸法(フォントの単位)、使う字の形。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutlineFont {
    /// 書体の名前(英語の名前。PC のフォントを探す時に使う)
    pub family: String,
    /// 太さ・斜体の名前(例 `Regular`)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub style: String,
    /// 画面に出す名前(日本語の名前があればそれ)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    /// 1 em のフォントの単位の数
    pub units_per_em: f64,
    /// 大文字の高さ(OS/2 の sCapHeight。無ければ「H」の高さ、それも無ければ 0.7 em)
    pub cap_height: f64,
    /// 基準線から上・下(下は負)と、行の間に足す長さ
    pub ascent: f64,
    pub descent: f64,
    #[serde(default)]
    pub leading: f64,
    /// 字(1 文字の文字列)ごとの送り幅と輪郭
    #[serde(default)]
    pub glyphs: BTreeMap<String, OutlineGlyph>,
}

/// 1 字の送り幅と輪郭(フォントの単位、y は上向き)。輪郭は `M x y`・`L x y`・`Q cx cy x y`・
/// `C c1x c1y c2x c2y x y`・`Z` を空白で区切って並べた文字列(SVG の path と同じ書き方)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutlineGlyph {
    pub advance: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path: String,
}

/// 輪郭の 1 区間(フォントの単位か mm)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Piece {
    Line(P2, P2),
    /// 3 次のベジエ(始点・制御点 2 つ・終点)
    Cubic([P2; 4]),
}

impl Piece {
    pub fn map(&self, f: impl Fn(P2) -> P2) -> Piece {
        match *self {
            Piece::Line(a, b) => Piece::Line(f(a), f(b)),
            Piece::Cubic(p) => Piece::Cubic(p.map(f)),
        }
    }

    pub fn points(&self) -> Vec<P2> {
        match self {
            Piece::Line(a, b) => vec![*a, *b],
            Piece::Cubic(p) => p.to_vec(),
        }
    }
}

/// 字の閉じた輪(区間の並び)。
pub type Contour = Vec<Piece>;

impl OutlineGlyph {
    /// 輪郭を区間の輪にする。開いた輪は閉じ、長さの無い区間は捨てる。
    pub fn contours(&self) -> Result<Vec<Contour>, String> {
        parse_path(&self.path)
    }
}

impl OutlineFont {
    /// 文字 `ch` の字(文書に入っていなければ `None`)。
    pub fn glyph(&self, ch: char) -> Option<&OutlineGlyph> {
        self.glyphs.get(ch.encode_utf8(&mut [0; 4]) as &str)
    }

    /// 文字列のうち、形が文書に入っていない字(改行は除く。同じ字は 1 回)。
    pub fn missing(&self, text: &str) -> Vec<char> {
        let mut out: Vec<char> = text
            .chars()
            .filter(|c| *c != '\n' && *c != '\r' && self.glyph(*c).is_none())
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// フォントのファイル(TTC なら `index` 番目の書体)を読み、字の寸法を取る(字はまだ入れない)。
    pub fn open(bytes: &[u8], index: u32) -> Result<OutlineFont, String> {
        let font = font_ref(bytes, index)?;
        check_embedding(&font)?;
        let m = font.metrics(Size::unscaled(), LocationRef::default());
        let upem = f64::from(m.units_per_em.max(1));
        let names = |id| {
            font.localized_strings(id)
                .english_or_first()
                .map(|s| s.to_string())
                .unwrap_or_default()
        };
        use skrifa::string::StringId;
        let family = Some(names(StringId::TYPOGRAPHIC_FAMILY_NAME))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| names(StringId::FAMILY_NAME));
        let style = Some(names(StringId::TYPOGRAPHIC_SUBFAMILY_NAME))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| names(StringId::SUBFAMILY_NAME));
        let mut f = OutlineFont {
            family,
            style,
            label: String::new(),
            units_per_em: upem,
            cap_height: m
                .cap_height
                .map(f64::from)
                .filter(|h| *h > 0.0)
                .unwrap_or(0.0),
            ascent: f64::from(m.ascent),
            descent: f64::from(m.descent),
            leading: f64::from(m.leading),
            glyphs: BTreeMap::new(),
        };
        if f.cap_height <= 0.0 {
            // 「H」の上の高さで測る
            let h = draw(&font, 'H')
                .and_then(|(_, path)| parse_path(&path).ok())
                .and_then(|c| {
                    c.iter()
                        .flatten()
                        .flat_map(|p| p.points().into_iter().map(|q| q[1]))
                        .reduce(f64::max)
                });
            f.cap_height = h.filter(|h| *h > 0.0).unwrap_or(0.7 * upem);
        }
        Ok(f)
    }

    /// 文字列の字のうち入っていない物を、同じフォントのファイルから足す。フォントに無い字を返す。
    pub fn add_glyphs(
        &mut self,
        bytes: &[u8],
        index: u32,
        text: &str,
    ) -> Result<Vec<char>, String> {
        let font = font_ref(bytes, index)?;
        check_embedding(&font)?;
        let mut absent = Vec::new();
        for ch in self.missing(text) {
            match draw(&font, ch) {
                Some((advance, path)) => {
                    self.glyphs
                        .insert(ch.to_string(), OutlineGlyph { advance, path });
                }
                None => absent.push(ch),
            }
        }
        Ok(absent)
    }
}

fn font_ref(bytes: &[u8], index: u32) -> Result<skrifa::FontRef<'_>, String> {
    skrifa::FontRef::from_index(bytes, index)
        .map_err(|e| format!("フォントのファイルを読めません({e})"))
}

/// fsType が「制限つき」「ビットマップだけ」のフォントは、字の形を文書に入れない。
fn check_embedding(font: &skrifa::FontRef<'_>) -> Result<(), String> {
    let fs_type = font.os2().map(|t| t.fs_type()).unwrap_or(0);
    if crate::fonts::embeddable(fs_type) {
        Ok(())
    } else {
        Err("このフォントは、作った人が字の形を文書に入れることを許していません(別のフォントを選んでください)".into())
    }
}

/// 字の送り幅と輪郭の文字列。フォントに無い字(cmap に無い)は `None`。
fn draw(font: &skrifa::FontRef<'_>, ch: char) -> Option<(f64, String)> {
    let gid = font.charmap().map(ch)?;
    let advance = font
        .glyph_metrics(Size::unscaled(), LocationRef::default())
        .advance_width(gid)
        .map(f64::from)
        .unwrap_or(0.0);
    let mut pen = PathPen(String::new());
    if let Some(g) = font.outline_glyphs().get(gid) {
        let settings = DrawSettings::unhinted(Size::unscaled(), LocationRef::default());
        g.draw(settings, &mut pen).ok()?;
    }
    Some((advance, pen.0.trim().to_string()))
}

/// 数を短く書く(小数 2 桁まで、後ろの 0 を落とす)。
fn num(v: f32) -> String {
    let r = (f64::from(v) * 100.0).round() / 100.0;
    let s = format!("{r:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

struct PathPen(String);

impl PathPen {
    fn put(&mut self, cmd: &str, xs: &[f32]) {
        self.0.push_str(cmd);
        for x in xs {
            self.0.push(' ');
            self.0.push_str(&num(*x));
        }
        self.0.push(' ');
    }
}

impl OutlinePen for PathPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.put("M", &[x, y]);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.put("L", &[x, y]);
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.put("Q", &[cx0, cy0, x, y]);
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.put("C", &[cx0, cy0, cx1, cy1, x, y]);
    }
    fn close(&mut self) {
        self.put("Z", &[]);
    }
}

fn near(a: P2, b: P2) -> bool {
    (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9
}

/// 輪郭の文字列を区間の輪にする。2 次のベジエは 3 次に上げる(誤差なし)。
pub fn parse_path(src: &str) -> Result<Vec<Contour>, String> {
    let mut toks = src.split_ascii_whitespace().peekable();
    let mut out: Vec<Contour> = Vec::new();
    let mut cur: Contour = Vec::new();
    let (mut start, mut at) = ([0.0; 2], [0.0; 2]);
    let bad = || {
        format!(
            "字の輪郭の書き方が違います: {}",
            src.chars().take(40).collect::<String>()
        )
    };
    let read = |toks: &mut std::iter::Peekable<std::str::SplitAsciiWhitespace<'_>>, n: usize| {
        (0..n)
            .map(|_| {
                toks.next()
                    .and_then(|t| t.parse::<f64>().ok())
                    .filter(|v| v.is_finite())
            })
            .collect::<Option<Vec<f64>>>()
    };
    let close = |cur: &mut Contour, at: P2, start: P2, out: &mut Vec<Contour>| {
        if !near(at, start) {
            cur.push(Piece::Line(at, start));
        }
        if !cur.is_empty() {
            out.push(std::mem::take(cur));
        }
    };
    while let Some(cmd) = toks.next() {
        match cmd {
            "M" => {
                close(&mut cur, at, start, &mut out);
                let v = read(&mut toks, 2).ok_or_else(bad)?;
                start = [v[0], v[1]];
                at = start;
            }
            "L" => {
                let v = read(&mut toks, 2).ok_or_else(bad)?;
                let p = [v[0], v[1]];
                if !near(p, at) {
                    cur.push(Piece::Line(at, p));
                }
                at = p;
            }
            "Q" => {
                let v = read(&mut toks, 4).ok_or_else(bad)?;
                let (q, p) = ([v[0], v[1]], [v[2], v[3]]);
                let c1 = [
                    at[0] + 2.0 / 3.0 * (q[0] - at[0]),
                    at[1] + 2.0 / 3.0 * (q[1] - at[1]),
                ];
                let c2 = [
                    p[0] + 2.0 / 3.0 * (q[0] - p[0]),
                    p[1] + 2.0 / 3.0 * (q[1] - p[1]),
                ];
                cur.push(Piece::Cubic([at, c1, c2, p]));
                at = p;
            }
            "C" => {
                let v = read(&mut toks, 6).ok_or_else(bad)?;
                let p = [v[4], v[5]];
                cur.push(Piece::Cubic([at, [v[0], v[1]], [v[2], v[3]], p]));
                at = p;
            }
            "Z" => {
                close(&mut cur, at, start, &mut out);
                at = start;
            }
            _ => return Err(bad()),
        }
    }
    close(&mut cur, at, start, &mut out);
    // 長さの無いベジエ(全部の点が同じ)を捨てる
    for c in &mut out {
        c.retain(|p| match p {
            Piece::Line(a, b) => !near(*a, *b),
            Piece::Cubic(q) => !q.iter().all(|x| near(*x, q[0])),
        });
    }
    out.retain(|c| !c.is_empty());
    Ok(out)
}

impl OutlineFont {
    /// 画面に出す名前(無ければ英語の書体の名前と太さ)。
    pub fn label_or_family(&self) -> String {
        if self.label.is_empty() {
            format!("{} {}", self.family, self.style).trim().to_string()
        } else {
            self.label.clone()
        }
    }
}
