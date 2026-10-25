//! コアの公開の項目の一覧(`cargo xtask api`)。`verify` は分かれ目の一覧と今の一覧を比べ、変わって
//! いれば契約テスト(kmb-contract)を使う crate を全部選ぶ(版の数 `CORE_API_VERSION` を PR ごとに
//! 上げずに済む)。
//!
//! 組み立てずに字面で読む(rustdoc の JSON は nightly が要るため)。拾うのは `pub` で始まる項目の
//! 頭(関数なら引数と戻り値まで、構造体の欄なら型まで)と、`pub enum` の値・`pub trait` の項目。
//! 非公開の mod の中の `pub` も拾うので多めに出る(見落とすより、契約テストを余計に流す方を取る)。
//! `#[cfg(test)]` の範囲と、名前が `tests` の付くファイルは読まない。項目はどのファイルにあるかを
//! 問わずに比べる(大きいファイルを子のファイルへ分けただけでは、公開の項目は変わらないため)。

use crate::source::{Source, rust_files};
use std::collections::BTreeMap;
use std::path::Path;

/// 公開の項目と、それを持つファイル(同じ字面の項目が 2 つあれば 2 つ並ぶ)。
pub type Api = BTreeMap<String, Vec<String>>;

/// フォルダの下の `.rs` から一覧を作る。
pub fn of_dir(src: &Path) -> Api {
    let mut api = Api::new();
    for f in rust_files(src) {
        let rel = f
            .strip_prefix(src)
            .unwrap_or(&f)
            .to_string_lossy()
            .replace('\\', "/");
        if let Ok(text) = std::fs::read_to_string(&f) {
            add(&mut api, &rel, &text);
        }
    }
    api
}

/// 1 ファイル分を足す。`file` は一覧に書く名前。
pub fn add(api: &mut Api, file: &str, text: &str) {
    if file.contains("tests") {
        return;
    }
    for item in items(text) {
        api.entry(item).or_default().push(file.to_string());
    }
}

/// 2 つの一覧の差(消えた物、増えた物。`ファイル: 項目` の形)。数だけを比べる。
pub fn diff(old: &Api, new: &Api) -> (Vec<String>, Vec<String>) {
    (fewer(old, new), fewer(new, old))
}

/// `a` にあって `b` では数の減った項目。
fn fewer(a: &Api, b: &Api) -> Vec<String> {
    a.iter()
        .filter(|(k, fs)| b.get(*k).map_or(0, Vec::len) < fs.len())
        .map(|(k, fs)| format!("{}: {k}", fs[0]))
        .collect()
}

/// 公開の項目の字面(空白を 1 つにまとめた物)。
fn items(text: &str) -> Vec<String> {
    let src = Source::new(text);
    let mut code: Vec<u8> = src.code.clone().into_bytes();
    let len = code.len();
    for (a, b) in src.test_ranges() {
        for c in &mut code[a..b.min(len)] {
            if *c != b'\n' {
                *c = b' ';
            }
        }
    }
    let code = String::from_utf8_lossy(&code).into_owned();
    let b = code.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while let Some(off) = code[i..].find("pub") {
        let at = i + off;
        i = at + 3;
        let before_ok = at == 0 || !is_ident(b[at - 1]);
        let after_ok = b.get(at + 3).is_some_and(|c| c.is_ascii_whitespace());
        if !before_ok || !after_ok {
            continue;
        }
        let rest = &code[at..];
        let is_const = word_after(rest, &["pub", "const"]) || word_after(rest, &["pub", "static"]);
        let end = if word_after(rest, &["pub", "use"]) {
            // `pub use a::{B, C};` は名前の並びまで
            code[at..].find(';').map_or(b.len(), |k| at + k)
        } else {
            head_end(b, at, is_const)
        };
        let head = squash(&code[at..end]);
        out.push(head.clone());
        let opens_body = b.get(end) == Some(&b'{');
        if opens_body && (word_after(rest, &["pub", "enum"]) || word_after(rest, &["pub", "trait"]))
        {
            let close = src.block_end(end);
            let name = head
                .split_whitespace()
                .nth(2)
                .unwrap_or_default()
                .split(['<', ':'])
                .next()
                .unwrap_or_default()
                .to_string();
            for m in members(&code[end + 1..close.saturating_sub(1)]) {
                out.push(format!("{name}::{m}"));
            }
            i = close;
        }
    }
    out
}

