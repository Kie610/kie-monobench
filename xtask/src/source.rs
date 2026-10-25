//! Rust のソースを、組み立てずに字面で読む部品(層の点検と飛ばしの点検が使う)。
//!
//! 字句を簡単に分けるだけで、構文は解かない。コメントと文字列の中身を空白に置き換えた「コード」を
//! 作るので、波括弧や `#[cfg(test)]` を数える時に、文字列やコメントの中の物を取り違えない。

use std::fs;
use std::path::{Path, PathBuf};

/// 文字列のリテラル 1 つ(中身は書いたままの字。エスケープは解かない)。
#[derive(Debug, Clone, PartialEq)]
pub struct Literal {
    /// 始まりの `"` の位置(バイト)
    pub at: usize,
    pub text: String,
}

/// 読んだソース。
pub struct Source {
    /// コメントと文字列の中身を空白にしたもの(改行と位置は元のまま)
    pub code: String,
    pub literals: Vec<Literal>,
    line_starts: Vec<usize>,
}

impl Source {
    pub fn new(text: &str) -> Self {
        let (code, literals) = mask(text);
        let line_starts = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        Source {
            code,
            literals,
            line_starts,
        }
    }

    /// 位置(バイト)の行番号(1 から)。
    pub fn line_of(&self, at: usize) -> usize {
        self.line_starts.partition_point(|&s| s <= at)
    }

    /// `#[cfg(test)]` を付けた項目(mod・fn・use など)の範囲。項目が波括弧を持てば、その閉じまで。
    pub fn test_ranges(&self) -> Vec<(usize, usize)> {
        let b = self.code.as_bytes();
        let mut out = vec![];
        for (at, _) in self.code.match_indices("#[cfg(test)]") {
            let mut i = at + "#[cfg(test)]".len();
            let mut depth = 0usize;
            while i < b.len() {
                match b[i] {
                    b'{' => depth += 1,
                    b'}' => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            i += 1;
                            break;
                        }
                    }
                    b';' if depth == 0 => {
                        i += 1;
                        break;
                    }
                    _ => {}
                }
                i += 1;
            }
            out.push((at, i));
        }
        out
    }

    /// 開き括弧 `{` の位置から、対になる `}` の次の位置。
    pub fn block_end(&self, open: usize) -> usize {
        let b = self.code.as_bytes();
        let mut depth = 0usize;
        for (i, &c) in b.iter().enumerate().skip(open) {
            match c {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return i + 1;
                    }
                }
                _ => {}
            }
        }
        b.len()
    }
}

