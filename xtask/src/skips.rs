//! 飛ばしの点検: 理由の無い飛ばしと、黙って消えたテストを落とす。
//!
//! nextest は `#[ignore]` だけを「飛ばし」に数え、テストの中で環境を見て `return` した物は「通過」に
//! 数える。そのため「飛ばし 0」でも確かめていないテストがあり得た。ここでは組み立てずに字面で:
//!
//! 1. `#[ignore]` に理由(`#[ignore = "…"]`)が無い物を落とす。
//! 2. `#[ignore]` でないテストの本体に `return` がある物を落とす(環境が無い時に黙って通過する
//!    形)。実行中に飛ばす時は `kmb_contract::skip!("理由")` を使う(印を書いてから return する。
//!    xtask が数えて「飛ばし(実行中)」に分ける)。例外は `xtask/skips.toml` の `[static]`。
//! 3. `--base` との分かれ目から消えたテストを落とす。消してよいのは `xtask/removed-tests.toml` に
//!    日付と理由を書いた物だけ(Kie の確認は要らない。消した物は区切りの報告に一覧で書く)。
//!    同じ名前のテストが別のファイルや別の crate へ移っただけの物は消えたことにしない。
//!
//! 4. 決まった回数だけ回して、待つ物が揃わなくても黙って先へ進む待ちを落とす(`for _ in 0..n` の中で
//!    `sleep` し、揃えば値を持たない `return;`・`break;` で抜け、回し終えた後に誤りにする文が無い物)。裏で読む物を
//!    待ちきれずに先へ進み、負荷の高い時だけ別の所で落ちる揺れの元になる。時間切れで誤りにして止める
//!    (例: `crates/kmb-ui-library/src/tests.rs` の `settle`)。見るのはテストのファイルと `#[cfg(test)]` の中。
//!
//! 実行中の飛ばし(`skip!` の印)は、`JC_SKIP_LOG` のファイルに 1 行ずつ書かれる。`skips.toml` の
//! `[runtime]` に無い物が出たら落とす。
//!
//! 字面の点検なので、マクロが作るテスト(`kernel_contract_tests!` など)は数えない。

use crate::Package;
use crate::source::{Source, rust_files, test_fns};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

/// テスト 1 つの名前(`crate::ファイル::関数`。ファイルは crate の中の相対パス)。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TestName {
    pub krate: String,
    pub file: String,
    pub name: String,
}

impl std::fmt::Display for TestName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}::{}", self.krate, self.file, self.name)
    }
}

/// `xtask/skips.toml`。
#[derive(Debug, Default)]
pub struct Allowed {
    /// 本体に return を持ってよいテスト(`crate 関数` → 理由)
    pub r#static: BTreeMap<String, String>,
    /// 実行中に飛ばしてよいテスト(`skip!` の印。テストの名前の終わり → 理由)
    pub runtime: BTreeMap<String, String>,
    /// 消してよいテスト(`crate 関数` → 日付と理由)
    pub removed: BTreeMap<String, String>,
}

fn table(text: &str, file: &str) -> Result<BTreeMap<String, BTreeMap<String, String>>, String> {
    let v: toml::Table = text
        .parse()
        .map_err(|e| format!("{file} が読めない: {e}"))?;
    let mut out = BTreeMap::new();
    for (sec, items) in v {
        let Some(items) = items.as_table() else {
            return Err(format!("{file} の {sec} は表でない"));
        };
        let mut m = BTreeMap::new();
        for (k, why) in items {
            let why = why.as_str().unwrap_or_default().trim().to_string();
            if why.is_empty() {
                return Err(format!("{file} の [{sec}] {k} に理由が無い"));
            }
            m.insert(k.clone(), why);
        }
        out.insert(sec, m);
    }
    Ok(out)
}

