//! クラッシュ報告。パニックと異常終了ごとに、場所・逆追跡・版・最近の操作・再現用の文書・作業中の部品を
//! ユーザーのデータフォルダの `crash` に JSON で残す。どこへも送らない。
//!
//! - パニックはフック([`install`])で、その場で報告を書く。
//! - パニックにならない異常終了(形状カーネルの C++ の中で落ちた等)は、動いている間の
//!   印のファイル(`session-*.json`)で見つける。印は起動中ずっと鍵を掛けて持ち、操作のたびに
//!   最近の操作と文書を書き直す。正しく終われば消す。次の起動で、鍵の掛かっていない
//!   (持ち主のいない)印が残っていれば、異常終了の報告に変える。
//! - 報告の数・合計の大きさ・日数に上限があり、超えたら古いものから消す。
//!
//! - 報告には、その起動のログ([`crate::logging`])のファイル名と終わりの行を添える。
//!
//! 画面・CLI・MCP は起動時に [`install`] を呼び、文書が変わりうる所で [`note`] を呼ぶ。

use kmb_core::Document;
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use std::fs::File;
use std::io::{Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

/// 残す報告の数の上限。
pub const MAX_REPORTS: usize = 20;
/// 報告の合計の大きさの上限(バイト)。
pub const MAX_BYTES: u64 = 20 * 1024 * 1024;
/// 報告を残す日数。
pub const MAX_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// 報告に残す最近の操作の数。
pub const RECENT_COMMANDS: usize = 30;

/// データフォルダの中の、クラッシュ報告の置き場所。
pub fn crash_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("crash")
}

/// 報告 1 つの要約(一覧に出す)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    /// ファイル名(`read` に渡す)
    pub name: String,
    /// `panic`(パニック)か `abnormal_exit`(異常終了)
    pub kind: String,
    /// 起きた時刻(UNIX 時刻の秒)。次の起動で見つけた異常終了では、その起動が最後に印を
    /// 書いた時刻(落ちたのはこの後)
    pub time: u64,
    /// 次の起動で遅れて見つけた(異常終了)。`time` は落ちた時刻そのものではない
    pub detected_later: bool,
    pub app: String,
    pub version: String,
    pub message: String,
}

/// 動いている間の状態(パニックのフックが読む)。
struct State {
    dir: PathBuf,
    app: String,
    marker: File,
    marker_path: PathBuf,
    /// 文書の変化を見分ける印(取り消しの数、やり直しの数、最後の操作の名前、作業中の部品)
    key: Option<(usize, usize, String, Option<u64>)>,
    commands: Vec<String>,
    document: Option<Json>,
    /// 作業中(アクティブ)の部品の番号と名前(文書の直下なら `None`)。開いている間だけの状態で
    /// 文書に入らないので、別に残す(無いと、作ったボディがどの部品に入ったかを再現できない)
    active: Option<Json>,
    /// 起動した時刻(印を書き直しても変えない)
    started: u64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn base_report(kind: &str, app: &str) -> Json {
    json!({
        "kind": kind,
        "app": app,
        "version": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "time": now_secs(),
    })
}

/// 報告を書く(書けなければ何もしない。報告のせいで別の誤りを起こさない)。書いたファイルを返す。
fn write_report(dir: &Path, report: &Json) -> Option<PathBuf> {
    std::fs::create_dir_all(dir).ok()?;
    let stamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let path = dir.join(format!("crash-{stamp}-{}.json", std::process::id()));
    let text = serde_json::to_string_pretty(report).ok()?;
    std::fs::write(&path, text).ok()?;
    Some(path)
}

/// 印のファイルの中身(異常終了の報告の元)。
fn marker_json(s: &State) -> Json {
    json!({
        "app": s.app,
        "version": env!("CARGO_PKG_VERSION"),
        "pid": std::process::id(),
        "started": s.started,
        // 最後に印を書いた時刻。異常終了は、この後から次の起動までの間に起きた
        "noted": now_secs(),
        "commands": s.commands,
        "document": s.document,
        "active_component": s.active,
        "log_file": crate::logging::current_file(),
    })
}

fn rewrite_marker(s: &mut State) {
    let text = serde_json::to_string(&marker_json(s)).unwrap_or_default();
    // 鍵を掛けた同じハンドルで書く(Windows では別のハンドルから書けない)
    let _ = s.marker.set_len(0);
    let _ = s.marker.rewind();
    let _ = s.marker.write_all(text.as_bytes());
    let _ = s.marker.flush();
}

/// 起動時に呼ぶ。前の起動の異常終了を報告に変え、古い報告を片付け、パニックのフックを入れる。
/// 返した [`Guard`] を `main` の終わりまで持つ(落とすと正しい終了として印を消す)。
pub fn install(data_dir: &Path, app: &str) -> Result<Guard, String> {
    let dir = crash_dir(data_dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{} を作れません: {e}", dir.display()))?;
    collect_abnormal_exits(&dir);
    prune(&dir, SystemTime::now());
    let stamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let marker_path = dir.join(format!("session-{stamp}-{}.json", std::process::id()));
    let marker = File::options()
        .create(true)
        .truncate(true)
        .read(true)
        .write(true)
        .open(&marker_path)
        .map_err(|e| format!("{} を作れません: {e}", marker_path.display()))?;
    marker
        .try_lock()
        .map_err(|e| format!("{} に鍵を掛けられません: {e}", marker_path.display()))?;
    let mut state = State {
        dir,
        app: app.into(),
        marker,
        marker_path: marker_path.clone(),
        key: None,
        commands: Vec::new(),
        document: None,
        active: None,
        started: now_secs(),
    };
    rewrite_marker(&mut state);
    *STATE.lock().unwrap_or_else(|e| e.into_inner()) = Some(state);

    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        on_panic(info);
        previous(info);
    }));
    Ok(Guard { marker_path })
}

