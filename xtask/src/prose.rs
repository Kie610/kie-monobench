//! 使う人向けの資料の点検: README・CONTRIBUTING・使い方などの文が、書き方の決まり
//! (.claude/rules/docs.md)から外れたら落とす。
//!
//! 字面で確かめられる物だけを見る。
//! - 作る側の名前と経緯: 決めた人・AI・スレッドの語と、日付(YYYY-MM-DD)。
//! - 文の長さ: 「。」で区切った 1 文が `MAX_CHARS` 字を超える。
//! - 文体: 地の文と箇条書きの「。」で終わる文が、敬体(です・ます)で終わらない。
//!
//! コードの区画・インラインのコード・リンクの行き先・HTML の行は見ない。表の行は語と日付だけを見る
//! (欄の中は文でないことが多いため)。
//!
//! 決まりと記録の資料(policy・plan・verify・地図・用語表・docs/機能/)は常体で、測った日や特許の
//! 満了の日のような事実の日付も書くので、決めた人と AI の名前だけを見る(`check_records`)。

use std::fs;
use std::path::Path;

/// 見るファイル(リポジトリの根から)。
pub const FILES: &[&str] = &[
    "README.md",
    "CONTRIBUTING.md",
    "SECURITY.md",
    "TRADEMARKS.md",
    "docs/使い方.md",
    "docs/できないことと予定.md",
];

/// 決まりと記録の資料(リポジトリの根から)。ほかに docs/機能/ の全部を見る。
pub const RECORD_FILES: &[&str] = &[
    "docs/policy.md",
    "docs/plan.md",
    "docs/verify.md",
    "docs/地図.md",
    "docs/用語表.md",
    "docs/避ける形.md",
    "docs/受け方.md",
];

/// 書かない語。後ろに英数字が続く所(`Kie610` など)は数えない。
const WORDS: &[&str] = &["Kie", "Claude", "スレッド", "コーディネーター"];

/// 決まりと記録の資料で書かない語(「スレッド」は計算の用語でも使うので見ない)。
const RECORD_WORDS: &[&str] = &["Kie", "Claude", "コーディネーター"];

/// 互換を示すために許す書き方(AI の道具の名前と、画面の下の帯の字)。
const ALLOWED: &[&str] = &[
    "Claude Code",
    "Claude Desktop",
    "Claude デスクトップ",
    "Claude 待ち受け中",
    "Claude 操作中",
];

/// 1 文の長さの上限(字)。
const MAX_CHARS: usize = 120;

/// 敬体の文末。
const POLITE: &[&str] = &[
    "です",
    "ます",
    "ません",
    "ください",
    "でした",
    "ました",
    "ましょう",
];

/// 点検の誤りの文。
pub fn check(root: &Path) -> Vec<String> {
    let mut out = vec![];
    for f in FILES {
        let Ok(text) = fs::read_to_string(root.join(f)) else {
            continue;
        };
        out.extend(problems(f, &text));
    }
    out
}

/// 決まりと記録の資料の誤り(点検: 記録の資料の経緯)。決めた人と AI の名前を書いた行を落とす。
pub fn check_records(root: &Path) -> Vec<String> {
    let mut files: Vec<String> = RECORD_FILES.iter().map(|f| f.to_string()).collect();
    let mut frags: Vec<String> = fs::read_dir(root.join("docs/機能"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".md"))
        .map(|n| format!("docs/機能/{n}"))
        .collect();
    frags.sort();
    files.extend(frags);
    let mut out = vec![];
    for f in &files {
        let Ok(text) = fs::read_to_string(root.join(f)) else {
            continue;
        };
        out.extend(record_problems(f, &text));
    }
    out
}

/// 決まりと記録の資料 1 つの誤り。
pub fn record_problems(file: &str, text: &str) -> Vec<String> {
    let mut out = vec![];
    let mut fenced = false;
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        for w in names(&plain(line), RECORD_WORDS) {
            out.push(format!(
                "{file}:{}: 「{w}」を書かない(決めた人と経緯はリポジトリの外の開発の記録へ)",
                i + 1
            ));
        }
    }
    out
}

/// 1 ファイルの誤り。
pub fn problems(file: &str, text: &str) -> Vec<String> {
    let mut out = vec![];
    let mut fenced = false;
    for (i, raw) in text.lines().enumerate() {
        let n = i + 1;
        let line = raw.trim();
        if line.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced || line.starts_with('<') || line.starts_with('[') && line.contains("]:") {
            continue;
        }
        let plain = plain(line);
        for w in names(&plain, WORDS) {
            out.push(format!(
                "{file}:{n}: 「{w}」を書かない(作る側の名前と経緯は外の記録へ)"
            ));
        }
        if let Some(d) = date(&plain) {
            out.push(format!(
                "{file}:{n}: 日付「{d}」を書かない(経緯は外の記録へ)"
            ));
        }
        if line.starts_with('|') || line.starts_with('#') {
            continue;
        }
        let body = plain.trim_start_matches(['-', '*', '>', ' ']);
        let body = body
            .trim_start_matches(|c: char| c.is_ascii_digit())
            .trim_start_matches(['.', ' ']);
        for s in sentences(body) {
            let len = s.chars().count();
            if len > MAX_CHARS {
                let head: String = s.chars().take(20).collect();
                out.push(format!(
                    "{file}:{n}: 1 文が {len} 字(上限 {MAX_CHARS} 字): 「{head}…」"
                ));
            }
            if !polite(&s) {
                let tail: String = s
                    .chars()
                    .rev()
                    .take(12)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                out.push(format!("{file}:{n}: 敬体で終わっていない: 「…{tail}。」"));
            }
        }
    }
    out
}

