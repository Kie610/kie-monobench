//! モデル管理の仕分け: デザインとフォルダを移す・名前を変える・PC のごみ箱へ移す・新しいフォルダ・
//! ここに新規デザイン・分類を直す。
//! 画面・CLI・MCP が同じ関数を使う。どれも戻せるように、行ったことを [`Done`] で返す。
//!
//! デザインを移す・名前を変える時は、版の履歴(`.jc/history/<名前>/`)も一緒に動かし、元のフォルダの
//! `.jc/moved.json` に移した記録を足す。リンクの部品は、保存した絶対パスに元が無い時にこの記録を
//! たどって元を探す(kmb-asm の `link::resolve`。同じ形を読む)。
//!
//! 移した記録の形(`<元のフォルダ>/.jc/moved.json`):
//! `{"moves":[{"from":"天板.json","to":"D:/作品/棚/天板.json","at":1792700000}]}`
//! (`from` は元のファイル名、`to` は移した先の絶対パス、`at` は UNIX 時刻の秒。同じ名前は最後の物が正)
//! フォルダを移した時は、その親のフォルダの記録に `from` = フォルダの名前で残す。中のデザインは、
//! 上のフォルダの記録をたどって探す([`follow_moves`])。

use crate::history;
use crate::projects::ModelInfo;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 移した記録 1 つ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Moved {
    /// 元のファイル名
    pub from: String,
    /// 移した先の絶対パス
    pub to: PathBuf,
    /// 移した時刻(UNIX 時刻の秒)
    #[serde(default)]
    pub at: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct MovedFile {
    #[serde(default)]
    moves: Vec<Moved>,
}

/// フォルダ `dir` の移した記録のファイル。
pub fn moved_file(dir: &Path) -> PathBuf {
    dir.join(".jc").join("moved.json")
}

/// フォルダ `dir` の移した記録(読めなければ空)。
pub fn moved(dir: &Path) -> Vec<Moved> {
    std::fs::read_to_string(moved_file(dir))
        .ok()
        .and_then(|s| serde_json::from_str::<MovedFile>(&s).ok())
        .map(|f| f.moves)
        .unwrap_or_default()
}

fn write_moved(dir: &Path, moves: Vec<Moved>) -> Result<(), String> {
    let f = moved_file(dir);
    if moves.is_empty() {
        if f.exists() {
            std::fs::remove_file(&f).map_err(|e| format!("{} を消せません: {e}", f.display()))?;
        }
        return Ok(());
    }
    if let Some(d) = f.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{} を作れません: {e}", d.display()))?;
    }
    let s = serde_json::to_string_pretty(&MovedFile { moves }).expect("JSON にできる");
    crate::write_atomic(&f, &(s + "\n"), false)
}

/// 行ったこと 1 つ(戻す時に逆に行う)。
#[derive(Debug, Clone, PartialEq)]
pub enum Done {
    /// デザインを `from` から `to` へ移した(名前を変えたのも同じ)
    Move { from: PathBuf, to: PathBuf },
    /// デザインを PC のごみ箱へ移した(版の履歴も)
    Trash { path: PathBuf },
    /// フォルダを作った
    NewFolder { path: PathBuf },
    /// フォルダを `from` から `to` へ移した(名前を変えたのも同じ。中の物も一緒に)
    MoveFolder { from: PathBuf, to: PathBuf },
    /// フォルダを PC のごみ箱へ移した(中の物も)
    TrashFolder { path: PathBuf },
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().to_string())
}

/// 名前に使えない文字(Windows のファイル名で使えない物と、フォルダの区切り)。
fn bad_name(name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() {
        return Some("名前が空です".into());
    }
    if name == "." || name == ".." || name.starts_with('.') {
        return Some("名前を「.」で始められません".into());
    }
    if let Some(c) = name.chars().find(|c| {
        matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control()
    }) {
        return Some(format!("名前に「{c}」は使えません"));
    }
    None
}

