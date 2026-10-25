//! 他製品の名前の点検: リポジトリのファイルに、ほかの CAD・CAM・解析のソフトと作り手の名前が
//! 入ったら落とす。
//!
//! 公開する物に他製品の名前と調査元(調べた資料・出典)を書かない決まり(AGENTS.md 2 節の 7)を
//! 字面で守る。比べた記録・調べた資料はリポジトリの外の開発の記録に置く。大小は無視し、英数字の
//! 続きの途中(`confusion` の fusion など)は数えない。`_` は区切りに数えるので、識別子の中の
//! 名前(`from_xxx`)も落ちる。
//!
//! 許すのは互換に要る所だけ(`ALLOWED_FILES`・`ALLOWED_PHRASES`)。他者のコード(third_party/)と
//! ライセンスの文、文字でないファイルは見ない。機械の名前(Carvera Air)とポストの形式の名前
//! (Grbl・LightBurn など)は互換の名前なので語の一覧に入れない。
//!
//! 調べた資料の出典(百科事典・論文の置き場・質問の場・動画・販売店など。`SOURCES`)も落とす
//! (8-4、2026-10-24)。値の出典はリポジトリの外の控えに置き、コードには値と働きだけを書く。
//!
//! 特許の点検の記録(特許の番号と持ち主)も公開する物に載せないので、特許の番号の形(`US` などの
//! 国の印に 6 桁以上の数字)と、調べた特許の持ち主の名前も落とす。避ける形の決まりだけを
//! docs/避ける形.md に書く。
//!
//! 公開しない資料(リポジトリの外の開発の記録: 調べた資料・設計の控え・決定の台帳・引継ぎ・写真)を
//! 指す書き方も落とす(2026-10-25)。`ops/`・`proposals/` の道筋、`design/` の日付つきの置き場と
//! 資料のファイル、作業場の共有の置き場の写真、Artifact の URL がそれ。働きと理由はリポジトリの
//! 中に書き、外の資料への道筋だけを書かない。クラウドの作業場の検査の控え(`.cache/` と検査の
//! 記録)の置き場は、設定の値として使うので許す。
//!
//! 押せばコミットの説明も公開になるので、分かれ目から先のコミットの説明も同じ決まりで見る
//! (`check_commits`)。取り込みの検査で押す前に止まる。

use std::path::Path;
use std::process::Command;

/// 落とす名前(小文字)。
const WORDS: &[&str] = &[
    "fusion",
    "autodesk",
    "autocad",
    "solidcraft",
    "solvecraft",
    "solidworks",
    "onshape",
    "freecad",
    "inventor",
    "mastercam",
    "solidcam",
    "imachining",
    "truemill",
    "blender",
    "plasticity",
    "shapr3d",
    "vectric",
    "estlcam",
    "deskproto",
    "opencamlib",
    "bcnc",
    "siemens",
    "creo",
    "catia",
    "rhino",
    "sketchup",
    "kicad",
    "spaceclaim",
    "solvespace",
    "ansys",
    "abaqus",
    "comsol",
    "calculix",
    "bambu",
    "prusa",
    "cura",
    "mcmaster",
    "traceparts",
    "makera",
    // 特許の持ち主として調べた作り手
    "dassault",
    "bricsys",
    "boeing",
    "makino",
    "merl",
    "celeritive",
    "cimatron",
    "open mind",
    "cnc software",
    "dp technology",
];

/// 落とす出典の名前(小文字)。調べた資料・研究・販売店の名前。
const SOURCES: &[&str] = &[
    "wikipedia",
    "ウィキペディア",
    "arxiv",
    "researchgate",
    "sciencedirect",
    "stackoverflow",
    "stack overflow",
    "qiita",
    "zenn",
    "reddit",
    "youtube",
    "amazon",
    "monotaro",
    "モノタロウ",
    "core electronics",
];

/// 名前を書いてよいファイルと理由。
const ALLOWED_FILES: &[(&str, &str)] = &[
    (
        "crates/kmb-cam-tool/src/exchange.rs",
        "ほかの CAM の工具ライブラリの形式を読み書きする(保存したデータの鍵の名前も残す)",
    ),
    (
        "crates/smlib-sys/build.rs",
        "取り込んだ形状カーネルの部品の名前(他者の識別子)",
    ),
    (
        "xtask/removed-tests.toml",
        "名前を変えたテストの元の名前(消えたテストの点検が引く)",
    ),
    ("xtask/src/layers/code.rs", "機械の名前の点検の語の一覧"),
    ("xtask/src/products.rs", "この点検の語の一覧"),
];

/// 名前を含んでよい言い回し(小文字)。ファイルの形式の名前など、互換に要る物。
const ALLOWED_PHRASES: &[&str] = &["autocad binary dxf"];

