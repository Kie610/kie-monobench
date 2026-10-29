//! 時間のかかる操作の試験。通常の選択と記録を保ち、組み立てが済んでから独立した段で流す。

use super::*;

const PACKAGES: [&str; 2] = ["kmb-samples", "kmb-ui"];
const FILTER: &str = "(package(kmb-samples) | package(kmb-ui)) & test(/(^|::)wide_/)";
const STAGE: &str = "広い操作の試験";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Plan {
    Run,
    NotRun(&'static str),
}

pub(super) fn plan(o: &Options) -> Plan {
    if o.lower {
        Plan::NotRun("下位層の検査なので、広い操作の試験は CI で行う")
    } else if o.truck {
        Plan::Run
    } else {
        Plan::NotRun("truck の回ではない")
    }
}

/// 各 crate に ignored な wide_ が実在し、filter に合うことを確かめる。
fn listed_counts(list: &Json) -> Result<[usize; 2], String> {
    let suites = list["rust-suites"]
        .as_object()
        .ok_or("試験の一覧に rust-suites が無い")?;
    let mut counts = [0; 2];
    for (id, suite) in suites {
        let package = suite["package-name"]
            .as_str()
            .unwrap_or_else(|| id.split("::").next().unwrap_or(id));
        let Some(index) = PACKAGES.iter().position(|p| *p == package) else {
            continue;
        };
        let tests = suite["testcases"]
            .as_object()
            .ok_or("試験の一覧に testcases が無い")?;
        counts[index] += tests
            .iter()
            .filter(|(name, t)| {
                name.rsplit("::")
                    .next()
                    .is_some_and(|n| n.starts_with("wide_"))
                    && t["ignored"].as_bool() == Some(true)
                    && t["filter-match"]["status"].as_str() == Some("matches")
            })
            .count();
    }
    if counts.contains(&0) {
        Err(format!(
            "広い試験が両 crate に揃っていません: {PACKAGES:?} {counts:?}"
        ))
    } else {
        Ok(counts)
    }
}

/// 同じ profile の出力を区別し、通常の実 XML を byte ごと戻す。
struct JunitGuard {
    originals: Vec<(PathBuf, Option<Vec<u8>>)>,
}

impl JunitGuard {
    fn new(paths: Vec<PathBuf>, dir: &Path) -> Result<Self, String> {
        let mut guard = Self {
            originals: Vec::new(),
        };
        for (i, path) in paths.into_iter().enumerate() {
            if guard.originals.iter().any(|(p, _)| p == &path) {
                continue;
            }
            let bytes = match fs::read(&path) {
                Ok(bytes) => {
                    fs::write(dir.join(format!("normal-junit-{i}.xml")), &bytes)
                        .map_err(|e| e.to_string())?;
                    fs::remove_file(&path).map_err(|e| e.to_string())?;
                    Some(bytes)
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => return Err(e.to_string()),
            };
            guard.originals.push((path, bytes));
        }
        Ok(guard)
    }

    fn save_and_restore(&mut self, dir: &Path) -> Result<(), String> {
        let mut saved = false;
        let mut problems = Vec::new();
        for (i, (path, _)) in self.originals.iter().enumerate() {
            if path.exists() {
                match fs::read(path)
                    .and_then(|bytes| fs::write(dir.join(format!("junit-{i}.xml")), bytes))
                {
                    Ok(()) => saved = true,
                    Err(e) => problems.push(e.to_string()),
                }
            }
        }
        if let Err(e) = self.restore() {
            problems.push(e);
        }
        if !saved {
            problems.push("広い試験の実 JUnit が作られませんでした".into());
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems.join("、"))
        }
    }

    fn restore(&mut self) -> Result<(), String> {
        let mut problems = Vec::new();
        for (path, bytes) in &self.originals {
            let result = match bytes {
                Some(bytes) => fs::write(path, bytes),
                None => match fs::remove_file(path) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    result => result,
                },
            };
            if let Err(e) = result {
                problems.push(e.to_string());
            }
        }
        if problems.is_empty() {
            self.originals.clear();
            Ok(())
        } else {
            Err(problems.join("、"))
        }
    }
}

