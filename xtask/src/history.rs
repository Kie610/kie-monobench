//! 検査(`cargo xtask verify`)にかかった時間の記録。1 回ごとに 1 行を Markdown の表へ足す。
//!
//! - クラウドの作業場では、コンテナをまたいで残る `JC_VERIFY_HISTORY`(`.cargo/config.toml`。
//!   `/mnt/project-files/ops/verify/history.md`)へ書く。その置き場所の最初の 2 段
//!   (`/mnt/project-files`)が無い所(利用者の PC など)では、`target/verify/history.md` へ書く。
//! - 書けなくても検査は止めない(知らせるだけ)。
//! - `--all` は、この記録で truck の回が最後に通ってから 7 日を過ぎていれば truck の回も流す
//!   (`truck_due`。週に 1 回、2026-10-25 Kie)。
//!
//! 手順の説明は docs/verify.md の 1 節。

use crate::Done;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const HEADER: &str = "\
# 検査の時間の記録

`cargo xtask verify` が 1 回ごとに 1 行を足す(xtask/src/history.rs)。秒は段ごとの時間で、テスト・truck・clippy は組み立ての後に同時に流す。組み立ての「コンパイル 0 個」は組み立て済みだったこと。点検は組み立てずに流す層・飛ばし・消えたテストの点検と、実行中の飛ばし・ライセンス(2026-10-21 から)。テストの欄が「-」なら未実行(通過ではない)。

| 日時(JST) | 場所 | コミット | 指定 | 結果 | 合わせて | 組み立て | テスト | テスト(truck) | clippy | 点検 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
";

/// 記録を書く場所と、それがコンテナをまたいで残る置き場所か。
fn path(target: &Path) -> (PathBuf, bool) {
    path_from(std::env::var_os("JC_VERIFY_HISTORY"), target)
}

fn path_from(var: Option<std::ffi::OsString>, target: &Path) -> (PathBuf, bool) {
    if let Some(p) = var.filter(|p| !p.is_empty()).map(PathBuf::from) {
        // 根からの最初の 2 段(`/mnt/project-files`、Windows なら `C:\Users` のようにドライブの後の 1 段)
        let top: PathBuf = p.components().take(3).collect();
        if p.is_absolute() && top.is_dir() {
            return (p, true);
        }
    }
    (target.join("verify").join("history.md"), false)
}

/// 1 段の表の欄。`86.0 秒(通過 968・失敗 0・飛ばし 6)`。
fn cell(d: &Done) -> String {
    let mut s = format!("{:.1} 秒", d.secs);
    if let Some(x) = &d.detail {
        s += &format!("({x})");
    }
    if !d.ok {
        s += " 失敗";
    }
    s
}

/// 段の名前から表の列(0 組み立て・1 テスト・2 truck・3 clippy・4 点検)。
fn column(name: &str) -> Option<usize> {
    if name == "組み立て" {
        Some(0)
    } else if name.starts_with("テスト(truck)") {
        Some(2)
    } else if name == "テスト" {
        Some(1)
    } else if name.contains("clippy") || name.contains("doctest") {
        Some(3)
    } else if matches!(name, "点検" | "実行中の飛ばし" | "ライセンス") {
        Some(4)
    } else {
        None
    }
}

/// 表の 1 行。
fn row(
    when: &str,
    place: &str,
    commit: &str,
    args: &[String],
    results: &[Done],
    total: f64,
) -> String {
    let mut cols = vec![String::new(); 5];
    for d in results {
        if let Some(i) = column(&d.name) {
            if !cols[i].is_empty() {
                cols[i] += "、";
            }
            cols[i] += &cell(d);
        }
    }
    let args = if args.is_empty() {
        "(変えた crate)".to_string()
    } else {
        format!("`{}`", args.join(" "))
    };
    let ok = if results.iter().all(|d| d.ok) {
        "通過"
    } else {
        "失敗"
    };
    format!(
        "| {when} | {place} | {commit} | {args} | {ok} | {total:.1} 秒 | {} |\n",
        cols.iter()
            .map(|c| if c.is_empty() { "-" } else { c.as_str() })
            .collect::<Vec<_>>()
            .join(" | ")
    )
}