/// ファイルかフォルダを動かす(別のドライブなら写してから消す)。
fn rename(from: &Path, to: &Path) -> Result<(), String> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) if from.is_file() => {
            std::fs::copy(from, to)
                .map_err(|e| format!("{} を {} へ写せません: {e}", from.display(), to.display()))?;
            std::fs::remove_file(from).map_err(|e| format!("{} を消せません: {e}", from.display()))
        }
        Err(_) if from.is_dir() => {
            copy_dir(from, to)?;
            std::fs::remove_dir_all(from)
                .map_err(|e| format!("{} を消せません: {e}", from.display()))
        }
        Err(e) => Err(format!(
            "{} を {} へ移せません: {e}",
            from.display(),
            to.display()
        )),
    }
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("{} を作れません: {e}", to.display()))?;
    let rd =
        std::fs::read_dir(from).map_err(|e| format!("{} を読めません: {e}", from.display()))?;
    for e in rd.flatten() {
        let (s, d) = (e.path(), to.join(e.file_name()));
        if s.is_dir() {
            copy_dir(&s, &d)?;
        } else {
            std::fs::copy(&s, &d).map_err(|e| format!("{} を写せません: {e}", s.display()))?;
        }
    }
    Ok(())
}

/// デザイン `path` をフォルダ `dir` へ、名前 `name`(拡張子を含むファイル名。無ければ今の名前)で移す。
/// 版の履歴も一緒に動かし、元のフォルダに移した記録を足す。同じ名前の物が行き先にあれば移さない。
/// 移した先のパスを返す(行き先が今と同じなら何もせず今のパス)。
pub fn move_design(path: &Path, dir: &Path, name: Option<&str>) -> Result<PathBuf, String> {
    let name = name.map_or_else(|| file_name(path), |n| n.trim().to_string());
    if let Some(e) = bad_name(&name) {
        return Err(e);
    }
    if !path.is_file() {
        return Err(format!("{} が見つかりません", path.display()));
    }
    if !dir.is_dir() {
        return Err(format!("{} はフォルダではありません", dir.display()));
    }
    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let dir = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
    let to = dir.join(&name);
    if to == path {
        return Ok(to);
    }
    // Windows は大文字と小文字だけ違う名前を同じファイルと見る。名前の大小だけ変える時は通す
    let same_file =
        to.exists() && std::fs::canonicalize(&to).ok() == std::fs::canonicalize(&path).ok();
    if to.exists() && !same_file {
        return Err(format!(
            "{} に同じ名前の「{name}」があります",
            dir.display()
        ));
    }
    let (h_from, h_to) = (history::dir(&path), history::dir(&to));
    let has_history = h_from.is_dir();
    if has_history && h_to.exists() && !same_file {
        return Err(format!(
            "{} に「{name}」の版の履歴が残っています(先に片付けてください)",
            dir.display()
        ));
    }
    rename(&path, &to)?;
    if has_history {
        let moved = h_to
            .parent()
            .map_or(Ok(()), |p| {
                std::fs::create_dir_all(p).map_err(|e| format!("{} を作れません: {e}", p.display()))
            })
            .and_then(|()| rename(&h_from, &h_to));
        if let Err(e) = moved {
            // 履歴が付いて来ないなら、ファイルも元へ戻して止める(黙って履歴と離さない)
            let back = rename(&to, &path);
            log::error!("版の履歴を移せないので戻した: {e}");
            return Err(match back {
                Ok(()) => format!("版の履歴を移せないので、移すのをやめました: {e}"),
                Err(b) => format!("版の履歴を移せず、戻すのにも失敗しました: {e} / {b}"),
            });
        }
    }
    let src_dir = path.parent().unwrap_or(Path::new("."));
    let from = file_name(&path);
    let mut m = moved(src_dir);
    m.retain(|x| x.from != from);
    m.push(Moved {
        from,
        to: to.clone(),
        at: now(),
    });
    if let Err(e) = write_moved(src_dir, m) {
        log::warn!("移した記録を書けない(移すのは済んだ): {e}");
    }
    log::info!("デザインを移した: {} → {}", path.display(), to.display());
    Ok(to)
}

