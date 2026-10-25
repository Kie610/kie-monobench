//! target の中の、今の版では使わない成果物を消す(`cargo xtask sweep`)。
//!
//! cargo は、依存の機能・プロファイル・Rust の版・ビルドスクリプトなどが変わるたびに、別の
//! 名前(`<crate>-<16 桁の指紋>`)で作り直し、古いものを消さない。今使う組み立てを
//! `--message-format json` で問い合わせ、そこに出てくる指紋だけを残して、ほかを消す。
//! 問い合わせる組み立ては検査と同じもの(テストと clippy、あればリリースのアプリ)なので、
//! 消した後の次のビルドは遅くならない。
//!
//! - incremental(自前の crate のコンパイルの途中結果)は名前から指紋が分からないので、
//!   `--keep-days` 日(既定 3)より前から使っていないフォルダを消す。消したものは、その crate
//!   を次にコンパイルする時に 1 回だけ全部をコンパイルし直す。使っているフォルダでも、rustc が
//!   残すいちばん新しい回より前の回(次のコンパイルでは読まない)は消す。
//! - 既定は消さずに見積もりを出すだけ。`--yes` で消す。

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use serde_json::Value as Json;

#[derive(Debug, Default, PartialEq)]
pub struct SweepOptions {
    pub yes: bool,
    pub keep_days: u64,
}

pub fn parse(args: &[String]) -> Result<SweepOptions, String> {
    let mut o = SweepOptions {
        keep_days: 3,
        ..Default::default()
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--yes" => o.yes = true,
            "--keep-days" => {
                o.keep_days = it
                    .next()
                    .and_then(|d| d.parse().ok())
                    .ok_or("--keep-days の後に日数を書いてください")?
            }
            s => return Err(format!("知らない指定です: {s}")),
        }
    }
    Ok(o)
}

/// 名前の終わりの `-<16 桁の 16 進>`(拡張子の前)。`libjc_ui-0123456789abcdef.rlib` → その 16 桁。
pub fn fingerprint_of(name: &str) -> Option<&str> {
    let stem = name.split('.').next().unwrap_or(name);
    let (_, h) = stem.rsplit_once('-')?;
    (h.len() == 16 && h.bytes().all(|b| b.is_ascii_hexdigit())).then_some(h)
}

/// 今の組み立てが使うもの。
#[derive(Debug, Default, PartialEq)]
pub struct Keep {
    /// 名前の指紋
    pub hashes: BTreeSet<String>,
    /// 名前に指紋の付かない実行ファイルの、`.fingerprint/<名前>-<指紋>/` の中の印(`bin-xtask`)。
    /// Windows(MSVC)の cargo は実行ファイルの名前に指紋を付けない(PDB の場所が実行ファイルに
    /// 埋め込まれるため)ので、名前からは `.fingerprint` のどれを使うか分からない。印のあるものは
    /// 版の古いものも含めて残す(1 つ数 KB)
    pub targets: BTreeSet<String>,
}

impl Keep {
    fn extend(&mut self, other: Keep) {
        self.hashes.extend(other.hashes);
        self.targets.extend(other.targets);
    }
}

/// `cargo ... --message-format json` の出力から、使っている指紋を集める。作り直した crate も返す。
pub fn fingerprints_in(json_lines: &str) -> (Keep, Vec<String>) {
    let mut keep = Keep::default();
    let mut rebuilt = Vec::new();
    for line in json_lines.lines() {
        let Ok(m) = serde_json::from_str::<Json>(line) else {
            continue;
        };
        let mut paths: Vec<&str> = Vec::new();
        let mark = format!(
            "{}-{}",
            m["target"]["kind"][0].as_str().unwrap_or_default(),
            m["target"]["name"].as_str().unwrap_or_default()
        );
        match m["reason"].as_str() {
            Some("compiler-artifact") => {
                if m["fresh"] == Json::Bool(false) {
                    rebuilt.push(format!(
                        "{}({})",
                        m["target"]["name"].as_str().unwrap_or("?"),
                        m["target"]["kind"][0].as_str().unwrap_or("?")
                    ));
                }
                paths.extend(
                    m["filenames"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Json::as_str),
                );
                paths.extend(m["executable"].as_str());
            }
            Some("build-script-executed") => paths.extend(m["out_dir"].as_str()),
            _ => {}
        }
        for p in paths {
            let mut found = false;
            for c in Path::new(p).components() {
                if let Some(h) = fingerprint_of(&c.as_os_str().to_string_lossy()) {
                    keep.hashes.insert(h.to_string());
                    found = true;
                }
            }
            if !found {
                let h = uplifted(Path::new(p));
                if h.is_empty() {
                    keep.targets.insert(mark.clone());
                }
                keep.hashes.extend(h);
            }
        }
    }
    (keep, rebuilt)
}