/// `pub` から項目の頭の終わり(`{`・`;`・`,` か、波括弧の閉じ。定数は `=`)まで。
fn head_end(b: &[u8], from: usize, is_const: bool) -> usize {
    let mut depth = 0i32;
    let mut j = from;
    while j < b.len() {
        match b[j] {
            b'(' | b'[' | b'<' => depth += 1,
            b')' | b']' => depth -= 1,
            b'>' if j > 0 && b[j - 1] != b'-' && b[j - 1] != b'=' => depth -= 1,
            b'{' | b';' | b'}' if depth <= 0 => return j,
            b',' if depth <= 0 => return j,
            b'=' if is_const && depth <= 0 => return j,
            _ => {}
        }
        j += 1;
    }
    j
}

/// enum の値・trait の項目(本体の中の 1 段目)。
fn members(body: &str) -> Vec<String> {
    let b = body.as_bytes();
    let mut out = vec![];
    let mut depth = 0i32;
    let mut start = 0;
    let push = |s: &str, out: &mut Vec<String>| {
        let s = squash(s);
        if !s.is_empty() {
            out.push(s);
        }
    };
    for (j, &c) in b.iter().enumerate() {
        match c {
            b'(' | b'[' | b'<' => depth += 1,
            b')' | b']' => depth -= 1,
            b'>' if j > 0 && b[j - 1] != b'-' && b[j - 1] != b'=' => depth -= 1,
            b'{' => {
                if depth == 0 {
                    // trait の既定の本体・enum の欄つきの値: 頭だけ取り、本体は飛ばす
                    push(&body[start..j], &mut out);
                    start = usize::MAX;
                }
                depth += 1;
            }
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    start = j + 1;
                }
            }
            b',' | b';' if depth == 0 => {
                if start != usize::MAX && start <= j {
                    push(&body[start..j], &mut out);
                }
                start = j + 1;
            }
            _ => {}
        }
    }
    if start != usize::MAX && start < body.len() {
        push(&body[start..], &mut out);
    }
    out
}

fn word_after(s: &str, words: &[&str]) -> bool {
    let mut it = s.split_whitespace();
    words.iter().all(|w| it.next() == Some(*w))
}

fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heads_variants_and_trait_items() {
        let text = "pub struct A {\n    pub x: f64,\n    y: f64,\n}\n\
            pub fn f<T: Into<String>>(\n    a: T,\n) -> Result<(), String> {\n    Ok(())\n}\n\
            pub const V: &str = \"0.1\";\n\
            pub use m::{\n    X,\n    Y,\n};\n\
            pub enum E {\n    One,\n    Two(u8),\n    Three { z: i32 },\n}\n\
            pub trait T {\n    fn g(&self) -> u8;\n    fn h(&self) {\n        let _ = 1;\n    }\n}\n\
            pub(crate) fn hidden() {}\n\
            #[cfg(test)]\nmod tests {\n    pub fn test_only() {}\n}\n";
        let got = items(text);
        assert_eq!(
            got,
            vec![
                "pub struct A",
                "pub x: f64",
                "pub fn f<T: Into<String>>( a: T, ) -> Result<(), String>",
                "pub const V: &str",
                "pub use m::{ X, Y, }",
                "pub enum E",
                "E::One",
                "E::Two(u8)",
                "E::Three",
                "pub trait T",
                "T::fn g(&self) -> u8",
                "T::fn h(&self)",
            ]
        );
    }

    #[test]
    fn diff_finds_added_and_removed() {
        let mut old = Api::new();
        add(&mut old, "a.rs", "pub fn f() {}\npub fn g() {}\n");
        let mut new = Api::new();
        add(&mut new, "a.rs", "pub fn f() {}\npub fn h(x: u8) {}\n");
        let (lost, gained) = diff(&old, &new);
        assert_eq!(lost, vec!["a.rs: pub fn g()"]);
        assert_eq!(gained, vec!["a.rs: pub fn h(x: u8)"]);
        // 子のファイルへ移しただけなら差は出ない
        let mut moved = Api::new();
        add(&mut moved, "a.rs", "pub fn f() {}\n");
        add(&mut moved, "a/g.rs", "pub fn g() {}\n");
        assert_eq!(diff(&old, &moved), (vec![], vec![]));
        // 版の値だけを変えても差は出ない
        let mut a = Api::new();
        add(
            &mut a,
            "lib.rs",
            "pub const CORE_API_VERSION: &str = \"0.39.0\";\n",
        );
        let mut b = Api::new();
        add(
            &mut b,
            "lib.rs",
            "pub const CORE_API_VERSION: &str = \"0.40.0\";\n",
        );
        assert_eq!(diff(&a, &b), (vec![], vec![]));
    }
}