/// デザインの名前を変える(拡張子は今のまま)。版の履歴も一緒に。新しいパスを返す。
pub fn rename_design(path: &Path, new_stem: &str) -> Result<PathBuf, String> {
    let stem = new_stem.trim();
    if let Some(e) = bad_name(stem) {
        return Err(e);
    }
    let name = match path.extension() {
        Some(x) => format!("{stem}.{}", x.to_string_lossy()),
        None => stem.to_string(),
    };
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    move_design(path, dir, Some(&name))
}

/// [`move_design`] を戻す: `to` を `from` へ戻し、足した移した記録を消す。
pub fn undo_move(from: &Path, to: &Path) -> Result<(), String> {
    let dir = from
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    move_design(to, dir, Some(&file_name(from)))?;
    drop_records(from, to);
    Ok(())
}

/// 戻したことで行き先の側に足された記録と、元の側の記録の両方を消す。
fn drop_records(from: &Path, to: &Path) {
    let dir = from
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    for (d, name) in [
        (dir.to_path_buf(), file_name(from)),
        (
            to.parent().map(Path::to_path_buf).unwrap_or_default(),
            file_name(to),
        ),
    ] {
        let mut m = moved(&d);
        let n = m.len();
        m.retain(|x| x.from != name);
        if m.len() != n
            && let Err(e) = write_moved(&d, m)
        {
            log::warn!("移した記録を直せない: {e}");
        }
    }
}

/// `path` が見つからない時、移した記録(自分か上のフォルダの物)をたどって今の場所を探す。
/// 見つかればそのパス(ファイルがある物だけ)。リンクの部品(kmb-asm の `link::follow_moves`)も同じ読み方。
pub fn follow_moves(path: &Path) -> Option<PathBuf> {
    let mut cur = path.to_path_buf();
    for _ in 0..16 {
        let next = cur.ancestors().find_map(|a| {
            let name = a.file_name()?.to_string_lossy().to_string();
            let m = moved(a.parent()?)
                .into_iter()
                .rev()
                .find(|m| m.from == name)?;
            let rest = cur.strip_prefix(a).ok()?;
            Some(if rest.as_os_str().is_empty() {
                m.to
            } else {
                m.to.join(rest)
            })
        })?;
        if next.is_file() {
            return Some(next);
        }
        cur = next;
    }
    None
}

/// フォルダ `path` をフォルダ `dir` の中へ、名前 `name`(無ければ今の名前)で移す。中のデザインと
/// 版の履歴(`.jc`)も一緒に動く。元の親のフォルダに移した記録を足す。移した先のパスを返す。
pub fn move_folder(path: &Path, dir: &Path, name: Option<&str>) -> Result<PathBuf, String> {
    let name = name.map_or_else(|| file_name(path), |n| n.trim().to_string());
    if let Some(e) = bad_name(&name) {
        return Err(e);
    }
    if !path.is_dir() {
        return Err(format!("{} が見つかりません", path.display()));
    }
    if !dir.is_dir() {
        return Err(format!("{} はフォルダではありません", dir.display()));
    }
    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let dir = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
    let to = dir.join(&name);
    if to == path {
        return Ok(to);
    }
    if dir.starts_with(&path) {
        return Err(format!(
            "「{}」をその中のフォルダへは移せません",
            file_name(&path)
        ));
    }
    let same_file =
        to.exists() && std::fs::canonicalize(&to).ok() == std::fs::canonicalize(&path).ok();
    if to.exists() && !same_file {
        return Err(format!(
            "{} に同じ名前の「{name}」があります",
            dir.display()
        ));
    }
    rename(&path, &to)?;
    let src_dir = path.parent().unwrap_or(Path::new("."));
    let from = file_name(&path);
    let mut m = moved(src_dir);
    m.retain(|x| x.from != from);
    m.push(Moved {
        from,
        to: to.clone(),
        at: now(),
    });
    if let Err(e) = write_moved(src_dir, m) {
        log::warn!("移した記録を書けない(移すのは済んだ): {e}");
    }
    log::info!("フォルダを移した: {} → {}", path.display(), to.display());
    Ok(to)
}

/// フォルダの名前を変える。新しいパスを返す。
pub fn rename_folder(path: &Path, new_name: &str) -> Result<PathBuf, String> {
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    move_folder(path, dir, Some(new_name))
}