/// 点検の誤りの文。
pub fn check(root: &Path) -> Vec<String> {
    let listed = Command::new("git")
        .args([
            "-c",
            "core.quotepath=off",
            "ls-files",
            "-co",
            "--exclude-standard",
        ])
        .current_dir(root)
        .output();
    let Ok(listed) = listed else {
        return vec!["他製品の名前: git ls-files を動かせません".into()];
    };
    let mut out = vec![];
    for path in String::from_utf8_lossy(&listed.stdout).lines() {
        if skipped(path) {
            continue;
        }
        // 消したが git に残る物と、文字でないファイルは見ない
        let Ok(text) = std::fs::read_to_string(root.join(path)) else {
            continue;
        };
        if ALLOWED_FILES.iter().any(|(f, _)| *f == path) {
            // 名前を書いてよいファイルでも、公開しない資料は指さない
            if path != "xtask/src/products.rs" {
                out.extend(private_docs(path, &text));
            }
        } else {
            out.extend(problems(path, &text));
        }
    }
    out
}

/// 分かれ目 `fork` から先のコミットの説明の誤りの文。
pub fn check_commits(fork: &str) -> Vec<String> {
    let listed = Command::new("git")
        .args(["log", "--format=%h%x1f%B%x1e", &format!("{fork}..HEAD")])
        .output();
    let Ok(listed) = listed else {
        return vec!["他製品の名前: git log を動かせません".into()];
    };
    commit_problems(&String::from_utf8_lossy(&listed.stdout))
}

/// `git log --format=%h%x1f%B%x1e` の出力の中の誤りの文。
fn commit_problems(log: &str) -> Vec<String> {
    let mut out = vec![];
    for entry in log.split('\x1e') {
        let Some((hash, body)) = entry.trim_start_matches('\n').split_once('\x1f') else {
            continue;
        };
        out.extend(problems(&format!("コミット {hash} の説明"), body));
    }
    out
}

/// 見ないファイル。
fn skipped(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    path.starts_with("third_party/")
        || name.starts_with("LICENSE")
        || name == "Cargo.lock"
        || [".step", ".stp", ".wasm", ".png", ".jpg", ".ico"]
            .iter()
            .any(|e| name.to_ascii_lowercase().ends_with(e))
}

/// 1 つのファイルの中の名前。
pub fn problems(path: &str, text: &str) -> Vec<String> {
    let mut out = vec![];
    for (i, line) in text.lines().enumerate() {
        let line = line.trim_end_matches('\r');
        let mut lower = line.to_lowercase();
        for p in ALLOWED_PHRASES {
            lower = lower.replace(p, "");
        }
        if let Some(n) = patent_number_in(&lower) {
            out.push(format!(
                "{path}:{}: 特許の番号「{n}」がある。特許の点検の記録はリポジトリの外の開発の記録に置き、\
                 ここには避ける形の決まり(docs/避ける形.md)だけを書く",
                i + 1
            ));
        }
        for w in sources_in(&lower) {
            out.push(format!(
                "{path}:{}: 出典の名前「{w}」がある。値の出典はリポジトリの外の控えに置き、\
                 ここには値と働きだけを書く",
                i + 1
            ));
        }
        for w in names_in(&lower) {
            out.push(format!(
                "{path}:{}: 他製品の名前「{w}」がある。比べた記録や出典はリポジトリの外の開発の記録に置き、\
                 ここには働きだけを書く(互換に要る物は xtask/src/products.rs の許す一覧へ)",
                i + 1
            ));
        }
    }
    out.extend(private_docs(path, text));
    out
}

/// 1 つのファイルの中の、公開しない資料を指す書き方。
fn private_docs(path: &str, text: &str) -> Vec<String> {
    let mut out = vec![];
    for (i, line) in text.lines().enumerate() {
        for d in private_doc_in(line.trim_end_matches('\r')) {
            out.push(format!(
                "{path}:{}: 公開しない資料を指す「{d}」がある。働きと理由をここに書き、\
                 リポジトリの外の資料への道筋は書かない",
                i + 1
            ));
        }
    }
    out
}

/// 資料のファイルの拡張子(`design/` の下でこれで終わる物は資料)。
const DOC_EXTENSIONS: [&str; 8] = [
    ".md", ".html", ".ps1", ".py", ".png", ".jpg", ".json", ".csv",
];

/// 作業場の共有の置き場。
const SHARED_ROOT: &str = "/mnt/project-files/";

/// 共有の置き場の下で、設定の値として使ってよい物。
const SHARED_ALLOWED: [&str; 2] = [".cache/", "ops/verify/history.md"];

