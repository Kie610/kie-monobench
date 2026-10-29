//! 文字(`cad.text@1`)。平面の上に、1 本線のフォント(Hershey)か輪郭のフォント(TrueType・
//! OpenType)で文字を置き、線(`curves`)と閉じた領域(`profile`)を出す。
//! 機能のデータの既定は 1 本線。新しく置く操作では PC の日本語のゴシック体を選ぶ。
//! 彫刻の加工は 1 本線を深さでなぞり、線は二重にならない。
//! 輪郭のフォント(日本語も)は、領域を押し出し・ポケット・V 彫りで加工する。
//!
//! 1 本線のフォントのデータは `fonts/` の Hershey フォント(許諾は `fonts/HERSHEY-LICENSE.txt`)。
//! 英数字と記号(ASCII の 32〜126)だけで、それ以外の文字は「?」に置き換えて知らせる。
//! 輪郭のフォントは、使う字の形を機能のデータ(`outline`)に入れて持つ([`outline`])。
//!
//! 置き方: 基準の位置(`x`, `y`、またはスケッチの点 `anchor` からのずれ)に、9 か所の揃え
//! (横は左・中・右、縦は大文字の上・中・基準線)のどこを合わせるかを選び、`angle` で回す。
//! 大きさは高さの式だけで決める(拘束で文字が伸び縮みしない)。外接の枠は `frame` に出す
//! (実際の線から計算する参照用の線。彫刻はなぞらない)。

mod commands;
pub mod fonts;
mod glyph;
pub mod outline;
mod path;

pub use commands::{
    EXPLODE_TEXT, TEXT_AT_POINT, TEXT_FONT, TEXT_ON_FACE, TEXT_ON_PATH, TextCommands,
    refresh_glyphs, use_face,
};
use kmb_core::{
    Curves, Dim, EvalCtx, EvalError, Feature, FeatureId, InputRef, Outputs, ProfileEdge, Registry,
    Segment, expr_inputs,
};
use kmb_sketch::{EntityId, SketchPlane, profile_value, resolve_plane, solution_of};
pub use outline::{OutlineFont, OutlineGlyph};
pub use path::{Path, lay_along, lay_along_with};
use serde::{Deserialize, Serialize};

pub const TEXT: &str = "cad.text@1";

/// 1 本線のフォント。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Font {
    /// 太さの一定な、飾りのない字(Hershey の Futura Light)
    #[default]
    Sans,
    /// 小さな飾りのある字(Hershey の Roman Simplex)
    Roman,
}

impl Font {
    fn data(self) -> &'static str {
        match self {
            Font::Sans => include_str!("../fonts/futural.jhf"),
            Font::Roman => include_str!("../fonts/rowmans.jhf"),
        }
    }
}

/// 横の揃え(基準の位置に、行のどこを合わせるか)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// 縦の揃え。複数行なら、上は 1 行目の大文字の上、下は最後の行の基準線。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VAlign {
    /// 1 行目の基準線(縦の揃えの無い前の文書もこれ)
    #[default]
    Baseline,
    /// 最後の行の基準線
    Bottom,
    Middle,
    Top,
}

/// 幅に合わせる時に変えるもの。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FitMode {
    /// 字の幅だけを変える(高さはそのまま)
    #[default]
    Stretch,
    /// 高さも同じ割合で変える(字の形を保つ)
    Scale,
}

/// 文字の高さの基準。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeightBasis {
    /// 大文字の高さ(英字の名入れ。1 本線のフォントはいつもこれ)
    #[default]
    Cap,
    /// 字の枠(全角の字の大きさ、em。日本語の名入れ)
    Em,
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

