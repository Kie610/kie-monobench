//! プロジェクト: PC のフォルダを 1 つのまとまりとして登録し、中のフォルダとモデル(文書)を探す。
//! クラウドには置かない。中の階層もそのままフォルダ。登録の一覧はデータフォルダの `projects.json`。
//!
//! 将来は、別のファイルを部品として参照する機能(コンポーネント)の「つながり」もここで調べる。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// 登録したプロジェクト(フォルダ)。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Projects {
    pub folders: Vec<PathBuf>,
}

impl Projects {
    fn file(dir: &Path) -> PathBuf {
        dir.join("projects.json")
    }

    /// 読めなければ空。
    pub fn load(dir: &Path) -> Projects {
        std::fs::read_to_string(Self::file(dir))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("{} を作れません: {e}", dir.display()))?;
        let s = serde_json::to_string_pretty(self).expect("JSON にできる");
        crate::write_atomic(&Self::file(dir), &s, false)
    }

    /// 足す(同じものは足さない)。フォルダでなければ理由を返す。
    pub fn add(&mut self, folder: &Path) -> Result<(), String> {
        if !folder.is_dir() {
            return Err(format!("{} はフォルダではありません", folder.display()));
        }
        let p = std::path::absolute(folder).unwrap_or_else(|_| folder.to_path_buf());
        if !self.folders.contains(&p) {
            self.folders.push(p);
        }
        Ok(())
    }

    /// 登録から外す(フォルダは消さない)。
    pub fn remove(&mut self, folder: &Path) {
        self.folders.retain(|f| f != folder);
    }
}

/// フォルダの中身(下のフォルダと、モデル)。
#[derive(Debug, Clone, PartialEq)]
pub struct Folder {
    pub path: PathBuf,
    pub name: String,
    pub folders: Vec<Folder>,
    pub models: Vec<Model>,
}

impl Folder {
    /// 中のモデルの数(下のフォルダも含む)。
    pub fn model_count(&self) -> usize {
        self.models.len() + self.folders.iter().map(Folder::model_count).sum::<usize>()
    }

    /// 中のモデル全部(下のフォルダも。フォルダの順)。
    pub fn all_models(&self) -> Vec<&Model> {
        let mut out: Vec<&Model> = self.models.iter().collect();
        for f in &self.folders {
            out.extend(f.all_models());
        }
        out
    }

    /// `path` のフォルダ(自分か下のもの)。
    pub fn find(&self, path: &Path) -> Option<&Folder> {
        if self.path == path {
            return Some(self);
        }
        self.folders.iter().find_map(|f| f.find(path))
    }
}

/// モデル(文書のファイル)。
#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    pub path: PathBuf,
    pub name: String,
    pub modified: Option<SystemTime>,
    pub bytes: u64,
}

/// モデルの分類と見出し(設計の属性から。モデル管理の検索と絞り込みに使う)。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelInfo {
    pub title: String,
    /// 作品(設計の属性の `project`。フォルダとは別に、関わるデザインをまとめる札)
    pub project: String,
    pub genre: String,
    pub tags: Vec<String>,
    pub part_number: String,
    pub designer: String,
    pub description: String,
    pub material: String,
}

impl ModelInfo {
    /// `query` の語(空白で区切る)が全部、名前・属性・タグのどこかに入っているか
    /// (大文字と小文字・カタカナとひらがな・全角と半角は区別しない。[`fold`])。
    pub fn matches(&self, name: &str, query: &str) -> bool {
        let hay = [
            name,
            &self.title,
            &self.project,
            &self.genre,
            &self.part_number,
            &self.designer,
            &self.description,
            &self.material,
            &self.tags.join(" "),
        ]
        .join("\n");
        let hay = fold(&hay);
        query.split_whitespace().all(|w| hay.contains(&fold(w)))
    }
}

/// 探す時に同じと見なす形にそろえる: 大文字と小文字、カタカナとひらがな、全角と半角の英数字。
pub fn fold(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'ァ'..='ヶ' => char::from_u32(c as u32 - 0x60).unwrap_or(c),
            '！'..='～' => char::from_u32(c as u32 - 0xfee0).unwrap_or(c),
            _ => c,
        })
        .flat_map(char::to_lowercase)
        .collect()
}

/// 文書の設計の属性を読む(読めなければ空)。文書全体を読むので、一覧では控えて使う。
pub fn model_info(path: &Path) -> ModelInfo {
    std::fs::read_to_string(path)
        .map(|s| info_of_json(&s))
        .unwrap_or_default()
}

/// 文書の JSON から設計の属性を読む(読めなければ空)。
pub fn info_of_json(json: &str) -> ModelInfo {
    #[derive(Deserialize)]
    struct Head {
        #[serde(default)]
        properties: kmb_core::Properties,
    }
    let Some(p) = serde_json::from_str::<Head>(json)
        .ok()
        .map(|h| h.properties)
    else {
        return ModelInfo::default();
    };
    ModelInfo {
        title: p.title,
        project: p.project,
        genre: p.genre,
        tags: p.tags,
        part_number: p.part_number,
        designer: p.designer,
        description: p.description,
        material: p.material,
    }
}

