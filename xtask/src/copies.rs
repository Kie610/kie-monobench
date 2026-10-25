//! 別の crate にある同じ関数の写しを数える。
//!
//! 共通の部品の置き場が無いと、写した方が早いので、小さな幾何や線のつなぎ方が crate ごとに増え、
//! 片方だけ直して食い違う(2026-10-22 に、線の読み方の写しの片方だけがセットアップの回し方を
//! 当てていなかった)。本体が 8 行以上で、コメントと空白を除いた中身(文字列を含む)が同じ関数が
//! 2 つ以上の crate にあれば写しとする。今ある写しは xtask/copies.txt に並べ、増えたら落とす。
//! 直して無くなった写しの行が残っていても落とす(表を今に合わせる)。

use crate::source::Source;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

/// 写しとみなす本体の行の数の下限。
const MIN_LINES: usize = 8;
const LIST: &str = "xtask/copies.txt";

/// 関数 1 つ: (crate, ファイル, 名前, 行)。
type Place = (String, String, String, usize);

/// ソース `src` の関数の (名前, 本体を比べる鍵, 行)。本体が短い物は除く。
fn functions(src: &Source) -> Vec<(String, String, usize)> {
    let code = &src.code;
    let b = code.as_bytes();
    let mut out = vec![];
    for (at, _) in code.match_indices("fn ") {
        if at > 0 && (b[at - 1].is_ascii_alphanumeric() || b[at - 1] == b'_') {
            continue;
        }
        let rest = &code[at + 3..];
        let name: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if name.is_empty() {
            continue;
        }
        // 本体の開き括弧(宣言だけの `;` が先なら本体が無い)
        let Some(open) = code[at..].find(['{', ';']).map(|i| at + i) else {
            continue;
        };
        if b[open] != b'{' {
            continue;
        }
        let end = src.block_end(open);
        let body = &code[open..end];
        if body.matches('\n').count() < MIN_LINES {
            continue;
        }
        let mut key: String = body.chars().filter(|c| !c.is_whitespace()).collect();
        for l in src.literals.iter().filter(|l| open <= l.at && l.at < end) {
            key.push('\u{1}');
            key.push_str(&l.text);
        }
        out.push((name, key, src.line_of(at)));
    }
    out
}

/// 写しの組の名前(`crate::関数` を並べた物)。表の 1 行になる。
fn group_name(places: &[Place]) -> String {
    let names: BTreeSet<String> = places
        .iter()
        .map(|(k, _, n, _)| format!("{k}::{n}"))
        .collect();
    names.into_iter().collect::<Vec<_>>().join(" = ")
}

/// 今の写しの組(名前 → 場所)。`sources` は crate ごとの (相対パス, ソース)。
pub fn groups(sources: &[(String, String, Source)]) -> BTreeMap<String, Vec<Place>> {
    let mut by_key: BTreeMap<String, Vec<Place>> = BTreeMap::new();
    for (krate, rel, src) in sources {
        for (name, key, line) in functions(src) {
            by_key
                .entry(key)
                .or_default()
                .push((krate.clone(), rel.clone(), name, line));
        }
    }
    by_key
        .into_values()
        .filter(|v| v.iter().map(|p| &p.0).collect::<BTreeSet<_>>().len() > 1)
        .map(|v| (group_name(&v), v))
        .collect()
}

/// 表 `text` の行(空行と `#` の行を除く)。
fn listed(text: &str) -> BTreeSet<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// 点検: 表に無い写しと、もう無いのに表に残る行。`JC_BLESS=1` なら表を今に書き直す。
pub fn check(root: &Path, sources: &[(String, String, Source)]) -> Vec<String> {
    let now = groups(sources);
    let path = root.join(LIST);
    if std::env::var_os("JC_BLESS").is_some() {
        let mut text = String::from(
            "# 別の crate にある同じ関数の写し(xtask/src/copies.rs)。1 行が 1 組。\n\
             # 増やさない。共通の部品(jc-cam-geom・jc-cam-setup など)へ寄せて直したら行を消す。\n\
             # 手で直さず JC_BLESS=1 cargo xtask checks で書き直す(増えた行は直してから)。\n",
        );
        for g in now.keys() {
            text.push_str(g);
            text.push('\n');
        }
        let _ = fs::write(&path, text);
    }
    let known = listed(&fs::read_to_string(&path).unwrap_or_default());
    let mut out = vec![];
    for (g, places) in &now {
        if known.contains(g) {
            continue;
        }
        let at: Vec<String> = places
            .iter()
            .map(|(k, f, _, l)| format!("{k}/{f}:{l}"))
            .collect();
        out.push(format!(
            "同じ関数の写しが別の crate に増えた: {g}({})。共通の部品へ寄せる",
            at.join("、")
        ));
    }
    for g in known.iter().filter(|g| !now.contains_key(*g)) {
        out.push(format!(
            "写し「{g}」はもう無い。{LIST} から行を消す(JC_BLESS=1 cargo xtask checks)"
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(text: &str) -> Source {
        Source::new(text)
    }

    #[test]
    fn long_bodies_that_match_across_crates_are_copies() {
        let body = "fn dist(a: P, b: P) -> f64 {\n    let x = a[0] - b[0];\n    let y = a[1] - b[1];\n    // 長さ\n    let z = 0.0;\n    let w = 0.0;\n    let v = 0.0;\n    let u = 0.0;\n    (x * x + y * y).sqrt()\n}\n";
        let other = body
            .replace("// 長さ", "// ちがう説明")
            .replace("    let", "  let");
        let short = "fn tiny() -> f64 {\n    1.0\n}\n";
        let s = vec![
            ("jc-a".into(), "src/lib.rs".into(), src(body)),
            ("jc-b".into(), "src/geo.rs".into(), src(&other)),
            ("jc-b".into(), "src/x.rs".into(), src(short)),
            ("jc-c".into(), "src/x.rs".into(), src(short)),
        ];
        let g = groups(&s);
        assert_eq!(
            g.keys().collect::<Vec<_>>(),
            vec!["jc-a::dist = jc-b::dist"]
        );
        // 同じ crate の中の写しは数えない
        let same = vec![
            ("jc-a".into(), "src/lib.rs".into(), src(body)),
            ("jc-a".into(), "src/b.rs".into(), src(body)),
        ];
        assert!(groups(&same).is_empty());
    }
}