/// 行の中の、公開しない資料を指す書き方。
fn private_doc_in(line: &str) -> Vec<String> {
    // 道筋の続きの字(区切りの字で止める)
    let token = |s: &str| -> String {
        s.chars()
            .take_while(|c| !c.is_whitespace() && !"()（）、。,`'\"[]<>「」|".contains(*c))
            .collect()
    };
    let mut found = vec![];
    for url in ["claude.ai/artifact", "claude.ai/code/artifact"] {
        if line.contains(url) {
            found.push(url.to_string());
        }
    }
    for (at, _) in line.match_indices(SHARED_ROOT) {
        let rest = &line[at + SHARED_ROOT.len()..];
        if !SHARED_ALLOWED.iter().any(|a| rest.starts_with(a)) {
            found.push(token(&line[at..]));
        }
    }
    // 道筋の途中(`hole_ops/`・`/mnt/…/ops/` など)は数えない
    let starts = |at: usize| {
        !line[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_ascii_alphanumeric() || "_./-".contains(c))
    };
    for dir in ["ops/", "proposals/"] {
        for (at, _) in line.match_indices(dir) {
            if starts(at) {
                found.push(token(&line[at..]));
            }
        }
    }
    for (at, _) in line.match_indices("design/") {
        if !starts(at) {
            continue;
        }
        let t = token(&line[at..]);
        let rest = &t["design/".len()..];
        // 作業空間の名前の `design/…`(設定の頁・ピンの鍵・kmb-ui のフォルダ)は数えない
        if rest.starts_with(|c: char| c.is_ascii_digit())
            || rest.starts_with("特許")
            || DOC_EXTENSIONS
                .iter()
                .any(|e| rest.to_ascii_lowercase().ends_with(e))
        {
            found.push(t);
        }
    }
    found
}

/// 特許の番号を示す国の印(小文字)。
const PATENT_PREFIXES: [&str; 5] = ["us", "ep", "jp", "wo", "re"];

/// 小文字にした行の中の特許の番号(国の印の後に、空白か `-` を挟んでよく 6 桁以上の数字)。
fn patent_number_in(lower: &str) -> Option<String> {
    let word_char = |c: char| c.is_ascii_alphanumeric();
    for p in PATENT_PREFIXES {
        for (at, _) in lower.match_indices(p) {
            if lower[..at].chars().next_back().is_some_and(word_char) {
                continue;
            }
            let rest = &lower[at + p.len()..];
            let rest = rest.strip_prefix([' ', '-']).unwrap_or(rest);
            let digits: String = rest
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == ',')
                .collect();
            if digits.chars().filter(char::is_ascii_digit).count() >= 6 {
                return Some(format!("{}{}", p.to_uppercase(), digits));
            }
        }
    }
    None
}

/// 小文字にした行の中の名前(英数字の続きの途中は数えない)。
fn names_in(lower: &str) -> Vec<&'static str> {
    words_in(lower, WORDS)
}

/// 小文字にした行の中の出典の名前。
fn sources_in(lower: &str) -> Vec<&'static str> {
    words_in(lower, SOURCES)
}