/// [`move_folder`] を戻す: `to` を `from` へ戻し、足した移した記録を消す。
pub fn undo_move_folder(from: &Path, to: &Path) -> Result<(), String> {
    let dir = from
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    move_folder(to, dir, Some(&file_name(from)))?;
    drop_records(from, to);
    Ok(())
}

/// フォルダを PC のごみ箱へ移す(中のデザインと版の履歴も)。
pub fn trash_folder(path: &Path) -> Result<(), String> {
    if !path.is_dir() {
        return Err(format!("{} が見つかりません", path.display()));
    }
    trash::delete(path).map_err(|e| format!("{} をごみ箱へ移せません: {e}", path.display()))?;
    log::info!("フォルダをごみ箱へ移した: {}", path.display());
    Ok(())
}

/// フォルダ `dir` に新しいデザインのファイルを作る(名前は `name`、あれば「name 2」…)。
/// 中身は `doc`(新規デザインの既定の値を入れた空の文書)。版の履歴にも最初の版を残す。作ったパスを返す。
pub fn new_design(dir: &Path, name: &str, doc: &kmb_core::Document) -> Result<PathBuf, String> {
    if let Some(e) = bad_name(name) {
        return Err(e);
    }
    if !dir.is_dir() {
        return Err(format!("{} はフォルダではありません", dir.display()));
    }
    let name = name.trim();
    let path = (1..1000)
        .map(|k| {
            let stem = if k == 1 {
                name.to_string()
            } else {
                format!("{name} {k}")
            };
            dir.join(format!("{stem}.{}", kmb_core::DOCUMENT_EXTENSION))
        })
        .find(|p| !p.exists() && !history::dir(p).exists())
        .ok_or_else(|| format!("{} に「{name}」の名前の空きがありません", dir.display()))?;
    crate::save(&path, doc)?;
    log::info!("新規デザインを作った: {}", path.display());
    Ok(path)
}

/// デザインを PC のごみ箱へ移す(版の履歴も一緒に)。
pub fn trash_design(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err(format!("{} が見つかりません", path.display()));
    }
    let h = history::dir(path);
    let mut items = vec![path.to_path_buf()];
    if h.is_dir() {
        items.push(h);
    }
    trash::delete_all(&items)
        .map_err(|e| format!("{} をごみ箱へ移せません: {e}", path.display()))?;
    log::info!("デザインをごみ箱へ移した: {}", path.display());
    Ok(())
}

/// ごみ箱へ移したデザイン(と版の履歴)を元の場所へ戻す。Windows と Linux だけ(macOS は
/// ごみ箱の中を読む口が無いので、Finder で戻してもらう)。
pub fn restore_design(path: &Path) -> Result<(), String> {
    restore(path, &[path.to_path_buf(), history::dir(path)])
}

/// ごみ箱へ移したフォルダ(中の物も)を元の場所へ戻す。
pub fn restore_folder(path: &Path) -> Result<(), String> {
    restore(path, &[path.to_path_buf()])
}

/// ごみ箱の元の場所と比べる綴り。親のフォルダ(ごみ箱へ移した後も残る)を実際の綴りに直し、
/// 名前を足す。Windows のごみ箱は元の場所を長い名前で覚えるが、一時フォルダなどは短い名前
/// (`C:\Users\RUNNER~1\…`)で渡ることがあり、そのままでは見つからなかった。
fn same_spelling(p: &Path) -> PathBuf {
    match (p.parent(), p.file_name()) {
        (Some(dir), Some(name)) => std::fs::canonicalize(dir)
            .map(|d| d.join(name))
            .unwrap_or_else(|_| p.to_path_buf()),
        _ => p.to_path_buf(),
    }
}

