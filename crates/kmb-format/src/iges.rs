//! IGES の固定長レコードと大域の値。形の解釈はカーネルが行う。
use std::collections::{BTreeMap, BTreeSet};

const MAX_BYTES: usize = 32 * 1024 * 1024;
const MAX_ENTRIES: usize = 100_000;

#[derive(Debug, Clone)]
pub struct Global {
    pub separator: u8,
    pub terminator: u8,
    /// モデルの数を現物の mm に直す倍率。モデル空間の尺度も含む。
    pub mm: f64,
    pub tolerance_mm: f64,
    pub producer: String,
}
#[derive(Debug, Clone)]
pub struct Entry {
    /// ディレクトリの最初のレコードの番号。
    pub id: u32,
    pub kind: u32,
    pub form: i32,
    pub transform: u32,
    pub physically_dependent: bool,
    /// 先頭の種類の番号も含む。空の文字列は省略した値。
    pub params: Vec<String>,
}
#[derive(Debug)]
pub struct File {
    pub global: Global,
    pub entries: BTreeMap<u32, Entry>,
}
fn unsigned(s: &str, what: &str) -> Result<u32, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(0);
    }
    s.parse()
        .map_err(|_| format!("{what} の整数「{s}」を読めません"))
}
fn real(s: &str, what: &str) -> Result<f64, String> {
    let value: f64 = s
        .trim()
        .replace(['d', 'D'], "E")
        .parse()
        .map_err(|_| format!("{what} の数「{s}」を読めません"))?;
    if !value.is_finite() {
        return Err(format!("{what} は有限の数にしてください"));
    }
    Ok(value)
}
fn field(s: &str, slot: usize) -> Result<u32, String> {
    unsigned(
        s.get(slot * 8..slot * 8 + 8)
            .ok_or("ディレクトリの欄が途中で終わっています")?,
        "ディレクトリ",
    )
}

fn form_number(s: &str) -> Result<i32, String> {
    let value = s.get(32..40).ok_or("形式の欄がありません")?.trim();
    if value.is_empty() {
        Ok(0)
    } else {
        value.parse().map_err(|_| "形式の整数を読めません".into())
    }
}

