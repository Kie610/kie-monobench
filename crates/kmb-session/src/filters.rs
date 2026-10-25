//! モデル管理の絞り込み(探す語・ジャンル・作品・タグ・変更日)と、名前を付けて保存した絞り込み
//! (ホームの左の「保存した絞り込み」。Eagle のスマートフォルダと同じ考え)。
//! 画面・CLI・MCP が同じ判定を使う。保存した絞り込みはデータフォルダの `filters.json` に置く。
//!
//! 形: `{"filters":[{"name":"家具の塗装","query":"","genre":"家具","project":null,"tags":["塗装"],"period":"all"}]}`

use crate::library::Period;
use crate::projects::{ModelInfo, fold};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// 絞り込み 1 つ(全部に合う物だけ出す)。保存した物は名前を持つ。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Filter {
    /// 保存した時の名前(今の絞り込みでは空)
    #[serde(default)]
    pub name: String,
    /// 探す語(空白で区切ると、全部を含む物)
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub genre: Option<String>,
    /// 作品(設計の属性の `project`)
    #[serde(default)]
    pub project: Option<String>,
    /// タグ(全部を持つ物)
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub period: Period,
}

impl Filter {
    /// 探す語だけの絞り込み。
    pub fn query(q: &str) -> Self {
        Self {
            query: q.trim().to_string(),
            ..Self::default()
        }
    }

    /// 探す語・ジャンル・作品・タグのどれも無いか(変更日は見ない)。
    pub fn is_empty(&self) -> bool {
        self.query.trim().is_empty()
            && self.genre.is_none()
            && self.project.is_none()
            && self.tags.is_empty()
    }

    /// 名前と分類が合うか(分類をまだ読んでいない物は名前だけで。変更日は [`Filter::passes_at`])。
    pub fn passes(&self, name: &str, info: Option<&ModelInfo>) -> bool {
        match info {
            Some(i) => {
                i.matches(name, &self.query)
                    && self.genre.as_ref().is_none_or(|g| *g == i.genre)
                    && self.project.as_ref().is_none_or(|g| *g == i.project)
                    && self.tags.iter().all(|t| i.tags.contains(t))
            }
            None => {
                self.genre.is_none()
                    && self.project.is_none()
                    && self.tags.is_empty()
                    && self
                        .query
                        .split_whitespace()
                        .all(|w| fold(name).contains(&fold(w)))
            }
        }
    }

    /// 名前・分類・変更日が合うか。
    pub fn passes_at(
        &self,
        name: &str,
        info: Option<&ModelInfo>,
        modified: Option<SystemTime>,
        now: SystemTime,
    ) -> bool {
        (self.period == Period::All || self.period.contains(modified, now))
            && self.passes(name, info)
    }

    /// 人が読む説明(例: 「ジャンル: 家具・#塗装・今月」)。
    pub fn describe(&self) -> String {
        let mut v = Vec::new();
        if !self.query.trim().is_empty() {
            v.push(format!("「{}」", self.query.trim()));
        }
        if let Some(g) = &self.genre {
            v.push(format!("ジャンル: {g}"));
        }
        if let Some(g) = &self.project {
            v.push(format!("作品: {g}"));
        }
        v.extend(self.tags.iter().map(|t| format!("#{t}")));
        if self.period != Period::All {
            v.push(self.period.label().to_string());
        }
        if v.is_empty() {
            "すべて".into()
        } else {
            v.join("・")
        }
    }
}

/// 保存した絞り込み(左に並べる順)。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    #[serde(default)]
    pub filters: Vec<Filter>,
}

impl Saved {
    fn file(dir: &Path) -> PathBuf {
        dir.join("filters.json")
    }

    /// データフォルダ `dir` から読む(無い・読めない時は空。読めない時は元を残してログに出す)。
    pub fn load(dir: &Path) -> Saved {
        let f = Self::file(dir);
        let Ok(s) = std::fs::read_to_string(&f) else {
            return Saved::default();
        };
        serde_json::from_str(&s).unwrap_or_else(|e| {
            log::warn!(
                "保存した絞り込みを読めない(空で始める): {}: {e}",
                f.display()
            );
            Saved::default()
        })
    }

    pub fn save(&self, dir: &Path) -> Result<(), String> {
        let s = serde_json::to_string_pretty(self).expect("JSON にできる");
        crate::write_atomic(&Self::file(dir), &(s + "\n"), false)
    }

    /// 名前を付けて足す(同じ名前があれば置き換える)。名前が空なら誤り。
    pub fn put(&mut self, f: Filter) -> Result<(), String> {
        if f.name.trim().is_empty() {
            return Err("絞り込みの名前が空です".into());
        }
        let f = Filter {
            name: f.name.trim().to_string(),
            ..f
        };
        match self.filters.iter_mut().find(|x| x.name == f.name) {
            Some(x) => *x = f,
            None => self.filters.push(f),
        }
        Ok(())
    }

    pub fn remove(&mut self, name: &str) {
        self.filters.retain(|f| f.name != name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_filters_round_trip_and_match_like_the_home_filter() {
        let dir = std::env::temp_dir().join(format!("kmb-filters-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(Saved::load(&dir), Saved::default());
        let mut s = Saved::default();
        assert!(s.put(Filter::default()).is_err(), "名前が要る");
        let f = Filter {
            name: " 家具の塗装 ".into(),
            genre: Some("家具".into()),
            tags: vec!["塗装".into()],
            period: Period::Month,
            ..Filter::default()
        };
        s.put(f.clone()).unwrap();
        s.put(Filter {
            name: "家具の塗装".into(),
            ..f.clone()
        })
        .unwrap();
        assert_eq!(s.filters.len(), 1, "同じ名前は置き換える");
        assert_eq!(s.filters[0].name, "家具の塗装");
        s.save(&dir).unwrap();
        assert_eq!(Saved::load(&dir), s);
        assert_eq!(s.filters[0].describe(), "ジャンル: 家具・#塗装・今月");
        let info = ModelInfo {
            genre: "家具".into(),
            tags: vec!["塗装".into(), "ナラ".into()],
            ..ModelInfo::default()
        };
        let now = SystemTime::now();
        assert!(f.passes_at("天板", Some(&info), Some(now), now));
        assert!(
            !f.passes_at("天板", Some(&info), None, now),
            "変更日が分からない物は外す"
        );
        assert!(
            !f.passes("天板", None),
            "分類を読む前は分類の絞り込みに合わない"
        );
        assert!(Filter::query("てん").passes("テン板", None));
        // 読めない時は空で始め、元は残す
        std::fs::write(dir.join("filters.json"), "{壊れた").unwrap();
        assert_eq!(Saved::load(&dir), Saved::default());
        assert!(dir.join("filters.json").exists());
        s.remove("家具の塗装");
        assert!(s.filters.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
