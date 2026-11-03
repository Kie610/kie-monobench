//! STEP の部品の名前(PRODUCT の 1 つ目の値)を読む。読み込んだ機能の名前を、
//! ファイル名でなく書いた CAD の部品の名前にするため。形は読まないので、カーネルを通さず字で拾う。

/// STEP の文字列にある PRODUCT の名前(空の物を除き、出てきた順、重なりなし)。
pub fn product_names(step: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for record in records(step) {
        let Some(rest) = record_body(record, "PRODUCT") else {
            continue;
        };
        if let Some(name) = first_string(rest).map(|s| decode(&s))
            && !name.trim().is_empty()
            && !out.contains(&name)
        {
            out.push(name);
        }
    }
    out
}

/// 読み込んだ機能に付ける名前。部品が 1 つならその名前、ほかは `fallback`(ファイル名)。
/// 仮の名前(JC が書く製品名。前の名前も)は
/// 部品の名前として使わない
pub fn import_name(step: &str, fallback: &str) -> String {
    let names: Vec<String> = product_names(step)
        .into_iter()
        .filter(|n| !placeholder(n))
        .collect();
    match names.as_slice() {
        [one] => one.clone(),
        _ => fallback.to_string(),
    }
}

/// JC が書く STEP の作った system の名前。前の名前で書いた STEP も JC の物と見分ける。
const OUR_SYSTEMS: [&str; 2] = ["kieMonoBench", "jisaku-cadcam"];

/// 頭の作成元を、書き出しと同じ上向きの軸として見分ける。
pub(crate) fn our_producer(producer: &str) -> bool {
    producer
        .split_whitespace()
        .any(|word| OUR_SYSTEMS.contains(&word))
}

/// JC が書いた STEP か(頭の FILE_NAME の作った system が `OUR_SYSTEMS` のどれか)。JC は Z が上で書くので、
/// 読み込んだ後に立てるかを聞かない(U12)
pub fn written_by_jc(step: &str) -> bool {
    let head = step.find("DATA;").map_or(step, |i| &step[..i]);
    head.split(';').any(|r| {
        let r = r.trim_start();
        r.get(..9)
            .is_some_and(|k| k.eq_ignore_ascii_case("FILE_NAME"))
            && OUR_SYSTEMS.iter().any(|s| r.contains(&format!("'{s}'")))
    })
}

fn placeholder(name: &str) -> bool {
    let n = name.trim();
    (n.starts_with('(') && n.ends_with(')')) || OUR_SYSTEMS.contains(&n) || n == "part"
}