/// ごみ箱の中の、元が `want` の物を戻す(`path` が無ければ誤り)。
fn restore(path: &Path, want: &[PathBuf]) -> Result<(), String> {
    #[cfg(any(
        windows,
        all(
            unix,
            not(target_os = "macos"),
            not(target_os = "ios"),
            not(target_os = "android")
        )
    ))]
    {
        let target = same_spelling(path);
        let want: Vec<PathBuf> = want.iter().map(|w| same_spelling(w)).collect();
        let items: Vec<trash::TrashItem> = trash::os_limited::list()
            .map_err(|e| format!("ごみ箱を読めません: {e}"))?
            .into_iter()
            .filter(|i| want.contains(&same_spelling(&i.original_parent.join(&i.name))))
            .collect();
        if !items
            .iter()
            .any(|i| same_spelling(&i.original_parent.join(&i.name)) == target)
        {
            return Err(format!("{} がごみ箱に見つかりません", path.display()));
        }
        // 同じ物を何度も消していれば、いちばん新しい物だけ戻す
        let mut latest: Vec<trash::TrashItem> = Vec::new();
        for i in items {
            let key = i.original_parent.join(&i.name);
            match latest
                .iter_mut()
                .find(|l| l.original_parent.join(&l.name) == key)
            {
                Some(l) if l.time_deleted < i.time_deleted => *l = i,
                Some(_) => {}
                None => latest.push(i),
            }
        }
        trash::os_limited::restore_all(latest)
            .map_err(|e| format!("{} をごみ箱から戻せません: {e:?}", path.display()))?;
        log::info!("ごみ箱から戻した: {}", path.display());
        Ok(())
    }
    #[cfg(not(any(
        windows,
        all(
            unix,
            not(target_os = "macos"),
            not(target_os = "ios"),
            not(target_os = "android")
        )
    )))]
    {
        let _ = want;
        Err(format!(
            "この OS ではごみ箱から戻せません。{} を Finder のごみ箱から戻してください",
            path.display()
        ))
    }
}

/// フォルダ `parent` の中に新しいフォルダ `name` を作る。作ったパスを返す。
pub fn new_folder(parent: &Path, name: &str) -> Result<PathBuf, String> {
    if let Some(e) = bad_name(name) {
        return Err(e);
    }
    if !parent.is_dir() {
        return Err(format!("{} はフォルダではありません", parent.display()));
    }
    let p = parent.join(name.trim());
    if p.exists() {
        return Err(format!(
            "{} に同じ名前の「{}」があります",
            parent.display(),
            name.trim()
        ));
    }
    std::fs::create_dir(&p).map_err(|e| format!("{} を作れません: {e}", p.display()))?;
    log::info!("フォルダを作った: {}", p.display());
    Ok(p)
}

/// [`Done`] を戻す。
pub fn undo(done: &Done) -> Result<(), String> {
    match done {
        Done::Move { from, to } => undo_move(from, to),
        Done::Trash { path } => restore_design(path),
        Done::NewFolder { path } => std::fs::remove_dir(path).map_err(|e| {
            format!(
                "{} を消せません(中に物があるかもしれません): {e}",
                path.display()
            )
        }),
        Done::MoveFolder { from, to } => undo_move_folder(from, to),
        Done::TrashFolder { path } => restore_folder(path),
    }
}