/// 実行ファイルは、cargo が `deps/<名前>-<指紋>` から `target/debug/<名前>` へ写したもの(リンク
/// か写し)だけを知らせてくる。`deps` の中の同じ名前で大きさの同じものの指紋を返す。
fn uplifted(p: &Path) -> Vec<String> {
    let (Some(dir), Some(stem), Ok(m)) = (p.parent(), p.file_stem(), fs::metadata(p)) else {
        return Vec::new();
    };
    let stem = stem.to_string_lossy().replace('-', "_");
    fs::read_dir(dir.join("deps"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.metadata().is_ok_and(|d| d.len() == m.len()))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let h = fingerprint_of(&name)?;
            name.starts_with(&format!("{stem}-")).then(|| h.to_string())
        })
        .collect()
}

pub(crate) fn size_of(p: &Path) -> u64 {
    let Ok(m) = fs::symlink_metadata(p) else {
        return 0;
    };
    if m.is_dir() {
        fs::read_dir(p)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| size_of(&e.path()))
            .sum()
    } else {
        m.len()
    }
}

/// フォルダの中でいちばん新しい更新時刻(フォルダ自身を含む)。
fn newest(p: &Path) -> SystemTime {
    let own = fs::metadata(p)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH);
    fs::read_dir(p)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| {
            let q = e.path();
            if q.is_dir() {
                newest(&q)
            } else {
                e.metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(SystemTime::UNIX_EPOCH)
            }
        })
        .fold(own, SystemTime::max)
}

/// 消す候補: (分類, 場所)
pub fn stale(
    profile_dir: &Path,
    keep: &Keep,
    incremental_before: SystemTime,
) -> Vec<(&'static str, PathBuf)> {
    let mut out = Vec::new();
    for (kind, sub) in [
        ("deps", "deps"),
        ("build", "build"),
        (".fingerprint", ".fingerprint"),
        ("examples", "examples"),
    ] {
        for e in fs::read_dir(profile_dir.join(sub))
            .into_iter()
            .flatten()
            .flatten()
        {
            let name = e.file_name().to_string_lossy().into_owned();
            if fingerprint_of(&name).is_none_or(|h| keep.hashes.contains(h)) {
                continue;
            }
            if kind == ".fingerprint" && keep.targets.iter().any(|t| e.path().join(t).exists()) {
                continue;
            }
            out.push((kind, e.path()));
        }
    }
    for e in fs::read_dir(profile_dir.join("incremental"))
        .into_iter()
        .flatten()
        .flatten()
    {
        if newest(&e.path()) < incremental_before {
            out.push(("incremental", e.path()));
        } else {
            out.extend(
                old_sessions(&e.path())
                    .into_iter()
                    .map(|p| ("incremental の前の回", p)),
            );
        }
    }
    out
}

