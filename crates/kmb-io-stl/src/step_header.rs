//! STEP の HEADER の値だけを、設計の属性で置き換える。
use kmb_core::{Problem, ProblemKind, Properties};
use std::ops::Range;

fn invalid() -> Problem {
    Problem::new(
        ProblemKind::Compute,
        "STEP の頭の情報を読めませんでした(HEADER の構文を確かめてください)",
    )
}

/// 文字列・コメントの中の区切りを、レコードの区切りとして扱わない。
struct Lexer<'a> {
    source: &'a str,
    at: usize,
}
impl<'a> Lexer<'a> {
    fn next(&mut self) -> Result<Option<Range<usize>>, Problem> {
        let b = self.source.as_bytes();
        while self.at < b.len() {
            if b[self.at].is_ascii_whitespace() {
                self.at += 1;
            } else if b[self.at..].starts_with(b"/*") {
                let rest = &self.source[self.at + 2..];
                self.at += 2 + rest.find("*/").ok_or_else(invalid)? + 2;
            } else {
                break;
            }
        }
        if self.at == b.len() {
            return Ok(None);
        }
        let start = self.at;
        if b[start] == b'\'' {
            self.at += 1;
            loop {
                match b.get(self.at) {
                    None => return Err(invalid()),
                    Some(b'\'') if b.get(self.at + 1) == Some(&b'\'') => self.at += 2,
                    Some(b'\'') => {
                        self.at += 1;
                        break;
                    }
                    Some(_) => self.at += 1,
                }
            }
        } else if b[start].is_ascii_alphanumeric() || b[start] == b'_' {
            self.at += 1;
            while b
                .get(self.at)
                .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
            {
                self.at += 1;
            }
        } else if b[start].is_ascii() {
            self.at += 1;
        } else {
            return Err(invalid());
        }
        Ok(Some(start..self.at))
    }
    fn required(&mut self) -> Result<Range<usize>, Problem> {
        self.next()?.ok_or_else(invalid)
    }
}

fn args(source: &str, tokens: &[Range<usize>]) -> Result<Vec<Range<usize>>, Problem> {
    let mut out = Vec::new();
    let (mut start, mut depth) = (0, 0usize);
    for (i, token) in tokens.iter().enumerate() {
        match &source[token.clone()] {
            "(" => depth += 1,
            ")" => depth = depth.checked_sub(1).ok_or_else(invalid)?,
            "," if depth == 0 => {
                if i == start {
                    return Err(invalid());
                }
                out.push(tokens[start].start..tokens[i - 1].end);
                start = i + 1;
            }
            _ => {}
        }
    }
    if depth != 0 || start == tokens.len() {
        return Err(invalid());
    }
    out.push(tokens[start].start..tokens.last().ok_or_else(invalid)?.end);
    Ok(out)
}

/// ASCII の区切りは引用符で守り、字の符号化には X2・X4 を使う。
fn string(value: &str) -> String {
    use std::fmt::Write;
    let mut out = String::from("'");
    let mut width = 0;
    for c in value.chars() {
        let next = if c.is_ascii_graphic() || c == ' ' {
            0
        } else if u32::from(c) <= 0xffff {
            4
        } else {
            8
        };
        if next != width {
            if width != 0 {
                out.push_str("\\X0\\");
            }
            if next != 0 {
                out.push_str(if next == 4 { "\\X2\\" } else { "\\X4\\" });
            }
            width = next;
        }
        match c {
            '\'' => out.push_str("''"),
            '\\' => out.push_str("\\\\"),
            _ if width == 0 => out.push(c),
            _ if width == 4 => write!(out, "{:04X}", u32::from(c)).expect("文字列へ書く"),
            _ => write!(out, "{:08X}", u32::from(c)).expect("文字列へ書く"),
        }
    }
    if width != 0 {
        out.push_str("\\X0\\");
    }
    out.push('\'');
    out
}

