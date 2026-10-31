//! ファイルの扱いと、画面を持たない作業([`Headless`])。画面(kmb-ui)・CLI・MCP が共通で使う。
//! コアと、測る計算(kmb-measure)と、形を絵にする計算(kmb-render の picture)にだけ依存する。
//!
//! - 保存は同じフォルダの一時ファイルに書いてから置き換え、直前の版を `.bak` に残す
//!   (書いている途中で落ちても、元のファイルは壊れない)。
//! - 自動保存は、ユーザーのデータフォルダの `recovery` に復旧用の写しを書く。
//!   保存したら消す。起動時に残っていれば、開始画面で復旧できる。
//! - 最近使ったファイルは、データフォルダの `recent.json` に持つ。読めなければ空とみなす
//!   (設定ファイルのせいで起動できなくしない)。
//! - クラッシュ報告は、データフォルダの `crash` に残す(送信しない。[`crash`])。
//! - ログは、データフォルダの `logs` に残す(送信しない。[`logging`])。
//! - 保存のたびに、文書の横の `.jc/history` に版を残す([`history`]。書けなくても保存は成功のまま)。

pub mod canvas;
pub mod components;
pub mod crash;
pub mod doc_view;
pub mod edit;
pub mod filters;
pub mod gallery;
mod headless;
pub mod history;
mod inspect;
pub mod library;
pub mod live;
pub mod near;
pub use inspect::{MeasurePick, PictureSpec, body_material};
pub mod logging;
pub mod organize;
pub mod param_batch;
pub mod param_io;
pub mod params;
pub mod projects;
pub mod roots;
pub mod run_args;
pub mod startup;
pub mod tidy;
pub use headless::{
    After, BodyMass, Handled, Headless, Picks, Polled, Ran, Recomputed, Settled, SheetFacts,
};

use kmb_core::Document;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// 文書を管理する共通の操作。
pub fn module() -> kmb_core::Module {
    kmb_core::Module::commands("kmb-session").with_extension(param_batch::Commands)
}

/// 最近使ったファイルの数の上限。
pub const RECENT_MAX: usize = 10;
/// 自動保存の間隔。
pub const AUTOSAVE_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// データフォルダの名前。
pub const DATA_DIR_NAME: &str = "kie-monobench";
/// 改名の前のデータフォルダの名前。中身は新しいフォルダへ写して引き継ぐ(`carry_over`)。
pub const OLD_DATA_DIR_NAME: &str = "jisaku-cadcam";

/// ユーザーのデータフォルダ。Windows は `%APPDATA%\kie-monobench`、
/// それ以外は `$XDG_DATA_HOME/kie-monobench` か `~/.local/share/kie-monobench`。
/// 新しいフォルダが無く、改名の前のフォルダがあれば、その中身を写してから返す。
pub fn default_data_dir() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
    }?;
    let dir = base.join(DATA_DIR_NAME);
    // 写せなくても新しいフォルダで始める(設定は既定になるが、起動は止めない)
    if let Err(e) = carry_over(&base.join(OLD_DATA_DIR_NAME), &dir) {
        eprintln!("前のデータフォルダを引き継げません: {e}");
    }
    Some(dir)
}

/// 改名の前のデータフォルダ `old` の中身(設定・並び・窓の記憶など)を `new` へ写す。
/// `new` が既にあるか `old` が無ければ何もしない。ログ(`logs`)は写さない。
/// 前の版に戻した時のため、`old` は消さずに残す。途中で切れても半分だけの `new` を残さないよう、
/// 隣のフォルダへ写してから名前を替える。写したら true。
pub fn carry_over(old: &Path, new: &Path) -> std::io::Result<bool> {
    if new.exists() || !old.is_dir() {
        return Ok(false);
    }
    let moving = new.with_extension("moving");
    let _ = std::fs::remove_dir_all(&moving);
    copy_tree(old, &moving, &[logging::LOG_DIR_NAME])?;
    std::fs::rename(&moving, new)?;
    Ok(true)
}

