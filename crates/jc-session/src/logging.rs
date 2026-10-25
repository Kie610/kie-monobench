//! ログ。各モジュールは `log` の `error!`・`warn!`・`info!`・`debug!`・`trace!` を使い、
//! 画面・CLI・MCP は起動時に [`init`] を呼んで、ユーザーのデータフォルダの `logs` へ書く。
//! どこへも送らない。
//!
//! - 1 回の起動で 1 ファイル(`<アプリ>-<UTC の日時>-<pid>.log`)。アプリごとに新しい [`MAX_FILES`] 個を残し
//!   (CLI を何度も呼んでも画面のログを押し出さない)、
//!   1 ファイルが [`MAX_FILE_BYTES`] を超えたら、それ以降は書かない(ディスクを埋めない)。
//! - 1 行ずつ書いて閉じない(バッファに溜めない)ので、C++ の中で落ちても直前の行まで残る。
//! - 最近の [`RECENT_LINES`] 行を覚えておき、クラッシュ報告に添える([`recent`])。
//! - どこまで書くかは環境変数 `JC_LOG` で決める([`Filter::parse`])。既定は自前のモジュール
//!   (`jc_`・`kie_monobench` で始まる)が `info`、ほかのライブラリ(wgpu など)が `warn`。
//!   `JC_LOG` を設定した時は、同じ内容を標準エラーにも出す。
//!
//! # レベルの決まり(各モジュールで同じにする)
//! - `error`: 結果を出せなかった(保存できない、操作が失敗した、機能の評価中にパニック)
//! - `warn`: 結果は出たが、ユーザーが知るべき(機能の評価の誤り、読めない設定、旧い形式の変換)
//! - `info`: 1 回の操作に 1〜数行(開いた・保存した・操作を実行した・再計算の要約と時間)
//! - `debug`: 調べるときだけ見る中身(機能ごとの評価時間、経路の点の数、引数)
//! - `trace`: ループの中など大量に出るもの
//!
//! 毎フレーム呼ばれる所(画面の描画)では `info` 以上を出さない。

use log::{LevelFilter, Log, Metadata, Record};
use std::collections::VecDeque;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

/// 残すログのファイルの数。
pub const MAX_FILES: usize = 20;
/// 1 ファイルの大きさの上限(バイト)。
pub const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;
/// クラッシュ報告に添える最近の行の数。
pub const RECENT_LINES: usize = 200;
/// どこまで書くかを決める環境変数。
pub const ENV: &str = "JC_LOG";

/// データフォルダの中の、ログの置き場所の名前。
pub const LOG_DIR_NAME: &str = "logs";

/// データフォルダの中の、ログの置き場所。
pub fn log_dir(data_dir: &Path) -> PathBuf {
    data_dir.join(LOG_DIR_NAME)
}

/// 自前のモジュールとみなす target の始まり。
fn is_own(target: &str) -> bool {
    target.starts_with("jc_") || target.starts_with("kie_monobench")
}

/// どの target をどのレベルまで書くか。
#[derive(Debug, Clone, PartialEq)]
pub struct Filter {
    /// 自前のモジュールの既定
    own: LevelFilter,
    /// ほかのライブラリの既定
    other: LevelFilter,
    /// `target の始まり=レベル`(長く一致したものを使う)
    rules: Vec<(String, LevelFilter)>,
}

impl Default for Filter {
    fn default() -> Self {
        Filter {
            own: LevelFilter::Info,
            other: LevelFilter::Warn,
            rules: Vec::new(),
        }
    }
}