pub fn allowed(root: &Path) -> Result<Allowed, String> {
    let read = |f: &str| fs::read_to_string(root.join(f)).unwrap_or_default();
    let mut s = table(&read("xtask/skips.toml"), "xtask/skips.toml")?;
    let mut r = table(
        &read("xtask/removed-tests.toml"),
        "xtask/removed-tests.toml",
    )?;
    Ok(Allowed {
        r#static: s.remove("static").unwrap_or_default(),
        runtime: s.remove("runtime").unwrap_or_default(),
        removed: r.remove("removed").unwrap_or_default(),
    })
}

/// 1 つのファイルのテストと、そのうち問題のある物。
fn file_tests(krate: &str, file: &str, text: &str) -> Vec<(TestName, usize, Problem, bool)> {
    let src = Source::new(text);
    let mut out = vec![];
    for t in test_fns(&src) {
        let body = &src.code[t.body.0..t.body.1];
        let problem = match &t.ignore {
            Some(why) if why.trim().is_empty() => Problem::IgnoreWithoutReason,
            Some(_) => Problem::None,
            None if has_return(body) && !body.contains("skip!") => Problem::SilentReturn,
            None => Problem::None,
        };
        out.push((
            TestName {
                krate: krate.into(),
                file: file.into(),
                name: t.name,
            },
            t.line,
            problem,
            t.ignore.is_some(),
        ));
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Problem {
    None,
    IgnoreWithoutReason,
    SilentReturn,
}

/// 本体の `return`。入れ子の閉包(`|x| { … }`)と関数(`fn f() { … }`)の中の物は、テストを
/// 終える物でないので数えない(字面の点検)。
fn has_return(body: &str) -> bool {
    let b = body.as_bytes();
    // 閉包・関数の本体の範囲
    let mut inner: Vec<(usize, usize)> = vec![];
    for (i, &c) in b.iter().enumerate().skip(1) {
        if c != b'{' {
            continue;
        }
        let seg_start = body[..i].rfind([';', '{', '}']).map_or(0, |p| p + 1);
        let seg = &body[seg_start..i];
        let after_bar = seg.rfind('|').map(|p| seg[p + 1..].trim());
        let closure = after_bar.is_some_and(|a| a.is_empty() || a.starts_with("->"));
        let nested_fn = seg
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .any(|w| w == "fn");
        if closure || nested_fn {
            inner.push((i, block_end(b, i)));
        }
    }
    body.match_indices("return").any(|(at, _)| {
        let before = at == 0 || !(b[at - 1].is_ascii_alphanumeric() || b[at - 1] == b'_');
        let after = b
            .get(at + 6)
            .is_none_or(|c| !(c.is_ascii_alphanumeric() || *c == b'_'));
        before && after && !inner.iter().any(|&(s, e)| s < at && at < e)
    })
}

fn block_end(b: &[u8], open: usize) -> usize {
    let mut depth = 0usize;
    for (i, &c) in b.iter().enumerate().skip(open) {
        match c {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return i;
                }
            }
            _ => {}
        }
    }
    b.len()
}

/// テストのコードの中の、黙って抜ける待ち(4)の行。テストのファイル(名前に test を含むか、
/// tests・benches・examples の下)は全部を、ほかは `#[cfg(test)]` の範囲だけを見る。
fn silent_waits(file: &str, text: &str) -> Vec<usize> {
    let src = Source::new(text);
    let whole =
        !file.starts_with("src/") || file.rsplit('/').next().is_some_and(|n| n.contains("test"));
    let ranges = if whole {
        vec![(0, src.code.len())]
    } else {
        src.test_ranges()
    };
    wait_loops(&src.code)
        .into_iter()
        .filter(|at| ranges.iter().any(|&(a, b)| a <= *at && *at < b))
        .map(|at| src.line_of(at))
        .collect()
}