/// 探さないフォルダ(隠しフォルダ、ビルドの出力、自動保存の写し)。
fn skipped(name: &str) -> bool {
    name.starts_with('.') || matches!(name, "target" | "node_modules" | "recovery")
}

/// 文書のファイルか(JSON で、最初の方に機能の一覧と保存形式の版がある)。
pub fn is_document(path: &Path) -> bool {
    let ext = path.extension().and_then(|x| x.to_str());
    if !ext.is_some_and(|x| kmb_core::DOCUMENT_EXTENSIONS.contains(&x)) {
        return false;
    }
    use std::io::Read;
    let Ok(f) = std::fs::File::open(path) else {
        return false;
    };
    let mut head = String::new();
    // 大きな文書でも、版と機能の一覧は先頭の方にある
    let _ = f.take(64 * 1024).read_to_string(&mut head);
    head.trim_start().starts_with('{')
        && head.contains("\"format_version\"")
        && head.contains("\"features\"")
}

/// フォルダ `root` の中を、深さ `depth` まで探す。
pub fn scan(root: &Path, depth: usize) -> Folder {
    let name = root.file_name().map_or(root.display().to_string(), |n| {
        n.to_string_lossy().to_string()
    });
    let mut out = Folder {
        path: root.to_path_buf(),
        name,
        folders: Vec::new(),
        models: Vec::new(),
    };
    let Ok(rd) = std::fs::read_dir(root) else {
        return out;
    };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        let n = e.file_name().to_string_lossy().to_string();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            if depth > 0 && !skipped(&n) {
                out.folders.push(scan(&p, depth - 1));
            }
        } else if ft.is_file() && is_document(&p) {
            let meta = e.metadata().ok();
            out.models.push(Model {
                name: p
                    .file_stem()
                    .map_or(n.clone(), |s| s.to_string_lossy().to_string()),
                modified: meta.as_ref().and_then(|m| m.modified().ok()),
                bytes: meta.map_or(0, |m| m.len()),
                path: p,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projects_are_folders_with_models_found_inside() {
        let base = std::env::temp_dir().join(format!("kmb-projects-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let proj = base.join("棚");
        std::fs::create_dir_all(proj.join("部品").join("小物")).unwrap();
        std::fs::create_dir_all(proj.join(".git")).unwrap();
        let doc = kmb_core::Document::new().to_json();
        std::fs::write(proj.join("天板.json"), &doc).unwrap();
        std::fs::write(proj.join("部品").join("脚.json"), &doc).unwrap();
        std::fs::write(proj.join("部品").join("メモ.json"), "{\"a\": 1}").unwrap();
        std::fs::write(proj.join(".git").join("x.json"), &doc).unwrap();
        let f = scan(&proj, 4);
        assert_eq!(f.models.len(), 1);
        assert_eq!(f.models[0].name, "天板");
        assert_eq!(f.folders.len(), 1, "隠しフォルダは探さない");
        let parts = &f.folders[0];
        assert_eq!(parts.name, "部品");
        assert_eq!(parts.models.len(), 1, "文書でない JSON は出さない");
        assert_eq!(f.model_count(), 2);
        assert_eq!(f.all_models().len(), 2);
        // 設計の属性の分類で探せる
        let mut d = kmb_core::Document::new();
        let mut props = kmb_core::Properties::default();
        props.set("title", "棚の天板");
        props.set("genre", "家具");
        props.set("tags", "ナラ, 塗装");
        d.apply(kmb_core::Transaction::new(
            "属性",
            vec![kmb_core::Op::SetProperties {
                properties: Box::new(props),
            }],
        ))
        .unwrap();
        std::fs::write(proj.join("天板.json"), d.to_json()).unwrap();
        let info = model_info(&proj.join("天板.json"));
        assert_eq!(info.genre, "家具");
        assert_eq!(info.tags, vec!["ナラ", "塗装"]);
        assert!(info.matches("天板", "家具 なら"));
        assert!(info.matches("天板", "天板"));
        assert!(!info.matches("天板", "家具 アルミ"));
        assert_eq!(model_info(&proj.join("無い.json")), ModelInfo::default());
        assert!(f.find(&proj.join("部品").join("小物")).is_some());
        // 登録はデータフォルダに残る
        let data = base.join("data");
        let mut p = Projects::default();
        p.add(&proj).unwrap();
        p.add(&proj).unwrap();
        assert!(p.add(&proj.join("天板.json")).is_err());
        assert_eq!(p.folders.len(), 1);
        p.save(&data).unwrap();
        assert_eq!(Projects::load(&data), p);
        p.remove(&p.folders[0].clone());
        assert!(p.folders.is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }
}