/// 文字の機能のデータ。例: `{"plane": {"kind": "xy"}, "text": "JISAKU", "height": "5 mm"}`
/// スケッチの点に中央で置く例: `{"text": "A", "anchor": 3, "align": "center", "valign": "middle"}`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Text {
    /// 置く平面(`anchor` があれば、そのスケッチの平面を使う)
    #[serde(default)]
    pub plane: SketchPlane,
    pub text: String,
    /// 大文字の高さ
    #[serde(default = "height")]
    pub height: String,
    /// 基準の位置。平面の座標(`anchor` があれば、その点からのずれ)
    #[serde(default = "zero")]
    pub x: String,
    #[serde(default = "zero")]
    pub y: String,
    #[serde(default)]
    pub font: Font,
    #[serde(default, skip_serializing_if = "is_default")]
    pub align: HAlign,
    #[serde(default, skip_serializing_if = "is_default")]
    pub valign: VAlign,
    /// 基準の位置のまわりの回転(空なら 0)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<String>,
    /// 字と字の間に足す長さ(空なら 0)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spacing: Option<String>,
    /// 字の幅の倍率(空なら 1)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<String>,
    /// 基準の点のあるスケッチ。点はスケッチの拘束で動かせる
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<FeatureId>,
    /// 基準の点(省くと、そのスケッチの単独の点のうち最初のもの)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor_point: Option<EntityId>,
    /// 最も長い行をこの幅に合わせる(空なら合わせない)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fit_width: Option<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub fit: FitMode,
    /// 文字を沿わせる線のあるスケッチ。`x` は線の始点から揃えの点までの長さ、
    /// `y` は線から離す長さ(進む向きの左が正)になる
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<FeatureId>,
    /// 沿わせる線(線分・円弧・円。省くと、そのスケッチの最初の線。補助線でもよい)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_entity: Option<EntityId>,
    /// 線を逆の向きにたどる(円の上側に、外向きに正立した文字を置く時など)
    #[serde(default, skip_serializing_if = "is_default")]
    pub reverse: bool,
    /// 選んだ線に端のつながる線分・円弧もたどる(折れ線や、線と円弧をつないだ形に沿わせる)
    #[serde(default, skip_serializing_if = "is_default")]
    pub chain: bool,
    /// 押し出しで彫る・浮かせる時の線の太さ(空なら大文字の高さの 12%)。1 本線をこの太さに
    /// 太らせた閉じた領域を輪郭(`profile`)に出す
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<String>,
    /// 輪郭のフォント(使う字の形を入れて持つ)。あれば `font` の 1 本線の代わりに使う
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline: Option<OutlineFont>,
    /// 高さの基準(輪郭のフォントの時。1 本線はいつも大文字の高さ)
    #[serde(default, skip_serializing_if = "is_default")]
    pub basis: HeightBasis,
}

impl Default for Text {
    fn default() -> Self {
        Text {
            plane: SketchPlane::default(),
            text: "TEXT".into(),
            height: height(),
            x: zero(),
            y: zero(),
            font: Font::default(),
            align: HAlign::default(),
            valign: VAlign::default(),
            angle: None,
            spacing: None,
            width: None,
            anchor: None,
            anchor_point: None,
            fit_width: None,
            fit: FitMode::default(),
            path: None,
            path_entity: None,
            reverse: false,
            chain: false,
            stroke: None,
            outline: None,
            basis: HeightBasis::default(),
        }
    }
}

fn height() -> String {
    "5 mm".into()
}

fn zero() -> String {
    "0 mm".into()
}

/// 1 文字分: 左端と右端、ペンを下ろしたままの折れ線の列(フォントの単位。y は下向き)。
struct Glyph {
    left: i32,
    right: i32,
    strokes: Vec<Vec<(i32, i32)>>,
}

/// JHF の 1 行(番号 5 文字、点の数 3 文字、その後は 2 文字ずつ。`R` が 0、" R" はペンを上げる)。
fn parse_glyph(line: &str) -> Option<Glyph> {
    let b = line.as_bytes();
    if b.len() < 10 {
        return None;
    }
    let v = |c: u8| c as i32 - b'R' as i32;
    let body = &b[8..];
    let (left, right) = (v(body[0]), v(body[1]));
    let mut strokes = vec![Vec::new()];
    for pair in body[2..].as_chunks::<2>().0 {
        if pair == b" R" {
            strokes.push(Vec::new());
        } else if let Some(s) = strokes.last_mut() {
            s.push((v(pair[0]), v(pair[1])));
        }
    }
    strokes.retain(|s| s.len() >= 2);
    Some(Glyph {
        left,
        right,
        strokes,
    })
}

/// 大文字の高さ(フォントの単位)と、基準線の高さ(y は下向き)。
const CAP: f64 = 21.0;
const BASELINE: f64 = 9.0;

/// 文字の形と置き方(長さは mm、角度はラジアン)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    pub font: Font,
    /// 大文字の高さ
    pub height: f64,
    /// 字と字の間に足す長さ
    pub spacing: f64,
    /// 字の幅の倍率
    pub width: f64,
    pub align: HAlign,
    pub valign: VAlign,
    pub angle: f64,
    /// 高さの基準(輪郭のフォントの時)
    pub basis: HeightBasis,
}

impl Style {
    pub fn new(font: Font, height: f64) -> Self {
        Style {
            font,
            height,
            spacing: 0.0,
            width: 1.0,
            align: HAlign::Left,
            valign: VAlign::Baseline,
            angle: 0.0,
            basis: HeightBasis::Cap,
        }
    }
}

impl Font {
    fn glyphs(self) -> Vec<Option<Glyph>> {
        self.data().lines().map(parse_glyph).collect()
    }
}

/// 文字の番号(フォントの何行目か)。フォントにない文字は「?」にして `true` を返す。
fn code_of(ch: char) -> (usize, bool) {
    if (' '..='~').contains(&ch) {
        (ch as usize - ' ' as usize, false)
    } else {
        ('?' as usize - ' ' as usize, true)
    }
}

impl Style {
    /// 最も長い行の幅が `width` になるようにした書式。`uniform` なら高さも同じ割合で変え
    /// (字の形を保つ)、そうでなければ字の幅の倍率だけを変える。字の間隔は変えない。
    pub fn fit_width(&self, text: &str, width: f64, uniform: bool) -> Result<Style, String> {
        self.fit_width_with(text, width, uniform, None)
    }