impl Drop for JunitGuard {
    fn drop(&mut self) {
        if let Err(e) = self.restore() {
            eprintln!("通常の JUnit を戻せません: {e}");
        }
    }
}

/// nextest の対象外と、実行中に飛ばした関数を分ける。
fn adjusted_counts(raw: &str, runtime: usize) -> Result<String, String> {
    let numbers: Vec<usize> = raw
        .split('・')
        .filter_map(|part| part.split_whitespace().nth(1)?.parse().ok())
        .collect();
    if numbers.len() != 3 || numbers[0] < runtime {
        return Err("広い試験の集計と実行中の飛ばしが食い違います".into());
    }
    Ok(format!(
        "関数: 通過 {}・失敗 {}・実行中飛ばし {runtime}・対象外 {}（raw 通過 {}）",
        numbers[0] - runtime,
        numbers[1],
        numbers[2],
        numbers[0]
    ))
}

fn failed(reason: impl Into<String>) -> Done {
    let mut d = Done::new(STAGE, false, 0.0);
    d.detail = Some(format!("未実行/使えない: {}", reason.into()));
    d
}

pub(super) fn verify(ws: &Workspace) -> Result<Vec<Done>, String> {
    if !nextest_available() {
        return Ok(vec![failed("cargo-nextest が無い")]);
    }
    let selected = Some(PACKAGES.iter().map(|s| (*s).to_string()).collect());
    let dir = ws.target_dir.join("verify/wide");
    let (built, meta, bins) = build_nextest(&selected, ws, &dir, "広い操作の組み立て")?;
    let ok = built.ok;
    let mut results = vec![built];
    if !ok {
        return Ok(results);
    }
    let command = |verb: &str| {
        let mut c = cargo();
        c.env("JC_KERNEL", "smlib")
            .args(["nextest", verb, "--run-ignored", "only", "-E", FILTER])
            .arg("--binaries-metadata")
            .arg(&bins)
            .arg("--cargo-metadata")
            .arg(&meta);
        c
    };
    let listed = output(command("list").args(["--message-format", "json"]));
    let listed = match listed.and_then(|text| {
        fs::write(dir.join("list.json"), &text).map_err(|e| e.to_string())?;
        let list: Json = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        listed_counts(&list)
    }) {
        Ok(counts) => counts,
        Err(e) => {
            results.push(failed(e));
            return Ok(results);
        }
    };
    println!(
        "\n== {STAGE}: {} 本と {} 本、SMLib で 1 回",
        listed[0], listed[1]
    );
    let mut xml = JunitGuard::new(
        vec![
            ws.root.join("target/nextest/verify/junit.xml"),
            ws.target_dir.join("nextest/verify/junit.xml"),
        ],
        &dir,
    )?;
    let skip = dir.join("skips.log");
    match fs::remove_file(&skip) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.to_string()),
        _ => {}
    }
    let mut run = command("run");
    run.env("JC_SKIP_LOG", &skip).args([
        "--no-fail-fast",
        "--profile",
        "verify",
        "--test-threads",
        "1",
        "--success-output",
        "final",
        "--no-tests",
        "fail",
    ]);
    let job = spawn(STAGE, run, dir.join("test.log"), true)?;
    let mut done = wait_all(vec![job]);
    if let Err(e) = xml.save_and_restore(&dir) {
        done[0].ok = false;
        done[0].detail = Some(format!(
            "{}。{e}",
            done[0].detail.as_deref().unwrap_or("件数不明")
        ));
    }
    let log = fs::read_to_string(&skip).unwrap_or_default();
    let (runtime, problems) = match skips::allowed(&ws.root) {
        Ok(allowed) => skips::runtime(&log, &allowed),
        Err(e) => (vec![], vec![e]),
    };
    let raw = fs::read_to_string(dir.join("test.log")).unwrap_or_default();
    let counts = jobs::nextest_counts(&raw)
        .ok_or("広い試験の集計が無い")
        .and_then(|raw| {
            adjusted_counts(&raw, runtime.len()).map_err(|_| "広い試験の集計が合わない")
        });
    match counts {
        Ok(counts) => {
            let detail = done[0].detail.as_deref().unwrap_or_default();
            done[0].detail = Some(format!("{counts}。操作/ボタンはログの別表。{detail}"));
        }
        Err(e) => {
            done[0].ok = false;
            done[0].detail = Some(e.into());
        }
    }
    if !problems.is_empty() {
        done[0].ok = false;
        done[0].detail = Some(format!(
            "{}。{}",
            done[0].detail.as_deref().unwrap_or_default(),
            problems.join("、")
        ));
    }
    for skipped in runtime {
        println!("  広い試験の実行中の飛ばし: {skipped}");
    }
    results.extend(done);
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 確かめること: 下位検査を優先し、狭い明示 truck でも別段を要求する。
    #[test]
    fn lower_never_adds_wide_and_effective_truck_runs_for_named_scopes() {
        let mut o = Options {
            truck: true,
            crates: vec!["kmb-cad-hole".into()],
            ..Default::default()
        };
        assert_eq!(plan(&o), Plan::Run);
        o.lower = true;
        assert!(matches!(plan(&o), Plan::NotRun(_)));
        o.all = true;
        assert!(matches!(plan(&o), Plan::NotRun(_)));
        o.lower = false;
        o.truck = false;
        assert!(matches!(plan(&o), Plan::NotRun(_)));
    }

    /// 確かめること: 片方の広い関数が無い一覧を成功としない。
    #[test]
    fn both_packages_need_matching_ignored_wide_tests() {
        let mut list = serde_json::json!({"rust-suites": {
            "kmb-samples": {"testcases": {"x::wide_actions": {"ignored": true, "filter-match": {"status": "matches"}}}},
            "kmb-ui": {"testcases": {"wide_toolbar": {"ignored": true, "filter-match": {"status": "matches"}}, "measure_speed": {"ignored": true, "filter-match": {"status": "mismatch"}}}}
        }});
        assert_eq!(listed_counts(&list).unwrap(), [1, 1]);
        list["rust-suites"]["kmb-ui"]["testcases"]["wide_toolbar"]["ignored"] = false.into();
        assert!(listed_counts(&list).is_err());
        assert!(listed_counts(&serde_json::json!({"rust-suites": {}})).is_err());
    }

    /// 確かめること: 実行中の飛ばしは通過から除き、filter の対象外と混ぜない。
    #[test]
    fn runtime_skips_are_subtracted_from_passed_functions() {
        let s = adjusted_counts("通過 2・失敗 0・飛ばし 640", 1).unwrap();
        assert!(s.contains("通過 1・失敗 0・実行中飛ばし 1・対象外 640"));
        assert!(adjusted_counts("通過 0・失敗 0・飛ばし 2", 1).is_err());
    }

    /// 確かめること: 広い段が失敗しても通常の XML を同じ byte へ戻す。
    #[test]
    fn junit_is_preserved_for_success_and_missing_wide_report() {
        let dir = std::env::temp_dir().join(format!(
            "kmb-wide-junit-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let xml = dir.join("normal.xml");
        fs::write(&xml, b"normal bytes\n").unwrap();
        {
            let mut guard = JunitGuard::new(vec![xml.clone()], &dir).unwrap();
            fs::write(&xml, b"wide bytes\n").unwrap();
            guard.save_and_restore(&dir).unwrap();
        }
        assert_eq!(fs::read(&xml).unwrap(), b"normal bytes\n");
        assert_eq!(fs::read(dir.join("junit-0.xml")).unwrap(), b"wide bytes\n");
        {
            let mut guard = JunitGuard::new(vec![xml.clone()], &dir).unwrap();
            assert!(guard.save_and_restore(&dir).is_err());
        }
        assert_eq!(fs::read(&xml).unwrap(), b"normal bytes\n");
        fs::remove_dir_all(dir).unwrap();
    }
}
