//! `cargo xtask licenses`: 配る実行ファイルに入る他者の部品の許諾文を、1 つのファイルに集める。
//!
//! ```text
//! cargo xtask licenses [--target <三つ組>] [--out <ファイル>]   既定は今の PC の向きと target/release/THIRD-PARTY-LICENSES.txt
//! ```
//!
//! - 配る物は層 `app` の crate(画面・CLI・MCP の実行ファイル)。そこから通常の依存だけをたどり、
//!   マクロ(proc-macro)とその先、build スクリプト用、試験用は入れない(組み立ての時だけ動き、
//!   実行ファイルに入らない)。`cargo metadata --filter-platform` で、その向きで組み立てる依存だけを見る。
//! - 部品の許諾文は、crate の一番上にある LICENSE・COPYING・NOTICE などのファイルをそのまま載せる。
//!   GPL の名前の付くファイル(GPL との二択の部品が持つ)は載せない(選ぶのは GPL でない方)。
//!   ファイルを持たない crate は、宣言(`license`)の本文を `xtask/licenses/` の雛形から作り、
//!   著作者は `authors` から入れる。雛形の無い許諾の crate は誤りにする(新しい依存で気づく)。
//! - 同じ本文は 1 回だけ載せ、使う部品を並べる。
//! - crate でない部品(SMLib などの C++、Hershey のフォント、Rust の標準ライブラリ)は `EXTRAS` に
//!   書き、持ち主の crate が配る物に入る時だけ載せる。
//! - `cargo xtask release` が組み立てた後に実行ファイルの横へ置き、`verify --all` が作れることを確かめる。

use serde_json::Value as Json;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// 作るファイルの名前。
pub const FILE: &str = "THIRD-PARTY-LICENSES.txt";

/// 配る物の向き(PC で作る Windows 版)。`verify` はこれで確かめる。
pub const WINDOWS: &str = "x86_64-pc-windows-msvc";

/// 今の PC で組み立てる向き。
pub fn host_target() -> &'static str {
    if cfg!(windows) {
        WINDOWS
    } else {
        "x86_64-unknown-linux-gnu"
    }
}

const MIT: &str = include_str!("../licenses/LICENSE-MIT.txt");
const BSD2: &str = include_str!("../licenses/LICENSE-BSD-2-Clause.txt");
const BSL: &str = include_str!("../licenses/LICENSE-BSL-1.0.txt");
const LLVM_EXCEPTION: &str = include_str!("../licenses/LICENSE-LLVM-exception.txt");
const APACHE: &str = include_str!("../../LICENSE-APACHE");

/// crate でない部品。持ち主の crate が配る物に入る時だけ載せる。
struct Extra {
    owner: &'static str,
    name: &'static str,
    license: &'static str,
    /// ワークスペースの根からのパス。None は MIT の雛形に `copyright` を入れる
    file: Option<&'static str>,
    copyright: &'static str,
}

const EXTRAS: &[Extra] = &[
    Extra {
        owner: "jc-core",
        name: "Rust の標準ライブラリ (Rust standard library)",
        license: "MIT OR Apache-2.0",
        file: None,
        copyright: "The Rust Project Developers",
    },
    Extra {
        owner: "smlib-sys",
        name: "SMLib・SM_API (usd-brep 1.0.0、C++)",
        license: "Apache-2.0",
        file: Some("third_party/usd-brep/LICENSE"),
        copyright: "",
    },
    Extra {
        owner: "smlib-sys",
        name: "SMLib の不具合を直した写し (smlibpy、C++)",
        license: "Apache-2.0 AND MIT",
        file: Some("third_party/smlibpy-patches/LICENSE"),
        copyright: "",
    },
    Extra {
        owner: "jc-text",
        name: "Hershey フォント (Roman Simplex・Futura Light)",
        license: "Hershey fonts license",
        file: Some("crates/jc-text/fonts/HERSHEY-LICENSE.txt"),
        copyright: "",
    },
];

/// 一番上に許諾のファイルを持たず、下のフォルダに置く crate。
const NESTED: &[(&str, &[&str])] = &[(
    "epaint_default_fonts",
    &[
        "fonts/Hack-Regular.txt",
        "fonts/UFL.txt",
        "fonts/OFL.txt",
        "fonts/emoji-icon-font-mit-license.txt",
    ],
)];

/// リポジトリに置いた写し(`[patch.crates-io]`)の注記。
const COPIES: &[(&str, &str)] = &[(
    "truck-shapeops",
    "このソフト用に一部を変えた写し (modified): 殻の内外の数え方と、release の警告の扱い",
)];