fn is_ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// コメントと文字列の中身を空白にする。文字列は取り出して返す。
fn mask(text: &str) -> (String, Vec<Literal>) {
    let b = text.as_bytes();
    let mut out = b.to_vec();
    let mut lits = vec![];
    let blank = |out: &mut Vec<u8>, from: usize, to: usize| {
        for c in &mut out[from..to] {
            if *c != b'\n' {
                *c = b' ';
            }
        }
    };
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'/' && b.get(i + 1) == Some(&b'/') {
            let end = text[i..].find('\n').map_or(b.len(), |e| i + e);
            blank(&mut out, i, end);
            i = end;
        } else if c == b'/' && b.get(i + 1) == Some(&b'*') {
            // 入れ子のコメント
            let mut depth = 0;
            let mut j = i;
            while j < b.len() {
                if b[j] == b'/' && b.get(j + 1) == Some(&b'*') {
                    depth += 1;
                    j += 2;
                } else if b[j] == b'*' && b.get(j + 1) == Some(&b'/') {
                    depth -= 1;
                    j += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    j += 1;
                }
            }
            blank(&mut out, i, j);
            i = j;
        } else if (c == b'r' || (c == b'b' && b.get(i + 1) == Some(&b'r')))
            && (i == 0 || !is_ident(b[i - 1]))
            && {
                let s = if c == b'r' { i + 1 } else { i + 2 };
                let mut k = s;
                while b.get(k) == Some(&b'#') {
                    k += 1;
                }
                b.get(k) == Some(&b'"')
            }
        {
            // 生の文字列 r"…"・r#"…"#
            let s = if c == b'r' { i + 1 } else { i + 2 };
            let mut k = s;
            while b[k] == b'#' {
                k += 1;
            }
            let hashes = k - s;
            let close: String = std::iter::once('"')
                .chain(std::iter::repeat_n('#', hashes))
                .collect();
            let body = k + 1;
            let end = text[body..].find(&close).map_or(b.len(), |e| body + e);
            lits.push(Literal {
                at: k,
                text: text[body..end].to_string(),
            });
            blank(&mut out, body, end);
            i = (end + close.len()).min(b.len());
        } else if c == b'"' {
            let body = i + 1;
            let mut j = body;
            while j < b.len() && b[j] != b'"' {
                j += if b[j] == b'\\' { 2 } else { 1 };
            }
            let end = j.min(b.len());
            lits.push(Literal {
                at: i,
                text: text[body..end].to_string(),
            });
            blank(&mut out, body, end);
            i = end + 1;
        } else if c == b'\'' {
            // 文字のリテラル('x'・'\n'・'\u{..}')。寿命('a)はそのまま
            if b.get(i + 1) == Some(&b'\\') {
                let end = text[i + 2..].find('\'').map_or(b.len(), |e| i + 2 + e);
                blank(&mut out, i + 1, end);
                i = end + 1;
            } else if let Some(ch) = text[i + 1..].chars().next() {
                let n = ch.len_utf8();
                if b.get(i + 1 + n) == Some(&b'\'') {
                    blank(&mut out, i + 1, i + 1 + n);
                    i += n + 2;
                } else {
                    i += 1;
                }
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    (String::from_utf8_lossy(&out).into_owned(), lits)
}

/// フォルダの下の `.rs`(並びは名前の順)。
pub fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(rust_files(&p));
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
    out.sort();
    out
}

/// テストの関数 1 つ。
#[derive(Debug, Clone, PartialEq)]
pub struct TestFn {
    pub name: String,
    pub line: usize,
    /// `#[ignore]` が付いているか。付いていれば理由(`= "…"` が無ければ空)
    pub ignore: Option<String>,
    /// 本体 `{ … }` の範囲
    pub body: (usize, usize),
}

/// `#[test]` を付けた関数。
pub fn test_fns(src: &Source) -> Vec<TestFn> {
    let code = &src.code;
    let mut out = vec![];
    for (at, _) in code.match_indices("#[test]") {
        // 次の `fn 名前` までの属性を見る
        let rest = &code[at..];
        let Some(f) = find_fn(rest) else { continue };
        let attrs_end = at + f;
        let attrs = &code[at..attrs_end];
        let ignore = attrs.find("#[ignore").map(|i| {
            let a = at + i;
            let close = code[a..].find(']').map_or(attrs_end, |e| a + e);
            src.literals
                .iter()
                .find(|l| l.at > a && l.at < close)
                .map(|l| l.text.clone())
                .unwrap_or_default()
        });
        let after = &code[attrs_end..];
        let name: String = after["fn".len()..]
            .trim_start()
            .bytes()
            .take_while(|&c| is_ident(c))
            .map(char::from)
            .collect();
        let Some(open) = after.find('{').map(|o| attrs_end + o) else {
            continue;
        };
        out.push(TestFn {
            name,
            line: src.line_of(attrs_end),
            ignore,
            body: (open, src.block_end(open)),
        });
    }
    out
}

/// 属性の並びの後の `fn` の位置(`pub fn`・`async fn` も)。
fn find_fn(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut i = 0;
    while let Some(p) = s[i..].find("fn") {
        let at = i + p;
        let before_ok = at == 0 || !is_ident(b[at - 1]);
        let after_ok = b.get(at + 2).is_some_and(|c| c.is_ascii_whitespace());
        if before_ok && after_ok {
            return Some(at);
        }
        i = at + 2;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments_and_strings_are_blanked_and_strings_are_kept() {
        let text = "let a = \"x{y\"; // }\n/* { /* } */ */ let b = r#\"q\"z\"#; let c = '{'; fn f<'a>() {}";
        let s = Source::new(text);
        assert!(!s.code.contains("x{y"));
        assert_eq!(s.code.matches('{').count(), 1);
        assert_eq!(s.code.matches('}').count(), 1);
        let texts: Vec<&str> = s.literals.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["x{y", "q\"z"]);
        assert_eq!(s.line_of(0), 1);
        assert_eq!(s.line_of(text.find("let b").unwrap()), 2);
        // 字の長さが変わらない(行と位置がずれない)
        assert_eq!(s.code.len(), text.len());
    }

    #[test]
    fn a_cfg_test_covers_only_the_item_after_it() {
        let text = "fn a() {}\n#[cfg(test)]\nfn helper() { let x = \"}\"; }\nfn b() { \"cad.x\"; }\n#[cfg(test)]\nuse std::fmt;\n#[cfg(test)]\nmod tests { mod inner { } }\nfn c() {}";
        let s = Source::new(text);
        let r = s.test_ranges();
        assert_eq!(r.len(), 3);
        let covered = |pat: &str| {
            let at = text.find(pat).unwrap();
            r.iter().any(|&(a, b)| a <= at && at < b)
        };
        assert!(covered("helper"));
        assert!(!covered("fn b"));
        assert!(covered("std::fmt"));
        assert!(covered("inner"));
        assert!(!covered("fn c"));
    }

    #[test]
    fn test_functions_are_found_with_their_ignore_reason() {
        let text = "#[test]\nfn plain() { return; }\n#[test]\n#[ignore]\nfn bare() {}\n#[test]\n#[ignore = \"道具: 絵を書く\"]\npub fn why() { let s = \"{\"; }\n";
        let s = Source::new(text);
        let t = test_fns(&s);
        let names: Vec<&str> = t.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["plain", "bare", "why"]);
        assert_eq!(t[0].ignore, None);
        assert_eq!(t[1].ignore.as_deref(), Some(""));
        assert_eq!(t[2].ignore.as_deref(), Some("道具: 絵を書く"));
        assert_eq!(t[1].line, 5);
        assert!(s.code[t[0].body.0..t[0].body.1].contains("return"));
        assert!(s.code[t[2].body.0..t[2].body.1].ends_with('}'));
    }
}