/// truck の回を流す間隔(日)。
const TRUCK_EVERY_DAYS: i64 = 7;

/// 記録で truck の回が最後に通ってから `TRUCK_EVERY_DAYS` 日を過ぎたか(記録が無い・通った回が
/// 無い時も過ぎたと見る)。SMLib だけの操作の試験が truck の飛ばしに入らないまま溜まらないように、
/// `--all` がこれを見て truck の回を足す。
pub fn truck_due(target: &Path) -> bool {
    let text = fs::read_to_string(path(target).0).unwrap_or_default();
    let today = jst_days(SystemTime::now());
    last_truck_pass(&text).is_none_or(|d| today - d >= TRUCK_EVERY_DAYS)
}

/// 記録の中で、truck の欄が通過だった最後の行の日(1970-01-01 からの日数、日本時間)。
fn last_truck_pass(text: &str) -> Option<i64> {
    text.lines()
        .filter_map(|l| {
            let c: Vec<&str> = l.split('|').map(str::trim).collect();
            // | 日時 | 場所 | コミット | 指定 | 結果 | 合わせて | 組み立て | テスト | テスト(truck) | …
            let truck = c.get(9)?;
            // 落ちた段は欄の終わりに「 失敗」が付く(`cell`)。中の「失敗 0」は件数
            if *truck == "-" || truck.ends_with("失敗") {
                return None;
            }
            days_of(c.get(1)?.get(..10)?)
        })
        .max()
}

/// `2026-10-18` を 1970-01-01 からの日数へ(Howard Hinnant の days_from_civil)。
fn days_of(date: &str) -> Option<i64> {
    let mut it = date.split('-').map(|x| x.parse::<i64>().ok());
    let (y, m, d) = (it.next()??, it.next()??, it.next()??);
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// 日本時間の 1970-01-01 からの日数。
fn jst_days(t: SystemTime) -> i64 {
    let secs = t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64 + 9 * 3600;
    secs.div_euclid(86_400)
}

/// 日本時間の `2026-10-18 16:05`。
fn jst(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64 + 9 * 3600;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // 1970-01-01 からの日数を年月日へ(Howard Hinnant の civil_from_days)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}",
        rem / 3600,
        rem % 3600 / 60
    )
}

fn git(args: &[&str]) -> Option<String> {
    let o = Command::new("git").args(args).output().ok()?;
    o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// 今のコミット。コミットしていない変更があれば `+変更` を付ける。
fn commit() -> String {
    let head = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "不明".into());
    match git(&["status", "--porcelain", "--untracked-files=no"]) {
        Some(s) if !s.is_empty() => format!("{head}+変更"),
        _ => head,
    }
}

fn place(shared: bool) -> String {
    let cpus = std::thread::available_parallelism().map_or(0, |n| n.get());
    let name = if shared {
        "クラウド".to_string()
    } else {
        std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .unwrap_or_else(|_| "PC".into())
    };
    format!("{name}({cpus} CPU)")
}

