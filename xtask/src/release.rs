//! `cargo xtask release`: アプリを release で作り、実行ファイルにビルドした PC のパスが残っていないかを数える。
//!
//! ```text
//! cargo xtask release [--check] [cargo の引数...]   既定は -p kmb-app。--check は cargo check だけ(数えない)
//! cargo xtask paths <ファイル>...                     ファイルに残るこの PC のパスを数える
//! ```
//!
//! rustc は、依存の crate のソースの場所(`~/.cargo/registry/src/...`)を、パニックの位置(`file!()`・
//! `#[track_caller]`)やデバッグ情報として実行ファイルに書く。そのままでは利用者のホームのフォルダ
//! (ユーザー名を含む)やリポジトリの置き場所が配る exe に入る。ここでは `--remap-path-prefix` で
//! それらの頭を短い名前(`~`・`cargo`・`jc` など)に置き換えてから組み立てる。
//!
//! - cargo の `trim-paths`(profile の設定)は 1.99 でもまだ安定版で使えないので、rustc の引数で渡す。
//! - 引数は `.cargo/config.toml` に書けない(PC ごとにパスが違い、環境変数を展開できない)。debug の
//!   検査には要らず、入れると検査の組み立てまで作り直しになるので、release の組み立ての時だけ
//!   `--config target.'cfg(all())'.rustflags=[...]` で渡す。target の rustflags は利用者の設定の物と
//!   つながる(`build.rustflags` と環境変数 `RUSTFLAGS` を使っている時は、どちらかが効かなくなるので、
//!   組み立てた後に数えて確かめる)。
//! - 同じ引数を `sweep` の release の問い合わせにも渡す(渡さないと別の指紋になり、使っている物を消す)。
//! - C と C++ の `__FILE__` は、同じ頭をコンパイラの引数(`CFLAGS`・`CXXFLAGS`)でも消す(`c_env`)。
//!   SMLib は smlib-sys の build.rs がソースと見出しを相対のパスで渡すので、引数が無くても入らない。
//! - 組み立てた後、exe の横に他者の部品の許諾文の一覧(THIRD-PARTY-LICENSES.txt、licenses.rs)を置く。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// 置き換えるパスの頭と、置き換え先の名前。後ろほど優先する(rustc は合う物のうち最後を使う)。
fn prefixes(root: &Path, target: &Path) -> Vec<(PathBuf, &'static str)> {
    let env = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let home = env("USERPROFILE").or_else(|| env("HOME"));
    let mut v = Vec::new();
    if let Some(h) = &home {
        v.push((h.clone(), "~"));
    }
    if let Some(r) = env("RUSTUP_HOME").or_else(|| home.as_ref().map(|h| h.join(".rustup"))) {
        v.push((r, "rustup"));
    }
    if let Some(c) = env("CARGO_HOME").or_else(|| home.as_ref().map(|h| h.join(".cargo"))) {
        v.push((c, "cargo"));
    }
    v.push((root.to_path_buf(), "jc"));
    if !target.starts_with(root) {
        v.push((target.to_path_buf(), "target"));
    }
    // 同じ場所が別の書き方(Windows の \\?\ の付く形など)で渡されても置き換わるよう、両方を並べる
    let mut out = Vec::new();
    for (p, to) in v {
        let canon = fs::canonicalize(&p).ok().map(|c| strip_verbatim(&c));
        out.push((p.clone(), to));
        if let Some(c) = canon.filter(|c| *c != p) {
            out.push((c, to));
        }
    }
    out
}

/// Windows の `\\?\C:\...` を `C:\...` にする。
fn strip_verbatim(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => p.to_path_buf(),
    }
}

/// release の組み立てに足す cargo の引数(`--config target.'cfg(all())'.rustflags=[...]`)。
pub fn cargo_config(root: &Path, target: &Path) -> Vec<String> {
    let flags: Vec<String> = prefixes(root, target)
        .iter()
        .map(|(from, to)| {
            // TOML の文字列は JSON の文字列と同じ書き方で書ける(\ と " を逃がす)
            serde_json::Value::String(format!("--remap-path-prefix={}={to}", from.display()))
                .to_string()
        })
        .collect();
    vec![
        "--config".into(),
        format!("target.'cfg(all())'.rustflags=[{}]", flags.join(",")),
    ]
}

