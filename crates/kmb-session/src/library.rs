//! モデル管理(ホームとデータパネル)の中身: デザイン 1 つの概要、フォルダの概要、変更日の絞り込み。
//! 画面・CLI・MCP が同じ関数を使う。読むだけで、ファイルを変えない(変えるのは [`crate::organize`])。
//!
//! 概要は文書の JSON と版の履歴の一覧を読むので、画面は選んだ物だけ読み、控えて使う。

use crate::history::{self, Version};
use crate::projects::{Folder, ModelInfo, model_info};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// デザイン(文書のファイル)1 つの概要。右の概要の欄に出す。
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub path: PathBuf,
    /// 拡張子を除いたファイル名
    pub name: String,
    /// 置いてあるフォルダ
    pub folder: PathBuf,
    /// ファイルがあるか(最近の一覧に残っていて、消された・動かされた物は偽)
    pub exists: bool,
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
    /// ファイルの大きさ(バイト)
    pub bytes: u64,
    /// 版の履歴の大きさ(バイト。`.jc/history/<名前>/` の中の合計)
    pub history_bytes: u64,
    /// 版(新しい順)
    pub versions: Vec<Version>,
    /// 分類と見出し(設計の属性)
    pub info: ModelInfo,
}

impl Summary {
    /// いちばん新しい版の番号(版が無ければ無し)。カードの「vN」に出す。
    pub fn latest(&self) -> Option<u32> {
        self.versions.first().map(|v| v.number)
    }
}

/// `path` の概要を読む。ファイルが無くても名前と場所だけの概要を返す。
pub fn summary(path: &Path) -> Summary {
    let meta = std::fs::metadata(path).ok().filter(|m| m.is_file());
    let versions = if meta.is_some() {
        history::list(path)
    } else {
        Vec::new()
    };
    // 保存は書き直し(別のファイルに書いて置き換える)なので、ファイルの作成日は最後の保存の日に
    // なる。版があれば最初の版の日を作成日にする
    let first = versions
        .last()
        .map(|v| SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(v.saved_at));
    let created = match (first, meta.as_ref().and_then(|m| m.created().ok())) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    Summary {
        path: path.to_path_buf(),
        name: path.file_stem().map_or_else(
            || path.display().to_string(),
            |s| s.to_string_lossy().to_string(),
        ),
        folder: path.parent().map(Path::to_path_buf).unwrap_or_default(),
        exists: meta.is_some(),
        modified: meta.as_ref().and_then(|m| m.modified().ok()),
        created,
        bytes: meta.as_ref().map_or(0, |m| m.len()),
        history_bytes: dir_bytes(&history::dir(path)),
        versions,
        info: if meta.is_some() {
            model_info(path)
        } else {
            ModelInfo::default()
        },
    }
}

/// フォルダ(プロジェクトとその中のフォルダ)の概要。右の概要の欄に出す。数は下のフォルダも含む。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FolderSummary {
    pub designs: usize,
    pub folders: usize,
    /// デザインのファイルの大きさの合計(版の履歴は含まない)
    pub bytes: u64,
    /// いちばん新しい変更日
    pub modified: Option<SystemTime>,
    /// 最近変えたデザイン(新しい順に 3 つまで)
    pub recent: Vec<PathBuf>,
    /// ジャンルの内訳(多い順。同じ数は名前の順)
    pub genres: Vec<(String, usize)>,
    /// よく付いているタグ(多い順に 8 つまで)
    pub tags: Vec<(String, usize)>,
}

/// フォルダ `f` の概要。分類は `infos`(デザインのパスから。無い物は分類なし)から数える。
pub fn folder_summary(f: &Folder, infos: &HashMap<PathBuf, ModelInfo>) -> FolderSummary {
    fn count_folders(f: &Folder) -> usize {
        f.folders.len() + f.folders.iter().map(count_folders).sum::<usize>()
    }
    let models = f.all_models();
    let mut by_time: Vec<_> = models.iter().filter(|m| m.modified.is_some()).collect();
    by_time.sort_by(|a, b| b.modified.cmp(&a.modified).then(a.path.cmp(&b.path)));
    let mut genres: HashMap<&str, usize> = HashMap::new();
    let mut tags: HashMap<&str, usize> = HashMap::new();
    for m in &models {
        if let Some(i) = infos.get(&m.path) {
            if !i.genre.is_empty() {
                *genres.entry(&i.genre).or_default() += 1;
            }
            for t in &i.tags {
                *tags.entry(t).or_default() += 1;
            }
        }
    }
    let ranked = |m: HashMap<&str, usize>, n: usize| {
        let mut v: Vec<(String, usize)> = m.into_iter().map(|(k, c)| (k.to_string(), c)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v.truncate(n);
        v
    };
    FolderSummary {
        designs: models.len(),
        folders: count_folders(f),
        bytes: models.iter().map(|m| m.bytes).sum(),
        modified: by_time.first().and_then(|m| m.modified),
        recent: by_time.iter().take(3).map(|m| m.path.clone()).collect(),
        genres: ranked(genres, usize::MAX),
        tags: ranked(tags, 8),
    }
}

/// フォルダの中のファイルの大きさの合計(下のフォルダも。読めない物は 0)。
fn dir_bytes(dir: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return 0;
    };
    rd.flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_bytes(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map_or(0, |m| m.len()),
            _ => 0,
        })
        .sum()
}