/// 配る物に入る他者の crate 1 つ。
#[derive(Debug, Clone, PartialEq)]
pub struct Crate {
    pub name: String,
    pub version: String,
    pub license: String,
    pub authors: Vec<String>,
    pub dir: PathBuf,
    pub license_file: Option<String>,
    /// crates.io でなく、リポジトリのパスから入る
    pub local: bool,
}

/// `cargo metadata`(依存つき)の出力から、配る物の他者の crate と、配る物に入る
/// ワークスペースの crate の名前を返す。
pub fn shipped(meta: &Json) -> (Vec<Crate>, BTreeSet<String>) {
    let empty = Vec::new();
    let packages: BTreeMap<&str, &Json> = meta["packages"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .filter_map(|p| Some((p["id"].as_str()?, p)))
        .collect();
    let members: BTreeSet<&str> = meta["workspace_members"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .filter_map(Json::as_str)
        .collect();
    let nodes: BTreeMap<&str, &Json> = meta["resolve"]["nodes"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .filter_map(|n| Some((n["id"].as_str()?, n)))
        .collect();
    let proc_macro = |id: &str| {
        packages[id]["targets"]
            .as_array()
            .unwrap_or(&empty)
            .iter()
            .any(|t| {
                t["kind"]
                    .as_array()
                    .is_some_and(|k| k.iter().any(|k| k == "proc-macro"))
            })
    };
    let mut todo: Vec<&str> = members
        .iter()
        .copied()
        .filter(|id| packages[id]["metadata"]["jc"]["layer"] == "app")
        .collect();
    let mut seen = BTreeSet::new();
    while let Some(id) = todo.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(node) = nodes.get(id) else { continue };
        for d in node["deps"].as_array().unwrap_or(&empty) {
            let normal = d["dep_kinds"]
                .as_array()
                .is_some_and(|k| k.iter().any(|k| k["kind"].is_null()));
            let Some(dep) = d["pkg"].as_str() else {
                continue;
            };
            if normal && packages.contains_key(dep) && !proc_macro(dep) {
                todo.push(dep);
            }
        }
    }
    let mut out = Vec::new();
    let mut own = BTreeSet::new();
    for id in seen {
        let p = packages[id];
        let name = p["name"].as_str().unwrap_or_default().to_string();
        if members.contains(id) {
            own.insert(name);
            continue;
        }
        let manifest = PathBuf::from(p["manifest_path"].as_str().unwrap_or_default());
        out.push(Crate {
            name,
            version: p["version"].as_str().unwrap_or_default().into(),
            license: p["license"].as_str().unwrap_or_default().into(),
            authors: p["authors"]
                .as_array()
                .unwrap_or(&empty)
                .iter()
                .filter_map(Json::as_str)
                .map(String::from)
                .collect(),
            dir: manifest.parent().map(Path::to_path_buf).unwrap_or_default(),
            license_file: p["license_file"].as_str().map(String::from),
            local: p["source"].is_null(),
        });
    }
    out.sort_by(|a, b| {
        (a.name.to_lowercase(), &a.version).cmp(&(b.name.to_lowercase(), &b.version))
    });
    (out, own)
}

/// 許諾のファイルとして載せる名前か(大小は無視。GPL の物は載せない)。
fn is_license_file(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ["licen", "copying", "copyright", "notice", "unlicense"]
        .iter()
        .any(|p| n.starts_with(p))
        && !n.contains("gpl")
}

/// 本文をそろえる(改行を LF に、行末の空白と前後の空行を除く)。同じ本文を 1 つにまとめるため。
fn normalize(text: &str) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    lines.join("\n").trim_matches('\n').to_string()
}

/// 著作者の一覧を、メールを除いて 1 行にする。
fn copyright(authors: &[String], name: &str) -> String {
    let names: Vec<String> = authors
        .iter()
        .map(|a| a.split('<').next().unwrap_or(a).trim().to_string())
        .filter(|a| !a.is_empty())
        .collect();
    if names.is_empty() {
        format!("the {name} authors")
    } else {
        names.join(", ")
    }
}

/// 許諾の 1 つの名前の本文を雛形から作る。雛形が無ければ None。
fn template(id: &str, holder: &str) -> Option<String> {
    Some(match id {
        "MIT" => MIT.replace("{copyright}", holder),
        "BSD-2-Clause" => BSD2.replace("{copyright}", holder),
        "BSL-1.0" => BSL.to_string(),
        "Apache-2.0" => APACHE.to_string(),
        "Apache-2.0 WITH LLVM-exception" => format!("{APACHE}\n\n{LLVM_EXCEPTION}"),
        _ => return None,
    })
}

/// 宣言(SPDX の式)から、載せる本文を雛形で作る。`A AND B` は両方、`A OR B` は雛形のある最初の物。
/// 雛形の無い項は `missing` に入れる。
fn from_expression(expr: &str, holder: &str, missing: &mut Vec<String>) -> Vec<(String, String)> {
    let flat = expr.replace(['(', ')'], " ").replace('/', " OR ");
    let mut out = Vec::new();
    for term in flat.split(" AND ") {
        let options: Vec<String> = term
            .split(" OR ")
            .map(|o| o.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|o| !o.is_empty())
            .collect();
        match options
            .iter()
            .find_map(|o| Some((o.clone(), template(o, holder)?)))
        {
            Some((id, text)) => out.push((id, text)),
            None => missing.push(term.trim().to_string()),
        }
    }
    out
}

/// crate 1 つの本文(ファイルの名前か許諾の名前、本文)。載せられない時は誤りの文。
pub fn texts(c: &Crate) -> Result<Vec<(String, String)>, String> {
    let read = |p: &Path| fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()));
    let mut out = Vec::new();
    let mut names: Vec<String> = fs::read_dir(&c.dir)
        .map_err(|e| format!("{} {}: {e}", c.name, c.dir.display()))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| is_license_file(n))
        .collect();
    if let Some(f) = &c.license_file
        && !names.contains(f)
    {
        names.push(f.clone());
    }
    names.sort();
    for n in names {
        out.push((n.clone(), read(&c.dir.join(&n))?));
    }
    let nested = NESTED
        .iter()
        .find(|(n, _)| *n == c.name)
        .map_or(&[][..], |(_, f)| *f);
    for f in nested {
        out.push((f.to_string(), read(&c.dir.join(f))?));
    }
    if out.is_empty() || (!nested.is_empty() && !out.iter().any(|(n, _)| is_license_file(n))) {
        let mut missing = Vec::new();
        out.extend(from_expression(
            &c.license,
            &copyright(&c.authors, &c.name),
            &mut missing,
        ));
        // 下のフォルダの本文がある crate は、雛形の無い項(フォントの許諾)をそちらで賄う
        if !missing.is_empty() && nested.is_empty() {
            return Err(format!(
                "{} {}: 許諾のファイルが無く、{} の雛形もありません(xtask/licenses/ に足す)",
                c.name,
                c.version,
                missing.join("・")
            ));
        }
    }
    Ok(out)
}