/// `from` の下を `to` へ写す。`skip` は直下で写さない名前。
fn copy_tree(from: &Path, to: &Path, skip: &[&str]) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let name = e.file_name();
        if skip.iter().any(|s| name == *s) {
            continue;
        }
        let kind = e.file_type()?;
        if kind.is_dir() {
            copy_tree(&e.path(), &to.join(&name), &[])?;
        } else if kind.is_file() {
            std::fs::copy(e.path(), to.join(&name))?;
        }
    }
    Ok(())
}

/// 文書のファイルを読む。
pub fn load(path: &Path) -> Result<Document, kmb_core::Problem> {
    use kmb_core::{Problem, ProblemKind};
    let r = std::fs::read_to_string(path)
        .map_err(|e| {
            Problem::new(
                ProblemKind::File,
                format!("{} を読めません: {e}", path.display()),
            )
        })
        .and_then(|s| {
            Document::from_json(&s).map_err(|e| format!("{}: {e}", path.display()).into())
        });
    match &r {
        Ok(d) => log::info!(
            "開いた: {} (機能 {} 個)",
            path.display(),
            d.data().features.len()
        ),
        Err(e) => log::error!("開けない: {e}"),
    }
    r
}

/// `path` の直前の版の置き場所(`部品.json` なら `部品.json.bak`)。
pub fn backup_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".bak");
    PathBuf::from(s)
}

/// 壊れない書き込み。同じフォルダの一時ファイルに書いて置き換える。
/// `backup` が真で元のファイルがあれば、元を `.bak` に残す。
pub fn write_atomic(path: &Path, contents: impl AsRef<[u8]>, backup: bool) -> Result<(), String> {
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| format!("{} はファイルのパスではありません", path.display()))?;
    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(name);
    tmp_name.push(format!(".{}.tmp", std::process::id()));
    let tmp = dir.join(tmp_name);
    std::fs::write(&tmp, contents.as_ref())
        .map_err(|e| format!("{} に書けません: {e}", tmp.display()))?;
    if backup && path.exists() {
        let bak = backup_path(path);
        if let Err(e) = std::fs::copy(path, &bak) {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("{} を残せません: {e}", bak.display()));
        }
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{} に置き換えられません: {e}", path.display())
    })
}

/// 書き出したファイル(G-code・STL・画像・表など)を壊れない書き込みで置き、ログに残す。画面・CLI・
/// MCP・[`Headless`] の書き出しはみなここを通す(途中で落ちても半分のファイルを残さず、ログも必ず残す)。
pub fn write_output(path: &Path, contents: &[u8]) -> Result<(), kmb_core::Problem> {
    let r = write_atomic(path, contents, false)
        .map_err(|e| kmb_core::Problem::new(kmb_core::ProblemKind::File, e));
    match &r {
        Ok(()) => log::info!("書き出した: {}({} バイト)", path.display(), contents.len()),
        Err(e) => log::warn!("書き出せない: {e}"),
    }
    r
}

/// 文書を保存する(壊れない書き込み、直前の版を `.bak` に残す)。版の履歴にも残す(縮小図なし)。
pub fn save(path: &Path, doc: &Document) -> Result<(), String> {
    save_with(path, doc, &history::Snapshot::default(), "")
}

/// 文書を保存し、版の履歴に `snap`(縮小図と形の数)と説明 `note` を付けて残す。
/// 履歴を書けなくても保存は成功のまま(ログに残す)。
pub fn save_with(
    path: &Path,
    doc: &Document,
    snap: &history::Snapshot,
    note: &str,
) -> Result<(), String> {
    let r = write_atomic(path, &(doc.to_json() + "\n"), true);
    match &r {
        Ok(()) => {
            log::info!("保存した: {}", path.display());
            if let Err(e) = history::record(path, doc, snap, note) {
                log::warn!("版の履歴を残せない: {e}");
            }
        }
        Err(e) => log::error!("保存できない: {e}"),
    }
    r
}