/// インラインのコードとリンクの行き先を除いた字。リンクは見える字だけを残す。
fn plain(line: &str) -> String {
    let mut out = String::new();
    let mut code = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '`' {
            code = !code;
            continue;
        }
        if code {
            continue;
        }
        // リンクの行き先 `](...)` を飛ばす
        if c == ']' && chars.peek() == Some(&'(') {
            let mut depth = 0;
            for d in chars.by_ref() {
                match d {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            continue;
        }
        if c == '[' || c == '!' && chars.peek() == Some(&'[') {
            continue;
        }
        out.push(c);
    }
    out
}

/// 書かない語のうち、行にある物。
fn names(text: &str, words: &[&'static str]) -> Vec<&'static str> {
    let mut rest = text.to_string();
    for a in ALLOWED {
        rest = rest.replace(a, "");
    }
    words
        .iter()
        .copied()
        .filter(|w| {
            rest.match_indices(w).any(|(at, _)| {
                !rest[at + w.len()..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphanumeric())
            })
        })
        .collect()
}

/// 行の最初の日付(YYYY-MM-DD)。
fn date(text: &str) -> Option<String> {
    let cs: Vec<char> = text.chars().collect();
    let digits = |r: std::ops::Range<usize>| r.into_iter().all(|i| cs[i].is_ascii_digit());
    (0..cs.len().saturating_sub(9)).find_map(|i| {
        let ok = digits(i..i + 4)
            && cs[i + 4] == '-'
            && digits(i + 5..i + 7)
            && cs[i + 7] == '-'
            && digits(i + 8..i + 10)
            && cs[i] == '2'
            && (i == 0 || !cs[i - 1].is_ascii_digit());
        ok.then(|| cs[i..i + 10].iter().collect())
    })
}

/// 「。」で終わる文(最後の「。」より後ろの、終わっていない字は見ない)。
fn sentences(text: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut depth = 0i32;
    for c in text.chars() {
        match c {
            '(' | '（' | '「' => depth += 1,
            ')' | '）' | '」' => depth -= 1,
            _ => {}
        }
        if c == '。' && depth <= 0 {
            let s = cur.trim().to_string();
            if !s.is_empty() {
                out.push(s);
            }
            cur.clear();
            depth = 0;
        } else {
            cur.push(c);
        }
    }
    out
}

/// 文末が敬体か。終わりの括弧・かぎ括弧・強調の記号は外して見る。
fn polite(s: &str) -> bool {
    let mut t = s.trim_end_matches(['*', '_', ' ']).to_string();
    // 文末の括弧書き「…です(〜)」は括弧の前で見る
    while t.ends_with(')') || t.ends_with('）') {
        let open = t.rfind(['(', '（']);
        match open {
            Some(at) => t.truncate(at),
            None => break,
        }
        t = t.trim_end().to_string();
    }
    let t = t.trim_end_matches(['」', '*', '_', ' ']);
    POLITE.iter().any(|p| t.ends_with(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_polite_sentences_pass() {
        let text = "# 始める\n\n見本を開けます。[頁](https://github.com/Kie610/x)を見てください。\n\n- 保存は Ctrl+S です(`.bak` も残ります)。\n\n```sh\nkmb 2026-10-26\n```\n| 欄 | 2026 |\n";
        assert!(
            problems("a.md", text).is_empty(),
            "{:?}",
            problems("a.md", text)
        );
    }

    #[test]
    fn plain_form_is_caught_but_not_inside_brackets() {
        let p = problems("a.md", "保存する。「保存する。」と出ます。\n");
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(p[0].contains("保存する"));
    }

    #[test]
    fn names_and_dates_are_caught_outside_urls_and_code() {
        let p = problems("a.md", "Kie が決めました(2026-10-21)。`Kie` は良い。\n");
        assert!(p.iter().any(|m| m.contains("「Kie」")), "{p:?}");
        assert!(p.iter().any(|m| m.contains("2026-10-21")), "{p:?}");
        assert!(problems("a.md", "Claude Code から使えます。\n").is_empty());
        assert!(!problems("a.md", "AI のスレッドが作ります。\n").is_empty());
    }

    #[test]
    fn records_catch_names_but_not_dates_or_threads() {
        let p = record_problems("docs/plan.md", "- 文字はゴシック体(Kie 2026-10-19)。\n");
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(p[0].contains("「Kie」"), "{p:?}");
        let ok = "- 網は裏のスレッドで作る。2026-10-20 の値: 1%。\n\
                  - 下の帯は「Claude 待ち受け中」。`Kie` と Kie610/x。\n\
                  ```\nKie\n```\n";
        assert!(record_problems("docs/plan.md", ok).is_empty());
    }

    #[test]
    fn long_sentences_are_caught() {
        let long = format!("{}です。\n", "あ".repeat(MAX_CHARS));
        let p = problems("a.md", &long);
        assert!(p.iter().any(|m| m.contains("字(上限")), "{p:?}");
    }
}
