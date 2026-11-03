//! 並列の作業場が共通に使う部分を先に組む(`cargo xtask warm`、docs/verify.md の 1 節)。
//!
//! 作業場ごとに空の target から検査すると、外の crate・SMLib の C++・下の層の自前の crate を毎回
//! 組み直していた(押し出しの `--dependents-lower` で 375 秒のうち組み立て 285 秒。2026-10-29)。
//! 組み立てと入口の層(`UPPER_LAYERS`)より下の全部を、`verify` と同じ単位(テストの組み立てと
//! clippy)で組んでおけば、その後の担当分の検査は作り直す所だけになる。
//!
//! - 共有フォルダの無いクラウドの作業場は、準備の台本でこれを流す(準備の後の作業場を取っておいて
//!   次の作業に使い回す所なら、その間の作業は作り直す所だけになる)。
//! - 共有フォルダのある作業場は、これの後に `scripts/cloud-target-cache.sh save-deps` で依存だけの
//!   写しを置く(次からは `verify` と `warm` が組み立ての前に自分で戻す)。
//!
//! 組んだ後、自前のテストの実行ファイルと incremental を消す(下の層を全部組むと 8.9 GB のうち
//! 6 GB ほどがこの 2 つ。作り直すのは触った crate だけで、残した依存の 2.4 GB より軽い)。

use crate::jobs::{Done, step};
use crate::space::remember_hack;
use crate::{Package, UPPER_LAYERS, Workspace, cargo, package_args};
use serde_json::Value as Json;
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::process::{ExitCode, Stdio};
use std::time::Instant;

/// 先に組む crate: 組み立てと入口の層より下の全部(`--dependents-lower` が選びうる範囲)。
pub(crate) fn lower_packages(packages: &[Package]) -> BTreeSet<String> {
    packages
        .iter()
        .filter(|p| {
            !p.layer
                .as_deref()
                .is_some_and(|l| UPPER_LAYERS.contains(&l))
        })
        .map(|p| p.name.clone())
        .collect()
}

/// `cargo test --no-run --message-format json` の出力から、テストの実行ファイルを拾う。
/// テストとして組むのはワークスペースの crate だけなので、拾うのは自前の物だけ。
pub(crate) fn test_executables(json_lines: &str) -> Vec<PathBuf> {
    json_lines
        .lines()
        .filter_map(|l| serde_json::from_str::<Json>(l).ok())
        .filter(|m| m["reason"] == "compiler-artifact" && m["profile"]["test"] == true)
        .filter_map(|m| m["executable"].as_str().map(PathBuf::from))
        .collect()
}

pub(crate) fn run(ws: &Workspace) -> Result<ExitCode, String> {
    let sel = Some(lower_packages(&ws.packages));
    let pk = package_args(&sel);
    println!(
        "組み立てと入口の層より下の {} crate を組みます(テストは流しません)",
        sel.as_ref().map_or(0, BTreeSet::len)
    );
    let dir = ws.target_dir.join("verify");
    fs::create_dir_all(&dir).map_err(|e| format!("{} を作れません: {e}", dir.display()))?;
    let json = dir.join("warm.json");

    // 1. テストの組み立て(verify の `nextest list` が組む物と同じ)
    let mut test = cargo();
    test.args([
        "test",
        "--no-run",
        "--message-format",
        "json-render-diagnostics",
    ])
    .args(&pk)
    .stdout(Stdio::from(
        fs::File::create(&json).map_err(|e| e.to_string())?,
    ));
    let built = step("組み立て", test);
    // 2. clippy(verify と同じ引数。自前の crate の結果も、ソースが変わらなければ使い回せる)
    let mut clippy = cargo();
    clippy
        .arg("clippy")
        .args(&pk)
        .args(["--all-targets", "--", "-D", "warnings"]);
    let checked = step("clippy", clippy);
    remember_hack(ws, std::slice::from_ref(&built));

    // 3. 自前のテストの実行ファイルと incremental を消す
    let t = Instant::now();
    let text = fs::read_to_string(&json).unwrap_or_default();
    let exes = test_executables(&text);
    let removed = exes.iter().filter(|p| fs::remove_file(p).is_ok()).count();
    let inc = ws.target_dir.join("debug/incremental");
    let inc_removed = inc.is_dir() && fs::remove_dir_all(&inc).is_ok();
    let mut cleaned = Done::new("片付け", true, t.elapsed().as_secs_f64());
    cleaned.detail = Some(format!(
        "テストの実行ファイル {removed} 個、incremental {}",
        if inc_removed { "消した" } else { "無し" }
    ));

    println!();
    for d in [&built, &checked, &cleaned] {
        println!(
            "{} {} {:.1} 秒{}",
            if d.ok { "通過" } else { "失敗" },
            d.name,
            d.secs,
            d.detail
                .as_deref()
                .map(|s| format!("({s})"))
                .unwrap_or_default()
        );
    }
    // clippy の警告は組んだ物を壊さないので、失敗にするのは組み立てが通らなかった時だけ
    if !checked.ok {
        println!("clippy は警告で落ちましたが、組んだ物は使えます(検査の時に直す)");
    }
    Ok(if built.ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warms_everything_below_the_assembly_and_entry_layers() {
        let p = |name: &str, layer: Option<&str>| Package {
            name: name.into(),
            layer: layer.map(String::from),
            ..Default::default()
        };
        let got = lower_packages(&[
            p("kmb-core", Some("core")),
            p("kmb-cad-extrude", Some("cad")),
            p("xtask", Some("support")),
            p("kmb-bundle", Some("assembly")),
            p("kmb-session", Some("session")),
            p("kmb-ui", Some("ui")),
            p("kmb-app", Some("app")),
            p("no-layer", None),
        ]);
        assert_eq!(
            got.into_iter().collect::<Vec<_>>(),
            ["kmb-cad-extrude", "kmb-core", "no-layer", "xtask"]
        );
    }

    #[test]
    fn picks_only_test_executables_from_the_build_messages() {
        let lines = [
            r#"{"reason":"compiler-artifact","profile":{"test":true},"executable":"/t/debug/deps/kmb_core-0123456789abcdef"}"#,
            r#"{"reason":"compiler-artifact","profile":{"test":false},"executable":null}"#,
            r#"{"reason":"compiler-artifact","profile":{"test":false},"executable":"/t/debug/xtask"}"#,
            r#"{"reason":"build-script-executed","out_dir":"/t/debug/build/x/out"}"#,
            "not json",
        ]
        .join("\n");
        assert_eq!(
            test_executables(&lines),
            [PathBuf::from("/t/debug/deps/kmb_core-0123456789abcdef")]
        );
    }
}