/// incremental の crate のフォルダの中の、いちばん新しい仕上がった回より前の回とその錠。
///
/// rustc は次のコンパイルでいちばん新しい仕上がった回だけを読むが、片付けではその 1 つ前の回も
/// 残す。使わないのに crate ごとに 1 回分ずつ場所を取る(2026-10-23 のクラウドで incremental
/// 7.9 GB のうち 3.5 GB)。途中の回(`-working`)は動いている rustc の物なので残す。
pub fn old_sessions(crate_dir: &Path) -> Vec<PathBuf> {
    let mut done: Vec<String> = fs::read_dir(crate_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("s-") && !n.ends_with("-working"))
        .collect();
    // 名前は s-<時刻>-<乱数>-<印>。時刻は base36 なので、長さ・文字の順で古い順に並ぶ
    done.sort_by_key(|n| {
        let t = n.split('-').nth(1).unwrap_or("").to_string();
        (t.len(), t)
    });
    done.pop();
    let mut out = Vec::new();
    for n in done {
        out.push(crate_dir.join(&n));
        if let Some((stem, _)) = n.rsplit_once('-') {
            let lock = crate_dir.join(format!("{stem}.lock"));
            if lock.exists() {
                out.push(lock);
            }
        }
    }
    out
}

/// 組み立てを `--message-format json` で問い合わせる(足りないものは作る)。
fn ask(
    cargo: &dyn Fn() -> Command,
    args: &[&str],
    env: &[(String, String)],
) -> Result<(Keep, Vec<String>), String> {
    // `--message-format` はサブコマンドのすぐ後に置く(`--` の後は clippy への指定になる)
    let mut c = cargo();
    c.envs(env.iter().map(|(k, v)| (k, v)))
        .args(&args[..1])
        .args(["--message-format", "json"])
        .args(&args[1..])
        .stderr(Stdio::inherit());
    println!("== 問い合わせ: {c:?}");
    let o = c
        .output()
        .map_err(|e| format!("{c:?} を動かせません: {e}"))?;
    if !o.status.success() {
        return Err(format!(
            "{c:?} が失敗しました。組み立てが通る状態で動かしてください"
        ));
    }
    Ok(fingerprints_in(&String::from_utf8_lossy(&o.stdout)))
}

/// リリースで作ってあるアプリ(target/release にある実行ファイル)の crate。
fn release_apps(target: &Path) -> Vec<&'static str> {
    let exe = if cfg!(windows) { ".exe" } else { "" };
    [
        ("jc-app", "kie-monobench"),
        ("jc-cli", "kmb"),
        ("jc-mcp", "kmb-mcp"),
    ]
    .into_iter()
    .filter(|(_, bin)| target.join("release").join(format!("{bin}{exe}")).is_file())
    .map(|(p, _)| p)
    .collect()
}

fn gb(b: u64) -> String {
    format!("{:.1} GB", b as f64 / 1e9)
}

/// 問い合わせるプロファイルのフォルダ・組み立て・足す環境変数。
type Plan<'a> = (PathBuf, Vec<Vec<String>>, &'a [(String, String)]);