/// release の組み立てに足す、C と C++ のコンパイラの引数(cc が読む環境変数 `CFLAGS`・`CXXFLAGS` に足す)。
///
/// cc で作る C の依存(lzma-sys の xz など)の `__FILE__`(assert の文)は、渡されたソースと見出しの
/// パスのまま入る。rustc の置き換えは C に効かないので、コンパイラにも同じ頭を消させる。MSVC は
/// `/d1trimfile:<頭>\`(`__FILE__` の頭を切る。VS 2019 から)、gcc・clang は `-ffile-prefix-map`。
/// cc は環境変数を空白で区切るので、空白を含むパスは渡さない(残れば数えた件数で分かる)。
pub fn c_env(root: &Path, target: &Path) -> Vec<(String, String)> {
    let flags: Vec<String> = prefixes(root, target)
        .iter()
        .map(|(from, to)| {
            (
                from.to_string_lossy()
                    .trim_end_matches(['/', '\\'])
                    .to_string(),
                to,
            )
        })
        .filter(|(from, _)| !from.is_empty() && !from.contains(char::is_whitespace))
        .map(|(from, to)| {
            if cfg!(target_env = "msvc") {
                format!("/d1trimfile:{from}\\")
            } else {
                format!("-ffile-prefix-map={from}={to}")
            }
        })
        .collect();
    ["CFLAGS", "CXXFLAGS"]
        .into_iter()
        .map(|k| {
            let mut v: Vec<String> = std::env::var(k).ok().into_iter().collect();
            v.extend(flags.iter().cloned());
            (k.to_string(), v.join(" "))
        })
        .collect()
}

/// 実行ファイルの中から探す、この PC のパス(ホーム・リポジトリ・cargo と rustup の置き場所・target)。
fn needles(root: &Path, target: &Path) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for (p, _) in prefixes(root, target) {
        let s = p
            .to_string_lossy()
            .trim_end_matches(['/', '\\'])
            .to_string();
        // "/" や "C:" のような短い物は、関係のない所にも合うので探さない
        if s.len() < 4 {
            continue;
        }
        for t in [s.clone(), s.replace('\\', "/"), s.replace('/', "\\")] {
            let t = t.to_ascii_lowercase();
            if !v.contains(&t) {
                v.push(t);
            }
        }
    }
    v
}

/// 見つかった 1 か所(バイトの位置と、その周りの読める文字列)。
#[derive(Debug, PartialEq)]
pub struct Hit {
    pub at: usize,
    pub text: String,
}

/// `bytes` の中の `needles`(小文字で渡す)の出どころを数える。大文字と小文字は区別しない(Windows の
/// パスは区別しない)。UTF-8 と UTF-16LE(C++ の `_T(__FILE__)` など)の両方を探す。
/// 1 つの文字列に複数の頭が重なって合っても(ホームとその下のリポジトリなど)1 か所に数える。
pub fn scan(bytes: &[u8], needles: &[String]) -> Vec<Hit> {
    let hay = bytes.to_ascii_lowercase();
    let mut spans: Vec<(usize, usize, bool)> = Vec::new();
    for n in needles {
        let utf8 = n.as_bytes().to_vec();
        let utf16: Vec<u8> = n.encode_utf16().flat_map(u16::to_le_bytes).collect();
        for (pat, wide) in [(utf8, false), (utf16, true)] {
            if pat.is_empty() {
                continue;
            }
            let mut i = 0;
            while let Some(k) = find(&hay[i..], &pat) {
                spans.push((i + k, i + k + pat.len(), wide));
                i += k + 1;
            }
        }
    }
    spans.sort();
    let mut hits: Vec<Hit> = Vec::new();
    let mut end = 0;
    for (s, e, wide) in spans {
        if !hits.is_empty() && s < end {
            end = end.max(e);
            continue;
        }
        end = e;
        hits.push(Hit {
            at: s,
            text: around(bytes, s, wide),
        });
    }
    hits
}

fn find(hay: &[u8], pat: &[u8]) -> Option<usize> {
    hay.windows(pat.len()).position(|w| w == pat)
}

/// 見つかった所を含む、読める文字の並び(見本として見せる。長すぎる物は切る)。
fn around(bytes: &[u8], at: usize, wide: bool) -> String {
    let step = if wide { 2 } else { 1 };
    let readable = |i: usize| {
        bytes.get(i).is_some_and(|b| (0x20..0x7f).contains(b))
            && (!wide || bytes.get(i + 1) == Some(&0))
    };
    let mut s = at;
    while s >= step && readable(s - step) {
        s -= step;
    }
    let mut e = at;
    while readable(e) {
        e += step;
    }
    let text: String = (s..e).step_by(step).map(|i| bytes[i] as char).collect();
    text.chars().take(200).collect()
}

/// 数えた結果を出す。見本は最初の数か所だけ。
fn report(label: &str, file: &Path, hits: &[Hit]) {
    println!(
        "{label}: {} に、この PC のパスが {} 件",
        file.display(),
        hits.len()
    );
    for h in hits.iter().take(5) {
        println!("  0x{:08x}: {}", h.at, h.text);
    }
}

fn read(file: &Path) -> Result<Vec<u8>, String> {
    fs::read(file).map_err(|e| format!("{} を読めません: {e}", file.display()))
}