    /// [`Style::fit_width`] の、輪郭のフォント `outline` も使える版。
    pub fn fit_width_with(
        &self,
        text: &str,
        width: f64,
        uniform: bool,
        outline: Option<&OutlineFont>,
    ) -> Result<Style, String> {
        if width <= 0.0 {
            return Err("合わせる幅は 0 より大きくしてください".into());
        }
        let glyphs = self.font.glyphs();
        // 大きさの基準(フォントの単位)
        let basis = match outline {
            Some(f) => basis_units(f, self.basis),
            None => CAP,
        };
        // 横の縮尺(mm / フォントの単位)の候補のうち最も小さいもの = 最も長い行が幅に合う
        let mut kx: Option<f64> = None;
        for src in text.split('\n') {
            let (mut units, mut n) = (0.0, 0usize);
            for ch in src.chars() {
                if let Some(f) = outline {
                    units += advance(f, ch);
                    n += 1;
                } else if let Some(Some(g)) = glyphs.get(code_of(ch).0) {
                    units += (g.right - g.left) as f64;
                    n += 1;
                }
            }
            if units <= 0.0 {
                continue;
            }
            let room = width - self.spacing * n.saturating_sub(1) as f64;
            if room <= 0.0 {
                return Err(format!(
                    "字の間隔だけで幅 {width:.3} mm を超えます(間隔を狭めてください)"
                ));
            }
            let k = room / units;
            kx = Some(kx.map_or(k, |b| b.min(k)));
        }
        let kx = kx.ok_or("文字がありません")?;
        let mut s = *self;
        if uniform {
            s.height = kx / self.width * basis;
        } else {
            s.width = kx / (self.height / basis);
        }
        Ok(s)
    }
}

/// 輪郭のフォントの高さの基準(フォントの単位)。
fn basis_units(f: &OutlineFont, basis: HeightBasis) -> f64 {
    match basis {
        HeightBasis::Cap => f.cap_height,
        HeightBasis::Em => f.units_per_em,
    }
}

/// 輪郭のフォントの字の送り幅(フォントの単位)。形の入っていない字は半角の幅にする。
fn advance(f: &OutlineFont, ch: char) -> f64 {
    f.glyph(ch).map_or(f.units_per_em / 2.0, |g| g.advance)
}

/// 並べた文字: 線、枠(参照用の線)、フォントにない文字の数、注意。輪郭のフォントなら、
/// 字の閉じた輪(たどる向きつきの区間)も持つ(`segments` は輪の区間を並べた物)。
#[derive(Debug, Clone, PartialEq)]
pub struct Laid {
    pub segments: Vec<Segment>,
    pub frame: Vec<Segment>,
    pub missing: usize,
    pub warnings: Vec<String>,
    pub rings: Vec<Vec<(Segment, bool)>>,
}

/// 線分 1 本(始点と終点)。
type Stroke = ([f64; 2], [f64; 2]);

/// 揃えた 1 文字: 行の基準線の上での字の中心の x、その行の基準線の y、字の線
/// (どれも揃えの点を原点にした、回す前の座標)。
struct Placed {
    cx: f64,
    base: f64,
    strokes: Vec<Stroke>,
    /// 輪郭のフォントの字の閉じた輪(たどる向きつき)
    rings: Vec<Vec<(Segment, bool)>>,
}

/// 揃えた文字の全部と、その外接の四角形。
struct Shaped {
    glyphs: Vec<Placed>,
    lo: [f64; 2],
    hi: [f64; 2],
    missing: usize,
}

/// 文字列を、揃えの点を原点にした座標に並べる(回す前)。`outline` があれば輪郭のフォントで。
fn shape(text: &str, style: &Style, outline: Option<&OutlineFont>) -> Shaped {
    if let Some(f) = outline {
        return shape_outline(text, style, f);
    }
    let glyphs = style.font.glyphs();
    let k = style.height / CAP;
    let kx = k * style.width;
    let line_gap = CAP * 1.6 * k;
    let mut missing = 0;
    // 行ごとに、基準線の左端を原点にした字と行の幅
    let mut lines: Vec<(Vec<Placed>, f64)> = Vec::new();
    for (row, src) in text.split('\n').enumerate() {
        let base = -(row as f64) * line_gap;
        let mut placed = Vec::new();
        let mut pen = 0.0;
        let mut right: f64 = 0.0;
        for ch in src.chars() {
            let (code, absent) = code_of(ch);
            if absent {
                missing += 1;
            }
            let Some(Some(g)) = glyphs.get(code) else {
                continue;
            };
            let p = |(x, y): (i32, i32)| {
                [
                    pen + (x - g.left) as f64 * kx,
                    (BASELINE - y as f64) * k + base,
                ]
            };
            let strokes: Vec<Stroke> = g
                .strokes
                .iter()
                .flat_map(|s| s.windows(2).map(|w| (p(w[0]), p(w[1]))))
                .collect();
            right = pen + (g.right - g.left) as f64 * kx;
            if !strokes.is_empty() {
                placed.push(Placed {
                    cx: (pen + right) / 2.0,
                    base,
                    strokes,
                    rings: Vec::new(),
                });
            }
            pen = right + style.spacing;
        }
        lines.push((placed, right));
    }
    let rows = lines.len().max(1) as f64;
    let bottom = -(rows - 1.0) * line_gap;
    align(lines, style, missing, style.height, bottom, bottom)
}