/// 分類を直す(開かずに。設計の属性の部品名・作品・ジャンル・タグ・材料を `info` にする)。
/// 文書を読み、属性を置き換えて保存し、版の履歴に「分類を変更」を残す。変わらなければ何もしない。
/// 真なら書いた。
pub fn set_info(path: &Path, info: &ModelInfo) -> Result<bool, String> {
    let src = std::fs::read_to_string(path)
        .map_err(|e| format!("{} を読めません: {e}", path.display()))?;
    let mut doc = kmb_core::Document::from_json(&src)
        .map_err(|e| format!("{} を読めません: {e}", path.display()))?;
    let mut p = doc.data().properties.clone();
    let before = p.clone();
    p.title = info.title.trim().to_string();
    p.project = info.project.trim().to_string();
    p.genre = info.genre.trim().to_string();
    p.material = info.material.trim().to_string();
    p.tags = info
        .tags
        .iter()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    if p == before {
        return Ok(false);
    }
    doc.apply(crate::edit::set_properties(p))
        .map_err(|e| format!("分類を変えられません: {e}"))?;
    // 縮小図と形の数は前の版の物を引き継ぐ(形は変わらない)
    crate::save_with(path, &doc, &history::latest_snapshot(path), "分類を変更")?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kmb-organize-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn doc_at(p: &Path) {
        let d = kmb_core::Document::new();
        crate::save(p, &d).unwrap();
    }

    #[test]
    fn a_design_moves_with_its_history_and_leaves_a_record_and_comes_back() {
        let t = temp("move");
        let (a, b) = (t.as_path().join("a"), t.as_path().join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let p = a.join("天板.json");
        doc_at(&p);
        assert_eq!(history::list(&p).len(), 1);
        let to = move_design(&p, &b, None).unwrap();
        assert_eq!(to, std::path::absolute(b.join("天板.json")).unwrap());
        assert!(!p.exists() && to.is_file());
        assert_eq!(history::list(&to).len(), 1, "版の履歴も一緒に動く");
        assert!(!history::dir(&p).exists());
        let m = moved(&a);
        assert_eq!(m.len(), 1);
        assert_eq!((m[0].from.as_str(), &m[0].to), ("天板.json", &to));
        // 戻すと記録も消える
        undo(&Done::Move {
            from: std::path::absolute(&p).unwrap(),
            to: to.clone(),
        })
        .unwrap();
        assert!(p.is_file() && !to.exists());
        assert_eq!(history::list(&p).len(), 1);
        assert!(moved(&a).is_empty() && moved(&b).is_empty());
        assert!(!moved_file(&a).exists());
    }

    #[test]
    fn a_design_is_not_moved_over_another_or_with_a_bad_name() {
        let t = temp("overwrite");
        let (a, b) = (t.as_path().join("a"), t.as_path().join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        doc_at(&a.join("x.json"));
        doc_at(&b.join("x.json"));
        let e = move_design(&a.join("x.json"), &b, None).unwrap_err();
        assert!(e.contains("同じ名前"), "{e}");
        assert!(a.join("x.json").is_file(), "移さない");
        for bad in ["", "a/b", "c:d", ".隠し"] {
            assert!(rename_design(&a.join("x.json"), bad).is_err(), "{bad}");
        }
        // 名前を変えると拡張子はそのままで、履歴も付いて来る
        let to = rename_design(&a.join("x.json"), "y").unwrap();
        assert_eq!(file_name(&to), "y.json");
        assert_eq!(history::list(&to).len(), 1);
        assert_eq!(moved(&a)[0].from, "x.json");
    }

    #[test]
    fn a_new_folder_is_made_once_and_undone_while_empty() {
        let t = temp("folder");
        let p = new_folder(t.as_path(), "部品").unwrap();
        assert!(p.is_dir());
        assert!(new_folder(t.as_path(), "部品").is_err());
        undo(&Done::NewFolder { path: p.clone() }).unwrap();
        assert!(!p.exists());
    }

    #[test]
    fn classification_is_changed_without_opening_and_kept_as_a_version() {
        let t = temp("info");
        let p = t.as_path().join("棚.json");
        doc_at(&p);
        let info = ModelInfo {
            title: "棚板".into(),
            project: "本棚".into(),
            genre: "家具".into(),
            tags: vec!["木".into(), " ".into()],
            material: "パイン".into(),
            ..Default::default()
        };
        assert!(set_info(&p, &info).unwrap());
        let got = crate::projects::model_info(&p);
        assert_eq!(
            (got.title, got.project, got.genre, got.tags, got.material),
            (
                "棚板".into(),
                "本棚".into(),
                "家具".into(),
                vec!["木".to_string()],
                "パイン".into()
            )
        );
        let v = history::list(&p);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].note, "分類を変更");
        // 同じなら書かない
        assert!(!set_info(&p, &crate::projects::model_info(&p)).unwrap());
        assert_eq!(history::list(&p).len(), 2);
    }

    /// 本物の PC のごみ箱を使う(試験の一時フォルダの物を移して、すぐ戻す)。
    #[test]
    #[cfg(any(windows, target_os = "linux"))]
    fn a_design_goes_to_the_trash_with_its_history_and_comes_back() {
        let t = temp("trash");
        let p = t.join(format!("ごみ箱の試験-{}.json", std::process::id()));
        doc_at(&p);
        trash_design(&p).unwrap();
        assert!(!p.exists() && !history::dir(&p).exists());
        undo(&Done::Trash { path: p.clone() }).unwrap();
        assert!(p.is_file());
        assert_eq!(history::list(&p).len(), 1, "版の履歴も戻る");
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn a_folder_moves_and_renames_with_what_is_inside_and_comes_back() {
        let t = temp("move-folder");
        let (a, b) = (t.as_path().join("a"), t.as_path().join("b"));
        std::fs::create_dir_all(a.join("部品")).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let p = a.join("部品").join("脚.json");
        doc_at(&p);
        assert!(
            move_folder(&a, &a.join("部品"), None).is_err(),
            "自分の中へは移せない"
        );
        let to = move_folder(&a.join("部品"), &b, None).unwrap();
        let moved_p = to.join("脚.json");
        assert!(moved_p.is_file() && !p.exists());
        assert_eq!(history::list(&moved_p).len(), 1, "版の履歴も一緒に動く");
        // 中のデザインは、上のフォルダの移した記録をたどって探せる
        assert_eq!(follow_moves(&p), Some(moved_p.clone()));
        assert!(move_folder(&to, &b, Some("a/b")).is_err());
        // 名前を変えた後もたどれる(記録が 2 段)
        let renamed = rename_folder(&to, "小物").unwrap();
        assert_eq!(follow_moves(&p), Some(renamed.join("脚.json")));
        undo(&Done::MoveFolder {
            from: to.clone(),
            to: renamed.clone(),
        })
        .unwrap();
        undo(&Done::MoveFolder {
            from: std::path::absolute(a.join("部品")).unwrap(),
            to: to.clone(),
        })
        .unwrap();
        assert!(p.is_file() && !to.exists());
        assert!(moved(&a).is_empty() && moved(&b).is_empty());
        // 同じ名前の物があれば移さない
        std::fs::create_dir_all(b.join("部品")).unwrap();
        assert!(move_folder(&a.join("部品"), &b, None).is_err());
    }

    #[test]
    fn a_new_design_gets_a_free_name_and_a_first_version() {
        let t = temp("new-design");
        let doc = kmb_core::Document::new();
        let p = new_design(t.as_path(), "新規デザイン", &doc).unwrap();
        assert_eq!(
            p,
            t.join(format!("新規デザイン.{}", kmb_core::DOCUMENT_EXTENSION))
        );
        assert!(crate::projects::is_document(&p));
        assert_eq!(history::list(&p).len(), 1);
        let q = new_design(t.as_path(), "新規デザイン", &doc).unwrap();
        assert_eq!(
            q,
            t.join(format!("新規デザイン 2.{}", kmb_core::DOCUMENT_EXTENSION))
        );
        assert!(new_design(&t.join("無い"), "x", &doc).is_err());
        assert!(new_design(t.as_path(), "a:b", &doc).is_err());
    }

    /// 本物の PC のごみ箱を使う(試験の一時フォルダの物を移して、すぐ戻す)。
    #[test]
    #[cfg(any(windows, target_os = "linux"))]
    fn a_folder_goes_to_the_trash_with_what_is_inside_and_comes_back() {
        let t = temp("trash-folder");
        let f = t.join(format!("ごみ箱の試験のフォルダ-{}", std::process::id()));
        std::fs::create_dir_all(&f).unwrap();
        doc_at(&f.join("脚.json"));
        trash_folder(&f).unwrap();
        assert!(!f.exists());
        undo(&Done::TrashFolder { path: f.clone() }).unwrap();
        assert!(f.join("脚.json").is_file());
        assert_eq!(history::list(&f.join("脚.json")).len(), 1);
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn the_moved_record_has_the_documented_shape() {
        // kmb-asm の link::resolve が同じ形を読む(あちらの試験も同じ文字列を使う)
        let t = temp("shape");
        std::fs::create_dir_all(t.as_path().join(".jc")).unwrap();
        std::fs::write(
            moved_file(t.as_path()),
            r#"{"moves":[{"from":"天板.json","to":"D:/作品/棚/天板.json","at":1792700000}]}"#,
        )
        .unwrap();
        assert_eq!(
            moved(t.as_path()),
            vec![Moved {
                from: "天板.json".into(),
                to: PathBuf::from("D:/作品/棚/天板.json"),
                at: 1792700000
            }]
        );
    }
}
