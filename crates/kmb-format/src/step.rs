//! STEP(ISO 10303-21)の長さの単位を読む。形の読み手(カーネル)は座標と半径にこの倍率を掛けて
//! mm にそろえ、読み込みの機能は読めなかった時の注意を出す。どちらも同じ読み方を使うよう、ここに置く。
//! 実体を 1 つずつに分ける読み方([`instances_at`])と、立体と一緒に置かれた面だけの殻の数
//! ([`open_shells_beside_solids`])も、両方の読み手と読み込みの機能が使うのでここに置く。
//!
//! 単位は、形の文脈(`GLOBAL_UNIT_ASSIGNED_CONTEXT`)が並べる単位のうち長さの物を見る。
//! - `SI_UNIT` は接頭辞と `.METRE.` から(`.MILLI.` なら 1 mm、接頭辞が無ければ 1000 mm)。
//! - `CONVERSION_BASED_UNIT`(inch・foot など)は名前を当てにせず、添えられた量
//!   (`LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#mm)` など)の数と元の単位から求める。
//!
//! 長さの単位が見つからない・量を読めない・文脈ごとに違う時は、注意の文を返す。倍率は、見つからない
//! 時と読めない時は 1(ファイルの数を mm とみなす)、違う時は番号の最も小さい文脈の物にする。

use std::collections::HashMap;

/// 読んだ長さの単位。
#[derive(Debug, Clone, PartialEq)]
pub struct LengthUnit {
    /// ファイルの長さ 1 が何 mm か
    pub mm: f64,
    /// 単位の名前(`mm`・`m`・`inch` など。表示と記録に使う)
    pub name: String,
    /// 読めなかった・混ざっていた時の注意(使う人に見せる文)
    pub warning: Option<String>,
}

/// STEP の字から長さの単位を読む。
pub fn length_unit(text: &str) -> LengthUnit {
    let recs = unit_records(text);
    let mut contexts: Vec<(u64, u64)> = recs
        .iter()
        .flat_map(|(id, inst)| {
            inst.iter()
                .filter(|r| r.name == "GLOBAL_UNIT_ASSIGNED_CONTEXT")
                .filter_map(|r| match r.params.first() {
                    Some(P::List(v)) => Some(v.as_slice()),
                    _ => None,
                })
                .flatten()
                .filter_map(|p| match p {
                    P::Ref(u) => Some((*id, *u)),
                    _ => None,
                })
        })
        .filter(|(_, u)| recs.get(u).is_some_and(|i| is_length(i)))
        .collect();
    contexts.sort();
    let Some(&(_, first)) = contexts.first() else {
        return assumed_mm(
            "STEP に長さの単位が書かれていないので、ファイルの数を mm とみなしました",
        );
    };
    let read = |u: u64| resolve(&recs, u, 0);
    let unit = match read(first) {
        Ok(v) => v,
        Err(why) => {
            return assumed_mm(&format!(
                "STEP の長さの単位を読めないので、ファイルの数を mm とみなしました({why})"
            ));
        }
    };
    let mixed = contexts
        .iter()
        .any(|&(_, u)| read(u).is_ok_and(|(mm, _)| ((mm - unit.0) / unit.0).abs() > 1e-9));
    LengthUnit {
        mm: unit.0,
        warning: mixed.then(|| {
            format!(
                "STEP の中で長さの単位が混ざっています。全体を {} として読みました",
                unit.1
            )
        }),
        name: unit.1,
    }
}

/// データの節の実体を 1 つずつ、字の中の位置(バイト)と字(終わりの `;` を除く)で返す。
/// 文字列と注釈の中の `;` では切らない。見出しの節の記録は `#` で始まらないので入らない。
pub fn instances_at(text: &str) -> Vec<(usize, &str)> {
    let base = text.as_ptr() as usize;
    instances(text.as_bytes())
        .into_iter()
        // 切れ目は `;` と ASCII の空白・注釈の後なので、字の途中では切れない
        .filter_map(|c| Some((c.as_ptr() as usize - base, std::str::from_utf8(c).ok()?)))
        .collect()
}

/// 字の位置(バイト)が何行目か(1 から数える)。
pub fn line_at(text: &str, at: usize) -> usize {
    text.as_bytes()[..at.min(text.len())]
        .iter()
        .filter(|&&c| c == b'\n')
        .count()
        + 1
}