/// `for … in …..… { … sleep(…) … return; か break; … }` で、ループの直後が誤りにする文でない物の位置。
fn wait_loops(code: &str) -> Vec<usize> {
    let b = code.as_bytes();
    // 値を持たずに抜ける `return;`・`break;`(値を返す物は、回し終えた時も呼び手が値で分かる)
    let bare = |body: &str, w: &str| {
        let bb = body.as_bytes();
        body.match_indices(w).any(|(i, _)| {
            (i == 0 || !is_ident(bb[i - 1])) && body[i + w.len()..].trim_start().starts_with(';')
        })
    };
    let mut out = vec![];
    for (at, _) in code.match_indices("for") {
        if (at > 0 && is_ident(b[at - 1])) || !b.get(at + 3).is_some_and(u8::is_ascii_whitespace) {
            continue;
        }
        let Some(open) = code[at..].find('{').map(|o| at + o) else {
            continue;
        };
        let head = &code[at..open];
        if !head.contains(" in ") || !head.contains("..") {
            continue;
        }
        let end = block_end(b, open);
        let body = &code[open..end];
        if !body.contains("sleep(") || !(bare(body, "return") || bare(body, "break")) {
            continue;
        }
        let next = code.get(end + 1..).unwrap_or_default().trim_start();
        if ["panic!", "unreachable!", "assert"]
            .iter()
            .any(|w| next.starts_with(w))
        {
            continue;
        }
        out.push(at);
    }
    out
}