fn on_panic(info: &std::panic::PanicHookInfo<'_>) {
    let message = info
        .payload()
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(文字でない理由)".into());
    let location = info
        .location()
        .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()));
    let backtrace = std::backtrace::Backtrace::force_capture().to_string();
    log::error!(
        "パニック: {message} ({})",
        location.as_deref().unwrap_or("場所不明")
    );
    // 状態を持ったままパニックした時は待たない(固まらせない)
    let guard = match STATE.try_lock() {
        Ok(g) => Some(g),
        Err(std::sync::TryLockError::Poisoned(e)) => Some(e.into_inner()),
        Err(std::sync::TryLockError::WouldBlock) => None,
    };
    let Some(state) = guard.as_ref().and_then(|g| g.as_ref()) else {
        return;
    };
    let mut r = base_report("panic", &state.app);
    r["message"] = json!(message);
    r["location"] = json!(location);
    r["thread"] = json!(std::thread::current().name().unwrap_or("(名前なし)"));
    r["backtrace"] = json!(backtrace);
    r["commands"] = json!(state.commands);
    r["document"] = state.document.clone().unwrap_or(Json::Null);
    r["active_component"] = state.active.clone().unwrap_or(Json::Null);
    r["log_file"] = json!(crate::logging::current_file());
    r["log"] = json!(crate::logging::recent());
    if let Some(p) = write_report(&state.dir, &r) {
        eprintln!("クラッシュ報告を残しました: {}", p.display());
    }
}

/// 文書が変わったかもしれない所で呼ぶ(画面なら毎フレームでよい)。`active` は作業中(アクティブ)の
/// 部品(文書の直下なら `None`)。変わっていれば、最近の操作と文書の写しと作業中の部品を持ち直し、
/// 印のファイルを書き直す。[`install`] の前は何もしない。
pub fn note(doc: &Document, active: Option<u64>) {
    let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let Some(state) = g.as_mut() else { return };
    let done = doc.undo_labels();
    let key = (
        done.len(),
        doc.redo_labels().len(),
        done.last().map(|s| s.to_string()).unwrap_or_default(),
        active,
    );
    if state.key.as_ref() == Some(&key) {
        return;
    }
    state.key = Some(key);
    let skip = done.len().saturating_sub(RECENT_COMMANDS);
    state.commands = done[skip..].iter().map(|s| s.to_string()).collect();
    state.document = serde_json::from_str(&doc.to_json()).ok();
    state.active =
        active.map(|c| json!({"id": c, "name": doc.data().component(c).map(|k| k.name.clone())}));
    rewrite_marker(state);
}

/// 正しい終了の印。落とすと印のファイルを消す。
pub struct Guard {
    marker_path: PathBuf,
}

impl Drop for Guard {
    fn drop(&mut self) {
        let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
        if g.as_ref()
            .is_some_and(|s| s.marker_path == self.marker_path)
        {
            // ハンドルを閉じてから消す(Windows は開いたファイルを消せない)
            *g = None;
        }
        drop(g);
        let _ = std::fs::remove_file(&self.marker_path);
    }
}