/// DATA 節の 1 件ずつ(`#12=PRODUCT(...)` の `=` の後ろ、`;` の手前)。字の中の `;` では切らない。
fn records(step: &str) -> impl Iterator<Item = &str> {
    let data = step.find("DATA;").map_or(step, |i| &step[i + 5..]);
    let mut parts = Vec::new();
    let (mut start, mut quoted) = (0, false);
    for (i, c) in data.char_indices() {
        match c {
            '\'' => quoted = !quoted,
            ';' if !quoted => {
                parts.push(&data[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.into_iter().filter_map(|r| {
        let r = r.trim_start();
        r.starts_with('#').then(|| r.split_once('=').map(|x| x.1))?
    })
}

/// 種類が `name` の件なら、括弧の中身。
fn record_body<'a>(record: &'a str, name: &str) -> Option<&'a str> {
    let r = record.trim_start();
    let rest = r
        .get(..name.len())?
        .eq_ignore_ascii_case(name)
        .then(|| &r[name.len()..])?;
    rest.trim_start().strip_prefix('(')
}

/// 最初の値が字(`'...'`)なら、その中身(`''` は `'` にする)。
fn first_string(params: &str) -> Option<String> {
    let s = params.trim_start().strip_prefix('\'')?;
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\'' {
            if chars.peek() == Some(&'\'') {
                chars.next();
                out.push('\'');
            } else {
                return Some(out);
            }
        } else {
            out.push(c);
        }
    }
    None
}

/// STEP の字の書き方を戻す: `\X2\30C6\X0\`(UTF-16 の 16 進)、`\X4\…\X0\`(UTF-32)、
/// `\X\E9`(ISO 8859-1 の 1 字)、`\\`。読めない書き方はそのまま残す。
fn decode(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('\\') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let wide = |tail: &str, tag: &str, width: usize| -> Option<(String, usize)> {
            let body = tail.strip_prefix(tag)?;
            let end = body.find("\\X0\\")?;
            let hex = &body[..end];
            if hex.len() % width != 0 {
                return None;
            }
            let units: Vec<u32> = (0..hex.len() / width)
                .map(|k| u32::from_str_radix(&hex[k * width..(k + 1) * width], 16).ok())
                .collect::<Option<_>>()?;
            let text = if width == 4 {
                let u16s: Vec<u16> = units.iter().map(|&u| u as u16).collect();
                String::from_utf16(&u16s).ok()?
            } else {
                units
                    .iter()
                    .map(|&u| char::from_u32(u))
                    .collect::<Option<_>>()?
            };
            Some((text, tag.len() + end + 4))
        };
        let step = wide(tail, "\\X2\\", 4)
            .or_else(|| wide(tail, "\\X4\\", 8))
            .or_else(|| {
                let hex = tail.strip_prefix("\\X\\")?.get(..2)?;
                let b = u8::from_str_radix(hex, 16).ok()?;
                Some((char::from(b).to_string(), 5))
            })
            .or_else(|| tail.starts_with("\\\\").then(|| ("\\".to_string(), 2)));
        match step {
            Some((text, used)) => {
                out.push_str(&text);
                rest = &tail[used..];
            }
            None => {
                out.push('\\');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_product_names_are_read_and_decoded() {
        let step = "ISO-10303-21;\nHEADER;\nFILE_NAME('x.step','',(''),(''),'','','');\nENDSEC;\nDATA;\n\
            #1=PRODUCT('Bracket','Bracket','',(#2));\n\
            #3 = PRODUCT ( 'It''s; a \\X2\\30D6\\X0\\\\X2\\30E9\\X0\\\\X2\\30B1\\X0\\\\X2\\30C3\\X0\\\\X2\\30C8\\X0\\' , '', '', (#2));\n\
            #4=PRODUCT_DEFINITION('design','',#5,#6);\n\
            #5=PRODUCT('Bracket','','',(#2));\n\
            #6=PRODUCT('','','',(#2));\n\
            #7=PRODUCT('caf\\X\\E9','','',(#2));\nENDSEC;\nEND-ISO-10303-21;\n";
        assert_eq!(
            product_names(step),
            vec![
                "Bracket".to_string(),
                "It's; a ブラケット".to_string(),
                "café".to_string()
            ]
        );
        // 部品が 2 つ以上・無い時はファイル名
        assert_eq!(import_name(step, "file"), "file");
        let one = "DATA;\n#1=PRODUCT('Shaft','','',(#2));\nENDSEC;";
        assert_eq!(import_name(one, "file"), "Shaft");
        assert_eq!(import_name("DATA;\nENDSEC;", "file"), "file");
        // 仮の名前はファイル名にする
        let unsaved = "DATA;\n#580=PRODUCT('(\\X2\\672A4FDD5B58\\X0\\)','',$,(#579));\nENDSEC;";
        assert_eq!(product_names(unsaved), vec!["(未保存)".to_string()]);
        assert_eq!(import_name(unsaved, "file"), "file");
        for s in OUR_SYSTEMS {
            let ours = format!("DATA;\n#4=PRODUCT('{s}','{s}','',(#3));\nENDSEC;");
            assert_eq!(import_name(&ours, "file"), "file");
        }
    }

    #[test]
    fn written_by_jc_reads_the_header() {
        // 今の名前と前の名前のどちらで書いた STEP も
        for s in OUR_SYSTEMS {
            let ours = format!(
                "ISO-10303-21;\nHEADER;\nFILE_NAME('a','2026',(''),(''),'{s}','{s} (SMLib)','');\nENDSEC;\nDATA;\nENDSEC;"
            );
            assert!(written_by_jc(&ours));
        }
        let other = "ISO-10303-21;\nHEADER;\nFILE_NAME('a','2026',(''),(''),'Other','Other CAD','');\nENDSEC;\nDATA;\n#1=PRODUCT('jisaku-cadcam','','',(#2));\nENDSEC;";
        assert!(!written_by_jc(other));
    }
}