fn is_ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// crate のテストを持ち得るファイル(src・tests・benches・examples)。
fn test_files(root: &Path, dir: &str) -> Vec<String> {
    let base = root.join(dir);
    ["src", "tests", "benches", "examples"]
        .iter()
        .flat_map(|d| rust_files(&base.join(d)))
        .map(|f| {
            f.strip_prefix(&base)
                .unwrap_or(&f)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

/// 点検の結果。
#[derive(Debug, Default)]
pub struct Report {
    pub problems: Vec<String>,
    /// `#[ignore]` の数
    pub ignored: usize,
    /// 全部のテストの数(字面で数えた物)
    pub tests: usize,
}

/// 今のソースの飛ばしの点検(1・2)。
pub fn check(root: &Path, packages: &[Package], allow: &Allowed) -> Report {
    let mut r = Report::default();
    let mut used = BTreeSet::new();
    for p in packages {
        for file in test_files(root, &p.dir) {
            let Ok(text) = fs::read_to_string(root.join(&p.dir).join(&file)) else {
                continue;
            };
            for line in silent_waits(&file, &text) {
                r.problems.push(format!(
                    "{}/{file}:{line} の待ちは決まった回数を回し終えると黙って先へ進む。時間切れで誤りにして止める\
                     (assert! か panic!。例: crates/kmb-ui-library/src/tests.rs の settle)",
                    p.dir
                ));
            }
            for (t, line, problem, ignored) in file_tests(&p.name, &file, &text) {
                r.tests += 1;
                r.ignored += usize::from(ignored);
                let key = format!("{} {}", t.krate, t.name);
                match problem {
                    Problem::None => {}
                    Problem::IgnoreWithoutReason => r.problems.push(format!(
                        "{}/{}:{line} {} の #[ignore] に理由が無い(#[ignore = \"道具: …\"] のように書く)",
                        p.dir, t.file, t.name
                    )),
                    Problem::SilentReturn if allow.r#static.contains_key(&key) => {
                        used.insert(key);
                    }
                    Problem::SilentReturn => r.problems.push(format!(
                        "{}/{}:{line} {} が本体で return している。環境が無くて飛ばすなら \
                         kmb_contract::skip!(\"理由\") を使う(黙って通過に数えない)。道具なら \
                         #[ignore = \"…\"]。そのままにするなら xtask/skips.toml の [static] に理由を書く",
                        p.dir, t.file, t.name
                    )),
                }
            }
        }
    }
    for k in allow.r#static.keys() {
        if !used.contains(k) {
            r.problems.push(format!(
                "{k} はもう本体で return していないか無い。xtask/skips.toml の [static] から消す"
            ));
        }
    }
    r
}

/// `base` との分かれ目から消えたテスト(3)。変えたファイルだけを、分かれ目の版と今の版で比べる。
/// 同じ名前のテストが別のファイルや別の crate へ移っただけなら消えたことにしない(crate を分ける
/// 時に試験も一緒に移すため。Kie 2026-10-22)。丸ごと消した crate のファイルも数える。
pub fn removed(
    root: &Path,
    packages: &[Package],
    fork: &str,
    changed: &[String],
    allow: &Allowed,
) -> Vec<String> {
    let dirs: Vec<(&str, &str)> = packages
        .iter()
        .map(|p| (p.name.as_str(), p.dir.as_str()))
        .collect();
    let files: Vec<(String, String, String)> = changed
        .iter()
        .filter(|f| f.ends_with(".rs"))
        .filter_map(|f| owner(&dirs, f).map(|(k, d)| (k, d, f.clone())))
        .collect();
    let old = git_show_all(root, fork, files.iter().map(|(.., f)| f.as_str()));
    let mut count: BTreeMap<(String, String), i64> = BTreeMap::new();
    for ((krate, dir, f), before) in files.iter().zip(old) {
        let rel = &f[dir.len() + 1..];
        for (t, ..) in file_tests(krate, rel, &before.unwrap_or_default()) {
            *count.entry((t.krate, t.name)).or_default() += 1;
        }
        let now = fs::read_to_string(root.join(f)).unwrap_or_default();
        for (t, ..) in file_tests(krate, rel, &now) {
            *count.entry((t.krate, t.name)).or_default() -= 1;
        }
    }
    gone(count)
        .into_iter()
        .filter(|(k, n)| !allow.removed.contains_key(&format!("{k} {n}")))
        .map(|(k, n)| {
            format!(
                "{k} のテスト {n} が {fork} から消えている。消すなら \
                 xtask/removed-tests.toml の [removed] に「\"{k} {n}\" = \"日付と理由\"」を書く"
            )
        })
        .collect()
}

/// ファイルの持ち主の crate(名前とフォルダ)。今のワークスペースに無いフォルダ(丸ごと消した
/// crate)は、`crates/<名前>/` などのフォルダの名前を crate の名前とする。
fn owner(dirs: &[(&str, &str)], f: &str) -> Option<(String, String)> {
    if let Some((k, d)) = dirs
        .iter()
        .filter(|(_, d)| f.starts_with(&format!("{d}/")))
        .max_by_key(|(_, d)| d.len())
    {
        return Some((k.to_string(), d.to_string()));
    }
    let mut parts = f.splitn(3, '/');
    let (top, name, rest) = (parts.next()?, parts.next()?, parts.next()?);
    (["crates", "apps", "plugins"].contains(&top) && !rest.is_empty())
        .then(|| (name.to_string(), format!("{top}/{name}")))
}

/// crate と名前ごとの(前の数 − 今の数)から、どこにも残っていないテストを選ぶ。増えた側の同じ
/// 名前で減った側を打ち消す(別の crate へ移っただけの物)。
fn gone(count: BTreeMap<(String, String), i64>) -> Vec<(String, String)> {
    let mut gained: BTreeMap<String, i64> = BTreeMap::new();
    for ((_, n), c) in &count {
        if *c < 0 {
            *gained.entry(n.clone()).or_default() -= c;
        }
    }
    let mut out = Vec::new();
    for ((k, n), c) in count {
        if c <= 0 {
            continue;
        }
        let g = gained.entry(n.clone()).or_default();
        let moved = c.min(*g);
        *g -= moved;
        if c > moved {
            out.push((k, n));
        }
    }
    out
}

/// `git cat-file --batch` で、版 `rev` のファイルを順に読む(無い物は `None`)。
fn git_show_all<'a>(
    root: &Path,
    rev: &str,
    files: impl Iterator<Item = &'a str>,
) -> Vec<Option<String>> {
    let files: Vec<&str> = files.collect();
    if files.is_empty() {
        return vec![];
    }
    let input: String = files.iter().map(|f| format!("{rev}:{f}\n")).collect();
    let out = (|| {
        let mut child = Command::new("git")
            .args(["cat-file", "--batch"])
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .ok()?;
        let mut stdin = child.stdin.take()?;
        let writer = std::thread::spawn(move || {
            use std::io::Write;
            let _ = stdin.write_all(input.as_bytes());
        });
        let o = child.wait_with_output().ok()?;
        let _ = writer.join();
        Some(o.stdout)
    })()
    .unwrap_or_default();
    let mut res = vec![];
    let mut i = 0;
    for _ in &files {
        let Some(nl) = out[i..].iter().position(|&c| c == b'\n') else {
            res.push(None);
            continue;
        };
        let head = String::from_utf8_lossy(&out[i..i + nl]).into_owned();
        i += nl + 1;
        if head.ends_with(" missing") {
            res.push(None);
            continue;
        }
        let size: usize = head
            .rsplit(' ')
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let end = (i + size).min(out.len());
        res.push(Some(String::from_utf8_lossy(&out[i..end]).into_owned()));
        i = end + 1;
    }
    res
}

