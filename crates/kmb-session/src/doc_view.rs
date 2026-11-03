//! 文書ごとの画面の見え方の覚え(ブラウザの木の開け閉め・畳んだタイムラインのグループ)。目の開け閉めは
//! 文書の区画 `cad.visibility` に保存するので、ここには置かない。文書を開き直しても前の見え方に戻すため、データフォルダの
//! `doc-views.json` に文書のパスごとに置く。
//!
//! - 文書のファイルには書かない(開け閉めで「変更あり」にせず、取り消しの履歴にも乗せない)。
//! - 読めなければ空(設定のせいで起動できなくしない)。覚えるのは新しい順に [`DOC_VIEWS_MAX`] 件まで。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// 覚えておく文書の数(古い物から忘れる)
pub const DOC_VIEWS_MAX: usize = 200;

/// 部品ごとの色を切り替える、文書を変更しない表示操作。
pub const PART_HUES: &str = "view.part_hues";

/// 画面・CLI・MCP が共有する表示操作の宣言。
pub fn actions() -> Vec<kmb_core::Action> {
    vec![
        kmb_core::Action::new(PART_HUES, "部品ごとに色を変える", "view")
            .arg(kmb_core::Arg::flag("on", "入れるか(省くと切り替える)"))
            .aliases("ぶひん いろ 色 component part colors hues")
            .hint("外観や材料の無いボディを、部品ごとの色で見分ける。文書ごとに見え方を覚える"),
    ]
}

pub fn is_action(id: &str) -> bool {
    id == PART_HUES
}

/// 1 つの文書の見え方。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DocView {
    /// ブラウザの木の枝の開け閉め(枝の鍵 → 開いているか)
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub open: BTreeMap<String, bool>,
    /// 畳んでいるタイムラインのグループ
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collapsed: Vec<u64>,
    /// 外観や材料を持たないボディを、部品ごとの色で見る。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub part_hues: bool,
    /// 知らない見え方の項目も、同じ文書へ書き戻す。
    #[serde(flatten)]
    pub rest: BTreeMap<String, serde_json::Value>,
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
    #[serde(flatten)]
    rest: BTreeMap<String, serde_json::Value>,
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
    fn unknown_view_fields_are_preserved() {
        let source = serde_json::json!({
            "future_root": {"keep": [1, 2]},
            "docs": [{"path":"/tmp/part-colors.kmb", "future_entry":{"keep":true}}]
        });
        let views: DocViews = serde_json::from_value(source.clone()).unwrap();
        let back = serde_json::to_value(views).unwrap();
        assert_eq!(back["future_root"], source["future_root"]);
        assert_eq!(
            back["docs"][0]["future_entry"],
            source["docs"][0]["future_entry"]
        );
    }

    #[test]
    fn plain_views_default_off_and_do_not_absorb_the_entry_path() {
        let path = key(&std::env::temp_dir().join("plain-view.kmb"));
        let views: DocViews = serde_json::from_value(serde_json::json!({
            "docs": [{"path":path, "open":{"branch":true}, "future":7}]
        }))
        .unwrap();
        let view = views.get(&path).unwrap();
        assert!(!view.part_hues);
        assert_eq!(view.rest["future"], 7);
        assert!(!view.rest.contains_key("path"));
        let back = serde_json::to_value(&views).unwrap();
        assert!(back["docs"][0].get("part_hues").is_none());
        assert_eq!(back["docs"][0]["path"], serde_json::json!(path));
    }

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
            collapsed: vec![7],
            ..Default::default()
        };
        v.set(&a, open.clone());
        v.set(
            &b,
            DocView {
                collapsed: vec![1],
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