/// 輪郭のフォントで並べる。字の形の入っていない字は数えて、半角の幅を空ける。
fn shape_outline(text: &str, style: &Style, f: &OutlineFont) -> Shaped {
    let k = style.height / basis_units(f, style.basis);
    let kx = k * style.width;
    let em_gap = f.ascent - f.descent + f.leading;
    let line_gap = if em_gap > 0.0 {
        em_gap
    } else {
        1.2 * f.units_per_em
    } * k;
    let mut missing = 0;
    let mut lines: Vec<(Vec<Placed>, f64)> = Vec::new();
    for (row, src) in text.split('\n').enumerate() {
        let base = -(row as f64) * line_gap;
        let mut placed = Vec::new();
        let mut pen = 0.0;
        let mut right: f64 = 0.0;
        for ch in src.chars().filter(|c| *c != '\r') {
            let g = f.glyph(ch);
            if g.is_none() {
                missing += 1;
            }
            right = pen + advance(f, ch) * kx;
            let contours = g.and_then(|g| g.contours().ok()).unwrap_or_default();
            let rings: Vec<Vec<(Segment, bool)>> = contours
                .iter()
                .map(|c| {
                    let mm: outline::Contour = c
                        .iter()
                        .map(|p| p.map(|q| [pen + q[0] * kx, q[1] * k + base]))
                        .collect();
                    glyph::approximate(&mm, glyph::TOLERANCE)
                })
                .filter(|r| !r.is_empty())
                .collect();
            if !rings.is_empty() {
                placed.push(Placed {
                    cx: (pen + right) / 2.0,
                    base,
                    strokes: Vec::new(),
                    rings,
                });
            }
            pen = right + style.spacing;
        }
        lines.push((placed, right));
    }
    let rows = lines.len().max(1) as f64;
    let last = -(rows - 1.0) * line_gap;
    // 字の枠の上下: 大文字の高さの基準なら大文字の上、字の枠の基準なら字の枠(ascent・descent)
    let (top, low) = match style.basis {
        HeightBasis::Cap => (style.height, last),
        HeightBasis::Em => (f.ascent * k, last + f.descent * k),
    };
    align(lines, style, missing, top, last, low)
}

/// 行を揃える。`top` は 1 行目の上、`last` は最後の行の基準線、`low` は中央揃えで使う下の端。
fn align(
    lines: Vec<(Vec<Placed>, f64)>,
    style: &Style,
    missing: usize,
    top: f64,
    last: f64,
    low: f64,
) -> Shaped {
    let dy = match style.valign {
        VAlign::Baseline => 0.0,
        VAlign::Bottom => -last,
        VAlign::Middle => -(top + low) / 2.0,
        VAlign::Top => -top,
    };
    let mut out = Vec::new();
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    let mut grow = |q: [f64; 2]| {
        for i in 0..2 {
            lo[i] = lo[i].min(q[i]);
            hi[i] = hi[i].max(q[i]);
        }
    };
    for (placed, w) in lines {
        let dx = match style.align {
            HAlign::Left => 0.0,
            HAlign::Center => -w / 2.0,
            HAlign::Right => -w,
        };
        for mut g in placed {
            for (a, b) in &mut g.strokes {
                for q in [a, b] {
                    q[0] += dx;
                    q[1] += dy;
                    grow(*q);
                }
            }
            let shift = |q: [f64; 2]| [q[0] + dx, q[1] + dy];
            for r in &mut g.rings {
                for (seg, back) in r.iter_mut() {
                    *seg = glyph::rigid(seg, &shift, 0.0);
                    let e = ProfileEdge {
                        name: String::new(),
                        segment: seg.clone(),
                        reversed: *back,
                    };
                    for q in e.polyline(64) {
                        grow(q);
                    }
                }
            }
            g.cx += dx;
            g.base += dy;
            out.push(g);
        }
    }
    Shaped {
        glyphs: out,
        lo,
        hi,
        missing,
    }
}

/// 文字列を線にする。`origin` に `style` の揃えの点を合わせ、そのまわりに回す。
/// 枠は線の外接の四角形(4 本の線)。
pub fn lay_out(text: &str, style: &Style, origin: [f64; 2]) -> Laid {
    lay_out_with(text, style, None, origin)
}