/// 大きさを人が読む形にする(1024 で割る。例: 「12.3 KB」)。
pub fn bytes_label(b: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if b < 1024 {
        return format!("{b} B");
    }
    let mut v = b as f64 / 1024.0;
    let mut u = 0;
    while v >= 1024.0 && u + 1 < UNITS.len() {
        v /= 1024.0;
        u += 1;
    }
    if v < 10.0 {
        format!("{v:.1} {}", UNITS[u])
    } else {
        format!("{v:.0} {}", UNITS[u])
    }
}

/// 日時をこの PC の時刻で短く書く(例: 「2026-10-22 10:05」)。
pub fn date_label(t: SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(t)
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

/// 経った時間を短く書く(「たった今」「3 分前」「2 時間前」「5 日前」)。
pub fn ago_label(t: SystemTime, now: SystemTime) -> String {
    let m = now.duration_since(t).map_or(0, |d| d.as_secs()) / 60;
    match m {
        0 => "たった今".into(),
        1..=59 => format!("{m} 分前"),
        60..=1439 => format!("{} 時間前", m / 60),
        _ => format!("{} 日前", m / 1440),
    }
}

/// 変更日の絞り込み(ホームの中央の上のチップ)。日・週・月の境はこの PC の時刻で数える。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Period {
    #[default]
    All,
    /// 今日の 0 時から
    Today,
    /// 今週の月曜の 0 時から
    Week,
    /// 今月の 1 日の 0 時から
    Month,
    /// 3 か月前の同じ日の 0 時から
    Quarter,
}