pub(crate) fn apply(source: &str, p: &Properties) -> Result<String, Problem> {
    let name_needed = [&p.title, &p.designer, &p.organization]
        .iter()
        .any(|v| !v.is_empty());
    if !name_needed && p.description.is_empty() {
        return Ok(source.to_owned());
    }
    let mut lex = Lexer { source, at: 0 };
    loop {
        let token = lex.required()?;
        if source[token].eq_ignore_ascii_case("HEADER") {
            let end = lex.required()?;
            if &source[end] != ";" {
                return Err(invalid());
            }
            break;
        }
    }
    let (mut name_found, mut desc_found) = (false, false);
    let mut replacements = Vec::new();
    loop {
        let record = lex.required()?;
        let name = &source[record];
        let open = lex.required()?;
        if name.eq_ignore_ascii_case("ENDSEC") {
            if &source[open] != ";" {
                return Err(invalid());
            }
            break;
        }
        if &source[open] != "(" {
            return Err(invalid());
        }
        let mut depth = 1;
        let mut tokens = Vec::new();
        while depth != 0 {
            let token = lex.required()?;
            match &source[token.clone()] {
                "(" => depth += 1,
                ")" => depth -= 1,
                ";" => return Err(invalid()),
                _ => {}
            }
            if depth != 0 {
                tokens.push(token);
            }
        }
        let end = lex.required()?;
        if &source[end] != ";" {
            return Err(invalid());
        }
        if name.eq_ignore_ascii_case("FILE_NAME") && name_needed {
            if name_found {
                return Err(invalid());
            }
            name_found = true;
            let values = args(source, &tokens)?;
            if values.len() != 7 {
                return Err(invalid());
            }
            for (index, value, list) in [
                (0, &p.title, false),
                (2, &p.designer, true),
                (3, &p.organization, true),
            ] {
                if !value.is_empty() {
                    let encoded = string(value);
                    replacements.push((
                        values[index].clone(),
                        if list {
                            format!("({encoded})")
                        } else {
                            encoded
                        },
                    ));
                }
            }
        } else if name.eq_ignore_ascii_case("FILE_DESCRIPTION") && !p.description.is_empty() {
            if desc_found {
                return Err(invalid());
            }
            desc_found = true;
            let values = args(source, &tokens)?;
            if values.len() != 2 {
                return Err(invalid());
            }
            replacements.push((values[0].clone(), format!("({})", string(&p.description))));
        }
    }
    if (name_needed && !name_found) || (!p.description.is_empty() && !desc_found) {
        return Err(invalid());
    }
    replacements.sort_by_key(|(range, _)| range.start);
    let mut out = String::with_capacity(source.len());
    let mut at = 0;
    for (range, value) in replacements {
        out.push_str(&source[at..range.start]);
        out.push_str(&value);
        at = range.end;
    }
    out.push_str(&source[at..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    const FILE: &str = "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('old description'),'2;1');\nFILE_NAME('old name','2026-10-29T12:00:00',('author','other author'),('org'),'unknown preprocessor','unknown originating system','authorization');\nFILE_SCHEMA(('schema'));\nENDSEC;\nDATA;\n#1=PRODUCT('FILE_NAME(a;b)','','',());\nENDSEC;\nEND-ISO-10303-21;\n";
    #[test]
    fn replaces_attributes_without_changing_the_rest_of_the_file() {
        let p = Properties {
            title: "plate".into(),
            designer: "author".into(),
            organization: "workshop".into(),
            description: "description".into(),
            ..Default::default()
        };
        let out = apply(FILE, &p).unwrap();
        let expected = FILE
            .replace("('old description')", "('description')")
            .replace("'old name'", "'plate'")
            .replace("('author','other author')", "('author')")
            .replace("('org')", "('workshop')");
        assert_eq!(out, expected);
        assert_eq!(
            out.split_once("\nDATA;\n").unwrap().1,
            FILE.split_once("\nDATA;\n").unwrap().1
        );
    }
    #[test]
    fn empty_items_keep_their_original_values_and_whitespace_is_a_value() {
        for (p, from, to) in [
            (
                Properties {
                    title: " ".into(),
                    ..Default::default()
                },
                "'old name'",
                "' '",
            ),
            (
                Properties {
                    designer: "new".into(),
                    ..Default::default()
                },
                "('author','other author')",
                "('new')",
            ),
            (
                Properties {
                    organization: "new".into(),
                    ..Default::default()
                },
                "('org')",
                "('new')",
            ),
            (
                Properties {
                    description: "new".into(),
                    ..Default::default()
                },
                "('old description')",
                "('new')",
            ),
        ] {
            assert_eq!(apply(FILE, &p).unwrap(), FILE.replace(from, to));
        }
        for src in [FILE, "not a STEP file"] {
            assert_eq!(
                apply(
                    src,
                    &Properties {
                        part_number: "123".into(),
                        custom: vec![("x".into(), "y".into())],
                        ..Default::default()
                    }
                )
                .unwrap(),
                src
            );
        }
    }
    #[test]
    fn quoted_delimiters_and_comments_are_not_header_records() {
        let source = "/* HEADER; FILE_NAME('decoy'); */ ISO-10303-21;\nheader;\nfile_description(('old; DATA; (name)'),'2;1');\nfile_name(/* keep */ 'old;'' name', 'stamp', ('one','two'), ('org'), 'pre;DATA;', 'origin', 'auth');\nFILE_SCHEMA(('HEADER; ENDSEC; FILE_NAME'));\nendsec;\nDATA;\n#1=PRODUCT('old;'' name','','',());\nENDSEC;";
        let p = Properties {
            title: "new; DATA; 'name'".into(),
            ..Default::default()
        };
        let expected = source.replacen(
            "/* keep */ 'old;'' name'",
            "/* keep */ 'new; DATA; ''name'''",
            1,
        );
        assert_eq!(apply(source, &p).unwrap(), expected);
    }
    #[test]
    fn step_strings_escape_quotes_backslashes_and_both_unicode_widths() {
        assert_eq!(
            string("部品 O'Reilly\\tool😀"),
            "'\\X2\\90E854C1\\X0\\ O''Reilly\\\\tool\\X4\\0001F600\\X0\\'"
        );
        assert_eq!(
            string("café台😀é"),
            "'caf\\X2\\00E953F0\\X0\\\\X4\\0001F600\\X0\\\\X2\\00E9\\X0\\'"
        );
        assert_eq!(string("a\nb\t"), "'a\\X2\\000A\\X0\\b\\X2\\0009\\X0\\'");
        assert_eq!(string("; DATA; (a,b)"), "'; DATA; (a,b)'");
    }
    #[test]
    fn malformed_or_duplicate_required_records_are_compute_problems() {
        let p = Properties {
            title: "new".into(),
            description: "new".into(),
            ..Default::default()
        };
        for source in [
            "",
            "DATA;",
            "HEADER; ENDSEC;",
            "HEADER; /* unterminated",
            "HEADER; FILE_NAME('unterminated); ENDSEC;",
            "HEADER; FILE_NAME('x','s',(''),(''),'pre','origin'); ENDSEC;",
            "HEADER; FILE_NAME('x','s',(''),(''),'pre','origin','auth'); FILE_DESCRIPTION(('x')); ENDSEC;",
            "HEADER; FILE_NAME('x','s',(''),(''),'pre','origin','auth'); FILE_NAME('y','s',(''),(''),'pre','origin','auth'); FILE_DESCRIPTION(('x'),'2;1'); ENDSEC;",
            "HEADER; FILE_NAME('x','s',(''),(''),'pre','origin','auth'); FILE_DESCRIPTION(('x'),'2;1'); FILE_DESCRIPTION(('x'),'2;1'); ENDSEC;",
            "HEADER; FILE_NAME('x',,'s',(''),(''),'pre','origin','auth'); ENDSEC;",
        ] {
            assert_eq!(
                apply(source, &p).unwrap_err().kind,
                ProblemKind::Compute,
                "{source}"
            );
        }
    }
}
