//! 検査の段を動かす: 1 段ずつ流すか、ログのファイルへ書かせて同時に動かし、終わった順に結果を
//! 集める。nextest の段は、ログの終わりの集計から通過・失敗・飛ばしの件数を読む。

use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// 終わった 1 段。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Done {
    pub(crate) name: String,
    pub(crate) ok: bool,
    pub(crate) secs: f64,
    /// 記録に添える中身(テストの件数、作り直した crate の数)
    pub(crate) detail: Option<String>,
}

impl Done {
    pub(crate) fn new(name: &str, ok: bool, secs: f64) -> Self {
        Done {
            name: name.into(),
            ok,
            secs,
            detail: None,
        }
    }
}

/// 1 段を動かし、成否とかかった時間を返す。
pub(crate) fn step(name: &str, mut cmd: Command) -> Done {
    println!("\n== {name}: {cmd:?}");
    let t = Instant::now();
    let ok = cmd.status().is_ok_and(|s| s.success());
    Done::new(name, ok, t.elapsed().as_secs_f64())
}

/// 同時に動かす 1 つの段(出力はログのファイルへ)。
pub(crate) struct Job {
    name: String,
    child: Child,
    started: Instant,
    log: PathBuf,
    /// nextest の段なら終わりの集計だけを、ほかの段は失敗した時にログの終わりを表示する
    nextest: bool,
}

pub(crate) fn spawn(
    name: &str,
    mut cmd: Command,
    log: PathBuf,
    nextest: bool,
) -> Result<Job, String> {
    // 前のログは消してから作る。止めた検査の子(nextest)が生き残っていても、そちらは消えた方の
    // ファイルに書くので、この回のログに混ざらない(Windows では開いたままのファイルは消せず、
    // ここで止まる)
    match fs::remove_file(&log) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(format!(
                "{} を消せません(前の検査がまだ書いている?): {e}",
                log.display()
            ));
        }
    }
    let f = fs::File::create(&log).map_err(|e| format!("{} を作れません: {e}", log.display()))?;
    let f2 = f.try_clone().map_err(|e| e.to_string())?;
    println!("== {name}: {cmd:?}\n   ログ: {}", log.display());
    let child = cmd
        .stdout(Stdio::from(f))
        .stderr(Stdio::from(f2))
        .spawn()
        .map_err(|e| format!("{name} を動かせません: {e}"))?;
    Ok(Job {
        name: name.into(),
        child,
        started: Instant::now(),
        log,
        nextest,
    })
}

/// nextest の出力の終わりの集計(`Summary` の行から後)。
pub(crate) fn nextest_summary(log: &str) -> String {
    let lines: Vec<&str> = log.lines().collect();
    let from = lines
        .iter()
        .rposition(|l| l.trim_start().starts_with("Summary"))
        .unwrap_or(lines.len().saturating_sub(30));
    lines[from..].join("\n")
}

/// 段の成否と nextest の集計が食い違う時の説明。集計が 2 本あるのは、止めた前の検査の nextest が
/// 生き残って同じログに書いた時(2026-10-22、終了は通過なのに「失敗 75」と出た)。
pub(crate) fn summary_problem(log: &str, ok: bool) -> Option<String> {
    let summaries = log
        .lines()
        .filter(|l| l.trim_start().starts_with("Summary"))
        .count();
    if summaries > 1 {
        return Some(format!(
            "ログに集計が {summaries} 本ある(前の検査の nextest が同じログに書いた疑い。\
             `pgrep -fa nextest` で残りを止めてから流し直す)"
        ));
    }
    let failed = nextest_failed(log)?;
    (ok && failed > 0).then(|| format!("終了は通過なのに、集計は失敗 {failed}"))
}

/// nextest の集計の失敗の数。
pub(crate) fn nextest_failed(log: &str) -> Option<u64> {
    let counts = nextest_counts(log)?;
    let rest = counts.split("失敗 ").nth(1)?;
    rest.split('・').next()?.parse().ok()
}

/// nextest の集計の行(`Summary [ 52.1s] 968 tests run: 968 passed (2 slow), 6 skipped`)から、
/// 通過・失敗・飛ばしの件数。
pub(crate) fn nextest_counts(log: &str) -> Option<String> {
    let line = log
        .lines()
        .rfind(|l| l.trim_start().starts_with("Summary"))?;
    let (_, counts) = line.split_once("run:")?;
    // 括弧の中(`(5 slow, 2 leaky)` など)は通過の内訳なので数えない
    let mut plain = String::new();
    let mut depth = 0u32;
    for c in counts.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => plain.push(c),
            _ => {}
        }
    }
    let mut n = [0u64; 3];
    for part in plain.split(',') {
        let mut w = part.split_whitespace();
        let Some(num) = w.next().and_then(|x| x.parse::<u64>().ok()) else {
            continue;
        };
        let words: Vec<&str> = w.collect();
        match words.as_slice() {
            ["passed"] => n[0] += num,
            ["skipped"] => n[2] += num,
            // 失敗のほか、時間切れ(timed out)・途中で止まった(exec failed など)も失敗に数える
            ws if ws.contains(&"failed") || ws.starts_with(&["timed", "out"]) => n[1] += num,
            _ => {}
        }
    }
    Some(format!("通過 {}・失敗 {}・飛ばし {}", n[0], n[1], n[2]))
}