/// [`lay_out`] の、輪郭のフォント `outline` も使える版。
pub fn lay_out_with(
    text: &str,
    style: &Style,
    outline: Option<&OutlineFont>,
    origin: [f64; 2],
) -> Laid {
    let sh = shape(text, style, outline);
    let (sin, cos) = style.angle.sin_cos();
    let place = |p: [f64; 2]| {
        [
            origin[0] + p[0] * cos - p[1] * sin,
            origin[1] + p[0] * sin + p[1] * cos,
        ]
    };
    let rings: Vec<Vec<(Segment, bool)>> = sh
        .glyphs
        .iter()
        .flat_map(|g| g.rings.iter())
        .map(|r| {
            r.iter()
                .map(|(s, back)| (glyph::rigid(s, &place, style.angle), *back))
                .collect()
        })
        .collect();
    let segments: Vec<Segment> = sh
        .glyphs
        .iter()
        .flat_map(|g| g.strokes.iter())
        .map(|(a, b)| Segment::Line {
            a: place(*a),
            b: place(*b),
        })
        .chain(rings.iter().flatten().map(|(s, _)| s.clone()))
        .collect();
    let frame = if segments.is_empty() {
        Vec::new()
    } else {
        let (lo, hi) = (sh.lo, sh.hi);
        let c = [lo, [hi[0], lo[1]], hi, [lo[0], hi[1]]].map(place);
        (0..4)
            .map(|i| Segment::Line {
                a: c[i],
                b: c[(i + 1) % 4],
            })
            .collect()
    };
    Laid {
        segments,
        frame,
        missing: sh.missing,
        warnings: Vec::new(),
        rings,
    }
}

/// 文字列を線にする。`origin` は 1 文字目の左下(基準線の上)、`h` は大文字の高さ。
/// 置き換えた文字の数も返す。
pub fn text_segments(text: &str, font: Font, origin: [f64; 2], h: f64) -> (Vec<Segment>, usize) {
    let l = lay_out(text, &Style::new(font, h), origin);
    (l.segments, l.missing)
}

fn length(ctx: &EvalCtx<'_>, src: &str, what: &str) -> Result<f64, EvalError> {
    let q = ctx.expr(src)?;
    if q.dim != Dim::LENGTH {
        return Err(EvalError::InvalidData(format!(
            "{what} `{src}` は長さではありません(値: {q})"
        )));
    }
    Ok(q.value)
}

fn exprs(t: &mut Text) -> Vec<&mut String> {
    let mut v = vec![&mut t.height, &mut t.x, &mut t.y];
    v.extend(
        [
            &mut t.angle,
            &mut t.spacing,
            &mut t.width,
            &mut t.fit_width,
            &mut t.stroke,
        ]
        .into_iter()
        .flatten(),
    );
    v
}

impl Text {
    fn style(&self, ctx: &EvalCtx<'_>) -> Result<Style, EvalError> {
        let h = length(ctx, &self.height, "文字の高さ")?;
        if h <= 0.0 {
            return Err(EvalError::InvalidData(
                "文字の高さは 0 より大きくしてください".into(),
            ));
        }
        let angle = match &self.angle {
            Some(src) => {
                let q = ctx.expr(src)?;
                if q.dim != Dim::ANGLE {
                    return Err(EvalError::InvalidData(format!(
                        "回転 `{src}` は角度ではありません(値: {q})"
                    )));
                }
                q.value
            }
            None => 0.0,
        };
        let spacing = match &self.spacing {
            Some(src) => length(ctx, src, "字の間隔")?,
            None => 0.0,
        };
        let width = match &self.width {
            Some(src) => {
                let q = ctx.expr(src)?;
                if q.dim != Dim::NONE || q.value <= 0.0 {
                    return Err(EvalError::InvalidData(format!(
                        "幅の倍率 `{src}` は 0 より大きい数にしてください(値: {q})"
                    )));
                }
                q.value
            }
            None => 1.0,
        };
        let style = Style {
            font: self.font,
            height: h,
            spacing,
            width,
            align: self.align,
            valign: self.valign,
            angle,
            basis: self.basis,
        };
        match &self.fit_width {
            Some(src) => {
                let w = length(ctx, src, "合わせる幅")?;
                style
                    .fit_width_with(
                        &self.text,
                        w,
                        self.fit == FitMode::Scale,
                        self.outline.as_ref(),
                    )
                    .map_err(EvalError::InvalidData)
            }
            None => Ok(style),
        }
    }

