//! 開発用の道具。変えた crate だけをテストと clippy に掛ける(全体を作り直さない)。
//!
//! ```text
//! cargo xtask affected [--base <ref>] [--dependents]
//! cargo xtask checks [--base <ref>]
//! cargo xtask verify [<crate>...] [--base <ref>] [--dependents] [--all] [--truck] [--no-clippy]
//! cargo xtask sweep [--yes] [--keep-days <日数>]
//! cargo xtask docs
//! cargo xtask api
//! cargo xtask dxc
//! cargo xtask release [--check] [cargo の引数...]
//! cargo xtask paths <ファイル>...
//! cargo xtask licenses [--target <三つ組>] [--out <ファイル>]
//! ```
//!
//! - crate を書かなければ、`--base`(既定は `origin/main`)から変えたファイル(まだコミットして
//!   いないもの、git に入れていないものを含む)を持つ crate を選ぶ。
//! - `--dependents` は、選んだ crate に依存する crate も足す(プッシュの前に)。
//! - `--all` はワークスペース全体。`--truck` は `JC_KERNEL=truck` でもう一度テストする(形の計算の
//!   試験だけ。`TRUCK_TESTS`)。普段は付けず、公開の準備の時だけ流す(2026-10-18 Kie)。
//! - cargo-nextest があれば、テストは nextest で crate をまたいで並べて流し、SMLib の回・truck の回・
//!   clippy を同時に動かす。無ければ `cargo test` で 1 つずつ(`JC_VERIFY_NEXTEST=off` でも)。
//!
//! - `verify` は、どの crate を選んでも(docs だけの変更で選ばなくても)最初に組み立てずに点検する:
//!   層の向き(layers.rs)、ソースの書き方(layers/code.rs)、他の層の名前(layers/names.rs)、理由の
//!   無い飛ばしと消えたテスト(skips.rs)。`--all` ではライセンス(cargo-deny)も。`--all`・`--dependents` は cargo-nextest が無ければ失敗にする。
//! - 点検には資料の形(docs.rs)も入る: docs/機能/ の断片、凍結した経緯、地図.md の crate の一覧。
//!   `docs` は断片から一覧の表を target/docs/ に作り、地図.md の crate の一覧を書き直す。
//! - 毎回読む骨格の資料(agents.rs): AGENTS.md と .claude/rules/ に書いた名前・パス・節の実在、
//!   大きさの上限、決まりに付けた点検の名前、層の表(`docs` が layers.rs から作る)。
//! - コアの公開の項目(api.rs)が分かれ目から変わっていれば、契約テスト(kmb-contract)を使う crate を
//!   全部足して流す(`CORE_API_VERSION` は公開まで上げない、2026-10-21)。`api` は今の一覧を出す。
//! - `release` はアプリを release で作り、exe にこの PC のパス(ユーザー名を含むホームなど)が残って
//!   いないかを数える。`paths` は数えるだけ(release.rs)。組み立てた後、exe の横に他者の部品の
//!   許諾文の一覧(THIRD-PARTY-LICENSES.txt)を置く。`licenses` はその一覧だけを作る(licenses.rs)。
//!   `verify --all` と依存を変えた時は、Windows 版の一覧が作れること(許諾の本文の無い部品が無いこと)を確かめる。
//! - `dxc` は DirectX 12 のシェーダーを作る DXC(dxcompiler.dll)を、アプリの横に置く(dxc.rs)。
//! - `verify` は組み立てが通った後、target のあるディスクの空きが `JC_SWEEP_BELOW_GB`(既定 12)GB を
//!   切っていれば `sweep --yes` を流す。cargo は依存の機能や版が変わるたびに別の名前で作り直して
//!   古いものを消さず、クラウドの作業場(約 30 GB)では main を 2〜3 回取り込むと溢れていた
//!   (docs/verify.md の 8 節)。空きを測れない所(`df` の無い Windows など)では何もしない。
//! - `verify` は組み立ての前に、揃えた依存(kmb-workspace-hack)が前の組み立てから変わっていれば
//!   使われなくなる incremental を消し、空きが少なければ incremental の前の回を消す(組み立ての
//!   途中で空きが尽きないように。2026-10-23)。
//! - `verify` は組み立ての間、空きが `JC_WATCH_BELOW_GB`(既定 6)GB を切れば、作り直して要らなく
//!   なった古いテストの実行ファイルと incremental の前の回を消す。組み立ての後の片付けは、cargo に
//!   問い合わせる sweep を `--all` の時だけにし、ほかは作り直して要らなくなった物だけを消す
//!   (space.rs。依存の宣言が変わって下流の指紋が全部変わった時に、組み立てと sweep で尽きていた)。
//! - クラウド(Linux)では、cargo-nextest と cargo-hakari が無ければ `scripts/install-*.sh` で入れる。
//!
//! - 段を流してログから件数を読むのは jobs.rs、空きを保つ片付けは space.rs と sweep.rs。
//!
//! 手順の説明は docs/verify.md。

mod agents;
mod api;
mod board;
mod copies;
mod docs;
mod dxc;
mod history;
mod jobs;
mod layers;
mod licenses;
mod products;
mod prose;
mod release;
mod skips;
mod source;
mod space;
mod sweep;
mod words;

use jobs::{Done, run_counting_compiles, spawn, step, wait_all};
use serde_json::Value as Json;
use space::{avail_gb_at, free_before_build, remember_hack, sweep_if_low};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Instant, SystemTime};

/// ワークスペースの crate 1 つ。
#[derive(Debug, Clone, PartialEq, Default)]
struct Package {
    name: String,
    /// ワークスペースの根からのフォルダ(`/` 区切り、例 `crates/kmb-core`)
    dir: String,
    /// ワークスペースの中の依存先(開発用・ビルド用を含む)
    deps: Vec<String>,
    /// `[package.metadata.kmb]` の `layer`
    layer: Option<String>,
    /// Cargo.toml の `description`(docs/地図.md の crate の一覧に使う)
    description: Option<String>,
    /// ワークスペースの中の依存の辺(種類つき)
    edges: Vec<Edge>,
}

/// 依存の種類。
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Normal,
    /// `[dev-dependencies]`(テスト・例だけが使う)
    Dev,
    /// `[build-dependencies]`
    Build,
}

/// 依存の辺 1 本。
#[derive(Debug, Clone, PartialEq)]
struct Edge {
    name: String,
    kind: Kind,
}