/// 実体 `#id = 名前(…)` の名前(複合の実体は空)。
pub fn keyword(instance: &str) -> &str {
    let rest = instance.split_once('=').map_or("", |(_, r)| r.trim_start());
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    &rest[..end]
}

/// 閉じた殻(`CLOSED_SHELL`、立体の外側と空洞)と一緒に置かれた、閉じていない殻(`OPEN_SHELL`、
/// 面だけのボディ)の数。公差の補助の面などがこの形で入る。立体を読む時、これらは立体に入れない。
/// 閉じた殻が無い(面だけのファイル)時は 0。
pub fn open_shells_beside_solids(text: &str) -> usize {
    let (mut open, mut closed) = (0, 0);
    for (_, i) in instances_at(text) {
        match keyword(i) {
            "OPEN_SHELL" => open += 1,
            "CLOSED_SHELL" => closed += 1,
            _ => {}
        }
    }
    if closed > 0 { open } else { 0 }
}

fn assumed_mm(warning: &str) -> LengthUnit {
    LengthUnit {
        mm: 1.0,
        name: "mm".into(),
        warning: Some(warning.into()),
    }
}

/// 単位の実体 1 つが長さの単位か。
fn is_length(inst: &[Rec]) -> bool {
    inst.iter().any(|r| r.name == "LENGTH_UNIT")
        || find(inst, "SI_UNIT")
            .is_some_and(|r| matches!(r.params.last(), Some(P::Enum(e)) if e == "METRE"))
}

fn find<'a>(inst: &'a [Rec], name: &str) -> Option<&'a Rec> {
    inst.iter().find(|r| r.name == name)
}

/// 単位 `id` の 1 が何 mm かと、その名前。換算の単位は元の単位をたどる(輪にならないよう深さを限る)。
fn resolve(recs: &HashMap<u64, Vec<Rec>>, id: u64, depth: u32) -> Result<(f64, String), String> {
    if depth > 8 {
        return Err("換算の単位のたどりが深すぎます".into());
    }
    let inst = recs
        .get(&id)
        .ok_or_else(|| format!("単位 #{id} がありません"))?;
    if let Some(r) = find(inst, "SI_UNIT") {
        // 単独の形は (次元, 接頭辞, 名前)、複合の形は (接頭辞, 名前)。後ろの 2 つを見る
        let n = r.params.len();
        let prefix = match (n.checked_sub(2).map(|i| &r.params[i]), r.params.last()) {
            (Some(p), Some(P::Enum(m))) if m == "METRE" => p,
            _ => return Err(format!("単位 #{id} は長さの SI 単位ではありません")),
        };
        let (scale, short) = match prefix {
            P::Enum(e) => si_prefix(e).ok_or_else(|| format!("接頭辞 .{e}. を知りません"))?,
            _ => (1.0, ""),
        };
        return Ok((scale * 1000.0, format!("{short}m")));
    }
    if let Some(r) = find(inst, "CONVERSION_BASED_UNIT") {
        // 単独の形は (次元, 名前, 量)、複合の形は (名前, 量)。後ろの 2 つを見る
        let n = r.params.len();
        let (Some(P::Str(name)), Some(P::Ref(m))) =
            (n.checked_sub(2).map(|i| &r.params[i]), r.params.last())
        else {
            return Err(format!("換算の単位 #{id} の書き方を読めません"));
        };
        let measure = recs
            .get(m)
            .ok_or_else(|| format!("換算の量 #{m} がありません"))?;
        let w = measure
            .iter()
            .find(|r| r.name.ends_with("MEASURE_WITH_UNIT"))
            .ok_or_else(|| format!("換算の量 #{m} の書き方を読めません"))?;
        let (Some(value), Some(P::Ref(base))) =
            (w.params.first().and_then(number), w.params.get(1))
        else {
            return Err(format!("換算の量 #{m} の書き方を読めません"));
        };
        let (base_mm, _) = resolve(recs, *base, depth + 1)?;
        let mm = value * base_mm;
        if !(mm.is_finite() && mm > 0.0) {
            return Err(format!("換算の量 #{m} が正の数ではありません"));
        }
        return Ok((mm, name.to_lowercase()));
    }
    Err(format!("単位 #{id} は SI 単位でも換算の単位でもありません"))
}