fn literal(bytes: &[u8], at: &mut usize) -> Result<Option<String>, String> {
    let start = *at;
    let mut end = start;
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    if end == start || !matches!(bytes.get(end), Some(b'H' | b'h')) {
        return Ok(None);
    }
    let count: usize = std::str::from_utf8(&bytes[start..end])
        .map_err(|_| "文字数を読めません")?
        .parse()
        .map_err(|_| "文字数が大きすぎます")?;
    let first = end + 1;
    let last = first.checked_add(count).ok_or("文字数が大きすぎます")?;
    let text = bytes
        .get(first..last)
        .ok_or("文字列が宣言した文字数より短くなっています")?;
    let text = std::str::from_utf8(text)
        .map_err(|_| "文字列のバイト数と文字コードが合いません。IGES を保存し直してください")?
        .to_owned();
    *at = last;
    Ok(Some(text))
}
fn whitespace(bytes: &[u8], at: &mut usize) {
    while bytes.get(*at).is_some_and(u8::is_ascii_whitespace) {
        *at += 1;
    }
}
fn delimiters(text: &str) -> Result<(u8, u8), String> {
    let bytes = text.as_bytes();
    let mut at = 0;
    whitespace(bytes, &mut at);
    let sep = match literal(bytes, &mut at)? {
        Some(s) if s.len() == 1 => s.as_bytes()[0],
        Some(_) => return Err("区切りの文字は1文字で指定してください".into()),
        None => b',',
    };
    if bytes.get(at) != Some(&sep) {
        return Err("大域の節の区切りを読めません".into());
    }
    at += 1;
    whitespace(bytes, &mut at);
    let term = match literal(bytes, &mut at)? {
        Some(s) if s.len() == 1 => s.as_bytes()[0],
        Some(_) => return Err("終わりの文字は1文字で指定してください".into()),
        None => b';',
    };
    if sep == term
        || !sep.is_ascii_graphic()
        || !term.is_ascii_graphic()
        || sep.is_ascii_alphanumeric()
        || term.is_ascii_alphanumeric()
    {
        return Err("区切りと終わりの文字が不正です".into());
    }
    Ok((sep, term))
}
/// 区切りを含む文字列を、その文字数を守って読む。
pub fn parameters(text: &str, separator: u8, terminator: u8) -> Result<Vec<String>, String> {
    let bytes = text.as_bytes();
    let mut at = 0;
    let mut values = Vec::new();
    loop {
        whitespace(bytes, &mut at);
        if at >= bytes.len() {
            return Err("パラメータの終わりの文字がありません".into());
        }
        let value = if let Some(s) = literal(bytes, &mut at)? {
            s
        } else {
            let start = at;
            while bytes
                .get(at)
                .is_some_and(|c| *c != separator && *c != terminator)
            {
                at += 1;
            }
            std::str::from_utf8(&bytes[start..at])
                .map_err(|_| "パラメータの文字コードを読めません")?
                .trim()
                .to_owned()
        };
        values.push(value);
        if values.len() > 1_000_000 {
            return Err("パラメータが多すぎます。ファイルを分けてください".into());
        }
        whitespace(bytes, &mut at);
        match bytes.get(at) {
            Some(c) if *c == separator => at += 1,
            Some(c) if *c == terminator => {
                if bytes[at + 1..].iter().any(|c| !c.is_ascii_whitespace()) {
                    return Err("パラメータの終わりの後にも値があります".into());
                }
                return Ok(values);
            }
            _ => return Err("パラメータの区切りがありません".into()),
        }
    }
}
fn global(text: &str) -> Result<Global, String> {
    let (separator, terminator) = delimiters(text)?;
    let p = parameters(text, separator, terminator)?;
    let get = |i: usize| p.get(i).map(String::as_str).unwrap_or("");
    let number = |i: usize, default: f64| {
        if get(i).is_empty() {
            Ok(default)
        } else {
            real(get(i), "大域の節")
        }
    };
    let scale = number(12, 1.)?;
    if scale <= 0. {
        return Err("モデル空間の尺度は0より大きくしてください".into());
    }
    let flag = if get(13).is_empty() {
        1
    } else {
        unsigned(get(13), "単位")?
    };
    let unit = match flag {
        1 => 25.4,
        2 => 1.,
        4 => 304.8,
        5 => 1_609_344.,
        6 => 1000.,
        7 => 1_000_000.,
        8 => 0.0254,
        9 => 0.001,
        10 => 10.,
        11 => 0.000_025_4,
        3 => match get(14).to_ascii_uppercase().as_str() {
            "MM" | "MILLIMETER" | "MILLIMETERS" => 1.,
            "IN" | "INCH" | "INCHES" => 25.4,
            "M" | "METER" | "METERS" => 1000.,
            "CM" => 10.,
            "UM" => 0.001,
            "FT" | "FEET" => 304.8,
            _ => {
                return Err(format!(
                    "単位「{}」を換算できません。mm で保存し直してください",
                    get(14)
                ));
            }
        },
        _ => {
            return Err(format!(
                "単位の番号 {flag} を換算できません。mm で保存し直してください"
            ));
        }
    };
    let mm = unit / scale;
    let tolerance_mm = number(18, 1e-6)? * mm;
    if !mm.is_finite() || mm <= 0. || !tolerance_mm.is_finite() || tolerance_mm <= 0. {
        return Err("単位・尺度・精度の値が不正です".into());
    }
    Ok(Global {
        separator,
        terminator,
        mm,
        tolerance_mm,
        producer: [get(2), get(4), get(5)].join(" "),
    })
}