/// 実行中の飛ばしの印(`JC_SKIP_LOG` の各行 `テストの名前\t理由`)と、表に無い物。
pub fn runtime(log: &str, allow: &Allowed) -> (Vec<String>, Vec<String>) {
    let mut seen = vec![];
    let mut problems = vec![];
    for line in log.lines().filter(|l| !l.trim().is_empty()) {
        let (name, why) = line.split_once('\t').unwrap_or((line, ""));
        seen.push(format!("{name}({why})"));
        if !allow
            .runtime
            .keys()
            .any(|k| name == k || name.ends_with(&format!("::{k}")))
        {
            problems.push(format!(
                "テスト {name} が実行中に飛ばした({why})。xtask/skips.toml の [runtime] に無い"
            ));
        }
    }
    (seen, problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_that_give_up_silently_after_a_fixed_count_are_problems() {
        let text = "\
fn settle(h: &mut H) {\n    for _ in 0..200 {\n        if h.ready() {\n            return;\n        }\n        \
std::thread::sleep(D);\n        h.run();\n    }\n}\n\
fn until_done(h: &mut H) {\n    for i in 0..n { if h.ready() { break; } sleep(D); }\n    assert!(h.ready(), \"揃わない\");\n}\n\
fn deadline(h: &mut H) {\n    for _ in 0..50 { if h.ready() { return; } sleep(D); }\n    panic!(\"揃わない\");\n}\n\
fn hold(h: &mut H) {\n    for _ in 0..6 { h.step(); std::thread::sleep(D); }\n}\n\
fn slow_task(c: &C) -> R {\n    for i in 0..200 { if c.cancelled() { return Ok(i); } sleep(D); }\n    Ok(200)\n}\n\
fn scan(v: &[u8]) {\n    for x in v { if *x == 0 { break; } }\n    // for _ in 0..9 { sleep(D); return; }\n}\n";
        // 回し終えた後に何もしない 1 つ目だけ。後で確かめる物・決まった時間だけ押す物・値を返して抜ける物・
        // sleep の無い物・コメントの中は数えない
        assert_eq!(silent_waits("src/tests.rs", text), [2]);
        // テストでないファイルは #[cfg(test)] の中だけ
        assert!(silent_waits("src/lib.rs", text).is_empty());
        let in_cfg = format!("fn a() {{}}\n#[cfg(test)]\nmod tests {{\n{text}}}\n");
        assert_eq!(silent_waits("src/lib.rs", &in_cfg), [5]);
        assert_eq!(silent_waits("tests/e2e.rs", text), [2]);
    }

    #[test]
    fn ignores_without_a_reason_and_silent_returns_are_problems() {
        let text = "\
#[test]\n#[ignore]\nfn tool() {}\n\
#[test]\n#[ignore = \"道具: 絵を書く\"]\nfn tool2() { return; }\n\
#[test]\nfn gpu() { let Some(g) = gpu() else { eprintln!(\"no\"); return; }; }\n\
#[test]\nfn marked() { if no_gpu() { kmb_contract::skip!(\"GPU が無い\"); } }\n\
#[test]\nfn returns_later() { let r = 1; r.returned(); }\n\
#[test]\nfn closures() { let f = |x: u8| { if x > 1 { return 2; } x }; let g = move || -> u8 { return 1 }; \
fn h() -> u8 { return 3; } if a || b { f(1); } }\n\
#[test]\nfn loop_return() { loop { if done() { return; } } }\n";
        let t = file_tests("kmb-x", "src/a.rs", text);
        let got: Vec<(&str, Problem)> =
            t.iter().map(|(t, _, p, _)| (t.name.as_str(), *p)).collect();
        assert_eq!(
            got,
            [
                ("tool", Problem::IgnoreWithoutReason),
                ("tool2", Problem::None),
                ("gpu", Problem::SilentReturn),
                ("marked", Problem::None),
                ("returns_later", Problem::None),
                ("closures", Problem::None),
                ("loop_return", Problem::SilentReturn),
            ]
        );
    }

    #[test]
    fn runtime_skips_must_be_listed() {
        let mut a = Allowed::default();
        a.runtime.insert("gpu_pick".into(), "GPU の無い所".into());
        let (seen, bad) = runtime("tests::gpu_pick\tGPU が無い\nother\t理由\n", &a);
        assert_eq!(seen.len(), 2);
        assert_eq!(bad.len(), 1);
        assert!(bad[0].contains("other"));
    }

    #[test]
    fn a_test_moved_to_another_crate_is_not_gone() {
        let c = |v: &[(&str, &str, i64)]| -> BTreeMap<(String, String), i64> {
            v.iter()
                .map(|(k, n, c)| ((k.to_string(), n.to_string()), *c))
                .collect()
        };
        // kmb-a から kmb-b へ移した solve、本当に消した old、2 つ消して 1 つだけ移した twice
        let got = gone(c(&[
            ("kmb-a", "solve", 1),
            ("kmb-b", "solve", -1),
            ("kmb-a", "old", 1),
            ("kmb-a", "twice", 2),
            ("kmb-c", "twice", -1),
        ]));
        let want = [("kmb-a", "old"), ("kmb-a", "twice")];
        assert_eq!(
            got,
            want.map(|(k, n)| (k.to_string(), n.to_string())).to_vec()
        );
        // 足しただけの物は消えたことにならない
        assert!(gone(c(&[("kmb-b", "new", -1)])).is_empty());
    }

    #[test]
    fn files_of_a_removed_crate_still_have_an_owner() {
        let dirs = [
            ("kmb-sketch", "crates/kmb-sketch"),
            ("kmb-sketch-solver", "crates/kmb-sketch-solver"),
        ];
        let o = |f: &str| owner(&dirs, f);
        assert_eq!(
            o("crates/kmb-sketch-solver/src/lib.rs"),
            Some((
                "kmb-sketch-solver".into(),
                "crates/kmb-sketch-solver".into()
            ))
        );
        assert_eq!(
            o("crates/kmb-old/src/lib.rs"),
            Some(("kmb-old".into(), "crates/kmb-old".into()))
        );
        assert_eq!(o("xtask/src/main.rs"), None);
    }

    #[test]
    fn tables_need_a_reason_for_every_line() {
        assert!(table("[static]\n\"kmb-x f\" = \"\"\n", "f").is_err());
        let t = table("[static]\n\"kmb-x f\" = \"GPU\"\n", "f").unwrap();
        assert_eq!(t["static"]["kmb-x f"], "GPU");
    }
}