/// `release`・`release_env` は release の組み立てに足す引数と環境変数(`cargo xtask release` と同じ。
/// release.rs。cc は `CFLAGS` が変わるとビルドスクリプトを走らせ直すので、環境変数も揃える)。
pub fn run(
    o: &SweepOptions,
    target: &Path,
    release: &[String],
    release_env: &[(String, String)],
    cargo: &dyn Fn() -> Command,
) -> Result<bool, String> {
    let before = SystemTime::now() - Duration::from_secs(o.keep_days * 86_400);
    // プロファイルのフォルダごとに、問い合わせる組み立て
    let mut plans: Vec<Plan> = vec![(
        target.join("debug"),
        vec![
            // 検査(cargo xtask verify)のテストと clippy、xtask 自身
            vec!["test", "--workspace", "--no-run"],
            vec![
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
            vec!["build", "-p", "xtask"],
        ]
        .into_iter()
        .map(|v| v.into_iter().map(String::from).collect())
        .collect(),
        &[],
    )];
    let apps = release_apps(target);
    if !apps.is_empty() {
        let mut b: Vec<String> = vec!["build".into(), "--release".into()];
        b.extend(release.iter().cloned());
        for a in &apps {
            b.extend(["-p".into(), a.to_string()]);
        }
        plans.push((target.join("release"), vec![b], release_env));
    } else if target.join("release").is_dir() {
        println!("target/release にアプリが無いので、release は調べません");
    }
    let mut total: BTreeMap<&str, u64> = BTreeMap::new();
    let mut doomed = Vec::new();
    for (dir, asks, env) in &plans {
        if !dir.is_dir() {
            continue;
        }
        let mut keep = Keep::default();
        for a in asks {
            let args: Vec<&str> = a.iter().map(String::as_str).collect();
            keep.extend(ask(cargo, &args, env)?.0);
        }
        for (kind, p) in stale(dir, &keep, before) {
            *total.entry(kind).or_default() += size_of(&p);
            doomed.push((dir.clone(), p));
        }
    }
    let sum: u64 = total.values().sum();
    println!("\n== 今の版で使わないもの(合わせて {})", gb(sum));
    for (kind, b) in &total {
        println!("  {kind}: {}", gb(*b));
    }
    if !o.yes {
        println!("消すには --yes を付けて動かしてください(この見積もりでは何も消していません)");
        return Ok(true);
    }
    let mut failed = 0;
    for (_, p) in &doomed {
        let r = if p.is_dir() {
            fs::remove_dir_all(p)
        } else {
            fs::remove_file(p)
        };
        if r.is_err() {
            failed += 1;
        }
    }
    println!(
        "{} 個を消しました{}",
        doomed.len() - failed,
        if failed > 0 {
            format!("({failed} 個は使用中などで消せませんでした)")
        } else {
            String::new()
        }
    );
    // 消した後に組み立てが作り直しなしで済むかを確かめる
    let mut rebuilt = Vec::new();
    for (dir, asks, env) in &plans {
        if !dir.is_dir() {
            continue;
        }
        for a in asks {
            let args: Vec<&str> = a.iter().map(String::as_str).collect();
            rebuilt.extend(ask(cargo, &args, env)?.1);
        }
    }
    println!(
        "消した後の問い合わせで作り直した crate: {} 個(0 なら次のビルドは遅くならない){}",
        rebuilt.len(),
        if rebuilt.is_empty() {
            String::new()
        } else {
            format!(": {}", rebuilt.join(", "))
        }
    );
    Ok(rebuilt.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprints_are_read_from_artifact_names() {
        assert_eq!(
            fingerprint_of("libjc_ui-0123456789abcdef.rlib"),
            Some("0123456789abcdef")
        );
        assert_eq!(
            fingerprint_of("jc-ui-0123456789abcdef"),
            Some("0123456789abcdef")
        );
        assert_eq!(
            fingerprint_of("jc_ui-0123456789abcdef.pdb"),
            Some("0123456789abcdef")
        );
        assert_eq!(fingerprint_of("kie-monobench.exe"), None);
        assert_eq!(fingerprint_of("jc_ui-1sc38580bcvvp"), None);
    }

    #[test]
    fn only_listed_fingerprints_are_kept() {
        let lines = [
            r#"{"reason":"compiler-artifact","fresh":true,"filenames":["/t/debug/deps/libjc_core-00000000000000aa.rlib"],"executable":null}"#,
            r#"{"reason":"compiler-artifact","fresh":false,"filenames":["/t/debug/build/smlib-sys-00000000000000bb/build-script-build"]}"#,
            r#"{"reason":"build-script-executed","out_dir":"/t/debug/build/smlib-sys-00000000000000cc/out"}"#,
            "not json",
        ]
        .join("\n");
        let (keep, rebuilt) = fingerprints_in(&lines);
        assert_eq!(
            keep.hashes,
            ["00000000000000aa", "00000000000000bb", "00000000000000cc"]
                .map(String::from)
                .into()
        );
        assert!(keep.targets.is_empty());
        assert_eq!(rebuilt, ["?(?)"]);

        let dir = std::env::temp_dir().join(format!("jc-sweep-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        for f in [
            "deps/libjc_core-00000000000000aa.rlib",
            "deps/libjc_core-00000000000000dd.rlib",
            "build/smlib-sys-00000000000000cc/out/x",
            "build/smlib-sys-00000000000000ee/out/x",
            ".fingerprint/jc-core-00000000000000dd/lib",
            "incremental/jc_core-1sc38580bcvvp/s-1/x",
        ] {
            let p = dir.join(f);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, b"x").unwrap();
        }
        let mut s: Vec<(&str, String)> = stale(&dir, &keep, SystemTime::UNIX_EPOCH)
            .into_iter()
            .map(|(k, p)| {
                (
                    k,
                    p.strip_prefix(&dir)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                )
            })
            .collect();
        s.sort();
        assert_eq!(
            s,
            [
                (
                    ".fingerprint",
                    ".fingerprint/jc-core-00000000000000dd".to_string()
                ),
                ("build", "build/smlib-sys-00000000000000ee".to_string()),
                ("deps", "deps/libjc_core-00000000000000dd.rlib".to_string()),
            ]
        );
        // 使っていない日数を越えた incremental は消す候補
        let later = SystemTime::now() + Duration::from_secs(60);
        assert!(
            stale(&dir, &keep, later)
                .iter()
                .any(|(k, _)| *k == "incremental")
        );
        fs::remove_dir_all(&dir).unwrap();
    }
    #[test]
    fn only_the_newest_finished_incremental_session_is_kept() {
        let dir = std::env::temp_dir().join(format!("jc-sweep-inc-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let c = dir.join("incremental/jc_core-1sc38580bcvvp");
        for d in [
            "s-hngrz1pi1r-0ejr8ys-1v55np94xv04z",
            "s-hngs88nqt9-0dmgt06-1v55np94xv04z",
            "s-hngs9aaaaa-0aaaaaa-working",
        ] {
            fs::create_dir_all(c.join(d)).unwrap();
            let stem = d.rsplit_once('-').unwrap().0;
            fs::write(c.join(format!("{stem}.lock")), b"").unwrap();
        }
        let names = |v: Vec<PathBuf>| -> Vec<String> {
            let mut v: Vec<String> = v
                .iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect();
            v.sort();
            v
        };
        // 古い仕上がった回とその錠だけ。いちばん新しい回と途中の回は残す
        assert_eq!(
            names(old_sessions(&c)),
            [
                "s-hngrz1pi1r-0ejr8ys-1v55np94xv04z",
                "s-hngrz1pi1r-0ejr8ys.lock"
            ]
        );
        // フォルダごと消さない時は、前の回を消す候補に入れる
        let s = stale(&dir, &Keep::default(), SystemTime::UNIX_EPOCH);
        assert_eq!(
            names(
                s.iter()
                    .filter(|(k, _)| *k == "incremental の前の回")
                    .map(|(_, p)| p.clone())
                    .collect()
            ),
            [
                "s-hngrz1pi1r-0ejr8ys-1v55np94xv04z",
                "s-hngrz1pi1r-0ejr8ys.lock"
            ]
        );
        fs::remove_dir_all(&dir).unwrap();
    }
    #[test]
    fn executables_without_a_fingerprint_keep_their_fingerprint_folders() {
        // Windows(MSVC)の cargo は実行ファイルの名前に指紋を付けない(deps の中も)
        let lines = r#"{"reason":"compiler-artifact","fresh":true,"target":{"kind":["bin"],"name":"xtask"},"filenames":["D:\\t\\debug\\xtask.exe","D:\\t\\debug\\xtask.pdb"],"executable":"D:\\t\\debug\\xtask.exe"}"#;
        let (keep, _) = fingerprints_in(lines);
        assert!(keep.hashes.is_empty());
        assert_eq!(keep.targets, ["bin-xtask".to_string()].into());

        let dir = std::env::temp_dir().join(format!("jc-sweep-bin-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        for f in [
            "deps/xtask.exe",
            ".fingerprint/xtask-00000000000000aa/bin-xtask",
            ".fingerprint/xtask-00000000000000bb/bin-xtask",
            ".fingerprint/xtask-00000000000000cc/lib-xtask",
        ] {
            let p = dir.join(f);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, b"x").unwrap();
        }
        let s: Vec<String> = stale(&dir, &keep, SystemTime::UNIX_EPOCH)
            .into_iter()
            .map(|(_, p)| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        // 実行ファイルの印のあるものは残し、ほかの古いもの(ライブラリの版)は消す
        assert_eq!(s, ["xtask-00000000000000cc"]);
        fs::remove_dir_all(&dir).unwrap();
    }
}
