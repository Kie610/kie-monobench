//! AI に許したフォルダ(作業フォルダと、ほかに許したフォルダ)。
//!
//! MCP の道具が読み書きするパスも、MCP から動いている画面へ送る頼みに入ったファイルの引数も、ここで
//! 確かめる。外を指すもの(`..` やリンクで外へ出るものも)は断る。操作がファイルを読む引数は
//! [`kmb_core::ArgKind::File`] と宣言してあるので、どの操作でも同じ確かめを通る。設定の欄も、パスは
//! [`kmb_core::FieldKind::File`] と宣言し、同じ確かめを通る。

use kmb_core::{Action, ArgKind, FieldKind, FileGate, Settings};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// 読み書きしてよいフォルダ。相対パスは作業フォルダ(最初のフォルダ)から数える。
#[derive(Debug, Clone, PartialEq)]
pub struct Roots {
    dirs: Vec<PathBuf>,
}

impl Roots {
    /// 作業フォルダだけを許す。
    pub fn new(root: PathBuf) -> Self {
        Roots {
            dirs: vec![root.canonicalize().unwrap_or(root)],
        }
    }

    /// 作業フォルダ(先頭)と、ほかに許すフォルダの並びから作る(画面へ送った頼みの写し)。
    /// 空なら `None`。
    pub fn from_dirs(dirs: Vec<PathBuf>) -> Option<Self> {
        let mut it = dirs.into_iter();
        let mut r = Roots::new(it.next()?);
        for d in it {
            r.dirs.push(d.canonicalize().unwrap_or(d));
        }
        Some(r)
    }

    /// 作業フォルダのほかに、読み書きしてよいフォルダを足す。
    pub fn allow(&mut self, dir: &Path) -> Result<(), String> {
        let d = dir
            .canonicalize()
            .map_err(|e| format!("{} がありません: {e}", dir.display()))?;
        if !d.is_dir() {
            return Err(format!("{} はフォルダではありません", dir.display()));
        }
        self.dirs.push(d);
        Ok(())
    }

    /// 作業フォルダ。
    pub fn root(&self) -> &Path {
        &self.dirs[0]
    }

    /// 許したフォルダのすべて(先頭が作業フォルダ)。
    pub fn dirs(&self) -> &[PathBuf] {
        &self.dirs
    }

    fn inside(&self, p: &Path) -> bool {
        self.dirs.iter().any(|r| p.starts_with(r))
    }

    /// 許したフォルダの中のパスにする。外を指すもの(`..` やリンクで出るものも)は断る。
    pub fn resolve(&self, path: &str) -> Result<PathBuf, String> {
        let p = Path::new(path);
        let p = if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.root().join(p)
        };
        let parent = p
            .parent()
            .ok_or_else(|| format!("{path} はファイルのパスではありません"))?;
        let parent = parent
            .canonicalize()
            .map_err(|e| format!("{} がありません: {e}", parent.display()))?;
        let outside = || {
            format!(
                "{path} は作業フォルダ {} と許したフォルダの外です",
                self.root().display()
            )
        };
        if !self.inside(&parent) {
            return Err(outside());
        }
        let name = p
            .file_name()
            .ok_or_else(|| format!("{path} にファイル名がありません"))?;
        let full = parent.join(name);
        // ファイルそのものがリンクなら、指す先も確かめる(行き先のないリンクは断る)
        if full
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            let target = full
                .canonicalize()
                .map_err(|e| format!("{path} のリンクの先がありません: {e}"))?;
            if !self.inside(&target) {
                return Err(outside());
            }
        }
        Ok(full)
    }

    /// 操作 `action` の引数のうち、ファイルの引数を許したフォルダの中の絶対パスに置き換える。
    /// 外を指すパスがあれば、どの引数かを添えて断る(操作は動かさない)。
    pub fn confine_args(&self, action: &Action, args: &mut Value) -> Result<(), String> {
        let Some(map) = args.as_object_mut() else {
            return Ok(());
        };
        for a in action.args.iter().filter(|a| a.kind == ArgKind::File) {
            let Some(Value::String(p)) = map.get(&a.key) else {
                continue;
            };
            if p.trim().is_empty() {
                continue;
            }
            let full = self
                .resolve(p)
                .map_err(|e| format!("{} の {}: {e}", action.id, a.key))?;
            map.insert(a.key.clone(), Value::String(full.display().to_string()));
        }
        Ok(())
    }

    /// 設定 `group` の値 `value` のうち、ファイルの欄を許したフォルダの中の絶対パスに置き換える。
    /// 外を指すパスがあれば、どの欄かを添えて断る(設定は変えない)。
    pub fn confine_settings(&self, group: &Settings, value: &mut Value) -> Result<(), String> {
        for f in group.fields.iter().filter(|f| f.kind == FieldKind::File) {
            let Some(p) = f.get(value).and_then(Value::as_str).map(str::to_string) else {
                continue;
            };
            if p.trim().is_empty() {
                continue;
            }
            let full = self
                .resolve(&p)
                .map_err(|e| format!("設定 {} の {}: {e}", group.id, f.key))?;
            f.set(value, Some(Value::String(full.display().to_string())));
        }
        Ok(())
    }
}

/// 操作が文書のデータから開くファイル(リンクの元など)も、許したフォルダの中に限る。
impl FileGate for Roots {
    fn open(&self, path: &Path) -> Result<PathBuf, String> {
        self.resolve(&path.to_string_lossy())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kmb_core::Arg;
    use serde_json::json;

    /// 確かめること: ファイルの引数だけを作業フォルダの中の絶対パスにし、外を指せば断る。
    /// 文字の引数は触らない。
    #[test]
    fn file_args_are_confined_to_the_allowed_folders() {
        let base = std::env::temp_dir().join(format!("kmb-roots-{}", std::process::id()));
        let (work, other) = (base.join("work"), base.join("other"));
        std::fs::create_dir_all(&work).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        let roots = Roots::new(work.clone());
        let mut action = Action::new("x.paste", "貼る", "");
        action.args = vec![Arg::file("path", "絵"), Arg::text("name", "名前")];

        let mut ok = json!({"path": "a.png", "name": "../b"});
        roots.confine_args(&action, &mut ok).unwrap();
        let root = work.canonicalize().unwrap();
        assert_eq!(ok["path"], json!(root.join("a.png").display().to_string()));
        assert_eq!(ok["name"], json!("../b"));

        let outside = other.join("b.png").display().to_string();
        for p in [outside.as_str(), "../other/b.png"] {
            let e = roots
                .confine_args(&action, &mut json!({ "path": p }))
                .unwrap_err();
            assert!(e.contains("x.paste の path") && e.contains("外"), "{e}");
        }

        let mut both = Roots::new(work.clone());
        both.allow(&other).unwrap();
        let mut v = json!({ "path": outside });
        both.confine_args(&action, &mut v).unwrap();
        let _ = std::fs::remove_dir_all(&base);
    }
}