    /// 沿わせる線と、その平面。つながった線をたどる時に 3 本以上が出会う所で止めたら、3 つ目が真。
    fn path(&self, ctx: &EvalCtx<'_>) -> Result<Option<(kmb_core::Plane, Path, bool)>, EvalError> {
        let Some(sketch) = self.path else {
            return Ok(None);
        };
        if self.anchor.is_some() {
            return Err(EvalError::InvalidData(
                "基準の点と沿わせる線は一緒に使えません(どちらかを空にしてください)".into(),
            ));
        }
        let not_sketch = || EvalError::InvalidData(format!("{sketch} はスケッチではありません"));
        let paths = kmb_sketch::paths_of(ctx.output(sketch, "paths")?).ok_or_else(not_sketch)?;
        let plane = profile_value(ctx.output(sketch, "profile")?)
            .ok_or_else(not_sketch)?
            .plane;
        let found = match self.path_entity {
            Some(id) => paths
                .iter()
                .find(|(e, _)| *e == id)
                .cloned()
                .ok_or_else(|| {
                    EvalError::InvalidData(format!(
                        "沿わせる線 {id} がスケッチにありません(線分・円弧・円を選んでください)"
                    ))
                })?,
            None => paths.first().cloned().ok_or_else(|| {
                EvalError::InvalidData(
                    "沿わせる線がありません(スケッチに線分・円弧・円を描いてください)".into(),
                )
            })?,
        };
        // 楕円・スプラインなどは何区間にもなる(同じ要素の番号が続く)ので、その区間はいつもつなぐ
        let first = paths
            .iter()
            .position(|(e, s)| *e == found.0 && *s == found.1)
            .unwrap_or(0);
        if !self.chain {
            let same: Vec<Segment> = paths
                .iter()
                .enumerate()
                .filter(|(i, (e, _))| *e == found.0 && *i != first)
                .map(|(_, (_, s))| s.clone())
                .collect();
            if same.is_empty() {
                return Ok(Some((plane, Path::new(found.1, self.reverse), false)));
            }
            let (path, _) = Path::chain(found.1, &same, self.reverse);
            return Ok(Some((plane, path, false)));
        }
        let others: Vec<Segment> = paths
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != first)
            .map(|(_, (_, s))| s.clone())
            .collect();
        let (path, branched) = Path::chain(found.1, &others, self.reverse);
        Ok(Some((plane, path, branched)))
    }

    /// 置く平面と基準の位置(平面の座標)。
    fn origin(&self, ctx: &EvalCtx<'_>) -> Result<(kmb_core::Plane, [f64; 2]), EvalError> {
        let offset = [
            length(ctx, &self.x, "位置 x")?,
            length(ctx, &self.y, "位置 y")?,
        ];
        let Some(sketch) = self.anchor else {
            return Ok((resolve_plane(&self.plane, ctx)?, offset));
        };
        let not_sketch = || EvalError::InvalidData(format!("{sketch} はスケッチではありません"));
        let sol = solution_of(ctx.output(sketch, "sketch")?).ok_or_else(not_sketch)?;
        let plane = profile_value(ctx.output(sketch, "profile")?)
            .ok_or_else(not_sketch)?
            .plane;
        let point = match self.anchor_point {
            Some(p) => p,
            None => *sol.lone.first().ok_or_else(|| {
                EvalError::InvalidData(
                    "基準の点がありません(スケッチに、線や円に使っていない点を描いてください)"
                        .into(),
                )
            })?,
        };
        let p = sol.points.get(&point).ok_or_else(|| {
            EvalError::InvalidData(format!("基準の点 {point} がスケッチにありません"))
        })?;
        Ok((plane, [p[0] + offset[0], p[1] + offset[1]]))
    }
}

impl Feature for Text {
    fn inputs(&self) -> Result<Vec<InputRef>, EvalError> {
        let mut out = Vec::new();
        if let Some(s) = self.path {
            for slot in ["paths", "profile"] {
                out.push(InputRef::Output {
                    feature: s,
                    slot: slot.into(),
                });
            }
        }
        if let Some(s) = self.anchor {
            for slot in ["sketch", "profile"] {
                out.push(InputRef::Output {
                    feature: s,
                    slot: slot.into(),
                });
            }
        } else {
            out.extend(self.plane.inputs());
        }
        for e in exprs(&mut self.clone()) {
            for i in expr_inputs(e)? {
                if !out.contains(&i) {
                    out.push(i);
                }
            }
        }
        Ok(out)
    }

