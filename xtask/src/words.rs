//! 言葉の揺れの点検: 用語表(docs/用語表.md)で使わないと決めた語が、画面の字に入ったら落とす。
//! 10-23 u4(2026-10-25)。
//!
//! 同じ物は画面のどこでも同じ語で呼ぶ決まり(用語表「書き方の決まり」)を字面で守る。用語表の
//! 「使わない語(検査で止める)」の表の語と、ソースの文字列のリテラルが丸ごと同じ時だけ落とす。
//! 文の中の語(「固定を外すと最新にできます」)は数えない。「当てた言葉」の表の前の名前は、
//! 「向き」「寸法」のように別の所では正しい語も多いので、ここでは使わない。
//!
//! 見るのは crates/ と apps/ の `src` の下のコードだけ。テスト(`#[cfg(test)]` の中と、名前が
//! `tests` で終わるファイル・フォルダ)は、操作の名前に打つ字として使うので見ない。

use crate::source::{Source, rust_files};
use std::fs;
use std::path::Path;

/// 用語表の置き場所。
const TERMS: &str = "docs/用語表.md";
/// 使わない語の表の見出し。
const HEADING: &str = "## 使わない語(検査で止める)";

/// 使わない語と使う語。
#[derive(Debug, PartialEq)]
pub struct Banned {
    pub word: String,
    pub instead: String,
}

/// 点検の誤りの文。
pub fn check(root: &Path) -> Vec<String> {
    let Ok(terms) = fs::read_to_string(root.join(TERMS)) else {
        return vec![format!("言葉の揺れ: {TERMS} を読めません")];
    };
    let banned = match banned_words(&terms) {
        Ok(b) => b,
        Err(e) => return vec![format!("言葉の揺れ: {TERMS}: {e}")],
    };
    let mut out = vec![];
    for top in ["crates", "apps"] {
        let Ok(dirs) = fs::read_dir(root.join(top)) else {
            continue;
        };
        let mut dirs: Vec<_> = dirs.flatten().map(|d| d.path()).collect();
        dirs.sort();
        for dir in dirs {
            for file in rust_files(&dir.join("src")) {
                let rel = file.strip_prefix(root).unwrap_or(&file);
                let rel = rel.to_string_lossy().replace('\\', "/");
                if is_test_file(&rel) {
                    continue;
                }
                let Ok(text) = fs::read_to_string(&file) else {
                    continue;
                };
                out.extend(problems(&rel, &text, &banned));
            }
        }
    }
    out
}

/// 用語表の「使わない語」の表(1 列目が使わない語、2 列目が使う語)。
pub fn banned_words(terms: &str) -> Result<Vec<Banned>, String> {
    let terms = terms.replace("\r\n", "\n");
    let Some(at) = terms.find(&format!("\n{HEADING}\n")) else {
        return Err(format!("見出し「{HEADING}」がありません"));
    };
    let mut out = vec![];
    let mut header = true;
    for line in terms[at + 1..].lines().skip(1) {
        if line.starts_with("## ") {
            break;
        }
        let Some(row) = line.trim().strip_prefix('|') else {
            continue;
        };
        let cells: Vec<&str> = row
            .trim_end_matches('|')
            .split('|')
            .map(str::trim)
            .collect();
        if cells
            .iter()
            .all(|c| c.chars().all(|ch| matches!(ch, '-' | ':')))
        {
            continue;
        }
        if std::mem::take(&mut header) {
            continue;
        }
        match cells.as_slice() {
            [word, instead, ..] if !word.is_empty() && !instead.is_empty() => out.push(Banned {
                word: word.to_string(),
                instead: instead.to_string(),
            }),
            _ => return Err(format!("使わない語の行が読めません: {line}")),
        }
    }
    if out.is_empty() {
        return Err(format!("「{HEADING}」の表に行がありません"));
    }
    Ok(out)
}

/// テストのファイル(名前が `tests` で終わるファイルかフォルダの下)。
fn is_test_file(rel: &str) -> bool {
    rel.split('/').any(|p| {
        let p = p.strip_suffix(".rs").unwrap_or(p);
        p.ends_with("tests")
    })
}

/// 1 つのファイルの中の使わない語(テストの中は見ない)。
pub fn problems(path: &str, text: &str, banned: &[Banned]) -> Vec<String> {
    let src = Source::new(text);
    let tests = src.test_ranges();
    let mut out = vec![];
    for lit in &src.literals {
        if tests.iter().any(|&(a, b)| (a..b).contains(&lit.at)) {
            continue;
        }
        if let Some(b) = banned.iter().find(|b| b.word == lit.text) {
            out.push(format!(
                "{path}:{}: 画面の字「{}」は用語表で使わない語。「{}」にする({TERMS} の「使わない語」)",
                src.line_of(lit.at),
                b.word,
                b.instead
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const TERMS: &str = "# 用語表\n\n## 使わない語(検査で止める)\n\n説明。\n\n\
        | 使わない語 | 使う語 | 理由 |\n|---|---|---|\n| 消す | 削除 | — |\n| やめる | キャンセル | |\n\n\
        ## 次の節\n\n| 消す2 | x |\n";

    fn banned() -> Vec<Banned> {
        banned_words(TERMS).unwrap()
    }

    #[test]
    fn the_table_is_read_up_to_the_next_heading_and_crlf_reads_the_same() {
        let b = banned();
        assert_eq!(
            b.iter().map(|b| b.word.as_str()).collect::<Vec<_>>(),
            ["消す", "やめる"]
        );
        assert_eq!(b[1].instead, "キャンセル");
        assert_eq!(banned_words(&TERMS.replace('\n', "\r\n")).unwrap(), b);
        assert!(banned_words("# 用語表\n").is_err());
    }

    #[test]
    fn only_whole_literals_outside_tests_are_reported() {
        let code = "fn a(ui: &mut Ui) {\n    ui.button(\"消す\");\n    ui.label(\"消すと戻せない\");\n    \
            // \"やめる\"\n}\n#[cfg(test)]\nmod t {\n    fn b() { click(\"やめる\"); }\n}\n";
        let ps = problems("crates/jc-x/src/lib.rs", code, &banned());
        assert_eq!(ps.len(), 1, "{ps:?}");
        assert!(ps[0].starts_with("crates/jc-x/src/lib.rs:2:") && ps[0].contains("「削除」"));
    }

    #[test]
    fn test_files_are_skipped() {
        assert!(is_test_file("crates/jc-ui/src/render/render_tests.rs"));
        assert!(is_test_file(
            "crates/jc-solid/src/asm_tests/motion_tests.rs"
        ));
        assert!(is_test_file("crates/jc-core/src/tests.rs"));
        assert!(!is_test_file("crates/jc-ui/src/render/gallery_ui.rs"));
    }

    #[test]
    fn the_repository_terms_table_has_the_banned_words() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let terms = fs::read_to_string(root.join(super::TERMS)).unwrap();
        let b = banned_words(&terms).unwrap();
        assert!(
            b.iter().any(|b| b.word == "消す" && b.instead == "削除"),
            "{b:?}"
        );
    }
}