/// 80桁のレコードを検査し、番号で参照できるディレクトリを返す。
pub fn read(text: &str) -> Result<File, String> {
    let text = text.trim_start_matches('\u{feff}');
    if text.len() > MAX_BYTES {
        return Err("IGES は32 MiBまでです。ファイルを分けてください".into());
    }
    let mut sections: BTreeMap<u8, Vec<(u32, &str)>> = BTreeMap::new();
    let mut previous = 0;
    for (line, record) in text.lines().enumerate() {
        if record.trim().is_empty() {
            continue;
        }
        if record.len() != 80 {
            return Err(format!(
                "{}行目が80桁ではありません(文字コードか改行を確かめてください)",
                line + 1
            ));
        }
        let bytes = record.as_bytes();
        let kind = bytes[72];
        let order = match kind {
            b'S' => 1,
            b'G' => 2,
            b'D' => 3,
            b'P' => 4,
            b'T' => 5,
            _ => return Err(format!("{}行目の節の印を読めません", line + 1)),
        };
        if order < previous {
            return Err(format!("{}行目で節の順が逆になっています", line + 1));
        }
        previous = order;
        let seq = unsigned(
            record.get(73..80).ok_or("レコード番号の位置が不正です")?,
            "レコード番号",
        )?;
        if seq == 0 {
            return Err(format!("{}行目のレコード番号が0です", line + 1));
        }
        let rows = sections.entry(kind).or_default();
        if rows.last().is_some_and(|(n, _)| *n >= seq) {
            return Err(format!(
                "{}行目のレコード番号が重なっているか逆順です",
                line + 1
            ));
        }
        rows.push((seq, record.get(..72).ok_or("レコードの文字の境が不正です")?));
    }
    for key in [b'S', b'G', b'D', b'P', b'T'] {
        if sections.get(&key).is_none_or(Vec::is_empty) {
            return Err(format!("IGES に {} の節がありません", char::from(key)));
        }
    }
    if sections[&b'T'].len() != 1 {
        return Err("終わりの節は1レコードにしてください".into());
    }
    let term = sections[&b'T'][0].1;
    for (i, key) in [b'S', b'G', b'D', b'P'].iter().enumerate() {
        if term.as_bytes().get(i * 8) != Some(key)
            || unsigned(
                term.get(i * 8 + 1..i * 8 + 8)
                    .ok_or("終わりの節が短すぎます")?,
                "節のレコード数",
            )? as usize
                != sections[key].len()
        {
            return Err(format!(
                "終わりの節と {} のレコード数が合いません",
                char::from(*key)
            ));
        }
    }
    let global = global(&sections[&b'G'].iter().map(|(_, s)| *s).collect::<String>())?;
    let d = &sections[&b'D'];
    if d.len() % 2 != 0 || d.len() / 2 > MAX_ENTRIES {
        return Err("ディレクトリの2行組が不正か、要素が10万件を超えています".into());
    }
    let mut data = BTreeMap::new();
    for (seq, row) in &sections[&b'P'] {
        let owner = unsigned(
            row.get(64..72)
                .ok_or("パラメータの参照番号の位置が不正です")?,
            "パラメータの参照番号",
        )?;
        data.insert(
            *seq,
            (
                owner,
                row.get(..64).ok_or("パラメータの文字の境が不正です")?,
            ),
        );
    }
    let mut entries = BTreeMap::new();
    let mut consumed = BTreeSet::new();
    for rows in d.as_chunks::<2>().0 {
        let (id, first) = rows[0];
        let (second_id, second) = rows[1];
        let kind = field(first, 0)?;
        if id % 2 != 1 || second_id != id + 1 || kind == 0 || field(second, 0)? != kind {
            return Err(format!("ディレクトリ {id} の2行組が合いません"));
        }
        let pointer = field(first, 1)?;
        let count = field(second, 3)?;
        if pointer == 0 || count == 0 || count as usize > data.len() {
            return Err(format!("要素 {id} のパラメータの範囲が不正です"));
        }
        let mut text = String::new();
        for n in pointer
            ..pointer
                .checked_add(count)
                .ok_or("パラメータの番号が大きすぎます")?
        {
            let (owner, row) = data
                .get(&n)
                .ok_or_else(|| format!("要素 {id} が読むパラメータ {n} がありません"))?;
            if *owner != id {
                return Err(format!("パラメータ {n} の参照先が要素 {id} と違います"));
            }
            if !consumed.insert(n) {
                return Err(format!("パラメータ {n} を重ねて読もうとしています"));
            }
            text.push_str(row);
        }
        let params = parameters(&text, global.separator, global.terminator)
            .map_err(|e| format!("要素 {id}: {e}"))?;
        if params
            .first()
            .is_none_or(|p| unsigned(p, "パラメータの種類").ok() != Some(kind))
        {
            return Err(format!("要素 {id} の種類がパラメータと違います"));
        }
        let status = first.get(64..72).ok_or("状態の欄がありません")?;
        entries.insert(
            id,
            Entry {
                id,
                kind,
                form: form_number(second)?,
                transform: field(first, 6)?,
                physically_dependent: matches!(
                    unsigned(status.get(2..4).ok_or("状態の欄が不正です")?, "従属の印")?,
                    1 | 3
                ),
                params,
            },
        );
    }
    if consumed.len() != data.len() {
        return Err("ディレクトリの範囲に入らないパラメータがあります".into());
    }
    if data.values().any(|(owner, _)| !entries.contains_key(owner)) {
        return Err("ディレクトリの無いパラメータがあります".into());
    }
    Ok(File { global, entries })
}