/// SI の接頭辞の倍率と記号。
fn si_prefix(e: &str) -> Option<(f64, &'static str)> {
    Some(match e {
        "EXA" => (1e18, "E"),
        "PETA" => (1e15, "P"),
        "TERA" => (1e12, "T"),
        "GIGA" => (1e9, "G"),
        "MEGA" => (1e6, "M"),
        "KILO" => (1e3, "k"),
        "HECTO" => (1e2, "h"),
        "DECA" => (1e1, "da"),
        "DECI" => (1e-1, "d"),
        "CENTI" => (1e-2, "c"),
        "MILLI" => (1e-3, "m"),
        "MICRO" => (1e-6, "µ"),
        "NANO" => (1e-9, "n"),
        "PICO" => (1e-12, "p"),
        "FEMTO" => (1e-15, "f"),
        "ATTO" => (1e-18, "a"),
        _ => return None,
    })
}

fn number(p: &P) -> Option<f64> {
    match p {
        P::Num(v) => Some(*v),
        P::Typed(v) => number(v),
        _ => None,
    }
}

// ---- Part 21 の字句(単位に関わる実体だけを読む) ----

#[derive(Debug, Clone)]
enum P {
    Ref(u64),
    Num(f64),
    Str(String),
    Enum(String),
    List(Vec<P>),
    /// 型つきの値(`LENGTH_MEASURE(25.4)`。型の名前は見ない)
    Typed(Box<P>),
    Other,
}

#[derive(Debug, Clone)]
struct Rec {
    name: String,
    params: Vec<P>,
}

/// 字を実体ごとに分け、名前に UNIT か MEASURE を含む実体だけを読む(形の実体は読まないので、
/// 大きなファイルでも速い)。読めない実体は飛ばす。
fn unit_records(text: &str) -> HashMap<u64, Vec<Rec>> {
    let mut out = HashMap::new();
    for chunk in instances(text.as_bytes()) {
        if !(contains(chunk, b"UNIT") || contains(chunk, b"MEASURE")) {
            continue;
        }
        let mut lx = Lexer { s: chunk, i: 0 };
        if let Some((id, recs)) = lx.instance() {
            out.insert(id, recs);
        }
    }
    out
}

fn contains(h: &[u8], n: &[u8]) -> bool {
    h.windows(n.len()).any(|w| w == n)
}