/// 最近使ったファイル(新しい順)。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Recent {
    pub files: Vec<PathBuf>,
    /// 留めたファイル(一覧の先頭にいつも出し、数の上限と「一覧を消す」で消えない。U15)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pinned: Vec<PathBuf>,
}

impl Recent {
    fn file(dir: &Path) -> PathBuf {
        dir.join("recent.json")
    }

    /// 読めなければ空(設定のせいで起動できなくしない)。
    pub fn load(dir: &Path) -> Recent {
        std::fs::read_to_string(Self::file(dir))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("{} を作れません: {e}", dir.display()))?;
        let s = serde_json::to_string_pretty(self).expect("JSON にできる");
        write_atomic(&Self::file(dir), &s, false)
    }

    /// 先頭に足す(同じものは前から消す)。
    pub fn push(&mut self, path: &Path) {
        let p = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        self.files.retain(|f| f != &p);
        self.files.insert(0, p);
        self.files.truncate(RECENT_MAX);
    }

    /// 出す順の一覧: 留めたもの、その後に留めていないもの(新しい順)。
    pub fn list(&self) -> Vec<PathBuf> {
        let mut v = self.pinned.clone();
        v.extend(
            self.files
                .iter()
                .filter(|f| !self.pinned.contains(f))
                .cloned(),
        );
        v
    }

    /// 今はないファイルを除いた一覧。
    pub fn existing(&self) -> Vec<PathBuf> {
        self.list().into_iter().filter(|p| p.exists()).collect()
    }

    pub fn is_pinned(&self, path: &Path) -> bool {
        self.pinned.iter().any(|p| p == path)
    }

    /// 留める・留めを外す。
    pub fn set_pinned(&mut self, path: &Path, pin: bool) {
        self.pinned.retain(|p| p != path);
        if pin {
            self.pinned.push(path.to_path_buf());
        }
    }

    /// ファイルを移した・名前を変えた時に、一覧のパスを付け替える(並びとピン留めはそのまま)。
    pub fn moved(&mut self, from: &Path, to: &Path) {
        for p in self.files.iter_mut().chain(self.pinned.iter_mut()) {
            if p == from {
                *p = to.to_path_buf();
            }
        }
    }

    /// フォルダ `from` を `to` へ移した後に、中のファイルのパスを付け替える。
    pub fn moved_folder(&mut self, from: &Path, to: &Path) {
        for p in self.files.iter_mut().chain(self.pinned.iter_mut()) {
            if let Ok(rest) = p.strip_prefix(from) {
                *p = to.join(rest);
            }
        }
    }

    /// 一覧から外す(留めていても外す。ファイルは消さない)。
    pub fn remove(&mut self, path: &Path) {
        self.files.retain(|p| p != path);
        self.pinned.retain(|p| p != path);
    }

    /// 留めていないものを一覧から外す。
    pub fn clear_unpinned(&mut self) {
        let pinned = self.pinned.clone();
        self.files.retain(|p| pinned.contains(p));
    }
}

/// ファイルのあるフォルダを OS のファイルの窓で開く(Windows と macOS はファイルを選んだ状態で)。
/// 窓を開くのは OS に任せ、待たない。パッケージは足さない(std の `Command`)。
pub fn show_in_folder(path: &Path) -> Result<(), String> {
    let dir = path
        .parent()
        .filter(|d| d.exists())
        .ok_or_else(|| format!("{} のフォルダが見つかりません", path.display()))?;
    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = std::process::Command::new("explorer");
        if path.exists() {
            // explorer は `/select,` とパスを 1 つの引数で受ける
            let mut arg = std::ffi::OsString::from("/select,");
            arg.push(path.as_os_str());
            c.arg(arg);
        } else {
            c.arg(dir);
        }
        c
    } else if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("open");
        if path.exists() {
            c.arg("-R").arg(path);
        } else {
            c.arg(dir);
        }
        c
    } else {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(dir);
        c
    };
    log::info!("フォルダを開く: {}", dir.display());
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("フォルダを開けません({}): {e}", dir.display()))
}