impl Filter {
    /// `JC_LOG` の書き方を読む。コンマで区切り、各項目は次のどれか。
    /// - `debug`(レベルだけ): 自前のモジュールのレベル
    /// - `jc_cam_commands=trace`: その target(の始まり)のレベル
    /// - `*=info`: ほかのライブラリのレベル
    ///
    /// 読めない項目は飛ばし、その旨を返す(ログのせいで起動できなくしない)。
    pub fn parse(spec: &str) -> (Filter, Vec<String>) {
        let mut f = Filter::default();
        let mut bad = Vec::new();
        for item in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            match item.split_once('=') {
                None => match item.parse::<LevelFilter>() {
                    Ok(l) => f.own = l,
                    Err(_) => bad.push(item.to_string()),
                },
                Some((t, l)) => match l.trim().parse::<LevelFilter>() {
                    Ok(l) if t.trim() == "*" => f.other = l,
                    Ok(l) => f.rules.push((t.trim().to_string(), l)),
                    Err(_) => bad.push(item.to_string()),
                },
            }
        }
        (f, bad)
    }

    /// `target` で書くレベルの上限。
    pub fn level(&self, target: &str) -> LevelFilter {
        self.rules
            .iter()
            .filter(|(t, _)| target.starts_with(t.as_str()))
            .max_by_key(|(t, _)| t.len())
            .map(|(_, l)| *l)
            .unwrap_or(if is_own(target) { self.own } else { self.other })
    }

    /// どれかの target で書く、最も細かいレベル(`log` の全体の上限にする)。
    pub fn max(&self) -> LevelFilter {
        self.rules
            .iter()
            .map(|(_, l)| *l)
            .chain([self.own, self.other])
            .max()
            .unwrap_or(LevelFilter::Off)
    }
}

struct Sink {
    file: Option<File>,
    written: u64,
    recent: VecDeque<String>,
}

/// ファイルと最近の行へ書く記録係。[`init`] で全体に 1 つ入れる。テストでは直接作って使える。
pub struct Logger {
    filter: Filter,
    stderr: bool,
    path: Option<PathBuf>,
    sink: Mutex<Sink>,
}

impl Logger {
    /// `dir` に今回のファイルを作る(`None` ならファイルに書かず、最近の行だけ覚える)。
    pub fn new(dir: Option<&Path>, app: &str, filter: Filter, stderr: bool) -> Logger {
        let (file, path) = match dir.and_then(|d| open_file(d, app).ok()) {
            Some((f, p)) => (Some(f), Some(p)),
            None => (None, None),
        };
        Logger {
            filter,
            stderr,
            path,
            sink: Mutex::new(Sink {
                file,
                written: 0,
                recent: VecDeque::with_capacity(RECENT_LINES),
            }),
        }
    }

    /// 今回のログのファイル。
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// 最近の行(古い順)。
    pub fn recent(&self) -> Vec<String> {
        let s = self.sink.lock().unwrap_or_else(|e| e.into_inner());
        s.recent.iter().cloned().collect()
    }
}

/// 1 行の形: `2026-10-14T12:34:56.789Z INFO  [main] jc_core::document: 内容`
pub fn format_line(time: SystemTime, record: &Record<'_>) -> String {
    let thread = std::thread::current();
    format!(
        "{} {:<5} [{}] {}: {}",
        utc(time),
        record.level(),
        thread.name().unwrap_or("-"),
        record.target(),
        record.args()
    )
}

impl Log for Logger {
    fn enabled(&self, m: &Metadata<'_>) -> bool {
        m.level() <= self.filter.level(m.target())
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format_line(SystemTime::now(), record);
        if self.stderr {
            eprintln!("{line}");
        }
        let mut s = self.sink.lock().unwrap_or_else(|e| e.into_inner());
        if s.recent.len() == RECENT_LINES {
            s.recent.pop_front();
        }
        s.recent.push_back(line.clone());
        let written = s.written;
        if written >= MAX_FILE_BYTES {
            return;
        }
        let text = if written + line.len() as u64 + 1 >= MAX_FILE_BYTES {
            format!("{line}\n(ファイルが上限の大きさに達したので、以降は書きません)\n")
        } else {
            format!("{line}\n")
        };
        // 書けなくても止めない(ログのせいで別の誤りを起こさない)
        if let Some(f) = s.file.as_mut()
            && f.write_all(text.as_bytes()).is_ok()
        {
            s.written += text.len() as u64;
        }
    }