/// cargo を動かし、誤りの出力(進み具合)をそのまま見せながら、コンパイルした crate の数を数える。
/// 0 個なら組み立て済みだった。
pub(crate) fn run_counting_compiles(cmd: &mut Command) -> (bool, usize) {
    let Ok(mut child) = cmd.stderr(Stdio::piped()).spawn() else {
        return (false, 0);
    };
    let mut compiled = 0;
    if let Some(err) = child.stderr.take() {
        use std::io::BufRead;
        for line in std::io::BufReader::new(err).lines().map_while(Result::ok) {
            eprintln!("{line}");
            if line.trim_start().starts_with("Compiling ") {
                compiled += 1;
            }
        }
    }
    (child.wait().is_ok_and(|s| s.success()), compiled)
}

pub(crate) fn tail(log: &str, n: usize) -> String {
    let lines: Vec<&str> = log.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// 段を全部終わるまで待ち、終わった順に結果を表示する。
pub(crate) fn wait_all(mut jobs: Vec<Job>) -> Vec<Done> {
    let mut results = Vec::new();
    while !jobs.is_empty() {
        let mut i = 0;
        while i < jobs.len() {
            match jobs[i].child.try_wait() {
                Ok(None) => i += 1,
                done => {
                    let j = jobs.remove(i);
                    let ok = done.is_ok_and(|s| s.is_some_and(|s| s.success()));
                    let secs = j.started.elapsed().as_secs_f64();
                    let log = fs::read_to_string(&j.log).unwrap_or_default();
                    println!(
                        "\n== {} が終わりました({}、{secs:.1} 秒)",
                        j.name,
                        if ok { "通過" } else { "失敗" }
                    );
                    let mut d = Done::new(&j.name, ok, secs);
                    if j.nextest {
                        println!("{}", nextest_summary(&log));
                        d.detail = nextest_counts(&log);
                        if let Some(p) = summary_problem(&log, ok) {
                            println!("{p}");
                            d.ok = false;
                            d.detail = Some(format!("{}。{p}", d.detail.unwrap_or_default()));
                        }
                    } else if !ok {
                        println!("{}", tail(&log, 80));
                    }
                    results.push(d);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_summary_or_failures_after_a_passing_exit_fail_the_step() {
        // 止めた前の検査の nextest が同じログに書いた形(2026-10-22 の「失敗 75」)
        let mixed = "Summary [ 9.0s] 75 tests run: 0 passed, 75 failed\n\
                     Summary [ 60.0s] 867 tests run: 867 passed, 2 skipped\n";
        assert!(summary_problem(mixed, true).is_some_and(|p| p.contains("2 本")));
        let lying = "Summary [ 9.0s] 10 tests run: 7 passed, 3 failed";
        assert!(summary_problem(lying, true).is_some_and(|p| p.contains("失敗 3")));
        assert_eq!(summary_problem(lying, false), None);
        let fine = "Summary [ 9.0s] 10 tests run: 10 passed, 1 skipped";
        assert_eq!(summary_problem(fine, true), None);
    }

    #[test]
    fn nextest_summaries_give_the_counts() {
        let log = "PASS [ 1.0s] a\n------------\n     Summary [  52.123s] 974 tests run: 968 passed (2 slow), 6 skipped\n";
        assert_eq!(
            nextest_counts(log).as_deref(),
            Some("通過 968・失敗 0・飛ばし 6")
        );
        let log = "Summary [ 9.0s] 10 tests run: 7 passed, 2 failed, 1 timed out, 3 skipped";
        assert_eq!(
            nextest_counts(log).as_deref(),
            Some("通過 7・失敗 3・飛ばし 3")
        );
        // 括弧の中の leaky・slow は通過の内訳で、失敗ではない
        let log = "Summary [ 60.0s] 1384 tests run: 1384 passed (5 slow, 2 leaky), 19 skipped";
        assert_eq!(
            nextest_counts(log).as_deref(),
            Some("通過 1384・失敗 0・飛ばし 19")
        );
        let log = "Summary [ 9.0s] 5 tests run: 3 passed (1 leaky), 1 failed, 1 exec failed";
        assert_eq!(
            nextest_counts(log).as_deref(),
            Some("通過 3・失敗 2・飛ばし 0")
        );
        assert_eq!(nextest_counts("error: no tests"), None);
    }
}
