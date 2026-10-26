//! 文書ごとの画面の見え方の覚え(ブラウザの木の開け閉め・原点とまとまりの目・行の目・畳んだ
//! タイムラインのグループ)。文書を開き直しても前の見え方に戻すため、データフォルダの
//! `doc-views.json` に文書のパスごとに置く。
//!
//! - 文書のファイルには書かない(開け閉めで「変更あり」にせず、取り消しの履歴にも乗せない)。
//! - 読めなければ空(設定のせいで起動できなくしない)。覚えるのは新しい順に [`DOC_VIEWS_MAX`] 件まで。

use kmb_core::FeatureId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// 覚えておく文書の数(古い物から忘れる)
pub const DOC_VIEWS_MAX: usize = 200;

/// 1 つの文書の見え方。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DocView {
    /// ブラウザの木の枝の開け閉め(枝の鍵 → 開いているか)
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub open: BTreeMap<String, bool>,
    /// 原点の平面をいつも出すか
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub origin: bool,
    /// まとめて隠しているまとまり(画面の名前)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden_folders: Vec<String>,
    /// 目を閉じた機能(自動で隠した物は入れない)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden: Vec<FeatureId>,
    /// 使われて自動で隠れるのに、目を開けて出しているスケッチ
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shown: Vec<FeatureId>,
    /// 目を閉じたボディ(最初に作った機能)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden_bodies: Vec<FeatureId>,
    /// 畳んでいるタイムラインのグループ
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collapsed: Vec<u64>,
}

impl DocView {
    /// 何も覚えていない(既定の見え方)か。
    pub fn is_empty(&self) -> bool {
        *self == DocView::default()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Entry {
    path: PathBuf,
    #[serde(flatten)]
    view: DocView,
}

/// 文書ごとの見え方の一覧(新しい順)。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DocViews {
    #[serde(default)]
    docs: Vec<Entry>,
}

impl DocViews {
    fn file(dir: &Path) -> PathBuf {
        dir.join("doc-views.json")
    }

    /// 読めなければ空。
    pub fn load(dir: &Path) -> DocViews {
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

    /// 文書 `path` の見え方。覚えていなければ `None`。
    pub fn get(&self, path: &Path) -> Option<&DocView> {
        let p = key(path);
        self.docs.iter().find(|e| e.path == p).map(|e| &e.view)
    }

    /// 文書 `path` の見え方を覚える(先頭へ動かす)。既定の見え方なら忘れる。
    pub fn set(&mut self, path: &Path, view: DocView) {
        let p = key(path);
        self.docs.retain(|e| e.path != p);
        if !view.is_empty() {
            self.docs.insert(0, Entry { path: p, view });
            self.docs.truncate(DOC_VIEWS_MAX);
        }
    }
}

fn key(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn views_are_kept_per_document_newest_first_and_forgotten_when_plain() {
        let dir = std::env::temp_dir().join(format!("kmb-doc-views-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let dir = dir.as_path();
        let (a, b) = (dir.join("a.kmb"), dir.join("b.kmb"));
        let mut v = DocViews::load(dir);
        assert!(v.get(&a).is_none(), "無ければ空");
        let open = DocView {
            open: [("\"origin\"".to_string(), true)].into(),
            origin: true,
            hidden: vec![FeatureId(3)],
            collapsed: vec![7],
            ..DocView::default()
        };
        v.set(&a, open.clone());
        v.set(
            &b,
            DocView {
                origin: true,
                ..DocView::default()
            },
        );
        v.save(dir).unwrap();
        let back = DocViews::load(dir);
        assert_eq!(back.get(&a), Some(&open), "読み直しても同じ");
        assert_eq!(back.docs[0].path, key(&b), "新しい順");
        let mut back = back;
        back.set(&a, DocView::default());
        assert!(back.get(&a).is_none(), "既定の見え方は忘れる");
        // 壊れたファイルは空として読む
        std::fs::write(DocViews::file(dir), "{").unwrap();
        assert_eq!(DocViews::load(dir), DocViews::default());
        // 上限を超えたら古い物から忘れる
        let mut many = DocViews::default();
        for i in 0..=DOC_VIEWS_MAX {
            many.set(&dir.join(format!("{i}.kmb")), open.clone());
        }
        assert_eq!(many.docs.len(), DOC_VIEWS_MAX);
        assert!(many.get(&dir.join("0.kmb")).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