fn words_in(lower: &str, words: &[&'static str]) -> Vec<&'static str> {
    let word_char = |c: char| c.is_ascii_alphanumeric();
    let mut found = vec![];
    for w in words {
        let hit = lower.match_indices(w).any(|(at, _)| {
            let before = lower[..at].chars().next_back();
            let after = lower[at + w.len()..].chars().next();
            !before.is_some_and(word_char) && !after.is_some_and(word_char)
        });
        if hit {
            found.push(*w);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_found_in_comments_and_identifiers_but_not_inside_words() {
        assert_eq!(names_in("// fusion と同じ"), vec!["fusion"]);
        assert_eq!(names_in("fn from_fusion() {}"), vec!["fusion"]);
        assert_eq!(
            names_in("solidworks・onshape"),
            vec!["solidworks", "onshape"]
        );
        assert!(names_in("confusion diffusion accurate procurable").is_empty());
        assert!(names_in("carvera air の grbl").is_empty());
    }

    #[test]
    fn allowed_phrases_are_not_reported_but_patent_sections_are() {
        assert!(problems("crates/kmb-io-dxf/src/lib.rs", "b\"AutoCAD Binary DXF\"").is_empty());
        // 特許の点検の記録も公開する物に載せないので、断片の中でも許さない
        let frag = "# 題\n\n## 作った物\n\n働き。\n\n## 特許の点検\n\nMakino の特許。\n";
        let ps = problems("docs/機能/2026-10-24-題-abcdef.md", frag);
        assert_eq!(ps.len(), 1, "{ps:?}");
        assert!(ps[0].contains(":9:") && ps[0].contains("makino"), "{ps:?}");
        assert_eq!(names_in("open mind の特許"), vec!["open mind"]);
    }

    #[test]
    fn commit_messages_are_checked() {
        let log = "abc1234\x1f題\n\nFusion と同じ\n\x1e\ndef5678\x1f別の題\n\nops/handoff/x.md を見た\n\x1e\n9999999\x1fclaude/x を取り込み\n\x1e";
        let ps = commit_problems(log);
        assert_eq!(ps.len(), 2, "{ps:?}");
        assert!(ps[0].starts_with("コミット abc1234 の説明:3:") && ps[0].contains("fusion"));
        assert!(ps[1].starts_with("コミット def5678 の説明:3:") && ps[1].contains("ops/handoff"));
    }

    #[test]
    fn sources_are_found_but_not_inside_words() {
        assert_eq!(
            sources_in("// 値は wikipedia の表(core electronics の仕様)"),
            vec!["wikipedia", "core electronics"]
        );
        assert_eq!(sources_in("ウィキペディアの表"), vec!["ウィキペディア"]);
        assert!(sources_in("amazonite xzenn ゼン").is_empty());
        let ps = problems("crates/kmb-x/src/lib.rs", "a\n/// 出典は Wikipedia\n");
        assert!(
            ps.len() == 1 && ps[0].contains(":2:") && ps[0].contains("出典"),
            "{ps:?}"
        );
    }

    #[test]
    fn patent_numbers_are_found() {
        assert_eq!(
            patent_number_in("us8584012 の請求項"),
            Some("US8584012".into())
        );
        assert_eq!(
            patent_number_in("(us 7,451,013)"),
            Some("US7,451,013".into())
        );
        assert_eq!(
            patent_number_in("re44,123 と ep-1234567"),
            Some("EP1234567".into())
        );
        assert!(patent_number_in("status123456 us12 bus1234567").is_none());
    }

    #[test]
    fn private_docs_are_found_but_workspace_paths_and_caches_are_not() {
        let ops = format!("{}/", "ops");
        let design = format!("{}/", "design");
        let shared = SHARED_ROOT;
        for (line, want) in [
            (
                format!("台帳 {ops}決定/a.md の R3"),
                format!("{ops}決定/a.md"),
            ),
            (
                format!("(引継ぎ `{ops}handoff/x.md`)"),
                format!("{ops}handoff/x.md"),
            ),
            (
                format!("写真は {design}2026-10-21-画面/段3/。"),
                format!("{design}2026-10-21-画面/段3/"),
            ),
            (
                format!("試作({design}画面構成/screens.html)の値"),
                format!("{design}画面構成/screens.html"),
            ),
            (
                format!("共有フォルダ `{design}特許の点検/機能/`"),
                format!("{design}特許の点検/機能/"),
            ),
            (
                format!("- 写真: {shared}screens/a.png"),
                format!("{shared}screens/a.png"),
            ),
            (
                format!("https://claude.ai/{}/abc", "artifact"),
                "claude.ai/artifact".into(),
            ),
        ] {
            assert_eq!(private_doc_in(&line), vec![want], "{line}");
        }
        for line in [
            r#"page(Tab::Design, "design/view", "3D の表示")"#,
            r#""design/スケッチ/直す": ["trim"]"#,
            "kmb-ui の `design/`・`assembly/` に分けた",
            "crates/kmb-strategy-drill/src/hole_ops/mod.rs",
            "find(\"design/sketch.display\")",
        ] {
            assert!(private_doc_in(line).is_empty(), "{line}");
        }
        for line in [
            format!("SMLIB_SHARED_CACHE = \"{shared}.cache/smlib-sys\""),
            format!("JC_VERIFY_HISTORY = \"{shared}{ops}verify/history.md\""),
            "root=${JC_CACHE_ROOT:-/mnt/project-files}".to_string(),
        ] {
            assert!(private_doc_in(&line).is_empty(), "{line}");
        }
        let ps = problems(
            "docs/機能/a.md",
            &format!("一\n設計は {design}2026-10-21-a/設計.md\n"),
        );
        assert!(
            ps.len() == 1 && ps[0].contains(":2:") && ps[0].contains("公開しない資料"),
            "{ps:?}"
        );
    }

    #[test]
    fn crlf_reads_the_same() {
        let lf = "一\nSolidCraft の誤り\n";
        let crlf = lf.replace('\n', "\r\n");
        assert_eq!(problems("a.md", lf), problems("a.md", &crlf));
        assert!(problems("a.md", &crlf)[0].starts_with("a.md:2:"));
    }

    #[test]
    fn third_party_licences_and_data_are_skipped() {
        assert!(skipped("third_party/truck-shapeops/src/lib.rs"));
        assert!(skipped("crates/smlib-sys/LICENSE-APACHE"));
        assert!(skipped("crates/kmb-kernel-truck/testdata/reference.step"));
        assert!(!skipped("crates/kmb-ui/src/lib.rs"));
    }
}