/// 型の定める順に、上限を持って数と参照を読む。
pub struct Params<'a> {
    entry: &'a Entry,
    at: usize,
}
impl<'a> Params<'a> {
    pub fn new(entry: &'a Entry) -> Self {
        Self { entry, at: 1 }
    }
    fn take(&mut self) -> Result<&'a str, String> {
        let value = self.entry.params.get(self.at).ok_or_else(|| {
            format!(
                "要素 {} のパラメータ {} がありません",
                self.entry.id, self.at
            )
        })?;
        self.at += 1;
        Ok(value)
    }
    pub fn number(&mut self) -> Result<f64, String> {
        let id = self.entry.id;
        real(self.take()?, &format!("要素 {id}"))
    }
    pub fn integer(&mut self) -> Result<u32, String> {
        let id = self.entry.id;
        unsigned(self.take()?, &format!("要素 {id}"))
    }
    pub fn count(&mut self, limit: usize) -> Result<usize, String> {
        let n = self.integer()? as usize;
        if n > limit {
            Err(format!(
                "要素 {} の個数 {n} が上限 {limit} を超えています",
                self.entry.id
            ))
        } else {
            Ok(n)
        }
    }
    pub fn flag(&mut self) -> Result<bool, String> {
        match self.integer()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(format!(
                "要素 {} の印は0か1で指定してください",
                self.entry.id
            )),
        }
    }
    pub fn point(&mut self) -> Result<[f64; 3], String> {
        Ok([self.number()?, self.number()?, self.number()?])
    }
    pub fn remaining(&self) -> usize {
        self.entry.params.len().saturating_sub(self.at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn records(section: char, text: &str) -> Vec<String> {
        text.as_bytes()
            .chunks(72)
            .enumerate()
            .map(|(i, p)| {
                format!(
                    "{:<72}{section}{:7}",
                    std::str::from_utf8(p).unwrap(),
                    i + 1
                )
            })
            .collect()
    }
    fn file(scale: &str, units: &str, name: &str) -> String {
        let mut g = vec!["".to_string(); 20];
        g[0] = "1H,".into();
        g[1] = "1H;".into();
        g[2] = "4Htest".into();
        g[12] = scale.into();
        g[13] = units.into();
        g[14] = format!("{}H{name}", name.len());
        g[18] = "1D-6".into();
        let start = records('S', "test");
        let global = records('G', &(g.join(",") + ";"));
        let directory = [
            format!(
                "{:8}{:8}{:8}{:8}{:8}{:8}{:8}{:8}00000000D{:7}",
                110, 1, 0, 0, 0, 0, 0, 0, 1
            ),
            format!(
                "{:8}{:8}{:8}{:8}{:8}{:8}{:8}{:8}{:8}D{:7}",
                110, 0, 0, 1, 0, 0, 0, 0, 0, 2
            ),
        ];
        let parameter = format!("{:<64}{:8}P{:7}", "110,0,0,0,1D0,0,0;", 1, 1);
        let end = format!(
            "{:<72}T{:7}",
            format!(
                "S{:7}G{:7}D{:7}P{:7}",
                start.len(),
                global.len(),
                directory.len(),
                1
            ),
            1
        );
        start
            .into_iter()
            .chain(global)
            .chain(directory)
            .chain([parameter, end])
            .collect::<Vec<_>>()
            .join("\n")
    }
    #[test]
    fn fixed_records_references_units_and_model_scale_are_checked() {
        for (scale, units, name, mm) in [
            ("1", "2", "MM", 1.),
            ("1", "1", "IN", 25.4),
            ("0.125", "2", "MM", 8.),
            ("2", "6", "M", 500.),
            ("1", "3", "UM", 0.001),
        ] {
            let doc = read(&file(scale, units, name)).unwrap();
            assert!((doc.global.mm - mm).abs() < 1e-12);
            assert_eq!(doc.entries[&1].params[4], "1D0");
            assert_eq!(doc.entries[&1].kind, 110);
        }
        assert!(read(&file("0", "2", "MM")).is_err());
        assert!(read(&file("1", "3", "UNKNOWN")).is_err());
        let valid = file("1", "2", "MM");
        for corrupt in [
            valid.replace("D      2", "D      4"),
            valid.replace("P      1", "P      2"),
            valid.replace("110,0,0,0", "128,0,0,0"),
            valid.lines().take(2).collect::<Vec<_>>().join("\n"),
        ] {
            assert!(read(&corrupt).is_err());
        }
    }
    #[test]
    fn delimiters_and_hollerith_strings_do_not_split_inside_text() {
        let input = "1H^^1H!^7Hone^two^2D-3!";
        let (sep, end) = delimiters(input).unwrap();
        assert_eq!((sep, end), (b'^', b'!'));
        assert_eq!(
            parameters(input, sep, end).unwrap(),
            ["^", "!", "one^two", "2D-3"]
        );
        assert!(parameters("9Habc;", b',', b';').is_err());
        assert!(parameters("1;2", b',', b';').is_err());
        assert_eq!(real("2D-3", "test").unwrap(), 0.002);
        assert!(real("NaN", "test").is_err());
    }
    #[test]
    fn truncated_or_non_record_inputs_return_reasons() {
        for text in ["", "garbage", "S0000001", "1H,,1H;;"] {
            assert!(read(text).is_err());
        }
        let doc = read(&file("1", "2", "MM")).unwrap();
        let mut p = Params::new(&doc.entries[&1]);
        assert_eq!(p.point().unwrap(), [0.; 3]);
        assert_eq!(p.point().unwrap(), [1., 0., 0.]);
        assert!(p.number().is_err());
    }
}