/// フォルダそのものを OS のファイルの窓で開く(エクスプローラー・Finder など。待たない)。
pub fn open_folder(dir: &Path) -> Result<(), String> {
    if !dir.is_dir() {
        return Err(format!("{} が見つかりません", dir.display()));
    }
    let prog = if cfg!(target_os = "windows") {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    log::info!("フォルダを開く: {}", dir.display());
    std::process::Command::new(prog)
        .arg(dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("フォルダを開けません({}): {e}", dir.display()))
}

/// 自動保存の写し 1 つ。
#[derive(Debug, Clone, PartialEq)]
pub struct Recovery {
    /// 写しのファイル
    pub file: PathBuf,
    /// 元の文書のパス(無題なら `None`)
    pub original: Option<PathBuf>,
    pub saved_at: SystemTime,
}

#[derive(Serialize, Deserialize)]
struct RecoveryFile {
    original: Option<PathBuf>,
    document: serde_json::Value,
}

/// 自動保存。変更があれば `AUTOSAVE_INTERVAL` ごとに写しを書く。
#[derive(Debug)]
pub struct Autosave {
    dir: PathBuf,
    /// この作業の写しの名前(起動ごとに変わる)
    name: String,
    last: Option<SystemTime>,
    pub interval: Duration,
}

impl Autosave {
    /// `data_dir` の下の `recovery` に書く。
    pub fn new(data_dir: &Path) -> Autosave {
        // 文書のタブごとに 1 つ作るので、同じ時刻でも名前が重ならないよう通し番号を足す
        static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let stamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let name = if seq == 0 {
            format!("{stamp}-{}.json", std::process::id())
        } else {
            format!("{stamp}-{}-{seq}.json", std::process::id())
        };
        Autosave {
            dir: data_dir.join("recovery"),
            name,
            last: None,
            interval: AUTOSAVE_INTERVAL,
        }
    }

    fn file(&self) -> PathBuf {
        self.dir.join(&self.name)
    }

    /// 書く時か。最初の変更からも `interval` 待つ。
    pub fn due(&mut self, now: SystemTime, modified: bool) -> bool {
        if !modified {
            self.last = None;
            return false;
        }
        match self.last {
            None => {
                self.last = Some(now);
                false
            }
            Some(t) => now.duration_since(t).unwrap_or_default() >= self.interval,
        }
    }

    /// 写しを書く。
    pub fn write(
        &mut self,
        doc: &Document,
        original: Option<&Path>,
        now: SystemTime,
    ) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir)
            .map_err(|e| format!("{} を作れません: {e}", self.dir.display()))?;
        let f = RecoveryFile {
            original: original.map(Path::to_path_buf),
            document: serde_json::from_str(&doc.to_json()).expect("文書は JSON"),
        };
        write_atomic(
            &self.file(),
            serde_json::to_string(&f).expect("JSON にできる"),
            false,
        )
        .inspect_err(|e| log::error!("自動保存できない: {e}"))?;
        log::info!("自動保存した: {}", self.file().display());
        self.last = Some(now);
        Ok(())
    }

    /// 保存した(または変更を捨てた)ので写しを消す。
    pub fn clear(&mut self) {
        let _ = std::fs::remove_file(self.file());
        self.last = None;
    }
}

/// PC 全体の設定のファイル(データフォルダの中。設定の名前ごとの値)。
pub const SETTINGS_FILE: &str = "settings.json";

/// データフォルダの設定を読み、モジュールに当てる。当てられない設定は飛ばし、理由を返す。
/// ファイルがなければ何もしない。
pub fn load_settings(ext: &kmb_core::Extensions, data_dir: &Path) -> Vec<String> {
    let path = data_dir.join(SETTINGS_FILE);
    let Ok(s) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let v: serde_json::Value = match serde_json::from_str(&s) {
        Ok(v) => v,
        Err(e) => return vec![format!("{} を読めません: {e}", path.display())],
    };
    let Some(map) = v.as_object() else {
        return vec![format!("{} の形が違います", path.display())];
    };
    let errors: Vec<String> = map
        .iter()
        .filter_map(|(id, value)| {
            ext.set_settings(id, value)
                .err()
                .map(|e| format!("設定「{id}」を使えません: {e}"))
        })
        .collect();
    log::info!("設定を読んだ: {} (項目 {} 個)", path.display(), map.len());
    for e in &errors {
        log::warn!("{e}");
    }
    errors
}

