//! 起動の安全策: 壊れた設定ファイルを脇へ退ける、窓が開く前に止まった起動を数える、
//! 安全モード(既定の設定・ソフトウェア描画)で起動する。
//!
//! - 読めない設定ファイルは `<名前>.bad-<時刻>` へ名前を変えて残し、既定で始める
//!   (消さないので、後で中身を確かめられる)。
//! - 起動のたびに `startup.json` の「窓が開く前に止まった回数」を 1 つ増やし、最初の画面を
//!   描いたら 0 に戻す。[`FAILED_STARTS_FOR_SAFE_MODE`] 回続けて止まったら、次は安全モードで始める。
//! - 安全モードは `--safe-mode` か `JC_SAFE_MODE=1` でも選べる。利用者の設定ファイルは読まず、
//!   書きもしない(次の普通の起動で元の設定に戻る)。

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// 安全モードを選ぶ環境変数。
pub const SAFE_MODE_ENV: &str = "JC_SAFE_MODE";
/// 描画の方法を選ぶ環境変数(`cpu` でソフトウェア描画)。
pub const RENDERER_ENV: &str = "JC_RENDERER";
/// 窓が開く前に続けて止まったら、次を安全モードにする回数。
pub const FAILED_STARTS_FOR_SAFE_MODE: u32 = 2;
/// 起動の記録のファイル(データフォルダの中)。
pub const MARKER_FILE: &str = "startup.json";

/// 起動時に中身を確かめる設定ファイル(データフォルダの中。どれも JSON)。
/// 工具のライブラリ(`tools.json`)は、壊れていれば上書きを断る作りなので、ここでは触らない。
pub const SETTINGS_FILES: [&str; 4] = [
    crate::SETTINGS_FILE,
    "recent.json",
    "projects.json",
    "layout.json",
];

/// 環境変数の値が「入」か(`1`・`true`・`yes`・`on`)。
pub fn env_on(v: Option<&str>) -> bool {
    v.is_some_and(|v| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

/// 描画の環境変数がソフトウェア描画を求めているか。
pub fn cpu_renderer_env(v: Option<&str>) -> bool {
    v.is_some_and(|v| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "cpu" | "software" | "soft"
        )
    })
}

/// 読めない設定ファイルを `<名前>.bad-<時刻>` へ退ける。退けたものごとに、画面に出す文を返す。
pub fn set_aside_broken(dir: &Path) -> Vec<String> {
    let mut notes = Vec::new();
    for name in SETTINGS_FILES {
        let path = dir.join(name);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Err(e) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let aside = dir.join(format!("{name}.bad-{}", stamp()));
        match std::fs::rename(&path, &aside) {
            Ok(()) => {
                log::warn!(
                    "読めない設定ファイルを退けた: {} → {} ({e})",
                    path.display(),
                    aside.display()
                );
                notes.push(format!(
                    "設定ファイル {name} が読めないので、{} へ名前を変えて退け、既定で始めました",
                    aside
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default()
                ));
            }
            Err(re) => log::warn!("{} を退けられません: {re}", path.display()),
        }
    }
    notes
}

/// 起動の記録。
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
struct Marker {
    /// 窓が開く前に止まった(最初の画面を描かなかった)起動の数
    pending: u32,
}

fn marker_path(dir: &Path) -> PathBuf {
    dir.join(MARKER_FILE)
}

/// 起動を記録する。前までに窓が開く前に止まった起動の数を返す。
/// 記録できなくても起動は続ける(数は 0 とみなす)。
pub fn begin(dir: &Path) -> u32 {
    let path = marker_path(dir);
    let before = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<Marker>(&s).ok())
        .unwrap_or_default()
        .pending;
    let next = Marker {
        pending: before.saturating_add(1),
    };
    if std::fs::create_dir_all(dir).is_ok() {
        let s = serde_json::to_string(&next).expect("JSON にできる");
        if let Err(e) = crate::write_atomic(&path, &s, false) {
            log::warn!("起動の記録を書けません: {e}");
        }
    }
    if before > 0 {
        log::warn!("前の起動が {before} 回続けて、窓が開く前に止まっていた");
    }
    before
}

/// 最初の画面を描いた(起動できた)。記録を 0 に戻す。
pub fn started(dir: &Path) {
    let s = serde_json::to_string(&Marker::default()).expect("JSON にできる");
    if let Err(e) = crate::write_atomic(&marker_path(dir), &s, false) {
        log::warn!("起動の記録を書けません: {e}");
    }
}

/// 今日の日時を `YYYYMMDD-HHMMSS`(UTC)で返す。
fn stamp() -> String {
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let (y, m, d) = civil(days as i64);
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

/// 1970-01-01 からの日数を年・月・日にする(グレゴリオ暦)。
fn civil(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kmb-startup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn broken_settings_are_renamed_and_good_ones_stay() {
        let d = temp("aside");
        std::fs::write(d.join("settings.json"), "{壊れた").unwrap();
        std::fs::write(d.join("recent.json"), r#"{"files": []}"#).unwrap();
        let notes = set_aside_broken(&d);
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].contains("settings.json"));
        assert!(!d.join("settings.json").exists());
        assert!(d.join("recent.json").exists());
        let bad: Vec<_> = std::fs::read_dir(&d)
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("settings.json.bad-")
            })
            .collect();
        assert_eq!(bad.len(), 1);
        // 退けた中身はそのまま残る
        assert_eq!(std::fs::read_to_string(bad[0].path()).unwrap(), "{壊れた");
        // もう一度確かめても、何も起きない
        assert!(set_aside_broken(&d).is_empty());
    }

    #[test]
    fn starts_that_never_reach_a_window_are_counted_until_one_does() {
        let d = temp("marker");
        assert_eq!(begin(&d), 0);
        assert_eq!(begin(&d), 1);
        assert_eq!(begin(&d), 2);
        started(&d);
        assert_eq!(begin(&d), 0);
        // 記録が壊れていても 0 とみなして進む
        std::fs::write(d.join(MARKER_FILE), "x").unwrap();
        assert_eq!(begin(&d), 0);
    }

    #[test]
    fn switches_read_from_the_environment() {
        assert!(env_on(Some("1")) && env_on(Some(" TRUE ")) && env_on(Some("on")));
        assert!(!env_on(Some("0")) && !env_on(Some("")) && !env_on(None));
        assert!(cpu_renderer_env(Some("cpu")) && cpu_renderer_env(Some("Software")));
        assert!(!cpu_renderer_env(Some("gpu")) && !cpu_renderer_env(None));
    }

    #[test]
    fn the_stamp_is_a_calendar_date() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(20_742), (2026, 10, 16));
        assert_eq!(civil(11_016), (2000, 2, 29));
        let s = stamp();
        assert_eq!(s.len(), 15, "{s}");
    }
}