/// 変えたファイルが、どの crate の検査を要るか。
#[derive(Debug, Default, PartialEq)]
struct Changes {
    packages: BTreeSet<String>,
    /// ワークスペース全体に効く(根の Cargo.toml・Cargo.lock・.cargo)
    everything: bool,
    /// どの crate にも入らない(docs など。検査は要らない)
    unrelated: Vec<String>,
}

fn classify(files: &[String], packages: &[Package]) -> Changes {
    let mut c = Changes::default();
    for f in files {
        let f = f.replace('\\', "/");
        if f == "Cargo.toml" || f == "Cargo.lock" || f.starts_with(".cargo/") {
            c.everything = true;
            continue;
        }
        // SMLib の C++ のソースは smlib-sys が作る
        if f.starts_with("third_party/usd-brep/") || f.starts_with("third_party/smlibpy-patches/") {
            c.packages.insert("smlib-sys".into());
            continue;
        }
        // 例のプラグイン(plugins/、ワークスペースの外)は kmb-plugin-host の試験が読む
        if f.starts_with("plugins/") && packages.iter().any(|p| p.name == "kmb-plugin-host") {
            c.packages.insert("kmb-plugin-host".into());
            continue;
        }
        let owner = packages
            .iter()
            .filter(|p| f.starts_with(&format!("{}/", p.dir)))
            .max_by_key(|p| p.dir.len());
        match owner {
            Some(p) => {
                c.packages.insert(p.name.clone());
            }
            None => c.unrelated.push(f),
        }
    }
    c
}

/// truck の回で流す試験: カーネルの違いで結果が変わり得る、形の計算の試験だけ。
/// (crate, 試験の名前の頭)。名前の頭が `None` なら、その crate の試験を全部流す。
///
/// 形を作った後の処理(加工・画面の操作・MCP・見本の加工)は、どちらのカーネルでも同じ
/// 中身の試験になるので、速い方の SMLib の回だけで流す(2026-10-17 Kie の判断。truck は
/// 同じ試験で 3〜15 倍遅い。docs/verify.md の 6 節)。形の計算の試験を足した crate や
/// モジュールは、ここに足す。
const TRUCK_TESTS: &[(&str, Option<&str>)] = &[
    // 押し出し・ブーリアン・フィレット・面取り・穴など、形そのものの試験
    ("kmb-solid", None),
    // 穴の見分けと選び方は、カーネルが返す面(名前・円筒)から作る
    ("kmb-samples", Some("hole_tests::")),
];

/// truck の回で流す試験のうち、選んだ crate にあるもの。
fn truck_targets(
    sel: &Option<BTreeSet<String>>,
    packages: &[Package],
) -> Vec<(&'static str, Option<&'static str>)> {
    TRUCK_TESTS
        .iter()
        .filter(|(n, _)| packages.iter().any(|p| p.name == *n))
        .filter(|(n, _)| sel.as_ref().is_none_or(|s| s.contains(*n)))
        .copied()
        .collect()
}