/// 鍵の掛かっていない(持ち主が終わった)印を、異常終了の報告に変える。動いている別の
/// 起動の印は鍵が掛かっているので触らない。変えた報告の数を返す。
pub fn collect_abnormal_exits(dir: &Path) -> usize {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut n = 0;
    for e in rd.flatten() {
        let path = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if !(name.starts_with("session-") && name.ends_with(".json")) {
            continue;
        }
        let Ok(f) = File::options().read(true).write(true).open(&path) else {
            continue;
        };
        if f.try_lock().is_err() {
            continue;
        }
        let text = std::io::read_to_string(&f).unwrap_or_default();
        // 印を最後に書いた時刻(古い印に "noted" が無ければファイルの更新時刻)
        let written = f
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_secs());
        drop(f);
        let m: Json = serde_json::from_str(&text).unwrap_or(Json::Null);
        let app = m["app"].as_str().unwrap_or("(不明)");
        let mut r = base_report("abnormal_exit", app);
        // 異常終了はその場で報告を書けず、次の起動で見つける。報告の
        // 時刻は見つけた時刻でなく、落ちた起動が最後に印を書いた時刻にし、見つけた時刻は別に残す
        r["detected"] = r["time"].clone();
        r["detected_later"] = json!(true);
        if let Some(t) = m["noted"].as_u64().or(written) {
            r["time"] = json!(t);
        }
        r["version"] = m["version"].clone();
        r["message"] = json!(
            "前回の起動が正しく終わりませんでした(パニックでない異常終了)。次の起動で見つけたので、\
             時刻は最後に操作を記録した頃で、落ちたのはその後です"
        );
        r["pid"] = m["pid"].clone();
        r["started"] = m["started"].clone();
        r["commands"] = m["commands"].clone();
        r["document"] = m["document"].clone();
        r["active_component"] = m["active_component"].clone();
        // その起動のログの終わり(落ちる直前に何をしていたか)
        r["log_file"] = m["log_file"].clone();
        if let Some(f) = m["log_file"].as_str() {
            r["log"] = json!(crate::logging::tail(
                Path::new(f),
                crate::logging::RECENT_LINES
            ));
        }
        if write_report(dir, &r).is_some() {
            n += 1;
        }
        let _ = std::fs::remove_file(&path);
    }
    n
}

/// 報告のファイル(古い順)と、その大きさと時刻。
fn report_files(dir: &Path) -> Vec<(PathBuf, u64, SystemTime)> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut v: Vec<(PathBuf, u64, SystemTime)> = rd
        .flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.starts_with("crash-") && n.ends_with(".json")
        })
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            Some((e.path(), m.len(), m.modified().ok()?))
        })
        .collect();
    v.sort_by(|a, b| a.2.cmp(&b.2).then_with(|| a.0.cmp(&b.0)));
    v
}

/// 上限(数・合計の大きさ・日数)を超えた古い報告を消す。消した数を返す。
pub fn prune(dir: &Path, now: SystemTime) -> usize {
    let mut files = report_files(dir);
    let mut removed = 0;
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    while let Some((path, size, time)) = files.first().cloned() {
        let old = now.duration_since(time).unwrap_or_default() > MAX_AGE;
        if !(old || files.len() > MAX_REPORTS || total > MAX_BYTES) {
            break;
        }
        let _ = std::fs::remove_file(&path);
        total = total.saturating_sub(size);
        files.remove(0);
        removed += 1;
    }
    removed
}

/// 残っている報告の要約(新しい順)。
pub fn list(data_dir: &Path) -> Vec<Summary> {
    let mut out: Vec<Summary> = report_files(&crash_dir(data_dir))
        .into_iter()
        .rev()
        .filter_map(|(path, _, _)| {
            let r: Json = serde_json::from_str(&std::fs::read_to_string(&path).ok()?).ok()?;
            let s = |k: &str| r[k].as_str().unwrap_or_default().to_string();
            Some(Summary {
                name: path.file_name()?.to_string_lossy().into_owned(),
                kind: s("kind"),
                time: r["time"].as_u64().unwrap_or(0),
                app: s("app"),
                version: s("version"),
                message: s("message"),
                detected_later: r["detected_later"].as_bool().unwrap_or(false),
            })
        })
        .collect();
    out.sort_by_key(|s| std::cmp::Reverse(s.time));
    out
}

/// 報告 1 つを読む。`name` は [`list`] のファイル名(フォルダの外は指せない)。
pub fn read(data_dir: &Path, name: &str) -> Result<Json, String> {
    let ok = name.starts_with("crash-")
        && name.ends_with(".json")
        && !name.contains(['/', '\\'])
        && !name.contains("..");
    if !ok {
        return Err(format!("{name} はクラッシュ報告の名前ではありません"));
    }
    let path = crash_dir(data_dir).join(name);
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("{} を読めません: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{name}: {e}"))
}