/// 1 回分を記録へ足す。
pub fn record(target: &Path, args: &[String], results: &[Done], total: f64) {
    let (p, shared) = path(target);
    let line = row(
        &jst(SystemTime::now()),
        &place(shared),
        &commit(),
        args,
        results,
        total,
    );
    let res = (|| -> std::io::Result<()> {
        if let Some(dir) = p.parent() {
            fs::create_dir_all(dir)?;
        }
        let new = !p.exists();
        let mut f = OpenOptions::new().create(true).append(true).open(&p)?;
        if new {
            f.write_all(HEADER.as_bytes())?;
        }
        f.write_all(line.as_bytes())
    })();
    match res {
        Ok(()) => println!("時間の記録: {}", p.display()),
        Err(e) => eprintln!("時間の記録を {} に書けません: {e}", p.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_becomes_one_row_with_a_column_per_step() {
        let mut built = Done::new("組み立て", true, 24.04);
        built.detail = Some("コンパイル 0 個".into());
        let mut test = Done::new("テスト", true, 86.0);
        test.detail = Some("通過 968・失敗 0・飛ばし 6".into());
        let clippy = Done::new("clippy", false, 7.0);
        let mut checks = Done::new("点検", true, 0.4);
        checks.detail = Some("層 0".into());
        let r = row(
            "2026-10-18 16:05",
            "クラウド(4 CPU)",
            "5f01732",
            &["--all".into()],
            &[checks, built, test, clippy],
            110.0,
        );
        assert_eq!(
            r,
            "| 2026-10-18 16:05 | クラウド(4 CPU) | 5f01732 | `--all` | 失敗 | 110.0 秒 | \
             24.0 秒(コンパイル 0 個) | 86.0 秒(通過 968・失敗 0・飛ばし 6) | - | 7.0 秒 失敗 | \
             0.4 秒(層 0) |\n"
        );
        // nextest の無い所では truck の回が crate ごとに分かれる
        let r = row(
            "t",
            "p",
            "c",
            &[],
            &[
                Done::new("テスト(truck) kmb-solid", true, 1.0),
                Done::new("テスト(truck) kmb-samples", true, 2.0),
            ],
            3.0,
        );
        assert!(r.contains("| (変えた crate) | 通過 |"));
        assert!(r.contains("| 1.0 秒、2.0 秒 |"));
    }

    #[test]
    fn the_truck_run_is_due_a_week_after_it_last_passed() {
        let text = format!(
            "{HEADER}\
| 2026-10-18 15:53 | クラウド | a | `--all --truck` | 通過 | 1 秒 | - | 1 秒 | 2 秒(通過 50・失敗 0・飛ばし 54) | - |
| 2026-10-24 10:00 | クラウド | b | `--all --truck` | 失敗 | 1 秒 | - | 1 秒 | 2 秒(通過 9・失敗 1・飛ばし 5) 失敗 | - | - |
| 2026-10-25 10:00 | クラウド | c | `--all` | 通過 | 1 秒 | - | 1 秒 | - | - | - |
"
        );
        // 最後に通ったのは 10-18(10-24 は失敗、10-25 は truck の回なし)
        assert_eq!(last_truck_pass(&text), days_of("2026-10-18"));
        assert_eq!(days_of("1970-01-01"), Some(0));
        assert_eq!(
            days_of("2026-10-25")
                .zip(days_of("2026-10-18"))
                .map(|(a, b)| a - b),
            Some(7)
        );
        assert_eq!(
            days_of("2024-03-01")
                .zip(days_of("2024-02-28"))
                .map(|(a, b)| a - b),
            Some(2)
        );
        assert_eq!(
            jst_days(UNIX_EPOCH + std::time::Duration::from_secs(15 * 3600)),
            1
        );
        assert_eq!(last_truck_pass(HEADER), None);
    }

    #[test]
    fn japan_time_is_nine_hours_ahead_of_utc() {
        let t = UNIX_EPOCH + std::time::Duration::from_secs(1_792_306_800); // 2026-10-18 07:00 UTC
        assert_eq!(jst(t), "2026-10-18 16:00");
        assert_eq!(jst(UNIX_EPOCH), "1970-01-01 09:00");
        // うるう年の 2 月 29 日
        let t = UNIX_EPOCH + std::time::Duration::from_secs(1_709_164_800); // 2024-02-29 00:00 UTC
        assert_eq!(jst(t), "2024-02-29 09:00");
    }

    #[test]
    fn a_shared_place_that_does_not_exist_falls_back_to_target() {
        let local = (Path::new("t").join("verify").join("history.md"), false);
        let missing = Some("/kmb-no-such-place/ops/verify/history.md".into());
        assert_eq!(path_from(missing, Path::new("t")), local);
        assert_eq!(path_from(None, Path::new("t")), local);
        assert_eq!(path_from(Some("".into()), Path::new("t")), local);
        // 最初の 2 段があれば、その下のフォルダはまだ無くてよい(書く時に作る)
        let tmp = std::env::temp_dir();
        if tmp.is_absolute() && tmp.components().count() >= 2 {
            let p = tmp.join("kmb-history-test").join("a").join("history.md");
            let top: PathBuf = p.components().take(3).collect();
            assert_eq!(
                path_from(Some(p.clone().into()), Path::new("t")).1,
                top.is_dir()
            );
        }
    }
}