/// truck の回の nextest の絞り込みの式。
fn truck_filter(ts: &[(&str, Option<&str>)]) -> String {
    ts.iter()
        .map(|(n, head)| match head {
            None => format!("package({n})"),
            Some(h) => format!("(package({n}) & test(/^{h}/))"),
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

/// `roots` と、それに(間接に)依存する crate。
fn with_dependents(roots: &BTreeSet<String>, packages: &[Package]) -> BTreeSet<String> {
    let mut out = roots.clone();
    loop {
        let more: Vec<String> = packages
            .iter()
            .filter(|p| !out.contains(&p.name) && p.deps.iter().any(|d| out.contains(d)))
            .map(|p| p.name.clone())
            .collect();
        if more.is_empty() {
            return out;
        }
        out.extend(more);
    }
}

fn parse_metadata(meta: &Json) -> Vec<Package> {
    let root = meta["workspace_root"]
        .as_str()
        .unwrap_or_default()
        .replace('\\', "/");
    let members: BTreeSet<&str> = meta["workspace_members"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Json::as_str)
        .collect();
    let all = meta["packages"].as_array().cloned().unwrap_or_default();
    let names: BTreeSet<String> = all
        .iter()
        .filter(|p| p["id"].as_str().is_some_and(|id| members.contains(id)))
        .filter_map(|p| p["name"].as_str().map(String::from))
        .collect();
    all.iter()
        .filter(|p| p["id"].as_str().is_some_and(|id| members.contains(id)))
        .map(|p| {
            let manifest = p["manifest_path"]
                .as_str()
                .unwrap_or_default()
                .replace('\\', "/");
            let dir = manifest
                .trim_end_matches("/Cargo.toml")
                .strip_prefix(&root)
                .unwrap_or_default()
                .trim_start_matches('/')
                .to_string();
            let edges: Vec<Edge> = p["dependencies"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|d| {
                    let name = d["name"].as_str().filter(|n| names.contains(*n))?;
                    let kind = match d["kind"].as_str() {
                        Some("dev") => Kind::Dev,
                        Some("build") => Kind::Build,
                        _ => Kind::Normal,
                    };
                    Some(Edge {
                        name: name.into(),
                        kind,
                    })
                })
                .collect();
            let mut deps: Vec<String> = edges.iter().map(|e| e.name.clone()).collect();
            deps.dedup();
            Package {
                name: p["name"].as_str().unwrap_or_default().into(),
                dir,
                deps,
                layer: p["metadata"]["kmb"]["layer"].as_str().map(String::from),
                description: p["description"].as_str().map(String::from),
                edges,
            }
        })
        .collect()
}

fn cargo() -> Command {
    Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
}

fn output(cmd: &mut Command) -> Result<String, String> {
    let o = cmd
        .output()
        .map_err(|e| format!("{cmd:?} を動かせません: {e}"))?;
    if !o.status.success() {
        return Err(format!(
            "{cmd:?} が失敗: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}

/// ワークスペースの crate と、target のフォルダと、`cargo metadata` の出力(nextest に渡す)。
struct Workspace {
    packages: Vec<Package>,
    root: PathBuf,
    target_dir: PathBuf,
    metadata: String,
}

fn workspace() -> Result<Workspace, String> {
    let text = output(cargo().args(["metadata", "--format-version", "1", "--no-deps"]))?;
    let meta: Json = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    Ok(Workspace {
        packages: parse_metadata(&meta),
        root: PathBuf::from(meta["workspace_root"].as_str().unwrap_or(".")),
        target_dir: PathBuf::from(meta["target_directory"].as_str().unwrap_or("target")),
        metadata: text,
    })
}

/// `base` との分かれ目。
fn fork_point(base: &str) -> String {
    output(Command::new("git").args(["merge-base", "HEAD", base]))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|e| {
            eprintln!("{base} との分かれ目が分からないので HEAD と比べます({e})");
            "HEAD".into()
        })
}

/// 分かれ目 `fork` から変えたファイル(作業中・git に入れていないものを含む)。
fn changed_files(fork: &str) -> Result<Vec<String>, String> {
    let mut files: BTreeSet<String> = BTreeSet::new();
    for args in [
        vec!["diff", "--name-only", fork],
        vec!["ls-files", "--others", "--exclude-standard"],
    ] {
        // quotepath を切らないと、日本語の名前(docs/機能/ など)が "\346\251…" の形で返る
        files.extend(
            output(
                Command::new("git")
                    .args(["-c", "core.quotepath=off"])
                    .args(&args),
            )?
            .lines()
            .filter(|l| !l.is_empty())
            .map(String::from),
        );
    }
    Ok(files.into_iter().collect())
}

#[derive(Debug, Default, PartialEq)]
struct Options {
    crates: Vec<String>,
    base: String,
    dependents: bool,
    all: bool,
    truck: bool,
    clippy: bool,
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut o = Options {
        base: "origin/main".into(),
        clippy: true,
        ..Default::default()
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--base" => {
                o.base = it
                    .next()
                    .ok_or("--base の後に比べる版を書いてください")?
                    .clone()
            }
            "--dependents" => o.dependents = true,
            "--all" => o.all = true,
            "--truck" => o.truck = true,
            "--no-clippy" => o.clippy = false,
            s if s.starts_with('-') => return Err(format!("知らない指定です: {s}")),
            s => o.crates.push(s.into()),
        }
    }
    Ok(o)
}

/// 検査する crate を決める。`None` はワークスペース全体。
fn select(
    o: &Options,
    packages: &[Package],
    changed: &[String],
) -> Result<Option<BTreeSet<String>>, String> {
    if o.all {
        return Ok(None);
    }
    let roots: BTreeSet<String> = if o.crates.is_empty() {
        let c = classify(changed, packages);
        if !c.unrelated.is_empty() {
            println!(
                "crate に入らないのでテストしないファイル(点検はする): {}",
                c.unrelated.join(", ")
            );
        }
        if c.everything {
            println!("ワークスペース全体に効くファイルを変えたので、全体を検査します");
            return Ok(None);
        }
        c.packages
    } else {
        for c in &o.crates {
            if !packages.iter().any(|p| &p.name == c) {
                return Err(format!("ワークスペースに crate {c} はありません"));
            }
        }
        o.crates.iter().cloned().collect()
    };
    Ok(Some(if o.dependents {
        with_dependents(&roots, packages)
    } else {
        roots
    }))
}

fn package_args(sel: &Option<BTreeSet<String>>) -> Vec<String> {
    match sel {
        None => vec!["--workspace".into()],
        Some(s) => s
            .iter()
            .flat_map(|n| ["-p".to_string(), n.clone()])
            .collect(),
    }
}

/// cargo-nextest を使えるか。
fn nextest_available() -> bool {
    if nextest_off() {
        return false;
    }
    cargo()
        .args(["nextest", "--version"])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// 説明の中の例(doctest)を持つ crate。nextest は doctest を流さないので、ある時だけ
/// `cargo test --doc` で流す(2026-10-17 の時点では 1 つも無い)。
fn with_doctests(sel: &Option<BTreeSet<String>>, packages: &[Package]) -> Vec<String> {
    packages
        .iter()
        .filter(|p| sel.as_ref().is_none_or(|s| s.contains(&p.name)))
        .filter(|p| {
            source::rust_files(&Path::new(&p.dir).join("src"))
                .iter()
                .any(|f| fs::read_to_string(f).is_ok_and(|t| has_doctest(&t)))
        })
        .map(|p| p.name.clone())
        .collect()
}

/// 説明のコメント(`///`・`//!`)に、rustdoc が試験として動かすコードの囲みがあるか。
/// `doc = include_str!` で外のファイルを説明にしている時も、あるとみなす。
fn has_doctest(text: &str) -> bool {
    let mut open = false;
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with("#![doc") || t.starts_with("#[doc") {
            if t.contains("include_str!") {
                return true;
            }
            continue;
        }
        let Some(body) = t.strip_prefix("///").or_else(|| t.strip_prefix("//!")) else {
            continue;
        };
        let Some(tag) = body.trim_start().strip_prefix("```") else {
            continue;
        };
        if open {
            open = false;
            continue;
        }
        open = true;
        let runs = tag.split(',').map(str::trim).all(|w| {
            matches!(w, "" | "rust" | "should_panic" | "no_run" | "compile_fail")
                || w.starts_with("edition")
        });
        if runs {
            return true;
        }
    }
    false
}

/// verify を同じ target で同時に 2 つ動かさない錠(target/verify.lock)。持っている間だけ効き、
/// 落ちても OS が外す。2 つ目は待たずに止める(同じログと同じテストの実行ファイルを取り合い、
/// 件数が混ざるため)。
fn verify_lock(ws: &Workspace) -> Result<fs::File, String> {
    fs::create_dir_all(&ws.target_dir).map_err(|e| e.to_string())?;
    let path = ws.target_dir.join("verify.lock");
    let f = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| format!("{} を開けません: {e}", path.display()))?;
    match f.try_lock() {
        Ok(()) => Ok(f),
        Err(fs::TryLockError::WouldBlock) => Err(format!(
            "別の cargo xtask verify が同じ target で動いています({})。終わるのを待つか止めてから流す",
            path.display()
        )),
        Err(fs::TryLockError::Error(e)) => Err(format!("{} の錠を取れません: {e}", path.display())),
    }
}

/// nextest で検査する。組み立ては 1 回だけにし、できたテストの実行ファイルを SMLib の回と
/// truck の回で使い回す(cargo を呼ばないので、同時に動かす clippy と錠を取り合わない)。
fn verify_nextest(
    o: &Options,
    sel: &Option<BTreeSet<String>>,
    ws: &Workspace,
) -> Result<Vec<Done>, String> {
    let pk = package_args(sel);
    let dir = ws.target_dir.join("verify");
    fs::create_dir_all(&dir).map_err(|e| format!("{} を作れません: {e}", dir.display()))?;
    let meta = dir.join("cargo-metadata.json");
    let bins = dir.join("binaries.json");
    fs::write(&meta, &ws.metadata).map_err(|e| e.to_string())?;

    // 1. 組み立て(テストの実行ファイルを 1 回だけ作る)
    let t = Instant::now();
    let mut list = cargo();
    list.args(["nextest", "list"]).args(&pk).args([
        "--list-type",
        "binaries-only",
        "--message-format",
        "json",
    ]);
    println!("\n== 組み立て: {list:?}");
    let out = fs::File::create(&bins).map_err(|e| e.to_string())?;
    let since = SystemTime::now();
    let target = ws.target_dir.clone();
    let watch = space::Watch::start(
        ws.target_dir.join("debug"),
        since,
        space::watch_below_gb(std::env::var("JC_WATCH_BELOW_GB").ok().as_deref()),
        move || avail_gb_at(&target),
    );
    let (mut ok, mut compiled) = run_counting_compiles(list.stdout(Stdio::from(out)));
    let freed = watch.finish();
    if ok && freed.count > 0 {
        // 同じ名前の別の試験を消し過ぎていれば、ここで作り直す(足りない物だけ)
        println!("\n== 組み立ての途中で消した物があるので、足りない物が無いかもう一度組み立てます");
        let out = fs::File::create(&bins).map_err(|e| e.to_string())?;
        let (ok2, again) = run_counting_compiles(list.stdout(Stdio::from(out)));
        ok = ok2;
        compiled += again;
    }
    let mut built = Done::new("組み立て", ok, t.elapsed().as_secs_f64());
    built.detail = Some(format!("コンパイル {compiled} 個"));
    let mut results = vec![built];
    if !ok {
        return Ok(results);
    }

    // 2. SMLib の回・truck の回・(doctest と)clippy を同時に動かす
    println!();
    let run = |profile: &str| {
        let mut c = cargo();
        c.env("JC_SKIP_LOG", skip_log(ws))
            .args(["nextest", "run", "--no-fail-fast", "--profile", profile])
            .arg("--binaries-metadata")
            .arg(&bins)
            .arg("--cargo-metadata")
            .arg(&meta);
        c
    };
    let mut jobs = vec![spawn("テスト", run("verify"), dir.join("test.log"), true)?];
    if o.truck {
        let ts = truck_targets(sel, &ws.packages);
        if ts.is_empty() {
            println!("== テスト(truck): 形の計算の試験が対象にないので省きます");
        } else {
            let mut c = run("verify-truck");
            c.args(["-E", &truck_filter(&ts)]).env("JC_KERNEL", "truck");
            jobs.push(spawn("テスト(truck)", c, dir.join("test-truck.log"), true)?);
        }
    }
    // cargo を呼ぶ段は錠を取り合うので、1 つの列に並べる
    let docs = with_doctests(sel, &ws.packages);
    let mut cargo_steps: Vec<String> = Vec::new();
    if !docs.is_empty() {
        let p: Vec<String> = docs.iter().map(|n| format!("-p {n}")).collect();
        cargo_steps.push(format!("cargo test --doc {}", p.join(" ")));
    }
    if o.clippy {
        cargo_steps.push(format!(
            "cargo clippy {} --all-targets -- -D warnings",
            pk.join(" ")
        ));
    }
    if !cargo_steps.is_empty() {
        let name = if o.clippy && docs.is_empty() {
            "clippy"
        } else if o.clippy {
            "doctest と clippy"
        } else {
            "doctest"
        };
        jobs.push(spawn(
            name,
            chain(&cargo_steps),
            dir.join("clippy.log"),
            false,
        )?);
    }
    results.extend(wait_all(jobs));
    Ok(results)
}

/// cargo のコマンドを順に動かす(1 つ目が失敗しても 2 つ目を動かし、どれかが失敗したら失敗)。
/// xtask 自身を「列を動かす係」として呼び直す。
fn chain(steps: &[String]) -> Command {
    let mut c = Command::new(std::env::current_exe().unwrap_or_else(|_| "xtask".into()));
    c.arg("__chain").args(steps);
    c
}

/// `__chain` の中身: 渡された各コマンド(空白区切り、先頭は cargo)を順に動かす。
fn run_chain(steps: &[String]) -> ExitCode {
    let mut ok = true;
    for s in steps {
        let mut w = s.split_whitespace();
        let _cargo = w.next();
        let mut c = cargo();
        c.args(w);
        println!("== {c:?}");
        ok &= c.status().is_ok_and(|s| s.success());
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// nextest の無い所での検査(前からの形。テストの実行ファイルを 1 つずつ順に流す)。
fn verify_cargo_test(o: &Options, sel: &Option<BTreeSet<String>>, ws: &Workspace) -> Vec<Done> {
    println!(
        "cargo-nextest が無いので cargo test で 1 つずつ流します(遅い。入れ方は docs/verify.md の 1 節、\
         クラウドは scripts/install-nextest.sh)"
    );
    let pk = package_args(sel);
    let mut results = Vec::new();
    let mut test = cargo();
    test.env("JC_SKIP_LOG", skip_log(ws))
        .arg("test")
        .args(&pk)
        .arg("--no-fail-fast");
    results.push(step("テスト", test));
    if o.truck {
        let ts = truck_targets(sel, &ws.packages);
        if ts.is_empty() {
            println!("\n== テスト(truck): 形の計算の試験が対象にないので省きます");
        }
        for (n, head) in ts {
            let mut t = cargo();
            t.args(["test", "-p", n, "--no-fail-fast"])
                .env("JC_KERNEL", "truck")
                .env("JC_SKIP_LOG", skip_log(ws));
            if let Some(h) = head {
                t.args(["--", h]);
            }
            results.push(step(&format!("テスト(truck) {n}"), t));
        }
    }
    if o.clippy {
        let mut c = cargo();
        c.arg("clippy")
            .args(&pk)
            .args(["--all-targets", "--", "-D", "warnings"]);
        results.push(step("clippy", c));
    }
    results
}

/// 実行中の飛ばしの印を書くファイル(kmb-contract の `skip!`)。
fn skip_log(ws: &Workspace) -> PathBuf {
    ws.target_dir.join("verify").join("skips.log")
}

/// 組み立てずに流す点検(層・飛ばし・消えたテスト)。どの crate を選んでも、選ばなくても流す。
/// 分かれ目 `fork` のファイルの中身(無ければ `None`)。
fn git_show(fork: &str, path: &str) -> Option<String> {
    output(Command::new("git").args(["show", &format!("{fork}:{path}")])).ok()
}

/// kmb-core の公開の項目の一覧(今と分かれ目)。
fn core_api(ws: &Workspace, fork: &str) -> Option<(api::Api, api::Api)> {
    let dir = &ws.packages.iter().find(|p| p.name == "kmb-core")?.dir;
    let src = format!("{dir}/src");
    let now = api::of_dir(Path::new(&src));
    let mut old = api::Api::new();
    let listed =
        output(Command::new("git").args(["ls-tree", "-r", "--name-only", fork, &src])).ok()?;
    for f in listed.lines().filter(|f| f.ends_with(".rs")) {
        if let Some(text) = git_show(fork, f) {
            let rel = f.strip_prefix(&format!("{src}/")).unwrap_or(f);
            api::add(&mut old, rel, &text);
        }
    }
    Some((old, now))
}

/// コアの公開の項目が変わっていれば、契約テストを使う crate(と kmb-contract)を選びに足す。
fn add_contract_users(ws: &Workspace, fork: &str, sel: &mut Option<BTreeSet<String>>) {
    let Some(s) = sel.as_mut() else {
        return;
    };
    let Some((old, now)) = core_api(ws, fork) else {
        return;
    };
    let (lost, gained) = api::diff(&old, &now);
    if lost.is_empty() && gained.is_empty() {
        return;
    }
    println!(
        "コアの公開の項目が変わった(消えた {}・増えた {})ので、契約テストを使う crate を全部足します",
        lost.len(),
        gained.len()
    );
    for (sign, v) in [("-", &lost), ("+", &gained)] {
        for x in v.iter().take(10) {
            println!("  {sign} {x}");
        }
        if v.len() > 10 {
            println!("  {sign} …ほか {} 件", v.len() - 10);
        }
    }
    s.insert("kmb-contract".into());
    for p in &ws.packages {
        if p.edges.iter().any(|e| e.name == "kmb-contract") {
            s.insert(p.name.clone());
        }
    }
}

fn checks(ws: &Workspace, fork: &str, changed: &[String]) -> Done {
    println!(
        "\n== 点検(層の向き・他の層の名前・飛ばし・消えたテスト・資料・他製品の名前・言葉の揺れ)"
    );
    let t = Instant::now();
    let root = Path::new(".");
    let layer = layers::check(root, &ws.packages);
    let (skip, removed, counts) = match skips::allowed(root) {
        Ok(allow) => {
            let r = skips::check(root, &ws.packages, &allow);
            let gone = skips::removed(root, &ws.packages, fork, changed, &allow);
            let counts = format!("テスト {}・ignore {}", r.tests, r.ignored);
            (r.problems, gone, counts)
        }
        Err(e) => (vec![e], vec![], String::new()),
    };
    let read = |f: &str| git_show(fork, f);
    let mut doc = docs::check(root, &ws.packages, &docs::Base { read: &read }, changed);
    doc.extend(board::check(root, &read));
    doc.extend(prose::check(root));
    let skeleton = agents::check(root);
    doc.extend(skeleton.problems);
    let mut product = products::check(root);
    product.extend(products::check_commits(fork));
    let word = words::check(root);
    // 変えたら直す合図(落とさない)
    for h in agents::change_hints(root, &read) {
        println!("  合図: {h}");
    }
    for (title, ps) in [
        ("層", &layer),
        ("飛ばし", &skip),
        ("消えたテスト", &removed),
        ("資料", &doc),
        ("他製品の名前", &product),
        ("言葉の揺れ", &word),
    ] {
        for p in ps {
            println!("  {title}: {p}");
        }
    }
    println!(
        "  骨格の資料: 点検の無い決まり {}・決まりの無い点検 {}(一覧は `cargo xtask docs`)",
        skeleton.unchecked.len(),
        skeleton.unused.len()
    );
    let ok = layer.is_empty()
        && skip.is_empty()
        && removed.is_empty()
        && doc.is_empty()
        && product.is_empty()
        && word.is_empty();
    let mut d = Done::new("点検", ok, t.elapsed().as_secs_f64());
    d.detail = Some(format!(
        "層 {}・飛ばし {}・消えたテスト {}・資料 {}・他製品の名前 {}・言葉の揺れ {}。{counts}",
        layer.len(),
        skip.len(),
        removed.len(),
        doc.len(),
        product.len(),
        word.len()
    ));
    println!(
        "== 点検: {}({})",
        if ok { "通過" } else { "失敗" },
        d.detail.as_deref().unwrap_or("")
    );
    d
}

/// 実行中の飛ばし(テストが `skip!` で書いた印)。印が無ければ `None`。
fn runtime_skips(ws: &Workspace) -> Option<Done> {
    let log = fs::read_to_string(skip_log(ws)).ok()?;
    if log.trim().is_empty() {
        return None;
    }
    let (seen, problems) = match skips::allowed(Path::new(".")) {
        Ok(a) => skips::runtime(&log, &a),
        Err(e) => (vec![], vec![e]),
    };
    println!("\n== 実行中の飛ばし: {} 件", seen.len());
    for s in &seen {
        println!("  {s}");
    }
    for p in &problems {
        println!("  表に無い: {p}");
    }
    let mut d = Done::new("実行中の飛ばし", problems.is_empty(), 0.0);
    d.detail = Some(format!("{} 件", seen.len()));
    Some(d)
}

/// ライセンス・取り寄せ先の点検(cargo-deny、deny.toml)。`required` なら、道具が無いのも失敗。
fn deny(ws: &Workspace, required: bool) -> Option<Done> {
    let have = cargo()
        .args(["deny", "--version"])
        .output()
        .is_ok_and(|o| o.status.success());
    if !have {
        if !required {
            return None;
        }
        println!(
            "\n== ライセンス: cargo-deny が無いので点検できません(入れ方は docs/verify.md の 1 節。\
             クラウドは scripts/install-deny.sh)"
        );
        let mut d = Done::new("ライセンス", false, 0.0);
        d.detail = Some("cargo-deny が無い".into());
        return Some(d);
    }
    let log = ws.target_dir.join("verify").join("deny.log");
    let mut c = cargo();
    c.args(["deny", "check", "licenses", "bans", "sources"]);
    println!("\n== ライセンス: {c:?}\n   ログ: {}", log.display());
    let t = Instant::now();
    let out = c.output();
    let ok = out.as_ref().is_ok_and(|o| o.status.success());
    let text = out
        .map(|o| {
            String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)
        })
        .unwrap_or_else(|e| e.to_string());
    let _ = fs::write(&log, &text);
    let last = text
        .lines()
        .rfind(|l| l.contains(" ok") || l.contains("FAILED"))
        .unwrap_or("")
        .trim()
        .to_string();
    if !ok {
        for l in text.lines().filter(|l| l.starts_with("error")) {
            println!("  {l}");
        }
    }
    let mut d = Done::new("ライセンス", ok, t.elapsed().as_secs_f64());
    d.detail = Some(last);
    Some(d)
}

/// 許諾文の一覧の点検: 配る Windows 版の部品の許諾文が全部そろうこと(licenses.rs)。
fn licenses_check(ws: &Workspace) -> Done {
    println!("\n== 許諾文の一覧: {} 向け", licenses::WINDOWS);
    let t = Instant::now();
    let r = licenses::build(&ws.root, licenses::WINDOWS, &cargo);
    let mut d = Done::new("許諾文の一覧", r.is_ok(), t.elapsed().as_secs_f64());
    d.detail = Some(match r {
        Ok((_, n)) => format!("他者の crate {n} 件"),
        Err(p) => {
            for l in &p {
                println!("  {l}");
            }
            format!("{} 件の誤り", p.len())
        }
    });
    d
}

/// 揃えた依存(kmb-workspace-hack)の点検(cargo-hakari)。`cargo hakari generate --diff` と
/// `manage-deps --dry-run` が差を出さないこと。依存を変えたのに作り直さないと、PC の取り込みで
/// 初めて落ちる(道具の無いクラウドで、古い揃えのまま通ってしまう)。
/// 道具が無いのも失敗(クラウドは scripts/install-hakari.sh)。
fn hakari(ws: &Workspace) -> Vec<Done> {
    let have = || {
        cargo()
            .args(["hakari", "--version"])
            .output()
            .is_ok_and(|o| o.status.success())
    };
    if !have() {
        install_on_linux("scripts/install-hakari.sh");
    }
    if !have() {
        println!(
            "\n== 揃えた依存: cargo-hakari が無いので点検できません(入れ方は docs/verify.md の 1 節。\
             クラウドは scripts/install-hakari.sh)"
        );
        let mut d = Done::new("揃えた依存", false, 0.0);
        d.detail = Some("cargo-hakari が無い".into());
        return vec![d];
    }
    let log = ws.target_dir.join("verify").join("hakari.log");
    let t = Instant::now();
    let mut text = String::new();
    let mut ok = true;
    for args in [
        &["hakari", "generate", "--diff"][..],
        &["hakari", "manage-deps", "--dry-run"][..],
    ] {
        let mut c = cargo();
        c.args(args);
        println!("\n== 揃えた依存: {c:?}");
        let out = c.output();
        let fine = out.as_ref().is_ok_and(|o| o.status.success());
        let got = out
            .map(|o| {
                String::from_utf8_lossy(&o.stdout).into_owned()
                    + &String::from_utf8_lossy(&o.stderr)
            })
            .unwrap_or_else(|e| e.to_string());
        // どちらも差があれば 1 で終わる
        if !fine {
            ok = false;
            for l in got.lines().take(20) {
                println!("  {l}");
            }
        }
        text += &got;
    }
    let _ = fs::write(&log, &text);
    let mut d = Done::new("揃えた依存", ok, t.elapsed().as_secs_f64());
    d.detail = Some(if ok {
        "そろっている".into()
    } else {
        "cargo hakari generate と manage-deps --yes で直し、plugins/post-example/Cargo.lock も作り直す"
            .into()
    });
    vec![d]
}

/// クラウドの作業場(Linux)で道具が無い時に、入れる手順(`scripts/install-*.sh`)を流す。
/// コンテナは毎回空から始まり、入れ忘れると `--dependents` が 3 秒で落ちていた(2026-10-23)
fn install_on_linux(script: &str) {
    if cfg!(target_os = "linux") && Path::new(script).is_file() {
        println!("\n== 無いので {script} で入れます");
        if !Command::new("sh")
            .arg(script)
            .status()
            .is_ok_and(|s| s.success())
        {
            println!("{script} が失敗しました");
        }
    }
}

/// nextest を使わないと明示したか(`JC_VERIFY_NEXTEST=off`)。
fn nextest_off() -> bool {
    std::env::var("JC_VERIFY_NEXTEST").is_ok_and(|v| v.trim() == "off")
}

fn run() -> Result<ExitCode, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((cmd, rest)) = args.split_first() else {
        return Err(
            "使い方: cargo xtask affected|checks|verify|sweep|docs|io|api|dxc|release|paths [...](docs/verify.md)"
                .into(),
        );
    };
    if cmd == "__chain" {
        return Ok(run_chain(rest));
    }
    if cmd == "sweep" {
        let so = sweep::parse(rest)?;
        let ws = workspace()?;
        let release = release::cargo_config(&ws.root, &ws.target_dir);
        let env = release::c_env(&ws.root, &ws.target_dir);
        return Ok(
            if sweep::run(&so, &ws.target_dir, &release, &env, &cargo)? {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            },
        );
    }
    if cmd == "release" {
        let ws = workspace()?;
        return release::run(&ws.root, &ws.target_dir, rest, &cargo);
    }
    if cmd == "licenses" {
        let ws = workspace()?;
        licenses::run(&ws.root, &ws.target_dir, rest, &cargo)?;
        return Ok(ExitCode::SUCCESS);
    }
    if cmd == "paths" {
        let ws = workspace()?;
        return release::paths(&ws.root, &ws.target_dir, rest);
    }
    if cmd == "dxc" {
        dxc::run(&workspace()?.target_dir)?;
        return Ok(ExitCode::SUCCESS);
    }
    if cmd == "docs" {
        let ws = workspace()?;
        for p in docs::generate(Path::new("."), &ws.packages, &ws.target_dir.join("docs"))? {
            println!("書いた: {}", p.display());
        }
        if agents::generate(Path::new("."))? {
            println!("書いた: {}(層の表)", agents::FILE);
        }
        let r = agents::check(Path::new("."));
        for p in &r.problems {
            println!("骨格の資料の誤り: {p}");
        }
        println!("点検の名前が付いていない決まり({} 件):", r.unchecked.len());
        for u in &r.unchecked {
            println!("  {u}");
        }
        println!(
            "どの決まりにも付いていない点検({} 件): {}",
            r.unused.len(),
            r.unused.join("・")
        );
        let open = docs::open_decisions(Path::new("."));
        println!(
            "まだ断片の無い決定({} 件。作った断片の頭に「- 決定: 名前」を書く):",
            open.len()
        );
        for d in &open {
            println!("  {d}");
        }
        return Ok(ExitCode::SUCCESS);
    }
    if cmd == "io" {
        // 入出力の一覧 docs/入出力.md を登録から書き直す(apps/kmb-mcp/src/reference.rs)
        let ok = cargo()
            .args(["test", "-q", "-p", "kmb-mcp", "--lib", "io_reference"])
            .env("JC_BLESS", "1")
            .status()
            .map_err(|e| format!("cargo test を動かせない: {e}"))?
            .success();
        println!(
            "{}: docs/入出力.md",
            if ok {
                "書いた"
            } else {
                "書けなかった"
            }
        );
        return Ok(if ok {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        });
    }
    if cmd == "api" {
        let ws = workspace()?;
        let (_, now) = core_api(&ws, "HEAD").ok_or("kmb-core が見つかりません")?;
        for (k, files) in &now {
            println!("{}: {k}", files.join("・"));
        }
        return Ok(ExitCode::SUCCESS);
    }
    let mut o = parse_args(rest)?;
    let ws = workspace()?;
    let fork = fork_point(&o.base);
    let changed = changed_files(&fork)?;
    let mut sel = select(&o, &ws.packages, &changed)?;
    if o.crates.is_empty() {
        add_contract_users(&ws, &fork, &mut sel);
    }
    match cmd.as_str() {
        "checks" => {
            let d = checks(&ws, &fork, &changed);
            Ok(if d.ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        "affected" => {
            match &sel {
                None => println!("(ワークスペース全体)"),
                Some(s) => s.iter().for_each(|n| println!("{n}")),
            }
            Ok(ExitCode::SUCCESS)
        }
        "verify" => {
            let t = Instant::now();
            let _lock = verify_lock(&ws)?;
            if o.all && !o.truck && history::truck_due(&ws.target_dir) {
                // 週に 1 回(2026-10-25 Kie)。truck の回は SMLib の回と同時に流すので、時間はほぼ延びない
                println!(
                    "== 記録で truck の回が通ってから 7 日を過ぎたので、truck の回も流します\
                     (落ちたら 1 件ずつ truck の弱さか JC の退行かを確かめ、弱さなら試験に \
                     kmb_contract::skip! を書いて xtask/skips.toml の [runtime] に理由を足す。docs/verify.md の 1 節)"
                );
                o.truck = true;
            }
            let mut results = vec![checks(&ws, &fork, &changed)];
            let _ = fs::remove_file(skip_log(&ws));
            let started = SystemTime::now();
            let testing = !sel.as_ref().is_some_and(BTreeSet::is_empty);
            if testing && !nextest_off() && !nextest_available() {
                install_on_linux("scripts/install-nextest.sh");
            }
            if !testing {
                // 文書だけの変更など。点検だけを流し、テストは「未実行」と書く(通過ではない)
                println!(
                    "\n== テスト: 未実行(テストする crate がない。変えたのは crate の外のファイルだけ)"
                );
            } else if nextest_available() {
                free_before_build(&ws);
                results.extend(verify_nextest(&o, &sel, &ws)?);
            } else if (o.all || o.dependents) && !nextest_off() {
                // nextest の無い回は件数が残らないので、手渡しやプッシュの前の検査には使わない
                println!(
                    "\n== テスト: cargo-nextest が無いので失敗にします(--all・--dependents は nextest で流す。\
                     入れ方は docs/verify.md の 1 節、クラウドは scripts/install-nextest.sh。\
                     nextest なしで流すなら JC_VERIFY_NEXTEST=off)"
                );
                let mut d = Done::new("テスト", false, 0.0);
                d.detail = Some("cargo-nextest が無い".into());
                results.push(d);
            } else {
                free_before_build(&ws);
                results.extend(verify_cargo_test(&o, &sel, &ws));
            }
            remember_hack(&ws, &results);
            results.extend(runtime_skips(&ws));
            let lock_changed = changed
                .iter()
                .any(|f| f == "Cargo.lock" || f.ends_with("Cargo.toml") || f == "deny.toml");
            if o.all || lock_changed {
                results.extend(deny(&ws, o.all));
                results.push(licenses_check(&ws));
                results.extend(hakari(&ws));
            }
            let scope = match &sel {
                Some(s) if s.is_empty() => "テストする crate なし".to_string(),
                _ => package_args(&sel).join(" "),
            };
            println!("\n== まとめ({scope})");
            for d in &results {
                let detail = d
                    .detail
                    .as_ref()
                    .map(|x| format!("({x})"))
                    .unwrap_or_default();
                println!(
                    "{} {} {:.1} 秒{detail}",
                    if d.ok { "通過" } else { "失敗" },
                    d.name,
                    d.secs
                );
            }
            let total = t.elapsed().as_secs_f64();
            println!("合わせて {total:.1} 秒");
            history::record(&ws.target_dir, rest, &results, total);
            sweep_if_low(&ws, &results, started, o.all);
            Ok(if results.iter().all(|d| d.ok) {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        other => Err(format!(
            "知らない道具です: {other}(affected か checks か verify か sweep か docs か api か dxc)"
        )),
    }
}

fn main() -> ExitCode {
    // ワークスペースの根で動かす(どこから呼んでも同じ結果にする)
    if let Some(root) = Path::new(env!("CARGO_MANIFEST_DIR")).parent() {
        let _ = std::env::set_current_dir(root);
    }
    run().unwrap_or_else(|e| {
        eprintln!("誤り: {e}");
        ExitCode::FAILURE
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(name: &str, dir: &str, deps: &[&str]) -> Package {
        Package {
            name: name.into(),
            dir: dir.into(),
            deps: deps.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    fn ws() -> Vec<Package> {
        vec![
            pkg("kmb-core", "crates/kmb-core", &[]),
            pkg("kmb-cam", "crates/kmb-cam", &["kmb-core"]),
            pkg("kmb-cam-commands", "crates/kmb-cam-commands", &["kmb-cam"]),
            pkg("kmb-ui", "crates/kmb-ui", &["kmb-core"]),
            pkg("kmb-app", "apps/kmb-app", &["kmb-ui", "kmb-cam-commands"]),
            pkg("smlib-sys", "crates/smlib-sys", &[]),
        ]
    }

    fn set(v: &[&str]) -> BTreeSet<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn files_are_given_to_the_crate_whose_folder_holds_them() {
        let files: Vec<String> = [
            "crates/kmb-cam/src/lib.rs",
            // 名前の始まりが同じ別の crate を取り違えない
            "crates/kmb-cam-commands/src/entry.rs",
            "crates\\kmb-ui\\src\\ribbon.rs",
            "third_party/smlibpy-patches/SmBool.cpp",
            "docs/plan.md",
        ]
        .map(String::from)
        .to_vec();
        let c = classify(&files, &ws());
        assert_eq!(
            c.packages,
            set(&["kmb-cam", "kmb-cam-commands", "kmb-ui", "smlib-sys"])
        );
        assert!(!c.everything);
        assert_eq!(c.unrelated, ["docs/plan.md"]);
        assert!(classify(&["Cargo.lock".into()], &ws()).everything);
        assert!(classify(&[".cargo/config.toml".into()], &ws()).everything);
        let mut w = ws();
        w.push(pkg("kmb-plugin-host", "crates/kmb-plugin-host", &[]));
        assert_eq!(
            classify(&["plugins/post-example.wasm".into()], &w).packages,
            set(&["kmb-plugin-host"])
        );
    }

    #[test]
    fn dependents_are_followed_through_other_crates() {
        assert_eq!(
            with_dependents(&set(&["kmb-cam"]), &ws()),
            set(&["kmb-cam", "kmb-cam-commands", "kmb-app"])
        );
        assert_eq!(with_dependents(&set(&["kmb-app"]), &ws()), set(&["kmb-app"]));
        assert_eq!(with_dependents(&set(&["kmb-core"]), &ws()).len(), 5);
    }

    #[test]
    fn truck_runs_only_the_shape_tests() {
        let mut w = ws();
        w.push(pkg("kmb-solid", "crates/kmb-solid", &[]));
        w.push(pkg("kmb-samples", "crates/kmb-samples", &[]));
        let all = truck_targets(&None, &w);
        assert_eq!(
            all,
            [("kmb-solid", None), ("kmb-samples", Some("hole_tests::"))]
        );
        assert_eq!(
            truck_filter(&all),
            "package(kmb-solid) | (package(kmb-samples) & test(/^hole_tests::/))"
        );
        // 選んだ crate に形の計算の試験がなければ流さない(画面や加工だけの変更)
        assert_eq!(
            truck_targets(&Some(set(&["kmb-core", "kmb-ui", "kmb-solid"])), &w),
            [("kmb-solid", None)]
        );
        assert!(truck_targets(&Some(set(&["kmb-ui", "kmb-app"])), &w).is_empty());
        // ワークスペースにない crate は数えない
        assert!(truck_targets(&None, &ws()).is_empty());
    }

    #[test]
    fn only_code_blocks_that_rustdoc_runs_count_as_doctests() {
        assert!(!has_doctest(
            "//! ```text\n//! cargo xtask verify\n//! ```\nfn f() {}"
        ));
        assert!(has_doctest("/// ```\n/// assert!(true);\n/// ```"));
        assert!(has_doctest("/// ```rust,no_run\n/// f();\n/// ```"));
        // 閉じの ``` を開きと取り違えない
        assert!(!has_doctest("/// ```json\n/// {}\n/// ```\n/// 説明"));
        assert!(has_doctest("#![doc = include_str!(\"../README.md\")]"));
        assert!(!has_doctest("let s = \"```\";"));
    }

    #[test]
    fn metadata_is_read_into_workspace_packages() {
        let meta = serde_json::json!({
            "workspace_root": "D:\\w",
            "workspace_members": ["a 0.1.0", "b 0.1.0"],
            "packages": [
                {"id": "a 0.1.0", "name": "kmb-a", "manifest_path": "D:\\w\\crates\\kmb-a\\Cargo.toml",
                 "metadata": {"kmb": {"layer": "cad"}},
                 "dependencies": [{"name": "serde", "kind": null}, {"name": "kmb-b", "kind": null},
                                  {"name": "kmb-b", "kind": "dev"}]},
                {"id": "b 0.1.0", "name": "kmb-b", "manifest_path": "D:\\w\\crates\\kmb-b\\Cargo.toml",
                 "metadata": null, "dependencies": []}
            ]
        });
        let got = parse_metadata(&meta);
        assert_eq!(got.len(), 2);
        assert_eq!(
            (got[0].name.as_str(), got[0].dir.as_str()),
            ("kmb-a", "crates/kmb-a")
        );
        assert_eq!(got[0].deps, ["kmb-b"]);
        assert_eq!(got[0].layer.as_deref(), Some("cad"));
        assert_eq!(
            got[0].edges,
            [
                Edge {
                    name: "kmb-b".into(),
                    kind: Kind::Normal
                },
                Edge {
                    name: "kmb-b".into(),
                    kind: Kind::Dev
                }
            ]
        );
        assert_eq!(got[1], pkg("kmb-b", "crates/kmb-b", &[]));
    }

    #[test]
    fn arguments_choose_crates_and_steps() {
        let a = |v: &[&str]| parse_args(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        let o = a(&["kmb-ui", "--dependents", "--no-clippy", "--base", "main"]).unwrap();
        assert_eq!(o.crates, ["kmb-ui"]);
        assert!(o.dependents && !o.clippy && !o.truck);
        assert_eq!(o.base, "main");
        assert_eq!(a(&[]).unwrap().base, "origin/main");
        assert!(a(&["--fast"]).is_err());
        assert_eq!(
            package_args(&Some(set(&["kmb-ui", "kmb-core"]))),
            ["-p", "kmb-core", "-p", "kmb-ui"]
        );
        assert_eq!(package_args(&None), ["--workspace"]);
    }
}