/// `cargo xtask paths <ファイル>...`。どれかに 1 件でもあれば失敗にする。
pub fn paths(root: &Path, target: &Path, files: &[String]) -> Result<ExitCode, String> {
    if files.is_empty() {
        return Err("使い方: cargo xtask paths <ファイル>...".into());
    }
    let n = needles(root, target);
    let mut total = 0;
    for f in files {
        let hits = scan(&read(Path::new(f))?, &n);
        report("パス", Path::new(f), &hits);
        total += hits.len();
    }
    Ok(if total == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

/// `cargo xtask release [--check] [cargo の引数...]`。
pub fn run(
    root: &Path,
    target: &Path,
    args: &[String],
    cargo: &dyn Fn() -> Command,
) -> Result<ExitCode, String> {
    let check = args.iter().any(|a| a == "--check");
    let mut rest: Vec<String> = args.iter().filter(|a| *a != "--check").cloned().collect();
    if !rest.iter().any(|a| a == "-p" || a.starts_with("--package")) {
        rest.extend(["-p".into(), "kmb-app".into()]);
    }
    let exe = target.join("release").join(if cfg!(windows) {
        "kie-monobench.exe"
    } else {
        "kie-monobench"
    });
    let n = needles(root, target);
    let app = rest.windows(2).any(|w| w[0] == "-p" && w[1] == "kmb-app");
    // 作り直す前の exe を数えておき、直した後と並べる
    let before = (!check && app && exe.is_file())
        .then(|| read(&exe).map(|b| scan(&b, &n)))
        .transpose()?;
    if let Some(h) = &before {
        report("前", &exe, h);
    }
    let mut c = cargo();
    c.arg(if check { "check" } else { "build" })
        .arg("--release")
        .args(cargo_config(root, target))
        .envs(c_env(root, target))
        .args(&rest);
    println!("== {c:?}");
    let st = c
        .status()
        .map_err(|e| format!("{c:?} を動かせません: {e}"))?;
    if !st.success() {
        return Ok(ExitCode::FAILURE);
    }
    if check {
        return Ok(ExitCode::SUCCESS);
    }
    // 配る物の横に、他者の部品の許諾文の一覧を置く(licenses.rs)
    let list = target.join("release").join(crate::licenses::FILE);
    let k = crate::licenses::write(root, crate::licenses::host_target(), &list, cargo)?;
    println!("書いた: {}(他者の crate {k} 件)", list.display());
    if !app {
        return Ok(ExitCode::SUCCESS);
    }
    let after = scan(&read(&exe)?, &n);
    report("後", &exe, &after);
    println!(
        "パス: 前 {} 件 → 後 {} 件",
        before.map_or("(exe なし)".to_string(), |h| h.len().to_string()),
        after.len()
    );
    Ok(if after.is_empty() {
        ExitCode::SUCCESS
    } else {
        println!("exe にこの PC のパスが残っています。出どころは上の見本(docs/verify.md の 12 節)");
        ExitCode::FAILURE
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_counts_utf8_and_utf16_ignoring_case() {
        let mut b = b"\0\0C:\\Users\\Dev\\.cargo\\registry\\src\\serde\\lib.rs\0xx".to_vec();
        b.extend(
            "c:\\users\\dev\\a.cpp"
                .encode_utf16()
                .flat_map(u16::to_le_bytes),
        );
        b.extend(b"\0\0D:\\other\\place\0");
        let hits = scan(&b, &[r"c:\users\dev".to_string()]);
        assert_eq!(hits.len(), 2);
        assert_eq!(
            hits[0].text,
            r"C:\Users\Dev\.cargo\registry\src\serde\lib.rs"
        );
        assert_eq!(hits[1].text, r"c:\users\dev\a.cpp");
    }

    #[test]
    fn overlapping_prefixes_count_once() {
        let b = b"\0/home/dev/repo/crates/x.rs\0/home/dev/.cargo/y.rs\0";
        let n = vec!["/home/dev".to_string(), "/home/dev/repo".to_string()];
        let hits = scan(b, &n);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].text, "/home/dev/repo/crates/x.rs");
    }

    #[test]
    fn config_is_a_toml_array_with_escaped_paths() {
        let args = cargo_config(Path::new(r"D:\w\jc"), Path::new(r"D:\w\jc\target"));
        assert_eq!(args[0], "--config");
        let v: toml::Table = toml::from_str(&args[1]).expect("TOML として読める");
        let flags = v["target"]["cfg(all())"]["rustflags"].as_array().unwrap();
        assert!(
            flags
                .iter()
                .any(|f| f.as_str() == Some(r"--remap-path-prefix=D:\w\jc=jc")),
            "{flags:?}"
        );
    }

    #[test]
    fn c_flags_strip_the_same_prefixes() {
        let env = c_env(Path::new("/w/jc"), Path::new("/w/jc/target"));
        let cflags = &env.iter().find(|(k, _)| k == "CFLAGS").unwrap().1;
        let want = if cfg!(target_env = "msvc") {
            "/d1trimfile:/w/jc\\"
        } else {
            "-ffile-prefix-map=/w/jc=jc"
        };
        assert!(cflags.split(' ').any(|f| f == want), "{cflags}");
        assert!(env.iter().any(|(k, _)| k == "CXXFLAGS"));
    }

    #[test]
    fn verbatim_prefix_is_dropped() {
        assert_eq!(
            strip_verbatim(Path::new(r"\\?\C:\a")),
            PathBuf::from(r"C:\a")
        );
        assert_eq!(strip_verbatim(Path::new("/a/b")), PathBuf::from("/a/b"));
    }
}