    fn evaluate(&self, ctx: &mut EvalCtx<'_>) -> Result<Outputs, EvalError> {
        let style = self.style(ctx)?;
        let (plane, laid) = match self.path(ctx)? {
            Some((plane, path, branched)) => {
                let offset = [
                    length(ctx, &self.x, "線の上の位置")?,
                    length(ctx, &self.y, "線からの距離")?,
                ];
                let mut laid =
                    lay_along_with(&self.text, &style, self.outline.as_ref(), &path, offset)
                        .map_err(EvalError::InvalidData)?;
                if branched {
                    laid.warnings
                        .push("線が 3 本以上出会う所があり、そこまでの線に沿わせました".into());
                }
                (plane, laid)
            }
            None => {
                let (plane, origin) = self.origin(ctx)?;
                (
                    plane,
                    lay_out_with(&self.text, &style, self.outline.as_ref(), origin),
                )
            }
        };
        if laid.segments.is_empty() {
            // 輪郭のフォントで、どの字も形が文書に入っていない(フォントの無い PC で打った字)
            if let Some(f) = self.outline.as_ref().filter(|_| laid.missing > 0) {
                let absent: String = f.missing(&self.text).into_iter().take(12).collect();
                return Err(EvalError::InvalidData(format!(
                    "字「{absent}」の形が文書に入っていません。フォント「{}」の入った PC で文字を直すと入ります。フォントに無い字なら別のフォントを選んでください",
                    f.label_or_family()
                )));
            }
            return Err(EvalError::InvalidData("文字がありません".into()));
        }
        // 押し出しで彫る・浮かせるための閉じた領域。輪郭のフォントは字の輪から、1 本線は線を
        // 太らせて作る
        let regions = if self.outline.is_some() {
            glyph::regions(laid.rings.clone())
        } else {
            let stroke = match &self.stroke {
                Some(src) if !src.trim().is_empty() => length(ctx, src, "線の太さ")?,
                _ => length(ctx, &self.height, "文字の高さ")? * 0.12,
            };
            if stroke <= 0.0 {
                return Err(EvalError::InvalidData(
                    "線の太さは 0 より大きくしてください".into(),
                ));
            }
            thicken(&laid.segments, stroke)
        };
        let profile = kmb_core::Profile { plane, regions };
        let mut out: Outputs = [
            (
                "curves".to_string(),
                Curves {
                    plane,
                    segments: laid.segments,
                }
                .to_value(),
            ),
            (
                "frame".to_string(),
                Curves {
                    plane,
                    segments: laid.frame,
                }
                .to_value(),
            ),
            ("profile".to_string(), profile.to_value()),
        ]
        .into();
        let mut warnings = laid.warnings;
        match &self.outline {
            Some(f) if laid.missing > 0 => {
                let absent: String = f.missing(&self.text).into_iter().take(12).collect();
                warnings.push(format!(
                    "字「{absent}」の形が文書に入っていません(空けました)。フォント「{}」の入った PC で文字を直すと入ります。フォントに無い字なら別のフォントを選んでください",
                    f.label_or_family()
                ));
            }
            Some(_) => {
                if self.stroke.is_some() {
                    warnings.push("輪郭のフォントでは線の太さを使いません".into());
                }
            }
            None if laid.missing > 0 => {
                warnings.push(format!(
                    "このフォントにない文字 {} 個を「?」にしました(使えるのは英数字と記号)",
                    laid.missing
                ));
            }
            None => {}
        }
        if self.path.is_some() && self.angle.is_some() {
            warnings.push("線に沿わせる時は回転を使いません(字は線の向きに回ります)".into());
        }
        if !warnings.is_empty() {
            kmb_core::add_warnings(&mut out, warnings);
        }
        Ok(out)
    }

    fn rename_param(&self, old: &str, new: &str) -> Result<Option<serde_json::Value>, EvalError> {
        let mut c = self.clone();
        let mut changed = false;
        for e in exprs(&mut c) {
            let r = kmb_core::rename_reference(e, old, new)
                .map_err(|x| EvalError::InvalidData(x.to_string()))?;
            if r != *e {
                *e = r;
                changed = true;
            }
        }
        Ok(changed.then(|| serde_json::to_value(&c).expect("JSON にできる")))
    }
}

/// 1 本線の文字を太さ `width` の線にして、閉じた領域にする(押し出しで彫る・浮かせる)。
/// 端と角は丸め、重なる線は 1 つの領域にまとめる(「O」「A」の内側は穴になる)。
pub fn thicken(segments: &[Segment], width: f64) -> Vec<kmb_core::Region> {
    use i_overlay::mesh::float::stroke::offset::StrokeOffset;
    use i_overlay::mesh::float::style::{LineCap, LineJoin, StrokeStyle};
    use kmb_core::{ProfileEdge, ProfileLoop, Region};
    // 続いている区間を 1 本の折れ線にまとめる
    let mut paths: Vec<Vec<[f64; 2]>> = Vec::new();
    for s in segments {
        let mut pts = s.polyline(s.steps_per_turn(width * 0.05));
        pts.push(s.end_point());
        match paths.last_mut() {
            Some(last)
                if last
                    .last()
                    .is_some_and(|q| (q[0] - pts[0][0]).hypot(q[1] - pts[0][1]) < 1e-9) =>
            {
                last.extend(pts.into_iter().skip(1));
            }
            _ => paths.push(pts),
        }
    }
    let style = StrokeStyle::new(width)
        .line_join(LineJoin::Round(0.25))
        .start_cap(LineCap::Round(0.25))
        .end_cap(LineCap::Round(0.25));
    let shapes = paths.stroke(style, false);
    let to_loop = |k: usize, c: &[[f64; 2]], ccw: bool| {
        let n = c.len();
        let mut l = ProfileLoop {
            edges: (0..n)
                .map(|i| ProfileEdge {
                    name: format!("t{k}.{i}"),
                    segment: Segment::Line {
                        a: c[i],
                        b: c[(i + 1) % n],
                    },
                    reversed: false,
                })
                .collect(),
        };
        if (l.signed_area() > 0.0) != ccw {
            l.reverse();
        }
        l
    };
    let mut k = 0;
    shapes
        .iter()
        .filter(|sh| sh.first().is_some_and(|c| c.len() >= 3))
        .map(|sh| {
            let mut loops = sh.iter().filter(|c| c.len() >= 3).map(|c| {
                k += 1;
                (k, c)
            });
            let (k0, outer) = loops.next().expect("外側の輪がある");
            Region {
                outer: to_loop(k0, outer, true),
                holes: loops.map(|(k, c)| to_loop(k, c, false)).collect(),
            }
        })
        .collect()
}