/// `;` で区切った実体のうち `#` で始まる物(見出しの節は `#` で始まらない)。文字列と注釈の中の
/// `;` では切らない。
fn instances(s: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let (mut start, mut i) = (0, 0);
    while i < s.len() {
        match s[i] {
            b'\'' => {
                i += 1;
                while i < s.len() {
                    if s[i] == b'\'' {
                        if s.get(i + 1) == Some(&b'\'') {
                            i += 1;
                        } else {
                            break;
                        }
                    }
                    i += 1;
                }
            }
            b'/' if s.get(i + 1) == Some(&b'*') => {
                i = s[i + 2..]
                    .windows(2)
                    .position(|w| w == b"*/")
                    .map_or(s.len(), |k| i + 2 + k + 1);
            }
            b';' => {
                let c = skip_comments(&s[start..i]);
                if c.first() == Some(&b'#') {
                    out.push(c);
                }
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// 頭の空白と注釈を外す。
fn skip_comments(mut c: &[u8]) -> &[u8] {
    loop {
        c = c.trim_ascii_start();
        match c.strip_prefix(b"/*") {
            Some(rest) => {
                c = rest
                    .windows(2)
                    .position(|w| w == b"*/")
                    .map_or(&[][..], |k| &rest[k + 2..]);
            }
            None => return c.trim_ascii_end(),
        }
    }
}

struct Lexer<'a> {
    s: &'a [u8],
    i: usize,
}

impl Lexer<'_> {
    fn peek(&mut self) -> Option<u8> {
        loop {
            while self.s.get(self.i).is_some_and(u8::is_ascii_whitespace) {
                self.i += 1;
            }
            if self.s[self.i..].starts_with(b"/*") {
                self.i = self.s[self.i + 2..]
                    .windows(2)
                    .position(|w| w == b"*/")
                    .map_or(self.s.len(), |k| self.i + 4 + k);
            } else {
                return self.s.get(self.i).copied();
            }
        }
    }

    fn eat(&mut self, c: u8) -> Option<()> {
        (self.peek()? == c).then(|| self.i += 1)
    }

    fn take_while(&mut self, f: impl Fn(u8) -> bool) -> &str {
        let a = self.i;
        while self.s.get(self.i).is_some_and(|&c| f(c)) {
            self.i += 1;
        }
        std::str::from_utf8(&self.s[a..self.i]).unwrap_or("")
    }

    /// `#id = 記録` か `#id = ( 記録 記録 … )`。
    fn instance(&mut self) -> Option<(u64, Vec<Rec>)> {
        self.eat(b'#')?;
        let id = self.take_while(|c| c.is_ascii_digit()).parse().ok()?;
        self.eat(b'=')?;
        if self.peek()? == b'(' {
            self.i += 1;
            let mut recs = Vec::new();
            while self.peek()? != b')' {
                recs.push(self.record()?);
            }
            Some((id, recs))
        } else {
            Some((id, vec![self.record()?]))
        }
    }

    fn record(&mut self) -> Option<Rec> {
        self.peek()?;
        let name = self
            .take_while(|c| c.is_ascii_alphanumeric() || c == b'_')
            .to_ascii_uppercase();
        if name.is_empty() {
            return None;
        }
        Some(Rec {
            name,
            params: self.params()?,
        })
    }

    fn params(&mut self) -> Option<Vec<P>> {
        self.eat(b'(')?;
        let mut out = Vec::new();
        if self.eat(b')').is_some() {
            return Some(out);
        }
        loop {
            out.push(self.param()?);
            match self.peek()? {
                b',' => self.i += 1,
                b')' => {
                    self.i += 1;
                    return Some(out);
                }
                _ => return None,
            }
        }
    }

    fn param(&mut self) -> Option<P> {
        Some(match self.peek()? {
            b'#' => {
                self.i += 1;
                P::Ref(self.take_while(|c| c.is_ascii_digit()).parse().ok()?)
            }
            b'\'' => {
                self.i += 1;
                let mut v = Vec::new();
                loop {
                    match *self.s.get(self.i)? {
                        b'\'' if self.s.get(self.i + 1) == Some(&b'\'') => {
                            v.push(b'\'');
                            self.i += 2;
                        }
                        b'\'' => {
                            self.i += 1;
                            break;
                        }
                        c => {
                            v.push(c);
                            self.i += 1;
                        }
                    }
                }
                P::Str(String::from_utf8_lossy(&v).into_owned())
            }
            b'.' => {
                self.i += 1;
                let e = self
                    .take_while(|c| c.is_ascii_alphanumeric() || c == b'_')
                    .to_ascii_uppercase();
                self.eat(b'.')?;
                P::Enum(e)
            }
            b'$' | b'*' => {
                self.i += 1;
                P::Other
            }
            b'"' => {
                self.i += 1;
                self.take_while(|c| c != b'"');
                self.eat(b'"')?;
                P::Other
            }
            b'(' => P::List(self.params()?),
            c if c.is_ascii_digit() || c == b'-' || c == b'+' => P::Num(
                self.take_while(|c| {
                    c.is_ascii_digit() || matches!(c, b'-' | b'+' | b'.' | b'e' | b'E')
                })
                .parse()
                .ok()?,
            ),
            c if c.is_ascii_alphabetic() => {
                self.take_while(|c| c.is_ascii_alphanumeric() || c == b'_');
                let mut v = self.params()?;
                if v.len() != 1 {
                    return Some(P::Other);
                }
                P::Typed(Box::new(v.remove(0)))
            }
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 単位の実体と、それを並べる形の文脈だけの STEP。
    fn step(units: &str, used: &str) -> String {
        format!(
            "ISO-10303-21;\nHEADER;\nFILE_NAME('a;b','',(''),(''),'','','');\nENDSEC;\nDATA;\n\
             {units}\n#90=( GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT(({used},#80)) \
             REPRESENTATION_CONTEXT('Context #1','3D Context with UNIT and UNCERTAINTY') );\n\
             #80=( NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.) );\nENDSEC;\nEND-ISO-10303-21;\n"
        )
    }

    #[test]
    fn si_millimetre_and_metre_and_simple_form() {
        let mm = length_unit(&step(
            "#10=( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) );",
            "#10",
        ));
        assert_eq!((mm.mm, mm.name.as_str(), mm.warning), (1.0, "mm", None));
        let m = length_unit(&step(
            "#10=( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT($,.METRE.) );",
            "#10",
        ));
        assert_eq!((m.mm, m.name.as_str()), (1000.0, "m"));
        // 単独の形(次元つき)も読む
        let cm = length_unit(&step("#10=SI_UNIT(*,.CENTI.,.METRE.);", "#10"));
        assert_eq!((cm.mm, cm.name.as_str()), (10.0, "cm"));
    }

    #[test]
    fn inch_is_read_from_its_measure_not_its_name() {
        let units = "#10=( CONVERSION_BASED_UNIT('INCH',#11) LENGTH_UNIT() NAMED_UNIT(#12) );\n\
                     #11=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#13);\n\
                     #12=DIMENSIONAL_EXPONENTS(1.,0.,0.,0.,0.,0.,0.);\n\
                     #13=( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) );";
        let u = length_unit(&step(units, "#10"));
        assert_eq!((u.mm, u.name.as_str(), u.warning), (25.4, "inch", None));
        // 元の単位が m でも(0.0254 m)、名前を知らない単位でも量から求める
        let units = "#10=( CONVERSION_BASED_UNIT('SHAKU',#11) LENGTH_UNIT() NAMED_UNIT(*) );\n\
                     #11=LENGTH_MEASURE_WITH_UNIT(0.30303,#13);\n\
                     #13=( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT($,.METRE.) );";
        let u = length_unit(&step(units, "#10"));
        assert!((u.mm - 303.03).abs() < 1e-9 && u.name == "shaku");
    }

    #[test]
    fn missing_broken_or_mixed_units_give_a_warning() {
        // 単位が無い: mm とみなして注意
        let none =
            length_unit("ISO-10303-21;\nDATA;\n#1=CARTESIAN_POINT('',(0.,0.,0.));\nENDSEC;\n");
        assert_eq!(none.mm, 1.0);
        assert!(none.warning.is_some());
        // 換算の量が無い
        let broken = length_unit(&step(
            "#10=( CONVERSION_BASED_UNIT('INCH',#11) LENGTH_UNIT() NAMED_UNIT(*) );",
            "#10",
        ));
        assert_eq!(broken.mm, 1.0);
        assert!(broken.warning.unwrap().contains("#11"));
        // 文脈ごとに違う: 番号の小さい文脈の単位で読み、注意
        let text = step(
            "#10=( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) );\n\
             #20=( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT($,.METRE.) );\n\
             #95=( GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#20)) REPRESENTATION_CONTEXT('','') );",
            "#10",
        );
        let mixed = length_unit(&text);
        assert_eq!(mixed.mm, 1.0);
        assert!(mixed.warning.unwrap().contains("混ざって"));
    }

    #[test]
    fn semicolons_in_strings_and_comments_do_not_split_entities() {
        let units = "/* ; */#10=( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) );\n\
                     #11=PRODUCT('it''s;',';','',(#1));";
        let u = length_unit(&step(units, "#10"));
        assert_eq!((u.mm, u.warning), (1.0, None));
    }

    #[test]
    fn instances_keep_their_place_and_line() {
        let text = "ISO-10303-21;\nHEADER;\nFILE_NAME('a;b');\nENDSEC;\nDATA;\n\
                    #1=CLOSED_SHELL('',(#5));\n/* ; */ #2 = OPEN_SHELL('x;y',(#6));\n\
                    #3=(A() B());\nENDSEC;\n";
        let got = instances_at(text);
        let ids: Vec<&str> = got.iter().map(|(_, i)| &i[..2]).collect();
        assert_eq!(ids, ["#1", "#2", "#3"]);
        let lines: Vec<usize> = got.iter().map(|(at, _)| line_at(text, *at)).collect();
        assert_eq!(lines, [6, 7, 8]);
        let names: Vec<&str> = got.iter().map(|(_, i)| keyword(i)).collect();
        assert_eq!(names, ["CLOSED_SHELL", "OPEN_SHELL", ""]);
        assert_eq!(open_shells_beside_solids(text), 1);
        // 面だけのファイルは数えない(読み手は面のボディとして読む)
        assert_eq!(
            open_shells_beside_solids("DATA;#2=OPEN_SHELL('',(#6));ENDSEC;"),
            0
        );
    }
}