/// 一覧の 1 行(部品・版・許諾・注記)と本文。
struct Entry {
    label: String,
    license: String,
    note: String,
    texts: Vec<(String, String)>,
}

/// 一覧の本文を作る。`root` はワークスペースの根(本体と EXTRAS の本文を読む)。
pub fn render(
    root: &Path,
    target: &str,
    crates: &[Crate],
    own: &BTreeSet<String>,
) -> Result<String, Vec<String>> {
    let mut problems = Vec::new();
    let read = |rel: &str, problems: &mut Vec<String>| {
        fs::read_to_string(root.join(rel))
            .map_err(|e| problems.push(format!("{rel}: {e}")))
            .unwrap_or_default()
    };
    let mut entries = vec![Entry {
        label: "このソフト本体と自作の素材 (this software)".into(),
        license: "MIT OR Apache-2.0".into(),
        note: "アプリのアイコン・機械の台の穴の位置のデータ・陰影の式などの自作の素材を含む".into(),
        texts: vec![
            ("LICENSE-MIT".into(), read("LICENSE-MIT", &mut problems)),
            (
                "LICENSE-APACHE".into(),
                read("LICENSE-APACHE", &mut problems),
            ),
        ],
    }];
    for x in EXTRAS.iter().filter(|x| own.contains(x.owner)) {
        let text = match x.file {
            Some(f) => read(f, &mut problems),
            None => MIT.replace("{copyright}", x.copyright),
        };
        let mut texts = vec![(x.file.unwrap_or("MIT").to_string(), text)];
        if x.file.is_none() {
            texts.push(("Apache-2.0".into(), APACHE.into()));
        }
        entries.push(Entry {
            label: x.name.into(),
            license: x.license.into(),
            note: String::new(),
            texts,
        });
    }
    for c in crates {
        let note = COPIES
            .iter()
            .find(|(n, _)| *n == c.name)
            .map(|(_, n)| n.to_string())
            .unwrap_or_default();
        if c.local && note.is_empty() {
            problems.push(format!(
                "{}: リポジトリから当てた写しなのに注記がありません(licenses.rs の COPIES)",
                c.name
            ));
        }
        match texts(c) {
            Ok(texts) => entries.push(Entry {
                label: format!("{} {}", c.name, c.version),
                license: c.license.clone(),
                note,
                texts,
            }),
            Err(e) => problems.push(e),
        }
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    // 同じ本文を 1 つにまとめる(最初に出た順)
    let mut order: Vec<String> = Vec::new();
    let mut users: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for e in &entries {
        for (_, t) in &e.texts {
            let t = normalize(t);
            if !users.contains_key(&t) {
                order.push(t.clone());
            }
            let u = users.entry(t).or_default();
            if !u.contains(&e.label) {
                u.push(e.label.clone());
            }
        }
    }
    let mut s = String::new();
    s.push_str("他者の部品の許諾文 (Third-party licenses)\n");
    s.push_str("==========================================\n\n");
    s.push_str(&format!(
        "このソフトの実行ファイル({target} 向け)に入る部品と、その許諾文です。\n\
         This file lists the components built into the executables of this software ({target})\n\
         and reproduces their license texts. `cargo xtask licenses` が作る。手で直さない。\n\n\
         部品の数 (components): {}\n\n",
        entries.len()
    ));
    s.push_str("1. 部品の一覧 (Components)\n--------------------------\n\n");
    for e in &entries {
        s.push_str(&format!("- {}: {}", e.label, e.license));
        if !e.note.is_empty() {
            s.push_str(&format!(" — {}", e.note));
        }
        s.push('\n');
    }
    s.push_str("\n2. 許諾文 (License texts)\n-------------------------\n");
    for t in &order {
        s.push_str(&format!(
            "\n================================================================\n\
             使う部品 (used by): {}\n\
             ================================================================\n\n{t}\n",
            users[t].join(", ")
        ));
    }
    Ok(s)
}

/// `cargo metadata` を `target` の向きで読み、一覧を作る。
pub fn build(
    root: &Path,
    target: &str,
    cargo: &dyn Fn() -> Command,
) -> Result<(String, usize), Vec<String>> {
    let mut c = cargo();
    c.args([
        "metadata",
        "--format-version",
        "1",
        "--filter-platform",
        target,
    ]);
    let out = c.output().map_err(|e| vec![format!("{c:?}: {e}")])?;
    if !out.status.success() {
        return Err(vec![format!(
            "{c:?}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )]);
    }
    let meta: Json = serde_json::from_slice(&out.stdout).map_err(|e| vec![e.to_string()])?;
    let (crates, own) = shipped(&meta);
    let text = render(root, target, &crates, &own)?;
    Ok((text, crates.len()))
}

/// 作って `out` に書く。書いた部品の数を返す。
pub fn write(
    root: &Path,
    target: &str,
    out: &Path,
    cargo: &dyn Fn() -> Command,
) -> Result<usize, String> {
    let (text, n) = build(root, target, cargo).map_err(|p| p.join("\n"))?;
    if let Some(d) = out.parent() {
        fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    fs::write(out, text).map_err(|e| format!("{}: {e}", out.display()))?;
    Ok(n)
}

/// `cargo xtask licenses [--target <三つ組>] [--out <ファイル>]`。
pub fn run(
    root: &Path,
    target_dir: &Path,
    args: &[String],
    cargo: &dyn Fn() -> Command,
) -> Result<(), String> {
    let mut target = host_target().to_string();
    let mut out = target_dir.join("release").join(FILE);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let v = || {
            format!(
                "{a} の後に値が要ります(使い方: cargo xtask licenses [--target <三つ組>] [--out <ファイル>])"
            )
        };
        match a.as_str() {
            "--target" => target = it.next().ok_or_else(v)?.clone(),
            "--out" => out = PathBuf::from(it.next().ok_or_else(v)?),
            _ => return Err(format!("知らない引数: {a}")),
        }
    }
    let n = write(root, &target, &out, cargo)?;
    println!("書いた: {}(他者の crate {n} 件)", out.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pkg(id: &str, kind: &str, layer: Option<&str>) -> Json {
        json!({"id": id, "name": id, "version": "1.0.0", "license": "MIT",
               "authors": ["A B <a@b>"], "manifest_path": format!("/r/{id}/Cargo.toml"),
               "source": "registry", "license_file": null,
               "targets": [{"kind": [kind]}],
               "metadata": layer.map(|l| json!({"jc": {"layer": l}}))})
    }

    fn dep(id: &str, kind: Option<&str>) -> Json {
        json!({"pkg": id, "dep_kinds": [{"kind": kind}]})
    }

    #[test]
    fn only_normal_deps_outside_proc_macros_ship() {
        let meta = json!({
            "packages": [pkg("app", "bin", Some("app")), pkg("lib", "lib", Some("cad")),
                         pkg("tool", "bin", Some("support")), pkg("a", "lib", None),
                         pkg("mac", "proc-macro", None), pkg("syn", "lib", None),
                         pkg("cc", "lib", None), pkg("test", "lib", None), pkg("x", "lib", None)],
            "workspace_members": ["app", "lib", "tool"],
            "resolve": {"nodes": [
                {"id": "app", "deps": [dep("lib", None), dep("cc", Some("build")), dep("test", Some("dev"))]},
                {"id": "lib", "deps": [dep("a", None), dep("mac", None)]},
                {"id": "mac", "deps": [dep("syn", None)]},
                {"id": "tool", "deps": [dep("x", None)]},
                {"id": "a", "deps": []}, {"id": "syn", "deps": []}, {"id": "cc", "deps": []},
                {"id": "test", "deps": []}, {"id": "x", "deps": []}]}
        });
        let (crates, own) = shipped(&meta);
        let names: Vec<&str> = crates.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["a"]);
        assert_eq!(own.into_iter().collect::<Vec<_>>(), ["app", "lib"]);
        assert_eq!(crates[0].dir, PathBuf::from("/r/a"));
    }

    #[test]
    fn license_file_names_skip_gpl() {
        for n in [
            "LICENSE-MIT",
            "license-apache-2.0",
            "COPYING",
            "NOTICE",
            "UNLICENSE",
            "LICENCE",
        ] {
            assert!(is_license_file(n), "{n}");
        }
        for n in [
            "LICENSE-GPLv2",
            "COPYING.LGPLv2.1",
            "README.md",
            "Cargo.toml",
        ] {
            assert!(!is_license_file(n), "{n}");
        }
    }

    #[test]
    fn expressions_pick_one_of_or_and_all_of_and() {
        let mut missing = Vec::new();
        let t = from_expression("MIT OR Apache-2.0", "X", &mut missing);
        assert_eq!(t.len(), 1);
        assert!(t[0].1.contains("Copyright (c) X"));
        let t = from_expression("Apache-2.0/MIT", "X", &mut missing);
        assert_eq!(t[0].0, "Apache-2.0");
        let t = from_expression("Apache-2.0 WITH LLVM-exception", "X", &mut missing);
        assert!(t[0].1.contains("LLVM Exceptions"));
        let t = from_expression("(MIT OR Apache-2.0) AND OFL-1.1", "X", &mut missing);
        assert_eq!(t.len(), 1);
        assert_eq!(missing, ["OFL-1.1"]);
    }

    #[test]
    fn copyright_drops_mail_and_falls_back_to_the_name() {
        assert_eq!(copyright(&["A B <a@b>".into(), "C".into()], "x"), "A B, C");
        assert_eq!(copyright(&[], "x"), "the x authors");
    }

    #[test]
    fn same_text_is_listed_once_with_all_users() {
        let dir = std::env::temp_dir().join(format!("jc-licenses-{}", std::process::id()));
        let root = dir.join("root");
        for (d, files) in [
            (
                "root",
                &[("LICENSE-MIT", "own mit"), ("LICENSE-APACHE", "own apache")][..],
            ),
            ("a", &[("LICENSE", "same\r\ntext  \n")][..]),
            ("b", &[("LICENSE-MIT", "same\ntext\n\n")][..]),
            ("c", &[][..]),
        ] {
            fs::create_dir_all(dir.join(d)).unwrap();
            for (f, t) in files {
                fs::write(dir.join(d).join(f), t).unwrap();
            }
        }
        let c = |n: &str, lic: &str| Crate {
            name: n.into(),
            version: "1.0.0".into(),
            license: lic.into(),
            authors: vec![],
            dir: dir.join(n),
            license_file: None,
            local: false,
        };
        let text = render(
            &root,
            WINDOWS,
            &[c("a", "MIT"), c("b", "MIT"), c("c", "BSD-2-Clause")],
            &BTreeSet::new(),
        )
        .unwrap();
        assert_eq!(text.matches("same\ntext").count(), 1);
        assert!(text.contains("使う部品 (used by): a 1.0.0, b 1.0.0"));
        assert!(text.contains("Copyright (c) the c authors"));
        assert!(text.contains("- c 1.0.0: BSD-2-Clause"));
        let bad = render(&root, WINDOWS, &[c("c", "Zlib")], &BTreeSet::new()).unwrap_err();
        assert!(bad[0].contains("Zlib の雛形"), "{bad:?}");
        let _ = fs::remove_dir_all(&dir);
    }
}