    fn flush(&self) {
        let mut s = self.sink.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(f) = s.file.as_mut() {
            let _ = f.flush();
        }
    }
}

fn open_file(dir: &Path, app: &str) -> std::io::Result<(File, PathBuf)> {
    std::fs::create_dir_all(dir)?;
    prune(dir, app, MAX_FILES.saturating_sub(1));
    let stamp: String = utc(SystemTime::now())
        .chars()
        .filter(char::is_ascii_digit)
        .take(14)
        .collect();
    let path = dir.join(format!("{app}-{stamp}-{}.log", std::process::id()));
    let file = File::options().create(true).append(true).open(&path)?;
    Ok((file, path))
}

/// アプリ `app` のログのファイル(新しい順)。
pub fn files(dir: &Path, app: &str) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let prefix = format!("{app}-");
    let mut v: Vec<(SystemTime, PathBuf)> = rd
        .flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.starts_with(&prefix) && n.ends_with(".log")
        })
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    v.sort_by(|a, b| b.cmp(a));
    v.into_iter().map(|(_, p)| p).collect()
}

/// アプリ `app` のログを、新しい `keep` 個を残して消す。消した数を返す。
pub fn prune(dir: &Path, app: &str, keep: usize) -> usize {
    let old = files(dir, app).into_iter().skip(keep);
    old.filter(|p| std::fs::remove_file(p).is_ok()).count()
}

/// ファイルの終わりの `n` 行(異常終了した前回のログを報告に添える)。
pub fn tail(path: &Path, n: usize) -> Vec<String> {
    let Ok(bytes) = std::fs::read(path) else {
        return vec![];
    };
    // 大きいファイルは終わりの方だけ読む
    let start = bytes.len().saturating_sub(256 * 1024);
    let text = String::from_utf8_lossy(&bytes[start..]);
    let lines: Vec<&str> = text.lines().collect();
    let skip = lines.len().saturating_sub(n);
    lines[skip..].iter().map(|s| s.to_string()).collect()
}

/// UNIX 時刻を `YYYY-MM-DDThh:mm:ss.mmmZ` にする(依存を増やさないために自前で計算する)。
pub fn utc(time: SystemTime) -> String {
    let d = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = d.as_secs();
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    // 日数から年月日(H. Hinnant の civil_from_days)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        rem / 60 % 60,
        rem % 60,
        d.subsec_millis()
    )
}

static LOGGER: OnceLock<Logger> = OnceLock::new();

/// 起動時に 1 回呼ぶ。`data_dir` の `logs` へ今回のファイルを作り、`log` の記録係として入れる。
/// 2 回目以降と、ファイルを作れない時も止めない(最近の行だけ覚える)。今回のファイルを返す。
pub fn init(data_dir: Option<&Path>, app: &str) -> Option<PathBuf> {
    let spec = std::env::var(ENV).ok();
    let (filter, bad) = Filter::parse(spec.as_deref().unwrap_or(""));
    let max = filter.max();
    let dir = data_dir.map(log_dir);
    let logger = LOGGER.get_or_init(|| Logger::new(dir.as_deref(), app, filter, spec.is_some()));
    if log::set_logger(logger).is_ok() {
        log::set_max_level(max);
        log::info!(
            target: "jc_session::logging",
            "{app} {} を起動 (os={} arch={} pid={} ログ={})",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            std::process::id(),
            logger.path().map_or("(ファイルなし)".into(), |p| p.display().to_string()),
        );
        for b in bad {
            log::warn!(target: "jc_session::logging", "{ENV} の項目 {b:?} を読めないので飛ばしました");
        }
    }
    logger.path().map(Path::to_path_buf)
}

/// 今回のログのファイル([`init`] の前や、作れなかった時は `None`)。
pub fn current_file() -> Option<PathBuf> {
    LOGGER.get().and_then(|l| l.path().map(Path::to_path_buf))
}

/// 最近の行(古い順)。[`init`] の前は空。
pub fn recent() -> Vec<String> {
    LOGGER.get().map(Logger::recent).unwrap_or_default()
}