/// 今の設定をデータフォルダに書く。このアプリの知らない設定(外したモジュールのもの)は残し、
/// 直前の中身を `.bak` に残す。読めないファイル(壊れた JSON・表でない)は上書きせず、
/// `settings.json.broken-<時刻>` に移してから今の設定で作る(黙って既定で上書きすると、ほかの
/// モジュールの設定ごと消える)。
pub fn save_settings(ext: &kmb_core::Extensions, data_dir: &Path) -> Result<(), String> {
    let path = data_dir.join(SETTINGS_FILE);
    let mut map = match std::fs::read_to_string(&path) {
        Ok(s) => match serde_json::from_str::<serde_json::Value>(&s) {
            Ok(serde_json::Value::Object(m)) => m,
            _ => {
                let stamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs());
                let aside = data_dir.join(format!("{SETTINGS_FILE}.broken-{stamp}"));
                std::fs::rename(&path, &aside).map_err(|e| {
                    format!(
                        "読めない {} を {} に移せません: {e}",
                        path.display(),
                        aside.display()
                    )
                })?;
                log::warn!("読めない設定のファイルを {} に移した", aside.display());
                serde_json::Map::new()
            }
        },
        Err(_) => serde_json::Map::new(),
    };
    for s in ext.settings() {
        map.insert(s.id, s.value);
    }
    std::fs::create_dir_all(data_dir)
        .map_err(|e| format!("{} を作れません: {e}", data_dir.display()))?;
    let text = serde_json::to_string_pretty(&serde_json::Value::Object(map))
        .expect("JSON の値は文字にできる");
    write_atomic(&path, &text, true)
}

/// ユーザーのデータフォルダの設定をモジュールに当て、データフォルダを渡す(CLI・MCP の起動時)。
/// 当てられない設定は飛ばし、理由を標準エラーに出す。
pub fn apply_user_settings(m: &kmb_core::Modules) {
    if let Some(d) = default_data_dir() {
        m.extensions.set_data_dir(Some(&d));
        for p in load_settings(&m.extensions, &d) {
            eprintln!("{p}");
        }
    }
}

/// `data_dir` に残っている自動保存の写し(新しい順)。読めないものは飛ばす。
pub fn recoveries(data_dir: &Path) -> Vec<Recovery> {
    let Ok(rd) = std::fs::read_dir(data_dir.join("recovery")) else {
        return vec![];
    };
    let mut out: Vec<Recovery> = rd
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| {
            let s = std::fs::read_to_string(e.path()).ok()?;
            let f: RecoveryFile = serde_json::from_str(&s).ok()?;
            Some(Recovery {
                file: e.path(),
                original: f.original,
                saved_at: e.metadata().ok()?.modified().ok()?,
            })
        })
        .collect();
    out.sort_by_key(|b| std::cmp::Reverse(b.saved_at));
    out
}

/// 写しから文書を取り出す。
pub fn restore(r: &Recovery) -> Result<Document, String> {
    let s = std::fs::read_to_string(&r.file)
        .map_err(|e| format!("{} を読めません: {e}", r.file.display()))?;
    let f: RecoveryFile = serde_json::from_str(&s).map_err(|e| e.to_string())?;
    let d = Document::from_json(&f.document.to_string()).map_err(|e| e.to_string());
    match &d {
        Ok(_) => log::info!("自動保存の写しから復旧した: {}", r.file.display()),
        Err(e) => log::error!("自動保存の写しを読めない: {}: {e}", r.file.display()),
    }
    d
}

/// 写しを捨てる。
pub fn discard(r: &Recovery) {
    let _ = std::fs::remove_file(&r.file);
}

#[cfg(test)]
mod tests;
