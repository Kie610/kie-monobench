//! SVG の読み込み。XML は要素名・属性・入れ子だけを自分で読む(CDATA・コメント・
//! `<?xml ?>`・`<!DOCTYPE>` は読み飛ばす)。
//!
//! 座標は根の `svg` の `width`・`height`・`viewBox` で mm に直し、y を上向きに反転する
//! (紙の下の端が y = 0)。単位の無い `width` は 96 dpi の px とみなす。

use kmb_core::kernel::P2;
use kmb_sketch::{Affine, Imported, Shape, ellipse_from_conjugate, transform_shape};
use std::collections::HashMap;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

const PX_MM: f64 = 25.4 / 96.0;

/// 開始タグ・終了タグ
enum Tag<'a> {
    Start {
        name: &'a str,
        attrs: Vec<(&'a str, String)>,
        empty: bool,
    },
    End(&'a str),
}

/// 最小限の XML の字句読み。壊れていたら、そこまでのタグを返す。
fn tags(s: &str) -> Vec<Tag<'_>> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let find = |from: usize, pat: &str| s.get(from..).and_then(|r| r.find(pat)).map(|k| from + k);
    while let Some(lt) = find(i, "<") {
        let rest = &s[lt..];
        if rest.starts_with("<!--") {
            match find(lt + 4, "-->") {
                Some(e) => i = e + 3,
                None => break,
            }
            continue;
        }
        if rest.starts_with("<![CDATA[") {
            match find(lt + 9, "]]>") {
                Some(e) => i = e + 3,
                None => break,
            }
            continue;
        }
        if rest.starts_with("<?") {
            match find(lt + 2, "?>") {
                Some(e) => i = e + 2,
                None => break,
            }
            continue;
        }
        if rest.starts_with("<!") {
            // DOCTYPE など。[ ] の中の > は数えない
            let mut depth = 0i32;
            let mut j = lt + 2;
            while j < b.len() {
                match b[j] {
                    b'[' => depth += 1,
                    b']' => depth -= 1,
                    b'>' if depth <= 0 => break,
                    _ => {}
                }
                j += 1;
            }
            i = j + 1;
            continue;
        }
        let is_end = rest.starts_with("</");
        let mut j = lt + if is_end { 2 } else { 1 };
        let name_start = j;
        while j < b.len() && !b[j].is_ascii_whitespace() && b[j] != b'>' && b[j] != b'/' {
            j += 1;
        }
        let name = &s[name_start..j];
        if name.is_empty() {
            i = lt + 1;
            continue;
        }
        if is_end {
            match find(j, ">") {
                Some(e) => i = e + 1,
                None => break,
            }
            out.push(Tag::End(name));
            continue;
        }
        let mut attrs = Vec::new();
        let mut empty = false;
        loop {
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            if j >= b.len() {
                return out;
            }
            if b[j] == b'>' {
                j += 1;
                break;
            }
            if b[j] == b'/' {
                empty = true;
                j += 1;
                continue;
            }
            let an = j;
            while j < b.len() && !b[j].is_ascii_whitespace() && !matches!(b[j], b'=' | b'>' | b'/')
            {
                j += 1;
            }
            let aname = &s[an..j];
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < b.len() && b[j] == b'=' {
                j += 1;
                while j < b.len() && b[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j >= b.len() {
                    return out;
                }
                let q = b[j];
                if q == b'"' || q == b'\'' {
                    let Some(e) = find(j + 1, if q == b'"' { "\"" } else { "'" }) else {
                        return out;
                    };
                    attrs.push((aname, unescape(&s[j + 1..e])));
                    j = e + 1;
                } else {
                    // 引用符の無い値(正しくないが受け入れる)
                    let vs = j;
                    while j < b.len() && !b[j].is_ascii_whitespace() && b[j] != b'>' {
                        j += 1;
                    }
                    attrs.push((aname, unescape(&s[vs..j])));
                }
            } else if aname.is_empty() {
                j += 1;
            } else {
                attrs.push((aname, String::new()));
            }
        }
        i = j;
        out.push(Tag::Start { name, attrs, empty });
    }
    out
}

/// 文字参照を戻す(`&amp;` など 5 つと数値の参照)
fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(k) = rest.find('&') {
        out.push_str(&rest[..k]);
        rest = &rest[k..];
        let Some(e) = rest.find(';').filter(|e| *e <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let ent = &rest[1..e];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => ent
                .strip_prefix("#x")
                .or_else(|| ent.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| ent.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[e + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// 数の並びを読む字句(path の d、points、viewBox、transform の引数)
struct Lexer<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Lexer<'a> {
    fn new(s: &'a str) -> Self {
        Lexer {
            b: s.as_bytes(),
            i: 0,
        }
    }

    fn skip_sep(&mut self) {
        while self.i < self.b.len()
            && (self.b[self.i].is_ascii_whitespace() || self.b[self.i] == b',')
        {
            self.i += 1;
        }
    }

    /// 次がコマンドの文字ならそれを返す(進めない)
    fn peek_cmd(&mut self) -> Option<u8> {
        self.skip_sep();
        let c = *self.b.get(self.i)?;
        (c.is_ascii_alphabetic() && c != b'e' && c != b'E').then_some(c)
    }

    fn at_end(&mut self) -> bool {
        self.skip_sep();
        self.i >= self.b.len()
    }

    fn number(&mut self) -> Option<f64> {
        self.skip_sep();
        let b = self.b;
        let start = self.i;
        let mut j = self.i;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        let mut digits = false;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
            digits = true;
        }
        if j < b.len() && b[j] == b'.' {
            j += 1;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
                digits = true;
            }
        }
        if !digits {
            return None;
        }
        if j < b.len() && (b[j] == b'e' || b[j] == b'E') {
            let mut k = j + 1;
            if k < b.len() && (b[k] == b'+' || b[k] == b'-') {
                k += 1;
            }
            if k < b.len() && b[k].is_ascii_digit() {
                while k < b.len() && b[k].is_ascii_digit() {
                    k += 1;
                }
                j = k;
            }
        }
        let v = std::str::from_utf8(&b[start..j])
            .ok()?
            .parse::<f64>()
            .ok()?;
        self.i = j;
        v.is_finite().then_some(v)
    }

    /// 円弧の旗(1 文字の 0 か 1。区切りが無くてもよい)
    fn flag(&mut self) -> Option<bool> {
        self.skip_sep();
        let c = *self.b.get(self.i)?;
        let v = match c {
            b'0' => false,
            b'1' => true,
            _ => return None,
        };
        self.i += 1;
        Some(v)
    }
}

fn numbers(s: &str) -> Vec<f64> {
    let mut l = Lexer::new(s);
    let mut out = Vec::new();
    while let Some(v) = l.number() {
        out.push(v);
    }
    out
}

/// 長さの単位(mm に直す倍率)。無い・px なら 96 dpi の px。% などは `None`。
fn unit_mm(u: &str) -> Option<f64> {
    Some(match u.trim() {
        "" | "px" => PX_MM,
        "mm" => 1.0,
        "cm" => 10.0,
        "in" => 25.4,
        "pt" => 25.4 / 72.0,
        "pc" => 25.4 / 6.0,
        "q" | "Q" => 0.25,
        _ => return None,
    })
}

/// 単位つきの長さを mm で読む
fn length_mm(s: &str) -> Option<f64> {
    let s = s.trim();
    let k = s
        .find(|c: char| (c.is_ascii_alphabetic() && c != 'e' && c != 'E') || c == '%')
        .unwrap_or(s.len());
    let v: f64 = s[..k].trim().parse().ok()?;
    Some(v * unit_mm(&s[k..])?).filter(|x| x.is_finite())
}

/// 要素の属性の長さを利用者単位(user unit)で読む。mm などの単位は px に直す。
fn length_user(s: Option<&str>) -> f64 {
    let Some(s) = s else { return 0.0 };
    match length_mm(s) {
        Some(mm) => mm / PX_MM,
        None => 0.0,
    }
}

/// transform 属性を読む
fn parse_transform(s: &str) -> Affine {
    let mut m = Affine::IDENTITY;
    let mut rest = s;
    while let Some(open) = rest.find('(') {
        let name = rest[..open].trim_matches(|c: char| c.is_whitespace() || c == ',');
        let Some(close) = rest[open..].find(')') else {
            break;
        };
        let args = numbers(&rest[open + 1..open + close]);
        rest = &rest[open + close + 1..];
        let a = |k: usize| args.get(k).copied();
        let t = match (name, args.len()) {
            ("matrix", 6) => Affine([args[0], args[1], args[2], args[3], args[4], args[5]]),
            ("translate", 1 | 2) => Affine::translate(args[0], a(1).unwrap_or(0.0)),
            ("scale", 1 | 2) => Affine::scale(args[0], a(1).unwrap_or(args[0])),
            ("rotate", 1) => Affine::rotate(args[0].to_radians()),
            ("rotate", 3) => Affine::translate(args[1], args[2])
                .then_after(&Affine::rotate(args[0].to_radians()))
                .then_after(&Affine::translate(-args[1], -args[2])),
            ("skewX", 1) => Affine([1.0, 0.0, args[0].to_radians().tan(), 1.0, 0.0, 0.0]),
            ("skewY", 1) => Affine([1.0, args[0].to_radians().tan(), 0.0, 1.0, 0.0, 0.0]),
            // 読めない変換があれば、変換全体を無効にする(SVG の決まり)
            _ => return Affine::IDENTITY,
        };
        m = m.then_after(&t);
    }
    m
}

/// SVG の楕円弧(端点の形)を中心の形にする(W3C SVG 1.1 実装の注記 F.6.5・F.6.6)
#[allow(clippy::too_many_arguments)]
fn svg_arc(
    p1: P2,
    rx: f64,
    ry: f64,
    phi_deg: f64,
    large: bool,
    sweep: bool,
    p2: P2,
    out: &mut Vec<Shape>,
) {
    if p1 == p2 {
        return;
    }
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx == 0.0 || ry == 0.0 {
        out.push(Shape::Line { a: p1, b: p2 });
        return;
    }
    let (s, c) = phi_deg.to_radians().sin_cos();
    let (dx, dy) = ((p1[0] - p2[0]) / 2.0, (p1[1] - p2[1]) / 2.0);
    let x1 = c * dx + s * dy;
    let y1 = -s * dx + c * dy;
    let lam = x1 * x1 / (rx * rx) + y1 * y1 / (ry * ry);
    if lam > 1.0 {
        let k = lam.sqrt();
        rx *= k;
        ry *= k;
    }
    let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut coef = if den > 0.0 {
        (num / den).max(0.0).sqrt()
    } else {
        0.0
    };
    if large == sweep {
        coef = -coef;
    }
    let cx1 = coef * rx * y1 / ry;
    let cy1 = -coef * ry * x1 / rx;
    let center = [
        c * cx1 - s * cy1 + (p1[0] + p2[0]) / 2.0,
        s * cx1 + c * cy1 + (p1[1] + p2[1]) / 2.0,
    ];
    let u = [(x1 - cx1) / rx, (y1 - cy1) / ry];
    let v = [(-x1 - cx1) / rx, (-y1 - cy1) / ry];
    let t1 = u[1].atan2(u[0]);
    let mut dt = (u[0] * v[1] - u[1] * v[0]).atan2(u[0] * v[0] + u[1] * v[1]);
    if !sweep && dt > 0.0 {
        dt -= TAU;
    } else if sweep && dt < 0.0 {
        dt += TAU;
    }
    let major = [rx * c, rx * s];
    let minor = [-ry * s, ry * c];
    out.push(ellipse_from_conjugate(center, major, minor, t1, t1 + dt));
}

/// path の d を形にする(利用者単位)。読めないところで止め、そこまでを返す。
fn parse_path(d: &str) -> Vec<Shape> {
    let mut out = Vec::new();
    let mut l = Lexer::new(d);
    let mut cur = [0.0, 0.0];
    let mut start = [0.0, 0.0];
    // 前のコマンドの制御点(S・T の反転に使う)
    let mut last_c: Option<P2> = None;
    let mut last_q: Option<P2> = None;
    let mut cmd: Option<u8> = None;
    let line = |out: &mut Vec<Shape>, a: P2, b: P2| {
        if a != b {
            out.push(Shape::Line { a, b });
        }
    };
    loop {
        if l.at_end() {
            break;
        }
        let c = match l.peek_cmd() {
            Some(c) => {
                l.i += 1;
                c
            }
            None => match cmd {
                // 続きの数は同じコマンド(M の後は L)
                Some(b'M') => b'L',
                Some(b'm') => b'l',
                Some(c) if !matches!(c, b'Z' | b'z') => c,
                _ => break,
            },
        };
        let rel = c.is_ascii_lowercase();
        let base = if rel { cur } else { [0.0, 0.0] };
        let pt = |l: &mut Lexer| -> Option<P2> {
            let x = l.number()?;
            let y = l.number()?;
            Some([base[0] + x, base[1] + y])
        };
        let (mut nc, mut nq) = (None, None);
        match c.to_ascii_uppercase() {
            b'M' => {
                let Some(p) = pt(&mut l) else { break };
                cur = p;
                start = p;
            }
            b'L' => {
                let Some(p) = pt(&mut l) else { break };
                line(&mut out, cur, p);
                cur = p;
            }
            b'H' => {
                let Some(x) = l.number() else { break };
                let p = [if rel { cur[0] + x } else { x }, cur[1]];
                line(&mut out, cur, p);
                cur = p;
            }
            b'V' => {
                let Some(y) = l.number() else { break };
                let p = [cur[0], if rel { cur[1] + y } else { y }];
                line(&mut out, cur, p);
                cur = p;
            }
            b'C' | b'S' => {
                let c1 = if c.eq_ignore_ascii_case(&b'C') {
                    let Some(p) = pt(&mut l) else { break };
                    p
                } else {
                    match last_c {
                        Some(q) => [2.0 * cur[0] - q[0], 2.0 * cur[1] - q[1]],
                        None => cur,
                    }
                };
                let (Some(c2), Some(p)) = (pt(&mut l), pt(&mut l)) else {
                    break;
                };
                if !(cur == c1 && c1 == c2 && c2 == p) {
                    out.push(Shape::Bezier {
                        p: [cur, c1, c2, p],
                    });
                }
                nc = Some(c2);
                cur = p;
            }
            b'Q' | b'T' => {
                let q = if c.eq_ignore_ascii_case(&b'Q') {
                    let Some(p) = pt(&mut l) else { break };
                    p
                } else {
                    match last_q {
                        Some(q) => [2.0 * cur[0] - q[0], 2.0 * cur[1] - q[1]],
                        None => cur,
                    }
                };
                let Some(p) = pt(&mut l) else { break };
                // 2 次を 3 次にする(正確に同じ曲線)
                let c1 = [
                    cur[0] + 2.0 / 3.0 * (q[0] - cur[0]),
                    cur[1] + 2.0 / 3.0 * (q[1] - cur[1]),
                ];
                let c2 = [
                    p[0] + 2.0 / 3.0 * (q[0] - p[0]),
                    p[1] + 2.0 / 3.0 * (q[1] - p[1]),
                ];
                if !(cur == q && q == p) {
                    out.push(Shape::Bezier {
                        p: [cur, c1, c2, p],
                    });
                }
                nq = Some(q);
                cur = p;
            }
            b'A' => {
                let (Some(rx), Some(ry), Some(phi)) = (l.number(), l.number(), l.number()) else {
                    break;
                };
                let (Some(large), Some(sw)) = (l.flag(), l.flag()) else {
                    break;
                };
                let Some(p) = pt(&mut l) else { break };
                svg_arc(cur, rx, ry, phi, large, sw, p, &mut out);
                cur = p;
            }
            b'Z' => {
                line(&mut out, cur, start);
                cur = start;
            }
            _ => break,
        }
        last_c = nc;
        last_q = nq;
        cmd = Some(c);
    }
    out
}

/// 根の svg の、利用者単位から mm(y 上向き)への変換
fn root_transform(attr: &dyn Fn(&str) -> Option<String>) -> Affine {
    let w = attr("width").and_then(|s| length_mm(&s));
    let h = attr("height").and_then(|s| length_mm(&s));
    let vb = attr("viewBox")
        .map(|s| numbers(&s))
        .filter(|v| v.len() == 4 && v[2] > 0.0 && v[3] > 0.0);
    let (m, height) = match vb {
        Some(v) => {
            let (minx, miny, vw, vh) = (v[0], v[1], v[2], v[3]);
            let (w, h) = match (w, h) {
                (Some(w), Some(h)) => (w, h),
                (Some(w), None) => (w, w * vh / vw),
                (None, Some(h)) => (h * vw / vh, h),
                (None, None) => (vw * PX_MM, vh * PX_MM),
            };
            let par = attr("preserveAspectRatio").unwrap_or_default();
            let mut words = par.split_whitespace();
            let align = words.next().unwrap_or("xMidYMid");
            let slice = words.next() == Some("slice");
            let (sx, sy) = (w / vw, h / vh);
            let m = if align == "none" {
                Affine([sx, 0.0, 0.0, sy, -minx * sx, -miny * sy])
            } else {
                let s = if slice { sx.max(sy) } else { sx.min(sy) };
                let ax = if align.starts_with("xMin") {
                    0.0
                } else if align.starts_with("xMax") {
                    1.0
                } else {
                    0.5
                };
                let ay = if align.ends_with("YMin") {
                    0.0
                } else if align.ends_with("YMax") {
                    1.0
                } else {
                    0.5
                };
                Affine([
                    s,
                    0.0,
                    0.0,
                    s,
                    (w - vw * s) * ax - minx * s,
                    (h - vh * s) * ay - miny * s,
                ])
            };
            (m, Some(h))
        }
        None => (Affine::scale(PX_MM, PX_MM), h),
    };
    // y を反転する。高さが分かれば紙の下の端を y = 0 にする
    Affine([1.0, 0.0, 0.0, -1.0, 0.0, height.unwrap_or(0.0)]).then_after(&m)
}

/// 描かない入れ物(中の要素は参照されて初めて描かれる)
const HIDDEN: &[&str] = &[
    "defs",
    "clipPath",
    "mask",
    "symbol",
    "marker",
    "pattern",
    "metadata",
    "title",
    "desc",
    "style",
    "script",
    "linearGradient",
    "radialGradient",
    "filter",
    "font",
    "font-face",
];
/// 読めない(画面で知らせる)要素
const UNSUPPORTED: &[&str] = &["text", "image", "use", "foreignObject", "textPath"];

struct Frame<'a> {
    name: &'a str,
    m: Affine,
    hidden: bool,
}

/// SVG を読む。path・rect・circle・ellipse・line・polyline・polygon を形にし、
/// g と要素の transform を当てる。text・image・use などは数えて読み飛ばす。
pub fn from_svg(text: &str) -> Result<Imported, String> {
    let mut shapes: Vec<Shape> = Vec::new();
    let mut skipped: HashMap<String, usize> = HashMap::new();
    let mut stack: Vec<Frame> = Vec::new();
    let mut root: Option<Affine> = None;
    for tag in tags(text) {
        let (name, attrs, empty) = match tag {
            Tag::End(name) => {
                if let Some(k) = stack.iter().rposition(|f| f.name == name) {
                    stack.truncate(k);
                }
                continue;
            }
            Tag::Start { name, attrs, empty } => (name, attrs, empty),
        };
        // 名前空間の接頭辞(svg: など)を外す。svg 以外の名前空間の要素は描かない
        let (prefix, local) = match name.split_once(':') {
            Some((p, l)) => (Some(p), l),
            None => (None, name),
        };
        let attr = |k: &str| -> Option<String> {
            attrs
                .iter()
                .find(|(n, _)| *n == k || n.split_once(':').is_some_and(|(_, l)| l == k))
                .map(|(_, v)| v.clone())
        };
        let parent = stack.last();
        let Some(base) = (match (parent, root) {
            (Some(f), _) => Some(f.m),
            (None, Some(r)) => Some(r),
            (None, None) if local == "svg" => {
                let r = root_transform(&attr);
                root = Some(r);
                Some(r)
            }
            (None, None) => None,
        }) else {
            // 根の svg より前の要素は無視する
            continue;
        };
        let mut hidden = parent.is_some_and(|f| f.hidden)
            || prefix.is_some_and(|p| p != "svg")
            || HIDDEN.contains(&local)
            || attr("display").is_some_and(|d| d.trim() == "none");
        let mut m = base;
        if let Some(t) = attr("transform") {
            m = m.then_after(&parse_transform(&t));
        }
        // 入れ子の svg は x・y だけずらす
        if local == "svg" && parent.is_some() {
            m = m.then_after(&Affine::translate(
                length_user(attr("x").as_deref()),
                length_user(attr("y").as_deref()),
            ));
        }
        if !hidden {
            if UNSUPPORTED.contains(&local) {
                *skipped.entry(local.to_string()).or_default() += 1;
                hidden = true;
            } else {
                let num = |k: &str| length_user(attr(k).as_deref());
                let local_shapes = element_shapes(local, &attr, &num);
                shapes.extend(
                    local_shapes
                        .iter()
                        .map(|s| transform_shape(s, &m))
                        .filter(Shape::is_finite),
                );
            }
        }
        if !empty {
            stack.push(Frame { name, m, hidden });
        }
    }
    if root.is_none() {
        return Err("SVG ではありません(svg 要素がない)".into());
    }
    Ok(Imported::new(shapes, skipped))
}

/// 要素 1 つの形(利用者単位)
fn element_shapes(
    local: &str,
    attr: &dyn Fn(&str) -> Option<String>,
    num: &dyn Fn(&str) -> f64,
) -> Vec<Shape> {
    let mut out = Vec::new();
    match local {
        "path" => {
            if let Some(d) = attr("d") {
                out = parse_path(&d);
            }
        }
        "line" => {
            let (a, b) = ([num("x1"), num("y1")], [num("x2"), num("y2")]);
            if a != b {
                out.push(Shape::Line { a, b });
            }
        }
        "polyline" | "polygon" => {
            let v = numbers(&attr("points").unwrap_or_default());
            let pts: Vec<P2> = v.as_chunks::<2>().0.to_vec();
            let n = pts.len();
            let count = if local == "polygon" && n > 2 {
                n
            } else {
                n.saturating_sub(1)
            };
            for k in 0..count {
                let (a, b) = (pts[k], pts[(k + 1) % n]);
                if a != b {
                    out.push(Shape::Line { a, b });
                }
            }
        }
        "circle" => {
            let r = num("r");
            if r > 0.0 {
                out.push(Shape::Circle {
                    center: [num("cx"), num("cy")],
                    radius: r,
                });
            }
        }
        "ellipse" => {
            let (rx, ry) = (num("rx"), num("ry"));
            if rx > 0.0 && ry > 0.0 {
                out.push(ellipse_from_conjugate(
                    [num("cx"), num("cy")],
                    [rx, 0.0],
                    [0.0, ry],
                    0.0,
                    TAU,
                ));
            }
        }
        "rect" => {
            let (x, y, w, h) = (num("x"), num("y"), num("width"), num("height"));
            if !(w > 0.0 && h > 0.0) {
                return out;
            }
            let (ax, ay) = (attr("rx"), attr("ry"));
            let (mut rx, mut ry) = (num("rx"), num("ry"));
            if ax.is_none() || rx < 0.0 {
                rx = if ry > 0.0 { ry } else { 0.0 };
            }
            if ay.is_none() || ry < 0.0 {
                ry = rx;
            }
            let (rx, ry) = (rx.min(w / 2.0), ry.min(h / 2.0));
            let p = |px: f64, py: f64| [px, py];
            let mut line = |a: P2, b: P2| {
                if a != b {
                    out.push(Shape::Line { a, b });
                }
            };
            line(p(x + rx, y), p(x + w - rx, y));
            line(p(x + w, y + ry), p(x + w, y + h - ry));
            line(p(x + w - rx, y + h), p(x + rx, y + h));
            line(p(x, y + h - ry), p(x, y + ry));
            if rx > 0.0 && ry > 0.0 {
                // 角の 4 分の 1 の楕円(右上・右下・左下・左上)
                for (cx, cy, t0) in [
                    (x + w - rx, y + ry, -FRAC_PI_2),
                    (x + w - rx, y + h - ry, 0.0),
                    (x + rx, y + h - ry, FRAC_PI_2),
                    (x + rx, y + ry, PI),
                ] {
                    out.push(ellipse_from_conjugate(
                        [cx, cy],
                        [rx, 0.0],
                        [0.0, ry],
                        t0,
                        t0 + FRAC_PI_2,
                    ));
                }
            }
        }
        _ => {}
    }
    out
}