impl Period {
    pub const ALL: [Period; 5] = [
        Period::All,
        Period::Today,
        Period::Week,
        Period::Month,
        Period::Quarter,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Period::All => "すべて",
            Period::Today => "今日",
            Period::Week => "今週",
            Period::Month => "今月",
            Period::Quarter => "過去 3 か月",
        }
    }

    /// `modified` がこの期間に入るか。変更日の分からない物は「すべて」の時だけ入る。
    pub fn contains(self, modified: Option<SystemTime>, now: SystemTime) -> bool {
        let local = chrono::DateTime::<chrono::Local>::from(now);
        self.contains_at(modified, local.fixed_offset())
    }

    /// [`Period::contains`] の中身(時差を決めて試せるように、今の時刻を時差つきで受ける)。
    fn contains_at(
        self,
        modified: Option<SystemTime>,
        now: chrono::DateTime<chrono::FixedOffset>,
    ) -> bool {
        use chrono::{Datelike, Days, Months, NaiveTime};
        if self == Period::All {
            return true;
        }
        let Some(m) = modified else {
            return false;
        };
        let today = now.date_naive();
        let from = match self {
            Period::All => return true,
            Period::Today => Some(today),
            Period::Week => {
                today.checked_sub_days(Days::new(u64::from(today.weekday().num_days_from_monday())))
            }
            Period::Month => today.with_day(1),
            Period::Quarter => today.checked_sub_months(Months::new(3)),
        };
        let Some(start) = from
            .map(|d| d.and_time(NaiveTime::MIN))
            .and_then(|d| d.and_local_timezone(*now.offset()).single())
        else {
            return true;
        };
        chrono::DateTime::<chrono::Utc>::from(m) >= start
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn summary_reads_size_versions_and_properties() {
        let base = std::env::temp_dir().join(format!("kmb-library-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let path = base.join("台座.json");
        let mut d = kmb_core::Document::new();
        let mut props = kmb_core::Properties::default();
        props.set("title", "台座");
        props.set("project", "棚");
        props.set("material", "A5052");
        d.apply(kmb_core::Transaction::new(
            "属性",
            vec![kmb_core::Op::SetProperties {
                properties: Box::new(props),
            }],
        ))
        .unwrap();
        crate::save(&path, &d).unwrap();
        history::record(&path, &d, &Default::default(), "保存").unwrap();
        let s = summary(&path);
        assert!(s.exists);
        assert_eq!(s.name, "台座");
        assert_eq!(s.folder, base);
        assert_eq!(s.latest(), Some(1));
        let first = SystemTime::UNIX_EPOCH + Duration::from_secs(s.versions[0].saved_at);
        assert!(
            s.created.is_some_and(|c| c <= first),
            "作成日は最初の版より後にならない"
        );
        assert_eq!(s.info.project, "棚");
        assert_eq!(s.info.material, "A5052");
        assert!(s.bytes > 0);
        assert!(s.history_bytes > 0, "版の中身と一覧の分");
        // 無いファイルは名前と場所だけ
        let gone = summary(&base.join("無い.json"));
        assert!(!gone.exists);
        assert_eq!(gone.name, "無い");
        assert!(gone.versions.is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn bytes_are_shown_in_binary_units() {
        assert_eq!(bytes_label(512), "512 B");
        assert_eq!(bytes_label(1536), "1.5 KB");
        assert_eq!(bytes_label(20 * 1024 * 1024), "20 MB");
        let now = SystemTime::now();
        assert_eq!(ago_label(now, now), "たった今");
        assert_eq!(
            ago_label(now - std::time::Duration::from_secs(7200), now),
            "2 時間前"
        );
        assert_eq!(date_label(now).len(), "2026-10-22 10:05".len());
    }

    #[test]
    fn periods_count_from_local_calendar_boundaries() {
        use chrono::TimeZone;
        // 2026-10-22(木)10:00、時差 +9 時間
        let jst = chrono::FixedOffset::east_opt(9 * 3600).unwrap();
        let now = jst.with_ymd_and_hms(2026, 10, 22, 10, 0, 0).unwrap();
        let at = |y, mo, d, h| {
            Some(SystemTime::from(
                jst.with_ymd_and_hms(y, mo, d, h, 0, 0).unwrap(),
            ))
        };
        let p = |p: Period, m| p.contains_at(m, now);
        assert!(p(Period::Today, at(2026, 10, 22, 0)));
        assert!(
            !p(Period::Today, at(2026, 10, 21, 23)),
            "昨日の夜は今日に入らない"
        );
        assert!(p(Period::Week, at(2026, 10, 19, 0)), "月曜から");
        assert!(!p(Period::Week, at(2026, 10, 18, 23)));
        assert!(p(Period::Month, at(2026, 10, 1, 0)));
        assert!(!p(Period::Month, at(2026, 9, 30, 23)));
        assert!(p(Period::Quarter, at(2026, 7, 22, 0)));
        assert!(!p(Period::Quarter, at(2026, 7, 21, 23)));
        assert!(p(Period::All, None));
        assert!(
            !p(Period::Today, None),
            "変更日の分からない物は絞ると出ない"
        );
        // 今の PC の時刻で数える入口
        let now = SystemTime::now();
        assert!(Period::Today.contains(Some(now - Duration::from_secs(1)), now));
    }

    #[test]
    fn folder_summary_counts_inside_and_ranks_genres_and_tags() {
        use crate::projects::Model;
        let t = |s: u64| Some(SystemTime::UNIX_EPOCH + Duration::from_secs(s));
        let m = |p: &str, at: u64, b: u64| Model {
            path: PathBuf::from(p),
            name: p.into(),
            modified: t(at),
            bytes: b,
        };
        let f = Folder {
            path: "棚".into(),
            name: "棚".into(),
            folders: vec![Folder {
                path: "棚/部品".into(),
                name: "部品".into(),
                folders: vec![],
                models: vec![m("脚", 30, 10), m("棚板", 20, 20)],
            }],
            models: vec![m("天板", 10, 5), m("台座", 40, 1)],
        };
        let info = |g: &str, tags: &[&str]| ModelInfo {
            genre: g.into(),
            tags: tags.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        };
        let infos = HashMap::from([
            (PathBuf::from("脚"), info("家具", &["木"])),
            (PathBuf::from("棚板"), info("家具", &["木", "塗装"])),
            (PathBuf::from("天板"), info("治具", &[])),
        ]);
        let s = folder_summary(&f, &infos);
        assert_eq!((s.designs, s.folders, s.bytes), (4, 1, 36));
        assert_eq!(s.modified, t(40));
        assert_eq!(
            s.recent,
            vec![PathBuf::from("台座"), "脚".into(), "棚板".into()]
        );
        assert_eq!(s.genres, vec![("家具".into(), 2), ("治具".into(), 1)]);
        assert_eq!(s.tags, vec![("木".into(), 2), ("塗装".into(), 1)]);
    }
}