pub fn register(reg: &mut Registry) {
    use kmb_core::Field;
    reg.register_described(
        kmb_core::Descriptor::new(TEXT, kmb_core::category::SKETCH, "文字")
            .icon("textf")
            .hint("平面の上に文字を置く。1 本線のフォント(英数字と記号)は彫刻でなぞるか、線の太さで太らせて押し出す。輪郭のフォント(日本語も)は押し出し・ポケット・V 彫りで彫る・浮かせる")
            .aliases("もじ text てきすと ふぉんと font にほんご ないれ")
            .field(Field::text("text", "文字"))
            .field(Field::expr("height", "文字の高さ").length())
            .field(
                Field::choice(
                    "basis",
                    "高さの基準",
                    &[("cap", "大文字の高さ"), ("em", "字の枠")],
                )
                .unless("outline.family", "")
                .help("字の枠は全角の字の大きさ(日本語の名入れ)。1 本線のフォントはいつも大文字の高さ"),
            )
            .field(Field::choice(
                "align",
                "横の揃え",
                &[("left", "左"), ("center", "中央"), ("right", "右")],
            ))
            .field(Field::choice(
                "valign",
                "縦の揃え",
                &[
                    ("baseline", "1 行目の基準線"),
                    ("bottom", "最後の行の基準線"),
                    ("middle", "中央"),
                    ("top", "大文字の上"),
                ],
            ))
            .field(
                Field::opt_output("anchor", "基準の点のスケッチ", "sketch")
                    .help("スケッチの単独の点に合わせる。点は拘束で動かせる"),
            )
            .field(Field::expr("x", "位置 x").length().help("基準の点があれば、そこからのずれ"))
            .field(Field::expr("y", "位置 y").length())
            .field(Field::opt_expr("angle", "回転").angle().help("空なら 0。例: 30 deg"))
            .field(Field::opt_expr("spacing", "字の間隔").length().help("空なら 0。字と字の間に足す長さ"))
            .field(Field::opt_expr("width", "幅の倍率").ratio().help("空なら 1"))
            .field(
                Field::opt_expr("fit_width", "幅に合わせる").length()
                    .help("最も長い行をこの幅にする。空なら合わせない"),
            )
            .field(Field::choice(
                "fit",
                "合わせ方",
                &[("stretch", "字の幅だけ変える"), ("scale", "高さも変える")],
            ))
            .field(
                Field::opt_output("path", "沿わせる線のスケッチ", "paths")
                    .help("線分・円弧・円に沿って並べる。位置 x は線の上の長さ、y は線からの距離"),
            )
            .field(Field::bool("reverse", "線を逆にたどる").help(
                "円と円弧は反時計回りで字の上が中心を向く。逆にたどると外を向く(円の上側に正立)",
            ))
            .field(Field::bool("chain", "つながった線もたどる").help(
                "選んだ線に端のつながる線分・円弧も続けてたどる(折れ線や角を丸めた形に沿わせる)。3 本以上が出会う所で止まる",
            ))
            .field(
                Field::choice(
                    "font",
                    "1 本線のフォント",
                    &[("sans", "飾りなし"), ("roman", "飾りあり")],
                )
                .only("outline.family", "", ""),
            )
            .field(
                Field::info("outline.family", "輪郭のフォント").unless("outline.family", ""),
            )
            .field(
                Field::opt_expr("stroke", "線の太さ").length()
                    .help("押し出しで彫る・浮かせる時の太さ。空なら大文字の高さの 12%")
                    .only("outline.family", "", ""),
            )
            .template(serde_json::json!({"text": "TEXT", "height": "5 mm"})),
        |d| {
            let t: Text = serde_json::from_value(d.clone())
                .map_err(|e| EvalError::InvalidData(e.to_string()))?;
            Ok(Box::new(t))
        },
    );
}

/// この crate の名乗り(文字の種類と、文字を置く・分解する操作)。組み立て(kmb-bundle)が 1 回だけ並べる。
pub fn module() -> kmb_core::Module {
    kmb_core::Module::new("kmb-text", register).with_extension(TextCommands::default())
}

#[cfg(test)]
mod outline_tests;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod test_fonts;
